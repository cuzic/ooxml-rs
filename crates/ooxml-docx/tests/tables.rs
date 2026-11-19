//! ADR-0011 §7: table rows, columns and merges. Unit documents (T2, T4:
//! Err ⇒ byte-identical), T5 read-back with docx-rs, and a fixture sweep
//! that checks the grid stays consistent.

mod common;

use std::assert_matches;

use common::*;
use ooxml_docx::{Document, Error, RowTemplate, StoryIndex, TableRef};

fn t0() -> TableRef {
    TableRef::new(StoryIndex::MAIN, 0)
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

fn cell(text: &str) -> String {
    if text.is_empty() { "<w:tc><w:p/></w:tc>".into() } else { format!("<w:tc><w:p><w:r><w:t>{text}</w:t></w:r></w:p></w:tc>") }
}

fn table(grid: &[u32], rows: &[String]) -> String {
    let g: String = grid.iter().map(|w| format!(r#"<w:gridCol w:w="{w}"/>"#)).collect();
    format!("<w:tbl><w:tblPr/><w:tblGrid>{g}</w:tblGrid>{}</w:tbl>", rows.concat())
}

#[test]
fn insert_rows_copies_the_template() {
    let head = r#"<w:tr w14:paraId="0A0B0C0D" w:rsidR="00AB12CD" xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml"><w:trPr><w:tblHeader/><w:cantSplit/></w:trPr><w:tc><w:tcPr><w:tcW w:w="1000" w:type="dxa"/><w:shd w:val="clear" w:fill="FF0000"/></w:tcPr><w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="2"/></w:numPr><w:jc w:val="center"/></w:pPr><w:r><w:t>H</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:vMerge w:val="restart"/></w:tcPr><w:p/></w:tc></w:tr>"#;
    let body_row = r#"<w:tr><w:tc><w:p><w:r><w:t>b</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:vMerge/></w:tcPr><w:p/></w:tc></w:tr>"#;
    let mut doc = open(&table(&[1000, 1000], &[head.into(), body_row.into()]));
    doc.insert_rows(t0(), 1, 1, RowTemplate::Above).unwrap();
    let b = body_of(&doc);
    // no attributes, no tblHeader, numPr dropped, vMerge continues (the row below continues it)
    assert!(
        b.contains(r#"<w:tr><w:trPr><w:cantSplit/></w:trPr><w:tc><w:tcPr><w:tcW w:w="1000" w:type="dxa"/><w:shd w:val="clear" w:fill="FF0000"/></w:tcPr><w:p><w:pPr><w:jc w:val="center"/></w:pPr></w:p></w:tc><w:tc><w:tcPr><w:vMerge/></w:tcPr><w:p/></w:tc></w:tr>"#),
        "{b}"
    );
    assert_eq!(doc.table_row_count(t0()), Some(3));
    // appended below the last row: the vMerge is not continued (no row below)
    doc.insert_rows(t0(), 3, 2, RowTemplate::Above).unwrap();
    let b = body_of(&doc);
    assert_eq!(b.matches("<w:vMerge/>").count(), 2, "{b}");
    assert_eq!(doc.table_row_count(t0()), Some(5));
    // paragraphs moved: texts read back in order
    let texts: Vec<String> = doc.paragraphs().map(|p| p.text()).collect();
    assert_eq!(texts, ["H", "", "", "", "b", "", "", "", "", ""]);
    // refusals
    assert_matches!(refused(&mut doc, |d| d.insert_rows(t0(), 9, 1, RowTemplate::Above)), Error::InvalidValue { .. });
    assert_matches!(refused(&mut doc, |d| d.insert_rows(TableRef::new(StoryIndex::MAIN, 3), 0, 1, RowTemplate::Above)), Error::NoSuchTable(3));
    let tracked = r#"<w:tr><w:trPr><w:ins w:id="1" w:author="a"/></w:trPr><w:tc><w:p/></w:tc></w:tr>"#;
    let mut doc = open(&table(&[1000], &[tracked.into()]));
    assert_matches!(refused(&mut doc, |d| d.insert_rows(t0(), 1, 1, RowTemplate::Above)), Error::TrackedPropertyChange { .. });
}

#[test]
fn grid_preconditions() {
    // inconsistent: two cells in a one-column grid
    let mut doc = open(&table(&[1000], &[format!("<w:tr>{}{}</w:tr>", cell("a"), cell("b"))]));
    assert_matches!(refused(&mut doc, |d| d.insert_rows(t0(), 0, 1, RowTemplate::Below)), Error::InconsistentGrid);
    // no tblGrid
    let mut doc = open(&format!("<w:tbl><w:tr>{}</w:tr></w:tbl>", cell("a")));
    assert_matches!(refused(&mut doc, |d| d.insert_column(t0(), 0, 500)), Error::InconsistentGrid);
    // a row-level content control
    let mut doc = open(&table(&[1000], &[format!("<w:sdt><w:sdtContent><w:tr>{}</w:tr></w:sdtContent></w:sdt>", cell("a"))]));
    assert_matches!(refused(&mut doc, |d| d.insert_column(t0(), 0, 500)), Error::UnsupportedContext { .. });
    // legacy hMerge
    let mut doc = open(&table(&[1000, 1000], &[r#"<w:tr><w:tc><w:tcPr><w:hMerge w:val="restart"/></w:tcPr><w:p/></w:tc><w:tc><w:tcPr><w:hMerge/></w:tcPr><w:p/></w:tc></w:tr>"#.into()]));
    assert_matches!(refused(&mut doc, |d| d.merge_cells(t0(), 0..1, 0..2)), Error::UnsupportedContext { ancestor: "w:hMerge" });
    // a table inside a content control
    let mut doc = open(&format!("<w:sdt><w:sdtContent>{}</w:sdtContent></w:sdt>", table(&[1000], &[format!("<w:tr>{}</w:tr>", cell("a"))])));
    assert_matches!(refused(&mut doc, |d| d.insert_rows(t0(), 0, 1, RowTemplate::Below)), Error::UnsupportedContext { .. });
}

#[test]
fn delete_rows_and_vmerge_repair() {
    let rows = [
        r#"<w:tr><w:tc><w:tcPr><w:vMerge w:val="restart"/></w:tcPr><w:p><w:r><w:t>m</w:t></w:r></w:p></w:tc><w:tc><w:p/></w:tc></w:tr>"#.to_owned(),
        r#"<w:tr><w:tc><w:tcPr><w:vMerge/></w:tcPr><w:p/></w:tc><w:tc><w:p><w:bookmarkStart w:id="1" w:name="b"/><w:bookmarkEnd w:id="1"/></w:p></w:tc></w:tr>"#.to_owned(),
        r#"<w:tr><w:tc><w:tcPr><w:vMerge/></w:tcPr><w:p/></w:tc><w:tc><w:p><w:r><w:t>keep</w:t></w:r></w:p></w:tc></w:tr>"#.to_owned(),
    ];
    let mut doc = open(&table(&[1000, 1000], &rows));
    doc.delete_rows(t0(), 0..2).unwrap();
    assert_eq!(
        body_of(&doc),
        r#"<w:tbl><w:tblPr/><w:tblGrid><w:gridCol w:w="1000"/><w:gridCol w:w="1000"/></w:tblGrid><w:tr><w:tc><w:tcPr><w:vMerge w:val="restart"/></w:tcPr><w:p/></w:tc><w:tc><w:p><w:r><w:t>keep</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#
    );
    assert_matches!(refused(&mut doc, |d| d.delete_rows(t0(), 0..1)), Error::TableWouldBeEmpty);
    // refusals
    let with = |inner: &str| {
        let r = format!("<w:tr><w:tc><w:p>{inner}</w:p></w:tc></w:tr>");
        open(&table(&[1000], &[r, format!("<w:tr>{}</w:tr>", cell("x"))]))
    };
    let mut doc = with(r#"<w:r><w:footnoteReference w:id="1"/></w:r>"#);
    assert_matches!(refused(&mut doc, |d| d.delete_rows(t0(), 0..1)), Error::WouldRemoveNote);
    let mut doc = with(r#"<w:ins w:id="1" w:author="a"><w:r><w:t>x</w:t></w:r></w:ins>"#);
    assert_matches!(refused(&mut doc, |d| d.delete_rows(t0(), 0..1)), Error::RevisionInRange { .. });
    let mut doc = with(r#"<w:bookmarkStart w:id="4" w:name="open"/>"#);
    assert_matches!(refused(&mut doc, |d| d.delete_rows(t0(), 0..1)), Error::AnchorWouldBreak { .. });
    let mut doc = with(r#"<w:commentRangeStart w:id="2"/><w:commentRangeEnd w:id="2"/>"#);
    assert_matches!(refused(&mut doc, |d| d.delete_rows(t0(), 0..1)), Error::AnchorWouldBreak { .. });
    let mut doc = with(r#"<w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText>PAGE</w:instrText></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r>"#);
    assert_matches!(refused(&mut doc, |d| d.delete_rows(t0(), 0..1)), Error::InsideField);
}

#[test]
fn columns() {
    // row 0: two cells; row 1: one cell spanning both; row 2: gridBefore 1 + one cell
    let rows = [
        format!("<w:tr>{}{}</w:tr>", r#"<w:tc><w:tcPr><w:tcW w:w="1000" w:type="dxa"/><w:shd w:fill="00FF00"/></w:tcPr><w:p/></w:tc>"#, cell("b")),
        r#"<w:tr><w:tc><w:tcPr><w:gridSpan w:val="2"/></w:tcPr><w:p><w:r><w:t>wide</w:t></w:r></w:p></w:tc></w:tr>"#.to_owned(),
        format!(r#"<w:tr><w:trPr><w:gridBefore w:val="1"/></w:trPr>{}</w:tr>"#, cell("c")),
    ];
    let mut doc = open(&table(&[1000, 1000], &rows));
    doc.insert_column(t0(), 1, 700).unwrap();
    assert_eq!(doc.table_grid_columns(t0()), Some(3));
    assert_eq!(
        body_of(&doc),
        r#"<w:tbl><w:tblPr/><w:tblGrid><w:gridCol w:w="1000"/><w:gridCol w:w="700"/><w:gridCol w:w="1000"/></w:tblGrid><w:tr><w:tc><w:tcPr><w:tcW w:w="1000" w:type="dxa"/><w:shd w:fill="00FF00"/></w:tcPr><w:p/></w:tc><w:tc><w:tcPr><w:tcW w:w="700" w:type="dxa"/><w:shd w:fill="00FF00"/></w:tcPr><w:p/></w:tc><w:tc><w:p><w:r><w:t>b</w:t></w:r></w:p></w:tc></w:tr><w:tr><w:tc><w:tcPr><w:gridSpan w:val="3"/></w:tcPr><w:p><w:r><w:t>wide</w:t></w:r></w:p></w:tc></w:tr><w:tr><w:trPr><w:gridBefore w:val="1"/></w:trPr><w:tc><w:tcPr><w:tcW w:w="700" w:type="dxa"/></w:tcPr><w:p/></w:tc><w:tc><w:p><w:r><w:t>c</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#
    );
    // and back: the new (empty) column goes, spans narrow
    doc.delete_column(t0(), 1).unwrap();
    assert_eq!(
        body_of(&doc),
        r#"<w:tbl><w:tblPr/><w:tblGrid><w:gridCol w:w="1000"/><w:gridCol w:w="1000"/></w:tblGrid><w:tr><w:tc><w:tcPr><w:tcW w:w="1000" w:type="dxa"/><w:shd w:fill="00FF00"/></w:tcPr><w:p/></w:tc><w:tc><w:p><w:r><w:t>b</w:t></w:r></w:p></w:tc></w:tr><w:tr><w:tc><w:tcPr><w:gridSpan w:val="2"/></w:tcPr><w:p><w:r><w:t>wide</w:t></w:r></w:p></w:tc></w:tr><w:tr><w:trPr><w:gridBefore w:val="1"/></w:trPr><w:tc><w:p><w:r><w:t>c</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#
    );
    // gridBefore covers column 0: it shrinks and goes
    doc.insert_column(t0(), 2, 500).unwrap();
    assert_eq!(doc.table_grid_columns(t0()), Some(3));
    assert_matches!(refused(&mut doc, |d| d.delete_column(t0(), 1)), Error::CellNotEmpty);
    let mut one = open(&table(&[1000, 1000], &[format!("<w:tr>{}{}</w:tr>", cell(""), cell(""))]));
    one.delete_column(t0(), 0).unwrap();
    assert_matches!(refused(&mut one, |d| d.delete_column(t0(), 0)), Error::TableWouldBeEmpty);
    let mut only = open(&table(&[1000, 1000], &[format!("<w:tr><w:trPr><w:gridAfter w:val=\"1\"/></w:trPr>{}</w:tr>", cell(""))]));
    assert_matches!(refused(&mut only, |d| d.delete_column(t0(), 0)), Error::RowWouldBeEmpty);
}

#[test]
fn merges() {
    let rows: Vec<String> = (0..3).map(|r| format!("<w:tr>{}{}{}</w:tr>", cell(&format!("r{r}")), cell(""), cell(""))).collect();
    let mut doc = open(&table(&[1000, 1500, 2000], &rows));
    // a 1×2 merge in row 0 needs empty cells other than the first
    doc.merge_cells(t0(), 0..1, 1..3).unwrap();
    assert!(body_of(&doc).contains(r#"<w:tc><w:tcPr><w:tcW w:w="3500" w:type="dxa"/><w:gridSpan w:val="2"/></w:tcPr><w:p/></w:tc></w:tr>"#), "{}", body_of(&doc));
    // a vertical merge of column 1..3 over rows 1..3
    doc.merge_cells(t0(), 1..3, 1..3).unwrap();
    let b = body_of(&doc);
    assert!(b.contains(r#"<w:gridSpan w:val="2"/><w:vMerge w:val="restart"/></w:tcPr>"#), "{b}");
    assert!(b.contains(r#"<w:gridSpan w:val="2"/><w:vMerge/></w:tcPr>"#), "{b}");
    // overlap: cuts the span of row 0
    assert_matches!(refused(&mut doc, |d| d.merge_cells(t0(), 0..1, 0..2)), Error::MergeOverlaps);
    // overlap: the rectangle's top cuts the vertical merge
    assert_matches!(refused(&mut doc, |d| d.merge_cells(t0(), 2..3, 1..3)), Error::MergeOverlaps);
    // non-empty
    assert_matches!(refused(&mut doc, |d| d.merge_cells(t0(), 0..2, 0..1)), Error::MergeNonEmpty);
    // T5: docx-rs reads gridSpan and vMerge
    let json: String = docx_rs::read_docx(&doc.to_bytes().unwrap()).unwrap().json().split_whitespace().collect();
    assert_eq!(json.matches(r#""gridSpan":2"#).count(), 3);
    assert!(json.contains(r#""verticalMerge":"restart""#) && json.contains(r#""verticalMerge":"continue""#));
}

/// Is every table of every story grid-consistent (ADR-0011 §7)?
fn consistent(doc: &Document) -> bool {
    doc.stories().all(|s| {
        (0..doc.table_count(s.index())).all(|i| {
            let t = TableRef::new(s.index(), i);
            // insert_rows refuses an inconsistent grid with InconsistentGrid only
            doc.table_grid_columns(t).is_some()
        })
    })
}

/// Fixture sweep: on every table of every writable fixture, insert a row at
/// 1, a grid column at 1, delete that column and that row again. Refusals
/// change nothing; after the round trip the story texts are those of the
/// input, and the output reopens.
#[test]
fn fixture_sweep() {
    let (mut ok, mut refusals) = (0, 0);
    for stem in writable() {
        let input = fixture(stem);
        let mut doc = Document::from_bytes(input.clone()).unwrap();
        let texts: Vec<Vec<String>> = doc.stories().map(|s| s.paragraphs().map(|p| p.text()).collect()).collect();
        let stories: Vec<StoryIndex> = doc.stories().map(|s| s.index()).collect();
        for s in stories {
            for i in 0..doc.table_count(s) {
                let t = TableRef::new(s, i);
                let snap = doc.to_bytes().unwrap();
                match doc.insert_rows(t, 1.min(doc.table_row_count(t).unwrap()), 1, RowTemplate::Above) {
                    Ok(()) => {
                        ok += 1;
                        doc.delete_rows(t, 1..2).unwrap_or_else(|e| panic!("{stem} t{i}: delete the inserted row: {e}"));
                    }
                    Err(_) => {
                        refusals += 1;
                        assert_eq!(doc.to_bytes().unwrap(), snap, "{stem}");
                    }
                }
                let snap = doc.to_bytes().unwrap();
                match doc.insert_column(t, 1.min(doc.table_grid_columns(t).unwrap_or(0)), 500) {
                    Ok(()) => {
                        ok += 1;
                        doc.delete_column(t, 1.min(doc.table_grid_columns(t).unwrap() - 1)).unwrap_or_else(|e| panic!("{stem} t{i}: delete the inserted column: {e}"));
                    }
                    Err(_) => {
                        refusals += 1;
                        assert_eq!(doc.to_bytes().unwrap(), snap, "{stem}");
                    }
                }
            }
        }
        assert!(consistent(&doc));
        let re = Document::from_bytes(doc.to_bytes().unwrap()).unwrap();
        let got: Vec<Vec<String>> = re.stories().map(|s| s.paragraphs().map(|p| p.text()).collect()).collect();
        assert_eq!(got, texts, "{stem}");
    }
    eprintln!("tables sweep: {ok} inserts (each undone), {refusals} refusals");
    assert!(ok > 20, "{ok}");
}

#[test]
fn delete_rows_preserves_vertical_merge_boundaries() {
    let restart = r#"<w:tr><w:tc><w:tcPr><w:vMerge w:val="restart"/></w:tcPr><w:p/></w:tc></w:tr>"#;
    let continuation = "<w:tr><w:tc><w:tcPr><w:vMerge/></w:tcPr><w:p/></w:tc></w:tr>";
    let plain = "<w:tr><w:tc><w:p/></w:tc></w:tr>";
    for (rows, range, remaining) in [
        (vec![restart, continuation, restart, continuation], 1..3, vec![restart, restart]),
        (vec![restart, continuation, continuation, continuation], 1..3, vec![restart, continuation]),
        (vec![plain, restart, continuation, continuation], 1..3, vec![plain, restart]),
        (vec![restart, continuation, restart, continuation], 1..2, vec![restart, restart, continuation]),
        (vec![restart, continuation, restart, continuation], 2..4, vec![restart, continuation]),
    ] {
        let input = table(&[1000], &rows.into_iter().map(str::to_owned).collect::<Vec<_>>());
        let mut doc = open(&input);
        doc.delete_rows(t0(), range.clone()).unwrap();
        let expected = table(&[1000], &remaining.into_iter().map(str::to_owned).collect::<Vec<_>>());
        assert_eq!(body_of(&doc), expected, "deleted {range:?} from {input}");
    }
}

#[test]
fn cell_removal_refuses_revisions() {
    for (name, contents) in [
        ("cellIns", r#"<w:tcPr><w:cellIns w:id="1" w:author="a"/></w:tcPr><w:p/>"#),
        ("tcPrChange", r#"<w:tcPr><w:tcPrChange w:id="1" w:author="a"><w:tcPr/></w:tcPrChange></w:tcPr><w:p/>"#),
        ("ins", r#"<w:p><w:pPr><w:rPr><w:ins w:id="1" w:author="a"/></w:rPr></w:pPr></w:p>"#),
        ("pPrChange", r#"<w:p><w:pPr><w:pPrChange w:id="1" w:author="a"><w:pPr/></w:pPrChange></w:pPr></w:p>"#),
    ] {
        let row = format!("<w:tr>{}<w:tc>{contents}</w:tc></w:tr>", cell(""));
        let mut doc = open(&table(&[1000, 1000], &[row]));
        for err in [
            refused(&mut doc, |d| d.merge_cells(t0(), 0..1, 0..2)),
            refused(&mut doc, |d| d.delete_column(t0(), 1)),
        ] {
            assert_matches!(err, Error::RevisionInRange { element } if element == format!("w:{name}"));
        }
    }
}

#[test]
fn cell_removal_shares_note_field_and_comment_checks() {
    let cases = [
        (r#"<w:r><w:footnoteReference w:id="1"/></w:r>"#, Error::WouldRemoveNote),
        (r#"<w:fldSimple w:instr="PAGE"/>"#, Error::InsideField),
        (r#"<w:commentRangeStart w:id="1"/>"#, Error::AnchorWouldBreak { marker: "w:commentRangeStart".into() }),
    ];
    for (contents, expected) in cases {
        let row = format!("<w:tr>{}<w:tc><w:p>{contents}</w:p></w:tc></w:tr>", cell(""));
        let mut doc = open(&table(&[1000, 1000], &[row]));
        for err in [
            refused(&mut doc, |d| d.merge_cells(t0(), 0..1, 0..2)),
            refused(&mut doc, |d| d.delete_column(t0(), 1)),
        ] {
            assert_eq!(std::mem::discriminant(&err), std::mem::discriminant(&expected), "{err}");
        }
    }
}

#[test]
fn horizontal_merge_refuses_vertical_merge_crossing_bottom() {
    let rows = [
        format!(r#"<w:tr><w:tc><w:tcPr><w:vMerge w:val="restart"/></w:tcPr><w:p/></w:tc>{}</w:tr>"#, cell("")),
        format!("<w:tr><w:tc><w:tcPr><w:vMerge/></w:tcPr><w:p/></w:tc>{}</w:tr>", cell("")),
    ];
    let mut doc = open(&table(&[1000, 1000], &rows));
    assert_matches!(refused(&mut doc, |d| d.merge_cells(t0(), 0..1, 0..2)), Error::MergeOverlaps);
}

#[test]
fn span_changes_adjust_only_known_dxa_widths() {
    for (unit, grid_known, widened, narrowed) in [
        ("dxa", true, 2700, 1500),
        ("pct", true, 2000, 2000),
        ("dxa", false, 2000, 2000),
    ] {
        let row = format!(r#"<w:tr><w:tc><w:tcPr><w:tcW w:w="2000" w:type="{unit}"/><w:gridSpan w:val="2"/></w:tcPr><w:p/></w:tc></w:tr>"#);
        let mut input = table(&[1200, 800], &[row]);
        if !grid_known {
            input = input.replace(r#"<w:gridCol w:w="800"/>"#, "<w:gridCol/>");
        }
        let mut doc = open(&input);
        doc.insert_column(t0(), 1, 700).unwrap();
        assert!(body_of(&doc).contains(&format!(r#"<w:tcW w:w="{widened}" w:type="{unit}"/><w:gridSpan w:val="3"/>"#)));
        doc.delete_column(t0(), 0).unwrap();
        assert!(body_of(&doc).contains(&format!(r#"<w:tcW w:w="{narrowed}" w:type="{unit}"/><w:gridSpan w:val="2"/>"#)));
    }
}

#[test]
fn merge_keeps_preferred_width_when_grid_width_is_unknown() {
    let row = format!(r#"<w:tr><w:tc><w:tcPr><w:tcW w:w="1234" w:type="dxa"/></w:tcPr><w:p/></w:tc>{}</w:tr>"#, cell(""));
    let input = table(&[1200, 800], &[row]).replace(r#"<w:gridCol w:w="800"/>"#, "<w:gridCol/>");
    let mut doc = open(&input);
    doc.merge_cells(t0(), 0..1, 0..2).unwrap();
    assert!(body_of(&doc).contains(r#"<w:tcW w:w="1234" w:type="dxa"/><w:gridSpan w:val="2"/>"#));
}

#[test]
fn column_edits_and_merges_refuse_row_revisions() {
    for name in ["ins", "del", "trPrChange"] {
        let revision = if name == "trPrChange" {
            r#"<w:trPrChange w:id="1" w:author="a"><w:trPr/></w:trPrChange>"#.to_owned()
        } else {
            format!(r#"<w:{name} w:id="1" w:author="a"/>"#)
        };
        let cells = cell("").repeat(2);
        let plain = format!("<w:tr>{cells}</w:tr>");
        let tracked = format!("<w:tr><w:trPr>{revision}</w:trPr>{cells}</w:tr>");
        let mut doc = open(&table(&[1000, 1000], &[plain, tracked]));
        for err in [
            refused(&mut doc, |d| d.insert_column(t0(), 1, 500)),
            refused(&mut doc, |d| d.delete_column(t0(), 1)),
            refused(&mut doc, |d| d.merge_cells(t0(), 0..1, 0..2)),
        ] {
            assert_matches!(err, Error::RevisionInRange { element } if element == format!("w:{name}"));
        }
    }
}

#[test]
fn structural_edits_refuse_revisions_in_retained_cells() {
    for (name, revision) in [
        ("tcPrChange", r#"<w:tcPrChange w:id="1" w:author="a"><w:tcPr/></w:tcPrChange>"#),
        ("cellIns", r#"<w:cellIns w:id="1" w:author="a"/>"#),
        ("cellDel", r#"<w:cellDel w:id="1" w:author="a"/>"#),
        ("cellMerge", r#"<w:cellMerge w:id="1" w:author="a" w:vMerge="cont"/>"#),
    ] {
        let merged = format!(r#"<w:tr><w:tc><w:tcPr><w:gridSpan w:val="2"/>{revision}</w:tcPr><w:p/></w:tc></w:tr>"#);
        let mut doc = open(&table(&[1000, 1000], &[merged]));
        let retained = format!("<w:tr><w:tc><w:tcPr>{revision}</w:tcPr><w:p/></w:tc>{}</w:tr>", cell(""));
        let plain = format!("<w:tr>{}</w:tr>", cell("").repeat(2));
        let mut vertical = open(&table(&[1000, 1000], &[plain, retained]));
        for err in [
            refused(&mut doc, |d| d.insert_column(t0(), 1, 500)),
            refused(&mut doc, |d| d.delete_column(t0(), 1)),
            refused(&mut vertical, |d| d.merge_cells(t0(), 0..2, 0..1)),
        ] {
            assert_matches!(err, Error::RevisionInRange { element } if element == format!("w:{name}"));
        }
    }
}

#[test]
fn delete_rows_refuses_revisions_in_repaired_cell() {
    let restart = r#"<w:tr><w:tc><w:tcPr><w:vMerge w:val="restart"/></w:tcPr><w:p/></w:tc></w:tr>"#;
    let continuation = r#"<w:tr><w:tc><w:tcPr><w:vMerge/><w:cellIns w:id="1" w:author="a"/></w:tcPr><w:p/></w:tc></w:tr>"#;
    let mut doc = open(&table(&[1000], &[restart.into(), continuation.into()]));
    assert_matches!(refused(&mut doc, |d| d.delete_rows(t0(), 0..1)), Error::RevisionInRange { element } if element == "w:cellIns");
}

#[test]
fn retained_cells_keep_tracked_paragraph_content_byte_identical() {
    let paragraph = r#"<w:p><w:ins w:author='a' w:id='1'><w:r><w:t>tracked</w:t></w:r></w:ins></w:p>"#;
    let untouched = "<w:tr><!-- untouched --><w:tc><w:p/></w:tc><w:tc><w:p/></w:tc></w:tr>";
    let row = format!("<w:tr><w:tc>{paragraph}</w:tc>{}</w:tr>", cell(""));
    let mut doc = open(&table(&[1000, 1000], &[row, untouched.into()]));
    doc.merge_cells(t0(), 0..1, 0..2).unwrap();

    let merged = format!(r#"<w:tr><w:tc><w:tcPr><w:tcW w:w="2000" w:type="dxa"/><w:gridSpan w:val="2"/></w:tcPr>{paragraph}</w:tc></w:tr>"#);
    assert_eq!(body_of(&doc), table(&[1000, 1000], &[merged.clone(), untouched.into()]));

    let mut doc = open(&table(&[1000, 1000], std::slice::from_ref(&merged)));
    doc.insert_column(t0(), 1, 500).unwrap();
    let widened = merged.replace(r#"w:w="2000""#, r#"w:w="2500""#).replace(r#"w:val="2""#, r#"w:val="3""#);
    assert_eq!(body_of(&doc), table(&[1000, 500, 1000], &[widened]));
    doc.delete_column(t0(), 1).unwrap();
    assert_eq!(body_of(&doc), table(&[1000, 1000], &[merged]));

    let restart = r#"<w:tr><w:tc><w:tcPr><w:vMerge w:val="restart"/></w:tcPr><w:p/></w:tc></w:tr>"#;
    let continuation = format!("<w:tr><w:tc><w:tcPr><w:vMerge/></w:tcPr>{paragraph}</w:tc></w:tr>");
    let mut doc = open(&table(&[1000], &[restart.into(), continuation.clone()]));
    doc.delete_rows(t0(), 0..1).unwrap();
    let repaired = continuation.replace("<w:vMerge/>", r#"<w:vMerge w:val="restart"/>"#);
    assert_eq!(body_of(&doc), table(&[1000], &[repaired]));
}

#[test]
fn merge_refuses_tracked_paragraph_marks_in_continuation_cells() {
    let plain = format!("<w:tr>{}</w:tr>", cell(""));
    let tracked = r#"<w:tr><w:tc><w:p><w:pPr><w:rPr><w:ins w:id="1" w:author="a"/></w:rPr></w:pPr></w:p></w:tc></w:tr>"#;
    let mut doc = open(&table(&[1000], &[plain, tracked.into()]));
    assert_matches!(refused(&mut doc, |d| d.merge_cells(t0(), 0..2, 0..1)), Error::RevisionInRange { element } if element == "w:ins");
}

#[test]
fn delete_rows_refuses_row_revisions_at_repair_destination() {
    let restart = r#"<w:tr><w:tc><w:tcPr><w:vMerge w:val="restart"/></w:tcPr><w:p/></w:tc></w:tr>"#;
    for revision in ["ins", "del", "trPrChange"] {
        let continuation = format!(r#"<w:tr><w:trPr><w:{revision} w:id="1" w:author="a"/></w:trPr><w:tc><w:tcPr><w:vMerge/></w:tcPr><w:p/></w:tc></w:tr>"#);
        let mut doc = open(&table(&[1000], &[restart.into(), continuation]));
        assert_matches!(refused(&mut doc, |d| d.delete_rows(t0(), 0..1)), Error::RevisionInRange { element } if element == format!("w:{revision}"));
    }
}

#[test]
fn delete_rows_treats_a_different_span_as_a_vertical_merge_boundary() {
    let continuation = format!("<w:tr><w:tc><w:tcPr><w:vMerge/></w:tcPr><w:p/></w:tc>{}</w:tr>", cell(""));
    let restart = r#"<w:tr><w:tc><w:tcPr><w:gridSpan w:val="2"/><w:vMerge w:val="restart"/></w:tcPr><w:p/></w:tc></w:tr>"#;
    let plain = format!("<w:tr>{}</w:tr>", cell("").repeat(2));
    // A differently sized restart cannot be the origin of this continuation.
    let mut doc = open(&table(&[1000, 1000], &[plain.clone(), restart.into(), continuation.clone()]));
    doc.delete_rows(t0(), 1..2).unwrap();
    assert_eq!(body_of(&doc), table(&[1000, 1000], &[plain, continuation]));
}
