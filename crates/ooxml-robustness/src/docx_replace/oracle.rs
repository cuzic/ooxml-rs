//! The ADR-0007 text coordinates of raw runs, boundaries, and anchors.

use std::{collections::BTreeMap, ops::Range};

use ooxml_core::{QName, RawItem, RawNode};
use ooxml_docx::{Error, Paragraph, RunFormatting};

use super::W_NS;

fn w(node: &RawNode) -> &str {
    if node.name().ns() == W_NS {
        node.name().local()
    } else {
        ""
    }
}

fn property(run: &RawNode) -> Option<&RawNode> {
    run.elements().find(|child| w(child) == "rPr")
}

pub(super) fn overlaps(a: &Range<usize>, b: &Range<usize>) -> bool {
    !a.is_empty() && a.start < b.end && b.start < a.end
}

fn inside(range: &Range<usize>, pos: usize) -> bool {
    range.start < pos && pos < range.end
}

struct Run<'a> {
    node: &'a RawNode,
    range: Range<usize>,
    blocked: Option<&'static str>,
    children: Vec<(&'a RawNode, Range<usize>)>,
}

#[derive(Default)]
pub(super) struct ParagraphXml<'a> {
    runs: Vec<Run<'a>>,
    boundaries: Vec<usize>,
    markers: Vec<usize>,
    len: usize,
}

fn collapsed(items: &[RawItem], index: usize, name: &str) -> bool {
    let id = QName::new(W_NS, "id");
    let Some(value) = items[index].as_element().and_then(|node| node.attr(id)) else {
        return false;
    };
    let other = if name == "bookmarkStart" {
        items[index + 1..].iter().find(|item| !whitespace(item))
    } else {
        items[..index].iter().rev().find(|item| !whitespace(item))
    };
    other.and_then(RawItem::as_element).is_some_and(|node| {
        w(node)
            == if name == "bookmarkStart" {
                "bookmarkEnd"
            } else {
                "bookmarkStart"
            }
            && node.attr(id) == Some(value)
    })
}

fn whitespace(item: &RawItem) -> bool {
    matches!(item, RawItem::Text(text) if text.is_whitespace())
}

const MARKERS: &[&str] = &[
    "bookmarkStart",
    "bookmarkEnd",
    "commentRangeStart",
    "commentRangeEnd",
    "permStart",
    "permEnd",
    "moveFromRangeStart",
    "moveFromRangeEnd",
    "moveToRangeStart",
    "moveToRangeEnd",
    "customXmlInsRangeStart",
    "customXmlInsRangeEnd",
    "customXmlDelRangeStart",
    "customXmlDelRangeEnd",
    "customXmlMoveFromRangeStart",
    "customXmlMoveFromRangeEnd",
    "customXmlMoveToRangeStart",
    "customXmlMoveToRangeEnd",
];

impl<'a> ParagraphXml<'a> {
    pub(super) fn read(paragraph: &Paragraph<'a>) -> Self {
        // Reuse the Phase0 run gates for field/revision context. The positions,
        // range refusals and formatting rules below come from the raw tree.
        let runs = paragraph
            .runs()
            .map(|run| {
                let blocked = match run.check_settable() {
                    Err(Error::InsideField) => Some("InsideField"),
                    Err(Error::InsideRevision { .. }) => Some("InsideRevision"),
                    Err(Error::UnsupportedContext { .. }) => Some("UnsupportedContext"),
                    Err(Error::RunNotTextOnly { .. }) => Some("RunNotTextOnly"),
                    Err(error) => panic!("unexpected run gate: {error:?}"),
                    Ok(()) => property(run.raw())
                        .is_some_and(|p| p.elements().any(|e| w(e) == "rPrChange"))
                        .then_some("InsideRevision"),
                };
                (run.raw().span().expect("parsed run").start, (run.is_removed(), blocked))
            })
            .collect::<BTreeMap<_, _>>();
        let mut model = Self::default();
        model.inline(paragraph.raw(), &runs);
        assert_eq!(model.runs.len(), runs.len(), "raw run walk");
        assert_eq!(model.len, paragraph.text().len(), "raw paragraph text width");
        model
    }

    fn inline(&mut self, parent: &'a RawNode, runs: &BTreeMap<usize, (bool, Option<&'static str>)>) {
        let items = parent.children();
        for (index, item) in items.iter().enumerate() {
            let Some(node) = item.as_element() else {
                if !whitespace(item) {
                    self.boundaries.push(self.len);
                }
                continue;
            };
            match w(node) {
                "r" => {
                    let (removed, blocked) = runs[&node.span().expect("parsed run").start];
                    let start = self.len;
                    let children = node
                        .elements()
                        .map(|child| {
                            let at = self.len;
                            if !removed {
                                self.len += child_width(child);
                            }
                            (child, at..self.len)
                        })
                        .collect();
                    if blocked.is_some() && start == self.len {
                        self.boundaries.push(start);
                    }
                    self.runs.push(Run {
                        node,
                        range: start..self.len,
                        blocked,
                        children,
                    });
                    for ruby in node.elements().filter(|e| w(e) == "ruby") {
                        self.boundaries.push(self.len);
                        for base in ruby.elements().filter(|e| w(e) == "rubyBase") {
                            self.inline(base, runs);
                        }
                        self.boundaries.push(self.len);
                    }
                }
                "sdtContent" => self.inline(node, runs),
                "hyperlink" | "smartTag" | "sdt" | "customXml" | "fldSimple" | "ins" | "del" | "moveFrom"
                | "moveTo" | "dir" | "bdo" => {
                    self.boundaries.push(self.len);
                    self.inline(node, runs);
                    self.boundaries.push(self.len);
                }
                "pPr" | "proofErr" => {}
                name if MARKERS.contains(&name) => {
                    if !matches!(name, "bookmarkStart" | "bookmarkEnd") || !collapsed(items, index, name) {
                        self.markers.push(self.len);
                    }
                }
                _ => self.boundaries.push(self.len),
            }
        }
    }

    pub(super) fn refusal(&self, range: &Range<usize>, text: &str, formatting: RunFormatting) -> Option<&'static str> {
        let mut held = self.runs.iter().filter(|run| overlaps(&run.range, range));
        if let Some(reason) = held.clone().find_map(|run| run.blocked) {
            return Some(reason);
        }
        if self.boundaries.iter().any(|&pos| inside(range, pos)) {
            return Some("MatchCrossesBoundary");
        }
        if self.markers.iter().any(|&pos| inside(range, pos)) {
            return Some("MarkerInsideMatch");
        }
        if formatting != RunFormatting::FirstRun
            && let Some(first) = held.next()
            && held.any(|run| {
                let matched = &text[run.range.start.max(range.start)..run.range.end.min(range.end)];
                !same_formatting(first.node, run.node, matched, formatting)
            })
        {
            return Some("MixedFormatting");
        }
        let ws = |ch| matches!(ch, ' ' | '\t' | '\r' | '\n');
        self.runs
            .iter()
            .filter(|run| overlaps(&run.range, range))
            .flat_map(|run| &run.children)
            .any(|(child, span)| {
                w(child) == "t"
                    && overlaps(span, range)
                    && child.xml_space_preserve() != Some(true)
                    && ((span.start < range.start && child.text().starts_with(ws))
                        || (span.end > range.end && child.text().ends_with(ws)))
            })
            .then_some("UnpreservedWhitespace")
    }

    pub(super) fn check_locality<'b>(
        &self,
        new_runs: impl Iterator<Item = &'b RawNode>,
        a: &[u8],
        b: &[u8],
        edits: &[&Range<usize>],
    ) {
        let new_runs: Vec<_> = new_runs.collect();
        assert_eq!(self.runs.len(), new_runs.len(), "run count changed");
        for (old, new) in self.runs.iter().zip(new_runs) {
            let old_span = old.node.span().expect("parsed run");
            let new_span = new.span().expect("parsed run");
            assert_eq!(
                &a[old_span.start..old_span.open_end],
                &b[new_span.start..new_span.open_end],
                "run start tag changed"
            );
            if !edits.iter().any(|range| overlaps(&old.range, range)) {
                assert_eq!(bytes(a, old.node), bytes(b, new), "untouched run changed");
                continue;
            }
            assert!(
                old.node
                    .elements()
                    .filter(|e| w(e) == "rPr")
                    .map(|n| bytes(a, n))
                    .eq(new.elements().filter(|e| w(e) == "rPr").map(|n| bytes(b, n))),
                "run properties changed"
            );
            let mut remaining = new.elements();
            for (child, span) in &old.children {
                let edited = edits.iter().any(|range| {
                    overlaps(span, range) || (w(child) == "lastRenderedPageBreak" && inside(range, span.start))
                });
                if !edited {
                    assert!(
                        remaining.any(|node| bytes(b, node) == bytes(a, child)),
                        "untouched run child changed: {:?}",
                        child.name()
                    );
                }
            }
        }
    }
}

fn bytes<'a>(part: &'a [u8], node: &RawNode) -> &'a [u8] {
    let span = node.span().expect("parsed element");
    &part[span.start..span.end()]
}

fn child_width(child: &RawNode) -> usize {
    match w(child) {
        "t" | "delText" => child.text().len(),
        "tab" | "br" | "cr" => 1,
        "noBreakHyphen" => 3,
        "softHyphen" => 2,
        _ => 0,
    }
}

#[derive(PartialEq, Eq)]
struct Property<'a> {
    name: (&'a str, &'a str),
    attrs: Vec<(&'a str, &'a str, &'a str)>,
    children: Vec<Property<'a>>,
    text: String,
}

fn canonical(node: &RawNode) -> Property<'_> {
    let mut attrs = node
        .attrs()
        .iter()
        .filter(|attr| !attr.name().is_namespace_declaration())
        .map(|attr| (attr.name().ns(), attr.name().local(), attr.value()))
        .collect::<Vec<_>>();
    attrs.sort_unstable();
    Property {
        name: (node.name().ns(), node.name().local()),
        attrs,
        children: node.elements().map(canonical).collect(),
        text: node
            .children()
            .iter()
            .filter_map(|item| match item {
                RawItem::Text(t) if !t.is_whitespace() => Some(t.value()),
                _ => None,
            })
            .collect(),
    }
}

fn properties(run: &RawNode, ignore_proofing: bool, ignore_hint: bool) -> Vec<Property<'_>> {
    property(run)
        .into_iter()
        .flat_map(RawNode::elements)
        .filter_map(|node| {
            let local = w(node);
            if ignore_proofing && local == "noProof" {
                return None;
            }
            let ignored_attr = match local {
                "lang" if ignore_proofing => Some("val"),
                "rFonts" if ignore_hint => Some("hint"),
                _ => None,
            };
            let mut value = canonical(node);
            if let Some(attr) = ignored_attr {
                let count = value.attrs.len();
                value.attrs.retain(|&(ns, name, _)| (ns, name) != (W_NS, attr));
                if count > 0 && value.attrs.is_empty() && value.children.is_empty() && value.text.is_empty() {
                    return None;
                }
            }
            Some(value)
        })
        .collect()
}

fn same_formatting(a: &RawNode, b: &RawNode, matched: &str, rule: RunFormatting) -> bool {
    let proofing = rule == RunFormatting::IgnoreInvisible;
    if properties(a, proofing, false) == properties(b, proofing, false) {
        return true;
    }
    let neutral_hint = |run| {
        let hint = property(run)
            .and_then(|p| p.elements().find(|e| w(e) == "rFonts"))
            .and_then(|e| e.attr(QName::new(W_NS, "hint")));
        matches!(hint, None | Some("eastAsia"))
    };
    proofing
        && neutral_hint(a)
        && neutral_hint(b)
        && matched.chars().all(|ch| {
            matches!(ch as u32,
            0x0000..=0x007F | 0x3000..=0x303F | 0x3040..=0x30FF | 0x3100..=0x312F |
            0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF | 0xFF01..=0xFFEF)
        })
        && properties(a, true, true) == properties(b, true, true)
}
