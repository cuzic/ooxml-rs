# ADR-0021. Browser XLSX APIs compatible with SheetJS CE and ExcelJS

- Status: **Proposed** (revision 4)
- Date: 2026-11-23
- Repository baseline: `f88d02c`
- Depends on [ADR-0001](0001-lossless-round-trip-architecture.md), [ADR-0003](0003-xlsx-phase1-formula-and-calc-state.md), [ADR-0006](0006-xlsx-phase2-shared-strings-and-styles.md), and [ADR-0009](0009-xlsx-phase3-sheet-operations.md).

## Revision history

Revision 2 addresses the [round 1 review](0021-adversarial-review-round1.md). B1 specifies staging and fault recovery. M1 restricts edits to the original range. M2 defines projections and conservative edit gates. M3 specifies handle identity. M4 defines installation and import-only conformance. M5 specifies scalar validation. m1 records distribution and attribution.

Revision 3 allows Rust-side changes where needed for reliable compatibility and evaluates a Rust transaction path against byte-snapshot staging.

Revision 4 requires independently tree-shakable JavaScript entry points and production bundle verification.

## Context

Browser applications already use two distinct XLSX interfaces. SheetJS CE exposes synchronous functions and mutable workbook and cell objects. ExcelJS exposes a workbook class with live worksheet and cell handles and Promise-based XLSX I/O. Keeping familiar call sites can reduce migration work while giving applications the round-trip preservation of ooxml-rs. The initial use case is a browser application editing values in an existing workbook.

The preceding Wasm spike built `ooxml-xlsx` at the baseline for `wasm32-unknown-unknown` with Rust 1.99 and wasm-bindgen 0.2.130. In headless Chromium it fetched the repository fixture `reference/fixtures/xlsx/poi/57523.xlsx`, wrote `42817` to B20, serialized to changed bytes, and reopened them to verify the value. The same path passed in Node.js 18. This establishes the byte I/O path. Initialization as a distributed package, compatibility behavior, multi-sheet editing, and workbook creation still require validation.

## Upstream comparison

The conformance references are SheetJS CE 0.20.3 and ExcelJS v4.4.0. The SheetJS documentation is public and rolling. Pin its 0.20.3 release for behavior tests. ExcelJS source references below are versioned.

| Operation | SheetJS CE | ExcelJS v4.4.0 |
| --- | --- | --- |
| Load | Synchronous `read(bytes, opts)` returns a workbook | `await wb.xlsx.load(bytes)` populates an existing workbook and returns it |
| Select worksheet | Ordered `wb.SheetNames` and name-keyed `wb.Sheets` | `wb.worksheets` and `wb.getWorksheet(nameOrId)`. Number means ID, independent of position. Name comparison is exact. |
| Read and edit | Sparse `ws.A1` is a mutable object with `t`, `v`, optional `f` and presentation fields. Dense mode has a different shape. | `ws.getCell('A1').value` is a property. Formula values can be objects with formula and result fields. An empty cell has a null value. |
| Save | Synchronous `write(wb, { bookType: 'xlsx', type: 'array' })` | `await wb.xlsx.writeBuffer()` |
| Create and transform | `book_new`, `book_append_sheet`, `aoa_to_sheet`, `sheet_add_aoa`, `sheet_to_json` | `new Workbook()`, `addWorksheet`, row and cell APIs |

Sources for SheetJS are the [0.20.3 release instructions](https://docs.sheetjs.com/docs/getting-started/installation/standalone/), [read options](https://docs.sheetjs.com/docs/api/parse-options/), [write options](https://docs.sheetjs.com/docs/api/write-options/), [workbook model](https://docs.sheetjs.com/docs/csf/book/), [worksheet model](https://docs.sheetjs.com/docs/csf/sheet/), [cell model](https://docs.sheetjs.com/docs/csf/cell/), and [array utilities](https://docs.sheetjs.com/docs/api/utilities/array/). Sources for ExcelJS are [workbook.js, worksheet lookup and iteration](https://github.com/exceljs/exceljs/blob/v4.4.0/lib/doc/workbook.js#L100-L123), [index.d.ts, XLSX interfaces](https://github.com/exceljs/exceljs/blob/v4.4.0/index.d.ts#L1397-L1428), and the [release README](https://github.com/exceljs/exceljs/blob/v4.4.0/README.md).

## Decision

Provide two opt-in browser ESM entry points with TypeScript declarations over one Rust/Wasm XLSX binding. Both keep the original OOXML package as the basis of serialization. Rust owns parsing, edit guards, formula-cache policy, and serialization. JS owns API adaptation, supported-option validation, and value projection. Loading official libraries at runtime is unnecessary. For covered workflows, the migration target is an import or constructor reference change only. Keep existing method calls, property assignments, options, and await placement intact. ExcelJS callers can import the replacement under their existing `Workbook` or `ExcelJS` name. SheetJS has a function namespace, so keep the `XLSX` alias and replace its module specifier. An application that uses an uncovered method or option needs additional compatibility coverage before it can migrate this way.

### First milestone

Both entry points accept ordinary `ArrayBuffer` and `Uint8Array` inputs. A view uses exactly its `byteOffset` and `byteLength`. Output is a JS-owned copy so later edits or Wasm memory growth cannot invalidate it. SheetJS array output is an `ArrayBuffer`. Pin the ExcelJS browser output representation with an upstream v4.4.0 probe before finalizing the type declaration.

| Surface | Initial contract |
| --- | --- |
| SheetJS | `read(bytes)` and `read(bytes, { type: 'array' })`, `SheetNames`, `Sheets[name]`, sparse A1 cells with scalar `t` and `v`, `!ref`, and `write(wb, { bookType: 'xlsx', type: 'array' })`. Support changing `v` on an existing number or string cell and assigning a complete `{t,v}` number or string cell at an existing or new A1 address. New cells must be inside the original `!ref`. Changing `!ref` is outside this milestone. |
| ExcelJS | `new Workbook()` creates an empty JS holder for `await wb.xlsx.load(bytes)`. Support `worksheets`, `getWorksheet(name)` with exact matching and `undefined` for an absent name, `getCell(A1).value` and `await wb.xlsx.writeBuffer()`. Simple number, string and boolean assignments apply immediately through Rust. Worksheet IDs, numeric and no-argument lookup are follow-up candidates. |

The first import-only profiles are the two namespace ESM imports and the exact call sequences below. They cover ordinary XLSX files with addressable worksheets and the listed scalar cell operations. The SheetJS profile covers consumers of `t` and `v`. Consumers of its default `w` field need additional compatibility coverage. The option matrix permits omission of read options, SheetJS read `type: 'array'`, the displayed SheetJS write options, and omitted ExcelJS load and write options. Empty read and ExcelJS option objects are equivalent to omission. Other explicit options are rejected. Broader profiles require conformance fixtures before release claims.

Example migrated call sites. Only the import source changes. The package and export names are provisional.

```js
import * as XLSX from '@ooxml-rs/xlsx/sheetjs';

const book = XLSX.read(input);
const sheet = book.Sheets[book.SheetNames[0]];
sheet.B20 = { t: 'n', v: 42817 }; // B20 is already inside this file's !ref.
const output = XLSX.write(book, { bookType: 'xlsx', type: 'array' });
```

```js
import * as ExcelJS from '@ooxml-rs/xlsx/exceljs';

const book = new ExcelJS.Workbook();
await book.xlsx.load(input);
book.getWorksheet('Data').getCell('B20').value = 42817;
const output = await book.xlsx.writeBuffer();
```

For the browser compatibility entry points, test automatic Wasm initialization during ESM evaluation, using top-level await where supported. Share initialization per JS realm. This keeps SheetJS calls synchronous and avoids an extra initialization call in migrated code. Measure browser and bundler support for this entry point. An explicit initialization entry point can serve other environments and direct Wasm users. ExcelJS I/O continues to return Promises. Worksheet loaders run on the caller's thread. A worker can host an entire adapter instance through application-level messaging. Measure whether larger browser documents warrant worker guidance or an API extension.

### Cell and range projection

| Source state | SheetJS read | ExcelJS read | Edit |
| --- | --- | --- | --- |
| Ordinary scalar | Own enumerable `{t,v}` with matching number, string or boolean | Primitive `.value` | Supported scalar assignments within the respective API contract |
| Absent cell or recognized valueless ordinary cell | No own key, lookup yields `undefined` | `.value === null` | Allowed if the address is eligible for insertion |
| Known unsupported cell or occupied coordinate | Own enumerable accessor with contextual error on value read. `Object.keys` includes the key and spread throws. Assignment and deletion are rejected or caught before save. | Contextual error on value read | Reject |
| Incomplete classification | Accessing the affected sheet projection raises a contextual error | Cell access on the affected sheet raises a contextual error | Reject edits on the affected sheet |

Classify from Rust cell, style, shared-string and worksheet metadata before projecting values. A scalar-looking cache under a formula, a merge participant, a rich shared or inline string, an error, an explicit date or an ExcelJS date-formatted numeric cell belongs in the known-unsupported category. SheetJS's ordinary numeric date serials can be projected as numbers under the supported read defaults once verified. Include covered coordinates without a local cell or formula node when a merge, array or spill span declares them. Keep these spans as ranges for edit checks. Sparse enumeration lists stored cells, with virtual covered coordinates guarded at access and edit time without allocating a cell for every coordinate. A plain object may represent the supported subset, with an accessor or proxy where the access contract requires it. If metadata or enumeration cannot establish a complete edit classification, apply the incomplete-classification row. Read-only/no-op save remains available without materializing the affected sheet. Rust workbook-wide guards still apply.

For SheetJS, use a valid original worksheet `<dimension>` as `!ref`. Exclude original cells outside it from the JS projection and preserve them in the package. Missing or invalid ranges make that worksheet ineligible for edits in this milestone. Preserve the original range, including blank margins, and accept insertion only within it. Reject deletion or modification of `!ref` before applying edits. The browser spike must check missing and undersized ranges against the reference release and document any projection differences. Non-worksheet entries are preserved in the package. An attempted grid access produces an unsupported-sheet error.

Validate the JS input before crossing the Wasm boundary. Accept primitive finite numbers, booleans where enabled, and well-formed UTF-16 strings. Normalize signed zero to zero and test the saved representation. Reject isolated surrogates, non-finite numbers, coercions, mismatched SheetJS `t` and `v`, and coordinates outside XLSX bounds. Preserve empty strings and valid supplementary characters. Initial scope excludes formatting, dense SheetJS, detached worksheet objects, node file and stream APIs, other formats, and unlisted public methods.

### Mutation and ownership

- **ExcelJS.** Keep a committed byte snapshot and an associated Rust read view. Each setter stages from the last committed bytes, performs one Rust edit, and serializes the candidate. Publish the candidate bytes and read view only after both succeed. Discard candidates on preflight refusal, apply failure including poison, or serialization failure. Return an independent copy of the committed bytes from `writeBuffer()`. This makes the failure guarantee refer to the state just before the setter, including all previous successful edits. Cache worksheet handles by load generation and worksheet identity, and cell handles by worksheet and canonical coordinate. Return a fresh `worksheets` array with the same worksheet objects. Failed load retains the generation and handles. Successful load starts a new generation and old handles throw an explicit stale-handle error. Verify and document this reload behavior against upstream.
- **SheetJS.** Associate a private original byte snapshot and sparse worksheet baselines with a workbook returned by `read`. Use own-property-safe dictionaries for sheet names. Compare edited enumerable own data properties with the baseline without invoking unsupported-cell accessors or caller-supplied accessors. Preflight the whole change set and use a disposable workbook loaded from the original bytes for each edit-bearing `write`. Return bytes only after all edits and serialization succeed. A failed save discards the disposable workbook and leaves the caller's pending changes available for correction and retry. Each write recomputes the full change set. Reverting a value removes that edit. Track known unsupported cells and guarded ranges separately. Reject structural, range, removal, or unsupported field changes. An unchanged write returns a copy of the original bytes.
- Reuse Rust `Workbook::from_bytes`, worksheet and cell access, setters and `Workbook::to_bytes`. Group SheetJS number and string edits through `write_cells` per sheet. That batch lacks boolean writes at the baseline. Preserve Rust dependency invalidation and the default calculation policy. Map errors to documented codes with sheet and address where available. Promised I/O rejects and synchronous calls throw. For ExcelJS, use byte-snapshot staging as the reference implementation and measure it against Rust-side staging as described below.

### Rust changes and transaction choice

The compatibility layer may extend `ooxml-xlsx` where the available Rust API cannot express or safely classify an operation. Keep upstream JS calling conventions in the adapters. Put decisions that require OOXML structure or workbook-wide validation in Rust. Use the same classifiers and edit guards for native and Wasm entry points where the semantics match.

- Start with read-only worksheet and cell metadata for the supported projections. Check which information is available through existing APIs, including original dimensions, sheet kinds, raw cell and shared-string representation, number format, and merge and formula-range membership. Add accessors only for missing information. Reuse the existing formula, merge and style machinery. Represent incomplete classification explicitly so the binding can fail closed without reading worksheet XML a second time in JS.
- Compare staging from the last committed bytes with an isolated Rust candidate or a Rust transaction that can recover from an apply failure. The Rust option may avoid public serialization on each setter. To qualify, it must preserve the last successful state, including dirty parts, workbook indexes, shared strings and calculation state, and keep that state serializable after any rejected edit. A candidate must either validate its save path before publication or establish that publication introduces no new deferred save failure. Existing poison behavior in native callers cannot be treated as successful rollback.
- Use the smallest Rust change that meets the contract. Start in `ooxml-xlsx` and validate with fault injection and browser measurements before proposing a change to `ooxml-core`. Keep byte-snapshot staging if the Rust candidate cannot demonstrate the same failure and locality guarantees. Record any new public Rust API or behavior in the applicable ADR or implementation PR.

### Distribution and naming

Publish under an ooxml-rs package name and use SheetJS CE and ExcelJS names to identify the tested compatibility profiles. Provide independently authored sources, declarations, examples and tests. Use version-pinned upstream distributions as development references. Track and fulfill the applicable license and attribution terms for any redistributed upstream material. [SheetJS CE licensing](https://docs.sheetjs.com/docs/miscellany/license/) · [ExcelJS v4.4.0 license](https://github.com/exceljs/exceljs/blob/v4.4.0/LICENSE)

### Follow-up scope

1. **Creation and appending.** Add SheetJS `book_new`, `book_append_sheet`, and `aoa_to_sheet`, plus ExcelJS `addWorksheet` and basic row APIs. Validate zero-sheet JS state, first-sheet naming, duplicate names and order. Rust `Workbook::new` starts with `Sheet1` and calls `SystemTime::now()`. Verify the clock path on browser Wasm and design first-sheet creation against `rename_sheet` and `add_sheet` from ADR-0009 before exposing it.
2. **Data helpers and more value types.** Prioritize `sheet_to_json` raw array mode and `sheet_add_aoa`, including blank rows, array holes and range behavior. Agree on object headers, duplicate header disambiguation, `defval`, and formatting separately. Add clearing, boolean batches, date systems and number formats, errors, merge semantics and formula objects after fixtures establish parity. Formula cached results require an explicit decision under ADR-0003. Never invent cached results.
3. **Broader object API.** Evaluate numerical ExcelJS sheet IDs on fixtures with reordered sheets and gaps, stable row handles, style objects and other workbook metadata. Expand the published compatibility table as each contract is verified.

### Tree shaking

Expose `./exceljs` and `./sheetjs` as independent static ESM entries via package exports. Each entry imports only its own adapter and shared XLSX runtime. Avoid a runtime registry or a public entry that eagerly imports both adapters. Use static named exports and independently importable modules for optional helpers. Keep the namespace import used by existing callers. If a future compatibility object dynamically exposes all helpers, provide named or subpath exports for callers who want finer-grained elimination.

Keep automatic Wasm initialization on the compatibility import path so the migration contract still holds. Keep shared utility modules free of import-time work. Declare initialization side effects accurately in the published files and package metadata. Verify that production optimizers retain required initialization without bringing in the unused adapter. When both adapters are used in the same realm, share one Wasm asset and initialization. JS bundle elimination and Rust/Wasm size are distinct measurements. An unused JS export does not by itself narrow the shared Wasm artifact. [webpack tree shaking](https://webpack.js.org/guides/tree-shaking/) · [Rollup side-effect configuration](https://rollupjs.org/configuration-options/#treeshake-modulesideeffects)

## Alternatives and consequences

- An ExcelJS-only facade could ship sooner. It would leave SheetJS applications to migrate both worksheet representation and editing code.
- A common JS workbook model would simplify the binding. It would require both upstream calling styles to change, especially existing sparse-cell assignments.
- Rebuilding a file from an upstream-shaped model would support externally constructed objects early. It would discard parts and XML that the model cannot describe. Lossless save for loaded workbooks requires preserving the source package.
- A SheetJS proxy can expose immediate live edits with less snapshot storage. It makes nested property mutations, enumeration, and assignment of whole cell objects depend on proxy semantics and can reject edits earlier than SheetJS does. The proposed snapshot has simpler observable semantics and reloads the package on each save. Measure that cost before locking in the implementation.

## Acceptance criteria for implementation

1. Pin upstream fixture expectations and browser output types using the reference releases. Test offset byte views, sync return versus Promise, ExcelJS load return identity, and rejection behavior. Test worksheet order, exact names, same-generation object identity, and stale handles after reload.
2. Package the built ESM distribution as an installable tarball and install it in a separate browser test project. Run the paired workflows above with only the module specifier changed. Cover native browser ESM and a documented bundler, Chromium and Firefox, Wasm asset resolution under a non-root deployment path, TypeScript imports, and a missing Wasm asset causing an import rejection. State tested versions and applicable CSP requirements. Limit compatibility claims to passing environments and profiles.
3. Test sparse reads, guards and write/reopen with empty sheets, absent and undersized dimensions, original cells outside the declared range, and blank range margins. Verify an insertion within the original range and refusal of an attempted expansion. Cover absence versus unprojectable cells, own keys, spread, direct access and assignment, including formula results, merge and spill coordinates, dates, rich strings and incomplete scans. Round-trip empty strings and supplementary characters. Check rejection of lone surrogates, NaN, infinities and invalid coordinates, and check signed zero normalization.
4. Verify ADR-0001 T1/T1b no-op and T2 edit locality and ADR-0003 cache invalidation. Compare ExcelJS saved bytes before and after failed load, setter refusal, and injected apply and serialization failures following a successful edit. Inject apply failures after a registry edit and before the cell edit, and prove that subsequent edits still work. For SheetJS, stage a valid edit on one sheet and a rejected edit on another, then test both reverting to the byte-identical baseline and correcting the edit and retrying.
5. Verify a deliberate mutation bypassing failure isolation or dropping untouched data is caught. Measure Wasm size, initialization, load/save time, ExcelJS setter latency and peak memory for byte-snapshot staging and any Rust candidate, using small and representative sparse multi-sheet fixtures, including repeated edits on an already-dirty workbook. Record browser versions and fixture sizes. The preceding spike covers the Chromium numeric round trip only.

6. Build production bundles from the installed tarball with a version-pinned bundler. Use separate fixtures for ExcelJS only, SheetJS only, and both. Inspect the generated module graph and output chunks to verify that each single-adapter bundle excludes the other adapter and that the combined build shares the Wasm asset. When optional named helpers are published, show that an unused helper is eliminated in a targeted import fixture. Run each resulting bundle in a browser to verify initialization and the edit round trip. Report raw and compressed JS and Wasm sizes separately.

## Open questions

- Collect additional production call sites to prioritize profiles beyond the tested initial workflows.
- Confirm whether the first release should include new workbook creation or ship the existing-file workflow first. Proposed default is the existing-file workflow.
- Choose the public package name and entry point names. Proposed shape is a single ESM package with distinct `sheetjs` and `exceljs` subpaths and a common initialization export.
- Confirm whether SheetJS consumers need formatted values, object-mode `sheet_to_json`, or dense cells in their first release. Proposed default is the sparse edit workflow followed by raw array export.

## Implementation notes

The initial package uses validated byte-snapshot staging. The comparison
candidate compiled with the Wasm crate's `transaction-spike` feature keeps
the committed ZIP bytes and an isolated read view in Rust. It uses an
`Arc<[u8]>` to open each candidate and still validates serialization before
publishing it. Browser checks compare the saved output against the reference
path and inject apply and serialization failures. The comparison did not
show a consistent setter-latency improvement across both browsers. Keep the
baseline in the distribution. Measurements and reproduction steps are in
the [browser package README](../../packages/xlsx/README.md).

The Rust API adds `Workbook::worksheet_names`, `Worksheet::declared_range`,
`Worksheet::non_scalar_ranges` and `CellRef::has_non_scalar_content` to reuse
the worksheet and formula metadata for JS projections. Formula-census
uncertainty that Rust only identifies at workbook scope makes classification
fail closed at that scope. The no-op adapter save still preserves the input.

The ExcelJS adapter now exposes `Worksheet.id` and `Workbook.eachSheet`.
Numeric `getWorksheet` uses the declared OOXML ID and omitted lookup
selects the lowest ID. Enumeration remains in document order. The Rust
`Workbook::worksheet_identities` accessor returns names and optional
declared IDs without changing the handling of malformed IDs at load.

The SheetJS adapter also supports creating scalar workbooks with
`book_new`, `book_append_sheet`, and `write`. The Wasm path supplies
the browser's current time to the Rust `Workbook::new_at` constructor
and uses the existing template and sheet operations.

ExcelJS `addWorksheet` also uses the isolated package candidate.
`Workbook::add_sheet_with_id` permits an explicit nonzero ID only
when it is unused and unreserved. The default Rust allocator continues
to allocate above every reserved ID.

`CellWrite::Bool` extends the worksheet batch transaction to Boolean cells.
Both sparse and dense SheetJS projections can save Boolean changes through
the same guarded batch used by numbers and strings.

`bookProps: true` reads package core, extended and scalar custom properties
without materializing worksheet cells. Combining it with `bookSheets: true`
also returns sheet names. The metadata-only view has its own return type.

Fresh SheetJS workbook drafts can save string, finite numeric, boolean and
date values from `Custprops`. The Rust writer stages the custom property
part, content type and package relationship together.

XLSX saves support explicit ZIP compression. The package re-encodes entries
that use a different compression method while preserving their decompressed
contents and order. Saves with the option omitted use the existing
passthrough policy.

`ignoreEC` controls SpreadsheetML's `numberStoredAsText` ignored-error flag.
An explicit false value removes this flag while retaining unrelated flags.
Fresh drafts enable suppression by default. A loaded file preserves its
existing warning settings when the option is omitted.

Fresh drafts use inline strings unless `bookSST: true` requests shared
strings. An unused template table is pruned together with its OPC records.
Loaded workbooks retain their existing representation.

`themeXLSX` replaces the workbook's theme part with a validated DrawingML
theme document. Other parts are retained, apart from OPC records needed
to register a missing theme.

`cellDates: true` on XLSX writes serializes Date-valued cells with type
`d` and millisecond precision. Ordinary ISO date cells read as Date
objects or serials using the workbook's date system. The same encoding
applies to caller-supplied date results in a new array formula range.

Time-only ISO cells and cached results retain millisecond precision.
On read, a time without a date is anchored to the workbook's date epoch.

The ExcelJS adapter projects explicit ISO dates and time-only values as
Date objects, including ordinary formula results, and retains ISO storage
when editing an existing date cell. ExcelJS 4.4.0 can interpret an ISO
date's leading year as a serial value.

The ExcelJS adapter accepts caller-supplied results on ordinary formula
assignments. Numeric, Boolean, text and error results retain their XLSX
types. Date results use the workbook's serial date system and apply a
date format when the cell has no date format. Omitting the result clears
the cache. Explicit zero and false results round-trip through the adapter.

The ExcelJS adapter projects array formula anchors with `shareType: 'array'`,
the result range as `ref`, and any stored result. Array follower cells
expose stored scalar results or null for absent results. Array and spill
ranges retain their existing edit guards.

`Worksheet.fillFormula` supports explicit array mode for A1 ranges.
Caller-supplied results can be flat arrays, row arrays, or a callback
receiving one-based worksheet coordinates. Edits publish as one candidate.

The ExcelJS adapter projects shared formula masters with their range and
`shareType: 'shared'`. Followers expose the master address as `sharedFormula`
and the stored result. Their formula getter uses Rust's reference translation.

`Worksheet.fillFormula` defaults to shared mode. Rust allocates a
worksheet-local shared index, writes the master and members in one batch,
and tracks their references for cache invalidation. The batch can
replace an existing shared group when it covers every member. Partial
replacement still follows the existing shared-master hand-off rules.

`fillFormula` accepts the one-based `Location` shape listed in ExcelJS
types, including objects with the corresponding range accessors.
ExcelJS 4.4.0's runtime requires a string. Reversed endpoints are accepted
and stored in canonical top-left to bottom-right order.

`Cell.value` accepts an array formula object with `shareType: 'array'`
and an explicit A1 `ref` starting at the assigned cell. The write preserves
the existing scalar results of the range's other cells.

A `Cell.value` assignment with `sharedFormula` and an optional result
writes a follower within an existing, unambiguous master's declared range.

A `Cell.value` assignment with `shareType: 'shared'` and a `ref` creates
a master without assigning other cells. Followers can then be assigned
individually. Replacing an existing master still uses `fillFormula`.

The SheetJS entry exports `SSF` with a format registry, date-code
classification and date serial parsing. `SSF.format` uses the adapter's
existing number format coverage. Complex format rendering is tracked
separately in the API implementation plan.

Number format rendering includes English month and weekday names,
combined date and clock displays, and the built-in elapsed-hour format.

Numeric display also covers the built-in scientific and engineering
formats and grouped decimal formats with a parenthesized negative section.

Mixed fraction display supports the built-in variable denominators
and fixed denominators, including alignment spaces.

The built-in accounting formats preserve currency symbols where defined,
parenthesized negatives, aligned text and the zero-value dash.

The SheetJS formatter resolves the built-in numeric format aliases,
regional time format 56 and implicit currency formats 63–66.

Custom fixed-decimal formats can select sections by sign, value
condition, or text type, with quoted literals, escapes, and padding.

For custom numeric sections, unescaped semicolons inside quoted literals
split sections to match the pinned SheetJS release.

General formatting rounds numeric display to the reference width rules,
normalizes exponential notation, and renders date and nonfinite inputs.

Numeric formats support scaling commas and optional decimal digits,
including percent forms and quoted units.

SheetJS CSV, HTML, and JSON streaming accepts an injected Readable
constructor or factory and produces output on demand. CSV starts with a
BOM and JSON emits rows in object mode.

SpreadsheetML 2003 streaming writes scalar worksheet data in configurable
row batches, with number formats, merged cells and hyperlinks. Cell
references in ordinary and array formulas are converted to R1C1 while
quoted literals remain intact. Workbook metadata coverage includes the
date system and sheet visibility.

SpreadsheetML worksheet metadata includes column widths, row heights,
hidden rows and columns, and the AutoFilter range.

SpreadsheetML writes standard and custom document properties, accepts
standard property overrides, and emits workbook-scoped and sheet-scoped
defined names with their references converted to R1C1.

SpreadsheetML 2003 output is also available through `write` and browser
downloads with book types `xml` and `xlml`. String output carries Unicode
text; byte and binary forms carry UTF-8.

The browser recognizes SpreadsheetML 2003 text and byte input. Its reader
projects sparse or dense typed cells, number formats, and ordinary formulas.

SpreadsheetML input also projects array formula ranges, merged ranges,
row and column layout, autofilters, and hyperlinks.

SpreadsheetML input projects document properties, typed custom
properties, scoped names and sheet visibility. Either `bookProps` or
`bookSheets` reads workbook metadata without constructing cell models.

SpreadsheetML reads and writes plain cell comments and page margins.
Comments are XML-escaped, and explicit zero margins are preserved.

SpreadsheetML input resolves named and inherited number formats.
With `cellDates`, date-formatted numeric cells use the workbook date
system to project JavaScript Date values.

SpreadsheetML `sheetRows` preserves the declared row extent and full
cell width, exposing the complete range as `!fullref` when truncated.

SpreadsheetML models can be saved as new XLSX workbooks. The conversion
uses the XLSX writer's supported cells and metadata, and writes defined
names using their workbook or worksheet scope.

New SheetJS XLSX workbooks accept `Workbook.Names` for global and
sheet-scoped names, with optional comments. The same name can appear
in distinct scopes.

SheetJS `write` and browser downloads support DIF output, including
single-sheet selection, CRLF records, Unicode text and typed fields.

SheetJS recognizes DIF text and byte input. The DIF reader keeps Unicode
and recognizes grouped numeric values without splitting on their commas.

SheetJS `write` and browser downloads support SYLK output with scalar
cells, ordinary numeric formulas, number formats and row/column layout.

SheetJS recognizes SYLK text and byte input and projects scalar cells,
ordinary formulas, and explicit cell number formats. The reader supports
dense sheets, date projection, and the row limit.

SYLK input also recovers row and column layout, comments, sheet-local
names, and shared formulas expanded from their base records.

SYLK formatting records can inherit their row coordinate. A row-limited
read resolves shared formulas from base expressions outside the projection
without returning those cells.

SheetJS reads and writes EtherCalc (SocialCalc) ETH worksheets with typed
scalar values and formula results. Escape sequences decode in one pass
to preserve literal backslashes.

SheetJS `write` and browser downloads can export a worksheet as an RTF
table. Cell text escapes RTF syntax and writes Unicode control words.

RTF input recognizes worksheet tables in text and byte input, including
Unicode control words, escaped syntax, typed fields and empty columns.

SheetJS `write` and browser downloads can export FODS with multiple
worksheets, typed scalar cells and ordinary OpenFormula expressions.

The FODS reader accepts text and byte input in browsers. It projects
typed cells and ordinary OpenFormula expressions and interprets repeated
row and column records without allocating blank cells.

FODS reads and writes merged ranges with explicit anchor spans and
covered-cell records, preserving adjacent cell coordinates.

FODS array formula anchors carry matrix spans. Loaded cells in the span
expose `F` alongside any stored values, and `cellFormula: false` hides
the expression and span while retaining cell values.

FODS round-trips hyperlink targets and tooltips, and plain annotations
with author, line breaks and visibility.

ODS uses the existing Rust ZIP implementation with a first, stored
`mimetype` entry, `content.xml`, `styles.xml` and a manifest. The FODS reader and writer
handle shared table content. ODS output supports optional ZIP compression.

FODS and ODS support `bookSheets`, worksheet selection, and `sheetRows`
with `!fullref`. Worksheet selection follows the adapter's XLSX behavior;
SheetJS 0.20.3 reads every ODS/FODS worksheet for the same option.

ODS and FODS read and write workbook and sheet-scoped named ranges and
named expressions through OpenDocument named-expression records.

ODS and FODS map sheet visibility to OpenDocument table styles.
Very-hidden sheets map to ordinary hidden sheets because OpenDocument
has a single hidden state.

ODS reads and writes document metadata in `meta.xml`. FODS uses inline
metadata. Standard properties and typed custom entries are projected
with the workbook directory, including in `bookSheets` mode. ODS also
reads table styles from the separate styles part.

In document mode, ExcelJS `Row.commit()` returns without modifying
the workbook and the row remains editable. ExcelJS 4.4.0 does not
expose `Worksheet.commit()` on its document worksheet at runtime.

ExcelJS `Cell.isMergedTo(master)` tests whether a merged follower
refers to the exact cell object supplied as its master.

ExcelJS `Cell.toCsvString()` returns the hyperlink target for a link
cell. The display label remains available through `Cell.text`.

A nonpositive ExcelJS CSV maxRows leaves the browser reader
unlimited, matching the pinned Node parser. Positive limits count
returned rows after comment handling, ignoreEmpty, skipLines,
and skipRows.
