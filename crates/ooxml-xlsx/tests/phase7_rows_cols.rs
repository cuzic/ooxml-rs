mod common;

use common::{p1, rebuild, text};
use ooxml_xlsx::{Error, StructuralEditReport, Workbook, Worksheet};

fn book() -> Vec<u8> {
    p1::book(
        &[
            (
                "Data",
                r#"<row r='1' spans='1:4'><c r='A1'><v>10</v></c></row><row r='2' s='0'><c r='A2'><v>20</v></c><c r='B2'><f>$A$2</f><v>20</v></c></row>"#,
            ),
            (
                "Other",
                r#"<row r='1'><c r='A1'><f>Data!A2</f><v>20</v></c></row>"#,
            ),
        ],
        r#"<definedName name="Input">Data!$A$2</definedName>"#,
        "",
    )
}

#[test]
fn edits_rebuild_addresses_and_rewrite_formulas_on_other_sheets() {
    let mut wb = Workbook::from_bytes(book()).unwrap();
    wb.worksheet_mut("Data").unwrap().insert_rows(1, 2).unwrap();
    wb.worksheet_mut("Data").unwrap().insert_cols(0, 1).unwrap();
    assert!(
        wb.worksheet_mut("Data")
            .unwrap()
            .cell_a1("A2")
            .unwrap()
            .is_none()
    );
    let bytes = wb.to_bytes().unwrap();
    if let Some(path) = std::env::var_os("OOXML_XLSX_PHASE7_CASE") {
        std::fs::write(path, &bytes).unwrap();
    }
    let data = text(&bytes, "xl/worksheets/sheet1.xml");
    assert!(
        data.contains("<row r=\"4\" s='0'><c r=\"B4\"><v>20</v></c>"),
        "{data}"
    );
    assert!(data.contains("<f>$B$4</f>"), "{data}");
    assert!(text(&bytes, "xl/worksheets/sheet2.xml").contains("<f>Data!B4</f>"));
    assert!(text(&bytes, "xl/workbook.xml").contains("Data!$B$4"));
    wb.worksheet_mut("Data").unwrap().delete_cols(1, 1).unwrap();
    wb.worksheet_mut("Data").unwrap().delete_rows(1, 2).unwrap();
    let mut reopened = Workbook::from_bytes(wb.to_bytes().unwrap()).unwrap();
    assert_eq!(
        reopened
            .worksheet_mut("Data")
            .unwrap()
            .cell_a1("B2")
            .unwrap()
            .unwrap()
            .formula()
            .as_deref(),
        Some("#REF!")
    );
    assert_eq!(
        reopened
            .worksheet_mut("Other")
            .unwrap()
            .cell_a1("A1")
            .unwrap()
            .unwrap()
            .formula()
            .as_deref(),
        Some("Data!#REF!")
    );
}

#[test]
fn cross_sheet_rules_and_sticky_ranges_shift() {
    let base = book();
    let data = "xl/worksheets/sheet1.xml";
    let other = "xl/worksheets/sheet2.xml";
    let own_xml = text(&base, data).replace("</worksheet>", r#"<ignoredErrors><ignoredError sqref="A2:A1048576" numberStoredAsText="1"/></ignoredErrors></worksheet>"#);
    let other_xml = text(&base, other).replace("</worksheet>", r#"<conditionalFormatting sqref="B1:B4"><cfRule type="expression" priority="1"><formula>Data!A2&gt;0</formula></cfRule></conditionalFormatting><dataValidations count="1"><dataValidation type="custom" sqref="C1"><formula1>Data!$A$2&gt;0</formula1></dataValidation></dataValidations></worksheet>"#);
    let input = rebuild(
        &base,
        &[
            (data, own_xml.into_bytes()),
            (other, other_xml.into_bytes()),
        ],
    );
    let mut wb = Workbook::from_bytes(input).unwrap();
    wb.worksheet_mut("Data").unwrap().insert_rows(1, 1).unwrap();
    let bytes = wb.to_bytes().unwrap();
    let other = text(&bytes, other);
    assert!(other.contains("<formula>Data!A3&gt;0</formula>"), "{other}");
    assert!(
        other.contains("<formula1>Data!$A$3&gt;0</formula1>"),
        "{other}"
    );
    assert!(other.contains("sqref=\"B1:B4\""));
    assert!(text(&bytes, data).contains("sqref=\"A3:A1048576\""));
    wb.worksheet_mut("Data").unwrap().delete_rows(0, 1).unwrap();
    let bytes = wb.to_bytes().unwrap();
    assert!(text(&bytes, data).contains("sqref=\"A2:A1048576\""));
}

#[test]
fn shared_members_split_when_relative_formulas_diverge() {
    let input = p1::book(
        &[(
            "Data",
            r#"<row r="2"><c r="B2"><f t="shared" si="0" ref="B2:B3">A1</f><v>1</v></c></row><row r="3"><c r="B3"><f t="shared" si="0"/><v>2</v></c></row>"#,
        )],
        "",
        "",
    );
    let mut wb = Workbook::from_bytes(input).unwrap();
    wb.worksheet_mut("Data").unwrap().insert_rows(2, 1).unwrap();
    let bytes = wb.to_bytes().unwrap();
    let xml = text(&bytes, "xl/worksheets/sheet1.xml");
    assert!(xml.contains("<c r=\"B2\"><f>A1</f></c>"), "{xml}");
    assert!(xml.contains("<c r=\"B4\"><f>A2</f></c>"), "{xml}");
    assert!(!xml.contains("t=\"shared\""), "{xml}");
}

#[test]
fn protection_and_off_sheet_refusals_leave_every_byte_unchanged() {
    let base = book();
    let wb_part = "xl/workbook.xml";
    let locked = text(&base, wb_part).replace(
        "<sheets>",
        r#"<workbookProtection lockStructure="1"/><sheets>"#,
    );
    let mut wb = Workbook::from_bytes(rebuild(&base, &[(wb_part, locked.into_bytes())])).unwrap();
    let before = wb.to_bytes().unwrap();
    for (_, edit) in OPERATIONS {
        std::assert_matches!(
            edit(&mut wb.worksheet_mut("Data").unwrap(), 1, 1),
            Err(Error::Protected {
                scope: "workbook",
                ..
            })
        );
        assert_eq!(wb.to_bytes().unwrap(), before);
    }
    let edge = p1::book(
        &[(
            "Data",
            r#"<row r="1048576"><c r="A1048576"><v>1</v></c></row>"#,
        )],
        "",
        "",
    );
    let mut wb = Workbook::from_bytes(edge).unwrap();
    let before = wb.to_bytes().unwrap();
    std::assert_matches!(
        wb.worksheet_mut("Data").unwrap().insert_rows(0, 1),
        Err(Error::ShiftOffSheet { .. })
    );
    assert_eq!(wb.to_bytes().unwrap(), before);
    std::assert_matches!(
        wb.worksheet_mut("Data").unwrap().delete_cols(0, 0),
        Err(Error::InvalidStructuralBand { .. })
    );
    assert_eq!(wb.to_bytes().unwrap(), before);
}

#[test]
fn crossing_array_is_atomic_and_moving_formula_removes_chain() {
    let mut wb = Workbook::from_bytes(p1::synthetic("o1_arrays.xlsx")).unwrap();
    let before = wb.to_bytes().unwrap();
    std::assert_matches!(
        wb.worksheet_mut("Sheet1").unwrap().insert_rows(1, 1),
        Err(Error::ArrayFormulaSplit { .. })
    );
    assert_eq!(wb.to_bytes().unwrap(), before);
    wb.worksheet_mut("Sheet1")
        .unwrap()
        .insert_rows(0, 1)
        .unwrap();
    let bytes = wb.to_bytes().unwrap();
    assert!(
        !common::entries(&bytes)
            .iter()
            .any(|entry| entry.0 == "xl/calcChain.xml")
    );
    assert!(text(&bytes, "xl/worksheets/sheet1.xml").contains("ref=\"B2:B4\""));
}

type Edit = fn(&mut Worksheet<'_>, u32, u32) -> ooxml_xlsx::Result<StructuralEditReport>;
const OPERATIONS: [(&str, Edit); 4] = [
    ("insert_rows", |ws, at, count| ws.insert_rows(at, count)),
    ("delete_rows", |ws, at, count| ws.delete_rows(at, count)),
    ("insert_cols", |ws, at, count| ws.insert_cols(at, count)),
    ("delete_cols", |ws, at, count| ws.delete_cols(at, count)),
];

#[test]
#[ignore = "requires OOXML_XLSX_CORPUS; OOXML_XLSX_CORPUS_OUTPUT optionally exports successes"]
fn rows_and_columns_corpus() {
    let corpus = std::env::var_os("OOXML_XLSX_CORPUS").expect("set OOXML_XLSX_CORPUS");
    let output = std::env::var_os("OOXML_XLSX_CORPUS_OUTPUT").map(std::path::PathBuf::from);
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    let mut files = std::fs::read_dir(corpus)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.extension()
                .is_some_and(|ext| ext == "xlsx" || ext == "xlsm")
        })
        .collect::<Vec<_>>();
    files.sort();
    for (file_id, path) in files.iter().enumerate() {
        let input = std::fs::read(path).unwrap();
        let Ok(book) = Workbook::from_bytes(input.clone()) else {
            *counts.entry("open refused".into()).or_default() += 1;
            continue;
        };
        let before = book.to_bytes().unwrap();
        for (sheet_id, sheet) in book.sheet_names().enumerate() {
            for (operation, edit) in OPERATIONS {
                let mut wb = Workbook::from_bytes(input.clone()).unwrap();
                let result = wb
                    .worksheet_mut(sheet)
                    .and_then(|mut ws| edit(&mut ws, 1, 1));
                match result {
                    Ok(_) => {
                        let bytes = wb.to_bytes().unwrap();
                        Workbook::from_bytes(bytes.clone()).unwrap();
                        if let Some(output) = &output {
                            let ext = path.extension().unwrap().to_str().unwrap();
                            std::fs::write(
                                output.join(format!("{file_id}-{sheet_id}-{operation}.{ext}")),
                                bytes,
                            )
                            .unwrap();
                        }
                        *counts.entry("success".into()).or_default() += 1;
                    }
                    Err(error) => {
                        assert!(
                            wb.to_bytes().unwrap() == before,
                            "{} {sheet} {operation}: {error:?}",
                            path.display()
                        );
                        let debug = format!("{error:?}");
                        let kind = debug.split([' ', '(', '{']).next().unwrap();
                        *counts.entry(kind.into()).or_default() += 1;
                        eprintln!("{} {sheet} {operation}: {error:?}", path.display());
                    }
                }
            }
        }
    }
    println!("{counts:#?}");
}

#[test]
fn moving_an_object_refuses_but_an_edit_after_it_succeeds() {
    use ooxml_xlsx::{CellRange, Table};
    let mut wb = Workbook::new().unwrap();
    wb.worksheet()
        .unwrap()
        .add_table(Table::new(CellRange::parse("A1:B3").unwrap()))
        .unwrap();
    let before = wb.to_bytes().unwrap();
    std::assert_matches!(
        wb.worksheet().unwrap().insert_rows(1, 1),
        Err(Error::StructureBlocked { .. })
    );
    assert_eq!(wb.to_bytes().unwrap(), before);
    wb.worksheet().unwrap().insert_rows(3, 1).unwrap();
}

#[test]
fn column_formats_split_and_new_columns_are_unformatted() {
    let base = book();
    let part = "xl/worksheets/sheet1.xml";
    let xml = text(&base, part).replace(
        "<sheetData>",
        "<cols><col min='2' max='16384' width='12' customWidth='1'/></cols><sheetData>",
    );
    let mut wb = Workbook::from_bytes(rebuild(&base, &[(part, xml.into_bytes())])).unwrap();
    wb.worksheet_mut("Data").unwrap().insert_cols(3, 2).unwrap();
    let xml = text(&wb.to_bytes().unwrap(), part);
    assert!(
        xml.contains("<col min='2' max=\"3\" width='12' customWidth='1'/>"),
        "{xml}"
    );
    assert!(
        xml.contains("<col width=\"12\" customWidth=\"1\" min=\"6\" max=\"16384\"/>"),
        "{xml}"
    );
}

#[test]
fn sheet_protection_and_filter_refusals_are_atomic() {
    let base = book();
    let part = "xl/worksheets/sheet1.xml";
    let data = text(&base, part);
    let protection = data.replace(
        "</sheetData>",
        r#"</sheetData><sheetProtection sheet="1" insertRows="0"/>"#,
    );
    let mut wb = Workbook::from_bytes(rebuild(&base, &[(part, protection.into_bytes())])).unwrap();
    let before = wb.to_bytes().unwrap();
    std::assert_matches!(
        wb.worksheet_mut("Data").unwrap().delete_rows(0, 1),
        Err(Error::Protected {
            scope: "worksheet",
            ..
        })
    );
    assert_eq!(wb.to_bytes().unwrap(), before);
    wb.worksheet_mut("Data").unwrap().insert_rows(1, 1).unwrap();

    let filter = data.replace("</sheetData>", r#"</sheetData><autoFilter ref="A1:B2"><filterColumn colId="0"><filters/></filterColumn></autoFilter>"#);
    let mut wb = Workbook::from_bytes(rebuild(&base, &[(part, filter.into_bytes())])).unwrap();
    let before = wb.to_bytes().unwrap();
    std::assert_matches!(
        wb.worksheet_mut("Data").unwrap().delete_rows(0, 1),
        Err(Error::StructureBlocked { .. })
    );
    assert_eq!(wb.to_bytes().unwrap(), before);
    std::assert_matches!(
        wb.worksheet_mut("Data").unwrap().delete_cols(0, 1),
        Err(Error::StructureBlocked { .. })
    );
    assert_eq!(wb.to_bytes().unwrap(), before);
}

#[cfg(feature = "fault-injection")]
#[test]
fn structural_failures_after_apply_poison_and_stale_plan_leaves_bytes_unchanged() {
    use ooxml_xlsx::ApplyStep;
    for step in [
        ApplyStep::Registry,
        ApplyStep::Strip,
        ApplyStep::HandOff,
        ApplyStep::Target,
        ApplyStep::LaterTarget,
        ApplyStep::StructuralRebuild,
        ApplyStep::StructuralReindex,
    ] {
        for panic in [false, true] {
            let mut wb = Workbook::from_bytes(book()).unwrap();
            if panic {
                wb.inject_apply_panic(step);
            } else {
                wb.inject_apply_failure(step);
            }
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                wb.worksheet_mut("Data").unwrap().insert_rows(1, 1)
            }));
            if panic {
                assert!(result.is_err());
            } else {
                std::assert_matches!(result.unwrap(), Err(Error::Poisoned(_)));
            }
            std::assert_matches!(wb.to_bytes(), Err(Error::Poisoned(_)));
        }
    }
    let mut wb = Workbook::from_bytes(book()).unwrap();
    let before = wb.to_bytes().unwrap();
    wb.inject_apply_failure(ApplyStep::StaleRegistryPlan);
    assert!(wb.worksheet_mut("Data").unwrap().insert_rows(1, 1).is_err());
    assert_eq!(wb.to_bytes().unwrap(), before);
}

#[test]
fn linked_x14_rules_follow_their_2007_rule_on_deletion() {
    let base = p1::book(&[("Data", "")], "", "");
    let part = "xl/worksheets/sheet1.xml";
    let data = text(&base, part).replace("</worksheet>", concat!(
        r#"<conditionalFormatting sqref="A2"><cfRule type="dataBar" priority="1"><extLst>"#,
        r#"<ext uri="{B025F937-C7B1-47D3-B67F-A62EFF666E3E}"><x14:id xmlns:x14="http://schemas.microsoft.com/office/spreadsheetml/2009/9/main">{00000000-0000-0000-0000-000000000001}</x14:id></ext>"#,
        r#"</extLst></cfRule></conditionalFormatting><extLst>"#,
        r#"<ext uri="{78C0D931-6437-407d-A8EE-F0AAD7539E65}" xmlns:x14="http://schemas.microsoft.com/office/spreadsheetml/2009/9/main" xmlns:xm="http://schemas.microsoft.com/office/excel/2006/main">"#,
        r#"<x14:conditionalFormattings><x14:conditionalFormatting><x14:cfRule type="dataBar" id="{00000000-0000-0000-0000-000000000001}"/><xm:sqref>A2:A3</xm:sqref></x14:conditionalFormatting></x14:conditionalFormattings>"#,
        r#"</ext></extLst></worksheet>"#,
    ));
    let mut wb = Workbook::from_bytes(rebuild(&base, &[(part, data.into_bytes())])).unwrap();
    wb.worksheet().unwrap().delete_rows(1, 1).unwrap();
    let data = text(&wb.to_bytes().unwrap(), part);
    assert!(!data.contains("conditionalFormatting"), "{data}");
    assert!(!data.contains("extLst"), "{data}");
}

#[test]
fn cross_sheet_x14_and_table_formulas_shift() {
    use ooxml_xlsx::{CellRange, Col, Table};
    let base = book();
    let other = "xl/worksheets/sheet2.xml";
    let xml = text(&base, other).replace("</worksheet>", concat!(
        r#"<extLst><ext uri="{CCE6A557-97BC-4B89-ADB6-D9C93CAAB3DF}" xmlns:x14="http://schemas.microsoft.com/office/spreadsheetml/2009/9/main" xmlns:xm="http://schemas.microsoft.com/office/excel/2006/main">"#,
        r#"<x14:dataValidations count="1"><x14:dataValidation type="custom"><x14:formula1><xm:f>Data!A2&gt;0</xm:f></x14:formula1><xm:sqref>D2</xm:sqref></x14:dataValidation></x14:dataValidations>"#,
        r#"</ext></extLst></worksheet>"#,
    ));
    let mut wb = Workbook::from_bytes(rebuild(&base, &[(other, xml.into_bytes())])).unwrap();
    wb.worksheet_mut("Other")
        .unwrap()
        .add_table(
            Table::new(CellRange::parse("A4:B7").unwrap())
                .columns([Col::new("Input"), Col::new("Result").formula("Data!A2")]),
        )
        .unwrap();
    wb.worksheet_mut("Data").unwrap().insert_rows(1, 1).unwrap();
    let saved = wb.to_bytes().unwrap();
    let xml = text(&saved, other);
    assert!(xml.contains("<xm:f>Data!A3&gt;0</xm:f>"), "{xml}");
    assert!(xml.contains("<xm:sqref>D2</xm:sqref>"), "{xml}");
    let xml = text(&saved, "xl/tables/table1.xml");
    assert!(
        xml.contains("<calculatedColumnFormula>Data!A3</calculatedColumnFormula>"),
        "{xml}"
    );
}

#[test]
fn deleting_shared_string_cells_updates_the_reference_count() {
    let mut wb = Workbook::new().unwrap();
    let mut sheet = wb.worksheet().unwrap();
    sheet.write_string(0, 0, "Same").unwrap();
    sheet.write_string(1, 0, "Same").unwrap();
    sheet.delete_rows(0, 1).unwrap();
    let saved = wb.to_bytes().unwrap();
    let xml = text(&saved, "xl/sharedStrings.xml");
    assert!(xml.contains("count=\"1\""), "{xml}");
    assert!(xml.contains("uniqueCount=\"1\""), "{xml}");
}

#[test]
fn absolute_and_one_cell_drawing_modes_keep_their_position() {
    use ooxml_xlsx::{Anchor, CellIndex, Image};
    let mut wb = Workbook::new().unwrap();
    let image = Image::from_bytes(include_bytes!("images/blue.png").as_slice()).unwrap();
    wb.worksheet()
        .unwrap()
        .insert_image(
            Anchor::two_cell(
                CellIndex::parse("A1").unwrap(),
                CellIndex::parse("D4").unwrap(),
            ),
            &image,
        )
        .unwrap();
    let base = wb.to_bytes().unwrap();
    let part = "xl/drawings/drawing1.xml";
    let xml = text(&base, part);
    for (mode, at) in [("absolute", 0), ("oneCell", 1)] {
        let mut xml = xml.clone();
        let anchor = xml.find("twoCellAnchor").unwrap() + "twoCellAnchor".len();
        let end = anchor + xml[anchor..].find('>').unwrap();
        if let Some(start) = xml[anchor..end].find("editAs=\"") {
            let start = anchor + start + "editAs=\"".len();
            let end = start + xml[start..].find('"').unwrap();
            xml.replace_range(start..end, mode);
        } else {
            xml.insert_str(anchor, &format!(" editAs=\"{mode}\""));
        }
        let mut wb =
            Workbook::from_bytes(rebuild(&base, &[(part, xml.as_bytes().to_vec())])).unwrap();
        wb.worksheet().unwrap().insert_rows(at, 1).unwrap();
        assert_eq!(text(&wb.to_bytes().unwrap(), part), xml);
    }
}
