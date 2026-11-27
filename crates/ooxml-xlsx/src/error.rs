//! The crate's error type.

/// Errors from `ooxml-xlsx`.
///
/// Most variants are Phase0 *refusals* (ADR-0001 "fail honestly rather than
/// silently producing a corrupt-looking-fine file"): the operation is not
/// implemented safely yet, so nothing is changed and an error is returned.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// A structural edit has an empty or out-of-bounds band.
    #[error("invalid structural edit band at {at} with count {count}")]
    InvalidStructuralBand { at: u32, count: u32 },
    /// An insertion would push existing content or a reference off the sheet.
    #[error("structural edit would shift {what} off the sheet")]
    ShiftOffSheet { what: String },
    /// A structural edit would change an unsupported object or feature.
    #[error("structural edit blocked by {place} in {part}")]
    StructureBlocked {
        /// OPC part containing the object or feature.
        part: String,
        /// Object or feature that blocks the edit.
        place: String,
    },
    /// A structural edit would delete a required reference.
    #[error("structural edit would delete the reference at {location} in {part}")]
    ReferenceDeleted {
        /// OPC part containing the reference.
        part: String,
        /// Location of the reference.
        location: String,
    },
    /// A structural edit crosses an array or data-table result range.
    #[error("structural edit splits the formula range at {cell}")]
    ArrayFormulaSplit { cell: String },

    /// The sparkline group is empty or has a non-finite custom axis bound.
    #[error("sparkline group must have entries and finite custom axis bounds")]
    InvalidSparklineGroup,
    /// The object container cannot be edited safely.
    #[error("cannot edit {part}: {reason}")]
    PartNotEditable {
        /// OPC part name.
        part: String,
        /// Why it cannot be edited.
        reason: String,
    },
    /// Workbook or worksheet protection forbids this operation.
    #[error("{scope} protection forbids {operation}")]
    Protected {
        /// The protected scope.
        scope: &'static str,
        /// The requested operation.
        operation: &'static str,
    },
    /// The cell already has a legacy note.
    #[error("the cell already has a note")]
    NoteExists,
    /// The cell already has a thread and its fallback is managed by that thread.
    #[error("the cell has a threaded comment")]
    ThreadedComment,
    /// The cell has no note or thread for this operation.
    #[error("the cell has no matching note or thread")]
    NoSuchNote,
    /// Unsupported image format.
    #[error("unrecognized image format")]
    ImageFormat,
    /// Missing or out-of-range image dimensions.
    #[error("invalid image dimensions")]
    ImageSize,
    /// The anchor is outside the worksheet or the OOXML coordinate range.
    #[error("invalid image anchor")]
    ImageAnchor,
    /// The id does not identify exactly one removable picture.
    #[error("no uniquely removable image with id {0}")]
    NoSuchImage(u32),
    /// A cell's rich-value data could not be resolved.
    #[error("unresolved rich value at {step}")]
    UnresolvedRichValue {
        /// The failed resolution step.
        step: &'static str,
    },
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

    /// A value or formula write to a **non-anchor** cell of a merged range
    /// (ADR-0014 §3: the top-left cell, the anchor, is writable; a value in
    /// any other cell would be hidden by the merge).
    #[error("cell {cell} is a non-anchor cell of the merged range {range}")]
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

    /// Caller input (not file content) is invalid. `cell` is `Some` for cell-level
    /// validation, `None` for batch-level argument errors (ADR-0023 §8).
    #[error("{message}")]
    InvalidArgument {
        /// The cell, A1 style, when the error is about one cell.
        cell: Option<String>,
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

    /// No defined name folds equal to the requested one (in the requested
    /// scope, if any).
    #[error("no defined name {0:?}")]
    NoSuchName(String),

    /// The lookup cannot pick one name: two fold alike in the chosen scope,
    /// or a same-folded name has an invalid scope (fail closed).
    #[error("ambiguous defined name {name:?}: {reason}")]
    AmbiguousName {
        /// The requested name, as passed.
        name: String,
        /// One of `"invalid localSheetId"`, `"names fold alike"`.
        reason: &'static str,
    },

    /// The name resolves but its text is not one sheet-qualified absolute
    /// area on a worksheet.
    #[error("defined name {name:?} is not a range: {reason}")]
    NameNotARange {
        /// The requested name, as passed.
        name: String,
        /// One of `"constant"`, `"formula"`, `"multiple areas"`,
        /// `"relative reference"`, `"external reference"`, `"#REF!"`,
        /// `"3D reference"`, `"not a worksheet"`.
        reason: &'static str,
    },

    /// An earlier edit failed while being applied (a bug: the plan had
    /// proved it would succeed). The workbook may hold a partial edit, so
    /// every later edit and every save is refused; re-open the file.
    #[error("the workbook is poisoned by a failed edit ({0}); re-open it")]
    Poisoned(String),

    /// Naming or renaming a sheet is not supported in Phase0 (it touches
    /// `workbook.xml` and `docProps/app.xml` together).
    #[error("naming or renaming a sheet is not supported in Phase0")]
    SheetNamingUnsupported,

    /// A batch write ([`crate::Worksheet::write_cells`]) names the same cell
    /// twice (ADR-0003 addendum P5-pre).
    #[error("cell {cell} is written more than once in one batch")]
    DuplicateCellWrite {
        /// The cell.
        cell: String,
    },

    /// A formula calls a known future function (added to Excel after the
    /// 2007 format) without its file prefix; Excel would show `#NAME?`
    /// (ADR-0003 addendum P5-pre, ADR-0017 Q7). Write the file form, or
    /// convert with [`crate::add_future_prefixes`].
    #[error("{name} must be written as {prefix}{name} in the file")]
    MissingFunctionPrefix {
        /// The function, without prefix.
        name: String,
        /// The prefix it needs (`_xlfn.` or `_xlfn._xlws.`).
        prefix: &'static str,
    },

    /// A formula uses the spill (`A1#`) or implicit-intersection (`@`)
    /// operator, whose file forms are function calls (ADR-0003 addendum
    /// P5-pre, ADR-0017 review m7).
    #[error("the {operator} operator is written as {file_form} in the file")]
    DynamicArrayOperator {
        /// `#` or `@`.
        operator: char,
        /// `_xlfn.ANCHORARRAY(…)` or `_xlfn.SINGLE(…)`.
        file_form: &'static str,
    },

    /// A sheet name Excel would not accept (ADR-0009 §3): empty or longer
    /// than 31 UTF-16 units, one of `: \\ / ? * [ ]`, U+0000 or U+0003, a
    /// leading or trailing `'`, or `History`.
    #[error("invalid sheet name {name:?}: {reason}")]
    InvalidSheetName {
        /// The name.
        name: String,
        /// Which rule.
        reason: &'static str,
    },

    /// Another sheet already has this name (compared as ADR-0003 folds).
    #[error("a sheet named {0:?} exists")]
    DuplicateSheetName(String),

    /// The operation would leave no visible sheet, or remove the last one.
    #[error("the workbook must keep at least one visible sheet")]
    LastVisibleSheet,

    /// The workbook is shared (a `revisions` relationship): its revision log
    /// keeps a sheet-id map Phase3 cannot maintain.
    #[error("shared workbooks are not supported for sheet operations")]
    SharedWorkbook,

    /// The sheet to delete owns parts Phase3 does not cascade (pivot tables,
    /// slicers, OLE objects, controls, …), by relationship type.
    #[error("the sheet has parts that cannot be deleted with it: {rel_types:?}")]
    SheetHasUnsupportedParts {
        /// The relationship types.
        rel_types: Vec<String>,
    },

    /// A pivot cache takes its data from the sheet.
    #[error("sheet {0:?} is the source of a pivot cache")]
    SheetIsPivotSource(String),

    /// A table on the sheet is used elsewhere (structured references).
    #[error("table {0:?} on the sheet is used elsewhere")]
    TableInUse(String),
    /// The requested table range cannot be used.
    #[error("invalid table range: {reason}")]
    TableRange { reason: &'static str },
    /// The supplied name is not an Excel table name.
    #[error("invalid table name: {reason}")]
    InvalidTableName { reason: &'static str },
    /// Two table columns have the same name.
    #[error("duplicate column name: {0}")]
    DuplicateColumnName(String),
    /// A column heading cannot be derived from this cell.
    #[error("header cell is not text: {0}")]
    HeaderNotText(String),
    /// A formula uses the column name.
    #[error("table column is in use: {0}")]
    ColumnInUse(String),
    /// The named table is absent from the worksheet.
    #[error("no such table: {0}")]
    NoSuchTable(String),

    /// A custom workbook view names the sheet as its active sheet.
    #[error("a custom workbook view references the sheet")]
    CustomViewReferencesSheet,

    /// A reference to the sheet sits where this version cannot rewrite it
    /// (ADR-0009 §6 closed-world scan): the operation is refused rather than
    /// leaving a reference that silently stops working.
    #[error("{part}: a reference to the sheet at {location} cannot be rewritten")]
    UnrewritableReference {
        /// The part.
        part: String,
        /// Where in it.
        location: String,
    },

    /// The move would reverse a 3-D reference's endpoints (ADR-0009 §7).
    #[error("the move would reverse the 3-D reference {0}")]
    ReversesThreeDReference(String),

    /// The workbook has ActiveX binaries, whose linked cells cannot be read
    /// or rewritten: renames are refused.
    #[error("the workbook has ActiveX controls whose references cannot be checked")]
    BinaryReferencesPossible,

    /// `delete_sheet_strict`: the sheet is still referenced.
    #[error("sheet {sheet:?} is still referenced ({count} references)")]
    SheetInUse {
        /// The sheet.
        sheet: String,
        /// Number of references found.
        count: usize,
    },

    /// A format or style write needs `styles.xml` and the workbook has none
    /// (ADR-0006 §5.1; creating the part is out of scope).
    #[error("the workbook has no styles part")]
    NoStylesPart,

    /// The styles part's root is not a SpreadsheetML `styleSheet`.
    #[error("unsupported styles part: {0}")]
    UnsupportedStylesPart(String),

    /// A style table would exceed Excel's limit (ADR-0006 §5.10).
    #[error("the {table} table is full (Excel limit {limit})")]
    StyleTableFull {
        /// The table (`cellXfs+cellStyleXfs`, `fonts`, `fills`, `borders`,
        /// `custom numFmts`).
        table: String,
        /// The limit.
        limit: u32,
    },

    /// No custom number format id is left (`numFmtId` would exceed 65,535).
    #[error("no free custom numFmtId (ids are 16-bit)")]
    NumFmtIdExhausted,

    /// A built-in number format id the workbook redefines in `numFmts`;
    /// its meaning in this file is not the built-in one.
    #[error("built-in number format {id} is redefined by this workbook")]
    BuiltinNumFormatRedefined {
        /// The id.
        id: u32,
    },

    /// A [`crate::Format`] value outside its schema enumeration or range
    /// (fill pattern, border style, alignment, font scheme, …).
    #[error("invalid format value: {0}")]
    InvalidFormat(String),

    /// A number format code XML cannot carry.
    #[error("invalid number format code {0:?}")]
    InvalidNumFormat(String),

    /// `set_style_index` beyond `cellXfs`.
    #[error("style index {index} is out of range (cellXfs has {count} entries)")]
    StyleIndexOutOfRange {
        /// The index.
        index: u32,
        /// Entries in `cellXfs`.
        count: u32,
    },

    /// No named cell style with that name.
    #[error("no cell style named {0:?}")]
    NoSuchStyle(String),

    /// An existing reference (a cell `s`, an `xf`'s `fontId`, …) points at
    /// or past the end of a table; appending would give it a meaning
    /// (ADR-0006 §2).
    #[error("an existing reference to {table}[{index}] is out of range; appending would give it a meaning")]
    DanglingReference {
        /// The table.
        table: String,
        /// The largest out-of-range index.
        index: u32,
    },

    /// Serial 60 in the 1900 date system: Excel's nonexistent 1900-02-29.
    #[error("serial 60 is Excel's nonexistent 1900-02-29")]
    NonexistentDate,

    /// Several cell styles have that name (compared case-insensitively).
    #[error("several cell styles are named {0:?}")]
    AmbiguousStyleName(String),

    /// The requested change cannot be expressed without losing content the
    /// registry does not model (extension attributes or children).
    #[error("cannot express the style change: {0}")]
    NotExpressible(String),

    /// The date does not exist in the workbook's date system or is outside
    /// its range (1900 system: 1899-12-31 to 9999-12-31, never the false
    /// 1900-02-29; 1904 system: from 1904-01-01).
    #[error("date out of range for the workbook's date system")]
    DateOutOfRange,

    // ---- Phase4 (ADR-0014 §13) ----
    /// A merge needs at least two cells.
    #[error("{0} is a single cell; a merge needs at least two")]
    MergeSingleCell(String),

    /// The range overlaps an existing merged range.
    #[error("{range} overlaps the merged range {existing}")]
    MergeOverlap {
        /// The requested range.
        range: String,
        /// The existing merged range.
        existing: String,
    },

    /// `merge` would hide values or formulas in non-anchor cells; use
    /// `merge_clearing` to clear them explicitly.
    #[error("merging {range} would hide the contents of {cells:?}")]
    MergeWouldHideValues {
        /// The requested range.
        range: String,
        /// The non-anchor cells that hold a value or a formula.
        cells: Vec<String>,
    },

    /// No merged range equals the given range.
    #[error("no merged range {0}")]
    NoSuchMerge(String),

    /// The range overlaps a table (a table carries its own autofilter, and
    /// cannot contain merged cells).
    #[error("{range} overlaps table {table:?}")]
    TableOverlap {
        /// The requested range.
        range: String,
        /// The table's name.
        table: String,
    },

    /// The range overlaps a PivotTable's location.
    #[error("{range} overlaps the PivotTable at {location}")]
    PivotOverlap {
        /// The requested range.
        range: String,
        /// The PivotTable's `location/@ref`, or its part name when the
        /// location cannot be read (treated as overlapping).
        location: String,
    },

    /// The sheet has a query table whose output range cannot be located,
    /// so a merge or filter on it is refused.
    #[error("the sheet has a query table ({0}); its output range may overlap")]
    QueryTableOverlap(String),

    /// The sheet's `autoFilter` has filter criteria or a sort state; moving
    /// its range would retarget them.
    #[error("the sheet's autoFilter has filter criteria or a sort state")]
    AutoFilterExists,

    /// The sheet's `autoFilter` filters rows (a `filterColumn`); removing it
    /// would leave rows hidden that no filter explains.
    #[error("the sheet's autoFilter is active (filterColumn); it is not removed")]
    FilterActive,

    /// More than one built-in name of this kind and scope.
    #[error("{name} is defined more than once for this sheet")]
    DuplicateBuiltInName {
        /// The name.
        name: String,
    },

    /// A formula uses the defined name that the edit would change.
    #[error("defined name {name} is used by a formula ({cell})")]
    NameInUse {
        /// The name.
        name: String,
        /// One formula cell that uses it (`Sheet!A1`), or `defined name X`.
        cell: String,
    },

    /// A protection spin count above 10,000,000 (POI's and Excel's limit).
    #[error("spin count {0} exceeds 10,000,000")]
    InvalidSpinCount(u32),

    /// A formula of a range feature (validation, conditional format)
    /// refers to another sheet directly; use a defined name for the source
    /// (ADR-0014 §5, Q2).
    #[error("{text:?} refers to another sheet; use a defined name instead")]
    CrossSheetReference {
        /// The formula.
        text: String,
    },

    /// A reference names a sheet that does not exist.
    #[error("no sheet {0:?}")]
    UnknownSheet(String),

    /// A new data validation overlaps an existing one (a cell has one
    /// validation).
    #[error("{sqref} overlaps the validation on {existing}")]
    ValidationOverlap {
        /// The new range.
        sqref: String,
        /// The existing rule's `sqref`.
        existing: String,
    },

    /// An inline list over 255 characters (quotes and commas included).
    #[error("the inline list has {0} characters (at most 255)")]
    ListTooLong(usize),

    /// An inline list item containing a comma cannot be represented.
    #[error("list item {0:?} contains a comma")]
    ListItemHasComma(String),

    /// A new hyperlink overlaps an existing one.
    #[error("{range} overlaps the hyperlink on {existing}")]
    HyperlinkOverlap {
        /// The new range.
        range: String,
        /// The existing hyperlink's `ref`.
        existing: String,
    },

    /// A URL or file target that cannot be stored.
    #[error("invalid link target: {0}")]
    InvalidUrl(String),

    /// The conditional format belongs to a PivotTable (`pivot="1"`).
    #[error("conditional formatting on {0} belongs to a PivotTable")]
    PivotConditionalFormat(String),

    /// No rule matches the given range (and priority).
    #[error("no {what} on {sqref}")]
    NoSuchRule {
        /// "validation" or "conditional format".
        what: &'static str,
        /// The range.
        sqref: String,
    },

    /// `freeze_panes(0, 0)`: nothing to freeze.
    #[error("freeze_panes(0, 0) freezes nothing")]
    NothingToFreeze,

    /// The sheet view is `view="pageLayout"`, which cannot hold a frozen pane.
    #[error("the sheet view is in page layout mode, which cannot hold a frozen pane")]
    PageLayoutView,

    /// The sheet has more than one `cols` element, overlapping `col`
    /// ranges, or a `col` attribute this crate cannot copy when splitting.
    #[error("the sheet's column definitions are ambiguous: {0}")]
    AmbiguousColumns(String),

    /// An outline level outside 0–7.
    #[error("outline level {0} is outside 0–7")]
    InvalidOutlineLevel(u8),

    /// A header or footer string Excel cannot hold (over 255 characters, an
    /// unknown `&` code, an image code `&G`).
    #[error("invalid header/footer text: {0}")]
    InvalidHeaderFooter(String),

    /// A setting outside its allowed range (zoom 10–400, column width
    /// 0–255, scale 10–400, a margin below 0, …).
    #[error("{what}: {reason}")]
    InvalidSetting {
        /// The setting.
        what: String,
        /// Why it is refused.
        reason: String,
    },

    /// The operating system's random number generator failed (salt for a
    /// password hash).
    #[error("could not obtain random bytes for the salt: {0}")]
    Random(String),
}

/// `Result` with [`Error`].
pub type Result<T, E = Error> = std::result::Result<T, E>;

impl Error {
    /// The variant name, e.g. `"MergedCell"`. Stable: renaming a variant is a
    /// breaking change recorded in the owning ADR (ADR-0023 §8).
    pub fn kind(&self) -> &'static str {
        match self {
            Error::InvalidStructuralBand { .. } => "InvalidStructuralBand",
            Error::ShiftOffSheet { .. } => "ShiftOffSheet",
            Error::StructureBlocked { .. } => "StructureBlocked",
            Error::ReferenceDeleted { .. } => "ReferenceDeleted",
            Error::ArrayFormulaSplit { .. } => "ArrayFormulaSplit",
            Error::InvalidSparklineGroup => "InvalidSparklineGroup",
            Error::PartNotEditable { .. } => "PartNotEditable",
            Error::Protected { .. } => "Protected",
            Error::NoteExists => "NoteExists",
            Error::ThreadedComment => "ThreadedComment",
            Error::NoSuchNote => "NoSuchNote",
            Error::ImageFormat => "ImageFormat",
            Error::ImageSize => "ImageSize",
            Error::ImageAnchor => "ImageAnchor",
            Error::NoSuchImage(..) => "NoSuchImage",
            Error::UnresolvedRichValue { .. } => "UnresolvedRichValue",
            Error::Core(e) => e.kind(),
            Error::NoWorkbook => "NoWorkbook",
            Error::NoVisibleWorksheet => "NoVisibleWorksheet",
            Error::Malformed(..) => "Malformed",
            Error::FormulaScanFailed { .. } => "FormulaScanFailed",
            Error::MergedCell { .. } => "MergedCell",
            Error::RichValueCell { .. } => "RichValueCell",
            Error::TableHeaderCell { .. } => "TableHeaderCell",
            Error::MissingReference { .. } => "MissingReference",
            Error::OutOfOrder(..) => "OutOfOrder",
            Error::InvalidReference(..) => "InvalidReference",
            Error::OutOfRange { .. } => "OutOfRange",
            Error::InvalidNumber(..) => "InvalidNumber",
            Error::StringTooLong(..) => "StringTooLong",
            Error::InvalidValue { .. } => "InvalidValue",
            Error::InvalidArgument { .. } => "InvalidArgument",
            Error::SheetAdditionUnsupported => "SheetAdditionUnsupported",
            Error::ArrayResultCell { .. } => "ArrayResultCell",
            Error::ArrayAnchor { .. } => "ArrayAnchor",
            Error::UnknownCellMetadata { .. } => "UnknownCellMetadata",
            Error::MacroSheetDependent { .. } => "MacroSheetDependent",
            Error::IterativeCalculation => "IterativeCalculation",
            Error::SharedFormulaMaster { .. } => "SharedFormulaMaster",
            Error::InvalidFormula { .. } => "InvalidFormula",
            Error::NoSuchSheet(..) => "NoSuchSheet",
            Error::NotAWorksheet(..) => "NotAWorksheet",
            Error::NoSuchName(..) => "NoSuchName",
            Error::AmbiguousName { .. } => "AmbiguousName",
            Error::NameNotARange { .. } => "NameNotARange",
            Error::Poisoned(..) => "Poisoned",
            Error::SheetNamingUnsupported => "SheetNamingUnsupported",
            Error::DuplicateCellWrite { .. } => "DuplicateCellWrite",
            Error::MissingFunctionPrefix { .. } => "MissingFunctionPrefix",
            Error::DynamicArrayOperator { .. } => "DynamicArrayOperator",
            Error::InvalidSheetName { .. } => "InvalidSheetName",
            Error::DuplicateSheetName(..) => "DuplicateSheetName",
            Error::LastVisibleSheet => "LastVisibleSheet",
            Error::SharedWorkbook => "SharedWorkbook",
            Error::SheetHasUnsupportedParts { .. } => "SheetHasUnsupportedParts",
            Error::SheetIsPivotSource(..) => "SheetIsPivotSource",
            Error::TableInUse(..) => "TableInUse",
            Error::TableRange { .. } => "TableRange",
            Error::InvalidTableName { .. } => "InvalidTableName",
            Error::DuplicateColumnName(..) => "DuplicateColumnName",
            Error::HeaderNotText(..) => "HeaderNotText",
            Error::ColumnInUse(..) => "ColumnInUse",
            Error::NoSuchTable(..) => "NoSuchTable",
            Error::CustomViewReferencesSheet => "CustomViewReferencesSheet",
            Error::UnrewritableReference { .. } => "UnrewritableReference",
            Error::ReversesThreeDReference(..) => "ReversesThreeDReference",
            Error::BinaryReferencesPossible => "BinaryReferencesPossible",
            Error::SheetInUse { .. } => "SheetInUse",
            Error::NoStylesPart => "NoStylesPart",
            Error::UnsupportedStylesPart(..) => "UnsupportedStylesPart",
            Error::StyleTableFull { .. } => "StyleTableFull",
            Error::NumFmtIdExhausted => "NumFmtIdExhausted",
            Error::BuiltinNumFormatRedefined { .. } => "BuiltinNumFormatRedefined",
            Error::InvalidFormat(..) => "InvalidFormat",
            Error::InvalidNumFormat(..) => "InvalidNumFormat",
            Error::StyleIndexOutOfRange { .. } => "StyleIndexOutOfRange",
            Error::NoSuchStyle(..) => "NoSuchStyle",
            Error::DanglingReference { .. } => "DanglingReference",
            Error::NonexistentDate => "NonexistentDate",
            Error::AmbiguousStyleName(..) => "AmbiguousStyleName",
            Error::NotExpressible(..) => "NotExpressible",
            Error::DateOutOfRange => "DateOutOfRange",
            Error::MergeSingleCell(..) => "MergeSingleCell",
            Error::MergeOverlap { .. } => "MergeOverlap",
            Error::MergeWouldHideValues { .. } => "MergeWouldHideValues",
            Error::NoSuchMerge(..) => "NoSuchMerge",
            Error::TableOverlap { .. } => "TableOverlap",
            Error::PivotOverlap { .. } => "PivotOverlap",
            Error::QueryTableOverlap(..) => "QueryTableOverlap",
            Error::AutoFilterExists => "AutoFilterExists",
            Error::FilterActive => "FilterActive",
            Error::DuplicateBuiltInName { .. } => "DuplicateBuiltInName",
            Error::NameInUse { .. } => "NameInUse",
            Error::InvalidSpinCount(..) => "InvalidSpinCount",
            Error::CrossSheetReference { .. } => "CrossSheetReference",
            Error::UnknownSheet(..) => "UnknownSheet",
            Error::ValidationOverlap { .. } => "ValidationOverlap",
            Error::ListTooLong(..) => "ListTooLong",
            Error::ListItemHasComma(..) => "ListItemHasComma",
            Error::HyperlinkOverlap { .. } => "HyperlinkOverlap",
            Error::InvalidUrl(..) => "InvalidUrl",
            Error::PivotConditionalFormat(..) => "PivotConditionalFormat",
            Error::NoSuchRule { .. } => "NoSuchRule",
            Error::NothingToFreeze => "NothingToFreeze",
            Error::PageLayoutView => "PageLayoutView",
            Error::AmbiguousColumns(..) => "AmbiguousColumns",
            Error::InvalidOutlineLevel(..) => "InvalidOutlineLevel",
            Error::InvalidHeaderFooter(..) => "InvalidHeaderFooter",
            Error::InvalidSetting { .. } => "InvalidSetting",
            Error::Random(..) => "Random",
        }
    }

    /// The A1 cell the error is about, for variants that carry one.
    pub fn cell(&self) -> Option<&str> {
        match self {
            Error::InvalidStructuralBand { .. } => None,
            Error::ShiftOffSheet { .. } => None,
            Error::StructureBlocked { .. } => None,
            Error::ReferenceDeleted { .. } => None,
            Error::ArrayFormulaSplit { cell, .. } => Some(cell.as_str()),
            Error::InvalidSparklineGroup => None,
            Error::PartNotEditable { .. } => None,
            Error::Protected { .. } => None,
            Error::NoteExists => None,
            Error::ThreadedComment => None,
            Error::NoSuchNote => None,
            Error::ImageFormat => None,
            Error::ImageSize => None,
            Error::ImageAnchor => None,
            Error::NoSuchImage(..) => None,
            Error::UnresolvedRichValue { .. } => None,
            Error::Core(e) => e.cell(),
            Error::NoWorkbook => None,
            Error::NoVisibleWorksheet => None,
            Error::Malformed(..) => None,
            Error::FormulaScanFailed { .. } => None,
            Error::MergedCell { cell, .. } => Some(cell.as_str()),
            Error::RichValueCell { cell, .. } => Some(cell.as_str()),
            Error::TableHeaderCell { cell, .. } => Some(cell.as_str()),
            Error::MissingReference { .. } => None,
            Error::OutOfOrder(..) => None,
            Error::InvalidReference(..) => None,
            Error::OutOfRange { .. } => None,
            Error::InvalidNumber(..) => None,
            Error::StringTooLong(..) => None,
            Error::InvalidValue { cell, .. } => Some(cell.as_str()),
            Error::InvalidArgument { cell, .. } => cell.as_deref(),
            Error::SheetAdditionUnsupported => None,
            Error::ArrayResultCell { cell, .. } => Some(cell.as_str()),
            Error::ArrayAnchor { cell, .. } => Some(cell.as_str()),
            Error::UnknownCellMetadata { cell, .. } => Some(cell.as_str()),
            Error::MacroSheetDependent { cell, .. } => Some(cell.as_str()),
            Error::IterativeCalculation => None,
            Error::SharedFormulaMaster { cell, .. } => Some(cell.as_str()),
            Error::InvalidFormula { .. } => None,
            Error::NoSuchSheet(..) => None,
            Error::NotAWorksheet(..) => None,
            Error::NoSuchName(..) => None,
            Error::AmbiguousName { .. } => None,
            Error::NameNotARange { .. } => None,
            Error::Poisoned(..) => None,
            Error::SheetNamingUnsupported => None,
            Error::DuplicateCellWrite { cell, .. } => Some(cell.as_str()),
            Error::MissingFunctionPrefix { .. } => None,
            Error::DynamicArrayOperator { .. } => None,
            Error::InvalidSheetName { .. } => None,
            Error::DuplicateSheetName(..) => None,
            Error::LastVisibleSheet => None,
            Error::SharedWorkbook => None,
            Error::SheetHasUnsupportedParts { .. } => None,
            Error::SheetIsPivotSource(..) => None,
            Error::TableInUse(..) => None,
            Error::TableRange { .. } => None,
            Error::InvalidTableName { .. } => None,
            Error::DuplicateColumnName(..) => None,
            Error::HeaderNotText(..) => None,
            Error::ColumnInUse(..) => None,
            Error::NoSuchTable(..) => None,
            Error::CustomViewReferencesSheet => None,
            Error::UnrewritableReference { .. } => None,
            Error::ReversesThreeDReference(..) => None,
            Error::BinaryReferencesPossible => None,
            Error::SheetInUse { .. } => None,
            Error::NoStylesPart => None,
            Error::UnsupportedStylesPart(..) => None,
            Error::StyleTableFull { .. } => None,
            Error::NumFmtIdExhausted => None,
            Error::BuiltinNumFormatRedefined { .. } => None,
            Error::InvalidFormat(..) => None,
            Error::InvalidNumFormat(..) => None,
            Error::StyleIndexOutOfRange { .. } => None,
            Error::NoSuchStyle(..) => None,
            Error::DanglingReference { .. } => None,
            Error::NonexistentDate => None,
            Error::AmbiguousStyleName(..) => None,
            Error::NotExpressible(..) => None,
            Error::DateOutOfRange => None,
            Error::MergeSingleCell(..) => None,
            Error::MergeOverlap { .. } => None,
            Error::MergeWouldHideValues { .. } => None,
            Error::NoSuchMerge(..) => None,
            Error::TableOverlap { .. } => None,
            Error::PivotOverlap { .. } => None,
            Error::QueryTableOverlap(..) => None,
            Error::AutoFilterExists => None,
            Error::FilterActive => None,
            Error::DuplicateBuiltInName { .. } => None,
            Error::NameInUse { cell, .. } => Some(cell.as_str()),
            Error::InvalidSpinCount(..) => None,
            Error::CrossSheetReference { .. } => None,
            Error::UnknownSheet(..) => None,
            Error::ValidationOverlap { .. } => None,
            Error::ListTooLong(..) => None,
            Error::ListItemHasComma(..) => None,
            Error::HyperlinkOverlap { .. } => None,
            Error::InvalidUrl(..) => None,
            Error::PivotConditionalFormat(..) => None,
            Error::NoSuchRule { .. } => None,
            Error::NothingToFreeze => None,
            Error::PageLayoutView => None,
            Error::AmbiguousColumns(..) => None,
            Error::InvalidOutlineLevel(..) => None,
            Error::InvalidHeaderFooter(..) => None,
            Error::InvalidSetting { .. } => None,
            Error::Random(..) => None,
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    fn samples() -> Vec<(Error, Option<&'static str>)> {
        vec![
            (Error::InvalidStructuralBand { at: 0, count: 0 }, None),
            (Error::ShiftOffSheet { what: String::from("x") }, None),
            (Error::StructureBlocked { part: String::from("x"), place: String::from("x") }, None),
            (Error::ReferenceDeleted { part: String::from("x"), location: String::from("x") }, None),
            (Error::ArrayFormulaSplit { cell: String::from("B2") }, Some("B2")),
            (Error::InvalidSparklineGroup, None),
            (Error::PartNotEditable { part: String::from("x"), reason: String::from("x") }, None),
            (Error::Protected { scope: "x", operation: "x" }, None),
            (Error::NoteExists, None),
            (Error::ThreadedComment, None),
            (Error::NoSuchNote, None),
            (Error::ImageFormat, None),
            (Error::ImageSize, None),
            (Error::ImageAnchor, None),
            (Error::NoSuchImage(0), None),
            (Error::UnresolvedRichValue { step: "x" }, None),
            (Error::Core(ooxml_core::Error::EncryptedPackage), None),
            (Error::NoWorkbook, None),
            (Error::NoVisibleWorksheet, None),
            (Error::Malformed(String::from("x")), None),
            (Error::FormulaScanFailed { part: String::from("x"), reason: String::from("x") }, None),
            (Error::MergedCell { cell: String::from("B2"), range: String::from("x") }, Some("B2")),
            (Error::RichValueCell { cell: String::from("B2") }, Some("B2")),
            (Error::TableHeaderCell { cell: String::from("B2"), table: String::from("x") }, Some("B2")),
            (Error::MissingReference { part: String::from("x"), what: String::from("x") }, None),
            (Error::OutOfOrder(String::from("x")), None),
            (Error::InvalidReference(String::from("x")), None),
            (Error::OutOfRange { row: 0, col: 0 }, None),
            (Error::InvalidNumber(0.0), None),
            (Error::StringTooLong(0), None),
            (Error::InvalidValue { cell: String::from("B2"), message: String::from("x") }, Some("B2")),
            (Error::InvalidArgument { cell: None, message: String::from("x") }, None),
            (Error::InvalidArgument { cell: Some(String::from("B2")), message: String::from("x") }, Some("B2")),
            (Error::SheetAdditionUnsupported, None),
            (Error::ArrayResultCell { cell: String::from("B2"), anchor: String::from("x") }, Some("B2")),
            (Error::ArrayAnchor { cell: String::from("B2") }, Some("B2")),
            (Error::UnknownCellMetadata { part: String::from("x"), cell: String::from("B2") }, Some("B2")),
            (Error::MacroSheetDependent { sheet: String::from("x"), cell: String::from("B2") }, Some("B2")),
            (Error::IterativeCalculation, None),
            (Error::SharedFormulaMaster { cell: String::from("B2"), reason: String::from("x") }, Some("B2")),
            (Error::InvalidFormula { text: String::from("x"), reason: String::from("x") }, None),
            (Error::NoSuchSheet(String::from("x")), None),
            (Error::NotAWorksheet(String::from("x")), None),
            (Error::NoSuchName(String::from("x")), None),
            (Error::AmbiguousName { name: String::from("x"), reason: "x" }, None),
            (Error::NameNotARange { name: String::from("x"), reason: "x" }, None),
            (Error::Poisoned(String::from("x")), None),
            (Error::SheetNamingUnsupported, None),
            (Error::DuplicateCellWrite { cell: String::from("B2") }, Some("B2")),
            (Error::MissingFunctionPrefix { name: String::from("x"), prefix: "x" }, None),
            (Error::DynamicArrayOperator { operator: 'x', file_form: "x" }, None),
            (Error::InvalidSheetName { name: String::from("x"), reason: "x" }, None),
            (Error::DuplicateSheetName(String::from("x")), None),
            (Error::LastVisibleSheet, None),
            (Error::SharedWorkbook, None),
            (Error::SheetHasUnsupportedParts { rel_types: vec![String::from("x")] }, None),
            (Error::SheetIsPivotSource(String::from("x")), None),
            (Error::TableInUse(String::from("x")), None),
            (Error::TableRange { reason: "x" }, None),
            (Error::InvalidTableName { reason: "x" }, None),
            (Error::DuplicateColumnName(String::from("x")), None),
            (Error::HeaderNotText(String::from("x")), None),
            (Error::ColumnInUse(String::from("x")), None),
            (Error::NoSuchTable(String::from("x")), None),
            (Error::CustomViewReferencesSheet, None),
            (Error::UnrewritableReference { part: String::from("x"), location: String::from("x") }, None),
            (Error::ReversesThreeDReference(String::from("x")), None),
            (Error::BinaryReferencesPossible, None),
            (Error::SheetInUse { sheet: String::from("x"), count: 0 }, None),
            (Error::NoStylesPart, None),
            (Error::UnsupportedStylesPart(String::from("x")), None),
            (Error::StyleTableFull { table: String::from("x"), limit: 0 }, None),
            (Error::NumFmtIdExhausted, None),
            (Error::BuiltinNumFormatRedefined { id: 0 }, None),
            (Error::InvalidFormat(String::from("x")), None),
            (Error::InvalidNumFormat(String::from("x")), None),
            (Error::StyleIndexOutOfRange { index: 0, count: 0 }, None),
            (Error::NoSuchStyle(String::from("x")), None),
            (Error::DanglingReference { table: String::from("x"), index: 0 }, None),
            (Error::NonexistentDate, None),
            (Error::AmbiguousStyleName(String::from("x")), None),
            (Error::NotExpressible(String::from("x")), None),
            (Error::DateOutOfRange, None),
            (Error::MergeSingleCell(String::from("x")), None),
            (Error::MergeOverlap { range: String::from("x"), existing: String::from("x") }, None),
            (Error::MergeWouldHideValues { range: String::from("x"), cells: vec![String::from("x")] }, None),
            (Error::NoSuchMerge(String::from("x")), None),
            (Error::TableOverlap { range: String::from("x"), table: String::from("x") }, None),
            (Error::PivotOverlap { range: String::from("x"), location: String::from("x") }, None),
            (Error::QueryTableOverlap(String::from("x")), None),
            (Error::AutoFilterExists, None),
            (Error::FilterActive, None),
            (Error::DuplicateBuiltInName { name: String::from("x") }, None),
            (Error::NameInUse { name: String::from("x"), cell: String::from("B2") }, Some("B2")),
            (Error::InvalidSpinCount(0), None),
            (Error::CrossSheetReference { text: String::from("x") }, None),
            (Error::UnknownSheet(String::from("x")), None),
            (Error::ValidationOverlap { sqref: String::from("x"), existing: String::from("x") }, None),
            (Error::ListTooLong(0), None),
            (Error::ListItemHasComma(String::from("x")), None),
            (Error::HyperlinkOverlap { range: String::from("x"), existing: String::from("x") }, None),
            (Error::InvalidUrl(String::from("x")), None),
            (Error::PivotConditionalFormat(String::from("x")), None),
            (Error::NoSuchRule { what: "x", sqref: String::from("x") }, None),
            (Error::NothingToFreeze, None),
            (Error::PageLayoutView, None),
            (Error::AmbiguousColumns(String::from("x")), None),
            (Error::InvalidOutlineLevel(0), None),
            (Error::InvalidHeaderFooter(String::from("x")), None),
            (Error::InvalidSetting { what: String::from("x"), reason: String::from("x") }, None),
            (Error::Random(String::from("x")), None),
        ]
    }

    #[test]
    fn kind_matches_debug_name() {
        for (error, _) in samples() {
            if matches!(error, Error::Core(_)) {
                continue;
            }
            let debug = format!("{error:?}");
            assert!(debug.starts_with(error.kind()), "{debug}");
        }
        let inner = ooxml_core::Error::EncryptedPackage;
        let (k, dbg) = (inner.kind().to_string(), format!("{inner:?}"));
        assert_eq!(Error::Core(inner).kind(), k);
        assert!(dbg.starts_with("EncryptedPackage"));
    }

    #[test]
    fn cell_reports_carrying_variants() {
        for (error, expected) in samples() {
            assert_eq!(error.cell(), expected);
        }
    }
}

