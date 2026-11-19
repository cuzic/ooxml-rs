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
    let old = r#"<w:num w:numId='3'><w:abstractNumId w:val='7'/><w:lvlOverride w:ilvl='0'><w:startOverride w:val='4'/><w:lvl w:ilvl='0'><w:numFmt w:val='upperRoman'/></w:lvl></w:lvlOverride><w:lvlOverride w:ilvl='1'><w:startOverride w:val='17'/></w:lvlOverride></w:num>"#;
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
    assert!(output.contains(r#"<w:num w:numId="23"><w:abstractNumId w:val="7"/><w:lvlOverride w:ilvl="0"><w:startOverride w:val="7"/><w:lvl w:ilvl="0"><w:numFmt w:val="upperRoman"/></w:lvl></w:lvlOverride><w:lvlOverride w:ilvl="1"><w:startOverride w:val="17"/></w:lvlOverride></w:num>"#), "{output}");
    let output = xml(&doc, MAIN);
    assert!(output.contains(item));
    assert_eq!(output.matches(r#"<w:numId w:val="23"/>"#).count(), 2, "{output}");
}

#[test]
fn restart_list_refuses_duplicate_level_overrides() {
    let body = r#"<w:p><w:pPr><w:numPr><w:numId w:val="3"/></w:numPr></w:pPr></w:p>"#;
    for (level, alias) in [("0", "00"), ("1", "01")] {
        let numbering = story_xml(
            "numbering",
            &format!(r#"<w:num w:numId="3"><w:abstractNumId w:val="7"/><w:lvlOverride w:ilvl="{level}"><w:startOverride w:val="4"/></w:lvlOverride><w:lvlOverride w:ilvl="{alias}"><w:startOverride w:val="9"/></w:lvlOverride></w:num>"#),
        );
        let mut doc = open(body, &[("numbering", "n.xml", &numbering)]);
        assert_matches!(refusals(&mut doc, |d| d.restart_list(p(0), 3)), Error::InvalidNumbering);
    }
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
    assert_eq!(doc.add_style(&spec).unwrap(), "a0");
    let written = xml(&doc, "word/styles.xml");
    assert!(written.contains(old));
    assert!(written.contains(r#"<w:style w:styleId="a0" w:type="paragraph" w:customStyle="1"><w:name w:val="日本語"/><w:basedOn w:val="Base"/><w:next w:val="Base"/><w:qFormat/><w:pPr><w:jc w:val="center"/></w:pPr><w:rPr><w:b/><w:highlight w:val="yellow"/></w:rPr></w:style>"#), "{written}");
    unchanged_parts(&doc, &before, &["word/styles.xml"]);
    assert_eq!(doc.add_style(&style("別名")).unwrap(), "a2");
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

#[test]
fn bookmark_split_preserves_run_children_in_order() {
    let children = r#"<w:t>a</w:t><w:noBreakHyphen/><w:softHyphen /><w:cr/><w:lastRenderedPageBreak /><w:t>b</w:t><w:tab/><w:br /><w:t>c</w:t>"#;
    let mut doc = open(&format!("<w:p><w:r>{children}</w:r></w:p>"), &[]);
    let needle = "\u{2011}\u{AD}\nb\t\nc";
    let m = doc.find(needle, &ReplaceOptions::default()).unwrap().remove(0);
    doc.add_bookmark("Keep", BookmarkRange::Match(&m)).unwrap();
    let output = xml(&doc, MAIN);
    let parsed = roxmltree::Document::parse(&output).unwrap();
    let runs: Vec<_> = parsed.descendants().filter(|n| n.has_tag_name((W_NS, "r"))).collect();
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0].children().filter(roxmltree::Node::is_element).count(), 1);
    let joined: String = runs.iter().flat_map(|r| r.children().filter(roxmltree::Node::is_element))
        .map(|n| &output[n.range()]).collect();
    assert_eq!(joined, children);
    assert_eq!(doc.paragraph(0).unwrap().text(), format!("a{needle}"));
}

#[test]
fn bookmark_split_copies_run_xml_space() {
    for space in ["preserve", "default"] {
        let body = format!(r#"<w:p xml:space="preserve"><w:r xml:space="{space}"><w:t>ab</w:t><w:tab/><w:t>cd</w:t></w:r></w:p>"#);
        let mut doc = open(&body, &[]);
        let m = doc.find("cd", &ReplaceOptions::default()).unwrap().remove(0);
        doc.add_bookmark("Mark", BookmarkRange::Match(&m)).unwrap();
        let output = xml(&doc, MAIN);
        assert!(output.contains(&format!(r#"<w:r xml:space="{space}"><w:t>ab</w:t><w:tab/></w:r>"#)));
        assert!(output.contains(&format!(r#"<w:r xml:space="{space}"><w:t>cd</w:t></w:r>"#)));
    }
}

#[test]
fn bookmark_range_checks_fields_and_hyperlink_endpoints() {
    let field = r#"<w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText>PAGE</w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>1</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r>"#;
    let hyperlink = r#"<w:hyperlink w:anchor="other"><w:r><w:t>middle</w:t></w:r></w:hyperlink>"#;
    let properties = r#"<w:pPr><w:pPrChange w:id="2"><w:pPr/></w:pPrChange></w:pPr>"#;
    let body = format!("<w:p>{properties}<w:r><w:t>before</w:t></w:r>{field}<w:r><w:t>left</w:t></w:r>{hyperlink}<w:r><w:t>right</w:t></w:r></w:p>");
    let mut doc = open(&body, &[]);
    for needle in ["before1left", "1"] {
        let m = doc.find(needle, &ReplaceOptions::default()).unwrap().remove(0);
        assert_matches!(refusals(&mut doc, |d| d.add_bookmark("Mark", BookmarkRange::Match(&m))), Error::InsideField);
    }
    assert_matches!(refusals(&mut doc, |d| d.add_bookmark("Mark", whole(0))), Error::InsideField);
    for needle in ["leftmid", "dleright"] {
        let m = doc.find(needle, &ReplaceOptions::default()).unwrap().remove(0);
        assert_matches!(refusals(&mut doc, |d| d.add_bookmark("Mark", BookmarkRange::Match(&m))), Error::UnsupportedContext { .. });
    }
    let m = doc.find("leftmiddleright", &ReplaceOptions::default()).unwrap().remove(0);
    doc.add_bookmark("Mark", BookmarkRange::Match(&m)).unwrap();
    let output = xml(&doc, MAIN);
    assert!(output.contains(properties));
    assert!(output.contains(field));
    assert!(output.contains(&format!(r#"<w:bookmarkStart w:id="3" w:name="Mark"/><w:r><w:t>left</w:t></w:r>{hyperlink}<w:r><w:t>right</w:t></w:r><w:bookmarkEnd w:id="3"/>"#)));
}

#[test]
fn bookmark_split_refuses_to_move_a_child_out_of_its_namespace_scope() {
    let body = format!(r#"<w:p><w:r xmlns:alias="{W_NS}"><w:t>before</w:t><alias:t>target</alias:t></w:r></w:p>"#);
    let mut doc = open(&body, &[]);
    let m = doc.find("target", &ReplaceOptions::default()).unwrap().remove(0);
    assert_matches!(
        refusals(&mut doc, |d| d.add_bookmark("Mark", BookmarkRange::Match(&m))),
        Error::Core(ooxml_core::Error::NotMovable { .. })
    );
}

#[test]
fn glossary_only_contributes_to_annotation_high_water() {
    let gloss = story_xml("glossaryDocument", r#"<w:docParts><w:docPart><w:docPartBody><w:p><w:bookmarkStart w:id="41" w:name="Mark"/><w:hyperlink w:anchor="Mark"/><w:bookmarkEnd w:id="41"/></w:p></w:docPartBody></w:docPart></w:docParts>"#);
    let mut doc = open("<w:p/>", &[("glossaryDocument", "glossary/document.xml", &gloss)]);
    let before = doc.to_bytes().unwrap();
    assert_matches!(refusals(&mut doc, |d| d.remove_bookmark("Mark")), Error::UnknownBookmark { .. });
    assert_eq!(doc.add_bookmark("Mark", whole(0)).unwrap(), 42);
    doc.remove_bookmark("Mark").unwrap();
    unchanged_parts(&doc, &before, &[MAIN]);
    assert!(!xml(&doc, MAIN).contains("bookmark"));
}

#[test]
fn settings_references_reserve_style_ids() {
    let settings = story_xml("settings", r#"<w:clickAndTypeStyle w:val="ClickStyle"/><w:defaultTableStyle w:val="TableStyle"/>"#);
    let styles = story_xml("styles", "");
    let mut doc = open("<w:p/>", &[("styles", "styles.xml", &styles), ("settings", "settings.xml", &settings)]);
    for id in ["clickstyle", "TABLESTYLE"] {
        let mut spec = style("Custom");
        spec.id = Some(id);
        assert_matches!(refusals(&mut doc, |d| d.add_style(&spec)), Error::StyleIdInUse { .. });
    }
    let before = doc.to_bytes().unwrap();
    assert_eq!(doc.add_style(&style("Click Style")).unwrap(), "ClickStyle0");
    assert_eq!(doc.add_style(&style("Table Style")).unwrap(), "TableStyle0");
    unchanged_parts(&doc, &before, &["word/styles.xml"]);
}

#[derive(Default, Debug, PartialEq, Eq)]
struct FixtureRegistry {
    definitions: std::collections::BTreeMap<(&'static str, String), usize>,
    occupied: std::collections::BTreeSet<(&'static str, String)>,
}

impl FixtureRegistry {
    fn read(bytes: &[u8]) -> Self {
        let mut registry = Self::default();
        for name in part_names(bytes).iter().filter(|n| n.starts_with("word/") && n.ends_with(".xml")) {
            let content = part(bytes, name);
            let Ok(content) = std::str::from_utf8(&content) else { continue };
            // The independent parser has DTD support disabled.
            let Ok(xml) = roxmltree::Document::parse(content) else { continue };
            for node in xml.descendants().filter(|n| n.tag_name().namespace() == Some(W_NS)) {
                let (kind, attr, definition) = match node.tag_name().name() {
                    "num" => ("num", "numId", true),
                    "numId" | "numIdMacAtCleanup" => ("num", "val", false),
                    "abstractNum" => ("abstract", "abstractNumId", true),
                    "abstractNumId" => ("abstract", "val", false),
                    "nsid" if node.parent().is_some_and(|p| p.has_tag_name((W_NS, "abstractNum"))) => ("nsid", "val", true),
                    "style" => ("style", "styleId", true),
                    "pStyle" | "rStyle" | "tblStyle" | "basedOn" | "next" | "link" | "styleLink"
                    | "numStyleLink" | "clickAndTypeStyle" | "defaultTableStyle" => ("style", "val", false),
                    "bookmarkStart" => ("annotation", "id", true),
                    "bookmarkEnd" | "commentRangeStart" | "commentRangeEnd" | "commentReference" | "comment"
                    | "ins" | "del" | "moveFrom" | "moveTo" | "rPrChange" | "pPrChange" | "sectPrChange"
                    | "tblPrChange" | "trPrChange" | "tcPrChange" | "tblGridChange" | "numberingChange"
                    | "cellIns" | "cellDel" | "cellMerge" | "moveFromRangeStart" | "moveFromRangeEnd"
                    | "moveToRangeStart" | "moveToRangeEnd" | "customXmlInsRangeStart" | "customXmlInsRangeEnd"
                    | "customXmlDelRangeStart" | "customXmlDelRangeEnd" | "customXmlMoveFromRangeStart"
                    | "customXmlMoveFromRangeEnd" | "customXmlMoveToRangeStart" | "customXmlMoveToRangeEnd"
                    | "permStart" | "permEnd" => ("annotation", "id", false),
                    _ => continue,
                };
                let Some(value) = node.attribute((W_NS, attr)) else { continue };
                let value = match kind {
                    "style" => value.to_lowercase(),
                    "nsid" => match u32::from_str_radix(value.trim(), 16) {
                        Ok(value) => value.to_string(),
                        Err(_) => continue,
                    },
                    _ => match value.trim().parse::<u64>() {
                        Ok(value) => value.to_string(),
                        Err(_) => continue,
                    },
                };
                let key = (kind, value);
                if definition {
                    *registry.definitions.entry(key.clone()).or_default() += 1;
                }
                registry.occupied.insert(key);
            }
        }
        registry
    }

    fn check_additions(&self, after: &Self, mut kinds: Vec<&str>) {
        let mut added = Vec::new();
        for (key, count) in &after.definitions {
            if !self.definitions.contains_key(key) {
                assert!(!self.occupied.contains(key), "allocated a referenced id: {key:?}");
                assert_eq!(*count, 1, "duplicate new id: {key:?}");
                added.push(key.0);
            }
        }
        for (key, count) in &self.definitions {
            assert_eq!(after.definitions.get(key), Some(count), "{key:?}");
        }
        added.sort_unstable();
        kinds.sort_unstable();
        assert_eq!(added, kinds);
    }
}

fn fixture_edit<T>(
    doc: &mut Document,
    allowed_parts: &[&str],
    edit: impl FnOnce(&mut Document) -> Result<T, Error>,
) -> Option<T> {
    let before = doc.to_bytes().unwrap();
    let result = edit(doc);
    let after = doc.to_bytes().unwrap();
    match result {
        Ok(value) => {
            unchanged_parts(doc, &before, allowed_parts);
            let old = part_names(&before);
            let new = part_names(&after);
            assert!(old.iter().all(|p| new.contains(p)));
            assert!(new.iter().all(|p| old.contains(p) || allowed_parts.contains(&p.as_str())));
            Some(value)
        }
        Err(err) => {
            assert_eq!(before, after, "{err}: fixture refusal changed ZIP bytes");
            None
        }
    }
}

fn related_part(doc: &Document, relationship: &str) -> Option<String> {
    let rels = doc.package().relationships(doc.main_part_name()).unwrap();
    let rel_type = format!("http://schemas.openxmlformats.org/officeDocument/2006/relationships/{relationship}");
    rels.by_type(&rel_type).find_map(|r| rels.resolve(r))
}

#[test]
fn fixture_sweep() {
    let mut successes = [0; 7];
    let story_texts = |doc: &Document| {
        doc.stories().map(|s| s.paragraphs().map(|p| p.text()).collect::<Vec<_>>()).collect::<Vec<_>>()
    };
    for stem in writable() {
        let mut doc = Document::from_bytes(fixture(stem)).unwrap();
        let texts = story_texts(&doc);
        let main = doc.main_part_name().to_owned();
        let registry = |doc: &Document| FixtureRegistry::read(&doc.to_bytes().unwrap());
        let initial = registry(&doc);
        let existing_numbering = related_part(&doc, "numbering");
        let mut allowed: Vec<&str> = existing_numbering.iter().map(String::as_str).collect();
        let added_numbering;
        let relationships;
        if existing_numbering.is_none() {
            let (dir, file) = main.rsplit_once('/').unwrap();
            added_numbering = format!("{dir}/numbering.xml");
            relationships = format!("{dir}/_rels/{file}.rels");
            allowed.extend([added_numbering.as_str(), relationships.as_str(), "[Content_Types].xml"]);
        }
        if let Some(num) = fixture_edit(&mut doc, &allowed, |d| d.create_list(ListTemplate::Decimal)) {
            successes[0] += 1;
            let created = registry(&doc);
            initial.check_additions(&created, vec!["num", "abstract", "nsid"]);
            let numbering = related_part(&doc, "numbering").unwrap();
            for i in 0..texts[0].len() {
                if fixture_edit(&mut doc, &[&main], |d| d.apply_list(p(i), num, 0)).is_none() {
                    continue;
                }
                successes[1] += 1;
                created.check_additions(&registry(&doc), vec![]);
                let before = registry(&doc);
                if fixture_edit(&mut doc, &[&main, &numbering], |d| d.restart_list(p(i), 3)).is_some() {
                    successes[2] += 1;
                    before.check_additions(&registry(&doc), vec!["num"]);
                }
                let before = registry(&doc);
                if fixture_edit(&mut doc, &[&main], |d| d.remove_from_list(p(i))).is_some() {
                    successes[3] += 1;
                    before.check_additions(&registry(&doc), vec![]);
                }
                break;
            }
        }
        let before = registry(&doc);
        for i in 0..texts[0].len() {
            if fixture_edit(&mut doc, &[&main], |d| d.add_bookmark("RegistrySweepBookmark", whole(i))).is_some() {
                successes[4] += 1;
                before.check_additions(&registry(&doc), vec!["annotation"]);
                if fixture_edit(&mut doc, &[&main], |d| d.remove_bookmark("RegistrySweepBookmark")).is_some() {
                    successes[5] += 1;
                    before.check_additions(&registry(&doc), vec![]);
                }
                break;
            }
        }
        let before = registry(&doc);
        let styles_part = related_part(&doc, "styles");
        let allowed: Vec<_> = styles_part.as_deref().into_iter().collect();
        if fixture_edit(&mut doc, &allowed, |d| d.add_style(&style("Registry Sweep Style"))).is_some() {
            successes[6] += 1;
            before.check_additions(&registry(&doc), vec!["style"]);
        }
        let reopened = Document::from_bytes(doc.to_bytes().unwrap()).unwrap();
        assert_eq!(story_texts(&reopened), texts, "{stem}");
    }
    assert!(successes.iter().all(|&count| count > 0), "{successes:?}");
}
