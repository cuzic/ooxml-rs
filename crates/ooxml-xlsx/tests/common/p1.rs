//! Phase1 oracles (ADR-0003 §7): exactly which bytes an edit may touch, and
//! that no reader sees a stale cached value.

use std::collections::{BTreeMap, BTreeSet};

use calamine::Data;
use ooxml_core::{ElementSpec, RawNode, XmlDocument};
use ooxml_schema::sml;
use ooxml_xlsx::{CellIndex, Workbook};

use super::{assert_l1_except, calamine_cells, entries, part, read, try_calamine_cells};

pub const SML: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";

pub fn synthetic(name: &str) -> Vec<u8> {
    read(&format!("synthetic/{name}"))
}

/// (sheet name, cell) set from `["Sheet1!B1", …]`.
pub fn cells(list: &[&str]) -> BTreeSet<(String, CellIndex)> {
    list.iter()
        .map(|s| {
            let (sh, c) = s.rsplit_once('!').unwrap();
            (sh.trim_matches('\'').to_owned(), CellIndex::parse(c).unwrap())
        })
        .collect()
}

/// Sheet name → worksheet part, from the workbook.
pub fn sheet_parts(bytes: &[u8]) -> BTreeMap<String, String> {
    let mut wb = Workbook::from_bytes(bytes.to_vec()).unwrap();
    let names: Vec<String> = wb.sheet_names().map(str::to_owned).collect();
    let mut out = BTreeMap::new();
    for n in names {
        if let Ok(ws) = wb.worksheet_by_name(&n) {
            out.insert(n, ws.part_name().to_owned());
        }
    }
    out
}

fn sheet_data(doc: &XmlDocument) -> Option<&RawNode> {
    doc.root().elements().find(|e| e.is(sml::SheetData::QNAME))
}

/// Every `c` of a sheet part: cell → its original source text.
pub fn cell_spans(xml: &[u8]) -> BTreeMap<CellIndex, String> {
    let doc = XmlDocument::parse(xml).unwrap();
    let src = doc.source();
    let mut out = BTreeMap::new();
    if let Some(sd) = sheet_data(&doc) {
        for row in sd.elements() {
            for c in row.elements().filter(|c| c.is(sml::Cell::QNAME)) {
                let s = c.span().unwrap();
                out.insert(CellIndex::parse(c.attr(sml::Cell::ATTR_R).unwrap()).unwrap(), src[s.start..s.end()].to_owned());
            }
        }
    }
    out
}

/// Source text outside `sheetData` (and outside `dimension`).
fn frame(xml: &[u8]) -> (String, String) {
    let doc = XmlDocument::parse(xml).unwrap();
    let src = doc.source();
    let sd = sheet_data(&doc).unwrap().span().unwrap();
    let mut before = src[..sd.start].to_owned();
    if let Some(d) = doc.root().elements().find(|e| e.is(sml::SheetDimension::QNAME)) {
        let s = d.span().unwrap();
        before = format!("{}<dimension/>{}", &src[..s.start], &src[s.end()..sd.start]);
    }
    (before, src[sd.end()..].to_owned())
}

/// Parsed facts of one output `c` span.
pub struct CellFacts {
    pub f: Option<String>,
    pub has_v: bool,
    pub t: Option<String>,
    pub has_is: bool,
}

pub fn facts(span: &str) -> CellFacts {
    let doc = XmlDocument::parse(span.as_bytes()).unwrap_or_else(|_| {
        XmlDocument::parse(format!("<c xmlns=\"{SML}\"{}", &span[2..]).as_bytes()).unwrap()
    });
    let c = doc.root();
    let child = |l: &'static str| c.elements().find(|e| e.name().local() == l);
    CellFacts {
        f: child("f").map(|f| f.text().into_owned()),
        has_v: child("v").is_some(),
        t: c.attrs().iter().find(|a| a.name().local() == "t").map(|a| a.value().to_owned()),
        has_is: child("is").is_some(),
    }
}

/// The `f` element's source text of a span.
pub fn f_text(span: &str) -> Option<String> {
    let i = span.find("<f")?;
    let rest = &span[i..];
    let end = rest.find("</f>").map(|e| e + 4).or_else(|| rest.find("/>").map(|e| e + 2))?;
    Some(rest[..end].to_owned())
}

/// What an edit is expected to have done, checked on the output bytes.
pub struct Expected<'a> {
    pub sheet: &'a str,
    pub at: CellIndex,
    /// Dependents (formula and result cells) by hand.
    pub dependents: BTreeSet<(String, CellIndex)>,
    /// `fullCalcOnLoad` must be set in the output.
    pub full_calc: bool,
    /// `calcChain.xml` must be gone (formula set changed).
    pub chain_removed: bool,
    /// Cells other than the target whose `c` span may change for another
    /// reason (shared-formula hand-off).
    pub also_changed: &'a [&'a str],
}

/// ADR-0003 §7: the allowed diff and nothing else.
///
/// - every part not named below is L1;
/// - in each sheet, every `c` other than the target, the dependents and
///   `also_changed` keeps its bytes; the frame outside `sheetData` is
///   unchanged (but `dimension`);
/// - every dependent keeps its `f` element verbatim and has no `v`, `t`, `is`;
/// - `workbook.xml` changes only in `calcPr/@fullCalcOnLoad`;
/// - `calcChain.xml` is L1, or gone with one registry element each.
pub fn assert_allowed_diff(input: &[u8], output: &[u8], e: &Expected<'_>) {
    let parts = sheet_parts(input);
    let wb_part = Workbook::from_bytes(input.to_vec()).unwrap().workbook_part().to_owned();
    let mut changed: Vec<String> = Vec::new();
    for (sheet, p) in &parts {
        let (a, b) = (part(input, p), part(output, p));
        if a == b {
            continue;
        }
        changed.push(p.clone());
        assert_eq!(frame(&a), frame(&b), "{sheet}: bytes outside sheetData changed");
        let (ia, ib) = (cell_spans(&a), cell_spans(&b));
        for (at, span) in &ia {
            let key = (sheet.clone(), *at);
            let is_target = sheet.eq_ignore_ascii_case(e.sheet) && *at == e.at;
            let also = e.also_changed.iter().any(|s| cells(&[s]).contains(&key));
            if is_target || also {
                continue;
            }
            let out = ib.get(at).unwrap_or_else(|| panic!("{sheet}!{at} disappeared"));
            if e.dependents.contains(&key) {
                let f = facts(out);
                assert!(!f.has_v && f.t.is_none() && !f.has_is, "{sheet}!{at}: dependent keeps a cached value: {out}");
                assert_eq!(f_text(out), f_text(span), "{sheet}!{at}: f must be untouched");
            } else {
                assert_eq!(out, span, "{sheet}!{at}: a non-dependent changed");
            }
        }
        for at in ib.keys() {
            let new = !ia.contains_key(at);
            if new {
                assert!(sheet.eq_ignore_ascii_case(e.sheet) && *at == e.at, "{sheet}!{at}: unexpected new cell");
            }
        }
    }
    // workbook.xml
    let (wa, wbx) = (String::from_utf8(part(input, &wb_part)).unwrap(), String::from_utf8(part(output, &wb_part)).unwrap());
    if e.full_calc {
        if wa.contains("fullCalcOnLoad=\"1\"") {
            assert_eq!(wa, wbx);
        } else {
            changed.push(wb_part.clone());
            assert!(wbx.contains("fullCalcOnLoad=\"1\""), "fullCalcOnLoad set");
            let strip = |s: &str| s.replace(" fullCalcOnLoad=\"1\"", "").replace("<calcPr/>", "");
            assert_eq!(strip(&wa), strip(&wbx), "workbook.xml: only calcPr changes");
        }
    } else {
        assert_eq!(wa, wbx, "workbook.xml unchanged");
    }
    // calcChain
    let names_in = entries(input).into_iter().map(|e| e.0).collect::<Vec<_>>();
    let chain = names_in.iter().find(|n| n.ends_with("calcChain.xml")).cloned();
    if e.chain_removed && let Some(c) = &chain {
        let names_out = entries(output).into_iter().map(|e| e.0).collect::<Vec<_>>();
        assert!(!names_out.contains(c), "calcChain removed");
        let rels = format!("{}/_rels/{}.rels", wb_part.rsplit_once('/').unwrap().0, wb_part.rsplit_once('/').unwrap().1);
        for reg in ["[Content_Types].xml", rels.as_str()] {
            let (a, b) = (String::from_utf8(part(input, reg)).unwrap(), String::from_utf8(part(output, reg)).unwrap());
            let removed = a.len() - b.len();
            assert!(removed > 0 && b.len() < a.len(), "{reg} lost one element");
            assert!(a.contains("calcChain") && !b.contains("calcChain"), "{reg}: only the calcChain entry goes");
            assert_eq!(a.matches("<Relationship ").count() + a.matches("<Override ").count(),
                b.matches("<Relationship ").count() + b.matches("<Override ").count() + 1, "{reg}: exactly one element");
        }
        // every other part L1 (compare on the remaining entries)
        let a: Vec<_> = entries(input).into_iter().filter(|x| &x.0 != c).collect();
        let b = entries(output);
        assert_eq!(a.len(), b.len());
        for (x, y) in a.iter().zip(&b) {
            let skip = changed.contains(&x.0) || x.0 == "[Content_Types].xml" || x.0 == rels;
            if !skip {
                assert!(x == y, "{} must be L1", x.0);
            }
        }
    } else {
        let refs: Vec<&str> = changed.iter().map(String::as_str).collect();
        assert_l1_except(input, output, &refs);
    }
}

/// Read-back (ADR-0003 §7.2 item 10): calamine reads every dependent as
/// empty, the target as written, and every other cell as before. So no
/// stale value is observable.
pub fn assert_no_stale(input: &[u8], output: &[u8], e: &Expected<'_>, target: Option<&Data>) {
    // calamine cannot open every input (e.g. a workbook with an XLM macro
    // sheet); then there is no independent reader to check with.
    let Some(a) = try_calamine_cells(input) else { return };
    let b = calamine_cells(output);
    for (sheet, cells_a) in &a {
        let cells_b = &b[sheet];
        let keys: BTreeSet<_> = cells_a.keys().chain(cells_b.keys()).collect();
        for k in keys {
            let at = CellIndex { row: k.0, col: k.1 };
            let key = (sheet.clone(), at);
            let (va, vb) = (cells_a.get(k), cells_b.get(k));
            if sheet.eq_ignore_ascii_case(e.sheet) && at == e.at {
                if let Some(t) = target {
                    assert_eq!(vb, Some(t), "{sheet}!{at}: target read-back");
                }
                continue;
            }
            if e.also_changed.iter().any(|s| cells(&[s]).contains(&key)) {
                assert_eq!(va, vb, "{sheet}!{at}: hand-off keeps the value");
                continue;
            }
            if e.dependents.contains(&key) {
                assert!(matches!(vb, None | Some(Data::Empty)), "{sheet}!{at}: dependent must read empty, reads {vb:?}");
            } else {
                assert_eq!(va, vb, "{sheet}!{at}: non-dependent must read as before");
            }
        }
    }
}

/// Writes `value` into `sheet!at`, checks the computed dependents equal
/// `expected` (hand-written), then every oracle. Returns the output.
pub fn value_edit(input: &[u8], sheet: &str, at: &str, value: f64, expected: &[&str]) -> Vec<u8> {
    let at = CellIndex::parse(at).unwrap();
    let mut wb = Workbook::from_bytes(input.to_vec()).unwrap();
    let want = cells(expected);
    let got: BTreeSet<_> = wb.dependents(sheet, at).unwrap().into_iter().collect();
    assert_eq!(got, want, "dependents of {sheet}!{at}");
    let had_full = String::from_utf8(part(input, wb.workbook_part())).unwrap().contains("fullCalcOnLoad=\"1\"");
    let manual = String::from_utf8(part(input, wb.workbook_part())).unwrap().contains("calcMode=\"manual\"");
    wb.worksheet_by_name(sheet).unwrap().cell_mut(at.row, at.col).unwrap().set_number(value).unwrap();
    let out = wb.to_bytes().unwrap();
    let e = Expected {
        sheet,
        at,
        dependents: want.clone(),
        full_calc: (!want.is_empty() && !manual) || had_full,
        chain_removed: false,
        also_changed: &[],
    };
    assert_allowed_diff(input, &out, &e);
    assert_no_stale(input, &out, &e, Some(&Data::Float(value)));
    // Re-open: the result is a valid workbook for us, with no edit refusal.
    let again = Workbook::from_bytes(out.clone()).unwrap();
    assert!(again.edit_refusal().is_none());
    out
}
