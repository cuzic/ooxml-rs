# ooxml-xlsx

Lossless Excel (`.xlsx`/`.xlsm`) editing on [`ooxml-core`](../ooxml-core): Phase0 of
[ADR-0001](../../docs/adr/0001-lossless-round-trip-architecture.md) and Phase1 of
[ADR-0003](../../docs/adr/0003-xlsx-phase1-formula-and-calc-state.md) (Proposed, revision 3),
with the typed lens of [ADR-0002](../../docs/adr/0002-schema-driven-code-generation.md). The
Phase0 acceptance tests are specified in [`docs/roadmap/xlsx.md`](../../docs/roadmap/xlsx.md) §7,
the Phase1 ones in ADR-0003 §7.

- **Phase0** = open a workbook written by anyone, overwrite or create cell **values**, save, and
  lose nothing else.
- **Phase1** = the same in workbooks **with formulas**, on any worksheet, plus writing and
  removing formulas. No reader ever sees a stale cell-formula value: every formula that may
  depend on an edited cell loses its cached value.

It is narrow on purpose: whatever it cannot do safely, it refuses with an `Err` and changes
nothing.

## API

```rust
use ooxml_xlsx::{CellIndex, CellValue, Workbook};

let mut wb = Workbook::open("book.xlsx")?;      // whole file read into memory
let mut ws = wb.worksheet()?;                    // "sheet1" (see below)
if let Some(c) = ws.cell(0, 0)? {                // A1, 0-based; never modifies the tree
    let v: CellValue = c.value()?;               // Number / String / Bool / Error / Date / Empty
}
ws.cell_mut(1, 1)?.set_number(42.0)?;            // B2
ws.cell_mut(0, 2)?.set_string("  padded ")?;     // C1: t="inlineStr", xml:space="preserve"
wb.save("book.xlsx")?;                           // temp file + rename: same path is safe

let mut wb = Workbook::open("formulas.xlsx")?;
let mut ws = wb.worksheet_by_name("Inputs")?;    // case-insensitive; or worksheet_by_index(i)
ws.cell_mut(0, 0)?.set_number(5.0)?;             // dependents lose v/t, fullCalcOnLoad set
ws.cell_mut(0, 1)?.set_formula("A1*2")?;         // no cached value; calcChain.xml removed
ws.cell_mut(0, 2)?.remove_formula()?;
let deps = wb.dependents("Inputs", CellIndex::parse("A1")?)?; // what an edit would invalidate
wb.save("formulas.xlsx")?;

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
| `Worksheet::cells()` | Every existing cell in document order (row by row, left to right), in one pass over the sheet's row/cell index: the bulk read (1M cells ≈ 0.6 s). A row whose cells cannot be located yields its error; a sheet-level problem yields one error. |
| Row/cell index | Built once per worksheet when it is opened. It is one validating pass with the same rules as before. `mc:AlternateContent` is recorded apart: the target cell's read or write refuses it, but stripping dependents' caches does not, as before. Lookups are binary searches. The target write keeps the index valid: new rows and new cells are placed with ooxml-core's `insert_at` at the indexed position, checked by `check_insert_at` in the plan. Appending a row or cell is O(1). Inserting before existing rows or cells costs time in proportion to the items after it, as the tree's own insertion does. Reads, writes and Phase1 stripping no longer rescan every row. |
| `cell_mut(row, col)` | A handle; nothing is created or dirtied until a setter runs. |
| `set_number(f64)` | `<v>` gets the number; a stale `t` and every `<is>` are removed; `s` kept. NaN/∞ refused. |
| `set_string(&str)` | `t="inlineStr"`, `<is><t>…</t></is>`; `xml:space="preserve"` iff outer whitespace; control characters as `_xHHHH_`, a literal `_xHHHH_` protected as `_x005F_xHHHH_`; at most 32,767 UTF-16 units. Never touches `sharedStrings.xml`. |
| Malformed cells | Every setter removes **every** `<f>`, `<v>` and `<is>` of the cell before writing the new content, so a schema-invalid duplicate (two `v`, two `is`, `v` and `is` together, also around or after an `extLst`) is cleaned up rather than left for another reader to find; the write is `Ok`. One element of the kind the new content needs (the first `v` for a number, the first `f` for a formula, the first `is` holding a single `t` for a string) is reused in place, so comments, processing instructions and unknown children keep their position relative to the value; a new kind goes to its schema slot. |
| New rows/cells | Inserted in row/column order with `r`; no whitespace added; existing `spans` left as is. |
| `dimension` | Grown to include the written cell if present; **never inserted** if absent. Accepted `ref` spellings: `A1` or `A1:D2` with column letters in either case, optional `$` before the column and/or row, corners in either order, within 1,048,576 × 16,384. Untouched when the cell is inside it; when grown it is rewritten in canonical form (`A1:F9`). Any other `ref` (surrounding spaces, `Sheet1!A1`, `A0`, a third part, empty) or a missing one refuses the write (`Malformed`). |
| `Workbook::new` | Opens `template/blank.xlsx` (built by `template/build.py` from `template/blank/`) through the same code path; `docProps/core.xml` is regenerated (created/modified = now) with `Package::set_part`. |
| `add_worksheet()` | First call on a `new()` workbook returns the template's sheet; every other call, and any call on an opened file, `Err(SheetAdditionUnsupported)`. `add_worksheet_with_name` is always `Err(SheetNamingUnsupported)`. |

## Phase1: formula workbooks (ADR-0003)

| Item | Behaviour |
|---|---|
| Census at open | Every worksheet, macro and dialog sheet is scanned with quick-xml events by element name: formula cells (normal, shared, array, data table), result ranges, XLDAPR dynamic arrays (`cm` + `metadata.xml`), `vm` cells. No tree is built; unedited parts stay raw-copied. |
| Precedents | Sound by default: a formula gets precise rectangles only if every construct is allowlisted (A1 references and ranges, whole rows/columns, cross-sheet and 3-D references, absolute defined names with Excel's local/global/sheet-qualified resolution, operators, and the functions in `src/formula/functions.rs`, each with its evidence). Anything else (`INDIRECT`, `OFFSET`, `LOOKUP`, UDFs, structured and external references, unknown sheets or names, sheet or defined names that fold alike, relative names, unqualified references inside names, shared groups with two masters for one `si`, `ca`, data tables, `#SPILL!`) depends on everything. |
| Value edit | Every dependent (transitively; array and spill results included; a dependent dynamic array seeds its potential spill area) loses `v`, `t` and `is`, nothing else; `calcPr/@fullCalcOnLoad="1"` is set (inserted if needed) unless the workbook is in manual mode; `calcChain.xml` is kept. |
| `set_formula`, `remove_formula`, value over a formula | As above, and `calcChain.xml` (part, relationship, content-type override) is removed when the set of formula cells changes. Overwriting a shared master hands the group to the first remaining member (text translated, `ref` = bounding rectangle). |
| `CalcPolicy::KeepStaleCaches` | Opt-in, Excel only: keep stale caches, only set `fullCalcOnLoad`. |
| Atomicity | Every edit is planned and validated without changing anything (only `workbook.xml` is cloned), then applied in place; a failure while applying (a bug) poisons the workbook (`Err(Poisoned)` for every later edit and save). |
| Cost | Proportional to the dependent set: 2.0 ms per edit on a 200,000-formula, 9.7 MB sheet; 6.7 ms in the worst case where every formula is a dependent (`examples/phase1_bench.rs`, `WORST=1`). |

## Refusals (`Err`, nothing changed)

Every write is planned first: all refusals, the row/cell insertion points, the new cell content,
the dependents to strip, the `calcPr` change, the calcChain removal and the new
`dimension/@ref` are decided, and each insertion is dry-run against the siblings it will have,
before anything is touched; only then is the plan applied, by steps that cannot fail. After an
`Err` no part is dirty and the workbook saves byte-identical to the input.

| Error | When |
|---|---|
| `FormulaScanFailed` | A sheet part (or `metadata.xml`) could not be scanned (not well-formed, UTF-16, DTD, an `f` outside a cell, a formula sheet not listed in `<sheets>`): every edit is refused, reads still work. |
| `MissingReference` | Reads and writes, when any `row` in `sheetData`, or any `c` in the row concerned, lacks `r`; **every** edit when a sheet with formulas lacks `r`. |
| `UnknownCellMetadata` | The workbook has a `cm` that is not an XLDAPR dynamic array: every edit. |
| `ArrayResultCell`, `ArrayAnchor` | A write into an array/data-table/spill range, or to its anchor. |
| `MacroSheetDependent` | The edit would invalidate an XLM macro-sheet formula. |
| `IterativeCalculation` | `calcPr/@iterate` and the edit has dependents. |
| `SharedFormulaMaster` | Overwriting a shared master whose text cannot be moved (a reference would leave the sheet, or collides with a name). |
| `InvalidFormula` | `set_formula` text that does not lex or has characters XML cannot carry. |
| `NoSuchSheet`, `NotAWorksheet` | `worksheet_by_name`/`by_index` miss, or name a chartsheet/macro/dialog sheet. |
| `Poisoned` | An earlier edit failed while applying. |
| `MergedCell` | The cell is in a merged range, top-left cell included. |
| `TableHeaderCell` | The cell is in a table's header row (`headerRowCount` rows from the top of `ref`). |
| `RichValueCell` | The cell has a `vm` attribute (image in cell, data type), or a dependent has one. |
| `OutOfOrder`, `InvalidReference`, `OutOfRange` | Rows/cells not ascending; a `c/@r` naming another row; indices beyond 1,048,576 × 16,384. |
| `Core(EncryptedPackage)`, `Core(StrictNotSupported)`, `Core(DtdNotAllowed)` | From `ooxml-core` at open. |
| `Malformed` | `dimension` exists but its `ref` is missing or not a cell range, so it could not be kept up to date; or the census and the tree disagree about a dependent. |
| `Core(UnresolvedSibling)` | An `mc:AlternateContent` child of `sheetData`, of the row concerned, of the target cell or its `is`, or of a `worksheet` without `sheetData`: what it wraps may be the cell or its value, so reads **and** writes are refused instead of answering "absent"/"empty". Also when a new `sheetData`, row, cell, `f`, `v` or `is` has no schema-order position among its siblings (an AC sibling, ADR-0002 §4.3, or e.g. a row's `extLst` before its cells). |
| `Core(InvalidXmlChar)` | Text XML 1.0 cannot carry even after `_xHHHH_` escaping (checked up front; with the current escaping this does not occur for strings). |

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
- `tests/phase1_o1.rs` — O1: hand-written dependent sets on `reference/fixtures/xlsx/synthetic/`
  (README there), with the allowed-diff and no-stale-value (calamine) oracles.
- `tests/phase1_o2.rs` — O2: 400 random workbooks, a test-only evaluator; computed dependents ⊇
  observed changes.
- `tests/phase1_o3.rs` — O3: kept cached values against LibreOffice's recalculation, recorded in
  `reference/fixtures/xlsx/o3/expected.tsv` (generated by `spikes/xlsx/o3_recalc.py` in the
  `o3-recalc` workflow; README there).
- `tests/phase1_formulas.rs`, `tests/phase1_fixtures.rs` — formula writes and removals, hand-off,
  calcChain removal, validate-before-mutate, fault injection (`--features fault-injection`), the
  POI formula fixtures, tokenizer sweep.

Manual Excel check (no Excel in CI): `cargo run -p ooxml-xlsx --example manual_check -- <outdir>`
writes 21 edited files (11 Phase0, 10 Phase1), their originals and `CHECKLIST.md`.
