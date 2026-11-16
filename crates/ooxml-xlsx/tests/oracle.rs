//! The oracles must be able to fail: a stray change outside the allowed
//! regions, or an insertion at the wrong place, is caught.

mod common;

use common::*;

fn edited_sheet() -> (Vec<u8>, Vec<u8>) {
    let input = read("minimal");
    let out = check_edit("minimal", &input, a1("C1"), &W::Num(1.0));
    (part(&input, "xl/worksheets/sheet1.xml"), part(&out, "xl/worksheets/sheet1.xml"))
}

#[test]
fn t2_accepts_a_local_edit() {
    let (a, b) = edited_sheet();
    assert_t2(&a, &b, a1("C1"));
}

#[test]
#[should_panic(expected = "T2")]
fn t2_catches_a_stray_change() {
    let (a, b) = edited_sheet();
    let b = String::from_utf8(b).unwrap().replace(r#"defaultRowHeight="15""#, r#"defaultRowHeight="16""#);
    assert_t2(&a, b.as_bytes(), a1("C1"));
}

#[test]
#[should_panic(expected = "T2")]
fn t2_catches_a_misplaced_insertion() {
    let input = read("minimal");
    let out = check_edit("minimal", &input, a1("E2"), &W::Num(1.0));
    let (a, b) = (part(&input, "xl/worksheets/sheet1.xml"), part(&out, "xl/worksheets/sheet1.xml"));
    // move the new cell in front of D2 (wrong column order)
    let s = String::from_utf8(b).unwrap();
    let (d2, e2) = (r#"<c r="D2" s="1"/>"#, r#"<c r="E2"><v>1</v></c>"#);
    assert!(s.contains(&format!("{d2}\n{e2}</row>")), "{s}");
    let s = s.replace(&format!("{d2}\n{e2}"), &format!("{e2}{d2}\n"));
    assert_t2(&a, s.as_bytes(), a1("E2"));
}

#[test]
#[should_panic(expected = "must pass through")]
fn l1_catches_a_changed_part() {
    let input = read("minimal");
    let mut pkg = ooxml_core::Package::from_bytes(input.clone()).unwrap();
    pkg.set_part("xl/styles.xml", part(&input, "xl/styles.xml")).unwrap();
    assert_l1_except(&input, &pkg.to_bytes().unwrap(), &[]);
}

#[test]
#[should_panic(expected = "CR")]
fn cr_catches_a_wrong_value() {
    let input = read("minimal");
    let out = check_edit("minimal", &input, a1("B2"), &W::Num(5.0));
    assert_cr(&input, &out, "Sheet1", a1("B2"), &ooxml_xlsx::CellValue::Number(6.0));
}
