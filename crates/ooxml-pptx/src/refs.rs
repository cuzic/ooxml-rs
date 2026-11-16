//! References **into** slide text from other nodes and parts (ADR-0005 §6):
//! animations in the slide's `p:timing`, and modern comment anchors.
//! Both scans only read; no part changes state because of them.

use std::collections::BTreeSet;

use ooxml_core::{QName, RawNode, XmlDocument};

use crate::names::{AC_NS, P188_NS, P_NS, PC_NS};

/// A set of shape ids, or "every shape" when a reference could not be
/// resolved to an id (conservative).
#[derive(Clone, Debug, Default)]
pub(crate) struct Lock {
    all: bool,
    ids: BTreeSet<u32>,
}

impl Lock {
    fn add(&mut self, raw: Option<&str>) {
        match raw.and_then(ooxml_core::value::parse_u32) {
            Some(id) => {
                self.ids.insert(id);
            }
            None => self.all = true,
        }
    }

    /// Does the lock cover any of `ids` (the shape and its enclosing groups)?
    pub(crate) fn covers(&self, ids: &[u32]) -> bool {
        self.all || ids.iter().any(|i| self.ids.contains(i))
    }
}

/// What the slide's own animations and its modern comments pin down.
#[derive(Clone, Debug, Default)]
pub(crate) struct SlideRefs {
    /// Paragraph indices referenced (`bldP@build` ≠ `whole`, `p:pRg`).
    pub paragraphs: Lock,
    /// Character offsets referenced (`p:charRg`, unknown `p:txEl` content).
    pub characters: Lock,
    /// Text ranges anchored by modern comments (or unverifiable anchors).
    pub comments: Lock,
}

const fn p(local: &'static str) -> QName {
    QName::new(P_NS, local)
}

fn descendants<'a>(n: &'a RawNode, out: &mut Vec<&'a RawNode>) {
    for c in n.elements() {
        out.push(c);
        descendants(c, out);
    }
}

impl SlideRefs {
    /// Scans every `p:timing` under the slide root, including those inside
    /// `mc:AlternateContent` branches.
    pub(crate) fn scan_timing(&mut self, slide: &RawNode) {
        let mut all = Vec::new();
        descendants(slide, &mut all);
        for timing in all.iter().filter(|n| n.is(p("timing"))) {
            let mut nodes = Vec::new();
            descendants(timing, &mut nodes);
            for n in nodes {
                if n.is(p("bldP")) && n.attr(QName::new("", "build")).unwrap_or("whole") != "whole" {
                    self.paragraphs.add(n.attr(QName::new("", "spid")));
                }
                if n.is(p("spTgt")) {
                    let spid = n.attr(QName::new("", "spid"));
                    for el in n.elements().filter(|e| e.is(p("txEl"))) {
                        for r in el.elements() {
                            if r.is(p("pRg")) {
                                self.paragraphs.add(spid);
                            } else {
                                // p:charRg, or anything unknown: offsets
                                self.characters.add(spid);
                            }
                        }
                    }
                }
            }
        }
    }

    /// Scans one modern comment part (`p188:cmLst`). `None` = the part
    /// could not be read or parsed: every shape is locked.
    ///
    /// Anchors (`p188:cm`'s children other than `pos`, `replyLst`,
    /// `txBody`, `extLst`) are classified:
    /// - `pc:sldMkLst`, and `ac:deMkLst` without any `txMk` / `cp` / `len`
    ///   below it, anchor to the slide or to a whole drawing element: safe.
    /// - Any other anchor (`ac:txMkLst`, `ac:txBodyMkLst`, `ac:tcMkLst`,
    ///   `ac:trMkLst`, `ac:gridColMkLst`, `p188:unknownAnchor`, unknown
    ///   names): every `@id` of an `ac:*` moniker below it is locked; if it
    ///   has none, every shape is locked.
    /// - A `p188:cm` without an anchor, or an unknown child of `cmLst`:
    ///   every shape is locked.
    pub(crate) fn scan_comments(&mut self, part: Option<&XmlDocument>) {
        let Some(doc) = part else {
            self.comments.all = true;
            return;
        };
        let root = doc.root();
        if !root.is(QName::new(P188_NS, "cmLst")) {
            self.comments.all = true;
            return;
        }
        for cm in root.elements() {
            if cm.is(QName::new(P188_NS, "extLst")) {
                continue;
            }
            if !cm.is(QName::new(P188_NS, "cm")) {
                self.comments.all = true;
                continue;
            }
            let anchors: Vec<&RawNode> = cm
                .elements()
                .filter(|c| {
                    !(c.name().ns() == P188_NS && matches!(c.name().local(), "pos" | "replyLst" | "txBody" | "extLst"))
                })
                .collect();
            if anchors.is_empty() {
                self.comments.all = true;
            }
            for a in anchors {
                let mut below = Vec::new();
                descendants(a, &mut below);
                let text_like = below.iter().any(|n| {
                    n.name().local() == "txMk"
                        || n.attr(QName::new("", "cp")).is_some()
                        || n.attr(QName::new("", "len")).is_some()
                });
                let safe = (a.is(QName::new(PC_NS, "sldMkLst")) || a.is(QName::new(AC_NS, "deMkLst"))) && !text_like;
                if safe {
                    continue;
                }
                let ids: Vec<Option<&str>> = below
                    .iter()
                    .filter(|n| n.name().ns() == AC_NS)
                    .filter_map(|n| n.attr(QName::new("", "id")).map(Some))
                    .collect();
                if ids.is_empty() {
                    self.comments.all = true;
                }
                for id in ids {
                    self.comments.add(id);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(s: &str) -> XmlDocument {
        XmlDocument::parse(s.as_bytes()).unwrap()
    }

    const HEAD: &str = r#"<p188:cmLst xmlns:p188="http://schemas.microsoft.com/office/powerpoint/2018/8/main" xmlns:ac="http://schemas.microsoft.com/office/drawing/2013/main/command" xmlns:pc="http://schemas.microsoft.com/office/powerpoint/2013/main/command">"#;

    fn scan(body: &str) -> Lock {
        let mut r = SlideRefs::default();
        r.scan_comments(Some(&doc(&format!("{HEAD}{body}</p188:cmLst>"))));
        r.comments
    }

    #[test]
    fn comment_anchor_classification() {
        // shape anchor (the fixture's structure): safe
        let l = scan(r#"<p188:cm id="x"><ac:deMkLst><pc:docMk/><pc:sldMk sldId="256"/><ac:spMk id="2"/></ac:deMkLst><p188:txBody/></p188:cm>"#);
        assert!(!l.covers(&[2]));
        // text anchor: monikers are flat siblings; the shape id is locked
        let l = scan(r#"<p188:cm id="x"><ac:txMkLst><pc:docMk/><pc:sldMk sldId="256"/><ac:spMk id="2"/><ac:txMk cp="0" len="5"/></ac:txMkLst></p188:cm>"#);
        assert!(l.covers(&[2]) && !l.covers(&[3]));
        // table cell anchor: graphic frame id locked
        let l = scan(r#"<p188:cm id="x"><ac:tcMkLst><ac:graphicFrameMk id="7"/><ac:tcMk rowId="1" colId="2"/></ac:tcMkLst></p188:cm>"#);
        assert!(l.covers(&[7]));
        // deMkLst with offsets below it is not trusted
        let l = scan(r#"<p188:cm id="x"><ac:deMkLst><ac:spMk id="4"/><ac:txMk cp="1" len="1"/></ac:deMkLst></p188:cm>"#);
        assert!(l.covers(&[4]) && !l.covers(&[5]));
        // unknown anchor without ids, missing anchor, unknown child: everything
        assert!(scan(r#"<p188:cm id="x"><p188:unknownAnchor/></p188:cm>"#).covers(&[99]));
        assert!(scan(r#"<p188:cm id="x"><p188:txBody/></p188:cm>"#).covers(&[99]));
        assert!(scan(r#"<p188:other/>"#).covers(&[99]));
        let mut r = SlideRefs::default();
        r.scan_comments(None);
        assert!(r.comments.covers(&[1]));
    }

    #[test]
    fn timing_classification() {
        let s = doc(r#"<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><p:timing><p:bldLst><p:bldP spid="3" grpId="0" build="p"/><p:bldP spid="4" grpId="0"/></p:bldLst><p:tnLst><p:spTgt spid="5"><p:txEl><p:charRg st="0" end="3"/></p:txEl></p:spTgt><p:spTgt spid="6"><p:txEl><p:pRg st="0" end="0"/></p:txEl></p:spTgt></p:tnLst></p:timing><mc:AlternateContent><mc:Choice Requires="x"><p:timing><p:bldLst><p:bldP spid="x" build="p"/></p:bldLst></p:timing></mc:Choice></mc:AlternateContent></p:sld>"#);
        let mut r = SlideRefs::default();
        r.scan_timing(s.root());
        assert!(r.paragraphs.covers(&[3]) && r.paragraphs.covers(&[6]));
        assert!(r.characters.covers(&[5]) && !r.characters.covers(&[6]));
        // the unparsable spid inside AC locks every shape's paragraphs
        assert!(r.paragraphs.covers(&[42]));
        let s = doc(r#"<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:timing><p:bldLst><p:bldP spid="4" grpId="0" build="whole"/></p:bldLst></p:timing></p:sld>"#);
        let mut r = SlideRefs::default();
        r.scan_timing(s.root());
        assert!(!r.paragraphs.covers(&[4]) && !r.characters.covers(&[4]) && !r.comments.covers(&[4]));
    }
}
