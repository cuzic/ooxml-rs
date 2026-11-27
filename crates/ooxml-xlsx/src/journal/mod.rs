//! The xlsx Edit Journal, delivery J0a (ADR-0024 §3, §4, §7.1 R1/R2):
//! keyed ops on cells (content and style) planned on a shadow workbook and
//! logged by id, with R1's no-op test after validation (§4.5) on C1's plan
//! report and the snapshot-and-write-back rule R2 on C2's snapshots. No
//! save, no Commit, no structural ops (J1, J2).
//!
//! Stand-ins, until their core APIs land (decisions A2, A16 of the task):
//! C8's typed cell-state reads are `state.rs`'s raw reads; C3's deferred
//! caches are the shadow's `KeepStaleCaches` policy.
//!
//! Nothing here is public API.

#![cfg_attr(not(test), expect(dead_code))]
#![cfg_attr(test, allow(dead_code))]

pub(crate) mod ids;
pub(crate) mod op;
mod replay;
pub(crate) mod state;

#[cfg(test)]
mod project;
#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use ids::{AxisVec, ColId, Epoch, IdCounter, RowId, SheetId};
use op::{
    Api, CellCall, Field, Key, LoggedOp, Op, OpInput, OpKind, Origin, WriteValue, apply_seq,
    cell_write, col_prop, fields, plan_call, row_prop, writes_content,
};
use replay::Mark;
use state::{CellState, state_of};

use crate::dynamic::MetaAction;
use crate::edit::{CellOp, CellWritePlan, Extra};
use crate::error::{Error, Result};
use crate::reference::{CellIndex, MAX_COLS, MAX_ROWS};
use crate::registry::styles::BaseStyleCounts;
use crate::snapshot::{CellSnapshot, RestoreStyle};
use crate::tables::StagedTables;
use crate::workbook::Workbook;

#[derive(Debug)]
pub(crate) enum JournalError {
    /// The sequential call refused the op (nothing changed).
    Op(Error),
    // (an `Unsupported { op }` variant returns when ADR-0024 P3-P6 enter the op set)
    /// The shadow was poisoned or a restore failed: the call was undone by
    /// replay (the op is not logged and `seq` is not consumed). If the replay
    /// failed too, the Journal is broken.
    Internal { seq: u64, source: Error },
    /// After a replay failed (§9.3); `truncate` and reload are J3.
    Broken,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct CellKey {
    pub(crate) sheet: SheetId,
    pub(crate) row: RowId,
    pub(crate) col: ColId,
}

/// A cell the Journal has opened a snapshot for (§4.4): the first touch of
/// the session, kept until a commit (J1).
struct Tracked {
    snap: CellSnapshot,
    state: CellState,
    /// The cell differs from its snapshot state now (§4.4 "changed").
    differs: bool,
}

pub(crate) struct Journal {
    base: Arc<[u8]>,
    shadow: Workbook,
    /// The style table sizes of the Base, read once at open (T5b): `Base(i)`
    /// components of a logged `xf` are checked against them.
    base_counts: BaseStyleCounts,
    /// Shadow sheet index to id.
    sheets: Vec<SheetId>,
    rows: HashMap<SheetId, AxisVec<RowId>>,
    cols: HashMap<SheetId, AxisVec<ColId>>,
    ids: IdCounter,
    log: Vec<LoggedOp>,
    /// The seq of the next op; the first is 1.
    next_seq: u64,
    keys: HashMap<Key, Vec<u64>>,
    tracked: BTreeMap<CellKey, Tracked>,
    broken: bool,
    /// Set while a call runs; a call that panicked leaves it (§3.3).
    stale: Option<Mark>,
    #[cfg(test)]
    stats: RestoreStats,
    /// Test hooks (`replay.rs`).
    #[cfg(all(test, feature = "fault-injection"))]
    replay_fault: Option<(crate::workbook::ApplyStep, bool)>,
    #[cfg(test)]
    /// `Some(panic)`: the next restore fails (or panics).
    fail_restore_once: Option<bool>,
    #[cfg(test)]
    drop_fresh_once: bool,
}

/// R2's restores by kind of the snapshot's content (test coverage only).
#[cfg(test)]
#[derive(Debug, Default, Clone)]
pub(crate) struct RestoreStats {
    pub(crate) full: HashMap<&'static str, u32>,
    pub(crate) content: u32,
}

/// A cell the op touches: where, and whether the op names it (a hand-off's
/// new master is a target without being named).
struct Target {
    at: CellIndex,
    named: bool,
}

/// What an apply installs besides the cell plan: staged header renames,
/// or the metadata work of a dynamic write.
enum Staged {
    Headers(StagedTables),
    Dynamic(MetaAction),
}

impl Staged {
    /// Whether the staged work is empty: only then may R1 skip (a staged
    /// header rename is a real change even when the cell report is
    /// all-same, e.g. an identity write to a diverged header cell). A
    /// dynamic write always stages metadata work.
    fn is_empty(&self) -> bool {
        match self {
            Staged::Headers(staged) => staged.is_empty(),
            Staged::Dynamic(_) => false,
        }
    }
}

impl Journal {
    pub(crate) fn open(base: impl Into<Arc<[u8]>>) -> Result<Journal> {
        let base: Arc<[u8]> = base.into();
        let mut shadow = Journal::open_shadow(&base)?;
        let base_counts = shadow.base_style_counts()?;
        let sheets = sheet_ids(&mut shadow)?;
        // identity vectors for every sheet, so a refused op creates nothing
        let rows = sheets
            .iter()
            .map(|s| (*s, AxisVec::base(MAX_ROWS)))
            .collect();
        let cols = sheets
            .iter()
            .map(|s| (*s, AxisVec::base(MAX_COLS)))
            .collect();
        Ok(Journal {
            base,
            shadow,
            base_counts,
            sheets,
            rows,
            cols,
            ids: IdCounter::new(),
            log: Vec::new(),
            next_seq: 1,
            keys: HashMap::new(),
            tracked: BTreeMap::new(),
            broken: false,
            stale: None,
            #[cfg(test)]
            stats: RestoreStats::default(),
            #[cfg(all(test, feature = "fault-injection"))]
            replay_fault: None,
            #[cfg(test)]
            fail_restore_once: None,
            #[cfg(test)]
            drop_fresh_once: false,
        })
    }

    pub(crate) fn shadow(&self) -> &Workbook {
        &self.shadow
    }

    pub(crate) fn base(&self) -> &Arc<[u8]> {
        &self.base
    }

    pub(crate) fn log(&self) -> &[LoggedOp] {
        &self.log
    }

    /// The seqs that wrote `key`, newest last.
    pub(crate) fn key_seqs(&self, key: &Key) -> &[u64] {
        self.keys.get(key).map_or(&[], Vec::as_slice)
    }

    /// The shadow sheet index of an id.
    pub(crate) fn sheet_index_of(&self, id: SheetId) -> Option<usize> {
        self.sheets.iter().position(|s| *s == id)
    }

    pub(crate) fn sheet_id(&self, sheet: usize) -> SheetId {
        self.sheets[sheet]
    }

    /// The cell's key, through the id vectors.
    pub(crate) fn cell_key(&self, sheet: usize, at: CellIndex) -> CellKey {
        let sid = self.sheets[sheet];
        let row = self.rows[&sid].id_at(at.row);
        let col = self.cols[&sid].id_at(at.col);
        CellKey {
            sheet: sid,
            row,
            col,
        }
    }

    /// Appends one op: validates it on the shadow by applying it, then logs
    /// it and runs R2 on every cell it touched. `Err(Op)` leaves everything as
    /// it was (P2's `set_style_index(i)` for a session `xf` that `xf_content`
    /// refuses, a dangling Base reference or extended content, is `Err(Op(..))`
    /// though the sequential API may accept it: part B is outside oracle 1 (i),
    /// F8); `Err(Internal)` has undone the call by
    /// replay (§3.2, §9.3), so the Journal is as it was unless it is `broken`.
    pub(crate) fn append(
        &mut self,
        api: Api,
        input: OpInput,
    ) -> std::result::Result<u64, JournalError> {
        if self.broken {
            return Err(JournalError::Broken);
        }
        self.settle()?;
        let mark = Mark {
            log_len: self.log.len(),
            next_seq: self.next_seq,
        };
        self.stale = Some(mark);
        let r = self.append_inner(api, &input);
        // a panic skips this line: the next call finds the mark
        self.stale = None;
        match r {
            Err(JournalError::Internal { seq, source }) => Err(self.recover(mark, seq, source)),
            r => r,
        }
    }

    /// Undoes a call that panicked, or left the shadow poisoned, by replay.
    /// `Err` if the replay failed (the Journal is then broken).
    pub(crate) fn settle(&mut self) -> std::result::Result<(), JournalError> {
        if self.stale.is_some() || self.shadow.check_poisoned().is_err() {
            let mark = self.stale.take().unwrap_or(Mark {
                log_len: self.log.len(),
                next_seq: self.next_seq,
            });
            let source = self
                .shadow
                .check_poisoned()
                .err()
                .unwrap_or_else(|| Error::Malformed("a call was interrupted".into()));
            // the interrupted call's seq: it was not logged, `mark` has it
            let e = self.recover(mark, mark.next_seq, source);
            if self.broken {
                return Err(e);
            }
        }
        Ok(())
    }

    fn append_inner(
        &mut self,
        api: Api,
        input: &OpInput,
    ) -> std::result::Result<u64, JournalError> {
        // P2 (F12): a session `xf` is logged and applied by content, live and
        // in replay alike; an index past the table stays an index, refused by
        // the sequential call as usual
        if let OpKind::Cell { call: CellCall::StyleXf(_, c), .. } = &input.kind {
            // never given by a caller: only the conversion below makes it
            debug_assert!(*c == self.base_counts, "a caller-made StyleXf");
        }
        let by_content;
        let input = match &input.kind {
            OpKind::Cell {
                at,
                call: CellCall::StyleIndex(i),
            } if *i >= self.base_counts.cell_xfs
                && self.shadow.worksheet_by_index(input.sheet).is_ok() =>
            {
                let now = self.shadow.cell_format_count().map_err(JournalError::Op)?;
                if *i < now {
                    let x = self
                        .shadow
                        .xf_content(*i, &self.base_counts)
                        .map_err(JournalError::Op)?;
                    by_content = OpInput {
                        sheet: input.sheet,
                        kind: OpKind::Cell {
                            at: *at,
                            call: CellCall::StyleXf(Box::new(x), self.base_counts),
                        },
                    };
                    &by_content
                } else {
                    input
                }
            }
            _ => input,
        };
        let seq = self.next_seq;
        let acc = self.accept(input, seq)?;
        self.next_seq += 1;
        self.log.push(LoggedOp {
            seq,
            origin: Origin::Session { seq, api },
            born: Epoch(0),
            applied: acc.applied,
            op: acc.op,
        });
        self.r2(input, &acc.targets, acc.snapshot_failed, seq)?;
        Ok(seq)
    }

    /// Everything up to and including the keys, shared by `append` and
    /// replay: the plan, R1's test, skip or apply, the key index. Nothing
    /// is logged here. `Err(Op)` changed nothing.
    fn accept(&mut self, input: &OpInput, seq: u64) -> std::result::Result<Accepted, JournalError> {
        if let Err(e) = self.shadow.worksheet_by_index(input.sheet).map(drop) {
            return Err(JournalError::Op(e));
        }
        match &input.kind {
            // row/column properties target no cell (§4.4) and R1 never
            // skips them: no plan, the sequential call directly, as before C1
            OpKind::Row { .. } | OpKind::Cols { .. } => {
                if let Err(e) = apply_seq(&mut self.shadow, input) {
                    if self.shadow.check_poisoned().is_err() {
                        return Err(JournalError::Internal { seq, source: e });
                    }
                    return Err(JournalError::Op(e));
                }
                let op = self.op_of(input).map_err(|source| JournalError::Internal { seq, source })?;
                for key in self.keys_of(&op) {
                    self.keys.entry(key).or_default().push(seq);
                }
                Ok(Accepted {
                    op,
                    targets: Vec::new(),
                    snapshot_failed: false,
                    applied: true,
                })
            }
            OpKind::Cell {
                at,
                call: CellCall::DynamicFormula(text),
            } => self.accept_dynamic(input, *at, text, seq),
            OpKind::Cell { at, call } => {
                let was_active = self.shadow.registries.strings.is_some();
                let (op, extra) = match plan_call(&mut self.shadow, *at, call) {
                    Ok(p) => p,
                    Err(e) => return Err(self.plan_err(was_active, e)),
                };
                self.accept_headers(input, vec![(input.sheet, *at, op, extra)], was_active, seq)
            }
            OpKind::WriteCells(cells) => {
                // `check_editable` before prep, as `write_cells` does
                if let Err(e) = self.shadow.check_editable() {
                    return Err(JournalError::Op(e));
                }
                let was_active = self.shadow.registries.strings.is_some();
                let mut prepared = Vec::with_capacity(cells.len());
                for (at, v) in cells {
                    match self.shadow.plan_write_value(*at, cell_write(v)) {
                        Ok((op, extra)) => prepared.push((input.sheet, *at, op, extra)),
                        Err(e) => return Err(self.plan_err(was_active, e)),
                    }
                }
                self.accept_headers(input, prepared, was_active, seq)
            }
        }
    }

    /// A prep or plan refusal: forget a fresh string activation, as the
    /// sequential setters do, and return the refusal. Nothing else changed.
    fn plan_err(&mut self, was_active: bool, e: Error) -> JournalError {
        let r: Result<()> = Err(e);
        self.shadow.forget_activation_on_error(was_active, &r);
        match r {
            Err(e) => JournalError::Op(e),
            Ok(()) => unreachable!("just constructed as Err"),
        }
    }

    /// Plans prepared writes with header staging, then R1 decides.
    fn accept_headers(
        &mut self,
        input: &OpInput,
        prepared: Vec<(usize, CellIndex, CellOp, Extra)>,
        was_active: bool,
        seq: u64,
    ) -> std::result::Result<Accepted, JournalError> {
        let (staged, plan) = match self.shadow.plan_with_headers(prepared) {
            Ok(p) => p,
            Err(e) => return Err(self.plan_err(was_active, e)),
        };
        self.accept_decided(input, Staged::Headers(staged), plan, was_active, seq)
    }

    /// Plans a dynamic write (metadata work plus cells), then R1 decides
    /// (a dynamic write never reports same, so it always applies).
    fn accept_dynamic(
        &mut self,
        input: &OpInput,
        at: CellIndex,
        text: &str,
        seq: u64,
    ) -> std::result::Result<Accepted, JournalError> {
        // `cell_mut`'s range check first, as sequentially
        if let Err(e) = CellIndex::new(at.row, at.col) {
            return Err(JournalError::Op(e));
        }
        let was_active = self.shadow.registries.strings.is_some();
        let (action, plan) = match self.shadow.plan_dynamic_formula(input.sheet, at, text) {
            Ok(p) => p,
            Err(e) => return Err(JournalError::Op(e)),
        };
        self.accept_decided(input, Staged::Dynamic(action), plan, was_active, seq)
    }

    /// R1's test over a plan's report, then skip or apply. Between the R1
    /// test and the apply nothing mutates — only snapshot reads — so the
    /// plan's positions stay valid. Apply failures keep today's meaning
    /// (bug → poison → `Internal`); apply never re-plans and never
    /// re-validates.
    fn accept_decided(
        &mut self,
        input: &OpInput,
        staged: Staged,
        plan: CellWritePlan,
        was_active: bool,
        seq: u64,
    ) -> std::result::Result<Accepted, JournalError> {
        let report = plan
            .report(&mut self.shadow)
            .map_err(|source| JournalError::Internal { seq, source })?;
        if !report.is_empty()
            && staged.is_empty()
            && report
                .iter()
                .all(|t| t.exists && !t.group_or_array && t.state_same && t.node_same && t.validation_same)
        {
            // R1 skip (a call is skipped only whole, §7.1): no snapshots,
            // no apply, no R2. Keys are pushed (echo needs them); a fresh
            // string activation is forgotten, so the skipped op is invisible
            // even white-box. This is `forget_activation_on_error`'s Err
            // branch inlined: the plan and the report only read, so the
            // shadow cannot have poisoned here and its guard is vacuous.
            if !was_active {
                self.shadow.registries.strings = None;
            }
            let op = self.op_of(input).map_err(|source| JournalError::Internal { seq, source })?;
            for key in self.keys_of(&op) {
                self.keys.entry(key).or_default().push(seq);
            }
            return Ok(Accepted {
                op,
                targets: Vec::new(),
                snapshot_failed: false,
                applied: false,
            });
        }
        let targets: Vec<Target> = report.iter().map(|t| Target { at: t.at, named: t.named }).collect();
        // snapshots of the targets not yet tracked (kept local until the op is accepted)
        let mut fresh: Vec<(CellKey, Tracked)> = Vec::new();
        let mut snapshot_error = None;
        for t in &targets {
            let key = self.cell_key(input.sheet, t.at);
            if self.tracked.contains_key(&key) || fresh.iter().any(|(k, _)| *k == key) {
                continue;
            }
            let snap = match self.shadow.snapshot_cell(input.sheet, t.at) {
                Ok(s) => s,
                Err(e) => {
                    snapshot_error = Some(e);
                    fresh.clear();
                    break;
                }
            };
            match state_of(&mut self.shadow, input.sheet, t.at, snap.node()) {
                Ok(state) => fresh.push((
                    key,
                    Tracked {
                        snap,
                        state,
                        differs: false,
                    },
                )),
                Err(e) => {
                    snapshot_error = Some(e);
                    fresh.clear();
                    break;
                }
            }
        }
        // A sheet the journal cannot index can still take an op that changes
        // nothing (a no-op `remove_formula` never reads the row index). The
        // part's serialization before the op tells, afterwards, whether it
        // did change anything; the dirty flags would not (a write that
        // restores a value, or a no-op that marks, leaves them set).
        let part_before = snapshot_error
            .as_ref()
            .map(|_| self.sheet_part_text(input.sheet));
        let applied = match staged {
            Staged::Headers(staged) => self.shadow.apply_with_headers(staged, plan),
            Staged::Dynamic(action) => {
                let registry = match action.into_registry(&self.shadow.pkg, &self.shadow.workbook_part) {
                    Ok(registry) => registry,
                    Err(e) => {
                        drop(fresh);
                        return Err(JournalError::Op(e));
                    }
                };
                self.shadow.apply_writes_with(plan, registry)
            }
        };
        if let Err(e) = applied {
            drop(fresh);
            if self.shadow.check_poisoned().is_err() {
                return Err(JournalError::Internal { seq, source: e });
            }
            return Err(JournalError::Op(e));
        }
        let snapshot_failed = snapshot_error.is_some();
        if let (Some(source), Some(before)) = (snapshot_error, part_before) {
            // The op was accepted although the snapshot could not be taken.
            // If it changed the part, the edit located a cell the snapshot
            // could not: an inconsistency. If not, it was a no-op: log it as
            // usual and track nothing (R2 skips untracked cells).
            if !part_unchanged(&before, &self.sheet_part_text(input.sheet)) {
                return Err(JournalError::Internal { seq, source });
            }
        }
        #[cfg(test)]
        if std::mem::take(&mut self.drop_fresh_once) {
            fresh.clear();
        }
        self.tracked.extend(fresh);
        let op = self.op_of(input).map_err(|source| JournalError::Internal { seq, source })?;
        for key in self.keys_of(&op) {
            self.keys.entry(key).or_default().push(seq);
        }
        Ok(Accepted {
            op,
            targets,
            snapshot_failed,
            applied: true,
        })
    }

    /// R2 on every cell the op touched.
    fn r2(
        &mut self,
        input: &OpInput,
        targets: &[Target],
        snapshot_failed: bool,
        seq: u64,
    ) -> std::result::Result<(), JournalError> {
        let content_written = match &input.kind {
            OpKind::Cell { call, .. } => writes_content(call),
            _ => true,
        };
        for t in targets {
            let key = self.cell_key(input.sheet, t.at);
            if !self.tracked.contains_key(&key) {
                debug_assert!(
                    snapshot_failed,
                    "an untracked target without a snapshot error"
                );
                // a no-op on a sheet that cannot be indexed: nothing changed
                continue;
            }
            self.write_back(input.sheet, t.at, key, seq, content_written)?;
        }
        Ok(())
    }

    /// The keys an accepted op writes (§4.2, §4.3), in the order of its
    /// targets: one per named cell and field, one per row property, one per
    /// column of a range and property. `append` and replay both use it.
    fn keys_of(&self, op: &Op) -> Vec<Key> {
        let mut out = Vec::new();
        match op {
            Op::Cell {
                sheet,
                row,
                col,
                call,
            } => {
                for &field in fields(call) {
                    out.push(Key::Cell {
                        sheet: *sheet,
                        row: *row,
                        col: *col,
                        field,
                    });
                }
            }
            Op::WriteCells { sheet, cells } => {
                let mut seen: Vec<(RowId, ColId)> = Vec::new();
                for (row, col, _) in cells {
                    if seen.contains(&(*row, *col)) {
                        continue;
                    }
                    seen.push((*row, *col));
                    out.push(Key::Cell {
                        sheet: *sheet,
                        row: *row,
                        col: *col,
                        field: Field::Content,
                    });
                }
            }
            Op::Row { sheet, id, call } => out.push(Key::Row {
                sheet: *sheet,
                id: *id,
                prop: row_prop(call),
            }),
            Op::Cols {
                sheet,
                first,
                last,
                call,
            } => {
                let cols = &self.cols[sheet];
                let ends = (cols.index_of(*first), cols.index_of(*last));
                debug_assert!(
                    ends.0.is_some() && ends.1.is_some(),
                    "a column range whose ends are not in the id list"
                );
                if let (Some(a), Some(b)) = ends {
                    for i in a..=b {
                        out.push(Key::Col {
                            sheet: *sheet,
                            id: cols.id_at(i),
                            prop: col_prop(call),
                        });
                    }
                }
            }
        }
        out
    }

    /// The serialized sheet part (`None` if it cannot be read), to tell
    /// whether an op changed it.
    fn sheet_part_text(&mut self, sheet: usize) -> Option<String> {
        let part = self.shadow.sheets.get(sheet)?.part.clone()?;
        self.shadow.pkg.xml_part(&part).ok().map(|d| d.serialize())
    }

    /// R2 for one cell: back in its snapshot state gets the snapshot (and
    /// is no longer changed); only `s` differing gets a content restore.
    fn write_back(
        &mut self,
        sheet: usize,
        at: CellIndex,
        key: CellKey,
        seq: u64,
        content_written: bool,
    ) -> std::result::Result<(), JournalError> {
        let internal = |source| JournalError::Internal { seq, source };
        let cur = self
            .shadow
            .snapshot_cell(sheet, at)
            .map_err(internal)
            .and_then(|s| state_of(&mut self.shadow, sheet, at, s.node()).map_err(internal));
        let cur = cur?;
        let t = self.tracked.get_mut(&key).expect("a target is tracked");
        let r = if cur == t.state {
            t.differs = false;
            if cur.present || t.state.present {
                #[cfg(test)]
                {
                    *self
                        .stats
                        .full
                        .entry(content_kind(&t.state.content))
                        .or_default() += 1;
                }
                restore(
                    &mut self.shadow,
                    #[cfg(test)]
                    &mut self.fail_restore_once,
                    sheet,
                    at,
                    &t.snap,
                    RestoreStyle::Snapshot,
                )
            } else {
                Ok(())
            }
        } else if cur.present && t.state.present && cur.content == t.state.content {
            t.differs = true;
            if content_written {
                #[cfg(test)]
                {
                    self.stats.content += 1;
                }
                restore(
                    &mut self.shadow,
                    #[cfg(test)]
                    &mut self.fail_restore_once,
                    sheet,
                    at,
                    &t.snap,
                    RestoreStyle::Current,
                )
            } else {
                // a style-only op leaves the content as the previous R2 (or
                // the first touch) left it, which is the snapshot's: nothing
                // to restore (review of PR 159, OQ1 (b))
                Ok(())
            }
        } else {
            t.differs = true;
            Ok(())
        };
        r.map_err(internal)
    }

    /// The logged form of an op the shadow accepted (ids from the vectors).
    /// Row and column indices are read only now, after the plan accepted
    /// the op (or, for row/column ops, after the sequential call returned
    /// `Ok`): a refused out-of-range op never reaches the id vectors, and
    /// an R1 skip reads them with no apply at all.
    fn op_of(&mut self, input: &OpInput) -> Result<Op> {
        let sheet = self.sheets[input.sheet];
        Ok(match &input.kind {
            OpKind::Cell { at, call } => {
                let k = self.cell_key(input.sheet, *at);
                Op::Cell {
                    sheet,
                    row: k.row,
                    col: k.col,
                    call: call.clone(),
                }
            }
            OpKind::WriteCells(cells) => {
                let cells = cells
                    .iter()
                    .map(|(at, v): &(CellIndex, WriteValue)| {
                        let k = self.cell_key(input.sheet, *at);
                        (k.row, k.col, v.clone())
                    })
                    .collect();
                Op::WriteCells { sheet, cells }
            }
            OpKind::Row { row, call } => {
                let rows = &self.rows[&sheet];
                if *row >= rows.len() {
                    return Err(Error::Malformed("an accepted row beyond the sheet".into()));
                }
                Op::Row {
                    sheet,
                    id: rows.id_at(*row),
                    call: *call,
                }
            }
            OpKind::Cols { first, last, call } => {
                let cols = &self.cols[&sheet];
                if *first > *last || *last >= cols.len() {
                    return Err(Error::Malformed("an accepted column range out of order".into()));
                }
                Op::Cols {
                    sheet,
                    first: cols.id_at(*first),
                    last: cols.id_at(*last),
                    call: *call,
                }
            }
        })
    }
}

/// `before` and `after` are the same, readable text. `None` on either side
/// (a part that cannot be read) counts as changed (PR 159 round 3, m1).
fn part_unchanged(before: &Option<String>, after: &Option<String>) -> bool {
    matches!((before, after), (Some(a), Some(b)) if a == b)
}

fn restore(
    wb: &mut Workbook,
    #[cfg(test)] fail_once: &mut Option<bool>,
    sheet: usize,
    at: CellIndex,
    snap: &CellSnapshot,
    style: RestoreStyle,
) -> Result<()> {
    #[cfg(test)]
    if let Some(panic) = fail_once.take() {
        assert!(!panic, "injected panic in a restore");
        return Err(Error::Malformed("injected restore failure".into()));
    }
    wb.restore_cell(sheet, at, snap, style)
}

/// What `accept` hands on: the logged form, the targets, whether a
/// snapshot failed (R2 then skips the untracked cells), and R1's verdict.
struct Accepted {
    op: Op,
    targets: Vec<Target>,
    snapshot_failed: bool,
    applied: bool,
}

/// `sheet/@sheetId` of every sheet, in workbook order.
fn sheet_ids(wb: &mut Workbook) -> Result<Vec<SheetId>> {
    let part = wb.workbook_part.clone();
    let doc = wb.pkg.xml_part(&part)?;
    let sheets = doc
        .root()
        .elements()
        .find(|e| e.name().local() == "sheets")
        .ok_or_else(|| Error::Malformed(format!("{part} has no <sheets>")))?;
    let ids = sheets
        .elements()
        .filter(|e| e.name().local() == "sheet")
        .map(|e| {
            let id = e
                .attr(ooxml_core::QName::new("", "sheetId"))
                .and_then(|v| v.trim().parse().ok());
            id.map(SheetId::Base).ok_or_else(|| {
                Error::Malformed(format!("{part}: <sheet> without a numeric sheetId"))
            })
        })
        .collect::<Result<Vec<_>>>()?;
    if ids.len() != wb.sheets.len() {
        return Err(Error::Malformed(format!(
            "{part}: sheet list disagrees with the workbook"
        )));
    }
    Ok(ids)
}

#[cfg(test)]
fn content_kind(c: &state::Content) -> &'static str {
    use state::Content;
    match c {
        Content::Empty => "empty",
        Content::Value { .. } => "value",
        Content::Str { inline: false, .. } => "shared",
        Content::Str { inline: true, .. } => "inline",
        Content::Formula { .. } => "formula",
    }
}

#[cfg(test)]
impl Journal {
    pub(crate) fn stats(&self) -> &RestoreStats {
        &self.stats
    }

    pub(crate) fn changed_cells(&self) -> Vec<CellKey> {
        self.tracked
            .iter()
            .filter(|(_, t)| t.differs)
            .map(|(k, _)| *k)
            .collect()
    }

    pub(crate) fn open_snapshots(&self) -> usize {
        self.tracked.len()
    }

    pub(crate) fn next_seq(&self) -> u64 {
        self.next_seq
    }

    pub(crate) fn keys(&self) -> &HashMap<Key, Vec<u64>> {
        &self.keys
    }

    pub(crate) fn tracked_differs(&self) -> Vec<(CellKey, bool)> {
        self.tracked.iter().map(|(k, t)| (*k, t.differs)).collect()
    }

    pub(crate) fn snapshot_node(&self, k: &CellKey) -> Option<Option<&ooxml_core::RawNode>> {
        self.tracked.get(k).map(|t| t.snap.node())
    }

    pub(crate) fn shadow_mut(&mut self) -> &mut Workbook {
        &mut self.shadow
    }
}
