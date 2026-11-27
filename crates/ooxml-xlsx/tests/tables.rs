mod common;

use std::io::Cursor;

use calamine::{Data, Reader, Xlsx};
use ooxml_xlsx::{
    CellIndex, CellRange, CellValue, CellWrite, Col, Error, Table, TableStyle, Total, Validation,
    Workbook, Worksheet,
};

fn table(range: &str) -> Table {
    Table::new(CellRange::parse(range).unwrap())
}

fn at(cell: &str) -> CellIndex {
    CellIndex::parse(cell).unwrap()
}

fn refused<T: std::fmt::Debug>(
    wb: &mut Workbook,
    edit: impl FnOnce(&mut Worksheet<'_>) -> ooxml_xlsx::Result<T>,
) -> Error {
    let before = wb.to_bytes().unwrap();
    let err = edit(&mut wb.worksheet().unwrap()).unwrap_err();
    assert_eq!(wb.to_bytes().unwrap(), before);
    err
}

const TABLE1: &str = "xl/tables/table1.xml";

fn table_xml(wb: &Workbook, part: &str) -> String {
    String::from_utf8(common::part(&wb.to_bytes().unwrap(), part)).unwrap()
}

#[test]
fn appending_rows_updates_the_grid_and_both_table_ranges() {
    let mut wb = Workbook::new().unwrap();
    let mut ws = wb.worksheet().unwrap();
    ws.add_table(table("A1:B2").name("Sales")).unwrap();
    ws.write_string(2, 2, "outside").unwrap();
    refused(&mut wb, |ws| ws.append_table_rows("Sales", &[&[CellWrite::Number(3.0)]]));
    refused(&mut wb, |ws| ws.append_table_rows("Sales", &[
        &[CellWrite::Number(3.0), CellWrite::Bool(true)],
        &[CellWrite::Number(f64::NAN), CellWrite::String("new")],
    ]));
    wb.worksheet().unwrap().append_table_rows("Sales", &[
        &[CellWrite::Number(3.0), CellWrite::Bool(true)],
        &[CellWrite::Formula("=A3+1"), CellWrite::String("new")],
    ]).unwrap();
    let xml = table_xml(&wb, TABLE1);
    assert_eq!(xml.matches(r#"ref="A1:B4""#).count(), 2, "{xml}");
    refused(&mut wb, |ws| ws.add_table(table("A3:A4").name("Overlap")));
    let mut reopened = Workbook::from_bytes(wb.to_bytes().unwrap()).unwrap();
    let ws = reopened.worksheet().unwrap();
    assert_eq!(ws.cell(2, 0).unwrap().unwrap().value().unwrap(), CellValue::Number(3.0));
    assert_eq!(ws.cell(2, 1).unwrap().unwrap().value().unwrap(), CellValue::Bool(true));
    assert_eq!(ws.cell(3, 0).unwrap().unwrap().formula().as_deref(), Some("A3+1"));
    assert_eq!(ws.cell(3, 1).unwrap().unwrap().value().unwrap(), CellValue::String("new".into()));
    assert_eq!(ws.cell(2, 2).unwrap().unwrap().value().unwrap(), CellValue::String("outside".into()));
}

#[test]
fn removing_table_tail_rows_clears_only_table_cells_and_shrinks_the_filter() {
    use ooxml_xlsx::{Link, LinkOptions};
    let mut wb = Workbook::new().unwrap();
    let mut ws = wb.worksheet().unwrap();
    ws.add_table(table("A1:B4").name("Sales")).unwrap();
    ws.write_number(2, 0, 10.0).unwrap();
    ws.write_string(3, 1, "removed").unwrap();
    ws.write_number(3, 2, 30.0).unwrap();
    ws.set_hyperlink("C4", &Link::Location("Sheet1!A2".into()), &LinkOptions::default()).unwrap();
    ws.set_hyperlink("A2:A3", &Link::Location("Sheet1!C4".into()), &LinkOptions::default()).unwrap();
    ws.set_hyperlink("B2:B5", &Link::Location("Sheet1!A2".into()), &LinkOptions::default()).unwrap();
    refused(&mut wb, |ws| ws.remove_table_tail_rows("Sales", 2));
    let mut ws = wb.worksheet().unwrap();
    ws.remove_hyperlink("B2:B5").unwrap();
    ws.set_hyperlink("B3:B4", &Link::Location("Sheet1!C4".into()), &LinkOptions::default()).unwrap();
    refused(&mut wb, |ws| ws.remove_table_tail_rows("Sales", 4));
    wb.worksheet().unwrap().remove_table_tail_rows("Sales", 2).unwrap();
    let xml = table_xml(&wb, TABLE1);
    assert_eq!(xml.matches(r#"ref="A1:B2""#).count(), 2, "{xml}");
    let mut reopened = Workbook::from_bytes(wb.to_bytes().unwrap()).unwrap();
    let mut ws = reopened.worksheet().unwrap();
    let links = ws.hyperlinks().unwrap();
    assert_eq!(links.iter().map(|link| link.range.as_str()).collect::<Vec<_>>(), ["C4", "A2"]);
    assert_eq!(links[1].location.as_deref(), Some("Sheet1!C4"));
    assert_eq!(ws.cell(2, 0).unwrap().unwrap().value().unwrap(), CellValue::Empty);
    assert_eq!(ws.cell(3, 1).unwrap().unwrap().value().unwrap(), CellValue::Empty);
    assert_eq!(ws.cell(3, 2).unwrap().unwrap().value().unwrap(), CellValue::Number(30.0));
}

#[test]
fn patching_table_column_styles_preserves_omitted_formats_and_values() {
    use ooxml_xlsx::Format;
    let mut wb = Workbook::new().unwrap();
    let mut ws = wb.worksheet().unwrap();
    ws.add_table(table("A1:B3").name("Sales")).unwrap();
    ws.cell_mut(1, 0).unwrap().set_format(&Format::new().set_bold()).unwrap();
    ws.cell_mut(1, 0).unwrap().set_formula_with_result("1+2", CellValue::Number(3.0)).unwrap();
    ws.cell_mut(1, 1).unwrap().set_num_format("0.0").unwrap();
    refused(&mut wb, |ws| ws.patch_table_column_style("Sales", 2, Format::new().set_num_format("0.00")));
    refused(&mut wb, |ws| ws.patch_table_column_styles("Sales", vec![
        (0, Format::new().set_num_format("0.00")), (2, Format::new().set_num_format("0.0000")),
    ]));
    wb.worksheet().unwrap().patch_table_column_styles("Sales", vec![
        (0, Format::new().set_num_format("0.00")), (1, Format::new().set_bold()),
    ]).unwrap();
    let mut reopened = Workbook::from_bytes(wb.to_bytes().unwrap()).unwrap();
    let mut ws = reopened.worksheet().unwrap();
    for row in 0..3 {
        assert_eq!(ws.number_format(row, 0).unwrap().as_deref(), Some("0.00"));
    }
    assert!(ws.cell_font(1, 0).unwrap().unwrap().bold);
    assert_eq!(ws.number_format(1, 1).unwrap().as_deref(), Some("0.0"));
    assert!(ws.cell_font(1, 1).unwrap().unwrap().bold);
    assert_eq!(ws.cell_a1("A2").unwrap().unwrap().value().unwrap(), CellValue::Number(3.0));
}

#[test]
fn changing_a_column_total_updates_only_its_definition_and_cell() {
    use ooxml_xlsx::ColumnTotal;
    let mut wb = Workbook::new().unwrap();
    let mut ws = wb.worksheet().unwrap();
    ws.add_table(table("A1:B3").name("Sales").totals_row(true)
        .columns([Col::new("Name").total_label("Total"), Col::new("Amount")])).unwrap();
    ws.write_number(1, 1, 12.0).unwrap();
    ws.cell_mut(2, 1).unwrap().set_num_format("0.00").unwrap();
    refused(&mut wb, |ws| ws.set_table_column_total("Sales", 1, Some(ColumnTotal::Formula("[@Missing]".into()))));
    let mut ws = wb.worksheet().unwrap();
    ws.set_table_column_total("Sales", 1, Some(ColumnTotal::Function(Total::Sum))).unwrap();
    assert_eq!(ws.cell_a1("B3").unwrap().unwrap().formula().as_deref(), Some("SUBTOTAL(109,Sales[Amount])"));
    ws.set_table_column_total_with_result("Sales", 1, Some(ColumnTotal::Formula("SUM(B2:B2)".into())),
        CellValue::Number(12.0)).unwrap();
    assert_eq!(ws.cell_a1("B3").unwrap().unwrap().value().unwrap(), CellValue::Number(12.0));
    assert!(table_xml(&wb, TABLE1).contains("<totalsRowFormula>SUM(B2:B2)</totalsRowFormula>"));
    let mut ws = wb.worksheet().unwrap();
    ws.set_table_column_total("Sales", 1, Some(ColumnTotal::Label("Replaced".into()))).unwrap();
    assert!(table_xml(&wb, TABLE1).contains(r#"totalsRowLabel="Replaced""#));
    wb.worksheet().unwrap().set_table_column_total("Sales", 1, None).unwrap();
    let xml = table_xml(&wb, TABLE1);
    assert!(!xml.contains("Replaced") && !xml.contains("totalsRowFunction") && !xml.contains("totalsRowFormula"));
    let mut reopened = Workbook::from_bytes(wb.to_bytes().unwrap()).unwrap();
    let mut ws = reopened.worksheet().unwrap();
    assert_eq!(ws.cell_a1("B3").unwrap().unwrap().value().unwrap(), CellValue::Empty);
    assert_eq!(ws.cell_a1("A3").unwrap().unwrap().value().unwrap(), CellValue::String("Total".into()));
    assert_eq!(ws.cell_a1("B2").unwrap().unwrap().value().unwrap(), CellValue::Number(12.0));
    assert_eq!(ws.number_format(2, 1).unwrap().as_deref(), Some("0.00"));
}

#[test]
fn replacing_all_table_columns_writes_only_the_final_footprint() {
    let mut wb = Workbook::new().unwrap();
    let mut ws = wb.worksheet().unwrap();
    ws.add_table(table("A1:B3").name("Sales").columns([
        Col::new("Same").filter_button(false), Col::new("Old").filter_button(false),
    ])).unwrap();
    ws.write_number(1, 2, 99.0).unwrap();
    refused(&mut wb, |ws| ws.replace_table_columns("Sales", vec![
        (Col::new("Duplicate"), &[]), (Col::new("duplicate"), &[]),
    ]));
    wb.worksheet().unwrap().replace_table_columns("Sales", vec![
        (Col::new("Same"), &[CellWrite::Number(7.0)]),
        (Col::new("New").filter_button(false), &[CellWrite::Bool(true)]),
    ]).unwrap();
    let xml = table_xml(&wb, TABLE1);
    assert!(xml.contains(r#"<tableColumn id="3" name="Same""#));
    assert!(xml.contains(r#"<tableColumn id="4" name="New""#));
    assert_eq!(xml.matches("<tableColumn ").count(), 2);
    assert_eq!(xml.matches("<filterColumn ").count(), 1);
    assert!(xml.contains(r#"<filterColumn colId="1" hiddenButton="1""#));
    let mut reopened = Workbook::from_bytes(wb.to_bytes().unwrap()).unwrap();
    let ws = reopened.worksheet().unwrap();
    assert_eq!(ws.cell_a1("A2").unwrap().unwrap().value().unwrap(), CellValue::Number(7.0));
    assert_eq!(ws.cell_a1("B2").unwrap().unwrap().value().unwrap(), CellValue::Bool(true));
    assert_eq!(ws.cell_a1("C2").unwrap().unwrap().value().unwrap(), CellValue::Number(99.0));
}

#[test]
fn inserting_an_interior_table_column_preserves_column_ids_and_moves_cells() {
    use ooxml_xlsx::{Link, LinkOptions};
    let mut wb = Workbook::new().unwrap();
    let mut ws = wb.worksheet().unwrap();
    ws.add_table(table("A1:B3").name("Sales").columns([
        Col::new("First"), Col::new("Shifted").filter_button(false),
    ])).unwrap();
    ws.cell_mut(1, 1).unwrap().set_formula_with_result("2+3", CellValue::Number(5.0)).unwrap();
    ws.cell_mut(1, 1).unwrap().set_num_format("0.0000").unwrap();
    ws.set_hyperlink("B3", &Link::Url("https://example.org/moved".into()), &LinkOptions::default()).unwrap();
    ws.write_string(1, 3, "outside").unwrap();
    let values = [CellWrite::Bool(true), CellWrite::String("new")];
    refused(&mut wb, |ws| ws.insert_table_column("Sales", 3, Col::new("Added"), &values));
    wb.worksheet().unwrap().insert_table_column("Sales", 1, Col::new("Added"), &values).unwrap();
    let xml = table_xml(&wb, TABLE1);
    assert!(xml.contains(r#"<autoFilter ref="A1:C3""#));
    assert!(xml.contains(r#"<filterColumn colId="2" hiddenButton="1""#));
    assert!(xml.find(r#"<tableColumn id="3" name="Added""#).unwrap() <
        xml.find(r#"<tableColumn id="2" name="Shifted""#).unwrap());
    let mut reopened = Workbook::from_bytes(wb.to_bytes().unwrap()).unwrap();
    let mut ws = reopened.worksheet().unwrap();
    assert_eq!(ws.cell_a1("B1").unwrap().unwrap().value().unwrap(), CellValue::String("Added".into()));
    assert_eq!(ws.cell_a1("B2").unwrap().unwrap().value().unwrap(), CellValue::Bool(true));
    let formula = ws.cell_a1("C2").unwrap().unwrap();
    assert_eq!(formula.ordinary_formula().as_deref(), Some("2+3"));
    assert_eq!(formula.value().unwrap(), CellValue::Number(5.0));
    assert_eq!(ws.number_format(1, 2).unwrap().as_deref(), Some("0.0000"));
    assert_eq!(ws.hyperlinks().unwrap()[0].range, "C3");
    assert_eq!(ws.cell_a1("D2").unwrap().unwrap().value().unwrap(), CellValue::String("outside".into()));
}

#[test]
fn removing_an_interior_table_column_shifts_cells_and_filter_positions() {
    use ooxml_xlsx::{Link, LinkOptions};
    let mut wb = Workbook::new().unwrap();
    let mut ws = wb.worksheet().unwrap();
    ws.add_table(table("A1:C3").name("Sales").columns([
        Col::new("First"), Col::new("Removed").filter_button(false), Col::new("Moved").filter_button(false),
    ])).unwrap();
    ws.write_string(1, 1, "discarded").unwrap();
    ws.cell_mut(1, 2).unwrap().set_formula_with_result("1+2", CellValue::Number(3.0)).unwrap();
    ws.cell_mut(1, 2).unwrap().set_num_format("0.0000").unwrap();
    ws.write_string(2, 2, "link").unwrap();
    ws.set_hyperlink("C3", &Link::Url("https://example.org/moved".into()), &LinkOptions::default()).unwrap();
    ws.write_string(1, 3, "outside").unwrap();
    ws.write_formula(1, 2, "SUM(Sales[Removed])").unwrap();
    assert!(matches!(refused(&mut wb, |ws| ws.remove_table_columns("Sales", 1, 1)), Error::TableInUse(_)));
    wb.worksheet().unwrap().cell_mut(1, 2).unwrap().set_formula_with_result("1+2", CellValue::Number(3.0)).unwrap();
    refused(&mut wb, |ws| ws.remove_table_columns("Sales", 2, 2));
    wb.worksheet().unwrap().remove_table_columns("Sales", 1, 1).unwrap();
    let xml = table_xml(&wb, TABLE1);
    assert!(xml.contains(r#"<autoFilter ref="A1:B3""#));
    assert_eq!(xml.matches(r#"<filterColumn colId="1""#).count(), 1);
    assert!(!xml.contains(r#"colId="2""#));
    assert!(xml.contains(r#"<tableColumn id="3" name="Moved""#));
    assert!(!xml.contains("Removed"));
    let mut reopened = Workbook::from_bytes(wb.to_bytes().unwrap()).unwrap();
    let mut ws = reopened.worksheet().unwrap();
    assert_eq!(ws.cell_a1("B1").unwrap().unwrap().value().unwrap(), CellValue::String("Moved".into()));
    let formula = ws.cell_a1("B2").unwrap().unwrap();
    assert_eq!(formula.ordinary_formula().as_deref(), Some("1+2"));
    assert_eq!(formula.value().unwrap(), CellValue::Number(3.0));
    assert_eq!(ws.number_format(1, 1).unwrap().as_deref(), Some("0.0000"));
    assert_eq!(ws.hyperlinks().unwrap()[0].range, "B3");
    assert_eq!(ws.cell_a1("C3").unwrap().unwrap().value().unwrap(), CellValue::Empty);
    assert_eq!(ws.cell_a1("D2").unwrap().unwrap().value().unwrap(), CellValue::String("outside".into()));
}

#[test]
fn removing_table_tail_columns_keeps_surviving_ids_and_filters() {
    let mut wb = Workbook::new().unwrap();
    let mut ws = wb.worksheet().unwrap();
    ws.add_table(table("A1:C3").name("Sales").columns([
        Col::new("First").filter_button(false),
        Col::new("Second").filter_button(false),
        Col::new("Third").filter_button(false),
    ])).unwrap();
    ws.write_number(1, 0, 11.0).unwrap();
    ws.write_number(1, 1, 22.0).unwrap();
    ws.write_number(2, 2, 33.0).unwrap();
    ws.write_number(1, 3, 44.0).unwrap();
    refused(&mut wb, |ws| ws.remove_table_tail_columns("Sales", 3));
    wb.worksheet().unwrap().remove_table_tail_columns("Sales", 2).unwrap();
    let xml = table_xml(&wb, TABLE1);
    assert!(xml.contains(r#"<autoFilter ref="A1:A3""#));
    assert!(xml.contains(r#"<filterColumn colId="0" hiddenButton="1""#));
    assert!(!xml.contains(r#"colId="1""#) && !xml.contains(r#"colId="2""#));
    assert!(xml.contains(r#"<tableColumn id="1" name="First""#));
    assert!(!xml.contains("Second") && !xml.contains("Third"));
    let mut reopened = Workbook::from_bytes(wb.to_bytes().unwrap()).unwrap();
    let mut ws = reopened.worksheet().unwrap();
    for reference in ["B1", "B2", "C1", "C3"] {
        assert_eq!(ws.cell_a1(reference).unwrap().unwrap().value().unwrap(), CellValue::Empty);
    }
    assert_eq!(ws.cell_a1("A2").unwrap().unwrap().value().unwrap(), CellValue::Number(11.0));
    assert_eq!(ws.cell_a1("D2").unwrap().unwrap().value().unwrap(), CellValue::Number(44.0));
    ws.write_number(0, 1, 55.0).unwrap();
}

#[test]
fn appending_a_table_column_preserves_ids_and_grows_the_filter() {
    let mut wb = Workbook::new().unwrap();
    let mut ws = wb.worksheet().unwrap();
    ws.add_table(table("A1:B4").name("Sales").totals_row(true).columns([
        Col::new("Label").total_label("Total"),
        Col::new("Existing"),
    ])).unwrap();
    ws.write_number(1, 1, 10.0).unwrap();
    ws.write_string(2, 3, "outside").unwrap();
    refused(&mut wb, |ws| ws.append_table_column("Sales", Col::new("existing"), &[]));
    wb.worksheet().unwrap().append_table_column(
        "Sales", Col::new("Amount").total(Total::Sum).filter_button(false),
        &[CellWrite::Number(5.0), CellWrite::Number(6.0)],
    ).unwrap();
    let xml = table_xml(&wb, TABLE1);
    assert!(xml.contains(r#"<autoFilter ref="A1:C3""#));
    assert!(xml.contains(r#"<filterColumn colId="2" hiddenButton="1""#));
    assert!(xml.contains(r#"<tableColumns count="3""#));
    assert!(xml.contains(r#"<tableColumn id="2" name="Existing""#));
    assert!(xml.contains(r#"<tableColumn id="3" name="Amount""#));
    let mut reopened = Workbook::from_bytes(wb.to_bytes().unwrap()).unwrap();
    let ws = reopened.worksheet().unwrap();
    assert_eq!(ws.cell_a1("C1").unwrap().unwrap().value().unwrap(), CellValue::String("Amount".into()));
    assert_eq!(ws.cell_a1("C2").unwrap().unwrap().value().unwrap(), CellValue::Number(5.0));
    assert_eq!(ws.cell_a1("C4").unwrap().unwrap().formula().as_deref(), Some("SUBTOTAL(109,Sales[Amount])"));
    assert_eq!(ws.cell_a1("D3").unwrap().unwrap().value().unwrap(), CellValue::String("outside".into()));
}

#[test]
fn editing_table_rows_moves_the_total_and_excludes_it_from_the_filter() {
    let mut wb = Workbook::new().unwrap();
    let mut ws = wb.worksheet().unwrap();
    ws.add_table(table("A1:B3").name("Sales").totals_row(true).columns([
        Col::new("Label").total_label("Total"),
        Col::new("Amount").total(Total::Sum),
    ])).unwrap();
    ws.write_number(1, 1, 5.0).unwrap();
    ws.cell_mut(2, 1).unwrap().set_formula_with_result("SUBTOTAL(109,Sales[Amount])", CellValue::Number(5.0)).unwrap();
    ws.append_table_rows("Sales", &[&[CellWrite::String("added"), CellWrite::Number(10.0)]]).unwrap();
    assert_eq!(ws.cell_a1("A4").unwrap().unwrap().value().unwrap(), CellValue::String("Total".into()));
    assert_eq!(ws.cell_a1("B4").unwrap().unwrap().formula().as_deref(), Some("SUBTOTAL(109,Sales[Amount])"));
    assert_eq!(ws.cell_a1("B4").unwrap().unwrap().value().unwrap(), CellValue::Empty);
    assert!(table_xml(&wb, TABLE1).contains(r#"<autoFilter ref="A1:B3""#));
    wb.worksheet().unwrap().remove_table_rows("Sales", 0, 1).unwrap();
    let xml = table_xml(&wb, TABLE1);
    assert!(xml.contains(r#"<autoFilter ref="A1:B2""#));
    let mut reopened = Workbook::from_bytes(wb.to_bytes().unwrap()).unwrap();
    let ws = reopened.worksheet().unwrap();
    assert_eq!(ws.cell_a1("B2").unwrap().unwrap().value().unwrap(), CellValue::Number(10.0));
    assert_eq!(ws.cell_a1("A3").unwrap().unwrap().value().unwrap(), CellValue::String("Total".into()));
    assert_eq!(ws.cell_a1("B3").unwrap().unwrap().formula().as_deref(), Some("SUBTOTAL(109,Sales[Amount])"));
    assert_eq!(ws.cell_a1("B4").unwrap().unwrap().value().unwrap(), CellValue::Empty);
}

#[test]
fn inserting_table_rows_moves_existing_cells_and_extends_the_filter() {
    use ooxml_xlsx::{Link, LinkOptions};
    let mut wb = Workbook::new().unwrap();
    let mut ws = wb.worksheet().unwrap();
    ws.add_table(table("A1:B3").name("Sales")).unwrap();
    ws.write_string(1, 0, "first").unwrap();
    ws.cell_mut(2, 0).unwrap().set_formula_with_result("1+3", CellValue::Number(4.0)).unwrap();
    ws.cell_mut(2, 0).unwrap().set_num_format("0.0000").unwrap();
    ws.write_number(2, 2, 30.0).unwrap();
    let link = Link::Url("https://example.org/preserved".into());
    ws.set_hyperlink("A3:B3", &link, &LinkOptions::default()).unwrap();
    ws.set_hyperlink("B4", &link, &LinkOptions::default()).unwrap();
    let rows: &[&[CellWrite<'_>]] = &[&[CellWrite::String("inserted"), CellWrite::Bool(true)]];
    refused(&mut wb, |ws| ws.insert_table_rows("Sales", 1, rows));
    wb.worksheet().unwrap().remove_hyperlink("B4").unwrap();
    refused(&mut wb, |ws| ws.insert_table_rows("Sales", 3, rows));
    wb.worksheet().unwrap().insert_table_rows("Sales", 1, rows).unwrap();
    let xml = table_xml(&wb, TABLE1);
    assert_eq!(xml.matches(r#"ref="A1:B4""#).count(), 2, "{xml}");
    let mut reopened = Workbook::from_bytes(wb.to_bytes().unwrap()).unwrap();
    let mut ws = reopened.worksheet().unwrap();
    let links = ws.hyperlinks().unwrap();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].range, "A4:B4");
    assert_eq!(links[0].target.as_deref(), Some("https://example.org/preserved"));
    assert_eq!(ws.cell_a1("A2").unwrap().unwrap().value().unwrap(), CellValue::String("first".into()));
    assert_eq!(ws.cell_a1("A3").unwrap().unwrap().value().unwrap(), CellValue::String("inserted".into()));
    assert_eq!(ws.cell_a1("B3").unwrap().unwrap().value().unwrap(), CellValue::Bool(true));
    let formula = ws.cell_a1("A4").unwrap().unwrap();
    assert_eq!(formula.ordinary_formula().as_deref(), Some("1+3"));
    assert_eq!(formula.value().unwrap(), CellValue::Number(4.0));
    assert_eq!(ws.number_format(3, 0).unwrap().as_deref(), Some("0.0000"));
    assert_eq!(ws.cell_a1("C3").unwrap().unwrap().value().unwrap(), CellValue::Number(30.0));
}

#[test]
fn table_interior_removal_shifts_values_inside_the_table() {
    use ooxml_xlsx::{Link, LinkOptions};
    let mut wb = Workbook::new().unwrap();
    let mut ws = wb.worksheet().unwrap();
    ws.add_table(table("A1:B5").name("Sales")).unwrap();
    ws.write_string(1, 0, "first").unwrap();
    ws.write_string(2, 0, "remove").unwrap();
    ws.write_string(2, 1, "clear").unwrap();
    ws.cell_mut(3, 0).unwrap().set_iso_date("2025-06-01T05:04:03.012Z").unwrap();
    ws.cell_mut(3, 0).unwrap().set_num_format("0.0000").unwrap();
    ws.cell_mut(2, 1).unwrap().set_num_format("0.000").unwrap();
    ws.write_number(3, 2, 10.0).unwrap();
    ws.write_number(4, 1, 20.0).unwrap();
    ws.cell_mut(4, 0).unwrap().set_formula_with_result("C4+1", CellValue::Number(11.0)).unwrap();
    let target = Link::Url("https://example.org/moved".into());
    ws.set_hyperlink("A3:A4", &target, &LinkOptions::default()).unwrap();
    refused(&mut wb, |ws| ws.remove_table_rows("Sales", 1, 1));
    let mut ws = wb.worksheet().unwrap();
    ws.remove_hyperlink("A3:A4").unwrap();
    ws.set_hyperlink("A3", &Link::Location("Sheet1!A1".into()), &LinkOptions::default()).unwrap();
    ws.set_hyperlink("A4:B4", &target, &LinkOptions::default()).unwrap();
    refused(&mut wb, |ws| ws.remove_table_rows("Sales", 3, 2));
    wb.worksheet().unwrap().remove_table_rows("Sales", 1, 1).unwrap();
    let xml = table_xml(&wb, TABLE1);
    assert_eq!(xml.matches(r#"ref="A1:B4""#).count(), 2, "{xml}");
    let mut reopened = Workbook::from_bytes(wb.to_bytes().unwrap()).unwrap();
    let mut ws = reopened.worksheet().unwrap();
    let links = ws.hyperlinks().unwrap();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].range, "A3:B3");
    assert_eq!(links[0].target.as_deref(), Some("https://example.org/moved"));
    assert_eq!(ws.cell_a1("A2").unwrap().unwrap().value().unwrap(), CellValue::String("first".into()));
    assert_eq!(ws.cell_a1("A3").unwrap().unwrap().value().unwrap(), CellValue::Date("2025-06-01T05:04:03.012Z".into()));
    assert_eq!(ws.number_format(2, 0).unwrap().as_deref(), Some("0.0000"));
    assert_eq!(ws.cell_a1("B3").unwrap().unwrap().value().unwrap(), CellValue::Empty);
    assert_eq!(ws.cell_a1("B3").unwrap().unwrap().style_index().unwrap(), 0);
    assert_eq!(ws.cell_a1("B4").unwrap().unwrap().value().unwrap(), CellValue::Number(20.0));
    let formula = ws.cell_a1("A4").unwrap().unwrap();
    assert_eq!(formula.ordinary_formula().as_deref(), Some("C4+1"));
    assert_eq!(formula.value().unwrap(), CellValue::Number(11.0));
    assert_eq!(ws.cell_a1("B5").unwrap().unwrap().value().unwrap(), CellValue::Empty);
    assert_eq!(ws.cell_a1("C4").unwrap().unwrap().value().unwrap(), CellValue::Number(10.0));
}

#[test]
fn table_tail_removal_clips_a_link_crossing_the_table_side() {
    use ooxml_xlsx::{Link, LinkOptions};
    let mut wb = Workbook::new().unwrap();
    let mut ws = wb.worksheet().unwrap();
    ws.add_table(table("A1:B4").name("Sales")).unwrap();
    ws.set_hyperlink("B3:C4", &Link::Url("https://example.org/range".into()), &LinkOptions::default()).unwrap();
    ws.remove_table_tail_rows("Sales", 2).unwrap();
    let mut reopened = Workbook::from_bytes(wb.to_bytes().unwrap()).unwrap();
    let links = reopened.worksheet().unwrap().hyperlinks().unwrap();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].range, "C3:C4");
    assert_eq!(links[0].target.as_deref(), Some("https://example.org/range"));
}

#[test]
fn filter_button_batch_is_atomic_and_local() {
    let mut wb = Workbook::new().unwrap();
    wb.worksheet().unwrap().add_table(table("A1:B2").name("Sales")).unwrap();
    refused(&mut wb, |ws| ws.set_table_filter_buttons("Sales", &[(0, false), (2, false)]));
    let before = wb.to_bytes().unwrap();
    wb.worksheet().unwrap().set_table_filter_buttons("Sales", &[(0, false), (1, false)]).unwrap();
    common::assert_l1_except(&before, &wb.to_bytes().unwrap(), &[TABLE1]);
    let xml = table_xml(&wb, TABLE1);
    assert!(xml.contains(r#"colId="0" hiddenButton="1""#));
    assert!(xml.contains(r#"colId="1" hiddenButton="1""#));
}

#[test]
fn create_sync_and_read_back() {
    let mut wb = Workbook::new().unwrap();
    let before = wb.to_bytes().unwrap();
    {
        let mut ws = wb.worksheet().unwrap();
        ws.write_number(1, 1, 2.0).unwrap();
        ws.write_number(2, 1, 5.0).unwrap();
        assert_eq!(
            ws.add_table(
                table("A1:C4")
                    .name("Sales")
                    .columns([
                        Col::new("Region").total_label("Total"),
                        Col::new("Units, shipped").total(Total::Sum),
                        Col::new("Double").formula(r#"[@[Units, shipped]]*2+IF("a@b"="a@b",0,1)"#),
                    ])
                    .totals_row(true)
            )
            .unwrap(),
            "Sales"
        );
    }
    let original_xml = table_xml(&wb, TABLE1);
    assert!(original_xml.contains(r#"<autoFilter ref="A1:C3""#));
    assert!(original_xml.contains(r#"totalsRowFunction="sum""#));
    assert!(original_xml.contains(r#"TableStyleMedium9"#));
    let after_add = wb.to_bytes().unwrap();
    for original in common::entries(&before) {
        if ![
            "[Content_Types].xml",
            wb.worksheet_part(),
            wb.workbook_part(),
            "xl/sharedStrings.xml",
        ]
        .contains(&original.0.as_str())
        {
            assert!(
                common::entries(&after_add).contains(&original),
                "changed: {}",
                original.0
            );
        }
    }
    wb.worksheet()
        .unwrap()
        .write_string(0, 0, "Market")
        .unwrap();
    let bytes = wb.to_bytes().unwrap();
    let sheet = wb.worksheet_part();
    common::assert_l1_except(&after_add, &bytes, &[sheet, TABLE1, "xl/sharedStrings.xml"]);
    common::assert_t2(
        &common::part(&after_add, sheet),
        &common::part(&bytes, sheet),
        at("A1"),
    );
    assert_eq!(
        table_xml(&wb, TABLE1),
        original_xml.replacen(r#"name="Region""#, r#"name="Market""#, 1)
    );

    let mut xlsx = Xlsx::new(Cursor::new(bytes)).unwrap();
    xlsx.load_tables().unwrap();
    let sales = xlsx.table_by_name("Sales").unwrap();
    assert_eq!(sales.columns(), &["Market", "Units, shipped", "Double"]);
    assert_eq!(sales.data().start(), Some((1, 0)));
    assert_eq!(sales.data().end(), Some((2, 2)));
    assert_eq!(sales.data().get_value((2, 1)), Some(&Data::Float(5.0)));
    let values = xlsx.worksheet_range(sales.sheet_name()).unwrap();
    assert_eq!(
        values.get_value((3, 0)),
        Some(&Data::String("Total".into()))
    );
    let formulas = xlsx.worksheet_formula(sales.sheet_name()).unwrap();
    assert_eq!(
        formulas.get_value((3, 1)).unwrap(),
        "SUBTOTAL(109,Sales[[Units, shipped]])"
    );
    assert_eq!(
        formulas.get_value((1, 2)).unwrap(),
        r#"Sales[[#This Row],[Units, shipped]]*2+IF("a@b"="a@b",0,1)"#
    );
}

#[test]
fn removal_and_id_high_water() {
    let mut wb = Workbook::new().unwrap();
    let mut ws = wb.worksheet().unwrap();
    ws.write_string(0, 0, "Old").unwrap();
    ws.add_table(table("A1:A2").name("First")).unwrap();
    ws.add_table(table("C1:C2").name("Second")).unwrap();
    ws.remove_table("SECOND").unwrap();
    assert_eq!(
        ws.cell_a1("C1").unwrap().unwrap().value().unwrap(),
        CellValue::String("Column1".into())
    );
    ws.write_number(0, 2, 10.0).unwrap();
    assert_eq!(ws.add_table(table("E1:E2")).unwrap(), "Table1");
    let bytes = wb.to_bytes().unwrap();
    assert!(
        String::from_utf8(common::part(&bytes, "xl/tables/table3.xml"))
            .unwrap()
            .contains(r#"id="3""#)
    );
    let mut wb = Workbook::from_bytes(bytes).unwrap();
    let mut ws = wb.worksheet().unwrap();
    ws.remove_table("First").unwrap();
    ws.remove_table("Table1").unwrap();
    assert_eq!(
        ws.cell_a1("A1").unwrap().unwrap().value().unwrap(),
        CellValue::String("Old".into())
    );
    let bytes = wb.to_bytes().unwrap();
    assert!(
        !String::from_utf8(common::part(&bytes, wb.worksheet_part()))
            .unwrap()
            .contains("tableParts")
    );
    assert!(
        !common::entries(&bytes)
            .iter()
            .any(|e| e.0.starts_with("xl/tables/"))
    );
}

#[test]
fn table_differential_formats_use_workbook_style_indices() {
    let mut wb = Workbook::new().unwrap();
    let id = wb.add_differential_format(&ooxml_xlsx::DxfFormat::new().set_bold()).unwrap();
    let second = wb.add_differential_format(&ooxml_xlsx::DxfFormat::new().set_font_color(
        ooxml_xlsx::Color::Rgb(0xff123456),
    )).unwrap();
    let ids = wb.differential_format_nodes(&[second, id, second, u32::MAX]).unwrap();
    assert_eq!(ids.iter().map(|(id, _)| *id).collect::<Vec<_>>(), [id, second]);
    assert_eq!(wb.differential_format_nodes(&[id, id]).unwrap().len(), 1);
    assert!(wb.differential_format_nodes(&[]).unwrap().is_empty());
    wb.worksheet().unwrap().add_table(
        table("A1:B3")
            .border_formats([None, Some(id), None])
            .columns([
                Col::new("a").differential_formats([Some(id), None, None]),
                Col::new("b"),
            ]),
    ).unwrap();
    let xml = table_xml(&wb, TABLE1);
    assert!(xml.contains(&format!(r#"tableBorderDxfId="{id}""#)));
    assert!(xml.contains(&format!(r#"headerRowDxfId="{id}""#)));
}

#[test]
fn creation_refusals_are_byte_identical() {
    let mut wb = Workbook::new().unwrap();
    wb.worksheet()
        .unwrap()
        .add_table(table("A1:B3").name("Sales"))
        .unwrap();
    for name in [
        "A1",
        "A0001",
        "RC",
        "R1C1",
        "R5Suffix",
        "\\a",
        "false",
        "_xlnm.X",
        "two words",
        "#bad",
        "",
    ] {
        std::assert_matches!(
            refused(&mut wb, |ws| ws.add_table(table("D1:E3").name(name))),
            Error::InvalidTableName { .. }
        );
    }
    std::assert_matches!(
        refused(&mut wb, |ws| ws.add_table(table("D1:E3").name("sALES"))),
        Error::NameInUse { .. }
    );
    std::assert_matches!(
        refused(&mut wb, |ws| ws.add_table(
            table("D1:E3").columns([Col::new("a"), Col::new("A")])
        )),
        Error::DuplicateColumnName(_)
    );
    for t in [
        table("B2:C4"),
        table("D1:E3").border_formats([None, Some(0), None]),
        table("D1:E3").columns([
            Col::new("a").differential_formats([Some(0), None, None]), Col::new("b"),
        ]),
        table("D1:E1").totals_row(true),
        table("D1:E3").columns([Col::new("a")]),
        table("D1:E3").columns([Col::new("a").total(Total::Sum), Col::new("b")]),
        table("D1:E3").columns([Col::new("a\u{1}"), Col::new("b")]),
    ] {
        std::assert_matches!(
            refused(&mut wb, |ws| ws.add_table(t)),
            Error::TableRange { .. }
        );
    }
    std::assert_matches!(
        refused(&mut wb, |ws| ws
            .add_table(table("D1:E3").style(TableStyle::Dark(42)))),
        Error::InvalidSetting { .. }
    );
    wb.worksheet().unwrap().write_number(0, 3, 42.0).unwrap();
    std::assert_matches!(
        refused(&mut wb, |ws| ws.add_table(table("D1:E3"))),
        Error::HeaderNotText(_)
    );
    std::assert_matches!(
        refused(&mut wb, |ws| ws.add_table(
            table("D1:E3").columns([Col::new("D"), Col::new("E")])
        )),
        Error::TableRange {
            reason: "cell not empty"
        }
    );
    wb.worksheet()
        .unwrap()
        .add_table(
            table("D1:E3")
                .columns([Col::new("D"), Col::new("E")])
                .overwrite(true),
        )
        .unwrap();
    wb.worksheet().unwrap().merge("G1:H1").unwrap();
    std::assert_matches!(
        refused(&mut wb, |ws| ws.add_table(table("G1:H3"))),
        Error::TableRange { .. }
    );
    wb.worksheet().unwrap().set_autofilter("J1:K2").unwrap();
    std::assert_matches!(
        refused(&mut wb, |ws| ws.add_table(table("J1:K3"))),
        Error::TableRange { .. }
    );
}

#[test]
fn header_reference_and_removal_refusals() {
    for (cell, formula) in [
        ("E1", "Sales[sales]"),
        ("E1", "Sales[[#Totals],[Sales]]"),
        ("E1", "Sales[[Sales]:[Region]]"),
        ("B2", "[@[Sales]]"),
        ("B2", "[Sales]"),
        ("E1", "Other[[#Unknown],[Sales]]"),
    ] {
        let mut wb = Workbook::new().unwrap();
        let mut ws = wb.worksheet().unwrap();
        ws.add_table(
            table("A1:B3")
                .name("Sales")
                .columns([Col::new("Sales"), Col::new("Region")]),
        )
        .unwrap();
        let target = at(cell);
        ws.cell_mut(target.row, target.col)
            .unwrap()
            .set_formula(formula)
            .unwrap();
        std::assert_matches!(
            refused(&mut wb, |ws| ws.write_string(0, 0, "Revenue")),
            Error::ColumnInUse(_)
        );
        if !formula.starts_with("Other") {
            std::assert_matches!(
                refused(&mut wb, |ws| ws.remove_table("Sales")),
                Error::TableInUse(_)
            );
        }
    }
    let mut wb = Workbook::new().unwrap();
    wb.worksheet()
        .unwrap()
        .add_table(
            table("A1:B3")
                .name("Sales")
                .columns([Col::new("A"), Col::new("B")]),
        )
        .unwrap();
    std::assert_matches!(
        refused(&mut wb, |ws| ws.write_string(0, 0, "b")),
        Error::DuplicateColumnName(_)
    );
    std::assert_matches!(
        refused(&mut wb, |ws| ws.write_string(0, 0, "")),
        Error::TableHeaderCell { .. }
    );
    std::assert_matches!(
        refused(&mut wb, |ws| ws.write_number(0, 0, 2.0)),
        Error::TableHeaderCell { .. }
    );
    std::assert_matches!(
        refused(&mut wb, |ws| ws.remove_table("Missing")),
        Error::NoSuchTable(_)
    );
    wb.worksheet()
        .unwrap()
        .add_validation("E1", &Validation::list_from("A2:A3"))
        .unwrap();
    let part = wb.worksheet_part().to_owned();
    let bytes = wb.to_bytes().unwrap();
    let xml = String::from_utf8(common::part(&bytes, &part))
        .unwrap()
        .replace(">A2:A3<", ">Sales[A]<");
    let mut wb = Workbook::from_bytes(common::p1::with_part(&bytes, &part, &xml)).unwrap();
    std::assert_matches!(
        refused(&mut wb, |ws| ws.remove_table("Sales")),
        Error::TableInUse(_)
    );
    std::assert_matches!(
        refused(&mut wb, |ws| ws.write_cells(&[
            (at("F1"), CellWrite::Number(8.0)),
            (at("A1"), CellWrite::String("Revenue")),
        ])),
        Error::ColumnInUse(_)
    );
}

#[test]
fn xstring_and_normal_string_entry_points() {
    let mut wb = Workbook::new().unwrap();
    let mut ws = wb.worksheet().unwrap();
    ws.add_table(table("A1:B3")).unwrap();
    ws.write_cells(&[
        (at("A1"), CellWrite::String("Column2")),
        (at("B1"), CellWrite::String("Column1")),
    ])
    .unwrap();
    ws.cell_mut(0, 1)
        .unwrap()
        .set_string_inline("line\n_x0041_")
        .unwrap();
    assert_eq!(
        ws.cell_a1("B1").unwrap().unwrap().value().unwrap(),
        CellValue::String("line\n_x0041_".into())
    );
    assert!(table_xml(&wb, TABLE1).contains(r#"name="line_x000a__x005F_x0041_""#));
}

#[cfg(feature = "fault-injection")]
#[test]
fn table_and_header_faults() {
    use ooxml_xlsx::ApplyStep;
    use std::panic::{AssertUnwindSafe, catch_unwind};

    for create in [true, false] {
        let mut steps = vec![
            ApplyStep::Registry,
            ApplyStep::Strip,
            ApplyStep::HandOff,
            ApplyStep::Registries,
            ApplyStep::Target,
            ApplyStep::Index,
        ];
        if create {
            steps.extend([ApplyStep::TableParts, ApplyStep::LaterTarget]);
        }
        for step in steps {
            for panic in [false, true] {
                let mut wb = Workbook::new().unwrap();
                if !create {
                    wb.worksheet().unwrap().add_table(table("A1:B3")).unwrap();
                }
                if panic {
                    wb.inject_apply_panic(step);
                } else {
                    wb.inject_apply_failure(step);
                }
                let run = catch_unwind(AssertUnwindSafe(|| {
                    let mut ws = wb.worksheet().unwrap();
                    if create {
                        ws.add_table(table("A1:B3")).map(|_| ())
                    } else {
                        ws.write_string(0, 0, "Changed")
                    }
                }));
                if panic {
                    assert!(run.is_err());
                } else {
                    std::assert_matches!(run.unwrap(), Err(Error::Poisoned(_)));
                }
                std::assert_matches!(wb.to_bytes(), Err(Error::Poisoned(_)));
            }
        }
        let mut wb = Workbook::new().unwrap();
        if !create {
            wb.worksheet().unwrap().add_table(table("A1:B3")).unwrap();
        }
        wb.inject_apply_failure(ApplyStep::StaleRegistryPlan);
        assert!(!matches!(
            refused(&mut wb, |ws| {
                if create {
                    ws.add_table(table("A1:B3")).map(|_| ())
                } else {
                    ws.write_string(0, 0, "Changed")
                }
            }),
            Error::Poisoned(_)
        ));
    }
}

#[test]
fn metadata_and_array_boundaries_are_refused_before_apply() {
    for cell in [
        r#"<c r="A2" vm="1"><v>0</v></c>"#,
        r#"<c r="A2" cm="1"><v>0</v></c>"#,
        r#"<c r="A2"><f t="shared" si="0" ref="A2:A3">1</f><v>1</v></c>"#,
        r#"<c r="A2"><f t="array" ref="A2:D3">1</f><v>1</v></c>"#,
    ] {
        let xml = format!(r#"<row r="2">{cell}</row>"#);
        let mut wb = Workbook::from_bytes(common::p1::book(&[("Sheet1", &xml)], "", "")).unwrap();
        std::assert_matches!(
            refused(&mut wb, |ws| ws.add_table(table("A1:B3"))),
            Error::TableRange { .. } | Error::UnknownCellMetadata { .. }
        );
    }
}

#[test]
fn defined_names_are_checked_across_scopes() {
    let sheet = "";
    let names = r#"<definedName name="Sales" localSheetId="1">1</definedName>"#;
    let mut wb = Workbook::from_bytes(common::p1::book(
        &[("Sheet1", sheet), ("Sheet2", sheet)],
        names,
        "",
    ))
    .unwrap();
    std::assert_matches!(
        refused(&mut wb, |ws| ws.add_table(table("A1:B3").name("SALES"))),
        Error::NameInUse { .. }
    );

    let names = r#"<definedName name="Report">Sales[Column1]</definedName>"#;
    let mut wb = Workbook::from_bytes(common::p1::book(&[("Sheet1", sheet)], names, "")).unwrap();
    wb.worksheet()
        .unwrap()
        .add_table(table("A1:B3").name("Sales"))
        .unwrap();
    std::assert_matches!(
        refused(&mut wb, |ws| ws.write_string(0, 0, "Other")),
        Error::ColumnInUse(_)
    );
    std::assert_matches!(
        refused(&mut wb, |ws| ws.remove_table("Sales")),
        Error::TableInUse(_)
    );
}

#[test]
fn headerless_table_seeds_the_formulas_that_could_resolve_its_name() {
    let data = r#"<row r="1"><c r="E1"><f>Sales[Column1]</f><v>42</v></c></row>"#;
    let mut wb = Workbook::from_bytes(common::p1::book(&[("Sheet1", data)], "", "")).unwrap();
    let mut ws = wb.worksheet().unwrap();
    ws.add_table(table("A1:B2").header_row(false).name("Sales"))
        .unwrap();
    assert!(ws.cell_a1("A1").unwrap().is_none());
    assert_eq!(
        ws.cell_a1("E1").unwrap().unwrap().value().unwrap(),
        CellValue::Empty
    );
    ws.write_number(0, 0, 9.0).unwrap();
    let xml = table_xml(&wb, TABLE1);
    assert!(xml.contains(r#"headerRowCount="0""#));
    assert!(!xml.contains("autoFilter"));
    let bytes = wb.to_bytes().unwrap();
    let workbook = String::from_utf8(common::part(&bytes, wb.workbook_part())).unwrap();
    assert!(workbook.contains(r#"fullCalcOnLoad="1""#));
}

#[test]
fn subtotal_and_calculated_column_escape_literal_special_characters() {
    let mut wb = Workbook::new().unwrap();
    let mut ws = wb.worksheet().unwrap();
    ws.add_table(
        table("A1:B3")
            .name("Sales")
            .columns([
                Col::new("@Price [est] #1").total(Total::Sum),
                Col::new("Copy").formula("[@['@Price '[est'] '#1]]"),
            ])
            .totals_row(true),
    )
    .unwrap();
    assert_eq!(
        ws.cell_a1("A3").unwrap().unwrap().formula().unwrap(),
        "SUBTOTAL(109,Sales[['@Price '[est'] '#1]])"
    );
    assert_eq!(
        ws.cell_a1("B2").unwrap().unwrap().formula().unwrap(),
        "Sales[[#This Row],['@Price '[est'] '#1]]"
    );
    std::assert_matches!(
        refused(&mut wb, |ws| ws.write_string(0, 0, "Price")),
        Error::ColumnInUse(_)
    );
    std::assert_matches!(
        refused(&mut wb, |ws| ws.remove_table("Sales")),
        Error::TableInUse(_)
    );
}

#[test]
fn deleting_a_sheet_preserves_the_ids_and_releases_the_names_of_its_tables() {
    let mut wb = Workbook::new().unwrap();
    wb.add_sheet("Other", 1).unwrap();
    wb.worksheet_by_name("Other")
        .unwrap()
        .add_table(table("A1:B2").name("Sales"))
        .unwrap();
    let mut wb = Workbook::from_bytes(wb.to_bytes().unwrap()).unwrap();
    wb.delete_sheet("Other").unwrap();
    wb.worksheet()
        .unwrap()
        .add_table(table("A1:B2").name("Sales"))
        .unwrap();
    let tables: Vec<_> = wb
        .package()
        .part_names()
        .filter(|p| p.starts_with("xl/tables/"))
        .collect();
    assert_eq!(tables.len(), 1);
    assert!(table_xml(&wb, tables[0]).contains(r#"id="2""#));
}

#[test]
fn slicer_ids_are_reserved_and_slicer_sources_are_protected() {
    use ooxml_core::{Package, PartNameRequest, RegistryPlan};

    const SLICER: &str = "xl/slicerCaches/slicerCache1.xml";
    const NS: &str = "http://schemas.microsoft.com/office/spreadsheetml/2010/11/main";
    let mut wb = Workbook::new().unwrap();
    wb.worksheet()
        .unwrap()
        .add_table(table("A1:B2").name("Sales"))
        .unwrap();
    let mut pkg = Package::from_bytes(wb.to_bytes().unwrap()).unwrap();
    let mut plan = RegistryPlan::new(&pkg);
    plan.add_part(
        &pkg,
        PartNameRequest::Exact(SLICER.into()),
        "application/vnd.ms-excel.slicerCache+xml",
        format!(r#"<ext xmlns:x15="{NS}"><x15:tableSlicerCache tableId="1"/><x15:tableSlicerCache tableId="17"/></ext>"#).into_bytes(),
    ).unwrap();
    pkg.apply(plan).unwrap();
    let mut wb = Workbook::from_bytes(pkg.to_bytes().unwrap()).unwrap();

    std::assert_matches!(
        refused(&mut wb, |ws| ws.write_string(0, 0, "Different")),
        Error::TableHeaderCell { .. }
    );
    std::assert_matches!(
        refused(&mut wb, |ws| ws.remove_table("Sales")),
        Error::TableInUse(_)
    );
    wb.worksheet().unwrap().add_table(table("D1:E2")).unwrap();
    assert!(table_xml(&wb, "xl/tables/table2.xml").contains(r#"id="18""#));
}

#[test]
fn last_table_removal_is_local_to_sheet_reference_and_registry() {
    use ooxml_core::{ElementSpec, XmlDocument};
    use ooxml_schema::sml;

    let mut wb = Workbook::new().unwrap();
    let mut ws = wb.worksheet().unwrap();
    ws.write_string(1, 0, "Preserve me").unwrap();
    ws.add_table(table("A1:B3").name("Sales")).unwrap();
    let sheet = wb.worksheet_part().to_owned();
    let before = wb.to_bytes().unwrap();

    wb.worksheet().unwrap().remove_table("Sales").unwrap();
    let after = wb.to_bytes().unwrap();
    let mut expected = common::part(&before, &sheet);
    let doc = XmlDocument::parse(&expected).unwrap();
    let span = doc
        .root()
        .elements()
        .find(|n| n.is(sml::TableParts::QNAME))
        .unwrap()
        .span()
        .unwrap();
    expected.drain(span.start..span.end());
    assert_eq!(common::part(&after, &sheet), expected);
    let after_entries = common::entries(&after);
    for entry in common::entries(&before) {
        if ![
            sheet.as_str(),
            TABLE1,
            "[Content_Types].xml",
            "xl/worksheets/_rels/sheet1.xml.rels",
        ]
        .contains(&entry.0.as_str())
        {
            assert!(
                after_entries.contains(&entry),
                "removed or changed {}",
                entry.0
            );
        }
    }
}

#[test]
fn calculated_column_qualifies_and_validates_structured_references() {
    for (formula, expected) in [
        ("[@Q]*2", "Sales[[#This Row],[Q]]*2"),
        ("sales[@q]*2", "Sales[[#This Row],[q]]*2"),
        ("[[#This Row],[Q]]*2", "Sales[[#This Row],[Q]]*2"),
        ("SUM([Q])", "SUM(Sales[Q])"),
        ("SUM([1])", "SUM(Sales[1])"),
        ("SUM([[#Data],[Q]:[1]])", "SUM(Sales[[#Data],[Q]:[1]])"),
        (
            "Other[@Missing]+[@Q]",
            "Other[@Missing]+Sales[[#This Row],[Q]]",
        ),
        (
            "IF([@Q],\"[@Q]\",0)",
            "IF(Sales[[#This Row],[Q]],\"[@Q]\",0)",
        ),
    ] {
        let mut wb = Workbook::new().unwrap();
        let mut ws = wb.worksheet().unwrap();
        ws.add_table(table("A1:C3").name("Sales").columns([
            Col::new("Q"),
            Col::new("1"),
            Col::new("C").formula(formula),
        ]))
        .unwrap();
        assert_eq!(ws.cell(1, 2).unwrap().unwrap().formula().unwrap(), expected);
        assert_eq!(ws.cell(2, 2).unwrap().unwrap().formula().unwrap(), expected);
        assert!(table_xml(&wb, "xl/tables/table1.xml").contains(&format!(
            "<calculatedColumnFormula>{expected}</calculatedColumnFormula>"
        )));
    }

    for formula in [
        "[@Missing]+1",
        "Sales[missing]",
        "SUM([[Q]:[Missing]])",
        "[[#Unknown],[Q]]",
        "[[Q],[C]]",
        "Other[[#Unknown],[Q]]",
        "[@Q",
    ] {
        let mut wb = Workbook::new().unwrap();
        std::assert_matches!(
            refused(&mut wb, |ws| ws.add_table(
                table("A1:B3")
                    .name("Sales")
                    .columns([Col::new("Q"), Col::new("C").formula(formula)])
            )),
            Error::InvalidFormula { .. }
        );
    }
}

#[test]
fn header_rename_checks_the_batch_result() {
    let mut wb = Workbook::new().unwrap();
    wb.worksheet()
        .unwrap()
        .add_table(
            table("A1:B3")
                .name("Sales")
                .columns([Col::new("Q"), Col::new("C")]),
        )
        .unwrap();
    for (cell, formula) in [("Z1", "SUM(Sales[q])"), ("B2", "[@Q]")] {
        std::assert_matches!(
            refused(&mut wb, |ws| ws.write_cells(&[
                (at("A1"), CellWrite::String("Quantity")),
                (at(cell), CellWrite::Formula(formula)),
            ])),
            Error::ColumnInUse(_)
        );
    }
    let mut ws = wb.worksheet().unwrap();
    ws.write_cells(&[(at("B2"), CellWrite::Formula("[@Q]"))])
        .unwrap();
    ws.write_cells(&[
        (at("A1"), CellWrite::String("Quantity")),
        (at("B2"), CellWrite::Formula("[@Quantity]")),
    ])
    .unwrap();
    assert_eq!(
        ws.cell(1, 1).unwrap().unwrap().formula().unwrap(),
        "[@Quantity]"
    );
}

#[test]
fn removal_checks_bare_table_names_in_unlexable_formulas() {
    for (formula, blocked) in [("ROWS(sales)+`", true), ("ROWS(SalesArchive)+`", false)] {
        let mut wb = Workbook::new().unwrap();
        let mut ws = wb.worksheet().unwrap();
        ws.add_table(table("A1:B3").name("Sales")).unwrap();
        ws.write_cells(&[(at("Z1"), CellWrite::Formula("ROWS(Sales)"))])
            .unwrap();
        let part = wb.worksheet_part().to_owned();
        let bytes = wb.to_bytes().unwrap();
        let xml = String::from_utf8(common::part(&bytes, &part))
            .unwrap()
            .replace(">ROWS(Sales)<", &format!(">{formula}<"));
        let mut wb = Workbook::from_bytes(common::p1::with_part(&bytes, &part, &xml)).unwrap();
        if blocked {
            std::assert_matches!(
                refused(&mut wb, |ws| ws.remove_table("Sales")),
                Error::TableInUse(_)
            );
        } else {
            wb.worksheet().unwrap().remove_table("Sales").unwrap();
        }
    }
}

#[test]
fn table_metadata_keeps_explicit_none_and_optional_label() {
    let mut wb = Workbook::new().unwrap();
    wb.worksheet().unwrap().add_table(
        table("A1:C2").name("TotalsMetadata").metadata_only().columns([
            Col::new("Unspecified"),
            Col::new("Explicit").no_total_function(),
            Col::new("Labelled").total_label("Total").no_total_function(),
        ]),
    ).unwrap();
    let xml = table_xml(&wb, TABLE1);
    assert!(xml.contains(r#"name="Unspecified"/>"#), "{xml}");
    assert!(xml.contains(r#"name="Explicit" totalsRowFunction="none"/>"#), "{xml}");
    assert!(xml.contains(r#"name="Labelled" totalsRowFunction="none" totalsRowLabel="Total"/>"#), "{xml}");
    assert!(wb.worksheet().unwrap().cell(0, 0).unwrap().is_none());
}
