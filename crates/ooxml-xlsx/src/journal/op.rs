//! Typed keyed ops (ADR-0024 §4.1 part A, cells), their origins, the keys
//! they write (§4.2, §4.3) and the one sequential call that applies an op.

use super::ids::{ColId, Epoch, RowId, SheetId};
use crate::error::Result;
use crate::reference::CellIndex;
use crate::workbook::Workbook;
use crate::{Border, CellWrite, ExcelDateTime, Fill, Font, Format};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Api {
    Native,
    Python,
    SheetJs,
    ExcelJs,
}

/// Where an op came from. `Base` is never stored: the absence of an op means Base (§4.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Origin {
    Session { seq: u64, api: Api },
}

/// An op as the caller gives it, in the caller's coordinates.
#[derive(Clone, Debug)]
pub(crate) struct OpInput {
    pub(crate) sheet: usize,
    pub(crate) kind: OpKind,
}

#[derive(Clone, Debug)]
#[expect(clippy::large_enum_variant, reason = "ops are few and short-lived")]
pub(crate) enum OpKind {
    Cell {
        at: CellIndex,
        call: CellCall,
    },
    WriteCells(Vec<(CellIndex, WriteValue)>),
    /// A row property (`row` 0-based). Targets no cell (§4.4), takes no
    /// snapshot, is never restored; its key is per property (§4.3).
    Row { row: u32, call: RowCall },
    /// A column property over `first..=last` (0-based, inclusive); one key
    /// per column of the range. Like [`OpKind::Row`], no cell is a target.
    Cols {
        first: u32,
        last: u32,
        call: ColCall,
    },
}

/// One row-property setter with its argument.
#[derive(Clone, Copy, Debug)]
pub(crate) enum RowCall {
    Height(f64),
    Hidden(bool),
    Outline(u8),
}

/// One column-property setter with its argument.
#[derive(Clone, Copy, Debug)]
pub(crate) enum ColCall {
    Width(f64),
    Hidden(bool),
    Outline(u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum RowProp {
    Height,
    Hidden,
    Outline,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum ColProp {
    Width,
    Hidden,
    Outline,
}

pub(crate) fn row_prop(c: &RowCall) -> RowProp {
    match c {
        RowCall::Height(_) => RowProp::Height,
        RowCall::Hidden(_) => RowProp::Hidden,
        RowCall::Outline(_) => RowProp::Outline,
    }
}

pub(crate) fn col_prop(c: &ColCall) -> ColProp {
    match c {
        ColCall::Width(_) => ColProp::Width,
        ColCall::Hidden(_) => ColProp::Hidden,
        ColCall::Outline(_) => ColProp::Outline,
    }
}

/// One `CellMut` setter with its arguments.
#[derive(Clone, Debug)]
pub(crate) enum CellCall {
    Number(f64),
    String(String),
    StringInline(String),
    RichString(Vec<(Font, String)>),
    Bool(bool),
    Error(String),
    DateTime(ExcelDateTime),
    Formula(String),
    RemoveFormula,
    /// P1: `CellMut::clear_value` (a conditional content op).
    ClearValue,
    /// `CellMut::set_dynamic_formula`.
    DynamicFormula(String),
    NumberWithFormat(f64, Format),
    StringWithFormat(String, Format),
    FormulaWithFormat(String, Format),
    DateTimeWithFormat(ExcelDateTime, Format),
    Format(Format),
    NumFormat(String),
    Font(Font),
    Fill(Fill),
    Border(Border),
    StyleIndex(u32),
    NamedStyle(String),
}

#[derive(Clone, Debug)]
pub(crate) enum WriteValue {
    Number(f64),
    String(String),
    Formula(String),
    RemoveFormula,
    ClearValue,
}

/// An op as logged: positions by id.
#[derive(Clone, Debug)]
#[expect(clippy::large_enum_variant, reason = "ops are few and short-lived")]
pub(crate) enum Op {
    Cell {
        sheet: SheetId,
        row: RowId,
        col: ColId,
        call: CellCall,
    },
    WriteCells {
        sheet: SheetId,
        cells: Vec<(RowId, ColId, WriteValue)>,
    },
    Row {
        sheet: SheetId,
        id: RowId,
        call: RowCall,
    },
    /// `first..=last` is contiguous when logged.
    Cols {
        sheet: SheetId,
        first: ColId,
        last: ColId,
        call: ColCall,
    },
}

#[derive(Clone, Debug)]
pub(crate) struct LoggedOp {
    pub(crate) seq: u64,
    pub(crate) origin: Origin,
    pub(crate) born: Epoch,
    /// Always true in J0: R1 is off.
    pub(crate) applied: bool,
    pub(crate) op: Op,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum StyleGroup {
    NumFmt,
    Font,
    Fill,
    Border,
    Alignment,
    Protection,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Field {
    Content,
    Style(StyleGroup),
}

/// What an op writes. Row and column ops have no fields: they target no
/// cell, so R2 never restores them; their keys are per property (§4.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Key {
    Cell {
        sheet: SheetId,
        row: RowId,
        col: ColId,
        field: Field,
    },
    Row {
        sheet: SheetId,
        id: RowId,
        prop: RowProp,
    },
    Col {
        sheet: SheetId,
        id: ColId,
        prop: ColProp,
    },
}

const ALL_STYLE: [Field; 6] = [
    Field::Style(StyleGroup::NumFmt),
    Field::Style(StyleGroup::Font),
    Field::Style(StyleGroup::Fill),
    Field::Style(StyleGroup::Border),
    Field::Style(StyleGroup::Alignment),
    Field::Style(StyleGroup::Protection),
];
const CONTENT_AND_STYLE: [Field; 7] = [
    Field::Content,
    Field::Style(StyleGroup::NumFmt),
    Field::Style(StyleGroup::Font),
    Field::Style(StyleGroup::Fill),
    Field::Style(StyleGroup::Border),
    Field::Style(StyleGroup::Alignment),
    Field::Style(StyleGroup::Protection),
];
const CONTENT: [Field; 1] = [Field::Content];
const DATE: [Field; 2] = [Field::Content, Field::Style(StyleGroup::NumFmt)];
const NUM: [Field; 1] = [Field::Style(StyleGroup::NumFmt)];
const FONT: [Field; 1] = [Field::Style(StyleGroup::Font)];
const FILL: [Field; 1] = [Field::Style(StyleGroup::Fill)];
const BORDER: [Field; 1] = [Field::Style(StyleGroup::Border)];

/// The fields a call writes (§4.3; `*_with_format` and the full style calls
/// write every style group, decision A5).
pub(crate) fn fields(call: &CellCall) -> &'static [Field] {
    use CellCall::*;
    match call {
        Number(_) | String(_) | StringInline(_) | RichString(_) | Bool(_) | Error(_)
        | Formula(_) | RemoveFormula | ClearValue | DynamicFormula(_) => &CONTENT,
        DateTime(_) => &DATE,
        NumberWithFormat(..)
        | StringWithFormat(..)
        | FormulaWithFormat(..)
        | DateTimeWithFormat(..) => &CONTENT_AND_STYLE,
        Format(_) | StyleIndex(_) | NamedStyle(_) => &ALL_STYLE,
        NumFormat(_) => &NUM,
        Font(_) => &FONT,
        Fill(_) => &FILL,
        Border(_) => &BORDER,
    }
}

/// Does the call write the cell's content (a hand-off candidate)?
pub(crate) fn writes_content(call: &CellCall) -> bool {
    fields(call).contains(&Field::Content)
}

/// Applies `input` by its one sequential API call: the shadow's engine and
/// the sequential reference of oracle 1.
pub(crate) fn apply_seq(wb: &mut Workbook, input: &OpInput) -> Result<()> {
    match &input.kind {
        OpKind::WriteCells(cells) => {
            let writes: Vec<(CellIndex, CellWrite<'_>)> = cells
                .iter()
                .map(|(at, w)| {
                    let w = match w {
                        WriteValue::Number(v) => CellWrite::Number(*v),
                        WriteValue::String(s) => CellWrite::String(s),
                        WriteValue::Formula(t) => CellWrite::Formula(t),
                        WriteValue::RemoveFormula => CellWrite::RemoveFormula,
                        WriteValue::ClearValue => CellWrite::ClearValue,
                    };
                    (*at, w)
                })
                .collect();
            wb.worksheet_by_index(input.sheet)?.write_cells(&writes)
        }
        OpKind::Row { row, call } => {
            let mut ws = wb.worksheet_by_index(input.sheet)?;
            match call {
                RowCall::Height(h) => ws.set_row_height(*row, *h),
                RowCall::Hidden(b) => ws.set_row_hidden(*row, *b),
                RowCall::Outline(l) => ws.set_row_outline(*row, *l),
            }
        }
        OpKind::Cols { first, last, call } => {
            let mut ws = wb.worksheet_by_index(input.sheet)?;
            match call {
                ColCall::Width(w) => ws.set_column_width(*first, *last, *w),
                ColCall::Hidden(b) => ws.set_column_hidden(*first, *last, *b),
                ColCall::Outline(l) => ws.set_column_outline(*first, *last, *l),
            }
        }
        OpKind::Cell { at, call } => {
            let mut ws = wb.worksheet_by_index(input.sheet)?;
            let c = ws.cell_mut(at.row, at.col)?;
            match call {
                CellCall::Number(v) => c.set_number(*v),
                CellCall::String(s) => c.set_string(s),
                CellCall::StringInline(s) => c.set_string_inline(s),
                CellCall::RichString(runs) => {
                    let runs: Vec<(&Font, &str)> =
                        runs.iter().map(|(f, t)| (f, t.as_str())).collect();
                    c.set_rich_string(&runs)
                }
                CellCall::Bool(b) => c.set_bool(*b),
                CellCall::Error(e) => c.set_error(e),
                CellCall::DateTime(d) => c.set_datetime(*d),
                CellCall::Formula(t) => c.set_formula(t),
                CellCall::RemoveFormula => c.remove_formula(),
                CellCall::ClearValue => c.clear_value(),
                CellCall::DynamicFormula(t) => c.set_dynamic_formula(t),
                CellCall::NumberWithFormat(v, f) => c.set_number_with_format(*v, f),
                CellCall::StringWithFormat(s, f) => c.set_string_with_format(s, f),
                CellCall::FormulaWithFormat(t, f) => c.set_formula_with_format(t, f),
                CellCall::DateTimeWithFormat(d, f) => c.set_datetime_with_format(*d, f),
                CellCall::Format(f) => c.set_format(f),
                CellCall::NumFormat(code) => c.set_num_format(code),
                CellCall::Font(f) => c.set_font(f),
                CellCall::Fill(f) => c.set_fill(f),
                CellCall::Border(b) => c.set_border(b),
                CellCall::StyleIndex(i) => c.set_style_index(*i),
                CellCall::NamedStyle(n) => c.set_named_style(n),
            }
        }
    }
}
