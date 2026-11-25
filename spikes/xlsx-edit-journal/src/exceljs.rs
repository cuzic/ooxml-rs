//! A thin ExcelJS 4.4.0-shaped API over the Journal: `getCell('B2').value`
//! get/set and `.font`, `.fill`, `.numFmt`, `.style` get/set. Rust types
//! that mirror ExcelJS's shapes; no Wasm.

use ooxml_xlsx::{CellIndex, CellValue, Color, ExcelDateTime, Fill, Font, FontSize, Format, FormatRead, NumFormat};

use crate::journal::{JError, JResult, Journal, NormCell};
use crate::op::{Api, CellKey, Content, EchoField, Op, Patch, RawInput};

/// ExcelJS `cell.value`.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Number(f64),
    String(String),
    Bool(bool),
    Date(ExcelDateTime),
    /// `{ formula, result }`; `formula` without `=`, as ExcelJS reads it.
    Formula { formula: String, result: Option<Box<Value>> },
    /// `{ error: '#N/A' }`.
    Error(String),
}

/// ExcelJS `cell.font` (the subset the spike maps).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EjFont {
    pub name: Option<String>,
    pub size: Option<f64>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    /// `color: { argb }`.
    pub argb: Option<u32>,
}

/// ExcelJS `cell.fill` of `type: 'pattern'`.
#[derive(Clone, Debug, PartialEq)]
pub struct EjFill {
    pub pattern: String,
    pub fg_argb: Option<u32>,
}

/// ExcelJS `cell.style` (subset).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EjStyle {
    pub num_fmt: Option<String>,
    pub font: Option<EjFont>,
    pub fill: Option<EjFill>,
}

pub struct EjWorkbook {
    pub journal: Journal,
}

impl EjWorkbook {
    pub fn load(bytes: impl Into<std::sync::Arc<[u8]>>) -> JResult<Self> {
        Ok(EjWorkbook { journal: Journal::open(bytes)? })
    }
    /// `workbook.getWorksheet(name)`.
    pub fn get_worksheet(&mut self, name: &str) -> JResult<EjWorksheet<'_>> {
        let sheet = self.journal.shadow()?.sheet_names().position(|n| n == name).ok_or_else(|| JError::Xlsx(ooxml_xlsx::Error::NoSuchSheet(name.into())))?;
        Ok(EjWorksheet { j: &mut self.journal, sheet })
    }
    /// `workbook.xlsx.writeBuffer()`.
    pub fn write_buffer(&mut self) -> JResult<Vec<u8>> {
        Ok(self.journal.commit()?.0)
    }
}

pub struct EjWorksheet<'a> {
    j: &'a mut Journal,
    sheet: usize,
}

impl EjWorksheet<'_> {
    /// `worksheet.getCell('B2')`.
    pub fn get_cell(&mut self, a1: &str) -> JResult<EjCell<'_>> {
        let at = CellIndex::parse(a1)?;
        Ok(EjCell { j: self.j, key: CellKey::new(self.sheet, at.row, at.col) })
    }
}

pub struct EjCell<'a> {
    j: &'a mut Journal,
    key: CellKey,
}

impl EjCell<'_> {
    /// `cell.value` (getter): echo of a session write, else the normalized
    /// read in ExcelJS's projection.
    pub fn value(&mut self) -> JResult<Value> {
        if let Some(RawInput::Value(v)) = self.j.echo(self.key, EchoField::Value) {
            return Ok(v.clone());
        }
        Ok(project_value(self.j.normalized(self.key)?.as_ref()))
    }

    /// `cell.value = v`.
    pub fn set_value(&mut self, v: Value) -> JResult<()> {
        let k = self.key;
        let op = match &v {
            Value::Null => Op::Clear(k),
            Value::Number(n) => Op::Content(k, Content::Number(*n)),
            Value::String(s) => Op::Content(k, Content::String(s.clone())),
            Value::Bool(b) => Op::Content(k, Content::Bool(*b)),
            Value::Error(e) => Op::Content(k, Content::Error(e.clone())),
            // Q1 (A): a supplied result is not kept
            Value::Formula { formula, .. } => Op::Content(k, Content::Formula(formula.clone())),
            Value::Date(d) => Op::DateTime(k, *d),
        };
        self.j.append(Api::ExcelJs, op, Some(RawInput::Value(v))).map(|_| ())
    }

    pub fn font(&mut self) -> JResult<Option<EjFont>> {
        if let Some(r) = self.j.echo(self.key, EchoField::Font) {
            match r {
                RawInput::Font(f) => return Ok(Some(f.clone())),
                RawInput::Style(s) => return Ok(s.font.clone()),
                _ => {}
            }
        }
        Ok(self.norm_format()?.and_then(|f| f.font).map(|f| font_out(&f)))
    }
    pub fn set_font(&mut self, f: EjFont) -> JResult<()> {
        self.j.append(Api::ExcelJs, Op::Style(self.key, Patch::Font(font_in(&f))), Some(RawInput::Font(f))).map(|_| ())
    }
    pub fn fill(&mut self) -> JResult<Option<EjFill>> {
        if let Some(r) = self.j.echo(self.key, EchoField::Fill) {
            match r {
                RawInput::Fill(f) => return Ok(Some(f.clone())),
                RawInput::Style(s) => return Ok(s.fill.clone()),
                _ => {}
            }
        }
        Ok(self.norm_format()?.and_then(|f| f.fill).and_then(|f| fill_out(&f)))
    }
    pub fn set_fill(&mut self, f: EjFill) -> JResult<()> {
        self.j.append(Api::ExcelJs, Op::Style(self.key, Patch::Fill(fill_in(&f))), Some(RawInput::Fill(f))).map(|_| ())
    }
    pub fn num_fmt(&mut self) -> JResult<Option<String>> {
        if let Some(r) = self.j.echo(self.key, EchoField::NumFmt) {
            match r {
                RawInput::NumFmt(s) => return Ok(Some(s.clone())),
                RawInput::Style(s) => return Ok(s.num_fmt.clone()),
                _ => {}
            }
        }
        Ok(self.norm_format()?.and_then(|f| f.num_format).and_then(|n| numfmt_out(&n)))
    }
    pub fn set_num_fmt(&mut self, code: &str) -> JResult<()> {
        self.j.append(Api::ExcelJs, Op::Style(self.key, Patch::NumFmt(code.into())), Some(RawInput::NumFmt(code.into()))).map(|_| ())
    }
    /// `cell.style = { … }`: a full replacement (unset groups default).
    pub fn set_style(&mut self, s: EjStyle) -> JResult<()> {
        let f = Format {
            num_format: s.num_fmt.as_ref().map(|c| NumFormat::Code(c.clone())),
            font: s.font.as_ref().map(font_in),
            fill: s.fill.as_ref().map(fill_in),
            ..Format::default()
        };
        self.j.append(Api::ExcelJs, Op::Style(self.key, Patch::Format(Box::new(f))), Some(RawInput::Style(s))).map(|_| ())
    }
    pub fn style(&mut self) -> JResult<EjStyle> {
        Ok(EjStyle { num_fmt: self.num_fmt()?, font: self.font()?, fill: self.fill()? })
    }

    fn norm_format(&mut self) -> JResult<Option<Format>> {
        Ok(match self.j.normalized(self.key)? {
            Some(NormCell { format: FormatRead::Expressible(f), .. }) => Some(*f),
            _ => None,
        })
    }
}

/// ExcelJS's projection of a normalized cell (as its reader would load it).
pub fn project_value(c: Option<&NormCell>) -> Value {
    let Some(c) = c else { return Value::Null };
    let plain = |v: &CellValue| match v {
        CellValue::Empty => None,
        CellValue::Number(n) if c.is_date => ExcelDateTime::from_serial(*n, false).map(Value::Date),
        CellValue::Number(n) => Some(Value::Number(*n)),
        CellValue::String(s) => Some(Value::String(s.clone())),
        CellValue::Bool(b) => Some(Value::Bool(*b)),
        CellValue::Error(e) => Some(Value::Error(e.clone())),
        CellValue::Date(s) => Some(Value::String(s.clone())),
    };
    match &c.formula {
        Some(f) => Value::Formula { formula: f.clone(), result: plain(&c.value).map(Box::new) },
        None => plain(&c.value).unwrap_or(Value::Null),
    }
}

/// The adapter's `normalize(echo(x))` for the Reload law: what ExcelJS
/// reads back after a save (Q1 (A): a formula's result is not kept).
pub fn normalize_echo(v: &Value) -> Value {
    match v {
        Value::Formula { formula, .. } => Value::Formula { formula: formula.strip_prefix('=').unwrap_or(formula).to_owned(), result: None },
        v => v.clone(),
    }
}

pub fn font_in(f: &EjFont) -> Font {
    let mut out = Font::default();
    if let Some(n) = &f.name {
        out.name = n.clone();
    }
    if let Some(s) = f.size.and_then(FontSize::new) {
        out.size = s;
    }
    out.bold = f.bold.unwrap_or(false);
    out.italic = f.italic.unwrap_or(false);
    if let Some(c) = f.argb {
        out.color = Some(Color::Rgb(c));
    }
    out
}

pub fn font_out(f: &Font) -> EjFont {
    EjFont {
        name: Some(f.name.clone()),
        size: Some(f.size.get()),
        bold: f.bold.then_some(true),
        italic: f.italic.then_some(true),
        argb: match f.color {
            Some(Color::Rgb(c)) => Some(c),
            _ => None,
        },
    }
}

/// ExcelJS's reload of a font it was given: the defaults filled in.
pub fn normalize_font(f: &EjFont) -> EjFont {
    font_out(&font_in(f))
}

pub fn fill_in(f: &EjFill) -> Fill {
    Fill::Pattern { pattern: f.pattern.clone(), fg: f.fg_argb.map(Color::Rgb), bg: None }
}

pub fn fill_out(f: &Fill) -> Option<EjFill> {
    match f {
        Fill::Pattern { pattern, fg, .. } => Some(EjFill { pattern: pattern.clone(), fg_argb: match fg { Some(Color::Rgb(c)) => Some(*c), _ => None } }),
        Fill::NoFill => None,
    }
}

pub fn numfmt_out(n: &NumFormat) -> Option<String> {
    match n {
        NumFormat::General => None,
        NumFormat::Code(c) => Some(c.clone()),
        NumFormat::Builtin(14) | NumFormat::DefaultDate => Some("mm-dd-yy".into()),
        NumFormat::Builtin(i) => Some(format!("builtin:{i}")),
    }
}
