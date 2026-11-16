//! Expected refusals: docs/roadmap/xlsx.md §7.5. A refused call changes
//! nothing: the workbook still saves to the input's entries (L1).

mod common;

use std::assert_matches;

use common::*;
use ooxml_core::PartStateKind;
use ooxml_xlsx::{CellValue, Error, Workbook};

fn refuse_everywhere(name: &str, check: impl Fn(&Error) -> bool) {
    let input = read(name);
    let wb = Workbook::from_bytes(input.clone()).unwrap();
    assert!(wb.edit_refusal().is_some_and(|e| check(&e)), "{name}");
    // every 5th cell keeps the debug-build run short; all cells are refused alike
    let mut cells: Vec<_> = existing_cells(&input).into_iter().step_by(5).collect();
    cells.push(a1("Z99"));
    for at in cells {
        for w in [W::Num(1.0), W::Str("x".into())] {
            let (wb, r) = apply(&input, at, &w);
            let e = r.expect_err(name);
            assert!(check(&e), "{name} {at}: {e:?}");
            assert_unchanged(&wb, &input);
        }
    }
}

/// P0-R1..R3: any formula anywhere → every write refused, with or without
/// `calcChain.xml`; reads still work (cached values).
#[test]
fn r1_to_r3_formula_present() {
    for name in [
        "poi/shared_formulas.xlsx",
        "poi/70005-countifs.xlsx",
        "poi/MatrixFormulaEvalTestData.xlsx",
        "poi/xlookup.xlsx",
        "poi/NewStyleConditionalFormattings.xlsx",
    ] {
        refuse_everywhere(name, |e| matches!(e, Error::FormulaPresent { .. }));
    }
    // the calcChain-less one is caught by the element scan, not the fast path
    let wb = Workbook::from_bytes(read("poi/70005-countifs.xlsx")).unwrap();
    assert_matches!(wb.edit_refusal(), Some(Error::FormulaPresent { part }) if part == "xl/worksheets/sheet1.xml");
    // reads: a formula cell's cached string result and its formula text
    let mut wb = Workbook::from_bytes(read("poi/shared_formulas.xlsx")).unwrap();
    let ws = wb.worksheet().unwrap();
    let c = ws.cell(1, 0).unwrap().unwrap();
    assert_eq!(c.formula().as_deref(), Some("B2"));
    assert_eq!(c.value().unwrap(), CellValue::String("ProductionOrderConfirmation".into()));
    assert_matches!(c.value().unwrap(), CellValue::String(_));
}

/// P0-R4, R5: rows/cells without `r` → reads and writes refused.
#[test]
fn r4_r5_missing_reference() {
    for name in ["poi/59746_NoRowNums.xlsx", "poi/InlineString.xlsx"] {
        let input = read(name);
        let mut wb = Workbook::from_bytes(input.clone()).unwrap();
        {
            let ws = wb.worksheet().unwrap();
            assert_matches!(ws.cell(0, 0), Err(Error::MissingReference { .. }), "{name}");
        }
        let (wb, r) = apply(&input, a1("A1"), &W::Num(1.0));
        assert_matches!(r, Err(Error::MissingReference { .. }), "{name}");
        assert_unchanged(&wb, &input);
    }
}

/// P0-R6 (+ coordinator default): every cell of a merged range is refused,
/// the top-left one included.
#[test]
fn r6_merged() {
    let input = read("poi/61605.xlsx");
    for cell in ["B1", "C1", "E1", "B9"] {
        let (wb, r) = apply(&input, a1(cell), &W::Num(1.0));
        assert_matches!(r, Err(Error::MergedCell { ref range, .. }) if range.starts_with('B'), "{cell}");
        assert_unchanged(&wb, &input);
    }
}

/// P0-R7: `cell_mut` without a setter changes nothing at all.
#[test]
fn r7_cell_mut_without_setter() {
    let input = read("poi/61605.xlsx");
    let mut wb = Workbook::from_bytes(input.clone()).unwrap();
    {
        let mut ws = wb.worksheet().unwrap();
        let _ = ws.cell_mut(19, 0).unwrap();
        let _ = ws.cell_mut(1, 1).unwrap();
        let _ = ws.cell(1, 1).unwrap();
    }
    assert_unchanged(&wb, &input);
}

/// P0-R8: there is no `set_formula` in Phase0; formula writes are not
/// expressible. (Compile-time: `CellMut` has only `set_string`/`set_number`.)
#[test]
fn r8_no_formula_write_api() {
    let mut wb = Workbook::from_bytes(read("minimal")).unwrap();
    let mut ws = wb.worksheet().unwrap();
    ws.cell_mut(0, 0).unwrap().set_string("=1+1").unwrap();
    assert_eq!(ws.cell(0, 0).unwrap().unwrap().formula(), None, "a leading = is text, not a formula");
}

const WS_OPEN: &str = r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006">"#;

fn with_sheet(xml: &str) -> Vec<u8> {
    let mut pkg = ooxml_core::Package::from_bytes(read("minimal")).unwrap();
    pkg.set_part("xl/worksheets/sheet1.xml", xml.as_bytes().to_vec()).unwrap();
    pkg.to_bytes().unwrap()
}

/// P0-R9 (adapted: a missing `dimension` is never inserted, coordinator
/// default): a worksheet with an `mc:AlternateContent` child and no
/// `dimension` is writable; one that also lacks `sheetData` needs an
/// insertion next to the AC → `UnresolvedSibling`, nothing changed.
#[test]
fn r9_alternate_content_sibling() {
    let ac = "<mc:AlternateContent><mc:Choice Requires=\"x14\"><controls/></mc:Choice></mc:AlternateContent>";
    let ok = with_sheet(&format!("{WS_OPEN}<sheetData/>{ac}</worksheet>"));
    let (wb, r) = apply(&ok, a1("B2"), &W::Num(1.0));
    r.unwrap();
    let xml = String::from_utf8(part(&wb.to_bytes().unwrap(), "xl/worksheets/sheet1.xml")).unwrap();
    assert!(xml.contains(r#"<sheetData><row r="2"><c r="B2"><v>1</v></c></row></sheetData>"#) && !xml.contains("dimension"), "{xml}");
    let bad = with_sheet(&format!("{WS_OPEN}{ac}</worksheet>"));
    let (wb, r) = apply(&bad, a1("B2"), &W::Num(1.0));
    assert_matches!(r, Err(Error::Core(ooxml_core::Error::UnresolvedSibling(_))));
    assert_eq!(wb.package().state("xl/worksheets/sheet1.xml"), Some(PartStateKind::Parsed { dirty: false }));
}

/// Rows out of order, a cell whose `r` names another row, out-of-range
/// indices, and a byte-pattern `<f`-lookalike that is not a formula.
#[test]
fn malformed_shapes() {
    let rows = with_sheet(&format!(r#"{WS_OPEN}<sheetData><row r="3"/><row r="2"/></sheetData></worksheet>"#));
    assert_matches!(apply(&rows, a1("A1"), &W::Num(1.0)).1, Err(Error::OutOfOrder(_)));
    let wrong = with_sheet(&format!(r#"{WS_OPEN}<sheetData><row r="1"><c r="A2"/></row></sheetData></worksheet>"#));
    assert_matches!(apply(&wrong, a1("A1"), &W::Num(1.0)).1, Err(Error::InvalidReference(_)));
    let mut wb = Workbook::from_bytes(read("minimal")).unwrap();
    let mut ws = wb.worksheet().unwrap();
    assert_matches!(ws.cell_mut(1 << 20, 0), Err(Error::OutOfRange { .. }));
    assert_matches!(ws.cell(0, 1 << 14), Err(Error::OutOfRange { .. }));
    let fill = with_sheet(&format!(r#"{WS_OPEN}<sheetData/><filterColumn/><formula/></worksheet>"#));
    assert!(Workbook::from_bytes(fill).unwrap().edit_refusal().is_none());
    let f = with_sheet(&format!(r#"{WS_OPEN}<sheetData><row r="1"><c r="A1"><f>1</f></c></row></sheetData></worksheet>"#));
    assert_matches!(apply(&f, a1("C3"), &W::Num(1.0)).1, Err(Error::FormulaPresent { .. }));
    let dtd = with_sheet(&format!(r#"<!DOCTYPE x>{WS_OPEN}<sheetData/></worksheet>"#));
    assert_matches!(Workbook::from_bytes(dtd), Err(Error::Core(ooxml_core::Error::DtdNotAllowed { .. })));
}

/// Reads: shared strings, rich inline strings, booleans, errors, styles.
#[test]
fn reads() {
    let mut wb = Workbook::from_bytes(read("minimal")).unwrap();
    let ws = wb.worksheet().unwrap();
    let v = |r, c| ws.cell(r, c).unwrap().map(|c| c.value().unwrap());
    assert_eq!(v(0, 0), Some(CellValue::String("Name".into())));
    assert_eq!(v(1, 1), Some(CellValue::Number(90.0)));
    assert_eq!(v(1, 2), Some(CellValue::Bool(true)));
    assert_eq!(v(1, 3), Some(CellValue::Empty));
    assert_eq!(v(5, 5), None);
    assert_eq!(ws.cell_a1("A1").unwrap().unwrap().style_index().unwrap(), 1);
    let mut wb = Workbook::from_bytes(read("calamine/pictures_in_cell_and_over_cell.xlsx")).unwrap();
    let ws = wb.worksheet().unwrap();
    assert_eq!(ws.cell(0, 0).unwrap().unwrap().value().unwrap(), CellValue::Error("#VALUE!".into()));
    // every readable cell of every openable fixture decodes, and agrees with calamine
    for name in all_zip_fixtures() {
        let input = read(&name);
        let Ok(mut wb) = Workbook::from_bytes(input.clone()) else { continue };
        let sheet = wb.sheet_name().to_owned();
        let Some(cal) = try_calamine_cells(&input) else { continue }; // calamine rejects macro sheets
        let Some(cal) = cal.get(&sheet) else { continue };
        let ws = wb.worksheet().unwrap();
        if ws.cell(0, 0).is_err() {
            continue; // MissingReference fixtures
        }
        for at in existing_cells(&input) {
            let c = ws.cell(at.row, at.col).unwrap().unwrap();
            let v = c.value().unwrap_or_else(|e| panic!("{name} {at}: {e}"));
            let d = cal.get(&(at.row, at.col));
            let agree = match (&v, d) {
                (CellValue::Error(_), Some(calamine::Data::Error(_))) => true,
                (CellValue::String(s), None) if s.is_empty() => true,
                _ => calamine_matches(d, &v),
            };
            assert!(agree, "{name} {at}: ours {v:?}, calamine {d:?}");
        }
    }
}

// ---------------------------------------------------------------------------
// Implementation review round 1 (docs/reviews/xlsx-phase0-impl-round1.md)
// ---------------------------------------------------------------------------

const SHEET1: &str = "xl/worksheets/sheet1.xml";
const MC: &str = r#"xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006""#;
const AC: &str = "<mc:AlternateContent><mc:Fallback/></mc:AlternateContent>";

fn unresolved(r: &ooxml_xlsx::Result<()>) -> bool {
    matches!(r, Err(Error::Core(ooxml_core::Error::UnresolvedSibling(_))))
}

/// F1: a write that is refused changes nothing, even when the refusal comes
/// from the cell's own children. `fixture_minimal` with an
/// `mc:AlternateContent` child added to A2 (value "Alice") and to the
/// value-less D2 (given `t="b"`): before the fix, the setters removed the
/// old value / `t`, then failed, leaving the part dirty and "Alice" lost on
/// save.
#[test]
fn f1_err_inside_cell_changes_nothing() {
    let input = read("minimal");
    let sheet = text(&input, SHEET1)
        .replace(r#"<c r="A2" t="s"><v>1</v></c>"#, &format!(r#"<c r="A2" t="s" {MC}><v>1</v>{AC}</c>"#))
        .replace(r#"<c r="D2" s="1"/>"#, &format!(r#"<c r="D2" s="1" t="b" {MC}>{AC}</c>"#));
    let bytes = rebuild(&input, &[(SHEET1, sheet.into_bytes())]);
    for (cell, w) in [
        ("A2", W::Str("new".into())),
        ("A2", W::Num(1.0)),
        ("D2", W::Num(1.0)),
        ("D2", W::Str("x".into())),
    ] {
        let (mut wb, r) = apply(&bytes, a1(cell), &w);
        assert!(unresolved(&r), "{cell} {w:?}: {r:?}");
        assert_unchanged(&wb, &bytes);
        // the caller who skips the failed cell and saves keeps the old value
        let v = wb.worksheet().unwrap().cell(1, 0).unwrap().unwrap().value().unwrap();
        assert_eq!(v, CellValue::String("Alice".into()));
    }
    // a neighbour without AC is still writable
    let (_, r) = apply(&bytes, a1("B2"), &W::Num(2.0));
    r.unwrap();
}

/// F1 + F6: an `mc:AlternateContent` child of the row concerned, or of
/// `sheetData` (rows wrapped in AC), may hold the cell: reads are `Err`
/// rather than `Ok(None)`, writes are refused before anything changes.
#[test]
fn f6_alternate_content_around_rows_and_cells() {
    let input = read("minimal");
    // (a) the row-1 element wrapped in AC inside sheetData
    let rows = patched(
        &input,
        SHEET1,
        "<sheetData>\n<row r=\"1\">\n<c r=\"A1\" s=\"1\" t=\"s\"><v>0</v></c>\n</row>",
        &format!(
            "<sheetData {MC}><mc:AlternateContent><mc:Choice Requires=\"x\"/><mc:Fallback><row r=\"1\"><c r=\"A1\" s=\"1\" t=\"s\"><v>0</v></c></row></mc:Fallback></mc:AlternateContent>"
        ),
    );
    // (b) the A2.. cells of row 2 followed by an AC
    let cells =
        patched(&input, SHEET1, "<row r=\"2\">", &format!("<row r=\"2\" {MC}><mc:AlternateContent><mc:Fallback/></mc:AlternateContent>"));
    for (what, sheet) in [("rows in AC", rows), ("AC in row 2", cells)] {
        let bytes = rebuild(&input, &[(SHEET1, sheet)]);
        let mut wb = Workbook::from_bytes(bytes.clone()).unwrap();
        {
            let ws = wb.worksheet().unwrap();
            let read = |r, c| ws.cell(r, c).map(|c| c.is_some());
            if what == "rows in AC" {
                assert_matches!(read(0, 0), Err(Error::Core(ooxml_core::Error::UnresolvedSibling(_))), "{what}");
            }
            assert_matches!(read(1, 0), Err(Error::Core(ooxml_core::Error::UnresolvedSibling(_))), "{what}");
            assert_matches!(read(1, 7), Err(Error::Core(ooxml_core::Error::UnresolvedSibling(_))), "{what}");
        }
        assert_unchanged(&wb, &bytes);
        for cell in ["A2", "H2", "B2"] {
            let (wb, r) = apply(&bytes, a1(cell), &W::Num(1.0));
            assert!(unresolved(&r), "{what} {cell}: {r:?}");
            assert_unchanged(&wb, &bytes);
        }
    }
    // (c) a worksheet with an AC and no sheetData: sheetData may be inside it
    let bytes = with_sheet(&format!("{WS_OPEN}<mc:AlternateContent><mc:Fallback><sheetData/></mc:Fallback></mc:AlternateContent></worksheet>"));
    let mut wb = Workbook::from_bytes(bytes).unwrap();
    assert_matches!(wb.worksheet().unwrap().cell(0, 0), Err(Error::Core(ooxml_core::Error::UnresolvedSibling(_))));
}

/// F7: a `dimension` whose `ref` is missing or not a cell range refuses
/// writes with `Malformed` (it could not be kept up to date); a parseable
/// one is grown (lower case is normalized) or left alone if it already
/// covers the cell.
#[test]
fn f7_dimension_ref() {
    let input = read("minimal");
    let with_dim = |dim: &str| rebuild(&input, &[(SHEET1, patched(&input, SHEET1, r#"<dimension ref="A1:D2"/>"#, dim))]);
    for dim in [r#"<dimension ref="A1:D"/>"#, r#"<dimension ref="A1:D2:E3"/>"#, r#"<dimension ref=""/>"#, "<dimension/>"] {
        let bytes = with_dim(dim);
        let (wb, r) = apply(&bytes, a1("F9"), &W::Num(1.0));
        assert_matches!(r, Err(Error::Malformed(_)), "{dim}");
        assert_unchanged(&wb, &bytes);
        let (wb, r) = apply(&bytes, a1("B2"), &W::Num(1.0));
        assert_matches!(r, Err(Error::Malformed(_)), "{dim}: refused inside the range too");
        assert_unchanged(&wb, &bytes);
    }
    for (dim, want) in [("a1:d2", "A1:F9"), ("A1", "A1:F9"), ("A1:XFD1048576", "A1:XFD1048576")] {
        let bytes = with_dim(&format!(r#"<dimension ref="{dim}"/>"#));
        let (wb, r) = apply(&bytes, a1("F9"), &W::Num(1.0));
        r.unwrap();
        assert!(sheet_xml(&wb.to_bytes().unwrap()).contains(&format!(r#"<dimension ref="{want}"/>"#)), "{dim}");
    }
}

