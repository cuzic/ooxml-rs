# ADR-0013: PowerPoint Phase 1b — table structure and SmartArt text

- Status: **Proposed** (revision 1)
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
| merges | 32 tables (gridSpan > 1: 56 cells, rowSpan > 1: 22, hMerge: 166, vMerge: 32); 31 consistent (every origin's span is covered by flagged cells), **1 inconsistent** (`LIBRE_OFFICE-100610-0.pptx`); covered cells with text: 1; covered cells that **also** carry a span (the python-pptx/LibreOffice "edge" form): 3 vMerge+gridSpan, 4 hMerge+rowSpan |
| grid | one table has a row whose `a:tc` count ≠ `gridCol` count |
| frame size | `p:xfrm/a:ext@cx` = Σ `gridCol@w` in 113/119; `@cy` = Σ `tr@h` in 81 (rows grow with text in 38) |
| row/column ids | `a16:rowId` / `a16:colId` (Office 2016+) in **50 of 119** tables; modern comments anchor table cells by them (`ac:tcMk@rowId,colId`, ADR-0005 §6) |
| table styles | `tableStyleId` in 92 tables: 84 defined in `ppt/tableStyles.xml`, 8 built-in GUIDs not defined there (PowerPoint and LibreOffice both know built-ins: LibreOffice's `CreateTableStyle` builds them from `predefined-table-styles.cxx`); `tableStyles.xml@def` is `{5C22544A-…}` (Medium Style 2 – Accent 1) in 551 files; flags `firstRow` 93, `bandRow` 92 |
| cell properties | `tcPr` attributes `marL/marR` ~716, `marT/marB` 573, `anchor` 376, `vert` 10; children `solidFill` 617, `lnL/R/T/B` ~575 each, `lnTlToBr`/`lnBlToTr` 56, `noFill` 32 |
| SmartArt | 105 frames in 82 files; layouts `default` 14, `hierarchy4` 10, `orgChart1` 6, … (25+ distinct); points `pres` 1,953, `parTrans`/`sibTrans` 650 each, `node` 646, `doc` 105, `asst` 3; connections `presParOf` 1,848, `presOf` 992, `parOf` 650 |
| SmartArt text | 649 data nodes (`node`/`asst`), 596 with text; 677 data runs, 155 with `a:rPr` children (format copy needed); 26 paragraphs with > 1 run, 0 with fields; placeholder nodes (`prSet@phldr="1"`) 37, **never with text** (nodes with text: `phldr` absent 555, `"0"` 39) |
| drawing cache | 76 frames have `dsp:dataModelExt` → `diagramDrawing` part; 61 non-empty, **15 empty** (LibreOffice test files: no shapes); 26 have no cache; 3 point at a relationship the slide lacks |
| cache consistency | of 326 drawing shapes with text, **321 show exactly the concatenation of the paragraphs of the data nodes whose `presOf` connection points at them**, ordered by `destOrd` (16 of them aggregate > 1 node, e.g. a parent and its children as bullets); 5 do not (one file, `Segmented_Cycle_oneColor.pptx`); per data node with text in a non-empty cache: 305 shown 1:1, 50 as part of an aggregate, 1 not found (a lone NBSP) |

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
  a Microsoft extension, `http://schemas.microsoft.com/office/drawing/2008/diagram`);
  owner manual check Q1.

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

Model: `rows × gridCol` grid; each `a:tc` occupies one grid slot; a
**merge** is an origin cell with `gridSpan`/`rowSpan` and the other cells
of its rectangle carrying `hMerge` (other columns) and/or `vMerge` (other
rows). Before any structural edit the table must be **well-formed**:
every row has as many `a:tc` as `gridCol`, every origin's rectangle is
inside the grid and fully flagged, every flagged cell lies in exactly one
origin's rectangle; otherwise `TableNotWellFormed` (at most 2 of 119 corpus tables: one with inconsistent merge flags, one with a short row).
Covered cells may additionally carry the edge spans written by
python-pptx and LibreOffice (7 corpus cells); they are read as covered
and **kept as found**; cells created or re-flagged by Phase 1b get only
the flags (POI's and, judging by the corpus, PowerPoint's form).

| Operation | Rule |
|---|---|
| `insert_row(at)` | the new `a:tr` copies `@h` from the row above (`at − 1`, else the row below); each new `a:tc` copies, through the Phase 0.1 copy primitive, the `a:tcPr` of the cell above/below and gets a text body with that cell's `a:bodyPr`/`a:lstStyle` copies and one empty paragraph whose `a:pPr`/`a:endParaRPr` copy that cell's first paragraph (ADR-0005 §3 format sources); `p:xfrm` `cy` grows by `@h`. **Inside a vertical merge** (between its origin row and its last covered row) the new cells in the merged columns are covered (`vMerge`, plus `hMerge` where the row above has it) and the origin's `rowSpan` grows by 1; at a merge's boundary the new row is outside it. |
| `delete_row(r)` | removes the `a:tr`; `cy` shrinks by its `@h`; a covered cell's merge shrinks (`rowSpan − 1`); a row holding the **origin** of a merge with `rowSpan > 1` is refused (`MergeOriginInRow`: unmerge first). Never the last row. |
| `insert_column(at)`, `delete_column(c)` | the same with `a:gridCol` (`@w` copied from the column left, else right; `cx` adjusted), `gridSpan`, `hMerge`; `MergeOriginInColumn`; never the last column. |
| `merge(r1, c1, r2, c2)` | refused when the rectangle contains any merged cell (python-pptx's rule) or leaves the grid. The origin gets `gridSpan`/`rowSpan`, the other cells `hMerge`/`vMerge`; the **paragraphs of the other cells move to the origin** (appended in row-major order, empty cells skipped — python-pptx `move_content_to_origin`, PowerPoint's UI behaviour) and each covered cell keeps one empty paragraph (its first paragraph's `a:pPr`/`a:endParaRPr`). |
| `unmerge(r, c)` | on an origin: spans and flags removed; text stays in the origin (python-pptx `split`). |
| `set_column_width(c, w)`, `set_row_height(r, h)` | `gridCol@w` / `tr@h`, `p:xfrm` `cx`/`cy` adjusted by the difference. |

**Row and column ids.** When the table uses `a16:rowId`/`a16:colId` (50 of
119), a new row/column gets `max(existing in this table) + 1`
(append-only, as ADR-0012 §3); a table where only some rows/columns carry
ids is `TableNotWellFormed`. Deleting a row or column is refused when a
modern comment anchors to it (`ac:tcMk@rowId/colId`) — already covered by
ADR-0005 §6, which locks the whole graphic frame for such anchors
(`CommentAnchoredText`).

**Atomicity.** Plan → validate → apply as in ADR-0005: the well-formedness
check, the merge arithmetic, every format copy (`detach_as`,
`check_insert_under` with depth) and the frame size are computed before
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
- borders `lnL/lnR/lnT/lnB`: width (EMU) and solid colour, or `noFill`;
  an existing `a:ln*` is replaced as a whole;
- table style: `set_style(id)` only to a `styleId` defined in
  `ppt/tableStyles.xml` or equal to its `@def` (else
  `TableStyleNotDefined`: adding a definition is Phase 3; the 8 corpus
  tables that already name an undefined built-in are read as is); the
  `tblPr` flags `firstRow`, `firstCol`, `lastRow`, `lastCol`, `bandRow`,
  `bandCol`.

Reading resolved table-style formatting stays out (ADR-0008:
`Unresolved::TableStyle`).

### 4. SmartArt text

**Model.** `ShapeRef::smart_art()` returns, for a graphic frame with
`graphicData@uri = …/diagram`, the data part (`dgm:relIds@r:dm`) as a
tree of data points: the `doc` point, then `node`/`asst` points ordered
by their `parOf` connections (`srcOrd`). Each node exposes its text body
(`dgm:pt/dgm:t`, a `CT_TextBody`) with the Phase 0/1 text read API. The
data and drawing parts are parsed (Layer 2) only when a SmartArt is
accessed; the layout, quick-style and colour parts are never parsed.

**Edit.** `SmartArtMut::node_mut(model_id)` gives a text body with the
Phase 0/0.1 setters (`set_text`, paragraph/run edits, the same character,
field and format-source rules; the 155 data runs with `a:rPr` children
need the Phase 0.1 subtree copy). Additionally:

- **placeholder flag:** giving a node with `prSet@phldr="1"` non-empty
  text removes `phldr` (37 placeholder nodes in the corpus are all
  empty; nodes with text have `phldr` absent or `"0"`); `phldrT` (the
  prompt) is kept.
- **drawing cache** (`dsp:dataModelExt@relId` on the data model →
  `diagramDrawing` part from the slide):
  - **no cache, or a cache with no shapes** (26 + 15 frames): edit the
    data only; consumers lay out themselves (LibreOffice's `bCreate`).
  - **non-empty cache:** the edit is applied to the cache too. The shapes
    showing the node are the drawing shapes whose `modelId` is a
    `presOf` destination of the node. Before the edit each such shape
    must show exactly the concatenation of its sources' paragraphs in
    `destOrd` order (the invariant holding for 321 of 326 corpus shapes);
    the node's slice of that shape's paragraphs is then replaced with the
    node's new paragraphs, **keeping the drawing's own formatting**:
    existing paragraphs keep their `a:pPr` and first run's `a:rPr` (the
    layout's sizes and bullet levels live there, not in the data), and new
    paragraphs/runs take the Phase 0.1 format sources **of the drawing
    shape**. A shape violating the invariant makes the edit
    `SmartArtCacheInconsistent` (5 shapes, one file). Sizes and autofit
    computed by the layout are not recomputed (as text autofit in
    ADR-0005 §6: cosmetic, manual check Q2).
  - **explicit alternative:** `SmartArtMut::drop_drawing_cache()` removes
    the `dsp:dataModelExt` extension and, through `remove_part` (PR #14,
    relationship type `…/2007/relationships/diagramDrawing` allowed), the
    drawing part — for callers who prefer the consumer to lay out (then
    LibreOffice uses its own engine, POI shows nothing, PowerPoint per
    Q1). Never done implicitly.
- **refused:** a frame inside `mc:AlternateContent` (Phase 0 rule), a
  missing data part, an unresolved `dataModelExt` relationship
  (3 frames: `SmartArtCacheUnresolved`), text animations or comment
  anchors on the graphic frame (the ADR-0005 §6 locks).

Regenerating the cache (running the SmartArt layout definitions) is
rejected for Phase 1b: LibreOffice's engine is thousands of lines and POI
declines it; the edit-both rule needs no layout knowledge.

### 5. Errors

| Error | When |
|---|---|
| `NotATable`, `CellNotFound`, `MergedCell` (Phase 0) | as before |
| `TableNotWellFormed` | §2 well-formedness, partial `a16` ids |
| `MergeOriginInRow`, `MergeOriginInColumn` | delete through a merge origin |
| `RangeContainsMerge`, `RangeOutsideGrid` | `merge` |
| `NotAMergeOrigin` | `unmerge` |
| `LastRow`, `LastColumn` | deleting the only row/column |
| `TableStyleNotDefined` | §3 |
| `NotSmartArt`, `SmartArtNodeNotFound` | model access |
| `SmartArtCacheInconsistent`, `SmartArtCacheUnresolved` | §4 |
| `FormatNotCopyable`, `Core(…)` (`TooDeep`, `NamespaceNotInScope`) | format copies and inserts, all checked before the first change |
| Phase 0 guards | `InsideAlternateContent`, `AnimatedCharacters`, `CommentAnchoredText`, … |

### 6. Reference implementations (source-verified)

| Rule | python-pptx 1.0.2 (`pptx/table.py`, `pptx/oxml/table.py`) | POI `47dcff76` (`XSLFTable`, `XSLFTableRow`, `XSLFDiagram`) | LibreOffice `2e885d0b` | ADR-0013 |
|---|---|---|---|---|
| insert/delete rows, columns | none | `insertRow` (new cells, height 20 pt), `addRow` (no cells), `removeRow`, `insertColumn` (copies the width of column `colIdx`), `removeColumn`; no span, frame-size or `a16` update | — | §2 |
| merge | refuses a range containing any merged cell; moves content to the origin; sets `rowSpan` on the whole top row and `gridSpan` on the whole left column, flags elsewhere (`_Cell.merge`, `TcRange`) | no checks; spans on the origin only, flags elsewhere; content stays in covered cells (`mergeCells`) | export writes covered edge cells with spans (`oox/source/export/shapes.cxx`, `vMerge`+`gridSpan`, `hMerge`+`rowSpan`) | python-pptx's checks and content move; spans on the origin only |
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
  inconsistent merges, partial `a16` ids, an inconsistent cache.
- **Manual (owner):** PowerPoint opens edited tables (inserted rows in a
  vertical merge, merged ranges, borders) without repair; Q1 and Q2.

## Consequences

- Positive: tables can be extended and reshaped without losing cell
  formatting, merge consistency or comment anchors; SmartArt text can be
  updated and every consumer that renders the cache sees the new text.
- Negative: structural edits refuse malformed tables (≤ 2 of 119) and
  deletions through merge origins; SmartArt node structure stays
  read-only; cached sizes can be stale after longer text.
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

- **Q1** Does PowerPoint re-lay out SmartArt from the data model on open,
  or render the cache? (Decides whether `drop_drawing_cache` is safe for
  PowerPoint users.)
- **Q2** After longer node text, does PowerPoint re-fit the cached sizes
  on open or only on the next edit?
- **Q3** New rows inside a vertical merge join the merge (§2); PowerPoint's
  UI does the same — confirm on the manual-check deck.
- **Q4** Borders and fills in Phase 1b (§3) or all formatting in Phase 3?

## Sources

- Spike: `spikes/pptx-phase1b/tables_smartart.py`, output
  `results/tables_smartart-619.txt`.
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
