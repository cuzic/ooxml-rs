# PowerPoint (`ooxml-pptx`) roadmap and gap analysis

> **要約（日本語）**
> ooxml-rs には PowerPoint の実装がまだ何も無い。本書は PowerPoint の機能面を
> 洗い出し、パート・要素・パートをまたぐ登録簿・難しさ・既存実装の対応状況を表にし、
> 「利用者にとっての価値 × リスク」で段階（Phase 0〜4）を決める。最初の段階
> （Phase 0、[ADR-0005](../adr/0005-pptx-phase0.md)）は **既存の図形のテキストの
> 置き換えと、既存のテキスト本体への段落・ランの追加だけ** で、新しいパートも ID も
> 作らない。プレースホルダーの継承は Phase 0 では読まず（直接の値だけを返す）、
> Phase 1 で「どこから来た値か」付きで解決する。スライドの追加は Phase 2 で、
> `sldIdLst`・リレーション・Content Types・パート名・セクションを
> 追記のみの登録簿として扱う。根拠の数値は Apache POI などの実ファイル 105 個を
> `spikes/pptx/` のスクリプトで数えたもの。

- Status: draft for review (milestone `pptx-plan-v0`; Phase 0 updated for
  ADR-0005 revision 2, milestone `pptx-phase0`)
- Date: 2026-11-16
- Depends on: [ADR-0001](../adr/0001-lossless-round-trip-architecture.md),
  [ADR-0002](../adr/0002-schema-driven-code-generation.md),
  [research memo](../research/2026-11-16-word-powerpoint-applicability.md)
- Phase 0 design: [ADR-0005](../adr/0005-pptx-phase0.md) (Proposed)
- Phase 1 design: [ADR-0008](../adr/0008-pptx-phase1-deck-reading.md)
  (Proposed; evidence `spikes/pptx-phase1/`, 633 files)

## 1. Evidence base

Every number below was produced by a script in [`spikes/pptx/`](../../spikes/pptx/README.md),
run with `python3 -I` on untrusted files (nothing in the files is executed):

| Corpus | Files | Licence | Where |
|---|---:|---|---|
| Apache POI `test-data/slideshow/*.pptx` (trunk, 2026-11-16) | 94 (83 readable; 11 are fuzz cases or not zips) | Apache-2.0 | not committed (except fixtures) |
| Open XML SDK `test/.../assets/TestFiles` (8 files) | 8 | MIT | not committed (except fixtures) |
| python-pptx `features/steps/test_files` (11 files) | 11 | MIT | not committed (except fixtures) |
| LibreOffice `sd/qa/unit/data/pptx` (3 files) | 3 | MPL-2.0 | not committed (except fixtures) |
| **Total readable** | **105** | | |

The committed subset is [`reference/fixtures/pptx/`](../../reference/fixtures/pptx/README.md)
(23 files, 2.0 MB). Schema data: Open XML SDK `data/` at `431ab05c` and the
ECMA-376 5th-edition Transitional XSDs, the same inputs as ADR-0002.

Corpus-wide feature counts (`survey.py`, slides only unless noted):

| Feature | Instances | Files (of 105) |
|---|---:|---:|
| slides | 546 | 103 |
| `p:sp` / `p:txBody` | 3,450 / 3,418 | 74 |
| paragraphs with text / with ≥ 2 runs or fields | 3,611 / 597 | 70 / 40 |
| placeholders on slides (`p:ph`) | 713 | 56 |
| `p:pic` / `p:grpSp` / `p:cxnSp` | 313 / 174 / 78 | 37 / 11 / 7 |
| tables (`a:tbl`) / charts / SmartArt (`graphicData` uri) | 45 / 30 / 19 | 21 / 13 / 7 |
| `a:fld` / `a:hlinkClick` | 106 / 87 | 10 / 11 |
| notes slides (parts) | 228 | 31 |
| legacy comments / modern (p188) comments (parts) | 2 / 1 | 1 / 1 |
| `p14:section` | 17 | 4 |
| `p:embeddedFont` | 3 | 1 |
| `p:timing` directly under `p:sld` | 279 | 29 |
| `mc:AlternateContent` directly under `p:sld` | 12 | 7 |
| `mc:AlternateContent` directly under `p:spTree` | 3 | 3 |

## 2. Feature surface vs. ooxml-rs

Status for ooxml-rs is "none" everywhere today; the column says **when** it is
planned. Difficulty: **L** low (lens + existing primitives), **M** medium
(cross-part reads or one registry), **H** high (several registries, caches or
MCE), **VH** (another format embedded, or a cache that only Office can
recompute).

Reference coverage: **python-pptx** 1.0.2 (verified by reading its source /
module list), **POI** = Apache POI XSLF trunk (verified by class list and the
named source files), **ppt-rs** 0.2.37 and other Rust crates: README /
crates.io description only, **unverified** (their mechanism, in particular
whether untouched content survives a save, was not read).

| # | Feature | Parts / key elements | Cross-part registries | Diff. | python-pptx | POI XSLF | Phase |
|---|---|---|---|---|---|---|---|
| 1 | Presentation & slide list | `presentation.xml` `p:sldIdLst/p:sldId@id,@r:id`, `sldSz`, `notesSz` | `sldId` (256 ≤ id < 2³¹, unique, never reused in practice), presentation rels | L read / H add | yes | yes | P0 read, P2 add |
| 2 | Slide → layout → master → theme chain | `slideLayoutN.xml`, `slideMasterN.xml` (`p:sldLayoutIdLst`, ids ≥ 2³¹), `theme/themeN.xml`, `p:clrMap`/`p:clrMapOvr` | one `slideLayout` rel per slide; master owns layouts; layout id space shared with master ids | M | yes | yes | P1 |
| 3 | Placeholders & inheritance | `p:nvPr/p:ph@type,@idx`, layout/master `p:ph`, `p:txStyles` (`titleStyle`, `bodyStyle`, `otherStyle`), `a:lstStyle`, `presentation.xml` `p:defaultTextStyle` | resolution across 3–4 parts (§3.1) | M read / H write | geometry only (`_InheritsDimensions`) | `getPlaceholder` (idx, then type) | P0 direct values, P1 resolved |
| 4 | Text bodies, paragraphs, runs | `p:txBody`/`a:txBody` → `a:bodyPr`, `a:lstStyle`, `a:p+` → `a:pPr`, (`a:r`, `a:br`, `a:fld`, `a14:m`)\*, `a:endParaRPr` | none (no ids) | L | yes | yes | **P0** |
| 5 | Fields | `a:fld@type` (`slidenum`, `datetime*`), cached `a:t` | cache recomputed by Office | M | read as text only (`runs` excludes fields) | yes | P0 read; writes Err |
| 6 | Autofit caches | `a:normAutofit@fontScale,@lnSpcReduction` (43 in corpus) | none; value is a layout cache | M | `fit_text` (computes its own) | — | P0 leaves stale (ADR-0005 §8) |
| 7 | Run/paragraph formatting | `a:rPr` (`sz`, `b`, `baseline` = `ST_Percentage`, fonts, fills), `a:pPr` (`lvl`, `algn`, `a:spcPct`, `a:buSzPct`) | theme fonts/colours by reference | M | yes | yes | P3 |
| 8 | Shapes, preset geometry | `p:sp/p:spPr/a:prstGeom@prst`, `a:avLst` | shape id per slide | L read / M add | yes | yes | P1 read, P2 add |
| 9 | Custom geometry | `a:custGeom/a:pathLst` | — | M | freeform builder | yes | P1 read, P4 write |
| 10 | Pictures | `p:pic/p:blipFill/a:blip@r:embed`, `media/imageN.*` | rId, content type `Default` by extension, media dedup | M | yes | yes | P2 |
| 11 | Group shapes | `p:grpSp` (`a:chOff`/`a:chExt` child coordinate space) | shape ids are per slide **including** group members | M | yes | yes | P0 walk, P2 add |
| 12 | Connectors | `p:cxnSp`, `a:stCxn@id`/`a:endCxn@id` | **reference shape ids** on the same slide | M | yes | yes | P1 read, P3 add |
| 13 | Tables | `p:graphicFrame/a:graphic/a:graphicData[@uri=…/table]/a:tbl` → `a:tblGrid/a:gridCol`, `a:tr/a:tc` (`gridSpan`, `rowSpan`, `hMerge`, `vMerge`) | `tableStyles.xml` style GUIDs | M text / H structure | yes | yes | **P0 cell text**, P3 structure |
| 14 | Charts | `graphicData[@uri=…/chart]/c:chart@r:id` → `charts/chartN.xml` (+ `style`, `colors`) → `embeddings/*.xlsx` | chart rels, embedded workbook, cached values in `c:numCache`/`c:strCache` | VH | yes (writes xlsx via XlsxWriter) | yes | P4 (via `ooxml-xlsx`) |
| 15 | Extended charts (Office 2016) | `cx:chart` inside `mc:AlternateContent` directly under `p:spTree`, picture fallback (`Of16-01.pptx`) | as charts + MCE | VH | no (shape invisible to it) | partial (unverified) | P4 |
| 16 | SmartArt | `graphicData[@uri=…/diagram]` → `dgm` data/layout/style/colors + `drawingN.xml` cache | 4–5 parts per diagram; drawing is a render cache | VH | no | read (`XSLFDiagram`) | P1b read text (ADR-0008 scope note), writes not planned |
| 17 | Notes / handouts | `notesSlides/notesSlideN.xml` (↔ slide, both directions), `notesMaster`, `handoutMaster` | notes ↔ slide rels, notes master | M | notes yes | notes yes | P1 edit existing, P3 create |
| 18 | Comments (legacy) | `comments/commentN.xml` `p:cm@authorId,@idx`, `commentAuthors.xml` `p:cmAuthor@id,@lastIdx` | author id + per-author `lastIdx` counter | M | no | yes (`XSLFComments`, legacy `CTComment` only) | P1 read, P4 write |
| 19 | Comments (modern, p188) | `comments/modernComment_*.xml` `p188:cm`, `authors.xml`; anchors `pc:sldMk@sldId,@cId`, `ac:spMk@id`, `ac:txMk@cp,@len` | **reference sldId, `p14:creationId`, shape id, character offsets** | H | no | no (legacy only) | P0 scan (Err), P1 read, P4 write |
| 20 | Transitions | `p:transition` (often `mc:AlternateContent` with `p14`/`p15` Choice) | none | M read / H write | no | no dedicated class | P1 read, P4 write |
| 21 | Timing / animations | `p:timing/p:tnLst`, `p:bldLst/p:bldP@spid,@build`, `p:spTgt@spid`, `p:txEl/p:pRg`, `p:charRg` | **reference shape ids and paragraph/character ranges** | H | no | no | P0 scan (Err), P4 |
| 22 | Media | `p:nvPr/a:videoFile`, `p14:media@r:embed`, `media/*` | two rels per media (link + embed), timing node | H | movie add | partial | P4 |
| 23 | Hyperlinks / actions | `a:hlinkClick@r:id,@action` (`ppaction://hlinksldjump` → slide rel) | external rels; slide-jump rels to other slides | M | yes (`action.py`) | yes | P1 read, P3 write |
| 24 | Sections (p14) | `p:presentation/p:extLst/p:ext/p14:sectionLst/p14:section/p14:sldIdLst/p14:sldId@id` | **reference sldIds** | M | no | no dedicated class | P1 read, P2 maintain |
| 25 | Custom shows | `p:custShowLst/p:custShow/p:sldLst/p:sld@r:id` | **presentation rels to slides** | M | no | no | P2 maintain (Err until then) |
| 26 | Embedded fonts | `p:embeddedFontLst`, `fonts/*.fntdata` (obfuscated) | font rels | H | no | `XSLFFontInfo`/`XSLFFontData` | passthrough only |
| 27 | OLE objects | `p:oleObj` in `graphicData`, `embeddings/*` | rels, often AC-wrapped | H | add OLE | yes | passthrough only |
| 28 | VBA (`.pptm`), custom XML, tags | `vbaProject.bin`, `customXml/*`, `tags/*` | rels | — | — | — | Layer 1 passthrough (P0) |
| 29 | Document properties | `docProps/app.xml` (`Slides`, `Notes`, `TitlesOfParts`), `core.xml` | counts duplicated from the deck | M | core only | yes | P2 (decide whether to update, §3.5) |
| 30 | From-scratch presentation | embedded template, same path as edit (ADR-0001) | all of the above at seed time | M | default template | yes | P2 |

Other Rust crates found on crates.io (2026-11-16; **all unverified**, claims
from their descriptions): `ppt-rs` 0.2.37 (create/read/update, "edit
round-trip", templates, editable charts), `pptx` 0.1.0 (create/manipulate),
`rust-pptx` 0.4.1 (parse PPTX/PPT into a model; read-only), `betteroffice-pptx-*`
0.5.1 ("bounded PresentationML parsing and part-preserving package
serialization"), `office_oxide` 0.1.17 (multi-format processing),
`msoffice_pptx` 0.2.1 (2019 deserializer), `ooxmlsdk` 0.15.0 (typed model of
every part; a loaded part is fully re-serialized, see ADR-0002 Alternative 1).
None was evaluated as a dependency; `ooxmlsdk` is the only one already read
(for ADR-0002) and is a test-oracle candidate in ADR-0005.

## 3. Answers to the design questions (with evidence)

### 3.1 Placeholder inheritance for reads

Resolution rule observed in the two reference implementations:

- python-pptx (`shapes/placeholder.py`): a **slide** placeholder inherits from
  the layout placeholder with the **same `idx`** (`layout.placeholders.get(idx=…)`);
  a **layout** placeholder inherits from the master placeholder of the
  **mapped type** (`ctrTitle→title`, `subTitle/obj/body/chart/tbl/pic/media/clipArt/dgm→body`,
  `dt`, `ftr`, `sldNum`, `title` unchanged). It resolves geometry only.
- POI (`XSLFSheet.getPlaceholder(CTPlaceholder)`): by `idx` if set, **then by
  type** if nothing matched.

Measured (`placeholders.py`, 711 slide placeholders resolved in 105 files; `survey.py` counts 713 `p:ph`; 2 slides were skipped because their layout or master could not be read):

| Observation | Count |
|---|---:|
| matched a layout placeholder by `idx` (absent `idx` = 0) | 687 |
| … of which the layout placeholder's `type` differs | 49 |
| matched only by `type` (POI's fallback) | 2 |
| no layout placeholder at all (orphan) | 22 |
| layout placeholder found the master placeholder by mapped type | 670 / 689 |
| slide placeholder carries its own `a:xfrm` | 239 (34 %) |
| runs inside slide placeholders with an explicit `sz` | 801 of 1,883 (43 %) |

So two thirds of placeholder positions and more than half of the font sizes
are **not in the slide part at all**. Decision (ADR-0005 §4, Phase 1):

1. Phase 0 reads return only values written **on the element itself**
   (`Option<T>`, `None` = "not set here"). It never returns an inherited value
   as if it were direct, and never writes inherited values into the slide
   (that would freeze them and break the layout link that PowerPoint keeps
   live).
2. Phase 1 adds resolvers that return `Resolved<T> { value, source }` where
   `source` names the part and element the value came from (slide, layout
   placeholder, master placeholder, master `txStyles`, `defaultTextStyle`,
   theme). Placeholder matching follows POI's order (idx, then type), and
   reports the 49 idx-match/type-mismatch cases as `source` so callers can
   see them. An orphan placeholder resolves only from the master by type, or
   returns `Err(UnresolvedPlaceholder)`; it never silently returns a default.
3. The text-style chain for a placeholder run is
   `a:rPr` → shape `a:lstStyle/a:lvlNpPr` → layout placeholder `a:lstStyle` →
   master placeholder `a:lstStyle` → master `p:txStyles` (`titleStyle` for
   title types, `bodyStyle` for body types, `otherStyle` otherwise) →
   theme fonts/colours. For **non-placeholder** shapes the chain after the
   shape's own `a:lstStyle` is `presentation.xml` `p:defaultTextStyle`; whether
   `otherStyle` also applies there is **not verified** and is settled by the
   Phase 1 oracle (open question Q6), not by assumption.
   *Phase 1 (ADR-0008 §1.2–1.3) refines this:* placeholder matching breaks
   idx ties (69 placeholders in layouts with several idx-0 placeholders, 633
   files) by type and family; the chain adds `a:pPr/a:defRPr` and the
   shape's own `p:style/a:fontRef` (placeholders included, rev 2); Q6 is proposed as
   "`defaultTextStyle`, then `otherStyle`" (POI) pending a PowerPoint check,
   with LibreOffice (`defaultTextStyle` only) and the ECMA note (`bodyStyle`)
   measured as alternatives.
4. Oracle for Phase 1: python-pptx's inherited `left/top/width/height` for
   geometry; for text properties a hand-checked expected table for
   `ph-inherit-props.pptx` and `placeholder-layout-color.pptx` plus a manual
   PowerPoint check. *Corrected in ADR-0008 rev 2 (review m10a):* POI does
   resolve text properties (`CharacterPropertyFetcher`) and is an optional
   differential oracle with a committed table of known differences.

### 3.2 `ST_Percentage` and DrawingML default mismatches (`set_type`)

`percent.py` lists **66** attributes in `dml-main.xsd`, `pml.xsd` and
`dml-chart.xsd` whose XSD type is a DrawingML percentage union
(`ST_PercentageDecimal` = int in 1/1000 % **or** `s:ST_Percentage` =
`"-?\d+(\.\d+)?%"`). The SDK types 61 of them as `Int32Value` and a few as
`UInt16/Byte/SByte/UInt32Value`. ADR-0002's `typecheck.py` found 39 XSD
defaults that do not fit the SDK type (25 `a`, 9 `c`, 5 `p`), all of the
`"100%"`/`"0%"` form.

What real files contain: across 105 files, **0** attribute values of these
types use the `"NN%"` form; every value is an integer (e.g. `a:spcPct@val`
5,547 times). `a:buSzPct@val` (XSD `ST_TextBulletSizePercent`, a **string-only**
`"NN%"` pattern in the 2016 Transitional XSD) appears 138 times, always as an
integer such as `100000`, i.e. the SDK's `Int32Value` matches Office and the
XSD does not. `p:presentation@serverZoom` (default `"50%"`) never appears.

Decision:

- `ooxml-core` provides one union simple type, `DmlPercent`
  (`Thousandths(i32)` | `Literal { value: f64, raw }` for the `%` form), as
  ADR-0002 §1.2 anticipated. Reads accept both forms; writes always emit the
  integer form, which is what Office writes.
- Every percentage attribute gets a `set_type { type = "DmlPercent" }`
  override **when its element enters the allowlist**, with this section and
  the corpus count as evidence. The XSD default is then valid and adopted
  (no `DefaultMismatch`).
- `a:buSzPct@val` gets `set_type` to `DmlPercent` too (SDK and files agree;
  the XSD pattern is the outlier), with a comment saying so.
- Phase 0 has **no** override (project-owner default, 2026-11-16, review
  m2): `p:presentation` is allowlisted read-only to walk `sldIdLst`, and
  its `serverZoom` default `"50%"` stays dropped with one `DefaultMismatch`
  line in `codegen/report.txt`; Phase 0 never reads `serverZoom`.
  `DmlPercent` and the `set_type` vocabulary are decided with Phase 3.
- ~~Phase 3 (formatting writes) brings in the rest that matter for text~~
  **Moved to Phase 1** (ADR-0008 §6), because Phase 1 reads these values:
  `DmlPercent` and `set_type` for `a:CT_TextCharacterProperties@baseline`,
  `a:CT_TextSpacingPercent@val`, `a:CT_TextBulletSizePercent@val`,
  `a:CT_TextNormalAutofit@fontScale`/`@lnSpcReduction`, `a:hslClr`
  `@sat`/`@lum`, `a:scrgbClr` `@r`/`@g`/`@b`
  and `p:presentation@serverZoom` (11 overrides, 3 defaults adopted,
  `DefaultMismatch` 39 → 36; `p:cTn` percentages are not read and stay
  dropped, ADR-0008 rev 2). Still 0 `"NN%"` values in 633 files.

### 3.3 Transitions wrapped in `mc:AlternateContent`

Direct `mc:AlternateContent` children in slide-family parts (105 files):

| Parent | Count | Files | Content |
|---|---:|---:|---|
| `p:sld` | 12 | 7 | always `p:transition`: `Choice Requires="p14"` (or `p15`, e.g. `p15:prstTrans prst="airplane"`) + `Fallback` with a plain transition |
| `p:sldLayout` / `p:sldMaster` | 4 / 2 | 2 / 1 | same |
| `p:spTree` | 3 | 3 | Office 2016 `cx:chart` graphic frame, `Fallback` = picture with the **same `cNvPr@id`** |
| `a:graphicData` (slides, layouts, masters) | 17 | ≤ 8 | OLE objects and similar |

ADR-0002 §4.3 refuses insertion into a parent with an AC child. Consequences:

- Phase 0 never inserts into `p:sld`, `p:spTree` or `a:graphicData`; its
  insert targets (`p:txBody`/`a:txBody`, `a:p`, `a:r`) had **no** AC child
  in any file (this corpus and the research memo's 197 files).
- Shapes **inside** an AC branch are read-only in Phase 0
  (`Err(InsideAlternateContent)`): editing one branch would desync the other,
  and readers pick different branches (python-pptx does not see the
  `Of16-01.pptx` chart at all).
- Shape-id bookkeeping (Phase 2) must count an AC as **one** shape: the two
  branches legitimately reuse id 7 in `Of16-01.pptx`.
- Reading a transition (Phase 1) follows ECMA-376 Part 3: take the first
  `mc:Choice` whose `Requires` prefixes are all understood, else
  `mc:Fallback`; the API reports which branch was used.
- Writing a transition or `p:timing` on such a slide (Phase 4) needs
  ADR-0002's "later rule" (AC rank = minimum rank of its first listed
  element, here `transition`), so a new `p:timing` lands **after** the AC,
  which is what `bug68703.pptx` shows PowerPoint writes. Both branches are
  written; the Fallback uses only base-namespace transition types.

### 3.4 Text edits across runs

Measured (`runs.py`, slide paragraphs in 105 files): 597 of 3,611 text
paragraphs (16.5 %) have two or more runs/fields. In 252 of those (42 %) the
runs differ only in proofing attributes (`lang`, `altLang`, `err`, `dirty`,
`smtClean`, `smtId`, `noProof`); 345 differ visibly. Within multi-run
paragraphs, 22 % of two-word windows and 30 % of three-word windows cross a
run boundary, so "find a phrase and replace it" will often span runs. 106
`a:fld` and 52 paragraphs with `a:br` occur. No `a:t` in 4,980 (`probe.py`) carries
`xml:space` (DrawingML's `a:t` has no `xml:space` attribute, ADR-0002's
`set_text` therefore never adds one), yet 1,074 have leading/trailing spaces (209 are whitespace only);
they are preserved because XML parsers keep text content as is.

Reference behaviour: python-pptx `_Paragraph.text = s` clears all runs and
appends **new runs without `a:rPr`** (`\n`/`\v` become `a:br`), so visible
formatting is lost; `TextFrame.text = s` clears all paragraphs.

Decision (ADR-0005 §4): Phase 0 offers run-level `set_text` (exact), and
paragraph-level `set_text` that **keeps the first run's `a:rPr`** and removes
the rest (Err if the paragraph contains an `a:fld`). Cross-run find/replace is
Phase 1, defined on the concatenated paragraph text: the replacement goes
into the run where the match starts, fully covered runs are removed, partly
covered runs are trimmed, and a match that covers an `a:fld` or `a:br` is an
`Err`. ~~An optional "merge proofing-only splits first" step~~ Dropped in
ADR-0008 §4: the start-run rule already yields the visible format, and a
merge would rewrite runs the match does not touch; a `MixedFormat::Refuse`
option refuses matches across visibly different runs instead.

### 3.5 What "add slide" needs (Phase 2)

From the OPC/PML rules and the two reference implementations
(python-pptx `parts/presentation.py`, `slide.py`, `shapes/shapetree.py`;
POI `XMLSlideShow.createSlide`):

1. **Layout**: a `slideLayout` part reachable from a master listed in
   `p:sldMasterIdLst`.
2. **Part name**: an unused `/ppt/slides/slideN.xml`. POI picks an unused index
   (`getUnusedPartIndex`, bug 55791). python-pptx instead **renames every
   slide part** to `slide1..N` when `Presentation.slides` is accessed
   (`rename_slide_parts`); observed on `airplane-transition.pptx`, whose only
   slide part is `slide2.xml` (`ppy_add_slide_check.py`). ooxml-rs cannot
   rename parts (ADR-0001 Layer 1), so it uses POI's rule.
3. **Content type**: an `Override` for the new part
   (`application/vnd.openxmlformats-officedocument.presentationml.slide+xml`).
4. **Slide rels**: one `slideLayout` relationship.
5. **Presentation rels**: a new unique `rId`.
6. **`p:sldId`**: `id = max(existing, 255) + 1` (both libraries), searching
   for a gap only if that exceeds 2³¹−1 (python-pptx); `r:id` = the new rId;
   appended to `sldIdLst` (or inserted at a position for "insert at").
7. **Slide XML**: `p:cSld/p:spTree` with `nvGrpSpPr`/`grpSpPr` (id 1),
   placeholders cloned from the layout (python-pptx copies only
   `type`/`orient`/`sz`/`idx`, no `spPr`, ids `max+1`, and skips `dt`/`ftr`/`sldNum`),
   `p:clrMapOvr/a:masterClrMapping`.
8. **Sections**: if `p14:sectionLst` exists (4 files), the new `sldId` must be
   added to a section. **Not verified** what PowerPoint does with a slide in
   no section; Phase 2 adds it to the section of the preceding slide.
9. **Custom shows / slide-jump hyperlinks / modern comments** reference slides
   by rId or `sldId`; adding is safe, **deleting or reordering** must update
   or refuse them.
10. **`docProps/app.xml`** (`Slides`, `TitlesOfParts`): neither library
    updates it (python-pptx has no `app.xml` handling at all). Whether stale
    counts matter to PowerPoint is **unverified**; Phase 2 decides with a
    manual check.

These are ADR-0001 Layer 3 registries: OPC ones (`[Content_Types].xml`,
rels) owned by `ooxml-core::Package`; PML ones (`sldId`, slide part index,
per-slide shape ids, section membership) owned by `ooxml-pptx`. The
append-only invariant carries over: existing `sldId`s, part names and shape
ids are **never** renumbered, because modern comments
(`pc:sldMk@sldId`, `ac:spMk@id`), animations (`p:spTgt@spid`), connectors
(`a:stCxn@id`) and sections point at them.

### 3.6 Cross-part registries (summary)

| Registry | Scope | Referenced from | Notes from the corpus |
|---|---|---|---|
| `p:sldId@id` | presentation | `p14:sectionLst`, modern comments `pc:sldMk@sldId`, `p:custShow` (via rId) | 256-based; gaps occur (`slide-sections.pptx`: 309…404) |
| slide/master/layout ids (`p:sldMasterId`, `p:sldLayoutId` ≥ 2³¹) | presentation + masters | — | shared id space |
| relationship ids | per source part | `r:id`, `r:embed`, `r:link` in XML | |
| content types | package | — | `Default` by extension for media |
| shape ids (`p:cNvPr@id`) | per slide part | `p:spTgt`, `p:bldP@spid`, `a:stCxn`/`a:endCxn`, `ac:spMk`, `p:txEl` | no duplicate shape ids outside AC in 105 files (the id 0 ×3 in `bug62513.pptx` slide 17 sits inside OLE fallbacks in `a:graphicData`, not on shapes); id 7 twice across AC branches in `Of16-01.pptx`; reads must tolerate, writes must allocate `max+1` |
| `p14:creationId` | per slide | modern comments `pc:sldMk@cId` | optional |
| comment authors | `commentAuthors.xml` (`@id`, `@lastIdx`) / `authors.xml` (GUID) | `p:cm@authorId,@idx` / `p188:cm@authorId` | |
| text positions | per shape | `p:txEl/p:pRg`, `p:charRg`, `ac:txMk@cp,@len` | 97 `p:pRg` in 6 files; Phase 0 refuses edits that would shift them |

## 4. Phased goals

Priority = user value × (1 / risk). Each phase lists acceptance criteria and
its oracles; "T1/T1b/T2" are ADR-0001's oracles, applied to slide parts.

### Phase 0 — edit text in place (ADR-0005)

Value: the most common automation ("fill this template", "translate these
slides", "fix a typo in 40 decks"). Risk: low; no new parts or ids.

Scope: open/save any `.pptx`/`.pptm`/`.ppsx`/`.potx` losslessly; list slides
in presentation order; walk shapes (groups, table cells); read text (runs,
breaks, cached field text) and direct placeholder `type`/`idx`; replace run
text; set paragraph text (first run keeps its format); set body text
(existing paragraphs reused); add paragraphs and runs to an **existing**
text body; remove runs/paragraphs (not the last) and fields (explicitly);
`Err` for removing fields implicitly, AC-wrapped shapes, merged table cells,
animated or comment-anchored text, unsupported paragraph content, format
sources that need a subtree copy, and characters XML cannot hold.

Acceptance (ADR-0005 rev 2 §7): all 23 fixtures open and save with zero
edits byte-identical per part (ADR-0001 Layer 1); T1/T1b on every slide
part; T2 locality for every run, paragraph and body in every fixture; T3:
python-pptx's read of every edited file equals the expected model
`apply(edits, python-pptx(input))` and ooxml-pptx's own read; every `Err`
case has a real or documented synthetic input; manual check in PowerPoint
(`examples/manual_check.rs`).

Implemented in `crates/ooxml-pptx` (milestone `pptx-phase0`).

### Phase 1 — read the whole deck (ADR-0008, Proposed)

Value: inspection, extraction, search, QA tools. Risk: medium (inheritance
semantics). Design: [ADR-0008](../adr/0008-pptx-phase1-deck-reading.md);
evidence `spikes/pptx-phase1/` (633 files: this roadmap's 105, 417 more
LibreOffice test decks, 100 ShapeCrawler test decks; 619 readable).

Scope:

- slide → layout → master → theme chain (notes → notes master); every
  resolved value is `Resolved { value, source }` naming the part and step
  (run, paragraph default, shape list style, shape style `fontRef`,
  layout/master placeholder with its match kind, master `txStyles`,
  `defaultTextStyle`); text, paragraph, body properties and geometry;
  theme fonts (`+mj`/`+mn`) and scheme colours through
  `clrMap`/`clrMapOvr` (transforms listed, not applied);
- placeholder matching: idx first, ties and implicit idx 0 decided by type
  and family, then type, then orphan → master;
- notes: read, and edit existing notes bodies with the Phase 0/0.1 text API
  and rules (creating notes stays Phase 3);
- read hyperlinks/actions, p14 sections, legacy and p188 comments,
  transitions (MCE branch: first Choice in `p14`/`p15`, else Fallback),
  timing summary;
- cross-run find/replace on slides (and optionally notes): start run keeps
  its format, fields/breaks inside a match refused, plan-then-apply;
- 124 read allowlist entries; `DmlPercent` with 11 `set_type` overrides;
- values are *as specified* (autofit scale, hyperlink colour and East
  Asian theme fonts are exposed as inputs or `Unresolved`, not applied).

Moved out (follow-up "Phase 1b" ADR): SmartArt text read, table structure
beyond Phase 0's cell reads.

Acceptance (ADR-0008 §7): geometry equals python-pptx's for every fixture
placeholder except a committed, hand-checked table of deliberate
differences (preview 1,266 / 1,272 equal); text properties equal committed
expected tables for synthetic one-rule decks and two real fixtures; links
equal python-pptx's `click_action`/`hyperlink`; notes edits pass
L1/T1/T1b/T2/T3/T4; find/replace keeps invariants T (text = `str.replace`)
and F (per-character format) on every fixture paragraph and agrees with an
independent lxml implementation; reading everything leaves the package
byte-identical; owner checks M1–M5 in PowerPoint.

### Phase 2 — slides and shapes as units

Value: generate decks, reorder/delete slides, add text boxes/pictures.
Risk: high (Layer 3). Scope: PML registries (§3.5, §3.6); add slide from
layout, delete, reorder (with sections, custom shows, slide-jump links,
modern comments kept consistent or `Err`); add text box/autoshape/picture
(media dedup by hash); `Presentation::new()` from an embedded template
through the same edit path (ADR-0001 From-scratch path).

Acceptance: python-pptx and POI (if available) open every output and agree
on slide order, ids and shape ids; ooxmlsdk parses every part; a manual
PowerPoint open-without-repair check on a recorded sample; no existing id or
part name changes (diff check).

### Phase 3 — formatting, tables, links, notes creation

Scope: `a:rPr`/`a:pPr` writes (with `DmlPercent` `set_type`s), table
row/column add/remove and merge, hyperlink writes (rId registry), creating
notes slides, connectors.

### Phase 4 — rich objects

Scope (each likely its own ADR): charts (chart part + embedded workbook via
`ooxml-xlsx`, caches), transitions/animations editing (AC rank rule),
modern comment writes (author registry, GUIDs), media, extended charts.
SmartArt and embedded-font **writes** are not planned (render caches /
obfuscated fonts); they stay Layer 1 passthrough.

## 5. Risks

- **No PowerPoint in the sandbox.** As for Excel (ADR-0001), "opens without
  a repair dialog" is a manual check by the owner; the automated oracles
  prove consistency and agreement with independent readers only.
- **Stale caches.** Field text, autofit scale, SmartArt drawings and chart
  caches are computed by Office. Phase 0 refuses field edits and leaves
  autofit as is (cosmetic; PowerPoint recomputes on the next edit of that box
  — **unverified**).
- **Placeholder semantics** are only partially specified in prose; Phase 1
  relies on reference implementations plus manual checks, and exposes the
  `source` of every resolved value so mistakes are visible.
