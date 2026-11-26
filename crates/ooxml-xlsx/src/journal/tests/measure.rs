//! Oracle 10 (the task's §6; J0b §4.4). Not a gate: it prints numbers.
//!
//! `cargo test -p ooxml-xlsx --release --lib journal::tests::measure -- --ignored --nocapture`
//! with `OOXML_J0_ROWS` (default 10,000) and `OOXML_J0_OPS` (default 2,000)
//! for `append_cost`; `insert_rows_cost` and `axis_vec_cost` take no knobs.

use std::time::Instant;

use super::fixtures::large_workbook;
use super::*;
use crate::journal::ids::{AxisVec, IdCounter};
use crate::reference::MAX_ROWS;
use crate::workbook::CalcPolicy;

fn env(name: &str, default: u32) -> u32 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// A fixed-seed generator (xorshift).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

/// Random cells of `A`..`J`: half numbers, a quarter strings, a quarter styles.
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
                _ if rng.next().is_multiple_of(2) => {
                    CellCall::Font(if rng.next().is_multiple_of(2) {
                        bold()
                    } else {
                        italic()
                    })
                }
                _ => CellCall::Fill(if rng.next().is_multiple_of(2) {
                    red()
                } else {
                    blue()
                }),
            };
            OpInput {
                sheet: 0,
                kind: OpKind::Cell { at, call },
            }
        })
        .collect()
}

/// Each op followed by the write that puts the Base value back (numbers only).
fn write_back_ops(rows: u32, n: u32) -> Vec<OpInput> {
    let mut rng = Rng(0x2545_F491_4F6C_DD1D);
    let mut out = Vec::new();
    for _ in 0..n / 2 {
        let at = CellIndex {
            row: (rng.next() % u64::from(rows)) as u32,
            col: (rng.next() % 10) as u32,
        };
        let base = f64::from((at.row + 1) * 10 + at.col);
        out.push(OpInput {
            sheet: 0,
            kind: OpKind::Cell {
                at,
                call: CellCall::Number(base + 1.0),
            },
        });
        out.push(OpInput {
            sheet: 0,
            kind: OpKind::Cell {
                at,
                call: CellCall::Number(base),
            },
        });
    }
    out
}

fn micros(f: impl FnOnce()) -> f64 {
    let t = Instant::now();
    f();
    t.elapsed().as_secs_f64() * 1e6
}

fn bare(base: &[u8], ops: &[OpInput], policy: CalcPolicy) -> f64 {
    let mut wb = Workbook::from_bytes(base.to_vec()).unwrap();
    wb.set_calc_policy(policy);
    micros(|| {
        for op in ops {
            let _ = super::super::op::apply_seq(&mut wb, op);
        }
    }) / ops.len() as f64
}

fn journal(base: &[u8], ops: &[OpInput]) -> (f64, usize) {
    let mut j = Journal::open(base.to_vec()).unwrap();
    let us = micros(|| {
        for op in ops {
            let _ = j.append(Api::Native, op.clone());
        }
    }) / ops.len() as f64;
    (us, j.open_snapshots())
}

#[test]
#[ignore = "a measurement, not a gate"]
fn append_cost() {
    let (rows, n) = (env("OOXML_J0_ROWS", 10_000), env("OOXML_J0_OPS", 2_000));
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    println!("profile: {profile}, rows {rows}, ops {n}");
    let base = large_workbook(rows);
    println!("base: {} KiB", base.len() / 1024);
    // 1. open
    let t_bare = micros(|| drop(Workbook::from_bytes(base.clone()).unwrap())) / 1e3;
    let t_journal = micros(|| drop(Journal::open(base.clone()).unwrap())) / 1e3;
    println!("open: Workbook::from_bytes {t_bare:.1} ms, Journal::open {t_journal:.1} ms");
    // 2. keyed append, both run orders
    let ops = ops(rows, n);
    for order in ["journal first", "bare first"] {
        let (j, snaps);
        let (keep, strip);
        if order == "journal first" {
            (j, snaps) = journal(&base, &ops);
            keep = bare(&base, &ops, CalcPolicy::KeepStaleCaches);
            strip = bare(&base, &ops, CalcPolicy::default());
        } else {
            keep = bare(&base, &ops, CalcPolicy::KeepStaleCaches);
            strip = bare(&base, &ops, CalcPolicy::default());
            (j, snaps) = journal(&base, &ops);
        }
        println!(
            "{order}: Journal {j:.1} us/op, bare KeepStaleCaches {keep:.1}, bare default {strip:.1}; open snapshots {snaps}"
        );
    }
    // by kind (a diagnostic)
    for (name, keep) in [("numbers", 0), ("strings", 1), ("styles", 2)] {
        let sub: Vec<OpInput> = ops
            .iter()
            .filter(|o| {
                let OpKind::Cell { call, .. } = &o.kind else {
                    return false;
                };
                let k = match call {
                    CellCall::Number(_) => 0,
                    CellCall::String(_) => 1,
                    _ => 2,
                };
                k == keep
            })
            .cloned()
            .collect();
        let (j, _) = journal(&base, &sub);
        let b = bare(&base, &sub, CalcPolicy::KeepStaleCaches);
        println!(
            "  {name} only ({} ops): Journal {j:.1} us/op, bare {b:.1}",
            sub.len()
        );
    }
    // 3. write-back heavy: each op followed by its revert
    let wb_ops = write_back_ops(rows, n);
    let (jw, snaps) = journal(&base, &wb_ops);
    let bw = bare(&base, &wb_ops, CalcPolicy::KeepStaleCaches);
    println!(
        "write-back: Journal {jw:.1} us/op, bare KeepStaleCaches {bw:.1} ({:.1}x); open snapshots {snaps}",
        jw / bw
    );
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}

fn profile() -> &'static str {
    if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    }
}

/// 10 x `insert_rows(0, 1)` on 100,000 rows, on a bare workbook and on one
/// opened as the shadow is.
#[test]
#[ignore = "a measurement, not a gate"]
fn insert_rows_cost() {
    println!("profile: {}, 100,000 rows x (A..J + K = A*2)", profile());
    let base = large_workbook(100_000);
    for (name, shadow) in [("bare (default policy)", false), ("shadow-like", true)] {
        let mut wb = Workbook::from_bytes(base.clone()).unwrap();
        if shadow {
            wb.set_calc_policy(CalcPolicy::KeepStaleCaches);
            wb.validate_strips = true;
        }
        let ms: Vec<f64> = (0..10)
            .map(|_| {
                micros(|| {
                    wb.worksheet_by_index(0)
                        .unwrap()
                        .insert_rows(0, 1)
                        .expect("insert_rows");
                }) / 1e3
            })
            .collect();
        let each: Vec<String> = ms.iter().map(|m| format!("{m:.1}")).collect();
        println!(
            "{name}: insert_rows(0, 1) x10 in ms: [{}]; first {:.1}, last {:.1}, median {:.1}",
            each.join(", "),
            ms[0],
            ms[9],
            median(ms.clone())
        );
    }
}

/// `AxisVec::<RowId>` costs at R runs.
#[test]
#[ignore = "a measurement, not a gate"]
fn axis_vec_cost() {
    println!("profile: {}", profile());
    let mut rng = Rng(0x1234_5678_9ABC_DEF1);
    for r in [1usize, 1_000, 10_000] {
        let mut ids = IdCounter::new();
        let mut v = AxisVec::<RowId>::base(MAX_ROWS);
        let step = (MAX_ROWS / r as u32).max(2);
        let mut at = step;
        while v.run_count() < r && at < MAX_ROWS - 1 {
            v.insert(at, 1, &mut ids);
            at += step;
        }
        let runs = v.run_count();
        let ins = micros(|| {
            for _ in 0..10 {
                v.insert(0, 1, &mut ids);
            }
        }) / 10.0;
        let del = micros(|| {
            for _ in 0..10 {
                v.delete(0, 1, &mut ids);
            }
        }) / 10.0;
        let pos: Vec<u32> = (0..10_000)
            .map(|_| (rng.next() % u64::from(MAX_ROWS)) as u32)
            .collect();
        let mut sink = 0u64;
        let id_at = micros(|| {
            for &p in &pos {
                if let RowId::Base(n) = v.id_at(p) {
                    sink += u64::from(n);
                }
            }
        }) / 10_000.0;
        let live: Vec<RowId> = pos[..1_000].iter().map(|&p| v.id_at(p)).collect();
        let index_of = micros(|| {
            for &id in &live {
                sink += u64::from(v.index_of(id).expect("a live id"));
            }
        }) / 1_000.0;
        println!(
            "R = {r} ({runs} runs): insert {ins:.2} us, delete {del:.2} us, id_at {id_at:.3} us, index_of {index_of:.2} us (sink {sink})"
        );
    }
}
