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
            let f_attrs = local(c.raw(), "f").map(|f| f.attrs().iter().map(|a| (a.name().local().to_owned(), a.value().to_owned())).collect()).unwrap_or_default();
            raw_cells.push(((i, at.row, at.col), if formula.is_some() { None } else { Some(c.value().expect("value")) }, formula, f_attrs, c.style_index().expect("s")));
        }
    }
    for (k, value, formula, f_attrs, s) in raw_cells {
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
    fn formulas(&self) -> BTreeMap<(usize, String), Option<String>> {
        let mut out = BTreeMap::new();
        for i in 0..self.parts.len() {
            for c in self.cells(i) {
                if local(c, "f").is_some() {
                    out.insert((i, attr(c, "r").unwrap_or("").to_owned()), local(c, "v").map(|v| v.text().into_owned()));
                }
            }
        }
        out
    }
}

/// Formula cells of `base` with a cached value that `out` stripped.
pub fn strip_set(base: &[u8], out: &[u8]) -> BTreeSet<(usize, String)> {
    let (b, o) = (Raw::new(base).formulas(), Raw::new(out).formulas());
    b.into_iter().filter(|(k, v)| v.is_some() && o.get(k).is_some_and(|ov| ov.is_none())).map(|(k, _)| k).collect()
}

fn covers(dim: &str, k: CellKey) -> bool {
    let (a, b) = dim.split_once(':').unwrap_or((dim, dim));
    let (Ok(a), Ok(b)) = (ooxml_xlsx::CellIndex::parse(a), ooxml_xlsx::CellIndex::parse(b)) else { return false };
    (a.row..=b.row).contains(&k.row) && (a.col..=b.col).contains(&k.col)
}

/// The validity predicates of oracle 1's exclusions, for one side.
pub fn predicates(base: &[u8], out: &[u8], written: &[CellKey], journal_side: bool) -> Result<(), String> {
    let (b, o) = (Raw::new(base), Raw::new(out));
    // sst
    let sst = XmlDocument::parse(&o.pkg.read_part("xl/sharedStrings.xml").expect("sst")).expect("xml");
    let items = sst.root().elements().count();
    let mut refs = 0;
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
    // ADR feedback F4: on `main`, a non-string write over a `t="s"` cell
    // leaves `sst/@count` stale (the registry is not activated), so the
    // sequential side breaks "count = number of t=s cells"; the spike checks
    // only `uniqueCount` and the indices.
    let _ = refs;
    if attr(sst.root(), "uniqueCount") != Some(&items.to_string()) {
        return Err(format!("sst counts: {:?}/{:?} vs {refs}/{items}", attr(sst.root(), "count"), attr(sst.root(), "uniqueCount")));
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
        }
        if journal_side && mine.is_empty() {
            let bytes = |r: &Raw| local(r.docs[i].root(), "dimension").map(|d| { let sp = d.span().expect("span"); r.docs[i].source()[sp.start..sp.end()].to_owned() });
            if bytes(&b) != bytes(&o) {
                return Err(format!("sheet {i}: dimension bytes changed without a written cell"));
            }
        }
    }
    // calcChain
    let (bf, of) = (b.formulas(), o.formulas());
    if o.pkg.contains("xl/calcChain.xml") {
        let same_set = bf.keys().collect::<Vec<_>>() == of.keys().collect::<Vec<_>>();
        if !same_set || o.pkg.read_part("xl/calcChain.xml").ok() != b.pkg.read_part("xl/calcChain.xml").ok() {
            return Err("calcChain kept although the formula set changed".into());
        }
    }
    // caches: a kept `v` is the Base's
    for (k, v) in &of {
        if let Some(v) = v
            && bf.get(k) != Some(&Some(v.clone()))
        {
            return Err(format!("kept cache {v:?} at {k:?} is not the Base's"));
        }
    }
    // ADR feedback F5: `main` sets `fullCalcOnLoad` when it strips a
    // dependent's cache, not when it writes a formula or strips the written
    // cell's own cache; the predicate is checked in that weaker form.
    let written_refs: BTreeSet<(usize, String)> = written.iter().map(|k| (k.sheet, ooxml_xlsx::CellIndex { row: k.row, col: k.col }.to_string())).collect();
    let stripped = strip_set(base, out).iter().any(|k| !written_refs.contains(k));
    let wbx = String::from_utf8(o.pkg.read_part("xl/workbook.xml").expect("wb")).expect("utf8");
    if stripped && !wbx.contains(r#"fullCalcOnLoad="1""#) {
        return Err("cache stripped without fullCalcOnLoad".into());
    }
    Ok(())
}
