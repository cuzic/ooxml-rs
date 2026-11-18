//! Phase4 part 2: data validation (ADR-0014 §5), 2007 form.

use ooxml_core::{ElementSpec, LensMut, QName};
use ooxml_schema::sml;

use crate::error::{Error, Result};
use crate::reference::CellRange;
use crate::workbook::Workbook;
use crate::worksheet::Worksheet;
use crate::x14;

const fn q(local: &'static str) -> QName {
    QName::new("", local)
}

/// What a validation allows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValidationType {
    /// Whole numbers.
    Whole,
    /// Decimal numbers.
    Decimal,
    /// A value from a list.
    List,
    /// Dates.
    Date,
    /// Times.
    Time,
    /// Text of a length.
    TextLength,
    /// A formula that must be true.
    Custom,
}

/// The comparison of `Whole`, `Decimal`, `Date`, `Time`, `TextLength`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValidationOperator {
    /// Between `formula1` and `formula2` (the default).
    Between,
    /// Not between.
    NotBetween,
    /// Equal.
    Equal,
    /// Not equal.
    NotEqual,
    /// Less than.
    LessThan,
    /// Less than or equal.
    LessThanOrEqual,
    /// Greater than.
    GreaterThan,
    /// Greater than or equal.
    GreaterThanOrEqual,
}

/// The error alert's style.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorStyle {
    /// Refuse the value (the default).
    Stop,
    /// Warn.
    Warning,
    /// Inform.
    Information,
}

/// A 2007-form data validation (ADR-0014 §5).
#[derive(Clone, Debug)]
pub struct Validation {
    kind: ValidationType,
    operator: Option<ValidationOperator>,
    formula1: Option<String>,
    formula2: Option<String>,
    list: Option<Vec<String>>,
    allow_blank: bool,
    hide_dropdown: bool,
    input: Option<(String, String)>,
    error: Option<(String, String, ErrorStyle)>,
}

impl Validation {
    fn new(kind: ValidationType) -> Self {
        Validation { kind, operator: None, formula1: None, formula2: None, list: None, allow_blank: true, hide_dropdown: false, input: None, error: None }
    }

    /// A drop-down of these values (written as an inline list `"a,b,c"`).
    pub fn list(items: &[&str]) -> Self {
        let mut v = Self::new(ValidationType::List);
        v.list = Some(items.iter().map(|s| (*s).to_owned()).collect());
        v
    }

    /// A drop-down whose values come from a range or a defined name on the
    /// same sheet (`"$A$1:$A$9"`, `"=Colours"`). A range on **another
    /// sheet** is refused ([`Error::CrossSheetReference`]): define a name for
    /// it and use the name (ADR-0014 Q2).
    pub fn list_from(formula: &str) -> Self {
        let mut v = Self::new(ValidationType::List);
        v.formula1 = Some(formula.to_owned());
        v
    }

    /// `kind` (not `List`/`Custom`) compared with `op` against `formula1`
    /// (and `formula2` for between / not between).
    pub fn compare(kind: ValidationType, op: ValidationOperator, formula1: &str, formula2: Option<&str>) -> Self {
        let mut v = Self::new(kind);
        v.operator = Some(op);
        v.formula1 = Some(formula1.to_owned());
        v.formula2 = formula2.map(str::to_owned);
        v
    }

    /// A formula that must evaluate to true (relative to the top-left cell
    /// of the first range).
    pub fn custom(formula: &str) -> Self {
        let mut v = Self::new(ValidationType::Custom);
        v.formula1 = Some(formula.to_owned());
        v
    }

    /// Whether blank cells pass (default `true`).
    pub fn allow_blank(mut self, on: bool) -> Self {
        self.allow_blank = on;
        self
    }

    /// Hides a list's in-cell drop-down.
    pub fn hide_dropdown(mut self, on: bool) -> Self {
        self.hide_dropdown = on;
        self
    }

    /// The input message (title ≤ 32, message ≤ 255 characters).
    pub fn input_message(mut self, title: &str, message: &str) -> Self {
        self.input = Some((title.to_owned(), message.to_owned()));
        self
    }

    /// The error alert (title ≤ 32, message ≤ 255 characters).
    pub fn error_message(mut self, title: &str, message: &str, style: ErrorStyle) -> Self {
        self.error = Some((title.to_owned(), message.to_owned(), style));
        self
    }
}

fn len16(s: &str) -> usize {
    s.encode_utf16().count()
}

/// The space-separated ranges of an `sqref`.
fn ranges(sqref: &str) -> Result<Vec<CellRange>> {
    let v: Vec<CellRange> = sqref.split_ascii_whitespace().map(CellRange::parse).collect::<Result<_>>()?;
    if v.is_empty() {
        return Err(Error::InvalidReference(sqref.to_owned()));
    }
    Ok(v)
}

fn overlaps(a: &[CellRange], b: &[CellRange]) -> bool {
    a.iter().any(|x| b.iter().any(|y| x.intersects(y)))
}

impl Workbook {
    fn add_validation(&mut self, sheet: usize, sqref: &str, v: &Validation) -> Result<()> {
        self.check_editable()?;
        let new = ranges(sqref)?;
        let sqref = new.iter().map(ToString::to_string).collect::<Vec<_>>().join(" ");
        // formulas
        let formula1 = match (&v.list, &v.formula1) {
            (Some(items), _) => {
                if let Some(i) = items.iter().find(|i| i.contains(',')) {
                    return Err(Error::ListItemHasComma(i.clone()));
                }
                let text = format!("\"{}\"", items.iter().map(|i| i.replace('"', "\"\"")).collect::<Vec<_>>().join(","));
                if len16(&text) > 255 {
                    return Err(Error::ListTooLong(len16(&text)));
                }
                if let Some((offset, ch)) = ooxml_core::value::first_invalid_xml_char(&text) {
                    return Err(ooxml_core::Error::InvalidXmlChar { ch, offset }.into());
                }
                Some(text)
            }
            (None, Some(f)) => Some(self.check_range_formula(sheet, f)?),
            (None, None) => None,
        };
        let formula2 = v.formula2.as_deref().map(|f| self.check_range_formula(sheet, f)).transpose()?;
        let texts = [v.input.as_ref().map(|i| (&i.0, &i.1)), v.error.as_ref().map(|e| (&e.0, &e.1))];
        for (title, message) in texts.into_iter().flatten() {
            if len16(title) > 32 || len16(message) > 255 {
                return Err(Error::InvalidSetting { what: "validation message".into(), reason: "title over 32 or message over 255 characters".into() });
            }
            for t in [title, message] {
                if let Some((offset, ch)) = ooxml_core::value::first_invalid_xml_char(t) {
                    return Err(ooxml_core::Error::InvalidXmlChar { ch, offset }.into());
                }
            }
        }
        // overlap with 2007 and x14 rules
        let part = self.sheets[sheet].part.clone().expect("worksheet");
        {
            let doc = self.pkg.xml_part(&part)?;
            let ws = doc.root();
            let mut existing: Vec<String> = ws
                .elements()
                .filter(|e| e.is(sml::DataValidations::QNAME))
                .flat_map(|d| d.elements().filter(|e| e.is(sml::DataValidation::QNAME)).filter_map(|e| e.attr(q("sqref")).map(str::to_owned)).collect::<Vec<_>>())
                .collect();
            for (_, ext) in x14::exts(ws, x14::URI_DV) {
                for dvs in ext.elements() {
                    existing.extend(dvs.elements().filter_map(x14::sqref));
                }
            }
            for e in existing {
                if overlaps(&ranges(&e).unwrap_or_default(), &new) {
                    return Err(Error::ValidationOverlap { sqref, existing: e });
                }
            }
        }
        {
            let doc = self.pkg.xml_part_mut(&part)?;
            let mut ws = doc.lens_mut::<sml::Worksheet>()?;
            match ws.child_mut::<sml::DataValidations>() {
                Some(d) => d.check_insert::<sml::DataValidation>()?,
                None => ws.check_insert_by::<sml::DataValidations>(|_, _| false)?,
            }
        }
        let v = v.clone();
        self.poisoning_layout(|wb| {
            let doc = wb.pkg.xml_part_mut(&part)?;
            let mut ws = doc.lens_mut::<sml::Worksheet>()?;
            let mut dvs = ws.get_or_insert::<sml::DataValidations>()?;
            {
                let mut d = dvs.insert::<sml::DataValidation>()?;
                write_validation(&mut d, &v, &sqref, formula1.as_deref(), formula2.as_deref())?;
            }
            let n = dvs.node().elements().filter(|e| e.is(sml::DataValidation::QNAME)).count();
            dvs.set_attr(q("count"), &n.to_string())?;
            Ok(())
        })
    }

    fn remove_validation(&mut self, sheet: usize, sqref: &str) -> Result<()> {
        self.check_editable()?;
        let want = ranges(sqref)?;
        let part = self.sheets[sheet].part.clone().expect("worksheet");
        let (plain, ext_hits) = {
            let doc = self.pkg.xml_part(&part)?;
            let ws = doc.root();
            let plain: Vec<usize> = ws
                .elements()
                .find(|e| e.is(sml::DataValidations::QNAME))
                .map(|d| d.indexed_elements().filter(|(_, e)| e.is(sml::DataValidation::QNAME) && e.attr(q("sqref")).and_then(|s| ranges(s).ok()) == Some(want.clone())).map(|(i, _)| i).collect())
                .unwrap_or_default();
            let mut hits = Vec::new();
            for (ei, ext) in x14::exts(ws, x14::URI_DV) {
                for (di, dvs) in ext.indexed_elements().filter(|(_, e)| e.is(x14::DataValidations::QNAME)) {
                    let items: Vec<usize> = dvs.indexed_elements().filter(|(_, r)| x14::sqref(r).and_then(|s| ranges(&s).ok()) == Some(want.clone())).map(|(i, _)| i).collect();
                    let total = dvs.elements().count();
                    if !items.is_empty() {
                        hits.push((ei, di, items, total));
                    }
                }
            }
            (plain, hits)
        };
        if plain.is_empty() && ext_hits.is_empty() {
            return Err(Error::NoSuchRule { what: "validation", sqref: sqref.to_owned() });
        }
        self.poisoning_layout(|wb| {
            let doc = wb.pkg.xml_part_mut(&part)?;
            let mut ws = doc.lens_mut::<sml::Worksheet>()?;
            if !plain.is_empty() {
                let mut dvs = ws.child_mut::<sml::DataValidations>().expect("planned");
                for &i in plain.iter().rev() {
                    dvs.remove_at(i);
                }
                let n = dvs.node().elements().filter(|e| e.is(sml::DataValidation::QNAME)).count();
                if n == 0 {
                    ws.remove::<sml::DataValidations>(); // an empty one is repaired by Excel
                } else {
                    dvs.set_attr(q("count"), &n.to_string())?;
                }
            }
            for (ei, di, items, total) in ext_hits.iter().rev() {
                let mut list = ws.child_mut::<sml::WorksheetExtensionList>().expect("planned");
                if items.len() == *total {
                    list.remove_at(*ei); // the ext became empty
                    continue;
                }
                let ext = list.into_child_at::<sml::WorksheetExtension>(*ei).expect("planned");
                let mut dvs = ext.into_child_at::<x14::DataValidations>(*di).expect("planned");
                for &i in items.iter().rev() {
                    dvs.remove_at(i);
                }
                let n = dvs.node().elements().count();
                dvs.set_attr(q("count"), &n.to_string())?;
            }
            Ok(())
        })
    }
}

fn write_validation(d: &mut LensMut<'_, sml::DataValidation>, v: &Validation, sqref: &str, f1: Option<&str>, f2: Option<&str>) -> Result<()> {
    let kind = match v.kind {
        ValidationType::Whole => "whole",
        ValidationType::Decimal => "decimal",
        ValidationType::List => "list",
        ValidationType::Date => "date",
        ValidationType::Time => "time",
        ValidationType::TextLength => "textLength",
        ValidationType::Custom => "custom",
    };
    d.set_attr(q("type"), kind)?;
    if let Some((_, _, style)) = &v.error {
        match style {
            ErrorStyle::Stop => {}
            ErrorStyle::Warning => d.set_attr(q("errorStyle"), "warning")?,
            ErrorStyle::Information => d.set_attr(q("errorStyle"), "information")?,
        }
    }
    if let Some(op) = v.operator.filter(|o| *o != ValidationOperator::Between) {
        let op = match op {
            ValidationOperator::Between => unreachable!(),
            ValidationOperator::NotBetween => "notBetween",
            ValidationOperator::Equal => "equal",
            ValidationOperator::NotEqual => "notEqual",
            ValidationOperator::LessThan => "lessThan",
            ValidationOperator::LessThanOrEqual => "lessThanOrEqual",
            ValidationOperator::GreaterThan => "greaterThan",
            ValidationOperator::GreaterThanOrEqual => "greaterThanOrEqual",
        };
        d.set_attr(q("operator"), op)?;
    }
    if v.allow_blank {
        d.set_attr(q("allowBlank"), "1")?;
    }
    // `showDropDown="1"` *hides* the drop-down (ECMA-376's naming)
    if v.hide_dropdown {
        d.set_attr(q("showDropDown"), "1")?;
    }
    if v.input.is_some() {
        d.set_attr(q("showInputMessage"), "1")?;
    }
    if v.error.is_some() {
        d.set_attr(q("showErrorMessage"), "1")?;
    }
    if let Some((t, m, _)) = &v.error {
        d.set_attr(q("errorTitle"), t)?;
        d.set_attr(q("error"), m)?;
    }
    if let Some((t, m)) = &v.input {
        d.set_attr(q("promptTitle"), t)?;
        d.set_attr(q("prompt"), m)?;
    }
    d.set_attr(q("sqref"), sqref)?;
    if let Some(f) = f1 {
        d.insert::<sml::Formula1>()?.set_text(f)?;
    }
    if let Some(f) = f2 {
        d.insert::<sml::Formula2>()?.set_text(f)?;
    }
    Ok(())
}

impl Worksheet<'_> {
    /// Adds a 2007-form data validation on `sqref` (`"A1:A9"`, or several
    /// ranges separated by spaces; ADR-0014 §5). Formulas are relative to
    /// the top-left cell of the first range. Refused, with every byte
    /// unchanged: overlap with an existing rule, 2007 or x14
    /// ([`Error::ValidationOverlap`]); an inline list over 255 characters
    /// ([`Error::ListTooLong`]) or with a comma in an item
    /// ([`Error::ListItemHasComma`]); a formula that does not lex, is over 255
    /// characters, or refers to another sheet
    /// ([`Error::CrossSheetReference`]: use a defined name).
    pub fn add_validation(&mut self, sqref: &str, validation: &Validation) -> Result<()> {
        self.wb.add_validation(self.sheet, sqref, validation)
    }

    /// Removes the validations whose `sqref` is exactly `sqref`, 2007 or
    /// x14 (an x14 `ext` that becomes empty goes too); [`Error::NoSuchRule`]
    /// when there is none.
    pub fn remove_validation(&mut self, sqref: &str) -> Result<()> {
        self.wb.remove_validation(self.sheet, sqref)
    }
}
