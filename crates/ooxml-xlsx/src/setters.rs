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
use crate::registry::styles::{BaseStyleCounts, StyleRequest, XfContent};
use crate::workbook::Workbook;
use crate::worksheet::{CellMut, MAX_STRING_LEN};

pub(crate) fn prepare_rich_text(at: CellIndex, runs: &[(Option<&Font>, &str)]) -> Result<(Vec<crate::registry::strings::RichRun>, String)> {
    use crate::registry::strings::RichRun;
    let bad = |message: String| Error::InvalidArgument { cell: Some(at.to_string()), message };
    if runs.is_empty() {
        return Err(bad("a rich string needs at least one run".into()));
    }
    let mut out = Vec::with_capacity(runs.len());
    let mut decoded = String::new();
    let mut units = 0;
    for (i, (font, text)) in runs.iter().enumerate() {
        // Excel refuses empty runs (rust_xlsxwriter does too)
        if text.is_empty() {
            return Err(bad(format!("run {i} is empty")));
        }
        if let Some(font) = font {
            Format::with_font((*font).clone()).validate()?;
        }
        units += text.encode_utf16().count();
        let escaped = escape::encode(text).into_owned();
        if let Some((offset, ch)) = ooxml_core::value::first_invalid_xml_char(&escaped) {
            return Err(ooxml_core::Error::InvalidXmlChar { ch, offset }.into());
        }
        decoded.push_str(text);
        out.push(RichRun { font: font.cloned(), escaped });
    }
    if units > MAX_STRING_LEN {
        return Err(Error::StringTooLong(units));
    }
    Ok((out, decoded))
}

pub(crate) fn project_alignment(node: &ooxml_core::RawNode) -> Vec<(String, String)> {
    ["horizontal", "vertical", "textRotation", "wrapText", "indent", "shrinkToFit", "readingOrder"]
        .into_iter().filter_map(|key| {
            let value = node.attr(ooxml_core::QName::new("", key))?;
            Some((key.into(), value.into()))
        }).collect()
}

pub(crate) fn project_protection(node: &ooxml_core::RawNode) -> crate::Protection {
    let read = |name| node.attr(ooxml_core::QName::new("", name)).and_then(ooxml_core::value::parse_bool);
    crate::Protection { locked: read("locked").unwrap_or(true), hidden: read("hidden").unwrap_or(false) }
}

pub(crate) fn iso_date_op(at: CellIndex, value: &str) -> Result<CellOp> {
    if parse_iso_date(value).is_none() {
        return Err(Error::InvalidArgument { cell: Some(at.to_string()), message: "expected a UTC ISO date".into() });
    }
    Ok(CellOp::Date(value.into()))
}

pub(crate) fn parse_iso_date(value: &str) -> Option<ExcelDateTime> {
    fn digits(text: &str) -> Option<u16> {
        text.bytes().all(|b| b.is_ascii_digit()).then(|| text.parse().expect("bounded digits"))
    }

    if !value.is_ascii() {
        return None;
    }
    let (date, time) = match value.len() {
        12 => (None, value),
        24 if [4, 7, 10, 23].map(|i| value.as_bytes()[i]) == *b"--TZ" =>
            (Some(&value[..10]), &value[11..23]),
        _ => return None,
    };
    if [2, 5, 8].map(|i| time.as_bytes()[i]) != *b"::." {
        return None;
    }
    let hour = digits(&time[..2])? as u8;
    let minute = digits(&time[3..5])? as u8;
    let second = f64::from(digits(&time[6..8])?) + f64::from(digits(&time[9..12])?) / 1000.0;
    if let Some(date) = date {
        let year = digits(&date[..4])?;
        if year == 0 {
            return None;
        }
        ExcelDateTime::from_ymd_hms(year, digits(&date[5..7])? as u8, digits(&date[8..10])? as u8,
            hour, minute, second)
    } else {
        ExcelDateTime::from_hms(hour, minute, second)
    }
}

/// Error codes for [`dynamic::formula_cache`](crate::dynamic::formula_cache); keep in sync with [`error_value`].
pub(crate) const CELL_ERRORS: &[&str] = &["#NULL!", "#DIV/0!", "#VALUE!", "#REF!", "#NAME?", "#NUM!", "#N/A", "#GETTING_DATA"];

fn number_format_code(id: Option<u32>, custom: Option<String>) -> Option<String> {
    custom.or_else(|| {
        crate::registry::format::BUILTIN.iter()
            .find(|&&(number, _)| Some(u32::from(number)) == id && number != 0)
            .map(|&(_, code)| code.to_owned())
    })
}

fn number_format_with_override(
    id: Option<u32>,
    custom: Option<String>,
    date_override: Option<&str>,
) -> Option<String> {
    if id == Some(14) && custom.is_none() && let Some(code) = date_override {
        return Some(code.to_owned());
    }
    number_format_code(id, custom)
}

impl Workbook {
    /// Registers a cell format derived from an existing style by changing its number format.
    pub fn register_number_format_style(&mut self, style: u32, code: &str) -> Result<u32> {
        self.register_derived_style(style, StyleRequest::NumFormatOnly(NumFormat::Code(code.to_owned())))
    }

    /// Registers a cell format derived by changing only the font.
    pub fn register_font_style(&mut self, style: u32, font: Font) -> Result<u32> {
        self.register_derived_style(style, StyleRequest::FontOnly(Box::new(font)))
    }

    /// Registers a cell format derived by changing only its alignment.
    pub fn register_alignment_style(&mut self, style: u32, alignment: crate::Alignment) -> Result<u32> {
        self.register_derived_style(style, StyleRequest::AlignmentOnly(alignment))
    }

    /// Registers a cell format derived by changing only its protection.
    pub fn register_protection_style(&mut self, style: u32, protection: crate::Protection) -> Result<u32> {
        self.register_derived_style(style, StyleRequest::ProtectionOnly(protection))
    }

    /// Registers a cell format derived by changing only its fill.
    pub fn register_fill_style(&mut self, style: u32, fill: Fill) -> Result<u32> {
        self.register_derived_style(style, StyleRequest::FillOnly(fill))
    }

    /// Registers a cell format derived by changing only its border.
    pub fn register_border_style(&mut self, style: u32, border: Border) -> Result<u32> {
        self.register_derived_style(style, StyleRequest::BorderOnly(Box::new(border)))
    }

    /// Registers a complete style for a streamed cell without creating the cell.
    pub fn register_streaming_style(&mut self, format: Format) -> Result<u32> {
        self.register_derived_style(0, StyleRequest::Format(Box::new(format)))
    }

    fn register_derived_style(&mut self, style: u32, request: StyleRequest) -> Result<u32> {
        self.check_editable()?;
        self.styles()?;
        let registry = self.registries.styles.take().expect("activated");
        let plan = registry.plan(self, &request, style, None);
        self.registries.styles = Some(registry);
        let plan = plan?;
        let index = plan.s;
        self.poisoning(|book| {
            if let Some(doc) = plan.staged {
                *book.pkg.xml_part_mut(&plan.next.part)? = doc;
            }
            book.registries.styles = Some(plan.next);
            Ok(())
        })?;
        Ok(index)
    }

    /// Reads the protection flags referenced by a cell format index.
    pub fn protection_of_style(&mut self, index: u32) -> Result<Option<crate::Protection>> {
        let Some(part) = crate::registry::styles::styles_part(self)? else { return Ok(None) };
        let doc = self.pkg.xml_part(&part)?;
        let xf = doc.root().elements().find(|node| node.is(sml::CellFormats::QNAME))
            .and_then(|table| table.elements().nth(index as usize));
        let protection = xf.and_then(|node| node.elements().find(|child| child.is(sml::Protection::QNAME)));
        Ok(protection.map(project_protection))
    }

    /// Reads the border referenced by a cell format index.
    pub fn border_of_style(&mut self, index: u32) -> Result<Option<Border>> {
        let Some(part) = crate::registry::styles::styles_part(self)? else { return Ok(None) };
        let doc = self.pkg.xml_part(&part)?;
        let border = doc.root().elements().find(|node| node.is(sml::CellFormats::QNAME))
            .and_then(|table| table.elements().nth(index as usize))
            .and_then(|xf| xf.attr(ooxml_core::QName::new("", "borderId")))
            .and_then(|value| value.parse::<usize>().ok()).unwrap_or(0);
        if border == 0 {
            return Ok(None);
        }
        Ok(doc.root().elements().find(|node| node.is(sml::Borders::QNAME))
            .and_then(|borders| borders.elements().nth(border))
            .and_then(|node| crate::registry::read::border_from_node(node).ok()))
    }

    /// Reads the fill referenced by a cell format index.
    pub fn fill_of_style(&mut self, index: u32) -> Result<Option<Fill>> {
        self.read_style_fill(index, false)
    }

    /// Reads a cell format's fill, including the default fill at index zero.
    pub fn fill_of_style_including_default(&mut self, index: u32) -> Result<Option<Fill>> {
        self.read_style_fill(index, true)
    }

    fn read_style_fill(&mut self, index: u32, include_default: bool) -> Result<Option<Fill>> {
        let Some(part) = crate::registry::styles::styles_part(self)? else { return Ok(None) };
        let doc = self.pkg.xml_part(&part)?;
        let fill = doc.root().elements().find(|node| node.is(sml::CellFormats::QNAME))
            .and_then(|table| table.elements().nth(index as usize))
            .and_then(|xf| xf.attr(ooxml_core::QName::new("", "fillId")))
            .and_then(|value| value.parse::<usize>().ok()).unwrap_or(0);
        if fill == 0 && !include_default {
            return Ok(None);
        }
        Ok(doc.root().elements().find(|node| node.is(sml::Fills::QNAME))
            .and_then(|fills| fills.elements().nth(fill))
            .and_then(|node| crate::registry::read::fill_from_node(node).ok()))
    }

    /// Reads alignment attributes referenced by a cell format index.
    pub fn alignment_of_style(&mut self, index: u32) -> Result<Vec<(String, String)>> {
        let Some(part) = crate::registry::styles::styles_part(self)? else { return Ok(Vec::new()) };
        let doc = self.pkg.xml_part(&part)?;
        let alignment = doc.root().elements().find(|node| node.is(sml::CellFormats::QNAME))
            .and_then(|table| table.elements().nth(index as usize))
            .and_then(|xf| xf.elements().find(|node| node.is(sml::Alignment::QNAME)));
        Ok(alignment.map(project_alignment).unwrap_or_default())
    }

    /// Reads the font referenced by a cell format index.
    pub fn font_of_style(&mut self, index: u32) -> Result<Option<Font>> {
        self.font_node_of_style(index, crate::registry::read::font_from_node)
    }

    /// Reads the font referenced by a cell format index as written: a font
    /// without `name` or `sz` is still returned, with
    /// [`crate::DeclaredFont::fields`] marking which of the two it declares.
    pub fn declared_font_of_style(&mut self, index: u32) -> Result<Option<crate::DeclaredFont>> {
        self.font_node_of_style(index, crate::registry::read::declared_font_from_node)
    }

    fn font_node_of_style<T>(&mut self, index: u32, read: impl FnOnce(&ooxml_core::RawNode) -> Option<T>) -> Result<Option<T>> {
        let Some(part) = crate::registry::styles::styles_part(self)? else { return Ok(None) };
        let doc = self.pkg.xml_part(&part)?;
        let font = doc.root().elements().find(|node| node.is(sml::CellFormats::QNAME))
            .and_then(|table| table.elements().nth(index as usize))
            .and_then(|xf| xf.attr(ooxml_core::QName::new("", "fontId")))
            .and_then(|value| value.parse::<usize>().ok()).unwrap_or(0);
        Ok(doc.root().elements().find(|node| node.is(sml::Fonts::QNAME))
            .and_then(|fonts| fonts.elements().nth(font))
            .and_then(read))
    }

    /// A string write: shared when the workbook has a shared-string table,
    /// else inline.
    pub(crate) fn write_string(&mut self, sheet: usize, at: CellIndex, value: &str, style: Option<StyleRequest>) -> Result<()> {
        let was_active = self.registries.strings.is_some();
        let r = (|| {
            let (op, mut extra) = self.plan_string_write(value)?;
            extra.style = style;
            self.edit_cell_ext(sheet, at, op, extra)
        })();
        self.forget_activation_on_error(was_active, &r);
        r
    }

    /// A refused write leaves the registry as it found it: activation is
    /// otherwise a side effect of the call (the registry keeps the count of
    /// `t="s"` cells exact only once active), and a workbook rebuilt from
    /// the ops that were accepted would not have it (ADR-0024 §4.4, R2).
    pub(crate) fn forget_activation_on_error<T>(&mut self, was_active: bool, r: &Result<T>) {
        if !was_active && r.is_err() && self.poisoned.is_none() {
            self.registries.strings = None;
        }
    }

    pub(crate) fn plan_string_write(&mut self, value: &str) -> Result<(CellOp, Extra)> {
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
            Ok((CellOp::Shared(index), Extra { string: Some(plan), style: None }))
        } else {
            Ok((CellOp::Text(escaped), Extra::default()))
        }
    }

    /// A rich-string write (ADR-0006 §4.2): shared when the workbook has a
    /// shared-string table, else inline (`is/r`).
    pub(crate) fn write_rich(&mut self, sheet: usize, at: CellIndex, runs: &[(&Font, &str)]) -> Result<()> {
        let was_active = self.registries.strings.is_some();
        let r = self.write_rich_inner(sheet, at, runs);
        self.forget_activation_on_error(was_active, &r);
        r
    }

    /// Plans a rich-string write: validates the runs and reserves the
    /// item (shared) or builds inline runs, without applying. The caller
    /// edits the cell with the op ([`edit_cell_ext`](Workbook::edit_cell_ext)).
    pub(crate) fn plan_rich_write(
        &mut self,
        at: CellIndex,
        runs: &[(&Font, &str)],
    ) -> Result<(CellOp, Extra)> {
        use crate::registry::strings::{RichRun, SiContent};
        let bad = |message: String| Error::InvalidArgument { cell: Some(at.to_string()), message };
        if runs.is_empty() {
            return Err(bad("a rich string needs at least one run".into()));
        }
        let mut out = Vec::with_capacity(runs.len());
        let mut decoded = String::new();
        let mut units = 0;
        for (i, (font, text)) in runs.iter().enumerate() {
            // Excel refuses empty runs (rust_xlsxwriter does too)
            if text.is_empty() {
                return Err(bad(format!("run {i} is empty")));
            }
            Format::with_font((*font).clone()).validate()?;
            units += text.encode_utf16().count();
            let escaped = escape::encode(text).into_owned();
            if let Some((offset, ch)) = ooxml_core::value::first_invalid_xml_char(&escaped) {
                return Err(ooxml_core::Error::InvalidXmlChar { ch, offset }.into());
            }
            decoded.push_str(text);
            // rust_xlsxwriter's form: a first run in the default font has no rPr
            let plain_first = i == 0 && **font == Font::default();
            out.push(RichRun { font: (!plain_first).then(|| (*font).clone()), escaped });
        }
        if units > MAX_STRING_LEN {
            return Err(Error::StringTooLong(units));
        }
        if self.shared_strings_active()? {
            let plan = self.registries.strings.as_ref().expect("active").plan_content(SiContent::Rich(out), &decoded)?;
            let index = plan.index;
            Ok((
                CellOp::Shared(index),
                Extra {
                    string: Some(plan),
                    style: None,
                },
            ))
        } else {
            Ok((CellOp::InlineRich(out), Extra::default()))
        }
        }

    /// An optional-font rich-string write (browser adapters): like
    /// [`Self::write_rich`], but a run may omit its font (no `rPr`).
    pub(crate) fn write_rich_opt(&mut self, sheet: usize, at: CellIndex, runs: &[(Option<&Font>, &str)]) -> Result<()> {
        let was_active = self.registries.strings.is_some();
        let r = self.write_rich_opt_inner(sheet, at, runs);
        self.forget_activation_on_error(was_active, &r);
        r
    }

    fn write_rich_opt_inner(&mut self, sheet: usize, at: CellIndex, runs: &[(Option<&Font>, &str)]) -> Result<()> {
        use crate::registry::strings::SiContent;
        let (out, decoded) = prepare_rich_text(at, runs)?;
        if self.shared_strings_active()? {
            let plan = self.registries.strings.as_ref().expect("active").plan_content(SiContent::Rich(out), &decoded)?;
            let index = plan.index;
            self.edit_cell_ext(sheet, at, CellOp::Shared(index), Extra { string: Some(plan), style: None })
        } else {
            self.edit_cell_ext(sheet, at, CellOp::InlineRich(out), Extra::default())
        }
    }

    fn write_rich_inner(
        &mut self,
        sheet: usize,
        at: CellIndex,
        runs: &[(&Font, &str)],
    ) -> Result<()> {
        let (op, extra) = self.plan_rich_write(at, runs)?;
        self.edit_cell_ext(sheet, at, op, extra)
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

    /// Changes the workbook's date system, keeping existing numeric serials.
    pub fn set_date1904(&mut self, enabled: bool) -> Result<()> {
        self.check_editable()?;
        if self.date1904()? == enabled {
            return Ok(());
        }
        let part = self.workbook_part.clone();
        self.poisoning(|wb| {
            let doc = wb.pkg.xml_part_mut(&part)?;
            let mut root = doc.lens_mut::<sml::Workbook>()?;
            let mut properties = root.get_or_insert::<sml::WorkbookProperties>()?;
            properties.set_attr(sml::WorkbookProperties::ATTR_DATE1904, if enabled { "1" } else { "0" })?;
            Ok(())
        })
    }

    /// Gives the cell the `xf` described by `xf` (see
    /// [`XfContent`]); the value is kept. `base` bounds the `Base(i)`
    /// components (the counts the content was captured against). Nothing is
    /// changed on `Err`.
    pub(crate) fn set_cell_xf(&mut self, sheet: usize, at: CellIndex, xf: &XfContent, base: &BaseStyleCounts) -> Result<()> {
        self.edit_cell_ext(sheet, at, CellOp::StyleOnly, Extra { string: None, style: Some(StyleRequest::Xf(Box::new(xf.clone()), *base)) })
    }

    /// Number of entries in `cellXfs` (activates the style registry).
    pub fn cell_format_count(&mut self) -> Result<u32> {
        Ok(self.styles()?.xf_count())
    }
}

/// The error value `value` names (ECMA-376 §18.17.4.6 plus Excel's
/// `#GETTING_DATA`), else [`Error::InvalidArgument`]. Shared by `set_error`
/// and the Journal's plan input, so the table cannot diverge.
pub(crate) fn error_value(at: CellIndex, value: &str) -> Result<&'static str> {
    const ERRORS: &[&str] = &[
        "#NULL!",
        "#DIV/0!",
        "#VALUE!",
        "#REF!",
        "#NAME?",
        "#NUM!",
        "#N/A",
        "#GETTING_DATA",
    ];
    ERRORS
        .iter()
        .copied()
        .find(|e| *e == value)
        .ok_or_else(|| Error::InvalidArgument {
            cell: Some(at.to_string()),
            message: format!("{value:?} is not an error value"),
        })
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
        self.edit(CellOp::Formula(text.to_owned(), None), Some(StyleRequest::Format(Box::new(format.clone()))))
    }

    /// Writes an error value (`t="e"`): one of `#NULL!`, `#DIV/0!`,
    /// `#VALUE!`, `#REF!`, `#NAME?`, `#NUM!`, `#N/A`, `#GETTING_DATA`
    /// (ECMA-376 §18.17.4.6 plus Excel's `#GETTING_DATA`), else
    /// [`Error::InvalidArgument`]. A value write, as [`Self::set_number`].
    pub fn set_error(self, value: &str) -> Result<()> {
        let at = self.at;
        self.edit(CellOp::Error(error_value(at, value)?), None)
    }

    /// Writes a rich string: runs of text, each in its own font (ADR-0006
    /// §4.2). With a shared-string table it is a new `si` of `r` runs (an
    /// existing item with exactly these bytes is reused); without one an
    /// inline `is` of runs. Each run's `rPr` is the font's canonical form
    /// (`rFont` for the face, as Excel writes it); as in rust_xlsxwriter, a
    /// first run in [`Font::default`] gets no `rPr`. `s` is kept.
    ///
    /// Errors (nothing changed): no runs or an empty run
    /// ([`Error::InvalidArgument`]), more than 32,767 UTF-16 units in total
    /// ([`Error::StringTooLong`]), an invalid font
    /// ([`Error::InvalidFormat`]), and those of [`Self::set_string`].
    pub fn set_rich_string(self, runs: &[(&Font, &str)]) -> Result<()> {
        let ws = &mut *self.ws;
        ws.wb.write_rich(ws.sheet, self.at, runs)
    }

    /// Writes rich text with optional formatting on each run.
    pub fn set_rich_text(self, runs: &[(Option<&Font>, &str)]) -> Result<()> {
        self.ws.wb.write_rich_opt(self.ws.sheet, self.at, runs)
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

    /// Changes the cell alignment, keeping the other style groups.
    pub fn set_alignment(self, alignment: crate::Alignment) -> Result<()> {
        self.edit(CellOp::StyleOnly, Some(StyleRequest::AlignmentOnly(alignment)))
    }

    /// Changes the cell protection settings, keeping the other style groups.
    pub fn set_protection(self, protection: crate::Protection) -> Result<()> {
        self.edit(CellOp::StyleOnly, Some(StyleRequest::ProtectionOnly(protection)))
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

    /// Writes a UTC ISO date or a time-only cell with millisecond precision.
    pub fn set_iso_date(self, value: &str) -> Result<()> {
        let op = iso_date_op(self.at, value)?;
        self.edit(op, None)
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
    /// Adds a differential format to `styles.xml`'s `dxfs` and returns its
    /// index (ADR-0006 §5.8). Append-only: a `dxf` whose bytes are exactly
    /// the canonical form of `dxf` is reused, also one an earlier session
    /// appended. Nothing refers to it until a Phase4 feature (conditional
    /// formats, table styles) uses the index; an unused `dxf` is valid.
    ///
    /// Errors (nothing changed): [`Error::NoStylesPart`],
    /// [`Error::InvalidFormat`], [`Error::BuiltinNumFormatRedefined`],
    /// [`Error::NumFmtIdExhausted`], [`Error::DanglingReference`] (an
    /// existing `dxfId` in any part already points at or past the end of
    /// `dxfs`), and core `UnresolvedSibling` when `dxfs` has an
    /// `mc:AlternateContent` child.
    pub fn add_differential_format(&mut self, dxf: &crate::DxfFormat) -> Result<u32> {
        self.styles()?;
        let st = self.registries.styles.take().expect("activated");
        let plan = st.plan_dxf(self, dxf);
        self.registries.styles = Some(st);
        let plan = plan?;
        // install: the staged styles.xml and the registry, nothing else
        self.poisoned = Some("an edit was interrupted while being applied".into());
        let result = (|| -> Result<()> {
            if let Some(doc) = plan.staged {
                *self.pkg.xml_part_mut(&plan.next.part)? = doc;
            }
            Ok(())
        })();
        match result {
            Ok(()) => {
                self.registries.styles = Some(plan.next);
                self.poisoned = None;
                Ok(plan.s)
            }
            Err(e) => {
                let msg = e.to_string();
                self.poisoned = Some(msg.clone());
                Err(Error::Poisoned(msg))
            }
        }
    }

    /// The `cellXfs` entry `index` read back as a [`Format`]; see
    /// [`crate::Worksheet::format`].
    pub fn format_of_style(&mut self, index: u32) -> Result<crate::FormatRead> {
        let Some(part) = crate::registry::styles::styles_part(self)? else {
            return Ok(crate::FormatRead::Expressible(Box::new(Format::new())));
        };
        let doc = self.pkg.xml_part(&part)?;
        Ok(crate::registry::read::read_format(doc, index))
    }

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
        Ok(kids.get(font_id).and_then(|f| crate::registry::read::font_from_node(f)))
    }
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
    ///
    /// A read: it never activates the style registry (PR #50 review m5).
    /// A workbook without a styles part has no date formats (`false`).
    pub fn is_date(&mut self, row: u32, col: u32) -> Result<bool> {
        let (id, custom) = self.number_format_parts(row, col)?;
        Ok(crate::registry::format::is_date_format(id, custom.as_deref()))
    }

    /// Tests a style's number format with the same rule as a cell date read.
    pub fn is_date_style(&mut self, style: u32) -> Result<bool> {
        let (id, custom) = self.number_format_parts_for_style(style)?;
        Ok(crate::registry::format::is_date_format(id, custom.as_deref()))
    }

    /// Returns the cell's custom or built-in number format code.
    pub fn number_format(&mut self, row: u32, col: u32) -> Result<Option<String>> {
        self.number_format_with_date_override(row, col, None)
    }

    /// Returns the number format with an optional display override for built-in format 14.
    /// A custom format stored at that ID takes precedence.
    pub fn number_format_with_date_override(
        &mut self,
        row: u32,
        col: u32,
        date_override: Option<&str>,
    ) -> Result<Option<String>> {
        let (id, custom) = self.number_format_parts(row, col)?;
        Ok(number_format_with_override(id, custom, date_override))
    }

    /// Resolves a style's number format without looking up a worksheet cell.
    pub fn number_format_for_style(
        &mut self,
        style: u32,
        date_override: Option<&str>,
    ) -> Result<Option<String>> {
        let (id, custom) = self.number_format_parts_for_style(style)?;
        Ok(number_format_with_override(id, custom, date_override))
    }

    /// Reads the configured column default number format.
    pub fn column_number_format(&mut self, col: u32) -> Result<Option<String>> {
        let style = self.column_style_index(col)?;
        let (id, custom) = self.number_format_parts_for_style(style)?;
        Ok(number_format_code(id, custom))
    }

    /// Reads the configured row default number format.
    pub fn row_number_format(&mut self, row: u32) -> Result<Option<String>> {
        let Some(style) = self.row_style_index(row)? else { return Ok(None) };
        let (id, custom) = self.number_format_parts_for_style(style)?;
        Ok(number_format_code(id, custom))
    }

    /// Returns the cell number format ID and custom code, when present.
    pub fn number_format_parts(&mut self, row: u32, col: u32) -> Result<(Option<u32>, Option<String>)> {
        let Some(style) = self.effective_cell_style_index(row, col)? else { return Ok((None, None)) };
        self.number_format_parts_for_style(style)
    }

    /// Reads the explicit cell format, falling back to the row and column defaults.
    pub fn effective_cell_style_index(&self, row: u32, col: u32) -> Result<Option<u32>> {
        let Some(cell) = self.cell(row, col)? else { return Ok(None) };
        self.effective_style_for_cell(&cell).map(Some)
    }

    /// Resolves the effective style using a cell view from this worksheet.
    pub fn effective_style_for_cell(&self, cell: &crate::CellRef<'_>) -> Result<u32> {
        if cell.raw().attr(sml::Cell::ATTR_S).is_some() {
            return cell.style_index();
        }
        let at = cell.index();
        match self.row_style_index(at.row)? {
            Some(index) => Ok(index),
            None => self.column_style_index(at.col),
        }
    }

    fn number_format_parts_for_style(&mut self, s: u32) -> Result<(Option<u32>, Option<String>)> {
        if let Some(st) = self.wb.registries.styles.as_ref() {
            let (id, code) = st.num_format_of(s);
            let custom = id.filter(|i| st.codes.contains_key(i)).and(code);
            return Ok((id, custom));
        }
        let Some(part) = crate::registry::styles::styles_part(self.wb)? else { return Ok((None, None)) };
        let doc = self.wb.pkg.xml_part(&part)?;
        let root = doc.root();
        if !root.is(sml::Stylesheet::QNAME) {
            return Ok((None, None));
        }
        let table = |local: &str| root.elements().find(|e| e.name().ns() == crate::SML_NS && e.name().local() == local);
        let id = table("cellXfs")
            .and_then(|t| t.elements().nth(s as usize))
            .filter(|x| x.is(sml::CellFormat::QNAME))
            .and_then(|x| x.attr(ooxml_core::QName::new("", "numFmtId")))
            .and_then(|v| v.trim().parse::<u32>().ok());
        let custom = id.and_then(|i| {
            table("numFmts")?
                .elements()
                .filter(|e| e.is(sml::NumberingFormat::QNAME))
                .find(|e| e.attr(ooxml_core::QName::new("", "numFmtId")).and_then(|v| v.trim().parse::<u32>().ok()) == Some(i))
                .and_then(|e| e.attr(ooxml_core::QName::new("", "formatCode")).map(str::to_owned))
        });
        Ok((id, custom))
    }

    /// Reads the border referenced by a cell's style.
    pub fn cell_border(&mut self, row: u32, col: u32) -> Result<Option<crate::Border>> {
        let Some(index) = self.effective_cell_style_index(row, col)? else {
            return Ok(None);
        };
        self.wb.border_of_style(index)
    }

    /// Reads the pattern fill referenced by a cell's style.
    pub fn cell_fill(&mut self, row: u32, col: u32) -> Result<Option<crate::Fill>> {
        let Some(index) = self.effective_cell_style_index(row, col)? else {
            return Ok(None);
        };
        self.wb.fill_of_style(index)
    }

    /// Reads the font referenced by a cell's style.
    pub fn cell_font(&mut self, row: u32, col: u32) -> Result<Option<Font>> {
        let Some(index) = self.effective_cell_style_index(row, col)? else {
            return Ok(None);
        };
        self.wb.font_of_style(index)
    }

    /// Reads the alignment attributes represented by the cell's style.
    pub fn cell_alignment(&mut self, row: u32, col: u32) -> Result<Vec<(String, String)>> {
        let Some(index) = self.effective_cell_style_index(row, col)? else {
            return Ok(Vec::new());
        };
        self.wb.alignment_of_style(index)
    }

    /// Reads the protection group stored in the cell's style.
    pub fn cell_protection(&mut self, row: u32, col: u32) -> Result<Option<crate::Protection>> {
        let Some(index) = self.effective_cell_style_index(row, col)? else {
            return Ok(None);
        };
        self.wb.protection_of_style(index)
    }

    /// The cell's format read back as a [`Format`] (ADR-0006 §3
    /// "Reading"): [`FormatRead::Expressible`], or
    /// [`FormatRead::NotExpressible`] with the reasons when the cell's `xf`
    /// (or its font, fill or border) has anything outside the model. `None`
    /// for a missing cell. A read: the style registry is not activated; a
    /// workbook without a styles part reads as `Format::new()`.
    pub fn format(&mut self, row: u32, col: u32) -> Result<Option<crate::FormatRead>> {
        let Some(s) = self.effective_cell_style_index(row, col)? else { return Ok(None) };
        self.wb.format_of_style(s).map(Some)
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
