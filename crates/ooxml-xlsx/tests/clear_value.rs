//! `CellMut::clear_value` (ADR-0024 P1): ExcelJS `cell.value = null`.
//! The cell keeps its style, a cell that does not exist is left alone.

mod common;

use std::assert_matches;

use common::p1::*;
use common::*;
use ooxml_xlsx::{CellIndex, CellWrite, Error, Workbook};

const SHEET: &str = "xl/worksheets/sheet1.xml";

fn a(s: &str) -> CellIndex {
    CellIndex::parse(s).unwrap()
}

fn clear(input: &[u8], at: &str) -> (Workbook, ooxml_xlsx::Result<()>) {
    let mut wb = Workbook::from_bytes(input.to_vec()).unwrap();
    let r = {
        let mut ws = wb.worksheet().unwrap();
        let i = a(at);
        ws.cell_mut(i.row, i.col).unwrap().clear_value()
    };
    (wb, r)
}

fn cleared_sheet(rows: &str, at: &str) -> String {
    let input = book(&[("Sheet1", rows)], "", "");
    let (wb, r) = clear(&input, at);
    r.unwrap();
    String::from_utf8(part(&wb.to_bytes().unwrap(), SHEET)).unwrap()
}

/// An emptied `c` is written `<c ..></c>` or `<c ../>` depending on whether
/// it had children; both are the same element.
fn has(x: &str, cell: &str) {
    let x = &x.replace(r#""></c>"#, r#""/>"#);
    assert!(x.contains(cell), "missing {cell} in:\n{x}");
}

#[test]
fn clears_number_bool_error_and_keeps_style() {
    let rows = r#"<row r="1"><c r="A1" s="3"><v>1.5</v></c><c r="B1" s="2" t="b"><v>1</v></c><c r="C1" s="4" t="e"><v>#N/A</v></c><c r="D1"><v>7</v></c></row>"#;
    has(&cleared_sheet(rows, "A1"), r#"<c r="A1" s="3"/>"#);
    has(&cleared_sheet(rows, "B1"), r#"<c r="B1" s="2"/>"#);
    has(&cleared_sheet(rows, "C1"), r#"<c r="C1" s="4"/>"#);
    has(&cleared_sheet(rows, "D1"), r#"<c r="D1"/>"#);
}

#[test]
fn clears_inline_plain_and_rich_strings() {
    let rows = r#"<row r="1"><c r="A1" s="3" t="inlineStr"><is><t>hi</t></is></c><c r="B1" s="1" t="inlineStr"><is><r><t>a</t></r><r><t>b</t></r></is></c></row>"#;
    has(&cleared_sheet(rows, "A1"), r#"<c r="A1" s="3"/>"#);
    has(&cleared_sheet(rows, "B1"), r#"<c r="B1" s="1"/>"#);
}

#[test]
fn clears_shared_string_cell() {
    let base = book(&[("Sheet1", r#"<row r="1"><c r="A1" s="2" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c></row>"#)], "", "");
    let sst = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="2" uniqueCount="2"><si><t>x</t></si><si><t>y</t></si></sst>"#;
    let ct = patched(&base, "[Content_Types].xml", "</Types>", r#"<Override PartName="/xl/sharedStrings.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml"/></Types>"#);
    let rels = patched(&base, "xl/_rels/workbook.xml.rels", "</Relationships>", r#"<Relationship Id="rId9" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings" Target="sharedStrings.xml"/></Relationships>"#);
    let input = rebuild(&base, &[("[Content_Types].xml", ct), ("xl/_rels/workbook.xml.rels", rels), ("xl/sharedStrings.xml", sst.as_bytes().to_vec())]);
    let (wb, r) = clear(&input, "A1");
    r.unwrap();
    let out = wb.to_bytes().unwrap();
    let x = text(&out, SHEET);
    has(&x, r#"<c r="A1" s="2"/>"#);
    has(&x, r#"<c r="B1" t="s"><v>1</v></c>"#);
    // the shared-string table is kept (entries are never garbage-collected here)
    assert!(text(&out, "xl/sharedStrings.xml").contains("<si><t>x</t></si>"));
    let mut again = Workbook::from_bytes(out).unwrap();
    let ws = again.worksheet().unwrap();
    assert_eq!(ws.cell_a1("A1").unwrap().unwrap().style_index().unwrap(), 2);
}

#[test]
fn clears_formula_with_cache_and_invalidates_dependents() {
    let rows = r#"<row r="1"><c r="A1"><v>2</v></c><c r="B1" s="5"><f>A1*2</f><v>4</v></c><c r="C1"><f>B1+1</f><v>5</v></c></row>"#;
    let x = cleared_sheet(rows, "B1");
    has(&x, r#"<c r="B1" s="5"/>"#);
    assert!(!x.contains("A1*2"), "{x}");
    // the dependent's cached value is stripped (as for remove_formula)
    has(&x, r#"<f>B1+1</f></c>"#);
    // reopens: B1 reads as empty with style kept
    let input = book(&[("Sheet1", rows)], "", "");
    let (wb, r) = clear(&input, "B1");
    r.unwrap();
    let mut again = Workbook::from_bytes(wb.to_bytes().unwrap()).unwrap();
    let ws = again.worksheet().unwrap();
    let c = ws.cell_a1("B1").unwrap().unwrap();
    assert_eq!(c.formula(), None);
    assert_eq!(c.style_index().unwrap(), 5);
}

#[test]
fn shared_formula_master_hands_off_and_member_is_removed() {
    let rows = r#"<row r="1"><c r="A1"><v>1</v></c><c r="B1" s="1"><f t="shared" ref="B1:B3" si="0">A1*2</f><v>2</v></c></row><row r="2"><c r="A2"><v>2</v></c><c r="B2"><f t="shared" si="0"/><v>4</v></c></row><row r="3"><c r="A3"><v>3</v></c><c r="B3"><f t="shared" si="0"/><v>6</v></c></row>"#;
    let x = cleared_sheet(rows, "B1");
    has(&x, r#"<c r="B1" s="1"/>"#);
    has(&x, r#"<c r="B2"><f t="shared" si="0" ref="B2:B3">A2*2</f>"#);
    has(&x, r#"<c r="B3"><f t="shared" si="0"/>"#);
    let x = cleared_sheet(rows, "B3");
    has(&x, r#"<c r="B3"/>"#);
    has(&x, r#"<f t="shared" ref="B1:B3" si="0">A1*2</f>"#);
}

#[test]
fn array_cells_are_refused_like_other_writes() {
    let rows = r#"<row r="1"><c r="A1"><v>1</v></c><c r="B1"><f t="array" ref="B1:B2">A1*2</f><v>2</v></c></row><row r="2"><c r="B2"><v>4</v></c></row>"#;
    let input = book(&[("Sheet1", rows)], "", "");
    for at in ["B1", "B2"] {
        let (wb, r) = clear(&input, at);
        let mut other = Workbook::from_bytes(input.clone()).unwrap();
        let o = {
            let i = a(at);
            other.worksheet().unwrap().cell_mut(i.row, i.col).unwrap().set_number(1.0)
        };
        assert_matches!(r, Err(Error::ArrayAnchor { .. } | Error::ArrayResultCell { .. }), "{at}");
        assert_eq!(std::mem::discriminant(&r.unwrap_err()), std::mem::discriminant(&o.unwrap_err()), "{at}");
        assert_eq!(wb.to_bytes().unwrap(), input, "{at}: Err must leave the package unchanged");
    }
}

#[test]
fn missing_cell_under_a_column_style_is_untouched() {
    let input = book(&[("Sheet1", r#"<row r="1"><c r="A1"><v>1</v></c></row>"#)], "", "");
    let sheet = patched(&input, SHEET, "<sheetData>", r#"<cols><col min="2" max="2" width="9" style="1"/></cols><sheetData>"#);
    let input = rebuild(&input, &[(SHEET, sheet)]);
    for at in ["B6", "B1", "Z99"] {
        let (wb, r) = clear(&input, at);
        r.unwrap();
        assert_eq!(wb.to_bytes().unwrap(), input, "{at}");
    }
    // in a batch, too, next to a real write
    let mut wb = Workbook::from_bytes(input.clone()).unwrap();
    wb.worksheet().unwrap().write_cells(&[(a("B6"), CellWrite::ClearValue)]).unwrap();
    assert_eq!(wb.to_bytes().unwrap(), input);
}

#[test]
fn set_number_then_clear_on_a_missing_cell_leaves_an_empty_c_without_style() {
    let input = book(&[("Sheet1", r#"<row r="1"><c r="A1"><v>1</v></c></row>"#)], "", "");
    let mut wb = Workbook::from_bytes(input).unwrap();
    {
        let mut ws = wb.worksheet().unwrap();
        let i = a("B6");
        ws.cell_mut(i.row, i.col).unwrap().set_number(5.0).unwrap();
        ws.cell_mut(i.row, i.col).unwrap().clear_value().unwrap();
    }
    let out = wb.to_bytes().unwrap();
    let x = String::from_utf8(part(&out, SHEET)).unwrap();
    has(&x, r#"<row r="6"><c r="B6"/></row>"#);
    let mut again = Workbook::from_bytes(out).unwrap();
    let ws = again.worksheet().unwrap();
    let c = ws.cell_a1("B6").unwrap().unwrap();
    assert_eq!(c.formula(), None);
    assert_eq!(c.style_index().unwrap(), 0);
}

#[test]
fn batch_clear_matches_single_clear() {
    let rows = r#"<row r="1"><c r="A1" s="2"><v>1</v></c><c r="B1"><f>A1</f><v>1</v></c></row>"#;
    let input = book(&[("Sheet1", rows)], "", "");
    let (single, r) = clear(&input, "B1");
    r.unwrap();
    let mut wb = Workbook::from_bytes(input).unwrap();
    wb.worksheet().unwrap().write_cells(&[(a("B1"), CellWrite::ClearValue), (a("A1"), CellWrite::ClearValue)]).unwrap();
    let x = String::from_utf8(part(&wb.to_bytes().unwrap(), SHEET)).unwrap();
    has(&x, r#"<c r="A1" s="2"/><c r="B1"/>"#);
    let _ = single;
}
