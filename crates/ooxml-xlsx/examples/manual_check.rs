//! Writes the Phase0, Phase1 and Phase2 outputs the project owner opens in Microsoft
//! 365 Excel (roadmap §7.6, ADR-0001 Consequences, ADR-0003 §7.4: no Excel in
//! CI; ADR-0006 "Test oracles" 10 for Phase2), plus a checklist.
//!
//! ```sh
//! cargo run -p ooxml-xlsx --example xlsx_manual_check -- <outdir>
//! ```
//!
//! `<outdir>/` gets the edited files, `originals/` (unedited inputs, to tell
//! a pre-existing repair dialog from one this crate caused) and
//! `CHECKLIST.md`.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use ooxml_xlsx::{Comment, Note, PersonId, CellRange, Col, Table, TableStyle, Total, Anchor, Border, CellIndex, Image, Size, CellValue, Color, ExcelDateTime, Fill, Font, Format, Result, Side, Tint, Underline, Workbook, Worksheet};

type BoxResult<T> = std::result::Result<T, Box<dyn std::error::Error>>;
/// (original for `originals/`, edited output)
type Built = BoxResult<(Option<Vec<u8>>, Vec<u8>)>;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../reference");

struct Case {
    out: &'static str,
    /// `None` = `Workbook::new()`.
    input: Option<&'static str>,
    edits: &'static str,
    look_for: &'static str,
    edit: fn(&mut Worksheet<'_>) -> Result<()>,
}

const CASES: &[Case] = &[
    Case {
        out: "01-minimal.xlsx",
        input: Some("round_trip_corpus/fixture_minimal.xlsx"),
        edits: "A1 = \"Name (edited)\", A2 = \"  padded \", B2 = 91, C2 (was TRUE) = 0, D2 (empty, bold style) = 7, F5 = \"new\"",
        look_for: "A1 and D2 bold; C2 shows 0 (not FALSE); A2 keeps its leading/trailing spaces (check in the formula bar); used range A1:F5. This input is hand-made (fills count=1): if Excel also repairs originals/01-minimal.xlsx, the dialog is not ours.",
        edit: |ws| {
            ws.write_string(0, 0, "Name (edited)")?;
            ws.write_string(1, 0, "  padded ")?;
            ws.write_number(1, 1, 91.0)?;
            ws.write_number(1, 2, 0.0)?;
            ws.write_number(1, 3, 7.0)?;
            ws.write_string(4, 5, "new")
        },
    },
    Case {
        out: "02-61605-cyrillic-x14ac.xlsx",
        input: Some("fixtures/xlsx/poi/61605.xlsx"),
        edits: "B2 (self-closing styled cell) = 2, B5 (shared string) = \"edited\", A20 (new row after row 15) = \"new row\"",
        look_for: "Sheet tab Лист1; B2 keeps its style; merged rows 1/3/4/6/8/9 unchanged; the hyperlink on B5's row still works; A20 present; Ctrl+End goes to E20.",
        edit: |ws| {
            ws.write_number(1, 1, 2.0)?;
            ws.write_string(4, 1, "edited")?;
            ws.write_string(19, 0, "new row")
        },
    },
    Case {
        out: "03-fillwithoutcolor-row2.xlsx",
        input: Some("fixtures/xlsx/poi/FillWithoutColor.xlsx"),
        edits: "B2 (row 2 was a self-closing `<row/>`) = 5; K61 = \"x\"",
        look_for: "Row 2 keeps its height and thick bottom border; conditional formatting still shows; second sheet 'User Guide' intact.",
        edit: |ws| {
            ws.write_number(1, 1, 5.0)?;
            ws.write_string(60, 10, "x")
        },
    },
    Case {
        out: "04-reordered-sheets-empty-sheetdata.xlsx",
        input: Some("fixtures/xlsx/poi/reordered_sheets.xlsx"),
        edits: "first tab Sheet4 (was `<sheetData/>`): A1 = 1, C3 = \"x\"",
        look_for: "Tab order Sheet4, Sheet2, Sheet3, Sheet1; values on Sheet4 only.",
        edit: |ws| {
            ws.write_number(0, 0, 1.0)?;
            ws.write_string(2, 2, "x")
        },
    },
    Case {
        out: "05-pivot-and-table.xlsx",
        input: Some("fixtures/xlsx/poi/ExcelPivotTableSample.xlsx"),
        edits: "A1 = \"outside\", E2 (table body) = 9 (header row E1:G1 is guarded because the table is a pivot source)",
        look_for: "Table E1:G4 still a table with the same headers; both pivot tables open (they may show stale data until Refresh: that is Excel's normal behaviour, roadmap §4).",
        edit: |ws| {
            ws.write_string(0, 0, "outside")?;
            ws.write_number(1, 4, 9.0)
        },
    },
    Case {
        out: "06-pictures-in-cell.xlsx",
        input: Some("fixtures/xlsx/calamine/pictures_in_cell_and_over_cell.xlsx"),
        edits: "A2 = 1 (A1/A3 hold images in cells and are refused by design)",
        look_for: "Images in A1 and A3 still shown; the floating picture still there.",
        edit: |ws| ws.write_number(1, 0, 1.0),
    },
    Case {
        out: "07-52348-inline-rich.xlsx",
        input: Some("fixtures/xlsx/poi/52348.xlsx"),
        edits: "B11 = \"  padded \" (inline string, xml:space=preserve), R11 = 3",
        look_for: "Rich-text inline strings elsewhere keep their formatting; B11 keeps outer spaces; merged cells intact.",
        edit: |ws| {
            ws.write_string(10, 1, "  padded ")?;
            ws.write_number(10, 17, 3.0)
        },
    },
    Case {
        out: "08-from-scratch.xlsx",
        input: None,
        edits: "Workbook::new() → add_worksheet() → A1 = \"a\", B2 = 2, C3 = \"  spaced  \", A5 = 1E300",
        look_for: "One sheet 'Sheet1'; Calibri 11; File > Info shows a fresh Created date; no repair dialog (this exercises the embedded template: theme, styles, app.xml).",
        edit: |ws| {
            ws.write_string(0, 0, "a")?;
            ws.write_number(1, 1, 2.0)?;
            ws.write_string(2, 2, "  spaced  ")?;
            ws.write_number(4, 0, 1e300)
        },
    },
    Case {
        out: "09-62629-macro.xlsm",
        input: Some("fixtures/xlsx/poi/62629_target.xlsm"),
        edits: "first tab 'IT': A1 = \"macro book\"",
        look_for: "Opens as a macro-enabled workbook (macros stay disabled unless you enable them — do not enable them, the file is third-party test data); form controls on sheet 'Report' intact.",
        edit: |ws| ws.write_string(0, 0, "macro book"),
    },
    Case {
        out: "10-hancom-x-prefix.xlsx",
        input: Some("fixtures/xlsx/poi/style-alternate-content.xlsx"),
        edits: "E50 = \"x\" (the sheet uses the x: prefix, so new elements are written as `<x:row>`/`<x:c>`)",
        look_for: "Korean font names in styles survive; E50 present.",
        edit: |ws| ws.write_string(49, 4, "x"),
    },
    Case {
        out: "11-hidden-first-sheet.xlsx",
        input: Some("fixtures/xlsx/poi/TwoSheetsOneHidden.xlsx"),
        edits: "the first *visible* worksheet (Sheet2; Sheet1 is hidden): B3 = 3",
        look_for: "Sheet1 still hidden (Unhide shows it, unchanged); B3 on Sheet2.",
        edit: |ws| ws.write_number(2, 1, 3.0),
    },
];

/// Phase1 (ADR-0003 §7.4): edits in formula workbooks; the whole workbook
/// is available to the edit.
struct P1Case {
    out: &'static str,
    input: &'static str,
    edits: &'static str,
    look_for: &'static str,
    edit: fn(&mut Workbook) -> Result<()>,
}

const P1_CASES: &[P1Case] = &[
    P1Case {
        out: "12-shared-formulas-value.xlsx",
        input: "fixtures/xlsx/poi/shared_formulas.xlsx",
        edits: "Label!B5 = \"EDITED-B5\" (A2:A41 is a shared formula =B2…; A5's cached value is removed, fullCalcOnLoad set, calcChain kept)",
        look_for: "**XL 7.2-1.** A5 shows EDITED-B5 (recalculated on open); every other A cell shows its B neighbour. No repair dialog.",
        edit: |wb| wb.worksheet_by_name("Label")?.cell_mut(4, 1)?.set_string("EDITED-B5"),
    },
    P1Case {
        out: "13-matrix-array.xlsx",
        input: "fixtures/xlsx/poi/MatrixFormulaEvalTestData.xlsx",
        edits: "Sheet1!B2 = 100 (precedent of the array H2:J4 = B2:D4 + E2:G4; the other arrays use TRANSPOSE/MMULT/… and lose their caches too)",
        look_for: "**XL 7.2-2.** H2 = 100 + E2; every array range shows values (none blank after open). No repair dialog.",
        edit: |wb| wb.worksheet()?.cell_mut(1, 1)?.set_number(100.0),
    },
    P1Case {
        out: "14-xlookup-dynamic-array.xlsx",
        input: "fixtures/xlsx/poi/xlookup.xlsx",
        edits: "Sheet1!B2 = the value of B7 (the lookup key of the dynamic array C2:D2 = XLOOKUP(B2, B5:B14, C5:D14))",
        look_for: "**XL 7.2-3.** C2:D2 show C7:D7 (spilled, blue spill border when selected). Sheets 2 and 3 recalculated too. No repair dialog.",
        edit: |wb| {
            let key = wb.worksheet()?.cell(6, 1)?.map(|c| c.value()).transpose()?;
            let mut ws = wb.worksheet()?;
            match key {
                Some(CellValue::String(s)) => ws.cell_mut(1, 1)?.set_string(&s),
                Some(CellValue::Number(n)) => ws.cell_mut(1, 1)?.set_number(n),
                _ => ws.cell_mut(1, 1)?.set_string("?"),
            }
        },
    },
    P1Case {
        out: "15-formula-write-no-calcchain.xlsx",
        input: "fixtures/xlsx/poi/shared_formulas.xlsx",
        edits: "Label!C50 = formula `A2&\"!\"` (new formula cell: calcChain.xml, its relationship and content-type override are removed)",
        look_for: "**XL 7.2-6.** C50 shows A2's text followed by \"!\"; the formula bar shows =A2&\"!\". No repair dialog.",
        edit: |wb| wb.worksheet_by_name("Label")?.cell_mut(49, 2)?.set_formula("A2&\"!\""),
    },
    P1Case {
        out: "16-shared-master-hand-off.xlsx",
        input: "fixtures/xlsx/poi/NewStyleConditionalFormattings.xlsx",
        edits: "CF!D2 = 5 (D2 was the master of the shared formula D2:U17 = $A2; E2 becomes the master)",
        look_for: "**XL 7.2-7.** D2 = 5; E2:U17 still show their row's A value (formula bar of E2: =$A2, of U17: =$A17); data bars and icon sets intact. No repair dialog.",
        edit: |wb| wb.worksheet()?.cell_mut(1, 3)?.set_number(5.0),
    },
    P1Case {
        out: "17-manual-calc-mode.xlsx",
        input: "fixtures/xlsx/synthetic/o1_manual.xlsx",
        edits: "Sheet1!A1 = 5 (calcMode=manual: B1 = A1*2 loses its cached value; fullCalcOnLoad is *not* set)",
        look_for: "**XL 7.2-9.** Record what B1 shows **before** pressing F9 (blank? 0? 10?), then press F9: B1 = 10. C1 = 3. No repair dialog.",
        edit: |wb| wb.worksheet()?.cell_mut(0, 0)?.set_number(5.0),
    },
    P1Case {
        out: "18-cross-sheet-names-3d.xlsx",
        input: "fixtures/xlsx/synthetic/o1_precise.xlsx",
        edits: "Sheet1!A1 = 50 (cross-sheet, 3-D, whole-column and SUMIF dependents lose their caches)",
        look_for: "Sheet1!B1 = 150; Sheet2!B1 = 100, C1 = 101; Sheet3!A1 = 202, C1 = 60, E1 = 53, J1 = 150; untouched: Sheet1!C1 = 14, Sheet3!G1 = 2, H1 = 1, I1 = 10. No repair dialog.",
        edit: |wb| wb.worksheet()?.cell_mut(0, 0)?.set_number(50.0),
    },
    P1Case {
        out: "19-formula-removal.xlsx",
        input: "fixtures/xlsx/synthetic/o1_precise.xlsx",
        edits: "Sheet2!B1: formula removed (=Sheet1!A1*2); calcChain.xml removed",
        look_for: "Sheet2!B1 empty, Sheet2!C1 = 1, Sheet3!A1 = 2. No repair dialog.",
        edit: |wb| wb.worksheet_by_name("Sheet2")?.cell_mut(0, 1)?.remove_formula(),
    },
    P1Case {
        out: "20-shared-master-not-top-left.xlsx",
        input: "fixtures/xlsx/synthetic/o1_shared.xlsx",
        edits: "Sheet2!D2 = 99 (master of C2:E3 = $A2+D$1, not its top-left cell; C2 takes over as master)",
        look_for: "C2 = 11, E2 = 31, C3 = 12, D3 = 22, E3 = 32 (formula bar of E3: =$A3+E$1); D2 = 99. No repair dialog.",
        edit: |wb| wb.worksheet_by_name("Sheet2")?.cell_mut(1, 3)?.set_number(99.0),
    },
    P1Case {
        out: "21-lookup-short-result-vector.xlsx",
        input: "fixtures/xlsx/synthetic/o1_everything_lookup.xlsx",
        edits: "Sheet1!E2 = 222, F1 = 111 (B1 = LOOKUP(2, A1:A3, E1): the result vector E1 is shorter than the lookup vector; ADR-0003 R2-M1 2)",
        look_for: "**Record what B1 shows**: 222 means Excel reads down past E1, 111 means across, 0/#N/A something else. (Our rule treats LOOKUP as 'depends on everything' either way.)",
        edit: |wb| {
            let mut ws = wb.worksheet()?;
            ws.cell_mut(1, 4)?.set_number(222.0)?;
            ws.cell_mut(0, 5)?.set_number(111.0)
        },
    },
];

/// Phase3 (ADR-0009 §9 test oracle 9, XL items): sheet operations.
const P3_CASES: &[P1Case] = &[
    P1Case {
        out: "30-rename-51850.xlsx",
        input: "fixtures/xlsx/poi/51850.xlsx",
        edits: "rename `more comments` -> `Mehr Kommentare 2026` (x14 data validation on it; cell formulas, names and links follow)",
        look_for: "**XL 9(a).** Tab shows the new name; data validation lists on the renamed sheet still work; no repair dialog.",
        edit: |wb| wb.rename_sheet("more comments", "Mehr Kommentare 2026").map(drop),
    },
    P1Case {
        out: "31-delete-hidden-sheet.xlsx",
        input: "fixtures/xlsx/poi/TwoSheetsOneHidden.xlsx",
        edits: "delete the hidden `Sheet1` (only `Sheet2` stays, active and selected)",
        look_for: "**XL 9(a).** One tab, `Sheet2`, selected; no repair dialog; Unhide is greyed out.",
        edit: |wb| wb.delete_sheet("Sheet1").map(drop),
    },
    P1Case {
        out: "32-reverse-reordered.xlsx",
        input: "fixtures/xlsx/poi/reordered_sheets.xlsx",
        edits: "reverse the tab order (Sheet4, Sheet2, Sheet3, Sheet1 -> Sheet1, Sheet3, Sheet2, Sheet4)",
        look_for: "**XL 9(a).** Tabs in the new order; each tab shows its own original content; no repair dialog.",
        edit: |wb| {
            let n = wb.sheet_names().count();
            for i in 0..n {
                wb.move_sheet(n - 1, i)?;
            }
            Ok(())
        },
    },
    P1Case {
        out: "33-delete-ref-errors-3d.xlsx",
        input: "fixtures/xlsx/synthetic/o1_precise.xlsx",
        edits: "delete `Sheet2` (formulas on it are gone; references to it become a bare #REF!; 3-D spans with it as an endpoint move inward)",
        look_for: "**XL 9(b), 9(f).** Formulas that pointed at Sheet2 show #REF! (formula bar: `#REF!`, not `Sheet2!#REF!`); 3-D sums still compute over the remaining sheets; no repair dialog.",
        edit: |wb| wb.delete_sheet("Sheet2").map(drop),
    },
    P1Case {
        out: "34-rename-chart-source.xlsx",
        input: "fixtures/xlsx/poi/57362.xlsx",
        edits: "rename `Sheet1` -> `Daten 1` (the chart's series formulas follow: `'Daten 1'!$A$1:$B$1`)",
        look_for: "The chart still plots the same data; Select Data shows ='Daten 1'!…; no repair dialog.",
        edit: |wb| wb.rename_sheet("Sheet1", "Daten 1").map(drop),
    },
    P1Case {
        out: "35-add-hide-tab-colour.xlsx",
        input: "fixtures/xlsx/synthetic/o1_precise.xlsx",
        edits: "add `New 1` at the front, hide `Sheet3`, red tab colour on `Sheet1`",
        look_for: "Tabs: New 1, Sheet1 (red), Sheet2, Defines; Sheet3 under Unhide; formulas recalculate to the same values as the original; no repair dialog.",
        edit: |wb| {
            wb.add_sheet("New 1", 0)?;
            wb.set_sheet_state("Sheet3", ooxml_xlsx::SheetState::Hidden)?;
            wb.set_tab_color("Sheet1", Some(0xFF0000))
        },
    },
];

/// Phase4 part 1 (ADR-0014 §3, §7, §11, Test oracles 7).
const P4_CASES: &[P1Case] = &[
    P1Case {
        out: "22-merge.xlsx",
        input: "fixtures/xlsx/poi/61605.xlsx",
        edits: "Лист1!G1:H2 merged; its anchor G1 = \"MERGED\" (ADR-0014 §3: the anchor is writable)",
        look_for: "G1:H2 is one merged cell showing MERGED; the six existing merges unchanged. No repair dialog.",
        edit: |wb| {
            let mut ws = wb.worksheet()?;
            ws.merge("G1:H2")?;
            ws.cell_mut(0, 6)?.set_string("MERGED")
        },
    },
    P1Case {
        out: "23-merge-clearing.xlsx",
        input: "fixtures/xlsx/synthetic/o1_precise.xlsx",
        edits: "Sheet1!A1:B1 merged with merge_clearing (B1's formula is cleared; its dependents lose their caches; fullCalcOnLoad set)",
        look_for: "A1:B1 is one merged cell showing A1's value; formulas that read B1 recalculate with B1 empty. No repair dialog.",
        edit: |wb| wb.worksheet()?.merge_clearing("A1:B1"),
    },
    P1Case {
        out: "24-unmerge.xlsx",
        input: "fixtures/xlsx/poi/61605.xlsx",
        edits: "the first merged range of Лист1 is unmerged",
        look_for: "That range shows separate cells; the other five merges unchanged. No repair dialog.",
        edit: |wb| {
            let mut ws = wb.worksheet()?;
            let m = ws.merged_ranges()[0];
            ws.unmerge(&m.to_string())
        },
    },
    P1Case {
        out: "25-autofilter.xlsx",
        input: "fixtures/xlsx/poi/bug66827.xlsx",
        edits: "Main!A1:B20 autofilter; the leftover hidden _xlnm._FilterDatabase name is reused (ADR-0014 §7, B1)",
        look_for: "Filter arrows on A1 and B1; filtering works; Formulas > Name Manager shows no duplicate name error. No repair dialog.",
        edit: |wb| wb.worksheet()?.set_autofilter("A1:B20"),
    },
    P1Case {
        out: "26-autofilter-removed.xlsx",
        input: "fixtures/xlsx/poi/bug66827.xlsx",
        edits: "Main!A1:B20 autofilter set, then removed (the sheet's sortState and the name are kept)",
        look_for: "No filter arrows. Data > Sort still remembers the last sort (if Excel shows it). No repair dialog.",
        edit: |wb| {
            let mut ws = wb.worksheet()?;
            ws.set_autofilter("A1:B20")?;
            ws.remove_autofilter().map(drop)
        },
    },
    P1Case {
        out: "27-protected-sha512.xlsx",
        input: "fixtures/xlsx/poi/61605.xlsx",
        edits: "Лист1 protected with password `pwd` (SHA-512, random salt, spin count 100,000; ADR-0014 §11)",
        look_for: "Typing into a cell is refused (sheet protected). Review > Unprotect Sheet asks for a password; `pwd` unprotects, `x` does not. No repair dialog.",
        edit: |wb| wb.worksheet()?.protect_sheet(&ooxml_xlsx::SheetProtection::new().password("pwd")),
    },
    P1Case {
        out: "28-autofilter-sheet-true.xlsx",
        input: "fixtures/xlsx/synthetic/p4_sheet_true.xlsx",
        edits: "TRUE!A1:B2 autofilter; the new _xlnm._FilterDatabase is written `'TRUE'!$A$1:$B$2`, quoted (issue #112: Excel cannot open the unquoted `TRUE!` form)",
        look_for: "Opens with no repair dialog (before issue #112 Excel could not open it); filter arrows on A1 and B1.",
        edit: |wb| wb.worksheet()?.set_autofilter("A1:B2"),
    },
];

const P5B_CASE: Case = Case {
    out: "39-p5b-table.xlsx",
    input: None,
    edits: "Create Sales at A1:D4 with a totals row and a calculated column. Rename A1 from Region to Market.",
    look_for: "Sales spans A1:D4 and its filter excludes row 4. A1 and the first table column name are Market. A4 says Total, B4 is 7 and D2:D3 calculate to 4 and 10.",
    edit: |ws| {
        ws.write_string(1, 0, "North")?;
        ws.write_string(2, 0, "South")?;
        ws.write_number(1, 1, 2.0)?;
        ws.write_number(2, 1, 5.0)?;
        ws.add_table(Table::new(CellRange::parse("A1:D4")?)
            .name("Sales")
            .columns([Col::new("Region").total_label("Total"), Col::new("Units, shipped").total(Total::Sum),
                Col::new("Notes"), Col::new("Doubled").formula("[@[Units, shipped]]*2")])
            .totals_row(true)
            .style(TableStyle::Medium9))?;
        ws.write_string(0, 0, "Market")
    },
};

fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let out_dir = PathBuf::from(std::env::args().nth(1).ok_or("usage: manual_check <outdir>")?);
    let originals = out_dir.join("originals");
    std::fs::create_dir_all(&originals)?;
    let mut md = String::from(CHECKLIST_HEAD);
    for case in CASES {
        let mut wb = match case.input {
            Some(input) => {
                let path = Path::new(ROOT).join(input);
                std::fs::copy(&path, originals.join(case.out))?;
                Workbook::open(&path)?
            }
            None => Workbook::new()?,
        };
        let sheet = wb.sheet_name().to_owned();
        (case.edit)(&mut wb.worksheet()?)?;
        wb.save(out_dir.join(case.out))?;
        writeln!(
            md,
            "| [ ] | `{}` | {} | {} | {} | |",
            case.out,
            case.input.map_or("(embedded template)".to_owned(), |i| format!("`{i}`")),
            format_args!("sheet `{sheet}`: {}", case.edits),
            case.look_for
        )?;
    }
    md.push_str(CHECKLIST_P1_HEAD);
    for case in P1_CASES {
        let path = Path::new(ROOT).join(case.input);
        std::fs::copy(&path, originals.join(case.out))?;
        let mut wb = Workbook::open(&path)?;
        (case.edit)(&mut wb)?;
        wb.save(out_dir.join(case.out))?;
        writeln!(md, "| [ ] | `{}` | `{}` | {} | {} | |", case.out, case.input, case.edits, case.look_for)?;
    }
    md.push_str(CHECKLIST_P2_HEAD);
    for case in P2_CASES {
        let (original, output) = (case.build)()?;
        if let Some(o) = original {
            std::fs::write(originals.join(case.out), o)?;
        }
        std::fs::write(out_dir.join(case.out), output)?;
        writeln!(md, "| [ ] | `{}` | {} | {} | {} | |", case.out, case.input, case.edits, case.look_for)?;
    }

    md.push_str(CHECKLIST_P3_HEAD);
    for case in P3_CASES {
        let path = Path::new(ROOT).join(case.input);
        std::fs::copy(&path, originals.join(case.out))?;
        let mut wb = Workbook::open(&path)?;
        (case.edit)(&mut wb)?;
        wb.save(out_dir.join(case.out))?;
        writeln!(md, "| [ ] | `{}` | `{}` | {} | {} | |", case.out, case.input, case.edits, case.look_for)?;
    }

    md.push_str(CHECKLIST_P4_HEAD);
    for case in P4_CASES {
        let path = Path::new(ROOT).join(case.input);
        std::fs::copy(&path, originals.join(case.out))?;
        let mut wb = Workbook::open(&path)?;
        (case.edit)(&mut wb)?;
        wb.save(out_dir.join(case.out))?;
        writeln!(md, "| [ ] | `{}` | `{}` | {} | {} | |", case.out, case.input, case.edits, case.look_for)?;
    }
    let mut images = Workbook::new()?;
    let mut ws = images.add_worksheet()?;
    let logo = Image::from_bytes(include_bytes!("../tests/images/blue.png").as_slice())?.alt_text("Blue swatch");
    ws.insert_image(Anchor::one_cell(CellIndex::parse("B2")?, Size::Emu { width: 914_400, height: 685_800 }), &logo)?;
    let swatch = Image::from_bytes(include_bytes!("../tests/images/orange.png").as_slice())?;
    ws.insert_image(Anchor::two_cell(CellIndex::parse("D3")?, CellIndex::parse("F6")?), &swatch)?;
    images.save(out_dir.join("32-p5a-images.xlsx"))?;
    md.push_str("\n## Phase 5a — floating pictures (ADR-0017)\n\n");
    md.push_str("- [ ] Open `32-p5a-images.xlsx` without repair. A blue picture begins at B2 and is 1 × 0.75 in; an orange picture spans D3 to the start of F6. The blue picture's alt text is ‘Blue swatch’. Save and reopen.\n");
    let mut table = Workbook::new()?;
    (P5B_CASE.edit)(&mut table.worksheet()?)?;
    table.save(out_dir.join(P5B_CASE.out))?;
    md.push_str("\n## Phase 5b — tables (ADR-0017)\n\n");
    writeln!(md, "- [ ] Open `{}` without repair. {} {}", P5B_CASE.out, P5B_CASE.edits, P5B_CASE.look_for)?;
    md.push_str("\n## Phase 5c — notes and threads (ADR-0017)\n\n");
    let mut note = Workbook::new()?;
    let mut ws = note.worksheet()?;
    ws.write_string(1, 1, "Budget")?;
    ws.add_note(CellIndex::parse("B2")?, Note::new("Quarterly budget").author("Ana"))?;
    note.save(out_dir.join("40-p5c-note.xlsx"))?;
    md.push_str("- [ ] Open `40-p5c-note.xlsx` without repair. Hover over B2 to read Ana's note, ‘Quarterly budget’.\n");

    let mut thread = Workbook::new()?;
    let ana = thread.person("Ana", PersonId::None)?;
    let sam = thread.person("Sam", PersonId::None)?;
    let mut ws = thread.worksheet()?;
    ws.write_string(2, 3, "Review")?;
    let at = CellIndex::parse("D3")?;
    ws.add_comment(at, Comment::new("Looks wrong").by(ana))?;
    ws.reply(at, Comment::new("Fixed").by(sam))?;
    thread.save(out_dir.join("41-p5c-thread.xlsx"))?;
    md.push_str("- [ ] Open `41-p5c-thread.xlsx` without repair. D3 has Ana's ‘Looks wrong’ thread and Sam's ‘Fixed’ reply. In an older Excel version, check that the fallback note shows both messages. Save and reopen.\n");
    md.push_str(CHECKLIST_TAIL);
    std::fs::write(out_dir.join("CHECKLIST.md"), md)?;
    // Self-check before handing the bundle over: every output re-opens with
    // ooxml-xlsx and every worksheet reads with calamine (an independent
    // reader), so the owner's Excel check starts from files known to parse.
    let outs: Vec<&str> = CASES.iter().map(|c| c.out)
        .chain(P1_CASES.iter().map(|c| c.out))
        .chain(P2_CASES.iter().map(|c| c.out))
        .chain(P3_CASES.iter().map(|c| c.out))
        .chain(P4_CASES.iter().map(|c| c.out))
        .chain(["32-p5a-images.xlsx", P5B_CASE.out, "40-p5c-note.xlsx", "41-p5c-thread.xlsx"])
        .collect();
    for out in &outs {
        let path = out_dir.join(out);
        Workbook::open(&path).map_err(|e| format!("{out}: ooxml-xlsx cannot re-open it: {e}"))?;
        let mut cal: calamine::Sheets<_> = calamine::open_workbook_auto(&path).map_err(|e| format!("{out}: calamine: {e}"))?;
        let mut cells = 0usize;
        let names: Vec<String> = calamine::Reader::sheet_names(&cal);
        for name in names {
            if let Ok(range) = calamine::Reader::worksheet_range(&mut cal, &name) {
                cells += range.used_cells().count();
            }
        }
        println!("ok  {out}  (calamine reads {cells} non-empty cells)");
    }
    println!("wrote {} files and CHECKLIST.md to {}", outs.len(), out_dir.display());
    Ok(())
}

const CHECKLIST_HEAD: &str = "# ooxml-xlsx — manual Excel check

Open each file below in current Microsoft 365 Excel (desktop). For each one:

1. **Repair dialog?** \"We found a problem with some content … Do you want us to try to recover
   as much as we can?\" = **fail**. Note the exact text and, if Excel offers it, the repair log
   (the XML file it names).
2. If a file fails, open the same name in `originals/`: if the original also fails, the
   problem predates this crate (write that down too).
3. Check the edited cells show the values listed, and the \"look for\" column.
4. Close **without saving** (Excel may ask to save because it recalculates; answer No).

Protected View for downloaded files is expected; click \"Enable Editing\" only for the
non-macro files. Never enable macros in `09-62629-macro.xlsm` (third-party test data).

Report: Excel version (File > Account > About Excel), OS, and per file: pass/fail + notes.

| Pass | Output | Input | Edits | Look for | Notes |
|---|---|---|---|---|---|
";

const CHECKLIST_P1_HEAD: &str = "
## Phase1 (ADR-0003 §7.4): formula workbooks

These files had formulas; the edit removed the cached values of every formula that may depend
on the edited cell and set `fullCalcOnLoad` (except 17, manual mode). Excel must **recalculate
on open** and show the values below, with **no repair dialog**. Excel will ask to save on close
(it recalculated): answer No. Files 17–21 are synthetic workbooks made by this project
(`reference/fixtures/xlsx/synthetic/`); their originals have no styles beyond the default.

| Pass | Output | Input | Edits | Look for | Notes |
|---|---|---|---|---|---|
";

const CHECKLIST_P2_HEAD: &str = "
## Phase2 (ADR-0006 \"Test oracles\" 10): shared strings, styles, dates

Excel must open each file **without a repair dialog** and show the formats listed. The XL item
letters refer to ADR-0006 oracle 10. Where the row says **Record**, write down what Excel does:
those rows decide an open question (Q2, Q5) or an inferred limit (§5.10), not pass/fail.

| Pass | Output | Input | Edits | Look for | Notes |
|---|---|---|---|---|---|
";

const CHECKLIST_P4_HEAD: &str = "
## Phase4 part 1 (ADR-0014 Test oracles 7): merges, autofilter, protection

| Pass | Output | Input | Edits | Look for | Notes |
|---|---|---|---|---|---|
";

const CHECKLIST_P3_HEAD: &str = "
## Phase3 (ADR-0009): sheet operations

Each file had one structural operation (rename, delete, reorder, add, hide, tab colour). Excel
must open it **without a repair dialog** and show the result described. Where formulas changed
(30, 33, 35) Excel recalculates on open and asks to save on close: answer No. Not yet covered:
`docProps/app.xml` titles (left as they were; Excel rewrites them on save), so File > Info >
Properties may list old sheet names until saved.

| Pass | Output | Input | Edits | Look for | Notes |
|---|---|---|---|---|---|
";

const CHECKLIST_TAIL: &str = "
Not claimed even if every row passes (roadmap §7.6): correct `row/@spans` hints after edits
outside a span (left as is), and anything about sheets other than the edited one except that
they are byte-for-byte preserved.
";

// ------------------------------------------------------------ Phase2 (ADR-0006)

struct P2Case {
    out: &'static str,
    input: &'static str,
    edits: &'static str,
    look_for: &'static str,
    build: fn() -> Built,
}

fn fixture(rel: &str) -> BoxResult<Vec<u8>> {
    Ok(std::fs::read(Path::new(ROOT).join(rel))?)
}

/// Replaces one part's bytes (for inputs built from the template).
fn with_part(bytes: &[u8], name: &str, text: &str) -> BoxResult<Vec<u8>> {
    let mut pkg = ooxml_core::Package::from_bytes(bytes.to_vec())?;
    pkg.set_part(name, text.as_bytes().to_vec())?;
    Ok(pkg.to_bytes()?)
}

fn part_text(bytes: &[u8], name: &str) -> BoxResult<String> {
    let pkg = ooxml_core::Package::from_bytes(bytes.to_vec())?;
    Ok(String::from_utf8(pkg.read_part(name)?)?)
}

fn template() -> BoxResult<Vec<u8>> {
    Ok(Workbook::new()?.to_bytes()?)
}

/// The from-scratch format matrix (oracle 5), one format per row; column A
/// says what to look for.
fn matrix() -> Vec<(&'static str, Format)> {
    let side = |style: &str| Some(Side { style: style.into(), color: Some(Color::rgb(0x0000FF)) });
    vec![
        ("default (Calibri 11)", Format::new()),
        ("bold", Format::new().set_bold()),
        ("italic, single underline", Format::new().set_italic().set_underline(Underline::Single)),
        ("double underline, strikethrough", Format::new().set_underline(Underline::Double).set_strikethrough()),
        ("Arial 14", Format::new().set_font_size(14.0).set_font_name("Arial")),
        ("red font", Format::new().set_font_color(Color::rgb(0xFF0000))),
        ("theme 4 (accent1) font, tint -0.25 (darker)", Format::new().set_font_color(Color::Theme { index: 4, tint: Tint::new(-0.249977111117893).expect("tint") })),
        ("yellow fill", Format::new().set_background_color(Color::rgb(0xFFFF00))),
        ("lightGray pattern, red on white", Format { fill: Some(Fill::Pattern { pattern: "lightGray".into(), fg: Some(Color::rgb(0xFF0000)), bg: Some(Color::rgb(0xFFFFFF)) }), ..Format::new() }),
        ("thin border all sides", Format::new().set_border("thin")),
        ("blue medium bottom, dashed diagonal up", Format { border: Some(Border { bottom: side("medium"), diagonal: side("dashed"), diagonal_up: true, ..Border::default() }), ..Format::new() }),
        ("0.00 (built-in id 2): 1234.5678 shows 1234.57", Format::new().set_num_format("0.00")),
        ("custom 0.000\" kg\": shows 1234.568 kg", Format::new().set_num_format("0.000\" kg\"")),
        ("percent 0.0%: shows 123456.8%", Format::new().set_num_format("0.0%")),
        ("centered, wrapped", Format::new().set_align("center").set_text_wrap()),
        ("right, top", Format::new().set_align("right").set_valign("top")),
        ("unlocked + hidden (Format Cells > Protection)", Format::new().set_unlocked().set_hidden()),
        ("everything: bold red on yellow, thin border, 0.000", Format::new().set_num_format("0.000").set_bold().set_font_color(Color::rgb(0xFF0000)).set_background_color(Color::rgb(0xFFFF00)).set_border("thin")),
    ]
}

fn p2_matrix() -> Built {
    let mut wb = Workbook::new()?;
    {
        let mut ws = wb.add_worksheet()?;
        for (i, (what, f)) in matrix().iter().enumerate() {
            let r = i as u32;
            ws.cell_mut(r, 0)?.set_string(what)?;
            ws.cell_mut(r, 1)?.set_number_with_format(1234.5678, f)?;
            ws.cell_mut(r, 2)?.set_string_with_format("text", f)?;
        }
        let n = matrix().len() as u32;
        ws.cell_mut(n, 0)?.set_string("quote prefix: shows =1+1 as text")?;
        ws.cell_mut(n, 2)?.set_string_with_format("=1+1", &Format::new().set_quote_prefix())?;
        ws.cell_mut(n + 1, 0)?.set_string("TRUE, #N/A, date 2026-11-16 (short-date format)")?;
        ws.cell_mut(n + 1, 1)?.set_bool(true)?;
        ws.cell_mut(n + 1, 2)?.set_error("#N/A")?;
        ws.cell_mut(n + 1, 3)?.set_datetime(ExcelDateTime::from_ymd(2026, 11, 16).ok_or("date")?)?;
    }
    Ok((None, wb.to_bytes()?))
}

fn p2_matrix_rx() -> Built {
    use rust_xlsxwriter as rx;
    let side = rx::FormatBorder::Thin;
    let theirs: Vec<rx::Format> = vec![
        rx::Format::new(),
        rx::Format::new().set_bold(),
        rx::Format::new().set_italic().set_underline(rx::FormatUnderline::Single),
        rx::Format::new().set_underline(rx::FormatUnderline::Double).set_font_strikethrough(),
        rx::Format::new().set_font_size(14).set_font_name("Arial"),
        rx::Format::new().set_font_color(rx::Color::RGB(0xFF0000)),
        rx::Format::new().set_font_color(rx::Color::Theme(4, 4)),
        rx::Format::new().set_background_color(rx::Color::RGB(0xFFFF00)),
        rx::Format::new().set_pattern(rx::FormatPattern::LightGray).set_foreground_color(rx::Color::RGB(0xFF0000)).set_background_color(rx::Color::RGB(0xFFFFFF)),
        rx::Format::new().set_border(side),
        rx::Format::new()
            .set_border_bottom(rx::FormatBorder::Medium)
            .set_border_bottom_color(rx::Color::RGB(0x0000FF))
            .set_border_diagonal(rx::FormatBorder::Dashed)
            .set_border_diagonal_color(rx::Color::RGB(0x0000FF))
            .set_border_diagonal_type(rx::FormatDiagonalBorder::BorderUp),
        rx::Format::new().set_num_format_index(2),
        rx::Format::new().set_num_format("0.000\" kg\""),
        rx::Format::new().set_num_format("0.0%"),
        rx::Format::new().set_align(rx::FormatAlign::Center).set_text_wrap(),
        rx::Format::new().set_align(rx::FormatAlign::Right).set_align(rx::FormatAlign::Top),
        rx::Format::new().set_unlocked().set_hidden(),
        rx::Format::new().set_num_format("0.000").set_bold().set_font_color(rx::Color::RGB(0xFF0000)).set_background_color(rx::Color::RGB(0xFFFF00)).set_border(side),
    ];
    let mut wb = rx::Workbook::new();
    let ws = wb.add_worksheet();
    for (i, ((what, _), f)) in matrix().iter().zip(&theirs).enumerate() {
        let r = i as u32;
        ws.write_string(r, 0, *what)?;
        ws.write_number_with_format(r, 1, 1234.5678, f)?;
        ws.write_string_with_format(r, 2, "text", f)?;
    }
    Ok((None, wb.save_to_buffer()?))
}

fn p2_inherit_arial() -> Built {
    let input = fixture("fixtures/xlsx/poi/62272.xlsx")?;
    let mut wb = Workbook::from_bytes(input.clone())?;
    let bold = Format::with_default_font_of(&mut wb)?.set_bold();
    {
        let mut ws = wb.worksheet()?;
        ws.cell_mut(39, 0)?.set_number_with_format(1234.5678, &Format::new().set_num_format("0.000"))?;
        ws.cell_mut(39, 1)?.set_number_with_format(1234.5678, &bold)?;
        ws.cell_mut(39, 2)?.set_number_with_format(1234.5678, &Format::new().set_bold())?;
    }
    Ok((Some(input), wb.to_bytes()?))
}

fn p2_dates(date1904: bool) -> Built {
    let base = template()?;
    let input = if date1904 {
        let wbx = part_text(&base, "xl/workbook.xml")?.replacen("<workbookPr ", "<workbookPr date1904=\"1\" ", 1);
        with_part(&base, "xl/workbook.xml", &wbx)?
    } else {
        base
    };
    let mut wb = Workbook::from_bytes(input.clone())?;
    {
        let mut ws = wb.worksheet()?;
        let dates = if date1904 {
            [(1904, 1, 1), (1904, 1, 2), (2000, 2, 29), (2026, 11, 16), (9999, 12, 31)]
        } else {
            [(1899, 12, 31), (1900, 2, 28), (1900, 3, 1), (2026, 11, 16), (9999, 12, 31)]
        };
        for (i, (y, m, d)) in dates.into_iter().enumerate() {
            let r = i as u32;
            ws.cell_mut(r, 0)?.set_string(&format!("{y:04}-{m:02}-{d:02}"))?;
            ws.cell_mut(r, 1)?.set_datetime(ExcelDateTime::from_ymd(y, m, d).ok_or("date")?)?;
            let iso = Format::new().set_num_format("yyyy-mm-dd");
            ws.cell_mut(r, 2)?.set_datetime_with_format(ExcelDateTime::from_ymd(y, m, d).ok_or("date")?, &iso)?;
        }
        ws.cell_mut(5, 0)?.set_string("2026-11-16 13:45:30")?;
        let dt = Format::new().set_num_format("yyyy-mm-dd hh:mm:ss");
        ws.cell_mut(5, 2)?.set_datetime_with_format(ExcelDateTime::from_ymd_hms(2026, 11, 16, 13, 45, 30.0).ok_or("date")?, &dt)?;
        ws.cell_mut(6, 0)?.set_string("12:00 (time only)")?;
        ws.cell_mut(6, 2)?.set_datetime_with_format(ExcelDateTime::from_hms(12, 0, 0.0).ok_or("time")?, &Format::new().set_num_format("hh:mm"))?;
    }
    Ok((Some(input), wb.to_bytes()?))
}

fn p2_numfmts_inserted() -> Built {
    let input = fixture("round_trip_corpus/fixture_minimal.xlsx")?;
    let mut wb = Workbook::from_bytes(input.clone())?;
    {
        let mut ws = wb.worksheet()?;
        ws.cell_mut(1, 1)?.set_num_format("#,##0.000\" kg\"")?;
        ws.cell_mut(6, 0)?.set_number_with_format(0.25, &Format::new().set_num_format("0.0%").set_background_color(Color::rgb(0xFFFF00)))?;
    }
    Ok((Some(input), wb.to_bytes()?))
}

fn p2_known_fonts() -> Built {
    let input = fixture("fixtures/xlsx/poi/61605.xlsx")?;
    let mut wb = Workbook::from_bytes(input.clone())?;
    {
        let mut ws = wb.worksheet()?;
        let f = Format::new().set_font_name("Meiryo").set_font_size(16.0).set_bold().set_font_color(Color::rgb(0xC00000));
        ws.cell_mut(24, 0)?.set_string_with_format("new font, new row", &f)?;
        ws.cell_mut(24, 1)?.set_number_with_format(2.0, &f)?;
    }
    Ok((Some(input), wb.to_bytes()?))
}

fn p2_wrong_sst_count() -> Built {
    let mut wb = Workbook::new()?;
    {
        let mut ws = wb.add_worksheet()?;
        ws.cell_mut(0, 0)?.set_string("a")?;
        ws.cell_mut(1, 0)?.set_string("b")?;
    }
    let base = wb.to_bytes()?;
    let sst = part_text(&base, "xl/sharedStrings.xml")?;
    if !sst.contains("count=\"2\"") {
        return Err("expected sst count=2".into());
    }
    let input = with_part(&base, "xl/sharedStrings.xml", &sst.replacen("count=\"2\"", "count=\"7\"", 1))?;
    let mut wb = Workbook::from_bytes(input.clone())?;
    {
        let mut ws = wb.worksheet()?;
        ws.cell_mut(2, 0)?.set_string("c")?;
        ws.cell_mut(3, 0)?.set_string("a")?;
    }
    Ok((Some(input), wb.to_bytes()?))
}

fn p2_empty_sst() -> Built {
    let mut wb = Workbook::new()?;
    {
        let mut ws = wb.add_worksheet()?;
        ws.cell_mut(0, 0)?.set_number(1.0)?;
        ws.cell_mut(1, 0)?.set_number(2.0)?;
    }
    Ok((None, wb.to_bytes()?))
}

/// A template workbook whose `styles.xml` has `n` entries in one table and
/// a cell `A1` using the last one; built by hand because the registry
/// refuses to grow past §5.10's limits.
fn p2_limit(table: &str, n: usize) -> Built {
    let base = template()?;
    let mut s = part_text(&base, "xl/styles.xml")?;
    let last = n - 1;
    let xf = match table {
        "fills" => {
            let entries: String = (0..n)
                .map(|i| match i {
                    0 => "<fill><patternFill patternType=\"none\"/></fill>".to_owned(),
                    1 => "<fill><patternFill patternType=\"gray125\"/></fill>".to_owned(),
                    _ => format!("<fill><patternFill patternType=\"solid\"><fgColor rgb=\"FF{:06X}\"/><bgColor indexed=\"64\"/></patternFill></fill>", i * 40_000),
                })
                .collect();
            s = s.replacen("<fills count=\"2\"><fill><patternFill patternType=\"none\"/></fill><fill><patternFill patternType=\"gray125\"/></fill></fills>", &format!("<fills count=\"{n}\">{entries}</fills>"), 1);
            format!("<xf numFmtId=\"0\" fontId=\"0\" fillId=\"{last}\" borderId=\"0\" xfId=\"0\" applyFill=\"1\"/>")
        }
        "borders" => {
            let styles = ["thin", "medium", "dashed", "dotted", "thick", "double", "hair", "mediumDashed", "dashDot", "mediumDashDot", "dashDotDot", "mediumDashDotDot", "slantDashDot"];
            let entries: String = (0..n)
                .map(|i| {
                    if i == 0 {
                        return "<border><left/><right/><top/><bottom/><diagonal/></border>".to_owned();
                    }
                    let st = styles[i % styles.len()];
                    format!("<border><left style=\"{st}\"><color rgb=\"FF{:06X}\"/></left><right/><top/><bottom/><diagonal/></border>", i * 40_000)
                })
                .collect();
            s = s.replacen("<borders count=\"1\"><border><left/><right/><top/><bottom/><diagonal/></border></borders>", &format!("<borders count=\"{n}\">{entries}</borders>"), 1);
            format!("<xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"{last}\" xfId=\"0\" applyBorder=\"1\"/>")
        }
        "fonts" => {
            let entries: String = (0..n)
                .map(|i| {
                    if i == 0 {
                        return "<font><sz val=\"11\"/><color theme=\"1\"/><name val=\"Calibri\"/><family val=\"2\"/><scheme val=\"minor\"/></font>".to_owned();
                    }
                    format!("<font><sz val=\"{}\"/><color rgb=\"FF{:06X}\"/><name val=\"Calibri\"/><family val=\"2\"/></font>", 8 + i % 40, i * 20_000)
                })
                .collect();
            s = s.replacen("<fonts count=\"1\"><font><sz val=\"11\"/><color theme=\"1\"/><name val=\"Calibri\"/><family val=\"2\"/><scheme val=\"minor\"/></font></fonts>", &format!("<fonts count=\"{n}\">{entries}</fonts>"), 1);
            format!("<xf numFmtId=\"0\" fontId=\"{last}\" fillId=\"0\" borderId=\"0\" xfId=\"0\" applyFont=\"1\"/>")
        }
        "numFmts" => {
            let entries: String = (0..n).map(|i| format!("<numFmt numFmtId=\"{}\" formatCode=\"0.{}\"/>", 164 + i, "0".repeat(1 + i % 20) + &format!("&quot; n{i}&quot;"))).collect();
            let open_end = s.find("<fonts").ok_or("styles")?;
            s.insert_str(open_end, &format!("<numFmts count=\"{n}\">{entries}</numFmts>"));
            format!("<xf numFmtId=\"{}\" fontId=\"0\" fillId=\"0\" borderId=\"0\" xfId=\"0\" applyNumberFormat=\"1\"/>", 164 + last)
        }
        _ => return Err("table".into()),
    };
    s = s.replacen("<cellXfs count=\"1\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\" xfId=\"0\"/></cellXfs>", &format!("<cellXfs count=\"2\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\" xfId=\"0\"/>{xf}</cellXfs>"), 1);
    if !s.contains(&xf) {
        return Err(format!("{table}: template styles.xml changed").into());
    }
    let input = with_part(&base, "xl/styles.xml", &s)?;
    let mut wb = Workbook::from_bytes(input)?;
    {
        let mut ws = wb.worksheet()?;
        ws.cell_mut(0, 0)?.set_number(1234.5678)?;
        ws.cell_mut(0, 0)?.set_style_index(1)?;
        ws.cell_mut(1, 0)?.set_string(&format!("A1 uses {table}[{last}] ({n} entries)"))?;
    }
    Ok((None, wb.to_bytes()?))
}

/// The template with Excel's built-in "Good" cell style (green fill, dark
/// green font, `applyNumberFormat="0"` as Excel writes it) and one styled
/// cell to apply it to.
fn p2_named_and_preserving() -> Built {
    let base = template()?;
    let styles = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><fonts count="2"><font><sz val="11"/><color theme="1"/><name val="Calibri"/><family val="2"/><scheme val="minor"/></font><font><sz val="11"/><color rgb="FF006100"/><name val="Calibri"/><family val="2"/><scheme val="minor"/></font></fonts><fills count="3"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill><fill><patternFill patternType="solid"><fgColor rgb="FFC6EFCE"/></patternFill></fill></fills><borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders><cellStyleXfs count="2"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/><xf numFmtId="0" fontId="1" fillId="2" borderId="0" applyNumberFormat="0" applyBorder="0" applyAlignment="0" applyProtection="0"/></cellStyleXfs><cellXfs count="2"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/><xf numFmtId="4" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1" applyAlignment="1"><alignment horizontal="right" vertical="top"/></xf></cellXfs><cellStyles count="2"><cellStyle name="Good" xfId="1" builtinId="26"/><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles><dxfs count="0"/><tableStyles count="0" defaultTableStyle="TableStyleMedium9" defaultPivotStyle="PivotStyleLight16"/></styleSheet>"#;
    let b = with_part(&base, "xl/styles.xml", styles)?;
    let sheet = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1" s="1"><v>1234.5</v></c><c r="B1" s="1"><v>1234.5</v></c><c r="C1" s="1"><v>1234.5</v></c><c r="D1" s="1"><v>1234.5</v></c><c r="E1" s="1"><v>1234.5</v></c></row></sheetData></worksheet>"#;
    let input = with_part(&b, "xl/worksheets/sheet1.xml", sheet)?;
    let mut wb = Workbook::from_bytes(input.clone())?;
    {
        let mut ws = wb.worksheet()?;
        ws.cell_mut(0, 0)?.set_named_style("Good")?;
        ws.cell_mut(0, 1)?.set_num_format("0.000")?;
        ws.cell_mut(0, 2)?.set_font(&Font { italic: true, color: Some(Color::rgb(0x0000FF)), ..Font::default() })?;
        ws.cell_mut(0, 3)?.set_fill(&Fill::Pattern { pattern: "solid".into(), fg: Some(Color::rgb(0xFFC7CE)), bg: None })?;
        let side = Some(Side { style: "double".into(), color: None });
        ws.cell_mut(0, 4)?.set_border(&Border { bottom: side, ..Border::default() })?;
    }
    Ok((Some(input), wb.to_bytes()?))
}

/// XL (f) with a Japanese default font (`游ゴシック`, `charset="128"`; PR #50
/// review M3): the inherited font and `with_default_font_of(..).set_bold()`.
fn p2_japanese_default_font() -> Built {
    let base = template()?;
    let s = part_text(&base, "xl/styles.xml")?.replacen(
        r#"<font><sz val="11"/><color theme="1"/><name val="Calibri"/><family val="2"/><scheme val="minor"/></font>"#,
        r#"<font><sz val="11"/><color theme="1"/><name val="游ゴシック"/><family val="2"/><charset val="128"/><scheme val="minor"/></font>"#,
        1,
    );
    if !s.contains("游ゴシック") {
        return Err("template styles.xml changed".into());
    }
    // as in Japanese Excel's own template: the theme names 游ゴシック for the
    // Jpan script, so Japanese text resolves to it (issue #115)
    let theme = part_text(&base, "xl/theme/theme1.xml")?;
    let minor = r#"<a:minorFont><a:latin typeface="Calibri"/><a:ea typeface=""/><a:cs typeface=""/>"#;
    if !theme.contains(minor) {
        return Err("template theme1.xml changed".into());
    }
    let theme = theme.replacen(minor, &format!(r#"{minor}<a:font script="Jpan" typeface="游ゴシック"/>"#), 1);
    let input = with_part(&with_part(&base, "xl/styles.xml", &s)?, "xl/theme/theme1.xml", &theme)?;
    let mut wb = Workbook::from_bytes(input.clone())?;
    let bold = Format::with_default_font_of(&mut wb)?.set_bold();
    {
        let mut ws = wb.worksheet()?;
        ws.cell_mut(0, 0)?.set_string("既定のフォント")?;
        ws.cell_mut(0, 1)?.set_number_with_format(1234.5678, &Format::new().set_num_format("#,##0.000"))?;
        ws.cell_mut(1, 0)?.set_string("太字（既定フォントのまま）")?;
        ws.cell_mut(1, 1)?.set_number_with_format(1234.5678, &bold)?;
    }
    Ok((Some(input), wb.to_bytes()?))
}

fn rich_runs() -> (Font, Font) {
    (Font { bold: true, ..Font::default() }, Font { italic: true, color: Some(Color::rgb(0xC00000)), ..Font::default() })
}

/// Part 2: rich strings into the shared-string table.
fn p2_rich_shared() -> Built {
    let (bold, red) = rich_runs();
    let mut wb = Workbook::new()?;
    {
        let mut ws = wb.add_worksheet()?;
        ws.cell_mut(0, 0)?.set_rich_string(&[(&Font::default(), "plain, "), (&bold, "bold"), (&red, " and red italic")])?;
        ws.cell_mut(1, 0)?.set_rich_string(&[(&bold, "same "), (&red, "runs")])?;
        ws.cell_mut(2, 0)?.set_rich_string(&[(&bold, "same "), (&red, "runs")])?;
        let sup = Font { vert_align: Some(ooxml_xlsx::VertAlign::Superscript), ..Font::default() };
        ws.cell_mut(3, 0)?.set_rich_string(&[(&Font::default(), "E = mc"), (&sup, "2")])?;
    }
    Ok((None, wb.to_bytes()?))
}

/// Part 2: inline rich strings in a workbook without a shared-string table.
fn p2_rich_inline() -> Built {
    let (bold, red) = rich_runs();
    let input = fixture("fixtures/xlsx/poi/52348.xlsx")?;
    let mut wb = Workbook::from_bytes(input.clone())?;
    if wb.shared_strings_available()? {
        return Err("52348.xlsx unexpectedly has a shared-string table".into());
    }
    wb.worksheet()?.cell_mut(39, 0)?.set_rich_string(&[(&bold, "inline "), (&red, "rich")])?;
    Ok((Some(input), wb.to_bytes()?))
}

/// Part 2: a dxf, made visible by a conditional format that already refers
/// to `dxfId="0"` (the template has an empty `dxfs`).
fn p2_dxf() -> Built {
    let base = template()?;
    let sheet = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1"><v>1</v></c></row><row r="2"><c r="A2"><v>2</v></c></row><row r="3"><c r="A3"><v>3</v></c></row></sheetData><conditionalFormatting sqref="A1:A3"><cfRule type="cellIs" dxfId="0" priority="1" operator="greaterThan"><formula>1</formula></cfRule></conditionalFormatting></worksheet>"#;
    let s = part_text(&base, "xl/styles.xml")?;
    // the reference to dxf 0 is dangling until the dxf exists: give the
    // input one placeholder so the append is not refused, then add ours
    let s = s.replacen(r#"<dxfs count="0"/>"#, r#"<dxfs count="1"><dxf><font><i/></font></dxf></dxfs>"#, 1);
    let input = with_part(&with_part(&base, "xl/worksheets/sheet1.xml", sheet)?, "xl/styles.xml", &s)?;
    let mut wb = Workbook::from_bytes(input.clone())?;
    let green = ooxml_xlsx::DxfFormat::new().set_font_color(Color::rgb(0x006100)).set_background_color(Color::rgb(0xC6EFCE));
    let i = wb.add_differential_format(&green)?;
    // point a second rule at the new dxf (a hand-made sheet edit; the API
    // for conditional formats is Phase4)
    let out = wb.to_bytes()?;
    let sheet2 = sheet.replace(
        "</conditionalFormatting></worksheet>",
        &format!(r#"</conditionalFormatting><conditionalFormatting sqref="B1:B3"><cfRule type="expression" dxfId="{i}" priority="2"><formula>TRUE</formula></cfRule></conditionalFormatting></worksheet>"#),
    );
    let out = with_part(&out, "xl/worksheets/sheet1.xml", &sheet2.replace(r#"<c r="A1"><v>1</v></c>"#, r#"<c r="A1"><v>1</v></c><c r="B1"><v>9</v></c>"#))?;
    Ok((Some(input), out))
}

const P2_CASES: &[P2Case] = &[
    P2Case {
        out: "22-p2-format-matrix.xlsx",
        input: "(embedded template)",
        edits: "oracle 5: one format per row (column A names it), applied to a number (B) and a string (C); then a quote prefix, TRUE, #N/A and a date",
        look_for: "Each row looks as column A says. Compare with `22b-p2-format-matrix-rx.xlsx` (rust_xlsxwriter, same matrix): B and C should look the same row by row (row 7's tint differs slightly: rust_xlsxwriter's shade table). No repair dialog.",
        build: p2_matrix,
    },
    P2Case {
        out: "22b-p2-format-matrix-rx.xlsx",
        input: "(rust_xlsxwriter 0.102)",
        edits: "the reference for 22, written by rust_xlsxwriter",
        look_for: "Reference only.",
        build: p2_matrix_rx,
    },
    P2Case {
        out: "23-p2-62272-arial-inherit.xlsx",
        input: "`fixtures/xlsx/poi/62272.xlsx`",
        edits: "**XL (f).** A40 = 1234.5678 with only a number format (font inherited); B40 with `with_default_font_of(..).set_bold()`; C40 with `Format::new().set_bold()`",
        look_for: "A40 is **Arial 10** like its neighbours, shows 1234.568; B40 is **Arial 10 bold**; C40 is 11 pt bold in the theme's minor font: this workbook has no theme, so Excel 365 shows Aptos Narrow (`Format::new()` writes Calibri with `scheme=minor`, as rust_xlsxwriter does; issue #115).",
        build: p2_inherit_arial,
    },
    P2Case {
        out: "24-p2-dates-1904.xlsx",
        input: "template with `date1904=\"1\"`",
        edits: "**XL (a).** Column A names a date; B = `set_datetime` (short-date format), C = the same date as yyyy-mm-dd; C6 a date-time, C7 a time",
        look_for: "Every B and C cell shows the date in column A (B in your short-date form). File > Options > Advanced shows \"Use 1904 date system\" checked.",
        build: || p2_dates(true),
    },
    P2Case {
        out: "25-p2-dates-1900.xlsx",
        input: "(embedded template)",
        edits: "As 24 in the 1900 system (1899-12-31 = serial 0, around Excel's false 1900-02-29)",
        look_for: "Row 1 shows 1900-01-00 (Excel's display of serial 0) or 1899-12-31: **Record** which. Rows 2–5 show the dates in column A; C6 2026-11-16 13:45:30; C7 12:00.",
        build: || p2_dates(false),
    },
    P2Case {
        out: "26-p2-numfmts-inserted.xlsx",
        input: "`round_trip_corpus/fixture_minimal.xlsx`",
        edits: "**XL (b).** B2 `set_num_format(\"#,##0.000\\\" kg\\\"\")` (the file has no `numFmts`: the table is inserted; it has one fill: `gray125` is added first); A7 = 25% on yellow",
        look_for: "B2 shows 91.000 kg style formatting of its value, keeping its other formatting; A7 = 25.0% on yellow. No repair dialog.",
        build: p2_numfmts_inserted,
    },
    P2Case {
        out: "27-p2-knownfonts-new-font-row.xlsx",
        input: "`fixtures/xlsx/poi/61605.xlsx` (`x14ac:knownFonts=\"1\"`)",
        edits: "**XL (c), Q5.** A25:B25 (new row) in a new font: Meiryo 16 bold dark red",
        look_for: "**Record**: does Excel open it without complaint, and is row 25 tall enough for the 16 pt text (or clipped)? `knownFonts` is left untouched (Q5 proposed default).",
        build: p2_known_fonts,
    },
    P2Case {
        out: "28-p2-sst-count-was-wrong.xlsx",
        input: "from-scratch file whose `sst/@count` was patched from 2 to 7",
        edits: "**XL (d).** A3 = \"c\" (appended), A4 = \"a\" (reused); `count` is rewritten as the number of `t=\"s\"` cells (4)",
        look_for: "A1:A4 = a, b, c, a. No repair dialog. (`originals/` has the wrong count; record whether Excel minds it there.)",
        build: p2_wrong_sst_count,
    },
    P2Case {
        out: "29-p2-empty-sst.xlsx",
        input: "(embedded template)",
        edits: "**XL (e), Q2.** Numbers only: the template's empty `<sst count=\"0\" uniqueCount=\"0\"/>` is saved as it is",
        look_for: "**Record**: no repair dialog? (If Excel repairs, Q2's proposed default — an empty sst in the template — must change.)",
        build: p2_empty_sst,
    },
    P2Case {
        out: "30-p2-limit-300-fills.xlsx",
        input: "template with 300 fills",
        edits: "**XL (g).** A1 uses fills[299]",
        look_for: "**Record**: does Excel open it, and does it report a limit? (The registry refuses a 257th fill; §5.10's mapping is inferred.)",
        build: || p2_limit("fills", 300),
    },
    P2Case {
        out: "31-p2-limit-300-borders.xlsx",
        input: "template with 300 borders",
        edits: "**XL (g).** A1 uses borders[299]",
        look_for: "**Record** as 30 (registry limit 256).",
        build: || p2_limit("borders", 300),
    },
    P2Case {
        out: "32-p2-limit-600-fonts.xlsx",
        input: "template with 600 fonts",
        edits: "**XL (g).** A1 uses fonts[599]",
        look_for: "**Record** as 30 (registry limit 512).",
        build: || p2_limit("fonts", 600),
    },
    P2Case {
        out: "33-p2-limit-260-numfmts.xlsx",
        input: "template with 260 custom number formats",
        edits: "**XL (g).** A1 uses numFmt 423 (the 260th)",
        look_for: "**Record** as 30 (registry limit 200; Excel documents 200–250).",
        build: || p2_limit("numFmts", 260),
    },
    P2Case {
        out: "34-p2-named-style-and-preserving.xlsx",
        input: "template with Excel's \"Good\" style; A1:E1 = 1234.5, `#,##0.00`, right/top aligned",
        edits: "A1 `set_named_style(\"Good\")`; B1 `set_num_format(\"0.000\")`; C1 `set_font(italic blue)`; D1 `set_fill(pink)`; E1 `set_border(double bottom)`",
        look_for: "A1 green on green **and still 1,234.50** (Good does not apply a number format); Home > Cell Styles shows A1 as Good. B1 1234.500; C1 italic blue, D1 pink fill, E1 double bottom border; B1:E1 keep 1,234.50-style formatting except what changed, and stay right/top aligned.",
        build: p2_named_and_preserving,
    },
    P2Case {
        out: "35-p2-japanese-default-font.xlsx",
        input: "template whose default font is 游ゴシック 11 (`charset=\"128\"`), with 游ゴシック as the theme's Jpan minor font",
        edits: "**XL (f), Japanese.** B1 = 1234.5678 with only a number format; B2 with `with_default_font_of(..).set_bold()`",
        look_for: "A1's Japanese text is drawn in 游ゴシック; B1 has A1's font and shows 1,234.568; B2 is the same font **bold**, 11 pt. Japanese-UI Excel is expected to name the font 游ゴシック; Excel 365 with an English UI named Calibri (the theme's Latin font) for all three (checked 2026-11-21). No repair dialog.",
        build: p2_japanese_default_font,
    },
    P2Case {
        out: "36-p2-rich-strings-shared.xlsx",
        input: "(embedded template)",
        edits: "Part 2: A1 `plain, ` + **bold** + red italic; A2 and A3 the same rich string (one shared item); A4 `E = mc²` with a superscript run",
        look_for: "Each run shows its own formatting in the cell and in the formula bar; A2 and A3 look identical; A4's 2 is superscript. No repair dialog.",
        build: p2_rich_shared,
    },
    P2Case {
        out: "37-p2-rich-strings-inline.xlsx",
        input: "`fixtures/xlsx/poi/52348.xlsx` (no shared-string table)",
        edits: "Part 2: A40 an inline rich string (`is/r`): **inline** + red italic rich",
        look_for: "A40 shows both runs formatted. No repair dialog.",
        build: p2_rich_inline,
    },
    P2Case {
        out: "38-p2-dxf.xlsx",
        input: "template with a conditional format on A1:A3 (> 1) using dxf 0 (italic)",
        edits: "Part 2: `add_differential_format(green text on light green)` appends dxf 1; a second rule (hand-written, Phase4 API) shows it on B1",
        look_for: "A2 and A3 italic (the existing dxf, unchanged); B1 dark green on light green. Home > Conditional Formatting > Manage Rules shows both rules. No repair dialog.",
        build: p2_dxf,
    },
];
