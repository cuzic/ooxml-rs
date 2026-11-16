//! O3 (ADR-0003 §7.1, R2-M2, R2-m6): the deterministic list of edits whose
//! inputs and outputs LibreOffice recalculates offline. Shared by
//! `examples/o3_inputs.rs` (writes the files for `spikes/xlsx/o3_recalc.py`)
//! and `tests/phase1_o3.rs` (regenerates them and compares).
#![allow(dead_code)]

use ooxml_xlsx::{CellIndex, Workbook};

use super::o2gen::{ROWS, Rng, VCOLS, generate};

pub struct Case {
    pub id: String,
    pub input: Vec<u8>,
    pub output: Vec<u8>,
}

/// FNV-1a 64 (the Python script computes the same), to tie expected data
/// to the exact bytes it was computed from.
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

fn fixture(rel: &str) -> Vec<u8> {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reference/fixtures/xlsx").join(rel);
    std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn edit(input: &[u8], sheet: &str, at: &str, v: f64) -> Option<Vec<u8>> {
    let mut wb = Workbook::from_bytes(input.to_vec()).ok()?;
    let at = CellIndex::parse(at).ok()?;
    wb.worksheet_by_name(sheet).ok()?.cell_mut(at.row, at.col).ok()?.set_number(v).ok()?;
    wb.to_bytes().ok()
}

/// Values of different magnitude per edit (R2-m6).
const VALUES: [f64; 3] = [1000.0, -0.5, 7.0];

pub fn cases() -> Vec<Case> {
    let mut out = Vec::new();
    let mut push = |id: String, input: &[u8], sheet: &str, at: &str| {
        for (k, v) in VALUES.iter().enumerate() {
            if let Some(o) = edit(input, sheet, at, *v) {
                out.push(Case { id: format!("{id}-v{k}"), input: input.to_vec(), output: o });
            }
        }
    };
    let synthetic: &[(&str, &[(&str, &str)])] = &[
        ("o1_precise", &[("Sheet1", "A1"), ("Sheet1", "A2"), ("Sheet1", "A3"), ("Sheet2", "A1"), ("Sheet1", "B2")]),
        ("o1_arrays", &[("Sheet1", "A1"), ("Sheet1", "A9")]),
        ("o1_shared", &[("Sheet2", "A3"), ("Sheet2", "D1")]),
        ("o1_manual", &[("Sheet1", "A1")]),
        ("o1_no_calcpr", &[("Sheet1", "A1")]),
        ("o1_everything_indirect", &[("Sheet1", "A1")]),
        ("o1_everything_offset", &[("Sheet1", "A1")]),
        ("o1_everything_lookup", &[("Sheet1", "A2")]),
        ("o1_everything_sumif_computed", &[("Sheet1", "A1")]),
        ("o1_everything_range_computed", &[("Sheet1", "A2")]),
        ("o1_everything_relative_name", &[("Sheet1", "A1")]),
        ("o1_everything_volatile_ca", &[("Sheet1", "A1")]),
    ];
    for (name, edits) in synthetic {
        let input = fixture(&format!("synthetic/{name}.xlsx"));
        for (sheet, at) in *edits {
            push(format!("{name}-{sheet}!{at}"), &input, sheet, at);
        }
    }
    for (name, sheet, at) in [
        ("poi/shared_formulas.xlsx", "Label", "B5"),
        ("poi/70005-countifs.xlsx", "Sheet1", "B1"),
        ("poi/NewStyleConditionalFormattings.xlsx", "CF", "A2"),
        ("poi/MatrixFormulaEvalTestData.xlsx", "Sheet1", "B2"),
    ] {
        let input = fixture(name);
        let id = name.trim_start_matches("poi/").trim_end_matches(".xlsx");
        push(format!("{id}-{sheet}!{at}"), &input, sheet, at);
    }
    // A fixed-seed sample of O2 workbooks.
    for seed in 0..200u64 {
        let book = generate(seed);
        let input = book.to_xlsx();
        let mut r = Rng(seed ^ 0x0303_0303_0303_0303 | 1);
        let s = r.below(book.sheets.len() as u64) as usize;
        let at = CellIndex { row: r.below(ROWS as u64) as u32, col: r.below(VCOLS as u64) as u32 };
        push(format!("o2_{seed}-{}!{at}", s), &input, &book.sheets[s].clone(), &at.to_string());
    }
    out
}

/// A file name for a case id (no `!`, spaces or quotes).
pub fn file_stem(id: &str) -> String {
    id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect()
}
