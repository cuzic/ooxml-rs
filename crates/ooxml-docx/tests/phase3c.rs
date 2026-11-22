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

fn assert_local(before: &str, after: &str, allowed: std::ops::Range<usize>) {
    if allowed.is_empty() {
        // An insertion can share bytes with the original following tag.
        assert!(after.len() > before.len());
        assert!(after.starts_with(&before[..allowed.start]));
        assert!(after.ends_with(&before[allowed.end..]));
        return;
    }
    let (start, end, _) = diff_range(before.as_bytes(), after.as_bytes()).unwrap();
    assert!(
        allowed.start <= start && end <= allowed.end,
        "changed {start}..{end} outside {allowed:?}",
    );
}

fn span(xml: &str, tag: &str, index: usize) -> std::ops::Range<usize> {
    roxmltree::Document::parse(xml)
        .unwrap()
        .descendants()
        .filter(|n| w(*n, tag))
        .nth(index)
        .unwrap()
        .range()
}

fn format_view(
    xml: &str,
    accept: bool,
    element: &str,
    prop: &str,
    change: &str,
) -> Vec<Vec<String>> {
    let tree = roxmltree::Document::parse(xml).unwrap();
    tree.descendants()
        .filter(|n| w(*n, element))
        .map(|n| {
            let Some(live) = n.children().find(|n| w(*n, prop)) else {
                return Vec::new();
            };
            let snapshot = live.children().find(|n| w(*n, change));
            let previous = snapshot.and_then(|n| n.children().find(|n| w(*n, prop)));
            let mut result: Vec<_> = if let Some(previous) = previous.filter(|_| !accept) {
                previous.children().filter(Node::is_element).collect()
            } else {
                live.children()
                    .filter(|n| n.is_element() && !w(*n, change))
                    .collect()
            };
            if !accept && previous.is_some() && prop == "pPr" {
                result.extend(live.children().filter(|n| w(*n, "rPr") || w(*n, "sectPr")));
            }
            result
                .iter()
                .map(|n| {
                    let mut attrs: Vec<_> = n
                        .attributes()
                        .map(|a| (a.namespace(), a.name(), a.value()))
                        .collect();
                    attrs.sort_unstable();
                    format!("{:?}:{attrs:?}", n.tag_name())
                })
                .collect()
        })
        .collect()
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

fn paragraph_view(xml: &str, accept: bool) -> Vec<(String, Vec<String>)> {
    let tree = roxmltree::Document::parse(xml).unwrap();
    let mut paragraphs = Vec::new();
    let mut text = String::new();
    for paragraph in tree.descendants().filter(|n| w(*n, "p")) {
        text.push_str(&text_view(paragraph, accept));
        let properties = paragraph.children().find(|n| w(*n, "pPr"));
        let mark = properties.and_then(|p| p.children().find(|n| w(*n, "rPr")));
        let removed_mark = if accept { "del" } else { "ins" };
        // The N1 rejected-view model keeps the following mark after merging.
        if mark.is_some_and(|m| m.children().any(|n| w(n, removed_mark))) {
            continue;
        }
        let properties = properties
            .into_iter()
            .flat_map(|p| p.descendants().skip(1))
            .filter(Node::is_element)
            .filter(|n| !n.ancestors().any(|n| w(n, "ins") || w(n, "del")))
            .filter(|n| {
                !w(*n, "rPr")
                    || n.children()
                        .any(|c| c.is_element() && !w(c, "ins") && !w(c, "del"))
            })
            .map(|n| {
                let mut attrs: Vec<_> = n
                    .attributes()
                    .map(|a| (a.namespace(), a.name(), a.value()))
                    .collect();
                attrs.sort_unstable();
                format!("{:?}:{attrs:?}", n.tag_name())
            })
            .collect();
        paragraphs.push((std::mem::take(&mut text), properties));
    }
    assert!(
        text.is_empty(),
        "a marked paragraph must have a following mark"
    );
    paragraphs
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
    let input = docx(&format!("{A}<!-- separator -->{B}"), None, None);
    let mut doc = Document::from_bytes(input.clone()).unwrap();
    doc.track_changes(author(), None)
        .unwrap()
        .delete_text(TextRange::with_offsets(r(0, 0), 7, r(0, 0), 11))
        .unwrap();
    let before = xml(&input);
    let deleted = xml(&doc.to_bytes().unwrap());
    assert_local(&before, &deleted, span(&before, "r", 0));
    doc.track_changes(author(), None)
        .unwrap()
        .insert_text(RunPosition::EndOf(p(1)), " Inserted")
        .unwrap();
    let after = xml(&doc.to_bytes().unwrap());
    let tree = roxmltree::Document::parse(&after).unwrap();
    let end = span(&deleted, "r", 3).end;
    assert_local(&deleted, &after, end..end);
    let revision = tree.descendants().find(|n| w(*n, "ins")).unwrap();
    let date = revision.attribute((W_NS, "date")).unwrap();
    chrono::NaiveDateTime::parse_from_str(date, "%Y-%m-%dT%H:%M:%SZ").unwrap();
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
    let mut doc = Document::from_bytes(input.clone()).unwrap();
    doc.track_changes(author(), None)
        .unwrap()
        .append_paragraph("New")
        .unwrap();
    let edited = xml(&doc.to_bytes().unwrap());
    let before = xml(&input);
    assert_local(&before, &edited, span(&before, "p", 0));
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
    let mut baseline = Document::from_bytes(input.clone()).unwrap();
    baseline
        .insert_paragraph(Place::After(p(0)), "New", &ParagraphSpec::LikeNeighbour)
        .unwrap();
    assert_eq!(
        paragraph_view(&edited, true),
        paragraph_view(&xml(&baseline.to_bytes().unwrap()), true)
    );
    assert_eq!(
        paragraph_view(&edited, false),
        paragraph_view(&xml(&input), true)
    );
}

#[test]
fn paragraph_insertion_and_deletion_marks() {
    let input = docx(&format!("{A}{B}"), None, None);
    let mut doc = Document::from_bytes(input.clone()).unwrap();
    doc.track_changes(author(), None)
        .unwrap()
        .insert_paragraph(Place::Before(p(0)), "First", &ParagraphSpec::Plain)
        .unwrap();
    let before = doc.to_bytes().unwrap();
    let original = xml(&input);
    let start = span(&original, "p", 0).start;
    assert_local(&original, &xml(&before), start..start);
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
    let before_delete = xml(&before);
    assert_local(&before_delete, &after, span(&before_delete, "p", 1));
    let tree = roxmltree::Document::parse(&after).unwrap();
    let paragraphs: Vec<_> = tree.descendants().filter(|n| w(*n, "p")).collect();
    assert_eq!(text_view(paragraphs[0], false), "");
    assert_eq!(text_view(paragraphs[0], true), "First");
    assert_eq!(text_view(paragraphs[1], false), "Before αβ after");
    assert_eq!(text_view(paragraphs[1], true), "");
    let untracked = docx(
        &format!("<w:p><w:r><w:t>First</w:t></w:r></w:p>{B}"),
        None,
        None,
    );
    assert_eq!(
        paragraph_view(&after, true),
        paragraph_view(&xml(&untracked), true)
    );
    assert_eq!(
        paragraph_view(&after, false),
        paragraph_view(&xml(&input), true)
    );
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
    let b = B.replace("<w:r>", "<w:pPr><w:jc w:val='right'/></w:pPr><w:r>");
    let input = docx(&format!("{a}<!-- separator -->{b}"), None, None);
    let mut doc = Document::from_bytes(input.clone()).unwrap();
    let pf = ParagraphFormat {
        alignment: Prop::Set(Alignment::Center),
        ..ParagraphFormat::default()
    };
    let original = xml(&input);
    doc.track_changes(author(), None)
        .unwrap()
        .set_run_properties(r(0, 0), &bold())
        .unwrap();
    let run_edited = xml(&doc.to_bytes().unwrap());
    assert_local(&original, &run_edited, span(&original, "rPr", 0));
    doc.track_changes(author(), None)
        .unwrap()
        .set_paragraph_properties(p(1), &pf)
        .unwrap();
    let before = doc.to_bytes().unwrap();
    let edited = xml(&before);
    assert_local(&run_edited, &edited, span(&run_edited, "pPr", 1));

    let mut baseline = Document::from_bytes(input).unwrap();
    baseline.format_run(r(0, 0), &bold()).unwrap();
    baseline.format_paragraph(p(1), &pf).unwrap();
    let untracked = xml(&baseline.to_bytes().unwrap());
    for (element, prop, change) in [("r", "rPr", "rPrChange"), ("p", "pPr", "pPrChange")] {
        assert_eq!(
            format_view(&edited, true, element, prop, change),
            format_view(&untracked, true, element, prop, change),
        );
        assert_eq!(
            format_view(&edited, false, element, prop, change),
            format_view(&original, true, element, prop, change),
        );
    }
    let tree = roxmltree::Document::parse(&edited).unwrap();
    let rpr = tree.descendants().find(|n| w(*n, "rPrChange")).unwrap();
    assert!(rpr.descendants().any(|n| w(n, "i")));
    assert!(!rpr.descendants().any(|n| w(n, "b")));
    assert!(rpr.parent().unwrap().children().any(|n| w(n, "b")));
    let ppr = tree.descendants().find(|n| w(*n, "pPrChange")).unwrap();
    assert!(
        ppr.descendants()
            .any(|n| w(n, "jc") && n.attribute((W_NS, "val")) == Some("right"))
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
    doc.track_changes(author(), None)
        .unwrap()
        .insert_rows(table, 2, 1, RowTemplate::Above)
        .unwrap();
    let original = xml(&input);
    let inserted = xml(&doc.to_bytes().unwrap());
    let end = span(&original, "tr", 1).end;
    assert_local(&original, &inserted, end..end);
    doc.track_changes(author(), None)
        .unwrap()
        .delete_rows(table, 0..1)
        .unwrap();
    let output = xml(&doc.to_bytes().unwrap());
    assert_local(&inserted, &output, span(&inserted, "tr", 0));
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

#[test]
fn revision_attributes_get_a_safe_prefix_in_default_wml() {
    let input = docx(A, None, None);
    let default_wml = xml(&input)
        .replace("xmlns:w=", "xmlns=")
        .replace("<w:", "<")
        .replace("</w:", "</")
        .replace("<document ", "<document xmlns:w='urn:reserved' ");
    let mut doc =
        Document::from_bytes(common::with_part(&input, MAIN, default_wml.as_bytes())).unwrap();
    doc.track_changes(author(), None)
        .unwrap()
        .set_run_text(0, 0, "New")
        .unwrap();
    let after = xml(&doc.to_bytes().unwrap());
    let tree = roxmltree::Document::parse(&after).unwrap();
    let ins = tree.descendants().find(|n| w(*n, "ins")).unwrap();
    assert_eq!(ins.attribute((W_NS, "author")), Some("Ada & Grace"));
    assert!(after.contains("xmlns:w='urn:reserved'"));
}

#[test]
fn append_inside_cell_moves_the_cell_paragraph_mark() {
    let input = docx(&format!("{}{B}", table()), None, None);
    let mut doc = Document::from_bytes(input.clone()).unwrap();
    let original = xml(&input);
    doc.track_changes(author(), None)
        .unwrap()
        .insert_paragraph(Place::After(p(0)), "Cell append", &ParagraphSpec::Plain)
        .unwrap();
    let edited = xml(&doc.to_bytes().unwrap());
    assert_local(&original, &edited, span(&original, "p", 0));
    let tree = roxmltree::Document::parse(&edited).unwrap();
    let cell = tree.descendants().find(|n| w(*n, "tc")).unwrap();
    let paragraphs: Vec<_> = cell.children().filter(|n| w(*n, "p")).collect();
    assert_eq!(paragraphs.len(), 2);
    assert!(
        paragraphs[0]
            .descendants()
            .any(|n| w(n, "ins") && w(n.parent().unwrap(), "rPr"))
    );
    assert!(paragraphs[1].children().any(|n| w(n, "ins")));

    let mut baseline = Document::from_bytes(input).unwrap();
    baseline
        .insert_paragraph(
            Place::After(p(0)),
            "Cell append",
            &ParagraphSpec::LikeNeighbour,
        )
        .unwrap();
    assert_eq!(
        paragraph_view(&edited, true),
        paragraph_view(&xml(&baseline.to_bytes().unwrap()), true)
    );
    assert_eq!(
        paragraph_view(&edited, false),
        paragraph_view(&original, true)
    );
}

#[test]
fn paragraph_deletion_refuses_a_section_break() {
    let with_break = A.replace(
        "<w:p>",
        "<w:p><w:pPr><w:sectPr><w:type w:val='nextPage'/></w:sectPr></w:pPr>",
    );
    let input = docx(&format!("{with_break}{B}<w:sectPr/>"), None, None);
    let mut doc = Document::from_bytes(input.clone()).unwrap();
    assert_matches!(
        doc.track_changes(author(), None)
            .unwrap()
            .delete_paragraph(p(0)),
        Err(Error::TrackedSectionBreakDeletion)
    );
    assert_eq!(doc.to_bytes().unwrap(), input);
}

#[test]
fn row_deletion_refuses_nested_run_content() {
    let table_ref = TableRef::new(StoryIndex::MAIN, 0);
    for (content, expected) in [
        (
            "<w:ruby><w:rt><w:r><w:t>かん</w:t></w:r></w:rt><w:rubyBase><w:r><w:t>漢</w:t></w:r></w:rubyBase></w:ruby>",
            "ruby",
        ),
        (
            "<mc:AlternateContent><mc:Choice Requires='w'><w:t>Choice</w:t></mc:Choice><mc:Fallback><w:t>Fallback</w:t></mc:Fallback></mc:AlternateContent>",
            "AlternateContent",
        ),
    ] {
        let body = table().replace("<w:t>Before αβ after</w:t>", content);
        let input = docx(&body, None, None);
        let mut doc = Document::from_bytes(input.clone()).unwrap();
        assert_matches!(
            doc.track_changes(author(), None).unwrap().delete_rows(table_ref, 0..1),
            Err(Error::RunNotTextOnly { child }) if child.ends_with(expected)
        );
        assert_eq!(doc.to_bytes().unwrap(), input);
    }
}

#[test]
fn successive_deletions_preserve_a_live_row() {
    let input = docx(&table(), None, None);
    let table = TableRef::new(StoryIndex::MAIN, 0);
    let mut doc = Document::from_bytes(input).unwrap();
    doc.track_changes(author(), None)
        .unwrap()
        .delete_rows(table, 0..1)
        .unwrap();
    let before = doc.to_bytes().unwrap();
    let mut reopened = Document::from_bytes(before.clone()).unwrap();
    assert_matches!(
        reopened
            .track_changes(author(), None)
            .unwrap()
            .delete_rows(table, 1..2),
        Err(Error::TableWouldBeEmpty)
    );
    assert_eq!(reopened.to_bytes().unwrap(), before);
}

#[test]
fn outer_row_deletion_marks_nested_rows() {
    let nested = table();
    let body = table().replacen("<w:tc>", &format!("<w:tc>{nested}"), 1);
    let input = docx(&body, None, None);
    let mut doc = Document::from_bytes(input).unwrap();
    let table = TableRef::new(StoryIndex::MAIN, 0);
    doc.track_changes(author(), None)
        .unwrap()
        .delete_rows(table, 0..1)
        .unwrap();
    let edited = xml(&doc.to_bytes().unwrap());
    let tree = roxmltree::Document::parse(&edited).unwrap();
    let deleted: Vec<_> = tree
        .descendants()
        .filter(|n| w(*n, "tr"))
        .map(|row| {
            row.children()
                .filter(|n| w(*n, "trPr"))
                .flat_map(|n| n.children())
                .any(|n| w(n, "del"))
        })
        .collect();
    assert_eq!(deleted, [true, true, true, false]);
}

#[test]
fn tracked_structure_refuses_move_and_custom_xml_ranges() {
    let table_ref = TableRef::new(StoryIndex::MAIN, 0);
    for kind in [
        "moveFrom",
        "moveTo",
        "customXmlIns",
        "customXmlDel",
        "customXmlMoveFrom",
        "customXmlMoveTo",
    ] {
        let start = format!("<w:{kind}RangeStart w:id='11' w:name='range'/>");
        let end = format!("<w:{kind}RangeEnd w:id='11'/>");
        let rows = table()
            .replacen("<w:tr>", &format!("{start}<w:tr>"), 1)
            .replacen("</w:tr>", &format!("</w:tr>{end}"), 1);
        let input = docx(&rows, None, None);
        let mut doc = Document::from_bytes(input.clone()).unwrap();
        assert_matches!(
            doc.track_changes(author(), None)
                .unwrap()
                .delete_rows(table_ref, 0..1),
            Err(Error::InsideRevision { .. })
        );
        assert_eq!(doc.to_bytes().unwrap(), input);
        assert_matches!(
            doc.track_changes(author(), None).unwrap().insert_rows(
                table_ref,
                1,
                1,
                RowTemplate::Above
            ),
            Err(Error::InsideRevision { .. })
        );
        assert_eq!(doc.to_bytes().unwrap(), input);

        let input = docx(&format!("{start}{A}{end}{B}"), None, None);
        let mut doc = Document::from_bytes(input.clone()).unwrap();
        assert_matches!(
            doc.track_changes(author(), None).unwrap().delete_paragraph(p(0)),
            Err(Error::InsideRevision { .. })
        );
        assert_eq!(doc.to_bytes().unwrap(), input);
        assert_matches!(
            doc.track_changes(author(), None).unwrap().insert_paragraph(
                Place::Before(p(0)),
                "Refused",
                &ParagraphSpec::Plain
            ),
            Err(Error::InsideRevision { .. })
        );
        assert_eq!(doc.to_bytes().unwrap(), input);
        let input = docx(&format!("{start}{A}{B}<w:sectPr/>{end}"), None, None);
        let mut doc = Document::from_bytes(input.clone()).unwrap();
        assert_matches!(
            doc.track_changes(author(), None)
                .unwrap()
                .append_paragraph("Refused"),
            Err(Error::InsideRevision { .. })
        );
        assert_eq!(doc.to_bytes().unwrap(), input);
    }
}

#[test]
fn tracked_delete_refuses_to_orphan_a_vertical_merge() {
    let body = table()
        .replacen("<w:tc>", "<w:tc><w:tcPr><w:vMerge w:val='restart'/></w:tcPr>", 1)
        .replace(&format!("<w:tc>{B}"), &format!("<w:tc><w:tcPr><w:vMerge/></w:tcPr>{B}"));
    let input = docx(&body, None, None);
    let mut doc = Document::from_bytes(input.clone()).unwrap();
    let table = TableRef::new(StoryIndex::MAIN, 0);
    assert_matches!(
        doc.track_changes(author(), None).unwrap().delete_rows(table, 0..1),
        Err(Error::TrackedPropertyChange { element: "w:vMerge" })
    );
    assert_eq!(doc.to_bytes().unwrap(), input);
    doc.delete_rows(table, 0..1).unwrap();
    let output = xml(&doc.to_bytes().unwrap());
    let tree = roxmltree::Document::parse(&output).unwrap();
    let merge = tree.descendants().find(|node| w(*node, "vMerge")).unwrap();
    assert_eq!(merge.attribute((W_NS, "val")), Some("restart"));
}

#[test]
fn tracked_insert_refuses_revision_at_boundary_or_marker_in_template() {
    let start = "<w:customXmlInsRangeStart w:id='11'/>";
    let end = "<w:customXmlInsRangeEnd w:id='11'/>";
    let at_end = table().replace("</w:tbl>", &format!("{start}</w:tbl>{end}"));
    let in_template = table().replacen(
        "</w:p>",
        &format!("{start}{end}</w:p>"),
        1,
    );
    for (body, position) in [(at_end, 2), (in_template, 1)] {
        let input = docx(&body, None, None);
        let mut doc = Document::from_bytes(input.clone()).unwrap();
        assert_matches!(
            doc.track_changes(author(), None).unwrap().insert_rows(
                TableRef::new(StoryIndex::MAIN, 0), position, 1, RowTemplate::Above,
            ),
            Err(Error::InsideRevision { .. })
        );
        assert_eq!(doc.to_bytes().unwrap(), input);
    }
}

#[test]
fn tracked_insertions_refuse_same_slot_alternate_content() {
    let ac = |member: &str| format!(
        r#"<mc:AlternateContent><mc:Choice Requires="w">{member}</mc:Choice></mc:AlternateContent>"#
    );
    let input = docx(&format!("<w:p><w:r><w:t>anchor</w:t></w:r>{}</w:p>{}", ac("<w:r/>"), ac("<w:p/>")), None, None);
    let mut doc = Document::from_bytes(input.clone()).unwrap();
    let before = doc.to_bytes().unwrap();
    assert_matches!(
        doc.track_changes(author(), None).unwrap().insert_text(RunPosition::After(r(0, 0)), "new"),
        Err(Error::Core(ooxml_core::Error::UnresolvedSibling(_)))
    );
    assert_eq!(doc.to_bytes().unwrap(), before);
    assert_matches!(
        doc.track_changes(author(), None).unwrap().delete_text(TextRange::new(r(0, 0), r(0, 0))),
        Err(Error::Core(ooxml_core::Error::UnresolvedSibling(_)))
    );
    assert_eq!(doc.to_bytes().unwrap(), before);
    assert_matches!(
        doc.track_changes(author(), None).unwrap().insert_paragraph(Place::After(p(0)), "new", &ParagraphSpec::Plain),
        Err(Error::Core(ooxml_core::Error::UnresolvedSibling(_)))
    );
    assert_eq!(doc.to_bytes().unwrap(), before);
}
