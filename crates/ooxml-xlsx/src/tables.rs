//! SpreadsheetML tables and synchronized header writes (ADR-0017 §6).

mod references;

use std::collections::{HashMap, HashSet};
use std::fmt::NumBuffer;

use ooxml_core::{ElementSpec, NsDecl, PartNameRequest, QName, RegistryPlan, Target, XmlDocument};
use ooxml_schema::sml;
use sml::Table as TableElement;

use crate::census::FKind;
use crate::edit::{CellOp, Extra};
use crate::escape;
use crate::formula::precedents::fold;
use crate::workbook::{ApplyStepHook, REL_TABLE, TableHeader};
use crate::{CellIndex, CellRange, CellValue, Error, Result, Workbook, Worksheet};

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
    columns: Option<Vec<Col>>,
    headers: bool,
    totals: bool,
    overwrite: bool,
    style: TableStyle,
    row_stripes: bool,
    column_stripes: bool,
}

impl Table {
    /// Creates a table with a header and banded rows.
    pub fn new(range: CellRange) -> Self {
        Self {
            range,
            name: None,
            columns: None,
            headers: true,
            totals: false,
            overwrite: false,
            style: TableStyle::Medium9,
            row_stripes: true,
            column_stripes: false,
        }
    }

    /// Sets the workbook-wide table name.
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
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

#[derive(Clone, Debug)]
enum TotalsCell {
    Function(Total),
    Label(String),
}

/// Definition of one table column.
#[derive(Clone, Debug)]
pub struct Col {
    name: String,
    total: Option<TotalsCell>,
    formula: Option<String>,
}

impl Col {
    /// Sets the heading and structured-reference name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            total: None,
            formula: None,
        }
    }

    /// Sets a totals-row function for this column.
    pub fn total(mut self, function: Total) -> Self {
        self.total = Some(TotalsCell::Function(function));
        self
    }

    /// Sets a text label in the totals row.
    pub fn total_label(mut self, label: impl Into<String>) -> Self {
        self.total = Some(TotalsCell::Label(label.into()));
        self
    }

    /// Sets a formula for every data row. Local row references such as
    /// `[@Amount]` are converted to the file form.
    pub fn formula(mut self, formula: impl Into<String>) -> Self {
        self.formula = Some(formula.into());
        self
    }
}

type Write = (usize, CellIndex, CellOp, Extra);

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

    pub(crate) fn write_with_headers(&mut self, writes: Vec<Write>) -> Result<()> {
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
                matches!(op, CellOp::Formula(formula) if references::has_reference(
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
        let plan = self.plan_table_writes(writes, &headers, false)?;
        if staged.is_empty() {
            return self.apply_writes(plan);
        }
        let mut registry = RegistryPlan::new(&self.pkg);
        for (part, doc) in staged {
            registry.stage_part(&self.pkg, &part, doc)?;
        }
        self.apply_writes_with(plan, registry)
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
        check_unique(columns.iter().map(|c| &c.name))?;
        let column_names = columns.iter().map(|col| fold(&col.name)).collect();
        let style = table.style.name()?;
        let mut writes = Vec::new();
        for (i, col) in columns.iter_mut().enumerate() {
            column_name(&col.name)?;
            let c = range.first.col + i as u32;
            if table.headers {
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
            if let Some(total) = &col.total {
                if !table.totals {
                    return Err(range_error("column total requires a totals row"));
                }
                let at = CellIndex {
                    row: range.last.row,
                    col: c,
                };
                match total {
                    TotalsCell::Label(label) => self.plan_table_cell(
                        &mut writes,
                        at,
                        Some(label),
                        None,
                        table.overwrite,
                        false,
                    )?,
                    TotalsCell::Function(total) => {
                        let (_, code) = total.file_form();
                        let spec = references::specifier(&col.name, false);
                        let formula = format!("SUBTOTAL({code},{name}{spec})");
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
                *formula = references::calculated(formula, &name, &column_names)?;
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
        let doc = table_document(&table, &name, id, style.as_deref(), &columns)?;
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
        wb.apply_writes_with_insertion(cell_plan, registry, |wb| {
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
                .insert(part.clone(), vec![fold(&name)]);
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
                CellOp::Formula(formula.expect("formula supplied").into()),
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
        if r.last.row - r.first.row < u32::from(table.headers) + u32::from(table.totals) {
            return Err(range_error("table requires at least one data row"));
        }
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

fn table_document(
    table: &Table,
    name: &str,
    id: u32,
    style: Option<&str>,
    columns: &[Col],
) -> Result<XmlDocument> {
    let mut doc = XmlDocument::parse(format!("<table xmlns=\"{SML}\"/>").as_bytes())?;
    let mut root = doc.lens_mut::<TableElement>()?;
    root.set_attr(attr("id"), id.format_into(&mut NumBuffer::new()))?;
    root.set_attr(attr("name"), name)?;
    root.set_attr(attr("displayName"), name)?;
    root.set_attr(attr("ref"), &table.range.to_string())?;
    if table.totals {
        root.set_attr(attr("totalsRowCount"), "1")?;
    } else {
        root.set_attr(attr("totalsRowShown"), "0")?;
    }
    if table.headers {
        let mut filter = table.range;
        filter.last.row -= u32::from(table.totals);
        root.insert::<sml::AutoFilter>()?
            .set_attr(attr("ref"), &filter.to_string())?;
    } else {
        root.set_attr(attr("headerRowCount"), "0")?;
    }
    let mut list = root.insert::<sml::TableColumns>()?;
    list.set_attr(
        attr("count"),
        columns.len().format_into(&mut NumBuffer::new()),
    )?;
    for (i, col) in columns.iter().enumerate() {
        let mut child = list.insert::<sml::TableColumn>()?;
        child.set_attr(attr("id"), (i + 1).format_into(&mut NumBuffer::new()))?;
        child.set_attr(attr("name"), &column_name(&col.name)?)?;
        match &col.total {
            Some(TotalsCell::Function(t)) => {
                child.set_attr(attr("totalsRowFunction"), t.file_form().0)?
            }
            Some(TotalsCell::Label(t)) => {
                child.set_attr(attr("totalsRowLabel"), &escape::encode(t))?
            }
            None => {}
        }
        if let Some(f) = &col.formula {
            child
                .insert::<sml::CalculatedColumnFormula>()?
                .set_text(f)?;
        }
    }
    let mut info = root.insert::<sml::TableStyleInfo>()?;
    if let Some(style) = style {
        info.set_attr(attr("name"), style)?;
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
