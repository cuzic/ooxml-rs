//! Cross-run find and replace (ADR-0007 §1, §7).
//!
//! A paragraph is read as a *flow*: its text ([`crate::Paragraph::text`]),
//! the runs that hold it, and the zero-width elements between them, each
//! classified by ADR-0007 §1.1 as transparent, anchor or boundary. A match
//! is classified against the flow, and an accepted one is applied as
//! **child-local** edits (§1.2): only the content children of a run that
//! hold matched characters are rewritten, removed or created. Every call
//! plans all its edits, dry-runs every insertion against the unchanged tree,
//! and only then writes (§0, atomicity).

use std::ops::Range;
use std::sync::Arc;

use ooxml_core::{RawItem, RawNode};
use ooxml_schema::wml::{
    Break, CarriageReturn, LastRenderedPageBreak, NoBreakHyphen, ParagraphProperties, Run as R, RunProperties,
    SoftHyphen, TabChar, Text,
};

use crate::RunContext;
use crate::body::{Mode, Wrapper, descend};
use crate::document::{Document, Paragraph, set_t};
use crate::error::{Error, Result};
use crate::fields::Operation;
use crate::names::{
    ANCHORS, BOOKMARK_END, BOOKMARK_START, HINT, LANG, NO_PROOF, PROOF_ERR, RFONTS, VAL, W_ID, display_name, is,
};
use crate::story::{DocumentId, ParaRef, RunRef, StoryId, StoryIndex};
use crate::text::{Piece, map_text, run_text};

/// How the formatting of the runs a match spans must compare (ADR-0007
/// §1.3). The replacement always takes the formatting of the run holding
/// the first matched character.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RunFormatting {
    /// All `w:rPr` canonically equal (prefixes, attribute order and
    /// whitespace-only text ignored; a missing `w:rPr` equals an empty one).
    Identical,
    /// Equal after dropping `w:noProof` and `w:lang/@w:val` (proofing and
    /// hyphenation language), and `w:rFonts/@w:hint` when both hints are
    /// `eastAsia` or absent and every matched character of the later run is
    /// hint-neutral (ASCII or the CJK blocks of §1.3).
    #[default]
    IgnoreInvisible,
    /// Anything: the first run's formatting wins.
    FirstRun,
}

/// What `replace` does with a refused match (ADR-0007 §1.4).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum OnRefused {
    /// One refused match refuses the whole call: nothing is written.
    #[default]
    Refuse,
    /// Accepted matches are written; refused ones are reported as skipped.
    Skip,
}

/// Where `find` and `replace` look.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Scope {
    /// Every paragraph of every story.
    #[default]
    All,
    /// One story.
    Story(StoryId),
    /// One paragraph.
    Paragraph(ParaRef),
}

/// Options of [`Document::find`] and the replace calls.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct ReplaceOptions {
    /// Where to look (default: everywhere).
    pub scope: Scope,
    /// The formatting rule (default: [`RunFormatting::IgnoreInvisible`]).
    pub formatting: RunFormatting,
    /// What to do with refused matches (default: refuse the call).
    pub on_refused: OnRefused,
}

impl ReplaceOptions {
    /// The defaults, with `scope`.
    pub fn with_scope(scope: Scope) -> Self {
        ReplaceOptions { scope, ..Self::default() }
    }

    /// These options with `formatting`.
    #[must_use]
    pub fn formatting(mut self, formatting: RunFormatting) -> Self {
        self.formatting = formatting;
        self
    }

    /// These options with `on_refused`.
    #[must_use]
    pub fn on_refused(mut self, on_refused: OnRefused) -> Self {
        self.on_refused = on_refused;
        self
    }
}

/// Outcome of a replace call that wrote (or had nothing to write).
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct ReplaceReport {
    /// Matches replaced (a match whose replacement equals its text counts,
    /// though it writes nothing).
    pub replaced: usize,
    /// Matches skipped under [`OnRefused::Skip`], in document order.
    pub skipped: Vec<Refusal>,
}

/// A refused match: where, and why.
#[derive(Clone, Debug)]
pub struct Refusal {
    /// The paragraph.
    pub para: ParaRef,
    /// Byte range in the paragraph's text.
    pub range: Range<usize>,
    /// Why.
    pub reason: RefusalReason,
}

/// Why a match is refused (ADR-0007 §1.4). When several apply, the first
/// in this order is reported: a matched character's run (R1–R4, then a
/// tracked formatting change), a boundary, an anchor, the formatting rule,
/// unpreserved whitespace, the dry run.
#[derive(Clone, Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RefusalReason {
    /// R1: a matched character is in a run inside a field.
    #[error("a matched character is inside a field")]
    InsideField,
    /// R2: a matched character is in a run inside a tracked change
    /// (`context`), or its run's `w:rPr` has `w:rPrChange` (`None`).
    #[error("a matched character is inside a tracked change")]
    InsideRevision {
        /// The revision wrapper; `None` for a tracked formatting change.
        context: Option<RunContext>,
    },
    /// R3: a matched character is in a run inside `ancestor`.
    #[error("a matched character is inside {ancestor}")]
    UnsupportedContext {
        /// As in [`Error::UnsupportedContext`].
        ancestor: &'static str,
    },
    /// R4: a matched character is in a run holding non-text content.
    #[error("a matched character is in a run holding {child}")]
    RunNotTextOnly {
        /// As in [`Error::RunNotTextOnly`].
        child: String,
    },
    /// The match crosses the start or end of a wrapper, a run that is not a
    /// text run, or another paragraph child (ADR-0007 §1.1 "boundary").
    #[error("the match crosses {boundary}")]
    MatchCrossesBoundary {
        /// What is crossed, e.g. `w:hyperlink`, `m:oMath`, `w:r (inside a field)`,
        /// or `text` / `markup` for non-whitespace text or a comment or
        /// processing instruction directly in the paragraph (review of PR #52, n2).
        boundary: String,
    },
    /// A range marker lies strictly inside the match.
    #[error("{marker} (id {id:?}) lies inside the match")]
    MarkerInsideMatch {
        /// The marker's `prefix:local` name, e.g. `w:bookmarkStart`.
        marker: String,
        /// Its `w:id`.
        id: Option<String>,
    },
    /// The runs the match spans differ in formatting (§1.3).
    #[error("the matched runs differ in formatting")]
    MixedFormatting,
    /// A `w:t` that keeps text outside the match has edge whitespace
    /// without `xml:space="preserve"`: writing it would make that whitespace
    /// visible (§1.2).
    #[error("a w:t keeps edge whitespace without xml:space=\"preserve\"")]
    UnpreservedWhitespace,
    /// The dry run of an insertion the match needs failed in core
    /// (`UnresolvedSibling`, `AlreadyPresent`, `TooDeep`, …).
    #[error("dry run failed: {0}")]
    Core(Arc<ooxml_core::Error>),
}

impl RefusalReason {
    /// The variant's name (`"MixedFormatting"`, …), for reports and tests.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::InsideField => "InsideField",
            Self::InsideRevision { .. } => "InsideRevision",
            Self::UnsupportedContext { .. } => "UnsupportedContext",
            Self::RunNotTextOnly { .. } => "RunNotTextOnly",
            Self::MatchCrossesBoundary { .. } => "MatchCrossesBoundary",
            Self::MarkerInsideMatch { .. } => "MarkerInsideMatch",
            Self::MixedFormatting => "MixedFormatting",
            Self::UnpreservedWhitespace => "UnpreservedWhitespace",
            Self::Core(_) => "Core",
        }
    }

    fn of_run_error(e: Error) -> Self {
        match e {
            Error::InsideField => Self::InsideField,
            Error::InsideRevision { context } => Self::InsideRevision { context: Some(context) },
            Error::UnsupportedContext { ancestor } => Self::UnsupportedContext { ancestor },
            Error::RunNotTextOnly { child } => Self::RunNotTextOnly { child },
            other => Self::Core(Arc::new(ooxml_core::Error::Io(std::io::Error::other(other.to_string())))),
        }
    }
}

/// One occurrence found by [`Document::find`].
#[derive(Clone, Debug)]
pub struct Match {
    /// The paragraph.
    pub para: ParaRef,
    /// Byte range in [`crate::Paragraph::text`].
    pub range: Range<usize>,
    /// The matched text.
    pub text: String,
    /// The document it was found in.
    pub doc: DocumentId,
    /// [`Document::generation`] of the paragraph at find time.
    pub generation: u64,
    /// What `replace` with the same options would do, as far as it does not
    /// depend on the replacement (informational: the replace calls re-run
    /// every check). Dry-run failures of insertions depend on the
    /// replacement's pieces and are reported only by the replace calls.
    pub status: Result<(), Refusal>,
}

// ---------------------------------------------------------------------
// The flow of a paragraph
// ---------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChildKind {
    /// `w:t`
    T,
    /// `w:tab`, `w:br`, `w:cr`, `w:noBreakHyphen`, `w:softHyphen`
    Char,
    /// `w:lastRenderedPageBreak`
    Zero,
}

#[derive(Clone, Debug)]
struct Child {
    /// Item index in the run's children.
    item: usize,
    kind: ChildKind,
    /// Byte range in the paragraph text.
    range: Range<usize>,
}

struct FlowRun<'d> {
    node: &'d RawNode,
    range: Range<usize>,
    /// `Ok` for a text run (ADR-0007 §1.1).
    status: std::result::Result<(), RefusalReason>,
    /// Content children (text runs only).
    children: Vec<Child>,
}

enum Event {
    Boundary(String),
    Anchor { marker: String, id: Option<String> },
}

struct Flow<'d> {
    para: ParaRef,
    text: String,
    runs: Vec<FlowRun<'d>>,
    /// Zero-width elements, with their position in the text.
    events: Vec<(usize, Event)>,
}

/// Is `node` the `w:bookmarkEnd` with id `id`?
fn is_end_of(node: &RawNode, id: Option<&str>) -> bool {
    node.is(BOOKMARK_END) && id.is_some() && node.attr(W_ID) == id
}

/// The element sibling next to child `i` of `parent` in direction `step`,
/// if only whitespace-only text lies between (a collapsed bookmark has
/// nothing between its start and end).
fn adjacent(parent: &RawNode, i: usize, forward: bool) -> Option<&RawNode> {
    let items = parent.children();
    let mut k = i;
    loop {
        k = if forward { k.checked_add(1)? } else { k.checked_sub(1)? };
        match items.get(k)? {
            RawItem::Text(t) if t.is_whitespace() => {}
            RawItem::Element(e) => return Some(e),
            _ => return None,
        }
    }
}

/// How a non-descended child of paragraph-level content counts.
enum Class {
    Ignore,
    Anchor,
    Boundary,
}

fn classify_inline(parent: &RawNode, i: usize, c: &RawNode) -> Class {
    if is::<ParagraphProperties>(c) || c.is(PROOF_ERR) {
        return Class::Ignore;
    }
    if c.is(BOOKMARK_START) && adjacent(parent, i, true).is_some_and(|n| is_end_of(n, c.attr(W_ID))) {
        return Class::Ignore; // collapsed bookmark: a point, transparent
    }
    if c.is(BOOKMARK_END)
        && adjacent(parent, i, false).is_some_and(|n| n.is(BOOKMARK_START) && is_end_of(c, n.attr(W_ID)))
    {
        return Class::Ignore;
    }
    if c.name().ns() == ooxml_schema::wml::NS_W && ANCHORS.contains(&c.name().local()) {
        return Class::Anchor;
    }
    Class::Boundary
}

fn rpr(run: &RawNode) -> Option<&RawNode> {
    run.elements().find(|e| is::<RunProperties>(e))
}

/// The width in the paragraph text of a content child of a text run.
fn child_kind(c: &RawNode) -> Option<(ChildKind, usize)> {
    Some(match () {
        () if is::<Text>(c) => (ChildKind::T, c.text().len()),
        () if is::<TabChar>(c) || is::<Break>(c) || is::<CarriageReturn>(c) => (ChildKind::Char, 1),
        () if is::<NoBreakHyphen>(c) => (ChildKind::Char, '\u{2011}'.len_utf8()),
        () if is::<SoftHyphen>(c) => (ChildKind::Char, '\u{AD}'.len_utf8()),
        () if is::<LastRenderedPageBreak>(c) => (ChildKind::Zero, 0),
        () => return None,
    })
}

impl<'d> Flow<'d> {
    fn build(p: &Paragraph<'d>) -> Flow<'d> {
        let runs: Vec<crate::Run<'d>> = p.runs().collect();
        let mut out = Flow { para: p.para_ref(), text: String::new(), runs: Vec::new(), events: Vec::new() };
        let mut next_run = 0usize;
        // (element, mode for its children, next child item, wrapper it opened)
        let mut stack: Vec<(&'d RawNode, Mode, usize, Option<Wrapper>)> = vec![(p.raw(), Mode::Inline, 0, None)];
        while let Some(top) = stack.last_mut() {
            let (node, mode, i) = (top.0, top.1, top.2);
            let Some(item) = node.children().get(i) else {
                let (.., w) = stack.pop().expect("non-empty");
                if let Some(w) = w {
                    out.events.push((out.text.len(), Event::Boundary(format!("the end of {}", w.qname()))));
                }
                continue;
            };
            top.2 += 1;
            let c = match item {
                RawItem::Element(c) => c,
                RawItem::Text(t) if t.is_whitespace() => continue,
                _ if mode == Mode::Inline => {
                    let what = if matches!(item, RawItem::Text(_)) { "text" } else { "markup" };
                    out.events.push((out.text.len(), Event::Boundary(what.into())));
                    continue;
                }
                _ => continue,
            };
            match descend(mode, c) {
                Some((Mode::Run, _)) => {
                    let run = runs[next_run];
                    next_run += 1;
                    out.push_run(run);
                    stack.push((c, Mode::Run, 0, None));
                }
                Some((child_mode, w)) => {
                    if let Some(w) = w {
                        out.events.push((out.text.len(), Event::Boundary(w.qname().into())));
                    }
                    stack.push((c, child_mode, 0, w));
                }
                None if mode == Mode::Inline => match classify_inline(node, i, c) {
                    Class::Ignore => {}
                    Class::Anchor => out.events.push((
                        out.text.len(),
                        Event::Anchor { marker: display_name(c), id: c.attr(W_ID).map(str::to_owned) },
                    )),
                    Class::Boundary => out.events.push((out.text.len(), Event::Boundary(display_name(c)))),
                },
                None => {}
            }
        }
        debug_assert_eq!(next_run, runs.len(), "the flow walk and the story walk agree on runs");
        debug_assert_eq!(out.text, p.text());
        out
    }

    fn push_run(&mut self, run: crate::Run<'d>) {
        let text = if run.is_removed() { String::new() } else { run_text(run.node) };
        let start = self.text.len();
        let status = match run.check_settable() {
            Err(Error::InsideRevision { context: RunContext::Direct }) if run.has_formatting_change() => {
                Err(RefusalReason::InsideRevision { context: None })
            }
            Err(e) => Err(RefusalReason::of_run_error(e)),
            Ok(()) => Ok(()),
        };
        let mut children = Vec::new();
        if status.is_ok() {
            let mut pos = start;
            for (item, c) in run.node.indexed_elements() {
                if let Some((kind, w)) = child_kind(c) {
                    children.push(Child { item, kind, range: pos..pos + w });
                    pos += w;
                }
            }
            debug_assert_eq!(pos, start + text.len());
        } else if text.is_empty() {
            let why = status.as_ref().err().map(ToString::to_string).unwrap_or_default();
            self.events.push((start, Event::Boundary(format!("w:r ({why})"))));
        }
        self.text.push_str(&text);
        self.runs.push(FlowRun { node: run.node, range: start..self.text.len(), status, children });
    }

    /// The runs holding characters of `m`, with their indices.
    fn runs_in(&self, m: &Range<usize>) -> impl Iterator<Item = (usize, &FlowRun<'d>)> {
        self.runs.iter().enumerate().filter(move |(_, r)| !r.range.is_empty() && r.range.start < m.end && r.range.end > m.start)
    }

    /// ADR-0007 §1.1–§1.3 and §1.2's whitespace rule for the match `m`.
    fn classify(&self, m: &Range<usize>, formatting: RunFormatting) -> std::result::Result<(), RefusalReason> {
        for (_, r) in self.runs_in(m) {
            r.status.clone()?;
        }
        let inside = |pos: usize| m.start < pos && pos < m.end;
        if let Some((_, Event::Boundary(b))) = self.events.iter().find(|(p, e)| inside(*p) && matches!(e, Event::Boundary(_))) {
            return Err(RefusalReason::MatchCrossesBoundary { boundary: b.clone() });
        }
        if let Some((_, Event::Anchor { marker, id })) = self.events.iter().find(|(p, e)| inside(*p) && matches!(e, Event::Anchor { .. })) {
            return Err(RefusalReason::MarkerInsideMatch { marker: marker.clone(), id: id.clone() });
        }
        let held: Vec<&FlowRun<'d>> = self.runs_in(m).map(|(_, r)| r).collect();
        if let Some((first, rest)) = held.split_first() {
            for r in rest {
                let chars = &self.text[r.range.start.max(m.start)..r.range.end.min(m.end)];
                if !same_formatting(rpr(first.node), rpr(r.node), chars, formatting) {
                    return Err(RefusalReason::MixedFormatting);
                }
            }
        }
        for r in &held {
            for c in r.children.iter().filter(|c| c.kind == ChildKind::T && c.range.start < m.end && c.range.end > m.start) {
                let t = r.node.children()[c.item].as_element().expect("content children are elements");
                if t.xml_space_preserve() == Some(true) {
                    continue;
                }
                let text = t.text();
                let ws = |ch: char| matches!(ch, ' ' | '\t' | '\r' | '\n');
                if (text.starts_with(ws) && c.range.start < m.start) || (text.ends_with(ws) && c.range.end > m.end) {
                    return Err(RefusalReason::UnpreservedWhitespace);
                }
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------
// The formatting rule (§1.3)
// ---------------------------------------------------------------------

/// A canonical element: namespace URI, local name, sorted attributes
/// (namespace declarations excluded), children, non-whitespace text.
#[derive(PartialEq, Eq, Debug)]
struct Canon {
    ns: String,
    local: String,
    attrs: Vec<(String, String, String)>,
    children: Vec<Canon>,
    text: String,
}

fn canon(n: &RawNode) -> Canon {
    let mut attrs: Vec<(String, String, String)> = n
        .attrs()
        .iter()
        .filter(|a| !a.name().is_namespace_declaration())
        .map(|a| (a.name().ns().to_owned(), a.name().local().to_owned(), a.value().to_owned()))
        .collect();
    attrs.sort();
    let text: String = n
        .children()
        .iter()
        .filter_map(|c| match c {
            RawItem::Text(t) if !t.is_whitespace() => Some(t.value().to_owned()),
            _ => None,
        })
        .collect();
    Canon { ns: n.name().ns().to_owned(), local: n.name().local().to_owned(), attrs, children: n.elements().map(canon).collect(), text }
}

/// The children of a `w:rPr`, canonical, with the invisible properties
/// dropped as asked. An element left without attributes or children by
/// the drop (a `w:lang` that only had `w:val`) is dropped too.
fn rpr_children(rpr: Option<&RawNode>, proofing: bool, hint: bool) -> Vec<Canon> {
    let Some(rpr) = rpr else { return Vec::new() };
    let mut out = Vec::new();
    for c in rpr.elements() {
        if proofing && c.is(NO_PROOF) {
            continue;
        }
        let mut k = canon(c);
        let had = k.attrs.len();
        let w = ooxml_schema::wml::NS_W;
        if proofing && c.is(LANG) {
            k.attrs.retain(|(ns, l, _)| !(ns == w && l == VAL.local));
        }
        if hint && c.is(RFONTS) {
            k.attrs.retain(|(ns, l, _)| !(ns == w && l == HINT.local));
        }
        if had > 0 && k.attrs.is_empty() && k.children.is_empty() && k.text.is_empty() && (c.is(LANG) || c.is(RFONTS)) {
            continue;
        }
        out.push(k);
    }
    out
}

fn hint_of(rpr: Option<&RawNode>) -> Option<&str> {
    rpr?.elements().find(|e| e.is(RFONTS))?.attr(HINT)
}

/// The hint-neutral code points of ADR-0007 §1.3 (fail closed: anything
/// not listed is visible).
pub(crate) fn hint_neutral(ch: char) -> bool {
    matches!(u32::from(ch),
        0x0000..=0x007F | 0x3000..=0x303F | 0x3040..=0x30FF | 0x3100..=0x312F | 0x3400..=0x4DBF
        | 0x4E00..=0x9FFF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF | 0xFF01..=0xFFEF)
}

/// §1.3: may `later` (holding `chars` of the match) join `first`?
fn same_formatting(first: Option<&RawNode>, later: Option<&RawNode>, chars: &str, rule: RunFormatting) -> bool {
    match rule {
        RunFormatting::FirstRun => true,
        RunFormatting::Identical => rpr_children(first, false, false) == rpr_children(later, false, false),
        RunFormatting::IgnoreInvisible => {
            if rpr_children(first, true, false) == rpr_children(later, true, false) {
                return true;
            }
            let ignorable = |h: Option<&str>| matches!(h, None | Some("eastAsia"));
            ignorable(hint_of(first))
                && ignorable(hint_of(later))
                && chars.chars().all(hint_neutral)
                && rpr_children(first, true, true) == rpr_children(later, true, true)
        }
    }
}

// ---------------------------------------------------------------------
// Planning child-local edits (§1.2)
// ---------------------------------------------------------------------

/// An accepted edit to apply: the match range and its replacement pieces.
#[derive(Clone, Debug)]
struct Edit {
    range: Range<usize>,
    pieces: Vec<Piece>,
}

/// One planned change to a run's children, as indices of the unchanged
/// tree.
#[derive(Clone, Debug, Default)]
struct RegionOps {
    /// Where the new pieces go (item index in the unchanged run).
    insert_at: usize,
    inserts: Vec<Piece>,
    /// A kept `w:t` and its new text.
    set_text: Option<(usize, String)>,
    removes: Vec<usize>,
}

#[derive(Clone, Debug)]
struct RunPlan {
    run: RunRef,
    /// Regions in document order.
    regions: Vec<RegionOps>,
    /// Indices (into the paragraph's edits) of the edits touching the run.
    edits: Vec<usize>,
}

enum Tok {
    Lit(String, usize),
    Rep(Piece, usize),
}

enum Out {
    Text { s: String, from: Vec<usize> },
    Tab,
    Br,
}

impl Out {
    fn piece(self) -> Piece {
        match self {
            Out::Text { s, .. } => Piece::Text(s),
            Out::Tab => Piece::Tab,
            Out::Br => Piece::Br,
        }
    }
}

fn plan_run(run: &FlowRun<'_>, edits: &[&Edit]) -> Vec<RegionOps> {
    let cs = &run.children;
    let affected = |c: &Child| -> bool {
        if c.range.is_empty() {
            edits.iter().any(|e| e.range.start < c.range.start && c.range.start < e.range.end)
        } else {
            edits.iter().any(|e| e.range.start < c.range.end && c.range.start < e.range.end)
        }
    };
    let mut regions: Vec<Vec<usize>> = Vec::new();
    let mut prev = false;
    for (j, c) in cs.iter().enumerate() {
        let a = affected(c);
        if a {
            if prev {
                regions.last_mut().expect("open region").push(j);
            } else {
                regions.push(vec![j]);
            }
        }
        prev = a;
    }
    let mut out = Vec::new();
    for region in regions {
        let mut toks = Vec::new();
        let mut starts = false;
        for &j in &region {
            let c = &cs[j];
            match c.kind {
                ChildKind::T => {
                    let node = run.node.children()[c.item].as_element().expect("element");
                    let t = node.text();
                    let a = c.range.start;
                    let mut cur = a;
                    for e in edits.iter().filter(|e| e.range.start < c.range.end && e.range.end > c.range.start) {
                        if e.range.start > cur {
                            toks.push(Tok::Lit(t[cur - a..e.range.start - a].to_owned(), j));
                        }
                        if e.range.start >= a {
                            starts = true;
                            toks.extend(e.pieces.iter().cloned().map(|p| Tok::Rep(p, j)));
                        }
                        cur = cur.max(e.range.end.min(c.range.end));
                    }
                    if cur < c.range.end {
                        toks.push(Tok::Lit(t[cur - a..].to_owned(), j));
                    }
                }
                ChildKind::Char => {
                    if let Some(e) = edits.iter().find(|e| e.range.start == c.range.start) {
                        starts = true;
                        toks.extend(e.pieces.iter().cloned().map(|p| Tok::Rep(p, j)));
                    }
                }
                ChildKind::Zero => {}
            }
        }
        // Merge adjacent text: kept literal text and the replacement's text
        // pieces share one `w:t` where they meet.
        let mut pieces: Vec<Out> = Vec::new();
        for tok in toks {
            let (s, j) = match tok {
                Tok::Lit(s, j) | Tok::Rep(Piece::Text(s), j) => (s, j),
                Tok::Rep(Piece::Tab, _) => {
                    pieces.push(Out::Tab);
                    continue;
                }
                Tok::Rep(Piece::Br, _) => {
                    pieces.push(Out::Br);
                    continue;
                }
            };
            match pieces.last_mut() {
                Some(Out::Text { s: acc, from }) => {
                    acc.push_str(&s);
                    if !from.contains(&j) {
                        from.push(j);
                    }
                }
                _ => pieces.push(Out::Text { s, from: vec![j] }),
            }
        }
        let first = region[0];
        let last = *region.last().expect("non-empty region");
        let item = |j: usize| cs[j].item;
        let text_from = |o: Option<&Out>, j: usize| matches!(o, Some(Out::Text { from, .. }) if from.contains(&j));
        let mut ops = RegionOps::default();
        if cs[first].kind == ChildKind::T && text_from(pieces.first(), first) {
            // §1.2 step 2: the first affected w:t keeps text; it is kept.
            let mut it = pieces.into_iter();
            let Some(Out::Text { s, .. }) = it.next() else { unreachable!("checked above") };
            ops.set_text = Some((item(first), s));
            ops.insert_at = item(first) + 1;
            ops.inserts = it.map(Out::piece).collect();
            ops.removes = region[1..].iter().map(|&j| item(j)).collect();
        } else if !starts && cs[last].kind == ChildKind::T && text_from(pieces.last(), last) {
            // §1.2 step 3: rₖ's last affected w:t holds the suffix; it is kept.
            let Some(Out::Text { s, .. }) = pieces.pop() else { unreachable!("checked above") };
            ops.set_text = Some((item(last), s));
            ops.insert_at = item(first);
            ops.inserts = pieces.into_iter().map(Out::piece).collect();
            ops.removes = region[..region.len() - 1].iter().map(|&j| item(j)).collect();
        } else {
            ops.insert_at = item(first);
            ops.inserts = pieces.into_iter().map(Out::piece).collect();
            ops.removes = region.iter().map(|&j| item(j)).collect();
        }
        out.push(ops);
    }
    out
}

/// Plans the edits of one paragraph (sorted, non-overlapping, accepted).
fn plan_paragraph(flow: &Flow<'_>, edits: &[Edit]) -> Vec<RunPlan> {
    let mut plans = Vec::new();
    for (k, run) in flow.runs.iter().enumerate() {
        let touching: Vec<usize> =
            (0..edits.len()).filter(|&i| run.range.start < edits[i].range.end && run.range.end > edits[i].range.start).collect();
        if touching.is_empty() || run.status.is_err() {
            continue;
        }
        let refs: Vec<&Edit> = touching.iter().map(|&i| &edits[i]).collect();
        let regions = plan_run(run, &refs);
        if !regions.is_empty() {
            plans.push(RunPlan { run: RunRef::new(flow.para, k), regions, edits: touching });
        }
    }
    plans
}

/// Every insertion of `plan`, checked against the unchanged run.
fn dry_run(lens: &mut ooxml_core::LensMut<'_, R>, plan: &RunPlan) -> ooxml_core::Result<()> {
    for ops in &plan.regions {
        for piece in &ops.inserts {
            match piece {
                Piece::Text(_) => lens.check_insert_at::<Text>(ops.insert_at)?,
                Piece::Tab => lens.check_insert_at::<TabChar>(ops.insert_at)?,
                Piece::Br => lens.check_insert_at::<Break>(ops.insert_at)?,
            }
        }
    }
    Ok(())
}

/// Applies `plan` (after a successful dry run): per region, from the last
/// to the first, the inserts at their unchanged position, then the kept
/// `w:t`'s text, then the removals, each shifted past the inserts.
fn apply(lens: &mut ooxml_core::LensMut<'_, R>, plan: &RunPlan) -> Result<()> {
    for ops in plan.regions.iter().rev() {
        let n = ops.inserts.len();
        for (i, piece) in ops.inserts.iter().enumerate() {
            match piece {
                Piece::Text(s) => set_t(&mut lens.insert_at::<Text>(ops.insert_at + i)?, s)?,
                Piece::Tab => drop(lens.insert_at::<TabChar>(ops.insert_at + i)?),
                Piece::Br => drop(lens.insert_at::<Break>(ops.insert_at + i)?),
            }
        }
        let shift = |idx: usize| if idx >= ops.insert_at { idx + n } else { idx };
        if let Some((idx, s)) = &ops.set_text {
            let idx = shift(*idx);
            let unchanged = lens.node().children()[idx].as_element().is_some_and(|t| t.text() == s.as_str());
            if !unchanged {
                let mut t = lens.nth_text_at(idx).expect("planned w:t");
                set_t(&mut t, s)?;
            }
        }
        let mut removes: Vec<usize> = ops.removes.iter().map(|&i| shift(i)).collect();
        removes.sort_unstable();
        for idx in removes.into_iter().rev() {
            lens.remove_at(idx);
        }
    }
    Ok(())
}

/// Mutable lens on the `w:t` at item index `idx` of a run.
trait TextAt {
    fn nth_text_at(&mut self, idx: usize) -> Option<ooxml_core::LensMut<'_, Text>>;
}

impl TextAt for ooxml_core::LensMut<'_, R> {
    fn nth_text_at(&mut self, idx: usize) -> Option<ooxml_core::LensMut<'_, Text>> {
        let n = self.node().indexed_elements().filter(|(i, c)| *i < idx && is::<Text>(c)).count();
        self.nth_child_mut::<Text>(n)
    }
}

// ---------------------------------------------------------------------
// The API
// ---------------------------------------------------------------------

/// `Err(EmptyNeedle)` / `Err(InvalidXmlChar)` for a needle.
fn check_needle(needle: &str) -> Result<()> {
    if needle.is_empty() {
        return Err(Error::EmptyNeedle);
    }
    map_text(needle).map(drop)
}

impl Document {
    /// The paragraphs `scope` covers, in document order.
    fn scoped(&self, scope: &Scope) -> Result<Vec<ParaRef>> {
        let all = |s: StoryIndex| (0..self.stories[s.0].index.paragraphs.len()).map(move |i| ParaRef::new(s, i));
        Ok(match scope {
            Scope::All => (0..self.stories.len()).flat_map(|s| all(StoryIndex(s))).collect(),
            Scope::Story(id) => all(StoryIndex(self.stories.iter().position(|s| &s.id == id).ok_or(Error::NoSuchStory)?)).collect(),
            Scope::Paragraph(p) => {
                self.paragraph_at(*p).ok_or(Error::NoSuchParagraph(p.index))?;
                vec![*p]
            }
        })
    }

    fn flow(&self, p: ParaRef) -> Option<Flow<'_>> {
        Some(Flow::build(&self.paragraph_at(p)?))
    }

    /// Every occurrence of `needle` in the paragraphs `opts.scope` covers
    /// (ADR-0007 §1.1): exact, case-sensitive, leftmost first and
    /// non-overlapping in each paragraph's [`crate::Paragraph::text`]; never
    /// across paragraphs. Each [`Match`] carries the status `replace` would
    /// give it with the same options (except dry-run failures, which depend
    /// on the replacement).
    ///
    /// Errors: [`Error::EmptyNeedle`], [`Error::InvalidXmlChar`],
    /// [`Error::NoSuchStory`] / [`Error::NoSuchParagraph`] for the scope.
    pub fn find(&self, needle: &str, opts: &ReplaceOptions) -> Result<Vec<Match>> {
        check_needle(needle)?;
        let mut out = Vec::new();
        for p in self.scoped(&opts.scope)? {
            let flow = self.flow(p).expect("scoped paragraphs exist");
            for (s, _) in flow.text.match_indices(needle) {
                let range = s..s + needle.len();
                let status = flow
                    .classify(&range, opts.formatting)
                    .map_err(|reason| Refusal { para: p, range: range.clone(), reason });
                out.push(Match {
                    para: p,
                    range,
                    text: needle.to_owned(),
                    doc: self.id,
                    generation: self.generation(p),
                    status,
                });
            }
        }
        Ok(out)
    }

    /// Replaces every occurrence of `needle` in `opts.scope` with
    /// `replacement` (ADR-0007 §1). The replacement takes the formatting of
    /// the run holding the first matched character; only the run content
    /// children holding matched characters change (§1.2); `\t` and `\n` in
    /// `replacement` become `w:tab` and `w:br`.
    ///
    /// Atomic: the gates, then every match's checks (§1.4), then a dry run
    /// of every insertion, then the writes. With [`OnRefused::Refuse`] one
    /// refused match refuses the call ([`Error::ReplaceRefused`], listing
    /// every refused match) and nothing changes; with [`OnRefused::Skip`]
    /// the refused matches are reported in [`ReplaceReport::skipped`].
    ///
    /// Errors: [`Self::write_gate_for`] ([`Operation::Text`]),
    /// [`Error::EmptyNeedle`], [`Error::InvalidXmlChar`],
    /// [`Error::ReplaceRefused`], scope errors.
    pub fn replace(&mut self, needle: &str, replacement: &str, opts: &ReplaceOptions) -> Result<ReplaceReport> {
        self.write_gate_for(Operation::Text)?;
        check_needle(needle)?;
        let pieces = map_text(replacement)?;
        let mut per_para = Vec::new();
        let mut refused = Vec::new();
        for p in self.scoped(&opts.scope)? {
            let flow = self.flow(p).expect("scoped paragraphs exist");
            let mut edits = Vec::new();
            for (s, _) in flow.text.match_indices(needle) {
                let range = s..s + needle.len();
                match flow.classify(&range, opts.formatting) {
                    Ok(()) => edits.push((range, replacement == needle)),
                    Err(reason) => refused.push(Refusal { para: p, range, reason }),
                }
            }
            if !edits.is_empty() {
                per_para.push((p, edits.into_iter().map(|(range, noop)| (Edit { range, pieces: pieces.clone() }, noop)).collect()));
            }
        }
        self.execute(per_para, refused, opts.on_refused)
    }

    /// Replaces one match from [`Self::find`] (ADR-0007 §7). See
    /// [`Self::replace_matches`].
    pub fn replace_at(&mut self, m: &Match, replacement: &str, opts: &ReplaceOptions) -> Result<ReplaceReport> {
        self.replace_matches(&[(m, replacement)], opts)
    }

    /// Replaces several matches from [`Self::find`] atomically, each with
    /// its own replacement (ADR-0007 §7, revision 3). Every check of §1.4
    /// and the gates are re-run with `opts` (`m.status` is not trusted;
    /// `opts.scope` is ignored).
    ///
    /// Errors: [`Error::StaleMatch`] when a match comes from another
    /// [`Document`], its paragraph's [`Self::generation`] changed since
    /// `find`, or its text is no longer at its range;
    /// [`Error::OverlappingMatches`]; [`Error::InvalidXmlChar`];
    /// [`Error::ReplaceRefused`]; the gates.
    pub fn replace_matches(&mut self, edits: &[(&Match, &str)], opts: &ReplaceOptions) -> Result<ReplaceReport> {
        self.write_gate_for(Operation::Text)?;
        let mut by_para: std::collections::BTreeMap<ParaRef, Vec<(Edit, bool)>> = std::collections::BTreeMap::new();
        for (m, replacement) in edits {
            // a hand-made Match with an empty range would insert, bypassing
            // find's EmptyNeedle, or count a write that never happens (review
            // of PR #52, m2)
            if m.range.is_empty() {
                return Err(Error::EmptyNeedle);
            }
            let fresh = m.doc == self.id
                && m.generation == self.generation(m.para)
                && self.paragraph_at(m.para).is_some_and(|p| p.text().get(m.range.clone()) == Some(m.text.as_str()));
            if !fresh {
                return Err(Error::StaleMatch);
            }
            let pieces = map_text(replacement)?;
            by_para.entry(m.para).or_default().push((Edit { range: m.range.clone(), pieces }, *replacement == m.text));
        }
        let mut per_para = Vec::new();
        let mut refused = Vec::new();
        for (p, mut list) in by_para {
            list.sort_by_key(|(e, _)| e.range.start);
            if list.windows(2).any(|w| w[0].0.range.end > w[1].0.range.start) {
                return Err(Error::OverlappingMatches);
            }
            let flow = self.flow(p).expect("checked above");
            let mut ok = Vec::new();
            for (e, noop) in list {
                match flow.classify(&e.range, opts.formatting) {
                    Ok(()) => ok.push((e, noop)),
                    Err(reason) => refused.push(Refusal { para: p, range: e.range, reason }),
                }
            }
            if !ok.is_empty() {
                per_para.push((p, ok));
            }
        }
        self.execute(per_para, refused, opts.on_refused)
    }

    /// Plans, dry-runs and writes accepted edits (`(edit, is_noop)` per
    /// paragraph, in document order). `refused` are the matches the checks
    /// already refused.
    fn execute(
        &mut self,
        mut per_para: Vec<(ParaRef, Vec<(Edit, bool)>)>,
        mut refused: Vec<Refusal>,
        on_refused: OnRefused,
    ) -> Result<ReplaceReport> {
        if !refused.is_empty() && on_refused == OnRefused::Refuse {
            return Err(Error::ReplaceRefused { refused });
        }
        let replaced_total: usize = per_para.iter().map(|(_, l)| l.len()).sum();
        let mut skipped_by_dry_run = 0usize;
        // Plan and dry-run every paragraph; a dry-run failure refuses the
        // matches of that run (and, under Skip, the paragraph is planned
        // again without them).
        let mut plans: Vec<(ParaRef, Vec<RunPlan>)> = Vec::new();
        for (p, list) in &mut per_para {
            loop {
                let edits: Vec<Edit> = list.iter().filter(|(_, noop)| !noop).map(|(e, _)| e.clone()).collect();
                let flow = self.flow(*p).expect("planned paragraphs exist");
                let run_plans = plan_paragraph(&flow, &edits);
                drop(flow);
                let mut failed: Option<(Vec<usize>, ooxml_core::Error)> = None;
                for plan in &run_plans {
                    if let Err(e) = self.with_run(plan.run, |lens| dry_run(lens, plan).map_err(Error::from)) {
                        let core = match e {
                            Error::Core(c) => c,
                            other => ooxml_core::Error::Io(std::io::Error::other(other.to_string())),
                        };
                        failed = Some((plan.edits.clone(), core));
                        break;
                    }
                }
                let Some((bad, core)) = failed else {
                    plans.push((*p, run_plans));
                    break;
                };
                let core = Arc::new(core);
                let bad_ranges: Vec<Range<usize>> = bad.iter().map(|&i| edits[i].range.clone()).collect();
                for r in &bad_ranges {
                    refused.push(Refusal { para: *p, range: r.clone(), reason: RefusalReason::Core(core.clone()) });
                }
                if on_refused == OnRefused::Refuse {
                    refused.sort_by_key(|r| (r.para, r.range.start));
                    return Err(Error::ReplaceRefused { refused });
                }
                list.retain(|(e, _)| !bad_ranges.contains(&e.range));
                skipped_by_dry_run += bad_ranges.len();
            }
        }
        // Write. After the dry runs nothing below fails.
        for (p, run_plans) in &plans {
            for plan in run_plans {
                self.with_run(plan.run, |lens| apply(lens, plan))?;
            }
            if !run_plans.is_empty() {
                self.bump(*p);
            }
        }
        refused.sort_by_key(|r| (r.para, r.range.start));
        Ok(ReplaceReport { replaced: replaced_total - skipped_by_dry_run, skipped: refused })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neutral_table() {
        for c in ['a', '~', '\u{3001}', 'あ', 'ア', '漢', '한', '\u{FF01}', '\u{F900}', '\u{3400}', '\u{3105}'] {
            assert!(hint_neutral(c), "{c:?}");
        }
        for c in ['\u{2019}', '\u{2013}', '\u{A0}', '\u{2026}', '\u{1100}', '\u{31F0}', '\u{FE30}', '\u{20000}', '\u{FF00}'] {
            assert!(!hint_neutral(c), "{c:?}");
        }
    }
}
