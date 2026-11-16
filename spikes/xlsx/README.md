# spikes/xlsx

Inspection scripts behind `docs/roadmap/xlsx.md`, `docs/adr/0003-*.md` and
`reference/fixtures/xlsx/`. They only *read* untrusted files, using `zipfile` and `xml.etree`;
nothing is extracted or executed. Always run them with `python3 -I` (isolated mode), from the
repository root, passing the data directories as arguments.

| Script | Purpose |
|---|---|
| `survey.py DIR…` | One JSON line per package: producer, zip data descriptors, formula kinds, cells/rows without `r`, inline/rich strings, `spans`, empty `sheetData`, merges, `mc:Ignorable`, `mc:AlternateContent`, `xml:space`, parts present (tables, pivots, charts, comments, VBA, rich data, …). Roadmap §5. |
| `formula_stats.py DIR…` | Formula census by QName per package, `calcChain`/`calcPr`, shared/array, and the features that defeat syntactic dependency analysis (names, structured refs, `INDIRECT`/`OFFSET`, external, cross-sheet). ADR-0003 Context. |
| `o3_recalc.py DIR OUT.tsv` | ADR-0003 O3 data: recalculates every `*.xlsx` in DIR with LibreOffice headless (hard recalc in a throw-away profile) and writes every cell value of the re-saved files. Owner/CI only (needs LibreOffice); inputs come from `cargo run -p ooxml-xlsx --example o3_inputs -- DIR`. |
| `phase0_facts.py DIR…` | Facts the Phase0 acceptance tests rely on (first sheet in workbook order, cell types, self-closing shapes, merges, `dimension`, QName vs byte-prefix formula counts) and the verdict ADR-0001/0002 predict. Roadmap §7. |

Reproducing the corpus numbers (files are not committed; Apache-2.0 / MIT):

```sh
# list and fetch POI test-data/spreadsheet at the pinned commit
SHA=47dcff769bb395034dddfd59c4609659b6dbc6a4
gh api "repos/apache/poi/git/trees/$SHA?recursive=1" \
  --jq '.tree[]|select(.path|startswith("test-data/spreadsheet/"))|select(.path|test("\\.(xlsx|xlsm)$"))|.path' > poi.txt
mkdir -p /tmp/poi-xlsx && while read -r p; do
  curl -sfL -o "/tmp/poi-xlsx/$(basename "$p")" "https://raw.githubusercontent.com/apache/poi/$SHA/$p"; done < poi.txt
python3 -I spikes/xlsx/survey.py /tmp/poi-xlsx > survey.jsonl
python3 -I spikes/xlsx/formula_stats.py /tmp/poi-xlsx | tail -20
python3 -I spikes/xlsx/phase0_facts.py reference/fixtures/xlsx/poi reference/fixtures/xlsx/calamine
```

Measured on 2026-11-16 (Python 3.15): 367 POI files, 346 open as zip packages, 341 Transitional
workbooks readable by `formula_stats.py`, 126 of them with formulas.
