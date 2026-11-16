//! T7 (ADR-0007 §9): the find/replace differential oracle.
//!
//! For every paragraph of every story of the writable fixtures, three needle
//! sets (the ≤6-character span centred on each run boundary, every `\w{2,}`
//! word, placeholder-shaped tokens) are classified by
//!
//! - (a) the production code, [`Document::find`];
//! - (b) the reference classifier in `tests/reference/` (written by a
//!   different agent from the ADR text only, over `roxmltree`).
//!
//! Both must agree on every match, for each [`RunFormatting`]. Each accepted
//! match (default rule) is then applied and must satisfy (i) the string
//! oracle, (ii) per-character run identity (each character keeps its run,
//! the replacement is in r₀), (iii) T2 locality (the part changes only
//! inside the spans of r₀ … rₖ; other parts unchanged), (iv) byte identity
//! outside the match: in each of r₀ … rₖ the start tag and `w:rPr` are
//! byte-identical and the child sequence equals the original with the
//! affected children replaced by one marker, in order (n3), and no new
//! `w:tab`/`w:br` appears outside the replacement (n1).
//!
//! The needle sets and the application step are sampled on the largest
//! fixture unless `OOXML_T7_FULL=1` (every needle and every accepted match;
//! run with `--release`).

mod common;
mod reference;

use std::collections::BTreeSet;
use std::ops::Range;

use common::*;
use ooxml_docx::{Document, ParaRef, Paragraph, ReplaceOptions, RunFormatting, Scope};

/// Character-boundary-safe slice of `n` characters around byte `b`.
fn around(text: &str, b: usize, n: usize) -> &str {
    let mut s = b;
    for _ in 0..n {
        if s == 0 {
            break;
        }
        s = text.floor_char_boundary(s - 1);
    }
    let mut e = b;
    for _ in 0..n {
        if e >= text.len() {
            break;
        }
        e = text.ceil_char_boundary(e + 1);
    }
    &text[s..e]
}

fn needles(p: &Paragraph<'_>) -> Vec<String> {
    let text = p.text();
    let mut out = Vec::new();
    // (1) boundary needles: between each two text-holding runs
    let mut pos = 0usize;
    let mut boundaries = Vec::new();
    for r in p.runs() {
        let t = r.text();
        if !r.is_removed() && !t.is_empty() {
            if pos > 0 {
                boundaries.push(pos);
            }
            pos += t.len();
        }
    }
    for b in boundaries {
        if b <= text.len() && text.is_char_boundary(b) {
            out.push(around(&text, b, 3).to_owned());
        }
    }
    // (2) words
    let mut word = String::new();
    for c in text.chars().chain(std::iter::once(' ')) {
        if c.is_alphanumeric() || c == '_' {
            word.push(c);
        } else {
            if word.chars().count() >= 2 {
                out.push(std::mem::take(&mut word));
            }
            word.clear();
        }
    }
    // (3) placeholder-shaped tokens
    for (open, close) in [("{{", "}}"), ("«", "»"), ("[", "]"), ("${", "}"), ("{", "}")] {
        let mut from = 0;
        while let Some(i) = text[from..].find(open) {
            let s = from + i;
            match text[s + open.len()..].find(close) {
                Some(j) if j < 64 => {
                    out.push(text[s..s + open.len() + j + close.len()].to_owned());
                    from = s + open.len();
                }
                _ => break,
            }
        }
    }
    out.retain(|n| !n.is_empty());
    out
}

#[derive(Default, Debug)]
struct Stats {
    needles: usize,
    matches: usize,
    accepted: usize,
    applied: usize,
}

fn kind(status: &Result<(), ooxml_docx::Refusal>) -> Result<(), &'static str> {
    status.as_ref().map_err(|r| r.reason.kind()).copied()
}

#[test]
fn production_and_reference_agree_and_accepted_matches_satisfy_the_oracles() {
    let full = std::env::var_os("OOXML_T7_FULL").is_some();
    let mut total = Stats::default();
    for stem in writable() {
        let input = fixture(stem);
        let doc = Document::from_bytes(input.clone()).unwrap();
        let mut stats = Stats::default();
        let mut accepted = Vec::new();
        for story in doc.stories() {
            let part = story.id().part().to_owned();
            let xml = String::from_utf8(doc.package().read_part(&part).unwrap()).unwrap();
            let prod_texts: Vec<String> = story.paragraphs().map(|p| p.text()).collect();
            assert_eq!(prod_texts, reference::paragraph_texts(&xml), "{stem} {part}: paragraph texts");
            let set: BTreeSet<String> = story.paragraphs().flat_map(|p| needles(&p)).collect();
            // Every needle with OOXML_T7_FULL; else at most ~1,500 per story, evenly sampled
            // (the largest fixture has 6,045).
            let step = if full || set.len() <= 1500 { 1 } else { set.len().div_ceil(1500) };
            let list: Vec<&str> = set.iter().step_by(step).map(String::as_str).collect();
            stats.needles += list.len();
            for (fmt, rf) in [
                (RunFormatting::IgnoreInvisible, reference::Formatting::IgnoreInvisible),
                (RunFormatting::Identical, reference::Formatting::Identical),
                (RunFormatting::FirstRun, reference::Formatting::FirstRun),
            ] {
                let refs = reference::classify_many(&xml, &list, rf);
                for (needle, refm) in list.iter().zip(refs) {
                    // only paragraphs holding the needle can match (find over the
                    // whole story per needle would be quadratic)
                    let mut prod = Vec::new();
                    for (i, t) in prod_texts.iter().enumerate() {
                        if t.contains(needle) {
                            let opts = ReplaceOptions::with_scope(Scope::Paragraph(ParaRef::new(story.index(), i))).formatting(fmt);
                            prod.extend(doc.find(needle, &opts).unwrap());
                        }
                    }
                    let a: Vec<(usize, Range<usize>, Result<(), &str>)> =
                        prod.iter().map(|m| (m.para.index(), m.range.clone(), kind(&m.status))).collect();
                    let b: Vec<(usize, Range<usize>, Result<(), &str>)> =
                        refm.into_iter().map(|m| (m.para, m.range, m.outcome)).collect();
                    assert_eq!(a, b, "{stem} {part} {fmt:?} needle {needle:?}");
                    if fmt == RunFormatting::IgnoreInvisible {
                        stats.matches += prod.len();
                        accepted.extend(prod.into_iter().filter(|m| m.status.is_ok()));
                    }
                }
            }
        }
        stats.accepted = accepted.len();
        // Apply accepted matches: one per paragraph per round on one document
        // (generations are per paragraph, so other matches stay valid).
        let step = if full || accepted.len() <= 600 { 1 } else { accepted.len() / 600 };
        let mut doc = Document::from_bytes(input.clone()).unwrap();
        let mut done = BTreeSet::new();
        // (iii) for other parts: at the end of each document, only edited
        // story parts differ from the input.
        let check_parts = |doc: &Document, edited: &BTreeSet<String>| {
            for (a, b) in entries(&doc.to_bytes().unwrap()).iter().zip(entries(&input)) {
                assert_eq!(a.0, b.0);
                if *a != b {
                    assert!(edited.contains(&a.0), "{stem}: (iii) part {} changed", a.0);
                }
            }
        };
        let mut edited = BTreeSet::new();
        for (k, m) in accepted.iter().enumerate().filter(|(k, _)| k % step == 0) {
            if !done.insert(m.para) {
                // a second match in an already edited paragraph: fresh document
                check_parts(&doc, &edited);
                edited.clear();
                doc = Document::from_bytes(input.clone()).unwrap();
                done.clear();
                done.insert(m.para);
            }
            let replacement = ["Z", "", "a\tb\nc", " sp  ", "長い置換"][k % 5];
            edited.insert(doc.story_at(m.para.story()).unwrap().id().part().to_owned());
            apply_and_check(stem, &mut doc, m, replacement);
            stats.applied += 1;
        }
        check_parts(&doc, &edited);
        eprintln!("T7 {stem}: {stats:?}");
        total.needles += stats.needles;
        total.matches += stats.matches;
        total.accepted += stats.accepted;
        total.applied += stats.applied;
    }
    eprintln!("T7 total: {total:?}");
    assert!(total.applied > 500, "{total:?}");
}

/// A child element of a run in the paragraph-text model.
#[derive(Clone, Debug)]
struct ChildBytes {
    bytes: String,
    name: String,
    range: Range<usize>,
}

/// The run elements (roxmltree, document order) of a part.
fn runs_of<'a>(doc: &'a roxmltree::Document<'a>) -> Vec<roxmltree::Node<'a, 'a>> {
    doc.descendants().filter(|n| n.is_element() && n.tag_name().namespace() == Some(W_NS) && n.tag_name().name() == "r").collect()
}

/// Children of a run (elements, `w:rPr` excluded) with their text width
/// starting at `start`.
fn children(xml: &str, run: roxmltree::Node<'_, '_>, start: usize) -> (String, Vec<ChildBytes>) {
    let mut rpr = String::new();
    let mut out = Vec::new();
    let mut pos = start;
    for c in run.children().filter(|c| c.is_element()) {
        let name = c.tag_name().name().to_owned();
        let bytes = xml[c.range()].to_owned();
        if name == "rPr" && c.tag_name().namespace() == Some(W_NS) {
            rpr = bytes;
            continue;
        }
        let w = match name.as_str() {
            "t" => c.text().unwrap_or("").len(),
            "tab" | "br" | "cr" => 1,
            "noBreakHyphen" => 3,
            "softHyphen" => 2,
            _ => 0,
        };
        out.push(ChildBytes { bytes, name, range: pos..pos + w });
        pos += w;
    }
    (rpr, out)
}

fn apply_and_check(stem: &str, doc: &mut Document, m: &ooxml_docx::Match, replacement: &str) {
    let ctx = format!("{stem} {:?} {:?} {:?}", m.para, m.range, m.text);
    let part = doc.story_at(m.para.story()).unwrap().id().part().to_owned();
    let before_part = String::from_utf8(doc.package().read_part(&part).unwrap()).unwrap();
    let p = doc.paragraph_at(m.para).unwrap();
    let before_text = p.text();
    // per run: (index, text range, ordinal among the part's w:r elements)
    let root = doc.story_at(m.para.story()).unwrap().raw();
    let mut all_runs = Vec::new();
    let mut stack = vec![root];
    while let Some(n) = stack.pop() {
        if n.name().ns() == W_NS && n.name().local() == "r" {
            all_runs.push(n as *const ooxml_core::RawNode);
        }
        let kids: Vec<_> = n.elements().collect();
        stack.extend(kids.into_iter().rev());
    }
    let mut runs = Vec::new();
    let mut pos = 0usize;
    for r in p.runs() {
        let len = if r.is_removed() { 0 } else { r.text().len() };
        let ord = all_runs.iter().position(|&q| std::ptr::eq(q, r.raw())).expect("run in its part");
        runs.push((r.index(), pos..pos + len, ord));
        pos += len;
    }
    let held: Vec<_> = runs.iter().filter(|(_, rg, ..)| !rg.is_empty() && rg.start < m.range.end && rg.end > m.range.start).cloned().collect();
    let r0 = held[0].0;
    let before_doc = roxmltree::Document::parse(&before_part).unwrap();
    let before_runs = runs_of(&before_doc);
    let held_ords: Vec<usize> = held.iter().map(|h| h.2).collect();
    assert_eq!(before_runs.len(), all_runs.len(), "{ctx}: roxmltree and ooxml-core agree on w:r count");

    // the match was found in another (identical) Document
    let m = &ooxml_docx::Match { doc: doc.id(), generation: doc.generation(m.para), ..m.clone() };
    doc.replace_at(m, replacement, &ReplaceOptions::default()).unwrap_or_else(|e| panic!("{ctx}: {e}"));

    // (i) string oracle
    let after_text = doc.paragraph_at(m.para).unwrap().text();
    let expected = format!("{}{}{}", &before_text[..m.range.start], replacement, &before_text[m.range.end..]);
    assert_eq!(after_text, expected, "{ctx}: (i)");
    // (ii) per-character run identity
    let p = doc.paragraph_at(m.para).unwrap();
    let mut owner_after = Vec::new();
    for r in p.runs() {
        if !r.is_removed() {
            owner_after.extend(std::iter::repeat_n(r.index(), r.text().len()));
        }
    }
    let mut owner_before = Vec::new();
    for (i, rg, ..) in &runs {
        owner_before.extend(std::iter::repeat_n(*i, rg.len()));
    }
    let shift = replacement.len() as isize - m.range.len() as isize;
    for (b, &o) in owner_after.iter().enumerate() {
        let want = if b < m.range.start {
            owner_before[b]
        } else if b < m.range.start + replacement.len() {
            r0
        } else {
            owner_before[(b as isize - shift) as usize]
        };
        assert_eq!(o, want, "{ctx}: (ii) byte {b}");
    }
    // (iii) locality: only this part, only inside r₀ … rₖ
    let after_part = String::from_utf8(doc.package().read_part(&part).unwrap()).unwrap();
    let first = before_runs[held_ords[0]].range().start;
    let last_end = before_runs[*held_ords.last().unwrap()].range().end;
    if let Some((s, e, _)) = diff_range(before_part.as_bytes(), after_part.as_bytes()) {
        assert!(s >= first && e <= last_end, "{ctx}: (iii) diff {s}..{e} outside {first}..{last_end}");
    }
    // (iv) byte identity outside the match, order and position
    let after_doc = roxmltree::Document::parse(&after_part).unwrap();
    let after_runs = runs_of(&after_doc);
    for (h, &o) in held.iter().zip(&held_ords) {
        let (rb, ra) = (before_runs[o], after_runs[o]);
        let open = &before_part[rb.range().start..rb.first_child().expect("held runs have children").range().start];
        assert_eq!(&after_part[ra.range().start..ra.range().start + open.len()], open, "{ctx}: (iv) start tag");
        let (rpr_b, cb) = children(&before_part, rb, h.1.start);
        let (rpr_a, ca) = children(&after_part, ra, 0);
        assert_eq!(rpr_a, rpr_b, "{ctx}: (iv) rPr");
        let affected = |c: &ChildBytes| {
            if c.range.is_empty() {
                m.range.start < c.range.start && c.range.start < m.range.end
            } else {
                c.range.start < m.range.end && m.range.start < c.range.end
            }
        };
        let lo = cb.iter().position(affected).unwrap_or_else(|| panic!("{ctx}: a held run has affected children"));
        let hi = cb.iter().rposition(affected).unwrap() + 1;
        assert!(cb[lo..hi].iter().all(affected), "{ctx}: affected children are contiguous");
        let (l, r) = (&cb[..lo], &cb[hi..]);
        assert!(ca.len() >= l.len() + r.len(), "{ctx}: (iv) length");
        let bytes = |v: &[ChildBytes]| v.iter().map(|c| c.bytes.clone()).collect::<Vec<_>>();
        assert_eq!(bytes(&ca[..l.len()]), bytes(l), "{ctx}: (iv) children before the affected ones");
        assert_eq!(bytes(&ca[ca.len() - r.len()..]), bytes(r), "{ctx}: (iv) children after the affected ones");
        let mid = &ca[l.len()..ca.len() - r.len()];
        assert!(mid.iter().all(|c| matches!(c.name.as_str(), "t" | "tab" | "br")), "{ctx}: (iv) only written pieces in the middle");
        let breaks = mid.iter().filter(|c| c.name != "t").count();
        let want = if h.0 == r0 { replacement.chars().filter(|c| matches!(c, '\t' | '\n')).count() } else { 0 };
        assert_eq!(breaks, want, "{ctx}: (n1) no w:tab/w:br outside the replacement");
    }
}
