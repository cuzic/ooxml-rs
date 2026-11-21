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
            ("rename_sheet", |wb, _, name| wb.rename_sheet(name, "New sheet").unwrap_err()),
            ("delete_sheet", |wb, _, name| wb.delete_sheet(name).unwrap_err()),
            ("delete_sheet", |wb, _, name| wb.delete_sheet_strict(name).unwrap_err()),
            ("add_sheet", |wb, index, _| wb.add_sheet("New sheet", index).unwrap_err()),
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
        let saved = Workbook::from_bytes(input.clone()).unwrap().to_bytes().unwrap();
        println!("{name}: {sheets} sheets, original ZIP identical: {}", saved == input);
        let checked = check_locked(&input);
        assert_eq!(checked, sheets, "{name}");
        total += checked;
    }
    assert_eq!(total, 35);
}

#[test]
fn unregistered_positions_in_changed_extensions_refuse_atomically() {
    let base = common::p1::book(&[("Data", ""), ("Other", "")], "", "");
    let part = "xl/workbook.xml";
    for uri in ["{UNKNOWN}", "{CCE6A557-97BC-4B89-ADB6-D9C93CAAB3DF}"] {
        let workbook = text(&base, part).replace(
            "</workbook>",
            &format!(r#"<extLst><ext uri="{uri}"><newFeature ref="A1:B2"/></ext></extLst></workbook>"#),
        );
        let input = rebuild(&base, &[(part, workbook.into_bytes())]);
        let mut book = Workbook::from_bytes(input.clone()).unwrap();
        std::assert_matches!(
            book.rename_sheet("Data", "New"),
            Err(Error::UnrewritableReference { part: p, location }) if p == part && location.contains(uri)
        );
        assert!(book.to_bytes().unwrap() == input);
    }
}
