//! Phase0 edit tests: docs/roadmap/xlsx.md §7.4 (T2 + CR), with the
//! 2026-11-16 owner decisions (first *visible* worksheet; any cell of a
//! merged range, top-left included, is refused).

mod common;

use std::assert_matches;

use common::*;
use ooxml_core::PartStateKind;
use ooxml_xlsx::{CellValue, Error, Workbook};

fn merged_expect(input: &[u8]) -> impl Fn(ooxml_xlsx::CellIndex) -> Expect {
    let m = merged(input);
    // ADR-0014 §3: the anchor (top-left) of a merged range is writable.
    move |at| if m.iter().any(|r| r.contains(at) && r.first != at) { Expect::Merged } else { Expect::Ok }
}

/// P0-E1: ADR-0001's sweep on the hand-made fixture (A1, A2 shared strings,
/// B2 number, C2 boolean → number drops `t="b"`, D2 self-closing).
#[test]
fn e1_fixture_minimal_sweep() {
    let input = read("minimal");
    let cells = existing_cells(&input);
    assert_eq!(cells.iter().map(ToString::to_string).collect::<Vec<_>>(), ["A1", "A2", "B2", "C2", "D2"]);
    let (ok, refused) = sweep("minimal", &cells, |_| Expect::Ok);
    assert_eq!((ok, refused), (14, 0));
    let out = check_edit("minimal", &input, a1("C2"), &W::Num(1.0));
    assert!(sheet_xml(&out).contains(r#"<c r="C2"><v>1</v></c>"#), "stale t=\"b\" dropped");
    let out = check_edit("minimal", &input, a1("D2"), &W::Num(7.0));
    assert!(sheet_xml(&out).contains(r#"<c r="D2" s="1"><v>7</v></c>"#), "self-closing cell re-rendered");
    assert!(sheet_xml(&out).contains("opaque-vendor-extension-should-survive-round-trip"));
    // a new cell beyond the dimension grows it
    let out = check_edit("minimal", &input, a1("F5"), &W::Str("x".into()));
    assert!(sheet_xml(&out).contains(r#"<dimension ref="A1:F5"/>"#));
}

/// P0-E2: Excel 2013 file, 45 self-closing styled cells, 6 merges, x14ac.
#[test]
fn e2_61605_sweep_and_new_row() {
    let name = "poi/61605.xlsx";
    let input = read(name);
    let cells = existing_cells(&input);
    assert_eq!(cells.len(), 46);
    let (ok, refused) = sweep(name, &cells, merged_expect(&input));
    // 6 merges of 4 cells, 2 writes each; the anchors are writable since
    // ADR-0014 §3, so 3 refused cells per merge
    assert!(ok > 0 && refused == 6 * 3 * 2, "ok {ok}, refused {refused}");
    let out = check_edit(name, &input, a1("B2"), &W::Num(2.0));
    assert!(sheet_xml(&out).contains(r#"<c r="B2" s="1"><v>2</v></c>"#));
    let out = check_edit(name, &input, a1("A20"), &W::Str("new".into()));
    let xml = sheet_xml(&out);
    assert!(xml.contains(r#"<dimension ref="A1:E20"/>"#), "{xml}");
    let (r15, r20) = (xml.find(r#"<row r="15""#).unwrap(), xml.find(r#"<row r="20">"#).unwrap());
    assert!(r15 < r20 && xml[r20..].starts_with(r#"<row r="20"><c r="A20" t="inlineStr"><is><t>new</t></is></c></row></sheetData>"#));
}

/// P0-E3: self-closing `<row r="2" …/>` gains its first cell; CF
/// `<formula>` elements do not trigger the formula refusal.
#[test]
fn e3_fill_without_color() {
    let name = "poi/FillWithoutColor.xlsx";
    let input = read(name);
    let wb = Workbook::from_bytes(input.clone()).unwrap();
    assert!(wb.edit_refusal().is_none(), "CF <formula> is not a cell formula");
    let out = check_edit(name, &input, a1("B2"), &W::Num(5.0));
    let xml = sheet_xml(&out);
    assert!(
        xml.contains(r#"<row r="2" spans="2:9" ht="15.75" thickBot="1" x14ac:dyDescent="0.3"><c r="B2"><v>5</v></c></row>"#),
        "row 2 re-rendered with its attributes"
    );
    let cells: Vec<_> = existing_cells(&input).into_iter().step_by(26).take(20).collect();
    assert_eq!(cells.len(), 20);
    let (ok, refused) = sweep(name, &cells, merged_expect(&input));
    assert!(ok > 0, "ok {ok}, refused {refused}");
}

/// P0-E4: first sheet `<sheetData/>` (and part order ≠ sheet order).
#[test]
fn e4_reordered_sheets_empty_sheet_data() {
    let name = "poi/reordered_sheets.xlsx";
    let input = read(name);
    check_edit(name, &input, a1("A1"), &W::Num(1.0));
    check_edit(name, &input, a1("C3"), &W::Str("x".into()));
    let mut wb = Workbook::from_bytes(input.clone()).unwrap();
    {
        let mut ws = wb.worksheet().unwrap();
        ws.write_number(0, 0, 1.0).unwrap();
        ws.write_string(2, 2, "x").unwrap();
    }
    let out = wb.to_bytes().unwrap();
    assert_l1_except(&input, &out, &["xl/worksheets/sheet1.xml"]);
    let xml = sheet_xml(&out);
    assert!(xml.contains(r#"<dimension ref="A1:C3"/>"#));
    assert!(xml.contains(r#"<sheetData><row r="1"><c r="A1"><v>1</v></c></row><row r="3"><c r="C3" t="inlineStr"><is><t>x</t></is></c></row></sheetData>"#), "{xml}");
    let cr = calamine_cells(&out);
    assert_eq!(cr["Sheet4"].len(), 2);
}

/// P0-E5: LibreOffice output with data descriptors.
#[test]
fn e5_62272_data_descriptors() {
    let name = "poi/62272.xlsx";
    let input = read(name);
    sweep(name, &[a1("A1")], |_| Expect::Ok);
    check_edit(name, &input, a1("B2"), &W::Num(2.0));
}

/// P0-E6: no `dimension` element: none is inserted.
#[test]
fn e6_renamed_workbook_part_no_dimension() {
    let name = "calamine/renamed_workbook_part.xlsx";
    let input = read(name);
    let wb = Workbook::from_bytes(input.clone()).unwrap();
    assert_eq!((wb.workbook_part(), wb.worksheet_part()), ("xl/workbook2.xml", "xl/sheet1.xml"));
    let out = check_edit(name, &input, a1("C1"), &W::Num(3.5));
    assert!(!sheet_xml(&out).contains("dimension"));
}

/// P0-E7: threaded comments, persons, legacy note, VML stay L1.
#[test]
fn e7_64759_threaded_comments() {
    let name = "poi/64759.xlsx";
    let input = read(name);
    sweep(name, &existing_cells(&input), |_| Expect::Ok);
}

/// P0-E8: chart over the edited cells stays L1 (its cache goes stale; §4).
#[test]
fn e8_57362_chart() {
    let name = "poi/57362.xlsx";
    check_edit(name, &read(name), a1("B2"), &W::Num(5.0));
}

/// P0-E9: x14 data validation with `xm:f` on sheet 2 is not a cell formula.
#[test]
fn e9_51850_xm_f() {
    let name = "poi/51850.xlsx";
    let input = read(name);
    assert!(Workbook::from_bytes(input.clone()).unwrap().edit_refusal().is_none());
    sweep(name, &existing_cells(&input), |_| Expect::Ok);
}

/// P0-E10: external links and customXml stay L1.
#[test]
fn e10_54399_external_links() {
    let name = "poi/54399.xlsx";
    check_edit(name, &read(name), a1("B6"), &W::Str("edited".into()));
}

/// P0-E11: table `E1:G4` on the first sheet: header row refused, body and
/// outside Ok; pivot parts L1.
#[test]
fn e11_pivot_and_table() {
    let name = "poi/ExcelPivotTableSample.xlsx";
    let input = read(name);
    check_edit(name, &input, a1("A1"), &W::Str("outside".into()));
    check_edit(name, &input, a1("E2"), &W::Num(9.0));
    let header = |at: ooxml_xlsx::CellIndex| {
        if at.row == 0 && (4..=6).contains(&at.col) { Expect::TableHeader } else { Expect::Ok }
    };
    let (_, refused) = sweep(name, &[a1("E1"), a1("F1"), a1("G1"), a1("G4")], header);
    assert_eq!(refused, 3 * 3);
}

/// Review round 1, F3: table-header variants. `ExcelPivotTableSample` with
/// (a) a second table `T2` (`J5:K8`, `headerRowCount="2"`) added to the
/// sheet (table part, sheet relationship, `tablePart`, content-type
/// override): both of its header rows are refused in every column, the
/// first table is still guarded, body cells are Ok; (b) the original table
/// with `headerRowCount="0"`: its top row is writable.
#[test]
fn e11b_table_header_variants() {
    let name = "poi/ExcelPivotTableSample.xlsx";
    let input = read(name);
    let s1 = text(&input, "xl/worksheets/sheet1.xml");
    let tp = s1.find("<tableParts").unwrap()..s1.find("</tableParts>").unwrap() + "</tableParts>".len();
    let rid = {
        let seg = &s1[tp.clone()];
        let i = seg.find("r:id=\"").unwrap() + 6;
        seg[i..i + seg[i..].find('"').unwrap()].to_owned()
    };
    let s1 = format!(
        r#"{}<tableParts count="2"><tablePart r:id="{rid}"/><tablePart r:id="rId99"/></tableParts>{}"#,
        &s1[..tp.start],
        &s1[tp.end..]
    );
    let rels = patched(
        &input,
        "xl/worksheets/_rels/sheet1.xml.rels",
        "</Relationships>",
        r#"<Relationship Id="rId99" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/table" Target="../tables/table2.xml"/></Relationships>"#,
    );
    let ct = patched(
        &input,
        "[Content_Types].xml",
        "</Types>",
        r#"<Override PartName="/xl/tables/table2.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.table+xml"/></Types>"#,
    );
    let t2 = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><table xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" id="2" name="T2" displayName="T2" ref="J5:K8" headerRowCount="2"><tableColumns count="2"><tableColumn id="1" name="a"/><tableColumn id="2" name="b"/></tableColumns></table>"#;
    let bytes = rebuild(
        &input,
        &[
            ("xl/worksheets/sheet1.xml", s1.into_bytes()),
            ("xl/worksheets/_rels/sheet1.xml.rels", rels),
            ("xl/tables/table2.xml", t2.to_vec()),
            ("[Content_Types].xml", ct),
        ],
    );
    for (cell, table) in [("E1", Some("Tabelle1")), ("J5", Some("T2")), ("K5", Some("T2")), ("J6", Some("T2")), ("K6", Some("T2")), ("J7", None), ("K8", None), ("E2", None), ("J4", None)] {
        let (wb, r) = apply(&bytes, a1(cell), &W::Num(1.0));
        match table {
            Some(t) => {
                assert_matches!(r, Err(Error::TableHeaderCell { ref table, .. }) if table == t, "{cell}");
                assert_unchanged(&wb, &bytes);
            }
            None => r.unwrap_or_else(|e| panic!("{cell}: {e}")),
        }
    }
    // headerRowCount="0": no header row at all
    let t1 = patched(&input, "xl/tables/table1.xml", r#"totalsRowShown="0""#, r#"totalsRowShown="0" headerRowCount="0""#);
    let bytes = rebuild(&input, &[("xl/tables/table1.xml", t1)]);
    for cell in ["E1", "F1", "G1"] {
        apply(&bytes, a1(cell), &W::Num(1.0)).1.unwrap_or_else(|e| panic!("{cell}: {e}"));
    }
}

/// Review round 2, N2: insertion order. `fixture_minimal` with A2 removed
/// (row 2 starts at B2), a row 4 added after row 2 and `dimension` A1:D4.
/// A2 goes in front of B2, C3 becomes a new row between rows 2 and 4, and
/// E1 is appended to row 1; each write passes T2 (byte locality at the
/// expected insertion point, rows and cells ascending) and CR.
#[test]
fn e16_insert_in_the_middle() {
    let input = read("minimal");
    let s = text(&input, "xl/worksheets/sheet1.xml")
        .replacen("<c r=\"A2\" t=\"s\"><v>1</v></c>\n", "", 1)
        .replacen("</row>\n</sheetData>", "</row>\n<row r=\"4\"><c r=\"A4\"><v>4</v></c></row>\n</sheetData>", 1)
        .replacen(r#"<dimension ref="A1:D2"/>"#, r#"<dimension ref="A1:D4"/>"#, 1);
    assert!(!s.contains(r#"r="A2""#) && s.contains(r#"<row r="4">"#) && s.contains("A1:D4"));
    let bytes = rebuild(&input, &[("xl/worksheets/sheet1.xml", s.into_bytes())]);
    let out = check_edit("minimal-gaps", &bytes, a1("A2"), &W::Str("first".into()));
    assert!(sheet_xml(&out).contains("<row r=\"2\">\n<c r=\"A2\" t=\"inlineStr\"><is><t>first</t></is></c><c r=\"B2\">"));
    let out = check_edit("minimal-gaps", &bytes, a1("C3"), &W::Num(3.0));
    assert!(sheet_xml(&out).contains("</row>\n<row r=\"3\"><c r=\"C3\"><v>3</v></c></row><row r=\"4\">"));
    check_edit("minimal-gaps", &bytes, a1("E1"), &W::Num(5.0));
}

/// P0-E12: images in cells (`vm`) are refused; a neighbour is Ok.
#[test]
fn e12_rich_value_cells() {
    let name = "calamine/pictures_in_cell_and_over_cell.xlsx";
    let input = read(name);
    let (_, refused) =
        sweep(name, &[a1("A1"), a1("A3")], |_| Expect::RichValue);
    assert_eq!(refused, 4);
    check_edit(name, &input, a1("A2"), &W::Num(1.0));
}

/// P0-E13: `.xlsm` with VBA; drawing `<from>` is not a formula.
#[test]
fn e13_62629_xlsm() {
    let name = "poi/62629_target.xlsm";
    let input = read(name);
    let out = check_edit(name, &input, a1("A1"), &W::Str("macro book".into()));
    assert_eq!(part(&input, "[Content_Types].xml"), part(&out, "[Content_Types].xml"));
}

/// P0-E14: inline rich strings, `xml:space="preserve"`, valueless
/// `t="inlineStr"` cells, no sharedStrings part.
#[test]
fn e14_52348_inline_strings() {
    let name = "poi/52348.xlsx";
    let input = read(name);
    let m = merged(&input);
    let cells: Vec<_> = existing_cells(&input).into_iter().filter(|c| !m.iter().any(|r| r.contains(*c))).collect();
    let inline: Vec<_> = {
        let mut wb = Workbook::from_bytes(input.clone()).unwrap();
        let ws = wb.worksheet().unwrap();
        cells
            .iter()
            .copied()
            .filter(|c| matches!(ws.cell(c.row, c.col).unwrap().unwrap().value().unwrap(), CellValue::String(_)))
            .collect()
    };
    assert!(inline.len() >= 10, "{}", inline.len());
    sweep(name, &inline, |_| Expect::Ok);
    let at = inline[0];
    let out = check_edit(name, &input, at, &W::Str("  padded ".into()));
    assert!(sheet_xml(&out).contains(&format!(r#"<c r="{at}" s="#)));
    assert!(sheet_xml(&out).contains(r#"<t xml:space="preserve">  padded </t>"#));
    let out = check_edit(name, &input, at, &W::Str("plain".into()));
    assert!(sheet_xml(&out).contains("<t>plain</t>"), "no xml:space without outer whitespace");
    // a valueless inlineStr cell written with a number drops t
    let empty = existing_cells(&input)
        .into_iter()
        .find(|c| !m.iter().any(|r| r.contains(*c)) && sheet_xml(&input).contains(&format!(r#"<c r="{c}" s="1" t="inlineStr"></c>"#)))
        .expect("valueless inlineStr cell");
    let out = check_edit(name, &input, empty, &W::Num(3.0));
    assert!(sheet_xml(&out).contains(&format!(r#"<c r="{empty}" s="1"><v>3</v></c>"#)));
    assert!(!entries(&out).iter().any(|e| e.0.contains("sharedStrings")));
}

/// P0-E15: from scratch.
#[test]
fn e15_from_scratch() {
    let mut wb = Workbook::new().unwrap();
    assert_matches!(wb.add_worksheet_with_name("Data"), Err(Error::SheetNamingUnsupported));
    {
        let mut ws = wb.add_worksheet().unwrap();
        assert_eq!(ws.name(), "Sheet1");
        ws.write_string(0, 0, "a").unwrap();
        ws.write_number(1, 1, 2.0).unwrap();
    }
    assert_matches!(wb.add_worksheet(), Err(Error::SheetAdditionUnsupported));
    assert_eq!(wb.package().state("docProps/core.xml"), Some(PartStateKind::Replaced));
    let out = wb.to_bytes().unwrap();
    let template = include_bytes!("../template/blank.xlsx");
    assert_l1_except(template, &out, &["xl/worksheets/sheet1.xml", "docProps/core.xml"]);
    let core = String::from_utf8(part(&out, "docProps/core.xml")).unwrap();
    assert!(!core.contains("ooxml-rs template") && core.contains("dcterms:created"));
    let xml = sheet_xml(&out);
    assert!(xml.contains(r#"<dimension ref="A1:B2"/>"#));
    assert!(xml.contains(r#"<sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>a</t></is></c></row><row r="2"><c r="B2"><v>2</v></c></row></sheetData>"#), "{xml}");
    let cr = calamine_cells(&out);
    assert_eq!(cr["Sheet1"].len(), 2);
    assert!(calamine_matches(cr["Sheet1"].get(&(0, 0)), &CellValue::String("a".into())));
    assert!(calamine_matches(cr["Sheet1"].get(&(1, 1)), &CellValue::Number(2.0)));
}

/// ADR-0001 R2-M4: every edit fixture once more with `save` to the path it
/// was opened from.
#[test]
fn save_to_the_same_path() {
    let dir = temp_dir("same-path");
    for (name, cell) in [
        ("minimal", "B2"),
        ("poi/61605.xlsx", "A20"),
        ("poi/FillWithoutColor.xlsx", "B2"),
        ("poi/reordered_sheets.xlsx", "C3"),
        ("poi/62272.xlsx", "B2"),
        ("calamine/renamed_workbook_part.xlsx", "C1"),
        ("poi/64759.xlsx", "A1"),
        ("poi/57362.xlsx", "B2"),
        ("poi/51850.xlsx", "A1"),
        ("poi/54399.xlsx", "B6"),
        ("poi/ExcelPivotTableSample.xlsx", "A1"),
        ("calamine/pictures_in_cell_and_over_cell.xlsx", "A2"),
        ("poi/62629_target.xlsm", "A1"),
        ("poi/52348.xlsx", "A1"),
        ("poi/TwoSheetsOneHidden.xlsx", "A1"),
        ("poi/xlmmacro.xlsm", "A1"),
        ("poi/style-alternate-content.xlsx", "E50"),
    ] {
        let input = read(name);
        let path = dir.join(name.replace('/', "_"));
        std::fs::write(&path, &input).unwrap();
        let at = a1(cell);
        let mut wb = Workbook::open(&path).unwrap();
        wb.worksheet().unwrap().cell_mut(at.row, at.col).unwrap().set_number(42.0).unwrap_or_else(|e| panic!("{name}: {e}"));
        let expected = wb.to_bytes().unwrap();
        wb.save(&path).unwrap();
        let saved = std::fs::read(&path).unwrap();
        assert_eq!(entries(&saved), entries(&expected), "{name}");
        let part_name = wb.worksheet_part().to_owned();
        assert_l1_except(&input, &saved, &[&part_name]);
        assert_t2(&part(&input, &part_name), &part(&saved, &part_name), at);
        let leftovers: Vec<_> = std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()).filter(|e| e.file_name().to_string_lossy().ends_with(".tmp")).collect();
        assert!(leftovers.is_empty());
    }
}

/// Writes after writes on one workbook: strings, numbers, overwrite of an
/// inline string with a number and back, characters XML cannot carry.
#[test]
fn repeated_edits_and_escapes() {
    let name = "minimal";
    let input = read(name);
    let mut wb = Workbook::from_bytes(input.clone()).unwrap();
    {
        let mut ws = wb.worksheet().unwrap();
        ws.write_string(1, 1, "first").unwrap();
        ws.write_number(1, 1, 3.0).unwrap();
        ws.write_string(1, 1, "a\u{1}b _x0041_ c\r\nd").unwrap();
        ws.write_string(0, 0, "\ttabbed").unwrap();
        assert_matches!(ws.write_number(0, 0, f64::NAN), Err(Error::InvalidNumber(_)));
        assert_matches!(ws.write_string(0, 0, &"x".repeat(32_768)), Err(Error::StringTooLong(32_768)));
        ws.write_string(2, 0, &"\u{1F61C}".repeat(16_383)).unwrap();
    }
    let out = wb.to_bytes().unwrap();
    let xml = sheet_xml(&out);
    assert!(xml.contains(r#"<c r="B2" t="inlineStr"><is><t>a_x0001_b _x005F_x0041_ c&#13;
d</t></is></c>"#), "{xml}");
    assert!(xml.contains(r#"<t xml:space="preserve">	tabbed</t>"#), "{xml}");
    let cr = calamine_cells(&out);
    assert!(calamine_matches(cr["Sheet1"].get(&(1, 1)), &CellValue::String("a\u{1}b _x0041_ c\r\nd".into())), "{:?}", cr["Sheet1"].get(&(1, 1)));
    assert!(calamine_matches(cr["Sheet1"].get(&(0, 0)), &CellValue::String("\ttabbed".into())));
    let mut again = Workbook::from_bytes(out).unwrap();
    let ws = again.worksheet().unwrap();
    assert_eq!(ws.cell(1, 1).unwrap().unwrap().value().unwrap(), CellValue::String("a\u{1}b _x0041_ c\r\nd".into()));
}
