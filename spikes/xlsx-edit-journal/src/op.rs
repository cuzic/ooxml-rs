//! Journal ops (ADR-0024 §4.1 part A, keyed rows only), their origins and
//! the one place that applies an op through the sequential API.

use ooxml_xlsx::{Border, ExcelDateTime, Fill, Font, Format, Workbook};

use crate::exceljs;

/// A cell, by Base ids. In this keyed-only slice there are no structural
/// ops, so the id vectors of §5.2 are the identity: `RowId::Base(r)` is row
/// `r` (0-based here) for the whole session, and `SheetId::Base` is the
/// sheet's position.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CellKey {
    pub sheet: usize,
    pub row: u32,
    pub col: u32,
}

impl CellKey {
    pub fn new(sheet: usize, row: u32, col: u32) -> Self {
        CellKey { sheet, row, col }
    }
}

/// The content half of a keyed write.
#[derive(Clone, Debug, PartialEq)]
pub enum Content {
    Number(f64),
    String(String),
    Bool(bool),
    Error(String),
    Formula(String),
    /// Conditional (N1): a cell without a formula is left alone.
    RemoveFormula,
}

impl Content {
    pub fn is_conditional(&self) -> bool {
        matches!(self, Content::RemoveFormula)
    }
}

/// A style patch (`StyleRequest`, ADR-0024 §4.4 N1s).
#[derive(Clone, Debug, PartialEq)]
pub enum Patch {
    /// Full: unset groups from the default style.
    Format(Box<Format>),
    /// Full: a Base `xf` (an index from this session is P2, `Unsupported`).
    Index(u32),
    /// Partial.
    NumFmt(String),
    Font(Font),
    Fill(Fill),
    Border(Border),
}

impl Patch {
    pub fn is_full(&self) -> bool {
        matches!(self, Patch::Format(_) | Patch::Index(_))
    }
}

#[derive(Clone, Debug, PartialEq)]
#[expect(clippy::large_enum_variant, reason = "spike: ops are few and short-lived")]
pub enum Op {
    Content(CellKey, Content),
    Style(CellKey, Patch),
    /// Two-half: `set_datetime` (content + `EnsureDate`).
    DateTime(CellKey, ExcelDateTime),
    /// Two-half: `set_number_with_format`.
    NumberWithFormat(CellKey, f64, Format),
    /// P1 (ExcelJS `cell.value = null`): part B, refused with `Unsupported`.
    Clear(CellKey),
    /// `write_cells`: one sequential call over several cells of one sheet
    /// (ADR-0024 §4.1: one op per cell in the key index, one call on the
    /// shadow; R1 skips it only as a whole). Shadow mode only.
    WriteCells(Vec<(CellKey, Content)>),
    /// A structural op (ADR-0018 rows/columns). Shadow mode only.
    Structural(Structural),
}

/// A structural op, in the positions current when it is appended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Structural {
    InsertRows { sheet: usize, at: u32, count: u32 },
    DeleteRows { sheet: usize, at: u32, count: u32 },
    InsertCols { sheet: usize, at: u32, count: u32 },
    DeleteCols { sheet: usize, at: u32, count: u32 },
}

impl Structural {
    pub fn sheet(self) -> usize {
        match self {
            Structural::InsertRows { sheet, .. } | Structural::DeleteRows { sheet, .. } | Structural::InsertCols { sheet, .. } | Structural::DeleteCols { sheet, .. } => sheet,
        }
    }

    /// Where a cell at `k` (before the op) is after it; `None` if deleted.
    pub fn map(self, k: CellKey) -> Option<CellKey> {
        if k.sheet != self.sheet() {
            return Some(k);
        }
        let shift = |p: u32, at: u32, count: u32, insert: bool| -> Option<u32> {
            if p < at {
                Some(p)
            } else if insert {
                Some(p + count)
            } else if p < at + count {
                None
            } else {
                Some(p - count)
            }
        };
        match self {
            Structural::InsertRows { at, count, .. } => shift(k.row, at, count, true).map(|row| CellKey { row, ..k }),
            Structural::DeleteRows { at, count, .. } => shift(k.row, at, count, false).map(|row| CellKey { row, ..k }),
            Structural::InsertCols { at, count, .. } => shift(k.col, at, count, true).map(|col| CellKey { col, ..k }),
            Structural::DeleteCols { at, count, .. } => shift(k.col, at, count, false).map(|col| CellKey { col, ..k }),
        }
    }

    /// The inverse: where a cell at `k` (after the op) was before it;
    /// `None` for a cell of an inserted band.
    pub fn unmap(self, k: CellKey) -> Option<CellKey> {
        if k.sheet != self.sheet() {
            return Some(k);
        }
        let back = |p: u32, at: u32, count: u32, insert: bool| -> Option<u32> {
            if p < at {
                Some(p)
            } else if insert {
                if p < at + count { None } else { Some(p - count) }
            } else {
                Some(p + count)
            }
        };
        match self {
            Structural::InsertRows { at, count, .. } => back(k.row, at, count, true).map(|row| CellKey { row, ..k }),
            Structural::DeleteRows { at, count, .. } => back(k.row, at, count, false).map(|row| CellKey { row, ..k }),
            Structural::InsertCols { at, count, .. } => back(k.col, at, count, true).map(|col| CellKey { col, ..k }),
            Structural::DeleteCols { at, count, .. } => back(k.col, at, count, false).map(|col| CellKey { col, ..k }),
        }
    }

    pub fn apply(self, wb: &mut Workbook) -> ooxml_xlsx::Result<()> {
        let mut ws = wb.worksheet_by_index(self.sheet())?;
        match self {
            Structural::InsertRows { at, count, .. } => ws.insert_rows(at, count),
            Structural::DeleteRows { at, count, .. } => ws.delete_rows(at, count),
            Structural::InsertCols { at, count, .. } => ws.insert_cols(at, count),
            Structural::DeleteCols { at, count, .. } => ws.delete_cols(at, count),
        }
        .map(drop)
    }
}

impl Op {
    /// The op's one cell. A `write_cells` call answers its first cell, a
    /// structural op a dummy key: callers that index by key use `keys()`.
    pub fn key(&self) -> CellKey {
        match self {
            Op::Content(k, _) | Op::Style(k, _) | Op::DateTime(k, _) | Op::NumberWithFormat(k, ..) | Op::Clear(k) => *k,
            Op::WriteCells(w) => w[0].0,
            Op::Structural(s) => CellKey::new(s.sheet(), u32::MAX, u32::MAX),
        }
    }
    /// The cells the op names (not a hand-off's new master: see `targets`).
    pub fn keys(&self) -> Vec<CellKey> {
        match self {
            Op::WriteCells(w) => w.iter().map(|(k, _)| *k).collect(),
            Op::Structural(_) => Vec::new(),
            _ => vec![self.key()],
        }
    }
    pub fn is_structural(&self) -> bool {
        matches!(self, Op::Structural(_))
    }
    pub fn is_two_half(&self) -> bool {
        matches!(self, Op::DateTime(..) | Op::NumberWithFormat(..))
    }
    /// The echo fields this op sets (§6.1 "written again", per field).
    pub fn echo_fields(&self) -> &'static [EchoField] {
        use EchoField::*;
        match self {
            Op::Content(..) | Op::Clear(_) => &[Value],
            Op::Style(_, Patch::Format(_) | Patch::Index(_)) => &[Font, Fill, NumFmt, Border],
            Op::Style(_, Patch::NumFmt(_)) => &[NumFmt],
            Op::Style(_, Patch::Font(_)) => &[Font],
            Op::Style(_, Patch::Fill(_)) => &[Fill],
            Op::Style(_, Patch::Border(_)) => &[Border],
            // `EnsureDate` may change the number format
            Op::DateTime(..) => &[Value, NumFmt],
            Op::NumberWithFormat(..) => &[Value, Font, Fill, NumFmt, Border],
            Op::WriteCells(_) => &[Value],
            Op::Structural(_) => &[],
        }
    }
}

/// What an echo read is keyed on. ADR-0024's key index has one style key per
/// cell; ExcelJS echoes per style group (see README "ADR feedback").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EchoField {
    Value,
    Font,
    Fill,
    NumFmt,
    Border,
}

/// The key's state on the shadow just before an op (§4.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Start {
    Formula,
    Missing,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Api {
    Native,
    ExcelJs,
}

/// The caller's input exactly as given. ADR-0024 §4.2 stores it in the
/// adapter keyed by `seq`; the spike keeps it in the origin, as asked.
#[derive(Clone, Debug, PartialEq)]
pub enum RawInput {
    Value(exceljs::Value),
    Font(exceljs::EjFont),
    Fill(exceljs::EjFill),
    NumFmt(String),
    Style(exceljs::EjStyle),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Origin {
    /// Never stored: the absence of an op means Base.
    Base,
    Session { seq: u64, api: Api, raw_input: Option<RawInput> },
}

#[derive(Clone, Debug)]
pub struct LoggedOp {
    pub seq: u64,
    pub origin: Origin,
    pub start: Start,
    pub op: Op,
}

/// The cell's start state on `wb`.
pub fn start_of(wb: &mut Workbook, k: CellKey) -> ooxml_xlsx::Result<Start> {
    let ws = wb.worksheet_by_index(k.sheet)?;
    Ok(match ws.cell(k.row, k.col)? {
        None => Start::Missing,
        Some(c) if c.formula().is_some() => Start::Formula,
        Some(_) => Start::Other,
    })
}

/// Applies `op` by its one sequential API call: the shadow's engine, the
/// sequential reference of oracle 1, and Commit's path for every entry that
/// `write_cells` cannot carry.
pub fn apply_seq(wb: &mut Workbook, op: &Op) -> ooxml_xlsx::Result<()> {
    match op {
        Op::WriteCells(w) => {
            let writes: Vec<(ooxml_xlsx::CellIndex, ooxml_xlsx::CellWrite<'_>)> = w
                .iter()
                .map(|(k, c)| {
                    let at = ooxml_xlsx::CellIndex { row: k.row, col: k.col };
                    let cw = match c {
                        Content::Number(v) => ooxml_xlsx::CellWrite::Number(*v),
                        Content::String(s) => ooxml_xlsx::CellWrite::String(s),
                        Content::Formula(t) => ooxml_xlsx::CellWrite::Formula(t),
                        Content::RemoveFormula => ooxml_xlsx::CellWrite::RemoveFormula,
                        Content::Bool(_) | Content::Error(_) => unreachable!("write_cells carries numbers, strings and formulas"),
                    };
                    (at, cw)
                })
                .collect();
            let mut ws = wb.worksheet_by_index(w[0].0.sheet)?;
            return ws.write_cells(&writes);
        }
        Op::Structural(s) => return s.apply(wb),
        _ => {}
    }
    let k = op.key();
    let mut ws = wb.worksheet_by_index(k.sheet)?;
    let c = ws.cell_mut(k.row, k.col)?;
    match op {
        Op::Content(_, x) => match x {
            Content::Number(v) => c.set_number(*v),
            Content::String(s) => c.set_string(s),
            Content::Bool(b) => c.set_bool(*b),
            Content::Error(e) => c.set_error(e),
            Content::Formula(t) => c.set_formula(t),
            Content::RemoveFormula => c.remove_formula(),
        },
        Op::Style(_, p) => apply_patch(c, p),
        Op::DateTime(_, d) => c.set_datetime(*d),
        Op::NumberWithFormat(_, v, f) => c.set_number_with_format(*v, f),
        Op::Clear(_) => unreachable!("P1 is refused before it reaches a workbook"),
        Op::WriteCells(_) | Op::Structural(_) => unreachable!("handled above"),
    }
}

pub fn apply_patch(c: ooxml_xlsx::CellMut<'_, '_>, p: &Patch) -> ooxml_xlsx::Result<()> {
    match p {
        Patch::Format(f) => c.set_format(f),
        Patch::Index(i) => c.set_style_index(*i),
        Patch::NumFmt(code) => c.set_num_format(code),
        Patch::Font(f) => c.set_font(f),
        Patch::Fill(f) => c.set_fill(f),
        Patch::Border(b) => c.set_border(b),
    }
}
