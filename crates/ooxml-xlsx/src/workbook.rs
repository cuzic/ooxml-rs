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
const XML_WORKBOOK_TYPES: &[(&str, &str)] = &[
    ("xlsx", "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"),
    ("xlsm", "application/vnd.ms-excel.sheet.macroEnabled.main+xml"),
    ("xlam", "application/vnd.ms-excel.addin.macroEnabled.main+xml"),
];

pub(crate) const REL_SHARED_STRINGS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings";
pub(crate) const REL_CALC_CHAIN: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/calcChain";
pub(crate) const REL_TABLE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/table";
const REL_PIVOT_TABLE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/pivotTable";
const REL_QUERY_TABLE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/queryTable";
pub(crate) const REL_METADATA: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/sheetMetadata";
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
    pub(crate) part: String,
    pub(crate) rel: String,
    pub(crate) name: String,
    pub(crate) rows: Option<CellRange>,
    /// The whole table (`table/@ref`), for Phase4's overlap refusals.
    pub(crate) range: CellRange,
}

/// What a worksheet's guards need besides its tree, loaded when the sheet is
/// first opened for editing.
#[derive(Clone, Debug)]
pub(crate) struct SheetContext {
    /// The sheet's merged ranges; Phase4's `merge`/`unmerge` keep it up to
    /// date in the same apply step as the sheet edit (ADR-0014 §3).
    pub(crate) merges: Vec<CellRange>,
    pub(crate) tables: Vec<TableHeader>,
    /// The sheet's PivotTable parts; their `location/@ref` is read only
    /// when a merge needs it (ADR-0014 §3), so a damaged pivot part never
    /// stops a workbook from opening.
    pub(crate) pivots: Vec<String>,
    /// The target of a sheet-level query-table relationship, if any (its
    /// output range is not located, so overlapping features are refused).
    pub(crate) query_table: Option<String>,
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
    /// Before writing the (first) target cell.
    Target,
    /// Before writing the second cell of a multi-cell plan (the first one
    /// is already written: a partly applied plan).
    LaterTarget,
    /// Before `Package::apply` of a registry transaction (ADR-0003 addendum
    /// P5-pre).
    Registry,
    /// Not a failure point: changes the package between planning and
    /// `Package::apply`, so the registry transaction is stale (the plan's
    /// ordering bug ADR-0017 §2.1 describes). Used with
    /// `inject_apply_failure` only.
    StaleRegistryPlan,
    /// Before updating the formula index.
    Index,
    /// A structural operation (ADR-0009): before the registry plan is
    /// applied.
    StructuralApply,
    /// A structural operation: after apply, before the indexes are rebuilt.
    StructuralRebuild,
    /// A structural operation: inside the rebuild, after the package was
    /// re-read.
    StructuralReindex,
    /// Before a Phase4 range-feature edit (merge, autofilter, protection).
    RangeFeature,
    /// Before inserting the worksheet drawing reference.
    ImageDrawing,
    /// Before inserting the worksheet legacy drawing reference.
    NoteDrawing,
    /// Before recording object identifiers allocated or retired by a comment edit.
    CommentIndex,
    /// Before inserting the worksheet table reference.
    TableParts,
    /// Before inserting the worksheet sparkline group.
    Sparklines,
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
///
/// Sheet operations that strip dependent formula caches can return the
/// [wrapped-grid error described on `Worksheet`](Worksheet#wrapped-worksheet-grids).
#[derive(Debug)]
pub struct Workbook {
    pub(crate) pkg: Package,
    pub(crate) workbook_part: String,
    pub(crate) comment_guids: HashSet<String>,
    pub(crate) comment_guids_scanned: bool,
    pub(crate) vml_block_high: Option<u32>,
    #[cfg(test)]
    pub(crate) comment_random: Option<fn() -> [u8; 16]>,
    pub(crate) sheets: Vec<SheetEntry>,
    first_visible: Option<usize>,
    pub(crate) shared_strings: Option<Vec<String>>,
    /// Positions of the shared-string items in the table part, for reading
    /// an item's tree by index in O(1). Keyed by [`Package::generation`], so
    /// any package change (a registry append included) rebuilds it on the
    /// next read. See [`Self::shared_string_items`].
    shared_string_index: std::sync::Mutex<Option<SharedStringIndex>>,
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
    /// `style_sensitive()`'s answer, computed once and dropped whenever a
    /// formula is written or removed (PR #50 review M5).
    pub(crate) style_sensitive: Option<bool>,
    from_template: bool,
    template_sheet_taken: bool,
    /// `sheetId`s of sheets deleted in this session (never reused).
    pub(crate) removed_sheet_ids: Vec<u32>,
    pub(crate) table_high_water: u32,
    pub(crate) table_names: Option<HashMap<String, Vec<String>>>,
    pub(crate) objects: crate::images::ObjectState,
    #[cfg(feature = "fault-injection")]
    /// The step to fail at, and whether to panic instead of returning `Err`.
    pub(crate) fault: Option<(ApplyStep, bool)>,
}

impl Workbook {
    /// Copies the current editing state without reopening the ZIP archive.
    pub fn fork(&self) -> Self {
        Self {
            pkg: self.pkg.fork(),
            workbook_part: self.workbook_part.clone(),
            comment_guids: self.comment_guids.clone(),
            comment_guids_scanned: self.comment_guids_scanned,
            vml_block_high: self.vml_block_high,
            #[cfg(test)]
            comment_random: self.comment_random,
            sheets: self.sheets.clone(),
            first_visible: self.first_visible,
            shared_strings: self.shared_strings.clone(),
            shared_string_index: Default::default(),
            contexts: self.contexts.clone(),
            indexes: self.indexes.clone(),
            census: self.census.clone(),
            env: self.env.clone(),
            graph: self.graph.clone(),
            calc: self.calc,
            calc_chain: self.calc_chain.clone(),
            policy: self.policy,
            poisoned: self.poisoned.clone(),
            registries: self.registries.clone(),
            style_sensitive: self.style_sensitive,
            from_template: self.from_template,
            template_sheet_taken: self.template_sheet_taken,
            removed_sheet_ids: self.removed_sheet_ids.clone(),
            table_high_water: self.table_high_water,
            table_names: self.table_names.clone(),
            objects: self.objects.clone(),
            #[cfg(feature = "fault-injection")]
            fault: self.fault,
        }
    }


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
        Self::new_at(std::time::SystemTime::now())
    }

    /// Creates a template workbook with a caller-supplied creation time.
    pub fn new_at(time: std::time::SystemTime) -> Result<Self> {
        let mut wb = Self::from_bytes(TEMPLATE)?;
        wb.pkg.set_part("docProps/core.xml", core_props::generate(time))?;
        wb.from_template = true;
        Ok(wb)
    }

    /// Creates an editable, zero-sheet template workbook for adapters that allow empty XLSX files.
    pub fn new_empty_at(time: std::time::SystemTime) -> Result<Self> {
        let package = Self::new_at(time)?.package_without_initial_sheet()?;
        let mut workbook = Self::load_package(package, true)?;
        workbook.from_template = true;
        Ok(workbook)
    }

    /// Removes the empty shared string table from a template-based workbook.
    /// Returns false if the template's table has already been removed or used.
    pub fn prune_empty_template_shared_strings(&mut self) -> Result<bool> {
        self.check_editable()?;
        if !self.from_template || !self.shared_strings_active()? {
            return Ok(false);
        }
        let registry = self.registries.strings.as_ref().expect("activated");
        if !registry.is_empty() {
            return Ok(false);
        }
        let part = registry.part.clone();
        let mut plan = ooxml_core::RegistryPlan::new(&self.pkg);
        plan.remove_parts(&self.pkg, &[part.as_str()], &[REL_SHARED_STRINGS])?;
        self.apply_registry_edit(plan, |wb| {
            wb.registries.strings = None;
            wb.shared_strings = None;
            Ok(())
        })?;
        Ok(true)
    }

    /// Returns the XML-based workbook format declared by the package.
    pub fn xml_workbook_type(&self) -> Option<&'static str> {
        let content_type = self.pkg.content_type(&self.workbook_part)?;
        XML_WORKBOOK_TYPES.iter().find(|(_, mime)| *mime == content_type).map(|(name, _)| *name)
    }

    /// Selects an XML-based workbook type for a new template-based workbook.
    pub fn set_template_workbook_type(&mut self, format: &str) -> Result<()> {
        self.check_editable()?;
        if !self.from_template {
            return Err(Error::Malformed("workbook format change requires a new workbook".into()));
        }
        let (_, content_type) = XML_WORKBOOK_TYPES.iter().find(|(name, _)| *name == format)
            .ok_or_else(|| Error::Malformed(format!("unknown XML workbook type {format:?}")))?;
        if self.pkg.content_type(&self.workbook_part) != Some(content_type) {
            self.pkg.set_content_type(&self.workbook_part, content_type)?;
        }
        Ok(())
    }

    /// Wraps an opened package.
    pub fn from_package(pkg: Package) -> Result<Self> {
        Self::load_package(pkg, false)
    }

    /// Opens an XLSX that may have no sheets. A nonempty workbook still needs a visible worksheet.
    pub fn from_bytes_allow_empty(bytes: impl Into<std::sync::Arc<[u8]>>) -> Result<Self> {
        Self::load_package(Package::from_bytes(bytes)?, true)
    }

    fn load_package(mut pkg: Package, allow_empty: bool) -> Result<Self> {
        let l = Loaded::load(&mut pkg, allow_empty)?;
        let mut wb = Self {
            pkg,
            workbook_part: l.workbook_part,
            comment_guids: HashSet::new(),
            comment_guids_scanned: false,
            vml_block_high: None,
            #[cfg(test)]
            comment_random: None,
            sheets: l.sheets,
            first_visible: l.first_visible,
            shared_strings: l.shared_strings,
            shared_string_index: Default::default(),
            contexts: HashMap::new(),
            indexes: HashMap::new(),
            census: l.census,
            env: l.env,
            graph: None,
            calc: l.calc,
            calc_chain: l.calc_chain,
            policy: CalcPolicy::default(),
            poisoned: None,
            registries: Default::default(),
            style_sensitive: None,
            from_template: false,
            template_sheet_taken: false,
            removed_sheet_ids: Vec::new(),
            table_high_water: 0,
            table_names: None,
            objects: Default::default(),
            #[cfg(feature = "fault-injection")]
            fault: None,
        };
        if let Some(index) = wb.first_visible {
            wb.load_context(index)?;
        }
        Ok(wb)
    }

    /// Re-reads every index (sheet list, census, names, calculation state,
    /// shared strings) from the package, as `open` does (ADR-0009 §2, review
    /// R2-M2: rebuilt, never hand re-indexed). Used after a structural
    /// operation's plan is applied. A failure poisons the workbook: the
    /// package and the indexes may then disagree, so nothing more may be
    /// edited or saved (ADR-0003 §3.1).
    pub(crate) fn rebuild(&mut self) -> Result<()> {
        self.rebuild_with_empty(self.sheets.is_empty())
    }

    pub(crate) fn rebuild_with_empty(&mut self, allow_empty: bool) -> Result<()> {
        let r = Loaded::load(&mut self.pkg, allow_empty).and_then(|l| {
            self.fault_point(ApplyStep::StructuralReindex)?;
            self.workbook_part = l.workbook_part;
            self.sheets = l.sheets;
            self.first_visible = l.first_visible;
            self.shared_strings = l.shared_strings;
            self.census = l.census;
            self.env = l.env;
            self.calc = l.calc;
            self.calc_chain = l.calc_chain;
            self.contexts.clear();
            self.indexes.clear();
            self.graph = None;
            self.registries = Default::default();
            self.style_sensitive = None;
            self.first_visible.map_or(Ok(()), |index| self.load_context(index))
        });
        if let Err(e) = &r {
            self.poisoned = Some(format!("re-indexing after a structural edit failed: {e}"));
            return Err(Error::Poisoned(self.poisoned.clone().unwrap_or_default()));
        }
        Ok(())
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
        let pivots = sheet_rels.by_type(REL_PIVOT_TABLE).filter_map(|rel| sheet_rels.resolve(rel)).filter(|p| self.pkg.contains(p)).collect();
        let query_table = sheet_rels.by_type(REL_QUERY_TABLE).next().map(|r| r.target.clone());
        self.contexts.insert(sheet, SheetContext { merges, tables, pivots, query_table });
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
        let i = self.first_visible.ok_or(Error::NoVisibleWorksheet)?;
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

    pub(crate) fn sheet_index(&self, name: &str) -> Result<usize> {
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

    /// Name of the first visible worksheet, or an empty string for a zero-sheet workbook.
    pub fn sheet_name(&self) -> &str {
        self.first_visible.map_or("", |index| &self.sheets[index].name)
    }

    /// Part name of the first visible worksheet, or an empty string for a zero-sheet workbook.
    pub fn worksheet_part(&self) -> &str {
        self.first_visible.and_then(|index| self.sheets[index].part.as_deref()).unwrap_or("")
    }

    /// Part name of the workbook (e.g. `xl/workbook.xml`).
    pub fn workbook_part(&self) -> &str {
        &self.workbook_part
    }

    /// `Some(error)` if every edit of this workbook is refused: a sheet part
    /// could not be scanned for formulas, a sheet with formulas lacks `r`
    /// attributes, a cell's `r` names another row than its `row/@r`
    /// ([`Error::InvalidReference`]), the workbook has cell metadata Phase1
    /// does not model, or an earlier edit failed while applying
    /// ([`Error::Poisoned`]).
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

    /// The stored XML inside each requested shared-string item (`si`), with
    /// outer whitespace removed, as SheetJS reports it in a cell's `r`.
    /// An entry is `None` when the index is out of range, the item is empty,
    /// or the table was edited since it was read (its source no longer
    /// matches the tree). Uses the cached item positions of
    /// the crate-internal `shared_string_items` (no scan when they are current).
    pub fn shared_string_items_xml(&self, indices: &[u32]) -> Result<Vec<Option<String>>> {
        let items = self.shared_string_items()?.filter(|items| !items.document.is_dirty());
        let Some(items) = items else { return Ok(vec![None; indices.len()]) };
        let source = items.document.source();
        Ok(indices.iter().map(|&index| {
            let span = items.get(index)?.span()?;
            let (close, _) = span.close?;
            Some(source.get(span.open_end..close)?.trim()).filter(|xml| !xml.is_empty()).map(str::to_owned)
        }).collect())
    }

    /// The parsed shared-string table and O(1) access to its items by index,
    /// or `None` when the workbook has no parsed table part.
    ///
    /// The item positions are collected once per [`Package::generation`]
    /// and reused by every later read, so a sheet read resolves the
    /// relationship and scans the table once instead of once per cell
    /// (the per-cell scan made shared-string reads quadratic). Every package
    /// change bumps the generation (registry appends go through
    /// [`Package::xml_part_mut`]), which drops the positions. A missing
    /// table is not cached, so a part parsed later is still found.
    pub(crate) fn shared_string_items(&self) -> Result<Option<SharedStringItems<'_>>> {
        use ooxml_core::ElementSpec as _;
        let generation = self.pkg.generation();
        let mut cache = self.shared_string_index.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(index) = cache.as_ref().filter(|index| index.generation == generation)
            && let Some(document) = self.pkg.parsed_part(&index.part)
        {
            return Ok(Some(SharedStringItems { document, positions: index.positions.clone() }));
        }
        *cache = None;
        let relationships = self.pkg.relationships(&self.workbook_part)?;
        let Some(part) = relationships
            .by_type(REL_SHARED_STRINGS)
            .next()
            .and_then(|relationship| relationships.resolve(relationship))
        else {
            return Ok(None);
        };
        let Some(document) = self.pkg.parsed_part(&part) else { return Ok(None) };
        let positions: std::sync::Arc<[u32]> = document.root().indexed_elements()
            .filter(|(_, item)| item.is(sml::SharedStringItem::QNAME))
            .map(|(position, _)| position as u32)
            .collect();
        *cache = Some(SharedStringIndex { generation, part, positions: positions.clone() });
        Ok(Some(SharedStringItems { document, positions }))
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

    /// Serializes with an explicit ZIP compression policy, refusing poisoned workbooks.
    pub fn to_bytes_with_compression(&self, compressed: bool) -> Result<Vec<u8>> {
        self.check_poisoned()?;
        Ok(self.pkg.to_bytes_with_compression(compressed)?)
    }

    /// Recompresses the workbook using the specified DEFLATE level.
    pub fn to_bytes_with_deflate_level(&self, level: u8) -> Result<Vec<u8>> {
        self.check_poisoned()?;
        Ok(self.pkg.to_bytes_with_deflate_level(level)?)
    }

    /// Changes the string storage for this export without changing the open workbook.
    pub fn to_bytes_with_string_storage(
        &self, shared: bool, plain_as_value: bool, compression: Option<bool>, level: Option<u8>,
    ) -> Result<Vec<u8>> {
        self.check_poisoned()?;
        self.export_string_storage(&self.pkg, shared, plain_as_value, compression, level)
    }

    fn export_string_storage(
        &self, package: &Package, shared: bool, plain_as_value: bool, compression: Option<bool>, level: Option<u8>,
    ) -> Result<Vec<u8>> {
        let rewritten = if shared {
            crate::registry::strings::share(self, package)?
        } else {
            crate::registry::strings::unshare(self, package, plain_as_value)?
        };
        Self::serialize_export(rewritten.as_ref().unwrap_or(package), compression, level)
    }

    /// Exports cells without ordinary display styles, retaining date formats.
    pub fn to_bytes_without_cell_styles(
        &self, use_shared_strings: Option<bool>, compression: Option<bool>, level: Option<u8>,
    ) -> Result<Vec<u8>> {
        self.check_poisoned()?;
        let rewritten = crate::registry::styles::without_cell_styles(self)?;
        if let Some(shared) = use_shared_strings {
            return self.export_string_storage(
                rewritten.as_ref().unwrap_or(&self.pkg), shared, true, compression, level,
            );
        }
        Self::serialize_export(rewritten.as_ref().unwrap_or(&self.pkg), compression, level)
    }

    fn serialize_export(package: &Package, compression: Option<bool>, level: Option<u8>) -> Result<Vec<u8>> {
        Ok(match (compression, level) {
            (Some(false), _) => package.to_bytes_with_compression(false)?,
            (_, Some(level)) => package.to_bytes_with_deflate_level(level)?,
            (Some(true), _) => package.to_bytes_with_compression(true)?,
            (None, _) => package.to_bytes()?,
        })
    }

    pub(crate) fn check_editable(&self) -> Result<()> {
        self.edit_refusal().map_or(Ok(()), Err)
    }

    pub(crate) fn check_poisoned(&self) -> Result<()> {
        match &self.poisoned {
            Some(p) => Err(Error::Poisoned(p.clone())),
            None => Ok(()),
        }
    }

    /// Test hook (not API): a fingerprint of every index the workbook keeps
    /// (sheet list, formula census, defined names, calculation state), for
    /// the ADR-0009 oracle "rebuilt state == state of the reopened output".
    #[doc(hidden)]
    pub fn __index_fingerprint(&self) -> String {
        use std::fmt::Write as _;
        let mut out = String::new();
        let _ = writeln!(out, "workbook {} first_visible {} calc_chain {:?}", self.workbook_part, self.first_visible.map_or_else(|| "none".to_owned(), |index| index.to_string()), self.calc_chain);
        let _ = writeln!(out, "calc manual={} iterate={} full={}", self.calc.manual, self.calc.iterate, self.calc.full_calc_on_load);
        for s in &self.sheets {
            let _ = writeln!(out, "sheet {:?} {:?} {:?} visible={}", s.name, s.part, s.kind, s.visible);
        }
        let mut cells: Vec<String> = self
            .census
            .alive()
            .map(|(_, c)| format!("{} {} {:?} {:?} {:?} cache={} {:?}", c.sheet, c.at, c.kind, c.range, c.dynamic, c.has_cache, c.text))
            .collect();
        cells.sort();
        for c in cells {
            let _ = writeln!(out, "f {c}");
        }
        let mut names: Vec<String> = self.env.names.iter().map(|n| format!("{} {:?} {:?}", n.name_lc, n.scope, n.expr.is_some())).collect();
        names.sort();
        for n in names {
            let _ = writeln!(out, "name {n}");
        }
        let _ = writeln!(out, "block {:?} shared_strings {:?}", self.census.block.is_some(), self.shared_strings.as_ref().map(Vec::len));
        out
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

/// Everything [`Workbook`] derives from its package.
struct Loaded {
    workbook_part: String,
    sheets: Vec<SheetEntry>,
    first_visible: Option<usize>,
    shared_strings: Option<Vec<String>>,
    census: Census,
    env: Env,
    calc: CalcState,
    calc_chain: Option<String>,
}

impl Loaded {
    fn load(pkg: &mut Package, allow_empty: bool) -> Result<Loaded> {
        let workbook_part = pkg.main_document_part().ok_or(Error::NoWorkbook)?;
        let wb_rels = pkg.relationships(&workbook_part)?;
        let (sheets, names, calc) = read_workbook(pkg, &workbook_part, &wb_rels, allow_empty)?;
        let first_visible = sheets
            .iter()
            .position(|s| s.visible && s.kind == SheetKind::Worksheet);
        if first_visible.is_none() && !(allow_empty && sheets.is_empty()) {
            return Err(Error::NoVisibleWorksheet);
        }
        let calc_chain = wb_rels
            .by_type(REL_CALC_CHAIN)
            .next()
            .and_then(|r| wb_rels.resolve(r))
            .filter(|p| pkg.contains(p));
        let census = formula_census(pkg, &wb_rels, &sheets)?;
        let shared_strings = load_shared_strings(pkg, &wb_rels)?;
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
        Ok(Loaded { workbook_part, sheets, first_visible, shared_strings, census, env, calc, calc_chain })
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

pub(crate) fn is_sheet_relationship(kind: &str) -> bool {
    matches!(kind, REL_WORKSHEET | REL_CHARTSHEET | REL_DIALOGSHEET) || REL_MACROSHEETS.contains(&kind)
}

type Names = Vec<(String, Option<u32>, String)>;

/// `<sheets>` (with each sheet's kind, ADR-0002 §6 + owner decision of
/// 2026-11-16 for "first visible"), defined names and `calcPr`.
fn read_workbook(pkg: &mut Package, workbook_part: &str, rels: &Relationships, allow_empty: bool) -> Result<(Vec<SheetEntry>, Names, CalcState)> {
    let mut raw = Vec::new();
    let mut names = Vec::new();
    let mut calc = CalcState::default();
    {
        let doc = pkg.xml_part(workbook_part)?;
        let wb = sml::WorkbookView(doc.lens::<sml::Workbook>().map_err(|e| strict_or(e, workbook_part))?);
        let sheets = wb.sheets();
        if sheets.is_none() && !allow_empty {
            return Err(Error::Malformed(format!("{workbook_part} has no <sheets>")));
        }
        let mut listed = sheets.iter().flat_map(|sheets| sheets.sheet_iter()).peekable();
        if allow_empty && listed.peek().is_none() && rels.iter().any(|rel| is_sheet_relationship(&rel.rel_type)) {
            return Err(Error::Malformed(format!("{workbook_part} has sheet relationships but no listed sheets")));
        }
        for s in listed {
            let (Some(name), Some(id)) = (s.name(), s.id()) else {
                return Err(Error::Malformed(format!("{workbook_part}: <sheet> without name or r:id")));
            };
            let visible = matches!(s.state(), EnumValue::Known(SheetStateValues::Visible));
            // ST_Xstring: the name as Excel shows it (PR #53 review R2-m4)
            let sheet_id = s.sheet_id().ok().flatten();
            raw.push((crate::escape::decode(name).into_owned(), id.to_owned(), sheet_id, visible));
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
    for (name, id, sheet_id, visible) in raw {
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
        sheets.push(SheetEntry { name, sheet_id, part, kind, visible });
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
        let scanned = match pkg.parsed_part(part).filter(|doc| doc.is_dirty()) {
            Some(doc) => census::scan_parsed_sheet(doc.root(), part, i, &dynamic, &mut census),
            None => census::scan_sheet(&pkg.read_part(part)?, part, i, &dynamic, &mut census),
        };
        if let Err(reason) = scanned {
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

/// The cached item positions behind [`Workbook::shared_string_items`].
#[derive(Debug)]
struct SharedStringIndex {
    generation: u64,
    part: String,
    /// Child position (in the root's `children()`) of each `si`, in order.
    positions: std::sync::Arc<[u32]>,
}

/// The shared-string table of one package generation (see
/// [`Workbook::shared_string_items`]).
pub(crate) struct SharedStringItems<'a> {
    pub(crate) document: &'a XmlDocument,
    positions: std::sync::Arc<[u32]>,
}

impl<'a> SharedStringItems<'a> {
    /// The `index`-th `si` element, `None` when out of range.
    pub(crate) fn get(&self, index: u32) -> Option<&'a ooxml_core::RawNode> {
        let position = *self.positions.get(index as usize)?;
        self.document.root().children().get(position as usize)?.as_element()
    }
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
            r.last.row = range.first.row.saturating_add(n - 1).min(range.last.row);
            r
        });
        out.push(TableHeader { part, rel: id.clone(), name, rows, range });
    }
    Ok(out)
}


impl Workbook {
    /// Names of grid worksheets in workbook order, including hidden worksheets.
    pub fn worksheet_names(&self) -> impl Iterator<Item = &str> {
        self.worksheet_identities().map(|(name, _)| name)
    }

    /// Grid worksheet names and their declared OOXML IDs in workbook order.
    pub fn worksheet_identities(&self) -> impl Iterator<Item = (&str, Option<u32>)> {
        self.sheets
            .iter()
            .filter(|sheet| sheet.kind == SheetKind::Worksheet)
            .map(|sheet| (sheet.name.as_str(), sheet.sheet_id))
    }
}

#[cfg(test)]
mod open_performance {
    use super::*;
    use std::{sync::Arc, time::Instant};

    #[test]
    #[ignore = "timing (set OOXML_PERF_WORKBOOK to an XLSX fixture)"]
    fn formula_census_and_sheet_index() {
        let path = std::env::var_os("OOXML_PERF_WORKBOOK").expect("set OOXML_PERF_WORKBOOK");
        let bytes: Arc<[u8]> = std::fs::read(path).unwrap().into();
        let mut census_times = Vec::new();
        let mut index_times = Vec::new();
        for trial in 0..7 {
            let mut pkg = Package::from_bytes(bytes.clone()).unwrap();
            let workbook = pkg.main_document_part().unwrap();
            let relationships = pkg.relationships(&workbook).unwrap();
            let (sheets, _, _) = read_workbook(&mut pkg, &workbook, &relationships, false).unwrap();
            let start = Instant::now();
            let census = formula_census(&pkg, &relationships, &sheets).unwrap();
            let census_elapsed = start.elapsed();
            std::hint::black_box(census);

            let visible = sheets.iter().find(|sheet| sheet.visible && sheet.kind == SheetKind::Worksheet).unwrap();
            let doc = pkg.xml_part(visible.part.as_deref().unwrap()).unwrap();
            let start = Instant::now();
            let index = SheetIndex::build(doc);
            let index_elapsed = start.elapsed();
            std::hint::black_box(index);
            if trial >= 2 {
                census_times.push(census_elapsed);
                index_times.push(index_elapsed);
            }
        }
        for (label, times) in [("formula census", &mut census_times), ("first visible worksheet index", &mut index_times)] {
            times.sort_unstable();
            println!("{label}: {:?}", times[times.len() / 2]);
        }
    }
}

#[cfg(test)]
mod shared_string_markup {
    use super::*;

    #[test]
    fn items_keep_stored_markup_and_cells_report_their_index() {
        let template = Workbook::new().unwrap();
        let sheet_part = template.worksheet_part().to_owned();
        let mut pkg = Package::from_bytes(template.to_bytes().unwrap()).unwrap();
        let ns = r#"xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main""#;
        let items = concat!(
            "<si><t>a &amp; b</t></si>",
            "<si>\n <t>p</t><rPh sb=\"0\" eb=\"1\"><t>ピ</t></rPh><phoneticPr fontId=\"1\"/>\n</si>",
            "<si></si>",
            "<si><r><rPr><b/></rPr><t>R</t></r></si>",
        );
        pkg.set_part("xl/sharedStrings.xml", format!("<sst {ns}>{items}</sst>").into_bytes()).unwrap();
        let cells = r#"<row r="1"><c r="A1" t="s"><v>1</v></c><c r="B1" t="inlineStr"><is><t>i</t></is></c><c r="C1"><v>2</v></c></row>"#;
        pkg.set_part(&sheet_part, format!("<worksheet {ns}><sheetData>{cells}</sheetData></worksheet>").into_bytes()).unwrap();
        let mut wb = Workbook::from_bytes(pkg.to_bytes().unwrap()).unwrap();
        assert_eq!(wb.shared_string_items_xml(&[0, 1, 2, 3, 9]).unwrap(), vec![
            Some("<t>a &amp; b</t>".to_owned()),
            Some("<t>p</t><rPh sb=\"0\" eb=\"1\"><t>ピ</t></rPh><phoneticPr fontId=\"1\"/>".to_owned()),
            None,
            Some("<r><rPr><b/></rPr><t>R</t></r>".to_owned()),
            None,
        ]);
        let sheet = wb.worksheet().unwrap();
        let index = |column| sheet.cell(0, column).unwrap().unwrap().shared_string_index();
        assert_eq!((index(0), index(1), index(2)), (Some(1), None, None));
    }

    /// The item positions are built once per package generation, reused by
    /// later reads, and rebuilt after an edit appends shared-string items
    /// (a stale index would miss the new items or misreport their runs).
    #[test]
    fn item_index_is_reused_and_rebuilt_after_shared_string_edits() {
        use std::sync::Arc;
        let template = Workbook::new().unwrap();
        let sheet_part = template.worksheet_part().to_owned();
        let mut pkg = Package::from_bytes(template.to_bytes().unwrap()).unwrap();
        let ns = r#"xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main""#;
        let items = "<si><t>a</t></si>\n<si><r><rPr><b/></rPr><t>R</t></r></si>";
        pkg.set_part("xl/sharedStrings.xml", format!("<sst {ns}>{items}</sst>").into_bytes()).unwrap();
        let cells = r#"<row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c></row>"#;
        pkg.set_part(&sheet_part, format!("<worksheet {ns}><sheetData>{cells}</sheetData></worksheet>").into_bytes()).unwrap();
        let mut wb = Workbook::from_bytes(pkg.to_bytes().unwrap()).unwrap();

        let first = wb.shared_string_items().unwrap().unwrap().positions;
        // Text between items is not an item: positions are child positions.
        assert_eq!(&*first, &[0, 2]);
        assert!(Arc::ptr_eq(&first, &wb.shared_string_items().unwrap().unwrap().positions));
        {
            let sheet = wb.worksheet().unwrap();
            let complex = |column| sheet.cell(0, column).unwrap().unwrap().has_non_scalar_content().unwrap();
            assert_eq!((complex(0), complex(1)), (false, true));
        }
        assert!(Arc::ptr_eq(&first, &wb.shared_string_items().unwrap().unwrap().positions));

        let bold = crate::Font { bold: true, ..crate::Font::default() };
        {
            let mut sheet = wb.worksheet().unwrap();
            sheet.cell_mut(1, 0).unwrap().set_rich_text(&[(Some(&bold), "X"), (None, "y")]).unwrap();
            sheet.cell_mut(1, 1).unwrap().set_string("new").unwrap();
            sheet.cell_mut(0, 1).unwrap().set_string("a").unwrap();
        }
        let rebuilt = wb.shared_string_items().unwrap().unwrap().positions;
        assert!(!Arc::ptr_eq(&first, &rebuilt));
        assert_eq!(rebuilt.len(), wb.shared_strings.as_ref().unwrap().len());
        let check = |wb: &mut Workbook| {
            let sheet = wb.worksheet().unwrap();
            let cell = |row, column| sheet.cell(row, column).unwrap().unwrap();
            assert_eq!(cell(1, 0).shared_string_index(), Some(2));
            assert!(cell(1, 0).has_non_scalar_content().unwrap());
            assert_eq!(cell(1, 0).rich_text().unwrap().iter().map(|run| run.0.as_str()).collect::<String>(), "Xy");
            assert_eq!(cell(1, 1).shared_string_index(), Some(3));
            assert!(!cell(1, 1).has_non_scalar_content().unwrap());
            assert_eq!(cell(1, 1).value().unwrap(), crate::CellValue::String("new".into()));
            // B1 now reuses the plain item 0 instead of the rich item 1.
            assert_eq!(cell(0, 1).shared_string_index(), Some(0));
            assert!(!cell(0, 1).has_non_scalar_content().unwrap());
        };
        check(&mut wb);
        // An edited table no longer matches its source: no stored markup.
        assert_eq!(wb.shared_string_items_xml(&[1, 2]).unwrap(), vec![None, None]);
        let mut reloaded = Workbook::from_bytes(wb.to_bytes().unwrap()).unwrap();
        check(&mut reloaded);
        assert_eq!(reloaded.shared_string_items_xml(&[1, 3]).unwrap(), vec![
            Some("<r><rPr><b/></rPr><t>R</t></r>".to_owned()),
            Some("<t>new</t>".to_owned()),
        ]);
    }
    /// Phonetic guide data (`rPh`, `phoneticPr`) on a string without rich
    /// runs does not make the value complex (ExcelJS 4.4.0 reads the base
    /// text); rich runs still do, and the SheetJS view still sees the markup.
    /// Editing a neighbour, or overwriting the cell, keeps the phonetic item.
    #[test]
    fn phonetic_strings_read_as_their_base_text() {
        let template = Workbook::new().unwrap();
        let sheet_part = template.worksheet_part().to_owned();
        let mut pkg = Package::from_bytes(template.to_bytes().unwrap()).unwrap();
        let ns = r#"xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main""#;
        let plain = r#"<si><t>漢字</t><rPh sb="0" eb="2"><t>カンジ</t></rPh><phoneticPr fontId="1"/></si>"#;
        let items = format!(
            "{plain}<si><t>abc</t><phoneticPr fontId=\"1\"/></si><si><r><t>太</t></r><rPh sb=\"0\" eb=\"1\"><t>フト</t></rPh></si><si><t>p</t></si>"
        );
        pkg.set_part("xl/sharedStrings.xml", format!("<sst {ns}>{items}</sst>").into_bytes()).unwrap();
        let cells = concat!(
            r#"<row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c><c r="C1" t="s"><v>2</v></c><c r="D1" t="s"><v>3</v></c>"#,
            r#"<c r="E1" t="inlineStr"><is><t>漢字</t><rPh sb="0" eb="2"><t>カンジ</t></rPh></is></c>"#,
            r#"<c r="F1" t="inlineStr"><is><r><t>x</t></r><phoneticPr fontId="1"/></is></c><c r="G1" t="s"><f>A1</f><v>0</v></c></row>"#,
        );
        pkg.set_part(&sheet_part, format!("<worksheet {ns}><sheetData>{cells}</sheetData></worksheet>").into_bytes()).unwrap();
        let mut wb = Workbook::from_bytes(pkg.to_bytes().unwrap()).unwrap();
        {
            let sheet = wb.worksheet().unwrap();
            let cell = |column| sheet.cell(0, column).unwrap().unwrap();
            assert_eq!((0..7).map(|column| cell(column).has_complex_value(false).unwrap()).collect::<Vec<_>>(),
                [false, false, true, false, false, true, true]);
            // The SheetJS and structural classification is unchanged.
            assert_eq!((0..7).map(|column| cell(column).has_non_scalar_content().unwrap()).collect::<Vec<_>>(),
                [true, true, true, false, true, true, true]);
            assert_eq!(cell(0).value().unwrap(), crate::CellValue::String("漢字".into()));
            assert_eq!(cell(4).value().unwrap(), crate::CellValue::String("漢字".into()));
            assert_eq!(cell(2).rich_text().unwrap().len(), 1);
        }
        {
            let mut sheet = wb.worksheet().unwrap();
            sheet.cell_mut(1, 0).unwrap().set_number(1.0).unwrap();
            sheet.cell_mut(0, 1).unwrap().set_string("y").unwrap();
        }
        let bytes = wb.to_bytes().unwrap();
        let sst = String::from_utf8(Package::from_bytes(bytes.clone()).unwrap().read_part("xl/sharedStrings.xml").unwrap()).unwrap();
        assert!(sst.contains(plain), "{sst}");
        let mut saved = Workbook::from_bytes(bytes).unwrap();
        let sheet = saved.worksheet().unwrap();
        assert_eq!(sheet.cell(0, 0).unwrap().unwrap().shared_string_index(), Some(0));
        assert_eq!(sheet.cell(0, 1).unwrap().unwrap().value().unwrap(), crate::CellValue::String("y".into()));
    }
}
