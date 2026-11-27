//! Phase4 part 1 (ADR-0014): merged cells (§3), the autofilter (§7) and sheet
//! protection (§11) on an opened worksheet.
//!
//! Every call follows ADR-0003 §3.1: everything that can refuse is checked
//! first without changing anything (`Err` ⇒ every byte unchanged), then the
//! edit is applied in place in a fixed order. A failure while applying is a
//! bug (the plan proved success); it poisons the workbook.

use ooxml_core::{ElementSpec, QName, RawNode};
use ooxml_schema::sml;

use crate::census::FKind;
use crate::edit::CellOp;
use crate::error::{Error, Result};
use crate::formula::precedents::{NameDef, Scope, fold};
use crate::protect::{MANAGED, MANAGED_WORKBOOK, SheetProtection, SheetProtectionModel, WorkbookProtection};
use crate::reference::{CellIndex, CellRange};
use crate::sheet_ops::quote_sheet;
use crate::workbook::Workbook;
use crate::worksheet::Worksheet;

const FILTER_DATABASE: &str = "_xlnm._FilterDatabase";
const COUNT: QName = QName::new("", "count");
const REF: QName = QName::new("", "ref");

impl CellRange {
    /// Do the two ranges share a cell?
    pub fn intersects(&self, other: &CellRange) -> bool {
        self.first.row <= other.last.row
            && other.first.row <= self.last.row
            && self.first.col <= other.last.col
            && other.first.col <= self.last.col
    }
}

fn absolute(r: &CellRange) -> String {
    let a = |c: CellIndex| format!("${}${}", CellIndex::column_name(c.col), c.row + 1);
    format!("{}:{}", a(r.first), a(r.last))
}

impl Workbook {
    /// Runs `apply` with ADR-0003 §3.1's poisoning: poisoned before, cleared
    /// on success; a failure (a bug after a successful plan) stays poisoned.
    pub(crate) fn poisoning(&mut self, apply: impl FnOnce(&mut Self) -> Result<()>) -> Result<()> {
        self.poisoned = Some("a range-feature edit was interrupted while being applied".into());
        let result = self.fault_point(crate::workbook::ApplyStep::RangeFeature).and_then(|()| apply(self));
        match result {
            Ok(()) => {
                self.poisoned = None;
                Ok(())
            }
            Err(e) => {
                let msg = e.to_string();
                self.poisoned = Some(msg.clone());
                Err(Error::Poisoned(msg))
            }
        }
    }

    fn sheet_part(&self, sheet: usize) -> String {
        self.sheets[sheet].part.clone().expect("worksheet")
    }

    // ---- merged cells (ADR-0014 §3) ----

    /// The refusals shared by `merge` and `merge_clearing`, and the
    /// non-anchor cells that hold a value or a formula.
    fn plan_merge(&mut self, sheet: usize, range: &str) -> Result<(CellRange, Vec<CellIndex>)> {
        self.check_editable()?;
        let r = CellRange::parse(range)?;
        if r.first == r.last {
            return Err(Error::MergeSingleCell(r.to_string()));
        }
        let ctx = self.contexts.get(&sheet).expect("context loaded with the worksheet");
        if let Some(m) = ctx.merges.iter().find(|m| m.intersects(&r)) {
            return Err(Error::MergeOverlap { range: r.to_string(), existing: m.to_string() });
        }
        if let Some(t) = ctx.tables.iter().find(|t| t.range.intersects(&r)) {
            return Err(Error::TableOverlap { range: r.to_string(), table: t.name.clone() });
        }
        if let Some(q) = &ctx.query_table {
            return Err(Error::QueryTableOverlap(q.clone()));
        }
        for part in ctx.pivots.clone() {
            // read lazily; a part that does not parse or has no location is
            // treated as overlapping (refusing is safe)
            let loc = match self.pkg.xml_part(&part) {
                Ok(doc) => doc.root().elements().find(|e| e.name().local() == "location").and_then(|l| l.attr(ooxml_core::QName::new("", "ref"))).map(str::to_owned),
                Err(_) => None,
            };
            if loc.as_deref().and_then(|l| CellRange::parse(l).ok()).is_none_or(|p| p.intersects(&r)) {
                return Err(Error::PivotOverlap { range: r.to_string(), location: loc.unwrap_or_else(|| format!("{part} (no readable location)")) });
            }
        }
        self.check_array_overlap(sheet, &r)?;
        if let Some(&(_, c)) = self.census.vm_cells.iter().find(|(s, c)| *s == sheet && r.contains(*c)) {
            return Err(Error::RichValueCell { cell: c.to_string() });
        }
        let filled = self.filled_cells(sheet, &r)?;
        let hidden: Vec<CellIndex> = filled.into_iter().filter(|&c| c != r.first).collect();
        // the mergeCell insertion
        let part = self.sheet_part(sheet);
        let doc = self.pkg.xml_part_mut(&part)?;
        let mut ws = doc.lens_mut::<sml::Worksheet>()?;
        match ws.child_mut::<sml::MergeCells>() {
            Some(mc) => mc.check_insert::<sml::MergeCell>()?,
            None => ws.check_insert::<sml::MergeCells>()?,
        }
        Ok((r, hidden))
    }

    /// Refuses a range that overlaps an array, data-table or dynamic-array
    /// anchor or result range (Phase1's guards).
    fn check_array_overlap(&self, sheet: usize, r: &CellRange) -> Result<()> {
        for (_, c) in self.census.alive().filter(|(_, c)| c.sheet == sheet) {
            let area = match c.kind {
                // a spill (dynamic array) is an `Array` with `cm` (census)
                FKind::Array | FKind::DataTable => c.range,
                _ => continue,
            }
            .unwrap_or(CellRange { first: c.at, last: c.at });
            if !area.intersects(r) {
                continue;
            }
            if r.contains(c.at) {
                return Err(Error::ArrayAnchor { cell: c.at.to_string() });
            }
            let first = CellIndex { row: area.first.row.max(r.first.row), col: area.first.col.max(r.first.col) };
            return Err(Error::ArrayResultCell { cell: first.to_string(), anchor: c.at.to_string() });
        }
        Ok(())
    }

    /// Cells of `r` that hold a value or a formula (`v`, `is` or `f`).
    fn filled_cells(&mut self, sheet: usize, r: &CellRange) -> Result<Vec<CellIndex>> {
        self.ensure_index(sheet)?;
        let part = self.sheet_part(sheet);
        let doc = self.pkg.xml_part(&part)?;
        let index = self.indexes.get(&sheet).expect("built");
        let rows = index.target_rows(&part)?;
        let Some(sd) = crate::index::sheet_data(doc) else { return Ok(Vec::new()) };
        let lo = rows.partition_point(|e| e.row < r.first.row + 1);
        let hi = rows.partition_point(|e| e.row <= r.last.row + 1);
        let mut out = Vec::new();
        for e in &rows[lo..hi] {
            let row = sd.children()[e.item].as_element().expect("indexed row");
            for &(col, ci) in index.target_cells(e, &part)? {
                if !(r.first.col..=r.last.col).contains(&col) {
                    continue;
                }
                let c = row.children()[ci].as_element().expect("indexed cell");
                let filled = c.elements().any(|x| x.is(sml::CellValue::QNAME) || x.is(sml::InlineString::QNAME) || x.is(sml::CellFormula::QNAME));
                if filled {
                    out.push(CellIndex { row: e.row - 1, col });
                }
            }
        }
        Ok(out)
    }

    /// Inserts the `mergeCell` (container created in its slot when absent)
    /// and updates `count` and the session's merge list.
    fn apply_merge(&mut self, sheet: usize, r: CellRange) -> Result<()> {
        let part = self.sheet_part(sheet);
        let doc = self.pkg.xml_part_mut(&part)?;
        let mut ws = doc.lens_mut::<sml::Worksheet>()?;
        let mut mc = ws.get_or_insert::<sml::MergeCells>()?;
        mc.insert::<sml::MergeCell>()?.set_attr(REF, &r.to_string())?;
        let n = mc.node().elements().filter(|e| e.is(sml::MergeCell::QNAME)).count();
        mc.set_attr(COUNT, &n.to_string())?;
        self.contexts.get_mut(&sheet).expect("context").merges.push(r);
        Ok(())
    }

    pub(crate) fn merge(&mut self, sheet: usize, range: &str) -> Result<()> {
        let (r, hidden) = self.plan_merge(sheet, range)?;
        if !hidden.is_empty() {
            return Err(Error::MergeWouldHideValues { range: r.to_string(), cells: hidden.iter().map(ToString::to_string).collect() });
        }
        self.poisoning(|wb| wb.apply_merge(sheet, r))
    }

    fn merge_preserving_covered_values(&mut self, sheet: usize, range: &str) -> Result<()> {
        let (range, _) = self.plan_merge(sheet, range)?;
        self.poisoning(|wb| wb.apply_merge(sheet, range))
    }

    pub(crate) fn merge_clearing(&mut self, sheet: usize, range: &str) -> Result<()> {
        let (r, hidden) = self.plan_merge(sheet, range)?;
        // Each clearing is a Phase1 removal. Every one is checked before the
        // first is applied; a shared-formula master is refused (its hand-off
        // to a member that is cleared next could not be checked in advance).
        for &at in &hidden {
            if let Some(fid) = self.census.at(sheet, at)
                && matches!(self.census.get(fid).expect("alive").kind, FKind::Shared { master: true, .. })
            {
                return Err(Error::SharedFormulaMaster {
                    cell: at.to_string(),
                    reason: "merge_clearing does not clear a shared-formula master".into(),
                });
            }
            self.check_edit_cell(sheet, at, CellOp::Clear)?;
        }
        for (k, &at) in hidden.iter().enumerate() {
            if let Err(e) = self.edit_cell(sheet, at, CellOp::Clear) {
                if k == 0 && !matches!(e, Error::Poisoned(_)) {
                    return Err(e); // refused before anything changed
                }
                let msg = format!("merge_clearing stopped after clearing {k} cells: {e}");
                self.poisoned = Some(msg.clone());
                return Err(Error::Poisoned(msg));
            }
        }
        self.poisoning(|wb| wb.apply_merge(sheet, r))
    }

    pub(crate) fn unmerge(&mut self, sheet: usize, range: &str) -> Result<()> {
        self.check_editable()?;
        let r = CellRange::parse(range)?;
        self.remove_merges(sheet, r, false)
    }

    fn remove_merges(&mut self, sheet: usize, range: CellRange, intersecting: bool) -> Result<()> {
        let matches = |candidate: &CellRange| {
            if intersecting { candidate.intersects(&range) } else { *candidate == range }
        };
        let limit = if intersecting { usize::MAX } else { 1 };
        let expected = self.contexts.get(&sheet).expect("context").merges.iter()
            .filter(|candidate| matches(candidate)).take(limit).count();
        if expected == 0 {
            return if intersecting { Ok(()) } else { Err(Error::NoSuchMerge(range.to_string())) };
        }
        let part = self.sheet_part(sheet);
        let missing = || Error::Malformed(format!("{part}: merge {range} not in the tree"));
        let doc = self.pkg.xml_part(&part)?;
        let ws = doc.lens::<sml::Worksheet>()?;
        let mc = ws.child::<sml::MergeCells>().ok_or_else(missing)?;
        let items: Vec<_> = mc.raw().indexed_elements()
            .filter(|(_, e)| e.is(sml::MergeCell::QNAME) && e.attr(REF).and_then(|v| CellRange::parse(v).ok()).is_some_and(|r| matches(&r)))
            .map(|(index, _)| index).take(limit).collect();
        if items.len() != expected {
            return Err(missing());
        }
        let remaining = mc.raw().elements().filter(|e| e.is(sml::MergeCell::QNAME)).count() - items.len();
        self.poisoning(|wb| {
            let doc = wb.pkg.xml_part_mut(&part)?;
            let mut ws = doc.lens_mut::<sml::Worksheet>()?;
            if remaining == 0 {
                ws.remove::<sml::MergeCells>();
            } else {
                let mut mc = ws.child_mut::<sml::MergeCells>().expect("planned");
                for index in items.into_iter().rev() {
                    mc.remove_at(index);
                }
                mc.set_attr(COUNT, &remaining.to_string())?;
            }
            let merges = &mut wb.contexts.get_mut(&sheet).expect("context").merges;
            if intersecting {
                merges.retain(|candidate| !matches(candidate));
            } else {
                let at = merges.iter().position(matches).expect("checked above");
                merges.remove(at);
            }
            Ok(())
        })
    }

    // ---- autofilter (ADR-0014 §7) ----

    pub(crate) fn set_autofilter(&mut self, sheet: usize, range: &str) -> Result<()> {
        self.check_editable()?;
        let r = CellRange::parse(range)?;
        let ctx = self.contexts.get(&sheet).expect("context");
        if let Some(t) = ctx.tables.iter().find(|t| t.range.intersects(&r)) {
            return Err(Error::TableOverlap { range: r.to_string(), table: t.name.clone() });
        }
        // the sheet's autoFilter
        let part = self.sheet_part(sheet);
        {
            let doc = self.pkg.xml_part_mut(&part)?;
            let mut ws = doc.lens_mut::<sml::Worksheet>()?;
            match ws.child_mut::<sml::AutoFilter>() {
                Some(af) => {
                    let busy = af.node().elements().any(|e| e.is(sml::FilterColumn::QNAME) || e.is(sml::SortState::QNAME));
                    if busy {
                        return Err(Error::AutoFilterExists);
                    }
                }
                None => ws.check_insert::<sml::AutoFilter>()?,
            }
        }
        // the sheet-local _xlnm._FilterDatabase
        let local = sheet as u32;
        let text = format!("{}!{}", quote_sheet(&self.sheets[sheet].name), absolute(&r));
        let wb_part = self.workbook_part.clone();
        let existing: Vec<usize> = {
            let doc = self.pkg.xml_part(&wb_part)?;
            let wb = doc.lens::<sml::Workbook>()?;
            wb.child::<sml::DefinedNames>()
                .map(|dn| {
                    dn.raw()
                        .indexed_elements()
                        .filter(|(_, e)| is_filter_database(e, local))
                        .map(|(i, _)| i)
                        .collect()
                })
                .unwrap_or_default()
        };
        if existing.len() > 1 {
            return Err(Error::DuplicateBuiltInName { name: FILTER_DATABASE.into() });
        }
        if let Some((_, c)) = self.census.alive().find(|(_, c)| c.text.to_ascii_lowercase().contains("_filterdatabase")) {
            return Err(Error::NameInUse { name: FILTER_DATABASE.into(), cell: format!("{}!{}", self.sheets[c.sheet].name, c.at) });
        }
        // a defined name that refers to it (formulas using that name would
        // need re-analysis): refused as well
        {
            let doc = self.pkg.xml_part(&wb_part)?;
            let wb = doc.lens::<sml::Workbook>()?;
            if let Some(dn) = wb.child::<sml::DefinedNames>()
                && let Some(n) = dn.raw().elements().find(|e| {
                    e.is(sml::DefinedName::QNAME)
                        && !e.attr(sml::DefinedName::ATTR_NAME).is_some_and(|n| n.eq_ignore_ascii_case(FILTER_DATABASE))
                        && e.text().to_ascii_lowercase().contains("_filterdatabase")
                })
            {
                let name = n.attr(sml::DefinedName::ATTR_NAME).unwrap_or_default();
                return Err(Error::NameInUse { name: FILTER_DATABASE.into(), cell: format!("defined name {name}") });
            }
        }
        if existing.is_empty() {
            let doc = self.pkg.xml_part_mut(&wb_part)?;
            let mut wb = doc.lens_mut::<sml::Workbook>()?;
            match wb.child_mut::<sml::DefinedNames>() {
                Some(dn) => dn.check_insert::<sml::DefinedName>()?,
                None => wb.check_insert::<sml::DefinedNames>()?,
            }
        }
        if let Some((offset, ch)) = ooxml_core::value::first_invalid_xml_char(&text) {
            return Err(ooxml_core::Error::InvalidXmlChar { ch, offset }.into());
        }

        self.poisoning(|wb| {
            // workbook.xml: the name
            {
                let doc = wb.pkg.xml_part_mut(&wb_part)?;
                let mut w = doc.lens_mut::<sml::Workbook>()?;
                let mut dn = w.get_or_insert::<sml::DefinedNames>()?;
                let item = match existing.first() {
                    Some(&i) => i,
                    None => {
                        let mut n = dn.insert::<sml::DefinedName>()?;
                        n.set_attr(sml::DefinedName::ATTR_NAME, FILTER_DATABASE)?;
                        n.set_attr(sml::DefinedName::ATTR_LOCAL_SHEET_ID, &local.to_string())?;
                        *n.path().last().expect("a child")
                    }
                };
                let mut name = dn.into_child_at::<sml::DefinedName>(item).expect("planned");
                if name.node().attr(sml::DefinedName::ATTR_HIDDEN).is_none_or(|v| v != "1" && v != "true") {
                    name.set_attr(sml::DefinedName::ATTR_HIDDEN, "1")?;
                }
                name.set_text(&text)?;
            }
            // the sheet's autoFilter
            {
                let doc = wb.pkg.xml_part_mut(&part)?;
                let mut ws = doc.lens_mut::<sml::Worksheet>()?;
                let mut af = ws.get_or_insert::<sml::AutoFilter>()?;
                af.set_attr(REF, &r.to_string())?;
            }
            // Phase1's view of the names
            let lc = fold(FILTER_DATABASE);
            wb.env.names.retain(|n| !(n.name_lc == lc && n.scope == Scope::Local(sheet)));
            wb.env.names.push(NameDef::new(FILTER_DATABASE, Scope::Local(sheet), &text));
            wb.graph = None;
            Ok(())
        })
    }

    pub(crate) fn remove_autofilter(&mut self, sheet: usize) -> Result<bool> {
        self.check_editable()?;
        let part = self.sheet_part(sheet);
        let doc = self.pkg.xml_part(&part)?;
        let ws = doc.lens::<sml::Worksheet>()?;
        let Some(af) = ws.child::<sml::AutoFilter>() else { return Ok(false) };
        if af.raw().elements().any(|e| e.is(sml::FilterColumn::QNAME)) {
            return Err(Error::FilterActive);
        }
        let filter_mode = ws.child::<sml::SheetProperties>().is_some_and(|p| p.raw().attr(sml::SheetProperties::ATTR_FILTER_MODE).is_some());
        self.poisoning(|wb| {
            let doc = wb.pkg.xml_part_mut(&part)?;
            let mut ws = doc.lens_mut::<sml::Worksheet>()?;
            ws.remove::<sml::AutoFilter>();
            if filter_mode {
                ws.child_mut::<sml::SheetProperties>().expect("planned").remove_attr(sml::SheetProperties::ATTR_FILTER_MODE)?;
            }
            Ok(())
        })?;
        Ok(true)
    }

    // ---- protection (ADR-0014 §11) ----

    pub(crate) fn protect_sheet(&mut self, sheet: usize, p: &SheetProtection) -> Result<()> {
        self.check_editable()?;
        self.write_sheet_protection(sheet, &p.attributes()?)
    }

    fn write_sheet_protection(&mut self, sheet: usize, attrs: &[(&'static str, String)]) -> Result<()> {
        for (_, value) in attrs {
            if let Some((offset, ch)) = ooxml_core::value::first_invalid_xml_char(value) {
                return Err(ooxml_core::Error::InvalidXmlChar { ch, offset }.into());
            }
        }
        let part = self.sheet_part(sheet);
        {
            let doc = self.pkg.xml_part_mut(&part)?;
            let mut ws = doc.lens_mut::<sml::Worksheet>()?;
            if ws.child_mut::<sml::SheetProtection>().is_none() {
                ws.check_insert::<sml::SheetProtection>()?;
            }
        }
        self.poisoning(|wb| {
            let doc = wb.pkg.xml_part_mut(&part)?;
            let mut ws = doc.lens_mut::<sml::Worksheet>()?;
            let mut sp = ws.get_or_insert::<sml::SheetProtection>()?;
            // Every hash and flag attribute is replaced, so a legacy
            // `password` never coexists with a SHA hash (review M4); unknown
            // attributes are kept.
            for name in MANAGED {
                sp.remove_attr(QName::new("", name))?;
            }
            for (name, value) in attrs {
                sp.set_attr(QName::new("", name), value)?;
            }
            Ok(())
        })
    }

    pub(crate) fn unprotect_sheet(&mut self, sheet: usize) -> Result<bool> {
        self.check_editable()?;
        let part = self.sheet_part(sheet);
        let present = self.pkg.xml_part(&part)?.lens::<sml::Worksheet>()?.child::<sml::SheetProtection>().is_some();
        if !present {
            return Ok(false);
        }
        self.poisoning(|wb| {
            let doc = wb.pkg.xml_part_mut(&part)?;
            doc.lens_mut::<sml::Worksheet>()?.remove::<sml::SheetProtection>();
            Ok(())
        })?;
        Ok(true)
    }
}

impl Workbook {
    /// Protects the workbook (`workbookProtection` in `workbook.xml`,
    /// ADR-0014 §11): every hash and lock attribute is replaced (no legacy
    /// `workbookPassword` stays next to a SHA hash); `revisions*` and
    /// `lockRevision` are kept. Errors: [`Error::InvalidSpinCount`],
    /// [`Error::Random`], [`Error::Poisoned`].
    pub fn protect_workbook(&mut self, protection: &WorkbookProtection) -> Result<()> {
        self.check_editable()?;
        let attrs = protection.attributes()?;
        let part = self.workbook_part.clone();
        {
            let doc = self.pkg.xml_part_mut(&part)?;
            let mut wb = doc.lens_mut::<sml::Workbook>()?;
            if wb.child_mut::<sml::WorkbookProtection>().is_none() {
                wb.check_insert::<sml::WorkbookProtection>()?;
            }
        }
        self.poisoning(|wb| {
            let doc = wb.pkg.xml_part_mut(&part)?;
            let mut w = doc.lens_mut::<sml::Workbook>()?;
            let mut p = w.get_or_insert::<sml::WorkbookProtection>()?;
            for name in MANAGED_WORKBOOK {
                p.remove_attr(QName::new("", name))?;
            }
            for (name, value) in &attrs {
                p.set_attr(QName::new("", name), value)?;
            }
            Ok(())
        })
    }

    /// Removes `workbookProtection`; `Ok(false)` if there is none.
    pub fn unprotect_workbook(&mut self) -> Result<bool> {
        self.check_editable()?;
        let part = self.workbook_part.clone();
        if self.pkg.xml_part(&part)?.lens::<sml::Workbook>()?.child::<sml::WorkbookProtection>().is_none() {
            return Ok(false);
        }
        self.poisoning(|wb| {
            wb.pkg.xml_part_mut(&part)?.lens_mut::<sml::Workbook>()?.remove::<sml::WorkbookProtection>();
            Ok(())
        })?;
        Ok(true)
    }
}

fn is_filter_database(e: &RawNode, local: u32) -> bool {
    e.is(sml::DefinedName::QNAME)
        && e.attr(sml::DefinedName::ATTR_NAME).is_some_and(|n| n.eq_ignore_ascii_case(FILTER_DATABASE))
        && e.attr(sml::DefinedName::ATTR_LOCAL_SHEET_ID).and_then(|v| v.parse::<u32>().ok()) == Some(local)
}

impl Worksheet<'_> {
    /// The sheet's merged ranges, as of this session.
    pub fn merged_ranges(&self) -> Vec<CellRange> {
        self.wb.contexts.get(&self.sheet).map(|c| c.merges.clone()).unwrap_or_default()
    }

    /// Merges `range` (`"B2:D4"`, at least two cells; ADR-0014 §3).
    ///
    /// Refused, with every byte unchanged: a single cell
    /// ([`Error::MergeSingleCell`]); overlap with a merged range
    /// ([`Error::MergeOverlap`]), a table ([`Error::TableOverlap`]), a
    /// PivotTable ([`Error::PivotOverlap`]), a sheet with a query table
    /// ([`Error::QueryTableOverlap`]), an array, data-table or spill range
    /// ([`Error::ArrayAnchor`], [`Error::ArrayResultCell`]) or a rich-value
    /// cell ([`Error::RichValueCell`]); and **any value or formula in a
    /// non-anchor cell** ([`Error::MergeWouldHideValues`]): use
    /// [`Self::merge_clearing`] to clear them. The anchor (top-left) keeps
    /// its content and stays writable; the other cells refuse value writes.
    pub fn merge(&mut self, range: &str) -> Result<()> {
        self.wb.merge(self.sheet, range)
    }

    /// Clears the values and formulas of the non-anchor cells, then merges
    /// (what Excel does after its warning). Each clearing is a Phase1
    /// removal: dependents lose their cached values and `calcChain.xml` goes
    /// when a formula is removed. Styles of the cleared cells stay. Refuses
    /// what [`Self::merge`] refuses (except hidden values), and a
    /// shared-formula master among the cells to clear.
    pub fn merge_clearing(&mut self, range: &str) -> Result<()> {
        self.wb.merge_clearing(self.sheet, range)
    }

    /// Adds a merged rectangle while retaining existing covered cell contents.
    ///
    /// This supports exporting models that store a merge independently of
    /// the cell records. Covered cells remain protected from later value edits.
    pub fn merge_preserving_covered_values(&mut self, range: &str) -> Result<()> {
        self.wb.merge_preserving_covered_values(self.sheet, range)
    }

    /// Copies a merge member's effective cell format to its still-linked peers.
    pub fn copy_merge_style(
        &mut self,
        range: CellRange,
        source: CellIndex,
        detached: &std::collections::HashSet<CellIndex>,
    ) -> Result<()> {
        use crate::edit::Extra;
        use crate::registry::styles::StyleRequest;

        let effective_style = |row, col| {
            if let Some(style) = self.effective_cell_style_index(row, col)? {
                return Ok(style);
            }
            match self.row_style_index(row)? {
                Some(style) => Ok(style),
                None => self.column_style_index(col),
            }
        };
        let anchor = range.first;
        let style = effective_style(source.row, source.col)?;
        let linked = |at| at != source && !detached.contains(&at);
        let mut writes = Vec::new();
        let mut add_style = |at| {
            writes.push((self.sheet, at, CellOp::StyleOnly, Extra {
                style: Some(StyleRequest::Index(style)),
                ..Extra::default()
            }));
        };
        let doc = self.wb.pkg.parsed_part(self.part_name()).expect("worksheet is parsed");
        let has_inherited_style = doc.root().elements().any(|group| {
            if group.is(sml::SheetData::QNAME) {
                group.elements().any(|row| row.is(sml::Row::QNAME) && row.attr(sml::Row::ATTR_S).is_some())
            } else if group.is(sml::Columns::QNAME) {
                group.elements().any(|col| col.attr(QName::new("", "style")).is_some())
            } else {
                false
            }
        });
        if style == 0 && !has_inherited_style {
            for cell in self.cells_before_row(range.last.row + 1) {
                let cell = cell?;
                let at = cell.index();
                if linked(at) && range.contains(at) && cell.style_index()? != 0 {
                    add_style(at);
                }
            }
        } else {
            for row in anchor.row..=range.last.row {
                for col in anchor.col..=range.last.col {
                    let at = CellIndex { row, col };
                    if linked(at) && effective_style(row, col)? != style {
                        add_style(at);
                    }
                }
            }
        }
        if writes.is_empty() {
            return Ok(());
        }
        let plan = self.wb.plan_writes(writes)?;
        self.wb.apply_writes(plan)
    }

    /// Removes the merged range equal to `range` ([`Error::NoSuchMerge`]
    /// otherwise). Values that the merge hid in non-anchor cells become
    /// visible again.
    pub fn unmerge(&mut self, range: &str) -> Result<()> {
        self.wb.unmerge(self.sheet, range)
    }

    /// Removes all merged ranges that intersect the selection.
    pub fn unmerge_intersecting(&mut self, selection: CellRange) -> Result<()> {
        self.wb.check_editable()?;
        self.wb.remove_merges(self.sheet, selection, true)
    }

    /// Restores inherited formats on followers of merges intersecting the selection.
    pub fn reset_merge_follower_styles(&mut self, selection: CellRange) -> Result<()> {
        use crate::edit::Extra;
        use crate::registry::styles::StyleRequest;

        let merges: Vec<_> = self.merged_ranges().into_iter()
            .filter(|range| range.intersects(&selection))
            .collect();
        let Some(last_row) = merges.iter().map(|range| range.last.row).max() else {
            return Ok(());
        };
        // Explicit formats need a reset. Unstored followers need a write only
        // when an inherited row style must be combined with a column style.
        let mut requests = std::collections::BTreeMap::new();
        for cell in self.cells_before_row(last_row + 1) {
            let cell = cell?;
            if cell.raw().attr(sml::Cell::ATTR_S).is_none() {
                continue;
            }
            let at = cell.index();
            if !merges.iter().any(|range| at != range.first && range.contains(at)) {
                continue;
            }
            let row = self.row_style_index(at.row)?;
            let column = self.column_style_index(at.col)?;
            let inherited = row.unwrap_or(column);
            let request = match row {
                Some(row) if column != 0 && column != row => StyleRequest::RowWithColumnDefaults { row, column },
                _ if cell.style_index()? != inherited => StyleRequest::Index(inherited),
                _ => continue,
            };
            requests.insert((at.row, at.col), request);
        }

        let mut column_styles = std::collections::HashMap::new();
        for row in self.row_numbers()?.map(|row| row - 1).take_while(|&row| row <= last_row) {
            let mut covering = merges.iter().filter(|range| range.first.row <= row && range.last.row >= row).peekable();
            if covering.peek().is_none() {
                continue;
            }
            let Some(row_style) = self.row_style_index(row)? else {
                continue;
            };
            for range in covering {
                for col in range.first.col..=range.last.col {
                    if range.first == (CellIndex { row, col }) {
                        continue;
                    }
                    let column = match column_styles.entry(col) {
                        std::collections::hash_map::Entry::Occupied(entry) => *entry.get(),
                        std::collections::hash_map::Entry::Vacant(entry) => {
                            *entry.insert(self.column_style_index(col)?)
                        }
                    };
                    if column != 0 && column != row_style {
                        requests.entry((row, col)).or_insert(StyleRequest::RowWithColumnDefaults {
                            row: row_style,
                            column,
                        });
                    }
                }
            }
        }
        if requests.is_empty() {
            return Ok(());
        }
        let writes = requests.into_iter().map(|((row, col), request)| {
            (self.sheet, CellIndex { row, col }, CellOp::StyleOnly, Extra {
                style: Some(request),
                ..Extra::default()
            })
        }).collect();
        let plan = self.wb.plan_writes(writes)?;
        self.wb.apply_writes(plan)
    }

    /// Sets the sheet's autofilter range (ADR-0014 §7): a new `autoFilter`,
    /// or the `ref` of an existing one without criteria or sort
    /// ([`Error::AutoFilterExists`] otherwise). The sheet-local
    /// `_xlnm._FilterDatabase` name that Excel keeps is reused (its text
    /// rewritten, `hidden="1"`), or created. Refused: overlap with a table
    /// ([`Error::TableOverlap`]), two such names
    /// ([`Error::DuplicateBuiltInName`]), a formula that uses the name
    /// ([`Error::NameInUse`]).
    pub fn set_autofilter(&mut self, range: &str) -> Result<()> {
        self.wb.set_autofilter(self.sheet, range)
    }

    /// Removes the sheet's autofilter; `Ok(false)` if there is none. An
    /// active filter (with criteria; rows may be hidden by it) is refused
    /// ([`Error::FilterActive`]). The `_xlnm._FilterDatabase` name and the
    /// sheet's `sortState` are kept, as Excel keeps them.
    pub fn remove_autofilter(&mut self) -> Result<bool> {
        self.wb.remove_autofilter(self.sheet)
    }

    /// Reads stored worksheet protection attributes, if a protection element is present.
    pub fn sheet_protection_attributes(&self) -> Result<Option<Vec<(String, String)>>> {
        let doc = self.wb.pkg.parsed_part(self.part_name()).expect("worksheet is parsed");
        let worksheet = doc.lens::<sml::Worksheet>()?;
        Ok(worksheet.child::<sml::SheetProtection>().map(|protection| {
            protection.raw().attrs().iter().filter(|attr| attr.name().ns().is_empty())
                .map(|attr| (attr.name().local().to_owned(), attr.value().to_owned()))
                .collect()
        }))
    }

    /// Protects the sheet (ADR-0014 §11). An existing `sheetProtection` has
    /// every hash and flag attribute replaced (a legacy `password` never
    /// stays next to a SHA-512 hash); unknown attributes are kept. Sheet
    /// protection is an Excel UI setting: it does not stop edits through
    /// this crate. Errors: [`Error::InvalidSpinCount`], [`Error::Random`].
    pub fn protect_sheet(&mut self, protection: &SheetProtection) -> Result<()> {
        self.wb.protect_sheet(self.sheet, protection)
    }

    /// Replaces the worksheet model's stored protection flags and hash.
    pub fn replace_sheet_protection(&mut self, model: &SheetProtectionModel<'_>) -> Result<()> {
        self.wb.check_editable()?;
        let attrs = model.attributes();
        if attrs.is_empty() {
            return self.unprotect_sheet().map(drop);
        }
        self.wb.write_sheet_protection(self.sheet, &attrs)
    }

    /// Removes the sheet's protection; `Ok(false)` if it had none.
    pub fn unprotect_sheet(&mut self) -> Result<bool> {
        self.wb.unprotect_sheet(self.sheet)
    }
}
