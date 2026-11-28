//! [`Worksheet`], [`CellRef`] (read) and [`CellMut`] (write).

use ooxml_core::name::MC_ALTERNATE_CONTENT;
use ooxml_core::{Content, ElementSpec, EnumValue, QName, RawNode, XmlDocument};
use ooxml_schema::sml::{self, CellValues, TextView};

use crate::edit::{CellOp, Extra};
use crate::index::{Located, SheetIndex, located_cell, sheet_data};
use crate::error::{Error, Result};
use crate::escape;
use crate::reference::{CellIndex, CellRange};
use crate::workbook::Workbook;

/// A worksheet of a [`Workbook`], borrowed from it mutably: an edit can
/// reach other sheets, `workbook.xml` and the package (ADR-0003 §6).
///
/// Rows and columns are 0-based (`A1` = `(0, 0)`).
///
/// # Wrapped worksheet grids
///
/// Cell reads (including iteration), cell writes (including batches and
/// style changes), row-property setters, merges, and hyperlinks that write
/// cell text can return [`Error::Core`] with
/// [`UnresolvedSibling`](ooxml_core::Error::UnresolvedSibling) for `sheetData`.
/// This occurs when a SpreadsheetML `sheetData`, `row`, or `c` appears
/// outside its expected parent (`worksheet`, `sheetData`, or `row`,
/// respectively), including inside markup compatibility wrappers, or when
/// the worksheet has multiple direct `sheetData` children.
/// `mc:AlternateContent` directly under `sheetData` or `row` is handled by
/// the existing position-specific checks.
/// Edits that need to strip dependent formula caches on an affected sheet
/// can return the same error even when the edit targets a different sheet.
#[derive(Debug)]
pub struct Worksheet<'wb> {
    pub(crate) wb: &'wb mut Workbook,
    pub(crate) sheet: usize,
    pub(crate) workbook_view_id: u32,
}

/// A decoded cell value.
#[derive(Clone, Debug, PartialEq)]
pub enum CellValue {
    /// No value (`<c r="A1" s="1"/>`, or a value-less cell of any type).
    Empty,
    /// A number (`t` absent or `n`).
    Number(f64),
    /// Text: a shared string (`t="s"`), an inline string (`t="inlineStr"`,
    /// rich runs concatenated) or a formula's string result (`t="str"`).
    /// `_xHHHH_` escapes are decoded.
    String(String),
    /// `t="b"`.
    Bool(bool),
    /// `t="e"`: the error text, e.g. `#DIV/0!`.
    Error(String),
    /// `t="d"`: an ISO 8601 date/time, as written.
    Date(String),
}

/// A read-only view of one existing cell.
#[derive(Clone, Debug)]
pub struct CellRef<'a> {
    at: CellIndex,
    view: sml::CellView<'a>,
    sst: Option<&'a [String]>,
    workbook: &'a Workbook,
}

/// Concatenates `t` texts (rich runs included) the way calamine reads them:
/// a `t` that does not itself carry `xml:space="preserve"` has its outer
/// ASCII whitespace (space, tab, CR, LF) removed. Like calamine, only the
/// `t`'s own attribute counts: an `xml:space` inherited from `si`/`is`/`r`
/// is ignored (review round 1, F4). `_xHHHH_` escapes are decoded.
pub(crate) fn text_of<'a>(t: Option<TextView<'a>>, runs: impl Iterator<Item = Option<TextView<'a>>>) -> String {
    let mut out = String::new();
    for t in t.into_iter().chain(runs.flatten()) {
        let raw = t.text();
        let preserve = t.0.raw().xml_space_preserve() == Some(true);
        let raw = if preserve { &raw[..] } else { raw.trim_matches([' ', '\t', '\r', '\n']) };
        out.push_str(&escape::decode(raw));
    }
    out
}

impl<'a> CellRef<'a> {
    /// Resolves a local rich-value image stored in this cell.
    pub fn image(&self) -> Result<Option<crate::CellImage>> {
        crate::images::cell_image(&self.workbook.pkg, &self.workbook.workbook_part, self.raw())
    }

    /// The cell's zero-based position from the worksheet index.
    pub fn index(&self) -> CellIndex {
        self.at
    }

    /// The `r` attribute (e.g. `B2`).
    pub fn reference(&self) -> &'a str {
        self.view.cell_reference().unwrap_or_default()
    }

    /// The style index (`s`, default 0).
    pub fn style_index(&self) -> Result<u32> {
        Ok(self.view.style_index().map_err(ooxml_core::Error::from)?)
    }

    /// The `t` attribute (default `n`).
    pub fn data_type(&self) -> EnumValue<'a, CellValues> {
        self.view.data_type()
    }

    /// The shared-string index of a `t="s"` cell (its `v`), if it parses.
    pub fn shared_string_index(&self) -> Option<u32> {
        if !matches!(self.data_type(), EnumValue::Known(CellValues::SharedString)) {
            return None;
        }
        ooxml_core::value::parse_u32(&self.view.cell_value()?.text())
    }

    /// The formula text, if the cell has an `f` element.
    pub fn formula(&self) -> Option<String> {
        self.view.cell_formula().map(|f| f.text().into_owned())
    }

    /// The expression of an ordinary formula without cell metadata.
    pub fn ordinary_formula(&self) -> Option<String> {
        if self.raw().attr(QName::new("", "vm")).is_some() ||
            self.raw().attr(QName::new("", "cm")).is_some() {
            return None;
        }
        let formula = self.view.cell_formula()?;
        match formula.raw().attr(QName::new("", "t")) {
            None | Some("normal") => Some(formula.text().into_owned()),
            _ => None,
        }
    }

    /// The raw element.
    pub fn raw(&self) -> &'a RawNode {
        self.view.raw()
    }

    /// The decoded value. Shared strings are resolved; a formula cell
    /// returns its cached result.
    pub fn value(&self) -> Result<CellValue> {
        let bad = |message: String| Error::InvalidValue { cell: self.reference().to_owned(), message };
        let v = self.view.cell_value().map(|v| v.text());
        let t = self.data_type();
        if let EnumValue::Known(CellValues::InlineString) = t {
            return Ok(match self.view.inline_string() {
                Some(is) => CellValue::String(text_of(is.text(), is.run_iter().map(|r| r.text()))),
                None => CellValue::Empty,
            });
        }
        let Some(v) = v else { return Ok(CellValue::Empty) };
        Ok(match t {
            EnumValue::Known(CellValues::Number) => CellValue::Number(
                ooxml_core::value::parse_f64(&v).ok_or_else(|| bad(format!("{v:?} is not a number")))?,
            ),
            EnumValue::Known(CellValues::SharedString) => {
                let i: usize = ooxml_core::value::parse_u32(&v)
                    .ok_or_else(|| bad(format!("shared-string index {v:?}")))? as usize;
                let sst = self.sst.ok_or_else(|| bad("no shared-string table".into()))?;
                CellValue::String(sst.get(i).ok_or_else(|| bad(format!("shared-string index {i} out of range")))?.clone())
            }
            EnumValue::Known(CellValues::Boolean) => CellValue::Bool(
                ooxml_core::value::parse_bool(&v).ok_or_else(|| bad(format!("{v:?} is not a boolean")))?,
            ),
            EnumValue::Known(CellValues::Error) => CellValue::Error(v.into_owned()),
            EnumValue::Known(CellValues::String) => CellValue::String(escape::decode(&v).into_owned()),
            EnumValue::Known(CellValues::Date) => CellValue::Date(v.into_owned()),
            EnumValue::Known(CellValues::InlineString) => unreachable!(),
            EnumValue::Unknown(raw) => return Err(bad(format!("unknown cell type t={raw:?}"))),
        })
    }
}

/// Finds the row and cell through the sheet's [`SheetIndex`] (ADR-0002
/// §4.2's `MissingReference` rule, the order insertion relies on, and the
/// Phase0 review's `mc:AlternateContent` refusals for `worksheet`,
/// `sheetData` and the row, all checked once when the index was built), then
/// refuses an `mc:AlternateContent` inside the cell or its `is`, which could
/// hold the value (Phase0 review round 1 F6, round 2 N3).
pub(crate) fn locate(doc: &XmlDocument, index: &SheetIndex, part: &str, at: CellIndex) -> Result<Located> {
    let loc = index.locate(part, at)?;
    if let Some(c) = located_cell(doc, &loc) {
        refuse_alternate_content(c, sml::Cell::QNAME)?;
        for is in c.elements().filter(|n| n.is(sml::InlineString::QNAME)) {
            refuse_alternate_content(is, sml::InlineString::QNAME)?;
        }
    }
    Ok(loc)
}

/// `Err(UnresolvedSibling(parent))` if `n` has an `mc:AlternateContent`
/// child. Callers that rely on the physical child index cannot account for
/// items contributed through branches.
pub(crate) fn refuse_alternate_content(n: &RawNode, parent: QName) -> Result<()> {
    if n.elements().any(|c| c.is(MC_ALTERNATE_CONTENT)) {
        return Err(ooxml_core::Error::UnresolvedSibling(parent).into());
    }
    Ok(())
}

/// Greedy append preflight for a proposed list of children (ADR-0002 §4.1).
/// It checks schema order and slot limits and skips unlisted children.
/// Lists with AlternateContent are refused here because this planner relies
/// on the physical child list (ADR-0020 §8).
///
/// The new child's namespace prefix cannot be refused: it is always
/// SpreadsheetML main, the namespace of `E` itself, so the parent's own
/// prefix is bound to it.
pub(crate) fn check_insertable<'n, E: ElementSpec>(children: impl Iterator<Item = &'n RawNode>, q: QName) -> Result<()> {
    let Content::Slots(slots) = E::CONTENT else {
        return Err(ooxml_core::Error::UnsupportedContentModel(E::QNAME).into());
    };
    let named = |n: &RawNode| -> Vec<usize> {
        let v: Vec<usize> = slots.iter().enumerate().filter(|(_, s)| s.names.iter().any(|x| n.is(*x))).map(|(i, _)| i).collect();
        if !v.is_empty() {
            return v;
        }
        slots.iter().enumerate().filter(|(_, s)| s.wildcard.is_some_and(|w| w.matches(n.name().ns()))).map(|(i, _)| i).collect()
    };
    let Some(slot) = slots.iter().position(|s| s.names.contains(&q)) else {
        return Err(ooxml_core::Error::NotAllowed { parent: E::QNAME, child: q }.into());
    };
    let (mut cur, mut same_q, mut same_slot) = (0usize, 0u32, 0u32);
    for child in children {
        if child.is(MC_ALTERNATE_CONTENT) {
            return Err(ooxml_core::Error::UnresolvedSibling(E::QNAME).into());
        }
        let cands = named(child);
        if cands.is_empty() {
            continue;
        }
        let Some(&rank) = cands.iter().find(|&&r| r >= cur) else {
            return Err(ooxml_core::Error::UnresolvedSibling(E::QNAME).into());
        };
        cur = rank;
        if rank == slot {
            same_slot += 1;
            same_q += u32::from(child.is(q));
        }
    }
    let s = slots[slot];
    if s.max_each.is_some_and(|m| same_q >= m) || s.max_total.is_some_and(|m| same_slot >= m) {
        return Err(ooxml_core::Error::AlreadyPresent(q).into());
    }
    Ok(())
}

/// [`Worksheet::declared_bound`] for a `dimension/@ref` value.
fn dimension_bound(reference: &str) -> Option<CellRange> {
    let (first, last) = reference.split_once(':')?;
    let (first, last) = (CellIndex::parse(first).ok()?, CellIndex::parse(last).ok()?);
    let plain = reference.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b':');
    (plain && first.row <= last.row && first.col <= last.col).then_some(CellRange { first, last })
}

/// [`Worksheet::stored_extent`] from the sheet index.
fn stored_extent(index: &SheetIndex, part: &str) -> Result<Option<CellRange>> {
    let rows = index.target_rows(part)?;
    let mut columns: Option<(u32, u32)> = None;
    for row in rows {
        for &(column, _) in index.target_cells(row, part)? {
            columns = Some(columns.map_or((column, column), |(low, high)| (low.min(column), high.max(column))));
        }
    }
    // Rows are indexed in ascending order.
    let (Some((low, high)), Some(top), Some(bottom)) = (columns, rows.first(), rows.last()) else {
        return Ok(None);
    };
    Ok(Some(CellRange {
        first: CellIndex { row: top.row - 1, col: low },
        last: CellIndex { row: bottom.row - 1, col: high },
    }))
}

/// The target-cell half of an edit's plan (ADR-0003 §3.1; Phase0 review
/// round 2, N1: plan → validate → apply).
#[derive(Clone, Debug)]
pub(crate) struct TargetPlan {
    pub(crate) loc: Located,
    /// The new `dimension/@ref`, if `dimension` exists and must grow.
    pub(crate) dimension: Option<String>,
    /// The new `s` (Phase2): `None` keeps the cell's style.
    pub(crate) style: Option<u32>,
}

/// Decides the target write without changing anything: where the row and
/// cell go, that each insertion `write_cell` will make is possible (dry run
/// against the children it will actually have), the value's characters, the
/// `vm` guard and the new `dimension/@ref` (a `dimension` that cannot be
/// kept up to date is refused, round 1 F7). Once this returns `Ok`,
/// [`write_cell`] cannot fail.
///
/// A new row's position comes from the sheet index and is checked with
/// ooxml-core's non-mutating `check_insert_at` (O(1), no scan of the rows).
pub(crate) fn plan_target(doc: &mut XmlDocument, index: &SheetIndex, part: &str, at: CellIndex, op: &CellOp) -> Result<TargetPlan> {
    let text = match op {
        CellOp::Number(t) | CellOp::Date(t) | CellOp::Text(t) | CellOp::Formula(t, _) | CellOp::DynamicFormula(t, _) | CellOp::ArrayFormula(t, ..) | CellOp::SharedFormula(t, ..) => Some(t.as_str()),
        CellOp::RemoveFormula | CellOp::Clear | CellOp::ClearValue | CellOp::Shared(_) | CellOp::Bool(_) | CellOp::Error(_) | CellOp::StyleOnly | CellOp::InlineRich(_) => None,
    };
    if let Some(t) = text
        && let Some((offset, ch)) = ooxml_core::value::first_invalid_xml_char(t)
    {
        return Err(ooxml_core::Error::InvalidXmlChar { ch, offset }.into());
    }
    let loc = locate(doc, index, part, at)?;
    if index.sheet_data && loc.row.is_none() {
        let ws = doc.lens_mut::<sml::Worksheet>()?;
        let sd = ws.into_child_by::<sml::SheetData>(|_| true).expect("indexed sheetData");
        sd.check_insert_at::<sml::Row>(loc.row_insert_at)?;
    }
    if let (Some(ri), None) = (loc.row, loc.cell) {
        // a new cell at the indexed position (O(1), PR #29 review m3); a row
        // whose `extLst` precedes a cell is refused as `insert_by` would
        let rows = index.target_rows(part)?;
        let entry = rows.binary_search_by_key(&(at.row + 1), |e| e.row).ok().map(|k| &rows[k]);
        if entry.is_some_and(|e| e.misordered) {
            return Err(ooxml_core::Error::UnresolvedSibling(sml::Row::QNAME).into());
        }
        let ws = doc.lens_mut::<sml::Worksheet>()?;
        let sd = ws.into_child_by::<sml::SheetData>(|_| true).expect("indexed sheetData");
        let row = sd.into_child_at::<sml::Row>(ri).expect("located row");
        row.check_insert_at::<sml::Cell>(loc.cell_insert_at)?;
    }
    if !index.sheet_data {
        doc.lens_mut::<sml::Worksheet>()?.check_get_or_insert::<sml::SheetData>()?;
    }
    let ws = sml::WorksheetView(doc.lens::<sml::Worksheet>()?);
    let new_child = match op {
        CellOp::Number(_) | CellOp::Date(_) | CellOp::Shared(_) | CellOp::Bool(_) | CellOp::Error(_) => Some(sml::CellValue::QNAME),
        CellOp::Text(_) | CellOp::InlineRich(_) => Some(sml::InlineString::QNAME),
        CellOp::Formula(..) | CellOp::DynamicFormula(..) | CellOp::ArrayFormula(..) | CellOp::SharedFormula(..) => Some(sml::CellFormula::QNAME),
        CellOp::RemoveFormula | CellOp::Clear | CellOp::ClearValue | CellOp::StyleOnly => None,
    };
    match ws.sheet_data() {
        None => {},
        Some(sd) => match loc.row {
            None => {} // checked above, at the indexed position
            Some(ri) => {
                let row = sd.raw().children()[ri].as_element().expect("located row");
                match loc.cell {
                    None => {} // checked above, at the indexed position
                    Some(ci) => {
                        let c = row.children()[ci].as_element().expect("located cell");
                        if c.attr(sml::Cell::ATTR_VM).is_some() && !matches!(op, CellOp::StyleOnly) {
                            return Err(Error::RichValueCell { cell: at.to_string() });
                        }
                        // `write_cell` removes every `f`, `v` and `is`
                        // (duplicates included), then inserts the new one
                        let gone = [sml::CellFormula::QNAME, sml::CellValue::QNAME, sml::InlineString::QNAME];
                        if let Some(q) = new_child {
                            let kept = || c.elements().filter(|n| !gone.iter().any(|g| n.is(*g)));
                            check_insertable::<sml::Cell>(kept(), q)?;
                            if matches!(op, CellOp::Formula(_, Some(_)) | CellOp::ArrayFormula(_, _, _, Some(_)) | CellOp::SharedFormula(_, _, _, Some(_))) {
                                check_insertable::<sml::Cell>(kept(), sml::CellValue::QNAME)?;
                            }
                        }
                    }
                }
            }
        },
    }
    let dimension = match ws.sheet_dimension() {
        None => None,
        Some(dim) => {
            let r = dim.reference();
            let Some(Ok(range)) = r.map(CellRange::parse) else {
                return Err(Error::Malformed(format!(
                    "{part}: dimension/@ref {:?} is not a cell range; it could not be kept up to date",
                    r.unwrap_or_default()
                )));
            };
            let mut expanded = range.union(at);
            if let CellOp::ArrayFormula(_, array, ..) = op {
                expanded = expanded.union(array.last);
            }
            // A `ref` that is not a read bound (a single cell such as `A1`
            // from writers that do not track the extent) grows over the
            // stored data too: from the written cell alone it would become
            // a bound that hides the other cells from readers.
            if expanded != range
                && r.and_then(dimension_bound).is_none()
                && let Some(stored) = stored_extent(index, part)?
            {
                expanded = expanded.union(stored.first).union(stored.last);
            }
            (expanded != range).then(|| expanded.to_string())
        }
    };
    Ok(TargetPlan { loc, dimension, style: None })
}

/// The item index of the existing child that the new content can reuse in
/// place (R3-3): the first `v` for a number, the first `f` for a formula,
/// and the first `is` for a string when it holds exactly one `t` and no other
/// element (rich runs and phonetic data are replaced, not merged).
fn reusable(c: &RawNode, op: &CellOp) -> Option<usize> {
    let first = |q: QName| c.indexed_elements().find(|(_, n)| n.is(q)).map(|(i, _)| i);
    match op {
        CellOp::Number(_) | CellOp::Date(_) | CellOp::Shared(_) | CellOp::Bool(_) | CellOp::Error(_) => first(sml::CellValue::QNAME),
        CellOp::Formula(..) | CellOp::DynamicFormula(..) | CellOp::ArrayFormula(..) | CellOp::SharedFormula(..) => first(sml::CellFormula::QNAME),
        CellOp::Text(_) => first(sml::InlineString::QNAME).filter(|&i| {
            let is = c.children()[i].as_element().expect("element");
            let mut kids = is.elements();
            matches!((kids.next(), kids.next()), (Some(t), None) if t.is(sml::Text::QNAME))
        }),
        CellOp::RemoveFormula | CellOp::Clear | CellOp::ClearValue | CellOp::StyleOnly | CellOp::InlineRich(_) => None,
    }
}

/// Writes the target cell (after the plan validated everything): creates the
/// row and cell if needed, applies `op`, then grows `dimension` if it exists
/// (never inserts one).
///
/// None of the `?` below can fire after [`plan_target`]: each insertion was
/// dry-run against the same children, the text was checked for XML
/// characters, and the attribute writes are on `Cell`/`Row`/`SheetDimension`,
/// whose attribute lists are complete.
pub(crate) fn write_cell(doc: &mut XmlDocument, plan: &TargetPlan, at: CellIndex, op: &CellOp) -> Result<WriteEffect> {
    let loc = plan.loc;
    let row_no = at.row + 1;
    let mut effect = WriteEffect::default();
    {
        effect.created_sheet_data = sml::WorksheetView(doc.lens::<sml::Worksheet>()?).sheet_data().is_none();
        let mut ws = sml::WorksheetViewMut(doc.lens_mut::<sml::Worksheet>()?);
        let mut sd = ws.get_or_insert_sheet_data()?;
        let mut row = match loc.row {
            Some(i) => sml::RowViewMut(sd.0.into_child_at::<sml::Row>(i).expect("located row")),
            None => {
                let at_item = if effect.created_sheet_data { 0 } else { loc.row_insert_at };
                let mut r = sml::RowViewMut(sd.0.insert_at::<sml::Row>(at_item)?);
                r.set_row_index(row_no)?;
                effect.new_row = Some(at_item);
                r
            }
        };
        let mut c = match loc.cell {
            Some(i) => sml::CellViewMut(row.0.into_child_at::<sml::Cell>(i).expect("located cell")),
            None => {
                let item = if loc.row.is_some() { loc.cell_insert_at } else { 0 };
                let mut c = sml::CellViewMut(row.0.insert_at::<sml::Cell>(item)?);
                c.set_cell_reference(&at.to_string())?;
                effect.new_cell = Some(item);
                c
            }
        };
        // Every `f`, `v` and `is`, not just the first (a schema-invalid
        // duplicate would otherwise survive and be read by others), except
        // one element of the kind the new content needs: that one is reused
        // in place, so comments, PIs and unknown children keep their
        // position relative to the value (Phase0 review round 3, R3-3).
        if let Some(s) = plan.style {
            c.set_style_index(s)?;
        }
        let reuse = reusable(c.0.node(), op);
        let doomed: Vec<usize> = c
            .0
            .node()
            .indexed_elements()
            .filter(|(i, n)| {
                Some(*i) != reuse
                    && !matches!(op, CellOp::StyleOnly)
                    && (n.is(sml::CellFormula::QNAME) || n.is(sml::CellValue::QNAME) || n.is(sml::InlineString::QNAME))
            })
            .map(|(i, _)| i)
            .collect();
        for i in doomed.into_iter().rev() {
            c.0.remove_at(i);
        }
        let reused = reuse.is_some();
        match op {
            CellOp::Number(text) | CellOp::Date(text) => {
                if matches!(op, CellOp::Date(_)) {
                    c.set_data_type(CellValues::Date)?;
                } else {
                    c.remove_data_type()?;
                }
                match c.cell_value_mut() {
                    Some(mut v) if reused => v.set_text(text)?,
                    _ => c.get_or_insert_cell_value()?.set_text(text)?,
                }
            }
            CellOp::Text(text) => {
                c.set_data_type(CellValues::InlineString)?;
                c.get_or_insert_inline_string()?.get_or_insert_text()?.set_text(text)?;
            }
            CellOp::InlineRich(runs) => {
                // a new `is` (every old one was removed above): `is/r`, valid per CT_Rst
                c.set_data_type(CellValues::InlineString)?;
                let mut is = c.get_or_insert_inline_string()?;
                for r in runs {
                    crate::registry::strings::build_run(&mut is.0, r)?;
                }
            }
            CellOp::Formula(text, _) | CellOp::DynamicFormula(text, _) | CellOp::ArrayFormula(text, ..) | CellOp::SharedFormula(text, ..) => {
                if let CellOp::ArrayFormula(_, _, Some(cm), _) | CellOp::DynamicFormula(_, cm) = op {
                    c.set_cell_meta_index(*cm)?;
                }
                c.remove_data_type()?;
                let mut f = c.get_or_insert_cell_formula()?;
                if reused {
                    // a plain formula: drop shared/array/data-table attributes
                    f.remove_formula_type()?;
                    f.remove_reference()?;
                    f.remove_shared_index()?;
                    f.remove_always_calculate_array()?;
                    f.remove_data_table2_d()?;
                    f.remove_data_table_row()?;
                    f.remove_input1_deleted()?;
                    f.remove_input2_deleted()?;
                    f.remove_r1()?;
                    f.remove_r2()?;
                    f.remove_calculate_cell()?;
                    f.remove_bx()?;
                }
                if let CellOp::ArrayFormula(_, range, ..) = op {
                    f.set_formula_type(sml::CellFormulaValues::Array)?;
                    f.set_reference(&range.to_string())?;
                }
                if matches!(op, CellOp::DynamicFormula(..)) {
                    f.set_formula_type(sml::CellFormulaValues::Array)?;
                    f.set_reference(&at.to_string())?;
                }
                if let CellOp::SharedFormula(_, si, range, _) = op {
                    f.set_formula_type(sml::CellFormulaValues::Shared)?;
                    f.set_shared_index(*si)?;
                    if let Some(range) = range {
                        f.set_reference(&range.to_string())?;
                    }
                }
                f.set_text(text)?;
                if let CellOp::Formula(_, Some((kind, value))) | CellOp::ArrayFormula(_, _, _, Some((kind, value))) | CellOp::SharedFormula(_, _, _, Some((kind, value))) = op {
                    c.set_data_type(*kind)?;
                    c.get_or_insert_cell_value()?.set_text(value)?;
                }
            }
            CellOp::RemoveFormula | CellOp::Clear | CellOp::ClearValue => {
                c.remove_data_type()?;
            }
            CellOp::Shared(index) => {
                c.set_data_type(CellValues::SharedString)?;
                c.get_or_insert_cell_value()?.set_text(&index.to_string())?;
            }
            CellOp::Bool(b) => {
                c.set_data_type(CellValues::Boolean)?;
                c.get_or_insert_cell_value()?.set_text(if *b { "1" } else { "0" })?;
            }
            CellOp::Error(e) => {
                c.set_data_type(CellValues::Error)?;
                c.get_or_insert_cell_value()?.set_text(e)?;
            }
            CellOp::StyleOnly => {}
        }
    }
    if let Some(r) = &plan.dimension {
        let mut ws = sml::WorksheetViewMut(doc.lens_mut::<sml::Worksheet>()?);
        ws.sheet_dimension_mut().expect("planned with a dimension").set_reference(r)?;
    }
    Ok(effect)
}

/// What [`write_cell`] inserted, for the sheet index.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct WriteEffect {
    pub(crate) created_sheet_data: bool,
    /// Item index of a new `row` in `sheetData`.
    pub(crate) new_row: Option<usize>,
    /// Item index of a new `c` in its row.
    pub(crate) new_cell: Option<usize>,
}

impl WriteEffect {
    /// Keeps the sheet index valid after the write.
    pub(crate) fn update(&self, index: &mut SheetIndex, at: CellIndex) {
        if self.created_sheet_data {
            index.sheet_data_created();
        }
        if let Some(item) = self.new_row {
            index.row_inserted(at.row + 1, item);
        }
        if let Some(item) = self.new_cell {
            index.cell_inserted(at.row + 1, at.col, item);
        }
    }
}

impl<'wb> Worksheet<'wb> {
    pub(crate) fn new(wb: &'wb mut Workbook, sheet: usize) -> Self {
        Self { wb, sheet, workbook_view_id: 0 }
    }

    /// Removes physically empty, unstyled cell elements at the given coordinates.
    ///
    /// Cells with children or additional attributes are retained.
    pub fn prune_empty_cells(&mut self, positions: &[CellIndex]) -> Result<()> {
        if positions.is_empty() {
            return Ok(());
        }
        self.wb.check_editable()?;
        let sheet = self.sheet;
        let part = self.part_name().to_owned();
        self.wb.ensure_index(sheet)?;
        let doc = self.wb.pkg.xml_part(&part)?;
        let index = self.wb.indexes.get(&sheet).expect("built");
        let mut remove = Vec::new();
        for &at in positions {
            let loc = locate(doc, index, &part, at)?;
            let Some(cell) = located_cell(doc, &loc) else { continue };
            if cell.children().is_empty() && cell.attrs().iter().all(|attr| {
                let name = attr.name();
                name.ns().is_empty() && (name.local() == "r" ||
                    (name.local() == "s" && ooxml_core::value::parse_u32(attr.value()) == Some(0)))
            }) {
                remove.push((loc.row.expect("located cell row"), loc.cell.expect("located cell"), at.row + 1, at.col));
            }
        }
        if remove.is_empty() {
            return Ok(());
        }
        remove.sort_unstable();
        remove.dedup();
        self.wb.poisoning(|workbook| {
            let doc = workbook.pkg.xml_part_mut(&part)?;
            let mut worksheet = doc.lens_mut::<sml::Worksheet>()?;
            let index = workbook.indexes.get_mut(&sheet).expect("built");
            for (row, cell, row_no, col) in remove.into_iter().rev() {
                let mut row = worksheet.child_mut::<sml::SheetData>().expect("located")
                    .into_child_at::<sml::Row>(row).expect("located");
                row.remove_at(cell);
                index.cell_removed(row_no, col, cell);
            }
            Ok(())
        })
    }

    /// The sheet's name.
    pub fn name(&self) -> &str {
        &self.wb.sheets[self.sheet].name
    }

    /// The worksheet part's name.
    pub fn part_name(&self) -> &str {
        self.wb.sheets[self.sheet].part.as_deref().expect("worksheet")
    }

    fn doc(&self) -> &XmlDocument {
        self.wb.pkg.parsed_part(self.part_name()).expect("parsed when the worksheet was opened")
    }

    fn index(&self) -> &SheetIndex {
        self.wb.indexes.get(&self.sheet).expect("indexed when the worksheet was opened")
    }

    /// A read view of a `c` node (its lens is rooted at the cell: reads
    /// never look at ancestors).
    fn cell_ref<'s>(&'s self, c: &'s RawNode, at: CellIndex) -> Result<CellRef<'s>> {
        Ok(CellRef {
            at,
            view: sml::CellView(ooxml_core::Lens::root(c)?),
            sst: self.wb.shared_strings.as_deref(),
            workbook: self.wb,
        })
    }

    /// The declared worksheet autofilter range, if present.
    pub fn autofilter_range(&self) -> Option<&str> {
        self.doc().root().elements()
            .find(|node| node.is(sml::AutoFilter::QNAME))
            .and_then(|node| node.attr(QName::new("", "ref")))
    }

    /// Returns the height in points, hidden flag, and outline level of a zero-based row.
    pub fn row_layout(&self, row: u32) -> Result<(Option<f64>, bool, u32)> {
        let Some(node) = self.row_node(row)? else { return Ok((None, false, 0)) };
        let height = node.attr(ooxml_core::QName::new("", "ht"))
            .and_then(ooxml_core::value::parse_f64);
        let hidden = node.attr(ooxml_core::QName::new("", "hidden"))
            .and_then(ooxml_core::value::parse_bool).unwrap_or(false);
        let outline = node.attr(QName::new("", "outlineLevel"))
            .and_then(ooxml_core::value::parse_u32).unwrap_or(0);
        Ok((height, hidden, outline))
    }

    fn row_node(&self, row: u32) -> Result<Option<&RawNode>> {
        CellIndex::new(row, 0)?;
        let rows = self.index().target_rows(self.part_name())?;
        let Ok(entry) = rows.binary_search_by_key(&(row + 1), |entry| entry.row) else {
            return Ok(None);
        };
        let node = sheet_data(self.doc())
            .and_then(|data| data.children()[rows[entry].item].as_element())
            .expect("indexed row");
        Ok(Some(node))
    }

    /// Reads the explicit default format of a row.
    pub fn row_style_index(&self, row: u32) -> Result<Option<u32>> {
        Ok(self.row_node(row)?.and_then(|node| {
            let custom = node.attr(QName::new("", "customFormat")).and_then(ooxml_core::value::parse_bool);
            (custom == Some(true)).then(|| node.attr(QName::new("", "s")).and_then(ooxml_core::value::parse_u32).unwrap_or(0))
        }))
    }

    /// The one-based row numbers stored in the worksheet, in document order.
    pub fn row_numbers(&self) -> Result<impl Iterator<Item = u32> + '_> {
        Ok(self.index().target_rows(self.part_name())?.iter().map(|row| row.row))
    }

    /// Every existing cell, in document order (row by row, left to right):
    /// a bulk read in one pass over the index, instead of one lookup per
    /// cell. A row whose cells cannot be located yields its error and the
    /// iteration continues; a sheet-level problem (a `row` without `r`, rows
    /// out of order, `mc:AlternateContent` around rows) yields one error.
    pub fn cells(&self) -> impl Iterator<Item = Result<CellRef<'_>>> + '_ {
        self.cells_before_row(u32::MAX)
    }

    /// Reads stored cells with zero-based row indices below `end`.
    /// Sheet-level errors still apply. Individual cells in later rows are not opened.
    pub fn cells_before_row(&self, end: u32) -> impl Iterator<Item = Result<CellRef<'_>>> + '_ {
        let part = self.part_name();
        let index = self.index();
        let sd = sheet_data(self.doc());
        let rows: Vec<Result<&crate::index::RowEntry>> = match index.target_rows(part) {
            Ok(rows) => rows[..rows.partition_point(|row| row.row <= end)].iter().map(Ok).collect(),
            Err(e) => vec![Err(e)],
        };
        rows.into_iter().flat_map(move |r| -> Box<dyn Iterator<Item = Result<CellRef<'_>>> + '_> {
            let e = match r {
                Ok(e) => e,
                Err(err) => return Box::new(std::iter::once(Err(err))),
            };
            let cells = match index.target_cells(e, part) {
                Ok(c) => c,
                Err(err) => return Box::new(std::iter::once(Err(err))),
            };
            let row = sd.and_then(|sd| sd.children()[e.item].as_element()).expect("indexed row");
            Box::new(cells.iter().map(move |&(col, ci)| {
                let c = row.children()[ci].as_element().expect("indexed cell");
                refuse_alternate_content(c, sml::Cell::QNAME)?;
                for is in c.elements().filter(|n| n.is(sml::InlineString::QNAME)) {
                    refuse_alternate_content(is, sml::InlineString::QNAME)?;
                }
                self.cell_ref(c, CellIndex { row: e.row - 1, col })
            }))
        })
    }

    /// Reads a cell. `Ok(None)` if it does not exist. Never changes the tree.
    ///
    /// Errors: [`Error::MissingReference`] if the sheet has a `row` without
    /// `r`, or the row has a `c` without `r` (ADR-0002 §4.2: a silent `None`
    /// could hide a value); [`Error::OutOfRange`].
    pub fn cell(&self, row: u32, col: u32) -> Result<Option<CellRef<'_>>> {
        let at = CellIndex::new(row, col)?;
        let doc = self.doc();
        let loc = locate(doc, self.index(), self.part_name(), at)?;
        let Some(c) = located_cell(doc, &loc) else { return Ok(None) };
        Ok(Some(self.cell_ref(c, at)?))
    }

    /// Reads a cell by A1 reference.
    pub fn cell_a1(&self, reference: &str) -> Result<Option<CellRef<'_>>> {
        let at = CellIndex::parse(reference)?;
        self.cell(at.row, at.col)
    }

    /// A write handle. Nothing is created or marked dirty until a setter is
    /// called on it (ADR-0001 R2-m1).
    pub fn cell_mut(&mut self, row: u32, col: u32) -> Result<CellMut<'_, 'wb>> {
        let at = CellIndex::new(row, col)?;
        Ok(CellMut { ws: self, at })
    }

    /// `cell_mut(row, col)?.set_string(value)` (the from-scratch API name).
    pub fn write_string(&mut self, row: u32, col: u32, value: &str) -> Result<()> {
        self.cell_mut(row, col)?.set_string(value)
    }

    /// `cell_mut(row, col)?.set_string_inline(value)`.
    pub fn write_string_inline(&mut self, row: u32, col: u32, value: &str) -> Result<()> {
        self.cell_mut(row, col)?.set_string_inline(value)
    }

    /// `cell_mut(row, col)?.set_number(value)` (the from-scratch API name).
    pub fn write_number(&mut self, row: u32, col: u32, value: f64) -> Result<()> {
        self.cell_mut(row, col)?.set_number(value)
    }

    /// `cell_mut(row, col)?.set_formula(text)`.
    pub fn write_formula(&mut self, row: u32, col: u32, text: &str) -> Result<()> {
        self.cell_mut(row, col)?.set_formula(text)
    }
}

/// A write handle for one cell (ADR-0001: like `HashMap::entry`, nothing
/// happens until a value is set). See [`Worksheet`] for the wrapped-grid
/// refusal shared by cell edits.
#[derive(Debug)]
pub struct CellMut<'s, 'wb> {
    pub(crate) ws: &'s mut Worksheet<'wb>,
    pub(crate) at: CellIndex,
}

/// Excel's cell text limit, in UTF-16 code units.
pub(crate) const MAX_STRING_LEN: usize = 32_767;

/// Plans an inline-string write: the length check and `_xHHHH_` escaping.
/// Shared by `set_string_inline` and the Journal's plan input.
pub(crate) fn plan_inline_write(value: &str) -> Result<CellOp> {
    let units = value.encode_utf16().count();
    if units > MAX_STRING_LEN {
        return Err(Error::StringTooLong(units));
    }
    Ok(CellOp::Text(escape::encode(value).into_owned()))
}

/// Formats a finite number as `xsd:double`: plain decimal in the usual
/// range, `1E300`-style outside it; `-0` is written as `0`.
pub(crate) fn format_number(v: f64) -> String {
    let a = v.abs();
    if v == 0.0 {
        "0".into()
    } else if (1e-5..1e16).contains(&a) {
        format!("{v}")
    } else {
        format!("{v:E}")
    }
}

/// One cell of a [`Worksheet::write_cells`] batch.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CellWrite<'a> {
    /// As [`CellMut::set_number`].
    Number(f64),
    /// As [`CellMut::set_string`].
    String(&'a str),
    /// As [`CellMut::set_iso_date`].
    IsoDate(&'a str),
    /// As [`CellMut::set_error`].
    Error(&'a str),
    /// As [`CellMut::set_bool`].
    Bool(bool),
    /// Clears the value and formula while keeping the cell style.
    /// An absent target becomes a blank cell.
    Clear,
    /// As [`CellMut::set_formula`] (file form; one leading `=` is dropped).
    Formula(&'a str),
    /// As [`CellMut::set_formula_with_result`].
    FormulaWithResult(&'a str, &'a CellValue),
    /// As [`CellMut::remove_formula`].
    RemoveFormula,
    /// As [`CellMut::clear_value`].
    ClearValue,
}

impl<'wb> Worksheet<'wb> {
    /// Reads the original XML fragment inside a rich-text cell or shared-string item.
    pub fn rich_text_xml(&self, row: u32, column: u32) -> Result<Option<String>> {
        let Some(cell) = self.cell(row, column)? else { return Ok(None) };
        let source = match cell.data_type() {
            EnumValue::Known(CellValues::InlineString) => cell.raw().elements()
                .find(|node| node.is(sml::InlineString::QNAME)).map(|node| (self.doc(), node)),
            EnumValue::Known(CellValues::SharedString) => cell.shared_string_source()?,
            _ => None,
        };
        let Some((document, node)) = source else { return Ok(None) };
        if document.is_dirty() || !node.elements().any(|child| child.is(QName::new(crate::SML_NS, "r"))) {
            return Ok(None);
        }
        let Some(span) = node.span() else { return Ok(None) };
        Ok(span.close.and_then(|(end, _)| document.source().get(span.open_end..end)).map(str::to_owned))
    }

    /// Writes several cells as **one** edit (ADR-0003 addendum P5-pre): every
    /// guard is checked for every cell before anything changes, so either
    /// all cells are written or none (`Err`, nothing changed). Dependents of
    /// all the cells lose their cached values once, `fullCalcOnLoad` is set
    /// once, and `calcChain.xml` is removed when the set of formula cells
    /// changes.
    ///
    /// Errors: those of the single-cell setters for any cell, plus
    /// [`Error::DuplicateCellWrite`] for a cell named twice and
    /// [`Error::SharedFormulaMaster`] when two cells of one shared-formula
    /// group are written together (write them in separate calls).
    pub fn write_cells(&mut self, writes: &[(CellIndex, CellWrite<'_>)]) -> Result<()> {
        let was_active = self.wb.registries.strings.is_some();
        let r = self.write_cells_inner(writes);
        self.wb.forget_activation_on_error(was_active, &r);
        r
    }

    fn write_cells_inner(&mut self, writes: &[(CellIndex, CellWrite<'_>)]) -> Result<()> {
        self.wb.check_editable()?;
        let ops = self.prepare_cell_writes(writes)?;
        self.wb.write_with_headers(ops)
    }

    /// Writes cell contents together with optional explicit style indexes.
    pub fn write_cells_with_styles(
        &mut self, writes: &[(CellIndex, CellWrite<'_>)], styles: &[Option<u32>],
    ) -> Result<()> {
        if writes.len() != styles.len() {
            return Err(Error::invalid_argument(None, "expected one style slot per cell write".into()));
        }
        self.wb.check_editable()?;
        let mut ops = self.prepare_cell_writes(writes)?;
        for ((_, _, _, extra), style) in ops.iter_mut().zip(styles) {
            extra.style = style.map(crate::registry::styles::StyleRequest::Index);
        }
        self.wb.write_with_headers(ops)
    }

    pub(crate) fn prepare_cell_writes(&mut self, writes: &[(CellIndex, CellWrite<'_>)]) -> Result<Vec<crate::tables::Write>> {
        let mut ops = Vec::with_capacity(writes.len());
        for &(at, w) in writes {
            let (op, extra) = self.wb.plan_write_value(at, w)?;
            ops.push((self.sheet, at, op, extra));
        }
        Ok(ops)
    }
}

impl Workbook {
    /// Plans one `write_cells` item: validates the coordinate and the value
    /// and reserves its string, without applying. Shared by `write_cells`
    /// and the Journal's plan input.
    pub(crate) fn plan_write_value(
        &mut self,
        at: CellIndex,
        w: CellWrite<'_>,
    ) -> Result<(CellOp, Extra)> {
        CellIndex::new(at.row, at.col)?;
        Ok(match w {
            CellWrite::Number(v) => {
                if !v.is_finite() {
                    return Err(Error::InvalidNumber(v));
                }
                (CellOp::Number(format_number(v)), Default::default())
            }
            CellWrite::String(s) => self.plan_string_write(s)?,
            CellWrite::Formula(t) => (
                CellOp::Formula(t.strip_prefix('=').unwrap_or(t).to_owned(), None),
                Default::default(),
            ),
            CellWrite::FormulaWithResult(text, result) => (
                CellOp::Formula(
                    text.strip_prefix('=').unwrap_or(text).to_owned(),
                    crate::dynamic::formula_cache(at, result.clone())?,
                ),
                Default::default(),
            ),
            CellWrite::IsoDate(value) => (crate::setters::iso_date_op(at, value)?, Default::default()),
            CellWrite::Error(value) => (CellOp::Error(crate::setters::error_value(at, value)?), Default::default()),
            CellWrite::Bool(value) => (CellOp::Bool(value), Default::default()),
            CellWrite::Clear => (CellOp::Clear, Default::default()),
            CellWrite::RemoveFormula => (CellOp::RemoveFormula, Default::default()),
            CellWrite::ClearValue => (CellOp::ClearValue, Default::default()),
        })
    }
}

impl CellMut<'_, '_> {
    /// The cell's position.
    pub fn index(&self) -> CellIndex {
        self.at
    }

    /// Writes a number: `<v>` holds it, a stale `t` (`b`, `s`,
    /// `inlineStr`, …) and any `<is>` are removed, `s` is kept.
    ///
    /// Errors (nothing is changed): [`Error::InvalidNumber`] for NaN or
    /// infinities, and the refusals listed on [`Self::set_string`].
    pub fn set_number(self, value: f64) -> Result<()> {
        if !value.is_finite() {
            return Err(Error::InvalidNumber(value));
        }
        let ws = &mut *self.ws;
        ws.wb.edit_cell(ws.sheet, self.at, CellOp::Number(format_number(value)))
    }

    /// Writes a string. When the workbook has a shared-string table it is
    /// written as a shared string (`t="s"`, ADR-0006 §4): an existing plain
    /// item with exactly these bytes is reused, otherwise one `si` is
    /// appended, and `count`/`uniqueCount` are kept current. Without a
    /// shared-string table it is an inline string, as
    /// [`Self::set_string_inline`] writes. `xml:space="preserve"` is set when
    /// the value has leading or trailing whitespace; characters XML cannot
    /// carry are written as `_xHHHH_`. `s` is kept. At most 32,767 UTF-16
    /// code units ([`Error::StringTooLong`]).
    ///
    /// In a workbook with formulas, the cached values of every cell that may
    /// depend on this one are removed and `fullCalcOnLoad` is set (ADR-0003
    /// §3); a formula in the cell itself is replaced by the value.
    ///
    /// Errors (nothing is changed): workbook-wide refusals
    /// ([`Workbook::edit_refusal`]), [`Error::ArrayResultCell`],
    /// [`Error::ArrayAnchor`], [`Error::MacroSheetDependent`],
    /// [`Error::IterativeCalculation`], [`Error::SharedFormulaMaster`],
    /// [`Error::MergedCell`],
    /// [`Error::TableHeaderCell`], [`Error::RichValueCell`],
    /// [`Error::MissingReference`], [`Error::OutOfOrder`]; core refusals
    /// such as `UnresolvedSibling` if `sheetData` itself must be inserted
    /// next to an `mc:AlternateContent`.
    pub fn set_string(self, value: &str) -> Result<()> {
        let ws = &mut *self.ws;
        ws.wb.write_string(ws.sheet, self.at, value, None)
    }

    /// Writes a string as an inline string (`t="inlineStr"`,
    /// `<is><t>…</t></is>`), never touching the shared-string table (the
    /// Phase0 behaviour of [`Self::set_string`]). Same rules and refusals.
    pub fn set_string_inline(self, value: &str) -> Result<()> {
        let ws = &mut *self.ws;
        ws.wb
            .edit_cell(ws.sheet, self.at, plan_inline_write(value)?)
    }

    /// Writes a formula (ADR-0003 §4.1): `<f>text</f>` with no cached value
    /// and no `t`; one leading `=` is dropped (so `"==A1"` is refused). Dependents lose their cached
    /// values, `fullCalcOnLoad` is set (except in manual calculation mode),
    /// and `calcChain.xml` is removed when the cell was not a formula cell.
    ///
    /// The text is the **file form** (ADR-0003 addendum P5-pre): a known
    /// future function needs its prefix (`_xlfn.XLOOKUP`,
    /// `_xlfn._xlws.FILTER`), else Excel would show `#NAME?`;
    /// [`crate::add_future_prefixes`] converts a formula as typed. The spill
    /// (`A1#`) and implicit-intersection (`@`) operators are written as
    /// `_xlfn.ANCHORARRAY(…)` and `_xlfn.SINGLE(…)`.
    ///
    /// Errors (nothing is changed): [`Error::InvalidFormula`] if the text
    /// does not lex or holds characters XML cannot carry;
    /// [`Error::MissingFunctionPrefix`], [`Error::DynamicArrayOperator`];
    /// the refusals of [`Self::set_number`].
    pub fn set_formula(self, text: &str) -> Result<()> {
        self.set_formula_with_result(text, CellValue::Empty)
    }

    /// Writes a formula with a caller-supplied cached result.
    pub fn set_formula_with_result(self, text: &str, result: CellValue) -> Result<()> {
        let cache = crate::dynamic::formula_cache(self.at, result)?;
        let text = text.strip_prefix('=').unwrap_or(text);
        let ws = &mut *self.ws;
        ws.wb.edit_cell(ws.sheet, self.at, CellOp::Formula(text.to_owned(), cache))
    }

    /// Removes the cell's formula and its cached value (`f`, `v`, `t`, `is`);
    /// the cell keeps its style. Dependents are invalidated and
    /// `calcChain.xml` is removed. A cell without a formula is left alone.
    pub fn remove_formula(self) -> Result<()> {
        let ws = &mut *self.ws;
        ws.wb.edit_cell(ws.sheet, self.at, CellOp::RemoveFormula)
    }

    /// Clears the cell's value and formula (`v`, `is`, `f`, `t`) and keeps its
    /// style (`s`): the equivalent of ExcelJS 4.4.0 `cell.value = null`.
    /// A cell that does not exist is left alone: no row or `c` is created and
    /// the part is byte-identical, so the row or column default style stays
    /// (ExcelJS keeps it too). Dependents are invalidated and `calcChain.xml`
    /// is removed when a formula cell is cleared.
    ///
    /// Refusals are those of the other writes, nothing is changed: a merged
    /// non-anchor cell, a table header cell, an array or data-table anchor or
    /// result cell, a rich-value cell (`vm`), and a shared-formula master
    /// whose group is also written in the same batch.
    pub fn clear_value(self) -> Result<()> {
        let ws = &mut *self.ws;
        ws.wb.edit_cell(ws.sheet, self.at, CellOp::ClearValue)
    }
}

impl Worksheet<'_> {
    /// Clears an exact one-row static array formula and its stored results.
    /// Styles remain, and normal dependent-cache invalidation applies.
    pub fn clear_static_array_row(&mut self, range: CellRange) -> Result<()> {
        self.wb.clear_static_array_row(self.sheet, range)
    }

    /// Clears the exact master and member columns of a one-row shared formula.
    pub fn clear_shared_formula_row(&mut self, master: CellIndex, columns: &[u32]) -> Result<()> {
        self.wb.clear_shared_formula_row(self.sheet, master, columns)
    }

    /// The declared worksheet extent, if present and valid.
    pub fn declared_range(&self) -> Option<CellRange> {
        self.doc()
            .root()
            .elements()
            .find(|node| node.name().ns() == crate::SML_NS && node.name().local() == "dimension")
            .and_then(|node| node.attr(QName::new("", "ref")))
            .and_then(|reference| CellRange::parse(reference).ok())
    }

    /// The declared extent when it can bound reads: `dimension/@ref` written
    /// as `first:last` in upper case without `$`, with `first` above and left of `last`.
    /// A single-cell `ref` (`A1`, which writers that do not track the extent
    /// emit whatever the data), a reversed, absolute or invalid one, or no
    /// `dimension` at all is not a bound; readers then derive the extent from
    /// the stored rows and cells ([`Worksheet::stored_extent`]).
    pub fn declared_bound(&self) -> Option<CellRange> {
        self.doc()
            .root()
            .elements()
            .find(|node| node.name().ns() == crate::SML_NS && node.name().local() == "dimension")
            .and_then(|node| node.attr(QName::new("", "ref")))
            .and_then(dimension_bound)
    }

    /// The extent of the stored worksheet data: every `row` element (with or
    /// without cells) and every stored cell, empty ones included. `None` when
    /// the sheet stores no cell. One pass over the row and cell index.
    pub fn stored_extent(&self) -> Result<Option<CellRange>> {
        stored_extent(self.index(), self.part_name())
    }

    /// Lists array formula anchors with their result ranges, text and spill metadata.
    pub fn array_formulas(&self) -> Vec<(CellIndex, CellRange, String, bool)> {
        self.wb.census.alive()
            .filter(|(_, formula)| formula.sheet == self.sheet && formula.kind == crate::census::FKind::Array)
            .filter_map(|(_, formula)| Some((formula.at, formula.range?, formula.text.clone(), formula.dynamic)))
            .collect()
    }

    /// Returns the formula text and result range for an array anchor.
    pub fn array_formula_at(&self, at: CellIndex) -> Option<(&str, CellRange)> {
        let census = &self.wb.census;
        let formula = census.get(census.at(self.sheet, at)?)?;
        if formula.kind != crate::census::FKind::Array {
            return None;
        }
        Some((&formula.text, formula.range?))
    }

    /// Writes an entire shared formula group in row-major order, with its optional cached results.
    pub fn set_shared_formula(&mut self, range: CellRange, text: &str, results: Vec<CellValue>) -> Result<()> {
        self.wb.check_editable()?;
        let first = range.first;
        let last = range.last;
        CellIndex::new(last.row, last.col)?;
        if !range.contains(first) {
            return Err(Error::invalid_argument(None, "invalid shared formula range".into()));
        }
        let rows = (last.row - first.row + 1) as usize;
        let cols = (last.col - first.col + 1) as usize;
        if rows.checked_mul(cols) != Some(results.len()) {
            return Err(Error::invalid_argument(None, "shared formula results must cover the range".into()));
        }
        let cells = results.into_iter().enumerate().map(|(index, value)| {
            (CellIndex { row: first.row + (index / cols) as u32, col: first.col + (index % cols) as u32 }, value)
        });
        self.write_shared_formula_cells(range, text, cells)
    }

    /// Writes a one-row shared group, keeping columns without members sparse.
    pub fn set_sparse_shared_formula_row(
        &mut self, range: CellRange, text: &str, master: CellValue, members: Vec<(u32, CellValue)>,
    ) -> Result<()> {
        self.wb.check_editable()?;
        CellIndex::new(range.last.row, range.last.col)?;
        if range.first.row != range.last.row || !range.contains(range.first) ||
            members.iter().any(|(col, _)| *col <= range.first.col || *col > range.last.col)
        {
            return Err(Error::invalid_argument(None, "invalid sparse shared formula range".into()));
        }
        let cells = std::iter::once((range.first, master)).chain(members.into_iter().map(|(col, value)| {
            (CellIndex { row: range.first.row, col }, value)
        }));
        self.write_shared_formula_cells(range, text, cells)
    }

    fn write_shared_formula_cells(
        &mut self, range: CellRange, text: &str, cells: impl Iterator<Item = (CellIndex, CellValue)>,
    ) -> Result<()> {
        let si = self.next_shared_formula_index()?;
        let text = text.strip_prefix('=').unwrap_or(text);
        let mut writes = Vec::with_capacity(cells.size_hint().0);
        for (at, value) in cells {
            let master = at == range.first;
            if !master && self.translate_shared_formula(text, range.first, at).is_none() {
                return Err(Error::SharedFormulaMaster { cell: at.to_string(), reason: "formula cannot be translated".into() });
            }
            let (expression, reference) = if master { (text.to_owned(), Some(range)) } else { (String::new(), None) };
            let cache = crate::dynamic::formula_cache(at, value)?;
            writes.push((self.sheet, at, CellOp::SharedFormula(expression, si, reference, cache), crate::edit::Extra::default()));
        }
        let plan = self.wb.plan_writes(writes)?;
        self.wb.apply_writes(plan)
    }

    fn next_shared_formula_index(&self) -> Result<u32> {
        use crate::census::FKind;
        self.wb.census.alive()
            .filter(|(_, formula)| formula.sheet == self.sheet)
            .filter_map(|(_, formula)| match formula.kind { FKind::Shared { si, .. } => Some(si), _ => None })
            .max()
            .map_or(Some(0), |si| si.checked_add(1))
            .ok_or_else(|| Error::Malformed("shared formula indexes exhausted".into()))
    }

    /// Creates a shared formula master without changing other cells in its range.
    pub fn set_shared_formula_master(&mut self, range: CellRange, text: &str, result: CellValue) -> Result<()> {
        use crate::census::FKind;
        let at = range.first;
        CellIndex::new(range.last.row, range.last.col)?;
        if !range.contains(at) {
            return Err(Error::invalid_argument(None, "invalid shared formula range".into()));
        }
        let census = &self.wb.census;
        if census.at(self.sheet, at).and_then(|id| census.get(id))
            .is_some_and(|formula| matches!(formula.kind, FKind::Shared { master: true, .. }))
        {
            return Err(Error::SharedFormulaMaster {
                cell: at.to_string(),
                reason: "replace the shared group together with its members".into(),
            });
        }
        let si = self.next_shared_formula_index()?;
        let text = text.strip_prefix('=').unwrap_or(text).to_owned();
        let cache = crate::dynamic::formula_cache(at, result)?;
        let op = CellOp::SharedFormula(text, si, Some(range), cache);
        let plan = self.wb.plan_writes(vec![(self.sheet, at, op, crate::edit::Extra::default())])?;
        self.wb.apply_writes(plan)
    }

    /// Assigns a shared formula follower within an existing master's declared range.
    pub fn set_shared_formula_member(&mut self, at: CellIndex, master: CellIndex, result: CellValue) -> Result<()> {
        use crate::census::FKind;
        let census = &self.wb.census;
        let source = census.at(self.sheet, master).and_then(|id| census.get(id));
        let invalid = || Error::SharedFormulaMaster {
            cell: at.to_string(),
            reason: format!("expected an unambiguous shared master at {master} whose range contains the cell"),
        };
        let Some(source) = source else { return Err(invalid()) };
        let FKind::Shared { si, master: true } = source.kind else { return Err(invalid()) };
        if at == master || source.ambiguous_group || !source.range.is_some_and(|range| range.contains(at)) {
            return Err(invalid());
        }
        if self.translate_shared_formula(&source.text, master, at).is_none() {
            return Err(Error::SharedFormulaMaster { cell: at.to_string(), reason: "formula cannot be translated".into() });
        }
        let cache = crate::dynamic::formula_cache(at, result)?;
        let op = CellOp::SharedFormula(String::new(), si, None, cache);
        let plan = self.wb.plan_writes(vec![(self.sheet, at, op, crate::edit::Extra::default())])?;
        self.wb.apply_writes(plan)
    }

    /// Returns the master address, range and expression for a shared formula cell.
    pub fn shared_formula_at(&self, at: CellIndex) -> Option<(CellIndex, CellRange, &str)> {
        let census = &self.wb.census;
        let formula = census.get(census.at(self.sheet, at)?)?;
        let crate::census::FKind::Shared { si, master } = formula.kind else { return None };
        if formula.ambiguous_group {
            return None;
        }
        let source = if master { formula } else { census.get(census.master_of(self.sheet, si)?)? };
        let range = source.range?;
        range.contains(at).then_some((source.at, range, &source.text))
    }

    /// Expands a shared formula at its requested member coordinate.
    pub fn translate_shared_formula(&self, text: &str, from: CellIndex, to: CellIndex) -> Option<String> {
        let dr = i64::from(to.row) - i64::from(from.row);
        let dc = i64::from(to.col) - i64::from(from.col);
        crate::formula::translate::translate(text, dr, dc, &|name| self.wb.env.is_name(name)).ok()
    }

    /// Whether the coordinate is a non-anchor cell of an array formula.
    pub fn is_array_result_cell(&self, at: CellIndex) -> bool {
        let census = &self.wb.census;
        census.result_anchor(self.sheet, at)
            .and_then(|id| census.get(id))
            .is_some_and(|formula| formula.kind == crate::census::FKind::Array)
    }

    /// Merged, array-formula and data-table ranges that a scalar-only
    /// adapter must guard, including coordinates without stored cell nodes.
    pub fn non_scalar_ranges(&self) -> Result<Vec<CellRange>> {
        if let Some(block) = &self.wb.census.block {
            return Err(block.to_error());
        }
        let mut ranges = self.merged_ranges();
        let mut merge_parts = 0;
        let mut merge_entries = 0;
        for child in self.doc().root().elements() {
            if child.is(sml::MergeCells::QNAME) {
                merge_parts += 1;
                refuse_alternate_content(child, sml::MergeCells::QNAME)?;
                merge_entries += child.elements().filter(|item| item.is(sml::MergeCell::QNAME)).count();
            } else if child.is(MC_ALTERNATE_CONTENT) {
                let mut branch: Vec<_> = child.elements().collect();
                while let Some(node) = branch.pop() {
                    if node.is(sml::MergeCells::QNAME) || node.is(sml::MergeCell::QNAME) {
                        return Err(ooxml_core::Error::UnresolvedSibling(sml::Worksheet::QNAME).into());
                    }
                    branch.extend(node.elements());
                }
            }
        }
        if merge_parts > 1 || merge_entries != ranges.len() {
            return Err(Error::Malformed(format!("{}: incomplete merge ranges", self.part_name())));
        }
        ranges.extend(self.formula_result_ranges()?);
        Ok(ranges)
    }

    /// Array-formula and data-table result ranges: the coordinates whose
    /// value is owned by a multi-cell formula. Unlike
    /// [`Self::non_scalar_ranges`], merged ranges are not included, since a
    /// cell inside a merge still stores its own scalar value.
    pub fn formula_result_ranges(&self) -> Result<Vec<CellRange>> {
        if let Some(block) = &self.wb.census.block {
            return Err(block.to_error());
        }
        let mut ranges = Vec::new();
        for (_, formula) in self.wb.census.alive().filter(|(_, f)| f.sheet == self.sheet) {
            if let Some(range) = formula.result_range() {
                ranges.push(range);
            } else if matches!(formula.kind, crate::census::FKind::Array | crate::census::FKind::DataTable) {
                return Err(Error::Malformed(format!(
                    "{}: formula at {} has no recognized result range",
                    self.part_name(),
                    formula.at
                )));
            }
        }
        Ok(ranges)
    }
}

impl CellRef<'_> {
    fn shared_string_entry(&self) -> Result<Option<&RawNode>> {
        Ok(self.shared_string_source()?.map(|(_, node)| node))
    }

    fn shared_string_source(&self) -> Result<Option<(&XmlDocument, &RawNode)>> {
        let Some(index) = self.view.cell_value() else { return Ok(None) };
        let index = ooxml_core::value::parse_u32(&index.text())
            .ok_or_else(|| Error::Malformed("invalid shared string index".into()))?;
        // One cached index per package generation: O(1) per cell (the
        // former per-cell relationship parse and `nth` scan was quadratic).
        let entry = self.workbook.shared_string_items()?
            .and_then(|items| items.get(index).map(|node| (items.document, node)))
            .ok_or_else(|| Error::Malformed("shared string representation unavailable".into()))?;
        Ok(Some(entry))
    }

    /// Reads inline or shared rich text and its run fonts.
    pub fn rich_text(&self) -> Result<Vec<crate::RichTextRun>> {
        let Some(node) = self.rich_text_node()? else { return Ok(Vec::new()) };
        crate::registry::read::rich_text_runs(node)
    }

    /// Like [`Self::rich_text`], but each run font is read as written: name
    /// and size may be absent, and `outline` / `shadow` are accepted
    /// (see [`crate::DeclaredFont`]).
    pub fn declared_rich_text(&self) -> Result<Vec<(String, Option<crate::DeclaredFont>)>> {
        let Some(node) = self.rich_text_node()? else { return Ok(Vec::new()) };
        crate::registry::read::declared_rich_text_runs(node)
    }

    fn rich_text_node(&self) -> Result<Option<&RawNode>> {
        Ok(match self.data_type() {
            EnumValue::Known(CellValues::InlineString) => self.raw().elements().find(|node| node.is(sml::InlineString::QNAME)),
            EnumValue::Known(CellValues::SharedString) => self.shared_string_entry()?,
            _ => None,
        })
    }

    /// Whether the stored representation carries formula, rich text or cell
    /// metadata that cannot be faithfully represented by a scalar alone.
    pub fn has_non_scalar_content(&self) -> Result<bool> {
        self.has_complex_content(false)
    }

    /// Like scalar classification, but allows ordinary error cells.
    pub fn has_complex_content(&self, allow_error: bool) -> Result<bool> {
        self.has_complex_content_with_dates(allow_error, false)
    }

    /// Like [`Self::has_complex_content`], but phonetic guide data (`rPh`
    /// runs, `phoneticPr`) on a string without rich runs is not complex: the
    /// value reads as the base text, as ExcelJS 4.4.0 reads a shared string.
    /// The phonetic XML stays in its `si`/`is` unless the cell is rewritten.
    pub fn has_complex_value(&self, allow_error: bool) -> Result<bool> {
        self.complex_content(allow_error, false, true)
    }

    pub(crate) fn has_complex_content_with_dates(&self, allow_error: bool, allow_date: bool) -> Result<bool> {
        self.complex_content(allow_error, allow_date, false)
    }

    fn complex_content(&self, allow_error: bool, allow_date: bool, allow_phonetic: bool) -> Result<bool> {
        let node = self.raw();
        if self.view.cell_formula().is_some()
            || node.attr(QName::new("", "vm")).is_some()
            || node.attr(QName::new("", "cm")).is_some()
        {
            return Ok(true);
        }

        let has_runs = |node: &RawNode| {
            node.elements().any(|child| {
                child.name().ns() == crate::SML_NS
                    && match child.name().local() {
                        "r" => true,
                        "rPh" | "phoneticPr" => !allow_phonetic,
                        _ => false,
                    }
            })
        };
        match self.data_type() {
            EnumValue::Known(CellValues::Error) => Ok(!allow_error),
            EnumValue::Known(CellValues::Date) => Ok(!allow_date),
            EnumValue::Known(CellValues::InlineString) => {
                Ok(self.view.inline_string().is_some_and(|inline| has_runs(inline.raw())))
            }
            EnumValue::Known(CellValues::SharedString) => {
                Ok(self.shared_string_entry()?.is_some_and(has_runs))
            }
            _ => Ok(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{check_insertable, format_number};
    use ooxml_core::{ChildOf, ElementSpec, XmlDocument};
    use ooxml_schema::sml;

    const NS: &str = r#"xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006""#;

    /// Compares the dry run with ooxml-core's real insertion for `C` in `P`
    /// on one shape (Phase0 review round 3, R3-1: the copy must not drift).
    fn agree<P: ElementSpec, C: ChildOf<P>>(root: &str, inner: &str) -> bool {
        let xml = format!("<{root} {NS}>{inner}</{root}>");
        let doc = XmlDocument::parse(xml.as_bytes()).unwrap();
        let dry = check_insertable::<P>(doc.root().elements(), C::QNAME);
        let mut doc2 = XmlDocument::parse(xml.as_bytes()).unwrap();
        let real = doc2.lens_mut::<P>().unwrap().insert::<C>().map(|_| ());
        assert_eq!(dry.is_ok(), real.is_ok(), "<{root}>{inner} + {:?}: dry {dry:?} vs core {real:?}", C::QNAME);
        dry.is_ok()
    }

    /// `check_insertable` agrees with ooxml-core's `place` on every
    /// parent/child pair the write path inserts, over generated shapes:
    /// every ordering of up to three of the parent's listed children
    /// (including duplicates), an `mc:AlternateContent`, an unlisted child.
    #[test]
    fn dry_run_matches_core_for_every_inserted_pair() {
        fn shapes(names: &[&str]) -> Vec<String> {
            let mut atoms: Vec<String> = names.iter().map(|n| format!("<{n}/>")).collect();
            atoms.push("<mc:AlternateContent/>".into());
            atoms.push(r#"<foo xmlns="urn:x"/>"#.into());
            let mut out = vec![String::new()];
            for a in &atoms {
                out.push(a.clone());
                for b in &atoms {
                    out.push(format!("{a}{b}"));
                    for c in &atoms {
                        out.push(format!("{a}{b}{c}"));
                    }
                }
            }
            out
        }
        let mut ok = (0, 0);
        for s in shapes(&["f", "v", "is", "extLst"]) {
            for r in [agree::<sml::Cell, sml::CellValue>("c", &s), agree::<sml::Cell, sml::InlineString>("c", &s), agree::<sml::Cell, sml::CellFormula>("c", &s)] {
                if r { ok.0 += 1 } else { ok.1 += 1 }
            }
        }
        for s in shapes(&["c", "extLst"]) {
            agree::<sml::Row, sml::Cell>("row", &s);
        }
        for s in shapes(&["row"]) {
            agree::<sml::SheetData, sml::Row>("sheetData", &s);
        }
        for s in shapes(&["sheetPr", "dimension", "sheetData", "mergeCells", "extLst"]) {
            agree::<sml::Worksheet, sml::SheetData>("worksheet", &s);
        }
        assert!(ok.0 > 10 && ok.1 > 10, "both outcomes exercised: {ok:?}");
    }

    #[test]
    fn read_views_expose_indexed_coordinates() {
        let mut book = crate::Workbook::new().unwrap();
        let mut sheet = book.worksheet_by_name("Sheet1").unwrap();
        for (row, col) in [(2, 7), (4, 1), (4, 27)] {
            sheet.cell_mut(row, col).unwrap().set_number(1.0).unwrap();
        }
        let coordinates: Vec<_> = sheet
            .cells()
            .map(|cell| {
                let cell = cell.unwrap();
                let at = cell.index();
                assert_eq!(cell.reference(), at.to_string());
                assert_eq!(sheet.cell(at.row, at.col).unwrap().unwrap().index(), at);
                (at.row, at.col)
            })
            .collect();
        assert_eq!(coordinates, [(2, 7), (4, 1), (4, 27)]);
        let prefix: Vec<_> = sheet
            .cells_before_row(3)
            .map(|cell| cell.unwrap().index())
            .collect();
        assert_eq!(prefix.len(), 1);
        assert_eq!((prefix[0].row, prefix[0].col), (2, 7));
        assert_eq!(sheet.cells_before_row(0).count(), 0);
    }

    #[test]
    fn formula_result_ranges_exclude_merges() {
        use crate::CellRange;
        let mut book = crate::Workbook::new().unwrap();
        let mut sheet = book.worksheet_by_name("Sheet1").unwrap();
        let anchor = CellRange::parse("A1:B1").unwrap().first;
        sheet.cell_mut(anchor.row, anchor.col).unwrap().set_string("merged").unwrap();
        sheet.merge("A1:B1").unwrap();
        let array = CellRange::parse("D1:D2").unwrap();
        sheet.cell_mut(array.first.row, array.first.col).unwrap()
            .set_array_formula("1+2", array, false).unwrap();
        let bytes = book.to_bytes().unwrap();
        let mut reopened = crate::Workbook::from_bytes(bytes).unwrap();
        let sheet = reopened.worksheet_by_name("Sheet1").unwrap();
        let merged = CellRange::parse("A1:B1").unwrap();
        assert_eq!(sheet.formula_result_ranges().unwrap(), [array]);
        assert_eq!(sheet.non_scalar_ranges().unwrap(), [merged, array]);
    }

    #[test]
    fn numbers() {
        assert_eq!(format_number(90.0), "90");
        assert_eq!(format_number(-0.0), "0");
        assert_eq!(format_number(0.1), "0.1");
        assert_eq!(format_number(3.5), "3.5");
        assert_eq!(format_number(1e300), "1E300");
        assert_eq!(format_number(1.5e-7), "1.5E-7");
        assert_eq!(format_number(123456789012345.0), "123456789012345");
        for v in [1e300, 1.5e-7, 0.1, 2.0f64.sqrt(), -1e-300, f64::MAX, f64::MIN_POSITIVE] {
            assert_eq!(format_number(v).parse::<f64>().unwrap(), v);
            assert!(ooxml_core::value::parse_f64(&format_number(v)).is_some());
        }
    }
}
