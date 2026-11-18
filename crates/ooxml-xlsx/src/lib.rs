//! # ooxml-xlsx
//!
//! Lossless Excel (`.xlsx`/`.xlsm`) editing on the `ooxml-core` foundation:
//! open a workbook written by anyone, overwrite or create cell values and
//! formulas on any worksheet, save, and lose nothing else. Untouched parts
//! are copied through with their original compressed bytes; inside an
//! edited part, untouched elements are written back byte for byte.
//!
//! - Phase0 ([ADR-0001](https://github.com/cuzic/ooxml-rs/blob/main/docs/adr/0001-lossless-round-trip-architecture.md)):
//!   value edits, refusals, a from-scratch template.
//! - Phase1 ([ADR-0003](https://github.com/cuzic/ooxml-rs/blob/main/docs/adr/0003-xlsx-phase1-formula-and-calc-state.md)):
//!   edits in workbooks with formulas. Every formula that may depend on an
//!   edited cell loses its cached value (so no reader sees a stale one),
//!   `fullCalcOnLoad` is set, and `calcChain.xml` is removed when the set of
//!   formula cells changes. Formulas can be written and removed.
//!
//! ```no_run
//! # fn main() -> ooxml_xlsx::Result<()> {
//! let mut wb = ooxml_xlsx::Workbook::open("book.xlsx")?;
//! let mut ws = wb.worksheet_by_name("Inputs")?;
//! ws.cell_mut(1, 1)?.set_number(42.0)?;         // B2; dependents lose their caches
//! ws.cell_mut(0, 0)?.set_string("  padded ")?;  // A1: shared string if the file has a table
//! ws.cell_mut(2, 1)?.set_formula("B2*2")?;      // B3
//! wb.save("book.xlsx")?;                         // same path is safe
//! # Ok(()) }
//! ```
//!
//! From scratch (`Workbook::new` opens an embedded template):
//!
//! ```
//! # fn main() -> ooxml_xlsx::Result<()> {
//! let mut wb = ooxml_xlsx::Workbook::new()?;
//! let mut ws = wb.add_worksheet()?;
//! ws.write_string(0, 0, "a")?;
//! ws.write_number(1, 1, 2.0)?;
//! let bytes = wb.to_bytes()?;
//! # assert!(!bytes.is_empty()); Ok(()) }
//! ```
//!
//! ## What is refused (`Err`, nothing changed)
//!
//! - every edit when the formula census is incomplete or the workbook has
//!   cell metadata Phase1 does not model ([`Workbook::edit_refusal`]);
//! - writes into array/data-table/spill ranges, edits that would invalidate
//!   macro-sheet formulas, edits with dependents under iterative
//!   calculation, shared-formula hand-offs whose translation fails;
//! - writes into a merged range (top-left cell included), into a table's
//!   header row, or into a cell with a rich value (`vm`);
//! - reads and writes where `row`/`c` elements lack `r`
//!   ([`Error::MissingReference`]);
//! - adding or naming sheets (except the template's one sheet);
//! - encrypted and Strict packages, DTDs (from `ooxml-core`).
//!
//! See `README.md` for the full contract.

mod census;
mod core_props;
mod edit;
mod error;
mod escape;
mod formula;
mod graph;
mod index;
mod protect;
mod range;
mod reference;
mod sheet_ops;
mod registry;
mod setters;
mod workbook;
mod worksheet;

pub use error::{Error, Result};
pub use reference::{CellIndex, CellRange, MAX_COLS, MAX_ROWS};
#[doc(hidden)]
pub use workbook::ApplyStep;
pub use sheet_ops::{RewrittenLocation, SheetState};
pub use registry::dates::ExcelDateTime;
pub use registry::read::FormatRead;
pub use registry::format::{Alignment, Border, Color, DxfFont, DxfFormat, Fill, Font, FontSize, Format, NumFormat, Protection, Side, Tint, Underline, VertAlign};
pub use workbook::{CalcPolicy, Workbook};
pub use formula::future::add_future_prefixes;
pub use worksheet::{CellMut, CellRef, CellValue, CellWrite, Worksheet};
pub use protect::{Allow, DEFAULT_SPIN_COUNT, MAX_SPIN_COUNT, SheetProtection};

/// Test hook (not API): does `text` lex and parse, and does translating it
/// by one row and column either succeed or fail cleanly? Never panics.
#[doc(hidden)]
pub fn __formula_parses(text: &str) -> bool {
    let ok = formula::parse::parse(text).is_ok();
    let _ = formula::translate::translate(text, 1, 1, &|_| false);
    let _ = formula::translate::translate(text, -1, -1, &|_| false);
    ok
}

/// The SpreadsheetML main namespace (Transitional).
pub const SML_NS: &str = ooxml_schema::sml::NS_X;
