//! Read APIs of ADR-0008 §3: hyperlinks and actions, p14 sections, legacy
//! and modern comments, transitions (with `mc:AlternateContent` branch
//! selection by namespace URI) and a timing summary. All read-only.

use ooxml_core::{EnumValue, QName, RawNode};
use ooxml_schema::pml::{ParagraphBuildValues, TimeNodePresetClassValues, TimeNodeValues, TransitionSpeedValues};

use crate::deck::{Deck, DeckSlide, a, child, p, rel, un};
use crate::error::{Error, Result};
use crate::read::{ShapePath, TextItem};

const MC_NS: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";
const P14_NS: &str = "http://schemas.microsoft.com/office/powerpoint/2010/main";
const P15_NS: &str = "http://schemas.microsoft.com/office/powerpoint/2012/main";
const P159_NS: &str = "http://schemas.microsoft.com/office/powerpoint/2015/09/main";
const P15_COMMENTS_NS: &str = P15_NS;
const AHYP_NS: &str = "http://schemas.microsoft.com/office/drawing/2018/hyperlinkcolor";
const R_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const XMLNS_NS: &str = "http://www.w3.org/2000/xmlns/";
const SECTIONS_URI: &str = "{521415D9-36F7-43E2-AB2F-B90AF26B5E84}";

/// Namespaces whose `mc:Choice` is taken (ADR-0008 §3.4, Q5): p14, p15 and
/// p159 (morph).
pub const UNDERSTOOD_NAMESPACES: &[&str] = &[P14_NS, P15_NS, P159_NS];

// ---------------------------------------------------------------------------
// §3.1 Hyperlinks and actions

/// Click or hover.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkEvent {
    /// `a:hlinkClick`.
    Click,
    /// `a:hlinkMouseOver` (runs) / `a:hlinkHover` (shapes).
    Hover,
}

/// `ppaction://hlinkshowjump?jump=…`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShowJump {
    /// `firstslide`.
    First,
    /// `lastslide`.
    Last,
    /// `nextslide`.
    Next,
    /// `previousslide`.
    Previous,
    /// `lastslideviewed`.
    LastViewed,
    /// `endshow`.
    EndShow,
}

/// What a link does (ADR-0008 §3.1; python-pptx's `ActionSetting` vocabulary
/// plus `media` and `noaction`).
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum LinkTarget {
    /// No action, an external relationship.
    Url(String),
    /// No action, an internal relationship (a part).
    Part(String),
    /// `ppaction://hlinksldjump` to a listed slide.
    Slide {
        /// `p:sldId@id`.
        slide_id: u32,
        /// Index in `p:sldIdLst`.
        index: usize,
    },
    /// `ppaction://hlinkshowjump?jump=…`.
    Jump(ShowJump),
    /// `ppaction://customshow?id=…&return=…`.
    CustomShow {
        /// Custom show id.
        id: String,
        /// `return=true`.
        return_after: bool,
    },
    /// `ppaction://hlinkpres` (target of the relationship).
    Presentation(String),
    /// `ppaction://hlinkfile`.
    File(String),
    /// `ppaction://program`.
    Program(String),
    /// `ppaction://macro?name=…`.
    Macro(String),
    /// `ppaction://ole?verb=…`.
    Ole(String),
    /// `ppaction://media`.
    Media,
    /// `ppaction://noaction`.
    NoAction,
    /// Any other action, raw.
    Unknown(String),
    /// `r:id` not in the part's relationships.
    Dangling {
        /// The relationship id.
        rel_id: String,
    },
}

/// How PowerPoint colours linked text (ADR-0008 §1.6, r2-m10).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkColor {
    /// The theme's `hlink` colour (the default).
    ThemeHyperlink,
    /// The run's own colour (`ahyp:hlinkClr val="tx"`).
    RunColor,
}

/// One `a:hlinkClick` / `a:hlinkMouseOver` / `a:hlinkHover`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link<'a> {
    /// Click or hover.
    pub event: LinkEvent,
    /// What it does.
    pub target: LinkTarget,
    /// `r:id` (`""` normalised to `None`).
    pub rel_id: Option<&'a str>,
    /// Raw `@action`.
    pub action: Option<&'a str>,
    /// `@tooltip`.
    pub tooltip: Option<&'a str>,
    /// `@highlightClick` (direct value, default false).
    pub highlight_click: bool,
    /// `@endSnd` (default false).
    pub end_sound: bool,
    /// `@history` (default true).
    pub history: bool,
    /// `a:hlinkClick` carries `ahyp:hlinkClr val="tx"`.
    pub(crate) run_color: bool,
}

impl Link<'_> {
    /// The colour rule for linked text: only an `a:hlinkClick` changes the
    /// colour (`None` for a hover link); `RunColor` when its `a:extLst`
    /// carries `ahyp:hlinkClr val="tx"`.
    pub fn color_rule(&self) -> Option<LinkColor> {
        (self.event == LinkEvent::Click).then_some(if self.run_color { LinkColor::RunColor } else { LinkColor::ThemeHyperlink })
    }
}

/// Where a link sits on a slide.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkLocation {
    /// The shape.
    pub shape: ShapePath,
    /// Table cell `(row, col)`.
    pub cell: Option<(usize, usize)>,
    /// `(paragraph, item)` for a run or field; `None` for the shape's own action.
    pub text: Option<(usize, usize)>,
}

fn query<'q>(q: &'q str, key: &str) -> Option<&'q str> {
    q.split('&').find_map(|kv| kv.split_once('=').filter(|(k, _)| *k == key).map(|(_, v)| v))
}

/// Links and actions of one part (a slide, notes slide, layout or master),
/// ADR-0008 §3.1. From [`DeckSlide::link_reader`], [`DeckSlide::notes_link_reader`]
/// or [`Deck::link_reader`].
#[derive(Clone, Copy, Debug)]
pub struct PartLinks<'a, 'd> {
    deck: &'d Deck<'a>,
    part: &'a str,
    doc: &'a ooxml_core::XmlDocument,
}

impl<'a> Deck<'a> {
    /// The links of a resolution context's part (slide, notes slide,
    /// layout or master; ADR-0008 §3.1 "on slides and notes", §1.7).
    pub fn link_reader(&self, sheet: &crate::Sheet<'a>) -> PartLinks<'a, '_> {
        let part = sheet.part();
        PartLinks { deck: self, part: part.part_name(), doc: part.doc }
    }
}

impl<'a, 'd> DeckSlide<'a, 'd> {
    /// The slide's links.
    pub fn link_reader(&self) -> PartLinks<'a, 'd> {
        let s = self.sheet();
        PartLinks { deck: self.deck, part: s.part().part_name(), doc: self.doc }
    }
    /// The links of the slide's notes slide (`None` without one).
    pub fn notes_link_reader(&self) -> Option<PartLinks<'a, 'd>> {
        let n = self.notes_sheet()?;
        Some(PartLinks { deck: self.deck, part: n.part().part_name(), doc: n.part().doc })
    }
    /// [`PartLinks::text_link`] on the slide.
    pub fn text_link(&self, rpr_owner: &'a RawNode, event: LinkEvent) -> Option<Link<'a>> {
        self.link_reader().text_link(rpr_owner, event)
    }
    /// [`PartLinks::shape_action`] on the slide.
    pub fn shape_action(&self, shape: &crate::ShapeRef<'a>, event: LinkEvent) -> Option<Link<'a>> {
        self.link_reader().shape_action(shape, event)
    }
    /// [`PartLinks::links`] of the slide.
    pub fn links(&self) -> Vec<(LinkLocation, Link<'a>)> {
        self.link_reader().links()
    }
}

impl<'a> PartLinks<'a, '_> {
    fn parse_link(&self, n: &'a RawNode, event: LinkEvent) -> Link<'a> {
        let rels = self.deck.rels_of(self.part);
        let rel_id = n.attr(QName::new(R_NS, "id")).filter(|r| !r.is_empty());
        let action = n.attr(un("action")).filter(|s| !s.is_empty());
        let rel = rel_id.map(|r| (r, rels.and_then(|x| x.by_id(r))));
        let rel_target = |want_internal: bool| -> Option<String> {
            let (_, r) = rel?;
            let r = r?;
            if want_internal { rels.and_then(|x| x.resolve(r)) } else { Some(r.target.clone()) }
        };
        let dangling = rel.and_then(|(id, r)| r.is_none().then(|| LinkTarget::Dangling { rel_id: id.to_owned() }));
        let target = if let Some(d) = dangling {
            d
        } else {
            match action.and_then(|x| x.strip_prefix("ppaction://")) {
                None if action.is_some() => LinkTarget::Unknown(action.unwrap_or_default().to_owned()),
                None => match rel.and_then(|(_, r)| r) {
                    Some(r) if r.target_mode == ooxml_core::opc::TargetMode::External => LinkTarget::Url(r.target.clone()),
                    Some(_) => LinkTarget::Part(rel_target(true).unwrap_or_default()),
                    None => LinkTarget::NoAction,
                },
                Some(act) => {
                    let (verb, q) = act.split_once('?').unwrap_or((act, ""));
                    match verb {
                        "hlinksldjump" => {
                            let part = rel_target(true).unwrap_or_default();
                            match self.deck.slides.iter().enumerate().find_map(|(i, s)| s.as_ref().ok().filter(|(info, _)| info.part_name == part).map(|(info, _)| (i, info.slide_id))) {
                                Some((index, slide_id)) => LinkTarget::Slide { slide_id, index },
                                None => LinkTarget::Part(part),
                            }
                        }
                        "hlinkshowjump" => match query(q, "jump") {
                            Some("firstslide") => LinkTarget::Jump(ShowJump::First),
                            Some("lastslide") => LinkTarget::Jump(ShowJump::Last),
                            Some("nextslide") => LinkTarget::Jump(ShowJump::Next),
                            Some("previousslide") => LinkTarget::Jump(ShowJump::Previous),
                            Some("lastslideviewed") => LinkTarget::Jump(ShowJump::LastViewed),
                            Some("endshow") => LinkTarget::Jump(ShowJump::EndShow),
                            _ => LinkTarget::Unknown(act.to_owned()),
                        },
                        "customshow" => LinkTarget::CustomShow { id: query(q, "id").unwrap_or_default().to_owned(), return_after: query(q, "return") == Some("true") },
                        "hlinkpres" => LinkTarget::Presentation(rel_target(false).unwrap_or_default()),
                        "hlinkfile" => LinkTarget::File(rel_target(false).unwrap_or_default()),
                        "program" => LinkTarget::Program(rel_target(false).unwrap_or_default()),
                        "macro" => LinkTarget::Macro(query(q, "name").unwrap_or_default().to_owned()),
                        "ole" => LinkTarget::Ole(query(q, "verb").unwrap_or_default().to_owned()),
                        "media" => LinkTarget::Media,
                        "noaction" => LinkTarget::NoAction,
                        _ => LinkTarget::Unknown(act.to_owned()),
                    }
                }
            }
        };
        let flag = |k: &'static str, d: bool| n.attr(un(k)).and_then(ooxml_core::value::parse_bool).unwrap_or(d);
        Link {
            event,
            target,
            rel_id,
            action,
            tooltip: n.attr(un("tooltip")),
            highlight_click: flag("highlightClick", false),
            end_sound: flag("endSnd", false),
            history: flag("history", true),
            run_color: child(n, a("extLst"))
                .into_iter()
                .flat_map(|e| e.elements())
                .flat_map(|ext| ext.elements())
                .any(|c| c.name().ns() == AHYP_NS && c.name().local() == "hlinkClr" && c.attr(un("val")) == Some("tx")),
        }
    }

    /// The link of a run, field or `a:endParaRPr` format element (`a:rPr`).
    pub fn text_link(&self, rpr_owner: &'a RawNode, event: LinkEvent) -> Option<Link<'a>> {
        let rpr = child(rpr_owner, a("rPr"))?;
        let q = match event {
            LinkEvent::Click => a("hlinkClick"),
            LinkEvent::Hover => a("hlinkMouseOver"),
        };
        child(rpr, q).map(|n| self.parse_link(n, event))
    }

    /// A shape's own action (`p:cNvPr/a:hlinkClick` or `a:hlinkHover`).
    pub fn shape_action(&self, shape: &crate::ShapeRef<'a>, event: LinkEvent) -> Option<Link<'a>> {
        // an AC entry: its representative shape (as the resolver, #56 m4)
        let node = if *shape.kind() == crate::ShapeKind::AlternateContent { crate::read::ac_representative(shape.raw())? } else { shape.raw() };
        let c = crate::read::c_nv_pr(node)?;
        let q = match event {
            LinkEvent::Click => a("hlinkClick"),
            LinkEvent::Hover => a("hlinkHover"),
        };
        child(c, q).map(|n| self.parse_link(n, event))
    }

    /// Every link of the part: shape actions, then runs and fields of text
    /// bodies and table cells, in shape order.
    pub fn links(&self) -> Vec<(LinkLocation, Link<'a>)> {
        let mut out = Vec::new();
        for sh in crate::read::shapes_of(self.doc) {
            for ev in [LinkEvent::Click, LinkEvent::Hover] {
                if let Some(l) = self.shape_action(&sh, ev) {
                    out.push((LinkLocation { shape: sh.path().clone(), cell: None, text: None }, l));
                }
            }
            let mut bodies = Vec::new();
            if let Some(b) = sh.text_body() {
                bodies.push((None, b));
            }
            if let Some(t) = sh.table() {
                for (r, row) in t.rows().into_iter().enumerate() {
                    for (c, cell) in row.into_iter().enumerate() {
                        if let Some(b) = cell.text_body() {
                            bodies.push((Some((r, c)), b));
                        }
                    }
                }
            }
            for (cell, b) in bodies {
                for (k, para) in b.paragraphs().into_iter().enumerate() {
                    for (j, it) in para.items().into_iter().enumerate() {
                        let node = match it {
                            TextItem::Run(r) => r.raw(),
                            TextItem::Field(f) => f.raw(),
                            _ => continue,
                        };
                        for ev in [LinkEvent::Click, LinkEvent::Hover] {
                            if let Some(l) = self.text_link(node, ev) {
                                out.push((LinkLocation { shape: sh.path().clone(), cell, text: Some((k, j)) }, l));
                            }
                        }
                    }
                }
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// §3.2 Sections

/// One `p14:section`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section<'a> {
    /// `@name`.
    pub name: &'a str,
    /// `@id` (a GUID).
    pub id: &'a str,
    /// `p14:sldId@id` in order.
    pub slide_ids: Vec<u32>,
    /// The index of each in `p:sldIdLst` (`None`: unknown id).
    pub slides: Vec<Option<usize>>,
}

/// `p14:sectionLst` with its consistency report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sections<'a> {
    /// The sections in order.
    pub sections: Vec<Section<'a>>,
    /// Slides in no section.
    pub unsectioned: Vec<usize>,
    /// Slide ids listed more than once.
    pub duplicated: Vec<u32>,
    /// Slide ids not in `p:sldIdLst`.
    pub unknown: Vec<u32>,
}

impl<'a> Deck<'a> {
    /// The p14 sections (`p:extLst/p:ext[@uri=…]/p14:sectionLst`, ADR-0008 §3.2).
    pub fn sections(&self) -> Option<Sections<'a>> {
        let ext = child(self.pres.root(), p("extLst"))?.elements().find(|e| e.is(p("ext")) && e.attr(un("uri")) == Some(SECTIONS_URI))?;
        let list = child(ext, QName::new(P14_NS, "sectionLst"))?;
        let all_ids: Vec<u32> = self.slides.iter().map(|s| match s {
            Ok((i, _)) => i.slide_id,
            Err(id) => *id,
        }).collect();
        let mut sections = Vec::new();
        let mut seen: Vec<u32> = Vec::new();
        let (mut duplicated, mut unknown) = (Vec::new(), Vec::new());
        for s in list.elements().filter(|e| e.is(QName::new(P14_NS, "section"))) {
            let slide_ids: Vec<u32> = child(s, QName::new(P14_NS, "sldIdLst"))
                .into_iter()
                .flat_map(|l| l.elements())
                .filter(|e| e.is(QName::new(P14_NS, "sldId")))
                .filter_map(|e| e.attr(un("id")).and_then(|v| v.parse().ok()))
                .collect();
            for &id in &slide_ids {
                if seen.contains(&id) {
                    duplicated.push(id);
                }
                seen.push(id);
                if !all_ids.contains(&id) {
                    unknown.push(id);
                }
            }
            let slides = slide_ids.iter().map(|id| all_ids.iter().position(|x| x == id)).collect();
            sections.push(Section { name: s.attr(un("name")).unwrap_or(""), id: s.attr(un("id")).unwrap_or(""), slide_ids, slides });
        }
        let unsectioned = all_ids.iter().enumerate().filter(|(_, id)| !seen.contains(id)).map(|(i, _)| i).collect();
        Some(Sections { sections, unsectioned, duplicated, unknown })
    }
}

// ---------------------------------------------------------------------------
// §3.3 Comments

/// A comment author.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Author<'a> {
    /// `@id` (number for legacy, GUID for modern).
    pub id: &'a str,
    /// `@name`.
    pub name: &'a str,
    /// `@initials`.
    pub initials: Option<&'a str>,
}

/// The anchor of a modern comment (classification of ADR-0005 §6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Anchor {
    /// The slide (or no shape id).
    Slide,
    /// Shapes (`ac:spMk@id` etc.).
    Shapes(Vec<u32>),
    /// A text range `cp`/`len` in a shape.
    Text {
        /// Shape id (the first moniker id).
        shape_id: Option<u32>,
        /// `ac:txMk@cp`.
        cp: Option<u32>,
        /// `ac:txMk@len`.
        len: Option<u32>,
    },
    /// Any other anchor element (local name).
    Other(String),
}

/// One comment, legacy (`p:cm`) or modern (`p188:cm`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Comment<'a> {
    /// Legacy or modern.
    pub modern: bool,
    /// The comment id (`p188:cm@id`; legacy: `@idx`).
    pub id: Option<&'a str>,
    /// The author, `Err` with the id when it is not in the authors part.
    pub author: std::result::Result<Author<'a>, &'a str>,
    /// `@dt` / `@created`.
    pub created: Option<&'a str>,
    /// `p:pos` / `p188:pos` (`x`, `y`, raw; units unspecified for legacy, §19.4.5).
    pub pos: Option<(i64, i64)>,
    /// The text (`p:text`, or the paragraphs of `p188:txBody` joined by `\n`).
    pub text: String,
    /// Modern: the anchor. Legacy: `Slide`.
    pub anchor: Anchor,
    /// Modern: `@status`.
    pub status: Option<EnumValue<'a, ooxml_schema::pml::CommentStatus>>,
    /// Modern replies; legacy: empty.
    pub replies: Vec<Comment<'a>>,
    /// Legacy reply: `p15:parentCm` (`authorId`, `idx`).
    pub parent: Option<(&'a str, &'a str)>,
}

fn pos_of(n: Option<&RawNode>) -> Option<(i64, i64)> {
    let n = n?;
    Some((n.attr(un("x"))?.parse().ok()?, n.attr(un("y"))?.parse().ok()?))
}

fn modern_text(body: Option<&RawNode>) -> String {
    let Some(b) = body else { return String::new() };
    crate::TextBodyRef { node: b }.text()
}

/// The reading view of [`crate::refs::classify_anchor`] (one classifier
/// for reading and locking, ADR-0008 §3.3, review #56 m5).
fn classify_anchor(a_el: &RawNode) -> Anchor {
    let c = crate::refs::classify_anchor(a_el);
    let num = |v: &str| v.parse::<u32>().ok();
    if c.text {
        return Anchor::Text { shape_id: c.ids.first().and_then(|v| num(v)), cp: c.cp.and_then(num), len: c.len.and_then(num) };
    }
    if c.safe {
        return if c.ids.is_empty() { Anchor::Slide } else { Anchor::Shapes(c.ids.iter().filter_map(|v| num(v)).collect()) };
    }
    Anchor::Other(a_el.name().local().to_owned())
}

impl<'a> DeckSlide<'a, '_> {
    fn related(&self, from: &str, rel_type: &str) -> Vec<&'a ooxml_core::XmlDocument> {
        let Some(r) = self.deck.rels_of(from) else { return Vec::new() };
        r.by_type(rel_type).filter_map(|x| r.resolve(x)).filter_map(|n| self.deck.pkg.parsed_part(&n)).collect()
    }

    /// Legacy comments, then modern ones (ADR-0008 §3.3). A comment part
    /// that is missing or did not parse is `Err` for this slide.
    pub fn comments(&self) -> Result<Vec<Comment<'a>>> {
        let pres = self.deck.pres_part;
        let mut out = Vec::new();
        let rels = self.deck.rels_of(&self.info.part_name);
        let count = |t| rels.map_or(0, |r| r.by_type(t).count());
        let legacy = self.related(&self.info.part_name, rel::COMMENTS);
        let modern = self.related(&self.info.part_name, rel::MODERN_COMMENTS);
        if legacy.len() != count(rel::COMMENTS) || modern.len() != count(rel::MODERN_COMMENTS) {
            return Err(Error::UnsupportedContent(format!("{}: a comment part is missing or unreadable", self.info.part_name)));
        }
        let lauthors: Vec<&'a RawNode> = self.related(pres, rel::COMMENT_AUTHORS).into_iter().flat_map(|d| d.root().elements()).filter(|e| e.is(p("cmAuthor"))).collect();
        let p188 = |l: &'static str| QName::new(crate::names::P188_NS, l);
        let mauthors: Vec<&'a RawNode> = self.related(pres, rel::AUTHORS).into_iter().flat_map(|d| d.root().elements()).filter(|e| e.is(p188("author"))).collect();
        let author = |list: &[&'a RawNode], id: &'a str| -> std::result::Result<Author<'a>, &'a str> {
            list.iter()
                .find(|x| x.attr(un("id")) == Some(id))
                .map(|x| Author { id, name: x.attr(un("name")).unwrap_or(""), initials: x.attr(un("initials")) })
                .ok_or(id)
        };
        for d in legacy {
            for cm in d.root().elements().filter(|e| e.is(p("cm"))) {
                let parent = cm
                    .elements()
                    .filter(|e| e.is(p("extLst")))
                    .flat_map(|e| e.elements())
                    .flat_map(|e| e.elements())
                    .flat_map(|e| std::iter::once(e).chain(e.elements()))
                    .find(|e| e.name().ns() == P15_COMMENTS_NS && e.name().local() == "parentCm")
                    .and_then(|e| Some((e.attr(un("authorId"))?, e.attr(un("idx"))?)));
                out.push(Comment {
                    modern: false,
                    id: cm.attr(un("idx")),
                    author: author(&lauthors, cm.attr(un("authorId")).unwrap_or("")),
                    created: cm.attr(un("dt")),
                    pos: pos_of(child(cm, p("pos"))),
                    text: child(cm, p("text")).map(|t| t.text().into_owned()).unwrap_or_default(),
                    anchor: Anchor::Slide,
                    status: None,
                    replies: Vec::new(),
                    parent,
                });
            }
        }
        for d in modern {
            for cm in d.root().elements().filter(|e| e.is(p188("cm"))) {
                let anchor_el = cm.elements().find(|c| !(c.name().ns() == crate::names::P188_NS && matches!(c.name().local(), "pos" | "replyLst" | "txBody" | "extLst")));
                let replies = child(cm, p188("replyLst"))
                    .into_iter()
                    .flat_map(|l| l.elements())
                    .filter(|e| e.is(p188("reply")))
                    .map(|r| Comment {
                        modern: true,
                        id: r.attr(un("id")),
                        author: author(&mauthors, r.attr(un("authorId")).unwrap_or("")),
                        created: r.attr(un("created")),
                        pos: None,
                        text: modern_text(child(r, p188("txBody"))),
                        anchor: Anchor::Slide,
                        status: r.attr(un("status")).map(EnumValue::from_raw),
                        replies: Vec::new(),
                        parent: None,
                    })
                    .collect();
                out.push(Comment {
                    modern: true,
                    id: cm.attr(un("id")),
                    author: author(&mauthors, cm.attr(un("authorId")).unwrap_or("")),
                    created: cm.attr(un("created")),
                    pos: pos_of(child(cm, p188("pos"))),
                    text: modern_text(child(cm, p188("txBody"))),
                    anchor: anchor_el.map_or(Anchor::Other("(none)".into()), classify_anchor),
                    status: cm.attr(un("status")).map(EnumValue::from_raw),
                    replies,
                    parent: None,
                });
            }
        }
        Ok(out)
    }
}

// ---------------------------------------------------------------------------
// §3.4 Transitions

/// Which branch a transition was read from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Branch<'a> {
    /// `p:transition` directly under the root.
    Direct,
    /// An `mc:Choice` (its `Requires`).
    Choice {
        /// `@Requires`.
        requires: &'a str,
    },
    /// `mc:Fallback`.
    Fallback,
}

/// A slide transition (ADR-0008 §3.4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Transition<'a> {
    /// The branch it was read from.
    pub branch: Branch<'a>,
    /// The kind: the child element's (namespace, local name), e.g. `p:fade`, `p159:morph`.
    pub kind: Option<(&'a str, &'a str)>,
    /// The kind element's attributes: ((namespace, local name), raw value).
    pub kind_attributes: Vec<((&'a str, &'a str), &'a str)>,
    /// `@spd` (default `fast`).
    pub speed: EnumValue<'a, TransitionSpeedValues>,
    /// `@p14:dur` (ms).
    pub duration_ms: Option<u32>,
    /// `@advClick` (default true, §19.3.1.50).
    pub advance_on_click: bool,
    /// `@advTm` (ms).
    pub advance_after_ms: Option<u32>,
    /// `p:sndAc` present.
    pub has_sound: bool,
}

fn parse_transition<'a>(t: &'a RawNode, branch: Branch<'a>) -> Transition<'a> {
    let kind_el = t.elements().find(|c| !c.is(p("sndAc")) && !c.is(p("extLst")));
    Transition {
        branch,
        kind: kind_el.map(|k| (k.name().ns(), k.name().local())),
        kind_attributes: kind_el
            .map(|k| k.attrs().iter().filter(|x| !x.name().is_namespace_declaration()).map(|x| ((x.name().ns(), x.name().local()), x.value())).collect())
            .unwrap_or_default(),
        speed: EnumValue::from_raw(t.attr(un("spd")).unwrap_or("fast")),
        duration_ms: t.attr(QName::new(P14_NS, "dur")).and_then(|v| v.parse().ok()),
        advance_on_click: t.attr(un("advClick")).and_then(ooxml_core::value::parse_bool).unwrap_or(true),
        advance_after_ms: t.attr(un("advTm")).and_then(|v| v.parse().ok()),
        has_sound: child(t, p("sndAc")).is_some(),
    }
}

/// Namespace declarations in scope along `chain` (outermost first).
fn declared<'n>(chain: &[&'n RawNode], prefix: &str) -> Option<&'n str> {
    chain.iter().rev().find_map(|n| n.attrs().iter().find(|x| x.name().ns() == XMLNS_NS && x.name().local() == prefix).map(|x| x.value()))
}

/// Every transition branch of a part root (`p:sld`, `p:sldLayout`, `p:sldMaster`).
pub(crate) fn transition_branches(root: &RawNode) -> Vec<Transition<'_>> {
    let mut out = Vec::new();
    for c in root.elements() {
        if c.is(p("transition")) {
            out.push(parse_transition(c, Branch::Direct));
        } else if c.is(QName::new(MC_NS, "AlternateContent")) {
            for b in c.elements() {
                let branch = match b.name().local() {
                    "Choice" if b.name().ns() == MC_NS => Branch::Choice { requires: b.attr(un("Requires")).unwrap_or("") },
                    "Fallback" if b.name().ns() == MC_NS => Branch::Fallback,
                    _ => continue,
                };
                if let Some(t) = child(b, p("transition")) {
                    out.push(parse_transition(t, branch));
                }
            }
        }
    }
    out
}

/// Does a reader that understands [`UNDERSTOOD_NAMESPACES`] process this
/// `mc:Choice`? Every `Requires` prefix must resolve (by the declarations
/// in scope) to an understood URI; an empty `Requires` is not understood.
fn understood(root: &RawNode, ac: &RawNode, choice: &RawNode) -> bool {
    let requires = choice.attr(un("Requires")).unwrap_or("");
    requires.split_whitespace().next().is_some() && requires.split_whitespace().all(|pfx| declared(&[root, ac, choice], pfx).is_some_and(|u| UNDERSTOOD_NAMESPACES.contains(&u)))
}

/// The `name` child of a part root as such a reader sees it: a direct one,
/// else, per `mc:AlternateContent`, the first understood `mc:Choice` that
/// has one, else its `mc:Fallback` (ECMA-376 Part 3). Used for transitions
/// and timing (ADR-0008 §3.4, §3.5).
fn chosen(root: &RawNode, name: QName) -> Option<(&RawNode, Branch<'_>)> {
    if let Some(t) = child(root, name) {
        return Some((t, Branch::Direct));
    }
    for ac in root.elements().filter(|c| c.is(QName::new(MC_NS, "AlternateContent"))) {
        let mut fallback = None;
        for b in ac.elements() {
            if b.is(QName::new(MC_NS, "Choice")) {
                if understood(root, ac, b) && let Some(t) = child(b, name) {
                    return Some((t, Branch::Choice { requires: b.attr(un("Requires")).unwrap_or("") }));
                }
            } else if b.is(QName::new(MC_NS, "Fallback")) {
                fallback = child(b, name).map(|t| (t, Branch::Fallback));
            }
        }
        if fallback.is_some() {
            return fallback;
        }
    }
    None
}

/// The transition a reader that understands [`UNDERSTOOD_NAMESPACES`] uses
/// ([`chosen`]).
pub(crate) fn transition_of(root: &RawNode) -> Option<Transition<'_>> {
    chosen(root, p("transition")).map(|(t, b)| parse_transition(t, b))
}

impl<'a> DeckSlide<'a, '_> {
    /// The slide's own transition (layouts/masters are reported separately, Q5).
    pub fn transition(&self) -> Option<Transition<'a>> {
        transition_of(self.doc.root())
    }
    /// Every branch of the slide's transition.
    pub fn transition_branches(&self) -> Vec<Transition<'a>> {
        transition_branches(self.doc.root())
    }
    /// The layout's transition (not applied to the slide, ADR-0008 §3.4).
    pub fn layout_transition(&self) -> Result<Option<Transition<'a>>> {
        Ok(transition_of(self.layout()?.root()))
    }
    /// The master's transition.
    pub fn master_transition(&self) -> Result<Option<Transition<'a>>> {
        Ok(transition_of(self.master()?.root()))
    }
}

// ---------------------------------------------------------------------------
// §3.5 Timing summary

/// What an effect targets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// A shape.
    Shape(u32),
    /// Paragraphs `st..=end` of a shape.
    Paragraphs(u32, u32, u32),
    /// Characters `st..end` of a shape.
    Chars(u32, u32, u32),
    /// The slide background of a shape target.
    Background(u32),
    /// Anything else.
    Other {
        /// `@spid` if any.
        spid: Option<u32>,
        /// The element's local name.
        element: String,
    },
}

/// One effect (`p:cTn` with `@presetClass`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Effect<'a> {
    /// `@id`.
    pub ctn_id: Option<u32>,
    /// `@presetClass`.
    pub preset_class: Option<EnumValue<'a, TimeNodePresetClassValues>>,
    /// `@presetID`.
    pub preset_id: Option<u32>,
    /// `@presetSubtype`.
    pub preset_subtype: Option<u32>,
    /// `@nodeType`.
    pub node_type: Option<EnumValue<'a, TimeNodeValues>>,
    /// The 0-based click of the main sequence the effect belongs to (a
    /// `clickEffect` starts the next click); `None` before the first click
    /// and outside the main sequence.
    pub click_index: Option<usize>,
    /// Targets below the effect.
    pub targets: Vec<Target>,
}

/// A build entry of `p:bldLst`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Build<'a> {
    /// `p:bldP`.
    Paragraph {
        /// `@spid`.
        spid: u32,
        /// `@build` (default `whole`).
        build: EnumValue<'a, ParagraphBuildValues>,
        /// `@grpId`.
        group: Option<u32>,
    },
    /// `p:bldGraphic`.
    Graphic {
        /// `@spid`.
        spid: u32,
    },
    /// Anything else (local name).
    Other(String),
}

/// The slide's animations, summarised (ADR-0008 §3.5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Timing<'a> {
    /// Effects in document order.
    pub effects: Vec<Effect<'a>>,
    /// Build entries.
    pub builds: Vec<Build<'a>>,
}

fn targets_below(n: &RawNode, out: &mut Vec<Target>) {
    for c in n.elements() {
        if c.is(p("spTgt")) {
            let spid = c.attr(un("spid")).and_then(|v| v.parse().ok());
            let t = match (spid, c.elements().next()) {
                (Some(id), None) => Target::Shape(id),
                (Some(id), Some(e)) if e.is(p("txEl")) => {
                    let r = e.elements().next();
                    let rng = |r: &RawNode| (r.attr(un("st")).and_then(|v| v.parse().ok()).unwrap_or(0), r.attr(un("end")).and_then(|v| v.parse().ok()).unwrap_or(0));
                    match r {
                        Some(r) if r.is(p("pRg")) => {
                            let (s, e2) = rng(r);
                            Target::Paragraphs(id, s, e2)
                        }
                        Some(r) if r.is(p("charRg")) => {
                            let (s, e2) = rng(r);
                            Target::Chars(id, s, e2)
                        }
                        _ => Target::Other { spid: Some(id), element: "txEl".into() },
                    }
                }
                (Some(id), Some(e)) if e.is(p("bg")) => Target::Background(id),
                (spid, e) => Target::Other { spid, element: e.map_or("spTgt".into(), |e| e.name().local().to_owned()) },
            };
            out.push(t);
        } else {
            targets_below(c, out);
        }
    }
}

/// Effects in document order. `click` counts the clicks of the main
/// sequence: an effect with `nodeType="clickEffect"` starts the next click,
/// `withEffect`/`afterEffect` join the current one (#56 m1: PowerPoint
/// writes no `clickPar` node type on the click groups).
fn walk_effects<'a>(n: &'a RawNode, click: &mut usize, current: &mut Option<usize>, in_main: bool, out: &mut Vec<Effect<'a>>) {
    for c in n.elements() {
        if c.is(p("cTn")) {
            let nt = c.attr(un("nodeType"));
            match nt {
                Some("mainSeq") => {
                    walk_effects(c, click, current, true, out);
                    continue;
                }
                Some("interactiveSeq") => {
                    walk_effects(c, click, &mut None, false, out);
                    continue;
                }
                _ => {}
            }
            if c.attr(un("presetClass")).is_some() {
                if in_main && nt == Some("clickEffect") {
                    *click += 1;
                    *current = Some(*click - 1);
                }
                let mut targets = Vec::new();
                targets_below(c, &mut targets);
                let num = |k: &'static str| c.attr(un(k)).and_then(|v| v.parse().ok());
                out.push(Effect {
                    ctn_id: num("id"),
                    preset_class: c.attr(un("presetClass")).map(EnumValue::from_raw),
                    preset_id: num("presetID"),
                    preset_subtype: num("presetSubtype"),
                    node_type: nt.map(EnumValue::from_raw),
                    click_index: if in_main { *current } else { None },
                    targets,
                });
                continue;
            }
        }
        walk_effects(c, click, current, in_main, out);
    }
}

impl<'a> DeckSlide<'a, '_> {
    /// The timing summary; `None` without `p:timing`. An `mc:AlternateContent`
    /// around `p:timing` follows the transition branch rule (`Requires` by
    /// namespace URI, review #56 m4).
    pub fn timing(&self) -> Option<Timing<'a>> {
        let (tm, _) = chosen(self.doc.root(), p("timing"))?;
        let mut effects = Vec::new();
        if let Some(tn) = child(tm, p("tnLst")) {
            walk_effects(tn, &mut 0, &mut None, false, &mut effects);
        }
        let builds = child(tm, p("bldLst"))
            .into_iter()
            .flat_map(|l| l.elements())
            .map(|b| {
                let spid = b.attr(un("spid")).and_then(|v| v.parse().ok()).unwrap_or(0);
                if b.is(p("bldP")) {
                    Build::Paragraph { spid, build: EnumValue::from_raw(b.attr(un("build")).unwrap_or("whole")), group: b.attr(un("grpId")).and_then(|v| v.parse().ok()) }
                } else if b.is(p("bldGraphic")) {
                    Build::Graphic { spid }
                } else {
                    Build::Other(b.name().local().to_owned())
                }
            })
            .collect();
        Some(Timing { effects, builds })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_strings() {
        assert_eq!(query("id=3&return=true", "return"), Some("true"));
        assert_eq!(query("jump=nextslide", "jump"), Some("nextslide"));
        assert_eq!(query("", "x"), None);
    }
}
