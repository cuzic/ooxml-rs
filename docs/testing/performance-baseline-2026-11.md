# Performance baseline, November 2026

This is the first measured baseline for ooxml-rs, at `main` @ `c4116e9` (PR #16's depth limit,
xlsx Phase0, pptx Phase0). It is measured against the roadmap's xlsx Phase6 goal: "within 2× of
rust_xlsxwriter (`constant_memory`) write time and calamine read time for a 1M-cell sheet; memory
bounded by rows touched, not cells in the sheet" (`docs/roadmap/xlsx.md`). No crate source was
changed for this report.

## Summary

| Workload (1M-cell sheet unless noted) | ooxml-rs | Reference | Ratio | Phase6 goal |
|---|---|---|---|---|
| Read every cell | Not feasible through the public API: `Worksheet::cell` costs O(rows) per call, so a full read is predicted to take about 1.8–4 h. Open plus raw tree walk: 1.83 s, heap 1.48 GiB | calamine 0.33 s, 77 MiB | 5.5× time, 19× memory (raw walk) | 2× |
| Open only (`Workbook::from_bytes`) | 1.21 s, heap 1.61 GiB | calamine's whole read, 0.33 s | 3.6× | — |
| Write a new sheet | 100k cells: 20.9 s. This is quadratic; 1M cells is predicted at about 35 min | rust_xlsxwriter: 0.11 s at 100k, 1.0 s at 1M. With `constant_memory`: 1.75 s at 1M (run 2), 3.9 MiB | ≈ 190× at 100k | 2× |
| Open, edit 1,000 cells, save | 17.5 s, heap 1.61 GiB | umya-spreadsheet 3.8 s, 300 MiB | 4.6× | — |
| Open, edit 100 cells, save | 4.1 s | umya-spreadsheet 3.7 s | 1.1× | — |
| Package open and save, nothing parsed (3.4 MiB file) | 0.8 ms | — | — | — |
| pptx, 1,000 slides: open, `set_text` on all 10,000 text bodies, save | 0.34 s, heap 86 MiB | — | — | — |

What the numbers say:

- **Layer 1 passthrough is not a concern.** Compressed entries are raw-copied at 0.6–4.5 GB/s.
- **The gaps all come from the Layer 2 tree.**
  - Parsing allocates 45–50 bytes of heap per byte of XML, at 5.4 allocations per element.
  - Parsing runs at about 40 MB/s, which is about 6× slower than quick-xml's event pass alone.
  - Dropping the tree costs another 40% of the parse time.
- **The xlsx Phase0 cell API scans every row on every call.** Reading all cells and writing a sheet
  from scratch are therefore quadratic, and each edit in a 100k-row sheet costs about 22 ms.
- **Saving after an edit is dominated by deflate.** Recompressing the edited 33 MiB sheet takes
  about 1 s at zip's default level and 70 ms at level 1. Serializing it takes only 31 ms.

## Method

- **Harness.** `crates/ooxml-bench` is a dev-only crate (`publish = false`) with two
  `harness = false` bench targets. `cargo test` builds none of its targets. Its dev-dependencies
  (calamine, rust_xlsxwriter, umya-spreadsheet) compile only for `cargo bench`.
  - `cargo bench -p ooxml-bench --bench baseline [-- <id filter>]` produces the results table.
  - `cargo bench -p ooxml-bench --bench breakdown` produces the hotspot tables.
  - `OOXML_BENCH_MAX=100000` limits the generated sizes.
- **Why a custom harness instead of criterion.** Every case also needs a heap peak, several cases
  take seconds per run, and the output is a single Markdown table.
  - Each case is timed at least 3 times and until a 3 s budget is spent, with at most 15 runs. A
    single run that exceeds the budget is reported as is.
  - Inputs are already in memory, so no I/O is timed.
  - A quadratic case is skipped at the next size when it is predicted to exceed 60 s. The
    prediction is the previous median times the size ratio squared, and it is reported instead.
- **Memory.** A counting global allocator (`ooxml_bench::PeakAlloc`) records the peak of live heap
  bytes during the first timed run, above the live heap at its start.
  - It counts heap only: no code, stack or allocator overhead.
  - It includes the clone of the input bytes that each case makes as an owned `Vec`, for APIs that
    take one.
  - Heap peaks are deterministic; they were identical in both runs.
- **Inputs.**
  - Generated workbooks of 10k, 100k and 1M cells, 10 columns per row, written by rust_xlsxwriter.
    Column A is a unique string in the shared-string table and the other columns are numbers. The
    zips are 39 KiB, 356 KiB and 3.4 MiB; sheet1.xml is 298 KiB, 3.1 MiB and 33 MiB.
  - Generated decks: `python-pptx/ph-inherit-props.pptx` plus 100 or 1,000 slides, each with a
    title and 9 text boxes of 5 two-run paragraphs (`ooxml_bench::generate_deck`).
  - Fixtures: the 23 xlsx and 23 pptx files under `reference/fixtures/`.
- **What some cases do.**
  - `xlsx/edit-save` sets column B of 100 or 1,000 rows spread evenly over the sheet.
  - `xlsx/write-new` writes the same grid as the generated input, row by row.
  - `xlsx/read-all/ooxml-raw-walk` is not a public API. It opens the `Package`, parses
    sheet1.xml and walks `sheetData/row/c/v` on the raw tree. It is the lower bound for any
    full-read API built on today's Layer 2.
- **Machine.** AMD EPYC 7313P (16 cores, 32 threads), 125 GiB RAM, Linux 6.16. rustc 1.99.1,
  release profile (`opt-level = 3`, no LTO). calamine 0.38.0, rust_xlsxwriter 0.102.0 (feature
  `constant_memory`), umya-spreadsheet 2.3.3, quick-xml 0.42.0, zip 9.6.0.
- **Host load.** The host is shared with other agents' builds and fuzzers and could not be made
  quiet, so absolute numbers are upper bounds.
  - Run 1 (06:35 UTC): the load was not recorded, but it was lower.
  - Run 2 (07:00 UTC): the 1-minute load average was 59 at the start and 47 at the end, on 32
    hardware threads.
  - Absolute times in run 2 are about 1.5–3× those of run 1.
  - Ratios against the reference libraries agree between the runs within about 30%: read of 1M
    cells 5.5× and 4.3×, 1,000 edits 4.6× and 4.5×, 100 edits 1.1× and 0.9×.
  - Rerun on a quiet machine before quoting absolute numbers.
- **Profiling.** Sampling profilers were not usable here.
  - `perf` has no tools package for this kernel.
  - `gprofng` clock profiling captured only about 10% of the CPU time.
  - The hotspots below rest on targeted timings and allocation counts instead (`breakdown`,
    run 2).
  - The short gprofng sample of `XmlDocument::parse` agrees with them. About 50% of samples were
    in `malloc`/`free`, and `drop_glue::<RawNode>` and `RawVec<RawItem>::grow_one` were the top
    ooxml-rs frames.

## Results

Run 1 was the quieter run and run 2 ran under heavy load (see Method). "—" means the case was not
in run 1; `constant_memory` was added for run 2.

| Case | Input | Median, run 1 | Median, run 2 | Min, run 2 | Heap peak | Throughput, run 1 (run 2) |
|---|---|---|---|---|---|---|
| `core/package-passthrough/fixtures` | 46 xlsx+pptx fixtures, 2.2 MiB | 3.58 ms | 19.41 ms | 12.69 ms | 859.1 KiB | 641.4 MB/s (118.4 MB/s) |
| `core/package-passthrough/generated` | 10k cells, 38.6 KiB | 36.7 µs | 69.0 µs | 67.7 µs | 99.4 KiB | 1075.0 MB/s (572.1 MB/s) |
| `core/package-parse-sheet-passthrough` | 10k cells: open, parse sheet1.xml, save | 14.60 ms | 57.74 ms | 49.40 ms | 15.0 MiB |  |
| `core/xml-parse` | 10k cells: sheet1.xml 298.0 KiB | 7.25 ms | 44.83 ms | 28.34 ms | 14.6 MiB | 42.1 MB/s (6.8 MB/s) |
| `core/xml-serialize-unedited` | 10k cells: sheet1.xml 298.0 KiB | 7.4 µs | 18.0 µs | 17.6 µs | 298.1 KiB | 41014.0 MB/s (17001.0 MB/s) |
| `core/xml-serialize-rerender` | 10k cells: sheet1.xml 298.0 KiB | 1.40 ms | 5.64 ms | 2.54 ms | 298.1 KiB | 217.8 MB/s (54.1 MB/s) |
| `core/xml-clone` | 10k cells: sheet1.xml 298.0 KiB | 3.04 ms | 18.51 ms | 18.04 ms | 5.6 MiB | 100.4 MB/s (16.5 MB/s) |
| `core/raw-walk-all-cells` | 10k cells: sheet1.xml 298.0 KiB | 277.1 µs | 5.76 ms | 2.24 ms | 0 B | 36090790.0 cells/s (1736075.8 cells/s) |
| `core/package-passthrough/generated` | 100k cells, 355.5 KiB | 91.9 µs | 172.8 µs | 146.6 µs | 748.2 KiB | 3962.9 MB/s (2106.2 MB/s) |
| `core/package-parse-sheet-passthrough` | 100k cells: open, parse sheet1.xml, save | 154.61 ms | 591.43 ms | 447.16 ms | 151.0 MiB |  |
| `core/xml-parse` | 100k cells: sheet1.xml 3.1 MiB | 82.67 ms | 238.17 ms | 215.35 ms | 147.5 MiB | 39.4 MB/s (13.7 MB/s) |
| `core/xml-serialize-unedited` | 100k cells: sheet1.xml 3.1 MiB | 104.8 µs | 142.3 µs | 114.6 µs | 3.1 MiB | 31078.9 MB/s (22887.6 MB/s) |
| `core/xml-serialize-rerender` | 100k cells: sheet1.xml 3.1 MiB | 15.57 ms | 36.85 ms | 28.00 ms | 3.1 MiB | 209.2 MB/s (88.4 MB/s) |
| `core/xml-clone` | 100k cells: sheet1.xml 3.1 MiB | 62.86 ms | 152.51 ms | 91.48 ms | 56.0 MiB | 51.8 MB/s (21.4 MB/s) |
| `core/raw-walk-all-cells` | 100k cells: sheet1.xml 3.1 MiB | 12.30 ms | 49.11 ms | 27.95 ms | 0 B | 8130784.6 cells/s (2036212.0 cells/s) |
| `core/package-passthrough/generated` | 1000k cells, 3.4 MiB | 798.7 µs | 4.41 ms | 1.56 ms | 9.4 MiB | 4517.6 MB/s (817.5 MB/s) |
| `core/package-parse-sheet-passthrough` | 1000k cells: open, parse sheet1.xml, save | 1.68 s | 4.65 s | 4.65 s | 1.48 GiB |  |
| `core/xml-parse` | 1000k cells: sheet1.xml 33.1 MiB | 850.44 ms | 2.77 s | 1.73 s | 1.44 GiB | 40.8 MB/s (12.5 MB/s) |
| `core/xml-serialize-unedited` | 1000k cells: sheet1.xml 33.1 MiB | 18.32 ms | 25.51 ms | 22.81 ms | 33.1 MiB | 1892.8 MB/s (1358.8 MB/s) |
| `core/xml-serialize-rerender` | 1000k cells: sheet1.xml 33.1 MiB | 179.43 ms | 361.12 ms | 319.67 ms | 33.1 MiB | 193.2 MB/s (96.0 MB/s) |
| `core/xml-clone` | 1000k cells: sheet1.xml 33.1 MiB | 626.85 ms | 1.38 s | 1.04 s | 563.9 MiB | 55.3 MB/s (25.1 MB/s) |
| `core/raw-walk-all-cells` | 1000k cells: sheet1.xml 33.1 MiB | 180.03 ms | 332.36 ms | 288.39 ms | 0 B | 5554705.6 cells/s (3008753.3 cells/s) |
| `xlsx/open/ooxml` | 10k cells | 10.62 ms | 21.76 ms | 21.21 ms | 16.3 MiB | 941653.3 cells/s (459457.1 cells/s) |
| `xlsx/read-all/calamine` | 10k cells | 3.23 ms | 5.91 ms | 5.50 ms | 882.9 KiB | 3091387.9 cells/s (1692854.8 cells/s) |
| `xlsx/read-all/ooxml-raw-walk` | 10k cells (open + Package raw tree walk, no public API) | 9.72 ms | 26.08 ms | 25.50 ms | 15.0 MiB | 1029178.1 cells/s (383444.8 cells/s) |
| `xlsx/read-all/ooxml-cell-api` | 10k cells (open + Worksheet::cell for every cell) | 653.50 ms | 1.50 s | 1.22 s | 16.3 MiB | 15302.2 cells/s (6685.7 cells/s) |
| `xlsx/open/ooxml` | 100k cells | 116.63 ms | 296.69 ms | 247.13 ms | 165.0 MiB | 857411.6 cells/s (337046.9 cells/s) |
| `xlsx/read-all/calamine` | 100k cells | 31.77 ms | 73.40 ms | 62.31 ms | 8.8 MiB | 3147137.5 cells/s (1362358.1 cells/s) |
| `xlsx/read-all/ooxml-raw-walk` | 100k cells (open + Package raw tree walk, no public API) | 161.84 ms | 318.93 ms | 294.93 ms | 151.0 MiB | 617906.1 cells/s (313546.3 cells/s) |
| `xlsx/read-all/ooxml-cell-api` | 100k cells | skipped: predicted ≈ 65.35 s (quadratic: Worksheet::cell scans every row) | skipped: predicted ≈ 149.57 s (quadratic: Worksheet::cell scans every row) | | | |
| `xlsx/open/ooxml` | 1000k cells | 1.21 s | 3.98 s | 3.98 s | 1.61 GiB | 826907.1 cells/s (251163.5 cells/s) |
| `xlsx/read-all/calamine` | 1000k cells | 332.72 ms | 779.04 ms | 723.85 ms | 77.2 MiB | 3005537.8 cells/s (1283630.7 cells/s) |
| `xlsx/read-all/ooxml-raw-walk` | 1000k cells (open + Package raw tree walk, no public API) | 1.83 s | 3.37 s | 3.37 s | 1.48 GiB | 545425.9 cells/s (296816.6 cells/s) |
| `xlsx/read-all/ooxml-cell-api` | 1000k cells | skipped: predicted ≈ 6535.02 s (quadratic: Worksheet::cell scans every row) | skipped: predicted ≈ 14957.24 s (quadratic: Worksheet::cell scans every row) | | | |
| `xlsx/edit-save/ooxml` | 10k cells, 100 edits | 23.79 ms | 49.49 ms | 46.79 ms | 16.7 MiB | 4204.1 edits/s (2020.5 edits/s) |
| `xlsx/edit-save/umya` | 10k cells, 100 edits | 26.42 ms | 51.14 ms | 49.15 ms | 3.3 MiB | 3785.0 edits/s (1955.4 edits/s) |
| `xlsx/edit-save/ooxml` | 10k cells, 1000 edits | 57.35 ms | 110.08 ms | 107.26 ms | 17.0 MiB | 17437.7 edits/s (9084.0 edits/s) |
| `xlsx/edit-save/umya` | 10k cells, 1000 edits | 26.44 ms | 50.05 ms | 49.06 ms | 3.3 MiB | 37816.5 edits/s (19978.7 edits/s) |
| `xlsx/edit-save/ooxml` | 100k cells, 100 edits | 293.71 ms | 565.62 ms | 544.10 ms | 165.9 MiB | 340.5 edits/s (176.8 edits/s) |
| `xlsx/edit-save/umya` | 100k cells, 100 edits | 288.93 ms | 620.18 ms | 582.40 ms | 28.4 MiB | 346.1 edits/s (161.2 edits/s) |
| `xlsx/edit-save/ooxml` | 100k cells, 1000 edits | 641.08 ms | 1.10 s | 1.10 s | 166.0 MiB | 1559.9 edits/s (907.6 edits/s) |
| `xlsx/edit-save/umya` | 100k cells, 1000 edits | 299.55 ms | 591.72 ms | 552.80 ms | 28.4 MiB | 3338.3 edits/s (1690.0 edits/s) |
| `xlsx/edit-save/ooxml` | 1000k cells, 100 edits | 4.11 s | 8.33 s | 8.33 s | 1.61 GiB | 24.3 edits/s (12.0 edits/s) |
| `xlsx/edit-save/umya` | 1000k cells, 100 edits | 3.74 s | 9.17 s | 9.17 s | 300.3 MiB | 26.7 edits/s (10.9 edits/s) |
| `xlsx/edit-save/ooxml` | 1000k cells, 1000 edits | 17.49 s | 35.27 s | 35.27 s | 1.61 GiB | 57.2 edits/s (28.4 edits/s) |
| `xlsx/edit-save/umya` | 1000k cells, 1000 edits | 3.84 s | 7.83 s | 7.83 s | 300.3 MiB | 260.3 edits/s (127.6 edits/s) |
| `xlsx/write-new/rust_xlsxwriter` | 10k cells | 12.31 ms | 29.76 ms | 20.77 ms | 1.8 MiB | 812380.0 cells/s (336044.4 cells/s) |
| `xlsx/write-new/rust_xlsxwriter-constant-memory` | 10k cells | — | 13.99 ms | 12.20 ms | 493.0 KiB | — (714652.9 cells/s) |
| `xlsx/write-new/ooxml` | 10k cells | 238.85 ms | 460.77 ms | 437.51 ms | 19.8 MiB | 41867.7 cells/s (21702.8 cells/s) |
| `xlsx/write-new/rust_xlsxwriter` | 100k cells | 108.62 ms | 212.59 ms | 170.31 ms | 12.8 MiB | 920611.1 cells/s (470393.1 cells/s) |
| `xlsx/write-new/rust_xlsxwriter-constant-memory` | 100k cells | — | 122.87 ms | 112.34 ms | 886.6 KiB | — (813852.2 cells/s) |
| `xlsx/write-new/ooxml` | 100k cells | 20.92 s | 45.23 s | 45.23 s | 194.9 MiB | 4779.5 cells/s (2211.0 cells/s) |
| `xlsx/write-new/rust_xlsxwriter` | 1000k cells | 1.01 s | 2.19 s | 1.89 s | 148.3 MiB | 993628.4 cells/s (455874.5 cells/s) |
| `xlsx/write-new/rust_xlsxwriter-constant-memory` | 1000k cells | — | 1.75 s | 1.72 s | 3.9 MiB | — (570109.4 cells/s) |
| `xlsx/write-new/ooxml` | 1000k cells | skipped: predicted ≈ 2092.26 s (quadratic: each write scans every row) | skipped: predicted ≈ 4522.80 s (quadratic: each write scans every row) | | | |
| `pptx/open` | 23 pptx fixtures, 2.0 MiB | 3.54 ms | 6.77 ms | 6.60 ms | 751.5 KiB |  |
| `pptx/read-all-text` | 23 pptx fixtures, 2.0 MiB (open + every slide's shapes' text) | 16.45 ms | 31.69 ms | 30.66 ms | 4.0 MiB |  |
| `pptx/set-text-all-save` | 23 pptx fixtures, 2.0 MiB (open + set_text on every text body + save) | 23.95 ms | 45.51 ms | 43.44 ms | 4.6 MiB |  |
| `pptx/open` | generated deck: 100 slides × 10 shapes × 5 paragraphs, 147.3 KiB | 548.3 µs | 1.03 ms | 980.4 µs | 385.5 KiB |  |
| `pptx/read-all-text` | generated deck: 100 slides × 10 shapes × 5 paragraphs, 147.3 KiB (open + every slide's shapes' text) | 23.26 ms | 60.44 ms | 49.44 ms | 21.2 MiB |  |
| `pptx/set-text-all-save` | generated deck: 100 slides × 10 shapes × 5 paragraphs, 147.3 KiB (open + set_text on every text body + save) | 28.32 ms | 59.70 ms | 57.87 ms | 9.2 MiB |  |
| `pptx/open` | generated deck: 1000 slides × 10 shapes × 5 paragraphs, 1.2 MiB | 9.11 ms | 17.19 ms | 16.01 ms | 3.1 MiB |  |
| `pptx/read-all-text` | generated deck: 1000 slides × 10 shapes × 5 paragraphs, 1.2 MiB (open + every slide's shapes' text) | 309.39 ms | 623.02 ms | 591.76 ms | 210.7 MiB |  |
| `pptx/set-text-all-save` | generated deck: 1000 slides × 10 shapes × 5 paragraphs, 1.2 MiB (open + set_text on every text body + save) | 336.64 ms | 906.49 ms | 737.95 ms | 86.0 MiB |  |

## Hotspots

The tables below come from `cargo bench -p ooxml-bench --bench breakdown` (run 2).

### Type sizes

| Type | Bytes |
|---|---|
| `RawNode` | 144 |
| `RawItem` | 144 |
| `RawAttr` | 72 |
| `RawText` | 56 |
| `XName` | 48 |

### XML tree cost (sheet1.xml)

| Cells | XML | Elements | Attributes | Text nodes | Allocations | Heap after parse | Heap / XML byte | Allocs / element | quick-xml events only | `XmlDocument::parse` | `drop` |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 10000 | 298.0 KiB | 21007 | 13012 | 10000 | 113079 | 14.6 MiB | 50.3 | 5.38 | 4.09 ms | 22.88 ms | 7.76 ms |
| 100000 | 3.1 MiB | 210007 | 130012 | 100000 | 1130083 | 147.5 MiB | 47.5 | 5.38 | 41.83 ms | 266.22 ms | 104.91 ms |
| 1000000 | 33.1 MiB | 2100007 | 1300012 | 1000000 | 11300086 | 1.44 GiB | 44.6 | 5.38 | 498.85 ms | 2.87 s | 1.23 s |

### `Workbook::from_bytes` phases

| Cells | `Package::from_bytes` | decompress sheet1.xml | parse sheet1.xml | quick-xml scan of sheet1.xml | `Workbook::from_bytes` total |
|---|---|---|---|---|---|
| 10000 | 41.6 µs | 356.4 µs | 23.71 ms | 2.67 ms | 31.77 ms |
| 100000 | 91.8 µs | 4.32 ms | 263.42 ms | 26.64 ms | 356.87 ms |
| 1000000 | 457.5 µs | 42.20 ms | 3.48 s | 273.68 ms | 3.91 s |

### Per-call cost of the cell API (after open)

| Cells (rows) | `cell(0, 0)` | `cell(last row, 0)` | `cell_mut(last row, 1).set_number` (one edit) | `to_bytes` after one edit | of which: serialize sheet1.xml | of which: deflate sheet1.xml (zip default level) | deflate at level 1 |
|---|---|---|---|---|---|---|---|
| 10000 (1000) | 56.4 µs | 172.5 µs | 58.5 µs | 10.07 ms | 22.3 µs | 9.81 ms | 684.0 µs |
| 100000 (10000) | 1.64 ms | 2.05 ms | 589.3 µs | 92.94 ms | 480.2 µs | 90.98 ms | 7.16 ms |
| 1000000 (100000) | 18.51 ms | 44.17 ms | 22.40 ms | 1.09 s | 30.99 ms | 987.09 ms | 70.04 ms |

### From-scratch writes: cost of one row (10 cells) after k rows

| k (rows already written) | one more row |
|---|---|
| 100 | 113.9 µs |
| 1000 | 798.8 µs |
| 5000 | 3.82 ms |
| 10000 | 18.51 ms |

### PowerPoint: 1,000 generated slides

| Step | Time | Note |
|---|---|---|
| `Presentation::from_bytes` | 25.37 ms | |
| parse all 1,000 slides (`slide(i)`, first call) | 380.05 ms | 11.9 MiB of slide XML; 210.7 MiB retained in the package after parsing (18× the XML) |
| `shapes()` on every slide (already parsed) | 11.41 ms | 10004 shapes |
| `shapes()` + `text_body().text()` on every slide | 69.39 ms | 3474909 bytes of text; transient heap peak 2.3 KiB |

### 1. Layer 2 tree: parsing is allocation-bound and needs 45–50× the XML size in memory

- **Evidence.**
  - `XmlDocument::parse` takes 6–7× as long as a bare quick-xml event pass over the same bytes
    (2.9 s against 0.5 s for 33 MiB).
  - It makes 5.38 allocations per element:
    - each element: an `XName` with a boxed `local` (and a boxed `prefix` when there is one), an
      `attrs` `Vec` and a `children` `Vec`;
    - each attribute: another `XName` and a `String` value;
    - each text node: a `String`.
  - `RawNode` and `RawItem` are 144 bytes, `RawAttr` 72, `XName` 48.
  - Dropping the tree costs 40% of the parse time.
  - Holding a 1M-cell sheet takes 1.44 GiB, against 77 MiB for calamine's whole read.
  - `XmlDocument::clone` runs at 25–100 MB/s. xlsx Phase1 clones `workbook.xml` on every edit,
    but never a sheet.
- **Suggestions**, roughly in order of payoff per effort:
  1. **Intern names.** Make `local` and `prefix` `Arc<str>` (or a `u32` symbol) from the parser's
     existing interner, as `ns` already is. That removes 2–3 of the 5.4 allocations per element
     and shrinks `XName` to 24 bytes or less.
  2. **Keep untouched attribute values and text as spans into the source.** Decode them lazily
     (`Cow`, or a `Range<u32>` plus a "needs unescape" bit) and materialize them only when edited.
     The source is already kept for T1.
  3. **Smaller nodes.** Use `u32` offsets in `ElementSpan`, pack the two dirty flags with the span,
     and give `children` and `attrs` small inline capacity. Allocate attributes once per start
     tag; the parser already knows how many there are.
  4. **Do not build what is not read.** For the worksheet this is the "lazy row-level `sheetData`"
     that Phase6 already plans: index rows by byte span and build a row only when it is read or
     written. Memory is then bounded by rows touched (the Phase6 goal), and open becomes a scan,
     which quick-xml does at about 80 MB/s here.

### 2. xlsx cell API: O(rows) per call, so full reads and from-scratch writes are quadratic

- **Evidence.**
  - `Worksheet::cell(last row, 0)` takes 0.17 ms at 1k rows, 2 ms at 10k and 44 ms at 100k.
  - Even `cell(0, 0)` takes 18 ms at 100k rows, because `locate` first checks the order of every
    row.
  - Writing one more row of 10 cells takes 0.1 ms after 100 rows and 18.5 ms after 10,000.
  - `Worksheet::locate` (`crates/ooxml-xlsx/src/worksheet.rs`) iterates every `row` of `sheetData`
    and parses `r` to check ascending order, then scans the target row's cells. `cell` then walks
    the rows again through the typed lens to find the same node (`children::<Row>().find(ptr::eq)`).
  - Each write also re-checks the dimension, plans the edit, and re-walks the `LensMut` path from
    the root.
- **Suggestions.**
  1. **A row index per worksheet.** Build a `row number → child index` index once, on first use,
     and validate the order then. Update it on row insertion; inserts only happen through
     `Worksheet`, which owns the tree. `locate` then costs O(log rows) or O(1), plus a scan of the
     row's cells, which is bounded by the column count.
  2. **An append fast path for from-scratch writes.** When the target row is past the last row,
     push the row and cell directly with no lookup.
  3. **A full-read iterator**, e.g. `Worksheet::rows()` or `cells()`, that yields `CellRef`s over
     `sheetData` in one pass. The raw-walk case shows this would cost about as much as opening
     (1.83 s at 1M cells, run 1), not hours.
  4. **A streaming from-scratch writer** (planned for Phase6). Rows are written in order, so
     `Workbook::new` can build `sheetData` as text the way rust_xlsxwriter does, instead of as a
     tree.

### 3. Save: default-level deflate of the rewritten part

- **Evidence.** After one edit of the 1M-cell sheet, `to_bytes` takes 1.09 s:
  - 31 ms to serialize sheet1.xml (untouched siblings are copied verbatim);
  - 0.99 s to deflate the 33 MiB result at zip's default level.

  Deflating at level 1 takes 70 ms, 14× faster. Unedited parts cost nothing: they are raw-copied,
  which is why `core/package-passthrough` runs at GB/s.
- **Suggestions.**
  1. Make the compression level of rewritten parts configurable on `Package` (e.g.
     `SaveOptions { level }`) and choose a fast default. Excel itself writes at a low level
     *(u)*; measure the size cost on real files before choosing.
  2. For very large parts, compress in parallel chunks. Deflate allows independent blocks.

### 4. Open: the tree is most of the cost

- **Evidence.** In `Workbook::from_bytes` for 1M cells (run 2), parsing sheet1.xml takes 3.48 s
  of the 3.91 s total. The zip directory takes 0.5 ms, decompression 42 ms, and the formula census
  (a quick-xml scan) about 0.27 s.
- **Suggestions.**
  1. The census and the parse each read the sheet. With the lazy `sheetData` of hotspot 1, they
     can share one pass.
  2. Until then, the census of the first sheet could run on the parsed tree instead of re-scanning
     the bytes.

### 5. pptx: every parsed slide stays in memory

- **Evidence.**
  - Reading the text of 1,000 slides parses each slide once, in 380 ms (about 31 MB/s, as
    hotspot 1 predicts).
  - All 1,000 parsed slides stay in the `Package`: 211 MiB retained for 11.9 MiB of slide XML
    (18×).
  - `shapes()` and `text()` on the parsed slides take 11 ms and 69 ms for 10,000 shapes, so the
    walk is not the cost.
  - Opening a deck is cheap: 9 ms for 1,000 slides.
- **Suggestions.**
  1. Hotspot 1 shrinks this directly.
  2. Add a read path that does not keep the tree. Either `Presentation::slide` parses into a
     temporary `XmlDocument` instead of going through `Package::xml_part`'s cache, or callers can
     drop a parsed, unmodified part back to `Raw` (e.g. `Package::release_part`).
  3. Editing paths must keep their trees until save. Those trees are bounded by the slides
     actually edited.

### Not hotspots

- Package open and save passthrough.
- Serializing an unedited tree, which copies the source: 1.3–40 GB/s.
- `XmlDocument::serialize` after `force_rerender`: about 200 MB/s.
- The pptx shape walk.
- Opening the 46 fixtures, passthrough included: 3.6 ms in total.

## Reproducing

```sh
cargo bench -p ooxml-bench --bench baseline     # results table, about 2–5 min
cargo bench -p ooxml-bench --bench breakdown    # hotspot tables, about 1–2 min
cargo bench -p ooxml-bench --bench baseline -- xlsx/edit-save   # one family
```

Rerun on a quiet machine before comparing absolute numbers across commits. Ratios against the
reference libraries measured in the same run are the stable quantity.
