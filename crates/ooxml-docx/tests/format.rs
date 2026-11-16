//! ADR-0007 §4 (direct formatting), §5 (styles by id), §6.3 (the
//! `updateFields` opt-in): unit documents and fixture sweeps (T2, T4, T5,
//! T6, T8, T9). Every refused call leaves `to_bytes()` identical.

mod common;

use std::assert_matches;

use common::*;
use ooxml_core::{Content, ElementSpec, Error as CoreError, RawNode};
use ooxml_docx::{
    Alignment, Color, Document, Error, FirstLine, HalfPoints, Highlight, LineSpacing, Operation, ParaRef,
    ParagraphFormat, Prop, ReplaceOptions, RunFormat, RunRef, StoryIndex, Underline, VertAlign,
};
use ooxml_schema::wml;
use serde_json::Value;

fn p(i: usize) -> ParaRef {
    ParaRef::new(StoryIndex::MAIN, i)
}
fn r(para: usize, run: usize) -> RunRef {
    RunRef::new(p(para), run)
}

const STYLES_REL: (&str, &str) = ("styles", "styles.xml");

/// A package with a body and, optionally, `styles.xml` / `settings.xml` inner XML.
fn open_with(body: &str, styles: Option<&str>, settings: Option<&str>) -> Document {
    let styles = styles.map(|s| story_xml("styles", s));
    let settings = settings.map(|s| story_xml("settings", s));
    let mut rel = Vec::new();
    if let Some(s) = &styles {
        rel.push((STYLES_REL.0, STYLES_REL.1, s.as_str()));
    }
    if let Some(s) = &settings {
        rel.push(("settings", "settings.xml", s.as_str()));
    }
    Document::from_bytes(package(&document_xml(body), &rel)).unwrap()
}

fn open(body: &str) -> Document {
    open_with(body, None, None)
}

fn body_of(doc: &Document) -> String {
    let s = String::from_utf8(doc.package().read_part("word/document.xml").unwrap()).unwrap();
    let a = s.find("<w:body>").unwrap() + 8;
    let b = s.rfind("</w:body>").unwrap();
    s[a..b].to_owned()
}

/// Asserts the call failed and changed nothing.
fn refused<T: std::fmt::Debug>(doc: &mut Document, f: impl FnOnce(&mut Document) -> Result<T, Error>) -> Error {
    let before = doc.to_bytes().unwrap();
    let e = f(doc).unwrap_err();
    assert_eq!(doc.to_bytes().unwrap(), before, "Err ⇒ byte-identical");
    e
}

fn bold() -> RunFormat {
    RunFormat { bold: Prop::Set(true), ..RunFormat::default() }
}

#[test]
fn run_properties_are_written_in_slot_order() {
    let mut doc = open("<w:p><w:r><w:t>a</w:t></w:r></w:p>");
    let f = RunFormat {
        bold: Prop::Set(true),
        italic_cs: Prop::Set(false),
        underline: Prop::Set(Underline::Double),
        color: Prop::Set(Color::Rgb(0xC0, 0, 0x1A)),
        size: Prop::Set(HalfPoints::new(24).unwrap()),
        highlight: Prop::Set(Highlight::Yellow),
        vert_align: Prop::Set(VertAlign::Superscript),
        fonts: ooxml_docx::FontSlots { ascii: Prop::Set("Arial".into()), ..Default::default() },
        ..RunFormat::default()
    };
    doc.format_run(r(0, 0), &f).unwrap();
    assert_eq!(
        body_of(&doc),
        r#"<w:p><w:r><w:rPr><w:rFonts w:ascii="Arial"/><w:b/><w:iCs w:val="0"/><w:color w:val="C0001A"/><w:sz w:val="24"/><w:highlight w:val="yellow"/><w:u w:val="double"/><w:vertAlign w:val="superscript"/></w:rPr><w:t>a</w:t></w:r></w:p>"#
    );
    // the same again is a no-op
    let before = doc.to_bytes().unwrap();
    doc.format_run(r(0, 0), &f).unwrap();
    assert_eq!(doc.to_bytes().unwrap(), before);
    assert_eq!(doc.generation(p(0)), 1);
    // Clear removes; true replaces val="0" by the bare form
    doc.format_run(r(0, 0), &RunFormat { italic_cs: Prop::Set(true), size: Prop::Clear, ..RunFormat::default() }).unwrap();
    assert!(body_of(&doc).contains(r#"<w:b/><w:iCs/><w:color w:val="C0001A"/><w:highlight"#), "{}", body_of(&doc));
}

#[test]
fn superseding_attributes_are_removed() {
    // the complex-script slot's theme attribute is spelled `w:cstheme` (review m3)
    let mut doc = open(r#"<w:p><w:r><w:rPr><w:rFonts w:cs="Arial" w:cstheme="minorBidi"/></w:rPr><w:t>a</w:t></w:r></w:p>"#);
    let f = RunFormat { fonts: ooxml_docx::FontSlots { cs: Prop::Set("Times New Roman".into()), ..Default::default() }, ..RunFormat::default() };
    doc.format_run(r(0, 0), &f).unwrap();
    assert!(body_of(&doc).contains(r#"<w:rFonts w:cs="Times New Roman"/>"#), "{}", body_of(&doc));
    let mut doc = open(
        r#"<w:p><w:r><w:rPr><w:rFonts w:asciiTheme="minorHAnsi" w:hAnsiTheme="minorHAnsi" w:hint="eastAsia"/><w:color w:val="1F3864" w:themeColor="accent1" w:themeShade="80"/></w:rPr><w:t>a</w:t></w:r></w:p>"#,
    );
    let f = RunFormat {
        color: Prop::Set(Color::Auto),
        fonts: ooxml_docx::FontSlots { ascii: Prop::Set("Calibri".into()), ..Default::default() },
        ..RunFormat::default()
    };
    doc.format_run(r(0, 0), &f).unwrap();
    assert_eq!(
        body_of(&doc),
        r#"<w:p><w:r><w:rPr><w:rFonts w:hAnsiTheme="minorHAnsi" w:hint="eastAsia" w:ascii="Calibri"/><w:color w:val="auto"/></w:rPr><w:t>a</w:t></w:r></w:p>"#
    );
    // an equal colour under a theme colour still removes the theme attributes
    let mut doc = open(r#"<w:p><w:r><w:rPr><w:color w:val="1F3864" w:themeColor="accent1"/></w:rPr><w:t>a</w:t></w:r></w:p>"#);
    doc.format_run(r(0, 0), &RunFormat { color: Prop::Set(Color::Rgb(0x1F, 0x38, 0x64)), ..RunFormat::default() }).unwrap();
    assert!(body_of(&doc).contains(r#"<w:color w:val="1F3864"/>"#));
}

#[test]
fn paragraph_properties_and_inherited_superseding_attributes() {
    let mut doc = open(r#"<w:p><w:pPr><w:ind w:leftChars="200" w:left="420" w:firstLineChars="100"/></w:pPr><w:r><w:t>a</w:t></w:r></w:p>"#);
    let f = ParagraphFormat {
        alignment: Prop::Set(Alignment::Center),
        space_after: Prop::Set(120),
        line_spacing: Prop::Set(LineSpacing::Auto(276)),
        indent_left: Prop::Set(720),
        first_line: Prop::Set(FirstLine::Hanging(360)),
        keep_next: Prop::Set(true),
        ..ParagraphFormat::default()
    };
    doc.format_paragraph(p(0), &f).unwrap();
    assert_eq!(
        body_of(&doc),
        r#"<w:p><w:pPr><w:keepNext/><w:spacing w:after="120" w:line="276" w:lineRule="auto"/><w:ind w:left="720" w:hanging="360"/><w:jc w:val="center"/></w:pPr><w:r><w:t>a</w:t></w:r></w:p>"#
    );
    // a style-level firstLineChars is cancelled with 0 on the direct element (review m3)
    let styles = r#"<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/><w:pPr><w:ind w:firstLineChars="200"/><w:spacing w:beforeAutospacing="1"/></w:pPr></w:style>"#;
    let mut doc = open_with("<w:p><w:r><w:t>a</w:t></w:r></w:p>", Some(styles), None);
    let f = ParagraphFormat { first_line: Prop::Set(FirstLine::Indent(420)), space_before: Prop::Set(0), ..ParagraphFormat::default() };
    doc.format_paragraph(p(0), &f).unwrap();
    assert_eq!(
        body_of(&doc),
        r#"<w:p><w:pPr><w:spacing w:before="0" w:beforeAutospacing="0"/><w:ind w:firstLine="420" w:firstLineChars="0"/></w:pPr><w:r><w:t>a</w:t></w:r></w:p>"#
    );
    // Clear returns to the inherited value: the attribute, the superseding ones and
    // the earlier "0" cancellation go, and an element left empty goes too (review m2)
    doc.format_paragraph(p(0), &ParagraphFormat { first_line: Prop::Clear, ..ParagraphFormat::default() }).unwrap();
    assert_eq!(body_of(&doc), r#"<w:p><w:pPr><w:spacing w:before="0" w:beforeAutospacing="0"/></w:pPr><w:r><w:t>a</w:t></w:r></w:p>"#);
    doc.format_paragraph(p(0), &ParagraphFormat { space_before: Prop::Clear, ..ParagraphFormat::default() }).unwrap();
    assert_eq!(body_of(&doc), r#"<w:p><w:r><w:t>a</w:t></w:r></w:p>"#, "the emptied pPr goes too (n1)");
    // review m2's case: direct *Chars/*Lines are cleared with their twips value
    let mut doc = open(r#"<w:p><w:pPr><w:spacing w:before="100" w:beforeLines="50"/><w:ind w:left="720" w:leftChars="200" w:firstLine="100" w:firstLineChars="50"/></w:pPr></w:p>"#);
    let f = ParagraphFormat { space_before: Prop::Clear, indent_left: Prop::Clear, first_line: Prop::Clear, ..ParagraphFormat::default() };
    doc.format_paragraph(p(0), &f).unwrap();
    assert_eq!(body_of(&doc), r#"<w:p></w:p>"#);
    // m1: an undefined pStyle falls back to the default paragraph style
    let mut doc = open_with(r#"<w:p><w:pPr><w:pStyle w:val="Missing"/></w:pPr></w:p>"#, Some(styles), None);
    doc.format_paragraph(p(0), &ParagraphFormat { first_line: Prop::Set(FirstLine::Indent(10)), ..ParagraphFormat::default() }).unwrap();
    assert!(body_of(&doc).contains(r#"<w:ind w:firstLine="10" w:firstLineChars="0"/>"#), "{}", body_of(&doc));
    // n2: Auto line spacing on an element without lineRule (auto by default) is a no-op
    let mut doc = open(r#"<w:p><w:pPr><w:spacing w:line="276"/></w:pPr></w:p>"#);
    let before = doc.to_bytes().unwrap();
    doc.format_paragraph(p(0), &ParagraphFormat { line_spacing: Prop::Set(LineSpacing::Auto(276)), ..ParagraphFormat::default() }).unwrap();
    assert_eq!(doc.to_bytes().unwrap(), before);
    // a style chain that loops
    let looped = r#"<w:style w:type="paragraph" w:styleId="A"><w:basedOn w:val="B"/></w:style><w:style w:type="paragraph" w:styleId="B"><w:basedOn w:val="A"/></w:style>"#;
    let mut doc = open_with(r#"<w:p><w:pPr><w:pStyle w:val="A"/></w:pPr></w:p>"#, Some(looped), None);
    let e = refused(&mut doc, |d| d.format_paragraph(p(0), &ParagraphFormat { indent_left: Prop::Set(1), ..ParagraphFormat::default() }));
    assert_matches!(e, Error::StyleChainTooDeep);
}

#[test]
fn refusals_before_any_write() {
    // M1: references out of range are errors, not panics, also for no-op style writes
    let mut doc = open_with("<w:p><w:r><w:t>a</w:t></w:r></w:p>", Some(STYLES), None);
    assert_matches!(refused(&mut doc, |d| d.set_paragraph_style(p(5), Some("Rubrik1"))), Error::NoSuchParagraph(5));
    assert_matches!(refused(&mut doc, |d| d.set_paragraph_style(p(5), None)), Error::NoSuchParagraph(5));
    assert_matches!(refused(&mut doc, |d| d.set_run_style(r(0, 3), Some("Rubrik1Char"))), Error::NoSuchRun { .. });
    assert_matches!(refused(&mut doc, |d| d.set_run_style(r(4, 0), None)), Error::NoSuchParagraph(4) | Error::NoSuchRun { .. });
    assert_matches!(refused(&mut doc, |d| d.format_run(r(0, 9), &bold())), Error::NoSuchRun { .. });
    // M2: an invalid XML character in a font name refuses before any run is written
    let mut doc = open("<w:p><w:r><w:t>a</w:t></w:r><w:r><w:t>b</w:t></w:r></w:p>");
    let f = RunFormat { bold: Prop::Set(true), fonts: ooxml_docx::FontSlots { ascii: Prop::Set("A\u{1}".into()), ..Default::default() }, ..RunFormat::default() };
    assert_matches!(refused(&mut doc, |d| d.format_paragraph_runs(p(0), &f)), Error::InvalidXmlChar { ch: '\u{1}' });
    // n3: two styles relationships refuse the style setters
    let st = story_xml("styles", r#"<w:style w:type="paragraph" w:styleId="A"><w:name w:val="A"/></w:style>"#);
    let mut doc = Document::from_bytes(package(&document_xml("<w:p/>"), &[("styles", "styles.xml", &st), ("styles", "styles2.xml", &st)])).unwrap();
    assert_matches!(refused(&mut doc, |d| d.set_paragraph_style(p(0), Some("A"))), Error::AmbiguousStylesPart);
    // M2: a producer's out-of-order pPr (docxtemplater's shape) refuses before the first write
    let mut doc = open(r#"<w:p><w:pPr><w:sectPr/><w:pStyle w:val="x"/><w:bidi/><w:jc w:val="left"/><w:rPr/></w:pPr><w:r><w:t>a</w:t></w:r></w:p>"#);
    let f = ParagraphFormat { keep_next: Prop::Set(true), keep_lines: Prop::Set(true), ..ParagraphFormat::default() };
    assert_matches!(refused(&mut doc, |d| d.format_paragraph(p(0), &f)), Error::Core(CoreError::UnresolvedSibling(_)));
    // a run holding mc:AlternateContent: inserting rPr is refused (and the first run is not written either)
    let mut doc = open(r#"<w:p><w:r><w:t>a</w:t></w:r><w:r><mc:AlternateContent xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"/></w:r></w:p>"#);
    assert_matches!(refused(&mut doc, |d| d.format_paragraph_runs(p(0), &bold())), Error::Core(CoreError::UnresolvedSibling(_)));
    // a duplicated property
    let mut doc = open(r#"<w:p><w:r><w:rPr><w:sz w:val="20"/><w:sz w:val="22"/></w:rPr><w:t>a</w:t></w:r></w:p>"#);
    let f = RunFormat { size: Prop::Set(HalfPoints::new(24).unwrap()), ..RunFormat::default() };
    assert_matches!(refused(&mut doc, |d| d.format_run(r(0, 0), &f)), Error::DuplicateProperty { name } if name == "w:sz");
    // n4: WML declared only as the default namespace
    let xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><document xmlns="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><body><p><r><t>a</t></r></p></body></document>"#;
    let mut doc = Document::from_bytes(package(xml, &[])).unwrap();
    let e = refused(&mut doc, |d| d.format_run(r(0, 0), &RunFormat { size: Prop::Set(HalfPoints::new(24).unwrap()), ..RunFormat::default() }));
    assert_matches!(e, Error::Core(CoreError::NamespaceNotInScope(_)));
    // …but a new on/off element needs no attribute, so `<b/>` works there
    doc.format_run(r(0, 0), &bold()).unwrap();
    // targets
    let fld = r#"<w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText>PAGE</w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>1</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p>"#;
    let mut doc = open(fld);
    assert_matches!(refused(&mut doc, |d| d.format_run(r(0, 3), &bold())), Error::InsideField);
    assert_matches!(refused(&mut doc, |d| d.format_paragraph(p(0), &ParagraphFormat { keep_next: Prop::Set(true), ..Default::default() })), Error::InsideField);
    let mut doc = open(r#"<w:p><w:r><w:rPr><w:rPrChange w:id="1" w:author="a"><w:rPr/></w:rPrChange></w:rPr><w:t>a</w:t></w:r></w:p>"#);
    assert_matches!(refused(&mut doc, |d| d.format_run(r(0, 0), &bold())), Error::TrackedPropertyChange { element: "w:rPrChange" });
    let mut doc = open(r#"<w:p><w:pPr><w:pPrChange w:id="1" w:author="a"><w:pPr/></w:pPrChange></w:pPr></w:p><w:p><w:pPr><w:rPr><w:ins w:id="2" w:author="a"/></w:rPr></w:pPr></w:p><w:sdt><w:sdtContent><w:p/></w:sdtContent></w:sdt>"#);
    let f = ParagraphFormat { keep_next: Prop::Set(true), ..ParagraphFormat::default() };
    assert_matches!(refused(&mut doc, |d| d.format_paragraph(p(0), &f)), Error::TrackedPropertyChange { element: "w:pPrChange" });
    assert_matches!(refused(&mut doc, |d| d.format_paragraph(p(1), &f)), Error::InsideRevision { .. });
    assert_matches!(refused(&mut doc, |d| d.format_paragraph(p(2), &f)), Error::UnsupportedContext { ancestor: "w:sdt" });
    // R4 is not required: a run holding a tab and a page break can be formatted
    let mut doc = open(r#"<w:p><w:r><w:tab/><w:br w:type="page"/></w:r></w:p>"#);
    doc.format_run(r(0, 0), &bold()).unwrap();
    // atomic over the runs of a paragraph
    let mut doc = open(r#"<w:p><w:r><w:t>a</w:t></w:r><w:ins w:id="1" w:author="x"><w:r><w:t>b</w:t></w:r></w:ins></w:p>"#);
    assert_matches!(refused(&mut doc, |d| d.format_paragraph_runs(p(0), &bold())), Error::InsideRevision { .. });
    // an empty font name
    let mut doc = open("<w:p><w:r><w:t>a</w:t></w:r></w:p>");
    let f = RunFormat { fonts: ooxml_docx::FontSlots { cs: Prop::Set(String::new()), ..Default::default() }, ..RunFormat::default() };
    assert_matches!(refused(&mut doc, |d| d.format_run(r(0, 0), &f)), Error::InvalidValue { .. });
    assert!(HalfPoints::new(0).is_none() && HalfPoints::new(3277).is_none() && HalfPoints::new(3276).is_some());
}

/// Review m3: formatting the run holding a ruby inserts its `w:rPr` before
/// the `w:ruby`, which shifts the paths of the ruby base runs; the story is
/// re-walked, so later edits find their runs.
#[test]
fn ruby_runs_after_an_rpr_insert() {
    let body = r#"<w:p><w:r><w:ruby><w:rubyPr/><w:rt><w:r><w:t>かん</w:t></w:r></w:rt><w:rubyBase><w:r><w:t>漢</w:t></w:r></w:rubyBase></w:ruby></w:r><w:r><w:t>字</w:t></w:r></w:p>"#;
    let mut doc = open(body);
    let n = doc.paragraph(0).unwrap().run_count();
    assert_eq!(n, 3);
    doc.format_run(r(0, 0), &bold()).unwrap();
    assert_matches!(doc.format_run(r(0, 1), &bold()), Err(Error::UnsupportedContext { ancestor: "w:ruby" }));
    doc.format_run(r(0, 2), &bold()).unwrap();
    doc.format_paragraph(p(0), &ParagraphFormat { keep_next: Prop::Set(true), ..ParagraphFormat::default() }).unwrap();
    assert_eq!(
        body_of(&doc),
        r#"<w:p><w:pPr><w:keepNext/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:ruby><w:rubyPr/><w:rt><w:r><w:t>かん</w:t></w:r></w:rt><w:rubyBase><w:r><w:t>漢</w:t></w:r></w:rubyBase></w:ruby></w:r><w:r><w:rPr><w:b/></w:rPr><w:t>字</w:t></w:r></w:p>"#
    );
}

#[test]
fn formatting_uses_the_format_column_and_styles_the_document_wide_w2() {
    let fld = r#"<w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText>NUMWORDS</w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>9</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p><w:p><w:r><w:t>a</w:t></w:r></w:p>"#;
    let styles = r#"<w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/></w:style>"#;
    let mut doc = open_with(fld, Some(styles), None);
    doc.format_run(r(1, 0), &bold()).unwrap();
    assert_matches!(refused(&mut doc, |d| d.set_paragraph_style(p(1), Some("Heading1"))), Error::StaleFieldRisk { .. });
    assert_matches!(refused(&mut doc, |d| d.replace("a", "b", &ReplaceOptions::default())), Error::StaleFieldRisk { .. });
    // the opt-in lifts W2 for every operation
    let mut doc = open_with(fld, Some(styles), Some(r#"<w:zoom w:percent="100"/>"#));
    doc.enable_update_fields_on_open().unwrap();
    doc.set_paragraph_style(p(1), Some("Heading1")).unwrap();
    doc.replace("a", "b", &ReplaceOptions::default()).unwrap();
    assert!(doc.write_gates().is_empty());
}

const STYLES: &str = r#"<w:latentStyles><w:lsdException w:name="heading 2"/></w:latentStyles>
<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style>
<w:style w:type="paragraph" w:styleId="Rubrik1"><w:name w:val="heading 1"/><w:link w:val="Rubrik1Char"/></w:style>
<w:style w:type="character" w:styleId="Rubrik1Char"><w:name w:val="Rubrik 1 Char"/><w:link w:val="Rubrik1"/></w:style>
<w:style w:styleId="NoType"><w:name w:val="Ünïcode NAME"/></w:style>
<w:style w:type="paragraph" w:styleId="dup"/><w:style w:type="paragraph" w:styleId="DUP"/>"#;

#[test]
fn styles_by_id() {
    let mut doc = open_with(r#"<w:p><w:r><w:t>a</w:t></w:r></w:p><w:p><w:pPr><w:pStyle w:val="Rubrik1"/></w:pPr></w:p>"#, Some(STYLES), None);
    let ids: Vec<&str> = doc.styles().map(|s| s.id).collect();
    assert_eq!(ids, ["Normal", "Rubrik1", "Rubrik1Char", "NoType", "dup", "DUP"]);
    assert_eq!(doc.style_ids_by_name("Heading 1"), ["Rubrik1"]);
    assert_eq!(doc.style_ids_by_name("ünïcode name"), ["NoType"]);
    doc.set_paragraph_style(p(0), Some("Rubrik1")).unwrap();
    doc.set_paragraph_style(p(0), Some("NoType")).unwrap(); // absent type = paragraph
    doc.set_run_style(r(0, 0), Some("Rubrik1Char")).unwrap();
    assert_eq!(
        body_of(&doc),
        r#"<w:p><w:pPr><w:pStyle w:val="NoType"/></w:pPr><w:r><w:rPr><w:rStyle w:val="Rubrik1Char"/></w:rPr><w:t>a</w:t></w:r></w:p><w:p><w:pPr><w:pStyle w:val="Rubrik1"/></w:pPr></w:p>"#
    );
    // the same again: no-op; None removes
    let before = doc.to_bytes().unwrap();
    doc.set_paragraph_style(p(1), Some("Rubrik1")).unwrap();
    assert_eq!(doc.to_bytes().unwrap(), before);
    doc.set_paragraph_style(p(1), None).unwrap();
    assert!(body_of(&doc).ends_with("<w:p></w:p>"), "the emptied pPr goes (n1): {}", body_of(&doc));
    // refusals
    assert_matches!(refused(&mut doc, |d| d.set_paragraph_style(p(0), Some("Heading2"))), Error::UnknownStyle { latent_only: true, .. });
    assert_matches!(refused(&mut doc, |d| d.set_paragraph_style(p(0), Some("rubrik1x"))), Error::UnknownStyle { latent_only: false, .. });
    assert_matches!(
        refused(&mut doc, |d| d.set_run_style(r(0, 0), Some("Rubrik1"))),
        Error::StyleTypeMismatch { expected: "character", found, linked: Some(l), .. } if found == "paragraph" && l == "Rubrik1Char"
    );
    assert_matches!(refused(&mut doc, |d| d.set_paragraph_style(p(0), Some("dup"))), Error::AmbiguousStyle { .. });
    let mut doc = open("<w:p/>");
    assert_matches!(refused(&mut doc, |d| d.set_paragraph_style(p(0), Some("Normal"))), Error::UnknownStyle { latent_only: false, .. });
    assert!(doc.styles().next().is_none());
}

#[test]
fn update_fields_opt_in() {
    // inserted in slot order (after zoom, before compat), value true
    let mut doc = open_with("<w:p/>", None, Some(r#"<w:zoom w:percent="100"/><w:compat/>"#));
    doc.enable_update_fields_on_open().unwrap();
    let s = String::from_utf8(doc.package().read_part("word/settings.xml").unwrap()).unwrap();
    assert!(s.contains(r#"<w:zoom w:percent="100"/><w:updateFields w:val="true"/><w:compat/>"#), "{s}");
    let before = doc.to_bytes().unwrap();
    doc.enable_update_fields_on_open().unwrap();
    assert_eq!(doc.to_bytes().unwrap(), before);
    assert!(doc.update_fields_opted_in());
    // already on: byte-identical; off: set to true
    let mut doc = open_with("<w:p/>", None, Some(r#"<w:updateFields w:val="on"/>"#));
    let before = doc.to_bytes().unwrap();
    doc.enable_update_fields_on_open().unwrap();
    assert_eq!(doc.to_bytes().unwrap(), before);
    let mut doc = open_with("<w:p/>", None, Some(r#"<w:updateFields w:val="0"/>"#));
    doc.enable_update_fields_on_open().unwrap();
    assert!(String::from_utf8(doc.package().read_part("word/settings.xml").unwrap()).unwrap().contains(r#"<w:updateFields w:val="true"/>"#));
    // refusals
    assert_matches!(refused(&mut open("<w:p/>"), |d| d.enable_update_fields_on_open()), Error::NoSettingsPart);
    let mut doc = open_with("<w:p/>", None, Some(r#"<w:updateFields/><w:updateFields w:val="0"/>"#));
    assert_matches!(refused(&mut doc, |d| d.enable_update_fields_on_open()), Error::DuplicateSetting { .. });
    // LibreOffice's out-of-order settings (hyphenationZone after compat)
    let mut doc = open_with("<w:p/>", None, Some(r#"<w:compat/><w:hyphenationZone w:val="357"/>"#));
    assert_matches!(refused(&mut doc, |d| d.enable_update_fields_on_open()), Error::Core(CoreError::UnresolvedSibling(_)));
    assert!(!doc.update_fields_opted_in());
    // n4: settings.xml binding WML only as the default namespace refuses before inserting
    let settings = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><settings xmlns="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><zoom/></settings>"#;
    let mut doc = Document::from_bytes(package(&document_xml("<w:p/>"), &[("settings", "settings.xml", settings)])).unwrap();
    assert_matches!(refused(&mut doc, |d| d.enable_update_fields_on_open()), Error::Core(CoreError::NamespaceNotInScope(_)));
    let mut doc = open_with("<w:p/>", None, Some(r#"<w:trackRevisions/>"#));
    assert_matches!(refused(&mut doc, |d| d.enable_update_fields_on_open()), Error::TrackRevisionsOn);
    let two = package(
        &document_xml("<w:p/>"),
        &[("settings", "settings.xml", &story_xml("settings", "")), ("settings", "settings2.xml", &story_xml("settings", ""))],
    );
    assert_matches!(refused(&mut Document::from_bytes(two).unwrap(), |d| d.enable_update_fields_on_open()), Error::AmbiguousSettings);
}

#[test]
fn t9_fixtures_after_the_opt_in() {
    for stem in ["sdk-TOC2", "sdk-complex2010"] {
        let input = fixture(stem);
        let mut doc = Document::from_bytes(input.clone()).unwrap();
        assert!(doc.write_gate().is_err());
        doc.enable_update_fields_on_open().unwrap();
        for op in [Operation::Text, Operation::Append, Operation::Format, Operation::Style] {
            assert!(doc.write_gate_for(op).is_ok(), "{stem} {op:?}");
        }
        let out = doc.to_bytes().unwrap();
        // only settings.xml changed, and it has the element once
        let changed: Vec<String> = entries(&out).into_iter().zip(entries(&input)).filter(|(a, b)| a != b).map(|(a, _)| a.0).collect();
        assert_eq!(changed, ["word/settings.xml"], "{stem}");
        let s = String::from_utf8(part(&out, "word/settings.xml")).unwrap();
        assert_eq!(s.matches("updateFields").count(), 1, "{stem}");
        // the opt-in does not survive into a reopened Document: it is per Document
        assert!(Document::from_bytes(out).unwrap().write_gate().is_err(), "{stem}");
    }
}

/// T6: element children in `E`'s slot order.
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

fn every_run_format() -> Vec<RunFormat> {
    let fonts = |p: Prop<String>| ooxml_docx::FontSlots { ascii: p.clone(), h_ansi: p.clone(), east_asia: p.clone(), cs: p };
    vec![
        RunFormat {
            bold: Prop::Set(true),
            bold_cs: Prop::Set(false),
            italic: Prop::Set(true),
            italic_cs: Prop::Set(true),
            strike: Prop::Set(false),
            underline: Prop::Set(Underline::Single),
            color: Prop::Set(Color::Rgb(0x12, 0x34, 0x56)),
            size: Prop::Set(HalfPoints::new(28).unwrap()),
            size_cs: Prop::Set(HalfPoints::new(30).unwrap()),
            highlight: Prop::Set(Highlight::Green),
            vert_align: Prop::Set(VertAlign::Subscript),
            fonts: fonts(Prop::Set("Courier New".into())),
        },
        RunFormat {
            bold: Prop::Clear,
            bold_cs: Prop::Clear,
            italic: Prop::Clear,
            italic_cs: Prop::Clear,
            strike: Prop::Clear,
            underline: Prop::Clear,
            color: Prop::Clear,
            size: Prop::Clear,
            size_cs: Prop::Clear,
            highlight: Prop::Clear,
            vert_align: Prop::Clear,
            fonts: fonts(Prop::Clear),
        },
    ]
}

/// T2 + T6 + T4 over the fixtures: every setter (all Set, then all Clear)
/// on every formattable run of every story (up to 200 per fixture), and the
/// paragraph setters on every formattable paragraph. Each edit changes the
/// story part only inside that run's or paragraph's span, children stay in
/// slot order, a repeated Set is byte-identical, and a refusal changes
/// nothing. Finally the package reopens with the same texts.
#[test]
fn t2_t6_setters_over_the_fixtures() {
    let pf = ParagraphFormat {
        alignment: Prop::Set(Alignment::Right),
        space_before: Prop::Set(60),
        space_after: Prop::Set(60),
        line_spacing: Prop::Set(LineSpacing::Exact(300)),
        indent_left: Prop::Set(100),
        indent_right: Prop::Set(-100),
        first_line: Prop::Set(FirstLine::Indent(200)),
        keep_next: Prop::Set(true),
        keep_lines: Prop::Set(false),
    };
    let (mut ok, mut refusals) = (0usize, 0usize);
    for stem in writable() {
        let input = fixture(stem);
        let mut doc = Document::from_bytes(input.clone()).unwrap();
        let texts: Vec<Vec<String>> = doc.stories().map(|s| s.paragraphs().map(|p| p.text()).collect()).collect();
        let runs: Vec<(RunRef, String)> = doc
            .stories()
            .flat_map(|s| {
                let part = s.id().part().to_owned();
                s.paragraphs().flat_map(move |p| p.runs().map(|r| (r.run_ref(), part.clone())).collect::<Vec<_>>()).collect::<Vec<_>>()
            })
            .take(200)
            .collect();
        for (at, part) in runs {
            for f in every_run_format() {
                let before = doc.package().read_part(&part).unwrap();
                match doc.format_run(at, &f) {
                    Ok(()) => {
                        ok += 1;
                        let after = doc.package().read_part(&part).unwrap();
                        if let Some((s, e, _)) = diff_range(&before, &after) {
                            let (lo, hi) = locate(&before, &doc, at);
                            assert!(s >= lo && e <= hi, "{stem} {at:?} T2: {s}..{e} outside {lo}..{hi}");
                        }
                        let run = doc.run_at(at).unwrap();
                        assert!(in_slot_order::<wml::Run>(run.raw()), "{stem} T6 run");
                        if let Some(rpr) = run.raw().elements().find(|e| e.name().local() == "rPr") {
                            assert!(in_slot_order::<wml::RunProperties>(rpr), "{stem} T6 rPr");
                        }
                        let again = doc.package().read_part(&part).unwrap();
                        doc.format_run(at, &f).unwrap();
                        assert_eq!(doc.package().read_part(&part).unwrap(), again, "{stem} {at:?} repeated Set is a no-op");
                    }
                    Err(_) => {
                        refusals += 1;
                        assert_eq!(doc.package().read_part(&part).unwrap(), before, "{stem} Err ⇒ byte-identical");
                    }
                }
            }
        }
        let paras: Vec<ParaRef> = doc.stories().flat_map(|s| s.paragraphs().map(|p| p.para_ref()).collect::<Vec<_>>()).take(200).collect();
        for at in paras {
            let part = doc.story_at(at.story()).unwrap().id().part().to_owned();
            let before = doc.package().read_part(&part).unwrap();
            match doc.format_paragraph(at, &pf) {
                Ok(()) => {
                    ok += 1;
                    let para = doc.paragraph_at(at).unwrap();
                    assert!(in_slot_order::<wml::Paragraph>(para.raw()), "{stem} T6 p");
                    if let Some(ppr) = para.raw().elements().find(|e| e.name().local() == "pPr") {
                        assert!(in_slot_order::<wml::ParagraphProperties>(ppr), "{stem} T6 pPr");
                    }
                }
                Err(_) => {
                    refusals += 1;
                    assert_eq!(doc.package().read_part(&part).unwrap(), before, "{stem} Err ⇒ byte-identical");
                }
            }
        }
        let re = Document::from_bytes(doc.to_bytes().unwrap()).unwrap();
        let after: Vec<Vec<String>> = re.stories().map(|s| s.paragraphs().map(|p| p.text()).collect()).collect();
        assert_eq!(after, texts, "{stem}: texts unchanged");
        // styles.xml is never written
        if let Some(name) = entries(&input).iter().map(|e| e.0.clone()).find(|n| n.ends_with("styles.xml")) {
            assert_eq!(part(&doc.to_bytes().unwrap(), &name), part(&input, &name), "{stem}");
        }
    }
    eprintln!("T2/T6 setters: {ok} writes, {refusals} refusals");
    assert!(ok > 1000);
}

/// The byte range of a run in the current serialization `bytes` of its
/// part: located through its text and properties is fragile, so the run's
/// ordinal among the part's `w:r` start tags is used.
fn locate(bytes: &[u8], doc: &Document, at: RunRef) -> (usize, usize) {
    let root = doc.story_at(at.para().story()).unwrap().raw();
    let target = doc.run_at(at).unwrap().raw() as *const RawNode;
    let mut stack = vec![root];
    let mut ord = 0usize;
    let mut found = None;
    while let Some(n) = stack.pop() {
        if n.name().local() == "r" && n.name().ns() == W_NS {
            if std::ptr::eq(n, target) {
                found = Some(ord);
                break;
            }
            ord += 1;
        }
        let kids: Vec<&RawNode> = n.elements().collect();
        stack.extend(kids.into_iter().rev());
    }
    let ord = found.expect("run in part");
    let s = std::str::from_utf8(bytes).unwrap();
    let doc = roxmltree::Document::parse(s).unwrap();
    let node = doc.descendants().filter(|n| n.is_element() && n.tag_name().namespace() == Some(W_NS) && n.tag_name().name() == "r").nth(ord).unwrap();
    (node.range().start, node.range().end)
}

/// T5: docx-rs reads the written formatting and styles back.
#[test]
fn t5_docx_rs_reads_formatting_back() {
    let styles = r#"<w:style w:type="paragraph" w:styleId="Title"><w:name w:val="Title"/></w:style><w:style w:type="character" w:styleId="Strong"><w:name w:val="Strong"/></w:style>"#;
    let mut doc = open_with("<w:p><w:r><w:t>a</w:t></w:r></w:p>", Some(styles), None);
    doc.format_run(r(0, 0), &every_run_format()[0]).unwrap();
    doc.set_run_style(r(0, 0), Some("Strong")).unwrap();
    doc.set_paragraph_style(p(0), Some("Title")).unwrap();
    doc.format_paragraph(p(0), &ParagraphFormat { alignment: Prop::Set(Alignment::Center), indent_left: Prop::Set(240), ..Default::default() }).unwrap();
    let json = docx_rs::read_docx(&doc.to_bytes().unwrap()).unwrap().json();
    let v: Value = serde_json::from_str(&json).unwrap();
    let para = &v["document"]["children"][0]["data"];
    let rp = &para["children"][0]["data"]["runProperty"];
    assert_eq!(rp["bold"], true);
    assert_eq!(rp["italic"], true);
    assert_eq!(rp["sz"], 28);
    assert_eq!(rp["color"], "123456");
    assert_eq!(rp["highlight"], "green");
    assert_eq!(rp["underline"], "single");
    assert_eq!(rp["style"], "Strong");
    let pp = &para["property"];
    assert_eq!(pp["style"], "Title");
    assert_eq!(pp["alignment"], "center");
    assert_eq!(pp["indent"]["start"], 240, "{pp}");
}
