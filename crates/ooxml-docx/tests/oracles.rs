//! ADR-0001/0004 oracles on the fixtures: T1 (prolog/epilog, zero-edit
//! identity), T1b (parser/renderer), T2 (locality), T3 (Layer 1
//! passthrough), T6 (schema order of edited parents).

mod common;

use common::*;
use ooxml_core::{Content, ElementSpec, RawItem, RawNode, XmlDocument};
use ooxml_docx::Document;
use ooxml_schema::wml;

fn xml_parts(bytes: &[u8]) -> Vec<String> {
    part_names(bytes).into_iter().filter(|n| n.ends_with(".xml") || n.ends_with(".rels")).collect()
}

/// T1 + T1b on every XML part of every openable fixture (a superset of the
/// parts ooxml-docx parses: main, settings, headers, footers, notes,
/// comments).
#[test]
fn t1_t1b_every_xml_part() {
    let mut n = 0;
    for stem in openable() {
        let bytes = fixture(stem);
        for name in xml_parts(&bytes) {
            let data = part(&bytes, &name);
            let mut doc = XmlDocument::parse(&data).unwrap_or_else(|e| panic!("{stem}/{name}: {e}"));
            assert_eq!(doc.serialize().as_bytes(), &data[..], "T1 {stem}/{name}");
            let before = doc.root().clone();
            doc.force_rerender();
            let again = XmlDocument::parse(doc.serialize().as_bytes()).unwrap_or_else(|e| panic!("T1b {stem}/{name}: {e}"));
            assert!(before.structurally_eq(again.root()), "T1b {stem}/{name}");
            n += 1;
        }
    }
    assert!(n > 300, "{n} parts");
}

/// T3: opening (which parses main, settings and scans stories) and saving
/// without edits copies every entry; after an edit only the main part
/// differs.
#[test]
fn t3_passthrough() {
    for stem in openable() {
        let input = fixture(stem);
        let doc = Document::from_bytes(input.clone()).unwrap();
        assert_eq!(entries(&doc.to_bytes().unwrap()), entries(&input), "{stem}");
    }
    for stem in writable() {
        let input = fixture(stem);
        let mut doc = Document::from_bytes(input.clone()).unwrap();
        let main = doc.main_part_name().to_owned();
        if doc.append_paragraph("T3").is_err() {
            continue; // sdk-HelloO14 (A1)
        }
        let (a, b) = (entries(&input), entries(&doc.to_bytes().unwrap()));
        assert_eq!(a.len(), b.len());
        for (x, y) in a.iter().zip(&b) {
            assert_eq!(x.0, y.0, "{stem}: entry order");
            if x.0 != main {
                assert_eq!(x, y, "{stem}: {}", x.0);
            } else {
                assert_eq!(x.1, y.1, "{stem}: main part keeps its compression method");
            }
        }
    }
}

/// T6: the element children of `node` are in the order of `E`'s slots
/// (greedy assignment, ADR-0002 §4.1); unlisted children are ignored.
fn in_slot_order<E: ElementSpec>(node: &RawNode) -> bool {
    let Content::Slots(slots) = E::CONTENT else { return true };
    let mut cur = 0;
    for c in node.elements() {
        let ranks: Vec<usize> = slots.iter().enumerate().filter(|(_, s)| s.names.iter().any(|q| c.is(*q))).map(|(i, _)| i).collect();
        if ranks.is_empty() {
            continue;
        }
        match ranks.into_iter().find(|&r| r >= cur) {
            Some(r) => cur = r,
            None => return false,
        }
    }
    true
}

/// Every new `w:t` that needs it carries `xml:space="preserve"` (docx-rs
/// cannot see this, review m7).
fn preserve_where_needed(run: &RawNode) -> bool {
    run.elements().filter(|c| c.name().local() == "t").all(|t| {
        let s = t.text();
        !(s.starts_with(' ') || s.ends_with(' ') || s.contains("  ")) || t.attrs().iter().any(|a| a.name().local() == "space" && a.value() == "preserve")
    })
}

/// Is the run already in the form Phase0 writes (so rewriting its own text
/// must be byte-identical)?
fn canonical(run: &RawNode) -> bool {
    run.children().iter().all(|c| match c {
        RawItem::Element(e) => matches!(e.name().local(), "rPr" | "t" | "tab") || (e.name().local() == "br" && e.attrs().is_empty()),
        RawItem::Text(t) => t.is_whitespace(),
        RawItem::Markup(_) => false,
    })
}

/// Asserts that `b` differs from `a` only inside `a[lo..hi]`.
fn assert_local(a: &[u8], b: &[u8], lo: usize, hi: usize, what: &str) {
    let tail = a.len() - hi;
    assert!(b.len() >= lo + tail, "{what}");
    assert!(a[..lo] == b[..lo], "{what}: bytes before the span changed");
    assert!(a[hi..] == b[b.len() - tail..], "{what}: bytes after the span changed");
}

/// T2 (locality) for every settable run of every writable fixture: write
/// the run's own text, then a text with edge and double spaces and a tab.
/// Runs are edited in document order in one document; each diff must lie
/// inside that run's original span, shifted by the length change of the
/// earlier edits.
#[test]
fn t2_every_settable_run() {
    let mut total = 0;
    for stem in writable() {
        let input = fixture(stem);
        let fresh = Document::from_bytes(input.clone()).unwrap();
        let main = fresh.main_part_name().to_owned();
        let original = fresh.package().read_part(&main).unwrap();
        let targets: Vec<(usize, usize, usize, usize, String, bool)> = fresh
            .paragraphs()
            .flat_map(|p| {
                p.runs()
                    .filter(|r| r.check_settable().is_ok())
                    .map(|r| {
                        let s = r.raw().span().unwrap();
                        (p.index(), r.index(), s.start, s.end(), r.text(), canonical(r.raw()))
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        let mut doc = Document::from_bytes(input).unwrap();
        let mut prev = original.clone();
        for (p, r, start, end, own, canon) in targets {
            let what = format!("{stem} p{p} r{r}");
            for text in [own.clone(), format!(" {own}\tx  y ")] {
                let delta = prev.len() as isize - original.len() as isize;
                let (lo, hi) = ((start as isize + delta) as usize, (end as isize + delta) as usize);
                doc.set_run_text(p, r, &text).unwrap_or_else(|e| panic!("{what}: {e}"));
                let next = doc.package().read_part(&main).unwrap();
                if text == own && canon {
                    assert!(next == prev, "{what}: own text must be a no-op");
                }
                assert_local(&prev, &next, lo, hi, &what);
                let run = doc.paragraph(p).unwrap().run(r).unwrap();
                assert_eq!(run.text(), text, "{what}");
                assert!(in_slot_order::<wml::Run>(run.raw()), "T6 {what}");
                // A no-op leaves existing `w:t` as written, including the
                // producers' edge whitespace without `preserve`.
                assert!(next == prev || preserve_where_needed(run.raw()), "{what}");
                prev = next;
            }
            total += 1;
        }
    }
    // 3,418 settable runs with a `w:t` (as in `fixtures.rs`) plus 5 settable
    // runs holding only tabs or breaks.
    assert_eq!(total, 3423);
}

/// T2 for append: a pure insertion immediately before the body-level
/// `<w:sectPr`, or before `</w:body>` when there is none (`poi-55733`).
#[test]
fn t2_append_is_a_pure_insertion() {
    let mut n = 0;
    for stem in writable().filter(|s| *s != "sdk-HelloO14") {
        let mut doc = Document::from_bytes(fixture(stem)).unwrap();
        let main = doc.main_part_name().to_owned();
        let before = doc.package().read_part(&main).unwrap();
        let at = {
            let xml = XmlDocument::parse(&before).unwrap();
            let body = xml.root().elements().find(|e| e.name().local() == "body").unwrap();
            match body.elements().filter(|e| e.name().local() == "sectPr").last() {
                Some(s) => s.span().unwrap().start,
                None => body.span().unwrap().close.unwrap().0,
            }
        };
        let i = doc.append_paragraph("Appended  paragraph\twith tab").unwrap();
        let after = doc.package().read_part(&main).unwrap();
        let inserted = br#"<w:p><w:r><w:t xml:space="preserve">Appended  paragraph</w:t><w:tab/><w:t>with tab</w:t></w:r></w:p>"#;
        assert_eq!(after.len(), before.len() + inserted.len(), "{stem}");
        assert!(after[..at] == before[..at] && after[at..at + inserted.len()] == inserted[..] && after[at + inserted.len()..] == before[at..], "{stem}");
        assert_eq!(doc.paragraph(i).unwrap().text(), "Appended  paragraph\twith tab");
        assert_eq!(i, doc.paragraph_count() - 1);
        let xml = XmlDocument::parse(&after).unwrap();
        let body = xml.root().elements().find(|e| e.name().local() == "body").unwrap();
        assert!(in_slot_order::<wml::Body>(body), "T6 {stem}");
        n += 1;
    }
    assert_eq!(n, 18);
}
