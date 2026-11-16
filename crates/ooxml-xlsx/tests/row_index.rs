//! The sheet row/cell index (`src/index.rs`): it agrees with a fresh parse
//! after any sequence of writes (new rows before, between and after existing
//! ones, new cells in existing rows), and the bulk iterator returns every
//! cell in document order.

mod common;

use std::assert_matches;

use common::*;
use ooxml_xlsx::{CellIndex, CellValue, Error, Workbook};

fn all(wb: &mut Workbook) -> Vec<(String, CellValue)> {
    let ws = wb.worksheet().unwrap();
    ws.cells().map(|c| c.unwrap()).map(|c| (c.reference().to_owned(), c.value().unwrap())).collect()
}

#[test]
fn index_stays_valid_across_writes_and_matches_a_fresh_open() {
    let input = read("poi/61605.xlsx");
    let mut wb = Workbook::from_bytes(input).unwrap();
    let mut seed = 0x1234_5678_9abc_def1u64;
    let mut written: Vec<(CellIndex, f64)> = Vec::new();
    for k in 0..300u32 {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let at = CellIndex { row: (seed % 40) as u32 + 15, col: ((seed >> 20) % 12) as u32 + 6 };
        let r = wb.worksheet().unwrap().cell_mut(at.row, at.col).unwrap().set_number(f64::from(k));
        if r.is_ok() {
            written.retain(|(c, _)| *c != at);
            written.push((at, f64::from(k)));
        }
        // every write so far reads back through the (updated) index
        if k % 50 == 49 {
            let ws = wb.worksheet().unwrap();
            for (c, v) in &written {
                assert_eq!(ws.cell(c.row, c.col).unwrap().unwrap().value().unwrap(), CellValue::Number(*v), "{c}");
            }
        }
    }
    let out = wb.to_bytes().unwrap();
    let mut fresh = Workbook::from_bytes(out).unwrap();
    assert_eq!(all(&mut wb), all(&mut fresh), "the maintained index equals a freshly built one");
    // document order: rows ascending, cells ascending in each row
    let refs: Vec<CellIndex> = all(&mut fresh).iter().map(|(r, _)| CellIndex::parse(r).unwrap()).collect();
    assert!(refs.is_sorted(), "document order");
    assert!(written.len() > 200);
}

#[test]
fn bulk_iterator_matches_per_cell_reads_on_fixtures() {
    for name in ["minimal", "poi/61605.xlsx", "poi/52348.xlsx", "poi/shared_formulas.xlsx", "calamine/renamed_workbook_part.xlsx"] {
        let mut wb = Workbook::from_bytes(read(name)).unwrap();
        let ws = wb.worksheet().unwrap();
        let bulk: Vec<(String, CellValue)> =
            ws.cells().map(|c| c.unwrap()).map(|c| (c.reference().to_owned(), c.value().unwrap())).collect();
        let cells = existing_cells(&read(name));
        assert_eq!(bulk.len(), cells.len(), "{name}");
        for ((r, v), at) in bulk.iter().zip(&cells) {
            assert_eq!(*r, at.to_string(), "{name}");
            assert_eq!(&ws.cell(at.row, at.col).unwrap().unwrap().value().unwrap(), v, "{name} {at}");
        }
    }
}

#[test]
fn bulk_iterator_reports_location_errors() {
    let mut wb = Workbook::from_bytes(read("poi/59746_NoRowNums.xlsx")).unwrap();
    let ws = wb.worksheet().unwrap();
    let v: Vec<_> = ws.cells().collect();
    assert_eq!(v.len(), 1);
    assert_matches!(&v[0], Err(Error::MissingReference { .. }));
}
