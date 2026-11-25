//! ADR-0024 Test oracle 1, reduced to keyed ops (J0): sequential application
//! ≡ normalize then compile, under P and its predicates; plus its fixed
//! cases that keyed ops can express, and `normalize` idempotence.

mod common;

use common::*;
use ooxml_xlsx::Format;
use proptest::prelude::*;
use xlsx_edit_journal_spike::fixture::oracle_workbook;
use xlsx_edit_journal_spike::normalize::{EKind, normalize};
use xlsx_edit_journal_spike::op::{LoggedOp, Origin};
use xlsx_edit_journal_spike::{CellKey, Content, Mode, Op, Patch};

// ------------------------------------------------------------ generator

fn cell() -> impl Strategy<Value = CellKey> {
    prop_oneof![
        8 => (0u32..7, 0u32..5).prop_map(|(r, c)| CellKey::new(0, r, c)),
        // a few hot cells, so keys repeat (folding, chains)
        6 => prop::sample::select(vec![k("B5"), k("C3"), k("C1"), k("A3"), k("D7"), k("B6")]),
        1 => (0u32..2, 0u32..2).prop_map(|(r, c)| CellKey::new(1, r, c)),
    ]
}

fn content_s() -> impl Strategy<Value = Content> {
    prop_oneof![
        2 => prop::sample::select(vec![1.0, 2.0, 3.0, 7.0, 5.0, 0.5]).prop_map(Content::Number),
        1 => prop::sample::select(vec!["x", "m", "y"]).prop_map(|s| Content::String(s.into())),
        1 => any::<bool>().prop_map(Content::Bool),
        1 => Just(Content::Error("#N/A".into())),
        2 => prop::sample::select(vec!["A1*2", "=SUM(A1:A3)", "1+1", "B5*2", "SUM("]).prop_map(|s| Content::Formula(s.into())),
        3 => Just(Content::RemoveFormula),
    ]
}

fn patch_s() -> impl Strategy<Value = Patch> {
    prop_oneof![
        Just(Patch::Format(Box::new(bold()))),
        Just(Patch::Format(Box::new(Format::new().set_num_format("0.0%")))),
        prop::sample::select(vec![0u32, 1, 2, 3, 99]).prop_map(Patch::Index),
        Just(Patch::NumFmt("0.00".into())),
        Just(Patch::Font(italic())),
        Just(Patch::Fill(red())),
        Just(Patch::Border(thin())),
    ]
}

fn op_s() -> impl Strategy<Value = Op> {
    prop_oneof![
        5 => (cell(), content_s()).prop_map(|(k, c)| Op::Content(k, c)),
        5 => (cell(), patch_s()).prop_map(|(k, p)| Op::Style(k, p)),
        1 => cell().prop_map(|k| Op::DateTime(k, date())),
        1 => cell().prop_map(|k| Op::NumberWithFormat(k, 5.0, bold())),
    ]
}

fn dim_s() -> impl Strategy<Value = &'static str> {
    prop::sample::select(vec!["A1:C5", "A1:A2", "A1"])
}

proptest! {
    #![proptest_config(ProptestConfig { cases: std::env::var("EJ_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(1024), failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn sequential_equals_normalize_then_compile(dim in dim_s(), ops in prop::collection::vec(op_s(), 1..=32)) {
        oracle1(&oracle_workbook(dim), &ops);
    }

    /// `normalize(normalize(log)) = normalize(log)`.
    #[test]
    fn normalize_is_idempotent(ops in prop::collection::vec(op_s(), 1..=32)) {
        let base = oracle_workbook("A1:C5");
        let j = journaled_log(&base, &ops);
        let once = normalize(&j);
        let relog: Vec<LoggedOp> = once.iter().enumerate().map(|(i, e)| LoggedOp {
            seq: i as u64 + 1,
            origin: Origin::Base,
            start: e.start,
            op: match &e.kind {
                EKind::Content(c) => Op::Content(e.key, c.clone()),
                EKind::Patch(p) => Op::Style(e.key, p.clone()),
                EKind::TwoHalf(op) => op.clone(),
            },
        }).collect();
        let twice = normalize(&relog);
        let shape = |v: &[xlsx_edit_journal_spike::normalize::Entry]| v.iter().map(|e| (e.key, e.kind.clone())).collect::<Vec<_>>();
        prop_assert_eq!(shape(&once), shape(&twice));
    }
}

fn journaled_log(base: &[u8], ops: &[Op]) -> Vec<LoggedOp> {
    let mut j = xlsx_edit_journal_spike::Journal::open(base.to_vec()).unwrap();
    for op in ops {
        let _ = j.append(xlsx_edit_journal_spike::Api::Native, op.clone(), None);
    }
    j.log().to_vec()
}

// ---------------------------------------------------------- fixed cases

fn base() -> Vec<u8> {
    oracle_workbook("A1:C5")
}

#[test]
fn formula_then_constant_on_a_formula_cell() {
    let (s, j) = oracle1(&base(), &[content(k("C1"), Content::Formula("=1".into())), content(k("C1"), Content::Number(5.0))]);
    assert_eq!(raw_cell(&j.bytes, S1, "C1"), raw_cell(&s.bytes, S1, "C1"));
    if j.journal.mode() == Mode::Compile {
        assert_eq!(j.report.entries, 1, "the full write folded the formula away");
    }
}

#[test]
fn set_formula_then_remove_formula_on_a_base_constant_leaves_an_empty_cell() {
    let (_, j) = oracle1(&base(), &[content(k("C3"), Content::Formula("1+1".into())), content(k("C3"), Content::RemoveFormula)]);
    let c3 = raw_cell(&j.bytes, S1, "C3").unwrap();
    assert!(!c3.contains("<v>") && !c3.contains("<f>"), "{c3}");
}

#[test]
fn number_then_num_format_on_a_missing_cell_in_a_styled_column() {
    // B5 is missing under `col/@style` = 1 (bold): the content creates the
    // cell without `s`, so the patch resolves on 0 (no bold)
    let (_, j) = oracle1(&base(), &[content(k("B5"), Content::Number(5.0)), patch(k("B5"), Patch::NumFmt("0.00".into()))]);
    if j.journal.mode() == Mode::Compile {
        assert_eq!(j.report.batches, 2, "no merge on a Missing start (§7.1)");
    }
}

#[test]
fn x_then_y_and_y_then_x_on_existing_and_missing_cells() {
    for at in [k("A2"), k("B5"), k("D7"), k("B6")] {
        oracle1(&base(), &[content(at, Content::Number(9.0)), patch(at, Patch::Fill(red()))]);
        oracle1(&base(), &[patch(at, Patch::Fill(red())), content(at, Content::Number(9.0))]);
        oracle1(&base(), &[patch(at, Patch::Fill(red())), patch(at, Patch::Font(italic())), patch(at, Patch::Border(thin()))]);
    }
    // an existing cell: value then fill is one write
    let (_, j) = oracle1(&base(), &[content(k("A2"), Content::Number(9.0)), patch(k("A2"), Patch::Fill(red()))]);
    if j.journal.mode() == Mode::Compile {
        assert_eq!(j.report.batches, 1);
    }
    // three patches are three batches
    let (_, j) = oracle1(&base(), &[patch(k("A2"), Patch::Fill(red())), patch(k("A2"), Patch::Font(italic())), patch(k("A2"), Patch::Border(thin()))]);
    if j.journal.mode() == Mode::Compile {
        assert_eq!(j.report.batches, 3);
    }
}

#[test]
fn datetime_and_number_both_orders_on_missing_cells_under_a_bold_column() {
    let b1 = CellKey { row: 6, ..k("B1") };
    let b2 = CellKey { row: 7, ..k("B1") };
    oracle1(&base(), &[Op::DateTime(b1, date()), content(b1, Content::Number(5.0))]);
    oracle1(&base(), &[content(b2, Content::Number(5.0)), Op::DateTime(b2, date())]);
}

#[test]
fn style_index_zero_on_missing_b5_under_a_bold_column_is_not_a_noop() {
    let (s, j) = oracle1(&base(), &[patch(k("B5"), Patch::Index(0))]);
    assert_eq!(raw_cell(&j.bytes, S1, "B5"), raw_cell(&s.bytes, S1, "B5"));
    assert!(raw_cell(&j.bytes, S1, "B5").is_some(), "the Journal writes the cell");
    assert_eq!(j.report.skipped_noops, 0);
}

#[test]
fn writing_back_the_base_value_under_a_stale_dimension_is_skipped() {
    let base = oracle_workbook("A1:A2");
    let (s, j) = oracle1(&base, &[content(k("C3"), Content::Number(7.0))]);
    assert_eq!(dimension(&s.bytes, S1).as_deref(), Some("A1:C3"));
    assert_eq!(dimension(&j.bytes, S1).as_deref(), Some("A1:A2"));
    assert_eq!(j.bytes, base, "oracle 5: an all-no-op Journal saves the Base bytes");
}

#[test]
fn remove_formula_then_style_index_on_a_constant() {
    let (_, j) = oracle1(&base(), &[content(k("C3"), Content::RemoveFormula), patch(k("C3"), Patch::Index(1))]);
    assert!(raw_cell(&j.bytes, S1, "C3").unwrap().contains(r#"s="1""#));
}

#[test]
fn number_remove_formula_fill_on_a_formula_cell_keeps_the_fill() {
    let (_, j) = oracle1(&base(), &[content(k("C1"), Content::Number(4.0)), content(k("C1"), Content::RemoveFormula), patch(k("C1"), Patch::Fill(red()))]);
    assert!(raw_cell(&j.bytes, S1, "C1").unwrap().contains(" s="));
}

#[test]
fn remove_formula_outside_the_dimension_grows_nothing() {
    let base = oracle_workbook("A1");
    let d10 = CellKey { row: 9, col: 3, sheet: 0 };
    let (s, j) = oracle1(&base, &[content(k("C3"), Content::RemoveFormula), content(d10, Content::RemoveFormula)]);
    assert_eq!(dimension(&s.bytes, S1).as_deref(), Some("A1"));
    assert_eq!(dimension(&j.bytes, S1).as_deref(), Some("A1"));
}

#[test]
fn value_changed_and_reverted_under_a_dependent_formula() {
    let base = base();
    let (_, j) = oracle1(&base, &[content(k("A1"), Content::Number(10.0)), content(k("A1"), Content::Number(1.0))]);
    assert_eq!(j.bytes, base, "oracle 5: write then revert is byte-identical");
}

/// ADR feedback F1: a conditional op first, then a patch, then a full write,
/// on a missing cell in a formatted row. Fails with `adr_literal_placement`.
#[test]
fn remove_formula_patch_number_on_a_missing_cell_in_a_formatted_row() {
    let b6 = k("B6");
    oracle1(&base(), &[content(b6, Content::RemoveFormula), patch(b6, Patch::NumFmt("0.00".into())), content(b6, Content::Number(1.0))]);
}

#[test]
fn refusals_match() {
    // C4 is inside merge B4:C4; 99 is out of range; `SUM(` does not parse
    oracle1(&base(), &[content(k("C4"), Content::Number(1.0)), patch(k("A1"), Patch::Index(99)), content(k("A1"), Content::Formula("SUM(".into()))]);
}

#[test]
fn a_full_patch_in_the_middle_of_a_chain() {
    let at = k("A3");
    oracle1(&base(), &[patch(at, Patch::Fill(red())), patch(at, Patch::Format(Box::new(bold()))), patch(at, Patch::Font(italic())), Op::DateTime(at, date()), patch(at, Patch::NumFmt("0.00".into()))]);
}
