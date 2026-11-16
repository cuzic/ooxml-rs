# Spike: compact Layer 2 tree and lazy `sheetData` (ADR-0010)

This is a standalone crate with its own `[workspace]`. The main workspace never builds it. It
depends on `ooxml-core` by path, for comparison only, and changes no crate.

```sh
cd spikes/core-performance && cargo run --release
```

## What it prototypes

- **`src/compact.rs`** is a Layer 2 tree with the same observable model as
  `ooxml_core::XmlDocument`. Names are resolved and keep their original prefix. Attributes stay
  in order, `xmlns` declarations included. Text is merged across entity references, comments and
  PIs are kept verbatim, element spans are recorded, and serialization follows the same rule. It
  changes three things:
  1. **Interned names.** `Name` is one thin `Arc` holding `(ns, local, prefix)`, interned per
     parse, so elements and attributes allocate nothing for their names.
  2. **Span-backed values.** Attribute values, text and markup are `SStr`: an `Arc<str>` of the
     whole source plus a `u32` range. Only a value whose decoded form differs from its source
     bytes gets an owned copy: entities, or line-ending or attribute-whitespace normalization.
     `as_str()` still returns `&str`, so core's accessor signatures (`attr() -> Option<&str>`,
     `RawText::value() -> &str`) can stay as they are. The `Arc` also keeps a value valid in a
     subtree copied to another document (`detach_as`, PR #18).
  3. **Smaller nodes and exact-size vectors.** Spans are `u32` offsets. Attributes go in a boxed
     slice. Children collect in per-depth scratch vectors and move into an exact-capacity `Vec`
     once the element closes.
- **`src/rows.rs`** is a lazy `sheetData`. A quick-xml scan, with no tree, records
  `(r, start, end)` for each `row`. A row is built from its source slice only when it is read or
  edited. Saving splices the re-rendered rows into the original bytes.

## Correctness checks (run on every start)

These cover every XML part of every fixture under `reference/fixtures/` (docx, xlsx and pptx,
1,589 parts that core accepts), plus three adversarial inputs: entities, CDATA, a comment, a PI,
a default namespace, a BOM, CRLF, and `xmlns=""`.

- **T1:** the compact tree serializes, unedited, to exactly the input bytes.
- **Same tree:** same names (namespace, local, prefix), attributes, child items, text values and
  markup as core's `RawNode`.
- **T1b equivalence:** after `force_rerender`, the compact serialization is byte-identical to
  core's.
- **Lazy sheet:** 1,000 rows of the 1M-cell sheet are edited. The output parses with core, every
  edited cell reads back the new value, and every byte outside the edited rows' spans is
  unchanged (T2).

## Results (2026-11-16, release, rustc 1.99.1)

These were measured on the same shared AMD EPYC 7313P as `docs/testing/performance-baseline-2026-11.md`, with a 1-minute load average of 38–50 during the run, so times are noisy.

- **Parse** excludes dropping the tree. The value is dropped after the clock stops, and **Drop** is measured on its own (review of PR #24, M1).
- Times are the best of 2–5 runs.
- Allocation counts and heap sizes are exact. Fresh allocations and reallocations are counted separately.

| Input | XML | Tree | Parse (no drop) | Allocations (+ reallocations) | Allocs/element (+ reallocs) | Heap retained | Heap/XML byte | Drop | Unedited serialize | Full re-render |
|---|---|---|---|---|---|---|---|---|---|---|
| sheet1.xml, 10k cells | 0.3 MiB | core `XmlDocument` | 15.4 ms | 111064 (+ 2015) | 5.29 (+ 0.10) | 14.6 MiB | 50.3 | 6.4 ms | 0.0 ms | 2.7 ms |
| | | compact (spike) | 13.7 ms | 43115 (+ 21) | 2.05 (+ 0.00) | 2.8 MiB | 9.7 | 1.5 ms | 0.0 ms | 2.5 ms |
| sheet1.xml, 100k cells | 3.1 MiB | core `XmlDocument` | 223.0 ms | 1110064 (+ 20019) | 5.29 (+ 0.10) | 147.5 MiB | 47.5 | 95.6 ms | 0.1 ms | 27.1 ms |
| | | compact (spike) | 133.4 ms | 430115 (+ 25) | 2.05 (+ 0.00) | 28.4 MiB | 9.1 | 19.1 ms | 0.1 ms | 30.6 ms |
| sheet1.xml, 1000k cells | 33.1 MiB | core `XmlDocument` | 2840.8 ms | 11100064 (+ 200022) | 5.29 (+ 0.10) | 1474.6 MiB | 44.6 | 1304.3 ms | 60.4 ms | 549.1 ms |
| | | compact (spike) | 1493.1 ms | 4300115 (+ 28) | 2.05 (+ 0.00) | 285.6 MiB | 8.6 | 200.1 ms | 12.2 ms | 363.9 ms |
| pptx slide (ppt/slides/slide12.xml) | 0.0 MiB | core `XmlDocument` | 0.4 ms | 2564 (+ 88) | 4.89 (+ 0.17) | 0.2 MiB | 16.8 | 0.1 ms | 0.0 ms | 0.1 ms |
| | | compact (spike) | 0.4 ms | 1493 (+ 51) | 2.85 (+ 0.10) | 0.1 MiB | 5.9 | 0.0 ms | 0.0 ms | 0.1 ms |
| largest fixture word/document.xml | 1.3 MiB | core `XmlDocument` | 31.1 ms | 319805 (+ 1345) | 8.69 (+ 0.04) | 15.2 MiB | 11.8 | 12.8 ms | 0.1 ms | 10.1 ms |
| | | compact (spike) | 39.2 ms | 153825 (+ 166) | 4.18 (+ 0.00) | 5.6 MiB | 4.3 | 2.6 ms | 0.1 ms | 11.1 ms |

Heap retained includes the source text: core keeps a `Box<str>` copy, compact an `Arc<str>`.

Node sizes:

| | core | compact |
|---|---|---|
| Node (`RawNode` / `Node`) | 144 B | 72 B |
| Item (`RawItem` / `Item`) | 144 B | 72 B |
| Attribute (`RawAttr` / `Attr`) | 72 B | 32 B |
| Name (`XName` / `Name`) | 48 B | 8 B |
| `SStr` | — | 24 B |

**Lazy `sheetData`** (1M cells, 100k rows):

| Step | Time | Heap | Allocations |
|---|---|---|---|
| Index the rows (quick-xml scan, no tree) | 344 ms (164 ms in an earlier run at load ≈ 30) | 1.5 MiB | 200,031 |
| Find, materialize, edit and re-render 1,000 rows | 22 ms | — | — |
| Splice into the source (save, before deflate) | 4.1 ms | — | — |

**Deflate level for a rewritten part** (the re-rendered 33.1 MiB sheet):

| Level | Time | Compressed size |
|---|---|---|
| 1 | 72 ms | 5.8 MiB |
| 3 | 217 ms | 3.3 MiB |
| 6 (zip default, today) | 1,079 ms | 3.2 MiB |
| 9 | 1,709 ms | 3.1 MiB |

For comparison, the 2026-11 baseline (run 3) measures the same workload on today's tree end to end:

- Open: 3.5 s, peak heap 1.61 GiB, drop included.
- 1,000 edits: about 20 ms each.

## Reading the numbers

- **Sheets.**
  - About 5× less memory: 8.6–9.7 heap bytes per XML byte instead of 45–50.
  - 2.6× fewer fresh allocations, and essentially no reallocations.
  - Parsing is 1.1–1.9× faster with drop excluded; drop is 4–6.5× faster.
  - For the 1M-cell sheet, parse plus drop goes from 4.1 s to 1.7 s (2.4×).
- **Text-heavy documents.** The largest fixture `word/document.xml` needs 2.7× less memory. Its parse is no faster in this run (31 ms core, 39 ms compact); that difference is within this host's noise. Its remaining allocations are children vectors and attribute slices, one each per element, and the arena for children sketched as ADR-0010 stage 4 would remove most of them.
- **The lazy sheet is the real step change for large sheets.** Indexing 100k rows needs 1.5 MiB and one scan, against 1.47 GiB for core's tree, and an edit materializes only the rows it touches.
- **Not measured here.**
  - Lens mutation on the compact tree. A changed value becomes an owned `SStr`; the dirty flags are unchanged.
  - Atomic reference-count traffic under threads.
  - The full validation (depth, entities, prefixes) that ADR-0010 requires during the lazy skip. The spike's index scan does not do it yet.
