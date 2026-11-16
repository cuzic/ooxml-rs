# spikes/xlsx-phase3

Measurements behind [ADR-0009](../../docs/adr/0009-xlsx-phase3-sheet-operations.md) (xlsx
Phase3: sheet operations). Read-only: `zipfile` + `xml.etree`, parts with a DTD are skipped,
nothing is extracted or executed. Run with `python3 -I` from the repository root and pass the
data directories as arguments.

| Script | Purpose |
|---|---|
| `sheet_ops_survey.py DIR…` | One JSON line per package: sheets (kinds, states), `sheetId` gaps and order, part number vs `sheetId`, relationship-id patterns, content-type Overrides, `app.xml` shape (and whether it agrees with `workbook.xml`), defined names (local, hidden, `_xlnm`, `#REF!`), `activeTab`/`firstSheet`, VBA and `codeName`s, the locations that refer to each sheet **by name** (rename impact), and the relationship types in each sheet's own `.rels` (delete impact). |
| `sheet_ops_survey.py --summary DIR…` | The table below. |

References are found with a regex over the XML text: sheet-name prefixes `'…'!`, `Name!` and
`A:B!`, with string literals masked first and counted separately. It is a survey, not a
tokenizer.

**Limits** (ADR-0009 review m12): the survey does not scan chartEx, `c15:` chart extensions,
ctrlProps, VML form controls, table parts (`calculatedColumnFormula`) or `connections.xml`.
The review's probe found real references in the last three, so the "referenced by name" counts
below are a **lower bound**. ADR-0009 revision 2's closed-world scan and residual-mention
oracle cover every XML part regardless.

Reproducing: fetch POI `test-data/spreadsheet` @ `47dcff769bb395034dddfd59c4609659b6dbc6a4`
as described in [`../xlsx/README.md`](../xlsx/README.md), then:

```sh
python3 -I spikes/xlsx-phase3/sheet_ops_survey.py --summary "$SCRATCH/poi"
python3 -I spikes/xlsx-phase3/sheet_ops_survey.py --summary reference/fixtures/xlsx reference/round_trip_corpus
```

## Results (2026-11-16, Python 3.15)

| Fact | POI (366 files) | Fixtures |
|---|---|---|
| packages (Transitional, readable) | 342 | 24 |
| sheets per workbook: median / p90 / max | 2 / 3 / 30 | 1 / 3 / 5 |
| packages having a sheet of kind | {'worksheet': 341, 'chartsheet': 7, '?': 1, 'macrosheet': 2} | {'worksheet': 24, 'macrosheet': 1} |
| packages with hidden / veryHidden sheets | 7 / 1 | 1 / 0 |
| sheetId gaps (max != count) | 56/342 (16%) | 7/24 (29%) |
| sheetId not ascending in workbook order | 39 | 4 |
| sheetId duplicates | 0 | 0 |
| every sheet part number == its sheetId | 264/342 (77%) | 14/24 (58%) |
| workbook rel ids all rIdN | 338/342 (99%) | 22/24 (92%) |
| workbook rel ids rId1..rIdN contiguous | 337/342 (99%) | 22/24 (92%) |
| every sheet part has a CT Override | 340/342 (99%) | 24/24 (100%) |
| app.xml present | 331/342 (97%) | 21/24 (88%) |
| app.xml with HeadingPairs + TitlesOfParts | 294/342 (86%) | 16/24 (67%) |
| ... heading counts sum to TitlesOfParts | 293/294 (100%) | 16/16 (100%) |
| ... first block = visible+hidden worksheet names in workbook order | 290/294 (99%) | 16/16 (100%) |
| ... TitlesOfParts mentions every non-veryHidden sheet | 290/294 (99%) | 16/16 (100%) |
| ... heading labels | {'Worksheets': 238, 'Named Ranges': 32, 'Arbeitsblätter': 20, '工作表': 8, 'Fogli di lavoro': 7, 'ワークシート': 6, 'Charts': 6, 'Листы': 5, 'Feuilles de calcul': 5, 'Benannte Bereiche': 4} | {'Worksheets': 14, 'Листы': 1, 'Named Ranges': 1, 'Arbeitsblätter': 1, 'Excel 4.0 Macros': 1} |
| ... producers where first block != sheets | {'Microsoft': 4} | {} |
| app.xml without HeadingPairs/TitlesOfParts: producers | {'Microsoft': 9, 'Apache': 6, '?': 4, 'LibreOffice/25.2.7.2$Linux_X86_64': 3, 'LibreOffice/4.3.3.2$Linux_X86_64': 3, 'LibreOffice/7.3.7.2$Linux_X86_64': 2} | {'LibreOffice/7.3.7.2$Linux_X86_64': 1, 'LibreOffice/25.2.7.2$Linux_X86_64': 1, 'Hancom': 1, 'Microsoft': 1, 'ooxml-rs': 1} |
| defined names (packages with any / with local / with _xlnm) | 63 / 25 / 22 | 3 / 1 / 1 |
| localSheetId out of range | 0 | 0 |
| names containing #REF! | 12 | 1 |
| activeTab > 0 / firstSheet > 0 | 38 / 10 | 3 / 2 |
| VBA project / signed / workbook codeName / sheets with codeName (packages) | 9 / 0 / 35 / 32 | 1 / 0 / 2 / 3 |
| calcChain / externalLinks (packages) | 117 / 10 | 4 / 1 |
| multi-sheet packages | 196 | 8 |
| ... where some sheet is referenced by name from anywhere | 62/196 (32%) | 2/8 (25%) |
| packages with a by-name sheet reference in: definedName | 46 | 1 |
| packages with a by-name sheet reference in: chart c:f | 25 | 1 |
| packages with a by-name sheet reference in: cell formula | 17 | — |
| packages with a by-name sheet reference in: pivot cache worksheetSource | 5 | 1 |
| packages with a by-name sheet reference in: x14 ext (xm:f: DV/CF/sparklines) | 3 | — |
| packages with a by-name sheet reference in: hyperlink @location | 3 | — |
| packages with 3-D references (Sheet1:Sheet3!) | 3 | 0 |
| packages with sheet names inside string literals (INDIRECT etc.; not rewritable) | 0 | 0 |
| worksheet own-.rels types (packages) | {'printerSettings': 110, 'drawing': 61, 'vmlDrawing': 37, 'table': 29, 'comments': 27, 'hyperlink': 14, 'tableSingleCells': 14, 'pivotTable': 7, 'oleObject': 5, 'customProperty': 5, 'image': 4, 'threadedComment': 3, 'control': 2, 'package': 2} | {'drawing': 5, 'printerSettings': 5, 'vmlDrawing': 3, 'comments': 2, 'hyperlink': 1, 'ctrlProp': 1, 'threadedComment': 1, 'table': 1, 'pivotTable': 1} |

Notes:

- "Referenced by name" counts every location that names a sheet, including a sheet's own
  `_xlnm.Print_Area`. A rename has to rewrite all of them.
- The `app.xml` check groups titles by sheet kind (worksheets first) and leaves out
  `veryHidden` sheets, as Excel does. The 4 POI mismatches are:
  - two stale or empty `app.xml`s (`60289.xlsx`, `bug66675.xlsx`);
  - the Excel 2007 beta file (`sample-beta.xlsx`);
  - a sheet name with an `_x000a_` escape (`xssf-enum.xltx.xlsx`).
- `[Content_Types].xml` `Default Extension="bin"` (separate count, not in the table): it is the
  printer-settings type in 110 POI packages, `vbaProject` in 4, `oleObject` in 3 and
  `customProperty` in 2. A VBA project added to an existing package needs an `Override`.
