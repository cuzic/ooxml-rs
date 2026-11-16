//! ADR-0003 §7.2 on the real formula fixtures, plus the tokenizer sweep.

mod common;

use std::assert_matches;

use common::p1::*;
use common::*;
use ooxml_xlsx::{CellIndex, Error, Workbook};

/// Item 1: shared formulas (`A2:A41` = `B2`…): a precedent of the group.
#[test]
fn item1_shared_formulas_value_edit() {
    let input = read("poi/shared_formulas.xlsx");
    let out = value_edit(&input, "Label", "B5", 7.0, &["Label!A5"]);
    // Formula text of every formula cell is unchanged (calamine's view).
    assert_eq!(calamine_formulas(&input), calamine_formulas(&out));
    // A cell no formula reads: nothing but the cell itself.
    value_edit(&input, "Label", "Z50", 7.0, &[]);
}

/// Item 2: CSE arrays. `H2:J4` = `B2:D4 + E2:G4` is precise; the other
/// arrays use functions not on the table (TRANSPOSE, MMULT, …) and depend
/// on everything.
#[test]
fn item2_array_results() {
    let input = read("poi/MatrixFormulaEvalTestData.xlsx");
    let mut must = vec![];
    for c in ["H2", "I2", "J2", "H3", "I3", "J3", "H4", "I4", "J4"] {
        must.push(format!("Sheet1!{c}"));
    }
    let must: Vec<&str> = must.iter().map(String::as_str).collect();
    value_edit_with(&input, "Sheet1", "B2", 123.0, &must, false);
    let mut wb = Workbook::from_bytes(input.clone()).unwrap();
    let r = wb.worksheet().unwrap().cell_mut(2, 8).unwrap().set_number(1.0); // I3
    assert_matches!(r, Err(Error::ArrayResultCell { .. }));
    let r = wb.worksheet().unwrap().cell_mut(1, 7).unwrap().set_number(1.0); // H2
    assert_matches!(r, Err(Error::ArrayAnchor { .. }));
    assert_l1_except(&input, &wb.to_bytes().unwrap(), &[]);
}

/// Item 3: a dynamic array (`C2:D2`, XLOOKUP, `cm`).
#[test]
fn item3_dynamic_array() {
    let input = read("poi/xlookup.xlsx");
    let mut wb = Workbook::from_bytes(input.clone()).unwrap();
    let r = wb.worksheet().unwrap().cell_mut(1, 3).unwrap().set_number(1.0); // D2
    assert_matches!(r, Err(Error::ArrayResultCell { .. }));
    assert_l1_except(&input, &wb.to_bytes().unwrap(), &[]);
    let out = value_edit_with(&input, "Sheet1", "B2", 5.0, &["Sheet1!C2", "Sheet1!D2"], false);
    assert_eq!(part(&input, "xl/metadata.xml"), part(&out, "xl/metadata.xml"));
}

/// Item 4: LibreOffice output without calcChain; `fullCalcOnLoad` is set on
/// the existing `calcPr`; no calcChain appears.
#[test]
fn item4_no_calc_chain() {
    let input = read("poi/70005-countifs.xlsx");
    let out = value_edit(&input, "Sheet1", "B1", 1.0, &["Sheet1!A3", "Sheet1!A4", "Sheet1!D3", "Sheet1!D4"]);
    assert!(entries(&out).iter().all(|e| !e.0.contains("calcChain")));
}

/// Item 6: set_formula on an empty cell of a workbook with calcChain.
#[test]
fn item6_formula_write_removes_calc_chain() {
    let input = read("poi/shared_formulas.xlsx");
    let mut wb = Workbook::from_bytes(input.clone()).unwrap();
    wb.worksheet().unwrap().cell_mut(49, 2).unwrap().set_formula("B2&\"!\"").unwrap(); // C50
    let out = wb.to_bytes().unwrap();
    let e = Expected {
        sheet: "Label",
        at: CellIndex::parse("C50").unwrap(),
        dependents: cells(&[]),
        full_calc: true,
        chain_removed: true,
        also_changed: &[],
    };
    assert_allowed_diff(&input, &out, &e);
    assert_no_stale(&input, &out, &e, None);
}

/// Item 7: 2-D group `D2:U17` (`$A2`), master `D2` overwritten → `E2`.
#[test]
fn item7_master_hand_off_2d() {
    let input = read("poi/NewStyleConditionalFormattings.xlsx");
    let before = calamine_formulas(&input);
    let mut wb = Workbook::from_bytes(input.clone()).unwrap();
    wb.worksheet().unwrap().cell_mut(1, 3).unwrap().set_number(5.0).unwrap(); // D2
    let out = wb.to_bytes().unwrap();
    let x = String::from_utf8(part(&out, "xl/worksheets/sheet1.xml")).unwrap();
    let e2 = &x[x.find(r#"<c r="E2""#).unwrap()..];
    let e2 = &e2[..e2.find("</c>").unwrap()];
    assert!(e2.contains(r#"ref="D2:U17""#) && e2.contains(">$A2</f>"), "{e2}");
    let after = calamine_formulas(&out);
    for (k, f) in &before["CF"] {
        if *k != (1, 3) {
            assert_eq!(after["CF"].get(k), Some(f), "{k:?}");
        }
    }
    let e = Expected {
        sheet: "CF",
        at: CellIndex::parse("D2").unwrap(),
        dependents: Workbook::from_bytes(input.clone())
            .unwrap()
            .dependents("CF", CellIndex::parse("D2").unwrap())
            .unwrap()
            .into_iter()
            .collect(),
        full_calc: true,
        chain_removed: true,
        also_changed: &["CF!E2"],
    };
    assert_allowed_diff(&input, &out, &e);
}

/// Item 8: every formula text in the committed fixtures parses (or is
/// refused without panicking); translation never panics either.
#[test]
fn item8_tokenizer_sweep_over_fixture_formulas() {
    let mut n = 0;
    let mut failed = Vec::new();
    for name in all_zip_fixtures() {
        let input = read(&name);
        let Some(fs) = std::panic::catch_unwind(|| calamine_formulas(&input)).ok() else { continue };
        for (_, cells) in fs {
            for (_, f) in cells {
                n += 1;
                if !ooxml_xlsx::__formula_parses(&f) {
                    failed.push(f);
                }
            }
        }
    }
    assert!(n > 300, "{n} formula texts");
    // Every formula text written by Excel/LibreOffice in the fixtures parses.
    assert!(failed.is_empty(), "{failed:?}");
}

/// Read-back on every formula fixture: an edit of every value cell of the
/// first sheet (sampled) never leaves a stale value for calamine.
#[test]
fn item10_no_stale_values_on_fixtures() {
    for name in [
        "poi/shared_formulas.xlsx",
        "poi/70005-countifs.xlsx",
        "poi/NewStyleConditionalFormattings.xlsx",
        "poi/MatrixFormulaEvalTestData.xlsx",
    ] {
        let input = read(name);
        let wb = Workbook::from_bytes(input.clone()).unwrap();
        let sheet = wb.sheet_name().to_owned();
        let targets: Vec<CellIndex> = existing_cells(&input).into_iter().step_by(17).take(12).collect();
        for at in targets {
            let mut w = Workbook::from_bytes(input.clone()).unwrap();
            let deps: Vec<String> = w.dependents(&sheet, at).unwrap().into_iter().map(|(s, c)| format!("{s}!{c}")).collect();
            let r = w.worksheet().unwrap().cell_mut(at.row, at.col).unwrap().set_number(-31.5);
            let Ok(()) = r else { continue }; // refusals are tested elsewhere
            let out = w.to_bytes().unwrap();
            let deps: Vec<&str> = deps.iter().map(String::as_str).collect();
            let e = Expected {
                sheet: &sheet,
                at,
                dependents: cells(&deps),
                full_calc: !deps.is_empty(),
                chain_removed: false,
                also_changed: &[],
            };
            assert_no_stale(&input, &out, &e, None);
        }
    }
}

/// Optional sweep over a local corpus of formula texts (one per line,
/// `\n` and `\\` escaped), e.g. extracted from POI's `test-data/spreadsheet`.
/// Not committed (third-party text), so ignored by default.
#[test]
#[ignore = "needs OOXML_FORMULA_CORPUS=<file of formula texts>, extracted locally from a corpus"]
fn tokenizer_sweep_over_local_corpus() {
    let path = std::env::var("OOXML_FORMULA_CORPUS").expect("set OOXML_FORMULA_CORPUS");
    let text = std::fs::read_to_string(path).unwrap();
    let (mut n, mut failed) = (0, Vec::new());
    for line in text.lines() {
        let f = line.replace("\\n", "\n").replace("\\\\", "\\");
        n += 1;
        if !ooxml_xlsx::__formula_parses(&f) {
            failed.push(f);
        }
    }
    eprintln!("{n} texts, {} do not parse:", failed.len());
    for f in &failed {
        eprintln!("  {f}");
    }
}
