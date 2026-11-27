# ooxml-rs XLSX browser adapters

The browser package exposes separate `@ooxml-rs/xlsx/exceljs` and
`@ooxml-rs/xlsx/sheetjs` ESM entry points. Each loads the shared Wasm asset
during module initialization. See ADR-0021 for the supported workflows and
the remaining compatibility checks.

Both entry points also support Node.js ESM imports. In Node.js, SheetJS
file APIs use the filesystem passed to `set_fs`, for example
`SheetJS.set_fs(fs)` after importing `fs` from `node:fs`. To verify both
entry points from a packed install, copy `test/node-install.mjs` into the
scratch project and run `node node-install.mjs`.
Both SheetJS entry points expose `CFB` for low-level Compound File, ZIP,
and MIME archive operations. The Node.js entry also supports filesystem
paths through the CFB file APIs.
SheetJS reads BIFF5 and BIFF8 workbooks through `read` or
`parse_xlscfb`. Use `bookType: 'biff5'`, `'biff8'`, or `'xls'` to write
legacy workbooks with cell values and number formats.

In Node.js, ExcelJS exposes `workbook.xlsx.readFile` and `writeFile`.
The `read` and `write` methods accept Node.js streams as well as Web
Streams. The Node.js `writeBuffer` returns a `Buffer`.
The Node.js entry also exposes `workbook.csv` for stream and file
reads, stream and file writes, and buffer output. In browsers,
`workbook.csv.read` accepts a Web ReadableStream of bytes,
`workbook.csv.writeBuffer` returns UTF-8 bytes, and `workbook.csv.write`
accepts a Web WritableStream.

ExcelJS XLSX output accepts `useStyles: false` to export neutral cell
formats while retaining dates, row heights, and column widths.

The SheetJS entry reads XLSX byte views, numeric byte arrays, Base64, and
binary strings. With `type: 'string'`, it reads delimited text with support
for quoted fields, `raw`, `dense`, `FS`, `cellText`, and `sheetRows`.
Scalar CSV data can be saved as XLSX. CSV-specific date and formula
inference is outside this input profile. XLSX `read` accepts `sheets` to
select worksheet projections by name or zero-based index,
`bookSheets: true` to return only names, and `bookProps: true` to read
standard and custom document properties without projecting cells. Use
both options to retrieve properties and sheet names. Ordinary XLSX reads
expose the same properties as read-only fields. Fresh XLSX drafts can set
standard document properties through `Props` and scalar custom properties
through `Custprops`. XLSX write options can
override supported core properties in fresh and loaded workbooks.
`bookVBA: true` exposes a copy of the raw VBA project in `vbaraw` when present.
`bookDeps: true` reads any recorded calculation chain into `Deps`.
`bookFiles: true` exposes ZIP entry names and copies of their decompressed
contents through `keys` and `files`. Use `sheetRows` to limit the
projected rows while recording a truncated extent in `!fullref`.
`dense: true` projects cells into the editable zero-based `!data` arrays.
`sheetStubs: true` exposes stored blank cells as `t: 'z'`, including
their number format when `cellNF` is also enabled. `cellStyles: true` enables
both options and exposes row and column layout plus read-only cell fills
in `s`. XLSX write methods accept `cellStyles` and `sheetStubs`.
Fresh workbooks use the default fill when a draft cell carries `s`,
matching the SheetJS CE XLSX writer. Explicit `compression: true` writes
DEFLATE entries, and `compression: false` stores them uncompressed.
Omitting the option keeps the default package-preservation behavior.
Fresh workbooks suppress the number-stored-as-text warning by default.
Set `ignoreEC: false` to enable it. For loaded XLSX workbooks the setting
changes only when `ignoreEC` is explicitly supplied.
New workbooks use inline strings by default. With `bookSST: true`,
they build a shared-string table when needed. Loaded workbooks retain
their original string storage by default. Set `bookSST: false` in
SheetJS or `useSharedStrings: false` in ExcelJS to export cell-local
strings. Set either option to `true` to put cell-local strings in a
workbook-wide shared string table. `themeXLSX` accepts replacement DrawingML
theme XML when saving fresh or loaded workbooks. With `cellDates: true`,
XLSX writes store supported Date cells as ISO timestamps. Read the same
cells as Date objects or spreadsheet serials using the read option.
Explicit `cellDates: false` converts ordinary ISO dates on loaded sheets
to serials in the workbook's date system. Replacing a supported
value cell with `{ t: 'z' }` clears its value and preserves its format.
SheetJS exposes the worksheet filter as `!autofilter.ref`. New and loaded
workbooks can save a filter range; the Rust layer guards active filters.
New workbooks can save explicit array formula ranges and retain
caller-supplied results for ordinary and array formulas. Loaded workbooks
can update or clear the cached result of an ordinary formula. Loaded workbooks can add array formulas within
the original worksheet range. XLSX reads strip `_xlfn.` from displayed formula
text unless `xlfn: true` is set. When reading with `cellText: true`,
`dateNF` overrides the display text for built-in date format 14. The
`cellNF` format code is preserved. Saving these XLSX workbooks
preserves unprojected sheets and returns the original bytes when unedited.
XML workbook reads expose `bookType: 'xlsx'`, including macro-enabled
workbooks. The generic write APIs accept `xlsx`, `xlsm`, and `xlam`
for new workbooks and preserve the declared format of loaded files.
`write` and `writeXLSX` support ArrayBuffer, browser
Uint8Array, Base64, and binary string output. In a browser document,
`writeFile`, `writeFileSync`, and `writeFileXLSX` start an XLSX download
using the supplied filename.

The SheetJS entry also exports A1 helpers and `utils.sheet_to_json` (including
the `sheet_to_row_object_array` alias). Data export accepts sparse and dense
worksheet objects, explicit ranges, array or object headers, and hidden-row
and hidden-column filters. Formatted export uses existing cell display text
or a basic scalar representation. Worksheets can also be exported as CSV,
tab-separated text, or lists of formula assignments. `format_cell` and the formatted exports support common fixed-decimal,
grouping, percentage and date codes. Other formats require additional support. Array and JSON input helpers can
populate a new workbook built with `book_new` and `book_append_sheet`.
For new XLSX output, `write` accepts sparse or dense sheets of unformatted
numbers, strings, booleans, blank stubs, Excel errors, dates and formulas without
cached results. Cell number format codes are written to XLSX. The XLSX extent is computed from written cells; explicit
blank margins and other cell styles are outside this creation profile.

ExcelJS `xlsx.load` accepts a Base64 string with `{ base64: true }`.
Optional `maxRows` and `maxCols` validate stored row counts per worksheet
and stored cell counts per row before the workbook is published. These
checks run after opening the package.

The ExcelJS entry exposes worksheet IDs, name and numeric ID lookup, and
`eachSheet`. Iteration follows document order. Omitting the lookup argument
selects the worksheet with the lowest ID. Row and cell lookups share handles
across A1 and numeric coordinates. Row and cell counts and iteration include
stored row metadata and support the `includeEmpty` option. The values
getters return sparse one-based arrays of supported cell values.
`addRow` and `addRows` accept scalar arrays or objects using column keys.
Column definitions support keys, headers, width, and hidden state. Width
and hidden state are read from the XLSX and persist on save. Column keys
and header declarations are JavaScript metadata; the header cells persist.
Row height, hidden state, and row and column outline levels are read from
and written to XLSX. ExcelJS `collapsed` follows the worksheet's
`properties.outlineLevelRow` and `properties.outlineLevelCol`.
Assigning `null` clears a supported ExcelJS cell value and preserves the
cell style. Clearing an already empty cell keeps the original bytes.
ExcelJS exposes date-formatted serials as JavaScript `Date` in both Excel
date systems. Date assignments preserve a date format already on the cell
or apply `mm-dd-yy`. Explicit ISO-date XML cells remain guarded.
`addWorksheet` adds a default or named worksheet to a new or loaded
workbook and accepts the initial worksheet `state`. The new sheet uses the first free ID
provided that the underlying workbook has no reference reserving it.
Assigning to `Worksheet.name` renames the sheet using Rust's reference-aware
rename operation and preserves existing row and cell handles.
`Worksheet.state` reads or writes `visible`, `hidden`, and `veryHidden`.
Hiding the last visible worksheet is refused. `Worksheet.autoFilter`
accepts an A1 range or two endpoints, and returns the serialized A1 range.
The filter can also be supplied to `addWorksheet`. ExcelJS `mergeCells`
clears covered values before merging. Covered cell handles read and write
through the master value, while existing cell styles are preserved.
`removeWorksheet` accepts a name or ID and invalidates the removed sheet's
handles. The last visible worksheet and deletions rejected by the Rust
reference and relationship checks are refused before publication.

## Declare JS input options

Add API input fields to `exceljs-option-contracts.mjs` or
`sheetjs-option-contracts.mjs`. For example, the ExcelJS streaming reader's
`worksheets` option uses `type: 'enum'`, `presence: 'optional'`, a list of
`choices` and `default: 'emit'`. For strings, use `unicodeString` when an
empty string is valid and `nonemptyUnicodeString` when it is invalid.
These types reject unpaired surrogates. Optional fields allow
`undefined`. The declared string and enum types reject `null` unless it
appears in an enum's choices. For ZIP options, `store` uses
`type: 'boolean'` and `level` uses `type: 'integer'` with an inclusive
`range: [-1, 9]`.

Field declarations validate values after the contract's input-reading
rules and defaults run. For example, the streaming reader uses
`ownDataProperties` to read only own data properties. Keep validation
that needs workbook state, such as resolving worksheet references, in Rust.

A field's `when` can depend on earlier own data fields. For example, the
ExcelJS color contract checks `tint` only when `argb` is absent and
`theme` is defined. An array of conditions requires all conditions to match.

For SheetJS XLSX reads, add options to `xlsxReadOptions`. Each entry
holds its validation rule and default. Use `projection: {}` to pass a
normalized option through to worksheet settings under the same name.
Boolean projections can specify `trueWhen` and `alsoEnabledBy`. XLSB
projections can specify `rustField` to name a field expected by Rust.
List each projected option in the corresponding projection order to
preserve the order of option reads. For workbook resources, declare
`workbookResource.output` and `workbookResource.disabledBy`. For
example, `bookVBA` requests `vba` unless `bookSheets` is enabled. Add
validated XLSX fields to `xlsxReadValidationOrder` to give them an
explicit position in error precedence.

Regenerate both adapters with
`node packages/xlsx/generate-sheetjs-options.mjs`. Append `--check` to
verify that the committed generated files match the declarations.

Generate a Markdown reference for the declared contracts with
`node packages/xlsx/generate-sheetjs-options.mjs --reference`. Search
the output by format under `SheetJS read(data, options)` to find the
option tables. The tables describe rules declared in the DSL, including
format-specific checks. Search by generated helper name for expanded option
lists, validation and fallback rules, and Rust projection inputs. Use
the public API definitions for function signatures and overloads.

## Build and verify

Install the Rust `wasm32-unknown-unknown` target and wasm-bindgen CLI
0.2.130, then run `bash packages/xlsx/build.sh` from the repository root.
Run `node --test packages/xlsx/test/contract.test.js` for the JS input
contract. Serve the repository root over HTTP and open
`/packages/xlsx/test/browser.html` for the native ESM smoke test.

For distribution verification, pack `packages/xlsx` with npm, install the
tarball in a scratch project together with webpack 5.113.0, TypeScript
5.9.3, Playwright 1.58.2, ExcelJS 4.4.0, and SheetJS CE 0.20.3 from the
SheetJS CDN. Generate fixtures under the scratch project's `fixtures` folder
using the script below, then run `node /path/to/packages/xlsx/test/bundle.mjs` from that
project. Serve the scratch project over HTTP and open `/bundles.html?exceljs`,
`/bundles.html?sheetjs`, and `/bundles.html?both`. Copy `test/imports.mts`
into the scratch project and run
`npx tsc --noEmit --strict --target es2022 --module nodenext imports.mts`. To run all distribution probes, use
`node /path/to/test/run-distribution.mjs chromium http://127.0.0.1:8124`
and repeat with `firefox`. Use `OOXML_BROWSER_BINARY` if Playwright needs a
specific executable.

## Characteristic fixtures

Generate fixtures with `python3 -I packages/xlsx/test/make-fixtures.py
<scratch>/fixtures`. Build the Wasm crate with `--all-features`
and generate wasm-bindgen output under `<scratch>/wasm`. Copy `src` and
`test` to the same scratch directory and open `/test/properties.html`
through an HTTP server. The fault-injection artifact is separate from the
normal package build. Use `run-properties.mjs` from the scratch Playwright
project to save outputs for `check-parts.py`. Run it with a final `mutation`
argument to verify the failure test catches premature publication.
Use `run-staging.mjs` against that same scratch server to measure the
byte-snapshot and Rust-held staging paths in fresh browser documents.
Pass a sample count after the server URL to alternate the order of the
two paths across repeated measurements.

## Reference differences verified so far

SheetJS CE 0.20.3 infers a range when dimension is absent and exposes stored
cells outside an undersized dimension. The initial adapter uses the valid
declared range and omits cells outside it from the JS projection, preserving
them in the package. ExcelJS 4.4.0 keeps old cell objects readable after a
successful reload. The adapter reports a stale handle to avoid applying
edits to an obsolete workbook.

## Baseline measurements

One local run with webpack 5.113.0 produced the following raw and gzip
sizes. Both adapters shared one 8,510,679-byte Wasm asset (2,380,349 bytes
gzip).

| JS bundle | Raw bytes | Gzip bytes |
| --- | ---: | ---: |
| ExcelJS only | 333,878 | 74,702 |
| SheetJS only | 380,951 | 106,592 |
| Both | 614,128 | 166,937 |

| Browser | Fixture | Initialization ms | ExcelJS load / 10 setters / save ms | SheetJS load and projection / save ms | Wasm linear-memory high water |
| --- | --- | ---: | ---: | ---: | ---: |
| Chromium 158.0.8127.18 | 4,580 bytes | 13.9 | 1.2 / 6.6 / <0.1 | 1.1 / 1.0 | 1,769,472 bytes |
| Chromium 158.0.8127.18 | 26,165 bytes | 11.5 | 8.6 / 94.2 / <0.1 | 21.2 / 11.8 | 3,014,656 bytes |
| Firefox 146.0.1 | 4,580 bytes | 47 | 3 / 35 / <1 | 4 / 4 | 1,769,472 bytes |
| Firefox 146.0.1 | 26,165 bytes | 50 | 86 / 943 / <1 | 183 / 98 | 3,014,656 bytes |

The sparse fixture has two worksheets and 4,004 stored numeric cells. The
ExcelJS setter figures include reloading and serializing each candidate.
Memory is Wasm linear memory measured at the end of the run. JS heap and
browser-process overhead are outside this measurement.

Same-origin loading passed with `script-src 'self' 'wasm-unsafe-eval'` and
`connect-src 'self'`, plus a nonce for the probe page's inline loader.

### Rust staging comparison

The comparison candidate keeps the committed bytes in Rust. Both paths still
serialize to validate a setter before publishing its result. In one isolated
run against the 26,165-byte sparse fixture, ten setters took 95.3 ms with the
baseline and 91.4 ms with the candidate in Chromium. In Firefox the times
were 936 ms and 994 ms respectively. The candidate's Wasm memory high water
was 2,949,120 bytes versus 3,014,656 bytes for the baseline. Successful edits
produced identical committed bytes in the fixture test. The candidate also
retained the previous read view and committed bytes across fault injection.
The distributed adapters use the baseline due to the mixed timing results.


Delimited and HTML text reads can infer common numeric date and time patterns.
`cellDates` selects Date objects, `cellNF` exposes the date number format,
and `UTC: false` uses the local timezone.


Use `PRN: true` to read fixed-width text when a delimiter is absent.
Use `bookType: 'prn'` to export a worksheet as fixed-width text.


For byte-based text imports, `codepage` selects a supported browser
encoding, such as CP932 or Windows-1252. Unicode byte-order marks take
precedence. Use `set_cptable` to supply a legacy codepage decoder, such as
one that supports CP437. XLSX detection is independent of the text codepage.


Browser saves also accept `write(wb, { type: 'file', file: 'result.xlsx' })`.
The XLSX-specific write methods always select XLSX. The `codepage` write
option is accepted without changing the standard encodings for CSV,
TXT, HTML and PRN.
