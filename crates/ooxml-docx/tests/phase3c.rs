mod common;

use std::{assert_matches, collections::BTreeSet};

use common::{W_NS, diff_range, docx, fixture, part};
use ooxml_docx::{
    Author, Document, Error, LocalDateTime, ParaRef, ParagraphFormat, ParagraphSpec, Place, Prop,
    RevisionKind, RowTemplate, RunFormat, RunPosition, RunRef, StoryIndex, TableRef, TextRange,
};
use roxmltree::Node;

const MAIN: &str = "word/document.xml";
const A: &str = "<w:p><w:r><w:t>Before αβ after</w:t></w:r></w:p>";
const B: &str = "<w:p w:rsidR='00C0FFEE'><w:r><w:t>Keep me</w:t></w:r></w:p>";

fn p(index: usize) -> ParaRef {
    ParaRef::new(StoryIndex::MAIN, index)
}

fn r(paragraph: usize, run: usize) -> RunRef {
    RunRef::new(p(paragraph), run)
}

fn author() -> Author {
    Author::new("Ada & Grace")
}

fn bold() -> RunFormat {
    RunFormat {
        bold: Prop::Set(true),
        ..RunFormat::default()
    }
}

fn xml(bytes: &[u8]) -> String {
    String::from_utf8(part(bytes, MAIN)).unwrap()
}

fn w(node: Node<'_, '_>, local: &str) -> bool {
    node.has_tag_name((W_NS, local))
}

fn text_view(p: Node<'_, '_>, accept: bool) -> String {
    p.descendants()
        .filter(|n| w(*n, "t") || w(*n, "delText") || w(*n, "tab") || w(*n, "br"))
        .filter(|n| {
            !n.ancestors()
                .any(|a| w(a, if accept { "del" } else { "ins" }))
        })
        .filter_map(|n| match n.tag_name().name() {
            "tab" => Some("\t"),
            "br" => Some("\n"),
            _ => n.text(),
        })
        .collect()
}

#[test]
fn text_views_w1_attributes_id_union_and_locality() {
    let bookmark = "<w:bookmarkStart w:id='89' w:name='high'/><w:bookmarkEnd w:id='89'/>";
    let header = format!("<w:p>{bookmark}</w:p>");
    let input = docx(
        &format!("{A}<!-- outside -->{B}"),
        Some("<w:trackRevisions/>"),
        Some(&header),
    );
    let mut doc = Document::from_bytes(input.clone()).unwrap();
    assert_matches!(doc.set_run_text(0, 0, "x"), Err(Error::TrackRevisionsOn));
    assert_eq!(doc.to_bytes().unwrap(), input);
    let date = LocalDateTime {
        wall_clock: "2026-10-11T09:10:11".parse().unwrap(),
        utc_offset_minutes: 540,
    };
    doc.track_changes(author(), Some(date))
        .unwrap()
        .set_run_text(0, 0, "New\tline")
        .unwrap();
    let output = doc.to_bytes().unwrap();
    let (before, after) = (xml(&input), xml(&output));
    let (start, end, _) = diff_range(before.as_bytes(), after.as_bytes()).unwrap();
    let run_start = before.find("<w:r><w:t>Before").unwrap();
    assert!(start >= run_start && end <= run_start + "<w:r><w:t>Before αβ after</w:t></w:r>".len());
    for name in ["word/settings.xml", "word/header1.xml"] {
        assert_eq!(part(&input, name), part(&output, name));
    }
    let reopened = Document::from_bytes(output).unwrap();
    let p = reopened.paragraph(0).unwrap();
    let old = p.run(0).unwrap().revision().unwrap();
    let new = p.run(1).unwrap().revision().unwrap();
    assert_eq!((old.kind, old.id), (RevisionKind::Deletion, Some("90")));
    assert_eq!((new.kind, new.id), (RevisionKind::Insertion, Some("91")));
    assert_eq!(new.author, Some("Ada & Grace"));
    assert_eq!(new.date, Some("2026-10-11T09:10:11Z"));
    let tree = roxmltree::Document::parse(&after).unwrap();
    let p = tree.descendants().find(|n| w(*n, "p")).unwrap();
    let mut baseline = Document::from_bytes(docx(A, None, None)).unwrap();
    baseline.set_run_text(0, 0, "New\tline").unwrap();
    assert_eq!(text_view(p, true), baseline.paragraph(0).unwrap().text());
    assert_eq!(text_view(p, false), "Before αβ after");
}

#[test]
fn insert_and_split_delete_text() {
    let mut doc = Document::from_bytes(docx(&format!("{A}{B}"), None, None)).unwrap();
    let mut session = doc.track_changes(author(), None).unwrap();
    session
        .delete_text(TextRange::with_offsets(r(0, 0), 7, r(0, 0), 11))
        .unwrap();
    session
        .insert_text(RunPosition::EndOf(p(1)), " Inserted")
        .unwrap();
    let after = xml(&doc.to_bytes().unwrap());
    let tree = roxmltree::Document::parse(&after).unwrap();
    let views: Vec<_> = tree
        .descendants()
        .filter(|n| w(*n, "p"))
        .map(|p| (text_view(p, true), text_view(p, false)))
        .collect();
    assert_eq!(
        views,
        [
            ("Before  after".into(), "Before αβ after".into()),
            ("Keep me Inserted".into(), "Keep me".into()),
        ]
    );
}

#[test]
fn n1_moves_original_mark_and_places_ins_before_formatting() {
    let old = "<w:pPr><w:numPr><w:numId w:val='12'/></w:numPr><w:rPr><w:b/></w:rPr><w:sectPr><w:type w:val='continuous'/></w:sectPr></w:pPr>";
    let input = docx(
        &format!("<w:p>{old}<w:r><w:t>Old</w:t></w:r></w:p>"),
        None,
        None,
    );
    let mut doc = Document::from_bytes(input).unwrap();
    doc.track_changes(author(), None)
        .unwrap()
        .append_paragraph("New")
        .unwrap();
    let edited = xml(&doc.to_bytes().unwrap());
    let tree = roxmltree::Document::parse(&edited).unwrap();
    let paragraphs: Vec<_> = tree.descendants().filter(|n| w(*n, "p")).collect();
    assert_eq!(paragraphs.len(), 2);
    let ppr = paragraphs[0].first_element_child().unwrap();
    let mark = ppr.children().find(|n| w(*n, "rPr")).unwrap();
    assert_eq!(
        mark.children()
            .filter(Node::is_element)
            .map(|n| n.tag_name().name())
            .collect::<Vec<_>>(),
        ["ins", "b"]
    );
    assert!(!ppr.children().any(|n| w(n, "sectPr")));
    assert!(ppr.descendants().any(|n| w(n, "numId")));
    // ADR-0015 (u): the rejected-view model keeps the following mark.
    // Whether Word keeps that mark requires Reject All in Word.
    let surviving = paragraphs[1].first_element_child().unwrap();
    assert_eq!(&edited[surviving.range()], old);
    assert_eq!(
        text_view(paragraphs[0], false) + &text_view(paragraphs[1], false),
        "Old"
    );
    assert_eq!(text_view(paragraphs[1], true), "New");
}

#[test]
fn paragraph_insertion_and_deletion_marks() {
    let mut doc = Document::from_bytes(docx(&format!("{A}{B}"), None, None)).unwrap();
    doc.track_changes(author(), None)
        .unwrap()
        .insert_paragraph(Place::Before(p(0)), "First", &ParagraphSpec::Plain)
        .unwrap();
    let before = doc.to_bytes().unwrap();
    assert_matches!(
        doc.track_changes(author(), None).unwrap().insert_paragraph(
            Place::Before(p(0)),
            "Again",
            &ParagraphSpec::Plain
        ),
        Err(Error::InsideRevision { .. })
    );
    assert_eq!(doc.to_bytes().unwrap(), before);
    doc.track_changes(author(), None)
        .unwrap()
        .delete_paragraph(p(1))
        .unwrap();
    let after = xml(&doc.to_bytes().unwrap());
    let tree = roxmltree::Document::parse(&after).unwrap();
    let paragraphs: Vec<_> = tree.descendants().filter(|n| w(*n, "p")).collect();
    assert_eq!(text_view(paragraphs[0], false), "");
    assert_eq!(text_view(paragraphs[0], true), "First");
    assert_eq!(text_view(paragraphs[1], false), "Before αβ after");
    assert_eq!(text_view(paragraphs[1], true), "");
    assert!(
        paragraphs[1]
            .descendants()
            .any(|n| w(n, "del") && n.parent().is_some_and(|p| w(p, "rPr")))
    );
}

#[test]
fn formatting_snapshots_and_r2_refusals() {
    use ooxml_docx::Alignment;
    let a = "<w:p><w:pPr><w:jc w:val='left'/></w:pPr><w:r><w:rPr><w:i/></w:rPr><w:t>First</w:t></w:r></w:p>";
    let mut doc = Document::from_bytes(docx(&format!("{a}{B}"), None, None)).unwrap();
    let pf = ParagraphFormat {
        alignment: Prop::Set(Alignment::Center),
        ..ParagraphFormat::default()
    };
    let mut session = doc.track_changes(author(), None).unwrap();
    session.set_run_properties(r(0, 0), &bold()).unwrap();
    session.set_paragraph_properties(p(1), &pf).unwrap();
    let before = doc.to_bytes().unwrap();
    let edited = xml(&before);
    let tree = roxmltree::Document::parse(&edited).unwrap();
    let rpr = tree.descendants().find(|n| w(*n, "rPrChange")).unwrap();
    assert!(rpr.descendants().any(|n| w(n, "i")));
    assert!(!rpr.descendants().any(|n| w(n, "b")));
    assert!(rpr.parent().unwrap().children().any(|n| w(n, "b")));
    let ppr = tree.descendants().find(|n| w(*n, "pPrChange")).unwrap();
    assert_eq!(
        ppr.first_element_child()
            .unwrap()
            .children()
            .filter(Node::is_element)
            .count(),
        0
    );
    assert!(
        ppr.parent()
            .unwrap()
            .children()
            .any(|n| w(n, "jc") && n.attribute((W_NS, "val")) == Some("center"))
    );
    for i in [0, 1] {
        assert_matches!(
            doc.set_run_text(i, 0, "Refused"),
            Err(Error::InsideRevision { .. })
        );
        assert_eq!(doc.to_bytes().unwrap(), before);
    }
}

fn table() -> String {
    format!(
        "<w:tbl><w:tblGrid><w:gridCol w:w='1234'/></w:tblGrid><w:tr><w:tc>{A}</w:tc></w:tr><w:tr><w:tc>{B}</w:tc></w:tr></w:tbl>"
    )
}

fn row_view(xml: &str, accept: bool) -> Vec<String> {
    let tree = roxmltree::Document::parse(xml).unwrap();
    tree.descendants()
        .filter(|n| w(*n, "tr"))
        .filter(|n| {
            !n.children()
                .filter(|n| w(*n, "trPr"))
                .flat_map(|n| n.children())
                .any(|n| w(n, if accept { "del" } else { "ins" }))
        })
        .map(|n| text_view(n, accept))
        .collect()
}

#[test]
fn row_views_and_last_block_refusal() {
    let table = TableRef::new(StoryIndex::MAIN, 0);
    let input = docx(&format!("{}<w:sectPr/>", crate::table()), None, None);
    let mut doc = Document::from_bytes(input.clone()).unwrap();
    assert_matches!(
        doc.track_changes(author(), None)
            .unwrap()
            .append_paragraph("Last"),
        Err(Error::NoFollowingParagraph)
    );
    assert_eq!(doc.to_bytes().unwrap(), input);
    let mut baseline = Document::from_bytes(input.clone()).unwrap();
    baseline
        .insert_rows(table, 2, 1, RowTemplate::Above)
        .unwrap();
    baseline.delete_rows(table, 0..1).unwrap();
    let mut session = doc.track_changes(author(), None).unwrap();
    session
        .insert_rows(table, 2, 1, RowTemplate::Above)
        .unwrap();
    session.delete_rows(table, 0..1).unwrap();
    let output = xml(&doc.to_bytes().unwrap());
    assert!(doc.table_row(table, 0).unwrap().is_deleted());
    assert_eq!(doc.table_row_count(table), Some(3));
    assert_eq!(
        row_view(&output, true),
        row_view(&xml(&baseline.to_bytes().unwrap()), true)
    );
    assert_eq!(row_view(&output, false), row_view(&xml(&input), false));
}

#[test]
fn invalid_metadata_ranges_and_exhaustion_are_atomic() {
    let input = docx(A, None, None);
    let mut doc = Document::from_bytes(input.clone()).unwrap();
    assert_matches!(
        doc.track_changes(Author::new(""), None),
        Err(Error::InvalidValue { .. })
    );
    assert_eq!(doc.to_bytes().unwrap(), input);
    assert_matches!(
        doc.track_changes(author(), None)
            .unwrap()
            .delete_text(TextRange::with_offsets(r(0, 0), 8, r(0, 0), 11)),
        Err(Error::InvalidTextRange)
    );
    assert_eq!(doc.to_bytes().unwrap(), input);
    let input = docx(
        &format!("<w:p><w:bookmarkEnd w:id='2147483646'/></w:p>{A}"),
        None,
        None,
    );
    let mut doc = Document::from_bytes(input.clone()).unwrap();
    assert_matches!(
        doc.track_changes(author(), None)
            .unwrap()
            .set_run_text(1, 0, "Two ids"),
        Err(Error::AnnotationIdsExhausted)
    );
    assert_eq!(doc.to_bytes().unwrap(), input);
    doc.track_changes(author(), None)
        .unwrap()
        .set_run_text(1, 0, "")
        .unwrap();
    assert_eq!(
        doc.run_at(r(1, 0)).unwrap().revision().unwrap().id,
        Some("2147483647")
    );
}

#[test]
fn fixture_sweep() {
    let (mut opened, mut edited, mut refused) = (0, [0; 3], [0; 3]);
    for name in common::ALL {
        let input = fixture(name);
        if Document::from_bytes(input.clone()).is_err() {
            continue;
        }
        opened += 1;
        for operation in 0..3 {
            let mut doc = Document::from_bytes(input.clone()).unwrap();
            let unchanged = doc.to_bytes().unwrap();
            let target = doc
                .paragraphs()
                .flat_map(|p| p.runs())
                .find(|r| r.check_settable().is_ok() && !r.text().trim().is_empty())
                .map(|r| r.run_ref());
            let result = (|| {
                let target = target.ok_or(Error::InvalidTextRange)?;
                let mut session = doc.track_changes(Author::new("D3c corpus"), None)?;
                match operation {
                    0 => session.insert_text(RunPosition::After(target), " revision inserted"),
                    1 => session.delete_text(TextRange::new(target, target)),
                    _ => session.set_run_properties(
                        target,
                        &RunFormat {
                            color: Prop::Set(ooxml_docx::Color::Rgb(0x11, 0x22, 0x33)),
                            ..RunFormat::default()
                        },
                    ),
                }
            })();
            let output = doc.to_bytes().unwrap();
            if result.is_err() {
                refused[operation] += 1;
                assert!(output == unchanged, "{name}: {result:?}");
                continue;
            }
            edited[operation] += 1;
            Document::from_bytes(output.clone()).unwrap_or_else(|e| panic!("{name}: {e}"));
            let edited_xml = xml(&output);
            let tree = roxmltree::Document::parse(&edited_xml).unwrap();
            let marks: Vec<_> = tree
                .descendants()
                .filter(|n| n.attribute((W_NS, "author")) == Some("D3c corpus"))
                .collect();
            assert!(!marks.is_empty(), "{name}");
            let ids: BTreeSet<_> = marks
                .iter()
                .map(|n| n.attribute((W_NS, "id")).unwrap())
                .collect();
            assert_eq!(ids.len(), marks.len(), "{name}");
            for id in ids {
                assert_eq!(
                    tree.descendants()
                        .filter(|n| n.attribute((W_NS, "id")) == Some(id)
                            && [
                                "ins",
                                "del",
                                "rPrChange",
                                "pPrChange",
                                "bookmarkStart",
                                "bookmarkEnd"
                            ]
                            .contains(&n.tag_name().name()))
                        .count(),
                    1,
                    "{name}: {id}"
                );
            }
        }
    }
    eprintln!(
        "D3c fixture sweep: opened {opened}/{}; wrote {edited:?}; refused {refused:?}",
        common::ALL.len()
    );
}

#[test]
fn protected_modes_formatting_and_nontext_refusals() {
    let protected = |mode| {
        format!("<w:trackRevisions/><w:documentProtection w:edit='{mode}' w:enforcement='1'/>")
    };
    let settings = protected("trackedChanges");
    let input = docx(A, Some(&settings), None);
    let mut doc = Document::from_bytes(input.clone()).unwrap();
    doc.track_changes(author(), None)
        .unwrap()
        .set_run_text(0, 0, "Protected edit")
        .unwrap();
    assert_eq!(
        part(&doc.to_bytes().unwrap(), "word/settings.xml"),
        part(&input, "word/settings.xml")
    );
    let settings = format!("{}{}", protected("trackedChanges"), protected("readOnly"));
    let input = docx(A, Some(&settings), None);
    let mut doc = Document::from_bytes(input.clone()).unwrap();
    assert_matches!(
        doc.track_changes(author(), None),
        Err(Error::DocumentProtected { edit }) if edit == "readOnly"
    );
    assert_eq!(doc.to_bytes().unwrap(), input);

    let body = r#"<w:p><w:r><w:drawing/></w:r></w:p>"#;
    let input = docx(&format!("{body}{B}"), None, None);
    let mut doc = Document::from_bytes(input.clone()).unwrap();
    assert_matches!(
        doc.track_changes(author(), None)
            .unwrap()
            .set_run_text(0, 0, "invalid"),
        Err(Error::RunNotTextOnly { .. })
    );
    assert_eq!(doc.to_bytes().unwrap(), input);
    assert_matches!(
        doc.track_changes(author(), None).unwrap().insert_paragraph(
            Place::After(p(0)),
            "invalid",
            &ParagraphSpec::LikeNeighbour
        ),
        Err(Error::TrackedParagraphFormatting)
    );
    assert_eq!(doc.to_bytes().unwrap(), input);
    assert_matches!(
        doc.track_changes(author(), None)
            .unwrap()
            .delete_paragraph(p(1)),
        Err(Error::NoFollowingParagraph)
    );
    assert_eq!(doc.to_bytes().unwrap(), input);
}

#[test]
fn expanded_r2_refuses_a_revised_range_and_table_properties() {
    let body = r#"<w:p><w:moveToRangeStart w:id="11" w:name="m"/><w:r><w:t>Range</w:t></w:r><w:moveToRangeEnd w:id="11"/></w:p>"#;
    let input = docx(body, None, None);
    let mut doc = Document::from_bytes(input.clone()).unwrap();
    assert_matches!(
        doc.set_run_text(0, 0, "Refused"),
        Err(Error::InsideRevision { .. })
    );
    assert_eq!(doc.to_bytes().unwrap(), input);
    let table = table().replace(
        "<w:tblGrid>",
        "<w:tblPr><w:tblPrChange w:id='12'/></w:tblPr><w:tblGrid>",
    );
    let input = docx(&table, None, None);
    let mut doc = Document::from_bytes(input.clone()).unwrap();
    assert_matches!(
        doc.set_run_text(0, 0, "Refused"),
        Err(Error::InsideRevision { .. })
    );
    assert_eq!(doc.to_bytes().unwrap(), input);
}

#[test]
fn section_header_ref_is_moved_with_the_original_mark() {
    let old = r#"<w:pPr><w:sectPr xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:headerReference w:type="default" r:id="rId2"/></w:sectPr></w:pPr>"#;
    let input = docx(
        &format!("<w:p>{old}<w:r><w:t>Old</w:t></w:r></w:p>"),
        None,
        Some(B),
    );
    let mut doc = Document::from_bytes(input.clone()).unwrap();
    doc.track_changes(author(), None)
        .unwrap()
        .append_paragraph("New")
        .unwrap();
    let output = doc.to_bytes().unwrap();
    let edited = xml(&output);
    assert_eq!(edited.matches(old).count(), 1);
    assert_eq!(
        part(&input, "word/_rels/document.xml.rels"),
        part(&output, "word/_rels/document.xml.rels")
    );
    assert_eq!(
        part(&input, "word/header1.xml"),
        part(&output, "word/header1.xml")
    );
}

#[test]
fn ignorable_old_formatting_is_refused_before_write() {
    let body = r#"<w:p xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml" mc:Ignorable="w14"><w:r><w:rPr><w14:ligatures w14:val="standard"/></w:rPr><w:t>Old</w:t></w:r></w:p>"#;
    let input = docx(body, None, None);
    let mut doc = Document::from_bytes(input.clone()).unwrap();
    assert_matches!(
        doc.track_changes(author(), None)
            .unwrap()
            .set_run_properties(r(0, 0), &bold()),
        Err(Error::Core(ooxml_core::Error::NotDetachable { .. }))
    );
    assert_eq!(doc.to_bytes().unwrap(), input);
}
