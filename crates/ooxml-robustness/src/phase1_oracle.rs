//! The Phase1 (ADR-0003) "nothing else changes" oracle for xlsx edits
//! (review of PR #36 round 3, M2). Before each successful write the caller
//! records `Workbook::dependents` and whether the formula-cell set changes;
//! after the edits every part that changed must differ from the input only
//! by what Phase1 may write:
//!
//! - a sheet with dependents: `v` / `is` children and the `t` attribute of
//!   the dependent cells, nothing else;
//! - the workbook part: `calcPr/@fullCalcOnLoad` set to `1` (or a `calcPr`
//!   with it inserted), only if some edit had dependents or changed the
//!   formula set;
//! - `[Content_Types].xml` and the workbook's relationships: the calcChain
//!   `Override` / `Relationship` gone, only when calcChain was removed;
//! - `calcChain.xml` (the workbook's calcChain relationship target, as
//!   ooxml-xlsx resolves it) is removed exactly when the formula set
//!   changed;
//! - every other part (other sheets, every other `.rels`, styles, shared
//!   strings, …) is Layer-1 identical.
//!
//! The edited sheets are left to the callers' own read-back checks
//! (insertions there would need a tree alignment).

use std::collections::{BTreeMap, BTreeSet};

use ooxml_core::{QName, RawItem, RawNode, XmlDocument};
use ooxml_xlsx::{CellIndex, Workbook};

const CALC_CHAIN_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/calcChain";

/// What the successful writes so far may have changed.
#[derive(Debug, Default)]
pub struct Phase1Edits {
    /// (sheet name, A1 reference) of every dependent invalidated.
    deps: BTreeSet<(String, String)>,
    /// Some write changed the set of formula cells.
    formula_set_changed: bool,
    /// Some write had dependents or changed the formula set.
    needs_calc: bool,
}

/// Facts taken just before one write.
#[derive(Debug)]
pub struct Pending {
    deps: Vec<(String, CellIndex)>,
    had_formula: bool,
}

impl Phase1Edits {
    /// Call just before writing `sheet!(row, col)`.
    pub fn before_write(&self, wb: &mut Workbook, sheet: &str, row: u32, col: u32) -> Pending {
        let deps = CellIndex::new(row, col).ok().and_then(|ci| wb.dependents(sheet, ci).ok()).unwrap_or_default();
        let had_formula = wb
            .worksheet_by_name(sheet)
            .ok()
            .and_then(|ws| ws.cell(row, col).ok().flatten().map(|c| c.formula().is_some()))
            .unwrap_or(false);
        Pending { deps, had_formula }
    }

    /// Call after the write succeeded; `has_formula` = the cell holds a
    /// formula now (`set_formula`).
    pub fn wrote(&mut self, p: Pending, has_formula: bool) {
        let changed = p.had_formula != has_formula;
        self.needs_calc |= !p.deps.is_empty() || changed;
        self.formula_set_changed |= changed;
        self.deps.extend(p.deps.into_iter().map(|(s, c)| (s, c.to_string())));
    }
}

/// Asserts the Phase1 diff between `base` (the zero-edit save) and `out`;
/// `skip` are the edited sheet parts.
pub fn assert_phase1_diff(wb: &mut Workbook, base: &[u8], out: &[u8], skip: &[&str], workbook: &str, edits: &Phase1Edits) {
    let a = crate::zipcmp::raw_entries(base).expect("base raw");
    let b = crate::zipcmp::raw_entries(out).expect("out raw");
    let pa = ooxml_core::Package::from_bytes(base.to_vec()).expect("base reopens");
    let pb = ooxml_core::Package::from_bytes(out.to_vec()).expect("out reopens");
    let mut dep_parts: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (s, r) in &edits.deps {
        if let Ok(ws) = wb.worksheet_by_name(s) {
            dep_parts.entry(ws.part_name().to_owned()).or_default().insert(r.clone());
        }
    }
    let chain: Option<String> = pa
        .relationships(workbook)
        .ok()
        .and_then(|r| r.by_type(CALC_CHAIN_REL).next().and_then(|x| r.resolve(x)))
        .filter(|p| pa.contains(p));
    let chain_gone = chain.as_ref().is_some_and(|c| !b.iter().any(|e| &e.name == c));
    if edits.formula_set_changed {
        assert!(chain.is_none() || chain_gone, "the formula-cell set changed but calcChain.xml stayed");
    } else {
        assert!(!chain_gone, "calcChain.xml removed although the formula-cell set did not change");
    }
    let wb_rels = ooxml_core::opc::rels_part_name(Some(workbook));
    let names_b: Vec<&str> = b.iter().map(|e| e.name.as_str()).collect();
    let names_a: Vec<&str> = a.iter().map(|e| e.name.as_str()).filter(|n| Some(*n) != chain.as_deref() || !chain_gone).collect();
    assert_eq!(names_a, names_b, "entry names/order changed");
    for ea in &a {
        let Some(eb) = b.iter().find(|e| e.name == ea.name) else { continue };
        let n = ea.name.as_str();
        if skip.contains(&n) || (ea.method, ea.crc32, &ea.compressed) == (eb.method, eb.crc32, &eb.compressed) {
            continue;
        }
        let parse = |p: &ooxml_core::Package| {
            XmlDocument::parse(&p.read_part(n).unwrap_or_default()).unwrap_or_else(|e| panic!("{n}: changed part does not parse: {e}"))
        };
        let (da, db) = (parse(&pa), parse(&pb));
        let ok = if n == workbook {
            edits.needs_calc && eq_with(da.root(), db.root(), &Rule::CalcPr)
        } else if let Some(refs) = dep_parts.get(n) {
            eq_with(da.root(), db.root(), &Rule::Deps(refs))
        } else if let Some(c) = chain.as_deref().filter(|_| chain_gone && (n == wb_rels || n == "[Content_Types].xml")) {
            eq_with(da.root(), db.root(), &Rule::Chain(c))
        } else {
            false
        };
        assert!(ok, "{n} changed beyond what Phase1 may change");
    }
}

/// Which difference a comparison tolerates.
enum Rule<'a> {
    /// `calcPr/@fullCalcOnLoad` (set to 1, or a new `calcPr` carrying it).
    CalcPr,
    /// `v` / `is` / `@t` of these cells.
    Deps(&'a BTreeSet<String>),
    /// The calcChain `Relationship` / `Override` removed.
    Chain(&'a str),
}

fn attrs_eq(a: &RawNode, b: &RawNode, skip: &str) -> bool {
    let f = |n: &RawNode| n.attrs().iter().filter(|x| !(x.name().ns().is_empty() && x.name().local() == skip)).cloned().collect::<Vec<_>>();
    f(a) == f(b)
}

fn eq_with(a: &RawNode, b: &RawNode, rule: &Rule<'_>) -> bool {
    if a.name() != b.name() {
        return false;
    }
    let local = a.name().local();
    let (skip_attr, drop_a): (&str, &dyn Fn(&RawNode) -> bool) = match rule {
        Rule::CalcPr if local == "calcPr" => {
            if b.attr(QName::new("", "fullCalcOnLoad")) != Some("1") {
                return false;
            }
            ("fullCalcOnLoad", &|_| false)
        }
        Rule::Deps(refs) if local == "c" && a.attr(QName::new("", "r")).is_some_and(|r| refs.contains(r)) => {
            ("t", &|n: &RawNode| matches!(n.name().local(), "v" | "is"))
        }
        _ => ("\u{0}", &|_| false),
    };
    if !attrs_eq(a, b, skip_attr) {
        return false;
    }
    let keep_a = |it: &&RawItem| match it {
        RawItem::Element(e) => !drop_a(e) && !matches!(rule, Rule::Chain(c) if refs_chain(e, c)),
        _ => true,
    };
    let ca: Vec<&RawItem> = a.children().iter().filter(keep_a).collect();
    let mut cb: Vec<&RawItem> = b.children().iter().collect();
    // calcPr may be inserted where none existed
    if matches!(rule, Rule::CalcPr)
        && local == "workbook"
        && ca.len() + 1 == cb.len()
        && let Some(i) = cb.iter().position(|it| matches!(it, RawItem::Element(e) if e.name().local() == "calcPr"))
        && !ca.iter().any(|it| matches!(it, RawItem::Element(e) if e.name().local() == "calcPr"))
    {
        cb.remove(i);
    }
    // dropping elements can leave adjacent whitespace: compare without whitespace-only text
    let ws = |it: &&&RawItem| !matches!(it, RawItem::Text(t) if t.is_whitespace());
    let ca: Vec<&RawItem> = ca.iter().filter(ws).copied().collect();
    let cb: Vec<&RawItem> = cb.iter().filter(ws).copied().collect();
    ca.len() == cb.len()
        && ca.iter().zip(&cb).all(|(x, y)| match (x, y) {
            (RawItem::Element(x), RawItem::Element(y)) => eq_with(x, y, rule),
            (RawItem::Text(x), RawItem::Text(y)) => x.value() == y.value(),
            (RawItem::Markup(x), RawItem::Markup(y)) => x == y,
            _ => false,
        })
}

fn refs_chain(e: &RawNode, chain: &str) -> bool {
    let a = |k: &str| e.attrs().iter().find(|x| x.name().ns().is_empty() && x.name().local() == k).map_or("", |x| x.value());
    (e.name().local() == "Relationship" && a("Type").ends_with("/calcChain"))
        || (e.name().local() == "Override" && a("PartName").trim_start_matches('/').eq_ignore_ascii_case(chain))
}
