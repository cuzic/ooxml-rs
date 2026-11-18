# ADR-0014: xlsx Phase4 — range features on existing worksheets

- Status: **Accepted** (revision 3; review round 2: "Accepted after Minor only: yes", with the
  conditions recorded in §12 and §3)
- Date: 2026-11-16
- Depends on:
  - [ADR-0001](0001-lossless-round-trip-architecture.md): Layers 1–3, T1/T1b/T2, append-only
    registries. Revision 2 adds a post-acceptance note relaxing the Phase0 merged-cell guard for
    anchors (§3).
  - [ADR-0002](0002-schema-driven-code-generation.md): typed lens, allowlist, §4.3 insertion
    (schema slots, `mc:AlternateContent` refusal).
  - [ADR-0003](0003-xlsx-phase1-formula-and-calc-state.md) (Accepted, revision 5; PR #20): tokenizer, name and sheet
    resolution, plan → validate → apply, poisoning.
  - [ADR-0006](0006-xlsx-phase2-shared-strings-and-styles.md) (Accepted): the `dxfs` registry
    (§5 item 8) and the package-wide `numFmtId` allocator (§5 item 3).
  - [ADR-0009](0009-xlsx-phase3-sheet-operations.md), **the version that is Accepted**
    (revision 2 or later; PR #27; revision 3 is Proposed at the time of writing). §12's
    subset condition is re-checked against that version when it is accepted (revision 3,
    review R2-5):
    - defined names (§9);
    - `set_print_area`/`set_print_titles`;
    - the rename table of formula locations (§6) and its closed-world scan;
    - the bare `#REF!` on delete (§5).
  - **Core `RegistryPlan`** (PR #38, `core/add-part`): new relationships and parts. It is
    the single rules table for ids and names.
- Roadmap: [`docs/roadmap/xlsx.md`](../roadmap/xlsx.md) §2.6 (range-based features), §6 Phase4.
- Evidence: [`spikes/xlsx-phase4/`](../../spikes/xlsx-phase4/README.md) (corpus survey,
  re-run for revision 2 at the pinned commit).

## Revision history

- **Revision 1:** initial draft (Excel owner).
- **Revision 2** (after `0014-opus-review-round1.md`: 2 Blocker, 5 Major, 11 Minor; taken
  over from the Excel owner; branch merged with `origin/main`):
  - **B1:** `set_autofilter` reuses an existing sheet-local `_xlnm._FilterDatabase`, refuses
    duplicates, leaves a workbook-scope one alone, and defines its behaviour on a sheet that
    already has an `autoFilter` (§7).
  - **B2:** the Phase3 dependency is rebuilt on ADR-0009 revision 2 and PR #38's
    `RegistryPlan`.
    - §12 lists every formula and reference site Phase4 creates, including `cfvo/@val`
      (`type="formula"`), and checks each against ADR-0009 rev 2's rename table.
    - Relationship additions follow the dry-run → plan → apply → in-place order.
    - A deleted sheet becomes a bare `#REF!`.
  - **M1:** writes to a merged range's **anchor** are allowed, deliberately relaxing Phase0's
    guard with an ADR-0001 note. Non-anchor cells stay refused (§3).
  - **M2:** CF formulas and `cfvo` formulas that reference another sheet are refused
    (`CrossSheetReference`). Excel writes them only in x14 form, which Phase4 does not create
    (§6).
  - **M3:** `remove_autofilter` refuses an active filter (`FilterActive`), clears
    `sheetPr/@filterMode`, and keeps the sheet-level `sortState` (§7).
  - **M4:** re-protecting replaces **every** hash attribute, legacy `password` included (§11).
  - **M5:** §2 has a complete per-feature table of allowed changes; the oracle is generated
    from it.
  - **m1–m11:**
    - URL and file-link classification and escaping (m1);
    - the hyperlink-removal rationale (m2);
    - DV and CF details (m3, m4);
    - the ADR-0006 references (m5);
    - `cols` (m6), panes (m7), page setup (m8), protection encoding (m9) and merge details
      (m10);
    - the corpus table re-run on the full pinned copy (m11).
- **Revision 3** (after `0014-opus-review-round2.md`: Minor only, "Accepted after Minor only:
  yes"; branch merged with `origin/main`). Status **Accepted**.
  - **R2-1:** the merged-cell relaxation covers **value and formula writes**: anchor allowed,
    non-anchor `MergedCell`. **Style-only writes** are allowed on every cell of a merged
    range. `merge`/`unmerge` update `SheetContext.merges` in the same apply step. The tests and
    documents the implementation changes are listed (§3), and ADR-0001's note states the same
    scope.
  - **R2-2:** `merge_clearing` and `set_hyperlink` with `opts.text` compose ADR-0003 §7's
    allowed diff for the cell write (dependents' caches, `calcPr`, `calcChain`). `opts.text`
    is the same write as `set_string` (shared when the workbook has an sst). `protect_sheet`'s
    attribute removal and `remove_autofilter`'s emptied `sheetPr` are in the table.
  - **R2-3:** "generated from this table" is defined: a data table in
    `tests/allowed_diff.rs`, a name-set equality test, a span-based judgement and per-row
    mutants (§2).
  - **R2-4:** `remove_autofilter` keeps `_FilterDatabase`, as Excel does. Rewriting the
    name's text in `set_autofilter` is an ADR-0009 §9 name edit.
  - **R2-5:** depends on ADR-0009's Accepted version; §12's subset condition is re-checked
    then. Why Phase4 keeps in-place edits next to ADR-0009 rev 3's `stage_part` is recorded,
    with the rule that Phase4 follows if `stage_part` becomes the xlsx-wide rule.
  - **Owner decision 2026-11-16:** Q1–Q4 accepted as proposed.
  - The reviewer's answers to Q1–Q4 are recorded as **proposed defaults** pending the owner.
    Q2 refuses cross-sheet list sources until an Excel check, and documents the defined-name
    workaround.

## Context

Phase0–3 edit values, formulas, registries and sheets. The remaining everyday edits on an
existing workbook are **range features**: properties attached to a cell range or to the sheet,
stored in their own elements of `CT_Worksheet` (and, for a few, in `workbook.xml`,
`styles.xml` or relationships). They are visible: a wrong one is a repair dialog or wrong
formatting rather than a silently wrong number, which is why the roadmap ranks them after
Phase1–3 (roadmap §6).

### Where they live

`CT_Worksheet` is a strict sequence (ECMA-376 Part 1 §18.3, `CT_Worksheet` in the Transitional
`sml.xsd`):

```
sheetPr dimension sheetViews sheetFormatPr cols sheetData sheetCalcPr sheetProtection
protectedRanges scenarios autoFilter sortState dataConsolidate customSheetViews mergeCells
phoneticPr conditionalFormatting* dataValidations hyperlinks printOptions pageMargins
pageSetup headerFooter rowBreaks colBreaks customProperties cellWatches ignoredErrors
smartTags drawing legacyDrawing legacyDrawingHF drawingHF picture oleObjects controls
webPublishItems tableParts extLst
```

Every Phase4 feature inserts or edits one or two of these elements. ADR-0002 §4.1 places a
new child in its slot and refuses with `UnresolvedSibling` when an `mc:AlternateContent` child
of the parent makes the position unresolvable. 2 corpus worksheets have one. For a slot with
`maxOccurs=1` (e.g. `definedNames` in `workbook.xml`), an AC child **anywhere** in the parent
blocks the insertion, not only an adjacent one: see §13.

### What the corpus shows

`spikes/xlsx-phase4/range_survey.py --summary`, run with `python3 -I` on Apache POI
`test-data/spreadsheet` at `47dcff769bb395034dddfd59c4609659b6dbc6a4`, the **full directory,
367 files**. 342 are readable Transitional packages; the rest are Strict, encrypted, fuzzing
cases or not zip, and 4 are skipped for a DTD.

Revision 1 measured a partial local copy (350 files). Its table did not reproduce at the pinned
commit (review m11) and is replaced. Counts are *files* / *occurrences*:

| Feature | Files | Occurrences | Notes |
|---|---|---|---|
| `mergeCells` | 43 | 54,468 merges on 76 sheets | 0 overlapping merges; **1 file (`60512.xlsm`) has 12 values in non-anchor cells** |
| hyperlinks | 14 / 3 / 2 | 70 external (`r:id`), 35 internal (`location`), 8 with neither, 3 with both | external targets: 55 with a scheme (9 files), 11 `mailto:` (5), **4 relative file paths** (2 Excel 12 files: `WithMoreVariousData.xlsx`, `ExcelWithAttachments.xlsm`) |
| `dataValidations` (2007) | 11 | decimal 27, list 25, whole 21, textLength 20, custom 3, date 2, time 2, none 8 | 1 inline list over 255 characters (`DataValidationListTooLong.xlsx`) |
| x14 data validation (`extLst`) | 3 | 7 | sources: 5 on another sheet (1 file); **2 `#REF!`** (`51850.xlsx` bare `#REF!`, `61652.xlsx` `Model!#REF!`) |
| `conditionalFormatting` | 20 | 108 blocks; cellIs 50, expression 33, iconSet 27, dataBar 7, colorScale 5, top10 3, … | 89 rules carry `dxfId`; `stopIfTrue` 16 (4 files); `cfvo type="formula"` 1 (`NewStyleConditionalFormattings.xlsx`) |
| 2007 CF formulas referring to another sheet | 0 | 0 | the 5 hits with a `!` (3 Excel 12/15/16 files) are all bare `#REF!` from deleted references |
| x14 conditional formatting | 2 | 4 `x14:cfRule`, all linked by `x14:id` | no dangling ids |
| `autoFilter` (sheet) | 3 | 3 | 0 with `filterColumn`; 1 with a `sortState` child |
| `_xlnm._FilterDatabase` | 6 | 8 names | **4 sheet-local names in 3 files whose sheet has no `autoFilter`** (`57523.xlsx`, `bug66827.xlsx`, `absolute-anchor-over-empty-sheet.xlsx`); 1 workbook-scope; 2 with `#REF!` text |
| sheet-level `sortState` | 5 | 7 | all on sheets **without** an `autoFilter` (Excel's "last sort") |
| `sheetPr/@filterMode` | 0 | 0 | |
| panes | 19 / 1 | 32 frozen, 2 split | 3 frozen panes without any `selection` (2 files) |
| `cols/col` | 179 | 14,556 elements, 3,478 multi-column ranges | 798 hidden, 24 with outline; every `col` has `width`; no sheet with two `cols`; no overlapping ranges |
| row attributes | 107 / 11 / 26 | `ht` 30,228, hidden 186, outline 12,317 | |
| page setup | 329 / 170 / 53 / 73 | `pageMargins` / `pageSetup` / `printOptions` / `headerFooter` | `rowBreaks` 4/5, `colBreaks` 3/3 |
| print names | 9 / 7 | `_xlnm.Print_Area` 16, `_xlnm.Print_Titles` 11 | |
| `sheetProtection` | 10 / 2 / 2 | 12 flags only, **5 legacy `password`** (`57181.xlsm`, `CVLKRA-KYC_Download_File_Structure_V3.1.xlsx`), 4 SHA-512 | revision 1's "no legacy password on sheets" was wrong (review M4) |
| `workbookProtection` | 23 / 1 / 2 | flags only / legacy / SHA-512 | |
| `dxfs` with `mc:AlternateContent` | 1 | 1 | `style-alternate-content.xlsx`: ADR-0006 §2 refuses additions there |

Facts that shape the design:

1. **Non-anchor values in merged ranges exist** in Excel-written files (12 in `60512.xlsm`).
   ooxml-rs must neither delete them silently on unrelated edits nor create new hidden ones
   without saying so.
2. **x14 data validations hold other-sheet lists and dead `#REF!`s.** The 2007 form with a
   sheet-qualified `formula1` is what openpyxl and rust_xlsxwriter write. Excel writes x14 for
   cross-sheet lists (`56644.xlsx`; KEIKAI-464). No primary source shows Excel 2010+ accepting
   the 2007 form without repair (§5, Q2).
3. **x14 conditional formatting is always linked** by `x14:id` between a 2007 `cfRule`'s
   `extLst` and an `x14:cfRule` in the worksheet `extLst`. Deleting or re-prioritising one side
   without the other corrupts the pair.
4. **Excel writes deleted references as a bare `#REF!`** in 2007 CF formulas: 5 formulas in 3
   files, including `IF(#REF!="verkauft",1,0)`. This matches ADR-0009 rev 2 §5.
5. **Excel leaves `_xlnm._FilterDatabase` behind** after an autofilter is removed (3 files),
   and keeps a sheet-level `sortState` independent of any autofilter (5 files).
6. **`cols/col` ranges are common** (3,478): changing one column of a range means splitting the
   element.
7. **Protection:** SHA-512 is what current Excel writes; the legacy 16-bit `password` hash
   appears on sheets (5) and workbooks (1).
8. **Range features carry formulas and references** outside cells:
   - data-validation `formula1`/`formula2` and x14 `xm:f`;
   - CF `formula`, `cfvo/@val` and x14 `xm:f`;
   - `hyperlink/@location`;
   - `_FilterDatabase`.

   They hold no cached value, so Phase1's invalidation does not apply. They do contain sheet
   and name references that Phase3 must rewrite (§12).

### Reference libraries

Verified in source: rust_xlsxwriter 0.102.0 (**RX**, `src/`), openpyxl 3.1.5 (**OP**,
`openpyxl/`), Apache POI @ `47dcff76` (**POI**, `poi-ooxml/…/xssf/usermodel/` unless noted).
Every citation was re-checked independently by the round-1 review, against separately fetched
sources.

Only POI edits a loaded tree in place, and none of the three keeps an unmodelled element byte
for byte:

- RX writes from scratch;
- OP rebuilds every element from its model and **drops every worksheet `extLst`, including x14
  data validation and x14 conditional formatting** (OP `worksheet/_reader.py:323-329`,
  `formatting/rule.py:164-167`).

| Feature | RX | OP | POI |
|---|---|---|---|
| Merge | needs ≥ 2 cells, overlap → `MergeRangeOverlaps`; writes a **blank cell with the anchor's format** in every other cell (`worksheet.rs:5002-5057`); no unmerge | no overlap check (`cell_range.py:467-477`); **discards non-anchor values** and copies edge borders (`worksheet.py:605-614`, `merge.py:93-133`) | ≥ 2 cells; overlap with merges or array formulas → `IllegalStateException`; values untouched (`XSSFSheet.java:338-368`); removing the last merge removes `mergeCells` (`:1973-1990`) |
| Hyperlink | URL ≤ 2,080 characters, tooltip ≤ 255 (`url.rs:13, 363-375`); classifies URL / `mailto:` / relative / UNC / drive paths and escapes space, `%`, `"`, `<>[]^{}`; `#` → `%23` in file links (`url.rs:387-465, 469-486, 523-558`); `rId`s renumbered per save | sets the cell value only if empty (`cell/cell.py:231-246`); rels renumbered per save | adds an external relationship only for new links (`XSSFHyperlink.java:151-160`); `removeHyperlink` keeps the relationship (`XSSFSheet.java:3657-3659`) |
| Data validation | 2007 form only; inline list ≤ 255 UTF-16 units with `"` doubled (`data_validation.rs:555-576, 561-565`); titles ≤ 32, messages ≤ 255; cross-sheet list formula passed through (`:638-643`) | no limits; x14 forms dropped on load | inline list ≤ 257 including quotes (`XSSFDataValidationConstraint.java:36, 212-213`); messages ≤ 255; no x14 API |
| Conditional formatting | priority = per-sheet counter (`worksheet.rs:9313-9333`); x14 data bars by default; deterministic pseudo-GUID `x14:id` | `max_priority + 1` only for rules without one (`formatting/formatting.py:64-79`) | priority = 1 + number of rules (`XSSFSheetConditionalFormatting.java:210-218`); `putDxf` without dedup |
| AutoFilter | `_xlnm._FilterDatabase` local, hidden, `'Sheet'!$A$1:$D$5` (`workbook.rs:3648-3657`); hides non-matching rows itself | writes `_FilterDatabase` on every save; `sortState` neither read nor written | **reuses** the built-in name for that sheet (`getBuiltInName(BUILTIN_FILTER_DB, sheetIndex)`, `XSSFSheet.java` `setAutoFilter` L4417–4441); an existing `autoFilter`'s `ref` is replaced and its `filterColumn`s kept |
| Panes | one `sheetView`; freeze only, one selection per pane (`worksheet.rs:18401-18453`) | `sheetView[0]` | the **last** `sheetView` is the default (`XSSFSheet.java:3784-3794`) |
| Columns | merges adjacent equal columns; **always writes `width`** (`worksheet.rs:19899-19982`) | no split: may write overlapping `col` (inference) | splits ranges into before/target/after (`ColumnHelper.java:184-212`) |
| Margins (default) | 0.7/0.7/0.75/0.75/0.3/0.3 | 0.75/0.75/1/1/0.5/0.5 (`worksheet/page.py:167`) | 0.7/0.7/0.75/0.75/0.3/0.3 |
| Header/footer limit | 255 characters for the **whole string, codes included** (`worksheet.rs:13592`) | — | — |
| Protection | legacy hash only (UTF-8 bytes, `%04X`) (`utility.rs:1435-1453`) | computes only legacy (code points, no zero padding) (`utils/protection.py:14-22`) | SHA-512: H(salt ‖ UTF-16LE(pw)), then 100,000 × H(hash ‖ LE int32 i), 16-byte random salt (`CryptoFunctions.java:123-150`, `XSSFPasswordHelper.java:61-75`); spin count ≤ 10,000,000 |

What this means for an editing library:

- Every reference library loses or rewrites something an Excel-authored file contains. OP
  drops x14 and `sortState` and renumbers rels; RX cannot edit; POI rewrites `hyperlinks` on
  save and keeps stale `filterColumn`s. ooxml-rs's element-local edits need the byte-exact
  oracle (§2), not a comparison with these libraries' output.
- The libraries disagree on margin defaults, the default `sheetView`, the legacy hash's
  character encoding, and whether merging clears values. The ADR picks Excel's observable
  behaviour where the corpus documents it, and the safe choice otherwise.

## Decision

### 1. Scope and shape

Phase4 adds, on any worksheet opened with `worksheet_by_name`/`worksheet_by_index`:

| Area | API (sketch) |
|---|---|
| Merged cells | `merge(range)`, `merge_clearing(range)`, `unmerge(range)`, `merged_ranges()` |
| Hyperlinks | `set_hyperlink(cell_or_range, Link::Url(..) \| Link::File(..) \| Link::Location(..), opts)`, `remove_hyperlink(range)`, `hyperlinks()` |
| Data validation | `add_validation(sqref, Validation)`, `remove_validation(sqref)`, `validations()` |
| Conditional formatting | `add_conditional_format(sqref, Rule)` (lowest priority), `add_conditional_format_first(..)` (renumbers), `remove_conditional_format(..)`, `conditional_formats()` |
| AutoFilter | `set_autofilter(range)`, `remove_autofilter()` |
| Views | `freeze_panes(row, col)`, `split_panes(x, y)`, `unfreeze()`, `set_zoom`, `set_show_gridlines`, `set_right_to_left` |
| Columns and rows | `set_column_width(cols, w)`, `set_column_hidden`, `set_column_outline(cols, level)`, `set_row_height(row, h)`, `set_row_hidden`, `set_row_outline` |
| Page setup | `page_setup_mut()` (orientation, paper, scale, fit-to), `set_margins`, `set_print_options`, `set_header_footer`, `add_row_break`/`add_col_break`; print area/titles through ADR-0009's setters |
| Protection | `protect_sheet(Protection)`, `unprotect_sheet()`, `protect_workbook(..)` |

**Every call is one edit through ADR-0003 §3.1:**

- plan and validate without mutation, then apply in place;
- an apply failure poisons the workbook;
- every guard below is checked in the plan.

A call that adds a relationship follows §12's order. Reads (`merged_ranges()`, `hyperlinks()`,
…) return typed values decoded through the lens and never change the tree.

Each feature's write path is a set of **allowlist entries** (ADR-0002 §5), promoted from read
to write in `codegen/allowlist/sml.toml`, plus hand-written x14 lenses for the `extLst` forms
(§5, §6). Nothing outside those elements is re-rendered.

### 2. Locality: the allowed changes, per feature (revision 2, review M5)

Phase1's byte-exact allowed-diff oracle (ADR-0003 §7) is reused. For each call, the expected
output is the input with exactly the changes this table allows for that call. Every other byte
and part must be identical.

**"Generated from this table" (revision 3, review R2-3)** means:

- The table also exists as **data**: a `const` table in `crates/ooxml-xlsx/tests/allowed_diff.rs`
  mapping each call to a `&[Allowed]`. An `Allowed` is `{ part, path, kind }`: `part` selects
  the sheet, its `.rels`, `workbook.xml`, `styles.xml`, …; `path` is the element path
  (`mergeCells/mergeCell`); `kind` is `Create`, `Remove`, `EditAttrs(&[names])` or
  `Children`. `Compose(Phase1CellWrite { cell })` and `Compose(Phase2Registry)` include
  ADR-0003 §7's allowed diff for a cell write and ADR-0006's T2-append.
- A test checks that the set of call names in this ADR table equals the set in the data table,
  so neither can gain a row alone.
- The oracle judges with Layer 2 spans: every byte outside the spans of the allowed elements
  (and outside the allowed attributes' values) is identical, by `assert_t2`'s algorithm.
- Mutation check: for each row, a mutant that touches one sibling element the row does not
  allow must fail the oracle.

Across all features:

- "Container" means the feature's list element (`mergeCells`, `hyperlinks`,
  `dataValidations`, `cols`, `rowBreaks`, …). It is created in its slot when absent, and
  **removed when it becomes empty**: Excel repairs an empty `mergeCells`, `hyperlinks` or
  `dataValidations`, and an empty `conditionalFormatting` is invalid.
- "count" means that container's `count` attribute, where the schema has one.

| Call | Sheet part | Other parts |
|---|---|---|
| `merge` | one new `mergeCell`; `mergeCells` container + `count` | — |
| `merge_clearing` | as `merge`, plus the cleared `v`/`is`/`f` and `t` of non-anchor cells; **composed with** ADR-0003 §7's allowed diff for each cleared cell (dependents' `v`/`t`/`is`, other sheets included) | the same composition: `workbook.xml` `calcPr/@fullCalcOnLoad` (with `calcPr` created), `calcChain.xml` removed when the formula set changes; `sst/@count` (Phase2) |
| `unmerge` | one `mergeCell` removed; container + `count` | — |
| `set_hyperlink` | one new `hyperlink` (`ref`, `r:id` or `location`, `display`, `tooltip`); `hyperlinks` container; with `opts.text`, the anchor cell's string write (§4), **composed with** ADR-0003 §7's allowed diff for that cell | the sheet's `.rels` (one appended `Relationship`, or the part created); `[Content_Types].xml` only through core's rules when the `.rels` part is created; with `opts.text`: Phase1's `workbook.xml`/`calcChain.xml` changes as composed, `sharedStrings.xml` by T2-append when the workbook has an sst (§4), and `styles.xml` only with the "Hyperlink" style opt-in |
| `remove_hyperlink` | the `hyperlink` element(s); container | — (the relationship stays, §4) |
| `add_validation` | one new `dataValidation`; `dataValidations` container + `count` | — |
| `remove_validation` | the rule(s); container + `count`; for x14: the `x14:dataValidation`, `x14:dataValidations/@count`, and the `ext` element when it becomes empty | — |
| `add_conditional_format` | one new `conditionalFormatting` block with one `cfRule` (a new block: §6) | `styles.xml`: one appended `dxf` + `dxfs/@count` (Phase2 registry), possibly a new `numFmt` (ADR-0006 §5 item 3) |
| `add_conditional_format_first` | as above, plus `priority` on **every** other `cfRule` **and every `x14:cfRule`** in `extLst` | as above |
| `remove_conditional_format` | the `cfRule`, its block when empty; for linked rules the `x14:cfRule`, its `x14:conditionalFormatting` when empty, and the `ext` when empty | — |
| `set_autofilter` | `autoFilter` (new, or `@ref` of an existing plain one, §7) | `workbook.xml`: the local `_FilterDatabase`'s text and `hidden`, or one new `definedName` and the `definedNames` container when absent; when an existing name's text changes, **composed with** ADR-0009 §9's name edit (Phase1 invalidation of formulas that use the name) |
| `remove_autofilter` | the `autoFilter` element with its children; `sheetPr/@filterMode` (an emptied `sheetPr` element stays) | — (the `_FilterDatabase` name stays, §7) |
| `freeze_panes`/`split_panes`/`unfreeze` | the `sheetView` (created in `sheetViews`, both in their slots, when absent); its `pane`; its `selection` elements (replaced as in §8) | — |
| `set_zoom`/`set_show_gridlines`/`set_right_to_left` | attributes of that `sheetView` (created when absent) | — |
| column setters | the `col` elements of the range (edited, split, new); `cols` container; `sheetFormatPr/@outlineLevelCol` (and `sheetFormatPr` itself, created with the required `defaultRowHeight`, §9) | — |
| row setters | the `row` element's `ht`/`customHeight`/`hidden`/`outlineLevel` (or a new empty `row` in order); `sheetFormatPr/@outlineLevelRow` (as above) | — |
| `page_setup_mut` | `pageSetup` attributes (created when absent); for fit-to-page `sheetPr/pageSetUpPr/@fitToPage`, with `sheetPr` and `pageSetUpPr` created when absent | — |
| `set_margins`/`set_print_options`/`set_header_footer` | that element (created when absent) | — |
| `add_row_break`/`add_col_break` | one `brk`; container + `count` + `manualBreakCount` | — |
| `protect_sheet`/`unprotect_sheet` | `sheetProtection` (created, attributes replaced as in §11, or removed); replacing may **remove** an existing attribute whose value equals the default (§11) | — |
| `protect_workbook` | — | `workbook.xml`: `workbookProtection` (as in §11) |

A call whose changes would fall outside its row is a bug. The oracle reports it as such and
nobody widens the table ad hoc.

### 3. Merged cells

- **Create.** `merge(range)` needs at least two cells. It refuses with `Err` and changes
  nothing on:
  - overlap with an existing merge (`MergeOverlap`);
  - overlap with a table range (`TableOverlap`);
  - overlap with an array, data-table or spill result range or anchor (Phase1
    `ArrayResultCell`/`ArrayAnchor`), or with a `vm` cell;
  - overlap with a PivotTable `location/@ref` or a query table's output range
    (`PivotOverlap`, `QueryTableOverlap`; review m10). Excel's UI refuses these too.
    As implemented, a sheet with a query-table relationship refuses **every** merge: the query
    table's output range is not located (it lives in the table or a defined name), so overlap
    cannot be ruled out (PR #58 round 1 m7). A PivotTable part is read only when a merge needs
    it; one whose `location/@ref` cannot be read counts as overlapping (round 1 M1);
  - **any value or formula in a non-anchor cell** (`MergeWouldHideValues { cells }`): any cell
    with a `v`, an `is` or an `f` (review m10). Excel's UI warns and discards them; a library
    must not hide data silently.
- `merge_clearing(range)` does what Excel does after its warning. It clears non-anchor values
  and formulas, then merges. Each clearing is a Phase1 removal: dependents are invalidated,
  and calcChain is removed if the formula set changes.
  - Styles of non-anchor cells are left as they are; Excel keeps border formatting on them.
  - Whether Excel moves the value of a single non-anchor cell into the anchor is an XL item.
    Until it is checked, `merge_clearing` is documented as "clears", not as "same as Excel"
    (m10).
- **Remove.** `unmerge(range)` removes exactly the `mergeCell` whose `ref` equals the range
  (`NoSuchMerge` otherwise).
  - The non-anchor cells keep whatever they hold. In a file with hidden values (`60512.xlsm`)
    those values become visible again. The API documentation says so (m10).
- **Writing into merged ranges (revision 2, review M1; scope made exact in revision 3, R2-1).**
  Phase0 and Phase1 refuse writes into any cell of a merged range, the anchor included
  (ADR-0001 Phase0 list; ADR-0003's guard table; implemented as
  `ctx.merges.iter().find(|m| m.contains(at))` in `edit.rs`). Phase4 **deliberately relaxes**
  this:
  - **Value and formula writes** (`set_number`, `set_string`, `set_string_inline`,
    `set_formula`, `remove_formula`, `set_bool`, `set_error`, `set_datetime` and the
    `*_with_format` forms): allowed on the **anchor** (top-left cell), as in Excel; on a
    **non-anchor** cell they stay `Err(MergedCell)`, so no new hidden value is ever created.
    The later guards still apply: an anchor inside an array range, a table header or a `vm`
    cell is refused by them, since `edit_cell` evaluates merge → table header → array anchor
    → array result.
  - **Style-only writes** (ADR-0006 §7.3: `set_format`, `set_style_index`,
    `set_named_style`, `set_num_format`/`set_font`/`set_fill`/`set_border`): allowed on
    **every** cell of a merged range. They create no value, and Excel formats edge cells
    individually (the outer border of a merged range is the edge cells' borders).
  - **Session state.** The guard reads `SheetContext.merges`. `merge`, `merge_clearing` and
    `unmerge` update that list in the same apply step as the sheet edit. Tests: after `merge`,
    a value write to a non-anchor cell is `Err(MergedCell)`; after `unmerge`, it succeeds.
  - **Tests and documents that change** when this is implemented (the implementation PR lists
    them as intended changes):
    - `tests/refusals.rs` `r6_merged`: the write to `B1`, an anchor, becomes `Ok`;
    - `tests/edit.rs` (line 15's sweep): `Expect::Merged` no longer applies to anchors;
    - `crates/ooxml-xlsx/README.md`'s `MergedCell` row ("top-left cell included");
    - ADR-0003's guard row ("Write into any cell of a merged range (top-left included)" →
      "A value or formula write to a non-anchor cell of a merged range") and its line 31
      ("refusal of every cell of a merged range");
    - ADR-0001's post-acceptance note (this PR) states the same scope.
  - Existing hidden values (fact 1) are untouched by every other Phase4 call.
- `count` is updated. calamine exposes merges, so CR covers this feature.

### 4. Hyperlinks

- **Link kinds (revision 2, review m1):**
  - `Link::Url`: an absolute URI with a scheme (`https:`, `ftp:`, `mailto:`, …);
  - `Link::File`: a relative path, a UNC path (`\\server\share\x.xlsx`) or a drive path
    (`C:\dir\x.xlsx`). Excel itself writes relative file targets: 4 in 2 Excel 12 files;
  - `Link::Location("Sheet2!A1")`: internal only.
- **External links (URL, file).**
  - They get a `hyperlink` with `r:id` pointing at a new `TargetMode="External"` relationship of
    the sheet, added through PR #38's `RegistryPlan` (§12; ids never reused).
  - Escaping follows RX:
    - space, `%`, `"`, `<`, `>`, `[`, `]`, `^`, `{`, `}` are percent-encoded;
    - in file links, `#` becomes `%23`;
    - an existing `%XX` escape is kept.
  - Limits: at most 2,080 characters (RX) and tooltips at most 255.
  - A `#fragment` of a URL or file link goes to `location`, as Excel does.
  - Targets that core refuses (whitespace or control characters after escaping) are
    `Err(InvalidUrl)`.
- **Internal links.** `location` only. The target is lexed with the Phase1 tokenizer and
  resolved with ADR-0003 §2.4 (unknown sheet → `Err(UnknownSheet)`).
- `display` and `tooltip` are optional.
  - The **cell value is not changed** unless the caller asks (`opts.text`). That is the same
    write as `set_string` (revision 3, review R2-2): a shared string through ADR-0006's
    registry when the workbook has a `sharedStrings.xml`, else an inline string. It goes to
    the anchor of a merged range if the cell is in one (§3), and it is a Phase1 value write
    (dependents lose their caches, as composed in §2).
  - Applying the "Hyperlink" cell style is a Phase2 named-style lookup and is opt-in.
- A new hyperlink whose `ref` overlaps an existing one is refused (`HyperlinkOverlap`).
- **Removal** removes the `hyperlink` element and **keeps** the relationship (review m2). Core
  now allows removing an unreferenced relationship (`RegistryPlan::remove_relationship` with
  its `StillReferenced` check), but the relationship is kept anyway:
  - several `hyperlink` elements may share one `r:id`;
  - an unused relationship is valid and harmless;
  - Excel drops unused hyperlink relationships on its next save (0 unused ones in Excel-made
    corpus files).
- Hyperlinks in merged ranges are allowed on the anchor only.

### 5. Data validation

- `Validation` covers the 2007 attributes:
  - type (whole, decimal, list, date, time, textLength, custom) and operator;
  - `formula1`/`formula2`;
  - blank handling and drop-down suppression;
  - input and error messages, and error style.

  Titles are limited to 32 characters and messages to 255; RX and POI enforce the same.
- **Formulas.**
  - They are lexed with the Phase1 tokenizer, stored without `=`, and resolved by ADR-0003
    §2.4 (unknown sheet → `Err`).
  - They are relative to the top-left cell of the first range of `sqref`.
  - `formula1`/`formula2` are limited to 255 characters, range references included (EPPlus;
    m3).
- **Inline lists** (`"a,b,c"`):
  - limited to 255 characters (Excel's limit; 1 corpus file exceeds it, and Excel repairs it):
    `ListTooLong`;
  - a `"` inside an item is written doubled (`""`, RX; m3);
  - an item containing `,` cannot be represented and is refused (`ListItemHasComma`).
- **Cross-sheet sources (proposed default Q2, review M2/Q2).** A `formula1`/`formula2` that
  refers to **another sheet**, directly and not through a defined name, is refused with
  `Err(CrossSheetReference)` until an Excel check passes. The XL item: a 2007-form
  cross-sheet list opens in Microsoft 365 Excel without repair, the drop-down works, and we
  see which form Excel writes back on save.
  - The workaround, documented on the API: define a name for the source range on the other
    sheet and use it in `formula1` (`=Colours`). That is valid in the 2007 form, and Excel
    does not move it to `extLst`.
  - When the XL item passes, the refusal is lifted. Writing the x14 form remains out of scope
    (Alternatives 2).
- **x14 rules** (`extLst/ext[@uri="{CCE6A557-97BC-4b89-ADB6-D9C93CAAB3DF}"]/x14:dataValidations`)
  are read (hand-written lens: `xm:sqref`, `x14:formula1/xm:f`) and preserved.
  - A new 2007 rule whose `sqref` overlaps an x14 rule's, or another 2007 rule's, is refused
    (`ValidationOverlap`): a cell has one validation in Excel.
- `remove_validation(sqref)` removes rules whose `sqref` is exactly the given one, 2007 or x14.
  - For x14 it updates `x14:dataValidations/@count` and removes the `ext` when it becomes
    empty (§2; m3).
  - Partial removal (splitting an `sqref`) is a later refinement.

### 6. Conditional formatting

- **Formats** come from the Phase2 `dxfs` registry (`DxfFormat`, **ADR-0006 §5 item 8**;
  review m5):
  - append-only, deduplicated among new entries;
  - never copying default-style ids into a `dxf`;
  - a `dxf/numFmt` gets its id from the **package-wide** `numFmtId` allocator of ADR-0006 §5
    item 3;
  - a `dxfs` containing `mc:AlternateContent` (`style-alternate-content.xlsx`) refuses the
    addition (ADR-0006 §2). That error is passed through (§13).
- **Priorities** are unique per sheet, across 2007 `cfRule`s and `x14:cfRule`s; a lower number
  wins.
  - `add_conditional_format` gives the new rule `max + 1` (lowest priority), so no existing
    element changes.
  - `add_conditional_format_first` gives it priority 1 and renumbers **every other rule,
    including x14-only `x14:cfRule`s in `extLst`**. That is a documented T2 widening (§2,
    proposed default Q3).
  - Duplicate priorities already in a file are left as they are, and both calls still produce
    unique values for new rules.
- **Placement (m4).** A new rule always goes into a **new** `conditionalFormatting` block,
  appended after the last existing one, even when a block with the same `sqref` exists. That
  keeps every existing block byte-identical.
- **Rule kinds written** (2007 forms only):
  - cellIs and expression;
  - containsText, notContainsText, beginsWith, endsWith;
  - top10, aboveAverage, duplicate/unique values, timePeriod;
  - colorScale, dataBar, iconSet.

  `Rule` has `stop_if_true` (m4; 16 corpus rules). **x14-only forms** (data-bar extensions,
  the additional icon sets) are out of scope for writing; existing ones are read and
  preserved.
- **Cross-sheet references are refused (revision 2, review M2).** A `formula`, a cellIs
  operand or a `cfvo/@val` with `type="formula"` that refers to another sheet, other than
  through a defined name, is refused with `Err(CrossSheetReference)`.
  - Excel 2010+ writes such rules only in x14 form (`xm:f`), which Phase4 does not create.
    There is no evidence that Excel accepts them in 2007 form: the corpus's five `!` hits in
    2007 CF formulas are all dead `#REF!`s (fact 4).
  - This is lifted only together with the DV Q2 item.
- **x14 links** (fact 3). Removing a 2007 rule that carries an `x14:id` also removes the
  linked `x14:cfRule`, then its now empty `x14:conditionalFormatting`, then the `ext` if it
  becomes empty.
  - If the link cannot be resolved (no x14 rule with that id), the half-pair is already
    broken. Removing the 2007 side **is allowed** (it repairs rather than breaks), and the
    result reports it (`RemovedWithBrokenX14Link`; m4).
- **Pivot blocks.** `conditionalFormatting/@pivot="1"` blocks belong to a PivotTable. They are
  never removed or renumbered (removal refuses with `PivotConditionalFormat`; m4).
- **Formulas** (expression rules, cellIs operands, `cfvo` formulas) follow §5's formula rules
  and are relative to the top-left cell of the first range of `sqref`.
- `extLst` editing keeps every `ext` this phase does not own byte-identical. New `ext` elements
  go after existing ones (`CT_ExtensionList` imposes no order among `ext`s).

### 7. AutoFilter and sort state

- **`set_autofilter(range)` (revision 2, review B1).**
  - **The sheet's `autoFilter`:**
    - none → a new `autoFilter ref=…`;
    - an existing one with no `filterColumn` and no `sortState` child → only its `@ref` is
      rewritten;
    - an existing one with a `filterColumn` or a `sortState` child →
      `Err(AutoFilterExists)`. `filterColumn/@colId` is relative to the range's left edge, so
      moving `ref` would silently retarget the criteria (POI keeps them in this form).
  - **The defined name `_xlnm._FilterDatabase`** with `localSheetId` = the sheet's position:
    - **exactly one** → its text is rewritten in place to `'Sheet'!$A$1:$D$9`, and `hidden="1"`
      is set if missing. Excel leaves this name behind after the filter is removed (fact 5:
      4 names in 3 files); POI reuses it the same way;
    - **two or more** → `Err(DuplicateBuiltInName)`;
    - **none** → a new hidden sheet-local `definedName`, appended to `definedNames` (created
      in its slot when absent).
    - A **workbook-scope** `_FilterDatabase` (1 corpus file, text `#REF!`) is never touched.
  - ADR-0009 rev 2 §9 refuses `_xlnm.*` writes other than print names, and reserves
    `_FilterDatabase` for this phase. The name rules and the formula text follow ADR-0009 §9.
    Rewriting an existing name's text is a name edit under ADR-0009 §9: formulas that use
    `_FilterDatabase` (none expected) are invalidated like any name edit (§2's row).
  - A range overlapping a table is refused (`TableOverlap`): tables carry their own
    `autoFilter`.
- **Filter criteria are out of scope.** Writing a `filterColumn` without hiding the rows it
  excludes leaves a sheet that disagrees with itself until Excel re-applies the filter.
  Existing `filterColumn` and `sortState` are preserved.
- **`remove_autofilter()` (revision 2, review M3):**
  - with a `filterColumn` present (the filter is active, and rows may be hidden by it):
    `Err(FilterActive)`. Removing it would leave rows hidden that no filter explains. A later
    `remove_autofilter_showing_rows` could unhide them and report which ones (Alternatives 7).
    Corpus: 0 active filters;
  - otherwise it removes the `autoFilter` element with its children and clears
    `sheetPr/@filterMode` if present (0 in the corpus). **The local `_FilterDatabase` name is
    kept** (revision 3, review R2-4), as Excel keeps it (fact 5), and `set_autofilter`'s reuse
    rule relies on it. Removing it would widen the diff to `workbook.xml` and would be a name
    deletion under ADR-0009 §9 (`NameInUse` when used). `workbook.xml` is not touched;
  - the **sheet-level** `sortState` (a direct child of `worksheet`) is Excel's record of the
    last sort, independent of the filter (fact 5). It is **kept**. Revision 1 removed it.

### 8. Views and panes

- Edits apply to the `sheetView` whose `workbookViewId` is 0. Every corpus file has exactly one
  `sheetView`; a sheet without one gets one, in its slot. Other `sheetView`s are untouched.
- `freeze_panes(row, col)` writes:
  - `pane` with `xSplit`/`ySplit` (counts), `topLeftCell`, `activePane`
    (bottomRight/bottomLeft/topRight by which splits exist) and `state="frozen"`;
  - the `selection` elements, replaced with one per pane, as Excel does. The existing
    `activeCell`/`sqref` is carried over to the new active pane's `selection` (m7).
- Refusals:
  - `freeze_panes(0, 0)` is `Err(NothingToFreeze)`;
  - a `sheetView/@view="pageLayout"` cannot hold a frozen pane: `Err(PageLayoutView)` (m7).
- Reading accepts a frozen pane with no `selection` (3 corpus panes, 2 files).
- `split_panes` writes `state="split"` with twips. `unfreeze` removes `pane` and the
  pane-specific `selection`s.
- `set_zoom` (10–400, `zoomScale`), `set_show_gridlines`, `set_right_to_left`: attributes only.
- Active sheet and tab selection belong to Phase3.

### 9. Columns and rows

- `set_column_width(cols, width)` and the hidden/outline setters edit `cols/col` ranges:
  - an existing `col` exactly covering the range is edited in place;
  - a `col` partly covering it is **split** into up to three elements, each keeping the other
    attributes. Adjacent equal ranges are **not** merged afterwards (locality);
  - missing coverage gets new `col` elements in `min` order; `cols` is created in its slot.
- **Refusals (m6):** a sheet with more than one `cols` element (`cols` is `maxOccurs` unbounded),
  or with overlapping `col` ranges (as openpyxl can write), is refused (`AmbiguousColumns`).
  The corpus has neither.
- **A new `col` always has `width` (m6).** Every corpus `col` has one, and RX always writes
  one.
  - Setting only hidden or outline on uncovered columns writes the sheet's default width
    (`sheetFormatPr/@defaultColWidth`, else `@baseColWidth` + 5 *(u: Excel's formula; XL
    item)*), without `customWidth`.
  - Widths set by the caller are stored in Excel character units, with `customWidth="1"`.
    Widths above 255 are refused (POI's limit).
- Row height/hidden/outline edit `row` attributes (`ht` + `customHeight="1"`, `hidden`,
  `outlineLevel`), creating an empty row in order when needed (Phase0's row insertion, with
  the row index of PR #29).
- Outline levels are 0–7.
  - `sheetFormatPr/@outlineLevelRow`/`@outlineLevelCol` are raised to the maximum level
    present. They are never lowered automatically.
  - Creating `sheetFormatPr` writes the required `defaultRowHeight` (`sml.xsd` L2225) as 15
    *(u: depends on the default font; XL item)* (m6).

### 10. Page setup, margins, print

- `pageSetup` attributes are edited in place, and an existing `r:id` (the printer-settings
  binary, DEVMODE) is kept.
  - The XL list checks that the XML attributes take precedence when orientation or paper size
    changes while that binary is kept (m8).
  - Fit-to-page also sets `sheetPr/pageSetUpPr/@fitToPage="1"`, creating `sheetPr` and
    `pageSetUpPr` when absent (§2).
- `pageMargins` needs all six attributes. When it is absent, Excel's defaults are written
  (0.7/0.7/0.75/0.75/0.3/0.3, as RX and POI).
- `printOptions`: gridlines, headings, centring.
- `headerFooter`: odd, even and first text with Excel's `&` codes, validated by a small parser
  and escaped.
  - **The 255-character limit applies to the whole string, codes included** (RX;
    review m8). That is the stricter of the two readings. Whether Excel counts per section
    without codes is an XL item.
  - Images in headers (`legacyDrawingHF`) are out of scope.
- Manual breaks: `rowBreaks`/`colBreaks` `brk` elements with `man="1"`, with `count` and
  `manualBreakCount` updated.
- Print area and print titles: ADR-0009 rev 2 §9's `set_print_area`/`set_print_titles`.

### 11. Protection
> Implementation note (PR #58 review m1): The worksheet API uses `SheetProtection` to avoid a name collision with Phase 2 cell-format `Protection`.

- **`protect_sheet(Protection)`** writes `sheetProtection` with `sheet="1"` and the flags the
  caller chooses.
  - **Flag polarity (m9).** In `CT_SheetProtection`, `1` means **protected** (the action is
    forbidden), and defaults differ per attribute: `formatCells` defaults to true,
    `selectLockedCells` to false. The API takes `allow_*` booleans and writes the inverse,
    omitting an attribute when its value equals the schema default.
    `objects="1"`/`scenarios="1"` are written unless allowed, as RX and POI do.
  - With a password it writes the **SHA-512 form**:
    - `algorithmName="SHA-512"`;
    - a 16-byte `saltValue`;
    - `spinCount`;
    - `hashValue`: H(salt ‖ UTF-16LE(password)), then `spinCount` × H(previous ‖ LE int32
      i). The salt comes **first**; the review verified this order against the corpus file
      (m9).
  - `hashValue` and `saltValue` are **base64** (`xsd:base64Binary`, `sml.xsd` L2890–2891; m9).
- **Salt and spin count (proposed default Q4).**
  - `Protection::with_salt([u8; 16])` lets the caller pass the salt; the salt is not secret.
    The default salt comes from the OS CSPRNG.
  - `with_spin_count(n)`, default 100,000, max 10,000,000 like POI.
  - Byte-exact tests and hash vectors use `with_salt`.
- **Replacing existing protection (revision 2, review M4).** On a sheet that already has a
  `sheetProtection` (5 corpus elements carry a legacy `password`, e.g. `57181.xlsm`
  `password="EBF6"`):
  - **every hash attribute is replaced**: `password`, `algorithmName`, `hashValue`,
    `saltValue`, `spinCount`. Those not part of the new form are removed, so a legacy
    `password` never coexists with a SHA hash;
  - the flags the caller specifies are written;
  - unknown attributes are kept.

  `protect_workbook` does the same for `workbookPassword`, `workbookAlgorithmName`,
  `workbookHashValue`, `workbookSaltValue` and `workbookSpinCount`, and keeps `revisions*`
  untouched.
- The **legacy** 16-bit hash is read, preserved and never written by default.
  - The `Protection::legacy_hash` option exists for consumers that do not read the SHA form.
    It uses POI's "ANSI" byte rule and writes `%04X`, and it **removes** the SHA attributes.
  - RX hashes UTF-8 bytes and OP code points without zero padding, so the three disagree for
    non-ASCII passwords.
- `unprotect_sheet` removes the element. No call verifies or recovers passwords.
- `protect_workbook` writes `workbookProtection` (structure/windows) in `workbook.xml` with the
  same rules.
- Sheet protection does not stop ooxml-rs edits (it is an Excel UI setting); the API docs say
  so.

### 12. Interaction with Phase1, Phase2 and Phase3 (revision 2, review B2)

**Formula and reference sites created by Phase4**, and where ADR-0009 rev 2 handles them.
ADR-0014 is accepted only if this list is a subset of ADR-0009 rev 2's §6 rename table and of
its closed-world scan. **Acceptance condition (revision 3, review R2-5):** the subset is
re-checked against ADR-0009's **Accepted** version when it is accepted. The review confirmed it
for rev 2; ADR-0009's open round-2 finding R2-M1 (drawing `.rels` `#Sheet!A1`) touches no
Phase4 site.

| Site | Syntax | ADR-0009 rev 2 §6 (rename) | §5 (delete) |
|---|---|---|---|
| DV `formula1`/`formula2` | formula | "DV `formula1`/`formula2`" | bare `#REF!` |
| x14 DV `x14:formula1/2/xm:f` (read and preserved only) | formula | "x14 `xm:f` (DV, CF incl. `x14:cfvo`, sparklines)" | bare `#REF!` |
| CF `cfRule/formula` | formula | "CF `cfRule/formula`" | bare `#REF!` |
| CF `cfvo/@val` with `type="formula"` | formula | "2007 `cfvo/@val` (`type="formula"`)" | bare `#REF!` |
| x14 CF `xm:f`, `x14:cfvo/xm:f` (preserved only) | formula | "x14 `xm:f` … incl. `x14:cfvo`" | bare `#REF!` |
| `hyperlink/@location` | location | "`hyperlink/@location`" | **left as is** (ADR-0009's one listed exception: a navigation target) |
| `_xlnm._FilterDatabase` text | formula | "`definedName` text (all scopes)" | the name is local to the deleted sheet and removed with it |
| `_xlnm.Print_Area`/`Print_Titles` | formula | as above | as above |
| `mergeCell/@ref`, `sqref`, `autoFilter/@ref`, `hyperlink/@ref`, `pane/@topLeftCell` | sheet-local reference, no sheet name | unaffected | unaffected |

- **ADR-0009 rev 2 covers every row.** Its §6 table now lists `cfvo/@val`, which was review
  B2(a)'s gap. Its closed-world scan refuses a rename that would leave a sheet-prefix mention
  outside a known location, so a site Phase4 created and Phase3 forgot would be refused, not
  missed.
- **`#REF!` form (B2c).** A reference to a deleted sheet becomes a **bare `#REF!`**, as ADR-0009
  rev 2 §5 specifies and as Excel writes in CF (fact 4). Revision 1's `#REF!A1` is withdrawn.
  The lexer accepts every rewritten text (ADR-0009's invariant).

**Relationship and multi-part edits (B2b).** Phase4 adds relationships for external
hyperlinks only, and edits two parts for autofilters.

- **External hyperlink.** One call runs in this order:
  1. **dry run** of the sheet edits (ADR-0003 §3.1 plan, every guard), which may parse the
     sheet with `xml_part_mut`;
  2. a `RegistryPlan` (PR #38) with `add_relationship(sheet, …/hyperlink, Target::External)`.
     This decides the `Id` and creates the sheet's `.rels` part when it is absent. Core's
     rules decide its content type registration;
  3. `Package::apply`, which cannot fail except `StalePlan`. Nothing touches the package
     between steps 2 and 3;
  4. the in-place sheet edit writing the decided `r:id`, which cannot fail after step 1. A
     failure there poisons the workbook (ADR-0003 §7.3).

  Revision 1's reliance on ADR-0009 rev 1's `add_relationship` and `check_*` twins is
  withdrawn. Its multi-step validation was unsound (ADR-0009 review B2), and PR #38's plan
  replaces it.
- **AutoFilter** edits `workbook.xml` (the name) and the sheet (the element). It adds no
  registry entry. Both edits are planned and checked first, then applied in place, and a
  failure poisons (ADR-0003 §3.1). ADR-0009 rev 2 stages `workbook.xml` in a `RegistryPlan`
  for its own operations. Phase4 needs no plan, because no part or relationship is added.
- **Relation to ADR-0009's `stage_part` (revision 3, review R2-5).** ADR-0009 revision 3
  (Proposed) resolves its R2-M2 by putting everything one operation changes, Phase1's cache
  stripping included, into the plan through `stage_part`. Phase4 keeps the in-place order
  above for its two multi-part cases. Each is a small, fully validated edit: one sheet
  element plus one relationship, or one sheet element plus one name. Staging would clone a
  possibly large sheet for every hyperlink, while step 4 is already proven not to fail
  after step 1 (the same reasoning as ADR-0011 §1). If ADR-0009's Accepted version makes
  `stage_part` the rule for every multi-part xlsx operation, Phase4 follows it, and the
  hyperlink and autofilter orders here change to that form.

**Phase1 and Phase2:**

| Phase | Interaction |
|---|---|
| Phase1 (formulas) | Range-feature formulas hold no cached value, so no invalidation. They are lexed and resolved with ADR-0003's tokenizer and §2.4 rules, including "fold-alike ⇒ ambiguous" (`Err(AmbiguousName)`). `merge_clearing` removes formulas through Phase1. Phase1's guards (`ArrayResultCell`, `vm`) also guard merges. The merged-anchor relaxation (§3) changes Phase0/1's `MergedCell` guard. |
| Phase2 (registries) | Conditional formats use the `dxfs` registry and the package-wide `numFmtId` allocator; hyperlink text and the optional "Hyperlink" style use the shared-string and style registries. No other style edit happens here. |

### 13. Errors

New variants:

- merges: `MergeOverlap`, `MergeWouldHideValues { cells }`, `NoSuchMerge`, `TableOverlap`,
  `PivotOverlap`, `QueryTableOverlap`;
- hyperlinks: `HyperlinkOverlap`, `InvalidUrl`, `UnknownSheet`;
- data validation: `ValidationOverlap`, `ListTooLong`, `ListItemHasComma`;
- references: `CrossSheetReference`;
- conditional formatting: `InvalidRuleFormula`, `PivotConditionalFormat`;
- names: `AmbiguousName`;
- autofilter: `AutoFilterExists`, `FilterActive`, `DuplicateBuiltInName`;
- panes: `NothingToFreeze`, `PageLayoutView`;
- columns: `AmbiguousColumns`, `InvalidOutlineLevel`;
- page setup: `InvalidHeaderFooter`;
- protection: `InvalidSpinCount`.

`RemovedWithBrokenX14Link` is a report, not an error. These pass through unchanged:

- core refusals: `UnresolvedSibling` when an `mc:AlternateContent` child makes the new
  element's position unresolvable, and `RegistryRefusal` from PR #38;
- ADR-0006 §2's refusal of a `dxfs` with `mc:AlternateContent`.

**Decision 2026-11-16 (PR #58 round 1 M3): keep the refusal, state its real scope.** Core places
a `maxOccurs=1` slot by a greedy assignment over all of the parent's children, so an AC child
**anywhere** in the parent refuses the insertion, not only one next to it. For `set_autofilter`
this matters: a new `definedNames` in `workbook.xml` is refused whenever the workbook has any AC
child. In the POI corpus (47dcff76), 81 of 349 `workbook.xml` files have one, all of them
Excel 2013+'s `x15ac:absPath`; 61 of those have no `definedNames`. So `set_autofilter` is refused
(`Core(UnresolvedSibling)`, every byte unchanged) on most recent, locally saved Excel files that
define no name yet (57 of the sweep's packages). Reusing or adding to an existing `definedNames`
is not affected. A separate core follow-up after PR #58 relaxes this: a single-occurrence slot can
be placed when every AC sibling's own slot is resolvable (as for `x15ac:absPath`, which sits
between `workbookPr` and `sheets` in every reading), with `check_*` twins and tests.

## Test oracles

For every feature:

1. **Byte-exact allowed diff**, generated from §2's table.
   - It runs on a from-scratch file (`Workbook::new`) and on an edit of an Excel-authored
     fixture that already has the feature:
     - `61605.xlsx` (merges);
     - `ExcelPivotTableSample.xlsx` (table and pivot overlap refusals);
     - `NewStyleConditionalFormattings.xlsx` (x14 links, `cfvo` formula);
     - `51850.xlsx` (x14 DV with `#REF!`);
     - `57362.xlsx` (page setup);
     - `57181.xlsm` (legacy sheet password replaced by SHA-512, M4).
   - **B1:** the three files with a leftover `_FilterDatabase` (`57523.xlsx`, `bug66827.xlsx`,
     `absolute-anchor-over-empty-sheet.xlsx`). `set_autofilter` must reuse the name: no second
     name in the scope.
   - **M3:** `57523.xlsx` and `bug66827.xlsx` keep their sheet-level `sortState` through
     set and remove.
   - L1 for every other part.
2. **CR with calamine** where it exposes the feature: merged ranges, and cell values for
   `merge_clearing`, hyperlink text and anchor writes in merged ranges (M1).
3. **Independent read-back with openpyxl** (pinned, offline, a CI job like the pptx read-back):
   - merges and hyperlinks (target and location);
   - data validations, conditional-formatting rules and priorities;
   - autofilter ref and freeze panes;
   - column widths, hidden and outline, row heights;
   - page setup, margins, print options;
   - protection flags.
4. **Hash vectors.**
   - The SHA-512 sheet hash is checked against `workbookProtection-sheet_password-2013.xlsx`,
     whose password is **`pwd`** (POI `TestXSSFSheet.java` L1357–1358). The review recomputed
     the file's `hashValue` with the salt-first order (`probe/h.py`).
   - It is also checked against independent `hashlib` vectors, committed with their outputs.
   - The legacy hash is checked against POI's `createXorVerifier1` on fixed inputs.
5. **Refusals change nothing** (L1, no part dirty) for every `Err` above. Fault injection at
   each apply step poisons the workbook (ADR-0003 §7.3), including between `Package::apply`
   and the sheet edit for a hyperlink.
6. **Phase3 coverage:** for every row of §12, a test renames and deletes the referenced sheet
   (once ADR-0009 is implemented), and checks the rewrite and the closed-world scan.
7. **XL** (owner, Microsoft 365 Excel):
   - one output per feature opens without a repair dialog and shows the feature;
   - x14-preserving edits keep data bars and cross-sheet lists working;
   - the items named in §3, §5, §9 and §10 (the single-value merge, 2007 cross-sheet lists,
     default column width and row height, the DEVMODE precedence, the header/footer limit).

## Alternatives considered

1. **Rebuild each container from a model** (openpyxl's approach). Simple, but every untouched
   rule and attribute is re-rendered (T2 fails), and unknown attributes or `extLst` content
   are lost. Rejected (ADR-0001).
2. **Write x14 forms for new rules too** (data-bar extensions, x14 cross-sheet DV and CF). More
   faithful to current Excel, but doubles the surface and needs GUID management and Excel
   checks per form. Deferred. Until then, cross-sheet DV/CF are refused, and defined names
   are the documented workaround (§5, §6).
3. **Renumber conditional-format priorities on every add** (Excel UI behaviour). Touches every
   rule; offered only as the explicit `add_conditional_format_first`.
4. **Allow merges over values and keep them** (what the corpus file has). Rejected as a
   default: it creates data that Excel and most readers hide.
5. **Remove unused hyperlink relationships.** Core allows it now, but sharing and Excel's own
   clean-up make it pointless (§4).
6. **Write the legacy protection hash by default** (simple, widely read). Rejected: it is
   trivially reversible, and current Excel writes SHA-512.
7. **`remove_autofilter` that unhides filtered rows.** Deferred as a separate
   `remove_autofilter_showing_rows` that reports the rows it shows. Unhiding is a visible
   change the caller should ask for, and the corpus has no active filter.
8. **Keep Phase0's full merged-range guard** and drop anchor writes from Phase4 (review M1's
   other option). It would make `opts.text` on merged hyperlinks impossible, and with it the
   common "merged title cell" edit. Rejected in favour of the documented relaxation.

## Consequences

- **Positive:**
  - The common formatting and structure edits work on existing files without losing anything
    else.
  - Every feature is locally confined and checked byte-exactly, against a table of allowed
    changes.
- **Positive:** every formula site Phase4 creates is covered by Phase3's rename and delete,
  and a missed one is refused, not silently left (§12).
- **Negative:**
  - x14-only forms, including cross-sheet DV and CF, cannot be created. Cross-sheet sources need
    a defined name.
  - Filter criteria, unhiding filtered rows, sparklines, header/footer images and partial
    `sqref` removal are out of scope.
- **Negative:** anchor writes into merged ranges change Phase0/1 behaviour. Callers that relied
  on the refusal see a success.
- **Negative:** Phase4 depends on Phase2 (`dxfs`), Phase3 (ADR-0009 rev 2: names, rename and
  delete tables) and PR #38 (relationships).

## Open questions for the project owner

**Owner decision 2026-11-16:** Q1–Q4 are decided as their proposed defaults below (merge refuses
by default, with the `merge_clearing` opt-in; cross-sheet list validation refused until the XL
check; CF priority `max + 1`; a salt option with the OS CSPRNG by default and spin count ≤
10,000,000). They were recorded first as the round-1 reviewer's recommendations:

1. **Merge default.** *Proposed default:* refusal is the default, and `merge_clearing` is
   opt-in. Values that disappear are data loss with Phase1 invalidation, so they need an
   explicit call. `MergeWouldHideValues { cells }` tells the caller which cells.
2. **Cross-sheet list validation.** *Proposed default:* refuse
   (`Err(CrossSheetReference)`) until the XL item passes (§5), and document the defined-name
   workaround. If Excel accepts the 2007 form without repair, the refusal is lifted.
3. **CF priority default.** *Proposed default:* `max + 1`; `add_conditional_format_first`
   renumbers, x14-only priorities included.
4. **Salt.** *Proposed default:* `Protection::with_salt([u8; 16])` as a public option (the salt
   is not secret), the OS CSPRNG by default, `with_spin_count` up to 10,000,000.

## Sources

- ECMA-376 Part 1, 5th ed., §18.3 (worksheet) and §18.2 (workbook) elements, read through the
  Transitional `sml.xsd` in `reference/ecma-376-transitional-xsd/`:
  - `cols` `maxOccurs` unbounded (L2124);
  - `CT_SheetFormatPr/@defaultRowHeight` required (L2225);
  - `CT_Col` (L2238–2249), `CT_Hyperlink` (L2744–2750);
  - `CT_SheetProtection` (L2887–, base64 hash and salt);
  - `CT_MergeCell`, `CT_DataValidation(s)`, `CT_ConditionalFormatting`, `CT_CfRule`,
    `CT_AutoFilter`, `CT_SheetView`, `CT_Pane`, `CT_PageSetup`, `CT_PageMargins`,
    `CT_WorkbookProtection`, `CT_ExtensionList`.
- [MS-XLSX] §2.6 (x14 extensions: `x14:conditionalFormattings`, `x14:dataValidations`).
- EPPlus wiki, "Data validation – Exceptions" (2007-form formulas reference only their own
  sheet); ZK Keikai KEIKAI-464 (Excel writes cross-sheet list DV in x14);
  PhpSpreadsheet #2149; XlsxWriter docs, "Working with Data Validation".
- Corpus: Apache POI `test-data/spreadsheet` at `47dcff769bb395034dddfd59c4609659b6dbc6a4` (full
  directory, 367 files); `spikes/xlsx-phase4/range_survey.py` (revision 2 counters `r2:`).
- Reference implementations: rust_xlsxwriter 0.102.0, openpyxl 3.1.5, Apache POI `47dcff76`
  (citations in "Reference libraries", re-verified by the review).
- Review: `docs/adr/0014-opus-review-round1.md`, probes `probe/p.py` and `probe/h.py`.
- ADR-0009 rev 2 (PR #27, `plan/xlsx-phase3` `94f8783`); core `RegistryPlan` (PR #38,
  `core/add-part`); Phase1 implementation `crates/ooxml-xlsx/src/edit.rs` (PR #20).
