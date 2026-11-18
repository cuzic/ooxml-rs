//! `ooxml-docx`: open → gates → read every paragraph/run → `set_run_text` /
//! `append_paragraph` → save → reopen.

use std::collections::BTreeMap;

use ooxml_core::{ElementSpan, Error as CoreError, QName, RawNode, XmlDocument};
use ooxml_docx::{Document, Error, Operation};

use crate::fixtures::{DOCX, decode};
use crate::input::Input;
use crate::zipcmp::assert_l1_except;

const R_ID: QName = QName::new("http://schemas.openxmlformats.org/officeDocument/2006/relationships", "id");
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
    let mut depth = 0usize;
    for p in doc.paragraphs() {
        let _ = (p.text(), p.raw().name());
        assert_eq!(p.runs().count(), p.run_count());
        for r in p.runs() {
            let before = depth;
            let mut has = false;
            for c in r.raw().elements().filter(|c| is_w(c, "fldChar")) {
                has = true;
                match c.attrs().iter().find(|a| a.name().ns() == W_NS && a.name().local() == "fldCharType").map(|a| a.value()) {
                    Some("begin") => depth += 1,
                    Some("end") => depth = depth.saturating_sub(1),
                    _ => {}
                }
            }
            assert_eq!(r.in_field(), before > 0 || depth > 0 || has, "run ({}, {}): in_field disagrees with the fldChar recount", p.index(), r.index());
            let _ = (r.text(), r.context(), r.in_field(), r.raw().span());
            if r.check_settable().is_ok() {
                settable.push((p.index(), r.index()));
            }
        }
    }
    settable
}

fn is_w(n: &RawNode, local: &str) -> bool {
    n.name().ns() == W_NS && n.name().local() == local
}

/// `w:body`'s span and the span of its own (body-level) `w:sectPr`.
fn body_spans(doc: &XmlDocument) -> Option<(ElementSpan, Option<ElementSpan>)> {
    let body = doc.root().elements().find(|e| is_w(e, "body"))?;
    Some((body.span()?, body.elements().find(|e| is_w(e, "sectPr")).and_then(RawNode::span)))
}

/// The first character `set_run_text` / `append_paragraph` must refuse
/// (ADR-0004 §3: control characters other than tab/LF/CR, U+FFFE, U+FFFF),
/// derived here independently of ooxml-docx.
fn invalid_char(text: &str) -> Option<char> {
    text.chars().find(|&c| (c < ' ' && !matches!(c, '\t' | '\n' | '\r')) || c == '\u{FFFE}' || c == '\u{FFFF}')
}

/// Predicts `set_run_text(p, r, text)` using the public API.
/// In `crates/ooxml-docx/src/document.rs`, `Document::set_run_text` checks
/// the paragraph before delegating to `set_run_text_at`, which checks the
/// text gate, run index, `check_settable`, and text characters in that order.
fn predict_set(doc: &Document, p: usize, r: usize, text: &str) -> Result<(), String> {
    let para = doc.paragraph(p).ok_or_else(|| format!("{:?}", Error::NoSuchParagraph(p)))?;
    doc.write_gate_for(Operation::Text).map_err(|e| format!("{e:?}"))?;
    let run = para.run(r).ok_or_else(|| format!("{:?}", Error::NoSuchRun { paragraph: p, run: r }))?;
    run.check_settable().map_err(|e| format!("{e:?}"))?;
    invalid_char(text).map_or(Ok(()), |ch| Err(format!("{:?}", Error::InvalidXmlChar { ch })))
}

fn paragraph_properties(p: &RawNode) -> Option<&RawNode> {
    p.elements().find(|e| is_w(e, "pPr"))
}

fn section_properties(ppr: &RawNode) -> Option<&RawNode> {
    ppr.elements().find(|e| is_w(e, "sectPr"))
}

/// Predicts the append gates and whether Phase2 must move the A2 section.
fn predict_append<'a>(doc: &'a Document, parsed: &XmlDocument, text: &str) -> Result<Option<&'a RawNode>, String> {
    doc.write_gate_for(Operation::Append).map_err(|e| format!("{e:?}"))?;
    if let Some(ch) = invalid_char(text) {
        return Err(format!("{:?}", Error::InvalidXmlChar { ch }));
    }
    let Some(body) = parsed.root().elements().find(|e| is_w(e, "body")) else {
        return Err(format!("{:?}", Error::NoBody));
    };
    let mut depth = 0usize;
    for p in doc.paragraphs() {
        for r in p.runs() {
            for c in r.raw().elements().filter(|c| is_w(c, "fldChar")) {
                match c.attrs().iter().find(|a| a.name().ns() == W_NS && a.name().local() == "fldCharType").map(|a| a.value()) {
                    Some("begin") => depth += 1,
                    Some("end") => depth = depth.saturating_sub(1),
                    _ => {}
                }
            }
        }
    }
    if depth > 0 {
        return Err(format!("{:?}", Error::InsideField));
    }
    if body.elements().any(|e| is_w(e, "sectPr")) {
        return Ok(None);
    }
    let Some(last) = doc.paragraph(doc.paragraph_count().wrapping_sub(1)).map(|p| p.raw()) else { return Ok(None) };
    let Some(ppr) = paragraph_properties(last) else { return Ok(None) };
    let Some(section) = section_properties(ppr) else { return Ok(None) };
    if !body.elements().any(|e| is_w(e, "p") && e.span() == last.span()) {
        return Err(format!("{:?}", Error::SectionBreakAtEnd));
    }
    for (node, local, element) in [(ppr, "pPrChange", "w:pPrChange"), (section, "sectPrChange", "w:sectPrChange")] {
        if node.elements().any(|e| is_w(e, local)) {
            return Err(format!("{:?}", Error::TrackedPropertyChange { element }));
        }
    }
    let rels = doc.package().relationships(doc.main_part_name()).map_err(|e| format!("{:?}", Error::Core(e)))?;
    for reference in section.elements().filter(|e| is_w(e, "headerReference") || is_w(e, "footerReference")) {
        let id = reference.attr(R_ID).unwrap_or_default();
        if rels.by_id(id).is_none() {
            return Err(format!("{:?}", Error::RelIdReferenced { id: id.into() }));
        }
    }
    Ok(Some(section))
}

// The move may reserialize prefixes, and reference children are written
// again in schema order. Compare property names and values independently.
fn retains_section_properties(old: &RawNode, new: &RawNode) -> bool {
    old.name().ns() == new.name().ns() && old.name().local() == new.name().local()
        && old.attrs().iter().filter(|a| !a.name().is_namespace_declaration()).all(|a| {
            new.attrs().iter().any(|b| {
                a.name().ns() == b.name().ns() && a.name().local() == b.name().local() && a.value() == b.value()
            })
        })
        && old.elements().count() == new.elements().count()
        && old.elements().all(|a| new.elements().any(|b| retains_section_properties(a, b)))
}

/// An A2 append only removes the old section and inserts the new paragraph.
fn check_section_move(before: &[u8], after: &[u8], section: &RawNode, anchor: ElementSpan) {
    let parsed = XmlDocument::parse(after).expect("A2 append produced valid XML");
    let body = parsed.root().elements().find(|e| is_w(e, "body")).expect("A2 append retained w:body");
    let [previous, added] = body.elements().filter(|e| is_w(e, "p")).collect::<Vec<_>>()
        .last_chunk::<2>().copied().expect("A2 append has two body paragraphs");
    assert!(paragraph_properties(previous).and_then(section_properties).is_none(), "old paragraph still has w:sectPr");
    let moved = paragraph_properties(added).and_then(section_properties).expect("new paragraph has w:sectPr");
    assert!(retains_section_properties(section, moved), "moved section changed properties");
    let old = section.span().expect("original section span");
    let new = added.span().expect("appended paragraph span");
    assert!(new.start >= anchor.end() - (old.end() - old.start), "A2 append inserted before its anchor");
    assert!(
        before[..old.start].iter().chain(&before[old.end()..]).eq(after[..new.start].iter().chain(&after[new.end()..])),
        "A2 append changed bytes outside the old section and the new paragraph"
    );
}

/// After a successful `set_run_text`, the run kept its attributes and its
/// `w:rPr`, and every `w:t` whose text has edge or double spaces carries
/// `xml:space="preserve"` (ADR-0004 §3).
fn check_run_kept(old: &RawNode, new: &RawNode, what: &str) {
    assert_eq!(old.attrs(), new.attrs(), "{what}: run attributes changed");
    let rpr = |n: &RawNode| n.elements().find(|e| is_w(e, "rPr")).cloned();
    match (rpr(old), rpr(new)) {
        (None, None) => {}
        (Some(a), Some(b)) => assert!(a.structurally_eq(&b), "{what}: w:rPr changed"),
        (a, b) => panic!("{what}: w:rPr {} -> {}", a.is_some(), b.is_some()),
    }
    for t in new.elements().filter(|e| is_w(e, "t")) {
        let v = t.text();
        if v.starts_with(' ') || v.ends_with(' ') || v.contains("  ") {
            assert_eq!(t.xml_space_preserve(), Some(true), "{what}: w:t {v:?} lacks xml:space=\"preserve\"");
        }
    }
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
///   body was self-closing), or moves the A2 section to the new paragraph;
/// - after the edits only the main part differs (Layer 1), the output
///   reopens and every edit reads back again.
pub fn check_docx(zip: &[u8], mut ops: Input<'_>) {
    let Ok(mut doc) = Document::from_bytes(zip.to_vec()) else { return };
    let Ok(base) = doc.to_bytes() else { return };
    assert_l1_except(zip, &base, &[], "docx zero-edit save");
    let main = doc.main_part_name().to_owned();
    let mut settable = read_all(&doc);
    // Large main parts (the 1.35 MB fixture costs about 6 s under ASan
    // before any edit) skip the second save and get one edit, so a seed
    // stays under libFuzzer's -timeout=10 on a loaded CI runner (review of
    // PR #47 round 2, m5).
    let large = main_bytes(&doc).len() > 512 * 1024;
    if !large {
        assert_l1_except(zip, &doc.to_bytes().expect("saved before"), &[], "docx save after reading");
    }

    let mut runs: BTreeMap<(usize, usize), String> = BTreeMap::new();
    let mut appended: BTreeMap<usize, String> = BTreeMap::new();
    let mut steps = 0;
    // Every successful edit reopens the document, and an append also parses
    // the main part for its prediction (together about 0.1 s per step for
    // the largest fixture's 1.35 MB document.xml in release, more than ten
    // times that under ASan), so large main parts get fewer steps to stay
    // under libFuzzer's -timeout=10.
    let max_steps = if large { 1 } else { 12 };
    while !ops.is_empty() && steps < max_steps {
        steps += 1;
        let kind = ops.u8() % 4;
        // Half the texts use only characters Phase0 can write, so the
        // success paths get as many tries as the refusals (review m3).
        let text = if ops.u8().is_multiple_of(2) { ops.string() } else { ops.printable() };
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
        let mut kept: Option<RawNode> = None;
        let mut body_has_ac = false;
        let mut moves_section = false;
        let (predicted, result) = match kind {
            0..=2 => {
                let predicted = predict_set(&doc, p, r, &text);
                kept = doc.paragraph(p).and_then(|x| x.run(r)).map(|x| x.raw().clone());
                let span: Option<(usize, usize)> =
                    doc.paragraph(p).and_then(|x| x.run(r)).and_then(|x| x.raw().span()).map(|s| (s.start, s.end()));
                (predicted, doc.set_run_text(p, r, &text).map(|()| {
                    assert_eq!(doc.paragraph(p).and_then(|x| x.run(r)).map(|x| x.text()), Some(normalize(&text)), "set_run_text({p}, {r}) reads back");
                    if let Some((lo, hi)) = span {
                        assert_local(&before_main, &main_bytes(&doc), lo, hi, "set_run_text");
                    }
                    runs.insert((p, r), normalize(&text));
                    appended.remove(&p); // its text is now checked through the run
                }))
            }
            _ => {
                let count = doc.paragraph_count();
                // One parse of the main part serves the prediction and the
                // position check (the largest fixture's is 1.35 MB).
                let parsed = XmlDocument::parse(&before_main).expect("the main part parsed at open");
                body_has_ac = parsed
                    .root()
                    .elements()
                    .find(|e| is_w(e, "body"))
                    .is_some_and(|b| b.elements().any(|e| e.name().ns() == ooxml_core::name::MC_NS && e.name().local() == "AlternateContent"));
                let predicted = predict_append(&doc, &parsed, &text);
                let section = predicted.as_ref().ok().and_then(|e| *e).cloned();
                moves_section = section.is_some();
                let anchor = doc.paragraph(count.wrapping_sub(1)).and_then(|p| p.raw().span());
                let spans = body_spans(&parsed);
                (predicted.map(|_| ()), doc.append_paragraph(&text).map(|i| {
                    assert_eq!(i, count, "append_paragraph returns the new last index");
                    assert_eq!(doc.paragraph_count(), count + 1);
                    assert_eq!(doc.paragraph(i).map(|x| x.text()), Some(normalize(&text)), "appended paragraph reads back");
                    let after = main_bytes(&doc);
                    match spans {
                        _ if let Some(section) = &section => {
                            check_section_move(&before_main, &after, section, anchor.expect("A2 anchor span"));
                        }
                        Some((b, _)) if b.is_self_closing() => assert_local(&before_main, &after, b.start, b.end(), "append_paragraph"),
                        Some((b, sect)) => {
                            assert_insertion(&before_main, &after, "append_paragraph");
                            // Where the bytes went: some insertion point consistent with
                            // the common prefix/suffix lies inside w:body, before the
                            // body-level w:sectPr (review m2).
                            let prefix = before_main.iter().zip(&after).take_while(|(x, y)| x == y).count();
                            let suffix = before_main.iter().rev().zip(after.iter().rev()).take_while(|(x, y)| x == y).count();
                            let (lo, hi) = (before_main.len() - suffix.min(before_main.len()), prefix.min(before_main.len()));
                            let limit = sect.map_or_else(|| b.close.map_or(b.end(), |(c, _)| c), |s| s.start);
                            assert!(
                                lo.max(b.open_end) <= hi.min(limit),
                                "append_paragraph: inserted outside w:body or after its w:sectPr (insertion in {lo}..={hi}, allowed {}..={limit})",
                                b.open_end
                            );
                        }
                        None => panic!("append_paragraph succeeded without a w:body"),
                    }
                    appended.insert(i, normalize(&text));
                }))
            }
        };
        // Refusal oracle (review M1): the outcome is the predicted one. An
        // insertion the lens itself refuses is accepted for A1. An A2
        // move can additionally fail the lens's detach or destination checks.
        let actual = result.as_ref().map(|_| ()).map_err(|e| format!("{e:?}"));
        let lens_refusal = matches!(&result, Err(Error::Core(ooxml_core::Error::UnresolvedSibling(_))));
        let move_refusal = moves_section && matches!(&result, Err(Error::Core(
            CoreError::NotDetachable { .. } | CoreError::NamespaceNotInScope(_) | CoreError::TooDeep { .. }
        )));
        assert!(
            actual == predicted || (predicted.is_ok() && lens_refusal && kind == 3 && body_has_ac) || move_refusal,
            "docx edit kind {kind} ({p}, {r}, {text:?}): predicted {predicted:?}, got {actual:?}"
        );
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
                if let Some(old) = &kept {
                    let new = doc.paragraph(p).and_then(|x| x.run(r)).expect("edited run after reopen");
                    check_run_kept(old, new.raw(), "set_run_text");
                }
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

/// Bodies that reach the refusals and duties random mutation rarely hits:
/// A2 (last paragraph ends the last section), a field left open at the
/// end of the body (with and without a `separate`), and a run whose text
/// needs `xml:space="preserve"` but lacks it (rewriting the same text must
/// add it).
const TARGETED_BODIES: &[&str] = &[
    r#"<w:body><w:p><w:pPr><w:sectPr/></w:pPr><w:r><w:t>x</w:t></w:r></w:p></w:body>"#,
    r#"<w:body><w:p><w:r><w:t>x</w:t></w:r></w:p><w:p><w:pPr><w:sectPr/></w:pPr></w:p></w:body>"#,
    r#"<w:body><w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText> PAGE </w:instrText></w:r></w:p></w:body>"#,
    r#"<w:body><w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText> PAGE </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>1</w:t></w:r></w:p></w:body>"#,
    r#"<w:body><w:p><w:r><w:t>a  b</w:t></w:r></w:p></w:body>"#,
    r#"<w:body><w:p><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve"> a </w:t></w:r></w:p><w:sectPr/></w:body>"#,
    r#"<w:body/>"#,
];

/// Fuzz inputs (the [`crate::fixtures`] layout) built from
/// [`TARGETED_BODIES`] in a small fixture's `word/document.xml`, each with
/// an `append_paragraph("a")` and a `set_run_text` of `"a  b"` on the
/// first settable run. Used as seeds and by the smoke test.
pub fn targeted_inputs() -> Vec<Vec<u8>> {
    let Some((fi, fx)) = DOCX.iter().enumerate().find(|(_, f)| f.name.contains("55733")) else { return Vec::new() };
    let Some(pi) = fx.parts.iter().position(|(n, _)| n == "word/document.xml") else { return Vec::new() };
    let src = String::from_utf8_lossy(&fx.parts[pi].1).into_owned();
    let (Some(a), Some(b)) = (src.find("<w:body"), src.find("</w:body>")) else { return Vec::new() };
    // kind 3 (append), printable "a", pick 0, run byte (always read for
    // kind 3); then kind 0 (the first settable run, if any), printable
    // "a  b", pick 0 (a run byte follows only when nothing is settable).
    let ops: &[u8] = &[3, 1, 1, 0, 0, 0, 0, 0, 1, 4, 0, 4, 4, 1, 0, 0, 0];
    TARGETED_BODIES
        .iter()
        .map(|body| {
            let mut input = vec![fi as u8, pi as u8, ops.len() as u8];
            input.extend_from_slice(ops);
            input.extend_from_slice(format!("{}{body}{}", &src[..a], &src[b + "</w:body>".len()..]).as_bytes());
            input
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_paragraph_precedes_document_protection() {
        let zip = std::fs::read(crate::fixtures::fixtures_dir().join("docx/poi-documentProtection_readonly_no_password.docx"))
            .expect("read the protected fixture");
        let doc = Document::from_bytes(zip.clone()).expect("open the protected fixture");
        std::assert_matches!(doc.write_gate_for(Operation::Text), Err(Error::DocumentProtected { .. }));
        let count = doc.paragraph_count();
        assert!(count > 0);
        let [lo, hi] = u16::try_from(count).expect("paragraph index fits in the edit input").to_le_bytes();

        // Kind 2 selects a paragraph by index. Try the first missing paragraph
        // and then paragraph 0, so the oracle checks both errors and both saves.
        let ops = [2, 1, 0, lo, hi, 0, 2, 1, 0, 0, 0, 0];
        check_docx(&zip, Input::new(&ops));
    }
}
