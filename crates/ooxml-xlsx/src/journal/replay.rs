//! Recovery by replay (ADR-0024 §3.2, §4.5, §9.3): a call that goes wrong
//! (a poisoned shadow, a failed restore, a snapshot inconsistency, a panic)
//! is undone by rebuilding the shadow from the Base and re-running the log
//! without the failing op, through the same pipeline `append` uses.
//!
//! Replay keeps every logged op's `seq` and `origin`, and the id counter is
//! never reset (§5.1). The Journal breaks only when the replay itself fails.

use std::sync::Arc;

use super::op::{Op, OpInput, OpKind};
use super::{Journal, JournalError};
use crate::error::{Error, Result};
use crate::reference::CellIndex;
use crate::workbook::{CalcPolicy, Workbook};

/// The state before a call: what a recovery truncates the log to.
#[derive(Clone, Copy, Debug)]
pub(super) struct Mark {
    pub(super) log_len: usize,
    pub(super) next_seq: u64,
}

impl Journal {
    /// A shadow over `base` with every flag `open` sets. `open` and replay
    /// both use it, so a replayed shadow cannot differ in policy.
    pub(super) fn open_shadow(base: &Arc<[u8]>) -> Result<Workbook> {
        let mut shadow = Workbook::from_bytes(base.clone())?;
        shadow.set_calc_policy(CalcPolicy::KeepStaleCaches);
        // the strip is deferred (C3's), its refusals are not: ADR §4.5
        shadow.validate_strips = true;
        // cell writes leave `dimension` to the save (ADR-0024 C5)
        shadow.freeze_dimension = true;
        Ok(shadow)
    }

    /// A logged op in today's indices. An id that is not on its axis is a
    /// dead id (J2 turns this into `StaleHandle`).
    pub(crate) fn input_of(&self, op: &Op) -> std::result::Result<OpInput, Error> {
        let dead = || Error::Malformed("dead id".into());
        let sheet_of = |s| self.sheet_index_of(s).ok_or_else(dead);
        let row_of = |sheet, id| {
            self.rows
                .get(&sheet)
                .and_then(|v| v.index_of(id))
                .ok_or_else(dead)
        };
        let col_of = |sheet, id| {
            self.cols
                .get(&sheet)
                .and_then(|v| v.index_of(id))
                .ok_or_else(dead)
        };
        Ok(match op {
            Op::Cell {
                sheet,
                row,
                col,
                call,
            } => OpInput {
                sheet: sheet_of(*sheet)?,
                kind: OpKind::Cell {
                    at: CellIndex {
                        row: row_of(*sheet, *row)?,
                        col: col_of(*sheet, *col)?,
                    },
                    call: call.clone(),
                },
            },
            Op::WriteCells { sheet, cells } => {
                let mut out = Vec::with_capacity(cells.len());
                for (row, col, v) in cells {
                    out.push((
                        CellIndex {
                            row: row_of(*sheet, *row)?,
                            col: col_of(*sheet, *col)?,
                        },
                        v.clone(),
                    ));
                }
                OpInput {
                    sheet: sheet_of(*sheet)?,
                    kind: OpKind::WriteCells(out),
                }
            }
            Op::Row { sheet, id, call } => OpInput {
                sheet: sheet_of(*sheet)?,
                kind: OpKind::Row {
                    row: row_of(*sheet, *id)?,
                    call: *call,
                },
            },
            Op::Cols {
                sheet,
                first,
                last,
                call,
            } => OpInput {
                sheet: sheet_of(*sheet)?,
                kind: OpKind::Cols {
                    first: col_of(*sheet, *first)?,
                    last: col_of(*sheet, *last)?,
                    call: *call,
                },
            },
        })
    }

    /// Undoes the call that failed with `source`: truncates the log to
    /// `mark`, restores `next_seq` (the op is not logged and no seq is
    /// consumed), and replays what is left. Always returns `Internal`; the
    /// Journal is `broken` only if the replay failed.
    pub(super) fn recover(&mut self, mark: Mark, seq: u64, source: Error) -> JournalError {
        self.log.truncate(mark.log_len);
        self.next_seq = mark.next_seq;
        self.stale = None;
        match self.replay() {
            Ok(()) => JournalError::Internal { seq, source },
            Err(e) => {
                self.broken = true;
                JournalError::Internal { seq, source: e }
            }
        }
    }

    /// Replays the whole log onto a fresh shadow (J3's `truncate` will call
    /// it; tests and measurements do). No truncation.
    pub(crate) fn rebuild(&mut self) -> std::result::Result<(), JournalError> {
        match self.replay() {
            Ok(()) => Ok(()),
            Err(source) => {
                self.broken = true;
                Err(JournalError::Internal {
                    seq: self.log.last().map_or(0, |o| o.seq),
                    source,
                })
            }
        }
    }

    /// The replay: a fresh shadow, empty `tracked` and `keys`, the same id
    /// vectors, then every logged op through `accept` and `r2` under its own
    /// `seq`. On `Err` the Journal's state is partial and the caller breaks it.
    ///
    /// The Journal is `broken` for the whole replay and only a replay that
    /// returns `Ok` clears it again (§3.2, §9.3): a panic part-way leaves the
    /// half-rebuilt state refused (`Broken`), never accepted.
    fn replay(&mut self) -> Result<()> {
        let was_broken = std::mem::replace(&mut self.broken, true);
        let r = self.replay_inner();
        if r.is_ok() {
            self.broken = was_broken;
        }
        r
    }

    fn replay_inner(&mut self) -> Result<()> {
        #[cfg(test)]
        let stats = self.stats.clone();
        let fresh = Journal::open_shadow(&self.base)?;
        self.shadow = fresh;
        self.tracked.clear();
        self.keys.clear();
        #[cfg(all(test, feature = "fault-injection"))]
        {
            self.shadow.fault = self.replay_fault.take();
        }
        let r = self.replay_log();
        #[cfg(test)]
        {
            self.stats = stats;
        }
        r
    }

    fn replay_log(&mut self) -> Result<()> {
        let as_error = |e: JournalError| match e {
            JournalError::Op(e) | JournalError::Internal { source: e, .. } => e,
            other => Error::Malformed(format!("replay: {other:?}")),
        };
        for i in 0..self.log.len() {
            let logged = self.log[i].clone();
            let input = self.input_of(&logged.op)?;
            let acc = self.accept(&input, logged.seq).map_err(as_error)?;
            debug_assert_eq!(
                format!("{:?}", acc.op),
                format!("{:?}", logged.op),
                "a replayed op is logged as it was"
            );
            #[cfg(not(test))]
            drop(acc.op);
            self.r2(&input, &acc.targets, acc.snapshot_failed, logged.seq)
                .map_err(as_error)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "fault-injection")]
    use std::panic::{AssertUnwindSafe, catch_unwind};

    use super::*;
    use crate::journal::state::{CellState, Tree};
    use crate::journal::CellKey;
    use crate::journal::op::{Api, CellCall, ColCall, RowCall, WriteValue};
    use crate::journal::tests::fixtures::{group_workbook, large_workbook, oracle_workbook};
    use crate::journal::tests::{
        bold, blue, c1, formula, index_of_key, italic, num, red, remove_formula, write_cells,
    };
    use crate::journal::{part_unchanged, Journal, JournalError};
    #[cfg(feature = "fault-injection")]
    use crate::workbook::ApplyStep;

    fn oracle() -> Vec<u8> {
        oracle_workbook("A1:C5")
    }

    fn group() -> Vec<u8> {
        group_workbook("A1:H6")
    }

    fn row(row: u32, call: RowCall) -> OpInput {
        OpInput {
            sheet: 0,
            kind: OpKind::Row { row, call },
        }
    }

    fn cols(first: u32, last: u32, call: ColCall) -> OpInput {
        OpInput {
            sheet: 0,
            kind: OpKind::Cols { first, last, call },
        }
    }

    /// Appends every op; the accepted ones, in order.
    fn feed(j: &mut Journal, ops: &[OpInput]) -> Vec<OpInput> {
        let mut ok = Vec::new();
        for op in ops {
            if j.append(Api::Native, op.clone()).is_ok() {
                ok.push(op.clone());
            }
        }
        ok
    }

    /// A journal that never failed, fed the same accepted inputs.
    fn twin(base: &[u8], ops: &[OpInput]) -> Journal {
        let mut t = Journal::open(base.to_vec()).unwrap();
        for op in ops {
            t.append(Api::Native, op.clone())
                .unwrap_or_else(|e| panic!("{op:?}: {e:?}"));
        }
        t
    }

    /// What replay must reproduce for a cell: the census entry, `vm`
    /// membership, the row index entry, and the dependents of its inputs.
    fn derived(j: &mut Journal, k: &CellKey) -> String {
        let (sheet, at) = index_of_key(j, k);
        let wb = j.shadow_mut();
        let part = wb.sheets[sheet].part.clone().unwrap();
        let fcell = wb
            .census
            .at(sheet, at)
            .and_then(|f| wb.census.get(f))
            .map(|c| format!("{c:?}"));
        let vm = wb.census.vm_cells.contains(&(sheet, at));
        let located = wb
            .indexes
            .get(&sheet)
            .map(|i| format!("{:?}", i.locate(&part, at)));
        let mut cells: Vec<String> = wb
            .compute_dependents(sheet, at)
            .cells
            .iter()
            .map(|(s, c)| format!("{s}!{c}"))
            .collect();
        cells.sort();
        format!("{fcell:?} {vm} {located:?} {cells:?}")
    }

    /// White-box equality of a (recovered or rebuilt) Journal with its twin.
    fn same(live: &mut Journal, twin: &mut Journal) {
        let log = |j: &Journal| {
            j.log()
                .iter()
                .map(|o| format!("{} {:?} {:?}", o.seq, o.origin, o.op))
                .collect::<Vec<_>>()
        };
        assert_eq!(log(live), log(twin), "log");
        assert_eq!(live.next_seq(), twin.next_seq(), "next_seq");
        assert_eq!(live.keys(), twin.keys(), "keys");
        let tracked = |j: &Journal| -> Vec<(CellKey, bool, Option<Tree>, CellState)> {
            j.tracked
                .iter()
                .map(|(k, t)| (*k, t.differs, t.snap.node().map(crate::journal::state::tree), t.state.clone()))
                .collect()
        };
        assert_eq!(tracked(live), tracked(twin), "tracked");
        assert!(
            live.shadow().to_bytes().unwrap() == twin.shadow().to_bytes().unwrap(),
            "shadow bytes"
        );
        assert_eq!(
            live.shadow().__index_fingerprint(),
            twin.shadow().__index_fingerprint(),
            "index fingerprint"
        );
        let keys: Vec<CellKey> = live.tracked.keys().copied().collect();
        for k in keys {
            assert_eq!(derived(live, &k), derived(twin, &k), "derived {k:?}");
        }
    }

    /// 12 valid ops on the oracle fixture: hand-offs of nothing, full and
    /// content restores, row/col, P1, dynamic. `blue` is left unused.
    fn oracle_prefix() -> Vec<OpInput> {
        vec![
            num("A1", 9.0),
            num("A1", 1.0), // full restore
            formula("C3", "1+1"),
            remove_formula("C3"),
            c1("B5", CellCall::ClearValue), // P1 on a missing cell
            row(5, RowCall::Height(20.0)),
            cols(0, 2, ColCall::Width(20.0)),
            c1("A6", CellCall::DynamicFormula("1+1".into())),
            c1("B5", CellCall::Font(italic())),
            c1("D1", CellCall::String("abc".into())),
            c1("C3", CellCall::ClearValue),
            num("C3", 7.0), // write-back after P1
        ]
    }

    fn group_prefix() -> Vec<OpInput> {
        vec![
            num("A2", 3.0),
            c1("C3", CellCall::Font(bold())),
            row(3, RowCall::Hidden(true)),
            cols(1, 1, ColCall::Outline(2)),
            c1("H1", CellCall::ClearValue),
            formula("B4", "D3+2"),
            num("G4", 1.0),
            row(3, RowCall::Hidden(false)),
            c1("E3", CellCall::Fill(red())),
            write_cells(&[("A1", WriteValue::Number(1.0)), ("B1", WriteValue::ClearValue)]),
            num("A2", 1.0),
            cols(0, 0, ColCall::Hidden(true)),
        ]
    }

    #[test]
    fn replay_rebuild_equals_live() {
        for (base, prefix) in [(oracle(), oracle_prefix()), (group(), group_prefix())] {
            let mut ops = prefix.clone();
            // 40 ops: the prefix, then a second mixed round
            ops.extend([
                c1("A2", CellCall::Number(5.0)),
                c1("A2", CellCall::RemoveFormula),
                c1("B2", CellCall::Bool(true)),
                c1("C2", CellCall::Border(crate::journal::tests::thin())),
                row(7, RowCall::Outline(2)),
                row(7, RowCall::Outline(0)),
                cols(3, 5, ColCall::Width(10.0)),
                cols(4, 4, ColCall::Hidden(true)),
                c1("D2", CellCall::Number(2.0)),
                c1("D2", CellCall::ClearValue),
                c1("C2", CellCall::StyleIndex(1)),
                c1("A1", CellCall::DynamicFormula("1+".into())), // refused
                c1("A8", CellCall::DynamicFormula("2+2".into())),
                c1("A8", CellCall::String("t".into())),
                write_cells(&[("A3", WriteValue::ClearValue), ("B3", WriteValue::Number(2.0))]),
                num("D2", 7.0),
                num("C2", 1.0),
                c1("E3", CellCall::Number(1.5)),
                formula("A9", "A1*2"),
                c1("A9", CellCall::ClearValue),
                row(0, RowCall::Height(30.0)),
                row(0, RowCall::Height(15.0)),
                c1("B1", CellCall::NumFormat("0.00".into())),
                num("B1", 4.0),
            ]);
            let mut j = Journal::open(base.clone()).unwrap();
            let ok = feed(&mut j, &ops);
            assert!(ok.len() >= 30, "{} accepted", ok.len());
            let mut t = twin(&base, &ok);
            j.rebuild().unwrap();
            same(&mut j, &mut t);
            // and the rebuilt Journal goes on like the twin
            let more = [num("A1", 2.0), row(1, RowCall::Height(18.0))];
            feed(&mut j, &more);
            feed(&mut t, &more);
            same(&mut j, &mut t);
        }
    }

    /// (fixture, prefix, step, the op that reaches it)
    #[cfg(feature = "fault-injection")]
    fn poison_cases() -> Vec<(Vec<u8>, Vec<OpInput>, ApplyStep, OpInput)> {
        vec![
            (oracle(), oracle_prefix(), ApplyStep::Target, num("A2", 5.0)),
            (
                oracle(),
                oracle_prefix(),
                ApplyStep::LaterTarget,
                write_cells(&[("A2", WriteValue::Number(1.0)), ("A3", WriteValue::Number(2.0))]),
            ),
            (
                oracle(),
                oracle_prefix(),
                ApplyStep::Registries,
                c1("A3", CellCall::Fill(blue())),
            ),
            (group(), group_prefix(), ApplyStep::HandOff, num("D2", 7.0)),
            (oracle(), oracle_prefix(), ApplyStep::Index, formula("A3", "1+1")),
            (
                oracle(),
                oracle_prefix(),
                ApplyStep::RangeFeature,
                row(8, RowCall::Hidden(true)),
            ),
            (
                oracle(),
                oracle_prefix(),
                ApplyStep::Registry,
                c1("A7", CellCall::DynamicFormula("3+3".into())),
            ),
        ]
    }

    #[test]
    #[cfg(feature = "fault-injection")]
    fn poison_recovers_without_the_op() {
        for (base, prefix, step, op) in poison_cases() {
            let mut j = Journal::open(base.clone()).unwrap();
            let ok = feed(&mut j, &prefix);
            assert_eq!(ok.len(), prefix.len(), "{step:?}: the prefix is valid");
            let n = ok.len() as u64;
            j.shadow_mut().inject_apply_failure(step);
            let r = j.append(Api::Native, op.clone());
            let Err(JournalError::Internal { seq, source }) = r else {
                panic!("{step:?}: {r:?}");
            };
            assert!(matches!(source, Error::Poisoned(_)), "{step:?}: {source:?}");
            assert_eq!(seq, n + 1, "{step:?}");
            assert!(!j.broken, "{step:?}");
            let mut t = twin(&base, &ok);
            same(&mut j, &mut t);
            // the next op is Ok with the seq the failed one would have had
            let seq = j.append(Api::Native, num("A2", 6.0)).unwrap();
            assert_eq!(seq, n + 1, "{step:?}");
            t.append(Api::Native, num("A2", 6.0)).unwrap();
            same(&mut j, &mut t);
        }
    }

    #[test]
    #[cfg(feature = "fault-injection")]
    fn panic_recovers_on_the_next_call() {
        let mut j = Journal::open(oracle()).unwrap();
        let ok = feed(&mut j, &oracle_prefix());
        j.shadow_mut().inject_apply_panic(ApplyStep::Target);
        let r = catch_unwind(AssertUnwindSafe(|| j.append(Api::Native, num("A2", 5.0))));
        assert!(r.is_err(), "the injected panic got out");
        assert!(j.stale.is_some(), "a call that panicked leaves its mark");
        let next = num("A3", 6.0);
        let seq = j.append(Api::Native, next.clone()).unwrap();
        assert_eq!(seq, ok.len() as u64 + 1);
        let mut all = ok;
        all.push(next);
        let mut t = twin(&oracle(), &all);
        same(&mut j, &mut t);
    }

    #[test]
    fn restore_failure_recovers() {
        let mut j = Journal::open(oracle()).unwrap();
        let ok = feed(&mut j, &oracle_prefix());
        // A2 is untouched so far: a change then its revert is a full restore
        assert!(j.append(Api::Native, num("A2", 9.0)).is_ok());
        let mut prefix = ok;
        prefix.push(num("A2", 9.0));
        j.fail_restore_once = Some(false);
        let r = j.append(Api::Native, num("A2", 2.0));
        let Err(JournalError::Internal { seq, .. }) = r else {
            panic!("{r:?}");
        };
        assert_eq!(seq, prefix.len() as u64 + 1);
        assert!(!j.broken);
        let mut t = twin(&oracle(), &prefix);
        same(&mut j, &mut t);
        j.append(Api::Native, num("A2", 2.0)).unwrap();
        t.append(Api::Native, num("A2", 2.0)).unwrap();
        same(&mut j, &mut t);
    }

    /// A panic outside the workbook's own apply leaves the shadow unpoisoned:
    /// only the stale mark tells the next call to recover.
    #[test]
    fn a_panic_outside_the_workbook_recovers_on_the_next_call() {
        use std::panic::{AssertUnwindSafe, catch_unwind};
        let mut j = Journal::open(oracle()).unwrap();
        let mut ok = feed(&mut j, &oracle_prefix());
        ok.extend(feed(&mut j, &[num("A2", 9.0)]));
        j.fail_restore_once = Some(true);
        let r = catch_unwind(AssertUnwindSafe(|| j.append(Api::Native, num("A2", 2.0))));
        assert!(r.is_err());
        assert!(j.shadow().check_poisoned().is_ok(), "not poisoned");
        assert!(j.stale.is_some());
        let next = c1("A3", CellCall::Number(4.0));
        let seq = j.append(Api::Native, next.clone()).unwrap();
        assert_eq!(seq, ok.len() as u64 + 1);
        ok.push(next);
        let mut t = twin(&oracle(), &ok);
        same(&mut j, &mut t);
    }

    #[test]
    #[cfg(feature = "fault-injection")]
    fn replay_failure_breaks() {
        let mut j = Journal::open(oracle()).unwrap();
        feed(&mut j, &[num("A1", 9.0), num("A2", 1.0)]);
        // the first replayed op fails, so the recovery fails
        j.replay_fault = Some((ApplyStep::Target, false));
        j.shadow_mut().inject_apply_failure(ApplyStep::Target);
        let r = j.append(Api::Native, num("A3", 4.0));
        assert!(matches!(r, Err(JournalError::Internal { .. })), "{r:?}");
        assert!(j.broken);
        let r = j.append(Api::Native, num("A3", 4.0));
        assert!(matches!(r, Err(JournalError::Broken)), "{r:?}");
    }

    /// M1 (round 1): a panic inside a recovery's replay must not leave a
    /// half-rebuilt Journal that accepts calls. ADR 0024 §3.2/§9.3: a replay
    /// that does not return `Ok` leaves the Journal broken.
    #[test]
    #[cfg(feature = "fault-injection")]
    fn a_panic_during_replay_leaves_the_journal_broken() {
        let mut j = Journal::open(oracle()).unwrap();
        feed(&mut j, &[num("A1", 9.0), num("A1", 1.0), num("B1", 5.0)]);
        j.fail_restore_once = Some(true);
        j.shadow_mut().inject_apply_failure(ApplyStep::Target);
        // the apply fails, recover replays, and the replay's restore panics
        let r = catch_unwind(AssertUnwindSafe(|| j.append(Api::Native, num("A2", 5.0))));
        assert!(r.is_err(), "the panic got out of the replay");
        assert!(j.broken, "a replay that did not finish keeps the mark");
        let r = j.append(Api::Native, c1("C1", CellCall::Number(2.0)));
        assert!(matches!(r, Err(JournalError::Broken)), "{r:?}");
        // an explicit rebuild succeeds and equals the twin
        j.rebuild().unwrap();
        let mut t = twin(
            &oracle(),
            &[num("A1", 9.0), num("A1", 1.0), num("B1", 5.0)],
        );
        same(&mut j, &mut t);
    }

    /// m1 (round 1): the failing op touches a new cell (C3, never touched
    /// before: writing its own value is a no-op write-back) and then fails;
    /// replay must clear `tracked`, or C3 stays tracked after the recovery.
    #[test]
    fn restore_failure_after_a_new_cell_recovers() {
        let mut j = Journal::open(oracle()).unwrap();
        let ok = feed(&mut j, &[num("A1", 9.0), num("A1", 1.0), num("B1", 5.0)]);
        j.fail_restore_once = Some(false);
        let r = j.append(Api::Native, num("C3", 7.0));
        assert!(matches!(r, Err(JournalError::Internal { .. })), "{r:?}");
        assert!(!j.broken);
        let mut t = twin(&oracle(), &ok);
        same(&mut j, &mut t);
    }

    #[test]
    fn m1_none_is_changed() {
        let s = |t: &str| Some(t.to_owned());
        assert!(part_unchanged(&s("a"), &s("a")));
        assert!(!part_unchanged(&s("a"), &s("b")));
        assert!(!part_unchanged(&None, &s("a")));
        assert!(!part_unchanged(&s("a"), &None));
        assert!(!part_unchanged(&None, &None), "None is never 'the same'");
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "an untracked target without a snapshot error")]
    fn n1_untracked_target_asserts() {
        let mut j = Journal::open(oracle()).unwrap();
        j.drop_fresh_once = true;
        let _ = j.append(Api::Native, num("A1", 1.0));
    }

    /// The input of a logged op is the op the caller gave (ids round trip).
    #[test]
    fn input_of_round_trips() {
        let mut j = Journal::open(oracle()).unwrap();
        let ok = feed(&mut j, &oracle_prefix());
        for (lo, input) in j.log().to_vec().iter().zip(&ok) {
            let back = j.input_of(&lo.op).unwrap();
            assert_eq!(format!("{back:?}"), format!("{input:?}"));
        }
    }

    /// Replay cost: 1,000 and 2,000 mixed ops, `rebuild()` against the total
    /// append time and `from_bytes` (oracle 10; not a gate).
    ///
    /// `cargo test -p ooxml-xlsx --release --all-features --lib journal::replay -- --ignored --nocapture`
    #[test]
    #[ignore = "a measurement, not a gate"]
    fn replay_cost() {
        use std::time::Instant;
        struct Rng(u64);
        impl Rng {
            fn next(&mut self) -> u64 {
                self.0 ^= self.0 << 13;
                self.0 ^= self.0 >> 7;
                self.0 ^= self.0 << 17;
                self.0
            }
        }
        fn ops(rows: u32, n: u32) -> Vec<OpInput> {
            let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
            (0..n)
                .map(|_| {
                    let at = CellIndex {
                        row: (rng.next() % u64::from(rows)) as u32,
                        col: (rng.next() % 10) as u32,
                    };
                    let call = match rng.next() % 4 {
                        0 | 1 => CellCall::Number((rng.next() % 1000) as f64),
                        2 => CellCall::String(format!("s{}", rng.next() % 50)),
                        _ if rng.next().is_multiple_of(2) => CellCall::Font(
                            if rng.next().is_multiple_of(2) { bold() } else { italic() },
                        ),
                        _ => CellCall::Fill(if rng.next().is_multiple_of(2) { red() } else { blue() }),
                    };
                    OpInput {
                        sheet: 0,
                        kind: OpKind::Cell { at, call },
                    }
                })
                .collect()
        }
        println!(
            "profile: {}",
            if cfg!(debug_assertions) { "debug" } else { "release" }
        );
        for rows in [10_000u32, 100_000] {
            let base = large_workbook(rows);
            let from_bytes = {
                let t = Instant::now();
                drop(Workbook::from_bytes(base.clone()).unwrap());
                t.elapsed().as_secs_f64() * 1e3
            };
            for n in [1_000u32, 2_000] {
                let all = ops(rows, n);
                let mut j = Journal::open(base.clone()).unwrap();
                let t = Instant::now();
                for op in &all {
                    let _ = j.append(Api::Native, op.clone());
                }
                let append_ms = t.elapsed().as_secs_f64() * 1e3;
                let mut runs: Vec<f64> = (0..3)
                    .map(|_| {
                        let t = Instant::now();
                        j.rebuild().unwrap();
                        t.elapsed().as_secs_f64() * 1e3
                    })
                    .collect();
                runs.sort_by(f64::total_cmp);
                println!(
                    "rows {rows:>7}, {n} ops ({} logged): rebuild median {:.1} ms, appends {append_ms:.1} ms, from_bytes {from_bytes:.1} ms",
                    j.log().len(),
                    runs[1]
                );
            }
        }
    }
}
