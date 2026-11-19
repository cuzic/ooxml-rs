//! The shared-string registry (ADR-0006 §4).
//!
//! - Activated by the first shared-string write in a workbook that has a
//!   `sharedStrings.xml` part; without one, strings stay inline (§4.6).
//! - Append-only: a new string is a new last `si`.
//! - Exact-form reuse: an existing `si` whose bytes are exactly what would
//!   be appended is reused; rich and phonetic items never match.
//! - `uniqueCount` = entries; `count` = `t="s"` cells in every sheet part
//!   the census scans (worksheets, macro sheets, dialog sheets; review n1),
//!   counted at activation and kept current by every later write. An
//!   absent attribute stays absent.

use std::collections::HashMap;

use ooxml_core::{ChildOf, ElementSpec, LensMut, QName};
use ooxml_schema::sml;

use super::format::{Font, Underline};
use super::{Table, census_sheet_parts, current_document, render, scan_sheet};
use crate::error::{Error, Result};
use crate::workbook::Workbook;

/// The registry state.
#[derive(Debug)]
pub(crate) struct SharedStrings {
    pub(crate) part: String,
    prefix: Option<String>,
    entries: u32,
    reuse: HashMap<String, u32>,
    /// `t="s"` cells in the workbook.
    pub(crate) refs: i64,
    has_count: bool,
    has_unique: bool,
}

/// One run of a rich string: its font (`None` = no `rPr`) and its text,
/// already `_xHHHH_`-escaped.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct RichRun {
    pub(crate) font: Option<Font>,
    pub(crate) escaped: String,
}

/// The content of a new `si` (ADR-0006 §4.2): plain text or runs. The two
/// never collide: their canonical bytes differ (`si/t` against `si/r`).
#[derive(Clone, Debug)]
pub(crate) enum SiContent {
    Plain(String),
    Rich(Vec<RichRun>),
}

/// What one string write does to the registry.
#[derive(Clone, Debug)]
pub(crate) struct StringPlan {
    pub(crate) index: u32,
    /// The canonical bytes, when a new `si` is appended.
    append: Option<String>,
    content: SiContent,
    decoded: String,
}

impl StringPlan {
    pub(crate) fn will_append(&self) -> bool {
        self.append.is_some()
    }
}

fn build_si(parent: &mut LensMut<'_, sml::SharedStringTable>, content: &SiContent) -> Result<()> {
    let mut si = parent.insert::<sml::SharedStringItem>()?;
    match content {
        SiContent::Plain(escaped) => {
            si.insert::<sml::Text>()?.set_text(escaped)?;
        }
        SiContent::Rich(runs) => {
            for r in runs {
                build_run(&mut si, r)?;
            }
        }
    }
    Ok(())
}

/// Appends one `r` (with `rPr` when the run has a font, then `t`) under
/// `si` or `is`.
pub(crate) fn build_run<P>(parent: &mut LensMut<'_, P>, run: &RichRun) -> Result<()>
where
    P: ElementSpec,
    sml::Run: ChildOf<P>,
{
    let mut r = parent.insert::<sml::Run>()?;
    if let Some(f) = &run.font {
        build_rpr(&mut r, f)?;
    }
    r.insert::<sml::Text>()?.set_text(&run.escaped)?;
    Ok(())
}

/// The run properties of a font (`CT_RPrElt`), in Excel's and
/// rust_xlsxwriter's order: `b`, `i`, `strike`, `condense`, `extend`, `u`,
/// `vertAlign`, `sz`, `color`, `rFont`, `family`, `charset`, `scheme`.
fn build_rpr(r: &mut LensMut<'_, sml::Run>, f: &Font) -> Result<()> {
    let mut p = r.insert::<sml::RunProperties>()?;
    let val = QName::new("", "val");
    if f.bold {
        p.insert::<sml::Bold>()?;
    }
    if f.italic {
        p.insert::<sml::Italic>()?;
    }
    if f.strike {
        p.insert::<sml::Strike>()?;
    }
    if f.condense {
        p.insert::<sml::Condense>()?;
    }
    if f.extend {
        p.insert::<sml::Extend>()?;
    }
    if let Some(u) = f.underline {
        let mut e = p.insert::<sml::Underline>()?;
        match u {
            Underline::Single => {}
            Underline::Double => e.set_attr(val, "double")?,
            Underline::SingleAccounting => e.set_attr(val, "singleAccounting")?,
            Underline::DoubleAccounting => e.set_attr(val, "doubleAccounting")?,
        }
    }
    if let Some(v) = f.vert_align {
        p.insert::<sml::VerticalTextAlignment>()?.set_attr(val, v.as_str())?;
    }
    p.insert::<sml::FontSize>()?.set_attr(val, &f.size.to_string())?;
    if let Some(c) = &f.color {
        let mut e = p.insert::<sml::Color>()?;
        for (k, v) in c.attrs() {
            e.set_attr(QName::new("", k), &v)?;
        }
    }
    p.insert::<sml::RunFont>()?.set_attr(val, &f.name)?;
    if let Some(fam) = f.family {
        p.insert::<sml::FontFamily>()?.set_attr(val, &fam.to_string())?;
    }
    if let Some(cs) = f.charset {
        p.insert::<sml::RunPropertyCharSet>()?.set_attr(val, &cs.to_string())?;
    }
    if let Some(sc) = &f.scheme {
        p.insert::<sml::FontScheme>()?.set_attr(val, sc)?;
    }
    Ok(())
}

impl SharedStrings {
    /// Activates the registry if the workbook has a shared-string part.
    pub(crate) fn activate(wb: &mut Workbook) -> Result<Option<SharedStrings>> {
        let rels = wb.pkg.relationships(&wb.workbook_part)?;
        let Some(part) = rels
            .by_type(crate::workbook::REL_SHARED_STRINGS)
            .next()
            .and_then(|r| rels.resolve(r))
            .filter(|p| wb.pkg.contains(p))
        else {
            return Ok(None);
        };
        let mut refs = 0i64;
        for p in census_sheet_parts(wb) {
            refs += scan_sheet(&wb.pkg.read_part(&p)?)?.shared as i64;
        }
        let doc = current_document(wb.pkg.xml_part(&part)?)?;
        let root = doc.root();
        if !root.is(sml::SharedStringTable::QNAME) {
            return Err(Error::Malformed(format!("{part}: root is not sst")));
        }
        let t = Table::read(&doc, Some(root), sml::SharedStringItem::QNAME);
        if t.has_ac {
            // indices behind an mc:AlternateContent depend on the reader
            return Err(ooxml_core::Error::UnresolvedSibling(sml::SharedStringTable::QNAME).into());
        }
        Ok(Some(SharedStrings {
            prefix: root.name().prefix().map(str::to_owned),
            entries: t.count,
            reuse: std::sync::Arc::unwrap_or_clone(t.reuse),
            refs,
            has_count: root.attr(QName::new("", "count")).is_some(),
            has_unique: root.attr(QName::new("", "uniqueCount")).is_some(),
            part,
        }))
    }

    /// Plans one plain string (already `_xHHHH_`-escaped).
    pub(crate) fn plan(&self, escaped: &str, decoded: &str) -> Result<StringPlan> {
        self.plan_content(SiContent::Plain(escaped.into()), decoded)
    }

    /// Plans one string, plain or rich. An existing `si` is reused only if
    /// its bytes are exactly the canonical form of `content` (§4.3).
    pub(crate) fn plan_content(&self, content: SiContent, decoded: &str) -> Result<StringPlan> {
        let key = render::<sml::SharedStringTable>(self.prefix.as_deref(), |p| build_si(p, &content))?;
        if let Some(&i) = self.reuse.get(&key) {
            return Ok(StringPlan { index: i, append: None, content, decoded: decoded.into() });
        }
        Ok(StringPlan { index: self.entries, append: Some(key), content, decoded: decoded.into() })
    }

    /// Assigns new slots in write order and reuses earlier batch appends.
    pub(crate) fn reserve(&self, plan: &mut StringPlan, appends: &mut HashMap<String, u32>) {
        if let Some(key) = &plan.append {
            if let Some(&index) = appends.get(key) {
                plan.index = index;
                plan.append = None;
            } else {
                plan.index = self.entries + appends.len() as u32;
                appends.insert(key.clone(), plan.index);
            }
        }
    }

    /// Applies a plan and a change in the number of `t="s"` cells. Called
    /// from the apply phase; a failure poisons the workbook.
    pub(crate) fn apply(wb: &mut Workbook, plans: &[StringPlan], refs_delta: i64, refs_changed: bool) -> Result<()> {
        if plans.is_empty() && !refs_changed {
            return Ok(());
        }
        let Some(mut reg) = wb.registries.strings.take() else { return Ok(()) };
        let result = (|| -> Result<()> {
            let doc = wb.pkg.xml_part_mut(&reg.part)?;
            let mut sst = doc.lens_mut::<sml::SharedStringTable>()?;
            for p in plans {
                let key = p.append.as_ref().expect("new shared string");
                debug_assert_eq!(p.index, reg.entries);
                build_si(&mut sst, &p.content)?;
                reg.reuse.insert(key.clone(), reg.entries);
                reg.entries += 1;
                if let Some(cache) = wb.shared_strings.as_mut() {
                    cache.push(p.decoded.clone());
                }
            }
            if !plans.is_empty() && reg.has_unique {
                sst.set_attr(QName::new("", "uniqueCount"), &reg.entries.to_string())?;
            }
            reg.refs += refs_delta;
            if reg.has_count {
                sst.set_attr(QName::new("", "count"), &reg.refs.max(0).to_string())?;
            }
            Ok(())
        })();
        wb.registries.strings = Some(reg);
        result
    }
}
