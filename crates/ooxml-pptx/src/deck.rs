//! Phase 1 (ADR-0008): the whole deck parsed once, read through `&self`.
//!
//! [`crate::Presentation::deck`] parses the presentation part, every slide,
//! its slide layout, slide master and theme (ADR-0008 §1.1), and the
//! comment parts (§3.3), and hands out a [`Deck`] that borrows them. Nothing
//! is marked dirty: every part stays a raw copy on save. A missing or
//! unparsable link fails only the calls that need it
//! ([`Error::BrokenChain`]).

use std::collections::BTreeMap;

use ooxml_core::opc::Relationships;
use ooxml_core::{Package, QName, RawNode, XmlDocument};

use crate::error::{Error, Result};
use crate::names::{A_NS, P_NS};
const R_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
use crate::read::{Slide, SlideInfo};

/// Relationship types followed by [`Deck`].
pub(crate) mod rel {
    pub const LAYOUT: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout";
    pub const MASTER: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster";
    pub const THEME: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme";
    pub const THEME_OVERRIDE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/themeOverride";
    pub const COMMENTS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/comments";
    pub const COMMENT_AUTHORS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/commentAuthors";
    pub const MODERN_COMMENTS: &str = crate::names::MODERN_COMMENTS_REL;
    pub const NOTES_SLIDE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesSlide";
    pub const NOTES_MASTER: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesMaster";
    pub const AUTHORS: &str = "http://schemas.microsoft.com/office/2018/10/relationships/authors";
}

pub(crate) const fn p(local: &'static str) -> QName {
    QName::new(P_NS, local)
}
pub(crate) const fn a(local: &'static str) -> QName {
    QName::new(A_NS, local)
}
pub(crate) const fn un(local: &'static str) -> QName {
    QName::new("", local)
}

/// The first child element named `q`.
pub(crate) fn child(n: &RawNode, q: QName) -> Option<&RawNode> {
    n.elements().find(|c| c.is(q))
}

/// Which link of the chain is missing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ChainLink {
    /// The slide's `slideLayout` relationship (exactly one) or its part.
    Layout,
    /// The layout's `slideMaster` relationship or its part.
    Master,
    /// The master's (or notes master's) `theme` relationship or its part.
    Theme,
    /// The notes slide's `notesMaster` relationship or its part.
    NotesMaster,
}

/// What kind of part a [`Sheet`] resolves in (ADR-0008 §1.7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SheetKind {
    /// A slide: slide → layout → master.
    Slide,
    /// A notes slide: notes slide → notes master.
    Notes,
    /// A slide layout: layout → master.
    Layout,
    /// A slide master.
    Master,
}

/// The chain of one part (names only; resolved to [`Sheet`] on use).
#[derive(Clone, Debug)]
pub(crate) struct SheetEntry {
    pub kind: SheetKind,
    pub own: String,
    /// The slide's layout (slides only).
    pub layout: Option<std::result::Result<String, ChainLink>>,
    /// The master: a slide's or layout's slide master, a notes slide's
    /// notes master, a master itself.
    pub master: std::result::Result<String, ChainLink>,
    pub theme: std::result::Result<String, ChainLink>,
    pub theme_override: bool,
}

/// A part of the chain, parsed (read-only).
#[derive(Clone, Copy, Debug)]
pub struct PartRef<'a> {
    pub(crate) name: &'a str,
    pub(crate) doc: &'a XmlDocument,
}

#[derive(Debug)]
pub(crate) struct ChainEntry {
    pub layout: std::result::Result<String, ChainLink>,
    pub master: std::result::Result<String, ChainLink>,
    pub theme: std::result::Result<String, ChainLink>,
    /// A `themeOverride` relationship on the slide or its layout (ADR-0008
    /// §1.4, review r2-m7): theme lookups are `Unresolved(ThemeOverride)`.
    pub theme_override: bool,
    /// The slide's notes slide, if it has one that parses (ADR-0008 §2).
    pub notes: Option<SheetEntry>,
}

/// A slide and its chain, or the `sldId` of a slide whose part is missing.
pub(crate) type SlideSlot = std::result::Result<(SlideInfo, ChainEntry), u32>;

/// The whole deck, parsed once (ADR-0008 §1.1, §5). Borrowed from a
/// [`crate::Presentation`]; drop it before editing.
#[derive(Debug)]
pub struct Deck<'a> {
    pub(crate) pkg: &'a Package,
    pub(crate) pres_part: &'a str,
    pub(crate) pres: &'a XmlDocument,
    pub(crate) slides: Vec<SlideSlot>,
    pub(crate) rels: BTreeMap<String, Relationships>,
    /// Slide masters in `p:sldMasterIdLst` order, then their layouts in
    /// `p:sldLayoutIdLst` order (§1.7).
    pub(crate) masters: Vec<SheetEntry>,
    pub(crate) layouts: Vec<SheetEntry>,
}

/// What [`load`] returns.
pub(crate) struct Loaded {
    pub slides: Vec<SlideSlot>,
    pub rels: BTreeMap<String, Relationships>,
    pub masters: Vec<SheetEntry>,
    pub layouts: Vec<SheetEntry>,
}

/// Parses everything [`Deck`] reads. Errors are recorded per part (a part
/// that does not parse is a missing link), never returned.
pub(crate) fn load(pkg: &mut Package, pres_part: &str, entries: &[(u32, Option<String>)]) -> Result<Loaded> {
    let mut rels: BTreeMap<String, Relationships> = BTreeMap::new();
    let get_rels = |pkg: &Package, part: &str, rels: &mut BTreeMap<String, Relationships>| -> Option<Relationships> {
        if let Some(r) = rels.get(part) {
            return Some(r.clone());
        }
        let r = pkg.relationships(part).ok()?;
        rels.insert(part.to_owned(), r.clone());
        Some(r)
    };
    get_rels(pkg, pres_part, &mut rels);
    // every part named by `rel_type` from `part` that exists and parses
    fn targets(pkg: &mut Package, r: &Relationships, rel_type: &str) -> Vec<String> {
        r.by_type(rel_type).filter_map(|x| r.resolve(x)).filter(|n| pkg.contains(n)).collect()
    }
    fn parses(pkg: &mut Package, name: &str) -> bool {
        pkg.xml_part(name).is_ok()
    }
    let mut out = Vec::new();
    for (index, (slide_id, part)) in entries.iter().enumerate() {
        let Some(part) = part else {
            out.push(Err(*slide_id));
            continue;
        };
        if !parses(pkg, part) {
            out.push(Err(*slide_id));
            continue;
        }
        let info = SlideInfo { index, slide_id: *slide_id, part_name: part.clone() };
        let srels = get_rels(pkg, part, &mut rels).unwrap_or_else(|| Relationships::empty(Some(part)));
        let mut theme_override = srels.by_type(rel::THEME_OVERRIDE).next().is_some();
        let layouts = targets(pkg, &srels, rel::LAYOUT);
        let layout = match layouts.as_slice() {
            [l] if parses(pkg, l) => Ok(l.clone()),
            _ => Err(ChainLink::Layout),
        };
        let master = match &layout {
            Ok(l) => {
                let lrels = get_rels(pkg, l, &mut rels).unwrap_or_else(|| Relationships::empty(Some(l)));
                theme_override |= lrels.by_type(rel::THEME_OVERRIDE).next().is_some();
                match targets(pkg, &lrels, rel::MASTER).as_slice() {
                    [m] if parses(pkg, m) => Ok(m.clone()),
                    _ => Err(ChainLink::Master),
                }
            }
            Err(e) => Err(*e),
        };
        let theme = match &master {
            Ok(m) => {
                let mrels = get_rels(pkg, m, &mut rels).unwrap_or_else(|| Relationships::empty(Some(m)));
                match targets(pkg, &mrels, rel::THEME).as_slice() {
                    [t] if parses(pkg, t) => Ok(t.clone()),
                    _ => Err(ChainLink::Theme),
                }
            }
            Err(e) => Err(*e),
        };
        // the notes slide, its notes master and that master's theme (§1.7, §2)
        let notes_name = targets(pkg, &srels, rel::NOTES_SLIDE).into_iter().next();
        let notes = notes_name.filter(|n| parses(pkg, n)).map(|n| {
            let nrels = get_rels(pkg, &n, &mut rels).unwrap_or_else(|| Relationships::empty(Some(&n)));
            let theme_override = nrels.by_type(rel::THEME_OVERRIDE).next().is_some();
            let master = match targets(pkg, &nrels, rel::NOTES_MASTER).as_slice() {
                [m] if parses(pkg, m) => Ok(m.clone()),
                _ => Err(ChainLink::NotesMaster),
            };
            let theme = theme_of(pkg, &master, &mut rels);
            SheetEntry { kind: SheetKind::Notes, own: n, layout: None, master, theme, theme_override }
        });
        // comment parts (read-only, §3.3)
        for t in [rel::COMMENTS, rel::MODERN_COMMENTS] {
            for c in targets(pkg, &srels, t) {
                let _ = parses(pkg, &c);
            }
        }
        out.push(Ok((info, ChainEntry { layout, master, theme, theme_override, notes })));
    }
    if let Some(prels) = rels.get(pres_part).cloned() {
        for t in [rel::COMMENT_AUTHORS, rel::AUTHORS] {
            for c in targets(pkg, &prels, t) {
                let _ = parses(pkg, &c);
            }
        }
    }
    // masters and their layouts (§1.7)
    let rid = |n: &RawNode| n.attr(QName::new(R_NS, "id")).map(str::to_owned);
    let pres_rels = rels.get(pres_part).cloned().unwrap_or_else(|| Relationships::empty(Some(pres_part)));
    let master_ids: Vec<String> = pkg
        .xml_part(pres_part)
        .ok()
        .and_then(|d| child(d.root(), p("sldMasterIdLst")).map(|l| l.elements().filter_map(rid).collect()))
        .unwrap_or_default();
    let (mut masters, mut layouts) = (Vec::new(), Vec::new());
    for id in master_ids {
        let Some(m) = pres_rels.by_id(&id).and_then(|r| pres_rels.resolve(r)).filter(|m| pkg.contains(m) && parses(pkg, m)) else { continue };
        let mrels = get_rels(pkg, &m, &mut rels).unwrap_or_else(|| Relationships::empty(Some(&m)));
        let theme = theme_of(pkg, &Ok(m.clone()), &mut rels);
        let layout_ids: Vec<String> =
            pkg.xml_part(&m).ok().and_then(|d| child(d.root(), p("sldLayoutIdLst")).map(|l| l.elements().filter_map(rid).collect())).unwrap_or_default();
        for lid in layout_ids {
            let Some(l) = mrels.by_id(&lid).and_then(|r| mrels.resolve(r)).filter(|l| pkg.contains(l) && parses(pkg, l)) else { continue };
            let lrels = get_rels(pkg, &l, &mut rels).unwrap_or_else(|| Relationships::empty(Some(&l)));
            // the layout's own master relationship decides (it is normally `m`)
            let master = match targets(pkg, &lrels, rel::MASTER).as_slice() {
                [x] if parses(pkg, x) => Ok(x.clone()),
                _ => Err(ChainLink::Master),
            };
            let theme = theme_of(pkg, &master, &mut rels);
            let theme_override = lrels.by_type(rel::THEME_OVERRIDE).next().is_some();
            layouts.push(SheetEntry { kind: SheetKind::Layout, own: l, layout: None, master, theme, theme_override });
        }
        masters.push(SheetEntry { kind: SheetKind::Master, own: m.clone(), layout: None, master: Ok(m), theme, theme_override: false });
    }
    Ok(Loaded { slides: out, rels, masters, layouts })
}

/// The theme of a (notes) master.
fn theme_of(pkg: &mut Package, master: &std::result::Result<String, ChainLink>, rels: &mut BTreeMap<String, Relationships>) -> std::result::Result<String, ChainLink> {
    let m = master.as_ref().map_err(|e| *e)?;
    let mrels = match rels.get(m) {
        Some(r) => r.clone(),
        None => {
            let r = pkg.relationships(m).map_err(|_| ChainLink::Theme)?;
            rels.insert(m.clone(), r.clone());
            r
        }
    };
    let ts: Vec<String> = mrels.by_type(rel::THEME).filter_map(|x| mrels.resolve(x)).filter(|n| pkg.contains(n)).collect();
    match ts.as_slice() {
        [t] if pkg.xml_part(t).is_ok() => Ok(t.clone()),
        _ => Err(ChainLink::Theme),
    }
}

impl<'a> Deck<'a> {
    /// Number of entries in `p:sldIdLst`.
    pub fn slide_count(&self) -> usize {
        self.slides.len()
    }

    /// The `index`-th slide with its chain. [`Error::DanglingSlide`] when its
    /// part is missing or does not parse.
    pub fn slide(&self, index: usize) -> Result<DeckSlide<'a, '_>> {
        let e = self.slides.get(index).ok_or(Error::SlideIndexOutOfRange { index, count: self.slides.len() })?;
        let (info, chain) = e.as_ref().map_err(|&slide_id| Error::DanglingSlide { slide_id })?;
        let doc = self.pkg.parsed_part(&info.part_name).ok_or(Error::DanglingSlide { slide_id: info.slide_id })?;
        Ok(DeckSlide { deck: self, info: info.clone(), doc, chain })
    }

    /// `p:sldSz` (`cx`, `cy` in EMU).
    pub fn slide_size(&self) -> Option<(i64, i64)> {
        let s = child(self.pres.root(), p("sldSz"))?;
        Some((s.attr(un("cx"))?.parse().ok()?, s.attr(un("cy"))?.parse().ok()?))
    }

    pub(crate) fn part(&self, name: &str) -> Option<PartRef<'a>> {
        let (name, doc) = (self.pkg.part_names().find(|n| *n == name)?, self.pkg.parsed_part(name)?);
        Some(PartRef { name, doc })
    }

    pub(crate) fn rels_of(&self, part: &str) -> Option<&Relationships> {
        self.rels.get(part)
    }

    fn link(&self, name: &std::result::Result<String, ChainLink>) -> std::result::Result<PartRef<'a>, ChainLink> {
        let n = name.as_ref().map_err(|e| *e)?;
        // a named part always parsed in `load`
        self.part(n).ok_or(ChainLink::Theme)
    }

    pub(crate) fn sheet(&self, e: &SheetEntry) -> Option<Sheet<'a>> {
        let own = self.part(&e.own)?;
        Some(Sheet {
            kind: e.kind,
            own,
            layout: e.layout.as_ref().map(|l| self.link(l)),
            master: self.link(&e.master),
            theme: self.link(&e.theme),
            theme_override: e.theme_override,
            default_text_style: self.default_text_style(),
        })
    }

    /// Slide masters in `p:sldMasterIdLst` order, as resolution contexts
    /// for their own shapes (ADR-0008 §1.7).
    pub fn masters(&self) -> Vec<Sheet<'a>> {
        self.masters.iter().filter_map(|e| self.sheet(e)).collect()
    }

    /// Slide layouts (per master, in `p:sldLayoutIdLst` order), as
    /// resolution contexts for their own shapes (§1.7).
    pub fn layouts(&self) -> Vec<Sheet<'a>> {
        self.layouts.iter().filter_map(|e| self.sheet(e)).collect()
    }

    /// The presentation part's `p:defaultTextStyle`.
    pub(crate) fn default_text_style(&self) -> Option<&'a RawNode> {
        child(self.pres.root(), p("defaultTextStyle"))
    }
}

/// One slide of a [`Deck`], with its layout, master and theme.
#[derive(Debug)]
pub struct DeckSlide<'a, 'd> {
    pub(crate) deck: &'d Deck<'a>,
    pub(crate) info: SlideInfo,
    pub(crate) doc: &'a XmlDocument,
    pub(crate) chain: &'d ChainEntry,
}

impl<'a> DeckSlide<'a, '_> {
    /// Position, `sldId` and part name.
    pub fn info(&self) -> &SlideInfo {
        &self.info
    }

    /// The Phase 0 read view of the slide (shapes, text).
    pub fn slide(&self) -> Slide<'a> {
        Slide { doc: self.doc, info: self.info.clone() }
    }

    /// The slide layout part.
    pub fn layout(&self) -> Result<PartRef<'a>> {
        self.sheet().layout()
    }
    /// The slide master part.
    pub fn master(&self) -> Result<PartRef<'a>> {
        self.sheet().master()
    }
    /// The theme part.
    pub fn theme(&self) -> Result<PartRef<'a>> {
        self.sheet().theme()
    }
    /// Part names of the chain (layout, master, theme).
    pub fn chain_parts(&self) -> Result<(&'a str, &'a str, &'a str)> {
        Ok((self.layout()?.name, self.master()?.name, self.theme()?.name))
    }
    /// Does the slide or its layout relate to a `themeOverride` part?
    pub fn has_theme_override(&self) -> bool {
        self.chain.theme_override
    }

    /// The slide as a resolution context ([`Sheet`]); the resolver methods
    /// of `DeckSlide` delegate to it.
    pub fn sheet(&self) -> Sheet<'a> {
        let link = |n: &std::result::Result<String, ChainLink>| self.deck.link(n);
        Sheet {
            kind: SheetKind::Slide,
            own: PartRef { name: self.deck.part(&self.info.part_name).map_or("", |p| p.name), doc: self.doc },
            layout: Some(link(&self.chain.layout)),
            master: link(&self.chain.master),
            theme: link(&self.chain.theme),
            theme_override: self.chain.theme_override,
            default_text_style: self.deck.default_text_style(),
        }
    }

    /// The slide's notes slide, read-only (`None` if it has none;
    /// ADR-0008 §2).
    pub fn notes(&self) -> Option<crate::NotesRef<'a>> {
        let e = self.chain.notes.as_ref()?;
        let p = self.deck.part(&e.own)?;
        Some(crate::NotesRef { doc: p.doc, slide: self.info.clone(), part: e.own.clone() })
    }

    /// The notes slide as a resolution context (§1.7: notes slide → notes
    /// master; `p:notesStyle`).
    pub fn notes_sheet(&self) -> Option<Sheet<'a>> {
        self.deck.sheet(self.chain.notes.as_ref()?)
    }
}

/// A part as a resolution context (ADR-0008 §1.3, §1.7): a slide, a notes
/// slide, a layout or a master, with the parts its shapes inherit from.
/// The resolver methods (`run_props`, `para_props`, `body_props`,
/// `geometry`, `outline`, `placeholder_match`, `color_map`) take shapes
/// of this part.
#[derive(Clone, Copy, Debug)]
pub struct Sheet<'a> {
    pub(crate) kind: SheetKind,
    pub(crate) own: PartRef<'a>,
    pub(crate) layout: Option<std::result::Result<PartRef<'a>, ChainLink>>,
    pub(crate) master: std::result::Result<PartRef<'a>, ChainLink>,
    pub(crate) theme: std::result::Result<PartRef<'a>, ChainLink>,
    pub(crate) theme_override: bool,
    pub(crate) default_text_style: Option<&'a RawNode>,
}

impl<'a> Sheet<'a> {
    /// Slide, notes, layout or master.
    pub fn kind(&self) -> SheetKind {
        self.kind
    }
    /// The part itself.
    pub fn part(&self) -> PartRef<'a> {
        self.own
    }
    /// The part's shapes (as [`Slide::shapes`]).
    pub fn shapes(&self) -> Vec<crate::ShapeRef<'a>> {
        crate::read::shapes_of(self.own.doc)
    }
    fn broken(&self, missing: ChainLink) -> Error {
        Error::BrokenChain { from: self.own.name.to_owned(), missing }
    }
    /// The slide's layout (slides only; [`Error::BrokenChain`] otherwise).
    pub fn layout(&self) -> Result<PartRef<'a>> {
        self.layout.ok_or_else(|| self.broken(ChainLink::Layout))?.map_err(|m| self.broken(m))
    }
    /// The slide master (a notes slide's notes master; a master itself).
    pub fn master(&self) -> Result<PartRef<'a>> {
        self.master.map_err(|m| self.broken(m))
    }
    /// The theme (of the master or notes master).
    pub fn theme(&self) -> Result<PartRef<'a>> {
        self.theme.map_err(|m| self.broken(m))
    }
    /// A `themeOverride` relationship applies (theme lookups are
    /// `Unresolved(ThemeOverride)`).
    pub fn has_theme_override(&self) -> bool {
        self.theme_override
    }
}

impl<'a> PartRef<'a> {
    /// The root element of the part.
    pub fn root(&self) -> &'a RawNode {
        self.doc.root()
    }
    /// The part name (zip entry name).
    pub fn part_name(&self) -> &'a str {
        self.name
    }
}
