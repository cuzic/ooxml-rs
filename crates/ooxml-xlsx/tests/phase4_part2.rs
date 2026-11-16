//! Phase4 part 2 (ADR-0014 §4–§6, §8–§11): behaviour and refusals (every
//! `Err` leaves every byte unchanged). Byte-exact allowed diffs are in
//! `allowed_diff.rs`.

mod common;

use std::assert_matches;

use common::p1::{book, with_part};
use common::*;
use ooxml_xlsx::{Error, HeaderFooter, Margins, Orientation, PageSetup, PrintOptions, Workbook, WorkbookProtection};

const SML: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";

fn try_on<T>(input: &[u8], f: impl FnOnce(&mut ooxml_xlsx::Worksheet<'_>) -> ooxml_xlsx::Result<T>) -> (Workbook, ooxml_xlsx::Result<T>) {
    let mut wb = Workbook::from_bytes(input.to_vec()).unwrap();
    let r = f(&mut wb.worksheet().unwrap());
    if r.is_err() {
        assert_unchanged(&wb, input);
    }
    (wb, r)
}

fn sheet_with(before_data: &str, after_data: &str, data: &str) -> Vec<u8> {
    let b = book(&[("S", "")], "", "");
    with_part(&b, "xl/worksheets/sheet1.xml", &format!(r#"<worksheet xmlns="{SML}">{before_data}<sheetData>{data}</sheetData>{after_data}</worksheet>"#))
}

fn out_xml(wb: &Workbook) -> String {
    sheet_xml(&wb.to_bytes().unwrap())
}

// ---- views (§8) ----

#[test]
fn freeze_split_unfreeze() {
    let input = sheet_with(r#"<sheetViews><sheetView workbookViewId="0"><selection activeCell="C5" sqref="C5"/></sheetView></sheetViews>"#, "", "");
    let (wb, r) = try_on(&input, |ws| ws.freeze_panes(1, 2));
    r.unwrap();
    let xml = out_xml(&wb);
    assert!(xml.contains(r#"<pane xSplit="2" ySplit="1" topLeftCell="C2" activePane="bottomRight" state="frozen"/><selection pane="topRight"/><selection pane="bottomLeft"/><selection pane="bottomRight" activeCell="C5" sqref="C5"/>"#), "{xml}");
    let mut wb = Workbook::from_bytes(wb.to_bytes().unwrap()).unwrap();
    assert!(wb.worksheet().unwrap().unfreeze().unwrap());
    let xml = out_xml(&wb);
    assert!(xml.contains(r#"<sheetView workbookViewId="0"><selection activeCell="C5" sqref="C5"/></sheetView>"#), "{xml}");
    // a sheet without sheetViews gets one; rows only
    let (wb, r) = try_on(&sheet_with("", "", ""), |ws| ws.freeze_panes(3, 0));
    r.unwrap();
    assert!(out_xml(&wb).contains(r#"<sheetViews><sheetView workbookViewId="0"><pane ySplit="3" topLeftCell="A4" activePane="bottomLeft" state="frozen"/><selection pane="bottomLeft"/></sheetView></sheetViews><sheetData"#));
    let (wb, r) = try_on(&sheet_with("", "", ""), |ws| ws.split_panes(1200, 0));
    r.unwrap();
    assert!(out_xml(&wb).contains(r#"<pane xSplit="1200" activePane="topRight" state="split"/>"#));
    // refusals
    let (_, r) = try_on(&input, |ws| ws.freeze_panes(0, 0));
    assert_matches!(r, Err(Error::NothingToFreeze));
    let layout = sheet_with(r#"<sheetViews><sheetView view="pageLayout" workbookViewId="0"/></sheetViews>"#, "", "");
    let (_, r) = try_on(&layout, |ws| ws.freeze_panes(1, 0));
    assert_matches!(r, Err(Error::PageLayoutView));
    let (_, r) = try_on(&input, |ws| ws.set_zoom(5));
    assert_matches!(r, Err(Error::InvalidSetting { .. }));
    let (wb, r) = try_on(&input, |ws| {
        ws.set_zoom(150)?;
        ws.set_show_gridlines(false)?;
        ws.set_right_to_left(true)
    });
    r.unwrap();
    assert!(out_xml(&wb).contains(r#"<sheetView workbookViewId="0" zoomScale="150" showGridLines="0" rightToLeft="1">"#), "{}", out_xml(&wb));
    let (_, r) = try_on(&sheet_with("", "", ""), |ws| ws.unfreeze());
    assert!(!r.unwrap());
}

// ---- columns and rows (§9) ----

#[test]
fn columns_are_split_edited_and_created() {
    let input = sheet_with(r#"<cols><col min="2" max="6" width="12" style="3" customWidth="1"/><col min="9" max="9" width="5"/></cols>"#, "", "");
    let (wb, r) = try_on(&input, |ws| ws.set_column_width(3, 7, 20.5));
    r.unwrap();
    let xml = out_xml(&wb);
    assert!(
        xml.contains(r#"<cols><col min="2" max="3" width="12" style="3" customWidth="1"/><col min="4" max="6" width="20.5" style="3" customWidth="1"/><col min="7" max="8" width="20.5" customWidth="1"/><col min="9" max="9" width="5"/></cols>"#),
        "{xml}"
    );
    // a col exactly covered: edited in place
    let (wb, r) = try_on(&input, |ws| ws.set_column_hidden(8, 8, true));
    r.unwrap();
    assert!(out_xml(&wb).contains(r#"<col min="9" max="9" width="5" hidden="1"/>"#));
    // outline: sheetFormatPr created with defaultRowHeight
    let (wb, r) = try_on(&sheet_with("", "", ""), |ws| ws.set_column_outline(0, 1, 2));
    r.unwrap();
    let xml = out_xml(&wb);
    assert!(xml.contains(r#"<sheetFormatPr defaultRowHeight="15" outlineLevelCol="2"/><cols><col min="1" max="2" width="13" outlineLevel="2"/></cols>"#), "{xml}");
    // refusals
    let two = sheet_with(r#"<cols><col min="1" max="1" width="5"/></cols><cols><col min="2" max="2" width="5"/></cols>"#, "", "");
    let (_, r) = try_on(&two, |ws| ws.set_column_width(0, 0, 3.0));
    assert_matches!(r, Err(Error::AmbiguousColumns(_)));
    let overlap = sheet_with(r#"<cols><col min="1" max="3" width="5"/><col min="2" max="4" width="5"/></cols>"#, "", "");
    let (_, r) = try_on(&overlap, |ws| ws.set_column_width(0, 0, 3.0));
    assert_matches!(r, Err(Error::AmbiguousColumns(_)));
    let (_, r) = try_on(&input, |ws| ws.set_column_width(0, 0, 256.0));
    assert_matches!(r, Err(Error::InvalidSetting { .. }));
    let (_, r) = try_on(&input, |ws| ws.set_column_outline(0, 0, 8));
    assert_matches!(r, Err(Error::InvalidOutlineLevel(8)));
}

#[test]
fn rows_are_edited_or_created_in_order() {
    let input = sheet_with("", "", r#"<row r="2"><c r="A2"><v>1</v></c></row><row r="5"/>"#);
    let (wb, r) = try_on(&input, |ws| {
        ws.set_row_height(1, 30.0)?;
        ws.set_row_hidden(3, true)?;
        ws.set_row_outline(4, 1)?;
        ws.cell_mut(3, 0)?.set_number(7.0)
    });
    r.unwrap();
    let xml = out_xml(&wb);
    assert!(xml.contains(r#"<row r="2" ht="30" customHeight="1"><c r="A2"><v>1</v></c></row><row r="4" hidden="1"><c r="A4"><v>7</v></c></row><row r="5" outlineLevel="1"/>"#), "{xml}");
    assert!(xml.contains(r#"<sheetFormatPr defaultRowHeight="15" outlineLevelRow="1"/>"#), "{xml}");
    let (_, r) = try_on(&input, |ws| ws.set_row_height(0, 500.0));
    assert_matches!(r, Err(Error::InvalidSetting { .. }));
}

// ---- page setup (§10) ----

#[test]
fn page_setup_margins_print_header_footer_breaks() {
    let input = sheet_with("", r#"<pageSetup paperSize="9" r:id="rId1" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"/>"#, "");
    let (wb, r) = try_on(&input, |ws| {
        ws.set_page_setup(&PageSetup { orientation: Some(Orientation::Landscape), fit_to_width: Some(1), fit_to_height: Some(0), fit_to_page: Some(true), ..PageSetup::default() })?;
        ws.set_margins(&Margins::default())?;
        ws.set_print_options(&PrintOptions { gridlines: true, ..PrintOptions::default() })?;
        ws.set_header_footer(&HeaderFooter { odd_header: Some("&L&\"Arial,Bold\"&14Report&RPage &P of &N".into()), ..HeaderFooter::default() })?;
        assert!(ws.add_row_break(10)?);
        assert!(!ws.add_row_break(10)?);
        assert!(ws.add_row_break(5)?);
        ws.add_col_break(3).map(drop)
    });
    r.unwrap();
    let xml = out_xml(&wb);
    for want in [
        r#"<sheetPr><pageSetUpPr fitToPage="1"/></sheetPr>"#,
        r#"<printOptions gridLines="1"/><pageMargins left="0.7" right="0.7" top="0.75" bottom="0.75" header="0.3" footer="0.3"/>"#,
        r#"<pageSetup paperSize="9" r:id="rId1" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" orientation="landscape" fitToWidth="1" fitToHeight="0"/>"#,
        r#"<headerFooter><oddHeader>&amp;L&amp;"Arial,Bold"&amp;14Report&amp;RPage &amp;P of &amp;N</oddHeader></headerFooter>"#,
        r#"<rowBreaks count="2" manualBreakCount="2"><brk id="5" max="16383" man="1"/><brk id="10" max="16383" man="1"/></rowBreaks><colBreaks count="1" manualBreakCount="1"><brk id="3" max="1048575" man="1"/></colBreaks>"#,
    ] {
        assert!(xml.contains(want), "{want}\n{xml}");
    }
    for bad in ["&G", "&Q", "&Kzz0000", &"x".repeat(256), "&\"Arial"] {
        let (_, r) = try_on(&input, |ws| ws.set_header_footer(&HeaderFooter { odd_footer: Some(bad.to_owned()), ..HeaderFooter::default() }));
        assert_matches!(r, Err(Error::InvalidHeaderFooter(_)), "{bad}");
    }
    let (_, r) = try_on(&input, |ws| ws.set_page_setup(&PageSetup { scale: Some(5), ..PageSetup::default() }));
    assert_matches!(r, Err(Error::InvalidSetting { .. }));
    let (_, r) = try_on(&input, |ws| ws.set_margins(&Margins { left: -1.0, ..Margins::default() }));
    assert_matches!(r, Err(Error::InvalidSetting { .. }));
}

// ---- workbook protection (§11) ----

#[test]
fn workbook_protection_and_legacy_hash() {
    let input = book(&[("S", "")], "", "");
    let mut wb = Workbook::from_bytes(input.clone()).unwrap();
    wb.protect_workbook(&WorkbookProtection::new().password("pwd").with_salt([1; 16]).with_spin_count(10)).unwrap();
    let x = text(&wb.to_bytes().unwrap(), "xl/workbook.xml");
    assert!(x.contains(r#"<workbookProtection workbookAlgorithmName="SHA-512" workbookHashValue=""#) && x.contains(r#"workbookSpinCount="10" lockStructure="1"/>"#), "{x}");
    // replacing: the SHA form goes when the legacy hash is asked for
    wb.protect_workbook(&WorkbookProtection::new().password("password").legacy_hash().lock_windows(true)).unwrap();
    let x = text(&wb.to_bytes().unwrap(), "xl/workbook.xml");
    assert!(x.contains(r#"<workbookProtection workbookPassword="83AF" lockStructure="1" lockWindows="1"/>"#), "{x}");
    assert!(wb.unprotect_workbook().unwrap());
    assert!(!wb.unprotect_workbook().unwrap());
    let r = wb.protect_workbook(&WorkbookProtection::new().password("a").with_spin_count(20_000_000));
    assert_matches!(r, Err(Error::InvalidSpinCount(_)));
    // the sheet's legacy option
    let (wb, r) = try_on(&input, |ws| ws.protect_sheet(&ooxml_xlsx::Protection::new().password("password").legacy_hash()));
    r.unwrap();
    assert!(out_xml(&wb).contains(r#"<sheetProtection password="83AF" sheet="1" objects="1" scenarios="1"/>"#));
}
