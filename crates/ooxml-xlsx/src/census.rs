//! The formula census at open (ADR-0003 §1): one quick-xml event scan per
//! sheet part (worksheet, macro sheet, dialog sheet), by element name
//! (SpreadsheetML main + local name), never by bytes. No Layer 2 tree is
//! built, so unedited parts stay raw-copied.

use std::collections::{HashMap, HashSet};

use quick_xml::events::{BytesStart, Event};
use quick_xml::name::ResolveResult;
use quick_xml::{NsReader, XmlVersion};

use crate::SML_NS;
use crate::error::Error;
use crate::reference::{CellIndex, CellRange};

/// What a `<sheet>` entry designates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SheetKind {
    Worksheet,
    Chartsheet,
    MacroSheet,
    DialogSheet,
    /// Missing part, external target or unknown type.
    Other,
}

/// One `<sheet>` in workbook order.
#[derive(Clone, Debug)]
pub(crate) struct SheetEntry {
    pub(crate) name: String,
    pub(crate) part: Option<String>,
    pub(crate) kind: SheetKind,
    pub(crate) visible: bool,
}

/// Kind of a formula cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FKind {
    Normal,
    Shared { si: u32, master: bool },
    Array,
    DataTable,
}

/// One cell with an `f` element.
#[derive(Clone, Debug)]
pub(crate) struct FCell {
    pub(crate) sheet: usize,
    pub(crate) at: CellIndex,
    pub(crate) kind: FKind,
    /// `f/@ref` (array/data-table result range, shared group range).
    pub(crate) range: Option<CellRange>,
    /// `f/@ca` true.
    pub(crate) volatile: bool,
    /// An XLDAPR dynamic-array anchor (`cm`).
    pub(crate) dynamic: bool,
    /// Cached value is `#SPILL!`.
    pub(crate) spill_error: bool,
    /// The cell still has a cached value (`v`, `t` or `is`).
    pub(crate) has_cache: bool,
    /// The non-anchor result cells already lost their cached values.
    pub(crate) results_stripped: bool,
    /// A shared group with two or more masters for its `si` on this sheet:
    /// readers disagree on which master a member follows (LibreOffice takes
    /// the first), so the whole group depends on everything and cannot be
    /// handed off (review R3-M1 b).
    pub(crate) ambiguous_group: bool,
    /// The formula text (empty for shared members).
    pub(crate) text: String,
}

impl FCell {
    /// Cells other than the anchor that hold this formula's results.
    pub(crate) fn result_range(&self) -> Option<CellRange> {
        match self.kind {
            FKind::Array | FKind::DataTable => self.range,
            _ => None,
        }
    }
}

/// Why every edit of the workbook is refused (ADR-0003 §1, §5).
#[derive(Clone, Debug)]
pub(crate) enum EditBlock {
    ScanFailed { part: String, reason: String },
    MissingReference { part: String },
    UnknownCellMetadata { part: String, cell: String },
    /// A cell whose `r` names another row than its `row/@r` (fuzz finding
    /// F-8): the census (keyed by `c/@r`) and the row index (keyed by row)
    /// would disagree about where the cell is, and a write could add a
    /// second cell with the same reference.
    RowMismatch { part: String, cell: String, row: u32 },
}

impl EditBlock {
    pub(crate) fn to_error(&self) -> Error {
        match self {
            EditBlock::ScanFailed { part, reason } => {
                Error::FormulaScanFailed { part: part.clone(), reason: reason.clone() }
            }
            EditBlock::MissingReference { part } => Error::MissingReference {
                part: part.clone(),
                what: "row or cell (in a sheet that contains formulas)".into(),
            },
            EditBlock::UnknownCellMetadata { part, cell } => {
                Error::UnknownCellMetadata { part: part.clone(), cell: cell.clone() }
            }
            EditBlock::RowMismatch { part, cell, row } => Error::InvalidReference(format!("{part}: {cell} in row {row}")),
        }
    }
}

/// The census: every formula cell of the workbook, indexed by position.
#[derive(Debug, Default)]
pub(crate) struct Census {
    /// By formula id; `None` once removed.
    pub(crate) cells: Vec<Option<FCell>>,
    pub(crate) by_pos: HashMap<(usize, CellIndex), usize>,
    /// Every cell with `vm` (formula or not).
    pub(crate) vm_cells: HashSet<(usize, CellIndex)>,
    pub(crate) block: Option<EditBlock>,
    /// Some formula has `bx` true.
    pub(crate) bx_any: bool,
}

impl Census {
    pub(crate) fn get(&self, fid: usize) -> Option<&FCell> {
        self.cells.get(fid).and_then(Option::as_ref)
    }

    pub(crate) fn at(&self, sheet: usize, at: CellIndex) -> Option<usize> {
        self.by_pos.get(&(sheet, at)).copied()
    }

    pub(crate) fn push(&mut self, c: FCell) -> usize {
        let fid = self.cells.len();
        self.by_pos.insert((c.sheet, c.at), fid);
        self.cells.push(Some(c));
        fid
    }

    pub(crate) fn remove(&mut self, fid: usize) {
        if let Some(c) = self.cells.get_mut(fid).and_then(Option::take) {
            self.by_pos.remove(&(c.sheet, c.at));
        }
    }

    pub(crate) fn alive(&self) -> impl Iterator<Item = (usize, &FCell)> {
        self.cells.iter().enumerate().filter_map(|(i, c)| c.as_ref().map(|c| (i, c)))
    }

    /// The master of shared group `si` on `sheet`.
    pub(crate) fn master_of(&self, sheet: usize, si: u32) -> Option<usize> {
        self.alive()
            .find(|(_, c)| c.sheet == sheet && c.kind == FKind::Shared { si, master: true })
            .map(|(i, _)| i)
    }

    /// The anchor whose result range contains `at` (not the anchor itself).
    pub(crate) fn result_anchor(&self, sheet: usize, at: CellIndex) -> Option<usize> {
        self.alive()
            .find(|(_, c)| {
                c.sheet == sheet && c.at != at && (c.result_range().is_some_and(|r| r.contains(at)) || (c.dynamic && c.range.is_some_and(|r| r.contains(at))))
            })
            .map(|(i, _)| i)
    }

    fn block(&mut self, b: EditBlock) {
        if self.block.is_none() {
            self.block = Some(b);
        }
    }
}

fn attr(e: &BytesStart<'_>, local: &str) -> Result<Option<String>, String> {
    for a in e.attributes().with_checks(true) {
        let a = a.map_err(|e| e.to_string())?;
        if a.key.prefix().is_none() && a.key.local_name().into_inner() == local {
            return Ok(Some(a.normalized_value(XmlVersion::Implicit1_0).map_err(|e| e.to_string())?.into_owned()));
        }
    }
    Ok(None)
}

fn truthy(v: Option<&str>) -> bool {
    matches!(v, Some("1" | "true"))
}

/// Per-cell state while scanning.
#[derive(Default)]
struct CellScan {
    r: Option<String>,
    t: Option<String>,
    cm: Option<String>,
    vm: bool,
    f: Option<(Vec<(String, String)>, String)>,
    v: Option<String>,
    is: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TextSink {
    None,
    F,
    V,
}

/// Scans one sheet part. `dynamic_cm(cm)` says whether a `cm` value is an
/// XLDAPR dynamic-array anchor. Fails (as a reason string) if the part
/// cannot be scanned or has an `f` outside a cell.
pub(crate) fn scan_sheet(
    xml: &[u8],
    part: &str,
    sheet: usize,
    dynamic_cm: &HashSet<u32>,
    census: &mut Census,
) -> Result<(), String> {
    if xml.starts_with(&[0xFF, 0xFE]) || xml.starts_with(&[0xFE, 0xFF]) {
        return Err("UTF-16 parts are not supported".into());
    }
    let mut reader = NsReader::from_reader(xml);
    let mut stack: Vec<String> = Vec::new(); // SML local names (others as "")
    let mut cell: Option<CellScan> = None;
    let mut sink = TextSink::None;
    let mut missing_r = false;
    let mut formula_without_r = false;
    let mut found: Vec<FCell> = Vec::new();
    let mut vm_cells = Vec::new();
    let mut unknown_cm: Option<String> = None;
    let mut bx = false;
    // The number of the row being read (`row/@r`), and the first cell
    // naming another row (F-8).
    let mut row_number: Option<u32> = None;
    let mut row_mismatch: Option<(String, u32)> = None;
    loop {
        let pos = reader.buffer_position();
        let (ns, ev) = reader.read_resolved_event().map_err(|e| format!("XML error after byte {pos}: {e}"))?;
        let sml = matches!(ns, ResolveResult::Bound(n) if n.into_inner() == SML_NS);
        match ev {
            Event::Start(ref e) | Event::Empty(ref e) => {
                let empty = matches!(ev, Event::Empty(_));
                let local = if sml { e.local_name().into_inner().to_owned() } else { String::new() };
                let parent = stack.last().map(String::as_str);
                match local.as_str() {
                    "row" if parent == Some("sheetData") => {
                        let r = attr(e, "r")?;
                        if r.is_none() {
                            missing_r = true;
                        }
                        // Read exactly as the row index reads it (XML whitespace
                        // trimmed, row 0 invalid), or the two disagree again
                        // (review of PR #67, M1/m1). A row the index cannot read
                        // is its own refusal (`BadRowR`), not a mismatch.
                        row_number = r.as_deref().and_then(ooxml_core::value::parse_u32).filter(|r| *r >= 1);
                    }
                    "c" if parent == Some("row") => {
                        let mut c = CellScan { r: attr(e, "r")?, t: attr(e, "t")?, cm: attr(e, "cm")?, ..Default::default() };
                        c.vm = attr(e, "vm")?.is_some();
                        if c.r.is_none() {
                            missing_r = true;
                        }
                        if row_mismatch.is_none()
                            && let (Some(row), Some(at)) = (row_number, c.r.as_deref().and_then(|r| CellIndex::parse(r).ok()))
                            && at.row + 1 != row
                        {
                            row_mismatch = Some((at.to_string(), row));
                        }
                        cell = Some(c);
                    }
                    "f" => {
                        let Some(c) = cell.as_mut().filter(|_| parent == Some("c")) else {
                            return Err("an f element outside worksheet/sheetData/row/c".into());
                        };
                        let mut attrs = Vec::new();
                        for a in e.attributes().with_checks(true) {
                            let a = a.map_err(|e| e.to_string())?;
                            if a.key.prefix().is_none() {
                                let v = a.normalized_value(XmlVersion::Implicit1_0).map_err(|e| e.to_string())?;
                                attrs.push((String::from_utf8_lossy(a.key.local_name().into_inner().as_bytes()).into_owned(), v.into_owned()));
                            }
                        }
                        c.f = Some((attrs, String::new()));
                        if !empty {
                            sink = TextSink::F;
                        }
                    }
                    "v" if parent == Some("c") => {
                        if let Some(c) = cell.as_mut() {
                            c.v = Some(String::new());
                            if !empty {
                                sink = TextSink::V;
                            }
                        }
                    }
                    "is" if parent == Some("c") => {
                        if let Some(c) = cell.as_mut() {
                            c.is = true;
                        }
                    }
                    _ => {}
                }
                if empty {
                    if local == "c" && parent == Some("row") {
                        formula_without_r |= finish_cell(cell.take(), sheet, dynamic_cm, &mut found, &mut vm_cells, &mut unknown_cm, &mut bx)?;
                    }
                } else {
                    stack.push(local);
                }
            }
            Event::End(_) => {
                let local = stack.pop().unwrap_or_default();
                if local == "f" || local == "v" {
                    sink = TextSink::None;
                }
                if local == "c" && stack.last().map(String::as_str) == Some("row") {
                    formula_without_r |= finish_cell(cell.take(), sheet, dynamic_cm, &mut found, &mut vm_cells, &mut unknown_cm, &mut bx)?;
                }
            }
            Event::Text(t) => {
                let s = t.xml_content(XmlVersion::Implicit1_0);
                push_text(&mut cell, sink, &s);
            }
            Event::CData(t) => {
                let s = t.xml_content(XmlVersion::Implicit1_0);
                push_text(&mut cell, sink, &s);
            }
            Event::GeneralRef(r) => {
                let ch = match r.resolve_char_ref().map_err(|e| e.to_string())? {
                    Some(c) => c,
                    None => match &*r {
                        "lt" => '<',
                        "gt" => '>',
                        "amp" => '&',
                        "apos" => '\'',
                        "quot" => '"',
                        other => return Err(format!("undeclared entity &{other};")),
                    },
                };
                push_text(&mut cell, sink, ch.encode_utf8(&mut [0; 4]));
            }
            Event::DocType(_) => return Err("DTD (<!DOCTYPE>) is not allowed".into()),
            Event::Eof => break,
            _ => {}
        }
    }
    if formula_without_r || (missing_r && !found.is_empty()) {
        census.block(EditBlock::MissingReference { part: part.to_owned() });
    }
    if let Some(cell) = unknown_cm {
        census.block(EditBlock::UnknownCellMetadata { part: part.to_owned(), cell });
    }
    // Any sheet, with or without formulas: a write to the named cell would
    // not find it through the row index and add a duplicate, and a formula
    // anywhere may read it. Fail closed for every edit (reads still work).
    if let Some((cell, row)) = row_mismatch {
        census.block(EditBlock::RowMismatch { part: part.to_owned(), cell, row });
    }
    census.bx_any |= bx;
    let mut masters: HashMap<u32, usize> = HashMap::new();
    for f in &found {
        if let FKind::Shared { si, master: true } = f.kind {
            *masters.entry(si).or_default() += 1;
        }
    }
    for f in &mut found {
        if let FKind::Shared { si, .. } = f.kind
            && masters.get(&si).is_some_and(|n| *n > 1)
        {
            f.ambiguous_group = true;
        }
    }
    census.vm_cells.extend(vm_cells.into_iter().map(|c| (sheet, c)));
    for f in found {
        census.push(f);
    }
    Ok(())
}

fn push_text(cell: &mut Option<CellScan>, sink: TextSink, s: &str) {
    let Some(c) = cell.as_mut() else { return };
    match sink {
        TextSink::F => {
            if let Some((_, t)) = c.f.as_mut() {
                t.push_str(s);
            }
        }
        TextSink::V => {
            if let Some(v) = c.v.as_mut() {
                v.push_str(s);
            }
        }
        TextSink::None => {}
    }
}

fn finish_cell(
    c: Option<CellScan>,
    sheet: usize,
    dynamic_cm: &HashSet<u32>,
    found: &mut Vec<FCell>,
    vm_cells: &mut Vec<CellIndex>,
    unknown_cm: &mut Option<String>,
    bx_any: &mut bool,
) -> Result<bool, String> {
    let Some(c) = c else { return Ok(false) };
    let at = match &c.r {
        Some(r) => Some(CellIndex::parse(r).map_err(|e| e.to_string())?),
        None => None,
    };
    if c.vm
        && let Some(at) = at
    {
        vm_cells.push(at);
    }
    let Some((attrs, text)) = c.f else {
        if c.cm.is_some() && unknown_cm.is_none() {
            *unknown_cm = Some(c.r.unwrap_or_default());
        }
        return Ok(false);
    };
    let Some(at) = at else { return Ok(true) }; // a formula cell without r
    let get = |k: &str| attrs.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
    let range = match get("ref") {
        Some(r) => Some(CellRange::parse(r).map_err(|e| e.to_string())?),
        None => None,
    };
    let kind = match get("t") {
        None | Some("normal") => FKind::Normal,
        Some("array") => FKind::Array,
        Some("dataTable") => FKind::DataTable,
        Some("shared") => {
            let si = get("si").and_then(|s| s.parse().ok()).ok_or("shared formula without si")?;
            FKind::Shared { si, master: range.is_some() && !text.is_empty() }
        }
        Some(other) => return Err(format!("unknown formula type {other:?}")),
    };
    let dynamic = match &c.cm {
        None => false,
        Some(cm) => {
            let ok = kind == FKind::Array && cm.parse::<u32>().is_ok_and(|n| dynamic_cm.contains(&n));
            if !ok && unknown_cm.is_none() {
                *unknown_cm = Some(at.to_string());
            }
            ok
        }
    };
    if truthy(get("bx")) {
        *bx_any = true;
    }
    let spill_error = c.t.as_deref() == Some("e") && c.v.as_deref().map(str::trim) == Some("#SPILL!");
    found.push(FCell {
        sheet,
        at,
        kind,
        range,
        volatile: truthy(get("ca")),
        dynamic,
        spill_error,
        has_cache: c.v.is_some() || c.t.is_some() || c.is,
        results_stripped: false,
        ambiguous_group: false,
        text,
    });
    Ok(false)
}

/// `cm` values (1-based `cellMetadata` blocks) that denote an XLDAPR
/// dynamic array (`xda:dynamicArrayProperties fDynamic="1"`).
pub(crate) fn dynamic_array_cms(xml: &[u8]) -> Result<HashSet<u32>, String> {
    let mut reader = NsReader::from_reader(xml);
    let mut types: Vec<String> = Vec::new();
    // futureMetadata name -> per-block "fDynamic"
    let mut future: HashMap<String, Vec<bool>> = HashMap::new();
    let mut cur_future: Option<String> = None;
    // cellMetadata blocks: list of (t, v)
    let mut cell_blocks: Vec<Vec<(u32, u32)>> = Vec::new();
    let mut stack: Vec<String> = Vec::new();
    loop {
        let (ns, ev) = reader.read_resolved_event().map_err(|e| format!("XML error: {e}"))?;
        let sml = matches!(ns, ResolveResult::Bound(n) if n.into_inner() == SML_NS);
        match ev {
            Event::Start(ref e) | Event::Empty(ref e) => {
                let local = String::from_utf8_lossy(e.local_name().into_inner().as_bytes()).into_owned();
                let parent = stack.last().map(String::as_str).unwrap_or_default();
                match (sml, local.as_str()) {
                    (true, "metadataType") => types.push(attr(e, "name")?.unwrap_or_default()),
                    (true, "futureMetadata") => {
                        let n = attr(e, "name")?.unwrap_or_default();
                        future.entry(n.clone()).or_default();
                        cur_future = Some(n);
                    }
                    (true, "bk") if parent == "futureMetadata" => {
                        if let Some(n) = &cur_future {
                            future.get_mut(n).expect("entry").push(false);
                        }
                    }
                    (true, "bk") if parent == "cellMetadata" => cell_blocks.push(Vec::new()),
                    (true, "rc") if parent == "bk" && stack.iter().any(|s| s == "cellMetadata") => {
                        let t = attr(e, "t")?.and_then(|s| s.parse().ok()).unwrap_or(0);
                        let v = attr(e, "v")?.and_then(|s| s.parse().ok()).unwrap_or(u32::MAX);
                        if let Some(b) = cell_blocks.last_mut() {
                            b.push((t, v));
                        }
                    }
                    (false, "dynamicArrayProperties") if stack.iter().any(|s| s == "futureMetadata") => {
                        if truthy(attr(e, "fDynamic")?.as_deref())
                            && let Some(n) = &cur_future
                            && let Some(last) = future.get_mut(n).and_then(|v| v.last_mut())
                        {
                            *last = true;
                        }
                    }
                    _ => {}
                }
                if matches!(ev, Event::Start(_)) {
                    stack.push(if sml { local } else { String::new() });
                }
            }
            Event::End(_) => {
                if stack.pop().as_deref() == Some("futureMetadata") {
                    cur_future = None;
                }
            }
            Event::DocType(_) => return Err("DTD (<!DOCTYPE>) is not allowed".into()),
            Event::Eof => break,
            _ => {}
        }
    }
    let mut out = HashSet::new();
    for (i, block) in cell_blocks.iter().enumerate() {
        if let [(t, v)] = block.as_slice()
            && types.get((*t as usize).wrapping_sub(1)).is_some_and(|n| n == "XLDAPR")
            && future.get("XLDAPR").and_then(|b| b.get(*v as usize)).copied() == Some(true)
        {
            out.insert(i as u32 + 1);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NS: &str = r#"xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main""#;

    /// Corpus check for F-8 (review of PR #67, n2): no sheet part of any
    /// fixture has a cell naming another row, checked on each part on its
    /// own (so an earlier block of another kind cannot hide it).
    #[test]
    fn no_fixture_sheet_has_a_row_mismatch() {
        fn collect(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            for e in std::fs::read_dir(dir).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    collect(&p, out);
                } else if p.extension().is_some_and(|x| x == "xlsx" || x == "xlsm") {
                    out.push(p);
                }
            }
        }
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reference");
        let mut files = Vec::new();
        collect(&root.join("fixtures/xlsx"), &mut files);
        collect(&root.join("round_trip_corpus"), &mut files);
        let mut sheets = 0;
        for f in &files {
            let Ok(pkg) = ooxml_core::Package::from_bytes(std::fs::read(f).unwrap()) else { continue };
            for part in pkg.part_names().filter(|n| (n.contains("worksheets/") || n.contains("macrosheets/")) && n.ends_with(".xml")) {
                let mut c = Census::default();
                let _ = scan_sheet(&pkg.read_part(part).unwrap(), part, 0, &HashSet::new(), &mut c);
                assert!(!matches!(c.block, Some(EditBlock::RowMismatch { .. })), "{} {part}: {:?}", f.display(), c.block);
                sheets += 1;
            }
        }
        assert!(sheets >= 60, "{sheets} sheet parts checked");
    }

    fn scan(body: &str, dynamic: &[u32]) -> (Census, Result<(), String>) {
        let mut c = Census::default();
        let xml = format!("<worksheet {NS}><sheetData>{body}</sheetData></worksheet>");
        let r = scan_sheet(xml.as_bytes(), "s.xml", 0, &dynamic.iter().copied().collect(), &mut c);
        (c, r)
    }

    #[test]
    fn formula_cells_and_kinds() {
        let (c, r) = scan(
            r#"<row r="1"><c r="A1"><f>B1&amp;"x"</f><v>1</v></c><c r="B1" t="str"><f t="shared" ref="B1:B3" si="0">A1+1</f><v>x</v></c><c r="C1" cm="1"><f t="array" ref="C1:C4">SEQUENCE(4)</f><v>1</v></c><c r="D1" t="e"><f>SEQUENCE(9)</f><v>#SPILL!</v></c></row><row r="2"><c r="B2"><f t="shared" si="0"/></c><c r="E2" vm="1"><v>0</v></c></row>"#,
            &[1],
        );
        r.unwrap();
        assert!(c.block.is_none());
        let a1 = c.get(c.at(0, CellIndex::parse("A1").unwrap()).unwrap()).unwrap();
        assert_eq!(a1.text, "B1&\"x\"");
        assert!(a1.has_cache);
        let b1 = c.get(c.at(0, CellIndex::parse("B1").unwrap()).unwrap()).unwrap();
        assert_eq!(b1.kind, FKind::Shared { si: 0, master: true });
        let b2 = c.get(c.at(0, CellIndex::parse("B2").unwrap()).unwrap()).unwrap();
        assert_eq!(b2.kind, FKind::Shared { si: 0, master: false });
        assert!(!b2.has_cache);
        assert!(c.get(c.at(0, CellIndex::parse("C1").unwrap()).unwrap()).unwrap().dynamic);
        assert!(c.get(c.at(0, CellIndex::parse("D1").unwrap()).unwrap()).unwrap().spill_error);
        assert!(c.vm_cells.contains(&(0, CellIndex::parse("E2").unwrap())));
        assert_eq!(c.result_anchor(0, CellIndex::parse("C3").unwrap()), c.at(0, CellIndex::parse("C1").unwrap()));
    }

    #[test]
    fn blocks() {
        let (c, _) = scan(r#"<row r="1"><c r="A1" cm="2"><f t="array" ref="A1">1</f></c></row>"#, &[1]);
        assert!(matches!(c.block, Some(EditBlock::UnknownCellMetadata { .. })));
        let (c, _) = scan(r#"<row r="1"><c r="A1" cm="1"><v>1</v></c></row>"#, &[1]);
        assert!(matches!(c.block, Some(EditBlock::UnknownCellMetadata { .. })), "cm without f");
        let (c, _) = scan(r#"<row><c><f>1</f></c></row>"#, &[]);
        assert!(matches!(c.block, Some(EditBlock::MissingReference { .. })));
        // F-8: a cell naming another row, with or without a formula
        let (c, _) = scan(r#"<row r="1"><c r="A1"><v>1</v></c><c r="B2"><f>A1*2</f></c></row>"#, &[]);
        assert!(matches!(&c.block, Some(EditBlock::RowMismatch { cell, row: 1, .. }) if cell == "B2"));
        let (c, _) = scan(r#"<row r="3"><c r="A2"><v>1</v></c></row>"#, &[]);
        assert!(matches!(c.block, Some(EditBlock::RowMismatch { row: 3, .. })), "values only");
        let (c, _) = scan(r#"<row r="1"><c r="A1"><v>1</v></c></row><row r="2"><c r="B2"><f>A1*2</f></c></row>"#, &[]);
        assert!(c.block.is_none(), "cells in their own rows");
        let (c, _) = scan(r#"<row><c r="B2"><v>1</v></c></row>"#, &[]);
        assert!(c.block.is_none(), "a row without r cannot disagree (Phase0's MissingReference covers it)");
        // row/@r read like the row index reads it (review of PR #67, M1, m1)
        for r in [" 1", "\t1", "1 ", "\n1\r"] {
            let (c, _) = scan(&format!(r#"<row r="{r}"><c r="A1"><v>1</v></c><c r="B2"><f>A1*2</f></c></row>"#), &[]);
            assert!(matches!(&c.block, Some(EditBlock::RowMismatch { cell, row: 1, .. }) if cell == "B2"), "row r={r:?}: {:?}", c.block);
        }
        let (c, _) = scan(r#"<row r=" 2 "><c r="B2"><f>1</f></c></row>"#, &[]);
        assert!(c.block.is_none(), "padded row number, cell in its own row");
        let (c, _) = scan(r#"<row r="0"><c r="A1"><v>1</v></c></row>"#, &[]);
        assert!(c.block.is_none(), "row 0 is the row index's BadRowR for that sheet, not a mismatch");
        let (c, _) = scan(r#"<row><c><v>1</v></c></row>"#, &[]);
        assert!(c.block.is_none(), "no formulas: only that sheet's own edits are refused (Phase0)");
        let mut c = Census::default();
        let xml = format!("<worksheet {NS}><f>1</f></worksheet>");
        assert!(scan_sheet(xml.as_bytes(), "s", 0, &HashSet::new(), &mut c).is_err());
    }

    #[test]
    fn metadata() {
        let xml = r#"<metadata xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:xda="http://schemas.microsoft.com/office/spreadsheetml/2017/dynamicarray"><metadataTypes count="2"><metadataType name="XLRICHVALUE"/><metadataType name="XLDAPR"/></metadataTypes><futureMetadata name="XLDAPR" count="2"><bk><extLst><ext uri="{bdbb8cdc-fa1e-496e-a857-3c3f30c029c3}"><xda:dynamicArrayProperties fDynamic="1" fCollapsed="0"/></ext></extLst></bk><bk><extLst><ext uri="x"><xda:dynamicArrayProperties fDynamic="0"/></ext></extLst></bk></futureMetadata><cellMetadata count="3"><bk><rc t="2" v="0"/></bk><bk><rc t="2" v="1"/></bk><bk><rc t="1" v="0"/></bk></cellMetadata></metadata>"#;
        assert_eq!(dynamic_array_cms(xml.as_bytes()).unwrap(), HashSet::from([1]));
    }
}
