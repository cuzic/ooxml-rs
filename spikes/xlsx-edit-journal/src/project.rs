//! The projection P of oracle 1 (reduced to what keyed ops reach) and the
//! validity predicates that replace its exclusions.

use std::collections::{BTreeMap, BTreeSet};

use ooxml_core::{Package, QName, RawNode, XmlDocument};
use ooxml_xlsx::{CellValue, FormatRead, Workbook};

use crate::op::CellKey;

#[derive(Clone, Debug, PartialEq)]
pub struct CellM {
    /// `None` for a formula cell (its value is in the cache exclusion).
    pub value: Option<CellValue>,
    pub formula: Option<String>,
    /// The `f` element's attributes (normal / shared / array).
    pub f_attrs: Vec<(String, String)>,
    /// The resolved style, by content.
    pub style: FormatRead,
}

#[derive(Debug, PartialEq)]
pub struct Model {
    pub cells: BTreeMap<(usize, u32, u32), CellM>,
    /// Row attributes except `spans`.
    pub rows: BTreeMap<(usize, u32), Vec<(String, String)>>,
    /// Each worksheet without `dimension` and `sheetData`, as bytes.
    pub ws_rest: Vec<String>,
    /// Every other part, byte-for-byte, except the excluded registries
    /// (`sharedStrings`, `styles`), `calcChain` and the calc flag.
    pub parts: BTreeMap<String, Vec<u8>>,
}

fn local<'a>(n: &'a RawNode, name: &str) -> Option<&'a RawNode> {
    n.elements().find(|e| e.name().local() == name)
}
fn attr<'a>(n: &'a RawNode, name: &'static str) -> Option<&'a str> {
    n.attr(QName::new("", name))
}

pub fn worksheet_parts(bytes: &[u8]) -> Vec<String> {
    let mut wb = Workbook::from_bytes(bytes.to_vec()).expect("open");
    let n = wb.sheet_names().count();
    (0..n).map(|i| wb.worksheet_by_index(i).expect("worksheet").part_name().to_owned()).collect()
}

fn part_str(pkg: &Package, name: &str) -> String {
    String::from_utf8(pkg.read_part(name).expect("part")).expect("utf8")
}

fn cut(s: &str, open: &str, close: &str) -> String {
    let Some(a) = s.find(open) else { return s.to_owned() };
    let rest = &s[a..];
    let end = if let Some(e) = rest.find("/>").filter(|&e| !rest[..e].contains('>')) { e + 2 } else { rest.find(close).map_or(rest.len(), |e| e + close.len()) };
    format!("{}{}", &s[..a], &s[a + end..])
}

/// Removes every element mentioning `calcChain` (content types, rels).
fn drop_calc_chain(s: &str) -> String {
    let mut s = s.to_owned();
    while let Some(i) = s.find("calcChain") {
        let a = s[..i].rfind('<').expect("tag start");
        let b = i + s[i..].find('>').expect("tag end") + 1;
        s.replace_range(a..b, "");
    }
    s
}

pub fn model(bytes: &[u8]) -> Model {
    let pkg = Package::from_bytes(bytes.to_vec()).expect("package");
    let mut wb = Workbook::from_bytes(bytes.to_vec()).expect("open");
    let parts = worksheet_parts(bytes);
    let mut cells = BTreeMap::new();
    let mut raw_cells = Vec::new();
    for i in 0..parts.len() {
        let ws = wb.worksheet_by_index(i).expect("ws");
        for c in ws.cells() {
            let c = c.expect("cell");
            let at = ooxml_xlsx::CellIndex::parse(c.reference()).expect("ref");
            let formula = c.formula();
            let f_attrs: Vec<(String, String)> = local(c.raw(), "f").map(|f| f.attrs().iter().map(|a| (a.name().local().to_owned(), a.value().to_owned())).collect()).unwrap_or_default();
            raw_cells.push(((i, at.row, at.col), if formula.is_some() { None } else { Some(c.value().expect("value")) }, formula, f_attrs, c.style_index().expect("s")));
        }
    }
    // shared members expanded to their own text (oracle 1's P)
    let mut masters: BTreeMap<(usize, String), (u32, u32, String)> = BTreeMap::new();
    for ((i, r, c), _, formula, f_attrs, _) in &raw_cells {
        let get = |n: &str| f_attrs.iter().find(|(a, _)| a == n).map(|(_, v)| v.clone());
        if get("t").as_deref() == Some("shared") && get("ref").is_some() && let (Some(si), Some(f)) = (get("si"), formula) {
            masters.insert((*i, si), (*r, *c, f.clone()));
        }
    }
    let raw_cells: Vec<_> = raw_cells
        .into_iter()
        .map(|((i, r, c), v, formula, f_attrs, s)| {
            let get = |n: &str| f_attrs.iter().find(|(a, _)| a == n).map(|(_, v)| v.clone());
            let formula = match (&formula, get("t").as_deref(), get("si")) {
                (Some(f), Some("shared"), Some(si)) if f.is_empty() => masters.get(&(i, si)).map(|(mr, mc, t)| translate(t, i64::from(r) - i64::from(*mr), i64::from(c) - i64::from(*mc))).or(formula),
                _ => formula,
            };
            ((i, r, c), v, formula, f_attrs, s)
        })
        .collect();
    let mut arrays: Vec<(usize, ooxml_xlsx::CellIndex, ooxml_xlsx::CellIndex)> = Vec::new();
    for ((i, _, _), _, _, f_attrs, _) in &raw_cells {
        let get = |n: &str| f_attrs.iter().find(|(a, _)| a == n).map(|(_, v)| v.clone());
        if get("t").as_deref() == Some("array") && let Some(r) = get("ref") {
            let (a, b) = r.split_once(':').unwrap_or((&r, &r));
            arrays.push((*i, ooxml_xlsx::CellIndex::parse(a).expect("ref"), ooxml_xlsx::CellIndex::parse(b).expect("ref")));
        }
    }
    for (k, value, formula, f_attrs, s) in raw_cells {
        let in_array = arrays.iter().any(|(i, a, b)| *i == k.0 && (a.row..=b.row).contains(&k.1) && (a.col..=b.col).contains(&k.2));
        let value = if in_array && formula.is_none() { None } else { value };
        let style = match wb.format_of_style(s).expect("style") {
            FormatRead::NotExpressible { reasons, .. } => FormatRead::NotExpressible { style_index: 0, reasons },
            e => e,
        };
        cells.insert(k, CellM { value, formula, f_attrs, style });
    }
    let mut rows = BTreeMap::new();
    let mut ws_rest = Vec::new();
    for (i, p) in parts.iter().enumerate() {
        let src = part_str(&pkg, p);
        let doc = XmlDocument::parse(src.as_bytes()).expect("xml");
        if let Some(sd) = local(doc.root(), "sheetData") {
            for r in sd.elements() {
                let n: u32 = attr(r, "r").expect("r").parse().expect("r");
                let a = r.attrs().iter().filter(|a| a.name().local() != "spans").map(|a| (a.name().local().to_owned(), a.value().to_owned())).collect();
                rows.insert((i, n), a);
            }
        }
        let rest = cut(&cut(&src, "<dimension", "</dimension>"), "<sheetData", "</sheetData>");
        ws_rest.push(rest);
    }
    let mut other = BTreeMap::new();
    for name in pkg.part_names() {
        if parts.iter().any(|p| p == name) || ["xl/sharedStrings.xml", "xl/styles.xml", "xl/calcChain.xml"].contains(&name) {
            continue;
        }
        let mut b = pkg.read_part(name).expect("part");
        if name == "xl/workbook.xml" || name == "[Content_Types].xml" || name.ends_with(".rels") {
            let s = drop_calc_chain(&String::from_utf8(b).expect("utf8")).replace(r#" fullCalcOnLoad="1""#, "");
            b = s.into_bytes();
        }
        other.insert(name.to_owned(), b);
    }
    Model { cells, rows, ws_rest, parts: other }
}

/// Raw facts a predicate reads from one package.
struct Raw {
    pkg: Package,
    parts: Vec<String>,
    docs: Vec<XmlDocument>,
}

impl Raw {
    fn new(bytes: &[u8]) -> Raw {
        let pkg = Package::from_bytes(bytes.to_vec()).expect("package");
        let parts = worksheet_parts(bytes);
        let docs = parts.iter().map(|p| XmlDocument::parse(part_str(&pkg, p).as_bytes()).expect("xml")).collect();
        Raw { pkg, parts, docs }
    }
    fn cells(&self, i: usize) -> Vec<&RawNode> {
        local(self.docs[i].root(), "sheetData").map(|sd| sd.elements().flat_map(|r| r.elements()).collect()).unwrap_or_default()
    }
    fn dimension(&self, i: usize) -> Option<String> {
        local(self.docs[i].root(), "dimension").map(|d| attr(d, "ref").unwrap_or("").to_owned())
    }
    /// formula cells → their `v` text
    /// Formula cells and array result cells (their values are caches too)
    /// → their `v` text.
    fn formulas(&self) -> BTreeMap<(usize, String), Option<String>> {
        let mut out = BTreeMap::new();
        for i in 0..self.parts.len() {
            let arrays = array_ranges(&self.cells(i));
            for c in self.cells(i) {
                let r = attr(c, "r").unwrap_or("");
                let in_array = ooxml_xlsx::CellIndex::parse(r).is_ok_and(|at| arrays.iter().any(|(a, b)| (a.row..=b.row).contains(&at.row) && (a.col..=b.col).contains(&at.col)));
                if local(c, "f").is_some() || in_array {
                    out.insert((i, r.to_owned()), local(c, "v").map(|v| v.text().into_owned()));
                }
            }
        }
        out
    }
}

/// Formula cells of `base` with a cached value that `out` stripped.
fn array_ranges(cells: &[&RawNode]) -> Vec<(ooxml_xlsx::CellIndex, ooxml_xlsx::CellIndex)> {
    cells
        .iter()
        .filter_map(|c| {
            let f = local(c, "f")?;
            if attr(f, "t") != Some("array") {
                return None;
            }
            let r = attr(f, "ref")?;
            let (a, b) = r.split_once(':').unwrap_or((r, r));
            Some((ooxml_xlsx::CellIndex::parse(a).ok()?, ooxml_xlsx::CellIndex::parse(b).ok()?))
        })
        .collect()
}

pub fn strip_set(base: &[u8], out: &[u8]) -> BTreeSet<(usize, String)> {
    let (b, o) = (Raw::new(base).formulas(), Raw::new(out).formulas());
    b.into_iter().filter(|(k, v)| v.is_some() && o.get(k).is_some_and(|ov| ov.is_none())).map(|(k, _)| k).collect()
}

fn covers(dim: &str, k: CellKey) -> bool {
    let (a, b) = dim.split_once(':').unwrap_or((dim, dim));
    let (Ok(a), Ok(b)) = (ooxml_xlsx::CellIndex::parse(a), ooxml_xlsx::CellIndex::parse(b)) else { return false };
    (a.row..=b.row).contains(&k.row) && (a.col..=b.col).contains(&k.col)
}

/// The validity predicates of oracle 1's exclusions, for one side, with no
/// structural op (the Base is its own aligned Base).
pub fn predicates(base: &[u8], out: &[u8], written: &[CellKey], journal_side: bool) -> Result<(), String> {
    predicates_aligned(base, &Aligned { bytes: Some(base.to_vec()), structural_sheets: Vec::new(), chain_allowed: true }, out, written, &[], journal_side)
}

/// What the predicates compare with when structural ops ran: the probe (the
/// Base with only the log's structural ops, `KeepStaleCaches`), whose cells
/// sit at the output's positions and keep the Base's caches. `None` if the
/// probe refused an op (those cases skip the aligned checks and are
/// counted).
pub struct Aligned {
    pub bytes: Option<Vec<u8>>,
    /// Sheets a structural op of the log edited.
    pub structural_sheets: Vec<usize>,
    /// No structural op met its calcChain condition (on the probe).
    pub chain_allowed: bool,
}

/// The predicates against `al`. `targets`: the cells the side's keyed ops
/// targeted, at final positions (the journal side's `dimension` clause).
pub fn predicates_aligned(base: &[u8], al: &Aligned, out: &[u8], written: &[CellKey], targets: &[CellKey], journal_side: bool) -> Result<(), String> {
    let (b, o) = (Raw::new(base), Raw::new(out));
    let a = al.bytes.as_deref().map(Raw::new);
    // sst
    let sst = XmlDocument::parse(&o.pkg.read_part("xl/sharedStrings.xml").expect("sst")).expect("xml");
    let items = sst.root().elements().count();
    let mut refs = 0i64;
    for i in 0..o.parts.len() {
        for c in o.cells(i) {
            if attr(c, "t") == Some("s") {
                refs += 1;
                let v: usize = local(c, "v").expect("v").text().parse().expect("index");
                if v >= items {
                    return Err(format!("sst index {v} out of range"));
                }
            }
        }
    }
    if attr(sst.root(), "uniqueCount") != Some(&items.to_string()) {
        return Err(format!("sst counts: {:?}/{:?} vs {refs}/{items}", attr(sst.root(), "count"), attr(sst.root(), "uniqueCount")));
    }
    // Journal side only: `@count` = the Base's count + (t="s" cells now −
    // t="s" cells in the Base), counted over every sheet (R4; review round 7,
    // m2). The sequential side is not held to it (F4).
    if journal_side {
        let bsst = XmlDocument::parse(&b.pkg.read_part("xl/sharedStrings.xml").expect("sst")).expect("xml");
        let base_refs: i64 = (0..b.parts.len()).map(|i| b.cells(i).iter().filter(|c| attr(c, "t") == Some("s")).count() as i64).sum();
        let want = attr(bsst.root(), "count").map(|c| c.parse::<i64>().expect("count") + refs - base_refs);
        if attr(sst.root(), "count").map(|c| c.parse::<i64>().expect("count")) != want {
            return Err(format!("sst/@count {:?}, want {want:?} (t=s cells {refs}, Base {base_refs})", attr(sst.root(), "count")));
        }
    }
    // styles
    let st = XmlDocument::parse(&o.pkg.read_part("xl/styles.xml").expect("styles")).expect("xml");
    let xfs = local(st.root(), "cellXfs").expect("cellXfs").elements().count() as u32;
    for i in 0..o.parts.len() {
        for c in o.cells(i) {
            if attr(c, "s").map_or(0, |s| s.parse::<u32>().expect("s")) >= xfs {
                return Err("s out of range".into());
            }
        }
    }
    // dimension
    let (mo, ma) = (model(out), al.bytes.as_deref().map(model));
    for i in 0..o.parts.len() {
        let (bd, od) = (b.dimension(i), o.dimension(i));
        if bd.is_some() != od.is_some() {
            return Err(format!("sheet {i}: dimension presence changed"));
        }
        let mine: Vec<CellKey> = written.iter().copied().filter(|k| k.sheet == i).collect();
        if let Some(od) = &od {
            for k in &mine {
                if !covers(od, *k) {
                    return Err(format!("sheet {i}: dimension {od} misses written {k:?}"));
                }
            }
            // Journal side, independent of the changed set (review round 8,
            // M3): a targeted cell whose value, type, style, presence or
            // formula-ness differs from the aligned Base cell is covered
            if journal_side && let Some(ma) = &ma {
                for k in targets.iter().filter(|k| k.sheet == i) {
                    let key = (k.sheet, k.row, k.col);
                    let (x, y) = (mo.cells.get(&key), ma.cells.get(&key));
                    let differs = match (x, y) {
                        (Some(x), Some(y)) => x.value != y.value || x.style != y.style || x.formula.is_some() != y.formula.is_some(),
                        (None, None) => false,
                        _ => true,
                    };
                    if differs && !covers(od, *k) {
                        return Err(format!("sheet {i}: dimension {od} misses changed target {k:?}"));
                    }
                }
            }
        }
        if journal_side && mine.is_empty() {
            let bytes = |r: &Raw| local(r.docs[i].root(), "dimension").map(|d| { let sp = d.span().expect("span"); r.docs[i].source()[sp.start..sp.end()].to_owned() });
            // a sheet with no changed cell keeps the Base's element, as the
            // structural ops rewrote it (the probe's)
            let reference = if al.structural_sheets.is_empty() { Some(bytes(&b)) } else { a.as_ref().map(bytes) };
            if let Some(reference) = reference
                && reference != bytes(&o)
            {
                return Err(format!("sheet {i}: dimension bytes changed without a written cell: {reference:?} -> {:?}", bytes(&o)));
            }
        }
    }
    // calcChain
    if o.pkg.contains("xl/calcChain.xml") {
        if !al.chain_allowed {
            return Err("calcChain kept although a structural op moved or deleted a formula cell".into());
        }
        if let Some(a) = &a {
            let (af, of) = (a.formulas(), o.formulas());
            let same_set = af.keys().collect::<Vec<_>>() == of.keys().collect::<Vec<_>>();
            if !same_set || o.pkg.read_part("xl/calcChain.xml").ok() != b.pkg.read_part("xl/calcChain.xml").ok() {
                return Err("calcChain kept although the formula set changed".into());
            }
        }
    }
    let Some(aligned) = al.bytes.as_deref() else { return Ok(()) };
    let a = a.expect("aligned");
    // caches, first half: a kept `v` is the Base's (for that cell id)
    let (af, of) = (a.formulas(), o.formulas());
    for (k, v) in &of {
        if let Some(v) = v
            && af.get(k) != Some(&Some(v.clone()))
        {
            return Err(format!("kept cache {v:?} at {k:?} is not the Base's"));
        }
    }
    // caches, second half (review round 7, B1): no cell a kept `v` reads
    // (ADR-0003 dependents, transitively) differs in content from the Base
    if !cfg!(feature = "mutant_half_cache_predicate") {
        stale_cache_check(aligned, out)?;
    }
    // ADR feedback F5: `main` sets `fullCalcOnLoad` when it strips a
    // dependent's cache, not when it writes a formula or strips the written
    // cell's own cache; the predicate is checked in that weaker form.
    let written_refs: BTreeSet<(usize, String)> = written.iter().map(|k| (k.sheet, ooxml_xlsx::CellIndex { row: k.row, col: k.col }.to_string())).collect();
    let stripped = strip_set(aligned, out).iter().any(|k| !written_refs.contains(k));
    let wbx = String::from_utf8(o.pkg.read_part("xl/workbook.xml").expect("wb")).expect("utf8");
    if stripped && !wbx.contains(r#"fullCalcOnLoad="1""#) {
        return Err("cache stripped without fullCalcOnLoad".into());
    }
    Ok(())
}

/// Cells (final positions) that hold `f` (or are array results) and a
/// cached value in `out`.
pub fn cached_in(out: &[u8]) -> BTreeSet<CellKey> {
    Raw::new(out)
        .formulas()
        .into_iter()
        .filter(|(_, v)| v.is_some())
        .map(|((i, r), _)| {
            let at = ooxml_xlsx::CellIndex::parse(&r).expect("r");
            CellKey::new(i, at.row, at.col)
        })
        .collect()
}

/// A cell's content for "differs from the Base": the formula text (shared
/// members expanded), or the value with its type. A missing cell reads as an
/// empty one. Style is not content: an input whose only change is its style
/// makes no cache stale unless the workbook is style-sensitive (which the
/// fixtures are not; see README "Revision 8 verification", a).
pub fn content_map(bytes: &[u8]) -> BTreeMap<(usize, u32, u32), (Option<String>, Option<CellValue>)> {
    model(bytes).cells.into_iter().map(|(k, c)| (k, (c.formula, c.value))).collect()
}

fn empty_content() -> (Option<String>, Option<CellValue>) {
    (None, Some(CellValue::Empty))
}

/// The cells whose content differs between two outputs at the same position.
pub fn content_diff(a: &[u8], b: &[u8]) -> BTreeSet<(usize, u32, u32)> {
    let (ma, mb) = (content_map(a), content_map(b));
    let keys: BTreeSet<_> = ma.keys().chain(mb.keys()).copied().collect();
    keys.into_iter()
        .filter(|k| {
            let ca = ma.get(k).cloned().unwrap_or_else(empty_content);
            let cb = mb.get(k).cloned().unwrap_or_else(empty_content);
            let norm = |c: (Option<String>, Option<CellValue>)| if c.1 == Some(CellValue::String(String::new())) { empty_content() } else { c };
            norm(ca) != norm(cb)
        })
        .collect()
}

/// Every formula cell of `out` that holds a cached value which is the Base's
/// (`base` aligned by position: the Base itself, or the probe with only the
/// structural ops applied) while a cell it reads, transitively, differs in
/// content from `base`; or whose own content differs. The second half of
/// oracle 1's cache predicate.
pub fn stale_cache_check(base: &[u8], out: &[u8]) -> Result<(), String> {
    let diff = content_diff(base, out);
    if diff.is_empty() {
        return Ok(());
    }
    let mut wb = Workbook::from_bytes(out.to_vec()).expect("open");
    let names: Vec<String> = wb.sheet_names().map(str::to_owned).collect();
    let mut stale: BTreeSet<(usize, u32, u32)> = BTreeSet::new();
    for &(s, r, c) in &diff {
        for (sh, at) in wb.dependents(&names[s], ooxml_xlsx::CellIndex { row: r, col: c }).expect("dependents") {
            stale.insert((names.iter().position(|n| *n == sh).expect("sheet"), at.row, at.col));
        }
    }
    let o = Raw::new(out);
    for ((i, r), v) in o.formulas() {
        {
            if v.is_none() {
                continue;
            }
            let at = ooxml_xlsx::CellIndex::parse(&r).expect("r");
            let key = (i, at.row, at.col);
            if stale.contains(&key) {
                return Err(format!("stale cache kept at {key:?}: an input differs from the Base"));
            }
            if diff.contains(&key) {
                return Err(format!("cache kept at {key:?} although its formula differs from the Base"));
            }
        }
    }
    Ok(())
}

/// Shifts the relative parts of the A1 references in `text` by `(dr, dc)`:
/// a stand-in for the crate's private shared-formula translation, enough for
/// the fixtures' formulas (plain references, no names, no sheet prefixes
/// that look like references).
pub fn translate(text: &str, dr: i64, dc: i64) -> String {
    let b = text.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    while i < b.len() {
        let start = i;
        let prev_ok = start == 0 || !(b[start - 1].is_ascii_alphanumeric() || b[start - 1] == b'_' || b[start - 1] == b'.');
        let mut j = i;
        let col_abs = j < b.len() && b[j] == b'$';
        if col_abs {
            j += 1;
        }
        let cs = j;
        while j < b.len() && b[j].is_ascii_uppercase() && j - cs < 3 {
            j += 1;
        }
        let ce = j;
        let row_abs = j < b.len() && b[j] == b'$';
        if row_abs {
            j += 1;
        }
        let rs = j;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        let re = j;
        let next_ok = j >= b.len() || !(b[j].is_ascii_alphanumeric() || b[j] == b'(' || b[j] == b'_');
        if prev_ok && ce > cs && re > rs && next_ok {
            let col = ooxml_xlsx::CellIndex::parse(&format!("{}1", &text[cs..ce])).expect("column").col;
            let row: i64 = text[rs..re].parse().expect("row");
            let col = if col_abs { i64::from(col) } else { i64::from(col) + dc };
            let row = if row_abs { row } else { row + dr };
            let letters = ooxml_xlsx::CellIndex { row: 0, col: col as u32 }.to_string();
            let letters = letters.trim_end_matches('1');
            out.push_str(&format!("{}{letters}{}{row}", if col_abs { "$" } else { "" }, if row_abs { "$" } else { "" }));
            i = re;
        } else {
            let ch = text[i..].chars().next().expect("char");
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

/// Each `<c>` of a worksheet text, by `r`, as bytes.
pub fn cell_bytes(text: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let Some(sd) = text.find("<sheetData") else { return out };
    let stop = text[sd..].find("</sheetData>").map_or(text.len(), |e| sd + e);
    let mut pos = sd;
    while let Some(i) = text[pos..stop].find("<c ") {
        let start = pos + i;
        let open_end = start + text[start..].find('>').expect("tag end") + 1;
        let end = if text.as_bytes()[open_end - 2] == b'/' { open_end } else { open_end + text[open_end..].find("</c>").expect("close") + 4 };
        let tag = &text[start..open_end];
        let r = tag.find(" r=\"").map(|a| { let a = a + 4; tag[a..a + tag[a..].find('"').expect("quote")].to_owned() }).unwrap_or_default();
        out.insert(r, text[start..end].to_owned());
        pos = end;
    }
    out
}

/// Oracle 4 (locality) for one epoch: every Base cell that is not in the
/// changed set and was not stripped keeps its bytes, and every cell that is
/// new or gone is in the changed set.
pub fn locality(base: &[u8], out: &[u8], changed: &[CellKey]) -> Result<(), String> {
    let (b, o) = (Raw::new(base), Raw::new(out));
    let stripped = strip_set(base, out);
    for (i, p) in b.parts.iter().enumerate() {
        let (bt, ot) = (part_str(&b.pkg, p), part_str(&o.pkg, p));
        let (bc, oc) = (cell_bytes(&bt), cell_bytes(&ot));
        let keys: BTreeSet<&String> = bc.keys().chain(oc.keys()).collect();
        for r in keys {
            let at = ooxml_xlsx::CellIndex::parse(r).expect("r");
            let key = CellKey::new(i, at.row, at.col);
            if changed.contains(&key) || stripped.contains(&(i, r.clone())) {
                continue;
            }
            if bc.get(r) != oc.get(r) {
                return Err(format!("locality: {r} on sheet {i} changed outside the changed set: {:?} -> {:?}", bc.get(r), oc.get(r)));
            }
        }
    }
    Ok(())
}
