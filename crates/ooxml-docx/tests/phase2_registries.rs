//! ADR-0011 part 3: registry allocation, byte locality and rejection.

mod common;

use std::assert_matches;

use common::*;
use ooxml_docx::{
    Alignment, BookmarkRange, Document, Error, Highlight, ListTemplate, ParaRef, ParagraphFormat, Prop, ReplaceOptions, RunFormat,
    StoryIndex, StyleKind, StyleSpec,
};

const MAIN: &str = "word/document.xml";

fn p(index: usize) -> ParaRef {
    ParaRef::new(StoryIndex::MAIN, index)
}

fn whole(index: usize) -> BookmarkRange<'static> {
    BookmarkRange::Paragraphs {
        first: p(index),
        last: p(index),
    }
}

fn open(body: &str, related: &[(&str, &str, &str)]) -> Document {
    Document::from_bytes(package(&document_xml(body), related)).unwrap()
}

fn xml(doc: &Document, part: &str) -> String {
    String::from_utf8(doc.package().read_part(part).unwrap()).unwrap()
}

fn refusals<T: std::fmt::Debug>(doc: &mut Document, f: impl FnOnce(&mut Document) -> Result<T, Error>) -> Error {
    let before = doc.to_bytes().unwrap();
    let error = f(doc).unwrap_err();
    assert_eq!(doc.to_bytes().unwrap(), before, "{error}: Err must preserve the entire ZIP");
    error
}

fn unchanged_parts(doc: &Document, before: &[u8], except: &[&str]) {
    let output = entries(&doc.to_bytes().unwrap());
    for old in entries(before).iter().filter(|e| !except.contains(&e.0.as_str())) {
        assert_eq!(output.iter().find(|new| new.0 == old.0), Some(old), "{}", old.0);
    }
}

fn style(name: &str) -> StyleSpec<'_> {
    StyleSpec {
        kind: StyleKind::Paragraph,
        name,
        id: None,
        based_on: None,
        next: None,
        run: RunFormat::default(),
        paragraph: ParagraphFormat::default(),
    }
}

#[test]
fn bookmark_split_id_and_removal() {
    let right = r#"<w:p><w:r w:rsidR='000000AB'><w:t>untouched</w:t></w:r></w:p>"#;
    let body = format!(r#"<w:p><w:r w:rsidR="00000001"><w:rPr><w:b/></w:rPr><w:t>左中央右</w:t></w:r></w:p>{right}"#);
    let header = story_xml("hdr", r#"<w:p><w:permEnd w:id="12"/></w:p>"#);
    let mut doc = open(&body, &[("header", "header.xml", &header)]);
    let before = doc.to_bytes().unwrap();
    let m = doc.find("中央", &ReplaceOptions::default()).unwrap().remove(0);
    assert_eq!(doc.add_bookmark("日本語_1", BookmarkRange::Match(&m)).unwrap(), 13);
    let written = xml(&doc, MAIN);
    assert!(written.contains(right));
    assert!(written.contains(r#"<w:r w:rsidR="00000001"><w:rPr><w:b/></w:rPr><w:t>左</w:t></w:r><w:bookmarkStart w:id="13" w:name="日本語_1"/><w:r><w:rPr><w:b/></w:rPr><w:t>中央</w:t></w:r><w:bookmarkEnd w:id="13"/><w:r><w:rPr><w:b/></w:rPr><w:t>右</w:t></w:r>"#), "{written}");
    assert_eq!(doc.paragraph(0).unwrap().text(), "左中央右");
    unchanged_parts(&doc, &before, &[MAIN]);
    assert_matches!(
        refusals(&mut doc, |d| d.add_bookmark("日本語_1", whole(1))),
        Error::DuplicateBookmarkName { .. }
    );
    assert_matches!(
        refusals(&mut doc, |d| d.add_bookmark("New", BookmarkRange::Match(&m))),
        Error::StaleMatch
    );
    doc.remove_bookmark("日本語_1").unwrap();
    assert!(!xml(&doc, MAIN).contains("bookmarkStart"));
    assert!(!xml(&doc, MAIN).contains("bookmarkEnd"));
    assert_eq!(
        doc.add_bookmark("Again", BookmarkRange::Paragraphs { first: p(0), last: p(1) })
            .unwrap(),
        14
    );
    assert_matches!(refusals(&mut doc, |d| d.remove_bookmark("missing")), Error::UnknownBookmark { .. });
    let re = Document::from_bytes(doc.to_bytes().unwrap()).unwrap();
    assert_eq!(re.paragraph(0).unwrap().text(), "左中央右");
    assert_eq!(re.paragraph(1).unwrap().text(), "untouched");
}

#[test]
fn bookmark_refusals_cover_dangling_and_live_references() {
    let target = r#"<w:p><w:bookmarkStart w:id="1" w:name="Mark"/><w:bookmarkEnd w:id="1"/></w:p>"#;
    let ref_field = r#"<w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText> PAGEREF "ma</w:instrText></w:r><w:r><w:instrText>RK" \h</w:instrText></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p>"#;
    let simple_ref = r#"<w:p><w:fldSimple w:instr="HYPERLINK &quot;https://example.test&quot; \L &quot;MARK&quot;"/></w:p>"#;
    let anchor = r#"<w:p><w:hyperlink w:anchor="mArK"/></w:p>"#;
    for reference in [ref_field, simple_ref, anchor] {
        let mut doc = open(&format!("{target}{reference}"), &[]);
        assert_matches!(refusals(&mut doc, |d| d.remove_bookmark("mark")), Error::BookmarkReferenced { .. });
        let mut doc = open(&format!("<w:p/>{reference}"), &[]);
        assert_matches!(
            refusals(&mut doc, |d| d.add_bookmark("Mark", whole(0))),
            Error::NameReferencedByField { .. } | Error::BookmarkReferenced { .. }
        );
    }
    let mut doc = open(&format!("{target}{target}"), &[]);
    assert_matches!(
        refusals(&mut doc, |d| d.remove_bookmark("mark")),
        Error::DuplicateBookmarkName { .. }
    );
    let mut doc = open(r#"<w:p><w:bookmarkStart w:id="1" w:name="Mark"/></w:p>"#, &[]);
    assert_matches!(refusals(&mut doc, |d| d.remove_bookmark("mark")), Error::InvalidBookmark { .. });
    let mut doc = open("<w:p/><w:tbl><w:tr><w:tc><w:p/></w:tc></w:tr></w:tbl>", &[]);
    assert_matches!(
        refusals(&mut doc, |d| d
            .add_bookmark("Mark", BookmarkRange::Paragraphs { first: p(0), last: p(1) })),
        Error::BookmarkSpansContainers
    );
    assert_matches!(
        refusals(&mut doc, |d| d.add_bookmark("_Mark", whole(0))),
        Error::InvalidValue { .. }
    );
    let mut doc = open(r#"<w:p><w:permEnd w:id="2147483647"/></w:p>"#, &[]);
    assert_matches!(
        refusals(&mut doc, |d| d.add_bookmark("Mark", whole(0))),
        Error::AnnotationIdsExhausted
    );
}

#[test]
fn bookmark_run_and_container_refusals() {
    let body = r#"<w:p><w:r><w:rPr><w:rPrChange w:id="2"/></w:rPr><w:t>prefix target suffix</w:t></w:r></w:p>"#;
    let mut doc = open(body, &[]);
    let m = doc.find("target", &ReplaceOptions::default()).unwrap().remove(0);
    assert_matches!(
        refusals(&mut doc, |d| d.add_bookmark("Mark", BookmarkRange::Match(&m))),
        Error::TrackedPropertyChange { .. }
    );
    let mut doc = open(r#"<w:p><w:r><w:t>prefix&#13;target</w:t></w:r></w:p>"#, &[]);
    let m = doc.find("target", &ReplaceOptions::default()).unwrap().remove(0);
    assert_matches!(
        refusals(&mut doc, |d| d.add_bookmark("Mark", BookmarkRange::Match(&m))),
        Error::RunNotTextOnly { .. }
    );
    let mut doc = open(
        r#"<w:p><w:hyperlink w:anchor="other"><w:r><w:t>target</w:t></w:r></w:hyperlink></w:p>"#,
        &[],
    );
    let m = doc.find("target", &ReplaceOptions::default()).unwrap().remove(0);
    assert_matches!(
        refusals(&mut doc, |d| d.add_bookmark("Mark", BookmarkRange::Match(&m))),
        Error::UnsupportedContext { .. }
    );
}

#[test]
fn list_creation_and_style_suppression() {
    let body =
        r#"<w:p><w:pPr><w:pStyle w:val="Child"/><w:keepNext/></w:pPr><w:r><w:t>one</w:t></w:r></w:p><w:p><w:r><w:t>two</w:t></w:r></w:p>"#;
    let styles = story_xml(
        "styles",
        r#"<w:style w:styleId="Parent"><w:pPr><w:numPr><w:numId w:val="9"/></w:numPr></w:pPr></w:style><w:style w:styleId="Child"><w:basedOn w:val="Parent"/></w:style>"#,
    );
    let mut doc = open(body, &[("styles", "styles.xml", &styles)]);
    let before = doc.to_bytes().unwrap();
    let id = doc.create_list(ListTemplate::Bullet).unwrap();
    assert_eq!(id, 10, "skip the dangling id in a style");
    assert_eq!(xml(&doc, MAIN), document_xml(body));
    let pkg = doc.package();
    let rels = pkg.relationships(MAIN).unwrap();
    let numbering = rels.iter().find(|r| r.rel_type.ends_with("/numbering")).unwrap();
    let target = rels.resolve(numbering).unwrap();
    assert_eq!(target, "word/numbering.xml");
    assert_eq!(
        pkg.content_type(&target),
        Some("application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml")
    );
    let word_xml = xml(&doc, &target);
    let parsed = roxmltree::Document::parse(&word_xml).unwrap();
    assert_eq!(parsed.descendants().filter(|e| e.has_tag_name((W_NS, "lvl"))).count(), 9);
    assert!(word_xml.contains(r#"<w:numFmt w:val="bullet"/>"#));
    unchanged_parts(&doc, &before, &["[Content_Types].xml", "word/_rels/document.xml.rels"]);
    doc.apply_list(p(0), id, 3).unwrap();
    doc.apply_list(p(1), id, 5).unwrap();
    assert_eq!(doc.paragraph(1).unwrap().text(), "two");
    assert_eq!(doc.restart_list(p(1), 3).unwrap(), 11);
    let numbering_after_restart = xml(&doc, &target);
    assert!(xml(&doc, MAIN).contains(r#"<w:keepNext/><w:numPr><w:ilvl w:val="3"/><w:numId w:val="10"/></w:numPr>"#));
    doc.remove_from_list(p(0)).unwrap();
    assert!(xml(&doc, MAIN).contains(r#"<w:keepNext/><w:numPr><w:numId w:val="0"/></w:numPr>"#));
    doc.remove_from_list(p(1)).unwrap();
    assert_eq!(xml(&doc, MAIN).matches("<w:numPr>").count(), 1);
    assert_eq!(xml(&doc, &target), numbering_after_restart);
    assert_matches!(refusals(&mut doc, |d| d.apply_list(p(0), id, 9)), Error::InvalidValue { .. });
    assert_matches!(refusals(&mut doc, |d| d.apply_list(p(0), 99, 1)), Error::UnknownNumId { .. });
    assert_matches!(refusals(&mut doc, |d| d.restart_list(p(0), 1)), Error::InvalidNumbering);
}

#[test]
fn list_slots_and_restart_copy() {
    let old = r#"<w:num w:numId='3'><w:abstractNumId w:val='7'/><w:lvlOverride w:ilvl='0'><w:startOverride w:val='4'/></w:lvlOverride><w:lvlOverride w:ilvl='1'><w:startOverride w:val='17'/></w:lvlOverride></w:num>"#;
    let cleanup = r#"<w:numIdMacAtCleanup w:val='21'/>"#;
    let numbering = story_xml("numbering", &format!("{old}{cleanup}"));
    let item = r#"<w:p><w:pPr><w:numPr><w:ilvl w:val='0'/><w:numId w:val='3'/></w:numPr></w:pPr></w:p>"#;
    let revision = item.replace("</w:pPr>", r#"<w:pPrChange w:id="1"><w:pPr/></w:pPrChange></w:pPr>"#);
    let mut doc = open(&format!("{item}{item}{revision}"), &[("numbering", "n.xml", &numbering)]);
    assert_matches!(refusals(&mut doc, |d| d.restart_list(p(1), 7)), Error::TrackedPropertyChange { .. });
    assert_matches!(
        refusals(&mut doc, |d| d.apply_list(p(2), 3, 0)),
        Error::TrackedPropertyChange { .. }
    );
    assert_matches!(
        refusals(&mut doc, |d| d.remove_from_list(p(2))),
        Error::TrackedPropertyChange { .. }
    );
    let mut doc = open(&format!("{item}{item}{item}"), &[("numbering", "n.xml", &numbering)]);
    assert_eq!(doc.create_list(ListTemplate::Decimal).unwrap(), 22);
    let output = xml(&doc, "word/n.xml");
    assert!(output.find(r#"<w:abstractNum w:abstractNumId="8">"#).unwrap() < output.find(old).unwrap());
    assert!(output.find(r#"<w:num w:numId="22">"#).unwrap() < output.find(cleanup).unwrap());
    let old_abstract = output.split("</w:abstractNum>").next().unwrap().to_owned();
    assert_eq!(doc.restart_list(p(1), 7).unwrap(), 23);
    let output = xml(&doc, "word/n.xml");
    assert!(output.contains(&old_abstract));
    assert!(output.contains(old));
    assert!(output.contains(cleanup));
    assert!(output.contains(r#"<w:num w:numId="23"><w:abstractNumId w:val="7"/><w:lvlOverride w:ilvl="1"><w:startOverride w:val="17"/></w:lvlOverride><w:lvlOverride w:ilvl="0"><w:startOverride w:val="7"/></w:lvlOverride></w:num>"#), "{output}");
    let output = xml(&doc, MAIN);
    assert!(output.contains(item));
    assert_eq!(output.matches(r#"<w:numId w:val="23"/>"#).count(), 2, "{output}");
}

#[test]
fn numbering_refuses_exhausted_and_ambiguous_inputs() {
    let body = r#"<w:p><w:pPr><w:numPr><w:numId w:val="2147483647"/></w:numPr></w:pPr></w:p>"#;
    let mut doc = open(body, &[]);
    assert_matches!(
        refusals(&mut doc, |d| d.create_list(ListTemplate::Decimal)),
        Error::NumberingIdsExhausted
    );
    let numbering = story_xml("numbering", r#"<w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>"#);
    let mut doc = open(
        "<w:p/>",
        &[("numbering", "n.xml", &numbering), ("numbering", "other.xml", &numbering)],
    );
    assert_matches!(
        refusals(&mut doc, |d| d.create_list(ListTemplate::Decimal)),
        Error::InvalidNumbering
    );
    let body = r#"<w:p><w:pPr><w:numPr><w:numId w:val="1"/><w:numId w:val="2"/></w:numPr></w:pPr></w:p>"#;
    let mut doc = open(body, &[("numbering", "n.xml", &numbering)]);
    assert_matches!(refusals(&mut doc, |d| d.apply_list(p(0), 1, 0)), Error::InvalidNumbering);
    assert_matches!(refusals(&mut doc, |d| d.remove_from_list(p(0))), Error::InvalidNumbering);
}

#[test]
fn style_ids_references_and_locality() {
    let old = r#"<w:latentStyles><w:lsdException w:name='Heading 3'/></w:latentStyles><w:style w:styleId='Base'><w:name w:val='Base'/></w:style><w:style w:type='character' w:styleId='Emphasis'><w:name w:val='Emphasis'/></w:style>"#;
    let styles = story_xml("styles", old);
    let numbering = story_xml(
        "numbering",
        r#"<w:abstractNum w:abstractNumId="1"><w:lvl w:ilvl="0"><w:pStyle w:val="a1"/></w:lvl></w:abstractNum>"#,
    );
    let body = r#"<w:p><w:pPr><w:pStyle w:val='A'/></w:pPr></w:p>"#;
    let mut doc = open(body, &[("styles", "styles.xml", &styles), ("numbering", "n.xml", &numbering)]);
    let before = doc.to_bytes().unwrap();
    let mut spec = style("日本語");
    spec.based_on = Some("Base");
    spec.next = Some("Base");
    spec.run.bold = Prop::Set(true);
    spec.run.highlight = Prop::Set(Highlight::Yellow);
    spec.paragraph.alignment = Prop::Set(Alignment::Center);
    assert_eq!(doc.add_style(&spec).unwrap(), "a2");
    let written = xml(&doc, "word/styles.xml");
    assert!(written.contains(old));
    assert!(written.contains(r#"<w:style w:styleId="a2" w:type="paragraph" w:customStyle="1"><w:name w:val="日本語"/><w:basedOn w:val="Base"/><w:next w:val="Base"/><w:qFormat/><w:pPr><w:jc w:val="center"/></w:pPr><w:rPr><w:b/><w:highlight w:val="yellow"/></w:rPr></w:style>"#), "{written}");
    unchanged_parts(&doc, &before, &["word/styles.xml"]);
    assert_matches!(refusals(&mut doc, |d| d.add_style(&spec)), Error::DuplicateStyleName { .. });
    assert_matches!(
        refusals(&mut doc, |d| d.add_style(&style("heading 3"))),
        Error::BuiltInStyleName { .. }
    );
    assert_matches!(
        refusals(&mut doc, |d| d.add_style(&StyleSpec {
            id: Some("a"),
            ..style("New")
        })),
        Error::StyleIdInUse { .. }
    );
    assert_matches!(
        refusals(&mut doc, |d| d.add_style(&StyleSpec {
            based_on: Some("Emphasis"),
            ..style("New")
        })),
        Error::StyleTypeMismatch { .. }
    );
    assert_matches!(
        refusals(&mut doc, |d| d.add_style(&StyleSpec {
            next: Some("missing"),
            ..style("New")
        })),
        Error::UnknownStyle { .. }
    );
    assert_eq!(
        doc.add_style(&StyleSpec {
            kind: StyleKind::Character,
            based_on: Some("Emphasis"),
            ..style("2号")
        })
        .unwrap(),
        "2"
    );
    let mut doc = open("<w:p/>", &[]);
    assert_matches!(refusals(&mut doc, |d| d.add_style(&style("Missing"))), Error::NoStylesPart);
}

#[test]
fn phase2_operation_gates() {
    let styles = story_xml("styles", "");
    let mut doc = open(
        r#"<w:p/><w:p><w:fldSimple w:instr="SEQ figure"/></w:p>"#,
        &[("styles", "styles.xml", &styles)],
    );
    doc.add_bookmark("First", whole(0)).unwrap();
    doc.add_style(&style("Example")).unwrap();
    assert_matches!(
        refusals(&mut doc, |d| d.create_list(ListTemplate::Decimal)),
        Error::StaleFieldRisk { .. }
    );
    assert_matches!(refusals(&mut doc, |d| d.apply_list(p(0), 1, 0)), Error::StaleFieldRisk { .. });
    assert_matches!(refusals(&mut doc, |d| d.restart_list(p(0), 1)), Error::StaleFieldRisk { .. });
    assert_matches!(refusals(&mut doc, |d| d.remove_from_list(p(0))), Error::StaleFieldRisk { .. });
    assert_matches!(refusals(&mut doc, |d| d.remove_bookmark("First")), Error::StaleFieldRisk { .. });
    let mut doc = open(
        r#"<w:p/><w:p><w:fldSimple w:instr="TOC"/></w:p>"#,
        &[("styles", "styles.xml", &styles)],
    );
    doc.add_style(&style("Example")).unwrap();
    assert_matches!(
        refusals(&mut doc, |d| d.add_bookmark("First", whole(0))),
        Error::StaleFieldRisk { .. }
    );
}

#[test]
fn numbering_name_fallback_and_opc_delta() {
    const TYPES: &str = "[Content_Types].xml";
    const RELS: &str = "word/_rels/document.xml.rels";
    let header = story_xml("hdr", "<w:p/>");
    let mut doc = open("<w:p/>", &[("header", "numbering.xml", &header)]);
    let before = doc.to_bytes().unwrap();
    let num = doc.create_list(ListTemplate::Decimal).unwrap();
    assert_eq!(num, 1);
    let after = doc.to_bytes().unwrap();
    let names = part_names(&after);
    assert_eq!(names.len(), names.iter().collect::<std::collections::BTreeSet<_>>().len());
    let new = "word/numbering1.xml";
    assert_eq!(entries(&after).last().unwrap().0, new);
    unchanged_parts(&doc, &before, &[TYPES, RELS]);

    // Independent checks for the two new OPC registry entries. Removing
    // either new element from the output restores the old part's bytes.
    for (part_name, tag, attribute, value) in [
        (TYPES, "Override", "PartName", "/word/numbering1.xml"),
        (RELS, "Relationship", "Target", "numbering1.xml"),
    ] {
        let output = xml(&doc, part_name);
        let tree = roxmltree::Document::parse(&output).unwrap();
        let mut added = tree
            .root_element()
            .children()
            .filter(|n| n.has_tag_name(tag) && n.attribute(attribute) == Some(value));
        let entry = added.next().unwrap();
        assert!(added.next().is_none());
        if part_name == TYPES {
            assert_eq!(
                entry.attribute("ContentType"),
                Some("application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml")
            );
        } else {
            let id = entry.attribute("Id").unwrap();
            assert!(
                !tree
                    .root_element()
                    .children()
                    .any(|n| n != entry && n.attribute("Id").is_some_and(|other| other.eq_ignore_ascii_case(id)))
            );
            assert!(entry.attribute("Type").unwrap().ends_with("/numbering"));
        }
        let mut restored = output.clone();
        restored.replace_range(entry.range(), "");
        assert_eq!(restored.as_bytes(), part(&before, part_name));
    }
}
