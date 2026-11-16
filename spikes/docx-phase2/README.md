# Word Phase2 spike (evidence for ADR-0011)

Throwaway scripts; nothing here is production code. Every `.docx` input is untrusted data:
it is read only with `zipfile` + `ElementTree` (expat, no external entities) under
`python3 -I`. Packages with a DTD in any part are skipped, and so is element nesting deeper
than 256, as in core (ADR-0004 O2, O5). Run the scripts from a directory other than the
inputs'. `p2common.py` is a copy of `spikes/docx-phase1/p1common.py` (PR #25, not yet on
main). Nothing downloaded is executed.

## Corpora

Same as the Phase1 spike (`spikes/docx-phase1/README.md`):

| Name | Files | Open | Pass Phase0 gates |
|---|---|---|---|
| `fx`: `reference/fixtures/docx/` | 26 | 22 | 19 |
| `poi`: Apache POI `test-data/document`, `47dcff76` | 129 | 111 | 101 |
| `sdk`: Open XML SDK non-Strict assets, `431ab05c` | 442 | 440 | 404 |
| `d4j`: docx4j samples | 12 | 12 | 10 |
| `tpl`: python-docx-template `1f143fbe` `tests/templates` and docxtemplater `e1d61c64` `examples` | 212 | 212 | 212 |

`tpl` contains template engines' generated *outputs* as well (docxtemplater's `expected-*`).
That is where several uniqueness violations below come from: real files written by tools
other than Word.

## Scripts

| Script | Measures | ADR-0011 |
|---|---|---|
| `registries.py` | Every ID space and registry. **Drawing ids**: `wp:docPr/@id` duplicates per part, across parts, explained by `mc:AlternateContent` branches; value range; `pic:cNvPr/@id`. **Bookmarks**: id and name duplicates, case-only duplicates, names over 40 characters, hidden names. **Annotation ids**: the union of `w:id` over bookmarks, comments, revisions and permissions, cross-category collisions. **`w14:paraId`/`textId`**. **Numbering**: `num`/`abstractNum` ids, gaps, dangling references, `numId` 0, `nsid`, overrides. **Relationships**: ids, gaps, externals, relationships unused by their source. **`[Content_Types].xml`**. **Media**: naming, sharing, linked, SVG. **Part naming**. **Sections**. **Table grids and merges**. **Hyperlinks**. **Styles** | §2–§8 |
| `review1.py` (revision 2) | The facts behind the round-1 review: restart settings in `sectPr`, `a:graphicFrameLocks` declarations, references to undefined style/num/relationship ids, dangling targets, style names that strip to empty, `a`/`aN` ids, spans and merges in cells, notes and revisions in table rows, other `cNvPr` ids, `sdtPr/w:id`, `numIdMacAtCleanup`, `tblHeader` | §1–§8 |
| `needs.py` | (Its part-name prototype is revision 1's smallest-free rule. Since revision 2 the core API's max + 1 rule applies; the other allocations are unchanged.) Per writable file, what a first list, style, header or image would have to create (numbering part, Default for `png`/`jpeg`, …). A prototype of every allocation rule in §2, checked for freshness, with the largest value allocated (headroom against 2³¹) | §1, §2 |

```sh
S=spikes/docx-phase2; C=<corpus dir>
python3 -I $S/registries.py --json out/rg_poi.json $C/poi
python3 -I $S/needs.py --json out/nd_poi.json $C/poi
```

## Results (2026-11-16, Python 3.15)

"files" = packages with at least one occurrence; other counts are occurrences.

### Drawing object ids (`wp:docPr/@id`)

| | fx | POI | SDK | d4j | tpl |
|---|---|---|---|---|---|
| files with drawings / `docPr` elements | 5 / 43 | 20 / 87 | 85 / 393 | 6 / 8 | 45 / 80 |
| duplicate within a part, not explained by AC branches | 0 | 0 | 0 | 0 | 0 |
| duplicate across parts (document, headers, notes, …) | 0 | 0 | 0 | 0 | 0 |
| duplicate between the two branches of one `mc:AlternateContent` (files) | 0 | 1 | 8 | 0 | 0 |
| `docPr id="0"` (files) | 0 | 1 | 0 | 0 | 0 |
| largest id: ≤ 100 / ≤ 10⁴ / < 2³¹ (files) | 4 / 1 / 0 | 18 / 1 / 0 | 84 / 0 / 1 | 6 / 0 / 0 | 42 / 0 / 3 |
| `pic:cNvPr id="0"` / all `pic:cNvPr` | 31 / 32 | 54 / 63 | 265 / 285 | 6 / 6 | 16 / 42 |

The large ids are 1,012,874,200 (SDK) and up to 1,073,741,825 (tpl): those producers use
random-looking values. "max + 1" stays below 2³¹ in every file (needs.py).

### Bookmarks and annotation ids

| | fx | POI | SDK | d4j | tpl |
|---|---|---|---|---|---|
| `bookmarkStart` (files) | 236 (9) | 163 (47) | 2,014 (140) | 23 (8) | 143 (99) |
| duplicate id in a part (files) | 0 | 0 | 0 | 0 | 5 |
| duplicate id across parts | 0 | 0 | 0 | 0 | 0 |
| duplicate name, exact (files) | 0 | 1 (`__DdeLink__…`) | 0 | 0 | 8 (incl. `_GoBack`) |
| names equal ignoring case only | 0 | 0 | 0 | 0 | 0 |
| names over 40 characters | 0 | 0 | 1 | 0 | 0 |
| hidden (`_…`) names | 13 | 64 | 724 | 21 | 123 |
| id collisions between bookmarks, comments, revisions, permissions (files) | 0 | 0 | 0 | 0 | 2 (bookmark/comment `0`) |

Note and endnote ids are negative for the separators (`-1`, `0`) in every notes part. They are
a separate space (§2).

### `w14:paraId` / `w14:textId`

| | fx | POI | SDK | tpl |
|---|---|---|---|---|
| files with `paraId` | 8 | 16 | 66 | 37 |
| duplicates explained by AC branches (files) | 1 | 1 | 6 | 2 |
| other duplicates (files) | 1 | 1 | 0 | 5 |
| `paraId` or `textId` ≥ 0x80000000 | 0 | 0 | 0 | 0 |
| `textId="77777777"` (occurrences / all `paraId`) | 835 / 843 | 188 / 210 | 3,070 / 3,255 | 388 / 514 |

The other duplicates are copy-pasted table rows and paragraphs (`poi-table-alignment`) and
the tpl generated outputs. One random draw below 2³¹ was fresh in every file.

### Numbering (`numbering.xml`)

| | fx | POI | SDK | tpl |
|---|---|---|---|---|
| files with a numbering part (opened / writable) | 5 / 4 | 23 / 21 | 113 / 92 | 34 / 34 |
| `num` / `abstractNum` | 47 / 47 | 327 / 310 | 1,337 / 1,106 | 219 / 191 |
| duplicate `numId` or `abstractNumId`; gaps; `num` → undefined `abstractNum`; `numPr` → undefined `numId` | 0 | 0 | 0 | 0 |
| `numPr` with `numId="0"` (in document / styles) | 0 / 0 | 0 / 5 | 58 / 9 | 43 / 13 |
| `num` with `lvlOverride` / with `startOverride` (list restart) | 0 / 0 | 9 / 7 | 18 / 15 | 26 / 24 |
| `abstractNum` used by no `num` | 0 | 0 | 10 | 0 |
| duplicate `nsid` | 0 | 0 | 0 | 0 |
| `numIdMacAtCleanup` (files) | 0 | 3 | 13 | 2 |
| `abstractNum` with `styleLink` / `numStyleLink` | 0 / 0 | 28 / 0 | 16 / 2 | 0 / 0 |
| `multiLevelType` hybrid / multi / single / absent | 5 / 42 / 0 / 0 | 120 / 142 / 40 / 8 | 636 / 129 / 341 / 0 | 41 / 18 / 85 / 47 |

Largest `numId` + 1 over all files: 100 (SDK); largest `abstractNumId` + 1: 104.

### Relationships and content types

| | fx | POI | SDK | tpl |
|---|---|---|---|---|
| relationships / of shape `rIdN` | 586 / 580 | 1,673 / 1,666 | 6,654 / 6,629 | 2,538 / 2,480 |
| files with ids not `rIdN` (all `R` + 16 hex digits, e.g. `R44c83fbb49ca402c`) | 2 | 3 | 5 | 6 |
| duplicate Id (case-insensitive) | 0 | 0 | 0 | 0 |
| `rIdN` numbers not exactly `1..n` (files) | 1 | 1 | 0 | 2 |
| external: hyperlink / image / attachedTemplate / mailMergeSource | 307 / 0 / 2 / 0 | 357 / 0 / 4 / 1 | 208 / 1 / 6 / 2 | 12 / 0 / 4 / 0 |
| image, header, footer or hyperlink relationships whose Id the source part never mentions (files) | 0 | 1+1 | 0 | 8+2+2+1 |
| media registered by `Default` / `Override` (files) | 3 / 0 | 18 / 1 | 106 / 2 | 22 / 13 |
| parts with neither `Default` nor `Override` (files) | 0 | 1 | 0 | 0 |
| `Override` naming a missing part | 0 | 0 | 0 | 0 |

needs.py (writable files): `png` Default present in fx 2/19, POI 15/101, SDK 28/404, tpl 60/212;
`jpeg`/`jpg` in 1/19, 20/101, 47/404, 65/212. An `xml` Default is missing in 48 of 212 tpl
files. Largest `rId` allocated: 321 (POI/fx), 286 (SDK).

### Media and images

| | fx | POI | SDK | d4j | tpl |
|---|---|---|---|---|---|
| image relationship targets named `word/media/<name>N.<ext>` / otherwise | 27 / 0 | 56 / 1 | 436 / 1 | 9 / 0 | 82 / 0 |
| one media part targeted by several relationships (files) | 0 | 2 | 6 | 0 | 8 |
| one image `rId` used by several drawings in a part (files) | 1 | 4 | 7 | 0 | 3 |
| parts with `wp:inline` / `wp:anchor` (files) | 4 / 2 | 15 / 6 | 71 / 36 | 4 / 2 | 16 / 29 |
| linked images (`r:link`, external) | 0 | 0 | 1 | 1 | 0 |
| `asvg:svgBlip` | 0 | 0 | 1 | 0 | 0 |
| VML `v:imagedata` (files) | 1 | 4 | 29 | 0 | 6 |

Namespaces on the main part's root (writable files): `wp` is declared in 17/19, 96/101,
398/404 and 212/212; `a` in 0/19, 1/101, 1/404 and 2/212; `pic` in 0, 4, 1 and 11. Word
declares `a` on `a:graphic` and `pic` on `pic:pic`, locally.

### Parts, sections, headers

- Writable files with a numbering part: fx 4/19, POI 21/101, SDK 92/404, tpl 34/212. A styles
  part: 17/19, 97/101, 404/404, 212/212. A settings part: all but 2 POI. A header part: 3/19,
  29/101, 63/404, 86/212.
- Header/footer parts not named `word/headerN.xml`/`footerN.xml` (files): fx 1, POI 1, tpl 4.
  Next free number: up to `header20.xml` (SDK).
- Parts targeted by no relationship: POI 1, tpl 1.
- `stylesWithEffects.xml`: fx 5, POI 18, SDK 66, tpl 27 files.
- `sectPr` per file > 1: POI 2, SDK 39, tpl 18 files. `sectPr` without any header/footer
  reference (inherits): POI 88 of 123, SDK 480 of 613, tpl 146 of 247. `titlePg` on: 6, 71,
  12. `evenAndOddHeaders` on: 2, 11, 0 files.

### Tables

| | fx | POI | SDK | d4j | tpl |
|---|---|---|---|---|---|
| tables / rows / cells | 21 / 213 / 1,193 | 125 / 3,158 / 16,243 | 382 / 2,905 / 13,448 | 15 / 55 / 181 | 108 / 247 / 684 |
| rows whose grid width (gridBefore + Σ gridSpan + gridAfter) ≠ number of `gridCol` | 0 | 0 | 0 | 0 | 0 |
| tables without `tblGrid` | 0 | 1 | 0 | 0 | 0 |
| cells with `gridSpan` > 1 | 7 | 7,534 | 1,658 | 45 | 71 |
| `vMerge` restart / continue | 1 / 1 | 1,830 / 1,867 | 180 / 243 | 8 / 8 | 22 / 24 |
| `vMerge` continue with no merged cell above at that grid column | 0 | 0 | 0 | 0 | 1 |
| rows with `w14:paraId` | 53 | 37 | 54 | 0 | 29 |
| rows holding cell-level `w:sdt`/`w:customXml` (files) | 2 | 4 | 56 | 0 | 4 |
| row-level tracked insert/delete | 0 | 0 | 3 | 0 | 0 |
| tables containing nested tables (files) | 0 | 3 | 12 | 0 | 5 |

(Cells wrapped in cell-level content controls are counted; without them, 260 SDK rows seemed
to have the wrong grid width.)

### Hyperlinks

| | fx | POI | SDK | tpl |
|---|---|---|---|---|
| `w:hyperlink` with `r:id` / with `w:anchor` | 304 / 117 | 351 / 113 | 194 / 404 | 10 / 0 |
| one relationship shared by several `w:hyperlink` | 0 | 0 | 0 | 0 |
| several relationships with the same external target (files) | 1 | 5 | 8 | 0 |
| `HYPERLINK` fields (files) | 2 | 2 | 2 | 0 |

Producers write one relationship per hyperlink and never share one.

### Review round 1 facts (`review1.py`, revision 2)

| | fx | POI | SDK | d4j | tpl |
|---|---|---|---|---|---|
| `sectPr` with `pgNumType/@start` (files) | 0 | 6 (6) | 23 (8) | 0 | 11 (11) |
| `sectPr` with `footnotePr`/`endnotePr` `numRestart` | 0 | 0 | 20 / 1 | 0 | 0 |
| `a:graphicFrameLocks` declaring `xmlns:a` / not | 34 / 0 | 60 / 0 | 290 / 1 | 6 / 0 | 40 / 9 |
| references to undefined style ids (`pStyle` / `link`) | 0 / 0 | 22 / 1 | 0 / 0 | 0 / 0 | 0 / 8 |
| `numId` references to undefined `num` | 0 | 0 | 0 | 0 | 0 |
| `r:*` attributes / to an undefined relationship | 378 / 0 | 578 / 0 | 1,445 / 0 | 20 / 0 | 384 / 0 |
| dangling internal relationship targets (files) | 0 | 0 | 26 (19) | 0 | 0 |
| style names that strip to empty (ASCII alphanumerics) | 7 | 89 | 0 | 0 | 30 |
| style ids of the form `a`/`aN` | 37 | 131 | 131 | 0 | 16 |
| cells with `gridSpan` > 1 / `vMerge` / `hMerge` | 7 / 2 / 0 | 7,534 / 3,697 / 0 | 1,658 / 423 / 0 | 45 / 16 / 0 | 71 / 46 / 0 |
| table rows holding a footnote/endnote reference (files) | 0 | 3 | 9 (3) | 0 | 0 |
| table rows holding revision elements (files) | 0 | 0 | 3 (3) | 0 | 0 |
| `cNvPr/@id` other than `pic:` (`wps:` / DrawingML main) | 6 / 0 | 8 / 0 | 0 / 29 | 0 / 0 | 10 / 0 |
| `sdtPr/w:id` / negative | 9 / 0 | 76 / 31 | 1,948 / 21 | 8 / 0 | 42 / 12 |
| `numIdMacAtCleanup` ≥ max `numId` (files) | 0 | 0 | 4 | 0 | 0 |
| rows with `trPr/tblHeader` | 7 | 33 | 138 | 2 | 7 |
