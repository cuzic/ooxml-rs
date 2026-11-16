//! O2 (ADR-0003 §7.1): a perturbation oracle. Random workbooks in an
//! evaluable subset (cross-sheet and 3-D references, whole columns, names
//! global/local/sheet-qualified, SUMIF with mismatched shapes, INDEX, IF;
//! sheet names in random case, with and without quotes, `''` escapes; random
//! `$` and spaces) are evaluated by a test-only evaluator. Every formula
//! whose value changes when a cell is edited must be in the computed
//! dependent set (computed ⊇ observed). Over-approximation is allowed.

mod common;

use std::collections::BTreeSet;

use common::o2gen::*;
use ooxml_xlsx::{CellIndex, Workbook};

fn run(seed: u64) -> (usize, usize) {
    let book = generate(seed);
    let bytes = book.to_xlsx();
    let mut wb = Workbook::from_bytes(bytes.clone()).unwrap_or_else(|e| panic!("seed {seed}: {e}"));
    assert!(wb.edit_refusal().is_none(), "seed {seed}");
    let base = book.eval(&book.values);
    let mut r = Rng(seed ^ 0xA5A5_5A5A_DEAD_BEEF | 1);
    let (mut checked, mut observed_total) = (0, 0);
    for _ in 0..6 {
        let s = r.below(book.sheets.len() as u64) as usize;
        let (row, col) = (r.below(ROWS as u64) as u32, r.below(VCOLS as u64) as u32);
        let computed: BTreeSet<(String, CellIndex)> =
            wb.dependents(&book.sheets[s], CellIndex { row, col }).unwrap().into_iter().collect();
        for v in [1000.0, -0.5, 0.0, 7.0] {
            let mut values = book.values.clone();
            values.insert((s, row, col), v);
            let after = book.eval(&values);
            for (i, (fs, frow, fcol, _)) in book.formulas.iter().enumerate() {
                if after[i] != base[i] {
                    observed_total += 1;
                    let key = (book.sheets[*fs].clone(), CellIndex { row: *frow, col: *fcol });
                    assert!(
                        computed.contains(&key),
                        "seed {seed}: editing {}!{}{} to {v} changes {}!{} ({:?} -> {:?}) but it is not a computed dependent.\nformulas: {:#?}\nnames: {:#?}",
                        book.sheets[s],
                        col_letter(col),
                        row + 1,
                        key.0,
                        key.1,
                        base[i],
                        after[i],
                        book.formulas.iter().enumerate().map(|(k, f)| format!("{}!{}{} = {}", book.sheets[f.0], col_letter(f.2), f.1 + 1, book.render(&f.3, k as u64 * 1000 + 7))).collect::<Vec<_>>(),
                        book.names,
                    );
                }
            }
            checked += 1;
        }
    }
    (checked, observed_total)
}

#[test]
fn o2_computed_dependents_cover_every_observed_change() {
    let (mut checked, mut observed) = (0, 0);
    for seed in 0..400 {
        let (c, o) = run(seed);
        checked += c;
        observed += o;
    }
    // The generator must actually exercise the property.
    assert!(observed > 1000, "only {observed} observed changes in {checked} edits");
}
