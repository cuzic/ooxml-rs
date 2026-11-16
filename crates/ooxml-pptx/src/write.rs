//! Write side (ADR-0005 §3, §6): text edits inside existing text bodies.
//!
//! Every public setter first runs **all** of its checks on the current tree
//! (references into the text, content shape, characters, format sources)
//! and only then calls the ADR-0002 primitives, whose own refusals are
//! ruled out by those checks; a refused edit leaves the tree untouched.

use ooxml_core::{ChildOf, Detachable, Detached, ElementSpec, Lens, LensMut, QName, RawItem, RawNode, SameType, XmlDocument};
use ooxml_schema::dml::{
    self, Break, EndParagraphRunProperties, Field, Graphic, GraphicData, Paragraph, ParagraphProperties, Run,
    RunProperties, Table, TableCell, TableCellView, TableRow, Text,
};
use ooxml_schema::pml::{self, CommonSlideData, GraphicFrame, GroupShape, Shape, ShapeTree};

use crate::error::{Error, Result};
use crate::read::{self, ShapePath, Slide, SlideInfo, TextBodyRef, find_by_id, raw_shape_id};
use crate::refs::SlideRefs;
use crate::text;

pub(crate) fn node_at<'a>(root: &'a RawNode, path: &[usize]) -> Option<&'a RawNode> {
    path.iter().try_fold(root, |n, &i| n.children().get(i).and_then(RawItem::as_element))
}

fn child_is(n: &RawNode, i: usize, q: QName) -> bool {
    n.children().get(i).and_then(RawItem::as_element).is_some_and(|c| c.is(q))
}

/// Typed descent from `p:sld` along a node path (ADR-0002: every step is a
/// `ChildOf` edge of the generated schema).
enum Cursor<'d> {
    Sld(LensMut<'d, pml::Slide>),
    CSld(LensMut<'d, CommonSlideData>),
    Tree(LensMut<'d, ShapeTree>),
    Grp(LensMut<'d, GroupShape>),
    Sp(LensMut<'d, Shape>),
    Frame(LensMut<'d, GraphicFrame>),
    Graphic(LensMut<'d, Graphic>),
    GData(LensMut<'d, GraphicData>),
    Tbl(LensMut<'d, Table>),
    Tr(LensMut<'d, TableRow>),
    Tc(LensMut<'d, TableCell>),
    PBody(LensMut<'d, pml::TextBody>),
    ABody(LensMut<'d, dml::TextBody>),
}

macro_rules! group_step {
    ($l:expr, $i:expr) => {{
        let l = $l;
        if child_is(l.node(), $i, GroupShape::QNAME) {
            l.into_child_at::<GroupShape>($i).map(Cursor::Grp)
        } else if child_is(l.node(), $i, Shape::QNAME) {
            l.into_child_at::<Shape>($i).map(Cursor::Sp)
        } else {
            l.into_child_at::<GraphicFrame>($i).map(Cursor::Frame)
        }
    }};
}

impl<'d> Cursor<'d> {
    fn step(self, i: usize) -> Option<Self> {
        match self {
            Cursor::Sld(l) => l.into_child_at::<CommonSlideData>(i).map(Cursor::CSld),
            Cursor::CSld(l) => l.into_child_at::<ShapeTree>(i).map(Cursor::Tree),
            Cursor::Tree(l) => group_step!(l, i),
            Cursor::Grp(l) => group_step!(l, i),
            Cursor::Sp(l) => l.into_child_at::<pml::TextBody>(i).map(Cursor::PBody),
            Cursor::Frame(l) => l.into_child_at::<Graphic>(i).map(Cursor::Graphic),
            Cursor::Graphic(l) => l.into_child_at::<GraphicData>(i).map(Cursor::GData),
            Cursor::GData(l) => l.into_child_at::<Table>(i).map(Cursor::Tbl),
            Cursor::Tbl(l) => l.into_child_at::<TableRow>(i).map(Cursor::Tr),
            Cursor::Tr(l) => l.into_child_at::<TableCell>(i).map(Cursor::Tc),
            Cursor::Tc(l) => l.into_child_at::<dml::TextBody>(i).map(Cursor::ABody),
            Cursor::PBody(_) | Cursor::ABody(_) => None,
        }
    }
}

/// A typed lens on a shape's `p:txBody` or a cell's `a:txBody`.
enum BodyLens<'d> {
    P(LensMut<'d, pml::TextBody>),
    A(LensMut<'d, dml::TextBody>),
}

macro_rules! with_body {
    ($b:expr, $l:ident => $e:expr) => {
        match $b {
            BodyLens::P($l) => $e,
            BodyLens::A($l) => $e,
        }
    };
}

fn body_lens<'d>(doc: &'d mut XmlDocument, path: &[usize]) -> Result<BodyLens<'d>> {
    let mut cur = Cursor::Sld(doc.lens_mut::<pml::Slide>()?);
    for &i in path {
        cur = cur.step(i).ok_or(Error::ShapeNotFound)?;
    }
    match cur {
        Cursor::PBody(l) => Ok(BodyLens::P(l)),
        Cursor::ABody(l) => Ok(BodyLens::A(l)),
        _ => Err(Error::ShapeNotFound),
    }
}

/// A typed lens on the `a:p` at child index `para_child` of the text body
/// at `body` (a node path from the slide root).
pub(crate) fn paragraph_lens<'d>(doc: &'d mut XmlDocument, body: &[usize], para_child: usize) -> Result<LensMut<'d, Paragraph>> {
    let b = body_lens(doc, body)?;
    with_body!(b, l => paragraph_at(l, para_child))
}

fn paragraph_at<'d, B: ElementSpec>(l: LensMut<'d, B>, child: usize) -> Result<LensMut<'d, Paragraph>>
where
    Paragraph: ChildOf<B>,
{
    l.into_child_at::<Paragraph>(child).ok_or(Error::ShapeNotFound)
}

fn append_paragraph<'d, B: ElementSpec>(l: &'d mut LensMut<'_, B>) -> Result<LensMut<'d, Paragraph>>
where
    Paragraph: ChildOf<B>,
{
    Ok(l.insert::<Paragraph>()?)
}

// ---------------------------------------------------------------------------
// Content shape checks (review m7): only the children Phase 0 models, in
// schema order. Anything else makes the edit `UnsupportedContent`, which
// also rules out the `UnresolvedSibling` refusal of the insert primitive.

const fn a(local: &'static str) -> QName {
    QName::new(crate::names::A_NS, local)
}

fn ordered(n: &RawNode, ranks: &[(QName, u8, bool)], what: &str) -> Result<()> {
    let mut cur = 0u8;
    let mut seen = [0u8; 8];
    for c in n.elements() {
        let Some(&(_, rank, single)) = ranks.iter().find(|(q, _, _)| c.is(*q)) else {
            return Err(Error::UnsupportedContent(format!("{what}: {}", c.name().display())));
        };
        if rank < cur || (single && seen[rank as usize] > 0) {
            return Err(Error::UnsupportedContent(format!("{what}: {} out of order", c.name().display())));
        }
        cur = rank;
        seen[rank as usize] += 1;
    }
    Ok(())
}

/// Child indices of a well-formed `a:p`.
pub(crate) struct ParaShape {
    runs: Vec<usize>,
    breaks: Vec<usize>,
    fields: Vec<usize>,
}

pub(crate) fn para_shape(p: &RawNode) -> Result<ParaShape> {
    ordered(
        p,
        &[
            (ParagraphProperties::QNAME, 0, true),
            (Run::QNAME, 1, false),
            (Break::QNAME, 1, false),
            (Field::QNAME, 1, false),
            (EndParagraphRunProperties::QNAME, 2, true),
        ],
        "a:p",
    )?;
    let mut s = ParaShape { runs: Vec::new(), breaks: Vec::new(), fields: Vec::new() };
    for (i, c) in p.indexed_elements() {
        if c.is(Run::QNAME) {
            ordered(c, &[(RunProperties::QNAME, 0, true), (Text::QNAME, 1, true)], "a:r")?;
            s.runs.push(i);
        } else if c.is(Break::QNAME) {
            s.breaks.push(i);
        } else if c.is(Field::QNAME) {
            s.fields.push(i);
        }
    }
    Ok(s)
}

/// Child indices of the paragraphs of a well-formed text body
/// (`a:bodyPr [a:lstStyle] a:p+`; a body without `a:p` violates the schema
/// and is `UnsupportedContent`, review R2-1).
fn body_paragraphs(b: &RawNode) -> Result<Vec<usize>> {
    ordered(b, &[(a("bodyPr"), 0, true), (a("lstStyle"), 1, true), (Paragraph::QNAME, 2, false)], "text body")?;
    let paras: Vec<usize> = b.indexed_elements().filter(|(_, c)| c.is(Paragraph::QNAME)).map(|(i, _)| i).collect();
    if paras.is_empty() {
        return Err(Error::UnsupportedContent("text body without a:p".into()));
    }
    Ok(paras)
}

// ---------------------------------------------------------------------------
// Format sources (review M1; Phase 0.1, ADR-0005 §3 "Format sources"). A
// new run's `a:rPr` is a subtree copy of the paragraph's first `a:rPr` (or
// of `a:endParaRPr` when it has no run) without `a:hlinkClick` /
// `a:hlinkMouseOver`; a new paragraph in `TextBody::set_text` gets a copy
// of the last paragraph's `a:pPr`, and an `a:endParaRPr` copied from the
// last paragraph's own `a:endParaRPr` when that has any format, else from
// its run format source (review R2-5, N3-1). Copies go through
// `Lens::detach_as` (on a lens that knows the ancestors, `Lens::at`) and
// `LensMut::insert_detached` (ADR-0002 note "detach/insert"); what
// `detach_as` refuses is `FormatNotCopyable`.

/// Children of a run format source that new runs do not inherit: a link
/// stays on the run that had it (ADR-0005 §3 "Hyperlinks").
pub(crate) const LINKS: [QName; 2] = [a("hlinkClick"), a("hlinkMouseOver")];

/// Has `n` any format to copy (attributes or child elements, links aside)?
fn has_format(n: &RawNode) -> bool {
    !n.attrs().is_empty() || n.elements().any(|c| !LINKS.iter().any(|q| c.is(*q)))
}

/// Copy, as a `C`, of the element at `path` (absolute, from the part root);
/// `None` when there is nothing to copy (no attributes and no child
/// elements after `skip`).
pub(crate) fn detach<S, C>(root: &RawNode, path: Option<Vec<usize>>, skip: &[QName], what: &'static str) -> Result<Option<Detached<C>>>
where
    S: Detachable,
    C: SameType<S> + Detachable,
{
    let Some(path) = path else { return Ok(None) };
    let d = Lens::<S>::at(root, &path)?.detach_as::<C>(skip).map_err(|e| match e {
        ooxml_core::Error::NotDetachable { .. } => Error::FormatNotCopyable(what),
        e => e.into(),
    })?;
    let raw = d.raw();
    Ok((!raw.attrs().is_empty() || raw.elements().next().is_some()).then_some(d))
}

/// A paragraph being planned: the part, its root, its absolute path, its node.
#[derive(Clone, Copy)]
struct ParaAt<'r> {
    doc: &'r XmlDocument,
    root: &'r RawNode,
    path: &'r [usize],
    node: &'r RawNode,
}

impl<'r> ParaAt<'r> {
    fn new(doc: &'r XmlDocument, path: &'r [usize]) -> Self {
        let root = doc.root();
        ParaAt { doc, root, path, node: node_at(root, path).expect("paragraph path") }
    }
    /// Absolute path of the first child named `q` (below `under`, a child index).
    fn find(&self, under: Option<usize>, q: QName) -> Option<Vec<usize>> {
        let base = match under {
            Some(i) => node_at(self.node, &[i])?,
            None => self.node,
        };
        let (i, _) = base.indexed_elements().find(|(_, c)| c.is(q))?;
        let mut p = self.path.to_vec();
        p.extend(under);
        p.push(i);
        Some(p)
    }
}

/// The run format source of a paragraph, as a new run's `a:rPr` and as a
/// new paragraph's `a:endParaRPr` (when the last paragraph has no own one).
#[derive(Clone, Default)]
struct RunFmt {
    rpr: Option<Detached<RunProperties>>,
    end: Option<Detached<EndParagraphRunProperties>>,
}

fn run_format(p: ParaAt<'_>, shape: &ParaShape) -> Result<RunFmt> {
    match shape.runs.first() {
        Some(&r) => {
            let src = p.find(Some(r), RunProperties::QNAME);
            Ok(RunFmt {
                rpr: detach::<RunProperties, _>(p.root, src.clone(), &LINKS, "a:rPr")?,
                end: detach::<RunProperties, _>(p.root, src, &LINKS, "a:rPr")?,
            })
        }
        None => {
            let src = p.find(None, EndParagraphRunProperties::QNAME);
            Ok(RunFmt {
                rpr: detach::<EndParagraphRunProperties, _>(p.root, src.clone(), &LINKS, "a:endParaRPr")?,
                end: detach::<EndParagraphRunProperties, _>(p.root, src, &LINKS, "a:endParaRPr")?,
            })
        }
    }
}

/// What a paragraph appended by `TextBody::set_text` copies from the last
/// paragraph `p`: its `a:pPr`, its run format source, and the source of its
/// `a:endParaRPr` (N3-1: `p`'s own `a:endParaRPr` if it has any format,
/// which is the end-of-paragraph format PowerPoint carries over on Enter;
/// else the run format source).
/// The run format source is consulted only when it is used (review #19
/// N3): for new runs (`needs_run`), or for `a:endParaRPr` when the last
/// paragraph has no own end format.
fn new_para_format(p: ParaAt<'_>, needs_run: bool) -> Result<(Option<Detached<ParagraphProperties>>, RunFmt)> {
    let shape = para_shape(p.node)?;
    let ppr = detach::<ParagraphProperties, _>(p.root, p.find(None, ParagraphProperties::QNAME), &[], "a:pPr")?;
    let own_end = p.node.elements().find(|c| c.is(EndParagraphRunProperties::QNAME)).is_some_and(has_format);
    let source = if needs_run || !own_end { run_format(p, &shape)? } else { RunFmt::default() };
    let rpr = if needs_run { source.rpr } else { None };
    let end = if own_end {
        detach::<EndParagraphRunProperties, _>(p.root, p.find(None, EndParagraphRunProperties::QNAME), &LINKS, "a:endParaRPr")?
    } else {
        source.end
    };
    Ok((ppr, RunFmt { rpr, end }))
}

/// `NamespaceNotInScope` / `TooDeep` before any change (ADR-0005 §3): every
/// copy must get its prefixes, and fit under the depth limit, when inserted
/// `created` new elements below the element at `under`.
fn check_copies<'c>(doc: &XmlDocument, under: &[usize], created: usize, copies: impl IntoIterator<Item = &'c dyn CheckInsert>) -> Result<()> {
    for c in copies {
        c.check(doc, under, created)?;
    }
    Ok(())
}

/// The plain elements an edit creates below the element at `path` reach
/// `levels` further down: refuse with `TooDeep` before any change if that
/// passes the part's depth limit (ADR-0002 note "parser depth limit"; the
/// core would refuse only at the insert that crosses it, after earlier
/// inserts of the same edit).
fn check_depth(doc: &XmlDocument, path: &[usize], levels: usize) -> Result<()> {
    if path.len() + 1 + levels > doc.max_depth() {
        let position = node_at(doc.root(), path).and_then(RawNode::span).map_or(0, |s| s.start);
        return Err(Error::Core(ooxml_core::Error::TooDeep { limit: doc.max_depth(), position }));
    }
    Ok(())
}

/// `Detached::check_insert_under` for copies of any type.
pub(crate) trait CheckInsert {
    fn check(&self, doc: &XmlDocument, under: &[usize], created: usize) -> Result<()>;
}

impl<C: ElementSpec> CheckInsert for Detached<C> {
    /// A refusal of the copy at its destination (`NotDetachable`: the
    /// destination's `mc:Ignorable` or inherited `xml:space`) is the same
    /// "this format cannot be copied" as a refusal at the source, so it is
    /// reported as `FormatNotCopyable` naming the copy's element (review #19
    /// round 2, R2-4). Depth and namespace refusals stay `Core(…)`.
    fn check(&self, doc: &XmlDocument, under: &[usize], created: usize) -> Result<()> {
        self.check_insert_under(doc, under, created).map_err(|e| match e {
            ooxml_core::Error::NotDetachable { .. } => Error::FormatNotCopyable(element_name(C::QNAME)),
            e => e.into(),
        })
    }
}

/// `a:`-prefixed display name of a copied format element.
fn element_name(q: QName) -> &'static str {
    match q.local {
        "rPr" => "a:rPr",
        "endParaRPr" => "a:endParaRPr",
        "pPr" => "a:pPr",
        _ => "a:?",
    }
}

fn append_run(p: &mut LensMut<'_, Paragraph>, text: &str, fmt: Option<&Detached<RunProperties>>) -> Result<()> {
    let mut r = p.insert::<Run>()?;
    if let Some(f) = fmt {
        r.insert_detached(f)?;
    }
    r.insert::<Text>()?.set_text(text)?;
    Ok(())
}

/// What `Paragraph::set_text` will do, decided before any change.
struct ParaPlan {
    first_run: bool,
    remove: Vec<usize>,
    fmt: Option<Detached<RunProperties>>,
}

fn plan_paragraph(p: ParaAt<'_>, index: usize, segments: &[impl AsRef<str>]) -> Result<ParaPlan> {
    let shape = para_shape(p.node)?;
    if !shape.fields.is_empty() {
        return Err(Error::FieldInParagraph { paragraph: index });
    }
    let first_run = !shape.runs.is_empty();
    let needs_fmt = (!first_run && !segments[0].as_ref().is_empty()) || segments[1..].iter().any(|s| !s.as_ref().is_empty());
    let fmt = if needs_fmt { run_format(p, &shape)?.rpr } else { None };
    // What this paragraph gains, checked against the depth limit before any
    // change: new runs (`a:r` + `a:t`, or an `a:t` added to the first run)
    // reach 2 levels below it, breaks 1; nothing when only text changes.
    let first_lacks_t = shape.runs.first().is_some_and(|&r| {
        let run = p.node.children()[r].as_element().expect("index of an element");
        !run.elements().any(|c| c.is(Text::QNAME))
    });
    let levels = if needs_fmt || first_lacks_t {
        2
    } else {
        usize::from(segments.len() > 1)
    };
    check_depth(p.doc, p.path, levels)?;
    // the new runs' `a:rPr` copies go under a new `a:r` in this paragraph
    check_copies(p.doc, p.path, 1, fmt.iter().map(|d| d as &dyn CheckInsert))?;
    let mut remove: Vec<usize> = shape.runs.iter().skip(1).chain(&shape.breaks).copied().collect();
    remove.sort_unstable();
    Ok(ParaPlan { first_run, remove, fmt })
}

fn apply_paragraph(p: &mut LensMut<'_, Paragraph>, plan: &ParaPlan, segments: &[impl AsRef<str>]) -> Result<()> {
    let seg0 = segments[0].as_ref();
    if plan.first_run {
        let mut r = p.nth_child_mut::<Run>(0).ok_or(Error::RunNotFound(0))?;
        r.get_or_insert::<Text>()?.set_text(seg0)?;
    }
    for &i in plan.remove.iter().rev() {
        p.remove_at(i);
    }
    if !plan.first_run && !seg0.is_empty() {
        append_run(p, seg0, plan.fmt.as_ref())?;
    }
    for seg in &segments[1..] {
        p.insert::<Break>()?;
        if !seg.as_ref().is_empty() {
            append_run(p, seg.as_ref(), plan.fmt.as_ref())?;
        }
    }
    Ok(())
}

/// Fills a new, empty `a:p`: `a:pPr` from `ppr`, runs from `segments` and
/// `a:endParaRPr` from `fmt` (each omitted when `None`).
fn fill_new_paragraph(
    p: &mut LensMut<'_, Paragraph>,
    ppr: Option<&Detached<ParagraphProperties>>,
    fmt: &RunFmt,
    segments: &[impl AsRef<str>],
) -> Result<()> {
    if let Some(f) = ppr {
        p.insert_detached(f)?;
    }
    apply_paragraph(p, &ParaPlan { first_run: false, remove: Vec::new(), fmt: fmt.rpr.clone() }, segments)?;
    if let Some(f) = &fmt.end {
        p.insert_detached(f)?;
    }
    Ok(())
}

// Review R2-6: `set_attr` (also called by `set_text` for `xml:space`)
// refuses elements whose generated attribute list is incomplete
// (`IncompleteSpec`), and it would run after an insertion. No element this
// crate inserts or writes may have an incomplete list; checked at compile
// time (copies through `insert_detached` take attributes verbatim and do
// not depend on the list).
const _: () = assert!(
    !RunProperties::ATTRS_INCOMPLETE
        && !ParagraphProperties::ATTRS_INCOMPLETE
        && !EndParagraphRunProperties::ATTRS_INCOMPLETE
        && !Run::ATTRS_INCOMPLETE
        && !Text::ATTRS_INCOMPLETE
        && !Break::ATTRS_INCOMPLETE
        && !Paragraph::ATTRS_INCOMPLETE
        && !pml::TextBody::ATTRS_INCOMPLETE
        && !dml::TextBody::ATTRS_INCOMPLETE,
    "an element Phase 0 writes has an incomplete attribute list (ADR-0005 §3, review R2-6)"
);

// ---------------------------------------------------------------------------
// Guards: references into the text (ADR-0005 §6).

#[derive(Clone, Debug)]
pub(crate) struct Guard {
    shape_id: u32,
    paragraphs: bool,
    characters: bool,
    comments: bool,
}

impl Guard {
    fn new(refs: &SlideRefs, ids: &[u32]) -> Self {
        Guard {
            shape_id: ids[0],
            paragraphs: refs.paragraphs.covers(ids),
            characters: refs.characters.covers(ids),
            comments: refs.comments.covers(ids),
        }
    }
    /// Any change to the text of this body.
    pub(crate) fn text(&self) -> Result<()> {
        if self.characters {
            return Err(Error::AnimatedCharacters { shape_id: self.shape_id });
        }
        if self.comments {
            return Err(Error::CommentAnchoredText { shape_id: self.shape_id });
        }
        Ok(())
    }
    /// A change of the number or order of paragraphs (plus [`Self::text`]).
    fn structure(&self) -> Result<()> {
        if self.paragraphs {
            return Err(Error::AnimatedParagraphs { shape_id: self.shape_id });
        }
        self.text()
    }
}

// ---------------------------------------------------------------------------
// Public types

/// A slide opened for editing. Nothing is marked dirty until a setter
/// changes something.
#[derive(Debug)]
pub struct SlideMut<'a> {
    pub(crate) doc: &'a mut XmlDocument,
    pub(crate) info: SlideInfo,
    pub(crate) refs: SlideRefs,
}

impl<'a> SlideMut<'a> {
    /// Position, `sldId` and part name.
    pub fn info(&self) -> &SlideInfo {
        &self.info
    }

    /// Read access to the same slide.
    pub fn as_slide(&self) -> Slide<'_> {
        Slide { doc: self.doc, info: self.info.clone() }
    }

    /// The shape at `path` (from [`crate::ShapeRef::path`]).
    ///
    /// Errors: [`Error::ShapeNotFound`], [`Error::InsideAlternateContent`],
    /// [`Error::InvalidShapeId`] (the shape or an enclosing group has no
    /// usable `cNvPr@id`, so references to it cannot be checked).
    pub fn shape_mut(&mut self, path: &ShapePath) -> Result<ShapeMut<'_>> {
        let p = path.as_slice();
        let root = self.doc.root();
        if p.len() < 3 || !child_is(root, p[0], CommonSlideData::QNAME) {
            return Err(Error::ShapeNotFound);
        }
        let csld = node_at(root, &p[..1]).ok_or(Error::ShapeNotFound)?;
        if !child_is(csld, p[1], ShapeTree::QNAME) {
            return Err(Error::ShapeNotFound);
        }
        let mut ids = Vec::new();
        let mut n = node_at(root, &p[..2]).ok_or(Error::ShapeNotFound)?;
        for (k, &i) in p[2..].iter().enumerate() {
            let c = n.children().get(i).and_then(RawItem::as_element).ok_or(Error::ShapeNotFound)?;
            if c.is(ooxml_core::name::MC_ALTERNATE_CONTENT) {
                return Err(Error::InsideAlternateContent);
            }
            let last = k + 3 == p.len();
            if !last && !c.is(GroupShape::QNAME) {
                return Err(Error::ShapeNotFound);
            }
            if last && read::kind_of(c).is_none() {
                return Err(Error::ShapeNotFound);
            }
            ids.push(raw_shape_id(c).ok_or(Error::InvalidShapeId)?);
            n = c;
        }
        ids.reverse(); // own id first, then enclosing groups
        let guard = Guard::new(&self.refs, &ids);
        Ok(ShapeMut { doc: &mut *self.doc, path: p.to_vec(), guard })
    }

    /// The shape with this `cNvPr@id` ([`Error::ShapeNotFound`],
    /// [`Error::AmbiguousShapeId`], [`Error::InsideAlternateContent`]).
    pub fn shape_mut_by_id(&mut self, id: u32) -> Result<ShapeMut<'_>> {
        let found = find_by_id(read::shapes_of(self.doc), id)?.ok_or(Error::ShapeNotFound)?;
        if *found.kind() == read::ShapeKind::AlternateContent {
            return Err(Error::InsideAlternateContent);
        }
        let path = found.path();
        self.shape_mut(&path)
    }
}

/// A shape opened for editing.
#[derive(Debug)]
pub struct ShapeMut<'a> {
    doc: &'a mut XmlDocument,
    path: Vec<usize>,
    guard: Guard,
}

impl ShapeMut<'_> {
    fn node(&self) -> &RawNode {
        node_at(self.doc.root(), &self.path).expect("checked in shape_mut")
    }

    /// The shape's `p:txBody` ([`Error::NoTextBody`] if the shape is not a
    /// `p:sp` or has none; creating one is out of Phase 0 scope).
    pub fn text_body_mut(&mut self) -> Result<TextBodyMut<'_>> {
        let n = self.node();
        if !n.is(Shape::QNAME) {
            return Err(Error::NoTextBody);
        }
        let (i, _) = n.indexed_elements().find(|(_, c)| c.is(pml::TextBody::QNAME)).ok_or(Error::NoTextBody)?;
        let mut path = self.path.clone();
        path.push(i);
        Ok(TextBodyMut { doc: &mut *self.doc, path, guard: self.guard.clone() })
    }

    /// The text body of table cell (`row`-th `a:tr`, `col`-th `a:tc`).
    ///
    /// Errors: [`Error::NotATable`], [`Error::CellNotFound`],
    /// [`Error::MergedCell`] (`hMerge`/`vMerge`), [`Error::NoTextBody`].
    pub fn table_cell_mut(&mut self, row: usize, col: usize) -> Result<TextBodyMut<'_>> {
        let frame = self.node();
        if !frame.is(GraphicFrame::QNAME) {
            return Err(Error::NotATable);
        }
        let find = |n: &RawNode, q: QName| n.indexed_elements().find(|(_, c)| c.is(q)).map(|(i, _)| i);
        let gi = find(frame, Graphic::QNAME).ok_or(Error::NotATable)?;
        let graphic = node_at(frame, &[gi]).expect("found");
        let di = find(graphic, GraphicData::QNAME).ok_or(Error::NotATable)?;
        let gd = node_at(graphic, &[di]).expect("found");
        if gd.attr(QName::new("", "uri")) != Some(crate::names::TABLE_URI) {
            return Err(Error::NotATable);
        }
        let ti = find(gd, Table::QNAME).ok_or(Error::NotATable)?;
        let tbl = node_at(gd, &[ti]).expect("found");
        let nf = Error::CellNotFound { row, col };
        let (ri, tr) = tbl.indexed_elements().filter(|(_, c)| c.is(TableRow::QNAME)).nth(row).ok_or(nf)?;
        let (ci, tc) = tr
            .indexed_elements()
            .filter(|(_, c)| c.is(TableCell::QNAME))
            .nth(col)
            .ok_or(Error::CellNotFound { row, col })?;
        let v = TableCellView(ooxml_core::Lens::root(tc)?);
        if v.horizontal_merge()? || v.vertical_merge()? {
            return Err(Error::MergedCell { row, col });
        }
        let bi = find(tc, dml::TextBody::QNAME).ok_or(Error::NoTextBody)?;
        let mut path = self.path.clone();
        path.extend([gi, di, ti, ri, ci, bi]);
        Ok(TextBodyMut { doc: &mut *self.doc, path, guard: self.guard.clone() })
    }
}

/// A text body opened for editing (`p:txBody` or a cell's `a:txBody`).
#[derive(Debug)]
pub struct TextBodyMut<'a> {
    doc: &'a mut XmlDocument,
    path: Vec<usize>,
    guard: Guard,
}

impl TextBodyMut<'_> {
    fn node(&self) -> &RawNode {
        node_at(self.doc.root(), &self.path).expect("checked on creation")
    }

    /// The references guarding this body (ADR-0005 §6).
    pub(crate) fn guard(&self) -> &Guard {
        &self.guard
    }

    /// Read access to the current content.
    pub fn as_ref(&self) -> TextBodyRef<'_> {
        TextBodyRef { node: self.node() }
    }

    /// Number of paragraphs.
    pub fn paragraph_count(&self) -> usize {
        self.node().elements().filter(|c| c.is(Paragraph::QNAME)).count()
    }

    fn para_child(&self, index: usize) -> Result<usize> {
        self.node()
            .indexed_elements()
            .filter(|(_, c)| c.is(Paragraph::QNAME))
            .nth(index)
            .map(|(i, _)| i)
            .ok_or(Error::ParagraphNotFound(index))
    }

    /// The `index`-th paragraph.
    pub fn paragraph_mut(&mut self, index: usize) -> Result<ParagraphMut<'_>> {
        self.para_child(index)?;
        Ok(ParagraphMut { doc: &mut *self.doc, loc: ParaLoc { body: self.path.clone(), guard: self.guard.clone(), index } })
    }

    /// Appends a paragraph after the last `a:p`. `\v` becomes `a:br`. The new
    /// paragraph and its runs have **no** `a:pPr`/`a:rPr`: they take the
    /// list/placeholder style (as python-pptx's `add_paragraph`). Use
    /// [`Self::set_text`] for paragraphs that copy their neighbour's format.
    ///
    /// Errors: [`Error::LineBreakInParagraph`], [`Error::InvalidXmlChar`],
    /// [`Error::AnimatedParagraphs`], [`Error::AnimatedCharacters`],
    /// [`Error::CommentAnchoredText`], [`Error::UnsupportedContent`].
    pub fn add_paragraph(&mut self, text: &str) -> Result<ParagraphMut<'_>> {
        let segments = text::paragraph_segments(text)?;
        self.guard.structure()?;
        let count = body_paragraphs(self.node())?.len();
        check_depth(self.doc, &self.path, 3)?; // a:p / a:r / a:t
        let mut body = body_lens(self.doc, &self.path)?;
        with_body!(&mut body, l => {
            let mut p = append_paragraph(l)?;
            fill_new_paragraph(&mut p, None, &RunFmt::default(), &segments)?;
        });
        Ok(ParagraphMut {
            doc: &mut *self.doc,
            loc: ParaLoc { body: self.path.clone(), guard: self.guard.clone(), index: count },
        })
    }

    /// Removes the `index`-th paragraph (never the last one).
    ///
    /// Errors: [`Error::ParagraphNotFound`], [`Error::LastParagraph`],
    /// [`Error::FieldInParagraph`], [`Error::UnsupportedContent`], and the
    /// structure guards.
    pub fn remove_paragraph(&mut self, index: usize) -> Result<()> {
        let paras = body_paragraphs(self.node())?;
        let &child = paras.get(index).ok_or(Error::ParagraphNotFound(index))?;
        if paras.len() == 1 {
            return Err(Error::LastParagraph);
        }
        self.guard.structure()?;
        let shape = para_shape(node_at(self.node(), &[child]).expect("found"))?;
        if !shape.fields.is_empty() {
            return Err(Error::FieldInParagraph { paragraph: index });
        }
        let mut body = body_lens(self.doc, &self.path)?;
        with_body!(&mut body, l => { l.remove_at(child); });
        Ok(())
    }

    /// Replaces the whole text. `\r\n` and `\r` become `\n`; each line
    /// goes into the existing paragraph with the same index through
    /// [`ParagraphMut::set_text`] (keeping its `a:pPr` and first run's
    /// `a:rPr`); surplus paragraphs are removed; missing ones are appended
    /// with a copy of the last paragraph's `a:pPr`, and of its run format
    /// source (without links) for their runs and their `a:endParaRPr`.
    ///
    /// Errors: those of [`ParagraphMut::set_text`] for every line, plus
    /// [`Error::FieldInParagraph`] if any paragraph has a field (it would be
    /// rewritten or removed), [`Error::FormatNotCopyable`] for the new
    /// paragraphs' source, [`Error::Core`]`(NamespaceNotInScope)` when a
    /// copied format uses a namespace without a usable prefix in the body,
    /// and [`Error::AnimatedParagraphs`] when the number of paragraphs
    /// changes.
    pub fn set_text(&mut self, text: &str) -> Result<()> {
        let lines = text::body_lines(text)?;
        let node = self.node();
        let paras = body_paragraphs(node)?;
        if lines.len() == paras.len() {
            self.guard.text()?;
        } else {
            self.guard.structure()?;
        }
        let doc: &XmlDocument = self.doc;
        let para_path = |child: usize| [self.path.as_slice(), &[child]].concat();
        let mut plans = Vec::new();
        for (k, &child) in paras.iter().enumerate() {
            let p = node_at(node, &[child]).expect("found");
            let shape = para_shape(p)?;
            if !shape.fields.is_empty() {
                return Err(Error::FieldInParagraph { paragraph: k });
            }
            if let Some(segs) = lines.get(k) {
                let path = para_path(child);
                plans.push(plan_paragraph(ParaAt::new(doc, &path), k, segs)?);
            }
        }
        let extra = &lines[paras.len().min(lines.len())..];
        // New paragraphs copy from the last one (consulted for every new
        // paragraph, even an empty one, because of `a:endParaRPr`).
        let (ppr, run_fmt) = if extra.is_empty() {
            (None, RunFmt::default())
        } else {
            let path = para_path(*paras.last().expect("body_paragraphs: a:p+"));
            let needs_run = extra.iter().any(|segs| segs.iter().any(|s| !s.is_empty()));
            new_para_format(ParaAt::new(doc, &path), needs_run)?
        };
        if !extra.is_empty() {
            // new paragraphs under the body: a:p / a:r / a:t; a:pPr and
            // a:endParaRPr one level below the body, a:rPr two
            check_depth(doc, &self.path, 3)?;
            check_copies(doc, &self.path, 1, ppr.iter().map(|d| d as &dyn CheckInsert).chain(run_fmt.end.iter().map(|d| d as &dyn CheckInsert)))?;
            check_copies(doc, &self.path, 2, run_fmt.rpr.iter().map(|d| d as &dyn CheckInsert))?;
        }
        // Apply: appends first (the only primitive that could still be
        // refused by the core, before anything else changed), then rewrite,
        // then remove surplus paragraphs from the end.
        let mut body = body_lens(self.doc, &self.path)?;
        with_body!(&mut body, l => {
            for segs in extra {
                let mut p = append_paragraph(l)?;
                fill_new_paragraph(&mut p, ppr.as_ref(), &run_fmt, segs)?;
            }
        });
        for (k, plan) in plans.iter().enumerate() {
            let body = body_lens(self.doc, &self.path)?;
            let mut p = with_body!(body, l => paragraph_at(l, paras[k])?);
            apply_paragraph(&mut p, plan, &lines[k])?;
        }
        if lines.len() < paras.len() {
            let mut body = body_lens(self.doc, &self.path)?;
            for &child in paras[lines.len()..].iter().rev() {
                with_body!(&mut body, l => { l.remove_at(child); });
            }
        }
        Ok(())
    }
}

/// Where a paragraph is: its body's node path, the guard, its index.
#[derive(Clone, Debug)]
struct ParaLoc {
    body: Vec<usize>,
    guard: Guard,
    index: usize,
}

impl ParaLoc {
    fn child(&self, doc: &XmlDocument) -> Result<usize> {
        let body = node_at(doc.root(), &self.body).expect("checked on creation");
        body.indexed_elements()
            .filter(|(_, c)| c.is(Paragraph::QNAME))
            .nth(self.index)
            .map(|(i, _)| i)
            .ok_or(Error::ParagraphNotFound(self.index))
    }

    fn node<'d>(&self, doc: &'d XmlDocument) -> Result<&'d RawNode> {
        let c = self.child(doc)?;
        Ok(node_at(doc.root(), &self.body).and_then(|b| node_at(b, &[c])).expect("found"))
    }

    fn lens<'d>(&self, doc: &'d mut XmlDocument) -> Result<LensMut<'d, Paragraph>> {
        let c = self.child(doc)?;
        let body = body_lens(doc, &self.body)?;
        with_body!(body, l => paragraph_at(l, c))
    }
}

/// A paragraph opened for editing.
#[derive(Debug)]
pub struct ParagraphMut<'b> {
    doc: &'b mut XmlDocument,
    loc: ParaLoc,
}

impl ParagraphMut<'_> {
    fn node(&self) -> Result<&RawNode> {
        self.loc.node(self.doc)
    }

    fn lens(&mut self) -> Result<LensMut<'_, Paragraph>> {
        self.loc.lens(self.doc)
    }

    /// Read access to the current content.
    pub fn as_ref(&self) -> Result<crate::ParagraphRef<'_>> {
        Ok(crate::ParagraphRef { node: self.node()? })
    }

    /// Number of runs (`a:r`).
    pub fn run_count(&self) -> Result<usize> {
        Ok(self.node()?.elements().filter(|c| c.is(Run::QNAME)).count())
    }

    /// The `index`-th run (`a:r`; breaks and fields are not counted).
    pub fn run_mut(&mut self, index: usize) -> Result<RunMut<'_>> {
        if index >= self.run_count()? {
            return Err(Error::RunNotFound(index));
        }
        Ok(RunMut { doc: &mut *self.doc, loc: self.loc.clone(), index })
    }

    /// Replaces the paragraph's text: the first run keeps its `a:rPr` and
    /// gets the text before the first `\v`; other runs and breaks are
    /// removed; each `\v` becomes an `a:br` followed by a run with a copy
    /// of the first run's `a:rPr` (whole subtree, without
    /// `a:hlinkClick`/`a:hlinkMouseOver`: a link stays on its run). A
    /// paragraph without runs gets a run whose `a:rPr` is such a copy of
    /// `a:endParaRPr` (what PowerPoint does when typing into an empty
    /// paragraph). Empty segments make no run.
    ///
    /// Errors: [`Error::LineBreakInParagraph`], [`Error::InvalidXmlChar`],
    /// [`Error::FieldInParagraph`], [`Error::UnsupportedContent`],
    /// [`Error::FormatNotCopyable`] and [`Error::Core`]`(NamespaceNotInScope)`
    /// (only when a new run is needed),
    /// [`Error::AnimatedCharacters`], [`Error::CommentAnchoredText`].
    pub fn set_text(&mut self, text: &str) -> Result<()> {
        let segments = text::paragraph_segments(text)?;
        self.loc.guard.text()?;
        let mut path = self.loc.body.clone();
        path.push(self.loc.child(self.doc)?);
        let plan = plan_paragraph(ParaAt::new(self.doc, &path), self.loc.index, &segments)?;
        let mut p = self.lens()?;
        apply_paragraph(&mut p, &plan, &segments)
    }

    /// Appends a run **without** `a:rPr` (it takes the paragraph/list
    /// style), before `a:endParaRPr`: the explicit "unformatted" path.
    ///
    /// Errors: [`Error::LineBreakInRun`], [`Error::InvalidXmlChar`],
    /// [`Error::UnsupportedContent`], the text guards.
    pub fn add_run(&mut self, text: &str) -> Result<()> {
        text::check_run_text(text)?;
        self.loc.guard.text()?;
        para_shape(self.node()?)?;
        let mut path = self.loc.body.clone();
        path.push(self.loc.child(self.doc)?);
        check_depth(self.doc, &path, 2)?; // a:r / a:t
        let mut p = self.lens()?;
        append_run(&mut p, text, None)
    }

    /// Appends an `a:br` before `a:endParaRPr`.
    pub fn add_break(&mut self) -> Result<()> {
        self.loc.guard.text()?;
        para_shape(self.node()?)?;
        self.lens()?.insert::<Break>()?;
        Ok(())
    }

    /// Removes the `index`-th run.
    pub fn remove_run(&mut self, index: usize) -> Result<()> {
        self.loc.guard.text()?;
        let shape = para_shape(self.node()?)?;
        let &c = shape.runs.get(index).ok_or(Error::RunNotFound(index))?;
        self.lens()?.remove_at(c);
        Ok(())
    }

    /// Removes the `index`-th field (`a:fld`): the explicit way to drop a
    /// slide number or date before rewriting a paragraph (review M2).
    pub fn remove_field(&mut self, index: usize) -> Result<()> {
        self.loc.guard.text()?;
        let shape = para_shape(self.node()?)?;
        let &c = shape.fields.get(index).ok_or(Error::FieldNotFound(index))?;
        self.lens()?.remove_at(c);
        Ok(())
    }
}

/// A run opened for editing.
#[derive(Debug)]
pub struct RunMut<'c> {
    doc: &'c mut XmlDocument,
    loc: ParaLoc,
    index: usize,
}

impl RunMut<'_> {
    /// Replaces the run's `a:t` text; its `a:rPr` is kept.
    ///
    /// Errors: [`Error::LineBreakInRun`], [`Error::InvalidXmlChar`],
    /// [`Error::UnsupportedContent`], [`Error::AnimatedCharacters`],
    /// [`Error::CommentAnchoredText`].
    pub fn set_text(&mut self, text: &str) -> Result<()> {
        text::check_run_text(text)?;
        self.loc.guard.text()?;
        para_shape(self.loc.node(self.doc)?)?;
        let mut p = self.loc.lens(self.doc)?;
        let mut r = p.nth_child_mut::<Run>(self.index).ok_or(Error::RunNotFound(self.index))?;
        r.get_or_insert::<Text>()?.set_text(text)?;
        Ok(())
    }
}
