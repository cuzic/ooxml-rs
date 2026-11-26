//! ADR-0003 addendum (P5-pre): the multi-cell write plan, through
//! `Worksheet::write_cells`, and the future-function file form (ADR-0017 Q7).

mod common;

use std::assert_matches;

use common::p1::*;
use common::*;
use ooxml_xlsx::{CellIndex, CellWrite, Error, Workbook};

fn at(s: &str) -> CellIndex {
    CellIndex::parse(s).unwrap()
}

fn xml_of(bytes: &[u8], sheet: &str) -> String {
    String::from_utf8(part(bytes, &sheet_parts(bytes)[sheet])).unwrap()
}

type Batch<'a> = Vec<(&'a str, CellWrite<'a>)>;

fn batch(input: &[u8], sheet: &str, writes: &Batch<'_>) -> (Workbook, ooxml_xlsx::Result<()>) {
    let mut wb = Workbook::from_bytes(input.to_vec()).unwrap();
    let w: Vec<(CellIndex, CellWrite<'_>)> = writes.iter().map(|(c, w)| (at(c), *w)).collect();
    let r = wb.worksheet_by_name(sheet).unwrap().write_cells(&w);
    (wb, r)
}

fn sequential(input: &[u8], sheet: &str, writes: &Batch<'_>) -> Vec<u8> {
    let mut wb = Workbook::from_bytes(input.to_vec()).unwrap();
    {
        let mut ws = wb.worksheet_by_name(sheet).unwrap();
        for (c, w) in writes {
            let a = at(c);
            let cell = ws.cell_mut(a.row, a.col).unwrap();
            match *w {
                CellWrite::Number(v) => cell.set_number(v),
                CellWrite::String(s) => cell.set_string(s),
                CellWrite::Formula(t) => cell.set_formula(t),
                CellWrite::RemoveFormula => cell.remove_formula(),
                CellWrite::ClearValue => cell.clear_value(),
            }
            .unwrap();
        }
    }
    wb.to_bytes().unwrap()
}

/// A batch gives the same bytes as the same writes one by one: one
/// `fullCalcOnLoad`, one calcChain removal, the union of the strips.
#[test]
fn a_batch_equals_the_same_writes_in_sequence() {
    let cases: Vec<(&str, &str, Batch<'_>)> = vec![
        ("o1_precise.xlsx", "Sheet1", vec![("A1", CellWrite::Number(5.0)), ("A2", CellWrite::Number(6.0)), ("B10", CellWrite::Formula("A1+A2")), ("C10", CellWrite::String("label")), ("D11", CellWrite::Number(1.0))]),
        ("o1_precise.xlsx", "Sheet2", vec![("A1", CellWrite::Number(9.0)), ("B1", CellWrite::RemoveFormula), ("Z1", CellWrite::Formula("=B1*2"))]),
        ("o1_shared.xlsx", "Sheet2", vec![("D2", CellWrite::Number(99.0)), ("A2", CellWrite::Number(7.0))]),
        ("o1_no_calcpr.xlsx", "Sheet1", vec![("A1", CellWrite::Number(3.0)), ("C1", CellWrite::Formula("A1*4"))]),
    ];
    for (name, sheet, writes) in cases {
        let input = synthetic(name);
        let (wb, r) = batch(&input, sheet, &writes);
        r.unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(wb.to_bytes().unwrap(), sequential(&input, sheet, &writes), "{name} {sheet}");
    }
}

#[test]
fn a_batch_hands_off_both_shared_formula_masters() {
    let input = synthetic("o1_shared.xlsx");
    let writes = vec![("D2", CellWrite::Number(1.0)), ("B2", CellWrite::Number(2.0))];
    let (wb, result) = batch(&input, "Sheet2", &writes);
    result.unwrap();
    let out = wb.to_bytes().unwrap();
    assert_eq!(out, sequential(&input, "Sheet2", &writes));
    let xml = xml_of(&out, "Sheet2");
    assert_eq!(xml.split("<f t=\"shared\"").skip(1).filter(|f| f.split('>').next().unwrap().contains("ref=")).count(), 2, "{xml}");
}

#[test]
fn batch_uses_the_shared_string_registry_and_allows_the_merge_anchor() {
    let input = Workbook::new().unwrap().to_bytes().unwrap();
    let input = with_part(&input, "xl/worksheets/sheet1.xml",
        r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData/><mergeCells count="1"><mergeCell ref="A1:B1"/></mergeCells></worksheet>"#);
    let writes = vec![("A1", CellWrite::String("same")), ("C1", CellWrite::String("same")), ("D1", CellWrite::String("other"))];
    let (wb, result) = batch(&input, "Sheet1", &writes);
    result.unwrap();
    let out = wb.to_bytes().unwrap();
    assert_eq!(out, sequential(&input, "Sheet1", &writes));
    let sst = String::from_utf8(part(&out, "xl/sharedStrings.xml")).unwrap();
    assert!(sst.contains(r#"count="3" uniqueCount="2"><si><t>same</t></si><si><t>other</t></si>"#), "{sst}");
    let xml = xml_of(&out, "Sheet1");
    assert!(xml.contains(r#"<c r="A1" t="s"><v>0</v></c><c r="C1" t="s"><v>0</v></c><c r="D1" t="s"><v>1</v></c>"#), "{xml}");

    let (wb, result) = batch(&input, "Sheet1", &vec![("A1", CellWrite::String("same")), ("B1", CellWrite::Number(1.0))]);
    assert_matches!(result, Err(Error::MergedCell { .. }));
    assert_eq!(wb.to_bytes().unwrap(), input);
}

#[test]
fn batch_refusals_for_workbook_gate_and_sst_leave_identical_bytes() {
    let input = Workbook::new().unwrap().to_bytes().unwrap();
    let writes = vec![("C1", CellWrite::Number(1.0)), ("D1", CellWrite::String("new"))];
    let malformed_row = with_part(&input, "xl/worksheets/sheet1.xml",
        r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A2"><v>1</v></c></row></sheetData></worksheet>"#);
    let (wb, result) = batch(&malformed_row, "Sheet1", &writes);
    assert_matches!(result, Err(Error::InvalidReference(ref message)) if message.contains("A2 in row 1"));
    assert_eq!(wb.to_bytes().unwrap(), malformed_row);

    let unstable_sst = with_part(&input, "xl/sharedStrings.xml",
        r#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><mc:AlternateContent><mc:Choice Requires="x14"><si><t>ambiguous</t></si></mc:Choice></mc:AlternateContent></sst>"#);
    let (wb, result) = batch(&unstable_sst, "Sheet1", &writes);
    assert_matches!(result, Err(Error::Core(ooxml_core::Error::UnresolvedSibling(_))));
    assert_eq!(wb.to_bytes().unwrap(), unstable_sst);
}

#[test]
fn batch_calc_on_load_excludes_formulas_written_by_the_batch() {
    let input = synthetic("o1_shared.xlsx");
    let mut writes = vec![("A1", CellWrite::RemoveFormula), ("D3", CellWrite::Formula(r#"INDIRECT("A1")"#)), ("C5", CellWrite::String("s")), ("F9", CellWrite::String("s"))];
    let (wb, result) = batch(&input, "Sheet2", &writes);
    result.unwrap();
    let out = wb.to_bytes().unwrap();
    let full_calc = |bytes: &[u8]| String::from_utf8(part(bytes, "xl/workbook.xml")).unwrap().contains("fullCalcOnLoad=\"1\"");
    assert!(!full_calc(&out));
    assert!(full_calc(&sequential(&input, "Sheet2", &writes)));
    writes.reverse();
    assert_eq!(out, sequential(&input, "Sheet2", &writes));
}

#[test]
fn batch_iterative_guard_excludes_formulas_written_by_the_batch() {
    let input = synthetic("o1_iterate.xlsx");
    let writes = vec![("J20", CellWrite::Formula(r#"INDIRECT("A1")"#)), ("K20", CellWrite::Number(1.0))];
    let (wb, result) = batch(&input, "Sheet1", &writes);
    result.unwrap();
    let mut single = Workbook::from_bytes(input.clone()).unwrap();
    let mut ws = single.worksheet_by_name("Sheet1").unwrap();
    ws.write_formula(19, 9, r#"INDIRECT("A1")"#).unwrap();
    assert_matches!(ws.write_number(19, 10, 1.0), Err(Error::IterativeCalculation));
    let mut reversed = writes;
    reversed.reverse();
    assert_eq!(wb.to_bytes().unwrap(), sequential(&input, "Sheet1", &reversed));
}

#[test]
fn batch_iterative_guard_excludes_existing_formulas_it_replaces() {
    let input = book(
        &[("Sheet1", r#"<row r="1"><c r="A1"><v>1</v></c><c r="B1"><f>A1</f><v>1</v></c></row>"#)],
        "",
        r#"<calcPr iterate="1"/>"#,
    );
    let mut writes = vec![("A1", CellWrite::Number(2.0)), ("B1", CellWrite::Formula("1"))];
    let (wb, result) = batch(&input, "Sheet1", &writes);
    result.unwrap();
    let mut single = Workbook::from_bytes(input.clone()).unwrap();
    assert_matches!(single.worksheet().unwrap().write_number(0, 0, 2.0), Err(Error::IterativeCalculation));
    assert_eq!(single.to_bytes().unwrap(), input);
    writes.reverse();
    assert_eq!(wb.to_bytes().unwrap(), sequential(&input, "Sheet1", &writes));
}

/// Two writes into the same new row create one row with both cells.
#[test]
fn writes_into_one_new_row_share_it() {
    let input = synthetic("o1_no_calcpr.xlsx");
    let (wb, r) = batch(&input, "Sheet1", &vec![("C20", CellWrite::Number(2.0)), ("B20", CellWrite::Number(1.0)), ("A21", CellWrite::String("x"))]);
    r.unwrap();
    let x = xml_of(&wb.to_bytes().unwrap(), "Sheet1");
    assert_eq!(x.matches(r#"<row r="20""#).count(), 1, "{x}");
    assert!(x.contains(r#"<row r="20"><c r="B20"><v>1</v></c><c r="C20"><v>2</v></c></row><row r="21"><c r="A21" t="inlineStr"><is><t>x</t></is></c></row>"#), "{x}");
}

/// Every refusal of any cell happens before anything changes.
#[test]
fn a_refusal_of_any_cell_changes_nothing() {
    let input = synthetic("o1_precise.xlsx");
    let merged = with_part(
        &input,
        "xl/worksheets/sheet1.xml",
        &xml_of(&input, "Sheet1").replace("</sheetData>", r#"</sheetData><mergeCells count="1"><mergeCell ref="G1:H2"/></mergeCells>"#),
    );
    let cases: Vec<(Vec<u8>, Batch<'_>, &str)> = vec![
        (merged, vec![("A1", CellWrite::Number(1.0)), ("H2", CellWrite::Number(1.0))], "MergedCell"),
        (input.clone(), vec![("A1", CellWrite::Number(1.0)), ("A1", CellWrite::Number(2.0))], "DuplicateCellWrite"),
        (input.clone(), vec![("A1", CellWrite::Number(1.0)), ("B9", CellWrite::Formula("SUM(("))], "InvalidFormula"),
        (input.clone(), vec![("A1", CellWrite::Number(1.0)), ("B9", CellWrite::Formula("XLOOKUP(A1,A2:A3,B2:B3)"))], "MissingFunctionPrefix"),
        (input.clone(), vec![("A1", CellWrite::Number(1.0)), ("B9", CellWrite::Number(f64::NAN))], "InvalidNumber"),
    ];
    for (bytes, writes, what) in cases {
        let (mut wb, r) = batch(&bytes, "Sheet1", &writes);
        let e = r.expect_err(what);
        assert!(format!("{e:?}").starts_with(what), "{what}: {e:?}");
        assert_l1_except(&bytes, &wb.to_bytes().unwrap(), &[]);
        // the workbook stays usable
        wb.worksheet().unwrap().cell_mut(0, 0).unwrap().set_number(1.0).unwrap();
    }
}

/// ADR-0017 §2.1 regression cases: a `vm` cell and a shared-formula group
/// touched twice are refused in step 1 with every byte unchanged; a `cm`
/// cell blocks every edit at open (ADR-0003 `UnknownCellMetadata`).
#[test]
fn rich_value_cells_and_shared_groups_are_refused_in_step_1() {
    let input = synthetic("o1_precise.xlsx");
    let vm = with_part(&input, "xl/worksheets/sheet1.xml", &xml_of(&input, "Sheet1").replace(r#"<c r="C1""#, r#"<c r="C1" vm="1""#));
    let (wb, r) = batch(&vm, "Sheet1", &vec![("A5", CellWrite::Number(1.0)), ("C1", CellWrite::Number(2.0))]);
    assert_matches!(r, Err(Error::RichValueCell { .. }));
    assert_l1_except(&vm, &wb.to_bytes().unwrap(), &[]);
    // o1_shared: Sheet2!C2:E3 is one shared group (master D2)
    let shared = synthetic("o1_shared.xlsx");
    let (wb, r) = batch(&shared, "Sheet2", &vec![("D2", CellWrite::Number(1.0)), ("E3", CellWrite::Number(2.0))]);
    assert_matches!(r, Err(Error::SharedFormulaMaster { .. }));
    assert_l1_except(&shared, &wb.to_bytes().unwrap(), &[]);
    let cm = synthetic("o1_unknown_cm.xlsx");
    let mut wb = Workbook::from_bytes(cm.clone()).unwrap();
    let r = wb.worksheet().unwrap().write_cells(&[(at("A1"), CellWrite::Number(1.0)), (at("A2"), CellWrite::Number(1.0))]);
    assert_matches!(r, Err(Error::UnknownCellMetadata { .. }));
    assert_l1_except(&cm, &wb.to_bytes().unwrap(), &[]);
}

// ------------------------------------------------------------ future functions (ADR-0017 Q7)

#[test]
fn set_formula_takes_the_file_form_of_future_functions() {
    let input = synthetic("o1_no_calcpr.xlsx");
    let mut wb = Workbook::from_bytes(input.clone()).unwrap();
    {
        let mut ws = wb.worksheet().unwrap();
        let e = ws.cell_mut(5, 0).unwrap().set_formula("XLOOKUP(A1,A2:A3,B2:B3)");
        assert_matches!(e, Err(Error::MissingFunctionPrefix { ref name, prefix: "_xlfn." }) if name == "XLOOKUP");
        let e = ws.cell_mut(5, 0).unwrap().set_formula("_xlfn.SORT(A1:A3)");
        assert_matches!(e, Err(Error::MissingFunctionPrefix { ref name, prefix: "_xlfn._xlws." }) if name == "SORT");
        for (name, typed) in [("FILTER", "_xlws.FILTER(A1:A3,B1:B3)"), ("SORT", "_XLWS.SORT(A1:A3)")] {
            assert_matches!(ws.cell_mut(5, 0).unwrap().set_formula(typed),
                Err(Error::MissingFunctionPrefix { name: actual, prefix: "_xlfn._xlws." }) if actual == name);
            let expected = format!("_xlfn._xlws.{}", typed.split_once('.').unwrap().1);
            assert_eq!(ooxml_xlsx::add_future_prefixes(typed).unwrap(), expected);
        }
        assert_matches!(ws.cell_mut(5, 0).unwrap().set_formula("SUM(A1#)"), Err(Error::DynamicArrayOperator { operator: '#', .. }));
        assert_matches!(ws.cell_mut(5, 0).unwrap().set_formula("@A1:A3"), Err(Error::DynamicArrayOperator { operator: '@', .. }));
    }
    assert_eq!(wb.to_bytes().unwrap(), input);
    let typed = "IFS(A1>1,TEXTJOIN(\",\",TRUE,A1:A3),TRUE,\"XLOOKUP(\")";
    let file = ooxml_xlsx::add_future_prefixes(typed).unwrap();
    assert_eq!(file, "_xlfn.IFS(A1>1,_xlfn.TEXTJOIN(\",\",TRUE,A1:A3),TRUE,\"XLOOKUP(\")");
    wb.worksheet().unwrap().cell_mut(5, 0).unwrap().set_formula(&file).unwrap();
    assert!(xml_of(&wb.to_bytes().unwrap(), "Sheet1").contains("<f>_xlfn.IFS(A1&gt;1,_xlfn.TEXTJOIN"));
}

// ------------------------------------------------------------ fault injection

/// A failure or a panic before each in-place step of a multi-cell plan,
/// including after its first cell is written, poisons the workbook.
#[cfg(feature = "fault-injection")]
#[test]
fn a_partly_applied_batch_poisons_the_workbook() {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    use ooxml_xlsx::ApplyStep;
    let input = synthetic("o1_shared.xlsx");
    let writes = [(at("D2"), CellWrite::Number(1.0)), (at("A9"), CellWrite::Formula("A2+1")), (at("B9"), CellWrite::Number(3.0))];
    for step in [ApplyStep::RemoveCalcChain, ApplyStep::CalcPr, ApplyStep::Strip, ApplyStep::HandOff, ApplyStep::Registries, ApplyStep::Target, ApplyStep::LaterTarget, ApplyStep::Index] {
        let mut wb = Workbook::from_bytes(input.clone()).unwrap();
        wb.inject_apply_failure(step);
        let r = wb.worksheet_by_name("Sheet2").unwrap().write_cells(&writes);
        assert_matches!(r, Err(Error::Poisoned(_)), "{step:?}");
        assert_matches!(wb.to_bytes(), Err(Error::Poisoned(_)), "{step:?}");
        let mut wb = Workbook::from_bytes(input.clone()).unwrap();
        wb.inject_apply_panic(step);
        let r = catch_unwind(AssertUnwindSafe(|| wb.worksheet_by_name("Sheet2").unwrap().write_cells(&writes)));
        assert!(r.is_err(), "{step:?}: the injected panic fired");
        assert_matches!(wb.to_bytes(), Err(Error::Poisoned(_)), "{step:?}");
    }
}
