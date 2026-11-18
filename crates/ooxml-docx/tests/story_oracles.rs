//! ADR-0007 §9 for the stories other than the main one (review of PR #52,
//! m1): T2 (locality) for every settable run of every header, footer,
//! footnote, endnote and comment of every writable fixture, T3 (only the
//! edited story parts change), T5 (docx-rs reads headers and footers of the
//! last section and comments back) and T5b (an independent `roxmltree`
//! reading of every edited story part).

mod common;

use std::collections::BTreeSet;

use common::*;
use ooxml_core::RawNode;
use ooxml_docx::{Document, RunRef, StoryKind};

/// Ordinal of `run` among the part's `w:r` elements (document order).
fn ordinal(root: &RawNode, run: &RawNode) -> usize {
    let mut stack = vec![root];
    let mut k = 0;
    while let Some(n) = stack.pop() {
        if n.name().ns() == W_NS && n.name().local() == "r" {
            if std::ptr::eq(n, run) {
                return k;
            }
            k += 1;
        }
        let kids: Vec<&RawNode> = n.elements().collect();
        stack.extend(kids.into_iter().rev());
    }
    panic!("run not in its part")
}

/// Byte range of the `k`-th `w:r` of `xml`, read by roxmltree.
fn run_range(xml: &str, k: usize) -> std::ops::Range<usize> {
    let doc = roxmltree::Document::parse(xml).unwrap();
    doc.descendants().filter(|n| n.is_element() && n.tag_name().namespace() == Some(W_NS) && n.tag_name().name() == "r").nth(k).unwrap().range()
}

/// T5b: every paragraph's `w:t`/`w:tab`/`w:br` text, read by roxmltree.
fn roxml_texts(xml: &str) -> Vec<String> {
    let doc = roxmltree::Document::parse(xml).unwrap();
    doc.descendants()
        .filter(|n| n.is_element() && n.tag_name().namespace() == Some(W_NS) && n.tag_name().name() == "p")
        .map(|p| {
            p.descendants()
                .filter(|n| n.is_element() && n.tag_name().namespace() == Some(W_NS))
                .filter_map(|n| match n.tag_name().name() {
                    "t" => n.text().map(str::to_owned),
                    "tab" => Some("\t".into()),
                    "br" => Some("\n".into()),
                    _ => None,
                })
                .collect()
        })
        .collect()
}

#[test]
fn t2_t3_t5_t5b_every_story() {
    let mut edits = 0;
    for stem in writable() {
        let input = fixture(stem);
        let mut doc = Document::from_bytes(input.clone()).unwrap();
        let targets: Vec<(RunRef, String, StoryKind)> = doc
            .stories()
            .filter(|s| s.id().kind() != StoryKind::Main)
            .flat_map(|s| {
                let (part, kind) = (s.id().part().to_owned(), s.id().kind());
                s.paragraphs()
                    .flat_map(|p| p.runs().filter(|r| r.check_settable().is_ok()).map(|r| r.run_ref()).collect::<Vec<_>>())
                    .map(move |r| (r, part.clone(), kind))
                    .collect::<Vec<_>>()
            })
            .collect();
        let mut edited_parts = BTreeSet::new();
        let mut written: Vec<(String, StoryKind, String)> = Vec::new();
        for (n, (at, part, kind)) in targets.into_iter().enumerate() {
            let before = String::from_utf8(doc.package().read_part(&part).unwrap()).unwrap();
            let story = doc.story_at(at.para().story()).unwrap();
            let k = ordinal(story.raw(), doc.run_at(at).unwrap().raw());
            let text = format!("[EDIT-{n}]");
            doc.set_run_text_at(at, &text).unwrap_or_else(|e| panic!("{stem} {at:?}: {e}"));
            let after = String::from_utf8(doc.package().read_part(&part).unwrap()).unwrap();
            if let Some((s, e, _)) = diff_range(before.as_bytes(), after.as_bytes()) {
                let r = run_range(&before, k);
                assert!(s >= r.start && e <= r.end, "{stem} {part} {at:?}: T2 diff {s}..{e} outside {r:?}");
            }
            assert_eq!(doc.run_at(at).unwrap().text(), text);
            edited_parts.insert(part.clone());
            written.push((part, kind, text));
            edits += 1;
        }
        let out = doc.to_bytes().unwrap();
        // T3: only the edited story parts differ
        for (a, b) in entries(&out).iter().zip(entries(&input)) {
            if a != &b {
                assert!(edited_parts.contains(&a.0), "{stem}: T3 {} changed", a.0);
            }
        }
        // T5b: an independent reading of each edited part shows each text once
        for part_name in &edited_parts {
            let xml = String::from_utf8(part(&out, part_name)).unwrap();
            let texts = roxml_texts(&xml).concat();
            for (p, _, t) in written.iter().filter(|(p, ..)| p == part_name) {
                assert_eq!(texts.matches(t.as_str()).count(), 1, "{stem} {p}: T5b {t}");
            }
        }
        // T5: docx-rs reads comments, and the headers and footers the body-level sectPr names
        let json = docx_rs::read_docx(&out).map(|d| d.json()).unwrap_or_default();
        let re = Document::from_bytes(out).unwrap();
        let last = re.stories().flat_map(|s| s.sections().iter().map(|u| u.section).collect::<Vec<_>>()).max();
        for s in re.stories() {
            let in_last = s.sections().iter().any(|u| Some(u.section) == last);
            let checked = match s.id().kind() {
                StoryKind::Comments => true,
                StoryKind::Header | StoryKind::Footer => in_last,
                _ => false,
            };
            if !checked || json.is_empty() {
                continue;
            }
            for (_, _, t) in written.iter().filter(|(p, ..)| p == s.id().part()) {
                assert!(json.contains(t.as_str()), "{stem} {}: T5 docx-rs misses {t}", s.id().part());
            }
        }
    }
    eprintln!("story oracles: {edits} runs edited");
    assert!(edits >= 30, "{edits}"); // 37 in the fixtures
}
