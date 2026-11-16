# Excel (xlsx) roadmap: gap analysis, phases and Phase0 acceptance tests

- Status: Phase0 implemented (milestone `xlsx-phase0`); plan accepted by the owner 2026-11-16
- Date: 2026-11-16 (Phase0 implementation update: §1, §7, §8)
- Depends on: [ADR-0001](../adr/0001-lossless-round-trip-architecture.md) (Phase0 definition),
  [ADR-0002](../adr/0002-schema-driven-code-generation.md) (typed lens, allowlist)
- Proposes: [ADR-0003](../adr/0003-xlsx-phase1-formula-and-calc-state.md) (Phase1),
  [ADR-0006](../adr/0006-xlsx-phase2-shared-strings-and-styles.md) (Phase2)
- Fixtures: [`reference/fixtures/xlsx/`](../../reference/fixtures/xlsx/README.md);
  scripts: [`spikes/xlsx/`](../../spikes/xlsx/README.md)

## 要約（日本語）

- ADR-0001 の Phase0（既存ファイルの値の書き換えを、触っていない部分を壊さずに行う）を
  `crates/ooxml-xlsx` に実装した（milestone `xlsx-phase0`）。§7 の受け入れテストはすべて
  自動テストとして通る。Excel で修復ダイアログが出ないかの確認はオーナーが手で行う（§7.6）。
- Excel の機能面を約 60 項目に分け、それぞれについて関係する OOXML パートと要素、パートを
  またぐ整合性（ADR-0001 の Layer 3）、難しさ、参考ライブラリ（rust_xlsxwriter、calamine、
  umya-spreadsheet、openpyxl）の対応状況を表にした（§2、§3）。
- ooxml-rs は「触っていないものはバイト単位でそのまま残す」設計なので、openpyxl などが苦しんで
  いる「保存すると消える」問題は最初から起きない。代わりに優先すべきなのは、**ある編集が別の
  場所に保存されている派生データ（数式のキャッシュ値、テーブルの見出し名、calcChain など）を
  黙って古くしてしまう問題**である（§4 の「編集の波及」表）。
- Apache POI のテストデータ 367 ファイルを調べた結果、読めた 341 パッケージ（Transitional）の
  37%（126 件）が数式を含んでいた。Phase0 はこれらを一切編集できない。このため
  Phase1 は「数式と再計算状態の扱い」（ADR-0003）とし、共有文字列・スタイルの登録簿は Phase2
  とした（§6）。
- Phase2（共有文字列とスタイルの登録簿）の設計を ADR-0006（Proposed）にまとめた。既存の
  エントリは中身を解釈せずそのまま残し、新しく足すエントリどうしだけで重複をまとめる（新規作成
  のときはテンプレートの既存エントリとも重複をまとめる）。POI の 342 パッケージを調べると、値で
  比べると同じに見えるが XML が違う既存スタイルが 75 パッケージにあり、既存エントリとの照合は
  黙って別のスタイルに付け替える危険があることが分かった（`spikes/xlsx-phase2/`）。
- フィクスチャは POI（Apache-2.0）23 件と calamine（MIT）2 件の計 25 件、約 290 KB。ADR-0001 が
  「手作りフィクスチャでは確かめられていない」と書いた形（共有数式、配列数式、`mc:Ignorable`、
  `xml:space="preserve"`、データディスクリプタ付きの zip エントリ、`spans`、空の `sheetData`、
  結合セル、インラインのリッチテキスト、`r` の無いセル、calcChain の有無、シート順の入れ替え）を
  すべて含む。
- Phase0 の受け入れテストを、フィクスチャ・編集・判定方法（T1/T1b/T2、calamine での読み戻し）・
  期待されるエラーの組で定めた（§7）。オーナーの決定（2026-11-16）に合わせて一部を改めた
  （「最初のシート」は最初の表示されているワークシート、結合範囲は左上も含めて書き込み拒否、など）。

---

## 1. Where we are

| Area | State on `impl/xlsx-phase0` (from `main` @ d82d76d) |
|---|---|
| `ooxml-core` (Package, RawNode, lens) | Merged (foundation-v0); used unchanged |
| `ooxml-schema` / `xtask codegen` | Merged; `sml` allowlist grew by 3 read entries (table parts, for the header guard) |
| `ooxml-xlsx` | **Phase0 implemented**: `Workbook::open/new/save`, first visible worksheet, `cell`/`cell_mut` → `set_string`/`set_number`, refusals, template. See the crate README. |
| Excel features implemented | B1 (number write), B2 (read shared/inline, write inline), B3 (read flattened), B4 (`xml:space`, `_x` escapes), B7 (`dimension`), A1, A2/A3 (typed `Err`), A5 (from-scratch `core.xml`), plus the P0 guards of C1–C4, E1, F1, F10 |
| Fixtures | `reference/round_trip_corpus/fixture_minimal.xlsx` (hand-made) + 25 third-party files (unchanged) |

Phase0 (ADR-0001, with ADR-0002's allowlist) is the first target. Everything in §2 that is
not Phase0 is a gap.

## 2. Feature surface and gap table

### 2.1 How to read the table

- **Parts / elements**: the OOXML parts (Open XML SDK part names from `data/parts/*.json`) and
  the SpreadsheetML elements involved. Worksheet children are written in schema order, which
  matters for insertion (ADR-0002 §4.1). The 38 `CT_Worksheet` children are: `sheetPr dimension
  sheetViews sheetFormatPr cols sheetData sheetCalcPr sheetProtection protectedRanges scenarios
  autoFilter sortState dataConsolidate customSheetViews mergeCells phoneticPr
  conditionalFormatting dataValidations hyperlinks printOptions pageMargins pageSetup headerFooter
  rowBreaks colBreaks customProperties cellWatches ignoredErrors drawing legacyDrawing
  legacyDrawingHF drawingHF picture oleObjects controls webPublishItems tableParts extLst`.
- **L3 (cross-part)**: what must stay consistent across parts when this feature is *written*
  (ADR-0001 Layer 3 registries). `CT` = `[Content_Types].xml`, `rels` = a `.rels` part.
- **Difficulty**: S = local to one part, a few days; M = one part plus one registry or a
  non-trivial sub-schema; L = several parts or needs a parser (formula, VML); XL = needs an
  engine (calculation, layout) or touches every reference-bearing part.
- **Libraries**: RX = rust_xlsxwriter 0.102.0 (write-only), CA = calamine 0.38.0 (read-only),
  UM = umya-spreadsheet 3.1.1, OP = openpyxl 3.1.5. Values: **W** writes/creates, **R** reads,
  **RT** reads and writes back, **P** preserved without a model, **D** dropped on save,
  **–** no support, **(u)** unverified. Each claim was checked in the library's source or docs
  (paths in §3 and Sources); unverified ones are marked.
- **ooxml-rs**: nothing is implemented. The column gives the **planned phase** (§6). "L1" means
  the feature survives untouched by Layer 1/Layer 2 passthrough once Phase0 lands, without any
  code for that feature, **as long as no edit invalidates it** (see §4).

### 2.2 Package and workbook

| # | Feature | Parts / elements | L3 (cross-part) | Diff. | RX | CA | UM | OP | ooxml-rs |
|---|---|---|---|---|---|---|---|---|---|
| A1 | Open/save, part passthrough | all parts; zip entries | none for untouched parts | M | W | R | RT, unknown parts D | RT, unknown parts D | **P0** |
| A2 | Password-to-open (CFB/Agile encryption) | not a zip: CFB `EncryptionInfo` + `EncryptedPackage` | – | L | – | detects, `Err` | W only | – | P0: typed `Err(EncryptedPackage)`; decryption: Later |
| A3 | Strict OOXML (`purl.oclc.org/ooxml/…` namespaces) | every part | namespace mapping | M | – | R (u) | (u) | (u) | P0: `Err`; read: Later |
| A4 | `.xlsm`/`.xltx`/`.xltm` | `WorkbookPart` content type; `vbaProject.bin` | CT override of workbook | S | W | R | RT | RT (`keep_vba`) | L1 (P0), creation: P3 |
| A5 | Document properties | `docProps/core.xml`, `app.xml` (`TitlesOfParts`, `HeadingPairs`), `custom.xml` | `app.xml` sheet list ↔ `workbook.xml` sheets | S | W | R | RT | core/custom RT; **app.xml regenerated blank** | from-scratch `core.xml`: P0; edit: P3 |
| A6 | Sheets: add, delete, rename, reorder, hide; chart/macro/dialog sheets | `workbook.xml` `sheets/sheet`; `WorksheetPart`, `ChartsheetPart`, `MacroSheetPart`, `DialogsheetPart` | CT, workbook rels, `app.xml`, `definedName localSheetId`, formulas `Sheet!A1`, `calcChain i=` (sheet id) | L | W | R (type, visibility) | RT | RT | read any sheet: P1; structure edits: P3 |
| A7 | Defined names (incl. `_xlnm.Print_Area`, `_xlnm.Print_Titles`, `_xlnm._FilterDatabase`) | `workbook.xml` `definedNames/definedName` (`localSheetId`) | sheet index ↔ `localSheetId`; formulas using names | M | W | R, **scope lost** | RT | RT | read: P1 (dependency analysis); write: P3 |
| A8 | Workbook views, workbook protection, file sharing | `bookViews`, `workbookProtection`, `fileSharing` | `activeTab`/`firstSheet` indices ↔ sheet order | S | W (no workbook protection) | – | RT (SHA-512) | RT | P4 |
| A9 | 1900 vs 1904 date system | `workbookPr/@date1904` | all date serials | S | **– (by design)** | R | RT | RT | read: P2 (dates) |

### 2.3 Cells and values

| # | Feature | Parts / elements | L3 | Diff. | RX | CA | UM | OP | ooxml-rs |
|---|---|---|---|---|---|---|---|---|---|
| B1 | Numbers, booleans, errors | `c/@t` (`n`,`b`,`e`), `v` | – | S | W (errors only as formula results) | R | RT | RT | **P0** (number), bool/error write: P2 |
| B2 | Strings: shared (`t="s"`), inline (`t="inlineStr"`), formula string (`t="str"`) | `SharedStringTablePart` `sst/si`; `c/is` | **shared-string registry** (index stability) | M | W (shared; inline in `constant_memory`) | R | RT, inline → shared | RT, **always writes inline** | read both + write inline: **P0**; write shared: P2 |
| B3 | Rich text runs, phonetic runs | `si/r`, `is/r`, `rPr`, `rPh`, `phoneticPr` | font names in `rPr` vs theme | M | W | R, flattened | RT | RT (`rich_text=True`) | read flattened: P0; write: P2 |
| B4 | Whitespace and escapes | `t/@xml:space="preserve"`, `_xHHHH_` escapes (`_x000D_`) | – | S | W | R | RT | RT | **P0** (`xml:space`); `_x` decode: P0 read |
| B5 | Dates and times | number + date `numFmt` in `cellXfs`; `t="d"` (ISO 8601, Strict/rare) | **style registry** (needs a date `numFmt` xf) | M | W | R | RT | RT | read: P2; write: P2 |
| B6 | Cells/rows without `r` | `row` / `c` without `r` | – | S | n/a | R | (u) | (u) | P0: `Err(MissingReference)`; support: P6 |
| B7 | `dimension`, `row/@spans` | `dimension/@ref`, `row/@spans` (hint) | – | S | W | ignores (u) | RT | RT | **P0** (`dimension` recompute; `spans` left as is) |
| B8 | Large sheets (10^6–10^7 cells) | `sheetData` | – | XL | `constant_memory`/`low_memory` | streaming cell reader | `lazy_read`, streaming writer | `read_only`/`write_only` | P6 (lazy `sheetData`) |

### 2.4 Formulas and calculation

| # | Feature | Parts / elements | L3 | Diff. | RX | CA | UM | OP | ooxml-rs |
|---|---|---|---|---|---|---|---|---|---|
| C1 | Plain formulas | `c/f`, cached `c/v` (+ `t="str"/"b"/"e"`) | `calcChain`, `calcPr` | M | W (result default 0) | R (text + cached value) | RT | RT, **cached values lost on save** (u) | read: P0 (detect only); write: **P1** |
| C2 | Shared formulas | `f/@t="shared"`, `@si`, `@ref` (master holds text) | group integrity across cells | L | – (never written) | R, expanded | RT | read → expanded to plain on save | read/expand: P1; edit of master: P1 (promote next member, as POI) |
| C3 | Array (CSE) formulas | `f/@t="array"`, `@ref`; non-anchor cells carry only `v` | range integrity | M | W | text only | RT | RT | **P1** guard (no partial writes); write: P1 |
| C4 | Dynamic arrays / spills | `c/@cm`, `CellMetadataPart` (`metadata.xml`, `XLDAPR`), `_xlfn.`/`_xlws.` prefixes | `cm` index ↔ `metadata.xml` | L | W | `cm` ignored | **D** (`cm` not written) | **D** | guard: P1; write: P5 |
| C5 | Data tables (what-if) | `f/@t="dataTable"`, `r1/r2/dt2D/dtr` | range integrity | M | – | (u) | RT (u) | RT | guard: P1 |
| C6 | Cached results, recalc on load | `c/v`; `workbook.xml` `calcPr/@fullCalcOnLoad`, `@calcMode` | every dependent formula cell | L | W (`fullCalcOnLoad`) | R | `fullCalcOnLoad` not written | RT `calcPr` | **P1 (ADR-0003)** |
| C7 | Calculation chain | `CalculationChainPart` `calcChain/c` (`r`, `i` inherited from previous entry) | must list only formula cells; CT + workbook rels | M | – (never written) | – | **D** | **D** | **P1 (ADR-0003)** |
| C8 | Formula evaluation | – | – | XL | – | – | – | – | Non-goal (see §6, Later) |
| C9 | Formula tokenizer (references, names, functions) | `f` text, `definedName` text, CF/DV formulas | needed by C6, G1 | L | `_xlfn` auto-prefix | shared expansion only | (u) | `Tokenizer`, `Translator` | **P1** (references only) |

### 2.5 Formatting

| # | Feature | Parts / elements | L3 | Diff. | RX | CA | UM | OP | ooxml-rs |
|---|---|---|---|---|---|---|---|---|---|
| D1 | Cell formats (font, fill, border, alignment, protection) | `WorkbookStylesPart` `fonts fills borders cellXfs` (`xf`) | **style registry**: `c/@s`, `row/@s`, `col/@style` → `cellXfs` index | M | W (`Format`, dedup by value) | R (0.38) | RT, **re-indexed on save** | RT | keep `s` on edit: P0; new formats: **P2** |
| D2 | Number formats | `numFmts/numFmt` (ids ≥ 164 custom; 0–163 built-in) | numFmt id registry | S | W | R | RT | RT | P2 |
| D3 | Named cell styles | `cellStyleXfs`, `cellStyles`, `xf/@xfId` | xf → style xf | M | – (u) | (u) | RT | RT | P2 (read), P4 (write) |
| D4 | Theme / indexed colours, tints | `ThemePart`; `color/@theme @tint @indexed`; `indexedColors` | theme part | S | W (theme colours) | R | RT | RT | P2 (as opaque values) |
| D5 | Differential formats | `dxfs/dxf` | dxf index from CF, tables, `tableStyles` | S | W | (u) | RT | RT | P4 |
| D6 | Column widths, row heights, hidden, outline | `cols/col`, `row/@ht @hidden @outlineLevel`, `sheetFormatPr` | – | S | W | – | RT | RT | P4 |
| D7 | Style-part vendor extensions | `mc:AlternateContent` in `styles.xml` (Hancom), `x14ac`/`x15` ext | – | M | n/a | (u) | (u) | (u) | L1; registry must treat as opaque: P2 |

### 2.6 Range-based worksheet features

| # | Feature | Parts / elements | L3 | Diff. | RX | CA | UM | OP | ooxml-rs |
|---|---|---|---|---|---|---|---|---|---|
| E1 | Merged cells | `mergeCells/mergeCell/@ref` | – | S | W | R | RT | RT | P0: write `Err` inside a range; write: P4 |
| E2 | Conditional formatting (2007) | `conditionalFormatting/cfRule` (`dxfId`, `formula`) | dxf registry | M | W | – | RT | RT | P4 |
| E3 | Conditional formatting (x14: data bars, icon sets, …) | `extLst/ext/x14:conditionalFormattings` | `x14:id` ↔ 2007 `cfRule/extLst` id | L | W | – | **D** | **D (warns)** | L1; write: P4 |
| E4 | Data validation | `dataValidations/dataValidation`; x14 `x14:dataValidations` (`xm:f`, `xm:sqref`) | – | M | W (no x14) | – | RT incl. x14 | 2007 RT, **x14 D (warns)** | L1; write: P4 |
| E5 | Hyperlinks | `hyperlinks/hyperlink` (`r:id` external, `location` internal) | **sheet rels** (external targets) | S | W | R | RT | RT | P4 |
| E6 | Autofilter, sort state | `autoFilter`, `sortState`; `_xlnm._FilterDatabase` name | defined name | M | W (no sortState) | – | autofilter RT, sortState **D** | RT | P4 |
| E7 | Sheet views: freeze/split panes, zoom, selection, gridlines, RTL, tab colour | `sheetViews/sheetView/pane/selection`, `sheetPr/tabColor` | `tabSelected` ↔ `activeTab` | S | W (no split panes) | – | RT | RT | P4 |
| E8 | Page setup, margins, print options, breaks | `pageSetup` (+ `PrinterSettingsPart` via `r:id`), `pageMargins`, `printOptions`, `rowBreaks`, `colBreaks` | sheet rels | S | W | – | RT | RT | P4 |
| E9 | Header/footer (text, images) | `headerFooter`; images via `legacyDrawingHF` → `VmlDrawingPart` + image | VML + rels | M | W (incl. images) | – | text RT, **images D** | text RT, images – (u) | text: P4; images: P5 |
| E10 | Sheet / workbook protection | `sheetProtection` (`password` legacy hash or `algorithmName hashValue saltValue spinCount`), `protectedRanges` | – | S | W (legacy hash only) | – | RT (SHA-512) | RT (SHA-512 not computed) | P4 |
| E11 | Rare sheet elements | `scenarios`, `dataConsolidate`, `customSheetViews`, `cellWatches`, `ignoredErrors`, `phoneticPr`, `webPublishItems`, `smartTags` | – | S | partial | – | partial | partial | L1 only |

### 2.7 Objects and structures

| # | Feature | Parts / elements | L3 | Diff. | RX | CA | UM | OP | ooxml-rs |
|---|---|---|---|---|---|---|---|---|---|
| F1 | Tables (ListObjects) | `TableDefinitionPart` (`table`, `tableColumns`, `autoFilter`); `tableParts/tablePart` | **table id and name unique per workbook**; header cells ↔ `tableColumn/@name`; CT, sheet rels | L | W | R | RT | RT | edit guard: **P1**; create: P5 |
| F2 | Pivot tables and caches | `PivotTablePart`, `PivotTableCacheDefinitionPart`, `PivotTableCacheRecordsPart`; `workbook.xml` `pivotCaches` | `cacheId` ↔ workbook rels; source range | XL | – | R (names, cache records) | RT (u for creation) | P (editable, not creatable) | L1; create: Later |
| F3 | Charts, chartsheets, chartEx | `DrawingsPart` → `ChartPart` (+ `ChartStylePart`, `ChartColorStylePart`), `ExtendedChartPart`; `ChartsheetPart` | drawing rels; `c:numCache`/`c:strCache` copies of cell values | XL | W | type of sheet only | RT | W; read best effort (u) | L1; create: P5 |
| F4 | Images, shapes, text boxes | `DrawingsPart` (`xdr:twoCellAnchor`, `xdr:pic`, `xdr:sp`), `ImagePart` | CT default per extension, drawing rels, `cNvPr/@id` unique per drawing | L | W | R (images, feature) | RT | images W/R; **shapes D** | L1; images: P5 |
| F5 | Notes (legacy comments) | `WorksheetCommentsPart` (`comments`, `authors`), `VmlDrawingPart` shapes; `legacyDrawing` | comment `ref` ↔ VML `x:Row/x:Column`; `o:spid` | L | W | – | RT | RT (formatting lost) | L1; write: P5 |
| F6 | Threaded comments | `WorksheetThreadedCommentsPart`, `WorkbookPersonPart`; fallback note in `comments` | thread id ↔ note `ref`; person ids | L | – | – | RT | **D** | L1; write: P5 |
| F7 | Sparklines | `extLst/ext/x14:sparklineGroups` | – | M | W | – | **D** | **D (warns)** | L1; write: P5 |
| F8 | Slicers, timelines | `SlicersPart`, `SlicerCachePart`, `TimeLinePart`, `TimeLineCachePart`; workbook/sheet `extLst` | slicer cache ↔ pivot/table; defined names | XL | – | – | **D** | **D (warns)** | L1; create: Later |
| F9 | External links | `ExternalWorkbookPart` (`externalLink`), `workbook.xml` `externalReferences`; `[n]` in formulas | index `n` ↔ `externalReference` order | M | – | – | **D** | RT (`keep_links`) | L1; formula guard: P1 |
| F10 | Rich values: images in cell, data types | `RdRichValuePart` & friends (`xl/richData/*`), `CellMetadataPart`; `c/@vm` | `vm` index ↔ `valueMetadata` ↔ rich value | L | W (`embed_image`) | R (images) | **D** | **D** | L1; edit guard: **P0** (Q4); write: Later |
| F11 | VBA | `VbaProjectPart` (`vbaProject.bin`), `VbaDataPart`; `.xlsm` content type | CT, workbook rels, `codeName` | M | W (add blob) | R | RT (blob) | P (`keep_vba`) | L1; add blob: P3 |
| F12 | Form controls, ActiveX, OLE | `ControlPropertiesPart`, `EmbeddedControlPersistencePart`, `EmbeddedObjectPart`; `controls`, `oleObjects` wrapped in `mc:AlternateContent` | VML shape ids, rels | L | button/checkbox only | – | OLE RT; controls **D** | partial (u) | L1 only |
| F13 | Custom XML, query tables, connections, named sheet views | `CustomXmlPart`, `QueryTablePart`, `ConnectionsPart`, `NamedSheetViewsPart` | rels | M | – | – | (u) | (u) | L1 only |

### 2.8 Structural edits

| # | Feature | Parts / elements | L3 | Diff. | RX | CA | UM | OP | ooxml-rs |
|---|---|---|---|---|---|---|---|---|---|
| G1 | Insert/delete rows/columns | every reference: `c/@r`, `row/@r`, `dimension`, formulas (all sheets, names, CF, DV, charts, pivots, tables, merges, hyperlinks, comments, VML anchors, drawings anchors, print areas, `calcChain`) | **all of them** | XL | n/a | n/a | yes, scope (u) | yes; formulas **not** adjusted (documented) | P7 |
| G2 | Copy / move sheet | all sheet-owned parts; formulas referring to the sheet | ids, names, rels, CT | L | n/a | n/a | (u) | `copy_worksheet` (cells/styles only) | P7 |

## 3. How the reference libraries rank features

The four libraries were checked in source (versions above; details and per-file evidence in
Sources). What each one put first tells us what users ask for, and where each design breaks.

- **rust_xlsxwriter (write-only)** is the breadth benchmark for *creating* files: formats with
  value-based dedup, all CF types including x14, data validation, tables, charts, sparklines,
  images (including images in cells via rich values), notes, VBA blobs, header/footer images,
  `constant_memory` streaming. It never writes shared formulas or `calcChain.xml`, and always sets
  `fullCalcOnLoad`: its stance is "let Excel compute". Not supported: 1904 dates (deliberately),
  workbook-structure protection, threaded comments, split panes, sort state.
- **calamine (read-only)** puts *values* first: cell values with cached formula results,
  formula text with shared formulas expanded, merged regions, tables, defined names (scope
  dropped), and since 0.38 styles. It ignores `cm`, array `ref`s, CF and DV. It detects encrypted
  files and returns `XlsxError::Password`. It is the independent read-back oracle in ADR-0001.
- **umya-spreadsheet (model round trip)** models the most (threaded comments with persons, x14
  data validation, OLE, SHA-512 protection), but is model-based: anything without a struct is
  dropped (x14 CF, sparklines, slicers, external links, rich values, `calcChain`, `cm`), and the
  style table is re-indexed on save. With `lazy_read`, untouched sheets are written back raw; whether
  that stays consistent after style re-indexing is unverified.
- **openpyxl (model round trip)** chooses *preservation* for a few high-complaint parts (pivot
  tables "so that they will be preserved", VBA with `keep_vba`, external links) and drops the
  rest with warnings (every worksheet `extLst`: x14 CF, x14 DV, sparklines, slicers; shapes;
  threaded comments; `calcChain`). It writes every string inline and regenerates `app.xml`.

What this means for ordering ooxml-rs's work:

1. **Preservation is not where ooxml-rs needs to spend effort.** Every "D" in §2 is a part or
   element that ooxml-rs keeps by construction (Layer 1 copies the zip entry; Layer 2 re-emits
   untouched spans verbatim). The remaining preservation risk is narrower: an edit that makes a
   preserved copy *wrong* (§4).
2. **The write-feature bar is rust_xlsxwriter.** Its priorities (formats → formulas → CF/DV →
   tables → charts → images) match what end users ask for. ooxml-rs reaches it in Phases 2, 4
   and 5.
3. **The read bar is calamine.** Values, cached formula results, shared-formula expansion, and
   merged regions come first. ooxml-rs's typed lens reaches this in Phases 0–1.
4. **Nobody else edits a workbook in place without dropping something.** Making that safe for
   real workbooks (most of which contain formulas) is ooxml-rs's reason to exist, so it goes
   first (Phase1).

## 4. Edit hazards: what an edit can silently invalidate

Layer 1/2 guarantee that untouched bytes stay untouched. They cannot know that an untouched part
contains a **copy of, or a reference to,** something an edit changed. This table lists, for the
two edit classes ooxml-rs will support first, every place in a workbook that holds derived data.

| Derived data | Where | Invalidated by a value edit to cell X? | Failure if ignored | Handled in |
|---|---|---|---|---|
| Cached formula results | `c/v` of every formula cell that (transitively) reads X, on any sheet | **Yes** | Wrong values in calamine/pandas/openpyxl `data_only`; Excel recalculates only if told to | P0: refuse all edits if any formula exists. **P1**: invalidate dependents + `fullCalcOnLoad` |
| Array / spill / data-table outputs | `c/v` of non-anchor cells inside an `f/@ref` range (no `f` of their own) | **Yes**, if the anchor formula reads X | Same, and a write *into* such a cell breaks the array | P1 |
| Calculation chain | `calcChain.xml` | Only if the *set of formula cells* changes (formula added/removed) | Excel "unreadable content" repair (POI removes chain entries on formula deletion: `XSSFWorkbook.onDeleteFormula`) | P0: no formula writes. **P1**: policy in ADR-0003 |
| Table header names | `tableColumn/@name` ↔ header row cell text | **Yes**, if X is in a table's header row | Excel "Found unreadable content" (POI `XSSFTable.updateHeaders` javadoc) | **P1** guard (Err); sync: P5 |
| Table structured references | formulas `Table1[Col]` | If a header name changes | Broken formulas | P5 |
| Pivot cache records | `pivotCacheRecords*.xml` | Yes, if X is in a pivot source range | Stale pivot until refresh. **Same as Excel's own behaviour** (pivots refresh on demand) | Not a defect; document |
| Chart caches | `c:numCache` / `c:strCache` in `chart*.xml` | Yes, if X is in a series range | Renderers that trust caches show old data. Excel re-reads linked ranges on open (believed, **unverified**) | Document in P1; refresh: P5 |
| Rich value binding | `c/@vm` → `metadata.xml` → `richData` | Yes: overwriting a `vm` cell leaves an orphan or mismatched rich value | Image/data type shown in place of the new value, or repair (unverified) | **P0** guard (Q4) |
| Dynamic-array metadata | `c/@cm` → `metadata.xml` | Yes, for the anchor | Spill misinterpreted | P1 guard |
| Merged-cell content | non-anchor cells of a `mergeCell` | Writing them is invisible in Excel | Hidden data | P0 guard (Q3) |
| `dimension` | `dimension/@ref` | If X is outside it | Some readers truncate the used range | P0 recompute |
| `spans` hints | `row/@spans` | If X is outside the row's span | None known; it is an optimisation hint (ECMA-376 Part 1 §18.3.1.73) | Leave as is (P0 test checks Excel opens it: manual) |
| Shared-formula master text | `f` of the master cell | Writing the master breaks every member | Members lose their formula | P1 (promote next member, as POI `XSSFSheet.onDeleteFormula`) |

A **structural edit** (insert/delete rows or columns, rename or delete a sheet) invalidates every
reference-bearing element in every part (G1). That is why it is last (P7) and why it depends on
P1's formula tokenizer.

## 5. What the fixture survey shows

`spikes/xlsx/survey.py` and `spikes/xlsx/formula_stats.py` were run on all 367 `.xlsx`/`.xlsm`
files in Apache POI's `test-data/spreadsheet` at commit `47dcff76` (2026-11-16) and on calamine's
82 test files at `93e2e6b3`. These are test corpora (many files are minimal bug reproductions), so
the percentages describe the corpus, not the world. Of the 367 POI files, 346 opened as zip packages
(21 are fuzzer crash files, CFB-encrypted, or deliberately corrupt).

| Shape | POI packages | Consequence for ooxml-rs |
|---|---:|---|
| Excel-authored (`app.xml` Application contains "Excel") | 299 / 346 | Real Excel output dominates; LibreOffice 15, POI 6, Hancom 1 |
| Worksheet root has `mc:Ignorable` | 205 | Every Excel ≥ 2010 sheet. Ignorable-prefixed attributes (`x14ac:dyDescent` on `row`) must survive dirty re-rendering of a `row` (T1b on 61605/FillWithoutColor) |
| `row/@spans` | 279 | Edits outside a span leave a stale hint (see §4) |
| Some sheet with zero rows | 137 | `<sheetData/>` gaining its first row is common (ADR-0001 R2-B2), not only in the template |
| Zip entries with a data descriptor | 38 | LibreOffice and POI write them; Layer 1 raw copy must handle them |
| Contain formulas (SML `f`) | **126 / 341** (37%) | Phase0 refuses every edit to these |
| … of which without `calcChain.xml` | 11 | ADR-0001 R3-M1 confirmed: absence of `calcChain` does not mean formula-free (LibreOffice, POI-written) |
| … shared / array / dynamic | 48 / 6 / 1 | Shared formulas are the common case, not an edge case |
| … using defined names / structured refs / `INDIRECT`/`OFFSET` / external `[n]` / other sheets | 13 / 10 / 2 / 6 / 23 | Input for ADR-0003's conservative dependency analysis |
| Merged cells | 43 | |
| `c` or `row` without `r` | 4 / 3 | Rare; non-Excel producers |
| Inline strings / with rich runs | 12 / 3 | |
| `mc:AlternateContent` as a direct child of `worksheet` | 1 (`62629_target.xlsm`, sheet2: `legacyDrawing`/`controls`) | ADR-0002 §4.3 insert refusal is reachable in real files |
| Strict | 4 | |
| Hidden sheet / first sheet not a worksheet | 8 / 1 | "First sheet" may be hidden or a macro sheet (Q2) |

Two findings change how Phase0 must be implemented:

1. **The formula scan must match the QName, not the bytes `<f`.** ADR-0001 says "check for the
   presence of any `<f` start element". A byte search for `<f` also matches `<formula>` (CF
   rules), `<from>` (drawing anchors in `oleObjects`), `<filterColumn>`, `<fill>`.
   `FillWithoutColor.xlsx` (0 formulas, 4 `<formula>`) and `62629_target.xlsm` (0 formulas, 1
   `<from>`) would be wrongly refused. Formulas inside `extLst` (`xm:f` in x14 data validation,
   `51850.xlsx`) are not cell formulas and carry no cached value. The check is: an element with
   namespace SpreadsheetML main and local name `f`, anywhere in a worksheet or macro-sheet part.
2. **Excel-authored files contain things Phase0 must refuse rather than overwrite**: `vm` cells
   (images in cells, `pictures_in_cell_and_over_cell.xlsx`) and table header cells
   (`ExcelPivotTableSample.xlsx`, table `E1:G4` on the first sheet). Overwriting either produces
   a file that is wrong or that Excel repairs. These need guards (Q4, Q5) in addition to ADR-0001's
   list.

## 6. Phases

Ordering rule: **user value × risk of silent damage**, subject to dependencies. Value = how many
real workflows a phase unblocks; risk = how bad the result is if the phase is done naively or
not at all (silent wrong data ≫ Excel repair dialog ≫ missing feature).

| Candidate next step | Value | Risk if wrong/naive | Depends on | Decision |
|---|---|---|---|---|
| Formula-aware value edits + calc state | High: unblocks ≥ 37% of corpus workbooks for editing, and is ooxml-rs's differentiator | **Silent wrong values** (stale caches), Excel repair (`calcChain`) | P0 | **Phase1** (ADR-0003) |
| Shared-string + style registries | High: dates, number formats, any new formatting; from-scratch parity | Duplicate or merged styles (visible), index corruption (prevented by append-only rule) | P0 | Phase2 |
| Package structure edits (sheets, names) | Medium | Excel repair (CT/rels mismatch) | P0, part of P2 for `app.xml` | Phase3 |
| Range-based sheet features | Medium–high (breadth) | Mostly visible (repair, wrong formatting) | P2 (dxf) | Phase4 |
| Drawing-layer objects | Medium | Repair; ids | P3 (new parts) | Phase5 |
| Large sheets | Medium–high for data workloads | Performance only | P0 | Phase6 (can run in parallel after P1) |
| Structural edits | High but rare per call | Everything referencing anything | P1 tokenizer, P3, P4 | Phase7 |

Why formulas before registries: Phase0 already writes strings (inline) and numbers into
existing formatted cells, so registries add *capability*; formula handling removes a *refusal*
that covers most real workbooks, and its failure mode is the worst one in §4 (values that are
wrong with no error). ADR-0001 also left this decision explicitly open ("a future phase must
pick, and write down, one of …"). The registry design is lower-risk: rust_xlsxwriter's dedup and
ADR-0001's "two seed modes" note already pin it down, and it can start right after Phase1 or in
parallel.

Each phase below lists its goal, scope, acceptance criteria and test oracles. Oracles:
**L1** = Layer 1 entry equality (name, method, CRC32, compressed bytes) for every untouched part;
**T1/T1b/T2** = ADR-0001's oracles; **CR** = calamine read-back of the output equals calamine's
reading of the input except at edited cells, which read the new value; **CK** = `cargo xtask
codegen --check`; **XL** = manual "opens in Excel without a repair dialog" check by the project
owner (ADR-0001 Consequences: no Excel in the sandbox).

### Phase0 — lossless value edits (ADR-0001 + ADR-0002)

- **Goal:** open a real `.xlsx`, overwrite values of existing cells or create cells on the first
  sheet, save, and lose nothing; from-scratch skeleton via an embedded template.
- **Scope:** as ADR-0001 "Scope boundary" and ADR-0002 §5, plus §5's two findings (QName-based
  formula scan — now an ADR-0001 post-acceptance note; guards for `vm` cells and table header
  cells — coordinator defaults, §8).
- **Acceptance:** §7 below, every row green; clippy clean; CK clean.
- **Exit gate:** XL check by the owner on the outputs listed in §7.4.

### Phase1 — formula-aware edits and calculation state (ADR-0003, Proposed)

- **Goal:** value edits are allowed in workbooks that contain formulas, without ever leaving a
  cached value that may be wrong; formulas can be written and removed.
- **Scope:**
  1. Read and edit any worksheet by name or index (ADR-0002 §6 lookup generalised).
  2. Formula census at open by QName (§5 finding 1), plus array/spill/data-table output ranges.
  3. A reference-only formula tokenizer (A1 refs, ranges, whole rows/cols, sheet-qualified and
     3-D refs, names, structured refs, external `[n]`, function names); unknown → conservative.
  4. Dependency-based cache invalidation: remove `v` (and the result `t`) from every formula
     cell and array output cell that may transitively depend on an edited cell; set
     `calcPr/@fullCalcOnLoad="1"`.
  5. `set_formula` (plain, no cached value), formula → value overwrite, shared-formula master
     promotion (as POI), guards for partial array/spill/data-table writes.
  6. `calcChain.xml`: untouched when the set of formula cells is unchanged; removed (part,
     workbook relationship, content-type override) when it changes.
  7. Table header guard (writes into a table's header row → `Err` until P5 syncs names).
- **Acceptance:**
  - On every fixture with formulas (`shared_formulas`, `MatrixFormulaEvalTestData`, `xlookup`,
    `70005-countifs`, `NewStyleConditionalFormattings`): a value edit to a non-formula cell
    succeeds; T2 holds with the allowed diff extended to (a) the dependents' `v`/`t`, (b)
    `calcPr`; every formula cell that is *not* a dependent keeps its span byte-identical.
  - CR: dependents read as empty (no stale value); non-dependents read their original cached
    value; `worksheet_formula` text is unchanged for every formula cell, and calamine's shared
    formula expansion agrees before/after.
  - Tokenizer: on all 126 formula-bearing POI packages (fetched, not committed), every `f`
    text is tokenized without panic; results are "refs" or "conservative"; a differential test
    against openpyxl's `Tokenizer` (dev-only, offline-generated expected tokens) agrees on
    reference tokens.
  - `calcChain` removal: L1 for all other parts; `[Content_Types].xml` and workbook rels lose
    exactly one entry each (T2-style diff confined to those elements).
  - XL on: edited `shared_formulas.xlsx`, `xlookup.xlsx`, a workbook after formula removal.
- **Oracles:** L1, T1/T1b/T2 (extended allowed-diff set), CR, XL.

### Phase2 — shared-string and style registries ("two seed modes"; ADR-0006, Proposed)

- **Goal:** rust_xlsxwriter-level cell formatting for from-scratch files and for new cells in
  existing files; dates.
- **Design:** [ADR-0006](../adr/0006-xlsx-phase2-shared-strings-and-styles.md) (revision 2).
  Measurements: [`spikes/xlsx-phase2/`](../../spikes/xlsx-phase2/README.md) (342 POI packages +
  24 fixtures). A throwaway prototype on branch `spike/xlsx-phase2-proto` ran the edit-mode
  registry on all of them (330 POI packages pass every check, 0 failures).
- **Scope:**
  - append-only shared-string registry: `t="s"` when the workbook has an sst, inline fallback
    otherwise (28% of the corpus has none); rich strings; existing `si` opaque, except
    byte-exact reuse (proposed default for Q1);
  - style registry over `numFmts`, `fonts`, `fills`, `borders` and `cellXfs` (and the `dxfs`
    shape for Phase4), with a value-typed `Format` (raw theme/tint/indexed colours);
  - from-scratch mode dedups against the template's entries. Edit mode dedups among new entries,
    and against existing ones only by exact form: the whole bottom-up chain must be
    byte-identical to what would be appended, so repeated runs stop growing the tables
    (revision 2, review M2). Matching by value is rejected: 75/340 packages have distinct
    existing `xf`s that a `Format` projection would merge;
  - unset `Format` groups inherit the workbook's default style by index, not Calibri 11 (40% of
    files use another default font; review M1); Excel's table limits are guarded;
  - number formats: built-in ids only when the file does not redefine them (32 packages
    redefine ids < 164); new custom ids above every `numFmtId` used anywhere in the package
    (dxf, pivot, extLst; review M3); `numFmts` inserted when absent (234/340);
  - `sst/@count` recomputed from Phase1's census, `uniqueCount` set (7% of files have a wrong
    `count`);
  - applying an existing style by index or by unique named style; `set_datetime` (1900/1904,
    false 1900-02-29), `set_bool`, `set_error`; reading `format()` / `is_date()`;
  - vendor extensions (`x14ac:knownFonts`, `extLst`, `indexedColors`) untouched; a table with an
    `mc:AlternateContent` child cannot grow (`Err(UnresolvedSibling)`).
- **Acceptance (ADR-0006 "Test oracles"):** T2-append on `styles.xml`/`sharedStrings.xml`
  (existing entries byte-identical, diff confined to appended entries and `count`s); index
  stability for every `s` and `t="s"`; CR of values *and* styles with calamine 0.38
  (`worksheet_style`, dates as `DateTime`); from-scratch format matrix compared with
  rust_xlsxwriter 0.102 output (calamine + normalised `styles.xml`); growth bound on
  `61605`, `style-alternate-content`, `52348`, `ExcelPivotTableSample`, `fixture_minimal`;
  collision regressions from the spike; mutation testing.
- **Oracles:** L1, T2-append, CR (values + styles), RX (rust_xlsxwriter comparison), XL.
- **Depends on Phase1** for plan → validate → apply with staged clones and for the census
  (`t="s"` counting); value writes with a format invalidate dependents like any value write,
  style-only writes invalidate formulas that call `CELL`/`INFO`.

### Phase3 — package structure edits

- **Goal:** add, delete, rename, reorder, hide sheets in existing packages; defined names CRUD;
  VBA blob insertion; `app.xml`/`core.xml` maintenance.
- **Scope:** OPC registries in `ooxml-core` (CT defaults/overrides, rels id allocation, part
  name allocation that does not assume `sheetN.xml` numbering — `reordered_sheets.xlsx`,
  `renamed_workbook_part.xlsx`); `sheetId` allocation (never reused); `localSheetId` renumbering
  on reorder; formula sheet-name rewriting on rename (uses P1 tokenizer); `app.xml`
  `TitlesOfParts`/`HeadingPairs`.
- **Acceptance:** L1 for unrelated parts; OPC conformance checks (every rel target exists, every
  part has a content type, no orphan parts) on all fixture outputs; CR sheet names/order; XL.

### Phase4 — range-based worksheet features (write and edit)

- **Scope:** merged cells (write; anchor rule), hyperlinks (with sheet rels), data validation
  (incl. x14 via typed `extLst` lens), conditional formatting (2007 + x14 with id linking), dxf
  registry, autofilter and sort state, panes/views, column widths/row heights/outline, page
  setup/margins/print options/breaks, text header/footer, sheet and workbook protection
  (SHA-512 per ECMA-376 Part 1 §18.2.29 attributes and the legacy hash).
- **Acceptance:** for each feature, a from-scratch file and an edit of an Excel-authored fixture;
  T2 confined to the feature's element; CR where calamine exposes it (merges, hyperlinks); XL.
  Feature list matches rust_xlsxwriter's for these items (checklist in the phase ADR).

### Phase5 — drawing-layer objects and table/chart creation

- **Scope:** images (anchors, `cNvPr/@id` allocation), notes (comments + VML), threaded comments
  (+ persons + fallback note), tables (create; header sync replacing P1's guard), sparklines,
  charts (DrawingML chart allowlist growth, `ChartPart` creation, cache refresh), header/footer
  images, dynamic-array formulas (`cm` + `metadata.xml`).
- **Acceptance:** L1 for unrelated parts; T2 for sheet part changes; created parts validated
  by an independent reader (`ooxmlsdk` parse as oracle, per ADR-0002 Alternatives 1); XL.

### Phase6 — large sheets

- **Goal:** within 2× of rust_xlsxwriter (`constant_memory`) write time and calamine read time
  for a 1M-cell sheet; memory bounded by rows touched, not cells in the sheet.
- **Scope:** lazy row-level `sheetData` (ADR-0001 "future redesign point"), streaming
  from-scratch writer; possibly ADR-0001 Alternative 1b for the touched-part path.
- **Acceptance:** benchmark harness (criterion) with published numbers; all P0–P5 tests still
  green on the lazy path; T2 unchanged.

### Phase7 — structural edits

- **Scope:** insert/delete rows/columns and copy/move sheets, with reference rewriting in every
  part listed under G1, built on the P1 tokenizer and a registry of reference-bearing elements.
- **Acceptance:** per-part reference rewrite tests; round trip through Excel (XL) on a fixture
  with formulas, CF, DV, merges, tables and charts; CR values.

### Later / non-goals for now

Formula evaluation engine (C8); pivot table, slicer and timeline creation (F2, F8); decryption
and encryption (A2); Strict read/write (A3); ActiveX/OLE authoring (F12); rich data authoring
(F10). All of these remain preserved by L1 when untouched.

## 7. Phase0 acceptance-test specification

Everything here is runnable as soon as `ooxml-core` (Package, RawNode, lens) and the generated
`ooxml-schema` `sml` views land. Names of error variants are **proposals** where ADR-0001/0002 do
not name them; the variant *names* may change, the *classes* may not.

### 7.1 Inputs and conventions

- Fixtures: `reference/fixtures/xlsx/{poi,calamine}/*.xlsx|xlsm` and
  `reference/round_trip_corpus/fixture_minimal.xlsx`. Facts quoted below come from
  `spikes/xlsx/phase0_facts.py` (re-run it if a fixture changes).
- "First sheet" = the **first visible worksheet** in workbook order (owner decision 2026-11-16,
  ADR-0001 post-acceptance note; this replaces ADR-0002 §6's "first `<sheet>`"), resolved via the
  `officeDocument` relationship and the workbook's relationships, *not* by part name.
- Every test that writes runs against a temporary copy, and also once with
  `save` to the **same path** it was opened from (ADR-0001 R2-M4).
- A test "passes CR" when calamine (dev-dependency) opens the output, and for the first sheet
  `worksheet_range` equals the input's range except at edited cells; for other sheets, ranges
  are equal; `worksheet_formula` is unchanged.

### 7.2 Package-level tests (no edits)

| ID | Fixtures | Action | Oracle / expectation |
|---|---|---|---|
| P0-PKG-1 | all 24 zip fixtures (all but `protected_passtika`) + `fixture_minimal`; `SimpleStrict` is refused by `Package::open` itself (P0-PKG-3), so 23 + 1 | `Package::open` → `save` with zero edits, and `Workbook::open` → `save` | L1 for **every** entry; same part order. Includes the data-descriptor fixtures `52348` (9 entries), `62272` (9), `70005-countifs` (11) |
| P0-PKG-2 | `protected_passtika.xlsx` | `open` | `Err(EncryptedPackage)` (CFB magic `D0 CF 11 E0`), not a zip error |
| P0-PKG-3 | `SimpleStrict.xlsx` | `Workbook::open` | `Err(Core(StrictNotSupported))` (ADR-0001 out-of-scope list); in practice `Package::open` already refuses it (Strict package relationships) |
| P0-PKG-4 | `renamed_workbook_part.xlsx` | locate first sheet | workbook = `xl/workbook2.xml`, sheet = `xl/sheet1.xml` (no hard-coded paths) |
| P0-PKG-5 | `reordered_sheets.xlsx` | locate first sheet | first sheet = `Sheet4` → `xl/worksheets/sheet1.xml`; rels are listed out of id order. (Lookup by name is a Phase1 API; not tested in Phase0.) |
| P0-PKG-6 | `xlmmacro.xlsm` | locate first sheet | the first `<sheet>` is a macro sheet and is skipped: `Sheet1` → `xl/worksheets/sheet1.xml` (Q2, decided) |
| P0-PKG-7 | `TwoSheetsOneHidden.xlsx` | locate first sheet | `Sheet1` is `state="hidden"` and is skipped: `Sheet2` → `xl/worksheets/sheet2.xml` (Q2, decided). With every sheet hidden: `Err(NoVisibleWorksheet)` (synthetic) |

### 7.3 Parser/renderer tests (T1, T1b)

| ID | Parts | Fixtures | Expectation |
|---|---|---|---|
| P0-T1 | first worksheet, `workbook.xml`, `sharedStrings.xml` (if present), `styles.xml` | all openable fixtures | parse → serialize with zero edits is byte-identical |
| P0-T1b | same | same | force all dirty → render → re-parse: structurally equal. Must cover: `x:`-prefixed SML with indentation text nodes and an astral-plane character reference `&#x1f61c;` (`InlineString`); Cyrillic sheet names (`61605`); Hancom `mc:AlternateContent` with Korean font names in `styles.xml` (`style-alternate-content`); `mc:Ignorable` + `x14ac:dyDescent` attributes on `row` (`61605`, `FillWithoutColor`); `xr:uid` attributes (`62629_target`) |

### 7.4 Edit tests (T2 + CR)

All value writes use `set_number(…)` and `set_string(…)` (inline string) as in ADR-0001. "Sweep"
means ADR-0001 T2's per-cell loop: write the same value back, then a different value, for every
cell (existing and listed new positions), asserting the byte diff is confined to the cell's
span or insertion point plus `dimension`.

| ID | Fixture | Edit | Expectation |
|---|---|---|---|
| P0-E1 | `fixture_minimal.xlsx` | ADR-0001's sweep (A1, A2, B2, C2 bool→number, D2 self-closing) | T2; CR; foreign `extLst` intact (T2 locality) |
| P0-E2 | `61605.xlsx` (Excel 2013; 45 self-closing styled cells, 6 merges, `x14ac`) | sweep over all 46 cells; new cell `A20` | every cell of a merged range, anchors (`B1`, `B3`, …) included → `Err(MergedCell)` (Q3, coordinator default: ADR-0001's wording); `s` kept on every write; self-closing `<c r="B2" s="1"/>` becomes open/close (own_dirty); `A20` creates `row r="20"` after `row r="15"`; `dimension` `B1:E15` → `A1:E20`; hyperlink on `B5` and `printerSettings1.bin` intact (L1); CR |
| P0-E3 | `FillWithoutColor.xlsx` (Excel 16; 513 self-closing cells, 24 merges, self-closing `<row r="2" …/>`, 4 CF `<formula>`) | write `B2` (into the empty self-closing row); sweep a 20-cell sample across rows 3–61 | not refused by the formula scan (§5 finding 1); `row r="2"` gains its first child → re-rendered with `spans`, `ht`, `thickBot`, `x14ac:dyDescent` preserved; second sheet L1-identical; CR |
| P0-E4 | `reordered_sheets.xlsx` (first sheet has `<sheetData/>`) | write `A1` = 1, `C3` = "x" | `<sheetData/>` → open/close, `row 1`, `row 3` in order (ADR-0001 R2-B2, R3-m2); `dimension` `A1` → `A1:C3`; sheets 2–4 L1; CR |
| P0-E5 | `62272.xlsx` (LibreOffice 7.3; data descriptors; attribute-heavy `row`) | sweep `A1`; new `B2` | Ok; output entries for untouched parts L1 even though the input used data descriptors (container fields not compared, per ADR-0001 M1); CR |
| P0-E6 | `renamed_workbook_part.xlsx` (no `dimension`, no `styles` refs) | write `C1` = 3.5 | Ok; no `dimension` is inserted (optional element; Q6); CR |
| P0-E7 | `64759.xlsx` (threaded comments + note on a cell) | overwrite every cell | Ok; `threadedComments`, `persons`, `comments`, VML L1; CR |
| P0-E8 | `57362.xlsx` (chart over `A1:B3`) | write `B2` = 5 | Ok; chart part L1 (its `numCache` is now stale: documented, §4); CR |
| P0-E9 | `51850.xlsx` (first sheet: `sortState`, `drawing`; second sheet: x14 data validation with `xm:f` in `extLst`) | sweep the first sheet | Ok (the scan sees `xm:f`, which is not a cell formula); second sheet L1; CR |
| P0-E10 | `54399.xlsx` (external links, customXml) | write `B6` | Ok; `externalLink*.xml`, `customXml/*` L1; CR |
| P0-E11 | `ExcelPivotTableSample.xlsx` (table `E1:G4` on first sheet; 2 pivot caches) | write `A1` (outside table); write `E2` (table body); write `E1` (table header) | `A1`, `E2` Ok, pivot parts L1 (pivot staleness is Excel-normal, §4); `E1` → `Err(TableHeaderCell)` (Q5) |
| P0-E12 | `pictures_in_cell_and_over_cell.xlsx` (`A1`, `A3` are `t="e" vm=…`) | write `A1`; write `A2` | `A1` → `Err(RichValueCell)` (Q4); `A2` Ok; `richData/*`, `metadata.xml` L1; CR |
| P0-E13 | `62629_target.xlsm` (VBA, ctrlProps; first sheet `<sheetData/>`, `extLst` after `pageSetup`) | write `A1` | Ok (`<from>` must not trigger the formula scan); `vbaProject.bin` L1; `.xlsm` content type unchanged |
| P0-E14 | `52348.xlsx` (no producer; 24 inline rich strings, every `t` with `xml:space="preserve"`; 137 valueless `t="inlineStr"` cells; 27 merges; no `sharedStrings.xml`) | sweep inline-string cells outside merges, including the value `"  padded "` | Ok; the new `<t>` gets `xml:space="preserve"` exactly when the value has outer whitespace (ADR-0001 R2-M5); a valueless `t="inlineStr"` cell written with a number drops `t`; no `sharedStrings.xml` is created |
| P0-E15 | from scratch | `Workbook::new()` → `add_worksheet()` → `write_string(0,0,"a")`, `write_number(1,1,2.0)` → `save` | template `<sheetData/>` grows rows; second `add_worksheet()` → `Err`; named `add_worksheet` → `Err`; `docProps/core.xml` regenerated (Replaced), every other template part L1 vs the embedded template; CR |

### 7.5 Expected refusals (no output written; input file unchanged)

| ID | Fixture | Call | Expected |
|---|---|---|---|
| P0-R1 | `shared_formulas.xlsx` (40 shared, `calcChain`) | any `set_*` on any cell | `Err(FormulaPresent)`; detected at open, cached |
| P0-R2 | `70005-countifs.xlsx` (LibreOffice, formulas, **no** `calcChain`) | any `set_*` | `Err(FormulaPresent)` (ADR-0001 R3-M1: absence of calcChain is not safety) |
| P0-R3 | `MatrixFormulaEvalTestData.xlsx` (array formulas), `xlookup.xlsx` (dynamic array, `cm`), `NewStyleConditionalFormattings.xlsx` | any `set_*` | `Err(FormulaPresent)` |
| P0-R4 | `59746_NoRowNums.xlsx` (no `r` on rows/cells; part `xl/worksheets/sheet.xml`) | `cell(0,0)` and `set_number` | both `Err(MissingReference)` — reads too (ADR-0002 R3-m4) |
| P0-R5 | `InlineString.xlsx` (one row and cell without `r`) | `cell(0,0)` | `Err(MissingReference)` |
| P0-R6 | `61605.xlsx` | `set_number` on `C1` (inside `B1:E1`, not anchor) | `Err(MergedCell)` |
| P0-R7 | any | `cell_mut(r,c)` with no setter call, then `save` | no change at all: every entry L1 (ADR-0001 R2-m1) |
| P0-R8 | `fixture_minimal.xlsx` | `set_formula` | The method does not exist in Phase0, so formula writes are not expressible; `set_string("=1+1")` stores text |
| P0-R9 | synthetic `worksheet` with an `mc:AlternateContent` child | write | A missing `dimension` is never inserted (Q6), so with `<sheetData/>` present the write succeeds and no `dimension` appears; without `sheetData` its insertion is refused: `Err(UnresolvedSibling)` (ADR-0002 §4.3), nothing changed |

### 7.6 What Phase0 does not claim even when all of this passes

- That Excel opens the outputs without a repair dialog: owner's XL check. `cargo run -p
  ooxml-xlsx --example manual_check -- <outdir>` writes the outputs of P0-E1, E2, E3, E4, E11,
  E12, E13, E14, E15 plus the `x:`-prefixed (Hancom) and hidden-first-sheet cases, the unedited
  originals, and `CHECKLIST.md` (what to look for, how to report).
- Correct `spans` hints after edits outside a span (left as is; XL check covers it).
- Anything about sheets other than the first, except that they are byte-preserved.

### 7.7 Implementation status (milestone `xlsx-phase0`)

Every row of §7.2–§7.5 is an automated test in `crates/ooxml-xlsx/tests/` (`package.rs`,
`edit.rs`, `refusals.rs`), with the changes above. Additions beyond the table: the T2 oracle
compares the bytes *between* allowed regions piecewise, so a misplaced insertion fails it, and
`tests/oracle.rs` checks that L1, T2 and CR do fail on a stray byte, a misplaced cell, a changed
part and a wrong value; every readable cell of every openable fixture decodes to the value
calamine reads; same-path `save` is exercised on 17 fixtures. P0-E3 sweeps every 26th cell
(20 cells); P0-R1–R3 try every 5th cell plus a new one (debug-build run time).

Reading choices that follow calamine (the inherited-`xml:space` case is not yet checked in Excel): a `t` that does
not itself carry `xml:space="preserve"` has its outer ASCII whitespace (space, tab, CR, LF) trimmed;
an `xml:space` inherited from `si`/`is`/`r` is ignored, and other whitespace (NBSP, U+3000) is kept;
phonetic runs (`rPh`) are not part of the value; `_xHHHH_` escapes are decoded (surrogate pairs
combined).

## 8. Open questions for the project owner

Decisions of 2026-11-16: **Q1** accepted (Phase1 = formulas and calc state, ADR-0003). **Q2**
first *visible* worksheet (ADR-0001 note). **Q7** recorded as an ADR-0001 note. **Q9** the owner
runs the XL check on current Microsoft 365 Excel. Coordinator defaults, open to revision:
**Q3** any cell of a merged range is refused, top-left included (ADR-0001's wording; the
proposal above to allow the anchor is *not* adopted); **Q4** `vm` cells refused; **Q5** table
header rows refused (table parts read); **Q6** a missing `dimension` is never inserted; **Q8**
third-party fixtures stay committed under a 5 MB cap.

The original questions, for the record:

- **Q1 (ADR-0003 topic).** Is "formula-aware edits and calculation state" acceptable as Phase1,
  with registries as Phase2? (§6 explains the ordering.)
- **Q2 (first sheet).** ADR-0002 §6 defines "sheet1" as the first `<sheet>` in workbook order.
  When that is a macro/chart/dialog sheet (`xlmmacro.xlsm`) or hidden (`TwoSheetsOneHidden`),
  should Phase0 return `Err(NotAWorksheet)` / the hidden sheet (current proposal), or the first
  *visible worksheet*?
- **Q3 (merged cells).** ADR-0001 refuses writes "into a cell inside a merged range". Proposal:
  the top-left anchor is writable (that is where Excel stores the value), other cells refuse.
- **Q4 (rich-value cells).** Add `vm`-bearing cells to Phase0's refusal list (`Err(RichValueCell)`)?
  Overwriting them would leave the rich value bound to the cell.
- **Q5 (table header cells).** Add writes into a table header row to Phase0's refusal list? This
  requires reading `tableParts` → table parts (read-only allowlist additions:
  `x:CT_TableParts/x:tableParts`, `x:CT_TablePart/x:tablePart`, `x:CT_Table/x:table`).
- **Q6 (`dimension` absent).** Proposal: never insert a missing `dimension` (optional element).
- **Q7 (ADR-0001 wording).** The formula check should be specified as "SML-namespace element with
  local name `f`", not "`<f` start element" (§5 finding 1). Record as a post-acceptance note?
- **Q8 (fixture policy).** Third-party test files are committed (≈ 290 KB, Apache-2.0 + MIT,
  with notices). OK to keep growing this set per phase (cap ≤ 5 MB), or keep large corpora
  download-only as the Word/PowerPoint research did?
- **Q9 (Excel check).** Who runs the XL check, and on which Excel version(s)?

## Sources

- ADR-0001, ADR-0002 and their reviews in `docs/adr/`.
- Open XML SDK data `dotnet/Open-XML-SDK@431ab05c` (`data/parts/*.json`, `data/schemas/*.json`,
  MIT): part names, `CT_Worksheet`/`CT_Workbook` child order.
- ECMA-376 Part 1, 5th ed. (2016), §18 SpreadsheetML (element semantics referenced above).
- Apache POI `test-data/spreadsheet` at `47dcff769bb395034dddfd59c4609659b6dbc6a4` (Apache-2.0);
  POI source at the same commit: `poi-ooxml/src/main/java/org/apache/poi/xssf/usermodel/XSSFWorkbook.java`
  (`onDeleteFormula`), `…/model/CalculationChain.java` (`removeItem`, `i` inheritance),
  `…/usermodel/XSSFSheet.java` (`onDeleteFormula`: shared-formula master promotion),
  `…/usermodel/XSSFTable.java` (`updateHeaders`: "Found unreadable content" if out of sync).
- calamine `tests/` at `93e2e6b32b87a7b1e80bd036ea63bbe6f1ca90a6` (MIT).
- rust_xlsxwriter 0.102.0: <https://docs.rs/rust_xlsxwriter/0.102.0>; source
  `src/worksheet.rs`, `workbook.rs` (`calcPr fullCalcOnLoad`), `performance.rs`, `formula.rs`,
  `conditional_format.rs`, `datetime.rs` (1904 not exposed), `utility.rs` (legacy hash),
  `rich_value*.rs`, `sparkline.rs`, `table.rs`, `chart.rs`.
- calamine 0.38.0: <https://docs.rs/calamine/0.38.0>; source `src/xlsx/mod.rs`
  (`expand_shared_formula`, `has_1904_epoch`, `XlsxError::Password`, pivot tables, defined names),
  `src/xlsx/cells_reader.rs`, `src/style.rs`, `Changelog.md`.
- umya-spreadsheet 3.1.1: <https://docs.rs/umya-spreadsheet/3.1.1>; source `src/reader/xlsx.rs`
  (unknown rels ignored), `src/writer/xlsx.rs`, `src/structs/cell.rs` (`cm` not written),
  `src/structs/raw/raw_worksheet.rs` (lazy raw pass-through), `src/helper/crypt/`, `CHANGELOG.md`.
- openpyxl 3.1.5: <https://openpyxl.readthedocs.io/en/stable/pivot.html>, `/comments.html`,
  `/tutorial.html`; source <https://foss.heptapod.net/openpyxl/openpyxl>: `openpyxl/cell/_writer.py`
  (inline strings), `worksheet/_reader.py` (extLst warnings, shared formula translation),
  `xml/constants.py` (`EXT_TYPES`), `reader/drawings.py` (shapes lost), `writer/excel.py`
  (`keep_vba` part list, blank `app.xml`), `workbook/properties.py` (`fullCalcOnLoad`).
- Survey scripts and outputs: `spikes/xlsx/` (README lists commands); Phase2 registry facts:
  `spikes/xlsx-phase2/` (ADR-0006 lists its own sources: rust_xlsxwriter @ `d850973d`,
  openpyxl 3.1.5 sdist, calamine 0.38, [MS-XLSB] `BrtKnownFonts`).
