//! # ooxml-xlsx
//!
//! Lossless Excel (`.xlsx`/`.xlsm`) editing on the `ooxml-core` foundation,
//! Phase0 of [ADR-0001](https://github.com/cuzic/ooxml-rs/blob/main/docs/adr/0001-lossless-round-trip-architecture.md):
//! open a workbook written by anyone, overwrite or create cell values on
//! its first visible worksheet, save, and lose nothing else. Untouched parts
//! are copied through with their original compressed bytes; inside the
//! edited worksheet, untouched elements are written back byte for byte.
//!
//! ```no_run
//! # fn main() -> ooxml_xlsx::Result<()> {
//! let mut wb = ooxml_xlsx::Workbook::open("book.xlsx")?;
//! let mut ws = wb.worksheet()?;
//! ws.cell_mut(1, 1)?.set_number(42.0)?;         // B2
//! ws.cell_mut(0, 0)?.set_string("  padded ")?;  // A1, inline string
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
//! ## What Phase0 refuses (`Err`, nothing changed)
//!
//! - any value edit in a workbook containing a cell formula anywhere
//!   (SpreadsheetML `f` element in any sheet part) or a `calcChain.xml`:
//!   [`Error::FormulaPresent`];
//! - writes into a merged range (top-left cell included), into a table's
//!   header row, or into a cell with a rich value (`vm`);
//! - reads and writes where `row`/`c` elements lack `r`
//!   ([`Error::MissingReference`]);
//! - adding or naming sheets (except the template's one sheet);
//! - encrypted and Strict packages, DTDs (from `ooxml-core`).
//!
//! See `README.md` for the full contract.

mod core_props;
mod error;
mod escape;
mod formula;
mod reference;
mod scan;
mod workbook;
mod worksheet;

pub use error::{Error, Result};
pub use reference::{CellIndex, CellRange, MAX_COLS, MAX_ROWS};
pub use workbook::Workbook;
pub use worksheet::{CellMut, CellRef, CellValue, Worksheet};

/// The SpreadsheetML main namespace (Transitional).
pub const SML_NS: &str = ooxml_schema::sml::NS_X;
