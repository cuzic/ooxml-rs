# ADR-0013: PowerPoint Phase 1b — table structure and SmartArt text

- Status: **Accepted** (revision 4)
- Date: 2026-11-16
- Depends on: [ADR-0001](0001-lossless-round-trip-architecture.md),
  [ADR-0002](0002-schema-driven-code-generation.md) (lens, allowlist and
  `[detach]`, post-acceptance notes "detach/insert" and "parser depth
  limit"), [ADR-0005](0005-pptx-phase0.md) (Phase 0 text edits, references
  into text, refusals; revision 4 §9 "Phase 0.1 — subtree copy of format
  sources", PR #19), ADR-0008 (Phase 1, PR #28, Proposed), which moved
  both topics here. Core PRs: #44 (`core/move-subtree`, `merge`), #45
  (`core/insert-with-ns`, `a16` ids) plus the codegen addition of §2
  (review round 3 P1), #38 (`core/add-part`, registry transaction;
  `drop_drawing_cache` is implemented only after #38 merges with its
  round-1 fixes, P2).
- Evidence: [`spikes/pptx-phase1b/`](../../spikes/pptx-phase1b/README.md)
  (`tables_smartart.py`; 619 distinct presentations from the ADR-0008
  corpora: POI, Open XML SDK, python-pptx, LibreOffice `sd/qa/unit/data/pptx`,
  ShapeCrawler); python-pptx 1.0.2, Apache POI `47dcff76` and LibreOffice
  `2e885d0b` source, cited in §6.

- Review: [round 1](0013-opus-review-round1.md),
  [round 2](0013-opus-review-round2.md),
  [round 3](0013-opus-review-round3.md) (Accepted after Minor only)

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

- **Revision 3** (after `0013-opus-review-round2.md`, not converged: one
  Major, N1; N2–N9 Minor):

  | Finding | Change | Where |
  |---|---|---|
  | N1 a row inserted inside a vertical merge copied the merge attributes from the cell above (a second origin at `at = r0 + 1`) | merge attributes come from the **existing row at the insert position** (the cell below after insertion); formatting still from above; columns: from the right (0 mismatches against the logical oracle in the reviewer's 80,165 random cases, 8,759 with the old rule) | §2 |
  | N2 `drop_drawing_cache` allowed types | `[diagramDrawing]` only (types of relationships **into** the set from remaining parts); `SmartArtCacheShared` checked first through `incoming_relationships`; images reached only from the drawing removed with it; data part edit through `stage_part` | §4 |
  | N3 namespace primitive and references | create-with-declaration (`core/insert-with-ns`, PR #45) and `core/move-subtree` (PR #44); `a:ext` URIs and `[write]` entries named; session defined; separate row/column id spaces | §2, §3 |
  | N4 run children in SmartArt | formatting children mapped like attributes; runs with `r:*` (hyperlinks) refused; 193 of 431 runs have data attributes | §4 |
  | N5 table anchors | all five `*MkLst` names; a whole-frame `deMkLst` anchor does not lock; any `rowId`/`colId` or `tcMk`/`trMk`/`gridColMk`/`tblMk` under an anchor locks | §2 |
  | N6 span reaching 1 | the span attribute is removed from origin and edge cells | §2 |
  | N7 `[detach]` review, move checks | reachable complex types listed in the follow-up; PR #44's checks named | §2 |
  | N8 empty slices, empty data runs | "empty slice" folded into `SmartArtCacheInconsistent`; empty data runs ignored by the run mapping | §4 |
  | N9 border API details | covered cell → its merged range; outside neighbours paired; oracle limited to written edges; GUIDs compared case-insensitively | §3, §7 |

- **Revision 4** (after `0013-opus-review-round3.md`: N1 resolved, 0
  mismatches over about 430,000 random cases; Accepted after Minor
  only). Status: Accepted.

  | Finding | Change | Where |
  |---|---|---|
  | P1 no `ChildOf<a:ext>` (its SDK content is one `Any`) | a codegen addition is requested as an ADR-0002 note (children for a wildcard-only parent, keyed by `a:ext@uri`); `RowIdNotSupported` lifts only with #45 **and** that addition | §2 |
  | P2 `drop_drawing_cache` depends on #38's open fixes (M3, m6, M6/m10) | implemented after #38 merges with them; refused on any unreadable `.rels`; only existing parts added to the set; "no orphans" qualified | header, §4 |
  | P3 empty runs ignored on the data side only | ignored on **both** sides (431/431 unchanged); indices count non-empty runs; text put into an empty run goes after the preceding non-empty cache run | §4 |
  | P4 nits | (a) a 1-D vertical merge copies `vMerge` only; (b) `r:id` sits on `a:hlinkClick`/`a:hlinkMouseOver` under `a:rPr`, refusal scope stated; (c) `InvalidNamespaceDeclaration` in §5 | §2, §4, §5 |

- **Post-acceptance note (2026-11-16, implementation of §4 SmartArt text
  on `impl/pptx-phase1b-smartart` — no change to the decisions):**
  - API: `Presentation::smart_art_mut(slide, frame_id)` →
    `SmartArtMut::{nodes, set_text, set_paragraph_text, set_run_text,
    cache_report, drop_drawing_cache}`. Edits are the Phase 0/0.1 text
    setters, applied to the data node's `dgm:t` and, through a
    range-limited variant of `TextBodyMut::set_text`, to the node's slice
    of each cache shape; the data and drawing parts are edited on clones
    and installed with one `RegistryPlan` (`stage_part` + `apply`).
  - Sources of a shape are ordered by `destOrd`, then `srcOrd`.
  - The hyperlink refusal covers the run written and the run whose
    format a new run would copy, in the data and in the cache.
  - `drop_drawing_cache` refuses on any unreadable relationships part
    (P2's safe rule) before `incoming_relationships`, and removes, with
    the drawing, every part only the drawing references.
  - Codegen: the `dgm:`/`dsp:` text elements join `dml.toml`; their SDK
    class names collide with `a:txBody`'s `TextBody`, so the generator now
    keeps the plain name for the group's own namespace (`a:` in `dml`)
    and prefixes the others (`DgmTextBody`, `DspTextBody`).
  - Corpus (633 decks): 103 SmartArts, 600 nodes, 408 shown in a cache,
    396 consistent, **431 paired runs** — the review's 431/431 with
    empty runs ignored on both sides; 587 node edits succeed and leave the
    cache consistent, every refusal leaves the package unchanged.

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
a newly created covered cell copies the **merge attributes** of the
existing cell **at the insert position** — the cell that will be below
it for a new row (row `at`), right of it for a new column (column `at`)
(review N1). Inside a merge (`r0 < at ≤ r0 + R − 1`) that cell is always
a covered cell of the same merge, so the copy is `vMerge` (plus
`gridSpan` as that cell has it, i.e. in a 2-D merge) in the origin's
column and `hMerge` + `vMerge` elsewhere; in a 1-D vertical merge it is
`vMerge` only (review round 3 P4a) — never the
origin's own `rowSpan` (copying from the cell above at `at = r0 + 1`
would create a second origin; every corpus vertical merge has
`rowSpan="2"`, so `r0 + 1` is the only inside position there). Its
**formatting** (`a:tcPr`, text body, paragraph) still comes from the
cell above (left), a separate source. Edge spans are recomputed from the
origin's. When a span drops to 1 (a delete), the attribute is
**removed** from the origin and the edge cells alike, not written as
`"1"` (review N6).

| Operation | Rule |
|---|---|
| `insert_row(at)` | the new `a:tr` copies `@h` from the row above (`at − 1`, else the row below); each new `a:tc` copies the cell above's (else below's) `a:tcPr`, and gets a text body with that cell's `a:bodyPr`/`a:lstStyle` copies and one empty paragraph whose `a:pPr`/`a:endParaRPr` copy that cell's first paragraph (ADR-0005 §3 format sources); `p:xfrm` `cy` grows by `@h`. **Inside a vertical merge** (strictly after its origin row, up to its last covered row) the new cells in the merged columns are covered — merge attributes copied from the cell **below** (the existing row `at`), formatting from the cell above — and the origin's `rowSpan` — and the edge cells' — grow by 1; at a merge's boundary the new row is outside it. |
| `delete_row(r)` | removes the `a:tr`; `cy` shrinks by its `@h` (it may stay larger than the text-grown rows; PowerPoint corrects it on open, review m7); a covered row shrinks its merge (origin and edge `rowSpan − 1`; a span reaching 1 is removed from the origin and every edge cell, review N6); a row holding the **origin** of a merge with `rowSpan > 1` is refused (`MergeOriginInRow`: unmerge first). Never the last row. |
| `insert_column(at)`, `delete_column(c)` | the same with `a:gridCol` (`@w` copied from the column left, else right; `cx` adjusted), `gridSpan`, `hMerge`: merge attributes from the cell **right** of the new column (column `at`), formatting from the left; spans reaching 1 removed; `MergeOriginInColumn`; never the last column. |
| `merge(r1, c1, r2, c2)` | refused when the rectangle contains any merged cell (python-pptx's rule) or leaves the grid. Written in the edge form. The **paragraphs of the other cells move to the origin** in row-major order, empty cells skipped; when the origin holds a single empty paragraph it is replaced, not kept (python-pptx `append_ps_from`, review m4); each covered cell keeps one empty paragraph (its first paragraph's `a:pPr`/`a:endParaRPr`). |
| `unmerge(r, c)` | on an origin: spans (origin and edges) and flags removed; text stays in the origin (python-pptx `split`). |
| `set_column_width(c, w)`, `set_row_height(r, h)` | `gridCol@w` / `tr@h`, `p:xfrm` `cx`/`cy` adjusted by the difference. |

A graphic frame inside a group: the frame's `a:ext` changes, the group's
`a:ext`/`a:chExt` are **not** recomputed (ADR-0016 §6's rule; review m7).

**Comment anchors (review B2).** ADR-0005 §6's comment lock refuses only
*text* edits. Structural operations — `insert_*`, `delete_*`, `merge`,
`unmerge` — on a graphic frame that has a comment anchor **inside** the
table are refused with `CommentAnchoredTable { shape_id }`. Inside
means (review N5): a list of the table kinds (`oac:tblMkLst`,
`tcMkLst`, `trMkLst`, `gridColMkLst`; `graphicFrameMkLst` when it
nests one of them — all five are `xsd:any` in the Open XML SDK
schema), or, **whatever the list's name** (including a `deMkLst`), any
`rowId`/`colId` attribute or `tcMk`/`trMk`/`gridColMk`/`tblMk` element
under the anchor, or an anchor ADR-0005 §6 cannot verify. A
whole-frame anchor (`deMkLst` with only a `graphicFrameMk`, which §6
does not lock) does **not** lock structural edits: no row, column or
text offset of it can dangle. Corpus: 0 table-anchored comments (one
file has modern comments), so T4 uses synthetic inputs. The refusal is
needed because deleting an anchored row or column
leaves the anchor dangling, and moving paragraphs invalidates
`txMk@cp/len`. Cell properties, widths and heights are allowed (they move
no anchor). A finer rule (only rows/columns whose ids no anchor names) can
come later, provided every `tcMk`/`trMk`/`gridColMk` resolves.

**Row and column ids (review M1).** When the table carries
`a16:rowId`/`a16:colId` (50 of 119), a new row/column needs one, and
PowerPoint declares `a16` on that element itself (0 of 33 slides declare
it on `p:sld`). A create-then-declare order cannot work: core's
`LensMut::insert` refuses an unbound namespace (`NamespaceNotInScope`)
at creation. It needs **creation with the declaration** in one step,
the core PR `core/insert-with-ns` (PR #45: `insert_with_ns` with
`NsDecl { prefix: "a16", uri: "http://schemas.microsoft.com/office/drawing/2014/main" }`
and check twins; review N3). The elements created are
`a:tr/a:extLst/a:ext[@uri="{0D108BD9-81ED-4DB2-BD59-A6C34878D82A}"]/a16:rowId@val`
and
`a:gridCol/a:extLst/a:ext[@uri="{9D8B030D-6E8A-4147-A177-3AD203B41FA5}"]/a16:colId@val`
(the URIs of all 152 / 135 corpus ids). #45 alone is not enough
(review round 3 P1): its `*_with_ns` inserts need `C: ChildOf<E>`, and
the SDK data gives `a:CT_OfficeArtExtension/a:ext` a single `Any`
particle with no `Children`, so codegen emits no `ChildOf<a:ext>` (the
"wildcard parents" rule reads the parent's `Children`), and
`overrides.toml`'s `add_child` needs a named sibling to anchor on. This
ADR therefore requests a **codegen addition**, recorded as an ADR-0002
post-acceptance note: an override that gives a wildcard-only parent
named children, keyed by the extension URI (`a:ext@uri` decides the
content: one child per URI — `a16:rowId` under
`{0D108BD9-…}`, `a16:colId` under `{9D8B030D-…}`), with the corpus
counts (152/135) as evidence. Until **both** #45 and that addition are
merged, `insert_row`/`insert_column` on such a table are
`RowIdNotSupported`
(deletion, merge and properties are unaffected). **Ids:** `rowId` and
`colId` are separate value spaces. A new id is unused in its space in
the table and not deleted from that table earlier in the session:
`max + 1` when that fits `u32` (`xsd:unsignedInt`), else the smallest
unused value ≥ 1 (32 tables already hold ids ≥ 2³¹; PowerPoint's ids
look random). The **session** is the lifetime of the `Presentation`
value (as ADR-0012 §3); deleted ids are recorded per graphic frame. A
table where only some rows/columns carry ids is `TableNotWellFormed`.

**Copies and moves (review M2).** The cell copies above need
`Detachable` for `a:tcPr`, `a:bodyPr` and `a:lstStyle` — an addition to
`dml.toml`'s `[detach]` reviewed like ADR-0002's note "detach/insert"
(corpus `a:tcPr`: no `extLst`, no `r:*`; their unique attributes, if any,
listed in `unique_attrs`). The follow-up that adds them lists, as
ADR-0002's note requires, every complex type they reach — e.g.
`a:cell3D`, `a:headers`, `a:bodyPr`'s `a:scene3d`/`a:sp3d`/
`a:normAutofit`, the nine levels of `a:lstStyle` — and their unique
attributes (review N7). The `a16` ids live in `a:tr`/`a:gridCol`, not in
`a:tcPr`, and §2 copies only `a:tr@h`, so no id is duplicated;
`detach_as`'s refusals of `r:embed` in a `blipFill`, `hlinkClick` in
`a:lstStyle`, `buBlip` and `extLst` are sound refusals
(`FormatNotCopyable`). `merge` **moves** whole `a:p` subtrees inside one
part; a copy would wrongly refuse links (`r:id`), `smtId` and fields
that a move keeps unique. It uses the core same-part move of
`core/move-subtree` (PR #44: `LensMut::move_in_by` / `check_move_in_by`),
which refuses a destination inside the moved element, a prefix bound
differently at the destination, a namespace ignorable at one place only,
text whose inherited `xml:space` differs, and the depth limit, all
before any change; it keeps the moved paragraph's bytes (no re-render),
and paths into the parts it changed are stale afterwards (each move is
planned on the current tree). Until both exist, `insert_*` and `merge`
are not implemented (the ADR is not accepted without them).

**Atomicity.** Plan → validate → apply as in ADR-0005: the
well-formedness check, the merge arithmetic, every copy and move check
(`check_insert_under` with depth) and the frame size are computed before
the first change; a refusal leaves the package byte-identical.

### 3. Cell properties and table style

On `TableMut`/`CellMut` (typed setters on `a:tcPr`, `a:tblPr`; the
allowlist moves `a:tcPr`, `a:tblPr`, `a:tableStyleId`, `a:lnL/R/T/B`,
`a:solidFill`, `a:noFill`, `a:srgbClr`, `a:schemeClr` (read today) to
`[write]` in `dml.toml`; the structural edits of §2 add `a:tbl`,
`a:tblGrid`, `a:gridCol`, `a:tr`, `a:tc`, `a:txBody`, `a:p`,
`a:extLst`/`a:ext` and `a16:rowId`/`a16:colId`, review N3):

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
  found). On a merged origin **or a covered cell** the edge means the
  merged range's outline (a covered cell names its range, it is not
  `MergedCell`), and every cell along it (covered ones included) gets the
  line, as does the paired line of each neighbour **outside** the range
  along that edge (review N9). An existing
  `a:ln*` is replaced as a whole. `table.set_borders(Line)` sets all
  edges. Diagonals are not written;
- table style: `set_style(Some(id))` to a `styleId` defined in
  `ppt/tableStyles.xml`, or to one of PowerPoint's **built-in** style ids
  (the closed list of 74 that LibreOffice ships in
  `predefined-table-styles.cxx`, which includes the usual `@def`;
  compared case-insensitively, as GUID text), else
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
    paragraph, so a slice always has at least one paragraph). Runs with
    empty text (`<a:r><a:t/></a:r>`) are ignored by the run mapping **on
    both sides**, data and cache (review N8, round 3 P3; still 431/431
    runs): run indices count non-empty runs only, so an edit that
    empties a run does not break the next edit, and text put into an
    empty data run goes into the cache after the paired cache run of the
    preceding non-empty data run (at the paragraph's start when there is
    none). **Invariants checked before the edit** (else
    `SmartArtCacheInconsistent`; 5 shapes in one corpus file): (1) the
    shape's paragraphs equal that concatenation (321 of 326); (2) no two
    sources share a `destOrd` (0 ties in the corpus); (3) per paired
    paragraph, equal run counts and every data run's `a:rPr` attribute
    (proofing attributes `lang`, `altLang`, `dirty`, `err`, `smtClean`
    aside) present with the same value on the paired cache run (429/432
    paragraphs, 431/431 runs, of which 193 have data attributes; review
    N4). Ignoring empty data runs admits the two `tdf152606.pptx`
    paragraphs the round-2 count refused (an empty node, the typical one
    to fill in).
  - **Mapping the edit (review M3).** Paragraph and run edits map by
    index into the slice. A changed run text or a run/paragraph removal
    applies to the paired cache run/paragraph; a data `a:rPr` attribute
    change is written onto the paired cache run as well (the cache keeps
    its layout-computed `sz` unless the data sets `sz`), and so is a
    change of a formatting **child** (`a:solidFill`, `a:latin`, …; 148 of
    149 data runs with children have the same children in the cache).
    A run whose `a:rPr` **subtree** holds an `r:*` attribute (a
    hyperlink: the `r:id` sits on the child `a:hlinkClick` /
    `a:hlinkMouseOver`, and differs between the data and the drawing
    part's relationships, 1 in the corpus) is refused
    (`FormatNotCopyable`) for every SmartArt edit that would write to it
    or copy its `a:rPr` — text changes and formatting of that run, and
    new runs taking it as a format source; other runs of the same node
    stay editable (review round 3 P4b). **New**
    paragraphs and runs in the cache take their format sources from the
    node's **own slice** (its last paragraph's `a:pPr`/`a:endParaRPr`,
    its first run's `a:rPr`, by the Phase 0.1 rules) — never from another
    node's paragraphs, whose level and bullets differ (aggregate shapes
    have levels `0,1,1`, …). A slice with no paragraph cannot occur on
    a schema-valid input (`CT_TextBody` has at least one `a:p`, and a
    missing `dgm:t` counts as one); any such case is
    `SmartArtCacheInconsistent` (review N8).
    The cache keeps its own formatting otherwise; layout-computed sizes
    and autofit are not recomputed (cosmetic, Q2), and
    `docProps/thumbnail.jpeg` stays as it was.
  - **explicit alternative:** `SmartArtMut::drop_drawing_cache()`, one
    registry transaction (ADR-0012 §2; review N2). It depends on #38's
    round-1 fixes M3 (unreadable `.rels` in `remove_parts`), m6
    (`incoming_relationships` skipping them) and M6/m10 (fragments and
    unresolvable targets, false `kept_targets`), and is implemented only
    after #38 merges with them (review round 3 P2). Until then, and as a
    safe rule afterwards, a relationships part that cannot be read or
    parsed anywhere in the package refuses it (`Core(…)`):
    1. `Package::incoming_relationships(drawing)` must be exactly the
       one `diagramDrawing` relationship of this slide (75/75 drawing
       parts in the corpus), else `SmartArtCacheShared` — checked first,
       because `remove_parts` would silently drop another slide's
       relationship too;
    2. the data part, edited on a clone without the `a:ext` holding
       `dsp:dataModelExt` (and `dgm:extLst` if it becomes empty), is
       staged with `RegistryPlan::stage_part`;
    3. `remove_parts` with the drawing part **plus** each **existing**
       part it targets that nothing outside the set references (images
       used only by the cache; in the corpus all 7 drawing images are
       also referenced by the data part, so the set is the drawing
       alone), and `allowed_rel_types = [diagramDrawing]`: the
       allowed types are those of relationships **into** the set from
       remaining parts (only the slide's); the drawing's own
       relationships (`image` 7, `hyperlink` 1 in the corpus) go with it
       unchecked. The slide's relationship is removed by `remove_parts`
       (no separate step). Targets still used elsewhere stay; once #38's
       M6/m10 are fixed, the returned `kept_targets` are therefore parts
       still referenced and nothing is left orphaned.
    Never done implicitly.
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
| `FormatNotCopyable`, `Core(…)` (`TooDeep`, `NamespaceNotInScope`, `InvalidNamespaceDeclaration`, `NotMovable`) | format copies, inserts and moves, all checked before the first change; SmartArt runs with hyperlinks (§4) |
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
  (`delete_row` and `merge` refused, review B2), a `deMkLst` anchor
  holding a `trMk` (refused) and one holding only a `graphicFrameMk`
  (allowed, review N5), a shared drawing part on `drop_drawing_cache`.
- **Edge-form oracle (review B1):** on PowerPoint's `tbl-cell.pptx` and
  `sc066-001.pptx`, insert and delete rows/columns through 2-D merges —
  every insert position, `at = r0 + 1` included; the output is
  well-formed in the edge form, and python's independent model agrees;
  plus the reviewer's random generator (1–5 × 1–5 grids, up to four
  non-overlapping merges, every insert position) as a property test
  (review N1).
- **Borders:** after a `set_border`, every shared edge it wrote is equal
  on both sides; other edges are as in the input (5 already differ in
  other producers' tables, review N9).
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

**Owner decision 2026-11-16:** the proposed defaults of Q1–Q4 below are
accepted as stated.

- **Q1** Does PowerPoint re-lay out SmartArt from the data model?
  *Default:* very likely yes (Microsoft Q&A 912190); confirm with the
  fake-cache manual check (§7). Until then edit both data and cache (needed
  for LibreOffice, POI and cache-based renderers); `drop_drawing_cache`
  stays an explicit call.
- **Q2** Does PowerPoint re-fit cached sizes after longer text? *Default:*
  manual check; recomputing autofit stays out of scope either way
  (as ADR-0005 §6).
- **Q3** Rows inserted inside a vertical merge join it. *Default:* yes, in
  the edge form, merge attributes from the row below (§2); the
  manual-check deck has 1-D and 2-D merges.
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
