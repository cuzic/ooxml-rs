//! Formula writes and removals, shared-formula hand-off and calcChain
//! removal (ADR-0003 §4), validate-before-mutate, and failure atomicity
//! (§3.1, §7.3).

mod common;

use std::assert_matches;

use calamine::Data;
use common::p1::*;
use common::*;
use ooxml_xlsx::{CellIndex, Error, Workbook};

fn edit<T>(input: &[u8], sheet: &str, at: &str, f: impl FnOnce(ooxml_xlsx::CellMut<'_, '_>) -> ooxml_xlsx::Result<T>) -> (Workbook, ooxml_xlsx::Result<T>) {
    let mut wb = Workbook::from_bytes(input.to_vec()).unwrap();
    let at = CellIndex::parse(at).unwrap();
    let r = {
        let mut ws = wb.worksheet_by_name(sheet).unwrap();
        f(ws.cell_mut(at.row, at.col).unwrap())
    };
    (wb, r)
}

fn xml_of(bytes: &[u8], sheet: &str) -> String {
    String::from_utf8(part(bytes, &sheet_parts(bytes)[sheet])).unwrap()
}

#[test]
fn set_formula_on_an_empty_cell_removes_calc_chain() {
    let input = synthetic("o1_precise.xlsx");
    let (mut wb, r) = edit(&input, "Sheet1", "F5", |c| c.set_formula("=A1+1"));
    r.unwrap();
    // The new formula is in the graph at once.
    let deps = wb.dependents("Sheet1", CellIndex::parse("A1").unwrap()).unwrap();
    assert!(deps.contains(&("Sheet1".into(), CellIndex::parse("F5").unwrap())));
    let out = wb.to_bytes().unwrap();
    let e = Expected {
        sheet: "Sheet1",
        at: CellIndex::parse("F5").unwrap(),
        dependents: cells(&[]),
        full_calc: true,
        chain_removed: true,
        also_changed: &[],
    };
    assert_allowed_diff(&input, &out, &e);
    assert!(xml_of(&out, "Sheet1").contains(r#"<row r="5"><c r="F5"><f>A1+1</f></c></row>"#), "{}", xml_of(&out, "Sheet1"));
    assert_no_stale(&input, &out, &e, None);
    // Re-open: the census sees the new formula.
    let mut again = Workbook::from_bytes(out).unwrap();
    assert_eq!(again.worksheet().unwrap().cell_a1("F5").unwrap().unwrap().formula().as_deref(), Some("A1+1"));
}

#[test]
fn rewriting_a_formula_keeps_calc_chain() {
    let input = synthetic("o1_precise.xlsx");
    let (mut wb, r) = edit(&input, "Sheet1", "C1", |c| c.set_formula("A2*5"));
    r.unwrap();
    let deps = wb.dependents("Sheet1", CellIndex::parse("A2").unwrap()).unwrap();
    assert!(deps.contains(&("Sheet1".into(), CellIndex::parse("C1").unwrap())), "graph updated: {deps:?}");
    let out = wb.to_bytes().unwrap();
    let e = Expected {
        sheet: "Sheet1",
        at: CellIndex::parse("C1").unwrap(),
        dependents: cells(&[]),
        full_calc: false,
        chain_removed: false,
        also_changed: &[],
    };
    assert_allowed_diff(&input, &out, &e);
    assert!(xml_of(&out, "Sheet1").contains(r#"<c r="C1" s="0"><f>A2*5</f></c>"#));
}

#[test]
fn value_over_formula_and_formula_removal() {
    let input = synthetic("o1_precise.xlsx");
    // B1 = A1*3 is read by Sheet3!J1 (SUMIF sum_range resized over B1:B3).
    let (wb, r) = edit(&input, "Sheet1", "B1", |c| c.set_number(99.0));
    r.unwrap();
    let out = wb.to_bytes().unwrap();
    let e = Expected {
        sheet: "Sheet1",
        at: CellIndex::parse("B1").unwrap(),
        dependents: cells(&["Sheet3!J1"]),
        full_calc: true,
        chain_removed: true,
        also_changed: &[],
    };
    assert_allowed_diff(&input, &out, &e);
    assert_no_stale(&input, &out, &e, Some(&Data::Float(99.0)));
    assert!(xml_of(&out, "Sheet1").contains(r#"<c r="B1" s="0"><v>99</v></c>"#));
    // remove_formula: Sheet2!B1 = Sheet1!A1*2, read by Sheet2!C1 -> Sheet3!A1.
    let (wb, r) = edit(&input, "Sheet2", "B1", |c| c.remove_formula());
    r.unwrap();
    let out = wb.to_bytes().unwrap();
    let e = Expected {
        sheet: "Sheet2",
        at: CellIndex::parse("B1").unwrap(),
        dependents: cells(&["Sheet2!C1", "Sheet3!A1"]),
        full_calc: true,
        chain_removed: true,
        also_changed: &[],
    };
    assert_allowed_diff(&input, &out, &e);
    assert!(xml_of(&out, "Sheet2").contains(r#"<c r="B1" s="0"></c>"#), "{}", xml_of(&out, "Sheet2"));
    // remove_formula on a value cell: nothing at all.
    let (wb, r) = edit(&input, "Sheet1", "A1", |c| c.remove_formula());
    r.unwrap();
    assert_l1_except(&input, &wb.to_bytes().unwrap(), &[]);
}

#[test]
fn invalid_formulas_change_nothing() {
    let input = synthetic("o1_precise.xlsx");
    for bad in ["SUM(", "1+", "\"open", "A1\u{1}", "{1,2"] {
        let (wb, r) = edit(&input, "Sheet1", "F5", |c| c.set_formula(bad));
        assert_matches!(r, Err(Error::InvalidFormula { .. }), "{bad:?}");
        assert_l1_except(&input, &wb.to_bytes().unwrap(), &[]);
    }
}

#[test]
fn shared_master_hand_off_1d_and_not_top_left() {
    let input = synthetic("o1_shared.xlsx");
    let before = calamine_formulas(&input);
    // 1-D: B2 (master of B2:B5) overwritten; B3 becomes the master.
    let (wb, r) = edit(&input, "Sheet2", "B2", |c| c.set_number(100.0));
    r.unwrap();
    let out = wb.to_bytes().unwrap();
    let e = Expected {
        sheet: "Sheet2",
        at: CellIndex::parse("B2").unwrap(),
        dependents: cells(&["Sheet2!H9"]),
        full_calc: true,
        chain_removed: true,
        also_changed: &["Sheet2!B3"],
    };
    assert_allowed_diff(&input, &out, &e);
    assert_no_stale(&input, &out, &e, Some(&Data::Float(100.0)));
    let x = xml_of(&out, "Sheet2");
    assert!(x.contains(r#"<c r="B3" s="0"><f t="shared" si="0" ref="B3:B5">A3*2</f><v>4</v></c>"#), "{x}");
    assert!(x.contains(r#"<c r="B4" s="0"><f t="shared" si="0"/><v>6</v></c>"#));
    let after = calamine_formulas(&out);
    for c in ["B3", "B4", "B5", "C2", "D2", "E2", "C3", "D3", "E3"] {
        let k = { let i = CellIndex::parse(c).unwrap(); (i.row, i.col) };
        assert_eq!(after["Sheet2"].get(&k), before["Sheet2"].get(&k), "{c}: meaning kept (calamine)");
    }
    // 2-D, master D2 not top-left of C2:E3: C2 (first in row-major order) takes over.
    let (wb, r) = edit(&input, "Sheet2", "D2", |c| c.remove_formula());
    r.unwrap();
    let out = wb.to_bytes().unwrap();
    let x = xml_of(&out, "Sheet2");
    assert!(x.contains(r#"<c r="C2" s="0"><f t="shared" si="1" ref="C2:E3">$A2+C$1</f><v>11</v></c>"#), "{x}");
    let after = calamine_formulas(&out);
    // calamine expands members relative to the top-left of `ref`, not to
    // the master, so it misreads the input here (E2 as `$A2+F$1`); after the
    // hand-off the master is the top-left cell and calamine agrees with the
    // hand-written expectations below.
    // Hand-written expectations for the members (independent of calamine).
    let expect = [("C2", "$A2+C$1"), ("E2", "$A2+E$1"), ("C3", "$A3+C$1"), ("D3", "$A3+D$1"), ("E3", "$A3+E$1")];
    for (c, f) in expect {
        let k = { let i = CellIndex::parse(c).unwrap(); (i.row, i.col) };
        assert_eq!(after["Sheet2"][&k], f, "{c}");
    }
}

#[test]
fn shared_master_hand_off_failure_changes_nothing() {
    let input = synthetic("o1_shared_oob.xlsx");
    let (wb, r) = edit(&input, "Sheet2", "G3", |c| c.set_number(1.0));
    assert_matches!(r, Err(Error::SharedFormulaMaster { .. }));
    assert_l1_except(&input, &wb.to_bytes().unwrap(), &[]);
}

/// Coordinator note (Phase0 review): a refused write must never have
/// mutated anything. An `mc:AlternateContent` inside the target cell, or
/// beside the row's cells, is refused before any change.
#[test]
fn unexpected_children_are_refused_before_mutation() {
    let input = synthetic("o1_no_calcpr.xlsx");
    let ns = r#"xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006""#;
    let ac = r#"<mc:AlternateContent><mc:Choice Requires="x"><v>1</v></mc:Choice></mc:AlternateContent>"#;
    for sheet in [
        format!(r#"<worksheet {ns}><sheetData><row r="1"><c r="A1"><v>1</v>{ac}</c><c r="B1"><f>A1*2</f><v>2</v></c></row></sheetData></worksheet>"#),
        format!(r#"<worksheet {ns}><sheetData><row r="1"><c r="B1"><f>A1*2</f><v>2</v></c>{ac}</row></sheetData></worksheet>"#),
    ] {
        let wb_in = with_part(&input, "xl/worksheets/sheet1.xml", &sheet);
        for target in ["A1", "C1"] {
            let (wb, r) = edit(&wb_in, "Sheet1", target, |c| c.set_number(5.0));
            if sheet.contains(r#"<v>1</v><mc:"#) && target == "C1" {
                // AC only inside A1; C1 is a new cell in a clean row: fine.
                r.unwrap();
                continue;
            }
            assert_matches!(r, Err(Error::Core(ooxml_core::Error::UnresolvedSibling(_))), "{target}");
            assert_l1_except(&wb_in, &wb.to_bytes().unwrap(), &[]);
        }
    }
}

#[test]
fn keep_stale_caches_policy() {
    let input = synthetic("o1_precise.xlsx");
    let mut wb = Workbook::from_bytes(input.clone()).unwrap();
    wb.set_calc_policy(ooxml_xlsx::CalcPolicy::KeepStaleCaches);
    wb.worksheet().unwrap().cell_mut(0, 0).unwrap().set_number(50.0).unwrap();
    let out = wb.to_bytes().unwrap();
    // Only Sheet1 (target) and workbook.xml (fullCalcOnLoad) change.
    let parts = sheet_parts(&input);
    assert_l1_except(&input, &out, &[&parts["Sheet1"], "xl/workbook.xml"]);
    assert!(xml_of(&out, "Sheet1").contains(r#"<c r="B1" s="0"><f>A1*3</f><v>15</v></c>"#));
}

#[cfg(feature = "fault-injection")]
#[test]
fn apply_failures_poison_the_workbook() {
    use ooxml_xlsx::ApplyStep;
    let input = synthetic("o1_precise.xlsx");
    for step in [ApplyStep::RemoveCalcChain, ApplyStep::CalcPr, ApplyStep::Strip, ApplyStep::HandOff, ApplyStep::Target, ApplyStep::Index] {
        let mut wb = Workbook::from_bytes(input.clone()).unwrap();
        wb.inject_apply_failure(step);
        let r = wb.worksheet_by_name("Sheet2").unwrap().cell_mut(0, 1).unwrap().set_number(1.0);
        assert_matches!(r, Err(Error::Poisoned(_)), "{step:?}");
        assert_matches!(wb.to_bytes(), Err(Error::Poisoned(_)));
        assert_matches!(wb.save(std::env::temp_dir().join("never-written.xlsx")), Err(Error::Poisoned(_)));
        assert_matches!(wb.edit_refusal(), Some(Error::Poisoned(_)));
        assert_matches!(wb.worksheet().unwrap().cell_mut(5, 5).unwrap().set_number(1.0), Err(Error::Poisoned(_)));
    }
    assert!(!std::env::temp_dir().join("never-written.xlsx").exists());
}

/// Phase0 review round 3, R3-2: an existing cell outside `dimension` still
/// grows it when written (value, formula and removal alike).
#[test]
fn dimension_grows_when_the_existing_cell_is_outside_it() {
    let input = synthetic("o1_no_calcpr.xlsx");
    let sheet = r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><dimension ref="A1:B1"/><sheetData><row r="1"><c r="A1"><v>1</v></c><c r="B1"><f>A1*2</f><v>2</v></c></row><row r="5"><c r="D5"><v>9</v></c></row></sheetData></worksheet>"#;
    let wb_in = with_part(&input, "xl/worksheets/sheet1.xml", sheet);
    for f in [
        (|c: ooxml_xlsx::CellMut<'_, '_>| c.set_number(3.0)) as fn(ooxml_xlsx::CellMut<'_, '_>) -> ooxml_xlsx::Result<()>,
        |c| c.set_string_inline("x"),
        |c| c.set_formula("A1+1"),
    ] {
        let (wb, r) = edit(&wb_in, "Sheet1", "D5", f);
        r.unwrap();
        let x = xml_of(&wb.to_bytes().unwrap(), "Sheet1");
        assert!(x.contains(r#"<dimension ref="A1:D5"/>"#), "{x}");
    }
}

/// Phase0 review round 3, R3-3: comments, PIs and unknown children of a
/// cell keep their position relative to the value: an existing `v`, a
/// simple `is` or an `f` of the needed kind is reused in place; duplicates
/// are removed; a new kind goes to its schema slot (after unknown children
/// that precede it, never reordering them among themselves).
#[test]
fn cell_children_keep_their_relative_order() {
    let input = synthetic("o1_no_calcpr.xlsx");
    let ns = r#"xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:u="urn:unknown""#;
    type Write = fn(ooxml_xlsx::CellMut<'_, '_>) -> ooxml_xlsx::Result<()>;
    let cases: [(&str, Write, &str); 6] = [
        (r#"<c r="C1"><!--a--><v>1</v><?pi x?><u:x/><!--b--></c>"#, |c| c.set_number(5.0), r#"<c r="C1"><!--a--><v>5</v><?pi x?><u:x/><!--b--></c>"#),
        (r#"<c r="C1"><!--a--><v>1</v><!--m--><v>2</v><!--b--></c>"#, |c| c.set_number(5.0), r#"<c r="C1"><!--a--><v>5</v><!--m--><!--b--></c>"#),
        (r#"<c r="C1" t="inlineStr"><!--a--><is><t>x</t></is><!--b--></c>"#, |c| c.set_string_inline("y"), r#"<c r="C1" t="inlineStr"><!--a--><is><t>y</t></is><!--b--></c>"#),
        (r#"<c r="C1"><!--a--><f t="shared" si="3" ref="C1:C2">A1</f><v>1</v><!--b--></c>"#, |c| c.set_formula("A1*2"), r#"<c r="C1"><!--a--><f>A1*2</f><!--b--></c>"#),
        (r#"<c r="C1"><!--a--><f>A1</f><!--m--><v>1</v><!--b--></c>"#, |c| c.set_number(5.0), r#"<c r="C1"><!--a--><!--m--><v>5</v><!--b--></c>"#),
        (r#"<c r="C1"><!--a--><v>1</v><!--b--><u:x/></c>"#, |c| c.set_string_inline("s"), r#"<c r="C1" t="inlineStr"><!--a--><!--b--><u:x/><is><t>s</t></is></c>"#),
    ];
    for (cell, f, want) in cases {
        let sheet = format!(r#"<worksheet {ns}><sheetData><row r="1"><c r="A1"><v>1</v></c>{cell}</row></sheetData></worksheet>"#);
        let wb_in = with_part(&input, "xl/worksheets/sheet1.xml", &sheet);
        let (wb, r) = edit(&wb_in, "Sheet1", "C1", f);
        r.unwrap_or_else(|e| panic!("{cell}: {e}"));
        let x = xml_of(&wb.to_bytes().unwrap(), "Sheet1");
        assert!(x.contains(want), "{cell}\n=> {x}");
    }
}
