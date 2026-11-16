# PowerPoint Phase 1b spike (table structure, SmartArt text)

Evidence for [ADR-0013](../../docs/adr/0013-pptx-phase1b-tables-and-smartart.md).
Throwaway, not production code. Reads untrusted `.pptx` files with `zipfile`
+ `xml.etree` only; nothing in them is executed. Run with `python3 -I` from a
directory that does not contain the files.

```text
python3 -I tables_smartart.py <dirs/files...>
```

Corpora: the ADR-0008 set (`spikes/pptx-phase1/README.md`: POI
`test-data/slideshow`, Open XML SDK `TestFiles`, python-pptx test files,
LibreOffice `sd/qa/unit/data/pptx` at `2e885d0bda9b`, ShapeCrawler assets at
`2ab474918ac8`) plus `reference/fixtures/pptx/`, downloaded to a scratch
directory and not committed. The exact inputs of the run, in this order:
POI `test-data/slideshow` (94 files, trunk of 2026-11-16), Open XML SDK
`TestFiles` (8), python-pptx `features/steps/test_files` (11), LibreOffice
`sd/qa/unit/data/pptx` (3 + 420 + 417 files at `2e885d0bda9b`), ShapeCrawler
`ShapeCrawler.Tests/Assets` (100 at `2ab474918ac8`), and the fixtures.
De-duplicated by SHA-256: **619** distinct readable presentations (445
duplicates, 12 unreadable); the corpus is pinned by content in
[`results/corpus-sha256.txt`](results/corpus-sha256.txt) (written when
`P1B_SHA_LIST` names a file), because a run over a different set (e.g. all
of a scratch directory) gives other totals. Output of the 2026-11-16 run:
[`results/tables_smartart-619.txt`](results/tables_smartart-619.txt).

Measured:

- **Tables:** sizes; merges (`gridSpan`, `rowSpan`, `hMerge`, `vMerge`),
  whether every origin's rectangle is flagged consistently, covered cells
  that also carry spans (the python-pptx/LibreOffice edge form) or text;
  rows whose `a:tc` count differs from the grid; frame `cx`/`cy` vs. the
  grid widths and row heights; `a16:rowId`/`a16:colId`; `tableStyleId`
  defined in `tableStyles.xml` or not; `tblPr` flags; `tcPr` attributes and
  children.
- **Revision 2 additions (review m5, M1, M3, M4):** the full
  well-formedness check of ADR-0013 rev 2 §2 (each covered cell owned by
  exactly one origin, flag kinds, edge spans; the form of every 2-D
  merge), the largest `a16:rowId` per table, shared borders (a cell's
  `lnR` vs its right neighbour's `lnL`, `lnB` vs `lnT`), `presOf`
  `destOrd` ties, nodes without `dgm:t`, the run-level invariant (paired
  paragraphs' run counts; data run attributes ⊆ cache run attributes) and
  the paragraph levels of aggregate shapes.
- **SmartArt:** frames, layout ids, point and connection types; data nodes
  with text, runs with `a:rPr` children, multi-run paragraphs, fields,
  placeholder nodes (`phldr`); the drawing cache (`dsp:dataModelExt`):
  present, empty, unresolved; and the cache invariant: **does each drawing
  shape show exactly the concatenation of the paragraphs of the data nodes
  whose `presOf` connection points at it (by `destOrd`)?** (321 of 326 text
  shapes yes; 5 no, all in one file.)

Source read for the ADR's comparison (not vendored): python-pptx 1.0.2
(`pptx/table.py`, `pptx/oxml/table.py`), Apache POI
`47dcff769bb395034dddfd59c4609659b6dbc6a4` (`XSLFTable`, `XSLFTableRow`,
`XSLFTableCell`, `XSLFDiagram`), LibreOffice
`2e885d0bda9bc4ee9f3bde51905c4a7820b01f06`
(`oox/source/drawingml/diagram/diagram.cxx`,
`oox/source/drawingml/table/tableproperties.cxx`,
`oox/source/export/shapes.cxx`).
