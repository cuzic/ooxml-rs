//! ADR-0011 part 1: new paragraphs (§9.1), section breaks (§6, A2 lifted)
//! and hyperlinks (§4). Unit documents (T2, T4: Err ⇒ byte-identical),
//! T-OPC on the added relationships, T5 read-back with docx-rs, and a
//! fixture sweep.

mod common;

use std::assert_matches;

use common::*;
use ooxml_core::Error as CoreError;
use ooxml_docx::{
    Document, Error, HyperlinkTarget, Operation, ParaRef, ParagraphSpec, Place, ReplaceOptions, RunPosition, RunRef,
    SectionBreak, StoryIndex,
};

const R_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

fn p(i: usize) -> ParaRef {
    ParaRef::new(StoryIndex::MAIN, i)
}

fn open(body: &str) -> Document {
    Document::from_bytes(package(&document_xml(body), &[])).unwrap()
}

fn body_of(doc: &Document) -> String {
    let s = String::from_utf8(doc.package().read_part("word/document.xml").unwrap()).unwrap();
    let a = s.find("<w:body>").unwrap() + 8;
    let b = s.rfind("</w:body>").unwrap();
    s[a..b].to_owned()
}

fn refused<T: std::fmt::Debug>(doc: &mut Document, f: impl FnOnce(&mut Document) -> Result<T, Error>) -> Error {
    let before = doc.to_bytes().unwrap();
    let e = f(doc).unwrap_err();
    assert_eq!(doc.to_bytes().unwrap(), before, "Err ⇒ byte-identical");
    e
}

fn texts(doc: &Document) -> Vec<String> {
    doc.paragraphs().map(|p| p.text()).collect()
}

#[test]
fn insert_paragraphs() {
    let styles = story_xml("styles", r#"<w:style w:type="paragraph" w:styleId="Quote"><w:name w:val="Quote"/></w:style>"#);
    let body = r#"<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="3"/></w:numPr><w:sectPr/></w:pPr><w:r><w:t>one</w:t></w:r></w:p><w:tbl><w:tr><w:tc><w:p><w:r><w:t>cell</w:t></w:r></w:p></w:tc></w:tr></w:tbl><w:p><w:r><w:t>two</w:t></w:r></w:p>"#;
    let mut doc = Document::from_bytes(package(&document_xml(body), &[("styles", "styles.xml", &styles)])).unwrap();
    let m = doc.find("two", &ReplaceOptions::default()).unwrap().remove(0);
    let new = doc.insert_paragraph(Place::After(p(0)), "a\tb", &ParagraphSpec::LikeNeighbour).unwrap();
    assert_eq!(new, p(1));
    doc.insert_paragraph(Place::Before(p(0)), "zero", &ParagraphSpec::Style("Quote".into())).unwrap();
    doc.insert_paragraph(Place::After(p(3)), "in cell", &ParagraphSpec::Plain).unwrap();
    assert_eq!(texts(&doc), ["zero", "one", "a\tb", "cell", "in cell", "two"]);
    assert_eq!(
        body_of(&doc),
        r#"<w:p><w:pPr><w:pStyle w:val="Quote"/></w:pPr><w:r><w:t>zero</w:t></w:r></w:p><w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="3"/></w:numPr><w:sectPr/></w:pPr><w:r><w:t>one</w:t></w:r></w:p><w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="3"/></w:numPr></w:pPr><w:r><w:t>a</w:t><w:tab/><w:t>b</w:t></w:r></w:p><w:tbl><w:tr><w:tc><w:p><w:r><w:t>cell</w:t></w:r></w:p><w:p><w:r><w:t>in cell</w:t></w:r></w:p></w:tc></w:tr></w:tbl><w:p><w:r><w:t>two</w:t></w:r></w:p>"#
    );
    // a match found before the inserts is stale: its paragraph moved
    assert_matches!(doc.replace_at(&m, "2", &ReplaceOptions::default()), Err(Error::StaleMatch));
    // the same text moving into a match's index is still stale (generations shift)
    let mut doc = open(r#"<w:p><w:r><w:t>x</w:t></w:r></w:p><w:p><w:r><w:t>x</w:t></w:r></w:p>"#);
    let second = doc.find("x", &ReplaceOptions::default()).unwrap().remove(1);
    doc.insert_paragraph(Place::Before(p(0)), "y", &ParagraphSpec::Plain).unwrap();
    assert_eq!(doc.paragraph(1).unwrap().text(), "x", "the old first paragraph is now at the match's index");
    assert_matches!(doc.replace_at(&second, "z", &ReplaceOptions::default()), Err(Error::StaleMatch));
    // in a header
    let hdr = story_xml("hdr", r#"<w:p><w:r><w:t>h</w:t></w:r></w:p>"#);
    let mut doc = Document::from_bytes(package(&document_xml("<w:p/>"), &[("header", "header1.xml", &hdr)])).unwrap();
    let h = doc.stories().nth(1).unwrap().index();
    doc.insert_paragraph(Place::After(ParaRef::new(h, 0)), "h2", &ParagraphSpec::Plain).unwrap();
    let texts: Vec<String> = doc.story_at(h).unwrap().paragraphs().map(|p| p.text()).collect();
    assert_eq!(texts, ["h", "h2"]);
}

#[test]
fn insert_paragraph_refusals() {
    let mut doc = open(r#"<w:sdt><w:sdtContent><w:p><w:r><w:t>a</w:t></w:r></w:p></w:sdtContent></w:sdt>"#);
    assert_matches!(refused(&mut doc, |d| d.insert_paragraph(Place::After(p(0)), "x", &ParagraphSpec::Plain)), Error::UnsupportedContext { .. });
    let fld = r#"<w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText>PAGE</w:instrText></w:r></w:p><w:p><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p>"#;
    let mut doc = open(fld);
    assert_matches!(refused(&mut doc, |d| d.insert_paragraph(Place::After(p(0)), "x", &ParagraphSpec::Plain)), Error::InsideField);
    let mut doc = open(r#"<w:p><w:pPr><w:pPrChange w:id="1" w:author="a"><w:pPr/></w:pPrChange></w:pPr></w:p>"#);
    assert_matches!(refused(&mut doc, |d| d.insert_paragraph(Place::After(p(0)), "x", &ParagraphSpec::LikeNeighbour)), Error::TrackedPropertyChange { .. });
    let mut doc = open("<w:p/>");
    assert_matches!(refused(&mut doc, |d| d.insert_paragraph(Place::After(p(0)), "x", &ParagraphSpec::Style("Nope".into()))), Error::UnknownStyle { .. });
    assert_matches!(refused(&mut doc, |d| d.insert_paragraph(Place::After(p(0)), "\u{1}", &ParagraphSpec::Plain)), Error::InvalidXmlChar { .. });
    assert_matches!(refused(&mut doc, |d| d.insert_paragraph(Place::After(p(5)), "x", &ParagraphSpec::Plain)), Error::NoSuchParagraph(5));
    // W2: the structure column allows TC/XE only
    let seq = r#"<w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText>SEQ a</w:instrText></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p><w:p/>"#;
    let mut doc = open(seq);
    assert!(doc.write_gate_for(Operation::Text).is_ok());
    assert_matches!(refused(&mut doc, |d| d.insert_paragraph(Place::After(p(1)), "x", &ParagraphSpec::Plain)), Error::StaleFieldRisk { .. });
    let tc = r#"<w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText>TC "a"</w:instrText></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p><w:p/>"#;
    open(tc).insert_paragraph(Place::After(p(1)), "x", &ParagraphSpec::Plain).unwrap();
}

fn rels(doc: &Document, part: &str) -> String {
    let name = match part.rsplit_once('/') {
        Some((dir, file)) => format!("{dir}/_rels/{file}.rels"),
        None => format!("_rels/{part}.rels"),
    };
    String::from_utf8(doc.package().read_part(&name).unwrap_or_default()).unwrap()
}

fn sectioned(final_sect: &str, rels_for_header: bool) -> Document {
    let body = format!(r#"<w:p><w:r><w:t>one</w:t></w:r></w:p><w:p><w:r><w:t>two</w:t></w:r></w:p>{final_sect}"#);
    let hdr = story_xml("hdr", "<w:p/>");
    let related: Vec<(&str, &str, &str)> = if rels_for_header { vec![("header", "header1.xml", &hdr)] } else { vec![] };
    Document::from_bytes(package(&document_xml(&body), &related)).unwrap()
}

#[test]
fn section_breaks() {
    let s = r#"<w:sectPr><w:headerReference w:type="default" r:id="rIdX0"/><w:pgSz w:w="12240" w:h="15840"/><w:lnNumType w:countBy="1" w:restart="newSection"/><w:pgNumType w:fmt="lowerRoman" w:start="3"/><w:titlePg/></w:sectPr>"#;
    let mut doc = sectioned(s, true);
    doc.insert_section_break(p(0), SectionBreak::Continuous).unwrap();
    assert_eq!(
        body_of(&doc),
        r#"<w:p><w:pPr><w:sectPr><w:headerReference w:type="default" r:id="rIdX0"/><w:pgSz w:w="12240" w:h="15840"/><w:lnNumType w:countBy="1" w:restart="newSection"/><w:pgNumType w:fmt="lowerRoman" w:start="3"/><w:titlePg/></w:sectPr></w:pPr><w:r><w:t>one</w:t></w:r></w:p><w:p><w:r><w:t>two</w:t></w:r></w:p><w:sectPr><w:headerReference w:type="default" r:id="rIdX0"/><w:type w:val="continuous"/><w:pgSz w:w="12240" w:h="15840"/><w:lnNumType w:countBy="1" w:restart="continuous"/><w:pgNumType w:fmt="lowerRoman"/><w:titlePg/></w:sectPr>"#
    );
    assert_matches!(refused(&mut doc, |d| d.insert_section_break(p(0), SectionBreak::NextPage)), Error::AlreadySectionEnd);
    // a break inside a table, a revision, an undefined reference, no sectPr at all
    let mut doc = open(r#"<w:tbl><w:tr><w:tc><w:p/></w:tc></w:tr></w:tbl><w:sectPr/>"#);
    assert_matches!(refused(&mut doc, |d| d.insert_section_break(p(0), SectionBreak::NextPage)), Error::UnsupportedContext { .. });
    let mut doc = sectioned(r#"<w:sectPr><w:sectPrChange w:id="1" w:author="a"><w:sectPr/></w:sectPrChange></w:sectPr>"#, false);
    assert_matches!(refused(&mut doc, |d| d.insert_section_break(p(0), SectionBreak::NextPage)), Error::TrackedPropertyChange { element: "w:sectPrChange" });
    let mut doc = sectioned(r#"<w:sectPr><w:headerReference w:type="default" r:id="rId9"/></w:sectPr>"#, true);
    assert_matches!(refused(&mut doc, |d| d.insert_section_break(p(0), SectionBreak::NextPage)), Error::RelIdReferenced { .. });
    let mut doc = sectioned("", false);
    assert_matches!(refused(&mut doc, |d| d.insert_section_break(p(0), SectionBreak::NextPage)), Error::NoSectionProperties);
    // printerSettings carries r:id: the copy is refused (NotDetachable)
    let mut doc = sectioned(r#"<w:sectPr><w:printerSettings r:id="rIdX0"/></w:sectPr>"#, true);
    assert_matches!(refused(&mut doc, |d| d.insert_section_break(p(0), SectionBreak::NextPage)), Error::Core(CoreError::NotDetachable { .. }));
    // a section ended by a later paragraph: that paragraph's sectPr is S
    let body = r#"<w:p/><w:p><w:pPr><w:sectPr><w:pgSz w:w="1"/></w:sectPr></w:pPr></w:p><w:sectPr><w:pgSz w:w="2"/></w:sectPr>"#;
    let mut doc = open(body);
    let generations = [doc.generation(p(0)), doc.generation(p(1))];
    doc.insert_section_break(p(0), SectionBreak::OddPage).unwrap();
    assert_ne!(doc.generation(p(0)), generations[0]);
    assert_ne!(doc.generation(p(1)), generations[1]);
    assert_eq!(
        body_of(&doc),
        r#"<w:p><w:pPr><w:sectPr><w:pgSz w:w="1"/></w:sectPr></w:pPr></w:p><w:p><w:pPr><w:sectPr><w:type w:val="oddPage"/><w:pgSz w:w="1"/></w:sectPr></w:pPr></w:p><w:sectPr><w:pgSz w:w="2"/></w:sectPr>"#
    );
}

#[test]
fn append_moves_the_last_section_with_its_references() {
    let body = r#"<w:p><w:r><w:t>one</w:t></w:r></w:p><w:p><w:pPr><w:sectPr><w:footerReference w:type="first" r:id="rIdX0"/><w:pgSz w:w="11906"/></w:sectPr></w:pPr><w:r><w:t>last</w:t></w:r></w:p>"#;
    let ftr = story_xml("ftr", "<w:p/>");
    let mut doc = Document::from_bytes(package(&document_xml(body), &[("footer", "footer1.xml", &ftr)])).unwrap();
    assert_eq!(doc.append_paragraph("NEW").unwrap(), 2);
    assert_eq!(
        body_of(&doc),
        r#"<w:p><w:r><w:t>one</w:t></w:r></w:p><w:p><w:pPr></w:pPr><w:r><w:t>last</w:t></w:r></w:p><w:p><w:pPr><w:sectPr><w:footerReference w:type="first" r:id="rIdX0"/><w:pgSz w:w="11906"/></w:sectPr></w:pPr><w:r><w:t>NEW</w:t></w:r></w:p>"#
    );
    let re = Document::from_bytes(doc.to_bytes().unwrap()).unwrap();
    assert_eq!(texts(&re), ["one", "last", "NEW"]);
    assert_eq!(re.stories().nth(1).unwrap().sections().len(), 1, "the footer is still used by the (only) section");
}

#[test]
fn insert_after_a2_keeps_the_landscape_section() {
    let section = r#"<w:sectPr><w:pgSz w:w="16838" w:h="11906" w:orient="landscape"/></w:sectPr>"#;
    let num = r#"<w:numPr><w:ilvl w:val="0"/><w:numId w:val="3"/></w:numPr>"#;
    let body = format!(r#"<w:p/><w:p><w:pPr>{num}{section}</w:pPr><w:r><w:t>last</w:t></w:r></w:p>"#);
    let style = r#"<w:pStyle w:val="Quote"/>"#;
    let styles = story_xml("styles", r#"<w:style w:type="paragraph" w:styleId="Quote"><w:name w:val="Quote"/></w:style>"#);
    for (spec, properties) in [
        (ParagraphSpec::Plain, ""),
        (ParagraphSpec::LikeNeighbour, num),
        (ParagraphSpec::Style("Quote".into()), style),
    ] {
        let mut doc = Document::from_bytes(package(&document_xml(&body), &[("styles", "styles.xml", &styles)])).unwrap();
        let generation = doc.generation(p(1));
        assert_eq!(doc.insert_paragraph(Place::After(p(1)), "new", &spec).unwrap(), p(2));
        assert_ne!(doc.generation(p(1)), generation);
        assert_eq!(
            body_of(&doc),
            format!(r#"<w:p/><w:p><w:pPr>{num}</w:pPr><w:r><w:t>last</w:t></w:r></w:p><w:p><w:pPr>{properties}{section}</w:pPr><w:r><w:t>new</w:t></w:r></w:p>"#)
        );
    }
    for wrapped in [
        format!("<w:sdt><w:sdtContent><w:p><w:pPr>{section}</w:pPr></w:p></w:sdtContent></w:sdt>"),
        format!("<w:tbl><w:tr><w:tc><w:p><w:pPr>{section}</w:pPr></w:p></w:tc></w:tr></w:tbl>"),
    ] {
        let mut doc = open(&wrapped);
        assert_matches!(
            refused(&mut doc, |d| d.insert_paragraph(Place::After(p(0)), "new", &ParagraphSpec::Plain)),
            Error::SectionBreakAtEnd
        );
    }
}

#[test]
fn empty_paragraph_inside_a_complex_field_refuses_structure_edits() {
    let body = r#"<w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText>HYPERLINK "https://example.com/"</w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>first</w:t></w:r></w:p><w:p/><w:p><w:r><w:t>last</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p><w:sectPr/>"#;
    let mut doc = open(body);
    for place in [Place::Before(p(1)), Place::After(p(1))] {
        assert_matches!(
            refused(&mut doc, |d| d.insert_paragraph(place, "x", &ParagraphSpec::Plain)),
            Error::InsideField
        );
    }
    assert_matches!(
        refused(&mut doc, |d| d.insert_hyperlink(RunPosition::EndOf(p(1)), "x", &HyperlinkTarget::External("https://example.com/".into()))),
        Error::InsideField
    );
    assert_matches!(
        refused(&mut doc, |d| d.insert_section_break(p(1), SectionBreak::NextPage)),
        Error::InsideField
    );
}

#[test]
fn section_break_refuses_table_section_properties() {
    let mut doc = open(r#"<w:p/><w:tbl><w:tr><w:tc><w:p><w:pPr><w:sectPr><w:pgSz w:w="1"/></w:sectPr></w:pPr></w:p></w:tc></w:tr></w:tbl><w:p/><w:sectPr><w:pgSz w:w="2"/></w:sectPr>"#);
    assert_matches!(
        refused(&mut doc, |d| d.insert_section_break(p(0), SectionBreak::NextPage)),
        Error::UnsupportedContext { ancestor: "a section break inside a table" }
    );
}

#[test]
fn section_break_refuses_when_relationship_prefix_is_out_of_scope() {
    let xml = format!(r#"<w:document xmlns:w="{W_NS}"><w:body><w:p/><w:p/><w:sectPr><w:headerReference xmlns:r="{R_NS}" w:type="default" r:id="rIdX0"/></w:sectPr></w:body></w:document>"#);
    let header = story_xml("hdr", "<w:p/>");
    let mut doc = Document::from_bytes(package(&xml, &[("header", "header1.xml", &header)])).unwrap();
    assert_matches!(
        refused(&mut doc, |d| d.insert_section_break(p(0), SectionBreak::NextPage)),
        Error::Core(CoreError::NamespaceNotInScope(ns)) if ns == R_NS
    );
}

#[test]
fn append_refuses_unknown_section_relationship() {
    let mut doc = open(r#"<w:p><w:pPr><w:sectPr><w:headerReference w:type="default" r:id="rId9"/></w:sectPr></w:pPr></w:p>"#);
    assert_matches!(
        refused(&mut doc, |d| d.append_paragraph("x")),
        Error::RelIdReferenced { id } if id == "rId9"
    );
}

#[test]
fn section_break_refuses_last_paragraph_of_final_section() {
    let mut doc = open(r#"<w:p/><w:sectPr><w:pgSz w:w="1"/></w:sectPr>"#);
    assert_matches!(
        refused(&mut doc, |d| d.insert_section_break(p(0), SectionBreak::NextPage)),
        Error::AlreadySectionEnd
    );
}

#[test]
fn insertions_refuse_separator_notes() {
    for (note, ancestor) in [
        ("footnote", "w:footnote (separator)"),
        ("endnote", "w:endnote (separator)"),
    ] {
        for kind in ["separator", "continuationSeparator", "continuationNotice"] {
            let notes = story_xml(&format!("{note}s"), &format!(r#"<w:{note} w:type="{kind}" w:id="-1"><w:p/></w:{note}>"#));
            let part = format!("{note}s.xml");
            let mut doc = Document::from_bytes(package(&document_xml("<w:p/>"), &[(&format!("{note}s"), &part, &notes)])).unwrap();
            let at = ParaRef::new(doc.stories().nth(1).unwrap().index(), 0);
            assert_matches!(
                refused(&mut doc, |d| d.insert_paragraph(Place::Before(at), "x", &ParagraphSpec::Plain)),
                Error::UnsupportedContext { ancestor: a } if a == ancestor
            );
            assert_matches!(
                refused(&mut doc, |d| d.insert_hyperlink(RunPosition::EndOf(at), "x", &HyperlinkTarget::External("https://example.com/".into()))),
                Error::UnsupportedContext { ancestor: a } if a == ancestor
            );
        }
    }
}

#[test]
fn append_reports_the_tracked_paragraph_properties() {
    let mut doc = open(r#"<w:p><w:pPr><w:sectPr/><w:pPrChange w:id="1" w:author="a"><w:pPr/></w:pPrChange></w:pPr></w:p>"#);
    assert_matches!(
        refused(&mut doc, |d| d.append_paragraph("x")),
        Error::TrackedPropertyChange { element: "w:pPrChange" }
    );
}

#[test]
fn hyperlinks() {
    let styles = story_xml("styles", r#"<w:style w:type="character" w:styleId="a5"><w:name w:val="Hyperlink"/></w:style>"#);
    let body = r#"<w:p><w:r><w:t>see </w:t></w:r><w:r><w:t>end</w:t></w:r></w:p><w:p><w:bookmarkStart w:id="0" w:name="Target"/><w:bookmarkEnd w:id="0"/></w:p>"#;
    let input = package(&document_xml(body), &[("styles", "styles.xml", &styles)]);
    let mut doc = Document::from_bytes(input.clone()).unwrap();
    let r = doc.insert_hyperlink(RunPosition::After(RunRef::new(p(0), 0)), "ACME", &HyperlinkTarget::External("https://example.com/a b%20c".replace(' ', "%20"))).unwrap();
    assert_eq!(r, RunRef::new(p(0), 1));
    assert_eq!(doc.paragraph(0).unwrap().text(), "see ACMEend");
    doc.insert_hyperlink(RunPosition::EndOf(p(0)), "internal", &HyperlinkTarget::Bookmark("target".into())).unwrap();
    let b = body_of(&doc);
    assert!(
        b.starts_with(r#"<w:p><w:r><w:t>see </w:t></w:r><w:hyperlink r:id="rId1" w:history="1"><w:r><w:rPr><w:rStyle w:val="a5"/></w:rPr><w:t>ACME</w:t></w:r></w:hyperlink><w:r><w:t>end</w:t></w:r><w:hyperlink w:anchor="target" w:history="1"><w:r><w:rPr><w:rStyle w:val="a5"/></w:rPr><w:t>internal</w:t></w:r></w:hyperlink></w:p>"#),
        "{b}"
    );
    // T-OPC: the new relationship is external, unique, and resolves
    let rel = rels(&doc, "word/document.xml");
    assert!(rel.contains(r#"Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://example.com/a%20b%20c" TargetMode="External""#), "{rel}");
    let id = b.split(r#"r:id=""#).nth(1).unwrap().split('"').next().unwrap();
    assert!(rel.contains(&format!(r#"Id="{id}""#)));
    assert_eq!(rel.matches(&format!(r#"Id="{id}""#)).count(), 1);
    // only document.xml and its .rels changed (T3)
    let out = doc.to_bytes().unwrap();
    let changed: Vec<String> = entries(&out).into_iter().filter(|e| !entries(&input).contains(e)).map(|e| e.0).collect();
    assert!(changed.iter().all(|n| n == "word/_rels/document.xml.rels" || n == "word/document.xml"), "{changed:?}");
    // T5: docx-rs reads the hyperlink back
    let json = docx_rs::read_docx(&out).unwrap().json();
    assert!(json.contains("https://example.com/a%20b%20c") || json.contains("hyperlink"), "docx-rs sees the hyperlink");
}

#[test]
fn hyperlink_declares_r_only_when_needed_and_refusals() {
    // a document root without xmlns:r: the hyperlink declares it
    let xml = format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="{W_NS}"><w:body><w:p><w:r><w:t>a</w:t></w:r></w:p></w:body></w:document>"#);
    let mut doc = Document::from_bytes(package(&xml, &[])).unwrap();
    doc.insert_hyperlink(RunPosition::EndOf(p(0)), "x", &HyperlinkTarget::External("mailto:a@example.com".into())).unwrap();
    let b = body_of(&doc);
    assert!(b.contains(&format!(r#"<w:hyperlink xmlns:r="{R_NS}" r:id="#)), "{b}");
    // refusals
    let mut doc = open(r#"<w:p><w:hyperlink w:anchor="x"><w:r><w:t>in</w:t></w:r></w:hyperlink></w:p>"#);
    assert_matches!(refused(&mut doc, |d| d.insert_hyperlink(RunPosition::After(RunRef::new(p(0), 0)), "x", &HyperlinkTarget::External("https://e/".into()))), Error::NestedHyperlink);
    assert_matches!(refused(&mut doc, |d| d.insert_hyperlink(RunPosition::EndOf(p(0)), "x", &HyperlinkTarget::External("example.com".into()))), Error::InvalidValue { .. });
    assert_matches!(refused(&mut doc, |d| d.insert_hyperlink(RunPosition::EndOf(p(0)), "x", &HyperlinkTarget::Bookmark("missing".into()))), Error::UnknownBookmark { .. });
    // a dangling r:id: the new id never equals it (review N4; core's
    // allocation already skips ids the source part's attributes use, PR #38
    // M7, and ooxml-docx's RelIdReferenced check is the safety net)
    let mut doc = open(r#"<w:p><w:r><w:object><v:imagedata xmlns:v="urn:schemas-microsoft-com:vml" r:id="rId1"/></w:object></w:r></w:p>"#);
    doc.insert_hyperlink(RunPosition::EndOf(p(0)), "x", &HyperlinkTarget::External("https://e/".into())).unwrap();
    assert!(body_of(&doc).contains(r#"<w:hyperlink r:id="rId2" w:history="1">"#), "{}", body_of(&doc));
    // core refuses a target with whitespace
    let mut doc = open("<w:p/>");
    assert_matches!(refused(&mut doc, |d| d.insert_hyperlink(RunPosition::EndOf(p(0)), "x", &HyperlinkTarget::External("https://e/a b".into()))), Error::Core(_));
}

/// The zip's entry names, for T3.
fn names(bytes: &[u8]) -> Vec<String> {
    part_names(bytes)
}

/// Fixture sweep: on every writable fixture, a paragraph after each of the
/// first 30 paragraphs of every story, a section break after the first
/// body paragraph that allows one, a hyperlink at the end of the first
/// paragraph that allows one, then append. Refusals change nothing; the
/// output reopens, keeps every original text in order, and only the main
/// story parts and the main part's relationships change.
#[test]
fn fixture_sweep() {
    let (mut ok, mut refusals) = (0, 0);
    for stem in writable() {
        let input = fixture(stem);
        let mut doc = Document::from_bytes(input.clone()).unwrap();
        let before_names = names(&input);
        let stories: Vec<StoryIndex> = doc.stories().map(|s| s.index()).collect();
        let mut expected: Vec<Vec<String>> = doc.stories().map(|s| s.paragraphs().map(|p| p.text()).collect()).collect();
        for s in &stories {
            let n = doc.story_at(*s).unwrap().paragraph_count().min(30);
            // from the end, so earlier indices stay valid
            for i in (0..n).rev() {
                let snap = doc.to_bytes().unwrap();
                match doc.insert_paragraph(Place::After(ParaRef::new(*s, i)), "INSERTED", &ParagraphSpec::LikeNeighbour) {
                    Ok(_) => {
                        ok += 1;
                        expected[s.get()].insert(i + 1, "INSERTED".into());
                    }
                    Err(_) => {
                        refusals += 1;
                        assert_eq!(doc.to_bytes().unwrap(), snap, "{stem} Err ⇒ byte-identical");
                    }
                }
            }
        }
        for i in 0..doc.paragraph_count() {
            let snap = doc.to_bytes().unwrap();
            match doc.insert_section_break(p(i), SectionBreak::NextPage) {
                Ok(()) => {
                    ok += 1;
                    break;
                }
                Err(_) => assert_eq!(doc.to_bytes().unwrap(), snap, "{stem}"),
            }
        }
        for (i, t) in expected[0].iter_mut().enumerate() {
            let snap = doc.to_bytes().unwrap();
            match doc.insert_hyperlink(RunPosition::EndOf(p(i)), "LINK", &HyperlinkTarget::External("https://example.com/".into())) {
                Ok(_) => {
                    ok += 1;
                    t.push_str("LINK");
                    break;
                }
                Err(_) => assert_eq!(doc.to_bytes().unwrap(), snap, "{stem}"),
            }
        }
        if doc.append_paragraph("APPENDED").is_ok() {
            expected[0].push("APPENDED".into());
        }
        let out = doc.to_bytes().unwrap();
        let re = Document::from_bytes(out.clone()).unwrap();
        let got: Vec<Vec<String>> = re.stories().map(|s| s.paragraphs().map(|p| p.text()).collect()).collect();
        assert_eq!(got, expected, "{stem}");
        // no part added or removed; untouched parts byte-identical
        assert_eq!(names(&out).len(), before_names.len(), "{stem}");
        let story_parts: Vec<String> = doc.stories().map(|s| s.id().part().to_owned()).collect();
        for (a, b) in entries(&out).iter().zip(entries(&input)) {
            if a != &b {
                assert!(story_parts.contains(&a.0) || a.0 == "word/_rels/document.xml.rels" || a.0.ends_with(".rels"), "{stem}: {} changed", a.0);
            }
        }
    }
    eprintln!("structure sweep: {ok} writes, {refusals} refusals");
    assert!(ok > 100);
}
