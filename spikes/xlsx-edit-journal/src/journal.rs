//! Base, log, key index, shadow, truncation and call atomicity (ADR-0024
//! §3, §4.2–§4.5, §10.3), plus the View's two read modes (§6.1).

use std::collections::HashMap;
use std::sync::Arc;

use ooxml_xlsx::{CellValue, FormatRead, Workbook};

use crate::op::{Api, CellKey, EchoField, LoggedOp, Op, Origin, Patch, RawInput, Start, apply_seq, start_of};
use crate::shadow::{ShadowState, apply_tracked, set_shadow_policy};

/// How Commit produces the output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// ADR-0024 §7: normalize, open the Base, compile, serialize, (reopen).
    Compile,
    /// Alternative 9 with remedies 1–4 (README "Commit = shadow
    /// serialization"): the shadow is serialized and kept; no reopen.
    Shadow,
}

impl Mode {
    /// The default: `Shadow`, or `EJ_MODE=compile`.
    pub fn from_env() -> Mode {
        match std::env::var("EJ_MODE").as_deref() {
            Ok("compile") => Mode::Compile,
            _ => Mode::Shadow,
        }
    }
}

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
    /// Shadow mode: the deferred calc decision, the registries, the sheets'
    /// text layer, and the zip writing.
    pub t_calc: std::time::Duration,
    pub t_registries: std::time::Duration,
    pub t_sheets: std::time::Duration,
    pub parts_rewritten: usize,
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
    mode: Mode,
    /// Shadow mode: the per-shadow state of remedies 1–4.
    st: ShadowState,
}

impl Journal {
    pub fn open(bytes: impl Into<Arc<[u8]>>) -> JResult<Self> {
        Self::open_with(bytes, Mode::from_env())
    }

    pub fn open_with(bytes: impl Into<Arc<[u8]>>, mode: Mode) -> JResult<Self> {
        let base: Arc<[u8]> = bytes.into();
        let mut shadow = Workbook::from_bytes(base.clone())?;
        let base_xf_count = shadow.cell_format_count()?;
        let st = if mode == Mode::Shadow {
            set_shadow_policy(&mut shadow);
            ShadowState::new(&mut shadow)?
        } else {
            ShadowState::default()
        };
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
            mode,
            st,
        })
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }
    pub fn shadow_state(&self) -> &ShadowState {
        &self.st
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
            if self.mode == Mode::Shadow {
                // replay through the same remedies, from a fresh state
                set_shadow_policy(&mut wb);
                let mut st = ShadowState::new(&mut wb)?;
                for l in &self.log {
                    apply_tracked(&mut wb, &mut st, &l.op).map_err(|e| JError::JournalInternal { seq: l.seq, source: format!("replay: {e}") })?;
                }
                self.st = st;
            } else {
                for l in &self.log {
                    apply_seq(&mut wb, &l.op).map_err(|e| JError::JournalInternal { seq: l.seq, source: format!("replay: {e}") })?;
                }
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
        self.shadow()?;
        let shadow = self.shadow.as_mut().expect("built");
        let start = start_of(shadow, k)?;
        let applied = match self.mode {
            Mode::Shadow => apply_tracked(shadow, &mut self.st, &op).map(drop),
            Mode::Compile => apply_seq(shadow, &op),
        };
        if let Err(e) = applied {
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

    /// `truncate(t)` in Shadow mode without a replay (README, measured):
    /// every cell an op after `t` wrote is set back to its first-touch
    /// snapshot by ordinary writes, then that cell's ops up to `t` are
    /// applied again. Keyed ops on distinct cells do not interact, and
    /// remedies 2–4 make the leftovers (a restored node, garbage registry
    /// items, a grown `dimension`, the shadow's calc state) invisible at
    /// save. Falls back to `truncate` (replay) when a cell cannot be set
    /// back by a write: it was missing, empty, rich, inline or shared.
    /// Returns whether the fast path was taken.
    pub fn truncate_fast(&mut self, t: u64) -> JResult<bool> {
        if t < self.commit_mark {
            return Err(JError::TruncateBeforeCommit { seq: t, committed: self.commit_mark });
        }
        if self.mode != Mode::Shadow || self.shadow.is_none() {
            self.truncate(t)?;
            return Ok(false);
        }
        let keys: std::collections::BTreeSet<CellKey> = self.log.iter().filter(|l| l.seq > t).map(|l| l.op.key()).collect();
        let mut plan: Vec<Op> = Vec::new();
        for k in keys {
            let Some(snap) = self.st.touched.get(&k) else { continue };
            let restore = snap.as_ref().and_then(|s| {
                use crate::op::Content as C;
                let content = match (&s.formula, &s.value, s.t.as_deref()) {
                    (Some(f), _, _) if s.f_attrs.is_empty() => C::Formula(f.clone()),
                    (None, CellValue::Number(v), None | Some("n")) => C::Number(*v),
                    (None, CellValue::String(v), Some("s")) if !s.rich => C::String(v.clone()),
                    (None, CellValue::Bool(v), Some("b")) => C::Bool(*v),
                    (None, CellValue::Error(v), Some("e")) => C::Error(v.clone()),
                    _ => return None,
                };
                Some([Op::Content(k, content), Op::Style(k, Patch::Index(self.st.base_xf(s.s)))])
            });
            let Some(restore) = restore else {
                self.truncate(t)?;
                return Ok(false);
            };
            plan.extend(restore);
            plan.extend(self.log.iter().filter(|l| l.seq <= t && l.op.key() == k).map(|l| l.op.clone()));
        }
        // the log, key index and echo ends as `truncate` leaves them
        while self.log.last().is_some_and(|l| l.seq > t) {
            let l = self.log.pop().expect("non-empty");
            let v = self.key_index.get_mut(&l.op.key()).expect("indexed");
            v.pop();
            if v.is_empty() {
                self.key_index.remove(&l.op.key());
            }
        }
        self.echo_end.retain(|_, end| *end <= t);
        let wb = self.shadow.as_mut().expect("live");
        for op in &plan {
            if apply_tracked(wb, &mut self.st, op).is_err() {
                // not expected: the ops were accepted before; rebuild
                self.shadow = None;
                return Ok(false);
            }
        }
        Ok(true)
    }

    // ---------------------------------------------------------------- View

    /// Normalized read: the shadow's cell (§6.1). `None` for a missing cell.
    pub fn normalized(&mut self, k: CellKey) -> JResult<Option<NormCell>> {
        self.shadow()?;
        let shadow = self.shadow.as_mut().expect("built");
        let mut n = read_norm(shadow, k)?;
        if self.mode == Mode::Shadow
            && let Some(c) = &mut n
        {
            c.style_index = self.st.base_xf(c.style_index);
        }
        if self.mode == Mode::Shadow
            && let Some(c) = &mut n
            && c.formula.is_some()
        {
            // a restored cell reads its Base cache; a deferred strip reads none
            if let Some(v) = self.st.base_cache(shadow, k)? {
                c.value = v;
            }
            if self.st.all_strips(shadow)?.contains(&k) {
                c.value = CellValue::Empty;
            }
        }
        Ok(n)
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
        if self.mode == Mode::Shadow {
            return self.commit_shadow(fault);
        }
        let out = crate::commit::compile(&self.base, &self.log, fault);
        match out {
            Ok((bytes, report)) => {
                self.swap(&bytes);
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

    /// The output of a Commit in Shadow mode, without the swap (tests).
    pub fn preview(&mut self) -> JResult<Vec<u8>> {
        if self.log.is_empty() {
            return Ok(self.base.to_vec());
        }
        self.shadow()?;
        let wb = self.shadow.as_mut().expect("built");
        let mut st = self.st.clone();
        let last = self.log.last().map_or(0, |l| l.seq);
        Ok(crate::shadow::serialize(wb, &mut st, &self.base).map_err(|e| JError::JournalInternal { seq: last, source: e.to_string() })?.bytes)
    }

    /// Commit = serialize the shadow (README). The shadow is kept: the
    /// state of remedies 2–4 carries the difference between its bytes and
    /// the new Base.
    fn commit_shadow(&mut self, fault: Option<Fault>) -> JResult<(Vec<u8>, CommitReport)> {
        let last = self.log.last().expect("non-empty").seq;
        if fault == Some(Fault::Normalize) || fault == Some(Fault::OpenWorkingCopy) || matches!(fault, Some(Fault::Batch(_) | Fault::PanicInBatch(_))) {
            // no such step in this mode; the fault fires before anything
            return Err(JError::JournalInternal { seq: last, source: format!("injected: {fault:?}") });
        }
        self.shadow()?;
        let wb = self.shadow.as_mut().expect("built");
        let t = std::time::Instant::now();
        if fault == Some(Fault::Serialize) {
            return Err(JError::JournalInternal { seq: last, source: "injected: serialize".into() });
        }
        // serialize works on a copy of the state: a failure leaves it as is
        let mut st = self.st.clone();
        let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| crate::shadow::serialize(wb, &mut st, &self.base)));
        let s = match out {
            Ok(Ok(s)) => s,
            Ok(Err(e)) => return Err(JError::JournalInternal { seq: last, source: e.to_string() }),
            Err(_) => return Err(JError::JournalInternal { seq: last, source: "panic during Commit".into() }),
        };
        if fault == Some(Fault::Swap) {
            return Err(JError::JournalInternal { seq: last, source: "injected: before swap".into() });
        }
        let report = CommitReport {
            entries: self.log.len(),
            skipped_noops: self.st.noops,
            written: s.effective.clone(),
            t_serialize: t.elapsed(),
            t_calc: s.t_calc,
            t_registries: s.t_registries,
            t_sheets: s.t_sheets,
            parts_rewritten: s.parts_rewritten,
            ..Default::default()
        };
        st.after_commit(wb, &s.calc, s.reg).map_err(|e| JError::JournalInternal { seq: last, source: e.to_string() })?;
        st.noops = 0;
        self.st = st;
        self.swap(&s.bytes);
        // the new Base's `xf` count (P2 test of `set_style_index`)
        self.base_xf_count = self.st.reg.origin[0] + self.st.reg.maps[0].len() as u32;
        Ok((s.bytes, report))
    }

    /// The swap of §7 step 6: the output is the new Base; echo records
    /// survive (§10.1).
    fn swap(&mut self, bytes: &[u8]) {
        {
            {
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
                self.base = bytes.to_vec().into();
                self.log.clear();
                self.key_index.clear();
                self.echo_end.clear();
                self.commit_mark = last;
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
