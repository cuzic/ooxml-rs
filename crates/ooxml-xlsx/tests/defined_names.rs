//! Defined-name listing and resolution (ADR-0023 §7, oracle 9).

mod common;

use common::p1::{book, with_part};
use common::{part, read};
use ooxml_xlsx::{CellRange, DefinedName, Error, NameScope, ResolvedName, SheetRef, Workbook};

fn names_of(bytes: &[u8]) -> Vec<DefinedName> {
    Workbook::from_bytes(bytes.to_vec()).unwrap().defined_names().unwrap()
}

fn resolve(bytes: &[u8], name: &str, scope: Option<SheetRef<'_>>) -> Result<ResolvedName, Error> {
    Workbook::from_bytes(bytes.to_vec()).unwrap().resolve_defined_name(name, scope)
}

fn range(sheet: usize, text: &str) -> ResolvedName {
    ResolvedName::Range { sheet, range: CellRange::parse(text).unwrap() }
}

#[test]
fn listing_reports_every_scope_in_document_order() {
    let bytes = book(
        &[("Sheet1", ""), ("Sheet2", "")],
        concat!(
            r#"<definedName name="G">Sheet1!$A$1</definedName>"#,
            r#"<definedName name="L" localSheetId="1">Sheet2!$B$2</definedName>"#,
            r#"<definedName name="Bad" localSheetId="9">Sheet1!$C$3</definedName>"#,
            r#"<definedName name="_xlnm.Print_Area" localSheetId="0" comment="pa" hidden="1">Sheet1!$A$1:$B$2</definedName>"#,
            r#"<definedName name="_XLNM.Upper" hidden="yes">1</definedName>"#,
        ),
        "",
    );
    let names = names_of(&bytes);
    assert_eq!(names.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(),
        ["G", "L", "Bad", "_xlnm.Print_Area", "_XLNM.Upper"]);
    assert_eq!(names[0].scope, NameScope::Workbook);
    assert_eq!(names[0].formula, "Sheet1!$A$1");
    assert_eq!(names[0].comment, None);
    assert!(!names[0].hidden && !names[0].builtin);
    assert_eq!(names[1].scope, NameScope::Sheet(1));
    assert_eq!(names[2].scope, NameScope::Invalid("9".into()));
    assert_eq!(names[3].scope, NameScope::Sheet(0));
    assert_eq!(names[3].comment.as_deref(), Some("pa"));
    assert!(names[3].hidden && names[3].builtin);
    // Built-in detection is ASCII case-insensitive; garbage hidden is false.
    assert!(names[4].builtin && !names[4].hidden);

    let bare = book(&[("Sheet1", "")], "", "");
    assert!(names_of(&bare).is_empty());

    // An unparsable localSheetId never reaches the listing: open refuses it
    // (pre-existing behaviour), so Invalid usually holds an out-of-range id.
    let garbage = book(
        &[("Sheet1", "")],
        r#"<definedName name="Garbage" localSheetId="abc">Sheet1!$D$4</definedName>"#,
        "",
    );
    assert!(Workbook::from_bytes(garbage).is_err());
}

#[test]
fn scope_selects_local_then_global() {
    let bytes = book(
        &[("Sheet1", ""), ("Sheet2", "")],
        concat!(
            r#"<definedName name="Same">Sheet1!$A$1</definedName>"#,
            r#"<definedName name="Same" localSheetId="1">Sheet2!$B$2</definedName>"#,
            r#"<definedName name="LocalOnly" localSheetId="1">Sheet2!$C$3</definedName>"#,
        ),
        "",
    );
    // No scope: the global one, even though a local shares the text.
    assert_eq!(resolve(&bytes, "Same", None).unwrap(), range(0, "A1"));
    assert_eq!(resolve(&bytes, "same", None).unwrap(), range(0, "A1"));
    // A scope picks the local one; a name local elsewhere is not found.
    assert_eq!(resolve(&bytes, "Same", Some(SheetRef::Name("Sheet2"))).unwrap(), range(1, "B2"));
    assert_eq!(resolve(&bytes, "Same", Some(SheetRef::Index(1))).unwrap(), range(1, "B2"));
    assert!(matches!(resolve(&bytes, "LocalOnly", None), Err(Error::NoSuchName(_))));
    assert_eq!(resolve(&bytes, "LocalOnly", Some(SheetRef::Name("Sheet2"))).unwrap(), range(1, "C3"));
    // No local on that sheet: falls back to the global one.
    assert_eq!(resolve(&bytes, "Same", Some(SheetRef::Name("Sheet1"))).unwrap(), range(0, "A1"));
    assert!(matches!(resolve(&bytes, "LocalOnly", Some(SheetRef::Name("Sheet1"))), Err(Error::NoSuchName(_))));
    assert!(matches!(resolve(&bytes, "Missing", None), Err(Error::NoSuchName(_))));
}

#[test]
fn duplicates_and_invalid_scopes_are_ambiguous() {
    let dupes = book(
        &[("Sheet1", "")],
        concat!(
            r#"<definedName name="Rate">Sheet1!$A$1</definedName>"#,
            r#"<definedName name="RATE">Sheet1!$B$2</definedName>"#,
        ),
        "",
    );
    assert!(matches!(resolve(&dupes, "rate", None),
        Err(Error::AmbiguousName { reason: "names fold alike", .. })));

    let bad_scope = book(
        &[("Sheet1", "")],
        concat!(
            r#"<definedName name="Rate">Sheet1!$A$1</definedName>"#,
            r#"<definedName name="RATE" localSheetId="7">Sheet1!$B$2</definedName>"#,
        ),
        "",
    );
    // Fail closed even though the valid candidate alone would resolve.
    assert!(matches!(resolve(&bad_scope, "rate", None),
        Err(Error::AmbiguousName { reason: "invalid localSheetId", .. })));
    assert!(matches!(resolve(&bad_scope, "rate", Some(SheetRef::Index(0))),
        Err(Error::AmbiguousName { reason: "invalid localSheetId", .. })));
}

#[test]
fn scope_addressing_follows_sheetref_rules() {
    let bytes = book(
        &[("Data", ""), ("DATA", ""), ("Sheet3", "")],
        r#"<definedName name="G">Sheet3!$A$1</definedName>"#,
        "",
    );
    assert!(matches!(resolve(&bytes, "G", Some(SheetRef::Name("nope"))), Err(Error::NoSuchSheet(_))));
    assert!(matches!(resolve(&bytes, "G", Some(SheetRef::Index(9))), Err(Error::NoSuchSheet(_))));
    // Exact match wins over the fold-alike pair; a fold-only match is ambiguous.
    assert_eq!(resolve(&bytes, "G", Some(SheetRef::Name("Data"))).unwrap(), range(2, "A1"));
    assert_eq!(resolve(&bytes, "G", Some(SheetRef::Name("DATA"))).unwrap(), range(2, "A1"));
    assert!(matches!(resolve(&bytes, "G", Some(SheetRef::Name("data"))), Err(Error::NoSuchSheet(_))));
}

#[test]
fn absolute_areas_resolve() {
    let bytes = book(
        &[("Sheet1", ""), ("My sheet", ""), ("O'Brien", "")],
        concat!(
            r#"<definedName name="Area">Sheet1!$A$1:$B$3</definedName>"#,
            r#"<definedName name="Cell">'My sheet'!$C$5</definedName>"#,
            r#"<definedName name="Reversed">Sheet1!$B$3:$A$1</definedName>"#,
            r#"<definedName name="Quoted">'O''Brien'!$D$6</definedName>"#,
        ),
        "",
    );
    assert_eq!(resolve(&bytes, "Area", None).unwrap(), range(0, "A1:B3"));
    assert_eq!(resolve(&bytes, "Cell", None).unwrap(), range(1, "C5"));
    // Corners normalize.
    assert_eq!(resolve(&bytes, "Reversed", None).unwrap(), range(0, "A1:B3"));
    assert_eq!(resolve(&bytes, "Quoted", None).unwrap(), range(2, "D6"));
}

#[test]
fn every_not_a_range_reason() {
    let bytes = book(
        &[("Sheet1", ""), ("Sheet2", "")],
        concat!(
            r#"<definedName name="C1">1</definedName>"#,
            r#"<definedName name="C2">"x"</definedName>"#,
            r#"<definedName name="F1">SUM(Sheet1!$A$1:$A$2)</definedName>"#,
            r#"<definedName name="F2">Sheet1!$A$1+1</definedName>"#,
            r#"<definedName name="F3">(((</definedName>"#,
            r#"<definedName name="F4">OFFSET(Sheet1!$A$1,0,0)</definedName>"#,
            r#"<definedName name="M1">Sheet1!$A$1,Sheet1!$B$2</definedName>"#,
            r#"<definedName name="M2">Sheet1!$A$1:$B$2 Sheet1!$B$2:$C$3</definedName>"#,
            r#"<definedName name="R1">Sheet1!A1</definedName>"#,
            r#"<definedName name="R2">Sheet1!$A1:$B$2</definedName>"#,
            r#"<definedName name="R3">$A$1:$B$2</definedName>"#,
            r#"<definedName name="R4">Sheet1!$A:$B</definedName>"#,
            r#"<definedName name="R5">Sheet1!$1:$2</definedName>"#,
            r#"<definedName name="E1">[1]Sheet1!$A$1</definedName>"#,
            r#"<definedName name="X1">Sheet1!#REF!</definedName>"#,
            r#"<definedName name="X2">NoSheet!$A$1</definedName>"#,
            r#"<definedName name="T1">Sheet1:Sheet2!$A$1</definedName>"#,
        ),
        "",
    );
    let reason = |name: &str| match resolve(&bytes, name, None) {
        Err(Error::NameNotARange { reason, .. }) => reason,
        other => panic!("{name}: expected NameNotARange, got {other:?}"),
    };
    for name in ["C1", "C2"] {
        assert_eq!(reason(name), "constant", "{name}");
    }
    for name in ["F1", "F2", "F3", "F4"] {
        assert_eq!(reason(name), "formula", "{name}");
    }
    for name in ["M1", "M2"] {
        assert_eq!(reason(name), "multiple areas", "{name}");
    }
    for name in ["R1", "R2", "R3", "R4", "R5"] {
        assert_eq!(reason(name), "relative reference", "{name}");
    }
    assert_eq!(reason("E1"), "external reference");
    for name in ["X1", "X2"] {
        assert_eq!(reason(name), "#REF!", "{name}");
    }
    assert_eq!(reason("T1"), "3D reference");
}

#[test]
fn names_on_non_worksheets_do_not_resolve() {
    let bytes = book(
        &[("Sheet1", ""), ("Chart1", "")],
        r#"<definedName name="OnChart">Chart1!$A$1</definedName>"#,
        "",
    );
    // The second sheet becomes a chartsheet (kind follows the rel type).
    let rels = String::from_utf8(part(&bytes, "xl/_rels/workbook.xml.rels")).unwrap();
    let rels = rels.replace(
        "<Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet\"",
        "<Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/chartsheet\"",
    );
    let bytes = with_part(&bytes, "xl/_rels/workbook.xml.rels", &rels);
    assert!(matches!(resolve(&bytes, "OnChart", None),
        Err(Error::NameNotARange { reason: "not a worksheet", .. })));
    // ... but it still counts as a sheet for scopes and listings.
    assert_eq!(names_of(&bytes).len(), 1);
    assert!(Workbook::from_bytes(bytes).unwrap().worksheet_by_index(1).is_err());
}

#[test]
fn error_kinds_cover_the_new_variants() {
    let bytes = book(&[("Sheet1", "")], r#"<definedName name="G">1</definedName>"#, "");
    let cases: Vec<(Error, &str)> = vec![
        (resolve(&bytes, "Missing", None).unwrap_err(), "NoSuchName"),
        (resolve(&bytes, "G", None).unwrap_err(), "NameNotARange"),
        (resolve(&bytes, "G", Some(SheetRef::Index(9))).unwrap_err(), "NoSuchSheet"),
    ];
    for (error, kind) in cases {
        assert_eq!(error.kind(), kind);
        assert_eq!(error.cell(), None);
        assert!(format!("{error:?}").starts_with(kind), "{error:?}");
    }
    let dupes = book(
        &[("Sheet1", "")],
        concat!(
            r#"<definedName name="G">1</definedName>"#,
            r#"<definedName name="g">2</definedName>"#,
        ),
        "",
    );
    let error = resolve(&dupes, "G", None).unwrap_err();
    assert_eq!(error.kind(), "AmbiguousName");
}

#[test]
fn reads_agree_with_the_sequential_setters() {
    // Names written through the public setters list and resolve.
    let mut wb = Workbook::new().unwrap();
    wb.add_sheet("Data", 1).unwrap();
    wb.set_defined_names([
        ("Total", "Data!$A$1:$A$9", None, Some("sum me")),
        ("Total", "Data!$B$1", Some(1), None),
    ]).unwrap();
    assert_eq!(wb.resolve_defined_name("Total", None).unwrap(), range(1, "A1:A9"));
    assert_eq!(wb.resolve_defined_name("Total", Some(SheetRef::Name("Data"))).unwrap(), range(1, "B1"));
    let names = wb.defined_names().unwrap();
    assert_eq!(names.len(), 2);
    assert_eq!(names[0].comment.as_deref(), Some("sum me"));
    // The fixtures keep working through the new listing.
    for fixture in ["minimal", "poi/61605.xlsx"] {
        let bytes = read(fixture);
        let mut wb = Workbook::from_bytes(bytes).unwrap();
        let _ = wb.defined_names().unwrap();
    }
}
