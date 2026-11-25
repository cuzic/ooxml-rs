//! R2 on C2's snapshots, white-box (the task's §5.6).

use ooxml_core::RawNode;

use super::fixtures::{group_workbook, oracle_workbook};
use super::*;
use crate::journal::state::Tree;

fn base() -> Vec<u8> {
    oracle_workbook("A1:C5")
}

fn node_of(j: &mut Journal, sheet: usize, a1: &str) -> Option<RawNode> {
    j.shadow_mut()
        .snapshot_cell(sheet, at(a1))
        .unwrap()
        .node()
        .cloned()
}

fn s_of(n: &RawNode) -> Option<String> {
    n.attr(ooxml_core::QName::new("", "s")).map(str::to_owned)
}

fn without_s(n: &RawNode) -> Tree {
    match tree(n) {
        Tree::El {
            name,
            mut attrs,
            children,
        } => {
            attrs.remove("s");
            Tree::El {
                name,
                attrs,
                children,
            }
        }
        t => t,
    }
}

#[test]
fn r2_write_back_leaves_part_clean() {
    let mut p = run(&base(), [num("A1", 9.0), num("A1", 1.0)]);
    let a1 = p.j.cell_key(0, at("A1"));
    assert!(!p.j.changed_cells().contains(&a1));
    let snap = tree(p.j.snapshot_node(&a1).unwrap().unwrap());
    assert_eq!(tree(&node_of(&mut p.j, 0, "A1").unwrap()), snap);
    let doc =
        p.j.shadow_mut()
            .pkg
            .xml_part("xl/worksheets/sheet1.xml")
            .unwrap();
    assert!(!doc.root().subtree_dirty(), "the worksheet is clean again");
    assert!(!doc.is_dirty());
}

#[test]
fn r2_style_index_zero_restores_absent_s() {
    let mut p = run(&base(), [c1("A1", CellCall::StyleIndex(0))]);
    let a1 = p.j.cell_key(0, at("A1"));
    assert!(!p.j.changed_cells().contains(&a1));
    assert_eq!(
        s_of(&node_of(&mut p.j, 0, "A1").unwrap()),
        None,
        "`s=\"0\"` was written, and the restore took it away"
    );
}

#[test]
fn r2_content_restore_keeps_current_s() {
    let mut p = run(
        &base(),
        [
            c1("A1", CellCall::Font(bold())),
            num("A1", 9.0),
            num("A1", 1.0),
        ],
    );
    let a1 = p.j.cell_key(0, at("A1"));
    assert!(p.j.changed_cells().contains(&a1));
    let snap = p.j.snapshot_node(&a1).unwrap().unwrap().clone();
    let cur = node_of(&mut p.j, 0, "A1").unwrap();
    assert_eq!(
        without_s(&cur),
        without_s(&snap),
        "the content is the snapshot's"
    );
    assert_eq!(
        s_of(&cur).as_deref(),
        Some("1"),
        "the style is the current one (the bold xf)"
    );
    assert_eq!(s_of(&snap), None);
}

#[test]
fn r2_missing_cell_stays_present() {
    let mut p = run(
        &base(),
        [
            c1("D7", CellCall::Font(bold())),
            c1("D7", CellCall::StyleIndex(0)),
        ],
    );
    let d7 = p.j.cell_key(0, at("D7"));
    assert!(
        p.j.changed_cells().contains(&d7),
        "a present empty `c` is not the missing cell"
    );
    assert!(node_of(&mut p.j, 0, "D7").is_some());
}

#[test]
fn r2_handoff_target_tracked() {
    let mut p = run(&group_workbook("A1:H6"), [num("D2", 1.0)]);
    let (d2, c2) = (p.j.cell_key(0, at("D2")), p.j.cell_key(0, at("C2")));
    let changed = p.j.changed_cells();
    assert!(
        changed.contains(&d2) && changed.contains(&c2),
        "{changed:?}"
    );
}

#[test]
fn r2_split_member_not_restored() {
    let mut p = run(
        &group_workbook("A1:H6"),
        [num("E3", 7.0), formula("E3", "$A3+E$1")],
    );
    let e3 = p.j.cell_key(0, at("E3"));
    assert!(p.j.changed_cells().contains(&e3));
    let n = node_of(&mut p.j, 0, "E3").unwrap();
    let f = n
        .elements()
        .find(|e| e.name().local() == "f")
        .expect("a formula");
    assert_eq!(
        f.attr(ooxml_core::QName::new("", "t")),
        None,
        "a normal formula, not a shared member"
    );
}

#[test]
fn refusal_takes_no_snapshot() {
    let mut p = Pair::new(&base());
    assert!(p.step(num("C4", 1.0)).is_err(), "C4 is in a merge");
    assert_eq!(p.j.open_snapshots(), 0);
}

#[test]
fn r2_a_formula_with_the_masters_text_is_not_the_master_back() {
    // `set_formula` with the master's own text leaves a normal formula (the
    // hand-off made `C2` the master): equal text, different `f` attributes,
    // so `D2` is changed and is not restored to `t="shared"` (round 8 M1)
    let mut p = run(&group_workbook("A1:H6"), [formula("D2", "$A2+D$1")]);
    let d2 = p.j.cell_key(0, at("D2"));
    assert!(p.j.changed_cells().contains(&d2));
    let n = node_of(&mut p.j, 0, "D2").unwrap();
    let f = n
        .elements()
        .find(|e| e.name().local() == "f")
        .expect("a formula");
    assert_eq!(f.attr(ooxml_core::QName::new("", "t")), None);
}
