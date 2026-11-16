//! The crate's error type.

/// Errors from `ooxml-xlsx`.
///
/// Most variants are Phase0 *refusals* (ADR-0001 "fail honestly rather than
/// silently producing a corrupt-looking-fine file"): the operation is not
/// implemented safely yet, so nothing is changed and an error is returned.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// An error from the package or XML layer (`ooxml-core`), including
    /// [`ooxml_core::Error::EncryptedPackage`],
    /// [`ooxml_core::Error::StrictNotSupported`] and
    /// [`ooxml_core::Error::DtdNotAllowed`].
    #[error(transparent)]
    Core(#[from] ooxml_core::Error),

    /// The package has no `officeDocument` relationship to a workbook part.
    #[error("the package has no workbook part (officeDocument relationship)")]
    NoWorkbook,

    /// The workbook has no visible worksheet. Hidden sheets, chartsheets,
    /// macro sheets and dialog sheets are skipped when looking for the
    /// first sheet (ADR-0001 post-acceptance note, 2026-11-16).
    #[error("the workbook has no visible worksheet")]
    NoVisibleWorksheet,

    /// A part or relationship the workbook structure needs is missing or
    /// malformed.
    #[error("malformed workbook: {0}")]
    Malformed(String),

    /// A sheet part (or `metadata.xml`) could not be scanned for formulas
    /// (not well-formed, not UTF-8, an `f` outside a cell, a sheet part with
    /// formulas that `<sheets>` does not list): the formula census is
    /// incomplete, so every edit is refused (ADR-0003 §1). Reads still work.
    #[error("could not check {part} for formulas ({reason}); edits are refused")]
    FormulaScanFailed {
        /// The part that could not be scanned.
        part: String,
        /// Why.
        reason: String,
    },

    /// The cell is inside a merged range (any cell, the top-left one
    /// included).
    #[error("cell {cell} is inside the merged range {range}")]
    MergedCell {
        /// The cell, A1 style.
        cell: String,
        /// The merged range.
        range: String,
    },

    /// The cell carries a `vm` attribute (a rich value, e.g. an image in a
    /// cell); overwriting it would leave the rich value bound to the cell.
    #[error("cell {cell} holds a rich value (vm attribute); writes are refused")]
    RichValueCell {
        /// The cell, A1 style.
        cell: String,
    },

    /// The cell is in a table's header row; its text must stay equal to the
    /// table column name (`tableColumn/@name`), which Phase0 does not update.
    #[error("cell {cell} is a header cell of table {table:?}")]
    TableHeaderCell {
        /// The cell, A1 style.
        cell: String,
        /// The table's `displayName` (or its part name if it has none).
        table: String,
    },

    /// A `row` in `sheetData`, or a `c` in the row concerned, has no `r`
    /// attribute (position-implied references are not supported yet,
    /// ADR-0002 §4.2). Applies to reads and writes.
    #[error("{what} without an r attribute in {part}; position-implied references are not supported")]
    MissingReference {
        /// The worksheet part.
        part: String,
        /// `"row"` or `"cell in row N"`.
        what: String,
    },

    /// Rows or cells are not in ascending order, so there is no safe
    /// insertion point for a new one.
    #[error("{0} are not in ascending order; cannot insert")]
    OutOfOrder(String),

    /// A cell reference (or range) is not valid A1 notation, or a cell's
    /// `r` disagrees with its row.
    #[error("invalid cell reference {0:?}")]
    InvalidReference(String),

    /// Row or column index beyond Excel's limits (1,048,576 rows,
    /// 16,384 columns).
    #[error("cell index out of range: row {row}, column {col} (0-based)")]
    OutOfRange {
        /// 0-based row.
        row: u32,
        /// 0-based column.
        col: u32,
    },

    /// NaN and infinities cannot be stored as a cell number.
    #[error("{0} is not a finite number")]
    InvalidNumber(f64),

    /// Excel cells hold at most 32,767 characters (UTF-16 code units);
    /// a longer string would be truncated or repaired by Excel.
    #[error("string of {0} UTF-16 code units exceeds Excel's 32,767-character cell limit")]
    StringTooLong(usize),

    /// A stored value could not be decoded (bad number, unknown cell type,
    /// shared-string index out of range, …).
    #[error("cell {cell}: {message}")]
    InvalidValue {
        /// The cell, A1 style.
        cell: String,
        /// What is wrong.
        message: String,
    },

    /// Adding worksheets is not supported in Phase0. The only exception is
    /// the first `add_worksheet()` on a [`Workbook::new`](crate::Workbook::new)
    /// workbook, which returns the template's one sheet.
    #[error("adding a worksheet is not supported in Phase0")]
    SheetAdditionUnsupported,

    /// The cell is a non-anchor cell of an array formula, data table or
    /// dynamic-array spill range (ADR-0003 §5); Excel forbids editing part
    /// of an array.
    #[error("cell {cell} is a result cell of the array/spill range anchored at {anchor}")]
    ArrayResultCell {
        /// The cell, A1 style.
        cell: String,
        /// The anchor cell.
        anchor: String,
    },

    /// The cell is the anchor of an array formula, data table or dynamic
    /// array; array editing is a later phase.
    #[error("cell {cell} anchors an array formula, data table or dynamic array; array editing is not supported")]
    ArrayAnchor {
        /// The cell, A1 style.
        cell: String,
    },

    /// The workbook has cell metadata (`c/@cm`) that is not an XLDAPR
    /// dynamic-array anchor; every edit is refused (ADR-0003 §1).
    #[error("{part}: cell {cell} has cell metadata (cm) that Phase1 does not model; edits are refused")]
    UnknownCellMetadata {
        /// The sheet part.
        part: String,
        /// The cell.
        cell: String,
    },

    /// The edit would make a formula on a macro sheet or dialog sheet a
    /// dependent; XLM caches are not modelled (ADR-0003 §5).
    #[error("the edit would invalidate the macro-sheet formula {sheet}!{cell}")]
    MacroSheetDependent {
        /// The macro or dialog sheet.
        sheet: String,
        /// The formula cell.
        cell: String,
    },

    /// The workbook uses iterative calculation (`calcPr/@iterate`) and the
    /// edit has dependents: removing their cached values would change what
    /// Excel computes (ADR-0003 §3.3).
    #[error("the workbook uses iterative calculation; edits with dependent formulas are refused")]
    IterativeCalculation,

    /// A shared formula's master was overwritten and its text could not be
    /// moved to the next member (ADR-0003 §4.2).
    #[error("cannot hand the shared formula of {cell} to another member: {reason}")]
    SharedFormulaMaster {
        /// The master cell.
        cell: String,
        /// Why the translation failed.
        reason: String,
    },

    /// `set_formula` text that does not lex, or holds characters XML
    /// cannot carry.
    #[error("invalid formula {text:?}: {reason}")]
    InvalidFormula {
        /// The text.
        text: String,
        /// Why.
        reason: String,
    },

    /// No sheet with that name or index.
    #[error("no sheet {0:?}")]
    NoSuchSheet(String),

    /// The sheet is a chartsheet, macro sheet or dialog sheet, not a
    /// worksheet.
    #[error("sheet {0:?} is not a worksheet")]
    NotAWorksheet(String),

    /// An earlier edit failed while being applied (a bug: the plan had
    /// proved it would succeed). The workbook may hold a partial edit, so
    /// every later edit and every save is refused; re-open the file.
    #[error("the workbook is poisoned by a failed edit ({0}); re-open it")]
    Poisoned(String),

    /// Naming or renaming a sheet is not supported in Phase0 (it touches
    /// `workbook.xml` and `docProps/app.xml` together).
    #[error("naming or renaming a sheet is not supported in Phase0")]
    SheetNamingUnsupported,
}

/// `Result` with [`Error`].
pub type Result<T, E = Error> = std::result::Result<T, E>;
