//! Phase3 smoke tests (WIP): each operation on a template workbook, saved,
//! reopened, and checked against the reopened state.

use ooxml_xlsx::{Error, SheetState, Workbook};

fn reopen(wb: &Workbook) -> Workbook {
    Workbook::from_bytes(wb.to_bytes().unwrap()).unwrap()
}

#[test]
fn add_move_hide_rename_delete() {
    let mut wb = Workbook::new().unwrap();
    wb.add_sheet("Data", 1).unwrap();
    wb.add_sheet("My Sheet", 0).unwrap();
    assert_eq!(wb.sheet_names().collect::<Vec<_>>(), ["My Sheet", "Sheet1", "Data"]);
    wb.worksheet_by_name("Sheet1").unwrap().cell_mut(0, 0).unwrap().set_formula("Data!A1*2+'My Sheet'!B2").unwrap();
    let again = reopen(&wb);
    assert_eq!(again.sheet_names().collect::<Vec<_>>(), ["My Sheet", "Sheet1", "Data"], "rebuilt state == reopen");

    wb.move_sheet(2, 0).unwrap();
    assert_eq!(wb.sheet_names().collect::<Vec<_>>(), ["Data", "My Sheet", "Sheet1"]);
    wb.set_sheet_state("Data", SheetState::Hidden).unwrap();
    wb.set_tab_color("Sheet1", Some(0xFF0000)).unwrap();

    let rw = wb.rename_sheet("Data", "Input 2026").unwrap();
    assert_eq!(rw.len(), 1, "{rw:?}");
    let f = wb.worksheet_by_name("Sheet1").unwrap().cell(0, 0).unwrap().unwrap().formula();
    assert_eq!(f.as_deref(), Some("'Input 2026'!A1*2+'My Sheet'!B2"));

    let rw = wb.delete_sheet("My Sheet").unwrap();
    assert_eq!(rw.len(), 1, "{rw:?}");
    let f = wb.worksheet_by_name("Sheet1").unwrap().cell(0, 0).unwrap().unwrap().formula();
    assert_eq!(f.as_deref(), Some("'Input 2026'!A1*2+#REF!"));
    let again = reopen(&wb);
    assert_eq!(again.sheet_names().collect::<Vec<_>>(), wb.sheet_names().collect::<Vec<_>>());
    assert!(wb.delete_sheet("Sheet1").is_err(), "the last visible sheet");
}

#[test]
fn refusals_change_nothing() {
    let mut wb = Workbook::new().unwrap();
    let before = wb.to_bytes().unwrap();
    assert!(wb.add_sheet("Sheet1", 0).is_err());
    assert!(wb.add_sheet("a/b", 0).is_err());
    assert!(wb.set_sheet_state("Sheet1", SheetState::Hidden).is_err());
    assert!(wb.rename_sheet("Sheet1", "History").is_err());
    assert_eq!(wb.to_bytes().unwrap(), before);
}

#[test]
fn invalid_defined_name_comment_leaves_workbook_unchanged() {
    let mut workbook = Workbook::new().unwrap();
    workbook.set_defined_name("Input", "$A$1", Some(0), Some("original")).unwrap();
    let before = workbook.to_bytes().unwrap();
    assert!(workbook.set_defined_name("Input", "$B$1", Some(0), Some("bad\u{0}comment")).is_err());
    assert_eq!(workbook.to_bytes().unwrap(), before);
    workbook.set_defined_name("Input", "$B$1", Some(0), Some("corrected")).unwrap();
    let names = workbook.defined_name_nodes().unwrap();
    assert_eq!(names[0].text(), "$B$1");
}

#[test]
fn defined_name_batch_is_ordered_and_atomic() {
    let mut workbook = Workbook::new().unwrap();
    workbook.set_global_defined_name("Input", "$A$1").unwrap();
    let before = workbook.to_bytes().unwrap();
    assert!(workbook.set_global_defined_names([
        ("Input", "$B$1"), ("bad\u{0}name", "$A$1"),
    ]).is_err());
    assert_eq!(workbook.to_bytes().unwrap(), before);

    assert!(workbook.set_defined_names([
        ("Input", "$B$1", None, None), ("Outside", "$A$1", Some(1), None),
    ]).is_err());
    assert_eq!(workbook.to_bytes().unwrap(), before);

    workbook.set_global_defined_names([
        ("Input", "$B$1"), ("input", ""), ("Next", "$C$1"), ("next", "$D$1"),
    ]).unwrap();
    assert_eq!(workbook.global_defined_names().unwrap(), [("Next".into(), "$D$1".into())]);
    assert_eq!(workbook.global_defined_name_formulas("Next").unwrap(), ["$D$1"]);
    assert!(workbook.global_defined_name_formulas("next").unwrap().is_empty());
    assert!(workbook.global_defined_name_formulas("Old").unwrap().is_empty());

    // The local definition precedes the global one in the XML.
    workbook.set_defined_names([
        ("Same", "$A$2", Some(0), None), ("Same", "$B$2", None, None),
        ("same", "$C$2", None, None),
    ]).unwrap();
    let names = workbook.defined_name_nodes().unwrap();
    assert_eq!(names[1].text(), "$A$2");
    assert_eq!(names[2].text(), "$C$2");
}

#[test]
fn exact_global_defined_name_lookup_ignores_local_and_builtin_names() {
    let mut workbook = Workbook::new().unwrap();
    workbook.set_defined_names([
        ("Same", "Sheet1!$A$1", Some(0), None),
        ("Same", "Sheet1!$B$1", None, None),
        ("LocalOnly", "Sheet1!$C$1", Some(0), None),
        ("_xlnm.Print_Area", "Sheet1!$A$1:$B$1", None, None),
    ]).unwrap();

    let bytes = workbook.to_bytes().unwrap();
    let mut reopened = Workbook::from_bytes(bytes.clone()).unwrap();
    assert_eq!(reopened.global_defined_names().unwrap(), [("Same".into(), "Sheet1!$B$1".into())]);
    assert_eq!(reopened.global_defined_name_formulas("Same").unwrap(), ["Sheet1!$B$1"]);
    for name in ["same", "LocalOnly", "_xlnm.Print_Area"] {
        assert!(reopened.global_defined_name_formulas(name).unwrap().is_empty(), "{name}");
    }
    assert_eq!(reopened.to_bytes().unwrap(), bytes);
}

#[test]
fn empty_template_export_has_no_worksheet_and_preserves_source() {
    let mut workbook = ooxml_xlsx::Workbook::new().unwrap();
    let empty = ooxml_xlsx::Workbook::new_empty_at(std::time::UNIX_EPOCH).unwrap();
    let bytes = empty.to_bytes().unwrap();
    let mut package = ooxml_core::Package::from_bytes(bytes.clone()).unwrap();
    assert!(!package.part_names().any(|part| part.starts_with("xl/worksheets/")));
    let part = package.main_document_part().unwrap();
    let document = package.xml_part(&part).unwrap();
    let sheets = document.root().elements().find(|node| node.name().local() == "sheets").unwrap();
    assert_eq!(sheets.elements().count(), 0);
    assert!(matches!(ooxml_xlsx::Workbook::from_bytes(bytes.clone()), Err(ooxml_xlsx::Error::NoVisibleWorksheet)));
    let mut reopened = ooxml_xlsx::Workbook::from_bytes_allow_empty(bytes).unwrap();
    assert_eq!(reopened.sheet_names().count(), 0);
    assert!(matches!(reopened.worksheet(), Err(ooxml_xlsx::Error::NoVisibleWorksheet)));
    reopened.add_sheet("First", 0).unwrap();
    assert_eq!(reopened.sheet_name(), "First");
    let remove_sheet_list = |package: &mut ooxml_core::Package| {
        let part = package.main_document_part().unwrap();
        let document = package.xml_part_mut(&part).unwrap();
        let index = document.root().indexed_elements()
            .find(|(_, node)| node.name().local() == "sheets").unwrap().0;
        document.lens_mut::<ooxml_schema::sml::Workbook>().unwrap().remove_at(index);
    };
    remove_sheet_list(&mut package);
    let mut without_sheets_node =
        ooxml_xlsx::Workbook::from_bytes_allow_empty(package.to_bytes().unwrap()).unwrap();
    without_sheets_node.add_sheet("First", 0).unwrap();
    assert_eq!(without_sheets_node.sheet_name(), "First");

    let mut orphaned = ooxml_core::Package::from_bytes(workbook.to_bytes().unwrap()).unwrap();
    remove_sheet_list(&mut orphaned);
    assert!(matches!(
        ooxml_xlsx::Workbook::from_bytes_allow_empty(orphaned.to_bytes().unwrap()),
        Err(ooxml_xlsx::Error::Malformed(_)),
    ));
    assert!(matches!(
        ooxml_xlsx::streaming::read_workbook_manifest(|name| Ok(orphaned.read_part(name)?)),
        Err(ooxml_xlsx::Error::Malformed(_)),
    ));
    let document = orphaned.xml_part_mut(&part).unwrap();
    document.lens_mut::<ooxml_schema::sml::Workbook>().unwrap()
        .insert::<ooxml_schema::sml::Sheets>().unwrap();
    assert!(matches!(
        ooxml_xlsx::Workbook::from_bytes_allow_empty(orphaned.to_bytes().unwrap()),
        Err(ooxml_xlsx::Error::Malformed(_)),
    ));
    assert!(matches!(
        ooxml_xlsx::streaming::read_workbook_manifest(|name| Ok(orphaned.read_part(name)?)),
        Err(ooxml_xlsx::Error::Malformed(_)),
    ));
    assert!(matches!(workbook.delete_sheet("Sheet1"), Err(ooxml_xlsx::Error::LastVisibleSheet)));
    workbook.delete_sheet_allow_empty("Sheet1").unwrap();
    assert_eq!(workbook.sheet_names().count(), 0);
    assert!(ooxml_xlsx::Workbook::from_bytes_allow_empty(workbook.to_bytes().unwrap()).is_ok());
    workbook.add_sheet("Replacement", 0).unwrap();
    assert_eq!(workbook.sheet_name(), "Replacement");
}

#[test]
fn caller_scope_and_id_rejections_change_nothing() {
    let mut wb = Workbook::new().unwrap();
    let before = wb.to_bytes().unwrap();
    // A defined-name scope outside the workbook.
    let e = wb.set_defined_name("X", "$A$1", Some(5), None).unwrap_err();
    assert!(matches!(e, Error::InvalidArgument { .. }), "{e:?}");
    assert_eq!(e.cell(), None);
    assert_eq!(wb.to_bytes().unwrap(), before);
    // A reserved sheet id (0, or one already taken).
    for id in [Some(0), Some(1)] {
        let e = wb.add_sheet_with_id("S", 0, id).unwrap_err();
        assert!(matches!(e, Error::InvalidArgument { .. }), "{id:?}: {e:?}");
        assert_eq!(e.cell(), None);
        assert_eq!(wb.to_bytes().unwrap(), before);
    }
    wb.add_sheet_with_id("S", 0, Some(7)).unwrap();
    assert_eq!(wb.sheet_names().count(), 2);
}

#[test]
fn template_workbook_type_rejections_change_nothing() {
    // A workbook opened from bytes is not a template: changing its type is
    // a caller error (InvalidArgument, no cell) and changes no bytes.
    let bytes = Workbook::new().unwrap().to_bytes().unwrap();
    let mut opened = Workbook::from_bytes(bytes.clone()).unwrap();
    let e = opened.set_template_workbook_type("xlsm").unwrap_err();
    assert!(matches!(e, Error::InvalidArgument { .. }), "{e:?}");
    assert_eq!(e.cell(), None);
    assert_eq!(opened.to_bytes().unwrap(), bytes);
    // An unknown type name is rejected on a template workbook too.
    let mut template = Workbook::new().unwrap();
    let before = template.to_bytes().unwrap();
    let e = template.set_template_workbook_type("xlst").unwrap_err();
    assert!(matches!(e, Error::InvalidArgument { .. }), "{e:?}");
    assert_eq!(e.cell(), None);
    assert_eq!(template.to_bytes().unwrap(), before);
    // A known type applies: the content type changes and round-trips.
    template.set_template_workbook_type("xlsm").unwrap();
    assert_eq!(template.xml_workbook_type(), Some("xlsm"));
    let again = Workbook::from_bytes(template.to_bytes().unwrap()).unwrap();
    assert_eq!(again.xml_workbook_type(), Some("xlsm"));
}
