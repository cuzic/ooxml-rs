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

Phase0 is implemented for Excel, Word and PowerPoint on a shared foundation. Every phase is
designed in an ADR, reviewed in adversarial rounds, and then implemented. See
[`docs/adr/README.md`](docs/adr/README.md) for the ADR index and their status, and
[`docs/process.md`](docs/process.md) for how work is done. The design starts at
[ADR-0001](docs/adr/0001-lossless-round-trip-architecture.md), which sets out three layers:

- OPC part-level passthrough;
- a lossless raw XML tree with typed lenses for the parts that are edited;
- append-only cross-part registries.

Nothing is published to crates.io yet.

## Layout

| Crate | Status | What it does |
| --- | --- | --- |
| [`ooxml-core`](crates/ooxml-core/README.md) | implemented | **Layer 1:** OPC `Package`; untouched parts are copied with their original compressed bytes. **Layer 2:** lossless `XmlDocument`/`RawNode` tree; untouched elements are written back byte for byte; typed `Lens`/`LensMut` write primitives. **Layer 3:** content types and relationships, read-only. Refuses encrypted and Strict packages at open; refuses parsing a part that carries a DTD or nests deeper than 256 levels (unparsed parts are copied through unchanged). [ADR-0001](docs/adr/0001-lossless-round-trip-architecture.md), [ADR-0002](docs/adr/0002-schema-driven-code-generation.md) |
| [`ooxml-schema`](crates/ooxml-schema/README.md) | implemented (generated) | `ElementSpec` markers, `ChildOf` impls, enums and typed views, generated from the Open XML SDK data and the ECMA-376 XSDs for the allowlisted elements (`codegen/allowlist/`). [ADR-0002](docs/adr/0002-schema-driven-code-generation.md) |
| `xtask` | implemented | `cargo xtask codegen [--check]`: the schema code generator. `cargo xtask stats` reports on it. `cargo xtask manual-check <outdir>` builds the combined manual-check bundle. |
| [`ooxml-xlsx`](crates/ooxml-xlsx/README.md) | Phase0 | Opens Transitional, unencrypted workbooks. Reads cell values (number, string, bool, …) and writes numbers and strings on the first visible sheet. Creates a workbook from an embedded template. Refuses formulas, merged ranges, table headers and similar cases with a typed error. [ADR-0001](docs/adr/0001-lossless-round-trip-architecture.md), [`docs/roadmap/xlsx.md`](docs/roadmap/xlsx.md) |
| [`ooxml-docx`](crates/ooxml-docx/README.md) | Phase0 | Reads the body's paragraphs and runs. Replaces one run's text. Appends a paragraph. Refuses edits under track changes, protection or content-dependent fields, and edits inside fields, revisions or content controls. [ADR-0004](docs/adr/0004-docx-phase0.md) |
| [`ooxml-pptx`](crates/ooxml-pptx/README.md) | Phase0 | Walks slides and shapes and reads text. Edits text inside existing text bodies (shapes, group members, table cells). Refuses edits that would break animation or comment references. [ADR-0005](docs/adr/0005-pptx-phase0.md) |
| `ooxml-bench` | dev only | Performance baseline against calamine, rust_xlsxwriter and umya-spreadsheet. Not built by `cargo test`. [Report](docs/testing/performance-baseline-2026-11.md) |

Designs for later phases, and core changes still in review, are open PRs; the
[ADR index](docs/adr/README.md) links them.

## Building and testing

- **Toolchain:** Rust 1.99 or newer (MSRV 1.99, edition 2024); see `CLAUDE.md` for the API
  policy.
- **Checks** (as in CI, `.github/workflows/ci.yml`):

  ```sh
  cargo test --workspace --all-features
  cargo clippy --workspace --all-targets --all-features -- -D warnings
  cargo xtask codegen --check     # generated code and vendored inputs are up to date
  ```

- **Regenerating the schema code** after an allowlist or override change: `cargo xtask codegen`.
- **Manual checks in Microsoft 365**, which CI cannot run. Each writes edited files plus a
  checklist for the project owner:

  ```sh
  cargo run -p ooxml-xlsx --example xlsx_manual_check -- <outdir>
  cargo run -p ooxml-docx --example docx_manual_check -- <outdir>
  cargo run -p ooxml-pptx --example pptx_manual_check -- <outdir>
  # or all three at once, with a combined CHECKLIST.md index and a zip:
  cargo xtask manual-check <outdir>
  ```

- **Benchmarks:** `cargo bench -p ooxml-bench --bench baseline` (and `--bench breakdown`).

## Third-party test fixtures

Files under `reference/fixtures/**` are third-party test documents
(Apache POI, python-pptx, Open XML SDK, LibreOffice, calamine, …) kept
under **their own licences**, which are recorded next to them (each
directory's `README.md` and the licence files it names). They are not covered by
this repository's `MIT OR Apache-2.0` licence and are never packaged into a
published crate; tests read them by path.
