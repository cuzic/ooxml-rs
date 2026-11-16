# spikes/xlsx-phase5

Corpus evidence for [ADR-0017](../../docs/adr/0017-xlsx-phase5-objects.md) (xlsx Phase5,
drawing-layer and object features). The script only *reads* untrusted files with `zipfile` and
`xml.etree.ElementTree` (VML with regular expressions, since it is not always XML); nothing is
extracted or executed. Run it with `python3 -I` from the repository root, passing data
directories as arguments.

| Script | Purpose |
|---|---|
| `objects_survey.py [--summary] DIR…` | One JSON line per package. **Drawings**: anchor kinds, `editAs`, object kinds, `mc:AlternateContent` around and inside anchors, `cNvPr/@id` uniqueness per drawing and repetition across drawings, `id="0"`, media formats, shared and duplicate media. **Charts**: types, `c:f` (quoted), caches, style/colors parts, AC, chartEx, chart sheets. **Notes**: authors, comments, VML well-formedness, shape kinds, `o:idmap` blocks (block-level duplicates), note ↔ comment pairing. **Shape numbers per sheet** (ADR-0017 review B2): VML `id="…_sN"` on any element and `o:spid`, `control`/`oleObject` `shapeId`, drawing `cNvPr` in every `mc:AlternateContent` branch; numbers outside the sheet's blocks, negative numbers, sheets where drawing and VML numbers coincide, and sheets where revision 1's rule (1 + max over `id` only) would collide. **Threaded comments** and **persons**. **Tables**: ids (workbook-wide uniqueness, gaps), names vs defined names, header/totals rows, totals functions, calculated columns, table types, column ids, **header cell text vs `tableColumn/@name`**. **Sparklines**, **dynamic arrays** (`cm`, `metadata.xml` XLDAPR) and **rich values** (`vm`, `richData`). `--summary` adds `#` lines: files and totals per counter, and the common part-name and id patterns. |

Reproducing the ADR's numbers (files are not committed; Apache-2.0 / MIT):

```sh
# fetch POI test-data/spreadsheet at the pinned commit (as in spikes/xlsx/README.md)
SHA=47dcff769bb395034dddfd59c4609659b6dbc6a4
gh api "repos/apache/poi/git/trees/$SHA?recursive=1" \
  --jq '.tree[]|select(.path|startswith("test-data/spreadsheet/"))|select(.path|test("\\.(xlsx|xlsm)$"))|.path' > poi.txt
mkdir -p /tmp/poi-xlsx && while read -r p; do
  curl -sfL -o "/tmp/poi-xlsx/$(basename "$p")" "https://raw.githubusercontent.com/apache/poi/$SHA/$p"; done < poi.txt
python3 -I spikes/xlsx-phase5/objects_survey.py --summary /tmp/poi-xlsx reference/fixtures/xlsx/calamine | grep '^#'
```

Measured on 2026-11-16 (Python 3.15): 367 paths listed, 366 POI files fetched, plus the 2
calamine fixtures; 342 packages read (the rest are Strict, encrypted, fuzzing cases or not zip).
Runs in about 5 s.
