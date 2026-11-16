//! `ooxml-xlsx`: open → read → write cells → save → reopen.

use std::collections::BTreeMap;

use ooxml_core::PartStateKind;
use ooxml_xlsx::{CellValue, Workbook};

use crate::fixtures::{XLSX, decode};
use crate::input::Input;
use crate::zipcmp::{assert_l1_except, assert_l1_except_removing};

fn read_cell(wb: &mut Workbook, row: u32, col: u32) -> Option<CellValue> {
    let ws = wb.worksheet().ok()?;
    let c = ws.cell(row, col).ok()??;
    let _ = (c.reference(), c.style_index(), c.data_type(), c.formula(), c.raw().name());
    c.value().ok()
}

fn small_index(inp: &mut Input<'_>) -> (u32, u32) {
    match inp.u8() % 8 {
        // Near the corner where fixtures have data.
        0..=5 => (u32::from(inp.u8() % 24), u32::from(inp.u8() % 12)),
        // Anywhere, including just past the sheet limits.
        6 => (inp.u32() % 1_048_580, inp.u32() % 16_390),
        _ => (1_048_575, 16_383),
    }
}

/// Fuzz entry: a fixture-based input (see [`crate::fixtures`]).
pub fn check_xlsx_input(data: &[u8]) {
    if let Some((zip, ops)) = decode(&XLSX, data) {
        check_xlsx(&zip, ops);
    }
}

/// Checks on a workbook (if it opens):
///
/// - zero-edit save is Layer-1 identical to the input;
/// - reads never panic;
/// - a refused write leaves the package byte-identical (and, before any
///   successful write, the sheet not dirty);
/// - a successful write reads back as written, before and after a save and
///   reopen; every part but the sheet stays Layer-1 identical.
pub fn check_xlsx(zip: &[u8], mut ops: Input<'_>) {
    let Ok(mut wb) = Workbook::from_bytes(zip.to_vec()) else { return };
    let _ = (wb.sheet_name().len(), wb.workbook_part().len(), wb.edit_refusal());
    let sheet = wb.worksheet_part().to_owned();
    let workbook = wb.workbook_part().to_owned();
    let Ok(base) = wb.to_bytes() else { return };
    assert_l1_except(zip, &base, &[], "xlsx zero-edit save");

    for r in 0..6 {
        for c in 0..6 {
            let _ = read_cell(&mut wb, r, c);
        }
    }
    let mut written: BTreeMap<(u32, u32), CellValue> = BTreeMap::new();
    let mut steps = 0;
    while !ops.is_empty() && steps < 16 {
        steps += 1;
        let (row, col) = small_index(&mut ops);
        let kind = ops.u8() % 4;
        if kind == 2 {
            let s = String::from_utf8_lossy(ops.take(8)).into_owned();
            if let Ok(ws) = wb.worksheet() {
                let _ = ws.cell_a1(&s).map(|c| c.map(|c| c.value()));
            }
            continue;
        }
        if kind == 3 {
            let _ = read_cell(&mut wb, row, col);
            continue;
        }
        let before = wb.to_bytes().expect("to_bytes worked before");
        let (result, expect) = {
            let Ok(mut ws) = wb.worksheet() else { return };
            match kind {
                0 => {
                    let v = ops.f64();
                    (ws.cell_mut(row, col).and_then(|m| m.set_number(v)), CellValue::Number(v))
                }
                _ => {
                    let s = ops.string();
                    (ws.cell_mut(row, col).and_then(|m| m.set_string(&s)), CellValue::String(s))
                }
            }
        };
        match result {
            Err(e) => {
                let after = wb.to_bytes().expect("to_bytes after a refused write");
                assert!(after == before, "refused write ({e}) at ({row},{col}) changed the package");
                if written.is_empty() {
                    assert_ne!(
                        wb.package().state(&sheet),
                        Some(PartStateKind::Parsed { dirty: true }),
                        "refused write ({e}) left the sheet dirty"
                    );
                }
            }
            Ok(()) => {
                let got = read_cell(&mut wb, row, col);
                assert_eq!(got.as_ref(), Some(&expect), "write at ({row},{col}) does not read back");
                written.insert((row, col), expect);
            }
        }
    }
    let out = wb.to_bytes().unwrap_or_else(|e| panic!("save after edits failed: {e}"));
    // Phase1 (ADR-0003): a value edit strips the cached values of its
    // dependents on any sheet and sets `calcPr/@fullCalcOnLoad` in the
    // workbook part; when the set of formula cells changes, `calcChain.xml`
    // goes, with its relationship and content-type override.
    let names: Vec<String> = re_names(&base);
    let base_pkg = ooxml_core::Package::from_bytes(base.clone()).expect("the zero-edit save reopens");
    let mut except: Vec<&str> = names
        .iter()
        .map(String::as_str)
        .filter(|n| is_sheet(&base_pkg, n) || *n == "[Content_Types].xml" || n.ends_with(".rels"))
        .collect();
    except.push(&workbook);
    let removable: Vec<&str> = names.iter().map(String::as_str).filter(|n| n.ends_with("calcChain.xml")).collect();
    assert_l1_except_removing(&base, &out, &except, &removable, "xlsx save after edits");
    let mut re = Workbook::from_bytes(out).unwrap_or_else(|e| panic!("edited workbook does not reopen: {e}"));
    for ((row, col), v) in &written {
        assert_eq!(read_cell(&mut re, *row, *col).as_ref(), Some(v), "({row},{col}) after reopen");
    }
}

fn re_names(zip: &[u8]) -> Vec<String> {
    crate::zipcmp::raw_entries(zip).map(|v| v.into_iter().map(|e| e.name).collect()).unwrap_or_default()
}

/// A sheet part (worksheet, macro sheet, dialog sheet, chartsheet), by
/// content type: a value edit may strip dependents' cached values there.
fn is_sheet(pkg: &ooxml_core::Package, name: &str) -> bool {
    pkg.content_type(name).is_some_and(|t| t.contains("sheet+xml") && !t.contains("sharedStrings") && !t.contains(".main+xml"))
}
