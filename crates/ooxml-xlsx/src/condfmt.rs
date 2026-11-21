//! Phase4 part 2: conditional formatting (ADR-0014 §6), 2007 forms.
//!
//! Register a format with [`Workbook::add_differential_format`] and pass its
//! index to [`Rule::with_dxf`]. Colour scales, data bars and icon sets need no
//! differential format.

use ooxml_core::{ChildOf, ElementSpec, EnumType, LensMut, QName, RawNode};
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
}

impl Rule {
    /// A rule without a format (colour scales, data bars and icon sets need
    /// none).
    pub fn new(kind: RuleKind) -> Self {
        Rule { kind, dxf: None, stop_if_true: false }
    }

    /// The format: an existing `dxfs` entry of `styles.xml` (by index).
    pub fn with_dxf(mut self, dxf_id: u32) -> Self {
        self.dxf = Some(dxf_id);
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
    fn dxf_count(&mut self) -> Result<u32> {
        let rels = self.pkg.relationships(&self.workbook_part)?;
        let Some(p) = rels.by_type(REL_STYLES).next().and_then(|r| rels.resolve(r)) else { return Ok(0) };
        let doc = self.pkg.xml_part(&p)?;
        Ok(doc.root().elements().find(|e| e.name().local() == "dxfs").map_or(0, |d| d.elements().filter(|e| e.name().local() == "dxf").count() as u32))
    }

    /// The rule's formulas, checked (relative to the first range's
    /// top-left cell).
    fn rule_formulas(&self, sheet: usize, kind: &RuleKind, top_left: &str) -> Result<Vec<String>> {
        let text = |t: &str| -> Result<String> {
            if let Some((offset, ch)) = ooxml_core::value::first_invalid_xml_char(t) {
                return Err(ooxml_core::Error::InvalidXmlChar { ch, offset }.into());
            }
            Ok(t.replace('"', "\"\""))
        };
        Ok(match kind {
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
            RuleKind::ColorScale(stops) => {
                if !(2..=3).contains(&stops.len()) || stops.iter().any(|(_, c)| !argb_ok(c)) {
                    return Err(Error::InvalidSetting { what: "colour scale".into(), reason: "2 or 3 stops with ARGB colours".into() });
                }
                self.check_thresholds(sheet, stops.iter().map(|(c, _)| c))?;
                Vec::new()
            }
            RuleKind::DataBar(a, b, c) => {
                if !argb_ok(c) {
                    return Err(Error::InvalidSetting { what: "data bar".into(), reason: format!("{c:?} is not ARGB") });
                }
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

    fn add_conditional_format(&mut self, sheet: usize, sqref: &str, rule: &Rule, first: bool) -> Result<()> {
        self.check_editable()?;
        let rs = ranges(sqref)?;
        let sqref = rs.iter().map(ToString::to_string).collect::<Vec<_>>().join(" ");
        let top_left = rs[0].first.to_string();
        let formulas = self.rule_formulas(sheet, &rule.kind, &top_left)?;
        if let Some(d) = rule.dxf
            && d >= self.dxf_count()?
        {
            return Err(Error::InvalidSetting { what: "dxfId".into(), reason: format!("{d} is not an existing dxf") });
        }
        let part = self.sheets[sheet].part.clone().expect("worksheet");
        let (prios, pivot) = priorities(self.pkg.xml_part(&part)?.root());
        if first && pivot {
            return Err(Error::PivotConditionalFormat(sqref));
        }
        if first && prios.iter().any(|p| p.is_none_or(|p| p == 0)) {
            return Err(Error::InvalidSetting {
                what: "conditional formatting priority".into(),
                reason: "cannot insert first when an existing rule lacks a positive integer priority".into(),
            });
        }
        let max_priority = prios.into_iter().flatten().max().unwrap_or(0);
        if max_priority >= i32::MAX as u32 {
            return Err(Error::InvalidSetting {
                what: "conditional formatting priority".into(),
                reason: "no higher priority value fits in the schema's signed integer".into(),
            });
        }
        let priority = if first { 1 } else { max_priority + 1 };
        {
            let doc = self.pkg.xml_part_mut(&part)?;
            let ws = doc.lens_mut::<sml::Worksheet>()?;
            ws.check_insert::<sml::ConditionalFormatting>()?;
        }
        let rule = rule.clone();
        self.poisoning(|wb| {
            let doc = wb.pkg.xml_part_mut(&part)?;
            let mut ws = doc.lens_mut::<sml::Worksheet>()?;
            if first {
                renumber(&mut ws)?;
            }
            // a new block, after the last one (existing blocks stay byte-identical)
            let mut block = ws.insert::<sml::ConditionalFormatting>()?;
            block.set_attr(q("sqref"), &sqref)?;
            let mut r = block.insert::<sml::ConditionalFormattingRule>()?;
            write_rule(&mut r, &rule, priority, &formulas)
        })
    }

    fn remove_conditional_format(&mut self, sheet: usize, sqref: &str, priority: Option<u32>) -> Result<CfRemoval> {
        self.check_editable()?;
        let want = ranges(sqref)?;
        let part = self.sheets[sheet].part.clone().expect("worksheet");
        // (block item, rule items, block emptied), x14 ids
        let mut plan: Vec<(usize, Vec<usize>, bool)> = Vec::new();
        let mut ids: Vec<String> = Vec::new();
        {
            let doc = self.pkg.xml_part(&part)?;
            for (bi, b) in doc.root().indexed_elements().filter(|(_, e)| e.is(sml::ConditionalFormatting::QNAME)) {
                if b.attr(q("sqref")).and_then(|s| ranges(s).ok()) != Some(want.clone()) {
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

fn cfvo<P: ElementSpec>(parent: &mut LensMut<'_, P>, c: &Cfvo) -> Result<()>
where
    sml::ConditionalFormatValueObject: ChildOf<P>,
{
    let (ty, val) = match c {
        Cfvo::Min => ("min", None),
        Cfvo::Max => ("max", None),
        Cfvo::Num(x) => ("num", Some(num(*x))),
        Cfvo::Percent(x) => ("percent", Some(num(*x))),
        Cfvo::Percentile(x) => ("percentile", Some(num(*x))),
        Cfvo::Formula(f) => ("formula", Some(f.strip_prefix('=').unwrap_or(f).to_owned())),
    };
    let mut e = parent.insert::<sml::ConditionalFormatValueObject>()?;
    e.set_attr(q("type"), ty)?;
    if let Some(v) = val {
        e.set_attr(q("val"), &v)?;
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
        RuleKind::ContainsText(t) => ("containsText", Some("containsText"), Some(t.as_str())),
        RuleKind::NotContainsText(t) => ("notContainsText", Some("notContains"), Some(t.as_str())),
        RuleKind::BeginsWith(t) => ("beginsWith", Some("beginsWith"), Some(t.as_str())),
        RuleKind::EndsWith(t) => ("endsWith", Some("endsWith"), Some(t.as_str())),
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
        RuleKind::AboveAverage(false) => r.set_attr(q("aboveAverage"), "0")?,
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
    if let Some(op) = op {
        r.set_attr(q("operator"), op)?;
    }
    if let Some(t) = text {
        r.set_attr(q("text"), t)?;
    }
    for f in formulas {
        r.insert::<sml::Formula>()?.set_text(f)?;
    }
    match &rule.kind {
        RuleKind::ColorScale(stops) => {
            let mut cs = r.insert::<sml::ColorScale>()?;
            for (c, _) in stops {
                cfvo(&mut cs, c)?;
            }
            for (_, col) in stops {
                cs.insert::<sml::Color>()?.set_attr(q("rgb"), col)?;
            }
        }
        RuleKind::DataBar(a, b, col) => {
            let mut db = r.insert::<sml::DataBar>()?;
            cfvo(&mut db, a)?;
            cfvo(&mut db, b)?;
            db.insert::<sml::Color>()?.set_attr(q("rgb"), col)?;
        }
        RuleKind::IconSet(set, cfvos) => {
            let mut is = r.insert::<sml::IconSet>()?;
            is.set_attr(q("iconSet"), set.as_str())?;
            for c in cfvos {
                cfvo(&mut is, c)?;
            }
        }
        _ => {}
    }
    Ok(())
}

impl Worksheet<'_> {
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
        self.wb.add_conditional_format(self.sheet, sqref, rule, false)
    }

    /// As [`Self::add_conditional_format`], but with priority 1: every
    /// other rule's priority, x14 included, goes up by one (a documented
    /// widening, ADR-0014 §6). Refused on a sheet with PivotTable blocks
    /// ([`Error::PivotConditionalFormat`]) or when a rule to be renumbered
    /// lacks a positive integer priority ([`Error::InvalidSetting`]).
    pub fn add_conditional_format_first(&mut self, sqref: &str, rule: &Rule) -> Result<()> {
        self.wb.add_conditional_format(self.sheet, sqref, rule, true)
    }

    /// Removes the rules on exactly `sqref` (and with `priority`, if
    /// given), their emptied blocks, and the x14 rules they link to (with
    /// their emptied containers). A PivotTable block is refused
    /// ([`Error::PivotConditionalFormat`]); [`Error::NoSuchRule`] if none.
    pub fn remove_conditional_format(&mut self, sqref: &str, priority: Option<u32>) -> Result<CfRemoval> {
        self.wb.remove_conditional_format(self.sheet, sqref, priority)
    }
}
