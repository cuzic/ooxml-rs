# ooxml-xlsx

Lossless Excel (`.xlsx`/`.xlsm`) editing on [`ooxml-core`](../ooxml-core): Phase0 of
[ADR-0001](../../docs/adr/0001-lossless-round-trip-architecture.md), with the typed lens of
[ADR-0002](../../docs/adr/0002-schema-driven-code-generation.md). The acceptance tests are
specified in [`docs/roadmap/xlsx.md`](../../docs/roadmap/xlsx.md) §7.

Phase0 = open a workbook written by anyone, overwrite or create cell **values** on one sheet,
save, and lose nothing else. It is narrow on purpose: whatever it cannot do safely, it refuses
with an `Err` and changes nothing.

## API

```rust
use ooxml_xlsx::{CellValue, Workbook};

let mut wb = Workbook::open("book.xlsx")?;      // whole file read into memory
let mut ws = wb.worksheet()?;                    // "sheet1" (see below)
if let Some(c) = ws.cell(0, 0)? {                // A1, 0-based; never modifies the tree
    let v: CellValue = c.value()?;               // Number / String / Bool / Error / Date / Empty
}
ws.cell_mut(1, 1)?.set_number(42.0)?;            // B2
ws.cell_mut(0, 2)?.set_string("  padded ")?;     // C1: t="inlineStr", xml:space="preserve"
wb.save("book.xlsx")?;                           // temp file + rename: same path is safe

let mut wb = Workbook::new()?;                   // embedded template, one empty sheet
let mut ws = wb.add_worksheet()?;                // first call only
ws.write_string(0, 0, "a")?;
ws.write_number(1, 1, 2.0)?;
wb.save("new.xlsx")?;
```

| Item | Behaviour |
| --- | --- |
| `Workbook::open` / `from_bytes` / `from_package` | Locates the workbook through the `officeDocument` relationship and "sheet1" through `<sheets>` + relationships (no hard-coded part names). Parses the first worksheet, `workbook.xml`, `sharedStrings.xml` and the sheet's table parts for reading; all of them are still raw-copied on save unless edited. Scans every sheet part for formulas once. |
| "sheet1" | The **first visible worksheet in workbook order**: hidden/very hidden sheets, chartsheets, macro sheets and dialog sheets are skipped (owner decision 2026-11-16). `Err(NoVisibleWorksheet)` if there is none. |
| `Worksheet::cell(row, col)` | `Ok(None)` if absent. Shared strings resolved; inline strings with rich runs concatenated; phonetic runs ignored; a `t` that does not itself carry `xml:space="preserve"` has its outer ASCII whitespace (space, tab, CR, LF) trimmed, as calamine reads it: an `xml:space` inherited from `si`/`is`/`r` is ignored; `_xHHHH_` escapes decoded. A formula cell returns its cached value (`formula()` gives the text). |
| `cell_mut(row, col)` | A handle; nothing is created or dirtied until a setter runs. |
| `set_number(f64)` | `<v>` gets the number; a stale `t` and every `<is>` are removed; `s` kept. NaN/∞ refused. |
| `set_string(&str)` | `t="inlineStr"`, `<is><t>…</t></is>`; `xml:space="preserve"` iff outer whitespace; control characters as `_xHHHH_`, a literal `_xHHHH_` protected as `_x005F_xHHHH_`; at most 32,767 UTF-16 units. Never touches `sharedStrings.xml`. |
| Malformed cells | Both setters remove **every** `<v>` and `<is>` of the cell before writing the new one, so a schema-invalid duplicate (two `v`, two `is`, `v` and `is` together, also around or after an `extLst`) is cleaned up rather than left for another reader to find; the write is `Ok`. |
| New rows/cells | Inserted in row/column order with `r`; no whitespace added; existing `spans` left as is. |
| `dimension` | Grown to include the written cell if present; **never inserted** if absent. Accepted `ref` spellings: `A1` or `A1:D2` with column letters in either case, optional `$` before the column and/or row, corners in either order, within 1,048,576 × 16,384. Untouched when the cell is inside it; when grown it is rewritten in canonical form (`A1:F9`). Any other `ref` (surrounding spaces, `Sheet1!A1`, `A0`, a third part, empty) or a missing one refuses the write (`Malformed`). |
| `Workbook::new` | Opens `template/blank.xlsx` (built by `template/build.py` from `template/blank/`) through the same code path; `docProps/core.xml` is regenerated (created/modified = now) with `Package::set_part`. |
| `add_worksheet()` | First call on a `new()` workbook returns the template's sheet; every other call, and any call on an opened file, `Err(SheetAdditionUnsupported)`. `add_worksheet_with_name` is always `Err(SheetNamingUnsupported)`. |

## Refusals (`Err`, nothing changed)

Every write is planned first: all refusals, the row/cell insertion points,
the new cell content and the new `dimension/@ref` are decided, and each
insertion is dry-run against the siblings it will have, before the sheet is
touched; only then is the plan applied, by steps that cannot fail. After an
`Err` the part is not dirty and saves byte-identical to the input.

| Error | When |
| --- | --- |
| `FormulaPresent` | Any value edit, if any sheet part (worksheet, macro sheet, dialog sheet) contains an element in the SpreadsheetML main namespace with local name `f`, or the workbook has a `calcChain.xml`. Matched by QName, never by the bytes `<f` (`<formula>`, `<from>`, `<fill>` are not formulas; `xm:f` is another namespace). |
| `FormulaScanFailed` | A sheet part could not be scanned (not well-formed, UTF-16, DTD): edits refused, reads still work. |
| `MergedCell` | The cell is in a merged range, top-left cell included. |
| `TableHeaderCell` | The cell is in a table's header row (`headerRowCount` rows from the top of `ref`). |
| `RichValueCell` | The cell has a `vm` attribute (image in cell, data type). |
| `Malformed` | `dimension` exists but its `ref` is missing or not a cell range, so it could not be kept up to date. |
| `MissingReference` | Reads **and** writes, when any `row` in `sheetData`, or any `c` in the row concerned, lacks `r`. |
| `OutOfOrder`, `InvalidReference`, `OutOfRange` | Rows/cells not ascending; a `c/@r` naming another row; indices beyond 1,048,576 × 16,384. |
| `Core(EncryptedPackage)`, `Core(StrictNotSupported)`, `Core(DtdNotAllowed)` | From `ooxml-core` at open. |
| `Core(UnresolvedSibling)` | An `mc:AlternateContent` child of `sheetData`, of the row concerned, of the target cell or its `is`, or of a `worksheet` without `sheetData`: what it wraps may be the cell or its value, so reads **and** writes are refused instead of answering "absent"/"empty". Also when a new `sheetData`, row, cell, `v` or `is` has no schema-order position among its siblings (an AC sibling, ADR-0002 §4.3, or e.g. a row's `extLst` before its cells). |
| `Core(InvalidXmlChar)` | Text XML 1.0 cannot carry even after `_xHHHH_` escaping (checked up front; with the current escaping this does not occur). |

## Tests

`cargo test -p ooxml-xlsx` runs the roadmap §7 specification on
`reference/round_trip_corpus/fixture_minimal.xlsx` and the 25 files in
`reference/fixtures/xlsx/`:

- `tests/package.rs` — P0-PKG-1…7 (zero-edit L1 on every fixture, encrypted, Strict, sheet
  location), P0-T1/T1b on worksheet, workbook, shared strings and styles, template integrity.
- `tests/edit.rs` — P0-E1…E15: per-cell T2 sweeps (own value back, a number, a padded string)
  with L1 for every other part, T2 byte locality on the sheet, CR (calamine reads the output like
  the input except the edited cell) and re-open; same-path saves.
- `tests/refusals.rs` — P0-R1…R9 and malformed shapes; every refusal leaves the package
  dirty-free and L1-identical; every readable fixture cell agrees with calamine.
- `tests/oracle.rs` — the oracles fail when they should (stray byte, misplaced insertion, changed
  part, wrong value).

Manual Excel check (no Excel in CI): `cargo run -p ooxml-xlsx --example xlsx_manual_check -- <outdir>`
writes 11 edited files, their originals and `CHECKLIST.md`. `cargo xtask manual-check <outdir>`
runs the Excel, PowerPoint and Word checks together into one bundle with a zip.
