//! Revision 8 verification, item e: the second session after a save, with
//! the kept shadow (A: the text layer's carried state stands in for C6) and
//! with the output reopened after each save (A′, `reopen_after_save`).
//! Oracles 12 and 13's second-session fixed cases.

mod common;

use common::*;
use xlsx_edit_journal_spike::fixture::oracle_workbook;
use xlsx_edit_journal_spike::{Api, Content, Journal, Mode, Op};

fn journal(base: Vec<u8>, reopen: bool) -> Journal {
    let mut j = Journal::open_with(base, Mode::Shadow).unwrap();
    j.reopen_after_save = reopen;
    j
}

fn run(j: &mut Journal, ops: &[Op]) -> Vec<u8> {
    for op in ops {
        j.append(Api::Native, op.clone(), None).unwrap();
    }
    j.commit().unwrap().0
}

fn num(a1: &str, v: f64) -> Op {
    content(k(a1), Content::Number(v))
}

/// Review round 7, M1: session 1 sets `A1` from 1 to 6 and saves; session 2
/// writes 7 and then 6 (R2 restores it) and saves: byte-equal to session
/// 1's output, in both modes.
#[test]
fn m1_round7_second_session_write_back() {
    for reopen in [false, true] {
        let mut j = journal(oracle_workbook("A1:C5"), reopen);
        let s1 = run(&mut j, &[num("A1", 6.0)]);
        let s2 = run(&mut j, &[num("A1", 7.0), num("A1", 6.0)]);
        assert!(s1 == s2, "reopen={reopen}: session 2 differs from session 1's output");
    }
}

/// Review round 8, M4: session 1 writes `A1` and `A3` and saves; session 2
/// writes `A5` (later in the same part) and back, and saves: byte-equal to
/// session 1's output.
#[test]
fn m4_round8_later_cell_in_a_rerendered_part() {
    for reopen in [false, true] {
        let mut j = journal(oracle_workbook("A1:C5"), reopen);
        let s1 = run(&mut j, &[num("A1", 6.0), num("A3", 8.0)]);
        let s2 = run(&mut j, &[content(k("A5"), Content::Number(4.0)), content(k("A5"), Content::Formula("1+1".into()))]);
        assert!(s1 == s2, "reopen={reopen}: session 2 differs from session 1's output");
    }
}

/// Oracle 13 for the two cases: the kept (or reopened) Journal and a
/// Journal opened from session 1's output give the same next output.
#[test]
fn bisimulation_of_the_fixed_cases() {
    for (l1, l2) in [
        (vec![num("A1", 6.0)], vec![num("A1", 7.0), num("A1", 6.0)]),
        (vec![num("A1", 6.0), num("A3", 8.0)], vec![num("A5", 4.0), content(k("A5"), Content::Formula("1+1".into())), num("C3", 1.0)]),
    ] {
        for reopen in [false, true] {
            let mut j1 = journal(oracle_workbook("A1:C5"), reopen);
            let out1 = run(&mut j1, &l1);
            let mut j2 = journal(out1, reopen);
            let (a, b) = (run(&mut j1, &l2), run(&mut j2, &l2));
            assert!(a == b, "reopen={reopen}: next outputs differ");
        }
    }
}
