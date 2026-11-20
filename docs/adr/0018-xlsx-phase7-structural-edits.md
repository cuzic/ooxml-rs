# ADR-0018: xlsx Phase7 — structural edits

- Status: **Proposed** (revision 1)
- Date: 2026-11-20
- Depends on:
  - [ADR-0001](0001-lossless-round-trip-architecture.md): Layer 1–3, T1/T1b/T2, `Err` ⇒
    byte-identical output;
  - [ADR-0002](0002-schema-driven-code-generation.md): typed lens, allowlist codegen, the
    `mc:AlternateContent` refusal;
  - [ADR-0003](0003-xlsx-phase1-formula-and-calc-state.md) (revision 5, Accepted, with the P5-pre
    addendum §3.1a): census, lexer (§2.1), translation (§4.2), the calcChain policy (§4.3), plan →
    validate → apply with poisoning (§3.1), fault injection (§7.3);
  - [ADR-0006](0006-xlsx-phase2-shared-strings-and-styles.md): `sst/@count` upkeep (§4.5);
  - [ADR-0009](0009-xlsx-phase3-sheet-operations.md) (revision 4, Accepted): one `RegistryPlan`
    per operation (§2), ids and names for new sheets (§3), add-sheet seeds (§4), the location
    table and the closed-world scan (§6), `RewrittenLocation`; it defers G1 and G2 to this
    phase (§1);
  - [ADR-0010](0010-core-performance.md) (Accepted): the serializer's rules for dirty nodes, the
    deferred-row plan (§2.5);
  - [ADR-0014](0014-xlsx-phase4-range-features.md) (Accepted, revision 3): range features and
    the per-call allowed-diff table as data (§2);
  - [ADR-0017](0017-xlsx-phase5-objects.md) (Accepted, revision 3): §2.1 (plan steps, in-place
    edits after apply), §2.2 (id allocators, session high-waters), §6 (tables, names,
    structured-reference sub-grammar, header sync); it defers table and column renames to this
    phase (§1, §6.3).
- Roadmap: [`docs/roadmap/xlsx.md`](../roadmap/xlsx.md) §2.8 (G1, G2), §4, §6 Phase7.
- Evidence: [`spikes/xlsx-phase7/`](../../spikes/xlsx-phase7/README.md) (corpus survey and a
  shared-formula simulation).
- Review: none yet.

## Revision history

- **Revision 1**: initial draft.

## Context

Phase0–5 edit values, formulas, registries, sheets, range features and objects. None of them
moves a cell. A structural edit does: inserting a row renumbers every cell below it, and every
reference to those cells, **in every part**, must follow. The roadmap's G1 row lists that set
as "all of them", and §4 ends on the same point: a structural edit invalidates every
reference-bearing element in every part.

Three operations are designed here:

- **G1**: insert or delete whole rows or whole columns of a worksheet;
- **G2**: copy a worksheet within the workbook (moving one already exists: ADR-0009 §7);
- **renames** that ADR-0017 deferred: a table, and a table column, with every structured
  reference.

What makes them hard is not any single rewrite (each is a small rule) but **completeness**: a
place that holds a reference and is not rewritten silently points at the wrong cell. ADR-0009
met the same problem for sheet names with a location table plus a closed-world scan that
refuses unknown mentions. Its implementation (PR #53) still refuses DV, CF, x14, table and
connection formulas instead of rewriting them (ADR-0009 implementation note). Phase7 needs the
same table, extended to positions, and needs it to be **data that the code and the tests
share**, so that the list cannot drift.

### Where references live

```
workbook.xml ─ definedName text (names, Print_Area, Print_Titles, _FilterDatabase)
sheetN.xml ─┬ sheetData: row/@r, c/@r, f text, f/@ref, f/@r1 @r2, row/@spans
            ├ dimension, cols/col @min @max, sheetView: topLeftCell, pane (xSplit/ySplit/topLeftCell), selection (activeCell, sqref)
            ├ mergeCells, hyperlinks (@ref; @location on any sheet), autoFilter (+filterColumn @colId, sortState), sortState
            ├ conditionalFormatting @sqref + cfRule/formula + cfvo/@val; dataValidations @sqref + formula1/2
            ├ extLst: x14 CF/DV (xm:sqref, xm:f), sparklines (xm:f data, xm:sqref host)
            ├ rowBreaks/colBreaks brk/@id, ignoredErrors, protectedRanges, customSheetViews (views, breaks, filters)
            └ scenarios, dataConsolidate, cellWatches, webPublishItems, oleObjects/controls (anchors in mc:AlternateContent)
tables/tableN.xml ─ @ref, autoFilter, sortState, tableColumn set, calculatedColumnFormula, totalsRowFormula
commentsN.xml ─ comment/@ref        vmlDrawingN.vml ─ x:Row, x:Column, x:Anchor, x:Fmla*
threadedComments ─ @ref             drawings/drawingN.xml ─ xdr:from/xdr:to markers, sp/@textlink
charts/chartN.xml ─ c:f, c15:f      pivotCacheDefinitionN.xml ─ worksheetSource @ref @sheet @name
pivotTableN.xml ─ location/@ref     tableSingleCells ─ singleXmlCell/@r    connections.xml ─ parameter/@cell
calcChain.xml ─ c/@r @i             ctrlProps ─ @fmlaLink @fmlaRange …
```

The schema agrees that the set is large: `sml.xsd` types **56** attributes or elements as
`ST_Ref` (22), `ST_CellRef` (17), `ST_Formula` (9), `ST_Sqref` (7) or `ST_RefA` (1)
(`reference/ecma-376-transitional-xsd/sml.xsd`, counted with grep), besides the complex types
derived from `ST_Formula` (`f`, `definedName`) and the DrawingML, VML, x14 and chart places
that are plain strings in their schemas.

### What the corpus shows

`spikes/xlsx-phase7/structural_survey.py` over Apache POI `test-data/spreadsheet` at
`47dcff769bb395034dddfd59c4609659b6dbc6a4` (366 files) and the 2 calamine fixtures: **343
packages read**, 813 worksheets, 1,502,954 cells. Reproduction and full tables:
[spike README](../../spikes/xlsx-phase7/README.md). "Files" = packages with at least one.

| Topic | Finding |
|---|---|
| Size | Highest used row per sheet: median 3, p99 3,799, max 1,048,576 (1 sheet). 4 sheets have ≥ 10,000 rows. |
| Implicit positions | **Rows without `r`: 236 (3 files); cells without `r`: 5,561 (4 files)**; 15 sheets. `row/@spans` on 88,922 rows (275 files). |
| Formula texts by location | cells 11,561 (126 files); names 858 (62); chart `c:f` 174 (25); DV 122 (12); CF 89 (18); x14 `xm:f` 25 (6); `calculatedColumnFormula` 16 (8); `connections.xml` `parameter/@cell` 12 (1); `cfvo` formulas 2; `totalsRowFormula`, `c15:f`, `textlink`: 0. |
| Cell-formula reference forms (texts containing) | cell 7,702 (78); area 3,113 (45); **whole columns 1,644 (5)**; whole rows 5 (3); sheet-prefixed 6,147 (14); **3-D 16 (3)**; external 14 (5); **structured 4,022 (11)**, of which `#This Row`/`@` 3,897 (6) and column ranges 5 (2); names 52 (16); `#REF!` 335 (6), **sheet-prefixed `S!#REF!` 1**; spill `A1#` 0; R1C1-shaped 0; `INDIRECT` 3,801 (2), `OFFSET` 11 (1), `ROW` 5,490 (5). |
| Defined names | 858: sheet-local 142 (24 files), constants 240, `#REF!` 228 (12), **sheet-prefixed `S!#REF!` 13 (4)**, structured references 14 (6), `Print_Area` 16, `Print_Titles` 11, `_FilterDatabase` 8. **No name has an unprefixed or relative reference (0 of 858).** |
| Shared, array, data tables | `t="shared"` 19,558 cells, 359 masters (48 files), 58 2-D blocks; **array 53, of which 43 multi-cell (5 files)**; data tables 0 (1 synthetic); `cm` 1, `vm` 2. |
| Shared-group simulation | One row inserted (or deleted) in the middle of each of the 292 groups spanning ≥ 3 rows: **286 stay one group, 6 (4 files) must be split** (members that refer to a neighbouring row: `B5-B4`, `+A4+1`). |
| CF / DV | CF 108 blocks (20 files), 131 rules, 11 multi-range `sqref`s; **27 CF formulas with relative references** (8). DV 108 (13), **44 with relative references** (3). x14 CF in 2 files, x14 DV in 3, sparklines 11 (1). |
| Range features | merges 54,468 (43 files); hyperlinks 113 (15), `location` 38 (3); sheet autofilter 3, `sortState` 7; selections 481, frozen panes 32 (19); `cols` 361 sheets (179 files), 106 `col` up to XFD (44); row breaks 10 (4 files), column breaks 3 (3); `ignoredErrors` 5, `protectedRanges` 4, `customSheetViews` 3 (1), `dataConsolidate` 1, `scenarios`/`cellWatches`/`webPublishItems` 0. |
| Objects | notes 204 (27 files) with 204 VML `x:Row`/`x:Column`; VML `x:Anchor` 410 (37); VML form controls 166 and `x:Fmla*` 57 (4); threaded comments 4 (3); drawing anchors: `twoCellAnchor` 181 (`editAs` twoCell 102, oneCell 74, absolute 5), `oneCellAnchor` 30, **170 anchors inside `mc:AlternateContent` (4 files)**; tables 61 (29 files): 9 with totals, 13 `queryTable` + 5 `xml`; charts 40 (27 files), 168 sheet-prefixed `c:f`; pivot tables 12 on 12 sheets (7 files), pivot caches 10 (7 sourced by `ref`, 3 by table name); `singleXmlCells` 14 parts (14 files); `calcChain.xml` 116 packages (30,875 entries). |
| `mc:AlternateContent` in sheets | at the top level 2 sheets (both wrap `controls`); **inside `sheetData` or rows: 0**. |
| Blockers | G1: **798 of 813 worksheets (98%)** have no pivot table, top-level AC or `dataConsolidate`. G2: **773 of 813 (95%)** have only copyable relationships (§4.1). |
| `xr:uid` | 64 in sheets (34 files), 40 in tables (7): a copy would duplicate them (§4.3). |

Conclusions:

1. **Formula references are overwhelmingly plain**: single cells, areas and sheet-prefixed
   references. Whole columns and structured references are common in a few large files. 3-D
   references (3 files), multi-cell arrays (5) and external references (5) are rare enough to
   refuse where an edit would change them.
2. **Names are always absolute and sheet-qualified** in the corpus, so the one hard case for
   names (a relative reference whose meaning depends on the evaluating cell) can be refused
   without losing a real file.
3. **CF and DV formulas are relative to their range** in 71 formulas: a rule for
   re-anchoring them when the range's first cell is deleted is needed (§3.3.4).
4. **Shared groups survive structural edits** in 98% of the simulated cases; the rest are
   neighbour-row formulas whose members must be written out (§3.3.2).
5. **Excel writes both `#REF!` forms**: bare in 335 cell texts and sheet-prefixed in 14 (cells
   and names). ADR-0009 measured that a *deleted sheet* gives the bare form; a deleted *cell*
   reached through a sheet prefix gives the prefixed form (§3.2, an XL item).
6. Pivot tables, controls and AC-wrapped content are each in ≤ 7 files: refusing them costs
   2–5% of sheets.

### Reference libraries

Verified in source at pinned revisions (files and lines in the table and in Sources): Apache
POI @ `47dcff76` (**POI**), LibreOffice core @ `27135b95` (**LO**, master on 2026-11-20),
openpyxl 3.1.5 (**OP**), umya-spreadsheet 3.1.1 (**UM**). Microsoft's documentation is
cited where it states a behaviour; anything else about Excel is marked *(u)* and listed as an
XL item.

- **LO** is the reference for the **reference model**. Its rules are the ones Excel users
  expect and they are written down in one place: shrink, `#REF!`, expand inside, no expansion
  at the edge, sticky whole rows/columns, absolute = relative.
- **POI** is the reference for **what an editing library touches**, and for what goes wrong
  when it does not: it has no delete (a delete is a move onto the deleted rows), it does not
  adjust DV, autofilters, table `ref`s, pivots or chart series, and it does not check that a
  shared group stays consistent.
- **OP** and **UM** show what to avoid: OP adjusts no formula; UM leaves `$` references and
  never writes `#REF!`, so a reference into deleted rows silently points at other rows.

| Topic | LO | POI | OP | UM |
|---|---|---|---|---|
| Single reference in deleted rows | flagged deleted, shown `#REF!` (`token.cxx` L3213-3221; `setRefDeleted` L2809-2855) | `RefErrorPtg`/`AreaErrPtg`/`Deleted3DPxg` when the moved block lands on it (`FormulaShifter.java` L356-383, L793-813) | not adjusted (`worksheet.py` L717-750) | reduced by the offset: points at other rows (`helper/coordinate.rs` L335-341) |
| Range partly deleted | shrinks: top, middle, bottom (`token.cxx` L2870-2979) | truncated top or bottom (`FormulaShifter.java` L494-505) | – | not modelled |
| Range wholly deleted | deleted, `#REF!` (`token.cxx` L3251-3262) | deleted ref (L484-487) | – | – |
| Insert inside a range | expands (`expandRange`, `token.cxx` L2981-3054) | edge moves with the block (L418-474) | – | – |
| Insert at the top edge / just below | shifts / not expanded unless the `ExpandRefs` option (`token.cxx` L3037-3041, L3060-3129) | – | – | – |
| Whole rows/columns on the other axis | sticky, unchanged (`token.cxx` L2928-2930, L3022-3024, L3340-3347; `refupdat.cxx` L263-268) | no special case (a full column is an area 0..max) | – | skipped on insert; `unwrap` on remove (`formula.rs` L992-997, L1033) |
| `$` references | same as relative (`toAbs` before adjusting) | same | copy semantics only (`translate.py` L61-71) | **not adjusted** (`formula.rs` L971-984, L1036-1043) |
| Non-formula ranges (CF, merges, …) | `ScRefUpdate::Update` shrink/invalid/sticky (`refupdat.cxx` L187-282) | merges, CF `sqref` + formulas, hyperlinks, names, cell formulas on all sheets, drawings (`XSSFRowColShifter.java` L60-295; `RowShifter.java` L54-119); **not** DV, autofilter, table `ref`, pivots, chart series | none | CF, merges, autofilter, comments, drawings, names; **not** tables, DV, pivots (`worksheet.rs` L2916-3125) |
| Refusals | pivot intersect (`InsertCellsOperation.cxx` L365-371, `DeleteCellsOperation.cxx` L283-288); filled cells off the sheet (`InsertCellsOperation.cxx` L747-748, `column.cxx` L663-690); partial merges for cell shifts (L410-433, L316-335) | array cells in deleted rows skipped: "not implemented yet" (`XSSFSheet.java` L3284-3285) | – | – |
| Copy sheet | – | name "X (2)"; relations shared, unknown types dropped with a warning; **comments dropped**; drawing and charts cloned with `replaceReferences`; tables recreated with new `TableN` names, calculated columns by string replace; sheet-local names not copied (`XSSFWorkbook.java` L691-797, L815; `XSSFSheet.java` L4494-4503, L5107-5196) | "X Copy"; values, styles, merges, page setup, hyperlinks, comments; no images, charts, tables, CF, DV, names (`copier.py` L11-61) | – |
| Table / column rename | – | attribute only; **no formula rewritten** (`XSSFTable.java` L381-443; `XSSFTableColumn.java` L100-102) | – | – |

Microsoft documents three behaviours used here:

- "If you rename a column or table, Excel automatically changes the use of that table and
  column header in all structured references" ("Using structured references with Excel
  tables").
- Deleting a cell that a formula lists individually gives `#REF!`, while a range reference
  adjusts ("How to correct a #REF! error"; from the page summary, not re-fetched).
- Deleting part of an array formula is refused: "You cannot change part of an array" ("Rules
  for changing array formulas"; from the page summary) *(u for whole-row edits)*.

Excel's refusal to push non-blank cells off the sheet (KB 305568) and to shift cells in a
table were found only in summaries and community posts; they are treated as *(u)*.

## Decision

### 1. Scope

In scope, on worksheets of an opened workbook:

- **G1**: `Worksheet::insert_rows(at, n)`, `delete_rows(at, n)`, `insert_cols(at, n)`,
  `delete_cols(at, n)`. Indices are 0-based, like the column setters of ADR-0014 §9. The edit
  covers whole rows or whole columns.
- **G2**: `Workbook::copy_sheet(source, new_name, position)` for worksheets.
- **Renames**: `Workbook::rename_table(old, new)` and
  `Workbook::rename_table_column(table, old, new)`.
- **The location registry** (§2.2), which also lets ADR-0009's rename and delete rewrite the
  places they refuse today (DV, CF, x14, table formulas, connection parameters).

Each returns what it rewrote (`Vec<RewrittenLocation>`, ADR-0009) and what it removed (merges,
rules, notes, hyperlinks, …), so the caller can audit it.

Out of scope:

- inserting or deleting **cells** with a partial shift right or down, and cut/paste moves of
  ranges: these change references within one row or column only, which the model below could
  express, but Excel's refusals for them (merged cells, tables, arrays crossing the shifted
  block) need their own evidence;
- copying a sheet **between** workbooks (styles, strings, names and themes would have to be
  merged);
- copying chart sheets, macro sheets and dialog sheets;
- formula evaluation: values are invalidated, not recomputed (Phase1's policy);
- formatting of inserted rows and columns beyond §3.4 (Q1).

### 2. Common machinery

#### 2.1 One plan per operation

Every operation follows ADR-0017 §2.1, which is ADR-0003 §3.1 for multi-part edits:

1. **Validate (read-only).** Parse every part that holds a registry location (§2.2) and that
   the operation can reach. For each occurrence, compute the new value, a removal, or a
   refusal. Run the closed-world scan (§2.2). Build every new or changed small part. Compute
   the Phase1 seeds and the Phase2 counts. **Every refusal happens here**, so `Err` leaves
   every byte unchanged.
2. **Registry plan.** One `RegistryPlan` (PR #38) with:
   - `stage_part` for every changed **small** part: `workbook.xml`, tables, comments, VML,
     threaded comments, drawings, pivot cache definitions, `singleXmlCells`, `sharedStrings.xml`
     (`count`), `app.xml`; charts are spliced at the byte level as ADR-0009's implementation
     does (`sheet_ops/mod.rs` L982-1038), so only `c:f` text changes;
   - `remove_parts` for `calcChain.xml` (ADR-0003 §4.3) and for removed tables (ADR-0017 §6.4);
   - for G2, `add_part`, `add_relationship` and content types for the copy (§4).
3. **Apply.** The workbook is poisoned, `Package::apply(plan)` runs (its only error,
   `StalePlan`, comes before any change and clears the poison, ADR-0017 §2.1), then the
   **worksheet parts are edited in place** at positions validated in step 1:
   1. the edited sheet: removed rows and cells (in descending order), `r` renumbering,
      formula texts, `f/@ref`, the range features of §3.5;
   2. every other sheet: rewritten formula texts and `hyperlink/@location`s;
   3. cache strips of the Phase1 seeds (every `v`/`is`/`t`, ADR-0003 rev 5 §3.1), on any sheet;

   then the census, the dependency graph, the row index and the Phase2 registries are
   **rebuilt** from the output parts, as ADR-0009 §2 does after a sheet operation. The poison
   is cleared after the rebuild.

Worksheet parts are edited in place rather than staged as clones (ADR-0009 §2 stages clones)
because a G1 edit touches nearly every node of a large sheet: a staged clone of a 200k-formula
sheet costs 289 ms (ADR-0003, measured) and doubles the tree's memory (1.44 GiB for 1M cells,
ADR-0010 Context). This is the same trade ADR-0017 §2.1 made for the `CellWritePlan`.
In-place steps cannot refuse: their inputs (positions, texts, attribute values) are computed
and checked in step 1. A failure there is a bug and leaves the workbook poisoned.
**Fault injection** (`Err` and panic, `catch_unwind`) before each in-place step and inside the
rebuild, and an injected `StalePlan`, are tested (ADR-0003 §7.3, ADR-0017 §2.1).

#### 2.2 The location registry

One table lists **every place in a package that can hold a cell position, a range or a
formula**, with its syntax and its rule for each operation. It extends ADR-0009 §6's table
from sheet names to positions. It exists twice, and a test keeps the two equal:

- in this ADR (below), for review;
- as data: `crates/ooxml-xlsx/src/locations.rs`, a `const LOCATIONS: &[Location]` with
  `{ id, part: PartKind, path: &[(ns, local)], attr: Option<&str>, syntax, g1, copy, rename }`.

The code and the tests use the data:

- **Dispatch.** Each operation walks `LOCATIONS` and calls the rewriter for the row's syntax
  (§2.3). A location with no rewriter for an operation has rule `Refuse` or `Unaffected`; there
  is no third state.
- **Closed-world scan.** ADR-0009's scan (`sheet_ops/scan.rs` L52-73, a hand-written `match`)
  becomes a lookup in `LOCATIONS`. For G1 it looks for references to the edited sheet: the
  sheet-prefix form in every part, and any reference token in the edited sheet's own parts.
  For table renames it looks for `Name[` and bare `Name` tokens. A hit outside a registry row
  or a known non-reference refuses with `Err(UnrewritableReference)`. The blind spots are
  ADR-0009 §6's (binary parts; an unparseable XML part gets a byte scan, and a hit refuses).
- **Schema completeness test.** Every attribute and element that `sml.xsd` types as
  `ST_Ref`, `ST_CellRef`, `ST_Sqref`, `ST_RefA` or `ST_Formula` (56 today), and every
  element derived from `ST_Formula`, must map to a registry row or to an entry of an explicit
  `NOT_APPLICABLE` list with a reason (e.g. revision logs: the workbook is refused as shared;
  pivot-table internals: pivots are refused). The test reads the bundled XSD. A new schema
  place therefore fails CI until someone decides its rule.
- **ADR ↔ data test**, as ADR-0014 R2-3: the ids, parts and paths in the table below equal
  `LOCATIONS`.

Syntax kinds:

| Syntax | Meaning | Rewriter |
|---|---|---|
| `Formula(anchor)` | formula text; relative references are relative to `anchor` (the cell, the range's first cell, the master, or none) | §3.2–§3.3 on lexer tokens; the rest of the text is copied |
| `Sqref` | space-separated list of ranges | §3.2 range rule per item; empty items dropped |
| `Range` | one range | §3.2 range rule |
| `Cell` | one cell | §3.2 cell rule |
| `Index(axis)` | a row or column number (0- or 1-based as stated) | §3.2 on that axis |
| `Marker` | DrawingML or VML anchor: cell + offset | §3.6 |
| `Location` | `Sheet!A1` navigation target | §3.2 cell/range rule; never `#REF!` |
| `RawName` | a table or sheet name without syntax | renames only |

Rules (columns G1, Copy, Rename): **R** = rewrite; **D** = rewrite, and the element is removed
when its reference is wholly deleted; **X** = the operation refuses if this place would change;
**XX** = the operation refuses if the place exists on the edited or copied sheet; **–** =
unaffected; **N** = renumbered/duplicated as stated in §4.

| # | Part | Place | Syntax | G1 (§3) | Copy (§4) | Table / column rename (§5, §6) | Corpus (count / files) |
|---|---|---|---|---|---|---|---|
| L1 | worksheet | `sheetData/row/@r` | `Index(row, 1-based)` | D (row removed) | copied | – | 813 sheets |
| L2 | worksheet | `row/@spans` | column hint `a:b` | R (column edits) | copied | – | 88,922 / 275 |
| L3 | worksheet | `row/c/@r` | `Cell` | D (cell removed) | copied | – | 1,502,954 cells |
| L4 | worksheet (any sheet) | `c/f` text: normal, shared master, array, data-table anchor | `Formula(cell)`; shared: `Formula(master)` | R (§3.3) | R (§4.4) | R | 11,561 / 126 |
| L5 | worksheet | `c/f/@ref` (shared, array) | `Range` | R; X when an array is split (§3.3.3) | copied | – | 359 + 53 |
| L6 | worksheet | `c/f/@r1`, `@r2` (data table) | `Cell` | R; X when deleted | copied | – | 0 (1 synthetic) |
| L7 | worksheet | `dimension/@ref` | `Range` | R | copied | – | 787 |
| L8 | worksheet | `cols/col/@min`, `@max` | `Index(col, 1-based)` pairs | R (§3.4) | copied | – | 14,555 / 179 |
| L9 | worksheet | `sheetView/@topLeftCell`, `pane/@topLeftCell` | `Cell` | R, clamped (§3.5) | copied | – | 81 / 53 |
| L10 | worksheet | `pane/@xSplit`, `@ySplit` (frozen) | count | R (§3.5) | copied | – | 32 / 19 |
| L11 | worksheet | `selection/@activeCell`, `@sqref` | `Cell`, `Sqref` | R, clamped | copied | – | 481 / 272 |
| L12 | worksheet | `mergeCells/mergeCell/@ref` | `Range` | D (§3.5) | copied | – | 54,468 / 43 |
| L13 | worksheet | `hyperlinks/hyperlink/@ref` | `Range` | D | copied | – | 113 / 15 |
| L14 | worksheet (any sheet) | `hyperlink/@location` | `Location` | R; left when deleted (reported) | R if it names the source (§4.4) | – | 38 / 3 |
| L15 | worksheet | `autoFilter/@ref`, `filterColumn/@colId`, `autoFilter/sortState/@ref`, `sortCondition/@ref`; top-level `sortState` | `Range`, column offset | R; X header row or a filtered column deleted (§3.5) | copied | – | 3 + 7 |
| L16 | worksheet | `conditionalFormatting/@sqref` | `Sqref` | D (rule removed when empty) | copied | – | 108 / 20 |
| L17 | worksheet | `cfRule/formula`, `cfvo/@val` (`type="formula"`) | `Formula(sqref first cell)` | R (§3.3.4) | R | R | 89 / 18, 2 |
| L18 | worksheet | `dataValidation/@sqref` | `Sqref` | D | copied | – | 108 / 13 |
| L19 | worksheet | `dataValidation/formula1`, `formula2` | `Formula(sqref first cell)` | R (§3.3.4) | R | R | 122 / 12 |
| L20 | worksheet | `extLst` x14 CF/DV: `xm:sqref`, `xm:f`, `x14:cfvo/xm:f` | as L16–L19 | as L16–L19 | R | R | 22 / 6, 25 / 6 |
| L21 | worksheet | `extLst` sparklines: `x14:sparkline/xm:f` (data), `xm:sqref` (host), `x14:sparklineGroup/xm:f` (date axis) | `Formula(none)`, `Cell` | R; sparkline removed with its host | R | R | 11 / 1 |
| L22 | worksheet | `rowBreaks/brk/@id`, `colBreaks/brk/@id` (+ `count`, `manualBreakCount`) | `Index(0-based)` | D | copied | – | 10 + 3 / 4 + 3 |
| L23 | worksheet | `ignoredErrors/ignoredError/@sqref` | `Sqref` | D | copied | – | 5 / 3 |
| L24 | worksheet | `protectedRanges/protectedRange/@sqref` | `Sqref` | R; X when it would become empty | copied | – | 4 / 4 |
| L25 | worksheet | `customSheetViews/customSheetView/…` (`selection`, `pane`, breaks, `autoFilter`) | as L9–L15, L22 | as there | copied | – | 3 / 1 |
| L26 | worksheet | `cellWatches/cellWatch/@r`, `smartTags/cellSmartTags/@r` | `Cell` | D | copied | – | 0 |
| L27 | worksheet | `scenarios/scenario/inputCells/@r`, `scenarios/@sqref` | `Cell`, `Sqref` | X | XX | – | 0 |
| L28 | worksheet | `dataConsolidate/dataRefs/dataRef/@ref` | `Range` | XX | XX | – | 1 / 1 |
| L29 | worksheet | `webPublishItems/webPublishItem/@sourceRef` | `Range` | X | XX | – | 0 |
| L30 | worksheet | `oleObjects`, `controls` (anchors, `@linkedCell`, `@listFillRange`) | `Marker`, `Formula` | XX | XX | – | 6 / 5, 2 / 2 |
| L31 | worksheet | `mc:AlternateContent` at the top level, in `sheetData` or in a row | – | XX | XX | – | 2 / 2, 0 |
| L32 | workbook | `definedName` text (all scopes, `_xlnm.*` included) | `Formula(none)` | R (§3.3.5) | R + local names duplicated (§4.4) | R | 858 / 62 |
| L33 | workbook | `definedName/@localSheetId`, `workbookView/@activeTab`, `@firstSheet` | position | – | N (ADR-0009 §4) | – | 142 / 24 |
| L34 | `calcChain.xml` | `c/@r`, `c/@i` | `Cell` | part removed (ADR-0003 §4.3) | part removed | – | 116 |
| L35 | table | `table/@ref`, `autoFilter/@ref`, `sortState/@ref`, `sortCondition/@ref` | `Range` | R / X (§3.6.1) | N (§4.3) | – | 61 / 29 |
| L36 | table | `tableColumns/tableColumn` set, `filterColumn/@colId` | columns | R (§3.6.1) | copied | column name: R | 199 / 29 |
| L37 | table | `calculatedColumnFormula`, `totalsRowFormula` | `Formula(first data row)` | R | R | R | 16 / 8 |
| L38 | table | `table/@name`, `@displayName` | `RawName` | – | N | R | 61 / 29 |
| L39 | comments | `comment/@ref` | `Cell` | D (note removed with its VML shape and thread) | copied | – | 204 / 27 |
| L40 | VML | `x:ClientData/x:Row`, `x:Column` | `Index(0-based)` | R with L39 | N (§4.3) | – | 204 / 27 |
| L41 | VML | `x:ClientData/x:Anchor` | `Marker` (8 integers) | R (§3.6.3) | copied | – | 410 / 37 |
| L42 | VML | `x:FmlaLink`, `x:FmlaRange`, `x:FmlaTxbx`, `x:FmlaPict`, `x:FmlaGroup` | `Formula(none)` | X | XX (controls) | X | 57 / 4 |
| L43 | threaded comments | `threadedComment/@ref` | `Cell` | D with L39 | N (new GUIDs) | – | 4 / 3 |
| L44 | drawing | `xdr:twoCellAnchor`/`oneCellAnchor` `xdr:from`, `xdr:to` | `Marker` | R / X (§3.6.3) | copied | – | 181 + 30 / 47 + 8 |
| L45 | drawing | anchors inside `mc:AlternateContent` | `Marker` | X when they would change | XX | – | 170 / 4 |
| L46 | drawing, chart user shapes | `xdr:sp/@textlink`, `cdr:sp/@textlink` | `Formula(none)` | R | R | R | 0 |
| L47 | chart | `c:f`, `c15:f`, `c15:sqref`, `c15:fullRef`, `c15:formulaRef`, `c15:datalabelsRange`, `c15:filteredSeriesTitle`, `c15:filteredCategoryTitle` | `Formula(none)` | R; X when a series range is wholly deleted (§3.6.4) | R (§4.4) | R | 174 / 25 |
| L48 | chartEx | `cx:f` | `Formula(none)` | X | XX | X | 0 |
| L49 | chart | `c:pivotSource/c:name` | `RawName` | – (pivot rules) | XX | – | 1 / 1 |
| L50 | pivot cache | `worksheetSource/@ref` (+ `@sheet`) | `Range` | R when moved; X when expanded or shrunk (§3.6.5) | – | – | 7 / 5 |
| L51 | pivot cache | `worksheetSource/@name` | `RawName` (table or name) | – | – | R (table rename) | 3 / 3 |
| L52 | pivot table | `location/@ref` and everything inside | `Range` | X if the edit crosses or moves it (§3.6.5) | XX | – | 12 / 7 |
| L53 | pivot cache | `consolidation/rangeSets/rangeSet/@ref` | `Range` | X | – | – | 0 |
| L54 | `tableSingleCells` | `singleXmlCell/@r` | `Cell` | R; X when deleted | XX | – | 14 / 14 |
| L55 | `connections.xml` | `parameter/@cell` | `Formula(none)` | R | – | – | 12 / 1 |
| L56 | ctrlProps | `@fmlaLink`, `@fmlaRange`, `@fmlaTxbx`, `@fmlaGroup` | `Formula(none)` | X | XX | X | 0 non-empty |
| L57 | query table | `queryTable` bound to a `queryTable` table | – | X when its table's `ref` changes | XX | X (rename) | 13 / 2 |
| L58 | revisions | `xl/revisions/*` | – | XX (`SharedWorkbook`) | XX | XX | 1 / 1 |
| L59 | `activeX*.bin` | `LinkedCell`, `ListFillRange` (binary) | – | XX (`BinaryReferencesPossible`, ADR-0009 §6) | XX | XX | 0 |
| L60 | `externalLink` | another workbook's cells and names | – | – | – | – | 14 / 10 |
| L61 | `metadata.xml`, `richData/*` | `cm`/`vm` indices (not positions) | – | – (move with the cell) | copied | – | 1, 2 |

Known non-references (ADR-0009 §6): cell values, shared strings, comment and note text,
formula string literals (`INDIRECT("A1")`), `docProps`.

#### 2.3 One rewriter per syntax

The rewriters are pure functions from an old value and an edit to `Keep`, `New(value)`,
`Remove` or `Refuse(reason)`, shared by every location of that syntax:

- `shift_area(area, edit) -> AreaOutcome` on lexer areas (§3.2), with `Moved`, `Resized`,
  `Deleted` and `Unchanged` outcomes, so the caller knows whether a cache depends on it;
- `rewrite_formula(text, anchor, edit, ctx)`: lexes with Phase1's lexer
  (`formula/lexer.rs`), applies `shift_area` to every reference token that refers to the
  edited sheet, re-renders only those tokens (as `translate.rs` L53-80 and ADR-0009's
  `rewrite_formula`, `sheet_ops/names.rs` L185-237, do), and checks that the result lexes;
- `rewrite_sqref`, `rewrite_range`, `rewrite_cell`, `rewrite_index`, `rewrite_marker`;
- for renames, `rewrite_structured(text, table, column_map)` on the structured-reference
  sub-grammar of ADR-0017 §6.3 (`tables/references.rs` L37-126), which moves into
  `formula/` and returns the byte spans of the table name and of each column element.

A token "refers to the edited sheet" when its prefix is the sheet's name (ADR-0003 §2.4
folding), or when it has no prefix and its formula lives on the edited sheet (or in a place
anchored there: CF, DV, sparklines, tables, notes). A name is never anchored (§3.3.5).

#### 2.4 Refusals common to all operations

Refused before anything changes, with `Err` and every byte unchanged:

- a shared workbook (`SharedWorkbook`), ActiveX binaries (`BinaryReferencesPossible`), a
  poisoned workbook;
- an edited or copied sheet with rows or cells without `r` (`MissingReference`; 15 sheets in
  4 files): materialising `r` would rewrite every row of the sheet, and Phase1 already refuses
  formula edits there;
- a **registry place marked X or XX** (§2.2) that the operation would change, or a
  closed-world scan hit outside the registry (`UnrewritableReference { part, location }`);
- any formula text that must change and does not lex (`UnrewritableReference`; 3 cell texts
  in the corpus do not lex in the spike's lexer);
- a part that must change and cannot be parsed (`PartNotEditable`), e.g. the one corpus VML
  that is not XML.

### 3. Insert and delete rows and columns

#### 3.1 API

```rust
let report = wb.worksheet_mut("Data")?.insert_rows(4, 2)?;   // two blank rows before row index 4 (row 5)
let report = wb.worksheet_mut("Data")?.delete_cols(1, 1)?;   // delete column B
// report.rewritten: Vec<RewrittenLocation>; report.removed: Vec<RemovedItem>
```

`n ≥ 1` and the edit must lie inside the sheet. Rows and columns are symmetrical; this
section says "rows" and means either axis.

#### 3.2 The reference model

On the edited axis, an edit is `Insert { at: p, n }` (new rows take indices `p..p+n`, existing
rows `≥ p` move down by `n`) or `Delete { at: p, n }` (rows `p..p+n` go, rows `≥ p+n` move up
by `n`). Every component is adjusted whether it has a `$` or not: `$` only governs copying
(LO `toAbs`; POI; OP and UM differ and are wrong for this purpose).

| Reference | Insert at `p` | Delete `p..p+n` |
|---|---|---|
| cell `r` | `r ≥ p` → `r + n` | `r` inside → **deleted**; `r ≥ p+n` → `r − n` |
| range `r1..r2` | `p ≤ r1` → both move; `r1 < p ≤ r2` → **expands** to `r2 + n`; `p > r2` → unchanged (no expansion at the bottom edge) | wholly inside → **deleted**; partly → **shrinks** to the surviving rows; spanning → `r2 − n` |
| whole rows `1:3` | as a range | as a range |
| whole columns `A:C` (row edit) | **unchanged** (sticky) | unchanged |
| range spanning the whole axis (`A1:A1048576`, 0 in the corpus) | unchanged (sticky, LO `refupdat.cxx` L263-268) | unchanged |
| any other component pushed past row 1,048,576 or column XFD | refused: `ShiftOffSheet` *(u: Excel's form)* | – |

These are LO's rules with its `ExpandRefs` option off (§Reference libraries). Inserting at the
first row of a range moves it; inserting just below it does not grow it. Excel behaves the same
*(u for the edge cases: XL item 1)*.

**Deleted references.** A deleted reference token becomes `#REF!`, replacing the whole token
(area included):

- unprefixed: `#REF!` (`SUM(A5)` → `SUM(#REF!)`);
- sheet-prefixed: `Sheet1!#REF!`, keeping the prefix as written (quotes included). The corpus
  has both forms written by Excel (fact 5); ADR-0009 established the bare form for a deleted
  *sheet*. Phase1's lexer reads both (`Area::RefError`, `lexer.rs` L28-29) *(u: which form
  Excel writes for a deleted cell through a prefix: XL item 2)*.

**Insert refusals** (before any change):

- **Content pushed off the sheet**: any `row` element in the last `n` rows (or any `c` in the
  last `n` columns), any merge, hyperlink, rule, table, note, anchor or other registry range
  that reaches there: `Err(ShiftOffSheet { what })`. LO refuses the same
  (`InsertCellsOperation.cxx` L747-748, "Filled cells cannot be shifted beyond the sheet") and
  so does Excel *(u: KB 305568, not fetched)*. One corpus sheet uses row 1,048,576.

#### 3.3 Formulas

##### 3.3.1 Cell formulas

- A1 text stores positions, not offsets, so a formula's text depends on what it refers to, not
  on where its own cell moves. Every formula on **every sheet** is rewritten with §3.2 for the
  tokens that refer to the edited sheet (§2.3).
- **Pre-filter.** A formula whose census precedents (ADR-0003 §2.2) do not reach the edited
  sheet at or after `p`, and that is not `Everything`, cannot change and is not lexed again.
- **3-D references** whose span includes the edited sheet and whose area would change are
  refused (`UnrewritableReference`; 16 texts in 3 files). Excel's treatment of a 3-D reference
  when one sheet of the span changes is not documented *(u: XL item 3)*.
- **External references** (`[n]Sheet!A1`) are another workbook's and never change. `[0]`
  self-references that would change are refused (as ADR-0009).
- **Structured references** are relative to their table and do not change; the table's range
  does (§3.6.1). Deleting a table column that a structured reference names is refused
  (`ColumnInUse`), and so is deleting a whole table that is referenced (`TableInUse`,
  ADR-0017 §6.4). Excel rewrites such references to `#REF!` *(u)*; refusing keeps them
  readable until that is checked (XL item 4).
- **Spill references** (`A1#`) are cell references on their anchor (0 in the corpus).
- **`INDIRECT`, `OFFSET` with text, `ADDRESS`**: string arguments are not references and are
  not rewritten (ADR-0009 §6). The formula's precedents are `Everything` (ADR-0003 §2.2), so
  its cache is stripped (§3.7).

##### 3.3.2 Shared formulas

A member's formula is the master's text translated to the member (ADR-0003 §4.2). For each
group touching the edited sheet:

1. compute every surviving member's **own** formula (master text translated to it), and
   rewrite it with §3.2;
2. rewrite the master's text (if the master is deleted, hand off to the first remaining member
   in row-major order, ADR-0003 §4.2 steps 1–4, then rewrite);
3. a member stays in the group **iff** the new master text translated to the member's new
   position equals its rewritten own formula; otherwise it gets its own plain `f` with that
   text, and leaves the group;
4. the master's `ref` becomes the bounding rectangle of the remaining members (ADR-0003 §4.2
   step 3); a group left with one cell becomes a plain formula.

The spike runs this on the corpus: 286 of 292 multi-row groups stay whole, 6 split (fact 4).
POI rewrites the master and its `ref` without step 3 (`XSSFRowColShifter.java` L79-136), which
gives those 6 groups wrong member formulas *(inference from the code)*.

##### 3.3.3 Array formulas, data tables, dynamic arrays

- A multi-cell **array** whose `ref` would be split (an insert strictly inside it, a delete of
  part of it) is refused: `Err(ArrayFormulaSplit { cell })`. Microsoft refuses deleting part
  of an array ("Rules for changing array formulas"; *u* for whole rows); POI skips such cells
  with "not implemented yet" (`XSSFSheet.java` L3284-3285). An array wholly inside the
  deleted rows is deleted with them; one wholly outside moves (`f/@ref` rewritten). 43
  multi-cell arrays in 5 files.
- **Data tables** (`t="dataTable"`): the same rule for `ref`; `r1`/`r2` follow the cell rule;
  a deleted input cell is refused (`ReferenceDeleted`). 0 in the corpus.
- **Dynamic arrays** (`cm` anchor): the anchor's `ref` (the last spill extent) follows the
  array rule; an edit crossing it is refused until an Excel check shows how the spill is
  rewritten (1 anchor in the corpus; XL item 5). `cm`/`vm` indices move with their cells.

##### 3.3.4 Formulas relative to a range (CF, DV, sparklines, table columns)

A CF or DV formula's relative references are relative to the **first cell of its
`sqref`** (the top-left cell of the first range) *(u: ECMA-376 Part 1 §18.3.1 does not state
the anchor; this is how Excel's own files read: XL item 6)*. 71 corpus formulas use relative
references. After the edit:

- if the anchor survives, the formula is rewritten with §3.2;
- if the anchor is deleted but the `sqref` keeps cells, the new anchor is the first surviving
  cell. The formula is first **translated** from the old anchor to the new anchor's *old*
  position (ADR-0003 §4.2), then rewritten with §3.2. This is the same hand-off as a shared
  master, and it keeps the rule's meaning for every surviving cell *(u: XL item 6)*;
- if the `sqref` becomes empty, the rule is removed (§3.5).

Table `calculatedColumnFormula` and `totalsRowFormula` are treated as relative to the column's
first data cell and the totals cell *(u)*; their A1 references are rare (6 texts in 3 files).

##### 3.3.5 Defined names

- Name texts are rewritten with §3.2 like cell formulas, with no anchor.
- **A name with an unprefixed reference, or a relative component in a reference to the
  edited sheet, is refused** (`UnrewritableReference`): its meaning depends on the cell that
  uses it. The corpus has none (0 of 858).
- `_xlnm.Print_Area` and `_xlnm.Print_Titles` follow the range rule. A union member that is
  wholly deleted is dropped from the union; when none is left, the name is removed *(u: XL
  item 7)*. `_xlnm._FilterDatabase` follows the autofilter (§3.5).
- Names already holding `#REF!` are copied unchanged.

#### 3.4 `sheetData`, `dimension`, `cols`

- **Rows**: row elements inside a deleted band are removed. `row/@r` of every row at or after
  `p` changes. For column edits, cells in deleted columns are removed and every later cell's
  `r` changes; `row/@spans` (a hint, ECMA-376 §18.3.1.73) is rewritten with the range rule and
  removed when it would be empty.
- **Cells**: `c/@r` follows its row and column. Deleting a cell with `t="s"` lowers
  `sst/@count` (ADR-0006 §4.5).
- **`dimension/@ref`** follows the range rule (a delete may leave it larger than the used range;
  that is allowed, it is a hint).
- **`cols`**: a `col` range on the edited axis is rewritten with the range rule on columns,
  except that an insert strictly inside a `col` range **splits** it around the new columns
  instead of expanding it. New rows and columns therefore get the default format (no `row` or
  `col` element). Excel formats them like their neighbours by default *(u)*; Q1 asks whether
  to copy the neighbour's `row`/`col` attributes and cell styles.
- `sheetFormatPr`, outline levels, `row/@ht`/`hidden` move with their row; `outlineLevelRow`
  is not lowered (ADR-0014 §9).

#### 3.5 Range features

| Place | Rule |
|---|---|
| merges | range rule. A merge that shrinks to one cell or is wholly deleted is removed. If its anchor row is deleted, the first surviving cell becomes the anchor; non-anchor cells are empty (Phase0 guard), so no value is lost that Excel would keep *(u)*. |
| hyperlinks | `@ref` by the range rule; removed when deleted. Its relationship stays (as ADR-0014 `remove_hyperlink`). `@location` on any sheet: cell/range rule; a deleted target is left as written and reported (ADR-0009 §5). |
| autofilter | `@ref` by the range rule. Deleting its **header row** or a column with a `filterColumn` is refused (`StructureBlocked`). `filterColumn/@colId` (offset from the first column) is re-indexed. `sortState`/`sortCondition` follow. |
| CF / DV (2007 and x14) | `sqref` items by the range rule; empty items dropped; the rule removed when the list is empty, with its linked x14 rule and empty containers (ADR-0014 §2's container rule); `count` updated. Formulas: §3.3.4. |
| sparklines | data `xm:f` by §3.3; host `xm:sqref` by the cell rule; a sparkline whose host is deleted is removed, then an empty group, then an empty `ext`. |
| views | `topLeftCell`, `activeCell`: cell rule, **clamped** to the first surviving cell when deleted. `selection/@sqref`: range rule, `activeCell` when empty. Frozen panes: `ySplit` (rows above the freeze line) grows by the inserted rows above it and shrinks by the deleted rows above it *(u: XL item 8)*. |
| breaks | `brk/@id` by the index rule; removed when deleted; `count`, `manualBreakCount` updated. |
| `ignoredErrors` | as CF `sqref`; the element removed when empty. |
| `protectedRanges` | range rule; **refused** when a protected range would become empty (it carries a password hash; removing it silently changes protection). |
| `customSheetViews` | the same rules for their `selection`, `pane`, breaks and `autoFilter`. |
| `scenarios`, `dataConsolidate`, `webPublishItems`, `oleObjects`, `controls`, top-level AC | refused when present (§2.2 L27–L31). |

#### 3.6 Objects

##### 3.6.1 Tables

- Inserting rows strictly inside a table (after the header, up to and including the totals
  row) **expands** `ref` and the table's `autoFilter/@ref`; inserting at the header row moves
  the table. The new rows are empty table rows. Calculated columns are **not** filled into the
  new rows *(u: Excel fills them: XL item 9)*.
- Deleting rows: the header row → `Err(TableStructure { table, reason: "header row" })`; the
  totals row → refused; every data row → refused (a table keeps at least one); the whole
  table → removed through ADR-0017 §6.4 (refused when referenced, `TableInUse`).
- Inserting columns strictly inside a table adds `tableColumn`s (ids `max + 1`, names
  `Column{n}`, the smallest free, ADR-0017 §6.2) and writes their header cells through the
  `CellWritePlan` (ADR-0017 §2.1) *(u: Excel's names: XL item 9)*. Q11 asks whether to refuse
  instead. `queryTable` and `xml` tables refuse (L57).
- Deleting a column of a table removes its `tableColumn`, unless it is the last one or any
  structured reference names it (`ColumnInUse`), or the table is a pivot or slicer source.
- Tables are rewritten as small staged parts. The table's position check against merges and
  autofilters (ADR-0017 §6.1) holds after the edit by construction: everything moves alike.

##### 3.6.2 Notes and threaded comments

- `comment/@ref` and the VML `x:Row`/`x:Column` of its shape follow the cell rule; `x:Anchor`
  follows §3.6.3. A note whose cell is deleted is removed with its VML shape (matched as in
  ADR-0017 §4), and a threaded comment with its replies and fallback note (§5 there).
- This uses Phase5's notes machinery (P5c). Until P5c is merged, a G1 edit that would change a
  note refuses (`StructureBlocked`).

##### 3.6.3 Drawing and VML anchors

A marker is (cell index, offset in EMU or pixels). For each anchor on the edited sheet:

- `twoCellAnchor` with `editAs="twoCell"` (the default): `from` and `to` follow the cell rule,
  so inserting inside the object stretches it, as "move and size with cells" means;
- `editAs="oneCell"` and `oneCellAnchor`: `from` follows the cell rule and `to` moves by the
  same amount (size kept);
- `editAs="absolute"` and `absoluteAnchor`: unchanged;
- a marker in **deleted** rows: refused (`ObjectInDeletedRange { part, what }`). Excel either
  deletes the object or collapses it to zero height *(u)*; Q2 asks which to adopt;
- VML `x:Anchor` (left column, offset, top row, offset, right column, offset, bottom row,
  offset) follows the same rules by its `x:MoveWithCells`/`x:SizeWithCells` flags;
- anchors inside `mc:AlternateContent` (170 in 4 files, all form-control files) that would
  change: refused.

POI moves a shape when its top-left anchor is inside the shifted band and leaves absolute
anchors (`XSSFRowColShifter.java` L260-295); it does not resize.

##### 3.6.4 Charts

Every chart's `c:f` family (L47) is rewritten with §3.3, spliced into the original bytes, in
every chart part of the package (charts on other sheets plot this one too; 168 sheet-prefixed
`c:f`). Caches are left as they are; Excel re-reads linked ranges on open *(u, roadmap §4)*,
and ADR-0017 §7.2's explicit refresh is available. A series range that would be **wholly
deleted** is refused (`ReferenceDeleted`) rather than written as `Sheet1!#REF!` *(u: XL item
10; Q8)*.

##### 3.6.5 Pivot tables and caches

- An edit that crosses or moves a pivot table's `location/@ref` is refused
  (`StructureBlocked`), as LO does for intersecting edits
  (`InsertCellsOperation.cxx` L365-371). Edits wholly after (below or right of) it are
  allowed. 12 pivot tables on 12 sheets.
- A cache whose `worksheetSource` is on the edited sheet: when its `ref` only **moves**, it
  is rewritten; when it would expand, shrink or be deleted, the edit is refused: the cache
  records would no longer match the range until a refresh (Q7).

##### 3.6.6 Others

`singleXmlCell/@r` (14 parts) follows the cell rule (deleted → refused); `parameter/@cell`
in `connections.xml` follows §3.3; ctrlProps and VML `x:Fmla*` that would change are refused.

#### 3.7 Phase1 and Phase2

- **calcChain**: removed in the operation's plan whenever a formula cell moves or is deleted
  (ADR-0003 §4.3; the chain keys on cell positions).
- **Seeds** (cache invalidation, ADR-0003 §3.2), all sound over-approximations:
  - every formula whose text changed (an expanded or shrunk range changes `ROWS`,
    `COUNTBLANK`, …; `#REF!` changes everything);
  - every formula **cell that moved**, because `ROW()`, `COLUMN()` and `CELL()` without
    arguments depend on their own position (5,490 `ROW` calls in 5 files);
  - the dependents of deleted cells;
  - every `Everything` formula (ADR-0009 M3).

  Their caches are stripped on every sheet and `fullCalcOnLoad` is set, honouring
  `KeepStaleCaches` and manual calculation as ADR-0009 §2 does.
- **Phase2**: `sst/@count` lowered for deleted `t="s"` cells. No string or style entry is
  removed (Layer 3 append-only).

#### 3.8 Performance

- Work is linear in the rows and cells at or after `p` (an `r` write each) plus the formulas
  whose text changes (a lex and a render each). The census pre-filter (§3.3.1) skips formulas
  that cannot change.
- Changing an attribute marks only that element `own_dirty`. The serializer then re-renders
  only its open tag, with the original attribute spelling, and copies its children's spans
  (`crates/ooxml-core/src/xml/write.rs` L21-63). A renumbered row therefore costs its open tag
  and its cells' open tags, not their `v`/`f` bytes.
- No clone of a worksheet is made (§2.1). The census and row index are rebuilt once, at open
  cost: 0.29 s census scan for a 1M-cell sheet (ADR-0010 Context; measured on the 2026-11
  baseline).
- **Deferred rows** (ADR-0010 §2, not on main): a G1 edit must expand every row after `p`,
  because cells carry their row number. That is acceptable for a rare operation and is
  recorded as a requirement on stage 2x: the row index must allow a bulk renumber.
- Not measured yet: no G1 code exists. P7a adds a criterion benchmark (insert one row at
  row 2 of `ooxml-bench`'s 1M-cell sheet, then save), and reports time and peak memory
  against open-and-save of the same file. Q10 asks for the target.

### 4. Copy a worksheet

#### 4.1 What is copied, what is refused

`copy_sheet(source, new_name, position)` validates the name (ADR-0009 §3) and allocates
`sheetId`, part name and relationship id as ADR-0009 §3 does.

The relationship types of the source decide:

| Source relationship | Copy |
|---|---|
| `printerSettings`, `customProperty` | new part, same bytes |
| `image` (background), images of drawings and VML | **shared**: a new relationship to the same media part (ADR-0017 Q2) |
| `drawing` | new drawing part (bytes, then §4.4); its charts copied as new parts with `chartStyle`/`chartColorStyle`/`chartUserShapes` |
| `vmlDrawing`, `comments`, `threadedComment` | new parts; VML renumbered (§4.3); threads get new GUIDs, the same persons |
| `table` | new table parts (§4.3) |
| `hyperlink` (External) | copied relationship, same target |
| `pivotTable`, `oleObject`, `control`, `ctrlProp`, `package`, `tableSingleCells`, query tables, slicers, timelines, chartEx, any other | **refused** (`SheetHasUnsupportedParts { rel_types }`) |

Also refused: top-level `mc:AlternateContent`, `scenarios`, `dataConsolidate`, and a VML
part that is not XML when it must be renumbered. Under these rules **773 of 813 corpus
worksheets** can be copied; the blockers are `tableSingleCells` (14 sheets), pivot tables
(12), OLE objects (5), query tables (4), controls (2), `package` (2), AC (2).

#### 4.2 The copied sheet part

The new sheet part starts as the **source's current bytes** (or serialized tree), so it is
structurally identical, then:

- `r:id`s are remapped to the new relationships;
- `sheetView/@tabSelected` is removed (the copy is not selected);
- `xr:uid` attributes are regenerated (64 in sheets, 40 in tables in the corpus), so no two
  CF rules, DV rules or tables share one *(u: whether Excel minds duplicates; XL item 11)*;
- formulas: §4.4.

#### 4.3 Ids and names inside the copy

- **Tables**: new `id` (workbook `max + 1` with the session high-water, ADR-0017 §2.2), new
  name and display name: the source name without trailing digits, followed by the smallest
  `n ≥ 2` that is free among tables and defined names (`Sales` → `Sales2`, `Table1` →
  `Table2`) *(u: Excel's form; Q5)*. POI uses `TableN` (`XSSFSheet.java` L4494-4503).
- **VML**: a new `o:idmap` block (`max + 1` over the workbook, ADR-0017 §2.2); shape ids
  renumbered into it in document order, `o:spid` too.
- **Drawings**: `cNvPr/@id`s are scoped to the drawing part and stay.
- **Threaded comments**: new GUIDs for `id`, matching `parentId`s and the fallback notes'
  `xr:uid` (ADR-0017 §5).

#### 4.4 References in and to the copy

- **Formulas on the copy that refer to the source by name** (`Data!A1` written on `Data`)
  keep their text, so they still refer to the source. Excel's internal form for such a
  reference is believed to be sheet-qualified, so it would do the same *(u: XL item 12)*.
  Unprefixed references are the copy's own cells by construction.
- **Structured references on the copy** to a table of the source become references to the
  copy's table (by token, §2.3), in cells, calculated columns, totals, CF, DV and names
  duplicated below. References to tables on other sheets are kept. POI does the same with a
  string replace (`XSSFSheet.java` L5176).
- **Charts copied with the sheet** have their `c:f` references to the source retargeted to
  the copy, as POI's `replaceReferences` does (`XSSFWorkbook.java` L759-794) and as Excel is
  believed to do *(u: XL item 12)*.
- **Sheet-local names of the source** (`Print_Area`, `Print_Titles`, `_FilterDatabase`,
  others) are duplicated with `localSheetId` = the copy's position and their references to
  the source retargeted to the copy. POI does not copy them. Workbook-scope names that refer
  to the source are **not** duplicated (Q4) *(u: Excel may create local copies)*.
- Hyperlink `location`s on the copy that point into the source are kept (navigation targets).
- **Elsewhere**, nothing refers to the new sheet yet, except 3-D spans that now include it
  (inserted between endpoints) and unknown-sheet references that now resolve: ADR-0009 §4's
  seeds.

#### 4.5 Workbook-level changes

As ADR-0009 §4 (add a sheet): the `<sheet>` element at `position`, `localSheetId` /
`activeTab` / `firstSheet` renumbered, `app.xml` (§11 there), Phase1 seeds (3-D spans,
`SHEET()`/`SHEETS()`, every `Everything` formula, newly resolving names), `fullCalcOnLoad`.
The copy's formula cells enter the census by the rebuild; `calcChain.xml` is removed because
the set of formula cells changed (ADR-0003 §4.3). Cached values in the copy are kept except for
the seeds above and for formulas that call `SHEET`, `CELL` or `INFO`.

The source sheet is not touched (L1).

### 5. Rename a table

`rename_table(old, new)`:

- **Validation**: ADR-0017 §6.2's name rules and uniqueness against tables and defined names
  (`InvalidTableName`, `NameInUse`). A case-only change is allowed. Tables of type
  `queryTable`/`xml` are refused *(u: whether their query or map parts name the table; L57)*.
- **Writes**: `table/@name` and `@displayName` (the table part, staged);
- **Rewrites**, in every `Formula` place of §2.2 (cells on every sheet, names, CF, DV, x14,
  sparklines, table formulas of every table, chart `c:f`, `textlink`, connection
  parameters):
  - a structured-reference token whose table name folds equal to `old` (ADR-0003 §2.4): only
    the table-name bytes change, the bracket body is copied;
  - a bare name token equal to `old` (`=ROWS(Sales)`): it can only be the table, because
    tables and defined names share one namespace (ADR-0017 §6.2);
  - `worksheetSource/@name` of pivot caches (3 in 3 files).
- **Not rewritten** (documented): `INDIRECT("Sales[Q1]")` strings, external `[1]Sales[…]`
  references, VBA.
- **Closed-world scan** (§2.2) for `old[` and bare `old` tokens outside registry places;
  a formula that does not lex and mentions `old` refuses.
- **Phase1**: values do not change, but `Everything` formulas are seeded (a string that names
  the table no longer resolves), as for a sheet rename (ADR-0009 §6).

Microsoft documents that Excel rewrites "all structured references" on a rename (§Reference
libraries); POI rewrites none.

### 6. Rename a table column

`rename_table_column(table, old, new)`:

- **Validation**: as ADR-0017 §6.3: unique case-insensitively in the table
  (`DuplicateColumnName`), not empty, no control characters other than tab/LF/CR, encoded as
  ST_Xstring in `tableColumn/@name`. Refused for `queryTable`/`xml` tables and for tables
  that are a pivot or slicer source (`TableHeaderCell`, ADR-0017 §6.3 m6).
- **Writes**: `tableColumn/@name` and the header cell through the `CellWritePlan` (Phase5's
  header sync; the header text is the decoded name). A table without a header row writes only
  the attribute.
- **Rewrites**: every column element of every structured reference that resolves to this table
  (tokens naming the table, and table-less tokens in formulas inside the table's range: its
  cells, calculated columns, totals), including both endpoints of column ranges and the
  `[@Col]`/`[@[Col x]]` forms. Each rewritten element is re-rendered with ADR-0017 §6.1's
  `specifier()` (escapes, double brackets), the rest of the token copied. A
  structured-reference token that the sub-grammar cannot parse refuses (`ColumnInUse`), as in
  ADR-0017 §6.3.
- `set_string` on a header cell keeps refusing with `ColumnInUse` when the column is
  referenced: an implicit formula rewrite on a value write would be a surprise (Q6). The
  caller uses `rename_table_column` instead.

### 7. Interaction with Phase1–5

| Phase | Effect |
|---|---|
| Phase1 (ADR-0003) | Translation (§4.2) re-anchors shared members, CF/DV formulas and hand-offs; the calcChain policy removes the chain; seeds per §3.7, §4.5, §5. Rebuild after apply (ADR-0009 §2). |
| Phase2 (ADR-0006) | `sst/@count` on deletes; header cells of new table columns through the string path. |
| Phase3 (ADR-0009) | The registry replaces the scan's hard-coded `allowed()` and lets rename/delete rewrite DV, CF, x14, table formulas and connection parameters, which the implementation refuses today. Copy reuses §3 and §4 there. |
| Phase4 (ADR-0014) | Every range feature is a registry row; the allowed-diff data table gains Phase7's rows; the container rule (remove when empty) applies. |
| Phase5 (ADR-0017) | Tables (§6.2 names, §6.4 removal), notes (§4), threads (§5) and allocators (§2.2) are reused. Notes depend on P5c. |

### 8. Errors

New variants:

| Error | When |
|---|---|
| `ShiftOffSheet { what }` | an insert would push content or a reference past the last row or column (§3.2) |
| `ArrayFormulaSplit { cell }` | an edit splits a multi-cell array, data table or spill range (§3.3.3) |
| `TableStructure { table, reason }` | header or totals row, every data row, or the last column of a table deleted (§3.6.1) |
| `ObjectInDeletedRange { part, what }` | a drawing or VML anchor in deleted rows or columns (§3.6.3) |
| `ReferenceDeleted { part, location }` | a reference that cannot become `#REF!` would be deleted: chart series, pivot source, data-table input, `singleXmlCell` |
| `StructureBlocked { part, place }` | a registry place marked X/XX (§2.2), a pivot crossed, an autofilter header or filtered column deleted, a protected range emptied |

Reused: `UnrewritableReference`, `MissingReference`, `PartNotEditable`, `SharedWorkbook`,
`BinaryReferencesPossible`, `SheetHasUnsupportedParts`, `InvalidSheetName`,
`DuplicateSheetName`, `InvalidTableName`, `NameInUse`, `DuplicateColumnName`, `ColumnInUse`,
`TableInUse`, `TableHeaderCell`, `NoSuchTable`, `NoSuchSheet`, `NotAWorksheet`, `Poisoned`,
`RegistryRefused`.

## Test oracles

1. **Locality** (byte-exact, ADR-0014 §2, data table in `tests/allowed_diff.rs`):

   | Operation | Edited sheet | Other parts |
   |---|---|---|
   | insert/delete rows or columns | `row/@r`, `c/@r` at or after `p`; removed `row`/`c`; `row/@spans` (column edits); changed `f` text and `f/@ref`/`@r1`/`@r2`; seeded `v`/`is`/`t`; `dimension`; `cols/col`; `sheetView` descendants; `mergeCells`; `hyperlinks`; `autoFilter`; CF/DV blocks and their `extLst` content; breaks; `ignoredErrors`; `protectedRanges`; `customSheetViews` | other sheets: changed `f` text, seeded `v`/`is`/`t`, `hyperlink/@location`; `workbook.xml`: `definedName` texts, removed names, `calcPr`; `calcChain.xml` removed (+ rel, CT); `sharedStrings.xml` `count`; tables (`ref`, `autoFilter`, `tableColumn`s); comments, VML, threaded comments (rows, anchors, removed notes); drawings (markers); charts (`c:f` text only); pivot caches (`worksheetSource/@ref`); `singleXmlCells`; `connections.xml` (`parameter/@cell`); removed tables with their rels and CT |
   | copy sheet | — (source is L1) | new parts (any content); `workbook.xml` (`sheets`, `definedNames`, `bookViews`, `calcPr`); workbook `.rels`; CT; `app.xml`; calcChain removed; seeded caches on other sheets |
   | rename table | changed `f` texts on any sheet, seeded caches | the table part (`name`, `displayName`); every part with a rewritten formula place; pivot cache `worksheetSource/@name`; `workbook.xml` names and `calcPr` |
   | rename column | the header cell (Phase1 write composition), changed `f` texts | the table part (`tableColumn/@name`, `calculatedColumnFormula`, `totalsRowFormula`), parts with rewritten places, `sst` |

2. **Identity-tracking oracle** (independent of the rewriters). A synthetic fixture puts a
   unique value in every cell and one reference of every registry kind (every `Formula`,
   `Sqref`, `Range`, `Cell`, `Index`, `Marker` row of §2.2) pointing at known cells, on the
   edited sheet and on another. After an edit, a test parser written from §3.2's table (not
   from the implementation; its own small A1 parser in `tests/common`) checks for each
   reference that the set of *surviving* tagged cells it covers equals the set it covered
   before, that it covers no other tagged cell, and that it is `#REF!`/removed exactly when
   every tagged cell it covered was deleted. Generated for every registry row: a test fails
   if a row has no fixture case.
3. **Insert-then-delete identity** (property test): `delete(insert(x, p, n), p, n)` gives
   every registry place its original value, for random `p`, `n` and both axes, on the
   synthetic fixture and the committed POI fixtures; only calcChain, `calcPr` and stripped
   caches may differ.
4. **Shared-group oracle**: after an edit, every member's own formula (expanded) equals §3.2
   applied to its own formula before, whatever the group structure became (§3.3.2).
5. **LibreOffice differential** (manually triggered, pinned, like `o3-recalc.yml` and the
   LibreOffice check of #82): the same edits run in LibreOffice headless on the fixtures and the
   formula texts, CF/DV ranges, merges and table ranges are compared semantically with ours.
   The expected differences are listed (LO's `ExpandRefs`, its handling of x14 and of pivots).
6. **Registry completeness**: the XSD-derived test, the ADR ↔ data test (§2.2), and a
   **residual scan** of the output: no reference to the edited sheet outside registry places
   (the closed-world scan run on the result).
7. **Corpus sweep**: for every readable POI package and worksheet: insert one row at index 1,
   delete row index 1, insert column index 1, delete column index 1, copy the sheet, rename
   every table and every table column. Each result either refuses with every byte unchanged
   or passes oracles 1, 4 and 6 and reopens; refusal reasons are counted and compared with the
   spike's blocker counts.
8. **Failure atomicity**: every refusal of §2.4, §3 and §8 has an `Err ⇒ byte-identical` test;
   fault injection (`Err` and panic) before each in-place step and in the rebuild poisons the
   workbook; an injected `StalePlan` leaves it usable and unchanged.
9. **Independent readers (CR)**: calamine 0.38 reads back values, merged regions and tables
   after each operation; values match the moved input values.
10. **Mutation checks**: on the range rule's boundary cases (`p == r1`, `p == r2 + 1`,
    deletes touching one endpoint), the shared-group step 3 comparison, the re-anchoring, the
    VML renumbering, and the `#REF!` prefix rule.
11. **Excel checks (XL, owner)**: open without repair and save back, for each operation on a
    fixture with formulas (relative, absolute, whole columns, 3-D, names), shared groups
    (including the neighbour-row case), CF/DV with relative formulas, merges, hyperlinks,
    autofilter, a table with totals and a calculated column, notes, a picture and a chart.
    Record what Excel writes for the *(u)* items:
    1. edge cases of §3.2 (insert at the first row, just below a range, full-axis ranges);
    2. the `#REF!` form of a deleted cell through a sheet prefix;
    3. 3-D references when one sheet of the span changes;
    4. structured references to a deleted column;
    5. a spill range crossed by an edit;
    6. CF/DV re-anchoring when the first cell is deleted;
    7. `Print_Area`/`Print_Titles` wholly deleted;
    8. frozen panes above an insert;
    9. new table rows (calculated columns filled?) and new table columns (names);
    10. a chart series wholly deleted;
    11. duplicated `xr:uid` in a copy;
    12. self-prefixed formulas and charts in a copy; workbook names in a copy.

## Delivery

A prerequisite PR and four PRs:

0. **P7-pre** (no new public API): the location registry as data (`locations.rs`), its ADR ↔
   data and XSD-completeness tests; the closed-world scan reading it; ADR-0009's rename and
   delete extended to the registry's formula rows (DV, CF, x14, table formulas, connection
   parameters), removing the refusals of the ADR-0009 implementation note; the structured
   reference sub-grammar moved to `formula/` with token spans; `shift_area` and the rewriters
   of §2.3 with unit tests. Needed by every later PR.
1. **P7a**: insert/delete rows and columns for cells, formulas on every sheet, names, shared,
   array and data-table formulas, `dimension`, `cols`, views, breaks, merges, hyperlinks,
   autofilter, CF/DV (2007 and x14), sparklines, `ignoredErrors`, `protectedRanges`,
   `customSheetViews`, calcChain and Phase1/Phase2 (§3.1–§3.5, §3.7), the benchmark (§3.8).
   Objects that would change are refused (`StructureBlocked`) until P7b.
2. **P7b** (stacked on P7a; notes after P5c): tables, notes, threaded comments, drawing and
   VML anchors, charts, pivot caches, `singleXmlCells`, connections (§3.6).
3. **P7c** (after P7-pre, in parallel with P7a): table rename and column rename (§5, §6).
4. **P7d** (after P7-pre and P5c): copy sheet (§4).

## Alternatives considered

1. **Stage clones of every worksheet** (ADR-0009 §2): simpler failure model, but a G1 edit on
   a large sheet would clone its whole tree (289 ms and 2× memory for 200k formulas,
   ADR-0003). Rejected for worksheet parts; kept for small parts.
2. **Rewrite by regular expression over raw XML** (no lexer): fast, but cannot tell a string
   literal or a name from a reference, and cannot shrink a range. Rejected; the lexer exists.
3. **POI's model (a delete is a move onto the deleted rows)**: no notion of shrinking, and
   it leaves DV, autofilters, tables, pivots and chart series stale. Rejected.
4. **Refuse every sheet with a shared formula group** instead of splitting: 48 files have
   groups, and 98% of simulated edits keep them whole. Rejected.
5. **Excel-style `#REF!` for every lost reference** (charts, pivots, names, structured
   references): matches Excel where known, but leaves objects that Excel may repair or
   refuse; their forms are not verified. Refused instead until the XL checks (§8
   `ReferenceDeleted`).
6. **Hand-written per-feature code without a registry** (POI, UM): each library misses a
   different set of places (DV, tables, pivots, charts). Rejected: the registry and its XSD
   test make a missing place a CI failure.
7. **One ADR per operation**: the three operations share the registry, the rewriters and the
   transaction; splitting would duplicate them. One ADR, four delivery PRs, as ADR-0017.

## Consequences

- Inserting and deleting rows and columns keeps every known reference correct across all
  parts, which none of the reference libraries does; places it cannot rewrite refuse instead
  of going stale.
- The registry also closes ADR-0009's rename/delete gaps (DV, CF, x14, table formulas).
- 2% of corpus worksheets refuse every G1 edit (pivots, controls, `dataConsolidate`) and 5%
  refuse copying; the 15 sheets without `r` refuse both.
- A G1 edit on a large sheet re-renders the open tags of every later row and cell; deferred
  rows (ADR-0010 §2) must allow a bulk renumber.
- Several behaviours depend on Excel checks (*u* items, Test oracles 11); the defaults refuse
  rather than guess wherever the result would be a stale or broken object.

## Open questions for the project owner

Each has a proposed default; the ADR is written to it.

**Need the owner's decision:**

- **Q1. Format of inserted rows and columns.** Default: none (blank, default style; a `col`
  range around the insertion is split, not stretched). Alternative: copy the row above / column
  to the left (`row` attributes, `col` attributes, cell styles), as Excel's default "Format
  Same As Above" is believed to do *(u)*.
- **Q2. Objects whose anchors fall in deleted rows or columns.** Default: refuse
  (`ObjectInDeletedRange`). Alternatives: delete them, or collapse them to zero size.
- **Q3. Copy: self-prefixed formulas and charts.** Default: formulas keep their text (still the
  source); charts are retargeted to the copy. Both are XL items.
- **Q4. Copy: workbook-scope names that refer to the source.** Default: not duplicated.
  Alternative: duplicate as names local to the copy.
- **Q7. Pivot caches whose source range would grow or shrink.** Default: refuse. Alternative:
  rewrite the `ref` and set `refreshOnLoad="1"`.
- **Q10. Performance target** for P7a's benchmark (insert one row at row 2 of a 1M-cell
  sheet, then save). Proposed: within 2× of open + save of the same file.
- **Q11. Inserting columns inside a table.** Default: add `Column{n}` columns as Excel does.
  Alternative: refuse until an Excel check.

**Defaulted (owner may override):**

- **Q5. Names of copied tables**: source name without trailing digits + smallest free `n ≥ 2`.
- **Q6. Header-cell writes on referenced columns**: keep `ColumnInUse`;
  `rename_table_column` is the rewriting path.
- **Q8. Chart series wholly deleted**: refuse.
- **Q9. Shared groups that cannot stay whole**: split by writing members' own formulas.
- **Q12. `_xlnm.Print_Area`/`Print_Titles` wholly deleted**: drop the union member; remove
  the name when empty.

## Sources

- ECMA-376 Part 1 (5th ed.): §18.3.1 (worksheet elements: `c`, `row`, `f`, `dimension`,
  `cols`, `mergeCell`, `hyperlink`, `conditionalFormatting`, `dataValidation`,
  `selection`, `pane`, breaks; `row/@spans` §18.3.1.73), §18.5 (tables), §18.6 (calcChain), §18.7
  (comments), §18.10 (pivots), §18.2.5 (`definedName`); `sml.xsd` in
  `reference/ecma-376-transitional-xsd/` (the 56 typed places of §Context).
- Apache POI `47dcff769bb395034dddfd59c4609659b6dbc6a4`:
  `poi/src/main/java/org/apache/poi/ss/formula/FormulaShifter.java` L109-133, L187-231,
  L356-505, L793-813; `poi-ooxml/src/main/java/org/apache/poi/xssf/usermodel/`
  `XSSFSheet.java` L2032-2063, L3095-3197, L3273-3338, L4494-4503, L5107-5196;
  `helpers/XSSFRowColShifter.java` L60-295; `XSSFWorkbook.java` L691-797, L815;
  `XSSFTable.java` L381-443, L811-856; `XSSFTableColumn.java` L100-102;
  `poi/src/main/java/org/apache/poi/ss/usermodel/helpers/RowShifter.java` L54-119.
- LibreOffice core `27135b9594396e32eb88212df87d586780a55e69`:
  `sc/source/core/tool/token.cxx` L2809-3129, L3179-3420, L3790-3869;
  `sc/source/core/tool/refupdat.cxx` L27-67, L133-155, L187-282;
  `sc/source/ui/operation/InsertCellsOperation.cxx` L365-371, L410-433, L747-748;
  `sc/source/ui/operation/DeleteCellsOperation.cxx` L283-288, L316-335;
  `sc/source/core/data/column.cxx` L663-690; `sc/inc/globstr.hrc` L138-139, L235, L383, L528.
- openpyxl 3.1.5 (PyPI sdist): `openpyxl/worksheet/worksheet.py` L689-816;
  `openpyxl/formula/translate.py` L18-71; `openpyxl/workbook/workbook.py` L397-414;
  `openpyxl/worksheet/copier.py` L11-61.
- umya-spreadsheet 3.1.1 (crates.io): `src/structs/workbook.rs` L69-121, L1088-1139;
  `src/structs/worksheet.rs` L2916-3196; `src/helper/formula.rs` L930-1061;
  `src/helper/coordinate.rs` L335-341; `src/structs/defined_name.rs` L260-305.
- Microsoft Support: "Using structured references with Excel tables"; "How to correct a #REF!
  error"; "Rules for changing array formulas"; "Create a 3-D reference to the same cell range
  on multiple worksheets"; KB 305568 (not fetched; *u*).
- ooxml-rs at `origin/main` 2026-11-20: `crates/ooxml-xlsx/src/formula/lexer.rs`,
  `formula/translate.rs`, `sheet_ops/{mod,names,scan}.rs`, `tables/references.rs`,
  `index.rs`; `crates/ooxml-core/src/xml/write.rs`, `package/add.rs` (`RegistryPlan`).
- Corpus: POI `test-data/spreadsheet` at the POI commit above; calamine fixtures in
  `reference/fixtures/xlsx/calamine/`; synthetic fixtures in
  `reference/fixtures/xlsx/synthetic/`.
