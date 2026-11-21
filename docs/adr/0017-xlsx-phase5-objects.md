# ADR-0017: xlsx Phase5 — drawing-layer and object features

- Status: **Accepted** (revision 3; review round 2: Accepted after Minor only)
- Date: 2026-11-16
- Depends on:
  - [ADR-0001](0001-lossless-round-trip-architecture.md): Layer 1–3, T1/T1b/T2, append-only
    registries;
  - [ADR-0002](0002-schema-driven-code-generation.md): typed lens, allowlist codegen, §4.3
    schema-slot insertion and the `mc:AlternateContent` refusal;
  - [ADR-0003](0003-xlsx-phase1-formula-and-calc-state.md) (revision 5, Accepted, on main;
    section numbers refer to it; P5-pre amends §3.1's "A batch API is therefore not needed"):
    census, tokenizer,
    `Everything`, plan → validate → apply with poisoning, the potential spill area, the
    `RichValueCell`/`TableHeaderCell`/`UnknownCellMetadata` guards;
  - [ADR-0006](0006-xlsx-phase2-shared-strings-and-styles.md) (Accepted): registries,
    append-only with exact-form reuse;
  - **ADR-0009 revision 3** and **PR #38** (`ooxml-core` `RegistryPlan`): every
    package-structure edit is one staged registry transaction; the sheet-rename/delete
    location table;
  - [ADR-0014](0014-xlsx-phase4-range-features.md) (Accepted, revision 3, on main): range
    features, the per-edit allowed-diff lists, worksheet `extLst` handling.
- Roadmap: [`docs/roadmap/xlsx.md`](../roadmap/xlsx.md) §2.7 (F3–F7, F10, E9), §6 Phase5.
- Evidence: [`spikes/xlsx-phase5/`](../../spikes/xlsx-phase5/README.md) (corpus survey).
- Review: [round 1](0017-opus-review-round1.md), [round 2](0017-opus-review-round2.md)

## Revision history

- **Revision 1**: initial draft.
- **Revision 2** (after `0017-opus-review-round1.md`: 2 Blocker, 4 Major, 11 Minor). The
  owner decided Q1–Q7 on 2026-11-16 (all proposed defaults).
  - **B1:** everything that can refuse is decided before the first change.
    - A new **Phase1 `CellWritePlan`** (an ADR-0003 addendum, PR P5-pre) validates all cell
      writes of an operation at once, accumulating the census.
    - The calcChain removal and the `calcPr` change are folded into the operation's
      `RegistryPlan`.
    - Every `xml_part_mut` check runs before `RegistryPlan::new`.
    - After `apply` only checked insertions remain, with fault injection at each step.
    - §2.1 has a table per operation (§2.1).
  - **B2:** one **per-sheet shape-number allocator** over drawing `cNvPr` (all branches), VML
    `id` **and `o:spid`**, and `control`/`oleObject` `shapeId`. It is used for both drawing
    and VML shapes and moves into P5a. VML blocks are read from the bytes when the part is not
    XML (§2.2). The spike now counts `o:spid`: revision 1's rule would have collided in 3
    corpus files.
  - **M1:** deleting a table that refers to itself (totals, calculated columns), a non-default
    `tableType`, or a table with its own `.rels` is refused (§6.4).
  - **M2:** one function builds structured-reference column specifiers, with `'` escapes and
    the double-bracket form for edge whitespace; `ColumnInUse` matches every spelling on lexer
    tokens (§6.1, §6.3).
  - **M3:** the fallback note declares `xr`, `mc` and `mc:Ignorable="xr"` locally (§5).
  - **M4:** non-empty header, totals or calculated-column cells refuse unless
    `overwrite(true)`; `vm`/`cm` cells and shared masters in the range refuse (§6.1).
  - **Minor fixes, each in its section:**
    - m1: only table removal stages the sheet, as the plan's last step;
    - m2: `x15:tableSlicerCache/@tableId` counts for table ids;
    - m3: `metadata.xml` additions go into schema slots;
    - m4: image header rules;
    - m5: the allowed-diff table is complete;
    - m6: header sync is refused on query/XML/pivot/slicer-source tables and for empty names;
    - m7: `[@Col]` is converted on tokens, and `#`/`@` operators are refused;
    - m8: more `ChartNotRefreshable` cases, and the category-axis rule;
    - m9: the POI citation is fixed, and a removal with several matching note shapes is
      refused;
    - m10: GUID collision check, and `dT` as an XL item;
    - m11: sparkline ext order as an XL item.

- **Revision 3** (after `0017-opus-review-round2.md`: converged, Accepted after Minor only):
  - r2-m1: `ColumnInUse` parses structured-reference tokens with a defined sub-grammar,
    compares case-insensitively, and fails closed (§6.3);
  - r2-m2: the double-bracket rule follows Excel's list of special characters (§6.1);
  - r2-m3: allocator edge cases (§2.2);
  - r2-m4: what the `CellWritePlan` puts into the `RegistryPlan` and what it applies in place,
    and why this differs from ADR-0009 rev 3 (§2.1);
  - r2-m5: panic injection, coverage of the image, note and chart steps, `StalePlan` (§2.1);
  - r2-m6: allowed-diff rows (Test oracles 1);
  - r2-m7: references updated;
  - r2-m8: only edited trees built from the current state are staged (§2.1).

## Context

Phase0–4 edit cell values, formulas, registries, sheets and range features. Every object that
lives outside `sheetData` and outside the range-feature elements is still **Layer 1 pass-through**:

- pictures, shapes and charts in `DrawingsPart`s;
- notes (`comments` + VML) and threaded comments (+ `persons`);
- tables (`TableDefinitionPart`s);
- sparklines (worksheet `extLst`);
- rich values (images in cells) and dynamic-array cell metadata (`metadata.xml`).

Phase0 and Phase1 protect them with guards: a write to a `vm` cell is
`Err(RichValueCell)`, a write to a table header cell is `Err(TableHeaderCell)`, a `cm` that
is not an XLDAPR anchor is `Err(UnknownCellMetadata)`. Phase5 makes these objects **creatable**
and, where safe, **editable**, without giving up byte-exact preservation of everything else.

### Where they live

```
sheetN.xml ─ <drawing r:id>        → drawings/drawingN.xml ─┬ image → media/imageN.ext
           │                                                 ├ chart → charts/chartN.xml ─┬ chartStyle, chartColorStyle (optional)
           │                                                 │                            └ chartUserShapes, package (embedded data)
           │                                                 └ chartEx → charts/chartExN.xml (Excel 2016+)
           ├ <legacyDrawing r:id>  → drawings/vmlDrawingN.vml (note shapes, form controls, OLE icons)
           ├ (rel only)            → commentsN.xml (authors + commentList)
           ├ (rel only)            → threadedComments/threadedCommentN.xml ── personId → persons/person.xml (workbook rel)
           ├ <tableParts>          → tables/tableN.xml
           ├ <extLst> x14:sparklineGroups
           └ <c vm=… cm=…>         → metadata.xml (workbook rel) → richData/* (rich values) / XLDAPR (dynamic arrays)
```

Each kind has an **id space** that a new object must fit into:

| Id | Scope | Where |
|---|---|---|
| `xdr:cNvPr/@id` | one drawing part (groups and `mc:AlternateContent` branches included) | drawing |
| media part name | package | `xl/media/` |
| `table/@id` | workbook | every table part |
| `table/@name`, `@displayName` | workbook, shared with defined names, case-insensitive | table parts, `workbook.xml` |
| `tableColumn/@id` | one table | table part |
| comment author index | one comments part | `authors/author` |
| VML `o:idmap/@data` blocks | workbook (each block = 1,024 shape ids) | every VML part |
| VML shape id `_x0000_sN` | the sheet's idmap blocks | VML part |
| threaded comment `id`, `personId` | GUIDs; workbook | threaded-comment parts, `persons` |
| `metadataType` index, `cellMetadata`/`valueMetadata` block index | workbook | `metadata.xml`, `c/@cm`, `c/@vm` |

### What the corpus shows

`spikes/xlsx-phase5/objects_survey.py` over Apache POI `test-data/spreadsheet` at
`47dcff769bb395034dddfd59c4609659b6dbc6a4` (366 files) and the two calamine fixtures:
342 packages read (the rest are Strict, encrypted, fuzzing cases or not zip). Reproduction:
[spike README](../../spikes/xlsx-phase5/README.md).

| Topic | Finding |
|---|---|
| Drawings | 101 drawing parts in 62 files. Anchors: `twoCellAnchor` 181 (47 files), `oneCellAnchor` 30 (8), `absoluteAnchor` 9 (8: **8 in chart-sheet drawings**, 1 on a worksheet). `editAs`: `oneCell` 74, `absolute` 5, `twoCell` 1, absent (= `twoCell`) on the rest. |
| Drawing objects | `pic` 78 (21 files), `sp` 101 (18), `graphicFrame` 40 (27; all charts), `grpSp` 1. **`mc:AlternateContent` wraps whole anchors** 170 times in 4 files (163 in one form-heavy `.xlsm`); 1 inside an anchor. |
| `cNvPr/@id` | **No duplicate within any drawing** (0 of 101). Ids **repeat across drawings** in 8 files: the scope is the drawing part. In 44 drawings `max − count = 1` (Excel numbers from 2). 15 drawings have ids ≥ 1,025 that mirror VML shape ids (form controls). **`id="0"` in 4 drawings** (written by POI: `Diagramm0`, `Apache POI`; and by LibreOffice). |
| Media | png 54, jpeg 10, jpg 9, wmf 2, emf 2. One media part shared by several drawings in 3 files; identical bytes under two names in 1 file. No `r:link` (external) images, no SVG. |
| Notes | 31 comments parts in 27 files, 204 comments, 33 authors, no duplicate author, no bad `authorId`. Every `Note` VML shape has its comment and vice versa. 5 notes are visible. |
| VML | 42 parts in 37 files. **1 is not well-formed XML** (`BrNotClosed.xlsx`). Shapes: `Note` 204, `Pict` 93 (7 files), `Checkbox` 162 (1), `Button` 4, `Text` 1. **`o:idmap` blocks are unique across each workbook**, comma lists split (e.g. `45544`: 13, 16; `57181`: 5, 6, 7). Shape numbers come from `id="…_sN"` and from **`o:spid`** (243 `o:spid`s in 4 files; named form controls such as `CheckBox12` carry their number only there): 466 numbers, every non-negative one inside its sheet's blocks (2 are negative, `_x0000_s-1`). Sheets also hold 264 `control`/`oleObject` `shapeId`s, and drawing `cNvPr` and VML numbers coincide (mirrored controls) on 6 sheets in 5 files. No `legacyDrawingHF` (header/footer images) in the corpus. |
| Threaded comments | 3 files, 4 threads, no replies, no mentions; each has a `persons` part with one person (GUID id). The fallback note's author is `tc={thread id}` and its `xr:uid` is the thread id (`64759.xlsx`). |
| Tables | 61 tables in 29 files. **`id` unique workbook-wide in every file**; ids have gaps in 7 files (max − count from 1 to 22): ids are not compacted. `name ≠ displayName` in 1. No header row: 2. Totals row: 9 tables (6 files), 24 `totalsRowFunction`s. `calculatedColumnFormula` 16 (8 files). `tableType="queryTable"` 13, `"xml"` 5. Column ids not `1..N` in 14 tables. No two tables share a display name (case-insensitive); none equals a defined name. |
| Header cells | Every header cell's text equals its `tableColumn/@name`, **except 3 where the name is the ST_Xstring form**: header text `Handle⏎Bars`, name `Handle_x000a_Bars` (`ConditionalFormattingSamples.xlsx`). No header cell is a formula or missing. |
| Charts | 40 chart parts in 27 files; 8 chart sheets in 7 files. Types: bar 11, line 9, pie3D 8, scatter 6, area 2, pie 2, bar3D 2, line3D 1. 174 `c:f`, 40 with a quoted sheet name; 74 `numCache`; 24 `numRef` without a cache (2 files). Style/colors parts on 9 charts (8 files). `mc:AlternateContent` in 16 charts (13 files). **No chartEx** part. |
| Sparklines | 1 file (LibreOffice), 1 group, 11 sparklines; `x14` declared on the `ext`, not on the root. |
| Dynamic arrays | 1 anchor (`xlookup.xlsx`): `cm="1"`, `f t="array"`, `_xlfn.XLOOKUP`; `metadata.xml` with one XLDAPR type and one `cellMetadata` block. |
| Rich values | 1 file (calamine `pictures_in_cell_and_over_cell.xlsx`): 2 `vm` cells, 5 `richData` parts, `XLRICHVALUE` metadata. |

Conclusions:

1. **Every id space is respected by the files we must edit.** A new object only has to keep
   them so: max + 1 in the right scope, never reuse, never renumber.
2. **Excel numbers new things above the maximum and leaves gaps** (tables, `cNvPr`), so max + 1
   is indistinguishable from Excel's own allocation.
3. **VML is the fragile part**: it is not always XML (1 of 42), it mixes notes with form
   controls and OLE icons, and its ids are tied to workbook-wide blocks.
4. **Table column names are not header texts but their ST_Xstring encoding** (`_x000a_` for
   a newline). Header sync must encode.
5. **Threaded comments, sparklines, dynamic arrays and rich values are rare in the corpus**
   (1–3 files each). Their design rests on the libraries' output and on Excel checks, not on
   corpus statistics.

### Reference libraries

Verified in source (each fact cites file and line in the comparison below):
rust_xlsxwriter 0.102.0 (**RX**, `src/`), openpyxl 3.1.5 (**OP**, `openpyxl/`), Apache POI @
`47dcff76` (**POI**, `poi-ooxml/src/main/java/org/apache/poi/`).

- **RX** writes from scratch and supports every Phase5 object except threaded comments and
  chartEx. It is the reference for **what a new object looks like**.
- **OP** loads and rebuilds. It keeps charts and pictures it can model, **drops** shapes,
  threaded comments, persons, rich values, `metadata.xml`, `cm`, every worksheet `extLst`
  (sparklines) and unknown parts, and regenerates VML (`reader/drawings.py` L30-34, 53-54;
  `worksheet/_reader.py` L189-244, L324-329; `writer/excel.py` L53-110).
- **POI** edits a loaded package and keeps unknown parts as `POIXMLDocumentPart`
  (`ooxml/POIXMLFactory.java` L47-67). It is the reference for **editing an existing file**,
  and also for what goes wrong when ids are counted instead of scanned.

| Topic | RX | OP | POI |
|---|---|---|---|
| Image anchor | `twoCellAnchor` only (`absoluteAnchor` for chart sheets); `editAs="oneCell"` default for images (`drawing.rs` L56-66, L91-100; `image.rs` L260) | keeps a loaded anchor; new string anchor → `oneCellAnchor` sized from the image (`drawing/spreadsheet_drawing.py` L216-233) | `twoCellAnchor` only; `editAs` from the anchor type (`xssf/usermodel/XSSFDrawing.java` L201-215, L531-546) |
| `cNvPr/@id` | per drawing, from 2, images then charts then shapes (`drawing.rs` L56-64, L196-198) | per drawing from 1, renumbered on save (`spreadsheet_drawing.py` L280) | pictures: 1 + **number of anchors** (`XSSFDrawing.java` L572-575): collides after a deletion; charts: separate counter **from 0** (L87, L399-410) |
| Media | `xl/media/imageN`, workbook counter; dedup by (non-cryptographic) hash per category (`packager.rs` L968-1023; `image.rs` L941-944) | renumbered `imageN`, no dedup; non-PNG/JPEG/GIF re-encoded to PNG (`drawing/image.py` L49-65) | first free `imageN`, no dedup (`XSSFWorkbook.java` L611-654) |
| Images in cell | writes `vm`, `metadata.xml` `XLRICHVALUE`, 4 `richData` parts (`worksheet.rs` L19876-19895; `metadata.rs` L147-256; `rich_value*.rs`) | `vm` and the parts **dropped** | none |
| Notes | `commentsN.xml`; authors exact-match dedup; VML `o:idmap` block per sheet (+1 per 1,024 notes), shapes `_x0000_s{1024·block+k}` (`worksheet.rs` L16876-16890; `workbook.rs` L2813-2832; `vml.rs` L82-88) | VML regenerated with **`idmap data="1"` on every sheet** and ids from 1026; other VML shapes lost unless `keep_vba` (`comments/shape_writer.py` L33-34, L54-73) | reuses the sheet's VML; **`idmap data="1"` hard-coded**; ids = max existing + 1 (`XSSFVMLDrawing.java` L161-174, L208-250); duplicate note on a cell refused (`model/CommentsTable.java` L234-236) |
| Threaded comments | not supported | dropped | not supported |
| Table id, part | workbook counter from 1 = part number (`workbook.rs` L2934-2944) | renumbered on save `len(tables)` (`writer/excel.py` L232-237) | part count + 1, stepping past taken names; same number for `id` (`XSSFSheet.java` L4467-4486): **not max(id) + 1** |
| Table name | full rule set: characters, A1/R1C1 look-alikes, `TRUE`/`FALSE`, 255 units; unique vs tables and defined names, case-insensitive (`utility.rs` L986-1044, L1184-1265; `workbook.rs` L2950-2988) | only "no spaces"; unique case-insensitive vs tables and defined names (`worksheet/table.py` L184-193; `workbook/workbook.py` L425-437) | none on `setName`/`setDisplayName` (`XSSFTable.java` L381-443) |
| Header / columns | header cells written as strings; defaults `Column{n}`; names unique case-insensitive (`table.rs` L1249-1257; `worksheet.rs` L17457-17485) | names from header cells, no dedup (`worksheet/_writer.py` L266-276) | `updateHeaders` on every save, `_x000a_`/`_x000d_` escaping, no dedup (`XSSFTable.java` L811-855) |
| Totals, calculated columns | `SUBTOTAL(101…110,[Col])` in the cell; `[@Col]` → `[[#This Row],Col]` in `calculatedColumnFormula` and every data cell (`table.rs` L2020-2040; `formula.rs` L976-1011) | modelled, never computed | getters only |
| Table overlap | vs tables checked; vs merges not (`worksheet.rs` L9108-9125) | none | none |
| Charts | `chartN.xml`, caches from cell values at save, no style/colors parts, no `mc:AlternateContent`, no chartEx (`chart.rs` L4087-4160; `packager.rs` L351-632) | keeps a modelled subset; **drops** `extLst`, style/colors, `externalData`, user shapes (`chart/_chart.py` L125-150) | `chartN.xml`, caches from the formula evaluator; frame id from 0 (`xddf/usermodel/chart/XDDFChartData.java` L208-375) |
| Sparklines | x14 ext `{05C60535-…}`, `x14` declared on the `ext`, groups after x14 CF (`worksheet.rs` L20284-20459) | **dropped** with a warning | none |
| Dynamic arrays | `cm="1"`, `f t="array" ref=anchor`, `metadata.xml` XLDAPR; automatic `_xlfn.`/`_xlfn._xlws.` from a fixed table (`worksheet.rs` L19787-19838; `metadata.rs` L126-249; `formula.rs` L839-907, L1020-1191) | `cm` **dropped**: a spill becomes a legacy array formula (`worksheet/formula.py` L37-51) | none |

## Decision

### 1. Scope

In scope, on an opened workbook and on `Workbook::new()`:

- **Images (floating)**: insert a picture on a worksheet; list pictures; remove one we can
  identify. Formats PNG, JPEG, GIF, BMP, EMF, WMF; SVG with a PNG fallback.
- **Images in cells (read only)**: resolve a `vm` cell to its image bytes and alt text.
- **Notes**: add, edit the text of, and remove a legacy note.
- **Threaded comments**: add a thread or a reply, with its person and fallback note; remove a
  thread.
- **Tables**: create a table over a range (header row, totals row, column names, totals
  functions and labels, calculated columns, style); **header sync** replaces Phase0's
  `TableHeaderCell` guard; delete a table that nothing references.
- **Charts**: create bar, column, line, pie, scatter and area charts on a worksheet, or as a
  chart sheet; refresh the caches of existing charts on request.
- **Sparklines**: add sparkline groups.
- **Dynamic arrays**: write a dynamic-array (spilling) formula.

Out of scope:

- Shapes, text boxes, connectors, group shapes, form controls, OLE objects (created by none of
  our users' priorities; RX writes text boxes only).
- Editing existing charts beyond cache refresh; chartEx (`cx:`) creation; pivot charts.
- Writing images in cells (rich values): read only here. The `RichValueCell` guard stays.
- Header/footer images (`legacyDrawingHF`): absent from the corpus; deferred.
- Moving or resizing existing objects; renaming tables (needs the ADR-0009 rewrite machinery
  for structured references: Phase7).
- Threaded-comment mentions, resolved state, and editing someone else's comment.

### 2. Common machinery

#### 2.1 One plan per object operation (review B1)

Every operation follows ADR-0003 §3.1 plan → validate → apply. **Everything that can refuse is
decided before the first change**, and the only things left after `Package::apply` are
in-place insertions that were checked in step 1:

1. **Validate (read-only, before `RegistryPlan::new`).** Parse what is needed (sheet, drawing,
   comments, VML, table parts, `metadata.xml`). Compute ids and names (§2.2). Build the new XML
   trees. Check every in-place insertion with the lens's `check_insert_*` twins. For
   operations that write cells, build a **Phase1 multi-cell write plan** (`CellWritePlan`,
   below) for all of them at once. Every check that needs `xml_part_mut` (the lens's
   `check_insert_at` is only on `LensMut` today; Phase1 and Phase2 checks included) runs here,
   because `xml_part_mut` makes a `RegistryPlan` stale (PR #38).
2. **Registry plan.** Build one `RegistryPlan` containing:
   - `add_part` (`Numbered`), `add_relationship`, `add_default_content_type` (media);
   - `stage_part` for every existing **small** part whose content changes (drawing, comments,
     VML, table, `metadata.xml`, `persons`), and for `workbook.xml` when `calcPr` changes.
     **Only a tree the operation actually edits is staged, and it is cloned from the part's
     current state** (`xml_part`, which includes this session's earlier in-place edits), never
     from an older copy. An unedited clone is never staged: with PR #38 round 2's N1 such a
     stage would write the original entry and drop the session's edits (review r2-m8);
   - `remove_parts` for removals, **including `calcChain.xml`** when the cell writes require
     it (ADR-0003 §4.3 policy; `remove_parts(&[calcChain], &[REL_CALC_CHAIN])`, the same shape
     as ADR-0009 §2).

   Each step is validated against the virtual view. A refusal drops the plan: nothing changed.
3. **Apply.**
   - The workbook is poisoned, then `Package::apply(plan)` runs. Its only error is
     `StalePlan`, which it returns **before changing anything** (PR #38). So on `StalePlan` the
     poison is cleared and the error is returned as an ordinary `Err`: nothing changed, and the
     workbook stays usable (review r2-m5). A stale plan can only come from a bug in step 1's
     ordering, and a test injects one.
   - Then the in-place edits validated in step 1 follow, in a fixed order:
     1. element insertions in the sheet part (`<drawing>`, `<legacyDrawing>`,
        `<tableParts>`/`<tablePart>`, `extLst` content);
     2. the `CellWritePlan`'s in-place part: cell writes, cache strips (on this and other
        sheets), shared-formula hand-offs, and the `sst` appends of Phase2;
     3. the census and index updates.

   A failure here is a bug by construction. The workbook stays poisoned as in ADR-0003 §3.1
   (poisoned before apply, cleared on success), so a panic also poisons. **Fault injection**
   is tested before each of these steps, **as an `Err` and as a panic** (`catch_unwind`,
   ADR-0003 rev 5 R4-m6, ADR-0009 rev 3 §2). It covers table creation, dynamic arrays and
   header sync, plus one point each for image (`<drawing>`), note (`<legacyDrawing>`) and
   chart.

| Operation | Step 1 validates | Plan contains | In place after apply |
|---|---|---|---|
| image | anchor, media, `<drawing>` slot | drawing (new/staged), media, rels, CT | `<drawing>` (new drawing only) |
| note / thread | VML parseable, ids, `<legacyDrawing>` slot | comments, VML, threaded part, persons, rels, CT | `<legacyDrawing>` (new VML only) |
| table | range rules (§6.1), names, `CellWritePlan` of header, totals and column cells | table part, rels, CT, staged `workbook.xml` (`calcPr`), calcChain removal | `tableParts`, cell writes, census |
| header sync | `CellWritePlan` of one string cell, column rules (§6.3) | staged table part, `workbook.xml`/calcChain if dependents | the cell, census |
| chart | refs resolve, caches computed | chart part, drawing, rels, CT | `<drawing>` (new drawing only) |
| sparkline | refs lex and resolve, `extLst` slot | — | `extLst` content |
| dynamic array | `CellWritePlan` of the anchor, `metadata.xml` blocks | staged/new `metadata.xml`, rels, CT, `workbook.xml`, calcChain removal | the cell, census |
| removals | references (§3.2, §4.1, §5, §6.4) | `remove_parts`, staged parts, staged sheet (table only) | — |

**Prerequisite: Phase1 multi-cell write plan (an ADR-0003 addendum and its own PR, before P5b
and P5d).**
- `Workbook::plan_writes(Vec<(usize, CellIndex, CellOp, Extra)>)` validates the §3.1 guards
  for all writes in the operation.
- **Post-acceptance note (P5-pre).** The plan takes the union of dependents from the pre-batch
  graph and updates the census during apply. Cells whose values or formulas the plan replaces
  are excluded from the dependent-cell checks. An individual write after a formula write
  can therefore set `fullCalcOnLoad` or fail with `IterativeCalculation` in cases where
  the equivalent batch succeeds without that dependent-cell effect (ADR-0003 §3.1a).
- **What goes where** (review r2-m4).
  - Only the `calcPr` change (a staged `workbook.xml`) and the calcChain removal go into the
    `RegistryPlan`.
  - Strips, hand-offs, cell writes and `sst` appends are applied **in place after `apply`**,
    at positions validated in step 1. This avoids cloning large sheets (289 ms for a
    200k-formula sheet).
  - ADR-0009 rev 3 (R2-M2) instead applies its strips and `sst/@count` to staged clones,
    because a sheet operation already stages the parts it rewrites. Phase5's cell edits touch
    sheets nothing else stages.
  - P5-pre's API therefore returns positions for the in-place path, and can also apply them
    to a clone, for ADR-0009.
  - The `CellWritePlan`'s validation includes Phase2's `sst` checks: slot, exact-form reuse,
    and de-duplication of the same string appended twice within one plan.
- The usual refusals (`RichValueCell`, `UnknownCellMetadata`, `SharedFormulaMaster`, an
  `mc:AlternateContent` next to `calcPr`, …) therefore happen in step 1, with every byte
  unchanged. Regression tests cover a `vm` cell, a `cm` cell and a shared-formula master
  inside a new table's range.

**Removals.** Only table removal changes the sheet (its `tablePart`), so only it stages the
sheet part (review m1). Staging clones the tree (289 ms for a 200k-formula sheet, ADR-0003).
PR #38 shares staged trees between plan steps (`Arc`, "the copy is shallow"), so a staged
sheet is not cloned again. The sheet is still staged as the plan's **last** step, so that
every earlier step's reference scan sees the sheet's current `r:id`s and the removal is
validated against the final view (review r2-m7).
Picture, note and thread removals stage only the small parts they change.

Core follow-up (requested, not blocking): a read-only `Lens::check_insert_at`/
`check_insert_by`, so step 1 does not need `xml_part_mut` (also PR #29 review nit).

#### 2.2 Ids and names

Rules shared by every object. The scans run once per workbook, and the results are kept as
high-waters for the session: an id removed in the session is never handed out again.

**One shape-number allocator per sheet (review B2, part of P5a).**
- The drawing's `cNvPr/@id`s and the VML shape numbers share one number space per sheet.
  Excel mirrors a form control's VML `o:spid` in the drawing. `ExcelWithAttachments.xlsm`
  continues one counter from the drawing (1025–1031) into the VML (1032–1038).
- Shapes with a name for an `id` (`CheckBox28`) carry their number only in **`o:spid`**.
- The allocator's next number is 1 + the maximum of:
  - every `cNvPr/@id` in the sheet's drawing, inside groups and in every
    `mc:Choice`/`mc:Fallback` branch;
  - every number in a VML `id="…_sN"` **or `o:spid="…_sN"`** on any VML element of the
    sheet's `legacyDrawing` **and `legacyDrawingHF`** parts. A part that is not XML
    (`BrNotClosed.xlsx`) is scanned in its bytes with the spike's patterns; if that fails,
    every operation that allocates on the sheet is `Err(PartNotEditable)` (review r2-m3);
  - the sheet's `control/@shapeId` and `oleObject/@shapeId`, in every `mc:AlternateContent`
    branch (they usually sit in one: `mv-calculator…xlsm`);
  - the session high-water.
- Numbers that do not parse are ignored for the maximum (negative `_x0000_s-1`, a trailing
  space in LibreOffice's `id="_x0000_s1026 "`). A new id string must also not equal any
  existing one.
- The allocator is used for both new `cNvPr`s and new VML shapes. Never 0; at least 2 in a
  new drawing.
- **A VML shape needs a number inside the sheet's `o:idmap` blocks** (§4.2). Uniqueness is
  the requirement, not monotonicity (review r2-m3). The number is the first that applies of:
  1. the smallest number ≥ the allocator's value that is inside one of the sheet's blocks
     (`1024·b + 1 … 1024·b + 1023`) and equal to no existing number;
  2. otherwise the smallest unused number in the sheet's blocks;
  3. otherwise a new block (1 + max block of the workbook) appended to the sheet's `data`, and
     its first number.

  A drawing `cNvPr` above every block (a large drawing id) therefore does not push note
  numbers out of range.
- Corpus (spike re-run, `o:spid` included): 466 VML shape numbers. Every non-negative one is
  inside its sheet's blocks; 2 negative ones (`right-to-left.xlsx`) are not. 264
  `control`/`oleObject` `shapeId`s. Drawing and VML share numbers on 6 sheets in 5 files.
- Revision 1's rule (1 + max over `id="…_sN"` only, as POI's `XSSFVMLDrawing` L161-172) would
  have collided with an existing number in 3 files: `45540_form_Footer`/`_Header` (2139) and
  `mv-calculator-final-2-20-2013.xlsm` (1133).

| Id | Rule | Why |
|---|---|---|
| `cNvPr/@id` | the sheet's shape-number allocator (above) | one number space per sheet; never 0 (POI's chart counter writes 0) |
| `cNvPr/@name` | `Picture {n}`, `Chart {n}` (n = id − 1) unless the caller names it | Excel's and RX's form; names need not be unique |
| media part | `Numbered("xl/media/image{}.{ext}")` (PR #38: max + 1) | package scope |
| `table/@id` | 1 + max over every table part's `id`, every `x15:tableSlicerCache/@tableId` (review m2) and every id removed in the session | workbook scope; corpus gaps show Excel never compacts; slicer caches refer to table ids |
| table part | `Numbered("xl/tables/table{}.xml")` (need not equal the id) | PR #38 |
| table name | §6.2 | |
| comment author | exact string match among existing authors, else append | ADR-0006 exact-form reuse; RX and POI do the same |
| VML block | a sheet without VML gets **1 + max** over every block of every `o:idmap/@data` in the workbook (comma lists split). A VML part that is not XML has its `data` read from the bytes; if even that fails, allocating a new block is `Err(PartNotEditable)`. A sheet whose blocks are full gets a new block appended to its `data` list. | corpus: blocks unique per workbook; OP and POI write `data="1"` everywhere |
| VML shape | the allocator's rule for VML (above): a free number inside the sheet's blocks, else a new block | corpus: every non-negative number lies in its sheet's blocks |
| GUIDs (threads, persons) | random v4, upper-case, braced; checked against every existing thread and person id (review m10); a test hook injects a deterministic source (Q4) | Excel's form |
| `metadata.xml` indices | §9 | |

#### 2.3 Allowlist growth

New typed views come from the codegen allowlist (ADR-0002), each with its evidence:

- `dml.toml`/new `xdr`: `wsDr`, the three anchors, `from`/`to`/`pos`/`ext`, `pic`, `nvPicPr`,
  `cNvPr`, `cNvPicPr`, `blipFill`, `blip`, `stretch`, `spPr`, `xfrm`, `prstGeom`,
  `graphicFrame`, `clientData`;
- `dml.toml`/new `c`: the chart elements of §7 only;
- `sml.toml`: `comments`, `authors`, `commentList`, `comment`, `text`; `table`, `tableColumns`,
  `tableColumn`, `tableStyleInfo`, `autoFilter`, `calculatedColumnFormula`,
  `totalsRowFormula`; `metadata` and its blocks; `drawing`, `legacyDrawing`, `tableParts`,
  `tablePart`;
- Microsoft extension schemas (from `reference/openxml-sdk-data`): threaded comments
  (`ThreadedComments`, `threadedComment`, `personList`, `person`), x14 sparklines, `xda`.

VML is **not** schema-driven: §4.2.

#### 2.4 Refusals common to all objects

- The object's container is not parseable as XML: `Err(PartNotEditable { part, reason })`
  (e.g. the 1 VML part that is not well-formed).
- An insertion point is next to an `mc:AlternateContent` sibling in a schema-ordered parent
  (ADR-0002 §4.3): `Err(UnresolvedSibling)`. Appending a drawing anchor after AC-wrapped
  anchors is **not** refused: `wsDr` is a choice list, so order is z-order only.
- Sheet kinds: objects go on worksheets; charts also as chart sheets (§7.4).

### 3. Images

#### 3.1 Inserting a picture

```rust
ws.insert_image(Anchor::one_cell(at, Size::Natural), &Image::from_bytes(bytes)?)?;
ws.insert_image(Anchor::two_cell(from, to).edit_as(EditAs::OneCell), &img.alt_text("Logo"))?;
```

- **Format** from the magic bytes (PNG, JPEG, GIF, BMP, EMF, WMF; SVG needs a PNG fallback
  passed by the caller, stored as `a:blip` + `asvg:svgBlip` ext). Size and DPI from the header;
  unknown size → `Err(ImageSize)`. Bytes are never re-encoded (unlike OP).
- **Header parsing** (review m4). It is bounds-checked and never panics; it is a fuzz target.
  - DPI: a zero or absurdly small DPI, a PNG `pHYs` unit of 0 and a JFIF density unit of 0
    fall back to 96 dpi.
  - JPEG: every SOF marker (SOF0–SOF15 except DHT, JPG and DAC) is scanned; a height of 0
    (DNL) is `ImageSize`.
  - BMP: a negative height means top-down and its absolute value is used.
  - EMF: the size comes from `rclFrame` (0.01 mm).
  - WMF: only the placeable header is read; its units per inch are used.
  - EXIF orientation is not applied.
  - An `ext` above ST_PositiveCoordinate (27,273,042,316,900 EMU) is refused.
- **Anchor default: `oneCellAnchor`** with `ext` from the image's natural size (Owner
  decision 2026-11-16, Q1). It needs
  no column widths or row heights, so the size is exact. `twoCellAnchor` when the caller gives
  both cells (with `editAs`, default `oneCell` as Excel and RX); `absoluteAnchor` with EMU.
  The anchor cell must be inside the sheet's limits.
- **Drawing part**: the sheet's existing `<drawing r:id>` target, appended to (a new anchor is
  the topmost object); otherwise a new `xl/drawings/drawing{}.xml` with a `drawing`
  relationship and a `<drawing>` element inserted in its schema slot.
- **Media**:
  - identical bytes (SHA-256, then a byte compare) to an image added in this session → the
    same media part;
  - identical bytes to an **existing** media part that some drawing already embeds → reuse it
    (exact-form reuse, ADR-0006; corpus: 3 files share media between drawings), only for an
    internal target with an image content type and the same extension; hashes are computed
    lazily on the first insert (Owner decision 2026-11-16, Q2);
  - otherwise a new `xl/media/image{}.{ext}`. The extension's `Default` is added only when
    PR #38 allows it; otherwise the part gets an `Override`.
  - The drawing reuses its existing relationship to the same media part (POI does too).
- **XML**: `xdr:pic` with `nvPicPr/cNvPr id name descr` (alt text ≤ 255 UTF-16 units,
  ST_Xstring encoded), `cNvPicPr/a:picLocks noChangeAspect="1"`, `blipFill/a:blip r:embed` +
  `a:stretch/a:fillRect`, `spPr/a:xfrm` + `a:prstGeom prst="rect"`, then `xdr:clientData`.
  Optional `a:hlinkClick` (external relationship from the drawing).

#### 3.2 Reading and removing pictures

- `ws.images()` lists every anchor with a `pic` (top level or in a group): anchor, `cNvPr` id
  and name, alt text, media part name and bytes. Anchors inside `mc:AlternateContent` are
  listed as `Unresolved` (we do not pick a branch).
- `ws.remove_image(id)` removes one top-level anchor whose only object is that `pic`
  (staged drawing). Its relationship goes if nothing else in the drawing uses it, and the
  media part goes if `incoming_relationships` is then empty. An empty drawing stays (an
  empty `wsDr` is valid; removing the drawing too is a later refinement).

#### 3.3 Images in cells (read)

`CellRef::image()` follows `c/@vm` (1-based) → `valueMetadata` block → `rc t v` → the
`XLRICHVALUE` `futureMetadata` block → `xlrd:rvb i` → `rdrichvalue.xml` `rv[i]` → its structure
in `rdrichvaluestructure.xml` → the value at key `_rvRel:LocalImageIdentifier` → `richValueRel`
entry → relationship → media bytes; alt text from key `Text` when present. Any step that does
not resolve gives `Err(UnresolvedRichValue { step })`; a structure other than `_localImage`
gives `Ok(None)` (data types, web images). Writes to `vm` cells stay refused.

**Post-acceptance note (P5a).** The public XLSX API is `Worksheet::{insert_image,
images, remove_image}`, `Image::{from_bytes, from_svg}` and `CellRef::image`.
Insertion returns the allocated shape ID. Anchor positions use zero-based `CellIndex`
values and signed EMU offsets. SVG natural dimensions come from the caller's PNG
fallback. Listing uses explicit EMU extents for existing one-cell anchors and reports
`Unresolved` for compatibility-wrapped pictures or unreadable anchor coordinates.
External and missing media have no embedded bytes in the result. Removal refuses a
missing, grouped, compatibility-wrapped or ambiguous ID with `NoSuchImage`.

The worksheet drawing reference uses the generated worksheet slot. The drawing root
is an unordered choice for schema purposes, so its anchor is appended at the root's
closing span, including when the preceding anchor is in `mc:AlternateContent`. Removal
cuts only the selected anchor's original span. Both paths parse and stage the edited
drawing before applying the registry transaction. The shared shape scan includes
VML and worksheet control numbers. VML block allocation itself is exercised by note
creation in P5c.

### 4. Notes (legacy comments)

#### 4.1 API and rules

```rust
ws.add_note(at, Note::new("text").author("Ana").visible(false))?;
ws.set_note_text(at, "new text")?;
ws.remove_note(at)?;
```

- One note per cell: `add_note` on a cell that has one → `Err(NoteExists)` (POI too).
- A cell with a **threaded comment** refuses note edits and removal (`Err(ThreadedComment)`):
  the note is its fallback, and editing one without the other desynchronises them.
- **Comments part**: the sheet's `comments` relationship target, or a new
  `xl/comments{}.xml` (`Numbered`). Authors per §2.2. The new `comment` goes before the first
  existing comment that follows it in row-major order if the list is sorted, otherwise at the
  end; `shapeId="0"` as Excel writes. Text is a single run, ST_Xstring encoded, `xml:space`
  preserved when needed.

#### 4.2 VML

VML is not schema-driven, and Excel's VML is not always XML. The rules:

- The existing VML part is **parsed as XML** (Layer 2). If that fails, every note edit on the
  sheet is `Err(PartNotEditable)` (corpus: 1 of 42).
- New shapes are **appended** at the end of `xml`; existing shapes (form controls, OLE icons,
  other notes) are not touched. The note shape type `_x0000_t202` is added once if no
  `v:shapetype` with `o:spt="202"` exists; a shape type of another `spt` is never reused.
- `o:shapelayout/o:idmap/@data` gets a block per §2.2; a new VML part declares its block.
  The shape's `id` (and `o:spid`, which Excel writes for notes since 2010; XL item whether a
  note needs it) is `_x0000_s{N}` with N from the sheet's shape-number allocator (§2.2).
- The shape follows RX's note template (`vml.rs` L364-599): `v:shape id type="#_x0000_t202"`,
  style with `margin-left/top`, size 128×74 px, `visibility:hidden` (or `visible` + `x:Visible`),
  `fillcolor="#ffffe1"`, `v:fill`, `v:shadow`, `v:path`, `v:textbox`, and `x:ClientData
  ObjectType="Note"` with `x:MoveWithCells`, `x:SizeWithCells`, `x:Anchor` (RX's default
  placement rule, `note.rs` L649-726), `x:AutoFill`, `x:Row`, `x:Column`.
- Removal removes the `comment` and **the `Note` shape whose `x:Row`/`x:Column` match** (POI's
  `removeCellComment` does both; its `removeRow` forgets the shape). If no shape matches, the
  comment alone goes; if **more than one** `Note` shape matches, the removal is refused
  (`Err(PartNotEditable)`: we cannot tell which one is the note's; review m9). The comments
  part and VML part stay even when empty.
- Content type: all 37 corpus packages with VML declare a `vml` `Default`; when it is absent,
  PR #38 adds it (or an `Override`, under its safety condition).

### 5. Threaded comments

```rust
let p = wb.person("Ana Silva", PersonId::None)?;      // existing match or a new person
ws.add_comment(at, Comment::new("Looks wrong").by(p))?; // thread + fallback note
ws.reply(at, Comment::new("Fixed").by(p))?;
ws.remove_comment_thread(at)?;
```

- **Persons**: `xl/persons/person.xml` with a workbook relationship of type
  `…/2017/10/relationships/person`, created on first use. A person is reused when
  `displayName`, `userId` and `providerId` are all equal; otherwise appended with a new GUID.
  `providerId="None"` and `userId = displayName` for local people (Excel's offline form;
  XL item).
- **Thread**: `xl/threadedComments/threadedComment{}.xml` per sheet (`Numbered`), relationship
  type `…/2017/10/relationships/threadedComment`; `threadedComment ref dT personId id` +
  `text`. Replies carry `parentId` and are inserted after the last comment of the thread.
  `dT` is the caller's timestamp or now, without zone (Excel's form).
- **Fallback note** (required: Excel versions without threaded comments show it, and Excel
  pairs the two): a legacy note on the same cell (§4) whose author is `tc={thread id}` and whose
  `comment/@xr:uid` is the thread id (corpus `64759.xlsx`); its text is
  `"[Threaded comment]\n\n" + the thread's texts` (Q3, a constant). Each reply updates the
  note text.
- **`xr` declarations** (review M3). 26 of the 31 corpus comments parts declare neither `xr`
  nor `mc:Ignorable`. The new `comment` element therefore declares **locally**
  `xmlns:mc`, `xmlns:xr` and `mc:Ignorable="xr"` (MCE allows `Ignorable` on any element), unless
  the root already declares `xr` with that URI and lists it in `mc:Ignorable`. The root is not
  changed (as for sparklines, §8). If `xr` is bound to another URI in scope, `add_comment` is
  refused (`Err(PartNotEditable)`). XL item: an Excel 2010-class reader opens the result.
- `dT` is written as Excel does (no zone); XL item: whether Excel treats it as UTC or local
  time (review m10).
- A cell that already has a legacy note but no thread refuses `add_comment`
  (`Err(NoteExists)`): converting a note is Excel's UI action, not ours.
- Removal removes the thread's comments and the fallback note together (one plan).

### 6. Tables

#### 6.1 Creating a table

```rust
ws.add_table(Table::new(range).name("Sales")
    .columns([Col::new("Region"), Col::new("Q1").total(Total::Sum), Col::new("Δ").formula("[@Q1]-[@Q2]")])
    .totals_row(true).style(TableStyle::Medium9))?;
```

Refused (`Err(TableRange { reason })`) when the range:

- overlaps another table (any part), a merged range, a sheet `autoFilter`, a pivot table's
  location, or crosses the boundary of an array formula or a dynamic-array potential spill
  area (ADR-0003);
- contains a `vm` (rich value) or `cm` cell, or a shared-formula master (review M4);
- has fewer rows than header + totals + 1 data row, or is outside the sheet's limits.

**Existing content** (review M4). A header, totals or calculated-column position that holds
anything (value, formula, inline string) is refused with
`TableRange { reason: "cell not empty" }`. The one exception is a header cell whose string is
already the column's name. With `Table::overwrite(true)`, those cells are cleared and
rewritten through the `CellWritePlan` instead. Their dependents (a formula reading a header
cell by A1 reference) lose their caches as for any Phase1 write. Every cell write of the
operation is validated in step 1 (§2.1).

What is written:

- **Table part**: `table id name displayName ref [headerRowCount="0"] [totalsRowCount="1" |
  totalsRowShown="0"]`, `autoFilter ref` (header rows only, excluding the totals row; RX),
  `tableColumns` with `tableColumn id name [totalsRowFunction | totalsRowLabel]
  [calculatedColumnFormula]`, `tableStyleInfo` (default `TableStyleMedium9`, row stripes on, as
  RX and Excel).
- **Sheet**: `<tableParts count>` with a new `<tablePart r:id>` (created in its slot or
  appended to, `count` updated); the relationship; header cells; totals-row cells; calculated
  column cells.
- **Header cells** are written as strings through Phase2's string path. Their value is the
  column name, **decoded** from ST_Xstring (§6.3).
- **Totals row**: a function writes `SUBTOTAL(10x,Table[Col])` into the cell (109 sum, 101
  average, 103 count, 102 countNums, 104 max, 105 min, 107 stdDev, 110 var; RX's codes) through
  the `CellWritePlan`; a label writes the string.
- **Calculated column**: the caller's text is lexed with Phase1's lexer.
  - Each `[@Col]` **token** becomes the file form `Table[[#This Row],[Col]]`, with the table
    name included (unlike RX). This is not a character-level replacement as RX does, so `@` in
    string literals and names is untouched (review m7).
  - A spill operator (`A1#`) or an implicit-intersection `@` outside a structured reference is
    refused. The file forms are `_xlfn.ANCHORARRAY`/`_xlfn.SINGLE`, which the caller may write.
  - The result is stored in `calculatedColumnFormula` and in every data cell.
- **Structured-reference specifiers** (review M2). One function builds every column
  specifier that we write:
  - `'`, `#`, `[` and `]` in a column name are escaped with a leading `'` (RX `table.rs`
    L2021-2026);
  - a column specifier is written in the **double-bracket form** `Table[[Col]]` when the name
    has leading or trailing whitespace, or any of Excel's special characters for structured
    references (Microsoft, "Using structured references with Excel tables"): tab, LF, CR,
    `,` `:` `.` `[` `]` `#` `'` `"` `{` `}` `$` `^` `&` `*` `+` `=` `-` `>` `<` `/`
    (review r2-m2). LibreOffice writes `Table[[ Sum for Q2 ]]` (`LIBRE_OFFICE-128382-0.xlsx`,
    `63934.xlsx`). Inner spaces alone keep the single-bracket form, as Excel writes
    `T[Sales Amount]`;
  - inside a multi-part reference (`Table[[#This Row],[Col]]`) every part is bracketed anyway.

  XL item: totals over columns whose names contain `,`, `:` or `$`.

  Totals, calculated columns and the `ColumnInUse` match (§6.3) all use it. Every generated
  formula round-trips through Phase1's lexer (tested, with names from those two files).
- **Phase1**: the new formula cells are census entries; structured references are
  `Everything` precedents (ADR-0003 §2.4). Because a new table name can make a previously
  unresolved name resolve, every `Everything` formula is seeded (as ADR-0009 M3 for new
  sheets), and `fullCalcOnLoad` is set.

#### 6.2 Names

- **Characters and form**: RX's `check_name` rules (`utility.rs` L986-1044, L1184-1265): 1–255
  UTF-16 units; first character a letter, `_` or `\`; no space or punctuation other than `_`,
  `.` and `\`; not `TRUE`/`FALSE`; not an A1 or R1C1 reference or `R`/`C`/`RC`; not `_xlnm.`.
  Violations: `Err(InvalidTableName { reason })`. Names are never altered.
- **Uniqueness**: against every table's `name` and `displayName` and every defined name of any
  scope, case-insensitively with ADR-0003 §2.4's fold (`Err(NameInUse)`).
- **Default**: `Table{n}` with the smallest n that is free (POI; the name is not an id, so
  smallest-free is harmless here).
- `name` and `displayName` are written equal.

#### 6.3 Column names and header sync

- **From the caller**, or from the existing header cells when the caller gives none (strings
  as is; numbers, booleans and dates as Excel displays them would need the number format
  engine, so they are **refused**: `Err(HeaderNotText)`), or `Column{n}` for empty cells.
- **Unique case-insensitively** (`Err(DuplicateColumnName)`; Q5): never suffixed silently.
- **Encoding**: `tableColumn/@name` is the ST_Xstring encoding of the header text
  (`_x000a_` for a newline, `_x005f_` for a literal `_x`…`_` sequence). The corpus shows the
  newline form. POI escapes only `\n` and `\r` (`XSSFTable.java` L837-838); the `_x005f_`
  rule is ECMA-376's (review m9). A header text containing control characters other than tab/newline/carriage return
  is refused.
- **Header sync** (replaces `Err(TableHeaderCell)` for strings): `set_string` on a header
  cell updates the cell **and** `tableColumn/@name` in one plan (validated unique; the table
  part staged). Writing a number, formula or blank into a header cell stays refused
  (`TableHeaderCell`), since Excel would turn it into text and rename the column. A column
  name change does **not** rewrite structured references that use the old name: refused with
  `Err(ColumnInUse)` when any formula location (ADR-0009 §6 table) mentions the column.
  How a mention is found (review r2-m1; it fails closed):
  - Phase1's lexer keeps a structured reference as **one opaque token** (ADR-0003 rev 5 §2.1).
    A sub-grammar parses its inside:
    - an optional table name, then `[` … `]`;
    - inside, either one column, or a comma list of special items (`#All`, `#Data`,
      `#Headers`, `#Totals`, `#This Row`, and `@` as `#This Row`) and columns or column ranges
      (`[Q1]:[Q3]`);
    - each bracketed element with its `'` escapes removed;
    - the forms `[@Col]` and `[@[Col with space]]`.
  - The tokens checked are those whose table name is the table (compared with ADR-0003
    §2.4's fold), and table-less ones in formulas inside the table's range.
  - Each column element, **and both endpoints of a column range**, is compared with the old
    name **case-insensitively** with the same fold: `T[sales]` refers to `Sales`.
  - **Any structured-reference token the sub-grammar cannot parse refuses the rename**
    (`ColumnInUse`).
  - Tests: case differences, column ranges, `[@[…]]`, `#Totals`, escapes, and an
    unparseable token.
  - Renaming referenced columns is Phase7.
- **Not synced** (review m6):
  - tables with `tableType="queryTable"` (13 in the corpus) or `"xml"` (5), whose columns are
    bound to `queryTableField`/`xmlColumnPr`;
  - tables that are the source of a pivot cache or a slicer;
  - a change to the empty string (Excel would rename the column to `ColumnN`).

  Each is refused (`TableHeaderCell`).

#### 6.4 Deleting a table

`ws.remove_table(name)` is refused with `Err(TableInUse)` in any of these cases:

- any formula location, pivot source, slicer cache or data validation uses it (ADR-0009 §5's
  check), **including the table's own cells** (review M1). Totals rows (`SUBTOTAL(109,T[Q])`)
  and calculated columns (`T[[#This Row],[Q]]`, `T[[#Totals],[Total]]`, `bug66215.xlsx`) refer
  to their own table. Excel's "convert to range" rewrites them to A1 references, which is an
  XL item and a later option; leaving them would dangle;
- `tableType` is not the default `worksheet` (`queryTable`, `xml`);
- the table part has its own `.rels` (a query table).

Otherwise:

- the table part goes (`remove_parts`, with the sheet staged as the plan's last step);
- the `tablePart` element goes, and `tableParts` with its last child;
- the cells stay as plain cells;
- the id goes into the session high-water.

**Post-acceptance note (P5b).** The XLSX API is
`Worksheet::{add_table, remove_table}` with `Table`, `Col`, `Total` and `TableStyle`.
Creation returns the display name. Table styles select a built-in style family and
number, or omit the style name. The same header synchronization runs for
`set_string`, `set_string_inline` and string entries in `write_cells`; batch uniqueness
is checked against the resulting column names. Existing multi-row-header tables
refuse synchronization because one column name cannot track multiple header cells.
An unknown name passed to `remove_table` returns `NoSuchTable`. The workbook-wide
Phase1 metadata refusal may take precedence over the range refusal for unknown `cm`.

Removal stages the sheet before removing the part: the current core
`RegistryPlan::remove_parts` checks incoming relationship ids against the sheet
tree already staged in the plan. The plan still publishes all changes together.
The manual check includes totals over a column name with punctuation and a
header rename in a different column. The writer also escapes a literal `@` in a
column specifier so it cannot be read as a row selector.

Calculated-column formulas qualify local structured references with the table name
and expand the row selector to `[#This Row]`. Invalid structured references and
unknown columns in the new table return `InvalidFormula`. Header renames check
references against the cell writes in the same batch. Table removal also checks
for a bare table-name token when a formula cannot be lexed.
The LibreOffice workflow added in #82 checks `39-p5b-table.xlsx` against the
manual-check manifest. The [LibreOffice verification guide](../testing/libreoffice-check.md)
shows how to run the separate openpyxl table read-back.

### 7. Charts

#### 7.1 What a new chart is

```rust
let c = Chart::new(ChartType::Column)
    .series(Series::values("Sheet1!$B$2:$B$6").categories("Sheet1!$A$2:$A$6").name("Sheet1!$B$1"))
    .title("Sales");
ws.insert_chart(Anchor::two_cell(from, to), &c)?;
wb.add_chart_sheet("Chart1", &c, position)?;
```

- Types: bar, column (bar with `barDir="col"`), line, pie, scatter, area. Stacking, markers,
  axis titles, legend position, data labels. Other types and 3-D: out of scope.
- **Part**: `xl/charts/chart{}.xml` (`Numbered`), content type `…drawingml.chart+xml`, a
  `chart` relationship from the drawing, a `graphicFrame` anchor (`cNvPr` per §2.2,
  `a:graphicData uri=".../chart"` + `c:chart r:id`). No `chartStyle`/`chartColorStyle` parts
  and no `mc:AlternateContent` (as RX; 31 of 40 corpus charts have no style part).
- **References**: `c:f` written absolute with ADR-0009's sheet-name quoting rules; the sheet
  must exist (`Err(NoSuchSheet)`); every reference lexes with the Phase1 lexer.
- **Caches**: `c:numCache`/`c:strCache` from the current cell values (Phase0 reads):
  numbers as is; strings in `strCache`; a formula cell with a cached value contributes it;
  a formula cell **without** a cache (stripped by Phase1) and any non-numeric value in a
  number series is written as a missing point (`ptCount` covers it, no `pt`), never as 0
  (RX writes 0). `formatCode` `General`.
- **Category axis** (review m8): `c:cat` is a `c:numRef` when every category cell is a number
  (or a date), otherwise a `c:strRef`. Scatter X values are always `c:numRef`.
- The chart XML is built from a fixed template per type (RX's element order), validated by
  the allowlist's typed views.

#### 7.2 Cache refresh for existing charts

The roadmap's stale-cache problem (Phase0 documents it): `wb.refresh_chart_caches()` recomputes
the `numCache`/`strCache` of every chart whose `c:f` resolves (Phase1 resolution) to cells we
can read, on staged chart parts, under §7.1's rules. It is **explicit, not automatic** (Q6):

- Excel recomputes linked caches on open (believed, XL item), so automatic refresh would
  rewrite chart parts on every edit for a benefit only to other readers;
- a refresh changes only `c:pt`/`c:ptCount`/`c:v` inside the cache elements (allowed-diff
  list, Test oracles 1). It refuses (`Err(ChartNotRefreshable)`) a chart with any of:
  - `c:externalData`, a pivot source, `c15:` filtered series, references to other workbooks;
  - a cache inside `mc:AlternateContent` (16 corpus charts have AC) (review m8);
  - a `c15:datalabelsRange` cache;
  - a `c:multiLvlStrRef`;
  - a `c:f` that is a defined name, a table reference or a whole column/row.

#### 7.3 chartEx

`cx:` parts are never created. Existing ones stay L1; their formulas are in ADR-0009's rename
table (read only for Phase5).

#### 7.4 Chart sheets

`add_chart_sheet` uses ADR-0009 §4 (sheet allocation, `workbook.xml`, `app.xml`) with a
`chartsheet` part (`sheetPr`, `sheetViews`, `drawing r:id`) and a drawing with one
`absoluteAnchor` (the corpus's 8 of 8 chart-sheet drawings; RX `drawing.rs` L1184).

### 8. Sparklines

```rust
ws.add_sparklines(SparklineGroup::new(SparklineType::Line).add("G3", "'Basic features'!B3:E3"))?;
```

- Container: the worksheet `extLst` → `ext uri="{05C60535-1F16-4fd2-B633-F4F36F0B64E0}"` →
  `x14:sparklineGroups`. An existing ext with that uri gets the new group **appended**;
  otherwise a new `ext` is appended at the end of `extLst` (`extLst` created in its slot if
  absent). Existing exts and their order are untouched (ADR-0014's x14 rule).
  - Appending at the end is valid by the schema. Excel writes its exts in the order
    CF → DV → sparklines → slicers. XL item: whether Excel cares where a new sparkline ext
    sits among existing ones (review m11).
- **Namespaces are declared on the new `ext`** (`xmlns:x14`) and on `x14:sparklineGroups`
  (`xmlns:xm`), as LibreOffice (corpus) and RX do. The worksheet root, its `mc:Ignorable` and
  its declarations are not changed: T2 confined to `extLst`.
- `xm:f` (data range, sheet-qualified, quoted by ADR-0009's rules) and `xm:sqref` (one cell);
  data ranges lex with the Phase1 lexer and the sheet must exist. Options: type
  (line/column/stacked), markers, high/low/first/last/negative, colours (ARGB), axis, empty
  cells as gap/zero/span.
- Phase1: sparklines hold no cache; Phase3 rename/delete must rewrite `xm:f` (already in
  ADR-0009's table).

### 9. Dynamic arrays

```rust
ws.cell_mut(r, c)?.set_dynamic_formula("SORT(A2:A10)")?;
```

- The cell gets `cm="{k}"` and `f t="array" ref="{anchor}"` with the formula text and **no
  cached value** (the spill range is unknown until Excel computes it; RX writes `ref` = anchor
  too). `fullCalcOnLoad` is set (Phase1 policy).
- **`metadata.xml`**, append-only:
  - absent → a new part (`…spreadsheetml.sheetMetadata+xml`, workbook relationship
    `sheetMetadata`) with RX's XLDAPR content (one `metadataType`, one `futureMetadata` block,
    one `cellMetadata` block), and `cm="1"`;
  - present → reuse an existing `cellMetadata` block whose `rc` points at the XLDAPR
    `metadataType` and at a `futureMetadata` block with `fDynamic="1"`, `fCollapsed="0"`
    (ADR-0003's anchor test), and use its index as `cm`;
  - otherwise add the missing type, future block and cell block, keeping existing indices
    and updating the `count` attributes.

  Additions go into **schema slots** through the lens (review m3), not at the end:
  - `futureMetadata name="XLDAPR"` after the existing `futureMetadata`;
  - `cellMetadata` **before** an existing `valueMetadata`. The calamine rich-value fixture has
    `valueMetadata` and no `cellMetadata`, and is a test case.

  `xmlns:xda` is declared on the new `ext`; the root is not changed.
- **Future-function prefixes** (Owner decision 2026-11-16, Q7).
  - The text is the file form.
  - `set_dynamic_formula` (and, from now on, `set_formula`) **refuses** a call to a known
    future function written without its prefix: `Err(MissingFunctionPrefix { name, prefix })`.
    So Excel never shows `#NAME?`.
  - The prefix table (`_xlfn.`, `_xlfn._xlws.` for FILTER and SORT, `_xlpm.`) is a fixed copy
    of RX's `future_functions` (`formula.rs` L1020-1191).
  - An opt-in helper `formula::add_future_prefixes(text)` applies the prefixes.
  - Applying the check to `set_formula` changes Phase1's contract, so it is an ADR-0003
    addendum together with the `CellWritePlan` (§2.1). The spill (`#`) and implicit-intersection
    (`@`) operators are refused there too (review m7).
- **Phase1**: the new anchor is a dynamic anchor in the census; its potential spill area
  (anchor to the sheet's end) is seeded; writes into it keep `Err(ArrayResultCell)` only for
  existing cached spill ranges (ADR-0003). A spill blocked by existing values is Excel's
  `#SPILL!`, not our error; it is documented.

### 10. Interaction with Phase1–4

| Phase | Effect |
|---|---|
| Phase1 (ADR-0003) | Header, totals and calculated-column cells and dynamic arrays go through the `CellWritePlan` (§2.1; an ADR-0003 addendum): census, dependents, `fullCalcOnLoad`, and the calcChain removal folded into the `RegistryPlan`. A new table name seeds every `Everything` formula. Images, notes, threads and sparklines do not touch formulas. |
| Phase2 (ADR-0006) | Header and label strings use the shared-string registry. No style is created by Phase5, except a table's cells keep their existing `s`. |
| Phase3 (ADR-0009) | Delete/rename cover every Phase5 part through the cascade and location tables; The table-id high-water introduced in §2.2 (ADR-0009 rev 3 keeps high-waters only for `sheetId` and `rId`) also applies to tables removed by ADR-0009's sheet deletion, and Phase5's self-reference rule extends its `TableInUse` check (§6.4). Chart sheets use §4. |
| Phase4 (ADR-0014) | A table refuses an overlapping sheet `autoFilter` and merges; ADR-0014's merge and autofilter operations refuse table ranges. `extLst` handling is shared (§8). |

### 11. Errors

| Error | When |
|---|---|
| `PartNotEditable { part, reason }` | VML/drawing/comments/table/metadata part not parseable or with structure we refuse |
| `ImageFormat`, `ImageSize` | unrecognised bytes; no size |
| `NoteExists`, `ThreadedComment`, `NoSuchNote` | §4, §5 |
| `TableRange { reason }`, `InvalidTableName { reason }`, `NameInUse`, `DuplicateColumnName`, `HeaderNotText`, `ColumnInUse`, `TableInUse` | §6 |
| `ChartNotRefreshable { part, reason }`, `NoSuchSheet` | §7 |
| `MissingFunctionPrefix { name, prefix }` | §9 |
| `UnresolvedRichValue { step }` | §3.3 |
| `TableHeaderCell` | non-string write into a header cell (kept), or header sync refused (§6.3) |
| `RegistryRefused { reason }` | from PR #38 |

## Test oracles

1. **Locality** (byte-exact, ADR-0014 §2): every part the operation does not list is L1. The
   per-operation allowed-diff list:

   | Operation | Sheet part | Other parts |
   |---|---|---|
   | image | `<drawing>` inserted (new drawing only) | drawing (new, or one anchor appended), drawing `.rels`, media part, CT (`Default`/`Override` appended); **sheet `.rels`** for a new drawing (review m5) |
   | note | `<legacyDrawing>` inserted (new VML only) | comments (one `comment`, maybe one `author`), VML (one shape, maybe shapetype and idmap), sheet `.rels`, CT |
   | thread | as note | + threaded-comment part, `persons` (one person), workbook `.rels`, **two CT `Override`s** |
   | table | `tableParts`/`tablePart`, header/totals/column cells, their dependents' caches on this sheet | table part, sheet `.rels`, CT, `sst` (Phase2 appends and `count`), **`workbook.xml` `calcPr`, calcChain removed** (when formula cells are written), **other sheets with dependents (`v`/`is` removed only)** (review r2-m6) |
   | header sync | the header cell, its dependents' caches on this sheet | the table part (`tableColumn/@name`), `sst` (append and `count`), `workbook.xml`/calcChain if there are dependents, other sheets with dependents (`v`/`is` removed only) |
   | chart | as image | chart part, drawing, rels, CT |
   | chart sheet | — | chartsheet part, drawing, chart, **`workbook.xml`, workbook `.rels`, `app.xml`**, CT |
   | sparkline | `extLst` only | — |
   | dynamic array | the cell, its dependents' caches on this sheet | `metadata.xml` (blocks in their slots), workbook `.rels`, CT, **`workbook.xml`, calcChain removed**, other sheets with dependents (`v`/`is` removed only) |
   | cache refresh | — | `c:numCache`/`c:strCache` content of the refreshed charts |
   | remove image | — | the drawing (one anchor gone), its `.rels` and the media part when unreferenced, CT `Override`s of removed parts |
   | remove note / thread | — | comments (one `comment`), VML (one shape), threaded part (the thread's comments) |
   | remove table | `tablePart` (and `tableParts`) | the table part and its CT `Override`, sheet `.rels` |

   Appended registry entries are checked as "original bytes + appended entries", as PR #38's
   tests do.
2. **Id-space checker**, independent of the allocators: it is written from §2.2's
   *invariants*, not from its algorithms. It checks that:
   - per drawing, `cNvPr` ids are unique and ≠ 0;
   - **per sheet, `cNvPr` ∪ VML numbers (`id` and `o:spid`) are unique** (review B2), except
     for an existing mirrored pair;
   - table ids are unique workbook-wide, and new ones are above every pre-existing id and
     slicer `tableId`;
   - table display names are unique together with defined names;
   - VML blocks are unique per workbook, and every new shape number lies in its sheet's
     blocks;
   - every `personId` resolves, and every thread has its fallback note with a matching
     `xr:uid` **whose `xr` is declared and ignorable** (review M3);
   - every generated structured reference round-trips through the lexer;
   - every `cm`/`vm` resolves in `metadata.xml`, and `metadata.xml`'s children are in schema
     order.
3. **Corpus sweep**: for every readable POI package, add one of each object to the first
   worksheet (on a free range), save, reopen, and run (1) and (2). A refusal leaves every byte
   unchanged. Explicit cases:
   - the VML that is not XML, and the AC-anchor files;
   - `mv-calculator-final-2-20-2013.xlsm`, `45540_form_*.xlsx` and `ExcelWithAttachments.xlsm`
     (B2);
   - `bug66215.xlsx` (M1);
   - `LIBRE_OFFICE-128382-0.xlsx` and `63934.xlsx` (M2);
   - the calamine rich-value fixture (m3).
   - **Failure atomicity** (reviews B1, r2-m5):
     - Fault injection, as an `Err` and as a panic, before each in-place step after `apply`
       poisons the workbook. This covers table creation, dynamic arrays and header sync, plus
       the `<drawing>` and `<legacyDrawing>` insertions of image, chart and note.
     - An injected `StalePlan` returns an ordinary `Err` with the workbook unpoisoned and
       unchanged.
     - A `vm` cell, a `cm` cell or a shared-formula master in a new table's range is refused
       **before** apply, with every byte unchanged.
4. **Independent readers**: calamine 0.38 reads back tables (`load_tables`, `table_by_name`:
   name, columns, data range) and pictures (`pictures`); `ooxmlsdk`'s parse of every created
   part (roadmap Phase5 acceptance); created parts compared structurally with RX's output for
   the same calls (RX as a dev-dependency oracle: element names, order and attributes, ids
   normalised).
5. **Formula round trip**: every written formula (totals, calculated columns, chart and
   sparkline refs, dynamic arrays) lexes with the Phase1 lexer and the census picks it up.
6. **Mutation checks** on the allocators and the encoders (ST_Xstring, quoting).
7. **Excel checks (XL, owner)**: open without repair and save back, for each object on a
   fresh workbook and on POI fixtures (`64759.xlsx`, `53568.xlsx`, `57181.xlsm`, `45544.xlsx`,
   `ConditionalFormattingSamples.xlsx`, `xlookup.xlsx`): picture positions, note visible on
   hover, thread with replies and fallback note, table with totals and calculated column
   (structured refs work), chart renders with refreshed caches, sparklines render, dynamic
   array spills. Record what Excel writes back (person `providerId`, fallback text, `dT`).

## Delivery

A prerequisite PR plus five PRs, each with its oracles:

0. **P5-pre** (an ADR-0003 addendum and its implementation): the Phase1 `CellWritePlan` that
   folds the calcChain removal and the `calcPr` change into the caller's `RegistryPlan`, and the
   future-function prefix check (Q7). Needed before P5b and P5d.
1. **P5a**: §2 machinery, including the **per-sheet shape-number allocator** (B2), the plan
   order and the allowlist growth; images (§3); images in cells (read).
2. **P5b**: tables and header sync (§6).
3. **P5c**: notes and threaded comments (§4, §5).
4. **P5d**: sparklines and dynamic arrays (§8, §9).
5. **P5e**: charts, chart sheets, cache refresh (§7).

## Alternatives considered

1. **Regenerate VML from the comments** (OP): simple, but loses form controls and OLE icons
   (OP drops them unless `keep_vba`), and OP's `data="1"` collides across sheets. Rejected:
   append to the parsed VML.
2. **Count-based ids** (POI: anchors + 1, table parts + 1, chart frames from 0): collides
   after deletions and writes `id="0"`. Rejected: max + 1 over a full scan, plus session
   high-waters.
3. **`twoCellAnchor` by default** (Excel, RX, POI): needs column widths and row heights,
   which depend on `cols`, `sheetFormatPr`, the default font's metrics and hidden rows; an
   approximation misplaces the picture's bottom-right corner. Kept as an option when the
   caller gives both cells (Q1, decided).
4. **Automatic chart cache refresh on every cell edit**: rewrites chart parts on every edit;
   the parts are no longer L1 after an unrelated value change. Rejected for now (Q6).
5. **Writing images in cells now** (RX can): the rich-value parts are five interlinked
   registries with Microsoft-only schemas, and only 1 corpus file has them. Deferred.
6. **Smallest free number for table ids**: Excel's files show gaps are kept; reusing an id
   may revive references from slicer caches or external tools. Rejected: max + 1.

## Consequences

- Every object an Excel user commonly adds can be added to an existing file without touching
  unrelated parts, which none of the three reference libraries achieves (RX cannot load; OP
  drops; POI keeps but counts ids).
- The VML rules make one corpus file (not well-formed VML) uneditable for notes; that is
  deliberate.
- The allowlist grows by an estimated 60 DrawingML/SML entries and four Microsoft extension
  schemas; the codegen check keeps them evidenced.
- New formula cells (totals, calculated columns, dynamic arrays) inherit Phase1's guarantees,
  including `fullCalcOnLoad`.
- Threaded comments and dynamic arrays depend on Excel checks more than the other features,
  because the corpus has few examples.

## Open questions for the project owner (decided)

**Owner decision 2026-11-16:** every proposed default below is accepted.

- **Q1. Default picture anchor:** `oneCellAnchor` at the natural size. It is exact and needs
  no width model. `twoCellAnchor` is used when the caller gives both cells.
- **Q2. Reusing an existing media part** with identical bytes: yes, as exact-form reuse.
  - Only for an internal target whose content type is an image type and whose extension
    matches.
  - The hash is computed lazily on the first insert.
  - Removal counts references on the plan's view (`incoming_relationships`).
- **Q3. Fallback note text:** the short `[Threaded comment]` + the texts, as a constant. XL
  item: whether Excel rewrites it on save.
- **Q4. GUIDs:** random v4 with a deterministic source for tests, checked against existing
  ids.
- **Q5. Duplicate column names:** refused.
- **Q6. Chart cache refresh:** explicit `refresh_chart_caches()`, with §7.2's refusals.
- **Q7. Future-function prefixes:** refuse unprefixed known functions and offer an opt-in
  helper. The `set_formula` part belongs to the ADR-0003 addendum (P5-pre).

## Sources

- ECMA-376 Part 1 (5th ed.): §18.3.1.4 (`c`, `cm`, `vm`), §18.5 (tables), §18.7 (comments),
  §18.9 (metadata), §20.5 (SpreadsheetML drawing), §21.2 (charts); Part 4 (VML).
- [MS-XLSX]: threaded comments, persons, sparklines (x14), dynamic-array properties (`xda`),
  rich data. The element and attribute names used here were checked against the Open XML SDK
  schema data in `reference/openxml-sdk-data/` (threadedcomments, 2009/9/main, dynamicarray,
  richdata).
- rust_xlsxwriter 0.102.0 (crates.io), openpyxl 3.1.5 (PyPI sdist), Apache POI
  `47dcff769bb395034dddfd59c4609659b6dbc6a4`: files and lines as cited above.
- Corpus: POI `test-data/spreadsheet` at the same commit; calamine fixtures in
  `reference/fixtures/xlsx/calamine/`.

### Post-acceptance note (P5c)

- `Note::new` creates a hidden note with an empty author. Replies are separated by blank
  lines in the fallback note, in thread part order. Removing a thread leaves its now-empty
  part available for subsequent insertions, and leaves the workbook person list intact.
- `Comment::timestamp` accepts `SystemTime`. Like the default timestamp, it is formatted
  in UTC at second precision with no zone suffix. The Excel follow-up in §5 remains open.
- Comments, VML, threaded comments and persons are edited using Layer 2 source spans and
  staged together. VML's default placement uses the RX start offsets and default cell
  sizes. The sheet's schema lens checks and inserts `legacyDrawing` when needed.
- The SpreadsheetML allowlist includes the legacy and Microsoft threaded-comment elements.
  VML remains outside codegen.
- A mutable comments, VML, threaded-comment or person part with ambiguous OPC ownership
  is refused. An orphan VML note shape at the target cell also prevents insertion.
- Identifier scans seed the workbook's in-memory allocation state on first use.
  Removed thread GUIDs and shape numbers stay reserved for the current session.

### Post-acceptance note — P5d (Issue #108)

`Worksheet::add_sparklines` takes a `SparklineGroup` with sheet-qualified A1
sources and single-cell destinations. Groups can select the plot type,
individual point markers, ARGB colors, axis scaling and empty-cell handling.
An empty group or non-finite custom axis bound returns `InvalidSparklineGroup`.

`CellMut::set_dynamic_formula` writes the file form of a formula with an
anchor-only array reference and reuses the first compatible XLDAPR cell metadata
block in document order. Metadata updates, calcChain removal and calcPr updates
share the cell write's registry transaction. Dependency invalidation includes
the potential spill area. Adjacent writes remain legal outside any recorded
result range. Excel computes the spill and can report `#SPILL!` when occupied
cells block it. Future-function prefixes follow Q7.

Dynamic array writes require explicit row and cell references on the target
sheet so that the reopened formula census can resolve the new anchor. They
use Phase1's dimension maintenance and cache stripping, including the cache
type attribute and the shared-string reference count when applicable.

### Post-acceptance note — P5c follow-ups (Issue #128)

- A fallback note without a threaded comment on its cell can be edited and removed as a regular note.
- Microsoft 365 Excel (16.0.20326) round-trips ST_Xstring escapes in thread text, including literal `_xHHHH_` sequences and control characters.
- Adding a note to a threaded cell with a fallback note returns `NoteExists`.
- New VML parts declare `v`, `o` and `x` on their root. New shapes use the maximum numeric z-index plus one (one if absent). If the maximum is `i64::MAX`, the addition is refused.
- Inserted comment, author, text and person elements reuse an in-scope default namespace when it matches their namespace.
