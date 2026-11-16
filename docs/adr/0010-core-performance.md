# ADR-0010: Core performance: compact Layer 2 tree, deferred subtrees, save and read paths

- Status: **Proposed**
- Date: 2026-11-16
- Depends on: [ADR-0001](0001-lossless-round-trip-architecture.md) (layers, serialization rule,
  T1/T1b/T2, "large-sheet performance is a known future redesign point"),
  [ADR-0002](0002-schema-driven-code-generation.md) (lens primitives and dirty-flag duties; its
  post-acceptance note on the parser depth limit)
- Evidence:
  - `docs/testing/performance-baseline-2026-11.md` and `crates/ooxml-bench` (PR #24): the
    baseline and its hotspots.
  - `spikes/core-performance/` (this PR): a prototype of the compact tree and the lazy
    `sheetData`, checked against core and measured on the same inputs.
- Out of scope: the xlsx worksheet row index. The Excel owner is implementing it in ooxml-xlsx
  (branch `perf/xlsx-row-index`), and it covers baseline hotspot 2. This ADR provides the core
  mechanisms that index can later sit on (§2), but does not specify the index.

## Context

The 2026-11 baseline (revision 2, run 3 on a shared host; see its caveats) puts ooxml-rs far from the xlsx Phase6 goal
("within 2× of rust_xlsxwriter (`constant_memory`) and calamine for a 1M-cell sheet; memory
bounded by rows touched"). Layer 1 is not the problem: raw-copied parts move at GB/s. Four core
hotspots remain once the xlsx cell lookup is set aside:

| # | Hotspot | Measured (1M-cell sheet, 33 MiB `sheet1.xml`) |
|---|---|---|
| 1 | The Layer 2 tree: allocation-bound, large | Parse with drop excluded takes 4.0–5.6× as long as a bare quick-xml pass. Dropping the tree adds 46–80% of the parse time. 5.29 allocations plus 0.10 reallocations per element. 1.44 GiB of heap (45–50 bytes per XML byte), against 77 MiB for calamine's whole read. `RawNode` and `RawItem` are 144 B, `RawAttr` 72 B, `XName` 48 B. |
| 3 | Save re-deflates the rewritten part at zip's default level | `to_bytes` after one edit takes 1.06 s. Deflating that part alone takes 1.07 s at the default level and 69 ms at level 1. Serializing takes 29 ms. |
| 4 | Open builds the full tree, then re-scans the same bytes for the formula census | `Workbook::from_bytes` takes 1.95 s with drop excluded: parse 1.25 s and census scan 0.29 s. Dropping the `Workbook` adds another 1.2 s. |
| 5 | pptx read paths keep every parsed slide | 1,000 slides: 211 MiB retained for 11.9 MiB of XML. |

ADR-0001 anticipated this: "matching rust_xlsxwriter/calamine at scale will require `sheetData`
to be parsed/held lazily (row-span-based, not a `RawNode` per cell) … the tree design above
(span-based, dirty-bit per node) is chosen partly *because* it's a strict subset of what a future
lazy/row-level tree would need". This ADR is that redesign. It is done at the core level so that
every format crate benefits.

### Invariants that must survive (unchanged)

- **Layer 1:** an untouched part, or a parsed part with no dirty node, is written with its
  original (name, method, CRC-32, compressed payload).
- **Layer 2 serialization rule:** a node with `!subtree_dirty` is emitted as its original span.
  With `subtree_dirty && !own_dirty`, its original open and close tags are emitted and its
  children recurse. With `own_dirty`, it is re-rendered. Prolog and epilog are always verbatim.
- **Oracles:**
  - T1: no edits gives identical bytes.
  - T1b: forcing every node dirty, rendering and re-parsing gives a structurally equal tree.
  - T2: an edit changes bytes only inside the edited element's original span, or at the
    insertion point.
- **Refusals at parse time:** DTD (`DtdNotAllowed`), non-UTF-8, well-formedness errors, unbound
  prefixes, undeclared entities, and the depth limit (`TooDeep`, default 256, which also bounds
  lens inserts). The parse still fails, with the same error, for every input that fails today.
- **Public read API of `RawNode`, `RawItem`, `RawText`, `RawAttr`, `RawMarkup`, `XName` and
  `ElementSpan`:** format crates pattern-match `RawItem` and call
  `attr() -> Option<&str>`, `value() -> &str` and `children() -> &[RawItem]`. Stages 1–3 must not
  break this, so that open PRs (#14, #18, #19, the xlsx row index) need no rework.

## Decision

### 1. Compact `RawNode` (API-preserving)

The representation changes; the accessors do not.

| Today | Decision | Why it is safe |
|---|---|---|
| `XName { ns: Arc<str>, local: Box<str>, prefix: Option<Box<str>> }`, 48 B and 1–2 allocations per name | `XName(Arc<XNameData>)`: one thin pointer (8 B), interned per parse by `(prefix, local, ns)`. `XName::new` stays (it allocates; it is only used for inserted nodes). `ns()`, `local()`, `prefix()`, `is()`, `display()` and `Eq`/`Hash` keep their meaning. Equality compares by pointer first, then by content. | Names are immutable once built. Interning is per document, so a subtree copied between documents (`detach_as`, #18) carries its own `Arc`s. |
| `RawAttr.value: String`, `RawText.value: String`, `RawMarkup.raw: Box<str>` | `SStr`: an `Arc<str>` of the part's source plus a `u32` range (24 B, no allocation). When the decoded value differs from its source bytes (entities, CR/LF/tab normalization, text merged across an entity reference), the value is owned instead: the same type, with a buffer of its own. `value()`, `raw()` and `text()` still return `&str` / `Cow<str>`. | Values are only rewritten through the lens primitives, which install an owned `SStr`, and the dirty flags work exactly as today. The `Arc` keeps a value valid when its node is cloned or moved to another document. |
| `XmlDocument.source: Box<str>` | `Arc<str>`, shared by the values. `source() -> &str` is unchanged. | — |
| `ElementSpan { start, open_end, close: Option<(usize, usize)> }` stored as is | Stored as four `u32` offsets, with the two dirty flags packed into the same word. `span()` still returns the public `ElementSpan` with `usize` fields. | Parts are capped at 4 GiB, which zip64 aside is far above any real part. A larger part is refused with a typed error rather than truncated. |
| `attrs: Vec<RawAttr>` (grows while parsing) | A boxed slice, allocated once per start tag with the exact count. The lens's `set_attr`/`remove_attr` rebuild it; edits are rare. `attrs() -> &[RawAttr]` is unchanged. | — |
| `children: Vec<RawItem>` (grows by doubling while parsing) | Collected in a per-depth scratch vector and moved into an exact-capacity `Vec` when the element closes. Stage 4 adds an arena (below). | — |

**Measured** (spike `src/compact.rs`, same inputs as the baseline; full tables in the spike's
README):

- **Correctness:** on 1,589 fixture XML parts plus adversarial inputs, the compact tree passes T1
  byte for byte, is structurally identical to core's tree (names with prefixes, attributes, text,
  markup), and its full re-render is byte-identical to core's.
- **Sheets (10k–1M cells):**
  - fresh allocations per element fall from 5.29 to 2.05, and reallocations from 0.10 to about 0;
  - heap falls from 45–50 to 8.6–9.7 bytes per XML byte (1.44 GiB to 286 MiB at 1M cells);
  - with drop excluded, parsing is 1.1–1.9× faster, and dropping is 4–6.5× faster;
  - parse plus drop of the 1M-cell sheet goes from 4.1 s to 1.7 s (2.4×);
  - unedited serialization and full re-render are unchanged.
- **Text-heavy documents** (the largest fixture `word/document.xml`) need 2.7× less memory. Their
  parse is no faster: 31 ms against 39 ms in this run, within the host's noise.
- **Node sizes:** `RawNode`/`RawItem` go from 144 B to 72 B, `RawAttr` from 72 B to 32 B, and
  `XName` from 48 B to 8 B.

**Stage 4 (optional, decided later):** an arena for children. One `Vec<RawItem>` per document
holds every element's children contiguously, and a node keeps a `(start, len)` range instead of
its own `Vec`. That removes the remaining ~1 allocation per element. It is not API-preserving:
`children()` is unaffected, but insertion through the lens must splice the arena or move the
node's children out on first edit. It is therefore staged after the format crates have moved to
deferred subtrees, and only if the numbers still call for it.

### 2. Deferred subtrees: "parse only the subtree you need"

A general Layer 2 mechanism, of which row-based `sheetData` is the first user.

- **Request.** `ParseOptions::defer: &[QName]`, e.g. `[sml:row]`. For an element whose name is in
  `defer`, the parser builds the node itself (name, attributes, span) but not its children. The
  node is **deferred**; its content stays in the source until asked for.
- **Validation is not deferred.** While skipping a deferred element's content, the parser still
  reads every event without building anything. It checks well-formedness (quick-xml with
  `check_end_names`), the depth limit (a counter), entity references, and prefix bindings,
  tracking `xmlns` declarations in the skipped content. So `XmlDocument::parse_with` fails, with
  the same error, on every input it fails today: deferral can never turn a refusal into a later
  surprise. Each deferred node also records the in-scope namespace bindings: an `Arc` of the
  scope chain, shared by consecutive siblings.
- **Materialization.**
  - `RawNode::children()` on a deferred node parses its content slice on first use, with its
    recorded bindings and the document's `ParseOptions`. It stores the result in a `OnceLock`, so
    the read API stays `&self -> &[RawItem]`; materialization cannot fail, because the content was
    validated.
  - `LensMut` descending into a deferred node materializes it through `&mut` (no lock needed).
  - Spans of the materialized children are absolute offsets in the part's source, so the
    serialization rule is unchanged.
- **Serialization.** A deferred node that was never materialized is never dirty, so it is
  emitted as its span. Materialized nodes follow the normal rule. T1 and T2 therefore hold by
  construction. T1b's `force_rerender` materializes everything first.
- **Read-only accessors that do not materialize.** `RawNode::is_deferred()`, and
  `RawNode::content_span() -> Option<Range<usize>>`, which lets a caller scan the raw content
  itself, for example a census or a value read with quick-xml.
- **sheetData.** ooxml-xlsx opens worksheets with `defer = [sml:row]`. Each row then costs one
  small node: name, attributes, span, about 100 bytes. Its cells are built only when the row is
  read or written. Writes into existing rows edit materialized rows; new rows are inserted by the
  lens as today.
- **Measured** (spike `src/rows.rs`, 1M cells, 100k rows):
  - indexing the rows takes one scan of 164–344 ms (depending on host load) and 1.5 MiB, against
    1.47 GiB for today's tree;
  - finding, materializing, editing and re-rendering 1,000 rows takes 13–22 ms;
  - splicing the output takes about 4 ms.
  - The check confirms T2: every byte outside the edited rows is unchanged, and the output parses
    with core with the new values.
  - The spike's scan does not yet do the full validation described above. That is expected to
    cost a fraction of the scan, since no tree is built, and it is measured in stage 2.
  - Order of work for Phase6, per the baseline: the xlsx row index first, then the compression
    level (§4), then deferred rows (§2). Deferred rows are the only route to the memory goal.
    §1 helps every part, but for worksheets most of its effect is superseded by §2.
- **Row index.** `perf/xlsx-row-index` keeps its own row lookup in ooxml-xlsx. Over a deferred
  `sheetData`, that index is built from the row nodes' `r` attributes without materializing any
  row, so the two compose. This ADR does not change the xlsx API.

### 3. Census during the parse (single pass)

`ParseOptions::count: &[QName]` makes the parser count elements with those names, deferred
content included, in the same pass. The counts are reported by `XmlDocument::census(QName) ->
usize`. ooxml-xlsx's formula census (`f` elements) for the worksheet it parses then needs no
second scan. Other worksheets are still scanned once each, without a tree, as today. Measured
saving on the 1M-cell sheet: about 0.29 s of open time. This also gives other crates the general
"does this part contain X" question for free (pptx `p:timing`, docx `w:fldChar`).

### 4. Compression level for rewritten parts

`Package` gets save options:

```rust
pub struct SaveOptions { pub compression: Compression }   // #[non_exhaustive]
pub enum Compression { Fastest, Balanced, Smallest, Level(i64) }  // #[non_exhaustive]
impl Package { pub fn to_bytes_with(&self, o: &SaveOptions) -> Result<Vec<u8>>; pub fn save_with(..) }
```

- The level applies **only to parts that are written fresh**: replaced or dirty parts. Raw-copied
  parts keep their original compressed bytes, so Layer 1's equality guarantee is untouched. A
  part originally `Stored` stays `Stored`, as today.
- Measured on the rewritten 33 MiB sheet:

  | Level | Time | Compressed size |
  |---|---|---|
  | 1 | 72 ms | 5.8 MiB |
  | 3 | 217 ms | 3.3 MiB |
  | 6 (zip default) | 1,079 ms | 3.2 MiB |
  | 9 | 1,709 ms | 3.1 MiB |

- **Default:** `Balanced`, which is level 3: about 5× faster than today, with output within 3% of
  today's size. Users who need maximum compression ask for `Smallest`.
- `to_bytes()`/`save()` keep their signatures and use the default. Changing the default changes
  the bytes of rewritten parts only. No oracle compares compressed payloads of rewritten parts,
  and T3 compares only untouched entries.

### 5. Read paths that do not cache

- `Package::xml_part(&mut self)` caches the parsed tree, which is right for editing. For reading,
  add `Package::parse_part(&self, name) -> Result<XmlDocument>`. It returns a fresh tree, owned
  by the caller, and leaves the part `Raw`.
- `Package::release_part(&mut self, name) -> bool` drops a `Parsed` tree that has no dirty node
  back to `Raw`. It returns `false`, and does nothing, if the tree is dirty.
- ooxml-pptx's `Presentation::slide` (read) can then parse on demand without retaining. Its
  `slide_mut` keeps caching, and only the slides actually edited stay in memory until save.
  Whether `slide()` returns an owned view or keeps an LRU of parsed slides is ooxml-pptx's
  decision (ADR-0005's owner).
- Layer 1 is unaffected: a part that was only read stays `Raw` and is raw-copied.

### 6. What does not change

- The serialization rule, the dirty-flag duties of the lens primitives, ADR-0002's slots and
  generated views, the depth limit and its lens-insert bound, DTD refusal, and the
  metadata-parsing floor.
- The `Package` part states, except that `release_part` is a new transition `Parsed{clean} →
  Raw`.

## Migration plan

Each stage is one PR, mergeable on its own, with every existing test green and T1/T1b/T2 run on
all fixtures. No stage forces a change on an open PR.

| Stage | Content | API impact | Interaction with open work |
|---|---|---|---|
| 0 | Land `crates/ooxml-bench` (#24) as the yardstick. Port the spike's checks (T1, same tree, rerender equality) into `ooxml-core/tests` against a frozen copy of today's parser, used as the oracle for stage 1. | none | none |
| 1a | Interned `XName` (§1 row 1). | none (internals of `XName`) | #18 builds `XName::new(..)` and `RawAttr { name, value: a.value.clone() }`; both still compile. Merge #14 and #18 first if they are ready, else the stage-1 PR merges `origin/main` and resolves the mechanical conflicts itself. |
| 1b | `SStr` values, `Arc<str>` source, `u32` spans, boxed attribute slices, exact-size children (§1). | none: accessors unchanged, field types `pub(crate)` | #14's `remove_root_child_with_leading_ws` uses `RawText::is_whitespace`/`cdata`, which are kept. |
| 2 | Deferred subtrees (§2) with full validation during the skip, plus `content_span`/`is_deferred`; census (§3). | additive (`ParseOptions::defer`, `count`) | none until a format crate opts in |
| 2x | ooxml-xlsx opts in for worksheets (`defer = [sml:row]`, census of `f`), on top of the row index from `perf/xlsx-row-index`. | ooxml-xlsx internal | done by or with the Excel owner |
| 3 | `SaveOptions` and the default level (§4); `parse_part`/`release_part` (§5). | additive; default output bytes of rewritten parts change | ooxml-pptx opts in to non-caching reads (ADR-0005 owner) |
| 4 | Optional children arena (§1), only if stage 2's numbers still miss the targets. | may touch `pub(crate)` lens internals | after #18/#19 are merged |

**Acceptance for each stage:**

- `cargo bench -p ooxml-bench` numbers in the PR body, against `main` on the same host and in the
  same session.
- Every existing oracle green.
- For stage 1, the parse equivalence test (old parser vs new on all fixtures and on generated
  inputs) is green.
- For stage 2:
  - a property test that a deferred parse and a full parse serialize identically, with and
    without edits, and that errors match on corrupted inputs;
  - the depth-limit tests (10k/40k-deep inputs) still refuse at parse time even when the deep
    content is inside a deferred element.

**Targets** (1M-cell sheet, quiet host):

- Stage 1 alone: open ≤ 0.7 s and ≤ 350 MiB.
- With stages 2 and 2x: open ≤ 0.3 s and ≤ 40 MiB, and edit + save of 1,000 cells ≤ 0.5 s. That
  is within 2× of calamine's read time and below umya-spreadsheet's edit time.

## Alternatives considered

1. **Borrowed `&'src str` values (a lifetime on `XmlDocument`).** This is the cheapest
   representation, with no `Arc` and no reference counting. Rejected: `RawNode` would carry a
   lifetime through `Package`, every lens and every format crate's types. `Package` owns its
   parts' bytes, so self-referential storage would be needed. And #18's cross-document copies
   would be impossible. `SStr` gets most of the benefit (no allocation per value) for one atomic
   increment per value.
2. **Ranges without a pointer (values resolved through the document).** Values would be 8 B
   instead of 24 B. Rejected for stages 1–3, because accessors would need the document
   (`attr(&doc, q)`), which breaks every caller. It is reconsidered with the stage-4 arena, where
   a document handle becomes natural.
3. **An existing tree crate** (`roxmltree`, `xmltree`, `rowan`). `roxmltree` is read-only and
   borrows the input. `xmltree` has no spans and is allocation-heavy. `rowan`'s green/red trees
   are lossless but designed around token kinds, not XML namespaces, and would replace ADR-0002's
   lens primitives. None preserves the serialization rule without a wrapper as large as the
   change proposed here.
4. **Streaming-only writes for large sheets (no tree).** That is rust_xlsxwriter's model, and it
   fits only the from-scratch path. ADR-0001's edit path needs random access with locality,
   which deferred rows give. A streaming from-scratch writer can still be added in ooxml-xlsx on
   top of the row index; it is independent of this ADR.
5. **Making the zip default level 1.** It is about 15× faster than today but 1.8× larger
   (5.8 MiB against 3.2 MiB). Level 3 keeps the size within 3% at about 5× the speed, so it is the
   better default, and level 1 stays available as `Fastest`.
6. **Memory-mapped input instead of `Arc<[u8]>`/`Arc<str>`.** It saves one copy of the input. It
   is a Layer 1 change, independent of this ADR, and complicates save-to-the-same-path (ADR-0001
   R2-M4). Not now.

## Consequences

- **Positive:**
  - The largest memory and time costs of open are cut without touching any format crate's code
    (stage 1).
  - Large sheets become row-proportional (stage 2x), which is ADR-0001's stated redesign point.
  - Save of large edited parts becomes several times faster (stage 3).
  - pptx reads stop retaining every slide.
- **Positive:** the deferred-subtree mechanism is general. docx (body paragraphs) and pptx
  (slide shape trees) can use it later with the same guarantees.
- **Negative:**
  - `SStr` adds an atomic reference-count operation per parsed value (parse and drop). It was
    included in the spike's measurements, which still show 1.1–1.9× faster parsing and 4–6.5×
    faster drops on sheets.
  - A value copied into another document keeps that part's source text alive until the copy is
    dropped. This is bounded by the source size, and acceptable for copies of formatting
    subtrees.
- **Negative:**
  - Deferred nodes add a `OnceLock` per deferred element and a second code path in the parser
    (skip with validation). The parse-equivalence property test is the guard.
  - Bugs there would surface as differences between deferred and full parses, which that test
    checks on every fixture.
- **Negative:** changing the default compression level changes the bytes of rewritten parts
  (only those). Tools that compare whole files byte for byte after an edit will see different
  output from before.

## Open questions for the project owner

1. Default compression for rewritten parts: `Balanced` (level 3), or keep zip's default (6) and
   let callers opt in to faster?
2. Stage 4 (children arena): decide after stage 2's numbers, or rule it out now to keep the
   lens internals stable?
3. pptx read caching: `slide()` returning an owned view, or a small LRU? This is for ADR-0005's
   owner.

## Sources

- `docs/testing/performance-baseline-2026-11.md`, `crates/ooxml-bench` (PR #24).
- `spikes/core-performance/` (README, `src/compact.rs`, `src/rows.rs`, `src/main.rs`).
- ADR-0001 "Large-sheet performance is explicitly a non-goal for Phase0 and a known future
  redesign point"; ADR-0002 post-acceptance notes (depth limit; detach/insert, PR #18).
- quick-xml 0.42 (`Reader::from_str`, borrowed events, `check_end_names`), zip 9.6 (deflate
  levels via `SimpleFileOptions::compression_level`).
