# round_trip_corpus

Fixtures used by ooxml-core's round-trip idempotence tests
(load an existing OOXML package, save it unchanged, verify untouched parts
survive byte-for-byte).

## fixture_minimal.xlsx

**Not Excel-authored.** This sandbox has no Office/Excel, and LibreOffice's
headless `--convert-to` conversion fails to spawn here (and openpyxl isn't
installed), so this file was hand-authored directly from the ECMA-376 /
OOXML part structure by
`scratchpad/make_fixture.py` (kept out of the repo; regenerate from the
plan/session if needed, or just hand-edit this file's XML parts directly).

It deliberately includes things a naive "fully remodel into our own types,
then regenerate everything" implementation tends to drop on save:

- a shared string table (`xl/sharedStrings.xml`) with indices referenced
  from two different cells
- a `styles.xml` with two `cellXfs` entries (default + bold), so cell `A1`
  renders bold via `s="1"`
- a boolean cell (`t="b"`) and a plain numeric cell alongside the string
  cells, to exercise all the basic `CellType`s at once
- an `<extLst>` with a **foreign namespace** (`x14:customXmlExtension`)
  inside `sheet1.xml`, standing in for vendor extensions (conditional
  formatting extensions, rich values, etc.) that ooxml-core must never be
  required to understand in order to preserve them
- a **self-closing, styled, valueless cell** (`D2`, `<c r="D2" s="1"/>`)
  — real Excel writes these constantly for "formatted but empty" cells.
  Added after ADR-0001 round 2 review (R2-B2) found that writing a value
  into exactly this shape of cell is the first thing the from-scratch
  write path does (an empty templated sheet is nothing but these), and a
  span representation that can't turn a self-closing tag into an
  open/close pair without corrupting the XML would silently break on it.

**Still not covered** (see ADR-0001's Scope boundary and round 2 review,
R2-M1): shared/array formulas, `mc:Ignorable`, `xml:space="preserve"`
text, zip entries using a data descriptor, `<row spans="...">` hints, and
a `<sheetData/>` sheet with *zero* rows (the shape the from-scratch
path's embedded template will actually start from).

**TODO**: replace with (or add alongside) a genuinely Excel-authored and/or
LibreOffice-authored `.xlsx`, once one is available, so the round-trip
tests aren't only ever checked against a file this project's own
understanding of the spec produced. A real third-party-authored file is
the more trustworthy test — it can disagree with our assumptions in ways a
self-authored fixture structurally cannot.
