//! Phase4 part 2: conditional formatting (ADR-0014 §6), 2007 forms.
//!
//! Register a format with [`Workbook::add_differential_format`] and pass its
//! index to [`Rule::with_dxf`]. Colour scales, data bars and icon sets need no
//! differential format.

use ooxml_core::{ChildOf, ElementSpec, EnumType, LensMut, NsDecl, QName, RawNode, RegistryPlan};
use ooxml_schema::sml;
pub use ooxml_schema::sml::IconSetValues;

use crate::error::{Error, Result};
use crate::reference::CellRange;
use crate::workbook::Workbook;
use crate::worksheet::Worksheet;
use crate::x14;

const REL_STYLES: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles";
/// The `ext/@uri` inside a 2007 `cfRule` that holds its `x14:id`.
const URI_CF_ID: &str = "{B025F937-C7B1-47D3-B67F-A62EFF666E3E}";

const fn q(local: &'static str) -> QName {
    QName::new("", local)
}

/// A `cellIs` comparison.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CfOperator {
    /// Between (inclusive).
    Between,
    /// Not between.
    NotBetween,
    /// Equal.
    Equal,
    /// Not equal.
    NotEqual,
    /// Greater than.
    GreaterThan,
    /// Less than.
    LessThan,
    /// Greater than or equal.
    GreaterThanOrEqual,
    /// Less than or equal.
    LessThanOrEqual,
}

/// A `cfvo` threshold (colour scales, data bars, icon sets).
#[derive(Clone, Debug, PartialEq)]
pub enum Cfvo {
    /// The lowest value.
    Min,
    /// The highest value.
    Max,
    /// A number.
    Num(f64),
    /// A percentage of the range.
    Percent(f64),
    /// A percentile.
    Percentile(f64),
    /// A formula (no other sheet: [`Error::CrossSheetReference`]).
    Formula(String),
}

/// One standalone x14 conditional formatting icon rule.
pub struct ExtendedIconRule<'a> {
    /// A stored rule GUID, or None to generate one.
    pub id: Option<&'a str>,
    /// Explicit priority when present.
    pub priority: Option<u32>,
    /// Lower bound for automatically assigned priorities.
    pub priority_floor: u32,
    /// The x14 icon set name.
    pub name: &'a str,
    /// Threshold values and optional equality flags.
    pub thresholds: &'a [(Cfvo, Option<bool>)],
    /// Custom icon selections, one per threshold, or an empty slice.
    pub icons: &'a [(String, u32)],
    /// Whether to reverse the displayed icons.
    pub reverse: bool,
    /// Whether to show the cell value.
    pub show_value: bool,
}

/// Threshold used by an extended data bar.
pub enum ExtendedDataBarThreshold {
    /// A standard threshold.
    Value(Cfvo),
    /// The automatic minimum.
    AutoMin,
    /// The automatic maximum.
    AutoMax,
}

/// The x14 settings that accompany a standard data-bar rule.
pub struct ExtendedDataBar<'a> {
    /// A stored rule GUID, or None to generate a link ID.
    pub id: Option<&'a str>,
    /// The extended thresholds.
    pub thresholds: &'a [ExtendedDataBarThreshold; 2],
    /// Minimum and maximum bar lengths. `None` leaves the attribute out
    /// (the x14 schema defaults, 10 and 90, then apply and are what the
    /// range check uses).
    pub lengths: (Option<u32>, Option<u32>),
    /// Border, gradient, same negative fill, and same negative border;
    /// `None` leaves the attribute out (schema defaults off, on, on, on).
    pub flags: [Option<bool>; 4],
    /// Whether the cell value is shown in a standalone rule.
    pub show_value: Option<bool>,
    /// Axis placement (`auto`, `middle`, `none`); `None` leaves it out.
    pub axis: Option<&'a str>,
    /// Bar direction (`leftToRight`, `rightToLeft`, `context`); `None`
    /// leaves it out.
    pub direction: Option<&'a str>,
    /// Fill, border, negative fill, negative border, and axis colors.
    pub colors: [Option<crate::Color>; 5],
}

/// What a rule tests and shows (2007 forms; ADR-0014 §6).
#[derive(Clone, Debug, PartialEq)]
pub enum RuleKind {
    /// `cellIs`: the cell compared with one or two formulas.
    CellIs(CfOperator, String, Option<String>),
    /// `expression`: a formula that must be true (relative to the top-left
    /// cell of the first range).
    Expression(String),
    /// The cell text contains the string.
    ContainsText(String),
    /// The cell text does not contain the string.
    NotContainsText(String),
    /// The cell text begins with the string.
    BeginsWith(String),
    /// The cell text ends with the string.
    EndsWith(String),
    /// Cells whose contents are blank or nonblank.
    ContainsBlanks(bool),
    /// Cells whose value is an error or is not an error.
    ContainsErrors(bool),
    /// Date relative to the current day, week, or month.
    TimePeriod(sml::TimePeriodValues, Option<String>),
    /// Top or bottom `rank` values (or percent).
    Top10 {
        /// How many (or what percentage).
        rank: u32,
        /// `rank` is a percentage.
        percent: bool,
        /// Bottom instead of top.
        bottom: bool,
    },
    /// Above (or below) the average.
    AboveAverage(bool),
    /// Duplicate values.
    DuplicateValues,
    /// Unique values.
    UniqueValues,
    /// A two- or three-colour scale: thresholds and ARGB colours
    /// (`"FFF8696B"`), the same number of each.
    ColorScale(Vec<(Cfvo, String)>),
    /// A data bar between two thresholds, in an ARGB colour.
    DataBar(Cfvo, Cfvo, String),
    /// A 2007 icon set with one threshold per icon.
    IconSet(IconSetValues, Vec<Cfvo>),
}

/// A conditional-format rule.
#[derive(Clone, Debug, PartialEq)]
pub struct Rule {
    kind: RuleKind,
    dxf: Option<u32>,
    stop_if_true: bool,
    priority: Option<u32>,
    priority_floor: u32,
    reverse_icons: bool,
    show_values: bool,
    data_bar_lengths: (Option<u32>, Option<u32>),
    data_bar_show_value: Option<bool>,
    data_bar_color: Option<crate::Color>,
    scale_colors: Vec<crate::Color>,
    text_predicate: Option<TextPredicateModel>,
    equal_average: Option<bool>,
    std_dev: Option<u32>,
    icon_gte: Vec<Option<bool>>,
}

#[derive(Clone, Debug, PartialEq)]
struct TextPredicateModel {
    formula: String,
    omit_text: bool,
    omit_operator: bool,
}

impl Rule {
    /// A rule without a format (colour scales, data bars and icon sets need
    /// none).
    pub fn new(kind: RuleKind) -> Self {
        Rule { kind, dxf: None, stop_if_true: false, priority: None, priority_floor: 0, reverse_icons: false, show_values: true, data_bar_lengths: (None, None), data_bar_show_value: None, data_bar_color: None, scale_colors: Vec::new(), text_predicate: None, equal_average: None, std_dev: None, icon_gte: Vec::new() }
    }

    /// Builds an average rule with optional equality and standard deviation settings.
    pub fn above_average(above: bool, equal: Option<bool>, std_dev: Option<u32>) -> Result<Self> {
        if std_dev.is_some_and(|value| value > i32::MAX as u32) {
            return Err(Error::InvalidSetting {
                what: "conditional formatting standard deviation".into(),
                reason: "expected a nonnegative signed 32-bit count".into(),
            });
        }
        let mut rule = Self::new(RuleKind::AboveAverage(above));
        rule.equal_average = equal;
        rule.std_dev = std_dev;
        Ok(rule)
    }

    /// Supplies the stored formula of a text, blank, or error predicate.
    pub fn text_formula(mut self, formula: String, omit_text: bool, omit_operator: bool) -> Result<Self> {
        if !matches!(self.kind, RuleKind::ContainsText(_) | RuleKind::NotContainsText(_) |
            RuleKind::BeginsWith(_) | RuleKind::EndsWith(_) | RuleKind::ContainsBlanks(_) | RuleKind::ContainsErrors(_))
        {
            return Err(Error::InvalidSetting {
                what: "conditional formatting formula".into(),
                reason: "expected a text, blank, or error predicate".into(),
            });
        }
        self.text_predicate = Some(TextPredicateModel { formula, omit_text, omit_operator });
        Ok(self)
    }

    /// Builds a time-period rule, optionally with a supplied formula.
    pub fn time_period(period: &str, formula: Option<String>) -> Result<Self> {
        let period = sml::TimePeriodValues::parse(period).ok_or_else(|| Error::InvalidSetting {
            what: "time period".into(),
            reason: format!("unsupported time period {period:?}"),
        })?;
        Ok(Self::new(RuleKind::TimePeriod(period, formula)))
    }

    /// Builds an icon-set rule from its OOXML identifier.
    pub fn icon_set(name: &str, thresholds: Vec<Cfvo>, reverse: bool, show_value: bool) -> Result<Self> {
        let name = if name == "3TrafficLights" { "3TrafficLights1" } else { name };
        let icons = IconSetValues::parse(name).ok_or_else(|| Error::InvalidSetting {
            what: "icon set".into(),
            reason: format!("unsupported icon set {name:?}"),
        })?;
        let mut rule = Self::new(RuleKind::IconSet(icons, thresholds));
        rule.reverse_icons = reverse;
        rule.show_values = show_value;
        Ok(rule)
    }

    /// Specifies whether each icon threshold includes equality.
    pub fn icon_threshold_inclusion(mut self, inclusion: Vec<Option<bool>>) -> Result<Self> {
        if !matches!(&self.kind, RuleKind::IconSet(_, thresholds) if thresholds.len() == inclusion.len()) {
            return Err(Error::InvalidSetting {
                what: "conditional formatting icon thresholds".into(),
                reason: "expected one inclusion flag per threshold".into(),
            });
        }
        self.icon_gte = inclusion;
        Ok(self)
    }

    /// Builds a color scale using typed workbook colors.
    pub fn color_scale(stops: Vec<(Cfvo, crate::Color)>) -> Self {
        let (thresholds, colors) = stops.into_iter().map(|(value, color)| ((value, String::new()), color)).unzip();
        let mut rule = Self::new(RuleKind::ColorScale(thresholds));
        rule.scale_colors = colors;
        rule
    }

    /// Builds a data bar with explicit length limits and value visibility.
    pub fn data_bar(low: Cfvo, high: Cfvo, color: String, min: u32, max: u32, show_value: bool) -> Result<Self> {
        Self::new(RuleKind::DataBar(low, high, color)).data_bar_layout(min, max, show_value)
    }

    /// Builds a data bar using a typed workbook color.
    pub fn data_bar_with_color(low: Cfvo, high: Cfvo, color: crate::Color, min: u32, max: u32, show_value: bool) -> Result<Self> {
        let mut rule = Self::new(RuleKind::DataBar(low, high, String::new())).data_bar_layout(min, max, show_value)?;
        rule.data_bar_color = Some(color);
        Ok(rule)
    }

    /// Builds a data bar that writes only the declared `minLength`,
    /// `maxLength` and `showValue` attributes. Omitted limits are checked
    /// against the schema defaults (10 and 90) but are not written.
    pub fn data_bar_declared(
        low: Cfvo, high: Cfvo, color: crate::Color, min: Option<u32>, max: Option<u32>, show_value: Option<bool>,
    ) -> Result<Self> {
        let mut rule = Self::new(RuleKind::DataBar(low, high, String::new()));
        rule.data_bar_layout_declared(min, max, show_value)?;
        rule.data_bar_color = Some(color);
        Ok(rule)
    }

    fn data_bar_layout(mut self, min: u32, max: u32, show_value: bool) -> Result<Self> {
        self.data_bar_layout_declared(Some(min), Some(max), (!show_value).then_some(false))?;
        Ok(self)
    }

    fn data_bar_layout_declared(&mut self, min: Option<u32>, max: Option<u32>, show_value: Option<bool>) -> Result<()> {
        let (low, high) = (min.unwrap_or(10), max.unwrap_or(90));
        if low > high || high > 100 {
            return Err(Error::InvalidSetting {
                what: "data bar".into(),
                reason: "expected ordered length limits from 0 to 100".into(),
            });
        }
        self.data_bar_lengths = (min, max);
        self.data_bar_show_value = show_value;
        Ok(())
    }

    /// The format: an existing `dxfs` entry of `styles.xml` (by index).
    pub fn with_dxf(mut self, dxf_id: u32) -> Self {
        self.dxf = Some(dxf_id);
        self
    }

    /// Requests an explicit priority or reserves priorities through `floor` for auto-assignment.
    pub fn with_priority(mut self, priority: Option<u32>, floor: u32) -> Self {
        self.priority = priority;
        self.priority_floor = floor;
        self
    }

    /// `stopIfTrue`.
    pub fn stop_if_true(mut self, on: bool) -> Self {
        self.stop_if_true = on;
        self
    }
}

/// What `remove_conditional_format` did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct CfRemoval {
    /// Rules removed.
    pub removed: usize,
    /// A removed rule named an `x14:id` with no x14 rule: the pair was
    /// already broken (ADR-0014 `RemovedWithBrokenX14Link`).
    pub broken_x14_link: bool,
}

fn ranges(sqref: &str) -> Result<Vec<CellRange>> {
    let v: Vec<CellRange> = sqref.split_ascii_whitespace().map(CellRange::parse).collect::<Result<_>>()?;
    if v.is_empty() {
        return Err(Error::InvalidReference(sqref.to_owned()));
    }
    Ok(v)
}

fn num(x: f64) -> String {
    let s = format!("{x}");
    s.strip_suffix(".0").map_or(s.clone(), str::to_owned)
}

fn argb_ok(s: &str) -> bool {
    s.len() == 8 && s.chars().all(|c| c.is_ascii_hexdigit())
}

fn rule_priority(rule: &RawNode) -> Option<u32> {
    rule.attr(q("priority"))?.trim().parse().ok()
}

/// Priorities and pivot blocks are inspected before any renumbering.
fn priorities(ws: &RawNode) -> (Vec<Option<u32>>, bool) {
    let mut out = Vec::new();
    let mut pivot = false;
    let mut linked_ids = Vec::new();
    for b in ws.elements().filter(|e| e.is(sml::ConditionalFormatting::QNAME)) {
        pivot |= matches!(b.attr(q("pivot")), Some("1" | "true"));
        for rule in b.elements().filter(|r| r.is(sml::ConditionalFormattingRule::QNAME)) {
            out.push(rule_priority(rule));
            linked_ids.extend(x14_id(rule));
        }
    }
    for (_, ext) in x14::exts(ws, x14::URI_CF) {
        for cfs in ext.elements().filter(|e| e.is(x14::ConditionalFormattings::QNAME)) {
            for cf in cfs.elements().filter(|e| e.is(x14::ConditionalFormatting::QNAME)) {
                for rule in cf.elements().filter(|r| r.is(x14::CfRule::QNAME)) {
                    // Linked extensions can inherit the priority of their 2007 rule.
                    if rule.attr(q("priority")).is_none()
                        && rule.attr(q("id")).is_some_and(|id| linked_ids.iter().any(|linked| linked.eq_ignore_ascii_case(id)))
                    {
                        continue;
                    }
                    out.push(rule_priority(rule));
                }
            }
        }
    }
    (out, pivot)
}

impl Workbook {
    /// Reads one standard differential style by its registry index.
    pub fn differential_format_node(&mut self, id: usize) -> Result<Option<RawNode>> {
        let rels = self.pkg.relationships(&self.workbook_part)?;
        let Some(part) = rels.by_type(REL_STYLES).next().and_then(|r| rels.resolve(r)) else {
            return Ok(None);
        };
        let document = self.pkg.xml_part(&part)?;
        Ok(document.root().elements().find(|node| node.is(sml::DifferentialFormats::QNAME))
            .and_then(|table| table.elements().filter(|node| node.is(sml::DifferentialFormat::QNAME)).nth(id))
            .cloned())
    }

    /// Reads the requested differential formats in style-table order.
    pub fn differential_format_nodes(&mut self, ids: &[u32]) -> Result<Vec<(u32, RawNode)>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let rels = self.pkg.relationships(&self.workbook_part)?;
        let Some(part) = rels.by_type(REL_STYLES).next().and_then(|r| rels.resolve(r)) else {
            return Ok(Vec::new());
        };
        let document = self.pkg.xml_part(&part)?;
        let Some(table) = document.root().elements().find(|node| node.is(sml::DifferentialFormats::QNAME)) else {
            return Ok(Vec::new());
        };
        let mut remaining: std::collections::HashSet<_> = ids.iter().copied().collect();
        let mut formats = Vec::new();
        for (id, node) in table.elements()
            .filter(|node| node.is(sml::DifferentialFormat::QNAME))
            .enumerate()
        {
            let id = id as u32;
            if remaining.remove(&id) {
                formats.push((id, node.clone()));
                if remaining.is_empty() {
                    break;
                }
            }
        }
        Ok(formats)
    }

    pub(crate) fn dxf_count(&mut self) -> Result<u32> {
        let rels = self.pkg.relationships(&self.workbook_part)?;
        let Some(p) = rels.by_type(REL_STYLES).next().and_then(|r| rels.resolve(r)) else { return Ok(0) };
        let doc = self.pkg.xml_part(&p)?;
        Ok(doc.root().elements().find(|e| e.is(sml::DifferentialFormats::QNAME)).map_or(0, |d| d.elements().filter(|e| e.is(sml::DifferentialFormat::QNAME)).count() as u32))
    }

    /// The rule's formulas, checked (relative to the first range's
    /// top-left cell).
    fn rule_formulas(&self, sheet: usize, rule: &Rule, top_left: &str) -> Result<Vec<String>> {
        let text = |t: &str| -> Result<String> {
            if let Some((offset, ch)) = ooxml_core::value::first_invalid_xml_char(t) {
                return Err(ooxml_core::Error::InvalidXmlChar { ch, offset }.into());
            }
            Ok(t.replace('"', "\"\""))
        };
        Ok(match &rule.kind {
            RuleKind::CellIs(op, f1, f2) => {
                let between = matches!(op, CfOperator::Between | CfOperator::NotBetween);
                crate::rangefmla::check_second_formula(between, f2.is_some())?;
                let mut v = vec![self.check_range_formula(sheet, f1)?];
                if let Some(f2) = f2 {
                    v.push(self.check_range_formula(sheet, f2)?);
                }
                v
            }
            RuleKind::Expression(f) => vec![self.check_range_formula(sheet, f)?],
            RuleKind::ContainsText(t) => vec![format!("NOT(ISERROR(SEARCH(\"{}\",{top_left})))", text(t)?)],
            RuleKind::NotContainsText(t) => vec![format!("ISERROR(SEARCH(\"{}\",{top_left}))", text(t)?)],
            RuleKind::BeginsWith(t) => {
                let e = text(t)?;
                vec![format!("LEFT({top_left},LEN(\"{e}\"))=\"{e}\"")]
            }
            RuleKind::EndsWith(t) => {
                let e = text(t)?;
                vec![format!("RIGHT({top_left},LEN(\"{e}\"))=\"{e}\"")]
            }
            RuleKind::ContainsBlanks(true) => vec![format!("LEN(TRIM({top_left}))=0")],
            RuleKind::ContainsBlanks(false) => vec![format!("LEN(TRIM({top_left}))>0")],
            RuleKind::ContainsErrors(true) => vec![format!("ISERROR({top_left})")],
            RuleKind::ContainsErrors(false) => vec![format!("NOT(ISERROR({top_left}))")],
            RuleKind::TimePeriod(_, Some(formula)) => vec![self.check_range_formula(sheet, formula)?],
            RuleKind::TimePeriod(period, None) => vec![time_period_formula(*period, top_left)],
            RuleKind::ColorScale(stops) => {
                if !(2..=3).contains(&stops.len()) || (rule.scale_colors.is_empty() && stops.iter().any(|(_, c)| !argb_ok(c))) {
                    return Err(Error::InvalidSetting { what: "colour scale".into(), reason: "2 or 3 stops with ARGB colours".into() });
                }
                self.check_thresholds(sheet, stops.iter().map(|(c, _)| c))?;
                Vec::new()
            }
            RuleKind::DataBar(_, _, c) if rule.data_bar_color.is_none() && !argb_ok(c) => {
                return Err(Error::InvalidSetting { what: "data bar".into(), reason: format!("{c:?} is not ARGB") });
            }
            RuleKind::DataBar(a, b, _) => {
                self.check_thresholds(sheet, [a, b].into_iter())?;
                Vec::new()
            }
            RuleKind::IconSet(set, cfvos) => {
                // ST_IconSetType spells each set with its icon count first.
                let count = usize::from(set.as_str().as_bytes()[0] - b'0');
                if cfvos.len() != count {
                    return Err(Error::InvalidSetting {
                        what: "icon set".into(),
                        reason: format!("{} requires {count} thresholds", set.as_str()),
                    });
                }
                self.check_thresholds(sheet, cfvos.iter())?;
                Vec::new()
            }
            RuleKind::Top10 { rank, percent, .. } if *rank == 0 || (*percent && *rank > 100) => {
                return Err(Error::InvalidSetting {
                    what: "top10 rank".into(),
                    reason: "must be positive and at most 100 for a percentage".into(),
                });
            }
            _ => Vec::new(),
        })
    }

    fn check_thresholds<'a>(&self, sheet: usize, cfvos: impl Iterator<Item = &'a Cfvo>) -> Result<()> {
        for c in cfvos {
            let reason = match c {
                Cfvo::Formula(f) => {
                    self.check_range_formula(sheet, f)?;
                    continue;
                }
                Cfvo::Num(n) if !n.is_finite() => "number must be finite",
                Cfvo::Percent(n) | Cfvo::Percentile(n) if !(0.0..=100.0).contains(n) => {
                    "percentage and percentile must be between 0 and 100"
                }
                _ => continue,
            };
            return Err(Error::InvalidSetting { what: "conditional formatting threshold".into(), reason: reason.into() });
        }
        Ok(())
    }

    fn add_conditional_format(&mut self, sheet: usize, sqref: &str, rule: &Rule, first: bool, append: bool) -> Result<()> {
        self.check_editable()?;
        let rs = ranges(sqref)?;
        let sqref = rs.iter().map(ToString::to_string).collect::<Vec<_>>().join(" ");
        let top_left = rs[0].first.to_string();
        let mut formulas = self.rule_formulas(sheet, rule, &top_left)?;
        if let Some(model) = &rule.text_predicate {
            formulas = vec![self.check_range_formula(sheet, &model.formula)?];
        }
        if let Some(d) = rule.dxf
            && d >= self.dxf_count()?
        {
            return Err(Error::InvalidSetting { what: "dxfId".into(), reason: format!("{d} is not an existing dxf") });
        }
        let part = self.sheets[sheet].part.clone().expect("worksheet");
        let root = self.pkg.xml_part(&part)?.root();
        let (prios, pivot) = priorities(root);
        let previous_block = if append {
            let (index, block) = root.indexed_elements()
                .filter(|(_, node)| node.is(sml::ConditionalFormatting::QNAME))
                .last().ok_or_else(|| Error::NoSuchRule { what: "conditional format", sqref: sqref.clone() })?;
            if block.attr(q("sqref")) != Some(sqref.as_str()) || matches!(block.attr(q("pivot")), Some("1" | "true")) {
                return Err(Error::InvalidSetting {
                    what: "conditional formatting range".into(),
                    reason: "expected the last standard block to use the same range".into(),
                });
            }
            Some(index)
        } else {
            None
        };
        if first && pivot {
            return Err(Error::PivotConditionalFormat(sqref));
        }
        if first && prios.iter().any(|p| p.is_none_or(|p| p == 0)) {
            return Err(Error::InvalidSetting {
                what: "conditional formatting priority".into(),
                reason: "cannot insert first when an existing rule lacks a positive integer priority".into(),
            });
        }
        let max_priority = prios.iter().copied().flatten().max().unwrap_or(0);
        let priority = if let Some(priority) = rule.priority.filter(|_| !first) {
            if priority == 0 || priority > i32::MAX as u32 || prios.contains(&Some(priority)) {
                return Err(Error::InvalidSetting {
                    what: "conditional formatting priority".into(),
                    reason: "expected a unique positive signed integer priority".into(),
                });
            }
            priority
        } else {
            let max_priority = max_priority.max(rule.priority_floor);
            if max_priority >= i32::MAX as u32 {
                return Err(Error::InvalidSetting {
                    what: "conditional formatting priority".into(),
                    reason: "no higher priority value fits in the schema's signed integer".into(),
                });
            }
            if first { 1 } else { max_priority + 1 }
        };
        {
            let doc = self.pkg.xml_part_mut(&part)?;
            let mut ws = doc.lens_mut::<sml::Worksheet>()?;
            if let Some(index) = previous_block {
                x14::child_at::<_, sml::ConditionalFormatting>(&mut ws, index)
                    .check_insert::<sml::ConditionalFormattingRule>()?;
            } else {
                ws.check_insert::<sml::ConditionalFormatting>()?;
            }
        }
        let rule = rule.clone();
        self.poisoning(|wb| {
            let doc = wb.pkg.xml_part_mut(&part)?;
            let mut ws = doc.lens_mut::<sml::Worksheet>()?;
            if first {
                renumber(&mut ws)?;
            }
            let mut block = if let Some(index) = previous_block {
                x14::child_at::<_, sml::ConditionalFormatting>(&mut ws, index)
            } else {
                let mut block = ws.insert::<sml::ConditionalFormatting>()?;
                block.set_attr(q("sqref"), &sqref)?;
                block
            };
            let mut r = block.insert::<sml::ConditionalFormattingRule>()?;
            write_rule(&mut r, &rule, priority, &formulas)
        })
    }

    fn remove_conditional_format(&mut self, sheet: usize, sqref: &str, priority: Option<u32>) -> Result<CfRemoval> {
        let want = ranges(sqref)?;
        self.remove_conditional_formats_matching(sheet, sqref, priority, |_, block| {
            block.attr(q("sqref")).and_then(|value| ranges(value).ok()).as_ref() == Some(&want)
        })
    }

    fn remove_conditional_formats_matching(
        &mut self,
        sheet: usize,
        sqref: &str,
        priority: Option<u32>,
        select: impl Fn(usize, &RawNode) -> bool,
    ) -> Result<CfRemoval> {
        self.check_editable()?;
        let part = self.sheets[sheet].part.clone().expect("worksheet");
        // (block item, rule items, block emptied), x14 ids
        let mut plan: Vec<(usize, Vec<usize>, bool)> = Vec::new();
        let mut ids: Vec<String> = Vec::new();
        {
            let doc = self.pkg.xml_part(&part)?;
            let blocks = doc.root().indexed_elements().filter(|(_, e)| e.is(sml::ConditionalFormatting::QNAME));
            for (ordinal, (bi, b)) in blocks.enumerate() {
                if !select(ordinal, b) {
                    continue;
                }
                let rules: Vec<(usize, &RawNode)> = b
                    .indexed_elements()
                    .filter(|(_, r)| r.is(sml::ConditionalFormattingRule::QNAME))
                    .filter(|(_, r)| priority.is_none_or(|p| rule_priority(r) == Some(p)))
                    .collect();
                if rules.is_empty() {
                    continue;
                }
                if matches!(b.attr(q("pivot")), Some("1" | "true")) {
                    return Err(Error::PivotConditionalFormat(sqref.to_owned()));
                }
                let total = b.elements().filter(|r| r.is(sml::ConditionalFormattingRule::QNAME)).count();
                for (_, r) in &rules {
                    ids.extend(x14_id(r));
                }
                plan.push((bi, rules.iter().map(|(i, _)| *i).collect(), rules.len() == total));
            }
        }
        if plan.is_empty() {
            return Err(Error::NoSuchRule { what: "conditional format", sqref: sqref.to_owned() });
        }
        // the linked x14 rules: (ext item, cfs item, cf item, rule item)
        let mut linked: Vec<(usize, usize, usize, usize)> = Vec::new();
        let mut found = 0;
        {
            let doc = self.pkg.xml_part(&part)?;
            for (ei, ext) in x14::exts(doc.root(), x14::URI_CF) {
                for (si, cfs) in ext.indexed_elements().filter(|(_, e)| e.is(x14::ConditionalFormattings::QNAME)) {
                    for (ci, cf) in cfs.indexed_elements().filter(|(_, e)| e.is(x14::ConditionalFormatting::QNAME)) {
                        for (ri, r) in cf.indexed_elements().filter(|(_, e)| e.is(x14::CfRule::QNAME)) {
                            if r.attr(q("id")).is_some_and(|id| ids.iter().any(|x| x.eq_ignore_ascii_case(id))) {
                                linked.push((ei, si, ci, ri));
                                found += 1;
                            }
                        }
                    }
                }
            }
        }
        let broken = found < ids.len();
        let removed = plan.iter().map(|p| p.1.len()).sum();
        self.poisoning(|wb| {
            let doc = wb.pkg.xml_part_mut(&part)?;
            let mut ws = doc.lens_mut::<sml::Worksheet>()?;
            // x14 side first (higher item indices in the worksheet: extLst is last)
            for &(ei, si, ci, ri) in linked.iter().rev() {
                remove_x14_rule(&mut ws, ei, si, ci, ri)?;
            }
            for (bi, rules, emptied) in plan.iter().rev() {
                if *emptied {
                    ws.remove_at(*bi);
                } else {
                    let mut b = x14::child_at::<_, sml::ConditionalFormatting>(&mut ws, *bi);
                    for &i in rules.iter().rev() {
                        b.remove_at(i);
                    }
                }
            }
            Ok(())
        })?;
        Ok(CfRemoval { removed, broken_x14_link: broken })
    }
}

/// The `x14:id` a 2007 rule carries, if any.
pub(crate) fn x14_id(r: &RawNode) -> Option<String> {
    r.elements()
        .filter(|e| e.name().local() == "extLst")
        .flat_map(|l| l.elements())
        .filter(|e| e.name().local() == "ext" && e.attr(q("uri")).is_some_and(|u| u.eq_ignore_ascii_case(URI_CF_ID)))
        .flat_map(|e| e.elements())
        .find(|e| e.is(QName::new(x14::NS_X14, "id")))
        .map(|e| e.text().trim().to_owned())
}

/// Removes one x14 rule, then its `x14:conditionalFormatting`, the
/// `x14:conditionalFormattings` and the `ext` as each becomes empty.
fn remove_x14_rule(ws: &mut LensMut<'_, sml::Worksheet>, ei: usize, si: usize, ci: usize, ri: usize) -> Result<()> {
    let mut list = ws.child_mut::<sml::WorksheetExtensionList>().expect("planned");
    let ext_empty;
    {
        let mut ext = x14::child_at::<_, sml::WorksheetExtension>(&mut list, ei);
        let cfs_empty;
        {
            let mut cfs = x14::child_at::<_, x14::ConditionalFormattings>(&mut ext, si);
            let cf_empty;
            {
                let mut cf = x14::child_at::<_, x14::ConditionalFormatting>(&mut cfs, ci);
                cf.remove_at(ri);
                cf_empty = !cf.node().elements().any(|e| e.is(x14::CfRule::QNAME));
            }
            if cf_empty {
                cfs.remove_at(ci);
            }
            cfs_empty = cfs.node().elements().next().is_none();
        }
        if cfs_empty {
            ext.remove_at(si);
        }
        ext_empty = ext.node().elements().next().is_none();
    }
    if ext_empty {
        list.remove_at(ei);
    }
    Ok(())
}

/// Adds 1 to every rule's priority, 2007 and x14 (for a new first rule).
fn renumber(ws: &mut LensMut<'_, sml::Worksheet>) -> Result<()> {
    let blocks: Vec<usize> = ws.node().indexed_elements().filter(|(_, e)| e.is(sml::ConditionalFormatting::QNAME)).map(|(i, _)| i).collect();
    for bi in blocks {
        let mut b = x14::child_at::<_, sml::ConditionalFormatting>(ws, bi);
        let rules: Vec<(usize, u32)> = b
            .node()
            .indexed_elements()
            .filter(|(_, r)| r.is(sml::ConditionalFormattingRule::QNAME))
            .filter_map(|(i, r)| rule_priority(r).map(|p| (i, p)))
            .collect();
        for (ri, p) in rules {
            x14::child_at::<_, sml::ConditionalFormattingRule>(&mut b, ri).set_attr(q("priority"), &(p + 1).to_string())?;
        }
    }
    let Some(mut list) = ws.child_mut::<sml::WorksheetExtensionList>() else { return Ok(()) };
    let exts: Vec<usize> = x14::exts_in_list(list.node(), x14::URI_CF);
    for ei in exts {
        let mut ext = x14::child_at::<_, sml::WorksheetExtension>(&mut list, ei);
        let cfss: Vec<usize> = ext.node().indexed_elements().filter(|(_, e)| e.is(x14::ConditionalFormattings::QNAME)).map(|(i, _)| i).collect();
        for si in cfss {
            let mut cfs = x14::child_at::<_, x14::ConditionalFormattings>(&mut ext, si);
            let cfl: Vec<usize> = cfs.node().indexed_elements().filter(|(_, e)| e.is(x14::ConditionalFormatting::QNAME)).map(|(i, _)| i).collect();
            for ci in cfl {
                let mut cf = x14::child_at::<_, x14::ConditionalFormatting>(&mut cfs, ci);
                let rules: Vec<(usize, u32)> = cf
                    .node()
                    .indexed_elements()
                    .filter(|(_, r)| r.is(x14::CfRule::QNAME))
                    .filter_map(|(i, r)| rule_priority(r).map(|p| (i, p)))
                    .collect();
                for (ri, p) in rules {
                    x14::child_at::<_, x14::CfRule>(&mut cf, ri).set_attr(q("priority"), &(p + 1).to_string())?;
                }
            }
        }
    }
    Ok(())
}

const EXTENDED_CF_NS: [NsDecl<'static>; 2] = [
    NsDecl { prefix: "x14", uri: x14::NS_X14 },
    NsDecl { prefix: "xm", uri: x14::NS_XM },
];

fn valid_extended_rule_id(value: &str) -> bool {
    let id = value.as_bytes();
    id.len() == 38 && id[0] == b'{' && id[37] == b'}' &&
        id[1..37].iter().enumerate().all(|(index, ch)|
            if matches!(index, 8 | 13 | 18 | 23) { *ch == b'-' } else { ch.is_ascii_hexdigit() })
}

fn extended_rule_exists(root: &RawNode, id: &str) -> bool {
    x14::exts(root, x14::URI_CF).iter().any(|(_, ext)|
        ext.elements().flat_map(RawNode::elements).flat_map(RawNode::elements)
            .any(|node| node.is(x14::CfRule::QNAME) && node.attr(q("id")).is_some_and(|existing| existing.eq_ignore_ascii_case(id))))
}

fn append_extended_block(
    sheet: &mut LensMut<'_, sml::Worksheet>,
    sqref: &str,
    write: impl FnOnce(&mut LensMut<'_, x14::CfRule>) -> Result<()>,
) -> Result<()> {
    let mut list = sheet.get_or_insert::<sml::WorksheetExtensionList>()?;
    let index = x14::exts_in_list(list.node(), x14::URI_CF).first().copied();
    let mut ext = if let Some(index) = index {
        x14::child_at::<_, sml::WorksheetExtension>(&mut list, index)
    } else {
        let mut ext = list.insert::<sml::WorksheetExtension>()?;
        ext.set_attr(q("uri"), x14::URI_CF)?;
        ext
    };
    let mut blocks = ext.get_or_insert_with_ns::<x14::ConditionalFormattings>(&EXTENDED_CF_NS)?;
    let mut block = blocks.insert::<x14::ConditionalFormatting>()?;
    write(&mut block.insert::<x14::CfRule>()?)?;
    block.insert_with_ns::<x14::Sqref>(&EXTENDED_CF_NS[1..])?.set_text(sqref)?;
    Ok(())
}

fn write_extended_color<C: ChildOf<x14::DataBar>>(bar: &mut LensMut<'_, x14::DataBar>, color: Option<crate::Color>) -> Result<()> {
    if let Some(color) = color {
        let mut node = bar.insert::<C>()?;
        for (name, value) in color.attrs() {
            node.set_attr(q(name), &value)?;
        }
    }
    Ok(())
}

fn new_extended_rule_id() -> Result<String> {
    let mut bytes = [0; 16];
    getrandom::fill(&mut bytes).map_err(|error| Error::Random(error.to_string()))?;
    bytes[6] = bytes[6] & 0x0f | 0x40;
    bytes[8] = bytes[8] & 0x3f | 0x80;
    let hex = format!("{:032X}", u128::from_be_bytes(bytes));
    Ok(format!("{{{}-{}-{}-{}-{}}}", &hex[..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..]))
}

fn extended_icon_count(name: &str) -> Option<usize> {
    match name {
        "3Stars" | "3Triangles" | "3TrafficLights" => Some(3),
        "5Boxes" => Some(5),
        _ => IconSetValues::parse(name).map(|set| usize::from(set.as_str().as_bytes()[0] - b'0')),
    }
}

fn threshold_value(c: &Cfvo) -> (&'static str, Option<String>) {
    match c {
        Cfvo::Min => ("min", None),
        Cfvo::Max => ("max", None),
        Cfvo::Num(x) => ("num", Some(num(*x))),
        Cfvo::Percent(x) => ("percent", Some(num(*x))),
        Cfvo::Percentile(x) => ("percentile", Some(num(*x))),
        Cfvo::Formula(f) => ("formula", Some(f.strip_prefix('=').unwrap_or(f).to_owned())),
    }
}

fn cfvo<P: ElementSpec>(parent: &mut LensMut<'_, P>, c: &Cfvo, gte: Option<bool>) -> Result<()>
where
    sml::ConditionalFormatValueObject: ChildOf<P>,
{
    let (ty, val) = threshold_value(c);
    let mut e = parent.insert::<sml::ConditionalFormatValueObject>()?;
    e.set_attr(q("type"), ty)?;
    if let Some(v) = val {
        e.set_attr(q("val"), &v)?;
    }
    if let Some(gte) = gte {
        e.set_attr(q("gte"), if gte { "1" } else { "0" })?;
    }
    Ok(())
}

fn time_period_formula(period: sml::TimePeriodValues, cell: &str) -> String {
    use sml::TimePeriodValues::*;

    let days = |start: &str, end: &str| format!("AND({cell}>={start},{cell}<{end})");
    match period {
        Yesterday => days("TODAY()-1", "TODAY()"),
        Today => days("TODAY()", "TODAY()+1"),
        Tomorrow => days("TODAY()+1", "TODAY()+2"),
        Last7Days => days("TODAY()-6", "TODAY()+1"),
        LastWeek => days("TODAY()-WEEKDAY(TODAY())-6", "TODAY()-WEEKDAY(TODAY())+1"),
        ThisWeek => days("TODAY()-WEEKDAY(TODAY())+1", "TODAY()-WEEKDAY(TODAY())+8"),
        NextWeek => days("TODAY()-WEEKDAY(TODAY())+8", "TODAY()-WEEKDAY(TODAY())+15"),
        LastMonth => days("EOMONTH(TODAY(),-2)+1", "EOMONTH(TODAY(),-1)+1"),
        ThisMonth => days("EOMONTH(TODAY(),-1)+1", "EOMONTH(TODAY(),0)+1"),
        NextMonth => days("EOMONTH(TODAY(),0)+1", "EOMONTH(TODAY(),1)+1"),
    }
}

fn write_visual_color(node: &mut LensMut<'_, sml::Color>, color: Option<crate::Color>, argb: &str) -> Result<()> {
    if let Some(color) = color {
        for (name, value) in color.attrs() {
            node.set_attr(q(name), &value)?;
        }
    } else {
        node.set_attr(q("rgb"), argb)?;
    }
    Ok(())
}

fn write_rule(r: &mut LensMut<'_, sml::ConditionalFormattingRule>, rule: &Rule, priority: u32, formulas: &[String]) -> Result<()> {
    let (ty, op, text): (&str, Option<&str>, Option<&str>) = match &rule.kind {
        RuleKind::CellIs(op, _, _) => ("cellIs", Some(match op {
            CfOperator::Between => "between",
            CfOperator::NotBetween => "notBetween",
            CfOperator::Equal => "equal",
            CfOperator::NotEqual => "notEqual",
            CfOperator::GreaterThan => "greaterThan",
            CfOperator::LessThan => "lessThan",
            CfOperator::GreaterThanOrEqual => "greaterThanOrEqual",
            CfOperator::LessThanOrEqual => "lessThanOrEqual",
        }), None),
        RuleKind::Expression(_) => ("expression", None, None),
        RuleKind::ContainsBlanks(true) => ("containsBlanks", None, None),
        RuleKind::ContainsBlanks(false) => ("notContainsBlanks", None, None),
        RuleKind::ContainsErrors(true) => ("containsErrors", None, None),
        RuleKind::ContainsErrors(false) => ("notContainsErrors", None, None),
        RuleKind::ContainsText(t) => ("containsText", Some("containsText"), Some(t.as_str())),
        RuleKind::NotContainsText(t) => ("notContainsText", Some("notContains"), Some(t.as_str())),
        RuleKind::BeginsWith(t) => ("beginsWith", Some("beginsWith"), Some(t.as_str())),
        RuleKind::EndsWith(t) => ("endsWith", Some("endsWith"), Some(t.as_str())),
        RuleKind::TimePeriod(..) => ("timePeriod", None, None),
        RuleKind::Top10 { .. } => ("top10", None, None),
        RuleKind::AboveAverage(_) => ("aboveAverage", None, None),
        RuleKind::DuplicateValues => ("duplicateValues", None, None),
        RuleKind::UniqueValues => ("uniqueValues", None, None),
        RuleKind::ColorScale(_) => ("colorScale", None, None),
        RuleKind::DataBar(..) => ("dataBar", None, None),
        RuleKind::IconSet(..) => ("iconSet", None, None),
    };
    r.set_attr(q("type"), ty)?;
    if let Some(d) = rule.dxf {
        r.set_attr(q("dxfId"), &d.to_string())?;
    }
    r.set_attr(q("priority"), &priority.to_string())?;
    if rule.stop_if_true {
        r.set_attr(q("stopIfTrue"), "1")?;
    }
    match &rule.kind {
        RuleKind::TimePeriod(period, _) => r.set_attr(q("timePeriod"), period.as_str())?,
        RuleKind::AboveAverage(above) => {
            if !above {
                r.set_attr(q("aboveAverage"), "0")?;
            }
            if let Some(equal) = rule.equal_average {
                r.set_attr(q("equalAverage"), if equal { "1" } else { "0" })?;
            }
            if let Some(std_dev) = rule.std_dev {
                r.set_attr(q("stdDev"), &std_dev.to_string())?;
            }
        }
        RuleKind::Top10 { rank, percent, bottom } => {
            if *percent {
                r.set_attr(q("percent"), "1")?;
            }
            if *bottom {
                r.set_attr(q("bottom"), "1")?;
            }
            r.set_attr(q("rank"), &rank.to_string())?;
        }
        _ => {}
    }
    if let Some(op) = op && !rule.text_predicate.as_ref().is_some_and(|model| model.omit_operator) {
        r.set_attr(q("operator"), op)?;
    }
    if let Some(t) = text && !rule.text_predicate.as_ref().is_some_and(|model| model.omit_text) {
        r.set_attr(q("text"), t)?;
    }
    for f in formulas {
        r.insert::<sml::Formula>()?.set_text(f)?;
    }
    match &rule.kind {
        RuleKind::ColorScale(stops) => {
            let mut cs = r.insert::<sml::ColorScale>()?;
            for (c, _) in stops {
                cfvo(&mut cs, c, None)?;
            }
            for (index, (_, argb)) in stops.iter().enumerate() {
                write_visual_color(&mut cs.insert::<sml::Color>()?, rule.scale_colors.get(index).copied(), argb)?;
            }
        }
        RuleKind::DataBar(a, b, argb) => {
            let mut db = r.insert::<sml::DataBar>()?;
            if let Some(min) = rule.data_bar_lengths.0 {
                db.set_attr(q("minLength"), &min.to_string())?;
            }
            if let Some(max) = rule.data_bar_lengths.1 {
                db.set_attr(q("maxLength"), &max.to_string())?;
            }
            if let Some(show) = rule.data_bar_show_value {
                db.set_attr(q("showValue"), if show { "1" } else { "0" })?;
            }
            cfvo(&mut db, a, None)?;
            cfvo(&mut db, b, None)?;
            write_visual_color(&mut db.insert::<sml::Color>()?, rule.data_bar_color, argb)?;
        }
        RuleKind::IconSet(set, cfvos) => {
            let mut is = r.insert::<sml::IconSet>()?;
            is.set_attr(q("iconSet"), set.as_str())?;
            if rule.reverse_icons {
                is.set_attr(q("reverse"), "1")?;
            }
            if !rule.show_values {
                is.set_attr(q("showValue"), "0")?;
            }
            for (index, c) in cfvos.iter().enumerate() {
                cfvo(&mut is, c, rule.icon_gte.get(index).copied().flatten())?;
            }
        }
        _ => {}
    }
    Ok(())
}

impl Worksheet<'_> {
    /// Links an x14 data bar to the most recently written standard rule for the range.
    pub fn attach_extended_data_bar(&mut self, sqref: &str, model: &ExtendedDataBar<'_>) -> Result<()> {
        self.write_extended_data_bar(sqref, model, true, None)
    }

    /// Adds a data bar that has no standard conditional formatting counterpart.
    pub fn add_standalone_data_bar(
        &mut self, sqref: &str, model: &ExtendedDataBar<'_>, priority: Option<u32>,
    ) -> Result<()> {
        self.write_extended_data_bar(sqref, model, false, priority)
    }

    fn write_extended_data_bar(
        &mut self, sqref: &str, model: &ExtendedDataBar<'_>, link: bool, priority: Option<u32>,
    ) -> Result<()> {
        let invalid = |reason: &str| Error::InvalidSetting {
            what: "extended data bar".into(), reason: reason.into(),
        };
        let wb = &mut *self.wb;
        wb.check_editable()?;
        let sqref = ranges(sqref)?.iter().map(ToString::to_string).collect::<Vec<_>>().join(" ");
        let generated_id;
        let id = if let Some(id) = model.id {
            id
        } else {
            generated_id = new_extended_rule_id()?;
            &generated_id
        };
        if !valid_extended_rule_id(id) {
            return Err(invalid("expected a rule GUID"));
        }
        let (min, max) = (model.lengths.0.unwrap_or(10), model.lengths.1.unwrap_or(90));
        if min > max || max > 100 || !model.axis.is_none_or(|axis| matches!(axis, "auto" | "middle" | "none"))
            || !model.direction.is_none_or(|direction| matches!(direction, "leftToRight" | "rightToLeft" | "context"))
        {
            return Err(invalid("invalid lengths, axis, or direction"));
        }
        wb.check_thresholds(self.sheet, model.thresholds.iter().filter_map(|threshold| match threshold {
            ExtendedDataBarThreshold::Value(value) => Some(value),
            ExtendedDataBarThreshold::AutoMin | ExtendedDataBarThreshold::AutoMax => None,
        }))?;
        let part = wb.sheets[self.sheet].part.clone().expect("worksheet");
        let mut doc = wb.pkg.xml_part(&part)?.clone();
        if extended_rule_exists(doc.root(), id) {
            return Err(invalid("rule GUID already exists in worksheet"));
        }
        if let Some(priority) = priority {
            let (used, _) = priorities(doc.root());
            if priority == 0 || priority > i32::MAX as u32 || used.contains(&Some(priority)) {
                return Err(invalid("expected a unique positive priority"));
            }
        }
        let mut sheet = doc.lens_mut::<sml::Worksheet>()?;
        if link {
            let index = sheet.node().indexed_elements()
                .filter(|(_, node)| node.is(sml::ConditionalFormatting::QNAME))
                .filter(|(_, node)| node.attr(q("sqref")) == Some(sqref.as_str()))
                .last().map(|(index, _)| index).ok_or_else(|| invalid("missing standard data-bar rule"))?;
            let mut block = x14::child_at::<_, sml::ConditionalFormatting>(&mut sheet, index);
            let count = block.node().elements().filter(|node| node.is(sml::ConditionalFormattingRule::QNAME)).count();
            let mut rule = count.checked_sub(1).and_then(|index| block.nth_child_mut::<sml::ConditionalFormattingRule>(index))
                .ok_or_else(|| invalid("missing standard data-bar rule"))?;
            if rule.node().attr(q("type")) != Some("dataBar") || x14_id(rule.node()).is_some() {
                return Err(invalid("expected an unlinked standard data-bar rule"));
            }
            let mut list = rule.get_or_insert::<sml::ConditionalFormattingRuleExtensionList>()?;
            let mut extension = list.insert::<sml::ConditionalFormattingRuleExtension>()?;
            extension.set_attr(q("uri"), URI_CF_ID)?;
            extension.insert_with_ns::<x14::RuleId>(&EXTENDED_CF_NS[..1])?.set_text(id)?;
        }
        append_extended_block(&mut sheet, &sqref, |rule| {
            rule.set_attr(q("type"), "dataBar")?;
            rule.set_attr(q("id"), id)?;
            if let Some(priority) = priority {
                rule.set_attr(q("priority"), &priority.to_string())?;
            }
            let mut bar = rule.insert::<x14::DataBar>()?;
            // Only declared attributes are written; omitted ones keep the schema defaults.
            for (name, length) in [("minLength", model.lengths.0), ("maxLength", model.lengths.1)] {
                if let Some(length) = length {
                    bar.set_attr(q(name), &length.to_string())?;
                }
            }
            for (name, enabled) in ["border", "gradient", "negativeBarColorSameAsPositive",
                "negativeBarBorderColorSameAsPositive"].into_iter().zip(model.flags) {
                if let Some(enabled) = enabled {
                    bar.set_attr(q(name), if enabled { "1" } else { "0" })?;
                }
            }
            if let Some(axis) = model.axis {
                bar.set_attr(q("axisPosition"), axis)?;
            }
            if let Some(direction) = model.direction {
                bar.set_attr(q("direction"), direction)?;
            }
            if let Some(show) = model.show_value {
                bar.set_attr(q("showValue"), if show { "1" } else { "0" })?;
            }
            for threshold in model.thresholds {
                let (kind, value) = match threshold {
                    ExtendedDataBarThreshold::Value(value) => threshold_value(value),
                    ExtendedDataBarThreshold::AutoMin => ("autoMin", None),
                    ExtendedDataBarThreshold::AutoMax => ("autoMax", None),
                };
                let mut node = bar.insert::<x14::Cfvo>()?;
                node.set_attr(q("type"), kind)?;
                if let Some(value) = value {
                    node.insert_with_ns::<x14::Formula>(&EXTENDED_CF_NS[1..])?.set_text(&value)?;
                }
            }
            let [fill, border, negative_fill, negative_border, axis] = model.colors;
            write_extended_color::<x14::FillColor>(&mut bar, fill)?;
            write_extended_color::<x14::BorderColor>(&mut bar, border)?;
            write_extended_color::<x14::NegativeFillColor>(&mut bar, negative_fill)?;
            write_extended_color::<x14::NegativeBorderColor>(&mut bar, negative_border)?;
            write_extended_color::<x14::AxisColor>(&mut bar, axis)
        })?;
        let mut registry = RegistryPlan::new(&wb.pkg);
        registry.stage_part(&wb.pkg, &part, doc)?;
        wb.apply_registry_edit(registry, |_| Ok(()))
    }

    /// Adds a standalone x14 icon-set rule without modifying existing rules.
    pub fn add_extended_icon_rule(&mut self, sqref: &str, rule: &ExtendedIconRule<'_>) -> Result<()> {
        let invalid = |reason: &str| Error::InvalidSetting {
            what: "extended icon rule".into(), reason: reason.into(),
        };
        let wb = &mut *self.wb;
        wb.check_editable()?;
        let sqref = ranges(sqref)?.iter().map(ToString::to_string).collect::<Vec<_>>().join(" ");
        let count = extended_icon_count(rule.name).ok_or_else(|| invalid("unsupported extended icon set"))?;
        if rule.thresholds.len() != count {
            return Err(invalid("incorrect number of icon thresholds"));
        }
        if !rule.icons.is_empty() && rule.icons.len() != count {
            return Err(invalid("expected one custom icon per threshold"));
        }
        for (name, id) in rule.icons {
            let size = if name == "NoIcons" { Some(1) } else { extended_icon_count(name) };
            if size.is_none_or(|size| *id as usize >= size) {
                return Err(invalid("invalid custom icon selection"));
            }
        }
        let generated_id;
        let rule_id = if let Some(id) = rule.id {
            id
        } else {
            generated_id = new_extended_rule_id()?;
            &generated_id
        };
        if !valid_extended_rule_id(rule_id) {
            return Err(invalid("expected a rule GUID"));
        }
        wb.check_thresholds(self.sheet, rule.thresholds.iter().map(|(value, _)| value))?;
        let part = wb.sheets[self.sheet].part.clone().expect("worksheet");
        let mut doc = wb.pkg.xml_part(&part)?.clone();
        let (priorities, _) = priorities(doc.root());
        let priority = match (rule.priority, rule.id) {
            (None, None) => Some(priorities.iter().flatten().copied().max().unwrap_or(0)
                .max(rule.priority_floor).checked_add(1).ok_or_else(|| invalid("priority exhausted"))?),
            (priority, _) => priority,
        };
        if let Some(priority) = priority
            && (priority == 0 || priority > i32::MAX as u32 || priorities.contains(&Some(priority)))
        {
            return Err(invalid("expected a unique positive priority"));
        }
        if extended_rule_exists(doc.root(), rule_id) {
            return Err(invalid("rule GUID already exists in worksheet"));
        }
        let mut sheet = doc.lens_mut::<sml::Worksheet>()?;
        append_extended_block(&mut sheet, &sqref, |entry| {
            entry.set_attr(q("type"), "iconSet")?;
            entry.set_attr(q("id"), rule_id)?;
            if let Some(priority) = priority {
                entry.set_attr(q("priority"), &priority.to_string())?;
            }
            let mut icons = entry.insert::<x14::IconSet>()?;
            icons.set_attr(q("iconSet"), rule.name)?;
            icons.set_attr(q("reverse"), if rule.reverse { "1" } else { "0" })?;
            icons.set_attr(q("showValue"), if rule.show_value { "1" } else { "0" })?;
            if !rule.icons.is_empty() {
                icons.set_attr(q("custom"), "1")?;
            }
            for (threshold, gte) in rule.thresholds {
                let (kind, value) = threshold_value(threshold);
                let mut node = icons.insert::<x14::Cfvo>()?;
                node.set_attr(q("type"), kind)?;
                if let Some(gte) = gte {
                    node.set_attr(q("gte"), if *gte { "1" } else { "0" })?;
                }
                if let Some(value) = value {
                    node.insert_with_ns::<x14::Formula>(&EXTENDED_CF_NS[1..])?.set_text(&value)?;
                }
            }
            for (name, id) in rule.icons {
                let mut icon = icons.insert::<x14::CfIcon>()?;
                icon.set_attr(q("iconSet"), name)?;
                icon.set_attr(q("iconId"), &id.to_string())?;
            }
            Ok(())
        })?;
        let mut registry = RegistryPlan::new(&wb.pkg);
        registry.stage_part(&wb.pkg, &part, doc)?;
        wb.apply_registry_edit(registry, |_| Ok(()))
    }

    /// Use [`Workbook::add_differential_format`] to register a format for
    /// [`Rule::with_dxf`]. The priority must fit in a signed 32-bit integer.
    ///
    /// Adds a rule on `sqref` with the lowest priority (`max + 1` over the
    /// sheet's 2007 and x14 rules), in a new `conditionalFormatting` block
    /// after the last one (ADR-0014 §6). Refused, with every byte unchanged:
    /// a formula that does not lex or refers to another sheet
    /// ([`Error::CrossSheetReference`]), a `dxfId` that is not an existing
    /// `dxf`, a malformed colour.
    pub fn add_conditional_format(&mut self, sqref: &str, rule: &Rule) -> Result<()> {
        self.wb.add_conditional_format(self.sheet, sqref, rule, false, false)
    }

    /// Appends a rule to the last standard block on the same range.
    pub fn append_conditional_format_rule(&mut self, sqref: &str, rule: &Rule) -> Result<()> {
        self.wb.add_conditional_format(self.sheet, sqref, rule, false, true)
    }

    /// As [`Self::add_conditional_format`], but with priority 1: every
    /// other rule's priority, x14 included, goes up by one (a documented
    /// widening, ADR-0014 §6). Refused on a sheet with PivotTable blocks
    /// ([`Error::PivotConditionalFormat`]) or when a rule to be renumbered
    /// lacks a positive integer priority ([`Error::InvalidSetting`]).
    pub fn add_conditional_format_first(&mut self, sqref: &str, rule: &Rule) -> Result<()> {
        self.wb.add_conditional_format(self.sheet, sqref, rule, true, false)
    }

    /// Reads standard conditional-format blocks and their rule elements.
    pub fn conditional_format_blocks(&mut self) -> Result<Vec<RawNode>> {
        let part = self.wb.sheets[self.sheet].part.clone().expect("worksheet");
        Ok(self.wb.pkg.xml_part(&part)?.root().elements()
            .filter(|node| node.is(sml::ConditionalFormatting::QNAME)).cloned().collect())
    }

    /// Reads worksheet extension conditional-format blocks in document order.
    pub fn extended_conditional_format_blocks(&mut self) -> Result<Vec<RawNode>> {
        let part = self.wb.sheets[self.sheet].part.clone().expect("worksheet");
        let root = self.wb.pkg.xml_part(&part)?.root();
        let matches = |local| QName::new(x14::NS_X14, local);
        Ok(root.elements().filter(|node| node.is(sml::WorksheetExtensionList::QNAME))
            .flat_map(|list| list.elements().filter(|node| node.is(sml::WorksheetExtension::QNAME)))
            .flat_map(|ext| ext.elements().filter(|node| node.is(matches("conditionalFormattings"))))
            .flat_map(|group| group.elements().filter(|node| node.is(matches("conditionalFormatting"))))
            .cloned().collect())
    }

    /// Removes extension rules identified by unique IDs, leaving standard and unrelated rules intact.
    pub fn remove_standalone_conditional_formats(&mut self, ids: &[String]) -> Result<()> {
        use std::collections::HashSet;

        self.wb.check_editable()?;
        let part = self.wb.sheets[self.sheet].part.clone().expect("worksheet");
        let requested: HashSet<_> = ids.iter().map(|id| id.to_ascii_lowercase()).collect();
        let invalid = || Error::InvalidSetting {
            what: "conditional format".to_owned(),
            reason: "expected unique standalone extension rule IDs".to_owned(),
        };
        if requested.contains("") { return Err(invalid()); }
        let mut found = HashSet::new();
        let mut plan = Vec::new();
        {
            let root = self.wb.pkg.xml_part(&part)?.root();
            let lists = root.elements().filter(|node| node.is(sml::WorksheetExtensionList::QNAME)).count();
            if lists > 1 { return Err(invalid()); }
            for block in root.elements().filter(|node| node.is(sml::ConditionalFormatting::QNAME)) {
                if block.elements().filter_map(x14_id).any(|id| requested.contains(&id.to_ascii_lowercase())) {
                    return Err(invalid());
                }
            }
            for (ei, ext) in x14::exts(root, x14::URI_CF) {
                for (si, groups) in ext.indexed_elements().filter(|(_, node)| node.is(x14::ConditionalFormattings::QNAME)) {
                    for (ci, block) in groups.indexed_elements().filter(|(_, node)| node.is(x14::ConditionalFormatting::QNAME)) {
                        for (ri, rule) in block.indexed_elements().filter(|(_, node)| node.is(x14::CfRule::QNAME)) {
                            let Some(id) = rule.attr(q("id")).map(str::to_ascii_lowercase) else { continue };
                            if requested.contains(&id) {
                                if !found.insert(id) { return Err(invalid()); }
                                plan.push((ei, si, ci, ri));
                            }
                        }
                    }
                }
            }
        }
        if found != requested { return Err(invalid()); }
        if plan.is_empty() { return Ok(()); }
        self.wb.poisoning(|wb| {
            let mut sheet = wb.pkg.xml_part_mut(&part)?.lens_mut::<sml::Worksheet>()?;
            for (ei, si, ci, ri) in plan.into_iter().rev() {
                remove_x14_rule(&mut sheet, ei, si, ci, ri)?;
            }
            Ok(())
        })
    }

    /// Removes selected standard blocks in one operation.
    pub fn remove_selected_conditional_formats(&mut self, ordinals: &[u32]) -> Result<()> {
        match self.wb.remove_conditional_formats_matching(self.sheet, "", None, |index, _| {
            u32::try_from(index).is_ok_and(|index| ordinals.contains(&index))
        }) {
            Ok(_) | Err(Error::NoSuchRule { .. }) => Ok(()),
            Err(error) => Err(error),
        }
    }

    /// Counts standard conditional-format blocks in worksheet order.
    pub fn conditional_format_count(&mut self) -> Result<usize> {
        let part = self.wb.sheets[self.sheet].part.clone().expect("worksheet");
        Ok(self.wb.pkg.xml_part(&part)?.root().elements()
            .filter(|node| node.is(sml::ConditionalFormatting::QNAME)).count())
    }

    /// Removes one block by ordinal, or all standard conditional-format blocks.
    pub fn remove_conditional_format_blocks(&mut self, ordinal: Option<usize>) -> Result<()> {
        match self.wb.remove_conditional_formats_matching(self.sheet, "", None, |index, _| {
            ordinal.is_none_or(|wanted| wanted == index)
        }) {
            Ok(_) | Err(Error::NoSuchRule { .. }) => Ok(()),
            Err(error) => Err(error),
        }
    }

    /// Removes the rules on exactly `sqref` (and with `priority`, if
    /// given), their emptied blocks, and the x14 rules they link to (with
    /// their emptied containers). A PivotTable block is refused
    /// ([`Error::PivotConditionalFormat`]); [`Error::NoSuchRule`] if none.
    pub fn remove_conditional_format(&mut self, sqref: &str, priority: Option<u32>) -> Result<CfRemoval> {
        self.wb.remove_conditional_format(self.sheet, sqref, priority)
    }
}
