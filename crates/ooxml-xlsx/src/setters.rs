//! Phase2 setters (ADR-0006 §3, §5.6, §6): values with a format, formats
//! alone, existing styles, booleans and dates. Each goes through the
//! Phase1 edit pipeline (plan → validate → apply, poisoning on a failed
//! apply).

use ooxml_schema::sml;

use crate::edit::{CellOp, Extra};
use crate::error::{Error, Result};
use crate::escape;
use crate::reference::CellIndex;
use crate::registry::dates::ExcelDateTime;
use crate::registry::format::{Format, NumFormat};
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
        self.edit(CellOp::Number(crate::worksheet::format_number(value)), Some(StyleRequest::Format(format.clone())))
    }

    /// Writes a string with a format; see [`Self::set_string`] and
    /// [`Self::set_number_with_format`].
    pub fn set_string_with_format(self, value: &str, format: &Format) -> Result<()> {
        let ws = &mut *self.ws;
        ws.wb.write_string(ws.sheet, self.at, value, Some(StyleRequest::Format(format.clone())))
    }

    /// Writes a formula with a format; see [`Self::set_formula`].
    pub fn set_formula_with_format(self, text: &str, format: &Format) -> Result<()> {
        let text = text.strip_prefix('=').unwrap_or(text);
        self.edit(CellOp::Formula(text.to_owned()), Some(StyleRequest::Format(format.clone())))
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
        self.edit(CellOp::StyleOnly, Some(StyleRequest::Format(format.clone())))
    }

    /// Changes only the number format, keeping every other part of the
    /// cell's current style (font, fill, border, alignment, protection and
    /// the named style), and keeps the value.
    pub fn set_num_format(self, code: &str) -> Result<()> {
        self.edit(CellOp::StyleOnly, Some(StyleRequest::NumFormatOnly(NumFormat::Code(code.to_owned()))))
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
        self.edit(CellOp::Number(crate::worksheet::format_number(serial)), Some(StyleRequest::Format(format.clone())))
    }
}
