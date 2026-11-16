//! Writes the Phase0 and Phase1 outputs the project owner opens in Microsoft
//! 365 Excel (roadmap §7.6, ADR-0001 Consequences, ADR-0003 §7.4: no Excel in
//! CI), plus a checklist.
//!
//! ```sh
//! cargo run -p ooxml-xlsx --example manual_check -- <outdir>
//! ```
//!
//! `<outdir>/` gets the edited files, `originals/` (unedited inputs, to tell
//! a pre-existing repair dialog from one this crate caused) and
//! `CHECKLIST.md`.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use ooxml_xlsx::{CellValue, Result, Workbook, Worksheet};

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
        edits: "A1 = \"outside\", E2 (table body) = 9 (header row E1:G1 is refused by design)",
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
    md.push_str(CHECKLIST_TAIL);
    std::fs::write(out_dir.join("CHECKLIST.md"), md)?;
    // Self-check before handing the bundle over: every output re-opens with
    // ooxml-xlsx and every worksheet reads with calamine (an independent
    // reader), so the owner's Excel check starts from files known to parse.
    let outs: Vec<&str> = CASES.iter().map(|c| c.out).chain(P1_CASES.iter().map(|c| c.out)).collect();
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

const CHECKLIST_HEAD: &str = "# ooxml-xlsx Phase0 + Phase1 — manual Excel check

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

const CHECKLIST_TAIL: &str = "
Not claimed even if every row passes (roadmap §7.6): correct `row/@spans` hints after edits
outside a span (left as is), and anything about sheets other than the edited one except that
they are byte-for-byte preserved.
";
