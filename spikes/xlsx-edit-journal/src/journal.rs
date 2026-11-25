//! Base, log, key index, shadow, truncation and call atomicity (ADR-0024
//! §3, §4.2–§4.5, §10.3), plus the View's two read modes (§6.1).

use std::collections::HashMap;
use std::sync::Arc;

use ooxml_xlsx::{CellValue, FormatRead, Workbook};

use crate::op::{Api, CellKey, EchoField, LoggedOp, Op, Origin, Patch, RawInput, Start, apply_seq, start_of};

/// Errors of the Journal (ADR-0024 §12). `Xlsx` is "the same error" the
/// sequential API returned on the shadow.
#[derive(Debug)]
pub enum JError {
    Xlsx(ooxml_xlsx::Error),
    Unsupported { op: &'static str },
    TruncateBeforeCommit { seq: u64, committed: u64 },
    JournalInternal { seq: u64, source: String },
}

impl From<ooxml_xlsx::Error> for JError {
    fn from(e: ooxml_xlsx::Error) -> Self {
        JError::Xlsx(e)
    }
}

pub type JResult<T> = Result<T, JError>;

/// Fault injection for oracle 3 (each step of §7, plus a shadow apply).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    Normalize,
    OpenWorkingCopy,
    /// Before batch `n` (0-based).
    Batch(usize),
    Serialize,
    /// After serialization, before the swap.
    Swap,
    /// Panic inside batch `n`.
    PanicInBatch(usize),
}

/// A normalized read of one cell (from the shadow).
#[derive(Clone, Debug, PartialEq)]
pub struct NormCell {
    /// The value; for a formula cell, its cached result (`Empty` if stripped).
    pub value: CellValue,
    pub formula: Option<String>,
    pub style_index: u32,
    pub format: FormatRead,
    pub is_date: bool,
}

/// Counters for the README's measurements.
#[derive(Clone, Debug, Default)]
pub struct CommitReport {
    pub entries: usize,
    pub batches: usize,
    pub skipped_noops: usize,
    pub write_cells_calls: usize,
    pub single_calls: usize,
    /// Cells some call's plan targeted (for the `dimension` predicate).
    pub written: Vec<CellKey>,
    /// Wall time of §7 steps 3, 4 and 5.
    pub t_open: std::time::Duration,
    pub t_apply: std::time::Duration,
    pub t_serialize: std::time::Duration,
}

pub struct Journal {
    pub(crate) base: Arc<[u8]>,
    pub(crate) log: Vec<LoggedOp>,
    /// key → [seq], newest last (§4.3). One entry per cell; a two-half op
    /// and a content op and a patch all land on the cell.
    key_index: HashMap<CellKey, Vec<u64>>,
    next_seq: u64,
    commit_mark: u64,
    shadow: Option<Workbook>,
    /// seq of an op → seq of the op that ended its echo, per field.
    echo_end: HashMap<(u64, EchoField), u64>,
    /// Echo records left by a Commit (§10.1): field → (seq, raw input).
    echo_records: HashMap<(CellKey, EchoField), (u64, RawInput)>,
    base_xf_count: u32,
    pub fault: Option<Fault>,
    /// Number of shadow rebuilds (replays), for tests and measurements.
    pub replays: usize,
}

impl Journal {
    pub fn open(bytes: impl Into<Arc<[u8]>>) -> JResult<Self> {
        let base: Arc<[u8]> = bytes.into();
        let mut shadow = Workbook::from_bytes(base.clone())?;
        let base_xf_count = shadow.cell_format_count()?;
        Ok(Journal {
            base,
            log: Vec::new(),
            key_index: HashMap::new(),
            next_seq: 1,
            commit_mark: 0,
            shadow: Some(shadow),
            echo_end: HashMap::new(),
            echo_records: HashMap::new(),
            base_xf_count,
            fault: None,
            replays: 0,
        })
    }

    pub fn base(&self) -> &Arc<[u8]> {
        &self.base
    }
    pub fn log(&self) -> &[LoggedOp] {
        &self.log
    }
    pub fn next_seq(&self) -> u64 {
        self.next_seq
    }
    pub fn commit_mark(&self) -> u64 {
        self.commit_mark
    }
    pub fn shadow_is_stale(&self) -> bool {
        self.shadow.is_none()
    }
    pub fn mark_shadow_stale(&mut self) {
        self.shadow = None;
    }

    /// The shadow, rebuilt by replay when stale (§3.2).
    pub fn shadow(&mut self) -> JResult<&mut Workbook> {
        if self.shadow.is_none() {
            self.replays += 1;
            let mut wb = Workbook::from_bytes(self.base.clone())?;
            for l in &self.log {
                apply_seq(&mut wb, &l.op).map_err(|e| JError::JournalInternal { seq: l.seq, source: format!("replay: {e}") })?;
            }
            self.shadow = Some(wb);
        }
        Ok(self.shadow.as_mut().expect("just built"))
    }

    /// Appends one op as one call.
    pub fn append(&mut self, api: Api, op: Op, raw: Option<RawInput>) -> JResult<u64> {
        Ok(self.append_call(api, vec![(op, raw)])?[0])
    }

    /// Appends a call of several ops, all-or-nothing (§4.5). On a refusal
    /// the log, key index, `seq` counter and echo ends are as before the
    /// call, and the shadow is restored by replay (if an op had applied).
    pub fn append_call(&mut self, api: Api, ops: Vec<(Op, Option<RawInput>)>) -> JResult<Vec<u64>> {
        self.shadow()?;
        let saved_len = self.log.len();
        let saved_seq = self.next_seq;
        let mut new_ends: Vec<(u64, EchoField)> = Vec::new();
        let mut seqs = Vec::new();
        for (i, (op, raw)) in ops.into_iter().enumerate() {
            match self.append_one(api, op, raw, &mut new_ends) {
                Ok(seq) => seqs.push(seq),
                Err(e) => {
                    if i > 0 {
                        // undo the call: log, key index, seq counter, echo ends, shadow
                        for l in self.log.drain(saved_len..) {
                            let v = self.key_index.get_mut(&l.op.key()).expect("indexed");
                            v.pop();
                            if v.is_empty() {
                                self.key_index.remove(&l.op.key());
                            }
                        }
                        for k in &new_ends {
                            self.echo_end.remove(k);
                        }
                        self.next_seq = saved_seq;
                        #[cfg(not(feature = "mutant_keep_shadow"))]
                        {
                            self.shadow = None;
                        }
                    }
                    return Err(e);
                }
            }
        }
        Ok(seqs)
    }

    fn append_one(&mut self, api: Api, op: Op, raw: Option<RawInput>, new_ends: &mut Vec<(u64, EchoField)>) -> JResult<u64> {
        if let Op::Clear(_) = op {
            return Err(JError::Unsupported { op: "clear (P1)" });
        }
        if let Op::Style(_, Patch::Index(i)) = op
            && i >= self.base_xf_count
        {
            // an index beyond the Base's: either the sequential refusal, or
            // an `xf` this session created (P2)
            let shadow = self.shadow()?;
            if i < shadow.cell_format_count()? {
                return Err(JError::Unsupported { op: "set_style_index on a session xf (P2)" });
            }
        }
        let seq = self.next_seq;
        let k = op.key();
        let shadow = self.shadow()?;
        let start = start_of(shadow, k)?;
        if let Err(e) = apply_seq(shadow, &op) {
            if matches!(e, ooxml_xlsx::Error::Poisoned(_)) || shadow.edit_refusal().is_some_and(|r| matches!(r, ooxml_xlsx::Error::Poisoned(_))) {
                self.shadow = None;
                return Err(JError::JournalInternal { seq, source: format!("shadow apply: {e}") });
            }
            return Err(e.into());
        }
        // echo ends: an earlier op on this key whose fields this op sets
        if let Some(seqs) = self.key_index.get(&k) {
            for &s in seqs {
                let old = &self.log[self.log.binary_search_by_key(&s, |l| l.seq).expect("logged")].op;
                for f in op.echo_fields() {
                    if old.echo_fields().contains(f) && !self.echo_end.contains_key(&(s, *f)) {
                        self.echo_end.insert((s, *f), seq);
                        new_ends.push((s, *f));
                    }
                }
            }
        }
        for f in op.echo_fields() {
            self.echo_records.remove(&(k, *f));
        }
        self.next_seq += 1;
        #[cfg(feature = "mutant_fold_at_append")]
        {
            // B2's mutant: fold at append (last-wins on the content key)
            if let Op::Content(..) = op {
                let drop: Vec<u64> = self.log.iter().filter(|l| l.op.key() == k && matches!(l.op, Op::Content(..))).map(|l| l.seq).collect();
                self.log.retain(|l| !drop.contains(&l.seq));
                if let Some(v) = self.key_index.get_mut(&k) {
                    v.retain(|s| !drop.contains(s));
                }
            }
        }
        self.log.push(LoggedOp { seq, origin: Origin::Session { seq, api, raw_input: raw }, start, op });
        self.key_index.entry(k).or_default().push(seq);
        Ok(seq)
    }

    /// `truncate(t)`: drop every op with `seq > t` (§10.3).
    pub fn truncate(&mut self, t: u64) -> JResult<()> {
        if t < self.commit_mark {
            return Err(JError::TruncateBeforeCommit { seq: t, committed: self.commit_mark });
        }
        if self.log.iter().all(|l| l.seq <= t) {
            return Ok(());
        }
        while self.log.last().is_some_and(|l| l.seq > t) {
            let l = self.log.pop().expect("non-empty");
            let v = self.key_index.get_mut(&l.op.key()).expect("indexed");
            v.pop();
            if v.is_empty() {
                self.key_index.remove(&l.op.key());
            }
        }
        self.echo_end.retain(|_, end| *end <= t);
        // the seq counter is not rolled back: seqs are never reissued
        self.shadow = None;
        Ok(())
    }

    // ---------------------------------------------------------------- View

    /// Normalized read: the shadow's cell (§6.1). `None` for a missing cell.
    pub fn normalized(&mut self, k: CellKey) -> JResult<Option<NormCell>> {
        read_norm(self.shadow()?, k)
    }

    /// Echo read for one field: the newest live Session op's raw input.
    pub fn echo(&self, k: CellKey, field: EchoField) -> Option<&RawInput> {
        if let Some(seqs) = self.key_index.get(&k) {
            for &s in seqs.iter().rev() {
                let l = &self.log[self.log.binary_search_by_key(&s, |l| l.seq).expect("logged")];
                if !l.op.echo_fields().contains(&field) {
                    continue;
                }
                if self.echo_end.contains_key(&(s, field)) {
                    return None;
                }
                return match &l.origin {
                    Origin::Session { raw_input: Some(r), .. } => Some(r),
                    _ => None,
                };
            }
        }
        self.echo_records.get(&(k, field)).map(|(_, r)| r)
    }

    // -------------------------------------------------------------- Commit

    /// Commit (§7): normalize a copy of the log, drop the shadow, compile on
    /// a working copy opened from the Base, serialize once, swap.
    pub fn commit(&mut self) -> JResult<(Vec<u8>, CommitReport)> {
        if self.log.is_empty() {
            return Ok((self.base.to_vec(), CommitReport::default()));
        }
        let fault = self.fault;
        let out = crate::commit::compile(&self.base, &self.log, fault);
        match out {
            Ok((bytes, report)) => {
                // swap: the output is the new Base; echo records survive
                let last = self.log.last().expect("non-empty").seq;
                for l in &self.log {
                    if let Origin::Session { raw_input: Some(r), .. } = &l.origin {
                        for f in l.op.echo_fields() {
                            if !self.echo_end.contains_key(&(l.seq, *f)) {
                                self.echo_records.insert((l.op.key(), *f), (l.seq, r.clone()));
                            }
                        }
                    }
                }
                self.base = bytes.clone().into();
                self.log.clear();
                self.key_index.clear();
                self.echo_end.clear();
                self.commit_mark = last;
                self.shadow = None; // reopened lazily from the new Base (§10.1)
                Ok((bytes, report))
            }
            Err(e) => {
                // Base and log unchanged; the shadow was dropped at step 2 and
                // is rebuilt by replay on the next read or append
                self.shadow = None;
                Err(e)
            }
        }
    }
}

pub fn read_norm(wb: &mut Workbook, k: CellKey) -> JResult<Option<NormCell>> {
    let mut ws = wb.worksheet_by_index(k.sheet)?;
    let Some((value, formula, s)) = ws.cell(k.row, k.col)?.map(|c| Ok::<_, ooxml_xlsx::Error>((c.value()?, c.formula(), c.style_index()?))).transpose()? else {
        return Ok(None);
    };
    let is_date = ws.is_date(k.row, k.col)?;
    let format = wb.format_of_style(s)?;
    Ok(Some(NormCell { value, formula, style_index: s, format, is_date }))
}

#[doc(hidden)]
impl Journal {
    /// Test hook: the live shadow, if any.
    pub fn __shadow_if_live(&mut self) -> Option<&mut Workbook> {
        self.shadow.as_mut()
    }
    /// Test hook: a fingerprint of the internal state oracle 3 compares.
    pub fn __state(&self) -> String {
        let mut ends: Vec<_> = self.echo_end.iter().map(|((s, f), e)| format!("{s}:{f:?}>{e}")).collect();
        ends.sort();
        let mut keys: Vec<_> = self.key_index.iter().map(|(k, v)| format!("{k:?}={v:?}")).collect();
        keys.sort();
        format!("log={:?}\nseq={}\nmark={}\nkeys={keys:?}\nends={ends:?}", self.log.iter().map(|l| (l.seq, &l.op, l.start)).collect::<Vec<_>>(), self.next_seq, self.commit_mark)
    }
}

impl std::fmt::Display for JError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl Start {
    pub fn is_missing(self) -> bool {
        self == Start::Missing
    }
}
