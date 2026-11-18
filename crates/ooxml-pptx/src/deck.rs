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
    /// The master's `theme` relationship or its part.
    Theme,
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
}

/// Parses everything [`Deck`] reads. Errors are recorded per part (a part
/// that does not parse is a missing link), never returned.
pub(crate) fn load(pkg: &mut Package, pres_part: &str, entries: &[(u32, Option<String>)]) -> Result<(Vec<SlideSlot>, BTreeMap<String, Relationships>)> {
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
        // comment parts (read-only, §3.3)
        for t in [rel::COMMENTS, rel::MODERN_COMMENTS] {
            for c in targets(pkg, &srels, t) {
                let _ = parses(pkg, &c);
            }
        }
        out.push(Ok((info, ChainEntry { layout, master, theme, theme_override })));
    }
    if let Some(prels) = rels.get(pres_part).cloned() {
        for t in [rel::COMMENT_AUTHORS, rel::AUTHORS] {
            for c in targets(pkg, &prels, t) {
                let _ = parses(pkg, &c);
            }
        }
    }
    Ok((out, rels))
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

    fn link(&self, which: ChainLink) -> Result<PartRef<'a>> {
        let name = match which {
            ChainLink::Layout => &self.chain.layout,
            ChainLink::Master => &self.chain.master,
            ChainLink::Theme => &self.chain.theme,
        };
        let broken = |missing| Error::BrokenChain { from: self.info.part_name.clone(), missing };
        let name = name.as_ref().map_err(|&m| broken(m))?;
        self.deck.part(name).ok_or_else(|| broken(which))
    }

    /// The slide layout part.
    pub fn layout(&self) -> Result<PartRef<'a>> {
        self.link(ChainLink::Layout)
    }
    /// The slide master part.
    pub fn master(&self) -> Result<PartRef<'a>> {
        self.link(ChainLink::Master)
    }
    /// The theme part.
    pub fn theme(&self) -> Result<PartRef<'a>> {
        self.link(ChainLink::Theme)
    }
    /// Part names of the chain (layout, master, theme).
    pub fn chain_parts(&self) -> Result<(&'a str, &'a str, &'a str)> {
        Ok((self.layout()?.name, self.master()?.name, self.theme()?.name))
    }
    /// Does the slide or its layout relate to a `themeOverride` part?
    pub fn has_theme_override(&self) -> bool {
        self.chain.theme_override
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
