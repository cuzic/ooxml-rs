mod common;

use std::assert_matches;

use common::{p1::book, sheet_xml};
use ooxml_core::ElementSpec;
use ooxml_schema::sml;
use ooxml_xlsx::{Error, Workbook};

const WRAPPER_NS: &str = r#"xmlns:q="urn:test:extension" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006""#;
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
