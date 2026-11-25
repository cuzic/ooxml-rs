//! ADR-0024 Test oracle 2 (View law, replay, rollback, Reload law) through
//! the ExcelJS-shaped API, and oracle 5 (GetPut).

mod common;

use common::*;
use xlsx_edit_journal_spike::exceljs::{EjFill, EjFont, EjStyle, EjWorkbook, Value, normalize_echo, normalize_font};
use xlsx_edit_journal_spike::fixture::oracle_workbook;
use xlsx_edit_journal_spike::journal::read_norm;
use xlsx_edit_journal_spike::op::apply_seq;
use xlsx_edit_journal_spike::{Api, CellKey, Content, Fault, JError, Journal, Op, Patch};

fn wb() -> EjWorkbook {
    EjWorkbook::load(oracle_workbook("A1:C5")).unwrap()
}

fn formula(f: &str) -> Value {
    Value::Formula { formula: f.into(), result: Some(Box::new(Value::Number(42.0))) }
}

/// Every value kind ExcelJS can set through part A, with the shape it gave.
fn samples() -> Vec<(&'static str, Value)> {
    vec![
        ("A1", Value::Number(12.5)),
        ("B5", Value::String("hello".into())),
        ("C3", Value::Bool(false)),
        ("D7", Value::Date(date())),
        ("C1", formula("=A2*3")),
        ("A2", Value::Error("#N/A".into())),
    ]
}

#[test]
fn view_law_echoes_the_raw_input_until_written_again() {
    let mut w = wb();
    let mut ws = w.get_worksheet("Sheet1").unwrap();
    for (a1, v) in samples() {
        let mut c = ws.get_cell(a1).unwrap();
        c.set_value(v.clone()).unwrap();
        assert_eq!(c.value().unwrap(), v, "echo of {a1} right after the set");
    }
    // style writes do not end a value echo; other cells' writes neither
    let mut c = ws.get_cell("C1").unwrap();
    c.set_font(EjFont { bold: Some(true), ..EjFont::default() }).unwrap();
    assert_eq!(c.value().unwrap(), formula("=A2*3"));
    // the formula's supplied result is echoed, but the normalized read has none (Q1 (A))
    let k1 = k("C1");
    let n = w.journal.normalized(k1).unwrap().unwrap();
    assert_eq!(n.formula.as_deref(), Some("A2*3"));
    // written again: the new echo
    let mut ws = w.get_worksheet("Sheet1").unwrap();
    let mut c = ws.get_cell("C1").unwrap();
    c.set_value(Value::Number(1.0)).unwrap();
    assert_eq!(c.value().unwrap(), Value::Number(1.0));
}

#[test]
fn style_echo_is_per_group() {
    let mut w = wb();
    let mut ws = w.get_worksheet("Sheet1").unwrap();
    let mut c = ws.get_cell("A2").unwrap();
    let font = EjFont { bold: Some(true), ..EjFont::default() };
    let fill = EjFill { pattern: "solid".into(), fg_argb: Some(0xFFFF0000) };
    c.set_font(font.clone()).unwrap();
    c.set_fill(fill.clone()).unwrap();
    c.set_num_fmt("0.00").unwrap();
    assert_eq!(c.font().unwrap(), Some(font.clone()), "a fill patch does not end the font echo");
    assert_eq!(c.style().unwrap(), EjStyle { num_fmt: Some("0.00".into()), font: Some(font), fill: Some(fill) });
    // a full style replacement ends every group's echo
    c.set_style(EjStyle { num_fmt: Some("0%".into()), ..EjStyle::default() }).unwrap();
    assert_eq!(c.font().unwrap(), None);
    assert_eq!(c.num_fmt().unwrap().as_deref(), Some("0%"));
}

#[test]
fn rollback_echoes_x_again() {
    let mut w = wb();
    let mut ws = w.get_worksheet("Sheet1").unwrap();
    ws.get_cell("B5").unwrap().set_value(Value::String("x".into())).unwrap();
    let t = w.journal.next_seq() - 1;
    let mut ws = w.get_worksheet("Sheet1").unwrap();
    ws.get_cell("B5").unwrap().set_value(Value::String("y".into())).unwrap();
    w.journal.truncate(t).unwrap();
    let mut ws = w.get_worksheet("Sheet1").unwrap();
    assert_eq!(ws.get_cell("B5").unwrap().value().unwrap(), Value::String("x".into()));
    // the shadow was rebuilt by replay and reads the truncated state
    assert_eq!(w.journal.normalized(k("B5")).unwrap().unwrap().value, ooxml_xlsx::CellValue::String("x".into()));
    // and the next seq is not reissued
    assert!(w.journal.next_seq() > t + 1);
}

#[test]
fn replay_matches_a_sequential_workbook() {
    let base = oracle_workbook("A1:C5");
    let ops = vec![
        content(k("A1"), Content::Number(9.0)),
        patch(k("B5"), Patch::Fill(red())),
        content(k("B5"), Content::String("s".into())),
        Op::DateTime(k("D7"), date()),
        patch(k("A3"), Patch::Font(italic())),
        content(k("C1"), Content::RemoveFormula),
        content(k("A1"), Content::Number(8.0)),
    ];
    let keys: Vec<CellKey> = ops.iter().map(Op::key).collect();
    let check = |j: &mut Journal, n: usize| {
        let mut wb = ooxml_xlsx::Workbook::from_bytes(base.clone()).unwrap();
        for op in &ops[..n] {
            apply_seq(&mut wb, op).unwrap();
        }
        for &key in &keys {
            assert_eq!(j.normalized(key).unwrap(), read_norm(&mut wb, key).unwrap(), "{key:?} after {n} ops");
        }
    };
    let mut j = Journal::open(base.clone()).unwrap();
    for op in &ops {
        j.append(Api::Native, op.clone(), None).unwrap();
    }
    // forced rebuild
    j.mark_shadow_stale();
    check(&mut j, ops.len());
    // failed Commit
    j.fault = Some(Fault::Serialize);
    assert!(j.commit().is_err());
    j.fault = None;
    check(&mut j, ops.len());
    // truncation
    j.truncate(4).unwrap();
    check(&mut j, 4);
    assert!(j.replays >= 3);
}

#[test]
fn reload_law() {
    let mut w = wb();
    let mut ws = w.get_worksheet("Sheet1").unwrap();
    let font = EjFont { bold: Some(true), size: Some(14.0), ..EjFont::default() };
    for (a1, v) in samples() {
        ws.get_cell(a1).unwrap().set_value(v).unwrap();
    }
    ws.get_cell("A3").unwrap().set_font(font.clone()).unwrap();
    let out = w.write_buffer().unwrap();
    let mut r = EjWorkbook::load(out).unwrap();
    let mut ws = r.get_worksheet("Sheet1").unwrap();
    for (a1, v) in samples() {
        assert_eq!(ws.get_cell(a1).unwrap().value().unwrap(), normalize_echo(&v), "reload of {a1}");
    }
    assert_eq!(ws.get_cell("A3").unwrap().font().unwrap(), Some(normalize_font(&font)));
    // the writer keeps echoing after save (echo records, §10.1)
    let mut ws = w.get_worksheet("Sheet1").unwrap();
    assert_eq!(ws.get_cell("C1").unwrap().value().unwrap(), formula("=A2*3"));
}

#[test]
fn clear_is_unsupported_and_changes_nothing() {
    let mut w = wb();
    let before = w.journal.__state();
    let mut ws = w.get_worksheet("Sheet1").unwrap();
    let e = ws.get_cell("A1").unwrap().set_value(Value::Null).unwrap_err();
    assert!(matches!(e, JError::Unsupported { .. }), "{e:?}");
    assert_eq!(w.journal.__state(), before);
}

#[test]
fn get_put_empty_and_all_noop_journals_save_the_base() {
    let base = oracle_workbook("A1:C5");
    let mut j = Journal::open(base.clone()).unwrap();
    assert_eq!(j.commit().unwrap().0, base, "empty Journal");
    // write the Base value; write then revert; a value under dependents then revert
    for op in [
        content(k("C3"), Content::Number(7.0)),
        content(k("B1"), Content::String("y".into())),
        content(k("B1"), Content::String("x".into())),
        content(k("A1"), Content::Number(100.0)),
        content(k("A1"), Content::Number(1.0)),
        content(k("C1"), Content::Formula("A1*2".into())),
        content(k("A4"), Content::Bool(true)),
        patch(k("A3"), Patch::Index(1)),
        Op::Style(k2("A1"), Patch::Index(0)),
    ] {
        j.append(Api::Native, op, None).unwrap();
    }
    let (out, report) = j.commit().unwrap();
    assert_eq!(report.skipped_noops, 7, "{report:?}");
    assert_eq!(out, base, "all no-ops: byte-identical");
    // second save of an empty Journal: the first output
    assert_eq!(j.commit().unwrap().0, out);
}
