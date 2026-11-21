mod common;

use common::{read, rebuild, text};
use ooxml_xlsx::{Error, Workbook};

fn check_locked(input: &[u8]) -> usize {
    let mut book = Workbook::from_bytes(input.to_vec()).unwrap();
    let names: Vec<String> = book.sheet_names().map(str::to_owned).collect();
    let before = book.to_bytes().unwrap();
    assert_eq!(common::entries(&before), common::entries(input));
    for (index, name) in names.iter().enumerate() {
        type SheetOperation = fn(&mut Workbook, usize, &str) -> Error;
        let operations: [(&str, SheetOperation); 5] = [
            ("rename_sheet", |wb, _, name| {
                wb.rename_sheet(name, "New sheet").unwrap_err()
            }),
            ("delete_sheet", |wb, _, name| {
                wb.delete_sheet(name).unwrap_err()
            }),
            ("delete_sheet", |wb, _, name| {
                wb.delete_sheet_strict(name).unwrap_err()
            }),
            ("add_sheet", |wb, index, _| {
                wb.add_sheet("New sheet", index).unwrap_err()
            }),
            ("move_sheet", |wb, index, _| {
                let count = wb.sheet_names().count();
                wb.move_sheet(index, (index + 1) % count).unwrap_err()
            }),
        ];
        for (operation, run) in operations {
            let error = run(&mut book, index, name);
            std::assert_matches!(error, Error::Protected { scope: "workbook", operation: got } if got == operation);
            assert!(book.to_bytes().unwrap() == before, "{name}: {operation}");
        }
    }
    names.len()
}

#[test]
fn sheet_structure_protection_is_atomic() {
    let input = read("minimal");
    let part = "xl/workbook.xml";
    let xml = text(&input, part);
    for lock in ["1", "true", " 1 "] {
        let locked = xml.replace(
            "<sheets>",
            &format!(r#"<workbookProtection lockStructure="{lock}"/><sheets>"#),
        );
        assert_ne!(xml, locked);
        assert_eq!(
            check_locked(&rebuild(&input, &[(part, locked.into_bytes())])),
            1
        );
    }
    for lock in ["0", "false"] {
        let unlocked = xml.replace(
            "<sheets>",
            &format!(r#"<workbookProtection lockStructure="{lock}" lockWindows="1"/><sheets>"#),
        );
        let mut book =
            Workbook::from_bytes(rebuild(&input, &[(part, unlocked.into_bytes())])).unwrap();
        book.add_sheet("New sheet", 0).unwrap();
    }
}

#[test]
#[ignore = "requires the ADR-0018 POI corpus"]
fn protected_corpus_workbooks() {
    let corpus = std::path::PathBuf::from(
        std::env::var_os("OOXML_XLSX_CORPUS").expect("set OOXML_XLSX_CORPUS"),
    );
    let mut total = 0;
    for (name, sheets) in [
        ("CVLKRA-KYC_Download_File_Structure_V3.1.xlsx", 30),
        (
            "workbookProtection-workbook_password_user_range-2010.xlsx",
            3,
        ),
        ("workbookProtection-workbook_password-2013.xlsx", 1),
        ("workbookProtection_workbook_structure_protected.xlsx", 1),
    ] {
        let input = std::fs::read(corpus.join(name)).unwrap();
        let saved = Workbook::from_bytes(input.clone())
            .unwrap()
            .to_bytes()
            .unwrap();
        println!(
            "{name}: {sheets} sheets, original ZIP identical: {}",
            saved == input
        );
        let checked = check_locked(&input);
        assert_eq!(checked, sheets, "{name}");
        total += checked;
    }
    assert_eq!(total, 35);
}

#[test]
fn sheet_operations_preserve_unprefixed_extension_positions() {
    let base = common::p1::book(&[("Data", ""), ("Other", "")], "", "");
    let part = "xl/workbook.xml";
    let extension = r#"<extLst><ext uri="{UNKNOWN}"><newFeature ref="A1:B2"/></ext></extLst>"#;
    let workbook = text(&base, part).replace("</workbook>", &format!("{extension}</workbook>"));
    let input = rebuild(&base, &[(part, workbook.into_bytes())]);
    let mut book = Workbook::from_bytes(input).unwrap();
    book.rename_sheet("Data", "New").unwrap();
    assert!(text(&book.to_bytes().unwrap(), part).contains(extension));
    book.delete_sheet("New").unwrap();
    assert!(text(&book.to_bytes().unwrap(), part).contains(extension));
}

#[test]
fn sheet_operations_preserve_sort_state_extensions_and_rewrite_wrapped_controls() {
    let body = r#"<sheetData><row r="1"><c r="A1"><f>Data!A1</f></c></row></sheetData>
        <autoFilter ref="A1:A5"><sortState ref="A1:A5"><extLst>
          <ext uri="{0FC57C36-975A-43D2-8416-536B9B585D1B}">
            <x14:sortCondition xmlns:x14="http://schemas.microsoft.com/office/spreadsheetml/2009/9/main" ref="A2:A5"/>
          </ext>
        </extLst></sortState></autoFilter>
        <mc:AlternateContent xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"
            xmlns:x14="http://schemas.microsoft.com/office/spreadsheetml/2009/9/main">
          <mc:Choice Requires="x14"><controls><control><controlPr linkedCell="Data!B1"/></control></controls></mc:Choice>
          <mc:Fallback><controls><control><controlPr linkedCell="Data!C1"/></control></controls></mc:Fallback>
        </mc:AlternateContent>"#;
    let base = common::p1::book(&[("Data", ""), ("Other", "")], "", "");
    let part = "xl/worksheets/sheet2.xml";
    let other = text(&base, part).replace("<sheetData></sheetData>", body);
    assert!(other.contains(body));
    let untouched = body
        .split_once("</sheetData>")
        .unwrap()
        .1
        .split_once("<mc:AlternateContent")
        .unwrap()
        .0;
    let input = rebuild(&base, &[(part, other.into_bytes())]);
    for deleting in [false, true] {
        let mut book = Workbook::from_bytes(input.clone()).unwrap();
        let formula = if deleting {
            book.delete_sheet("Data").unwrap();
            "#REF!"
        } else {
            book.rename_sheet("Data", "New").unwrap();
            "New!A1"
        };
        let output = text(&book.to_bytes().unwrap(), part);
        assert!(output.contains(&format!("<f>{formula}</f>")), "{output}");
        let (choice, fallback) = if deleting {
            ("#REF!", "#REF!")
        } else {
            ("New!B1", "New!C1")
        };
        assert!(
            output.contains(&format!(r#"linkedCell="{choice}""#)),
            "{output}"
        );
        assert!(
            output.contains(&format!(r#"linkedCell="{fallback}""#)),
            "{output}"
        );
        assert!(output.contains(untouched), "{output}");
    }
}

#[test]
#[ignore = "requires the ADR-0018 POI corpus"]
fn newly_supported_corpus_locations_match_expected_values() {
    use ooxml_core::{Package, RawNode, XmlDocument};

    fn values(node: &RawNode, path: &[&str], attr: Option<&str>, found: &mut Vec<String>) {
        let Some((name, children)) = path.split_first() else {
            return;
        };
        if node.name().local() != *name {
            return;
        }
        if children.is_empty() {
            if let Some(attr) = attr {
                if let Some(value) = node
                    .attrs()
                    .iter()
                    .find(|a| a.name().ns().is_empty() && a.name().local() == attr)
                {
                    found.push(value.value().to_owned());
                }
            } else {
                found.push(node.text().into_owned());
            }
        } else {
            for child in node.elements() {
                values(child, children, attr, found);
            }
        }
    }

    fn part_values(book: &[u8], part: &str, location: &str) -> Vec<String> {
        let (path, attr) = location
            .split_once('@')
            .map_or((location, None), |(p, a)| (p, Some(a)));
        let pkg = Package::from_bytes(book.to_vec()).unwrap();
        let doc = XmlDocument::parse(&pkg.read_part(part).unwrap()).unwrap();
        let mut found = Vec::new();
        values(
            doc.root(),
            &path.split('/').collect::<Vec<_>>(),
            attr,
            &mut found,
        );
        found
    }

    let corpus = std::path::PathBuf::from(
        std::env::var_os("OOXML_XLSX_CORPUS").expect("set OOXML_XLSX_CORPUS"),
    );
    let dv = "worksheet/extLst/ext/dataValidations/dataValidation/formula1/f";
    let calc = "table/tableColumns/tableColumn/calculatedColumnFormula";
    let worksheet = "xl/worksheets/sheet1.xml";
    let cases = [
        ("56644.xlsx", "lists", "lists", 1, worksheet, dv, 5, true),
        ("61652.xlsx", "Model", "Model", 1, worksheet, dv, 1, true),
        (
            "LIBRE_OFFICE-128382-0.xlsx",
            "Basic features",
            "'Basic features'",
            0,
            worksheet,
            "worksheet/extLst/ext/sparklineGroups/sparklineGroup/sparklines/sparkline/f",
            10,
            false,
        ),
        (
            "StructuredRefs-lots-with-lookups.xlsx",
            "List",
            "List",
            1,
            "xl/tables/table1.xml",
            calc,
            1,
            true,
        ),
        (
            "WithChartSheet.xlsx",
            "Sheet2",
            "Sheet2",
            5,
            "xl/charts/chart1.xml",
            "chartSpace/pivotSource/name",
            1,
            false,
        ),
        (
            "dataValidationTableRange.xlsx",
            "County Ranking",
            "'County Ranking'",
            0,
            "xl/connections.xml",
            "connections/connection/parameters/parameter@cell",
            12,
            false,
        ),
    ];
    for (file, sheet, prefix, index, part, location, expected_count, delete_supported) in cases {
        let input = std::fs::read(corpus.join(file)).unwrap();
        let before = part_values(&input, part, location);
        let rename = format!("PhaseSevenRenamed{index}");
        for delete in [false, true]
            .into_iter()
            .filter(|delete| !delete || delete_supported)
        {
            let mut book = Workbook::from_bytes(input.clone()).unwrap();
            let mut expected = before.clone();
            let mut changes = Vec::new();
            for (value, old) in expected.iter_mut().zip(&before) {
                if delete {
                    let full_ref = match file {
                        "StructuredRefs-lots-with-lookups.xlsx" => "List!$B$3".into(),
                        _ => old.clone(),
                    };
                    if full_ref.starts_with(&format!("{prefix}!")) {
                        *value = old.replacen(&full_ref, "#REF!", 1);
                    }
                } else {
                    *value = old.replacen(&format!("{prefix}!"), &format!("{rename}!"), 1);
                }
                if value != old {
                    changes.push((old.as_str(), value.clone()));
                }
            }
            assert_eq!(changes.len(), expected_count, "{file}");
            let report = if delete {
                book.delete_sheet(sheet)
            } else {
                book.rename_sheet(sheet, &rename)
            }
            .unwrap();
            let actual: Vec<_> = report
                .iter()
                .filter(|row| row.part == part && row.location == location)
                .map(|row| (row.before.as_str(), row.after.clone()))
                .collect();
            assert_eq!(actual, changes, "{file}");
            assert_eq!(
                part_values(&book.to_bytes().unwrap(), part, location),
                expected,
                "{file}"
            );
        }
    }
}

#[test]
fn registered_web_extension_binding_is_rewritten() {
    let base = common::p1::book(&[("Data", ""), ("Bindings", "")], "", "");
    let part = "xl/worksheets/sheet2.xml";
    let sheet = text(&base, part).replace(
        "</worksheet>",
        concat!(
            r#"<extLst><ext uri="{F7C9EE02-42E1-4005-9D12-6889AFFD525C}">"#,
            r#"<x15:webExtensions xmlns:x15="http://schemas.microsoft.com/office/spreadsheetml/2010/11/main">"#,
            r#"<x15:webExtension appRef="binding"><xm:f xmlns:xm="http://schemas.microsoft.com/office/excel/2006/main">Data!A1</xm:f>"#,
            "</x15:webExtension></x15:webExtensions></ext></extLst></worksheet>",
        ),
    );
    let input = rebuild(&base, &[(part, sheet.into_bytes())]);
    let mut book = Workbook::from_bytes(input.clone()).unwrap();
    assert_eq!(book.rename_sheet("Data", "New").unwrap()[0].after, "New!A1");
    assert!(text(&book.to_bytes().unwrap(), part).contains(">New!A1</xm:f>"));
    let mut book = Workbook::from_bytes(input).unwrap();
    assert_eq!(book.delete_sheet("Data").unwrap()[0].after, "#REF!");
    assert!(text(&book.to_bytes().unwrap(), part).contains(">#REF!</xm:f>"));
}

#[test]
fn registered_formula_rewrite_escapes_text_and_respects_attribute_delimiters() {
    let extension = r#"<dataValidations count="1"><dataValidation sqref="B1"><formula1>Data!A1&amp;"&lt;&gt;"</formula1></dataValidation></dataValidations><controls><control><controlPr linkedCell='Data!A1'/></control></controls>"#;
    let input = common::p1::book(&[("Data", ""), ("Other", "")], "", "");
    let part = "xl/worksheets/sheet2.xml";
    let sheet = text(&input, part).replace("</worksheet>", &format!("{extension}</worksheet>"));
    let input = rebuild(&input, &[(part, sheet.into_bytes())]);
    let mut book = Workbook::from_bytes(input).unwrap();
    book.rename_sheet("Data", "New sheet").unwrap();
    let output = text(&book.to_bytes().unwrap(), part);
    assert!(
        output.contains(r#"<formula1>'New sheet'!A1&amp;"&lt;&gt;"</formula1>"#),
        "{output}"
    );
    assert!(
        output.contains("linkedCell='&apos;New sheet&apos;!A1'"),
        "{output}"
    );
}

#[test]
fn sheet_operations_refuse_an_unregistered_extension_formula_atomically() {
    let input = common::p1::book(&[("Data", ""), ("Other", "")], "", "");
    let part = "xl/worksheets/sheet2.xml";
    let extension = r#"<extLst><ext uri="{UNKNOWN}"><binding ref="Data!A1"/></ext></extLst>"#;
    let sheet = text(&input, part).replace("</worksheet>", &format!("{extension}</worksheet>"));
    let input = rebuild(&input, &[(part, sheet.into_bytes())]);
    let mut book = Workbook::from_bytes(input).unwrap();
    let before = book.to_bytes().unwrap();
    std::assert_matches!(
        book.rename_sheet("Data", "New"),
        Err(Error::UnrewritableReference { part: p, location }) if p == part && location.contains("{UNKNOWN}")
    );
    assert_eq!(book.to_bytes().unwrap(), before);
    std::assert_matches!(
        book.delete_sheet("Data"),
        Err(Error::UnrewritableReference { part: p, location }) if p == part && location.contains("{UNKNOWN}")
    );
    assert_eq!(book.to_bytes().unwrap(), before);
}
