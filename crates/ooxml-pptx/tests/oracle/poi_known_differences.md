# ADR-0008 §7 O2b — known differences from Apache POI (`47dcff769bb3`)

The optional POI differential (ADR-0008 §7 O2b) compares `sz`, `b`, the
latin typeface, the fill and `algn` of every fixture paragraph with POI's
`XSLFTextRun` / `XSLFTextParagraph` getters. A difference **listed here** is
expected and does not fail the job; any other difference does. The job needs
a JVM, which this repository's CI and sandbox do not have yet, so it is not
wired up; this table is the contract for whoever adds it.

| # | Rule | ooxml-pptx (ADR-0008 rev 3) | POI | Where in POI |
|---|---|---|---|---|
| 1 | Level gap in `p:txStyles` | exact level only (Q3) | walks down to lower levels | `XSLFTextParagraph.getDefaultMasterStyle` |
| 2 | Slide placeholder without `@idx` | effective idx 0, ties broken by type/family (§1.2) | matches by type only when `@idx` is absent | `XSLFSheet.getPlaceholder(CTPlaceholder)` |
| 3 | Shape style | `p:style/a:fontRef` is step 4 (typeface, colour) | used for the colour only, as `phClr` substitute | `XSLFTextRun.fetchFontColor`, `XSLFShape.selectPaint` |
| 4 | Notes text style | `notesStyle` for the body family only (§1.3) | `notesStyle` for every type | `getDefaultMasterStyle` |
| 5 | Master placeholder | by the layout placeholder's family | by the slide placeholder's type | `XSLFShape.fetchShapeProperty` l. 341–362 |
| 6 | `p:defaultTextStyle` for placeholders | step 8 (ECMA §19.2.1.8) | never (`getThemeProps` returns `null` for placeholders) | `ParagraphPropertyFetcher.getThemeProps` |
| 7 | Shapes on a master | steps 1–4, then `txStyles` (§1.7; not implemented in this PR) | steps 2–5 skipped | `CharacterPropertyFetcher.fetchProperty` |
| 8 | Values | as specified (no autofit scale, no colour transforms) | rendered (`getFontSize` applies `fontScale`) | `XSLFTextRun.getFontSize` |

In the 633-file corpus, rule 6 decides `marL` 327, `b` 5, `sz` 4, fill 4,
latin 3 and `algn` 3 values (`spikes/pptx-phase1/results/chain-633.txt`).
