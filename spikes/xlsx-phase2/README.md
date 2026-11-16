# spikes/xlsx-phase2

Measurements behind [ADR-0006](../../docs/adr/0006-xlsx-phase2-shared-strings-and-styles.md)
(shared-string and style registries). Read-only: `zipfile` + `xml.etree`, parts with a DTD are
skipped, nothing is extracted or executed. Run with `python3 -I` from the repository root and
pass the data directories as arguments.

| Script | Purpose |
|---|---|
| `registry_survey.py DIR…` | One JSON line per package: `styles.xml` table sizes and `count` correctness, mandatory fills, custom `numFmt` ids/codes, features a value `Format` cannot express, projection collisions among existing `cellXfs`, vendor extensions, tints; `sharedStrings.xml` `count`/`uniqueCount` against the `si` children and the actual `t="s"` references, rich/phonetic items, duplicates by flattened text. |
| `registry_survey.py --summary DIR…` | The aggregate table below. |

Reproducing (POI files are not committed; Apache-2.0):

```sh
SHA=47dcff769bb395034dddfd59c4609659b6dbc6a4
gh api "repos/apache/poi/git/trees/$SHA?recursive=1" \
  --jq '.tree[]|select(.path|startswith("test-data/spreadsheet/"))|select(.path|test("\\.(xlsx|xlsm)$"))|.path' > poi.txt
mkdir -p "$SCRATCH/poi" && while read -r p; do
  curl -sfL -o "$SCRATCH/poi/$(basename "$p")" "https://raw.githubusercontent.com/apache/poi/$SHA/$p"; done < poi.txt
python3 -I spikes/xlsx-phase2/registry_survey.py --summary "$SCRATCH/poi"
python3 -I spikes/xlsx-phase2/registry_survey.py --summary reference/fixtures/xlsx reference/round_trip_corpus
```

## Results (2026-11-16, Python 3.15)

POI `test-data/spreadsheet` @ `47dcff76`: 367 listed, 366 downloaded, 346 open as packages,
4 Strict, **342 Transitional packages readable**. Committed fixtures: 24 packages.

| Fact | POI (342) | Fixtures (24) |
|---|---|---|
| with `styles.xml` | 340 | 24 |
| `fills[0]` = `none` and `fills[1]` = `gray125` | 325/340 (96%) | 22/24 |
| `numFmts` absent (a new custom format must insert the element) | 234/340 | 16/24 |
| a `count` attribute that does not match its children (any table) | ≤ 2 per table | 1 (Hancom `fonts`, see below) |
| `cellXfs` entries: median / p90 / max | 2 / 31 / 482 | 2 / 22 / 201 |
| custom `numFmt` with id < 164 (packages) | 32 | 2 |
| custom `numFmt` whose code equals a built-in code (packages) | 16 | 3 |
| duplicate custom `numFmt` codes (packages) | 3 | 0 |
| **`cellXfs` entries a value `Format` cannot express** | **859/5686 (15%)**, in 53/340 packages | 46/338, in 5/24 |
| … because of `xfId` ≠ 0 (named-style parent) | 47 packages | 3 |
| … because of `pivotButton` | 6 | 1 |
| … `justifyLastLine` / `relativeIndent` / foreign attributes | 1 / 1 / 1 | 0 |
| gradient fills, font `outline`/`shadow`, unusual colour attributes | 0 | 0 |
| **existing `cellXfs` that a `Format` projection maps to the same value although their XML differs** | **75/340 packages, 388 groups** (263: `apply*` flags only, 115: different `xfId`, 10: other) | 7/24, 13 groups |
| exact-duplicate `cellXfs` / `font` entries already present | 41 / 53 packages | 4 / 6 |
| `styles.xml` with `x14ac:knownFonts` / `mc:Ignorable` | 172 / 188 | 12 / 13 |
| `styleSheet/extLst` (`{EB79DEF2…}` x14 slicer styles / `{9260A510…}` x15 timeline styles) | 186 / 94 | 14 / 11 |
| `mc:AlternateContent` inside `styles.xml` | 1 | 1 (`style-alternate-content.xlsx`: Hancom wraps a `font` *entry* inside `fonts`) |
| custom `indexedColors` palette | 13 | 1 |
| custom named styles (`cellStyle` without `builtinId`) / duplicate style names | 35 / 4 | 2 / 0 |
| distinct `tint` spellings; not in rust_xlsxwriter's `Theme(color, shade)` table | 21; 8 (e.g. `0.399975585192419` vs `0.39997558519241921`) | 4; 0 |
| `workbookPr/@date1904` | 20 (19 Excel) | 0 |
| with `sharedStrings.xml` | 247 (95 without) | 19 |
| `sst/@uniqueCount` ≠ number of `si` | 3 | 1 |
| `sst/@count` absent / `uniqueCount` absent | 8 / 7 | 1 / 1 |
| **`sst/@count` ≠ actual `t="s"` references** | **18/247 (7%)**, 12 of them written by Excel | 1/19 |
| sst with rich items / phonetic items | 16 / 29 | 0 / 0 |
| sst with duplicate flattened text | 22/247 (9%); 11 mix rich/phonetic/plain XML | 0 |
| sst items with outer whitespace but no `xml:space="preserve"` | 1 | 0 |

Notes:

- The "cannot express" rule is the `Format` model of ADR-0006 §3 (rust_xlsxwriter's feature
  set plus raw theme/tint/indexed colours). An entry that references a font, fill or border the
  model cannot express counts as well.
- A "projection" is what a `Format` sees of an `xf`: the resolved number-format code, font,
  fill and border XML, alignment, protection and `quotePrefix`. It ignores `xfId` and the
  `apply*` flags. A group of distinct existing indices with the same projection is what a
  dedup-against-existing policy would merge silently. The 115 groups that differ in `xfId`
  differ in meaning: the cell is attached to a different named style.
- The `fonts` count mismatch in `style-alternate-content.xlsx` is an artefact of counting only
  direct `font` children. Hancom puts one entry inside `mc:AlternateContent`, which counts as
  one entry after MCE processing.
