//! Phase4 part 2: data validation (ADR-0014 §5), 2007 form.

use ooxml_core::{ElementSpec, LensMut, QName, RawNode};
use ooxml_schema::sml;

use crate::error::{Error, Result};
use crate::reference::CellRange;
use crate::workbook::Workbook;
use crate::worksheet::Worksheet;
use crate::x14;

const fn q(local: &'static str) -> QName {
    QName::new("", local)
}

/// A worksheet's standard data validation rule and its range list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidationRuleAttributes {
    /// The space-separated OOXML cell references covered by the rule.
    pub reference: String,
    /// The supported rule attributes and formulas.
    pub attributes: Vec<(String, String)>,
}

fn rule_attributes(rule: &RawNode) -> Vec<(String, String)> {
    let mut values: Vec<(String, String)> = [
        "type", "operator", "allowBlank", "error", "errorTitle", "errorStyle", "prompt",
        "promptTitle", "showErrorMessage", "showInputMessage",
    ].into_iter().filter_map(|key| rule.attr(q(key)).map(|value| (key.into(), value.into()))).collect();
    if rule.attr(q("type")).is_none_or(|kind| kind == "none") {
        values.retain(|(key, _)| key != "type");
        values.push(("type".into(), "any".into()));
    }
    for (key, tag) in [("formula1", sml::Formula1::QNAME), ("formula2", sml::Formula2::QNAME)] {
        if let Some(element) = rule.elements().find(|child| child.is(tag)) {
            values.push((key.into(), element.text().into_owned()));
        }
    }
    values
}

/// What a validation allows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValidationType {
    /// Any input, optionally with a prompt.
    Any,
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
    show_input: bool,
    error: Option<(String, String, ErrorStyle)>,
    show_error: bool,
}

impl Validation {
    fn new(kind: ValidationType) -> Self {
        Validation { kind, operator: None, formula1: None, formula2: None, list: None, allow_blank: true, hide_dropdown: false, input: None, show_input: false, error: None, show_error: false }
    }

    /// An input prompt without a value constraint.
    pub fn any() -> Self {
        Self::new(ValidationType::Any).allow_blank(false)
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
        self.show_input = true;
        self
    }

    /// The error alert (title ≤ 32, message ≤ 255 characters).
    pub fn error_message(mut self, title: &str, message: &str, style: ErrorStyle) -> Self {
        self.error = Some((title.to_owned(), message.to_owned(), style));
        self.show_error = true;
        self
    }

    /// Controls message visibility independently of the saved text.
    pub fn show_messages(mut self, input: bool, error: bool) -> Self {
        self.show_input = input;
        self.show_error = error;
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

fn validation_references(sheet: &RawNode) -> impl Iterator<Item = String> + '_ {
    let standard = sheet.elements()
        .filter(|node| node.is(sml::DataValidations::QNAME))
        .flat_map(|rules| rules.elements().filter(|node| node.is(sml::DataValidation::QNAME)))
        .map(|rule| rule.attr(q("sqref")).unwrap_or_default().to_owned());
    let extended = x14::exts(sheet, x14::URI_DV).into_iter().flat_map(|(_, ext)| {
        ext.elements().filter(|node| node.is(x14::DataValidations::QNAME))
            .flat_map(|rules| rules.elements().filter(|node| node.is(x14::DV_RULE)))
            .map(|rule| x14::sqref(rule).unwrap_or_default())
    });
    standard.chain(extended)
}

impl Workbook {
    fn add_validation(&mut self, sheet: usize, sqref: &str, v: &Validation) -> Result<()> {
        self.check_editable()?;
        let new = ranges(sqref)?;
        let sqref = new.iter().map(ToString::to_string).collect::<Vec<_>>().join(" ");
        if v.operator.is_some() && matches!(v.kind, ValidationType::Any | ValidationType::List | ValidationType::Custom) {
            return Err(Error::InvalidSetting {
                what: "validation operator".into(),
                reason: "this validation type does not support comparison operators".into(),
            });
        }
        let between = matches!(v.operator, Some(ValidationOperator::Between | ValidationOperator::NotBetween));
        crate::rangefmla::check_second_formula(between, v.formula2.is_some())?;
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
            for e in validation_references(doc.root()) {
                if !ranges(&e).is_ok_and(|old| !overlaps(&old, &new)) {
                    return Err(Error::ValidationOverlap { sqref, existing: e });
                }
            }
        }
        {
            let doc = self.pkg.xml_part_mut(&part)?;
            let mut ws = doc.lens_mut::<sml::Worksheet>()?;
            match ws.child_mut::<sml::DataValidations>() {
                Some(d) => d.check_insert::<sml::DataValidation>()?,
                None => ws.check_insert::<sml::DataValidations>()?,
            }
        }
        let v = v.clone();
        self.poisoning(|wb| {
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
                    let items: Vec<usize> = dvs.indexed_elements().filter(|(_, r)| r.is(x14::DV_RULE) && x14::sqref(r).and_then(|s| ranges(&s).ok()) == Some(want.clone())).map(|(i, _)| i).collect();
                    if !items.is_empty() {
                        hits.push((ei, di, items));
                    }
                }
            }
            (plain, hits)
        };
        if plain.is_empty() && ext_hits.is_empty() {
            return Err(Error::NoSuchRule { what: "validation", sqref: sqref.to_owned() });
        }
        self.poisoning(|wb| {
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
            for (ei, di, items) in ext_hits.iter().rev() {
                let mut list = ws.child_mut::<sml::WorksheetExtensionList>().expect("planned");
                let ext_empty;
                {
                    let mut ext = x14::child_at::<_, sml::WorksheetExtension>(&mut list, *ei);
                    let mut dvs = x14::child_at::<_, x14::DataValidations>(&mut ext, *di);
                    for &i in items.iter().rev() {
                        dvs.remove_at(i);
                    }
                    if dvs.node().elements().next().is_none() {
                        ext.remove_at(*di);
                    } else {
                        let n = dvs.node().elements().filter(|e| e.is(x14::DV_RULE)).count();
                        dvs.set_attr(q("count"), &n.to_string())?;
                    }
                    ext_empty = ext.node().elements().next().is_none();
                }
                if ext_empty {
                    list.remove_at(*ei);
                }
            }
            Ok(())
        })
    }
}

fn write_validation(d: &mut LensMut<'_, sml::DataValidation>, v: &Validation, sqref: &str, f1: Option<&str>, f2: Option<&str>) -> Result<()> {
    let kind = match v.kind {
        ValidationType::Any => "none",
        ValidationType::Whole => "whole",
        ValidationType::Decimal => "decimal",
        ValidationType::List => "list",
        ValidationType::Date => "date",
        ValidationType::Time => "time",
        ValidationType::TextLength => "textLength",
        ValidationType::Custom => "custom",
    };
    if v.kind != ValidationType::Any {
        d.set_attr(q("type"), kind)?;
    }
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
    if v.allow_blank && v.kind != ValidationType::Any {
        d.set_attr(q("allowBlank"), "1")?;
    }
    // `showDropDown="1"` *hides* the drop-down (ECMA-376's naming)
    if v.hide_dropdown {
        d.set_attr(q("showDropDown"), "1")?;
    }
    if v.show_input {
        d.set_attr(q("showInputMessage"), "1")?;
    }
    if v.show_error {
        d.set_attr(q("showErrorMessage"), "1")?;
    }
    if let Some((t, m, _)) = &v.error {
        if !t.is_empty() {
            d.set_attr(q("errorTitle"), t)?;
        }
        if !m.is_empty() {
            d.set_attr(q("error"), m)?;
        }
    }
    if let Some((t, m)) = &v.input {
        if !t.is_empty() {
            d.set_attr(q("promptTitle"), t)?;
        }
        if !m.is_empty() {
            d.set_attr(q("prompt"), m)?;
        }
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

fn validation_range_without(range: CellRange, at: crate::CellIndex) -> Vec<CellRange> {
    use crate::CellIndex;
    if !range.contains(at) {
        return vec![range];
    }
    let CellRange { first, last } = range;
    let mut remaining = Vec::new();
    let mut push = |top, left, bottom, right| remaining.push(CellRange {
        first: CellIndex { row: top, col: left }, last: CellIndex { row: bottom, col: right },
    });
    if first.row < at.row {
        push(first.row, first.col, at.row - 1, last.col);
    }
    if first.col < at.col {
        push(at.row, first.col, at.row, at.col - 1);
    }
    if last.col > at.col {
        push(at.row, at.col + 1, at.row, last.col);
    }
    if last.row > at.row {
        push(at.row + 1, first.col, last.row, last.col);
    }
    remaining
}

impl Worksheet<'_> {
    /// Removes one cell from the standard validation rules that cover it.
    pub fn remove_validation_at(&mut self, at: crate::CellIndex) -> Result<()> {
        use crate::formula::translate::translate;
        let at = crate::CellIndex::new(at.row, at.col)?;
        self.wb.check_editable()?;
        let part = self.part_name().to_owned();
        let doc = self.wb.pkg.xml_part(&part)?;
        for (_, ext) in x14::exts(doc.root(), x14::URI_DV) {
            if ext.elements().filter(|element| element.is(x14::DataValidations::QNAME))
                .flat_map(|rules| rules.elements().filter(|rule| rule.is(x14::DV_RULE)))
                .any(|rule| x14::sqref(rule).and_then(|refs| ranges(&refs).ok())
                    .is_some_and(|ranges| ranges.iter().any(|range| range.contains(at))))
            {
                return Err(Error::InvalidSetting {
                    what: "validation range".into(), reason: "cannot split an x14 validation".into(),
                });
            }
        }
        let sheet = doc.lens::<sml::Worksheet>()?;
        let Some(rules) = sheet.child::<sml::DataValidations>() else { return Ok(()) };
        let mut edits = Vec::new();
        for (index, rule) in rules.raw().indexed_elements().filter(|(_, rule)| rule.is(sml::DataValidation::QNAME)) {
            let Some(old) = rule.attr(q("sqref")).and_then(|refs| ranges(refs).ok()) else { continue };
            if !old.iter().any(|range| range.contains(at)) {
                continue;
            }
            let remaining: Vec<_> = old.iter().flat_map(|range| validation_range_without(*range, at)).collect();
            let (mut first, mut second) = (None, None);
            if let Some(new) = remaining.first().filter(|new| new.first != old[0].first) {
                let dr = i64::from(new.first.row) - i64::from(old[0].first.row);
                let dc = i64::from(new.first.col) - i64::from(old[0].first.col);
                let shift = |tag| -> Result<Option<String>> {
                    rule.elements().find(|node| node.is(tag)).map(|node| {
                        translate(&node.text(), dr, dc, &|name| self.wb.env.is_name(name)).map_err(|error| Error::InvalidSetting {
                            what: "validation formula".into(), reason: error.to_string(),
                        })
                    }).transpose()
                };
                first = shift(sml::Formula1::QNAME)?;
                second = shift(sml::Formula2::QNAME)?;
            }
            let reference = (!remaining.is_empty()).then(|| remaining.iter().map(ToString::to_string).collect::<Vec<_>>().join(" "));
            edits.push((index, reference, first, second));
        }
        if edits.is_empty() {
            return Ok(());
        }
        self.wb.poisoning(|wb| {
            let doc = wb.pkg.xml_part_mut(&part)?;
            let mut sheet = doc.lens_mut::<sml::Worksheet>()?;
            let mut rules = sheet.child_mut::<sml::DataValidations>().expect("planned");
            for (index, reference, first, second) in edits.into_iter().rev() {
                if let Some(reference) = reference {
                    let mut rule = x14::child_at::<_, sml::DataValidation>(&mut rules, index);
                    rule.set_attr(q("sqref"), &reference)?;
                    if let Some(formula) = first {
                        rule.child_mut::<sml::Formula1>().expect("planned").set_text(&formula)?;
                    }
                    if let Some(formula) = second {
                        rule.child_mut::<sml::Formula2>().expect("planned").set_text(&formula)?;
                    }
                } else {
                    rules.remove_at(index);
                }
            }
            let count = rules.node().elements().filter(|node| node.is(sml::DataValidation::QNAME)).count();
            if count == 0 {
                sheet.remove::<sml::DataValidations>();
            } else {
                rules.set_attr(q("count"), &count.to_string())?;
            }
            Ok(())
        })
    }

    /// Reads the standard validation rule covering a cell.
    pub fn validation_attributes(&self, at: crate::CellIndex) -> Result<Vec<(String, String)>> {
        self.find_validation_attributes(|ranges| ranges.iter().any(|range| range.contains(at)))
    }

    pub(crate) fn validation_overlaps(&self, affected: CellRange) -> bool {
        let doc = self.wb.pkg.parsed_part(self.part_name()).expect("worksheet is parsed");
        validation_references(doc.root()).any(|reference| {
            !ranges(&reference).is_ok_and(|rule| !overlaps(&rule, &[affected]))
        })
    }

    /// Reads the standard validation with the specified range list.
    pub fn validation_attributes_for(&self, reference: &str) -> Result<Vec<(String, String)>> {
        let expected = ranges(reference)?;
        self.find_validation_attributes(|ranges| ranges == expected)
    }

    fn find_validation_attributes(&self, matches: impl Fn(&[CellRange]) -> bool) -> Result<Vec<(String, String)>> {
        let doc = self.wb.pkg.parsed_part(self.part_name()).expect("worksheet is parsed");
        let sheet = doc.lens::<sml::Worksheet>()?;
        let Some(rules) = sheet.child::<sml::DataValidations>() else { return Ok(Vec::new()) };
        let Some(rule) = rules.raw().elements().find(|rule| {
            rule.is(sml::DataValidation::QNAME) && rule.attr(q("sqref"))
                .and_then(|reference| ranges(reference).ok())
                .is_some_and(|ranges| matches(&ranges))
        }) else { return Ok(Vec::new()) };
        Ok(rule_attributes(rule))
    }

    /// Lists standard validation rules in worksheet order, omitting invalid references.
    pub fn validation_rules(&self) -> Result<Vec<ValidationRuleAttributes>> {
        let doc = self.wb.pkg.parsed_part(self.part_name()).expect("worksheet is parsed");
        let sheet = doc.lens::<sml::Worksheet>()?;
        let Some(rules) = sheet.child::<sml::DataValidations>() else { return Ok(Vec::new()) };
        Ok(rules.raw().elements().filter(|node| node.is(sml::DataValidation::QNAME))
            .filter_map(|rule| {
                let reference = rule.attr(q("sqref"))?;
                ranges(reference).ok()?;
                Some(ValidationRuleAttributes {
                    reference: reference.to_owned(),
                    attributes: rule_attributes(rule),
                })
            }).collect())
    }

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
