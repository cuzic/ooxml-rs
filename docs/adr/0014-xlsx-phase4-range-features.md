# ADR-0014: xlsx Phase4 — range features on existing worksheets

- Status: **Proposed** (revision 1)
- Date: 2026-11-16
- Depends on:
  - [ADR-0001](0001-lossless-round-trip-architecture.md): Layer 1–3, T1/T1b/T2, append-only
    registries;
  - [ADR-0002](0002-schema-driven-code-generation.md): typed lens, allowlist, §4.3 insertion
    (schema slots, `mc:AlternateContent` refusal);
  - [ADR-0003](0003-xlsx-phase1-formula-and-calc-state.md) (PR #20): tokenizer, name and sheet
    resolution, plan → validate → apply, poisoning;
  - [ADR-0006](0006-xlsx-phase2-shared-strings-and-styles.md) (PR #22): the `dxfs` registry and
    `DxfFormat` (§8 of that ADR, reserved for this phase);
  - [ADR-0009](0009-xlsx-phase3-sheet-operations.md) (PR #27): `add_relationship`, defined-name
    writes, `set_print_area`/`set_print_titles`, and the rename rewrite of formula sites.
- Roadmap: [`docs/roadmap/xlsx.md`](../roadmap/xlsx.md) §2.6 (range-based features), §6 Phase4.
- Evidence: [`spikes/xlsx-phase4/`](../../spikes/xlsx-phase4/README.md) (corpus survey).

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

Every Phase4 feature inserts or edits one or two of these elements. ADR-0002 §4.1 already
places a new child in its slot and refuses next to an `mc:AlternateContent` sibling
(`UnresolvedSibling`); 1 corpus worksheet (`62629_target.xlsm`) has one.

### What the corpus shows

`spikes/xlsx-phase4/range_survey.py`, run with `python3 -I` on the local copy of Apache POI
`test-data/spreadsheet` @ `47dcff76` (350 files; 327 readable Transitional packages; the rest
are Strict, encrypted, or not zip) — counts are *files* / *occurrences*:

| Feature | Files | Occurrences | Notes |
|---|---|---|---|
| `mergeCells` | 33 | 3,783 merges on 57 sheets | 0 overlapping merges; **1 file (`60512.xlsm`) has 12 values in non-anchor cells** |
| hyperlinks | 16 | 68 external (`r:id`), 2 internal (`location`), 8 with neither, 3 with both | |
| `dataValidations` (2007) | 11 | 90 rules: decimal 21, textLength 20, whole 18, list 16, … | 1 inline list over 255 characters (`DataValidationListTooLong.xlsx`) |
| x14 data validation (`extLst`) | 3 | 7 | **all 7 have list sources on another sheet** |
| `conditionalFormatting` | 16 | 55 blocks; cellIs 27, expression 19, iconSet 13, dataBar 2, colorScale 2, … | 48 rules carry `dxfId`; no duplicate priorities |
| x14 conditional formatting | 2 | 4 `x14:cfRule`, all linked by `x14:id` | no dangling ids |
| `autoFilter` (sheet) | 1 | 1 | `_xlnm._FilterDatabase` in 4 files; `sortState` in 4 |
| panes | 15 | 25 frozen, 2 split | no file with more than one `sheetView` |
| `cols/col` | 175 | 1,855 elements, 410 multi-column ranges | 14 hidden, 24 with outline levels |
| row attributes | 97 / 9 / 27 | `ht` 9,085, hidden 165, outline 4,748 | |
| page setup | 339 / 161 / 51 / 66 | `pageMargins` / `pageSetup` / `printOptions` / `headerFooter` | `rowBreaks` 2, `colBreaks` 2 |
| print names | 8 / 5 | `_xlnm.Print_Area` 14, `_xlnm.Print_Titles` 9 | |
| `sheetProtection` | 9 | 10 flags only, 1 SHA-512 | no legacy `password` on sheets |
| `workbookProtection` | 27 | 25 flags only, 1 legacy, 1 SHA-512 | |

Facts that shape the design:

1. **Non-anchor values in merged ranges exist** in Excel-written files (12 in `60512.xlsm`). A
   reader that shows only the anchor hides them; ooxml-rs must neither delete them silently on
   unrelated edits nor create new hidden ones without saying so.
2. **x14 data validations are, in this corpus, always lists on another sheet.** Excel 2010 and
   later write that form for cross-sheet list sources in some versions; the 2007 form with a
   sheet-qualified `formula1` is what openpyxl and rust_xlsxwriter write (see "Reference
   libraries"). Phase4 must read and preserve both and must not create a duplicate rule in the
   other form.
3. **x14 conditional formatting is always linked** by `x14:id` between a 2007 `cfRule`'s
   `extLst` and an `x14:cfRule` in the worksheet `extLst`. Deleting or re-prioritising one side
   without the other corrupts the pair.
4. **`cols/col` ranges are common** (410): changing one column of a range means splitting the
   element.
5. **Protection hashes**: SHA-512 with salt and spin count is what current Excel writes; the
   legacy 16-bit `password` hash still appears (workbook level, 1 file).
6. **Range features carry formulas** outside cells: data-validation `formula1`/`formula2` (and
   x14 `xm:f`), conditional-formatting `formula`, print names, hyperlink `location`, autofilter
   custom filters. They hold no cached value (Phase1's invalidation does not apply), but they
   contain sheet and name references that Phase3's rename must rewrite (ADR-0009 §6 already
   lists these sites).

### Reference libraries

Verified in source: rust_xlsxwriter 0.102.0 (**RX**, `src/`), openpyxl 3.1.5 (**OP**,
`openpyxl/`), Apache POI @ `47dcff76` (**POI**, `poi-ooxml/…/xssf/usermodel/` unless noted).
Only one of them (POI) edits a loaded tree in place, and none of them keeps an
unmodelled element byte-for-byte: RX writes from scratch, OP rebuilds every element from its
model and **drops every worksheet `extLst`, including x14 data validation and x14 conditional
formatting** (OP `worksheet/_reader.py:324-329`, `formatting/rule.py:164-167`).

| Feature | RX | OP | POI |
|---|---|---|---|
| Merge | needs ≥ 2 cells, overlap → `MergeRangeOverlaps`; writes a **blank cell with the anchor's format** in every other cell (`worksheet.rs:5002-5057`); no unmerge | no overlap check (`cell_range.py:467-477`); **discards non-anchor values** and copies edge borders (`worksheet.py:605-614`, `merge.py:93-133`) | ≥ 2 cells; overlap with merges or array formulas → `IllegalStateException`; values untouched (`XSSFSheet.java:338-368`); removing the last merge removes `mergeCells` (`:1973-1990`) |
| Hyperlink | writes the display text with the hyperlink format; URL ≤ 2080 characters, tooltip ≤ 255 (`url.rs:14, 363-375`); `rId`s renumbered per save (`worksheet.rs:18524-18533`) | sets the cell value only if empty, no style (`cell/cell.py:231-246`); rels renumbered per save, so removing a link drops its relationship (`_writer.py:54, 199-210`) | adds an external relationship only for new links (`XSSFHyperlink.java:151-160`); `removeHyperlink` keeps the relationship (`XSSFSheet.java:3657-3659`, inference) |
| Data validation | 2007 form only; inline list ≤ 255 UTF-16 units, titles ≤ 32, messages ≤ 255 (`data_validation.rs:555-576, 1060-1282`); cross-sheet list formula passed through (`:638-643`) | no limits; x14 forms dropped on load | inline list ≤ 257 including quotes (`XSSFDataValidationConstraint.java:36, 212-213`); messages ≤ 255 (`XSSFDataValidation.java:36, 107-131`); no x14 API |
| Conditional formatting | priority = per-sheet counter (`worksheet.rs:9313-9333`); data bars carry x14 extensions by default and x14-only icon sets are x14-only (`conditional_format.rs:4476, 5803-5807`); deterministic pseudo-GUID `x14:id` (`worksheet.rs:18556-18618`) | assigns `max_priority + 1` only to rules without one, no uniqueness check (`formatting/formatting.py:64-79`); dxfs deduplicated by equality | priority = 1 + number of existing rules (`XSSFSheetConditionalFormatting.java:210-218`); `putDxf` appends without dedup (`StylesTable.java:815-818`); data bars 2007 only |
| AutoFilter | `_xlnm._FilterDatabase` local, hidden, `'Sheet'!$A$1:$D$5` (`workbook.rs:3648-3657`); hides non-matching rows itself (`worksheet.rs:15182`); table overlap → error (`:8692-8703`) | writes `_FilterDatabase` on every save (`workbook/_writer.py:104-107`); sheet `sortState` neither read nor written | creates or reuses `_FilterDatabase`, hidden (`XSSFSheet.java:4417-4441`) |
| Panes | one `sheetView`; freeze only, with one selection per pane (`worksheet.rs:18401-18453`) | `sheetView[0]` (`worksheet/views.py:150-155`) | the **last** `sheetView` is the default (`XSSFSheet.java:3784-3794`); split = freeze + `state=split` (`:801-813`) |
| Columns | merges adjacent equal columns (`worksheet.rs:19899-19943`) | no split: editing one column of a range writes an overlapping `col` (`dimensions.py:176-181`, inference) | splits ranges into before/target/after (`ColumnHelper.java:184-212`); raises `sheetFormatPr` outline levels (`XSSFSheet.java:3522-3550`) |
| Margins (default) | 0.7/0.7/0.75/0.75/0.3/0.3 (`worksheet.rs:1808-1813`) | **0.75/0.75/1/1/0.5/0.5** (`worksheet/page.py:167-168`) | 0.7/0.7/0.75/0.75/0.3/0.3 (`XSSFSheet.java:83-88`) |
| Print titles | columns then rows (`worksheet.rs:21379-21410`) | rows then columns, sheet name always quoted (`print_settings.py:150-153`, `utils/cell.py:232-240`) | columns then rows (`XSSFSheet.java:4665-4700`) |
| Protection | legacy hash only (UTF-8 bytes, `%04X`) (`utility.rs:1435-1453`); `objects`/`scenarios` written unless allowed (`worksheet.rs:20133-20192`) | stores SHA attributes, computes only legacy (code points, no zero padding) (`utils/protection.py:14-22`) | SHA-512: H(salt ‖ UTF-16LE(pw)), then 100,000 × H(hash ‖ LE int32 i), 16-byte random salt (`poi/…/CryptoFunctions.java:123-150`, `XSSFPasswordHelper.java:61-75`); legacy via `createXorVerifier1` (`CryptoFunctions.java:399-427`) |

What this means for an editing library:

- Every reference library loses or rewrites something an Excel-authored file contains (OP
  drops x14 and `sortState`, renumbers rels; RX cannot edit; POI rewrites `hyperlinks` on
  save and does not dedupe dxfs). ooxml-rs's element-local edits are new ground and need the
  byte-exact oracle (§2), not a comparison with these libraries' output.
- The libraries disagree on margins defaults, print-titles order, the default `sheetView`, the
  legacy hash's character encoding, and whether merging clears values. The ADR picks Excel's
  observable behaviour where it is documented by the corpus (one `sheetView`, margins
  0.7/0.75/0.3) and the safe choice otherwise (refuse to hide values by default).

## Decision

### 1. Scope and shape

Phase4 adds, on any worksheet opened with `worksheet_by_name`/`worksheet_by_index`:

| Area | API (sketch) |
|---|---|
| Merged cells | `merge(range)`, `merge_clearing(range)`, `unmerge(range)`, `merged_ranges()` |
| Hyperlinks | `set_hyperlink(cell_or_range, Link::Url(..) \| Link::Location(..), opts)`, `remove_hyperlink(range)`, `hyperlinks()` |
| Data validation | `add_validation(sqref, Validation)`, `remove_validation(sqref)`, `validations()` |
| Conditional formatting | `add_conditional_format(sqref, Rule)` (lowest priority), `add_conditional_format_first(..)` (renumbers), `remove_conditional_format(..)`, `conditional_formats()` |
| AutoFilter | `set_autofilter(range)`, `remove_autofilter()` |
| Views | `freeze_panes(row, col)`, `split_panes(x, y)`, `unfreeze()`, `set_zoom`, `set_show_gridlines`, `set_right_to_left` |
| Columns and rows | `set_column_width(cols, w)`, `set_column_hidden`, `set_column_outline(cols, level)`, `set_row_height(row, h)`, `set_row_hidden`, `set_row_outline` |
| Page setup | `page_setup_mut()` (orientation, paper, scale, fit-to), `set_margins`, `set_print_options`, `set_header_footer`, `add_row_break`/`add_col_break`; print area/titles through ADR-0009's setters |
| Protection | `protect_sheet(Protection)`, `unprotect_sheet()`, `protect_workbook(..)` |

Every call is one edit through ADR-0003 §3.1: **plan and validate without mutation, then apply
in place; an apply failure poisons the workbook.** Every guard below is checked in the plan.
Reads (`merged_ranges()`, `hyperlinks()`, …) return typed values decoded through the lens and
never change the tree.

Each feature's write path is a set of **allowlist entries** (ADR-0002 §5) promoted from read to
write in `codegen/allowlist/sml.toml`, plus hand-written x14 lenses for the `extLst` forms
(§5, §6). Nothing outside those elements is re-rendered.

### 2. Locality and the byte-exact oracle

Phase1's byte-exact allowed-diff oracle (ADR-0003 §7, R3-M2) is reused: for each call, the
expected output is the input with exactly the feature's edits applied, and every other byte and
part must be identical. The allowed edits per call are:

- the feature's element (e.g. one `mergeCell`, one `hyperlink`, one `dataValidation`), its
  container's `count` attribute, and the container itself when it is created or becomes empty;
- the cells the feature must write (only `merge_clearing` and the optional hyperlink text);
- the registries a feature extends: a `dxf` (Phase2 registry), a relationship (ADR-0009
  `add_relationship`), a defined name (ADR-0009 names), a `sheetFormatPr` outline attribute;
- documented, larger touches, each opt-in: `add_conditional_format_first` renumbers the
  sheet's priorities; `merge_clearing` clears cells (a Phase1 edit: dependents invalidated).

A container that becomes empty is **removed** (Excel repairs an empty `mergeCells`,
`hyperlinks` or `dataValidations`; an empty `conditionalFormatting` is invalid).

### 3. Merged cells

- **Create.** `merge(range)` needs at least two cells and refuses (`Err`, nothing changed):
  - overlap with an existing merge (`MergeOverlap`);
  - overlap with a table range (`TableOverlap`), with an array/data-table/spill result range or
    anchor (Phase1 `ArrayResultCell`/`ArrayAnchor`), or with a `vm` cell;
  - **any value or formula in a non-anchor cell** (`MergeWouldHideValues { cells }`). Excel's
    UI warns and discards them; a library must not hide data silently.
- `merge_clearing(range)` does what Excel does after its warning: it clears non-anchor values
  and formulas (each a Phase1 removal: dependents invalidated, calcChain removed if the formula
  set changes) and then merges. Styles of non-anchor cells are left as they are (Excel keeps
  border formatting on them).
- **Remove.** `unmerge(range)` removes exactly the `mergeCell` whose `ref` equals the range
  (`NoSuchMerge` otherwise); non-anchor cells stay empty.
- Existing hidden values (fact 1) are untouched by every other Phase4 call, and Phase0's
  `MergedCell` guard keeps refusing writes into any merged cell.
- `count` is updated. calamine exposes merges, so CR covers this feature.

### 4. Hyperlinks

- **External** (`Link::Url`): a `hyperlink` with `r:id` pointing at a new
  `TargetMode="External"` relationship of the sheet, added with ADR-0009's
  `add_relationship` (append-only; ids never reused). URLs are validated (scheme present, at
  most 2,080 characters, as RX enforces; tooltips at most 255) and stored as given; `#fragment` goes to
  `location` as Excel does.
- **Internal** (`Link::Location("Sheet2!A1")`): only `location`; the target is lexed with the
  Phase1 tokenizer and resolved with ADR-0003 §2.4 (unknown sheet → `Err(UnknownSheet)`).
- `display` and `tooltip` attributes are optional. The **cell value is not changed** unless
  the caller asks (`opts.text`), which writes an inline string through Phase0; applying the
  workbook's "Hyperlink" cell style is a Phase2 named-style lookup and is opt-in.
- A new hyperlink whose `ref` overlaps an existing one is refused (`HyperlinkOverlap`).
- **Removal** removes the `hyperlink` element and keeps the now unused relationship: an unused
  relationship is valid OPC and harmless, and removing it would widen the registry-removal
  exception (ADR-0001 note) for no benefit.
- Hyperlinks in merged ranges are allowed on the anchor only.

### 5. Data validation

- `Validation` covers the 2007 attributes: type (whole, decimal, list, date, time, textLength,
  custom), operator, `formula1`/`formula2`, blank handling, input and error messages, error
  style, drop-down suppression. Titles are limited to 32 characters and messages to 255 (RX
  and POI enforce the same; Excel truncates or repairs longer ones).
- **Formulas** are lexed with the Phase1 tokenizer and stored without `=`; references to other
  sheets and names are resolved by ADR-0003 §2.4 (unknown sheet → `Err`). They are relative to
  the top-left cell of the first range of `sqref`, as Excel evaluates them.
- **Inline lists** (`"a,b,c"`) are limited to 255 characters (Excel's limit; 1 corpus file
  exceeds it and Excel repairs it): longer lists are refused (`ListTooLong`), and the caller
  can point the list at a range instead.
- New rules are always written in the **2007 form**, including cross-sheet list sources
  (`formula1` = `Sheet2!$A$1:$A$9`), as openpyxl and rust_xlsxwriter do. Whether every Excel
  version accepts that without rewriting it to x14 is an XL item.
- **x14 rules** (`extLst/ext[@uri="{CCE6A557-97BC-4b89-ADB6-D9C93CAAB3DF}"]/x14:dataValidations`)
  are read (hand-written lens: `xm:sqref`, `x14:formula1/xm:f`) and preserved. A new 2007 rule
  whose `sqref` overlaps an x14 rule's, or another 2007 rule's, is refused
  (`ValidationOverlap`): a cell has one validation in Excel.
- `remove_validation(sqref)` removes rules whose `sqref` is exactly the given one (2007 or x14);
  partial removal (splitting an `sqref`) is a later refinement.

### 6. Conditional formatting

- **Formats** come from the Phase2 `dxfs` registry (`DxfFormat`, ADR-0006 §8): append-only,
  deduplicated among new entries, never copying default-style ids into a `dxf`.
- **Priorities** are unique per sheet across 2007 `cfRule`s and `x14:cfRule`s (spec: lower
  number wins). `add_conditional_format` gives the new rule `max + 1` (lowest priority), so no
  existing element changes. `add_conditional_format_first` gives it priority 1 and renumbers
  every other rule (documented T2 widening). Duplicate priorities already in a file are left as
  they are, and both calls still produce unique values for new rules.
- **Rule kinds written:** cellIs, expression, containsText/notContainsText/beginsWith/endsWith,
  top10, aboveAverage, duplicate/unique values, timePeriod, colorScale, dataBar and iconSet in
  their 2007 forms. **x14-only forms** (data bar extensions, the additional icon sets, rules
  referencing other sheets) are out of scope for writing; existing ones are read and preserved.
- **x14 links** (fact 3): removing a 2007 rule that carries an `x14:id` also removes the linked
  `x14:cfRule` (and its now empty `x14:conditionalFormatting`, and the `ext` if it becomes
  empty). If the link cannot be resolved (no x14 rule with that id), the removal is refused
  (`BrokenX14Link`) rather than leaving half a pair.
- **Formulas** (expression rules, cellIs operands) follow §5's formula rules; they are relative
  to the top-left cell of the first range of `sqref`.
- `extLst` editing keeps every `ext` this phase does not own byte-identical and inserts new
  `ext` elements after existing ones (`CT_ExtensionList` imposes no order among `ext`s).

### 7. AutoFilter and sort state

- `set_autofilter(range)` writes `autoFilter/@ref` and the hidden sheet-local defined name
  `_xlnm._FilterDatabase` (`hidden="1"`, `localSheetId` = the sheet's position, text
  `'Sheet'!$A$1:$D$9`). ADR-0009 §9 reserves that name for this phase.
- A range overlapping a table is refused (`TableOverlap`): tables carry their own `autoFilter`.
- **Filter criteria are out of scope**: writing a `filterColumn` without hiding the rows it
  excludes leaves a sheet that disagrees with itself until Excel re-applies the filter.
  Existing `filterColumn` and `sortState` are preserved; `remove_autofilter` removes the element,
  any `sortState` that refers to its range, and the `_FilterDatabase` name.

### 8. Views and panes

- Edits apply to the `sheetView` whose `workbookViewId` is 0 (every corpus file has exactly one
  `sheetView`); a sheet without one gets one in its slot. Other `sheetView`s are untouched.
- `freeze_panes(row, col)` writes `pane` with `xSplit`/`ySplit` (counts), `topLeftCell`,
  `activePane` (bottomRight/bottomLeft/topRight by which splits exist) and `state="frozen"`,
  and replaces the `selection` elements with one per pane, as Excel does. `split_panes` writes
  `state="split"` with twips. `unfreeze` removes `pane` and the pane-specific `selection`s.
- `set_zoom` (10–400, `zoomScale`), `set_show_gridlines`, `set_right_to_left`: attributes only.
- Active sheet and tab selection (`bookViews/workbookView/@activeTab`, `sheetView/@tabSelected`)
  belong to Phase3's sheet operations and are not duplicated here.

### 9. Columns and rows

- `set_column_width(cols, width)` and the hidden/outline setters edit `cols/col` ranges:
  - an existing `col` exactly covering the range is edited in place;
  - a `col` partly covering it is **split** into up to three elements (left, middle, right),
    each keeping the other attributes; adjacent equal ranges are **not** merged afterwards
    (locality);
  - missing coverage gets new `col` elements in `min` order; `cols` is created in its slot.
- Widths are stored as given, in Excel character units (`width`), with `customWidth="1"`;
  widths above 255 are refused (POI's limit). Pixel conversions are a convenience for later.
- Row height/hidden/outline edit `row` attributes (`ht` + `customHeight="1"`, `hidden`,
  `outlineLevel`), creating an empty row in order when needed (Phase0's row insertion, with
  the row index of PR #29).
- Outline levels are 0–7; `sheetFormatPr/@outlineLevelRow`/`@outlineLevelCol` are raised to
  the maximum level present (they are never lowered automatically: that would require a scan,
  and Excel only uses them for the outline buttons).

### 10. Page setup, margins, print

- `pageSetup` attributes are edited in place; an existing `r:id` (printer settings binary) is
  kept. Fit-to-page also sets `sheetPr/pageSetUpPr/@fitToPage="1"`.
- `pageMargins` needs all six attributes; when it is absent, Excel's defaults are written
  (left/right 0.7, top/bottom 0.75, header/footer 0.3, as RX and POI; OP's 0.75/1/0.5 are
  older defaults).
- `printOptions`: gridlines, headings, centring.
- `headerFooter`: odd/even/first text with Excel's `&` codes, validated (length ≤ 255
  characters per section after codes, as Excel limits; codes checked by a small parser) and
  escaped; images in headers (`legacyDrawingHF`) are out of scope.
- Manual breaks: `rowBreaks`/`colBreaks` `brk` elements with `man="1"`, `count` and
  `manualBreakCount` updated.
- Print area and print titles: ADR-0009 §9's `set_print_area`/`set_print_titles` (Phase3), not
  re-specified here.

### 11. Protection

- `protect_sheet(Protection)` writes `sheetProtection` with `sheet="1"`, `objects="1"` and
  `scenarios="1"` (as RX and POI) and the flags the caller chooses. With a password, it writes the **SHA-512 form**: `algorithmName="SHA-512"`,
  a random 16-byte `saltValue`, `spinCount="100000"` and `hashValue`, computed as ECMA-376
  specifies for `CT_SheetProtection`/`CT_WorkbookProtection` (hash of salt + UTF-16LE password, then `spinCount`
  iterations of hash(previous ‖ iteration as 32-bit little-endian)).
- The **legacy** 16-bit `password` hash is read, preserved and never written by default; a
  `Protection::legacy_hash` option exists only for consumers that do not read the SHA form. It
  uses POI's "ANSI" byte rule and writes `%04X` (RX hashes UTF-8 bytes and OP code points
  without zero padding, so the three disagree for non-ASCII passwords).
- `unprotect_sheet` removes the element. No call verifies or recovers passwords.
- `protect_workbook` writes `workbookProtection` (structure/windows) with the same hash rules
  (`workbookAlgorithmName`, …) in `workbook.xml`.
- Sheet protection does not stop ooxml-rs edits (it is an Excel UI setting); the API docs say
  so.

### 12. Interaction with Phase1, Phase2 and Phase3

| Phase | Interaction |
|---|---|
| Phase1 (formulas) | Range-feature formulas hold no cached value, so no invalidation. They are lexed and resolved with ADR-0003's tokenizer and §2.4 rules, including the "fold-alike ⇒ ambiguous" rule (`Err(AmbiguousName)` here, since there is nothing to fall back to). `merge_clearing` removes formulas through Phase1. Phase1's guards (`ArrayResultCell`, `vm`) also guard merges. |
| Phase2 (registries) | Conditional formats use the `dxfs` registry; hyperlink text and the optional "Hyperlink" style use the shared-string/style registries. No other style edit happens here. |
| Phase3 (sheets, names) | Hyperlinks need `add_relationship`; autofilter needs `_xlnm._FilterDatabase` writes; print area/titles are Phase3 setters. Every formula site created here (DV, CF, x14 `xm:f`, `location`, `_FilterDatabase`) is one ADR-0009 §6 already rewrites on rename; deleting a sheet makes cross-sheet DV/CF references `#REF!` as Excel does (ADR-0009 §5). |

### 13. Errors

New variants: `MergeOverlap`, `MergeWouldHideValues { cells }`, `NoSuchMerge`,
`TableOverlap`, `HyperlinkOverlap`, `InvalidUrl`, `UnknownSheet`, `ValidationOverlap`,
`ListTooLong`, `InvalidRuleFormula`, `BrokenX14Link`, `AmbiguousName`, `InvalidOutlineLevel`,
`InvalidHeaderFooter`. Core refusals (`UnresolvedSibling` next to `mc:AlternateContent`) pass
through unchanged.

## Test oracles

For every feature:

1. **Byte-exact allowed diff** (§2) on a from-scratch file (`Workbook::new`) and on an edit of
   an Excel-authored fixture that already has the feature (e.g. `61605.xlsx` merges,
   `ExcelPivotTableSample.xlsx` table overlap refusals, `NewStyleConditionalFormattings.xlsx`
   x14 links, `51850.xlsx` x14 DV, `57362.xlsx` page setup), L1 for every other part.
2. **CR with calamine** where it exposes the feature (merged ranges; cell values for
   `merge_clearing` and hyperlink text).
3. **Independent read-back with openpyxl** (pinned, offline, CI job like the pptx read-back):
   merges, hyperlinks (target and location), data validations, conditional formatting rules
   and priorities, autofilter ref, freeze panes, column widths and hidden/outline, row heights,
   page setup, margins, print options, protection flags. openpyxl's model is independent of
   ooxml-rs's and catches attribute-level mistakes the byte oracle cannot judge.
4. **Hash vectors**: the SHA-512 sheet hash is checked against the corpus file
   `workbookProtection-sheet_password-2013.xlsx` (its password comes from POI's test for that
   file, to be confirmed when the test is written) and against independently computed vectors
   (a short Python `hashlib` script, committed with its outputs); the legacy hash against
   POI's `createXorVerifier1` on fixed inputs.
5. **Refusals change nothing** (L1, no part dirty) for every `Err` above, and fault injection
   at each apply step poisons the workbook (ADR-0003 §7.3).
6. **XL** (owner, Microsoft 365 Excel): one output per feature opens without a repair dialog
   and shows the feature; x14-preserving edits keep data bars and cross-sheet lists working.

## Alternatives considered

1. **Rebuild each container from a model** (openpyxl's approach: parse the whole element into
   objects, write it back). Simple, but every untouched rule and attribute is re-rendered (T2
   fails), and unknown attributes or `extLst` content are lost. Rejected (ADR-0001).
2. **Write x14 forms for new rules too** (data bar extensions, x14 data validation). More
   faithful to current Excel, but doubles the surface and needs GUID management and Excel
   checks per form. Deferred; preserving existing x14 content is enough for edit workflows.
3. **Renumber conditional-format priorities on every add** (Excel UI behaviour). Touches every
   rule; offered only as the explicit `add_conditional_format_first`.
4. **Allow merges over values and keep them** (what the corpus file has). Rejected as a
   default: it creates data that Excel and most readers hide.
5. **Remove unused hyperlink relationships.** Saves a few bytes, widens the registry-removal
   exception; rejected (§4).
6. **Write the legacy protection hash by default** (simple, widely read). Rejected: it is
   trivially reversible, and current Excel writes SHA-512.

## Consequences

- Positive: the common formatting-and-structure edits work on existing files without losing
  anything else; every feature is locally confined and checked byte-exactly.
- Positive: formula sites in range features are known and lexed, so Phase3 renames stay
  correct.
- Negative: x14-only forms cannot be created; files that need them still need Excel.
- Negative: filter criteria, sparklines, header/footer images, and partial removal of a
  validation's `sqref` are out of scope.
- Negative: Phase4 depends on Phase2 (`dxfs`) and Phase3 (relationships, names) being
  implemented first.

## Open questions

1. Should `merge` default to `merge_clearing`'s behaviour (Excel's), with the refusal as an
   option, instead of the reverse?
2. Is the 2007 form for cross-sheet list validation acceptable in every Excel the owner
   targets, or should cross-sheet lists be written in x14 form?
3. Should `add_conditional_format` default to Excel's "new rule first" priority (renumbering)?
4. Is SHA-512 with a random salt acceptable for reproducible builds, or should a deterministic
   salt option exist for tests only?

## Sources

- ECMA-376 Part 1, 5th ed., §18.3 (worksheet) and §18.2 (workbook) elements, read through the
  Transitional `sml.xsd` in `reference/ecma-376-transitional-xsd/` (`CT_Worksheet`,
  `CT_MergeCell`, `CT_Hyperlink`, `CT_DataValidation(s)`, `CT_ConditionalFormatting`,
  `CT_CfRule`, `CT_AutoFilter`, `CT_SheetView`, `CT_Pane`, `CT_Col`, `CT_PageSetup`,
  `CT_PageMargins`, `CT_SheetProtection`, `CT_WorkbookProtection`, `CT_ExtensionList`).
- [MS-XLSX] §2.6 (x14 extensions: `x14:conditionalFormattings`, `x14:dataValidations`).
- Corpus: Apache POI `test-data/spreadsheet` @ `47dcff76` (local copy), committed fixtures;
  `spikes/xlsx-phase4/range_survey.py`.
- Reference implementations: see "Reference libraries".
