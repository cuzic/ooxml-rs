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
# fetch the full POI test-data/spreadsheet directory at 47dcff769bb395034dddfd59c4609659b6dbc6a4
python3 -I spikes/xlsx-phase4/range_survey.py --summary /tmp/poi-xlsx | grep '^#'
```

**Corpus (revision 2, review m11).** The numbers in ADR-0014 revision 2 come from the **full**
`test-data/spreadsheet` directory of Apache POI at `47dcff769bb395034dddfd59c4609659b6dbc6a4`
(367 files, fetched 2026-11-16). 342 of them are readable Transitional packages: the rest are
Strict, encrypted, fuzzing cases or not zip, and 4 packages with a DTD in some part are skipped
(`r2:dtd_part`).

Revision 1 quoted "350 files, 327 readable", measured on a partial local copy. That table did
not reproduce at the pinned commit and has been replaced.

**Revision 2 counters** (prefix `r2:`), added for the review findings:

- `_FilterDatabase` scope and duplicates, and whether the sheet still has an `autoFilter`;
- sheet-level `sortState` and the one inside `autoFilter`, `sheetPr/@filterMode`, and hidden
  rows under an active filter;
- legacy and SHA protection on one element;
- `cfvo` formulas; 2007 CF formulas that refer to another sheet (`cf_formula_other_sheet`)
  vs. dead `#REF!` (`cf_formula_ref_error`); `stopIfTrue`; pivot CF blocks;
- x14 DV sources by kind; hyperlink targets by kind (scheme, `mailto:`, relative, drive, UNC);
- `cols` anomalies; frozen panes without `selection`; page-layout views; `dxfs` with
  `mc:AlternateContent`.

Results (files / occurrences):

| Counter | Result |
|---|---|
| `_FilterDatabase` names that are sheet-local on a sheet without `autoFilter` | 3 / 4 |
| `_FilterDatabase` names with `#REF!` text | 1 / 2 |
| `_FilterDatabase` names with workbook scope | 1 / 1 |
| sheet-level `sortState` on a sheet without `autoFilter` | 5 / 7 |
| `sortState` inside `autoFilter` | 1 / 1 |
| `cf_formula_ref_error` | 3 / 5 |
| `cf_formula_other_sheet` | 0 |
| `cf_stopIfTrue` | 4 / 16 |
| `cfvo_formula` | 1 / 1 |
| x14 DV sources: other sheet | 1 / 5 |
| x14 DV sources: `#REF!` | 2 / 2 |
| hyperlink targets: with a scheme / `mailto:` / relative | 9 / 55, 5 / 11, 2 / 4 |
| legacy sheet `password` | 2 / 5 |
| frozen panes without `selection` | 2 / 3 |
| `dxfs` with `mc:AlternateContent` | 1 / 1 |
| `filterMode`, hidden rows under an active filter, `cols` anomalies, `col` without `width` | 0 |
