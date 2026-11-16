//! Shared oracles for the Phase0 acceptance tests (docs/roadmap/xlsx.md §7):
//! L1 (zip entry equality), T2 (byte-level locality of a sheet edit) and CR
//! (independent read-back with calamine).
// `allow`, not `expect`: each test crate uses a different subset, so the lint
// fires in some crates and not in others.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::io::{Cursor, Read};
use std::path::PathBuf;

use calamine::{Data, Reader, Xlsx};
use ooxml_core::{ElementSpec, PartStateKind, RawNode, XmlDocument};
use ooxml_schema::sml;
use ooxml_xlsx::{CellIndex, CellValue, Workbook};
use zip::ZipArchive;

pub mod o2gen;
pub mod o3cases;
pub mod p1;
pub mod residual;

pub const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../reference");

/// Path of a fixture: `"poi/61605.xlsx"`, `"calamine/…"`, or `"minimal"`.
pub fn fixture(name: &str) -> PathBuf {
    if name == "minimal" {
        PathBuf::from(ROOT).join("round_trip_corpus/fixture_minimal.xlsx")
    } else {
        PathBuf::from(ROOT).join("fixtures/xlsx").join(name)
    }
}

pub fn read(name: &str) -> Vec<u8> {
    std::fs::read(fixture(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// Every zip fixture (all but the encrypted one) plus `fixture_minimal`.
pub fn all_zip_fixtures() -> Vec<String> {
    let mut v = vec!["minimal".to_owned()];
    for dir in ["poi", "calamine"] {
        let mut names: Vec<String> = std::fs::read_dir(PathBuf::from(ROOT).join("fixtures/xlsx").join(dir))
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .filter(|n| (n.ends_with(".xlsx") || n.ends_with(".xlsm")) && n != "protected_passtika.xlsx")
            .map(|n| format!("{dir}/{n}"))
            .collect();
        names.sort();
        v.extend(names);
    }
    v
}

pub type Entry = (String, zip::CompressionMethod, u32, Vec<u8>);

/// (name, compression method, CRC-32, compressed payload) per entry, in order.
pub fn entries(bytes: &[u8]) -> Vec<Entry> {
    let mut ar = ZipArchive::new(Cursor::new(bytes)).unwrap();
    (0..ar.len())
        .map(|i| {
            let mut f = ar.by_index_raw(i).unwrap();
            let mut raw = Vec::new();
            f.read_to_end(&mut raw).unwrap();
            (f.name().unwrap().into_owned(), f.compression(), f.crc32(), raw)
        })
        .collect()
}

pub fn part(bytes: &[u8], name: &str) -> Vec<u8> {
    let mut ar = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut f = ar.by_name(name).unwrap_or_else(|e| panic!("{name}: {e}"));
    let mut v = Vec::new();
    f.read_to_end(&mut v).unwrap();
    v
}

/// L1: same entry names in the same order; every entry not in `changed` is
/// equal in (name, method, CRC-32, compressed bytes).
pub fn assert_l1_except(input: &[u8], output: &[u8], changed: &[&str]) {
    let (a, b) = (entries(input), entries(output));
    let names = |v: &[Entry]| v.iter().map(|e| e.0.clone()).collect::<Vec<_>>();
    assert_eq!(names(&a), names(&b), "part list and order");
    for (x, y) in a.iter().zip(&b) {
        if !changed.contains(&x.0.as_str()) {
            assert!(x == y, "{} must pass through unchanged (L1)", x.0);
        }
    }
}

/// The workbook after a refused call must still save to the input entries.
pub fn assert_unchanged(wb: &Workbook, input: &[u8]) {
    assert_eq!(wb.package().state(wb.worksheet_part()), Some(PartStateKind::Parsed { dirty: false }));
    assert_l1_except(input, &wb.to_bytes().unwrap(), &[]);
}

fn sml_child(n: &RawNode, q: ooxml_core::QName) -> Option<&RawNode> {
    n.elements().find(|e| e.is(q))
}

fn row_r(n: &RawNode) -> u32 {
    n.attr(sml::Row::ATTR_R).unwrap().parse().unwrap()
}

fn cell_col(n: &RawNode) -> u32 {
    CellIndex::parse(n.attr(sml::Cell::ATTR_R).unwrap()).unwrap().col
}

/// (original byte range, output byte range) that an edit may change.
type Region = (std::ops::Range<usize>, std::ops::Range<usize>);

fn whole(n: &RawNode) -> std::ops::Range<usize> {
    let s = n.span().expect("parsed node");
    s.start..s.end()
}

/// T2 (ADR-0001): the byte diff between the original and the edited sheet is
/// confined to (a) the edited `c`'s original span, or — if it was created —
/// its insertion point among its original siblings (the next sibling's
/// start, or the parent's close tag), with the insertion point moving up to
/// the `row` (new row) or to a self-closing parent that had to be re-rendered;
/// plus (b) the `dimension` element. Every byte outside these regions must
/// be identical and in the same place relative to them.
pub fn assert_t2(orig: &[u8], out: &[u8], at: CellIndex) {
    let (o, n) = (XmlDocument::parse(orig).unwrap(), XmlDocument::parse(out).unwrap());
    let (os, ns) = (o.source(), n.source());
    let mut regions: Vec<Region> = Vec::new();
    if let Some(d) = sml_child(o.root(), sml::SheetDimension::QNAME) {
        let nd = sml_child(n.root(), sml::SheetDimension::QNAME).expect("dimension kept");
        regions.push((whole(d), whole(nd)));
    }
    let osd = sml_child(o.root(), sml::SheetData::QNAME).expect("sheetData");
    let nsd = sml_child(n.root(), sml::SheetData::QNAME).expect("sheetData");
    let want = at.row + 1;
    let nrow = nsd.elements().find(|r| row_r(r) == want).expect("row written");
    let ncell = nrow.elements().find(|c| cell_col(c) == at.col).expect("cell written");
    let region = if osd.span().unwrap().is_self_closing() {
        (whole(osd), whole(nsd))
    } else if let Some(orow) = osd.elements().find(|r| row_r(r) == want) {
        if orow.span().unwrap().is_self_closing() {
            (whole(orow), whole(nrow))
        } else if let Some(oc) = orow.elements().find(|c| cell_col(c) == at.col) {
            (whole(oc), whole(ncell))
        } else {
            let p = orow
                .elements()
                .find(|c| cell_col(c) > at.col)
                .map_or_else(|| orow.span().unwrap().close.unwrap().0, |c| c.span().unwrap().start);
            (p..p, whole(ncell))
        }
    } else {
        let p = osd
            .elements()
            .find(|r| row_r(r) > want)
            .map_or_else(|| osd.span().unwrap().close.unwrap().0, |r| r.span().unwrap().start);
        (p..p, whole(nrow))
    };
    regions.push(region);
    regions.sort_by_key(|r| r.0.start);
    let (mut po, mut pn) = (0, 0);
    for (ro, rn) in &regions {
        assert_eq!(&os[po..ro.start], &ns[pn..rn.start], "T2: bytes before an allowed region differ ({at})");
        (po, pn) = (ro.end, rn.end);
    }
    assert_eq!(&os[po..], &ns[pn..], "T2: bytes after the last allowed region differ ({at})");
    // rows and cells ascending in the output (insertion order)
    let rows: Vec<u32> = nsd.elements().map(row_r).collect();
    assert!(rows.is_sorted_by(|a, b| a < b), "rows ascending: {rows:?}");
    let cols: Vec<u32> = nrow.elements().filter(|c| c.is(sml::Cell::QNAME)).map(cell_col).collect();
    assert!(cols.is_sorted_by(|a, b| a < b), "cells ascending: {cols:?}");
}

/// Non-empty cells by absolute (row, col), per sheet name.
pub type Sheets = BTreeMap<String, BTreeMap<(u32, u32), Data>>;

/// calamine's view: sheet name → non-empty cells by absolute (row, col).
pub fn calamine_cells(bytes: &[u8]) -> Sheets {
    try_calamine_cells(bytes).expect("calamine opens it")
}

/// As [`calamine_cells`]; `None` if calamine cannot open the workbook.
pub fn try_calamine_cells(bytes: &[u8]) -> Option<Sheets> {
    let mut wb: Xlsx<_> = Xlsx::new(Cursor::new(bytes.to_vec())).ok()?;
    let names = wb.sheet_names().to_vec();
    let mut out = BTreeMap::new();
    for name in names {
        // chartsheets / macro sheets are not worksheets for calamine
        let Ok(range) = wb.worksheet_range(&name) else { continue };
        let mut cells = BTreeMap::new();
        for (r, c, d) in range.used_cells() {
            let (r0, c0) = range.start().unwrap();
            cells.insert((r0 + r as u32, c0 + c as u32), d.clone());
        }
        out.insert(name, cells);
    }
    Some(out)
}

/// What calamine should read for a value written by ooxml-xlsx.
pub fn calamine_matches(d: Option<&Data>, v: &CellValue) -> bool {
    match (d, v) {
        (None | Some(Data::Empty), CellValue::Empty) => true,
        (Some(Data::Float(f)), CellValue::Number(n)) => f == n,
        (Some(Data::Int(i)), CellValue::Number(n)) => *i as f64 == *n,
        (Some(Data::DateTime(dt)), CellValue::Number(n)) => dt.as_f64() == *n,
        (Some(Data::String(s)), CellValue::String(t)) => s == t,
        (None, CellValue::String(t)) => t.is_empty(),
        (Some(Data::Bool(a)), CellValue::Bool(b)) => a == b,
        _ => false,
    }
}

/// CR: calamine reads the output like the input, except at `at` on sheet
/// `sheet`, where it reads `expected`.
pub fn assert_cr(input: &[u8], output: &[u8], sheet: &str, at: CellIndex, expected: &CellValue) {
    let (mut a, mut b) = (calamine_cells(input), calamine_cells(output));
    assert_eq!(a.keys().collect::<Vec<_>>(), b.keys().collect::<Vec<_>>(), "CR: sheet list");
    let key = (at.row, at.col);
    let got = b.get_mut(sheet).expect("sheet").remove(&key);
    assert!(calamine_matches(got.as_ref(), expected), "CR: {at} reads {got:?}, expected {expected:?}");
    a.get_mut(sheet).unwrap().remove(&key);
    assert_eq!(a, b, "CR: other cells must read the same");
}

pub fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ooxml-xlsx-test-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A value write.
#[derive(Clone, Debug)]
pub enum W {
    Num(f64),
    Str(String),
}

impl W {
    pub fn expected(&self) -> CellValue {
        match self {
            W::Num(n) => CellValue::Number(*n),
            W::Str(s) => CellValue::String(s.clone()),
        }
    }
}

/// What a write to a cell is expected to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Expect {
    Ok,
    Merged,
    TableHeader,
    RichValue,
}

/// Applies one write to a fresh copy of `input`.
pub fn apply(input: &[u8], at: CellIndex, w: &W) -> (Workbook, ooxml_xlsx::Result<()>) {
    let mut wb = Workbook::from_bytes(input.to_vec()).unwrap();
    let r = {
        let mut ws = wb.worksheet().unwrap();
        let c = ws.cell_mut(at.row, at.col).unwrap();
        match w {
            W::Num(n) => c.set_number(*n),
            W::Str(s) => c.set_string(s),
        }
    };
    (wb, r)
}

/// One write, then every oracle: own read-back, `s` kept, L1 for every
/// other part, T2 on the sheet, CR with calamine, and re-open. Returns the
/// output bytes.
pub fn check_edit(name: &str, input: &[u8], at: CellIndex, w: &W) -> Vec<u8> {
    let style_before = {
        let mut wb = Workbook::from_bytes(input.to_vec()).unwrap();
        let ws = wb.worksheet().unwrap();
        ws.cell(at.row, at.col).unwrap().map(|c| c.raw().attr(sml::Cell::ATTR_S).map(str::to_owned))
    };
    let (mut wb, r) = apply(input, at, w);
    r.unwrap_or_else(|e| panic!("{name} {at} {w:?}: {e}"));
    let (sheet_part, sheet_name) = (wb.worksheet_part().to_owned(), wb.sheet_name().to_owned());
    {
        let ws = wb.worksheet().unwrap();
        let c = ws.cell(at.row, at.col).unwrap().expect("cell exists after write");
        assert_eq!(c.value().unwrap(), w.expected(), "{name} {at}: own read-back");
        if let Some(s) = style_before {
            assert_eq!(c.raw().attr(sml::Cell::ATTR_S).map(str::to_owned), s, "{name} {at}: s kept");
        }
    }
    let out = wb.to_bytes().unwrap();
    assert_l1_except(input, &out, &[&sheet_part]);
    assert_t2(&part(input, &sheet_part), &part(&out, &sheet_part), at);
    assert_cr(input, &out, &sheet_name, at, &w.expected());
    let mut again = Workbook::from_bytes(out.clone()).unwrap();
    let v = again.worksheet().unwrap().cell(at.row, at.col).unwrap().unwrap().value().unwrap();
    assert_eq!(v, w.expected(), "{name} {at}: re-opened read-back");
    out
}

/// Cells (with `r`) of the first worksheet, in document order.
pub fn existing_cells(input: &[u8]) -> Vec<CellIndex> {
    let mut wb = Workbook::from_bytes(input.to_vec()).unwrap();
    let doc = XmlDocument::parse(&part(input, wb.worksheet_part())).unwrap();
    let _ = wb.worksheet().unwrap();
    let sd = sml_child(doc.root(), sml::SheetData::QNAME).unwrap();
    sd.elements()
        .flat_map(|r| r.elements().filter(|c| c.is(sml::Cell::QNAME)))
        .map(|c| CellIndex::parse(c.attr(sml::Cell::ATTR_R).unwrap()).unwrap())
        .collect()
}

/// ADR-0001 T2 sweep: for each cell, write its own value back (a boolean as
/// a number), then a different number, then a padded string; or, where a
/// refusal is expected, check every write is refused and changes nothing.
/// Returns (successful writes, refused writes).
pub fn sweep(name: &str, cells: &[CellIndex], expect: impl Fn(CellIndex) -> Expect) -> (usize, usize) {
    let input = read(name);
    let (mut ok, mut refused) = (0, 0);
    for &at in cells {
        let current = {
            let mut wb = Workbook::from_bytes(input.clone()).unwrap();
            let ws = wb.worksheet().unwrap();
            ws.cell(at.row, at.col).unwrap().map_or(CellValue::Empty, |c| c.value().unwrap())
        };
        let mut writes = match current {
            CellValue::Number(n) => vec![W::Num(n)],
            CellValue::String(s) => vec![W::Str(s)],
            CellValue::Bool(b) => vec![W::Num(if b { 1.0 } else { 0.0 })],
            _ => vec![],
        };
        writes.push(W::Num(1234.5));
        writes.push(W::Str(format!("  {at} padded ")));
        for w in &writes {
            match expect(at) {
                Expect::Ok => {
                    check_edit(name, &input, at, w);
                    ok += 1;
                }
                e => {
                    let (wb, r) = apply(&input, at, w);
                    let err = r.expect_err(&format!("{name} {at}: expected {e:?}"));
                    let matched = match e {
                        Expect::Merged => matches!(err, ooxml_xlsx::Error::MergedCell { .. }),
                        Expect::TableHeader => matches!(err, ooxml_xlsx::Error::TableHeaderCell { .. }),
                        Expect::RichValue => matches!(err, ooxml_xlsx::Error::RichValueCell { .. }),
                        Expect::Ok => unreachable!(),
                    };
                    assert!(matched, "{name} {at}: expected {e:?}, got {err:?}");
                    assert_unchanged(&wb, &input);
                    refused += 1;
                }
            }
        }
    }
    (ok, refused)
}

/// Merged ranges of the first worksheet, as calamine reads them.
pub fn merged(input: &[u8]) -> Vec<ooxml_xlsx::CellRange> {
    let doc = {
        let wb = Workbook::from_bytes(input.to_vec()).unwrap();
        XmlDocument::parse(&part(input, wb.worksheet_part())).unwrap()
    };
    let Some(m) = sml_child(doc.root(), sml::MergeCells::QNAME) else { return vec![] };
    m.elements().map(|e| ooxml_xlsx::CellRange::parse(e.attr(sml::MergeCell::ATTR_REF).unwrap()).unwrap()).collect()
}

pub fn a1(s: &str) -> CellIndex {
    CellIndex::parse(s).unwrap()
}

pub fn sheet_xml(bytes: &[u8]) -> String {
    let wb = Workbook::from_bytes(bytes.to_vec()).unwrap();
    String::from_utf8(part(bytes, wb.worksheet_part())).unwrap()
}

/// A synthetic fixture: rebuilds the zip `input` with `edits` applied, each
/// replacing the entry of that name (or, if there is none, appended as a new
/// entry). Every other entry is copied as is. Used to derive minimal,
/// documented variants of real fixtures in memory.
pub fn rebuild(input: &[u8], edits: &[(&str, Vec<u8>)]) -> Vec<u8> {
    use std::io::Write;
    let mut ar = ZipArchive::new(Cursor::new(input)).unwrap();
    let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let opt = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut done = vec![false; edits.len()];
    for i in 0..ar.len() {
        let mut f = ar.by_index(i).unwrap();
        let name = f.name().unwrap().into_owned();
        let mut buf = Vec::new();
        f.read_to_end(&mut buf).unwrap();
        if let Some(k) = edits.iter().position(|(n, _)| *n == name) {
            done[k] = true;
            buf.clone_from(&edits[k].1);
        }
        zw.start_file(name.as_str(), opt).unwrap();
        zw.write_all(&buf).unwrap();
    }
    for (k, (n, b)) in edits.iter().enumerate() {
        if !done[k] {
            zw.start_file(*n, opt).unwrap();
            zw.write_all(b).unwrap();
        }
    }
    zw.finish().unwrap().into_inner()
}

/// A part of `bytes` as UTF-8 text.
pub fn text(bytes: &[u8], name: &str) -> String {
    String::from_utf8(part(bytes, name)).unwrap()
}

/// `text(input, name)` with `from` (which must occur exactly once) replaced
/// by `to`, ready for [`rebuild`].
pub fn patched(input: &[u8], name: &str, from: &str, to: &str) -> Vec<u8> {
    let t = text(input, name);
    assert_eq!(t.matches(from).count(), 1, "{name}: {from:?} must occur once");
    t.replacen(from, to, 1).into_bytes()
}
