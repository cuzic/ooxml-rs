//! ADR-0014 §2: the allowed changes per call, **as data**, with tests
//! generated from it.
//!
//! - `ROWS` maps each implemented call to its `&[Allowed]`; `PART2` names the
//!   ADR rows not implemented yet. A test checks that together they equal the
//!   ADR table's first column, so neither side can gain a row alone.
//! - Every case runs its call on a from-scratch workbook or an Excel-authored
//!   fixture and judges the output with the byte-exact oracle
//!   (`common/p4.rs`).
//! - For each case, a mutant that changes a sibling element the row does not
//!   allow must fail the oracle.

mod common;

use common::p1::{book, with_part};
use common::p4::{Allowed, Kind, Part, Val, check, sheet, workbook};
use common::{part, read, rebuild};
use ooxml_xlsx::{SheetProtection, Workbook};

const MERGE: &[Allowed] = &[sheet("mergeCells", Kind::Container), sheet("mergeCells/mergeCell", Kind::Create)];
const MERGE_CLEARING: &[Allowed] = &[
    sheet("mergeCells", Kind::Container),
    sheet("mergeCells/mergeCell", Kind::Create),
    // composed with ADR-0003 §7's allowed diff for each cleared cell
    sheet("sheetData", Kind::Cells),
    // Phase1's write path grows an understated dimension (ADR-0003 §7's T2
    // region); found by the corpus sweep (64508.xlsx)
    sheet("dimension", Kind::EditAttrs(&["ref"])),
    Allowed { part: Part::OtherSheetCells, path: "sheetData", kind: Kind::Cells, only: &[] },
    // ADR-0003 §7: calcPr is created, or gains fullCalcOnLoad; nothing else
    workbook("calcPr", Kind::Create),
    workbook("calcPr", Kind::EditAttrs(&["fullCalcOnLoad"])),
    Allowed { part: Part::CalcChainRemoval, path: "", kind: Kind::Subtree, only: &[] },
];
const UNMERGE: &[Allowed] = &[sheet("mergeCells", Kind::Container), sheet("mergeCells/mergeCell", Kind::Remove)];
const FILTER_DB: &[(&str, Val)] = &[("name", Val::Is("_xlnm._FilterDatabase")), ("localSheetId", Val::ThisSheet)];
const SET_AUTOFILTER: &[Allowed] = &[
    sheet("autoFilter", Kind::EditAttrs(&["ref"])),
    sheet("autoFilter", Kind::Create),
    workbook("definedNames", Kind::Container),
    // only this sheet's _xlnm._FilterDatabase
    workbook("definedNames/definedName", Kind::Create).only(FILTER_DB),
    workbook("definedNames/definedName", Kind::EditAttrs(&["hidden"])).only(FILTER_DB),
    workbook("definedNames/definedName", Kind::Content).only(FILTER_DB),
];
const REMOVE_AUTOFILTER: &[Allowed] = &[sheet("autoFilter", Kind::Remove), sheet("sheetPr", Kind::EditAttrs(&["filterMode"]))];
const PROTECT_SHEET: &[Allowed] = &[
    sheet("sheetProtection", Kind::Create),
    sheet(
        "sheetProtection",
        Kind::EditAttrs(&[
            "password", "algorithmName", "hashValue", "saltValue", "spinCount", "sheet", "objects", "scenarios", "formatCells",
            "formatColumns", "formatRows", "insertColumns", "insertRows", "insertHyperlinks", "deleteColumns", "deleteRows",
            "selectLockedCells", "sort", "autoFilter", "pivotTables", "selectUnlockedCells",
        ]),
    ),
];
const UNPROTECT_SHEET: &[Allowed] = &[sheet("sheetProtection", Kind::Remove)];

/// The implemented rows, keyed by the ADR table's first-column text.
const ROWS: &[(&str, &[Allowed])] = &[
    ("`merge`", MERGE),
    ("`merge_clearing`", MERGE_CLEARING),
    ("`unmerge`", UNMERGE),
    ("`set_autofilter`", SET_AUTOFILTER),
    ("`remove_autofilter`", REMOVE_AUTOFILTER),
    ("`protect_sheet`/`unprotect_sheet`", PROTECT_SHEET),
];

/// ADR rows for Phase4 part 2 (not implemented yet).
const PART2: &[&str] = &[
    "`set_hyperlink`",
    "`remove_hyperlink`",
    "`add_validation`",
    "`remove_validation`",
    "`add_conditional_format`",
    "`add_conditional_format_first`",
    "`remove_conditional_format`",
    "`freeze_panes`/`split_panes`/`unfreeze`",
    "`set_zoom`/`set_show_gridlines`/`set_right_to_left`",
    "column setters",
    "row setters",
    "`page_setup_mut`",
    "`set_margins`/`set_print_options`/`set_header_footer`",
    "`add_row_break`/`add_col_break`",
    "`protect_workbook`",
];

fn rows(call: &str) -> &'static [Allowed] {
    ROWS.iter().find(|(c, _)| *c == call).map(|(_, r)| *r).unwrap_or_else(|| panic!("no row {call}"))
}

#[test]
fn data_table_equals_the_adr_table() {
    let adr = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/adr/0014-xlsx-phase4-range-features.md")).unwrap();
    let section = adr.split("### 2. Locality").nth(1).unwrap().split("### 3.").next().unwrap();
    let mut in_adr: Vec<String> = section
        .lines()
        .filter(|l| l.starts_with("| ") && !l.starts_with("| Call") && !l.starts_with("|---"))
        .map(|l| l.trim_start_matches("| ").split(" |").next().unwrap().trim().to_owned())
        .collect();
    let mut ours: Vec<String> = ROWS.iter().map(|(c, _)| (*c).to_owned()).chain(PART2.iter().map(|c| (*c).to_owned())).collect();
    in_adr.sort();
    ours.sort();
    assert_eq!(in_adr, ours);
}

/// One generated case: an input, the call, the output, and the cells a
/// `Cells` row allows.
struct Case {
    name: &'static str,
    call: &'static str,
    input: Vec<u8>,
    output: Vec<u8>,
    sheet_part: String,
    cells: Vec<String>,
}

fn run(name: &'static str, call: &'static str, input: Vec<u8>, sheet: usize, f: impl FnOnce(&mut ooxml_xlsx::Worksheet<'_>)) -> Case {
    let mut wb = Workbook::from_bytes(input.clone()).unwrap();
    let sheet_part;
    {
        let mut ws = wb.worksheet_by_index(sheet).unwrap();
        sheet_part = ws.part_name().to_owned();
        f(&mut ws);
    }
    let output = wb.to_bytes().unwrap();
    Case { name, call, input, output, sheet_part, cells: Vec::new() }
}

fn template() -> Vec<u8> {
    Workbook::new().unwrap().to_bytes().unwrap()
}

const SML: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";

fn neighbours() -> Vec<u8> {
    book(
        &[("S", ""), ("T", "")],
        r#"<definedName name="_xlnm._FilterDatabase" localSheetId="0" hidden="1">S!$A$1:$A$2</definedName><definedName name="_xlnm.Print_Area" localSheetId="1">T!$A$1:$C$9</definedName><definedName name="Other">S!$B$2</definedName>"#,
        r#"<calcPr calcId="191029"/>"#,
    )
}

fn cases() -> Vec<Case> {
    let mut out = Vec::new();
    // merge: from scratch, and an Excel file that already has merges
    out.push(run("template", "`merge`", template(), 0, |ws| ws.merge("B2:C3").unwrap()));
    out.push(run("61605", "`merge`", read("poi/61605.xlsx"), 0, |ws| ws.merge("G1:H2").unwrap()));
    // unmerge: an Excel merge
    out.push(run("61605", "`unmerge`", read("poi/61605.xlsx"), 0, |ws| {
        let m = ws.merged_ranges()[0];
        ws.unmerge(&m.to_string()).unwrap();
    }));
    // merge_clearing: B1 (a formula, hidden by the merge) is cleared; C5
    // depends on it and loses its cache; calcChain is not present
    let b = book(&[("S", r#"<row r="1"><c r="A1"><v>1</v></c><c r="B1"><f>A1+1</f><v>2</v></c></row><row r="5"><c r="C5"><f>B1*2</f><v>4</v></c></row>"#)], "", r#"<calcPr calcId="191029"/>"#);
    let mut c = run("formulas", "`merge_clearing`", b, 0, |ws| ws.merge_clearing("A1:B1").unwrap());
    c.cells = vec!["B1".into(), "C5".into()];
    out.push(c);
    // set_autofilter: from scratch (new name), and the three files whose
    // _FilterDatabase Excel left behind (the name is reused)
    out.push(run("template", "`set_autofilter`", template(), 0, |ws| ws.set_autofilter("A1:C10").unwrap()));
    out.push(run("bug66827", "`set_autofilter`", read("poi/bug66827.xlsx"), 0, |ws| ws.set_autofilter("A1:B20").unwrap()));
    out.push(run("57523", "`set_autofilter`", read("poi/57523.xlsx"), 1, |ws| ws.set_autofilter("A1:C5").unwrap()));
    out.push(run("absolute-anchor", "`set_autofilter`", read("poi/absolute-anchor-over-empty-sheet.xlsx"), 0, |ws| ws.set_autofilter("A1:B2").unwrap()));
    // set_autofilter next to other names: a print area, a workbook name
    // and another sheet's _FilterDatabase stay byte-identical
    out.push(run("neighbours", "`set_autofilter`", neighbours(), 1, |ws| ws.set_autofilter("A1:B4").unwrap()));
    // remove_autofilter: a plain autoFilter with sheetPr/@filterMode
    let b = book(&[("S", r#"<row r="1"><c r="A1"><v>1</v></c></row>"#)], "", "");
    let b = with_part(&b, "xl/worksheets/sheet1.xml", &format!(r#"<worksheet xmlns="{SML}"><sheetPr filterMode="1" codeName="X"/><sheetData><row r="1"><c r="A1"><v>1</v></c></row></sheetData><autoFilter ref="A1:A3"/></worksheet>"#));
    out.push(run("filterMode", "`remove_autofilter`", b, 0, |ws| assert!(ws.remove_autofilter().unwrap())));
    // protect_sheet: from scratch, over Excel's SHA-512 protection, over a
    // legacy password (every hash attribute replaced)
    let p = SheetProtection::new().password("pwd").with_salt([7; 16]).with_spin_count(1000);
    out.push(run("template", "`protect_sheet`/`unprotect_sheet`", template(), 0, |ws| ws.protect_sheet(&p).unwrap()));
    out.push(run("sha512", "`protect_sheet`/`unprotect_sheet`", read("poi/workbookProtection-sheet_password-2013.xlsx"), 0, |ws| ws.protect_sheet(&p).unwrap()));
    let b = book(&[("S", "")], "", "");
    let b = with_part(&b, "xl/worksheets/sheet1.xml", &format!(r#"<worksheet xmlns="{SML}"><sheetData/><sheetProtection password="EBF6" sheet="1" objects="1" scenarios="1" futureFlag="x"/></worksheet>"#));
    out.push(run("legacy", "`protect_sheet`/`unprotect_sheet`", b, 0, |ws| ws.protect_sheet(&p).unwrap()));
    out
}

#[test]
fn every_case_stays_within_its_row() {
    let unprotect = run("sha512", "`unprotect_sheet`", read("poi/workbookProtection-sheet_password-2013.xlsx"), 0, |ws| assert!(ws.unprotect_sheet().unwrap()));
    let all = cases();
    assert!(all.len() >= 12);
    for c in &all {
        check(&c.input, &c.output, &c.sheet_part, rows(c.call), &c.cells).unwrap_or_else(|e| panic!("{} {}: {e}", c.call, c.name));
    }
    check(&unprotect.input, &unprotect.output, &unprotect.sheet_part, UNPROTECT_SHEET, &[]).unwrap();
}

/// The oracle's own check (ADR-0014 §2 "mutation check"): changing a sibling
/// element that the row does not allow fails it.
#[test]
fn a_sibling_change_fails_the_oracle() {
    for c in cases() {
        let xml = String::from_utf8(part(&c.output, &c.sheet_part)).unwrap();
        // an attribute on sheetData (or the root when there is none): never allowed by these rows
        let bad = if let Some(i) = xml.find("<sheetData") {
            format!("{}<sheetData data-x=\"1\"{}", &xml[..i], &xml[i + "<sheetData".len()..])
        } else {
            xml.replacen("<worksheet ", "<worksheet data-x=\"1\" ", 1)
        };
        let mutated = rebuild(&c.output, &[(c.sheet_part.as_str(), bad.into_bytes())]);
        assert!(check(&c.input, &mutated, &c.sheet_part, rows(c.call), &c.cells).is_err(), "{} {}: the mutant passed", c.call, c.name);
    }
}

/// Round 1 M2: changes inside an allowed path that the row does not allow
/// fail the oracle: other names' text or attributes, other calcPr
/// attributes, an existing mergeCell, an unknown sheetProtection attribute.
/// (A *created* element's own content is not judged by the oracle; the
/// behaviour tests pin it.)
#[test]
fn a_disallowed_change_inside_an_allowed_path_fails_the_oracle() {
    let all = cases();
    let find = |call: &str, name: &str| all.iter().find(|c| c.call == call && c.name == name).unwrap_or_else(|| panic!("{call} {name}"));
    // (case, part, from, to)
    let mutants: &[(&Case, &str, &str, &str)] = &[
        (find("`set_autofilter`", "neighbours"), "xl/workbook.xml", "T!$A$1:$C$9", "T!$Z$9"),
        (find("`set_autofilter`", "neighbours"), "xl/workbook.xml", r#"<definedName name="Other">"#, r#"<definedName name="Other" hidden="1">"#),
        (find("`set_autofilter`", "neighbours"), "xl/workbook.xml", "S!$A$1:$A$2", "S!$A$1:$A$3"),
        (find("`set_autofilter`", "neighbours"), "xl/workbook.xml", r#"localSheetId="0" hidden="1""#, r#"localSheetId="0""#),
        (find("`set_autofilter`", "neighbours"), "xl/workbook.xml", r#"<definedName name="Other">S!$B$2</definedName>"#, ""),
        (find("`merge_clearing`", "formulas"), "xl/workbook.xml", "<calcPr ", r#"<calcPr calcMode="manual" "#),
        (find("`merge_clearing`", "formulas"), "xl/workbook.xml", r#"calcId="191029""#, r#"calcId="1""#),
        (find("`merge`", "61605"), "xl/worksheets/sheet1.xml", "<mergeCell ref=\"", "<mergeCell ref=\"Z"),
        (find("`unmerge`", "61605"), "xl/worksheets/sheet1.xml", "<mergeCell ref=\"", "<mergeCell ref=\"Z"),
        (find("`protect_sheet`/`unprotect_sheet`", "legacy"), "xl/worksheets/sheet1.xml", r#" futureFlag="x""#, ""),
    ];
    for (c, p, from, to) in mutants {
        let xml = String::from_utf8(part(&c.output, p)).unwrap();
        assert!(xml.contains(from), "{} {}: {from:?} not in {p}", c.call, c.name);
        let mutated = rebuild(&c.output, &[(*p, xml.replacen(from, to, 1).into_bytes())]);
        check(&c.input, &c.output, &c.sheet_part, rows(c.call), &c.cells).unwrap();
        assert!(check(&c.input, &mutated, &c.sheet_part, rows(c.call), &c.cells).is_err(), "{} {}: {from:?} -> {to:?} passed", c.call, c.name);
    }
}

/// The first cell of the sheet with a value or formula, as a 2×2 range.
fn used_2x2(ws: &ooxml_xlsx::Worksheet<'_>) -> Option<String> {
    let c = ws.cells().filter_map(Result::ok).find(|c| c.raw().elements().next().is_some()).and_then(|c| ooxml_xlsx::CellIndex::parse(c.reference()).ok())?;
    let last = ooxml_xlsx::CellIndex::new(c.row + 1, c.col + 1).ok()?;
    Some(format!("{c}:{last}"))
}

/// For `merge_clearing`: the cells whose `c` changed. Inside the range any
/// change is allowed; outside it, only Phase1's strip of a dependent's
/// cached value (`stripped`). Returns their A1 refs for the oracle.
fn cleared_cells(input: &[u8], output: &[u8], range: &str) -> Result<Vec<String>, String> {
    use common::p1::{cell_spans, sheet_parts, stripped};
    let r = ooxml_xlsx::CellRange::parse(range).unwrap();
    let mut out = Vec::new();
    for (sheet, p) in sheet_parts(input) {
        let (a, b) = (cell_spans(&part(input, &p)), cell_spans(&part(output, &p)));
        for (at, span) in &a {
            if b.get(at) == Some(span) {
                continue;
            }
            let in_range = r.contains(*at);
            if !in_range && b.get(at).map(String::as_str) != Some(stripped(span).as_str()) {
                return Err(format!("{sheet}!{at}: changed beyond a cache strip"));
            }
            out.push(at.to_string());
        }
        if let Some(at) = b.keys().find(|k| !a.contains_key(k)) {
            return Err(format!("{sheet}!{at}: a cell appeared"));
        }
    }
    Ok(out)
}

/// Corpus sweep (ignored;/// Corpus sweep (ignored; `OOXML_XLSX_CORPUS=<dir>` of POI `test-data/
/// spreadsheet` at 47dcff76). On the first worksheet of every readable
/// package, each call is either refused with every byte unchanged, or its
/// output reopens and passes the byte-exact oracle for its row.
#[test]
#[ignore = "needs a local corpus: OOXML_XLSX_CORPUS=<dir>"]
fn corpus_sweep() {
    use std::collections::BTreeMap;
    let dir = std::env::var("OOXML_XLSX_CORPUS").expect("OOXML_XLSX_CORPUS");
    let mut files: Vec<_> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).filter(|p| p.extension().is_some_and(|x| x == "xlsx" || x == "xlsm")).collect();
    files.sort();
    // `None`: the call does not apply to this sheet
    type Op = fn(&mut ooxml_xlsx::Worksheet<'_>) -> Option<ooxml_xlsx::Result<String>>;
    let ops: [(&str, &str, Op); 8] = [
        ("merge", "`merge`", |ws| Some(ws.merge("XFA1048570:XFB1048571").map(|()| String::new()))),
        ("set_autofilter", "`set_autofilter`", |ws| Some(ws.set_autofilter("XFA1048570:XFB1048571").map(|()| String::new()))),
        ("protect_sheet", "`protect_sheet`/`unprotect_sheet`", |ws| Some(ws.protect_sheet(&SheetProtection::new().password("p").with_salt([3; 16]).with_spin_count(10)).map(|()| String::new()))),
        ("unprotect_sheet", "", |ws| Some(ws.unprotect_sheet().map(|_| String::new()))),
        // round 1 m6: existing merges and autofilters, and used cells
        ("unmerge", "`unmerge`", |ws| {
            let m = ws.merged_ranges().first().copied()?;
            Some(ws.unmerge(&m.to_string()).map(|()| String::new()))
        }),
        ("remove_autofilter", "`remove_autofilter`", |ws| match ws.remove_autofilter() {
            Ok(false) => None,
            r => Some(r.map(|_| String::new())),
        }),
        ("merge(used)", "`merge`", |ws| {
            let r = used_2x2(ws)?;
            Some(ws.merge(&r).map(|()| String::new()))
        }),
        ("merge_clearing(used)", "`merge_clearing`", |ws| {
            let r = used_2x2(ws)?;
            Some(ws.merge_clearing(&r).map(|()| r))
        }),
    ];
    let (mut ok, mut opened) = (0, 0);
    let mut refused: BTreeMap<String, usize> = BTreeMap::new();
    let mut applied: BTreeMap<&str, usize> = BTreeMap::new();
    for f in &files {
        let input = std::fs::read(f).unwrap();
        let Ok(mut probe) = Workbook::from_bytes(input.clone()) else { continue };
        let Ok(ws) = probe.worksheet() else { continue };
        let sheet_part = ws.part_name().to_owned();
        opened += 1;
        for (name, row, op) in ops {
            let mut wb = Workbook::from_bytes(input.clone()).unwrap();
            let Some(r) = op(&mut wb.worksheet().unwrap()) else { continue };
            *applied.entry(name).or_default() += 1;
            match r {
                Ok(range) => {
                    let out = wb.to_bytes().unwrap();
                    Workbook::from_bytes(out.clone()).unwrap_or_else(|e| panic!("{f:?} {name}: reopen: {e}"));
                    let allowed = if row.is_empty() { UNPROTECT_SHEET } else { rows(row) };
                    let cells = if range.is_empty() { Vec::new() } else { cleared_cells(&input, &out, &range).unwrap_or_else(|e| panic!("{f:?} {name}: {e}")) };
                    check(&input, &out, &sheet_part, allowed, &cells).unwrap_or_else(|e| panic!("{f:?} {name}: {e}"));
                    ok += 1;
                }
                Err(e) => {
                    assert!(!matches!(e, ooxml_xlsx::Error::Poisoned(_)), "{f:?} {name}: {e}");
                    common::assert_unchanged(&wb, &input);
                    let kind = format!("{e:?}").split(['(', ' ', '{']).next().unwrap().to_owned();
                    *refused.entry(format!("{name}: {kind}")).or_default() += 1;
                }
            }
        }
    }
    println!("corpus sweep: applied {applied:?}");
    println!("corpus sweep: {opened} packages, {ok} ok, refused {refused:?}");
    assert!(ok > 0);
}
