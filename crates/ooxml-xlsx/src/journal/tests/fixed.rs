//! Oracle 1's keyed, pre-save fixed cases (the task's §5.5). Each runs
//! the per-op assertions after every op; cache claims are J1's.

use super::fixtures::{group_workbook, oracle_workbook};
use super::*;
use crate::census::FKind;
use crate::journal::op::{Api, CellCall, OpKind, WriteValue};

fn base() -> Vec<u8> {
    oracle_workbook("A1:C5")
}

fn group() -> Vec<u8> {
    group_workbook("A1:H6")
}

#[test]
fn fixed_01_formula_then_constant_on_a_formula_cell() {
    run(&base(), [formula("C1", "1"), num("C1", 5.0)]);
}

#[test]
fn fixed_02_revert_under_a_dependent() {
    let p = run(&base(), [num("A1", 9.0), num("A1", 1.0)]);
    assert!(p.j.changed_cells().is_empty());
}

#[test]
fn fixed_03_set_then_remove_formula_on_a_constant() {
    run(&base(), [formula("C3", "1+1"), remove_formula("C3")]);
}

#[test]
fn fixed_04_number_then_num_format_on_a_missing_cell_in_a_bold_column() {
    run(
        &base(),
        [num("B5", 5.0), c1("B5", CellCall::NumFormat("0.00".into()))],
    );
}

#[test]
fn fixed_05_datetime_and_number_in_both_orders_under_a_bold_column() {
    run(
        &base(),
        [c1("B5", CellCall::DateTime(date())), num("B5", 5.0)],
    );
    run(
        &base(),
        [num("B7", 5.0), c1("B7", CellCall::DateTime(date()))],
    );
}

#[test]
fn fixed_06_style_index_zero_on_a_missing_cell_is_not_a_noop() {
    let p = run(&base(), [c1("B5", CellCall::StyleIndex(0))]);
    assert_eq!(
        p.j.changed_cells().len(),
        1,
        "the created `c` stays: present both sides, not restored"
    );
}

#[test]
fn fixed_07_remove_formula_then_style_index_on_a_constant() {
    run(
        &base(),
        [remove_formula("C3"), c1("C3", CellCall::StyleIndex(1))],
    );
}

#[test]
fn fixed_08_number_remove_formula_fill_on_a_formula_cell() {
    run(
        &base(),
        [
            num("C1", 4.0),
            remove_formula("C1"),
            c1("C1", CellCall::Fill(red())),
        ],
    );
}

#[test]
fn fixed_09_remove_formula_on_a_constant_and_on_a_missing_cell() {
    run(&base(), [remove_formula("C3"), remove_formula("D10")]);
}

#[test]
fn fixed_10_refusals_match_and_change_nothing() {
    let mut p = Pair::new(&group());
    assert!(matches!(
        p.step(num("B6", 5.0)),
        Err(Error::MergedCell { .. })
    ));
    assert!(matches!(
        p.step(num("G3", 4.0)),
        Err(Error::ArrayResultCell { .. })
    ));
    p.finish();
    assert_eq!(p.j.log().len(), 0);
}

#[test]
fn fixed_11_f1_missing_cell_in_a_formatted_row() {
    run(
        &base(),
        [
            remove_formula("B6"),
            c1("B6", CellCall::NumFormat("0.00".into())),
            num("B6", 1.0),
        ],
    );
}

#[test]
fn fixed_12_round_7_b1_number_then_font_on_the_dependent() {
    run(
        &base(),
        [num("A1", 5.0), c1("C1", CellCall::Font(italic()))],
    );
}

#[test]
fn fixed_13_round_7_m2_hand_off_then_a_formula_on_the_master() {
    let mut p = Pair::new(&group());
    p.step(num("D2", 7.0)).unwrap();
    let c2 = p.j.cell_key(0, at("C2"));
    assert!(
        p.j.changed_cells().contains(&c2),
        "the hand-off's new master is tracked and changed"
    );
    p.step(formula("D2", "$A2+D$1")).unwrap();
    p.finish();
}

#[test]
fn fixed_14_round_7_m1_a_plain_string_over_a_rich_one() {
    let mut p = Pair::new(&base());
    p.step(c1("D1", CellCall::String("abc".into()))).unwrap();
    let d1 = p.j.cell_key(0, at("D1"));
    assert!(
        p.j.changed_cells().contains(&d1),
        "the runs are dropped: not back in its state"
    );
    p.finish();
}

#[test]
fn fixed_15_round_8_m1_split_a_member_then_write_the_master_and_the_member() {
    let mut p = Pair::new(&group());
    p.step(formula("E3", "$A3+E$1")).unwrap();
    p.step(write_cells(&[
        ("D2", WriteValue::Number(1.0)),
        ("E3", WriteValue::Number(2.0)),
    ]))
    .unwrap();
    p.finish();
}

#[test]
fn fixed_16_round_8_m2_a_restored_formula_is_in_the_census_and_the_graph() {
    let mut p = Pair::new(&base());
    p.step(num("C1", 9.0)).unwrap();
    p.step(formula("C1", "A1*2")).unwrap();
    p.step(num("A1", 5.0)).unwrap();
    p.finish();
    let census = &p.j.shadow().census;
    let c = census
        .at(0, at("C1"))
        .and_then(|f| census.get(f))
        .expect("a census entry at C1");
    assert_eq!(c.kind, FKind::Normal);
    assert!(c.has_cache, "the cache came back with the restore");
    let deps = p.j.shadow_mut().dependents("Sheet1", at("A1")).unwrap();
    assert!(deps.contains(&("Sheet1".to_owned(), at("C1"))), "{deps:?}");
}

#[test]
fn fixed_17_round_9_m2_hand_off_and_an_input_in_both_orders() {
    run(&group(), [num("D2", 1.0), num("A2", 99.0)]);
    run(&group(), [num("A2", 99.0), num("D2", 1.0)]);
}

#[test]
fn fixed_18_round_9_m5_write_cells_with_a_base_value() {
    run(
        &base(),
        [write_cells(&[
            ("A1", WriteValue::Number(1.0)),
            ("A2", WriteValue::Number(7.0)),
        ])],
    );
}

#[test]
fn fixed_19_b2_an_input_under_an_array() {
    run(&group(), [num("A2", 99.0)]);
}

#[test]
fn fixed_20_p1_and_p2_are_unsupported_and_change_nothing() {
    let mut p = Pair::new(&base());
    p.step(c1("A1", CellCall::Font(italic()))).unwrap(); // creates xf 4
    let before = state_of_journal(&p.j);
    // accepted sequentially, so not part of (i)
    let r = p.j.append(Api::Native, c1("A2", CellCall::StyleIndex(4)));
    assert!(
        matches!(
            r,
            Err(JournalError::Unsupported {
                op: "set_style_index(session xf)"
            })
        ),
        "{r:?}"
    );
    assert_eq!(state_of_journal(&p.j), before);
    let r = p.j.append(
        Api::Native,
        OpInput {
            sheet: 0,
            kind: OpKind::ClearCell { at: at("A2") },
        },
    );
    assert!(
        matches!(r, Err(JournalError::Unsupported { op: "clear" })),
        "{r:?}"
    );
    assert_eq!(state_of_journal(&p.j), before);
    // a Base xf is fine
    p.step(c1("A2", CellCall::StyleIndex(3))).unwrap();
    p.finish();
}
