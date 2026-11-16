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

const MC: &str = r#"xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006""#;

fn snapshot(wb: &mut Workbook, sheet: &str) -> Vec<(String, Option<String>, String)> {
    let ws = wb.worksheet_by_name(sheet).unwrap();
    ws.cells()
        .map(|c| c.unwrap())
        .map(|c| (c.reference().to_owned(), c.formula(), format!("{:?}", c.value())))
        .collect()
}

/// PR #29 review m1: stripping a dependent's cache does not refuse because
/// the dependent's sheet has `mc:AlternateContent` in `sheetData` or in the
/// dependent's row (as before the index); the target cell still refuses it.
#[test]
fn stripping_ignores_alternate_content_the_target_refuses_it() {
    let ac_sd = format!(
        r#"<row r="1"><c r="A1"><f>S1!A1+1</f><v>2</v></c></row><mc:AlternateContent {MC}><mc:Choice Requires="x"><row r="9"/></mc:Choice></mc:AlternateContent>"#
    );
    let ac_row = format!(
        r#"<row r="1"><c r="A1"><f>S1!A1+1</f><v>2</v></c><mc:AlternateContent {MC}><mc:Choice Requires="x"><c r="B1"><v>1</v></c></mc:Choice></mc:AlternateContent></row>"#
    );
    for sd2 in [&ac_sd, &ac_row] {
        let bytes = common::p1::book(&[("S1", r#"<row r="1"><c r="A1"><v>1</v></c></row>"#), ("S2", sd2)], "", r#"<calcPr calcId="191029"/>"#);
        let mut wb = Workbook::from_bytes(bytes).unwrap();
        wb.worksheet_by_name("S1").unwrap().cell_mut(0, 0).unwrap().set_number(5.0).unwrap();
        let ws = wb.worksheet_by_name("S2").unwrap();
        let a1 = ws.cell(0, 0);
        if std::ptr::eq(sd2, &ac_sd) {
            assert_matches!(a1, Err(Error::Core(ooxml_core::Error::UnresolvedSibling(_))), "reads of S2 still refuse");
        } else {
            assert_matches!(a1, Err(Error::Core(ooxml_core::Error::UnresolvedSibling(_))), "the row still refuses");
        }
        let out = wb.to_bytes().unwrap();
        let s2 = String::from_utf8(part(&out, "xl/worksheets/sheet2.xml")).unwrap();
        assert!(s2.contains(r#"<c r="A1"><f>S1!A1+1</f></c>"#), "the dependent's cache was stripped: {s2}");
        // the target refuses on that sheet
        let mut wb = Workbook::from_bytes(out).unwrap();
        let r = wb.worksheet_by_name("S2").unwrap().cell_mut(0, 1).unwrap().set_number(1.0);
        assert_matches!(r, Err(Error::Core(ooxml_core::Error::UnresolvedSibling(_))));
    }
}

/// PR #29 review m3: a new cell goes to its indexed position (before the
/// row's `extLst`, between cells, at either end), byte-equal in structure to
/// a fresh open; a row whose `extLst` precedes a cell refuses a new cell.
#[test]
fn new_cells_at_indexed_positions() {
    let sd = r#"<row r="1"><c r="C1"><v>3</v></c> <!--x--> <c r="E1"><v>5</v></c> <extLst><ext uri="u"/></extLst></row><row r="2"><extLst><ext uri="u"/></extLst><c r="A2"/></row>"#;
    let bytes = common::p1::book(&[("S", sd)], "", "");
    let mut wb = Workbook::from_bytes(bytes).unwrap();
    for col in [6u32, 0, 3, 1, 5, 2, 9] {
        let r = wb.worksheet().unwrap().cell_mut(0, col).unwrap().set_number(f64::from(col) * 10.0);
        if col != 2 && col != 4 {
            r.unwrap();
        }
    }
    let out = wb.to_bytes().unwrap();
    let x = String::from_utf8(part(&out, "xl/worksheets/sheet1.xml")).unwrap();
    let refs: Vec<&str> = x.split(r#"<c r=""#).skip(1).map(|t| t.split('"').next().unwrap()).collect();
    assert_eq!(refs[..8], ["A1", "B1", "C1", "D1", "E1", "F1", "G1", "J1"], "{x}");
    let row1 = x.split("</row>").next().unwrap();
    assert!(row1.ends_with(r#"<extLst><ext uri="u"/></extLst>"#), "extLst stays last: {row1}");
    assert_eq!(snapshot(&mut wb, "S"), snapshot(&mut Workbook::from_bytes(out.clone()).unwrap(), "S"));
    // extLst before a cell: refused as insert_by refuses it; nothing changes
    let mut wb = Workbook::from_bytes(out.clone()).unwrap();
    let r = wb.worksheet().unwrap().cell_mut(1, 3).unwrap().set_number(1.0);
    assert_matches!(r, Err(Error::Core(ooxml_core::Error::UnresolvedSibling(_))));
    assert_eq!(wb.to_bytes().unwrap(), out);
}

/// PR #29 review m6: a small differential check. Random writes of every kind
/// (numbers, strings, formulas, formula removal, on two sheets with
/// dependents across them); after every few writes, the maintained workbook
/// reads the same as a fresh open of its own bytes, and `cells()` agrees
/// with `cell()`.
#[test]
fn random_writes_of_every_kind_match_a_fresh_open() {
    let s1 = r#"<row r="2"><c r="B2"><v>1</v></c><c r="D2"><f>B2*2</f><v>2</v></c></row> <row r="5"><c r="A5"><f>S2!C3+B2</f><v>9</v></c></row>"#;
    let s2 = r#"<row r="3"><c r="C3"><v>4</v></c><c r="E3"><f>S1!D2+1</f><v>3</v></c><extLst><ext uri="u"/></extLst></row>"#;
    let bytes = common::p1::book(&[("S1", s1), ("S2", s2)], "", r#"<calcPr calcId="191029"/>"#);
    let mut wb = Workbook::from_bytes(bytes).unwrap();
    let mut seed = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = |n: u64| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed % n
    };
    let mut oks = 0;
    for k in 0..400u32 {
        let sheet = if next(2) == 0 { "S1" } else { "S2" };
        let (row, col) = (next(9) as u32, next(7) as u32);
        let other = CellIndex { row: next(9) as u32, col: next(7) as u32 };
        let op = next(8);
        let r = {
            let mut ws = wb.worksheet_by_name(sheet).unwrap();
            let c = ws.cell_mut(row, col).unwrap();
            match op {
                0..=2 => c.set_number(f64::from(k)),
                3 => c.set_string(&format!("s{k}")),
                4 => c.set_formula(&format!("{other}+1")),
                5 => c.set_formula(&format!("S1!{other}*2")),
                6 => c.set_formula(&format!("SUM(S2!A1:{other})")),
                _ => c.remove_formula(),
            }
        };
        oks += usize::from(r.is_ok());
        if k % 20 == 19 {
            let out = wb.to_bytes().unwrap();
            let mut fresh = Workbook::from_bytes(out).unwrap();
            for s in ["S1", "S2"] {
                let snap = snapshot(&mut wb, s);
                assert_eq!(snap, snapshot(&mut fresh, s), "after op {k}, sheet {s}");
                let ws = wb.worksheet_by_name(s).unwrap();
                for (r, f, v) in &snap {
                    let at = CellIndex::parse(r).unwrap();
                    let c = ws.cell(at.row, at.col).unwrap().unwrap();
                    assert_eq!((c.reference(), c.formula(), format!("{:?}", c.value())), (r.as_str(), f.clone(), v.clone()));
                }
            }
        }
    }
    assert!(oks > 300, "{oks}");
}
