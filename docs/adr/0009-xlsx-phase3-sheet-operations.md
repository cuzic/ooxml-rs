# ADR-0009: xlsx Phase3 — sheet operations on existing files

- Status: **Proposed** (revision 1)
- Date: 2026-11-16
- Depends on:
  - [ADR-0001](0001-lossless-round-trip-architecture.md): Layer 3 append-only invariant, and the
    interim note on `remove_part` in PR #14;
  - [ADR-0002](0002-schema-driven-code-generation.md): lens, allowlist, §4.3 insertion;
  - [ADR-0003](0003-xlsx-phase1-formula-and-calc-state.md): census, tokenizer, `Everything`,
    plan → validate → apply, calcChain policy (PR #20);
  - [ADR-0006](0006-xlsx-phase2-shared-strings-and-styles.md): registries (PR #22).
- Roadmap: [`docs/roadmap/xlsx.md`](../roadmap/xlsx.md) §2.2 (A4–A7), §6 Phase3
- Evidence: [`spikes/xlsx-phase3/`](../../spikes/xlsx-phase3/README.md)

## Context

Phase3 is the first xlsx phase that **adds and removes parts** in an existing package:

- add, delete, rename, reorder and hide worksheets;
- create, edit and delete defined names, workbook-wide or sheet-local;
- insert, replace or remove a VBA project (`.xlsm`);
- keep `docProps/app.xml` (`HeadingPairs`/`TitlesOfParts`) consistent.

ADR-0001 kept all of this out of Phase0, because it touches `[Content_Types].xml`, `.rels`,
`workbook.xml` and `app.xml` together. The core primitive for *removal* exists as PR #14
(`Package::remove_part(name, allowed_rel_types)`, plus `check_remove_part`). Its ADR-0001
interim note limits it to `calcChain.xml`. Nothing in `ooxml-core` can *add* a part or a
relationship yet.

A sheet operation is not local:

- **References by name.** A sheet's name appears in:
  - formulas on every sheet (`Sheet2!A1`, `'Q1 2024'!A1`, 3-D `Sheet1:Sheet3!A1`);
  - defined names (`_xlnm.Print_Area` included);
  - chart series (`c:f`);
  - pivot cache sources (`worksheetSource/@sheet`);
  - data validation and conditional formatting formulas (2007 and x14 `xm:f`);
  - sparklines;
  - hyperlink locations (`hyperlink/@location`, `#Sheet!A1` targets);
  - `app.xml` titles.
- **References by position.** `definedName/@localSheetId`, `workbookView/@activeTab` and
  `@firstSheet` hold a sheet's *index* in `<sheets>`.
- **References by `sheetId`.** `calcChain.xml` `c/@i`.
- **Formula results** that depend on the set or order of sheets: 3-D references, `SHEET()`,
  `SHEETS()`, `CELL("filename")`.

### What the reference libraries do (verified in source)

- **openpyxl 3.1.5**:
  - On save it **renumbers** `sheetId` to the position and the relationship id to `rId{position}`
    (`workbook/_writer.py`, `write_worksheets`).
  - The title setter validates characters (`INVALID_TITLE_REGEX = [\\*?:/\[\]]`), warns above
    31 characters and de-duplicates by suffix. It does **not** rewrite any reference
    (`workbook/child.py`).
  - `remove()` and `move_sheet()` are list operations (`workbook/workbook.py`).
  - Sheet-local names live on the sheet object, so `localSheetId` is recomputed from the
    position at write time (`write_names`).
  - `app.xml` is written with `HeadingPairs`/`TitlesOfParts` set to `None`
    (`packaging/extended.py`).
- **umya-spreadsheet 3.1.1** (`MathNya/umya-spreadsheet@88279e86`):
  - It also renumbers `sheetId`/`rId` by position (`src/writer/xlsx/workbook.rs`).
  - `new_sheet` uses `sheetId = len + 1` (`src/structs/workbook.rs`).
  - `Worksheet::set_name` updates only the sheet's own defined names (`src/structs/worksheet.rs`).
  - `remove_sheet` drops the sheet from the collection and touches no reference.
  - `app.xml` is regenerated with a single "Worksheets" heading (`src/structs/properties.rs`),
    so localized labels and "Named Ranges" are lost.
- **Apache POI** (`47dcff76`, `XSSFWorkbook.java`, `XSSFFormulaUtils.java`, `WorkbookUtil.java`):
  - `createSheet`: `sheetId` = max + 1. The part name `sheet{N}.xml` starts at that number and is
    bumped until free (Bug 57165).
  - `removeSheetAt`:
    - removes `calcChain.xml`;
    - drops names local to the deleted sheet and decrements later `localSheetId`s;
    - removes pivot caches that only the deleted sheet's pivot tables used;
    - adjusts the active sheet;
    - does **not** rewrite formulas that refer to the deleted sheet.
  - `setSheetName` (`XSSFFormulaUtils.updateSheetName`) rewrites:
    - defined names (workbook-wide, and those local to that sheet);
    - cell formulas on all sheets;
    - charts in **the renamed sheet's own drawings only**.

    Data validation, conditional formatting, hyperlinks, pivot caches and charts on other
    sheets are not rewritten.
  - `setSheetOrder` remaps `localSheetId` and the active sheet.
  - `validateSheetName`: 1–31 characters; none of `/ \ ? * [ ] :`, U+0000 or U+0003; must not
    begin or end with `'`.
- **rust_xlsxwriter 0.102.0** (`src/workbook.rs`, `src/packager.rs`), when adding a VBA project:
  - sets `workbookPr/@codeName="ThisWorkbook"` and each sheet's `codeName` `Sheet{n}`;
  - writes the workbook content type `…sheet.macroEnabled.main+xml`, the content-type Default
    `bin` = `application/vnd.ms-office.vbaProject`, and the relationship type
    `http://schemas.microsoft.com/office/2006/relationships/vbaProject`;
  - puts an optional signature in `xl/_rels/vbaProject.bin.rels`.
- **Excel** (Microsoft Support, "Rename a worksheet"): names must not be blank, must not begin
  or end with an apostrophe, and must not be `History` (reserved). They are unique without
  regard to case.

None of the libraries keeps `sheetId`s, relationship ids and `app.xml` stable *and* rewrites
every by-name reference. openpyxl and umya rebuild. POI rewrites a subset.

### What the corpus shows

`spikes/xlsx-phase3/sheet_ops_survey.py` over 342 Transitional POI packages and the 24 committed
fixtures. The full table is in the spike README.

1. **`sheetId`s have gaps in 16%** (56/342), are not ascending in workbook order in 39, and are
   never duplicated. **Part numbers do not follow `sheetId`** in 23% (78/342). So allocation
   must not assume `sheetN.xml` = `sheetId` = position.
2. **Relationship ids** are all `rIdN` in 338/342, and contiguous `rId1…rIdN` in 337.
3. **Every sheet part has a content-type `Override`** in 340/342.
4. **`[Content_Types].xml` `Default Extension="bin"` is the printer-settings type in 110
   packages.** A VBA project added as `vbaProject.bin` therefore cannot rely on a `bin`
   Default, unlike rust_xlsxwriter, which always writes a new package. It needs an `Override`.
5. **`app.xml`.**
   - Present in 331, with `HeadingPairs` + `TitlesOfParts` in 294.
   - In those 294, the heading counts sum to the titles in 293.
   - The first block lists the worksheets in workbook order in 290. Excel leaves
     **`veryHidden` sheets out**, and puts macro sheets in their own block ("Excel 4.0
     Macros").
   - Labels are **localized**: "Worksheets" 238, "Arbeitsblätter" 20, "工作表" 8,
     "ワークシート" 6, …. "Named Ranges" (32 packages) lists names such as `'1'!Print_Area`.
   - The 4 mismatches are stale or empty `app.xml`s (`60289`, `bug66675`), the Excel 2007 beta
     file, and an `_x000a_` escape in a sheet name.
6. **Defined names** occur in 63 packages: sheet-local in 25, `_xlnm.*` in 22, `#REF!` in 12.
   No `localSheetId` is out of range. `activeTab > 0` in 38 and `firstSheet > 0` in 10.
7. **Rename impact.** In **62 of the 196 multi-sheet packages (32%)** some sheet is referenced
   by name. By location:
   - defined names: 46 packages;
   - chart `c:f`: 25;
   - cell formulas: 17;
   - pivot cache `worksheetSource`: 5;
   - x14 `xm:f` (DV/CF/sparklines): 3;
   - `hyperlink/@location`: 3;
   - 3-D references: 3 packages;
   - sheet names inside string literals (e.g. `INDIRECT("Sheet1!A1")`): 0.
8. **Delete impact.** The relationship types in worksheets' own `.rels`, by number of packages:
   - `printerSettings` 110, `drawing` 61, `vmlDrawing` 37;
   - `table` 29, `comments` 27, `hyperlink` 14, `tableSingleCells` 14;
   - `pivotTable` 7, `oleObject` 5, `customProperty` 5, `image` 4, `threadedComment` 3,
     `control` 2, `package` 2.
9. **VBA.** A VBA project in 9 packages, none signed. A workbook `codeName` in 35, sheet
   `codeName`s in 32. `calcChain.xml` in 117, external links in 10.

## Decision

### 1. Scope

In scope:

- `Workbook::add_worksheet(name, position)` on an opened file.
- `delete_sheet`, `rename_sheet`, `move_sheet`, and `set_sheet_state` (visible, hidden,
  veryHidden), for worksheets. Chart sheets, macro sheets and dialog sheets are covered by
  rename, reorder and hide. Adding them is out of scope; deleting them is covered by the same
  cascade rules as worksheets (§5).
- Defined names: `add_name`, `set_name_formula`, `rename_name`, `delete_name`, workbook-wide or
  sheet-local. `_xlnm.Print_Area`/`_xlnm.Print_Titles` are set through dedicated setters.
- VBA: `set_vba_project(bin, signature)`, `remove_vba_project()`.
- `docProps/app.xml` maintenance (§11).
- The `ooxml-core` Layer 3 write primitives these need (§2).

Out of scope:

- Copying or moving a sheet between workbooks, and copying within a workbook: roadmap G2,
  Phase7.
- Inserting or deleting rows and columns: G1, Phase7.
- Creating chart sheets: Phase5.
- Editing VBA source, signing.
- External-link maintenance: other workbooks that refer to this one are not ours to change.

### 2. Core primitives (`ooxml-core`, separate PR)

All of these keep ADR-0001's invariant that existing ids are never renumbered.

- **`Package::add_part(name, content_type, bytes)`**.
  - Refused if a part with an OPC-equivalent name exists (case-insensitive, percent-decoded,
    the same equivalence as `remove_part`, PR #14 round 2).
  - Adds an `Override` unless an existing `Default` for the extension already gives exactly
    that content type. It never adds or changes a `Default`.
  - The new part is `Replaced` (Layer 1's third state).
- **`Package::add_relationship(source, rel_type, target, mode) -> Id`**.
  - The new id is `rId{n}`, with `n` = 1 + the largest numeric suffix among the source's `rIdN`
    ids. It is checked to be free; non-`rIdN` ids (4 corpus packages) are kept.
  - The new `Relationship` is appended to the `.rels` part, which is created if absent.
- **`Package::set_content_type(part, ct)`** changes an `Override`, or adds one that shadows a
  `Default`. Used for the workbook part (xlsx ↔ xlsm) only.
- **`Package::remove_part`** (PR #14), used with explicit relationship-type allowlists per
  caller (§5).
- Every primitive has a `check_*` twin that validates without changing anything. Phase3's plan
  step calls those (ADR-0003 §3.1).

Each needs an ADR-0001 post-acceptance note. It widens the PR #14 interim note:

- parts may be **added** by these operations;
- parts may be **removed** as part of a sheet delete or VBA removal, under §5's cascade rules.

Remaining entries are never renumbered. This is an owner decision (Q1).

### 3. Ids and names for new things

- **`sheetId`:** 1 + the maximum of every `sheetId` in `workbook.xml` and every id deleted in
  this session. Ids are never reused: `calcChain c/@i` and some add-ins key on them. This
  matches POI and Excel's evident behaviour (the 56 gap files). openpyxl and umya renumber.
- **Part name:** `xl/worksheets/sheet{N}.xml`, with N starting at the new `sheetId` and bumped
  until no OPC-equivalent part name exists (POI's Bug 57165 rule). The folder of the existing
  sheets is used if they share one, otherwise `xl/worksheets/`.
- **Relationship id:** §2.
- **Sheet name validation:**
  - 1–31 UTF-16 code units;
  - none of `: \ / ? * [ ]`, U+0000 or U+0003;
  - must not begin or end with `'`;
  - not `History` (case-insensitive);
  - unique among all sheets.

  Uniqueness uses the same case folding as ADR-0003 §2.4 for sheet-name matching. A broader
  fold can only refuse more. Violations give `Err(InvalidSheetName { reason })`. Names are
  never truncated or suffixed, unlike POI and openpyxl.
- **Content of a new worksheet:** a minimal part, taken from the worksheet of the same
  embedded template `Workbook::new()` uses (`dimension ref="A1"`,
  `sheetViews/sheetView workbookViewId="0"`, `sheetFormatPr`, empty `sheetData`,
  `pageMargins`). `sheetFormatPr/@defaultRowHeight` is copied from the first visible
  worksheet, so row heights match the workbook's default font.

### 4. Add a worksheet

Plan:

1. Allocate the part, `sheetId` and relationship id (§3).
2. Insert `<sheet name sheetId r:id>` at `position` in `<sheets>`, a Layer 2 edit of
   `workbook.xml`.
3. Renumber by position:
   - every `definedName/@localSheetId` ≥ position: +1;
   - `workbookView/@activeTab` and `@firstSheet` ≥ position: +1.
4. Update `app.xml` (§11).
5. **Phase1** (§12): formulas with 3-D references whose span now includes the new sheet, and
   formulas calling `SHEETS()` or `SHEET(…)`, are dependents (cache removed,
   `fullCalcOnLoad`). `calcChain.xml` is untouched: no formula cell is created.
6. **VBA workbooks:**
   - The new sheet gets no `codeName`. Excel is expected to create the document module on the
     next open (inference; XL item).
   - `set_sheet_code_name` exists for callers who ship a matching project.

### 5. Delete a sheet

Refused when:

- it is the last sheet, or the last visible sheet (`Err(LastVisibleSheet)`);
- any pivot cache sources it (`worksheetSource/@sheet` or a name that resolves to it);
- it holds a pivot table, slicer, timeline, query table, external data connection or ActiveX
  or OLE object: `Err(SheetHasUnsupportedParts { rel_types })`. The corpus has pivot tables
  on sheets in 7 packages, OLE objects in 5 and controls in 2.

**Cascade.** The sheet's own relationships, recursively, are removed when the target part is
**exclusively** reachable from the sheet. A part that any other remaining relationship
targets (e.g. a shared image) stays. The allowed cascade, from the spike's type census:

| Relationship type (from the sheet, or recursively) | Action |
|---|---|
| `printerSettings`, `comments`, `vmlDrawing` (+ its `image`s), `threadedComment`, `table`, `tableSingleCells`, `customProperty`, `image` (background) | removed with the sheet |
| `drawing` → `image`, `chart` → (`chartStyle`, `chartColorStyle`, `chartUserShapes` → `drawing`), external `hyperlink` | removed with the sheet |
| `hyperlink` (external, no part) | dropped with the `.rels` |
| `pivotTable`, `slicer`, `timeline`, `queryTable`, `control`/`ctrlProp`, `oleObject`, `package`, `activeXControl` | refused (above) |
| any other type | refused (`SheetHasUnsupportedParts`) |

`remove_part` is called leaf-first with the allowlist of the edge being cut. Its refusals
(`StillReferenced`, `HasOwnRelationships`, …) then mean the plan was wrong, and they abort the
whole operation before anything changes (plan → validate → apply).

References to the deleted sheet are rewritten as Excel does, tokenized by Phase1's tokenizer.
Each rewrite is an XL-check item:

- a single-sheet reference `Sheet2!A1` becomes `#REF!A1`, in cell formulas, defined names,
  chart `c:f`, DV/CF formulas and `xm:f`;
- a 3-D endpoint moves inward (deleting `Sheet1` in `Sheet1:Sheet3!A1` gives
  `Sheet2:Sheet3!A1`);
- a 3-D range that becomes empty gives `#REF!`;
- a `hyperlink/@location` or `#Sheet!` target is left as is: it is a navigation target, not a
  formula, and Excel shows it as broken;
- text the tokenizer cannot lex that mentions the sheet name (case-folded substring) refuses
  the delete with `Err(UnrewritableReference { part, location })`. A silent miss is worse
  than a refusal.

Position-based state:

- names local to the deleted sheet are removed;
- later `localSheetId`s are decremented;
- `activeTab`/`firstSheet` are adjusted as POI does, and the next visible sheet becomes
  active if the deleted one was;
- the new active sheet gets `sheetView/@tabSelected="1"`.

**Phase1:** every formula that referred to the sheet (directly, through a name, 3-D) and every
`Everything` formula is a dependent. `calcChain.xml` is removed, because the set of formula
cells changed (the deleted sheet's) — ADR-0003's calcChain policy, as POI does.

**Phase2:** `sst/@count` is recomputed without the deleted sheet's references (ADR-0006 §4.5).
No style or string entry is removed (Layer 3).

### 6. Rename a sheet

Validation as in §3. The new name must not fold-equal another sheet or a defined name that
formulas could confuse with it (ADR-0003 §2.4 c).

**Rewritten** (tokenized, so only real sheet prefixes change; quoting is recomputed: quotes
when the name is not a plain identifier, looks like a cell or R1C1 reference, or contains
spaces or punctuation; `'` doubled):

| Location | Corpus packages |
|---|---|
| cell formulas on every sheet (normal, shared masters, array, data-table anchors), 3-D endpoints | 17 (3-D: 3) |
| defined names (all scopes) | 46 |
| chart `c:f` in every chart part, not only the renamed sheet's (POI's gap) | 25 |
| pivot cache `worksheetSource/@sheet` | 5 |
| DV/CF formulas (2007) and x14 `xm:f` (DV, CF, sparklines) | 3 |
| `hyperlink/@location` and `#Sheet!…` relationship targets in sheet and drawing `.rels` | 3 |
| `app.xml` titles (§11) | 294 with titles |

**Not rewritten**, because Excel itself does not rewrite these and they are not formulas:

- sheet names inside string literals (`INDIRECT("Sheet1!A1")`, `HYPERLINK("#Sheet1!A1")`; 0 in
  the corpus);
- VBA source (which uses `codeName`, unaffected);
- other workbooks' external links.

**Refused:** any location that holds formula text the tokenizer cannot lex and that mentions
the old name (case-folded), `Err(UnrewritableReference)`.

**Phase1:** a rename changes no value, except `CELL("filename", …)` results and `INFO` (the
ADR-0006 §7.3 function list, plus `CELL` with any argument). Formulas calling them become
dependents. Nothing else is invalidated. `calcChain.xml` is unaffected, because it keys on
`sheetId`.

### 7. Reorder (`move_sheet(from, to)`)

- `<sheet>` elements are moved in `workbook.xml`: a Layer 2 move, i.e. remove and insert the
  same raw node, so its bytes are unchanged.
- `localSheetId`s, `activeTab` and `firstSheet` are remapped by the permutation.
- `app.xml` order follows.
- **Phase1:** formulas with 3-D references whose span changes membership, and formulas
  calling `SHEET()`/`SHEETS()`, are dependents.

### 8. Hide and unhide

- `set_sheet_state(sheet, Visible | Hidden | VeryHidden)` sets or removes `sheet/@state`.
- Hiding the last visible sheet is `Err(LastVisibleSheet)`.
- Hiding the active sheet moves `activeTab` to the next visible sheet and sets `tabSelected`
  there.
- `app.xml`: `veryHidden` sheets are not listed (corpus fact 5), so going to or from
  `veryHidden` removes or adds the title.

### 9. Defined names

- **Scope:** workbook, or sheet-local (`localSheetId` = the sheet's *current position*,
  recomputed by §4/§5/§7).
- **Name rules:**
  - 1–255 characters;
  - starts with a letter, `_` or `\`;
  - then letters, digits, `.`, `_`, `\`, `?`;
  - not a cell reference in A1 or R1C1 form, and not `R`/`C` alone;
  - unique per scope under the ADR-0003 fold.
  - Violations give `Err(InvalidName)`.
- `_xlnm.*` names are reachable only through `set_print_area`/`set_print_titles`. Writing
  others (`_FilterDatabase`, `Criteria`, …) is refused: they belong to Phase4's autofilter
  work.
- **Formula text** must lex with Phase1's tokenizer, else `Err(InvalidNameFormula)`. Text is
  stored without a leading `=`.
- **Edit or delete:**
  - Every formula whose precedents go through the name, and every `Everything` formula, is a
    dependent (Phase1).
  - Deleting a name that formulas still use is `Err(NameInUse { count })`, unless
    `delete_name_force`. Then dependents are invalidated and the formulas keep the name text,
    which Excel evaluates to `#NAME?`.
- **`app.xml`:** names listed in the "Named Ranges" block are updated if the block exists
  (§11).

### 10. VBA project

- **`set_vba_project(bin, signature: Option<bin>)`**:
  - adds `xl/vbaProject.bin` with an **`Override`** of `application/vnd.ms-office.vbaProject`.
    A `bin` Default is often the printer-settings type (110 corpus packages), so the
    rust_xlsxwriter Default approach is wrong here;
  - adds the workbook relationship (`…/office/2006/relationships/vbaProject`);
  - sets the workbook content type to `…sheet.macroEnabled.main+xml` (`.xlsx`→`.xlsm`), or
    `…template.macroEnabled.main+xml` for templates;
  - adds the signature (`vbaProjectSignature.bin` via `xl/_rels/vbaProject.bin.rels`) when
    given;
  - sets `workbookPr/@codeName` (default `ThisWorkbook`) and each sheet's `codeName` (default
    `Sheet{n}`, skipping names in use) **only where absent**, as rust_xlsxwriter does for new
    files.
- **Replacing an existing project:** the old project and every signature part
  (`vbaProjectSignature*.bin`) are removed first. A changed project invalidates the signature.
  Existing `codeName`s are kept.
- **`remove_vba_project()`:** removes the parts and restores the non-macro workbook content
  type. `codeName`s stay (harmless).
- The binary is not inspected (CFB). Matching module names to `codeName`s is the caller's
  responsibility, and the docs say so.
- **`save` to a path whose extension does not match the workbook content type** (`.xlsx` with
  macros, `.xlsm` without) is `Err(ExtensionMismatch)`. Excel refuses such files (XL item to
  confirm the exact behaviour).

### 11. `docProps/app.xml`

`app.xml` is informational: Excel recomputes it on save. Phase3 keeps it consistent, but never
invents structure.

- **Recognized shape:**
  - `HeadingPairs` is a `vt:vector` of (`vt:lpstr` label, `vt:i4` count) pairs;
  - `TitlesOfParts` is a `vt:lpstr` vector whose size equals the sum of the counts;
  - the first block lists the worksheets that are not `veryHidden`, in workbook order.

  This holds in 290/294 corpus files that have both elements.
- **Maintained** for a recognized shape:
  - the worksheet block's titles and count;
  - a chart-sheet or macro-sheet block's titles, if that block exists;
  - the "Named Ranges" block, if it exists;
  - the `vt:vector/@size` attributes.

  Labels are never changed or translated, and blocks are never created or removed. If an
  operation would need a block that does not exist (e.g. the first chart sheet), `app.xml` is
  left untouched for that operation.
- **Unrecognized, stale or absent** `app.xml` (48 of 342 packages lack the elements or the
  part; 4 are stale) is left untouched (L1).
- This is a Layer 2 edit of `app.xml` with an allowlist entry for `ep:HeadingPairs`,
  `ep:TitlesOfParts`, `vt:vector`, `vt:variant`, `vt:lpstr`, `vt:i4`.

### 12. Interaction with Phase1 and Phase2 (summary)

| Operation | Phase1 seeds (dependents lose `v`, `fullCalcOnLoad`) | `calcChain.xml` | Phase2 |
|---|---|---|---|
| add sheet | 3-D refs whose span gains it; `SHEET()`/`SHEETS()` | untouched | new cells use registries |
| delete sheet | everything that referred to it (direct, name, 3-D); `Everything` formulas; `SHEET()`/`SHEETS()` | removed (formula set changed) | `sst/@count` recomputed |
| rename sheet | `CELL`, `INFO` callers | untouched | — |
| reorder | 3-D refs whose membership changes; `SHEET()`/`SHEETS()` | untouched | — |
| hide/unhide | none | untouched | — |
| name add/edit/delete | formulas through the name; `Everything` formulas | untouched | — |
| VBA | none | untouched | — |

All operations run plan → validate → apply (ADR-0003 §3.1), with staged clones of every
touched Layer 2 part and the core `check_*` twins for added and removed parts. An `Err` leaves
the package unchanged.

### 13. Errors

- `InvalidSheetName`
- `DuplicateSheetName`
- `LastVisibleSheet`
- `SheetHasUnsupportedParts { rel_types }`
- `SheetIsPivotSource`
- `UnrewritableReference { part, location }`
- `InvalidName`
- `DuplicateName`
- `InvalidNameFormula`
- `NameInUse { count }`
- `ExtensionMismatch`
- core `RemovePartRefused` / `AddPartRefused`

All are refusals in ADR-0001's sense: nothing changes.

## Test oracles

1. **OPC conformance** on every output:
   - every internal relationship target exists;
   - every part has exactly one content type;
   - no part is unreachable from the package root, except parts that were unreachable in the
     input;
   - relationship ids are unique per `.rels`;
   - no two part names are OPC-equivalent;
   - `sheetId`s are unique;
   - every `localSheetId` < sheet count.
2. **L1** for every part the operation does not touch.
3. **T2** on `workbook.xml`, `[Content_Types].xml`, `.rels` and `app.xml`: the diff is
   confined to the inserted, removed or moved elements and the renumbered attributes listed
   for the operation. A moved `<sheet>` keeps its bytes.
4. **Rewrite oracle.**
   - For every formula-bearing location the census found (cells, names, `c:f`, DV/CF,
     `xm:f`, pivot `@sheet`, hyperlinks), the output text equals the input text with exactly
     the sheet-prefix tokens changed.
   - The expected text is computed by an independent test-only rewriter over **openpyxl's
     tokenizer output**, generated offline and committed, as ADR-0003's differential test
     does.
   - A mutation that misses one location class must fail this oracle.
5. **CR (calamine 0.38):**
   - `sheet_names()` order and names;
   - `sheets_metadata()` visibility;
   - `defined_names()`;
   - every value of every surviving sheet reads as before, except dependents, which read as
     empty;
   - `worksheet_formula` texts equal oracle 4's.
6. **O3 (LibreOffice recalculation, the CI workflow from PR #21):**
   - after rename, recalculated values equal the input's;
   - after delete, dependents show `#REF!` and every other value is unchanged;
   - after reorder, values change only where 3-D spans changed membership.
7. **Corpus runs.** Every operation applied to every fixture and the POI corpus, in a
   throwaway harness like ADR-0006's prototype:
   - rename every sheet;
   - delete each deletable sheet;
   - add a sheet at the front and at the end;
   - reverse the order;
   - hide each sheet.

   Expected: 0 oracle failures, and every refusal typed.
8. **Mutation testing**, as in Phase0/Phase1. Mutants include:
   - `sheetId` reused;
   - `localSheetId` not renumbered;
   - `activeTab` not adjusted;
   - chart `c:f` on other sheets skipped (POI's gap);
   - `xm:f` skipped;
   - pivot `@sheet` skipped;
   - quoting not recomputed;
   - case-sensitive name match;
   - `bin` Default used for VBA;
   - cascade removing a shared image;
   - calcChain kept after delete;
   - `veryHidden` listed in `app.xml`.
9. **XL (owner):**
   - (a) rename/delete outputs on `51850`, `ExcelPivotTableSample` (refused delete),
     `TwoSheetsOneHidden`, `reordered_sheets`;
   - (b) `#REF!` forms after delete;
   - (c) a new sheet in an `.xlsm` with VBA (does Excel add the document module silently?);
   - (d) `set_vba_project` on an `.xlsx` with `printerSettings`;
   - (e) `ExtensionMismatch` behaviour.

## Alternatives considered

- **Renumber `sheetId`/`rId` by position on save (openpyxl, umya).** This is rejected by Layer
  3. It also breaks `calcChain c/@i` and anything else that keys on the id, and it rewrites
  bytes of parts the operation did not need to touch.
- **Regenerate `app.xml` (umya).** This is rejected: it loses localized labels (at least 55
  corpus packages use non-English labels) and the "Named Ranges" block.
- **Rewrite the subset POI rewrites.** This is rejected. Charts on other sheets (25 packages
  with chart refs), DV/CF/`xm:f`, pivot sources and hyperlinks would silently keep the old
  name, and a later "sheet not found" would turn them into errors.
- **Refuse rename when any reference exists.** This is rejected: it would refuse 32% of
  multi-sheet workbooks.
- **Cascade-delete everything a sheet owns, pivots included (POI).** Deferred. Pivot caches
  are shared and carry copies of the source data, and slicers and timelines hang off them.
  Refusing is honest until Phase5 models pivots.
- **A `bin` Default for VBA (rust_xlsxwriter).** This is rejected for edit mode (corpus fact 4).

## Consequences

- `ooxml-core` gains part and relationship addition and a content-type setter. ADR-0001 gains
  a post-acceptance note widening PR #14's interim exception (Q1).
- The Phase1 tokenizer becomes a rewriter. Its token spans must round-trip byte-exactly, so
  that only sheet-prefix tokens change.
- Census coverage grows. Phase1 scans worksheet `f`, names and calcChain. Phase3 also needs
  chart `c:f`, pivot cache sources, DV/CF formulas, `xm:f` and hyperlink locations. These
  are cheap scans (`spikes/xlsx-phase3/` does them in Python over the whole corpus in
  seconds).
- Sheet operations on workbooks with pivot tables, slicers, OLE objects or controls are
  refused in Phase3 (at most 16 of 342 corpus packages, under 5%).

## Open questions for the project owner

- **Q1 (ADR-0001).** May Phase3 add parts and relationships, and remove sheet-owned parts under
  §5's cascade, as a widening of PR #14's interim note? The proposed wording is in §2.
- **Q2 (delete rewrites).** Should delete rewrite references to `#REF!` as Excel does (proposed),
  or refuse deleting any sheet that is referenced?
- **Q3 (new sheet in VBA workbooks).** Leave `codeName` absent (proposed; Excel is expected to
  add the module), or always assign one?
- **Q4 (`app.xml` blocks).** Should Phase3 create a missing block (e.g. "Charts") with an
  English label, or keep "never invent structure" (proposed)?
- **Q5 (names in use).** `delete_name` refuses when formulas use the name (proposed), with
  `delete_name_force` as the escape hatch. Is that the right default?
- **Q6 (pivot-bearing sheets).** Are refusals for sheets with pivot tables, slicers, OLE
  objects or controls acceptable until Phase5?

## Sources

- ADR-0001 (Layer 3; PR #14 interim note), ADR-0002 §4.3, ADR-0003 (§1 census, §2.4 folding and
  3-D endpoints, §3.1 plan → validate → apply, calcChain policy), ADR-0006 §4.5, §7.3.
- PR #14 (`Package::remove_part`, `check_remove_part`, OPC part-name equivalence); PR #21 (O3
  LibreOffice recalculation workflow).
- ECMA-376 Part 1:
  - §18.2 (`workbook`, `sheets`, `definedNames`, `bookViews`, `workbookPr/@codeName`);
  - §18.3.1 (`sheetPr/@codeName`, `hyperlink/@location`);
  - §18.6 (calcChain `c/@i`);
  - §18.10 (pivot `worksheetSource`);
  - §21.2.2 (chart `c:f`);
  - Part 2 (OPC, content types, relationships).
- openpyxl 3.1.5 (PyPI sdist): `openpyxl/workbook/_writer.py` (`write_worksheets`,
  `write_names`), `openpyxl/workbook/child.py` (title setter), `openpyxl/workbook/workbook.py`
  (`remove`, `move_sheet`), `openpyxl/packaging/extended.py`.
- umya-spreadsheet 3.1.1 (`MathNya/umya-spreadsheet@88279e868caf1f02fb7fd5340eed3865ce138f31`):
  `src/writer/xlsx/workbook.rs`, `src/structs/workbook.rs` (`new_sheet`, `remove_sheet`),
  `src/structs/worksheet.rs` (`set_name`), `src/structs/defined_name.rs`,
  `src/structs/properties.rs`.
- Apache POI @ `47dcff769bb395034dddfd59c4609659b6dbc6a4`:
  - `poi-ooxml/src/main/java/org/apache/poi/xssf/usermodel/XSSFWorkbook.java` (`createSheet`,
    `removeSheetAt`, `onSheetDelete`, `removeUnusedPivotCaches`, `setSheetName`,
    `setSheetOrder`);
  - `…/xssf/usermodel/helpers/XSSFFormulaUtils.java` (`updateSheetName`);
  - `poi/src/main/java/org/apache/poi/ss/util/WorkbookUtil.java` (`validateSheetName`);
  - test data `test-data/spreadsheet` (Apache-2.0).
- rust_xlsxwriter 0.102.0 (`jmcnamara/rust_xlsxwriter@d850973d`): `src/workbook.rs`
  (`add_vba_project`, `set_vba_name`, default code names), `src/packager.rs` (content types,
  `vbaProject` relationship, signature `.rels`).
- Microsoft Support, "Rename a worksheet" (no leading or trailing apostrophe; `History`
  reserved): <https://support.microsoft.com/office/3f1f7148-ee83-404d-8ef0-9ff99fbad1f9>.
  Microsoft KB 159756, "XL97: Cannot Rename Worksheet to History":
  <https://techshelps.github.io/MSDN/KB/Source/excel/q159756.htm>.
- Corpus measurements: `spikes/xlsx-phase3/sheet_ops_survey.py`.
