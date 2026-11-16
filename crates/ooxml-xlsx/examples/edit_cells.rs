//! Open a workbook, read its first visible sheet, edit two cells, save it,
//! and show how refusals look.
//!
//! ```sh
//! cargo run -p ooxml-xlsx --example edit_cells -- [OUT_DIR]
//! ```
//!
//! `OUT_DIR` defaults to `<temp dir>/ooxml-rs-examples`. Inputs are fixtures
//! committed under `reference/`.

use std::path::PathBuf;

use ooxml_xlsx::{CellValue, Error, Workbook};

const REFERENCE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../reference");

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = std::env::args().nth(1).map_or_else(|| std::env::temp_dir().join("ooxml-rs-examples"), PathBuf::from);
    std::fs::create_dir_all(&out)?;

    // 1. Open and read.
    let input = format!("{REFERENCE}/round_trip_corpus/fixture_minimal.xlsx");
    let mut wb = Workbook::open(&input)?;
    println!("opened {input}");
    println!("first visible sheet: {:?} ({})", wb.sheet_name(), wb.worksheet_part());
    {
        let ws = wb.worksheet()?;
        for row in 0..3 {
            for col in 0..4 {
                // `cell` never changes the tree; `None` means the cell does not exist.
                if let Some(cell) = ws.cell(row, col)? {
                    let value = match cell.value()? {
                        CellValue::Empty => "(empty)".to_owned(),
                        CellValue::Number(n) => n.to_string(),
                        CellValue::String(s) => format!("{s:?}"),
                        CellValue::Bool(b) => b.to_string(),
                        CellValue::Error(e) => format!("error {e}"),
                        CellValue::Date(d) => format!("date {d}"),
                    };
                    println!("  {} = {value}", cell.reference());
                }
            }
        }
    }

    // 2. Edit. Workbook-wide refusals (a formula anywhere, calcChain) are
    //    known up front.
    if let Some(why) = wb.edit_refusal() {
        println!("edits would be refused: {why}");
        return Ok(());
    }
    {
        let mut ws = wb.worksheet()?;
        ws.cell_mut(1, 1)?.set_number(42.5)?; // B2
        ws.cell_mut(5, 0)?.set_string("written by ooxml-rs")?; // A6, created if missing
    }
    let saved = out.join("edit_cells.xlsx");
    wb.save(&saved)?;
    println!("saved {} (only the edited sheet part was rewritten)", saved.display());

    let mut again = Workbook::open(&saved)?;
    let ws = again.worksheet()?;
    for a1 in ["B2", "A6"] {
        let v = ws.cell_a1(a1)?.map(|c| c.value()).transpose()?;
        println!("  re-read {a1} = {v:?}");
    }

    // 3. Refusals: typed errors, and nothing is changed.
    let formulas = format!("{REFERENCE}/fixtures/xlsx/poi/shared_formulas.xlsx");
    let mut wb = Workbook::open(&formulas)?;
    match wb.worksheet()?.cell_mut(0, 0)?.set_number(1.0) {
        Err(e @ Error::FormulaPresent { .. }) => println!("refused (formulas present): {e}"),
        other => println!("unexpected: {other:?}"),
    }
    let strict = format!("{REFERENCE}/fixtures/xlsx/poi/SimpleStrict.xlsx");
    match Workbook::open(&strict) {
        Err(Error::Core(e)) => println!("refused at open (ooxml-core): {e}"),
        other => println!("unexpected: {:?}", other.map(|_| ())),
    }
    Ok(())
}
