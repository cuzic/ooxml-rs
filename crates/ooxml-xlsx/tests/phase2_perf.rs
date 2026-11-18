//! PR #50 review M5: the per-write cost of Phase2 writes does not grow with
//! the size of the style tables or the number of formulas.
//!
//! Both tests compare per-write times after a warm-up write (activation and
//! the one-time formula scan are excluded), so they hold on a loaded host and
//! in debug builds. Before the fix the ratios were about 185× (whole-registry
//! clone per write) and 150× (formula re-lex per style-only write).

mod common;

use std::time::{Duration, Instant};

use common::*;
use ooxml_xlsx::{Format, Workbook};

fn base() -> Vec<u8> {
    Workbook::new().unwrap().to_bytes().unwrap()
}

fn timed(f: impl FnOnce()) -> Duration {
    let t = Instant::now();
    f();
    t.elapsed()
}

#[test]
fn formatted_writes_do_not_scale_with_cell_xfs() {
    const WRITES: u32 = 1_000;
    let f = Format::new().set_bold().set_num_format("0.000");
    let run = |input: Vec<u8>| {
        let mut wb = Workbook::from_bytes(input).unwrap();
        let mut ws = wb.worksheet().unwrap();
        ws.cell_mut(0, 0).unwrap().set_number_with_format(1.0, &f).unwrap(); // warm-up: activation, first append
        timed(|| {
            for i in 1..=WRITES {
                ws.cell_mut(i, 0).unwrap().set_number_with_format(1.0, &f).unwrap();
            }
        })
    };
    let s = text(&base(), "xl/styles.xml");
    let xfs: String = (0..5_000).map(|i| format!(r#"<xf numFmtId="{}" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/>"#, i % 50)).collect();
    let big = s.replacen(
        r#"<cellXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/></cellXfs>"#,
        &format!(r#"<cellXfs count="5001"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/>{xfs}</cellXfs>"#),
        1,
    );
    let small_t = run(base());
    let big_t = run(p1::with_part(&base(), "xl/styles.xml", &big));
    eprintln!("{WRITES} formatted writes: 1 xf {small_t:?}, 5,001 xfs {big_t:?}");
    assert!(big_t <= small_t * 4 + Duration::from_millis(500), "1 xf {small_t:?}, 5,001 xfs {big_t:?}");
}

#[test]
fn style_only_writes_do_not_rescan_formulas() {
    const WRITES: u32 = 500;
    let rows: String = (1..=20_000).map(|r| format!(r#"<row r="{r}"><c r="A{r}"><v>{r}</v></c><c r="B{r}"><f>A{r}*2+SUM(A1:A10)</f><v>1</v></c></row>"#)).collect();
    let input = p1::with_part(
        &base(),
        "xl/worksheets/sheet1.xml",
        &format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>{rows}</sheetData></worksheet>"#),
    );
    let mut wb = Workbook::from_bytes(input).unwrap();
    let mut ws = wb.worksheet().unwrap();
    ws.cell_mut(0, 2).unwrap().set_style_index(0).unwrap(); // warm-up: the one formula scan
    ws.cell_mut(0, 3).unwrap().set_number(1.0).unwrap();
    let style_t = timed(|| {
        for i in 1..=WRITES {
            ws.cell_mut(i, 2).unwrap().set_style_index(0).unwrap();
        }
    });
    let value_t = timed(|| {
        for i in 1..=WRITES {
            ws.cell_mut(i, 3).unwrap().set_number(1.0).unwrap();
        }
    });
    eprintln!("{WRITES} writes with 20,000 formulas: style-only {style_t:?}, set_number {value_t:?}");
    assert!(style_t <= value_t * 5 + Duration::from_millis(500), "style-only {style_t:?}, set_number {value_t:?}");
}
