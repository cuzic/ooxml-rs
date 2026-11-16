# PowerPoint Phase 2 spike (slides and shapes as units)

Evidence for [ADR-0012](../../docs/adr/0012-pptx-phase2-slides-and-shapes.md).
Throwaway, not production code. Reads untrusted `.pptx` files with `zipfile`
+ `xml.etree` only; nothing in them is executed. Run with `python3 -I` from a
directory that does not contain the files.

```text
python3 -I units.py <dirs/files...> [--json out.json]
```

Corpus of the 2026-11-16 run: Apache POI `test-data/slideshow/*.pptx`
(94 files, fetched for the PowerPoint plan; not committed) plus the committed
fixtures `reference/fixtures/pptx/`. Files are de-duplicated by SHA-256: 91
distinct readable files (15 duplicates skipped, 11 unreadable), 522 slides.

What it measures: slide ids (gaps, order vs. id, range), presentation `rId`
patterns, slide/notes/media part naming, content types (`Default` vs
`Override`), layout use, notes sharing and back-relationships, media sharing
across slides and with layouts/masters, sections (`p14:sectionLst`: invariant
"sections concatenate to `sldIdLst`", empty sections), custom shows and
`presProps` show ranges, slide-to-slide links, slide relationship types (and
which slides the ADR's delete cascade and duplicate type set accept), shape
ids per slide (duplicates outside `mc:AlternateContent`, sparseness, non-shape
`@id`s), `p14:creationId`, hidden slides, and `docProps/app.xml` counts and
heading labels.

Output of the run (abridged; all of it is quoted in the ADR's Context table):

```text
files: 91            slides: 522          shapes (outside AC): 4089
files whose lowest sldId != 256: 26      files with sldId gaps: 18
files with sldIds not ascending in order: 20
pres rIds all rIdN: 91 (contiguous 1..n: 89, with gaps: 2)
files whose slide part numbers != 1..n in order: 1   (airplane-transition.pptx: [2])
notes part number != its slide part number: 101
slides typed by Override: 522    slide media typed by Default: 149 / Override: 8
layout parts: 1136 (used by no slide: 959)   files with >1 master: 9
slides with notes: 227 (none shared, back-rel always its slide)  notes master: 36 files
media parts used by slides: 157 (shared by >1 slide: 15; also by a layout/master: 1)
files with p14:sectionLst: 4  sections: 17  empty sections: 12
  slides outside every section: 0   section order != sldIdLst: 0
p:custShowLst: 0   presProps showPr/sldRg: 0
slide-to-slide relationships: 22   slides that are jump-link targets: 3
slides deletable by the ADR-0012 cascade: 519 (blocking: chartEx 1, control 2)
slides duplicable (ADR-0012 types): 457
duplicate shape ids (outside AC): 0   sparse ids: 257 slides
slides where some non-shape @id exceeds max shape id: 5
slides with p14:creationId: 172   hidden slides: 3
app.xml Slides == count: 86 (4 without Slides, 1 without app.xml)
  slide-titles heading: 78 (count == slides: 77); labels localized
  (Slide Titles, Folientitel, Bildrubriker, Заголовки слайдов, 幻灯片标题, …)
```

Reference-implementation source read for the ADR's comparison table:
python-pptx 1.0.2 (installed with `pip --target` into a scratch directory)
and Apache POI at commit `47dcff769bb395034dddfd59c4609659b6dbc6a4`
(`XMLSlideShow`, `XSLFSheet`, `XSLFSlideLayout`, `PackagePartCollection`,
`POIXMLDocumentPart`), fetched as raw files; neither is vendored.
