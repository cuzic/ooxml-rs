//! [`Worksheet`], [`CellRef`] (read) and [`CellMut`] (write).

use ooxml_core::name::MC_ALTERNATE_CONTENT;
use ooxml_core::{ElementSpec, EnumValue, QName, RawNode, XmlDocument};
use ooxml_schema::sml::{self, CellValues, TextView};

use crate::error::{Error, Result};
use crate::escape;
use crate::reference::{CellIndex, CellRange};
use crate::workbook::SheetContext;

/// "sheet1" of a [`Workbook`](crate::Workbook), borrowed from it.
///
/// Rows and columns are 0-based (`A1` = `(0, 0)`).
#[derive(Debug)]
pub struct Worksheet<'wb> {
    doc: &'wb mut XmlDocument,
    ctx: &'wb SheetContext,
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
    ctx: &'a SheetContext,
}

/// Concatenates `t` texts (rich runs included) the way Excel and calamine
/// read them: a `t` without `xml:space="preserve"` in effect has its outer
/// ASCII whitespace removed; `_xHHHH_` escapes are decoded.
pub(crate) fn text_of<'a>(t: Option<TextView<'a>>, runs: impl Iterator<Item = Option<TextView<'a>>>) -> String {
    let mut out = String::new();
    for t in t.into_iter().chain(runs.flatten()) {
        let raw = t.text();
        let raw = if t.0.xml_space_preserve() { &raw[..] } else { raw.trim_matches([' ', '\t', '\r', '\n']) };
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
                let sst = self.ctx.shared_strings.as_ref().ok_or_else(|| bad("no shared-string table".into()))?;
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
struct Located {
    /// Item index of the `row` in `sheetData`, if it exists.
    row: Option<usize>,
    /// Item index of the `c` in the row, if it exists.
    cell: Option<usize>,
}

fn row_number(n: &RawNode) -> Option<&str> {
    n.attr(sml::Row::ATTR_R)
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

impl<'wb> Worksheet<'wb> {
    pub(crate) fn new(doc: &'wb mut XmlDocument, ctx: &'wb SheetContext) -> Self {
        Self { doc, ctx }
    }

    /// The sheet's name.
    pub fn name(&self) -> &str {
        &self.ctx.sheet_name
    }

    /// The worksheet part's name.
    pub fn part_name(&self) -> &str {
        &self.ctx.sheet_part
    }

    fn view(&self) -> Result<sml::WorksheetView<'_>> {
        Ok(sml::WorksheetView(self.doc.lens::<sml::Worksheet>()?))
    }

    /// Finds the row and cell, enforcing ADR-0002 §4.2's `MissingReference`
    /// rule (any `row` without `r`, or any `c` without `r` in the row
    /// concerned) and checking the order that insertion relies on.
    ///
    /// An `mc:AlternateContent` child of `sheetData`, of the row concerned,
    /// or of `worksheet` when `sheetData` is absent could hold the cell, so
    /// it is refused rather than answered with "absent" (review round 1, F6).
    fn locate(&self, at: CellIndex) -> Result<Located> {
        let ws = self.view()?;
        let Some(sd) = ws.sheet_data() else {
            refuse_alternate_content(ws.raw(), sml::Worksheet::QNAME)?;
            return Ok(Located { row: None, cell: None });
        };
        refuse_alternate_content(sd.raw(), sml::SheetData::QNAME)?;
        let part = &self.ctx.sheet_part;
        let missing = |what: String| Error::MissingReference { part: part.clone(), what };
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
        Ok(Located { row: Some(ri), cell: found_cell })
    }

    /// Reads a cell. `Ok(None)` if it does not exist. Never changes the tree.
    ///
    /// Errors: [`Error::MissingReference`] if the sheet has a `row` without
    /// `r`, or the row has a `c` without `r` (ADR-0002 §4.2: a silent `None`
    /// could hide a value); [`Error::OutOfOrder`] if rows or the row's
    /// cells are not ascending; core `UnresolvedSibling` if `sheetData` or
    /// the row has an `mc:AlternateContent` child (or `worksheet` has one and
    /// no `sheetData`), for the same reason; [`Error::OutOfRange`].
    pub fn cell(&self, row: u32, col: u32) -> Result<Option<CellRef<'_>>> {
        let at = CellIndex::new(row, col)?;
        let Located { row: Some(ri), cell: Some(ci) } = self.locate(at)? else { return Ok(None) };
        let ws = self.view()?;
        let sd = ws.sheet_data().expect("located");
        let row_node = sd.raw().children()[ri].as_element().expect("row");
        let c = row_node.children()[ci].as_element().expect("cell");
        let row_lens = sd.0.children::<sml::Row>().find(|r| std::ptr::eq(r.raw(), row_node)).expect("row lens");
        let view = sml::CellView(row_lens.children::<sml::Cell>().find(|l| std::ptr::eq(l.raw(), c)).expect("cell lens"));
        Ok(Some(CellRef { view, ctx: self.ctx }))
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

    /// Every check that can refuse a write, before anything is changed: once
    /// this returns `Ok`, [`Self::write`] cannot fail part-way (review round
    /// 1, F1). The remaining fallible steps of `write` are insertions whose
    /// parent was checked here (`sheetData`, the row and the cell have no
    /// `mc:AlternateContent` child, rows and cells are ascending) or that go
    /// into a node `write` has just created.
    fn preflight(&self, at: CellIndex) -> Result<Located> {
        if let Some(block) = &self.ctx.edit_block {
            return Err(block.to_error());
        }
        let cell = at.to_string();
        if let Some(m) = self.ctx.merges.iter().find(|m| m.contains(at)) {
            return Err(Error::MergedCell { cell, range: m.to_string() });
        }
        if let Some(t) = self.ctx.tables.iter().find(|t| t.rows.is_some_and(|r| r.contains(at))) {
            return Err(Error::TableHeaderCell { cell, table: t.name.clone() });
        }
        let loc = self.locate(at)?;
        if let Some(c) = self.cell(at.row, at.col)? {
            if c.view.cell_formula().is_some() {
                return Err(Error::FormulaPresent { part: self.ctx.sheet_part.clone() });
            }
            if c.raw().attr(sml::Cell::ATTR_VM).is_some() {
                return Err(Error::RichValueCell { cell });
            }
            // the setters remove the old value and then insert `v`/`is`
            // next to the remaining children; an AC there would fail only
            // after the removal
            refuse_alternate_content(c.raw(), sml::Cell::QNAME)?;
        }
        // `write` grows `dimension` after the cell is set; an unusable `ref`
        // would silently stay stale (review round 1, F7)
        if let Some(dim) = self.view()?.sheet_dimension() {
            let r = dim.reference();
            if r.is_none_or(|r| CellRange::parse(r).is_err()) {
                return Err(Error::Malformed(format!(
                    "{}: dimension/@ref {:?} is not a cell range; it could not be kept up to date",
                    self.ctx.sheet_part,
                    r.unwrap_or_default()
                )));
            }
        }
        Ok(loc)
    }

    /// Creates the row and cell if needed and hands the cell lens to `f`;
    /// then grows `dimension` if it exists (never inserts one).
    fn write(&mut self, at: CellIndex, f: impl FnOnce(&mut sml::CellViewMut<'_>) -> Result<()>) -> Result<()> {
        let loc = self.preflight(at)?;
        let row_no = at.row + 1;
        let col = at.col;
        {
            let mut ws = sml::WorksheetViewMut(self.doc.lens_mut::<sml::Worksheet>()?);
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
            let mut cell = match loc.cell {
                Some(i) => sml::CellViewMut(row.0.into_child_at::<sml::Cell>(i).expect("located cell")),
                None => {
                    let mut c = row.insert_cell_by(|_, n| {
                        n.attr(sml::Cell::ATTR_R).and_then(|r| CellIndex::parse(r).ok()).is_some_and(|c| c.col > col)
                    })?;
                    c.set_cell_reference(&at.to_string())?;
                    c
                }
            };
            f(&mut cell)?;
        }
        let mut ws = sml::WorksheetViewMut(self.doc.lens_mut::<sml::Worksheet>()?);
        if let Some(mut dim) = ws.sheet_dimension_mut()
            && let Some(Ok(range)) = dim.as_view().reference().map(CellRange::parse)
            && !range.contains(at)
        {
            dim.set_reference(&range.union(at).to_string())?;
        }
        Ok(())
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
        let text = format_number(value);
        self.ws.write(self.at, |c| {
            c.remove_data_type()?;
            c.remove_inline_string();
            c.get_or_insert_cell_value()?.set_text(&text)?;
            Ok(())
        })
    }

    /// Writes a string as an inline string (`t="inlineStr"`,
    /// `<is><t>…</t></is>`; ADR-0001 Phase0 never touches the shared-string
    /// table). `xml:space="preserve"` is set when the value has leading or
    /// trailing whitespace; characters XML cannot carry are written as
    /// `_xHHHH_`. `s` is kept. At most 32,767 UTF-16 code units
    /// ([`Error::StringTooLong`]).
    ///
    /// Errors (nothing is changed): [`Error::FormulaPresent`] /
    /// [`Error::FormulaScanFailed`] (workbook-wide), [`Error::MergedCell`],
    /// [`Error::TableHeaderCell`], [`Error::RichValueCell`],
    /// [`Error::MissingReference`], [`Error::OutOfOrder`],
    /// [`Error::Malformed`] if `dimension/@ref` is not a cell range;
    /// core `UnresolvedSibling` if `sheetData`, the row or the cell has an
    /// `mc:AlternateContent` child, or if `sheetData` itself must be
    /// inserted next to one.
    pub fn set_string(self, value: &str) -> Result<()> {
        let units = value.encode_utf16().count();
        if units > MAX_STRING_LEN {
            return Err(Error::StringTooLong(units));
        }
        let text = escape::encode(value);
        self.ws.write(self.at, |c| {
            c.remove_cell_value();
            c.remove_inline_string();
            c.set_data_type(CellValues::InlineString)?;
            c.get_or_insert_inline_string()?.get_or_insert_text()?.set_text(&text)?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::format_number;

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
