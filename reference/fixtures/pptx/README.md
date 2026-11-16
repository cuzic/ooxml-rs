# PowerPoint fixtures (third-party, unmodified)

23 real `.pptx` files (2,048,535 bytes in total) used by
[ADR-0005](../../../docs/adr/0005-pptx-phase0.md) and the
[PowerPoint roadmap](../../../docs/roadmap/pptx.md). All are copied
**unmodified** from public test corpora; none was authored by this project.
Treat them as untrusted input: inspect with `python3 -I` scripts from
`spikes/pptx/`, never open them with a macro-capable application.

Licences (full texts in [`LICENSES/`](LICENSES/)):

| Directory | Source (commit, retrieved 2026-11-16) | Licence |
|---|---|---|
| `poi/` | [apache/poi](https://github.com/apache/poi) `test-data/slideshow/` (trunk `47dcff769bb3`) | Apache-2.0; `LICENSES/apache-poi-LICENSE.txt` and the required `LICENSES/apache-poi-NOTICE.txt` |
| `python-pptx/` | [scanny/python-pptx](https://github.com/scanny/python-pptx) `features/steps/test_files/` (master `278b47b1dedd`) | MIT, `LICENSES/python-pptx-LICENSE.txt` |
| `open-xml-sdk/` | [dotnet/Open-XML-SDK](https://github.com/dotnet/Open-XML-SDK) `test/DocumentFormat.OpenXml.Tests.Assets/assets/TestFiles/` (main `431ab05cf160`) | MIT, `LICENSES/open-xml-sdk-LICENSE.txt` |
| `libreoffice/` | [LibreOffice/core](https://github.com/LibreOffice/core) `sd/qa/unit/data/pptx/` (master `2e885d0bda9b`) | MPL-2.0 (the repository's licence for these files), `LICENSES/libreoffice-MPL-2.0.txt`; the files are unmodified, so their source form is the file itself |

The original authorship of individual test files (many are attachments to
bug reports) was not traced beyond the repository that distributes them.
The POI files were cross-checked against the git blob SHA-1 that the GitHub
API reported for `test-data/slideshow/` (all 15 match).

## Files

"App" is `docProps/app.xml` `Application AppVersion`. Feature counts are
from `spikes/pptx/survey.py` (slide parts only).

| File | Bytes | App | What it exercises |
|---|---:|---|---|
| `poi/sample.pptx` | 47,215 | PowerPoint 12 | 2 slides, 4 placeholders, notes; simplest baseline; last-paragraph `Err` |
| `poi/layouts.pptx` | 62,901 | PowerPoint 14 | 10 slides on different layouts, 27 placeholders, an `a:fld` (`FieldText` `Err`) |
| `poi/with_japanese.pptx` | 48,011 | PowerPoint 12 | CJK text, 3 multi-run paragraphs, table, 5 `a:hlinkClick` |
| `poi/shapes.pptx` | 68,822 | PowerPoint 14 | preset + custom geometry, 3 connectors, group, 3 tables (merged cells), hyperlinks |
| `poi/sample_pptx_grouping_issues.pptx` | 39,436 | PowerPoint 12 | 11 nested group shapes |
| `poi/table_test2.pptx` | 33,414 | PowerPoint 16 | table with 16 merged-away cells and 13 spans (`MergedCell` `Err`) |
| `poi/bar-chart.pptx` | 44,410 | Mac PowerPoint 15 | chart part + embedded workbook (passthrough) |
| `poi/SmartArt.pptx` | 41,382 | PowerPoint 16 | SmartArt (`dgm` parts + drawing cache, passthrough) |
| `poi/45545_Comment.pptx` | 308,394 | PowerPoint 12 | legacy comments + `commentAuthors.xml`, 11 notes slides, OLE, `p:bldP build="p"` / `p:pRg` (`AnimatedParagraphs` `Err`) |
| `poi/bug65551.pptx` | 34,383 | PowerPoint 16 | 10 `p14:section`s, comment authors, notes |
| `poi/bug68703.pptx` | 60,711 | Mac PowerPoint 16 | AC-wrapped `p:transition` followed by a direct `p:timing` under `p:sld`; AC under layouts/master; table with a merged cell |
| `poi/picture-transparency.pptx` | 11,793 | **LibreOffice 25.8** | non-PowerPoint producer; AC-wrapped transitions on 4 slides |
| `poi/placeholder-layout-color.pptx` | 346,876 | Mac PowerPoint 16 | 3 embedded fonts; placeholder colour inherited from layout |
| `poi/EmbeddedAudio.pptx` | 90,047 | Mac PowerPoint 16 | embedded audio (`a:audioFile` + `p14:media`), timing |
| `poi/bug62513.pptx` | 384,775 | PowerPoint 14 | 19 slides, 18 OLE objects, AC in `graphicData`; `cNvPr id="0"` three times on slide 17, all inside OLE fallbacks in `a:graphicData` (**not** duplicate shape ids, ADR-0005 review M3); `a:normAutofit` caches (manual check 02) |
| `python-pptx/ph-inherit-props.pptx` | 23,566 | Mac PowerPoint 14 | placeholders whose geometry comes from the layout (Phase 1 oracle) |
| `python-pptx/shp-connector-props.pptx` | 22,899 | Mac PowerPoint 14 | connectors referencing shape ids |
| `python-pptx/sld-notes.pptx` | 22,964 | Mac PowerPoint 14 | slide with and without a notes slide |
| `open-xml-sdk/animation.pptx` | 100,209 | PowerPoint 14 | animations (`p:timing`) and an AC-wrapped transition |
| `open-xml-sdk/Of16-01.pptx` | 58,675 | PowerPoint 16 | Office 2016 `cx:chart` in `mc:AlternateContent` **directly under `p:spTree`**, picture fallback with the same `cNvPr@id` (`InsideAlternateContent` `Err`) |
| `libreoffice/modern-comments.pptx` | 29,789 | PowerPoint 16 | modern (p188) comments, `authors.xml`, anchors `pc:sldMk@sldId` and `ac:spMk@id` |
| `libreoffice/airplane-transition.pptx` | 29,755 | PowerPoint 16 | `p15:prstTrans` in AC; only slide part is `slide2.xml` (part names are not `1..N`) |
| `libreoffice/slide-sections.pptx` | 138,108 | PowerPoint 16 | 2 sections, 7 notes slides, 18 groups, 36 custom geometries, `sldId` gaps (309…404) |

## Checksums (SHA-256)

```
0295a51f63150e3ee680154d07dab88062ba2306030b790561f85436f3ded7c8  poi/45545_Comment.pptx
c5ae4274e2bf5504a56aef9c8d7c5d2381ece69c9bf68f7749ad5eae3e675edb  poi/EmbeddedAudio.pptx
b97e4c6d2ee1dd4094f50f9043820610268dd452d7717c537f5456636adcc353  poi/SmartArt.pptx
79e1d218bfb2903e8dc8425a6b1997d9c1976f5a5f025bada85b0c47b5777969  poi/bar-chart.pptx
cd841112bd5b53f21e8434d080af8b2eeca78a07bc820e9ea0b97a0962c85c79  poi/bug62513.pptx
45f9ddb5ef78d4da97c220dbf5e1aedcafcfddf338222ef958545c773d19e27c  poi/bug65551.pptx
f232d0dc390c741855b4395f74a11d2610243fffd8699f72dd3185ad5f930852  poi/bug68703.pptx
9c3d53afa3115de2ba72ec77637a616aeed505a6ff3b30a46db871c4edd32be5  poi/layouts.pptx
0ce76411a6e2fe3a5b8acf42722b973131a91b93cb5c3118a5a9c481b063e386  poi/picture-transparency.pptx
b683af99cf4e71db112fe87db15840846c53dbc7a6839cbf84804980d21af9ce  poi/placeholder-layout-color.pptx
108d7219130a8460298896f45ff777dfe52b8b5527a4b4628b10731ed01d6a8e  poi/sample.pptx
a8639e12e763a01d889a6178e154c21db2dd4275f8f35ac81a458f81e26b2c18  poi/sample_pptx_grouping_issues.pptx
19fde9b87e33dd1a95fdbba0cf6abc2278bf03874f4665c7f8b88b6afe4a2571  poi/shapes.pptx
e80d09a5fa767f6ee2d024f3a4198f1e8b3241cc638183e5ebafdbacf1fb2838  poi/table_test2.pptx
f0b1efbfbd70f7ee2395dc2505639f99449971ba404a6ec8cf1b97d82cf45c10  poi/with_japanese.pptx
a9b1c1f57c93d43bac6f7ef0c87b7ba34a54950262531f55d19f40b28ca2653a  python-pptx/ph-inherit-props.pptx
07342c67908b5800b21cc4e035a9914fae7442cbfb406c7bc96d312b2c5e85f1  python-pptx/shp-connector-props.pptx
9a83da2e319161c1e2993de878f65b57ec10e14e3ef418b46354c48350866856  python-pptx/sld-notes.pptx
563b633c6a696faa6efe058b15a6bd6f3fa955c2286e4fe0160ef8a7e998e976  open-xml-sdk/Of16-01.pptx
b81bda36d34258d78329187df6beb99d12bba5a3bd25e7a51506ede016a72191  open-xml-sdk/animation.pptx
0450cf856d80ea8826540cceadb2c4b13fef28b7c613fd4db657611fa804c600  libreoffice/modern-comments.pptx
27eed094a47dfe56617061987b81520b1ae6af73f5702fd05cf656a9da64707c  libreoffice/airplane-transition.pptx
2e2d1a6cfa6c282a2c81f92c74c704716357155741c9ed0abd9815aeac684369  libreoffice/slide-sections.pptx
```

Verify with `sha256sum -c` after extracting the block above, run from this
directory.

## Synthetic test inputs

Constructs that no file here (or in the 105-file survey corpus) contains
are tested on inputs **derived in memory** from these files by one minimal
XML edit each, in `crates/ooxml-pptx/tests/refusals.rs` (marked
`SYNTHETIC:`): `conformance="strict"`, a dangling `sldId@r:id`, a duplicated
and a missing `cNvPr@id`, an `a14:m` inside `a:p`, `p:pRg` → `p:charRg`,
a text-range comment anchor (`ac:deMkLst` → `ac:txMkLst` + `ac:txMk`), an
unknown comment anchor, a malformed comment part, the DrawingML namespace
declared per element. Nothing derived is written to this directory, and the
fixtures are never packaged into a crate (they are read by relative path).

## Known gaps

- No modern comment anchored to a **text range** (`ac:txMk`; 0 in the
  105-file corpus, `spikes/pptx/probe.py`); covered by a synthetic input
  (above); a real one is still wanted from third-party corpora.
- No custom show (`p:custShow`) in any of the 105 surveyed files.
- No Strict-conformance or encrypted `.pptx` (ADR-0001's `Err` cases need a
  separate fixture; the SDK's `encrypted_pptx.pptx` is a candidate).
- All PowerPoint-produced except one LibreOffice file; Google Slides and
  Keynote outputs are not represented.
