# spikes/xlsx-phase7

Corpus evidence for [ADR-0018](../../docs/adr/0018-xlsx-phase7-structural-edits.md) (xlsx
Phase7, structural edits: insert/delete rows and columns, copy a worksheet, rename a table or a
table column). The script only *reads* untrusted files with `zipfile` and
`xml.etree.ElementTree`; parts with a DTD or an entity declaration are skipped, no entity is
resolved, VML is scanned with regular expressions, and nothing is extracted or executed. Run it
with `python3 -I` from the repository root, passing data directories as arguments.

| Script | Purpose |
|---|---|
| `structural_survey.py [--summary] DIR…` | One JSON line per package. Counts every **reference-bearing place** of ADR-0018 §3's location registry: `sheetData` (`r` on rows and cells, rows/cells without `r`, `spans`), `dimension`, `cols`, merges, hyperlinks, sheet autofilter and sort state, CF (2007 blocks, rules, `sqref` lists, formulas, `cfvo` formulas), DV, worksheet `extLst` (x14 CF/DV `xm:f`/`xm:sqref`, sparklines, other URIs), views (`selection`, `pane`, `topLeftCell`), breaks, `ignoredErrors`, `protectedRanges`, `customSheetViews`, `dataConsolidate`, `scenarios`, OLE/controls, `mc:AlternateContent` (top level, in `sheetData`, in rows), notes and their VML (`x:Row`, `x:Column`, `x:Anchor`, `x:Fmla*`), threaded comments, drawing anchors (kind, `editAs`, AC-wrapped, `textlink`), tables (`ref`, autofilter, totals, types, own `.rels`, calculated-column formulas), charts (`c:f`, `c15:f`, AC, pivot charts, chartEx), pivot caches (source by `ref` or by name/table) and pivot tables, defined names (scope, `_xlnm.*`, constants, relative or unprefixed references), calcChain entries, connections, query tables, `singleXmlCells`, external links, `xr:uid` attributes. **Formula reference forms** per location (cell formulas, names, CF, DV, x14, charts, table formulas), from an approximate lexer: cell, area, whole columns, whole rows, `A1:A1048576`-style full areas, sheet-prefixed, 3-D, external, structured (table-less, `#This Row`/`@`, column ranges), names, `#REF!` (bare and sheet-prefixed), spill (`A1#`), R1C1-shaped names, `INDIRECT`/`OFFSET`/`ROW`/… calls, sheet references inside string literals. **G1 blockers** from the registry's XX set (`sheet_blocker.*`, `sheet_no_blocker`), protection (`sheet_protected*`, `wb_lockStructure`), `codeName`, extension URIs per worksheet (`ext_uri_worksheet.*`). **Copy blockers** per sheet (`sheet_copy_blocker.*`, `sheet_copyable`). **Shared groups**: masters, 2-D blocks, sizes, and a **simulation** (`sim_shared_*`): for every multi-row group, insert one row (and, separately, delete one row) in the middle of the group, apply ADR-0018 §3.2's model to each member's own formula, and check whether every member still equals the edited master translated to it (`same`) or the group must be split (`split`). `--summary` adds `#` lines: files and totals per counter, the sheet max-row distribution and the shared-group sizes. |

Reproducing the ADR's numbers (files are not committed; Apache-2.0 / MIT):

```sh
# fetch POI test-data/spreadsheet at the pinned commit (as in spikes/xlsx-phase5/README.md)
SHA=47dcff769bb395034dddfd59c4609659b6dbc6a4
gh api "repos/apache/poi/git/trees/$SHA?recursive=1" \
  --jq '.tree[]|select(.path|startswith("test-data/spreadsheet/"))|select(.path|test("\\.(xlsx|xlsm)$"))|.path' > poi.txt
mkdir -p /tmp/poi-xlsx && while read -r p; do
  curl -sfL -o "/tmp/poi-xlsx/$(basename "$p")" "https://raw.githubusercontent.com/apache/poi/$SHA/$p"; done < poi.txt
python3 -I spikes/xlsx-phase7/structural_survey.py --summary /tmp/poi-xlsx reference/fixtures/xlsx/calamine | grep '^#'
# the hand-built Phase1 fixtures (data tables, cm, 3-D) separately:
python3 -I spikes/xlsx-phase7/structural_survey.py --summary reference/fixtures/xlsx/synthetic | grep '^#'
```

Measured on 2026-11-20 (Python 3.15): 367 paths listed, 366 POI files fetched, plus the 2
calamine fixtures; **343 packages read** (the rest are Strict, encrypted, fuzzing cases or not
zip). Runs in about 7 s. The synthetic directory: 28 packages.

## Results (POI + calamine)

"files" is the number of packages with at least one occurrence.

**Size.** 813 worksheets, 1,502,954 cells. Highest used row per sheet: median 3, 99th
percentile 3,799, maximum 1,048,576 (one sheet). 4 sheets have 10,000 rows or more.
`row/@spans` on 88,922 rows (275 files). **Rows without `r`: 236 (3 files); cells without `r`:
5,561 (4 files)**; 15 sheets in all.

**Formula locations and how many texts each holds.**

| Location | Texts | Files |
|---|---|---|
| cell formulas (`f` with text) | 11,561 | 126 |
| defined names | 858 | 62 |
| chart `c:f` | 174 | 25 |
| DV `formula1`/`formula2` | 122 | 12 |
| CF `cfRule/formula` | 89 | 18 |
| x14 `xm:f` | 25 | 6 |
| table `calculatedColumnFormula` | 16 | 8 |
| table `totalsRowFormula`, `c15:f`, `cfvo` formulas | 0, 0, 2 | –, –, 2 |
| `connections.xml` `parameter/@cell` | 12 | 1 |

**Cell formulas.** `t="shared"` on 19,558 cells, 359 masters (48 files), 58 of them 2-D blocks
(5 files); group size median 18 cells, max 684. `t="array"` 53 (6 files), **43 multi-cell (5
files)**. `t="dataTable"`: 0 (1 in the synthetic set). `cm` 1, `vm` 2. Lexer failures: 3 texts
(1 file).

**Reference forms in cell formulas** (number of formula texts containing the form):

| Form | Texts | Files |
|---|---|---|
| single cell | 7,702 | 78 |
| area | 3,113 | 45 |
| whole columns (`A:C`) | 1,644 | 5 |
| whole rows (`1:3`) | 5 | 3 |
| sheet-prefixed | 6,147 | 14 |
| 3-D (`Sheet1:Sheet3!`) | 16 | 3 |
| external (`[n]`) | 14 | 5 |
| structured references | 4,022 | 11 |
| … with `#This Row`/`@` | 3,897 | 6 |
| … with a column range `[A]:[B]` | 5 | 2 |
| defined-name use | 52 | 16 |
| `#REF!` | 335 | 6 |
| … sheet-prefixed `Sheet!#REF!` | 1 | 1 |
| spill `A1#`, R1C1-shaped names, `A1:A1048576`-style full areas | 0 | – |
| `INDIRECT` / `OFFSET` / `INDEX` / `ROW` | 3,801 / 11 / 3 / 5,490 | 2 / 1 / 2 / 5 |

**Defined names.** 858 (62 files): sheet-local 142 (24), constants 240 (7), `#REF!` 228 (12;
13 of them sheet-prefixed, 4 files), structured references 14 (6). `Print_Area` 16 (9),
`Print_Titles` 11 (7), `_FilterDatabase` 8 (6). **No name has an unprefixed reference or a
relative component** (0 of 858).

**CF and DV.** CF: 108 blocks (20 files), 131 rules, 11 blocks with several `sqref` ranges (6);
27 CF formulas with a relative cell reference (8). DV: 108 (13 files), 6 with several ranges
(1); 44 DV formulas with a relative reference (3). x14 extensions: CF in 2 files, DV in 3,
`xm:sqref` 22 (6). Sparklines 11 (1 file). The other worksheet `ext` URIs (`mac/excel/2008`,
`{64002731-…}`) hold only Mac page-layout settings (`mx:PLV`), no reference.

**Other sheet places.**

| Place | Count | Files |
|---|---|---|
| `mergeCell` | 54,468 | 43 |
| `hyperlink` (with `location` / range `ref`) | 113 (38 / 1) | 15 (3 / 1) |
| sheet `autoFilter` / `sortState` | 3 / 7 | 3 / 5 |
| `selection` (multi-range) / `pane` (frozen) / `topLeftCell` | 481 (2) / 34 (32) / 81 | 272 / 20 / 53 |
| `cols` / `col` / `col max=16384` | 361 / 14,555 / 106 | 179 / 179 / 44 |
| `rowBreaks` `brk` / `colBreaks` `brk` | 10 / 3 | 4 / 3 |
| `ignoredErrors` / `protectedRanges` / `customSheetViews` | 5 / 4 / 3 | 3 / 4 / 1 |
| `dataConsolidate` / `scenarios` / `cellWatches` / `webPublishItems` | 1 / 0 / 0 / 0 | 1 / – / – / – |
| `oleObjects` / `controls` | 6 / 2 | 5 / 2 |
| `mc:AlternateContent` at sheet top level (all wrap `controls`) / in `sheetData` or rows | 2 / 0 | 2 / – |
| `xr:uid` attributes in sheets / in tables | 64 / 40 | 34 / 7 |

**Objects.**

| Place | Count | Files |
|---|---|---|
| notes (`comment/@ref`) = VML `x:Row`/`x:Column` pairs | 204 | 27 |
| VML `x:Anchor` (notes + controls + pictures) | 410 | 37 |
| VML form controls / `x:Fmla*` (all `x:FmlaMacro`; position-bearing `x:FmlaLink`/`Range`/`Txbx`/`Pict`/`Group`: 0) | 166 / 57 | 4 / 4 |
| threaded comments | 4 | 3 |
| `twoCellAnchor` (`editAs` twoCell / oneCell / absolute) | 181 (102 / 74 / 5) | 47 |
| `oneCellAnchor` / `absoluteAnchor` on worksheets | 30 / 1 | 8 / 1 |
| anchors wrapped in `mc:AlternateContent` | 170 | 4 |
| `xdr:sp/@textlink` | 0 | – |
| tables (autofilter / totals / calculated columns) | 61 (49 / 9 / 16 formulas) | 29 |
| tables `queryTable` / `xml` / with own `.rels` / no header | 13 / 5 / 13 / 2 | 2 / 4 / 2 / 2 |
| charts (on worksheets / with AC / pivot charts) | 40 (32 / 16 parts / 1) | 27 (21 / 13 / 1) |
| `c:f` sheet-prefixed / name-based / external | 168 / 6 / 4 | 25 / 2 / 1 |
| pivot caches (source `ref` / source by table name) | 10 (7 / 3) | 7 (5 / 3) |
| pivot tables (on worksheets) | 12 | 7 |
| `singleXmlCells` parts | 14 | 14 |
| query tables / external links / connections | 13 / 14 / 6 | 2 / 10 / 6 |
| `calcChain.xml` (entries) | 116 (30,875) | 116 |

**Sheets every G1 edit must refuse** (ADR-0018 rev 2: the registry's XX places and §2.4):
rows or cells without `r` 15 sheets (4 files), `oleObjects` 6 (5), `controls` 2 (2), top-level
`mc:AlternateContent` 2 (2; the same sheets as `controls`), `dataConsolidate` 1 (1);
`scenarios` and `webPublishItems` 0. **787 of 813 worksheets (97%) have none.** Pivot tables
(12 sheets, 7 files) refuse only edits that cross or move them, so they are counted
separately (`sheet_with_pivot_table`). With rev 2's sticky-end rule (§3.2), `col max="16384"`
no longer blocks a column insert.

**Protection** (ADR-0018 Q13): 11 protected sheets (5 files); 7 of them (4 files) lock each of
`insertRows`, `insertColumns`, `deleteRows`, `deleteColumns` (the attributes default to
locked). `workbookProtection/@lockStructure` in 4 workbooks. **`sheetPr/@codeName`** on 93
sheets (32 files), 33 of them (8 files) in packages with a VBA project. Workbook `oleSize`: 2.

**Worksheet extension URIs** (`ext/@uri` at any depth): `mac/excel/2008/main` 29 (19 files) and
`{64002731-A6B0-56B0-2670-7721B7C09600}` 20 (9), both holding only `mx:PLV`; x14 CF
`{78C0D931-…}` 2 (2); x14 DV `{CCE6A557-…}` 3 (3); sparklines `{05C60535-…}` 1 (1); the cfRule
`x14:id` link `{B025F937-…}` 2 (2). No other URI.

**Sheets a copy must refuse** (ADR-0018 §4.1; a relationship type outside the copyable set
`printerSettings`, `drawing`, `vmlDrawing`, `comments`, `threadedComment`, `table`,
`hyperlink`, `image`, `customProperty`, or a blocker): `tableSingleCells` 14 sheets (14 files),
`pivotTable` 12 (7), `oleObject` 5 (5), query tables with their own `.rels` 4 sheets (2),
`control`/`ctrlProp` 2 (2), `package` 2 (2), top-level `mc:AlternateContent` 2 (2). **773 of
813 worksheets (95%) are copyable** under those rules.

**Shared-group simulation.** `#REF!` areas compare equal under translation, and a deleted
master hands off to the first surviving member (ADR-0003 §4.2). Of 292 groups that span 3 or
more rows:
- one row inserted, or deleted, in the middle of the group: **286 stay one group, 6 (4 files)
  must be split**;
- one row inserted, or deleted, **at every position** from the lowest row the master refers
  to through one past the last member (`sim_shared_all_*`; a group counts as split if any
  position splits it): the **same 286 / 6**.

The 6 refer to a neighbouring row (`B5-B4`, `+A4+1`, `B1+B2`): after the edit the member below
the new row still refers to the row above the edit, which no single translation of the master
can express. 67 groups span fewer than 3 rows. Shared master texts with a sheet prefix: 0.

## Synthetic fixtures (`reference/fixtures/xlsx/synthetic`)

28 packages, hand-built for Phase1: they add the forms the corpus lacks: a data table
(`t="dataTable"` with `r1`), `cm` anchors (2), whole-row and whole-column references, 3-D
references (2), a 2-D shared block.

## Limits

- The lexer is an approximation (3 failures); counts are "texts containing the form".
- A reference written in an unusual form (`A1:A1048576`) is counted as a full-column area only
  when it spans the whole axis; none does in the corpus.
- The simulation models only references without a sheet prefix and edits on rows; columns are
  symmetrical in the model but not simulated. A cross-sheet shared group (none in the corpus)
  would split at the edit position like the 6 above.
- Structured references are counted by syntax; whether their table exists is checked against
  the package's table names (all 4,022 cell-formula ones name an existing table).
