mod common;

use std::assert_matches;

use common::p1::{SML, book, with_part};
use common::sheet_xml;
use ooxml_core::ElementSpec;
use ooxml_schema::sml;
use ooxml_xlsx::{Error, Workbook};

const WRAPPER_NS: &str = r#"xmlns:q="urn:test:extension" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006""#;
const PROCESS_CONTENT: &str = r#"mc:Ignorable="q" mc:ProcessContent="q:w""#;
const PLAIN_ROW: &str = r#"<row r="1"><c r="A1"><v>1</v></c></row>"#;

#[test]
fn wrapped_grid_refuses_cell_edits_on_the_affected_sheet() {
    for mc in ["", r#"mc:Ignorable="q""#, r#"mc:Ignorable="q" mc:ProcessContent="q:w""#] {
        let cell = format!(r#"<row r="1"><c r="A1"/><q:w {WRAPPER_NS} {mc}><q:nested><c r="B1"/></q:nested></q:w></row>"#);
        let row = format!(r#"{PLAIN_ROW}<q:w {WRAPPER_NS} {mc}><row r="2"><c r="B2"/></row></q:w>"#);
        let empty_row = format!(r#"{PLAIN_ROW}<q:w {WRAPPER_NS} {mc}><row r="2"/></q:w>"#);
        for (rows, target_row) in [(&cell, 0), (&row, 1), (&empty_row, 1)] {
            let input = book(&[("Wrapped", rows), ("Plain", PLAIN_ROW)], "", "");
            let mut wb = Workbook::from_bytes(input.clone()).unwrap();
            for (r, c) in [(target_row, 1), (4, 0)] {
                let mut ws = wb.worksheet_by_name("Wrapped").unwrap();
                for result in [
                    ws.cell_mut(r, c).unwrap().set_number(7.0),
                    ws.cell_mut(r, c).unwrap().set_string("text"),
                    ws.cell_mut(r, c).unwrap().set_formula("1+2"),
                ] {
                    assert_matches!(result, Err(Error::Core(ooxml_core::Error::UnresolvedSibling(q))) if q == sml::SheetData::QNAME, "{rows}");
                }
                assert_eq!(wb.to_bytes().unwrap(), input);
            }
            wb.worksheet_by_name("Plain").unwrap().cell_mut(0, 0).unwrap().set_number(2.0).unwrap();
        }
    }
}

#[test]
fn wrapped_grid_refuses_dependent_cache_edits() {
    let rows = format!(
        r#"<row r="1"><c r="A1"><f>Plain!A1*2</f><v>2</v></c><q:w {WRAPPER_NS}><c r="B1"/></q:w></row>"#
    );
    let input = book(&[("Wrapped", &rows), ("Plain", PLAIN_ROW)], "", "");
    let mut wb = Workbook::from_bytes(input.clone()).unwrap();
    assert_matches!(
        wb.worksheet_by_name("Plain").unwrap().cell_mut(0, 0).unwrap().set_number(2.0),
        Err(Error::Core(ooxml_core::Error::UnresolvedSibling(q))) if q == sml::SheetData::QNAME
    );
    assert_eq!(wb.to_bytes().unwrap(), input);
}

#[test]
fn extension_data_without_sml_grid_entries_stays_editable() {
    let extension = format!(r#"<q:w {WRAPPER_NS} mc:Ignorable="q" mc:ProcessContent="q:w"><q:row><q:c/></q:row></q:w>"#);
    let rows = format!(r#"<row r="1"><c r="A1"/>{extension}</row>{extension}"#);
    let input = book(&[("Sheet1", &rows)], "", "");
    let mut wb = Workbook::from_bytes(input).unwrap();
    let mut ws = wb.worksheet().unwrap();
    ws.cell_mut(0, 1).unwrap().set_number(7.0).unwrap();
    ws.cell_mut(0, 2).unwrap().set_string("text").unwrap();
    ws.cell_mut(1, 1).unwrap().set_formula("1+2").unwrap();
    assert_eq!(sheet_xml(&wb.to_bytes().unwrap()).matches(&extension).count(), 2);
}

fn assert_wrapped_grid_error<T: std::fmt::Debug>(result: ooxml_xlsx::Result<T>) {
    assert_matches!(
        result,
        Err(Error::Core(ooxml_core::Error::UnresolvedSibling(q))) if q == sml::SheetData::QNAME
    );
}

#[test]
fn wrapped_rows_refuse_row_properties_and_merges() {
    let rows = format!(
        r#"{PLAIN_ROW}<q:w {WRAPPER_NS} {PROCESS_CONTENT}><row r="2"><c r="A2"><v>5</v></c></row></q:w>"#
    );
    let input = book(&[("Sheet1", &rows)], "", "");
    type Edit = fn(&mut ooxml_xlsx::Worksheet<'_>) -> ooxml_xlsx::Result<()>;
    let edits: [Edit; 5] = [
        |ws| ws.set_row_height(1, 30.0),
        |ws| ws.set_row_hidden(1, true),
        |ws| ws.set_row_outline(1, 1),
        |ws| ws.merge("A1:A2"),
        |ws| ws.merge_clearing("A1:A2"),
    ];
    for edit in edits {
        let mut wb = Workbook::from_bytes(input.clone()).unwrap();
        assert_wrapped_grid_error(edit(&mut wb.worksheet().unwrap()));
        assert_eq!(wb.to_bytes().unwrap(), input);
    }
}

#[test]
fn process_content_wrapped_cell_refuses_single_and_bulk_reads() {
    let rows = format!(
        r#"<row r="1"><q:w {WRAPPER_NS} {PROCESS_CONTENT}><c r="B1"><v>5</v></c></q:w></row>"#
    );
    let input = book(&[("Sheet1", &rows)], "", "");
    let mut wb = Workbook::from_bytes(input.clone()).unwrap();
    {
        let ws = wb.worksheet().unwrap();
        assert_wrapped_grid_error(ws.cell(0, 1));
        let mut cells = ws.cells();
        assert_wrapped_grid_error(cells.next().unwrap());
        assert!(cells.next().is_none());
    }
    assert_eq!(wb.to_bytes().unwrap(), input);
}

#[test]
fn grid_nodes_under_unexpected_parents_refuse_cell_edits() {
    const ROW: &str = r#"<row r="2"><c r="A2"><v>5</v></c></row>"#;
    let worksheets = [
        format!("<sheetData>{PLAIN_ROW}<foo>{ROW}</foo></sheetData>"),
        format!(r#"<sheetData>{PLAIN_ROW}<extLst><ext uri="{{X}}">{ROW}</ext></extLst></sheetData>"#),
        format!(r#"<q:w {WRAPPER_NS} {PROCESS_CONTENT}><sheetData>{PLAIN_ROW}{ROW}</sheetData></q:w>"#),
    ];
    for children in worksheets {
        let input = with_part(
            &book(&[("Sheet1", "")], "", ""),
            "xl/worksheets/sheet1.xml",
            &format!(r#"<worksheet xmlns="{SML}">{children}</worksheet>"#),
        );
        let mut wb = Workbook::from_bytes(input.clone()).unwrap();
        assert_wrapped_grid_error(wb.worksheet().unwrap().write_number(1, 0, 7.0));
        assert_eq!(wb.to_bytes().unwrap(), input);
    }
}

fn with_worksheet_children(children: &str) -> Vec<u8> {
    with_part(
        &book(&[("Sheet1", "")], "", ""),
        "xl/worksheets/sheet1.xml",
        &format!(r#"<worksheet xmlns="{SML}" {WRAPPER_NS} mc:Ignorable="q">{children}</worksheet>"#),
    )
}

fn alternate_content(content: &str) -> String {
    format!(r#"<mc:AlternateContent><mc:Choice Requires="q">{content}</mc:Choice></mc:AlternateContent>"#)
}

#[test]
fn grid_nodes_in_alternate_content_outside_indexed_parents_refuse_edits() {
    let row = alternate_content(r#"<row r="2"><c r="A2"><v>5</v></c></row>"#);
    let cell = alternate_content(r#"<c r="B1"><v>5</v></c>"#);
    let grid = alternate_content(r#"<sheetData><row r="2"><c r="A2"><v>5</v></c></row></sheetData>"#);
    let worksheets = [
        (format!(r#"<sheetData>{PLAIN_ROW}<q:w {PROCESS_CONTENT}>{row}</q:w></sheetData>"#), 1, 0),
        (format!(r#"<sheetData><row r="1"><c r="A1"/><q:w {PROCESS_CONTENT}>{cell}</q:w></row></sheetData>"#), 0, 1),
        (format!(r#"<sheetData>{PLAIN_ROW}</sheetData><q:w {PROCESS_CONTENT}>{grid}</q:w>"#), 1, 0),
        (format!("<sheetData>{PLAIN_ROW}</sheetData>{grid}"), 1, 0),
    ];
    for (children, row, col) in worksheets {
        let input = with_worksheet_children(&children);
        let mut wb = Workbook::from_bytes(input.clone()).unwrap();
        assert_wrapped_grid_error(wb.worksheet().unwrap().write_number(row, col, 7.0));
        assert_eq!(wb.to_bytes().unwrap(), input, "{children}");
    }
}

#[test]
fn alternate_content_with_controls_stays_editable() {
    let controls = alternate_content(r#"<controls><control shapeId="1"/></controls>"#);
    let input = with_worksheet_children(&format!("<sheetData>{PLAIN_ROW}</sheetData>{controls}"));
    let mut wb = Workbook::from_bytes(input).unwrap();
    wb.worksheet().unwrap().write_number(0, 0, 7.0).unwrap();
    let output = wb.to_bytes().unwrap();
    assert_eq!(sheet_xml(&output), sheet_xml(&with_worksheet_children(
        &format!(r#"<sheetData><row r="1"><c r="A1"><v>7</v></c></row></sheetData>{controls}"#),
    )));
}

#[test]
fn duplicate_sheet_data_refuses_cell_edits() {
    let input = with_worksheet_children(&format!(
        r#"<sheetData>{PLAIN_ROW}</sheetData><sheetData><row r="2"><c r="A2"><v>5</v></c></row></sheetData>"#
    ));
    let mut wb = Workbook::from_bytes(input.clone()).unwrap();
    assert_wrapped_grid_error(wb.worksheet().unwrap().write_number(1, 0, 7.0));
    assert_eq!(wb.to_bytes().unwrap(), input);
}
