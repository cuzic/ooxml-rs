# ooxml-xlsx

Lossless Excel (`.xlsx`/`.xlsm`) editing on [`ooxml-core`](../ooxml-core): Phase0 of
[ADR-0001](../../docs/adr/0001-lossless-round-trip-architecture.md) and Phase1 of
[ADR-0003](../../docs/adr/0003-xlsx-phase1-formula-and-calc-state.md) (Accepted, revision 5),
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
use ooxml_xlsx::{CellIndex, CellValue, ExcelDateTime, Format, Workbook};

let mut wb = Workbook::open("book.xlsx")?;      // whole file read into memory
let mut ws = wb.worksheet()?;                    // "sheet1" (see below)
if let Some(c) = ws.cell(0, 0)? {                // A1, 0-based; never modifies the tree
    let v: CellValue = c.value()?;               // Number / String / Bool / Error / Date / Empty
}
ws.cell_mut(1, 1)?.set_number(42.0)?;            // B2
ws.cell_mut(0, 2)?.set_string("  padded ")?;     // C1: t="s" if the file has an sst, else inline
let f = Format::new().set_bold().set_num_format("0.00%");
ws.cell_mut(0, 3)?.set_number_with_format(0.25, &f)?;           // D1: new xf (reused if byte-identical)
ws.cell_mut(0, 4)?.set_datetime(ExcelDateTime::from_ymd(2026, 11, 16).unwrap())?; // E1
ws.cell_mut(0, 5)?.set_num_format("#,##0")?;     // F1: only the number format changes
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
| `set_string(&str)` | `t="s"` through the shared-string registry when the workbook has a `sharedStrings.xml` (Phase2, below); otherwise as `set_string_inline`. |
| `set_string_inline(&str)` | Phase0's string write: `t="inlineStr"`, `<is><t>…</t></is>`; `xml:space="preserve"` iff outer whitespace; control characters as `_xHHHH_`, a literal `_xHHHH_` protected as `_x005F_xHHHH_`; at most 32,767 UTF-16 units. Never touches `sharedStrings.xml`. |
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

## Phase2: shared strings and styles (ADR-0006)

| Item | Behaviour |
|---|---|
| Activation | Each registry is built on the first write that needs it; until then `sharedStrings.xml` and `styles.xml` are copied through raw. An active registry only appends nodes and rewrites `count`/`uniqueCount`; no existing entry is moved, re-serialised or parsed into a value. |
| `set_string` | Reuses an existing `si` only if its bytes are exactly what would be appended (a single `t`, same escaping, no `r`/`rPh`/`phoneticPr`); else appends one. `uniqueCount` = entries; `count` = `t="s"` cells in every worksheet, macro sheet and dialog sheet, recomputed (not adjusted), absent stays absent. Overwritten items stay. No sst: inline (`shared_strings_available()`); the `Workbook::new()` template ships an empty sst (Q2). |
| `Format` | Optional groups: number format, font (including `charset`, `vert_align`, `condense`, `extend`, so East Asian default fonts are expressible), fill, border, alignment, protection, plus `quote_prefix`. An unset group takes the workbook's Normal style's id (`cellStyleXfs[0]`, else `cellXfs[0]`), so `Format::new()` looks like the workbook's default; `set_bold()` starts from Calibri 11 — use `Format::with_default_font_of(&mut wb)` to keep the workbook's face and size. Values are checked against their schema enumerations and Excel's ranges (`InvalidFormat`: theme colour 0–11, font name ≤ 31 characters, family 0–14, indent ≤ 250, non-empty number format code). Colours are raw (`Rgb`, `Theme { index, tint }`, `Indexed`, `Auto`). |
| Resolution | Bottom-up: `numFmt`, `font`, `fill`, `border`, then the `xf` from those ids. Each is reused when an entry has exactly the canonical bytes (including entries a previous session appended: repeated runs stop growing the tables), else appended; `fills[0]`/`fills[1]` only as `none`/`gray125`. Empty or missing tables get their reserved entries first (default font, empty border, `xf` 0, `none` + `gray125`); a missing table is inserted in schema order. `xfId="0"` only when `cellStyleXfs` exists. Canonical forms and `apply*` flags follow rust_xlsxwriter 0.102. |
| Number formats | `NumFormat::Builtin(id)` (0–163 only; from 164 up `InvalidFormat`), `General`, or a code that is exactly a locale-independent built-in code (never 14 or 22, whose display follows the user's locale), uses the built-in id unless the file redefines it (`BuiltinNumFormatRedefined`). `Format::date()` is `NumFormat::DefaultDate`: built-in 14, or `yyyy-mm-dd` when 14 is redefined. A new custom id is above every `numFmtId` in any XML part of the package (styles, dxfs, pivot caches, extLst). Existing custom codes (id ≥ 164, no other attributes) are reused. |
| Writes with a format | `set_number_with_format`, `set_string_with_format`, `set_formula_with_format`, `set_datetime_with_format` replace `s`; `set_number`/`set_string` keep it. |
| Style-only writes | `set_format(&Format)`, `set_style_index(n)` (lossless copy of another cell's `style_index()`), `set_named_style(name)` (unique `cellStyle` name, case-insensitive; groups whose `apply*="0"` keep the cell's own id, as Excel applies Good/Bad/Neutral), and the preserving setters `set_num_format`, `set_font`, `set_fill`, `set_border`: they copy the cell's current `xf` and replace one id and its `apply*` flag. A cell that does not exist yet starts from its row's style (`customFormat="1"`), else its column's `style`, else 0. The value is untouched, so the per-cell value guards (merged range, table header, array or spill range) do not apply to them (ADR-0014 revision 3 makes the same choice for merged ranges). If any formula or defined name uses `CELL`, `INFO`, `GET.CELL`, `GET.FORMULA` or a function outside Phase1's allowlist, a style-only write invalidates dependents like a value write. |
| Values | `set_bool`, `set_error` (`#N/A`, …), `set_datetime(ExcelDateTime)`: serial in the workbook's system (1900 with Excel's false 1900-02-29, or 1904); keeps a date `s`, else derives one with built-in 14 (`yyyy-mm-dd` if 14 is redefined). `Worksheet::is_date(row, col)` (calamine's rule) and `datetime(row, col)` read them back without activating the registry (`false` without a styles part). `ExcelDateTime` values come only from validating constructors. |
| Limits | Appending past 65,490 `cellXfs`+`cellStyleXfs`, 512 fonts, 256 fills, 256 borders or 200 custom number formats is `StyleTableFull` (mapping inferred, ADR-0006 §5.10). A table already past a limit does not block writes that do not grow it. |
| Rich strings | `set_rich_string(&[(&Font, &str)])`: with an sst, a new `si` of `r` runs (`rPr` with `rFont`, in Excel's order); an existing item with exactly these bytes is reused, and a rich item never matches a plain one. Without an sst, an inline `is` of runs. As in rust_xlsxwriter, a first run in `Font::default()` has no `rPr`. Empty runs and more than 32,767 UTF-16 units in total are refused. |
| Reading formats | `Worksheet::format(row, col)` / `Workbook::format_of_style(s)` → `FormatRead::Expressible(Format)` (a projection: every group the `xf` uses, `apply*` ignored; writing it again resolves to the same `xf`) or `NotExpressible { style_index, reasons }` (`xfId ≠ 0`, `pivotButton`, `justifyLastLine`, `relativeIndent`, gradient fills, `outline`/`shadow` fonts, extension content, `mc:AlternateContent`). A read: the registry is not activated. |
| Differential formats | `Workbook::add_differential_format(&DxfFormat) -> u32`: the `dxfs` registry (Phase4's conditional formats and table styles use the index). Only the overriding parts are written; a solid fill is `bgColor` without `patternType`, as Excel writes a `dxf`. Exact-form reuse, also across sessions; `dxfs` inserted in schema order when absent. A `dxf/numFmt` is self-contained: its id comes from the same package-wide allocator as `numFmts` but is not added there. Every `dxfId`/`…DxfId` in the package counts for the dangling-reference check. |
| Not yet | Named-style + overrides, named-style creation, column/row styles: Phase4. |

Phase2 refusals (in addition to those below): `NoStylesPart`, `UnsupportedStylesPart` (root not a
SpreadsheetML `styleSheet`), `StyleTableFull`, `NumFmtIdExhausted`, `BuiltinNumFormatRedefined`,
`InvalidFormat`, `InvalidNumFormat`, `StyleIndexOutOfRange`, `NoSuchStyle`, `AmbiguousStyleName`,
`DanglingReference` (an existing `s`/`fontId`/… already points at or past a table's end, so an
append would give it a meaning), `NotExpressible` (a preserving edit or named style whose `xf`
has extension content it could not carry over, or an `mc:AlternateContent` entry),
`DateOutOfRange`, `NonexistentDate` (reading serial 60 in the 1900 system), and
`Core(UnresolvedSibling)` for a style table or sst with an `mc:AlternateContent` child where an
index or an append depends on it. Registry additions are planned on a staged clone of
`styles.xml` and installed in the same apply step as the cell, so every refusal leaves the
package byte-identical.

## Usage example

`examples/edit_cells.rs` is a runnable walk-through on a committed fixture. It:

- opens a workbook;
- prints the first visible sheet's cells;
- sets a number and a string;
- saves the result to `OUT_DIR/edit_cells.xlsx` and re-reads it;
- edits a formula workbook (dependents lose their cached values);
- shows a refusal: a Strict package refused at open.

```sh
cargo run -p ooxml-xlsx --example edit_cells -- [OUT_DIR]   # default: <temp dir>/ooxml-rs-examples
```

```rust,no_run
use ooxml_xlsx::{Error, Workbook};

let mut wb = Workbook::open("book.xlsx")?;
if let Some(why) = wb.edit_refusal() {
    eprintln!("not editable: {why}"); // e.g. a poisoned workbook; nothing changed
} else {
    let mut ws = wb.worksheet()?;
    ws.cell_mut(1, 1)?.set_number(42.5)?;
    ws.cell_mut(5, 0)?.set_string("written by ooxml-rs")?;
    wb.save("book-edited.xlsx")?;
}
# Ok::<(), Error>(())
```

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
- `tests/phase2.rs` — ADR-0006 oracles: per-package sweep over every fixture (L1 elsewhere,
  T2-append locality on both registry parts, index stability, `count`/`uniqueCount` against an
  independent count by content type, calamine values and styles, inherited default font, a
  second session adds nothing); growth bounds; AC tables; refusals byte-identical; `numFmtId`
  collisions; reserved fills; projection collisions; preserving setters and named styles; dates
  in both systems; style-only invalidation; poisoning; the from-scratch matrix against
  rust_xlsxwriter 0.102. `OOXML_XLSX_CORPUS=<dir> cargo test -p ooxml-xlsx --test phase2 --
  --ignored` runs the same sweep over a local corpus (the POI download of `spikes/xlsx-phase2/`).

Manual Excel check (no Excel in CI): `cargo run -p ooxml-xlsx --example xlsx_manual_check -- <outdir>`
writes the edited files (Phase0, Phase1 and Phase2 cases), their originals and `CHECKLIST.md`.
`cargo xtask manual-check <outdir>` runs the Excel, PowerPoint and Word checks together into one
bundle with a zip.
