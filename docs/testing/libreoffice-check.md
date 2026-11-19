# LibreOffice verification of the manual-check bundle

The `LibreOffice manual-check` workflow runs on pull requests, pushes to
`main` and manual dispatch. It generates the bundle with
`cargo xtask manual-check <outdir>` and verifies its top-level XLSX, XLSM,
DOCX and PPTX cases. The XLSX example also bundles copies of source fixtures
under `xlsx/originals/` for manual comparison.

| Check | Evidence |
|---|---|
| (a) Opens | Headless LibreOffice reports a successful conversion and writes a nonempty PDF. Writer uses an explicit untagged export for DOCX cases 10 and 14a because the default export crashes on their shared SDK footnote/endnote fixture in the pinned release. Other cases use the default export. |
| (b) Content | Calc exports a UTF-8 CSV for each sheet, including hidden sheets. The check compares sheet names and order and looks up selected cells by address. Writer exports UTF-8 text, with PDF text used for selected headers, footers and notes. Impress exports PDF, read with `pdftotext`. Slide-count cases check the number of PDF pages. Clip-prone hyperlink text, speaker notes and comments are read from the resaved files. Selected structure and formatting edits are checked directly on the generated OOXML. |
| (c) Recalculation | Calc opens with `OOXMLRecalcMode=0` (always recalculate, as in O3). The checker compares each formula cell with its cached value, or with a manifest value for a cell expected to have no cache. An unexpected cache fails the check. A formula must still exist at that address after the save. Numeric comparisons use a relative tolerance of `1e-12`. |
| (d) Read-back | Calamine reads every worksheet of the resaved spreadsheets, python-docx reads body blocks of the resaved documents and python-pptx reads the slides and shapes of the resaved presentations. |

The job summary counts passing, failing and inapplicable checks. The failure
artifact holds each failing input and its diagnostics. Each input uses a fresh
headless profile with macro security set to Very High. The checker rejects
packages containing DTDs in OOXML XML parts before importing them.

Both LibreOffice workflows cache the release tarball in GitHub Actions under a
key containing the release version and its pinned SHA-256. A cache hit skips
the download. A miss uses curl's resume option and up to five retries on
transfer errors, with a 35-minute per-attempt limit and a 60-minute download
step limit. Both paths verify SHA-256 before extracting the tarball.

## Updating the expectations

`tools/lo-check/expected.json` is copied into the generated bundle. Add or
update entries when changing manual-check cases. Use the edit and the example's
checklist to choose at least one assertion that would fail if the edit were
omitted. For a structure or formatting change outside the exports, use an
`xml` assertion of the form `[part, XPath, expected match count]` on the input
to LibreOffice. `resaved_text`, `notes` (keyed by one-based slide number) and
`comments` assert text on LibreOffice's same-format output. JSON numbers represent
numeric cells. Strings can represent literal text or Calc’s CSV output for
formatted dates and percentages. Use `resaved_xml` to match XML on the resaved
package and `resaved_sha256` to check the bytes of named parts. Calc rewrites the
one-cell picture anchor as a two-cell anchor and rounds its geometry. The image
case checks the original size and end marker on the input, and the number of
pictures, start markers, alt text and original PNG bytes on the resave. Each
spreadsheet entry lists
all sheets in workbook order. For uncached formula cells, add an entry keyed
by `Sheet!A1` with the OOXML result type and value, for example `["n", "10"]`.
Some exploratory fixtures deliberately exercise application-dependent formula
behavior. For these, the manifest records the pinned Calc result, including
typed error results where appropriate.

To rerun the check locally with the workflow's LibreOffice version, put
`soffice` and `pdftotext` on PATH and use fresh paths for the output and
failures:

```sh
export LANG=C.UTF-8 LC_ALL=C.UTF-8 TZ=UTC
lo_check_tmp=$(mktemp -d)
python3 -m venv "$lo_check_tmp/venv"
"$lo_check_tmp/venv/bin/python" -I -m pip install --require-hashes -r tools/lo-check/requirements.txt
cargo build --locked --manifest-path tools/lo-check/Cargo.toml --target-dir "$lo_check_tmp/oracle"
cargo xtask manual-check "$lo_check_tmp/bundle"
"$lo_check_tmp/venv/bin/python" -I tools/lo-check/check.py \
  "$lo_check_tmp/bundle" "$lo_check_tmp/oracle/debug/lo-check-calamine" \
  "$lo_check_tmp/failures"
```

## XLSX table read-back

After generating the bundle, use openpyxl 3.1.5 to verify the table name,
column metadata, filter range and calculated-column formulas in the
original `39-p5b-table.xlsx`:

```sh
"$lo_check_tmp/venv/bin/python" -I -m pip install openpyxl==3.1.5
"$lo_check_tmp/venv/bin/python" -I crates/ooxml-xlsx/tests/oracle/tables.py \
  "$lo_check_tmp/bundle/xlsx"
```

## Coverage limits

The bundled manual checklists verify that Excel, Word and PowerPoint open
the files without a repair dialog. A successful LibreOffice conversion
only establishes that LibreOffice opened the file.

The following expectations record behavior of the pinned Calc release:

- `xlsx/05-pivot-and-table.xlsx` checks the pivot output Calc refreshes on
  open (Tabelle2 and Tabelle3). Excel may retain the cached layout until
  Refresh.
- `xlsx/25-p2-dates-1900.xlsx` checks the CSV display of serial 59 at B2.
  Calc exports `02/27/1900`. Excel displays February 28.
- The uncached formula results in `formulas` record Calc's recalculation
  for XLSX cases 12, 13, 14, 15, 17, 18, 19, 21, 23 (merge-clearing), 33
  (delete-ref-errors-3d) and 35 (add-hide-tab-colour). This includes the
  array errors in 13, the dynamic lookup in 14, and the broken references
  in 33. Excel recalculation is checked separately.

Text extraction collapses whitespace for DOCX and PPTX and can
split or clip text because of line wrapping and font substitutions. The
checks cover selected visible content and formula results. Layout, styling,
accessibility tags, animations, comment anchors, complete notes formatting, hyperlink targets,
macros and full lossless preservation require their own checks. Read-back
establishes that the independent readers can traverse the resaved files.
