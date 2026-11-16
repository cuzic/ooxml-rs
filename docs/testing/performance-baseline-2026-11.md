# Performance baseline, November 2026

This is the first measured baseline for ooxml-rs, at `main` @ `c4116e9` (PR #16's depth limit,
xlsx Phase0, pptx Phase0). It is compared against the roadmap goal for xlsx Phase6: "within 2×
of rust_xlsxwriter (`constant_memory`) write time and calamine read time for a 1M-cell sheet;
memory bounded by rows touched, not cells in the sheet" (`docs/roadmap/xlsx.md`). No crate source
was changed for this report.

Revision 2, after the review `docs/reviews/pr24-round1.md`:

- Every number comes from **one run** (run 3).
- Baseline cases now include dropping the result.
- Breakdown timings exclude drop, which is reported separately.
- Extrapolations are labelled as lower bounds.

## Summary

All figures are from run 3. "×" is ooxml-rs time divided by the reference library's time.

| Workload (1M-cell sheet unless noted) | ooxml-rs | Reference | Ratio | Phase6 goal |
|---|---|---|---|---|
| Read every cell through the public API | Not feasible. A quadratic extrapolation from 10k cells gives a lower bound of ≈ 3.7 h. The measured per-call cost on this sheet (20–44 ms, breakdown) gives ≈ 5.6–12 h. | calamine 0.65 s (median), 77 MiB | — | 2× |
| Open plus a walk of the raw tree (no public API) | 3.33 s (one cold run), heap 1.48 GiB | calamine 0.65 s, 77 MiB | 5.2× time, 19× memory. Favourable to ooxml-rs: the walk resolves neither shared strings nor styles; calamine does. | 2× |
| Open only (`Workbook::from_bytes`, drop included) | 3.51 s (one cold run), heap 1.61 GiB. Of this, drop is 1.2 s. | calamine's whole read 0.65 s | 5.4× | — |
| Write a new sheet | 100k cells: 35.9 s. It is quadratic: the 1M lower bound is ≈ 1.0 h (from 100k). | rust_xlsxwriter `constant_memory`: 0.115 s at 100k, 1.11 s at 1M | ≈ 310× at 100k | 2× |
| Open, edit 1,000 cells, save | 29.3 s, heap 1.61 GiB | umya-spreadsheet 6.7 s, 300 MiB | 4.4× | — |
| Open, edit 100 cells, save | 6.9 s | umya-spreadsheet 6.8 s | 1.0× | — |
| Package open and save, nothing parsed (3.4 MiB file) | 1.4 ms | — | — | — |
| pptx: open, `set_text` on all 10,000 text bodies, save (1,000 slides) | 0.63 s, heap 87 MiB | — | — | — |

What the numbers say:

- **Layer 1 passthrough is not a concern.** It raw-copies the compressed entries at 0.36–2.6 GB/s.
- **The xlsx Phase0 cell API scans every row on each call.**
  - Full reads and from-scratch writes are therefore quadratic.
  - Each edit in a 100k-row sheet costs about 20 ms. That is why 1,000 edits take 4.4× as long as
    umya-spreadsheet.
  - This is the single largest gap (hotspot 2).
- **The Layer 2 tree is allocation-bound.**
  - It makes 5.3 allocations (plus 0.1 reallocations) per element.
  - It holds 45–50 bytes of heap per byte of XML.
  - Parsing (drop excluded) takes 4.0–5.6× as long as quick-xml's bare event pass, and dropping
    the tree costs a further 46–80% of the parse time.
- **Saving after an edit is mostly compression.** `to_bytes` for the edited 33 MiB sheet takes
  1.06 s. Deflating that sheet alone takes 1.07 s at zip's default level and 69 ms at level 1;
  serializing the tree takes 29 ms.

**Recommended order of work** for Phase6 (review m8):

1. **Row index and append fast path in ooxml-xlsx** (hotspot 2.1–2.2; in progress on branch
   `perf/xlsx-row-index`). This removes the quadratic behaviour and most of the edit gap.
2. **A compression level for rewritten parts** (hotspot 3). It is a small change that makes saving
   a large edited part about 14× faster.
3. **A lazy, row-based `sheetData`** (hotspot 1.4). It is the only change that can meet the memory
   goal (memory proportional to the rows touched).
4. **The compact tree** (hotspot 1.1–1.3). It helps every part, pptx and docx included. For
   worksheets, most of its effect is superseded by item 3.
5. **A single-pass census** (hotspot 4) and **non-caching pptx reads** (hotspot 5).

Items 2–5 are designed in ADR-0010 (`docs/adr/0010-core-performance.md`).

## Method

- **Harness.**
  - `crates/ooxml-bench` is a dev-only crate (`publish = false`) with two `harness = false` bench
    targets:
    - `cargo bench -p ooxml-bench --bench baseline [-- <id filter>]` produces the results table;
    - `cargo bench -p ooxml-bench --bench breakdown` produces the hotspot tables.
  - `OOXML_BENCH_MAX=100000` limits the generated sizes.
  - **`cargo test --workspace` builds none of its targets.** It also does not build umya-spreadsheet
    or rust_xlsxwriter; calamine is built anyway, because it is a dev-dependency of ooxml-xlsx.
  - **CI's `cargo clippy --workspace --all-targets` does check the bench targets**, about 55 extra
    crates (umya-spreadsheet, rust_xlsxwriter, pest, chrono, aes, tempfile, …). It passes cleanly
    today.
    - The risk is that a future toolchain or lint breaks one of those crates and fails CI.
    - That risk is accepted for now, so that the benches stay compiled and correct.
    - If it bites, CI can pass `--exclude ooxml-bench`.
- **Timing.**
  - A small custom harness is used instead of criterion. Every case also needs a heap peak,
    several take seconds per run, and the output is one Markdown table.
  - **Baseline cases include dropping their result**: the total cost to the caller, uniform across
    cases (review M1). Inputs are in memory, so no input I/O is timed; `constant_memory` is the one
    exception (see below).
  - Each case gets one untimed warm-up. It is then timed at least 3 times and until a 3 s budget
    is spent, at most 15 runs.
  - If the warm-up alone takes 3 s or more, that cold run is the only sample (`Runs = 1`, so
    Median = Min). Those rows are single measurements on a loaded host; read them as approximate.
  - **Breakdown timings exclude drop.** The value is dropped after the clock stops, and drop is
    measured on its own. They are best of 3, except the "one more row" table, which is one
    measurement per row count.
- **Extrapolations.**
  - A quadratic case is skipped at a size where a quadratic extrapolation from the previous
    measured size exceeds 60 s.
  - The extrapolation is printed as a **lower bound** together with its starting size. The
    per-call cost grows faster than linearly in the row count (breakdown: `cell(0, 0)` costs 29×
    more at 10× the rows), so the true time is longer.
- **Memory.**
  - A counting global allocator (`ooxml_bench::PeakAlloc`) reports the peak of live heap bytes
    during the warm-up run, above the live heap at its start.
  - It counts heap only: no code, stack, allocator overhead or files. It includes the clone of the
    input that each case makes, for APIs that take an owned `Vec`.
  - A moving `realloc` is counted as holding both blocks, so the peak is conservative. Fresh
    allocations and reallocations are counted separately.
  - Heap peaks are deterministic.
- **Inputs.**
  - Generated workbooks of 10k, 100k and 1M cells, 10 columns per row, written by rust_xlsxwriter.
    Column A is a unique string in the shared-string table; the other columns are numbers. The
    zips are 39 KiB, 356 KiB and 3.4 MiB, and their sheet1.xml is 298 KiB, 3.1 MiB and 33 MiB.
  - Generated decks: `python-pptx/ph-inherit-props.pptx` plus 100 or 1,000 slides, each with a
    title and 9 text boxes of 5 two-run paragraphs (`ooxml_bench::generate_deck`).
  - Fixtures: the 23 xlsx and 23 pptx files under `reference/fixtures/`.
- **What the comparisons do.**
  - `xlsx/edit-save` sets column B of 100 or 1,000 rows spread evenly over the sheet. ooxml-rs and
    umya-spreadsheet edit the same cells; umya reads the whole sheet at open.
  - `xlsx/read-all/calamine` and `ooxml-cell-api` both resolve shared strings.
    `xlsx/read-all/ooxml-raw-walk` is not a public API. It opens the `Package`, parses sheet1.xml
    and walks `sheetData/row/c/v`. It loads neither shared strings nor styles, and simply adds up
    `v` (column A's `v` is an index into the shared-string table). It is a lower bound for a
    full-read API on today's Layer 2, and a comparison favourable to ooxml-rs.
  - `xlsx/write-new` writes the same grid, row by row.
    - ooxml-rs and rust_xlsxwriter's `constant_memory` both write column A as inline strings.
      rust_xlsxwriter's normal mode uses a shared-string table.
    - `constant_memory` streams rows to a temporary file (`tempfile_in`; `/tmp` on ext4 here). Its
      time includes that file I/O, and its heap peak excludes the file, about 33 MiB at 1M cells.
- **Machine.**
  - AMD EPYC 7313P (16 cores, 32 threads), 125 GiB RAM, Linux 6.16.
  - rustc 1.99.1, release profile (`opt-level = 3`, no LTO).
  - calamine 0.38.0, rust_xlsxwriter 0.102.0 (feature `constant_memory`), umya-spreadsheet 2.3.3,
    quick-xml 0.42.0, zip 9.6.0.
- **Host load.**
  - The host is shared with other agents' builds and fuzzers and could not be made quiet. During
    run 3 (07:19–07:24 UTC) the 1-minute load average was 34–39 on 32 hardware threads.
  - Absolute times are therefore upper bounds. Earlier exploratory runs on a quieter host were up
    to 2–3× faster in absolute terms, with ratios within about 30% of these.
  - Rerun on a quiet machine before quoting absolute numbers. Ratios within one run are the stable
    quantity.
- **Profiling.**
  - `perf` has no tools package for this kernel, and `gprofng` clock profiling captured only about
    10% of the CPU time. The hotspots therefore rest on targeted timings and allocation counts
    (the breakdown).
  - A short gprofng sample of `XmlDocument::parse` agreed with them. About half of the samples
    were in `malloc`/`free`, and `drop_glue::<RawNode>` and `RawVec<RawItem>::grow_one` were the
    top ooxml-rs frames.

## Results (run 3)

| Case | Input | Median | Min | Runs | Heap peak | Throughput |
|---|---|---|---|---|---|---|
| `core/package-passthrough/fixtures` | 46 xlsx+pptx fixtures, 2.2 MiB | 6.40 ms | 6.00 ms | 15 | 1.1 MiB | 359.2 MB/s |
| `core/package-passthrough/generated` | 10k cells, 38.6 KiB | 68.8 µs | 67.1 µs | 15 | 118.0 KiB | 573.8 MB/s |
| `core/package-parse-sheet-passthrough` | 10k cells: open, parse sheet1.xml, save | 25.94 ms | 23.88 ms | 15 | 15.0 MiB |  |
| `core/xml-parse` | 10k cells: sheet1.xml 298.0 KiB | 25.84 ms | 25.40 ms | 15 | 14.6 MiB | 11.8 MB/s |
| `core/xml-serialize-unedited` | 10k cells: sheet1.xml 298.0 KiB | 8.9 µs | 8.7 µs | 15 | 298.1 KiB | 34252.0 MB/s |
| `core/xml-serialize-rerender` | 10k cells: sheet1.xml 298.0 KiB | 2.65 ms | 2.42 ms | 15 | 298.1 KiB | 115.3 MB/s |
| `core/xml-clone` | 10k cells: sheet1.xml 298.0 KiB | 12.81 ms | 12.05 ms | 15 | 5.6 MiB | 23.8 MB/s |
| `core/raw-walk-all-cells` | 10k cells: sheet1.xml 298.0 KiB | 542.8 µs | 502.7 µs | 15 | 0 B | 18422109.5 cells/s |
| `core/package-passthrough/generated` | 100k cells, 355.5 KiB | 138.2 µs | 135.0 µs | 15 | 938.9 KiB | 2633.4 MB/s |
| `core/package-parse-sheet-passthrough` | 100k cells: open, parse sheet1.xml, save | 278.84 ms | 272.86 ms | 11 | 151.0 MiB |  |
| `core/xml-parse` | 100k cells: sheet1.xml 3.1 MiB | 277.34 ms | 243.36 ms | 11 | 147.5 MiB | 11.7 MB/s |
| `core/xml-serialize-unedited` | 100k cells: sheet1.xml 3.1 MiB | 94.7 µs | 87.1 µs | 15 | 3.1 MiB | 34394.2 MB/s |
| `core/xml-serialize-rerender` | 100k cells: sheet1.xml 3.1 MiB | 28.97 ms | 27.92 ms | 15 | 3.1 MiB | 112.5 MB/s |
| `core/xml-clone` | 100k cells: sheet1.xml 3.1 MiB | 140.23 ms | 117.63 ms | 15 | 56.0 MiB | 23.2 MB/s |
| `core/raw-walk-all-cells` | 100k cells: sheet1.xml 3.1 MiB | 30.13 ms | 27.12 ms | 15 | 0 B | 3318531.0 cells/s |
| `core/package-passthrough/generated` | 1000k cells, 3.4 MiB | 1.41 ms | 1.32 ms | 15 | 12.4 MiB | 2559.6 MB/s |
| `core/package-parse-sheet-passthrough` | 1000k cells: open, parse sheet1.xml, save | 3.42 s | 3.42 s | 1 | 1.48 GiB |  |
| `core/xml-parse` | 1000k cells: sheet1.xml 33.1 MiB | 2.74 s | 2.39 s | 3 | 1.44 GiB | 12.7 MB/s |
| `core/xml-serialize-unedited` | 1000k cells: sheet1.xml 33.1 MiB | 29.59 ms | 28.92 ms | 15 | 33.1 MiB | 1171.7 MB/s |
| `core/xml-serialize-rerender` | 1000k cells: sheet1.xml 33.1 MiB | 318.99 ms | 295.02 ms | 10 | 33.1 MiB | 108.7 MB/s |
| `core/xml-clone` | 1000k cells: sheet1.xml 33.1 MiB | 1.82 s | 1.81 s | 3 | 563.9 MiB | 19.1 MB/s |
| `core/raw-walk-all-cells` | 1000k cells: sheet1.xml 33.1 MiB | 334.29 ms | 289.21 ms | 10 | 0 B | 2991414.9 cells/s |
| `xlsx/open/ooxml` | 10k cells | 31.44 ms | 30.33 ms | 15 | 16.3 MiB | 318113.9 cells/s |
| `xlsx/read-all/calamine` | 10k cells | 6.58 ms | 5.87 ms | 15 | 882.9 KiB | 1519069.1 cells/s |
| `xlsx/read-all/ooxml-raw-walk` | 10k cells (open + Package raw tree walk, no public API) | 28.01 ms | 23.50 ms | 15 | 15.0 MiB | 357031.2 cells/s |
| `xlsx/read-all/ooxml-cell-api` | 10k cells (open + Worksheet::cell for every cell) | 1.32 s | 1.26 s | 3 | 16.3 MiB | 7586.9 cells/s |
| `xlsx/open/ooxml` | 100k cells | 369.29 ms | 339.05 ms | 9 | 165.0 MiB | 270790.0 cells/s |
| `xlsx/read-all/calamine` | 100k cells | 63.79 ms | 61.68 ms | 15 | 8.8 MiB | 1567590.5 cells/s |
| `xlsx/read-all/ooxml-raw-walk` | 100k cells (open + Package raw tree walk, no public API) | 348.19 ms | 314.47 ms | 9 | 151.0 MiB | 287199.4 cells/s |
| `xlsx/read-all/ooxml-cell-api` | 100k cells | skipped: lower bound ≈ 131.81 s (quadratic extrapolation from 10k cells; Worksheet::cell scans every row) | | | | |
| `xlsx/open/ooxml` | 1000k cells | 3.51 s | 3.51 s | 1 | 1.61 GiB | 284725.5 cells/s |
| `xlsx/read-all/calamine` | 1000k cells | 645.60 ms | 374.14 ms | 6 | 77.2 MiB | 1548952.2 cells/s |
| `xlsx/read-all/ooxml-raw-walk` | 1000k cells (open + Package raw tree walk, no public API) | 3.33 s | 3.33 s | 1 | 1.48 GiB | 300460.4 cells/s |
| `xlsx/read-all/ooxml-cell-api` | 1000k cells | skipped: lower bound ≈ 13180.59 s (quadratic extrapolation from 10k cells; Worksheet::cell scans every row) | | | | |
| `xlsx/edit-save/ooxml` | 10k cells, 100 edits | 49.11 ms | 47.70 ms | 15 | 16.7 MiB | 2036.1 edits/s |
| `xlsx/edit-save/umya` | 10k cells, 100 edits | 40.99 ms | 30.16 ms | 15 | 3.5 MiB | 2439.5 edits/s |
| `xlsx/edit-save/ooxml` | 10k cells, 1000 edits | 98.05 ms | 68.06 ms | 15 | 17.0 MiB | 10198.6 edits/s |
| `xlsx/edit-save/umya` | 10k cells, 1000 edits | 52.29 ms | 48.71 ms | 15 | 3.5 MiB | 19124.4 edits/s |
| `xlsx/edit-save/ooxml` | 100k cells, 100 edits | 470.74 ms | 387.95 ms | 7 | 166.2 MiB | 212.4 edits/s |
| `xlsx/edit-save/umya` | 100k cells, 100 edits | 562.38 ms | 489.14 ms | 6 | 29.3 MiB | 177.8 edits/s |
| `xlsx/edit-save/ooxml` | 100k cells, 1000 edits | 1.05 s | 1.05 s | 3 | 166.3 MiB | 947.9 edits/s |
| `xlsx/edit-save/umya` | 100k cells, 1000 edits | 600.66 ms | 578.48 ms | 6 | 29.3 MiB | 1664.8 edits/s |
| `xlsx/edit-save/ooxml` | 1000k cells, 100 edits | 6.88 s | 6.88 s | 1 | 1.61 GiB | 14.5 edits/s |
| `xlsx/edit-save/umya` | 1000k cells, 100 edits | 6.81 s | 6.81 s | 1 | 300.3 MiB | 14.7 edits/s |
| `xlsx/edit-save/ooxml` | 1000k cells, 1000 edits | 29.30 s | 29.30 s | 1 | 1.61 GiB | 34.1 edits/s |
| `xlsx/edit-save/umya` | 1000k cells, 1000 edits | 6.72 s | 6.72 s | 1 | 300.3 MiB | 148.8 edits/s |
| `xlsx/write-new/rust_xlsxwriter` | 10k cells | 16.19 ms | 15.12 ms | 15 | 1.9 MiB | 617548.7 cells/s |
| `xlsx/write-new/rust_xlsxwriter-constant-memory` | 10k cells | 13.81 ms | 12.62 ms | 15 | 519.2 KiB | 724267.8 cells/s |
| `xlsx/write-new/ooxml` | 10k cells | 427.27 ms | 298.36 ms | 8 | 20.1 MiB | 23404.3 cells/s |
| `xlsx/write-new/rust_xlsxwriter` | 100k cells | 148.91 ms | 127.00 ms | 15 | 13.9 MiB | 671566.7 cells/s |
| `xlsx/write-new/rust_xlsxwriter-constant-memory` | 100k cells | 114.88 ms | 110.28 ms | 15 | 1.1 MiB | 870442.2 cells/s |
| `xlsx/write-new/ooxml` | 100k cells | 35.88 s | 35.88 s | 1 | 196.3 MiB | 2787.4 cells/s |
| `xlsx/write-new/rust_xlsxwriter` | 1000k cells | 1.60 s | 1.45 s | 3 | 170.7 MiB | 625509.4 cells/s |
| `xlsx/write-new/rust_xlsxwriter-constant-memory` | 1000k cells | 1.11 s | 991.39 ms | 3 | 5.6 MiB | 900699.8 cells/s |
| `xlsx/write-new/ooxml` | 1000k cells | skipped: lower bound ≈ 3587.52 s (quadratic extrapolation from 100k cells; each write scans every row) | | | | |
| `pptx/open` | 23 pptx fixtures, 2.0 MiB | 3.94 ms | 3.63 ms | 15 | 751.5 KiB |  |
| `pptx/read-all-text` | 23 pptx fixtures, 2.0 MiB (open + every slide's shapes' text) | 18.43 ms | 17.56 ms | 15 | 4.0 MiB |  |
| `pptx/set-text-all-save` | 23 pptx fixtures, 2.0 MiB (open + set_text on every text body + save) | 28.32 ms | 27.18 ms | 15 | 4.7 MiB |  |
| `pptx/open` | generated deck: 100 slides × 10 shapes × 5 paragraphs, 147.3 KiB | 788.5 µs | 610.2 µs | 15 | 386.6 KiB |  |
| `pptx/read-all-text` | generated deck: 100 slides × 10 shapes × 5 paragraphs, 147.3 KiB (open + every slide's shapes' text) | 35.39 ms | 31.35 ms | 15 | 21.2 MiB |  |
| `pptx/set-text-all-save` | generated deck: 100 slides × 10 shapes × 5 paragraphs, 147.3 KiB (open + set_text on every text body + save) | 42.79 ms | 35.38 ms | 15 | 9.2 MiB |  |
| `pptx/open` | generated deck: 1000 slides × 10 shapes × 5 paragraphs, 1.2 MiB | 17.83 ms | 15.12 ms | 15 | 3.1 MiB |  |
| `pptx/read-all-text` | generated deck: 1000 slides × 10 shapes × 5 paragraphs, 1.2 MiB (open + every slide's shapes' text) | 532.78 ms | 361.65 ms | 6 | 210.7 MiB |  |
| `pptx/set-text-all-save` | generated deck: 1000 slides × 10 shapes × 5 paragraphs, 1.2 MiB (open + set_text on every text body + save) | 625.24 ms | 546.31 ms | 5 | 86.9 MiB |  |

## Hotspots

The tables below come from `cargo bench -p ooxml-bench --bench breakdown`, in the same session as
the results. Timings exclude drop unless a column says otherwise.

### Type sizes

| Type | Bytes |
|---|---|
| `RawNode` | 144 |
| `RawItem` | 144 |
| `RawAttr` | 72 |
| `RawText` | 56 |
| `XName` | 48 |

### XML tree cost (sheet1.xml)

| Cells | XML | Elements | Attributes | Text nodes | Allocations (+ reallocations) | Heap after parse | Heap / XML byte | Allocs / element (+ reallocs) | quick-xml events only | `XmlDocument::parse` (no drop) | drop | parse / events | drop / parse |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 10000 | 298.0 KiB | 21007 | 13012 | 10000 | 111064 (+ 2015) | 14.6 MiB | 50.3 | 5.29 (+ 0.10) | 4.10 ms | 16.21 ms | 7.53 ms | 4.0× | 46% |
| 100000 | 3.1 MiB | 210007 | 130012 | 100000 | 1110064 (+ 20019) | 147.5 MiB | 47.5 | 5.29 (+ 0.10) | 40.55 ms | 170.97 ms | 102.23 ms | 4.2× | 60% |
| 1000000 | 33.1 MiB | 2100007 | 1300012 | 1000000 | 11100064 (+ 200022) | 1.44 GiB | 44.6 | 5.29 (+ 0.10) | 204.80 ms | 1.15 s | 921.94 ms | 5.6× | 80% |

### `Workbook::from_bytes` phases

All columns exclude dropping the result; the last column is the drop of the opened `Workbook`.

| Cells | `Package::from_bytes` | decompress sheet1.xml | parse sheet1.xml | quick-xml scan of sheet1.xml | `Workbook::from_bytes` total | drop of the `Workbook` |
|---|---|---|---|---|---|---|
| 10000 | 46.2 µs | 464.6 µs | 15.73 ms | 2.85 ms | 23.27 ms | 7.90 ms |
| 100000 | 160.6 µs | 4.58 ms | 156.65 ms | 29.18 ms | 247.62 ms | 107.63 ms |
| 1000000 | 494.4 µs | 47.73 ms | 1.25 s | 291.37 ms | 1.95 s | 1.20 s |

### Per-call cost of the cell API (after open)

| Cells (rows) | `cell(0, 0)` | `cell(last row, 0)` | `cell_mut(last row, 1).set_number` (one edit) | `to_bytes` after one edit | of which: serialize sheet1.xml | of which: deflate sheet1.xml (zip default level) | deflate at level 1 |
|---|---|---|---|---|---|---|---|
| 10000 (1000) | 71.6 µs | 167.8 µs | 60.2 µs | 10.24 ms | 20.6 µs | 9.81 ms | 660.8 µs |
| 100000 (10000) | 1.77 ms | 2.08 ms | 565.5 µs | 97.90 ms | 537.6 µs | 93.96 ms | 6.62 ms |
| 1000000 (100000) | 20.27 ms | 44.43 ms | 20.37 ms | 1.06 s | 28.99 ms | 1.07 s | 68.50 ms |

### From-scratch writes: cost of one row (10 cells) after k rows (single measurement each)

| k (rows already written) | one more row |
|---|---|
| 100 | 121.1 µs |
| 1000 | 745.8 µs |
| 5000 | 3.67 ms |
| 10000 | 7.31 ms |

### PowerPoint: 1,000 generated slides

| Step | Time | Note |
|---|---|---|
| `Presentation::from_bytes` | 31.43 ms | |
| parse all 1,000 slides (`slide(i)`, first call) | 371.18 ms | 11.9 MiB of slide XML; 210.7 MiB retained in the package after parsing (18× the XML) |
| `shapes()` on every slide (already parsed) | 14.81 ms | 10004 shapes |
| `shapes()` + `text_body().text()` on every slide | 63.84 ms | 3474909 bytes of text; transient heap peak 2.3 KiB |

### 1. The Layer 2 tree: allocation-bound, 45–50× the XML size

- **Evidence.**
  - `XmlDocument::parse`, drop excluded, takes 4.0–5.6× as long as a bare quick-xml event pass
    over the same bytes: 1.15 s against 0.20 s for 33 MiB.
  - Dropping the tree takes a further 46–80% of the parse time; at 1M cells, 0.92 s against
    1.15 s.
  - Each element makes 5.29 fresh allocations plus 0.10 reallocations (a growing `children`
    `Vec`):
    - an `XName` with a boxed `local`, and a boxed `prefix` when there is one;
    - an `attrs` `Vec` and a `children` `Vec`;
    - per attribute, another `XName` and a `String` value;
    - per text node, a `String`.
  - `RawNode` and `RawItem` are 144 B, `RawAttr` 72 B, `XName` 48 B.
  - A 1M-cell sheet needs 1.44 GiB to hold, against 77 MiB for calamine's whole read.
  - `XmlDocument::clone` runs at about 20 MB/s, drop included. xlsx Phase1 clones `workbook.xml`
    on each edit, but never a sheet.
- **Suggestions.**
  1. **Intern names.** Make `local` and `prefix` `Arc<str>` or a `u32` symbol from the parser's
     existing interner, as `ns` already is.
  2. **Keep untouched attribute values and text as spans** into the source, materialized only
     when edited. The source is already kept for T1.
  3. **Smaller nodes.** Use `u32` spans, an exact-size attribute slice, and exact-size children.
  4. **Do not build what is not read.** For the worksheet this is the lazy row-level `sheetData`
     that Phase6 already plans: index rows by byte span, and materialize a row only when it is read
     or written.

  Only suggestion 4 can meet the Phase6 memory goal. Suggestions 1–3 reduce the tree several-fold
  but leave it orders of magnitude above calamine's 77 MiB; they matter for every other part
  (pptx, docx).
- ADR-0010 designs all four. Its spike measures 1–3 at 2.6× fewer allocations and 5× less heap
  on sheets.

### 2. The xlsx cell API is O(rows) per call, so full reads and from-scratch writes are quadratic

- **Evidence.**
  - `Worksheet::cell(last row, 0)` costs 0.17 ms at 1k rows, 2.1 ms at 10k and 44 ms at 100k.
  - `cell(0, 0)` already costs 20 ms at 100k rows, because `locate` first checks the order of
    every row.
  - Writing one more row of 10 cells costs 0.12 ms after 100 rows and 7.3 ms after 10,000 (single
    measurements).
  - `Worksheet::locate` (`crates/ooxml-xlsx/src/worksheet.rs`) iterates every `row` of `sheetData`
    and parses its `r` to check ascending order. It then scans the target row's cells, and `cell`
    walks the rows a second time through the typed lens (`children::<Row>().find(ptr::eq)`). Each
    write also re-checks the dimension, plans the edit, and re-walks the `LensMut` path from the
    root.
- **Suggestions.**
  1. **Row index** (`row number → child index`), built once and validated then.
     - It must live on `SheetContext` or `Workbook`, not on `Worksheet`. A `Worksheet` is a
       short-lived borrow created by every `Workbook::worksheet()` call, and the tree belongs to the
       `Package`.
     - Writes that insert rows must update or invalidate the index.
     - `locate` then costs O(log rows) or O(1), plus the target row's cells.
  2. **Append fast path** for from-scratch writes: when the target row is past the last row, push
     the row and cell directly.
  3. **A full-read iterator** (`Worksheet::rows()` / `cells()`), one pass over `sheetData`.
  4. **A streaming from-scratch writer** (Phase6 plan).

### 3. Save: default-level deflate of the rewritten part

- **Evidence.** `to_bytes` after one edit of the 1M-cell sheet takes 1.06 s. Serializing
  sheet1.xml takes 29 ms; untouched siblings are copied verbatim. Deflating the 33 MiB result at
  zip's default level takes 1.07 s, against 69 ms at level 1 (15× faster). Unedited parts are
  raw-copied.
- **Suggestions.**
  1. A configurable compression level for rewritten parts (e.g. `SaveOptions`) with a faster
     default. ADR-0010's spike measures level 3 at about 5× the speed of the default, with output
     within 3% of the size.
  2. Parallel deflate for very large parts.

### 4. Open: the tree and its census

- **Evidence.** For 1M cells, `Workbook::from_bytes` takes 1.95 s with drop excluded:
  - parsing sheet1.xml takes 1.25 s;
  - the formula census, a second quick-xml scan of the same bytes, takes 0.29 s;
  - decompression takes 48 ms and the zip directory 0.5 ms.

  Dropping the opened `Workbook` costs another 1.2 s.
- **Suggestions.**
  1. Count formulas during the parse (one pass).
  2. With a lazy `sheetData`, open becomes a single scan.

### 5. pptx: every parsed slide stays in memory

- **Evidence.**
  - Reading the text of 1,000 slides parses each slide once in 371 ms and keeps all of them in the
    `Package`: 211 MiB retained for 11.9 MiB of slide XML (18×).
  - The shape walk and `text()` take 15 ms and 64 ms for 10,000 shapes.
- **Suggestions.**
  1. A non-caching read path in core, which parses on demand without keeping the tree.
  2. Release clean trees back to `Raw`.
  3. Editing paths keep only the slides actually edited.

### Not hotspots

- Package passthrough.
- Serializing an unedited tree, which copies the source: 1.2–34 GB/s.
- Serializing after `force_rerender`: about 110 MB/s.
- The pptx shape walk.

## Reproducing

```sh
cargo bench -p ooxml-bench --bench baseline     # results table, about 3–6 min on this host
cargo bench -p ooxml-bench --bench breakdown    # hotspot tables, about 1–2 min
cargo bench -p ooxml-bench --bench baseline -- xlsx/edit-save   # one family
```

Run both benches in one session and quote ratios from the same run.
