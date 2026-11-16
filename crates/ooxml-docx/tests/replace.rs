//! ADR-0007 §1, §2, §6.1, §7: cross-run find/replace, stories, stale
//! matches, per-operation W2. Unit documents (T4 refusals included: a
//! refused call leaves `to_bytes()` identical to the input).

mod common;

use std::assert_matches;

use common::*;
use ooxml_core::Error as CoreError;
use ooxml_docx::{
    Container, Document, Error, HeaderFooterType, OnRefused, Operation, ParaRef, RefusalReason, ReplaceOptions,
    RunFormatting, Scope, SectionUse, StoryIndex, StoryKind,
};

fn open(body: &str) -> Document {
    Document::from_bytes(package(&document_xml(body), &[])).unwrap()
}

fn main_xml(doc: &Document) -> String {
    String::from_utf8(doc.package().read_part("word/document.xml").unwrap()).unwrap()
}

fn body_of(doc: &Document) -> String {
    let s = main_xml(doc);
    let a = s.find("<w:body>").unwrap() + 8;
    let b = s.rfind("</w:body>").unwrap();
    s[a..b].to_owned()
}

fn reopen(doc: &Document) -> Document {
    Document::from_bytes(doc.to_bytes().unwrap()).unwrap()
}

fn default() -> ReplaceOptions {
    ReplaceOptions::default()
}

fn kinds(doc: &Document, needle: &str, opts: &ReplaceOptions) -> Vec<&'static str> {
    doc.find(needle, opts).unwrap().iter().map(|m| m.status.as_ref().map_or_else(|r| r.reason.kind(), |()| "Ok")).collect()
}

/// Replaces and asserts the call wrote nothing when refused.
fn refused(body: &str, needle: &str) -> RefusalReason {
    let mut doc = open(body);
    let before = doc.to_bytes().unwrap();
    let err = doc.replace(needle, "X", &default()).unwrap_err();
    assert_eq!(doc.to_bytes().unwrap(), before, "a refused call writes nothing");
    match err {
        Error::ReplaceRefused { mut refused } => refused.remove(0).reason,
        e => panic!("{e}"),
    }
}

const R: &str = "<w:r><w:t>";
const RE: &str = "</w:t></w:r>";

#[test]
fn split_placeholder_with_proof_err_and_collapsed_go_back() {
    let body = r#"<w:p><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">Dear {{</w:t></w:r><w:proofErr w:type="spellStart"/><w:r><w:rPr><w:b/></w:rPr><w:t>na</w:t></w:r><w:bookmarkStart w:id="0" w:name="_GoBack"/><w:bookmarkEnd w:id="0"/><w:r><w:rPr><w:b/></w:rPr><w:t>me}}, hi</w:t></w:r><w:proofErr w:type="spellEnd"/></w:p>"#;
    let mut doc = open(body);
    let before = main_xml(&doc);
    let report = doc.replace("{{name}}", "Ada", &default()).unwrap();
    assert_eq!(report.replaced, 1);
    assert_eq!(doc.paragraph(0).unwrap().text(), "Dear Ada, hi");
    let after = body_of(&reopen(&doc));
    assert_eq!(
        after,
        r#"<w:p><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">Dear Ada</w:t></w:r><w:proofErr w:type="spellStart"/><w:r><w:rPr><w:b/></w:rPr></w:r><w:bookmarkStart w:id="0" w:name="_GoBack"/><w:bookmarkEnd w:id="0"/><w:r><w:rPr><w:b/></w:rPr><w:t>, hi</w:t></w:r><w:proofErr w:type="spellEnd"/></w:p>"#
    );
    // T2: the diff lies inside the spans of the touched runs.
    let (start, end, _) = diff_range(before.as_bytes(), main_xml(&doc).as_bytes()).unwrap();
    assert!(start >= before.find("<w:r>").unwrap() && end <= before.rfind("<w:proofErr").unwrap());
}

#[test]
fn bookmark_wrapping_a_placeholder_wraps_the_value() {
    let body = r#"<w:p><w:bookmarkStart w:id="1" w:name="client"/><w:r><w:t>{{c</w:t></w:r><w:r><w:t>lient}}</w:t></w:r><w:bookmarkEnd w:id="1"/></w:p>"#;
    let mut doc = open(body);
    doc.replace("{{client}}", "ACME", &default()).unwrap();
    assert_eq!(
        body_of(&doc),
        r#"<w:p><w:bookmarkStart w:id="1" w:name="client"/><w:r><w:t>ACME</w:t></w:r><w:r></w:r><w:bookmarkEnd w:id="1"/></w:p>"#
    );
}

#[test]
fn boundaries_markers_and_run_refusals() {
    // a hyperlink edge
    let r = refused(r#"<w:p><w:r><w:t>{{a</w:t></w:r><w:hyperlink w:anchor="x"><w:r><w:t>}}</w:t></w:r></w:hyperlink></w:p>"#, "{{a}}");
    assert_matches!(r, RefusalReason::MatchCrossesBoundary { boundary } if boundary == "w:hyperlink");
    // inside one hyperlink is fine
    let mut doc = open(r#"<w:p><w:hyperlink w:anchor="x"><w:r><w:t>{{a</w:t></w:r><w:r><w:t>}}</w:t></w:r></w:hyperlink></w:p>"#);
    doc.replace("{{a}}", "v", &default()).unwrap();
    assert_eq!(doc.paragraph(0).unwrap().text(), "v");
    // a comment range inside
    let r = refused(&format!(r#"<w:p>{R}{{{{a{RE}<w:commentRangeStart w:id="7"/>{R}}}}}{RE}</w:p>"#), "{{a}}");
    assert_matches!(r, RefusalReason::MarkerInsideMatch { marker, id } if marker == "w:commentRangeStart" && id.as_deref() == Some("7"));
    // a non-collapsed bookmark start inside
    let r = refused(&format!(r#"<w:p>{R}{{{{a{RE}<w:bookmarkStart w:id="3" w:name="b"/>{R}}}}}{RE}<w:bookmarkEnd w:id="3"/></w:p>"#), "{{a}}");
    assert_matches!(r, RefusalReason::MarkerInsideMatch { .. });
    // a field result
    let fld = r#"<w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText> DATE </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>{{d}}</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r>"#;
    assert_matches!(refused(&format!("<w:p>{fld}</w:p>"), "{{d}}"), RefusalReason::InsideField);
    // a zero-width boundary run (footnote reference) inside
    let r = refused(&format!(r#"<w:p>{R}{{{{a{RE}<w:r><w:footnoteReference w:id="1"/></w:r>{R}}}}}{RE}</w:p>"#), "{{a}}");
    assert_matches!(r, RefusalReason::MatchCrossesBoundary { .. });
    // a tracked formatting change
    let r = refused(r#"<w:p><w:r><w:rPr><w:rPrChange w:id="1" w:author="a"><w:rPr/></w:rPrChange></w:rPr><w:t>{{a}}</w:t></w:r></w:p>"#, "{{a}}");
    assert_matches!(r, RefusalReason::InsideRevision { context: None });
    // a smart tag stays closed (§2.5) and is a boundary
    let r = refused(r#"<w:p><w:r><w:t>{{</w:t></w:r><w:smartTag w:uri="u" w:element="e"><w:r><w:t>a}}</w:t></w:r></w:smartTag></w:p>"#, "{{a}}");
    assert_matches!(r, RefusalReason::UnsupportedContext { ancestor: "w:smartTag" });
    // math is a boundary
    let r = refused(r#"<w:p><w:r><w:t>{{</w:t></w:r><m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"/><w:r><w:t>a}}</w:t></w:r></w:p>"#, "{{a}}");
    assert_matches!(r, RefusalReason::MatchCrossesBoundary { boundary } if boundary == "m:oMath");
}

#[test]
fn formatting_rule() {
    let two = |a: &str, b: &str| format!(r#"<w:p><w:r><w:rPr>{a}</w:rPr><w:t>{{{{x</w:t></w:r><w:r><w:rPr>{b}</w:rPr><w:t>y}}}}</w:t></w:r></w:p>"#);
    let check = |a: &str, b: &str| -> [&'static str; 3] {
        let doc = open(&two(a, b));
        let k = |f| kinds(&doc, "{{xy}}", &default().formatting(f))[0];
        [k(RunFormatting::Identical), k(RunFormatting::IgnoreInvisible), k(RunFormatting::FirstRun)]
    };
    assert_eq!(check("<w:b/>", "<w:b/>"), ["Ok", "Ok", "Ok"]);
    assert_eq!(check("<w:b/>", ""), ["MixedFormatting", "MixedFormatting", "Ok"]);
    assert_eq!(check(r#"<w:lang w:val="en-US"/>"#, r#"<w:lang w:val="de-DE"/>"#), ["MixedFormatting", "Ok", "Ok"]);
    assert_eq!(check(r#"<w:noProof/>"#, ""), ["MixedFormatting", "Ok", "Ok"]);
    assert_eq!(check(r#"<w:lang w:eastAsia="ja-JP"/>"#, ""), ["MixedFormatting", "MixedFormatting", "Ok"]);
    assert_eq!(check(r#"<w:rFonts w:hint="eastAsia"/>"#, ""), ["MixedFormatting", "Ok", "Ok"]);
    assert_eq!(check(r#"<w:rFonts w:hint="cs"/>"#, ""), ["MixedFormatting", "MixedFormatting", "Ok"]);
    // prefixes and attribute order are ignored
    let doc = open(r#"<w:p><w:r><w:rPr><w:sz w:val="20"/><w:color w:val="FF0000" w:themeColor="accent1"/></w:rPr><w:t>{{x</w:t></w:r><w:r><x:rPr xmlns:x="http://schemas.openxmlformats.org/wordprocessingml/2006/main"> <x:sz x:val="20"/><x:color x:themeColor="accent1" x:val="FF0000"/></x:rPr><w:t>y}}</w:t></w:r></w:p>"#);
    assert_eq!(kinds(&doc, "{{xy}}", &default().formatting(RunFormatting::Identical)), ["Ok"]);
    // hint-ambiguous characters in the later run: the hint difference is visible
    let doc = open(r#"<w:p><w:r><w:t>can</w:t></w:r><w:r><w:rPr><w:rFonts w:hint="eastAsia"/></w:rPr><w:t>’t</w:t></w:r></w:p>"#);
    assert_eq!(kinds(&doc, "can’t", &default()), ["MixedFormatting"]);
    let doc = open(r#"<w:p><w:r><w:t>日本</w:t></w:r><w:r><w:rPr><w:rFonts w:hint="eastAsia"/></w:rPr><w:t>語</w:t></w:r></w:p>"#);
    assert_eq!(kinds(&doc, "日本語", &default()), ["Ok"]);
    // the replacement takes r₀'s formatting
    let mut doc = open(&two("<w:i/>", "<w:i/><w:noProof/>"));
    doc.replace("{{xy}}", "V", &default()).unwrap();
    assert_eq!(body_of(&doc), r#"<w:p><w:r><w:rPr><w:i/></w:rPr><w:t>V</w:t></w:r><w:r><w:rPr><w:i/><w:noProof/></w:rPr></w:r></w:p>"#);
}

#[test]
fn content_outside_the_match_is_untouched() {
    // M3: noBreakHyphen, softHyphen, cr outside the match stay elements; a
    // lastRenderedPageBreak at an edge (after `}}`) stays on its side; the
    // ones inside (after `{{` and after `x`) go.
    let body = r#"<w:p><w:r><w:noBreakHyphen/><w:t>a{{</w:t><w:lastRenderedPageBreak/></w:r><w:r><w:t>x</w:t><w:lastRenderedPageBreak/><w:t>}}</w:t><w:lastRenderedPageBreak/><w:softHyphen/><w:cr/><w:t>z</w:t></w:r></w:p>"#;
    let mut doc = open(body);
    assert_eq!(doc.paragraph(0).unwrap().text(), "\u{2011}a{{x}}\u{AD}\nz");
    doc.replace("{{x}}", "Q", &default()).unwrap();
    assert_eq!(
        body_of(&doc),
        r#"<w:p><w:r><w:noBreakHyphen/><w:t>aQ</w:t></w:r><w:r><w:lastRenderedPageBreak/><w:softHyphen/><w:cr/><w:t>z</w:t></w:r></w:p>"#
    );
    // a replace next to a noBreakHyphen keeps it as an element
    let mut doc = open(r#"<w:p><w:r><w:t>{{a}}</w:t><w:noBreakHyphen/><w:t>b</w:t></w:r></w:p>"#);
    doc.replace("{{a}}", "x", &default()).unwrap();
    assert_eq!(body_of(&doc), r#"<w:p><w:r><w:t>x</w:t><w:noBreakHyphen/><w:t>b</w:t></w:r></w:p>"#);
}

#[test]
fn replacement_mapping_and_literal_kept_text() {
    // \t and \n in the replacement become w:tab / w:br; the prefix and suffix
    // share a w:t with the adjacent text pieces.
    let mut doc = open(r#"<w:p><w:r><w:t>A{{x}}B</w:t></w:r></w:p>"#);
    doc.replace("{{x}}", "1\t2\n3", &default()).unwrap();
    assert_eq!(body_of(&doc), r#"<w:p><w:r><w:t>A1</w:t><w:tab/><w:t>2</w:t><w:br/><w:t>3B</w:t></w:r></w:p>"#);
    assert_eq!(reopen(&doc).paragraph(0).unwrap().text(), "A1\t2\n3B");
    // n1: a raw tab and line feed in a producer's w:t before and after the
    // match stay literal characters of that w:t.
    let mut doc = open("<w:p><w:r><w:t xml:space=\"preserve\">a\tb\n{{x}}\tc</w:t></w:r></w:p>");
    assert_eq!(doc.paragraph(0).unwrap().text(), "a\tb\n{{x}}\tc");
    doc.replace("{{x}}", "V", &default()).unwrap();
    assert_eq!(body_of(&doc), "<w:p><w:r><w:t xml:space=\"preserve\">a\tb\nV\tc</w:t></w:r></w:p>");
    // the replacement starts with a tab: the w:t is not kept, the suffix gets a new one
    let mut doc = open(r#"<w:p><w:r><w:t>{{x}}B</w:t></w:r></w:p>"#);
    doc.replace("{{x}}", "\tA", &default()).unwrap();
    assert_eq!(body_of(&doc), r#"<w:p><w:r><w:tab/><w:t>AB</w:t></w:r></w:p>"#);
    // empty replacement across runs, prefix and suffix in different runs
    let mut doc = open(r#"<w:p><w:r><w:t xml:space="preserve">a {{</w:t></w:r><w:r><w:t>x}} b</w:t></w:r></w:p>"#);
    doc.replace("{{x}}", "", &default()).unwrap();
    assert_eq!(body_of(&doc), r#"<w:p><w:r><w:t xml:space="preserve">a </w:t></w:r><w:r><w:t xml:space="preserve"> b</w:t></w:r></w:p>"#);
    assert_eq!(reopen(&doc).paragraph(0).unwrap().text(), "a  b");
}

#[test]
fn several_matches_compose() {
    let mut doc = open(r#"<w:p><w:r><w:t>{{a}} and {{a}}</w:t></w:r><w:r><w:t>{{</w:t></w:r><w:r><w:t>a}}!</w:t></w:r></w:p>"#);
    let report = doc.replace("{{a}}", "X\tY", &default()).unwrap();
    assert_eq!(report.replaced, 3);
    assert_eq!(doc.paragraph(0).unwrap().text(), "X\tY and X\tYX\tY!");
    assert_eq!(
        body_of(&doc),
        r#"<w:p><w:r><w:t>X</w:t><w:tab/><w:t>Y and X</w:t><w:tab/><w:t>Y</w:t></w:r><w:r><w:t>X</w:t><w:tab/><w:t>Y</w:t></w:r><w:r><w:t>!</w:t></w:r></w:p>"#
    );
}

#[test]
fn unpreserved_edge_whitespace_is_refused() {
    assert_matches!(refused(r#"<w:p><w:r><w:t> a{{x}}</w:t></w:r></w:p>"#, "{{x}}"), RefusalReason::UnpreservedWhitespace);
    assert_matches!(refused(r#"<w:p><w:r><w:t>{{x}}b </w:t></w:r></w:p>"#, "{{x}}"), RefusalReason::UnpreservedWhitespace);
    // inside the match it is replaced, not kept
    let mut doc = open(r#"<w:p><w:r><w:t> {{x}}</w:t></w:r></w:p>"#);
    doc.replace(" {{x}}", "v", &default()).unwrap();
    assert_eq!(body_of(&doc), r#"<w:p><w:r><w:t>v</w:t></w:r></w:p>"#);
}

#[test]
fn atomicity_and_skip() {
    let body = r#"<w:p><w:r><w:t>{{a}}</w:t></w:r></w:p><w:p><w:r><w:t>{{</w:t></w:r><w:r><w:rPr><w:b/></w:rPr><w:t>a}}</w:t></w:r></w:p><w:p><w:r><w:t>{{a}}</w:t></w:r></w:p>"#;
    let mut doc = open(body);
    let before = doc.to_bytes().unwrap();
    let err = doc.replace("{{a}}", "v", &default()).unwrap_err();
    assert_matches!(&err, Error::ReplaceRefused { refused } if refused.len() == 1 && refused[0].para == ParaRef::new(StoryIndex::MAIN, 1));
    assert_eq!(doc.to_bytes().unwrap(), before);
    assert_eq!(doc.generation(ParaRef::new(StoryIndex::MAIN, 0)), 0);
    let report = doc.replace("{{a}}", "v", &default().on_refused(OnRefused::Skip)).unwrap();
    assert_eq!(report.replaced, 2);
    assert_eq!(report.skipped.len(), 1);
    assert_eq!(report.skipped[0].reason.kind(), "MixedFormatting");
    let texts: Vec<String> = doc.paragraphs().map(|p| p.text()).collect();
    assert_eq!(texts, ["v", "{{a}}", "v"]);
}

#[test]
fn dry_run_failure_refuses_before_any_write() {
    // A producer's w:rPr after the content: inserting a new w:t after the
    // kept one would land before w:rPr (UnresolvedSibling). The first
    // paragraph's replace must not be written either.
    let body = r#"<w:p><w:r><w:t>{{a}}</w:t></w:r></w:p><w:p><w:r><w:t>{{a}}</w:t><w:rPr><w:b/></w:rPr></w:r></w:p>"#;
    let mut doc = open(body);
    let before = doc.to_bytes().unwrap();
    assert_eq!(kinds(&doc, "{{a}}", &default()), ["Ok", "Ok"], "find does not know the replacement");
    let err = doc.replace("{{a}}", "x\ty", &default()).unwrap_err();
    assert_matches!(&err, Error::ReplaceRefused { refused } if matches!(&refused[0].reason, RefusalReason::Core(c) if matches!(**c, CoreError::UnresolvedSibling(_))));
    assert_eq!(doc.to_bytes().unwrap(), before);
    // a one-piece replacement needs no insert there
    doc.replace("{{a}}", "x", &default()).unwrap();
    assert_eq!(body_of(&doc), r#"<w:p><w:r><w:t>x</w:t></w:r></w:p><w:p><w:r><w:t>x</w:t><w:rPr><w:b/></w:rPr></w:r></w:p>"#);
    // under Skip, the failing run is skipped and the rest written
    let mut doc = open(body);
    let report = doc.replace("{{a}}", "x\ty", &default().on_refused(OnRefused::Skip)).unwrap();
    assert_eq!((report.replaced, report.skipped.len()), (1, 1));
}

#[test]
fn stale_matches_and_generations() {
    let body = r#"<w:p><w:r><w:t>{{a}} {{b}}</w:t></w:r></w:p><w:p><w:r><w:t>{{c}}</w:t></w:r></w:p>"#;
    let mut doc = open(body);
    let p0 = ParaRef::new(StoryIndex::MAIN, 0);
    let ms = doc.find("{{", &default()).unwrap();
    let ma = doc.find("{{a}}", &default()).unwrap().remove(0);
    let mb = doc.find("{{b}}", &default()).unwrap().remove(0);
    let mc = doc.find("{{c}}", &default()).unwrap().remove(0);
    assert_eq!(ms.len(), 3);
    // two matches of one paragraph together
    let before = doc.to_bytes().unwrap();
    doc.replace_matches(&[(&ma, "A"), (&mb, "B")], &default()).unwrap();
    assert_eq!(doc.paragraph(0).unwrap().text(), "A B");
    assert_eq!(doc.generation(p0), 1);
    // the same matches again: stale
    assert_matches!(doc.replace_at(&ma, "Z", &default()), Err(Error::StaleMatch));
    // a match in another paragraph stays usable
    doc.replace_at(&mc, "C", &default()).unwrap();
    assert_eq!(doc.paragraph(1).unwrap().text(), "C");
    // one replace_at then another in the same paragraph: stale
    let mut doc = Document::from_bytes(before.clone()).unwrap();
    let ma = doc.find("{{a}}", &default()).unwrap().remove(0);
    let mb = doc.find("{{b}}", &default()).unwrap().remove(0);
    doc.replace_at(&ma, "A", &default()).unwrap();
    let snapshot = doc.to_bytes().unwrap();
    assert_matches!(doc.replace_at(&mb, "B", &default()), Err(Error::StaleMatch));
    assert_eq!(doc.to_bytes().unwrap(), snapshot);
    // find → another write (set_run_text) → replace_at: stale
    let mut doc = Document::from_bytes(before.clone()).unwrap();
    let ma = doc.find("{{a}}", &default()).unwrap().remove(0);
    doc.set_run_text(0, 0, "{{a}} {{b}} ").unwrap();
    assert_matches!(doc.replace_at(&ma, "A", &default()), Err(Error::StaleMatch));
    // no-ops do not count as edits
    let mut doc = Document::from_bytes(before.clone()).unwrap();
    let ma = doc.find("{{a}}", &default()).unwrap().remove(0);
    doc.replace("{{b}}", "{{b}}", &default()).unwrap();
    doc.set_run_text(1, 0, "{{c}}").unwrap();
    assert_eq!(doc.to_bytes().unwrap(), before);
    assert_eq!(doc.generation(p0), 0);
    doc.replace_at(&ma, "A", &default()).unwrap();
    // a match from another Document
    let other = Document::from_bytes(before.clone()).unwrap();
    let foreign = other.find("{{c}}", &default()).unwrap().remove(0);
    assert_matches!(doc.replace_at(&foreign, "C", &default()), Err(Error::StaleMatch));
    // replace_at re-runs the checks with its own options
    let mut doc = open(r#"<w:p><w:r><w:t>{{</w:t></w:r><w:r><w:rPr><w:b/></w:rPr><w:t>a}}</w:t></w:r></w:p>"#);
    let first_run = default().formatting(RunFormatting::FirstRun);
    let m = doc.find("{{a}}", &first_run).unwrap().remove(0);
    assert!(m.status.is_ok());
    assert_matches!(doc.replace_at(&m, "v", &default()), Err(Error::ReplaceRefused { .. }));
    doc.replace_at(&m, "v", &first_run).unwrap();
    // overlapping matches
    let mut doc = open(r#"<w:p><w:r><w:t>aaa</w:t></w:r></w:p>"#);
    let a = doc.find("aa", &default()).unwrap().remove(0);
    let b = ooxml_docx::Match { range: 1..3, ..a.clone() };
    assert_matches!(doc.replace_matches(&[(&a, "x"), (&b, "y")], &default()), Err(Error::OverlappingMatches));
}

#[test]
fn needle_errors() {
    let mut doc = open(r#"<w:p><w:r><w:t>a</w:t></w:r></w:p>"#);
    assert_matches!(doc.find("", &default()), Err(Error::EmptyNeedle));
    assert_matches!(doc.replace("a", "\u{1}", &default()), Err(Error::InvalidXmlChar { ch: '\u{1}' }));
    assert_matches!(doc.find("\u{FFFF}", &default()), Err(Error::InvalidXmlChar { .. }));
    // non-overlapping, leftmost first, replacement not searched again
    let mut doc = open(r#"<w:p><w:r><w:t>aaaa</w:t></w:r></w:p>"#);
    assert_eq!(doc.replace("aa", "a", &default()).unwrap().replaced, 2);
    assert_eq!(doc.paragraph(0).unwrap().text(), "aa");
}

fn footnotes() -> String {
    story_xml(
        "footnotes",
        r#"<w:footnote w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r></w:p></w:footnote><w:footnote w:id="1"><w:p><w:r><w:footnoteRef/></w:r><w:r><w:t xml:space="preserve"> See {{src}}.</w:t></w:r></w:p></w:footnote>"#,
    )
}

fn stories_doc() -> Vec<u8> {
    let body = r#"<w:p><w:r><w:t>Body {{x}}</w:t></w:r><w:r><w:footnoteReference w:id="1"/></w:r></w:p><w:p><w:pPr><w:sectPr><w:headerReference w:type="default" r:id="rIdX0"/><w:headerReference w:type="first" r:id="rIdX1"/></w:sectPr></w:pPr></w:p><w:sectPr><w:footerReference r:id="rIdX2"/><w:titlePg/></w:sectPr>"#;
    package(
        &document_xml(body),
        &[
            ("header", "header1.xml", &story_xml("hdr", r#"<w:p><w:r><w:t>{{x}} letterhead</w:t></w:r></w:p>"#)),
            ("header", "header2.xml", &story_xml("hdr", r#"<w:p><w:r><w:t>first</w:t></w:r></w:p>"#)),
            ("footer", "footer1.xml", &story_xml("ftr", r#"<w:tbl><w:tr><w:tc><w:p><w:r><w:t>{{x}}</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#)),
            ("footnotes", "footnotes.xml", &footnotes()),
            ("comments", "comments.xml", &story_xml("comments", r#"<w:comment w:id="0" w:author="a"><w:p><w:r><w:annotationRef/></w:r><w:r><w:t>check {{x}}</w:t></w:r></w:p></w:comment>"#)),
        ],
    )
}

#[test]
fn stories_are_listed_and_edited() {
    let input = stories_doc();
    let mut doc = Document::from_bytes(input.clone()).unwrap();
    let kinds: Vec<(StoryKind, String)> = doc.stories().map(|s| (s.id().kind(), s.id().part().to_owned())).collect();
    assert_eq!(
        kinds,
        [
            (StoryKind::Main, "word/document.xml".into()),
            (StoryKind::Header, "word/header1.xml".into()),
            (StoryKind::Header, "word/header2.xml".into()),
            (StoryKind::Footer, "word/footer1.xml".into()),
            (StoryKind::Footnotes, "word/footnotes.xml".into()),
            (StoryKind::Comments, "word/comments.xml".into()),
        ]
    );
    // §2.3: sections, with inheritance and displayed
    let s = |i| -> Vec<(usize, HeaderFooterType, bool)> {
        doc.stories().nth(i).unwrap().sections().iter().map(|u: &SectionUse| (u.section, u.kind, u.displayed)).collect()
    };
    let use_ = |section, kind, displayed| (section, kind, displayed);
    assert_eq!(s(1), [use_(0, HeaderFooterType::Default, true), use_(1, HeaderFooterType::Default, true)]);
    assert_eq!(s(2), [use_(0, HeaderFooterType::First, false), use_(1, HeaderFooterType::First, true)]);
    assert_eq!(s(3), [use_(1, HeaderFooterType::Default, true)]);
    assert!(s(4).is_empty());
    // containers and separator notes (§2.2)
    let notes = doc.stories().nth(4).unwrap();
    let paras: Vec<_> = notes.paragraphs().collect();
    assert_matches!(paras[0].container(), Container::Note { separator: true, .. });
    assert_matches!(paras[0].runs().next().unwrap().check_settable(), Err(Error::UnsupportedContext { ancestor: "w:footnote (separator)" }));
    assert_matches!(paras[1].container(), Container::Note { kind: StoryKind::Footnotes, id: Some(ref id), separator: false } if id == "1");
    assert_matches!(paras[1].runs().next().unwrap().check_settable(), Err(Error::RunNotTextOnly { .. }));
    assert!(paras[1].runs().nth(1).unwrap().check_settable().is_ok());
    // one replace edits every story; only the edited parts change (T3)
    let report = doc.replace("{{x}}", "X", &default()).unwrap();
    assert_eq!(report.replaced, 4);
    let out = doc.to_bytes().unwrap();
    let changed: Vec<String> = entries(&out).into_iter().zip(entries(&input)).filter(|(a, b)| a != b).map(|(a, _)| a.0).collect();
    assert_eq!(changed, ["word/document.xml", "word/header1.xml", "word/footer1.xml", "word/comments.xml"]);
    let re = Document::from_bytes(out).unwrap();
    let texts: Vec<String> = re.stories().flat_map(|s| s.paragraphs().map(|p| p.text()).collect::<Vec<_>>()).collect();
    assert_eq!(texts, ["Body X", "", "X letterhead", "first", "X", "", " See {{src}}.", "check X"]);
    // footnote edit through a story scope; the separator is never touched
    let notes_id = re.stories().nth(4).unwrap().id().clone();
    let mut re = re;
    re.replace("{{src}}", "ECMA-376", &ReplaceOptions::with_scope(Scope::Story(notes_id))).unwrap();
    let xml = String::from_utf8(re.package().read_part("word/footnotes.xml").unwrap()).unwrap();
    assert!(xml.contains(r#"<w:footnote w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r></w:p></w:footnote>"#));
    assert!(xml.contains(r#"<w:t xml:space="preserve"> See ECMA-376.</w:t>"#));
    // set_run_text_at in a header
    let h = re.stories().nth(1).unwrap().paragraphs().next().unwrap().runs().next().unwrap().run_ref();
    re.set_run_text_at(h, "Letterhead").unwrap();
    assert_eq!(re.stories().nth(1).unwrap().paragraphs().next().unwrap().text(), "Letterhead");
}

#[test]
fn per_operation_stale_field_risk() {
    let fld = |code: &str| {
        format!(r#"<w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText xml:space="preserve"> {code} </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>1</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p><w:p><w:r><w:t>{{{{a}}}}</w:t></w:r></w:p>"#)
    };
    let gate = |code: &str, op| open(&fld(code)).write_gate_for(op).is_ok();
    // §6.1 table: (code, text, append, format, style)
    let table: &[(&str, bool, bool, bool, bool)] = &[
        ("SEQ Figure \\* ARABIC", true, true, true, false),
        ("SEQ Figure \\s 1", true, true, true, false),
        ("SEQ Table bm", true, true, true, false),
        ("TC \"x\" \\l 1", true, true, true, false),
        ("XE \"x\"", true, true, true, false),
        ("NOTEREF n1 \\h", true, true, true, false),
        ("LISTNUM \\l 1", true, true, true, false),
        ("AUTONUM", true, true, true, false),
        ("AUTONUMLGL", true, true, true, false),
        ("AUTONUMOUT", true, true, true, false),
        ("NUMWORDS", false, false, true, false),
        ("NUMCHARS", false, false, true, false),
        ("INFO NumWords", false, false, true, false),
        ("INFO NumChars", false, false, true, false),
        ("DOCPROPERTY Words", false, false, true, false),
        ("DOCPROPERTY Characters", false, false, true, false),
        ("DOCPROPERTY CharactersWithSpaces", false, false, true, false),
        ("= 1+1 \\# 0", false, false, true, false),
        ("DOCPROPERTY Lines", false, false, false, false),
        ("TOC \\o", false, false, false, false),
        ("REF bm", false, false, false, false),
        ("PAGEREF bm", false, false, false, false),
        ("STYLEREF 1", false, false, false, false),
        ("IF 1 = 1 \"a\" \"b\"", false, false, false, false),
        ("INDEX", false, false, false, false),
        ("TOA", false, false, false, false),
        ("CITATION x", false, false, false, false),
        ("BIBLIOGRAPHY", false, false, false, false),
        ("FOO", false, false, false, false),
        ("PAGE", true, true, true, true),
    ];
    for &(code, text, append, format, style) in table {
        let got = [Operation::Text, Operation::Append, Operation::Format, Operation::Style].map(|op| gate(code, op));
        assert_eq!(got, [text, append, format, style], "{code}");
        // the operations themselves use their column
        let mut doc = open(&fld(code));
        assert_eq!(doc.replace("{{a}}", "v", &default()).is_ok(), text, "{code}");
        assert_eq!(doc.append_paragraph("x").is_ok(), append, "{code}");
        assert_eq!(doc.write_gate().is_ok(), style, "{code}");
    }
    // every nested field must be allowed too
    let nested = r#"<w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText>SEQ x </w:instrText></w:r><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText>REF bm</w:instrText></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p>"#;
    assert_matches!(open(nested).write_gate_for(Operation::Text), Err(Error::StaleFieldRisk { codes }) if codes == ["REF"]);
    // the fixtures: sdk-complex2010 (=, SEQ, TC) is lifted for formatting only
    let doc = Document::from_bytes(fixture("sdk-complex2010")).unwrap();
    assert_matches!(doc.write_gate_for(Operation::Text), Err(Error::StaleFieldRisk { codes }) if codes == ["="]);
    assert!(doc.write_gate_for(Operation::Format).is_ok());
    let doc = Document::from_bytes(fixture("sdk-TOC2")).unwrap();
    for op in [Operation::Text, Operation::Append, Operation::Format, Operation::Style] {
        assert!(doc.write_gate_for(op).is_err());
    }
}
