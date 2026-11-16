# spikes/xlsx-phase4

Corpus evidence for [ADR-0014](../../docs/adr/0014-xlsx-phase4-range-features.md) (xlsx Phase4,
range features). The script only *reads* untrusted files with `zipfile` and
`xml.etree.ElementTree`; nothing is extracted or executed. Run it with `python3 -I` from the
repository root, passing data directories as arguments.

| Script | Purpose |
|---|---|
| `range_survey.py [--summary] DIR…` | One JSON line per package: merged cells (count, overlaps, values in non-anchor cells), hyperlinks (external/internal), data validation (2007 types, cross-sheet lists, inline lists over 255 characters, x14 forms), conditional formatting (rule types, `dxfId`, duplicate priorities, x14 links and dangling ids), autofilter and sort state, panes and sheet views, `cols`/row attributes, page setup, print names, sheet and workbook protection (flags, legacy hash, SHA form), `mc:AlternateContent` children of `worksheet`. `--summary` adds a per-feature table (files, occurrences). |

Reproducing the ADR's numbers (files are not committed; Apache-2.0):

```sh
# fetch POI test-data/spreadsheet at the pinned commit (see spikes/xlsx/README.md)
python3 -I spikes/xlsx-phase4/range_survey.py --summary /tmp/poi-xlsx | grep '^#'
```

Measured on 2026-11-16 (Python 3.15): 350 POI files, 327 readable Transitional packages (the
rest are Strict, encrypted, fuzzing cases or not zip).
