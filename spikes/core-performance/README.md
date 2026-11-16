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

The host was the same shared AMD EPYC 7313P as `docs/testing/performance-baseline-2026-11.md`, with
a 1-minute load average of 26–44 during the run. Times are best-of-N and only roughly comparable;
allocation counts and heap sizes are exact.

| Input | XML | Tree | Parse | Allocations | Allocs/element | Heap retained | Heap/XML byte | Drop | Unedited serialize | Full re-render |
|---|---|---|---|---|---|---|---|---|---|---|
| sheet1.xml, 10k cells | 0.3 MiB | core `XmlDocument` | 12.3 ms | 113,079 | 5.38 | 14.6 MiB | 50.3 | 4.2 ms | 0.0 ms | 1.4 ms |
| | | compact (spike) | 8.0 ms | 43,136 | 2.05 | 2.8 MiB | 9.7 | 0.8 ms | 0.0 ms | 1.6 ms |
| sheet1.xml, 100k cells | 3.1 MiB | core `XmlDocument` | 160.4 ms | 1,130,083 | 5.38 | 147.5 MiB | 47.5 | 69.2 ms | 0.1 ms | 16.0 ms |
| | | compact (spike) | 84.7 ms | 430,140 | 2.05 | 28.4 MiB | 9.1 | 14.4 ms | 0.2 ms | 17.5 ms |
| sheet1.xml, 1M cells | 33.1 MiB | core `XmlDocument` | 1,782 ms | 11,300,086 | 5.38 | 1,474.6 MiB | 44.6 | 817.8 ms | 21.0 ms | 190.4 ms |
| | | compact (spike) | 902 ms | 4,300,143 | 2.05 | 285.6 MiB | 8.6 | 155.7 ms | 8.2 ms | 162.2 ms |
| pptx slide (POI `bug62513`, slide12) | 12 KiB | core `XmlDocument` | 0.2 ms | 2,652 | 5.06 | 0.2 MiB | 16.8 | 0.0 ms | 0.0 ms | 0.0 ms |
| | | compact (spike) | 0.3 ms | 1,544 | 2.95 | 0.1 MiB | 5.9 | 0.0 ms | 0.0 ms | 0.1 ms |
| largest fixture `word/document.xml` | 1.3 MiB | core `XmlDocument` | 24.5 ms | 321,150 | 8.72 | 15.2 MiB | 11.8 | 8.8 ms | 0.1 ms | 5.4 ms |
| | | compact (spike) | 23.3 ms | 153,991 | 4.18 | 5.6 MiB | 4.3 | 1.7 ms | 0.1 ms | 6.0 ms |

Heap retained includes the source text: core keeps a `Box<str>` copy and compact keeps an
`Arc<str>`.

Node sizes:

| Type | core | compact |
|---|---|---|
| element node (`RawNode` / `Node`) | 144 B | 72 B |
| child item (`RawItem` / `Item`) | 144 B | 72 B |
| attribute (`RawAttr` / `Attr`) | 72 B | 32 B |
| name (`XName` / `Name`) | 48 B | 8 B |
| value string (`SStr`, compact only) | — | 24 B |

**Lazy `sheetData`** (1M cells, 100k rows):

| Step | Time | Heap | Allocations |
|---|---|---|---|
| Index rows (quick-xml scan, no tree) | 163.5 ms | 1.5 MiB | 200,031 |
| Find, build, edit and re-render 1,000 rows | 13.3 ms | — | — |
| Splice into the source (save, before deflate) | 3.6 ms | — | — |

**Deflate level for a rewritten part** (the 33.1 MiB re-rendered sheet):

| Level | Time | Compressed size |
|---|---|---|
| 1 | 71.4 ms | 5.8 MiB |
| 3 | 202.1 ms | 3.3 MiB |
| 6 (zip default, today) | 983.3 ms | 3.2 MiB |
| 9 | 1,635.0 ms | 3.1 MiB |

For comparison, the 2026-11 baseline measured the same workload end to end on today's tree: open
1.21 s at 1.61 GiB, then 1,000 edits that cost about 22 ms each.

## Reading the numbers

- **Sheets.** The compact tree uses about 5× less memory (8.6–9.7 heap bytes per XML byte instead
  of 45–50) and 2.6× fewer allocations. It parses 1.5–2.0× faster and drops about 5× faster.
- **Text-heavy documents** gain less memory (2.7×) and about the same parse time. Their
  allocations are dominated by children vectors and attribute slices, which the spike still
  allocates once per element. An arena for children, sketched in ADR-0010 as stage 4, would remove
  most of the rest.
- **The lazy sheet is the real step change for large sheets.** Indexing 100k rows costs 1.5 MiB
  and one scan at about 200 MB/s, against 1.47 GiB for core's tree. An edit then only builds the
  rows it touches.
- **Not measured here:**
  - Lens mutation on the compact tree. A changed value becomes an owned `SStr`, and the dirty
    flags work as today.
  - Atomic reference-count traffic under threads.
  - The cost of re-validating row order while indexing (one comparison per row).
