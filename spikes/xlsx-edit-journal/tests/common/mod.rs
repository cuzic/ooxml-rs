#![allow(dead_code)]

use std::collections::BTreeSet;

use ooxml_xlsx::{Border, CalcPolicy, Color, ExcelDateTime, Fill, Font, Format, Side, Workbook};
use xlsx_edit_journal_spike::op::{apply_seq, start_of};
use xlsx_edit_journal_spike::project::{Aligned, cached_in, model, predicates_aligned, strip_set};
use xlsx_edit_journal_spike::shadow::cached_cells;
use xlsx_edit_journal_spike::{Api, CellKey, CommitReport, Content, JError, Journal, Op, Patch, Start};

pub fn k(a1: &str) -> CellKey {
    let i = ooxml_xlsx::CellIndex::parse(a1).unwrap();
    CellKey::new(0, i.row, i.col)
}
pub fn k2(a1: &str) -> CellKey {
    CellKey { sheet: 1, ..k(a1) }
}
pub fn bold() -> Format {
    Format::new().set_bold()
}
pub fn date() -> ExcelDateTime {
    ExcelDateTime::from_ymd(2024, 1, 2).unwrap()
}
pub fn red() -> Fill {
    Fill::Pattern { pattern: "solid".into(), fg: Some(Color::rgb(0xFF0000)), bg: None }
}
pub fn italic() -> Font {
    Font { italic: true, ..Font::default() }
}
pub fn thin() -> Border {
    let s = Some(Side { style: "thin".into(), color: None });
    Border { left: s.clone(), right: s.clone(), top: s.clone(), bottom: s, ..Border::default() }
}

/// The sequential reference: each op by its API call, refusals dropped.
pub struct SeqOut {
    pub bytes: Vec<u8>,
    pub refused: Vec<Option<std::mem::Discriminant<ooxml_xlsx::Error>>>,
    /// Cells written (final positions; a dead id dropped).
    pub written: Vec<CellKey>,
    /// The cells the sequential side's structural ops stripped (white-box,
    /// final positions): oracle 1's structural direction.
    pub records: BTreeSet<CellKey>,
}

/// Runs `ops` on `wb` by their sequential calls, recording each structural
/// op's strips (cells with a cache before whose cache is gone after, at
/// their new position).
fn run(wb: &mut Workbook, ops: &[&Op], refused: &mut Vec<Option<std::mem::Discriminant<ooxml_xlsx::Error>>>, written: &mut Vec<CellKey>, records: &mut BTreeSet<CellKey>) {
    for op in ops {
        let start = if op.is_structural() || matches!(op, Op::WriteCells(_)) { Start::Other } else { start_of(wb, op.key()).unwrap() };
        let before = if let Op::Structural(_) = op { Some(cached_cells(wb).unwrap()) } else { None };
        match apply_seq(wb, op) {
            Ok(()) => {
                refused.push(None);
                if let Op::Structural(st) = op {
                    let after = cached_cells(wb).unwrap();
                    *written = written.iter().filter_map(|k| st.map(*k)).collect();
                    *records = records.iter().filter_map(|k| st.map(*k)).collect();
                    records.extend(before.unwrap().iter().filter_map(|k| st.map(*k)).filter(|k| !after.contains(k)));
                    continue;
                }
                let skipped = matches!(op, Op::Content(_, Content::RemoveFormula)) && start != Start::Formula;
                if !skipped {
                    written.extend(op.keys());
                }
            }
            Err(e) => refused.push(Some(std::mem::discriminant(&e))),
        }
    }
}

pub fn sequential(base: &[u8], ops: &[Op]) -> SeqOut {
    let mut wb = Workbook::from_bytes(base.to_vec()).unwrap();
    let (mut refused, mut written, mut records) = (Vec::new(), Vec::new(), BTreeSet::new());
    run(&mut wb, &ops.iter().collect::<Vec<_>>(), &mut refused, &mut written, &mut records);
    SeqOut { bytes: wb.to_bytes().unwrap(), refused, written, records }
}

/// The probe of oracle 1 (review round 9, M1): the Base with only the log's
/// structural ops, in order. `None` if it refuses one.
pub struct Probe {
    /// Under `KeepStaleCaches`: the aligned Base.
    pub keep: Vec<u8>,
    /// Under `StripDependentCaches`: the cells its ops strip (final
    /// positions) and whether calcChain survived.
    pub records: BTreeSet<CellKey>,
    pub chain_kept: bool,
}

pub fn probe(base: &[u8], structural: &[&Op]) -> Option<Probe> {
    let mut keep = Workbook::from_bytes(base.to_vec()).unwrap();
    keep.set_calc_policy(CalcPolicy::KeepStaleCaches);
    let mut strip = Workbook::from_bytes(base.to_vec()).unwrap();
    let (mut r1, mut w, mut records) = (Vec::new(), Vec::new(), BTreeSet::new());
    run(&mut keep, structural, &mut r1, &mut w, &mut BTreeSet::new());
    let mut r2 = Vec::new();
    run(&mut strip, structural, &mut r2, &mut w, &mut records);
    if r1.iter().chain(&r2).any(Option::is_some) {
        return None;
    }
    let keep = keep.to_bytes().unwrap();
    let chain_kept = ooxml_core::Package::from_bytes(strip.to_bytes().unwrap()).unwrap().contains("xl/calcChain.xml");
    Some(Probe { keep, records, chain_kept })
}

pub struct JOut {
    pub bytes: Vec<u8>,
    pub refused: Vec<Option<std::mem::Discriminant<ooxml_xlsx::Error>>>,
    pub report: CommitReport,
    pub journal: Journal,
}

pub fn journaled(base: &[u8], ops: &[Op]) -> JOut {
    let mut j = Journal::open(base.to_vec()).unwrap();
    let mut refused = Vec::new();
    for op in ops {
        match j.append(Api::Native, op.clone(), None) {
            Ok(_) => refused.push(None),
            Err(JError::Xlsx(e)) => refused.push(Some(std::mem::discriminant(&e))),
            Err(e) => panic!("unexpected journal error {e:?} for {op:?}"),
        }
    }
    let (bytes, report) = j.commit().unwrap_or_else(|e| panic!("accepted ⇒ compiles: {e:?}"));
    JOut { bytes, refused, report, journal: j }
}

/// The cells accepted keyed ops targeted, at final positions.
pub fn targets(ops: &[Op], refused: &[Option<std::mem::Discriminant<ooxml_xlsx::Error>>]) -> Vec<CellKey> {
    let mut t: Vec<CellKey> = Vec::new();
    for (op, r) in ops.iter().zip(refused) {
        if r.is_some() {
            continue;
        }
        match op {
            Op::Structural(s) => t = t.iter().filter_map(|k| s.map(*k)).collect(),
            _ => t.extend(op.keys()),
        }
    }
    t.sort();
    t.dedup();
    t
}

/// Counters over a proptest run (cases whose probe refused).
pub static PROBE_SKIPPED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
pub static PROBE_RUN: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Oracle 1 for one case. Returns the two outputs for extra assertions.
pub fn oracle1(base: &[u8], ops: &[Op]) -> (SeqOut, JOut) {
    let s = sequential(base, ops);
    let j = journaled(base, ops);
    assert_eq!(s.refused, j.refused, "(i) refusal equivalence for {ops:?}");
    let (ms, mj) = (model(&s.bytes), model(&j.bytes));
    if ms != mj {
        for (key, c) in &ms.cells {
            if mj.cells.get(key) != Some(c) {
                eprintln!("cell {key:?}: sequential {c:?}\n            journal {:?}", mj.cells.get(key));
            }
        }
        for key in mj.cells.keys().filter(|k| !ms.cells.contains_key(*k)) {
            eprintln!("cell {key:?} only on the journal side: {:?}", mj.cells[key]);
        }
        assert!(ms.rows == mj.rows, "rows differ: {:?} vs {:?} for {ops:?}", ms.rows, mj.rows);
        assert!(ms.ws_rest == mj.ws_rest, "worksheet rest differs for {ops:?}\n{:?}\n{:?}", ms.ws_rest, mj.ws_rest);
        assert!(ms.parts == mj.parts, "other parts differ for {ops:?}");
        panic!("(iii) projections differ for {ops:?}");
    }
    let structural: Vec<&Op> = ops.iter().zip(&j.refused).filter(|(o, r)| o.is_structural() && r.is_none()).map(|(o, _)| o).collect();
    let pr = if structural.is_empty() {
        Some(Probe { keep: base.to_vec(), records: BTreeSet::new(), chain_kept: true })
    } else {
        PROBE_RUN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        probe(base, &structural)
    };
    if pr.is_none() {
        PROBE_SKIPPED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    let al = Aligned {
        bytes: pr.as_ref().map(|p| p.keep.clone()),
        structural_sheets: structural.iter().map(|o| if let Op::Structural(s) = o { s.sheet() } else { unreachable!() }).collect(),
        chain_allowed: pr.as_ref().is_none_or(|p| p.chain_kept),
    };
    let tg = targets(ops, &j.refused);
    predicates_aligned(base, &al, &s.bytes, &s.written, &[], false).unwrap_or_else(|e| panic!("(iv) sequential predicate: {e} for {ops:?}"));
    predicates_aligned(base, &al, &j.bytes, &j.report.written, &tg, true).unwrap_or_else(|e| panic!("(iv) journal predicate: {e} for {ops:?}"));
    if let Some(a) = &al.bytes {
        let (ss, js) = (strip_set(a, &s.bytes), strip_set(a, &j.bytes));
        assert!(js.is_subset(&ss), "journal strip set {js:?} ⊄ sequential {ss:?} for {ops:?}");
    }
    // structural direction (review round 8, M6): every cell the sequential
    // side's structural ops stripped that holds `f` in the Journal's output
    // has no cached value there
    let jc = cached_in(&j.bytes);
    if !cfg!(feature = "mutant_no_structural_clause") {
        for k in &s.records {
            assert!(!jc.contains(k), "structural direction: {k:?} keeps a cache the sequential op stripped, for {ops:?}");
        }
    }
    // probe clause (review round 9, M1): a targeted cell holding `f` and a
    // cache that the probe's structural ops stripped
    if let Some(p) = &pr
        && !cfg!(feature = "mutant_no_probe_clause")
    {
        for k in &tg {
            assert!(!(p.records.contains(k) && jc.contains(k)), "probe clause: {k:?} keeps a cache a structural op of the log made stale, for {ops:?}");
        }
    }
    if j.journal.mode() == xlsx_edit_journal_spike::Mode::Shadow && structural.is_empty() {
        xlsx_edit_journal_spike::project::locality(base, &j.bytes, &j.report.written).unwrap_or_else(|e| panic!("oracle 4: {e} for {ops:?}"));
    }
    (s, j)
}

pub fn content(at: CellKey, c: Content) -> Op {
    Op::Content(at, c)
}
pub fn patch(at: CellKey, p: Patch) -> Op {
    Op::Style(at, p)
}

/// The raw `<c>` element of a cell in the output, for fixed-case checks.
pub fn raw_cell(bytes: &[u8], sheet: &str, a1: &str) -> Option<String> {
    let pkg = ooxml_core::Package::from_bytes(bytes.to_vec()).unwrap();
    let s = String::from_utf8(pkg.read_part(sheet).unwrap()).unwrap();
    let pat = format!(r#"<c r="{a1}""#);
    let a = s.find(&pat)?;
    let rest = &s[a..];
    let e1 = rest.find("/>").unwrap_or(usize::MAX);
    let e2 = rest.find("</c>").map_or(usize::MAX, |e| e + 4);
    let open_end = rest.find('>').unwrap();
    let end = if e1 + 1 == open_end { e1 + 2 } else { e2 };
    Some(rest[..end].to_owned())
}

pub fn dimension(bytes: &[u8], part: &str) -> Option<String> {
    let pkg = ooxml_core::Package::from_bytes(bytes.to_vec()).unwrap();
    let s = String::from_utf8(pkg.read_part(part).unwrap()).unwrap();
    let a = s.find("<dimension ref=\"")? + 16;
    Some(s[a..a + s[a..].find('"')?].to_owned())
}

pub const S1: &str = "xl/worksheets/sheet1.xml";
