//! Row and column edits (ADR-0018).

use std::collections::{BTreeMap, HashMap, HashSet};

use ooxml_core::{ElementSpec, QName, RawNode, RegistryPlan, XmlDocument};
use ooxml_schema::sml;

use super::{Change, NthAt, RewrittenLocation, defined_names, names_lens, same_name};
use crate::census::{FKind, SheetKind};
use crate::edit::strip_cells;
use crate::formula::lexer::{Area, Prefix, TokKind, lex};
use crate::formula::precedents::{Prec, Rect};
use crate::formula::structural::{
    AreaOutcome, Axis, Rewrite, RewriteError, Shift, reference_token, rewrite_formula, shift_area,
};
use crate::formula::translate::translate;
use crate::reference::{MAX_COLS, MAX_ROWS};
use crate::registry::strings::SharedStrings;
use crate::workbook::{ApplyStepHook as Step, REL_CALC_CHAIN};
use crate::{CalcPolicy, CellIndex, CellRange, Error, Result, Workbook, Worksheet};

const fn attr(local: &'static str) -> QName {
    QName::new("", local)
}

const fn tag(local: &'static str) -> QName {
    QName::new(crate::SML_NS, local)
}

/// An element removed by a row or column operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemovedItem {
    /// Part containing the element.
    pub part: String,
    /// Element and, where available, its original address.
    pub location: String,
}

/// Changes made by a row or column operation.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StructuralEditReport {
    /// Updated references and formula text.
    pub rewritten: Vec<RewrittenLocation>,
    /// Deleted elements.
    pub removed: Vec<RemovedItem>,
}

impl<'wb> Worksheet<'wb> {
    /// Insert blank, unformatted rows before the zero-based row `at`.
    pub fn insert_rows(&mut self, at: u32, count: u32) -> Result<StructuralEditReport> {
        self.wb.edit_grid(
            self.sheet,
            Axis::Row,
            Shift::Insert { at, count },
            "insertRows",
        )
    }

    /// Delete `count` rows starting at the zero-based row `at`.
    pub fn delete_rows(&mut self, at: u32, count: u32) -> Result<StructuralEditReport> {
        self.wb.edit_grid(
            self.sheet,
            Axis::Row,
            Shift::Delete { at, count },
            "deleteRows",
        )
    }

    /// Insert blank, unformatted columns before the zero-based column `at`.
    pub fn insert_cols(&mut self, at: u32, count: u32) -> Result<StructuralEditReport> {
        self.wb.edit_grid(
            self.sheet,
            Axis::Column,
            Shift::Insert { at, count },
            "insertColumns",
        )
    }

    /// Delete `count` columns starting at the zero-based column `at`.
    pub fn delete_cols(&mut self, at: u32, count: u32) -> Result<StructuralEditReport> {
        self.wb.edit_grid(
            self.sheet,
            Axis::Column,
            Shift::Delete { at, count },
            "deleteColumns",
        )
    }
}

#[derive(Clone, Copy)]
struct Band {
    axis: Axis,
    shift: Shift,
    at: u32,
    count: u32,
    limit: u32,
}

impl Band {
    fn new(axis: Axis, shift: Shift) -> Result<Self> {
        let (Shift::Insert { at, count } | Shift::Delete { at, count }) = shift;
        let limit = match axis {
            Axis::Row => MAX_ROWS,
            Axis::Column => MAX_COLS,
        };
        if count == 0 || at.checked_add(count).is_none_or(|end| end > limit) {
            return Err(Error::InvalidStructuralBand { at, count });
        }
        Ok(Self {
            axis,
            shift,
            at,
            count,
            limit,
        })
    }

    fn index(self, cell: CellIndex) -> u32 {
        match self.axis {
            Axis::Row => cell.row,
            Axis::Column => cell.col,
        }
    }

    fn with_index(self, mut cell: CellIndex, index: u32) -> CellIndex {
        match self.axis {
            Axis::Row => cell.row = index,
            Axis::Column => cell.col = index,
        }
        cell
    }

    fn after(self, index: u32, what: &str) -> Result<Option<u32>> {
        self.shift
            .after(index, self.limit)
            .map_err(|_| Error::ShiftOffSheet { what: what.into() })
    }

    fn cell(self, cell: CellIndex) -> Result<Option<CellIndex>> {
        let index = self
            .shift
            .after(self.index(cell), self.limit)
            .map_err(|_| Error::ShiftOffSheet {
                what: cell.to_string(),
            })?;
        Ok(index.map(|index| self.with_index(cell, index)))
    }

    fn blocked(self, range: CellRange, part: &str, what: &str) -> Result<()> {
        let last = self.index(range.last);
        self.after(last, what)?;
        if last >= self.at {
            return Err(Error::StructureBlocked {
                part: part.into(),
                place: what.into(),
            });
        }
        Ok(())
    }

    fn extent(self, range: CellRange, cell: CellIndex) -> Result<Option<CellRange>> {
        let first = self.index(range.first);
        let last = self.index(range.last);
        let end = self.at + self.count;
        match self.shift {
            Shift::Insert { .. } if first < self.at && self.at <= last => {
                return Err(Error::ArrayFormulaSplit {
                    cell: cell.to_string(),
                });
            }
            Shift::Delete { .. } if first < end && self.at <= last => {
                if first >= self.at && last < end {
                    return Ok(None);
                }
                return Err(Error::ArrayFormulaSplit {
                    cell: cell.to_string(),
                });
            }
            _ => {}
        }
        Ok(Some(CellRange {
            first: self.cell(range.first)?.expect("surviving area"),
            last: self.cell(range.last)?.expect("surviving area"),
        }))
    }

    fn seed_all(sheet: usize) -> Rect {
        Rect {
            sheet,
            r1: 0,
            c1: 0,
            r2: MAX_ROWS - 1,
            c2: MAX_COLS - 1,
        }
    }
}

struct FormulaContext<'a> {
    sheets: &'a [String],
    edited: usize,
    band: Band,
}

impl FormulaContext<'_> {
    fn sheet(&self, name: &str) -> Option<usize> {
        self.sheets.iter().position(|s| same_name(s, name))
    }

    fn rewrite(
        &self,
        text: &str,
        own: Option<usize>,
        name: Option<&str>,
        part: &str,
        location: &str,
    ) -> Result<Option<String>> {
        let refuse = || Error::UnrewritableReference {
            part: part.into(),
            location: location.into(),
        };
        let tokens = lex(text).map_err(|_| refuse())?;
        for token in tokens {
            let Some((reference, _)) = reference_token(text, &token) else {
                continue;
            };
            if reference.area == Area::RefError {
                continue;
            }
            let applies = match &reference.prefix {
                Prefix::None if name.is_some() => return Err(refuse()),
                Prefix::None => own == Some(self.edited),
                Prefix::Sheet(sheet) => self.sheet(sheet) == Some(self.edited),
                Prefix::Sheets(first, last) => self
                    .sheet(first)
                    .zip(self.sheet(last))
                    .is_some_and(|(a, b)| (a.min(b)..=a.max(b)).contains(&self.edited)),
                Prefix::External => {
                    text[token.start..token.start + reference.area_offset].contains("[0]")
                }
            };
            if !applies {
                continue;
            }
            if name.is_some() && relative(reference.area) {
                return Err(refuse());
            }
            let outcome =
                shift_area(reference.area, self.band.axis, self.band.shift).map_err(|_| {
                    Error::ShiftOffSheet {
                        what: format!("{part} {location}"),
                    }
                })?;
            if outcome == AreaOutcome::Unchanged {
                continue;
            }
            if matches!(reference.prefix, Prefix::Sheets(..) | Prefix::External) {
                return Err(refuse());
            }
            if outcome == AreaOutcome::Deleted
                && name.is_some_and(|n| {
                    matches!(
                        n,
                        "_xlnm.Print_Area" | "_xlnm.Print_Titles" | "_xlnm._FilterDatabase"
                    )
                })
            {
                return Err(Error::ReferenceDeleted {
                    part: part.into(),
                    location: location.into(),
                });
            }
        }
        match rewrite_formula(
            text,
            self.band.axis,
            self.band.shift,
            |prefix| match prefix {
                Prefix::None => own == Some(self.edited),
                Prefix::Sheet(sheet) => self.sheet(sheet) == Some(self.edited),
                _ => false,
            },
        )
        .map_err(|error| rewrite_error(error, part, location))?
        {
            Rewrite::Keep => Ok(None),
            Rewrite::New(text) => Ok(Some(text)),
            Rewrite::Remove => unreachable!("a deleted formula token becomes #REF!"),
        }
    }
}

fn relative(area: Area) -> bool {
    match area {
        Area::Cell { col, row } => !col.abs || !row.abs,
        Area::Range { c1, r1, c2, r2 } => !c1.abs || !r1.abs || !c2.abs || !r2.abs,
        Area::Cols { c1, c2 } => !c1.abs || !c2.abs,
        Area::Rows { r1, r2 } => !r1.abs || !r2.abs,
        Area::RefError => false,
    }
}

fn rewrite_error(error: RewriteError, part: &str, location: &str) -> Error {
    match error {
        RewriteError::OffSheet => Error::ShiftOffSheet {
            what: format!("{part} {location}"),
        },
        _ => Error::UnrewritableReference {
            part: part.into(),
            location: location.into(),
        },
    }
}

#[derive(Clone, Default)]
struct FormulaEdit {
    text: Option<String>,
    reference: Option<Option<String>>,
    detach_shared: bool,
    inputs: Vec<(&'static str, String)>,
}

type FormulaEdits = HashMap<(usize, CellIndex), FormulaEdit>;

fn record(
    report: &mut StructuralEditReport,
    part: &str,
    location: String,
    before: &str,
    after: &str,
) {
    report.rewritten.push(RewrittenLocation {
        part: part.into(),
        location,
        before: before.into(),
        after: after.into(),
    });
}

fn removed(report: &mut StructuralEditReport, part: &str, location: String) {
    report.removed.push(RemovedItem {
        part: part.into(),
        location,
    });
}

struct GridPlan {
    rows: Vec<(usize, u32)>,
    cells: Vec<(usize, usize, String)>,
    spans: Vec<(usize, Option<String>)>,
    drop_rows: Vec<usize>,
    drop_cells: Vec<(usize, Vec<usize>)>,
    deleted_strings: i64,
}

fn is_alternate_content(node: &RawNode) -> bool {
    node.is(QName::new(
        "http://schemas.openxmlformats.org/markup-compatibility/2006",
        "AlternateContent",
    ))
}

fn xml_row(node: &RawNode, part: &str) -> Result<u32> {
    node.attr(attr("r"))
        .ok_or_else(|| Error::MissingReference {
            part: part.into(),
            what: "structural edit".into(),
        })?
        .parse::<u32>()
        .ok()
        .and_then(|row| row.checked_sub(1))
        .filter(|row| *row < MAX_ROWS)
        .ok_or_else(|| Error::Malformed(format!("row reference in {part}")))
}

fn xml_cell(node: &RawNode, part: &str) -> Result<CellIndex> {
    CellIndex::parse(
        node.attr(attr("r"))
            .ok_or_else(|| Error::MissingReference {
                part: part.into(),
                what: "structural edit".into(),
            })?,
    )
}

impl GridPlan {
    fn validate(
        doc: &XmlDocument,
        part: &str,
        band: Band,
        report: &mut StructuralEditReport,
    ) -> Result<Self> {
        let mut plan = Self {
            rows: Vec::new(),
            cells: Vec::new(),
            spans: Vec::new(),
            drop_rows: Vec::new(),
            drop_cells: Vec::new(),
            deleted_strings: 0,
        };
        let Some(data) = doc.root().elements().find(|n| n.is(tag("sheetData"))) else {
            return Ok(plan);
        };
        if data.elements().any(is_alternate_content) {
            return Err(Error::StructureBlocked {
                part: part.into(),
                place: "alternate sheetData content".into(),
            });
        }
        for (ri, row) in data.indexed_elements().filter(|(_, n)| n.is(tag("row"))) {
            if row.elements().any(is_alternate_content) {
                return Err(Error::StructureBlocked {
                    part: part.into(),
                    place: "alternate row content".into(),
                });
            }
            let old = xml_row(row, part)?;
            let row_number = if matches!(band.axis, Axis::Row) {
                band.after(old, "row")?
            } else {
                Some(old)
            };
            if row_number.is_none() {
                plan.drop_rows.push(ri);
                removed(report, part, format!("row {}", old + 1));
            }
            if let Some(new) = row_number.filter(|&new| new != old) {
                plan.rows.push((ri, new + 1));
            }
            if matches!(band.axis, Axis::Column)
                && let Some(old_spans) = row.attr(attr("spans"))
            {
                let mut spans = Vec::new();
                for span in old_spans.split_ascii_whitespace() {
                    let (lo, hi) = numeric_range(span, ':', 1, MAX_COLS)?;
                    if let Some((lo, hi)) = numeric_shift(band, lo, hi)? {
                        spans.push(format!("{}:{}", lo + 1, hi + 1));
                    }
                }
                let text = spans.join(" ");
                if text != old_spans {
                    plan.spans.push((ri, (!text.is_empty()).then_some(text)));
                }
            }
            let mut drop = Vec::new();
            for (ci, cell) in row.indexed_elements().filter(|(_, n)| n.is(tag("c"))) {
                let from = xml_cell(cell, part)?;
                if from.row != old {
                    return Err(Error::Malformed(format!(
                        "cell {from} disagrees with row {}",
                        old + 1
                    )));
                }
                match band.cell(from)? {
                    Some(to) if to != from => plan.cells.push((ri, ci, to.to_string())),
                    None => {
                        plan.deleted_strings += i64::from(
                            cell.attr(attr("t"))
                                .is_some_and(|value| value.trim() == "s"),
                        );
                        if row_number.is_some() {
                            drop.push(ci);
                            removed(report, part, format!("cell {from}"));
                        }
                    }
                    _ => {}
                }
            }
            if !drop.is_empty() {
                plan.drop_cells.push((ri, drop));
            }
        }
        Ok(plan)
    }

    fn renumber(&self, doc: &mut XmlDocument) -> Result<()> {
        for (ri, value) in &self.rows {
            let mut row = sheet_data(doc)?
                .into_child_at::<sml::Row>(*ri)
                .expect("planned row");
            row.preserve_attribute_spelling();
            row.set_attr(
                attr("r"),
                value.format_into(&mut core::fmt::NumBuffer::new()),
            )?;
        }
        for (ri, value) in &self.spans {
            let mut row = sheet_data(doc)?
                .into_child_at::<sml::Row>(*ri)
                .expect("planned row");
            row.preserve_attribute_spelling();
            if let Some(value) = value {
                row.set_attr(attr("spans"), value)?;
            } else {
                row.remove_attr(attr("spans"))?;
            }
        }
        for (ri, ci, value) in &self.cells {
            let mut cell = sheet_data(doc)?
                .into_child_at::<sml::Row>(*ri)
                .expect("planned row")
                .into_child_at::<sml::Cell>(*ci)
                .expect("planned cell");
            cell.preserve_attribute_spelling();
            cell.set_attr(attr("r"), value)?;
        }
        Ok(())
    }

    fn remove(self, doc: &mut XmlDocument) -> Result<()> {
        for (ri, indices) in self.drop_cells {
            let mut row = sheet_data(doc)?
                .into_child_at::<sml::Row>(ri)
                .expect("planned row");
            remove_indices(&mut row, &indices);
        }
        if !self.drop_rows.is_empty() {
            remove_indices(&mut sheet_data(doc)?, &self.drop_rows);
        }
        Ok(())
    }
}

fn sheet_data(doc: &mut XmlDocument) -> Result<ooxml_core::LensMut<'_, sml::SheetData>> {
    doc.lens_mut::<sml::Worksheet>()?
        .into_child_by::<sml::SheetData>(|_| true)
        .ok_or_else(|| Error::Malformed("planned sheetData".into()))
}

fn remove_indices<E: ElementSpec>(lens: &mut ooxml_core::LensMut<'_, E>, indices: &[usize]) {
    let Some((&last, rest)) = indices.split_last() else {
        return;
    };
    let (mut start, mut end) = (last, last + 1);
    for &index in rest.iter().rev() {
        if index + 1 == start {
            start = index;
        } else {
            assert!(lens.remove_range(start..end));
            (start, end) = (index, index + 1);
        }
    }
    assert!(lens.remove_range(start..end));
}

fn numeric_range(text: &str, separator: char, base: u32, limit: u32) -> Result<(u32, u32)> {
    let parse = |text: &str| {
        text.parse::<u32>()
            .ok()
            .and_then(|value| value.checked_sub(base))
            .filter(|&value| value < limit)
    };
    if let Some((first, last)) = text.split_once(separator)
        && let (Some(first), Some(last)) = (parse(first), parse(last))
        && first <= last
    {
        return Ok((first, last));
    }
    Err(Error::InvalidReference(text.into()))
}

fn numeric_shift(band: Band, lo: u32, hi: u32) -> Result<Option<(u32, u32)>> {
    use crate::formula::lexer::Comp;
    let component = |idx| Comp { idx, abs: false };
    let area = match band.axis {
        Axis::Row => Area::Rows {
            r1: component(lo),
            r2: component(hi),
        },
        Axis::Column => Area::Cols {
            c1: component(lo),
            c2: component(hi),
        },
    };
    Ok(
        match shift_area(area, band.axis, band.shift).map_err(|_| Error::ShiftOffSheet {
            what: "numeric range".into(),
        })? {
            AreaOutcome::Unchanged => Some((lo, hi)),
            AreaOutcome::Deleted => None,
            AreaOutcome::Moved(Area::Rows { r1, r2 })
            | AreaOutcome::Resized(Area::Rows { r1, r2 })
            | AreaOutcome::Moved(Area::Cols { c1: r1, c2: r2 })
            | AreaOutcome::Resized(Area::Cols { c1: r1, c2: r2 }) => Some((r1.idx, r2.idx)),
            _ => unreachable!("numeric range stays on one axis"),
        },
    )
}

fn translate_from(
    wb: &Workbook,
    text: &str,
    from: CellIndex,
    to: CellIndex,
    part: &str,
) -> Result<String> {
    translate(
        text,
        i64::from(to.row) - i64::from(from.row),
        i64::from(to.col) - i64::from(from.col),
        &|name| wb.env.is_name(name),
    )
    .map_err(|_| Error::UnrewritableReference {
        part: part.into(),
        location: format!("{from} formula"),
    })
}

fn plan_formulas(
    wb: &Workbook,
    ctx: &FormulaContext<'_>,
    report: &mut StructuralEditReport,
) -> Result<(FormulaEdits, Vec<usize>, bool)> {
    let mut edits = HashMap::new();
    let mut groups: BTreeMap<_, Vec<_>> = BTreeMap::new();
    let mut seeds = Vec::new();
    let mut chain = false;
    let mut masters = HashMap::new();
    let mut master_ids = HashMap::new();
    for (fid, cell) in wb.census.alive() {
        if let FKind::Shared { si, master: true } = cell.kind {
            master_ids.entry((cell.sheet, si)).or_insert(fid);
        }
    }
    let lookup = |sheet, si| master_ids.get(&(sheet, si)).copied();
    for (fid, cell) in wb.census.alive() {
        let own = cell.sheet == ctx.edited;
        let part = wb.sheets[cell.sheet].part.as_deref().expect("formula part");
        let next = if own {
            ctx.band.cell(cell.at)?
        } else {
            Some(cell.at)
        };
        chain |= next != Some(cell.at);
        if own
            && let Some(range) =
                cell.result_range()
                    .or(if cell.dynamic { cell.range } else { None })
        {
            let range = ctx.band.extent(range, cell.at)?;
            if let Some(range) = range
                && Some(range) != cell.result_range()
            {
                edits
                    .entry((cell.sheet, cell.at))
                    .or_insert_with(FormulaEdit::default)
                    .reference = Some(Some(range.to_string()));
            }
        }
        if next.is_none() {
            seeds.push(fid);
        }
        let precedents =
            crate::graph::precedents_of(&wb.env, &wb.census, cell, &lookup, &mut masters);
        let reads_band = match precedents {
            Prec::Everything => true,
            Prec::Cells(ranges) => ranges.iter().any(|range| {
                range.sheet == ctx.edited
                    && match ctx.band.axis {
                        Axis::Row => range.r2 >= ctx.band.at,
                        Axis::Column => range.c2 >= ctx.band.at,
                    }
            }),
        };
        if own && next != Some(cell.at) && position_dependent(&cell.text) {
            seeds.push(fid);
        }
        if let FKind::Shared { si, .. } = cell.kind {
            groups
                .entry((cell.sheet, si))
                .or_default()
                .push((cell, next, reads_band));
            continue;
        }
        if next.is_none() {
            continue;
        }
        if reads_band
            && !cell.text.is_empty()
            && let Some(text) = ctx.rewrite(
                &cell.text,
                Some(cell.sheet),
                None,
                part,
                &format!("{} formula", cell.at),
            )?
        {
            record(
                report,
                part,
                format!("{} formula", cell.at),
                &cell.text,
                &text,
            );
            edits
                .entry((cell.sheet, cell.at))
                .or_insert_with(FormulaEdit::default)
                .text = Some(text);
            seeds.push(fid);
        }
        if own && cell.kind == FKind::DataTable {
            let mut inputs = Vec::new();
            if let Some(doc) = wb.pkg.parsed_part(part)
                && let Some(node) = locate_formula(doc, wb, cell.sheet, cell.at)
            {
                for input in ["r1", "r2"] {
                    if let Some(before) = node.attr(attr(input)) {
                        let old = CellIndex::parse(before)?;
                        let new = ctx.band.cell(old)?.ok_or_else(|| Error::ReferenceDeleted {
                            part: part.into(),
                            location: format!("{} formula/@{input}", cell.at),
                        })?;
                        if old != new {
                            inputs.push((input, new.to_string()));
                        }
                    }
                }
            }
            if !inputs.is_empty() {
                edits
                    .entry((cell.sheet, cell.at))
                    .or_insert_with(FormulaEdit::default)
                    .inputs = inputs;
            }
        }
    }

    for ((sheet, si), mut group) in groups {
        if !group
            .iter()
            .any(|(cell, next, reads)| *reads || *next != Some(cell.at))
        {
            continue;
        }
        let part = wb.sheets[sheet].part.as_deref().expect("formula part");
        let master = lookup(sheet, si).and_then(|fid| wb.census.get(fid));
        let Some(master) =
            master.filter(|_| group.iter().all(|(cell, _, _)| !cell.ambiguous_group))
        else {
            return Err(Error::UnrewritableReference {
                part: part.into(),
                location: format!("shared formula {si}"),
            });
        };
        group.retain(|(_, next, _)| next.is_some());
        group.sort_by_key(|(cell, _, _)| cell.at);
        let Some(&(candidate, candidate_next, _)) = group
            .iter()
            .find(|(cell, _, _)| cell.at == master.at)
            .or(group.first())
        else {
            continue;
        };
        let candidate_next = candidate_next.expect("survivor");
        let mut texts = Vec::new();
        for (cell, next, _) in group {
            let own = translate_from(wb, &master.text, master.at, cell.at, part)?;
            let text = ctx
                .rewrite(
                    &own,
                    Some(sheet),
                    None,
                    part,
                    &format!("{} formula", cell.at),
                )?
                .unwrap_or(own);
            texts.push((cell, next.expect("survivor"), text));
        }
        let new_master = &texts
            .iter()
            .find(|(cell, _, _)| cell.at == candidate.at)
            .expect("candidate")
            .2;
        let mut retained = HashSet::new();
        let mut bounds = CellRange {
            first: candidate_next,
            last: candidate_next,
        };
        for (cell, next, text) in &texts {
            if translate_from(wb, new_master, candidate_next, *next, part)
                .is_ok_and(|expected| expected == *text)
            {
                retained.insert(cell.at);
                bounds = bounds.union(*next);
            }
        }
        for (cell, _, text) in texts {
            let stays = retained.len() > 1 && retained.contains(&cell.at);
            let is_master = stays && cell.at == candidate.at;
            let old_master = cell.at == master.at;
            let new_text = if !stays || is_master {
                text
            } else {
                String::new()
            };
            let new_ref = is_master.then(|| bounds.to_string());
            let old_ref = cell.range.map(|range| range.to_string());
            let change_ref = new_ref != old_ref;
            if new_text != cell.text || change_ref || !stays || is_master != old_master {
                if new_text != cell.text {
                    record(
                        report,
                        part,
                        format!("{} formula", cell.at),
                        &cell.text,
                        &new_text,
                    );
                }
                edits.insert(
                    (sheet, cell.at),
                    FormulaEdit {
                        text: (new_text != cell.text).then_some(new_text),
                        detach_shared: !stays,
                        reference: change_ref.then_some(new_ref),
                        ..FormulaEdit::default()
                    },
                );
            }
        }
    }
    Ok((edits, seeds, chain))
}

fn position_dependent(text: &str) -> bool {
    lex(text).is_ok_and(|tokens| tokens.iter().any(|token| {
        matches!(&token.kind, TokKind::Func(name) if matches!(name.as_str(), "ROW" | "COLUMN" | "CELL" | "ADDRESS"))
    }))
}

fn locate_formula<'a>(
    doc: &'a XmlDocument,
    wb: &Workbook,
    sheet: usize,
    cell: CellIndex,
) -> Option<&'a RawNode> {
    let located = crate::worksheet::locate(doc, wb.indexes.get(&sheet)?, "", cell).ok()?;
    doc.root()
        .elements()
        .find(|n| n.is(tag("sheetData")))?
        .indexed_elements()
        .find(|(i, _)| Some(*i) == located.row)?
        .1
        .indexed_elements()
        .find(|(i, _)| Some(*i) == located.cell)?
        .1
        .elements()
        .find(|n| n.is(tag("f")))
}

fn apply_formulas(doc: &mut XmlDocument, edits: &[([usize; 3], FormulaEdit)]) -> Result<()> {
    for (path, edit) in edits {
        let mut lens = doc
            .lens_mut_at::<sml::Cell>(path)?
            .into_child_by::<sml::CellFormula>(|_| true)
            .expect("planned formula");
        lens.preserve_attribute_spelling();
        if let Some(text) = &edit.text {
            lens.set_text(text)?;
        }
        if let Some(reference) = &edit.reference {
            if let Some(reference) = reference {
                lens.set_attr(attr("ref"), reference)?;
            } else {
                lens.remove_attr(attr("ref"))?;
            }
        }
        if edit.detach_shared {
            lens.remove_attr(attr("t"))?;
            lens.remove_attr(attr("si"))?;
        }
        for (name, value) in &edit.inputs {
            lens.set_attr(attr(name), value)?;
        }
    }
    Ok(())
}

fn formula_paths(
    doc: &XmlDocument,
    sheet: usize,
    edits: &mut FormulaEdits,
) -> Vec<([usize; 3], FormulaEdit)> {
    let mut paths = Vec::new();
    let Some((di, data)) = doc
        .root()
        .indexed_elements()
        .find(|(_, n)| n.is(tag("sheetData")))
    else {
        return paths;
    };
    for (ri, row) in data.indexed_elements().filter(|(_, n)| n.is(tag("row"))) {
        for (ci, cell) in row.indexed_elements().filter(|(_, n)| n.is(tag("c"))) {
            if cell.elements().any(|n| n.is(tag("f")))
                && let Some(at) = cell.attr(attr("r")).and_then(|s| CellIndex::parse(s).ok())
                && let Some(edit) = edits.remove(&(sheet, at))
            {
                paths.push(([di, ri, ci], edit));
            }
        }
    }
    paths
}

impl Workbook {
    /// Open a mutable view of the named worksheet.
    pub fn worksheet_mut(&mut self, name: &str) -> Result<Worksheet<'_>> {
        self.worksheet_by_name(name)
    }

    fn edit_grid(
        &mut self,
        sheet: usize,
        axis: Axis,
        shift: Shift,
        operation: &'static str,
    ) -> Result<StructuralEditReport> {
        let band = Band::new(axis, shift)?;
        self.sheet_structure_guard(operation)?;
        let part = self.sheets[sheet].part.clone().expect("worksheet part");
        let worksheet = self.pkg.xml_part(&part)?;
        if worksheet.root().elements().any(|node| {
            node.is(tag("sheetProtection"))
                && matches!(node.attr(attr("sheet")).map(str::trim), Some("1" | "true"))
                && !matches!(
                    node.attr(attr(operation)).map(str::trim),
                    Some("0" | "false")
                )
        }) {
            return Err(Error::Protected {
                scope: "worksheet",
                operation,
            });
        }
        let mut report = StructuralEditReport::default();
        let grid = GridPlan::validate(worksheet, &part, band, &mut report)?;
        let worksheets: Vec<_> = self
            .sheets
            .iter()
            .enumerate()
            .filter_map(|(i, entry)| {
                (entry.kind == SheetKind::Worksheet).then_some((i, entry.part.clone()?))
            })
            .collect();
        for (index, name) in &worksheets {
            self.pkg.xml_part(name)?;
            self.ensure_index(*index)?;
        }
        self.load_context(sheet)?;
        let context = &self.contexts[&sheet];
        for table in &context.tables {
            band.blocked(table.range, &table.part, &format!("table {}", table.name))?;
        }
        let sheet_names: Vec<_> = self.sheets.iter().map(|entry| entry.name.clone()).collect();
        let ctx = FormulaContext {
            sheets: &sheet_names,
            edited: sheet,
            band,
        };
        let mut change = Change::default();
        let book = self.workbook_part.clone();
        let mut updated_names = Vec::new();
        for (i, name, _, text) in defined_names(self.pkg.xml_part(&book)?) {
            let location = format!("defined name {name}");
            if let Some(new) = ctx.rewrite(&text, None, Some(&name), &book, &location)? {
                record(&mut report, &book, location, &text, &new);
                updated_names.push((i, new));
            }
        }
        if !updated_names.is_empty() {
            for (i, new) in updated_names {
                let mut names =
                    names_lens(change.doc(&mut self.pkg, &book)?)?.expect("defined names");
                names
                    .nth_child_mut_at::<sml::DefinedName>(i)
                    .expect("defined name")
                    .set_text(&new)?;
            }
        }
        let (mut formulas, seeds, chain) = plan_formulas(self, &ctx, &mut report)?;
        if let Some((index, _)) = formulas
            .keys()
            .find(|(index, _)| self.sheets[*index].kind != SheetKind::Worksheet)
        {
            return Err(Error::StructureBlocked {
                part: self.sheets[*index].part.clone().expect("formula part"),
                place: "formula on a non-worksheet sheet".into(),
            });
        }
        let mut patches = Vec::new();
        for (index, name) in worksheets {
            let doc = self.pkg.xml_part(&name)?;
            let formula = formula_paths(doc, index, &mut formulas);
            let features = features::plan(doc, &name, index, &ctx, &self.env, &mut report)?;
            if !formula.is_empty() || !features.is_empty() {
                patches.push((name, formula, features));
            }
        }
        if !formulas.is_empty() {
            return Err(Error::Malformed(
                "structural formula has no editable cell".into(),
            ));
        }
        features::objects(self, &part, &ctx, &mut change, &mut report)?;
        features::completeness(self, &ctx, &part)?;
        let mut deps = self.dependents_of_seeds(&seeds, vec![Band::seed_all(sheet)]);
        deps.formulas.retain(|&fid| {
            self.census
                .get(fid)
                .is_some_and(|f| f.sheet != sheet || band.cell(f.at).is_ok_and(|pos| pos.is_some()))
        });
        let live: HashSet<_> = deps.formulas.iter().copied().collect();
        deps.results.retain(|(fid, _)| live.contains(fid));
        deps.cells.retain(|&(index, cell)| {
            index != sheet || band.cell(cell).is_ok_and(|pos| pos.is_some())
        });
        self.check_write_dependents(&deps, &HashSet::new(), None)?;
        let strip = if self.policy == CalcPolicy::StripDependentCaches {
            self.plan_strip(&deps, &HashSet::new())?
        } else {
            Default::default()
        };
        if !self.calc.manual && !self.calc.full_calc_on_load && !deps.formulas.is_empty() {
            let mut lens = change
                .doc(&mut self.pkg, &book)?
                .lens_mut::<sml::Workbook>()?;
            lens.get_or_insert::<sml::CalculationProperties>()?
                .set_attr(attr("fullCalcOnLoad"), "1")?;
        }
        let delta = -(grid.deleted_strings + strip.shared_refs);
        if delta != 0 {
            self.shared_strings_active()?;
        }
        let mut registry = RegistryPlan::new(&self.pkg);
        for (part, doc) in change.docs {
            registry.stage_part(&self.pkg, &part, doc)?;
        }
        for (part, bytes) in change.bytes {
            registry.stage_part(&self.pkg, &part, bytes)?;
        }
        if chain && let Some(name) = &self.calc_chain {
            registry.remove_parts(&self.pkg, &[name.as_str()], &[REL_CALC_CHAIN])?;
            removed(&mut report, name, "calculation chain".into());
        }
        features::relationships(self, &ctx, &mut registry, &mut report)?;
        self.apply_registry_edit(registry, |wb| {
            wb.fault_point(Step::Strip)?;
            for (name, positions) in strip.sheets {
                strip_cells(wb.pkg.xml_part_mut(&name)?, &positions)?;
            }
            wb.fault_point(Step::HandOff)?;
            for (name, edits, features) in &patches {
                let doc = wb.pkg.xml_part_mut(name)?;
                apply_formulas(doc, edits)?;
                features.rewrite(doc)?;
            }
            wb.fault_point(Step::Target)?;
            grid.renumber(wb.pkg.xml_part_mut(&part)?)?;
            wb.fault_point(Step::LaterTarget)?;
            grid.remove(wb.pkg.xml_part_mut(&part)?)?;
            for (name, _, features) in patches {
                features.finish(wb.pkg.xml_part_mut(&name)?)?;
            }
            SharedStrings::apply(wb, &[], delta, delta != 0)?;
            wb.fault_point(Step::StructuralRebuild)?;
            wb.rebuild()
        })?;
        Ok(report)
    }
}

mod features;
