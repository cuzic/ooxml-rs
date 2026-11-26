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

    /// The formula text, if the cell has an `f` element.
    pub fn formula(&self) -> Option<String> {
        self.view.cell_formula().map(|f| f.text().into_owned())
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
        CellOp::Number(t) | CellOp::Text(t) | CellOp::Formula(t) | CellOp::DynamicFormula(t, _) => Some(t.as_str()),
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
        CellOp::Number(_) | CellOp::Shared(_) | CellOp::Bool(_) | CellOp::Error(_) => Some(sml::CellValue::QNAME),
        CellOp::Text(_) | CellOp::InlineRich(_) => Some(sml::InlineString::QNAME),
        CellOp::Formula(_) | CellOp::DynamicFormula(..) => Some(sml::CellFormula::QNAME),
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
                            let kept = c.elements().filter(|n| !gone.iter().any(|g| n.is(*g)));
                            check_insertable::<sml::Cell>(kept, q)?;
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
            (!range.contains(at)).then(|| range.union(at).to_string())
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
        CellOp::Number(_) | CellOp::Shared(_) | CellOp::Bool(_) | CellOp::Error(_) => first(sml::CellValue::QNAME),
        CellOp::Formula(_) | CellOp::DynamicFormula(..) => first(sml::CellFormula::QNAME),
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
            CellOp::Number(text) => {
                c.remove_data_type()?;
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
            CellOp::Formula(text) | CellOp::DynamicFormula(text, _) => {
                if let CellOp::DynamicFormula(_, cm) = op {
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
                if matches!(op, CellOp::DynamicFormula(..)) {
                    f.set_formula_type(sml::CellFormulaValues::Array)?;
                    f.set_reference(&at.to_string())?;
                }
                f.set_text(text)?;
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
        Self { wb, sheet }
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
    fn cell_ref<'s>(&'s self, c: &'s RawNode) -> Result<CellRef<'s>> {
        Ok(CellRef { view: sml::CellView(ooxml_core::Lens::root(c)?), sst: self.wb.shared_strings.as_deref(), workbook: self.wb })
    }

    /// Every existing cell, in document order (row by row, left to right):
    /// a bulk read in one pass over the index, instead of one lookup per
    /// cell. A row whose cells cannot be located yields its error and the
    /// iteration continues; a sheet-level problem (a `row` without `r`, rows
    /// out of order, `mc:AlternateContent` around rows) yields one error.
    pub fn cells(&self) -> impl Iterator<Item = Result<CellRef<'_>>> + '_ {
        let part = self.part_name();
        let index = self.index();
        let sd = sheet_data(self.doc());
        let rows: Vec<Result<&crate::index::RowEntry>> = match index.target_rows(part) {
            Ok(rows) => rows.iter().map(Ok).collect(),
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
            Box::new(cells.iter().map(move |&(_, ci)| {
                let c = row.children()[ci].as_element().expect("indexed cell");
                refuse_alternate_content(c, sml::Cell::QNAME)?;
                for is in c.elements().filter(|n| n.is(sml::InlineString::QNAME)) {
                    refuse_alternate_content(is, sml::InlineString::QNAME)?;
                }
                self.cell_ref(c)
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
        Ok(Some(self.cell_ref(c)?))
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
    /// As [`CellMut::set_formula`] (file form; one leading `=` is dropped).
    Formula(&'a str),
    /// As [`CellMut::remove_formula`].
    RemoveFormula,
    /// As [`CellMut::clear_value`].
    ClearValue,
}

impl<'wb> Worksheet<'wb> {
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
        let mut ops = Vec::with_capacity(writes.len());
        for &(at, w) in writes {
            let (op, extra) = self.wb.plan_write_value(at, w)?;
            ops.push((self.sheet, at, op, extra));
        }
        self.wb.write_with_headers(ops)
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
                CellOp::Formula(t.strip_prefix('=').unwrap_or(t).to_owned()),
                Default::default(),
            ),
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
        let text = text.strip_prefix('=').unwrap_or(text);
        let ws = &mut *self.ws;
        ws.wb.edit_cell(ws.sheet, self.at, CellOp::Formula(text.to_owned()))
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
