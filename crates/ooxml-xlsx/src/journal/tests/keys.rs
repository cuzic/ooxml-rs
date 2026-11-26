//! The key index (the task's §5.6): which call writes which key, with which seqs.

use super::fixtures::{group_workbook, oracle_workbook};
use super::*;
use crate::journal::op::{Field, StyleGroup};

const GROUPS: [StyleGroup; 6] = [
    StyleGroup::NumFmt,
    StyleGroup::Font,
    StyleGroup::Fill,
    StyleGroup::Border,
    StyleGroup::Alignment,
    StyleGroup::Protection,
];

fn base() -> Vec<u8> {
    oracle_workbook("A1:C5")
}

fn seqs(j: &mut Journal, a1: &str, field: Field) -> Vec<u64> {
    let k = j.cell_key(0, at(a1));
    j.key_seqs(&Key::Cell {
        sheet: k.sheet,
        row: k.row,
        col: k.col,
        field,
    })
    .to_vec()
}

fn style(g: StyleGroup) -> Field {
    Field::Style(g)
}

#[test]
fn a_font_and_a_fill_write_their_own_group_only() {
    let mut p = run(
        &base(),
        [
            c1("A1", CellCall::Font(italic())),
            c1("A1", CellCall::Fill(red())),
        ],
    );
    assert_eq!(seqs(&mut p.j, "A1", style(StyleGroup::Font)), vec![1]);
    assert_eq!(seqs(&mut p.j, "A1", style(StyleGroup::Fill)), vec![2]);
    assert_eq!(seqs(&mut p.j, "A1", Field::Content), Vec::<u64>::new());
    assert_eq!(
        seqs(&mut p.j, "A1", style(StyleGroup::Border)),
        Vec::<u64>::new()
    );
    assert_eq!(
        seqs(&mut p.j, "A1", style(StyleGroup::NumFmt)),
        Vec::<u64>::new()
    );
}

#[test]
fn a_full_format_writes_all_six_groups() {
    let mut p = run(&base(), [c1("A1", CellCall::Format(full_format()))]);
    for g in GROUPS {
        assert_eq!(seqs(&mut p.j, "A1", style(g)), vec![1], "{g:?}");
    }
    assert_eq!(seqs(&mut p.j, "A1", Field::Content), Vec::<u64>::new());
}

#[test]
fn a_style_index_and_a_named_style_write_all_six_groups() {
    let mut p = run(
        &base(),
        [
            c1("A1", CellCall::StyleIndex(1)),
            c1("A2", CellCall::NamedStyle("Normal".into())),
        ],
    );
    for g in GROUPS {
        assert_eq!(seqs(&mut p.j, "A1", style(g)), vec![1], "{g:?}");
        assert_eq!(seqs(&mut p.j, "A2", style(g)), vec![2], "{g:?}");
    }
}

#[test]
fn a_datetime_writes_content_and_the_number_format() {
    let mut p = run(&base(), [c1("A1", CellCall::DateTime(date()))]);
    assert_eq!(seqs(&mut p.j, "A1", Field::Content), vec![1]);
    assert_eq!(seqs(&mut p.j, "A1", style(StyleGroup::NumFmt)), vec![1]);
    assert_eq!(
        seqs(&mut p.j, "A1", style(StyleGroup::Font)),
        Vec::<u64>::new()
    );
}

#[test]
fn a_call_with_a_format_writes_content_and_every_group() {
    let mut p = run(
        &base(),
        [c1("A1", CellCall::NumberWithFormat(5.0, full_format()))],
    );
    assert_eq!(seqs(&mut p.j, "A1", Field::Content), vec![1]);
    for g in GROUPS {
        assert_eq!(seqs(&mut p.j, "A1", style(g)), vec![1], "{g:?}");
    }
}

#[test]
fn a_content_write_writes_no_style_key() {
    let mut p = run(
        &base(),
        [num("A1", 5.0), c1("A1", CellCall::String("s".into()))],
    );
    assert_eq!(seqs(&mut p.j, "A1", Field::Content), vec![1, 2]);
    for g in GROUPS {
        assert_eq!(seqs(&mut p.j, "A1", style(g)), Vec::<u64>::new(), "{g:?}");
    }
}

#[test]
fn write_cells_writes_content_on_each_cell() {
    let mut p = run(
        &base(),
        [write_cells(&[
            ("A1", WriteValue::Number(1.0)),
            ("A2", WriteValue::String("t".into())),
        ])],
    );
    assert_eq!(seqs(&mut p.j, "A1", Field::Content), vec![1]);
    assert_eq!(seqs(&mut p.j, "A2", Field::Content), vec![1]);
    assert_eq!(p.j.log().len(), 1, "one op, one call");
}

#[test]
fn a_hand_off_cell_gets_no_key() {
    let mut p = run(&group_workbook("A1:H6"), [num("D2", 1.0)]);
    assert_eq!(seqs(&mut p.j, "D2", Field::Content), vec![1]);
    assert_eq!(seqs(&mut p.j, "C2", Field::Content), Vec::<u64>::new());
    assert_eq!(p.j.keys().len(), 1);
}

#[test]
fn refusals_push_nothing_and_seqs_count_from_one() {
    let mut p = Pair::new(&base());
    p.step(num("A1", 1.0)).unwrap();
    assert!(p.step(num("C4", 1.0)).is_err());
    assert!(p.step(c1("A1", CellCall::Formula("1+".into()))).is_err());
    p.step(num("A2", 2.0)).unwrap();
    p.step(num("A1", 3.0)).unwrap();
    assert_eq!(
        p.j.log().iter().map(|o| o.seq).collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    assert_eq!(p.j.next_seq(), 4);
    assert_eq!(seqs(&mut p.j, "A1", Field::Content), vec![1, 3]);
    assert_eq!(seqs(&mut p.j, "A2", Field::Content), vec![2]);
    // the Base values skip (R1) but still consume seqs and push keys (C10)
    assert_eq!(
        p.j.log().iter().map(|o| o.applied).collect::<Vec<_>>(),
        vec![false, false, true]
    );
    assert!(
        p.j.log()
            .iter()
            .all(|o| o.born == crate::journal::ids::Epoch(0))
    );
}
