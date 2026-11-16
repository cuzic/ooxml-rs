//! `ooxml-xlsx` Phase1 (ADR-0003): formula-aware edits on any worksheet.
//!
//! The harness keeps its own, deliberately simple dependency model: it
//! reads every worksheet's cells and formulas from the XML, extracts the
//! plain A1 references of each formula (with sheet prefixes, ranges, whole
//! rows/columns; shared-formula members translated from their master;
//! anything it cannot read — names, 3-D references, structured references,
//! functions like `INDIRECT` — simply yields no edge), and closes them
//! transitively, with array results. Every edge it finds is a real
//! precedent, so its dependent set is a **subset** of what ooxml-xlsx must
//! invalidate (the library over-approximates whatever it cannot read
//! precisely). The checks:
//!
//! - `Err` ⇒ the package saves byte-identical; after `Err(Poisoned)` every
//!   later edit and the save are `Poisoned` too;
//! - `Workbook::dependents(sheet, cell)` ⊇ the model's dependents;
//! - after `Ok`: no model dependent keeps a `<v>`; `calcPr/@fullCalcOnLoad`
//!   is `1` (or the workbook is in manual mode) when there were dependents
//!   or the set of formula cells changed; `calcChain.xml` is removed exactly
//!   when the set of formula cells changed (with its relationship and
//!   content-type override); a written value reads back; parts other than
//!   the workbook, the package registries and the formula/edited sheets are
//!   Layer-1 identical;
//! - the set of formula cells read back from the output equals the model's
//!   expectation, and the output reopens.

use std::collections::{BTreeMap, BTreeSet};

use ooxml_core::{RawNode, XmlDocument};
use ooxml_xlsx::{CellIndex, CellValue, Error, Workbook};

use crate::fixtures::{XLSX, build_zip, decode};
use crate::input::Input;
use crate::zipcmp::{assert_l1_except_removing, raw_entries};

/// How often the oracles fired (for reporting coverage of the checks):
/// successful edits, dependents checked for a stripped cache, calcChain
/// decisions checked, refused edits.
pub static STATS: [std::sync::atomic::AtomicUsize; 4] = [const { std::sync::atomic::AtomicUsize::new(0) }; 4];

fn bump(i: usize, n: usize) {
    STATS[i].fetch_add(n, std::sync::atomic::Ordering::Relaxed);
}

const SML: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const MAX_ROW: u32 = 1_048_575;
const MAX_COL: u32 = 16_383;

/// A cell position: (sheet index, row, column), 0-based.
type At = (usize, u32, u32);

/// A rectangle a formula reads: (sheet, r1, c1, r2, c2), inclusive.
type Rect = (usize, u32, u32, u32, u32);

#[derive(Clone, Debug, Default)]
struct Cell {
    /// The formula text in effect (shared members: the master's, translated
    /// by the harness), if the cell has an `f`.
    formula: Option<String>,
    has_f: bool,
    has_v: bool,
    /// `f/@ref` of an array formula (its result range).
    array_ref: Option<(u32, u32, u32, u32)>,
    /// The edges the harness could read.
    reads: Vec<Rect>,
}

#[derive(Clone, Debug, Default)]
struct Model {
    names: Vec<String>,
    parts: Vec<Option<String>>,
    cells: Vec<BTreeMap<(u32, u32), Cell>>,
    manual: bool,
    full_calc: bool,
    calc_chain: Option<String>,
}

fn is_sml(n: &RawNode, local: &str) -> bool {
    n.name().ns() == SML && n.name().local() == local
}

fn attr<'a>(n: &'a RawNode, local: &str) -> Option<&'a str> {
    n.attrs().iter().find(|a| a.name().ns().is_empty() && a.name().local() == local).map(|a| a.value())
}

fn col_of(letters: &str) -> Option<u32> {
    if letters.is_empty() || letters.len() > 3 {
        return None;
    }
    let mut c = 0u32;
    for b in letters.bytes() {
        c = c * 26 + u32::from(b.to_ascii_uppercase() - b'A') + 1;
    }
    (c >= 1 && c - 1 <= MAX_COL).then(|| c - 1)
}

/// `A1`, `$A$1`: (row, col, row absolute, col absolute).
fn cell_ref(s: &str) -> Option<(u32, u32, bool, bool)> {
    let (ca, rest) = s.strip_prefix('$').map_or((false, s), |r| (true, r));
    let split = rest.find(|c: char| !c.is_ascii_alphabetic())?;
    let (letters, rest) = rest.split_at(split);
    let (ra, digits) = rest.strip_prefix('$').map_or((false, rest), |r| (true, r));
    if letters.is_empty() || digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) || digits.len() > 7 {
        return None;
    }
    let row: u32 = digits.parse().ok()?;
    (row >= 1 && row - 1 <= MAX_ROW).then_some(())?;
    Some((row - 1, col_of(letters)?, ra, ca))
}

/// The references of a formula as rectangles relative to `own_sheet`,
/// shifted by (`dr`, `dc`) for their relative parts (shared members).
/// Returns no rectangle for anything it does not understand.
fn references(text: &str, own_sheet: usize, names: &[String], dr: i64, dc: i64) -> Vec<Rect> {
    let mut out = Vec::new();
    let b = text.as_bytes();
    let mut i = 0;
    let mut sheet: Option<Option<usize>> = None; // Some(None) = unknown/3-D sheet prefix
    let shift = |v: u32, abs: bool, d: i64, max: u32| -> Option<u32> {
        if abs {
            return Some(v);
        }
        let n = i64::from(v) + d;
        (0..=i64::from(max)).contains(&n).then_some(n as u32)
    };
    let find_sheet = |name: &str| names.iter().position(|n| n.eq_ignore_ascii_case(name));
    while i < b.len() {
        let c = b[i];
        if c == b'"' {
            i += 1;
            while i < b.len() {
                if b[i] == b'"' {
                    if b.get(i + 1) == Some(&b'"') {
                        i += 2;
                        continue;
                    }
                    break;
                }
                i += 1;
            }
            i += 1;
            continue;
        }
        if c == b'[' {
            // structured or external reference: skip it and the token it belongs to
            while i < b.len() && b[i] != b']' {
                i += 1;
            }
            i += 1;
            sheet = None;
            continue;
        }
        if c == b'\'' {
            let mut name = String::new();
            i += 1;
            while i < b.len() {
                if b[i] == b'\'' {
                    if b.get(i + 1) == Some(&b'\'') {
                        name.push('\'');
                        i += 2;
                        continue;
                    }
                    break;
                }
                name.push(b[i] as char);
                i += 1;
            }
            i += 1;
            if b.get(i) == Some(&b'!') {
                i += 1;
                sheet = Some(if name.contains(':') { None } else { find_sheet(&name) });
            }
            continue;
        }
        if c.is_ascii_alphanumeric() || c == b'$' || c == b'_' || c == b'.' || c == b'\\' {
            let start = i;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || matches!(b[i], b'$' | b'_' | b'.' | b':' | b'\\')) {
                i += 1;
            }
            let word = &text[start..i];
            // part of a longer identifier (non-ASCII letters, e.g. a sheet name)
            let glued = start > 0 && !b[start - 1].is_ascii() || i < b.len() && !b[i].is_ascii();
            if b.get(i) == Some(&b'!') {
                i += 1;
                sheet = Some(if word.contains(':') || glued { None } else { find_sheet(word) });
                continue;
            }
            let prefix = sheet.take();
            if b.get(i) == Some(&b'(') || glued {
                continue; // function name (or unreadable)
            }
            let target = match prefix {
                None => Some(own_sheet),
                Some(s) => s,
            };
            let Some(sh) = target else { continue };
            let parts: Vec<&str> = word.split(':').collect();
            let rect = match parts[..] {
                [one] => cell_ref(one).and_then(|(r, c, ra, ca)| {
                    let (r, c) = (shift(r, ra, dr, MAX_ROW)?, shift(c, ca, dc, MAX_COL)?);
                    Some((sh, r, c, r, c))
                }),
                [a, z] => {
                    if let (Some((r1, c1, ra1, ca1)), Some((r2, c2, ra2, ca2))) = (cell_ref(a), cell_ref(z)) {
                        (|| {
                            let (r1, c1) = (shift(r1, ra1, dr, MAX_ROW)?, shift(c1, ca1, dc, MAX_COL)?);
                            let (r2, c2) = (shift(r2, ra2, dr, MAX_ROW)?, shift(c2, ca2, dc, MAX_COL)?);
                            Some((sh, r1.min(r2), c1.min(c2), r1.max(r2), c1.max(c2)))
                        })()
                    } else if let (Some(c1), Some(c2)) = (col_part(a), col_part(z)) {
                        (|| {
                            let (c1, c2) = (shift(c1.0, c1.1, dc, MAX_COL)?, shift(c2.0, c2.1, dc, MAX_COL)?);
                            Some((sh, 0, c1.min(c2), MAX_ROW, c1.max(c2)))
                        })()
                    } else if let (Some(r1), Some(r2)) = (row_part(a), row_part(z)) {
                        (|| {
                            let (r1, r2) = (shift(r1.0, r1.1, dr, MAX_ROW)?, shift(r2.0, r2.1, dr, MAX_ROW)?);
                            Some((sh, r1.min(r2), 0, r1.max(r2), MAX_COL))
                        })()
                    } else {
                        None
                    }
                }
                _ => None,
            };
            out.extend(rect);
            continue;
        }
        sheet = None;
        i += 1;
    }
    out
}

fn col_part(s: &str) -> Option<(u32, bool)> {
    let (abs, l) = s.strip_prefix('$').map_or((false, s), |r| (true, r));
    (l.bytes().all(|b| b.is_ascii_alphabetic())).then(|| col_of(l).map(|c| (c, abs)))?
}

fn row_part(s: &str) -> Option<(u32, bool)> {
    let (abs, d) = s.strip_prefix('$').map_or((false, s), |r| (true, r));
    if d.is_empty() || d.len() > 7 || !d.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let r: u32 = d.parse().ok()?;
    (r >= 1 && r - 1 <= MAX_ROW).then(|| (r - 1, abs))
}

fn range_of(s: &str) -> Option<(u32, u32, u32, u32)> {
    let (a, z) = s.split_once(':').unwrap_or((s, s));
    let (r1, c1, ..) = cell_ref(a)?;
    let (r2, c2, ..) = cell_ref(z)?;
    Some((r1.min(r2), c1.min(c2), r1.max(r2), c1.max(c2)))
}

/// Reads the model from a package. `None` if it is not a workbook the
/// harness can model (it then only runs the generic checks).
fn model(zip: &[u8]) -> Option<Model> {
    let mut wb = Workbook::from_bytes(zip.to_vec()).ok()?;
    let names: Vec<String> = wb.sheet_names().map(str::to_owned).collect();
    let parts: Vec<Option<String>> =
        (0..names.len()).map(|i| wb.worksheet_by_index(i).ok().map(|ws| ws.part_name().to_owned())).collect();
    let pkg = ooxml_core::Package::from_bytes(zip.to_vec()).ok()?;
    let mut m = Model { names: names.clone(), parts: parts.clone(), ..Model::default() };
    let wb_xml = XmlDocument::parse(&pkg.read_part(wb.workbook_part()).ok()?).ok()?;
    if let Some(c) = wb_xml.root().elements().find(|e| is_sml(e, "calcPr")) {
        m.manual = attr(c, "calcMode") == Some("manual");
        m.full_calc = matches!(attr(c, "fullCalcOnLoad"), Some("1" | "true"));
    }
    // The calculation chain is the workbook's calcChain relationship target
    // (a part merely named calcChain.xml is not one).
    let rels = pkg.relationships(wb.workbook_part()).ok()?;
    m.calc_chain = rels
        .by_type("http://schemas.openxmlformats.org/officeDocument/2006/relationships/calcChain")
        .filter_map(|r| rels.resolve(r))
        .find(|p| pkg.contains(p));
    let mut total = 0usize;
    for (sheet_i, part) in parts.iter().enumerate() {
        let mut cells = BTreeMap::new();
        if let Some(part) = part {
            let doc = XmlDocument::parse(&pkg.read_part(part).ok()?).ok()?;
            let sd = doc.root().elements().find(|e| is_sml(e, "sheetData"));
            // shared groups: si -> master (row, col, text); members with their si
            let mut masters: BTreeMap<String, (u32, u32, String)> = BTreeMap::new();
            let mut members: Vec<((u32, u32), String)> = Vec::new();
            for row in sd.into_iter().flat_map(RawNode::elements).filter(|e| is_sml(e, "row")) {
                for c in row.elements().filter(|e| is_sml(e, "c")) {
                    let (r, col, ..) = cell_ref(attr(c, "r")?)?;
                    let mut cell = Cell { has_v: c.elements().any(|e| is_sml(e, "v")), ..Cell::default() };
                    if let Some(f) = c.elements().find(|e| is_sml(e, "f")) {
                        cell.has_f = true;
                        total += 1;
                        let text = f.text().into_owned();
                        match attr(f, "t") {
                            Some("shared") => {
                                let group = attr(f, "si").unwrap_or("").to_owned();
                                if text.is_empty() {
                                    members.push(((r, col), group));
                                } else {
                                    masters.entry(group).or_insert((r, col, text.clone()));
                                    cell.formula = Some(text);
                                }
                            }
                            Some("array") => {
                                cell.array_ref = attr(f, "ref").and_then(range_of);
                                cell.formula = Some(text);
                            }
                            Some("dataTable") => {}
                            _ => cell.formula = Some(text),
                        }
                    }
                    cells.insert((r, col), cell);
                }
            }
            for cell in cells.values_mut() {
                if let Some(text) = &cell.formula {
                    cell.reads = references(text, sheet_i, &names, 0, 0);
                }
            }
            for ((r, col), group) in members {
                if let Some((mr, mc, text)) = masters.get(&group) {
                    let cell = cells.get_mut(&(r, col)).expect("member");
                    cell.reads = references(text, sheet_i, &names, i64::from(r) - i64::from(*mr), i64::from(col) - i64::from(*mc));
                }
            }
        }
        m.cells.push(cells);
    }
    (total <= 20_000).then_some(m)
}

impl Model {
    /// Transitive dependents of `start` the model can prove (subset of the
    /// truth): formula cells reading a changed cell, and the results of
    /// array formulas among them. `start` itself is excluded.
    fn dependents(&self, start: At) -> BTreeSet<At> {
        let mut out: BTreeSet<At> = BTreeSet::new();
        let mut frontier = vec![start];
        while let Some((sh, r, c)) = frontier.pop() {
            for (si, cells) in self.cells.iter().enumerate() {
                for (&(fr, fc), cell) in cells {
                    let hit = cell.reads.iter().any(|&(s, r1, c1, r2, c2)| s == sh && (r1..=r2).contains(&r) && (c1..=c2).contains(&c));
                    if !hit {
                        continue;
                    }
                    let mut new = vec![(si, fr, fc)];
                    if let Some((r1, c1, r2, c2)) = cell.array_ref {
                        // results of a dependent array formula (cells that exist)
                        new.extend(cells.range((r1, 0)..=(r2, MAX_COL)).filter(|((_, cc), _)| (c1..=c2).contains(cc)).map(|(&(rr, cc), _)| (si, rr, cc)));
                    }
                    for d in new {
                        if d != start && out.insert(d) {
                            frontier.push(d);
                        }
                    }
                }
            }
        }
        out
    }

    fn formula_cells(&self) -> BTreeSet<At> {
        self.cells
            .iter()
            .enumerate()
            .flat_map(|(si, cells)| cells.iter().filter(|(_, c)| c.has_f).map(move |(&(r, c), _)| (si, r, c)))
            .collect()
    }

    /// Sheets with at least one formula.
    fn formula_sheets(&self) -> BTreeSet<usize> {
        self.cells.iter().enumerate().filter(|(_, c)| c.values().any(|c| c.has_f)).map(|(i, _)| i).collect()
    }
}

/// Fuzz entry: a fixture-based input; fixtures are the xlsx fixtures and
/// [`formula_books`] (appended to the list).
pub fn check_xlsx_formula_input(data: &[u8]) {
    if let Some((zip, ops)) = decode(&FORMULA_FIXTURES, data) {
        check_xlsx_formula(&zip, ops);
    }
}

static FORMULA_FIXTURES: std::sync::LazyLock<Vec<crate::fixtures::Fixture>> = std::sync::LazyLock::new(|| {
    let mut v: Vec<crate::fixtures::Fixture> =
        XLSX.iter().map(|f| crate::fixtures::Fixture { name: f.name.clone(), parts: f.parts.clone() }).collect();
    for (name, zip) in formula_books() {
        if let Some(parts) = crate::fixtures::unzip(&zip) {
            v.push(crate::fixtures::Fixture { name: name.to_owned(), parts });
        }
    }
    v
});

/// The fixture list of this target (index = the input's first byte).
pub fn formula_fixtures() -> &'static [crate::fixtures::Fixture] {
    &FORMULA_FIXTURES
}

const FORMULA_TEXTS: &[&str] = &[
    "A1*2", "SUM(A1:B3)", "B2+C3", "Sheet2!A1+1", "'Sheet 3'!B2*2", "SUM(A:A)", "SUM(2:2)", "$A$1+B$1+$C2", "A1+", "1/0", "ROW(A1)",
    "INDIRECT(\"A1\")", "Rate*2", "\"text\"&A1", "SUM(Sheet1:Sheet2!A1)", "LEN(\"A1\")", "A1048576+XFD1", "Table1[Col]",
];

fn formula_text(ops: &mut Input<'_>) -> String {
    match ops.u8() % 4 {
        0 => ops.string(),
        1 => {
            let (r, c) = (ops.u8() % 8, ops.u8() % 6);
            format!("{}{}*{}", (b'A' + c) as char, r + 1, ops.u8())
        }
        _ => FORMULA_TEXTS[usize::from(ops.u8()) % FORMULA_TEXTS.len()].to_owned(),
    }
}

/// The cell an op targets: often a formula cell, a cell some formula reads,
/// or a shared master / array anchor or result, else a nearby cell.
fn pick_cell(m: &Model, sheet: usize, ops: &mut Input<'_>) -> (u32, u32) {
    let cells = m.cells.get(sheet);
    let k = ops.u16();
    match ops.u8() % 4 {
        0 | 1 => cells
            .filter(|c| !c.is_empty())
            .map(|c| *c.keys().nth(usize::from(k) % c.len()).expect("in range"))
            .unwrap_or((0, 0)),
        2 => {
            let reads: Vec<(u32, u32)> =
                m.cells.iter().flat_map(|c| c.values()).flat_map(|c| &c.reads).filter(|r| r.0 == sheet).map(|r| (r.1, r.2)).collect();
            reads.get(usize::from(k) % reads.len().max(1)).copied().unwrap_or((0, 0))
        }
        _ => (u32::from(ops.u8() % 12), u32::from(ops.u8() % 8)),
    }
}

fn worksheet_count(m: &Model) -> usize {
    m.parts.len()
}

/// Checks on one workbook; see the module doc.
pub fn check_xlsx_formula(zip: &[u8], mut ops: Input<'_>) {
    let Ok(mut wb) = Workbook::from_bytes(zip.to_vec()) else { return };
    let Ok(base) = wb.to_bytes() else { return };
    let Some(mut m) = model(&base) else { return };
    let refusal = wb.edit_refusal().map(|e| format!("{e:?}"));
    let mut expected_formulas = m.formula_cells();
    let mut poisoned = false;
    let mut steps = 0;
    while !ops.is_empty() && steps < 8 && worksheet_count(&m) > 0 {
        steps += 1;
        let sheet = usize::from(ops.u8()) % worksheet_count(&m);
        if m.parts[sheet].is_none() {
            continue; // not a worksheet
        }
        let (row, col) = pick_cell(&m, sheet, &mut ops);
        let kind = ops.u8() % 4;
        let (text, number) = (formula_text(&mut ops), ops.f64());
        let at = (sheet, row, col);
        let before = wb.to_bytes();
        let model_deps = m.dependents(at);
        // the library's dependents must include the model's
        if !poisoned
            && refusal.is_none()
            && let Ok(lib) = wb.dependents(&m.names[sheet], CellIndex::new(row, col).expect("in range"))
        {
            {
                let lib: BTreeSet<At> =
                    lib.iter().filter_map(|(n, c)| m.names.iter().position(|x| x == n).map(|s| (s, c.row, c.col))).collect();
                let missing: Vec<&At> = model_deps.iter().filter(|d| !lib.contains(d)).collect();
                assert!(missing.is_empty(), "Workbook::dependents({}!{row},{col}) misses {missing:?}", m.names[sheet]);
            }
        }
        let had_formula = m.cells[sheet].get(&(row, col)).is_some_and(|c| c.has_f);
        let result = {
            let Ok(mut ws) = wb.worksheet_by_index(sheet) else { continue };
            let Ok(cm) = ws.cell_mut(row, col) else { continue };
            match kind {
                0 => cm.set_number(number),
                1 => cm.set_string(&text),
                2 => cm.set_formula(&text),
                _ => cm.remove_formula(),
            }
        };
        match result {
            Err(e) => {
                bump(3, 1);
                if poisoned {
                    assert!(matches!(e, Error::Poisoned { .. }), "after Poisoned, an edit returned {e:?}");
                }
                if matches!(e, Error::Poisoned { .. }) {
                    poisoned = true;
                    assert!(wb.to_bytes().is_err(), "a poisoned workbook still saves");
                    continue;
                }
                let after = wb.to_bytes();
                assert!(
                    matches!((&before, &after), (Ok(a), Ok(b)) if a == b),
                    "refused edit {kind} at {}!({row},{col}) ({e:?}) changed the package",
                    m.names[sheet]
                );
            }
            Ok(()) => {
                assert!(!poisoned, "an edit succeeded after Poisoned");
                let noop = kind == 3 && !had_formula;
                let out = wb.to_bytes().unwrap_or_else(|e| panic!("save after an edit failed: {e:?}"));
                let Ok(before) = before else { panic!("saved after the edit but not before it") };
                if noop {
                    assert!(out == before, "remove_formula on a cell without a formula changed the package");
                    continue;
                }
                let new = model(&out).unwrap_or_else(|| panic!("edited workbook cannot be read back"));
                bump(0, 1);
                bump(1, model_deps.len());
                for &(s, r, c) in &model_deps {
                    if let Some(cell) = new.cells[s].get(&(r, c)) {
                        assert!(!cell.has_v, "dependent {}!({r},{c}) of {}!({row},{col}) kept its cached value", new.names[s], m.names[sheet]);
                    }
                }
                let has_formula_now = kind == 2;
                let set_changed = had_formula != has_formula_now;
                if (!model_deps.is_empty() || set_changed) && !m.manual {
                    let pkg = ooxml_core::Package::from_bytes(out.clone()).expect("reopens");
                    let w = XmlDocument::parse(&pkg.read_part(wb.workbook_part()).expect("workbook")).expect("parses");
                    let full = w.root().elements().find(|e| is_sml(e, "calcPr")).and_then(|c| attr(c, "fullCalcOnLoad"));
                    assert!(matches!(full, Some("1" | "true")), "calcPr/@fullCalcOnLoad not set after an edit with dependents or a formula-set change");
                }
                if let Some(chain) = &m.calc_chain {
                    bump(2, 1);
                    assert_eq!(new.calc_chain.is_some(), !set_changed, "calcChain.xml {chain} kept/removed wrongly (formula set changed: {set_changed}; edit {kind} at {}!({row},{col}), had formula {had_formula})", m.names[sheet]);
                    if new.calc_chain.is_none() {
                        let pkg = ooxml_core::Package::from_bytes(out.clone()).expect("reopens");
                        let left = pkg.content_types().overrides().iter().any(|(n, _)| n.trim_start_matches('/').eq_ignore_ascii_case(chain));
                        assert!(!left, "the Override for {chain} is left in [Content_Types].xml");
                        assert!(!pkg.contains(chain), "{chain} is still in the package");
                    }
                }
                // value read-back
                if kind <= 1 {
                    let mut ws = wb.worksheet_by_index(sheet).expect("worksheet");
                    let got = ws.cell(row, col).ok().flatten().and_then(|c| c.value().ok());
                    let want = if kind == 0 { CellValue::Number(number) } else { CellValue::String(text.clone()) };
                    assert_eq!(got.as_ref(), Some(&want), "value written at {}!({row},{col}) does not read back", m.names[sheet]);
                    let _ = &mut ws;
                }
                // formula census
                if has_formula_now {
                    expected_formulas.insert(at);
                } else {
                    expected_formulas.remove(&at);
                }
                assert_eq!(new.formula_cells(), expected_formulas, "formula cells after editing {}!({row},{col})", m.names[sheet]);
                // Layer 1 outside the sheets that may change
                let names: Vec<String> = raw_entries(&before).map(|v| v.into_iter().map(|e| e.name).collect()).unwrap_or_default();
                let touchable: BTreeSet<&str> = m
                    .formula_sheets()
                    .into_iter()
                    .chain([sheet])
                    .filter_map(|s| m.parts[s].as_deref())
                    .chain([wb.workbook_part(), "[Content_Types].xml"])
                    .collect();
                let except: Vec<&str> = names.iter().map(String::as_str).filter(|n| touchable.contains(n) || n.ends_with(".rels")).collect();
                let removable: Vec<&str> = m.calc_chain.iter().map(String::as_str).collect();
                assert_l1_except_removing(&before, &out, &except, &removable, "xlsx formula edit");
                m = new;
            }
        }
    }
    if poisoned {
        return;
    }
    let out = wb.to_bytes().unwrap_or_else(|e| panic!("final save failed: {e:?}"));
    let re = Workbook::from_bytes(out.clone()).unwrap_or_else(|e| panic!("edited workbook does not reopen: {e:?}"));
    assert_eq!(re.edit_refusal().map(|e| format!("{e:?}")), refusal, "edit refusal changed by save and reopen");
    let census = model(&out).map(|m| m.formula_cells());
    assert_eq!(census.as_ref(), Some(&expected_formulas), "formula census after reopen");
}

/// Small workbooks with the Phase1 shapes (shared groups, array formulas,
/// cross-sheet chains, a calcChain, manual mode) built from XML.
pub fn formula_books() -> Vec<(&'static str, Vec<u8>)> {
    let sheet1 = concat!(
        r#"<row r="1"><c r="A1"><v>1</v></c><c r="B1"><f>A1*2</f><v>2</v></c><c r="C1"><f t="shared" ref="C1:C3" si="0">A1+B1</f><v>3</v></c></row>"#,
        r#"<row r="2"><c r="A2"><v>2</v></c><c r="B2"><f>SUM(A1:A3)</f><v>6</v></c><c r="C2"><f t="shared" si="0"/><v>8</v></c></row>"#,
        r#"<row r="3"><c r="A3"><v>3</v></c><c r="C3"><f t="shared" si="0"/><v>9</v></c><c r="D3"><f t="array" ref="D3:E4">A1:B2*2</f><v>2</v></c><c r="E3"><v>4</v></c></row>"#,
        r#"<row r="4"><c r="D4"><v>4</v></c><c r="E4"><v>12</v></c></row>"#
    );
    let sheet2 = r#"<row r="1"><c r="A1"><f>Sheet1!B2+1</f><v>7</v></c><c r="B1"><f>A1*2</f><v>14</v></c><c r="C1"><v>5</v></c></row>"#;
    let sheet3 = r#"<row r="1"><c r="A1"><v>1</v></c><c r="B2"><f>Sheet2!B1+'Sheet 3'!A1</f><v>15</v></c></row>"#;
    let chain = r#"<calcChain xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><c r="B1" i="1"/><c r="C1"/><c r="B2"/></calcChain>"#;
    vec![
        ("formula/three-sheets-calcchain.xlsx", formula_book(&[("Sheet1", sheet1), ("Sheet2", sheet2), ("Sheet 3", sheet3)], "", Some(chain))),
        ("formula/manual.xlsx", formula_book(&[("Sheet1", sheet1), ("Sheet2", sheet2)], r#"<calcPr calcMode="manual"/>"#, None)),
        ("formula/full-calc-set.xlsx", formula_book(&[("Sheet1", sheet1)], r#"<calcPr fullCalcOnLoad="1"/>"#, Some(chain))),
        ("formula/values-only.xlsx", formula_book(&[("Sheet1", r#"<row r="1"><c r="A1"><v>1</v></c></row>"#), ("Sheet2", sheet2)], "", None)),
    ]
}

fn formula_book(sheets: &[(&str, &str)], extra_wb: &str, calc_chain: Option<&str>) -> Vec<u8> {
    let mut ct = String::from(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>"#);
    let mut rels = String::from(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#);
    let mut sh = String::new();
    let mut parts: Vec<(String, String)> = Vec::new();
    for (i, (name, sd)) in sheets.iter().enumerate() {
        let n = i + 1;
        parts.push((format!("xl/worksheets/sheet{n}.xml"), format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><worksheet xmlns="{SML}"><sheetData>{sd}</sheetData></worksheet>"#)));
        ct.push_str(&format!(r#"<Override PartName="/xl/worksheets/sheet{n}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>"#));
        rels.push_str(&format!(r#"<Relationship Id="rId{n}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet{n}.xml"/>"#));
        sh.push_str(&format!(r#"<sheet name="{name}" sheetId="{n}" r:id="rId{n}"/>"#));
    }
    if let Some(chain) = calc_chain {
        parts.push(("xl/calcChain.xml".into(), chain.into()));
        ct.push_str(r#"<Override PartName="/xl/calcChain.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.calcChain+xml"/>"#);
        rels.push_str(r#"<Relationship Id="rId99" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/calcChain" Target="calcChain.xml"/>"#);
    }
    ct.push_str("</Types>");
    rels.push_str("</Relationships>");
    let wb = format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><workbook xmlns="{SML}" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets>{sh}</sheets>{extra_wb}</workbook>"#);
    let top = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#;
    let mut all = vec![
        ("[Content_Types].xml".to_owned(), ct),
        ("_rels/.rels".to_owned(), top.to_owned()),
        ("xl/workbook.xml".to_owned(), wb),
        ("xl/_rels/workbook.xml.rels".to_owned(), rels),
    ];
    all.extend(parts);
    build_zip(all.iter().map(|(n, b)| (n.as_str(), b.as_bytes())))
}
