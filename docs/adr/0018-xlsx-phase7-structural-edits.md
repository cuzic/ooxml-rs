# ADR-0018: xlsx Phase7 — structural edits

- Status: **Accepted** (revision 4; Opus review rounds 1–4, owner decisions 2026-11-20)
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
- Review: [round 1](0018-opus-review-round1.md), [round 2](0018-opus-review-round2.md), [round 3](0018-opus-review-round3.md)

## Revision history

- **Revision 1**: initial draft.
- **Revision 2** (review round 1: B1, M1–M5, m1–m14, n1–n7; owner decisions Q1–Q4, Q7, Q10,
  Q11, Q13 on 2026-11-20; Q12 made fail-closed). Each fix is in its section.
  - **B1:** CF, DV, x14, sparkline and table formulas on **other** sheets that refer to the
    edited sheet are rewritten. Registry rows carry a scope (`own` / `any`). The dispatcher
    walks every part that holds a row. Apply step (2), the allowed-diff rows and oracle 2 cover
    them. Oracle 7 checks every formula place on every sheet. A sparkline whose data range is
    wholly deleted is refused (§2.2, §2.1, §3.5, Test oracles).
  - **M1:** a range end at row 1,048,576 or column XFD (including `col max="16384"`) stays
    pinned to the edge, as LibreOffice does (`refupdat.cxx` L270-276; `token.cxx` L2956,
    L2967, L3044). A range end pushed past the edge is clipped (`lcl_MoveEnd`, L49-67). Only a
    single cell or a range start pushed off the sheet refuses. The off-sheet content refusal
    applies to `row`/`c` and to objects with content, not to `cols`, `dimension`,
    `selection`, `sqref`s or `ignoredErrors` (§3.2, §3.4).
  - **M2:** one rule. Any structural edit strips the caches of every formula whose references
    touch the edited sheet, directly or through names or tables, plus the position-dependent
    and `Everything` formulas, and sets `fullCalcOnLoad` (§3.7). This also answers m8: moved
    cells are no longer a separate seed.
  - **M3:**
    - The in-place steps work on **pre-edit indices in one fixed order** that is validated
      before apply: strips, formula texts, `r` renumbering, removals in descending order,
      insertions last (§2.1).
    - The `CellWritePlan` composes with a structural edit through **one combined
      `RegistryPlan`** and an index map (§2.1.1), which covers table column inserts (Q11) and
      column renames.
    - `sst/@count` goes through Phase2's `RegistryApply`, not a staged clone.
    - Fault injection sits between every step (oracle 8).
  - **M4:**
    - The completeness scan is defined by element. Unprefixed references are recognised only
      in registry places, i.e. places that the schemas type as references or formulas.
    - Extension content is covered by an `EXTENSIONS` allowlist of URIs. An unknown URI with
      reference-shaped content in a part the operation edits refuses.
    - GUID-like values (`xr:uid`, `x14:id`, `{…}`) are never scanned.
    - The x14 `protectedRanges`/`ignoredErrors` and `webExtensions` places are added (§2.2).
  - **M5:** `copy_sheet` drops `sheetPr/@codeName`, as ADR-0009 does for new sheets (§4.2;
    XL item 13).
  - **Minor and nit fixes, each in its section:**
    - m1: blockers are recounted from the XX set (787 of 813), and `scenarios` and
      `webPublishItems` are XX;
    - m2: the all-positions simulation with `#REF!` equality gives the same 286/6, and
      `#REF!` compares equal (§3.3.2);
    - m3: a hyperlink `location` that points into deleted cells refuses;
    - m4: a multi-range `sqref` whose relative formula needs re-anchoring refuses;
    - m5: rows L62–L67 and `NOT_APPLICABLE` entries for the remaining sml.xsd places;
    - m6: allowed-diff rows;
    - m7: `preserve_attribute_spelling`, and a bulk-removal lens API in P7-pre;
    - m9: Q12 is fail-closed;
    - m10: Q13;
    - m11: dependencies on P5c, P5d and P5e are stated, and the drawing/chart closed set for
      copies;
    - m12: VML anchors are handled for notes only;
    - m13: data-model table sources refuse, and L55 is rewritten on renames;
    - m14: a merge anchor hand-off onto a non-empty cell refuses;
    - n1–n7.
  - **Declined:**
    - m4's alternative (bounding-box anchor) is not adopted: the refusal covers both readings
      until XL item 6.
    - The reviewer's Q11 recommendation (refuse column inserts in tables) is superseded by the
      owner's decision; §2.1.1 specifies the composition instead.
    - The m2 suggestion to split groups into two at `p`, rather than writing members out, is
      not adopted. Writing members out is simpler and affects only 6 corpus groups.

- **Revision 3** (review round 2: M1–M3, m1–m8, n1–n2). Each fix is in its section.
  - **M1:** every `Formula`/`Location` row has scope `any`, now including L30, L42, L48, L56
    and L70 (form controls, chartEx, ctrlProps, webExtensions). A compile-time check on
    `LOCATIONS` enforces it. A sheet-prefix hit is accepted only in a place that the
    dispatcher visited for this operation with a rule other than `–`; a hit anywhere else
    refuses. Oracle 2 gets cross-sheet control cases (§2.2).
  - **M2:** `EXTENSIONS` follows MS-XLSX §2.2.4.12:
    - ignoredErrors `{01252117-…}`, protectedRanges `{FC87AEE6-…}`; `{EB79DEF2-…}` dropped;
    - the URIs of webExtensions, slicerList and timelineRefs added;
    - the `sortState` and `filterColumn` extensions added.

    Content under a **known** URI that matches no registry place of that URI refuses too,
    with a unit case for a swapped mapping (§2.2).
  - **M3:** the registry is a superset of ADR-0009 §6's table:
    - L71 adds internal relationship targets `#Sheet!A1`, L72 the `@sheet` raw names and L73
      the `app.xml` titles;
    - two rule columns, sheet rename and sheet delete, hold ADR-0009's current behaviour;
    - P7-pre's acceptance is byte-identical ADR-0009 rename and delete output before and
      after the switch, over the corpus, including the 18 of 838 renames that retarget
      relationships (§2.2, Delivery).
  - **Minor and nit fixes, each in its section:**
    - m1: a wholly deleted range wins over pinning; view cells are clamped on insert (§3.2);
    - m2: moved formulas on the edited sheet are lexed for position-dependent calls; renames
      strip only `Everything`, in §3.7 and §5; a copy's strips are stated as an
      over-approximation;
    - m3: the apply invariant is restated, and the `cols` split moves to step 3.5;
    - m4: what P5-pre already provides on main is listed, and there is one staged
      `workbook.xml` per plan (§2.1.1, Delivery);
    - m5: the multi-`sqref` refusal covers either anchor (§3.3.4);
    - m6: `mc:AlternateContent` inside a registry element refuses (L31);
    - m7: §8 wording for Q2; partial-delete rules for one-cell and note anchors (§3.6.3);
    - m8: `lockStructure` applies to ADR-0009's sheet operations too; header writes on
      protected sheets (§2.4, §6, Consequences, Delivery);
    - n1: L42's corpus count is 0; the 57 are `x:FmlaMacro`;
    - n2: MS-XLSX is cited.

- **Revision 4** (review round 3: M1, M2; m1, n1).
  - **M1:** a sheet-prefix hit is accepted when its `(row, part)` was **dispatched**, whatever
    the rule; only X/XX refuse. Rows that hold the sheet name but do not change under G1 get
    the new rule **K** (dispatched, kept): L49 (pivot-chart source), L60 (external links) and
    L73 (`app.xml` titles). The known non-references are now `scan.rs`'s set, copied verbatim
    with file and line. Corpus effect: 0 additional G1 refusals (§2.2; Context; oracle 7).
  - **M2:** P7-pre's acceptance is split into four clauses (Delivery 0):
    1. byte-identical output outside `lockStructure` workbooks;
    2. `Protected` with bytes unchanged in the 4 `lockStructure` workbooks (35 sheets);
    3. today's refusals that become rewrites, checked per place;
    4. 0 refusals other than `Protected` over the 838 renames.
- **Accepted** (review round 4: Accept, nits n1–n2 applied): status set to Accepted on 2026-11-20.

    "Nothing else changes" is corrected to name both intended changes (§2.2), and the
    `lockStructure` change is recorded as a post-acceptance note for ADR-0009.
  - **m1:** L32 and L73 are marked `(any)`, so the compile-time scope check holds for every
    `Formula`/`Location` row without exemptions.
  - **n1:** the Context table records how many sheets `app.xml` titles name in prefix form.

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
| Shared-group simulation | One row inserted (or deleted) in the middle of each of the 292 groups spanning ≥ 3 rows, and separately **at every position** from the lowest referenced row to one past the last member, with `#REF!` comparing equal: **286 stay one group, 6 (4 files) must be split** in both runs (members that refer to a neighbouring row: `B5-B4`, `+A4+1`). |
| CF / DV | CF 108 blocks (20 files), 131 rules, 11 multi-range `sqref`s; **27 CF formulas with relative references** (8). DV 108 (13), **44 with relative references** (3). x14 CF in 2 files, x14 DV in 3, sparklines 11 (1). |
| Range features | merges 54,468 (43 files); hyperlinks 113 (15), `location` 38 (3); sheet autofilter 3, `sortState` 7; selections 481, frozen panes 32 (19); `cols` 361 sheets (179 files), 106 `col` up to XFD (44); row breaks 10 (4 files), column breaks 3 (3); `ignoredErrors` 5, `protectedRanges` 4, `customSheetViews` 3 (1), `dataConsolidate` 1, `scenarios`/`cellWatches`/`webPublishItems` 0. |
| Objects | notes 204 (27 files) with 204 VML `x:Row`/`x:Column`; VML `x:Anchor` 410 (37); VML form controls 166 and `x:Fmla*` 57 (4; all `x:FmlaMacro` macro names, none position-bearing); threaded comments 4 (3); drawing anchors: `twoCellAnchor` 181 (`editAs` twoCell 102, oneCell 74, absolute 5), `oneCellAnchor` 30, **170 anchors inside `mc:AlternateContent` (4 files)**; tables 61 (29 files): 9 with totals, 13 `queryTable` + 5 `xml`; charts 40 (27 files), 168 sheet-prefixed `c:f`; pivot tables 12 on 12 sheets (7 files), pivot caches 10 (7 sourced by `ref`, 3 by table name); `singleXmlCells` 14 parts (14 files); `calcChain.xml` 116 packages (30,875 entries). |
| `mc:AlternateContent` in sheets | at the top level 2 sheets (both wrap `controls`); **inside `sheetData` or rows: 0**. |
| Blockers | G1: **787 of 813 worksheets (97%)** have none of the registry's XX places (rows/cells without `r` 15 sheets, `oleObjects` 6, `controls`/top-level AC 2, `dataConsolidate` 1). Pivot tables (12 sheets) refuse only edits that cross or move them. G2: **773 of 813 (95%)** have only copyable relationships (§4.1). |
| Protection, `codeName` | 11 protected sheets (5 files), 7 of them (4 files) lock row and column inserts and deletes; `lockStructure` in 4 workbooks (Q13). `sheetPr/@codeName` on 93 sheets (32 files), 33 of them in the 8 packages with a VBA project (§4.2). |
| Sheets named in `app.xml` titles | **25 sheets in 15 packages** are named in prefix form in `docProps/app.xml` titles ("Named Ranges": `Data!Print_Area`); the round-3 review's probe counted 31 / 15 over a different file set (it included a fuzzing case outside the spike's 343 readable packages, 7 sheets, and missed one package the spike reads, 1 sheet: 31 − 7 + 1 = 25; per file the counts agree). The places are L73 (rule K), so they cause no refusal. |
| Extension URIs in worksheets | `mac/excel/2008` 29 and `{64002731-…}` 20 (Mac page layout only), x14 CF 2, x14 DV 3, sparklines 1, cfRule `x14:id` link `{B025F937-…}` 2; no other. |
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
4. **Shared groups survive structural edits** in 98% of the simulated groups, at every edit
   position; the rest are neighbour-row formulas whose members must be written out (§3.3.2).
5. **Excel writes both `#REF!` forms**: bare in 335 cell texts and sheet-prefixed in 15 (1 cell,
   13 names, 1 x14 formula). ADR-0009 measured that a *deleted sheet* gives the bare form; a deleted *cell*
   reached through a sheet prefix gives the prefixed form (§3.2, an XL item).
6. Pivot tables, controls and AC-wrapped content are each in ≤ 7 files: refusing them costs
   3% of sheets for G1 and 5% for copies.
7. **Ranges that end at the sheet edge are common** (106 `col` elements up to XFD in 44
   files): they must stay pinned to the edge, not refuse (§3.2).

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

1. **Validate (read-only).**
   - Parse every part that holds a registry location (§2.2) the operation can reach. That
     includes **every** worksheet, table, chart and drawing part for places of scope `any`
     (review B1).
   - For each occurrence, compute the new value, a removal, or a refusal.
   - Run the completeness scan (§2.2). Build every new or changed small part. Compute the
     Phase1 strip set (§3.7) and the Phase2 count deltas.
   - **Every refusal happens here**, so `Err` leaves every byte unchanged.
   - All positions are recorded as **pre-edit child indices** of the tree as it is now. The
     in-place order below is what keeps them valid (review M3).
2. **Registry plan.** One `RegistryPlan` (PR #38) for the whole operation, containing:
   - `stage_part` for every changed **small** part: `workbook.xml` (names, `calcPr`), tables,
     comments, VML, threaded comments, drawings, pivot cache definitions, `singleXmlCells`,
     `connections.xml`, `app.xml` (only when ADR-0009 §11 requires it). Charts are spliced at
     the byte level, as ADR-0009's implementation does (`sheet_ops/mod.rs` L982-1038), so
     only `c:f` text changes;
   - `remove_parts` for `calcChain.xml` (ADR-0003 §4.3), for removed tables (ADR-0017 §6.4),
     and for a comments/VML pair whose last note goes (with its relationships and CT);
   - for G2, `add_part`, `add_relationship` and content types for the copy (§4);
   - **Phase2's `RegistryApply`** for `sst`: `refs_delta` from the deleted and stripped `t="s"`
     cells and the appends of any composed `CellWritePlan` (§2.1.1). It applies in place like
     Phase1's (`edit.rs` L438-443); `sharedStrings.xml` is not staged as a clone (review M3).
3. **Apply.** The workbook is poisoned and `Package::apply(plan)` runs. Its only error,
   `StalePlan`, comes before any change and clears the poison (ADR-0017 §2.1). Then the
   **worksheet parts are edited in place**, every sheet in the same fixed order of steps:
   1. **strips**: the Phase1 cache strips (§3.7), at pre-edit indices (`strip_cells`,
      `edit.rs` L129-137, unchanged);
   2. **text and attribute rewrites at existing nodes**:
      - formula texts and `f/@ref`/`@r1`/`@r2`;
      - every rewritten place of §2.2 that lives in the sheet part: CF/DV blocks and their
        `extLst` content, sparklines, merges, hyperlinks, views, breaks, `autoFilter`,
        `dimension`, attribute edits of existing `col` elements;
      - on **other** sheets too (B1);
   3. **renumbering**: `row/@r`, `c/@r`, `row/@spans` of surviving nodes;
   4. **removals** of rows, cells and range-feature elements (merges, rules, hyperlinks,
      breaks, sparklines, emptied containers). They run in **descending** index order per
      parent, or as one bulk retain per parent (§3.8), so that no earlier removal shifts a
      later index;
   5. **insertions** last:
      - new cells of a composed `CellWritePlan` (§2.1.1);
      - the second `col` element of a `cols` split (§3.4);
      - other new elements (a new `tablePart` is not needed: table parts are staged).

   **Invariant** (review round 2 m3): until step 4, no child is added to or removed from any
   element whose children are addressed by a recorded index (`sheetData`, its `row`s, and the
   containers of range features that step 4 removes from). Steps 1–3 do change children
   elsewhere: strips remove `v`/`is` inside a `c`, and a shared member leaving its group gains
   `f` text. Those are not addressed by index. Step 4 removes in descending order. Step 5
   uses positions that §2.1.1 and §3.4 compute in post-edit coordinates.

   The census, dependency graph, row index and Phase2 registries are then **rebuilt** from
   the output parts, as ADR-0009 §2 does after a sheet operation. The poison is cleared after
   the rebuild.

Worksheet parts are edited in place rather than staged as clones (ADR-0009 §2 stages clones)
for two reasons:

- A G1 edit touches nearly every node of a large sheet.
- A staged clone of a 200k-formula sheet costs 289 ms (ADR-0003, measured) and doubles the
  tree's memory (1.44 GiB for 1M cells, ADR-0010 Context).

This is the same trade ADR-0017 §2.1 made for the `CellWritePlan`.

In-place steps cannot refuse. Their inputs (indices, texts, attribute values) are computed
and checked in step 1, against the same tree they are applied to, in the order above. A
failure there is a bug and leaves the workbook poisoned.

**Fault injection** (`Err` and panic, `catch_unwind`) is tested:

- before each of steps 3.1–3.5 and inside the rebuild, on a fixture where strips, rewrites,
  renumbering, removals and insertions all touch the same sheet;
- with an injected `StalePlan`.

(ADR-0003 §7.3, ADR-0017 §2.1; review M3.)

##### 2.1.1 Composing with the `CellWritePlan`

Two operations also write cells:

- inserting columns inside a table, which writes the header cells of the new columns (Q11,
  owner decision);
- renaming a table column (§6), which writes one header cell.

Each operation still builds **one** plan. The cell writes are not a second transaction:

**Already on main** (review round 2 m4):

- `apply_writes_with` and `apply_writes_with_insertion` (`edit.rs` L470-489, used by
  `tables.rs` L468, L661) fold a `CellWritePlan`'s `calcPr` change and calcChain removal into
  the caller's `RegistryPlan`.
- Translation copies `#REF!`: the prefixed form is `Area::RefError` (`translate.rs` L47,
  tested at L102), and the bare form is an error literal.

What P7-pre adds:

- `plan_writes` (ADR-0003 §3.1a) gets a variant `plan_writes_after(edit, writes)`. It takes
  the operation's **index map**, which maps pre-edit to post-edit rows and cells and is
  computed from the structural edit in step 1. It validates the writes against the post-edit
  view:
  - the target cells of new columns do not exist before the edit, so they are planned as
    insertions at their post-edit positions;
  - the census checks use the post-edit positions.
- Its outputs are **merged** into the operation's plan instead of carrying their own:
  - its `sst` appends and `refs_delta` go into the operation's single `RegistryApply`;
  - its `calcPr` change goes into the operation's single staged `workbook.xml`: **one staged
    clone per plan**, carrying the names and the `calcPr`. Today `apply_writes_with` stages
    the `CellWritePlan`'s own clone (`edit.rs` L479-481), which would stage `workbook.xml`
    twice. P7-pre makes it edit the operation's clone instead;
  - its calcChain removal is the operation's (one `remove_parts`);
  - its dependent strips are **subsumed** by §3.7's rule, which is computed on the pre-edit
    graph. The composed plan does not ask the pre-edit graph about post-edit cells;
  - its cell writes become step 3.5 insertions or step 3.2 rewrites.
- A rename does not move anything (the index map is the identity), so §6 uses
  `plan_writes_after` with the identity map and the same merging.

This is a P7-pre change to the P5-pre code (`edit.rs` L106-116: `CellWritePlan` gives up its
own `staged`, `chain` and `registry` when composed).

#### 2.2 The location registry

One table lists **every place in a package that can hold a cell position, a range or a
formula**, with its syntax and its rule for each operation. It extends ADR-0009 §6's table
from sheet names to positions. It exists twice, and a test keeps the two equal:

- in this ADR (below), for review;
- as data: `crates/ooxml-xlsx/src/locations.rs`, a `const LOCATIONS: &[Location]` with
  `{ id, part: PartKind, scope, path: &[(ns, local)], attr: Option<&str>, syntax, g1, copy, rename }`.

**Scope** (review B1). Every row whose syntax can hold a sheet-prefixed reference (`Formula`,
`Location`) is **`any`**: the place is rewritten wherever it is, on the edited sheet, on any
other sheet or in any other part, for the tokens that name the edited sheet. Unprefixed tokens
refer to the sheet that owns the place (§2.3), so they matter only on the edited sheet. The
other syntaxes (`Sqref`, `Range`, `Cell`, `Index`, `Marker`) are **`own`**: they are positions
on the sheet that owns them, and only the edited sheet's (and its tables', notes', drawings')
are rewritten. In the table, "(any)" after the part marks scope `any`.

The code and the tests use the data:

- **Dispatch.** Each operation walks `LOCATIONS` and, for every row, every part that can hold
  it: for scope `any`, every part of the row's part kind in the package; for `own`, the edited
  sheet and the parts it owns. It calls the rewriter for the row's syntax (§2.3). Every row
  has, per operation, exactly one of the rules defined below; a missing rule fails the
  ADR ↔ data test (review n4).
- **Completeness scan** (review M4). It replaces ADR-0009's scan (`sheet_ops/scan.rs`
  L54-73, a hand-written `match`). It runs on every XML part the operation does not remove.
  It is defined **by element**, not by searching text for cell-shaped tokens:
  - **Sheet-prefix form** (ADR-0009 §6, unchanged and proven: 0 refusals in 838 renames).
    Every text node and attribute value is searched for the edited sheet's name in prefix
    form (or, for table renames, `Name[`). A hit is accepted iff it lies:
    - in a registry place whose `(row, part)` is in the operation's **dispatch set**,
      **whatever the rule** (R, D, N, K, –); or
    - in a known non-reference.

    A hit in a dispatched place whose rule is X or XX applies that rule (X refuses only when the place would change). A hit
    anywhere else, including a registry place that was not dispatched, refuses with
    `Err(UnrewritableReference)` (review round 2 M1, round 3 M1). Rows that can hold a
    sheet name in prefix form but do not change under an operation are dispatched with rule
    **K** for it, not left out. In the corpus this rule adds **0** G1 refusals:
    - the 25 sheets named in `app.xml` titles hit L73 (K);
    - the pivot-chart source name hits L49 (K);
    - external links hit L60 (K) or are non-references.
  - **Unprefixed positions** are recognised **only in registry places**: places that the
    schemas type as references or formulas (`ST_Ref`, `ST_CellRef`, `ST_Sqref`, `ST_RefA`,
    `ST_Formula` and their derivatives in `sml.xsd`), plus the DrawingML, VML, chart and
    extension places listed in the table. Nothing else is tokenised. In particular, **values
    shaped like GUIDs** (`xr:uid`, `x14:id`, `xr:revisionPtr`, `{…}` values) and colours are
    never scanned.
  - **Extensions.** Every `ext/@uri`, at any depth, in a part the operation must change has
    to appear in the `EXTENSIONS` table below, as a URI with registry rows or as
    `NOT_APPLICABLE` with a reason.
    - **Reference-shaped content** means a sheet-prefix hit, or text matching the A1
      grammar in an element or attribute named `f`, `sqref`, `ref` or `r`.
    - It refuses (`UnrewritableReference { part, location: uri + path }`) whenever it lies
      under an `ext`:
      - of an **unknown** URI; or
      - of a **known** URI, at a path that is not a registry place of that URI (review
        round 2 M2).
    - So a swapped or incomplete URI entry fails closed, and so does a new
      reference-bearing child that Microsoft adds under an existing URI.
    - Content under any URI with no reference-shaped content is left alone.
  - Blind spots, as ADR-0009 §6: binary parts. An unparseable XML part gets a byte scan for the
    prefix form, and a hit refuses.
- **Schema completeness test.**
  - Every attribute and element that `sml.xsd` types as `ST_Ref`, `ST_CellRef`, `ST_Sqref`,
    `ST_RefA` or `ST_Formula` (56 today), and every element derived from `ST_Formula`, must
    map to a registry row or to an entry of an explicit `NOT_APPLICABLE` list with a reason.
    Examples of reasons: revision logs (the workbook is refused as shared), pivot-table
    internals (pivots are refused), `sheetPr/@syncRef` (a shared-workbook sync position:
    L58), `volatileDependencies` (`CT_VolTopicRef/@r`: real-time data topics, refused when
    the edited sheet has any; L65).
  - The test reads the bundled XSD, so a new schema place fails CI until someone decides its
    rule.
  - The bundled schemas do not include x14/x15/xr. Extension places are covered by the
    hand-written `EXTENSIONS` table and its own ADR ↔ data test.
- **ADR ↔ data tests**, as ADR-0014 R2-3: the ids, scopes, parts and paths in the tables below
  equal `LOCATIONS` and `EXTENSIONS`.
- **Scope check** (compile time, review round 2 M1): an inline `const { … }` over
  `LOCATIONS` asserts that every row of syntax `Formula` or `Location` has scope `any`. There
  are no exemptions: single-part rows (L32, L73) are marked `any` too (review round 3 m1).
- **ADR-0009 ↔ `LOCATIONS` test** (review round 2 M3): every place `sheet_ops/scan.rs`'s
  `allowed()` accepts today (`scan.rs` L54-72, including the `Relationships`/`Target` arm)
  maps to a row whose sheet-rename rule is R.

`EXTENSIONS` (from [MS-XLSX] §2.2.4.12 "Worksheet", the worksheet extension list, plus the
`cfRule`, `sortState` and `filterColumn` extensions on the same page; review round 2 M2):

| URI | Element | Rows / rule | Corpus |
|---|---|---|---|
| `{78C0D931-6437-407D-A8EE-F0AAD7539E65}` | x14 `conditionalFormattings` | L20 | 2 files |
| `{CCE6A557-97BC-4B89-ADB6-D9C93CAAB3DF}` | x14 `dataValidations` | L20 | 3 |
| `{05C60535-1F16-4FD2-B633-F4F36F0B64E0}` | x14 `sparklineGroups` | L21 | 1 |
| `{FC87AEE6-9EDD-4A0A-B7FB-166176984837}` | x14 `protectedRanges` (`xm:sqref`) | L68 | 0 |
| `{01252117-D84E-4E92-8308-4BE1C098FCBB}` | x14 `ignoredErrors` (`xm:sqref`) | L69 | 0 |
| `{F7C9EE02-42E1-4005-9D12-6889AFFD525C}` | x15 `webExtensions` (bindings with `xm:f`) | L70: X | 0 |
| `{A8765BA9-456A-4DAB-B4F3-ACF838C121DE}` | x14 `slicerList` | not applicable: relationships only (copies with slicers refuse, §4.1) | 0 |
| `{7E03D99C-DC04-49d9-9315-930204A7B6E9}` | x15 `timelineRefs` | not applicable: relationships only (copies with timelines refuse) | 0 |
| `{B025F937-C7B1-47D3-B67F-A62EFF666E3E}` (on `cfRule`) | `x14:id` link | not applicable: a GUID, no position | 2 |
| `{0FC57C36-975A-43D2-8416-536B9B585D1B}` (on `sortState`) | rich sort conditions (with `@ref`) | L74: X while it holds a `ref` | 0 |
| `{68452572-C921-441B-96FD-77FB3E787301}` (on `filterColumn`) | x14 filter extensions | L74: X while it holds a `ref` | 0 |
| `http://schemas.microsoft.com/office/mac/excel/2008/main`, `{64002731-A6B0-56B0-2670-7721B7C09600}` | `mx:PLV` (Mac page layout) | not applicable: no position | 19, 9 |

The URIs and element mappings are those of MS-XLSX §2.2.4.12 as fetched by the round-2
reviewer (learn.microsoft.com, page `07d607af-5618-4ca2-b683-6a78dc0d9627`, updated
2026-11-18). URIs compare case-insensitively. A unit test feeds `ignoredErrors` content under
its real URI, which must be rewritten (L69). It feeds the same content under a deliberately
swapped mapping, which must refuse.

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

Rules (columns G1, Copy, table/column rename, sheet rename, sheet delete), one per row and
operation (review n4). The two sheet columns hold ADR-0009 §5/§6's decided rules (review
round 2 M3). ADR-0009's implementation still refuses some places its table decides to rewrite:
DV, CF, x14, table formulas, connection parameters, form controls, `textlink` and
`c:pivotSource` (its implementation note). P7-pre makes exactly **two intended changes** to
ADR-0009's output:
1. those refusals become the R of this table;
2. under `workbookProtection/@lockStructure`, operations that succeed today refuse with
   `Protected` (Q13, §2.4).

Everything else, and every rename and delete that succeeds today in a workbook without
`lockStructure`, gives byte-identical output before and after the switch (Delivery 0,
clauses 1–4; review round 3 M2). Positional
rows (`Sqref`, `Range`, `Cell`, `Index`, `Marker`) are "–" for both: a rename does not move
anything, and a delete removes the sheet's own parts with it.

- **R**: rewrite.
- **D**: rewrite, and the element is removed when its reference is wholly deleted.
- **X**: the operation refuses if this place would change.
- **XX**: the operation refuses if the place exists on the edited or copied sheet (or, for
  workbook parts, in the package).
- **–**: unaffected, and not dispatched for this operation.
- **K**: dispatched and kept. The place can hold the sheet's name in prefix form, so the
  scan must accept it, but the operation does not change it (round 3 M1).
- **N**: renumbered or duplicated as stated in §4.
- **copied**: the copy keeps the value as it is (copy only).
- **part removed** / **removed on the copy**: as stated.

| # | Part | Place | Syntax | G1 (§3) | Copy (§4) | Table / column rename (§5, §6) | Sheet rename (ADR-0009 §6) | Sheet delete (ADR-0009 §5) | Corpus (count / files) |
|---|---|---|---|---|---|---|---|---|---|
| L1 | worksheet | `sheetData/row/@r` | `Index(row, 1-based)` | D (row removed) | copied | – | – | – | 813 sheets |
| L2 | worksheet | `row/@spans` | column hint `a:b` | R (column edits) | copied | – | – | – | 88,922 / 275 |
| L3 | worksheet | `row/c/@r` | `Cell` | D (cell removed) | copied | – | – | – | 1,502,954 cells |
| L4 | worksheet (any) | `c/f` text: normal, shared master, array, data-table anchor | `Formula(cell)`; shared: `Formula(master)` | R (§3.3) | R (§4.4) | R | R | R (`#REF!`) | 11,561 / 126 |
| L5 | worksheet | `c/f/@ref` (shared, array) | `Range` | R; X when an array is split (§3.3.3) | copied | – | – | – | 359 + 53 |
| L6 | worksheet | `c/f/@r1`, `@r2` (data table) | `Cell` | R; X when deleted | copied | – | – | – | 0 (1 synthetic) |
| L7 | worksheet | `dimension/@ref` | `Range` | R | copied | – | – | – | 787 |
| L8 | worksheet | `cols/col/@min`, `@max` | `Index(col, 1-based)` pairs | R (§3.4) | copied | – | – | – | 14,555 / 179 |
| L9 | worksheet | `sheetView/@topLeftCell`, `pane/@topLeftCell`, `customSheetView/@topLeftCell` | `Cell` | R, clamped (§3.5) | copied | – | – | – | 81 / 53 (`sheetView` only) |
| L10 | worksheet | `pane/@xSplit`, `@ySplit` (frozen: a row/column count; split: twips, unchanged) | count | R (§3.5) | copied | – | – | – | 32 / 19 |
| L11 | worksheet | `selection/@activeCell`, `@sqref` | `Cell`, `Sqref` | R, clamped | copied | – | – | – | 481 / 272 |
| L12 | worksheet | `mergeCells/mergeCell/@ref` | `Range` | D (§3.5) | copied | – | – | – | 54,468 / 43 |
| L13 | worksheet | `hyperlinks/hyperlink/@ref` | `Range` | D | copied | – | – | – | 113 / 15 |
| L14 | worksheet (any) | `hyperlink/@location` | `Location` | R; X when its target is deleted (§3.5) | kept (§4.4) | – | R | kept (navigation target, ADR-0009 §5) | 38 / 3 |
| L15 | worksheet | `autoFilter/@ref`, `filterColumn/@colId`, `autoFilter/sortState/@ref`, `sortCondition/@ref`; top-level `sortState` | `Range`, column offset | R; X header row or a filtered column deleted (§3.5) | copied | – | – | – | 3 + 7 |
| L16 | worksheet | `conditionalFormatting/@sqref` | `Sqref` | D (rule removed when empty) | copied | – | – | – | 108 / 20 |
| L17 | worksheet (any) | `cfRule/formula`, `cfvo/@val` (`type="formula"`) | `Formula(sqref first cell)` | R (§3.3.4) | R | R | R | R (`#REF!`) | 89 / 18, 2 |
| L18 | worksheet | `dataValidation/@sqref` | `Sqref` | D | copied | – | – | – | 108 / 13 |
| L19 | worksheet (any) | `dataValidation/formula1`, `formula2` | `Formula(sqref first cell)` | R (§3.3.4) | R | R | R | R (`#REF!`) | 122 / 12 |
| L20 | worksheet; `xm:f` (any) | `extLst` x14 CF/DV: `xm:sqref`, `xm:f`, `x14:cfvo/xm:f` | as L16–L19 | as L16–L19 | R | R | R (`xm:f`) | R (`#REF!`) | 22 / 6, 25 / 6 (15 sheet-prefixed / 2) |
| L21 | worksheet; `xm:f` (any) | `extLst` sparklines: `x14:sparkline/xm:f` (data), `xm:sqref` (host), `x14:sparklineGroup/xm:f` (date axis) | `Formula(none)`, `Cell` | R; sparkline removed with its host; X when its data range is wholly deleted (§3.5) | R (retargeted, Q3) | R | R (`xm:f`) | R (`#REF!`) | 11 / 1 |
| L22 | worksheet | `rowBreaks/brk/@id`, `colBreaks/brk/@id` (+ `count`, `manualBreakCount`); `brk/@min`, `@max` (extent on the other axis) | `Index(0-based)` | D; `@min`/`@max` by the range rule on the other axis, pinned at the edge (§3.2) | copied | – | – | – | 10 + 3 / 4 + 3 |
| L23 | worksheet | `ignoredErrors/ignoredError/@sqref` | `Sqref` | D | copied | – | – | – | 5 / 3 |
| L24 | worksheet | `protectedRanges/protectedRange/@sqref` | `Sqref` | R; X when it would become empty | copied | – | – | – | 4 / 4 |
| L25 | worksheet | `customSheetViews/customSheetView/…` (`selection`, `pane`, breaks, `autoFilter`) | as L9–L15, L22 | as there | copied | – | – | – | 3 / 1 |
| L26 | worksheet | `cellWatches/cellWatch/@r`, `smartTags/cellSmartTags/@r` | `Cell` | D | copied | – | – | – | 0 |
| L27 | worksheet | `scenarios/scenario/inputCells/@r`, `scenarios/@sqref` | `Cell`, `Sqref` | XX | XX | – | – | – | 0 |
| L28 | worksheet | `dataConsolidate/dataRefs/dataRef/@ref` | `Range` | XX | XX | – | – | – | 1 / 1 |
| L29 | worksheet | `webPublishItems/webPublishItem/@sourceRef` | `Range` | XX | XX | – | – | – | 0 |
| L30 | worksheet; formula attributes (any) | `oleObjects`, `controls` (anchors, `@linkedCell`, `@listFillRange`) | `Marker`, `Formula` | XX | XX | – | R | R (`#REF!`) | 6 / 5, 2 / 2 |
| L31 | worksheet | `mc:AlternateContent` at the top level, in `sheetData` or in a row; and **anywhere inside a registry element** of a part the operation edits (e.g. the Choice of a `sortState` or `filterColumn` holding x14 `sortCondition/@ref`) | – | XX; X inside a registry element | XX | X inside a registry element | – | – | 2 / 2, 0, 0 |
| L32 | workbook (any) | `definedName` text (all scopes, `_xlnm.*` included) | `Formula(none)` | R (§3.3.5) | R + local names duplicated (§4.4) | R | R | R (`#REF!`); local names of the sheet removed | 858 / 62 |
| L33 | workbook | `definedName/@localSheetId`, `workbookView/@activeTab`, `@firstSheet` | position | – | N (ADR-0009 §4) | – | – | N (renumbered) | 142 / 24 |
| L34 | `calcChain.xml` | `c/@r`, `c/@i` | `Cell` | part removed (ADR-0003 §4.3) | part removed | – | – | part removed | 116 |
| L35 | table | `table/@ref`, `autoFilter/@ref`, `sortState/@ref`, `sortCondition/@ref` | `Range` | R / X (§3.6.1) | N (§4.3) | – | – | – | 61 / 29 |
| L36 | table | `tableColumns/tableColumn` set, `filterColumn/@colId` | columns | R (§3.6.1) | copied | column name: R | – | – | 199 / 29 |
| L37 | table (any) | `calculatedColumnFormula`, `totalsRowFormula` | `Formula(first data row)` | R | R | R | R | R (`#REF!`) | 16 / 8 |
| L38 | table | `table/@name`, `@displayName` | `RawName` | – | N | R | – | – | 61 / 29 |
| L39 | comments | `comment/@ref` | `Cell` | D (note removed with its VML shape and thread) | copied | – | – | – | 204 / 27 |
| L40 | VML | `x:ClientData/x:Row`, `x:Column` | `Index(0-based)` | R with L39 | copied | – | – | – | 204 / 27 |
| L41 | VML | `x:ClientData/x:Anchor` | `Marker` (8 integers) | R (§3.6.3) | copied | – | – | – | 410 / 37 |
| L42 | VML (any) | `x:FmlaLink`, `x:FmlaRange`, `x:FmlaTxbx`, `x:FmlaPict`, `x:FmlaGroup` | `Formula(none)` | X | XX (controls) | X | R | R (`#REF!`) | 0 (the 57 `x:Fmla*` in 4 files are all `x:FmlaMacro` macro names, not positions) |
| L43 | threaded comments | `threadedComment/@ref` | `Cell` | D with L39 | N (new GUIDs) | – | – | – | 4 / 3 |
| L44 | drawing | `xdr:twoCellAnchor`/`oneCellAnchor` `xdr:from`, `xdr:to` | `Marker` | R / X (§3.6.3) | copied | – | – | – | 181 + 30 / 47 + 8 |
| L45 | drawing | anchors inside `mc:AlternateContent` | `Marker` | X when they would change | XX | – | – | – | 170 / 4 |
| L46 | drawing, chart user shapes (any) | `xdr:sp/@textlink`, `cdr:sp/@textlink` | `Formula(none)` | R | R | R | R | R (`#REF!`) | 0 |
| L47 | chart (any) | `c:f`, `c15:f`, `c15:sqref`, `c15:fullRef`, `c15:formulaRef`, `c15:datalabelsRange`, `c15:filteredSeriesTitle`, `c15:filteredCategoryTitle` | `Formula(none)` | R; X when a series range is wholly deleted (§3.6.4) | R (§4.4) | R | R | R (`#REF!`) | 174 / 25 |
| L48 | chartEx (any) | `cx:f` | `Formula(none)` | X | XX | X | X | X | 0 |
| L49 | chart (any) | `c:pivotSource/c:name` | `RawName` | K (pivot rules apply to the pivot table) | XX | – | R; X if the new name needs quotes | X | 1 / 1 |
| L50 | pivot cache | `worksheetSource/@ref` (+ `@sheet`) | `Range` | R when moved; X when expanded or shrunk (§3.6.5) | – | – | – | – | 7 / 5 |
| L51 | pivot cache | `worksheetSource/@name` | `RawName` (table or name) | – | – | R (table rename) | – | – | 3 / 3 |
| L52 | pivot table | `location/@ref` and everything inside | `Range` | X if the edit crosses or moves it (§3.6.5) | XX | – | – | – | 12 / 7 |
| L53 | pivot cache | `consolidation/rangeSets/rangeSet/@ref` | `Range` | X | – | – | – | – | 0 |
| L54 | `tableSingleCells` | `singleXmlCell/@r` | `Cell` | R; X when deleted | XX | – | – | – | 14 / 14 |
| L55 | `connections.xml` (any) | `parameter/@cell` | `Formula(none)` | R | – | R | R | R (`#REF!`) | 12 / 1 |
| L56 | ctrlProps (any) | `@fmlaLink`, `@fmlaRange`, `@fmlaTxbx`, `@fmlaGroup` | `Formula(none)` | X | XX | X | R | R (`#REF!`) | 0 non-empty |
| L57 | query table | `queryTable` bound to a `queryTable` table | – | X when its table's `ref` changes | XX | X (rename) | – | – | 13 / 2 |
| L58 | revisions | `xl/revisions/*` | – | XX (`SharedWorkbook`) | XX | XX | XX | XX | 1 / 1 |
| L59 | `activeX*.bin` | `LinkedCell`, `ListFillRange` (binary) | – | XX (`BinaryReferencesPossible`, ADR-0009 §6) | XX | XX | XX (`BinaryReferencesPossible`) | – | 0 |
| L60 | `externalLink` (any) | another workbook's cells and names | – | K | – | – | – | – | 14 / 10 |
| L61 | `metadata.xml`, `richData/*` | `cm`/`vm` indices (not positions) | – | – (move with the cell) | copied | – | – | – | 1, 2 |
| L62 | workbook | `oleSize/@ref` (visible area of an embedded workbook) | `Range` | R when it names the edited sheet's area *(u: it has no sheet; assumed the first sheet)*; X when its area is changed in any other way | – | – | – | – | 2 packages |
| L63 | workbook | `webPublishObjects/webPublishObject/@sourceObject` | `RawName`/range | X when it names the edited sheet | X | X | X | X | 0 |
| L64 | `connections.xml` | `x15:rangePr/@sourceName`, model connections naming a table (`WorksheetConnection_…`) | `RawName` | – | – | X (table rename; review m13) | – | – | 0 |
| L65 | `volatileDependencies.xml` | `tp/tr/@r` (`CT_VolTopicRef`) | `Cell` | XX when the edited sheet has any | XX | – | – | – | 0 |
| L66 | worksheet | `sheetPr/@codeName` | name | – | removed on the copy (§4.2) | – | – | – | 93 / 32 |
| L67 | worksheet, workbook | `sheetProtection` (`insertRows`, `insertColumns`, `deleteRows`, `deleteColumns`); `workbookProtection/@lockStructure` | flags | refuse when locked (Q13) | refuse on `lockStructure` (Q13) | – | Protected under `lockStructure` (m8) | Protected under `lockStructure` (m8) | 7 / 4; 4 |
| L68 | worksheet `extLst` | x14 `protectedRanges/protectedRange/xm:sqref` | `Sqref` | as L24 | copied | – | – | – | 0 |
| L69 | worksheet `extLst` | x14 `ignoredErrors/ignoredError/xm:sqref` | `Sqref` | as L23 | copied | – | – | – | 0 |
| L70 | worksheet `extLst` (any) | x15 `webExtensions` bindings (`xm:f`) | `Formula(none)` | X | XX | X | R | R (`#REF!`) | 0 |
| L71 | sheet `.rels`, drawing `.rels` (any) | Internal relationship `Target="#Sheet!A1"` (shape and cell hyperlinks) | `Location` | R through `set_relationship_target` in the operation's `RegistryPlan`; X when the target is wholly deleted (as L14) | kept | – | R (`set_relationship_target`) | kept (ADR-0009 §5) | 18 renames of 838 retarget one (ADR-0009 §6) |
| L72 | pivot cache, worksheet | `worksheetSource/@sheet`, `consolidation/rangeSets/rangeSet/@sheet`, `dataConsolidate/dataRefs/dataRef/@sheet` | `RawName` | – (positions are L50/L53/L28) | – | – | R for `worksheetSource/@sheet`; X for `rangeSet`/`dataRef` | X (`SheetIsPivotSource` / `UnrewritableReference`) | 5 / 0 / 0 |
| L73 | `docProps/app.xml` (any) | worksheet titles (`vt:lpstr`), "Named Ranges" titles (`'1'!Print_Area`) | `RawName`, `Location` | K | N (ADR-0009 §11) | – | R | R (title removed) | 294 with titles, 32 |
| L74 | worksheet, table | `sortState/extLst` `{0FC57C36-…}`, `filterColumn/extLst` `{68452572-…}` | `Range` | X while it holds a `ref` | copied | – | – | – | 0 |

Known non-references (review round 3 M1). These are `scan.rs`'s `NonReference` set, copied
verbatim from its classification (`sheet_ops/mod.rs` L1129-1137; doc comment `scan.rs`
L33-35), plus the in-part non-references of `scan.rs` L62-65:

- parts whose name starts with `docProps/`, other than L73's `app.xml` titles, which are
  dispatched instead;
- content types ending in `sharedStrings+xml`;
- content types containing `comments` (notes) or `threadedcomments`;
- content types ending in `person+xml`;
- content types ending in `externalLink+xml` ("another workbook's names and sheets"), also
  dispatched as L60 (K);
- in worksheets: cell values `c/v`, inline-string text `c/is/t` and `c/is/r/t`;
- in formulas: string literals (`INDIRECT("A1")`) and external `[n]` references
  (ADR-0009 §6).

The ADR-0009 ↔ `LOCATIONS` test asserts this set as well.

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
  that is not XML;
- **protection** (Q13, owner decision): a G1 edit on a sheet whose `sheetProtection` has
  `sheet="1"` and does not allow the operation. The attributes `insertRows`, `insertColumns`,
  `deleteRows` and `deleteColumns` default to *locked*, and only `"0"`/`"false"` allows the
  operation. Also `copy_sheet` in a workbook with `workbookProtection/@lockStructure`. Both
  give `Err(Protected { scope, operation })`, with no override (7 sheets in 4 files, 4
  workbooks).
- For consistency, P7-pre makes **ADR-0009's add, delete, rename and move sheet** refuse with
  `Protected` under `lockStructure` too (review round 2 m8). That changes the behaviour of
  existing operations (Consequences).

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
| range spanning the whole axis (`A1:A1048576`) | unchanged (sticky, LO `refupdat.cxx` L263-268) | unchanged |
| range whose **end is the last row/column** (`A2:A1048576`, `col min=3 max=16384`) | the end **stays pinned** at the edge; the start moves or the range expands as above | the end stays pinned; the start moves or the range shrinks as above |
| range end pushed past the last row/column | **clipped** to the edge | – |
| a single cell, or a range **start**, pushed past the edge | refused: `ShiftOffSheet` *(LO and Excel give `#REF!`)* | – |

**Sticky ends** (review M1, decision): LibreOffice keeps a range end at the maximum sticky,
whatever the start. For non-formula ranges, `refupdat.cxx` L270-276 says "End was sticky, but
start may have been moved. Only on range." For formulas, `token.cxx` L2956, L2967 and L3044
(`IsEndRowSticky`). A range end that would move past the maximum is cut at the maximum
(`lcl_MoveEnd`, `refupdat.cxx` L49-67, `bCut`). Phase7 does the same everywhere a range lives:
formulas, `cols` (106 `col max="16384"` in 44 files), `dimension`, `selection`, every `sqref`,
breaks' `@max`.

**Precedence** (review round 2 m1): **wholly deleted wins over pinning.** A range whose every
row is deleted is deleted (`#REF!`, or the element removed) even when its end is the last row.
Deleting rows `p..1048576` under `A1048570:A1048576` deletes it; LO marks it `UR_INVALID`
before applying the sticky end (`refupdat.cxx` L252-266). Pinning applies only to a range
that keeps at least one row.

**View cells** (`sheetView`/`pane` `topLeftCell`, `selection/@activeCell`, L9, L11) are not
refused at the edge: on an insert they are **clamped** to the last row/column, as they are
clamped to the first surviving cell on a delete. A cursor saved on the last row must not
block every row insert.

These are LO's rules with its `ExpandRefs` option off (§Reference libraries). Inserting at the
first row of a range moves it; inserting just below it does not grow it. Excel behaves the same
*(u for the edge cases: XL item 1)*.

**Deleted references.** A deleted reference token becomes `#REF!`, replacing the whole token
(area included):

- unprefixed: `#REF!` (`SUM(A5)` → `SUM(#REF!)`);
- sheet-prefixed: `Sheet1!#REF!`, keeping the prefix as written (quotes included). The corpus
  has both forms written by Excel (fact 5); ADR-0009 established the bare form for a deleted
  *sheet*. Phase1's lexer reads the prefixed form as a reference token with
  `Area::RefError` (`lexer.rs` L28-29) and the bare form as an error literal (review n6)
  *(u: which form Excel writes for a deleted cell through a prefix: XL item 2)*.

**Insert refusals** (before any change):

- **Content pushed off the sheet**: any `row` element in the last `n` rows (or any `c` in the
  last `n` columns), or any **object with content** that reaches there (merge, hyperlink,
  table, note, threaded comment, drawing anchor, array or data-table range):
  `Err(ShiftOffSheet { what })`. Ranges without content of their own (`cols`, `dimension`,
  `selection`, CF/DV/`ignoredErrors`/`protectedRanges` `sqref`s, formula ranges) are pinned
  or clipped instead (review M1). LO refuses the same
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
   text, and leaves the group. **`#REF!` tokens compare equal under translation**: ADR-0003
   §4.2's translation copies a `#REF!` (bare or prefixed) unchanged and does not fail on it.
   Without this rule, deleting a header row that every member refers to (`$A3*C$2`) would
   split every such group: the reviewer's probe split 74 of 294 groups that way (review m2);
4. the master's `ref` becomes the bounding rectangle of the remaining members (ADR-0003 §4.2
   step 3); a group left with one cell becomes a plain formula.

The spike runs this on the corpus: 286 of 292 multi-row groups stay whole and 6 split (fact 4).
That holds for an edit in the middle of each group and for an edit **at every position** from
the lowest referenced row to one past the last member. Shared master texts with a sheet prefix
(a cross-sheet group) occur 0 times. One would split at `p` like the 6.
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
  Nothing here needs Phase5's dynamic-array writer (P5d, not on main): Phase7 only moves
  anchors, or refuses (review m11).

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
- if the `sqref` becomes empty, the rule is removed (§3.5);
- **a `sqref` with several ranges** (11 CF and 6 DV in the corpus) whose first range's
  top-left cell is deleted, and whose formula has relative references, is **refused**
  (`StructureBlocked`). Excel may anchor such a rule at the bounding box's top-left instead.
  The two readings agree only while **both** candidate anchors survive. For
  `sqref="B5:B10 A3:A4"`, deleting row 3 removes the bounding-box anchor `A3` and keeps the
  first-range anchor `B5`. So a multi-range `sqref` with relative references refuses when
  **either** the first range's top-left **or** the bounding box's top-left is deleted
  (review m4; round 2 m5). XL item 6 decides the reading.

The scope rule of §2.2 applies here: a CF, DV, x14 or sparkline formula **on another sheet**
that names the edited sheet is rewritten like a cell formula, with no re-anchoring, because its
anchor is on its own sheet and does not move (review B1).

Table `calculatedColumnFormula` and `totalsRowFormula` are treated as relative to the column's
first data cell and the totals cell *(u)*; their A1 references are rare (6 texts in 3 files).

##### 3.3.5 Defined names

- Name texts are rewritten with §3.2 like cell formulas, with no anchor.
- **A name with an unprefixed reference, or a relative component in a reference to the
  edited sheet, is refused** (`UnrewritableReference`): its meaning depends on the cell that
  uses it. The corpus has none (0 of 858).
- `_xlnm.Print_Area` and `_xlnm.Print_Titles` follow the range rule. A delete that would
  **wholly delete** a print area or a print-titles range (any member of the union) is
  **refused** (`ReferenceDeleted`). The name is never dropped silently (decision on Q12;
  review m9). Excel's own form is *(u)*: XL item 7. `_xlnm._FilterDatabase` follows the
  autofilter (§3.5).
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
  instead of expanding it. The split edits the existing `col`'s `max` in step 3.2 and inserts
  the second `col` in step 3.5 (§2.1). New rows and columns therefore get the default format (no `row` or
  `col` element), by the owner's decision on Q1. Excel formats them like their neighbours by
  default *(u)*; copying the format can be added later without breaking anything.
  `col max="16384"` stays pinned (§3.2).
- `sheetFormatPr`, outline levels, `row/@ht`/`hidden` move with their row; `outlineLevelRow`
  is not lowered (ADR-0014 §9).

#### 3.5 Range features

| Place | Rule |
|---|---|
| merges | range rule. A merge that shrinks to one cell or is wholly deleted is removed. If its anchor is deleted, the first surviving cell becomes the anchor. Third-party files can carry values in non-anchor cells (12 in `60512.xlsm`, ADR-0014), so step 1 checks the new anchor: a **non-empty** new anchor is refused (`StructureBlocked`), because it would surface a hidden value (review m14). |
| hyperlinks | `@ref` by the range rule; removed when deleted. Its relationship stays (as ADR-0014 `remove_hyperlink`). `@location` on any sheet: cell/range rule; a target that would be **wholly deleted** is refused (`ReferenceDeleted`) rather than left pointing at other cells. ADR-0009 §5 left the text only because the target sheet no longer exists (review m3). |
| autofilter | `@ref` by the range rule. Deleting its **header row** or a column with a `filterColumn` is refused (`StructureBlocked`). `filterColumn/@colId` (offset from the first column) is re-indexed. `sortState`/`sortCondition` follow. |
| CF / DV (2007 and x14) | `sqref` items by the range rule; empty items dropped; the rule removed when the list is empty, with its linked x14 rule and empty containers (ADR-0014 §2's container rule); `count` updated. Formulas: §3.3.4. |
| sparklines | data `xm:f` by §3.3, on any sheet (B1); a data range that would be **wholly deleted** is refused (`ReferenceDeleted`), as for chart series. Host `xm:sqref` by the cell rule; a sparkline whose host is deleted is removed, then an empty group, then an empty `ext`, by ADR-0014 §2's container rule. Phase7 needs nothing from Phase5's sparkline writer (P5d, not on main) (review m11). |
| views | `topLeftCell`, `activeCell`: cell rule, **clamped** to the first surviving cell when deleted. `selection/@sqref`: range rule, `activeCell` when empty. Frozen panes: `ySplit` (rows above the freeze line) grows by the inserted rows above it and shrinks by the deleted rows above it *(u: XL item 8)*. Split (not frozen) panes keep `xSplit`/`ySplit`, which are twips, unchanged (n7). |
| breaks | `brk/@id` by the index rule; removed when deleted; `count`, `manualBreakCount` updated. `brk/@min`/`@max` (the extent on the other axis) by the range rule there, pinned at the edge (`max="16383"`). |
| `ignoredErrors` | as CF `sqref`; the element removed when empty. |
| `protectedRanges` | range rule; **refused** when a protected range would become empty (it carries a password hash; removing it silently changes protection). |
| `customSheetViews` | the same rules for their `selection`, `pane`, breaks and `autoFilter`. |
| `scenarios`, `dataConsolidate`, `webPublishItems`, `oleObjects`, `controls`, top-level AC | refused when present (§2.2 L27–L31, all XX; review m1). |

#### 3.6 Objects

##### 3.6.1 Tables

- Inserting rows strictly inside a table (after the header, up to and including the totals
  row) **expands** `ref` and the table's `autoFilter/@ref`; inserting at the header row moves
  the table. The new rows are empty table rows. Calculated columns are **not** filled into the
  new rows *(u: Excel fills them: XL item 9)*.
- Deleting rows: the header row → `Err(TableStructure { table, reason: "header row" })`; the
  totals row → refused; every data row → refused (a table keeps at least one); the whole
  table → removed through ADR-0017 §6.4 (refused when referenced, `TableInUse`).
- Inserting columns strictly inside a table adds `tableColumn`s, by the owner's decision on
  Q11, as Excel does: ids `max + 1`, names `Column{n}` with the smallest free `n`
  (ADR-0017 §6.2) *(u: Excel's exact names: XL item 9)*. Their header cells are written
  through the composed `CellWritePlan` of §2.1.1, at post-edit positions. `queryTable` and
  `xml` tables refuse (L57), and so do tables that are a pivot or slicer source.
- Deleting a column of a table removes its `tableColumn`, unless it is the last one or any
  structured reference names it (`ColumnInUse`), or the table is a pivot or slicer source.
- Tables are rewritten as small staged parts. The table's position check against merges and
  autofilters (ADR-0017 §6.1) holds after the edit by construction: everything moves alike.

##### 3.6.2 Notes and threaded comments

- `comment/@ref` and the VML `x:Row`/`x:Column` of its shape follow the cell rule; `x:Anchor`
  follows §3.6.3. A note whose cell is deleted is removed with its VML shape (matched as in
  ADR-0017 §4), and a threaded comment with its replies and fallback note (§5 there).
- When the last note of a sheet goes, the comments and VML parts are removed with their
  relationships and content types, unless the VML holds other shapes (§2.1).
- This uses Phase5's notes machinery (P5c, not on main). Until P5c is merged, a G1 edit that
  would change a note refuses (`StructureBlocked`).

##### 3.6.3 Drawing and VML anchors

A marker is (cell index, offset in EMU or pixels). For each anchor on the edited sheet:

- `twoCellAnchor` with `editAs="twoCell"` (the default): `from` and `to` follow the cell rule,
  so inserting inside the object stretches it, as "move and size with cells" means;
- `editAs="oneCell"` and `oneCellAnchor`: `from` follows the cell rule and `to` moves by the
  same amount (size kept);
- `editAs="absolute"` and `absoluteAnchor`: unchanged;
- a delete that **fully contains** an anchored object (both markers in the deleted rows) is
  refused (`ObjectInDeletedRange { part, what }`), by the owner's decision on Q2. Excel either
  deletes such objects or collapses them *(u)*;
- **partial deletes** (one corner in the deleted band; review round 2 m7):
  - `twoCell`: the corner in the band moves to the first surviving row/column with offset 0,
    so the object shrinks;
  - `oneCell` and `oneCellAnchor`: if `from` is in the band, it moves to the first surviving
    row with offset 0 and `to` moves by the same amount, so the size is kept. If only `to`
    is in the band, nothing moves;
  - a note's `x:Anchor` follows its note's cell (below), so a corner in the band takes the
    same shift as the note's cell; a note whose own cell is deleted goes with it (§3.6.2);
- VML `x:Anchor` (left column, offset, top row, offset, right column, offset, bottom row,
  offset) is rewritten **only for `Note` shapes**. It moves with its note (both corners by the
  cell rule, offsets kept), so the note box stays next to its cell. The rule does **not** read
  `x:MoveWithCells`/`x:SizeWithCells`: their polarity (presence vs value, and the notes'
  default) is a known trap that ECMA-376 Part 4's VML description does not settle (XL item 14;
  review m12). Any other VML shape type (`Pict`, form controls) whose anchor would change is
  refused;
- anchors inside `mc:AlternateContent` (170 in 4 files, all form-control files) that would
  change: refused.

POI moves a shape when its top-left anchor is inside the shifted band and leaves absolute
anchors (`XSSFRowColShifter.java` L260-295); it does not resize.

##### 3.6.4 Charts

Every chart's `c:f` family (L47) is rewritten with §3.3, spliced into the original bytes, in
every chart part of the package (charts on other sheets plot this one too; 168 sheet-prefixed
`c:f`). Caches are left as they are; Excel re-reads linked ranges on open *(u, roadmap §4)*.
ADR-0017 §7.2's explicit refresh will apply once P5e is merged; it is not on main (review
m11). A series range that would be **wholly deleted** is refused (`ReferenceDeleted`) rather
than written as `Sheet1!#REF!` *(u: XL item 10; Q8)*.

##### 3.6.5 Pivot tables and caches

- An edit that crosses or moves a pivot table's `location/@ref` is refused
  (`StructureBlocked`), as LO does for intersecting edits
  (`InsertCellsOperation.cxx` L365-371). Edits wholly after (below or right of) it are
  allowed. 12 pivot tables on 12 sheets.
- A cache whose `worksheetSource` is on the edited sheet: when its `ref` only **moves**, it
  is rewritten; when it would expand, shrink or be deleted, the edit is refused: the cache
  records would no longer match the range until a refresh (Q7, owner decision).

##### 3.6.6 Others

`singleXmlCell/@r` (14 parts) follows the cell rule (deleted → refused); `parameter/@cell`
in `connections.xml` follows §3.3; ctrlProps and VML `x:Fmla*` that would change are refused.

#### 3.7 Phase1 and Phase2

- **calcChain**: removed in the operation's plan whenever a formula cell moves or is deleted
  (ADR-0003 §4.3; the chain keys on cell positions).
- **Cache strips: one rule** (review M2, decision). Any structural edit strips the cached
  value of **every formula whose references touch the edited sheet**, and sets
  `fullCalcOnLoad`. That is ADR-0003's policy of invalidating rather than computing. A
  formula touches the edited sheet when any of these holds:
  - **directly**: one of its reference tokens refers to the edited sheet, unprefixed on that
    sheet or sheet-prefixed anywhere, 3-D spans included;
  - **through a name**: it uses a defined name whose text refers to the edited sheet
    (transitively through other names);
  - **through a table**: it uses a structured reference or a bare table name of a table on
    the edited sheet;
  - it is **position-dependent or unresolvable**: `Everything` in the census (`INDIRECT`,
    `OFFSET` with text, unknown names; ADR-0003 §2.2), or a call to `ROW`, `COLUMN`, `CELL`
    or `ADDRESS` without a reference argument, on the edited sheet. The census has no
    function flags, and `=ROW()` has no precedents, so **every formula cell on the edited
    sheet at or after `p` is lexed** to find these calls: one lex per moved formula, done in
    step 1 (review round 2 m2);

  and, transitively, **every dependent of those formulas** (ADR-0003 §3.2).
- This is deliberately not smarter than that. Whole-column references whose content shifts
  (`MATCH(x,A:A,0)`; LO marks them value-changed, `token.cxx` L3338-3346), names and tables
  whose extent grew or shrank (`ROWS(MyRange)`, `ROWS(Table1)`), and every changed or `#REF!`
  text are all inside the set. Formulas that only refer to other sheets keep their caches, and
  a formula cell that merely moved keeps its value unless it touches the edited sheet (review
  m8).
- The set is computed from the census precedents and the name/table resolution of ADR-0003
  §2.4 before the edit, in step 1 (§2.1). Strips are step 3.1, at pre-edit indices, on every
  sheet. `KeepStaleCaches` and manual calculation are honoured as ADR-0009 §2 does.
- A copy (§4.5) uses the same rule with "the copied sheet" in place of the edited sheet. That
  is a deliberate over-approximation, since the copy's values equal the source's except for
  position-dependent formulas.
- **Renames do not change values** (§5, §6): they strip only the `Everything` formulas (and
  their dependents), whose string arguments may name the old table or column.
- **Phase2**: `sst/@count` is lowered for deleted and stripped `t="s"` cells through the
  operation's `RegistryApply` (§2.1). No string or style entry is removed (Layer 3
  append-only).

#### 3.8 Performance

- Work is linear in the rows and cells at or after `p` (an `r` write each) plus the formulas
  whose text changes (a lex and a render each). The census pre-filter (§3.3.1) skips formulas
  that cannot change.
- **Removals in bulk** (review m7). `LensMut::remove_at` is a `Vec::remove` (`lens.rs`
  L1208), so removing `k` of `n` rows one at a time is O(k·n): 10¹¹ moves for 100k rows at the
  top of a 1M-row sheet. P7-pre adds a public `LensMut::retain_children`/`remove_range`. The
  crate-internal `RawNode::retain_children`/`splice_children` exist (`node.rs` L100-106).
  Step 3.4 removes each parent's children in one pass, which keeps the descending-order
  guarantee of §2.1 trivially.
- Changing an attribute marks only that element `own_dirty`. The serializer then re-renders
  only its open tag and copies its children's spans (`crates/ooxml-core/src/xml/write.rs`
  L21-63). The original attribute spelling (quotes, order, entity forms) is kept only for a
  node that opted in with `LensMut::preserve_attribute_spelling()` (`lens.rs` L1235,
  `node.rs` L68). **Every renumbered or rewritten element opts in**, so T2 holds at attribute
  level (review m7). A renumbered row therefore costs its open tag and its cells' open tags,
  not their `v`/`f` bytes.
- No clone of a worksheet is made (§2.1). The census and row index are rebuilt once, at open
  cost: 0.29 s census scan for a 1M-cell sheet (ADR-0010 Context; measured on the 2026-11
  baseline).
- **Deferred rows** (ADR-0010 §2, not on main): a G1 edit must expand every row after `p`,
  because cells carry their row number. That is acceptable for a rare operation and is
  recorded as a requirement on stage 2x: the row index must allow a bulk renumber.
- Not measured yet: no G1 code exists. P7a adds two criterion benchmarks on `ooxml-bench`'s
  1M-cell sheet:
  - insert one row at row index 1, then save;
  - `delete_rows(1, 100_000)`, then save.

  Each reports time and peak memory against open-and-save of the same file. **Target (Q10,
  owner decision):** the insert takes at most 2× the time of open + save. The target is
  measured and published with the PR, **not** a hard CI gate at first. The delete case is
  reported without a target until it has a baseline.

### 4. Copy a worksheet

#### 4.1 What is copied, what is refused

`copy_sheet(source, new_name, position)` validates the name (ADR-0009 §3) and allocates
`sheetId`, part name and relationship id as ADR-0009 §3 does.

The relationship types of the source decide:

| Source relationship | Copy |
|---|---|
| `printerSettings`, `customProperty` | new part, same bytes |
| `image` (background), images of drawings and VML | **shared**: a new relationship to the same media part (ADR-0017 Q2) |
| `drawing` | new drawing part (bytes, then §4.4). Its relationships, recursively (review m11): `image` shared; `chart` → new chart part with its `chartStyle`, `chartColorStyle`, `themeOverride` and `chartUserShapes` (→ drawing → `image` shared) copied as new parts; `hyperlink` (External) copied. **Refused**: an embedded workbook (`package`, `c:externalData`), SmartArt (`diagramData`/`diagramLayout`/`diagramQuickStyle`/`diagramColors`/`diagramDrawing`), chartEx, and any other type. |
| `vmlDrawing`, `comments`, `threadedComment` | new parts; VML renumbered (§4.3); threads get new GUIDs, the same persons |
| `table` | new table parts (§4.3) |
| `hyperlink` (External) | copied relationship, same target |
| `pivotTable`, `oleObject`, `control`, `ctrlProp`, `package`, `tableSingleCells`, query tables, slicers, timelines, chartEx, any other | **refused** (`SheetHasUnsupportedParts { rel_types }`) |

Also refused: top-level `mc:AlternateContent`, `scenarios`, `dataConsolidate`,
`webPublishItems`, a VML part that is not XML when it must be renumbered, and a workbook with
`workbookProtection/@lockStructure` (Q13: `Protected`). Under these rules **773 of 813 corpus
worksheets** can be copied; the blockers are `tableSingleCells` (14 sheets), pivot tables
(12), OLE objects (5), query tables (4), controls (2), `package` (2), AC (2).

#### 4.2 The copied sheet part

The new sheet part starts as the **source's current bytes** (or serialized tree), so it is
structurally identical, then:

- `r:id`s are remapped to the new relationships;
- `sheetView/@tabSelected` is removed (the copy is not selected);
- **`sheetPr/@codeName` is removed** (review M5, decision). ADR-0009 §4 gives a new sheet no
  `codeName` so that Excel creates its document module. A duplicate would name one VBA module
  for two sheets in a `vbaProject.bin` this crate cannot update. 93 corpus sheets carry a
  `codeName`, 33 of them in the 8 packages with a VBA project (XL item 13: an `.xlsm` copy
  opens with its macros intact and Excel adds the module);
- `xr:uid` attributes are regenerated (64 in sheets, 40 in tables in the corpus), so no two
  CF rules, DV rules or tables share one *(u: whether Excel minds duplicates; XL item 11)*;
- formulas: §4.4.

#### 4.3 Ids and names inside the copy

- **Tables**: new `id` (workbook `max + 1` with the session high-water, ADR-0017 §2.2), new
  name and display name: the source name without trailing digits, followed by the smallest
  `n ≥ 2` that is free among tables and defined names (`Sales` → `Sales2`, `Table1` →
  `Table2`) *(u: Excel's form; Q5, defaulted)*. POI uses `TableN` (`XSSFSheet.java` L4494-4503).
- **VML**: a new `o:idmap` block (`max + 1` over the workbook, ADR-0017 §2.2); shape ids
  renumbered into it in document order, `o:spid` too.
- **Drawings**: `cNvPr/@id`s are scoped to the drawing part and stay.
- **Threaded comments**: new GUIDs for `id`, matching `parentId`s and the fallback notes'
  `xr:uid` (ADR-0017 §5).

#### 4.4 References in and to the copy

- **Formulas on the copy that refer to the source by name** (`Data!A1` written on `Data`)
  keep their text, so they still refer to the source (Q3, owner decision). Excel's internal
  form for such a reference is believed to be sheet-qualified, so it would do the same
  *(u: XL item 12)*.
  Unprefixed references are the copy's own cells by construction.
- **Structured references on the copy** to a table of the source become references to the
  copy's table (by token, §2.3), in cells, calculated columns, totals, CF, DV and names
  duplicated below. References to tables on other sheets are kept. POI does the same with a
  string replace (`XSSFSheet.java` L5176).
- **Charts and sparklines copied with the sheet** have their references to the source (`c:f`
  family; sparkline data `xm:f`) retargeted to the copy (Q3, owner decision). POI does the same
  for charts with `replaceReferences` (`XSSFWorkbook.java` L759-794), and Excel is believed
  to *(u: XL item 12)*.
- **Sheet-local names of the source** (`Print_Area`, `Print_Titles`, `_FilterDatabase`,
  others) are duplicated with `localSheetId` = the copy's position and their references to
  the source retargeted to the copy. POI does not copy them. Workbook-scope names that refer
  to the source are **not** duplicated (Q4, owner decision). Excel is widely reported to create
  local copies of them *(u: XL item 12)*.
- Hyperlink `location`s on the copy that point into the source are kept (navigation targets),
  like formula texts.
- **Elsewhere**, nothing refers to the new sheet yet, except 3-D spans that now include it
  (inserted between endpoints) and unknown-sheet references that now resolve: ADR-0009 §4's
  seeds.

#### 4.5 Workbook-level changes

As ADR-0009 §4 (add a sheet): the `<sheet>` element at `position`, `localSheetId` /
`activeTab` / `firstSheet` renumbered, `app.xml` (§11 there), Phase1 seeds (3-D spans,
`SHEET()`/`SHEETS()`, every `Everything` formula, newly resolving names), `fullCalcOnLoad`.
The copy's formula cells enter the census by the rebuild; `calcChain.xml` is removed because
the set of formula cells changed (ADR-0003 §4.3). Caches are stripped by §3.7's rule applied
to the copy: every formula on the copy whose references touch the copy (its unprefixed
references, retargeted structured references and local names), every position-dependent or
`Everything` formula on it, and their dependents.

The source sheet part is not touched: it stays byte-identical.

### 5. Rename a table

`rename_table(old, new)`:

- **Validation**: ADR-0017 §6.2's name rules and uniqueness against tables and defined names
  (`InvalidTableName`, `NameInUse`). A case-only change is allowed. Tables of type
  `queryTable`/`xml` are refused *(u: whether their query or map parts name the table; L57)*.
- **Writes**: `table/@name` and `@displayName` (the table part, staged);
- **Rewrites**, in every `Formula` place of §2.2 of scope `any` (cells on every sheet, names,
  CF, DV, x14, sparklines, table formulas of every table, chart `c:f`, `textlink`, connection
  parameters, L55):
  - a structured-reference token whose table name folds equal to `old` (ADR-0003 §2.4): only
    the table-name bytes change, the bracket body is copied;
  - a bare name token equal to `old` (`=ROWS(Sales)`): it can only be the table, because
    tables and defined names share one namespace (ADR-0017 §6.2);
  - `worksheetSource/@name` of pivot caches (3 in 3 files).
- **Not rewritten** (documented): `INDIRECT("Sales[Q1]")` strings, external `[1]Sales[…]`
  references, VBA.
- **Refused**: a table named by a data-model connection (`x15:rangePr/@sourceName`,
  `WorksheetConnection_…`; L64; review m13).
- **Closed-world scan** (§2.2) for `old[` and bare `old` tokens outside registry places;
  a formula that does not lex and mentions `old` refuses.
- **Phase1**: values do not change. Only `Everything` formulas and their dependents are
  stripped (a string that names the table no longer resolves), as for a sheet rename
  (ADR-0009 §6) and as §3.7 states for renames.

Microsoft documents that Excel rewrites "all structured references" on a rename (§Reference
libraries); POI rewrites none.

### 6. Rename a table column

`rename_table_column(table, old, new)`:

- **Validation**: as ADR-0017 §6.3: unique case-insensitively in the table
  (`DuplicateColumnName`), not empty, no control characters other than tab/LF/CR, encoded as
  ST_Xstring in `tableColumn/@name`. Refused for `queryTable`/`xml` tables and for tables
  that are a pivot or slicer source (`TableHeaderCell`, ADR-0017 §6.3 m6).
- **Writes**: `tableColumn/@name` and the header cell, through the `CellWritePlan` composed
  into the operation's plan with the identity index map (§2.1.1): Phase5's header sync, with
  the decoded name as the header text. A table without a header row writes only the
  attribute.
- **Protection** (Q13; review round 2 m8): on a protected sheet (`sheetProtection/@sheet="1"`)
  the header write refuses with `Protected` **unless the header cell is unlocked**: its style's
  `xf` has `protection/@locked="0"`. That is a lookup in the Phase2 style registry, which is
  cheap. A table without a header row writes no cell and is not refused. Inserting table
  columns (§3.6.1) on a protected sheet is already refused by `insertColumns` (§2.4).
- **Rewrites**: every column element of every structured reference that resolves to this table
  (tokens naming the table, and table-less tokens in formulas inside the table's range: its
  cells, calculated columns, totals), including both endpoints of column ranges and the
  `[@Col]`/`[@[Col x]]` forms. Each rewritten element is re-rendered with ADR-0017 §6.1's
  `specifier()` (escapes, double brackets), the rest of the token copied. A
  structured-reference token that the sub-grammar cannot parse refuses (`ColumnInUse`), as in
  ADR-0017 §6.3.
- `set_string` on a header cell keeps refusing with `ColumnInUse` when the column is
  referenced: an implicit formula rewrite on a value write would be a surprise (Q6,
  defaulted). The caller uses `rename_table_column` instead.

### 7. Interaction with Phase1–5

| Phase | Effect |
|---|---|
| Phase1 (ADR-0003) | Translation (§4.2) re-anchors shared members, CF/DV formulas and hand-offs; the calcChain policy removes the chain; seeds per §3.7, §4.5, §5. Rebuild after apply (ADR-0009 §2). |
| Phase2 (ADR-0006) | `sst/@count` on deletes and strips through the operation's single `RegistryApply`; header cells of new table columns and renamed columns through the composed `CellWritePlan` (§2.1.1). |
| Phase3 (ADR-0009) | The registry replaces the scan's hard-coded `allowed()` and lets rename/delete rewrite DV, CF, x14, table formulas and connection parameters, which the implementation refuses today. Copy reuses §3 and §4 there. |
| Phase4 (ADR-0014) | Every range feature is a registry row; the allowed-diff data table gains Phase7's rows; the container rule (remove when empty) applies. |
| Phase5 (ADR-0017) | Tables (§6.2 names, §6.4 removal), notes (§4), threads (§5) and allocators (§2.2) are reused. The P5-pre `CellWritePlan` gains the composed variant of §2.1.1. Notes depend on P5c; chart cache refresh on P5e; nothing depends on P5d. |

### 8. Errors

New variants:

| Error | When |
|---|---|
| `ShiftOffSheet { what }` | an insert would push content (a `row`/`c` or an object with content), a single-cell reference or a range start past the last row or column (§3.2) |
| `ArrayFormulaSplit { cell }` | an edit splits a multi-cell array, data table or spill range (§3.3.3) |
| `TableStructure { table, reason }` | header or totals row, every data row, or the last column of a table deleted (§3.6.1) |
| `ObjectInDeletedRange { part, what }` | a drawing or VML object that lies **wholly inside** the deleted rows or columns (Q2; §3.6.3) |
| `ReferenceDeleted { part, location }` | a reference that cannot become `#REF!` would be deleted: chart series, sparkline data, pivot source, data-table input, `singleXmlCell`, a hyperlink target, a print area or print titles |
| `StructureBlocked { part, place }` | a registry place marked X/XX (§2.2), a pivot crossed, an autofilter header or filtered column deleted, a protected range emptied, a multi-range `sqref` re-anchor (§3.3.4), a non-empty new merge anchor (§3.5), a note before P5c |
| `Protected { scope, operation }` | sheet protection locks the G1 operation, or `lockStructure` blocks a copy (Q13; §2.4) |

Reused: `UnrewritableReference`, `MissingReference`, `PartNotEditable`, `SharedWorkbook`,
`BinaryReferencesPossible`, `SheetHasUnsupportedParts`, `InvalidSheetName`,
`DuplicateSheetName`, `InvalidTableName`, `NameInUse`, `DuplicateColumnName`, `ColumnInUse`,
`TableInUse`, `TableHeaderCell`, `NoSuchTable`, `NoSuchSheet`, `NotAWorksheet`, `Poisoned`,
`RegistryRefused`.

## Test oracles

1. **Locality** (byte-exact, ADR-0014 §2, data table in `tests/allowed_diff.rs`):

   | Operation | Edited sheet | Other parts |
   |---|---|---|
   | insert/delete rows or columns | `row/@r`, `c/@r` at or after `p`; removed `row`/`c`; `row/@spans` (column edits); changed `f` text and `f/@ref`/`@r1`/`@r2`; seeded `v`/`is`/`t`; `dimension`; `cols/col`; `sheetView` descendants; `mergeCells`; `hyperlinks`; `autoFilter`; CF/DV blocks and their `extLst` content; breaks; `ignoredErrors`; `protectedRanges`; `customSheetViews`; new header cells of inserted table columns (composed Phase1 write) | **other sheets** (review B1): changed `f` text, seeded `v`/`is`/`t`, `hyperlink/@location`, **CF `cfRule/formula` and `cfvo/@val`, DV `formula1`/`formula2`, `extLst` x14 `xm:f` and sparkline `xm:f` text**; **other sheets' tables**: `calculatedColumnFormula`, `totalsRowFormula`; `workbook.xml`: `definedName` texts, `calcPr`; `calcChain.xml` removed (+ rel, CT); `sharedStrings.xml` `count` (and appends for new header cells); the edited sheet's tables (`ref`, `autoFilter`, `tableColumn`s); comments, VML, threaded comments (rows, anchors, removed notes), **comments/VML parts removed with rel and CT when the last note goes** (m6); drawings (markers); charts (`c:f` text only, any chart); pivot caches (`worksheetSource/@ref`); `singleXmlCells`; `connections.xml` (`parameter/@cell`); removed tables with their rels and CT |
   | copy sheet | none: the source is unchanged (§4.5) | new parts (any content; the new sheet part without `sheetPr/@codeName`, M5); `workbook.xml` (`sheets`, `definedNames` with the duplicated local names, `bookViews`, `calcPr`); workbook `.rels`; CT; `app.xml`; calcChain removed; seeded caches on other sheets |
   | rename table | changed `f` texts on any sheet, seeded caches | the table part (`name`, `displayName`); every part with a rewritten formula place; pivot cache `worksheetSource/@name`; `workbook.xml` names and `calcPr` |
   | rename column | the header cell (Phase1 write composition), changed `f` texts | the table part (`tableColumn/@name`, `calculatedColumnFormula`, `totalsRowFormula`), parts with rewritten places, `sst` |

2. **Identity-tracking oracle** (independent of the rewriters). A synthetic fixture puts a
   unique value in every cell and one reference of every registry kind (every `Formula`,
   `Sqref`, `Range`, `Cell`, `Index`, `Marker` row of §2.2) pointing at known cells, on the
   edited sheet and, **for every scope-`any` row, on a second sheet** that refers to the
   edited sheet (review B1). It also has sticky-end ranges (`A2:A1048576`,
   `col min=3 max=16384`; review M1) and cache cases: `MATCH(x,A:A,0)`, `ROWS(name)`,
   `ROWS(Table1)`, `SUM(Table1[Qty])` and a formula on another sheet that refers to none of
   these, which must keep its cache (review M2). It also has a VML form control and a
   ctrlProp on a second sheet linked to the edited sheet, which must refuse, and a
   `#Sheet!A1` relationship target (review round 2 M1, M3). After an edit, a test parser written from §3.2's table (not
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
   every table and every table column.
   - Each result either refuses with every byte unchanged, or passes oracles 1, 4 and 6 and
     reopens.
   - **Every formula place on every sheet** (cells, names, CF, DV, x14, sparklines, table
     formulas, charts) is checked independently, in the output: each reference to the edited
     sheet, evaluated as a set of pre-edit cells, equals the input reference under §3.2
     (review B1).
   - Refusal reasons are counted and compared with the spike's blocker counts. A refusal
     that names a K or `–` place must not occur (review round 3 M1).
   - Explicit cases:
     - a package with `col max="16384"`;
     - an `.xlsm` with `codeName`s;
     - `NewStyleConditionalFormattings.xlsx` (x14 link ext);
     - the 2 files with sheet-prefixed x14 formulas;
     - `60512.xlsm` (values in non-anchor merge cells);
     - a protected sheet (Q13).
8. **Failure atomicity** (review M3), as ADR-0017's:
   - every refusal of §2.4, §3 and §8 has an `Err ⇒ byte-identical` test;
   - fault injection (`Err` and panic, `catch_unwind`) **between each pair** of steps 3.1–3.5
     and in the rebuild poisons the workbook. The fixture is one where strips, rewrites,
     renumbering, removals and composed header-cell insertions touch the same sheet, so a
     wrong index is visible;
   - an injected `StalePlan` leaves the workbook usable and unchanged;
   - a refusal raised by the composed `CellWritePlan` (§2.1.1; a `vm` cell where a new table
     column's header goes) leaves every byte unchanged.
9. **Independent readers (CR)**: calamine 0.38 reads back values, merged regions and tables
   after each operation. Values match the moved input values, and formula cells whose caches
   §3.7 keeps match their input caches.
10. **Mutation checks**:
    - the range rule's boundary cases: `p == r1`, `p == r2 + 1`, `p == max − n`, sticky ends,
      clipping, and deletes touching one endpoint;
    - the shared-group step 3 comparison, including `#REF!` equality;
    - the re-anchoring;
    - the VML renumbering;
    - the `#REF!` prefix rule;
    - the in-place step order: a mutation that moves the strips after step 3.4 must fail
      (review round 2 m3).
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
    12. self-prefixed formulas, charts and sparklines in a copy; workbook names in a copy;
    13. an `.xlsm` copy without `codeName`: macros intact, Excel adds the document module;
    14. note boxes (VML `x:Anchor`) after an insert above them, and the
        `x:MoveWithCells`/`x:SizeWithCells` polarity.

## Delivery

A prerequisite PR and four PRs:

0. **P7-pre** (no new public G1/G2 API):
   - the location registry as data (`locations.rs`, with scopes) and the `EXTENSIONS` table,
     with their ADR ↔ data, scope and XSD-completeness tests;
   - the completeness scan of §2.2 reading them;
   - ADR-0009's rename and delete switched to `LOCATIONS` (the two sheet columns of §2.2),
     which extends them to the registry's formula rows on every sheet (DV, CF, x14,
     sparklines, table formulas, connection parameters, form controls), removing the
     refusals of the ADR-0009 implementation note;
   - **acceptance** (review round 2 M3, round 3 M2), over the corpus, in four clauses:
     1. every ADR-0009 rename and delete that succeeds today **in a workbook without
        `lockStructure`** gives byte-identical output and an equal `RewrittenLocation` list
        after the switch. That includes the 18 of 838 renames that retarget a relationship
        (L71);
     2. in the **4 workbooks with `lockStructure` (35 sheets)**, every rename, delete, add
        and move refuses with `Err(Protected)`, output byte-identical to the input. Those are
        `CVLKRA-KYC_Download_File_Structure_V3.1.xlsx` 30,
        `workbookProtection-workbook_password_user_range-2010.xlsx` 3,
        `workbookProtection-workbook_password-2013.xlsx` 1 and
        `workbookProtection_workbook_structure_protected.xlsx` 1 (spike
        `wb_lockStructure_sheets`);
     3. every operation that refuses today either still refuses with the same error, or
        succeeds and changes only registry places listed in ADR-0009's implementation note.
        Each changed place is checked against its expected rewrite (oracle 7's per-place
        check);
     4. "0 refusals in 838 renames" becomes **0 refusals other than `Protected` (35)**;
     - the ADR-0009 ↔ `LOCATIONS` test passes;
     - ADR-0009 gets a post-acceptance note recording the `lockStructure` refusal, which
       changes an Accepted ADR's behaviour;
   - the structured-reference sub-grammar moved to `formula/` with token spans;
   - `shift_area` (sticky ends, clipping) and the rewriters of §2.3, with unit tests;
   - a test that the shared-group check relies on: translation copies `#REF!` (already on
     main, `translate.rs` L47, L102);
   - the composed `CellWritePlan` variant (§2.1.1): the index map, and editing the
     operation's single staged `workbook.xml` instead of `apply_writes_with`'s own clone
     (folding `calcPr`/calcChain is already on main, `edit.rs` L470-489);
   - the `lockStructure` refusal for ADR-0009's sheet operations (§2.4);
   - core: a public bulk child removal on `LensMut` (§3.8).

   Needed by every later PR.
1. **P7a**: insert/delete rows and columns:
   - cells, and formulas on every sheet, **including CF, DV, x14, sparkline and table
     formulas on other sheets** (B1);
   - names; shared, array and data-table formulas;
   - `dimension`, `cols`, views, breaks, merges, hyperlinks, autofilter, CF/DV (2007 and
     x14), sparklines, `ignoredErrors`, `protectedRanges`, `customSheetViews`;
   - protection (Q13), calcChain and Phase1/Phase2 (§3.1–§3.5, §3.7);
   - the benchmarks (§3.8).

   Objects on the edited sheet that would change are refused (`StructureBlocked`) until P7b.
2. **P7b** (stacked on P7a; notes after P5c): tables, including column inserts through the
   composed `CellWritePlan` (Q11); notes; threaded comments; drawing and VML anchors; charts;
   pivot caches; `singleXmlCells`; connections (§3.6).
3. **P7c** (after P7-pre, in parallel with P7a): table rename and column rename (§5, §6).
4. **P7d** (after P7-pre and P5c): copy sheet (§4), including the `lockStructure` refusal
   (Q13).

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
- 3% of corpus worksheets (26 of 813) refuse every G1 edit: rows or cells without `r` (15),
  `oleObjects`/`controls`/AC (8), `dataConsolidate` (1). Protected sheets refuse the locked
  operations (7). 5% refuse copying. Sheets with a pivot table refuse only edits that cross
  or move it.
- Ranges pinned to the sheet edge (`col max="16384"` in 44 files) no longer block column
  inserts.
- **Behaviour change to existing operations** (Q13, review round 2 m8): ADR-0009's add,
  delete, rename and move sheet start refusing with `Protected` in the 4 corpus workbooks with
  `lockStructure`, where they succeed today.
- Every formula whose references touch the edited sheet loses its cached value, even when its
  value did not change. That is the price of a rule that is simple enough to be sound (§3.7).
- A G1 edit on a large sheet re-renders the open tags of every later row and cell; deferred
  rows (ADR-0010 §2) must allow a bulk renumber.
- Several behaviours depend on Excel checks (*u* items, Test oracles 11); the defaults refuse
  rather than guess wherever the result would be a stale or broken object.

## Open questions for the project owner

**Owner decisions 2026-11-20** (Q1–Q4, Q7, Q10, Q11, Q13). The other questions keep their
defaults, which the owner may still override. Q12 was changed to fail closed (review m9).

- **Q1. Format of inserted rows and columns. Decided: none.** New rows and columns are blank,
  with the default style; a `col` range around the insertion is split, not stretched (§3.4).
- **Q2. Objects in deleted rows or columns. Decided: refuse** a delete that fully contains an
  anchored object (`ObjectInDeletedRange`, §3.6.3).
- **Q3. Copy: self-prefixed formulas, charts, sparklines. Decided:** formulas keep their text
  (still the source); charts and sparklines are retargeted to the copy (§4.4). Both are XL
  item 12.
- **Q4. Copy: workbook-scope names that refer to the source. Decided: not duplicated;** only
  the source's local names are copied (§4.4).
- **Q5. Names of copied tables** (defaulted): source name without trailing digits + smallest
  free `n ≥ 2`.
- **Q6. Header-cell writes on referenced columns** (defaulted): keep `ColumnInUse`;
  `rename_table_column` is the rewriting path.
- **Q7. Pivot caches whose source range would grow or shrink. Decided: refuse** (§3.6.5).
- **Q8. Chart series and sparkline data ranges wholly deleted** (defaulted): refuse.
- **Q9. Shared groups that cannot stay whole** (defaulted): split by writing members' own
  formulas, with `#REF!` comparing equal (§3.3.2).
- **Q10. Performance target. Decided:** an insert on a 1M-cell sheet takes at most 2× the
  time of open + save of the same file. It is measured and published, not a hard CI gate at
  first (§3.8).
- **Q11. Inserting columns inside a table. Decided: add `Column{n}` columns,** as Excel does
  (§3.6.1, through §2.1.1).
- **Q12. Print area or print titles wholly deleted** (fail-closed default, review m9):
  **refuse** (`ReferenceDeleted`); the name is never dropped silently (§3.3.5). Excel's form is
  XL item 7.
- **Q13 (new). Protection. Decided: refuse** structural edits on a sheet whose protection
  disallows the operation (`insertRows`, `insertColumns`, `deleteRows`, `deleteColumns`),
  and `copy_sheet` when the workbook has `lockStructure`. New error `Protected`, no override
  option (§2.4).

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
  `sc/source/core/tool/token.cxx` L2809-3129 (sticky ends L2956, L2967, L3044), L3179-3420
  (value-changed entire columns L3338-3346), L3790-3869;
  `sc/source/core/tool/refupdat.cxx` L27-67 (`lcl_MoveEnd` clipping L49-67), L133-155,
  L187-282 (sticky end with a moved start L270-276);
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
  `index.rs`, `edit.rs` (L106-137 `CellWritePlan`/`StripPlan`/`strip_cells`, L438-443
  `refs_delta`); `crates/ooxml-core/src/xml/write.rs`, `xml/node.rs` (L68, L100-106),
  `lens.rs` (L1208, L1235), `package/add.rs` (`RegistryPlan`).
- [MS-XLSX] §2.2.4.12 "Worksheet" (the worksheet, `cfRule`, `sortState` and `filterColumn`
  extension lists), learn.microsoft.com page `07d607af-5618-4ca2-b683-6a78dc0d9627`, updated
  2026-11-18, as fetched by the round-2 reviewer.
- Corpus: POI `test-data/spreadsheet` at the POI commit above; calamine fixtures in
  `reference/fixtures/xlsx/calamine/`; synthetic fixtures in
  `reference/fixtures/xlsx/synthetic/`.

### P7-pre implementation note — sheet operations and bulk removal

The sheet-name dispatch matches qualified XML element paths and checks the
enclosing extension URI for x14 formulas. The schema audit checks 56 direct reference and formula declarations and four
elements derived from the formula type against registered locations or explicit
exclusions. The new formula locations splice
escaped text or attribute values into the staged XML. Existing typed writers
retain their report labels and ordering. The structural range helper retains the
original endpoint order and absolute flags, using spatial order for clipping and
pinning. A cell write plan requests the full-calc flag, which can be applied to
the operation's staged workbook document.

Core exposes `LensMut::remove_range` over child-item indices (including text and
markup). Invalid ranges return false and empty ranges succeed without dirtying
the tree. The fuzz oracle compares the bulk operation with descending
`remove_at` calls.
