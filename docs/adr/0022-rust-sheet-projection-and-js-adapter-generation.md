# ADR-0022. Rust worksheet projection and generated JavaScript adapters

- Status: **Proposed** (revision 3)
- Date: 2026-11-23
- Related: [ADR-0001](0001-lossless-round-trip-architecture.md), [ADR-0021](0021-xlsx-browser-js-compatibility.md)

## Context

The SheetJS adapters have to repeat option validation, projection selection, and conversion to public result objects. XLSB adds a second source of duplication because its JavaScript reader and writer still encode workbook and cell-table records directly. Rust already owns OOXML package I/O and the XLSB worksheet tail codec. The API inventory gives us a list of public behaviors and reference versions against which to test each move.

## Decision

Keep package handling, format decoding, coordinate validation, selection and bounded allocation in Rust. Use a typed XLSB projection at the adapter boundary. A shared XLSX projection remains a hypothesis. XLSB and XLSX retain separate codecs and preservation contracts. Existing XLSX mutations continue through the lossless workbook and edit APIs. A future edit batch must validate before committing and preserve existing edit-refusal behavior.

Project the layout from within the archive call that decompresses each sheet. Run projection only inside the existing selected-sheet branch, after the bookSheets early return. For the current XLSB contract, Rust computes the first excluded row and returns a prefix ending before that record. It returns row and column metadata, merges, filter and margins separately. JavaScript decodes cells from the prefix without enforcing a second cutoff. The expected boundary traffic is one outgoing byte copy for the prefix plus each requested metadata vector. The released Rust projection owns its memory until the adapter frees it in a finally block. Wasm memory capacity may remain at its high-water mark.

Use an internal wire layout bound to the package build. Rows are Uint32Array tuples of index, height in twentieths of a point and flags. Column runs are Uint32Array tuples of inclusive first and last indices, width in 1/256 character units and flags. Ranges are four unsigned coordinates and margins are six floating-point values. Enforce Excel coordinate bounds before allocating adapter arrays. Map decode errors to INVALID_INPUT and preserve the package load error path for archive failures. The spike covers recoverable decode errors. Wasm trap recovery and instance lifecycle are open production questions.

Use a build-time JavaScript declaration table for all currently supported XLSB read options and fixture identifiers. At the original validation point after archive detection, generate the complete allowlist: type, sheets, bookSheets, dense, sheetRows, sheetStubs, cellDates, cellNF, cellText, cellFormula, cellStyles and codepage. Generate an inspectable ESM mapper with an unchanged optionsMatch descriptor policy. Reject unknown own keys, symbols and accessors without calling getters. Ignore inherited keys and preserve explicit undefined data properties. Leave current input decoding, format detection and error precedence outside the generated mapper. Preserve XLSB's current values and coercions for fields outside the new typed boundary. Compare hostile descriptors against the previous adapter contract and ordinary valid options against SheetJS CE.

## Spike

After two rounds of Opus review and revision, implement the public XLSB read slice. Add the typed Rust layout projection to the existing archive binding. The cell codec and writer stay in place during this measurement. Start by generating the mapping for sheetRows, cellStyles and dense, then add cellFormula with omission, explicit undefined and false fixtures. Require cellFormula to change only the declaration and fixture with zero adapter control-flow edits. Count authored adapter and generator lines separately. Check same-build deterministic output. An XLSB-only result cannot establish that XLSX shares the same wire format.

Test exact row cutoff, a merge spanning the cutoff, post-table filter and margins, dense and sparse output, column widths, an unknown option, inherited options and an accessor. The baseline has no XLSB protection or hyperlink projection. Probe the pinned reference on trailing hyperlink and protection records before adopting the prefix contract for those features. Include selected-sheet and bookSheets cases. Run native tests, Node package tests and Chromium and Firefox reference comparisons. Benchmark single-sheet reads using a small fixture, a sparse fixture and a dense fixture with at least 100,000 cells. Use warm fixed-size batches and interleave the baseline and spike in the same process. Report 30 or more samples per workload, median and p95, Node version, workload dimensions and pre/post Wasm artifact size. Measure initialization separately and record Wasm memory capacity before and after. The spike fails the hot-path gate if median latency increases by more than 15%, p95 by more than 25%, peak Wasm capacity by more than 25%, or the full uncompressed stream crosses back into Wasm. A performance failure leaves the ADR Proposed and warrants a single-pass Rust cell projection before adoption.

## Alternatives

Keeping the codec in JavaScript avoids additional boundary traffic but keeps validation and format knowledge spread across the adapter. Moving the entire JavaScript object model into Rust introduces many crossings or changes object identity and lifecycle semantics. A runtime DSL interpreter would add a second execution model to debug. A small deterministic generator can prove the reduction in hand-written adapter code before the design expands.

## Review history

- [Round 1](0022-opus-review-round1.md) led to revision 2.
- [Round 2](0022-opus-review-round2.md) led to revision 3. The prototype is evidence for the current XLSB surface. Trailing features and trap recovery remain adoption gates.

## Spike outcome

The reference comparisons passed for the exercised surface. The small-sheet latency gate failed in two runs, and dense p95 failed in one run. Keep this ADR Proposed. The candidate for further evaluation is one Rust pass that decodes cells and layout with batched output. See the [spike results and reproduction commands](../../spikes/adr-0022/README.md).

The [single-pass follow-up](../../spikes/adr-0022/README.md#follow-up-single-pass-rust-cell-and-layout-projection) passed the dense and small workload gates in both runs. Sparse and selected-sheet p95 still missed the gate. The decision remains Proposed pending repeatable tail-latency results.

After the `sheetRows` corrections, an isolated Node benchmark used fresh processes and 200 timed reads per workload and adapter. All seven workloads met the original numerical gates in two rounds. The earlier p95 regressions did not recur under this protocol. See the [isolated results](../../spikes/adr-0022/README.md#isolated-benchmark-after-the-sheetrows-fixes). The ADR remains Proposed.

### XLSB defined-name read projection

Rust reads workbook defined-name records and sends the ordered scope,
flags, UTF-16 text and formula tokens to the adapter. The XLSB reader
projects `Workbook.Names` and uses the same ordered data to resolve
name references in cell formulas. The SheetJS option declaration
accepts `xlfn` to control the prefix on formula references. Formulas
outside the token renderer's coverage use `#REF!` in name definitions.

The XLSB read contract also declares the four values sent to the Rust
sheet projection. The generator checks that every field references an
accepted option. The adapter applies the generated mapping inside the
selected-sheet branch and shares the normalized style and formula flags
between Rust projection and JavaScript rendering.

XLSB writes workbook and sheet-scoped defined names with the Rust
workbook catalogue writer. XLSB and BIFF8 share the name-formula
compilation and index assignment pass. Unsupported definitions and
their dependents are omitted before assigning token indices. XLSB macro
names are skipped.

### XLSB hyperlink read projection

Rust decodes the trailing hyperlink records and resolves sheet
relationships only when a link references one. The adapter attaches the
result to the dense or sparse cell model. A row cutoff reached before
the tail suppresses hyperlinks, matching SheetJS 0.20.3. Link ranges
are checked against worksheet bounds and limited to 1,048,576 total
cell assignments per sheet.

The XLSB writer uses the same Rust codec. It retains a rectangular
hyperlink record when every cell in the rectangle still shares the
link. An incomplete rectangle is written as individual cell links.
External targets are escaped in the sheet relationship part. Document
locations use an empty relationship identifier.

### XLSB sheet protection write

The XLSB writer maps the SheetJS `!protect` permission fields and
emits the legacy protection record in Rust. Password verification uses
the same default byte conversion as the pinned reference. That reference
skips protection records on read.

A [remeasurement after the metadata expansion](../../spikes/adr-0022/README.md#benchmark-after-xlsb-metadata-expansion)
passed 13 of 14 workload comparisons. In the second round the
`bookSheets` median was 16.9% slower, above the 15% threshold.

For metadata-only reads, Rust skips decoding workbook name and external
reference records. Its catalogue reader still checks record framing and
decodes sheet visibility, names and the date system. The
[catalogue-only benchmark](../../spikes/adr-0022/README.md#catalogue-only-read-follow-up)
isolates this change.

The [seven-workload rerun](../../spikes/adr-0022/README.md#seven-workload-gate-rerun)
passed the median, p95 and Wasm capacity gates in both isolated Node
rounds. The ADR remains Proposed pending review of the accumulated
scope and evidence.

The option declaration generates a workbook-level selector separately
from the per-sheet Rust-call settings. A full read normalizes selected
names once and preserves workbook tab order. Metadata-only reads
return before accessing any selector array entries.

The workbook selector declaration is shared by XLSX, XLSB and compound
BIFF. XLSB detection verifies the content type of the actual workbook
part. A default mapping for an unused binary extension does not
identify an XML workbook as XLSB.
Compound BIFF continues to honor the adapter's existing selection
contract. The pinned reference returns every compound BIFF sheet for
the same option.

The XLSX declaration also generates worksheet settings. The generated
mapping enables number formats and stub cells when styles are requested,
even when the two individual options are false.

The XLSX workbook resource settings declare when VBA, dependencies
and ZIP entries are requested. Catalogue reads validate selectors and
skip building the selector predicate. They still project defined names
for the existing XLSX metadata contract.

ODS and FODS reuse the generated workbook selector. OpenDocument
keeps its selector validation after XML parsing, and a catalogue-only
read skips selector normalization after validation.
Named validation steps in the declaration keep the row-limit check
before XML parsing and the selector check after spreadsheet discovery.

The CSV row iterator uses generated separator defaults. The public
`sheet_to_txt` utility uses the configured encoder for UTF-16LE binary
strings, and workbook TXT output encodes its tab-separated text once.

The shared JSON row iterator uses generated settings. Its `raw`
default applies when there is no own property, and `rawNumbers` can
override it for numeric cells. The array and Node stream APIs use the
same iterator.

`sheet_to_formulae` uses a generated `values` setting and reuses
encoded row and column labels during traversal. A Node v25.9.0
microbenchmark with 100,000 numeric cells and 36 alternating runs
measured medians of 18.63 → 16.01 ms for sparse and 6.19 → 3.88 ms
for dense output with values enabled. Dense p95 was 13.12 → 14.84 ms.
With values disabled, the numeric fixture emits no entries.

HTML string and stream output use the same generated wrapper settings.
Null headers and footers use the reference defaults. Empty strings
suppress the corresponding wrapper.

HTML table import uses generated origin, display and row-limit
settings. Explicit numeric error cells retain their error code and
text unless raw text is requested.
Browser and Node HTML import preserve the first external cell link.
Import and export share the existing scheme filter.
HTML import generates date and raw-value settings once per table.
Parsed dates retain a default or requested date format, and the
cell's format attribute is used when value parsing supplied none.
The Node HTML parser gathers text and link targets in one cell traversal.
In an alternating 36-read Node v25.9.0 comparison using 5,000 cells,
median read time fell from 22.67 to 20.61 ms without links and from
45.07 to 40.55 ms with links. The linked fixture's p95 rose from
54.76 to 59.93 ms. Full workbook results compared equal in both cases.

Array and JSON worksheet construction use generated input settings.
Array dates use the shared number formatter, and JSON updates use the
shared date-format classifier to preserve existing date formats.
The shared date formatter supports quoted and escaped calendar
literals. Reference checks cover both date systems, serials around
the fictitious 1900 leap day and date-time formats with literals.

XLSB applies the same style implications as XLSX. A binary blank
record produces a stub with an undefined value and no number format.
The benchmark explicitly requests number formats on both packages,
so the historical reader and the corrected reader do equivalent work.

ExcelJS worksheet, row and column iteration share the generated
`includeEmpty` default. Tests cover both callback overloads and
the sparse traversal order.
ExcelJS sparse row iteration checks stored values and merged ranges
without sorting cell columns. A warm Node v25.9.0 comparison of 80
alternating reads on a 500-row sparse sheet reduced the median from
0.186 to 0.076 ms at width 8 and from 0.549 to 0.082 ms at width 100.
The callback visited the same 375 row indices before and after.
ExcelJS populates its worksheet row and cell cache from one typed Rust
batch of coordinates and presence bits. The batch preserves empty
stored rows and cells. For the first `eachRow` after loading a
500 × 50 sparse workbook, 36 alternating Node v25.9.0 samples per
adapter measured median 20.31 → 8.91 ms and p95 34.72 → 11.75 ms.
The timing excludes workbook load and both paths visited 500 rows.
Rust read views expose coordinates from the existing worksheet index.
The ExcelJS row census, SheetJS sparse projection and the Wasm grid
limit check reuse them without parsing A1 references again.
Repeating the 500 × 50 first-iteration comparison against the typed
batch before coordinate reuse gave median 8.90 → 7.97 ms and p95
11.69 → 11.45 ms across 36 samples per adapter.

The XLSX projection passes its row limit to Rust. The sparse-cell
reader validates cell structure and skips value decoding and transfer
for cells beyond the limit. The adapter still computes the public
ranges and preserves unprojected rows on save.
Before the row-index shortcut, the value-transfer change on a
700 × 48 mixed-cell workbook with `sheetRows: 20` reduced Node median
from 196.05 to 151.26 ms and p95 from 211.05 to 163.59 ms across
36 alternating reads per adapter. The row-index shortcut also avoids
opening cells in later rows while retaining sheet-level index checks.
On a reference-generated 700 × 48 mixed-cell workbook with the same
cutoff, 36 alternating reads per adapter compared the value-transfer
change with and without the index shortcut. Median was 142.53 →
134.40 ms and p95 was 187.59 → 162.27 ms. Projected sheets matched.

The XLSX sparse projection transfers numeric row and column pairs
alongside the cell values. JavaScript builds public addresses once and
appends comment-only cells in the same coordinate representation.
On a 300 × 20 dense worksheet with mixed numbers and strings and
`cellText: false`, Node v25.9.0 measured median 33.95 → 29.79 ms and
p95 52.01 → 43.55 ms across 36 alternating reads per adapter. Projected
sheets compared equal.

The XLSX adapter resolves a number format ID and code once per format
request. The Rust method keeps custom formats and applies a display
override only to built-in format 14.
For a 300 × 20 numeric sheet with built-in and custom formats and
both `cellNF` and `cellText` enabled, 36 alternating Node v25.9.0
reads measured median 130.41 → 104.06 ms and p95 147.58 → 121.28 ms.
The projected sheets compared equal.

The XLSX declaration defaults `cellText` to true, matching the pinned
SheetJS reader. Explicit false suppresses display text. An explicitly
undefined option receives the same default as an omitted option.
For the same 300 × 20 formatted numeric fixture, 36 alternating
Node reads with and without display text measured median 101.35 ms
and 28.72 ms respectively. The two modes return different cell data.

XLSX transfers effective style IDs with the cell batch when number
formats or display text are requested. The adapter resolves each style
once per worksheet, including the built-in date display override.
A 36-sample alternating Node v25.9.0 comparison on the same 300 × 20
formatted fixture measured median 107.23 → 36.52 ms and p95
131.44 → 43.18 ms. Both adapters produced equal projected sheets.
The same style batch supplies date classification when `cellDates`
is enabled, including when other format outputs are disabled. On a
300 × 20 worksheet with alternating date and numeric styles, 36
alternating Node v25.9.0 reads per adapter measured median
92.13 → 32.67 ms and p95 97.45 → 35.97 ms. The projected results matched.

When `cellStyles` is enabled, the XLSX batch includes explicit cell
style IDs for fill lookup. Fill projection still resolves the theme
and caches each style once per worksheet.
On an ExcelJS-generated 300 × 20 numeric sheet with alternating solid
and default fills, 36 alternating Node v25.9.0 reads per adapter
produced equal sheets. Median was 37.00 → 33.45 ms and p95 was
42.04 → 40.43 ms.

Literal Excel errors and ISO dates are read with the XLSX sparse
cell batch. This measurement predates the formula batching below.
On a 200 × 20 mixed fixture, 36 alternating Node v25.9.0 reads
produced equal sheets. Median was 28.45 → 25.39 ms and p95 was
32.46 → 33.84 ms.

The XLSX sparse batch includes ordinary and translated shared formulas
with their cached results. It uses the worksheet's existing guarded
ranges, including the merge-anchor exception, for the batch. For a
20 × 20 ordinary formula sheet, 36 alternating Node v25.9.0 reads per adapter measured
median 23.19 → 4.04 ms and p95 27.16 → 7.50 ms with equal output.

Stored array results also travel in the sparse batch. The formula field
contains an index into the worksheet's array metadata. JavaScript uses
that metadata to preserve the anchor formula and dynamic flag and to
supply the range to result cells. It checks the ranges for note-only
coordinates added after the batch.

On a synthetic 20 × 20 worksheet with one populated array, 36
alternating Node v25.9.0 reads per adapter measured median
3.55 → 3.26 ms and p95 5.55 → 3.59 ms. The projected sheets matched for
default reads and for dense, row-limited reads with formulas hidden.

The SheetJS text-read declaration now generates format-specific checks
for SpreadsheetML, DIF, SYLK, EtherCalc and RTF. Each reader invokes its
check before parsing records, after the shared input validation and
format selection. The public error code and format-specific message
remain the same. The same nonnegative-integer rule supplies the row
limit checks for DIF, SYLK, EtherCalc, RTF and FODS.

ExcelJS table creation uses generated declarations for the table, style
and column option objects. These declarations check the supported own
keys before reading values with ordinary JavaScript property access.
The adapter still validates the relationships among values and applies
the edit through the existing Rust transaction.

The ExcelJS worksheet protection declaration supplies the permission
names, defaults and boolean checks for the adapter. The generated
mapper reads own data descriptors in permission order, so accessors
are skipped and validation stops at the first invalid permission.
The same defaults determine how the adapter projects stored protection
attributes. Password hashing and protection edits run in Rust.

ExcelJS uses the generated header and footer contract for worksheet
creation and updates. The declaration defines the ordered fields and
validates text and flags. Text accepts null to clear a field. An omitted
value or accessor leaves the field unchanged. The adapter packs the
accepted values for the Rust write.

The ExcelJS page setup declaration defines the supported option groups
and validates the print flags. The mapper reads own data descriptors in
caller key order, invoking the adapter's existing value conversions for
page dimensions, printer settings and print references as it goes.
The same declaration supplies the groups used to read and write
worksheet settings.

Page setup also declares its scale interval, unsigned integer fields,
boolean fields and accepted orientation and printer enum values.
JavaScript serializes the validated values for the Rust setters and
normalizes the legacy printer spelling before writing.

ExcelJS page margins use a generated field order and per-entry
validator. The adapter still reads the existing margins before
validating entries in caller order and writes only when a supplied
margin changes.

ExcelJS now sends print flag updates to Rust in one call. Rust reads
the current flags, preserves flags omitted from the update, and skips
writing when the resulting flags match. The boundary distinguishes
worksheet read errors from edit errors.

ExcelJS reads each group of print settings from Rust once per worksheet
and committed document. Groups are loaded on demand so each getter
retains its own error and ignoreNodes behavior. Existing pageSetup
handles refresh on the next read after the document is replaced.
In Node v25.9.0, 36 alternating samples of 1,000 full pageSetup snapshots
on one already-loaded worksheet produced matching results. Median
was 50.44 → 8.85 ms and p95 was 56.92 → 9.91 ms.

ExcelJS bulk column definitions and per-column definition replacement
share a generated allowlist. The two setters keep their existing
orders for checking coordinates, reading properties and applying
defaults before the worksheet transaction.

ExcelJS row insertion and append share a generated style-mode table.
It declares which row supplies the style, whether empty cells inherit
styles and which modes append accepts. The existing edit transaction
applies the resulting styles.

ExcelJS worksheet view creation and replacement use the same generated
allowlist, field defaults and scalar validation. The adapter handles
pane rules that depend on the view state and canonicalizes cell
references before the Rust edit.

ExcelJS sends each worksheet view to Rust in one typed call. Rust
applies panes, display options and selection in the same order as the
previous calls. The workbook transaction discards the replacement
if any view write fails.

ExcelJS workbook window settings use generated field order, validation
and input defaults. The adapter uses the generated field order for the
Rust read and write records. Signed window positions, unsigned sizes
and optional sheet selectors retain their existing numeric semantics.

ExcelJS validates workbook date-system and calculation settings through
generated contracts. Each reads only the own data value and shares the
same validation whether the workbook already has an XLSX document or
is still being assembled.

ExcelJS declares the supported worksheet creation properties and
shares the generated outline summary fields and boolean checks
between worksheet creation and updates.

ExcelJS uses generated scalar validators for outline levels, default
row height and column width, and row descent. Worksheet creation and
existing worksheet edits share the same numeric limits.

ExcelJS fillFormula uses generated validation for the mode, formula
text and result input. The adapter retains the validation order and
evaluates result callbacks only after those checks.

ExcelJS shared fillFormula now sends cached results in a single Wasm
call. Rust clears the overwritten cell hyperlinks, converts date
results using the workbook date system, and applies a default date
format where needed. The existing workbook transaction covers all
three operations. On a synthetic 400-cell numeric shared formula,
36 alternating Node v25.9.0 samples measured median 5.45 → 5.16 ms
and p95 8.73 → 8.64 ms for the fillFormula call.

ExcelJS array fillFormula sends cached results to Rust in one call.
Rust uses the existing cell setters for follower values before
registering the anchor. Existing ISO date followers keep their
storage type, and the workbook transaction covers the full operation.
On a synthetic 400-cell numeric array, 36 alternating Node v25.9.0
samples measured median 13.08 → 12.65 ms and p95 20.77 → 17.70 ms
for the fillFormula call.

ExcelJS shared fillFormula clears individual cell hyperlinks in one
worksheet traversal. Multi-cell hyperlinks and relationship entries
are preserved. On a synthetic 400-cell numeric shared formula with
no pre-existing hyperlinks, 36 alternating Node v25.9.0 samples
measured median 5.00 → 4.84 ms and p95 7.34 → 7.79 ms.

Three fresh Node v25.9.0 runs measured the same operation with 0, 40 and
400 links inside the edited range, plus one link outside it. With 400
links, median fell from 14.00–16.48 ms to 7.18–7.37 ms. The p95
figures fell from 19.11–21.17 ms to 11.30–14.63 ms. Tail latency varied at the lower link counts. Both versions
produced byte-identical XLSX output for each fixture. See the
[shared formula benchmark](../../spikes/adr-0022/README.md#shared-formula-write-with-existing-hyperlinks).

Array fillFormula now shares the same batched hyperlink removal. The
candidate document clears single-cell links inside the array range
before writing results, and the transaction discards those removals
if writing fails. For a 400-cell range with a hyperlink in every cell,
36 alternating Node v25.9.0 samples measured median 25.57 → 19.70 ms
and p95 27.18 → 23.79 ms. Output bytes matched across builds.

Array fillFormula reuses worksheet classification across scalar
followers. Each scalar still runs the existing cell-level edit checks.
On the 400-cell numeric benchmark, the fixture with one out-of-range
link measured median 11.34 → 3.59 ms and p95 12.77 → 5.09 ms. With
400 in-range links, median was 19.92 → 4.92 ms and p95 was
21.36 → 6.63 ms. Output XLSX bytes matched for each link count.

Array fillFormula shares worksheet classification between scalar and
error followers. Error followers still use the same cell edit checks
as direct ExcelJS error writes. With 400 cached division-by-zero
errors, 36 alternating Node v25.9.0 samples measured median
11.90 → 3.94 ms and p95 14.13 → 5.36 ms. The output bytes matched.

Array fillFormula reuses the same worksheet classification for date
followers. Existing ISO storage and numeric date formatting follow
the same cell checks as before. With 400 date results, 36 alternating
Node v25.9.0 samples measured median 26.83 → 10.22 ms and p95
30.38 → 13.56 ms. The output XLSX bytes matched.

Shared and array formula fills read the workbook date system lazily
and reuse its offset for the rest of the operation. ISO date followers
keep their existing storage path. On the 400-cell array-date workload,
36 alternating Node v25.9.0 samples measured median 9.97 → 9.78 ms and
p95 15.55 → 11.73 ms. Output XLSX bytes matched.

ExcelJS fillFormula computes the first merged-cell overlap in
row-major order once per committed document. It refreshes the
intersection if a result callback edits the document, and stops
before requesting the result for an overlapped cell. With 300 merges
outside the 400-cell write range, 36 alternating Node v25.9.0
samples of shared fill measured median 6.05 → 4.90 ms and p95
9.43 → 7.81 ms.

Rust now computes the first merged-cell overlap for an ExcelJS formula
fill directly from the worksheet ranges. The adapter retains callback
ordering and repeats the query when a callback replaces the document.
It receives only the first overlapping cell's row-major offset.

ExcelJS unMergeCells uses Rust to test the selection and remove all
intersecting merge records in one edit. Rust plans all XML removals
before applying them and updates the merge count once. A selection
with no intersecting merges still leaves the original bytes intact.

On a synthetic worksheet with 301 merges, three Node v25.9.0 runs
measured median 2.704–2.706 → 0.589–0.595 ms for removing 300 of them.
Selections removing zero, one or 300 merges produced identical output
across builds. See the [unmerge benchmark](../../spikes/adr-0022/README.md#unmerge-intersecting-ranges).

ExcelJS mergeCells, mergeCellsWithoutStyle and unMergeCells accept
rectangular objects and address or coordinate tuples in addition
to the positional overloads. An optional sheet name on an endpoint
pair does not change the worksheet receiver. These forms normalize
to the same validated range before the Rust edit.

The ExcelJS merge methods also accept absolute cell addresses and
sheet-qualified range strings. The worksheet receiver remains the
target, including when a quoted name with escaped apostrophes appears
in the reference. The adapter passes normalized coordinates to Rust.

ExcelJS mergeCells copies the anchor's effective cell format to
covered cells through the Rust style writer. mergeCellsWithoutStyle
preserves those cells' original formats. Both operations keep their
value-clearing behavior and use the existing workbook transaction.
This implements the persisted format at merge time.

ExcelJS unMergeCells resets explicitly styled followers to their
inherited row or column format before removing merge records. The
Rust adapter uses the worksheet's existing row-before-column format
resolution. ExcelJS's per-style-component combination and shared
mutable style identity remain separate compatibility work.

For explicitly styled merge followers, the unmerge reset now combines
the row and column formats by component in Rust. Where the row omits
a component, the planner takes the column's component and interns the
result in the existing style registry. Unstored follower cells and
shared mutable style identity remain outside this slice.

Unmerge also combines defaults for unstored followers at intersections
of styled rows and columns. Rust walks stored rows and caches column
style lookups. Followers with only one inherited style can continue
to use their existing row or column format without a cell record.

On the unstyled 301-merge fixture, one Node v25.9.0 run measured
median 0.634 → 0.640 ms for removing 300 merges. The output bytes
matched the preceding build.

For merges created by ExcelJS mergeCells in the current session,
individual style setters synchronize still-linked cells in the same
Rust transaction. Direct whole-style assignment detaches that cell.
The adapter preserves the resulting formats on save and does not
infer a shared mutable style object from a loaded file's style indices.

Row and column splices carry session-local merge style links and
detached cell addresses to their rewritten ranges. The adapter checks
the ranges against the Rust candidate before committing the splice,
and publishes the new style links only after the edit succeeds.

An insertion inside a session-linked merge styles the new band from
the first surviving linked member. Rust writes the new cells' format
in the same candidate as the grid insertion. Existing detached
members and their formats stay independent.

ExcelJS cell, row and column style setters share a generated
allowlist. The alignment contract declares accepted alignments,
rotation and indentation bounds, and boolean flags. JavaScript
encodes validated values for the Rust style writer.

ExcelJS font options use generated defaults and validation shared
by cell, row and column style edits. The adapter translates accepted
underline forms and encodes colors for Rust.

The ExcelJS style declaration now covers cell protection defaults and
boolean checks and the outer border shape. The generated order of
border sides is shared by the adapter's encoding and decoding.

ExcelJS fill declarations now define the supported pattern names,
the pattern object's keys, and the path gradient center defaults.
The adapter retains gradient geometry and stop-order validation.

Gradient fill validation uses generated bounds for angles and for
path and stop fractions. The adapter validates the endpoints and
the ordering of stops before handing the fill to Rust.

The ExcelJS border declaration also supplies the supported line
styles and the boolean check for diagonal direction flags. The
adapter retains side-specific errors and encodes accepted colors.

ExcelJS's tab and style color encoders use generated object and value
checks for ARGB, theme and indexed colors, and tint. The adapter
retains the precedence of ARGB over theme and indexed colors and the
narrower index limits for styles.

ExcelJS cell and range data validation share generated rule keys,
types, operators, message fields and error styles. The adapter
checks the required formula count for each rule before encoding it
for the Rust validation writer.

ExcelJS sends typed millisecond bounds with each date validation
write. Rust converts those bounds using a single workbook date-system
lookup per rule. Formula strings still pass through as supplied.

Date validation reads reuse the workbook date-system offset for all
numeric bounds in each rule. Formula strings retain the adapter's
existing numeric coercion and projection behavior.

ExcelJS data validation and conditional formatting now share a
generated comparison-operator list, the two-bound operator subset
and the checks for numeric and formula-string operands. The two APIs
retain their own rules for omitted operators and date operands.

ExcelJS visual conditional formatting shares generated threshold
checks, and data bars and icon sets read their defaults from the
declaration. The Rust writer retains validation of the complete
visual rule.

ExcelJS conditional rules use generated allowlists for each rule type.
The data bar and icon set declarations share their field lists with
their generated default readers.

ExcelJS sends each conditional-formatting block to Wasm in one call.
Rust computes the priority floor from the explicit priorities, then
uses the existing ordered rule and differential style writers. The
workbook transaction rolls back the block on a later rule refusal.

The [conditional-formatting benchmark](../../spikes/adr-0022/README.md#conditional-formatting-block-write)
isolates the call batching with the same Wasm build on both sides.
For a 200-rule block, three rounds measured median 14.374–14.760 ms
before batching and 13.977–14.282 ms after batching. All measured
rule counts produced byte-identical XLSX output.

A conditional-formatting block reuses the differential format ID for
identical encoded styles. The cache is local to the block and the
existing Rust registry still resolves each distinct style. On the
200-rule synthetic fixture with one repeated style, 36 alternating
Node v25.9.0 samples measured median 15.189 → 12.569 ms and p95
17.812 → 15.858 ms. Output XLSX bytes matched at every rule count.

ExcelJS caches decoded differential formats across reads of the same
worksheet document. Rule styles are still copied for callers and the
cache refreshes when an edit replaces the document. On one loaded
worksheet with 100 distinct differential formats, 36 alternating
Node v25.9.0 samples of ten reads measured median 31.39 → 7.04 ms
and p95 33.82 → 8.22 ms. The projected rule blocks matched.

ExcelJS reuses the full conditional-formatting projection for each
committed worksheet document. Each read returns a detached copy of
the rule blocks and their extension associations. In the 100-rule
Node v25.9.0 workload, 36 alternating samples of ten reads measured
median 7.37 → 1.96 ms and p95 9.08 → 3.61 ms versus the
style-only cache. The projected rule blocks matched.

On the first conditional-formatting read, ExcelJS sends the unique
referenced differential-format IDs in one Wasm call. Rust walks the
style table once and returns the requested records, with absent IDs
omitted. The adapter still copies each rule's style independently.

For a loaded worksheet with 100 distinct differential formats, 36
alternating Node v25.9.0 samples of the first conditional-format
read measured median 4.33 → 1.86 ms and p95 6.40 → 3.53 ms.
The projections matched.

ExcelJS table handles share the decoded Rust table projection for each
committed worksheet document. Pending edits are local to each handle.
After a document replacement, existing handles resolve against the new
projection, so they see committed changes and detect removed tables.
A named lookup builds only the requested handle.

For a loaded worksheet with six ten-column tables, 36 alternating
Node v25.9.0 samples of five reads of every table's theme, row-striping
flag, column names, and filter flags measured median 217.90 → 0.81 ms
and p95 233.29 → 1.48 ms. Property values matched. A browser mutation
that disabled projection reuse failed the Wasm call-count check.

ExcelJS table creation now declares field validation for table options,
style options, and column options in the adapter contracts. The generator
emits the existing defaults and validation errors together. The checks
for a custom totals formula and for row width still use the surrounding
table and row data. Browser tests verify that invalid options leave
the workbook bytes unchanged. Disabling the generated nonempty-row
check makes the test fail on the public error code.

Field validation can now depend on the value of another option in the
same object. The table column contract requires a nonblank formula for
the custom totals function. Characterization tests cover missing and
blank formulas and round-trip custom and built-in totals in one table.

Existing ExcelJS table edits use generated scalar validation for themes,
style flags, column names, and filter buttons. Table creation, read
projection, and commit share the same ordered style-flag list used by
the Rust binding. Browser tests verify rejected assignments preserve
pending changes, which then round-trip on commit. Disabling the
generated style-flag check fails the invalid-assignment test.

ExcelJS table commit now passes all changed filter-button flags to one
Rust call. Rust validates every column and updates one clone of the
table part, preserving filter criteria. The native table tests verify
that an invalid column leaves the archive byte-identical and that a
successful batch changes only the table part.

Table header renames now clear hyperlink metadata in the same Wasm
call as the header writes. Rust matches the selected cells in one
worksheet XML pass and preserves unrelated single-cell links and
multi-cell links. The adapter still publishes the candidate only
after the writes and hyperlink cleanup both succeed.

Rust now implements a table data-row append using the cell-write plan
and a staged table part in the same registry transaction. It extends
the table and its filter range, retains the table identity, and updates
the worksheet's overlap context. The current append path refuses tables
with totals, calculated columns, sort state, or uses which would need
reference updates. The native test covers persistence, overlap checks,
cells outside the table, and byte-identical refusal.

The ExcelJS Table.addRow adapter now stages scalar row appends until
commit and passes all pending rows to the Rust append operation.
An explicit zero-based row position must be the staged tail and
becomes stale if another handle changes the committed range.
Missing values are padded with empty cells. Browser tests cover
save and reload, staged visibility, concurrent handles, and rollback
of a style change when a table with totals refuses the append.

ExcelJS table appends now also accept ordinary formula cells without a
cached result. The binding distinguishes formulas from text and uses
the same Rust cell-write plan as native table appends. Browser tests
verify that staged formula text is captured at addRow time and remains
a formula after reload. Suppressing the formula marker fails the test.

The Rust cell batch supports ordinary formulas with a cached result
through `CellWrite::FormulaWithResult`. It shares cache validation and
encoding with the single-cell writer. ExcelJS table appends preserve
scalar and Excel error caches in that same row transaction. Date
results still require number-format handling in this adapter path.

Table.addRow now accepts dates as standalone cells and as ordinary
formula results. The adapter sends complete UTC timestamps and Rust
writes explicit ISO date values through the same validation as other
date writes. The operation preserves milliseconds without depending
on the workbook's date serial system or changing the cell style.

The ExcelJS DSL now generates the row-array check shared by table
creation and row append, and the optional nonnegative integer check
for addRow's position. Table appends distinguish Excel error values
from strings containing the same text. Rust validates a batched error
with the same code as a single-cell error.

ExcelJS Table.removeRows now supports trailing data rows with a default
count of one. Removed cells are cleared and table and filter ranges
are shortened by the shared Rust row-edit transaction. The adapter
stages changes until commit and can cancel pending appended rows.
A table must retain a data row. A removal that intersects a hyperlink
is refused until hyperlink relocation or removal is supported.

Trailing table row removal now stages cleanup of single-cell links
in the same Rust transaction as the cell writes and range changes.
Links outside the removed rows and their relationships are preserved.
If a multi-cell link touches the removed range or a link has no
parseable position, Rust refuses the edit before applying the plan.

Trailing table row removal extends hyperlink handling to ranges.
A fully covered link is removed. If the retained cells form one
rectangle, Rust clips the reference while retaining the target and
other link attributes. If the remainder needs multiple rectangles,
the edit is refused with the original bytes preserved.

The native table row edit can now remove interior rows for plain
data with matching source and destination styles. Rust snapshots the
surviving values before writing them at their new positions, clears
the former tail, and shrinks the table and filter in the same edit.
Interior shifts currently refuse formula cells, complex cell values,
links, comments, validation, and mismatched styles. ExcelJS still
routes only tail removal to Rust pending the staged operation wiring.

ExcelJS now stages interior removals by their zero-based positions in
the table as edited so far. The adapter handles removals of uncommitted
appends locally and sends committed row removals to Rust in call order.
The adapter publishes the candidate only if every pending operation
succeeds. Rust's interior-shift restrictions still apply.

Interior table removal now carries the surviving cells' explicit
styles with their values. Rust uses the existing style-index requests
in the cell-write transaction and restores the default index when a
source cell has no explicit style. The native test checks both
directions, and ExcelJS reads the moved number format after reopening.

Interior table row removal now preserves ordinary formula text and
cached results when it moves a cell. This matches the ExcelJS 4.4.0
table behavior observed with relative references: row data is stored
again without translating the formula. Rust includes these writes
in the same dependency and table-range transaction. Nonordinary
formulas and linked cells retain their existing guards.

Interior table row removal now relocates hyperlinks wholly within the
shifted cells. It removes links on deleted cells and leaves unrelated
links in place, preserving link targets and relationship IDs. Rust
stages those changes alongside cell writes and range changes. A link
that straddles shifted and stationary cells is refused before apply.

Interior row removal can now move explicit ISO date cells with their
styles and millisecond precision. The table editor admits this
representation through the existing cell classifier, while formats
that use strict scalar classification keep the previous behavior.

The native table row editor now also inserts rows at a zero-based
position. It shares the cell snapshot and write planning with
interior removal, and the table growth checks with append. Rust
writes the inserted values and moves the following cells, including
their number formats and ordinary formulas, in one transaction.
Interior insertion currently refuses intersecting hyperlinks and
other unsupported positional metadata. Wiring the staged operation
to ExcelJS remains open.

ExcelJS Table.addRow now accepts a zero-based interior position.
The adapter orders committed-row insertions and removals together,
while batching pending tail rows. Positions are evaluated against
the staged table and the candidate workbook is published only after
all edits succeed. The browser test stages append, insert, and
remove operations together and verifies their order after reload.

Interior Table.addRow now moves hyperlinks wholly within the shifted
rows, retaining their target and relationship. Rust applies the link
changes with the grid and table ranges. An insertion that would
overwrite a link below the old table or split a range is refused
before any workbook changes.

Row insertion and removal now handle one table totals row. Rust moves
the totals cells with their styles and formulas, invalidates their
cached results, and sets the filter range to exclude the totals row.
External table references still use the existing resize guard.

Table row shifts now check the edited range against worksheet
validation references in one pass. This also checks x14 validation,
using the same overlap logic as validation creation. In Node.js
25.9.0, deleting the second data row from a 400 × 8 table with 64
non-overlapping validation rules took 47.54 → 24.63 ms median and
57.30 → 31.81 ms p95 over 20 alternating samples per build. The
timed edit included commit and excluded workbook load. Output
archives were byte-identical.

Rust supports appending a table column with an ID above the existing
IDs, leaving those IDs and the table identity intact. It updates the
column list, table and filter ranges, header and data cells, and new
column totals in the same transaction. The table creation and append
paths share the column metadata encoding. The ExcelJS addColumn
binding remains open.

ExcelJS Table.addColumn stages tail columns until commit. Creation and
append share column-option decoding and the cell batch codec used by
row edits. Staged row edits can precede column appends in one commit,
and the committing handle exposes its appended columns after success.
Interior column insertion and row operations staged after column
append remain open.

Rust supports trimming table columns from the right edge. It clears
their worksheet cells, drops their filter entries and links, and
shrinks the table and filter references in one transaction. Existing
column IDs, styles on cleared cells, and neighboring cells are
retained. The table must keep at least one column. ExcelJS wiring and
interior column removal remain open.

ExcelJS Table.removeColumns stages tail removals with a default count
of one. It can discard a pending append, or remove committed columns
and stage replacements in the same commit. The committing table
handle retires removed column handles and binds the replacements.
Interior removal and row operations staged after column operations
remain open.

Rust now removes interior table columns. The row-shift cell planner
also supports horizontal moves, keeping explicit styles, ordinary
formulas and cached results. Surviving table column IDs are stable,
filter positions are remapped, and wholly moved hyperlinks retain
their targets. Deletions that would leave unsupported table
references are refused before applying changes. The ExcelJS
removeColumns interior binding remains open.

ExcelJS now stages interior column removals and applies them in order
through Rust. Column handles and pending metadata edits follow column
IDs, so renaming or changing a filter before commit also works when
the column moves. The committing handle retains surviving objects
and retires removed ones. Row edits still precede column edits in
a mixed commit.

Rust supports inserting a table column at a zero-based position.
Existing table column IDs and filter criteria follow their columns.
The horizontal cell planner moves values, ordinary formulas, styles,
and links alongside the inserted values and table growth. The
ExcelJS position argument is still landing separately.

ExcelJS Table.addColumn supports a zero-based insertion position.
The staged column order holds surviving column IDs and inserted
values, so removeColumns can target either kind before commit.
The candidate deletes original columns right-to-left and inserts
new columns in final order. A mixed operation must retain at least
one original column during the Rust deletion phase, and row edits
must still be staged before column edits.

Rows can now be added or removed after staged column edits in the
same ExcelJS commit. The adapter maps row assignments by existing
column ID, and adjusts the pending inserted-column values in place.
It still publishes one candidate workbook. A mixed-order test
checks inserted values, removed rows and columns, and the final
shape after reload.

ExcelJS can replace every committed table column in one commit when
an adjacent staging column is available. The adapter creates the
first replacement before deleting the original columns. A temporary
header resolves a duplicate name during staging. The candidate
guards the adjacent column against overwriting both values and
styled blanks. Same-name replacement in headerless tables and
replacement when the staging column is unavailable remain open.

Rust can now replace every table column directly, using the same
column cell planner as insertion. It uses the final table footprint,
assigns fresh column IDs, resets filter entries, and stages the
table and cells together. Same-name replacement preserves the
adjacent worksheet content. The ExcelJS binding has yet to switch
from the staged-column workaround to this Rust operation.

ExcelJS now sends full column replacements directly to Rust in one
batch. The adapter no longer creates an adjacent staging column or
renames an original header. The browser test covers a populated
adjacent column, same-name headerless replacement, and a formula
cache in the replacement values.

The native `Worksheet::set_table_column_total` operation edits a
column total as a label, built-in function, custom formula, or clear.
It shares the encoding and cell-write validation with the column
planner, and commits the worksheet cell and table metadata together.
The ExcelJS staged setter binding remains open.

ExcelJS column totals properties are now staged and validated by
the same contracts as column creation. At commit the adapter resolves
each surviving column by ID and sends its final total to Rust after
row and column structure changes. The browser tests cover a missing
custom formula, a successful label and custom formula, and clearing
a total without changing its data cells.

ExcelJS TableColumn.totalsRowResult reads the totals cell's formula
cache and stages caller-supplied results. Rust validates and writes
them with the totals formula. This uses the ordinary formula result
codec, including date and Excel error results. Column definitions
passed to addTable or addColumn do not yet accept a result.

The adapter accepts totalsRowResult in addTable and addColumn column
definitions. A result requires a formula or built-in subtotal.
The adapter batches all totals edits for a commit in Rust so one
new total cannot invalidate another result supplied in the same
operation. Appended column totals are installed after structural
changes, keeping intermediate table edits independent of the
new structured formulas.

Rust can apply a partial style to a table column with one cell
write plan. The style registry derives each cell's format from
its current style and updates only supplied groups. Header and
totals cells are included. Cell values remain intact, and the usual
style-dependent formula cache invalidation applies. The ExcelJS column-style binding remains open.

ExcelJS TableColumn.style stages a partial style with the existing
cell style contract. At commit the adapter resolves the column's
final position and calls the Rust style patch. The assigned object
is captured at setter time. The workbook persists the styles on the
cells, and the assigning handle retains the requested patch.

The adapter applies the style patches before writing any explicitly
supplied totals results, so a mixed commit retains those results.

ExcelJS accepts column style patches in addTable and addColumn.
The adapter applies the captured style to the column after
structural changes and before explicitly supplied totals caches.
The returned handles expose the style assigned during creation;
the cell-level formatting persists after reopening.

ExcelJS submits all column style patches in a single Wasm call.
Rust plans the style writes together and reuses the derived style
index for each repeated pair of patch and source style. In Node.js
25.9.0, a commit applying number format and alignment to 20 columns
of an 80-data-row table took 24.69 → 10.78 ms median and
25.58 → 11.27 ms p95 over 20 alternating samples per build. The
commit timing excludes input load and staging. Output archives were
byte-identical.

ExcelJS addTable sends rectangular data containing strings, numbers,
booleans, and blanks to Wasm in one call. Rust classifies the sheet
once, validates and plans the cell writes together, and removes
overwritten single-cell hyperlinks within the data range. Tables
containing structured cell values use the existing cell writers.

In Node.js 25.9.0, addTable on 200 rows by eight columns took
140.09 → 14.23 ms median and 143.79 → 16.33 ms p95 over 20 alternating
samples per build. The timing excludes workbook serialization. With
fixed workbook timestamps, the serialized archives were byte-identical.

The same addTable batch accepts Excel error cells via the table
cell kind encoding. Rust emits typed errors alongside ordinary
scalars. For a 200-row, eight-column table with one `#DIV/0!` cell
per row, the median was 139.69 → 14.35 ms and p95 was
143.95 → 15.52 ms under the same protocol, compared with the
original per-cell writer. Serialized archives were byte-identical
with fixed timestamps.

The addTable batch also accepts ordinary formulas with scalar or
error caches. Rust flushes preceding value writes before each
formula and preserves row-major edit order so changing a referenced
cell later still invalidates the formula's cache. Shared and array
formulas and formulas with date caches use the established writer.

For 200 rows by eight columns with one ordinary formula per row,
creation took 154.97 → 18.83 ms median and 159.42 → 21.24 ms p95
compared with the original per-cell writer. The same 20-sample
protocol and fixed timestamps produced byte-identical XLSX files.

Mixed addTable payloads batch each contiguous row-major run of
supported values. The adapter invokes the established writer at
specialized cells such as dates, hyperlinks, and rich text. Each
Rust batch clears hyperlinks only at its own coordinates, including
when the batch crosses a row boundary.

For a 200-row, eight-column table with 20 date cells interspersed
with ordinary values, creation took 146.03 → 22.32 ms median and
150.87 → 26.08 ms p95 against the original per-cell writer.
The same 20-sample protocol and fixed timestamps produced
byte-identical XLSX files.

The internal row-write binding accepts the row width followed by
an optional insertion index in one typed array. The column insertion
binding accepts the target index with its header and filter metadata.
Totals use the batched binding for both single and multiple columns.
The public ExcelJS adapter continues to expose the same table methods.

The ExcelJS option DSL validates the conditional-format block,
rule priority and stop flag, and top-rank and above-average options.
It supplies the default rule flags before the adapter packs the batch
for the existing Rust conditional-format planner.

Data-bar threshold count, length bounds, and visibility and
icon-set field types and defaults are defined by the ExcelJS DSL.
The adapter checks that data-bar lengths are ordered and encodes
ARGB colors. Rust applies the resulting visual rules.

The ExcelJS image-anchor contract defines the default and accepted
placement modes and validates hyperlink fields. The adapter resolves
the anchor against sheet dimensions and Rust writes the drawing and
its relationships.

The option DSL validates exactly one image source (buffer or base64)
and exactly one anchor shape (bottom-right corner or extent). The
source contract distinguishes undefined from supplied values and the
anchor contract keeps the existing truthiness check. Node-specific
filename input remains open.

Image marker contracts validate fractional grid coordinates and
nonnegative safe-integer offsets. The adapter still uses worksheet
row heights and column widths to resolve fractional coordinates.

An ExcelJS 4.4.0 Node probe confirmed that a filename may be missing
when the image is registered and placed. Writing then rejects with
ENOENT and succeeds on retry once the file exists. Filename support
therefore needs to preserve a deferred media source through
synchronous placement and resolve it while saving.

The Rust workbook and Wasm binding can replace bytes in an existing
image part after checking that the new image matches its registered
content type. Drawing anchors and relationships retain their part
references. The media deduplication index is invalidated after a
successful replacement. This lets a save candidate resolve deferred
image bytes without rewriting drawing XML.

Node ExcelJS registers filename sources synchronously. A unique
internal media marker lets the Rust editor place a drawing or
background before the file exists. Each XLSX save reads the sources
and replaces the matching media in a private candidate. A failed read
leaves the live workbook unchanged, and a later save reads the current
file contents. Unplaced media is read but is not included in the archive.

Node XLSX output reads each distinct deferred filename once per save
and serializes the candidate directly using the requested ZIP options.
For 24 placed images registered separately against one PNG filename
with roughly 64 KiB of data, a stored ZIP save took 6.39 → 2.28 ms
median and 7.42 → 4.01 ms p95 in 20 alternating Node.js samples.
With fixed workbook timestamps, the archives were byte-identical.

The Node filename option contract shares the supported extensions
with the browser image registration contract and rejects combinations
with buffer or base64 before allocating an image ID.

The browser ExcelJS declarations expose buffered media. The Node
declarations also expose filename-backed media and allow filename
registration through workbook references reached from worksheets,
rows, columns, and cells.

The ExcelJS option DSL validates the sheet name and optional A1
address in defined-name location objects. Coordinate and range
resolution still use the adapter's existing address functions.

The generated ExcelJS contracts validate the defined-name model and
each entry's ranges array before replacement. Duplicate normalized
references are collapsed in insertion order.

ExcelJS submits each global defined-name model edit as one Wasm
batch. Rust stages the workbook XML once and applies the name changes
in order. Validation and mutation failures leave the input bytes
intact. Replacing 200 ranges took 40.71 → 7.25 ms median and
45.74 → 9.06 ms p95 across 20 alternating Node.js samples. The
timing excludes saving, and output archives with fixed timestamps
were byte-identical.

SheetJS draft XLSX serialization sends defined names, scopes, and
optional comments through one Wasm batch using the shared Rust
staging path. Numeric names and comments produce an adapter input
error. Writing a workbook with 200 names, half scoped and commented,
took 31.84 → 6.62 ms median and 36.87 → 9.51 ms p95 across 20
alternating Node.js samples. Fixed timestamps yielded byte-identical
archives.

The SheetJS DSL reads defined-name records from their own data
properties without invoking accessors. It validates names, scope
types, and comments before the adapter resolves each scope against
the workbook's sheet count and sends the batch to Rust.

During a defined-name batch, Rust traverses the staged name nodes
until the requested name and scope match. It reads formula text only
when needed for that entry. Compared with the prior batch, replacing
200 global names through ExcelJS took 6.63 → 2.92 ms median and
7.36 → 3.85 ms p95. SheetJS output with 200 names took 6.52 → 5.20 ms
median and 6.92 → 5.78 ms p95. Each measurement used 20 alternating
Node.js samples per build. Fixed timestamps yielded byte-identical XLSX.

ExcelJS hyperlink and rich-text cell values use the generated DSL
for accepted fields and required text. The cell and table adapters
share these contracts before Rust processes the corresponding edits.

The ExcelJS DSL defines allowed keys for ordinary formulas, range
formula masters, and shared formula members. The adapter rejects
accessor-backed results without reading them and resolves formula
ranges and caches through the existing writers.

The generated formula contracts read the optional result from its
own data descriptor. Both ordinary formulas and shared formula
members reject accessor-backed results before processing a cell edit.

The ExcelJS error-value enum and validation predicate are generated
from the same DSL definition. Both direct cell errors and formula
results use that predicate, including the additional supported
`#GETTING_DATA` code.

Browser and Node ExcelJS CSV readers validate the mapping callback
and date-format list through one generated contract. A refused
option fails before the reader adds a worksheet.

CSV output validates its mapping callback through the same DSL rule
as input. Node validates before opening a destination file, so an
invalid callback option leaves any existing file untouched.

The browser and Node CSV formatters share the DSL validation for
row transforms. Callback-style transforms remain supported, and an
invalid Node transform is refused before the output file is opened.

CSV output validates dateFormat and dateUTC through the worksheet
output contract and passes the resolved settings to each row. Node
rejects invalid date options before opening the destination file.

Browser CSV date formatting reuses the English month and weekday
names across cells. In Node.js running the browser adapter, CSV
output for 1,200 date cells took 224.63 → 218.94 ms median and
233.14 → 226.34 ms p95 across 20 alternating runs. The output bytes
matched in every run.

Wasm scalar reads reuse a successful sheet classification for the
current OPC package generation. Package changes invalidate the
classification, and ExcelJS still permits reading a merge anchor
while refusing scalar reads of its followers. For the browser
adapter's 1,200-date CSV workload, output took 235.64 → 17.84 ms
median and 259.72 → 25.71 ms p95 in 20 alternating Node.js samples.
The CSV bytes matched in every run.

ExcelJS formula, ISO-date, and error reads share the same
generation-aware worksheet classification as scalar reads. Browser
adapter CSV output for 1,200 formula cells took 534.37 → 54.50 ms
median and 567.61 → 64.85 ms p95 in 20 alternating Node.js samples.
The output bytes matched in every run.

ExcelJS's scalar probe uses a reserved NaN return value to select the
typed read path without raising a JavaScript exception. Stored
scalar numbers are finite. If the typed readers cannot project the
cell, the original scalar error is still surfaced. Browser adapter
CSV output for 1,200 formula cells took 65.33 → 41.12 ms median and
74.05 → 52.62 ms p95 over 20 alternating Node.js samples. The output
bytes matched in every run.

ExcelJS's complex cell lookup uses one typed Wasm call for ISO dates,
rich text, formulas, and array followers. Rust reuses the cell
reference and preserves typed read precedence. In the 1,200-formula
CSV workload the median was 40.42 → 40.54 ms, p95 changed from
44.57 → 43.88 ms, and the output bytes matched across 20 paired
samples. The change reduces boundary calls without a measurable
change in this workload's median.

Complex ExcelJS reads return already-converted milliseconds for
numeric formula results with date formatting. Rust uses the
workbook's date epoch and the adapter constructs the JavaScript
Date. For 1,200 numeric formula cells, CSV write median was
34.31 → 34.09 ms and p95 was 38.48 → 40.84 ms across 20 paired
Node.js samples. Output bytes matched in every run.

Expected non-scalar classifications no longer format a Rust error
for ExcelJS scalar probes. Strict scalar reads still generate the
original diagnostic. For 1,200 formula cells, CSV output took
36.70 → 34.71 ms median and 40.60 → 38.76 ms p95. Output bytes
matched across 20 paired Node.js samples.

CSV output can read ordinary values in one sparse Wasm projection.
Rust supplies numeric dates in milliseconds and the adapter
reconstructs sparse rows and their existing gaps. Sheets with
complex cells or links and output using custom callbacks use the
existing row readers. In 20 paired runs using a shared source XLSX,
1,200 date cells took 39.37 → 22.39 ms median and 52.01 → 26.01 ms
p95. The formula workload stayed near its baseline median
(33.24 → 32.56 ms). Outputs matched byte-for-byte for both workloads.

CSV sparse projection includes ordinary formula caches with simple
result types. Rust preserves the adapter's blank output for missing,
zero, and false formula results. Date and error formula caches and
shared and array formulas use the established readers. CSV output
for 1,200 ordinary formula cells took 33.44 → 18.76 ms median and
40.64 → 19.18 ms p95 across 20 paired Node.js runs with the same
input XLSX. Output bytes matched in every run.

CSV sparse projection resolves ordinary cell hyperlinks to their
targets, including the location fragment. It honors ignoreNodes.
With 1,200 rows and 12 links, CSV output took 61.34 → 18.11 ms median
and 69.21 → 19.21 ms p95 across 20 paired Node.js runs. Outputs
matched byte-for-byte.

CSV projection marks individual cells that still need the full
ExcelJS read path, so rich text, errors, and other special values can
share a sheet with projected scalars. The mixed workload (1,200 rows
with 12 rich-text and 12 error cells) took 34.07 → 19.32 ms median
and 39.95 → 21.36 ms p95 in 20 paired Node.js runs. Date, hyperlink,
and formula workload medians stayed within 3%, and all CSV outputs
matched byte-for-byte.

A worksheet with array or data-table ranges can use CSV sparse
projection for cells outside those ranges. Guarded stored cells
retain the individual reader, and merged sheets retain the row
reader. For a 1,200-row sheet with one two-cell array formula, output
took 33.23 → 18.65 ms median and 38.04 → 19.23 ms p95 across 20 paired
Node.js runs. All measured outputs matched byte-for-byte.

CSV projection covers merged sheets and emits positions for merge
followers even when the XLSX has no stored follower cells. These
positions use the individual reader to resolve the master. Ignored
merges do not emit followers. To bound temporary allocations, sheets
with more than 65,536 projected followers retain row-by-row traversal.
The 1,200-row workbook with a four-cell merge took 32.44 → 18.51 ms
median and 36.78 → 19.92 ms p95 in 20 paired Node.js runs. All output
bytes matched.

CSV sparse projection reconstructs each JavaScript row as the
formatter requests it. The flat Wasm projection stays available
for the duration of output. A mixed-sheet regression test verifies
that consuming the first row does not read special cells in later
rows. Across the six 1,200-row workloads, median changes were within
2% and outputs matched in every paired run.

Browser ExcelJS CSV streams send BOM, headers, and formatted rows
incrementally and await the writable stream before advancing. They
share the formatter with writeBuffer and preserve UTF-16 pairs
across chunk boundaries. The flat Wasm projection remains buffered.
In six paired 1,200-row writeBuffer workloads, the median changed by
less than 4% and the CSV bytes matched.

If browser CSV formatting fails after output has started, the writer
aborts the stream and releases its lock. The write rejects with the
original failure even if aborting also fails.

Browser ExcelJS CSV input decodes stream chunks incrementally and
releases consumed text as fields advance. It cancels the remaining
input when maxRows is met. Mapped rows are still staged together
before the worksheet cell write. Differential checks covered 475
combinations of inputs, options, and chunk boundaries.

A browser CSV field that spans chunks joins its scanned fragments
once, preserving leading whitespace under the existing trim policy.
Parsing a 1 MiB unquoted field in 1 KiB chunks took 278.17 → 14.23 ms
median across 10 paired Node.js runs. All fields matched the expected
text, and another 475 chunk-boundary comparisons passed.

Quoted CSV fields collect contiguous text runs and escaped
characters separately, joining them when the closing quote is
found. Parsing a 1 MiB quoted field containing embedded newlines
and escaped quotes in 1 KiB chunks took 28.37 → 4.16 ms median over
10 paired Node.js runs. All decoded fields matched, and 360
additional input, option, and chunk-boundary comparisons passed.

The browser CSV reader preserves comment CRLF boundaries even when
empty chunks intervene. The row-limit test also covers cancellation
after a terminal CR while the input stream remains open.

SheetJS writeFileAsync validates the callback through the generated
contract before serializing or calling the injected filesystem. The
three- and four-argument signatures share that validation.

SheetJS sync file output validates filenames through the generated
contract. The type:file overload reads file from its own data
descriptor and forwards the remaining format option descriptors.

The generated SheetJS file-output contract provides extension aliases
and the default format shared by writeFile and writeFileAsync. Both
infer from the extension when bookType is undefined. The write type:file
overload defaults to XLSX, matching SheetJS CE 0.20.3. Async output
rejects bookType accessors before serializing or writing.

The SheetJS XLSX cell hyperlink adapter generates the shape and value
checks for Target and Tooltip from the same declaration used by draft
and loaded workbook saves. Browser coverage verifies the round trip,
getter-free target validation, and byte-identical output after correcting
a refused edit.

The generated XLSX legacy comment contract validates the single-entry
list and its author and text fields for both draft and loaded workbooks.
An invalid field or an unsupported multi-comment list is rejected
before saving. Restoring the original comment after either rejection
produces byte-identical XLSX output in the browser regression suite.

Generated SheetJS worksheet metadata contracts validate print margins
and autofilter references. Margin projection and validation share the
same field order. Nullish margins retain the loaded value or use the
existing default. Autofilter reference getters are rejected without
invocation. Restoring rejected metadata edits preserves source bytes.

The generated SheetJS XLSX cell-shape predicates share the allowed
fields across new and loaded workbook saves. SpreadsheetML imports
extend the shared list with four source fields. The caller still
controls descriptor checks, required fields, and error context.
Unknown symbol keys are rejected in both save paths, and reverting a
refused loaded-workbook edit preserves its source bytes.

SheetJS workbook metadata now declares the visibility entry shapes,
visibility range, and date1904 option in the DSL. The public visibility
utility shares the generated range check with XLSX saves. Reverting
invalid visibility or date-system edits preserves the input bytes.

The SheetJS worksheet layout DSL declares the supported row and column
fields and emits the ordered size keys used by the adapter. Column
entries retain support for DBF metadata. Tests cover distinct row and
column field sets and verify that reverting rejected edits preserves
the original workbook bytes.

SheetJS merge and array-formula saves use generated worksheet
coordinate bounds. Merge lists also use a generated array check.
Browser regressions exercise the first row and column beyond the
worksheet limits and verify that restoring a refused merge preserves
the original XLSX bytes.

### SheetJS save measurements

Node.js 25.9.0 compared the generated cell-shape and merge-endpoint
checks against the former inline checks using the same Wasm, adapter
code, and deterministic creation timestamp. After six warm-up pairs,
32 alternating pairs produced byte-identical XLSX outputs. Times are
baseline to generated, in milliseconds. Each workbook had three merges
and used uncompressed output.

| Workload | Median | p95 |
| --- | ---: | ---: |
| New workbook, 64 cells | 1.881 → 1.860 | 3.076 → 3.118 |
| New workbook, 256 cells | 6.879 → 6.868 | 7.746 → 8.324 |
| Unmodified loaded workbook, 1,024 cells | 2.189 → 2.189 | 3.909 → 4.017 |

A separate scaling probe measured repeated new-workbook saves with 16
columns of sequential numbers, no merges, and uncompressed output.
After one warm-up per size, the medians of three runs were 23.9 ms for
512 cells, 64.9 ms for 1,024, 233.4 ms for 2,048, 879.1 ms for 4,096,
and 3,502.9 ms for 8,192. The draft path invokes Rust write_scalar for
each cell. The loaded-workbook write_scalar_batch currently requires
a declared extent, so it cannot substitute directly for writes into an
empty draft. The scaling probe did not profile the cost within Rust.

SheetJS drafts now submit consecutive plain scalar cells to the Rust
worksheet writer in one batch per run. Formula, date, and metadata
operations retain their ordering, and repeated addresses flush the
pending batch before the overwrite. The draft call opts into extent
growth. Loaded-workbook batches retain their declared-extent guard.

Node.js 25.9.0 measurements compared the previous package with the
rebuilt package in alternating order after one warm-up pair per
size. All outputs matched byte-for-byte, with fixed creation time
and compression disabled. The table reports medians of three pairs.

| Numeric cells | Previous | Batched |
| ---: | ---: | ---: |
| 512 | 21.814 ms | 6.668 ms |
| 2,048 | 234.588 ms | 18.124 ms |
| 8,192 | 3,416.195 ms | 61.043 ms |

The rebuilt package also saved a 100,000-cell dense draft in
707.398 ms median across three runs after one warm-up. Reopening
the 3,003,534-byte file confirmed the sheet extent and final cell.
Four additional mixed-cell fixtures, covering dense and sparse
inputs with and without shared strings, were byte-identical to
the previous package.

ExcelJS addRows now sends filled, row-major rectangles of plain scalar
values through the same Rust writer used to initialize table cells.
Sparse and mixed-value appends retain their ordered cell writes.
Row style inheritance runs after the values in both paths.

On Node.js 25.9.0, three alternating measured pairs after one warm-up
pair gave median addRows times of 26.245 → 9.389 ms for 512 numeric
cells, 263.230 → 26.713 ms for 2,048, and 3,584.113 → 119.218 ms
for 8,192. The benchmark timed addRows, which stages and serializes
the candidate workbook in this adapter. Subsequent writeBuffer
outputs were byte-identical for all pairs.

ExcelJS addRows now sends runs of scalar edits as explicit coordinates,
covering sparse and ragged input without touching holes. Complex values
retain their position in the write sequence. Rust checks ExcelJS edit
rules, applies the scalar batch, and removes only single-cell hyperlinks
at the edited coordinates. The adapter publishes the staged workbook
only after the operation succeeds.

Node.js 25.9.0, three alternating pairs after one warm-up pair per
workload, measured addRows against the preceding rectangle-only
implementation. All writeBuffer results matched byte-for-byte.

| Numeric values | Previous median | Batched median |
| --- | ---: | ---: |
| 1,024, alternating with blank cells | 90.363 ms | 37.292 ms |
| 4,096, alternating with blank cells | 993.468 ms | 100.194 ms |
| 8,192, dense | 131.974 ms | 117.444 ms |

ExcelJS row.values assignments to empty rows now reuse the sparse
Rust scalar batch, except on merge-anchor rows. Assignments to
populated rows retain the existing ordered replacement and clear
operations. The Rust bridge shares scalar decoding across SheetJS,
ExcelJS and table writes. ExcelJS scalar batches no longer allocate
table-cell kind and formula-cache vectors.

Assigning 2,048 numeric values across a single 4,096-column row
with alternating blanks took 278.721 → 64.632 ms median on Node.js
25.9.0. Three alternating measured pairs after one warm-up pair
produced byte-identical XLSX output.

ExcelJS column.values now batches sparse assignments when all target
cells are unoccupied and outside merges. Assignments to occupied or
merged cells keep the existing sequential semantics. With 2,048
values in alternating rows of a new column, Node.js 25.9.0 took
279.054 → 22.348 ms median across three alternating measured pairs
after one warm-up pair. Every saved XLSX matched its baseline bytes.

The SheetJS CSV and TXT write-option declarations now accept dateNF
and forward it to the existing date formatter. Explicit cell number
formats take precedence. Six text-output comparisons with SheetJS CE
0.20.3 matched for yyyy-mm-dd, m/d/yy and dd-mm-yyyy. Node tests
exercise both file-write overloads. Ten further comparisons matched
for quoted, escaped and unquoted calendar periods and fractional
seconds. Unquoted calendar periods use the default cell format,
while direct SSF.format calls reject the invalid format.

SheetJS raw JSON projection converts numeric cells with date formats
to Date objects, using the serial converter shared with XLSX reads.
Serials in Excel's fictitious leap day remain numbers. Thirty raw
projection comparisons with CE 0.20.3 matched under America/New_York.
Formatted JSON projection also follows the CE 0.20.3 date round trip,
including the local-time adjustment and the treatment of serial 59.
A matrix of 224 raw and formatted cases matched the CE reference
under America/New_York.

SheetJS JSON projection uses a generated cell-type allowlist and
rejects unrecognized types even when the corresponding custom header
is absent. The reserved __rowNum__ property retains its read-only
behavior when a projected header collides with it. Two hundred CE
0.20.3 comparisons covered supported and unknown types, reserved and
omitted headers, raw output, and default values.

SheetJS automatic JSON headers now track the next suffix per string
property key, avoiding repeated scans for wide rows with duplicate
headers. The counter uses Map so inherited object properties do not
participate. CE 0.20.3 comparisons matched for overlapping suffixes
and numeric display text colliding with a string header. On Node.js
25.9.0, the median of five alternating runs was 29.414 → 1.122 ms
for 1,024 repeated headers and 782.446 → 4.756 ms for 4,096.
Previous and new outputs matched in every timed pair.

Dense SheetJS array input now stages cells by numeric coordinates
without generating unused A1 strings. Node.js 25.9.0 converted
100,000 numeric cells in 13.497 → 8.379 ms median across nine
alternating measured pairs after three warm-up pairs. Every pair
produced structurally identical worksheet models.

SheetJS JSON and CSV exporters now reuse row lookups. Dense output
indexes the row directly, and sparse output formats the A1 row suffix
once per row. A Node.js 25.9.0 benchmark of 100,000 numeric cells used
nine alternating measured pairs after three warm-up pairs. Every pair
produced identical output. Median milliseconds before and after were
41.260 → 39.852 for dense JSON, 9.886 → 8.808 for dense CSV,
59.301 → 56.504 for sparse JSON, and 24.233 → 24.348 for sparse CSV.

SheetJS JSON output and its stream adapter use generated settings
for range selection and UTC handling. The public JSON options also
expose the existing dateNF behavior. A dense fixture covers numeric
range selection with hidden rows and columns, and a date-header
fixture covers dateNF formatting.

The SheetJS CSV and TXT file contracts and CSV projection share a
single list of delimited output fields. The direct and stream CSV
exporters pass one generated settings object to the row iterator,
including delimiter, row separator and date formatting options.

SheetJS HTML file writes and HTML projection share the generated
option fields. HTML direct, stream and workbook serialization project
non-finite numbers as error cells without mutating the worksheet.
Eight CE 0.20.3 comparisons covered dense and sparse cells, editable
output, and both HTML format names.

SheetJS HTML output scans only merges active on the current row.
Overlapping ranges retain their original worksheet priority. In
Node.js 25.9.0, a 1,000-row, eight-column worksheet with 1,000
horizontal merges exported in 23.695 → 9.067 ms median across
nine alternating measured pairs. All HTML matched byte-for-byte.

The HTML row iterator now reuses column labels and each row suffix
for cell addresses and HTML IDs. Nine alternating measured pairs on
Node.js 25.9.0 used 65,536 string cells starting at AA3. All outputs
matched. Median times were 74.163 → 72.059 ms for dense input,
96.225 → 88.831 ms for sparse input, and 73.179 → 68.936 ms for
dense input with one horizontal merge per populated row.

SheetJS formula-list export enumerates populated addresses for large,
sparse data-property sheets and sorts them into worksheet order. The
existing grid walk handles dense and small sheets, custom prototypes,
and worksheet cell accessors. On Node.js 25.9.0, exporting 100 formulas
within a declared 256,000-cell range took 21.725 → 0.070 ms median
over nine alternating measured pairs. Outputs matched both the
previous adapter and SheetJS CE 0.20.3 for that workload.

Sparse formula enumeration also preserves live grid traversal when
cell fields are accessors or require object-to-string conversion.
Three CE 0.20.3 comparisons covered a worksheet getter, a formula
getter, and a conversion hook that adds a later cell.

After adding the live-traversal checks, the same 256,000-cell workload
measured 20.982 → 0.102 ms median in nine alternating pairs on Node.js
25.9.0. All outputs matched the original grid walk. A further 128
comparisons with CE 0.20.3 covered offset ranges, non-enumerable cells,
mixed values, and both values-option settings.

SheetJS sheet_new uses generated options and array-formula assignment
shares the generated coordinate bounds used by XLSX saves. Numeric
cell lookups in dense sheets use the row and column directly. Setting
one array formula spanning 100,000 dense cells measured 22.663 →
12.598 ms median over nine alternating measured pairs on Node.js
25.9.0. The complete worksheet models matched in every pair.

SheetJS hyperlink utilities omit an empty tooltip, and cell_add_comment
initializes a falsy comment list before appending. Forty-two comparisons
with CE 0.20.3 covered link removal, internal links, optional tooltips,
comment list initialization, and author defaults.

SheetJS sheet_get_cell matches the CE 0.20.3 stub behavior for falsy
slots and uses column zero for falsy numeric-column arguments. A
72-case differential check covered dense and sparse worksheets and
string, address object, and numeric overloads.

SheetJS A1 parsing scans UTF-16 code units and decodes a single-cell
range once while returning independent endpoints. Comparison with
CE 0.20.3 covered all 16,384 encoded column labels and 55 parse cases,
including absolute coordinates, oversized rows and non-ASCII input.

SheetJS and ExcelJS worksheet naming share generator predicates for
disallowed punctuation and leading or trailing apostrophes. Each
adapter retains its own name handling, error messages, and precedence.

SheetJS book_append_sheet allocates a name set only for automatic
naming or duplicate rollover. Explicit names check the live SheetNames
array. In Node.js 25.9.0, appending 5,000 explicitly named sheets took
494.561 → 208.128 ms median across five alternating measured pairs.
Workbook models matched, and a separate CE 0.20.3 comparison covered
automatic names, suffix gaps, zero-padded suffixes, and numeric names.

Automatic SheetN allocation reads the ordered prefix directly and
uses a name set at the first gap or out-of-order entry. The next name
still reflects edits to the live SheetNames array. In Node.js 25.9.0,
adding 4,000 automatically named sheets measured 661.878 → 154.261 ms
median across five alternating pairs. Workbook models matched, and
five naming fixtures matched SheetJS CE 0.20.3.

ExcelJS worksheet name truncation stops before a surrogate pair at
the 31-unit boundary and rejects unpaired surrogates in the retained
name. This keeps the JavaScript worksheet identity equal to the
name sent through Wasm and serialized in XLSX. The compatibility
adapter deliberately differs from ExcelJS 4.4.0's raw UTF-16 slicing
at that boundary. Duplicate checks normalize the candidate once.

Both adapters generate the same well-formed Unicode worksheet-name
predicate. SheetJS applies it during draft XLSX serialization so a
lone surrogate cannot silently become a replacement character in Wasm.
A 31-unit name ending with a complete supplementary character
round-trips unchanged.

ExcelJS defined-name addition, model replacement, and cell membership
share generated name validation. Invalid characters, the reserved
prefix, and names exceeding the length limit are rejected before
editing the workbook.

ExcelJS named references also validate the worksheet component with
the generated Unicode predicate before formatting the range. Rejected
references do not enter the pending workbook model or change the
serialized bytes of an existing workbook.

ExcelJS print-area and print-title options share generated string
validation before their respective range parsers. An invalid type
leaves the existing print ranges and serialized workbook unchanged.

ExcelJS autofilter endpoint fields and one-based worksheet coordinate
limits are declared in the DSL. Object endpoints share generated
coordinate checks with other worksheet operations. An out-of-range
endpoint leaves the existing autofilter and workbook bytes unchanged.

ExcelJS margin updates now use generated validation for both the
outer record and the existing per-field indices and values. Tests
cover null, arrays, and numbers in addition to invalid margin values,
and confirm rejected edits preserve the workbook bytes.

ExcelJS generated header and footer validation rejects lone surrogates
before writing through Wasm. Valid supplementary characters round-trip,
and rejected text preserves the existing workbook bytes.

Generated SheetJS legacy comment and ExcelJS note validation rejects
lone surrogates before Wasm converts them to replacement characters.
Reverting a rejected SheetJS model edit, or retrying an ExcelJS save
after rejection, preserves the original workbook bytes.

Generated cell hyperlink validation rejects lone surrogates in SheetJS
targets and tooltips and in ExcelJS targets, labels, and tooltips.
Rejection preserves the source XLSX bytes.

ExcelJS generated rich-text run and font validation reject lone
surrogates in run text and font names. Browser tests verify the
refused assignments leave the workbook bytes unchanged.

ExcelJS image links share the generated Unicode string checks used
for cell links. Invalid targets and tooltips are rejected before
editing the drawing, with byte-identical output after refusal.

ExcelJS validation messages and formula operands reject lone surrogates
using generated predicates. Numeric operands and date-validation dates
retain their existing paths. The regression checks that refused
validation updates preserve the workbook bytes.

ExcelJS nonblank formula contracts now reject lone surrogates in
conditional thresholds, fillFormula, and custom table totals.
Browser coverage exercises fillFormula refusal and verifies that
invalid conditional expressions preserve the workbook bytes.

ExcelJS table and column names, display names, and staged totals text
use generated Unicode validation. Rejected table names and column
names leave the workbook bytes unchanged.

ExcelJS number-format contracts cover the cell, row, column, and
default-style paths. They preserve each path's empty-string and
omission behavior and reject lone surrogates before writing to Wasm.
Rejected assignments preserve the original workbook bytes.

SheetJS draft and loaded XLSX saves use a generated Unicode predicate
after resolving number format IDs. A refused change to a loaded
workbook leaves the source bytes intact after restoring the format.

ExcelJS core-property strings and workbook revisions now use generated
validation, covering both pending properties and loaded workbooks.
Rejected Unicode input preserves the serialized bytes.

Generated descriptor projections now consume file-specific options
for SheetJS reads and sync and async writes. These projections preserve
the descriptors of the remaining format options. An overridden
read type is ignored without invoking its getter, and accessors on a
required sync filename or an async book type are rejected.

SheetJS parse_zip shares the generated byte-read option projection
with readFile. Overridden type accessors are ignored and non-enumerable
read options are forwarded to the format parser.

SheetJS parse_zip validates the names it will serialize before
reading their bodies. Lone surrogates are rejected at the adapter
boundary. The nested workbook path still reads only its selected
entry and ignores unrelated entries.

SheetJS binary-string input retains the low byte of every UTF-16
code unit. ZIP bodies containing a surrogate pair that encodes two
bytes now decode through both read and parse_zip. Three interior ZIP
offsets matched CE 0.20.3.

Binary strings and decoded Base64 share the preallocated byte
conversion. A Node.js 25.9.0 end-to-end read of a 969,766-byte
uncompressed XLSX with 32,768 numeric cells took 139.749 → 60.252 ms
median in nine alternating measured pairs after three warm-up pairs.
Dense worksheet projections matched in every pair. The timed path
was binary-string input. Base64 checks covered standard and URL-safe
alphabets, whitespace, and malformed input.

The same workbook encoded as 1,293,024 Base64 characters read in
141.129 → 61.133 ms median on Node.js 25.9.0 over nine alternating
measured pairs after three warm-up pairs. Worksheet outputs matched
for every pair. The contract suite now retains explicit coverage for
both alphabets, whitespace, and malformed input.

SheetJS Base64 output uses Uint8Array.toBase64 when present and falls
back to binary-string encoding otherwise. For an unchanged workbook
loaded with bookSheets from a 969,766-byte XLSX, Node.js 25.9.0 measured
37.943 → 0.727 ms median and 50.736 → 0.856 ms p95 across 30 alternating
pairs after six warm-up pairs. The native and fallback output strings
were identical in every pair.

Base64 reads use Uint8Array.fromBase64 when available and retain
atob plus preallocated byte conversion as the fallback. Both paths
normalize URL-safe characters first. On Node.js 25.9.0, decoding
969,766 bytes alone took 2.279 → 0.166 ms median in 30 alternating
pairs. End-to-end reading took 174.558 → 166.520 ms median in a
separate 30-pair run, with p95 of 182.970 and 183.913 ms. Worksheet
outputs matched across both paths.

The current tarball passed the webpack distribution suite in Chromium
158.0.8127.18 and Firefox 146.0.1, including both adapters together,
the deployment layout, and the pinned upstream reference probes.
Strict TypeScript imports and Node ESM package checks passed.
The combined bundle contained one shared 7,457,190-byte Wasm asset.

The all-features browser characterization suite now asserts the
supported rich text, formula, error, and date projections while
retaining edit-refusal coverage for guarded array cells. Chromium
and Firefox passed the suite and ZIP part locality checks. Both
caught the deliberate mutation that publishes a candidate before
serialization succeeds.

The current all-features staging comparison ran 12 alternating pairs
per fixture in fresh browser documents. The median time for ten scalar
setters, byte-snapshot to Rust-held staging, was 5.15 → 4.75 ms
(scalar) and 84.2 → 84.7 ms (sparse) in Chromium 158.0.8127.18,
and 42.0 → 43.5 ms (scalar) and 845 → 857 ms (sparse) in Firefox
146.0.1. The fixtures were 4,580 and 26,165 bytes. Sparse Wasm
capacity fell from 3,670,016 to 3,604,480 bytes in both engines.
The load phase includes adapter import only on the baseline path,
so the load timings do not isolate a staging improvement. The
results support keeping the existing byte-snapshot staging.

Separated the scalar update phases in the distribution profiler.
On the 26,165-byte sparse workbook, five measured trials after two
warm-up trials each performed ten updates. Chromium median times
were 68.8 ms reopening the snapshot, 0.1 ms clearing links, 2.9 ms
writing the scalar, and 9.1 ms serializing. Firefox measured 750,
0 (timer resolution), 28, and 60 ms respectively. Reopening the
snapshot dominates this workload. The distribution and fixed-version
reference checks passed in both engines.

A native release-build probe split snapshot loading at
Package::from_bytes and Workbook::from_package. With shared input
bytes and 30 measured opens after ten warm-ups, median times for
the 4,580-byte scalar fixture were 37.621 and 98.593 microseconds.
For the 26,165-byte sparse fixture they were 44.241 microseconds
and 4.357 ms. The workbook phase covers the formula census,
shared-string loading and first-sheet XML and indexing. This
measurement isolates the two public Rust loading phases. It does
not attribute their internal costs or extrapolate to browser Wasm.

ExcelJS CSV reads and writes share generated projections for the
destination sheet, parser options, formatter options, and Node file
encoding. The projections preserve inherited values and getter order.
The browser parser and Node fast-csv integration keep their existing
parser and formatter option handling. The Node file encoding defaults
to UTF-8 for omitted and null values.

An instrumented native release build measured the internal workbook
open phases separately. In 30 measured opens after ten warm-ups, the
median formula census took 30.90 microseconds on the scalar fixture
and 2.849 ms on the sparse fixture. First-sheet XML parsing took
27.59 microseconds and 1.350 ms, and cell indexing took 2.90 and
303.64 microseconds respectively. The sparse fixture spent the most
time in the formula census, followed by first-sheet XML parsing.

ExcelJS printer settings derive the worksheet field order and getter
defaults from one declaration. The generated write validator continues
to pass only supplied settings to Rust. An omitted first-page number
remains an enumerable worksheet property with an undefined value.

The streaming formula census now keeps static names for the six
SpreadsheetML tags it interprets. Unknown elements still occupy a
stack frame so direct-parent checks retain their meaning. In an
instrumented native release build, two runs per version with 30
measured opens each after ten warm-ups gave pooled median census
times of 26.07 → 21.94 microseconds on the 4,580-byte scalar fixture
and 3.540 → 2.474 ms on the 26,165-byte sparse fixture. Median
workbook-open times for the two sparse runs were 5.242 and 4.995 ms
before and 4.188 and 4.147 ms after. This probe measured native Rust.

The fresh all-features Wasm build passed the browser smoke and
characterization suites in Chromium 158.0.8127.18 and Firefox
146.0.1. Against the prior all-features artifact, median constructor
time over 30 alternating pairs after eight warm-up pairs on the sparse
fixture was 7.2 → 6.6 ms in Chromium and 78 → 74.5 ms in Firefox.
The scalar fixture measured 0.3 → 0.2 ms and 2 → 2 ms respectively.
This browser comparison used the prior test artifact as the baseline.

SheetJS XLSX and SpreadsheetML writers share generated print-margin
defaults. XLSX uses retained source margins first for missing or null
values. SpreadsheetML keeps omitted header and footer elements absent
and uses defaults for undefined page margins.

Repeating the Wasm constructor probe while validating the margin
change gave 6.8 → 6.4 ms in Chromium and 72.5 → 73 ms in Firefox
on the sparse fixture. The two runs establish a repeatable
improvement in Chromium under these conditions. Firefox results
varied across runs.

ExcelJS shares the print-margin declaration with SheetJS. The
generated ExcelJS field order drives its margin index and the
fallback values when pageMargins is ignored. Each adapter includes
the generated values in its own module.

The SpreadsheetML stream uses a generated stride contract. Omitted
and null stride values batch ten rows. The generator checks explicit
values on the first stream read and accepts positive safe integers.

SheetJS text writers and file downloads use one generated format table.
It supplies the supported text format keys and their download MIME
types, including the UTF-16LE designation for TXT.

SheetJS also generates the XLSX, XLSM and XLAM format and download
MIME table. Buffered writes use its format keys and file output uses
the matching MIME value.

SheetJS XLSB, ODS, FODS and SpreadsheetML downloads take their MIME
types from the generated file-format declarations. XML and XLML
share the same SpreadsheetML MIME type.

SheetJS accepts BIFF8 XLA add-in output through the buffered writer
and file APIs. XLA shares the compound BIFF8 serializer and writes
its empty Addin record immediately after the workbook BOF.

The SheetJS binary-output declaration drives the BIFF version lookup,
format-specific options, and binary download MIME types. The grouped
BIFF version tables supply the format keys for the option variants
automatically.

SheetJS output format aliases are generated alongside filename
extension mappings. Explicit HTM selects the HTML writer. The
asynchronous filesystem API also accepts the existing SocialCalc
alias for EtherCalc output.

The SheetJS type declarations expose the explicit SocialCalc alias
on writeFileAsync and the BIFF8 XLA option on buffered and file output.
The import fixture verifies the endpoint-specific option types.

A fresh tarball with the all-features Wasm build passed the webpack
distribution and pinned-reference probes in Chromium and Firefox.
The combined bundle shared one 7,534,530-byte Wasm asset. Node ESM
filesystem checks passed and the TypeScript import fixture passed with
strict checking of the fixture and skipLibCheck for dependencies.
The XLA comparison uses a separate workbook per implementation
because the reference writer can add properties to the input workbook.

The release-default Wasm tarball also passed the Node ESM filesystem
checks and the complete webpack distribution and pinned-reference
checks in Chromium and Firefox. The combined bundle shared one
7,457,637-byte Wasm asset.

ExcelJS definedNames.getRanges accepts the optional caller-provided
cell matrix. Matrix enumeration defines rectangle discovery order,
and ranges use the ExcelJS 4.4.0 sheet-name formatting. This query
leaves workbook-defined names intact and updates marks on the supplied
matrix cells as the reference does.

ExcelJS generates the list and value kinds of workbook core
properties from one ordered declaration. The adapter uses each
declared kind for pending values and loaded workbook properties.

Workbook date properties share a generated Date validator. It checks
the Date built-in's stored time even when the instance overrides
getTime. Rejected assignments leave loaded workbook bytes intact.

ExcelJS worksheet creation and worksheet state assignments use a
shared generated visibility predicate. Both paths retain their
existing error codes and reject invalid edits before changing bytes.

The worksheet visibility declaration connects ExcelJS state strings
with SheetJS numeric codes. The generator derives both validators and
the SheetJS public utility constants from that declaration.

SheetJS uses the reverse visibility mapping generated from the same
declaration when applying updates through the Rust workbook API.
A cross-adapter test creates a very-hidden sheet in ExcelJS, changes
it through SheetJS, and reads the hidden state back through ExcelJS.

SheetJS generates the document-property wire names, date classification,
and XLSB-derived property allowlist from declarations. The writer retains
own-data validation and forwards normalized values to Rust.

The adapters now derive core-property date fields from one declaration
and generate their Date checks from the same time predicate. SheetJS
retains its separate scalar path for document-property values that are
not Date objects.

SheetJS generates BIFF property classification for dates, heading
lists, and derived fields. The BIFF8 filesystem suite verifies the
timestamp and heading names round trip and that the heading count
is regenerated from the list.

SheetJS XLSX and XLSB writers share generated property override
rules. Explicit Props cannot override company or manager and
provide creation and modification dates only when absent.

ExcelJS generates page-layout fallback values from the pageSetup
contract. Ignoring pageSetup preserves fitToPage from sheetPr; ignoring
sheetPr gives fitToPage its declared default. Worksheet creation still
infers fitToPage from fit dimensions when scale is omitted.

The ExcelJS pageSetup declaration also generates the creation-only
fitToPage inference. Positive fit dimensions enable it when scale is
omitted and fitToPage is unspecified. Later edits keep explicit
patch semantics and apply only supplied values.

ExcelJS printer comment options derive their accepted spellings and
OOXML values from one declaration. The legacy asDisplyed spelling
continues to serialize as asDisplayed.

The generated ExcelJS printer projection builds the ordered string
pairs passed to Rust, including spelling aliases, boolean values and
the first-page-number flag. Explicit useFirstPageNumber wins over
inference, and an inferred flag is appended after supplied fields.

ExcelJS generates the numeric page-setup and print-option arrays
from the same field lists as the ordered option contract. Explicit
false and zero are preserved, and absent array entries retain the
sentinel required by each Rust method.

ExcelJS headerFooter settings generate the ordered string values and
change mask for Rust from the same field declaration. A null string
sets its mask bit and clears the existing text. An omitted value
keeps its bit clear, and false flags remain explicit updates.

The cumulative printer and header/footer changes passed the Chromium
and Firefox smoke and characterization suites. The browser print
round trip covers a single patch containing a null header and an
explicit false flag alongside an untouched odd header. Both browsers
also passed the ZIP part locality and dependent-cache checks.

ExcelJS margin updates generate the indexed patch from the declared
margin field order. It reads own data descriptors, preserves omitted
edges, and skips the Rust setter when the resulting values match.

SheetJS margin updates generate the Rust array from the validated
properties, retained margins and declared defaults. Both omitted and
null fields use the retained value when present, and a no-op patch
returns no update.

ExcelJS cells expose a read-only model snapshot for the supported cell
values, merged cells, styles and plain notes. The value fields matched
the 4.4.0 reference fixture. Styles come from the existing cell style
projection, which can include an inherited default font where the
reference uses an empty style in a newly created workbook. Model
assignment and live mutation of the returned object remain pending.

ExcelJS rows expose a read-only model snapshot built from recorded
cell positions and row layout. The projection keeps holes sparse,
includes already instantiated blank cells, and returns null for
rows without height or cells. Sparse row values and layout match
the reference fixture; styling uses the existing adapter projection.

Rust exposes worksheet-order enumeration of standard validation rules
with their OOXML range lists. It shares attribute and formula decoding
with the existing single-rule queries and omits invalid references.
The data is available for the ExcelJS worksheet model projection.

ExcelJS dataValidations.model reads standard rules through one Wasm
enumeration. For live range additions it retains the range keys.
Reopened workbooks expand range lists to cell keys and reuse the
decoded rule for each covered cell, as ExcelJS 4.4.0 does. The
projection is a read-only snapshot of standard validations and does
not reserialize the workbook.

ExcelJS worksheet.model exposes a read-only snapshot of row and cell
models, layout and formatting metadata, validation rules, tables,
images, compressed columns and merges. The row, merge, column and
validation fixture agrees with the ExcelJS 4.4.0 model. Style and
other metadata values use the existing Rust-backed projections.

ExcelJS workbook.model exposes a read-only snapshot of worksheet
models, properties, views, defined names, themes and image media.
Loaded theme XML uses the original ZIP entries. The getter reuses
worksheet snapshots across its two worksheet lists. The reference
fixture matches the top-level keys, creator, worksheet names, defined
names and theme XML. Model assignment remains pending.

ExcelJS cell.model assignment sends the supported value, replacement
style and optional plain note through a single candidate commit.
Existing merge membership and the target cell address must agree
with the assigned model. Invalid formulas leave serialized bytes
intact. Live mutation of a returned model remains pending.

Rust and Wasm expose clear_row_height for model replacement. The
operation removes only the explicit height and custom-height attributes
on an existing row. An absent row stays absent and the serialized
workbook remains byte-identical.

ExcelJS row.model assignment atomically replaces row layout and
supported cell models. Later addresses may be inferred from the
previous cell. Omitted, unmerged cells are cleared, with source XML
stubs retained when Rust preserves the cell node. Existing merge
membership is preserved. Invalid candidate edits leave bytes intact.

Row model replacement now prunes the omitted cell elements that
become empty and unstyled. Rust keeps elements with children or
additional attributes and updates the live cell index after each
removal. The sparse-row fixture retains only the supplied cell keys
after saving and reopening.

ExcelJS dataValidations.model assignment replaces standard validation
rules through a single candidate commit. The reference fixture
matches the live range keys and reloaded cell keys. An overlapping
replacement leaves the original bytes intact.

ExcelJS 4.4.0 omits x14 validation rules from dataValidations.model
in the mixed-format fixture. Replacing the standard rules preserves
the x14 extension XML verbatim. A replacement overlapping the
retained x14 range is refused and leaves the workbook byte-identical.

ExcelJS worksheet.model accepts an explicit input shape for replacing
rows while updating print settings, headers, views, filters, visibility
and standard validation rules. Row model preparation and application
are shared with row.model assignment. A rejected row or validation
rule rolls back the entire candidate. Sheet identity, merges, columns,
media and tables cannot be assigned through this input shape yet.

Worksheet model assignment supports replacing merges through either
merges or ExcelJS's import key mergeCells. It unmerges removed ranges
before writing rows and adds new ranges without copying cell styles.
A conflicting merge rolls back the rows and all preceding merge edits.

The worksheet column model projects loaded column ranges from Rust
layout records, without requiring JavaScript column handles to have
been initialized. The reference fixture matches compressed ranges
and their width and visibility, and ignoreNodes cols suppresses the
model projection without rewriting the workbook.

Worksheet model assignment accepts column ranges. The Wasm adapter
sets width, visibility, outline and default style over each range,
rather than addressing every column in JavaScript. Existing known
column settings are cleared before applying the replacement ranges.
A subsequent rejected row edit rolls back the column changes too.

Worksheet model assignment accepts the same supported worksheet
properties as creation. Default dimensions, descent, outline levels
and directions, and tab color apply through the same candidate.
The ExcelJS adapter shares the validation and Rust write paths with worksheet
creation.

Worksheet model assignment supports renaming the target sheet within
the same candidate as row and metadata edits. It shares name validation
with the name setter, updates dependent formulas through the Rust
rename operation, and publishes the new lookup key after commit.
Existing worksheet and retained cell handles remain usable.

Worksheet model assignment accepts conditionalFormattings through the
generated input contract. It replaces the visible blocks with the same
rule encoder and Rust writes used by addConditionalFormatting. An empty
array clears the blocks. A rejected rule rolls back the worksheet model
edit, including rows and any previous conditional formatting.

Worksheet model assignment accepts sheetProtection with stored hash
metadata and permission flags. Rust writes the stored hash without
recomputing it and shares the worksheet protection update with protect.
An explicit null clears protection. Invalid XML values reject the
candidate before any row or protection edits are published.

Worksheet model assignment accepts the projected rowBreaks and dimensions
fields. As in ExcelJS 4.4.0, the target keeps its existing page breaks
and derives dimensions from the assigned rows.

Worksheet media replacement has a Rust operation for removing the
background picture. It removes the sheet relationship when unused and
deletes the image part only after its final incoming relationship is
removed. Removal refuses unresolved background links before publishing
an edit.

Worksheet model assignment accepts media with workbook image IDs,
backgrounds, and one-cell or two-cell picture anchors. It replaces
projected media through the same Rust placement writes as addImage
and addBackgroundImage. An empty array clears projected media.
Image relationships, row edits, and placement IDs are published
together after the candidate is accepted. Picture shapes outside the
native editable set can cause replacement to be refused.

Worksheet model assignment accepts table definitions. The candidate
removes replaced table relationships, applies the assigned worksheet
rows, and attaches table metadata through Rust. The metadata path
preserves the supplied worksheet cell values instead of generating
table header, total, and calculated cells. Existing definitions with
matching options keep their OPC parts. A refused table edit rolls
back the worksheet candidate.
The table attachment path writes explicit header and filter-button
flags for ExcelJS readers and retains totals labels from the model
even when the totals row is hidden.
Explicit totalsRowFunction none is retained independently of a totals
label. A missing function still leaves the attribute absent.

Workbook model assignment stages a separate document. Rust inserts
sheets at their requested positions with explicit workbook IDs before
the worksheet models are applied. Core properties, calculation options,
views, named ranges, a single theme, and buffered media are written on
the candidate. The destination publishes its worksheet handles and
media registry only after the candidate is accepted.
Node workbook models also retain filename-backed images. The model
assignment and addImage share deferred image registration. File bytes
are read at save time, after the candidate model has been accepted.

Workbook model assembly edits its unpublished Rust document directly.
Publication serializes it once and transfers the document to the
destination. A Node regression test counts serializations across
multiple sheets with media and named ranges. For numeric models with
60 rows and 8 columns per sheet, five runs after two warmups gave
median assignment times of 29.6 to 27.5 ms for one sheet, 227.4 to
183.6 ms for eight sheets, and 820.1 to 751.0 ms for 24 sheets.

Fresh worksheets in a workbook model batch plain cell values across
rows using the existing Rust Excel scalar write. The row layout is
applied separately. Models with styled cells, notes, or complex values
keep the general cell model update. On the same numeric fixture the
median assignment time after batching was 7.6 ms for one sheet,
44.2 ms for eight sheets, and 195.1 ms for 24 sheets.

Mixed worksheet models batch each consecutive run of plain values.
The candidate flushes before writing a styled cell, note or formula.
Once a range formula can have materialized cells ahead of the writer,
subsequent values use the general path. With six ordinary formulas
per 480 cells, median assignment times before and after this change
were 25.0 to 10.9 ms for one sheet, 190.4 to 54.2 ms for eight
sheets, and 774.2 to 264.3 ms for 24 sheets.

For fresh rows with ordinary values, Rust cell insertion creates the
row and its default layout. The adapter skips the redundant row
layout calls in that case and retains explicit layout and empty rows.
The numeric benchmark with 24 sheets moved from 195.1 to 185.8 ms
median, within the observed run-to-run spread.

When assigning a projected conditional formatting rule, the adapter
uses its resolved differential style. Rust binds that style to the
destination workbook's differential format registry. A source
differential format ID without its style is refused.

Worksheet model assignment accepts loaded text, blank and error
conditional predicates, including the stored formula on the rule.
Rust validates the supplied formula before writing it and supplies
the default formula when omitted. The adapter accepts the projected
notContainsText, beginsWith and endsWith rule names.
The model also preserves omitted text and operator attributes when
the stored predicate consists of a formula alone.

The adapter writes duplicateValues and uniqueValues through the
native Rust rules. Worksheet and workbook model assignment preserve
the loaded kinds and resolve their differential styles in the
destination. ExcelJS 4.4.0 reads the rule kinds and omits them when
serializing a workbook.

Average rules accept equalAverage and the nonnegative stdDev count.
The Rust rule keeps explicit false and zero attributes distinct from
omitted settings. The same fields are projected for model assignment.

If worksheet model assignment receives the sheet's existing projected
conditional formats, it retains the original conditional formatting
nodes. A row-only edit can therefore carry the full model while
preserving extension-only icon rules and their original XML.
Extended threshold formulas are projected as text, including formulas
that start with a number. Numeric thresholds keep their numeric
representation. This preserves formulas that ExcelJS 4.4.0 coerces
when it projects an extended icon rule.

Standard icon thresholds accept a per-threshold gte flag. Rust
writes explicit false and true values, and model projection retains
the distinction between either value and an omitted flag.

Workbook and worksheet model assignment serialize standalone x14
3Stars, 3Triangles, and 5Boxes icon sets through Rust. The writer
retains the source rule GUID, priority, range, formula thresholds and
inclusion flags. It stages the worksheet XML so invalid input or
duplicate rule IDs leave the package unchanged.

Standalone x14 icon models preserve custom icon selections. Rust
checks the number of selections and each icon index against its set
before creating the worksheet extension. Explicit per-threshold icon
IDs survive model copy without being reassigned by position.

addConditionalFormatting creates extended icon rules for extension-only
sets and custom icon selections. Rust generates rule GUIDs and assigns
priorities above the explicit priorities in the same batch.

Extended data-bar projection retains the positive fill color from
x14 independently of the standard fallback color. It also exposes
an explicitly stored x14 showValue flag when the standard rule does
not supply the flag.

Model assignment can copy a linked x14 data bar with a standard
fallback and ARGB colors. Rust attaches both representations using
the same rule ID. It preserves border and gradient settings, negative
color options, axis, direction, ranges and thresholds represented in
the model.

addConditionalFormatting also creates linked data bars directly when
extended options are supplied. Rust generates the shared GUID and the
standard rule retains the assigned priority.
Extended data-bar color overrides use the existing Rust color type and
support RGB, theme with tint, indexed, and automatic colors. This
applies to the x14 fill, border, negative and axis colors.
If the standard and x14 thresholds differ, models expose the x14
values as extendedCfvo. Both threshold pairs survive copy. The x14
pair can contain automatic endpoints or formula text.

Model assignment copies x14-only data bars without synthesizing a
standard fallback. The standalone rule retains its priority, explicit
visibility flag, extended thresholds, and colors.

The standard data-bar fill also uses the typed Rust color path.
Data-bar models can retain RGB, tinted theme, indexed and automatic
fill colors independently from any x14 override.
Color scales use the same typed workbook colors at each stop. Model
copy preserves theme colors with tints, indexed colors, and automatic
colors in the same order as their thresholds.

ExcelJS note projection reuses Rust rich-text run and font decoding.
Cell.note and cell.model.comment retain the runs of a formatted or
multi-run legacy note, and one plain run still projects as a string.
Legacy notes also accept formatted runs on assignment and model copy.
Rust uses the shared rich-text writer and retains the author and VML
shape when replacing an existing note.
Note-only coordinates are included in worksheet models even when
the source has no cell value record. Model copy retains those notes.
Worksheet presence includes note-only coordinates in the existing
numeric record stream. Rust decodes the note coordinates without
copying note text or authors into the Wasm response.
In a Node fixture with 120 notes of 7,000 characters each, the
first row lookup transferred no note text after this change, down
from 840,000 characters. The median of five runs after two warmups
went from 7.7 to 2.2 ms.

Legacy-note models read and write the margin, protection, and
cell attachment settings of the associated VML shape. The ExcelJS
option contracts validate the settings, and Rust stages the VML and
note text changes together. Updating an existing note retains the
anchor, author, and other shapes. The plain-text getter returns a
string, while the cell model carries its box settings for copying.

The note option contracts use named fields with their validation,
error messages, and compatibility defaults in one declaration.
The generator emits the same validators and exports the defaults for
projection and writing. Validation preserves omissions so text-only
edits can retain the current VML settings.

Conditional formatting reuses the cell font option contract for
vertical alignment, theme scheme, family, and charset. Rust writes
these as differential overrides. An omitted font name or size
continues to leave the underlying cell font property alone.
The shared font contract declares validation and defaults beside each
field. The cell encoder applies the generated defaults for full fonts.
The same encoder passes omitted fields through for differential fonts.

The ExcelJS table adapter supports tables with headers and zero data
rows. Rust uses the same range rules for creation and removal. The
first insert expands the table and filter, and the final removal
leaves its headers and any totals row in place. The adapter clears
cells removed from the table, including cells that held moved totals.
Column insertion and removal also work with zero data rows. Rust
checks the header and totals footprint and accepts an empty column
value list when the table has no data rows.

Worksheet table models resolve header, data, and totals differential
format references at table and column level using the same projection
as conditional formatting. Model assignment registers the resolved
styles in the destination workbook and uses the resulting IDs in the
table part. A reference with no resolved style is refused.
The same path carries the table, header, and totals border references
on the table part. Column models use only the differential format
references available on table columns.

For differential-format lookup, the release Wasm benchmark used a
synthetic XLSX containing 25,000 differential styles. On Node 25.9.0
the median of nine measured batches, after three warmup batches, was
475 ms before and 11.5 ms after for 400 requests for IDs 0–2. For the
last three IDs, the medians were 543 ms and 517 ms. The benchmark
loads the workbook once per binary and measures the lookup and Wasm
result conversion, with equal results verified before timing.

The packed distribution was installed into a fresh Node project and
passed the public entry-point integration and TypeScript checks.
The Chromium 158 and Firefox 146 distribution suites also passed using
the installed package, including the standalone and combined Webpack
entries and comparison against ExcelJS 4.4.0 and SheetJS CE 0.20.3.
The combined bundle contained one Wasm asset.

On Firefox 146, the installed distribution took 902 ms for ten
ExcelJS scalar assignments on the 26,165-byte sparse fixture. A
separate Wasm phase benchmark used the same fixture and measured
five batches of ten writes after two warmups. The phase medians were
773 ms for reopening the package, 0 ms at timer resolution for
hyperlink removal, 30 ms for the scalar write, and 61 ms for
serialization. The scalar setter currently opens a candidate from the
last serialized bytes before executing the write.

The ExcelJS cell value setter now forks the opened Rust workbook.
The fork shares the original ZIP bytes, copies parsed and staged
state, and allocates its own package identity for registry plans.
The setter publishes the candidate only after the edit and save
succeed. On Firefox 146.0.1, the median for ten assignments on the
same sparse fixture fell from 857 ms to 101 ms across five measured
batches, following two warmups. The benchmark verified the last
value in memory and after saving and reopening.
Loaded-workbook edits and worksheet creation now use the same fork
path. On the same Firefox fixture, ten font assignments fell from
1,175 ms to 127 ms at the median across five measured batches after
two warmups. The final font size matched before and after reopening.
Shared-string conversion and style-free export now fork the package
before rewriting it. On Firefox 146.0.1, exporting the sparse fixture
with both style removal and shared strings enabled took a median of
157 ms, down from 179 ms, across five measured runs following two
warmups. The benchmark checked the exported value, removed font
style, and unchanged source bytes.
Combined style removal and shared-string conversion also reuse the
existing workbook's sheet list without constructing a second workbook
from the rewritten package. In a paired rerun of the same Firefox
benchmark, this reduced the median from 166 ms to 113 ms.

### Node ExcelJS model container

The Node entry point exports `ModelContainer` with the original model
reference and a cached `xlsx` accessor. Reads replace the model after a
successful load and resolve to the container. Writes use the current
model through the workbook model adapter and Rust XLSX codec. The
supported model shapes and XLSX options follow the document adapter.

### Streaming XLSX write primitives

The Node writer stages hyperlink display text with its row and retains
only the link metadata until workbook commit. Rust writes the worksheet
locations and external relationships after all sheets have been added.
A link can target a sheet added after the source sheet was committed.
An unresolved worksheet reference rejects workbook commit before output
publication. The integration test checks the worksheet and relationship
XML, then reads the result through both adapter readers. ExcelJS 4.4.0
document mode returned the external link's display text and URL and
returned plain display text for the internal location. Its external
hyperlink value omitted the tooltip.

The Node writer supports workbook image registration and worksheet
backgrounds using the existing Node image input contract and Rust OPC
writer. Filename-backed images resolve during workbook commit, before
output creation. The streaming archive retains the generated media and
relationships when replacing the worksheet's row data. Both adapter
and ExcelJS 4.4.0 document readers recovered the images and streamed
cell values from the same archive. Registered images are serialized when
used as worksheet backgrounds.

Streaming worksheets expose protection and removal before sheet commit.
They use the document worksheet's generated permission contract and
Rust protection writer. This includes the 10,000,000 spin-count limit.
ExcelJS 4.4.0 read back the flags and verified the stored SHA-512 hash
with the persisted salt and spin count. A sheet unprotected before
commit has no serialized protection.

Streaming worksheets also add and remove conditional formatting before
sheet commit, including after row commit. Removal accepts an index, a
predicate that keeps matching blocks, or no argument to clear all
blocks. The document worksheet's generated validation and Rust writer
handle the rules and differential styles. ExcelJS 4.4.0 read the
retained expression, differential font and staged cell value.

A streaming worksheet also exposes its auto-filter range. Callers can
update or clear it after rows are staged and before sheet commit. The
document worksheet validates the endpoints and Rust saves the range.
ExcelJS 4.4.0 read the updated range and both staged rows.

Rust encodes sparse worksheet rows as independent SpreadsheetML
fragments. The Wasm ZIP writer emits local entries and data descriptors
incrementally. The host drains compressed bytes between writes, and
finalization emits the central directory. The Node integration test
drains a stored worksheet separately, appends deflated workbook parts,
and reloads the resulting workbook. This verifies the codec path.
The Node `stream.xlsx.WorkbookWriter` stages committed scalar rows in
per-sheet temporary files and releases the row objects. At workbook
commit, it reads the staged XML in chunks and drives the ZIP stream
through Node's backpressure-aware pipeline. A supplied writable stream
or filename uses this path. The optionless output keeps bytes in memory
for callers reading `writer.stream`. Cell inputs include strings,
numbers, booleans, blanks, dates, Excel errors, and ordinary formulas with
optional cached results. Dates use the template workbook's registered
date format and numeric Excel serials. A sheet's dimension is computed as
rows commit and written with the staged XML so dimension-based readers
see the full range. The writer reuses document-mode column layout for
width, visibility and outline. Header rows enter the same streaming
row lifecycle as data rows. Keyed objects write through configured
column keys and omit unrelated keys. Row commits preserve height,
visibility and outline, including formatted rows without cells. Those
rows are emitted independently of the worksheet's cell range.
The writer also preserves shared formula masters, ranges, indexes and
member caches. Members require an earlier master in the same sheet and
must fall within its declared range. Once the writer commits the last
row of a group, it releases that group's metadata.
Streaming reads and writes preserve array formula anchors, ranges and
cached values. The writer rejects overlapping arrays and merges and
rejects formulas in array result cells. It releases array-range metadata
once the rows covered by that range have been committed.
With `useStyles: true`, cell fonts, fills, borders, alignment,
protection and number formats are registered in the Rust workbook
without creating template cells. Streamed rows reference those style
indexes, including on otherwise empty cells. Identical formats reuse
the same index. Dates receive a default number format when the cell
does not explicitly supply one.
Row and column styles use the same format groups. Each committed cell
resolves each group from the cell, row and column in that order.
The writer stores row style references even for empty rows and writes
column styles on the worksheet's column definitions.
Shared strings can be enabled for the workbook or a single worksheet,
including partway through a worksheet. Rust interns plain and rich
strings across sheets and emits the shared table in batches. Rich
runs reuse the document-mode validation and XML renderer, so a
shared item keeps its font and remains distinct from plain text or
runs with different fonts. Formula string
results remain per-cell caches. With 100,000 rows containing one
number and two copies of the same long string and with ZIP compression
disabled, inline and shared output took 2.46 s and 1.87 s and used
91.5 MB and 12.1 MB on disk. Peak RSS was 173.2 MiB and 117.2 MiB.
The registry retains distinct strings until commit, so this result
does not characterize workloads where most strings are unique.
Merge ranges use the same normalization and Rust validation as
document mode. Streaming members resolve to their master before
commit. Only the master value is written, and the merge record stays
in the worksheet template. A merge or a write through a member is
refused once the master row has been committed.

In a stored-ZIP run with three cells per row and a Writable that discarded
each chunk on the next event-loop turn, 10,000 / 100,000 / 500,000 rows
produced 5.2 / 52.2 / 263.8 MB of output, with peak process RSS of
100.6 / 138.1 / 208.9 MiB and elapsed times of 0.31 / 2.08 / 9.27 s.
RSS includes Node and Wasm. This run establishes that output is not
retained in full; peak process RSS still increased with row count.

The writer now batches XML in one 64 KiB buffer shared across worksheets.
It rolls back a partial spool append before the row is retried. Stored
ZIP entries forward each original row chunk after Rust updates the ZIP
offset and CRC, without returning a second copy in the ZIP drain buffer.
In separate-process runs of the same uncompressed workload at
100,000 and 500,000 rows, elapsed time changed from 2.22 / 10.18 s to
1.14 / 5.28 s. Output sizes were unchanged. Peak RSS changed from
145.5 / 213.8 MiB to 124.0 / 221.7 MiB. The 500,000-row run still
showed RSS growth. A separate run sampled 154.1 MiB just before
ZIP output and reached a 219.3 MiB peak for the process.

Row inputs now retain raw values and only allocate cell objects when
the API accesses a cell. In another 500,000-row run, Wasm linear memory
stayed at 2.3 MiB and Node ArrayBuffer memory reached 66.4 MiB during
ZIP output. The final row allocation change took 5.27 s with 218.1 MiB
peak RSS in a separate run. Increasing file read chunks from 64 KiB to
256 KiB or 1 MiB did not lower the observed RSS, so the writer keeps
64 KiB chunks.

### Streaming XLSX read primitives

The Wasm ZIP reader takes a seekable callback and reads the ZIP central
directory in Rust. It opens stored and deflated worksheets with bounded
source reads and checks each entry's decompressed size and CRC at EOF.
A namespace-aware Rust XML parser returns one sparse row at a time,
with raw shared-string and style indexes and formula payloads. Memory
retained by the parser depends on the largest row and XML event.
The Node integration test exercises both compression methods, including
a filesystem callback, and deliberately corrupts a stored worksheet to
check error reporting.

The Node `stream.xlsx.WorkbookReader` resolves sheets in workbook order
from OPC relationships and exposes row events and nested async iteration.
Rust resolves the package and workbook relationships using the existing
OPC reader, and reads shared text and cell number-format references
through schema views. The Node adapter maps these values to ExcelJS
objects and handles the reader lifecycle. Files use positional reads. Node Readable inputs are staged to a temporary
file so the ZIP directory can be read without retaining the archive in
memory. The reader supports the shared-string cache, emit and ignore
modes, entry notifications, and number formats and fonts in styles
cache mode. Rust reads the shared font table and its style references.
The adapter projects the cached number formats, fonts, fills, borders,
alignment and protection onto styled cells, rows and columns.
The shared font, fill and border registries are decoded once.
In shared-string emit mode we publish the documented event, where the
4.4.0 implementation emits an unwrapped record from its parser.
Shared and inline rich text retain runs and the font fields supported
by the Rust document-mode font decoder. Inline runs are decoded while
streaming their containing row. Unsupported run font markup keeps the
text without a projected font.

The worksheet parser also projects column intervals before the first
row. The adapter exposes column width, visibility, outline, collapse
and number format, supports column keys, and expands the observed
worksheet dimensions as rows are read. ExcelJS 4.4.0 agrees on row
dimensions in the tested sparse worksheet. Its streaming reader leaves
the tested column width, visibility and outline at their defaults.
Here the streaming reader exposes the values stored in the XML.
Row height, visibility, outline, collapse and the row number-format
reference now travel with the corresponding row from Rust. The row
height and number format were checked with a file authored by ExcelJS
4.4.0 in document mode.

Hyperlink emit mode reports worksheet references and OPC hyperlink
relationships. Cache mode scans the worksheet for links before it emits
rows, then applies external targets or internal locations to existing
cells. The prepass keeps hyperlink records and does not materialize
rows. On a 100,000-row stored archive without hyperlinks, ignore and
cache mode took 1.37 s and 2.23 s respectively, with peak RSS of
99.5 MiB and 99.7 MiB. The event payloads were compared against
ExcelJS 4.4.0 using an external hyperlink.

In separate Node processes with shared strings ignored and a stored ZIP
containing three cells per row, 10,000 / 100,000 / 500,000 rows took
0.17 / 1.43 / 6.68 s to read. Peak RSS was 91.5 / 99.4 / 122.3 MiB
for 5.2 / 52.2 / 263.8 MB input. The run retained no rows and did
not measure Readable staging or shared-string cache memory.

### ExcelJS row splice with notes and formula ranges

The regular adapter reads note metadata through the Rust row projection.
It stages note removal, note creation, and cell writes in a single document
transaction. Input validation remains attached to the original cell address.
Ordinary formulas retain explicitly supplied cached values, including zero,
false, and the empty string. Node integration covers failure after note
removal, retry, and XLSX round trips.

The streaming adapter can move or remove a complete one-row shared or array
formula range before encoding. It updates the range coordinates and the
shared formula members' master addresses. It refuses a partial edit when the
formula range crosses the splice boundary, including ranges anchored in an
earlier pending or committed row. The formula text and cached results move
with the cells.

A row without object values skips the scan for pending formula anchors.
On Node 25.9.0, with 200 pending numeric rows of 2,000 cells before the
edited row, 40 alternating pairs measured 11.532 ms and 0.773 ms median with
and without the scan. Each pair produced the same row values. The timing
covers the insertion only.

The regular adapter still refuses a splice through a shared or array formula
range. Its scalar edit API validates each cell against the existing range.
Supporting a move requires planning the row-local coordinate change and
updating the formula census together. The Rust whole-column editing API
also updates other rows and their references, so it has different semantics.

### ExcelJS workbooks with zero sheets

ExcelJS 4.4.0 writes workbooks whose workbook part omits `<sheets>`.
The ExcelJS load entry point accepts zero sheets, including that encoding
when the workbook has no sheet relationships. It rejects a workbook that
omits the sheet list but retains sheet relationships. The streaming
manifest applies the same rule. The first sheet added to a loaded empty
workbook creates the missing sheet list and gets ID 1.

A new ExcelJS workbook uses a Rust zero-sheet template for serialization.
The template removes the initial worksheet and its relationship in a
package copy and updates the extended properties. The zero-sheet document
uses the ordinary style, shared-string, and ZIP export options. Both the
standard Rust loader and the deletion guard for the last visible
worksheet retain their existing contracts.

Node integration covers standard and streaming output, metadata,
and adding the first sheet after loading. A separate comparison opens
empty files from both ExcelJS 4.4.0 writers, adds a sheet, and opens the
result in ExcelJS 4.4.0. Chromium and Firefox also run the standard
empty-workbook round trip.

A document row splice can move a complete shared-formula group contained
in that row. The adapter snapshots the cached results and styles. Rust
verifies the group membership on the edit candidate and clears the
entire group in one edit plan. The adapter sends the relocated master,
actual member columns and cached results to a second Rust write plan.
Independent cells inside the declared rectangle remain independent.
The workbook publishes only a successful candidate.

A document row splice can also move or replace a complete one-row
static array. Rust checks the exact rectangle and clears its stored
cells in one edit plan. The adapter then writes relocated cached values,
styles and the array anchor on the same candidate. Dynamic arrays and
partial row edits retain the existing refusal behavior.

Document row splicing transfers only affected note coordinates to
JavaScript for grid-boundary validation. Rust reads the author, rich
text and supported note-box settings and moves notes on the same edit
candidate as the cell values.
