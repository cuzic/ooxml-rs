//! Unit tests of every gate and per-run refusal on small synthetic packages
//! (ADR-0004 §6, T4, review round 1 M1, M3, m2, m6, m9).

mod common;

use std::assert_matches;

use common::*;
use ooxml_core::Error as CoreError;
use ooxml_docx::{Document, Error, RunContext};

fn open(body: &str, settings: Option<&str>, header: Option<&str>) -> Document {
    Document::from_bytes(docx(body, settings, header)).unwrap()
}

fn gate(body: &str, settings: Option<&str>, header: Option<&str>) -> Result<(), Error> {
    open(body, settings, header).write_gate()
}

const P: &str = "<w:p><w:r><w:t>Hello</w:t></w:r></w:p>";

fn fld(code: &str) -> String {
    format!(
        r#"<w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText xml:space="preserve"> {code} </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>result</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r>"#
    )
}

/// m6: every ST_OnOff spelling of w:trackRevisions, plus an invalid value
/// (fail closed) and the empty element (needs the CT_OnOff override).
#[test]
fn track_revisions_values() {
    assert!(gate(P, None, None).is_ok(), "no settings part");
    assert!(gate(P, Some(""), None).is_ok(), "no trackRevisions");
    for on in [r#"<w:trackRevisions/>"#, r#"<w:trackRevisions w:val="true"/>"#, r#"<w:trackRevisions w:val="1"/>"#, r#"<w:trackRevisions w:val="on"/>"#, r#"<w:trackRevisions w:val="bogus"/>"#] {
        assert_matches!(gate(P, Some(on), None), Err(Error::TrackRevisionsOn), "{on}");
    }
    for off in [r#"<w:trackRevisions w:val="false"/>"#, r#"<w:trackRevisions w:val="0"/>"#, r#"<w:trackRevisions w:val="off"/>"#] {
        assert!(gate(P, Some(off), None).is_ok(), "{off}");
    }
}

/// M3: enforced protection with a restriction refuses writes.
#[test]
fn document_protection() {
    let prot = |a: &str| format!("<w:documentProtection {a}/>");
    for (attrs, edit) in [
        (r#"w:edit="readOnly" w:enforcement="1""#, "readOnly"),
        (r#"w:edit="comments" w:enforcement="true""#, "comments"),
        (r#"w:edit="forms" w:enforcement="on""#, "forms"),
        (r#"w:edit="trackedChanges" w:enforcement="1""#, "trackedChanges"),
        (r#"w:edit="somethingNew" w:enforcement="1""#, "somethingNew"),
        (r#"w:edit="readOnly" w:enforcement="bogus""#, "readOnly"),
    ] {
        assert_matches!(gate(P, Some(&prot(attrs)), None), Err(Error::DocumentProtected { edit: e }) if e == edit, "{attrs}");
    }
    for attrs in [r#"w:edit="readOnly" w:enforcement="0""#, r#"w:edit="readOnly""#, r#"w:edit="none" w:enforcement="1""#, r#"w:enforcement="1""#, r#"w:formatting="1" w:enforcement="1""#] {
        assert!(gate(P, Some(&prot(attrs)), None).is_ok(), "{attrs}");
    }
    // Gate order: W1, W3, W2; all are reported by write_gates.
    let doc = open(&format!("<w:p>{}</w:p>", fld("TOC")), Some(r#"<w:trackRevisions/><w:documentProtection w:edit="forms" w:enforcement="1"/>"#), None);
    assert_matches!(doc.write_gate(), Err(Error::TrackRevisionsOn));
    let all: Vec<String> = doc.write_gates().iter().map(|e| format!("{e:?}")).collect();
    assert_eq!(all.len(), 3, "{all:?}");
}

fn codes(body: &str, header: Option<&str>) -> Vec<String> {
    match gate(body, None, header) {
        Ok(()) => vec![],
        Err(Error::StaleFieldRisk { codes }) => codes,
        Err(e) => panic!("{e}"),
    }
}

/// M1: nested fields; every level is classified with a stack.
#[test]
fn nested_fields_are_classified_at_every_level() {
    // Mail-merge rule: { IF { MERGEFIELD Gender } = "F" "Ms." "Mr." }
    let if_merge = r#"<w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText xml:space="preserve"> IF </w:instrText></w:r><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText xml:space="preserve"> MERGEFIELD Gender </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:instrText>F</w:instrText></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r><w:r><w:instrText xml:space="preserve"> = "F" "Ms." "Mr." </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>Ms.</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p>"#;
    assert_eq!(codes(if_merge, None), ["IF"]);
    // { = { PAGE } - 1 }
    let formula = r#"<w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText xml:space="preserve"> = </w:instrText></w:r><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText>PAGE</w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:instrText>3</w:instrText></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r><w:r><w:instrText xml:space="preserve"> - 1 </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>2</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p>"#;
    assert_eq!(codes(formula, None), ["="]);
    // { HYPERLINK "{ REF url }" }: the outer code is safe, the inner is not.
    let link_ref = r#"<w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText>HYPERLINK "</w:instrText></w:r><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText>REF url</w:instrText></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r><w:r><w:instrText>"</w:instrText></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p>"#;
    assert_eq!(codes(link_ref, None), ["REF"]);
    // A nested field's result, written as instrText after its separate,
    // stays out of the outer code: { PAGE { DATE } } with result "TOC".
    let leak = r#"<w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText>PAGE </w:instrText></w:r><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText>DATE</w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:instrText>TOC</w:instrText></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p>"#;
    assert_eq!(codes(leak, None), Vec::<String>::new());
}

#[test]
fn field_classification() {
    for safe in ["PAGE", "NUMPAGES", "DATE \\@ \"d MMMM yyyy\"", "HYPERLINK \"https://example.com\"", "MERGEFIELD name", "AUTHOR", "FORMTEXT"] {
        assert_eq!(codes(&format!("<w:p>{}</w:p>", fld(safe)), None), Vec::<String>::new(), "{safe}");
    }
    for (code, refused) in [("TOC \\o \"1-3\"", "TOC"), ("REF _Ref1 \\h", "REF"), ("pageref x", "PAGEREF"), ("SEQ Figure", "SEQ"), ("=2+2", "="), ("M:USERCONTENT", "M:USERCONTENT"), ("AUTONUM", "AUTONUM"), ("", "")] {
        assert_eq!(codes(&format!("<w:p>{}</w:p>", fld(code)), None), [refused], "{code}");
    }
    // fldSimple, header stories, unterminated fields, revised instructions.
    assert_eq!(codes(r#"<w:p><w:fldSimple w:instr=" NOTEREF x "><w:r><w:t>1</w:t></w:r></w:fldSimple></w:p>"#, None), ["NOTEREF"]);
    assert_eq!(codes(P, Some(&format!("<w:p>{}</w:p>", fld("STYLEREF Heading")))), ["STYLEREF"]);
    assert_eq!(codes(P, Some(&format!("<w:p>{}</w:p>", fld("PAGE")))), Vec::<String>::new());
    assert_eq!(codes(r#"<w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText>SEQ x</w:instrText></w:r></w:p>"#, None), ["SEQ"]);
    let revised = r#"<w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:del w:id="1" w:author="a"><w:r><w:delInstrText>PAGE</w:delInstrText></w:r></w:del><w:r><w:instrText>REF x</w:instrText></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p>"#;
    assert_eq!(codes(revised, None), [ooxml_docx::fields::REVISED_INSTRUCTION]);
}

/// m2: field state runs over the whole body in document order.
#[test]
fn inside_field_across_paragraphs_and_wrappers() {
    // A field whose result spans paragraphs (like a TOC), and whose last
    // result run sits in a hyperlink of a later paragraph.
    let body = r#"<w:p><w:r><w:t>before</w:t></w:r><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText>PAGE</w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>r1</w:t></w:r></w:p><w:p><w:r><w:t>r2</w:t></w:r></w:p><w:tbl><w:tr><w:tc><w:p><w:hyperlink w:anchor="a"><w:r><w:t>r3</w:t></w:r></w:hyperlink><w:r><w:fldChar w:fldCharType="end"/></w:r><w:r><w:t>after</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#;
    let doc = open(body, None, None);
    let runs: Vec<(String, bool, bool)> = doc
        .paragraphs()
        .flat_map(|p| p.runs().map(|r| (r.text(), r.in_field(), r.check_settable().is_ok())).collect::<Vec<_>>())
        .filter(|(t, ..)| !t.is_empty())
        .collect();
    let expect = |t: &str, inside: bool| (t.to_string(), inside, !inside);
    assert_eq!(runs, [expect("before", false), expect("r1", true), expect("r2", true), expect("r3", true), expect("after", false)]);
    let mut doc = doc;
    assert_matches!(doc.set_run_text(1, 0, "x"), Err(Error::InsideField));
    // Unterminated begin: the rest of the body is inside the field (fail
    // closed), and append is refused.
    let mut doc = open(r#"<w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText>PAGE</w:instrText></w:r></w:p><w:p><w:r><w:t>x</w:t></w:r></w:p>"#, None, None);
    assert_matches!(doc.paragraph(1).unwrap().run(0).unwrap().check_settable(), Err(Error::InsideField));
    assert_matches!(doc.append_paragraph("y"), Err(Error::InsideField));
}

#[test]
fn contexts() {
    let body = r#"<w:p><w:r><w:t>a</w:t></w:r><w:hyperlink r:id="x" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:r><w:t>b</w:t></w:r></w:hyperlink><w:ins w:id="1" w:author="x"><w:r><w:t>c</w:t></w:r></w:ins><w:del w:id="2" w:author="x"><w:r><w:delText>d</w:delText></w:r></w:del><w:moveFrom w:id="3" w:author="x"><w:r><w:t>e</w:t></w:r></w:moveFrom><w:moveTo w:id="4" w:author="x"><w:r><w:t>f</w:t></w:r></w:moveTo><w:smartTag w:uri="u" w:element="e"><w:r><w:t>g</w:t></w:r></w:smartTag><w:sdt><w:sdtPr/><w:sdtContent><w:hyperlink w:anchor="z"><w:r><w:t>h</w:t></w:r></w:hyperlink></w:sdtContent></w:sdt><w:customXml w:element="c"><w:r><w:t>i</w:t></w:r></w:customXml><w:fldSimple w:instr="PAGE"><w:r><w:t>j</w:t></w:r></w:fldSimple><w:dir w:val="rtl"><w:r><w:t>k</w:t></w:r></w:dir><w:bdo w:val="rtl"><w:r><w:t>l</w:t></w:r></w:bdo><w:r><w:ruby><w:rubyPr/><w:rt><w:r><w:t>ふりがな</w:t></w:r></w:rt><w:rubyBase><w:r><w:t>漢字</w:t></w:r></w:rubyBase></w:ruby></w:r><w:r><w:drawing><w:t>not flow</w:t></w:drawing></w:r></w:p><w:sdt><w:sdtContent><w:p><w:r><w:t>m</w:t></w:r></w:p></w:sdtContent></w:sdt><w:customXml w:element="b"><w:p><w:r><w:t>n</w:t></w:r></w:p></w:customXml><mc:AlternateContent><mc:Choice Requires="w14"><w:p><w:r><w:t>not flow</w:t></w:r></w:p></mc:Choice></mc:AlternateContent>"#;
    let doc = open(body, None, None);
    assert_eq!(doc.paragraph_count(), 3);
    let p = doc.paragraph(0).unwrap();
    use RunContext::*;
    let got: Vec<(String, RunContext)> = p.runs().map(|r| (r.text(), r.context())).collect();
    let want = [("a", Direct), ("b", Hyperlink), ("c", Inserted), ("d", Deleted), ("e", MovedFrom), ("f", MovedTo), ("g", SmartTag), ("h", ContentControl), ("i", CustomXml), ("j", SimpleField), ("k", Bidi), ("l", Bidi), ("", Direct), ("漢字", Ruby), ("", Direct)];
    assert_eq!(got, want.map(|(t, c)| (t.to_string(), c)));
    assert_eq!(p.text(), "abcfghijkl漢字", "deleted and moved-from text is excluded");
    let errs: Vec<String> = p.runs().map(|r| r.check_settable().map_or_else(|e| format!("{e:?}"), |()| "Ok".into())).collect();
    assert_eq!(errs[..3], ["Ok", "Ok", "InsideRevision { context: Inserted }"]);
    assert_eq!(errs[6..12], [
        r#"UnsupportedContext { ancestor: "w:smartTag" }"#,
        r#"UnsupportedContext { ancestor: "w:sdt" }"#,
        r#"UnsupportedContext { ancestor: "w:customXml" }"#,
        "InsideField",
        r#"UnsupportedContext { ancestor: "w:dir" }"#,
        r#"UnsupportedContext { ancestor: "w:bdo" }"#,
    ]);
    assert_eq!(errs[12], r#"RunNotTextOnly { child: "w:ruby" }"#);
    assert_eq!(errs[13], r#"UnsupportedContext { ancestor: "w:ruby" }"#);
    assert_eq!(errs[14], r#"RunNotTextOnly { child: "w:drawing" }"#);
    assert_eq!(doc.paragraph(1).unwrap().run(0).unwrap().context(), ContentControl);
    assert_eq!(doc.paragraph(2).unwrap().run(0).unwrap().context(), CustomXml);
}

#[test]
fn run_not_text_only() {
    let body = r#"<w:p><w:r><w:t>a</w:t><!-- c --></w:r><w:r><w:br w:type="page"/><w:t>b</w:t></w:r><w:r><w:br w:clear="all"/></w:r><w:r><w:sym w:font="Wingdings" w:char="F04A"/></w:r><w:r><w:footnoteReference w:id="1"/></w:r><w:r>text<w:t>x</w:t></w:r><w:r><w:rPr><w:b/></w:rPr>
  <w:t>ok</w:t><w:tab/><w:br/><w:br w:type="textWrapping"/><w:cr/><w:noBreakHyphen/><w:softHyphen/><w:lastRenderedPageBreak/>
</w:r></w:p>"#;
    let doc = open(body, None, None);
    let errs: Vec<String> = doc.paragraph(0).unwrap().runs().map(|r| r.check_settable().map_or_else(|e| e.to_string(), |()| "Ok".into())).collect();
    assert_eq!(errs, [
        "the run holds comment, not only text",
        "the run holds w:br with type/clear, not only text",
        "the run holds w:br with type/clear, not only text",
        "the run holds w:sym, not only text",
        "the run holds w:footnoteReference, not only text",
        "the run holds text, not only text",
        "Ok",
    ]);
    assert_eq!(doc.paragraph(0).unwrap().run(6).unwrap().text(), "ok\t\n\n\n\u{2011}\u{AD}");
}

#[test]
fn invalid_characters_change_nothing() {
    let input = docx(P, None, None);
    let mut doc = Document::from_bytes(input.clone()).unwrap();
    assert_matches!(doc.set_run_text(0, 0, "a\u{1}b"), Err(Error::InvalidXmlChar { ch: '\u{1}' }));
    assert_matches!(doc.append_paragraph("\u{FFFF}"), Err(Error::InvalidXmlChar { ch: '\u{FFFF}' }));
    assert_matches!(doc.set_run_text(5, 0, "x"), Err(Error::NoSuchParagraph(5)));
    assert_matches!(doc.set_run_text(0, 3, "x"), Err(Error::NoSuchRun { paragraph: 0, run: 3 }));
    assert_eq!(entries(&doc.to_bytes().unwrap()), entries(&input));
}

/// O2 / review m9: the DTD check is the parser's DocType event, wherever it
/// is in the prolog, and it covers scanned story parts too.
#[test]
fn dtd_anywhere_in_the_prolog_is_refused() {
    let input = docx(P, None, None);
    let main = String::from_utf8(part(&input, "word/document.xml")).unwrap();
    let decl_end = main.find("?>").unwrap() + 2;
    let hostile = format!("{}<!--{}--><!DOCTYPE w:document [<!ENTITY x SYSTEM \"file:///etc/passwd\">]>{}", &main[..decl_end], "c".repeat(10_000), &main[decl_end..]);
    assert_matches!(
        Document::from_bytes(with_part(&input, "word/document.xml", hostile.as_bytes())),
        Err(Error::Core(CoreError::DtdNotAllowed { position })) if position > 10_000
    );
    let header = docx(P, None, Some("<w:p/>"));
    let h = String::from_utf8(part(&header, "word/header1.xml")).unwrap().replacen("<w:hdr", "<!DOCTYPE w:hdr><w:hdr", 1);
    assert_matches!(Document::from_bytes(with_part(&header, "word/header1.xml", h.as_bytes())), Err(Error::Core(CoreError::DtdNotAllowed { .. })));
}

#[test]
fn package_level_refusals() {
    let input = docx(P, None, None);
    // O3: Strict namespace on the main root, with a Transitional relationship.
    let main = String::from_utf8(part(&input, "word/document.xml")).unwrap().replace(W_NS, "http://purl.oclc.org/ooxml/wordprocessingml/main");
    assert_matches!(Document::from_bytes(with_part(&input, "word/document.xml", main.as_bytes())), Err(Error::Core(CoreError::StrictNotSupported(_))));
    // O4: no officeDocument relationship; or a dangling one.
    let rels = String::from_utf8(part(&input, "_rels/.rels")).unwrap();
    let none = rels.replace("relationships/officeDocument", "relationships/somethingElse");
    assert_matches!(Document::from_bytes(with_part(&input, "_rels/.rels", none.as_bytes())), Err(Error::MainPartMissing));
    let dangling = rels.replace("word/document.xml", "word/missing.xml");
    assert_matches!(Document::from_bytes(with_part(&input, "_rels/.rels", dangling.as_bytes())), Err(Error::MainPartMissing));
    // Not a w:document.
    let other = br#"<x:worksheet xmlns:x="http://schemas.openxmlformats.org/spreadsheetml/2006/main"/>"#;
    assert_matches!(Document::from_bytes(with_part(&input, "word/document.xml", other)), Err(Error::Core(CoreError::WrongElement { .. })));
    // No body: reads are empty, append is refused.
    let nobody = format!(r#"<w:document xmlns:w="{W_NS}"/>"#);
    let mut doc = Document::from_bytes(with_part(&input, "word/document.xml", nobody.as_bytes())).unwrap();
    assert_eq!(doc.paragraph_count(), 0);
    assert_matches!(doc.append_paragraph("x"), Err(Error::NoBody));
}
