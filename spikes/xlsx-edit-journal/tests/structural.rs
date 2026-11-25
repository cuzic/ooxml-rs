//! Revision 8 verification, item c: structural ops (ADR-0018 rows and
//! columns) in the Journal. Oracle 1 with the full cache predicate, its
//! structural direction and the probe clause, over a two-sheet fixture with
//! whole-column, position-dependent and cross-sheet formulas; the fixed
//! cases of review rounds 7–9 that need an epoch. Shadow mode only.

mod common;

use common::*;
use proptest::prelude::*;
use xlsx_edit_journal_spike::fixture::{oracle_workbook, struct_workbook};
use xlsx_edit_journal_spike::op::Structural;
use xlsx_edit_journal_spike::{CellKey, Content, Mode, Op, Patch};

fn cases() -> u32 {
    std::env::var("EJ_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(512)
}

fn shadow_only() -> bool {
    Mode::from_env() == Mode::Shadow
}

fn a(a1: &str) -> CellKey {
    k(a1)
}
fn b(a1: &str) -> CellKey {
    k2(a1)
}

fn own_text(at: CellKey) -> Option<&'static str> {
    let a1 = ooxml_xlsx::CellIndex { row: at.row, col: at.col }.to_string();
    Some(match (at.sheet, a1.as_str()) {
        (0, "A1") => "MATCH(7,B!A:A,0)",
        (0, "B1") => "A3*2",
        (0, "A2") => "B!A5*2",
        (0, "A4") => "ROW()",
        (0, "A5") => "SUM(B!A1:A6)",
        (1, "B1") => "A1*2",
        (1, "B3") => "ROW()",
        (1, "B6") => "SUM(A1:A5)",
        _ => return None,
    })
}

fn cell() -> impl Strategy<Value = CellKey> {
    prop_oneof![
        4 => (0usize..2, 0u32..8, 0u32..4).prop_map(|(s, r, c)| CellKey::new(s, r, c)),
        4 => prop::sample::select(vec![a("A1"), a("A2"), a("A3"), a("A4"), a("B1"), b("A4"), b("A5"), b("B3"), b("B6"), b("C2"), b("C5")]),
    ]
}

fn content_for(at: CellKey) -> impl Strategy<Value = Content> {
    let own = own_text(at).map(str::to_owned);
    prop_oneof![
        3 => prop::sample::select(vec![1.0, 5.0, 7.0, 9.0, 10.0]).prop_map(Content::Number),
        1 => prop::sample::select(vec!["s", "t"]).prop_map(|s| Content::String(s.into())),
        3 => Just(Content::Formula(own.unwrap_or_else(|| "B!A2+1".into()))),
        1 => prop::sample::select(vec!["ROW()", "B!A5*2", "MATCH(7,B!A:A,0)", "SUM(A1:A3)"]).prop_map(|s| Content::Formula(s.into())),
        1 => Just(Content::RemoveFormula),
    ]
}

fn structural() -> impl Strategy<Value = Op> {
    (0usize..2, 0u8..10, 0u32..7, 1u32..3).prop_map(|(sheet, kind, at, count)| {
        Op::Structural(match kind {
            0..=3 => Structural::InsertRows { sheet, at, count },
            4..=7 => Structural::DeleteRows { sheet, at, count },
            8 => Structural::InsertCols { sheet, at: at % 4, count: 1 },
            _ => Structural::DeleteCols { sheet, at: at % 4, count: 1 },
        })
    })
}

fn op_s() -> impl Strategy<Value = Op> {
    prop_oneof![
        6 => cell().prop_flat_map(|k| content_for(k).prop_map(move |c| Op::Content(k, c))),
        2 => (cell(), prop_oneof![Just(Patch::Font(italic())), Just(Patch::Fill(red())), prop::sample::select(vec![0u32, 1]).prop_map(Patch::Index)]).prop_map(|(k, p)| Op::Style(k, p)),
        2 => structural(),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig { cases: cases(), failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn oracle1_with_structural_ops(dim in prop::sample::select(vec!["A1:C6", "A1"]), ops in prop::collection::vec(op_s(), 1..=16)) {
        if shadow_only() {
            oracle1(&struct_workbook(dim), &ops);
        }
    }

    /// The same over the oracle workbook (merges, `col/@style`, a
    /// `customFormat` row, a calcChain), with row and column ops.
    #[test]
    fn oracle1_with_structural_ops_on_the_oracle_workbook(dim in prop::sample::select(vec!["A1:C5", "A1:A2"]), ops in prop::collection::vec(prop_oneof![
        4 => (prop_oneof![(0u32..7, 0u32..5).prop_map(|(r, c)| CellKey::new(0, r, c)), Just(k2("A1"))], prop_oneof![
            prop::sample::select(vec![1.0, 7.0]).prop_map(Content::Number),
            prop::sample::select(vec!["A1*2", "SUM(A1:A3)", "1+1"]).prop_map(|s| Content::Formula(s.into())),
            Just(Content::String("x".into())),
        ]).prop_map(|(k, c)| Op::Content(k, c)),
        1 => (0u32..7, prop_oneof![Just(Patch::Fill(red())), Just(Patch::Index(1))]).prop_map(|(r, p)| Op::Style(CellKey::new(0, r, 2), p)),
        1 => (0u8..4, 0u32..7, 1u32..3).prop_map(|(kind, at, count)| Op::Structural(match kind {
            0 => Structural::InsertRows { sheet: 0, at, count },
            1 => Structural::DeleteRows { sheet: 0, at, count },
            2 => Structural::InsertCols { sheet: 0, at: at % 4, count: 1 },
            _ => Structural::DeleteCols { sheet: 0, at: at % 4, count: 1 },
        })),
    ], 1..=12)) {
        if shadow_only() {
            oracle1(&oracle_workbook(dim), &ops);
        }
    }
}

fn base() -> Vec<u8> {
    struct_workbook("A1:C6")
}

fn num(at: CellKey, v: f64) -> Op {
    Op::Content(at, Content::Number(v))
}
fn fx(at: CellKey, t: &str) -> Op {
    Op::Content(at, Content::Formula(t.into()))
}
fn ins_rows(sheet: usize, at: u32, count: u32) -> Op {
    Op::Structural(Structural::InsertRows { sheet, at, count })
}
fn del_rows(sheet: usize, at: u32, count: u32) -> Op {
    Op::Structural(Structural::DeleteRows { sheet, at, count })
}

const SA: &str = "xl/worksheets/sheet1.xml";
const SB: &str = "xl/worksheets/sheet2.xml";

fn no_cache(bytes: &[u8], part: &str, a1: &str) {
    let c = raw_cell(bytes, part, a1).unwrap();
    assert!(!c.contains("<v>"), "{a1} keeps a cache: {c}");
}

/// Review round 9, M1: `A!A1 = MATCH(7,B!A:A,0)` (4), set to 9,
/// `insert_rows(B, 2, 1)` (7 moves to row 5), the text written back. No cache
/// on either side. Fails with `mutant_close_own_sheet` only if the probe
/// clause sees the strip, which needs `main` to strip a whole-column reader
/// on a row insert (see the README for the result).
#[test]
fn m1_round9_whole_column_reader_on_another_sheet() {
    if !shadow_only() {
        return;
    }
    let (s, j) = oracle1(&base(), &[num(a("A1"), 9.0), ins_rows(1, 2, 1), fx(a("A1"), "MATCH(7,B!A:A,0)")]);
    no_cache(&s.bytes, SA, "A1");
    no_cache(&j.bytes, SA, "A1");
}

/// Review round 9, M1, second case: `A!A2 = B!A5*2` (10), set to 9,
/// `insert_rows(B, 2, 1)`, the same text written back (now the old `A4`).
/// No cache on either side; `A2` is content-changed.
#[test]
fn m1_round9_same_text_now_another_cell() {
    if !shadow_only() {
        return;
    }
    let (s, j) = oracle1(&base(), &[num(a("A2"), 9.0), ins_rows(1, 2, 1), fx(a("A2"), "B!A5*2")]);
    no_cache(&s.bytes, SA, "A2");
    no_cache(&j.bytes, SA, "A2");
    assert!(j.report.written.contains(&a("A2")));
}

/// Review round 9, M1, third case: a cell on `A` set from 5 to 9,
/// `insert_rows(B, 2, 1)`, then 10 and 9. `A!A3` keeps its carried mark
/// and its row in `dimension`. Fails with `mutant_restore_clears_mark`.
#[test]
fn m1_round9_carried_mark_survives_a_restore() {
    if !shadow_only() {
        return;
    }
    let (_, j) = oracle1(&struct_workbook("A1"), &[num(a("A3"), 9.0), ins_rows(1, 2, 1), num(a("A3"), 10.0), num(a("A3"), 9.0)]);
    assert!(j.report.written.contains(&a("A3")));
    assert_eq!(dimension(&j.bytes, SA).as_deref(), Some("A1:A3"));
}

/// Review round 8, M3: under `dimension` `A1:C5`, `set_number(F20, 6)`,
/// `insert_rows(100, 1)`, 7, 6. `F20` is in both sides' `dimension`.
#[test]
fn m3_round8_f20() {
    if !shadow_only() {
        return;
    }
    let f20 = CellKey::new(0, 19, 5);
    let (s, j) = oracle1(&oracle_workbook("A1:C5"), &[num(f20, 6.0), ins_rows(0, 100, 1), num(f20, 7.0), num(f20, 6.0)]);
    assert_eq!(dimension(&s.bytes, S1).as_deref(), Some("A1:F20"));
    assert_eq!(dimension(&j.bytes, S1).as_deref(), Some("A1:F20"));
}

/// Review round 7, M3: `insert_rows` before a row holding a formula, with
/// a calcChain: no calcChain on either side; then a style patch on the
/// moved formula cell (`B!B3 = ROW()`, now `B4`): no cache on either side.
/// Fails with `mutant_drop_records` (structural direction).
#[test]
fn m3_round7_insert_before_a_formula_then_restyle_it() {
    if !shadow_only() {
        return;
    }
    let (s, j) = oracle1(&base(), &[ins_rows(1, 0, 1), Op::Style(b("B4"), Patch::Font(italic()))]);
    for out in [&s.bytes, &j.bytes] {
        assert!(!ooxml_core::Package::from_bytes(out.clone()).unwrap().contains("xl/calcChain.xml"));
        no_cache(out, SB, "B4");
    }
}

/// Review round 7, m2: `delete_rows` over a Base `t="s"` cell, and over a
/// `t="s"` cell written earlier as a number, and over a string written in
/// the session: `sst/@count` by the Journal predicate. Fails with
/// `mutant_no_sst_removed`.
#[test]
fn m2_round7_sst_count_after_a_delete() {
    if !shadow_only() {
        return;
    }
    oracle1(&base(), &[del_rows(1, 1, 1)]);
    oracle1(&base(), &[num(b("C2"), 3.0), del_rows(1, 1, 1)]);
    oracle1(&base(), &[Op::Content(b("C5"), Content::String("t".into())), del_rows(1, 4, 1)]);
    oracle1(&base(), &[Op::Content(b("C5"), Content::String("t".into())), del_rows(1, 1, 1)]);
}

/// A write at `D10` then `delete_rows(9, 1)` under `dimension` `A1:C5`:
/// `D10`'s id is dead, so it is not in the changed set.
#[test]
fn a_write_then_its_row_deleted() {
    if !shadow_only() {
        return;
    }
    let (_, j) = oracle1(&oracle_workbook("A1:C5"), &[num(CellKey::new(0, 9, 3), 1.0), del_rows(0, 9, 1)]);
    assert_eq!(dimension(&j.bytes, S1).as_deref(), Some("A1:C5"));
}

/// ADR-0024 test oracle 1's B1 fixed case: `A1 = B5*2` then
/// `insert_rows(3, 1)` gives `B6*2`.
#[test]
fn session_formula_rewritten_by_a_later_insert() {
    if !shadow_only() {
        return;
    }
    let (_, j) = oracle1(&oracle_workbook("A1:C5"), &[fx(k("A1"), "B5*2"), ins_rows(0, 2, 1)]);
    assert!(raw_cell(&j.bytes, S1, "A1").unwrap().contains("<f>B6*2</f>"));
}

/// How often the probe refuses (those cases skip the aligned checks and the
/// probe clause; ADR-0024 oracle 1 asks for the count). Printed with
/// `--nocapture`.
#[test]
fn probe_refusal_count() {
    if !shadow_only() {
        return;
    }
    use proptest::test_runner::{Config, TestRunner};
    let mut runner = TestRunner::new(Config { cases: cases(), failure_persistence: None, ..Config::default() });
    let strat = (prop::sample::select(vec!["A1:C6", "A1"]), prop::collection::vec(op_s(), 1..=16));
    let (mut with_structural, mut skipped) = (0usize, 0usize);
    let counts = std::sync::Mutex::new((0usize, 0usize));
    runner
        .run(&strat, |(dim, ops)| {
            let base = struct_workbook(dim);
            let j = journaled(&base, &ops);
            let st: Vec<&Op> = ops.iter().zip(&j.refused).filter(|(o, r)| o.is_structural() && r.is_none()).map(|(o, _)| o).collect();
            if !st.is_empty() {
                let mut c = counts.lock().unwrap();
                c.0 += 1;
                if probe(&base, &st).is_none() {
                    c.1 += 1;
                }
            }
            Ok(())
        })
        .unwrap();
    (with_structural, skipped) = *counts.lock().unwrap();
    eprintln!("probe: {with_structural} cases with an accepted structural op, {skipped} skipped (the probe refused)");
}

// ------------------------------------------ oracles 11 and 12 across epochs

/// A noise block for the cell at `at`, read on `wb` (the sequential state
/// at the insertion point): a change, then the cell's content and `s`
/// written back. `None` for a cell that cannot be written back (missing,
/// empty, a formula with attributes).
fn noise_for(wb: &mut ooxml_xlsx::Workbook, at: CellKey, change: &Content, patch: &Patch) -> Option<Vec<Op>> {
    let ws = wb.worksheet_by_index(at.sheet).ok()?;
    let c = ws.cell(at.row, at.col).ok()??;
    let s = c.style_index().ok()?;
    if s >= 4 {
        // a session `xf` cannot be named by `set_style_index` (P2)
        return None;
    }
    let back = match (c.formula(), c.value().ok()?) {
        (Some(f), _) if c.raw().elements().find(|e| e.name().local() == "f").is_some_and(|f| f.attrs().is_empty()) => Content::Formula(f),
        (Some(_), _) => return None,
        (None, ooxml_xlsx::CellValue::Number(v)) => Content::Number(v),
        (None, ooxml_xlsx::CellValue::String(v)) if c.raw().attr(ooxml_core::QName::new("", "t")) == Some("s") => Content::String(v),
        (None, ooxml_xlsx::CellValue::Bool(v)) => Content::Bool(v),
        _ => return None,
    };
    Some(vec![Op::Content(at, change.clone()), Op::Content(at, back), Op::Style(at, patch.clone()), Op::Style(at, Patch::Index(s))])
}

proptest! {
    #![proptest_config(ProptestConfig { cases: cases(), failure_persistence: None, ..ProptestConfig::default() })]

    /// Oracle 11 across epochs: noise blocks inside epochs (on cells the log
    /// has not touched since the last structural op, ending in their state
    /// before the block) leave the output byte-identical.
    #[test]
    fn history_independence_across_epochs(
        dim in prop::sample::select(vec!["A1:C6", "A1"]),
        f in prop::collection::vec(op_s(), 1..=12),
        noise in prop::collection::vec((0usize..13, cell(), prop::sample::select(vec![Content::Number(42.0), Content::String("n".into()), Content::Formula("B!A1+5".into())]), prop_oneof![Just(Patch::Font(italic())), Just(Patch::Fill(red()))]), 0..=6),
    ) {
        if !shadow_only() {
            return Ok(());
        }
        let base = struct_workbook(dim);
        // build L, reading each block's write-back on a sequential workbook
        let mut wb = ooxml_xlsx::Workbook::from_bytes(base.clone()).unwrap();
        let mut touched: std::collections::BTreeSet<CellKey> = Default::default();
        let mut long = Vec::new();
        for i in 0..=f.len() {
            for (_, at, change, patch) in noise.iter().filter(|n| n.0 == i) {
                if touched.contains(at) {
                    continue;
                }
                if let Some(block) = noise_for(&mut wb, *at, change, patch) {
                    for op in &block {
                        xlsx_edit_journal_spike::op::apply_seq(&mut wb, op).unwrap();
                    }
                    long.extend(block);
                }
            }
            if let Some(op) = f.get(i) {
                if xlsx_edit_journal_spike::op::apply_seq(&mut wb, op).is_ok() {
                    if op.is_structural() {
                        touched.clear();
                    } else {
                        touched.extend(op.keys());
                    }
                }
                long.push(op.clone());
            }
        }
        let (js, jl) = (journaled(&base, &f), journaled(&base, &long));
        let (ms, ml) = (xlsx_edit_journal_spike::project::model(&js.bytes), xlsx_edit_journal_spike::project::model(&jl.bytes));
        prop_assert!(ms == ml, "P differs: F {:?} L {:?}", f, long);
        prop_assert!(js.bytes == jl.bytes, "bytes differ (P equal): F {:?} L {:?}", f, long);
    }

    /// Oracle 12 across epochs: the live shadow's output equals a replay's.
    #[test]
    fn replay_equality_across_epochs(dim in prop::sample::select(vec!["A1:C6", "A1"]), ops in prop::collection::vec(op_s(), 1..=16)) {
        if !shadow_only() {
            return Ok(());
        }
        let mut j = xlsx_edit_journal_spike::Journal::open(struct_workbook(dim)).unwrap();
        for op in &ops {
            let _ = j.append(xlsx_edit_journal_spike::Api::Native, op.clone(), None);
        }
        let live = j.preview().unwrap();
        j.mark_shadow_stale();
        let replayed = j.preview().unwrap();
        prop_assert!(live == replayed, "replay differs for {:?}", ops);
    }
}

/// Oracle 11's fourth kind: a block that spans a structural op (a change
/// before it, the write-back after it, at the cell's new position). The
/// outputs must be P-equal; this counts how often they are also
/// byte-identical (printed with `--nocapture`).
#[test]
fn history_across_a_structural_op_is_p_equal() {
    if !shadow_only() {
        return;
    }
    use proptest::test_runner::{Config, TestRunner};
    let mut runner = TestRunner::new(Config { cases: cases(), failure_persistence: None, ..Config::default() });
    let strat = (prop::sample::select(vec!["A1:C6", "A1"]), prop::collection::vec(op_s(), 1..=12), cell(), any::<prop::sample::Index>());
    let counts = std::sync::Mutex::new((0usize, 0usize));
    runner
        .run(&strat, |(dim, f, at, pos)| {
            let Some(si) = f.iter().position(|o| o.is_structural()).map(|p| p + pos.index(f.len() - p)).and_then(|p| f[p..].iter().position(|o| o.is_structural()).map(|q| p + q)) else { return Ok(()) };
            let base = struct_workbook(dim);
            let mut wb = ooxml_xlsx::Workbook::from_bytes(base.clone()).unwrap();
            for op in &f[..si] {
                let _ = xlsx_edit_journal_spike::op::apply_seq(&mut wb, op);
            }
            let Some(block) = noise_for(&mut wb, at, &Content::Number(42.0), &Patch::Font(italic())) else { return Ok(()) };
            // a formula's text is not its write-back across a structural op
            // (the op would have rewritten it)
            if matches!(block[1], Op::Content(_, Content::Formula(_))) {
                return Ok(());
            }
            let Op::Structural(s) = f[si] else { unreachable!() };
            let Some(moved) = s.map(at) else { return Ok(()) };
            let back: Vec<Op> = block[1..].iter().map(|o| match o {
                Op::Content(_, c) => Op::Content(moved, c.clone()),
                Op::Style(_, p) => Op::Style(moved, p.clone()),
                _ => unreachable!(),
            }).collect();
            let mut long: Vec<Op> = f[..si].to_vec();
            long.push(block[0].clone());
            long.push(f[si].clone());
            long.extend(back);
            long.extend(f[si + 1..].iter().cloned());
            let (js, jl) = (journaled(&base, &f), journaled(&base, &long));
            if js.refused.iter().any(Option::is_some) || jl.refused.iter().any(Option::is_some) {
                return Ok(());
            }
            let (ms, ml) = (xlsx_edit_journal_spike::project::model(&js.bytes), xlsx_edit_journal_spike::project::model(&jl.bytes));
            assert!(ms == ml, "P differs: F {f:?} L {long:?}");
            let mut c = counts.lock().unwrap();
            c.0 += 1;
            if js.bytes == jl.bytes {
                c.1 += 1;
            }
            Ok(())
        })
        .unwrap();
    let (n, same) = *counts.lock().unwrap();
    eprintln!("blocks across a structural op: {n} cases P-equal, {same} also byte-identical");
}

// ------------------------------------------- oracle 14 across epochs (d)

fn grid2() -> Vec<CellKey> {
    (0..2).flat_map(|s| (0..9).flat_map(move |r| (0..5).map(move |c| CellKey::new(s, r, c)))).collect()
}

fn compare_truncations(base: &[u8], ops: &[Op], t: u64) -> Result<bool, String> {
    let mut js = [xlsx_edit_journal_spike::Journal::open(base.to_vec()).unwrap(), xlsx_edit_journal_spike::Journal::open(base.to_vec()).unwrap()];
    for j in &mut js {
        for op in ops {
            let _ = j.append(xlsx_edit_journal_spike::Api::Native, op.clone(), None);
        }
    }
    js[0].truncate(t).unwrap();
    let fast = js[1].truncate_fast(t).unwrap();
    let [a, b] = &mut js;
    for k in grid2() {
        let (x, y) = (a.normalized(k).unwrap(), b.normalized(k).unwrap());
        if x != y {
            return Err(format!("read of {k:?}: replay {x:?}, fast {y:?}"));
        }
    }
    if a.preview().unwrap() != b.preview().unwrap() {
        return Err("bytes differ".into());
    }
    Ok(fast)
}

/// Review round 7, M4: `A1 = B5*2`, `insert_rows(2, 1)` kept, a write to
/// `A1` cut: the fast path gives `B6*2`, as replay does. Fails with
/// `mutant_tf_from_mark`.
#[test]
fn m4_round7_fast_truncate_after_a_kept_insert() {
    if !shadow_only() {
        return;
    }
    let base = oracle_workbook("A1:C5");
    let ops = [fx(k("A1"), "B5*2"), ins_rows(0, 2, 1), num(k("A1"), 9.0)];
    assert_eq!(compare_truncations(&base, &ops, 2), Ok(true), "the fast path is taken and equals replay");
    let mut j = xlsx_edit_journal_spike::Journal::open(base).unwrap();
    for op in ops {
        j.append(xlsx_edit_journal_spike::Api::Native, op, None).unwrap();
    }
    j.truncate_fast(2).unwrap();
    assert_eq!(j.normalized(k("A1")).unwrap().unwrap().formula.as_deref(), Some("B6*2"));
}

/// Counts how often `truncate_fast` takes the fast path (printed with
/// `--nocapture`) while asserting oracle 14 at random cuts.
#[test]
fn fast_truncate_equals_replay_across_epochs() {
    if !shadow_only() {
        return;
    }
    use proptest::test_runner::{Config, TestRunner};
    let mut runner = TestRunner::new(Config { cases: cases(), failure_persistence: None, ..Config::default() });
    let strat = (prop::sample::select(vec!["A1:C6", "A1"]), prop::collection::vec(op_s(), 1..=16), any::<prop::sample::Index>());
    let counts = std::sync::Mutex::new((0usize, 0usize));
    runner
        .run(&strat, |(dim, ops, cut)| {
            let t = cut.index(ops.len() + 1) as u64;
            let fast = compare_truncations(&struct_workbook(dim), &ops, t).map_err(|e| proptest::test_runner::TestCaseError::fail(format!("{e} for {ops:?} cut at {t}")))?;
            let mut c = counts.lock().unwrap();
            c.0 += 1;
            c.1 += usize::from(fast);
            Ok(())
        })
        .unwrap();
    let (n, fast) = *counts.lock().unwrap();
    eprintln!("truncate_fast across epochs: {n} cuts, {fast} on the fast path, all equal to replay");
}
