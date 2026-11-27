//! Phase4 part 2 (ADR-0014 §4–§6, §8–§11): behaviour and refusals (every
//! `Err` leaves every byte unchanged). Byte-exact allowed diffs are in
//! `allowed_diff.rs`.

mod common;

use std::assert_matches;

use common::p1::{book, with_part};
use common::*;
use ooxml_xlsx::{Error, HeaderFooter, Margins, Orientation, PageSetup, PrintOptions, Workbook, WorkbookProtection};

type ErrorCheck = fn(&Error) -> bool;

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

#[test]
fn number_stored_as_text_warning_preserves_other_ignored_errors() {
    let input = sheet_with(
        r#"<dimension ref="A1:C4"/>"#,
        r#"<ignoredErrors><ignoredError sqref="A1" formula="1" numberStoredAsText="1"/></ignoredErrors>"#,
        "",
    );
    let (book, result) = try_on(&input, |sheet| sheet.set_ignore_numbers_stored_as_text(false));
    result.unwrap();
    let xml = out_xml(&book);
    assert!(xml.contains(r#"<ignoredError sqref="A1" formula="1"/>"#));
    assert!(!xml.contains("numberStoredAsText"));
    let updated = book.to_bytes().unwrap();
    let (book, result) = try_on(&updated, |sheet| sheet.set_ignore_numbers_stored_as_text(true));
    result.unwrap();
    let xml = out_xml(&book);
    assert!(xml.contains(r#"<ignoredError sqref="A1:C4" numberStoredAsText="1"/>"#));
    assert!(xml.contains(r#"<ignoredError sqref="A1" formula="1"/>"#));

    let ambiguous = sheet_with(r#"<dimension ref="A1"/>"#, "<ignoredErrors/><ignoredErrors/>", "");
    let (_, result) = try_on(&ambiguous, |sheet| sheet.set_ignore_numbers_stored_as_text(true));
    assert!(result.is_err());
}

#[test]
fn theme_reset_replaces_custom_theme_or_adds_missing_theme() {
    const THEME_PART: &str = "xl/theme/theme1.xml";
    const DEFAULT_THEME: &[u8] = include_bytes!("../template/blank/xl/theme/theme1.xml");
    let original = include_bytes!("../template/blank.xlsx");
    let custom_theme = std::str::from_utf8(DEFAULT_THEME).unwrap().replace("Office Theme", "Custom Theme");
    let customized = with_part(original, THEME_PART, &custom_theme);
    let mut workbook = Workbook::from_bytes(customized.clone()).unwrap();
    workbook.reset_theme().unwrap();
    let before_refusal = workbook.to_bytes().unwrap();
    assert!(workbook.set_theme_xml("<wrongRoot/>").is_err());
    assert_eq!(workbook.to_bytes().unwrap(), before_refusal);
    let restored = workbook.to_bytes().unwrap();
    assert_eq!(part(&restored, THEME_PART), DEFAULT_THEME);
    for unchanged in ["xl/workbook.xml", "xl/_rels/workbook.xml.rels", "xl/worksheets/sheet1.xml"] {
        assert_eq!(part(&restored, unchanged), part(&customized, unchanged));
    }

    let without_theme = book(&[("S", "")], "", "");
    let mut workbook = Workbook::from_bytes(without_theme.clone()).unwrap();
    workbook.reset_theme().unwrap();
    let restored = workbook.to_bytes().unwrap();
    assert_eq!(part(&restored, THEME_PART), DEFAULT_THEME);
    assert_eq!(part(&restored, "xl/workbook.xml"), part(&without_theme, "xl/workbook.xml"));
    let mut reloaded = Workbook::from_bytes(restored).unwrap();
    reloaded.reset_theme().unwrap();
    assert_eq!(part(&reloaded.to_bytes().unwrap(), THEME_PART), DEFAULT_THEME);
}

// ---- views (§8) ----

#[test]
fn reads_sheet_views_in_document_order() {
    let bytes = sheet_with(
        r#"<sheetViews><sheetView workbookViewId="1" zoomScale="125"><pane xSplit="600" activePane="topRight"/><selection pane="topRight" activeCell="D4"/></sheetView><sheetView workbookViewId="0" showGridLines="0"><pane ySplit="600" activePane="bottomLeft"/><selection pane="topLeft" activeCell="A1"/></sheetView></sheetViews>"#,
        "",
        "",
    );
    let mut workbook = Workbook::from_bytes(bytes.clone()).unwrap();
    let mut sheet = workbook.worksheet().unwrap();
    let views = sheet.sheet_views().unwrap();
    let attribute = |index: usize, name: &str| {
        views[index].iter().find(|(key, _)| key == name).map(|(_, value)| value.as_str())
    };
    assert_eq!(views.len(), 2);
    assert_eq!(attribute(0, "workbookViewId"), Some("1"));
    assert_eq!(attribute(0, "activeCell"), Some("D4"));
    assert_eq!(attribute(0, "state"), Some("split"));
    assert_eq!(attribute(1, "workbookViewId"), Some("0"));
    assert_eq!(attribute(1, "showGridLines"), Some("0"));
    assert_eq!(attribute(1, "activeCell"), None);
    assert_eq!(sheet.primary_view().unwrap().as_ref(), Some(&views[1]));
    assert_unchanged(&workbook, &bytes);
}

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
fn reads_row_page_breaks_from_worksheet_xml() {
    let input = sheet_with("", r#"<rowBreaks count="3"><brk id="9" min="2" max="3" man="true"/><brk id="4"/><brk id="bad"/></rowBreaks>"#, "");
    let (_, result) = try_on(&input, |ws| ws.row_page_breaks());
    let breaks = result.unwrap();
    assert_eq!(breaks, [
        ooxml_xlsx::RowPageBreak { id: 9, min: 2, max: 3, manual: true },
        ooxml_xlsx::RowPageBreak { id: 4, min: 0, max: 0, manual: false },
    ]);
}

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
    let (wb, r) = try_on(&input, |ws| ws.protect_sheet(&ooxml_xlsx::SheetProtection::new().password("password").legacy_hash()));
    r.unwrap();
    assert!(out_xml(&wb).contains(r#"<sheetProtection password="83AF" sheet="1" objects="1" scenarios="1"/>"#));
}

#[test]
fn clearing_a_row_height_preserves_other_layout() {
    let input = book(&[("S", r#"<row r="2" ht="30" customHeight="1" hidden="1"><c r="A2"><v>7</v></c></row>"#)], "", "");
    let (wb, result) = try_on(&input, |ws| {
        ws.clear_row_height(1)?;
        ws.clear_row_height(5)
    });
    result.unwrap();
    let xml = out_xml(&wb);
    assert!(xml.contains(r#"<row r="2" hidden="1"><c r="A2"><v>7</v></c></row>"#), "{xml}");
    assert!(!xml.contains(r#"<row r="6""#), "{xml}");
    let (wb, result) = try_on(&input, |ws| ws.clear_row_height(5));
    result.unwrap();
    assert_eq!(wb.to_bytes().unwrap(), input);
}

#[test]
fn prune_only_plain_empty_cells_and_rebuild_the_index() {
    use ooxml_xlsx::CellIndex;
    let input = book(&[("S", r#"<row r="2"><c r="A2"/><c r="B2" s="1"/><c r="C2" custom="keep"/><c r="D2"><v>7</v></c><c r="E2" s="0"/></row>"#)], "", "");
    let (wb, result) = try_on(&input, |ws| {
        ws.prune_empty_cells(&[CellIndex::parse("D2")?, CellIndex::parse("E2")?, CellIndex::parse("C2")?,
            CellIndex::parse("A2")?, CellIndex::parse("B2")?, CellIndex::parse("A2")?])?;
        // This edit uses the shifted cell indices.
        assert_eq!(ws.cell(1, 3)?.unwrap().value()?, ooxml_xlsx::CellValue::Number(7.0));
        ws.cell_mut(1, 3)?.set_number(8.0)
    });
    result.unwrap();
    let xml = out_xml(&wb);
    assert!(xml.contains(r#"<row r="2"><c r="B2" s="1"/><c r="C2" custom="keep"/><c r="D2"><v>8</v></c></row>"#), "{xml}");
}

// ---- data validation (§5) ----

const X14_DV: &str = r#"<extLst><ext uri="{CCE6A557-97BC-4b89-ADB6-D9C93CAAB3DF}" xmlns:x14="http://schemas.microsoft.com/office/spreadsheetml/2009/9/main"><x14:dataValidations count="1" xmlns:xm="http://schemas.microsoft.com/office/excel/2006/main"><x14:dataValidation type="list" allowBlank="1"><x14:formula1><xm:f>Other!$A$1:$A$3</xm:f></x14:formula1><xm:sqref>D1:D9</xm:sqref></x14:dataValidation></x14:dataValidations></ext></extLst>"#;

#[test]
fn data_validation_add_and_remove() {
    use ooxml_xlsx::{ErrorStyle, Validation, ValidationOperator, ValidationType};
    let input = book(&[("S", ""), ("Other", "")], r#"<definedName name="Colours">Other!$A$1:$A$3</definedName>"#, "");
    let (wb, r) = try_on(&input, |ws| {
        ws.add_validation("A1:A9", &Validation::list(&["red", "say \"hi\"", "blue"]).input_message("Pick", "one colour"))?;
        ws.add_validation("B1 B3:B4", &Validation::compare(ValidationType::Whole, ValidationOperator::Between, "1", Some("=10")).error_message("No", "1 to 10", ErrorStyle::Warning))?;
        ws.add_validation("C1", &Validation::list_from("=Colours"))
    });
    r.unwrap();
    let xml = out_xml(&wb);
    for want in [
        r#"<dataValidations count="3"><dataValidation type="list" allowBlank="1" showInputMessage="1" promptTitle="Pick" prompt="one colour" sqref="A1:A9"><formula1>"red,say ""hi"",blue"</formula1></dataValidation>"#,
        r#"<dataValidation type="whole" errorStyle="warning" allowBlank="1" showErrorMessage="1" errorTitle="No" error="1 to 10" sqref="B1 B3:B4"><formula1>1</formula1><formula2>10</formula2></dataValidation>"#,
        r#"<dataValidation type="list" allowBlank="1" sqref="C1"><formula1>Colours</formula1></dataValidation></dataValidations>"#,
    ] {
        assert!(xml.contains(want), "{want}\n{xml}");
    }
    // removal of a 2007 rule, then of the last one (container removed)
    let mut wb = Workbook::from_bytes(wb.to_bytes().unwrap()).unwrap();
    {
        let mut ws = wb.worksheet().unwrap();
        let rules = ws.validation_rules().unwrap();
        assert_eq!(rules.iter().map(|rule| rule.reference.as_str()).collect::<Vec<_>>(), ["A1:A9", "B1 B3:B4", "C1"]);
        assert_eq!(rules[1].attributes, ws.validation_attributes_for("B1 B3:B4").unwrap());
        ws.remove_validation("B3:B4 B1").unwrap_err(); // the order is part of sqref here
        ws.remove_validation("B1 B3:B4").unwrap();
        ws.remove_validation("A1:A9").unwrap();
        ws.remove_validation("C1").unwrap();
        assert_matches!(ws.remove_validation("C1"), Err(Error::NoSuchRule { .. }));
    }
    assert!(!out_xml(&wb).contains("dataValidations"));
    // refusals
    let cases: Vec<(Validation, ErrorCheck)> = vec![
        (Validation::list_from("Other!$A$1:$A$3"), |e| matches!(e, Error::CrossSheetReference { .. })),
        (Validation::custom("Nope!A1>0"), |e| matches!(e, Error::UnknownSheet(_))),
        (Validation::list(&["a,b"]), |e| matches!(e, Error::ListItemHasComma(_))),
        (Validation::list(&[&"x".repeat(254)]), |e| matches!(e, Error::ListTooLong(256))),
        (Validation::custom("SUM(("), |e| matches!(e, Error::InvalidFormula { .. })),
        (Validation::custom("1").input_message(&"t".repeat(33), "m"), |e| matches!(e, Error::InvalidSetting { .. })),
    ];
    for (v, ok) in cases {
        let (_, r) = try_on(&input, |ws| ws.add_validation("A1", &v));
        let e = r.unwrap_err();
        assert!(ok(&e), "{e}");
    }
    // overlap with an x14 rule; removing the x14 rule removes its ext
    let x14 = sheet_with("", X14_DV, "");
    let (_, r) = try_on(&x14, |ws| ws.add_validation("D5:E5", &Validation::custom("1")));
    assert_matches!(r, Err(Error::ValidationOverlap { .. }));
    let (wb, r) = try_on(&x14, |ws| ws.remove_validation("D1:D9"));
    r.unwrap();
    assert!(out_xml(&wb).contains("<extLst></extLst>") || !out_xml(&wb).contains("CCE6A557"), "{}", out_xml(&wb));
}

#[test]
fn validation_review_refusals_are_atomic() {
    use ooxml_xlsx::{Validation, ValidationOperator as Op, ValidationType};

    let input = book(&[("S", ""), ("Other", "")], "", "");
    for rule in [
        Validation::custom("Other!A1#>0"),
        Validation::list_from("Other!$A$1#"),
        Validation::custom("Other!F(1)>0"),
        Validation::custom("Table1[Col]>0"),
    ] {
        let (_, result) = try_on(&input, |ws| ws.add_validation("A1", &rule));
        assert_matches!(result, Err(Error::InvalidFormula { .. }));
    }
    for (op, second) in [(Op::Between, None), (Op::NotBetween, None), (Op::Equal, Some("2"))] {
        let rule = Validation::compare(ValidationType::Whole, op, "1", second);
        let (_, result) = try_on(&input, |ws| ws.add_validation("A1", &rule));
        assert_matches!(result, Err(Error::InvalidSetting { .. }));
    }
    let plain = sheet_with("", r#"<dataValidations count="1"><dataValidation sqref="A:A"/></dataValidations>"#, "");
    let x14 = sheet_with("", &X14_DV.replace("D1:D9", "A:A"), "");
    for input in [plain, x14] {
        let (_, result) = try_on(&input, |ws| ws.add_validation("A5", &Validation::custom("1")));
        assert_matches!(result, Err(Error::ValidationOverlap { existing, .. }) if existing == "A:A");
    }
}

#[test]
fn table_row_shifts_check_standard_and_extended_validation_ranges() {
    use ooxml_xlsx::{CellRange, CellWrite, Table};
    let plain = r#"<dataValidations count="1"><dataValidation sqref="B3:B4"/></dataValidations>"#;
    let extended = X14_DV.replace("D1:D9", "B3:B4");
    for validation in [plain, extended.as_str()] {
        let input = sheet_with("", validation, "");
        let (mut wb, result) = try_on(&input, |ws| {
            ws.add_table(Table::new(CellRange::parse("A1:B4").unwrap()).name("Sales"))
        });
        result.unwrap();
        let before = wb.to_bytes().unwrap();
        let mut ws = wb.worksheet().unwrap();
        assert_matches!(ws.remove_table_rows("Sales", 0, 1), Err(Error::TableRange { .. }));
        assert_matches!(
            ws.insert_table_rows("Sales", 1, &[&[CellWrite::String("new"), CellWrite::Number(1.0)]]),
            Err(Error::TableRange { .. })
        );
        assert_eq!(wb.to_bytes().unwrap(), before);
    }
}

#[test]
fn remove_x14_validation_preserves_unknown_siblings() {
    let sibling = r#"<x14:other vendor="keep" />"#;
    let ext_sibling = r#"<ext uri="other"><vendor/></ext>"#;
    let input = X14_DV.replace("</ext>", &format!("{sibling}</ext>"))
        .replace("</extLst>", &format!("{ext_sibling}</extLst>"));
    let (wb, result) = try_on(&sheet_with("", &input, ""), |ws| ws.remove_validation("D1:D9"));
    result.unwrap();
    let start = input.find("<x14:dataValidations").unwrap();
    let end = input.find(sibling).unwrap();
    let expected = format!("{}{}", &input[..start], &input[end..]);
    assert!(out_xml(&wb).contains(&expected));
}

// ---- hyperlinks (§4) ----

#[test]
fn removes_only_single_cell_hyperlinks_in_the_requested_range() {
    use ooxml_xlsx::{CellRange, Link, LinkOptions};
    let mut wb = Workbook::new().unwrap();
    let mut ws = wb.worksheet().unwrap();
    for reference in ["A1", "A2", "B1:B2", "C1"] {
        ws.set_hyperlink(reference, &Link::Location("Sheet1!D4".into()), &LinkOptions::default()).unwrap();
    }
    ws.remove_cell_hyperlinks(CellRange::parse("A1:B2").unwrap()).unwrap();
    let bytes = wb.to_bytes().unwrap();
    let xml = sheet_xml(&bytes);
    assert!(!xml.contains(r#"<hyperlink ref="A1""#) && !xml.contains(r#"<hyperlink ref="A2""#), "{xml}");
    assert!(xml.contains(r#"ref="B1:B2""#) && xml.contains(r#"ref="C1""#), "{xml}");
    let mut reopened = Workbook::from_bytes(bytes.clone()).unwrap();
    reopened.worksheet().unwrap().remove_cell_hyperlinks(CellRange::parse("A1:B2").unwrap()).unwrap();
    assert_eq!(reopened.to_bytes().unwrap(), bytes);
}

#[test]
fn removes_only_single_cell_hyperlinks_at_selected_positions() {
    use ooxml_xlsx::{CellIndex, Link, LinkOptions};
    let mut wb = Workbook::new().unwrap();
    let mut ws = wb.worksheet().unwrap();
    for reference in ["A1", "B1", "C1", "D1:D2"] {
        ws.set_hyperlink(reference, &Link::Location("Sheet1!F1".into()), &LinkOptions::default()).unwrap();
    }
    let cells = ["A1", "C1", "D1"].map(|cell| CellIndex::parse(cell).unwrap()).into_iter().collect();
    ws.remove_cell_hyperlinks_at(&cells).unwrap();
    let bytes = wb.to_bytes().unwrap();
    let xml = sheet_xml(&bytes);
    assert!(!xml.contains(r#"<hyperlink ref="A1""#) && !xml.contains(r#"<hyperlink ref="C1""#), "{xml}");
    assert!(xml.contains(r#"ref="B1""#) && xml.contains(r#"ref="D1:D2""#), "{xml}");
    wb.worksheet().unwrap().remove_cell_hyperlinks_at(&cells).unwrap();
    assert_eq!(wb.to_bytes().unwrap(), bytes);
}

#[test]
fn hyperlinks_external_internal_and_removal() {
    use ooxml_xlsx::{Link, LinkOptions};
    let input = Workbook::new().unwrap().to_bytes().unwrap();
    let mut wb = Workbook::from_bytes(input.clone()).unwrap();
    {
        let mut ws = wb.worksheet().unwrap();
        ws.set_hyperlink("A1", &Link::Url("https://example.com/a b?x=[1]#top".into()), &LinkOptions { tooltip: Some("Go".into()), text: Some("Example".into()), ..LinkOptions::default() }).unwrap();
        ws.set_hyperlink("B2:C3", &Link::Location("Sheet1!D4".into()), &LinkOptions { display: Some("there".into()), ..LinkOptions::default() }).unwrap();
        ws.set_hyperlink("A5", &Link::File("..\\docs\\x#1.xlsx".into()), &LinkOptions::default()).unwrap();
        let links = ws.hyperlinks().unwrap();
        assert_eq!(links.len(), 3);
        assert_eq!(links[0].target.as_deref(), Some("https://example.com/a%20b?x=%5B1%5D"));
        assert_eq!(links[0].location.as_deref(), Some("top"));
        assert_eq!(links[2].target.as_deref(), Some("..\\docs\\x%231.xlsx"));
    }
    let out = wb.to_bytes().unwrap();
    let xml = sheet_xml(&out);
    assert!(xml.contains(r#"<hyperlinks><hyperlink ref="A1" r:id="rId1" location="top" tooltip="Go"/><hyperlink ref="B2:C3" location="Sheet1!D4" display="there"/><hyperlink ref="A5" r:id="rId2"/></hyperlinks>"#), "{xml}");
    let rels = text(&out, "xl/worksheets/_rels/sheet1.xml.rels");
    assert!(rels.contains(r#"Target="https://example.com/a%20b?x=%5B1%5D" TargetMode="External""#), "{rels}");
    assert_cr(&input, &out, "Sheet1", a1("A1"), &ooxml_xlsx::CellValue::String("Example".into()));
    // removal keeps the relationship; the last removal drops the container
    let mut wb = Workbook::from_bytes(out).unwrap();
    {
        let mut ws = wb.worksheet().unwrap();
        ws.remove_hyperlink("A1").unwrap();
        ws.remove_hyperlink("B2:C3").unwrap();
        ws.remove_hyperlink("A5").unwrap();
        assert_matches!(ws.remove_hyperlink("A5"), Err(Error::NoSuchRule { .. }));
    }
    let out2 = wb.to_bytes().unwrap();
    assert!(!sheet_xml(&out2).contains("hyperlink"));
    assert!(text(&out2, "xl/worksheets/_rels/sheet1.xml.rels").contains("example.com"), "the relationship stays");
    // refusals
    let with = Workbook::from_bytes(wb.to_bytes().unwrap()).unwrap().to_bytes().unwrap();
    let cases: Vec<(&str, Link, ErrorCheck)> = vec![
        ("A1", Link::Url("example.com".into()), |e| matches!(e, Error::InvalidUrl(_))),
        ("A1", Link::Url(format!("https://x/{}", "a".repeat(2100))), |e| matches!(e, Error::InvalidUrl(_))),
        ("A1", Link::Location("Nope!A1".into()), |e| matches!(e, Error::UnknownSheet(_))),
    ];
    for (r, l, ok) in cases {
        let (_, res) = try_on(&with, |ws| ws.set_hyperlink(r, &l, &LinkOptions::default()));
        let e = res.unwrap_err();
        assert!(ok(&e), "{e}");
    }
    let mut wb = Workbook::from_bytes(with.clone()).unwrap();
    wb.worksheet().unwrap().set_hyperlink("A1:B2", &Link::Location("Sheet1!Z9".into()), &LinkOptions::default()).unwrap();
    let twice = wb.to_bytes().unwrap();
    let (_, res) = try_on(&twice, |ws| ws.set_hyperlink("B2", &Link::Location("Sheet1!Z9".into()), &LinkOptions::default()));
    assert_matches!(res, Err(Error::HyperlinkOverlap { .. }));
    // a merged range: anchor only
    let mut wb = Workbook::from_bytes(with.clone()).unwrap();
    wb.worksheet().unwrap().merge("D1:E2").unwrap();
    let merged_in = wb.to_bytes().unwrap();
    let (_, res) = try_on(&merged_in, |ws| ws.set_hyperlink("E2", &Link::Location("Sheet1!A1".into()), &LinkOptions::default()));
    assert_matches!(res, Err(Error::MergedCell { .. }));
    let (_, res) = try_on(&merged_in, |ws| ws.set_hyperlink("D1", &Link::Location("Sheet1!A1".into()), &LinkOptions::default()));
    res.unwrap();
}

#[test]
fn hyperlink_unparseable_existing_range_is_atomic() {
    use ooxml_xlsx::{Link, LinkOptions};
    let input = sheet_with("", r#"<hyperlinks><hyperlink ref="A:A" location="S!B1"/></hyperlinks>"#, "");
    let (_, result) = try_on(&input, |ws| ws.set_hyperlink("A5", &Link::Location("S!B1".into()), &LinkOptions::default()));
    assert_matches!(result, Err(Error::HyperlinkOverlap { existing, .. }) if existing == "A:A");
}

#[test]
fn external_hyperlink_declares_relationship_namespace_locally() {
    use ooxml_xlsx::{Link, LinkOptions};
    for existing in ["", r#"<hyperlinks xmlns:r="urn:vendor"><hyperlink ref="A1" location="S!B1"/></hyperlinks>"#] {
        let input = sheet_with("", existing, "");
        let (wb, result) = try_on(&input, |ws| {
            ws.set_hyperlink("A2", &Link::Url("https://example.com".into()), &LinkOptions::default())?;
            let links = ws.hyperlinks()?;
            assert_eq!(links.last().unwrap().target.as_deref(), Some("https://example.com"));
            Ok(())
        });
        result.unwrap();
        let xml = out_xml(&wb);
        let prefix = if existing.is_empty() { "r" } else { "r1" };
        assert!(xml.contains(&format!(r#"<hyperlink xmlns:{prefix}="http://schemas.openxmlformats.org/officeDocument/2006/relationships" ref="A2" {prefix}:id="rId1"/>"#)), "{xml}");
    }
}

// ---- conditional formatting (§6) ----

fn cf_try(input: &[u8], f: impl FnOnce(&mut ooxml_xlsx::Worksheet<'_>) -> ooxml_xlsx::Result<()>) -> String {
    let (wb, r) = try_on(input, f);
    r.unwrap();
    out_xml(&wb)
}

/// Every priority in the sheet, 2007 and x14, sorted.
fn priorities(xml: &str) -> Vec<u32> {
    let mut v: Vec<u32> = xml.split("priority=\"").skip(1).map(|t| t[..t.find('"').unwrap()].parse().unwrap()).collect();
    v.sort_unstable();
    v
}

#[test]
fn conditional_formats_add_priorities_and_kinds() {
    use ooxml_xlsx::{CfOperator, Cfvo, Rule, RuleKind};
    let input = read("poi/NewStyleConditionalFormattings.xlsx");
    let before = sheet_xml(&input);
    // max + 1 over 2007 and x14 (23 is the highest); a new block after the last one
    let xml = cf_try(&input, |ws| ws.add_conditional_format("A2:A9 B4", &Rule::new(RuleKind::CellIs(CfOperator::Between, "1".into(), Some("=$T$5".into()))).with_dxf(2)));
    assert!(xml.contains(r#"<conditionalFormatting sqref="A2:A9 B4"><cfRule type="cellIs" dxfId="2" priority="24" operator="between"><formula>1</formula><formula>$T$5</formula></cfRule></conditionalFormatting><pageMargins "#), "{}", &xml[xml.find("</sheetData>").unwrap()..]);
    let mut p = priorities(&before);
    p.push(24);
    p.sort_unstable();
    assert_eq!(priorities(&xml), p, "no other priority changed");
    // first: every priority + 1, x14 included
    let xml = cf_try(&input, |ws| ws.add_conditional_format_first("A1", &Rule::new(RuleKind::Expression("MOD(ROW(),2)=0".into())).with_dxf(0).stop_if_true(true)));
    let mut p: Vec<u32> = priorities(&before).iter().map(|p| p + 1).collect();
    p.push(1);
    p.sort_unstable();
    assert_eq!(priorities(&xml), p);
    assert!(xml.contains(r#"<cfRule type="expression" dxfId="0" priority="1" stopIfTrue="1"><formula>MOD(ROW(),2)=0</formula></cfRule>"#));
    assert!(xml.contains(r#"<x14:cfRule type="iconSet" priority="7" id="{23404A22"#));
    // the 2007 kinds, from scratch
    let empty = sheet_with("", "", "");
    let xml = cf_try(&empty, |ws| {
        ws.add_conditional_format("A1:A9", &Rule::new(RuleKind::ContainsText("a\"b".into())))?;
        ws.add_conditional_format("B1:B9", &Rule::new(RuleKind::BeginsWith("x".into())))?;
        ws.add_conditional_format("C1:C9", &Rule::new(RuleKind::Top10 { rank: 10, percent: true, bottom: true }))?;
        ws.add_conditional_format("D1:D9", &Rule::new(RuleKind::AboveAverage(false)))?;
        ws.add_conditional_format("E1:E9", &Rule::new(RuleKind::ColorScale(vec![(Cfvo::Min, "FFF8696B".into()), (Cfvo::Percentile(50.0), "FFFFEB84".into()), (Cfvo::Max, "FF63BE7B".into())])))?;
        ws.add_conditional_format("F1:F9", &Rule::new(RuleKind::DataBar(Cfvo::Min, Cfvo::Formula("=$A$1".into()), "FF638EC6".into())))?;
        ws.add_conditional_format("G1:G9", &Rule::new(RuleKind::IconSet(ooxml_xlsx::IconSetValues::ThreeArrows, vec![Cfvo::Percent(0.0), Cfvo::Percent(33.0), Cfvo::Num(67.5)])))?;
        ws.add_conditional_format("H1:H9", &Rule::new(RuleKind::DuplicateValues))
    });
    for want in [
        r#"<conditionalFormatting sqref="A1:A9"><cfRule type="containsText" priority="1" operator="containsText" text="a&quot;b"><formula>NOT(ISERROR(SEARCH("a""b",A1)))</formula></cfRule></conditionalFormatting>"#,
        r#"<cfRule type="beginsWith" priority="2" operator="beginsWith" text="x"><formula>LEFT(B1,LEN("x"))="x"</formula></cfRule>"#,
        r#"<cfRule type="top10" priority="3" percent="1" bottom="1" rank="10"/>"#,
        r#"<cfRule type="aboveAverage" priority="4" aboveAverage="0"/>"#,
        r#"<colorScale><cfvo type="min"/><cfvo type="percentile" val="50"/><cfvo type="max"/><color rgb="FFF8696B"/><color rgb="FFFFEB84"/><color rgb="FF63BE7B"/></colorScale>"#,
        r#"<dataBar><cfvo type="min"/><cfvo type="formula" val="$A$1"/><color rgb="FF638EC6"/></dataBar>"#,
        r#"<iconSet iconSet="3Arrows"><cfvo type="percent" val="0"/><cfvo type="percent" val="33"/><cfvo type="num" val="67.5"/></iconSet>"#,
        r#"<cfRule type="duplicateValues" priority="8"/></conditionalFormatting></worksheet>"#,
    ] {
        assert!(xml.contains(want), "{want}\n{xml}");
    }
}

#[test]
fn conditional_format_refusals_and_removal() {
    use ooxml_xlsx::{CfOperator, Cfvo, Rule, RuleKind};
    let input = book(&[("S", ""), ("Other", "")], "", "");
    let cases: Vec<(Rule, ErrorCheck)> = vec![
        (Rule::new(RuleKind::Expression("Other!A1>0".into())), |e| matches!(e, Error::CrossSheetReference { .. })),
        (Rule::new(RuleKind::CellIs(CfOperator::Equal, "Nope!A1".into(), None)), |e| matches!(e, Error::UnknownSheet(_))),
        (Rule::new(RuleKind::DataBar(Cfvo::Min, Cfvo::Formula("Other!$A$1".into()), "FF000000".into())), |e| matches!(e, Error::CrossSheetReference { .. })),
        (Rule::new(RuleKind::Expression("SUM((".into())), |e| matches!(e, Error::InvalidFormula { .. })),
        (Rule::new(RuleKind::Expression("1".into())).with_dxf(0), |e| matches!(e, Error::InvalidSetting { .. })), // no dxfs
        (Rule::new(RuleKind::ColorScale(vec![(Cfvo::Min, "red".into()), (Cfvo::Max, "FF000000".into())])), |e| matches!(e, Error::InvalidSetting { .. })),
    ];
    for (rule, ok) in cases {
        let (_, r) = try_on(&input, |ws| ws.add_conditional_format("A1", &rule));
        let e = r.unwrap_err();
        assert!(ok(&e), "{e}");
    }
    // pivot blocks: add_first refused, removal refused
    let pivot = sheet_with("", r#"<conditionalFormatting sqref="A1:B2" pivot="1"><cfRule type="expression" priority="1"><formula>1</formula></cfRule></conditionalFormatting>"#, "");
    let (_, r) = try_on(&pivot, |ws| ws.add_conditional_format_first("C1", &Rule::new(RuleKind::UniqueValues)));
    assert_matches!(r, Err(Error::PivotConditionalFormat(_)));
    let (_, r) = try_on(&pivot, |ws| ws.remove_conditional_format("A1:B2", None).map(drop));
    assert_matches!(r, Err(Error::PivotConditionalFormat(_)));
    let (_, r) = try_on(&pivot, |ws| ws.add_conditional_format("C1", &Rule::new(RuleKind::UniqueValues)));
    r.unwrap(); // a plain add leaves the pivot block alone
    // removal with its x14 link: the data bar on E2:E17
    let ncf = read("poi/NewStyleConditionalFormattings.xlsx");
    let (wb, r) = try_on(&ncf, |ws| ws.remove_conditional_format("E2:E17", None));
    let rep = r.unwrap();
    assert_eq!((rep.removed, rep.broken_x14_link), (1, false));
    let xml = out_xml(&wb);
    assert!(!xml.contains("E2:E17") && !xml.contains("9B4F274F"), "{xml}");
    assert!(xml.contains("{23404A22"), "the other x14 rule stays");
    // by priority, in a two-rule block: T2:T17 keeps its iconSet
    let (wb, r) = try_on(&ncf, |ws| ws.remove_conditional_format("T2:T17", Some(2)));
    r.unwrap();
    let xml = out_xml(&wb);
    assert!(xml.contains(r#"<conditionalFormatting sqref="T2:T17"><cfRule type="iconSet" priority="3">"#) && !xml.contains("MOD(ROW($T2),2)=1"), "{xml}");
    let (_, r) = try_on(&ncf, |ws| ws.remove_conditional_format("T2:T17", Some(99)).map(drop));
    assert_matches!(r, Err(Error::NoSuchRule { .. }));
    // a broken link is reported
    let broken = sheet_with("", r#"<conditionalFormatting sqref="A1"><cfRule type="dataBar" priority="1"><dataBar><cfvo type="min"/><cfvo type="max"/><color rgb="FF000000"/></dataBar><extLst><ext uri="{B025F937-C7B1-47D3-B67F-A62EFF666E3E}" xmlns:x14="http://schemas.microsoft.com/office/spreadsheetml/2009/9/main"><x14:id>{0000}</x14:id></ext></extLst></cfRule></conditionalFormatting>"#, "");
    let (_, r) = try_on(&broken, |ws| ws.remove_conditional_format("A1", None));
    assert!(r.unwrap().broken_x14_link);
}

#[test]
fn conditional_format_review_refusals_are_atomic() {
    use ooxml_core::EnumType;
    use ooxml_xlsx::{CfOperator as Op, Cfvo, IconSetValues, Rule, RuleKind};

    let input = book(&[("S", ""), ("Other", "")], "", "");
    for formula in ["Other!A1#>0", "SUM(Other!F(1))>0", "Table1[Col]>0"] {
        let rule = Rule::new(RuleKind::Expression(formula.into()));
        let (_, result) = try_on(&input, |ws| ws.add_conditional_format("A1", &rule));
        assert_matches!(result, Err(Error::InvalidFormula { .. }));
    }
    for set in ["3Stars", "3Triangles", "5Boxes", "NoIcons", "Bogus"] {
        assert_eq!(IconSetValues::parse(set), None);
    }
    for kind in [
        RuleKind::CellIs(Op::Between, "1".into(), None),
        RuleKind::CellIs(Op::NotBetween, "1".into(), None),
        RuleKind::CellIs(Op::Equal, "1".into(), Some("2".into())),
        RuleKind::IconSet(IconSetValues::ThreeArrows, vec![Cfvo::Min]),
        RuleKind::IconSet(IconSetValues::FourArrows, vec![Cfvo::Min; 5]),
        RuleKind::DataBar(Cfvo::Num(f64::NAN), Cfvo::Max, "FF000000".into()),
        RuleKind::DataBar(Cfvo::Num(f64::INFINITY), Cfvo::Max, "FF000000".into()),
        RuleKind::DataBar(Cfvo::Percent(-1.0), Cfvo::Max, "FF000000".into()),
        RuleKind::DataBar(Cfvo::Min, Cfvo::Percentile(101.0), "FF000000".into()),
        RuleKind::Top10 { rank: 0, percent: false, bottom: false },
        RuleKind::Top10 { rank: 101, percent: true, bottom: false },
    ] {
        let rule = Rule::new(kind);
        let (_, result) = try_on(&input, |ws| ws.add_conditional_format("A1", &rule));
        assert_matches!(result, Err(Error::InvalidSetting { .. }));
    }
}

#[test]
fn workbook_edit_refusal_covers_part2() {
    use ooxml_xlsx::{Link, LinkOptions, Rule, RuleKind, Validation, Worksheet};

    let input = book(
        &[
            ("Mismatch", r#"<row r="1"><c r="B2"><f>A1*2</f><v>2</v></c></row>"#),
            ("Edits", ""),
        ],
        "",
        "",
    );
    type Edit = fn(&mut Worksheet<'_>) -> ooxml_xlsx::Result<()>;
    fn rule() -> Rule {
        Rule::new(RuleKind::Expression("A1>0".into()))
    }
    let edits: &[(&str, Edit)] = &[
        ("set hyperlink", |ws| ws.set_hyperlink("A1", &Link::Location("B2".into()), &LinkOptions::default())),
        ("remove hyperlink", |ws| ws.remove_hyperlink("A1")),
        ("add validation", |ws| ws.add_validation("A1", &Validation::list(&["yes"]))),
        ("remove validation", |ws| ws.remove_validation("A1")),
        ("add conditional formatting", |ws| ws.add_conditional_format("A1", &rule())),
        ("prepend conditional formatting", |ws| ws.add_conditional_format_first("A1", &rule())),
        ("remove conditional formatting", |ws| ws.remove_conditional_format("A1", None).map(drop)),
        ("freeze panes", |ws| ws.freeze_panes(1, 0)),
        ("split panes", |ws| ws.split_panes(1200, 0)),
        ("unfreeze", |ws| ws.unfreeze().map(drop)),
        ("zoom", |ws| ws.set_zoom(90)),
        ("gridlines", |ws| ws.set_show_gridlines(false)),
        ("right to left", |ws| ws.set_right_to_left(true)),
        ("column width", |ws| ws.set_column_width(0, 0, 17.5)),
        ("column hidden", |ws| ws.set_column_hidden(0, 0, true)),
        ("column outline", |ws| ws.set_column_outline(0, 0, 1)),
        ("row height", |ws| ws.set_row_height(0, 22.5)),
        ("row hidden", |ws| ws.set_row_hidden(0, true)),
        ("row outline", |ws| ws.set_row_outline(0, 1)),
        ("page setup", |ws| ws.set_page_setup(&PageSetup::default())),
        ("margins", |ws| ws.set_margins(&Margins::default())),
        ("print options", |ws| ws.set_print_options(&PrintOptions::default())),
        ("header and footer", |ws| ws.set_header_footer(&HeaderFooter::default())),
        ("row break", |ws| ws.add_row_break(1).map(drop)),
        ("column break", |ws| ws.add_col_break(1).map(drop)),
    ];
    let mut wb = Workbook::from_bytes(input.clone()).unwrap();
    for (name, edit) in edits {
        let result = edit(&mut wb.worksheet_by_name("Edits").unwrap());
        assert_matches!(result, Err(Error::InvalidReference(ref reason)) if reason.contains("B2 in row 1"), "{name}");
        assert_unchanged(&wb, &input);
    }
    let result = wb.protect_workbook(&WorkbookProtection::new());
    assert_matches!(result, Err(Error::InvalidReference(ref reason)) if reason.contains("B2 in row 1"));
    assert_unchanged(&wb, &input);
    let result = wb.unprotect_workbook();
    assert_matches!(result, Err(Error::InvalidReference(ref reason)) if reason.contains("B2 in row 1"));
    assert_unchanged(&wb, &input);
}

#[test]
fn prepending_conditional_format_rejects_invalid_priorities_atomically() {
    use ooxml_xlsx::{Rule, RuleKind};

    let rule = Rule::new(RuleKind::UniqueValues);
    for attr in ["", r#" priority="0""#, r#" priority="-1""#, r#" priority="x""#, r#" priority="1.0""#] {
        let legacy = format!(r#"<conditionalFormatting sqref="A1"><cfRule type="uniqueValues"{attr}/></conditionalFormatting>"#);
        let x14 = format!(r#"<extLst><ext uri="{{78C0D931-6437-407d-A8EE-F0AAD7539E65}}"><x14:conditionalFormattings xmlns:x14="http://schemas.microsoft.com/office/spreadsheetml/2009/9/main"><x14:conditionalFormatting><x14:cfRule type="iconSet" id="{{23404A22-2791-4FFE-B3BF-645D3DA783FE}}"{attr}/></x14:conditionalFormatting></x14:conditionalFormattings></ext></extLst>"#);
        for existing in [legacy, x14] {
            let input = sheet_with("", &existing, "");
            let (_, result) = try_on(&input, |ws| ws.add_conditional_format_first("A2", &rule));
            assert_matches!(result, Err(Error::InvalidSetting { .. }));
            let (wb, result) = try_on(&input, |ws| ws.add_conditional_format("A2", &rule));
            result.unwrap();
            assert!(out_xml(&wb).contains(&existing));
        }
    }
}

#[test]
fn comparison_validation_refuses_list_and_custom_atomically() {
    use ooxml_xlsx::{Validation, ValidationOperator, ValidationType};

    let input = sheet_with("", "", "");
    for kind in [ValidationType::List, ValidationType::Custom] {
        let validation = Validation::compare(kind, ValidationOperator::Between, "1", Some("2"));
        let (_, result) = try_on(&input, |ws| ws.add_validation("A1", &validation));
        assert_matches!(result, Err(Error::InvalidSetting { .. }));
    }
}

#[test]
fn conditional_format_priority_limit_is_atomic() {
    use ooxml_xlsx::{Rule, RuleKind};

    let input = sheet_with("", r#"<conditionalFormatting sqref="A1"><cfRule type="uniqueValues" priority="2147483647"/></conditionalFormatting>"#, "");
    let rule = Rule::new(RuleKind::UniqueValues);
    let (_, append) = try_on(&input, |ws| ws.add_conditional_format("B1", &rule));
    let (_, prepend) = try_on(&input, |ws| ws.add_conditional_format_first("B1", &rule));
    assert_matches!(append, Err(Error::InvalidSetting { .. }));
    assert_matches!(prepend, Err(Error::InvalidSetting { .. }));
}

#[test]
fn conditional_format_uses_the_differential_format_registry() {
    use ooxml_xlsx::{DxfFormat, Rule, RuleKind};

    let mut wb = Workbook::new().unwrap();
    let format = DxfFormat::new().set_num_format("0.000");
    let id = wb.add_differential_format(&format).unwrap();
    assert_eq!(wb.add_differential_format(&format).unwrap(), id);
    let rule = Rule::new(RuleKind::UniqueValues).with_dxf(id);
    wb.worksheet().unwrap().add_conditional_format("A1", &rule).unwrap();
    let bytes = wb.to_bytes().unwrap();
    let styles = String::from_utf8(part(&bytes, "xl/styles.xml")).unwrap();
    assert!(styles.contains(r#"<dxfs count="1">"#), "{styles}");
    let sheet = sheet_xml(&bytes);
    assert!(sheet.contains(&format!(r#"dxfId="{id}""#)), "{sheet}");
}

#[test]
fn layout_append_dry_runs_count_alternate_content_limits() {
    fn ac(member: &str) -> String {
        format!(r#"<mc:AlternateContent xmlns:mc="{}" xmlns:x="{SML}"><mc:Choice Requires="x">{member}</mc:Choice></mc:AlternateContent>"#,
                ooxml_core::name::MC_NS)
    }
    type Edit = fn(&mut ooxml_xlsx::Worksheet<'_>) -> ooxml_xlsx::Result<()>;
    let cases: [(Vec<u8>, Edit); 6] = [
        (sheet_with(&ac("<sheetViews/>"), "", ""), |ws| ws.freeze_panes(1, 0)),
        (sheet_with(&ac("<sheetFormatPr/>"), "", ""), |ws| ws.set_column_outline(0, 0, 1)),
        (sheet_with(&ac("<sheetFormatPr/>"), "", ""), |ws| ws.set_row_outline(0, 1)),
        (sheet_with(&ac("<sheetPr/>"), "", ""), |ws| ws.set_page_setup(&PageSetup { fit_to_page: Some(true), ..PageSetup::default() })),
        (sheet_with(&format!("<sheetPr>{}</sheetPr>", ac("<pageSetUpPr/>")), "", ""), |ws| {
            ws.set_page_setup(&PageSetup { fit_to_page: Some(true), ..PageSetup::default() })
        }),
        (sheet_with("", &ac("<pageSetup/>"), ""), |ws| ws.set_page_setup(&PageSetup::default())),
    ];
    for (input, edit) in cases {
        let (wb, result) = try_on(&input, edit);
        assert_matches!(result, Err(Error::Core(ooxml_core::Error::AlreadyPresent(_))));
        let unedited = Workbook::from_bytes(input).unwrap().to_bytes().unwrap();
        assert_eq!(wb.to_bytes().unwrap(), unedited);
    }

    // Worksheet columns are repeatable. The preflight still refuses a
    // branch whose interval straddles their slot.
    let input = sheet_with(&ac("<sheetViews/><sheetData/>"), "", "");
    let (wb, result) = try_on(&input, |ws| ws.set_column_width(0, 0, 12.0));
    assert_matches!(result, Err(Error::Core(ooxml_core::Error::UnresolvedSibling(_))));
    assert_eq!(wb.to_bytes().unwrap(), Workbook::from_bytes(input).unwrap().to_bytes().unwrap());
}

#[test]
fn indexed_column_splits_and_breaks_refuse_alternate_content_before_apply() {
    let ac = |child: &str| {
        format!(r#"<mc:AlternateContent xmlns:mc="{}" xmlns:x="{SML}"><mc:Choice Requires="x">{child}</mc:Choice></mc:AlternateContent>"#,
                ooxml_core::name::MC_NS)
    };
    let cols = format!(r#"<cols><col min="1" max="3" width="10"/>{}</cols>"#, ac(r#"<col min="9" max="9"/>"#));
    let input = sheet_with(&cols, "", "");
    let (wb, result) = try_on(&input, |ws| ws.set_column_outline(1, 1, 1));
    assert_matches!(result, Err(Error::Core(ooxml_core::Error::UnresolvedSibling(_))));
    assert_eq!(wb.to_bytes().unwrap(), Workbook::from_bytes(input).unwrap().to_bytes().unwrap());

    let breaks = format!(r#"<rowBreaks>{}<brk id="7"/></rowBreaks>"#, ac(r#"<brk id="5"/>"#));
    let input = sheet_with("", &breaks, "");
    let (wb, result) = try_on(&input, |ws| ws.add_row_break(2));
    assert_matches!(result, Err(Error::Core(ooxml_core::Error::UnresolvedSibling(_))));
    assert_eq!(wb.to_bytes().unwrap(), Workbook::from_bytes(input).unwrap().to_bytes().unwrap());
}

#[test]
fn standalone_extended_icons_preserve_formulas_and_reject_duplicate_ids() {
    use ooxml_xlsx::{Cfvo, ExtendedIconRule};
    let mut wb = Workbook::new().unwrap();
    let thresholds = [
        (Cfvo::Min, None),
        (Cfvo::Num(10.0), Some(false)),
        (Cfvo::Formula("A1+2".into()), Some(true)),
    ];
    let rule = ExtendedIconRule {
        id: Some("{00000000-0000-4000-8000-000000000001}"),
        priority: Some(1),
        priority_floor: 0,
        name: "3Stars",
        thresholds: &thresholds,
        icons: &[("NoIcons".into(), 0), ("5Boxes".into(), 4), ("3Triangles".into(), 1)],
        reverse: true,
        show_value: false,
    };
    wb.worksheet().unwrap().add_extended_icon_rule("A1:A3", &rule).unwrap();
    let output = wb.to_bytes().unwrap();
    let xml = sheet_xml(&output);
    assert!(xml.contains(r#"iconSet="3Stars" reverse="1" showValue="0" custom="1""#), "{xml}");
    assert!(xml.contains(r#"type="num" gte="0""#), "{xml}");
    assert!(xml.contains(">A1+2</xm:f>"), "{xml}");
    assert!(xml.contains(">A1:A3</xm:sqref>"), "{xml}");
    let mut reopened = Workbook::from_bytes(output.clone()).unwrap();
    assert_eq!(reopened.worksheet().unwrap().extended_conditional_format_blocks().unwrap().len(), 1);
    assert!(xml.contains(r#"iconSet="5Boxes" iconId="4""#), "{xml}");
    let invalid_icons = [("3Triangles".into(), 3), ("5Boxes".into(), 4), ("NoIcons".into(), 0)];
    let invalid = ExtendedIconRule { icons: &invalid_icons, ..rule };
    assert_matches!(reopened.worksheet().unwrap().add_extended_icon_rule("B1", &invalid), Err(Error::InvalidSetting { .. }));
    assert_unchanged(&reopened, &output);
    let duplicate = ExtendedIconRule { priority: Some(2), ..rule };
    assert_matches!(reopened.worksheet().unwrap().add_extended_icon_rule("B1", &duplicate), Err(Error::InvalidSetting { .. }));
    assert_unchanged(&reopened, &output);
}

#[test]
fn extended_data_bar_links_both_rules_and_refuses_invalid_settings_atomically() {
    use ooxml_xlsx::{Cfvo, Color, ExtendedDataBar, ExtendedDataBarThreshold, Rule, Tint};
    let mut wb = Workbook::new().unwrap();
    let thresholds = [Cfvo::Min, Cfvo::Formula("A1+10".into())];
    let rule = Rule::data_bar(thresholds[0].clone(), thresholds[1].clone(), "FF112233".into(), 0, 100, true).unwrap();
    wb.worksheet().unwrap().add_conditional_format("A1:A3", &rule).unwrap();
    let initial = wb.to_bytes().unwrap();
    let mut wb = Workbook::from_bytes(initial.clone()).unwrap();
    let bar = ExtendedDataBar {
        id: Some("{00000000-0000-4000-8000-000000000025}"),
        thresholds: &[ExtendedDataBarThreshold::AutoMin, ExtendedDataBarThreshold::Value(thresholds[1].clone())],
        lengths: (Some(0), Some(100)),
        flags: [true, false, true, true].map(Some),
        show_value: None,
        axis: Some("middle"),
        direction: Some("rightToLeft"),
        colors: [Some(Color::Rgb(0xFF556677)), Some(Color::Theme { index: 4, tint: Tint::new(-0.25).unwrap() }),
            Some(Color::Indexed(13)), None, Some(Color::Auto)],
    };
    let invalid = ExtendedDataBar { axis: Some("bad"), ..bar };
    assert_matches!(wb.worksheet().unwrap().attach_extended_data_bar("A1:A3", &invalid), Err(Error::InvalidSetting { .. }));
    assert_unchanged(&wb, &initial);
    wb.worksheet().unwrap().attach_extended_data_bar("A1:A3", &bar).unwrap();
    let xml = out_xml(&wb);
    assert_eq!(xml.matches(bar.id.unwrap()).count(), 2, "{xml}");
    assert!(xml.contains(r#"axisPosition="middle" direction="rightToLeft""#), "{xml}");
    assert!(xml.contains(r#"<x14:fillColor rgb="FF556677""#), "{xml}");
    assert!(xml.contains(r#"theme="4" tint="-0.25""#), "{xml}");
    assert!(xml.contains(r#"indexed="13""#), "{xml}");
    assert!(xml.contains(r#"<x14:axisColor auto="1""#), "{xml}");
    assert!(xml.contains(r#"<x14:cfvo type="autoMin""#), "{xml}");
    assert!(xml.contains(">A1+10</xm:f>"), "{xml}");
    wb.worksheet().unwrap().remove_conditional_format("A1:A3", None).unwrap();
    assert!(!out_xml(&wb).contains(bar.id.unwrap()));
}

#[test]
fn extended_data_bar_writes_only_declared_attributes() {
    use ooxml_xlsx::{Cfvo, Color, ExtendedDataBar, ExtendedDataBarThreshold, Rule};
    let thresholds = [ExtendedDataBarThreshold::Value(Cfvo::Min), ExtendedDataBarThreshold::Value(Cfvo::Max)];
    let declared = ExtendedDataBar {
        id: None, thresholds: &thresholds, lengths: (None, None), flags: [None; 4], show_value: None,
        axis: None, direction: None, colors: [None, None, None, None, None],
    };
    let base = Rule::data_bar_declared(Cfvo::Min, Cfvo::Max, Color::Rgb(0xFF638EC6), None, None, None).unwrap();
    let mut wb = Workbook::new().unwrap();
    wb.worksheet().unwrap().add_conditional_format("A1:A3", &base).unwrap();
    let before = wb.to_bytes().unwrap();
    let mut wb = Workbook::from_bytes(before.clone()).unwrap();
    // Omitted lengths are range-checked against the schema defaults (10 and 90), atomically.
    for invalid in [
        ExtendedDataBar { lengths: (Some(95), None), ..declared },
        ExtendedDataBar { lengths: (None, Some(5)), ..declared },
        ExtendedDataBar { lengths: (Some(0), Some(101)), ..declared },
        ExtendedDataBar { direction: Some("upward"), ..declared },
    ] {
        assert_matches!(wb.worksheet().unwrap().attach_extended_data_bar("A1:A3", &invalid), Err(Error::InvalidSetting { .. }));
        assert_unchanged(&wb, &before);
    }
    let x14_bar = |wb: &Workbook| {
        let xml = out_xml(wb);
        let start = xml.find("<x14:dataBar").unwrap();
        xml[start..xml[start..].find('>').unwrap() + start + 1].to_string()
    };
    wb.worksheet().unwrap().attach_extended_data_bar("A1:A3", &declared).unwrap();
    assert_eq!(x14_bar(&wb), "<x14:dataBar>");
    let mut wb = Workbook::from_bytes(before).unwrap();
    let some = ExtendedDataBar {
        lengths: (Some(0), Some(100)), flags: [Some(true), Some(false), None, Some(false)],
        axis: Some("middle"), direction: None, ..declared
    };
    wb.worksheet().unwrap().attach_extended_data_bar("A1:A3", &some).unwrap();
    assert_eq!(x14_bar(&wb), concat!(r#"<x14:dataBar minLength="0" maxLength="100" border="1" gradient="0" "#,
        r#"negativeBarBorderColorSameAsPositive="0" axisPosition="middle">"#));
}

#[test]
fn declared_data_bar_writes_only_declared_layout_attributes() {
    use ooxml_xlsx::{Cfvo, Color, Rule};
    let bar = |min, max, show| Rule::data_bar_declared(Cfvo::Min, Cfvo::Max, Color::Rgb(0xFF638EC6), min, max, show);
    let xml_of = |rule: &Rule| {
        let mut wb = Workbook::new().unwrap();
        wb.worksheet().unwrap().add_conditional_format("A1", rule).unwrap();
        let xml = out_xml(&wb);
        let start = xml.find("<dataBar").unwrap();
        xml[start..xml[start..].find('>').unwrap() + start + 1].to_string()
    };
    assert_eq!(xml_of(&bar(None, None, None).unwrap()), "<dataBar>");
    assert_eq!(xml_of(&bar(Some(5), None, None).unwrap()), r#"<dataBar minLength="5">"#);
    assert_eq!(xml_of(&bar(None, Some(70), Some(true)).unwrap()), r#"<dataBar maxLength="70" showValue="1">"#);
    assert_eq!(xml_of(&bar(Some(10), Some(90), Some(false)).unwrap()), r#"<dataBar minLength="10" maxLength="90" showValue="0">"#);
    // Omitted limits are checked against the schema defaults (10 and 90).
    assert_matches!(bar(Some(95), None, None), Err(Error::InvalidSetting { .. }));
    assert_matches!(bar(None, Some(5), None), Err(Error::InvalidSetting { .. }));
    assert_matches!(bar(None, Some(101), None), Err(Error::InvalidSetting { .. }));
    // The fixed-layout constructor still writes both limits and only a hidden value.
    let fixed = Rule::data_bar(Cfvo::Min, Cfvo::Max, "FF638EC6".into(), 10, 90, true).unwrap();
    assert_eq!(xml_of(&fixed), r#"<dataBar minLength="10" maxLength="90">"#);
}
