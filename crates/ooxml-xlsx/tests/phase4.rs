//! Phase4 part 1 (ADR-0014 §3, §7, §11): behaviour, refusals (every `Err`
//! leaves every byte unchanged), the merged-anchor relaxation, calamine
//! read-back, hash vectors and failure poisoning. The byte-exact allowed
//! diffs are in `allowed_diff.rs`.

mod common;

use std::assert_matches;

use common::p1::{book, with_part};
use common::*;
use ooxml_xlsx::{Allow, CellValue, Error, SheetProtection, Workbook};

const SML: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";

/// Applies `f` to sheet `sheet` of `input` and returns the workbook and the
/// result; an `Err` must leave every byte unchanged.
fn try_on<T>(input: &[u8], sheet: usize, f: impl FnOnce(&mut ooxml_xlsx::Worksheet<'_>) -> ooxml_xlsx::Result<T>) -> (Workbook, ooxml_xlsx::Result<T>) {
    let mut wb = Workbook::from_bytes(input.to_vec()).unwrap();
    let r = {
        let mut ws = wb.worksheet_by_index(sheet).unwrap();
        f(&mut ws)
    };
    if r.is_err() {
        assert_unchanged(&wb, input);
    }
    (wb, r)
}

fn sheet_with(children: &str, sheet_data: &str) -> Vec<u8> {
    let b = book(&[("S", "")], "", "");
    with_part(&b, "xl/worksheets/sheet1.xml", &format!(r#"<worksheet xmlns="{SML}"><sheetData>{sheet_data}</sheetData>{children}</worksheet>"#))
}

// ---- merged cells (§3) ----

/// Round 1 m2: a merge over a spill (dynamic array) range is refused.
#[test]
fn merge_over_a_spill_is_refused() {
    let input = read("poi/xlookup.xlsx"); // C2 spills to C2:D2
    assert_matches!(try_on(&input, 0, |ws| ws.merge("D2:E3")).1, Err(Error::ArrayResultCell { .. }));
    assert_matches!(try_on(&input, 0, |ws| ws.merge("C2:C3")).1, Err(Error::ArrayAnchor { .. }));
    assert_matches!(try_on(&input, 0, |ws| ws.merge_clearing("D1:D2")).1, Err(Error::ArrayResultCell { .. }));
}

/// Round 1 m2: `merge_clearing` checks every cell before clearing the first:
/// B1 could be cleared, C1 cannot (a dependent under iterative
/// calculation), so nothing changes and the workbook is not poisoned.
#[test]
fn merge_clearing_checks_every_cell_first() {
    let input = book(
        &[("S", r#"<row r="1"><c r="A1"><v>1</v></c><c r="B1"><v>2</v></c><c r="C1"><v>3</v></c></row><row r="5"><c r="D5"><f>C1*2</f><v>6</v></c></row>"#)],
        "",
        r#"<calcPr calcId="191029" iterate="1"/>"#,
    );
    let (mut wb, r) = try_on(&input, 0, |ws| ws.merge_clearing("A1:C1"));
    assert_matches!(r, Err(Error::IterativeCalculation));
    wb.worksheet().unwrap().merge_clearing("A1:B1").unwrap(); // not poisoned
}

/// Round 1 m1: with a duplicated `mergeCell`, `unmerge` removes one
/// element and the range stays merged (session and tree agree).
#[test]
fn unmerge_of_a_duplicated_ref_keeps_the_other() {
    let input = sheet_with(r#"<mergeCells count="2"><mergeCell ref="A1:B2"/><mergeCell ref="A1:B2"/></mergeCells>"#, "");
    let (mut wb, r) = try_on(&input, 0, |ws| ws.unmerge("A1:B2"));
    r.unwrap();
    let mut ws = wb.worksheet().unwrap();
    assert_eq!(ws.merged_ranges().len(), 1);
    assert_matches!(ws.cell_mut(1, 1).unwrap().set_number(1.0), Err(Error::MergedCell { .. }));
    ws.unmerge("A1:B2").unwrap();
    ws.cell_mut(1, 1).unwrap().set_number(1.0).unwrap();
}

/// Round 1 m4, m5: the password never shows in `Debug`; an empty password
/// writes no hash.
#[test]
fn protection_debug_and_empty_password() {
    let p = SheetProtection::new().password("hunter2");
    let d = format!("{p:?}");
    assert!(!d.contains("hunter2") && d.contains("<redacted>"), "{d}");
    let input = sheet_with("", "");
    let (wb, r) = try_on(&input, 0, |ws| ws.protect_sheet(&SheetProtection::new().password("")));
    r.unwrap();
    let xml = String::from_utf8(part(&wb.to_bytes().unwrap(), "xl/worksheets/sheet1.xml")).unwrap();
    assert!(xml.contains("<sheetProtection ") && !xml.contains("hashValue"), "{xml}");
}

/// Round 1 M1: pivot parts are read only by `merge`; a part without a
/// location (or one that does not parse) never stops the workbook opening,
/// and a merge on that sheet is refused as overlapping.
#[test]
fn unreadable_pivot_location_does_not_block_open() {
    let pivot = read("poi/ExcelPivotTableSample.xlsx");
    let p1 = String::from_utf8(part(&pivot, "xl/pivotTables/pivotTable1.xml")).unwrap();
    let no_loc = with_part(&pivot, "xl/pivotTables/pivotTable1.xml", &p1.replace(r#"<location ref="A3:D7" firstHeaderRow="1" firstDataRow="2" firstDataCol="1"/>"#, ""));
    let broken = with_part(&pivot, "xl/pivotTables/pivotTable2.xml", "<pivotTableDefinition");
    for input in [&no_loc, &broken] {
        for sheet in 0..3 {
            let mut wb = Workbook::from_bytes(input.clone()).unwrap();
            wb.worksheet_by_index(sheet).unwrap().cell_mut(98, 25).unwrap().set_number(1.0).unwrap();
            wb.to_bytes().unwrap();
        }
    }
    // pivotTable1 is on sheet 2, pivotTable2 on sheet 3
    let refused = |input: &[u8]| (0..3).filter(|&s| matches!(try_on(input, s, |ws| ws.merge("X50:Y51")).1, Err(Error::PivotOverlap { .. }))).collect::<Vec<_>>();
    assert_eq!(refused(&no_loc), [1]);
    assert_eq!(refused(&broken), [2]);
    assert_eq!(refused(&pivot), [0usize; 0]);
}

#[test]
fn merge_refusals_change_nothing() {
    let m61605 = read("poi/61605.xlsx");
    let first = merged(&m61605)[0];
    let pivot = read("poi/ExcelPivotTableSample.xlsx");
    let arrays = read("synthetic/o1_arrays.xlsx");
    let vm = read("calamine/pictures_in_cell_and_over_cell.xlsx");
    let values = sheet_with("", r#"<row r="1"><c r="A1"><v>1</v></c><c r="B1"><v>2</v></c></row><row r="2"><c r="A2" t="inlineStr"><is><t>x</t></is></c></row>"#);
    let qt = rebuild(&sheet_with("", ""), &[("xl/worksheets/_rels/sheet1.xml.rels", br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/queryTable" Target="../queryTables/queryTable1.xml"/></Relationships>"#.to_vec())]);
    type Case<'a> = (&'a [u8], usize, &'a str, fn(&Error) -> bool);
    let cases: Vec<Case<'_>> = vec![
        (&m61605, 0, "C3", |e| matches!(e, Error::MergeSingleCell(_))),
        (&m61605, 0, "A1:B1", |e| matches!(e, Error::MergeOverlap { .. })),
        (&pivot, 0, "D1:E2", |e| matches!(e, Error::TableOverlap { .. })),
        (&pivot, 1, "B5:C9", |e| matches!(e, Error::PivotOverlap { .. })),
        (&arrays, 0, "B2:C2", |e| matches!(e, Error::ArrayResultCell { .. })),
        (&arrays, 0, "A1:B1", |e| matches!(e, Error::ArrayAnchor { .. })),
        (&vm, 0, "A2:A3", |e| matches!(e, Error::RichValueCell { .. })),
        (&values, 0, "A1:B2", |e| matches!(e, Error::MergeWouldHideValues { cells, .. } if cells == &["B1", "A2"])),
        (&qt, 0, "A1:B2", |e| matches!(e, Error::QueryTableOverlap(_))),
        (&m61605, 0, "A1:ZZZ1", |e| matches!(e, Error::InvalidReference(_))),
    ];
    assert!(!m61605.is_empty() && first.first != first.last);
    for (input, sheet, range, ok) in cases {
        let (_, r) = try_on(input, sheet, |ws| ws.merge(range));
        let e = r.expect_err(range);
        assert!(ok(&e), "{range}: {e}");
    }
}

/// The anchor keeps its value and stays writable; other cells refuse value
/// writes after `merge` and accept them after `unmerge` (ADR-0014 §3, the
/// session merge list). calamine reads the merges back.
#[test]
fn merge_session_state_and_anchor_writes() {
    let input = sheet_with("", r#"<row r="1"><c r="A1"><v>7</v></c></row>"#);
    let mut wb = Workbook::from_bytes(input).unwrap();
    {
        let mut ws = wb.worksheet().unwrap();
        ws.merge("A1:C2").unwrap();
        assert_matches!(ws.cell_mut(0, 1).unwrap().set_number(1.0), Err(Error::MergedCell { .. }));
        ws.cell_mut(0, 0).unwrap().set_number(8.0).unwrap();
        assert_eq!(ws.merged_ranges().len(), 1);
    }
    let out = wb.to_bytes().unwrap();
    assert_eq!(merged(&out).iter().map(ToString::to_string).collect::<Vec<_>>(), ["A1:C2"]);
    assert_cr(&out, &out, "S", a1("A1"), &CellValue::Number(8.0));
    let xml = sheet_xml(&out);
    assert!(xml.contains(r#"<mergeCells count="1"><mergeCell ref="A1:C2"/></mergeCells>"#), "{xml}");
    let mut wb = Workbook::from_bytes(out).unwrap();
    {
        let mut ws = wb.worksheet().unwrap();
        assert_matches!(ws.unmerge("A1:B2"), Err(Error::NoSuchMerge(_)));
        ws.unmerge("A1:C2").unwrap();
        ws.cell_mut(0, 1).unwrap().set_number(1.0).unwrap();
    }
    let out = wb.to_bytes().unwrap();
    assert!(!sheet_xml(&out).contains("mergeCells"), "the emptied container is removed");
    assert!(merged(&out).is_empty());
}

/// `merge_clearing` clears non-anchor values and formulas through Phase1:
/// the dependent loses its cache and `fullCalcOnLoad` is set; styles stay.
/// A shared-formula master among them is refused before any change.
#[test]
fn merge_clearing_goes_through_phase1() {
    let input = book(
        &[("S", r#"<row r="1"><c r="A1"><v>1</v></c><c r="B1" s="0"><f>A1+1</f><v>2</v></c><c r="C1" t="inlineStr"><is><t>t</t></is></c></row><row r="5"><c r="C5"><f>B1*2</f><v>4</v></c></row>"#)],
        "",
        r#"<calcPr calcId="191029"/>"#,
    );
    let (wb, r) = try_on(&input, 0, |ws| ws.merge_clearing("A1:C1"));
    r.unwrap();
    let out = wb.to_bytes().unwrap();
    let xml = sheet_xml(&out);
    assert!(xml.contains(r#"<c r="B1" s="0"></c>"#) || xml.contains(r#"<c r="B1" s="0"/>"#), "{xml}");
    assert!(!xml.contains("<t>t</t>"), "{xml}");
    assert!(xml.contains(r#"<c r="C5"><f>B1*2</f></c>"#), "the dependent lost its cache: {xml}");
    assert!(text(&out, "xl/workbook.xml").contains(r#"fullCalcOnLoad="1""#));
    // a shared master in a non-anchor cell
    let shared = book(&[("S", r#"<row r="1"><c r="A1"><v>1</v></c><c r="B1"><f t="shared" ref="B1:B2" si="0">A1+1</f><v>2</v></c></row><row r="2"><c r="B2"><f t="shared" si="0"/><v>1</v></c></row>"#)], "", "");
    let (_, r) = try_on(&shared, 0, |ws| ws.merge_clearing("A1:B1"));
    assert_matches!(r, Err(Error::SharedFormulaMaster { .. }));
    // the other refusals are merge's
    let (_, r) = try_on(&read("poi/61605.xlsx"), 0, |ws| ws.merge_clearing("A1:B1"));
    assert_matches!(r, Err(Error::MergeOverlap { .. }));
}

// ---- autofilter (§7) ----

fn names(bytes: &[u8]) -> Vec<String> {
    let wb = text(bytes, "xl/workbook.xml");
    wb.split("<definedName ").skip(1).map(|s| format!("<definedName {}", s.split("</definedName>").next().unwrap())).collect()
}

#[test]
fn autofilter_creates_or_reuses_the_filter_database_name() {
    // new: a hidden sheet-local name
    let input = Workbook::new().unwrap().to_bytes().unwrap();
    let (wb, r) = try_on(&input, 0, |ws| ws.set_autofilter("A1:C10"));
    r.unwrap();
    let out = wb.to_bytes().unwrap();
    assert_eq!(names(&out), [r#"<definedName name="_xlnm._FilterDatabase" localSheetId="0" hidden="1">Sheet1!$A$1:$C$10"#]);
    assert!(sheet_xml(&out).contains(r#"<autoFilter ref="A1:C10"/>"#));
    // reused: Excel's leftover name; a quoted sheet name; the sheet-level
    // sortState kept through set and remove
    for (file, sheet, range, expected) in [
        ("poi/bug66827.xlsx", 0, "A1:B20", "Main!$A$1:$B$20"),
        ("poi/57523.xlsx", 1, "A1:C5", "'Attribute Master'!$A$1:$C$5"),
    ] {
        let input = read(file);
        let before = names(&input);
        let mut wb = Workbook::from_bytes(input.clone()).unwrap();
        wb.worksheet_by_index(sheet).unwrap().set_autofilter(range).unwrap();
        let mid = wb.to_bytes().unwrap();
        let after = names(&mid);
        assert_eq!(before.len(), after.len(), "{file}: no second name");
        assert!(after.iter().any(|n| n.ends_with(expected) && n.contains("hidden=\"1\"")), "{file}: {after:?}");
        assert!(wb.worksheet_by_index(sheet).unwrap().remove_autofilter().unwrap());
        let out = wb.to_bytes().unwrap();
        let part = Workbook::from_bytes(out.clone()).unwrap().worksheet_by_index(sheet).unwrap().part_name().to_owned();
        let xml = text(&out, &part);
        assert!(xml.contains("<sortState") && !xml.contains("<autoFilter"), "{file}");
        assert_eq!(names(&out), after, "{file}: the name is kept on remove");
    }
    // the workbook-scope _FilterDatabase (#REF!) is never touched
    let input = read("poi/absolute-anchor-over-empty-sheet.xlsx");
    let (wb, r) = try_on(&input, 0, |ws| ws.set_autofilter("A1:B2"));
    r.unwrap();
    let n = names(&wb.to_bytes().unwrap());
    assert!(n.iter().any(|x| x == r#"<definedName name="_xlnm._FilterDatabase" hidden="1">#REF!"#), "{n:?}");
    assert!(n.iter().any(|x| x.contains(r#"localSheetId="0""#) && x.ends_with("picture!$A$1:$B$2")), "{n:?}");
}

#[test]
fn autofilter_quotes_the_sheet_prefix_like_names() {
    // Issue #112: Excel 365 cannot open `TRUE!$A$1:$B$2` (checked on a real
    // Excel); the prefix follows the one quoting rule of ADR-0009 §6.
    for (sheet, prefix) in [
        ("TRUE", "'TRUE'"),
        ("false", "'false'"),
        ("R1C1", "'R1C1'"),
        ("RC", "'RC'"),
        ("A1", "'A1'"),
        ("My Sheet", "'My Sheet'"),
        ("It's", "'It''s'"),
        ("Data", "Data"),
        ("Q1.2024", "Q1.2024"),
    ] {
        let escaped = sheet.replace('\'', "&apos;");
        let input = book(&[(escaped.as_str(), "")], "", "");
        let (wb, r) = try_on(&input, 0, |ws| ws.set_autofilter("A1:B2"));
        r.unwrap();
        let expected = format!(r#"<definedName name="_xlnm._FilterDatabase" localSheetId="0" hidden="1">{prefix}!$A$1:$B$2"#);
        assert_eq!(names(&wb.to_bytes().unwrap()), [expected], "{sheet}");
    }
    // a name the old code wrote unquoted is repaired when it is reused (m1)
    let old = book(&[("TRUE", "")], r#"<definedName name="_xlnm._FilterDatabase" localSheetId="0" hidden="1">TRUE!$A$1:$B$2</definedName>"#, "");
    let (wb, r) = try_on(&old, 0, |ws| ws.set_autofilter("A1:C3"));
    r.unwrap();
    assert_eq!(
        names(&wb.to_bytes().unwrap()),
        [r#"<definedName name="_xlnm._FilterDatabase" localSheetId="0" hidden="1">'TRUE'!$A$1:$C$3"#]
    );
}

#[test]
fn autofilter_refusals_change_nothing() {
    let dup = book(
        &[("S", "")],
        r#"<definedName name="_xlnm._FilterDatabase" localSheetId="0" hidden="1">S!$A$1:$A$2</definedName><definedName name="_xlnm._FilterDatabase" localSheetId="0" hidden="1">S!$A$1:$A$3</definedName>"#,
        "",
    );
    let (_, r) = try_on(&dup, 0, |ws| ws.set_autofilter("A1:B2"));
    assert_matches!(r, Err(Error::DuplicateBuiltInName { .. }));
    let used = book(&[("S", r#"<row r="1"><c r="A1"><f>ROWS(_xlnm._FilterDatabase)</f><v>1</v></c></row>"#)], "", "");
    let (_, r) = try_on(&used, 0, |ws| ws.set_autofilter("A1:B2"));
    assert_matches!(r, Err(Error::NameInUse { .. }));
    let via_name = book(&[("S", "")], r#"<definedName name="Rows">ROWS(_xlnm._FilterDatabase)</definedName>"#, "");
    let (_, r) = try_on(&via_name, 0, |ws| ws.set_autofilter("A1:B2"));
    assert_matches!(r, Err(Error::NameInUse { ref cell, .. }) if cell == "defined name Rows");
    for child in [r#"<autoFilter ref="A1:B3"><filterColumn colId="0"><filters><filter val="1"/></filters></filterColumn></autoFilter>"#, r#"<autoFilter ref="A1:B3"><sortState ref="A2:B3"><sortCondition ref="A2:A3"/></sortState></autoFilter>"#] {
        let (_, r) = try_on(&sheet_with(child, ""), 0, |ws| ws.set_autofilter("A1:C9"));
        assert_matches!(r, Err(Error::AutoFilterExists), "{child}");
    }
    let (_, r) = try_on(&read("poi/ExcelPivotTableSample.xlsx"), 0, |ws| ws.set_autofilter("A1:F2"));
    assert_matches!(r, Err(Error::TableOverlap { .. }));
    let active = sheet_with(r#"<autoFilter ref="A1:B3"><filterColumn colId="0"><filters><filter val="1"/></filters></filterColumn></autoFilter>"#, "");
    let (_, r) = try_on(&active, 0, |ws| ws.remove_autofilter());
    assert_matches!(r, Err(Error::FilterActive));
    // removal clears sheetPr/@filterMode and keeps sheetPr's other attributes
    let b = book(&[("S", "")], "", "");
    let b = with_part(&b, "xl/worksheets/sheet1.xml", &format!(r#"<worksheet xmlns="{SML}"><sheetPr filterMode="1" codeName="X"/><sheetData/><autoFilter ref="A1:A3"/></worksheet>"#));
    let (wb, r) = try_on(&b, 0, |ws| ws.remove_autofilter());
    assert!(r.unwrap());
    let xml = sheet_xml(&wb.to_bytes().unwrap());
    assert!(xml.contains(r#"<sheetPr codeName="X"/>"#) && !xml.contains("autoFilter"), "{xml}");
    let (wb, r) = try_on(&sheet_with("", ""), 0, |ws| ws.remove_autofilter());
    assert!(!r.unwrap());
    assert_unchanged(&wb, &sheet_with("", ""));
    // an existing plain autoFilter: only its ref changes
    let (wb, r) = try_on(&sheet_with(r#"<autoFilter ref="A1:B3"/>"#, ""), 0, |ws| ws.set_autofilter("A1:D9"));
    r.unwrap();
    assert!(sheet_xml(&wb.to_bytes().unwrap()).contains(r#"<autoFilter ref="A1:D9"/>"#));
}

// ---- protection (§11) ----

fn attr(xml: &str, name: &str) -> Option<String> {
    let tag = xml.split("<sheetProtection").nth(1)?.split('>').next()?;
    let i = tag.find(&format!(" {name}=\""))? + name.len() + 3;
    Some(tag[i..].split('"').next()?.to_owned())
}

/// The SHA-512 hash reproduces Excel's: the file's salt and spin count with
/// the password `pwd` (POI `TestXSSFSheet` L1357–1358) give its hashValue.
#[test]
fn protection_hash_matches_excel() {
    let input = read("poi/workbookProtection-sheet_password-2013.xlsx");
    let xml = sheet_xml(&input);
    let salt_b64 = attr(&xml, "saltValue").unwrap();
    let want = attr(&xml, "hashValue").unwrap();
    let salt = decode_b64(&salt_b64);
    let p = SheetProtection::new().password("pwd").with_salt(salt.try_into().unwrap()).with_spin_count(100_000);
    let (wb, r) = try_on(&input, 0, |ws| ws.protect_sheet(&p));
    r.unwrap();
    let out = sheet_xml(&wb.to_bytes().unwrap());
    assert_eq!(attr(&out, "hashValue").unwrap(), want);
    assert_eq!(attr(&out, "saltValue").unwrap(), salt_b64);
}

fn decode_b64(s: &str) -> Vec<u8> {
    let val = |c: u8| match c {
        b'A'..=b'Z' => c - b'A',
        b'a'..=b'z' => c - b'a' + 26,
        b'0'..=b'9' => c - b'0' + 52,
        b'+' => 62,
        _ => 63,
    };
    let bits: Vec<u8> = s.bytes().filter(|&c| c != b'=').map(val).collect();
    bits.chunks(4)
        .flat_map(|c| {
            let n = c.iter().enumerate().fold(0u32, |n, (i, &v)| n | (u32::from(v) << (18 - 6 * i)));
            let k = c.len() - 1;
            (0..k).map(move |j| (n >> (16 - 8 * j)) as u8)
        })
        .collect()
}

#[test]
fn protection_flags_replacement_and_refusals() {
    // defaults: objects/scenarios protected, selecting allowed
    let (wb, r) = try_on(&sheet_with("", ""), 0, |ws| ws.protect_sheet(&SheetProtection::new()));
    r.unwrap();
    assert!(sheet_xml(&wb.to_bytes().unwrap()).contains(r#"<sheetProtection sheet="1" objects="1" scenarios="1"/>"#));
    // allow formatting cells, forbid selecting locked cells
    let allow = Allow { format_cells: true, select_locked_cells: false, ..Allow::default() };
    let (wb, r) = try_on(&sheet_with("", ""), 0, |ws| ws.protect_sheet(&SheetProtection::new().allow(allow)));
    r.unwrap();
    let xml = sheet_xml(&wb.to_bytes().unwrap());
    assert_eq!(attr(&xml, "formatCells").as_deref(), Some("0"));
    assert_eq!(attr(&xml, "selectLockedCells").as_deref(), Some("1"));
    assert_eq!(attr(&xml, "formatRows"), None, "the default is not written");
    // a legacy password is replaced, an unknown attribute kept
    let legacy = sheet_with(r#"<sheetProtection password="EBF6" sheet="1" formatCells="0" futureFlag="x"/>"#, "");
    let (wb, r) = try_on(&legacy, 0, |ws| ws.protect_sheet(&SheetProtection::new().password("a").with_salt([1; 16]).with_spin_count(10)));
    r.unwrap();
    let xml = sheet_xml(&wb.to_bytes().unwrap());
    assert_eq!(attr(&xml, "password"), None);
    assert_eq!(attr(&xml, "formatCells"), None, "flags are replaced");
    assert_eq!(attr(&xml, "futureFlag").as_deref(), Some("x"));
    assert_eq!(attr(&xml, "algorithmName").as_deref(), Some("SHA-512"));
    // the default salt is random
    let salts: Vec<String> = (0..2)
        .map(|_| {
            let (wb, r) = try_on(&sheet_with("", ""), 0, |ws| ws.protect_sheet(&SheetProtection::new().password("a").with_spin_count(1)));
            r.unwrap();
            attr(&sheet_xml(&wb.to_bytes().unwrap()), "saltValue").unwrap()
        })
        .collect();
    assert_eq!(salts[0].len(), 24);
    assert_ne!(salts[0], salts[1]);
    // refusals
    let (_, r) = try_on(&sheet_with("", ""), 0, |ws| ws.protect_sheet(&SheetProtection::new().password("a").with_spin_count(10_000_001)));
    assert_matches!(r, Err(Error::InvalidSpinCount(10_000_001)));
    let (wb, r) = try_on(&sheet_with("", ""), 0, |ws| ws.unprotect_sheet());
    assert!(!r.unwrap());
    assert_unchanged(&wb, &sheet_with("", ""));
}

/// ADR-0003 §7.3 for Phase4: a failure (or a panic) while applying poisons
/// the workbook; nothing can be saved.
#[cfg(feature = "fault-injection")]
#[test]
fn a_failure_while_applying_poisons() {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    use ooxml_xlsx::ApplyStep;
    type Op = fn(&mut ooxml_xlsx::Worksheet<'_>) -> ooxml_xlsx::Result<()>;
    let ops: [Op; 4] = [
        |ws| ws.merge("B2:C3"),
        |ws| ws.set_autofilter("A1:B2"),
        |ws| ws.protect_sheet(&SheetProtection::new()),
        |ws| ws.merge_clearing("D1:E1"),
    ];
    let input = Workbook::new().unwrap().to_bytes().unwrap();
    for (k, op) in ops.into_iter().enumerate() {
        for panic in [false, true] {
            let mut wb = Workbook::from_bytes(input.clone()).unwrap();
            if panic {
                wb.inject_apply_panic(ApplyStep::RangeFeature);
                let r = catch_unwind(AssertUnwindSafe(|| op(&mut wb.worksheet().unwrap())));
                assert!(r.is_err(), "op {k}: the panic fired");
            } else {
                wb.inject_apply_failure(ApplyStep::RangeFeature);
                assert_matches!(op(&mut wb.worksheet().unwrap()), Err(Error::Poisoned(_)), "op {k}");
            }
            assert_matches!(wb.to_bytes(), Err(Error::Poisoned(_)), "op {k} panic {panic}");
        }
    }
}
