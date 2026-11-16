# PowerPoint Phase 3 spike (formatting writes and links)

Evidence for [ADR-0016](../../docs/adr/0016-pptx-phase3-formatting-and-links.md).
Throwaway, not production code. Reads untrusted `.pptx` files with `zipfile`
+ `xml.etree` only; nothing in them is executed. Run with `python3 -I` from a
directory that does not contain the files.

```text
python3 -I formatting.py <dirs/files...>
```

Corpora: the ADR-0008 set (see `spikes/pptx-phase1/README.md`: POI, Open XML
SDK, python-pptx, LibreOffice `sd/qa/unit/data/pptx`, ShapeCrawler) plus
`reference/fixtures/pptx/`, downloaded to scratch, not committed;
de-duplicated by SHA-256: 619 presentations, 1,269 slides. Output of the
2026-11-16 run: [`results/formatting-619.txt`](results/formatting-619.txt).

Measured (slides only, unless noted): `a:rPr`/`a:endParaRPr`/`a:defRPr`
attributes and children, run colour kinds and transforms, typeface kinds
(explicit vs `+mj`/`+mn`), `a:pPr` attributes and children (alignment,
spacing units, bullet kinds, auto-number schemes), `a:bodyPr` autofit,
`p:spPr`/`p:grpSpPr` fills, lines, preset geometries and adjustments,
custom geometry, rotation/flip, missing `a:xfrm`, group child spaces (equal
to the group's own space or scaled/offset), hyperlinks (where they sit,
actions, relationship types, empty `r:id`, external schemes, `ahyp:hlinkClr`,
attributes), notes slides and notes masters, `tableStyles.xml` definitions
and the parts they use, theme colour slots and fonts, theme sharing,
`p:clrMap` and slide overrides.

Source read for the ADR's comparison (not vendored): python-pptx 1.0.2,
Apache POI `47dcff769bb395034dddfd59c4609659b6dbc6a4` (`XSLFTextRun`,
`XSLFHyperlink`, `XSLFGroupShape`, `XSLFTheme`, `XMLSlideShow`), LibreOffice
`2e885d0bda9bc4ee9f3bde51905c4a7820b01f06`
(`oox/source/drawingml/table/predefined-table-styles.cxx`). The preset count
(187 shapes, 123 with adjustment guides) is from `presetShapeDefinitions.xml`
in ECMA-376 Part 1's `OfficeOpenXML-DrawingMLGeometries.zip`.
