//! Regression tests for the ADR-0003 round-3 / Phase1 implementation
//! round-1 review (`docs/adr/0003-opus-review-round3.md`): the reviewer's
//! stale-value probes P1–P7 and P13, and fault injection on more edit kinds.

mod common;

use std::assert_matches;

use common::p1::*;
use common::*;
use ooxml_xlsx::{CellIndex, Error, Workbook};

fn deps(bytes: &[u8], sheet: &str, at: &str) -> Vec<String> {
    let mut wb = Workbook::from_bytes(bytes.to_vec()).unwrap();
    wb.dependents(sheet, CellIndex::parse(at).unwrap()).unwrap().into_iter().map(|(s, c)| format!("{s}!{c}")).collect()
}

/// The output of writing `v` into `at` of the sheet at `index`.
fn write_at(bytes: &[u8], index: usize, at: &str, v: f64) -> Vec<u8> {
    let mut wb = Workbook::from_bytes(bytes.to_vec()).unwrap();
    let at = CellIndex::parse(at).unwrap();
    wb.worksheet_by_index(index).unwrap().cell_mut(at.row, at.col).unwrap().set_number(v).unwrap();
    wb.to_bytes().unwrap()
}

fn sheet_xml_at(bytes: &[u8], index: usize) -> String {
    String::from_utf8(part(bytes, &format!("xl/worksheets/sheet{}.xml", index + 1))).unwrap()
}

/// R3-M1 (b), P1: two masters share `si` 0 on one sheet. LibreOffice reads A2
/// as `B2` (first master); ooxml-xlsx must not claim a precise set.
#[test]
fn p1_two_masters_with_one_si() {
    let b = book(&[("Sheet1", r#"<row r="1"><c r="A1"><f t="shared" ref="A1:A2" si="0">B1</f><v>1</v></c></row><row r="2"><c r="A2"><f t="shared" si="0"/><v>2</v></c></row><row r="5"><c r="A5"><f t="shared" ref="A5:A6" si="0">C5</f><v>5</v></c></row><row r="6"><c r="A6"><f t="shared" si="0"/><v>6</v></c></row>"#)], "", "");
    for at in ["B2", "C2", "B6", "Z99"] {
        let d = deps(&b, "Sheet1", at);
        for c in ["Sheet1!A1", "Sheet1!A2", "Sheet1!A5", "Sheet1!A6"] {
            assert!(d.contains(&c.to_owned()), "{at}: {d:?}");
        }
    }
    let out = write_at(&b, 0, "B2", 9.0);
    assert!(sheet_xml_at(&out, 0).contains(r#"<c r="A2"><f t="shared" si="0"/></c>"#), "A2 lost its cache");
    let mut wb = Workbook::from_bytes(b.clone()).unwrap();
    assert_matches!(wb.worksheet().unwrap().cell_mut(0, 0).unwrap().set_number(1.0), Err(Error::SharedFormulaMaster { .. }));
    assert_l1_except(&b, &wb.to_bytes().unwrap(), &[]);
}

/// R3-M1 (c), P2 and P13: sheet names that fold alike (`Data`/`DATA`;
/// `k` and the Kelvin sign U+212A) are ambiguous: a reference to either
/// depends on everything, and a folded lookup by name is refused.
#[test]
fn p2_p13_sheet_names_that_fold_alike() {
    let b = book(&[("Data", r#"<row r="1"><c r="A1"><v>1</v></c></row>"#), ("DATA", r#"<row r="1"><c r="A1"><v>2</v></c></row>"#), ("S", r#"<row r="1"><c r="A1"><f>DATA!A1</f><v>2</v></c></row>"#)], "", "");
    assert_eq!(deps(&b, "DATA", "A1"), ["S!A1"]);
    assert_eq!(deps(&b, "Data", "A1"), ["S!A1"]);
    assert!(sheet_xml_at(&write_at(&b, 1, "A1", 5.0), 2).contains(r#"<c r="A1"><f>DATA!A1</f></c>"#));
    let mut wb = Workbook::from_bytes(b).unwrap();
    assert_matches!(wb.worksheet_by_name("data"), Err(Error::NoSuchSheet(_)));
    assert!(wb.worksheet_by_name("DATA").is_ok(), "an exact match wins");

    let b = book(&[("k", r#"<row r="1"><c r="A1"><v>1</v></c></row>"#), ("\u{212A}", r#"<row r="1"><c r="A1"><v>2</v></c></row>"#), ("S", r#"<row r="1"><c r="A1"><f>'&#x212A;'!A1</f><v>2</v></c></row>"#)], "", "");
    assert_eq!(deps(&b, "\u{212A}", "A1"), ["S!A1"]);
    assert!(sheet_xml_at(&write_at(&b, 1, "A1", 5.0), 2).contains(r#"<c r="A1"><f>'&#x212A;'!A1</f></c>"#));
}

/// R3-M1 (c), P3: two global names `Rate` and `RATE`.
#[test]
fn p3_defined_names_that_fold_alike() {
    let b = book(&[("S", r#"<row r="1"><c r="A1"><v>1</v></c><c r="B1"><v>2</v></c><c r="C1"><f>Rate</f><v>1</v></c></row>"#)], r#"<definedName name="Rate">S!$A$1</definedName><definedName name="RATE">S!$B$1</definedName>"#, "");
    assert_eq!(deps(&b, "S", "B1"), ["S!C1"]);
    assert_eq!(deps(&b, "S", "Z9"), ["S!C1"]);
}

/// R3-M1 (a), P4: a local name whose text has an unqualified reference,
/// used from another sheet: Excel reads it on the using sheet.
#[test]
fn p4_unqualified_reference_inside_a_local_name() {
    let b = book(&[("S1", r#"<row r="1"><c r="A1"><v>1</v></c><c r="C1"><f>S2!Loc</f><v>7</v></c></row>"#), ("S2", r#"<row r="1"><c r="A1"><v>7</v></c></row>"#)], r#"<definedName name="Loc" localSheetId="1">$A$1</definedName>"#, "");
    assert_eq!(deps(&b, "S1", "A1"), ["S1!C1"]);
    assert_eq!(deps(&b, "S2", "A1"), ["S1!C1"]);
    assert!(sheet_xml_at(&write_at(&b, 0, "A1", 5.0), 0).contains(r#"<c r="C1"><f>S2!Loc</f></c>"#));
}

/// R3-m1, P5: a sheet-qualified name/reference collision in a shared master
/// cannot be handed off.
#[test]
fn p5_qualified_collision_hand_off_fails() {
    let b = book(&[("Sheet1", r#"<row r="1"><c r="B1"><f t="shared" ref="B1:B2" si="0">Sheet1!TAX2024+A1</f><v>1</v></c></row><row r="2"><c r="B2"><f t="shared" si="0"/><v>2</v></c></row>"#)], r#"<definedName name="TAX2024" localSheetId="0">Sheet1!$Z$1</definedName>"#, "");
    let mut wb = Workbook::from_bytes(b.clone()).unwrap();
    assert_matches!(wb.worksheet().unwrap().cell_mut(0, 1).unwrap().set_number(5.0), Err(Error::SharedFormulaMaster { .. }));
    assert_l1_except(&b, &wb.to_bytes().unwrap(), &[]);
}

/// R3-m2, P6: `set_formula("==A1")` is refused (one `=` is dropped, a
/// second one is not formula text).
#[test]
fn p6_double_equals_is_refused() {
    let b = book(&[("S", r#"<row r="1"><c r="A1"><v>1</v></c><c r="B1"><f>A1</f><v>1</v></c></row>"#)], "", "");
    let mut wb = Workbook::from_bytes(b.clone()).unwrap();
    assert_matches!(wb.worksheet().unwrap().cell_mut(0, 2).unwrap().set_formula("==A1"), Err(Error::InvalidFormula { .. }));
    assert_l1_except(&b, &wb.to_bytes().unwrap(), &[]);
}

/// R3-m6, P7: `calcPr` would have to be inserted next to an
/// `mc:AlternateContent` child of `workbook`: refused, nothing changed.
#[test]
fn p7_calc_pr_next_to_alternate_content() {
    let b = book(&[("S", r#"<row r="1"><c r="A1"><v>1</v></c><c r="B1"><f>A1</f><v>1</v></c></row>"#)], "", r#"<mc:AlternateContent><mc:Choice Requires="x15"/><mc:Fallback/></mc:AlternateContent>"#);
    let mut wb = Workbook::from_bytes(b.clone()).unwrap();
    assert_matches!(wb.worksheet().unwrap().cell_mut(0, 0).unwrap().set_number(2.0), Err(Error::Core(ooxml_core::Error::UnresolvedSibling(_))));
    assert_l1_except(&b, &wb.to_bytes().unwrap(), &[]);
}

/// R3-m6: fault injection on more edit kinds: a shared-master hand-off, a
/// workbook without calcChain, and `KeepStaleCaches`.
#[cfg(feature = "fault-injection")]
#[test]
fn fault_injection_on_more_edit_kinds() {
    use ooxml_xlsx::{ApplyStep, CalcPolicy};
    type Edit = fn(&mut Workbook) -> ooxml_xlsx::Result<()>;
    let cases: [(&str, bool, Edit); 3] = [
        ("o1_shared.xlsx", false, |wb| wb.worksheet_by_name("Sheet2")?.cell_mut(1, 3)?.set_number(9.0)),
        ("o1_no_calcpr.xlsx", false, |wb| wb.worksheet()?.cell_mut(0, 0)?.set_number(9.0)),
        ("o1_precise.xlsx", true, |wb| wb.worksheet()?.cell_mut(0, 0)?.set_number(9.0)),
    ];
    for (name, keep, edit) in cases {
        let input = synthetic(name);
        for step in [ApplyStep::RemoveCalcChain, ApplyStep::CalcPr, ApplyStep::Strip, ApplyStep::HandOff, ApplyStep::Target, ApplyStep::Index] {
            let mut wb = Workbook::from_bytes(input.clone()).unwrap();
            if keep {
                wb.set_calc_policy(CalcPolicy::KeepStaleCaches);
            }
            wb.inject_apply_failure(step);
            assert_matches!(edit(&mut wb), Err(Error::Poisoned(_)), "{name} {step:?}");
            assert_matches!(wb.to_bytes(), Err(Error::Poisoned(_)));
            assert_matches!(edit(&mut wb), Err(Error::Poisoned(_)));
        }
    }
}

/// Review R4-m6: a **panic** inside apply (a bug by construction) leaves the
/// workbook poisoned: it was poisoned before apply and is cleared only on
/// success, so after `catch_unwind` every save and edit is refused.
#[cfg(feature = "fault-injection")]
#[test]
fn a_panic_during_apply_poisons_the_workbook() {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    use ooxml_xlsx::ApplyStep;
    let input = synthetic("o1_shared.xlsx");
    for step in [ApplyStep::RemoveCalcChain, ApplyStep::CalcPr, ApplyStep::Strip, ApplyStep::HandOff, ApplyStep::Target, ApplyStep::Index] {
        let mut wb = Workbook::from_bytes(input.clone()).unwrap();
        wb.inject_apply_panic(step);
        let r = catch_unwind(AssertUnwindSafe(|| wb.worksheet_by_name("Sheet2").unwrap().cell_mut(1, 3).unwrap().set_number(9.0)));
        assert!(r.is_err(), "{step:?}: the injected panic fired");
        assert_matches!(wb.to_bytes(), Err(Error::Poisoned(_)), "{step:?}");
        assert_matches!(wb.worksheet().unwrap().cell_mut(0, 0).unwrap().set_number(1.0), Err(Error::Poisoned(_)), "{step:?}");
    }
}
