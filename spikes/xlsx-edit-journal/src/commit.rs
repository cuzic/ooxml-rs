//! Commit compilation (ADR-0024 §7) through `ooxml-xlsx`'s public API.
//!
//! The batch of §7.1 is one `CellWritePlan` with `(CellOp, Extra { style })`
//! per cell. That plan type is crate-private; the public batch call
//! `Worksheet::write_cells` carries only numbers, strings, formulas and
//! `RemoveFormula`, without a style request. So a batch is executed as:
//!
//! - phase A: style requests that must resolve on the row/col default: a
//!   style-only write, and a merged write whose cell is missing at that
//!   point (one plan would create the cell with `missing_cell_style`);
//! - phase B: content, one `write_cells` per sheet for the kinds it carries
//!   (on cells whose start state is not `Formula`, so no two cells of a
//!   shared group meet in one call: `SharedFormulaMaster`), the rest
//!   (booleans, errors, two-half ops, formula-start cells) one call each;
//! - phase C: style requests merged after content on an existing cell.
//!
//! Cells in a batch are distinct, and a content op never reads `s` nor a
//! patch the value, so the phases give the merged write's result.

use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use ooxml_xlsx::{CellIndex, CellValue, CellWrite as XWrite, Workbook};

use crate::journal::{CommitReport, Fault, JError, JResult};
use crate::normalize::{EKind, Entry, batches, normalize};
use crate::op::{CellKey, Content, LoggedOp, Op, Patch, Start, apply_patch, apply_seq};

fn internal(seq: u64, what: impl std::fmt::Display) -> JError {
    JError::JournalInternal { seq, source: what.to_string() }
}

pub fn compile(base: &Arc<[u8]>, log: &[LoggedOp], fault: Option<Fault>) -> JResult<(Vec<u8>, CommitReport)> {
    let last = log.last().map_or(0, |l| l.seq);
    match catch_unwind(AssertUnwindSafe(|| compile_inner(base, log, fault))) {
        Ok(r) => r,
        Err(_) => Err(internal(last, "panic during Commit")),
    }
}

fn compile_inner(base: &Arc<[u8]>, log: &[LoggedOp], fault: Option<Fault>) -> JResult<(Vec<u8>, CommitReport)> {
    let last = log.last().map_or(0, |l| l.seq);
    if fault == Some(Fault::Normalize) {
        return Err(internal(last, "injected: normalize"));
    }
    // 1. normalize a copy of the log
    let entries = normalize(log);
    let bs = batches(&entries);
    // 2. (the caller dropped the shadow) 3. working copy
    if fault == Some(Fault::OpenWorkingCopy) {
        return Err(internal(last, "injected: open"));
    }
    let t0 = std::time::Instant::now();
    let mut wc = Workbook::from_bytes(base.clone()).map_err(|e| internal(last, e))?;
    let t_open = t0.elapsed();
    let date1904 = wc.date1904().map_err(|e| internal(last, e))?;
    let mut report = CommitReport { entries: entries.len(), batches: bs.len(), t_open, ..Default::default() };
    let t1 = std::time::Instant::now();
    // 4. compile and apply
    for (bi, b) in bs.iter().enumerate() {
        if fault == Some(Fault::Batch(bi)) {
            return Err(internal(last, format!("injected: batch {bi}")));
        }
        if fault == Some(Fault::PanicInBatch(bi)) {
            panic!("injected panic in batch {bi}");
        }
        let mut phase_a: Vec<&Entry> = Vec::new();
        let mut phase_c: Vec<&Entry> = Vec::new();
        let mut contents: Vec<&Entry> = Vec::new();
        for w in &b.cells {
            let cell = read(&mut wc, w.key).map_err(|e| internal(w.content.as_ref().or(w.style.as_ref()).map_or(last, |e| e.seq), e))?;
            let content = w.content.as_ref().filter(|c| {
                let skip = is_noop_content(&cell, c, date1904);
                report.skipped_noops += skip as usize;
                !skip
            });
            let style = w.style.as_ref().filter(|s| {
                let skip = is_noop_style(&cell, s);
                report.skipped_noops += skip as usize;
                !skip
            });
            if let Some(c) = content {
                // a conditional op on a cell without a formula targets nothing
                let skipped_by_api = c.is_conditional() && cell.as_ref().is_none_or(|x| x.formula.is_none());
                if !skipped_by_api {
                    report.written.push(w.key);
                }
                contents.push(c);
            }
            if let Some(s) = style {
                report.written.push(w.key);
                if content.is_none() || w.style_first || cell.is_none() {
                    phase_a.push(s);
                } else {
                    phase_c.push(s);
                }
            }
        }
        for s in phase_a {
            run_patch(&mut wc, s, &mut report)?;
        }
        let mut grouped: BTreeMap<usize, Vec<(CellIndex, XWrite<'_>, u64)>> = BTreeMap::new();
        for c in contents {
            let w = match (&c.kind, c.start) {
                (_, Start::Formula) => None,
                (EKind::Content(Content::Number(v)), _) => Some(XWrite::Number(*v)),
                (EKind::Content(Content::String(s)), _) => Some(XWrite::String(s)),
                (EKind::Content(Content::Formula(t)), _) => Some(XWrite::Formula(t)),
                (EKind::Content(Content::RemoveFormula), _) => Some(XWrite::RemoveFormula),
                _ => None,
            };
            match w {
                Some(w) => grouped.entry(c.key.sheet).or_default().push((CellIndex { row: c.key.row, col: c.key.col }, w, c.seq)),
                None => {
                    report.single_calls += 1;
                    let op = match &c.kind {
                        EKind::Content(x) => Op::Content(c.key, x.clone()),
                        EKind::TwoHalf(op) => op.clone(),
                        EKind::Patch(_) => unreachable!(),
                    };
                    apply_seq(&mut wc, &op).map_err(|e| internal(c.seq, e))?;
                }
            }
        }
        for (sheet, ws) in grouped {
            report.write_cells_calls += 1;
            let first = ws[0].2;
            let writes: Vec<(CellIndex, XWrite<'_>)> = ws.into_iter().map(|(i, w, _)| (i, w)).collect();
            wc.worksheet_by_index(sheet).and_then(|mut s| s.write_cells(&writes)).map_err(|e| internal(first, e))?;
        }
        for s in phase_c {
            run_patch(&mut wc, s, &mut report)?;
        }
    }
    report.t_apply = t1.elapsed();
    // 5. serialize once
    let t2 = std::time::Instant::now();
    if fault == Some(Fault::Serialize) {
        return Err(internal(last, "injected: serialize"));
    }
    let bytes = wc.to_bytes().map_err(|e| internal(last, e))?;
    report.t_serialize = t2.elapsed();
    if fault == Some(Fault::Swap) {
        return Err(internal(last, "injected: before swap"));
    }
    Ok((bytes, report))
}

fn run_patch(wc: &mut Workbook, s: &Entry, report: &mut CommitReport) -> JResult<()> {
    let EKind::Patch(p) = &s.kind else { unreachable!() };
    report.single_calls += 1;
    let k = s.key;
    wc.worksheet_by_index(k.sheet)
        .and_then(|mut ws| apply_patch(ws.cell_mut(k.row, k.col)?, p))
        .map_err(|e| internal(s.seq, e))
}

/// The working copy's cell, for the no-op test of §9.2.
pub struct Cell {
    value: CellValue,
    pub formula: Option<String>,
    formula_plain: bool,
    t: Option<String>,
    rich: bool,
    s: u32,
    is_date: bool,
}

fn read(wb: &mut Workbook, k: CellKey) -> ooxml_xlsx::Result<Option<Cell>> {
    let mut ws = wb.worksheet_by_index(k.sheet)?;
    let Some(cell) = ws
        .cell(k.row, k.col)?
        .map(|c| {
            let raw = c.raw();
            let f = raw.elements().find(|e| e.name().local() == "f");
            let rich = raw.elements().any(|e| e.name().local() == "is" && e.elements().any(|r| r.name().local() == "r"));
            Ok::<_, ooxml_xlsx::Error>(Cell {
                value: c.value()?,
                formula: c.formula(),
                formula_plain: f.is_some_and(|f| f.attrs().is_empty()),
                t: raw.attr(ooxml_core::QName::new("", "t")).map(str::to_owned),
                rich,
                s: c.style_index()?,
                is_date: false,
            })
        })
        .transpose()?
    else {
        return Ok(None);
    };
    let is_date = ws.is_date(k.row, k.col)?;
    Ok(Some(Cell { is_date, ..cell }))
}

/// §9.2: only on an **existing** cell, and only when the payload reads equal.
fn is_noop_content(cell: &Option<Cell>, e: &Entry, date1904: bool) -> bool {
    let Some(c) = cell else { return false };
    if c.formula.is_some() {
        // a formula never reads equal to a constant
        return matches!(&e.kind, EKind::Content(Content::Formula(t)) if c.formula_plain && c.formula.as_deref() == Some(t.strip_prefix('=').unwrap_or(t)));
    }
    let t = c.t.as_deref();
    match &e.kind {
        EKind::Content(Content::Number(v)) => matches!(t, None | Some("n")) && c.value == CellValue::Number(*v),
        EKind::Content(Content::String(s)) => matches!(t, Some("s" | "inlineStr")) && !c.rich && c.value == CellValue::String(s.clone()),
        EKind::Content(Content::Bool(b)) => c.value == CellValue::Bool(*b),
        EKind::Content(Content::Error(x)) => c.value == CellValue::Error(x.clone()),
        EKind::TwoHalf(Op::DateTime(_, d)) => {
            matches!(t, None | Some("n")) && c.is_date && d.serial(date1904).is_some_and(|v| c.value == CellValue::Number(v))
        }
        _ => false,
    }
}

/// A style entry is a no-op when its resolved `xf` equals the cell's `s`.
/// The public API cannot resolve a patch without applying it, so only
/// `Index(i)` is tested here (README, ADR feedback F2).
fn is_noop_style(cell: &Option<Cell>, e: &Entry) -> bool {
    let EKind::Patch(p) = &e.kind else { return false };
    match cell {
        Some(c) => matches!(p, Patch::Index(i) if *i == c.s),
        None => cfg!(feature = "mutant_noop_missing") && matches!(p, Patch::Index(0)),
    }
}
