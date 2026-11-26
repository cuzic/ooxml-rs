//! The xlsx Edit Journal, delivery J0a (ADR-0024 §3, §4, §7.1 R2): keyed ops
//! on cells (content and style) appended to a shadow workbook, logged by id,
//! with the snapshot-and-write-back rule R2 on C2's snapshots. No save, no
//! Commit, no structural ops (J1, J2); R1 is off until C1 lands (§4.5).
//!
//! Stand-ins, until their core APIs land (decisions A2, A8, A16 of the task):
//! C1's target report is read off the census here; C8's typed cell-state
//! reads are `state.rs`'s raw reads; C3's deferred caches are the shadow's
//! `KeepStaleCaches` policy.
//!
//! Nothing here is public API.

#![cfg_attr(not(test), expect(dead_code))]
#![cfg_attr(test, allow(dead_code))]

pub(crate) mod ids;
pub(crate) mod op;
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
    fields, writes_content,
};
use state::{CellState, state_of};

use crate::census::FKind;
use crate::error::{Error, Result};
use crate::reference::{CellIndex, MAX_COLS, MAX_ROWS};
use crate::snapshot::{CellSnapshot, RestoreStyle};
use crate::workbook::{CalcPolicy, Workbook};

#[derive(Debug)]
pub(crate) enum JournalError {
    /// The sequential call refused the op (nothing changed).
    Op(Error),
    /// Part B (P1) and the session-xf case of P2.
    Unsupported { op: &'static str },
    /// The shadow is poisoned or a restore failed: the Journal is broken.
    Internal { seq: u64, source: Error },
    /// After `Internal`; no replay in J0a.
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
    base_xf_count: u32,
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
    #[cfg(test)]
    stats: RestoreStats,
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

impl Journal {
    pub(crate) fn open(base: impl Into<Arc<[u8]>>) -> Result<Journal> {
        let base: Arc<[u8]> = base.into();
        let mut shadow = Workbook::from_bytes(base.clone())?;
        shadow.set_calc_policy(CalcPolicy::KeepStaleCaches);
        // the strip is deferred (C3's), its refusals are not: ADR §4.5
        shadow.validate_strips = true;
        // cell writes leave `dimension` to the save (ADR-0024 C5)
        shadow.freeze_dimension = true;
        let base_xf_count = shadow.cell_format_count()?;
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
            base_xf_count,
            sheets,
            rows,
            cols,
            ids: IdCounter::new(),
            log: Vec::new(),
            next_seq: 1,
            keys: HashMap::new(),
            tracked: BTreeMap::new(),
            broken: false,
            #[cfg(test)]
            stats: RestoreStats::default(),
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
    /// it and runs R2 on every cell it touched. `Err` leaves everything as it was.
    pub(crate) fn append(
        &mut self,
        api: Api,
        input: OpInput,
    ) -> std::result::Result<u64, JournalError> {
        if self.broken {
            return Err(JournalError::Broken);
        }
        // P1 and P2
        match &input.kind {
            OpKind::ClearCell { .. } => return Err(JournalError::Unsupported { op: "clear" }),
            OpKind::Cell {
                call: CellCall::StyleIndex(i),
                ..
            } => {
                let now = self.shadow.cell_format_count().map_err(JournalError::Op)?;
                if self.base_xf_count <= *i && *i < now {
                    return Err(JournalError::Unsupported {
                        op: "set_style_index(session xf)",
                    });
                }
            }
            _ => {}
        }
        if let Err(e) = self.shadow.worksheet_by_index(input.sheet).map(drop) {
            return Err(JournalError::Op(e));
        }
        let targets = self.targets(&input);
        for t in &targets {
            if let Err(e) = CellIndex::new(t.at.row, t.at.col) {
                return Err(JournalError::Op(e));
            }
        }
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
            match state_of(&mut self.shadow, input.sheet, snap.node()) {
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
        let seq = self.next_seq;
        // A sheet the journal cannot index can still take an op that changes
        // nothing (a no-op `remove_formula` never reads the row index). The
        // part's serialization before the op tells, afterwards, whether it
        // did change anything; the dirty flags would not (a write that
        // restores a value, or a no-op that marks, leaves them set).
        let part_before = snapshot_error.as_ref().map(|_| self.sheet_part_text(input.sheet));
        if let Err(e) = apply_seq(&mut self.shadow, &input) {
            drop(fresh);
            if self.shadow.check_poisoned().is_err() {
                self.broken = true;
                return Err(JournalError::Internal { seq, source: e });
            }
            return Err(JournalError::Op(e));
        }
        if let Some(source) = snapshot_error {
            // The op was accepted although the snapshot could not be taken.
            // If it changed the part, the edit located a cell the snapshot
            // could not: an inconsistency. If not, it was a no-op: log it as
            // usual and track nothing (R2 skips untracked cells).
            if part_before != Some(self.sheet_part_text(input.sheet)) {
                self.broken = true;
                return Err(JournalError::Internal { seq, source });
            }
        }
        self.tracked.extend(fresh);
        self.next_seq += 1;
        let op = self.op_of(&input);
        let named: Vec<CellKey> = targets
            .iter()
            .filter(|t| t.named)
            .map(|t| self.cell_key(input.sheet, t.at))
            .collect();
        let written: &'static [Field] = match &input.kind {
            OpKind::Cell { call, .. } => fields(call),
            _ => &[Field::Content],
        };
        for k in &named {
            for &field in written {
                self.keys
                    .entry(Key::Cell {
                        sheet: k.sheet,
                        row: k.row,
                        col: k.col,
                        field,
                    })
                    .or_default()
                    .push(seq);
            }
        }
        self.log.push(LoggedOp {
            seq,
            origin: Origin::Session { seq, api },
            born: Epoch(0),
            applied: true,
            op,
        });
        // R2
        let content_written = match &input.kind {
            OpKind::Cell { call, .. } => writes_content(call),
            _ => true,
        };
        for t in &targets {
            let key = self.cell_key(input.sheet, t.at);
            if !self.tracked.contains_key(&key) {
                // a no-op on a sheet that cannot be indexed: nothing changed
                continue;
            }
            self.write_back(input.sheet, t.at, key, seq, content_written)?;
        }
        Ok(seq)
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
            .and_then(|s| state_of(&mut self.shadow, sheet, s.node()).map_err(internal));
        let cur = match cur {
            Ok(c) => c,
            Err(e) => {
                self.broken = true;
                return Err(e);
            }
        };
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
                self.shadow
                    .restore_cell(sheet, at, &t.snap, RestoreStyle::Snapshot)
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
                self.shadow
                    .restore_cell(sheet, at, &t.snap, RestoreStyle::Current)
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
        r.map_err(|source| {
            self.broken = true;
            JournalError::Internal { seq, source }
        })
    }

    /// The cells the op touches (C1's target report, read off the census): the
    /// named cells in op order, then each shared-formula master a content
    /// write may hand off, as its first member by position (`plan_handoff`).
    fn targets(&mut self, input: &OpInput) -> Vec<Target> {
        let mut own: Vec<(CellIndex, bool)> = Vec::new();
        match &input.kind {
            OpKind::Cell { at, call } => own.push((*at, writes_content(call))),
            OpKind::WriteCells(cells) => own.extend(cells.iter().map(|(at, _)| (*at, true))),
            OpKind::ClearCell { at } => own.push((*at, false)),
        }
        let mut out: Vec<Target> = Vec::new();
        for &(at, _) in &own {
            if !out.iter().any(|t| t.at == at) {
                out.push(Target { at, named: true });
            }
        }
        let census = &self.shadow.census;
        for &(at, content) in &own {
            if !content {
                continue;
            }
            let Some(c) = census.at(input.sheet, at).and_then(|f| census.get(f)) else {
                continue;
            };
            let FKind::Shared { si, master: true } = c.kind else {
                continue;
            };
            if c.ambiguous_group {
                continue;
            }
            let member = census
                .alive()
                .filter(|(_, m)| {
                    m.sheet == c.sheet && m.kind == (FKind::Shared { si, master: false })
                })
                .map(|(_, m)| m.at)
                .min();
            if let Some(m) = member
                && !out.iter().any(|t| t.at == m)
            {
                out.push(Target {
                    at: m,
                    named: false,
                });
            }
        }
        out
    }

    fn op_of(&mut self, input: &OpInput) -> Op {
        let sheet = self.sheets[input.sheet];
        match &input.kind {
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
            OpKind::ClearCell { .. } => unreachable!("refused before"),
        }
    }
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
        Content::Shared(_) => "shared",
        Content::Inline(_) => "inline",
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

    /// The shadow sheet index of an id.
    pub(crate) fn sheet_index_of(&self, id: SheetId) -> Option<usize> {
        self.sheets.iter().position(|s| *s == id)
    }
}
