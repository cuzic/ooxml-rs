//! `ooxml-xlsx`: open → read → write cells → save → reopen.

use std::collections::BTreeMap;

use ooxml_core::PartStateKind;
use ooxml_xlsx::{CellValue, Workbook};

use crate::fixtures::{XLSX, decode};
use crate::input::Input;
use crate::zipcmp::assert_l1_except;

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
    assert_l1_except(&base, &out, &[&sheet], "xlsx save after edits");
    let mut re = Workbook::from_bytes(out).unwrap_or_else(|e| panic!("edited workbook does not reopen: {e}"));
    for ((row, col), v) in &written {
        assert_eq!(read_cell(&mut re, *row, *col).as_ref(), Some(v), "({row},{col}) after reopen");
    }
}
