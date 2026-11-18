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
use common::p4::{Allowed, Kind, Part, Val, check, sheet, sheet_rels, workbook};
use common::{part, read, rebuild};
use ooxml_xlsx::{SheetProtection, Workbook, WorkbookProtection};

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

// ---- part 2 ----

const SET_HYPERLINK: &[Allowed] = &[
    sheet("hyperlinks", Kind::Create),
    sheet("hyperlinks/hyperlink", Kind::Create),
    sheet_rels("Relationship", Kind::Create),
    // with `opts.text`: the anchor cell's string write, composed with ADR-0003 §7
    sheet("sheetData", Kind::Cells),
    sheet("dimension", Kind::EditAttrs(&["ref"])),
    Allowed { part: Part::OtherSheetCells, path: "sheetData", kind: Kind::Cells, only: &[] },
    workbook("calcPr", Kind::Create),
    workbook("calcPr", Kind::EditAttrs(&["fullCalcOnLoad"])),
    Allowed { part: Part::CalcChainRemoval, path: "", kind: Kind::Subtree, only: &[] },
];
const REMOVE_HYPERLINK: &[Allowed] = &[sheet("hyperlinks", Kind::Remove), sheet("hyperlinks/hyperlink", Kind::Remove)];
const ADD_VALIDATION: &[Allowed] = &[
    sheet("dataValidations", Kind::Create),
    sheet("dataValidations", Kind::EditAttrs(&["count"])),
    sheet("dataValidations/dataValidation", Kind::Create),
];
const URI_DV: &[(&str, Val)] = &[("uri", Val::Is("{CCE6A557-97BC-4b89-ADB6-D9C93CAAB3DF}"))];
const URI_CF: &[(&str, Val)] = &[("uri", Val::Is("{78C0D931-6437-407d-A8EE-F0AAD7539E65}"))];
const REMOVE_VALIDATION: &[Allowed] = &[
    sheet("dataValidations", Kind::Remove),
    sheet("dataValidations", Kind::EditAttrs(&["count"])),
    sheet("dataValidations/dataValidation", Kind::Remove),
    sheet("extLst/ext", Kind::Remove).only(URI_DV),
    sheet("extLst/ext/dataValidations", Kind::EditAttrs(&["count"])),
    sheet("extLst/ext/dataValidations/dataValidation", Kind::Remove),
];
const ADD_CF: &[Allowed] = &[sheet("conditionalFormatting", Kind::Create)];
const ADD_CF_FIRST: &[Allowed] = &[
    sheet("conditionalFormatting", Kind::Create),
    sheet("conditionalFormatting/cfRule", Kind::EditAttrs(&["priority"])),
    sheet("extLst/ext/conditionalFormattings/conditionalFormatting/cfRule", Kind::EditAttrs(&["priority"])),
];
const REMOVE_CF: &[Allowed] = &[
    sheet("conditionalFormatting", Kind::Remove),
    sheet("conditionalFormatting/cfRule", Kind::Remove),
    sheet("extLst/ext", Kind::Remove).only(URI_CF),
    sheet("extLst/ext/conditionalFormattings", Kind::Remove),
    sheet("extLst/ext/conditionalFormattings/conditionalFormatting", Kind::Remove),
    sheet("extLst/ext/conditionalFormattings/conditionalFormatting/cfRule", Kind::Remove),
];
const VIEW0: &[(&str, Val)] = &[("workbookViewId", Val::Is("0"))];
const PANES: &[Allowed] = &[
    sheet("sheetViews", Kind::Create),
    sheet("sheetViews/sheetView", Kind::Create).only(VIEW0),
    sheet("sheetViews/sheetView/pane", Kind::Subtree),
    sheet("sheetViews/sheetView/selection", Kind::Subtree),
];
const VIEW_ATTRS: &[Allowed] = &[
    sheet("sheetViews", Kind::Create),
    sheet("sheetViews/sheetView", Kind::Create).only(VIEW0),
    sheet("sheetViews/sheetView", Kind::EditAttrs(&["zoomScale", "showGridLines", "rightToLeft"])).only(VIEW0),
];
const COLUMNS: &[Allowed] = &[
    sheet("cols", Kind::Create),
    sheet("cols/col", Kind::Create),
    sheet("cols/col", Kind::EditAttrs(&["min", "max", "width", "customWidth", "hidden", "outlineLevel"])),
    sheet("sheetFormatPr", Kind::Create),
    sheet("sheetFormatPr", Kind::EditAttrs(&["outlineLevelCol"])),
];
const ROWS_SETTERS: &[Allowed] = &[
    sheet("sheetData/row", Kind::Create),
    sheet("sheetData/row", Kind::EditAttrs(&["ht", "customHeight", "hidden", "outlineLevel"])),
    sheet("sheetFormatPr", Kind::Create),
    sheet("sheetFormatPr", Kind::EditAttrs(&["outlineLevelRow"])),
];
const PAGE_SETUP: &[Allowed] = &[
    sheet("pageSetup", Kind::Create),
    sheet("pageSetup", Kind::EditAttrs(&["paperSize", "scale", "fitToWidth", "fitToHeight", "orientation"])),
    sheet("sheetPr", Kind::Create),
    sheet("sheetPr/pageSetUpPr", Kind::Create),
    sheet("sheetPr/pageSetUpPr", Kind::EditAttrs(&["fitToPage"])),
];
const PRINT_PARTS: &[Allowed] = &[sheet("pageMargins", Kind::Subtree), sheet("printOptions", Kind::Subtree), sheet("headerFooter", Kind::Subtree)];
const BREAKS: &[Allowed] = &[
    sheet("rowBreaks", Kind::Create),
    sheet("rowBreaks", Kind::EditAttrs(&["count", "manualBreakCount"])),
    sheet("rowBreaks/brk", Kind::Create),
    sheet("colBreaks", Kind::Create),
    sheet("colBreaks", Kind::EditAttrs(&["count", "manualBreakCount"])),
    sheet("colBreaks/brk", Kind::Create),
];
const PROTECT_WORKBOOK: &[Allowed] = &[
    workbook("workbookProtection", Kind::Create),
    workbook(
        "workbookProtection",
        Kind::EditAttrs(&[
            "workbookPassword", "workbookPasswordCharacterSet", "workbookAlgorithmName", "workbookHashValue", "workbookSaltValue",
            "workbookSpinCount", "lockStructure", "lockWindows",
        ]),
    ),
];

/// The implemented rows, keyed by the ADR table's first-column text.
const ROWS: &[(&str, &[Allowed])] = &[
    ("`merge`", MERGE),
    ("`merge_clearing`", MERGE_CLEARING),
    ("`unmerge`", UNMERGE),
    ("`set_autofilter`", SET_AUTOFILTER),
    ("`remove_autofilter`", REMOVE_AUTOFILTER),
    ("`protect_sheet`/`unprotect_sheet`", PROTECT_SHEET),
    ("`set_hyperlink`", SET_HYPERLINK),
    ("`remove_hyperlink`", REMOVE_HYPERLINK),
    ("`add_validation`", ADD_VALIDATION),
    ("`remove_validation`", REMOVE_VALIDATION),
    ("`add_conditional_format`", ADD_CF),
    ("`add_conditional_format_first`", ADD_CF_FIRST),
    ("`remove_conditional_format`", REMOVE_CF),
    ("`freeze_panes`/`split_panes`/`unfreeze`", PANES),
    ("`set_zoom`/`set_show_gridlines`/`set_right_to_left`", VIEW_ATTRS),
    ("column setters", COLUMNS),
    ("row setters", ROWS_SETTERS),
    // implemented as `set_page_setup(&PageSetup)` (ADR-0014 §10 note)
    ("`page_setup_mut`", PAGE_SETUP),
    ("`set_margins`/`set_print_options`/`set_header_footer`", PRINT_PARTS),
    ("`add_row_break`/`add_col_break`", BREAKS),
    ("`protect_workbook`", PROTECT_WORKBOOK),
];

/// ADR rows not implemented yet (none since Phase4 part 2).
const PART2: &[&str] = &[];

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
    part2_cases(&mut out);
    out
}

/// A one-sheet book whose worksheet is `<worksheet>{xml}</worksheet>` (the
/// `r` namespace declared on the root).
fn sheet_xml_book(xml: &str) -> Vec<u8> {
    let b = book(&[("S", "")], "", r#"<calcPr calcId="191029"/>"#);
    with_part(&b, "xl/worksheets/sheet1.xml", &format!(r#"<worksheet xmlns="{SML}" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">{xml}</worksheet>"#))
}

const X14_DV: &str = r#"<extLst><ext uri="{CCE6A557-97BC-4b89-ADB6-D9C93CAAB3DF}" xmlns:x14="http://schemas.microsoft.com/office/spreadsheetml/2009/9/main"><x14:dataValidations count="2" xmlns:xm="http://schemas.microsoft.com/office/excel/2006/main"><x14:dataValidation type="list" allowBlank="1"><x14:formula1><xm:f>Other!$A$1:$A$3</xm:f></x14:formula1><xm:sqref>D1:D9</xm:sqref></x14:dataValidation><x14:dataValidation type="list"><x14:formula1><xm:f>Other!$B$1:$B$3</xm:f></x14:formula1><xm:sqref>E1</xm:sqref></x14:dataValidation></x14:dataValidations></ext></extLst>"#;

fn part2_cases(out: &mut Vec<Case>) {
    use ooxml_xlsx::{CfOperator, Cfvo, HeaderFooter, Link, LinkOptions, Margins, Orientation, PageSetup, PrintOptions, Rule, RuleKind, Validation};
    // hyperlinks: from scratch (the sheet .rels appears) with the cell text
    // composed; on Excel's 61605 (a relationship appended next to its own)
    let b = sheet_xml_book(r#"<sheetData><row r="1"><c r="A1"><v>1</v></c></row><row r="3"><c r="C3"><f>A1*2</f><v>2</v></c></row></sheetData>"#);
    let mut c = run("text", "`set_hyperlink`", b, 0, |ws| {
        ws.set_hyperlink("A1", &Link::Url("https://example.com/".into()), &LinkOptions { text: Some("go".into()), tooltip: Some("t".into()), ..LinkOptions::default() }).unwrap()
    });
    c.cells = vec!["A1".into(), "C3".into()];
    out.push(c);
    out.push(run("61605", "`set_hyperlink`", read("poi/61605.xlsx"), 0, |ws| {
        ws.set_hyperlink("Z1", &Link::Url("https://example.org/x".into()), &LinkOptions::default()).unwrap();
        ws.set_hyperlink("Z2", &Link::Location("A1".into()), &LinkOptions::default()).unwrap();
    }));
    out.push(run("61605", "`remove_hyperlink`", read("poi/61605.xlsx"), 0, |ws| {
        let r = ws.hyperlinks().unwrap()[0].range.clone();
        ws.remove_hyperlink(&r).unwrap();
    }));
    // validations
    out.push(run("template", "`add_validation`", template(), 0, |ws| ws.add_validation("A1:A9", &Validation::list(&["a", "b"])).unwrap()));
    let dv = sheet_xml_book(&format!(r#"<sheetData/><dataValidations count="2"><dataValidation type="whole" sqref="B1"><formula1>1</formula1></dataValidation><dataValidation type="list" sqref="C1"><formula1>"x"</formula1></dataValidation></dataValidations>{X14_DV}"#));
    out.push(run("existing", "`add_validation`", dv.clone(), 0, |ws| ws.add_validation("F1", &Validation::custom("F1>0")).unwrap()));
    out.push(run("2007", "`remove_validation`", dv.clone(), 0, |ws| ws.remove_validation("C1").unwrap()));
    out.push(run("x14", "`remove_validation`", dv.clone(), 0, |ws| ws.remove_validation("E1").unwrap()));
    out.push(run("x14-last", "`remove_validation`", dv, 0, |ws| {
        ws.remove_validation("E1").unwrap();
        ws.remove_validation("D1:D9").unwrap();
        ws.remove_validation("B1").unwrap();
        ws.remove_validation("C1").unwrap();
    }));
    // conditional formatting on Excel's NewStyleConditionalFormattings (x14 links and priorities)
    let ncf = || read("poi/NewStyleConditionalFormattings.xlsx");
    out.push(run("template", "`add_conditional_format`", template(), 0, |ws| ws.add_conditional_format("A1:B9", &Rule::new(RuleKind::DataBar(Cfvo::Min, Cfvo::Max, "FF638EC6".into()))).unwrap()));
    out.push(run("ncf", "`add_conditional_format`", ncf(), 0, |ws| ws.add_conditional_format("A2:A9", &Rule::new(RuleKind::CellIs(CfOperator::GreaterThan, "5".into(), None)).with_dxf(1)).unwrap()));
    out.push(run("ncf", "`add_conditional_format_first`", ncf(), 0, |ws| ws.add_conditional_format_first("A2:A9", &Rule::new(RuleKind::Expression("$A2>5".into())).with_dxf(0)).unwrap()));
    out.push(run("linked", "`remove_conditional_format`", ncf(), 0, |ws| assert!(!ws.remove_conditional_format("E2:E17", None).unwrap().broken_x14_link)));
    out.push(run("one-of-two", "`remove_conditional_format`", ncf(), 0, |ws| assert_eq!(ws.remove_conditional_format("T2:T17", Some(2)).unwrap().removed, 1)));
    // views, columns, rows on Excel's 57362 (sheetViews, cols present) and the template
    out.push(run("57362", "`freeze_panes`/`split_panes`/`unfreeze`", read("poi/57362.xlsx"), 0, |ws| ws.freeze_panes(2, 1).unwrap()));
    out.push(run("template", "`freeze_panes`/`split_panes`/`unfreeze`", template(), 0, |ws| ws.split_panes(1500, 900).unwrap()));
    out.push(run("57362", "`set_zoom`/`set_show_gridlines`/`set_right_to_left`", read("poi/57362.xlsx"), 0, |ws| {
        ws.set_zoom(85).unwrap();
        ws.set_show_gridlines(false).unwrap();
    }));
    out.push(run("57362", "column setters", read("poi/57362.xlsx"), 0, |ws| {
        ws.set_column_width(0, 1, 20.0).unwrap();
        ws.set_column_outline(4, 6, 2).unwrap();
    }));
    out.push(run("template", "column setters", template(), 0, |ws| ws.set_column_hidden(2, 2, true).unwrap()));
    out.push(run("57362", "row setters", read("poi/57362.xlsx"), 0, |ws| {
        ws.set_row_height(0, 30.0).unwrap();
        ws.set_row_hidden(500, true).unwrap();
        ws.set_row_outline(3, 1).unwrap();
    }));
    // page setup on Excel files that have pageSetup/headerFooter
    out.push(run("57523", "`page_setup_mut`", read("poi/57523.xlsx"), 0, |ws| {
        ws.set_page_setup(&PageSetup { orientation: Some(Orientation::Landscape), fit_to_width: Some(1), fit_to_page: Some(true), ..PageSetup::default() }).unwrap()
    }));
    out.push(run("62272", "`set_margins`/`set_print_options`/`set_header_footer`", read("poi/62272.xlsx"), 0, |ws| {
        ws.set_margins(&Margins { left: 1.0, ..Margins::default() }).unwrap();
        ws.set_print_options(&PrintOptions { gridlines: true, ..PrintOptions::default() }).unwrap();
        ws.set_header_footer(&HeaderFooter { odd_footer: Some("&CPage &P".into()), ..HeaderFooter::default() }).unwrap();
    }));
    out.push(run("template", "`add_row_break`/`add_col_break`", template(), 0, |ws| {
        ws.add_row_break(9).unwrap();
        ws.add_col_break(3).unwrap();
    }));
    let brk = sheet_xml_book(r#"<sheetData/><rowBreaks count="1" manualBreakCount="1"><brk id="20" max="16383" man="1"/></rowBreaks>"#);
    out.push(run("existing", "`add_row_break`/`add_col_break`", brk, 0, |ws| assert!(ws.add_row_break(10).unwrap())));
    // workbook protection: Workbook-level, over Excel's own (62272)
    for (name, input) in [("template", template()), ("62272", read("poi/62272.xlsx"))] {
        let mut wb = Workbook::from_bytes(input.clone()).unwrap();
        let sheet_part = wb.worksheet().unwrap().part_name().to_owned();
        wb.protect_workbook(&WorkbookProtection::new().password("pwd").with_salt([5; 16]).with_spin_count(100).lock_windows(true)).unwrap();
        out.push(Case { name, call: "`protect_workbook`", input, output: wb.to_bytes().unwrap(), sheet_part, cells: Vec::new() });
    }
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
        // part 2
        (find("`set_hyperlink`", "61605"), "xl/worksheets/sheet1.xml", r#"<hyperlink ref="B5""#, r#"<hyperlink ref="B6""#),
        (find("`set_hyperlink`", "61605"), "xl/worksheets/_rels/sheet1.xml.rels", "http://belinvestles.by/GIS.html", "http://x/"),
        (find("`remove_hyperlink`", "61605"), "xl/worksheets/_rels/sheet1.xml.rels", "http://belinvestles.by/GIS.html", "http://x/"),
        (find("`remove_validation`", "x14"), "xl/worksheets/sheet1.xml", "Other!$A$1:$A$3", "Other!$A$1:$A$4"),
        (find("`remove_validation`", "2007"), "xl/worksheets/sheet1.xml", r#"type="whole" sqref="B1""#, r#"type="decimal" sqref="B1""#),
        (find("`add_validation`", "existing"), "xl/worksheets/sheet1.xml", r#"type="whole" sqref="B1""#, r#"type="whole" sqref="B2""#),
        (find("`add_conditional_format_first`", "ncf"), "xl/worksheets/sheet1.xml", r#"dxfId="2""#, r#"dxfId="1""#),
        (find("`add_conditional_format_first`", "ncf"), "xl/worksheets/sheet1.xml", r#"type="iconSet" priority="7""#, r#"type="dataBar" priority="7""#),
        (find("`add_conditional_format`", "ncf"), "xl/worksheets/sheet1.xml", r#"priority="23""#, r#"priority="25""#),
        (find("`remove_conditional_format`", "one-of-two"), "xl/worksheets/sheet1.xml", r#"<conditionalFormatting sqref="T2:T17">"#, r#"<conditionalFormatting sqref="T2:T18">"#),
        (find("`remove_conditional_format`", "linked"), "xl/worksheets/sheet1.xml", "{23404A22", "{23404A23"),
        (find("`freeze_panes`/`split_panes`/`unfreeze`", "57362"), "xl/worksheets/sheet1.xml", r#"<sheetView tabSelected="1""#, r#"<sheetView zoomScale="50" tabSelected="1""#),
        (find("`set_zoom`/`set_show_gridlines`/`set_right_to_left`", "57362"), "xl/worksheets/sheet1.xml", r#"tabSelected="1""#, r#"tabSelected="0""#),
        // (`col` elements may be split and created, so a changed `col` reads as
        // a new one; `columns_are_split_edited_and_created` pins their bytes)
        (find("column setters", "57362"), "xl/worksheets/sheet1.xml", "<cols>", r#"<cols data-x="1">"#),
        (find("column setters", "57362"), "xl/worksheets/sheet1.xml", "<sheetFormatPr ", r#"<sheetFormatPr baseColWidth="3" "#),
        (find("row setters", "57362"), "xl/worksheets/sheet1.xml", " spans=\"", " spans=\"9"),
        (find("`page_setup_mut`", "57523"), "xl/worksheets/sheet1.xml", r#"verticalDpi="0""#, r#"verticalDpi="1""#),
        (find("`set_margins`/`set_print_options`/`set_header_footer`", "62272"), "xl/worksheets/sheet1.xml", "<pageSetup ", r#"<pageSetup scale="50" "#),
        (find("`add_row_break`/`add_col_break`", "existing"), "xl/worksheets/sheet1.xml", r#"<brk id="20""#, r#"<brk id="21""#),
        (find("`protect_workbook`", "62272"), "xl/workbook.xml", r#"backupFile="false""#, r#"backupFile="true""#),
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

/// Corpus sweep (ignored; `OOXML_XLSX_CORPUS=<dir>` of POI `test-data/
/// spreadsheet` at 47dcff76). On the first worksheet of every readable
/// package, each call is either refused with every byte unchanged, or its
/// output reopens and passes the byte-exact oracle for its row.
#[test]
#[ignore = "needs a local corpus: OOXML_XLSX_CORPUS=<dir>"]
fn corpus_sweep() {
    use std::collections::BTreeMap;
    use ooxml_xlsx::{Cfvo, HeaderFooter, Link, LinkOptions, Margins, Orientation, PageSetup, PrintOptions, Rule, RuleKind, Validation};
    let dir = std::env::var("OOXML_XLSX_CORPUS").expect("OOXML_XLSX_CORPUS");
    let mut files: Vec<_> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).filter(|p| p.extension().is_some_and(|x| x == "xlsx" || x == "xlsm")).collect();
    files.sort();
    // `None`: the call does not apply to this sheet
    type Op = fn(&mut ooxml_xlsx::Worksheet<'_>) -> Option<ooxml_xlsx::Result<String>>;
    let ops: [(&str, &str, Op); 25] = [
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
        // part 2: far ranges, and the sheet's existing features
        ("set_hyperlink(url)", "`set_hyperlink`", |ws| Some(ws.set_hyperlink("XFA1048570", &Link::Url("https://example.com/a b".into()), &LinkOptions::default()).map(|()| String::new()))),
        ("set_hyperlink(location)", "`set_hyperlink`", |ws| Some(ws.set_hyperlink("XFA1048571:XFB1048572", &Link::Location("A1".into()), &LinkOptions { tooltip: Some("t".into()), ..LinkOptions::default() }).map(|()| String::new()))),
        ("remove_hyperlink", "`remove_hyperlink`", |ws| {
            let r = ws.hyperlinks().ok()?.first()?.range.clone();
            Some(ws.remove_hyperlink(&r).map(|()| String::new()))
        }),
        ("add_validation", "`add_validation`", |ws| Some(ws.add_validation("XFA1048570:XFB1048571", &Validation::list(&["a", "b"])).map(|()| String::new()))),
        ("add_conditional_format", "`add_conditional_format`", |ws| Some(ws.add_conditional_format("XFA1048570:XFB1048571", &Rule::new(RuleKind::DataBar(Cfvo::Min, Cfvo::Max, "FF638EC6".into()))).map(|()| String::new()))),
        ("add_conditional_format_first", "`add_conditional_format_first`", |ws| Some(ws.add_conditional_format_first("XFA1048570", &Rule::new(RuleKind::Expression("XFA1048570>0".into()))).map(|()| String::new()))),
        ("freeze_panes", "`freeze_panes`/`split_panes`/`unfreeze`", |ws| Some(ws.freeze_panes(1, 1).map(|()| String::new()))),
        ("unfreeze", "`freeze_panes`/`split_panes`/`unfreeze`", |ws| match ws.unfreeze() {
            Ok(false) => None,
            r => Some(r.map(|_| String::new())),
        }),
        ("set_zoom", "`set_zoom`/`set_show_gridlines`/`set_right_to_left`", |ws| Some(ws.set_zoom(90).and_then(|()| ws.set_show_gridlines(false)).map(|()| String::new()))),
        ("set_column_width", "column setters", |ws| Some(ws.set_column_width(0, 2, 17.5).map(|()| String::new()))),
        ("set_column_outline(far)", "column setters", |ws| Some(ws.set_column_outline(16000, 16001, 1).map(|()| String::new()))),
        ("set_row_height", "row setters", |ws| Some(ws.set_row_height(0, 22.5).map(|()| String::new()))),
        ("set_row_hidden(far)", "row setters", |ws| Some(ws.set_row_hidden(1_048_000, true).map(|()| String::new()))),
        ("set_page_setup", "`page_setup_mut`", |ws| Some(ws.set_page_setup(&PageSetup { orientation: Some(Orientation::Landscape), fit_to_page: Some(true), fit_to_width: Some(1), ..PageSetup::default() }).map(|()| String::new()))),
        ("set_margins+print+hf", "`set_margins`/`set_print_options`/`set_header_footer`", |ws| {
            Some(ws.set_margins(&Margins::default()).and_then(|()| ws.set_print_options(&PrintOptions { gridlines: true, ..PrintOptions::default() })).and_then(|()| ws.set_header_footer(&HeaderFooter { odd_footer: Some("&CPage &P of &N".into()), ..HeaderFooter::default() })).map(|()| String::new()))
        }),
        ("add_breaks", "`add_row_break`/`add_col_break`", |ws| Some(ws.add_row_break(1_048_000).and_then(|_| ws.add_col_break(16000)).map(|_| String::new()))),
        ("protect_sheet(legacy)", "`protect_sheet`/`unprotect_sheet`", |ws| Some(ws.protect_sheet(&SheetProtection::new().password("password").legacy_hash()).map(|()| String::new()))),
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
    // protect_workbook: a workbook-level call
    for f in &files {
        let input = std::fs::read(f).unwrap();
        let Ok(mut wb) = Workbook::from_bytes(input.clone()) else { continue };
        let Ok(ws) = wb.worksheet() else { continue };
        let sheet_part = ws.part_name().to_owned();
        *applied.entry("protect_workbook").or_default() += 1;
        match wb.protect_workbook(&WorkbookProtection::new().password("p").with_salt([4; 16]).with_spin_count(10).lock_windows(true)) {
            Ok(()) => {
                let out = wb.to_bytes().unwrap();
                Workbook::from_bytes(out.clone()).unwrap_or_else(|e| panic!("{f:?} protect_workbook: reopen: {e}"));
                check(&input, &out, &sheet_part, PROTECT_WORKBOOK, &[]).unwrap_or_else(|e| panic!("{f:?} protect_workbook: {e}"));
                ok += 1;
            }
            Err(e) => {
                common::assert_unchanged(&wb, &input);
                let kind = format!("{e:?}").split(['(', ' ', '{']).next().unwrap().to_owned();
                *refused.entry(format!("protect_workbook: {kind}")).or_default() += 1;
            }
        }
    }
    println!("corpus sweep: applied {applied:?}");
    println!("corpus sweep: {opened} packages, {ok} ok, refused {refused:?}");
    assert!(ok > 0);
}
