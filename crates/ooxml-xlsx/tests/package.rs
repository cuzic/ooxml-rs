//! Package-level and parser/renderer tests: docs/roadmap/xlsx.md §7.2, §7.3.

mod common;

use std::assert_matches;

use common::*;
use ooxml_core::{PartStateKind, XmlDocument};
use ooxml_xlsx::{Error, Workbook};

/// P0-PKG-1: open → save with zero edits is L1 for every entry, in order,
/// including the data-descriptor fixtures. Opening parses `workbook.xml`,
/// the first worksheet, `sharedStrings.xml` and table parts, which must not
/// change the output either.
#[test]
fn pkg1_zero_edit_passthrough() {
    let fixtures = all_zip_fixtures();
    assert_eq!(fixtures.len(), 29); // 25 + 4 Phase4 fixtures (ADR-0014)
    for name in fixtures {
        let input = read(&name);
        if name == "poi/SimpleStrict.xlsx" {
            // ooxml-core refuses Strict at Package::open already (P0-PKG-3)
            continue;
        }
        let pkg = ooxml_core::Package::from_bytes(input.clone()).unwrap();
        assert_l1_except(&input, &pkg.to_bytes().unwrap(), &[]);
        match Workbook::from_bytes(input.clone()) {
            Ok(mut wb) => {
                let _ = wb.worksheet().unwrap();
                assert!(wb.package().part_names().all(|n| matches!(
                    wb.package().state(n),
                    Some(PartStateKind::Raw | PartStateKind::Parsed { dirty: false })
                )));
                assert_l1_except(&input, &wb.to_bytes().unwrap(), &[]);
            }
            Err(e) => panic!("{name}: {e}"),
        }
    }
}

/// P0-PKG-2: an encrypted (CFB) file is a typed error, not a zip error.
#[test]
fn pkg2_encrypted() {
    let r = Workbook::open(fixture("poi/protected_passtika.xlsx"));
    assert_matches!(r, Err(Error::Core(ooxml_core::Error::EncryptedPackage)));
}

/// P0-PKG-3: Strict OOXML is refused.
#[test]
fn pkg3_strict() {
    let r = Workbook::open(fixture("poi/SimpleStrict.xlsx"));
    assert_matches!(r, Err(Error::Core(ooxml_core::Error::StrictNotSupported(_))));
}

/// P0-PKG-4..7: locating "sheet1" = first visible worksheet in workbook
/// order (owner decision 2026-11-16), through relationships, never by name.
#[test]
fn pkg4_to_7_first_visible_worksheet() {
    for (name, sheet, part) in [
        ("calamine/renamed_workbook_part.xlsx", "Sheet1", "xl/sheet1.xml"),
        ("poi/reordered_sheets.xlsx", "Sheet4", "xl/worksheets/sheet1.xml"),
        ("poi/xlmmacro.xlsm", "Sheet1", "xl/worksheets/sheet1.xml"),
        ("poi/TwoSheetsOneHidden.xlsx", "Sheet2", "xl/worksheets/sheet2.xml"),
        ("poi/59746_NoRowNums.xlsx", "Features", "xl/worksheets/sheet.xml"),
        ("poi/61605.xlsx", "Лист1", "xl/worksheets/sheet1.xml"),
    ] {
        let wb = Workbook::open(fixture(name)).unwrap();
        assert_eq!((wb.sheet_name(), wb.worksheet_part()), (sheet, part), "{name}");
    }
    assert_eq!(Workbook::open(fixture("calamine/renamed_workbook_part.xlsx")).unwrap().workbook_part(), "xl/workbook2.xml");
}

/// No visible worksheet at all → `NoVisibleWorksheet` (synthetic: the
/// hidden-sheet fixture with its visible sheet hidden too).
#[test]
fn no_visible_worksheet() {
    let input = read("poi/TwoSheetsOneHidden.xlsx");
    let mut pkg = ooxml_core::Package::from_bytes(input).unwrap();
    let wb = String::from_utf8(pkg.read_part("xl/workbook.xml").unwrap()).unwrap();
    assert!(wb.contains(r#"<sheet name="Sheet2" sheetId="2" r:id="rId2"/>"#));
    let wb = wb.replace(r#"<sheet name="Sheet2" sheetId="2" r:id="rId2"/>"#, r#"<sheet name="Sheet2" sheetId="2" state="veryHidden" r:id="rId2"/>"#);
    pkg.set_part("xl/workbook.xml", wb).unwrap();
    assert_matches!(Workbook::from_bytes(pkg.to_bytes().unwrap()), Err(Error::NoVisibleWorksheet));
}

fn t1_parts(name: &str) -> Vec<String> {
    let wb = Workbook::from_bytes(read(name)).unwrap();
    let mut v = vec![wb.worksheet_part().to_owned(), wb.workbook_part().to_owned()];
    for p in ["xl/sharedStrings.xml", "xl/styles.xml"] {
        if wb.package().contains(p) {
            v.push(p.to_owned());
        }
    }
    v
}

/// P0-T1 and P0-T1b on the first worksheet, workbook, shared strings and
/// styles of every openable fixture.
#[test]
fn t1_and_t1b() {
    let mut checked = 0;
    for name in all_zip_fixtures().into_iter().filter(|n| n != "poi/SimpleStrict.xlsx") {
        let input = read(&name);
        for p in t1_parts(&name) {
            let bytes = part(&input, &p);
            let doc = XmlDocument::parse(&bytes).unwrap_or_else(|e| panic!("{name} {p}: {e}"));
            assert_eq!(doc.serialize().as_bytes(), bytes.as_slice(), "T1 {name} {p}");
            let mut forced = doc.clone();
            forced.force_rerender();
            let out = forced.serialize();
            let re = XmlDocument::parse(out.as_bytes()).unwrap_or_else(|e| panic!("T1b {name} {p}: {e}"));
            assert!(doc.root().structurally_eq(re.root()), "T1b {name} {p}");
            checked += 1;
        }
    }
    assert!(checked >= 80, "{checked}");
}

/// The shapes §7.3 names explicitly are among the T1b inputs.
#[test]
fn t1b_named_shapes_present() {
    let has = |name: &str, p: &str, needle: &str| String::from_utf8(part(&read(name), p)).unwrap().contains(needle);
    assert!(has("poi/InlineString.xlsx", "xl/worksheets/sheet1.xml", "&#x1f61c;"));
    assert!(has("poi/InlineString.xlsx", "xl/worksheets/sheet1.xml", "<x:"));
    assert!(has("poi/61605.xlsx", "xl/workbook.xml", "Лист1"));
    assert!(has("poi/style-alternate-content.xlsx", "xl/styles.xml", "mc:AlternateContent"));
    assert!(has("poi/FillWithoutColor.xlsx", "xl/worksheets/sheet1.xml", "x14ac:dyDescent"));
    assert!(has("poi/62629_target.xlsm", "xl/worksheets/sheet1.xml", "xr:uid"));
}

/// The embedded template is exactly what `template/build.py` makes from
/// `template/blank/`.
#[test]
fn template_matches_sources() {
    let template = include_bytes!("../template/blank.xlsx");
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("template/blank");
    let names: Vec<String> = entries(template).into_iter().map(|e| e.0).collect();
    assert_eq!(names[0], "[Content_Types].xml");
    for n in &names {
        let src = std::fs::read(dir.join(n)).unwrap_or_else(|e| panic!("{n}: {e}"));
        assert_eq!(part(template, n), src, "{n}: run template/build.py");
        XmlDocument::parse(&src).unwrap();
    }
    let mut on_disk = 0;
    let mut stack = vec![dir.clone()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() { stack.push(p) } else { on_disk += 1 }
        }
    }
    assert_eq!(on_disk, names.len());
}
