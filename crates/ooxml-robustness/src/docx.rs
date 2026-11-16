//! `ooxml-docx`: open → gates → read every paragraph/run → `set_run_text` /
//! `append_paragraph` → save → reopen.

use std::collections::BTreeMap;

use ooxml_core::XmlDocument;
use ooxml_docx::Document;

use crate::fixtures::{DOCX, decode};
use crate::input::Input;
use crate::zipcmp::assert_l1_except;

const W_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

/// `\r\n` and `\r` are written as `w:br`, which reads back as `\n`.
fn normalize(s: &str) -> String {
    s.replace("\r\n", "\n").replace('\r', "\n")
}

/// The main part's current bytes.
fn main_bytes(doc: &Document) -> Vec<u8> {
    doc.package().read_part(doc.main_part_name()).expect("main part readable")
}

/// Asserts that `after` equals `before` outside `[lo, hi)` of `before`:
/// the common prefix reaches `lo` and the common suffix reaches `hi`.
fn assert_local(before: &[u8], after: &[u8], lo: usize, hi: usize, what: &str) {
    let prefix = before.iter().zip(after).take_while(|(a, b)| a == b).count();
    let suffix = before.iter().rev().zip(after.iter().rev()).take_while(|(a, b)| a == b).count();
    assert!(
        prefix >= lo && suffix >= before.len() - hi,
        "{what}: document.xml changed outside bytes {lo}..{hi} (common prefix {prefix}, common suffix {suffix}, length {} -> {})",
        before.len(),
        after.len()
    );
}

/// Asserts that `after` is `before` with bytes inserted at one place.
fn assert_insertion(before: &[u8], after: &[u8], what: &str) {
    let prefix = before.iter().zip(after).take_while(|(a, b)| a == b).count();
    let suffix = before.iter().rev().zip(after.iter().rev()).take_while(|(a, b)| a == b).count();
    assert!(
        after.len() >= before.len() && prefix + suffix >= before.len(),
        "{what}: document.xml is not the input with one insertion (common prefix {prefix}, suffix {suffix}, length {} -> {})",
        before.len(),
        after.len()
    );
}

/// Reads everything; returns `(paragraph, run)` of every run that
/// `check_settable` accepts.
fn read_all(doc: &Document) -> Vec<(usize, usize)> {
    let _ = (doc.write_gate(), doc.write_gates().len(), doc.update_fields_on_open(), doc.write_protection(), doc.is_marked_final());
    let mut settable = Vec::new();
    assert_eq!(doc.paragraphs().count(), doc.paragraph_count());
    for p in doc.paragraphs() {
        let _ = (p.text(), p.raw().name());
        assert_eq!(p.runs().count(), p.run_count());
        for r in p.runs() {
            let _ = (r.text(), r.context(), r.in_field(), r.raw().span());
            if r.check_settable().is_ok() {
                settable.push((p.index(), r.index()));
            }
        }
    }
    settable
}

fn body_span(main: &[u8]) -> Option<ooxml_core::ElementSpan> {
    let doc = XmlDocument::parse(main).ok()?;
    let body = doc.root().elements().find(|e| e.name().ns() == W_NS && e.name().local() == "body")?;
    body.span()
}

/// Fuzz entry: a fixture-based input (see [`crate::fixtures`]).
pub fn check_docx_input(data: &[u8]) {
    if let Some((zip, ops)) = decode(&DOCX, data) {
        check_docx(&zip, ops);
    }
}

/// Checks on a document (if it opens):
///
/// - zero-edit save is Layer-1 identical to the input; reads never panic;
///   `Paragraph::runs`/`run_count` agree;
/// - every refused edit leaves the package byte-identical;
/// - a successful `set_run_text` reads back (`\r\n`/`\r` as `\n`) and
///   changes `document.xml` only inside the run's original span (T2);
/// - a successful `append_paragraph` returns the last paragraph index,
///   reads back, and only inserts bytes (inside `w:body`'s span when the
///   body was self-closing);
/// - after the edits only the main part differs (Layer 1), the output
///   reopens and every edit reads back again.
pub fn check_docx(zip: &[u8], mut ops: Input<'_>) {
    let Ok(mut doc) = Document::from_bytes(zip.to_vec()) else { return };
    let Ok(base) = doc.to_bytes() else { return };
    assert_l1_except(zip, &base, &[], "docx zero-edit save");
    let main = doc.main_part_name().to_owned();
    let mut settable = read_all(&doc);
    assert_l1_except(zip, &doc.to_bytes().expect("saved before"), &[], "docx save after reading");

    let mut runs: BTreeMap<(usize, usize), String> = BTreeMap::new();
    let mut appended: BTreeMap<usize, String> = BTreeMap::new();
    let mut steps = 0;
    // Every successful edit reopens the document (about 35 ms for the
    // largest fixture's 1.35 MB document.xml in release, ten times that under
    // ASan), so large main parts get fewer steps to stay under -timeout.
    let max_steps = if main_bytes(&doc).len() > 512 * 1024 { 4 } else { 12 };
    while !ops.is_empty() && steps < max_steps {
        steps += 1;
        let kind = ops.u8() % 4;
        let text = ops.string();
        let pick = ops.u16();
        let before = doc.to_bytes().expect("saved before");
        let before_main = main_bytes(&doc);
        let (p, r) = if kind < 2 && !settable.is_empty() {
            settable[usize::from(pick) % settable.len()]
        } else {
            let count = doc.paragraph_count().max(1);
            let p = usize::from(pick) % (count + 1);
            (p, usize::from(ops.u8() % 8))
        };
        let result = match kind {
            0..=2 => {
                let span: Option<(usize, usize)> =
                    doc.paragraph(p).and_then(|x| x.run(r)).and_then(|x| x.raw().span()).map(|s| (s.start, s.end()));
                doc.set_run_text(p, r, &text).map(|()| {
                    assert_eq!(doc.paragraph(p).and_then(|x| x.run(r)).map(|x| x.text()), Some(normalize(&text)), "set_run_text({p}, {r}) reads back");
                    if let Some((lo, hi)) = span {
                        assert_local(&before_main, &main_bytes(&doc), lo, hi, "set_run_text");
                    }
                    runs.insert((p, r), normalize(&text));
                    appended.remove(&p); // its text is now checked through the run
                })
            }
            _ => {
                let count = doc.paragraph_count();
                let body = body_span(&before_main);
                doc.append_paragraph(&text).map(|i| {
                    assert_eq!(i, count, "append_paragraph returns the new last index");
                    assert_eq!(doc.paragraph_count(), count + 1);
                    assert_eq!(doc.paragraph(i).map(|x| x.text()), Some(normalize(&text)), "appended paragraph reads back");
                    let after = main_bytes(&doc);
                    match body {
                        Some(b) if b.is_self_closing() => assert_local(&before_main, &after, b.start, b.end(), "append_paragraph"),
                        _ => assert_insertion(&before_main, &after, "append_paragraph"),
                    }
                    appended.insert(i, normalize(&text));
                })
            }
        };
        match result {
            Err(e) => {
                let after = doc.to_bytes().expect("to_bytes after a refused edit");
                assert!(after == before, "refused docx edit kind {kind} ({e}) changed the package");
            }
            Ok(()) => {
                // Reopen after every edit: the spans the next edit's
                // locality check uses are then those of the current bytes,
                // and every intermediate output is checked to reopen.
                let bytes = doc.to_bytes().unwrap_or_else(|e| panic!("save after an edit failed: {e}"));
                doc = Document::from_bytes(bytes).unwrap_or_else(|e| panic!("edited document does not reopen: {e}"));
                settable = read_all(&doc);
            }
        }
    }
    let out = doc.to_bytes().unwrap_or_else(|e| panic!("save after edits failed: {e}"));
    assert_l1_except(&base, &out, &[&main], "docx save after edits");
    let re = Document::from_bytes(out).unwrap_or_else(|e| panic!("edited document does not reopen: {e}"));
    let _ = read_all(&re);
    for ((p, r), t) in &runs {
        assert_eq!(re.paragraph(*p).and_then(|x| x.run(*r)).map(|x| x.text()).as_ref(), Some(t), "run ({p}, {r}) after reopen");
    }
    for (i, t) in &appended {
        assert_eq!(re.paragraph(*i).map(|x| x.text()).as_ref(), Some(t), "appended paragraph {i} after reopen");
    }
}
