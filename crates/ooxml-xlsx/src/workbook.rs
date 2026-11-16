//! [`Workbook`]: open/new/save, locating "sheet1", and the workbook-level
//! facts Phase0's refusals need (formula census, merges, table headers).

use std::path::Path;

use ooxml_core::opc::Relationships;
use ooxml_core::{EnumValue, Package, XmlDocument};
use ooxml_schema::sml::{self, SheetStateValues};

use crate::error::{Error, Result};
use crate::reference::CellRange;
use crate::worksheet::{Worksheet, text_of};
use crate::{core_props, scan};

const REL_WORKSHEET: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet";
const REL_SHARED_STRINGS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings";
const REL_CALC_CHAIN: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/calcChain";
const REL_TABLE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/table";
const CT_WORKSHEET: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml";
/// Parts that can hold cell formulas: worksheets, macro sheets, dialog sheets.
const SHEET_CONTENT_TYPES: &[&str] = &[
    CT_WORKSHEET,
    "application/vnd.ms-excel.macrosheet+xml",
    "application/vnd.ms-excel.intlmacrosheet+xml",
    "application/vnd.openxmlformats-officedocument.spreadsheetml.dialogsheet+xml",
];
const SHEET_REL_TYPES: &[&str] = &[
    REL_WORKSHEET,
    "http://schemas.microsoft.com/office/2006/relationships/xlMacrosheet",
    "http://schemas.microsoft.com/office/2006/relationships/xlIntlMacrosheet",
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/dialogsheet",
];
const STRICT_NS_PREFIX: &str = "http://purl.oclc.org/ooxml/";

/// The embedded from-scratch template (ADR-0001 "From-scratch path"); its
/// sources are `template/blank/`, built by `template/build.py`.
pub(crate) const TEMPLATE: &[u8] = include_bytes!("../template/blank.xlsx");

/// Why value edits are refused workbook-wide.
#[derive(Clone, Debug)]
pub(crate) enum EditBlock {
    Formula { part: String },
    ScanFailed { part: String, reason: String },
}

impl EditBlock {
    pub(crate) fn to_error(&self) -> Error {
        match self {
            EditBlock::Formula { part } => Error::FormulaPresent { part: part.clone() },
            EditBlock::ScanFailed { part, reason } => {
                Error::FormulaScanFailed { part: part.clone(), reason: reason.clone() }
            }
        }
    }
}

/// A table on the sheet: its header rows (empty if `headerRowCount="0"`).
#[derive(Clone, Debug)]
pub(crate) struct TableHeader {
    pub(crate) name: String,
    pub(crate) rows: Option<CellRange>,
}

/// What the worksheet API needs besides the sheet's own tree. Computed once
/// at open; Phase0 edits never change any of it.
#[derive(Debug)]
pub(crate) struct SheetContext {
    pub(crate) sheet_name: String,
    pub(crate) sheet_part: String,
    /// Decoded shared strings (`None`: the workbook has no shared-string part).
    pub(crate) shared_strings: Option<Vec<String>>,
    pub(crate) edit_block: Option<EditBlock>,
    pub(crate) merges: Vec<CellRange>,
    pub(crate) tables: Vec<TableHeader>,
}

/// An `.xlsx`/`.xlsm` workbook opened for lossless editing (ADR-0001 Phase0).
///
/// Phase0 works on one sheet, "sheet1": the **first visible worksheet in
/// workbook order** (hidden sheets, chartsheets, macro and dialog sheets are
/// skipped). Every part other than that worksheet is copied through
/// unchanged on save; `workbook.xml`, `sharedStrings.xml` and table parts are
/// parsed for reading only and are raw-copied as well.
///
/// # Examples
///
/// ```no_run
/// use ooxml_xlsx::{CellValue, Error, Workbook};
///
/// let mut wb = Workbook::open("book.xlsx")?;
/// if let Some(why) = wb.edit_refusal() {
///     return Err(why); // e.g. Error::FormulaPresent: nothing was changed
/// }
/// let mut ws = wb.worksheet()?;
/// if let Some(CellValue::Number(n)) = ws.cell_a1("B2")?.map(|c| c.value()).transpose()? {
///     ws.cell_mut(1, 1)?.set_number(n + 1.0)?;
/// }
/// ws.cell_mut(0, 0)?.set_string("total")?;
/// wb.save("book.xlsx")?; // temp file + rename: same path is safe
/// # Ok::<(), Error>(())
/// ```
///
/// A complete, runnable version is `examples/edit_cells.rs`.
#[derive(Debug)]
pub struct Workbook {
    pkg: Package,
    workbook_part: String,
    ctx: SheetContext,
    from_template: bool,
    template_sheet_taken: bool,
}

impl Workbook {
    /// Opens a file. The whole file is read into memory first, so saving to
    /// the same path is safe.
    ///
    /// Errors: [`ooxml_core::Error::EncryptedPackage`],
    /// [`ooxml_core::Error::StrictNotSupported`] (wrapped in [`Error::Core`]),
    /// [`Error::NoWorkbook`], [`Error::NoVisibleWorksheet`], XML errors of
    /// the parts that are parsed.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::from_package(Package::open(path)?)
    }

    /// Opens a workbook from bytes.
    pub fn from_bytes(bytes: impl Into<std::sync::Arc<[u8]>>) -> Result<Self> {
        Self::from_package(Package::from_bytes(bytes)?)
    }

    /// A new workbook from the embedded template: one empty worksheet named
    /// `Sheet1`, default styles, and `docProps/core.xml` regenerated with the
    /// current time (installed with `Package::set_part`). Use
    /// [`Self::add_worksheet`] to get the sheet.
    pub fn new() -> Result<Self> {
        let mut wb = Self::from_bytes(TEMPLATE)?;
        wb.pkg.set_part("docProps/core.xml", core_props::generate(std::time::SystemTime::now()))?;
        wb.from_template = true;
        Ok(wb)
    }

    /// Wraps an opened package.
    pub fn from_package(mut pkg: Package) -> Result<Self> {
        let workbook_part = pkg.main_document_part().ok_or(Error::NoWorkbook)?;
        let wb_rels = pkg.relationships(&workbook_part)?;
        let (sheet_name, sheet_part) = first_visible_worksheet(&mut pkg, &workbook_part, &wb_rels)?;
        let edit_block = formula_census(&pkg, &wb_rels)?;
        let shared_strings = load_shared_strings(&mut pkg, &wb_rels)?;
        let sheet_rels = pkg.relationships(&sheet_part)?;
        let doc = pkg.xml_part(&sheet_part)?;
        let ws = sml::WorksheetView(doc.lens::<sml::Worksheet>().map_err(|e| strict_or(e, &sheet_part))?);
        let merges = ws
            .merge_cells()
            .into_iter()
            .flat_map(|m| m.merge_cell_iter().collect::<Vec<_>>())
            .filter_map(|m| m.reference().map(CellRange::parse))
            .collect::<Result<Vec<_>>>()?;
        let table_parts: Vec<String> = ws
            .table_parts()
            .into_iter()
            .flat_map(|t| t.table_part_iter().collect::<Vec<_>>())
            .filter_map(|t| t.id().map(str::to_owned))
            .collect();
        let tables = load_tables(&mut pkg, &sheet_part, &sheet_rels, &table_parts)?;
        Ok(Self {
            pkg,
            workbook_part,
            ctx: SheetContext { sheet_name, sheet_part, shared_strings, edit_block, merges, tables },
            from_template: false,
            template_sheet_taken: false,
        })
    }

    /// "sheet1": the first visible worksheet in workbook order.
    pub fn worksheet(&mut self) -> Result<Worksheet<'_>> {
        let doc = self.pkg.xml_part_mut(&self.ctx.sheet_part)?;
        Ok(Worksheet::new(doc, &self.ctx))
    }

    /// From-scratch path only: the **first** call on a [`Self::new`]
    /// workbook returns the template's one worksheet. Any other call, and
    /// any call on an opened file, is [`Error::SheetAdditionUnsupported`]
    /// (ADR-0001 R2-B1: Phase0 never adds parts).
    pub fn add_worksheet(&mut self) -> Result<Worksheet<'_>> {
        if !self.from_template || self.template_sheet_taken {
            return Err(Error::SheetAdditionUnsupported);
        }
        self.template_sheet_taken = true;
        self.worksheet()
    }

    /// Always [`Error::SheetNamingUnsupported`] in Phase0 (naming a sheet
    /// touches `workbook.xml` and `docProps/app.xml` together).
    pub fn add_worksheet_with_name(&mut self, _name: &str) -> Result<Worksheet<'_>> {
        Err(Error::SheetNamingUnsupported)
    }

    /// Name of "sheet1" (the `name` attribute of its `<sheet>`).
    pub fn sheet_name(&self) -> &str {
        &self.ctx.sheet_name
    }

    /// Part name of "sheet1" (e.g. `xl/worksheets/sheet1.xml`).
    pub fn worksheet_part(&self) -> &str {
        &self.ctx.sheet_part
    }

    /// Part name of the workbook (e.g. `xl/workbook.xml`).
    pub fn workbook_part(&self) -> &str {
        &self.workbook_part
    }

    /// `Some(error)` if value edits are refused workbook-wide (a formula
    /// somewhere, a `calcChain.xml`, or a sheet that could not be scanned).
    pub fn edit_refusal(&self) -> Option<Error> {
        self.ctx.edit_block.as_ref().map(EditBlock::to_error)
    }

    /// The underlying package (part states, content types, relationships).
    pub fn package(&self) -> &Package {
        &self.pkg
    }

    /// Writes the workbook to `path` atomically (temporary file + rename).
    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        Ok(self.pkg.save(path)?)
    }

    /// Serializes the workbook.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        Ok(self.pkg.to_bytes()?)
    }
}

/// Maps a wrong-root error to `StrictNotSupported` when the root is in a
/// Strict namespace.
fn strict_or(e: ooxml_core::Error, part: &str) -> Error {
    match e {
        ooxml_core::Error::WrongElement { ref found_ns, .. } if found_ns.starts_with(STRICT_NS_PREFIX) => {
            Error::Core(ooxml_core::Error::StrictNotSupported(found_ns.clone()))
        }
        ooxml_core::Error::WrongElement { found_ns, found_local, .. } => {
            Error::Malformed(format!("{part}: root element is {{{found_ns}}}{found_local}"))
        }
        e => e.into(),
    }
}

/// ADR-0002 §6 with the 2026-11-16 owner decision: the first `<sheet>` in
/// workbook order that is visible and whose relationship and content type
/// are a worksheet's.
fn first_visible_worksheet(pkg: &mut Package, workbook_part: &str, rels: &Relationships) -> Result<(String, String)> {
    let mut candidates = Vec::new();
    {
        let doc = pkg.xml_part(workbook_part)?;
        let wb = sml::WorkbookView(doc.lens::<sml::Workbook>().map_err(|e| strict_or(e, workbook_part))?);
        let sheets = wb.sheets().ok_or_else(|| Error::Malformed(format!("{workbook_part} has no <sheets>")))?;
        for s in sheets.sheet_iter() {
            if !matches!(s.state(), EnumValue::Known(SheetStateValues::Visible)) {
                continue;
            }
            let (Some(name), Some(id)) = (s.name(), s.id()) else {
                return Err(Error::Malformed(format!("{workbook_part}: <sheet> without name or r:id")));
            };
            candidates.push((name.to_owned(), id.to_owned()));
        }
    }
    for (name, id) in candidates {
        let rel = rels
            .by_id(&id)
            .ok_or_else(|| Error::Malformed(format!("sheet {name:?}: relationship {id} not found")))?;
        if rel.rel_type != REL_WORKSHEET {
            continue;
        }
        let part = rels.resolve(rel).ok_or_else(|| Error::Malformed(format!("sheet {name:?} is external")))?;
        if !pkg.contains(&part) {
            return Err(Error::Malformed(format!("sheet {name:?}: part {part} not found")));
        }
        if pkg.content_type(&part) == Some(CT_WORKSHEET) {
            return Ok((name, part));
        }
    }
    Err(Error::NoVisibleWorksheet)
}

/// ADR-0001 R3-M1 by QName: scan every sheet part (by content type, and by
/// workbook relationship type) for a SpreadsheetML `f` element. A
/// `calcChain.xml` also blocks edits (the ADR's allowed fast path).
fn formula_census(pkg: &Package, wb_rels: &Relationships) -> Result<Option<EditBlock>> {
    if let Some(rel) = wb_rels.by_type(REL_CALC_CHAIN).next()
        && let Some(part) = wb_rels.resolve(rel)
        && pkg.contains(&part)
    {
        return Ok(Some(EditBlock::Formula { part }));
    }
    let mut parts: Vec<String> = pkg
        .part_names()
        .filter(|n| pkg.content_type(n).is_some_and(|ct| SHEET_CONTENT_TYPES.contains(&ct)))
        .map(str::to_owned)
        .collect();
    for rel in wb_rels.iter().filter(|r| SHEET_REL_TYPES.contains(&r.rel_type.as_str())) {
        if let Some(p) = wb_rels.resolve(rel)
            && pkg.contains(&p)
            && !parts.contains(&p)
        {
            parts.push(p);
        }
    }
    for part in parts {
        match scan::contains_sml_formula(&pkg.read_part(&part)?) {
            Ok(false) => {}
            Ok(true) => return Ok(Some(EditBlock::Formula { part })),
            Err(reason) => return Ok(Some(EditBlock::ScanFailed { part, reason })),
        }
    }
    Ok(None)
}

fn load_shared_strings(pkg: &mut Package, wb_rels: &Relationships) -> Result<Option<Vec<String>>> {
    let Some(part) = wb_rels.by_type(REL_SHARED_STRINGS).next().and_then(|r| wb_rels.resolve(r)) else {
        return Ok(None);
    };
    if !pkg.contains(&part) {
        return Ok(None);
    }
    let doc: &XmlDocument = pkg.xml_part(&part)?;
    let sst = sml::SharedStringTableView(doc.lens::<sml::SharedStringTable>().map_err(|e| strict_or(e, &part))?);
    Ok(Some(
        sst.shared_string_item_iter()
            .map(|si| text_of(si.text(), si.run_iter().map(|r| r.text())))
            .collect(),
    ))
}

fn load_tables(pkg: &mut Package, sheet_part: &str, rels: &Relationships, ids: &[String]) -> Result<Vec<TableHeader>> {
    let mut out = Vec::new();
    for id in ids {
        let Some(rel) = rels.by_id(id).filter(|r| r.rel_type == REL_TABLE) else {
            return Err(Error::Malformed(format!("{sheet_part}: tablePart {id} has no table relationship")));
        };
        let part = rels.resolve(rel).ok_or_else(|| Error::Malformed(format!("table {id} is external")))?;
        let doc = pkg.xml_part(&part)?;
        let t = sml::TableView(doc.lens::<sml::Table>().map_err(|e| strict_or(e, &part))?);
        let name = t.display_name().or(t.name()).unwrap_or(&part).to_owned();
        let range = CellRange::parse(t.reference().ok_or_else(|| Error::Malformed(format!("{part}: table without ref")))?)?;
        let n = t.header_row_count().map_err(ooxml_core::Error::from)?;
        let rows = (n > 0).then(|| {
            let mut r = range;
            r.last.row = (range.first.row + n - 1).min(range.last.row);
            r
        });
        out.push(TableHeader { name, rows });
    }
    Ok(out)
}

