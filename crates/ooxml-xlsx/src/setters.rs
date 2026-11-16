//! Phase2 setters (ADR-0006 §3, §5.6, §6): values with a format, formats
//! alone, existing styles, booleans and dates. Each goes through the
//! Phase1 edit pipeline (plan → validate → apply, poisoning on a failed
//! apply).

use ooxml_core::ElementSpec;
use ooxml_schema::sml;

use crate::edit::{CellOp, Extra};
use crate::error::{Error, Result};
use crate::escape;
use crate::reference::CellIndex;
use crate::registry::dates::ExcelDateTime;
use crate::registry::format::{Border, Fill, Font, Format, NumFormat};
use crate::registry::styles::StyleRequest;
use crate::workbook::Workbook;
use crate::worksheet::{CellMut, MAX_STRING_LEN};

impl Workbook {
    /// A string write: shared when the workbook has a shared-string table,
    /// else inline.
    pub(crate) fn write_string(&mut self, sheet: usize, at: CellIndex, value: &str, style: Option<StyleRequest>) -> Result<()> {
        let units = value.encode_utf16().count();
        if units > MAX_STRING_LEN {
            return Err(Error::StringTooLong(units));
        }
        let escaped = escape::encode(value).into_owned();
        if let Some((offset, ch)) = ooxml_core::value::first_invalid_xml_char(&escaped) {
            return Err(ooxml_core::Error::InvalidXmlChar { ch, offset }.into());
        }
        if self.shared_strings_active()? {
            let plan = self.registries.strings.as_ref().expect("active").plan(&escaped, value)?;
            let index = plan.index;
            self.edit_cell_ext(sheet, at, CellOp::Shared(index), Extra { string: Some(plan), style })
        } else {
            self.edit_cell_ext(sheet, at, CellOp::Text(escaped), Extra { string: None, style })
        }
    }

    /// Is the workbook in the 1904 date system (`workbookPr/@date1904`)?
    pub fn date1904(&mut self) -> Result<bool> {
        let doc = self.pkg.xml_part(&self.workbook_part)?;
        let wb = sml::WorkbookView(doc.lens::<sml::Workbook>()?);
        let Some(pr) = wb.workbook_properties() else { return Ok(false) };
        match pr.0.raw().attr(sml::WorkbookProperties::ATTR_DATE1904).map(str::trim) {
            None | Some("0" | "false") => Ok(false),
            Some("1" | "true") => Ok(true),
            Some(v) => Err(Error::Malformed(format!("workbookPr/@date1904 = {v:?}"))),
        }
    }

    /// Number of entries in `cellXfs` (activates the style registry).
    pub fn cell_format_count(&mut self) -> Result<u32> {
        Ok(self.styles()?.xf_count())
    }
}

impl CellMut<'_, '_> {
    fn edit(self, op: CellOp, style: Option<StyleRequest>) -> Result<()> {
        let ws = &mut *self.ws;
        ws.wb.edit_cell_ext(ws.sheet, self.at, op, Extra { string: None, style })
    }

    /// Writes a number with a format (ADR-0006 §3): unset groups of
    /// `format` inherit from the workbook's default style; set groups are
    /// reused byte-identically or appended.
    ///
    /// Errors (nothing is changed): those of [`Self::set_number`], plus
    /// [`Error::NoStylesPart`], [`Error::StyleTableFull`],
    /// [`Error::NumFmtIdExhausted`], [`Error::BuiltinNumFormatRedefined`],
    /// core `UnresolvedSibling` for a style table with an
    /// `mc:AlternateContent` child.
    pub fn set_number_with_format(self, value: f64, format: &Format) -> Result<()> {
        if !value.is_finite() {
            return Err(Error::InvalidNumber(value));
        }
        self.edit(CellOp::Number(crate::worksheet::format_number(value)), Some(StyleRequest::Format(Box::new(format.clone()))))
    }

    /// Writes a string with a format; see [`Self::set_string`] and
    /// [`Self::set_number_with_format`].
    pub fn set_string_with_format(self, value: &str, format: &Format) -> Result<()> {
        let ws = &mut *self.ws;
        ws.wb.write_string(ws.sheet, self.at, value, Some(StyleRequest::Format(Box::new(format.clone()))))
    }

    /// Writes a formula with a format; see [`Self::set_formula`].
    pub fn set_formula_with_format(self, text: &str, format: &Format) -> Result<()> {
        let text = text.strip_prefix('=').unwrap_or(text);
        self.edit(CellOp::Formula(text.to_owned()), Some(StyleRequest::Format(Box::new(format.clone()))))
    }

    /// Writes an error value (`t="e"`): one of `#NULL!`, `#DIV/0!`,
    /// `#VALUE!`, `#REF!`, `#NAME?`, `#NUM!`, `#N/A`, `#GETTING_DATA`
    /// (ECMA-376 §18.17.4.6 plus Excel's `#GETTING_DATA`), else
    /// [`Error::InvalidValue`]. A value write, as [`Self::set_number`].
    pub fn set_error(self, value: &str) -> Result<()> {
        const ERRORS: &[&str] = &["#NULL!", "#DIV/0!", "#VALUE!", "#REF!", "#NAME?", "#NUM!", "#N/A", "#GETTING_DATA"];
        let Some(e) = ERRORS.iter().copied().find(|e| *e == value) else {
            return Err(Error::InvalidValue { cell: self.at.to_string(), message: format!("{value:?} is not an error value") });
        };
        self.edit(CellOp::Error(e), None)
    }

    /// Writes a boolean (`t="b"`).
    pub fn set_bool(self, value: bool) -> Result<()> {
        self.edit(CellOp::Bool(value), None)
    }

    /// Sets the cell's format and keeps its value. Unset groups of `format`
    /// inherit from the default style (the cell's own font, fill, … are
    /// **not** kept; use [`Self::set_num_format`] to change only the number
    /// format). A workbook whose formulas may read formats (`CELL`, `INFO`,
    /// `GET.CELL`, or functions outside Phase1's allowlist) treats this as
    /// a value write: dependents lose their cached values (ADR-0006 §7).
    pub fn set_format(self, format: &Format) -> Result<()> {
        self.edit(CellOp::StyleOnly, Some(StyleRequest::Format(Box::new(format.clone()))))
    }

    /// Changes only the number format, keeping every other part of the
    /// cell's current style (font, fill, border, alignment, protection and
    /// the named style), and keeps the value (ADR-0006 §5.6). The new `xf`
    /// copies the current one's attributes and children and replaces only
    /// `numFmtId` and `applyNumberFormat`. Errors: [`Error::NotExpressible`]
    /// when the current `xf` has extension attributes or children (they
    /// could not be carried over) or is an `mc:AlternateContent` entry;
    /// [`Error::DanglingReference`] when the cell's `s` is out of range;
    /// those of [`Self::set_number_with_format`].
    pub fn set_num_format(self, code: &str) -> Result<()> {
        self.edit(CellOp::StyleOnly, Some(StyleRequest::NumFormatOnly(NumFormat::Code(code.to_owned()))))
    }

    /// Changes only the font, keeping every other part of the cell's
    /// current style and the value (ADR-0006 §5.6). Errors as for
    /// [`Self::set_num_format`].
    pub fn set_font(self, font: &Font) -> Result<()> {
        self.edit(CellOp::StyleOnly, Some(StyleRequest::FontOnly(Box::new(font.clone()))))
    }

    /// Changes only the fill; see [`Self::set_font`].
    pub fn set_fill(self, fill: &Fill) -> Result<()> {
        self.edit(CellOp::StyleOnly, Some(StyleRequest::FillOnly(fill.clone())))
    }

    /// Changes only the borders; see [`Self::set_font`].
    pub fn set_border(self, border: &Border) -> Result<()> {
        self.edit(CellOp::StyleOnly, Some(StyleRequest::BorderOnly(Box::new(border.clone()))))
    }

    /// Applies an existing `cellXfs` entry by index ([`Error::StyleIndexOutOfRange`]).
    pub fn set_style_index(self, index: u32) -> Result<()> {
        self.edit(CellOp::StyleOnly, Some(StyleRequest::Index(index)))
    }

    /// Applies a named cell style (`cellStyles`, compared like Excel,
    /// case-insensitively). Groups the style does not apply
    /// (`apply*="0"`) keep the cell's current values. Errors:
    /// [`Error::NoSuchStyle`], [`Error::AmbiguousStyleName`].
    pub fn set_named_style(self, name: &str) -> Result<()> {
        self.edit(CellOp::StyleOnly, Some(StyleRequest::Named(name.to_owned())))
    }

    /// Writes a date/time as its serial in the workbook's date system
    /// (1900 with Excel's false 1900-02-29, or 1904). If the cell's
    /// current number format is not a date or time format, it becomes
    /// built-in 14 (`mm-dd-yy`, shown in the user's short-date form); every
    /// other part of the style is kept. Errors: [`Error::DateOutOfRange`]
    /// plus those of [`Self::set_number`].
    pub fn set_datetime(self, value: ExcelDateTime) -> Result<()> {
        let ws = &mut *self.ws;
        let serial = value.serial(ws.wb.date1904()?).ok_or(Error::DateOutOfRange)?;
        self.edit(CellOp::Number(crate::worksheet::format_number(serial)), Some(StyleRequest::EnsureDate))
    }

    /// Writes a date/time with a format (whose number format should be a
    /// date or time format).
    pub fn set_datetime_with_format(self, value: ExcelDateTime, format: &Format) -> Result<()> {
        let ws = &mut *self.ws;
        let serial = value.serial(ws.wb.date1904()?).ok_or(Error::DateOutOfRange)?;
        self.edit(CellOp::Number(crate::worksheet::format_number(serial)), Some(StyleRequest::Format(Box::new(format.clone()))))
    }
}

impl Workbook {
    /// Does the workbook have a shared-string table? When it has none,
    /// `set_string` writes inline strings (ADR-0006 §4.6).
    pub fn shared_strings_available(&mut self) -> Result<bool> {
        self.shared_strings_active()
    }

    /// The workbook's default font (that of the Normal style,
    /// `cellStyleXfs[0]`, else `cellXfs[0]`) as a [`Font`] value, or
    /// `None` when it has anything a `Font` cannot express. See
    /// [`Format::with_default_font_of`].
    pub fn default_font(&mut self) -> Result<Option<Font>> {
        self.styles()?;
        let st = self.registries.styles.as_ref().expect("active");
        let Some(font_id) = st.default.as_ref().and_then(|d| d.get("fontId")).and_then(|v| v.trim().parse::<usize>().ok()) else {
            return Ok(None);
        };
        let part = st.part.clone();
        let doc = self.pkg.xml_part(&part)?;
        let Some(fonts) = doc.root().elements().find(|e| e.is(sml::Fonts::QNAME)) else { return Ok(None) };
        let kids: Vec<_> = fonts.elements().collect();
        if kids.iter().any(|k| !k.is(sml::Font::QNAME)) {
            return Ok(None);
        }
        Ok(kids.get(font_id).and_then(|f| font_from_node(f)))
    }
}

fn font_from_node(n: &ooxml_core::RawNode) -> Option<Font> {
    use crate::registry::format::{Color, Tint, Underline};
    if !n.attrs().is_empty() {
        return None;
    }
    let mut f = Font { color: None, family: None, scheme: None, name: String::new(), ..Font::default() };
    let val = |e: &ooxml_core::RawNode| e.attr(ooxml_core::QName::new("", "val")).map(str::to_owned);
    let only = |e: &ooxml_core::RawNode, allowed: &[&str]| e.elements().next().is_none() && e.attrs().iter().all(|a| a.name().ns().is_empty() && allowed.contains(&a.name().local()));
    for e in n.elements() {
        if e.name().ns() != crate::SML_NS {
            return None;
        }
        match e.name().local() {
            "b" | "i" | "strike" if only(e, &["val"]) => {
                let on = !matches!(val(e).as_deref(), Some("0" | "false"));
                match e.name().local() {
                    "b" => f.bold = on,
                    "i" => f.italic = on,
                    _ => f.strike = on,
                }
            }
            "u" if only(e, &["val"]) => {
                f.underline = match val(e).as_deref() {
                    None | Some("single") => Some(Underline::Single),
                    Some("double") => Some(Underline::Double),
                    Some("singleAccounting") => Some(Underline::SingleAccounting),
                    Some("doubleAccounting") => Some(Underline::DoubleAccounting),
                    Some("none") => None,
                    Some(_) => return None,
                }
            }
            "sz" if only(e, &["val"]) => f.size = crate::registry::format::FontSize::new(val(e)?.trim().parse().ok()?)?,
            "name" if only(e, &["val"]) => f.name = val(e)?,
            "family" if only(e, &["val"]) => f.family = Some(val(e)?.trim().parse().ok()?),
            "scheme" if only(e, &["val"]) => f.scheme = Some(val(e)?),
            "color" if e.elements().next().is_none() => {
                let a: Vec<(String, String)> = e.attrs().iter().map(|a| (a.name().display().to_string(), a.value().to_owned())).collect();
                let get = |k: &str| a.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
                let c = match (get("rgb"), get("theme"), get("indexed"), get("auto")) {
                    (Some(rgb), None, None, None) if a.len() == 1 => Color::Rgb(u32::from_str_radix(rgb, 16).ok()?),
                    (None, Some(t), None, None) => {
                        let tint = match get("tint") {
                            Some(t) => Tint::new(t.parse().ok()?)?,
                            None => Tint::NONE,
                        };
                        if a.len() != 1 + usize::from(get("tint").is_some()) {
                            return None;
                        }
                        Color::Theme { index: t.parse().ok()?, tint }
                    }
                    (None, None, Some(i), None) if a.len() == 1 => Color::Indexed(i.parse().ok()?),
                    (None, None, None, Some("1" | "true")) if a.len() == 1 => Color::Auto,
                    _ => return None,
                };
                f.color = Some(c);
            }
            _ => return None,
        }
    }
    let has_size = n.elements().any(|e| e.name().local() == "sz");
    (!f.name.is_empty() && has_size).then_some(f)
}

impl Format {
    /// A format whose font is the workbook's default font when a [`Font`]
    /// can express it (else rust_xlsxwriter's Calibri 11), so `set_bold()`
    /// keeps the workbook's face and size (ADR-0006 §3, review n5).
    pub fn with_default_font_of(wb: &mut Workbook) -> Result<Format> {
        Ok(match wb.default_font()? {
            Some(f) => Format::with_font(f),
            None => Format::new(),
        })
    }
}

impl crate::worksheet::Worksheet<'_> {
    /// Is the cell's number format a date or time format (calamine's rule:
    /// built-in ids 14–22, 27–36, 45, 47, 50–58, or a custom code with
    /// date/time tokens)? `false` for a missing cell.
    pub fn is_date(&mut self, row: u32, col: u32) -> Result<bool> {
        let Some(s) = self.cell(row, col)?.map(|c| c.style_index()).transpose()? else { return Ok(false) };
        let st = self.wb.styles()?;
        let (id, code) = st.num_format_of(s);
        let custom = id.filter(|i| st.codes.contains_key(i)).and(code);
        Ok(crate::registry::format::is_date_format(id, custom.as_deref()))
    }

    /// The cell's number as a date in the workbook's date system, if the
    /// cell holds a number and [`Self::is_date`]. Serial 60 in the 1900
    /// system is [`Error::NonexistentDate`].
    pub fn datetime(&mut self, row: u32, col: u32) -> Result<Option<ExcelDateTime>> {
        if !self.is_date(row, col)? {
            return Ok(None);
        }
        let Some(crate::worksheet::CellValue::Number(n)) = self.cell(row, col)?.map(|c| c.value()).transpose()? else { return Ok(None) };
        let d1904 = self.wb.date1904()?;
        if !d1904 && n.floor() == 60.0 {
            return Err(Error::NonexistentDate);
        }
        ExcelDateTime::from_serial(n, d1904).map(Some).ok_or(Error::DateOutOfRange)
    }
}
