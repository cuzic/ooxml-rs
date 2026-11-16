//! Word Phase2, part 1 (ADR-0011): new paragraphs (§9.1), section breaks
//! (§6) and hyperlinks (§4).
//!
//! Every operation runs in ADR-0011 §1's order: checks and the Layer 2 dry
//! run first (which may call `xml_part_mut`), then the registry plan for new
//! relationships, then `Package::apply`, then the tree edits, which cannot
//! fail after the dry run.

use ooxml_core::{ChildOf, ElementSpec, LensMut, NsDecl, QName, RawNode, RegistryPlan, Target, XmlDocument};
use ooxml_schema::wml::{
    self, Body, Comment, Comments, Endnote, Endnotes, Footer, FooterReference, Footnote, Footnotes, Header,
    HeaderReference, Hyperlink, LineNumberType, NS_R, NS_W, PageNumberType, Paragraph as P, ParagraphProperties,
    ParagraphStyleId, Run as R, RunProperties, RunStyle, SectionProperties, SectionType, Table, TableCell, TableRow,
};

use crate::body::Wrapper;
use crate::document::{Document, node_at, write_pieces};
use crate::error::{Error, Result};
use crate::fields::Operation;
use crate::names::is;
use crate::story::{ParaRef, RunRef, StoryIndex, StoryKind};
use crate::styles::StyleKind;
use crate::text::map_text;

const fn w(local: &'static str) -> QName {
    QName::new(NS_W, local)
}
const R_ID: QName = QName::new(NS_R, "id");
const HYPERLINK_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink";

/// Where a new paragraph goes relative to an existing one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Place {
    /// Right before the paragraph.
    Before(ParaRef),
    /// Right after the paragraph.
    After(ParaRef),
}

/// The properties of a new paragraph (ADR-0011 §9.1).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum ParagraphSpec {
    /// No `w:pPr`: the default paragraph style.
    #[default]
    Plain,
    /// A copy of the neighbour's `w:pPr` without `w:sectPr` and
    /// `w:pPrChange`; `w:numPr` is kept, so a list continues.
    LikeNeighbour,
    /// `w:pPr/w:pStyle` with an existing paragraph style id (ADR-0007 §5).
    Style(String),
}

/// How a section starts (`w:sectPr/w:type`, ADR-0011 §6).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SectionBreak {
    /// `nextPage`
    NextPage,
    /// `continuous`
    Continuous,
    /// `evenPage`
    EvenPage,
    /// `oddPage`
    OddPage,
}

impl SectionBreak {
    fn as_str(self) -> &'static str {
        match self {
            Self::NextPage => "nextPage",
            Self::Continuous => "continuous",
            Self::EvenPage => "evenPage",
            Self::OddPage => "oddPage",
        }
    }
}

/// Where a run-level insertion goes (ADR-0011 §3, §4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RunPosition {
    /// At the end of the paragraph's content.
    EndOf(ParaRef),
    /// Right after this run (a direct child of its paragraph).
    After(RunRef),
}

impl RunPosition {
    fn para(self) -> ParaRef {
        match self {
            Self::EndOf(p) => p,
            Self::After(r) => r.para(),
        }
    }
}

/// A hyperlink's target (ADR-0011 §4).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum HyperlinkTarget {
    /// An absolute URI (`scheme:…`), stored as given: one new external
    /// relationship per hyperlink. The caller percent-encodes.
    External(String),
    /// An existing bookmark of the document (`w:anchor`), compared ignoring case.
    Bookmark(String),
}

// ---------------------------------------------------------------------
// The containers a paragraph can be inserted into
// ---------------------------------------------------------------------

/// A mutable lens on a paragraph container.
enum Container<'a> {
    Body(LensMut<'a, Body>),
    Cell(LensMut<'a, TableCell>),
    Header(LensMut<'a, Header>),
    Footer(LensMut<'a, Footer>),
    Footnote(LensMut<'a, Footnote>),
    Endnote(LensMut<'a, Endnote>),
    Comment(LensMut<'a, Comment>),
}

macro_rules! on_container {
    ($c:expr, mut $l:ident => $e:expr) => {
        match $c {
            Container::Body(mut $l) => $e,
            Container::Cell(mut $l) => $e,
            Container::Header(mut $l) => $e,
            Container::Footer(mut $l) => $e,
            Container::Footnote(mut $l) => $e,
            Container::Endnote(mut $l) => $e,
            Container::Comment(mut $l) => $e,
        }
    };
    ($c:expr, $l:ident => $e:expr) => {
        match $c {
            Container::Body($l) => $e,
            Container::Cell($l) => $e,
            Container::Header($l) => $e,
            Container::Footer($l) => $e,
            Container::Footnote($l) => $e,
            Container::Endnote($l) => $e,
            Container::Comment($l) => $e,
        }
    };
}

/// `c` itself when `rest` is empty, else the cell reached through
/// `tbl/tr/tc` triples.
fn container_below<'a, E>(c: LensMut<'a, E>, rest: &[usize], wrap: fn(LensMut<'a, E>) -> Container<'a>) -> Option<Container<'a>>
where
    E: ElementSpec,
    Table: ChildOf<E>,
{
    let [t, r, k, more @ ..] = rest else {
        return rest.is_empty().then(|| wrap(c));
    };
    let mut cell = c.into_child_at::<Table>(*t)?.into_child_at::<TableRow>(*r)?.into_child_at::<TableCell>(*k)?;
    let mut more = more;
    while let [t, r, k, tail @ ..] = more {
        cell = cell.into_child_at::<Table>(*t)?.into_child_at::<TableRow>(*r)?.into_child_at::<TableCell>(*k)?;
        more = tail;
    }
    more.is_empty().then_some(Container::Cell(cell))
}

/// The container at `path` (from the story root), when it is a body,
/// header, footer, note, comment or table cell.
fn container_at<'a>(xml: &'a mut XmlDocument, kind: StoryKind, path: &[usize]) -> Result<Option<Container<'a>>> {
    Ok(match kind {
        StoryKind::Main => {
            let Some((&b, rest)) = path.split_first() else { return Ok(None) };
            match xml.lens_mut::<wml::Document>()?.into_child_at::<Body>(b) {
                Some(body) => container_below(body, rest, Container::Body),
                None => None,
            }
        }
        StoryKind::Header => container_below(xml.lens_mut::<Header>()?, path, Container::Header),
        StoryKind::Footer => container_below(xml.lens_mut::<Footer>()?, path, Container::Footer),
        StoryKind::Footnotes => {
            let Some((&n, rest)) = path.split_first() else { return Ok(None) };
            match xml.lens_mut::<Footnotes>()?.into_child_at::<Footnote>(n) {
                Some(c) => container_below(c, rest, Container::Footnote),
                None => None,
            }
        }
        StoryKind::Endnotes => {
            let Some((&n, rest)) = path.split_first() else { return Ok(None) };
            match xml.lens_mut::<Endnotes>()?.into_child_at::<Endnote>(n) {
                Some(c) => container_below(c, rest, Container::Endnote),
                None => None,
            }
        }
        StoryKind::Comments => {
            let Some((&n, rest)) = path.split_first() else { return Ok(None) };
            match xml.lens_mut::<Comments>()?.into_child_at::<Comment>(n) {
                Some(c) => container_below(c, rest, Container::Comment),
                None => None,
            }
        }
    })
}

// ---------------------------------------------------------------------
// The operations
// ---------------------------------------------------------------------

impl Document {
    /// The item index of a paragraph in its parent, the parent's path, and
    /// the wrappers between the story container and the paragraph.
    fn para_location(&self, at: ParaRef) -> Result<(Vec<usize>, usize)> {
        let data = self.stories.get(at.story.0).ok_or(Error::NoSuchStory)?;
        let info = data.index.paragraphs.get(at.index).ok_or(Error::NoSuchParagraph(at.index))?;
        if let Some(w) = data.index.wrappers(info.frame).into_iter().next() {
            return Err(Error::UnsupportedContext { ancestor: w.qname() });
        }
        let mut path = data.index.path(info.frame);
        let i = path.pop().expect("a paragraph has a parent");
        Ok((path, i))
    }

    /// ADR-0007 §4's paragraph targets (no field, no tracked mark, no
    /// property revision), for a neighbour or a paragraph being split.
    fn check_structure_neighbour(&self, at: ParaRef) -> Result<()> {
        let p = self.paragraph_at(at).ok_or(Error::NoSuchParagraph(at.index))?;
        if p.runs().any(|r| r.in_field()) {
            return Err(Error::InsideField);
        }
        if let Some(ppr) = p.raw().elements().find(|e| is::<ParagraphProperties>(e)) {
            if ppr.elements().any(|e| e.is(w("pPrChange"))) {
                return Err(Error::TrackedPropertyChange { element: "w:pPrChange" });
            }
            if let Some(mark) = ppr.elements().find(|e| e.is(w("rPr"))) {
                for (l, ctx) in [
                    ("ins", crate::RunContext::Inserted),
                    ("del", crate::RunContext::Deleted),
                    ("moveFrom", crate::RunContext::MovedFrom),
                    ("moveTo", crate::RunContext::MovedTo),
                ] {
                    if mark.elements().any(|e| e.is(w(l))) {
                        return Err(Error::InsideRevision { context: ctx });
                    }
                }
                if mark.elements().any(|e| e.is(w("rPrChange"))) {
                    return Err(Error::TrackedPropertyChange { element: "w:rPrChange" });
                }
            }
        }
        Ok(())
    }

    /// Inserts a paragraph with `text` before or after an existing one, in
    /// the same container (body, header, footer, note, comment or table
    /// cell; ADR-0011 §9.1). `\t` and `\n` in `text` become `w:tab` and
    /// `w:br`; the run has no `w:rPr` (copying the paragraph mark's run
    /// properties needs ADR-0007 §8.2's generator rule, Phase1 part 3).
    /// Returns the new paragraph. Every later paragraph of the story moves
    /// up by one and gets a new [`Self::generation`].
    ///
    /// Errors, before any change: [`Self::write_gate_for`]
    /// ([`Operation::Structure`]); [`Error::UnsupportedContext`] (the
    /// neighbour is in a block-level content control or custom XML);
    /// [`Error::InsideField`], [`Error::InsideRevision`],
    /// [`Error::TrackedPropertyChange`] (the neighbour); the style checks
    /// of [`Self::set_paragraph_style`] for [`ParagraphSpec::Style`];
    /// `Core(NotDetachable)` for a `w:pPr` that cannot be copied;
    /// [`Error::InvalidXmlChar`]; dry-run failures (`UnresolvedSibling`, …).
    pub fn insert_paragraph(&mut self, place: Place, text: &str, spec: &ParagraphSpec) -> Result<ParaRef> {
        self.write_gate_for(Operation::Structure)?;
        let (anchor, before) = match place {
            Place::Before(p) => (p, true),
            Place::After(p) => (p, false),
        };
        let (parent_path, item) = self.para_location(anchor)?;
        self.check_structure_neighbour(anchor)?;
        let pieces = map_text(text)?;
        if let ParagraphSpec::Style(id) = spec {
            self.resolve_style(id, StyleKind::Paragraph)?;
        }
        let kind = self.stories[anchor.story.0].id.kind;
        let part = self.stories[anchor.story.0].id.part.clone();
        // the copy of the neighbour's pPr (read-only, before any change)
        let copy = match spec {
            ParagraphSpec::LikeNeighbour => {
                let doc = self.pkg.parsed_part(&part).expect("story parts are parsed");
                let mut ppath = parent_path.clone();
                ppath.push(item);
                let p = node_at(doc.root(), &ppath);
                match p.indexed_elements().find(|(_, e)| is::<ParagraphProperties>(e)) {
                    Some((i, _)) => {
                        ppath.push(i);
                        let lens = doc.lens_at::<ParagraphProperties>(&ppath)?;
                        Some(lens.detach_as::<ParagraphProperties>(&[w("sectPr"), w("pPrChange")])?)
                    }
                    None => None,
                }
            }
            _ => None,
        };
        // dry run: the new w:p in the container, at its position
        let xml = self.pkg.xml_part_mut(&part)?;
        let mut c = container_at(xml, kind, &parent_path)?.ok_or(Error::NoSuchParagraph(anchor.index))?;
        let target = item;
        on_container!(&mut c, l => {
            let pos = if before { target } else { next_slot_sibling(l.node(), target) };
            l.check_insert_at::<P>(pos)?;
        });
        // write
        on_container!(c, mut l => {
            let pos = if before { target } else { next_slot_sibling(l.node(), target) };
            let mut p = l.insert_at::<P>(pos)?;
            match spec {
                ParagraphSpec::Plain => {}
                ParagraphSpec::LikeNeighbour => {
                    if let Some(d) = &copy {
                        p.insert_detached(d)?;
                    }
                }
                ParagraphSpec::Style(id) => {
                    p.insert::<ParagraphProperties>()?.insert::<ParagraphStyleId>()?.set_attr(w("val"), id)?;
                }
            }
            if !pieces.is_empty() {
                write_pieces(&mut p.insert::<R>()?, &pieces)?;
            }
        });
        let new = ParaRef::new(anchor.story, if before { anchor.index } else { anchor.index + 1 });
        self.paragraph_inserted(new);
        Ok(new)
    }

    /// Splits the section that holds body paragraph `after` there (ADR-0011
    /// §6). The earlier half ends at `after` with a copy of the section's
    /// `w:sectPr` (header and footer references written again, restart
    /// settings kept); the later half, the section's original `w:sectPr`,
    /// gets `w:type` = `kind`, loses `w:pgNumType/@w:start` (page numbers
    /// continue) and has `w:lnNumType/@w:restart="newSection"` set to
    /// `continuous`. `w:titlePg` stays on both halves, and footnote numbering
    /// restarts (`numRestart`) apply to both *(u)*.
    ///
    /// Errors, before any change: [`Self::write_gate_for`]
    /// ([`Operation::Structure`]); [`Error::UnsupportedContext`] (not a
    /// paragraph directly in the main story's body); [`Error::AlreadySectionEnd`];
    /// [`Error::NoSectionProperties`] (the section has no `w:sectPr`);
    /// [`Error::TrackedPropertyChange`] (`w:sectPrChange`, or a neighbour
    /// check); `Core(NotDetachable)` (e.g. `w:printerSettings r:id`);
    /// [`Error::RelIdReferenced`] (a header/footer reference whose `r:id`
    /// is not a relationship of the part); `Core(NamespaceNotInScope)` (no
    /// prefixed binding of the relationships namespace for `r:id`);
    /// dry-run failures.
    pub fn insert_section_break(&mut self, after: ParaRef, kind: SectionBreak) -> Result<()> {
        self.write_gate_for(Operation::Structure)?;
        if after.story != StoryIndex::MAIN {
            return Err(Error::UnsupportedContext { ancestor: "a story other than the main body" });
        }
        let (parent_path, item) = self.para_location(after)?;
        if parent_path.len() != 1 {
            return Err(Error::UnsupportedContext { ancestor: "w:tbl" });
        }
        self.check_structure_neighbour(after)?;
        let body_index = parent_path[0];
        let part = self.main_part_name().to_owned();
        let doc = self.pkg.parsed_part(&part).expect("main part parsed");
        let body = node_at(doc.root(), &parent_path);
        // the paragraph must not end a section; S is the next sectPr at or after it
        let sect_of = |n: &RawNode| -> Option<usize> {
            n.indexed_elements().find(|(_, e)| is::<ParagraphProperties>(e)).and_then(|(i, ppr)| {
                ppr.elements().any(is::<SectionProperties>).then_some(i)
            })
        };
        let p_node = body.children()[item].as_element().expect("paragraph");
        if sect_of(p_node).is_some() {
            return Err(Error::AlreadySectionEnd);
        }
        // S: path (from the root) of the w:sectPr that ends the section
        let mut s_path: Option<Vec<usize>> = None;
        for (i, e) in body.indexed_elements().filter(|(i, _)| *i > item) {
            if is::<P>(e)
                && let Some(ppr_i) = sect_of(e)
            {
                let ppr = e.children()[ppr_i].as_element().expect("pPr");
                let si = ppr.indexed_elements().find(|(_, x)| is::<SectionProperties>(x)).map(|(k, _)| k).expect("sectPr");
                s_path = Some(vec![body_index, i, ppr_i, si]);
                break;
            }
            if is::<SectionProperties>(e) {
                s_path = Some(vec![body_index, i]);
                break;
            }
        }
        // paragraphs inside tables after `after` may end sections too: they are
        // not body-level, and such files are refused rather than guessed
        let tables_between = body.indexed_elements().any(|(i, e)| {
            i > item && s_path.as_ref().is_some_and(|s| i < s[1]) && !is::<P>(e) && !is::<SectionProperties>(e)
                && descendant_sectpr(e)
        });
        if tables_between {
            return Err(Error::UnsupportedContext { ancestor: "a section break inside a table" });
        }
        let s_path = s_path.ok_or(Error::NoSectionProperties)?;
        let s_node = node_at(doc.root(), &s_path);
        if s_node.elements().any(|e| e.is(w("sectPrChange"))) {
            return Err(Error::TrackedPropertyChange { element: "w:sectPrChange" });
        }
        // references to write again, checked against the part's relationships
        let rels = self.pkg.relationships(&part)?;
        let mut refs: Vec<(bool, Option<String>, String)> = Vec::new();
        for e in s_node.elements() {
            let header = e.is(HeaderReference::QNAME);
            if !(header || e.is(FooterReference::QNAME)) {
                continue;
            }
            let id = e.attr(R_ID).unwrap_or_default().to_owned();
            if rels.by_id(&id).is_none() {
                return Err(Error::RelIdReferenced { id });
            }
            refs.push((header, e.attr(w("type")).map(str::to_owned), id));
        }
        if !refs.is_empty() && !crate::format::prefix_in_scope(doc.root(), &[body_index, item], NS_R) {
            return Err(Error::Core(ooxml_core::Error::NamespaceNotInScope(NS_R.into())));
        }
        let copy = doc
            .lens_at::<SectionProperties>(&s_path)?
            .detach_as::<SectionProperties>(&[HeaderReference::QNAME, FooterReference::QNAME, w("sectPrChange")])?;
        // dry run: pPr in p (or sectPr in its existing pPr), w:type in S
        let has_ppr = p_node.elements().any(is::<ParagraphProperties>);
        let s_has_type = s_node.elements().any(|e| e.is(SectionType::QNAME));
        let xml = self.pkg.xml_part_mut(&part)?;
        {
            let doc_lens = xml.lens_mut::<wml::Document>()?;
            let body = doc_lens.into_child_at::<Body>(body_index).ok_or(Error::NoBody)?;
            let p = body.into_child_at::<P>(item).ok_or(Error::NoSuchParagraph(after.index))?;
            if has_ppr {
                let ppr = p.into_child_by::<ParagraphProperties>(|_| true).expect("pPr");
                let idx = crate::format::slot_index::<ParagraphProperties>(ppr.node(), SectionProperties::QNAME);
                ppr.check_insert_at::<SectionProperties>(idx)?;
            } else {
                let idx = crate::format::slot_index::<P>(p.node(), ParagraphProperties::QNAME);
                p.check_insert_at::<ParagraphProperties>(idx)?;
            }
        }
        if !s_has_type {
            let s = sect_lens(xml, &s_path)?;
            let idx = crate::format::slot_index::<SectionProperties>(s.node(), SectionType::QNAME);
            s.check_insert_at::<SectionType>(idx)?;
        }
        // write: the earlier half
        {
            let doc_lens = xml.lens_mut::<wml::Document>()?;
            let body = doc_lens.into_child_at::<Body>(body_index).ok_or(Error::NoBody)?;
            let mut p = body.into_child_at::<P>(item).ok_or(Error::NoSuchParagraph(after.index))?;
            let mut ppr = p.get_or_insert::<ParagraphProperties>()?;
            let mut sect = ppr.insert_detached(&copy)?;
            write_refs(&mut sect, &refs)?;
        }
        // the later half: S (its path is unchanged: S comes after p)
        {
            let mut s = sect_lens(xml, &s_path)?;
            s.get_or_insert::<SectionType>()?.set_attr(w("val"), kind.as_str())?;
            if let Some(mut pg) = s.child_mut::<PageNumberType>() {
                pg.remove_attr(w("start"))?;
            }
            if let Some(mut ln) = s.child_mut::<LineNumberType>()
                && ln.node().attr(w("restart")) == Some("newSection")
            {
                ln.set_attr(w("restart"), "continuous")?;
            }
        }
        self.reindex(StoryIndex::MAIN);
        self.bump(after);
        Ok(())
    }

    /// Inserts a hyperlink holding one run with `text` (ADR-0011 §4), at the
    /// end of a paragraph or after a run that is a direct child of its
    /// paragraph. The run gets the character style named "Hyperlink" when
    /// the document has one (found by name, ADR-0007 §5), else no `w:rPr`.
    /// An external target gets a new `…/hyperlink` relationship
    /// (`TargetMode="External"`) of its own; `w:hyperlink` is inserted with
    /// `NsDecl(r)`, declaring `xmlns:r` only when no prefixed binding is in
    /// scope.
    ///
    /// Errors, before any change: [`Self::write_gate_for`]
    /// ([`Operation::Text`]); the paragraph checks of
    /// [`Self::insert_paragraph`]; [`Error::NestedHyperlink`] (the run is
    /// inside a hyperlink); [`Error::UnsupportedContext`];
    /// [`Error::InvalidValue`] (a URI without a scheme);
    /// [`Error::UnknownBookmark`]; [`Error::RelIdReferenced`] (the new
    /// relationship id is already used by an `r:` attribute of the part);
    /// core registry refusals (`InvalidTarget`, …); dry-run failures.
    pub fn insert_hyperlink(&mut self, at: RunPosition, text: &str, target: &HyperlinkTarget) -> Result<RunRef> {
        self.write_gate_for(Operation::Text)?;
        let para = at.para();
        let (parent_path, item) = self.para_location(para)?;
        self.check_structure_neighbour(para)?;
        let pieces = map_text(text)?;
        let p = self.paragraph_at(para).ok_or(Error::NoSuchParagraph(para.index))?;
        // the item index in w:p after which the hyperlink goes (None = end)
        let after_item: Option<usize> = match at {
            RunPosition::EndOf(_) => None,
            RunPosition::After(r) => {
                let run = p.run(r.index()).ok_or(Error::NoSuchRun { paragraph: para.index, run: r.index() })?;
                let ws = run.wrappers();
                if ws.contains(&Wrapper::Hyperlink) {
                    return Err(Error::NestedHyperlink);
                }
                if let Some(w) = ws.first() {
                    return Err(Error::UnsupportedContext { ancestor: w.qname() });
                }
                let rpath = self.stories[para.story.0].index.path(run.info.frame);
                Some(*rpath.last().expect("run path"))
            }
        };
        match target {
            HyperlinkTarget::External(uri) => {
                let scheme = uri.split_once(':').map(|(s, _)| s);
                let ok = scheme.is_some_and(|s| {
                    s.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
                        && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
                });
                if !ok {
                    return Err(Error::InvalidValue { what: "absolute URI", value: uri.clone() });
                }
            }
            HyperlinkTarget::Bookmark(name) => {
                let want: String = name.chars().flat_map(char::to_lowercase).collect();
                let found = self.stories().any(|s| {
                    let mut stack = vec![s.raw()];
                    while let Some(n) = stack.pop() {
                        if n.is(w("bookmarkStart"))
                            && n.attr(w("name")).is_some_and(|v| v.chars().flat_map(char::to_lowercase).collect::<String>() == want)
                        {
                            return true;
                        }
                        stack.extend(n.elements());
                    }
                    false
                });
                if !found {
                    return Err(Error::UnknownBookmark { name: name.clone() });
                }
            }
        }
        let style = self
            .styles()
            .find(|s| s.kind == StyleKind::Character && s.name.is_some_and(|n| n.eq_ignore_ascii_case("hyperlink")))
            .map(|s| s.id.to_owned());
        let kind = self.stories[para.story.0].id.kind;
        let part = self.stories[para.story.0].id.part.clone();
        // r:* values of the part (for the rId check, review N4)
        let r_values: Vec<String> = {
            let doc = self.pkg.parsed_part(&part).expect("story parts are parsed");
            let mut out = Vec::new();
            let mut stack = vec![doc.root()];
            while let Some(n) = stack.pop() {
                out.extend(n.attrs().iter().filter(|a| a.name().ns() == NS_R).map(|a| a.value().to_ascii_lowercase()));
                stack.extend(n.elements());
            }
            out
        };
        let mut ppath = parent_path.clone();
        ppath.push(item);
        let decls = [NsDecl { prefix: "r", uri: NS_R }];
        // in-slot index of the content child the hyperlink goes before, and the
        // index the new run will have among the paragraph's runs
        let (slot_k, new_run) = {
            let p_node = p.raw();
            let k = after_item.map(|a| p_node.indexed_elements().filter(|(i, e)| *i <= a && !is::<ParagraphProperties>(e)).count());
            let runs_before = p
                .runs()
                .filter(|run| {
                    let rp = self.stories[para.story.0].index.path(run.info.frame);
                    after_item.is_none_or(|a| rp[ppath.len()] <= a)
                })
                .count();
            (k, runs_before)
        };
        // 1. dry run
        {
            // depth bound (review R3-2): the paragraph's depth + w:hyperlink/w:r/w:rPr/w:rStyle
            let limit = self.pkg.parse_options().max_depth;
            if ppath.len() + 1 + 4 > limit {
                return Err(Error::Core(ooxml_core::Error::TooDeep { limit, position: 0 }));
            }
            let xml = self.pkg.xml_part_mut(&part)?;
            let p = para_lens(xml, kind, &ppath)?;
            p.check_insert_by_with_ns::<Hyperlink>(before_slot_child(slot_k), &decls)?;
        }
        // 2. plan, 3. apply
        let rid = match target {
            HyperlinkTarget::External(uri) => {
                let mut plan = RegistryPlan::new(&self.pkg);
                let id = plan.add_relationship(&self.pkg, Some(&part), HYPERLINK_REL, Target::External(uri.clone()))?;
                if r_values.contains(&id.to_ascii_lowercase()) {
                    return Err(Error::RelIdReferenced { id });
                }
                self.pkg.apply(plan)?;
                Some(id)
            }
            HyperlinkTarget::Bookmark(_) => None,
        };
        // 4. tree edits
        let xml = self.pkg.xml_part_mut(&part)?;
        let mut p = para_lens(xml, kind, &ppath)?;
        let mut h = p.insert_by_with_ns::<Hyperlink>(before_slot_child(slot_k), &decls)?;
        match (target, &rid) {
            (HyperlinkTarget::External(_), Some(id)) => h.set_attr(R_ID, id)?,
            (HyperlinkTarget::Bookmark(name), _) => h.set_attr(w("anchor"), name)?,
            _ => unreachable!("an external target has an id"),
        }
        h.set_attr(w("history"), "1")?;
        let mut r = h.insert::<R>()?;
        if let Some(id) = &style {
            r.insert::<RunProperties>()?.insert::<RunStyle>()?.set_attr(w("val"), id)?;
        }
        write_pieces(&mut r, &pieces)?;
        self.reindex(para.story);
        self.bump(para);
        Ok(RunRef::new(para, new_run))
    }
}

impl Document {
    /// `append_paragraph` when the body ends with a paragraph whose `w:pPr`
    /// holds the last section's `w:sectPr` (ADR-0004 A2, lifted by ADR-0011
    /// §6): the new paragraph is appended and the `w:sectPr` moves to it.
    pub(crate) fn append_moving_section(&mut self, pieces: &[crate::text::Piece]) -> Result<usize> {
        let index = &self.stories[0].index;
        let last = index.paragraphs.last().expect("A2 implies a last paragraph");
        if !index.wrappers(last.frame).is_empty() {
            return Err(Error::SectionBreakAtEnd);
        }
        let ppath = index.path(last.frame);
        let [body_index, item] = ppath[..] else { return Err(Error::SectionBreakAtEnd) };
        let part = self.main_part_name().to_owned();
        let doc = self.pkg.parsed_part(&part).expect("main part parsed");
        let p_node = node_at(doc.root(), &ppath);
        let (ppr_i, ppr) = p_node.indexed_elements().find(|(_, e)| is::<ParagraphProperties>(e)).expect("A2: pPr");
        let (s_i, s_node) = ppr.indexed_elements().find(|(_, e)| is::<SectionProperties>(e)).expect("A2: sectPr");
        if s_node.elements().any(|e| e.is(w("sectPrChange"))) || ppr.elements().any(|e| e.is(w("pPrChange"))) {
            return Err(Error::TrackedPropertyChange { element: "w:sectPrChange" });
        }
        let s_path = vec![body_index, item, ppr_i, s_i];
        let rels = self.pkg.relationships(&part)?;
        let mut refs: Vec<(bool, Option<String>, String)> = Vec::new();
        for e in s_node.elements() {
            let header = e.is(HeaderReference::QNAME);
            if !(header || e.is(FooterReference::QNAME)) {
                continue;
            }
            let id = e.attr(R_ID).unwrap_or_default().to_owned();
            if rels.by_id(&id).is_none() {
                return Err(Error::RelIdReferenced { id });
            }
            refs.push((header, e.attr(w("type")).map(str::to_owned), id));
        }
        if !refs.is_empty() && !crate::format::prefix_in_scope(doc.root(), &[body_index], NS_R) {
            return Err(Error::Core(ooxml_core::Error::NamespaceNotInScope(NS_R.into())));
        }
        let copy = doc
            .lens_at::<SectionProperties>(&s_path)?
            .detach_as::<SectionProperties>(&[HeaderReference::QNAME, FooterReference::QNAME])?;
        // dry run: the new paragraph in the body
        let xml = self.pkg.xml_part_mut(&part)?;
        {
            let body = xml.lens_mut::<wml::Document>()?.into_child_at::<Body>(body_index).ok_or(Error::NoBody)?;
            let at = next_slot_sibling(body.node(), item);
            body.check_insert_at::<P>(at)?;
        }
        // write: the new paragraph with the moved sectPr, then remove the old one
        {
            let mut body = xml.lens_mut::<wml::Document>()?.into_child_at::<Body>(body_index).ok_or(Error::NoBody)?;
            let at = next_slot_sibling(body.node(), item);
            let mut p = body.insert_at::<P>(at)?;
            {
                let mut ppr = p.insert::<ParagraphProperties>()?;
                let mut sect = ppr.insert_detached(&copy)?;
                write_refs(&mut sect, &refs)?;
            }
            if !pieces.is_empty() {
                write_pieces(&mut p.insert::<R>()?, pieces)?;
            }
        }
        {
            let body = xml.lens_mut::<wml::Document>()?.into_child_at::<Body>(body_index).ok_or(Error::NoBody)?;
            let mut ppr = body
                .into_child_at::<P>(item)
                .and_then(|p| p.into_child_at::<ParagraphProperties>(ppr_i))
                .ok_or(Error::NoBody)?;
            ppr.remove_at(s_i);
        }
        let n = self.stories[0].index.paragraphs.len();
        self.paragraph_inserted(ParaRef::new(StoryIndex::MAIN, n));
        self.bump(ParaRef::new(StoryIndex::MAIN, n - 1));
        Ok(n)
    }
}

/// Writes header/footer references (type, id) into a new `w:sectPr`.
fn write_refs(sect: &mut LensMut<'_, SectionProperties>, refs: &[(bool, Option<String>, String)]) -> Result<()> {
    for (header, ty, id) in refs {
        if *header {
            let mut r = sect.insert::<HeaderReference>()?;
            if let Some(t) = ty {
                r.set_attr(w("type"), t)?;
            }
            r.set_attr(R_ID, id)?;
        } else {
            let mut r = sect.insert::<FooterReference>()?;
            if let Some(t) = ty {
                r.set_attr(w("type"), t)?;
            }
            r.set_attr(R_ID, id)?;
        }
    }
    Ok(())
}

/// Does `e` (any element) contain a paragraph-level `w:sectPr` (inside a
/// table or a content control)?
fn descendant_sectpr(e: &RawNode) -> bool {
    let mut stack = vec![e];
    while let Some(n) = stack.pop() {
        if is::<ParagraphProperties>(n) && n.elements().any(is::<SectionProperties>) {
            return true;
        }
        stack.extend(n.elements());
    }
    false
}

/// Item index right after child `item` of the same slot: the next element
/// sibling, or the end.
fn next_slot_sibling(parent: &RawNode, item: usize) -> usize {
    parent.indexed_elements().find(|(i, _)| *i > item).map_or(parent.children().len(), |(i, _)| i)
}

/// The `before` predicate of `insert_by` that puts a new child of `w:p`'s
/// content slot before the slot child with in-slot index `k` (`None`: at the
/// end of the slot).
fn before_slot_child(k: Option<usize>) -> impl FnMut(usize, &RawNode) -> bool {
    move |i, _| Some(i) == k
}

/// A mutable lens on the main part's `w:sectPr` at `path`.
fn sect_lens<'a>(xml: &'a mut XmlDocument, path: &[usize]) -> Result<LensMut<'a, SectionProperties>> {
    let doc = xml.lens_mut::<wml::Document>()?;
    let body = doc.into_child_at::<Body>(path[0]).ok_or(Error::NoBody)?;
    match path {
        [_, s] => body.into_child_at::<SectionProperties>(*s),
        [_, p, ppr, s] => body
            .into_child_at::<P>(*p)
            .and_then(|p| p.into_child_at::<ParagraphProperties>(*ppr))
            .and_then(|ppr| ppr.into_child_at::<SectionProperties>(*s)),
        _ => None,
    }
    .ok_or(Error::NoSectionProperties)
}

/// A mutable lens on the table at `path` (from the story root), when its
/// parent is a body, header, footer, note, comment or table cell.
pub(crate) fn table_lens<'a>(xml: &'a mut XmlDocument, kind: StoryKind, path: &[usize]) -> Result<Option<LensMut<'a, Table>>> {
    let Some((item, parent)) = path.split_last() else { return Ok(None) };
    let Some(c) = container_at(xml, kind, parent)? else { return Ok(None) };
    Ok(on_container!(c, l => l.into_child_at::<Table>(*item)))
}

/// A mutable lens on the paragraph at `path` (from the story root).
fn para_lens<'a>(xml: &'a mut XmlDocument, kind: StoryKind, path: &[usize]) -> Result<LensMut<'a, P>> {
    let (parent, item) = path.split_at(path.len() - 1);
    let c = container_at(xml, kind, parent)?.ok_or(Error::NoSuchParagraph(0))?;
    on_container!(c, l => l.into_child_at::<P>(item[0]).ok_or(Error::NoSuchParagraph(0)))
}

