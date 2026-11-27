//! SpreadsheetML tables and synchronized header writes (ADR-0017 §6).

mod references;

use std::collections::{HashMap, HashSet};
use std::fmt::NumBuffer;

use ooxml_core::{ElementSpec, NsDecl, PartNameRequest, QName, RegistryPlan, Target, XmlDocument};
use ooxml_schema::sml;
use sml::Table as TableElement;

use crate::census::FKind;
use crate::edit::{CellOp, CellWritePlan, Extra};
use crate::escape;
use crate::formula::precedents::fold;
use crate::hyperlink::TableLinks;
use crate::registry::styles::StyleRequest;
use crate::workbook::{ApplyStepHook, REL_TABLE, TableHeader};
use crate::{CellIndex, CellRange, CellValue, CellWrite, Error, Result, Workbook, Worksheet};

const SML: &str = crate::SML_NS;
const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const RID: QName = QName::new(R, "id");
const REL_NS: [NsDecl<'static>; 1] = [NsDecl::new("r", R)];
const TABLE_TYPE: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.table+xml";

fn attr(local: &'static str) -> QName {
    QName::new("", local)
}

fn range_error(reason: &'static str) -> Error {
    Error::TableRange { reason }
}

/// Options for creating a worksheet table.
#[derive(Clone, Debug)]
pub struct Table {
    range: CellRange,
    name: Option<String>,
    display_name: Option<String>,
    columns: Option<Vec<Col>>,
    differential_formats: [Option<u32>; 3],
    border_formats: [Option<u32>; 3],
    headers: bool,
    totals: bool,
    overwrite: bool,
    write_cells: bool,
    style: TableStyle,
    row_stripes: bool,
    column_stripes: bool,
    first_column: bool,
    last_column: bool,
}

impl Table {
    /// Creates a table with a header and banded rows.
    pub fn new(range: CellRange) -> Self {
        Self {
            range,
            name: None,
            display_name: None,
            columns: None,
            differential_formats: [None; 3],
            border_formats: [None; 3],
            headers: true,
            totals: false,
            overwrite: false,
            write_cells: true,
            style: TableStyle::Medium9,
            row_stripes: true,
            column_stripes: false,
            first_column: false,
            last_column: false,
        }
    }

    /// Sets workbook-local differential format IDs for the header, data, and totals.
    pub fn differential_formats(mut self, ids: [Option<u32>; 3]) -> Self {
        self.differential_formats = ids;
        self
    }

    /// Sets workbook-local differential border IDs for the header, table, and totals.
    pub fn border_formats(mut self, ids: [Option<u32>; 3]) -> Self {
        self.border_formats = ids;
        self
    }

    /// Sets the workbook-wide table name.
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Sets the workbook-wide display name used by structured references.
    pub fn display_name(mut self, name: impl Into<String>) -> Self {
        self.display_name = Some(name.into());
        self
    }

    /// Supplies one definition per column.
    pub fn columns(mut self, columns: impl IntoIterator<Item = Col>) -> Self {
        self.columns = Some(columns.into_iter().collect());
        self
    }

    /// Controls whether the first row holds headers.
    pub fn header_row(mut self, enabled: bool) -> Self {
        self.headers = enabled;
        self
    }

    /// Controls whether the last row holds totals.
    pub fn totals_row(mut self, enabled: bool) -> Self {
        self.totals = enabled;
        self
    }

    /// Allows replacement of content in header, totals and calculated-column cells.
    pub fn overwrite(mut self, enabled: bool) -> Self {
        self.overwrite = enabled;
        self
    }

    /// Attaches table metadata to a range whose cells are supplied separately.
    pub fn metadata_only(mut self) -> Self {
        self.write_cells = false;
        self
    }

    /// Selects the table style.
    pub fn style(mut self, style: TableStyle) -> Self {
        self.style = style;
        self
    }

    /// Controls row banding.
    pub fn row_stripes(mut self, enabled: bool) -> Self {
        self.row_stripes = enabled;
        self
    }

    /// Controls column banding.
    pub fn column_stripes(mut self, enabled: bool) -> Self {
        self.column_stripes = enabled;
        self
    }
    /// Applies emphasis to the first and last columns independently.
    pub fn emphasize_columns(mut self, first: bool, last: bool) -> Self {
        self.first_column = first;
        self.last_column = last;
        self
    }

}

/// A built-in table style from Excel's Light, Medium or Dark family.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableStyle {
    /// The default TableStyleMedium9.
    Medium9,
    /// TableStyleLight1 through TableStyleLight21.
    Light(u8),
    /// TableStyleMedium1 through TableStyleMedium28.
    Medium(u8),
    /// TableStyleDark1 through TableStyleDark11.
    Dark(u8),
    /// No named style.
    None,
}

impl TableStyle {
    /// Parses a built-in table style name; an empty name disables the style.
    pub fn parse(name: &str) -> Result<Self> {
        if name.is_empty() {
            return Ok(Self::None);
        }
        for (prefix, constructor) in [
            ("TableStyleLight", Self::Light as fn(u8) -> Self),
            ("TableStyleMedium", Self::Medium),
            ("TableStyleDark", Self::Dark),
        ] {
            if let Some(index) = name.strip_prefix(prefix).and_then(|value| value.parse().ok()) {
                let style = constructor(index);
                style.name()?;
                return Ok(style);
            }
        }
        Err(Error::InvalidSetting { what: "table style".into(), reason: format!("unknown style {name:?}") })
    }

    fn name(self) -> Result<Option<String>> {
        let (family, index, max) = match self {
            Self::Medium9 => ("Medium", 9, 28),
            Self::Light(i) => ("Light", i, 21),
            Self::Medium(i) => ("Medium", i, 28),
            Self::Dark(i) => ("Dark", i, 11),
            Self::None => return Ok(None),
        };
        if !(1..=max).contains(&index) {
            return Err(Error::InvalidSetting {
                what: "table style".into(),
                reason: format!("invalid {family} style index {index}"),
            });
        }
        Ok(Some(format!("TableStyle{family}{index}")))
    }
}

/// The subtotal function for a table column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Total {
    /// Sum.
    Sum,
    /// Arithmetic mean.
    Average,
    /// Count nonempty values.
    Count,
    /// Count numeric values.
    CountNums,
    /// Maximum.
    Max,
    /// Minimum.
    Min,
    /// Sample standard deviation.
    StdDev,
    /// Sample variance.
    Var,
}

impl Total {
    fn file_form(self) -> (&'static str, u32) {
        match self {
            Self::Sum => ("sum", 109),
            Self::Average => ("average", 101),
            Self::Count => ("count", 103),
            Self::CountNums => ("countNums", 102),
            Self::Max => ("max", 104),
            Self::Min => ("min", 105),
            Self::StdDev => ("stdDev", 107),
            Self::Var => ("var", 110),
        }
    }
}

/// A label, built-in function, or formula in a table totals cell.
#[derive(Clone, Debug)]
pub enum ColumnTotal {
    Function(Total),
    Label(String),
    Formula(String),
}

/// Definition of one table column.
#[derive(Clone, Debug)]
pub struct Col {
    name: String,
    total: Option<ColumnTotal>,
    explicit_no_total_function: bool,
    formula: Option<String>,
    filter_button: bool,
    differential_formats: [Option<u32>; 3],
}

impl Col {
    /// Sets the heading and structured-reference name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            total: None,
            explicit_no_total_function: false,
            formula: None,
            filter_button: true,
            differential_formats: [None; 3],
        }
    }

    /// Sets workbook-local differential format IDs for the header, data, and totals.
    pub fn differential_formats(mut self, ids: [Option<u32>; 3]) -> Self {
        self.differential_formats = ids;
        self
    }

    /// Chooses whether to show this column's filter control.
    pub fn filter_button(mut self, visible: bool) -> Self {
        self.filter_button = visible;
        self
    }

    /// Sets a totals-row function for this column.
    pub fn total(mut self, function: Total) -> Self {
        self.total = Some(ColumnTotal::Function(function));
        self.explicit_no_total_function = false;
        self
    }

    /// Records an explicit `none` totals function alongside an optional label.
    pub fn no_total_function(mut self) -> Self {
        if !matches!(self.total, Some(ColumnTotal::Label(_))) {
            self.total = None;
        }
        self.explicit_no_total_function = true;
        self
    }

    /// Sets a text label in the totals row.
    pub fn total_label(mut self, label: impl Into<String>) -> Self {
        self.total = Some(ColumnTotal::Label(label.into()));
        self
    }

    /// Sets a formula for every data row. Local row references such as
    /// Writes a custom formula in this column's totals cell and table metadata.
    pub fn total_formula(mut self, formula: impl Into<String>) -> Self {
        self.total = Some(ColumnTotal::Formula(formula.into()));
        self.explicit_no_total_function = false;
        self
    }

    /// `[@Amount]` are converted to the file form.
    pub fn formula(mut self, formula: impl Into<String>) -> Self {
        self.formula = Some(formula.into());
        self
    }
}

pub(crate) type Write = (usize, CellIndex, CellOp, Extra);

struct ShiftedTableCell {
    at: CellIndex,
    value: CellValue,
    formula: Option<String>,
    style: Option<u32>,
}

/// The header renames a plan stages: cloned table docs, owned by the plan.
#[derive(Debug)]
pub(crate) struct StagedTables {
    docs: HashMap<String, XmlDocument>,
}

impl StagedTables {
    /// Whether the plan staged no header rename: an R1 skip must leave
    /// the tables alone too (ADR-0024 §7.1 R1(c)), so a non-empty stage
    /// vetoes the skip even when the cell report is all-same.
    pub(crate) fn is_empty(&self) -> bool {
        self.docs.is_empty()
    }
}

fn column_name(name: &str) -> Result<String> {
    if name.is_empty()
        || name
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\t' | '\n' | '\r'))
    {
        return Err(range_error(
            "empty column name or disallowed control character",
        ));
    }
    Ok(escape::encode(name)
        .replace('\n', "_x000a_")
        .replace('\r', "_x000d_"))
}

fn valid_name(name: &str) -> Result<()> {
    let mut chars = name.chars();
    let valid = chars
        .next()
        .is_some_and(|c| c.is_alphabetic() || matches!(c, '_' | '\\'))
        && chars.all(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | '\\'));
    if !valid || name.encode_utf16().count() > 255 {
        return Err(Error::InvalidTableName {
            reason: "invalid characters or length",
        });
    }
    let key = fold(name);
    if reference_like_name(name)
        || matches!(key.as_str(), "true" | "false")
        || key.starts_with("_xlnm.")
        || matches!(name.as_bytes(), [b'\\', c] if c.is_ascii_alphanumeric())
    {
        return Err(Error::InvalidTableName {
            reason: "reserved name or cell reference",
        });
    }
    Ok(())
}

fn reference_like_name(name: &str) -> bool {
    let split = name.bytes().take_while(u8::is_ascii_alphabetic).count();
    let (letters, rest) = name.split_at(split);
    let digits = rest.trim_start_matches('0');
    if (1..=3).contains(&letters.len())
        && !digits.is_empty()
        && digits.bytes().all(|b| b.is_ascii_digit())
        && CellIndex::parse(&format!("{letters}{digits}")).is_ok()
    {
        return true;
    }
    let max = match letters.to_ascii_uppercase().as_str() {
        "R" => crate::MAX_ROWS,
        "C" | "RC" => crate::MAX_COLS,
        _ => return false,
    };
    if rest.is_empty() {
        return true;
    }
    // Excel reserves a valid row/column number after R, C or RC even
    // when a suffix follows that numeric prefix.
    rest[..rest.bytes().take_while(u8::is_ascii_digit).count()]
        .parse::<u32>()
        .is_ok_and(|n| (1..=max).contains(&n))
}

fn descendants(node: &ooxml_core::RawNode) -> impl Iterator<Item = &ooxml_core::RawNode> {
    let mut stack = vec![node];
    std::iter::from_fn(move || {
        let next = stack.pop()?;
        stack.extend(next.elements());
        Some(next)
    })
}

impl Workbook {
    fn load_table_names(&mut self) -> Result<()> {
        if self.table_names.is_some() {
            return Ok(());
        }
        let mut max = self.table_high_water;
        let mut names = HashMap::new();
        let parts: Vec<_> = self
            .pkg
            .part_names()
            .filter_map(|p| {
                let ct = self.pkg.content_type(p)?;
                (ct == TABLE_TYPE || ct.ends_with("slicerCache+xml"))
                    .then(|| (p.to_owned(), ct == TABLE_TYPE))
            })
            .collect();
        for (part, table) in parts {
            let doc = self.pkg.xml_part(&part)?;
            if table {
                let root = doc.lens::<TableElement>()?;
                max = max.max(table_id(&root)?);
                names.insert(part, table_names(&root));
            } else {
                let ids = descendants(doc.root())
                    .filter(|n| {
                        n.is(QName::new(
                            "http://schemas.microsoft.com/office/spreadsheetml/2010/11/main",
                            "tableSlicerCache",
                        ))
                    })
                    .filter_map(|n| n.attr(attr("tableId")))
                    .filter_map(|id| id.parse::<u32>().ok());
                max = max.max(ids.max().unwrap_or_default());
            }
        }
        self.table_high_water = max;
        self.table_names = Some(names);
        Ok(())
    }

    pub(crate) fn forget_removed_tables(&mut self, parts: &[String], max_removed: u32) {
        self.table_high_water = self.table_high_water.max(max_removed);
        if let Some(names) = &mut self.table_names {
            for part in parts {
                names.remove(part);
            }
        }
    }

    pub(crate) fn removed_table_max(&self, parts: &[String]) -> u32 {
        parts
            .iter()
            .filter(|p| self.pkg.content_type(p) == Some(TABLE_TYPE))
            .filter_map(|p| {
                let bytes = self.pkg.read_part(p).ok()?;
                let doc = XmlDocument::parse_with(&bytes, &self.pkg.parse_options()).ok()?;
                table_id(&doc.lens::<TableElement>().ok()?).ok()
            })
            .max()
            .unwrap_or_default()
    }

    /// Plans cell writes with header staging (C1's plan entry for the
    /// `write_with_headers` funnel): renames staged on cloned table docs,
    /// cells planned without applying. The caller applies both
    /// ([`apply_with_headers`](Self::apply_with_headers)).
    pub(crate) fn plan_with_headers(
        &mut self,
        writes: Vec<Write>,
    ) -> Result<(StagedTables, CellWritePlan)> {
        self.check_editable()?;
        let mut staged = HashMap::new();
        let mut headers = HashSet::new();
        for (sheet, at, op, extra) in &writes {
            let text = match op {
                CellOp::Text(text) => escape::decode(text),
                CellOp::Shared(_) if let Some(string) = &extra.string => {
                    string.decoded.as_str().into()
                }
                _ => continue,
            };
            let Some(table) = self.contexts[sheet]
                .tables
                .iter()
                .find(|t| t.rows.is_some_and(|r| r.contains(*at)))
                .cloned()
            else {
                continue;
            };
            if !headers.insert((*sheet, *at)) {
                return Err(Error::DuplicateCellWrite {
                    cell: at.to_string(),
                });
            }
            let refused = || Error::TableHeaderCell {
                cell: at.to_string(),
                table: table.name.clone(),
            };
            if text.is_empty()
                || table
                    .rows
                    .is_some_and(|rows| rows.first.row != rows.last.row)
            {
                return Err(refused());
            }
            let doc = self.pkg.xml_part(&table.part)?;
            let root = doc.lens::<TableElement>()?;
            if !worksheet_table(&root) {
                return Err(refused());
            }
            let id = table_id(&root)?;
            let names = table_names(&root);
            let col = (at.col - table.range.first.col) as usize;
            let original = root
                .child::<sml::TableColumns>()
                .and_then(|cols| {
                    cols.children::<sml::TableColumn>()
                        .nth(col)?
                        .attr(attr("name"))
                })
                .ok_or_else(refused)?;
            let old = escape::decode(original).into_owned();
            let encoded = column_name(&text)?;
            if self.table_referenced(&table, &names, id, references::Use::Source, &[])? {
                return Err(refused());
            }
            if old == text {
                continue;
            }
            let usage = references::Use::Column(&old);
            if writes.iter().any(|(target_sheet, target, op, _)| {
                matches!(op, CellOp::Formula(formula, _) if references::has_reference(
                    formula, target_sheet == sheet && table.range.contains(*target), &names, usage,
                ))
            }) || self.table_referenced(&table, &names, id, usage, &writes)? {
                return Err(Error::ColumnInUse(old));
            }
            let doc = match staged.entry(table.part.clone()) {
                std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(self.pkg.xml_part(&table.part)?.clone())
                }
            };
            let mut root = doc.lens_mut::<TableElement>()?;
            let mut columns = root.child_mut::<sml::TableColumns>().ok_or_else(refused)?;
            let mut index = 0;
            let mut target = columns
                .child_mut_by::<sml::TableColumn>(|_| {
                    let found = index == col;
                    index += 1;
                    found
                })
                .ok_or_else(refused)?;
            target.set_attr(attr("name"), &encoded)?;
        }
        for doc in staged.values() {
            let root = doc.lens::<TableElement>()?;
            if let Some(cols) = root.child::<sml::TableColumns>() {
                check_unique(
                    cols.children::<sml::TableColumn>()
                        .filter_map(|c| c.attr(attr("name")))
                        .map(escape::decode),
                )?;
            }
        }
        let plan = self.plan_writes_after(None, writes, &headers, Default::default())?;
        Ok((StagedTables { docs: staged }, plan))
    }

    /// Applies staged header renames and a cell-write plan (the tail of
    /// `write_with_headers`).
    pub(crate) fn apply_with_headers(
        &mut self,
        staged: StagedTables,
        plan: CellWritePlan,
    ) -> Result<()> {
        if staged.docs.is_empty() {
            return self.apply_writes(plan);
        }
        let mut registry = RegistryPlan::new(&self.pkg);
        for (part, doc) in staged.docs {
            registry.stage_part(&self.pkg, &part, doc)?;
        }
        self.apply_writes_with(plan, registry)
    }

    pub(crate) fn write_with_headers(&mut self, writes: Vec<Write>) -> Result<()> {
        let (staged, plan) = self.plan_with_headers(writes)?;
        self.apply_with_headers(staged, plan)
    }
}

fn table_id(root: &ooxml_core::Lens<'_, TableElement>) -> Result<u32> {
    root.attr(attr("id"))
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| Error::Malformed("table without a valid id".into()))
}

fn table_names(root: &ooxml_core::Lens<'_, TableElement>) -> Vec<String> {
    ["name", "displayName"]
        .into_iter()
        .filter_map(|n| root.attr(attr(n)))
        .map(fold)
        .collect()
}

fn worksheet_table(root: &ooxml_core::Lens<'_, TableElement>) -> bool {
    root.attr(attr("tableType"))
        .is_none_or(|t| t == "worksheet")
}

fn table_totals_rows(root: &ooxml_core::Lens<'_, TableElement>) -> Result<u32> {
    match root.attr(attr("totalsRowCount")) {
        Some("0") => Ok(0),
        Some("1") => Ok(1),
        None if !root.attr(attr("totalsRowShown")).is_some_and(|shown| matches!(shown, "1" | "true")) => Ok(0),
        _ => Err(range_error("unsupported totals row count")),
    }
}

fn check_unique<T: AsRef<str>>(names: impl IntoIterator<Item = T>) -> Result<()> {
    let mut seen = HashSet::new();
    for name in names {
        if !seen.insert(fold(name.as_ref())) {
            return Err(Error::DuplicateColumnName(name.as_ref().to_owned()));
        }
    }
    Ok(())
}

impl Worksheet<'_> {
    /// Creates a table and returns its workbook-wide name.
    pub fn add_table(&mut self, mut table: Table) -> Result<String> {
        self.wb.check_editable()?;
        self.check_table_range(&table)?;
        self.wb.load_table_names()?;
        let names = self.wb.table_names.as_ref().expect("table names loaded");
        let in_use = |name: &str| {
            let key = fold(name);
            names.values().flatten().any(|existing| *existing == key) || self.wb.env.is_name(name)
        };
        let id = self
            .wb
            .table_high_water
            .checked_add(1)
            .ok_or_else(|| range_error("table id exhausted"))?;
        let name = match table.name.take() {
            Some(name) => {
                valid_name(&name)?;
                if in_use(&name) {
                    return Err(Error::NameInUse {
                        name,
                        cell: "table or defined name".into(),
                    });
                }
                name
            }
            None => (1..)
                .map(|n| format!("Table{n}"))
                .find(|n| !in_use(n))
                .expect("finite names"),
        };
        let display_name = table.display_name.as_deref().unwrap_or(&name);
        valid_name(display_name)?;
        if fold(display_name) != fold(&name) && in_use(display_name) {
            return Err(Error::NameInUse {
                name: display_name.into(),
                cell: "table or defined name".into(),
            });
        }
        let aliases = vec![fold(&name), fold(display_name)];
        let range = table.range;
        let first_data = range.first.row + u32::from(table.headers);
        let last_data = range.last.row - u32::from(table.totals);
        let width = (range.last.col - range.first.col + 1) as usize;
        let mut columns = match table.columns.take() {
            Some(columns) if columns.len() == width => columns,
            Some(_) => return Err(range_error("column count differs from range width")),
            None => (range.first.col..=range.last.col)
                .map(|col| {
                    let index = CellIndex {
                        row: range.first.row,
                        col,
                    };
                    let default = || Col::new(format!("Column{}", col - range.first.col + 1));
                    if table.headers
                        && let Some(cell) = self.cell(index.row, index.col)?
                    {
                        return match cell.value()? {
                            CellValue::String(s) if cell.formula().is_none() && !s.is_empty() => {
                                Ok(Col::new(s))
                            }
                            CellValue::Empty if cell.formula().is_none() => Ok(default()),
                            CellValue::String(s) if s.is_empty() && cell.formula().is_none() => {
                                Ok(default())
                            }
                            _ => Err(Error::HeaderNotText(index.to_string())),
                        };
                    }
                    Ok(default())
                })
                .collect::<Result<Vec<_>>>()?,
        };
        let last_style_id = table.differential_formats.into_iter()
            .chain(table.border_formats)
            .chain(columns.iter().flat_map(|column| column.differential_formats))
            .flatten()
            .max();
        if let Some(id) = last_style_id
            && id >= self.wb.dxf_count()?
        {
            return Err(range_error("table differential format ID does not exist"));
        }
        check_unique(columns.iter().map(|c| &c.name))?;
        let column_names = columns.iter().map(|col| fold(&col.name)).collect();
        let style = table.style.name()?;
        let mut writes = Vec::new();
        for (i, col) in columns.iter_mut().enumerate() {
            column_name(&col.name)?;
            let c = range.first.col + i as u32;
            if table.headers && table.write_cells {
                self.plan_table_cell(
                    &mut writes,
                    CellIndex {
                        row: range.first.row,
                        col: c,
                    },
                    Some(&col.name),
                    None,
                    table.overwrite,
                    true,
                )?;
            }
            if let Some(total) = &mut col.total {
                if !table.totals && table.write_cells {
                    return Err(range_error("column total requires a totals row"));
                }
                if let ColumnTotal::Formula(formula) = total {
                    *formula = references::calculated(formula, display_name, &column_names)?;
                }
                let at = CellIndex {
                    row: range.last.row,
                    col: c,
                };
                match total {
                    _ if !table.write_cells => {}
                    ColumnTotal::Label(label) => self.plan_table_cell(
                        &mut writes,
                        at,
                        Some(label),
                        None,
                        table.overwrite,
                        false,
                    )?,
                    ColumnTotal::Formula(formula) => {
                        self.plan_table_cell(&mut writes, at, None, Some(formula), table.overwrite, false)?;
                    }
                    ColumnTotal::Function(total) => {
                        let (_, code) = total.file_form();
                        let spec = references::specifier(&col.name, false);
                        let formula = format!("SUBTOTAL({code},{display_name}{spec})");
                        self.plan_table_cell(
                            &mut writes,
                            at,
                            None,
                            Some(&formula),
                            table.overwrite,
                            false,
                        )?;
                    }
                }
            }
            if let Some(formula) = &mut col.formula {
                *formula = references::calculated(formula, display_name, &column_names)?;
                if !table.write_cells {
                    continue;
                }
                for row in first_data..=last_data {
                    self.plan_table_cell(
                        &mut writes,
                        CellIndex { row, col: c },
                        None,
                        Some(formula),
                        table.overwrite,
                        false,
                    )?;
                }
            }
        }
        let sheet_part = self.part_name().to_owned();
        let wb = &mut *self.wb;
        {
            let mut sheet = wb
                .pkg
                .xml_part_mut(&sheet_part)?
                .lens_mut::<sml::Worksheet>()?;
            if let Some(parts) = sheet.child_mut::<sml::TableParts>() {
                parts.check_insert_with_ns::<sml::TablePart>(&REL_NS)?;
            } else {
                sheet.check_insert::<sml::TableParts>()?;
            }
        }
        let doc = table_document(&table, &name, display_name, id, style.as_deref(), &columns)?;
        let cell_plan = wb.plan_table_writes(writes, &HashSet::new(), true)?;
        let mut registry = RegistryPlan::new(&wb.pkg);
        let part = registry.add_part(
            &wb.pkg,
            PartNameRequest::Numbered("xl/tables/table{}.xml".into()),
            TABLE_TYPE,
            format!("<table xmlns=\"{SML}\"/>").into_bytes(),
        )?;
        registry.stage_part(&wb.pkg, &part, doc)?;
        let rel = registry.add_relationship(
            &wb.pkg,
            Some(&sheet_part),
            REL_TABLE,
            Target::Part(part.clone()),
        )?;
        let sheet_index = self.sheet;
        wb.apply_writes_with_insertion(cell_plan, registry, None, |wb| {
            wb.fault_point(ApplyStepHook::TableParts)?;
            let mut sheet = wb
                .pkg
                .xml_part_mut(&sheet_part)?
                .lens_mut::<sml::Worksheet>()?;
            let mut parts = sheet.get_or_insert::<sml::TableParts>()?;
            parts
                .insert_with_ns::<sml::TablePart>(&REL_NS)?
                .set_attr(RID, &rel)?;
            update_count(&mut parts)?;
            let rows = table.headers.then_some(CellRange {
                first: range.first,
                last: CellIndex {
                    row: range.first.row,
                    col: range.last.col,
                },
            });
            wb.table_names
                .as_mut()
                .expect("table names loaded")
                .insert(part.clone(), aliases);
            wb.contexts
                .get_mut(&sheet_index)
                .expect("worksheet context")
                .tables
                .push(TableHeader {
                    name: name.clone(),
                    part,
                    rel,
                    rows,
                    range,
                });
            wb.table_high_water = id;
            Ok(())
        })?;
        Ok(name)
    }

    fn plan_table_cell(
        &mut self,
        writes: &mut Vec<Write>,
        at: CellIndex,
        text: Option<&str>,
        formula: Option<&str>,
        overwrite: bool,
        header: bool,
    ) -> Result<()> {
        if !overwrite && let Some(cell) = self.cell(at.row, at.col)? {
            let value = cell.value()?;
            if header
                && cell.formula().is_none()
                && matches!((&value, text), (CellValue::String(s), Some(t)) if s == t)
            {
                return Ok(());
            }
            if cell.raw().elements().any(|n| {
                n.is(sml::CellFormula::QNAME)
                    || n.is(sml::CellValue::QNAME)
                    || n.is(sml::InlineString::QNAME)
            }) {
                return Err(range_error("cell not empty"));
            }
        }
        let (op, extra) = match text {
            Some(text) => self.wb.plan_string_write(text)?,
            None => (
                CellOp::Formula(formula.expect("formula supplied").into(), None),
                Extra::default(),
            ),
        };
        writes.push((self.sheet, at, op, extra));
        Ok(())
    }

    fn check_table_range(&mut self, table: &Table) -> Result<()> {
        let r = table.range;
        if CellIndex::new(r.last.row, r.last.col).is_err()
            || r.first.col > r.last.col
            || r.first.row > r.last.row
        {
            return Err(range_error("range outside sheet or reversed"));
        }
        if r.last.row - r.first.row < u32::from(table.totals) {
            return Err(range_error("table requires a header or data row before the totals row"));
        }
        self.check_table_footprint(r)
    }

    fn check_table_footprint(&mut self, r: CellRange) -> Result<()> {
        let part = self.part_name().to_owned();
        let ctx = &self.wb.contexts[&self.sheet];
        if ctx.tables.iter().any(|t| r.intersects(&t.range))
            || ctx.merges.iter().any(|m| r.intersects(m))
        {
            return Err(range_error("overlaps a table or merged range"));
        }
        if ctx.query_table.is_some() {
            return Err(range_error("sheet has an unlocated query table"));
        }
        for p in ctx.pivots.clone() {
            let doc = self.wb.pkg.xml_part(&p)?;
            let location = doc
                .root()
                .elements()
                .find(|e| e.is(QName::new(SML, "location")));
            if location
                .and_then(|n| n.attr(attr("ref")))
                .and_then(|a| CellRange::parse(a).ok())
                .is_none_or(|loc| loc.intersects(&r))
            {
                return Err(range_error(
                    "overlaps a pivot table or its range is unknown",
                ));
            }
        }
        let doc = self.wb.pkg.xml_part(&part)?;
        let ws = doc.lens::<sml::Worksheet>()?;
        if ws.children::<sml::AutoFilter>().any(|f| {
            f.attr(attr("ref"))
                .and_then(|s| CellRange::parse(s).ok())
                .is_none_or(|f| f.intersects(&r))
        }) {
            return Err(range_error(
                "overlaps a sheet autofilter or its range is unknown",
            ));
        }
        if let Some(data) = ws.child::<sml::SheetData>() {
            for row in data.children::<sml::Row>() {
                for c in row
                    .children::<sml::Cell>()
                    .filter(|c| c.attr(attr("vm")).is_some() || c.attr(attr("cm")).is_some())
                {
                    if c.attr(attr("r"))
                        .and_then(|s| CellIndex::parse(s).ok())
                        .is_none_or(|at| r.contains(at))
                    {
                        return Err(range_error("contains a cell with metadata"));
                    }
                }
            }
        }
        for (_, f) in self
            .wb
            .census
            .alive()
            .filter(|(_, f)| f.sheet == self.sheet)
        {
            if matches!(f.kind, FKind::Shared { master: true, .. }) && r.contains(f.at) {
                return Err(range_error("contains a shared-formula master"));
            }
            if matches!(f.kind, FKind::Array | FKind::DataTable) || f.dynamic {
                let area = f.result_range().unwrap_or(CellRange {
                    first: f.at,
                    last: f.at,
                });
                if area.intersects(&r) && !(r.contains(area.first) && r.contains(area.last)) {
                    return Err(range_error("crosses an array formula or spill boundary"));
                }
            }
        }
        Ok(())
    }

    /// Appends full-width data rows to a worksheet table, extending its filter range.
    /// Tables with calculated columns, sort state, or external uses require
    /// additional reference updates and are refused before the edit.
    pub fn append_table_rows(&mut self, name: &str, rows: &[&[CellWrite<'_>]]) -> Result<()> {
        self.insert_table_rows_at(name, None, rows)
    }

    /// Inserts full-width data rows at a zero-based position within a table.
    pub fn insert_table_rows(&mut self, name: &str, start: u32, rows: &[&[CellWrite<'_>]]) -> Result<()> {
        self.insert_table_rows_at(name, Some(start), rows)
    }

    fn insert_table_rows_at(&mut self, name: &str, start: Option<u32>, rows: &[&[CellWrite<'_>]]) -> Result<()> {
        let (table_index, existing) = self.table_for_row_edit(name)?;
        if rows.is_empty() {
            return Ok(());
        }
        let width = (existing.range.last.col - existing.range.first.col + 1) as usize;
        if rows.iter().any(|row| row.len() != width) {
            return Err(range_error("row width differs from table width"));
        }
        let mut range = existing.range;
        range.last.row = u32::try_from(rows.len()).ok()
            .and_then(|count| range.last.row.checked_add(count))
            .ok_or_else(|| range_error("range outside sheet or reversed"))?;
        CellIndex::new(range.last.row, range.last.col)?;
        let added = CellRange {
            first: CellIndex { row: existing.range.last.row + 1, col: range.first.col },
            last: range.last,
        };
        self.check_table_footprint(added)?;
        let totals = table_totals_rows(&self.wb.pkg.xml_part(&existing.part)?.lens::<TableElement>()?)?;
        let tail = added.first.row - totals;
        let first_data = existing.rows.map_or(range.first.row, |header| header.last.row + 1);
        if tail < first_data {
            return Err(range_error("table requires at least one data row"));
        }
        let start = start.map_or(Some(tail), |offset| first_data.checked_add(offset))
            .filter(|&row| row <= tail)
            .ok_or_else(|| range_error("table insertion is outside the data rows"))?;
        let mut shifted = Vec::new();
        let mut links = None;
        if start < added.first.row {
            let affected = CellRange { first: CellIndex { row: start, col: range.first.col }, last: range.last };
            self.check_table_shift(affected)?;
            let source = CellRange { first: affected.first, last: existing.range.last };
            shifted = self.table_shift_values(source, rows.len() as i32, 0)?;
            Self::invalidate_shifted_total(&mut shifted, range.last.row, totals);
            links = Some(TableLinks::Insert { moved: source, added });
        }
        let writes: Vec<_> = rows.iter().enumerate().flat_map(|(row, values)| {
            values.iter().enumerate().map(move |(column, &value)| (
                CellIndex { row: start + row as u32, col: added.first.col + column as u32 },
                value,
            ))
        }).collect();
        let mut writes = self.prepare_cell_writes(&writes)?;
        writes.extend(self.prepare_table_shift(&shifted)?);
        self.apply_table_row_edit(table_index, &existing, range, writes, links)
    }

    /// Removes trailing data rows and clears their values and formulas, keeping
    /// cell styles. A table without headers must retain one data row.
    pub fn remove_table_tail_rows(&mut self, name: &str, count: u32) -> Result<()> {
        let (table_index, existing) = self.table_for_row_edit(name)?;
        self.remove_table_rows_at(table_index, &existing, None, count)
    }

    /// Removes data rows from a zero-based position, shifting the remaining
    /// data cells and their explicit styles within the table.
    pub fn remove_table_rows(&mut self, name: &str, start: u32, count: u32) -> Result<()> {
        let (table_index, existing) = self.table_for_row_edit(name)?;
        self.remove_table_rows_at(table_index, &existing, Some(start), count)
    }

    fn remove_table_rows_at(&mut self, table_index: usize, existing: &TableHeader, start: Option<u32>, count: u32) -> Result<()> {
        if count == 0 {
            return Ok(());
        }
        let mut range = existing.range;
        let totals = table_totals_rows(&self.wb.pkg.xml_part(&existing.part)?.lens::<TableElement>()?)?;
        let first_data = existing.rows.map_or(range.first.row, |header| header.last.row + 1);
        let last = range.last.row.checked_sub(count)
            .filter(|&last| last >= range.first.row + totals)
            .ok_or_else(|| range_error("table requires a header or data row before the totals row"))?;
        let removed = CellRange {
            first: CellIndex { row: last + 1, col: range.first.col },
            last: range.last,
        };
        range.last.row = last;
        let data_tail = removed.first.row - totals;
        let start = start.map_or(Some(data_tail), |offset| first_data.checked_add(offset))
            .filter(|&row| row <= data_tail)
            .ok_or_else(|| range_error("table removal is outside the data rows"))?;
        let mut writes = Vec::new();
        if start < removed.first.row {
            let affected = CellRange { first: CellIndex { row: start, col: range.first.col }, last: existing.range.last };
            self.check_table_shift(affected)?;
            let source = CellRange {
                first: CellIndex { row: start + count, col: range.first.col },
                last: existing.range.last,
            };
            let mut shifted = self.table_shift_values(source, -(count as i32), 0)?;
            Self::invalidate_shifted_total(&mut shifted, range.last.row, totals);
            writes = self.prepare_table_shift(&shifted)?;
        }
        let mut clears = Vec::new();
        for row in removed.first.row..=removed.last.row {
            for col in removed.first.col..=removed.last.col {
                if self.cell(row, col)?.is_some() {
                    clears.push((CellIndex { row, col }, CellWrite::Clear));
                }
            }
        }
        writes.extend(self.prepare_cell_writes(&clears)?);
        let deleted = CellRange {
            first: CellIndex { row: start, col: range.first.col },
            last: CellIndex { row: start + count - 1, col: range.last.col },
        };
        let moved = (deleted.last.row < existing.range.last.row).then(|| CellRange {
            first: CellIndex { row: deleted.last.row + 1, col: deleted.first.col },
            last: existing.range.last,
        });
        self.apply_table_row_edit(table_index, existing, range, writes, Some(TableLinks::Remove { deleted, moved }))
    }

    fn prepare_table_shift(&mut self, shifted: &[ShiftedTableCell]) -> Result<Vec<Write>> {
        let mut writes = Vec::with_capacity(shifted.len());
        for cell in shifted {
            let write = match (&cell.formula, &cell.value) {
                (Some(formula), value) => CellWrite::FormulaWithResult(formula, value),
                (None, CellValue::Empty) => CellWrite::Clear,
                (None, CellValue::Number(value)) => CellWrite::Number(*value),
                (None, CellValue::String(value)) => CellWrite::String(value),
                (None, CellValue::Bool(value)) => CellWrite::Bool(*value),
                (None, CellValue::Error(value)) => CellWrite::Error(value),
                (None, CellValue::Date(value)) => CellWrite::IsoDate(value),
            };
            writes.push((cell.at, write));
        }
        let mut writes = self.prepare_cell_writes(&writes)?;
        for ((_, _, _, extra), cell) in writes.iter_mut().zip(shifted) {
            extra.style = cell.style.map(StyleRequest::Index);
        }
        Ok(writes)
    }

    fn invalidate_shifted_total(shifted: &mut [ShiftedTableCell], last: u32, totals: u32) {
        if totals != 0 {
            for cell in shifted.iter_mut().filter(|cell| cell.at.row == last && cell.formula.is_some()) {
                cell.value = CellValue::Empty;
            }
        }
    }

    fn check_table_shift(&mut self, affected: CellRange) -> Result<()> {
        if self.validation_overlaps(affected) {
            return Err(range_error("shifting table cells with validation is unsupported"));
        }
        let relationships = self.wb.pkg.relationships(self.part_name())?;
        if [
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/comments",
            "http://schemas.microsoft.com/office/2017/10/relationships/threadedComment",
        ].into_iter().any(|kind| relationships.by_type(kind).next().is_some()) {
            return Err(range_error("shifting table cells on a sheet with comments is unsupported"));
        }
        for row in affected.first.row..=affected.last.row {
            for col in affected.first.col..=affected.last.col {
                let cell = self.cell(row, col)?;
                if cell.as_ref().is_some_and(|cell| cell.formula().is_some() && cell.ordinary_formula().is_none()) {
                    return Err(range_error("shifting table cells with nonordinary formulas is unsupported"));
                }
            }
        }
        Ok(())
    }

    fn table_shift_values(&mut self, source: CellRange, row_delta: i32, column_delta: i32) -> Result<Vec<ShiftedTableCell>> {
        let mut values = Vec::new();
        for row in source.first.row..=source.last.row {
            let target = row.checked_add_signed(row_delta).expect("table shift bounds checked");
            for col in source.first.col..=source.last.col {
                let at = CellIndex { row: target, col: col.checked_add_signed(column_delta).expect("table shift bounds checked") };
                let cell = self.cell(at.row, at.col)?;
                let source = self.cell(row, col)?;
                let style = |cell: Option<&crate::CellRef<'_>>| cell.map_or(Ok(0), crate::CellRef::style_index);
                let source_style = style(source.as_ref())?;
                let moved_style = (source_style != style(cell.as_ref())?).then_some(source_style);
                let (value, formula) = match source {
                    Some(source) => {
                        let formula = source.ordinary_formula();
                        if formula.is_none() && source.has_complex_content_with_dates(true, true)? {
                            return Err(range_error("shifting table cells with complex cell values is unsupported"));
                        }
                        (source.value()?, formula)
                    }
                    None if cell.is_some() => (CellValue::Empty, None),
                    None => continue,
                };
                values.push(ShiftedTableCell { at, value, formula, style: moved_style });
            }
        }
        Ok(values)
    }

    fn table_for_row_edit(&mut self, name: &str) -> Result<(usize, TableHeader)> {
        self.wb.check_editable()?;
        self.wb.contexts[&self.sheet].tables.iter().enumerate()
            .find(|(_, table)| fold(&table.name) == fold(name))
            .map(|(index, table)| (index, table.clone()))
            .ok_or_else(|| Error::NoSuchTable(name.into()))
    }

    fn apply_table_row_edit(
        &mut self,
        table_index: usize,
        existing: &TableHeader,
        range: CellRange,
        writes: Vec<Write>,
        links: Option<TableLinks>,
    ) -> Result<()> {
        let mut document = self.wb.pkg.xml_part(&existing.part)?.clone();
        let root = document.lens::<TableElement>()?;
        let totals = table_totals_rows(&root)?;
        if !worksheet_table(&root)
            || root.child::<sml::SortState>().is_some()
            || root.child::<sml::TableColumns>().is_none_or(|columns| {
                columns.children::<sml::TableColumn>().any(|column| {
                    column.child::<sml::CalculatedColumnFormula>().is_some()
                })
            })
        {
            return Err(range_error("table has unsupported row metadata"));
        }
        let filter_range = |mut range: CellRange| {
            range.last.row -= totals;
            range
        };
        if let Some(filter) = root.child::<sml::AutoFilter>()
            && (filter.attr(attr("ref")).and_then(|value| CellRange::parse(value).ok()) != Some(filter_range(existing.range))
                || filter.child::<sml::SortState>().is_some())
        {
            return Err(range_error("table filter range or sort state cannot be resized"));
        }
        let total_writes: Vec<_> = writes.iter().filter(|(_, at, _, _)| {
            totals != 0 && at.row == existing.range.last.row && existing.range.contains(*at)
        }).map(|(sheet, at, _, _)| (*sheet, *at, CellOp::Clear, Extra::default())).collect();
        if self.wb.table_referenced(existing, &table_names(&root), table_id(&root)?, references::Use::Resize, &total_writes)? {
            return Err(Error::TableInUse(existing.name.clone()));
        }
        let mut root = document.lens_mut::<TableElement>()?;
        let reference = range.to_string();
        root.set_attr(attr("ref"), &reference)?;
        if let Some(mut filter) = root.child_mut::<sml::AutoFilter>() {
            filter.set_attr(attr("ref"), &filter_range(range).to_string())?;
        }
        let plan = self.wb.plan_table_writes(writes, &HashSet::new(), false)?;
        self.commit_table_range(table_index, existing, range, plan, document, links)
    }

    fn commit_table_range(
        &mut self,
        table_index: usize,
        existing: &TableHeader,
        range: CellRange,
        plan: CellWritePlan,
        document: XmlDocument,
        links: Option<TableLinks>,
    ) -> Result<()> {
        let mut registry = RegistryPlan::new(&self.wb.pkg);
        registry.stage_part(&self.wb.pkg, &existing.part, document)?;
        if let Some(links) = links {
            let part = self.part_name().to_owned();
            let sheet = self.wb.pkg.xml_part(&part)?;
            if let Some(sheet) = crate::hyperlink::table_edit_hyperlinks(sheet, links)? {
                registry.stage_part(&self.wb.pkg, &part, sheet)?;
            }
        }
        let sheet = self.sheet;
        self.wb.apply_writes_with_insertion(plan, registry, None, |wb| {
            let table = &mut wb.contexts.get_mut(&sheet).expect("worksheet context").tables[table_index];
            table.range = range;
            if let Some(header) = &mut table.rows {
                header.last.col = range.last.col;
            }
            Ok(())
        })
    }

    /// Appends a column without changing existing column IDs or table identity.
    pub fn append_table_column(&mut self, name: &str, column: Col, values: &[CellWrite<'_>]) -> Result<()> {
        self.insert_table_column_at(name, None, column, values)
    }

    /// Inserts a column at a zero-based position, keeping surviving column IDs.
    pub fn insert_table_column(&mut self, name: &str, start: u32, column: Col, values: &[CellWrite<'_>]) -> Result<()> {
        self.insert_table_column_at(name, Some(start), column, values)
    }

    fn insert_table_column_at(&mut self, name: &str, start: Option<u32>, mut column: Col, values: &[CellWrite<'_>]) -> Result<()> {
        let (index, existing) = self.table_for_row_edit(name)?;
        let mut range = existing.range;
        range.last = CellIndex::new(range.last.row, range.last.col + 1)?;
        let added = CellRange {
            first: CellIndex { row: range.first.row, col: range.last.col },
            last: range.last,
        };
        self.check_table_footprint(added)?;
        if self.validation_overlaps(added) {
            return Err(range_error("new table column overlaps validation"));
        }
        if self.hyperlinks()?.iter().any(|link| {
            CellRange::parse(&link.range).ok().is_none_or(|link| added.intersects(&link))
        }) {
            return Err(range_error("new table column overlaps a hyperlink"));
        }
        let mut document = self.wb.pkg.xml_part(&existing.part)?.clone();
        let root = document.lens::<TableElement>()?;
        let totals = table_totals_rows(&root)?;
        if !worksheet_table(&root) || root.child::<sml::SortState>().is_some() {
            return Err(range_error("table has unsupported column metadata"));
        }
        let first_data = existing.rows.map_or(range.first.row, |header| header.last.row + 1);
        if existing.rows.is_some_and(|header| header.first.row != header.last.row) {
            return Err(range_error("table column edits require at most one header row"));
        }
        let last_data = range.last.row.checked_sub(totals)
            .filter(|last| *last >= range.first.row && *last + 1 >= first_data)
            .ok_or_else(|| range_error("table has an invalid data range"))?;
        let data_count = (last_data + 1 - first_data) as usize;
        if values.len() > data_count {
            return Err(range_error("column values exceed table data rows"));
        }
        let columns = root.child::<sml::TableColumns>().ok_or_else(|| range_error("missing table columns"))?;
        let mut names = HashSet::new();
        let mut ids = HashSet::new();
        for old in columns.children::<sml::TableColumn>() {
            let old_name = old.attr(attr("name")).ok_or_else(|| range_error("table column has no name"))?;
            names.insert(fold(&escape::decode(old_name)));
            let id = old.attr(attr("id")).and_then(|id| id.parse::<u32>().ok()).filter(|id| *id > 0)
                .ok_or_else(|| range_error("table column has no valid id"))?;
            if !ids.insert(id) { return Err(range_error("table column IDs are not unique")); }
        }
        let old_width = (existing.range.last.col - existing.range.first.col + 1) as usize;
        let start = start.map_or(old_width, |value| value as usize);
        if start > old_width {
            return Err(range_error("table insertion is outside the columns"));
        }
        if ids.len() != old_width || names.len() != old_width {
            return Err(range_error("table column metadata does not match the range"));
        }
        if !names.insert(fold(&column.name)) {
            return Err(Error::DuplicateColumnName(column.name));
        }
        let next_id = ids.into_iter().max().and_then(|id| id.checked_add(1))
            .ok_or_else(|| range_error("table column IDs are exhausted"))?;
        let mut filter_range = existing.range;
        filter_range.last.row = last_data;
        if let Some(filter) = root.child::<sml::AutoFilter>()
            && (filter.child::<sml::SortState>().is_some()
                || filter.attr(attr("ref")).and_then(|value| CellRange::parse(value).ok()) != Some(filter_range))
        {
            return Err(range_error("table filter range or sort state cannot be resized"));
        }
        if self.wb.table_referenced(&existing, &table_names(&root), table_id(&root)?, references::Use::Table, &[])? {
            return Err(Error::TableInUse(existing.name));
        }
        let display = root.attr(attr("displayName")).unwrap_or(name);
        let inserted = range.first.col + start as u32;
        let grid = CellRange {
            first: CellIndex { row: range.first.row, col: inserted },
            last: CellIndex { row: range.last.row, col: inserted },
        };
        let mut writes = self.plan_table_column_cells(&mut column, values, grid, first_data, totals, (display, &names))?;
        let mut links = None;
        if start < old_width {
            let first = CellIndex { row: range.first.row, col: inserted };
            self.check_table_shift(CellRange { first, last: range.last })?;
            let source = CellRange { first, last: existing.range.last };
            let shifted = self.table_shift_values(source, 0, 1)?;
            writes.extend(self.prepare_table_shift(&shifted)?);
            links = Some(TableLinks::InsertColumn { moved: source, added });
        }
        let mut root = document.lens_mut::<TableElement>()?;
        root.set_attr(attr("ref"), &range.to_string())?;
        if let Some(filter) = root.child_mut::<sml::AutoFilter>() {
            let mut edits = Vec::new();
            for (index, node) in filter.node().indexed_elements().filter(|(_, node)| node.is(sml::FilterColumn::QNAME)) {
                let col = node.attr(attr("colId")).and_then(|id| id.parse::<usize>().ok())
                    .filter(|col| *col < old_width)
                    .ok_or_else(|| range_error("table filter has an invalid column position"))?;
                if col >= start {
                    edits.push((index, col + 1));
                }
            }
            for (index, col) in edits {
                let mut node = root.child_mut::<sml::AutoFilter>().expect("planned filter")
                    .into_child_at::<sml::FilterColumn>(index).expect("planned column");
                node.set_attr(attr("colId"), &col.to_string())?;
            }
            let mut filter = root.child_mut::<sml::AutoFilter>().expect("planned filter");
            filter_range.last.col = range.last.col;
            filter.set_attr(attr("ref"), &filter_range.to_string())?;
            if !column.filter_button {
                let mut flag = filter.insert::<sml::FilterColumn>()?;
                flag.set_attr(attr("colId"), &start.to_string())?;
                flag.set_attr(attr("hiddenButton"), "1")?;
            }
        }
        let mut columns = root.child_mut::<sml::TableColumns>().expect("validated column list");
        columns.set_attr(attr("count"), &(old_width + 1).to_string())?;
        let before = columns.node().indexed_elements().filter(|(_, node)| node.is(sml::TableColumn::QNAME))
            .nth(start).map(|(index, _)| index);
        let mut new_column = match before {
            Some(index) => columns.insert_at::<sml::TableColumn>(index)?,
            None => columns.insert::<sml::TableColumn>()?,
        };
        write_table_column(&mut new_column, next_id as usize, &column)?;
        let headers = writes.iter()
            .filter(|(_, at, _, _)| existing.rows.is_some_and(|header| header.contains(*at)))
            .map(|(sheet, at, _, _)| (*sheet, *at)).collect();
        let plan = self.wb.plan_table_writes(writes, &headers, false)?;
        self.commit_table_range(index, &existing, range, plan, document, links)
    }

    fn plan_table_column_cells(
        &mut self,
        column: &mut Col,
        values: &[CellWrite<'_>],
        grid: CellRange,
        first_data: u32,
        totals: u32,
        (display, names): (&str, &HashSet<String>),
    ) -> Result<Vec<Write>> {
        let mut cells = Vec::new();
        if first_data != grid.first.row {
            cells.push((grid.first, CellWrite::String(&column.name)));
        }
        if let Some(formula) = &mut column.formula {
            if !values.is_empty() {
                return Err(range_error("calculated table column cannot take explicit values"));
            }
            *formula = references::calculated(formula, display, names)?;
        }
        let data_count = (grid.last.row + 1 - totals - first_data) as usize;
        if values.len() > data_count {
            return Err(range_error("column values exceed table data rows"));
        }
        for offset in 0..data_count {
            let value = match &column.formula {
                Some(formula) => CellWrite::Formula(formula),
                None => values.get(offset).copied().unwrap_or(CellWrite::Clear),
            };
            cells.push((CellIndex { row: first_data + offset as u32, col: grid.first.col }, value));
        }
        let mut writes = self.prepare_cell_writes(&cells)?;
        if totals != 0 {
            writes.push(self.plan_table_total(grid.last, &column.name, &mut column.total, display, names, &CellValue::Empty)?);
        } else if column.total.is_some() {
            return Err(range_error("column total requires a totals row"));
        }
        Ok(writes)
    }

    fn plan_table_total(
        &mut self,
        at: CellIndex,
        column: &str,
        total: &mut Option<ColumnTotal>,
        display: &str,
        names: &HashSet<String>,
        result: &CellValue,
    ) -> Result<Write> {
        if !matches!(result, CellValue::Empty) && !matches!(total, Some(ColumnTotal::Formula(_) | ColumnTotal::Function(_))) {
            return Err(range_error("a totals result requires a formula or function"));
        }
        let subtotal;
        let value = match total {
            Some(ColumnTotal::Label(label)) => CellWrite::String(label),
            Some(ColumnTotal::Formula(formula)) => {
                *formula = references::calculated(formula, display, names)?;
                CellWrite::FormulaWithResult(formula, result)
            }
            Some(ColumnTotal::Function(function)) => {
                let (_, code) = function.file_form();
                let spec = references::specifier(column, false);
                subtotal = format!("SUBTOTAL({code},{display}{spec})");
                CellWrite::FormulaWithResult(&subtotal, result)
            }
            None => CellWrite::Clear,
        };
        Ok(self.prepare_cell_writes(&[(at, value)])?.pop().expect("one cell write"))
    }

    /// Changes a column's totals definition and worksheet cell together.
    /// Passing `None` clears the total. The table must have a totals row.
    pub fn set_table_column_total(&mut self, name: &str, position: u32, total: Option<ColumnTotal>) -> Result<()> {
        self.set_table_column_total_with_result(name, position, total, CellValue::Empty)
    }

    /// Writes a column total with the caller's cached formula result.
    pub fn set_table_column_total_with_result(
        &mut self,
        name: &str,
        position: u32,
        total: Option<ColumnTotal>,
        result: CellValue,
    ) -> Result<()> {
        self.set_table_column_totals(name, vec![(position, total, result)])
    }

    /// Applies column totals and their cached results as one checked edit.
    pub fn set_table_column_totals(
        &mut self,
        name: &str,
        mut edits: Vec<(u32, Option<ColumnTotal>, CellValue)>,
    ) -> Result<()> {
        let (index, existing) = self.table_for_row_edit(name)?;
        if edits.is_empty() {
            return Ok(());
        }
        let range = existing.range;
        let mut document = self.wb.pkg.xml_part(&existing.part)?.clone();
        let root = document.lens::<TableElement>()?;
        if !worksheet_table(&root) || table_totals_rows(&root)? != 1 {
            return Err(range_error("column total requires a worksheet table with a totals row"));
        }
        let columns = root.child::<sml::TableColumns>().ok_or_else(|| range_error("missing table columns"))?;
        let keys = columns.children::<sml::TableColumn>().map(|column| {
            column.attr(attr("name")).map(|name| escape::decode(name).into_owned())
                .ok_or_else(|| range_error("table column has no name"))
        }).collect::<Result<Vec<_>>>()?;
        let names = keys.iter().map(|name| fold(name)).collect();
        let display = root.attr(attr("displayName")).unwrap_or(name);
        let mut writes = Vec::with_capacity(edits.len());
        let mut targets = HashSet::new();
        for (position, total, result) in &mut edits {
            let key = keys.get(*position as usize)
                .filter(|_| *position <= range.last.col - range.first.col)
                .ok_or_else(|| range_error("table total column is outside the table"))?;
            let at = CellIndex { row: range.last.row, col: range.first.col + *position };
            targets.insert(at);
            writes.push(self.plan_table_total(at, key, total, display, &names, result)?);
        }
        let mut root = document.lens_mut::<TableElement>()?;
        let mut columns = root.child_mut::<sml::TableColumns>().expect("validated columns");
        for (position, total, _) in edits {
            let mut column = columns.nth_child_mut::<sml::TableColumn>(position as usize).expect("validated column");
            column.remove_attr(attr("totalsRowFunction"))?;
            column.remove_attr(attr("totalsRowLabel"))?;
            column.remove::<sml::TotalsRowFormula>();
            write_table_total(&mut column, total.as_ref())?;
        }
        let plan = self.wb.plan_table_writes(writes, &HashSet::new(), false)?;
        self.commit_table_range(index, &existing, range, plan, document, Some(TableLinks::ReplaceTotals(targets)))
    }

    /// Replaces every column while retaining the table's identity and style.
    pub fn replace_table_columns(&mut self, name: &str, mut replacements: Vec<(Col, &[CellWrite<'_>])>) -> Result<()> {
        let (index, existing) = self.table_for_row_edit(name)?;
        let width = u32::try_from(replacements.len()).ok().filter(|width| *width > 0)
            .ok_or_else(|| range_error("table requires at least one column"))?;
        let mut range = existing.range;
        let last_col = range.first.col.checked_add(width - 1)
            .ok_or_else(|| range_error("range outside sheet"))?;
        range.last = CellIndex::new(range.last.row, last_col)?;
        if range.last.col > existing.range.last.col {
            self.check_table_footprint(CellRange {
                first: CellIndex { row: range.first.row, col: existing.range.last.col + 1 },
                last: range.last,
            })?;
        }
        let affected = CellRange {
            first: range.first,
            last: CellIndex { row: range.last.row, col: range.last.col.max(existing.range.last.col) },
        };
        if self.validation_overlaps(affected) {
            return Err(range_error("replaced table columns overlap validation"));
        }
        let mut document = self.wb.pkg.xml_part(&existing.part)?.clone();
        let root = document.lens::<TableElement>()?;
        let totals = table_totals_rows(&root)?;
        let first_data = existing.rows.map_or(range.first.row, |header| header.last.row + 1);
        if !worksheet_table(&root) || root.child::<sml::SortState>().is_some()
            || existing.rows.is_some_and(|header| header.first.row != header.last.row)
            || range.last.row + 1 < (first_data + totals).max(range.first.row + totals + 1)
        {
            return Err(range_error("table has unsupported column metadata"));
        }
        let columns = root.child::<sml::TableColumns>().ok_or_else(|| range_error("missing table columns"))?;
        let last_id = columns.children::<sml::TableColumn>().map(|column| {
            column.attr(attr("id")).and_then(|id| id.parse::<u32>().ok()).filter(|id| *id != 0)
                .ok_or_else(|| range_error("table column has no valid id"))
        }).collect::<Result<Vec<_>>>()?.into_iter().max().ok_or_else(|| range_error("missing table columns"))?;
        last_id.checked_add(width).ok_or_else(|| range_error("table column IDs are exhausted"))?;
        let mut filter_range = existing.range;
        filter_range.last.row -= totals;
        if let Some(filter) = root.child::<sml::AutoFilter>()
            && (filter.child::<sml::SortState>().is_some()
                || filter.attr(attr("ref")).and_then(|value| CellRange::parse(value).ok()) != Some(filter_range))
        {
            return Err(range_error("table filter range or sort state cannot be resized"));
        }
        check_unique(replacements.iter().map(|(column, _)| &column.name))?;
        let names = replacements.iter().map(|(column, _)| fold(&column.name)).collect();
        let display = root.attr(attr("displayName")).unwrap_or(name);
        let mut writes = Vec::new();
        for (offset, (column, values)) in replacements.iter_mut().enumerate() {
            let col = range.first.col + offset as u32;
            let grid = CellRange {
                first: CellIndex { row: range.first.row, col },
                last: CellIndex { row: range.last.row, col },
            };
            writes.extend(self.plan_table_column_cells(column, values, grid, first_data, totals, (display, &names))?);
        }
        let mut clears = Vec::new();
        for row in affected.first.row..=affected.last.row {
            for col in (range.last.col + 1)..=affected.last.col {
                if self.cell(row, col)?.is_some() {
                    clears.push((CellIndex { row, col }, CellWrite::Clear));
                }
            }
        }
        writes.extend(self.prepare_cell_writes(&clears)?);
        if self.wb.table_referenced(&existing, &table_names(&root), table_id(&root)?, references::Use::Resize, &writes)? {
            return Err(Error::TableInUse(existing.name));
        }
        let mut root = document.lens_mut::<TableElement>()?;
        root.set_attr(attr("ref"), &range.to_string())?;
        if let Some(mut filter) = root.child_mut::<sml::AutoFilter>() {
            let positions: Vec<_> = filter.node().indexed_elements()
                .filter(|(_, node)| node.is(sml::FilterColumn::QNAME))
                .map(|(index, _)| index).collect();
            for index in positions.into_iter().rev() {
                filter.remove_at(index);
            }
            filter_range.last.col = range.last.col;
            filter.set_attr(attr("ref"), &filter_range.to_string())?;
            for (offset, (column, _)) in replacements.iter().enumerate() {
                if !column.filter_button {
                    let mut flag = filter.insert::<sml::FilterColumn>()?;
                    flag.set_attr(attr("colId"), &offset.to_string())?;
                    flag.set_attr(attr("hiddenButton"), "1")?;
                }
            }
        }
        let mut columns = root.child_mut::<sml::TableColumns>().expect("validated column list");
        let positions: Vec<_> = columns.node().indexed_elements()
            .filter(|(_, node)| node.is(sml::TableColumn::QNAME))
            .map(|(index, _)| index).collect();
        for index in positions.into_iter().rev() {
            columns.remove_at(index);
        }
        columns.set_attr(attr("count"), &width.to_string())?;
        for (offset, (column, _)) in replacements.iter().enumerate() {
            let id = last_id as usize + offset + 1;
            write_table_column(&mut columns.insert::<sml::TableColumn>()?, id, column)?;
        }
        let headers = writes.iter()
            .filter(|(_, at, _, _)| existing.rows.is_some_and(|header| header.contains(*at)))
            .map(|(sheet, at, _, _)| (*sheet, *at)).collect();
        let plan = self.wb.plan_table_writes(writes, &headers, false)?;
        self.commit_table_range(index, &existing, range, plan, document, Some(TableLinks::RemoveColumns {
            deleted: affected,
            moved: None,
        }))
    }

    /// Removes trailing table columns and clears their cells, retaining the
    /// IDs and filter settings of the surviving columns.
    pub fn remove_table_tail_columns(&mut self, name: &str, count: u32) -> Result<()> {
        self.remove_table_columns_at(name, None, count)
    }

    /// Removes columns from a zero-based position and shifts surviving cells.
    pub fn remove_table_columns(&mut self, name: &str, start: u32, count: u32) -> Result<()> {
        self.remove_table_columns_at(name, Some(start), count)
    }

    fn remove_table_columns_at(&mut self, name: &str, start: Option<u32>, count: u32) -> Result<()> {
        let (index, existing) = self.table_for_row_edit(name)?;
        if count == 0 {
            return Ok(());
        }
        let mut range = existing.range;
        range.last.col = range.last.col.checked_sub(count)
            .filter(|&last| last >= range.first.col)
            .ok_or_else(|| range_error("table requires at least one column"))?;
        let trailing = CellRange {
            first: CellIndex { row: range.first.row, col: range.last.col + 1 },
            last: existing.range.last,
        };
        let width = range.last.col - range.first.col + 1;
        let start = start.unwrap_or(width);
        if start > width {
            return Err(range_error("table removal is outside the columns"));
        }
        let deleted = CellRange {
            first: CellIndex { row: range.first.row, col: range.first.col + start },
            last: CellIndex { row: range.last.row, col: range.first.col + start + count - 1 },
        };
        let moved = (start < width).then(|| CellRange {
            first: CellIndex { row: deleted.first.row, col: deleted.last.col + 1 },
            last: existing.range.last,
        });
        let mut writes = Vec::new();
        if let Some(source) = moved {
            let affected = CellRange { first: deleted.first, last: existing.range.last };
            self.check_table_shift(affected)?;
            let shifted = self.table_shift_values(source, 0, -(count as i32))?;
            writes = self.prepare_table_shift(&shifted)?;
        } else if self.validation_overlaps(deleted) {
            return Err(range_error("removed table columns overlap validation"));
        }
        let mut document = self.wb.pkg.xml_part(&existing.part)?.clone();
        let root = document.lens::<TableElement>()?;
        let totals = table_totals_rows(&root)?;
        if !worksheet_table(&root) || root.child::<sml::SortState>().is_some() {
            return Err(range_error("table has unsupported column metadata"));
        }
        let mut filter_range = existing.range;
        filter_range.last.row = filter_range.last.row.checked_sub(totals)
            .filter(|&last| last >= range.first.row)
            .ok_or_else(|| range_error("table has an invalid filter range"))?;
        if let Some(filter) = root.child::<sml::AutoFilter>()
            && (filter.child::<sml::SortState>().is_some()
                || filter.attr(attr("ref")).and_then(|value| CellRange::parse(value).ok()) != Some(filter_range))
        {
            return Err(range_error("table filter range or sort state cannot be resized"));
        }
        let mut cells = Vec::new();
        for row in trailing.first.row..=trailing.last.row {
            for col in trailing.first.col..=trailing.last.col {
                if self.cell(row, col)?.is_some() {
                    cells.push((CellIndex { row, col }, CellWrite::Clear));
                }
            }
        }
        writes.extend(self.prepare_cell_writes(&cells)?);
        let headers = writes.iter()
            .filter(|(_, at, _, _)| existing.rows.is_some_and(|header| header.contains(*at)))
            .map(|(sheet, at, _, _)| (*sheet, *at)).collect();
        let names = table_names(&root);
        if writes.iter().any(|(_, _, op, _)| {
            matches!(op, CellOp::Formula(text, _) if references::has_reference(text, true, &names, references::Use::Table))
        }) || self.wb.table_referenced(&existing, &names, table_id(&root)?, references::Use::Table, &writes)? {
            return Err(Error::TableInUse(existing.name));
        }
        let mut root = document.lens_mut::<TableElement>()?;
        root.set_attr(attr("ref"), &range.to_string())?;
        if let Some(filter) = root.child_mut::<sml::AutoFilter>() {
            let mut edits = Vec::new();
            for (index, node) in filter.node().indexed_elements().filter(|(_, node)| node.is(sml::FilterColumn::QNAME)) {
                let col = node.attr(attr("colId")).and_then(|id| id.parse::<u32>().ok())
                    .filter(|&col| col < width + count)
                    .ok_or_else(|| range_error("table filter has an invalid column position"))?;
                if col >= start {
                    edits.push((index, (col >= start + count).then(|| col - count)));
                }
            }
            for (index, col) in edits.into_iter().rev() {
                if let Some(col) = col {
                    let mut node = root.child_mut::<sml::AutoFilter>().expect("planned filter")
                        .into_child_at::<sml::FilterColumn>(index).expect("planned column");
                    node.set_attr(attr("colId"), &col.to_string())?;
                } else {
                    root.child_mut::<sml::AutoFilter>().expect("planned filter").remove_at(index);
                }
            }
            filter_range.last.col = range.last.col;
            root.child_mut::<sml::AutoFilter>().expect("planned filter").set_attr(attr("ref"), &filter_range.to_string())?;
        }
        let mut columns = root.child_mut::<sml::TableColumns>().ok_or_else(|| range_error("missing table columns"))?;
        let positions: Vec<_> = columns.node().indexed_elements().filter(|(_, node)| node.is(sml::TableColumn::QNAME))
            .map(|(index, _)| index).collect();
        if positions.len() != (width + count) as usize {
            return Err(range_error("table column metadata does not match the range"));
        }
        for &index in positions[start as usize..(start + count) as usize].iter().rev() {
            columns.remove_at(index);
        }
        columns.set_attr(attr("count"), &width.to_string())?;
        let plan = self.wb.plan_table_writes(writes, &headers, false)?;
        self.commit_table_range(index, &existing, range, plan, document, Some(TableLinks::RemoveColumns { deleted, moved }))
    }

    /// Applies the supplied style groups to a table column's cells.
    /// Existing style groups omitted from the patch are preserved.
    pub fn patch_table_column_style(&mut self, name: &str, position: u32, format: crate::Format) -> Result<()> {
        self.patch_table_column_styles(name, vec![(position, format)])
    }

    /// Applies partial styles to several table columns in one edit plan.
    pub fn patch_table_column_styles(&mut self, name: &str, edits: Vec<(u32, crate::Format)>) -> Result<()> {
        let (_, table) = self.table_for_row_edit(name)?;
        let mut writes = Vec::new();
        for (position, format) in edits {
            let col = table.range.first.col.checked_add(position).filter(|col| *col <= table.range.last.col)
                .ok_or_else(|| range_error("table style column is outside the table"))?;
            let style = StyleRequest::Patch(std::sync::Arc::new(format));
            writes.extend((table.range.first.row..=table.range.last.row).map(|row| {
                (self.sheet, CellIndex { row, col }, CellOp::StyleOnly, Extra { style: Some(style.clone()), ..Extra::default() })
            }));
        }
        if writes.is_empty() {
            return Ok(());
        }
        let plan = self.wb.plan_writes(writes)?;
        self.wb.apply_writes(plan)
    }

    /// Updates the table style without rewriting cells or the table's identity.
    pub fn set_table_style(&mut self, name: &str, theme: Option<TableStyle>, flags: [bool; 4]) -> Result<()> {
        self.wb.check_editable()?;
        let part = self.wb.contexts[&self.sheet].tables.iter()
            .find(|table| fold(&table.name) == fold(name))
            .map(|table| table.part.clone()).ok_or_else(|| Error::NoSuchTable(name.into()))?;
        let theme = theme.map(TableStyle::name).transpose()?;
        let mut document = self.wb.pkg.xml_part(&part)?.clone();
        let mut table = document.lens_mut::<TableElement>()?;
        let mut info = table.get_or_insert::<sml::TableStyleInfo>()?;
        match theme {
            Some(Some(theme)) => info.set_attr(attr("name"), &theme)?,
            Some(None) => { info.remove_attr(attr("name"))?; }
            None => {}
        }
        for (key, value) in ["showFirstColumn", "showLastColumn", "showRowStripes", "showColumnStripes"].into_iter().zip(flags) {
            info.set_attr(attr(key), if value { "1" } else { "0" })?;
        }
        *self.wb.pkg.xml_part_mut(&part)? = document;
        Ok(())
    }

    /// Changes a column filter button while preserving the column's filter criteria.
    pub fn set_table_filter_button(&mut self, name: &str, column: u32, visible: bool) -> Result<()> {
        self.set_table_filter_buttons(name, &[(column, visible)])
    }

    /// Applies filter-button edits to one table part in a single transaction.
    pub fn set_table_filter_buttons(&mut self, name: &str, edits: &[(u32, bool)]) -> Result<()> {
        self.wb.check_editable()?;
        if edits.is_empty() {
            return Ok(());
        }
        let part = self.wb.contexts[&self.sheet].tables.iter()
            .find(|table| fold(&table.name) == fold(name))
            .filter(|table| edits.iter().all(|(column, _)| *column <= table.range.last.col - table.range.first.col))
            .map(|table| table.part.clone()).ok_or_else(|| Error::NoSuchTable(name.into()))?;
        let mut document = self.wb.pkg.xml_part(&part)?.clone();
        let mut table = document.lens_mut::<TableElement>()?;
        let Some(mut filter) = table.child_mut::<sml::AutoFilter>() else {
            if edits.iter().all(|(_, visible)| !visible) { return Ok(()); }
            return Err(range_error("filter buttons require a table header"));
        };
        let mut changed = false;
        for &(column, visible) in edits {
            let index = column.to_string();
            let existing = filter.child_mut_by::<sml::FilterColumn>(|node| {
                node.attr(attr("colId")) == Some(index.as_str())
            });
            if let Some(mut node) = existing {
                node.set_attr(attr("hiddenButton"), if visible { "0" } else { "1" })?;
                node.set_attr(attr("showButton"), if visible { "1" } else { "0" })?;
                changed = true;
            } else if !visible {
                let mut node = filter.insert::<sml::FilterColumn>()?;
                node.set_attr(attr("colId"), &index)?;
                node.set_attr(attr("hiddenButton"), "1")?;
                changed = true;
            }
        }
        if !changed {
            return Ok(());
        }
        *self.wb.pkg.xml_part_mut(&part)? = document;
        Ok(())
    }

    /// Reads table definitions in worksheet order without changing their parts.
    pub fn table_nodes(&mut self) -> Result<Vec<ooxml_core::RawNode>> {
        let tables = self.wb.contexts[&self.sheet].tables.clone();
        tables.iter().map(|table| {
            let document = self.wb.pkg.xml_part(&table.part)?;
            Ok(document.lens::<TableElement>()?.raw().clone())
        }).collect()
    }

    /// Removes an unreferenced worksheet table and leaves its cells intact.
    pub fn remove_table(&mut self, name: &str) -> Result<()> {
        let part = self.part_name().to_owned();
        let wb = &mut *self.wb;
        wb.check_editable()?;
        let table = wb.contexts[&self.sheet]
            .tables
            .iter()
            .find(|t| fold(&t.name) == fold(name))
            .cloned()
            .ok_or_else(|| Error::NoSuchTable(name.into()))?;
        let root = wb.pkg.xml_part(&table.part)?.lens::<TableElement>()?;
        let id = table_id(&root)?;
        let names = table_names(&root);
        let (dir, file) = table
            .part
            .rsplit_once('/')
            .map_or(("", table.part.as_str()), |(d, f)| (d, f));
        let rels = if dir.is_empty() {
            format!("_rels/{file}.rels")
        } else {
            format!("{dir}/_rels/{file}.rels")
        };
        if !worksheet_table(&root)
            || wb.pkg.part_name(&rels).is_some()
            || wb.table_referenced(&table, &names, id, references::Use::Table, &[])?
        {
            return Err(Error::TableInUse(table.name));
        }
        let mut sheet = wb.pkg.xml_part(&part)?.clone();
        {
            let mut root = sheet.lens_mut::<sml::Worksheet>()?;
            let mut parts = root
                .child_mut::<sml::TableParts>()
                .expect("table listed in sheet");
            let index = parts
                .node()
                .indexed_elements()
                .find(|(_, n)| n.is(sml::TablePart::QNAME) && n.attr(RID) == Some(&table.rel))
                .map(|(i, _)| i)
                .ok_or_else(|| Error::Malformed("table reference missing".into()))?;
            parts.remove_at(index);
            if parts.node().elements().next().is_none() {
                root.remove::<sml::TableParts>();
            } else {
                update_count(&mut parts)?;
            }
        }
        let mut registry = RegistryPlan::new(&wb.pkg);
        // Core checks surviving sources against the staged tree when removing a relationship.
        registry.stage_part(&wb.pkg, &part, sheet)?;
        registry.remove_parts(&wb.pkg, &[&table.part], &[REL_TABLE])?;
        let sheet_index = self.sheet;
        wb.apply_registry_edit(registry, |wb| {
            wb.contexts
                .get_mut(&sheet_index)
                .expect("worksheet context")
                .tables
                .retain(|t| t.part != table.part);
            wb.forget_removed_tables(std::slice::from_ref(&table.part), id);
            Ok(())
        })
    }
}

fn update_count(parts: &mut ooxml_core::LensMut<'_, sml::TableParts>) -> Result<()> {
    let count = parts
        .node()
        .elements()
        .filter(|n| n.is(sml::TablePart::QNAME))
        .count();
    parts.set_attr(attr("count"), count.format_into(&mut NumBuffer::new()))?;
    Ok(())
}

const TABLE_DXF_KEYS: [&str; 3] = ["headerRowDxfId", "dataDxfId", "totalsRowDxfId"];
const TABLE_BORDER_DXF_KEYS: [&str; 3] = ["headerRowBorderDxfId", "tableBorderDxfId", "totalsRowBorderDxfId"];

fn write_table_differential_formats<E: ooxml_core::ElementSpec>(
    node: &mut ooxml_core::LensMut<'_, E>,
    keys: [&'static str; 3],
    ids: [Option<u32>; 3],
) -> Result<()> {
    for (key, id) in keys.into_iter().zip(ids) {
        if let Some(id) = id {
            node.set_attr(attr(key), id.format_into(&mut NumBuffer::new()))?;
        }
    }
    Ok(())
}

fn write_table_column(child: &mut ooxml_core::LensMut<'_, sml::TableColumn>, id: usize, col: &Col) -> Result<()> {
    child.set_attr(attr("id"), id.format_into(&mut NumBuffer::new()))?;
    child.set_attr(attr("name"), &column_name(&col.name)?)?;
    if col.explicit_no_total_function {
        child.set_attr(attr("totalsRowFunction"), "none")?;
    }
    write_table_differential_formats(child, TABLE_DXF_KEYS, col.differential_formats)?;
    write_table_total(child, col.total.as_ref())?;
    if let Some(f) = &col.formula {
        child
            .insert::<sml::CalculatedColumnFormula>()?
            .set_text(f)?;
    }
    Ok(())
}

fn write_table_total(child: &mut ooxml_core::LensMut<'_, sml::TableColumn>, total: Option<&ColumnTotal>) -> Result<()> {
    match total {
        Some(ColumnTotal::Function(t)) => {
            child.set_attr(attr("totalsRowFunction"), t.file_form().0)?
        }
        Some(ColumnTotal::Label(t)) => {
            child.set_attr(attr("totalsRowLabel"), &escape::encode(t))?
        }
        Some(ColumnTotal::Formula(formula)) => {
            child.set_attr(attr("totalsRowFunction"), "custom")?;
            child.insert::<sml::TotalsRowFormula>()?.set_text(formula)?;
        }
        None => {}
    }
    Ok(())
}

fn table_document(
    table: &Table,
    name: &str,
    display_name: &str,
    id: u32,
    style: Option<&str>,
    columns: &[Col],
) -> Result<XmlDocument> {
    let mut doc = XmlDocument::parse(format!("<table xmlns=\"{SML}\"/>").as_bytes())?;
    let mut root = doc.lens_mut::<TableElement>()?;
    root.set_attr(attr("id"), id.format_into(&mut NumBuffer::new()))?;
    root.set_attr(attr("name"), name)?;
    root.set_attr(attr("displayName"), display_name)?;
    root.set_attr(attr("ref"), &table.range.to_string())?;
    write_table_differential_formats(&mut root, TABLE_DXF_KEYS, table.differential_formats)?;
    write_table_differential_formats(&mut root, TABLE_BORDER_DXF_KEYS, table.border_formats)?;
    if table.totals {
        root.set_attr(attr("totalsRowCount"), "1")?;
    } else {
        root.set_attr(attr("totalsRowShown"), "0")?;
    }
    if table.headers {
        if !table.write_cells {
            root.set_attr(attr("headerRowCount"), "1")?;
        }
        let mut filter = table.range;
        filter.last.row -= u32::from(table.totals);
        let mut auto_filter = root.insert::<sml::AutoFilter>()?;
        auto_filter.set_attr(attr("ref"), &filter.to_string())?;
        for (index, column) in columns.iter().enumerate() {
            if !column.filter_button || !table.write_cells {
                let mut node = auto_filter.insert::<sml::FilterColumn>()?;
                node.set_attr(attr("colId"), &index.to_string())?;
                node.set_attr(attr("hiddenButton"), if column.filter_button { "0" } else { "1" })?;
            }
        }
    } else {
        root.set_attr(attr("headerRowCount"), "0")?;
    }
    let mut list = root.insert::<sml::TableColumns>()?;
    list.set_attr(
        attr("count"),
        columns.len().format_into(&mut NumBuffer::new()),
    )?;
    for (i, col) in columns.iter().enumerate() {
        write_table_column(&mut list.insert::<sml::TableColumn>()?, i + 1, col)?;
    }
    let mut info = root.insert::<sml::TableStyleInfo>()?;
    if let Some(style) = style {
        info.set_attr(attr("name"), style)?;
    }
    if table.first_column {
        info.set_attr(attr("showFirstColumn"), "1")?;
    }
    if table.last_column {
        info.set_attr(attr("showLastColumn"), "1")?;
    }
    info.set_attr(
        attr("showRowStripes"),
        if table.row_stripes { "1" } else { "0" },
    )?;
    info.set_attr(
        attr("showColumnStripes"),
        if table.column_stripes { "1" } else { "0" },
    )?;
    Ok(doc)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(a1: &str) -> CellIndex {
        CellIndex::parse(a1).unwrap()
    }

    fn assert_clean(wb: &Workbook) {
        for name in wb.pkg.part_names() {
            if let Some(doc) = wb.pkg.parsed_part(name) {
                assert!(!doc.is_dirty(), "{name} is dirty");
                assert!(!doc.root().subtree_dirty(), "{name} has a dirty subtree");
            }
        }
    }

    /// One table `T` over `A1:A2`, header `Name` at `A1`.
    fn table_wb() -> Workbook {
        let mut wb = Workbook::new().unwrap();
        let mut ws = wb.worksheet().unwrap();
        ws.write_string(0, 0, "Name").unwrap();
        ws.write_string(1, 0, "ann").unwrap();
        ws.add_table(Table::new(crate::CellRange::parse("A1:A2").unwrap()).name("T"))
            .unwrap();
        wb
    }

    fn table_xml(wb: &Workbook, part: &str) -> String {
        String::from_utf8(wb.pkg.read_part(part).unwrap()).unwrap()
    }

    #[test]
    fn plan_plus_apply_equals_write_with_headers_and_plans_nothing() {
        let wb = table_wb();
        let rename = || {
            vec![(
                0,
                at("A1"),
                CellOp::Text("Renamed".into()),
                crate::edit::Extra::default(),
            )]
        };
        let plain = || {
            vec![(
                0,
                at("B1"),
                CellOp::Number("7".into()),
                crate::edit::Extra::default(),
            )]
        };
        let run = |make: &dyn Fn() -> Vec<Write>, renamed: bool| {
            let bytes = wb.to_bytes().unwrap();
            let mut fresh = Workbook::from_bytes(bytes.clone()).unwrap();
            assert_clean(&fresh);
            let (staged, plan) = fresh.plan_with_headers(make()).unwrap();
            assert_eq!(fresh.to_bytes().unwrap(), bytes, "a plan changes nothing");
            assert_clean(&fresh);
            fresh.apply_with_headers(staged, plan).unwrap();
            let mut combined = Workbook::from_bytes(bytes).unwrap();
            combined.write_with_headers(make()).unwrap();
            assert_eq!(fresh.to_bytes().unwrap(), combined.to_bytes().unwrap());
            let xml = table_xml(&fresh, "xl/tables/table1.xml");
            assert_eq!(xml.contains(r#"name="Renamed""#), renamed, "{xml}");
        };
        run(&rename, true);
        run(&plain, false);
    }

    #[test]
    fn single_header_writes_funnel_through_plan_with_headers() {
        let wb = table_wb();
        let bytes = wb.to_bytes().unwrap();
        // the real single-write funnel stages the rename and applies it
        let mut renamed = Workbook::from_bytes(bytes.clone()).unwrap();
        renamed
            .edit_cell(0, at("A1"), CellOp::Text("Renamed".into()))
            .unwrap();
        let xml = table_xml(&renamed, "xl/tables/table1.xml");
        assert!(xml.contains(r#"name="Renamed""#), "{xml}");
        // `plan_cell_write` stages no rename: the same write refuses there,
        // so slice B must plan single writes via `plan_with_headers`
        let mut planned = Workbook::from_bytes(bytes.clone()).unwrap();
        std::assert_matches!(
            planned.plan_cell_write(
                0,
                at("A1"),
                CellOp::Text("Renamed".into()),
                crate::edit::Extra::default()
            ),
            Err(Error::TableHeaderCell { .. })
        );
        assert_eq!(planned.to_bytes().unwrap(), bytes);
        assert_clean(&planned);
    }

    #[test]
    fn header_staging_refusals_are_byte_identical() {
        let wb = table_wb();
        let text = |cell: &str, t: &str| {
            (
                0,
                at(cell),
                CellOp::Text(t.into()),
                crate::edit::Extra::default(),
            )
        };
        let duplicate = vec![text("A1", "x"), text("A1", "y")];
        let empty = vec![text("A1", "")];
        let number = vec![(
            0,
            at("A1"),
            CellOp::Number("1".into()),
            crate::edit::Extra::default(),
        )];
        // a rename plus a formula using the column refuses before any parse
        let in_use = vec![
            text("A1", "New"),
            (
                0,
                at("B1"),
                CellOp::Formula("T[Name]*2".into(), None),
                crate::edit::Extra::default(),
            ),
        ];
        for (batch, kind) in [
            (duplicate, "DuplicateCellWrite"),
            (empty, "TableHeaderCell"),
            (number, "TableHeaderCell"),
            (in_use, "ColumnInUse"),
        ] {
            let bytes = wb.to_bytes().unwrap();
            let mut fresh = Workbook::from_bytes(bytes.clone()).unwrap();
            let err = fresh.plan_with_headers(batch).unwrap_err();
            let ok = match kind {
                "DuplicateCellWrite" => matches!(err, Error::DuplicateCellWrite { .. }),
                "TableHeaderCell" => matches!(err, Error::TableHeaderCell { .. }),
                _ => matches!(err, Error::ColumnInUse(_)),
            };
            assert!(ok, "wrong refusal: {err:?}");
            assert_eq!(
                fresh.to_bytes().unwrap(),
                bytes,
                "a refusal changes nothing"
            );
            assert_clean(&fresh);
        }
    }
}
