# ADR-0001: Lossless round-trip architecture for OOXML read/write/edit

- Status: **Accepted** (revision 3, confirmed converged by Opus round-3
  review — see `opus-review-round3.md`)
- Date: 2026-11-15

## Revision history

- **Revision 1** (original draft): proposed the two-layer design (OPC
  part-level passthrough; raw XML tree + typed lens for touched parts).
- **Revision 2** (after `opus-review-round1.md`): added the missing
  third layer (workbook-level index registries, B1), corrected the
  "shared foundation" claim for the from-scratch path (B2), fixed the
  self-contradictory Layer 2 serialization rule and added a test oracle
  (B3), and fixed the workspace's MSRV to match `zip`'s actual
  requirement (B4).
- **Revision 3** (this one, after `opus-review-round2.md`): revision 2's
  own fixes introduced three new blockers, all in the interplay between
  the new serialization rule and Phase0's scope — `span` unable to
  represent an empty-element tag growing a child (R2-B2), a dirty flag
  with two incompatible meanings that could silently drop an edit
  (R2-B3), and `add_worksheet()` reading as simultaneously in-scope and
  out-of-scope for Phase0 (R2-B1) — plus five Major issues: a trivial
  test oracle (R2-M1), an undefined locality check for newly-inserted
  nodes (R2-M2), silently stale formula-cache values on an untouched
  sheet (R2-M3), save-to-the-same-path corrupting the source before it's
  fully read (R2-M4), and no independent check that the bytes produced
  are valid Excel content rather than merely self-consistent (R2-M5).
  Every fix is folded directly into the sections below rather than
  listed separately; see the two review files for the full findings.

## Context

ooxml-rs is a new, independent Rust workspace that generates and edits
Excel/Word/PowerPoint files (OOXML: zip of XML parts) without driving a
running Office process (that's `com-office-rs`'s job, a separate repo).
Target maturity for the Excel crate is the level of two existing,
well-regarded single-purpose crates:

- `rust_xlsxwriter` — write-only, from-scratch generation, very complete
  feature coverage.
- `calamine` — read-only parsing.

The explicit, user-approved scope for this first phase goes further than
either of those: **Excel write is implemented from scratch (not wrapping
rust_xlsxwriter)**, and the library must also support **round-trip
editing** — open an existing `.xlsx` written by real Office, change a few
things, save it back out, without corrupting or dropping the parts that
weren't touched (charts, pivot caches, VBA, custom XML parts, styles the
API doesn't model yet, etc). The user explicitly declined wrapping
`rust_xlsxwriter`/`calamine` and bridging them (`既存クレートのラップでは
なく自前`) when scoping this phase — alternative 4 below records the
technical reasons that decision also holds up on its own.

Round-trip editing is the hardest of the three capabilities. A prior
research pass (three parallel investigations, reading the actual source
of `rust_xlsxwriter`, `calamine`, and several round-trip-capable editors)
found a consistent failure mode in the "wrap it all into our own object
model, then fully regenerate" approach used by `openpyxl`,
`umya-spreadsheet`-style crates, and most Word crates: anything the model
doesn't represent is silently destroyed on save. This is not a theoretical
risk — it's a chronic, repeatedly-reported bug class in exactly those
projects (openpyxl: charts wiped on save, pivot tables turned to plain
text, pivot-chart links broken; see Sources).

This design has gone through two rounds of Opus-model adversarial review
(see Revision history above); the sections below reflect the current,
revision-3 state, with each fix noted inline next to the design decision
it changed so the reasoning stays visible rather than being summarized
away.

## Decision

Round-trip fidelity is built in three layers — not two. The first draft
only had the outer two; the review's central finding (B1) is that a third,
narrower layer is required in between, and that Phase0 should sidestep
most of its hard cases rather than solve them yet.

### Layer 1 — OPC part-level passthrough (`ooxml_core::Package`)

An OOXML file is, before anything else, a zip archive of named parts. A
`Package` opened from an existing file keeps the **original compressed
bytes** of every part it does not re-derive, and only re-serializes parts
it actually changed.

Each part is tracked in one of three states (this replaces the original
draft's implicit binary "touched/untouched", which left "parsed but not
modified" undefined — Opus review M2):

- `Raw(bytes)` — never parsed. Written back via the `zip` crate's
  `ZipWriter::raw_copy_file`, which copies the still-compressed entry
  (name, compression method, CRC32, compressed payload) without
  decompressing or recompressing it.
- `Parsed(tree)` — parsed into a Layer 2 tree (below) but with **zero
  dirty nodes**. Also written back via `raw_copy_file` from the original
  entry — parsing for inspection must not, by itself, change the output.
  (This is only possible because the Layer 2 tree in this revision is
  read-only-safe; see Layer 2's `cell()`/`cell_mut()` split.)
- `Replaced(bytes)` — either a `Parsed` tree with at least one dirty
  node (re-serialized by Layer 2's own rules, see below) or a part
  written wholesale via `set_part`. Written via `start_file` + fresh
  compression.

**Passthrough guarantee, defined precisely** (the original draft's
"bit-identical" claim overstated what `raw_copy_file` actually gives —
Opus review M1): for a `Raw` or dirty-free `Parsed` part, the output zip
entry's **(name, compression method, CRC32, compressed payload bytes)**
match the input exactly. The surrounding zip container — local file
header fields, central-directory offsets, use of a data descriptor for
that entry, zip64 extra fields — is rewritten by `ZipWriter` and is
**not** claimed to be reproduced byte-for-byte. Container-level
reordering is also allowed: Phase0's `Package` preserves the original
part order, but this is a current behavior, not a load-bearing guarantee
other code should assume.

This single mechanism protects every part the typed API hasn't modeled
yet — charts, pivot caches, VBA, custom XML parts, theme — with no code
written for any of those formats specifically, as long as nothing else
needs to change their *contents*. It does not, by itself, protect parts
whose contents must change as a *side effect* of an edit somewhere else
(e.g. `[Content_Types].xml` when a new part is added) — that is Layer 3's
job.

Two more things have to hold for this to actually work for the most
common caller pattern — `Package::open("book.xlsx")`, edit,
`save("book.xlsx")` back to the *same path* — which round 2 review
(R2-M4) found the previous revision never addressed: `Package::open`
reads the entire source file into an in-memory buffer up front (not a
lazy file-backed reader), so the bytes `raw_copy_file` later copies from
can never be affected by what `save` does to the path on disk; and
`save` always writes to a fresh temporary file in the destination's
directory and renames it into place atomically at the end, rather than
truncating the destination path at the start. Either alone fixes the
same-path case; both together also mean a crash mid-save leaves the
original file intact.

### Layer 2 — raw XML tree + typed lens, for parts that *are* touched

A part a caller does modify (say, `xl/worksheets/sheet1.xml`) still isn't
safe to fully deserialize into a rigid struct and fully reserialize — that
regresses to the openpyxl failure mode *inside* that one part.

The original draft said non-dirty nodes serialize as "original source
bytes verbatim" while also saying only "structure", not bytes, survives
for untouched *siblings* of an edit, and defined `RawNode` without
anywhere to keep those bytes. Opus review B3 correctly called this
self-contradictory and untestable. This revision resolves it by actually
keeping the bytes:

```text
RawNode {
    name: QName,
    attrs: Vec<(String, String)>,   // order preserved
    children: Vec<RawXmlItem>,      // order preserved; comments/PIs included
    span: ElementSpan,              // see below — NOT a single byte range
    own_dirty: bool,                // this node's own name/attrs changed,
                                     // or it was newly created
    subtree_dirty: bool,            // this node or any descendant changed;
                                     // false => emit `span` verbatim, no
                                     // recursion needed
}

struct ElementSpan {
    start: usize,                   // '<' of the open tag
    open_end: usize,                // just past the open tag's final '>'
                                     // (or, if self-closing, past the `/>`)
    close: Option<(usize, usize)>,  // (start, end) of the close tag;
                                     // `None` iff the element is self-closing
}
```

A single `Range<usize>` (the original revision 2 shape) cannot represent
an empty-element tag (`<c r="B2" s="3"/>`) gaining children, because
there is no boundary between "open tag" and "close tag" to split at —
round 2 review, R2-B2, which also pointed out Phase0's from-scratch path
hits exactly this shape on its very first `write_number()` call: the
template's empty sheet has a self-closing `<sheetData/>` (round 3
review, R3-m2 — not self-closing `<c>` elements, there are no `<c>`
elements yet at all), and writing the first cell means that same
self-closing-element-gains-a-child transition happens to `<sheetData>`
first, then to the `<row>` it creates, before a `<c>` is reached.
`ElementSpan` fixes this by keeping the open tag's end and the close
tag's bounds (if any) separately.

Serialization rule, applied top-down from the part's root (rewritten in
this revision: round 2 review, R2-B3, found the previous `children_dirty`
flag used for two incompatible meanings at once — "the child *list*
changed" in its own definition vs. "some descendant is dirty" in the
rule that consumed it — which could silently drop an edit to a
grandchild; the two meanings are now two different, correctly-scoped
flags):

- If `!subtree_dirty`: emit the node's full span (`start` to `close.1`,
  or to `open_end` if self-closing) verbatim, with no recursion. This
  single check is what makes Layer 2 cheap for the parts of a sheet
  nobody touched — the common case.
- Otherwise (`subtree_dirty`): recurse. For this node's own tags:
  - if `own_dirty`: re-render the open tag from `name`/`attrs` (the only
    place "attribute quoting may normalize" legitimately applies — to a
    node a caller actually changed, never an untouched one); render a
    close tag if `children` is non-empty, a self-closing tag otherwise.
    **An element that was originally self-closing and gains its first
    child (or loses its last one) is always `own_dirty`** — `cell_mut`'s
    value-setting methods set this automatically, which is what makes
    the `<c r="B2" s="3"/>` case above well-formed instead of corrupt.
  - if `!own_dirty` (only a descendant is dirty): emit the original open
    tag span and close tag span verbatim — `xmlns:*` declarations,
    `mc:Ignorable` prefix lists, attribute order on this tag stay
    untouched even though something beneath it changed.
  - either way, then recurse into each child under the same rule: a
    child with `!subtree_dirty` emits verbatim (so a reordered/inserted
    child list still reuses verbatim spans for every child that didn't
    itself change); a dirty child recurses further. `subtree_dirty` is
    set on a node and on every ancestor up to the part's root as part of
    the write call that caused the change — it is never inferred
    afterwards by diffing child lists.

Text nodes carry an `xml:space="preserve"` flag inherited from their
parent; `preserve` text already in the tree is never touched by
normalization, only by an explicit value-changing edit (which marks it,
and every ancestor up to the root, dirty as above). Conversely, a write
that creates or rewrites a `<t>` must *set* `xml:space="preserve"` itself
whenever the new value has leading/trailing whitespace — otherwise a
conformant XML processor, Excel included, is allowed to normalize it
away on its own next save, silently changing the value later (round 2
review, R2-M5).

Three test oracles replace the original draft's single "shared string
table survives" check (which Opus round-1 review, B3, pointed out
doesn't exercise Layer 2 at all — an unedited part only ever goes
through Layer 1) and this revision's own first attempt at T1/T2 (which
round-2 review, R2-M1/R2-M2, found passes trivially when nothing is
edited, and is undefined for a brand-new node with no original span):

- **T1 (prolog/epilog preservation):** parse a part directly (not
  through `Package::open`/`save`, to bypass Layer 1's own passthrough),
  serialize it back with zero edits, assert the output is byte-identical
  to the input. This checks only that the XML declaration, any BOM, and
  any bytes outside the root element's span are preserved — on its own
  it is *not* a parser/renderer correctness check, since the "everything
  unedited" case only ever takes the verbatim-span branch above
  (round 2 review, R2-M1).
- **T1b (parser/renderer round-trip):** parse a part, force
  `own_dirty = true` and `subtree_dirty = true` on every node (as if
  every node had been edited, forcing full re-rendering from
  `name`/`attrs`/`children`), re-render, re-parse the result, and assert
  the re-parsed tree is structurally equal to the original one (same
  element names, same attribute keys/values/order, same child order and
  text). This is the oracle that actually exercises the parser and
  renderer, since T1 by itself cannot (round 2 review, R2-M1).
- **T2 (locality):** for every cell in the fixture (not just one — round
  2 review, R2-M1, pointed out a single cell barely exercises the
  empty-tag and insertion branches), write the same value back and then
  a different value, and assert each time that the byte-level diff
  against the input is confined to: the edited node's original span if
  it already existed, or its insertion point among its original
  siblings if it was newly created — and that the insertion point
  respects row/column ordering (round 2 review, R2-M2, on why "diff
  stays inside the edited `<c>`'s span" isn't even defined for a new
  node) — plus the `<dimension>` element's span, if that was also
  updated.

Reading must not cause writing, and merely *asking to write* must not
either. The original draft's single `Worksheet::cell(r, c)`
"find-or-create" method meant that *reading* a cell nobody had written
yet would create and dirty a node (Opus round-1 review, B3 point 6).
This revision splits the lens API, and — per round 2 review (R2-m1) —
defers node creation past even `cell_mut` itself to the point a value is
actually written (the same shape as `HashMap::entry`, so that
`let c = ws.cell_mut(5, 5)?;` with no further call does not insert an
empty, dirty `<c>` and mark the whole part `Replaced` for nothing):

- `cell(row, col) -> Option<CellRef<'_>>` — read-only, never mutates the
  tree.
- `cell_mut(row, col) -> CellMut<'_>` — returns a handle that *can*
  create the node but doesn't yet; only a value-setting call on that
  handle (`.set_string(..)`, `.set_number(..)`) creates the node if
  absent and sets `own_dirty`/`subtree_dirty` up to the root.

Non-local edits Phase0 *does* perform automatically (because skipping them
produces a file Excel visibly complains about, and they're cheap): the
`<dimension ref="...">` element is recomputed if an edit falls outside the
current range. Non-local edits Phase0 explicitly **refuses** (returns
`Err`, rather than silently producing a corrupt-looking-fine file) because
correctly handling them is real work deferred past this phase:

- writing into or reading-while-expecting-to-write a cell that is part of
  a shared formula group (`<f t="shared" si="...">`) or an array formula
  (`<f t="array">`),
- writing into a cell inside a merged range,
- any edit to a cell that currently holds, or would create, a `<f>`
  formula element at all (this also sidesteps `calcChain.xml`
  consistency — see Layer 3).

This mirrors this project's existing "fail honestly rather than silently
losing data" stance (see `com-office-rs`'s NotImplementedError convention
for the same reasons).

### Layer 3 (new in this revision) — workbook-level index registries

This is the layer Opus review B1 found missing entirely. Several OOXML
parts are not self-contained: a cell in `sheet1.xml` can reference a
numeric index into `sharedStrings.xml` or `styles.xml`, and some
operations ripple across still more parts (`calcChain.xml`,
`workbook.xml`, `[Content_Types].xml`, `.rels`). Layer 1's "didn't touch
it, pass it through" rule and Layer 3's job of keeping cross-references
valid are reconciled by one invariant:

> **Existing indices are append-only and are never renumbered or
> compacted.** A registry may add new entries at the end; it may never
> remove or renumber an existing one, even if it becomes unreferenced.

This is what lets Layer 1 keep passing through a part unchanged even
after *other* parts change: as long as nothing already-referenced ever
moves, "I didn't touch `styles.xml`" and "every existing `s="N"` attribute
elsewhere is still valid" are consistent statements at the same time. Each
registry is owned by one of:

- `ooxml-core::Package` — OPC-level registries: `[Content_Types].xml`
  and `_rels`. Owned here because they're format-agnostic (every OOXML
  document type has them).
- `ooxml-xlsx::Workbook` — SpreadsheetML-level registries: the shared
  string table, the style table, `calcChain.xml`'s existence.

**Phase0 policy** (deliberately conservative, to avoid needing any of
this machinery's harder cases yet):

- **New strings are written as `t="inlineStr"`** (`<c t="inlineStr">
  <is><t>...</t></is></c>`), never by appending to `sharedStrings.xml`.
  This is a format Excel accepts natively and round-trips through its own
  save (Excel will fold it into the shared string table on its next
  save, if it wants to) — so Phase0's edit path never needs to touch
  `sharedStrings.xml` at all, and the shared-string registry's append-only
  logic is simply not exercised yet.
- **No new style entries are created in Phase0's edit path.** Writing a
  value keeps the cell's existing `s="..."` attribute as-is. Value-based
  `Format` construction (the rust_xlsxwriter-style feature) is deferred —
  see "From-scratch path" below.
- **Formula cells are out of scope for writes in Phase0** (Layer 2's
  `Err` rule above) — but that alone isn't enough. Round 2 review
  (R2-M3) pointed out that editing a plain *value* cell on `sheet1.xml`
  can still silently leave a *formula* cell on an untouched `sheet2.xml`
  holding a stale cached result (e.g. `Sheet2!B1 = Sheet1!A1*2`'s cached
  `<v>`), because Phase0 never parses `sheet2.xml` to know it depends on
  the cell being edited, and Excel does not always force a full
  recalculation on open — so a reader (calamine, pandas, or this
  crate's own `cell()`) would get a wrong value back with no error at
  all, which directly contradicts the "fail honestly rather than
  silently losing data" stance this ADR otherwise holds to. Parsing
  every other sheet just to know whether it's *safe*, however, is a very
  different thing from parsing it into a Layer 2 tree — it only needs a
  yes/no answer, not an editable structure. Round 3 review (R3-M1) found
  that the revision-3 draft's actual check — "does the package contain a
  `calcChain.xml` part" — has false negatives: `calcChain.xml` is
  optional, and real producers that write formulas without it exist
  (`rust_xlsxwriter` doesn't emit one at all; `openpyxl` deletes it on
  save), so a value edit to a workbook from either could still leave a
  stale cached result on an unparsed sheet with no error. Phase0's actual
  policy is therefore: **decompress every worksheet part in the package
  (without building a Layer 2 tree — a cheap `quick-xml` event scan is
  enough) and check for the presence of any `<f` start element; if even
  one worksheet anywhere in the package contains a formula, every
  value-editing call returns `Err`.** This scan only inspects bytes, it
  never changes a part's state — an untouched worksheet stays `Raw` (or
  dirty-free `Parsed`) and is still copied through via `raw_copy_file`,
  exactly as the M2 "parsing for inspection must not change the output"
  rule already requires. The result is computed once at `Package::open`
  and cached; `calcChain.xml`'s presence may still be used as a cheap
  fast-path (if present, skip the scan and go straight to `Err`), but its
  *absence* no longer implies safety. With this fix, Phase0's "round-trip
  editing" is precisely: value edits are permitted when, and only when,
  no worksheet part in the workbook contains an `<f>` element at all —
  the same "formula-free workbooks" wording already used in Consequences,
  now actually matching what the check does. A future phase must pick,
  and write down, one of: delete `calcChain.xml` (+ its
  `.rels`/Content-Types entries) whenever a formula cell is touched
  (what `openpyxl` does), set `<calcPr fullCalcOnLoad="1"/>` in
  `workbook.xml` (which brings that part into Layer 2's scope), or
  incrementally maintain the dependency graph.
- **Adding a new worksheet to an *existing* package (one opened via
  `Package::open`), or any other edit that requires a new OPC part, is
  out of scope for Phase0.** (It needs `[Content_Types].xml`, `.rels`,
  and `workbook.xml`'s sheet list updated together — real work, not yet
  designed.) Round 2 review (R2-B1) found this flatly contradicted the
  from-scratch path's own `add_worksheet()` call below, as revision 2
  left it — nothing in that revision's text yet said `add_worksheet()`
  wasn't "add a worksheet to a package". Revision 3 resolves the
  contradiction, rather than explaining it away, by giving
  `add_worksheet()` the narrow rule in the From-scratch path section
  below: the embedded template already ships with exactly one worksheet,
  so Phase0's `add_worksheet()` only ever returns that existing sheet and
  never actually adds one.

### From-scratch path

The original draft claimed the from-scratch write path (no existing
file) is "just the degenerate case of `Package` with no original parts"
and shares rust_xlsxwriter's value-typed `Format`/shared-string
`Hash + Eq` dedup. Opus review B2 showed this doesn't actually hold: the
lens treats the tree as ground truth and resolves values immediately,
while rust_xlsxwriter resolves dedup indices once, immediately before
save — those are two different designs, and retrofitting dedup onto an
existing file's tables (whose entries may use features `Format` doesn't
model, like theme-color tints or rich-text runs) risks merging two
different existing styles/strings into one. Separately, holding one
`RawNode` per cell does not scale to the row/column counts
`rust_xlsxwriter`/`calamine` handle (B2c: a naive estimate puts a
10M-cell sheet at 2-3 GB just for the tree, vs. the typed-struct +
streaming approach those two crates use).

This revision resolves it by making the "degenerate case" claim literal
rather than aspirational, and narrowing what it needs to do in Phase0:

- **`Workbook::new()` opens a small embedded template `.xlsx`** (committed
  as bytes in the crate, built once offline — not generated at runtime)
  via the exact same `Package::open` / Layer 2 lens code path used for
  editing. The template carries the OOXML-required defaults a blank
  workbook needs (the two mandatory `fills` entries at index 0/1, a
  minimal theme, `app.xml`) as *existing* parts, so there is no separate
  "build a workbook from nothing" code path to keep in sync with the edit
  path — it's the same path, seeded from a different starting file.
  `docProps/core.xml` carries the template's own authoring timestamp and
  creator, which — per round 2 review (R2-m2) — every file
  `Workbook::new()` produces would otherwise keep, a cosmetic but
  needless giveaway that the file wasn't produced freshly. Round 3
  review (R3-m1) pointed out that fixing this via the Layer 2 tree
  wouldn't be consistent with Layer 2 only covering `sheet1.xml` in
  Phase0 (per Scope boundary, below) — `core.xml` is not a Layer 2 part
  at all. Phase0 instead regenerates `core.xml` wholesale and installs
  it via `Package::set_part` (making it `Replaced`, Layer 1's third
  state, not a `Parsed`-with-dirty-nodes Layer 2 part): there is nothing
  in the template's `core.xml` worth preserving node-by-node, so
  replacing it outright is simpler than parsing it just to edit two
  fields.
- **Phase0's from-scratch API is exactly `add_worksheet()` +
  `write_string()`/`write_number()`**, using the same `inlineStr`-only,
  no-new-styles policy as the edit path (so, again, no new mechanism is
  needed for Phase0 specifically). Per round 2 review (R2-B1),
  `add_worksheet()` does **not** implement general sheet addition (that
  stays out of scope, per Layer 3 above): the template ships with
  exactly one worksheet, and Phase0's rule is that the *first* call to
  `add_worksheet()` returns that existing sheet, while a second call, or
  any call that asks for a specific sheet name, returns `Err`. Renaming
  a sheet is `Err` for the same reason — it would touch `workbook.xml`
  and `docProps/app.xml`'s `TitlesOfParts`/`HeadingPairs` together, which
  Phase0 doesn't implement.
- **Value-based dedup for shared strings and `Format` (the actual
  rust_xlsxwriter-parity feature) is deferred to a later ADR.** That
  future ADR should record the insight this review surfaced: it needs
  **two seed modes**, not one — "from scratch: dedup by value" vs. "edit:
  freeze existing entries as opaque/unparsed, dedup only newly-added
  entries by value, append-only" — with only the append-only registry
  shape shared between them, not the dedup logic itself.
- **Large-sheet performance is explicitly a non-goal for Phase0 and a
  known future redesign point, not an oversight**: matching
  `rust_xlsxwriter`/`calamine` at scale will require `sheetData` to be
  parsed/held lazily (row-span-based, not a `RawNode` per cell) rather
  than the simple whole-tree `RawNode` this phase uses. Phase0's fixture
  and tests are small enough that this isn't yet a correctness or
  performance concern, but the tree design above (span-based, dirty-bit
  per node) is chosen partly *because* it's a strict subset of what a
  future lazy/row-level tree would need, not a dead end.

## Scope boundary for this ADR / this phase

In scope now:
- `Package`: three-state part tracking, `Raw`/dirty-free-`Parsed` passthrough
  defined precisely as (name, compression method, CRC32, payload) equality.
- A minimal `RawNode` tree (`ElementSpan` + `own_dirty`/`subtree_dirty`,
  as above) for `sheet1.xml` only.
- `Worksheet::cell` (read-only) / `cell_mut` (returns a handle; creation
  deferred to the first value-setting call on it), with exactly two
  value-setting calls, `set_string` (always writes `inlineStr`) and
  `set_number`. Proving the pattern end to end: open a real multi-part
  `.xlsx`, and for every cell in the fixture — including the existing
  string (`A1`/`A2`), number (`B2`), boolean (`C2`, overwritten with
  `set_number`, which drops the now-stale `t="b"` attribute — an
  `own_dirty` case, named explicitly here per round 3 review, R3-m3,
  since T2's per-cell sweep must exercise it), and self-closing (`D2`)
  cells — call `set_string` or `set_number` on it, save, and verify
  (T1/T1b/T2 oracles, plus Layer 1's guarantee for every other part)
  that the shared string table, styles, other cells, and an
  intentionally-foreign `<extLst>` extension element all survive, and
  that the output reads back correctly through an independent
  implementation (`calamine`, as a
  dev-dependency) — not just that `ooxml-xlsx` agrees with itself.
- The from-scratch skeleton: `Workbook::new()` (embedded template) →
  `add_worksheet()` (first call only; returns the template's one sheet)
  → `write_string()`/`write_number()` → `save()`, no new styles, strings
  always `inlineStr`.

Explicitly out of scope for this ADR (future ADRs, once this foundation is
validated):
- Value-based dedup for shared strings / `Format` (both from-scratch and
  edit-time) — see "two seed modes" above.
- Shared formulas, array formulas, merged-cell writes, any formula-cell
  write, `calcChain.xml` maintenance, and — per round 2 review (R2-M3) —
  *any* value edit at all to a workbook that contains a `calcChain.xml`
  part, to avoid silently leaving stale cached formula results on an
  untouched sheet. Phase0 detects all of these and returns `Err`.
- Adding worksheets/parts to an existing package, or anything else that
  requires updating `[Content_Types].xml`/`.rels`/`workbook.xml`'s sheet
  list together.
- Encrypted packages (CFB/OLE-wrapped, password-protected `.xlsx`) —
  `Package::open` returns a typed `Error::EncryptedPackage` rather than a
  confusing zip-parse failure.
- Strict OOXML (the ECMA-376 Strict variant's namespace URIs) — Phase0's
  lens only recognizes the Transitional namespaces real Excel actually
  writes; opening a Strict file returns `Err` rather than silently
  treating every element as unrecognized and duplicating rows via
  find-or-create.
- Byte-exact attribute fidelity *of nodes that are actually edited*
  (quote style, inter-attribute whitespace on a dirty node's own
  re-rendered tag) — everything *not* on the edited node's own tag,
  including all untouched siblings/ancestors, is already byte-exact
  under the corrected Layer 2 rule above, which is a stronger guarantee
  than the original draft gave.
- Word/PowerPoint crates — same `ooxml-core` foundation, but not started
  until this Excel-only proof holds up.
- Full `rust_xlsxwriter` feature parity (charts, conditional formatting,
  images, data validation, ...).
- Large-sheet performance parity with `rust_xlsxwriter`/`calamine` (lazy
  `sheetData` handling) — see above.
- A round-trip regression corpus at the scale of `rust_xlsxwriter`'s or
  Apache POI's. The current fixture
  (`reference/round_trip_corpus/fixture_minimal.xlsx`) is hand-authored,
  not Excel/LibreOffice-authored (LibreOffice headless conversion isn't
  available in this sandbox), and does **not** cover: shared/array
  formulas, `mc:Ignorable`, `xml:space="preserve"`, zip entries using a
  data descriptor, or `<row spans="...">` hints. Phase0's "pattern
  proven" claim is scoped to what the fixture actually contains; it is
  not a claim that these edge cases work. Extending the fixture (or
  sourcing a real Office/LibreOffice-authored one) is a near-term
  follow-up, not assumed done.

## Alternatives considered

1. **Full remodel, like openpyxl / `umya-spreadsheet` / `docx-rs`.**
   Rejected — but the evidence for rejecting it (openpyxl issues #847,
   #295, #1232, #1444, #1268: whole charts/pivot tables destroyed on
   save) specifically indicts the *absence of Layer 1* (those tools never
   read the chart/pivot parts at all, so they can't write them back) —
   not, by itself, the absence of something like Layer 2. See alternative
   1b.

   1b. **Middle option: Layer 1 passthrough + full typed remodel of
   touched parts, with an escape-hatch `Vec<RawXml>` for elements/
   attributes the typed model doesn't recognize.** This is a real
   contender, not a strawman: it avoids Layer 2's per-cell tree memory
   cost entirely and lets most of `rust_xlsxwriter`'s own typed-struct
   code be reused almost as-is for the touched-part case. It was not
   chosen for Phase0 because the escape hatch loses exactly the
   information Layer 2's `span`+dirty-bit design keeps for free: the
   *relative order* between modeled and unmodeled siblings (e.g. where an
   `<extLst>` or `mc:AlternateContent` block sits relative to modeled
   elements), and the unmodeled elements' own internal structure/spacing.
   For a part as structurally simple as `sheet1.xml`'s `sheetData`, that
   gap may be tolerable; it is not re-evaluated here as a bad choice in
   general, only as unnecessary for Phase0 given Layer 2 is already being
   built for other reasons (the round-trip requirement demands *some*
   fidelity mechanism regardless). Revisit this alternative explicitly
   once Phase0's performance non-goal becomes a real constraint (large
   sheets): migrating the touched-part path to 1b while keeping Layer 1
   and Layer 3 unchanged is a plausible future direction, not a dead end.

2. **Full byte-level fidelity from day one (exact quote style, attribute
   whitespace, comments, processing instructions, for every node
   including edited ones).** The original draft rejected this as
   "premature" for cost reasons. Opus review (M3c) correctly pointed out
   that reasoning was backwards once Layer 2 is span-based: verbatim
   passthrough of non-dirty nodes is *cheaper* to implement than
   re-rendering everything from attrs, not more expensive, and it's what
   Layer 2 already does. What's actually deferred, narrowly, is fidelity
   of a *dirty* node's own re-rendered open/close tags (its attribute
   quote style and inter-attribute whitespace) — a small, explicitly
   bounded gap, not a cost-driven postponement of the whole feature.

3. **Decompress-and-recompress untouched parts instead of raw copy.**
   Rejected: recompressing costs CPU for data that was never touched, for
   no benefit — M1's analysis shows the guarantee unit (compressed
   payload + CRC equality) is identical either way, since even raw copy
   doesn't make the surrounding zip container byte-identical. The
   original draft's framing ("decompress+recompress makes verification
   unverifiable") was too strong; decompressed-content + CRC comparison
   would in fact work for either approach. The real, narrower reason to
   prefer raw copy is simply: it is strictly cheaper and strictly
   no-worse, with no downside identified.

4. **Wrap `rust_xlsxwriter` for write and `calamine` for read, bridge
   them.** The user considered and rejected this when scoping the project
   (see Context). It also would not have actually produced round-trip
   editing: neither crate is designed to edit an existing file in place,
   so bridging them reduces to alternative 1 (read, fully convert to an
   internal model, fully regenerate) with extra steps, not a genuinely
   different design.

5. **Build on `ooxmlsdk`** (a Rust OOXML SDK, schema-code-generated,
   with part-level dirty tracking already implemented and validated
   against Apache POI's 682-file round-trip corpus — found during the
   prior research pass and cited in Sources). Not adopted for Phase0,
   for two reasons found during that same research: its generic DOM-level
   editing of elements/attributes the generated schema doesn't cover
   (exactly Layer 2's job here) was not yet public as of the research
   date, and adopting a schema-generation-first SDK is a heavier
   dependency than the minimal hand-rolled core Phase0 needs to prove its
   own narrower claims. This is a genuine gap in evaluation depth, not a
   considered-and-dismissed verdict — `ooxmlsdk` is worth a real,
   dedicated evaluation (as a dependency, or as a contribution target for
   its Layer-2-equivalent) once Phase0's own assumptions are validated,
   rather than ruled out here on partial information.

## Consequences

- Positive: the riskiest capability (round-trip editing) is de-risked
  first and cheaply, with hard byte-level test oracles (T1/T1b/T2) rather
  than manual inspection, before any feature-breadth work — and T1b/T2
  now actually exercise Layer 2's parser and renderer (the original
  draft's single test only ever exercised Layer 1).
- Positive: the three-layer split gives each kind of "don't break what I
  didn't touch" problem (whole unmodeled parts; unmodeled siblings inside
  a modeled part; cross-part index references) exactly one owner, with an
  explicit invariant (append-only registries) connecting them, instead of
  one mechanism quietly being expected to cover all three.
- Positive: narrowing Phase0 to `inlineStr`-only writes and no new
  styles/formulas removes the cross-part consistency hazard (B1) and the
  dedup/performance mismatch (B2) entirely from this phase, rather than
  asserting they're solved when they aren't yet.
- Negative: Phase0's "round-trip editing" is real but narrow — it covers
  plain value edits to existing string/number/boolean cells only; writing
  a brand-new styled cell, a formula, or a new sheet all return `Err` by
  design, not partial/silent support. This should not be read as "editing
  works" in a general sense yet.
- Negative: the fixture used to validate all of this is hand-authored and
  known not to cover several real-world shapes (shared formulas,
  `mc:Ignorable`, preserved whitespace, data-descriptor zip entries).
  Phase0's "pattern proven" claim is bounded by what the fixture actually
  contains, and that boundary is written down rather than implied.
- Negative: large-sheet performance parity with `rust_xlsxwriter`/
  `calamine` is not addressed by this design as written and will need a
  lazy/streaming redesign of the Layer 2 tree for `sheetData`
  specifically — flagged now so it isn't discovered as a surprise later.
- Negative: the test oracles (T1/T1b/T2, plus Layer 1's byte-equality
  guarantee) prove internal consistency — that ooxml-rs didn't change
  anything it wasn't supposed to — but not correctness against real
  Excel. The `calamine` read-back dev-dependency test (added in this
  revision, R2-M5) gives one independent check, but this sandbox has no
  Excel and LibreOffice's headless conversion doesn't work here either
  (see `reference/round_trip_corpus/README.md`), so "does Excel actually
  open this without a repair-dialog" remains an explicit, unresolved gap
  that needs the user's own manual check before this phase's output is
  trusted for real use, not something this ADR can claim to have
  verified.
- Negative, scoped narrowly: Phase0's blanket "no edits to any workbook
  containing `calcChain.xml`" rule (R2-M3) is a real usability
  limitation, not just a safety margin — it refuses edits to the common
  case of a workbook with formulas anywhere in it, even when the edited
  cell has nothing to do with them. This trade (safety now, broader
  eventually) is deliberate, but it means "round-trip editing works" in
  Phase0 should be read as "works for formula-free workbooks" until a
  future phase implements real dependency tracking.

## Sources

- rust_xlsxwriter: https://github.com/jmcnamara/rust_xlsxwriter (src/packager.rs, worksheet.rs, format.rs, xmlwriter.rs, shared_strings_table.rs; tests/integration/common/mod.rs `compare_xlsx_files`)
- calamine: https://github.com/tafia/calamine (src/xlsx/mod.rs, errors.rs, datatype.rs)
- openpyxl round-trip loss reports: https://foss.heptapod.net/openpyxl/openpyxl/-/issues/847, /295, /1232, /1444, /1268
- Apache POI part-level preservation: `POIXMLDocumentPart.commit()` (https://poi.apache.org/apidocs/org/apache/poi/openxml4j/opc/OPCPackage.html); dotnet-poi `_preservedEntries` (https://modeverv.github.io/dotnet-poi/compatibility/format-coverage)
- ooxmlsdk (Rust, part-level dirty tracking, 682/682 Apache POI corpus round-trip; version at time of citation: 0.15.0): https://docs.rs/crate/ooxmlsdk/latest/source/README.md, https://crates.io/crates/ooxmlsdk/0.15.0
- DomTrip (Java, node-level lossless XML editing): https://maveniverse.github.io/domtrip/docs/features/lossless-parsing
- `zip` crate `raw_copy_file` and MSRV: zip-9.6.0 `src/write.rs`, `Cargo.toml` (`rust-version = "1.88"`)
- `quick-xml` MSRV: quick-xml-0.42.0 `Cargo.toml` (`rust-version = "1.86"`)
- Opus round-1 review: `docs/adr/opus-review-round1.md`
- Opus round-2 review: `docs/adr/opus-review-round2.md`
