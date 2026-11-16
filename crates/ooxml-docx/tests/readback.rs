//! T5: independent read-back with docx-rs 0.4.22 (ADR-0004 §7, with the
//! review round 1 m7 corrections: only `document.children` is read, so
//! header/footer text in `sectionProperty` cannot be mistaken for body
//! text; `xml:space` is checked structurally in `oracles.rs`, because
//! docx-rs ignores it).

mod common;

use common::*;
use ooxml_docx::Document;
use serde_json::Value;

/// Every `{"type": "text", "data": {"text": …}}` under
/// `document.children`, in document order.
fn texts(bytes: &[u8]) -> Vec<String> {
    let json = docx_rs::read_docx(bytes).expect("docx-rs reads the file").json();
    let v: Value = serde_json::from_str(&json).unwrap();
    let mut out = Vec::new();
    collect(&v["document"]["children"], &mut out);
    out
}

fn collect(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Array(a) => a.iter().for_each(|x| collect(x, out)),
        Value::Object(o) => {
            if o.get("type").and_then(Value::as_str) == Some("text")
                && let Some(t) = o.get("data").and_then(|d| d.get("text")).and_then(Value::as_str)
            {
                out.push(t.to_owned());
                return;
            }
            o.values().for_each(|x| collect(x, out));
        }
        _ => {}
    }
}

/// Up to four settable runs with text, spread over the document.
fn sample(doc: &Document) -> Vec<(usize, usize, Vec<String>)> {
    let all: Vec<(usize, usize, Vec<String>)> = doc
        .paragraphs()
        .flat_map(|p| {
            p.runs()
                .filter(|r| r.check_settable().is_ok())
                .map(|r| {
                    let ts: Vec<String> = r.raw().elements().filter(|c| c.name().local() == "t").map(|c| c.text().into_owned()).collect();
                    (p.index(), r.index(), ts)
                })
                .filter(|(.., ts)| !ts.is_empty())
                .collect::<Vec<_>>()
        })
        .collect();
    if all.is_empty() {
        return all;
    }
    let mut idx = vec![0, all.len() / 4, all.len() / 2, all.len() - 1];
    idx.dedup();
    idx.into_iter().map(|i| all[i].clone()).collect()
}

#[test]
fn t5_set_run_text_reads_back() {
    let mut n = 0;
    for stem in writable() {
        let input = fixture(stem);
        let before = texts(&input);
        for (p, r, pieces) in sample(&Document::from_bytes(input.clone()).unwrap()) {
            let marker = format!("QZXMARK{n}");
            let mut doc = Document::from_bytes(input.clone()).unwrap();
            doc.set_run_text(p, r, &marker).unwrap();
            let after = texts(&doc.to_bytes().unwrap());
            let what = format!("{stem} p{p} r{r}");
            assert_eq!(after.iter().filter(|t| t.contains(&marker)).count(), 1, "{what}");
            let m = after.iter().position(|t| *t == marker).unwrap_or_else(|| panic!("{what}: marker is its own text"));
            let k = pieces.len();
            assert_eq!(before[..m], after[..m], "{what}: text before the run");
            assert_eq!(before[m..m + k], pieces[..], "{what}: the run's original pieces");
            assert_eq!(before[m + k..], after[m + 1..], "{what}: text after the run");
            n += 1;
        }
    }
    assert_eq!(n, 41, "edits");
}

#[test]
fn t5_append_reads_back_as_the_last_body_text() {
    for stem in writable().filter(|s| *s != "sdk-HelloO14") {
        let input = fixture(stem);
        let before = texts(&input);
        let mut doc = Document::from_bytes(input).unwrap();
        doc.append_paragraph("QZX appended").unwrap();
        let after = texts(&doc.to_bytes().unwrap());
        assert_eq!(after.last().map(String::as_str), Some("QZX appended"), "{stem}");
        assert_eq!(before[..], after[..after.len() - 1], "{stem}");
    }
}
