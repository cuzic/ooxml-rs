//! `set_run_text` and `append_paragraph` output, byte for byte (ADR-0004 §3,
//! review round 1 m1).

mod common;

use common::*;
use ooxml_docx::Document;

/// Body inner XML after one edit.
fn edit(body: &str, f: impl FnOnce(&mut Document)) -> String {
    let mut doc = Document::from_bytes(docx(body, None, None)).unwrap();
    f(&mut doc);
    let xml = String::from_utf8(doc.package().read_part("word/document.xml").unwrap()).unwrap();
    let start = xml.find("<w:body>").unwrap() + "<w:body>".len();
    xml[start..xml.rfind("</w:body>").unwrap()].to_owned()
}

fn set(body: &str, text: &str) -> String {
    edit(body, |d| d.set_run_text(0, 0, text).unwrap())
}

#[test]
fn fast_path_keeps_the_t_element() {
    let r = r#"<w:p><w:r w:rsidR="00A1"><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">Old </w:t></w:r></w:p>"#;
    assert_eq!(set(r, "New"), r#"<w:p><w:r w:rsidR="00A1"><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">New</w:t></w:r></w:p>"#, "existing preserve is kept");
    let r = r#"<w:p><w:r><w:t>Old</w:t></w:r></w:p>"#;
    assert_eq!(set(r, " lead"), r#"<w:p><w:r><w:t xml:space="preserve"> lead</w:t></w:r></w:p>"#);
    assert_eq!(set(r, "two  spaces"), r#"<w:p><w:r><w:t xml:space="preserve">two  spaces</w:t></w:r></w:p>"#);
    assert_eq!(set(r, "a < b & c"), r#"<w:p><w:r><w:t>a &lt; b &amp; c</w:t></w:r></w:p>"#);
    assert_eq!(set(r, "Old"), r, "same content: no-op");
}

#[test]
fn slow_path_maps_tabs_and_breaks() {
    let r = r#"<w:p><w:r><w:rPr><w:i/></w:rPr><w:t>Old</w:t></w:r></w:p>"#;
    assert_eq!(
        set(r, "a\tb\nc\r\nd\re"),
        r#"<w:p><w:r><w:rPr><w:i/></w:rPr><w:t>a</w:t><w:tab/><w:t>b</w:t><w:br/><w:t>c</w:t><w:br/><w:t>d</w:t><w:br/><w:t>e</w:t></w:r></w:p>"#
    );
    assert_eq!(set(r, ""), r#"<w:p><w:r><w:rPr><w:i/></w:rPr></w:r></w:p>"#, "empty text empties the run");
    // lastRenderedPageBreak, cr, hyphens are replaced; own text round-trips as text.
    let r = r#"<w:p><w:r><w:lastRenderedPageBreak/><w:t>x</w:t><w:noBreakHyphen/><w:t>y</w:t><w:cr/></w:r></w:p>"#;
    let mut doc = Document::from_bytes(docx(r, None, None)).unwrap();
    let own = doc.paragraph(0).unwrap().run(0).unwrap().text();
    assert_eq!(own, "x\u{2011}y\n");
    doc.set_run_text(0, 0, &own).unwrap();
    assert_eq!(doc.paragraph(0).unwrap().run(0).unwrap().text(), own);
    assert_eq!(set(r, &own), "<w:p><w:r><w:t>x\u{2011}y</w:t><w:br/></w:r></w:p>");
    // Tabs and breaks already in canonical form: writing them again is a no-op.
    let r = r#"<w:p><w:r><w:t>a</w:t><w:tab/><w:t>b</w:t><w:br/></w:r></w:p>"#;
    assert_eq!(set(r, "a\tb\n"), r);
}

#[test]
fn pretty_printed_runs() {
    let r = "<w:p>\n  <w:r>\n    <w:rPr><w:b/></w:rPr>\n    <w:t>Amet</w:t>\n  </w:r>\n</w:p>";
    assert_eq!(set(r, "Lorem"), "<w:p>\n  <w:r>\n    <w:rPr><w:b/></w:rPr>\n    <w:t>Lorem</w:t>\n  </w:r>\n</w:p>", "fast path despite whitespace nodes");
    assert_eq!(set(r, "a\tb"), "<w:p>\n  <w:r>\n    <w:rPr><w:b/></w:rPr>\n    \n  <w:t>a</w:t><w:tab/><w:t>b</w:t></w:r>\n</w:p>");
}

#[test]
fn runs_in_tables_and_hyperlinks() {
    let body = r#"<w:tbl><w:tblPr/><w:tr><w:tc><w:tcPr/><w:tbl><w:tr><w:tc><w:p><w:hyperlink w:anchor="a"><w:r><w:t>deep</w:t></w:r></w:hyperlink></w:p></w:tc></w:tr></w:tbl><w:p/></w:tc></w:tr></w:tbl>"#;
    let out = edit(body, |d| {
        assert_eq!(d.paragraph(0).unwrap().text(), "deep");
        d.set_run_text(0, 0, "nested").unwrap();
    });
    assert_eq!(out, body.replace(">deep<", ">nested<"));
}

#[test]
fn append_paragraph() {
    let body = r#"<w:p><w:r><w:t>a</w:t></w:r></w:p><w:sectPr><w:pgSz w:w="11906"/></w:sectPr>"#;
    let out = edit(body, |d| assert_eq!(d.append_paragraph(" x\ty").unwrap(), 1));
    assert_eq!(out, r#"<w:p><w:r><w:t>a</w:t></w:r></w:p><w:p><w:r><w:t xml:space="preserve"> x</w:t><w:tab/><w:t>y</w:t></w:r></w:p><w:sectPr><w:pgSz w:w="11906"/></w:sectPr>"#);
    let out = edit("<w:p/>", |d| {
        d.append_paragraph("").unwrap();
        d.append_paragraph("b").unwrap();
        assert_eq!(d.paragraph_count(), 3);
        assert_eq!(d.paragraph(2).unwrap().text(), "b");
    });
    assert_eq!(out, "<w:p/><w:p/><w:p><w:r><w:t>b</w:t></w:r></w:p>");
    // Appended runs are editable afterwards.
    let out = edit("", |d| {
        let i = d.append_paragraph("one").unwrap();
        d.set_run_text(i, 0, "two").unwrap();
    });
    assert_eq!(out, "<w:p><w:r><w:t>two</w:t></w:r></w:p>");
    // A sectPr in pPr (section break) is not the body-level sectPr.
    let body = r#"<w:p><w:pPr><w:sectPr/></w:pPr></w:p><w:sectPr/>"#;
    assert_eq!(edit(body, |d| { d.append_paragraph("z").unwrap(); }), r#"<w:p><w:pPr><w:sectPr/></w:pPr></w:p><w:p><w:r><w:t>z</w:t></w:r></w:p><w:sectPr/>"#);
}

#[test]
fn save_round_trip() {
    let dir = std::env::temp_dir().join(format!("ooxml-docx-save-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("a.docx");
    std::fs::write(&path, docx("<w:p><w:r><w:t>a</w:t></w:r></w:p>", None, None)).unwrap();
    let mut doc = Document::open(&path).unwrap();
    doc.set_run_text(0, 0, "b").unwrap();
    doc.save(&path).unwrap();
    assert_eq!(Document::open(&path).unwrap().paragraph(0).unwrap().text(), "b");
    std::fs::remove_dir_all(&dir).unwrap();
}
