//! Reference find/replace classifier for the T7 differential oracle (ADR-0007 §9).
//!
//! Written independently of the production code and of the Python spike, over `roxmltree`,
//! from ADR-0007 §0, §1.1–§1.4, §2.1–§2.2 and ADR-0004 §3 / §6 only.
//!
//! Interpretations where the ADRs are silent or ambiguous are marked `ASSUMPTION` below.
//! The general rule for those is "fail closed": when unsure, refuse.
// `allow`, not `expect`: each including test binary uses a different subset.
#![allow(dead_code)]

use roxmltree::{Document, Node, NodeType};
use std::ops::Range;

const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Formatting {
    Identical,
    IgnoreInvisible,
    FirstRun,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefMatch {
    pub para: usize,
    pub range: Range<usize>,
    pub outcome: Result<(), &'static str>,
}

const INSIDE_FIELD: &str = "InsideField";
const INSIDE_REVISION: &str = "InsideRevision";
const UNSUPPORTED_CONTEXT: &str = "UnsupportedContext";
const RUN_NOT_TEXT_ONLY: &str = "RunNotTextOnly";
const CROSSES_BOUNDARY: &str = "MatchCrossesBoundary";
const MARKER_INSIDE: &str = "MarkerInsideMatch";
const MIXED_FORMATTING: &str = "MixedFormatting";
const UNPRESERVED_WS: &str = "UnpreservedWhitespace";

/// One story paragraph: its "all revisions accepted" text (ADR-0004 §3.1 / ADR-0007 §1.1).
pub fn paragraph_texts(part_xml: &str) -> Vec<String> {
    let doc = parse(part_xml);
    story_paragraphs(&doc).into_iter().map(|p| p.text).collect()
}

/// Every match of `needle` (leftmost-first, non-overlapping, like str::match_indices) in every
/// paragraph of the story part, with the outcome `replace` must have.
pub fn classify(part_xml: &str, needle: &str, formatting: Formatting) -> Vec<RefMatch> {
    // ADR-0007 §1.1: an empty needle is a call error, not a set of matches.
    assert!(
        !needle.is_empty(),
        "reference classifier: empty needle (EmptyNeedle)"
    );
    let doc = parse(part_xml);
    let mut out = Vec::new();
    for (pi, p) in story_paragraphs(&doc).iter().enumerate() {
        for (s, m) in p.text.match_indices(needle) {
            let e = s + m.len();
            out.push(RefMatch {
                para: pi,
                range: s..e,
                outcome: classify_match(p, s, e, formatting),
            });
        }
    }
    out
}

/// [`classify`] for several needles, parsing the part once (harness glue added by the
/// production author: no classification logic).
pub fn classify_many(part_xml: &str, needles: &[&str], formatting: Formatting) -> Vec<Vec<RefMatch>> {
    let doc = parse(part_xml);
    let paras = story_paragraphs(&doc);
    needles
        .iter()
        .map(|needle| {
            assert!(!needle.is_empty());
            let mut out = Vec::new();
            for (pi, p) in paras.iter().enumerate() {
                for (s, m) in p.text.match_indices(needle) {
                    let e = s + m.len();
                    out.push(RefMatch { para: pi, range: s..e, outcome: classify_match(p, s, e, formatting) });
                }
            }
            out
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Model
// ---------------------------------------------------------------------------------------------

struct Wt {
    /// Byte range of this `w:t`'s text in the paragraph text.
    start: usize,
    end: usize,
    preserve: bool,
    first_ws: bool,
    last_ws: bool,
}

struct RunInfo<'a, 'i> {
    /// Byte range of the run's characters in the paragraph text (empty for deleted runs).
    start: usize,
    end: usize,
    /// `None` = text run (passes R1–R4, no `w:rPrChange`).
    fail: Option<&'static str>,
    rpr: Option<Node<'a, 'i>>,
    wts: Vec<Wt>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    Transparent,
    Anchor,
    Boundary,
}

enum Item<'a, 'i> {
    Run(RunInfo<'a, 'i>),
    /// A zero-width flow element at a byte position of the paragraph text.
    Point(usize, Class),
}

struct Para<'a, 'i> {
    text: String,
    items: Vec<Item<'a, 'i>>,
}

fn parse(xml: &str) -> Document<'_> {
    Document::parse(xml).expect("reference classifier: part is not well-formed XML")
}

fn w_local<'a>(n: Node<'a, '_>) -> Option<&'a str> {
    if n.is_element() && n.tag_name().namespace() == Some(W) {
        Some(n.tag_name().name())
    } else {
        None
    }
}

fn is_w(n: Node, local: &str) -> bool {
    w_local(n) == Some(local)
}

fn is_ws_only_text(n: Node) -> bool {
    n.node_type() == NodeType::Text && n.text().unwrap_or("").chars().all(is_xml_ws)
}

fn is_xml_ws(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\r' | '\n')
}

// ---------------------------------------------------------------------------------------------
// Story walk (ADR-0007 §2.1, ADR-0004 §3.1)
// ---------------------------------------------------------------------------------------------

fn story_paragraphs<'a, 'i>(doc: &'a Document<'i>) -> Vec<Para<'a, 'i>> {
    let root = doc.root_element();
    // R1's field depth: one counter per story part, in document order (ADR-0007 §2.1).
    let mut depth = 0usize;
    let mut out = Vec::new();
    match w_local(root) {
        Some("document") => {
            for body in root.children().filter(|c| is_w(*c, "body")) {
                walk_blocks(body, false, &mut depth, &mut out);
            }
        }
        Some("hdr") | Some("ftr") => walk_blocks(root, false, &mut depth, &mut out),
        Some(kind @ ("footnotes" | "endnotes")) => {
            let note = if kind == "footnotes" {
                "footnote"
            } else {
                "endnote"
            };
            for n in root.children().filter(|c| is_w(*c, note)) {
                // ADR-0007 §2.2: runs of separator notes are UnsupportedContext (SeparatorNote).
                let separator = matches!(
                    n.attribute((W, "type")),
                    Some("separator" | "continuationSeparator" | "continuationNotice")
                );
                walk_blocks(n, separator, &mut depth, &mut out);
            }
        }
        Some("comments") => {
            for c in root.children().filter(|c| is_w(*c, "comment")) {
                walk_blocks(c, false, &mut depth, &mut out);
            }
        }
        _ => panic!(
            "reference classifier: not a WML story root: {:?}",
            root.tag_name()
        ),
    }
    out
}

/// Block-level walk. `r3` is set below a block-level content control or custom XML element
/// (ADR-0004 §3.1: their runs are ContentControl / CustomXml) and in separator notes.
fn walk_blocks<'a, 'i>(
    container: Node<'a, 'i>,
    r3: bool,
    depth: &mut usize,
    out: &mut Vec<Para<'a, 'i>>,
) {
    for child in container.children() {
        match w_local(child) {
            Some("p") => out.push(build_para(child, r3, depth)),
            // ASSUMPTION: tbl/tr/tc are descended generically; their property children
            // (tblPr, trPr, tcPr, tblGrid) hold no paragraphs so the result is the same.
            Some("tbl" | "tr" | "tc") => walk_blocks(child, r3, depth, out),
            Some("sdt") => {
                for sc in child.children().filter(|c| is_w(*c, "sdtContent")) {
                    walk_blocks(sc, true, depth, out);
                }
            }
            Some("customXml") => walk_blocks(child, true, depth, out),
            // Everything else (sectPr, block-level range markers, mc:AlternateContent,
            // altChunk, block-level ins/del, ...) is not walked.
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Paragraph flow (ADR-0007 §1.1)
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Default)]
struct InCtx {
    simple_field: bool,
    revision: bool,
    deleted: bool,
    r3: bool,
}

struct Builder<'a, 'i, 'd> {
    text: String,
    items: Vec<Item<'a, 'i>>,
    depth: &'d mut usize,
}

impl<'a, 'i> Builder<'a, 'i, '_> {
    fn point(&mut self, class: Class) {
        let pos = self.text.len();
        self.items.push(Item::Point(pos, class));
    }
}

fn build_para<'a, 'i>(p: Node<'a, 'i>, r3: bool, depth: &mut usize) -> Para<'a, 'i> {
    let mut b = Builder {
        text: String::new(),
        items: Vec::new(),
        depth,
    };
    let ctx = InCtx {
        r3,
        ..InCtx::default()
    };
    walk_inline(p, ctx, &mut b);
    Para {
        text: b.text,
        items: b.items,
    }
}

const ANCHORS: &[&str] = &[
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

/// Property children of inline wrappers: not flow, skipped.
const WRAPPER_PROPS: &[&str] = &["smartTagPr", "customXmlPr", "fldData"];

fn walk_inline<'a, 'i>(parent: Node<'a, 'i>, ctx: InCtx, b: &mut Builder<'a, 'i, '_>) {
    let kids: Vec<Node<'a, 'i>> = parent.children().collect();
    let mut i = 0;
    while i < kids.len() {
        let n = kids[i];
        i += 1;
        match n.node_type() {
            NodeType::Text => {
                if !is_ws_only_text(n) {
                    // ASSUMPTION: non-whitespace text directly in w:p (or a wrapper) is an
                    // "unknown child": boundary.
                    b.point(Class::Boundary);
                }
                continue;
            }
            NodeType::Comment | NodeType::PI => {
                // ASSUMPTION: the ADR is silent on comments/PIs among a paragraph's children
                // (ADR-0004 refuses them only inside w:r). Fail closed: boundary.
                b.point(Class::Boundary);
                continue;
            }
            NodeType::Element => {}
            NodeType::Root => continue,
        }
        let Some(local) = w_local(n) else {
            // m:oMath, mc:AlternateContent, w14:..., any foreign element.
            b.point(Class::Boundary);
            continue;
        };
        match local {
            "pPr" if is_w(parent, "p") => {}
            l if WRAPPER_PROPS.contains(&l) => {}
            "r" => process_run(n, ctx, b),
            "proofErr" => b.point(Class::Transparent),
            "bookmarkStart" => {
                // Collapsed bookmark: bookmarkStart immediately followed (next sibling, only
                // whitespace-only text between) by the bookmarkEnd with the same w:id.
                // ASSUMPTION: both w:id must be present; a comment/PI between breaks it.
                let mut j = i;
                while j < kids.len() && is_ws_only_text(kids[j]) {
                    j += 1;
                }
                let id = n.attribute((W, "id"));
                let collapsed = j < kids.len()
                    && is_w(kids[j], "bookmarkEnd")
                    && id.is_some()
                    && kids[j].attribute((W, "id")) == id;
                if collapsed {
                    b.point(Class::Transparent);
                    i = j + 1;
                } else {
                    b.point(Class::Anchor);
                }
            }
            l if ANCHORS.contains(&l) => b.point(Class::Anchor),
            "hyperlink" => wrapper(n, ctx, b),
            "smartTag" | "customXml" | "dir" | "bdo" => wrapper(n, InCtx { r3: true, ..ctx }, b),
            "sdt" => {
                b.point(Class::Boundary);
                for sc in n.children().filter(|c| is_w(*c, "sdtContent")) {
                    walk_inline(sc, InCtx { r3: true, ..ctx }, b);
                }
                b.point(Class::Boundary);
            }
            "fldSimple" => wrapper(
                n,
                InCtx {
                    simple_field: true,
                    ..ctx
                },
                b,
            ),
            "ins" | "moveTo" => wrapper(
                n,
                InCtx {
                    revision: true,
                    ..ctx
                },
                b,
            ),
            "del" | "moveFrom" => wrapper(
                n,
                InCtx {
                    revision: true,
                    deleted: true,
                    ..ctx
                },
                b,
            ),
            _ => b.point(Class::Boundary),
        }
    }
}

fn wrapper<'a, 'i>(n: Node<'a, 'i>, ctx: InCtx, b: &mut Builder<'a, 'i, '_>) {
    b.point(Class::Boundary);
    walk_inline(n, ctx, b);
    b.point(Class::Boundary);
}

fn process_run<'a, 'i>(r: Node<'a, 'i>, ctx: InCtx, b: &mut Builder<'a, 'i, '_>) {
    let mut rpr: Option<Node<'a, 'i>> = None;
    let mut text = String::new();
    let mut wts: Vec<Wt> = Vec::new();
    let mut r4 = false;
    let mut holds_fldchar = false;
    let mut depth_after = *b.depth;
    let mut ruby_bases: Vec<Node<'a, 'i>> = Vec::new();

    for c in r.children() {
        match c.node_type() {
            NodeType::Text => {
                if !is_ws_only_text(c) {
                    r4 = true;
                }
                continue;
            }
            NodeType::Comment | NodeType::PI => {
                r4 = true;
                continue;
            }
            NodeType::Element => {}
            NodeType::Root => continue,
        }
        let Some(local) = w_local(c) else {
            r4 = true; // mc:AlternateContent and any foreign content
            continue;
        };
        match local {
            // ASSUMPTION: only the first w:rPr is properties; a second one is content (R4).
            "rPr" if rpr.is_none() => rpr = Some(c),
            "t" => {
                let t: String = c
                    .children()
                    .filter(|x| x.node_type() == NodeType::Text)
                    .map(|x| x.text().unwrap_or(""))
                    .collect();
                // ASSUMPTION: only the w:t's own xml:space counts (no inheritance from
                // ancestors), and only the exact value "preserve".
                let preserve = c.attribute((XML_NS, "space")) == Some("preserve");
                wts.push(Wt {
                    start: text.len(),
                    end: text.len() + t.len(),
                    preserve,
                    first_ws: t.chars().next().is_some_and(is_xml_ws),
                    last_ws: t.chars().next_back().is_some_and(is_xml_ws),
                });
                text.push_str(&t);
            }
            "delText" => {
                // Run::text reads it, but it is not allowed content (R4).
                let t: String = c
                    .children()
                    .filter(|x| x.node_type() == NodeType::Text)
                    .map(|x| x.text().unwrap_or(""))
                    .collect();
                text.push_str(&t);
                r4 = true;
            }
            "tab" => text.push('\t'),
            "br" => {
                text.push('\n');
                // ASSUMPTION: only the w:-qualified type/clear attributes are checked.
                let ty_ok = matches!(c.attribute((W, "type")), None | Some("textWrapping"));
                let clear_ok = matches!(c.attribute((W, "clear")), None | Some("none"));
                if !(ty_ok && clear_ok) {
                    r4 = true;
                }
            }
            "cr" => text.push('\n'),
            "noBreakHyphen" => text.push('\u{2011}'),
            "softHyphen" => text.push('\u{00AD}'),
            "lastRenderedPageBreak" => {}
            "fldChar" => {
                holds_fldchar = true;
                r4 = true;
                match c.attribute((W, "fldCharType")) {
                    Some("begin") => depth_after += 1,
                    Some("end") => depth_after = depth_after.saturating_sub(1),
                    _ => {}
                }
            }
            "ruby" => {
                r4 = true;
                for rb in c.children().filter(|x| is_w(*x, "rubyBase")) {
                    ruby_bases.push(rb);
                }
            }
            _ => r4 = true,
        }
    }

    let in_field = *b.depth > 0 || holds_fldchar;
    *b.depth = depth_after;

    // ASSUMPTION: rPrChange is reported in R2's slot (same reason, InsideRevision), i.e. after
    // R1 and before R3/R4.
    let rpr_change = rpr.is_some_and(|p| p.children().any(|x| is_w(x, "rPrChange")));
    // ASSUMPTION: R1–R4 are independent conditions checked in order, so a run in w:ins inside a
    // content control is InsideRevision (R2 before R3), whatever the outermost wrapper is.
    let fail = if in_field || ctx.simple_field {
        Some(INSIDE_FIELD)
    } else if ctx.revision || rpr_change {
        Some(INSIDE_REVISION)
    } else if ctx.r3 {
        Some(UNSUPPORTED_CONTEXT)
    } else if r4 {
        Some(RUN_NOT_TEXT_ONLY)
    } else {
        None
    };

    // Paragraph::text() omits runs under w:del / w:moveFrom.
    let start = b.text.len();
    if !ctx.deleted {
        b.text.push_str(&text);
    }
    let end = b.text.len();
    if fail.is_none() && start == end {
        // A text run with no text is transparent.
        b.items.push(Item::Point(start, Class::Transparent));
    } else {
        for wt in &mut wts {
            wt.start += start;
            wt.end += start;
        }
        b.items.push(Item::Run(RunInfo {
            start,
            end,
            fail,
            rpr,
            wts,
        }));
    }

    // w:r/w:ruby/w:rubyBase: descended after the enclosing run, context Ruby (R3).
    for rb in ruby_bases {
        walk_inline(rb, InCtx { r3: true, ..ctx }, b);
    }
}

// ---------------------------------------------------------------------------------------------
// Match classification (ADR-0007 §1.1–§1.4)
// ---------------------------------------------------------------------------------------------

fn classify_match(
    p: &Para,
    s: usize,
    e: usize,
    formatting: Formatting,
) -> Result<(), &'static str> {
    // 1. A matched character in a run that is not a text run: its own refusal, first run first.
    for it in &p.items {
        if let Item::Run(r) = it
            && let Some(kind) = r.fail
            && r.start < r.end
            && r.start < e
            && r.end > s
        {
            return Err(kind);
        }
    }
    // 2. A boundary strictly inside the match (wrapper edges, failing runs without text,
    //    other children). Failing runs with text inside the match were caught by step 1.
    let strictly_inside = |pos: usize| s < pos && pos < e;
    for it in &p.items {
        let hit = match it {
            Item::Point(pos, Class::Boundary) => strictly_inside(*pos),
            Item::Run(r) if r.fail.is_some() => strictly_inside(r.start) || strictly_inside(r.end),
            _ => false,
        };
        if hit {
            return Err(CROSSES_BOUNDARY);
        }
    }
    // 3. An anchor strictly inside the match.
    if p.items
        .iter()
        .any(|it| matches!(it, Item::Point(pos, Class::Anchor) if strictly_inside(*pos)))
    {
        return Err(MARKER_INSIDE);
    }
    // r₀ … rₖ: the text runs holding matched characters, in order.
    let runs: Vec<&RunInfo> = p
        .items
        .iter()
        .filter_map(|it| match it {
            Item::Run(r) if r.fail.is_none() && r.start < e && r.end > s => Some(r),
            _ => None,
        })
        .collect();
    assert!(!runs.is_empty(), "a match with no run (internal error)");
    // 4. Formatting rule.
    if !formatting_ok(p, &runs, s, e, formatting) {
        return Err(MIXED_FORMATTING);
    }
    // 5. Unpreserved edge whitespace outside the match in a w:t that holds matched characters.
    for r in &runs {
        for wt in &r.wts {
            if wt.preserve || wt.start >= e || wt.end <= s {
                continue;
            }
            if (wt.first_ws && wt.start < s) || (wt.last_ws && wt.end > e) {
                return Err(UNPRESERVED_WS);
            }
        }
    }
    Ok(())
}

fn formatting_ok(p: &Para, runs: &[&RunInfo], s: usize, e: usize, f: Formatting) -> bool {
    let canon0 = canon_rpr(runs[0].rpr);
    match f {
        Formatting::FirstRun => true,
        Formatting::Identical => runs[1..].iter().all(|r| canon_rpr(r.rpr) == canon0),
        Formatting::IgnoreInvisible => {
            let (base0, hint0) = strip_invisible(canon0);
            runs[1..].iter().all(|r| {
                let (base, hint) = strip_invisible(canon_rpr(r.rpr));
                if base != base0 {
                    return false;
                }
                if hint == hint0 {
                    return true;
                }
                // Hints differ. Ignored only when both are eastAsia or absent.
                // ASSUMPTION: an explicit w:hint="default" is not "absent" (fail closed).
                let ok_hint = |h: &Option<String>| matches!(h.as_deref(), None | Some("eastAsia"));
                if !(ok_hint(&hint) && ok_hint(&hint0)) {
                    return false;
                }
                // ASSUMPTION: "the run whose hint differs" = this rᵢ compared with r₀ (the
                // replacement takes r₀'s properties); its matched characters must all be
                // hint-neutral.
                let lo = s.max(r.start);
                let hi = e.min(r.end);
                p.text[lo..hi].chars().all(hint_neutral)
            })
        }
    }
}

fn hint_neutral(c: char) -> bool {
    matches!(c as u32,
        0x0000..=0x007F
        | 0x3000..=0x303F
        | 0x3040..=0x30FF
        | 0x3100..=0x312F
        | 0x3400..=0x4DBF
        | 0x4E00..=0x9FFF
        | 0xAC00..=0xD7AF
        | 0xF900..=0xFAFF
        | 0xFF01..=0xFFEF)
}

/// Canonical element tree: namespace URI + local name, attributes sorted by expanded name,
/// whitespace-only text dropped, adjacent text merged. Prefixes and namespace declarations
/// are invisible.
/// ASSUMPTION: comments and PIs inside w:rPr are kept (not listed as ignorable).
#[derive(Clone, Debug, PartialEq, Eq)]
enum Canon {
    Elem {
        ns: Option<String>,
        name: String,
        attrs: Vec<(Option<String>, String, String)>,
        kids: Vec<Canon>,
    },
    Text(String),
    Comment(String),
    Pi(String, Option<String>),
}

fn canon_rpr(rpr: Option<Node>) -> Canon {
    match rpr {
        Some(n) => canon(n),
        // A missing w:rPr equals an empty one.
        None => Canon::Elem {
            ns: Some(W.to_string()),
            name: "rPr".to_string(),
            attrs: Vec::new(),
            kids: Vec::new(),
        },
    }
}

fn canon(n: Node) -> Canon {
    let mut attrs: Vec<(Option<String>, String, String)> = n
        .attributes()
        .map(|a| {
            (
                a.namespace().map(str::to_string),
                a.name().to_string(),
                a.value().to_string(),
            )
        })
        .collect();
    attrs.sort();
    let mut kids: Vec<Canon> = Vec::new();
    let mut pending = String::new();
    for c in n.children() {
        if c.node_type() == NodeType::Text {
            pending.push_str(c.text().unwrap_or(""));
            continue;
        }
        flush_text(&mut pending, &mut kids);
        match c.node_type() {
            NodeType::Element => kids.push(canon(c)),
            NodeType::Comment => kids.push(Canon::Comment(c.text().unwrap_or("").to_string())),
            NodeType::PI => {
                if let Some(pi) = c.pi() {
                    kids.push(Canon::Pi(
                        pi.target.to_string(),
                        pi.value.map(str::to_string),
                    ));
                }
            }
            _ => {}
        }
    }
    flush_text(&mut pending, &mut kids);
    Canon::Elem {
        ns: n.tag_name().namespace().map(str::to_string),
        name: n.tag_name().name().to_string(),
        attrs,
        kids,
    }
}

fn flush_text(pending: &mut String, kids: &mut Vec<Canon>) {
    if !pending.chars().all(is_xml_ws) {
        kids.push(Canon::Text(std::mem::take(pending)));
    }
    pending.clear();
}

/// IgnoreInvisible: drop w:noProof, w:lang/@w:val and w:rFonts/@w:hint (direct children of
/// w:rPr); return the first rFonts hint separately.
/// RESOLVED after the T7 comparison (it was an ASSUMPTION keeping them): a w:lang / w:rFonts
/// left with no attribute and no child by the drop is dropped too, so
/// `<w:rFonts w:hint="eastAsia"/>` against no w:rFonts is an invisible difference. ADR-0007
/// §1.3 counts such run pairs as "differing only in hint", and its implementation notes record
/// the rule.
fn strip_invisible(c: Canon) -> (Canon, Option<String>) {
    let Canon::Elem {
        ns,
        name,
        attrs,
        kids,
    } = c
    else {
        unreachable!()
    };
    let mut hint = None;
    let mut seen_rfonts = false;
    let mut out = Vec::new();
    for k in kids {
        match k {
            Canon::Elem {
                ns: kns,
                name: kname,
                attrs: mut kattrs,
                kids: kkids,
            } if kns.as_deref() == Some(W) => {
                let had_attrs = !kattrs.is_empty();
                match kname.as_str() {
                    "noProof" => continue,
                    "lang" => {
                        kattrs.retain(|(ans, an, _)| !(ans.as_deref() == Some(W) && an == "val"))
                    }
                    "rFonts" => {
                        let h = kattrs
                            .iter()
                            .find(|(ans, an, _)| ans.as_deref() == Some(W) && an == "hint")
                            .map(|(_, _, v)| v.clone());
                        if !seen_rfonts {
                            hint = h;
                            seen_rfonts = true;
                        }
                        kattrs.retain(|(ans, an, _)| !(ans.as_deref() == Some(W) && an == "hint"));
                    }
                    _ => {}
                }
                let emptied = matches!(kname.as_str(), "lang" | "rFonts") && had_attrs && kattrs.is_empty() && kkids.is_empty();
                if emptied {
                    continue;
                }
                out.push(Canon::Elem {
                    ns: kns,
                    name: kname,
                    attrs: kattrs,
                    kids: kkids,
                });
            }
            other => out.push(other),
        }
    }
    (
        Canon::Elem {
            ns,
            name,
            attrs,
            kids: out,
        },
        hint,
    )
}

// ---------------------------------------------------------------------------------------------
// Self-tests on hand-written XML
// ---------------------------------------------------------------------------------------------

#[cfg(test)]
mod selftests {
    use super::*;

    const NS: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#;

    fn body(inner: &str) -> String {
        format!("<w:document {NS}><w:body>{inner}</w:body></w:document>")
    }

    fn outcomes(xml: &str, needle: &str, f: Formatting) -> Vec<Result<(), &'static str>> {
        classify(xml, needle, f)
            .into_iter()
            .map(|m| m.outcome)
            .collect()
    }

    fn one(xml: &str, needle: &str, f: Formatting) -> Result<(), &'static str> {
        let v = outcomes(xml, needle, f);
        assert_eq!(
            v.len(),
            1,
            "expected exactly one match of {needle:?}: {v:?}"
        );
        v[0]
    }

    #[test]
    fn split_placeholder_with_proof_err_and_collapsed_bookmark() {
        let xml = body(
            r#"<w:p><w:r><w:t>Dear {{na</w:t></w:r><w:proofErr w:type="spellStart"/>
               <w:bookmarkStart w:id="0" w:name="_GoBack"/><w:bookmarkEnd w:id="0"/>
               <w:r><w:t>me}}!</w:t></w:r></w:p>"#,
        );
        assert_eq!(paragraph_texts(&xml), vec!["Dear {{name}}!".to_string()]);
        let m = classify(&xml, "{{name}}", Formatting::Identical);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].range, 5..13);
        assert_eq!(m[0].outcome, Ok(()));
    }

    #[test]
    fn anchors_inside_and_at_edges() {
        let xml = body(
            r#"<w:p><w:bookmarkStart w:id="1" w:name="b"/><w:r><w:t>ab</w:t></w:r>
               <w:commentRangeStart w:id="2"/><w:r><w:t>cd</w:t></w:r>
               <w:bookmarkEnd w:id="1"/></w:p>"#,
        );
        assert_eq!(one(&xml, "abcd", Formatting::Identical), Err(MARKER_INSIDE));
        assert_eq!(one(&xml, "ab", Formatting::Identical), Ok(()));
        assert_eq!(one(&xml, "cd", Formatting::Identical), Ok(()));
    }

    #[test]
    fn hyperlink_and_wrappers_are_boundaries() {
        let xml = body(
            r#"<w:p><w:r><w:t>see </w:t></w:r><w:hyperlink r:id="x" xmlns:r="urn:r"><w:r><w:t>here</w:t></w:r></w:hyperlink></w:p>"#,
        );
        assert_eq!(
            one(&xml, "e h", Formatting::Identical),
            Err(CROSSES_BOUNDARY)
        );
        assert_eq!(one(&xml, "here", Formatting::Identical), Ok(()));
        let xml = body(
            r#"<w:p><w:smartTag w:uri="u" w:element="e"><w:r><w:t>abc</w:t></w:r></w:smartTag></w:p>"#,
        );
        assert_eq!(
            one(&xml, "b", Formatting::Identical),
            Err(UNSUPPORTED_CONTEXT)
        );
    }

    #[test]
    fn formatting_modes() {
        let xml = body(
            r#"<w:p><w:r><w:rPr><w:b/></w:rPr><w:t>{{a</w:t></w:r><w:r><w:t>}}</w:t></w:r></w:p>"#,
        );
        assert_eq!(
            one(&xml, "{{a}}", Formatting::Identical),
            Err(MIXED_FORMATTING)
        );
        assert_eq!(
            one(&xml, "{{a}}", Formatting::IgnoreInvisible),
            Err(MIXED_FORMATTING)
        );
        assert_eq!(one(&xml, "{{a}}", Formatting::FirstRun), Ok(()));

        // Prefix-independent, attribute-order-independent; lang/@val and noProof invisible.
        let xml = format!(
            r#"<x:document xmlns:x="{W}"><x:body><x:p>
               <x:r><x:rPr><x:lang x:val="en-US" x:eastAsia="ja-JP"/><x:noProof/></x:rPr><x:t>{{{{a</x:t></x:r>
               <x:r><x:rPr><x:lang x:eastAsia="ja-JP" x:val="fr-FR"/></x:rPr><x:t>}}}}</x:t></x:r>
               </x:p></x:body></x:document>"#
        );
        assert_eq!(
            one(&xml, "{{a}}", Formatting::Identical),
            Err(MIXED_FORMATTING)
        );
        assert_eq!(one(&xml, "{{a}}", Formatting::IgnoreInvisible), Ok(()));
    }

    #[test]
    fn hint_neutrality() {
        let mk = |second: &str| {
            body(&format!(
                r#"<w:p><w:r><w:rPr><w:rFonts w:ascii="A" w:hint="eastAsia"/></w:rPr><w:t>x</w:t></w:r><w:r><w:rPr><w:rFonts w:ascii="A"/></w:rPr><w:t>{second}</w:t></w:r></w:p>"#
            ))
        };
        assert_eq!(one(&mk("y漢"), "xy漢", Formatting::IgnoreInvisible), Ok(()));
        assert_eq!(
            one(&mk("y’"), "xy’", Formatting::IgnoreInvisible),
            Err(MIXED_FORMATTING)
        );
        assert_eq!(
            one(&mk("y漢"), "xy漢", Formatting::Identical),
            Err(MIXED_FORMATTING)
        );
        let cs = body(
            r#"<w:p><w:r><w:rPr><w:rFonts w:hint="cs"/></w:rPr><w:t>x</w:t></w:r><w:r><w:t>y</w:t></w:r></w:p>"#,
        );
        assert_eq!(
            one(&cs, "xy", Formatting::IgnoreInvisible),
            Err(MIXED_FORMATTING)
        );
    }

    #[test]
    fn complex_field_across_paragraphs_and_precedence() {
        let xml = body(
            r#"<w:p><w:r><w:t>A</w:t></w:r><w:r><w:fldChar w:fldCharType="begin"/></w:r>
               <w:r><w:instrText> PAGE </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r>
               <w:r><w:t>12</w:t></w:r></w:p>
               <w:p><w:r><w:t>34</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r><w:r><w:t>B</w:t></w:r></w:p>"#,
        );
        assert_eq!(paragraph_texts(&xml), vec!["A12", "34B"]);
        assert_eq!(one(&xml, "A1", Formatting::Identical), Err(INSIDE_FIELD));
        assert_eq!(one(&xml, "4B", Formatting::Identical), Err(INSIDE_FIELD));
        assert_eq!(one(&xml, "A", Formatting::Identical), Ok(()));
        assert_eq!(one(&xml, "B", Formatting::Identical), Ok(()));
        let simple = body(
            r#"<w:p><w:r><w:t>x</w:t></w:r><w:fldSimple w:instr="PAGE"><w:r><w:t>1</w:t></w:r></w:fldSimple><w:r><w:t>y</w:t></w:r></w:p>"#,
        );
        assert_eq!(
            one(&simple, "x1y", Formatting::Identical),
            Err(INSIDE_FIELD)
        );
    }

    #[test]
    fn revisions() {
        let xml = body(
            r#"<w:p><w:r><w:t>ab</w:t></w:r><w:del w:id="1" w:author="a"><w:r><w:delText>XX</w:delText></w:r></w:del>
               <w:r><w:t>cd</w:t></w:r><w:ins w:id="2" w:author="a"><w:r><w:t>ef</w:t></w:r></w:ins>
               <w:r><w:rPr><w:b/><w:rPrChange w:id="3" w:author="a"><w:rPr/></w:rPrChange></w:rPr><w:t>gh</w:t></w:r></w:p>"#,
        );
        assert_eq!(paragraph_texts(&xml), vec!["abcdefgh"]);
        assert_eq!(
            one(&xml, "bc", Formatting::Identical),
            Err(CROSSES_BOUNDARY)
        );
        assert_eq!(one(&xml, "e", Formatting::Identical), Err(INSIDE_REVISION));
        assert_eq!(one(&xml, "g", Formatting::Identical), Err(INSIDE_REVISION));
    }

    #[test]
    fn run_not_text_only_and_empty_failing_run_as_boundary() {
        let xml = body(
            r#"<w:p><w:r><w:t>ab</w:t></w:r><w:r><w:footnoteReference w:id="1"/></w:r><w:r><w:t>cd</w:t></w:r>
               <w:r><w:t>e</w:t><w:br w:type="page"/><w:t>f</w:t></w:r></w:p>"#,
        );
        assert_eq!(paragraph_texts(&xml), vec!["abcde\nf"]);
        assert_eq!(
            one(&xml, "bc", Formatting::Identical),
            Err(CROSSES_BOUNDARY)
        );
        assert_eq!(
            one(&xml, "f", Formatting::Identical),
            Err(RUN_NOT_TEXT_ONLY)
        );
        assert_eq!(one(&xml, "ab", Formatting::Identical), Ok(()));
    }

    #[test]
    fn unpreserved_whitespace() {
        let xml = body(r#"<w:p><w:r><w:t> {{x}}</w:t></w:r><w:r><w:t>{{y}} </w:t></w:r></w:p>"#);
        assert_eq!(
            one(&xml, "{{x}}", Formatting::Identical),
            Err(UNPRESERVED_WS)
        );
        assert_eq!(one(&xml, " {{x}}", Formatting::Identical), Ok(()));
        assert_eq!(
            one(&xml, "{{y}}", Formatting::Identical),
            Err(UNPRESERVED_WS)
        );
        assert_eq!(
            one(&xml, "}}{{y", Formatting::Identical),
            Err(UNPRESERVED_WS)
        );
        let xml = body(r#"<w:p><w:r><w:t xml:space="preserve"> {{x}} </w:t></w:r></w:p>"#);
        assert_eq!(one(&xml, "{{x}}", Formatting::Identical), Ok(()));
    }

    #[test]
    fn story_walk_tables_sdt_and_notes() {
        let xml = body(
            r#"<w:p><w:r><w:t>one</w:t></w:r></w:p>
               <w:tbl><w:tr><w:tc><w:tbl><w:tr><w:tc><w:p><w:r><w:t>two</w:t></w:r></w:p></w:tc></w:tr></w:tbl></w:tc></w:tr></w:tbl>
               <w:sdt><w:sdtPr/><w:sdtContent><w:p><w:r><w:t>three</w:t></w:r></w:p></w:sdtContent></w:sdt>
               <w:p><w:r><w:drawing><w:p><w:r><w:t>box</w:t></w:r></w:p></w:drawing></w:r></w:p>
               <w:sectPr/>"#,
        );
        assert_eq!(paragraph_texts(&xml), vec!["one", "two", "three", ""]);
        assert_eq!(one(&xml, "two", Formatting::Identical), Ok(()));
        assert_eq!(
            one(&xml, "three", Formatting::Identical),
            Err(UNSUPPORTED_CONTEXT)
        );

        let notes = format!(
            r#"<w:footnotes {NS}><w:footnote w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r></w:p></w:footnote>
               <w:footnote w:type="continuationSeparator" w:id="0"><w:p><w:r><w:t>sep</w:t></w:r></w:p></w:footnote>
               <w:footnote w:id="1"><w:p><w:r><w:footnoteRef/></w:r><w:r><w:t xml:space="preserve"> sep note</w:t></w:r></w:p></w:footnote></w:footnotes>"#
        );
        assert_eq!(paragraph_texts(&notes), vec!["", "sep", " sep note"]);
        let m = classify(&notes, "sep", Formatting::IgnoreInvisible);
        assert_eq!(m.len(), 2);
        assert_eq!((m[0].para, m[0].outcome), (1, Err(UNSUPPORTED_CONTEXT)));
        assert_eq!(
            (m[1].para, m[1].range.clone(), m[1].outcome),
            (2, 1..4, Ok(()))
        );
    }
}
