# Word (WordprocessingML) roadmap and gap analysis

## 概要（日本語）

- ooxml-rs には Word の実装がまだ無い。この文書は、Word の機能ごとに関係するパート・要素、パートをまたぐ ID の整合（ADR-0001 Layer 3 にあたるもの）、難しさ、既存ライブラリ（python-docx、docx-rs、docx-rust、Apache POI XWPF）の対応状況をまとめ、段階的な目標（Word Phase0〜4）と受け入れ条件・検証方法を定める。
- 根拠は実ファイル 631 個（Apache POI のテストデータ 129 個、Open XML SDK のテスト資産 502 個）の計測。スクリプトは `spikes/docx/` にある。
- 主な判断:
  - Phase0 は「既存のランのテキスト置換」と「本文末尾（`sectPr` の前）への段落追加」だけにする。
  - フィールドの中、変更履歴の中、SDT の中の書き込みは `Err` にする。
  - 内容に依存するフィールド（TOC、REF、PAGEREF、SEQ など）を含む文書と、変更履歴の記録がオンの文書への書き込みも `Err` にする。
  - 新しい ID（ブックマーク、コメント、`w14:paraId`、rsid）は作らない。
  - 独立した読み戻しの検証には `docx-rs` 0.4.22（`default-features = false`）を dev-dependency として使う。
- 実測の要点:
  - 隣り合うテキストランの組の 60〜76% は書式（`w:rPr`）が同一で、rsid が違うだけで分かれている。ランをまたぐ置換は Phase1 で「書式が同じランの並びの中だけ」許す。
  - `w14:paraId` の重複は 8 ファイルにあった。実ファイルは ID の一意性を守っていないので、読み込みで一意性を前提にしない。
  - ブックマーク ID の重複、変更履歴 ID とブックマーク ID の衝突は 0 件だった。
- Phase0 の詳細は ADR-0004（`docs/adr/0004-docx-phase0.md`、revision 2）。実装は `crates/ooxml-docx`。
- 2026-11-16 の更新（ADR-0004 revision 2 と同時）: 文書保護（`w:documentProtection` の強制）も書き込み拒否に加えた。入れ子のフィールドはスタックで全階層を判定する。`Document::new()`（新規作成）は Phase0 から外した（所有者の判断）。

## 1. Method and status legend

**Corpora.** Measured with the scripts in `spikes/docx/` (README there), on:

- POI: Apache POI `test-data/document/*.docx`, 129 files. 114 open as zip. The other 15 are 2 password-protected CFB files and 13 fuzz or truncated cases.
- SDK: Open XML SDK test assets, 502 `.docx/.docm/.dotx` files. "SDK (non-Strict)" is the 444 files outside `O14ISOStrict/`.

Both corpora are mostly small test documents, so frequencies are lower bounds for real-world documents. The 25 chosen fixtures are in `reference/fixtures/docx/`.

**Status of ooxml-rs:** nothing Word-specific exists. `crates/ooxml-core` (Package, RawNode, lens) is being built on `feat/foundation-v0`, and `ooxml-docx` will be built on it. Every row below is therefore "none" today. The tables give the target phase instead.

**Difficulty:**

- **L** = one part, no cross-part IDs.
- **M** = several parts or a read-only cross-part lookup.
- **H** = needs a Layer 3 registry (new IDs or new parts) or a semantic model (inheritance, layout).
- **XH** = needs layout or an external engine (pagination, field evaluation, rendering).

**Library coverage.** Checked against source at a pinned revision, unless marked otherwise:

- python-docx `e4545460` (1.2.0)
- docx-rs 0.4.22 (`bokuweb/docx-rs` `3b959fa7`)
- docx-rust 0.1.11 (`cstkingkey/docx-rs` `b3852d28`)
- Apache POI XWPF trunk `47dcff76`

Markers:

- ✓ = public API exists.
- R = read only.
- W = write or generate only.
- ✗ = absent.
- *(cl)* = from the changelog.
- *(u)* = unverified, from documentation or memory only.

A "✓" means the API exists. It does not mean the library round-trips the feature losslessly (see §4).

## 2. Feature surface vs. ooxml-rs

| # | Feature | Parts / key elements (SDK `Name`s) | Cross-part IDs / registries (Layer 3 analogue) | Diff. | python-docx | docx-rs | docx-rust | POI XWPF | Phase |
|---|---|---|---|---|---|---|---|---|---|
| 1 | Paragraph and run **text** | `document.xml`: `w:CT_P/w:p`, `w:CT_R/w:r`, `w:CT_Text/w:t`, `w:CT_Empty/w:tab`, `w:CT_Br/w:br`, `w:CT_Empty/w:cr`. Runs also sit under `w:hyperlink`, `w:ins`, `w:smartTag`, `w:sdtContent`, `w:fldSimple`, `w:customXml` | none | L | ✓ (`Paragraph.text` only covers `w:r\|w:hyperlink`, *src* `oxml/text/paragraph.py`) | ✓ | ✓ (no `w:ins` text: 0/36 in the spike) | ✓ (`XWPFParagraph` includes `w:ins` runs, also includes runs under `w:del`, *src*) | **0** (direct runs), 1 (all stories) |
| 2 | Text **across runs** (find/replace) | as 1 plus `w:proofErr`, `w:bookmarkStart/End`, `w:lastRenderedPageBreak` between runs | none (bookmark ranges must not be broken, see §3 Q1) | M | ✗ | ✗ | ✗ | R (`searchText` scans direct `w:r` only, *src*) | 1 |
| 3 | **Run formatting** (direct) | `w:CT_RPr/w:rPr`: 51 single-QName slots, 191 `w:CT_OnOff/…` entries | none | L | ✓ | ✓ | ✓ | ✓ | 1 |
| 4 | **Paragraph formatting** (direct) | `w:CT_PPr/w:pPr`: 36 slots | none (`numPr`: row 6) | L | ✓ | ✓ | ✓ | ✓ | 1 |
| 5 | **Styles** and inheritance (effective values) | `styles.xml` (`w:style`, `w:basedOn`, `w:link`, `w:docDefaults`, `w:latentStyles`), `stylesWithEffects.xml` (2010) | **Style-ID registry**: append-only, never rename. Real files have up to hundreds of styles (POI: 5,097 `w:style`, 3,579 with `basedOn`) | H (resolver: toggle properties XOR, table-style conditional formatting) | ✓ defs, no resolver *(u)* | ✓ defs | ✓ defs | ✓ defs; no resolver *(u)* | 1 (reference existing IDs), 2 (create), 3 (resolver) |
| 6 | **Numbering / lists** | `numbering.xml`: `w:abstractNum`, `w:num` → `abstractNumId`, `w:lvlOverride`, `w:numStyleLink`/`styleLink`; `w:numPr` in `pPr` | **numId / abstractNumId registry** (POI: 327 `w:num`, 25 `lvlOverride`). Restarting a list needs a new `w:num` with `lvlOverride/startOverride` | H | ✗ (oxml only) | ✓ | ✓ | ✓ | 2 |
| 7 | **Tables** | `w:CT_Tbl/w:tbl`, `w:CT_Row/w:tr`, `w:CT_Tc/w:tc`, `tblGrid`, `gridSpan`, `vMerge`; nested tables | none (cell text). Merges are structural invariants (grid width) | M | ✓ | ✓ | ✓ | ✓ | 1 (cell text), 2 (structure) |
| 8 | **Sections / page setup** | body-level `w:sectPr` (last child of `w:body`) plus `w:pPr/w:sectPr` for every section break; `pgSz`, `pgMar`, `cols`, `titlePg` | header/footer refs → rels (row 9) | M | ✓ | ✓ | ✓ | partial (`CTSectPr`) | 1 (read), 2 (edit) |
| 9 | **Headers / footers** | `header*.xml`/`footer*.xml` reached **only via rels** (POI `Bug60341`: `word/footer.xml`); `w:headerReference`/`footerReference` `@type` default/first/even; `settings/w:evenAndOddHeaders` | **OPC part + rel + content-type registry**; sectPr→rId | M (edit text), H (create) | ✓ | ✓ | ✓ | ✓ | 1 (text), 2 (create) |
| 10 | **Footnotes / endnotes** | `footnotes.xml`/`endnotes.xml`; separator entries (`w:type="separator"`, `continuationSeparator`); `w:footnoteReference`/`endnoteReference`; `settings/footnotePr` | **Note-ID registry** per kind; the separator IDs referenced from `settings.xml` | H | ✗ | ✓ footnotes, ✗ endnotes *(src: no endnote module)* | ✓ | ✓ (`createFootnote`) | 1 (text), 3 (create) |
| 11 | **Comments** | `comments.xml`; `commentRangeStart/End` + `commentReference` in stories; `commentsExtended.xml` (w15: `paraId` = `w14:paraId` of the comment's **last** paragraph (1,090/1,091 match), `paraIdParent` for replies, `done`); `commentsIds.xml` (w16cid `durableId`); `commentsExtensible.xml` (w16cex); `people.xml` | **Annotation-ID registry** (`w:id`), **paraId registry**, **durableId registry**, author registry (people). 5 parts must agree | H | ✓ (1.2.0 *(cl)*: basic comments, no replies *(u)*) | ✓ (+ `commentsExtended`) | ✓ | ✓ (`XWPFComments`; extended *(u)*) | 1 (read), 3 (create/reply/resolve) |
| 12 | **Tracked changes** | `w:ins`/`w:del`/`w:moveFrom`/`w:moveTo` (run level), `w:delText`, `w:rPrChange`, `w:pPrChange`, paragraph-mark `pPr/rPr/ins\|del`, table-row/cell revisions; `settings/w:trackRevisions` | **Annotation-ID registry**; author/date | H (generate), XH (accept/reject with numbering and moves) | ✗ | ✓ (ins/del/move, pPrChange) | ✗ | R + `setTrackRevisions` | 0 (detect → Err), 3 (generate, accept/reject) |
| 13 | **Fields**: simple, complex, cached results | `w:fldSimple`; `w:fldChar` begin/separate/end + `w:instrText` (split across runs), nested fields; result runs between `separate` and `end`; `settings/w:updateFields`; `fldChar/@dirty`, `@fldLock` | Bookmark names (REF/PAGEREF), `SEQ` identifiers, TOC→`_Toc` bookmarks | H (structure), XH (evaluation: page numbers need layout) | ✗ | W (`PAGE`, `NUMPAGES`, `TOC`, `PAGEREF`, `HYPERLINK` generation) | R (`fldChar`/`instrText` structs) | `XWPFFieldRun` (fldSimple), `createTOC`, `enforceUpdateFields` | 0 (detect → Err), 1 (opt-in updateFields), 3 (insert) |
| 14 | **Bookmarks** | `w:bookmarkStart`/`End` (in `w:p`, `w:body`, `w:tbl`, `w:tr`; `colFirst`/`colLast` for table columns) | **Annotation-ID registry** + bookmark-name uniqueness (`_GoBack`, `_Toc…`, `_Ref…`) | M | ✗ | ✓ | ✓ | ✗ (CT only) *(u)* | 1 (read), 2 (create) |
| 15 | **Hyperlinks** | `w:hyperlink` with `r:id` (external, rel `TargetMode="External"`) or `w:anchor` (bookmark); complex `HYPERLINK` field form | **rels registry** (new rId); anchor→bookmark name | M | R (1.0 *(cl)*) | ✓ | ✓ | ✓ (`XWPFHyperlinkRun`) | 1 (text), 2 (create) |
| 16 | **Content controls (SDT)** | `w:sdt` at block/run/row/cell level (`w:CT_SdtBlock/…`, `CT_SdtRun`, `CT_SdtRow`, `CT_SdtCell`), `w:sdtPr` (alias, tag, `w:id`, type: text/date/dropDownList/comboBox/picture/`w14:checkbox`/docPartObj), `w:dataBinding` → `customXml/item*.xml` + `itemProps` | SDT `w:id` registry; **dataBinding `storeItemID`** → customXml part; Word refreshes bound content from the custom XML on open (*(u)*, so editing bound text without editing the XML is lost) | H | ✗ | ✓ (+dataBinding) | ✓ | R (`XWPFSDT`) | 0 (Err inside), 3 |
| 17 | **Images / drawings** | `w:drawing` → `wp:inline`/`wp:anchor` → `a:graphic` → `pic:pic` (`r:embed` → media part); `wps`/`wpg`/`wpc` via `mc:AlternateContent` (`mc:Choice Requires="wps"` with VML `mc:Fallback`) inside `w:r`; legacy `w:pict` (VML), `w:object` (OLE) | **media part + rel + content-type default**; `wp:docPr/@id` unique per document; `pic:cNvPr/@id` | H (inline), XH (anchor/wrap) | W inline only *(src: `shape.py`)* | ✓ (pic, anchor, wps textbox) | ✓ (`drawing`) | ✓ (`XWPFPicture`) | 2 (inline), 4 (anchor/wps) |
| 18 | **Math (OMML)** | `m:oMath`/`m:oMathPara` inside `w:p` (shared-math schema, `m:` on/off types) | none | H (structure is large) | ✗ | ✗ | ✗ | ✗ (CT only) *(u)* | 4 (passthrough is Phase0) |
| 19 | **Charts** | `w:drawing/…/c:chart r:id` → `charts/chartN.xml` (+ `colors`/`style` parts, embedded `.xlsx` package); `cx:` chartex | chart part chain + embedded workbook; shared DrawingML/chart with ooxml-xlsx/pptx | XH | ✗ | ✗ | ✗ | ✓ (`XWPFChart`) | 4 (shared with PowerPoint/Excel ADRs) |
| 20 | **Settings, rsids, `w14:paraId`/`textId`** | `settings.xml` (103 single-QName slots: `trackRevisions`, `updateFields`, `documentProtection`, `rsids`, `compat`, `w14/w15:docId`); `w:rsidR`/`rsidRPr`/`rsidP` on p/r; `w14:paraId`/`textId` on p/tr | **rsid registry** (settings/rsids, optional); **paraId registry**: values < 0x80000000, uniqueness **not** held by real files (§3 Q4) | M | partial | ✓ (docId, paraId) | ✓ | ✓ (`XWPFSettings`) | 0 (read 2 flags), 2 |
| 21 | **Document protection** | `settings/w:documentProtection` (hash), `w:permStart/End` | none | M | ✗ | ✗ *(u)* | ✗ *(u)* | ✓ | 0 (enforced protection refuses writes: `DocumentProtected`), 4 |
| 22 | **Glossary document** (building blocks) | `glossary/document.xml` + its own styles/numbering/settings (a second main-like story tree) | its own registries | H | ✗ | ✗ | ✗ | ✗ *(u)* | passthrough only |
| 23 | **altChunk** (imported HTML/RTF/docx) | `w:altChunk r:id` → `AlternativeFormatImportPart` | rel + part | M (create), passthrough (read) | ✗ | ✗ | ✗ | ✗ *(u)* | 4 (no fixture yet) |
| 24 | **Smart tags, `customXml` markup** | `w:smartTag` (ECMA Transitional; **not in SDK `CT_P`**: an unlisted child), `w:customXml` (5 SDK entries with `attrs_incomplete`) | none | M | ✗ | ✗ | ✗ | R (smartTag runs flattened) | 1 (text inside via an `add_child` override) |
| 25 | **Legacy form fields** | complex fields `FORMTEXT`/`FORMCHECKBOX` with `w:ffData` | bookmark names | M | ✗ | ✗ | ✗ | ✗ *(u)* | 4 |
| 26 | **Package-level**: macro/template variants, Strict, encryption | main-part content type (`.docx`/`.docm`/`.dotx`/`.dotm`); Strict namespaces (`purl.oclc.org`); CFB-encrypted packages | OPC | M–H | ✓/✗/✗ | ✗ Strict (panics on `Strict01.docx`) | ✗ | ✓ / ✗ / ✓ (decrypt) | 0 (accept all four main types; Err for Strict and encrypted) |
| 27 | **From scratch** (`Document::new()`) | template package (main, styles, settings, fontTable, theme, docProps) | as Excel: same edit path, seeded from an embedded template | L | ✓ (default template) | W | W | ✓ | 1 (deferred from Phase0 by the owner, 2026-11-16) |

Coverage observations:

- **Every reference library either regenerates whole parts** (docx-rs, docx-rust: typed structs; POI: XmlBeans; python-docx: an lxml tree) **or drops what it does not model.** The docx-rs reader skips unknown elements (`reader/ignore.rs`). docx-rust's strict typed parse failed on 9 files that are valid zips, e.g. `headerReference/@type="odd"`, a `w:tbl` without `tblGrid`, and a style without `styleId`. Separately, docx-rs panicked on 2 files. python-docx keeps the lxml tree, so unknown XML survives, but whole parts are re-serialized.
- None of them offers "edit text without touching anything else" with byte-level locality, which is ooxml-rs's reason to exist (ADR-0001).
- `rdocx` 0.17 (tensorbee, 2026-11) claims to "retain unknown safe producer XML byte for byte" and has `replace_all`. *(u)*: not evaluated here, and worth a dedicated look before Phase1, as ADR-0001 did for `ooxmlsdk`.

## 3. Design questions, with evidence

All numbers come from `spikes/docx/design_facts.py` and `phase0_gate.py`. "POI" means the 114 openable POI files, "SDK" the 442 openable non-Strict SDK files.

### Q1. Text that spans runs (find/replace)

**Facts.**

- Paragraphs with text that have ≥ 2 text runs: POI 1,261 of 21,943 (5.7%), SDK 7,922 of 33,361 (23.7%).
- Adjacent direct text-run pairs: POI 6,195, of which **4,719 (76%) have byte-identical `w:rPr`**. SDK: 23,532, of which **14,163 (60%)**. These splits carry no formatting difference (rsid or proofing artefacts).
- What separates two adjacent text runs:
  - POI: nothing 2,846, `w:proofErr` 2,889, `w:hyperlink` 264, a field-char run 136, `w:br` run 89, bookmarks 72, `smartTag` 21.
  - SDK: nothing 17,319, `proofErr` 4,998, comment range markers or reference runs ~2,190, `smartTag` 212, `sdt` 107.
- Parents of text-bearing runs:
  - POI: `w:p` 28,074, `hyperlink` 372, `ins` 38, `smartTag` 30, `sdtContent` 28, `del` 12, `fldSimple` 6.
  - SDK: `w:p` 47,609, `hyperlink` 1,822, `sdtContent` 777, `smartTag` 510, `fldSimple` 109, `ins` 88, `del` 58.

**Decision.**

- **Phase0** exposes only per-run text replacement: `Document::set_run_text(paragraph, run, text)`, plus a read-only `Paragraph::text()`.
- **Phase1** adds `Paragraph::replace(needle, replacement)` with this rule:
  - The match is computed over the concatenated text of a **run group**: maximal sequences of direct text runs of one parent (`w:p` or `w:hyperlink`), separated only by nothing, `w:proofErr`, or `w:bookmarkStart`/`End`.
  - A match that crosses anything else (a hyperlink boundary, a field character, `ins`/`del`, `sdt`, a comment marker, a non-text child such as `w:drawing`) is `Err(MatchCrossesBoundary)`. It is never silently skipped.
  - The replacement goes into the **first** run of the match, which keeps its `w:rPr`. The matched remainder is cut from the following runs. A run left with no text loses its `w:t` but stays in place, so rsids and IDs are untouched and no element moves.
  - If the matched runs' `w:rPr` differ, the result takes the first run's formatting. This is opt-in (`ReplaceMode::FirstRunFormat`). The default is `Err(MixedFormatting)`, because 24–40% of splits are real formatting changes.
  - Bookmark markers inside a match stay at their original positions relative to the surviving text. A marker would end up inside the replaced span → `Err(MatchCrossesBoundary)`.
- POI's `searchText` and python-docx do the naive, direct-runs-only version. Neither handles the boundaries above.

### Q2. Fields and their cached results (the Excel formula-cache analogue)

**Facts.**

- POI: 124 complex fields (90 with a cached result) and 6 `fldSimple`.
- SDK: 658 complex fields (544 with a cached result), 118 `fldSimple`, 397 nested (mostly `PAGEREF` inside `TOC`), 21 spanning paragraphs (every one a `TOC`), and 54 with the instruction split over more than one `w:instrText` run. Example: `sdk-Field-more-field`, where `DATE \@ "h时m分s秒"` is split over 9 runs.
- `settings/w:updateFields` appears in **0** files, and `fldChar/@dirty="true"` in **0**.
- Field codes seen:
  - POI: `HYPERLINK`, `PAGE`, `NUMPAGES`, `FORMTEXT`, `FORMCHECKBOX`, `MERGEFIELD`, `AUTHOR`, `CREATEDATE`, `FILENAME`.
  - SDK: also `PAGEREF` 391, `EQ` 66, `TOC` 19, `DATE` 17, `REF` 13, `SEQ` 6, `STYLEREF` 5, `IF` 5, `TC` 5, `=` 2, and others.

**Analysis.** As with Excel's `<v>` under `<f>`, the runs between `separate` and `end` hold a value Word computed. Any reader except Word shows it as is: python-docx, docx-rs, search indexers, PDF converters. Field kinds fall into two groups by how an edit affects them:

- **Layout or metadata fields** (`PAGE`, `NUMPAGES`, `SECTIONPAGES`, `DATE`, `TIME`, document properties, `FILENAME`, `HYPERLINK`, `MERGEFIELD`, form fields, `EQ`, `SYMBOL`): a text edit elsewhere does not make the cached result *wrong* in a way a non-Word reader could have known. Page numbers need layout in any case.
- **Content-dependent fields** (`TOC`, `REF`, `PAGEREF`, `NOTEREF`, `SEQ`, `STYLEREF`, `=`, `IF`, `NUMWORDS`, `NUMCHARS`, `INDEX`, `TOA`, `TC`, `LISTNUM`, `CITATION`, `BIBLIOGRAPHY`, and any code ooxml-rs cannot classify): editing a heading or bookmarked text silently leaves a stale result, exactly ADR-0001's R2-M3 problem.

**Decision.**

- **Phase0 refuses** writes inside a field (between `begin` and `end`, or inside `w:fldSimple`).
- Phase0 also refuses **every write** to a document that has a content-dependent field in **any** story part: `Err(StaleFieldRisk { codes })`. This is checked by a scan at open, like ADR-0001's formula scan. The scan reassembles split `instrText` before classifying, tracks fields with a stack and classifies every nesting level (ADR-0004 rev 2, §6 W2).
- Measured cost: 30 of 444 SDK files and 1 of 114 POI files (which uses `M:USERCONTENT`, a code we cannot classify) are refused for this reason.
- **Phase1** adds the opt-in alternative: `Document::set_update_fields_on_open(true)` writes `<w:updateFields w:val="true"/>` into `settings.xml`, which brings that part into Layer 2. This lifts `StaleFieldRisk`. It is POI's `enforceUpdateFields`.
  - Cost to the user: Word asks "This document contains fields that may refer to other files. Update?" on open *(u: Word behaviour, owner to confirm)*.
  - Non-Word readers still see stale results, so the API documents this.
- Evaluating fields ourselves (TOC generation, PAGEREF) needs layout and is XH, out of the plan.

### Q3. Edits inside `w:ins` / `w:del`

**Facts.**

- Text runs inside `ins`: POI 38, SDK 88. Inside `del`: POI 12, SDK 58.
- Paragraph-mark revisions: POI 20, SDK 52.
- `settings/w:trackRevisions` is on in 5 POI and 6 SDK files. `poi-delins` writes it as an empty element, `<w:trackRevisions/>`, which **means on** (Q5).
- Reference readers disagree on revisions:
  - POI includes `w:del` runs in a paragraph's runs (source).
  - python-docx ignores `w:ins` text.
  - docx-rust drops `w:ins` text (0/36 in the spike).
  - docx-rs keeps it (36/36).

**Decision.**

- **Phase0:**
  - Any write targeting a run whose ancestor is `w:ins`, `w:del`, `w:moveFrom` or `w:moveTo` is `Err(InsideRevision)`.
  - When `trackRevisions` is on, every write is `Err(TrackRevisionsOn)`, because the user expects edits to be tracked and an untracked edit silently defeats the audit trail.
  - `Paragraph::text()` returns the "all revisions accepted" view: it includes `ins`/`moveTo` text and excludes `delText`/`moveFrom`. A separate `text_with_deletions()` is not in Phase0.
- **Phase3** generates tracked edits: `w:del` around the old runs, `w:ins` around new ones, author/date, and IDs from the annotation-ID registry. Accept/reject is later still, because it must handle moves, numbering changes and paragraph-mark merges.

### Q4. ID uniqueness (bookmarks, comments, revisions, `w14:paraId`)

**Facts.**

- Bookmark IDs duplicated within a part: **0** files. Bookmark IDs repeated only across parts: **0**.
- A revision ID equal to a bookmark ID in the same file: **0** files. Duplicate revision IDs: **0**.
- Duplicate bookmark names: 1 file (`56392.docx`, `__DdeLink__…`, a LibreOffice-written file).
- `w14:paraId` appears in 16 POI and 66 SDK files, and is **duplicated in 2 and 6 of them**:
  - `poi-table-alignment` has copy-pasted rows.
  - `poi-shapes-with-text` duplicates text-box paragraphs between `mc:Choice` and `mc:Fallback`, which is legitimate because only one branch is live.
  - `Complex01.docx` and `Document.docx` have 6 and 4 duplicates.
- Each `commentsExtended` entry points at the last paragraph of its comment by `paraId`: 1,090 of 1,091 match.
- 54 SDK files bind `w15` to a pre-release URI (`…/word/2010/11/wordml`) that is not in the SDK data.

**Decision.**

- **Reads never assume uniqueness.** A lookup by `paraId` returns all matches.
- **Phase0 creates no IDs at all:**
  - Appended paragraphs carry no `w14:paraId`, `w14:textId` or `w:rsid*`. All are optional, and Word assigns them on its next save *(u)*.
  - No bookmark, comment or revision is created.
  - Edited runs keep their attributes.
- **Phase2** adds the registries, append-only as ADR-0001 Layer 3 requires: never renumber, never "fix" an existing duplicate.
  - The **annotation-ID registry** allocates from max+1 over the **union** of every `w:id` on bookmarks, comments, revisions and permission ranges in **all story parts**. The ECMA text calls the ID unique per annotation type, but the union is the conservative choice, and the corpus never collides it.
  - The **paraId registry** allocates random values < 0x80000000 that are absent from all parts, including `commentsExtended`, and from both branches of every `mc:AlternateContent`.
  - The **bookmark-name registry** is case-insensitive *(u: Word treats bookmark names case-insensitively; confirm)*.
- The `w15` URI variance means ooxml-rs must match namespaces by URI and treat unknown URIs as opaque. The ADR-0002 QName rule already does this.

### Q5. `w:CT_OnOff` semantics

**Facts.**

- ECMA Transitional `CT_OnOff` has `val` of type `ST_OnOff` = union of `xsd:boolean` and `{on, off}`, with **no default** in the XSD (`wml.xsd` l.23, `shared-commonSimpleTypes.xsd` l.64).
- Lexical forms seen on 13 common on/off properties under `rPr`/`pPr`/`sectPr`:
  - POI: absent 17,301, `0` 335, `false` 81, `true` 68, `1` 25, `on` 3.
  - SDK: absent 17,503, `0` 2,697, `1` 9.
  - `off` was never seen.
- The SDK data has 191 `w:CT_OnOff/…` entries plus one base entry.

**Decision.**

1. Generated code gets the ADR-0002 §1.3 override `set_default { on = "w:CT_OnOff/", attribute = "w:val", value = "true" }`. An element present without `val` is *on*. Without it, `<w:trackRevisions/>` in `poi-delins` would read as "unknown", and Phase0's gate would be wrong.
2. The typed read of an on/off **property** is `Option<bool>`:
   - `None` means the element is absent, and the value is *inherited*, not false.
   - `Some(true)` covers `true`, `1`, `on` and absent `val`.
   - `Some(false)` covers `false`, `0`, `off`.
   - Anything else is an `AttrError` with the raw string, per ADR-0002's never-lossy rule.
   - Collapsing absent to `false` is a classic bug for the toggle properties (`b`, `i`, `caps`, `smallCaps`, `strike`, `dstrike`, `outline`, `shadow`, `emboss`, `imprint`, `vanish`), whose effective value XORs through the style hierarchy (ECMA-376 Part 1 §17.7.3 *(u: section number)*). The resolver is Phase3.
3. Writes (Phase1) emit `<w:b/>` for true and `<w:b w:val="0"/>` for false. `0` is the form Word itself writes most (SDK 2,697 against 9).
4. `m:` (30 entries) and `w14:` (4 entries) on/off types need their own overrides. They are not needed before OMML or w14 effects (Phase4).

### Q6. `sectPr` placement and appending paragraphs

**Facts.**

- The body-level `w:sectPr` is the **last child of `w:body`** in 108/113 POI and 439/440 SDK bodies. It is absent in 5 and 1 bodies, and never present but not last.
- Section breaks put `w:sectPr` inside the **last paragraph's `w:pPr`** of each earlier section: POI 16, SDK 174.
- `w:body` lowers to [blocks (free slot, 35 QNames), `w:sectPr`] (ADR-0002 research note §2).
- One SDK file (`sdk-HelloO14`) has `mc:AlternateContent` directly under `w:body`.

**Decision.**

- Phase0's `append_paragraph` is `body.insert::<P>()`. The slot rule places it before the final `w:sectPr`, or at the end of the body when there is none (`poi-55733`).
- An appended paragraph never carries a `sectPr`, so it joins the **last** section.
- An `mc:AlternateContent` child of `w:body` makes the insert `Err(UnresolvedSibling)` (ADR-0002 §4.3 Phase0 rule).
- Inserting a section break (moving properties into a new `pPr/sectPr`) is Phase2.

### Q7. Other open-time hazards found on the way

- **DTD / external entities.** `poi-ExternalEntityInText` declares `<!ENTITY testent SYSTEM "http://poi.apache.org/">` and uses `&testent;`. ooxml-rs must refuse any part with a DOCTYPE (`Err(DtdNotAllowed)`) rather than parse it with entities unexpanded. OPC forbids DTDs in package XML *(u: ECMA-376 Part 2 requirement M1.17, confirm the clause)*. This belongs in `ooxml-core`'s parser for all formats and is proposed to the foundation owner.
- **Zip shapes.** `poi-table-alignment` contains zip **directory entries** (`word/`, `_rels/`), which Layer 1 must pass through. Several fixtures use stored (uncompressed) entries. Both BOM-prefixed and pretty-printed `document.xml` occur (`poi-Bug60341`, `sdk-HelloO14`): T1 must cover the BOM.
- **Part naming.** Parts are found through relationships only: the main part via `/_rels/.rels` `officeDocument`, headers and footers via `word/_rels/document.xml.rels` (`poi-Bug60341` names its footer `footer.xml`). `MainDocumentPart` has no fixed content type in the SDK parts data: four variants exist.
- **Missing parts.** `poi-55733` has no `styles.xml`. Phase0 must not require it.

## 4. Phased goals

Ordering principle: **user value × risk**. Value comes first:

1. Template filling: replace placeholders in existing documents.
2. Appending generated content.
3. Generating simple documents from scratch.
4. Structural features: lists, tables, images.
5. Review features: comments and tracked changes.

Risk is mostly cross-part ID consistency and stale caches. Each phase is an ADR (Phase0 is ADR-0004) and must keep every earlier oracle green.

| Phase | Goal (one line) | Layer 3 needed | Main risk retired |
|---|---|---|---|
| **0** | Edit text of existing runs and append plain paragraphs in `document.xml`, refusing honestly everywhere else | none (read-only `settings.xml`, event scan of stories) | byte-local edits in WML under real Word files |
| **1** | Template filling: cross-run replace, all story parts (headers, footers, notes, comments), table cells, direct formatting, existing styles by ID, opt-in `updateFields` | read-only style-ID lookup | run-group semantics; multi-part Layer 2 |
| **2** | Structure: new runs/paragraphs with formatting, tables, lists (numbering), sections, inline images, hyperlinks, bookmarks, new headers/footers, new styles | OPC parts/rels/content types, style IDs, numId/abstractNumId, annotation IDs, paraId, `docPr` IDs, bookmark names | append-only registries across parts |
| **3** | Review and semantics: comments (create, reply, resolve), footnotes/endnotes, tracked-change generation, content controls (incl. data binding), field insertion, effective formatting resolver | 5-part comment set, note IDs, SDT IDs, customXml store items | multi-part transactional consistency |
| **4** | Rich content: anchored drawings / `wps` / `wpg` (AC-rank rule), OMML, charts (with Excel/PowerPoint), legacy form fields, protection, altChunk, Strict read | DrawingML shared registries | `mc:AlternateContent` ordering; shared DrawingML |

### Phase0 (ADR-0004)

Implemented in `crates/ooxml-docx` (ADR-0004 revision 4, still Proposed).

**Scope.**

- `Document::open`/`from_bytes` and `save`/`to_bytes`. `Document::new()` (from scratch) is
  deferred to Phase1 by the owner's decision of 2026-11-16.
- Read: `paragraphs()`, `Paragraph::text()` and `runs()`, each run with a `RunContext` and
  `in_field()`.
- Write: `Document::set_run_text(paragraph, run, text)`, and `append_paragraph(text)` before
  `sectPr`.
- Gates: `EncryptedPackage`, `DtdNotAllowed`, `StrictNotSupported`, `TooDeep` (core's nesting
  limit, O5), `TrackRevisionsOn`, `DocumentProtected`, `StaleFieldRisk` (fields classified with
  their arguments for `INFO`/`DOCPROPERTY`; malformed field markup refused).
- Append refusal `SectionBreakAtEnd` when the last section's `sectPr` sits in the last paragraph.
- Reported only: `writeProtection`, Mark as Final, `updateFields`.
- Per-run refusals: `InsideField`, `InsideRevision`, `UnsupportedContext` (SDT, smartTag,
  customXml, bidi, ruby), `RunNotTextOnly`.

**Acceptance** (all automated in `crates/ooxml-docx/tests/`, except the manual check).

- **T1/T1b:** pass on every XML part of the 23 openable fixtures. T1 includes BOM and
  pretty-printed parts.
- **T2 (locality):** for every settable run in every writable fixture (3,423 runs),
  `set_run_text(own text)` and `set_run_text(other)` change bytes only inside that run's span;
  own text on a canonical run changes nothing. Append changes bytes only at the insertion point.
- **T3:** every other part is (name, method, CRC, payload) identical, directory entries included.
- **T4:** each refusing fixture returns its documented `Err` and writes nothing.
- **T5:** `docx-rs` read-back of `document.children` shows the new text in place of the run's
  pieces, and every other text unchanged.
- **T6:** child order of every edited parent satisfies the generated slots.
- **Manual:** the owner opens the outputs of
  `cargo run -p ooxml-docx --example docx_manual_check -- <outdir>` in Word and gets no repair dialog.
  This is an explicit gap, as in ADR-0001.

**Oracle independence:** T5 uses docx-rs (justification in ADR-0004 §7).

### Phase1: template filling

**Scope.**

- `Paragraph::replace` with the run-group rule (Q1).
- Story iteration over `header*`/`footer*`/`footnotes`/`endnotes`/`comments`, found via rels.
- Text in `w:tc`, `w:smartTag` (with an `add_child` override for `w:smartTag` in `CT_P`) and `w:fldSimple` results, still under the field gate.
- Direct run and paragraph properties (`b`, `i`, `u`, `color`, `sz`, `jc`, …; the `Option<bool>` on/off model).
- `pStyle`/`rStyle` set to an **existing** style ID, read from `styles.xml`, else `Err(UnknownStyle)`.
- `set_update_fields_on_open`.
- `Document::new()` from an embedded template (deferred from Phase0; template source, page size and East Asian font defaults to be decided by the owner).
- Possibly: an opt-in to edit enforced-protection documents (with `readOnly` + `w:permStart`/`w:permEnd` editable ranges, a common template pattern, as an input to that decision).
- Moving the last section's `sectPr` on append (lifting A2), as an explicit, documented edit.

**Planned direction for `StaleFieldRisk` (ADR-0004 open questions 2 and 3; recommendation of
`0004-opus-review-round2.md`, recorded in ADR-0004 revision 3, awaiting the owner's
confirmation).**

- **Per-operation check, limited.** A two-column table in `fields.rs` (code → allowed under
  `set_run_text` / under `append_paragraph`) replaces the single `SAFE` list, which is the
  "allowed under both" special case. Phase1 adds only `SEQ`, `TC`, `XE`, `NOTEREF`, `LISTNUM`,
  `AUTONUM`, `AUTONUMLGL`, `AUTONUMOUT` as allowed under both operations: their results depend
  only on the number and order of fields, the instruction text or note numbering, none of which
  Phase0's operations change. Tests enumerate the whole table.
- **`TOC`, `PAGEREF`, `REF` under append** are not added in this step: a default paragraph style
  with `w:outlineLvl` would put the appended paragraph into a `\u` TOC, and endnotes move after
  the appended paragraph, which can change a `PAGEREF` to them. They go with the `updateFields`
  opt-in or a later step.
- **Refused under `set_run_text`** in any case: `TOC`, `REF`, `PAGEREF`, `STYLEREF`, `=`, `IF`
  (unless the next rule applies), `NUMWORDS`, `NUMCHARS`, and `INFO`/`DOCPROPERTY` statistics.
- **`IF`** is safe when (a) every nested field is safe and (b) neither side of the comparison is a
  bare identifier (a bookmark reference: `{ IF bm = "x" … }`). A tokenizer over the instruction
  (quoted strings, numbers, operators, nested-field placeholders) decides (b). The mail-merge
  pattern `{ IF { MERGEFIELD G } = "F" "Ms." "Mr." }` passes. Note: in a mail-merge main document
  (`settings/w:mailMerge`), `IF` and `MERGEFIELD` are re-evaluated on every merge *(u)*.
- **`updateFields` opt-in** covers the rest: documents with `REF`/`TOC` or an `IF` that fails (a)
  or (b), where the caller accepts that Word updates the fields on open. It is never the default,
  because readers other than Word keep the stale results (ADR-0004 Alternatives 4).

**Acceptance.**

- Phase0 oracles extended to all story parts.
- Replace tests on `poi-bug59058` (38 words split across runs), `poi-table-alignment` and `sdk-Field-more-field`, including the refusal cases: crossing a hyperlink, a field or mixed formatting.
- docx-rs read-back of headers and footers.
- A python-docx cross-check on the paragraph texts python-docx can see. It is optional, run only if the CI image has Python.

### Phase2: structure

**Scope.**

- Registries: OPC part/rel/content-type (from the foundation `Package`), style IDs, numbering, annotation IDs, paraId, `wp:docPr/@id`, bookmark names.
- APIs: insert paragraph or run anywhere with properties; create tables (`tblGrid` consistent with cells, `gridSpan`/`vMerge`); lists (new `w:num` per list, `lvlOverride` restart); inline images (media part + rel + content-type `Default`); hyperlinks (external rel / anchor); bookmarks; section breaks; new header/footer parts; new styles (two seed modes as in ADR-0001: from scratch dedups by value, edit appends only).

**Acceptance.**

- Registry property tests: existing IDs never change.
- The new ID is absent from every part, including both AC branches.
- docx-rs read-back of tables, lists and images.
- `ooxmlsdk` (ADR-0002's suggested oracle) parses the output without error. It is evaluated when Phase2 starts.
- Owner check in Word.

### Phase3: review and semantics

**Scope.**

- Comments across all five parts: `commentsExtended` `paraId` = last paragraph's `w14:paraId`, `paraIdParent` for replies, `durableId` in `commentsIds` and `commentsExtensible`, and `people.xml`.
- Footnotes and endnotes: preserve the separators, allocate IDs.
- Tracked-change generation, plus "track this edit" mode when `trackRevisions` is on, which lifts that gate.
- SDT: set text of plain-text and date controls; for data-bound controls, edit the custom XML store item and the cached content together.
- Field insertion: PAGE, TOC + `updateFields`.
- Effective-formatting resolver: docDefaults → style chain → direct, with toggle XOR.

**Acceptance.**

- `sdk-Comment010` round-trip with a reply added (all 5 parts agree).
- Tracked edit on `poi-delins` with docx-rs read-back of `ins`/`del`.
- Resolver differential tests against a hand-computed table from the fixtures. LibreOffice is the reference where available; it is not available in this sandbox.

### Phase4: rich content

**Scope.**

- ADR-0002's AC-rank rule, which enables inserts next to `mc:AlternateContent`.
- Anchored and `wps`/`wpg` drawings.
- OMML typed lens (`m:` on/off overrides).
- Charts, shared with the Excel/PowerPoint DrawingML work.
- Legacy form fields, document protection, `altChunk` (needs a fixture), Strict read (namespace mapping).

**Acceptance:** defined per feature in its ADR.

## 5. Cross-part registries (Layer 3 analogue), summary

| Registry | Owner (proposed) | Keys | Phase | Rule |
|---|---|---|---|---|
| OPC content types / rels | `ooxml-core::Package` | part names, rIds | 2 | as ADR-0001 |
| Style IDs | `ooxml-docx::Styles` | `w:styleId` | 1 read, 2 append | never rename; refs to unknown IDs are Err |
| Numbering | `ooxml-docx::Numbering` | `numId`, `abstractNumId` | 2 | append-only; never reuse a freed ID |
| Annotation IDs | `ooxml-docx::Annotations` | `w:id` of bookmarks, comments, ins/del/move, perm | 2 | max+1 over the union of all story parts |
| Note IDs | `ooxml-docx::Notes` | footnote / endnote `w:id` (+ separator IDs in settings) | 3 | per kind |
| paraId / textId / durableId | `ooxml-docx::ParaIds` | `w14:paraId`, `w14:textId`, `w16cid:durableId` | 2/3 | random < 0x80000000, absent everywhere; never fix duplicates |
| Bookmark names | `ooxml-docx::Annotations` | `w:name` | 2 | unique (case rule to be confirmed) |
| Drawing IDs | `ooxml-docx::Drawings` | `wp:docPr/@id` | 2 | max+1 over all stories |
| rsids | (none) | `settings/w:rsids` | — | never created; Word adds its own |
| Custom XML store | `ooxml-docx::CustomXml` | `ds:itemID` ↔ `w:dataBinding/@storeItemID` | 3 | edits keep both sides in sync |

## 6. Sources

- Spike scripts and results: `spikes/docx/` (2026-11-16).
- Fixtures: `reference/fixtures/docx/README.md`.
- Apache POI test data and XWPF source: https://github.com/apache/poi (trunk `47dcff769bb395034dddfd59c4609659b6dbc6a4`): `poi-ooxml/src/main/java/org/apache/poi/xwpf/usermodel/` (`XWPFParagraph.buildRunsInOrderFromXml`, `searchText`; `XWPFDocument.enforceUpdateFields`, `createTOC`; `XWPFSettings`).
- Open XML SDK data and test assets: https://github.com/dotnet/Open-XML-SDK (`431ab05cf160248cc3885a4a766026d4f8243792`): `data/schemas/schemas_openxmlformats_org_wordprocessingml_2006_main.json`, `data/parts/MainDocumentPart.json`.
- ECMA-376 Transitional XSDs (as ADR-0002 §1.2): `wml.xsd` (`CT_OnOff`, `CT_Body`, `EG_BlockLevelElts`), `shared-commonSimpleTypes.xsd` (`ST_OnOff`).
- python-docx: https://github.com/python-openxml/python-docx (`e45454602b53e8e572b179ccf1c91093ec9f4ed7`): `HISTORY.rst`, `src/docx/oxml/text/paragraph.py`, `src/docx/oxml/text/run.py`.
- docx-rs: https://github.com/bokuweb/docx-rs (`3b959fa73e6ea78a3c96ab50416407b2c3b8fa7a`) and crate 0.4.22: `docx-core/src/reader/`, `docx-core/src/documents/elements/`.
- docx-rust: https://github.com/cstkingkey/docx-rs (`b3852d283780b8cd064d6304672759551fa5673d`) and crate 0.1.11.
- rdocx 0.17.0: https://github.com/tensorbee/rdocx (README only).
- ADR-0001, ADR-0002, `docs/research/2026-11-16-word-powerpoint-applicability.md`.
