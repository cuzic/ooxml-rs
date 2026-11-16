# xlsx fixtures (third-party)

Real `.xlsx`/`.xlsm` files written by Excel, LibreOffice, Hancom and other producers. They cover
the shapes that the hand-made `reference/round_trip_corpus/fixture_minimal.xlsx` does not (ADR-0001
"Scope boundary", R2-M1). Each one was chosen for a specific purpose. The Phase0 acceptance tests
that use them are specified in `docs/roadmap/xlsx.md` §7.

- 29 files, 381,534 bytes in total.
- `poi/`: 27 files from Apache POI, Apache License 2.0. `poi/LICENSE` and `poi/NOTICE` are POI's
  `legal/LICENSE` and `legal/NOTICE` at the same commit.
- `calamine/`: 2 files from calamine, MIT. `calamine/LICENSE-MIT.md` is calamine's licence file.
- `o3/`: LibreOffice-recalculated values for the O3 test (generated data, not third-party files; `o3/README.md`).
- Checksums: `poi/SHA256SUMS`, `calamine/SHA256SUMS` (`sha256sum -c` from inside each directory).

## Provenance

| Directory | Upstream | Commit | Path | Licence |
|---|---|---|---|---|
| `poi/` | <https://github.com/apache/poi> | `47dcff769bb395034dddfd59c4609659b6dbc6a4` (trunk, fetched 2026-11-16) | `test-data/spreadsheet/<same file name>` | Apache-2.0 (`poi/LICENSE`, `poi/NOTICE`) |
| `calamine/` | <https://github.com/tafia/calamine> | `93e2e6b32b87a7b1e80bd036ea63bbe6f1ca90a6` (master, fetched 2026-11-16) | `tests/<same file name>` | MIT (`calamine/LICENSE-MIT.md`, © 2016 Johann Tuffe) |

The files are **unmodified** copies; the bytes match upstream (see `SHA256SUMS`). Many POI files
were attached to POI bug reports (the number in the name is the Bugzilla id) and are
redistributed by the Apache POI project as part of its Apache-2.0 source tree; that is the
licence under which they are used here.

## Handling

These are untrusted inputs. They were inspected only with `python3 -I` using `zipfile` and
`xml.etree` (scripts in `spikes/xlsx/`). Nothing in them is executed: `.xlsm` macros, OLE and
ActiveX payloads are opaque bytes to every test. Tests open them read-only and write outputs to
temporary paths.

## What each file exercises

"Producer" is `docProps/app.xml` `Application`/`AppVersion` (absent = not recorded). Facts were
produced by `spikes/xlsx/phase0_facts.py` and `spikes/xlsx/survey.py`.

### Value-edit targets (formula-free)

| File | Producer | Exercises |
|---|---|---|
| `poi/61605.xlsx` | Excel 15.0300 | **Primary Phase0 edit fixture.** One sheet named `Лист1` (Cyrillic). `mc:Ignorable="x14ac"` with `x14ac:dyDescent` on every `row`. `row/@spans` everywhere. 45 self-closing styled cells (`<c r="B1" s="16"/>`) and 1 shared string. 6 merged ranges. A hyperlink with a sheet relationship. `printerSettings1.bin`, `docProps/custom.xml`. `dimension` `B1:E15`. |
| `poi/FillWithoutColor.xlsx` | Excel 16.0300 | 2 sheets. 513 self-closing styled cells. A **self-closing `<row r="2" …/>`** with `spans`, `ht`, `thickBot`, `x14ac:dyDescent` (gains its first cell: ADR-0001 R2-B2 on a `row`). 24 merges on sheet 1 (40 in total). Shared strings with `xml:space="preserve"`. 4 conditional-formatting `<formula>` elements and **no** cell formulas, so a byte scan for `<f` gives a false positive (roadmap §5). |
| `poi/reordered_sheets.xlsx` | Excel 12.0000 | **Sheet order ≠ part order**: workbook order is `Sheet4, Sheet2, Sheet3, Sheet1`; `Sheet4` → `sheet1.xml`, `Sheet1` → `sheet4.xml`. Workbook rels are listed out of id order. The first sheet is **`<sheetData/>`** (the from-scratch template's shape). |
| `poi/62272.xlsx` | LibreOffice 7.3.7.2 | **Zip entries with data descriptors** (9 of 9). LibreOffice's attribute-heavy `row`/`sheetView` output. Shared string with `xml:space="preserve"`. |
| `poi/TwoSheetsOneHidden.xlsx` | Excel 12.0000 | 2 sheets; the **first sheet in workbook order is hidden** (roadmap Q2: skipped, so Phase0 edits `Sheet2`). `phoneticPr`. |
| `poi/64759.xlsx` | Excel 16.0300 | **Threaded comments** + `persons` + fallback legacy note (`comments`, `vmlDrawing`) on cells of the first sheet. |
| `poi/57362.xlsx` | Excel 15.0300 | A **chart** (`drawing` → `chart1.xml`) whose series reads the first sheet's cells; its `c:numCache` goes stale when they are edited (roadmap §4). |
| `poi/51850.xlsx` | Excel 12.0000 | **x14 data validation** in the second sheet's `extLst` with `xm:f` formulas, which are not cell formulas. `sortState` and a `drawing` on the first sheet. |
| `poi/54399.xlsx` | Excel 14.0300 | Two **external links** (`externalLinks/`), three `customXml` items with `itemProps`, `docProps/custom.xml`. |
| `poi/ExcelPivotTableSample.xlsx` | Excel 16.0300 | 2 **pivot tables** + caches + `connections.xml`; a **table** `E1:G4` on the first sheet (header-cell guard, roadmap Q5). `mc:AlternateContent` (`x15ac:absPath`) in `workbook.xml`. |
| `poi/62629_target.xlsm` | Excel 16.0300 | **VBA** (`vbaProject.bin`), `ctrlProps`, VML. Sheet 2 has **`mc:AlternateContent` as a direct child of `worksheet`** (after `legacyDrawing`, around `controls`): ADR-0002 §4.3. A drawing `<from>` element (another `<f` byte false positive). First sheet `<sheetData/>` with a Mac `mx:PLV` `extLst`. `xr:uid` attributes. |
| `poi/52348.xlsx` | not recorded | **Inline strings with rich runs** (24), every `t` with `xml:space="preserve"`; 137 valueless `t="inlineStr"` cells. 27 merges. **No** `sharedStrings.xml`. Data descriptors (9). A drawing part. |
| `poi/style-alternate-content.xlsx` | Hancom Office Hancell 2014 | **`mc:AlternateContent` inside `styles.xml`** (Hancom `hs:` font extensions), `x:`-prefixed SML, Korean font names, 12 merges. A non-Microsoft producer for the Phase2 style registry ("treat as opaque"). |
| `calamine/pictures_in_cell_and_over_cell.xlsx` | Mac Excel 16.0300 | **Images in cells**: `A1`/`A3` are `t="e" vm="…"` bound through `metadata.xml` and `richData/*`; plus a floating picture. The `vm`-cell guard (roadmap Q4). |
| `calamine/renamed_workbook_part.xlsx` | not recorded (synthetic) | Workbook part at **`xl/workbook2.xml`** and worksheet at `xl/sheet1.xml`: no hard-coded part names (ADR-0002 §6). No `dimension`. Inline string. |

### Formula-bearing (Phase0 refusal; Phase1 / ADR-0003 targets)

| File | Producer | Exercises |
|---|---|---|
| `poi/shared_formulas.xlsx` | Excel 12.0000 | 40 **shared formulas** with `t="str"` results; `calcChain.xml` present. |
| `poi/MatrixFormulaEvalTestData.xlsx` | Excel 16.0300 | 32 **array (CSE) formulas** (anchor `f t="array" ref=…`, result cells with only `v`); 246 error cells (`t="e"`); merges; `calcChain.xml`. |
| `poi/xlookup.xlsx` | Excel Online 16.0300 | **Dynamic array**: `c/@cm` + `metadata.xml`; `_xlfn.` functions; `calcChain.xml`. |
| `poi/70005-countifs.xlsx` | LibreOffice 25.2.7.2 | Formulas **without `calcChain.xml`** (ADR-0001 R3-M1). Data descriptors (11). |
| `poi/NewStyleConditionalFormattings.xlsx` | Excel 14.0300 | **x14 conditional formatting** (data bars, icon sets) in `extLst`, with 304 shared/plain formulas. |

### Refusal and edge cases

| File | Producer | Exercises |
|---|---|---|
| `poi/59746_NoRowNums.xlsx` | not recorded | **Rows and cells without `r`** (71 rows, 669 cells); worksheet part named `xl/worksheets/sheet.xml`. `Err(MissingReference)` for reads and writes. |
| `poi/InlineString.xlsx` | records Excel 16.0300, content hand-edited | **`x:`-prefixed** SML with indentation whitespace text nodes; a row and a cell without `r`; inline rich text with `xml:space="preserve"` and an astral-plane character reference (`&#x1f61c;`). |
| `poi/xlmmacro.xlsm` | Excel 16.0300 | The **first sheet in workbook order is an XLM macro sheet** (`xl/macrosheets/sheet1.xml`), not a worksheet (roadmap Q2: skipped, so Phase0 edits `Sheet1`). |
| `poi/SimpleStrict.xlsx` | not recorded | **Strict OOXML** namespaces; Phase0 `Err`. Contains a `t="d"` ISO date cell. |
| `poi/protected_passtika.xlsx` | – | **Password-encrypted** (CFB container, magic `D0 CF 11 E0`), not a zip: `Err(EncryptedPackage)`. |

### Range features (Phase4, ADR-0014)

| File | Producer | Exercises |
|---|---|---|
| `poi/bug66827.xlsx` | Excel 12.0000 | A sheet-local **`_xlnm._FilterDatabase` left behind** without an `autoFilter`, and a sheet-level **`sortState`** (ADR-0014 §7: the name is reused, the sort state kept). |
| `poi/57523.xlsx` | Excel 14.0300 | Two leftover `_FilterDatabase` names (one per sheet, `localSheetId` 0 and 1) and a sheet name that needs quoting (`'Attribute Master'`); sheet-level `sortState` on both sheets. |
| `poi/absolute-anchor-over-empty-sheet.xlsx` | Excel 16.0300 | A sheet-local **and a workbook-scope** `_FilterDatabase`, both `#REF!` (the workbook-scope one is never touched). |
| `poi/workbookProtection-sheet_password-2013.xlsx` | Excel 15.0300 | **SHA-512 sheet protection** (`algorithmName`, `hashValue`, `saltValue`, `spinCount="100000"`); the password is `pwd` (POI `TestXSSFSheet.java` L1357–1358). Hash vector of ADR-0014 §11. |

## Coverage against ADR-0001's "not covered" list

| Shape | Files |
|---|---|
| shared formulas | `shared_formulas`, `NewStyleConditionalFormattings` |
| array formulas | `MatrixFormulaEvalTestData`, `xlookup` (dynamic) |
| `mc:Ignorable` | `61605`, `FillWithoutColor`, `64759`, `62629_target`, `ExcelPivotTableSample`, … (all Excel ≥ 2010 files) |
| `xml:space="preserve"` | `52348`, `InlineString` (sheet); `FillWithoutColor`, `62272` (shared strings) |
| zip entries with data descriptors | `62272`, `70005-countifs`, `52348` |
| `<row spans>` | every Excel-authored file |
| `<sheetData/>` with zero rows | `reordered_sheets` (first sheet), `62629_target`, `xlmmacro` |
| merged cells | `61605`, `FillWithoutColor`, `52348`, `style-alternate-content` |
| rich-text inline strings | `52348`, `InlineString` |
| cells without `r` | `59746_NoRowNums`, `InlineString` |
| `calcChain` present / absent | `shared_formulas`, `MatrixFormulaEvalTestData`, `xlookup` / `70005-countifs` |
| multiple / reordered sheets | `reordered_sheets`, `FillWithoutColor`, `TwoSheetsOneHidden`, `xlmmacro` |

Not found in either corpus (as of the commits above): a workbook saved by Excel whose sheet
*parts* are out of workbook order with consistent names (`reordered_sheets` is the closest), and
any file with sparklines or slicers.

## Adding files

Keep the set small and purposeful (cap: 5 MB). For each new file: record upstream URL, commit,
path and licence above; add it to `SHA256SUMS`; say what it exercises that no existing file does;
inspect it only with `python3 -I`.
