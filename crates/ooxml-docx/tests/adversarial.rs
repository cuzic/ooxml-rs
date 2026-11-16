//! Adversarial probes from the ADR-0004 round-2 review (P01–P12), turned
//! into assertions, plus the tests of the fixes (N2, m1, m2, m3, m6).

mod common;

use std::assert_matches;
use std::io::{Cursor, Write};

use common::*;
use ooxml_docx::fields::{CALCULATED_FORM_FIELD, MALFORMED};
use ooxml_docx::{Document, Error, RunContext, WriteProtection};

fn main_xml(doc: &Document) -> String {
    String::from_utf8(doc.package().read_part("word/document.xml").unwrap()).unwrap()
}

fn body_of(s: &str) -> String {
    let a = s.find("<w:body>").map_or(0, |i| i + 8);
    let b = s.rfind("</w:body>").unwrap_or(s.len());
    s[a..b].to_owned()
}

fn raw_doc(document_xml: &str) -> Vec<u8> {
    with_part(&docx("<w:p/>", None, None), "word/document.xml", document_xml.as_bytes())
}

fn reopen(doc: &Document) -> Document {
    Document::from_bytes(doc.to_bytes().unwrap()).unwrap()
}

/// A complex field with `begin` as written.
fn fld_raw(begin: &str, code: &str) -> String {
    format!(
        r#"<w:r>{begin}</w:r><w:r><w:instrText xml:space="preserve"> {code} </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>RESULT</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r>"#
    )
}

fn fld(code: &str) -> String {
    fld_raw(r#"<w:fldChar w:fldCharType="begin"/>"#, code)
}

fn refused_codes(bytes: Vec<u8>) -> Vec<String> {
    match Document::from_bytes(bytes).unwrap().write_gate() {
        Ok(()) => vec![],
        Err(Error::StaleFieldRisk { codes }) => codes,
        Err(e) => panic!("{e}"),
    }
}

fn body_codes(body: &str) -> Vec<String> {
    refused_codes(docx(body, None, None))
}

/// Adds parts (and package-level relationships) to a package.
fn with_parts(bytes: &[u8], parts: &[(&str, &str)], package_rels: &str) -> Vec<u8> {
    let rels = String::from_utf8(part(bytes, "_rels/.rels")).unwrap().replace("</Relationships>", &format!("{package_rels}</Relationships>"));
    let bytes = with_part(bytes, "_rels/.rels", rels.as_bytes());
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..z.len() {
        w.raw_copy_file(z.by_index_raw(i).unwrap()).unwrap();
    }
    for (name, content) in parts {
        w.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
        w.write_all(content.as_bytes()).unwrap();
    }
    w.finish().unwrap().into_inner()
}

const CUSTOM_REL: &str = r#"<Relationship Id="rId9" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/custom-properties" Target="docProps/custom.xml"/>"#;

fn custom_xml(props: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/custom-properties" xmlns:vt="http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes">{props}</Properties>"#
    )
}

// ---- N2: classification by argument -------------------------------------

/// P04 and the audit of the safe codes.
#[test]
fn n2_statistics_and_argument_dependent_fields_are_refused() {
    for (code, key) in [
        ("INFO NumWords", "INFO NUMWORDS"),
        ("INFO NumChars", "INFO NUMCHARS"),
        ("INFO FileSize", "INFO FILESIZE"),
        ("DOCPROPERTY Words", "DOCPROPERTY WORDS"),
        ("DOCPROPERTY \"Characters\"", "DOCPROPERTY CHARACTERS"),
        ("DOCPROPERTY CharactersWithSpaces \\* MERGEFORMAT", "DOCPROPERTY CHARACTERSWITHSPACES"),
        ("DOCPROPERTY Lines", "DOCPROPERTY LINES"),
        ("DOCPROPERTY Paragraphs", "DOCPROPERTY PARAGRAPHS"),
        ("DOCPROPERTY Bytes", "DOCPROPERTY BYTES"),
        ("DOCPROPERTY \\* MERGEFORMAT", "DOCPROPERTY"),
        ("INFO", "INFO"),
        ("FILESIZE", "FILESIZE"),
        ("NUMWORDS", "NUMWORDS"),
        ("BARCODE \\b Address \\u", "BARCODE \\b"),
    ] {
        assert_eq!(body_codes(&format!("<w:p>{}</w:p><w:p><w:r><w:t>body</w:t></w:r></w:p>", fld(code))), [key], "{code}");
    }
    for code in ["INFO NumPages", "INFO Author", "DOCPROPERTY Title", "DOCPROPERTY Pages", "DOCPROPERTY \"Client Name\"", "BARCODE \"98052\" \\u"] {
        assert_eq!(body_codes(&format!("<w:p>{}</w:p>", fld(code))), Vec::<String>::new(), "{code}");
    }
    // fldSimple goes through the same key.
    assert_eq!(body_codes(r#"<w:p><w:fldSimple w:instr=" INFO NumWords "/></w:p>"#), ["INFO NUMWORDS"]);
    // A nested field as the argument: no literal argument, refused.
    let nested = r#"<w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText>DOCPROPERTY </w:instrText></w:r><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText>MERGEFIELD p</w:instrText></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p>"#;
    assert_eq!(body_codes(nested), ["DOCPROPERTY"]);
}

#[test]
fn n2_calculated_form_field_is_refused() {
    let begin = |ty: &str| format!(r#"<w:fldChar w:fldCharType="begin"><w:ffData><w:name w:val="Total"/><w:textInput><w:type w:val="{ty}"/><w:default w:val="=a+b"/></w:textInput></w:ffData></w:fldChar>"#);
    assert_eq!(body_codes(&format!("<w:p>{}</w:p>", fld_raw(&begin("calculated"), "FORMTEXT"))), [CALCULATED_FORM_FIELD]);
    assert_eq!(body_codes(&format!("<w:p>{}</w:p>", fld_raw(&begin("regular"), "FORMTEXT"))), Vec::<String>::new());
}

#[test]
fn n2_docproperty_of_a_linked_custom_property_is_refused() {
    let props = custom_xml(
        r#"<property fmtid="{D5CDD505-2E9C-101B-9397-08002B2CF9AE}" pid="2" name="Client" linkTarget="_ClientBookmark"><vt:lpwstr>ACME</vt:lpwstr></property><property fmtid="{D5CDD505-2E9C-101B-9397-08002B2CF9AE}" pid="3" name="Project"><vt:lpwstr>X</vt:lpwstr></property>"#,
    );
    let body = format!("<w:p>{}{}</w:p>", fld("DOCPROPERTY Client"), fld("DOCPROPERTY \"Project\""));
    let bytes = with_parts(&docx(&body, None, None), &[("docProps/custom.xml", &props)], CUSTOM_REL);
    assert_eq!(refused_codes(bytes), ["DOCPROPERTY CLIENT"]);
}

// ---- m1: field markup that cannot be followed fails closed --------------

/// P03: missing, unqualified or mis-cased `fldCharType`.
#[test]
fn m1_unknown_fld_char_type_is_malformed() {
    for begin in [r#"<w:fldChar/>"#, r#"<w:fldChar fldCharType="begin"/>"#, r#"<w:fldChar w:fldCharType="Begin"/>"#] {
        let codes = body_codes(&format!("<w:p>{}</w:p>", fld_raw(begin, "PAGE")));
        assert!(codes.contains(&MALFORMED.to_owned()), "{begin}: {codes:?}");
    }
    // instrText, separate or end with no open field.
    for stray in [r#"<w:r><w:instrText>TOC</w:instrText></w:r>"#, r#"<w:r><w:fldChar w:fldCharType="separate"/></w:r>"#, r#"<w:r><w:fldChar w:fldCharType="end"/></w:r>"#] {
        assert_eq!(body_codes(&format!("<w:p>{stray}<w:r><w:t>a</w:t></w:r></w:p>")), [MALFORMED], "{stray}");
    }
}

/// P07: a field begun inside a paragraph-level `mc:AlternateContent` is
/// malformed; a complete field in a text box inside one is classified
/// normally.
#[test]
fn m1_fields_in_paragraph_level_alternate_content() {
    let ac = r#"<w:p><mc:AlternateContent><mc:Choice Requires="w14"><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText>PAGE</w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r></mc:Choice><mc:Fallback><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText>PAGE</w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r></mc:Fallback></mc:AlternateContent><w:r><w:t>result</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p>"#;
    assert!(body_codes(ac).contains(&MALFORMED.to_owned()));
    let textbox = |code: &str| {
        format!(
            r#"<w:p><w:r><w:t>x</w:t></w:r><w:r><mc:AlternateContent><mc:Choice Requires="wps"><w:drawing><w:txbxContent><w:p>{}</w:p></w:txbxContent></w:drawing></mc:Choice><mc:Fallback><w:pict/></mc:Fallback></mc:AlternateContent></w:r></w:p>"#,
            fld(code)
        )
    };
    assert_eq!(body_codes(&textbox("REF a")), ["REF"]);
    assert_eq!(body_codes(&textbox("PAGE")), Vec::<String>::new());
    // Block-level AlternateContent around whole paragraphs is fine.
    let block = format!(r#"<mc:AlternateContent><mc:Choice Requires="w14"><w:p>{}</w:p></mc:Choice></mc:AlternateContent>"#, fld("PAGE"));
    assert_eq!(body_codes(&block), Vec::<String>::new());
}

// ---- m2: the last section's sectPr in the last paragraph ----------------

/// P01: append is refused (A2) instead of starting a new, default section.
#[test]
fn m2_append_after_paragraph_level_last_section_is_refused() {
    let last = r#"<w:p><w:pPr><w:sectPr><w:pgSz w:w="16838" w:h="11906" w:orient="landscape"/></w:sectPr></w:pPr></w:p>"#;
    for body in [
        format!(r#"<w:p><w:r><w:t>last</w:t></w:r></w:p>{last}"#),
        format!(r#"<w:p><w:r><w:t>last</w:t></w:r></w:p>{last}<w:bookmarkEnd w:id="0"/>"#),
        format!(r#"<w:sdt><w:sdtContent><w:p><w:r><w:t>a</w:t></w:r></w:p>{last}</w:sdtContent></w:sdt>"#),
    ] {
        let input = docx(&body, None, None);
        let mut doc = Document::from_bytes(input.clone()).unwrap();
        assert_matches!(doc.append_paragraph("NEW"), Err(Error::SectionBreakAtEnd), "{body}");
        assert_eq!(entries(&doc.to_bytes().unwrap()), entries(&input));
    }
    // Allowed: a body-level sectPr exists (the paragraph ends an earlier
    // section), or a table follows the section break, or no sectPr at all.
    for body in [
        format!(r#"<w:p><w:r><w:t>a</w:t></w:r></w:p>{last}<w:sectPr/>"#),
        format!(r#"{last}<w:tbl><w:tr><w:tc><w:p/></w:tc></w:tr></w:tbl>"#),
        r#"<w:p><w:r><w:t>a</w:t></w:r></w:p>"#.to_owned(),
    ] {
        let mut doc = Document::from_bytes(docx(&body, None, None)).unwrap();
        doc.append_paragraph("NEW").unwrap_or_else(|e| panic!("{body}: {e}"));
    }
}

// ---- m3: protection signals that do not gate, and duplicates ------------

/// P05.
#[test]
fn m3_protection_variants() {
    let open = |s: &str| Document::from_bytes(docx("<w:p><w:r><w:t>x</w:t></w:r></w:p>", Some(s), None)).unwrap();
    let d = open(r#"<w:writeProtection w:recommended="1"/>"#);
    assert!(d.write_gates().is_empty());
    assert_eq!(d.write_protection().map(|w| (w.recommended, w.has_password)), Some((true, false)));
    let d = open(r#"<w:writeProtection w:cryptProviderType="rsaAES" w:cryptAlgorithmClass="hash" w:cryptAlgorithmType="typeAny" w:cryptAlgorithmSid="14" w:cryptSpinCount="100000" w:hash="AAAA" w:salt="BBBB"/>"#);
    assert!(d.write_gates().is_empty());
    assert_eq!(d.write_protection().map(|w| (w.recommended, w.has_password)), Some((false, true)));
    assert_eq!(open("").write_protection(), None::<WriteProtection>);
    // Duplicates: any occurrence that is on, is on.
    for s in [
        r#"<w:documentProtection w:edit="none"/><w:documentProtection w:edit="readOnly" w:enforcement="1"/>"#,
        r#"<w:documentProtection w:edit="readOnly" w:enforcement="1"/><w:documentProtection w:edit="none"/>"#,
    ] {
        assert_matches!(open(s).write_gate(), Err(Error::DocumentProtected { edit }) if edit == "readOnly", "{s}");
    }
    assert_matches!(open(r#"<w:trackRevisions w:val="0"/><w:trackRevisions/>"#).write_gate(), Err(Error::TrackRevisionsOn));
    // Unqualified attributes are not w:edit / w:enforcement.
    assert!(open(r#"<w:documentProtection edit="readOnly" enforcement="1"/>"#).write_gate().is_ok());
    for s in [r#"<w:documentProtection w:edit="readOnly" w:enforcement="True"/>"#, r#"<w:documentProtection w:edit="readOnly" w:enforcement=" 1 "/>"#] {
        assert!(open(s).write_gate().is_err(), "{s}");
    }
}

#[test]
fn m3_mark_as_final_is_reported() {
    let base = docx("<w:p><w:r><w:t>x</w:t></w:r></w:p>", None, None);
    assert!(!Document::from_bytes(base.clone()).unwrap().is_marked_final());
    let fin = custom_xml(r#"<property fmtid="{D5CDD505-2E9C-101B-9397-08002B2CF9AE}" pid="2" name="_MarkAsFinal"><vt:bool>true</vt:bool></property>"#);
    let d = Document::from_bytes(with_parts(&base, &[("docProps/custom.xml", &fin)], CUSTOM_REL)).unwrap();
    assert!(d.is_marked_final());
    assert!(d.write_gate().is_ok(), "reported, not a gate");
    let not = fin.replace("true", "false");
    assert!(!Document::from_bytes(with_parts(&base, &[("docProps/custom.xml", &not)], CUSTOM_REL)).unwrap().is_marked_final());
}

// ---- m6: no-op and xml:space --------------------------------------------

/// P10.
#[test]
fn m6_noop_requires_needed_preserve() {
    let body = r#"<w:p><w:r><w:t> a</w:t></w:r><w:r><w:t xml:space="preserve"> b</w:t></w:r><w:r><w:t xml:space="default">c</w:t></w:r></w:p>"#;
    let mut d = Document::from_bytes(docx(body, None, None)).unwrap();
    d.set_run_text(0, 0, " a").unwrap();
    d.set_run_text(0, 1, " b").unwrap();
    d.set_run_text(0, 2, "c").unwrap();
    assert_eq!(
        body_of(&main_xml(&d)),
        r#"<w:p><w:r><w:t xml:space="preserve"> a</w:t></w:r><w:r><w:t xml:space="preserve"> b</w:t></w:r><w:r><w:t xml:space="default">c</w:t></w:r></w:p>"#
    );
    // Runs 1 and 2 were no-ops: the first run is the only change.
    let mut d = Document::from_bytes(docx(body, None, None)).unwrap();
    let before = main_xml(&d);
    d.set_run_text(0, 1, " b").unwrap();
    d.set_run_text(0, 2, "c").unwrap();
    assert_eq!(main_xml(&d), before);
    // Edge whitespace that is not U+0020 does not need preserve.
    d.set_run_text(0, 2, "\u{3000}c\u{a0}").unwrap();
    assert!(body_of(&main_xml(&d)).contains("<w:t xml:space=\"default\">\u{3000}c\u{a0}</w:t>"));
}

// ---- regression probes that passed in round 2 ---------------------------

/// P02: prefix variants.
#[test]
fn p02_default_namespace_and_prefix_variants() {
    let w = W_NS;
    let x = format!(r#"<?xml version="1.0"?><document xmlns="{w}"><body><p><r><rPr><b/></rPr><t>a</t></r></p><sectPr/></body></document>"#);
    let mut d = Document::from_bytes(raw_doc(&x)).unwrap();
    d.set_run_text(0, 0, " a\tb").unwrap();
    d.append_paragraph("z\nq").unwrap();
    assert_eq!(reopen(&d).paragraphs().map(|p| p.text()).collect::<Vec<_>>(), [" a\tb", "z\nq"]);
    assert!(main_xml(&d).contains(r#"<r><rPr><b/></rPr><t xml:space="preserve"> a</t><tab/><t>b</t></r>"#), "{}", main_xml(&d));

    let x = format!(r#"<?xml version="1.0"?><x:document xmlns:x="{w}" xmlns:w="urn:not-wml"><x:body><x:p><x:r><x:t>a</x:t></x:r></x:p><x:sectPr/></x:body></x:document>"#);
    let mut d = Document::from_bytes(raw_doc(&x)).unwrap();
    d.set_run_text(0, 0, "a\tb").unwrap();
    d.append_paragraph("z").unwrap();
    assert_eq!(reopen(&d).paragraphs().map(|p| p.text()).collect::<Vec<_>>(), ["a\tb", "z"]);

    let x = format!(r#"<?xml version="1.0"?><w:document xmlns:w="{w}"><w:body><w:p xmlns:w="{w}" xmlns="urn:other"><w:r><w:t>a</w:t></w:r></w:p></w:body></w:document>"#);
    let mut d = Document::from_bytes(raw_doc(&x)).unwrap();
    d.set_run_text(0, 0, "a\tb").unwrap();
    assert_eq!(reopen(&d).paragraph(0).unwrap().text(), "a\tb");
}

/// P06: runs around proofErr/bookmarks, empty runs, rPr after content,
/// awkward characters.
#[test]
fn p06_surroundings_of_edited_runs() {
    let body = r#"<w:p w14:paraId="1" xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml"><w:proofErr w:type="spellStart"/><w:bookmarkStart w:id="0" w:name="b"/><w:r><w:rPr><w:b/></w:rPr><w:lastRenderedPageBreak/><w:t xml:space="preserve">Helo </w:t></w:r><w:bookmarkEnd w:id="0"/><w:proofErr w:type="spellEnd"/><w:r><w:rPr><w:i/></w:rPr></w:r><w:r></w:r><w:r/><w:r><w:t/></w:r><w:r><w:t>a</w:t><w:rPr><w:b/></w:rPr></w:r><w:r><w:tab/><w:t>x</w:t><w:cr/><w:t xml:space="preserve"> y</w:t></w:r></w:p>"#;
    let mut d = Document::from_bytes(docx(body, None, None)).unwrap();
    let texts = ["Hello", "x", "y", "z", "", "b\r\n", "]]> & <\u{85}\u{2028}\u{7f}"];
    for (r, t) in texts.iter().enumerate() {
        d.set_run_text(0, r, t).unwrap_or_else(|e| panic!("run {r}: {e}"));
    }
    let body = body_of(&main_xml(&d));
    assert!(body.contains(r#"<w:proofErr w:type="spellStart"/><w:bookmarkStart w:id="0" w:name="b"/><w:r><w:rPr><w:b/></w:rPr><w:t>Hello</w:t></w:r><w:bookmarkEnd w:id="0"/>"#), "{body}");
    // rPr after content stays where it was; empty runs get content after rPr.
    assert!(body.contains(r#"<w:r><w:rPr><w:i/></w:rPr><w:t>x</w:t></w:r><w:r><w:t>y</w:t></w:r><w:r><w:t>z</w:t></w:r><w:r></w:r><w:r><w:rPr><w:b/></w:rPr><w:t>b</w:t><w:br/></w:r>"#), "{body}");
    assert!(body.contains("]]&gt; &amp; &lt;"), "{body}");
    let r = reopen(&d);
    let got: Vec<String> = r.paragraph(0).unwrap().runs().map(|r| r.text()).collect();
    assert_eq!(got, ["Hello", "x", "y", "z", "", "b\n", "]]> & <\u{85}\u{2028}\u{7f}"]);
}

/// P09 + P11 + P12: field state across wrappers and refusal order.
#[test]
fn p09_p11_p12_field_state() {
    let body = r#"<w:tbl><w:tr><w:tc><w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText>PAGE</w:instrText></w:r></w:p></w:tc></w:tr></w:tbl><w:p><w:r><w:t>after</w:t></w:r></w:p><w:sectPr/>"#;
    let mut d = Document::from_bytes(docx(body, None, None)).unwrap();
    assert_matches!(d.append_paragraph("x"), Err(Error::InsideField));
    assert_matches!(d.paragraph(1).unwrap().run(0).unwrap().check_settable(), Err(Error::InsideField));

    let body = r#"<w:p><w:hyperlink w:anchor="a"><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText>PAGE</w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>1</w:t></w:r></w:hyperlink><w:r><w:t>outside-before-end</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r><w:r><w:t>free</w:t></w:r></w:p>"#;
    let d = Document::from_bytes(docx(body, None, None)).unwrap();
    let got: Vec<(String, bool)> = d.paragraph(0).unwrap().runs().map(|r| (r.text(), r.check_settable().is_ok())).collect();
    assert_eq!(got.iter().filter(|(_, ok)| *ok).map(|(t, _)| t.as_str()).collect::<Vec<_>>(), ["free"]);

    let body = r#"<w:p><w:smartTag w:element="x"><w:ins w:id="1" w:author="a"><w:r><w:fldChar w:fldCharType="begin"/></w:r></w:ins></w:smartTag></w:p>"#;
    let d = Document::from_bytes(docx(body, None, None)).unwrap();
    assert_matches!(d.paragraph(0).unwrap().run(0).unwrap().check_settable(), Err(Error::InsideField));
    assert_eq!(d.paragraph(0).unwrap().run(0).unwrap().context(), RunContext::SmartTag);
}
