//! [`Worksheet`], [`CellRef`] (read) and [`CellMut`] (write).

use ooxml_core::name::MC_ALTERNATE_CONTENT;
use ooxml_core::{Content, ElementSpec, EnumValue, QName, RawNode, XmlDocument};
use ooxml_schema::sml::{self, CellValues, TextView};

use crate::edit::CellOp;
use crate::error::{Error, Result};
use crate::escape;
use crate::reference::{CellIndex, CellRange};
use crate::workbook::Workbook;

/// A worksheet of a [`Workbook`], borrowed from it mutably: an edit can
/// reach other sheets, `workbook.xml` and the package (ADR-0003 §6).
///
/// Rows and columns are 0-based (`A1` = `(0, 0)`).
#[derive(Debug)]
pub struct Worksheet<'wb> {
    wb: &'wb mut Workbook,
    sheet: usize,
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

/// Where a cell is (or would go) in `sheetData`; computed read-only.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Located {
    /// Item index of the `row` in `sheetData`, if it exists.
    pub(crate) row: Option<usize>,
    /// Item index of the `c` in the row, if it exists.
    pub(crate) cell: Option<usize>,
}

fn row_number(n: &RawNode) -> Option<&str> {
    n.attr(sml::Row::ATTR_R)
}

/// Finds the row and cell, enforcing ADR-0002 §4.2's `MissingReference`
/// rule (any `row` without `r`, or any `c` without `r` in the row
/// concerned) and checking the order that insertion relies on.
///
/// An `mc:AlternateContent` child of `sheetData`, of the row concerned, of
/// the cell or its `is`, or of `worksheet` when `sheetData` is absent could
/// hold the cell or its value, so it is refused rather than answered with
/// "absent" or "empty" (Phase0 review round 1 F6, round 2 N3).
pub(crate) fn locate(doc: &XmlDocument, part: &str, at: CellIndex) -> Result<Located> {
    let ws = sml::WorksheetView(doc.lens::<sml::Worksheet>()?);
    let Some(sd) = ws.sheet_data() else {
        refuse_alternate_content(ws.raw(), sml::Worksheet::QNAME)?;
        return Ok(Located { row: None, cell: None });
    };
    refuse_alternate_content(sd.raw(), sml::SheetData::QNAME)?;
    let missing = |what: String| Error::MissingReference { part: part.to_owned(), what };
    let want_row = at.row + 1;
    let mut found_row = None;
    let mut prev = 0u32;
    for (i, n) in sd.raw().indexed_elements().filter(|(_, n)| n.is(sml::Row::QNAME)) {
        let r = row_number(n).ok_or_else(|| missing("row".into()))?;
        let r = ooxml_core::value::parse_u32(r).filter(|r| *r >= 1).ok_or_else(|| Error::InvalidReference(r.into()))?;
        if r <= prev {
            return Err(Error::OutOfOrder(format!("rows in {part}")));
        }
        prev = r;
        if r == want_row {
            found_row = Some(i);
        }
    }
    let Some(ri) = found_row else { return Ok(Located { row: None, cell: None }) };
    let row = sd.raw().children()[ri].as_element().expect("row index");
    refuse_alternate_content(row, sml::Row::QNAME)?;
    let mut found_cell = None;
    let mut prev: Option<u32> = None;
    for (i, c) in row.indexed_elements().filter(|(_, n)| n.is(sml::Cell::QNAME)) {
        let r = c.attr(sml::Cell::ATTR_R).ok_or_else(|| missing(format!("cell in row {want_row}")))?;
        let idx = CellIndex::parse(r)?;
        if idx.row != at.row {
            return Err(Error::InvalidReference(format!("{r} in row {want_row}")));
        }
        if prev.is_some_and(|p| idx.col <= p) {
            return Err(Error::OutOfOrder(format!("cells in row {want_row} of {part}")));
        }
        prev = Some(idx.col);
        if idx.col == at.col {
            found_cell = Some(i);
        }
    }
    if let Some(ci) = found_cell {
        // an AC inside the cell or its `is` may hold the value (N3)
        let c = row.children()[ci].as_element().expect("cell index");
        refuse_alternate_content(c, sml::Cell::QNAME)?;
        for is in c.elements().filter(|n| n.is(sml::InlineString::QNAME)) {
            refuse_alternate_content(is, sml::InlineString::QNAME)?;
        }
    }
    Ok(Located { row: Some(ri), cell: found_cell })
}

/// The `c` element at a located position.
pub(crate) fn located_cell<'d>(doc: &'d XmlDocument, loc: &Located) -> Option<&'d RawNode> {
    let (Some(ri), Some(ci)) = (loc.row, loc.cell) else { return None };
    let sd = doc.root().elements().find(|e| e.is(sml::SheetData::QNAME))?;
    sd.children()[ri].as_element()?.children()[ci].as_element()
}

/// `Err(UnresolvedSibling(parent))` if `n` has an `mc:AlternateContent`
/// child: what it wraps (rows, cells, values) cannot be located without
/// MCE processing, so neither a read nor a write can be answered honestly.
fn refuse_alternate_content(n: &RawNode, parent: QName) -> Result<()> {
    if n.elements().any(|c| c.is(MC_ALTERNATE_CONTENT)) {
        return Err(ooxml_core::Error::UnresolvedSibling(parent).into());
    }
    Ok(())
}

/// Dry run of `LensMut::insert_by::<C>` on a parent `E` whose element
/// children would be `children` (ooxml-core's `place`, ADR-0002 §4.1, without
/// mutating anything): `Err` exactly when that insertion would be refused
/// (an `mc:AlternateContent` sibling, listed children out of schema order,
/// or the slot already full). Unlisted children are skipped, as in `place`.
///
/// The new child's namespace prefix cannot be refused: it is always
/// SpreadsheetML main, the namespace of `E` itself, so the parent's own
/// prefix is bound to it.
fn check_insertable<'n, E: ElementSpec>(children: impl Iterator<Item = &'n RawNode>, q: QName) -> Result<()> {
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
}

/// Decides the target write without changing anything: where the row and
/// cell go, that each insertion `write_cell` will make is possible (dry run
/// against the children it will actually have), the value's characters, the
/// `vm` guard and the new `dimension/@ref` (a `dimension` that cannot be
/// kept up to date is refused, round 1 F7). Once this returns `Ok`,
/// [`write_cell`] cannot fail.
pub(crate) fn plan_target(doc: &XmlDocument, part: &str, at: CellIndex, op: &CellOp) -> Result<TargetPlan> {
    let text = match op {
        CellOp::Number(t) | CellOp::Text(t) | CellOp::Formula(t) => Some(t.as_str()),
        CellOp::RemoveFormula => None,
    };
    if let Some(t) = text
        && let Some((offset, ch)) = ooxml_core::value::first_invalid_xml_char(t)
    {
        return Err(ooxml_core::Error::InvalidXmlChar { ch, offset }.into());
    }
    let loc = locate(doc, part, at)?;
    let ws = sml::WorksheetView(doc.lens::<sml::Worksheet>()?);
    let new_child = match op {
        CellOp::Number(_) => Some(sml::CellValue::QNAME),
        CellOp::Text(_) => Some(sml::InlineString::QNAME),
        CellOp::Formula(_) => Some(sml::CellFormula::QNAME),
        CellOp::RemoveFormula => None,
    };
    match ws.sheet_data() {
        None => check_insertable::<sml::Worksheet>(ws.raw().elements(), sml::SheetData::QNAME)?,
        Some(sd) => match loc.row {
            None => check_insertable::<sml::SheetData>(sd.raw().elements(), sml::Row::QNAME)?,
            Some(ri) => {
                let row = sd.raw().children()[ri].as_element().expect("located row");
                match loc.cell {
                    None => check_insertable::<sml::Row>(row.elements(), sml::Cell::QNAME)?,
                    Some(ci) => {
                        let c = row.children()[ci].as_element().expect("located cell");
                        if c.attr(sml::Cell::ATTR_VM).is_some() {
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
    Ok(TargetPlan { loc, dimension })
}

/// Writes the target cell (after the plan validated everything): creates the
/// row and cell if needed, applies `op`, then grows `dimension` if it exists
/// (never inserts one).
///
/// None of the `?` below can fire after [`plan_target`]: each insertion was
/// dry-run against the same children, the text was checked for XML
/// characters, and the attribute writes are on `Cell`/`Row`/`SheetDimension`,
/// whose attribute lists are complete.
pub(crate) fn write_cell(doc: &mut XmlDocument, plan: &TargetPlan, at: CellIndex, op: &CellOp) -> Result<()> {
    let loc = plan.loc;
    let row_no = at.row + 1;
    let col = at.col;
    {
        let mut ws = sml::WorksheetViewMut(doc.lens_mut::<sml::Worksheet>()?);
        let mut sd = ws.get_or_insert_sheet_data()?;
        let mut row = match loc.row {
            Some(i) => sml::RowViewMut(sd.0.into_child_at::<sml::Row>(i).expect("located row")),
            None => {
                let mut r = sd.insert_row_by(|_, n| {
                    row_number(n).and_then(ooxml_core::value::parse_u32).is_some_and(|r| r > row_no)
                })?;
                r.set_row_index(row_no)?;
                r
            }
        };
        let mut c = match loc.cell {
            Some(i) => sml::CellViewMut(row.0.into_child_at::<sml::Cell>(i).expect("located cell")),
            None => {
                let mut c = row.insert_cell_by(|_, n| {
                    n.attr(sml::Cell::ATTR_R).and_then(|r| CellIndex::parse(r).ok()).is_some_and(|c| c.col > col)
                })?;
                c.set_cell_reference(&at.to_string())?;
                c
            }
        };
        // every `f`, `v` and `is`, not just the first: a schema-invalid
        // duplicate would otherwise survive and be read by others
        while c.remove_cell_formula() {}
        while c.remove_cell_value() {}
        while c.remove_inline_string() {}
        match op {
            CellOp::Number(text) => {
                c.remove_data_type()?;
                c.get_or_insert_cell_value()?.set_text(text)?;
            }
            CellOp::Text(text) => {
                c.set_data_type(CellValues::InlineString)?;
                c.get_or_insert_inline_string()?.get_or_insert_text()?.set_text(text)?;
            }
            CellOp::Formula(text) => {
                c.remove_data_type()?;
                c.get_or_insert_cell_formula()?.set_text(text)?;
            }
            CellOp::RemoveFormula => {
                c.remove_data_type()?;
            }
        }
    }
    if let Some(r) = &plan.dimension {
        let mut ws = sml::WorksheetViewMut(doc.lens_mut::<sml::Worksheet>()?);
        ws.sheet_dimension_mut().expect("planned with a dimension").set_reference(r)?;
    }
    Ok(())
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

    /// Reads a cell. `Ok(None)` if it does not exist. Never changes the tree.
    ///
    /// Errors: [`Error::MissingReference`] if the sheet has a `row` without
    /// `r`, or the row has a `c` without `r` (ADR-0002 §4.2: a silent `None`
    /// could hide a value); [`Error::OutOfRange`].
    pub fn cell(&self, row: u32, col: u32) -> Result<Option<CellRef<'_>>> {
        let at = CellIndex::new(row, col)?;
        let doc = self.doc();
        let loc = locate(doc, self.part_name(), at)?;
        let (Some(ri), Some(ci)) = (loc.row, loc.cell) else { return Ok(None) };
        let ws = sml::WorksheetView(doc.lens::<sml::Worksheet>()?);
        let sd = ws.sheet_data().expect("located");
        let row_node = sd.raw().children()[ri].as_element().expect("row");
        let c = row_node.children()[ci].as_element().expect("cell");
        let row_lens = sd.0.children::<sml::Row>().find(|r| std::ptr::eq(r.raw(), row_node)).expect("row lens");
        let view = sml::CellView(row_lens.children::<sml::Cell>().find(|l| std::ptr::eq(l.raw(), c)).expect("cell lens"));
        Ok(Some(CellRef { view, sst: self.wb.shared_strings.as_deref() }))
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
/// happens until a value is set).
#[derive(Debug)]
pub struct CellMut<'s, 'wb> {
    ws: &'s mut Worksheet<'wb>,
    at: CellIndex,
}

/// Excel's cell text limit, in UTF-16 code units.
const MAX_STRING_LEN: usize = 32_767;

/// Formats a finite number as `xsd:double`: plain decimal in the usual
/// range, `1E300`-style outside it; `-0` is written as `0`.
fn format_number(v: f64) -> String {
    let a = v.abs();
    if v == 0.0 {
        "0".into()
    } else if (1e-5..1e16).contains(&a) {
        format!("{v}")
    } else {
        format!("{v:E}")
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

    /// Writes a string as an inline string (`t="inlineStr"`,
    /// `<is><t>…</t></is>`; ADR-0001 Phase0 never touches the shared-string
    /// table). `xml:space="preserve"` is set when the value has leading or
    /// trailing whitespace; characters XML cannot carry are written as
    /// `_xHHHH_`. `s` is kept. At most 32,767 UTF-16 code units
    /// ([`Error::StringTooLong`]).
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
        let units = value.encode_utf16().count();
        if units > MAX_STRING_LEN {
            return Err(Error::StringTooLong(units));
        }
        let ws = &mut *self.ws;
        ws.wb.edit_cell(ws.sheet, self.at, CellOp::Text(escape::encode(value).into_owned()))
    }

    /// Writes a formula (ADR-0003 §4.1): `<f>text</f>` with no cached value
    /// and no `t`; one leading `=` is dropped (so `"==A1"` is refused). Dependents lose their cached
    /// values, `fullCalcOnLoad` is set (except in manual calculation mode),
    /// and `calcChain.xml` is removed when the cell was not a formula cell.
    ///
    /// Errors (nothing is changed): [`Error::InvalidFormula`] if the text
    /// does not lex or holds characters XML cannot carry; the refusals of
    /// [`Self::set_number`].
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
