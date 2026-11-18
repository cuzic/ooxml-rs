//! The "nothing else changes" oracle for xlsx edits (ADR-0003 and ADR-0006)
//! (review of PR #36 round 3, M2). Before each successful write the caller
//! records `Workbook::dependents` and whether the formula-cell set changes;
//! after the edits every part that changed must differ from the input only
//! by the allowed edits:
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
//! - the existing shared-string table: append-only `si` entries and count
//!   updates after a successful string write;
//! - every other part (other sheets, every other `.rels`, styles, …) is
//!   Layer-1 identical.
//!
//! The edited sheets are left to the callers' own read-back checks
//! (insertions there would need a tree alignment).

use std::collections::{BTreeMap, BTreeSet};

use ooxml_core::{QName, RawItem, RawNode, XmlDocument};
use ooxml_xlsx::{CellIndex, SML_NS, Workbook};

const SHARED_STRINGS_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings";
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
    /// A successful string write could have activated the shared-string registry.
    wrote_string: bool,
}

/// Facts taken just before one write.
#[derive(Debug)]
pub struct Pending {
    deps: Vec<(String, CellIndex)>,
    had_formula: bool,
}

impl Pending {
    /// The dependents `Workbook::dependents` reported before the write.
    pub fn dependents(&self) -> &[(String, CellIndex)] {
        &self.deps
    }
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

    /// Call after a successful `set_string` write.
    pub fn wrote_string(&mut self, p: Pending) {
        self.wrote(p, false);
        self.wrote_string = true;
    }
}

/// Asserts the allowed xlsx diff between `base` (the zero-edit save) and `out`;
/// `skip` are the edited sheet parts.
pub fn assert_phase1_diff(wb: &mut Workbook, base: &[u8], out: &[u8], skip: &[&str], workbook: &str, edits: &Phase1Edits) {
    // Two sheets with exactly the same name resolve to the first one in
    // every by-name lookup (`dependents`, `worksheet_by_name`), so the
    // dependents cannot be placed: skip this check (Excel never writes such
    // a workbook; the callers' other checks still run). Review of PR #36
    // round 4, m12.
    let names: Vec<String> = wb.sheet_names().map(str::to_owned).collect();
    if names.iter().enumerate().any(|(i, n)| names[..i].contains(n)) {
        return;
    }
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
    let rels = pa.relationships(workbook).ok();
    let target = |rel_type| {
        let r = rels.as_ref()?;
        r.by_type(rel_type).next().and_then(|x| r.resolve(x)).filter(|p| pa.contains(p))
    };
    let chain = target(CALC_CHAIN_REL);
    let sst = target(SHARED_STRINGS_REL);
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
        } else if Some(n) == sst.as_deref() {
            edits.wrote_string && shared_strings_diff(&da, &db)
        } else if let Some(refs) = dep_parts.get(n) {
            eq_with(da.root(), db.root(), &Rule::Deps(refs))
        } else if let Some(c) = chain.as_deref().filter(|_| chain_gone && (n == wb_rels || n == "[Content_Types].xml")) {
            eq_with(da.root(), db.root(), &Rule::Chain(c))
        } else {
            false
        };
        assert!(ok, "{n} changed beyond the allowed xlsx edits");
    }
}

fn shared_strings_diff(base: &XmlDocument, out: &XmlDocument) -> bool {
    let (a, b) = (base.root(), out.root());
    let count = QName::new("", "count");
    let unique = QName::new("", "uniqueCount");
    let other_attrs = |n: &RawNode| {
        n.attrs().iter().filter(|x| !x.name().is(count) && !x.name().is(unique)).cloned().collect::<Vec<_>>()
    };
    if !a.is(QName::new(SML_NS, "sst")) || a.name() != b.name() || other_attrs(a) != other_attrs(b) {
        return false;
    }

    let si = QName::new(SML_NS, "si");
    let old_len = a.elements().filter(|n| n.is(si)).count();
    let items: Vec<_> = b.elements().filter(|n| n.is(si)).collect();
    let Some(appended) = items.get(old_len..) else { return false };

    // A missing counter stays missing. uniqueCount changes only on append.
    let valid_unique_count = if appended.is_empty() {
        a.attr(unique) == b.attr(unique)
    } else {
        b.attr(unique).is_none_or(|v| v.parse::<usize>() == Ok(items.len()))
    };
    if a.attr(count).is_some() != b.attr(count).is_some()
        || b.attr(count).is_some_and(|v| a.attr(count) != Some(v) && v.parse::<u64>().is_err())
        || a.attr(unique).is_some() != b.attr(unique).is_some()
        || !valid_unique_count
    {
        return false;
    }

    if let Some(last) = appended.last()
        && let Some(ext) = b.elements().find(|n| n.is(QName::new(SML_NS, "extLst")))
        && last.span().expect("parsed si span").end() > ext.span().expect("parsed extLst span").start
    {
        return false;
    }

    let a = a.span().expect("parsed sst span");
    let b = b.span().expect("parsed sst span");
    let (a_close, b_close) = (a.close.map_or(a.open_end, |(start, _)| start), b.close.map_or(b.open_end, |(start, _)| start));
    if base.source()[..a.start] != out.source()[..b.start] || base.source()[a.end()..] != out.source()[b.end()..] {
        return false;
    }

    // Compare the entire body after removing just the new entries. Even
    // whitespace, escapes, comments and extension XML must survive verbatim.
    let mut body = String::new();
    let mut cursor = b.open_end;
    for n in appended {
        let span = n.span().expect("parsed si span");
        body.push_str(&out.source()[cursor..span.start]);
        cursor = span.end();
    }
    body.push_str(&out.source()[cursor..b_close]);
    body == base.source()[a.open_end..a_close]
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
