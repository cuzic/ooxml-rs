//! Revision 8 verification, item b: shared and array formulas. Oracle 1 over
//! a fixture with a shared group (master in the middle), an array formula and
//! a merge whose non-anchor cell holds a value; and the fixed cases of review
//! rounds 7–9 on them. Shadow mode only (Compile refuses `write_cells`).

mod common;

use common::*;
use proptest::prelude::*;
use xlsx_edit_journal_spike::fixture::group_workbook;
use xlsx_edit_journal_spike::{CellKey, Content, Mode, Op, Patch};

fn cases() -> u32 {
    std::env::var("EJ_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(512)
}

fn shadow_only() -> bool {
    Mode::from_env() == Mode::Shadow
}

/// Each group cell's own text (`$A{r}+{c}$1`), the array anchor's, and
/// `H1`'s.
fn own_text(at: CellKey) -> Option<String> {
    let a1 = ooxml_xlsx::CellIndex { row: at.row, col: at.col }.to_string();
    let (c, r) = a1.split_at(1);
    match (at.sheet, a1.as_str()) {
        (0, "C2" | "D2" | "E2" | "C3" | "D3" | "E3") => Some(format!("$A{r}+{c}$1")),
        (0, "G2") => Some("A2:A3*2".into()),
        (0, "H1") => Some("SUM(C2:E3)".into()),
        (0, "B4") => Some("D3+1".into()),
        (1, "A1") => Some("Sheet1!C2*2".into()),
        _ => None,
    }
}

fn cell() -> impl Strategy<Value = CellKey> {
    prop_oneof![
        6 => (0u32..6, 0u32..8).prop_map(|(r, c)| CellKey::new(0, r, c)),
        8 => prop::sample::select(vec![k("D2"), k("C2"), k("E3"), k("D3"), k("A2"), k("A3"), k("D1"), k("G2"), k("G3"), k("B6"), k("H1"), k("B4")]),
        1 => Just(k2("A1")),
    ]
}

fn content_for(at: CellKey) -> impl Strategy<Value = Content> {
    let own = own_text(at);
    prop_oneof![
        3 => prop::sample::select(vec![1.0, 2.0, 5.0, 10.0, 99.0, 11.0, 4.0]).prop_map(Content::Number),
        1 => prop::sample::select(vec!["x", "y"]).prop_map(|s| Content::String(s.into())),
        1 => Just(Content::Bool(true)),
        3 => Just(own.map_or(Content::Formula("A2*3".into()), Content::Formula)),
        1 => prop::sample::select(vec!["A2*3", "$A3+D$1", "SUM(C2:C3)"]).prop_map(|s| Content::Formula(s.into())),
        1 => Just(Content::RemoveFormula),
    ]
}

fn wc_content() -> impl Strategy<Value = Content> {
    prop_oneof![
        prop::sample::select(vec![1.0, 2.0, 99.0]).prop_map(Content::Number),
        Just(Content::String("x".into())),
        prop::sample::select(vec!["$A2+D$1", "$A3+E$1", "A2*3"]).prop_map(|s| Content::Formula(s.into())),
    ]
}

fn op_s() -> impl Strategy<Value = Op> {
    prop_oneof![
        6 => cell().prop_flat_map(|k| content_for(k).prop_map(move |c| Op::Content(k, c))),
        2 => (cell(), prop_oneof![Just(Patch::Font(italic())), Just(Patch::Fill(red())), prop::sample::select(vec![0u32, 1, 2]).prop_map(Patch::Index)]).prop_map(|(k, p)| Op::Style(k, p)),
        2 => prop::collection::btree_set(prop::sample::select(vec![k("D2"), k("E3"), k("C2"), k("A2"), k("D1"), k("F5"), k("D3")]), 1..=3)
            .prop_flat_map(|ks| prop::collection::vec(wc_content(), ks.len()).prop_map(move |cs| Op::WriteCells(ks.iter().copied().zip(cs).collect()))),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig { cases: cases(), failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn oracle1_with_shared_and_array_formulas(dim in prop::sample::select(vec!["A1:H6", "A1:B2"]), ops in prop::collection::vec(op_s(), 1..=16)) {
        if shadow_only() {
            oracle1(&group_workbook(dim), &ops);
        }
    }
}

fn base() -> Vec<u8> {
    group_workbook("A1:H6")
}

fn num(at: &str, v: f64) -> Op {
    content(k(at), Content::Number(v))
}

fn no_cache(bytes: &[u8], a1: &str) {
    let c = raw_cell(bytes, S1, a1).unwrap();
    assert!(!c.contains("<v>"), "{a1} keeps a cache: {c}");
}

/// Review round 9, M2: `set_number(D2, 1)` hands the group off to `C2`
/// (`=$A2+C$1` with the Base cache 11), then `set_number(A2, 99)`; and the
/// reverse order. No cache for `C2` on either side. Fails with
/// `mutant_r3_subtract_content` (the round-7 wording of R3).
#[test]
fn m2_round9_handoff_then_input_both_orders() {
    if !shadow_only() {
        return;
    }
    for ops in [vec![num("D2", 1.0), num("A2", 99.0)], vec![num("A2", 99.0), num("D2", 1.0)]] {
        let (s, j) = oracle1(&base(), &ops);
        no_cache(&s.bytes, "C2");
        no_cache(&j.bytes, "C2");
        assert!(raw_cell(&j.bytes, S1, "C2").unwrap().contains(r#"ref="C2:E3""#), "C2 is the new master");
    }
}

/// Review round 8, M1: the member `E3`'s own text written back splits the
/// group on both sides; then `write_cells([D2: 1, E3: 2])` is `Ok` on both.
/// Fails with `mutant_r1_skip_group` (R1 judged by P skips the write-back,
/// the group stays whole on the shadow, and the batch is refused there).
#[test]
fn m1_round8_member_write_back_then_batch() {
    if !shadow_only() {
        return;
    }
    let ops = [content(k("E3"), Content::Formula("$A3+E$1".into())), Op::WriteCells(vec![(k("D2"), Content::Number(1.0)), (k("E3"), Content::Number(2.0))])];
    let (s, j) = oracle1(&base(), &ops);
    assert_eq!(s.refused, vec![None, None]);
    assert_eq!(j.refused, vec![None, None]);
}

/// Review round 7, M2: a shared master given a constant and then its
/// original text. Both sides hand off to `C2`; the Journal tracks `C2`
/// (a target), so oracle 4 and `dimension` cover its row. Fails (oracle 4)
/// with `mutant_no_handoff_target`.
#[test]
fn m2_round7_master_constant_then_text_back() {
    if !shadow_only() {
        return;
    }
    let (_, j) = oracle1(&group_workbook("A1:B2"), &[num("D2", 1.0), content(k("D2"), Content::Formula("$A2+D$1".into()))]);
    assert!(j.report.written.contains(&k("C2")), "the hand-off's new master is in the changed set");
    assert!(dimension(&j.bytes, S1).is_some_and(|d| d.ends_with("D2")), "dimension grows to C2/D2: {:?}", dimension(&j.bytes, S1));
}

/// F13: the same value written to a merge's non-anchor cell and to an
/// array result cell reads equal, but both writes are refused sequentially.
/// R1 off (the default until C1) gives the same refusals; the spike's old R1
/// (`r1_unvalidated`) skips them and fails (i).
#[test]
fn f13_same_value_on_merge_non_anchor_and_array_result() {
    if !shadow_only() {
        return;
    }
    let (s, _) = oracle1(&base(), &[num("B6", 5.0), num("G3", 4.0)]);
    assert!(s.refused.iter().all(Option::is_some));
}

/// Review round 8, M2 (spike analogue): a formula written to a constant and
/// back is restored with its cache; a later input change must strip it.
/// R3 must read the restored state, not the shadow's node (which lost the
/// cache). Fails with `mutant_r2_no_derived`.
#[test]
fn m2_round8_restored_formula_then_input() {
    let base = xlsx_edit_journal_spike::fixture::oracle_workbook("A1:C5");
    let (s, j) = oracle1(&base, &[num("C1", 5.0), content(k("C1"), Content::Formula("A1*2".into())), num("A1", 5.0)]);
    no_cache(&s.bytes, "C1");
    no_cache(&j.bytes, "C1");
}

/// A write to an array anchor or result is refused on both sides; a
/// write-back of the group's own texts and a style patch on a member are
/// accepted on both.
#[test]
fn group_cells_refusals_and_style() {
    if !shadow_only() {
        return;
    }
    oracle1(&base(), &[num("G2", 1.0), num("G3", 1.0), patch(k("D3"), Patch::Font(italic())), content(k("D3"), Content::Formula("$A3+D$1".into())), num("A3", 7.0)]);
}
