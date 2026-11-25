//! `normalize` (ADR-0024 §4.4, N1 and N1s for one epoch) and the batch
//! builder of §7.1. Works on a copy of the log; the log is never changed.

use std::collections::{BTreeMap, HashMap};

use crate::op::{CellKey, Content, LoggedOp, Op, Patch, Start};

#[derive(Clone, Debug, PartialEq)]
#[expect(clippy::large_enum_variant, reason = "spike: ops are few and short-lived")]
pub enum EKind {
    Content(Content),
    Patch(Patch),
    /// A two-half op, kept whole.
    TwoHalf(Op),
}

/// One entry of the normal form.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    /// Where it compiles: a folded full content op takes the place of its
    /// fold segment's first content op (§7.1); every other entry its own seq.
    pub place: u64,
    pub seq: u64,
    pub key: CellKey,
    pub kind: EKind,
    /// The key's start state in the epoch (its first op's `start`).
    pub start: Start,
}

impl Entry {
    pub fn is_conditional(&self) -> bool {
        matches!(&self.kind, EKind::Content(c) if c.is_conditional())
    }
}

/// The normal form of one epoch, in compile order `(place, seq)`.
pub fn normalize(log: &[LoggedOp]) -> Vec<Entry> {
    let mut per_cell: BTreeMap<CellKey, Vec<&LoggedOp>> = BTreeMap::new();
    for l in log {
        per_cell.entry(l.op.key()).or_default().push(l);
    }
    let mut out = Vec::new();
    for (key, ops) in per_cell {
        let start = ops[0].start;
        // index in `kept` just after the latest two-half op: folding never
        // reaches across it
        let mut seg = 0usize;
        let mut seg_first_content: Option<u64> = None;
        let mut kept: Vec<Entry> = Vec::new();
        for l in ops {
            let mk = |place, kind| Entry { place, seq: l.seq, key, kind, start };
            match &l.op {
                Op::DateTime(..) | Op::NumberWithFormat(..) => {
                    kept.push(mk(l.seq, EKind::TwoHalf(l.op.clone())));
                    seg = kept.len();
                    seg_first_content = None;
                }
                Op::Content(_, c) => {
                    // §7.1 places a folded full entry at its segment's first
                    // content op. Counting a conditional op there breaks the
                    // Missing-start rule (README, ADR feedback F1); the spike
                    // counts unconditional ops only, unless the literal rule
                    // is asked for.
                    if !c.is_conditional() || cfg!(feature = "adr_literal_placement") {
                        seg_first_content.get_or_insert(l.seq);
                    }
                    let first = seg_first_content.unwrap_or(l.seq);
                    let drops = !c.is_conditional() || cfg!(feature = "mutant_cond_last_wins");
                    if drops {
                        drop_after(&mut kept, seg, |k| matches!(k, EKind::Content(_)));
                        kept.push(mk(first, EKind::Content(c.clone())));
                    } else {
                        kept.push(mk(l.seq, EKind::Content(c.clone())));
                    }
                }
                Op::Style(_, p) => {
                    if p.is_full() || cfg!(feature = "mutant_style_last_wins") {
                        drop_after(&mut kept, seg, |k| matches!(k, EKind::Patch(_)));
                    }
                    kept.push(mk(l.seq, EKind::Patch(p.clone())));
                }
                Op::Clear(_) => unreachable!("never accepted"),
            }
        }
        out.extend(kept);
    }
    out.sort_by_key(|e| (e.place, e.seq));
    out
}

fn drop_after(kept: &mut Vec<Entry>, seg: usize, pred: impl Fn(&EKind) -> bool) {
    let mut i = seg;
    while i < kept.len() {
        if pred(&kept[i].kind) { kept.remove(i); } else { i += 1; }
    }
}

/// One cell's write in a batch: at most one content (or two-half) item and
/// at most one style request, as `(CellOp, Extra { style })` allows.
#[derive(Clone, Debug)]
pub struct CellWrite {
    pub key: CellKey,
    pub content: Option<Entry>,
    pub style: Option<Entry>,
    /// The style item came first in `seq` order.
    pub style_first: bool,
    /// A conditional op: no later item for this cell joins.
    pub sealed: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Batch {
    pub cells: Vec<CellWrite>,
    index: HashMap<CellKey, usize>,
}

/// §7.1: greedy placement into batches, closed when an item cannot join.
pub fn batches(entries: &[Entry]) -> Vec<Batch> {
    let mut out = Vec::new();
    let mut cur = Batch::default();
    for e in entries {
        if !try_join(&mut cur, e) {
            out.push(std::mem::take(&mut cur));
            assert!(try_join(&mut cur, e), "an empty batch takes any entry");
        }
    }
    if !cur.cells.is_empty() {
        out.push(cur);
    }
    out
}

fn try_join(b: &mut Batch, e: &Entry) -> bool {
    let Some(&i) = b.index.get(&e.key) else {
        let mut w = CellWrite { key: e.key, content: None, style: None, style_first: false, sealed: e.is_conditional() };
        match &e.kind {
            EKind::Patch(_) => w.style = Some(e.clone()),
            _ => w.content = Some(e.clone()),
        }
        b.index.insert(e.key, b.cells.len());
        b.cells.push(w);
        return true;
    };
    let w = &mut b.cells[i];
    if w.sealed {
        return false;
    }
    match &e.kind {
        // content joins a StyleOnly write with no content
        EKind::Content(c) if !c.is_conditional() => {
            if w.content.is_none() && w.style.is_some() {
                w.content = Some(e.clone());
                w.style_first = true;
                true
            } else {
                false
            }
        }
        EKind::Content(_) | EKind::TwoHalf(_) => false,
        EKind::Patch(_) => {
            let missing_ok = !e.start.is_missing() || cfg!(feature = "mutant_merge_missing");
            let content_ok = w.content.as_ref().is_some_and(|c| matches!(c.kind, EKind::Content(_)) && !c.is_conditional());
            if w.style.is_none() && content_ok && missing_ok {
                w.style = Some(e.clone());
                true
            } else {
                false
            }
        }
    }
}
