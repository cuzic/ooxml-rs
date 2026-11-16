//! Writes the Phase0 outputs the project owner opens in Microsoft 365 Excel
//! (roadmap §7.6, ADR-0001 Consequences: no Excel in CI), plus a checklist.
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

use ooxml_xlsx::{Result, Workbook, Worksheet};

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
    md.push_str(CHECKLIST_TAIL);
    std::fs::write(out_dir.join("CHECKLIST.md"), md)?;
    println!("wrote {} files and CHECKLIST.md to {}", CASES.len(), out_dir.display());
    Ok(())
}

const CHECKLIST_HEAD: &str = "# ooxml-xlsx Phase0 — manual Excel check

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

const CHECKLIST_TAIL: &str = "
Not claimed even if every row passes (roadmap §7.6): correct `row/@spans` hints after edits
outside a span (left as is), and anything about sheets other than the edited one except that
they are byte-for-byte preserved.
";
