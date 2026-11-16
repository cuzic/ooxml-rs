# ADR-0013: PowerPoint Phase 1b — table structure and SmartArt text

- Status: **Proposed** (revision 2)
- Date: 2026-11-16
- Depends on: [ADR-0001](0001-lossless-round-trip-architecture.md),
  [ADR-0002](0002-schema-driven-code-generation.md) (lens, allowlist and
  `[detach]`, post-acceptance notes "detach/insert" and "parser depth
  limit"), [ADR-0005](0005-pptx-phase0.md) (Phase 0 text edits, references
  into text, refusals; revision 4 §9 "Phase 0.1 — subtree copy of format
  sources", PR #19), ADR-0008 (Phase 1, PR #28, Proposed), which moved
  both topics here.
- Evidence: [`spikes/pptx-phase1b/`](../../spikes/pptx-phase1b/README.md)
  (`tables_smartart.py`; 619 distinct presentations from the ADR-0008
  corpora: POI, Open XML SDK, python-pptx, LibreOffice `sd/qa/unit/data/pptx`,
  ShapeCrawler); python-pptx 1.0.2, Apache POI `47dcff76` and LibreOffice
  `2e885d0b` source, cited in §6.

- Review: [round 1](0013-opus-review-round1.md)

## Revision history

- **Revision 1:** initial draft.
- **Revision 2** (after `0013-opus-review-round1.md`, not converged: B1,
  B2, M1–M4, m1–m8):

  | Finding | Change | Where |
  |---|---|---|
  | B1 2-D merges use PowerPoint's edge form | the **edge form is canonical**: covered cells in the origin's row carry `rowSpan`, in its column `gridSpan`, with the origin's values; span arithmetic applies to the origin and its edge cells; new covered cells copy their neighbour's merge attributes; well-formedness checks edge spans (all 3 corpus 2-D merges are edge form, from PowerPoint) | §2 |
  | B2 comment anchors and structure edits | structural operations on a comment-locked frame are `CommentAnchoredTable`; cell properties allowed | §2, §5, §7 |
  | M1 `a16:rowId`/`colId` creation | needs a core primitive declaring a namespace on a **created** element (shared need with ADR-0011 §1.3; requested from the core PR on `core/add-part`); until then insertions into id-bearing tables are `RowIdNotSupported`; ids: unused in the table, not deleted in this session, `max + 1` else the smallest unused above 0 (values up to `u32::MAX`; 32 tables have ids ≥ 2³¹) | §2 |
  | M2 copy primitive coverage | explicit requirements: `[detach]` for `a:tcPr`, `a:bodyPr`, `a:lstStyle` (ADR-0002 note), and a same-part move of whole `a:p` subtrees for `merge` (core) | §2 |
  | M3 SmartArt formatting sources | format sources per **slice** (the node's own paragraphs in the shape); empty slices; `dgm:t` missing = one empty paragraph; `destOrd` ties refused; run-level invariant (counts equal, data attributes ⊆ cache attributes: 429/432 paragraphs, 431/431 runs) and index-mapped run edits | §4 |
  | M4 shared borders | edge-level API (`set_border(edge, …)` writes both cells of an inner edge; whole outline of a merged range) | §3 |
  | m1 Q1 evidence | Microsoft Q&A 912190 ("drawing#.xml is mostly meant to be a cache…"); fake-cache manual check | Context, Q1 |
  | m2 `drop_drawing_cache` | steps and refusals stated (drawing parts with own relationships, the slide-relative `relId`, empty `dgm:extLst`) | §4 |
  | m3 built-in styles | the closed list of built-in GUIDs (LibreOffice's 74) allowed, like `@def`; `set_style(None)` | §3 |
  | m4 empty origin on merge | a single empty paragraph in the origin is replaced (python-pptx `append_ps_from`) | §2 |
  | m5 spike | full well-formedness check (1 of 119 not well-formed: a short row); borders; run invariant; corpus pinned by SHA-256 | spike |
  | m6 animation locks | table timing targets whole frames; SmartArt node animations (`p:graphicEl/a:dgm@id`) are unaffected by text edits: no lock | §4 |
  | m7 frame size | graphic frames in groups: group extents not updated; `cy` may stay larger than the text-grown rows | §2 |
  | m8 placeholders | `phldr` not restored on empty text; `dgm:t` created with `a:bodyPr`/`a:lstStyle` | §4 |
  | Q1–Q4 | the review's recommendations recorded as proposed defaults | Open questions |

## Context

Phase 0 reads tables (`TableRef`/`CellRef`: grid, spans, merges) and edits
the text of non-merged cells; ADR-0008 (Phase 1) resolves inherited text
formatting but explicitly leaves **table structure** and **SmartArt text**
to this ADR. Both are edits inside one slide's part graph; neither needs
the slide-level registries of Phase 2 (ADR-0012), but both have
structure that the text edits of Phase 0 do not touch: the table grid and
its merge flags, and the SmartArt data model with its cached drawing.

Corpus (`tables_smartart.py`, 619 distinct presentations, 445 duplicates
skipped, 12 unreadable; output committed in `results/`):

| Topic | Finding |
|---|---|
| tables | 119 tables in 69 files, 1,561 cells; 1–2 rows in 63, ≤ 3 columns in 99 |
| merges | 32 tables (gridSpan > 1: 56 cells, rowSpan > 1: 22, hMerge: 166, vMerge: 32); with the full rev-2 well-formedness check (§2) **118 of 119 tables are well-formed**, 1 is not (`LIBRE_OFFICE-100610-0.pptx`: a short row); covered cells with text: 1; **all three 2-D merges** (gridSpan and rowSpan > 1) use the **edge form** (covered cells in the origin's row carry `rowSpan`, in its column `gridSpan`: 3 vMerge+gridSpan, 4 hMerge+rowSpan cells), all from PowerPoint (`tbl-cell.pptx`, `sc066-001.pptx`); 1-D merges cannot show the form |
| grid | one table has a row whose `a:tc` count ≠ `gridCol` count |
| frame size | `p:xfrm/a:ext@cx` = Σ `gridCol@w` in 113/119; `@cy` = Σ `tr@h` in 81 (rows grow with text in 38) |
| row/column ids | `a16:rowId` / `a16:colId` (Office 2016+) in **50 of 119** tables, declared on the element itself (`<a16:rowId xmlns:a16="…" val="…"/>`; on `p:sld` in 0 of 33 slides, review M1); values look random (`2407265832`), **32 tables have an id ≥ 2³¹**; modern comments anchor table cells, rows and columns by them (`ac:tcMk`, `trMk`, `gridColMk`) |
| table styles | `tableStyleId` in 92 tables: 84 defined in `ppt/tableStyles.xml`, 8 built-in GUIDs not defined there (PowerPoint and LibreOffice both know built-ins: LibreOffice's `CreateTableStyle` builds them from `predefined-table-styles.cxx`); `tableStyles.xml@def` is `{5C22544A-…}` (Medium Style 2 – Accent 1) in 551 files; flags `firstRow` 93, `bandRow` 92 |
| cell properties | `tcPr` attributes `marL/marR` ~716, `marT/marB` 573, `anchor` 376, `vert` 10; children `solidFill` 617, `lnL/R/T/B` ~575 each, `lnTlToBr`/`lnBlToTr` 56, `noFill` 32; `extLst`/`r:*` in `tcPr`: 0 |
| shared borders | the right line of a cell vs the left line of its neighbour: equal 423, one side only 41, different 2; bottom vs top: equal 427, one side only 134, different 3 (PowerPoint-made tables: 0 different) |
| SmartArt | 105 frames in 82 files; layouts `default` 14, `hierarchy4` 10, `orgChart1` 6, … (25+ distinct); points `pres` 1,953, `parTrans`/`sibTrans` 650 each, `node` 646, `doc` 105, `asst` 3; connections `presParOf` 1,848, `presOf` 992, `parOf` 650 |
| SmartArt text | 649 data nodes (`node`/`asst`), 596 with text; 677 data runs, 155 with `a:rPr` children (format copy needed); 26 paragraphs with > 1 run, 0 with fields; placeholder nodes (`prSet@phldr="1"`) 37, **never with text** (nodes with text: `phldr` absent 555, `"0"` 39) |
| drawing cache | 76 frames have `dsp:dataModelExt` → `diagramDrawing` part; 61 non-empty, **15 empty** (LibreOffice test files: no shapes); 26 have no cache; 3 point at a relationship the slide lacks |
| cache consistency | of 326 drawing shapes with text, **321 show exactly the concatenation of the paragraphs of the data nodes whose `presOf` connection points at them**, ordered by `destOrd` (16 of them aggregate > 1 node, e.g. a parent and its children as bullets; their paragraph levels `0,1,1`, `1,1,1`, …); 5 do not (one file, `Segmented_Cycle_oneColor.pptx`); per data node with text in a non-empty cache: 305 shown 1:1, 50 as part of an aggregate, 1 not found (a lone NBSP); `destOrd` ties: 0; nodes without `dgm:t`: 12 |
| run invariant | for paired paragraphs (data ↔ cache): equal run counts in 429 of 432; for paired runs, the data run's `a:rPr` attributes (proofing ones aside) appear with the same values in the cache run in **431 of 431**; the cache adds layout-computed `sz` |

What consumers do with the drawing cache (source-verified):

- **LibreOffice** (`oox/source/drawingml/diagram/diagram.cxx` `loadDiagram`):
  imports data, layout, styles and colours, counts the shapes of each
  `dataModelExt` drawing (`DiagramShapeCounter`) and, when a non-empty
  drawing exists, **renders the drawing**; only without one does it run
  its own layout engine (`bCreate`). On export after an edit it rewrites
  the drawing from its model (`writeDiagramOOXDrawing`).
- **POI** (`XSLFDiagram`): reads only the drawing (`readDiagramDrawing`,
  found by renaming `data#` to `drawing#`) and exposes it as a group
  shape; its Javadoc: rendering from the data model "is not trivial … for
  loops, if/elses"; no editing.
- **python-pptx**: SmartArt is an opaque graphic frame (no API).
- **PowerPoint**: whether it re-lays out from the data model on open or
  trusts the cache is **not** documented in ECMA-376 (the drawing part is
  a Microsoft extension, `http://schemas.microsoft.com/office/drawing/2008/diagram`).
  A Microsoft Open Specifications answer (Microsoft Q&A 912190) calls
  `drawing#.xml` "mostly meant to be a cache of the generated/evaluated
  layout … overwritten on any recomputation", and a third-party report
  (Aspose forum 65095) saw PowerPoint show edited data text while a
  cache-based renderer showed the old text: PowerPoint most likely lays
  out from the data. Owner manual check Q1 confirms it with a fake cache.

So a SmartArt text edit that leaves the cache stale shows the old text in
LibreOffice and POI, and possibly in PowerPoint; the cache must be edited
with the data or explicitly dropped.

## Decision

### 1. Scope

**Tables (existing `a:tbl` in a `p:graphicFrame` outside AC):**
insert/delete rows and columns, merge/unmerge rectangular ranges, set
column widths and row heights, cell properties (margins, anchor,
vertical text, solid/no fill, the four edge borders as solid lines), and
the table style reference/flags. **SmartArt:** edit the text of existing
data nodes (`node`, `asst`), keeping the drawing cache consistent.

**Not in Phase 1b:** creating tables (Phase 2/3), table style
definitions (adding a style to `tableStyles.xml`: Phase 3), diagonal
borders, gradient/pattern/picture fills, adding/removing/moving SmartArt
nodes or changing its layout/colours (needs a layout engine), SmartArt
`doc` text, recomputing autofit sizes in the cache.

### 2. Table structure

**Model.** A `rows × gridCol` grid; each `a:tc` occupies one grid slot. A
**merge** is an origin cell with `gridSpan`/`rowSpan` and the other cells
of its rectangle carrying `hMerge` (cells not in the origin's column)
and/or `vMerge` (cells not in the origin's row). **Canonical form (review
B1): the edge form.** In a 2-D merge (both spans > 1), the covered cells
in the origin's row also carry `rowSpan` equal to the origin's, and the
covered cells in the origin's column carry `gridSpan` equal to the
origin's — what PowerPoint writes (all 3 corpus 2-D merges, from
PowerPoint; python-pptx's `_Cell.merge` and LibreOffice's export write the
same). In a 1-D merge the covered cells carry no span (both forms
coincide). Interior cells of a 2-D merge carry both flags and no span.

**Well-formed** (checked before any structural edit, else
`TableNotWellFormed`; 1 of 119 corpus tables fails, a short row):
every row has as many `a:tc` as `gridCol`; every origin's rectangle is
inside the grid; every other cell of it is covered by exactly that origin,
with `hMerge` iff not in the origin's column and `vMerge` iff not in its
row; no flag outside a rectangle; in each 2-D merge the edge spans are
either all present with the origin's values or all absent (a 2-D merge in
flag-only form is read and edited as found, converted to the edge form by
any operation that touches it); mixing is not well-formed.

**Span arithmetic** applies to the origin **and its edge cells** together;
a newly created covered cell copies the merge attributes of its neighbour
in the same merge (the cell above for a new row, left for a new column),
and edge spans are recomputed from the origin's.

| Operation | Rule |
|---|---|
| `insert_row(at)` | the new `a:tr` copies `@h` from the row above (`at − 1`, else the row below); each new `a:tc` copies the cell above's (else below's) `a:tcPr`, and gets a text body with that cell's `a:bodyPr`/`a:lstStyle` copies and one empty paragraph whose `a:pPr`/`a:endParaRPr` copy that cell's first paragraph (ADR-0005 §3 format sources); `p:xfrm` `cy` grows by `@h`. **Inside a vertical merge** (strictly after its origin row, up to its last covered row) the new cells in the merged columns are covered (attributes copied from the cell above, so `vMerge`, plus `hMerge`/`gridSpan` as that cell has them) and the origin's `rowSpan` — and the edge cells' — grow by 1; at a merge's boundary the new row is outside it. |
| `delete_row(r)` | removes the `a:tr`; `cy` shrinks by its `@h` (it may stay larger than the text-grown rows; PowerPoint corrects it on open, review m7); a covered row shrinks its merge (origin and edge `rowSpan − 1`; a 2-D merge reduced to one row drops its edge `rowSpan`s); a row holding the **origin** of a merge with `rowSpan > 1` is refused (`MergeOriginInRow`: unmerge first). Never the last row. |
| `insert_column(at)`, `delete_column(c)` | the same with `a:gridCol` (`@w` copied from the column left, else right; `cx` adjusted), `gridSpan`, `hMerge`; `MergeOriginInColumn`; never the last column. |
| `merge(r1, c1, r2, c2)` | refused when the rectangle contains any merged cell (python-pptx's rule) or leaves the grid. Written in the edge form. The **paragraphs of the other cells move to the origin** in row-major order, empty cells skipped; when the origin holds a single empty paragraph it is replaced, not kept (python-pptx `append_ps_from`, review m4); each covered cell keeps one empty paragraph (its first paragraph's `a:pPr`/`a:endParaRPr`). |
| `unmerge(r, c)` | on an origin: spans (origin and edges) and flags removed; text stays in the origin (python-pptx `split`). |
| `set_column_width(c, w)`, `set_row_height(r, h)` | `gridCol@w` / `tr@h`, `p:xfrm` `cx`/`cy` adjusted by the difference. |

A graphic frame inside a group: the frame's `a:ext` changes, the group's
`a:ext`/`a:chExt` are **not** recomputed (ADR-0016 §6's rule; review m7).

**Comment anchors (review B2).** ADR-0005 §6's comment lock refuses only
*text* edits. Structural operations — `insert_*`, `delete_*`, `merge`,
`unmerge` — on a graphic frame that §6 locks for comments (an anchor
naming the frame, a cell, row or column of it: `ac:tcMkLst`, `trMkLst`,
`gridColMkLst`, or an unverifiable anchor) are refused with
`CommentAnchoredTable { shape_id }`: deleting an anchored row or column
leaves the anchor dangling, and moving paragraphs invalidates
`txMk@cp/len`. Cell properties, widths and heights are allowed (they move
no anchor). A finer rule (only rows/columns whose ids no anchor names) can
come later, provided every `tcMk`/`trMk`/`gridColMk` resolves.

**Row and column ids (review M1).** When the table carries
`a16:rowId`/`a16:colId` (50 of 119), a new row/column needs one, and
PowerPoint declares `a16` on that element itself (0 of 33 slides declare
it on `p:sld`). Creating it needs a core primitive that **declares a
namespace on a created element** (`declare_namespace(prefix, uri)` on a
node created in the same edit; refused on original nodes) — the same need
as ADR-0011 §1.3, requested from the core PR on `core/add-part` as one
ADR-0002 addition, with the allowed URIs listed (here
`http://schemas.microsoft.com/office/drawing/2014/main`). Until it exists,
`insert_row`/`insert_column` on such a table are `RowIdNotSupported`
(deletion, merge and properties are unaffected). The new id is unused in
the table and not deleted earlier in the session: `max + 1` when that
fits `u32` (`xsd:unsignedInt`), else the smallest unused value ≥ 1 (32
tables already hold ids ≥ 2³¹; PowerPoint's ids look random). A table
where only some rows/columns carry ids is `TableNotWellFormed`.

**Copies and moves (review M2).** The cell copies above need
`Detachable` for `a:tcPr`, `a:bodyPr` and `a:lstStyle` — an addition to
`dml.toml`'s `[detach]` reviewed like ADR-0002's note "detach/insert"
(corpus `a:tcPr`: no `extLst`, no `r:*`; their unique attributes, if any,
listed in `unique_attrs`). `merge` **moves** whole `a:p` subtrees inside
one part; a copy would wrongly refuse links (`r:id`), `smtId` and fields
that a move keeps unique. It needs a core `move_subtree(from, to)` on
`LensMut` (same part; namespaces and depth checked at the destination
before the change) — requested with the core PR. Until both exist,
`insert_*` and `merge` are not implemented (the ADR is not accepted
without them).

**Atomicity.** Plan → validate → apply as in ADR-0005: the
well-formedness check, the merge arithmetic, every copy and move check
(`check_insert_under` with depth) and the frame size are computed before
the first change; a refusal leaves the package byte-identical.

### 3. Cell properties and table style

On `TableMut`/`CellMut` (typed setters on `a:tcPr`, `a:tblPr`; the
allowlist moves `a:tcPr`, `a:tblPr`, `a:tableStyleId`, `a:lnL/R/T/B`,
`a:solidFill`, `a:noFill`, `a:srgbClr`, `a:schemeClr` (read today) to
`[write]` in `dml.toml`):

- margins `marL/marR/marT/marB` (EMU), `anchor` (`t/ctr/b`), `anchorCtr`,
  `vert`, `horzOverflow`;
- fill: `solidFill` with `srgbClr` or `schemeClr`, or `noFill` (replaces
  any existing fill child of `a:tcPr`, which is the one fill choice of the
  schema);
- **borders by edge** (review M4): `set_border(row, col, Edge::Left |
  Right | Top | Bottom, Line)` with `Line` = width (EMU) and solid colour,
  or `noFill`. An inner edge is shared: the call writes both cells' lines
  (`lnR` of the left cell and `lnL` of the right one; `lnB`/`lnT`) to the
  same value, as PowerPoint does (0 different shared edges in
  PowerPoint-made tables; 5 in other producers' tables are read as
  found). On a merged cell the edge means the merged range's outline, and
  every cell along it (covered ones included) gets the line. An existing
  `a:ln*` is replaced as a whole. `table.set_borders(Line)` sets all
  edges. Diagonals are not written;
- table style: `set_style(Some(id))` to a `styleId` defined in
  `ppt/tableStyles.xml`, or to one of PowerPoint's **built-in** style ids
  (the closed list of 74 that LibreOffice ships in
  `predefined-table-styles.cxx`, which includes the usual `@def`), else
  `TableStyleNotDefined` (adding a definition: ADR-0016 §9, Phase 3); a
  built-in id without a definition renders in PowerPoint and LibreOffice,
  not in POI — the same as `@def`, which most files leave undefined
  (review m3). `set_style(None)` removes `a:tableStyleId`. The `tblPr`
  flags `firstRow`, `firstCol`, `lastRow`, `lastCol`, `bandRow`,
  `bandCol`.

Reading resolved table-style formatting stays out (ADR-0008:
`Unresolved::TableStyle`).

### 4. SmartArt text

**Model.** `ShapeRef::smart_art()` returns, for a graphic frame with
`graphicData@uri = …/diagram`, the data part (`dgm:relIds@r:dm`) as a
tree of data points: the `doc` point, then `node`/`asst` points ordered
by their `parOf` connections (`srcOrd`). Each node exposes its text body
(`dgm:pt/dgm:t`, a `CT_TextBody`) with the Phase 0/1 text read API; a
node without `dgm:t` (12 in the corpus) reads as one empty paragraph. The
data and drawing parts are parsed (Layer 2) only when a SmartArt is
accessed; the layout, quick-style and colour parts are never parsed.

**Edit.** `SmartArtMut::node_mut(model_id)` gives a text body with the
Phase 0/0.1 setters (same character, field and format-source rules; the
155 data runs with `a:rPr` children need the Phase 0.1 subtree copy).
Additionally:

- **placeholder flag:** non-empty text on a node with `prSet@phldr="1"`
  removes `phldr` (37 placeholder nodes in the corpus are all empty; nodes
  with text have it absent or `"0"`); `phldrT` is kept. Making the text
  empty again does **not** restore `phldr="1"` (the node keeps an empty
  paragraph, as PowerPoint does after deleting typed text; review m8).
- **missing `dgm:t`:** writing to such a node creates `dgm:t` with an
  empty `a:bodyPr` and `a:lstStyle` and the paragraphs.
- **drawing cache** (`dsp:dataModelExt@relId` on the data model, resolved
  from the **slide's** relationships → `diagramDrawing` part):
  - **no cache, or a cache with no shapes** (26 + 15 frames): edit the
    data only; consumers lay out themselves (LibreOffice's `bCreate`).
  - **non-empty cache:** the edit is applied to the cache too. The shapes
    showing the node are the drawing shapes whose `modelId` is a `presOf`
    destination of the node; in each, the node's **slice** is its
    paragraphs within the concatenation of the shape's sources (in
    `destOrd` order; a source without `dgm:t` counts as one empty
    paragraph). **Invariants checked before the edit** (else
    `SmartArtCacheInconsistent`; 5 shapes in one corpus file): (1) the
    shape's paragraphs equal that concatenation (321 of 326); (2) no two
    sources share a `destOrd` (0 ties in the corpus); (3) per paired
    paragraph, equal run counts and every data run's `a:rPr` attribute
    (proofing attributes `lang`, `altLang`, `dirty`, `err`, `smtClean`
    aside) present with the same value on the paired cache run (429/432
    paragraphs, 431/431 runs).
  - **Mapping the edit (review M3).** Paragraph and run edits map by
    index into the slice. A changed run text or a run/paragraph removal
    applies to the paired cache run/paragraph; a data `a:rPr` attribute
    change is written onto the paired cache run as well (the cache keeps
    its layout-computed `sz` unless the data sets `sz`). **New**
    paragraphs and runs in the cache take their format sources from the
    node's **own slice** (its last paragraph's `a:pPr`/`a:endParaRPr`,
    its first run's `a:rPr`, by the Phase 0.1 rules) — never from another
    node's paragraphs, whose level and bullets differ (aggregate shapes
    have levels `0,1,1`, …). An **empty slice** (a node with no shown
    paragraph) takes them from the preceding sibling slice's last
    paragraph in the same shape; with none, `SmartArtCacheInconsistent`.
    The cache keeps its own formatting otherwise; layout-computed sizes
    and autofit are not recomputed (cosmetic, Q2), and
    `docProps/thumbnail.jpeg` stays as it was.
  - **explicit alternative:** `SmartArtMut::drop_drawing_cache()` removes,
    in one registry transaction (ADR-0012 §2), the `a:ext` holding
    `dsp:dataModelExt` (and `dgm:extLst` if it becomes empty, staged
    data part), the **slide's** `diagramDrawing` relationship, and the
    drawing part with its own relationships (`remove_parts` with the
    allowed types `…/2007/relationships/diagramDrawing`, `image`,
    `hyperlink`; the plain `remove_part` of PR #14 refuses parts with own
    relationships, review m2). A drawing referenced by another frame
    (0 in the corpus) is `SmartArtCacheShared`. Never done implicitly.
- **locks:** a frame inside `mc:AlternateContent` (Phase 0 rule), a
  missing data part, an unresolved `dataModelExt` relationship
  (3 frames: `SmartArtCacheUnresolved`), comment anchors on the graphic
  frame (ADR-0005 §6). Animations do **not** lock SmartArt text: node
  animations target `p:graphicEl/a:dgm@id` (a data `modelId`), which a
  text edit keeps, and table/diagram timing in the corpus targets whole
  frames (review m6).

Regenerating the cache (running the SmartArt layout definitions) is
rejected for Phase 1b: LibreOffice's engine is thousands of lines and POI
declines it; the edit-both rule needs no layout knowledge.

### 5. Errors

| Error | When |
|---|---|
| `NotATable`, `CellNotFound`, `MergedCell` (Phase 0) | as before |
| `TableNotWellFormed` | §2 well-formedness, partial `a16` ids |
| `CommentAnchoredTable { shape_id }` | structural edit on a comment-locked frame (§2) |
| `RowIdNotSupported` | inserting into an `a16`-id table before the core namespace primitive exists (§2) |
| `MergeOriginInRow`, `MergeOriginInColumn` | delete through a merge origin |
| `RangeContainsMerge`, `RangeOutsideGrid` | `merge` |
| `NotAMergeOrigin` | `unmerge` |
| `LastRow`, `LastColumn` | deleting the only row/column |
| `TableStyleNotDefined` | §3 |
| `NotSmartArt`, `SmartArtNodeNotFound` | model access |
| `SmartArtCacheInconsistent`, `SmartArtCacheUnresolved`, `SmartArtCacheShared` | §4 |
| `FormatNotCopyable`, `Core(…)` (`TooDeep`, `NamespaceNotInScope`) | format copies and inserts, all checked before the first change |
| Phase 0 guards | `InsideAlternateContent`, `AnimatedCharacters`, `CommentAnchoredText`, … |

### 6. Reference implementations (source-verified)

| Rule | python-pptx 1.0.2 (`pptx/table.py`, `pptx/oxml/table.py`) | POI `47dcff76` (`XSLFTable`, `XSLFTableRow`, `XSLFDiagram`) | LibreOffice `2e885d0b` | ADR-0013 |
|---|---|---|---|---|
| insert/delete rows, columns | none | `insertRow` (new cells, height 20 pt), `addRow` (no cells), `removeRow`, `insertColumn` (copies the width of column `colIdx`), `removeColumn`; no span, frame-size or `a16` update | — | §2 |
| merge | refuses a range containing any merged cell; moves content to the origin; sets `rowSpan` on the whole top row and `gridSpan` on the whole left column, flags elsewhere (`_Cell.merge`, `TcRange`) | no checks; spans on the origin only, flags elsewhere; content stays in covered cells (`mergeCells`) | export writes covered edge cells with spans (`oox/source/export/shapes.cxx`, `vMerge`+`gridSpan`, `hMerge`+`rowSpan`) | python-pptx's checks and content move; the edge form (PowerPoint's, python-pptx's, LibreOffice's) |
| unmerge | `split`: spans and flags reset | — | — | same |
| table style | — | `getTableStyle` reads `tableStyles.xml` | undefined built-in GUIDs synthesized (`TableProperties::getUsedTableStyle` → `CreateTableStyle`) | set only defined ids |
| SmartArt | opaque | renders from the drawing cache only | renders the non-empty cache, else own layout; rewrites the cache on export | edit data **and** cache |

### 7. Test oracles

- **T2 locality:** every table operation changes only the `a:tbl`
  subtree and the frame's `a:ext`; SmartArt edits change only the edited
  node's `dgm:t` (and `phldr`) in the data part and the affected shapes'
  text bodies in the drawing part; every other part byte-identical.
- **Table model oracle:** after each operation, the grid model (cell text,
  origin/span/covered flags, widths, heights, frame size, `a16` ids) equals
  `apply(op, model(input))` computed in Rust and independently in python
  on python-pptx's lxml tree (python-pptx's `merge`/`split` serve as a
  partial reference for the merge rules), plus well-formedness of the
  output.
- **SmartArt oracle:** after an edit, the data node text equals the new
  text and the cache invariant (§4) holds for every shape; python reads
  both parts independently; LibreOffice (the pinned O3 workflow) renders
  the edited deck and its exported PDF text contains the new text.
- **T4 refusals:** every row of §5, byte-identical; synthetic inputs for
  inconsistent merges, mixed edge/flag spans, partial `a16` ids, an
  inconsistent cache, a `destOrd` tie, a table with a `tcMkLst` comment
  (`delete_row` and `merge` refused, review B2).
- **Edge-form oracle (review B1):** on PowerPoint's `tbl-cell.pptx` and
  `sc066-001.pptx`, insert and delete rows/columns through 2-D merges; the
  output is well-formed in the edge form, and python's independent model
  agrees.
- **Borders:** after any `set_border`, no shared edge differs.
- **Manual (owner):** PowerPoint opens edited tables (rows inserted into
  1-D and 2-D vertical merges, merged ranges, borders) without repair;
  for Q1, three decks opened, closed without saving and then saved: (a)
  data edited and the cache given a different text `CACHE-ONLY`, (b) both
  edited, (c) cache dropped — on Windows, macOS, the web and iOS; Q2: one
  node given a long text, sibling font sizes observed without editing.

## Consequences

- Positive: tables can be extended and reshaped without losing cell
  formatting, merge consistency or comment anchors; SmartArt text can be
  updated and every consumer that renders the cache sees the new text.
- Negative: structural edits refuse malformed tables (1 of 119),
  comment-anchored frames and deletions through merge origins; inserts
  into `a16`-id tables wait for a core primitive; SmartArt node structure
  stays read-only; cached sizes and the thumbnail can be stale.
- Negative: two more parts (SmartArt data and drawing) become editable
  Layer 2 parts; the `dml` allowlist grows by the `dgm`/`dsp` text
  elements and the table property elements.

## Alternatives considered

1. **Edit SmartArt data only** and leave the cache. Rejected: LibreOffice
   and POI render the cache, so the edit would be invisible there.
2. **Always drop the cache.** Rejected as a default: LibreOffice's own
   layout differs from PowerPoint's, POI shows nothing; kept as an
   explicit call.
3. **Regenerate the cache.** Rejected: needs the SmartArt layout engine.
4. **POI's merge (no checks, content left in covered cells).** Rejected:
   overlapping merges corrupt the grid, and hidden text survives in
   covered cells (1 corpus cell already has it).
5. **Allow any table style GUID** (PowerPoint knows built-ins).
   Deferred: a reader without the built-in table falls back to no style;
   adding the definition to `tableStyles.xml` is the Phase 3 answer.

## Open questions for the project owner

Each has a **proposed default** (the round-1 review's recommendation).

- **Q1** Does PowerPoint re-lay out SmartArt from the data model?
  *Default:* very likely yes (Microsoft Q&A 912190); confirm with the
  fake-cache manual check (§7). Until then edit both data and cache (needed
  for LibreOffice, POI and cache-based renderers); `drop_drawing_cache`
  stays an explicit call.
- **Q2** Does PowerPoint re-fit cached sizes after longer text? *Default:*
  manual check; recomputing autofit stays out of scope either way
  (as ADR-0005 §6).
- **Q3** Rows inserted inside a vertical merge join it. *Default:* yes, in
  the edge form (§2); the manual-check deck has 1-D and 2-D merges.
- **Q4** Borders and fills in Phase 1b. *Default:* yes, with the
  edge-level border API (§3); table styles accept the closed list of
  built-in ids.

## Sources

- Spike: `spikes/pptx-phase1b/tables_smartart.py`, output
  `results/tables_smartart-619.txt`, corpus by content
  `results/corpus-sha256.txt`.
- Microsoft Q&A 912190 (SmartArt `drawing#.xml` as a layout cache);
  Aspose forum thread 65095 (third-party observation).
- python-pptx 1.0.2: `pptx/table.py` (`_Cell.merge`, `split`,
  `span_height`, `span_width`), `pptx/oxml/table.py` (`TcRange`:
  `contains_merged_cell`, `move_content_to_origin`).
- Apache POI `47dcff769bb395034dddfd59c4609659b6dbc6a4`,
  `poi-ooxml/src/main/java/org/apache/poi/xslf/usermodel/`:
  `XSLFTable.java` (`addRow`, `insertRow`, `removeRow`, `addColumn`,
  `insertColumn`, `removeColumn`, `mergeCells`), `XSLFTableRow.java`,
  `XSLFDiagram.java` (`readDiagramDrawing`, class Javadoc).
- LibreOffice `2e885d0bda9bc4ee9f3bde51905c4a7820b01f06`:
  `oox/source/drawingml/diagram/diagram.cxx` (`loadDiagram`,
  `DiagramShapeCounter`, `writeDiagramOOXDrawing`),
  `oox/source/drawingml/table/tableproperties.cxx` (`getUsedTableStyle`),
  `oox/source/export/shapes.cxx` (table cell merge export).
- ECMA-376 Part 1, DrawingML tables (`a:tbl`, `a:tc@gridSpan/rowSpan/hMerge/vMerge`)
  and DrawingML diagrams (`dgm:dataModel`, `dgm:pt`, `dgm:cxn`); the
  vendored Transitional XSDs (`dml-main.xsd` `CT_TableCell`,
  `dml-diagram.xsd`); [MS-ODRAWXML] for the `dsp` drawing extension.
