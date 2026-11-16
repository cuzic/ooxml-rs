//! Fuzz finding F-8 (`docs/testing/fuzz-findings-2026-11.md` on the fuzz
//! PRs): a cell whose `r` names another row than its `row/@r`.
//!
//! The formula census keys cells by `c/@r`, the row index by row, so the
//! two disagree about where such a cell is; a write to the named cell used
//! to add a second `c` with the same reference (and keep `calcChain.xml`,
//! the census seeing no change in the formula set). Every edit of such a
//! workbook is now refused with `InvalidReference` (fail closed, as for a
//! formula sheet without `r` attributes); reads keep their Phase0
//! behaviour (the row holding the misplaced cell is refused).

mod common;

use std::assert_matches;

use common::p1::*;
use common::*;
use ooxml_xlsx::{Error, Workbook};

fn refuses_every_edit(input: &[u8]) {
    let mut wb = Workbook::from_bytes(input.to_vec()).expect("opens");
    let before = wb.to_bytes().unwrap();
    assert_matches!(wb.edit_refusal(), Some(Error::InvalidReference(ref m)) if m.contains("B2 in row 1"), "{:?}", wb.edit_refusal());
    let names: Vec<String> = wb.sheet_names().map(str::to_owned).collect();
    for name in &names {
        let mut ws = wb.worksheet_by_name(name).unwrap();
        for (row, col) in [(1, 1), (0, 0), (5, 5)] {
            assert_matches!(ws.cell_mut(row, col).unwrap().set_number(7.0), Err(Error::InvalidReference(_)), "{name} ({row},{col})");
            assert_matches!(ws.cell_mut(row, col).unwrap().set_string("x"), Err(Error::InvalidReference(_)));
            assert_matches!(ws.cell_mut(row, col).unwrap().set_formula("A1*3"), Err(Error::InvalidReference(_)));
            assert_matches!(ws.cell_mut(row, col).unwrap().remove_formula(), Err(Error::InvalidReference(_)));
        }
    }
    assert_eq!(wb.to_bytes().unwrap(), before, "a refused edit changed the package");
    // reads keep their Phase0 behaviour (rows without a misplaced cell read
    // normally)
    assert_matches!(wb.worksheet().unwrap().cell(7, 0), Ok(None));
}

/// The fuzzer's reproducer (PR #65, `f8_formula_cell_in_another_row_is_not_duplicated`).
#[test]
fn f8_formula_cell_in_another_row_is_not_duplicated() {
    let input = book(&[("Sheet1", r#"<row r="1"><c r="A1"><v>1</v></c><c r="B2"><f>A1*2</f><v>2</v></c></row>"#)], "", "");
    let mut wb = Workbook::from_bytes(input.clone()).expect("opens");
    if wb.worksheet().unwrap().cell_mut(1, 1).unwrap().set_formula("A1*3").is_ok() {
        let out = wb.to_bytes().unwrap();
        let sheet = String::from_utf8(part(&out, "xl/worksheets/sheet1.xml")).unwrap();
        assert_eq!(sheet.matches(r#"r="B2""#).count(), 1, "two cells named B2:\n{sheet}");
    }
    refuses_every_edit(&input);
    // the row holding the misplaced cell is refused for reads, as in Phase0
    let mut wb = Workbook::from_bytes(input).unwrap();
    assert_matches!(wb.worksheet().unwrap().cell(0, 0), Err(Error::InvalidReference(_)));
}

/// The same with no formula at all, and with the mismatch on another
/// sheet than the edited one: a value write would add a duplicate too, and
/// a formula elsewhere may read the misplaced cell.
#[test]
fn a_cell_naming_another_row_blocks_every_edit() {
    refuses_every_edit(&book(&[("Sheet1", r#"<row r="1"><c r="A1"><v>1</v></c><c r="B2"><v>2</v></c></row>"#)], "", ""));
    refuses_every_edit(&book(
        &[
            ("Sheet1", r#"<row r="1"><c r="A1"><v>1</v></c></row><row r="2"><c r="B2"><f>Sheet2!A1</f><v>0</v></c></row>"#),
            ("Sheet2", r#"<row r="1"><c r="A1"><v>0</v></c><c r="B2"><v>3</v></c></row>"#),
        ],
        "",
        "",
    ));
}

/// Control: the same cells in their own rows edit normally, without a
/// duplicate.
#[test]
fn cells_in_their_own_rows_still_edit() {
    let input = book(&[("Sheet1", r#"<row r="1"><c r="A1"><v>1</v></c></row><row r="2"><c r="B2"><f>A1*2</f><v>2</v></c></row>"#)], "", "");
    let mut wb = Workbook::from_bytes(input).expect("opens");
    assert!(wb.edit_refusal().is_none());
    wb.worksheet().unwrap().cell_mut(1, 1).unwrap().set_formula("A1*3").unwrap();
    wb.worksheet().unwrap().cell_mut(0, 0).unwrap().set_number(5.0).unwrap();
    let out = wb.to_bytes().unwrap();
    let sheet = String::from_utf8(part(&out, "xl/worksheets/sheet1.xml")).unwrap();
    assert_eq!(sheet.matches(r#"r="B2""#).count(), 1, "{sheet}");
    assert!(sheet.contains("<f>A1*3</f>"), "{sheet}");
}

/// Corpus check through the public API: no fixture is refused with
/// `InvalidReference` (the per-part check of the census itself is the unit
/// test `census::tests::no_fixture_sheet_has_a_row_mismatch`).
#[test]
fn no_fixture_is_refused_by_the_row_check() {
    let mut names = all_zip_fixtures();
    let synthetic = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reference/fixtures/xlsx/synthetic");
    names.extend(
        std::fs::read_dir(synthetic)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .filter(|n| n.ends_with(".xlsx"))
            .map(|n| format!("synthetic/{n}")),
    );
    let mut opened = 0;
    for name in &names {
        let Ok(wb) = Workbook::from_bytes(read(name)) else { continue };
        opened += 1;
        assert!(!matches!(wb.edit_refusal(), Some(Error::InvalidReference(_))), "{name}: {:?}", wb.edit_refusal());
    }
    assert!(opened >= 40, "{opened} fixtures opened");
}

/// `row/@r` is read as the row index reads it: XML whitespace around the
/// number does not hide a mismatch (review of PR #67, M1), and row 0 stays
/// the index's own refusal of that sheet, not a workbook-wide mismatch (m1).
#[test]
fn padded_row_numbers_and_row_zero() {
    for r in [" 1", "\t1", "1 "] {
        let input = book(&[("Sheet1", &format!(r#"<row r="{r}"><c r="A1"><v>1</v></c><c r="B2"><f>A1*2</f><v>2</v></c></row>"#))], "", "");
        let mut wb = Workbook::from_bytes(input).expect("opens");
        assert_matches!(wb.edit_refusal(), Some(Error::InvalidReference(_)), "row r={r:?}");
        let before = wb.to_bytes().unwrap();
        assert_matches!(wb.worksheet().unwrap().cell_mut(1, 1).unwrap().set_formula("A1*3"), Err(Error::InvalidReference(_)));
        assert_eq!(wb.to_bytes().unwrap(), before);
    }
    let input = book(
        &[("Sheet1", r#"<row r="0"><c r="A1"><v>1</v></c></row>"#), ("Sheet2", r#"<row r="1"><c r="A1"><v>1</v></c></row>"#)],
        "",
        "",
    );
    let mut wb = Workbook::from_bytes(input).expect("opens");
    assert!(wb.edit_refusal().is_none(), "{:?}", wb.edit_refusal());
    assert_matches!(wb.worksheet_by_name("Sheet1").unwrap().cell_mut(0, 0).unwrap().set_number(2.0), Err(Error::InvalidReference(ref m)) if m == "0");
    wb.worksheet_by_name("Sheet2").unwrap().cell_mut(0, 0).unwrap().set_number(2.0).expect("another sheet still edits");
}
