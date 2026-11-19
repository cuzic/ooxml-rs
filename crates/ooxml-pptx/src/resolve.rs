//! The inheritance resolver (ADR-0008 §1, Accepted revision 3).
//!
//! Every value comes back as a [`Lookup`]: `Set` with its [`Source`] (which
//! part and which step supplied it), `Unset` (no element of the chain sets
//! it: the application default applies, which Phase 1 never invents), or
//! `Unresolved` with a [`Reason`].
//!
//! *Set* means present on the raw element (§1.3 "What *set* means"): the
//! resolver tests attribute / child presence on the raw tree and parses the
//! present value; XSD defaults never stop a chain. Attributes of a group's
//! member element (autofit scales, `startAt`, a font's `pitchFamily` /
//! `charset`, `xfrm` `rot` / `flip*`) are read only from the element that won
//! the group; each is a [`WinnerAttr`]: the value (an `AttrError` with the
//! raw text when invalid) or, absent there, that element's XSD default with
//! `defaulted: true` (review r2-m1, #56 m3).
//!
//! Enumerated attributes are returned as [`EnumValue`] of the generated
//! simple type (ADR-0002 owner decision Q2, #56 M2); [`RunProps::xsd_default`]
//! and friends give the schema default for an `Unset` property (§1.3).
//!
//! Values are **as specified**, not as displayed (§1.6): autofit scales and
//! colour transforms are not applied; a linked run's display colour follows
//! [`crate::Link::color_rule`].
//!
//! Contexts (§1.7): a [`Sheet`] is a slide, notes slide, layout or master
//! with the parts its shapes inherit from; [`DeckSlide`] delegates to its
//! slide `Sheet`.

use ooxml_core::{AttrError, ElementSpec, EnumType, EnumValue, QName, RawNode};
use ooxml_schema::dml::{
    BodyProperties, ColorSchemeIndexValues, ParagraphProperties, ShapeTypeValues, RunProperties, TextAlignmentTypeValues, TextAnchoringTypeValues, TextAutoNumberSchemeValues,
    TextCapsValues, TextFontAlignmentValues, TextStrikeValues, TextUnderlineValues, TextVerticalValues, TextWrappingValues,
};
use ooxml_core::value::{DmlPercent, parse_dml_percent};

use crate::deck::{DeckSlide, Sheet, SheetKind, a, child, p, un};
use crate::error::Result;
use crate::read::{ParagraphRef, ShapeRef, TextBodyRef, ac_representative};

// ---------------------------------------------------------------------------
// Public result types

/// One property after the chain (ADR-0008 §5).
#[derive(Clone, Debug, PartialEq)]
pub enum Lookup<'a, T> {
    /// An element of the chain sets it.
    Set(Resolved<'a, T>),
    /// No element sets it: the application's default applies (never invented).
    Unset,
    /// The chain cannot decide in Phase 1.
    Unresolved(Reason<'a>),
}

impl<'a, T> Lookup<'a, T> {
    /// The value when `Set` and valid.
    pub fn value(&self) -> Option<&T> {
        match self {
            Lookup::Set(r) => r.value.as_ref().ok(),
            _ => None,
        }
    }
    /// The source when `Set`.
    pub fn source(&self) -> Option<&Source<'a>> {
        match self {
            Lookup::Set(r) => Some(&r.source),
            _ => None,
        }
    }
}

/// A value with its provenance.
#[derive(Clone, Debug, PartialEq)]
pub struct Resolved<'a, T> {
    /// The value, or the raw text when it is outside the type's lexical
    /// space (ADR-0002 §1.2, "never lossy").
    pub value: std::result::Result<T, AttrError>,
    /// Where it came from.
    pub source: Source<'a>,
    /// The value is the XSD default of an attribute absent on the element
    /// that won its group (an autofit scale on the shape itself, §1.6).
    /// Attributes inside a value carry their own flag ([`WinnerAttr`]).
    pub defaulted: bool,
}

/// An attribute read only from the element that won its group (§1.3,
/// review r2-m1): present → its value (`Err` with the raw text when
/// invalid); absent → the XSD default, `defaulted: true`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WinnerAttr<T> {
    /// The value or the invalid raw text.
    pub value: std::result::Result<T, AttrError>,
    /// The attribute is absent on the winning element (XSD default).
    pub defaulted: bool,
}

impl<T: Copy> WinnerAttr<T> {
    /// The value when valid.
    pub fn get(&self) -> Option<T> {
        self.value.as_ref().ok().copied()
    }
}

fn winner<T>(n: &RawNode, name: &'static str, default: T, parse: impl Fn(&str) -> Option<T>) -> WinnerAttr<T> {
    match n.attr(un(name)) {
        None => WinnerAttr { value: Ok(default), defaulted: true },
        Some(raw) => WinnerAttr { value: parse(raw).ok_or_else(|| attr_error(name, raw)), defaulted: false },
    }
}

fn defaulted<T>(v: T) -> WinnerAttr<T> {
    WinnerAttr { value: Ok(v), defaulted: true }
}

/// Why a lookup is not decided.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Reason<'a> {
    /// Table-cell text below the cell's own list style: table styles are Phase 3.
    TableStyle,
    /// `a:schemeClr val="phClr"`: only meaningful inside a theme style matrix.
    PlaceholderColor,
    /// `a:pPr@lvl` is not an integer 0–8: level-dependent steps cannot run (r2-m9).
    InvalidLevel,
    /// A placeholder in a group: geometry is in the group's coordinates (§1.5).
    GroupCoordinates,
    /// The slide or its layout has a `themeOverride`, which Phase 1 does not apply (r2-m7).
    ThemeOverride,
    /// `normAutofit` scales from a layout or master: a cache of another shape (§1.6).
    InheritedAutofitScale {
        /// The element that had the scale.
        from: Source<'a>,
    },
    /// An empty or missing theme typeface: the script fonts apply (§1.4).
    ScriptDependent {
        /// `+mn-ea` etc.
        reference: &'a str,
        /// The theme's `(script, typeface)` entries of that font collection.
        script_fonts: Vec<(&'a str, &'a str)>,
    },
    /// The chain is broken (missing layout, master or theme).
    BrokenChain,
}

/// Which chain step supplied a value.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Source<'a> {
    /// On the element asked about (`a:rPr`, `a:pPr`, `a:bodyPr`, `a:xfrm`).
    Direct,
    /// The paragraph's `a:pPr/a:defRPr` (step 2).
    ParagraphDefault,
    /// The shape's (or cell's) own `a:lstStyle` at `level`.
    ShapeListStyle {
        /// List level (0-based).
        level: u8,
    },
    /// The shape's own `p:style/a:fontRef` (step 4).
    ShapeStyle,
    /// A layout or master placeholder.
    Placeholder {
        /// Layout or master.
        role: Role,
        /// Part name.
        part: &'a str,
        /// `cNvPr@id` of that placeholder (`None` if it has none).
        shape_id: Option<u32>,
        /// How it was matched (§1.2).
        matched: MatchKind,
        /// List level for text properties.
        level: Option<u8>,
    },
    /// The master's `p:txStyles` (or another text style).
    MasterTextStyle {
        /// Master part.
        part: &'a str,
        /// Which style.
        style: TextStyleKind,
        /// List level.
        level: u8,
    },
    /// The presentation's `p:defaultTextStyle`.
    PresentationDefault {
        /// List level.
        level: u8,
    },
}

/// Layout or master.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Role {
    /// The slide layout.
    Layout,
    /// The slide master.
    Master,
    /// The notes master (§1.7).
    NotesMaster,
}

/// `p:txStyles` member.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum TextStyleKind {
    /// `p:titleStyle`.
    Title,
    /// `p:bodyStyle`.
    Body,
    /// `p:otherStyle`.
    Other,
    /// The notes master's `p:notesStyle` (§1.3 "Notes").
    Notes,
}

/// How a slide placeholder found its layout placeholder (ADR-0008 §1.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum MatchKind {
    /// Same effective idx, same type.
    Idx,
    /// Same idx, different type of the same family.
    IdxFamily,
    /// Several layout placeholders share the idx; exact type broke the tie.
    IdxTieType,
    /// Several share the idx; the family broke the tie.
    IdxTieFamily,
    /// Explicit `@idx` pointing at another family (taken anyway).
    IdxTypeMismatch,
    /// No usable idx candidate: exact type.
    Type,
    /// No usable idx candidate: same family.
    Family,
    /// No layout placeholder: resolved from the master by family.
    Orphan,
    /// Layout → master (by the layout placeholder's family). A value from
    /// a slide's master placeholder reports the slide placeholder's own
    /// match kind instead (§1.2: "every resolved value … reports the
    /// `MatchKind`").
    MasterFamily,
    /// A placeholder on a master: it inherits from no placeholder (§1.7).
    OnMaster,
}

/// The layout and master placeholders a slide placeholder inherits from.
#[derive(Clone, Debug)]
pub struct PlaceholderMatch<'a> {
    /// The layout placeholder (`None` for an orphan).
    pub layout: Option<&'a RawNode>,
    /// The master placeholder of the family (`None` if the master has none).
    pub master: Option<&'a RawNode>,
    /// How the layout placeholder was found.
    pub kind: MatchKind,
    /// "The first" of several equal candidates was taken (§1.2).
    pub ambiguous: bool,
    /// Number of equal candidates when `ambiguous`.
    pub candidates: usize,
    /// The slide placeholder's family (selects `p:txStyles`, §1.3 step 7).
    pub family: Family,
}

/// Placeholder family (§1.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    /// `title`, `ctrTitle`.
    Title,
    /// Everything else with text (`body`, `obj`, `subTitle`, `chart`, …).
    Body,
    /// `dt`.
    Date,
    /// `ftr`.
    Footer,
    /// `sldNum`.
    SlideNumber,
    /// `hdr`.
    Header,
    /// `sldImg` (notes).
    SlideImage,
}

impl Family {
    fn of(ty: &str) -> Family {
        match ty {
            "title" | "ctrTitle" => Family::Title,
            "dt" => Family::Date,
            "ftr" => Family::Footer,
            "sldNum" => Family::SlideNumber,
            "hdr" => Family::Header,
            "sldImg" => Family::SlideImage,
            _ => Family::Body,
        }
    }
    fn text_style(self) -> Option<TextStyleKind> {
        match self {
            Family::Title => Some(TextStyleKind::Title),
            Family::Body => Some(TextStyleKind::Body),
            Family::Date | Family::Footer | Family::SlideNumber | Family::Header => Some(TextStyleKind::Other),
            Family::SlideImage => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Placeholder matching (§1.2)

#[derive(Clone, Copy)]
struct Ph<'a> {
    shape: &'a RawNode,
    ty: &'a str,
    idx: Option<u32>,
    has_idx: bool,
    in_group: bool,
}

fn nv_ph(shape: &RawNode) -> Option<&RawNode> {
    let nv = shape.elements().find(|c| c.name().local().starts_with("nv"))?;
    child(child(nv, p("nvPr"))?, p("ph"))
}

fn ph_of(shape: &RawNode, in_group: bool) -> Option<Ph<'_>> {
    let ph = nv_ph(shape)?;
    let raw_idx = ph.attr(un("idx"));
    Some(Ph {
        shape,
        ty: ph.attr(un("type")).unwrap_or("obj"),
        idx: raw_idx.map_or(Some(0), |v| v.parse().ok()),
        has_idx: raw_idx.is_some(),
        in_group,
    })
}

fn tree_of(root: &RawNode) -> Option<&RawNode> {
    child(child(root, p("cSld"))?, p("spTree"))
}

/// Every placeholder of a part's shape tree, group members included.
fn placeholders(root: &RawNode) -> Vec<Ph<'_>> {
    fn walk<'a>(n: &'a RawNode, in_group: bool, out: &mut Vec<Ph<'a>>) {
        for c in n.elements() {
            if c.is(p("grpSp")) {
                walk(c, true, out);
            } else if c.is(ooxml_core::name::MC_ALTERNATE_CONTENT) {
                // the branch a reader uses (as for slide shapes, #56 m4)
                if let Some(ph) = ac_representative(c).and_then(|r| ph_of(r, in_group)) {
                    out.push(ph);
                }
            } else if let Some(ph) = ph_of(c, in_group) {
                out.push(ph);
            }
        }
    }
    let mut out = Vec::new();
    if let Some(t) = tree_of(root) {
        walk(t, false, &mut out);
    }
    out
}

/// Step 3's choice: idx 0 preferred, else the first. Returns the pick and
/// the number of equal candidates it was chosen from (those with idx 0 if
/// any has it, else all): "ambiguous" means more than one (#56 nit).
fn first_canon<'a>(v: &[Ph<'a>]) -> Option<(Ph<'a>, usize)> {
    let zero = v.iter().filter(|x| x.idx == Some(0)).count();
    let equal = if zero > 0 { zero } else { v.len() };
    v.iter().find(|x| x.idx == Some(0)).or(v.first()).map(|f| (*f, equal))
}

/// §1.2: slide → layout. Returns (layout placeholder, kind, ambiguous, candidates).
fn match_layout<'a>(sp: &Ph<'_>, lph: &[Ph<'a>]) -> (Option<Ph<'a>>, MatchKind, bool, usize) {
    let fam = |t: &str| Family::of(t);
    let by_idx: Vec<Ph<'a>> = lph.iter().filter(|x| x.idx.is_some() && x.idx == sp.idx).copied().collect();
    let tie = by_idx.len() > 1;
    let same: Vec<Ph<'a>> = by_idx.iter().filter(|x| x.ty == sp.ty).copied().collect();
    if let Some(&f) = same.first() {
        return (Some(f), if tie { MatchKind::IdxTieType } else { MatchKind::Idx }, same.len() > 1, same.len());
    }
    let famc: Vec<Ph<'a>> = by_idx.iter().filter(|x| fam(x.ty) == fam(sp.ty)).copied().collect();
    if let Some(&f) = famc.first() {
        return (Some(f), if tie { MatchKind::IdxTieFamily } else { MatchKind::IdxFamily }, famc.len() > 1, famc.len());
    }
    if let (Some(&f), true) = (by_idx.first(), sp.has_idx) {
        return (Some(f), MatchKind::IdxTypeMismatch, tie, by_idx.len());
    }
    let by_type: Vec<Ph<'a>> = lph.iter().filter(|x| x.ty == sp.ty).copied().collect();
    if let Some((f, n)) = first_canon(&by_type) {
        return (Some(f), MatchKind::Type, n > 1, n);
    }
    let by_fam: Vec<Ph<'a>> = lph.iter().filter(|x| fam(x.ty) == fam(sp.ty)).copied().collect();
    if let Some((f, n)) = first_canon(&by_fam) {
        return (Some(f), MatchKind::Family, n > 1, n);
    }
    (None, MatchKind::Orphan, false, 0)
}

fn shape_id(shape: &RawNode) -> Option<u32> {
    crate::read::raw_shape_id(shape)
}

// ---------------------------------------------------------------------------
// The text-property chain (§1.3)

/// A run-like item whose properties are resolved.
#[derive(Clone, Copy, Debug)]
pub enum RunItem<'a> {
    /// An `a:r`.
    Run(crate::RunRef<'a>),
    /// An `a:fld`.
    Field(crate::FieldRef<'a>),
    /// The paragraph end (`a:endParaRPr`).
    End,
}

/// One step of a run-property chain: the source and the `a:rPr`-like element.
#[derive(Clone, Debug)]
struct Step<'a> {
    source: Source<'a>,
    node: Option<&'a RawNode>,
    /// `p:style/a:fontRef` (fonts and fill only).
    font_ref: bool,
}

/// What the chain ends with when nothing is set.
#[derive(Clone, Debug)]
enum Tail<'a> {
    Unset,
    Unresolved(Reason<'a>),
}

struct Chain<'a> {
    steps: Vec<Step<'a>>,
    /// After the level-independent steps, the remaining ones are not
    /// consulted and this is returned (invalid level, r2-m9).
    stop_after: Option<(usize, Reason<'a>)>,
    tail: Tail<'a>,
    theme: Option<&'a RawNode>,
    theme_override: bool,
    clr_map: Option<ClrMap<'a>>,
}

fn level_el(list: Option<&RawNode>, level: u8) -> Option<&RawNode> {
    let list = list?;
    let name = ["lvl1pPr", "lvl2pPr", "lvl3pPr", "lvl4pPr", "lvl5pPr", "lvl6pPr", "lvl7pPr", "lvl8pPr", "lvl9pPr"][usize::from(level)];
    child(list, a(name))
}

fn lst_style(body: Option<&RawNode>) -> Option<&RawNode> {
    child(body?, a("lstStyle"))
}

fn txbody(shape: &RawNode) -> Option<&RawNode> {
    child(shape, p("txBody"))
}

/// The paragraph level: `Ok(0..=8)`, or `Err` with the raw value.
fn level_of(para: &RawNode) -> std::result::Result<u8, AttrError> {
    let Some(ppr) = child(para, a("pPr")) else { return Ok(0) };
    match ppr.attr(un("lvl")) {
        None => Ok(0),
        Some(v) => v.parse::<u8>().ok().filter(|l| *l <= 8).ok_or_else(|| attr_error("lvl", v)),
    }
}

fn attr_error(name: &'static str, raw: &str) -> AttrError {
    AttrError { attr: QName::new("", name), raw: raw.to_owned() }
}

/// The colour map in effect for a slide (§1.4).
#[derive(Clone, Copy, Debug)]
pub struct ClrMap<'a> {
    /// The `a:overrideClrMapping` or `p:clrMap` element.
    pub node: &'a RawNode,
    /// Where it came from.
    pub from: MapSource,
}

/// Which element supplied the colour map.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum MapSource {
    /// The slide's `p:clrMapOvr/a:overrideClrMapping`.
    SlideOverride,
    /// The layout's override.
    LayoutOverride,
    /// The master's `p:clrMap`.
    Master,
    /// The notes slide's own `p:clrMapOvr/a:overrideClrMapping` (§1.7).
    NotesOverride,
    /// The notes master's `p:clrMap`.
    NotesMaster,
}

impl<'a> Sheet<'a> {
    /// The effective colour map: slide override, else layout override, else
    /// the master's `p:clrMap` (ADR-0008 §1.4; POI's `mapSchemeColor`,
    /// LibreOffice's `layout-clrmap-override.pptx`; ECMA §19.3.1.7 says
    /// `a:masterClrMapping` means the master's, review m2).
    pub fn color_map(&self) -> Result<Option<ClrMap<'a>>> {
        let ovr = |root: &'a RawNode| child(child(root, p("clrMapOvr"))?, a("overrideClrMapping"));
        let own = self.own.root();
        let master = |from| -> Result<Option<ClrMap<'a>>> { Ok(child(self.master()?.root(), p("clrMap")).map(|n| ClrMap { node: n, from })) };
        match self.kind {
            SheetKind::Notes => {
                // §1.7: the notes slide's override, else the notes master's map
                if let Some(n) = ovr(own) {
                    return Ok(Some(ClrMap { node: n, from: MapSource::NotesOverride }));
                }
                master(MapSource::NotesMaster)
            }
            SheetKind::Master => master(MapSource::Master),
            SheetKind::Layout => {
                if let Some(n) = ovr(own) {
                    return Ok(Some(ClrMap { node: n, from: MapSource::LayoutOverride }));
                }
                master(MapSource::Master)
            }
            _ => {
                if let Some(n) = ovr(own) {
                    return Ok(Some(ClrMap { node: n, from: MapSource::SlideOverride }));
                }
                if let Some(n) = ovr(self.layout()?.root()) {
                    return Ok(Some(ClrMap { node: n, from: MapSource::LayoutOverride }));
                }
                master(MapSource::Master)
            }
        }
    }

    /// The layout and master placeholders of a slide placeholder (§1.2);
    /// `None` for a shape that is not a placeholder.
    pub fn placeholder_match(&self, shape: &ShapeRef<'a>) -> Result<Option<PlaceholderMatch<'a>>> {
        let node = representative(shape);
        let Some(sp) = ph_of(node, shape.depth() > 0) else { return Ok(None) };
        let family = Family::of(sp.ty);
        let none = |kind| PlaceholderMatch { layout: None, master: None, kind, ambiguous: false, candidates: 0, family };
        match self.kind {
            // §1.7: a master placeholder inherits from no placeholder
            SheetKind::Master => Ok(Some(none(MatchKind::OnMaster))),
            // §1.7: a layout placeholder → the master placeholder of its family
            SheetKind::Layout => {
                let mph = placeholders(self.master()?.root());
                let mp = mph.iter().find(|m| Family::of(m.ty) == family).map(|m| m.shape);
                Ok(Some(PlaceholderMatch { master: mp, ..none(MatchKind::MasterFamily) }))
            }
            // §1.2: notes slides match the notes master by step 3 only
            // (type, then family); no further master
            SheetKind::Notes => {
                let nph = placeholders(self.master()?.root());
                let by_type: Vec<Ph<'a>> = nph.iter().filter(|x| x.ty == sp.ty).copied().collect();
                let by_fam: Vec<Ph<'a>> = nph.iter().filter(|x| Family::of(x.ty) == family).copied().collect();
                let (found, kind) = match first_canon(&by_type) {
                    Some(f) => (Some(f), MatchKind::Type),
                    None => match first_canon(&by_fam) {
                        Some(f) => (Some(f), MatchKind::Family),
                        None => (None, MatchKind::Orphan),
                    },
                };
                let n = found.map_or(0, |f| f.1);
                Ok(Some(PlaceholderMatch { master: found.map(|f| f.0.shape), ambiguous: n > 1, candidates: n, ..none(kind) }))
            }
            _ => {
                let (layout, master) = (self.layout()?, self.master()?);
                let lph = placeholders(layout.root());
                let mph = placeholders(master.root());
                let (lp, kind, ambiguous, candidates) = match_layout(&sp, &lph);
                let fam_ty = lp.map_or(sp.ty, |l| l.ty);
                let mp = mph.iter().find(|m| Family::of(m.ty) == Family::of(fam_ty)).map(|m| m.shape);
                Ok(Some(PlaceholderMatch { layout: lp.map(|l| l.shape), master: mp, kind, ambiguous, candidates, family }))
            }
        }
    }

    fn chain(&self, shape: &ShapeRef<'a>, body: TextBodyRef<'a>, level: Option<u8>, run_level: bool) -> Result<Chain<'a>> {
        let node = representative(shape);
        let is_cell = body.raw().is(a("txBody"));
        let lvl = level.unwrap_or(0);
        let theme = self.theme().ok().map(|t| t.root());
        let mut steps = Vec::new();
        let clr_map = self.color_map().ok().flatten();
        let mut chain = Chain { steps: Vec::new(), stop_after: None, tail: Tail::Unset, theme, theme_override: self.theme_override, clr_map };
        // steps 1–2 are added by the caller (they depend on the item)
        let pre = |steps: &mut Vec<Step<'a>>, source: Source<'a>, n: Option<&'a RawNode>| steps.push(Step { source, node: n, font_ref: false });
        if level.is_none() {
            // invalid level: stop at the first level-dependent step (r2-m9)
            chain.stop_after = Some((0, Reason::InvalidLevel));
            chain.steps = steps;
            return Ok(chain);
        }
        pre(&mut steps, Source::ShapeListStyle { level: lvl }, level_el(lst_style(Some(body.raw())), lvl));
        if is_cell {
            chain.steps = steps;
            chain.tail = Tail::Unresolved(Reason::TableStyle);
            return Ok(chain);
        }
        if run_level && let Some(fr) = child(node, p("style")).and_then(|s| child(s, a("fontRef"))) {
            steps.push(Step { source: Source::ShapeStyle, node: Some(fr), font_ref: true });
        }
        let notes = self.kind == SheetKind::Notes;
        match self.placeholder_match(shape)? {
            Some(m) if notes => {
                // §1.3 "Notes": the notes-master placeholder, then
                // p:notesStyle for the body family only; no defaultTextStyle
                let nm = self.master()?;
                if let Some(mp) = m.master {
                    pre(&mut steps, Source::Placeholder { role: Role::NotesMaster, part: nm.name, shape_id: shape_id(mp), matched: m.kind, level: Some(lvl) }, level_el(lst_style(txbody(mp)), lvl));
                }
                if m.family == Family::Body {
                    let st = child(nm.root(), p("notesStyle"));
                    pre(&mut steps, Source::MasterTextStyle { part: nm.name, style: TextStyleKind::Notes, level: lvl }, level_el(st, lvl));
                }
            }
            // a non-placeholder on a notes slide: steps 1–4 only (§1.3:
            // p:defaultTextStyle is not consulted for notes)
            None if notes => {}
            Some(m) => {
                let master = self.master()?;
                if let Some(l) = m.layout {
                    let lp = self.layout()?;
                    pre(&mut steps, Source::Placeholder { role: Role::Layout, part: lp.name, shape_id: shape_id(l), matched: m.kind, level: Some(lvl) }, level_el(lst_style(txbody(l)), lvl));
                }
                if let Some(mp) = m.master {
                    pre(&mut steps, Source::Placeholder { role: Role::Master, part: master.name, shape_id: shape_id(mp), matched: m.kind, level: Some(lvl) }, level_el(lst_style(txbody(mp)), lvl));
                }
                if let Some(kind) = m.family.text_style() {
                    let local = match kind {
                        TextStyleKind::Title => "titleStyle",
                        TextStyleKind::Body => "bodyStyle",
                        TextStyleKind::Other | TextStyleKind::Notes => "otherStyle",
                    };
                    let st = child(master.root(), p("txStyles")).and_then(|t| child(t, p(local)));
                    pre(&mut steps, Source::MasterTextStyle { part: master.name, style: kind, level: lvl }, level_el(st, lvl));
                }
                pre(&mut steps, Source::PresentationDefault { level: lvl }, level_el(self.default_text_style, lvl));
            }
            None => {
                pre(&mut steps, Source::PresentationDefault { level: lvl }, level_el(self.default_text_style, lvl));
                let master = self.master()?;
                let st = child(master.root(), p("txStyles")).and_then(|t| child(t, p("otherStyle")));
                pre(&mut steps, Source::MasterTextStyle { part: master.name, style: TextStyleKind::Other, level: lvl }, level_el(st, lvl));
            }
        }
        chain.steps = steps;
        Ok(chain)
    }

    /// Run properties of `item` in paragraph `para` of `body` (the shape's
    /// `p:txBody` or one of its table cells' `a:txBody`), ADR-0008 §1.3.
    pub fn run_props(&self, shape: &ShapeRef<'a>, body: TextBodyRef<'a>, para: ParagraphRef<'a>, item: RunItem<'a>) -> Result<RunProps<'a>> {
        let level = level_of(para.raw());
        let own = match item {
            RunItem::Run(r) => child(r.raw(), a("rPr")),
            RunItem::Field(f) => child(f.raw(), a("rPr")),
            RunItem::End => child(para.raw(), a("endParaRPr")),
        };
        let ppr = child(para.raw(), a("pPr"));
        let mut chain = self.chain(shape, body, level.as_ref().ok().copied(), true)?;
        let mut steps = vec![
            Step { source: Source::Direct, node: own, font_ref: false },
            Step { source: Source::ParagraphDefault, node: ppr.and_then(|n| child(n, a("defRPr"))), font_ref: false },
        ];
        if let Some((_, reason)) = chain.stop_after.take() {
            chain.steps = steps;
            chain.stop_after = Some((2, reason));
        } else {
            // the list-style level elements carry run properties in a:defRPr
            for s in chain.steps.drain(..) {
                let node = if s.font_ref { s.node } else { s.node.and_then(|n| child(n, a("defRPr"))) };
                steps.push(Step { node, ..s });
            }
            chain.steps = steps;
        }
        Ok(RunProps::resolve(&chain))
    }

    /// Paragraph properties (§1.3: the chain without steps 2 and 4).
    pub fn para_props(&self, shape: &ShapeRef<'a>, body: TextBodyRef<'a>, para: ParagraphRef<'a>) -> Result<ParaProps<'a>> {
        let level = level_of(para.raw());
        let mut chain = self.chain(shape, body, level.as_ref().ok().copied(), false)?;
        let mut steps = vec![Step { source: Source::Direct, node: child(para.raw(), a("pPr")), font_ref: false }];
        if let Some((_, reason)) = chain.stop_after.take() {
            chain.stop_after = Some((1, reason));
        } else {
            steps.append(&mut chain.steps);
        }
        chain.steps = steps;
        Ok(ParaProps::resolve(&chain, level))
    }

    /// Body properties: slide `a:bodyPr` → layout placeholder → master
    /// placeholder for placeholders; the shape's own for other shapes.
    pub fn body_props(&self, shape: &ShapeRef<'a>) -> Result<BodyProps<'a>> {
        let node = representative(shape);
        let mut steps = vec![Step { source: Source::Direct, node: txbody(node).and_then(|b| child(b, a("bodyPr"))), font_ref: false }];
        if let Some(m) = self.placeholder_match(shape)? {
            if let Some(l) = m.layout {
                steps.push(Step {
                    source: Source::Placeholder { role: Role::Layout, part: self.layout()?.name, shape_id: shape_id(l), matched: m.kind, level: None },
                    node: txbody(l).and_then(|b| child(b, a("bodyPr"))),
                    font_ref: false,
                });
            }
            if let Some(mp) = m.master {
                steps.push(Step {
                    source: self.master_source(&m, mp)?,
                    node: txbody(mp).and_then(|b| child(b, a("bodyPr"))),
                    font_ref: false,
                });
            }
        }
        Ok(BodyProps::resolve(&steps))
    }

    /// The source of a value from the master-side placeholder `mp`.
    fn master_source(&self, m: &PlaceholderMatch<'a>, mp: &'a RawNode) -> Result<Source<'a>> {
        let role = if self.kind == SheetKind::Notes { Role::NotesMaster } else { Role::Master };
        let matched = m.kind;
        Ok(Source::Placeholder { role, part: self.master()?.name, shape_id: shape_id(mp), matched, level: None })
    }

    fn geometry_steps(&self, shape: &ShapeRef<'a>) -> Result<std::result::Result<Vec<Step<'a>>, Reason<'a>>> {
        let node = representative(shape);
        let own = |n: &'a RawNode| -> Option<&'a RawNode> {
            if n.is(p("graphicFrame")) {
                child(n, p("xfrm"))
            } else if n.is(p("grpSp")) {
                // #56 m2: a group's own a:xfrm (CT_GroupTransform2D)
                child(n, p("grpSpPr"))
            } else {
                child(n, p("spPr"))
            }
        };
        let mut steps = vec![Step { source: Source::Direct, node: own(node), font_ref: false }];
        if let Some(m) = self.placeholder_match(shape)? {
            // the placeholder it inherits from directly sits in a group of its part
            let in_group_layout = match (m.layout, m.master) {
                (Some(l), _) => placeholders(self.layout()?.root()).iter().any(|x| std::ptr::eq(x.shape, l) && x.in_group),
                (None, Some(mp)) if self.kind != SheetKind::Slide => placeholders(self.master()?.root()).iter().any(|x| std::ptr::eq(x.shape, mp) && x.in_group),
                _ => false,
            };
            if shape.depth() > 0 || in_group_layout {
                return Ok(Err(Reason::GroupCoordinates));
            }
            if let Some(l) = m.layout {
                steps.push(Step { source: Source::Placeholder { role: Role::Layout, part: self.layout()?.name, shape_id: shape_id(l), matched: m.kind, level: None }, node: own(l), font_ref: false });
            }
            if let Some(mp) = m.master {
                steps.push(Step { source: self.master_source(&m, mp)?, node: own(mp), font_ref: false });
            }
        }
        Ok(Ok(steps))
    }

    /// Position and size (`a:xfrm`/`p:xfrm` as one unit, §1.5).
    pub fn geometry(&self, shape: &ShapeRef<'a>) -> Result<Lookup<'a, Xfrm>> {
        let steps = match self.geometry_steps(shape)? {
            Ok(s) => s,
            Err(r) => return Ok(Lookup::Unresolved(r)),
        };
        for s in steps {
            let x = s.node.and_then(|n| if n.is(p("xfrm")) { Some(n) } else { child(n, a("xfrm")) });
            let Some(x) = x else { continue };
            let (Some(off), Some(ext)) = (child(x, a("off")), child(x, a("ext"))) else { continue };
            // a missing or invalid coordinate is an AttrError with its raw text (#56 m3)
            let num = |n: &RawNode, k: &'static str| -> std::result::Result<i64, AttrError> {
                let raw = n.attr(un(k)).unwrap_or("");
                raw.parse::<i64>().map_err(|_| attr_error(k, raw))
            };
            let value = (|| {
                Ok(Xfrm {
                    x: num(off, "x")?,
                    y: num(off, "y")?,
                    cx: num(ext, "cx")?,
                    cy: num(ext, "cy")?,
                    rot: winner(x, "rot", 0, |v| v.parse().ok()),
                    flip_h: winner(x, "flipH", false, ooxml_core::value::parse_bool),
                    flip_v: winner(x, "flipV", false, ooxml_core::value::parse_bool),
                })
            })();
            return Ok(Lookup::Set(Resolved { value, source: s.source, defaulted: false }));
        }
        Ok(Lookup::Unset)
    }

    /// The shape's rectangle in slide coordinates (§1.5): its resolved
    /// geometry, mapped through every enclosing group's
    /// `a:off`/`a:ext`/`a:chOff`/`a:chExt` (scaled, rounded to EMU).
    /// `None` when the geometry is not `Set` and valid (including a
    /// placeholder in a group, `GroupCoordinates`), or when an enclosing
    /// group's transform is invalid (`rot`/`flip*` not parseable, `a:off`/
    /// `a:ext` missing or invalid, a zero `a:chExt`; #56 r2-m3).
    ///
    /// Errors: [`crate::Error::TransformedGroup`] if an enclosing group has
    /// `rot` ≠ 0, `flipH` or `flipV`; chain errors.
    pub fn slide_rect(&self, shape: &ShapeRef<'a>) -> Result<Option<Rect>> {
        let Lookup::Set(Resolved { value: Ok(x), .. }) = self.geometry(shape)? else { return Ok(None) };
        let (mut rx, mut ry, mut rcx, mut rcy) = (x.x as f64, x.y as f64, x.cx as f64, x.cy as f64);
        let path = shape.path();
        let path = path.as_slice();
        let root = self.own.root();
        // enclosing groups, innermost first: the nodes at path[..k], k = len-1 … 3
        for k in (3..path.len()).rev() {
            let Some(g) = crate::write::node_at(root, &path[..k]) else { break };
            if !g.is(p("grpSp")) {
                continue;
            }
            // no a:xfrm: the group does not transform its children
            let Some(gx) = child(g, p("grpSpPr")).and_then(|sp| child(sp, a("xfrm"))) else { continue };
            // #56 r2-m3: an invalid rot/flip, a missing or invalid off/ext,
            // or a zero chExt leaves the rectangle undecided (Ok(None)),
            // never an identity guess
            let rot = match gx.attr(un("rot")) {
                None => 0,
                Some(v) => match v.parse::<i64>() {
                    Ok(r) => r,
                    Err(_) => return Ok(None),
                },
            };
            let flip = |k: &'static str| -> Option<bool> {
                match gx.attr(un(k)) {
                    None => Some(false),
                    Some(v) => ooxml_core::value::parse_bool(v),
                }
            };
            let (Some(fh), Some(fv)) = (flip("flipH"), flip("flipV")) else { return Ok(None) };
            if rot % 21_600_000 != 0 || fh || fv {
                return Err(crate::Error::TransformedGroup { group_id: shape_id(g) });
            }
            // Some(None): element absent; None: present but invalid
            let pt = |n: &'static str, ax: &'static str, ay: &'static str| -> Option<Option<(f64, f64)>> {
                let Some(e) = child(gx, a(n)) else { return Some(None) };
                let x = e.attr(un(ax))?.parse::<i64>().ok()?;
                let y = e.attr(un(ay))?.parse::<i64>().ok()?;
                Some(Some((x as f64, y as f64)))
            };
            let (Some(Some(off)), Some(Some(ext))) = (pt("off", "x", "y"), pt("ext", "cx", "cy")) else { return Ok(None) };
            let (Some(ch_off), Some(ch_ext)) = (pt("chOff", "x", "y"), pt("chExt", "cx", "cy")) else { return Ok(None) };
            let (ch_off, ch_ext) = (ch_off.unwrap_or(off), ch_ext.unwrap_or(ext));
            if ch_ext.0 == 0.0 || ch_ext.1 == 0.0 {
                return Ok(None);
            }
            let (sx, sy) = (ext.0 / ch_ext.0, ext.1 / ch_ext.1);
            rx = off.0 + (rx - ch_off.0) * sx;
            ry = off.1 + (ry - ch_off.1) * sy;
            rcx *= sx;
            rcy *= sy;
        }
        Ok(Some(Rect { x: rx.round() as i64, y: ry.round() as i64, cx: rcx.round() as i64, cy: rcy.round() as i64 }))
    }

    /// The outline: the XSD choice `a:prstGeom | a:custGeom` (§1.5, review m5a).
    pub fn outline(&self, shape: &ShapeRef<'a>) -> Result<Lookup<'a, Outline<'a>>> {
        let steps = match self.geometry_steps(shape)? {
            Ok(s) => s,
            Err(r) => return Ok(Lookup::Unresolved(r)),
        };
        for s in steps {
            let Some(sppr) = s.node.filter(|n| n.is(p("spPr"))) else { continue };
            if let Some(g) = child(sppr, a("prstGeom")) {
                return Ok(Lookup::Set(Resolved { value: Ok(Outline::Preset(EnumValue::from_raw(g.attr(un("prst")).unwrap_or("")))), source: s.source, defaulted: false }));
            }
            if child(sppr, a("custGeom")).is_some() {
                return Ok(Lookup::Set(Resolved { value: Ok(Outline::Custom), source: s.source, defaulted: false }));
            }
        }
        Ok(Lookup::Unset)
    }
}

/// The resolvers of a slide: [`Sheet`]'s, in the slide's context.
impl<'a> DeckSlide<'a, '_> {
    /// [`Sheet::color_map`] of the slide.
    pub fn color_map(&self) -> Result<Option<ClrMap<'a>>> {
        self.sheet().color_map()
    }
    /// [`Sheet::placeholder_match`] of a slide shape.
    pub fn placeholder_match(&self, shape: &ShapeRef<'a>) -> Result<Option<PlaceholderMatch<'a>>> {
        self.sheet().placeholder_match(shape)
    }
    /// [`Sheet::run_props`] of a slide shape.
    pub fn run_props(&self, shape: &ShapeRef<'a>, body: TextBodyRef<'a>, para: ParagraphRef<'a>, item: RunItem<'a>) -> Result<RunProps<'a>> {
        self.sheet().run_props(shape, body, para, item)
    }
    /// [`Sheet::para_props`] of a slide shape.
    pub fn para_props(&self, shape: &ShapeRef<'a>, body: TextBodyRef<'a>, para: ParagraphRef<'a>) -> Result<ParaProps<'a>> {
        self.sheet().para_props(shape, body, para)
    }
    /// [`Sheet::body_props`] of a slide shape.
    pub fn body_props(&self, shape: &ShapeRef<'a>) -> Result<BodyProps<'a>> {
        self.sheet().body_props(shape)
    }
    /// [`Sheet::geometry`] of a slide shape.
    pub fn geometry(&self, shape: &ShapeRef<'a>) -> Result<Lookup<'a, Xfrm>> {
        self.sheet().geometry(shape)
    }
    /// [`Sheet::slide_rect`] of a slide shape.
    pub fn slide_rect(&self, shape: &ShapeRef<'a>) -> Result<Option<Rect>> {
        self.sheet().slide_rect(shape)
    }
    /// [`Sheet::outline`] of a slide shape.
    pub fn outline(&self, shape: &ShapeRef<'a>) -> Result<Lookup<'a, Outline<'a>>> {
        self.sheet().outline(shape)
    }
}

/// The element a shape entry stands for (an AC's representative).
fn representative<'a>(shape: &ShapeRef<'a>) -> &'a RawNode {
    if *shape.kind() == crate::ShapeKind::AlternateContent { ac_representative(shape.raw()).unwrap_or(shape.raw()) } else { shape.raw() }
}

/// Position and size in EMU, with rotation (60000ths of a degree) and flips.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Xfrm {
    /// `a:off@x`.
    pub x: i64,
    /// `a:off@y`.
    pub y: i64,
    /// `a:ext@cx`.
    pub cx: i64,
    /// `a:ext@cy`.
    pub cy: i64,
    /// `@rot` (default 0, from the winning element).
    pub rot: WinnerAttr<i32>,
    /// `@flipH` (default false).
    pub flip_h: WinnerAttr<bool>,
    /// `@flipV` (default false).
    pub flip_v: WinnerAttr<bool>,
}

/// A rectangle in slide coordinates (EMU), from [`Sheet::slide_rect`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    /// Left.
    pub x: i64,
    /// Top.
    pub y: i64,
    /// Width.
    pub cx: i64,
    /// Height.
    pub cy: i64,
}

/// A shape's outline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outline<'a> {
    /// `a:prstGeom@prst`.
    Preset(EnumValue<'a, ShapeTypeValues>),
    /// `a:custGeom`.
    Custom,
}

// ---------------------------------------------------------------------------
// Property sets

/// A typeface with its theme reference (§1.4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Font<'a> {
    /// The typeface (the theme's, for a `+mj-*`/`+mn-*` reference).
    pub typeface: &'a str,
    /// `+mj-lt` etc. when the value came from the theme.
    pub theme_ref: Option<&'a str>,
    /// `@pitchFamily` of the winning element (default 0, r2-m1).
    pub pitch_family: WinnerAttr<u8>,
    /// `@charset` of the winning element (default 1).
    pub charset: WinnerAttr<u8>,
}

/// A fill (§1.3 element group).
#[derive(Clone, Debug, PartialEq)]
pub enum Fill<'a> {
    /// `a:noFill`.
    None,
    /// `a:solidFill`.
    Solid(Color<'a>),
    /// `a:gradFill`, `a:blipFill`, `a:pattFill`, `a:grpFill`: element name.
    Other(&'a str),
}

/// A colour as specified, with the scheme mapping (§1.4).
#[derive(Clone, Debug, PartialEq)]
pub struct Color<'a> {
    /// The colour element's local name (`srgbClr`, `schemeClr`, `sysClr`, …).
    pub kind: &'a str,
    /// Its `@val`.
    pub val: Option<&'a str>,
    /// For `schemeClr`: the theme slot after the colour map (`tx1` → `dk1`).
    pub theme_slot: Option<EnumValue<'a, ColorSchemeIndexValues>>,
    /// Which colour map applied.
    pub map_source: Option<MapSource>,
    /// RGB before transforms: `srgbClr@val`, `sysClr@lastClr`, or the theme
    /// slot's colour.
    pub base_rgb: Option<[u8; 3]>,
    /// Transforms (`lumMod`, `tint`, …) as `(name, value)`, not applied.
    pub transforms: Vec<(&'a str, Option<DmlPercent>)>,
}

fn hex_rgb(s: &str) -> Option<[u8; 3]> {
    if s.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(s, 16).ok()?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

const FILLS: [&str; 6] = ["noFill", "solidFill", "gradFill", "blipFill", "pattFill", "grpFill"];
const SCHEME_MAPPED: [&str; 12] = ["bg1", "tx1", "bg2", "tx2", "accent1", "accent2", "accent3", "accent4", "accent5", "accent6", "hlink", "folHlink"];

fn color_of<'a>(c: &'a RawNode, chain: &Chain<'a>) -> std::result::Result<Color<'a>, Reason<'a>> {
    let kind = c.name().local();
    let val = c.attr(un("val"));
    let transforms = c.elements().filter(|t| t.name().ns() == crate::names::A_NS).map(|t| (t.name().local(), t.attr(un("val")).and_then(parse_dml_percent))).collect();
    let mut col = Color { kind, val, theme_slot: None, map_source: None, base_rgb: None, transforms };
    match kind {
        "srgbClr" => col.base_rgb = val.and_then(hex_rgb),
        "sysClr" => col.base_rgb = c.attr(un("lastClr")).and_then(hex_rgb),
        "schemeClr" => {
            let v = val.unwrap_or("");
            if v == "phClr" {
                return Err(Reason::PlaceholderColor);
            }
            if chain.theme_override {
                return Err(Reason::ThemeOverride);
            }
            let slot = if SCHEME_MAPPED.contains(&v) {
                let map = chain.clr_map;
                col.map_source = map.map(|m| m.from);
                map.and_then(|m| m.node.attr(un(scheme_name(v)))).unwrap_or(v)
            } else {
                v
            };
            col.theme_slot = Some(EnumValue::from_raw(slot));
            col.base_rgb = chain.theme.and_then(|t| theme_color(t, slot));
            if chain.theme.is_none() {
                return Err(Reason::BrokenChain);
            }
        }
        _ => {}
    }
    Ok(col)
}

fn scheme_name(v: &str) -> &'static str {
    SCHEME_MAPPED.into_iter().find(|x| *x == v).unwrap_or("tx1")
}

fn theme_elements(theme: &RawNode) -> Option<&RawNode> {
    child(theme, a("themeElements"))
}

fn theme_color(theme: &RawNode, slot: &str) -> Option<[u8; 3]> {
    let cs = child(theme_elements(theme)?, a("clrScheme"))?;
    let e = cs.elements().find(|c| c.name().local() == slot)?;
    let c = e.elements().next()?;
    match c.name().local() {
        "srgbClr" => c.attr(un("val")).and_then(hex_rgb),
        "sysClr" => c.attr(un("lastClr")).and_then(hex_rgb),
        _ => None,
    }
}

/// Resolves a `+mj-*`/`+mn-*` reference through the theme.
fn theme_font<'a>(theme: &'a RawNode, reference: &'a str) -> std::result::Result<&'a str, Reason<'a>> {
    let coll = if reference.starts_with("+mj") { "majorFont" } else { "minorFont" };
    let script = match &reference[reference.len().saturating_sub(2)..] {
        "lt" => "latin",
        "ea" => "ea",
        _ => "cs",
    };
    let fc = theme_elements(theme).and_then(|t| child(t, a("fontScheme"))).and_then(|f| child(f, a(coll_static(coll))));
    let tf = fc.and_then(|f| child(f, a(script_static(script)))).and_then(|e| e.attr(un("typeface"))).filter(|t| !t.is_empty());
    tf.ok_or_else(|| Reason::ScriptDependent {
        reference,
        script_fonts: fc
            .map(|f| f.elements().filter(|e| e.is(a("font"))).filter_map(|e| Some((e.attr(un("script"))?, e.attr(un("typeface"))?))).collect())
            .unwrap_or_default(),
    })
}

fn coll_static(c: &str) -> &'static str {
    if c == "majorFont" { "majorFont" } else { "minorFont" }
}
fn script_static(s: &str) -> &'static str {
    match s {
        "latin" => "latin",
        "ea" => "ea",
        _ => "cs",
    }
}

/// Generic "first step that has the attribute" lookup.
fn find_attr<'a, T>(chain: &Chain<'a>, name: &'static str, parse: impl Fn(&'a str) -> Option<T>) -> Lookup<'a, T> {
    for (i, s) in chain.steps.iter().enumerate() {
        if let Some((stop, reason)) = &chain.stop_after
            && i >= *stop
        {
            return Lookup::Unresolved(reason.clone());
        }
        if s.font_ref {
            continue;
        }
        if let Some(raw) = s.node.and_then(|n| n.attr(un(name))) {
            let value = parse(raw).ok_or_else(|| AttrError { attr: un(name), raw: raw.to_owned() });
            return Lookup::Set(Resolved { value, source: s.source.clone(), defaulted: false });
        }
    }
    if let Some((_, reason)) = &chain.stop_after {
        return Lookup::Unresolved(reason.clone());
    }
    match &chain.tail {
        Tail::Unset => Lookup::Unset,
        Tail::Unresolved(r) => Lookup::Unresolved(r.clone()),
    }
}

/// The first step holding any member of a child-element group.
fn find_group<'a>(chain: &Chain<'a>, members: &[&str]) -> std::result::Result<Option<(&'a RawNode, Source<'a>)>, Reason<'a>> {
    for (i, s) in chain.steps.iter().enumerate() {
        if let Some((stop, reason)) = &chain.stop_after
            && i >= *stop
        {
            return Err(reason.clone());
        }
        if s.font_ref {
            continue;
        }
        if let Some(c) = s.node.and_then(|n| n.elements().find(|c| c.name().ns() == crate::names::A_NS && members.contains(&c.name().local()))) {
            return Ok(Some((c, s.source.clone())));
        }
    }
    if let Some((_, reason)) = &chain.stop_after {
        return Err(reason.clone());
    }
    match &chain.tail {
        Tail::Unset => Ok(None),
        Tail::Unresolved(r) => Err(r.clone()),
    }
}

/// A child group whose complete winning element is needed by the caller.
fn element_group<'a>(chain: &Chain<'a>, members: &[&str]) -> Lookup<'a, &'a RawNode> {
    match find_group(chain, members) {
        Ok(Some((node, source))) => Lookup::Set(Resolved { value: Ok(node), source, defaulted: false }),
        Ok(None) => Lookup::Unset,
        Err(reason) => Lookup::Unresolved(reason),
    }
}

fn highlight<'a>(chain: &Chain<'a>) -> Lookup<'a, Color<'a>> {
    match find_group(chain, &["highlight"]) {
        Ok(Some((node, source))) => match node.elements().next() {
            Some(color) => match color_of(color, chain) {
                Ok(value) => Lookup::Set(Resolved { value: Ok(value), source, defaulted: false }),
                Err(reason) => Lookup::Unresolved(reason),
            },
            None => Lookup::Set(Resolved { value: Err(attr_error("highlight", "")), source, defaulted: false }),
        },
        Ok(None) => Lookup::Unset,
        Err(reason) => Lookup::Unresolved(reason),
    }
}

fn parse_u32(s: &str) -> Option<u32> {
    s.parse().ok()
}
fn parse_i64(s: &str) -> Option<i64> {
    s.parse().ok()
}
fn parse_bool(s: &str) -> Option<bool> {
    ooxml_core::value::parse_bool(s)
}
fn parse_str(s: &str) -> Option<&str> {
    Some(s)
}
fn parse_enum<E: EnumType>(s: &str) -> Option<EnumValue<'_, E>> {
    Some(EnumValue::from_raw(s))
}

/// The XSD default of an attribute of element `E` (None if it has none).
fn xsd_default_of<E: ElementSpec>(name: &str) -> Option<&'static str> {
    E::ATTRS.iter().find(|a| a.name.ns.is_empty() && a.name.local == name).and_then(|a| a.default)
}

/// Run properties as specified (§1.3, §1.6).
#[derive(Clone, Debug)]
pub struct RunProps<'a> {
    /// `sz`, 1/100 pt (autofit's `fontScale` not applied).
    pub size: Lookup<'a, u32>,
    /// `b`.
    pub bold: Lookup<'a, bool>,
    /// `i`.
    pub italic: Lookup<'a, bool>,
    /// `u`.
    pub underline: Lookup<'a, EnumValue<'a, TextUnderlineValues>>,
    /// `strike`.
    pub strike: Lookup<'a, EnumValue<'a, TextStrikeValues>>,
    /// `cap`.
    pub caps: Lookup<'a, EnumValue<'a, TextCapsValues>>,
    /// `baseline` (`DmlPercent`).
    pub baseline: Lookup<'a, DmlPercent>,
    /// `spc` (1/100 pt).
    pub spacing: Lookup<'a, i64>,
    /// `kern`.
    pub kern: Lookup<'a, u32>,
    /// `lang`.
    pub lang: Lookup<'a, &'a str>,
    /// `altLang`.
    pub alt_lang: Lookup<'a, &'a str>,
    /// `a:latin`.
    pub latin: Lookup<'a, Font<'a>>,
    /// `a:ea`.
    pub east_asian: Lookup<'a, Font<'a>>,
    /// `a:cs`.
    pub complex_script: Lookup<'a, Font<'a>>,
    /// `a:sym`.
    pub symbol: Lookup<'a, Font<'a>>,
    /// The fill group.
    pub fill: Lookup<'a, Fill<'a>>,
    /// The highlight colour.
    pub highlight: Lookup<'a, Color<'a>>,
    /// The winning underline line choice as a raw element.
    pub underline_line: Lookup<'a, &'a RawNode>,
    /// The winning underline fill choice as a raw element.
    pub underline_fill: Lookup<'a, &'a RawNode>,
}

impl<'a> RunProps<'a> {
    fn resolve(c: &Chain<'a>) -> RunProps<'a> {
        RunProps {
            size: find_attr(c, "sz", parse_u32),
            bold: find_attr(c, "b", parse_bool),
            italic: find_attr(c, "i", parse_bool),
            underline: find_attr(c, "u", parse_enum),
            strike: find_attr(c, "strike", parse_enum),
            caps: find_attr(c, "cap", parse_enum),
            baseline: find_attr(c, "baseline", parse_dml_percent),
            spacing: find_attr(c, "spc", parse_i64),
            kern: find_attr(c, "kern", parse_u32),
            lang: find_attr(c, "lang", parse_str),
            alt_lang: find_attr(c, "altLang", parse_str),
            latin: font(c, "latin", "lt"),
            east_asian: font(c, "ea", "ea"),
            complex_script: font(c, "cs", "cs"),
            symbol: font(c, "sym", ""),
            fill: fill(c),
            highlight: highlight(c),
            underline_line: element_group(c, &["uLn", "uLnTx"]),
            underline_fill: element_group(c, &["uFill", "uFillTx"]),
        }
    }
}

/// One of the font slots, including the `fontRef` step (§1.3 step 4).
fn font<'a>(c: &Chain<'a>, slot: &'static str, suffix: &'static str) -> Lookup<'a, Font<'a>> {
    for (i, s) in c.steps.iter().enumerate() {
        if let Some((stop, reason)) = &c.stop_after
            && i >= *stop
        {
            return Lookup::Unresolved(reason.clone());
        }
        let Some(n) = s.node else { continue };
        if s.font_ref {
            if suffix.is_empty() {
                continue;
            }
            let reference: &'static str = match (n.attr(un("idx")), suffix) {
                (Some("major"), "lt") => "+mj-lt",
                (Some("major"), "ea") => "+mj-ea",
                (Some("major"), "cs") => "+mj-cs",
                (Some("minor"), "lt") => "+mn-lt",
                (Some("minor"), "ea") => "+mn-ea",
                (Some("minor"), "cs") => "+mn-cs",
                _ => continue, // idx="none": no typeface
            };
            return theme_ref(c, reference, s.source.clone(), defaulted(0), defaulted(1));
        }
        let Some(f) = child(n, a(slot)) else { continue };
        let raw = f.attr(un("typeface")).unwrap_or("");
        let pitch = winner(f, "pitchFamily", 0u8, |v| v.parse().ok());
        let charset = winner(f, "charset", 1u8, |v| v.parse().ok());
        if is_theme_font_ref(raw) {
            return theme_ref(c, raw, s.source.clone(), pitch, charset);
        }
        return Lookup::Set(Resolved { value: Ok(Font { typeface: raw, theme_ref: None, pitch_family: pitch, charset }), source: s.source.clone(), defaulted: false });
    }
    if let Some((_, reason)) = &c.stop_after {
        return Lookup::Unresolved(reason.clone());
    }
    match &c.tail {
        Tail::Unset => Lookup::Unset,
        Tail::Unresolved(r) => Lookup::Unresolved(r.clone()),
    }
}

/// `+mj-lt`, `+mj-ea`, `+mj-cs`, `+mn-lt`, `+mn-ea`, `+mn-cs` (§1.4).
fn is_theme_font_ref(s: &str) -> bool {
    matches!(s, "+mj-lt" | "+mj-ea" | "+mj-cs" | "+mn-lt" | "+mn-ea" | "+mn-cs")
}

fn theme_ref<'a>(c: &Chain<'a>, reference: &'a str, source: Source<'a>, pitch: WinnerAttr<u8>, charset: WinnerAttr<u8>) -> Lookup<'a, Font<'a>> {
    if c.theme_override {
        return Lookup::Unresolved(Reason::ThemeOverride);
    }
    let Some(theme) = c.theme else { return Lookup::Unresolved(Reason::BrokenChain) };
    match theme_font(theme, reference) {
        Ok(tf) => Lookup::Set(Resolved { value: Ok(Font { typeface: tf, theme_ref: Some(reference), pitch_family: pitch, charset }), source, defaulted: false }),
        Err(r) => Lookup::Unresolved(r),
    }
}

fn fill<'a>(c: &Chain<'a>) -> Lookup<'a, Fill<'a>> {
    for (i, s) in c.steps.iter().enumerate() {
        if let Some((stop, reason)) = &c.stop_after
            && i >= *stop
        {
            return Lookup::Unresolved(reason.clone());
        }
        let Some(n) = s.node else { continue };
        if s.font_ref {
            // the colour child of a:fontRef, if any, is a solid fill
            let Some(col) = n.elements().next() else { continue };
            return match color_of(col, c) {
                Ok(col) => Lookup::Set(Resolved { value: Ok(Fill::Solid(col)), source: s.source.clone(), defaulted: false }),
                Err(r) => Lookup::Unresolved(r),
            };
        }
        let Some(f) = n.elements().find(|e| e.name().ns() == crate::names::A_NS && FILLS.contains(&e.name().local())) else { continue };
        let v = match f.name().local() {
            "noFill" => Fill::None,
            "solidFill" => match f.elements().next().map(|col| color_of(col, c)) {
                Some(Ok(col)) => Fill::Solid(col),
                Some(Err(r)) => return Lookup::Unresolved(r),
                None => Fill::Other("solidFill"),
            },
            other => Fill::Other(other),
        };
        return Lookup::Set(Resolved { value: Ok(v), source: s.source.clone(), defaulted: false });
    }
    if let Some((_, reason)) = &c.stop_after {
        return Lookup::Unresolved(reason.clone());
    }
    match &c.tail {
        Tail::Unset => Lookup::Unset,
        Tail::Unresolved(r) => Lookup::Unresolved(r.clone()),
    }
}

/// A bullet as specified.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Bullet<'a> {
    /// `a:buNone`.
    None,
    /// `a:buChar@char`.
    Char(&'a str),
    /// `a:buAutoNum@type` and `@startAt` (default 1 from the winning element).
    AutoNum {
        /// Numbering scheme (`@type`).
        scheme: EnumValue<'a, TextAutoNumberSchemeValues>,
        /// First number (`@startAt`, default 1 on the winning element).
        start_at: WinnerAttr<u32>,
    },
    /// `a:buBlip`.
    Picture,
}

/// Paragraph properties as specified.
#[derive(Clone, Debug)]
pub struct ParaProps<'a> {
    /// `a:pPr@lvl` (direct; `Err` when invalid).
    pub level: std::result::Result<u8, AttrError>,
    /// `algn`.
    pub align: Lookup<'a, EnumValue<'a, TextAlignmentTypeValues>>,
    /// `marL` (EMU).
    pub margin_left: Lookup<'a, i64>,
    /// `indent` (EMU).
    pub indent: Lookup<'a, i64>,
    /// `rtl`.
    pub rtl: Lookup<'a, bool>,
    /// `defTabSz`.
    pub default_tab: Lookup<'a, i64>,
    /// `fontAlgn`.
    pub font_align: Lookup<'a, EnumValue<'a, TextFontAlignmentValues>>,
    /// The winning tab list as a raw element.
    pub tab_stops: Lookup<'a, &'a RawNode>,
    /// The winning bullet colour choice as a raw element.
    pub bullet_color: Lookup<'a, &'a RawNode>,
    /// The winning bullet size choice as a raw element.
    pub bullet_size: Lookup<'a, &'a RawNode>,
    /// The winning bullet font choice as a raw element.
    pub bullet_font: Lookup<'a, &'a RawNode>,
    /// Bullet type group.
    pub bullet: Lookup<'a, Bullet<'a>>,
    /// `a:lnSpc` (`spcPct` as percent or `spcPts` as 1/100 pt).
    pub line_spacing: Lookup<'a, Spacing>,
    /// `a:spcBef`.
    pub space_before: Lookup<'a, Spacing>,
    /// `a:spcAft`.
    pub space_after: Lookup<'a, Spacing>,
}

/// A text spacing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Spacing {
    /// `a:spcPct@val`.
    Percent(DmlPercent),
    /// `a:spcPts@val` (1/100 pt).
    Points(i64),
}

impl<'a> ParaProps<'a> {
    fn resolve(c: &Chain<'a>, level: std::result::Result<u8, AttrError>) -> ParaProps<'a> {
        let spacing = |name: &'static str| -> Lookup<'a, Spacing> {
            match find_group(c, &[name]) {
                Err(r) => Lookup::Unresolved(r),
                Ok(None) => Lookup::Unset,
                Ok(Some((n, source))) => {
                    // an invalid or missing @val is an AttrError with its raw text (#56 m3)
                    let (el, pct) = match child(n, a("spcPct")) {
                        Some(p) => (Some(p), true),
                        None => (child(n, a("spcPts")), false),
                    };
                    let raw = el.and_then(|e| e.attr(un("val"))).unwrap_or("");
                    let v = if pct { parse_dml_percent(raw).map(Spacing::Percent) } else { raw.parse().ok().map(Spacing::Points) };
                    Lookup::Set(Resolved { value: v.ok_or_else(|| attr_error("val", raw)), source, defaulted: false })
                }
            }
        };
        let bullet = match find_group(c, &["buNone", "buAutoNum", "buChar", "buBlip"]) {
            Err(r) => Lookup::Unresolved(r),
            Ok(None) => Lookup::Unset,
            Ok(Some((n, source))) => {
                let v = match n.name().local() {
                    "buNone" => Bullet::None,
                    "buChar" => Bullet::Char(n.attr(un("char")).unwrap_or("")),
                    "buAutoNum" => Bullet::AutoNum {
                        scheme: EnumValue::from_raw(n.attr(un("type")).unwrap_or("")),
                        start_at: winner(n, "startAt", 1, |v| v.parse().ok()),
                    },
                    _ => Bullet::Picture,
                };
                Lookup::Set(Resolved { value: Ok(v), source, defaulted: false })
            }
        };
        ParaProps {
            level,
            align: find_attr(c, "algn", parse_enum),
            margin_left: find_attr(c, "marL", parse_i64),
            indent: find_attr(c, "indent", parse_i64),
            rtl: find_attr(c, "rtl", parse_bool),
            default_tab: find_attr(c, "defTabSz", parse_i64),
            font_align: find_attr(c, "fontAlgn", parse_enum),
            tab_stops: element_group(c, &["tabLst"]),
            bullet_color: element_group(c, &["buClr", "buClrTx"]),
            bullet_size: element_group(c, &["buSzPct", "buSzPts", "buSzTx"]),
            bullet_font: element_group(c, &["buFont", "buFontTx"]),
            bullet,
            line_spacing: spacing("lnSpc"),
            space_before: spacing("spcBef"),
            space_after: spacing("spcAft"),
        }
    }
}

/// Autofit as specified (§1.6).
#[derive(Clone, Debug, PartialEq)]
pub enum Autofit<'a> {
    /// `a:noAutofit`.
    None,
    /// `a:normAutofit`: scales `Set` on the shape itself (defaulted 100 %/0 %
    /// when absent there), `Unresolved(InheritedAutofitScale)` when the
    /// winning element is a layout's or master's.
    Normal {
        /// `@fontScale`.
        font_scale: Lookup<'a, DmlPercent>,
        /// `@lnSpcReduction`.
        line_spacing_reduction: Lookup<'a, DmlPercent>,
    },
    /// `a:spAutoFit`.
    Shape,
}

/// Body properties as specified.
#[derive(Clone, Debug)]
pub struct BodyProps<'a> {
    /// `anchor`.
    pub anchor: Lookup<'a, EnumValue<'a, TextAnchoringTypeValues>>,
    /// `anchorCtr`.
    pub anchor_center: Lookup<'a, bool>,
    /// `wrap`.
    pub wrap: Lookup<'a, EnumValue<'a, TextWrappingValues>>,
    /// `lIns`, `tIns`, `rIns`, `bIns` (EMU).
    pub insets: [Lookup<'a, i64>; 4],
    /// `vert`.
    pub vertical: Lookup<'a, EnumValue<'a, TextVerticalValues>>,
    /// `rot`.
    pub rotation: Lookup<'a, i64>,
    /// `numCol`.
    pub columns: Lookup<'a, u32>,
    /// `upright` (inherited per attribute, §1.3).
    pub upright: Lookup<'a, bool>,
    /// The autofit group.
    pub autofit: Lookup<'a, Autofit<'a>>,
}

impl<'a> BodyProps<'a> {
    fn resolve(steps: &[Step<'a>]) -> BodyProps<'a> {
        let c = Chain { steps: steps.to_vec(), stop_after: None, tail: Tail::Unset, theme: None, theme_override: false, clr_map: None };
        let autofit = match find_group(&c, &["noAutofit", "normAutofit", "spAutoFit"]) {
            Err(r) => Lookup::Unresolved(r),
            Ok(None) => Lookup::Unset,
            Ok(Some((n, source))) => {
                let v = match n.name().local() {
                    "noAutofit" => Autofit::None,
                    "spAutoFit" => Autofit::Shape,
                    _ => {
                        let scale = |name: &'static str, default: i32| -> Lookup<'a, DmlPercent> {
                            if source != Source::Direct {
                                return Lookup::Unresolved(Reason::InheritedAutofitScale { from: source.clone() });
                            }
                            match n.attr(un(name)) {
                                Some(raw) => Lookup::Set(Resolved { value: parse_dml_percent(raw).ok_or_else(|| attr_error(name, raw)), source: source.clone(), defaulted: false }),
                                None => Lookup::Set(Resolved { value: Ok(DmlPercent::Thousandths(default)), source: source.clone(), defaulted: true }),
                            }
                        };
                        Autofit::Normal { font_scale: scale("fontScale", 100_000), line_spacing_reduction: scale("lnSpcReduction", 0) }
                    }
                };
                Lookup::Set(Resolved { value: Ok(v), source, defaulted: false })
            }
        };
        BodyProps {
            anchor: find_attr(&c, "anchor", parse_enum),
            anchor_center: find_attr(&c, "anchorCtr", parse_bool),
            wrap: find_attr(&c, "wrap", parse_enum),
            insets: [find_attr(&c, "lIns", parse_i64), find_attr(&c, "tIns", parse_i64), find_attr(&c, "rIns", parse_i64), find_attr(&c, "bIns", parse_i64)],
            vertical: find_attr(&c, "vert", parse_enum),
            rotation: find_attr(&c, "rot", parse_i64),
            columns: find_attr(&c, "numCol", parse_u32),
            upright: find_attr(&c, "upright", parse_bool),
            autofit,
        }
    }
}

impl RunProps<'_> {
    /// The XSD default of an `a:rPr` attribute (`"sz"`, `"b"`, …), for a
    /// property that is `Unset` (§1.3, §5). `None` if the schema has none.
    pub fn xsd_default(attr: &str) -> Option<&'static str> {
        xsd_default_of::<RunProperties>(attr)
    }

    /// Q4 helper (§1.4, owner decision): when a font slot is
    /// `Unresolved(ScriptDependent)`, picks the theme's script font for the
    /// run's language — `altLang` first, then `lang`, for any slot
    /// ([`script_for_lang`]). A candidate, never a `Set` value: PowerPoint
    /// decides per character.
    pub fn script_font_candidate<'f>(&self, slot: &'f Lookup<'_, Font<'_>>) -> Option<(&'static str, &'f str)> {
        let Lookup::Unresolved(Reason::ScriptDependent { script_fonts, .. }) = slot else { return None };
        [self.alt_lang.value(), self.lang.value()].into_iter().flatten().find_map(|l| {
            let script = script_for_lang(l)?;
            script_fonts.iter().find(|(s, _)| *s == script).map(|(_, t)| (script, *t))
        })
    }
}

impl ParaProps<'_> {
    /// The XSD default of an `a:pPr` attribute (`"algn"`, `"marL"`, …).
    pub fn xsd_default(attr: &str) -> Option<&'static str> {
        xsd_default_of::<ParagraphProperties>(attr)
    }
}

impl BodyProps<'_> {
    /// The XSD default of an `a:bodyPr` attribute (`"anchor"`, `"wrap"`, …).
    pub fn xsd_default(attr: &str) -> Option<&'static str> {
        xsd_default_of::<BodyProperties>(attr)
    }
}

/// The ISO 15924 script tag (as in a theme's `a:font@script`) for a
/// language tag (`lang`/`altLang`), for [`RunProps::script_font_candidate`]
/// (ADR-0008 §1.4, owner decision Q4). `None` for languages written in the
/// Latin script or unknown ones.
pub fn script_for_lang(lang: &str) -> Option<&'static str> {
    let lower = lang.to_ascii_lowercase();
    let mut parts = lower.split(['-', '_']);
    let primary = parts.next()?;
    let rest: Vec<&str> = parts.collect();
    let has = |t: &str| rest.contains(&t);
    Some(match primary {
        "ja" => "Jpan",
        "ko" => "Hang",
        "zh" => {
            if has("hant") || has("tw") || has("hk") || has("mo") {
                "Hant"
            } else {
                "Hans"
            }
        }
        "ar" | "fa" | "ur" | "ps" | "sd" => "Arab",
        "he" | "yi" => "Hebr",
        "th" => "Thai",
        "am" | "ti" => "Ethi",
        "bn" | "as" => "Beng",
        "gu" => "Gujr",
        "km" => "Khmr",
        "kn" => "Knda",
        "pa" => "Guru",
        "iu" => "Cans",
        "chr" => "Cher",
        "ii" => "Yiii",
        "bo" => "Tibt",
        "dv" => "Thaa",
        "hi" | "mr" | "ne" | "sa" | "kok" => "Deva",
        "te" => "Telu",
        "ta" => "Taml",
        "syr" => "Syrc",
        "or" => "Orya",
        "ml" => "Mlym",
        "lo" => "Laoo",
        "si" => "Sinh",
        "mn" if has("mong") => "Mong",
        "vi" => "Viet",
        "ug" => "Uigh",
        "ka" => "Geor",
        _ => return None,
    })
}

impl<'a> Lookup<'a, Font<'a>> {
    /// The typeface, if resolved.
    pub fn typeface(&self) -> Option<&'a str> {
        self.value().map(|f| f.typeface)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn family_and_styles() {
        assert_eq!(Family::of("ctrTitle"), Family::Title);
        assert_eq!(Family::of("obj"), Family::Body);
        assert_eq!(Family::of("pic"), Family::Body);
        assert_eq!(Family::Footer.text_style(), Some(TextStyleKind::Other));
        assert_eq!(Family::SlideImage.text_style(), None);
        assert_eq!(hex_rgb("FF0080"), Some([255, 0, 128]));
        assert_eq!(hex_rgb("FFF"), None);
    }
}
