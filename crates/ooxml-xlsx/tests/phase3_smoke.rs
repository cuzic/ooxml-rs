//! Phase3 smoke tests (WIP): each operation on a template workbook, saved,
//! reopened, and checked against the reopened state.

use ooxml_xlsx::{SheetState, Workbook};

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
