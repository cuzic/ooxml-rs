# ADR-0022 spike results

The prototype moved XLSB row and column metadata decoding and the row cutoff into Rust. Rust decompresses only the selected sheet and determines the record prefix. The public SheetJS adapter materializes rows and columns, decodes cells from the prefix and receives the existing tail metadata from the same projection. XLSB writes and the XLSX projection retain their previous paths.

The generator uses the entire existing XLSB allowance and runs after format detection. Adding `cellFormula` changed one declaration entry and a fixture, with zero additional adapter control flow. The generated mapper replaces six lines at the call site with one call and one import. The prototype adds roughly 35 authored lines for the generator and declaration table, so a single mapper does not establish a net source-size reduction. Deleting `cellFormula` in a scratch copy caused the generator's `--check` to fail as expected.

## Compatibility

The native XLSB tests and strict Wasm Clippy passed. Node ESM package smoke and the Chromium and Firefox distribution suites passed. The browser suites compare the pinned SheetJS CE 0.20.3 and cover sparse/dense shape, relative formulas, row cutoff, merges, filters, print margins and column-width units. Descriptor cases verify omitted and explicit undefined options, unknown keys, inherited keys and an accessor that must remain uncalled.

A standalone SheetJS 0.20.3 probe emitted both a hyperlink and a sheet-protection record after the cell table. It returned the hyperlink on a full read and omitted it with `sheetRows: 2` when a third row was present. It exposed no sheet-protection property in either case. This covers one fixture and leaves broader trailing-record policy open.

## Performance

Node v25.9.0. Each round used 36 interleaved warm samples per adapter and workload after six warm-up samples. Times are milliseconds. WASM capacity is linear-memory capacity, which includes allocator high-water history. The narrow operation was measured on generated files in one local process. Cold import was measured separately from worksheet reads, with concurrent imports, so it provides a diagnostic observation only.

| Workload | Run 1 median baseline → spike | Run 1 p95 baseline → spike | Run 2 median baseline → spike | Run 2 p95 baseline → spike |
| --- | ---: | ---: | ---: | ---: |
| Small, 20 × 8 | 0.313 → 0.395 | 0.466 → 0.599 | 0.319 → 0.370 | 0.574 → 0.499 |
| Sparse, 1,500 × 48 | 2.937 → 2.837 | 10.078 → 7.796 | 2.334 → 2.379 | 9.979 → 9.010 |
| Dense, 400 × 250 | 36.073 → 35.987 | 44.766 → 57.903 | 36.067 → 37.973 | 62.322 → 63.357 |
| Select small sheet from a two-sheet book | 0.962 → 0.912 | 25.283 → 2.335 | 0.688 → 0.805 | 1.568 → 1.577 |
| Names only for the two-sheet book | 0.547 → 0.558 | 3.815 → 2.814 | 0.571 → 0.572 | 2.670 → 2.537 |

The small-sheet median exceeded the +15% gate in both runs. The dense p95 exceeded the +25% gate in run 1, and the selected-sheet median missed the +15% gate in run 2. Dense peak Wasm capacity increased from 9,699,328 to 10,092,544 bytes (+4.1%) and passed the memory gate. The Wasm artifact grew from 6,713,539 to 6,718,101 bytes. Cold import observations were 121–129 ms for the baseline and 79–87 ms for the spike.

Worksheet prefix transfer is bounded by the original uncompressed stream sizes of 3,477, 108,777 and 1,810,857 bytes respectively. The fixture's layout adds one 12-byte row tuple and one 16-byte column tuple. The names-only call transfers no worksheet stream. No full uncompressed stream is sent back to Wasm. Rust still scans record headers before JavaScript traverses the prefix again for cells. The measurements combine that scan with the layout projection and Wasm adapter cost.

**Conclusion.** Rust can own this worksheet projection while preserving the exercised public contract. Generating the option boundary is feasible, with limited evidence for source reduction at this scale. Keep the ADR Proposed. For the next prototype, measure a single Rust pass that includes cells and batches projection output, then rerun the same gates.

## Reproduce

Run the following with unpacked baseline and prototype packages in Node's dependency tree. Keep the benchmark isolated from browser runs.

```sh
node spikes/adr-0022/measure.mjs /path/to/baseline-package /path/to/spike-package
node packages/xlsx/generate-sheetjs-options.mjs --check
cargo test -p ooxml-xlsx --lib xlsb::tests
```

## Follow-up: single-pass Rust cell and layout projection

Rust now decodes worksheet cell records, row and column metadata, and the tail in one traversal. It returns 16-byte cell entries in a batch, with a UTF-16 text pool and an optional formula payload batch. JavaScript materializes the public cells, formats displayed values and interprets formula tokens. It does not traverse the worksheet binary record stream. The workbook, stylesheet and shared-string components retain their previous paths. The original writer also remains in place.

An unreserved cell buffer initially raised dense peak capacity by about 26.4%. Reserving a bounded capacity once a projection reached 64 KiB reduced the final increase to 4.1%. The final Wasm artifact is 6,723,019 bytes, 9,480 bytes above the baseline. For the 100,000-cell fixture the cell batch is 1,600,000 bytes, compared with a 1,810,857-byte uncompressed worksheet stream. That fixture has no inline text or formula batch. There is no transfer of the raw worksheet stream between Wasm and JavaScript.

The final package passed the Chromium and Firefox reference suites, the four native XLSB tests, and strict Wasm Clippy. The packed cell test covers the high style bits, short cell coordinates and a signed RK numeric value. Both Node benchmark runs checked deep equality with the pre-spike adapter on each workload before measuring.

| Workload | Run 1 median baseline → single pass | Run 1 p95 baseline → single pass | Run 2 median baseline → single pass | Run 2 p95 baseline → single pass |
| --- | ---: | ---: | ---: | ---: |
| Small | 0.362 → 0.282 | 0.508 → 0.354 | 0.329 → 0.290 | 0.427 → 0.369 |
| Sparse | 2.896 → 1.578 | 4.931 → 7.682 | 2.936 → 1.667 | 5.125 → 6.976 |
| Dense | 36.949 → 17.044 | 72.977 → 37.160 | 37.909 → 17.045 | 45.732 → 35.380 |
| Selected sheet | 0.752 → 0.804 | 1.759 → 17.920 | 0.965 → 0.800 | 2.296 → 5.106 |
| Names only | 0.537 → 0.527 | 1.566 → 2.185 | 0.536 → 0.533 | 17.080 → 1.899 |

Small and dense meet all gates in both runs. Sparse and selected-sheet p95 miss the gate in both runs. Names-only p95 misses it in run 1 despite the unchanged path, illustrating the variability of short samples in this process. These measurements do not isolate the cause of the p95 regressions. The median gains justify further measurement, but the combined gate still fails. Keep the ADR Proposed.

## Isolated benchmark after the sheetRows fixes

Node v25.9.0. Each workload and adapter runs in a fresh process, with 20 warm-up and 200 timed reads. The two rounds reverse adapter order. Generated inputs are identical per pair and their projected results compare equal before timing. Reads include naturally occurring GC. The script reports a descriptive bootstrap interval for p95 and counts samples overlapping a V8 GC event. Initialization is recorded separately. The previous results above used the earlier interleaved harness.

| Workload | Round 1 median | Round 1 p95 | Round 2 median | Round 2 p95 | Wasm capacity, both adapters |
| --- | ---: | ---: | ---: | ---: | ---: |
| Small | 0.227 → 0.172 | 0.403 → 0.317 | 0.226 → 0.167 | 0.414 → 0.274 | 1.75 MiB |
| Sparse | 1.996 → 1.318 | 8.894 → 2.817 | 1.967 → 1.067 | 7.570 → 2.947 | 2.25 MiB |
| Dense (100,000 cells) | 36.740 → 19.715 | 48.980 → 40.333 | 36.346 → 19.017 | 49.039 → 39.790 | 9.00 MiB |
| Selected sheet | 0.590 → 0.535 | 1.460 → 1.449 | 0.565 → 0.544 | 1.443 → 1.679 | 6.94 MiB |
| Names only | 0.411 → 0.442 | 1.008 → 1.140 | 0.430 → 0.452 | 1.253 → 1.211 | 6.94 MiB |
| Inline text and formulas | 7.257 → 4.836 | 18.962 → 15.401 | 7.005 → 3.984 | 18.855 → 12.974 | 3.38 MiB |
| Shared strings, formulas and sheetRows: 200 | 3.121 → 1.755 | 7.774 → 4.529 | 2.926 → 1.405 | 7.022 → 2.949 | 3.25 MiB |

Timings are milliseconds, baseline → single-pass. All seven workloads met the original latency and capacity gates in both rounds. Dense peak capacity was 9 MiB for both adapters in the isolated process. In the two dense rounds, 139 and 140 baseline samples overlapped GC, compared with 77 and 79 single-pass samples. This is an association, and the benchmark does not isolate the source of the allocation difference.

The earlier sparse and selected-sheet p95 regressions did not recur under the isolated protocol. The selected-sheet p95 rose by 16% in round 2, within the +25% gate. Its bootstrap intervals overlap. The evidence covers these generated Node workloads and leaves the ADR review and broader input coverage open.

## Benchmark after XLSB metadata expansion

The pre-projection package was compared with the package that includes
the generated Rust-call settings, defined names, and hyperlink and
sheet-protection support. The benchmark uses Node v25.9.0 on an AMD EPYC
7313P. Each workload used 20 warm-ups and 200 timed reads per adapter
in fresh processes. The two rounds reverse adapter order. The preflight
compares worksheet results and checks the expected values of additional
book metadata and plain shared-string run information separately.

| Workload | Round 1 median | Round 1 p95 | Round 2 median | Round 2 p95 |
| --- | ---: | ---: | ---: | ---: |
| small | 0.230 → 0.209 | 0.368 → 0.329 | 0.233 → 0.201 | 0.435 → 0.465 |
| sparse | 2.054 → 0.977 | 6.931 → 3.262 | 1.923 → 1.340 | 7.176 → 4.497 |
| dense | 36.905 → 20.192 | 42.817 → 42.871 | 37.067 → 20.273 | 53.523 → 43.708 |
| selected | 0.619 → 0.546 | 1.719 → 1.386 | 0.558 → 0.592 | 1.642 → 1.832 |
| bookSheets | 0.459 → 0.450 | 1.391 → 1.396 | 0.467 → 0.546 | 1.522 → 1.604 |
| mixed-inline | 7.268 → 4.051 | 19.323 → 14.239 | 7.139 → 5.055 | 16.586 → 13.527 |
| mixed-sst-cutoff | 2.629 → 1.442 | 7.417 → 4.185 | 2.577 → 1.669 | 9.240 → 3.568 |

Timings are milliseconds, baseline → current. The Wasm module was
6,713,539 → 7,028,483 bytes. Final linear-memory capacity
increased by at most 7.1% across the workloads. The numerical
gates passed in 13 of 14 comparisons. Round 2 `bookSheets` missed the
median gate at +16.9%. Cold import ranged from 76.5–87.0 ms for the
baseline and 80.8–97.2 ms for the current package.
These timings cover generated sheets without hyperlinks or name
definitions. They measure the cumulative adapter change from the
pre-projection baseline. The ADR remains Proposed.

## Catalogue-only read follow-up

Both packages in this comparison include defined-name and hyperlink
support. The candidate skips formula metadata decoding in Rust for
`bookSheets` and `bookProps`. The fixtures contain one sheet and one
numeric cell. The second fixture adds 10,000 defined names with
`Data!$A$1` references and a 40-character comment each. The run checked
deep equality for the full read and the timed metadata reads. It used
the same Node worker protocol with two reversed-order rounds.

| Fixture and option | Round 1 median | Round 1 p95 | Round 2 median | Round 2 p95 |
| --- | ---: | ---: | ---: | ---: |
| small, `bookSheets` | 0.146 → 0.114 | 0.211 → 0.209 | 0.100 → 0.098 | 0.157 → 0.145 |
| names, `bookSheets` | 2.161 → 1.487 | 3.205 → 2.605 | 2.066 → 1.613 | 3.238 → 2.497 |
| names, `bookProps` | 2.312 → 1.621 | 3.286 → 2.370 | 2.273 → 1.573 | 3.282 → 2.325 |

Times are milliseconds, before → after. With 10,000 defined names,
final Wasm capacity fell from 10.13 MiB to 8.38 MiB in both rounds.
This comparison isolates the
catalogue shortcut. The previous pre-projection comparison used a
different input and remains the evidence for its original gate.

The catalogue comparison can be reproduced with the packages immediately
before and after the catalogue change.

```sh
node spikes/adr-0022/measure.mjs --catalogue /path/to/before /path/to/after
```

## Seven-workload gate rerun

The original pre-projection package and the current package with the
catalogue shortcut were compared using the same seven fixtures and
reversed-order isolated Node v25.9.0 protocol as above.

| Workload | Round 1 median | Round 1 p95 | Round 2 median | Round 2 p95 |
| --- | ---: | ---: | ---: | ---: |
| small | 0.231 → 0.193 | 0.350 → 0.325 | 0.291 → 0.215 | 0.490 → 0.315 |
| sparse | 1.887 → 1.272 | 6.836 → 3.570 | 1.784 → 1.140 | 6.219 → 4.385 |
| dense | 35.138 → 20.420 | 47.692 → 39.052 | 35.704 → 20.698 | 48.451 → 42.914 |
| selected | 0.898 → 0.613 | 2.352 → 1.709 | 0.622 → 0.608 | 1.805 → 1.566 |
| bookSheets | 0.486 → 0.463 | 1.389 → 1.319 | 0.459 → 0.464 | 1.609 → 1.440 |
| mixed-inline | 7.219 → 4.200 | 18.715 → 12.385 | 7.005 → 4.193 | 18.608 → 13.714 |
| mixed-sst-cutoff | 2.297 → 1.605 | 8.057 → 3.115 | 2.434 → 1.879 | 8.902 → 3.400 |

Times are milliseconds, baseline → current. 14
of 14 comparisons met the three numerical gates. The largest
final Wasm memory increase was 7.1%.
Wasm artifact size was 6,713,539 → 7,028,712 bytes.

## Shared formula write with existing hyperlinks

Run the benchmark with two unpacked builds of the npm package arranged
so their JavaScript dependencies resolve. The baseline should have
per-cell hyperlink removal within the Rust shared formula write and
the current build should have batched removal.

```sh
node spikes/adr-0022/measure-shared-formula-links.mjs /path/to/baseline /path/to/current
```

Each round uses a fresh Node v25.9.0 process, eight warm-up pairs and
36 alternating timed pairs. Every write starts from fresh bytes and
the timer covers the public `fillFormula` call on a 20 × 20 range.
There is one additional link outside the edited range in every fixture.
The script checks every result and hyperlink and compares the output
bytes across adapters for the first timed pair of each workload.

| Links inside range | Run 1 median (ms) | Run 1 p95 (ms) | Run 2 median (ms) | Run 2 p95 (ms) | Run 3 median (ms) | Run 3 p95 (ms) |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 0 | 4.52 → 4.25 | 6.85 → 5.69 | 4.99 → 4.41 | 6.11 → 6.37 | 4.68 → 4.28 | 6.29 → 6.73 |
| 40 | 4.40 → 4.07 | 4.92 → 7.66 | 4.42 → 4.12 | 6.05 → 5.41 | 4.47 → 4.16 | 6.16 → 5.23 |
| 400 | 16.48 → 7.19 | 20.27 → 11.30 | 14.00 → 7.37 | 21.17 → 14.63 | 15.99 → 7.18 | 19.11 → 11.33 |

The same script accepts `array` as a third argument. With the immediate
per-cell array-removal build as the baseline, the Node v25.9.0 array run
measured the following. The output bytes matched for all three fixtures.

| Links inside range | Median (ms) | p95 (ms) |
| ---: | ---: | ---: |
| 0 | 11.86 → 11.33 | 14.78 → 13.69 |
| 40 | 11.66 → 11.13 | 15.34 → 13.11 |
| 400 | 25.57 → 19.70 | 27.18 → 23.79 |

## Unmerge intersecting ranges

```sh
node spikes/adr-0022/measure-unmerge.mjs /path/to/baseline /path/to/current
```

Use unpacked npm package builds with their JavaScript dependencies
available. The baseline still filters the full merge list in
JavaScript and calls the exact-range setter for each match. The
current build filters and removes the matching ranges in Rust.

Three fresh Node v25.9.0 runs used eight warm-up pairs and 36 alternating
timed pairs each. The fixture has 300 two-cell merges in columns A:B
and one in D:E. Each operation starts from fresh bytes. Timing covers
only `unMergeCells`. The script checks all merge states and byte
equality across builds for the first timed pair of each workload. It
also checks that a selection without matches leaves the input bytes
unchanged.

| Merges removed | Median before → after (ms) | p95 before → after (ms) |
| ---: | ---: | ---: |
| 0 | 0.316–0.336 → 0.013–0.014 | 0.627–2.769 → 0.027–0.039 |
| 1 | 0.932–0.953 → 0.630–0.648 | 1.018–1.266 → 0.707–0.780 |
| 300 | 2.704–2.706 → 0.589–0.595 | 2.973–3.407 → 0.668–0.960 |

## Conditional formatting block write

```sh
node spikes/adr-0022/measure-conditional-formatting.mjs /path/to/baseline /path/to/current
```

The baseline used the same package and Wasm build with only the previous
per-rule calls restored in the JavaScript writer. Each of three Node
v25.9.0 processes used eight warm-up pairs and 36 alternating timed
pairs. Each write starts from a fresh workbook loaded from the same
bytes. Timing covers one `addConditionalFormatting` call with a block
of cell comparisons. Every rule supplies the same differential font.
The first timed pair of each workload compares the rule count, operand
order and output XLSX bytes.

| Rules | Median before → after (ms) | p95 before → after (ms) |
| ---: | ---: | ---: |
| 1 | 0.651–0.664 → 0.656–0.680 | 1.001–1.052 → 0.989–1.096 |
| 20 | 1.643–1.688 → 1.595–1.606 | 2.154–3.265 → 2.128–2.571 |
| 200 | 14.374–14.760 → 13.977–14.282 | 16.062–17.315 → 15.863–16.404 |

## ExcelJS CSV cell reads

```sh
node spikes/adr-0022/measure-csv-cell-reads.mjs /path/to/baseline /path/to/current
```

Pass unpacked builds with their Wasm binaries and runtime dependencies
available. This runs the browser ExcelJS adapter in Node.js. Both builds
load the same XLSX input for each workload. The script times CSV output
for six 1,200-row workloads: dates, ordinary formulas, 12 links,
24 rich-text or error cells, a two-cell array formula, and a four-cell merge. Each workload
uses four warm-up pairs and 20 alternating timed pairs. It checks byte equality after
every pair and reports median and nearest-rank p95 in milliseconds.

A Node.js 25.9.0 run compared the package before the Wasm worksheet
classification cache with the package after the scalar and complex
read changes.

| Workload | Median before → after (ms) | p95 before → after (ms) |
| --- | ---: | ---: |
| Dates | 196.02 → 40.70 | 206.36 → 48.09 |
| Formulas | 628.60 → 33.65 | 639.84 → 41.70 |

## Node streaming writer ZIP compression levels

Run against a built, unpacked package with
`node spikes/adr-0022/measure-stream-zip-level.mjs path/to/xlsx`.

This run used Node v25.9.0 on an AMD EPYC 7313P. Each run wrote
100,000 rows of four cells with repeated group and report strings and
two changing numeric fields. Shared strings were disabled. The output
went to a counting Writable so disk writes for the final archive were
outside the workload. Timings include row staging, compression and
archive finalization. Each level had a 5,000-row warmup and five
100,000-row samples, with execution order rotated between rounds.

| ZIP level | Median wall time | Time range | ZIP bytes |
| --- | ---: | ---: | ---: |
| 0 (stored) | 514 ms | 503–530 ms | 26,667,037 |
| 1 | 609 ms | 596–629 ms | 4,511,230 |
| 6 | 769 ms | 756–784 ms | 2,496,013–2,496,015 |
| 9 | 1,255 ms | 1,225–1,259 ms | 2,511,187–2,511,188 |

Level 6 produced the smallest archive in this workload. Level 9 took
about 1.63 times as long as level 6 and produced an archive about 0.61%
larger. Archive sizes varied by at most two bytes within a level because
the workbook timestamps changed between samples.

A second run of the checked-in script reproduced the size ordering.
Median times for levels 0, 1, 6 and 9 were 511, 592, 772 and 1,221 ms.

## Shared-string cell reads

```sh
node spikes/adr-0022/measure-shared-string-reads.mjs /path/to/baseline /path/to/current [cells ...]
```

Pass unpacked package directories whose runtime dependencies resolve.
The script writes stored-ZIP workbooks with one sheet of ten columns of
`t="s"` cells. The first 60% of the cells each reference a distinct
item and the rest repeat a hot tenth of the table. Every 50th item is
rich text and every 70th carries phonetic runs. Each sample is a fresh
process that performs two warm-up reads of a 1,000-cell workbook and
then one timed read. The timed read covers SheetJS `read` with default
options or `dense: true`, followed by access to every cell, since the
adapter projects a sheet on first access. For ExcelJS it covers
`xlsx.load` followed by `value` and `text` for every cell. File I/O is
excluded. Each process reports its peak RSS (`maxRSS`) and a hash of the
projected result, and the script requires every hash to match across
builds. Builds alternate order across five samples per build.

The baseline resolved each `t="s"` cell's item tree by parsing the
workbook relationships and scanning the table to the n-th `si`. It did
this once per cell in SheetJS `sparse_cells` and ExcelJS scalar
classification. A CPU profile of a 10,000-cell read attributed
69% of SheetJS time and 68% of ExcelJS time to `shared_string_source`,
mostly `memcmp` from the item-name comparisons. `Package::relationships`
accounted for 12% and 17%. The current build caches the item positions
once per package generation.

Node v25.9.0 on a 32-core host shared with other jobs, which produced
load averages up to about 28. Times are milliseconds and RSS is MiB,
shown as median (range).

| Cells | Read | Time before → after | RSS before → after |
| ---: | --- | ---: | ---: |
| 10,000 | SheetJS | 812 (784–835) → 133 (126–145) | 120 (119–121) → 118 (118–120) |
| 10,000 | SheetJS dense | 795 (788–862) → 122 (114–126) | 116 (115–117) → 116 (114–117) |
| 10,000 | ExcelJS | 4,989 (1,810–5,296) → 795 (319–1,408) | 256 (242–258) → 242 (241–259) |
| 50,000 | SheetJS | 16,353 (15,340–41,978) → 680 (616–1,876) | 216 (213–218) → 223 (221–224) |
| 50,000 | SheetJS dense | 16,711 (15,759–17,896) → 654 (635–680) | 191 (189–196) → 214 (196–215) |
| 50,000 | ExcelJS | 38,756 (33,721–100,094) → 1,622 (1,382–4,389) | 624 (622–635) → 630 (625–636) |
| 100,000 | SheetJS | 64,522 (60,397–67,478) → 1,322 (1,197–1,371) | 339 (311–343) → 348 (347–348) |
| 100,000 | SheetJS dense | 68,191 (60,119–79,326) → 1,315 (1,109–1,367) | 294 (294–302) → 319 (304–320) |
| 100,000 | ExcelJS | 181,819 (125,342–197,018) → 2,857 (2,757–2,889) | 1,073 (1,067–1,087) → 1,081 (1,075–1,100) |

Read time now grows roughly linearly with the cell count. The wide
ranges coincide with concurrent load on the host. Peak RSS rose by up to
25 MiB in the dense 50,000- and 100,000-cell reads. The cache holds four
bytes per item, about 0.23 MiB at 60,000 items, so it does not account
for that increase. This benchmark does not isolate the cause. After the
fix, a 100,000-cell SheetJS profile is dominated by workbook load and
parsing. In the ExcelJS profile, about 20% is the per-cell
`cell_hyperlink` call, which parses the sheet relationships for each
cell, and about 20% is `getCell` in JavaScript.
