//! [`Workbook`]: open/new/save, the sheet list, the per-sheet contexts that
//! guards need (merges, table headers), the formula census and the
//! calculation state (ADR-0001 Phase0, ADR-0003 Phase1).

use std::collections::{HashMap, HashSet};
use std::path::Path;

use ooxml_core::opc::Relationships;
use ooxml_core::{EnumValue, Package, XmlDocument};
use ooxml_schema::sml::{self, CalculateModeValues, SheetStateValues};

use crate::census::{self, Census, EditBlock, SheetEntry, SheetKind};
use crate::error::{Error, Result};
use crate::formula::precedents::{Env, NameDef, Scope, fold};
use crate::graph::Graph;
use crate::index::SheetIndex;
use crate::reference::{CellIndex, CellRange};
use crate::worksheet::{Worksheet, text_of};
use crate::core_props;

pub(crate) const REL_WORKSHEET: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet";
const REL_CHARTSHEET: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/chartsheet";
pub(crate) const REL_SHARED_STRINGS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings";
pub(crate) const REL_CALC_CHAIN: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/calcChain";
const REL_TABLE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/table";
const REL_METADATA: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/sheetMetadata";
const CT_WORKSHEET: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml";
const CT_MACROSHEETS: &[&str] = &["application/vnd.ms-excel.macrosheet+xml", "application/vnd.ms-excel.intlmacrosheet+xml"];
const CT_DIALOGSHEET: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.dialogsheet+xml";
const REL_MACROSHEETS: &[&str] = &[
    "http://schemas.microsoft.com/office/2006/relationships/xlMacrosheet",
    "http://schemas.microsoft.com/office/2006/relationships/xlIntlMacrosheet",
];
const REL_DIALOGSHEET: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/dialogsheet";
const STRICT_NS_PREFIX: &str = "http://purl.oclc.org/ooxml/";

/// The embedded from-scratch template (ADR-0001 "From-scratch path"); its
/// sources are `template/blank/`, built by `template/build.py`.
pub(crate) const TEMPLATE: &[u8] = include_bytes!("../template/blank.xlsx");

/// A table on a sheet: its header rows (empty if `headerRowCount="0"`).
#[derive(Clone, Debug)]
pub(crate) struct TableHeader {
    pub(crate) name: String,
    pub(crate) rows: Option<CellRange>,
}

/// What a worksheet's guards need besides its tree, loaded when the sheet is
/// first opened for editing.
#[derive(Debug)]
pub(crate) struct SheetContext {
    pub(crate) merges: Vec<CellRange>,
    pub(crate) tables: Vec<TableHeader>,
}

/// What happens to the cached values of dependents (ADR-0003 §3.3).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum CalcPolicy {
    /// Remove `v`/`t` from every dependent (the default): no reader ever
    /// sees a stale cell-formula value.
    #[default]
    StripDependentCaches,
    /// Keep the dependents' stale cached values and only set
    /// `fullCalcOnLoad`. **Excel only**: LibreOffice, calamine and pandas
    /// show the stale values.
    KeepStaleCaches,
}

/// `calcPr` as opened (and as changed by edits).
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct CalcState {
    pub(crate) manual: bool,
    pub(crate) iterate: bool,
    pub(crate) full_calc_on_load: bool,
}

/// A step of an edit's apply phase, for fault injection (ADR-0003 §7.3;
/// used only with the `fault-injection` feature).
#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ApplyStep {
    /// Before removing `calcChain.xml`.
    RemoveCalcChain,
    /// Before installing the edited `workbook.xml`.
    CalcPr,
    /// Before removing dependents' cached values.
    Strip,
    /// Before the shared-formula hand-off.
    HandOff,
    /// Before installing registry additions (`styles.xml`, `sharedStrings.xml`).
    Registries,
    /// Before writing the target cell.
    Target,
    /// Before updating the formula index.
    Index,
}

pub(crate) type ApplyStepHook = ApplyStep;

/// An `.xlsx`/`.xlsm` workbook opened for lossless editing.
///
/// Phase0's "sheet1" is the **first visible worksheet in workbook order**
/// ([`Self::worksheet`]); Phase1 opens any worksheet by name or index.
/// Every part that is not edited is copied through unchanged on save.
///
/// # Examples
///
/// ```no_run
/// use ooxml_xlsx::{CellValue, Error, Workbook};
///
/// let mut wb = Workbook::open("book.xlsx")?;
/// if let Some(why) = wb.edit_refusal() {
///     return Err(why); // e.g. Error::Poisoned: nothing was changed
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
    pub(crate) pkg: Package,
    pub(crate) workbook_part: String,
    pub(crate) sheets: Vec<SheetEntry>,
    first_visible: usize,
    pub(crate) shared_strings: Option<Vec<String>>,
    pub(crate) contexts: HashMap<usize, SheetContext>,
    /// Row/cell position index per worksheet (built once, kept valid by
    /// every structural write; see `index.rs`).
    pub(crate) indexes: HashMap<usize, SheetIndex>,
    pub(crate) census: Census,
    pub(crate) env: Env,
    pub(crate) graph: Option<Graph>,
    pub(crate) calc: CalcState,
    pub(crate) calc_chain: Option<String>,
    pub(crate) policy: CalcPolicy,
    pub(crate) poisoned: Option<String>,
    /// Phase2 registries (ADR-0006), activated by the first write that
    /// needs them.
    pub(crate) registries: crate::registry::Registries,
    from_template: bool,
    template_sheet_taken: bool,
    #[cfg(feature = "fault-injection")]
    /// The step to fail at, and whether to panic instead of returning `Err`.
    pub(crate) fault: Option<(ApplyStep, bool)>,
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
        let (sheets, names, calc) = read_workbook(&mut pkg, &workbook_part, &wb_rels)?;
        let first_visible = sheets
            .iter()
            .position(|s| s.visible && s.kind == SheetKind::Worksheet)
            .ok_or(Error::NoVisibleWorksheet)?;
        let calc_chain = wb_rels
            .by_type(REL_CALC_CHAIN)
            .next()
            .and_then(|r| wb_rels.resolve(r))
            .filter(|p| pkg.contains(p));
        let census = formula_census(&pkg, &wb_rels, &sheets)?;
        let shared_strings = load_shared_strings(&mut pkg, &wb_rels)?;
        let env = Env {
            sheets_lc: sheets.iter().map(|s| fold(&s.name)).collect(),
            names: names
                .into_iter()
                .map(|(name, local, text)| {
                    let scope = match local {
                        None => Scope::Global,
                        Some(i) if (i as usize) < sheets.len() => Scope::Local(i as usize),
                        Some(_) => Scope::Invalid,
                    };
                    NameDef::new(&name, scope, &text)
                })
                .collect(),
            bx_any: census.bx_any,
        };
        let mut wb = Self {
            pkg,
            workbook_part,
            sheets,
            first_visible,
            shared_strings,
            contexts: HashMap::new(),
            indexes: HashMap::new(),
            census,
            env,
            graph: None,
            calc,
            calc_chain,
            policy: CalcPolicy::default(),
            poisoned: None,
            registries: Default::default(),
            from_template: false,
            template_sheet_taken: false,
            #[cfg(feature = "fault-injection")]
            fault: None,
        };
        wb.load_context(first_visible)?;
        Ok(wb)
    }

    /// Parses a worksheet (once) and loads its merges and table headers.
    pub(crate) fn load_context(&mut self, sheet: usize) -> Result<()> {
        if self.contexts.contains_key(&sheet) {
            return Ok(());
        }
        let part = self.sheets[sheet].part.clone().expect("worksheet part");
        let sheet_rels = self.pkg.relationships(&part)?;
        let doc = self.pkg.xml_part(&part)?;
        let ws = sml::WorksheetView(doc.lens::<sml::Worksheet>().map_err(|e| strict_or(e, &part))?);
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
        let tables = load_tables(&mut self.pkg, &part, &sheet_rels, &table_parts)?;
        self.contexts.insert(sheet, SheetContext { merges, tables });
        self.ensure_index(sheet)?;
        Ok(())
    }

    /// Parses a worksheet (once) and builds its row/cell index (once).
    pub(crate) fn ensure_index(&mut self, sheet: usize) -> Result<()> {
        if !self.indexes.contains_key(&sheet) {
            let part = self.sheets[sheet].part.clone().expect("worksheet part");
            let index = SheetIndex::build(self.pkg.xml_part(&part)?);
            self.indexes.insert(sheet, index);
        }
        Ok(())
    }

    /// "sheet1": the first visible worksheet in workbook order.
    pub fn worksheet(&mut self) -> Result<Worksheet<'_>> {
        let i = self.first_visible;
        self.worksheet_at(i)
    }

    /// A worksheet by name (case-insensitive, as Excel compares sheet names).
    /// Hidden sheets are included. Errors: [`Error::NoSuchSheet`];
    /// [`Error::NotAWorksheet`] for a chartsheet, macro or dialog sheet.
    ///
    /// An exact match wins; otherwise the name must match exactly one sheet
    /// case-insensitively (two sheets whose names fold alike, which Excel
    /// itself never writes, make the folded lookup ambiguous: `NoSuchSheet`).
    pub fn worksheet_by_name(&mut self, name: &str) -> Result<Worksheet<'_>> {
        let i = self.sheet_index(name)?;
        self.worksheet_at(i)
    }

    fn sheet_index(&self, name: &str) -> Result<usize> {
        if let Some(i) = self.sheets.iter().position(|s| s.name == name) {
            return Ok(i);
        }
        let f = fold(name);
        let mut it = self.sheets.iter().enumerate().filter(|(_, s)| fold(&s.name) == f).map(|(i, _)| i);
        match (it.next(), it.next()) {
            (Some(i), None) => Ok(i),
            (Some(_), Some(_)) => Err(Error::NoSuchSheet(format!("{name} (ambiguous: several sheet names fold alike)"))),
            _ => Err(Error::NoSuchSheet(name.to_owned())),
        }
    }

    /// A worksheet by 0-based position in `<sheets>` (workbook order, hidden
    /// sheets, chartsheets, macro and dialog sheets included in the count).
    /// Errors: [`Error::NoSuchSheet`], [`Error::NotAWorksheet`].
    pub fn worksheet_by_index(&mut self, index: usize) -> Result<Worksheet<'_>> {
        if index >= self.sheets.len() {
            return Err(Error::NoSuchSheet(format!("#{index}")));
        }
        self.worksheet_at(index)
    }

    fn worksheet_at(&mut self, i: usize) -> Result<Worksheet<'_>> {
        if self.sheets[i].kind != SheetKind::Worksheet {
            return Err(Error::NotAWorksheet(self.sheets[i].name.clone()));
        }
        self.load_context(i)?;
        Ok(Worksheet::new(self, i))
    }

    /// Sheet names in workbook order (every kind of sheet).
    pub fn sheet_names(&self) -> impl Iterator<Item = &str> {
        self.sheets.iter().map(|s| s.name.as_str())
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

    /// Always [`Error::SheetNamingUnsupported`] (naming a sheet touches
    /// `workbook.xml` and `docProps/app.xml` together).
    pub fn add_worksheet_with_name(&mut self, _name: &str) -> Result<Worksheet<'_>> {
        Err(Error::SheetNamingUnsupported)
    }

    /// Name of "sheet1" (the `name` attribute of its `<sheet>`).
    pub fn sheet_name(&self) -> &str {
        &self.sheets[self.first_visible].name
    }

    /// Part name of "sheet1" (e.g. `xl/worksheets/sheet1.xml`).
    pub fn worksheet_part(&self) -> &str {
        self.sheets[self.first_visible].part.as_deref().expect("worksheet")
    }

    /// Part name of the workbook (e.g. `xl/workbook.xml`).
    pub fn workbook_part(&self) -> &str {
        &self.workbook_part
    }

    /// `Some(error)` if every edit of this workbook is refused: a sheet part
    /// could not be scanned for formulas, a sheet with formulas lacks `r`
    /// attributes, the workbook has cell metadata Phase1 does not model, or
    /// an earlier edit failed while applying ([`Error::Poisoned`]).
    pub fn edit_refusal(&self) -> Option<Error> {
        if let Some(p) = &self.poisoned {
            return Some(Error::Poisoned(p.clone()));
        }
        self.census.block.as_ref().map(EditBlock::to_error)
    }

    /// What happens to dependents' cached values on edits.
    pub fn calc_policy(&self) -> CalcPolicy {
        self.policy
    }

    /// Sets [`CalcPolicy`] for later edits.
    pub fn set_calc_policy(&mut self, policy: CalcPolicy) {
        self.policy = policy;
    }

    /// The cells whose cached values an edit of `sheet!cell` would remove
    /// (ADR-0003 §3.2): dependent formula cells and the result cells of
    /// dependent array, data-table and spill anchors, as (sheet name, cell),
    /// sorted. The cell itself is not included.
    pub fn dependents(&mut self, sheet: &str, cell: CellIndex) -> Result<Vec<(String, CellIndex)>> {
        let s = self.sheet_index(sheet)?;
        let d = self.compute_dependents(s, cell);
        let mut out: Vec<(String, CellIndex)> = d
            .cells
            .into_iter()
            .filter(|&(sh, at)| (sh, at) != (s, cell))
            .map(|(sh, at)| (self.sheets[sh].name.clone(), at))
            .collect();
        out.sort_by(|a, b| (&a.0, a.1).cmp(&(&b.0, b.1)));
        out.dedup();
        Ok(out)
    }

    /// The underlying package (part states, content types, relationships),
    /// for inspection.
    ///
    /// **Do not save through it** when [`Self::edit_refusal`] reports
    /// [`Error::Poisoned`]: the package may then hold a partial edit, and
    /// `Package::save`/`to_bytes` do not know about poisoning. Use
    /// [`Self::save`]/[`Self::to_bytes`], which refuse.
    pub fn package(&self) -> &Package {
        &self.pkg
    }

    /// Writes the workbook to `path` atomically (temporary file + rename).
    /// [`Error::Poisoned`] after a failed apply.
    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        self.check_poisoned()?;
        Ok(self.pkg.save(path)?)
    }

    /// Serializes the workbook. [`Error::Poisoned`] after a failed apply.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        self.check_poisoned()?;
        Ok(self.pkg.to_bytes()?)
    }

    pub(crate) fn check_poisoned(&self) -> Result<()> {
        match &self.poisoned {
            Some(p) => Err(Error::Poisoned(p.clone())),
            None => Ok(()),
        }
    }

    /// Test hook: make the next edit fail just before `step` of its apply
    /// phase (ADR-0003 §7.3). Only with the `fault-injection` feature.
    #[cfg(feature = "fault-injection")]
    #[doc(hidden)]
    pub fn inject_apply_failure(&mut self, step: ApplyStep) {
        self.fault = Some((step, false));
    }

    /// Test hook: make the next edit **panic** just before `step` of its
    /// apply phase (ADR-0003 §7.3, review R4-m6: a panic must leave the
    /// workbook poisoned). Only with the `fault-injection` feature.
    #[cfg(feature = "fault-injection")]
    #[doc(hidden)]
    pub fn inject_apply_panic(&mut self, step: ApplyStep) {
        self.fault = Some((step, true));
    }
}

/// Maps a wrong-root error to `StrictNotSupported` when the root is in a
/// Strict namespace.
pub(crate) fn strict_or(e: ooxml_core::Error, part: &str) -> Error {
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

type Names = Vec<(String, Option<u32>, String)>;

/// `<sheets>` (with each sheet's kind, ADR-0002 §6 + owner decision of
/// 2026-11-16 for "first visible"), defined names and `calcPr`.
fn read_workbook(pkg: &mut Package, workbook_part: &str, rels: &Relationships) -> Result<(Vec<SheetEntry>, Names, CalcState)> {
    let mut raw = Vec::new();
    let mut names = Vec::new();
    let mut calc = CalcState::default();
    {
        let doc = pkg.xml_part(workbook_part)?;
        let wb = sml::WorkbookView(doc.lens::<sml::Workbook>().map_err(|e| strict_or(e, workbook_part))?);
        let sheets = wb.sheets().ok_or_else(|| Error::Malformed(format!("{workbook_part} has no <sheets>")))?;
        for s in sheets.sheet_iter() {
            let (Some(name), Some(id)) = (s.name(), s.id()) else {
                return Err(Error::Malformed(format!("{workbook_part}: <sheet> without name or r:id")));
            };
            let visible = matches!(s.state(), EnumValue::Known(SheetStateValues::Visible));
            raw.push((name.to_owned(), id.to_owned(), visible));
        }
        if let Some(dn) = wb.defined_names() {
            for d in dn.defined_name_iter() {
                let Some(name) = d.name() else { continue };
                let local = d.local_sheet_id().map_err(ooxml_core::Error::from)?;
                names.push((name.to_owned(), local, d.0.text().into_owned()));
            }
        }
        if let Some(c) = wb.calculation_properties() {
            calc.manual = matches!(c.calculation_mode(), EnumValue::Known(CalculateModeValues::Manual));
            calc.iterate = c.iterate().unwrap_or(true); // unreadable: assume the cautious value
            calc.full_calc_on_load = c.full_calculation_on_load().unwrap_or(false);
        }
    }
    let mut sheets = Vec::new();
    for (name, id, visible) in raw {
        let rel = rels.by_id(&id).ok_or_else(|| Error::Malformed(format!("sheet {name:?}: relationship {id} not found")))?;
        let part = rels.resolve(rel).filter(|p| pkg.contains(p));
        let ct = part.as_deref().and_then(|p| pkg.content_type(p));
        let kind = match (rel.rel_type.as_str(), ct) {
            _ if part.is_none() => SheetKind::Other,
            (REL_WORKSHEET, Some(CT_WORKSHEET)) => SheetKind::Worksheet,
            (REL_CHARTSHEET, _) => SheetKind::Chartsheet,
            (t, _) if REL_MACROSHEETS.contains(&t) => SheetKind::MacroSheet,
            (_, Some(c)) if CT_MACROSHEETS.contains(&c) => SheetKind::MacroSheet,
            (REL_DIALOGSHEET, _) | (_, Some(CT_DIALOGSHEET)) => SheetKind::DialogSheet,
            _ => SheetKind::Other,
        };
        sheets.push(SheetEntry { name, part, kind, visible });
    }
    Ok((sheets, names, calc))
}

/// ADR-0003 §1: scan every sheet part by element name. Sheet parts not
/// listed in `<sheets>` that contain formulas block edits (their formulas
/// could not be placed in the dependency graph).
fn formula_census(pkg: &Package, wb_rels: &Relationships, sheets: &[SheetEntry]) -> Result<Census> {
    let mut census = Census::default();
    let dynamic = match wb_rels.by_type(REL_METADATA).next().and_then(|r| wb_rels.resolve(r)).filter(|p| pkg.contains(p)) {
        Some(p) => match census::dynamic_array_cms(&pkg.read_part(&p)?) {
            Ok(set) => set,
            Err(reason) => {
                census.block = Some(EditBlock::ScanFailed { part: p, reason });
                HashSet::new()
            }
        },
        None => HashSet::new(),
    };
    let mut listed = HashSet::new();
    for (i, s) in sheets.iter().enumerate() {
        let Some(part) = &s.part else { continue };
        if !matches!(s.kind, SheetKind::Worksheet | SheetKind::MacroSheet | SheetKind::DialogSheet) {
            continue;
        }
        listed.insert(part.clone());
        if let Err(reason) = census::scan_sheet(&pkg.read_part(part)?, part, i, &dynamic, &mut census) {
            census.block.get_or_insert(EditBlock::ScanFailed { part: part.clone(), reason });
        }
    }
    // Orphan sheet parts (by content type, or by workbook relationship type),
    // as Phase0 scanned them.
    let all_cts: Vec<&str> = [CT_WORKSHEET, CT_DIALOGSHEET].into_iter().chain(CT_MACROSHEETS.iter().copied()).collect();
    let all_rels: Vec<&str> = [REL_WORKSHEET, REL_DIALOGSHEET].into_iter().chain(REL_MACROSHEETS.iter().copied()).collect();
    let mut orphans: Vec<String> = pkg
        .part_names()
        .filter(|n| !listed.contains(*n) && pkg.content_type(n).is_some_and(|ct| all_cts.contains(&ct)))
        .map(str::to_owned)
        .collect();
    for rel in wb_rels.iter().filter(|r| all_rels.contains(&r.rel_type.as_str())) {
        if let Some(p) = wb_rels.resolve(rel)
            && pkg.contains(&p)
            && !listed.contains(&p)
            && !orphans.contains(&p)
        {
            orphans.push(p);
        }
    }
    for part in orphans {
        let mut scratch = Census::default();
        match census::scan_sheet(&pkg.read_part(&part)?, &part, usize::MAX, &dynamic, &mut scratch) {
            Ok(()) if scratch.cells.is_empty() => {}
            Ok(()) => {
                census.block.get_or_insert(EditBlock::ScanFailed {
                    part,
                    reason: "a sheet part with formulas that <sheets> does not list".into(),
                });
            }
            Err(reason) => {
                census.block.get_or_insert(EditBlock::ScanFailed { part, reason });
            }
        }
    }
    Ok(census)
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

