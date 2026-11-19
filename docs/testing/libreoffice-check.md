# LibreOffice verification of the manual-check bundle

The `LibreOffice manual-check` workflow runs on pull requests, pushes to
`main` and manual dispatch. It generates the bundle with
`cargo xtask manual-check <outdir>` and verifies its top-level XLSX, XLSM,
DOCX and PPTX cases. The XLSX example also bundles copies of source fixtures
under `xlsx/originals/` for manual comparison.

| Check | Evidence |
|---|---|
| (a) Opens | Headless LibreOffice reports a successful conversion and writes a nonempty PDF. Writer uses an explicit untagged export because the default export crashes on the SDK footnote/endnote source fixture in the pinned release. |
| (b) Content | Calc exports a UTF-8 CSV for each sheet, including hidden sheets. The check compares sheet names and order and looks up selected cells by address. Writer exports UTF-8 text, with PDF text used for selected headers, footers and notes. Impress exports PDF, read with `pdftotext`. The three empty decks are checked for page count and absence of extracted text. |
| (c) Recalculation | Calc opens with `OOXMLRecalcMode=0` (always recalculate, as in O3). The checker compares each formula cell with its cached value, or with a manifest value if the cache is absent. A formula must still exist at that address after the save. Numeric comparisons use a relative tolerance of `1e-12`. |
| (d) Read-back | Calamine reads every worksheet of the resaved spreadsheets, python-docx reads body blocks of the resaved documents and python-pptx reads the slides and shapes of the resaved presentations. |

The job summary counts passing, failing and inapplicable checks. The failure
artifact holds each failing input and its diagnostics. Each input uses a fresh
headless profile with macro security set to Very High. The checker rejects
packages containing DTDs in OOXML XML parts before importing them.

## Updating the expectations

`tools/lo-check/expected.json` is copied into the generated bundle. Add or
update entries when changing manual-check cases. Use the edit and the example's
checklist to choose representative cells and text. JSON numbers represent
numeric cells. Strings can represent literal text or Calc’s CSV output for
formatted dates and percentages. Each spreadsheet entry lists
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
"$lo_check_tmp/venv/bin/python" -I -m pip install -r tools/lo-check/requirements.txt
cargo build --manifest-path tools/lo-check/Cargo.toml --target-dir "$lo_check_tmp/oracle"
cargo xtask manual-check "$lo_check_tmp/bundle"
"$lo_check_tmp/venv/bin/python" -I tools/lo-check/check.py \
  "$lo_check_tmp/bundle" "$lo_check_tmp/oracle/debug/lo-check-calamine" \
  "$lo_check_tmp/failures"
```

## Coverage limits

Excel, Word and PowerPoint repair dialogs still require the bundled manual
checklists. Text extraction collapses whitespace for DOCX and PPTX and can
split or clip text because of line wrapping and font substitutions. The
checks cover selected visible content and formula results. Layout, styling,
accessibility tags, animations, comment anchors, notes, hyperlink targets,
macros and full lossless preservation require their own checks. Read-back
establishes that the independent readers can traverse the resaved files.
