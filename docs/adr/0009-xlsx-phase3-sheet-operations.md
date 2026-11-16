# ADR-0009: xlsx Phase3 — sheet operations on existing files

- Status: **Proposed** (revision 3)
- Date: 2026-11-16
- Depends on:
  - [ADR-0001](0001-lossless-round-trip-architecture.md): Layer 3 append-only invariant, and the
    draft post-acceptance note "registry writes" in PR #38, which generalises PR #14's interim
    note;
  - **PR #38** (`ooxml-core` registry transaction, `RegistryPlan`): the single source of truth
    for adding and removing parts, relationships and content types;
  - [ADR-0002](0002-schema-driven-code-generation.md): lens, allowlist, §4.3 insertion;
  - [ADR-0003](0003-xlsx-phase1-formula-and-calc-state.md) **revision 4** (on
    `impl/xlsx-phase1`, PR #20; §2.4 folding and unknown sheets exist only there): census,
    tokenizer, `Everything`, plan → validate → apply, §5 guards, calcChain policy;
  - [ADR-0006](0006-xlsx-phase2-shared-strings-and-styles.md): registries (PR #22).
- Roadmap: [`docs/roadmap/xlsx.md`](../roadmap/xlsx.md) §2.2 (A4–A7), §6 Phase3
- Evidence: [`spikes/xlsx-phase3/`](../../spikes/xlsx-phase3/README.md)
- Review: [round 1](0009-opus-review-round1.md), [round 2](0009-opus-review-round2.md)

## Revision history

- **Revision 1**: initial draft.
- **Revision 2** (after `0009-opus-review-round1.md`: 3 Blocker, 8 Major, 12 Minor):
  - **B1, B2:** package-structure edits use PR #38's transaction. The `workbook.xml` edit
    (`stage_part`), the closed-set removal of a sheet with everything it owns (`remove_parts`,
    each part with its own `.rels`) and any additions are validated together against the
    virtual view, then applied infallibly (§2, §5).
  - **B3:** rename uses a table of every known location with its syntax (§6). A
    **closed-world scan** refuses the rename when the old name appears, in sheet-prefix form,
    anywhere outside those locations. The table adds `connections.xml` parameters, table
    column formulas, `c:pivotSource/c:name`, 2007 `cfvo/@val`, `c15:` and chartEx formulas,
    `textlink`, form controls (ctrlProps, VML), `dataRef`/`rangeSet/@sheet` and the
    `app.xml` Named Ranges titles. Binary ActiveX parts refuse the rename.
  - **M1:** a deleted sheet's references become a bare `#REF!` (the whole reference token), as
    in every corpus file.
  - **M2:** the syntax rules per location are stated: formula vs raw name vs `[file]Sheet!Name`,
    3-D spans quoted as a whole, external `[n]` references never rewritten, ST_Xstring
    decoding and encoding, and a conservative quoting rule.
  - **M3:** every structural operation seeds Phase1 with all `Everything` formulas. A
    formula that names an unknown sheet, which a new or renamed sheet would resolve, is
    invalidated. A move that reverses a 3-D reference's endpoints is refused.
  - **M4:** deleting a sheet whose tables are used elsewhere is refused.
  - **M5:** state keyed by `sheetId` (`customWorkbookView/@activeSheetId`) is handled.
    Shared workbooks (`revisions`) are refused. The limit of the cross-session `sheetId`
    high-water is stated.
  - **M6:** a left-over `.rels` makes a part name taken (PR #38).
  - **M7:** there is an independent residual-mention oracle and a synthetic fixture with one
    reference per location.
  - **M8:** VBA signature parts are found by relationship type (legacy, Agile, V3), `.xlam`
    and templates are covered, and `ExtensionMismatch` only fires after a content-type change
    in the session.
  - **m1–m12** are folded in where they apply.
  - The reviewer's recommendations for Q1–Q6 are recorded as proposed defaults pending the
    owner's confirmation.
- **Revision 3** (after `0009-opus-review-round2.md`: 2 Major, 7 Minor):
  - **R2-M1:** `#Sheet!A1` targets in sheet and drawing `.rels` are rewritten with PR #38's
    new `set_relationship_target` (§2, §6). Option 1 of the review; Q7 records it.
  - **R2-M2:** §2 lists **everything** one operation changes. Phase1's cache stripping and
    `calcPr`, Phase2's `sst/@count`, and the formula rewrites go into the same staged clones
    (one clone per part), inside the one transaction. The in-memory state (census, sheet
    list, registries) is rebuilt after apply, under ADR-0003 §3.1's poisoning. ADR-0003 §5's
    guards apply to Phase3 (§12, §13, Consequences).
  - **R2-m1:** replacing a VBA project removes the old closed set and adds
    `xl/vbaProject.bin` with `Exact` in the same plan (PR #38 now allows an `Exact` name
    removed earlier in the plan). The content-type wording follows PR #38's rule.
  - **R2-m2:** parts in the closed set are neither rewritten nor scanned; the orphan check
    ignores fragment-only targets (PR #38 no longer reports them as parts).
  - **R2-m3:** the scan's blind spots are stated: XML parts that do not parse get a byte scan
    and refuse on a hit; non-ActiveX binaries are not scanned; raw-name locations rely on the
    table alone.
  - **R2-m4:** the scan folds like ADR-0003 rev 4 §2.4 (Unicode upper then lower).
  - **R2-m5:** "sheet-prefix token" includes `Sheet2!Name`, `Sheet2!#REF!` and
    `Sheet2!Func(`; their handling on rename and delete is stated.
  - **R2-m6:** a move is refused only when it **changes** the relative order of a 3-D
    reference's endpoints.
  - **R2-m7:** the `allowed_rel_types` passed to `remove_parts` per operation are listed.

## Context

Phase3 is the first xlsx phase that **adds and removes parts** in an existing package:

- add, delete, rename, reorder and hide worksheets;
- create, edit and delete defined names, workbook-wide or sheet-local;
- insert, replace or remove a VBA project (`.xlsm`);
- keep `docProps/app.xml` (`HeadingPairs`/`TitlesOfParts`) consistent.

ADR-0001 kept all of this out of Phase0, because it touches `[Content_Types].xml`, `.rels`,
`workbook.xml` and `app.xml` together. PR #14's `remove_part` removes one part that has no
relationships of its own, and only `calcChain.xml` is meant to use it. PR #38 adds the
transaction this ADR builds on: `RegistryPlan` stages part and relationship additions and
removals, content-type changes and new content for existing parts, validates the whole batch,
and applies it infallibly.

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

   These counts are a **lower bound** (review m12). The survey does not scan chartEx, `c15:`
   chart extensions, ctrlProps, VML, table parts or `connections.xml`, and the review's
   independent probe found real references in the last three (fact 10).
8. **Delete impact.** The relationship types in worksheets' own `.rels`, by number of packages:
   - `printerSettings` 110, `drawing` 61, `vmlDrawing` 37;
   - `table` 29, `comments` 27, `hyperlink` 14, `tableSingleCells` 14;
   - `pivotTable` 7, `oleObject` 5, `customProperty` 5, `image` 4, `threadedComment` 3,
     `control` 2, `package` 2.
9. **VBA.** A VBA project in 9 packages, none signed. A workbook `codeName` in 35, sheet
   `codeName`s in 32. `calcChain.xml` in 117, external links in 10.
10. **Locations outside rev 1's table** (review B3, independent probe over the POI corpus):
    - `connections.xml` `parameter/@cell` (`dataValidationTableRange.xlsx`,
      `cell="'County Ranking'!$B$5"` ×12, written by Excel);
    - table `calculatedColumnFormula` (`StructuredRefs-lots-with-lookups.xlsx`);
    - chart `c:pivotSource/c:name` (`WithChartSheet.xlsx`, `[CVT23.tmp]Sheet2!PivotTable2`).

    The schema allows more that the corpus does not show: 2007 `cfvo/@val`, `c15:f`/`c15:*`
    ranges, chartEx `cx:f`, `xdr:sp/@textlink`, form controls (ctrlProps `@fmlaLink` and
    relatives, VML `x:Fmla*`, `controlPr/@linkedCell`/`@listFillRange`),
    `dataRef/@sheet`, `rangeSet/@sheet`, and ActiveX `LinkedCell` inside `activeX*.bin`
    (binary).
11. **`#REF!` forms** (review M1): the corpus has 328 cell formulas and 212 defined names with
    `#REF!`. In every one, the whole reference token became a bare `#REF!`. `#REF!A1` occurs
    0 times, and Phase1's lexer would not accept it.
12. **State keyed by `sheetId`** (review M5): `customWorkbookView/@activeSheetId`
    (`61495-test.xlsm`). Shared-workbook revision logs (`xl/revisions/`, `header/sheetIdMap`)
    occur in 6 corpus places.

## Decision

### 1. Scope

In scope:

- `Workbook::add_worksheet(name, position)` on an opened file.
- `delete_sheet`, `rename_sheet`, `move_sheet`, and `set_sheet_state` (visible, hidden,
  veryHidden), for worksheets. Chart sheets, macro sheets and dialog sheets are covered by
  rename, reorder and hide. Adding them is out of scope. Deleting a chart sheet follows §5.
  Deleting a macro sheet or dialog sheet is refused, because XLM `Auto_Open` and names with
  `function`/`xlm` attributes point into it (review m6).
- Defined names: `add_name`, `set_name_formula`, `rename_name`, `delete_name`, workbook-wide or
  sheet-local. `_xlnm.Print_Area`/`_xlnm.Print_Titles` are set through dedicated setters.
- VBA: `set_vba_project(bin)`, `remove_vba_project()`.
- `docProps/app.xml` maintenance (§11).
- The `ooxml-core` Layer 3 write primitives these need (§2).

Out of scope:

- Copying or moving a sheet between workbooks, and copying within a workbook: roadmap G2,
  Phase7.
- Inserting or deleting rows and columns: G1, Phase7.
- Creating chart sheets: Phase5.
- Editing VBA source, signing.
- External-link maintenance: other workbooks that refer to this one are not ours to change.
- **Shared workbooks** (a `revisions` relationship): every Phase3 operation refuses with
  `Err(SharedWorkbook)`. The revision log keeps a `sheetIdMap` that Phase3 cannot maintain
  (review M5).

### 2. Core: one transaction per operation (PR #38)

Each Phase3 operation builds **one** `RegistryPlan` (PR #38) and applies it once. **Everything
the operation changes in the package is in that plan** (review R2-M2):

- **Staged parts** (`stage_part`), **one clone per part**, each carrying every change that part
  gets from the operation, because staging a part again replaces the earlier content:
  - `workbook.xml`: the `<sheets>` edit, `definedNames` (`localSheetId`, rewritten texts),
    `bookViews`, **and** Phase1's `calcPr/@fullCalcOnLoad`;
  - every sheet part whose formula texts are rewritten **and/or** whose dependent cells lose
    their cached values (ADR-0003 §3.1's strip): rewrite and strip go into the same clone.
    Strip positions are computed on the parsed tree before rewriting; rewriting changes only
    text of `f` elements, never the children of `sheetData`, so the positions hold in the
    clone (invariant, tested);
  - chart, drawing, pivot-cache, table, `connections.xml`, ctrlProps and VML parts with
    rewritten references;
  - `sharedStrings.xml` when `sst/@count` changes (delete; ADR-0006 §4.5), if the Phase2
    registry is active;
  - `docProps/app.xml` (§11).
- **Registry steps:** `add_part` (`Numbered`), `add_relationship`, `remove_parts` (closed set),
  `remove_relationship`, `set_content_type`, and **`set_relationship_target`** for `#Sheet!A1`
  hyperlink targets (review R2-M1; §6).
- **Then, in memory only**, after `Package::apply`: the census is **rebuilt** from the output
  parts (the same scan as at open; ADR-0003 §1), and the sheet list, contexts, dependency
  graph and Phase2 registries are reloaded. A structural operation is rare, so a rebuild at
  open cost is acceptable, and it cannot drift from a hand-maintained re-index (`c.sheet`,
  `localSheetId`, 3-D span membership, unknown-sheet resolution all change).

No Layer 2 edit happens outside the plan: `xml_part_mut` between planning and apply would make
the plan stale (`StalePlan`), so Phase3 never calls it. Each registry step is validated against
the **plan's view** (the package as the earlier steps leave it). Reference scans read the
staged content: the sheet part's relationship is removable because the staged `workbook.xml` no
longer names its `r:id`. If any step refuses, the plan is dropped and nothing changes.

**Failure after apply.** `Package::apply` cannot fail except `StalePlan`, which cannot happen
because nothing touches the package between planning and apply. The in-memory rebuild reads
parts this operation just wrote. If it nevertheless fails (a bug), the workbook is **poisoned**
exactly as ADR-0003 §3.1 does for a failed apply: every later edit and every save is refused,
so a package and an index that disagree can never be written. That is ADR-0003 §3.1's plan →
validate → apply for multi-part edits (review B2, R2-M2).

**`allowed_rel_types` per operation** (review R2-m7). These are the types of relationships
*into* the closed set from outside it; anything else from outside refuses:

| Operation | `allowed_rel_types` |
|---|---|
| delete a worksheet | `…/relationships/worksheet` |
| delete a chart sheet | `…/relationships/chartsheet` |
| calcChain removal (delete) | `…/relationships/calcChain` (Strict twin separate, as in Phase1) |
| replace or remove a VBA project | `…/office/2006/relationships/vbaProject` (the signature types are inside the set) |

The registry rules are those of PR #38's single rules table, and are not restated here:

- part names `max + 1` per template;
- `rId` `max + 1` per source, with a session high-water and no adoption of dangling `r:id`s;
- `Override` unless an exact `Default`;
- left-over `.rels` and `Override`s make a name taken;
- the closed-set removal conditions, failing closed on unreadable `.rels`.

The ADR-0001 wording is PR #38's draft note "registry writes" (Q1).

**Staging cost** (review m9). A rename or delete may touch every sheet that refers to the
sheet. The plan first computes the positions and replacement texts from the census (no clone).
It then clones only the parts that change and stages them. PR #38 shares staged trees between
plan steps (`Arc`), so later steps do not copy them again. ADR-0003 measured a 200k-formula
sheet clone at 289 ms. A Phase3 rename of a sheet referenced from N large sheets costs N such
clones, which is acceptable for a structural operation. An in-place apply path is a later
optimisation.

### 3. Ids and names for new things

- **`sheetId`:** 1 + the maximum of every `sheetId` in `workbook.xml`, every
  `customWorkbookView/@activeSheetId`, every `calcChain c/@i`, and every id deleted in this
  session. Ids are never reused within what the file and the session show:
  - `calcChain c/@i` and some add-ins key on them;
  - this matches POI and Excel's evident behaviour (the 56 gap files);
  - openpyxl and umya renumber instead.

  **Limit** (review M5): a sheet deleted in an *earlier* session, by any tool, leaves no trace.
  Its id can come back as max + 1 if it was the largest. Nothing in the file can prevent this.
  The id-keyed places this crate knows are scanned above.
- **Part name:** PR #38's `Numbered("{dir}/sheet{}.xml")`, where `{dir}` is the folder of the
  existing worksheets if they share one, otherwise `xl/worksheets`. That is max + 1 over every
  name in use. The part number need not equal the `sheetId` (it does not in 23% of the corpus).
- **Relationship id:** PR #38.
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
5. **Phase1** (§12): the seeds are:
   - formulas with 3-D references whose span now includes the new sheet;
   - formulas calling `SHEETS()` or `SHEET(…)`;
   - **every `Everything` formula** (review M3);
   - every formula that names a sheet that did not exist and now does. ADR-0003 §2.4 gives
     an unknown sheet `Everything`, and the new sheet makes it resolve.

   Seeded dependents lose their cache and `fullCalcOnLoad` is set. `calcChain.xml` is
   untouched: no formula cell is created.
6. **VBA workbooks:**
   - The new sheet gets no `codeName`. Excel is expected to create the document module on the
     next open (inference; XL item).
   - `set_sheet_code_name` exists for callers who ship a matching project.

### 5. Delete a sheet

Refused when:

- it is the last sheet, or the last visible sheet (`Err(LastVisibleSheet)`);
- the workbook is shared (`Err(SharedWorkbook)`, §1);
- it is a macro sheet or dialog sheet (§1);
- any pivot cache sources it (`worksheetSource/@sheet`, or `@name` resolving to a name or table
  on it);
- **a table on it is used elsewhere** (review M4): its name (case-insensitive) appears in a
  structured reference in any formula location of §6, in a pivot `worksheetSource/@name`, or in
  `x15:tableSlicerCache/@tableId`. This gives `Err(TableInUse { table })`. Rewriting such
  references to `#REF!` waits until the XL check shows Excel's form;
- a `customWorkbookView/@activeSheetId` names it (`Err(CustomViewReferencesSheet)`). Moving
  the view to another sheet is an XL item first;
- it holds a pivot table, slicer, timeline, query table, external data connection, ActiveX or
  OLE object, SmartArt (`diagram*`), chartEx, an embedded workbook in a chart
  (`c:externalData` → `package`), or any relationship type outside the cascade table:
  `Err(SheetHasUnsupportedParts { rel_types })`. The corpus has pivot tables on sheets in 7
  packages, OLE objects in 5 and controls in 2.

**One transaction** (B1, B2). The plan:

1. computes **the closed set** first: the sheet, every part reachable from it through the
   cascade types below that no part outside the set reaches. It uses PR #38's
   `RegistryPlan::incoming_relationships` on the plan's view, which fails closed on an
   unreadable `.rels`; a part with an incoming relationship from outside the set (a shared
   image) stays; fragment-only targets (`#Sheet1!A1`) are not parts and are ignored
   (review R2-m2);
2. stages `workbook.xml` without the `<sheet>`, with renumbered `localSheetId`s, adjusted
   `bookViews` and `calcPr` (§2);
3. stages every part **outside the set** whose references to the sheet are rewritten or whose
   caches are stripped (below). Parts in the set are neither rewritten nor scanned: they go
   (review R2-m2);
4. removes the closed set with all its `.rels` (`remove_parts`, `allowed_rel_types` per §2),
   and `calcChain.xml`;
5. stages `sharedStrings.xml` (`sst/@count`) and `app.xml` (§11).

The staged `workbook.xml` no longer names the sheet's `r:id`. The set's internal references
(`r:embed`, `r:id`, `o:relid`) do not block. Every other refusal comes from the plan, and
nothing changes.

| Relationship type (from the sheet, or recursively) | In the closed set |
|---|---|
| `printerSettings`, `comments`, `vmlDrawing` (+ its `image`s), `threadedComment`, `table`, `tableSingleCells`, `customProperty`, `image` (background) | yes, unless also reached from outside |
| `drawing` → `image`, `chart` → (`chartStyle`, `chartColorStyle`, `chartUserShapes` → `drawing` → `image`) | yes, unless also reached from outside |
| `hyperlink` (external, no part) | goes with the `.rels` |
| everything else | refused (above) |

Workbook-level parts stay: `persons.xml` (threaded-comment authors), `customXml`,
`xmlMaps.xml`, and every `Default` (review m5).

**References to the deleted sheet are rewritten as Excel writes them** (review M1). Each
rewritten location is returned (`Vec<RewrittenLocation>`) so the caller can audit it.

- **A sheet-prefixed reference token**, range included, becomes a bare `#REF!`:
  - `Sheet2!A1:B2` → `#REF!`;
  - `SUM(Sheet2!A1)` → `SUM(#REF!)`;
  - a sheet-local name use `Sheet2!LocalName` and an error reference `Sheet2!#REF!` (Phase1
    lexer: `TokKind::Name { prefix }`, `Area::RefError`) → `#REF!` (review R2-m5);
  - a qualified function call `Sheet2!Func(` (a macro-sheet function; Phase1 lexes it as a
    qualified function) refuses the delete with `Err(UnrewritableReference)`: Excel's form
    is not verified (XL item 9(b)).

  This applies in every formula location of §6's table: cell formulas, names, `c:f`/`c15:f`,
  DV/CF, `cfvo/@val`, `xm:f`, table column formulas, connection parameters, form-control
  links. **Invariant:** every rewritten text lexes with Phase1's lexer (tested).
- **A 3-D endpoint** moves inward to the next *worksheet* in the span. Chart sheets in between
  are skipped (review m8). Deleting `Sheet1` in `Sheet1:Sheet3!A1` gives `Sheet2:Sheet3!A1`;
  a span that shrinks to one sheet is written `Sheet3!A1`; an empty span gives `#REF!`. These
  are XL items.
- **Raw-name locations** (`worksheetSource/@sheet` is refused above; `dataRef/@sheet`,
  `rangeSet/@sheet`, `c:pivotSource/c:name`) refuse the delete with
  `Err(UnrewritableReference)`.
- A `hyperlink/@location` or `#Sheet!` relationship target is left as is. It is a navigation
  target, not a formula, and Excel shows it as broken. It is the one listed exception in the
  residual oracle.
- **Closed world:** a mention of the deleted sheet in sheet-prefix form outside the known
  locations (the scan of §6) refuses the delete with `Err(UnrewritableReference)`.

Position-based state:

- names local to the deleted sheet are removed;
- later `localSheetId`s are decremented;
- `activeTab`/`firstSheet` are adjusted as POI does, and the next visible sheet becomes
  active if the deleted one was;
- `tabSelected` is set on the new active sheet and cleared on every other sheet, ending any
  group selection (review m11).

**Phase1:** the dependents are:
- every formula that referred to the sheet (directly, through a name, or 3-D);
- every `Everything` formula;
- `SHEET()`/`SHEETS()` callers.

`calcChain.xml` is removed (in the same plan), because the set of formula cells changed. That
is ADR-0003's calcChain policy, and POI does the same.

**Phase2:** `sst/@count` is recomputed without the deleted sheet's references (ADR-0006 §4.5),
in the staged `sharedStrings.xml` (§2). No style or string entry is removed (Layer 3).

### 6. Rename a sheet

Validation as in §3. The new name must not fold-equal another sheet or a defined name that
formulas could confuse with it (ADR-0003 §2.4 c). Refused for shared workbooks, and for
workbooks with ActiveX binaries (`activeX*.bin`), whose `LinkedCell`/`ListFillRange` this
crate can neither read nor rewrite (`Err(BinaryReferencesPossible)`; review B3-3).

**Known locations and their syntax** (review B3, M2). Every location is rewritten by the
function for its syntax. The corpus count is the number of packages (a lower bound, fact 7).

| Location | Syntax | Corpus |
|---|---|---|
| cell formulas on every sheet part (normal, shared masters, array, data-table anchors) | formula | 17 (3-D: 3) |
| `definedName` text (all scopes) | formula | 46 |
| chart `c:f`, `c15:f`, `c15:sqref`, `c15:fullRef`/`formulaRef`, `c15:datalabelsRange`, `c15:filteredSeriesTitle`/`…CategoryTitle` in every chart part | formula | 25 |
| chartEx `cx:f` | formula | 0 |
| `c:pivotSource/c:name` | `[file]Sheet!PivotName` (sheet part raw, quoting not verified: refused if the new name needs quotes) | 1 |
| pivot cache `worksheetSource/@sheet`; `consolidation/rangeSets/rangeSet/@sheet`; `dataConsolidate/dataRefs/dataRef/@sheet` | raw name | 5 / 0 / 0 |
| DV `formula1`/`formula2`, CF `cfRule/formula`, 2007 `cfvo/@val` (`type="formula"`) | formula | (in 3 with x14) |
| x14 `xm:f` (DV, CF incl. `x14:cfvo`, sparklines) | formula | 3 |
| table `calculatedColumnFormula`, `totalsRowFormula` | formula | 1 |
| `connections.xml` `parameter/@cell` | formula (reference) | 1 |
| form controls: ctrlProps `@fmlaLink`/`@fmlaRange`/`@fmlaTxbx`/`@fmlaGroup`; VML `x:FmlaLink`/`x:FmlaRange`/`x:FmlaTxbx`/`x:FmlaPict`; `controlPr/@linkedCell`/`@listFillRange` | formula (reference) | 0 |
| `xdr:sp/@textlink`, `cdr:sp/@textlink` | formula (reference) | 0 non-empty |
| `hyperlink/@location` | location (`Sheet!A1` / `'S h'!A1`) | 3 |
| `#Sheet!…` targets of Internal relationships in sheet and drawing `.rels` (shape hyperlinks) | location, written through PR #38 `set_relationship_target` (`Id`, `Type`, `TargetMode` stay; review R2-M1) | 3 (18 renames of 838 in the POI corpus) |
| `app.xml` worksheet titles | raw name (`vt:lpstr`, ST_Xstring) | 294 with titles |
| `app.xml` "Named Ranges" titles (`'1'!Print_Area`) | location | 32 |
| `docProps/custom.xml` `linkTarget` | defined-name name (unchanged by a sheet rename) | 9 |

**Syntax rules** (review M2):

- **Formula:**
  - Only sheet-prefix tokens change. The token spans come from Phase1's lexer, so every other
    byte stays. A sheet-prefix token is any token with a sheet prefix: a cell or area
    reference, a 3-D span, a sheet-local name use (`Sheet2!Name`), an error reference
    (`Sheet2!#REF!`) and a qualified function call (`Sheet2!Func(`). Rename rewrites the
    prefix of each (review R2-m5).
  - A 3-D span is quoted **as a whole** (`'Jan:Q1 2024'!A1`), never per endpoint.
  - External references (`[1]Sheet1!A1`, `'[1]Sheet 1'!A1`) are never rewritten, even when
    the name matches: they are another workbook's sheets.
- **Quoting:**
  - The name is left unquoted only if it matches `^[A-Za-z_][A-Za-z0-9_.]*$` and is not an A1
    or R1C1 reference, `R`/`C` alone (any case), `TRUE` or `FALSE`. Otherwise it is quoted,
    with `'` doubled.
  - This is conservative: quoting is always valid, and Excel normalises it on save.
- **Raw name:** written as is, never quoted (`'Q1 2024'` would be a different name).
- **ST_Xstring** (`sheet/@name`, `<f>`, `vt:lpstr`): matching happens after `_xHHHH_`
  decoding. Writing escapes control characters and protects a literal `_xHHHH_` with
  `_x005F_` (the corpus has a sheet name with `_x000a_`).

**Closed-world scan** (review B3). Before planning, every XML part of the package that is not
in the operation's closed set is scanned, whatever its content type. The scan covers every
text node and every attribute value, after XML unescaping and ST_Xstring decoding, folded as
ADR-0003 rev 4 §2.4 folds sheet names (Unicode upper case, then lower case; a broader fold
only refuses more; review R2-m4).
- It looks for the old name in sheet-prefix form: `'…'!`, `Name!`, or a 3-D `A:B!` with the
  old name as an endpoint.
- Every hit must lie inside a known location, or inside a known non-reference: a cell value
  of `t="s"`/`t="str"`/`inlineStr`, `sharedStrings.xml`, a string literal inside a formula,
  an external `[n]` reference, or a comment text.
- Any other hit refuses the rename with `Err(UnrewritableReference { part, location })`. A
  silent miss is worse than a refusal.
- **Blind spots** (review R2-m3), stated in the API docs:
  - An XML-typed part that does not parse (Excel's legacy VML can leave `<br>` unclosed, e.g.
    POI `BrNotClosed.xlsx`) gets a byte scan instead (UTF-8 and UTF-16LE, decoded with the
    same fold); a hit refuses, because such a part cannot be rewritten either.
  - Binary parts other than ActiveX (`oleObject*.bin`, `printerSettings*.bin`, images) are not
    scanned. ActiveX refuses the rename outright (above).
  - The scan looks for the sheet-prefix form only. **Raw-name** locations (no `!`:
    `worksheetSource/@sheet`, `rangeSet/@sheet`, `dataRef/@sheet`, `app.xml` titles) are
    found through the table alone; an unknown raw-name location would be missed. The POI
    corpus has none (review round 2 measurement: the raw-name hits were the table's own
    locations, external-link sheet names and numeric coincidences).
- Measured on the POI corpus (review round 2): 0 refusals in 838 renames over 343 packages.

**Not rewritten**, and documented (Excel does not rewrite these either, or they are not ours):
- sheet names inside formula string literals (`INDIRECT("Sheet1!A1")`,
  `HYPERLINK("#Sheet1!A1")`; 0 in the corpus);
- VBA source. It refers to sheets by `codeName` (unaffected) **or by name**
  (`Worksheets("Sheet1")`), and the by-name references break (review m7);
- other workbooks' external links;
- `connections.xml` `dbPr/@command` text (`SELECT … FROM [Sheet1$]`), the Power Query mashup
  and the Power Pivot model (review m10).

**Phase1:** the seeds are:
- callers of `CELL` (any argument) and `INFO`;
- **every `Everything` formula**. That includes `INDIRECT("Old!A1")`, `SHEET("Old")` and
  `GET.WORKBOOK` in names (review M3);
- every formula naming an unknown sheet whose name equals the new name, which would resolve
  now.

`calcChain.xml` is unaffected: it keys on `sheetId`.

### 7. Reorder (`move_sheet(from, to)`)

- `<sheet>` elements are moved in `workbook.xml`: a Layer 2 move, i.e. remove and insert the
  same raw node, so its bytes are unchanged.
- `localSheetId`s, `activeTab` and `firstSheet` are remapped by the permutation.
- `app.xml` order follows.
- **Phase1:** the seeds are formulas with 3-D references whose span changes membership,
  `SHEET()`/`SHEETS()` callers, and every `Everything` formula (review M3).
- **3-D endpoint reversal** (review M3). Microsoft documents that "if you reverse the end
  points, the 3-D reference changes the endpoint worksheet". Excel's exact rewrite is not
  verified, so a move that **changes the relative order** of a 3-D reference's two endpoints is
  refused with `Err(ReversesThreeDReference { location })`. A span that is already reversed in
  the input (ADR-0003 §2.4 treats `Sheet3:Sheet1` like `Sheet1:Sheet3`) does not by itself
  refuse every move (review R2-m6). This is an XL item; adopting Excel's rewrite comes once it
  is known.

### 8. Hide and unhide

- `set_sheet_state(sheet, Visible | Hidden | VeryHidden)` sets or removes `sheet/@state`.
- Hiding the last visible sheet is `Err(LastVisibleSheet)`.
- Hiding the active sheet moves `activeTab` to the next visible sheet and sets `tabSelected`
  there. Hiding any sheet clears its `tabSelected`, so a group selection never includes a
  hidden sheet (review m11).
- `app.xml`: `veryHidden` sheets are not listed (corpus fact 5), so going to or from
  `veryHidden` removes or adds the title.

### 9. Defined names

- **Scope:** workbook, or sheet-local (`localSheetId` = the sheet's *current position*,
  recomputed by §4/§5/§7).
- **Name rules:**
  - 1–255 characters;
  - starts with a letter, `_` or `\`;
  - then letters, digits, `.`, `_`, `\`, `?`;
  - not a cell reference in A1 or R1C1 form (`R1`, `C2`, `RC`, `R[1]C`, any case), and not
    `R`/`C` alone in any case (review m3);
  - no reserved prefix: `_xlnm.`, `_xlfn.`, `_xlpm.`, `_xlws.`, `_xludf.`;
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
  - Deleting a name that is still used is `Err(NameInUse { count })`, unless
    `delete_name_force`. Then dependents are invalidated and the formulas keep the name text,
    which Excel evaluates to `#NAME?`.
  - Uses are counted in every formula location of §6, in other names, and in
    `docProps/custom.xml` `linkTarget` (9 corpus packages; review m4).
  - A use the scan cannot classify (a binary part, an unknown part) refuses unless forced.
- **`rename_name`** rewrites the name in all of those locations, `linkTarget` included.
- **`app.xml`:** the "Named Ranges" block is updated if it exists (§11). Excel evidently lists
  printable names (`Print_Area`, `Print_Titles`) and visible user names, but not hidden names
  or `_FilterDatabase` (inferred, review m1; XL item 9(g)).

### 10. VBA project

- **`set_vba_project(bin)`** (one transaction; review M8):
  - adds `xl/vbaProject.bin` of type `application/vnd.ms-office.vbaProject`, registered by PR
    #38's rule: nothing when a `bin` Default is exactly that type, else an `Override`. A `bin`
    Default is usually the printer-settings type (110 corpus packages), so in practice it is
    an `Override`; the rust_xlsxwriter Default approach would retype the printer settings
    (review R2-m1);
  - adds the workbook relationship (`…/office/2006/relationships/vbaProject`);
  - sets the workbook content type:
    - `.xlsx` → `…sheet.macroEnabled.main+xml`;
    - `.xltx` → `…template.macroEnabled.main+xml`;
    - `.xlsm`/`.xltm`/`.xlam` keep theirs;
  - sets `workbookPr/@codeName` (default `ThisWorkbook`) and each sheet's `codeName` (default
    `Sheet{n}`, skipping names in use) **only where absent**, as rust_xlsxwriter does for new
    files.
- **Signatures** are not produced. Signing is out of scope, so the API takes no signature.
- **Replacing an existing project** removes the old one and adds the new one in the same
  plan:
  1. `remove_parts` of the closed set: `vbaProject.bin`, its `.rels`, and every part its
     `.rels` reaches through the signature relationship types `…/vbaProjectSignature`,
     `…/vbaProjectSignatureAgile` and `…/vbaProjectSignatureV3` (found by type, not by file
     name), with `allowed_rel_types` = `vbaProject` (§2);
  2. `add_part(Exact("xl/vbaProject.bin"))` (the old name: PR #38 lets `Exact` reuse a name
     removed earlier in the same plan, and registers a fresh content type) and
     `add_relationship` from the workbook.

  A changed project invalidates every signature. Existing `codeName`s are kept. (Revision 2's
  wording failed against PR #38 at the time with `NameInUse`; review R2-m1.)
- **`remove_vba_project()`:** the same closed-set removal, plus the non-macro content type
  (`.xlsm` → `.xlsx`, `.xltm` → `.xltx`).
  - It is refused for `.xlam`: an add-in without macros is not a workbook type
    (`Err(AddinNeedsVba)`).
  - Macro bindings that dangle afterwards are not removed: VML `x:FmlaMacro` (57 corpus
    places) and shape `@macro`. The docs say so (review m7).
- The binary is not inspected (CFB). Matching module names to `codeName`s is the caller's
  responsibility, and the docs say so.
- **`ExtensionMismatch`** (review M8) applies only to `save(path)`, and only when **this
  session changed** the workbook content type and the path's extension does not match the new
  one. An unchanged round trip of an already mismatched file still saves, as in Phase0, and
  `to_bytes` never checks.

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
  operation would need a block that does not exist (e.g. the first chart sheet), **all of**
  `app.xml` is left untouched for that operation, so a partial update never leaves the counts
  inconsistent.
  - Rename rewrites sheet-prefixed titles in "Named Ranges" (`'Old'!Print_Area`).
  - Whether Excel omits `veryHidden` sheets rests on one corpus package, so it is XL item 9(g)
    (review m1).
- **Unrecognized, stale or absent** `app.xml` (48 of 342 packages lack the elements or the
  part; 4 are stale) is left untouched (L1).
- This is a Layer 2 edit of `app.xml` with an allowlist entry for `ep:HeadingPairs`,
  `ep:TitlesOfParts`, `vt:vector`, `vt:variant`, `vt:lpstr`, `vt:i4`.

### 12. Interaction with Phase1 and Phase2 (summary)

"`Everything`" means every formula ADR-0003 cannot give a precise precedent set. Its closure is
cached, so seeding it is cheap (review M3).

| Operation | Phase1 seeds (dependents lose `v`, `fullCalcOnLoad`) | `calcChain.xml` | Phase2 |
|---|---|---|---|
| add sheet | 3-D refs whose span gains it; `SHEET()`/`SHEETS()`; `Everything`; formulas naming an unknown sheet with the new name | untouched | new cells use registries |
| delete sheet | everything that referred to it (direct, name, 3-D); `Everything`; `SHEET()`/`SHEETS()` | removed (formula set changed) | `sst/@count` recomputed |
| rename sheet | `CELL`, `INFO`; `Everything`; formulas naming an unknown sheet with the new name | untouched | — |
| reorder | 3-D refs whose membership changes; `SHEET()`/`SHEETS()`; `Everything` | untouched | — |
| hide/unhide | none | untouched | — |
| name add/edit/delete | formulas through the name; `Everything` | untouched | — |
| VBA | none | untouched | — |

All operations run as one PR #38 transaction (§2): the strips, `calcPr` and `sst/@count`
are in staged clones, not applied separately. An `Err` leaves the package unchanged.

**ADR-0003 §5 guards apply** to Phase3, because the seeds include every `Everything` formula
(review R2-M2):
- `IterativeCalculation`: a structural operation with a non-empty dependent set in a
  workbook with `calcPr/@iterate` is refused (POI corpus: 2 packages, `47889`, `57535`);
- `MacroSheetDependent`: a dependent on a macro or dialog sheet refuses (2 packages);
- `RichValueCell`: a dependent `vm` cell refuses (0 packages);
- the workbook-wide blocks of ADR-0003 §1 (unknown cell metadata, a failed census) refuse
  every Phase3 operation, as they refuse every edit.

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
- `SharedWorkbook`
- `TableInUse { table }`
- `CustomViewReferencesSheet`
- `ReversesThreeDReference { location }`
- `BinaryReferencesPossible`
- `AddinNeedsVba`
- `IterativeCalculation`, `MacroSheetDependent`, `RichValueCell` and the workbook-wide
  blocks (ADR-0003 §1, §5; §12)
- `Poisoned` (a failed in-memory rebuild after apply; ADR-0003 §3.1)
- core `RegistryRefused` (PR #38)

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
   - every `localSheetId` < sheet count;
   - a new part has only the relationships the operation added (review M6).
1a. **Index oracle** (review R2-M2): after every operation, the rebuilt census, sheet list and
    registries equal those of `Workbook::open` on the saved output.
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
   - Every rewritten formula text lexes with Phase1's lexer (review M1).
4a. **Residual-mention oracle** (review M7). This oracle is independent of the implementation's
    location table. A regex scan, written separately from the implementation (the review's
    probe style), runs over **every** output part:
    - XML parts: all text and attribute values, decoded;
    - binary parts: their UTF-8 and UTF-16LE bytes.

    It asserts:
    - after a rename, the old name appears in sheet-prefix form only in the allowed
      non-reference places;
    - the new name's count equals the old name's count in the input plus the new name's count
      in the input;
    - after a delete, the deleted name appears nowhere except `hyperlink/@location`/`#…`
      targets.

    A **synthetic fixture** holds one reference in each row of §6's table, including a shape
    hyperlink `.rels` target `#'Sheet 1'!A1`, a `Sheet2!LocalName`, a `Sheet2!#REF!` and a
    VML part that does not parse. The locations openpyxl cannot produce are written as hand
    XML.
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
   - `veryHidden` listed in `app.xml`;
   - a `#Sheet!` relationship target not retargeted on rename;
   - strip and rewrite staged as two clones of one part (the later wins);
   - census re-indexed instead of rebuilt, or not updated;
   - a closed-set part rewritten (refused by PR #38 as `ConflictsWithPlan`);
   - the scan folding ASCII only;
   - a reversed input 3-D span refusing every move.
9. **XL (owner):**
   - (a) rename/delete outputs on `51850`, `ExcelPivotTableSample` (refused delete),
     `TwoSheetsOneHidden`, `reordered_sheets`;
   - (b) `#REF!` forms after delete;
   - (c) a new sheet in an `.xlsm` with VBA (does Excel add the document module silently?);
   - (d) `set_vba_project` on an `.xlsx` with `printerSettings`;
   - (e) `ExtensionMismatch` behaviour;
   - (f) 3-D endpoint forms after delete (inward move, single sheet, empty) and after a
     reversing move;
   - (g) `app.xml` after hiding a sheet as `veryHidden`, and which names "Named Ranges" lists;
   - (h) a deleted sheet's table referenced elsewhere (Excel's `#REF!` form, before
     `TableInUse` is relaxed).

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
- **Per-call `check_*` twins (revision 1).** These are rejected (review B2): they cannot
  validate dependent steps, such as removing a sheet part only after `workbook.xml` stops
  naming it. PR #38's transaction replaces them.
- **Leaf-first `remove_part` cascades (revision 1).** These are rejected (review B1): every
  first step is refused (`HasOwnRelationships`, `StillReferenced`). A closed-set removal
  replaces them.
- **`#REF!A1` after delete (revision 1).** This is rejected (review M1): Excel writes a bare
  `#REF!`, and the other form does not lex.

## Consequences

- `ooxml-core` gains PR #38's registry transaction, and ADR-0001 gains its note "registry
  writes" (Q1).
- The Phase1 tokenizer becomes a rewriter. Its token spans must round-trip byte-exactly, so
  that only sheet-prefix tokens change.
- Census coverage grows. Phase1 scans worksheet `f`, names and calcChain. Phase3 also needs
  chart `c:f`, pivot cache sources, DV/CF formulas, `xm:f` and hyperlink locations. These
  are cheap scans (`spikes/xlsx-phase3/` does them in Python over the whole corpus in
  seconds).
- These are refused in Phase3: sheet deletes involving pivot tables, slicers, OLE objects,
  controls, SmartArt, chartEx or embedded chart workbooks; renames in workbooks with ActiveX
  binaries; everything in shared workbooks. That is at most 16 of 342 corpus packages, under
  5% (review m5). ADR-0003 §5's guards can refuse a structural operation in 4 more packages
  (iterative calculation, macro-sheet dependents; review R2-M2).
- Phase1's strip planning (`plan_strip`) must work on a tree that will be staged, not only in
  place: Phase3 takes its positions and applies them to the clone. The Phase1 apply path
  stays as it is for value edits.
- PR #38 gains `set_relationship_target` (R2-M1) and lets `Exact` reuse a name removed in the
  same plan (R2-m1).

## Open questions for the project owner

**Owner decision 2026-11-16:** Q2 — references to a deleted sheet are rewritten to a bare
`#REF!` as Excel does, with `delete_sheet_strict` available for callers who want a referenced
sheet's deletion refused instead. Q1 and Q3–Q7 — the proposed defaults below are accepted
(including Q7: `#Sheet!` hyperlink relationship targets are rewritten on rename).

Each question carries the reviewer's recommendation (round 1; Q7 from round 2) as the **proposed default**,
pending the owner's confirmation.

- **Q1 (ADR-0001).** *Proposed: yes, as PR #38's note "registry writes".*
  - Additions are append-only: a left-over `.rels` makes a name taken, and `rId`s keep a
    session high-water.
  - Removal is a closed-set primitive with transaction-wide validation, which replaces PR #14's
    interim note rather than widening it.
- **Q2 (delete rewrites).** *Proposed: rewrite as Excel does*, with conditions:
  - bare `#REF!` (M1);
  - refuse when the sheet's tables are used elsewhere (M4);
  - the residual-mention oracle is mandatory (M7);
  - the rewritten locations are returned to the caller.

  A strict mode, `delete_sheet_strict`, refuses any referenced sheet.
- **Q3 (new sheet in VBA workbooks).** *Proposed: no `codeName`*, confirmed by XL 9(c). If
  Excel repairs or loses VBA, the next revision assigns a free `Sheet{n}`.
- **Q4 (`app.xml` blocks).** *Proposed: never invent structure.* When a needed block is
  missing, the whole of `app.xml` stays untouched for that operation.
- **Q5 (names in use).** *Proposed: refuse by default*, counting every location of §6, other
  names, `linkTarget` and connection parameters. Unclassifiable mentions refuse unless forced.
- **Q6 (refusals until Phase5).** *Proposed: yes.* That covers pivots, slicers, OLE objects,
  controls, tables used elsewhere, shared workbooks, ActiveX for rename, and
  chartEx/SmartArt for delete. Rename is allowed on sheets with pivots, because
  `worksheetSource/@sheet` and `c:pivotSource/c:name` are rewritten.

- **Q7 (`#Sheet!` relationship targets on rename).** *Proposed: rewrite them* with PR #38's
  `set_relationship_target`, the review's option 1 (18 of 838 corpus renames). The
  alternatives were to leave them stale and report `UnrewrittenLocation`, or to refuse such
  renames (2.1% false refusals).

## Sources

- ADR-0001 (Layer 3; PR #14 interim note), ADR-0002 §4.3, ADR-0003 (§1 census, §2.4 folding and
  3-D endpoints, §3.1 plan → validate → apply, calcChain policy), ADR-0006 §4.5, §7.3.
- PR #14 (`Package::remove_part`, OPC part-name equivalence); **PR #38** (`RegistryPlan`:
  staged transaction, closed-set removal, rules table, ADR-0001 note "registry writes";
  `set_relationship_target`, `incoming_relationships` on the plan's view, `Exact` reuse of a
  name removed in the plan — revision after its round-1 review); PR #21
  (O3 LibreOffice recalculation workflow); Phase1 lexer
  (`impl/xlsx-phase1`, `crates/ooxml-xlsx/src/formula/lexer.rs`, error literal and
  `Sheet!#REF!`).
- Microsoft Support, "Create a reference to the same cell range on multiple worksheets" (3-D
  references: insert, delete, move, move or reverse an endpoint):
  <https://support.microsoft.com/office/create-a-reference-to-the-same-cell-range-on-multiple-worksheets-c906f8b4-c648-4aa0-8063-65d38d03370f>.
- Review round 1 probe: `scratchpad/review-pr27/probe/locs.py` (all text nodes and attributes
  of every XML part, sheet-prefix mentions). Review round 2 probes:
  `scratchpad/review-38-27r2/adr0009/` (`cw_scan2.py`, `raw_probe.py`, `adr0009_probe.rs`).
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
