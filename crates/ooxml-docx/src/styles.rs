//! Existing styles by id (ADR-0007 §5), the read-only style-chain walk of
//! §4 (inherited superseding attributes), and the `updateFields` opt-in
//! (§6.3).

use std::collections::BTreeSet;

use ooxml_core::{ElementSpec, QName, RawNode, XmlDocument};
use ooxml_schema::wml::{NS_W, Settings, UpdateFieldsOnOpen};

use crate::document::Document;
use crate::error::{Error, Result};
use crate::format::StyleTarget;
use crate::story::{ParaRef, RunRef};

const fn w(local: &'static str) -> QName {
    QName::new(NS_W, local)
}

/// The kind of a style (`w:style/@w:type`; absent means paragraph *(u)*).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum StyleKind {
    /// `paragraph` (or no `w:type`)
    Paragraph,
    /// `character`
    Character,
    /// `table`
    Table,
    /// `numbering`
    Numbering,
    /// any other value
    Other,
}

impl StyleKind {
    pub(crate) fn of(t: Option<&str>) -> Self {
        match t {
            None | Some("paragraph") => Self::Paragraph,
            Some("character") => Self::Character,
            Some("table") => Self::Table,
            Some("numbering") => Self::Numbering,
            Some(_) => Self::Other,
        }
    }
}

/// One `w:style` of the styles part, read-only.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct StyleInfo<'d> {
    /// `w:styleId` (empty if absent).
    pub id: &'d str,
    /// `w:type`.
    pub kind: StyleKind,
    /// `w:name/@w:val`: the built-in English name for built-in styles
    /// (`heading 1`), whatever the UI language.
    pub name: Option<&'d str>,
    /// `w:default` is on.
    pub is_default: bool,
    /// `w:link/@w:val`: the linked paragraph or character style.
    pub linked: Option<&'d str>,
}

fn val<'a>(n: &'a RawNode, local: &'static str) -> Option<&'a str> {
    n.elements().find(|e| e.is(w(local))).and_then(|e| e.attr(w("val")))
}

fn style_info(s: &RawNode) -> StyleInfo<'_> {
    StyleInfo {
        id: s.attr(w("styleId")).unwrap_or(""),
        kind: StyleKind::of(s.attr(w("type"))),
        name: val(s, "name"),
        is_default: s.attr(w("default")).is_some_and(|v| ooxml_core::value::parse_on_off(v) == Some(true)),
        linked: val(s, "link"),
    }
}

/// Unicode default lower-casing of each `char` (ADR-0007 §5, review m10).
fn fold(s: &str) -> String {
    s.chars().flat_map(char::to_lowercase).collect()
}

impl Document {
    /// The parsed styles part's root (`w:styles`), if the package has one.
    pub(crate) fn styles_root(&self) -> Option<&RawNode> {
        let root = self.pkg.parsed_part(self.styles_part.as_deref()?)?.root();
        root.is(w("styles")).then_some(root)
    }

    pub(crate) fn style_nodes(&self) -> impl Iterator<Item = &RawNode> {
        self.styles_root().into_iter().flat_map(|r| r.elements().filter(|e| e.is(w("style"))))
    }

    /// Every `w:style` of the styles part, in document order (ADR-0007 §5).
    /// Empty without a styles part.
    pub fn styles(&self) -> impl Iterator<Item = StyleInfo<'_>> {
        self.style_nodes().map(style_info)
    }

    /// The ids of every style whose `w:name` equals `name`, ignoring case
    /// (Unicode lower-casing per `char`). The portable lookup: localized
    /// Word derives ids from the UI name (`Rubrik1`), but keeps the English
    /// `w:name` of built-in styles.
    pub fn style_ids_by_name(&self, name: &str) -> Vec<&str> {
        let want = fold(name);
        self.styles().filter(|s| s.name.is_some_and(|n| fold(n) == want)).map(|s| s.id).collect()
    }

    /// Finds the style a setter refers to: exactly one `w:style` with this
    /// id (case-insensitive duplicates refuse), of the right type.
    pub(crate) fn resolve_style(&self, id: &str, expected: StyleKind) -> Result<()> {
        let folded = fold(id);
        let same: Vec<StyleInfo<'_>> = self.styles().filter(|s| fold(s.id) == folded).collect();
        if same.len() > 1 {
            return Err(Error::AmbiguousStyle { id: id.to_owned() });
        }
        let Some(s) = same.first().filter(|s| s.id == id) else {
            let latent_only = same.is_empty() && self.styles_root().is_some_and(|r| {
                let key: String = folded.chars().filter(|c| !c.is_whitespace()).collect();
                r.elements()
                    .filter(|e| e.is(w("latentStyles")))
                    .flat_map(|l| l.elements().filter(|e| e.is(w("lsdException"))))
                    .filter_map(|e| e.attr(w("name")))
                    .any(|n| fold(n).chars().filter(|c| !c.is_whitespace()).collect::<String>() == key)
            });
            return Err(Error::UnknownStyle { id: id.to_owned(), latent_only });
        };
        if s.kind != expected {
            let found = self
                .style_nodes()
                .find(|n| n.attr(w("styleId")) == Some(id))
                .and_then(|n| n.attr(w("type")))
                .unwrap_or("paragraph")
                .to_owned();
            return Err(Error::StyleTypeMismatch {
                id: id.to_owned(),
                expected: if expected == StyleKind::Paragraph { "paragraph" } else { "character" },
                found,
                linked: s.linked.map(str::to_owned),
            });
        }
        Ok(())
    }

    fn set_style(&mut self, target: StyleTarget, id: Option<&str>, expected: StyleKind) -> Result<()> {
        self.write_gate()?; // the document-wide W2 (ADR-0007 §6.1: no style column)
        if self.styles_part.is_some() && self.styles_root().is_none() {
            // present but unreadable: report the parse error
            let name = self.styles_part.clone().expect("checked");
            XmlDocument::parse(&self.pkg.read_part(&name)?)?;
        }
        if self.styles_ambiguous {
            return Err(Error::AmbiguousStylesPart);
        }
        // the target first, also for a no-op (review of PR #59, M1)
        match target {
            StyleTarget::Para(p) => self.check_formattable_paragraph(p)?,
            StyleTarget::Run(r) => self.check_formattable_run(r)?,
        }
        if let Some(id) = id {
            if id.is_empty() {
                return Err(Error::InvalidValue { what: "style id", value: String::new() });
            }
            self.resolve_style(id, expected)?;
        }
        match self.plan_style_ref(target, id)? {
            Some(plan) => self.write_style_ref(target, plan),
            None => Ok(()),
        }
    }

    /// Writes `w:pStyle` (`Some(id)`) or removes it (`None`: the default
    /// paragraph style applies) on a paragraph (ADR-0007 §5). Direct
    /// formatting is not cleared. Writing the id the paragraph already has
    /// is a no-op.
    ///
    /// Errors, before any change: [`Self::write_gate`] (the document-wide
    /// W2); [`Error::InvalidValue`] for an empty id; [`Error::UnknownStyle`], [`Error::StyleTypeMismatch`],
    /// [`Error::AmbiguousStyle`]; the paragraph targets and dry run of
    /// [`Self::format_paragraph`].
    pub fn set_paragraph_style(&mut self, p: ParaRef, id: Option<&str>) -> Result<()> {
        self.set_style(StyleTarget::Para(p), id, StyleKind::Paragraph)
    }

    /// Writes `w:rStyle` (a `character` style) or removes it, on a run. See
    /// [`Self::set_paragraph_style`]; the run targets are those of
    /// [`Self::format_run`].
    pub fn set_run_style(&mut self, run: RunRef, id: Option<&str>) -> Result<()> {
        self.set_style(StyleTarget::Run(run), id, StyleKind::Character)
    }

    /// The value of `w:<element>/@w:<attr>` resolved along a paragraph's
    /// style chain: the style `style` (or the default paragraph style), its
    /// `w:basedOn` ancestors, then `w:docDefaults/w:pPrDefault`. Most
    /// specific level first; read-only; not the Phase3 resolver (ADR-0007
    /// §4, "inherited superseding attributes").
    pub(crate) fn inherited_ppr_attr(&self, style: Option<&str>, element: &'static str, attr: &'static str) -> Result<Option<String>> {
        self.inherited_paragraph_value(style, |ppr| {
            ppr.elements().find(|e| e.is(w(element)))?.attr(w(attr)).map(str::to_owned)
        })
    }

    fn inherited_paragraph_value(&self, style: Option<&str>, value: impl Fn(&RawNode) -> Option<String>) -> Result<Option<String>> {
        let Some(root) = self.styles_root() else { return Ok(None) };
        let from_ppr = |n: &RawNode| value(n.elements().find(|e| e.is(w("pPr")))?);
        let by_id = |id: &str| self.style_nodes().find(|s| s.attr(w("styleId")) == Some(id));
        let default = || {
            self.style_nodes().find(|s| {
                let i = style_info(s);
                i.kind == StyleKind::Paragraph && i.is_default
            })
        };
        // an undefined pStyle falls back to the default paragraph style, as Word
        // applies it (review of PR #59, m1)
        let mut cur = style.and_then(by_id).or_else(default);
        let mut seen = BTreeSet::new();
        while let Some(s) = cur {
            let id = s.attr(w("styleId")).unwrap_or("");
            if !seen.insert(id.to_owned()) || seen.len() > 32 {
                return Err(Error::StyleChainTooDeep);
            }
            if let Some(v) = from_ppr(s) {
                return Ok(Some(v));
            }
            cur = val(s, "basedOn").and_then(by_id);
        }
        Ok(root
            .elements()
            .find(|e| e.is(w("docDefaults")))
            .and_then(|d| d.elements().find(|e| e.is(w("pPrDefault"))))
            .and_then(from_ppr))
    }

    pub(crate) fn inherited_numbering(&self, style: Option<&str>) -> Result<Option<u32>> {
        if self.styles_ambiguous {
            return Err(Error::AmbiguousStylesPart);
        }
        self.inherited_paragraph_value(style, |ppr| {
            let props = ppr.elements().find(|n| n.is(w("numPr")))?;
            val(props, "numId").map(str::to_owned)
        }).map(|value| value.and_then(|v| v.trim().parse().ok()))
    }

    /// The explicit opt-in that lifts W2 (`StaleFieldRisk`) for every
    /// operation of this `Document` from now on (ADR-0007 §6.3). It writes
    /// `<w:updateFields w:val="true"/>` into `settings.xml`, so that Word
    /// updates fields when it opens the file (it may ask first *(u)*).
    /// Readers other than Word keep showing the stale cached results.
    ///
    /// An element that already reads as on is left byte-identical; any other
    /// value is set to `true`. Calling it again is a no-op.
    ///
    /// Errors, before any change: W1 [`Error::TrackRevisionsOn`] and W3
    /// [`Error::DocumentProtected`] (not W2); [`Error::NoSettingsPart`],
    /// [`Error::AmbiguousSettings`]; [`Error::DuplicateSetting`];
    /// `Core(UnresolvedSibling)` when `settings.xml`'s children are out of
    /// schema order or include `mc:AlternateContent` (dry run);
    /// `Core(NamespaceNotInScope)` when the part binds WML only as the
    /// default namespace.
    pub fn enable_update_fields_on_open(&mut self) -> Result<()> {
        self.write_gate_w1_w3()?;
        let name = match &self.settings_parts[..] {
            [] => return Err(Error::NoSettingsPart),
            [one] => one.clone(),
            _ => return Err(Error::AmbiguousSettings),
        };
        let xml = self.pkg.xml_part_mut(&name)?;
        let mut settings = xml.lens_mut::<Settings>()?;
        let found: Vec<bool> = settings
            .node()
            .elements()
            .filter(|e| e.is(UpdateFieldsOnOpen::QNAME))
            .map(|e| e.attr(w("val")).is_none_or(|v| ooxml_core::value::parse_on_off(v) == Some(true)))
            .collect();
        match found[..] {
            [] => {
                let idx = crate::format::slot_index::<Settings>(settings.node(), UpdateFieldsOnOpen::QNAME);
                settings.check_insert_at::<UpdateFieldsOnOpen>(idx)?;
                if !crate::format::wml_prefix_in_scope(settings.node(), &[]) {
                    return Err(Error::Core(ooxml_core::Error::NamespaceNotInScope(NS_W.into())));
                }
                settings.insert::<UpdateFieldsOnOpen>()?.set_attr(w("val"), "true")?;
            }
            [on] => {
                if !on {
                    settings.child_mut::<UpdateFieldsOnOpen>().expect("exists").set_attr(w("val"), "true")?;
                }
            }
            _ => return Err(Error::DuplicateSetting { name: "w:updateFields" }),
        }
        self.fields_opt_in = true;
        self.update_fields = Some(true);
        Ok(())
    }

    /// Whether [`Self::enable_update_fields_on_open`] was called.
    pub fn update_fields_opted_in(&self) -> bool {
        self.fields_opt_in
    }
}

/// Definition of a new paragraph or character style (ADR-0011 §8).
#[derive(Clone, Debug)]
pub struct StyleSpec<'a> {
    /// Paragraph or character.
    pub kind: StyleKind,
    /// Display name.
    pub name: &'a str,
    /// Explicit id, or an id derived from the display name.
    pub id: Option<&'a str>,
    /// Existing parent style id of the same kind.
    pub based_on: Option<&'a str>,
    /// Existing following style id of the same kind.
    pub next: Option<&'a str>,
    /// Run formatting of the new style.
    pub run: crate::RunFormat,
    /// Paragraph formatting of a paragraph style.
    pub paragraph: crate::ParagraphFormat,
}

impl Document {
    pub(crate) fn used_style_ids(&mut self) -> Result<BTreeSet<String>> {
        use crate::names::descendants;

        let mut used = BTreeSet::new();
        for part in self.registry_parts(&["styles", "stylesWithEffects", "numbering", "settings"])? {
            let xml = self.pkg.xml_part(&part)?;
            for n in descendants(xml.root()).filter(|n| n.name().ns() == NS_W) {
                let attr = match n.name().local() {
                    "style" => "styleId",
                    "pStyle" | "rStyle" | "tblStyle" | "basedOn" | "next" | "link" | "styleLink" | "numStyleLink" | "clickAndTypeStyle" | "defaultTableStyle" => "val",
                    _ => continue,
                };
                used.extend(n.attr(w(attr)).map(fold));
            }
        }
        Ok(used)
    }

    /// Append a custom style and return its id. Requires a styles part.
    pub fn add_style(&mut self, spec: &StyleSpec<'_>) -> Result<String> {
        use crate::format::{apply_ppr, apply_rpr, plan_paragraph, plan_run};
        use ooxml_core::{NsDecl, RegistryPlan};
        use ooxml_schema::wml::{BasedOn, NextParagraphStyle, PrimaryStyle, Style, StyleName, StyleParagraphProperties, StyleRunProperties, Styles};

        self.write_gate_w1_w3()?;
        if self.styles_ambiguous {
            return Err(Error::AmbiguousStylesPart);
        }
        let part = self.styles_part.clone().ok_or(Error::NoStylesPart)?;
        let root = self.styles_root().ok_or(Error::NoStylesPart)?;
        let kind = match spec.kind {
            StyleKind::Paragraph => "paragraph",
            StyleKind::Character => "character",
            _ => return Err(Error::InvalidValue { what: "style kind", value: format!("{:?}", spec.kind) }),
        };
        for (what, value) in [("style name", Some(spec.name)), ("style id", spec.id)] {
            if let Some(value) = value {
                if value.trim().is_empty() {
                    return Err(Error::InvalidValue { what, value: value.into() });
                }
                if let Some((_, ch)) = ooxml_core::value::first_invalid_xml_char(value) {
                    return Err(Error::InvalidXmlChar { ch });
                }
            }
        }
        let name = fold(spec.name);
        if !self.style_ids_by_name(spec.name).is_empty() {
            return Err(Error::DuplicateStyleName { name: spec.name.into() });
        }
        if root.elements().filter(|n| n.is(w("latentStyles"))).flat_map(RawNode::elements)
            .any(|n| n.is(w("lsdException")) && n.attr(w("name")).is_some_and(|n| fold(n) == name))
        {
            return Err(Error::BuiltInStyleName { name: spec.name.into() });
        }
        for id in [spec.based_on, spec.next].into_iter().flatten() {
            self.resolve_style(id, spec.kind)?;
        }
        let used = self.used_style_ids()?;
        let id = if let Some(id) = spec.id {
            if used.contains(&fold(id)) {
                return Err(Error::StyleIdInUse { id: id.into() });
            }
            id.into()
        } else {
            let mut stem: String = spec.name.chars().filter(char::is_ascii_alphanumeric).collect();
            if stem.is_empty() {
                stem.push('a');
            }
            if used.contains(&fold(&stem)) {
                (0..).map(|i| format!("{stem}{i}")).find(|id| !used.contains(&fold(id))).expect("finite registry")
            } else {
                stem
            }
        };
        let run = plan_run(None, &spec.run)?;
        let paragraph = plan_paragraph(None, &spec.paragraph, &|element, attr| {
            self.inherited_ppr_attr(spec.based_on, element, attr)
        })?;
        if spec.kind == StyleKind::Character && !paragraph.props.is_empty() {
            return Err(Error::InvalidValue { what: "character style paragraph formatting", value: spec.name.into() });
        }

        let mut xml = self.pkg.xml_part(&part)?.clone();
        let mut styles = xml.lens_mut::<Styles>()?;
        let mut style = styles.insert_with_ns::<Style>(&[NsDecl { prefix: "w", uri: NS_W }])?;
        style.set_attr(w("styleId"), &id)?;
        style.set_attr(w("type"), kind)?;
        style.set_attr(w("customStyle"), "1")?;
        style.insert::<StyleName>()?.set_attr(w("val"), spec.name)?;
        if let Some(id) = spec.based_on {
            style.insert::<BasedOn>()?.set_attr(w("val"), id)?;
        }
        if let Some(id) = spec.next {
            style.insert::<NextParagraphStyle>()?.set_attr(w("val"), id)?;
        }
        style.insert::<PrimaryStyle>()?;
        if !paragraph.props.is_empty() {
            let mut ppr = style.insert::<StyleParagraphProperties>()?.cast()?;
            for op in &paragraph.props {
                apply_ppr(&mut ppr, op)?;
            }
        }
        if !run.props.is_empty() {
            // The ECMA style rPr uses CT_RPr. The SDK's style-specific
            // projection omits highlight, which belongs to the Phase1 subset.
            let mut rpr = style.insert::<StyleRunProperties>()?.cast()?;
            for op in &run.props {
                apply_rpr(&mut rpr, op)?;
            }
        }
        let mut plan = RegistryPlan::new(&self.pkg);
        plan.stage_part(&self.pkg, &part, xml)?;
        self.pkg.apply(plan)?;
        Ok(id)
    }
}
