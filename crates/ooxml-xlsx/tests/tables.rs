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
        table("D1:E1"),
        table("D1:E2").totals_row(true),
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
