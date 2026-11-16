# ooxml-rs

Generate and edit Excel/Word/PowerPoint files (OOXML: zip of XML parts)
from Rust, without Office installed and without driving a running Office
process.

## Why

Two well-regarded single-purpose crates already cover half of this each
for Excel: [`rust_xlsxwriter`](https://github.com/jmcnamara/rust_xlsxwriter)
(write-only, from-scratch generation) and
[`calamine`](https://github.com/tafia/calamine) (read-only parsing).
ooxml-rs targets that same level of maturity, but for a capability neither
of those — nor most round-trip-capable editors in any language — actually
deliver: opening an existing file written by real Office, changing a few
things, and saving it back out **without corrupting or dropping the parts
that weren't touched** (charts, pivot caches, VBA, styles the API doesn't
model yet, ...). That failure mode (fully deserialize into an internal
model, fully regenerate, silently lose anything the model doesn't
represent) is a chronic, repeatedly-reported bug class in tools that take
the "full remodel" shortcut — see `openpyxl`'s own issue tracker for
charts and pivot tables destroyed on save.

This is a separate, independent project from
[`com-office-rs`](https://github.com/cuzic/com-office-rs) (typed,
late-binding Windows COM automation of a *running* Excel/Word/PowerPoint/
Outlook process) — same author, no shared code, complementary use case
(no Office/Windows required here, at the cost of not being able to do
anything only a live Office instance can do, like real recalculation).

## Status

**No implementation code exists yet.** What's here is the foundational
architecture decision — [ADR-0001](docs/adr/0001-lossless-round-trip-architecture.md),
**Accepted** after three rounds of independent adversarial review (see
the `opus-review-round*.md` files alongside it) — and a minimal hand-authored
test fixture (`reference/round_trip_corpus/`). The ADR is the place to
start reading: it lays out a three-layer design (OPC part-level
passthrough; a lossless raw-XML-tree-plus-typed-lens for parts that are
actually edited; append-only cross-part index registries) and a
deliberately narrow Phase0 scope (Excel only, plain value edits to
existing cells, formula-free workbooks only) chosen specifically because
each of those boundaries survived three rounds of someone else trying to
break it.

## Layout

| Crate | Status | Role |
| --- | --- | --- |
| `ooxml-core` | not started | OPC package abstraction, lossless raw XML tree, shared XML-writer/escaping utilities — see ADR-0001. |
| `ooxml-xlsx` | not started | Excel: `Workbook`/`Worksheet`/`Cell`, covering both from-scratch generation and round-trip editing through the same `ooxml-core` foundation. |

Word (`ooxml-docx`) and PowerPoint (`ooxml-pptx`) are out of scope until
`ooxml-core`'s design is validated against Excel.

## Third-party test fixtures

Files under `reference/fixtures/**` are third-party test documents
(Apache POI, python-pptx, Open XML SDK, LibreOffice, calamine, …) kept
under **their own licences**, which are recorded next to them (each
directory's `README.md` and the licence files it names). They are not covered by
this repository's `MIT OR Apache-2.0` licence and are never packaged into a
published crate; tests read them by path.
