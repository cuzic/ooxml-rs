//! Phase1 oracles (ADR-0003 §7): exactly which bytes an edit may touch, and
//! that no reader sees a stale cached value.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Write};

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

/// The span of a dependent `c` after Phase1 stripped its cached value: the
/// start tag without its `t` attribute, the content without `v` and `is`
/// (ADR-0003 §3.3). Byte-level, from the input text.
pub fn stripped(span: &str) -> String {
    let end = start_tag_end(span);
    let (open, rest) = span.split_at(end);
    let mut open = open.to_owned();
    let had_t = attr_pos(&open, "t").is_some();
    if let Some(i) = attr_pos(&open, "t") {
        let q = open[i..].find('"').unwrap() + i;
        let close = open[q + 1..].find('"').unwrap() + q + 1;
        open.replace_range(i - 1..=close, "");
    }
    let mut rest = rest.to_owned();
    for tag in ["v", "is"] {
        while let Some(i) = find_elem(&rest, tag) {
            let j = elem_end(&rest[i..], tag) + i;
            rest.replace_range(i..j, "");
        }
    }
    // ADR-0002 §4.2: a re-rendered (own-dirty) element that lost all its
    // children becomes self-closing; otherwise the close tag stays.
    if had_t && rest.starts_with("</") && !open.ends_with("/>") {
        open.truncate(open.len() - 1);
        return open + "/>";
    }
    open + &rest
}

fn start_tag_end(s: &str) -> usize {
    let mut q = false;
    for (i, c) in s.char_indices() {
        match c {
            '"' => q = !q,
            '>' if !q => return i + 1,
            _ => {}
        }
    }
    s.len()
}

/// Byte position of ` name="` in a start tag (after the space).
fn attr_pos(tag: &str, name: &str) -> Option<usize> {
    tag.find(&format!(" {name}=\"")).map(|i| i + 1)
}

fn find_elem(s: &str, tag: &str) -> Option<usize> {
    [format!("<{tag}>"), format!("<{tag} "), format!("<{tag}/>")].iter().filter_map(|p| s.find(p.as_str())).min()
}

fn elem_end(s: &str, tag: &str) -> usize {
    let open = start_tag_end(s);
    if s[..open].ends_with("/>") {
        return open;
    }
    let close = format!("</{tag}>");
    s.find(&close).unwrap() + close.len()
}

/// Input text with every `(cell, replacement)` span replaced.
fn replace_cells(xml: &str, spans: &BTreeMap<CellIndex, (std::ops::Range<usize>, String)>, with: &BTreeMap<CellIndex, String>) -> String {
    let mut out = xml.to_owned();
    let mut edits: Vec<(std::ops::Range<usize>, String)> =
        with.iter().filter_map(|(at, new)| spans.get(at).map(|(r, _)| (r.clone(), new.clone()))).collect();
    edits.sort_by_key(|e| std::cmp::Reverse(e.0.start));
    for (r, new) in edits {
        out.replace_range(r, &new);
    }
    out
}

/// Every `c` of a sheet part: cell → (byte range, source text).
fn cell_ranges(xml: &str) -> BTreeMap<CellIndex, (std::ops::Range<usize>, String)> {
    let doc = XmlDocument::parse(xml.as_bytes()).unwrap();
    let src = doc.source();
    let mut out = BTreeMap::new();
    if let Some(sd) = sheet_data(&doc) {
        for row in sd.elements() {
            for c in row.elements().filter(|c| c.is(sml::Cell::QNAME)) {
                let s = c.span().unwrap();
                out.insert(CellIndex::parse(c.attr(sml::Cell::ATTR_R).unwrap()).unwrap(), (s.start..s.end(), src[s.start..s.end()].to_owned()));
            }
        }
    }
    out
}

/// Removes the first element matching `pat` (a start of `<Tag` and a
/// predicate on the element text) with the whitespace right before it.
fn cut_registry_element(xml: &str, tag: &str, pred: impl Fn(&str) -> bool) -> String {
    let mut from = 0;
    while let Some(i) = xml[from..].find(&format!("<{tag} ")).map(|i| i + from) {
        let j = xml[i..].find("/>").unwrap() + i + 2;
        if pred(&xml[i..j]) {
            let ws = xml[..i].len() - xml[..i].trim_end_matches([' ', '\t', '\r', '\n']).len();
            return format!("{}{}", &xml[..i - ws], &xml[j..]);
        }
        from = j;
    }
    panic!("no <{tag}> to cut");
}

/// ADR-0003 §7: the output is **exactly** the input with the allowed edits
/// applied (review R3-M2), byte for byte:
///
/// - every sheet part equals the input in which each dependent's `c` span is
///   replaced by [`stripped`] (and each `also_changed` cell by its output
///   span); on the target sheet the remaining difference must be the
///   Phase0 T2 region of the target cell plus `dimension` ([`assert_t2`]);
/// - `workbook.xml` equals the input with `fullCalcOnLoad="1"` added to
///   `calcPr` (or `<calcPr fullCalcOnLoad="1"/>` inserted);
/// - on a calcChain removal the part is gone and each registry equals the
///   input minus exactly that element and the whitespace before it;
/// - every other part is L1.
pub fn assert_allowed_diff(input: &[u8], output: &[u8], e: &Expected<'_>) {
    let parts = sheet_parts(input);
    let wb_part = Workbook::from_bytes(input.to_vec()).unwrap().workbook_part().to_owned();
    let mut changed: Vec<String> = Vec::new();
    for (sheet, p) in &parts {
        let (a, b) = (String::from_utf8(part(input, p)).unwrap(), String::from_utf8(part(output, p)).unwrap());
        let ia = cell_ranges(&a);
        let ib = cell_spans(b.as_bytes());
        let mut with = BTreeMap::new();
        for (at, (_, span)) in &ia {
            let key = (sheet.clone(), *at);
            if e.also_changed.iter().any(|s| cells(&[s]).contains(&key)) {
                with.insert(*at, ib.get(at).cloned().unwrap_or_else(|| panic!("{sheet}!{at} disappeared")));
            } else if e.dependents.contains(&key) && !(sheet.eq_ignore_ascii_case(e.sheet) && *at == e.at) {
                with.insert(*at, stripped(span));
            }
        }
        let expected = replace_cells(&a, &ia, &with);
        if sheet.eq_ignore_ascii_case(e.sheet) {
            changed.push(p.clone());
            super::assert_t2(expected.as_bytes(), b.as_bytes(), e.at);
        } else {
            assert_eq!(b, expected, "{sheet}: output must be the input with dependents stripped, nothing else");
            if a != b {
                changed.push(p.clone());
            }
        }
    }
    // workbook.xml
    let (wa, wbx) = (String::from_utf8(part(input, &wb_part)).unwrap(), String::from_utf8(part(output, &wb_part)).unwrap());
    let has = |s: &str| s.contains(r#"fullCalcOnLoad="1""#) || s.contains(r#"fullCalcOnLoad="true""#);
    if e.full_calc && !has(&wa) {
        changed.push(wb_part.clone());
        let expected = match wa.find("<calcPr") {
            Some(i) => {
                let end = start_tag_end(&wa[i..]) + i;
                let cut = if wa[..end].ends_with("/>") { end - 2 } else { end - 1 };
                format!("{} fullCalcOnLoad=\"1\"{}", &wa[..cut], &wa[cut..])
            }
            None => {
                let i = wbx.find(r#"<calcPr fullCalcOnLoad="1"/>"#).expect("calcPr inserted");
                format!("{}{}", &wbx[..i], &wbx[i + r#"<calcPr fullCalcOnLoad="1"/>"#.len()..]).pipe_check(&wa);
                wbx.clone()
            }
        };
        assert_eq!(wbx, expected, "workbook.xml: only calcPr/@fullCalcOnLoad");
    } else {
        assert_eq!(wa, wbx, "workbook.xml unchanged");
    }
    // calcChain and the other parts
    let names_in = entries(input).into_iter().map(|e| e.0).collect::<Vec<_>>();
    let chain = names_in.iter().find(|n| n.to_ascii_lowercase().ends_with("calcchain.xml")).cloned();
    if e.chain_removed && let Some(c) = &chain {
        let names_out = entries(output).into_iter().map(|e| e.0).collect::<Vec<_>>();
        assert!(!names_out.contains(c), "calcChain removed");
        let (dir, file) = wb_part.rsplit_once('/').unwrap();
        let rels = format!("{dir}/_rels/{file}.rels");
        let ct_in = String::from_utf8(part(input, "[Content_Types].xml")).unwrap();
        let ct_out = String::from_utf8(part(output, "[Content_Types].xml")).unwrap();
        assert_eq!(ct_out, cut_registry_element(&ct_in, "Override", |el| el.to_ascii_lowercase().contains("calcchain.xml")), "[Content_Types].xml: exactly one Override cut");
        let r_in = String::from_utf8(part(input, &rels)).unwrap();
        let r_out = String::from_utf8(part(output, &rels)).unwrap();
        assert_eq!(r_out, cut_registry_element(&r_in, "Relationship", |el| el.contains("/calcChain\"")), "{rels}: exactly one Relationship cut");
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

trait PipeCheck {
    fn pipe_check(self, want: &str);
}
impl PipeCheck for String {
    /// Removing the inserted `calcPr` gives the input back.
    fn pipe_check(self, want: &str) {
        assert_eq!(self, want, "workbook.xml: only the inserted calcPr differs");
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
    value_edit_with(input, sheet, at, value, expected, true)
}

/// As [`value_edit`]; with `exact == false` the computed set only has to
/// contain `expected` (real fixtures, where `Everything` formulas add more).
pub fn value_edit_with(input: &[u8], sheet: &str, at: &str, value: f64, expected: &[&str], exact: bool) -> Vec<u8> {
    let at = CellIndex::parse(at).unwrap();
    let mut wb = Workbook::from_bytes(input.to_vec()).unwrap();
    let got: BTreeSet<_> = wb.dependents(sheet, at).unwrap().into_iter().collect();
    let want = if exact {
        let want = cells(expected);
        assert_eq!(got, want, "dependents of {sheet}!{at}");
        want
    } else {
        assert!(got.is_superset(&cells(expected)), "dependents of {sheet}!{at}: {got:?}");
        got
    };
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

/// calamine's formula text per sheet and cell (shared formulas expanded by
/// calamine: an independent translator).
pub fn calamine_formulas(bytes: &[u8]) -> BTreeMap<String, BTreeMap<(u32, u32), String>> {
    use calamine::{Reader, Xlsx};
    let mut wb: Xlsx<_> = Xlsx::new(std::io::Cursor::new(bytes.to_vec())).unwrap();
    let names = wb.sheet_names().to_vec();
    let mut out = BTreeMap::new();
    for n in names {
        let Ok(r) = wb.worksheet_formula(&n) else { continue };
        let (r0, c0) = r.start().unwrap_or((0, 0));
        let mut m = BTreeMap::new();
        for (r_, c_, f) in r.used_cells() {
            if !f.is_empty() {
                m.insert((r0 + r_ as u32, c0 + c_ as u32), f.clone());
            }
        }
        out.insert(n, m);
    }
    out
}

/// Replaces one part of a package (test set-up only).
pub fn with_part(bytes: &[u8], name: &str, xml: &str) -> Vec<u8> {
    let mut pkg = ooxml_core::Package::from_bytes(bytes.to_vec()).unwrap();
    pkg.set_part(name, xml.as_bytes().to_vec()).unwrap();
    pkg.to_bytes().unwrap()
}

/// A hand-built workbook (review round 3 probes): sheets with their
/// `sheetData` inner XML, `definedName` elements, extra `workbook` children.
pub fn book(sheets: &[(&str, &str)], defined_names: &str, extra_wb: &str) -> Vec<u8> {
    let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let o = zip::write::SimpleFileOptions::default();
    let mut ct = String::from(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>"#);
    let mut rels = String::from(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#);
    let mut sh = String::new();
    let mut parts = Vec::new();
    for (i, (name, sd)) in sheets.iter().enumerate() {
        parts.push((format!("xl/worksheets/sheet{}.xml", i + 1), format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><worksheet xmlns="{SML}"><sheetData>{sd}</sheetData></worksheet>"#)));
        ct.push_str(&format!(r#"<Override PartName="/xl/worksheets/sheet{}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>"#, i + 1));
        rels.push_str(&format!(r#"<Relationship Id="rId{}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet{}.xml"/>"#, i + 1, i + 1));
        sh.push_str(&format!(r#"<sheet name="{}" sheetId="{}" r:id="rId{}"/>"#, name, i + 1, i + 1));
    }
    ct.push_str("</Types>");
    rels.push_str("</Relationships>");
    let dn = if defined_names.is_empty() { String::new() } else { format!("<definedNames>{defined_names}</definedNames>") };
    let wb = format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><workbook xmlns="{SML}" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><sheets>{sh}</sheets>{dn}{extra_wb}</workbook>"#);
    let top = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#;
    let mut all = vec![
        ("[Content_Types].xml".to_string(), ct),
        ("_rels/.rels".to_string(), top.to_string()),
        ("xl/workbook.xml".to_string(), wb),
        ("xl/_rels/workbook.xml.rels".to_string(), rels),
    ];
    all.extend(parts);
    for (n, b) in all {
        zw.start_file(n, o).unwrap();
        zw.write_all(b.as_bytes()).unwrap();
    }
    zw.finish().unwrap().into_inner()
}

