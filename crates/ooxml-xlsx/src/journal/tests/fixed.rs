//! Oracle 1's keyed, pre-save fixed cases (the task's §5.5). Each runs
//! the per-op assertions after every op; cache claims are J1's.

use super::fixtures::{group_workbook, oracle_workbook, out_of_order_dependent_workbook};
use super::*;
use crate::census::FKind;
use crate::journal::op::{CellCall, ColCall, ColProp, Op, OpKind, RowCall, RowProp, WriteValue};
use crate::reference::MAX_ROWS;

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
fn fixed_21_m1_a_dependent_sheet_the_strip_refuses_is_refused_by_the_journal_too() {
    let base = out_of_order_dependent_workbook();
    assert!(
        Workbook::from_bytes(base.clone())
            .unwrap()
            .edit_refusal()
            .is_none()
    );
    let mut p = Pair::new(&base);
    let r = p.step(num("A1", 5.0));
    assert!(matches!(r, Err(Error::OutOfOrder(_))), "{r:?}");
    assert_eq!(p.j.log().len(), 0);
    // a style-only op on the same cell strips nothing, so both accept it
    p.step(c1("A1", CellCall::Font(italic()))).unwrap();
    p.finish();
}

#[test]
fn fixed_22_m1_bare_keep_stale_caches_still_does_not_look_at_the_strip() {
    let base = out_of_order_dependent_workbook();
    let mut wb = Workbook::from_bytes(base).unwrap();
    wb.set_calc_policy(crate::CalcPolicy::KeepStaleCaches);
    let r = crate::journal::op::apply_seq(&mut wb, &num("A1", 5.0));
    assert!(r.is_ok(), "the public policy is unchanged: {r:?}");
}

#[test]
fn fixed_23_m1r2_a_no_op_on_a_sheet_the_journal_cannot_index_is_accepted_and_not_tracked() {
    let base = out_of_order_dependent_workbook();
    let mut p = Pair::new(&base);
    // sequential: Ok (a no-op never reads the row index); the journal's
    // snapshot of the cell fails, but the part is unchanged
    p.step(cell(1, "B1", CellCall::RemoveFormula)).unwrap();
    assert_eq!(p.j.log().len(), 1);
    assert_eq!(p.j.open_snapshots(), 0);
    // not broken: later ops on both sheets still agree
    p.step(num("A1", 1.0)).ok();
    let r = p.step(cell(1, "A1", CellCall::Font(italic())));
    assert!(matches!(r, Err(Error::OutOfOrder(_))), "{r:?}");
    p.step(c1("A1", CellCall::Font(italic()))).unwrap();
    p.finish();
}

fn row_op(sheet: usize, row: u32, call: RowCall) -> OpInput {
    OpInput {
        sheet,
        kind: OpKind::Row { row, call },
    }
}

fn cols_op(sheet: usize, first: u32, last: u32, call: ColCall) -> OpInput {
    OpInput {
        sheet,
        kind: OpKind::Cols { first, last, call },
    }
}

fn clear(a1: &str) -> OpInput {
    c1(a1, CellCall::ClearValue)
}

fn dynamic(a1: &str, t: &str) -> OpInput {
    c1(a1, CellCall::DynamicFormula(t.into()))
}

fn content_seqs(j: &Journal, a1: &str) -> Vec<u64> {
    let k = j.cell_key(0, at(a1));
    j.key_seqs(&Key::Cell {
        sheet: k.sheet,
        row: k.row,
        col: k.col,
        field: crate::journal::op::Field::Content,
    })
    .to_vec()
}

fn row_seqs(j: &Journal, row: u32, prop: RowProp) -> Vec<u64> {
    let sheet = j.sheet_id(0);
    j.key_seqs(&Key::Row {
        sheet,
        id: j.rows[&sheet].id_at(row),
        prop,
    })
    .to_vec()
}

fn col_seqs(j: &Journal, col: u32, prop: ColProp) -> Vec<u64> {
    let sheet = j.sheet_id(0);
    j.key_seqs(&Key::Col {
        sheet,
        id: j.cols[&sheet].id_at(col),
        prop,
    })
    .to_vec()
}

fn node_tree(wb: &mut Workbook, a1: &str) -> Option<crate::journal::state::Tree> {
    wb.snapshot_cell(0, at(a1)).unwrap().node().map(tree)
}

fn sheet_xml(wb: &mut Workbook, sheet: usize) -> String {
    let part = wb.sheets[sheet].part.clone().unwrap();
    wb.pkg.xml_part(&part).unwrap().serialize()
}

#[test]
fn fixed_24_p1_on_a_missing_cell_under_a_bold_column() {
    let mut p = Pair::new(&base());
    p.step(clear("B5")).unwrap();
    p.finish();
    assert!(node_tree(&mut p.seq, "B5").is_none());
    assert!(node_tree(p.j.shadow_mut(), "B5").is_none());
    // C1: a conditional op that skips its cell is not a target (replaces
    // B7), but it is applied and pushes its key
    assert!(last_applied(&p));
    assert!(p.j.tracked_differs().is_empty());
    assert_eq!(p.j.open_snapshots(), 0);
    assert_eq!(content_seqs(&p.j, "B5"), vec![1]);
}

#[test]
fn fixed_25_p1_after_a_write_on_a_missing_cell_leaves_an_empty_c() {
    let mut p = Pair::new(&base());
    p.step(num("B6", 5.0)).unwrap();
    p.step(clear("B6")).unwrap();
    p.finish();
    let a = node_tree(&mut p.seq, "B6");
    let b = node_tree(p.j.shadow_mut(), "B6");
    assert!(a.is_some(), "the cell is present");
    assert_eq!(a, b, "the same node on both sides");
    println!("fixed 25 node: {a:?}");
    let b6 = p.j.cell_key(0, at("B6"));
    assert!(p.j.changed_cells().contains(&b6));
}

#[test]
fn fixed_26_p1_then_the_same_constant_is_a_full_restore() {
    let mut p = Pair::new(&base());
    p.step(clear("C3")).unwrap();
    let c3 = p.j.cell_key(0, at("C3"));
    assert!(p.j.changed_cells().contains(&c3));
    p.step(num("C3", 7.0)).unwrap();
    assert!(!p.j.changed_cells().contains(&c3));
    p.finish();
}

#[test]
fn fixed_27_p1_refusals_and_hand_off() {
    let mut p = Pair::new(&group());
    assert!(matches!(p.step(clear("B6")), Err(Error::MergedCell { .. })));
    assert!(matches!(
        p.step(clear("G3")),
        Err(Error::ArrayResultCell { .. })
    ));
    assert_eq!(p.j.log().len(), 0);
    p.step(clear("D2")).unwrap();
    let c2 = p.j.cell_key(0, at("C2"));
    assert!(p.j.changed_cells().contains(&c2), "hand-off to C2");
    p.step(write_cells(&[
        ("A1", WriteValue::ClearValue),
        ("D10", WriteValue::ClearValue),
        ("A2", WriteValue::Number(7.0)),
    ]))
    .unwrap();
    assert!(node_tree(&mut p.seq, "D10").is_none());
    assert!(node_tree(p.j.shadow_mut(), "D10").is_none());
    assert_eq!(content_seqs(&p.j, "D10"), vec![2]);
    p.finish();
}

#[test]
fn fixed_28_row_and_column_keys() {
    let mut p = Pair::new(&base());
    p.step(cols_op(0, 1, 3, ColCall::Width(20.0))).unwrap();
    for c in 1..=3 {
        assert_eq!(col_seqs(&p.j, c, ColProp::Width), vec![1], "col {c}");
    }
    assert_eq!(col_seqs(&p.j, 0, ColProp::Width), Vec::<u64>::new());
    assert_eq!(col_seqs(&p.j, 4, ColProp::Width), Vec::<u64>::new());
    assert!(p.j.keys().keys().all(|k| matches!(k, Key::Col { .. })));
    p.step(row_op(0, 6, RowCall::Height(20.0))).unwrap(); // missing row 7
    p.step(row_op(0, 6, RowCall::Hidden(true))).unwrap();
    p.step(row_op(0, 6, RowCall::Hidden(false))).unwrap();
    assert_eq!(row_seqs(&p.j, 6, RowProp::Hidden), vec![3, 4]);
    assert_eq!(row_seqs(&p.j, 6, RowProp::Height), vec![2]);
    p.step(row_op(0, 3, RowCall::Outline(2))).unwrap();
    p.step(row_op(0, 3, RowCall::Outline(0))).unwrap();
    for wb in [&mut p.seq, p.j.shadow_mut()] {
        assert!(
            sheet_xml(wb, 0).contains(r#"outlineLevelRow="2""#),
            "raised, never lowered"
        );
    }
    // a column's properties are keyed apart (round 1: a mutant keyed them all as `Width`)
    p.step(cols_op(0, 2, 2, ColCall::Hidden(true))).unwrap();
    assert_eq!(col_seqs(&p.j, 2, ColProp::Hidden), vec![7]);
    assert_eq!(col_seqs(&p.j, 2, ColProp::Width), vec![1]);
    assert_eq!(p.j.open_snapshots(), 0);
    p.finish();
}

#[test]
fn fixed_29_row_and_column_refusals() {
    let mut p = Pair::new(&base());
    assert!(p.step(cols_op(0, 0, 0, ColCall::Width(300.0))).is_err());
    assert!(p.step(row_op(0, 0, RowCall::Height(f64::NAN))).is_err());
    assert!(p.step(row_op(0, MAX_ROWS, RowCall::Height(1.0))).is_err());
    assert!(p.step(cols_op(0, 0, 0, ColCall::Outline(8))).is_err());
    assert!(p.step(cols_op(0, 5, 4, ColCall::Hidden(true))).is_err());
    assert!(p.step(cols_op(0, 0, 16_384, ColCall::Hidden(true))).is_err());
    assert_eq!(p.j.log().len(), 0);
    assert!(p.j.keys().is_empty());
    p.finish();
    let mut p = Pair::new(&out_of_order_dependent_workbook());
    let r = p.step(row_op(1, 0, RowCall::Height(20.0)));
    assert!(matches!(r, Err(Error::OutOfOrder(_))), "{r:?}");
    p.step(cols_op(1, 0, 0, ColCall::Width(20.0))).unwrap();
    p.finish();
}

#[test]
fn fixed_30_c5_the_shadow_leaves_the_dimension_alone() {
    let mut p = Pair::new(&base());
    p.step(num("Z99", 1.0)).unwrap();
    let seq = sheet_xml(&mut p.seq, 0);
    let shadow = sheet_xml(p.j.shadow_mut(), 0);
    assert!(seq.contains(r#"<dimension ref="A1:Z99""#), "{seq}");
    assert!(shadow.contains(r#"<dimension ref="A1:C5""#), "{shadow}");
    p.step(row_op(0, 0, RowCall::Height(20.0))).unwrap();
    p.step(cols_op(0, 0, 0, ColCall::Width(9.0))).unwrap();
}

#[test]
fn fixed_31_dynamic_formula() {
    let mut p = Pair::new(&base());
    p.step(dynamic("A6", "1+1")).unwrap();
    for wb in [&mut p.seq, p.j.shadow_mut()] {
        assert!(
            wb.pkg.part_names().any(|n| n.contains("metadata")),
            "a metadata part"
        );
    }
    // an array anchor cannot be unset back to missing: both sides refuse
    assert!(matches!(
        p.step(remove_formula("A6")),
        Err(Error::ArrayAnchor { .. })
    ));
    p.step(dynamic("C1", "A1*2")).unwrap();
    // on this main a plain set_formula over a dynamic anchor is refused, so
    // the ADR's "cm decides R2" case is unreachable (reported); state.rs's
    // unit test covers `cm`
    assert!(matches!(
        p.step(formula("C1", "A1*2")),
        Err(Error::ArrayAnchor { .. })
    ));
    for wb in [&mut p.seq, p.j.shadow_mut()] {
        let n = wb.snapshot_cell(0, at("C1")).unwrap();
        let cm = n.node().unwrap().attr(ooxml_core::QName::new("", "cm"));
        assert!(cm.is_some(), "C1 keeps cm");
    }
    assert!(p.step(dynamic("A7", "1+")).is_err());
    p.finish();
    let mut p = Pair::new(&group());
    // a dynamic array would replace a shared master: refused, not handed off
    assert!(matches!(
        p.step(dynamic("D2", "1+1")),
        Err(Error::SharedFormulaMaster { .. })
    ));
    assert_eq!(p.j.log().len(), 0);
    p.finish();
}

#[test]
fn fixed_32_keys_of_the_new_calls() {
    use crate::journal::op::Field;
    let mut p = Pair::new(&base());
    p.step(clear("C3")).unwrap();
    p.step(dynamic("A6", "1+1")).unwrap();
    let _ = p.step(row_op(0, MAX_ROWS, RowCall::Height(1.0)));
    let before = p.j.keys().len();
    for k in p.j.keys().keys() {
        assert!(
            matches!(k, Key::Cell { field: Field::Content, .. }),
            "{k:?}"
        );
    }
    assert_eq!(before, 2, "a refused row op pushes nothing");
}

/// P2 (ADR-0024 F12): a session `xf` is logged and applied by content, live
/// and in replay; the sequential reference takes the index directly.
#[test]
fn fixed_33_p2_session_xf_is_logged_by_content() {
    use crate::journal::op::StyleGroup;
    let mut p = Pair::new(&base());
    p.step(c1("A1", CellCall::Font(italic()))).unwrap(); // creates xf 4
    p.step(c1("A2", CellCall::StyleIndex(4))).unwrap();
    let last = p.j.log().last().unwrap();
    let Op::Cell { call, .. } = &last.op else { panic!("{last:?}") };
    assert!(matches!(call, CellCall::StyleXf(..)), "logged {call:?}, not an index");
    // all six style groups of A2 are keyed by the op, content is not
    let k = p.j.cell_key(0, at("A2"));
    let seq = last.seq;
    for g in [
        StyleGroup::NumFmt,
        StyleGroup::Font,
        StyleGroup::Fill,
        StyleGroup::Border,
        StyleGroup::Alignment,
        StyleGroup::Protection,
    ] {
        let key = Key::Cell { sheet: k.sheet, row: k.row, col: k.col, field: crate::journal::op::Field::Style(g) };
        assert_eq!(p.j.key_seqs(&key), [seq], "{g:?}");
    }
    assert!(content_seqs(&p.j, "A2").is_empty());
    // a Base xf stays an index
    p.step(c1("A3", CellCall::StyleIndex(3))).unwrap();
    let last = p.j.log().last().unwrap();
    assert!(matches!(&last.op, Op::Cell { call: CellCall::StyleIndex(3), .. }));
    // past the table: refused on both sides, nothing changes
    let r = p.step(c1("A2", CellCall::StyleIndex(99)));
    assert!(matches!(r, Err(Error::StyleIndexOutOfRange { .. })), "{r:?}");
    p.finish();
    let live = p.j.shadow().to_bytes().unwrap();
    p.j.rebuild().unwrap();
    assert_eq!(p.j.shadow().to_bytes().unwrap(), live, "replay by content equals the live bytes");
    p.finish();
}

/// P2 asymmetry (PR 165 review, round 1 m1): a session `xf` derived from a
/// Base `xf` with a dangling reference is refused by the Journal
/// (`xf_content`), though the sequential call takes the index. The Journal is
/// unchanged by the refusal. Part B is outside oracle 1 (i) (F8).
#[test]
fn fixed_34_p2_dangling_base_reference_is_refused_by_the_journal_only() {
    use super::fixtures::{custom_styles, package, plain};
    use crate::journal::op::apply_seq;
    let sheet = r#"<dimension ref="A1:A2"/><sheetData><row r="1"><c r="A1"><v>1</v></c></row></sheetData>"#;
    // xf 2 names border 9 of 1
    let styles = custom_styles(false).replace(
        r#"fillId="1" borderId="0""#,
        r#"fillId="1" borderId="9""#,
    );
    let bytes = package(&[("Sheet1", sheet.to_owned())], &[plain("x")], None, &styles);
    let mut p = Pair::new(&bytes);
    p.step(c1("A1", CellCall::StyleIndex(2))).unwrap();
    p.step(c1("A1", CellCall::Font(italic()))).unwrap(); // session xf 4, border 9
    let input = c1("A2", CellCall::StyleIndex(4));
    let before = state_of_journal(&p.j);
    assert!(apply_seq(&mut p.seq, &input).is_ok(), "the sequential call accepts it");
    let r = p.j.append(Api::Native, input);
    assert!(
        matches!(&r, Err(JournalError::Op(Error::DanglingReference { .. }))),
        "{r:?}"
    );
    assert_eq!(state_of_journal(&p.j), before, "a refusal changed the Journal");
}

// C1's R1 cases (spec 34-42; numbered 35+ after the existing fixed_34).
// Each asserts the `applied` flag; Pair asserts bytes/snapshots/tracked on
// every skip.

#[test]
fn fixed_35_spec34_plain_over_rich_is_applied() {
    let mut p = Pair::new(&base());
    p.step(c1("D1", CellCall::String("abc".into()))).unwrap();
    assert!(last_applied(&p), "the runs are dropped: not a no-op");
    let d1 = p.j.cell_key(0, at("D1"));
    assert!(p.j.changed_cells().contains(&d1));
    p.finish();
}

#[test]
fn fixed_36_spec35_a_member_written_back_splits_then_batch_verdicts_agree() {
    let mut p = Pair::new(&group());
    p.step(formula("E3", "$A3+E$1")).unwrap();
    assert!(last_applied(&p), "a member's own text splits as sequentially");
    p.step(write_cells(&[
        ("D2", WriteValue::Number(1.0)),
        ("E3", WriteValue::Number(2.0)),
    ]))
    .unwrap();
    assert!(last_applied(&p));
    p.finish();
}

#[test]
fn fixed_37_spec36_a_missing_cell_is_never_a_noop() {
    // both ops target a missing cell (B5 then B7, both under the bold
    // column): a second write to the now-existing B5 would skip instead,
    // which is fixed 39's case
    let mut p = Pair::new(&base());
    p.step(num("B5", 5.0)).unwrap();
    assert!(last_applied(&p));
    p.step(num("B7", 5.0)).unwrap();
    assert!(last_applied(&p));
    p.finish();
    // `set_style_index(0)` on missing B5 under the bold column (round 5 M1)
    let mut p = Pair::new(&base());
    p.step(c1("B5", CellCall::StyleIndex(0))).unwrap();
    assert!(last_applied(&p));
    assert_eq!(p.j.changed_cells().len(), 1);
    p.finish();
}

#[test]
fn fixed_38_spec37_conditional_on_missing_is_applied_but_noop() {
    let mut p = Pair::new(&base());
    p.step(remove_formula("D10")).unwrap();
    assert!(last_applied(&p));
    p.step(clear("B5")).unwrap();
    assert!(last_applied(&p));
    p.finish();
    assert!(node_tree(&mut p.seq, "D10").is_none());
    assert!(node_tree(p.j.shadow_mut(), "D10").is_none());
    assert!(node_tree(&mut p.seq, "B5").is_none());
    assert!(node_tree(p.j.shadow_mut(), "B5").is_none());
    assert!(p.j.tracked_differs().is_empty());
    assert_eq!(p.j.open_snapshots(), 0);
    assert_eq!(content_seqs(&p.j, "D10"), vec![1]);
    assert_eq!(content_seqs(&p.j, "B5"), vec![2]);
}

#[test]
fn fixed_39_spec38_identity_writes_skip_and_push_keys() {
    let mut p = Pair::new(&base());
    p.step(num("C3", 7.0)).unwrap();
    assert!(!last_applied(&p));
    // same string, same `si`
    p.step(c1("B1", CellCall::String("x".into()))).unwrap();
    assert!(!last_applied(&p));
    // same formula text on a plain formula
    p.step(formula("C1", "A1*2")).unwrap();
    assert!(!last_applied(&p));
    p.finish();
    assert_eq!(content_seqs(&p.j, "C3"), vec![1]);
    assert_eq!(content_seqs(&p.j, "B1"), vec![2]);
    assert_eq!(content_seqs(&p.j, "C1"), vec![3]);
}

#[test]
fn fixed_40_spec39_style_identity_skips() {
    let mut p = Pair::new(&base());
    // A3 holds s=1
    p.step(c1("A3", CellCall::StyleIndex(1))).unwrap();
    assert!(!last_applied(&p));
    // C3 is s-less: missing s equals 0
    p.step(c1("C3", CellCall::StyleIndex(0))).unwrap();
    assert!(!last_applied(&p));
    // a patch resolving to another xf applies
    p.step(c1("A3", CellCall::Font(italic()))).unwrap();
    assert!(last_applied(&p));
    p.finish();
}

#[test]
fn fixed_41_spec40_two_half_needs_both_halves() {
    let serial = date().serial(false).unwrap();
    let mut p = Pair::new(&base());
    p.step(num("C3", serial)).unwrap();
    assert!(last_applied(&p));
    // same serial, but EnsureDate changes the style: the style half vetoes
    p.step(c1("C3", CellCall::DateTime(date()))).unwrap();
    assert!(last_applied(&p));
    // same serial on a date format now: both halves same
    p.step(c1("C3", CellCall::DateTime(date()))).unwrap();
    assert!(!last_applied(&p));
    // same number, other format: the style half vetoes
    p.step(c1("A3", CellCall::NumberWithFormat(3.0, full_format())))
        .unwrap();
    assert!(last_applied(&p));
    p.finish();
}

#[test]
fn fixed_42_spec41_write_cells_skips_only_whole() {
    let mut p = Pair::new(&base());
    p.step(write_cells(&[
        ("C3", WriteValue::Number(7.0)),
        ("A3", WriteValue::Number(3.0)),
    ]))
    .unwrap();
    assert!(!last_applied(&p));
    assert_eq!(p.j.log().len(), 1, "skipped as one call");
    p.step(write_cells(&[
        ("C3", WriteValue::Number(7.0)),
        ("A3", WriteValue::Number(9.0)),
    ]))
    .unwrap();
    assert!(last_applied(&p), "one change applies the whole call");
    p.finish();
}

#[test]
fn fixed_43_spec42_dynamic_and_remove_on_a_formula_apply() {
    let mut p = Pair::new(&base());
    p.step(dynamic("A6", "1+1")).unwrap();
    assert!(last_applied(&p), "dynamic never reports same (C3)");
    p.step(remove_formula("C1")).unwrap();
    assert!(last_applied(&p), "removing f+cache always changes content (C4)");
    p.finish();
}

/// Re-zips `bytes` with one string replaced in `part`: builds files no
/// public call produces (here, a header cell the column name disagrees
/// with, as a third-party writer may leave it).
fn rezip_replace(bytes: &[u8], part: &str, from: &str, to: &str) -> Vec<u8> {
    use std::io::{Cursor, Read, Write};
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).expect("a package");
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for i in 0..z.len() {
        let mut f = z.by_index(i).expect("an entry");
        let name = f.name().expect("a name").into_owned();
        let mut body = String::new();
        f.read_to_string(&mut body).expect("text");
        if name == part {
            assert_eq!(body.matches(from).count(), 1, "one {from} in {part}");
            body = body.replace(from, to);
        }
        out.start_file(name, opts).expect("zip");
        out.write_all(body.as_bytes()).expect("zip");
    }
    out.finish().expect("zip").into_inner()
}

/// B1: an identity write to a diverged header cell is applied, not
/// skipped. Table `T` over `A1:A2` whose header cell holds `Old` while
/// the column is named `Diverged`: the cell report is all-same, but the
/// plan stages a rename, so the skip would drop a real change.
#[test]
fn fixed_44_b1_identity_write_to_a_diverged_header_is_applied() {
    use crate::{CellRange, Table};
    let mut wb = Workbook::new().unwrap();
    wb.worksheet()
        .unwrap()
        .cell_mut(0, 0)
        .unwrap()
        .set_string("Old")
        .unwrap();
    wb.worksheet()
        .unwrap()
        .add_table(Table::new(CellRange::parse("A1:A2").unwrap()).name("T"))
        .unwrap();
    let diverged = rezip_replace(
        &wb.to_bytes().unwrap(),
        "xl/tables/table1.xml",
        r#"name="Old""#,
        r#"name="Diverged""#,
    );
    let mut p = Pair::new(&diverged);
    p.step(c1("A1", CellCall::String("Old".into()))).unwrap();
    assert!(last_applied(&p), "the staged rename is not the identity");
    p.finish();
}

/// B1 (post-merge review): a `v` spelling a fresh reservation is dangling,
/// not the item. The table holds one item (`a` at 0); A1's `v` is 1, which
/// is what `set_string("fresh")` reserves. The sequential side appends the
/// item, so a skip would silently drop the user's write.
#[test]
fn fixed_45_b1_dangling_v_spelling_a_fresh_reservation_is_applied() {
    use super::fixtures::{custom_styles, package, plain};
    let sheet = |v: &str| {
        format!(
            r#"<dimension ref="A1"/><sheetData><row r="1"><c r="A1" t="s"><v>{v}</v></c></row></sheetData>"#
        )
    };
    let styles = custom_styles(false);
    // one item in the table, `v` 1: dangling, and the fresh reservation
    let bytes = package(&[("Sheet1", sheet("1"))], &[plain("a")], None, &styles);
    let mut p = Pair::new(&bytes);
    p.step(c1("A1", CellCall::String("fresh".into()))).unwrap();
    assert!(last_applied(&p), "the append is real: a skip would drop it");
    p.finish();
    // an empty table: `v` 0 is dangling too
    let bytes = package(&[("Sheet1", sheet("0"))], &[], None, &styles);
    let mut p = Pair::new(&bytes);
    p.step(c1("A1", CellCall::String("fresh".into()))).unwrap();
    assert!(last_applied(&p), "index 0 of an empty table is dangling");
    p.finish();
}
