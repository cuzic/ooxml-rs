//! O1 (ADR-0003 §7.1): hand-written expected dependent sets on the synthetic
//! workbooks of `reference/fixtures/xlsx/synthetic/` (see its README). Each
//! edit also runs the allowed-diff and no-stale-value oracles.

mod common;

use std::assert_matches;

use common::p1::*;
use common::*;
use ooxml_xlsx::{Error, Workbook};

#[test]
fn o1_precise_cross_sheet_names_3d_whole_rows_and_columns() {
    let wb = synthetic("o1_precise.xlsx");
    value_edit(&wb, "Sheet1", "A1", 50.0, &[
        "Sheet1!B1", "Sheet2!B1", "Sheet2!C1", "Sheet3!A1", "Sheet3!C1", "Sheet3!E1", "Sheet3!J1",
    ]);
    value_edit(&wb, "Sheet1", "A2", 100.0, &["Sheet1!E1", "Sheet3!D1", "Sheet3!E1", "Sheet3!H1", "Sheet3!J1"]);
    value_edit(&wb, "Sheet1", "A3", -7.5, &["Sheet1!E1", "Sheet3!E1", "Sheet3!F1", "Sheet3!G1", "Sheet3!J1"]);
    value_edit(&wb, "Sheet2", "A1", 1000.0, &["Sheet1!D1", "Sheet2!D1", "Sheet3!B1", "Sheet3!C1", "Sheet3!I1"]);
    // SUMIF's sum_range B1 is resized to B1:B3 (range A1:A3's shape).
    value_edit(&wb, "Sheet1", "B2", 3.0, &["Sheet3!J1"]);
    value_edit(&wb, "Sheet1", "B3", 3.0, &["Sheet3!F1", "Sheet3!J1"]);
    // Nothing reads these: no cached value is touched, workbook.xml unchanged.
    value_edit(&wb, "Defines", "A1", 2.0, &[]);
    value_edit(&wb, "sheet3", "Z9", 2.0, &[]);
}

#[test]
fn o1_everything_constructs() {
    for tag in [
        "indirect", "offset", "lookup", "sumif_computed", "range_computed", "udf", "external", "unknown_sheet",
        "structured", "relative_name", "name_collision", "volatile_ca", "spill_error", "data_table", "bx_name",
    ] {
        let wb = synthetic(&format!("o1_everything_{tag}.xlsx"));
        // Z100 is named by no formula: only Everything (B1) and its reader D1.
        value_edit(&wb, "Sheet1", "Z100", 42.0, &["Sheet1!B1", "Sheet1!D1"]);
    }
}

#[test]
fn o1_arrays_and_spill_growth() {
    let wb = synthetic("o1_arrays.xlsx");
    value_edit(&wb, "Sheet1", "A1", 10.0, &[
        "Sheet1!B1", "Sheet1!B2", "Sheet1!B3", "Sheet1!C1", "Sheet1!D1", "Sheet1!D2", "Sheet1!D3", "Sheet1!E1",
    ]);
    value_edit(&wb, "Sheet1", "A9", 10.0, &["Sheet1!F1"]);
    let input = wb.clone();
    for (at, anchor) in [("B2", true), ("B3", true), ("D3", true), ("B1", false), ("D1", false)] {
        let mut w = Workbook::from_bytes(input.clone()).unwrap();
        let a = ooxml_xlsx::CellIndex::parse(at).unwrap();
        let r = w.worksheet().unwrap().cell_mut(a.row, a.col).unwrap().set_number(1.0);
        if anchor {
            assert_matches!(r, Err(Error::ArrayResultCell { .. }), "{at}");
        } else {
            assert_matches!(r, Err(Error::ArrayAnchor { .. }), "{at}");
        }
        assert_l1_except(&input, &w.to_bytes().unwrap(), &[]);
    }
}

#[test]
fn o1_shared_groups() {
    let wb = synthetic("o1_shared.xlsx");
    value_edit(&wb, "Sheet2", "A3", 9.0, &["Sheet2!B3", "Sheet2!C3", "Sheet2!D3", "Sheet2!E3", "Sheet2!H9"]);
    value_edit(&wb, "Sheet2", "D1", 9.0, &["Sheet2!D2", "Sheet2!D3"]);
    // The member G2 of o1_shared_oob translates out of the sheet: Everything.
    let oob = synthetic("o1_shared_oob.xlsx");
    value_edit(&oob, "Q1 2024", "A2", 9.0, &["Sheet2!G2", "Sheet2!G3"]);
}

#[test]
fn o1_name_scope_unqualified_reference_in_a_local_name() {
    let wb = synthetic("o1_name_scope.xlsx");
    value_edit(&wb, "S1", "Z9", 3.0, &["S1!C1"]);
    value_edit(&wb, "S1", "A1", 3.0, &["S1!C1"]);
}

#[test]
fn o1_manual_mode_strips_but_does_not_set_full_calc() {
    let wb = synthetic("o1_manual.xlsx");
    let out = value_edit(&wb, "Sheet1", "A1", 5.0, &["Sheet1!B1"]);
    let w = Workbook::from_bytes(out.clone()).unwrap();
    assert!(!String::from_utf8(part(&out, w.workbook_part())).unwrap().contains("fullCalcOnLoad"));
}

#[test]
fn o1_calcpr_is_inserted_when_absent() {
    let wb = synthetic("o1_no_calcpr.xlsx");
    let out = value_edit(&wb, "Sheet1", "A1", 5.0, &["Sheet1!B1"]);
    let w = Workbook::from_bytes(out.clone()).unwrap();
    let x = String::from_utf8(part(&out, w.workbook_part())).unwrap();
    assert!(x.contains("<calcPr fullCalcOnLoad=\"1\"/></workbook>"), "{x}");
}

#[test]
fn o1_workbook_wide_refusals() {
    // Iterative calculation: an edit with dependents is refused, one without is fine.
    let it = synthetic("o1_iterate.xlsx");
    let mut w = Workbook::from_bytes(it.clone()).unwrap();
    assert_matches!(w.worksheet().unwrap().cell_mut(0, 1).unwrap().set_number(2.0), Err(Error::IterativeCalculation));
    assert_l1_except(&it, &w.to_bytes().unwrap(), &[]);
    value_edit(&it, "Sheet1", "C9", 1.0, &[]);
    // A macro-sheet formula would become a dependent.
    let m = synthetic("o1_macro.xlsx");
    let mut w = Workbook::from_bytes(m.clone()).unwrap();
    assert_matches!(w.worksheet().unwrap().cell_mut(0, 0).unwrap().set_number(2.0), Err(Error::MacroSheetDependent { .. }));
    assert_l1_except(&m, &w.to_bytes().unwrap(), &[]);
    value_edit(&m, "Sheet1", "C9", 1.0, &[]);
    // Unknown cell metadata: every edit refused, reads still work.
    let u = synthetic("o1_unknown_cm.xlsx");
    let mut w = Workbook::from_bytes(u.clone()).unwrap();
    assert_matches!(w.edit_refusal(), Some(Error::UnknownCellMetadata { .. }));
    assert_matches!(w.worksheet().unwrap().cell_mut(5, 5).unwrap().set_number(2.0), Err(Error::UnknownCellMetadata { .. }));
    assert_eq!(w.worksheet().unwrap().cell(0, 1).unwrap().unwrap().formula().as_deref(), Some("A1*2"));
    assert_l1_except(&u, &w.to_bytes().unwrap(), &[]);
    // Sheet lookup errors.
    assert_matches!(w.worksheet_by_name("nope"), Err(Error::NoSuchSheet(_)));
    let mut w = Workbook::from_bytes(m).unwrap();
    assert_matches!(w.worksheet_by_name("Macro1"), Err(Error::NotAWorksheet(_)));
    assert_matches!(w.worksheet_by_index(1), Err(Error::NotAWorksheet(_)));
    assert_matches!(w.worksheet_by_index(2), Err(Error::NoSuchSheet(_)));
    assert!(w.worksheet_by_index(0).is_ok());
}
