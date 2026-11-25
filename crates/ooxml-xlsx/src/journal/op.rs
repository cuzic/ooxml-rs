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
    /// P1: part B, refused with `Unsupported`.
    ClearCell {
        at: CellIndex,
    },
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

/// What an op writes (J0b adds row and column properties).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Key {
    Cell {
        sheet: SheetId,
        row: RowId,
        col: ColId,
        field: Field,
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
        | Formula(_) | RemoveFormula => &CONTENT,
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
/// the sequential reference of oracle 1. Never called with `ClearCell`.
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
                    };
                    (*at, w)
                })
                .collect();
            wb.worksheet_by_index(input.sheet)?.write_cells(&writes)
        }
        OpKind::ClearCell { .. } => unreachable!("P1 is refused before it reaches a workbook"),
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
