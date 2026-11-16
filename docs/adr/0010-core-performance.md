# ADR-0010: Core performance: compact Layer 2 tree, deferred subtrees, save and read paths

- Status: **Proposed** (revision 2)
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
  (PR #29, `perf/xlsx-row-index`), and it covers baseline hotspot 2. This ADR provides the core
  mechanisms that index can later sit on (§2), but does not specify the index. It does state what
  the index must do lazily on a deferred sheet (§2.5).

## Revision history

- **Revision 1:** initial proposal.
- **Revision 2** (after `0010-opus-review-round1.md`):
  - **M1 (deferred nodes).** Every `&mut` access to a deferred node expands it first. `children`
    is reachable only through accessors, and core code reads no field directly: the serializer,
    #18's `detach_node`, core/add-part's `apply` and the lens all go through them. The serializer
    also has a defence for an unexpanded node (§2.3).
  - **M2 (deferred skip and targets).**
    - The deferred skip is a non-building mode of the same parser loop, so errors are identical by
      construction (§2.2).
    - The validated skip costs about 315 ms at 1M cells: about 1.9× the unvalidated scan and about
      2.2× faster than a full compact parse (reviewer's probe).
    - Targets are re-stated from measured numbers, relative to `main` in the same session.
    - The retained source text is acknowledged: memory is not literally bounded by the rows
      touched (Migration plan, Targets).
  - **M3 (row index).** PR #29's per-row cell indices must be computed lazily on first lookup,
    and its bulk iterator must not expand every row. This is a stage-2x requirement on ooxml-xlsx
    (§2.5).
  - **M4 (`release_part`).** `release_part` bumps the package generation, so a staged
    core/add-part `RegistryPlan` made earlier becomes stale instead of silently skipping edits
    (§5).
  - **Minor findings:**
    - m1: the text-heavy parse slowdown (30–38% on the largest `word/document.xml`) is traced to
      the spike's name interner (a formatted key per lookup, hashed with SipHash). It is fixed in
      the spike and remeasured: now no slower than core on that document, and 1.6–1.8× faster on
      large sheets (§1).
    - m2: timings are given as ranges over repeated runs.
    - m3: `defer` is `&'static [QName]`, opted in per part (worksheets only), and a deferred node
      carries its depth and context.
    - m4: `parse_part` semantics for every part state.
    - m5: compression applies to added and replaced parts as well; level range is validated;
      real-corpus sizes are added.
    - m6: an interaction table covering core/add-part, #14, #18, #26, #29 and pptx's
      `Slide<'a>` borrows.
    - nits: `XName` `Hash` is by content and its `Debug` output changes; the frozen parser for
      stage 0 lives in `tests/support/`.
  - **Owner questions:** the reviewer's recommendations are recorded as the proposed answers
    (level 3 default; the arena decided after stage 2; pptx owned read API or `release_slide`,
    no LRU).

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
| `XName { ns: Arc<str>, local: Box<str>, prefix: Option<Box<str>> }`, 48 B and 1–2 allocations per name | `XName(Arc<XNameData>)`: one thin pointer (8 B), interned per parse by `(prefix, local, ns)`. `XName::new` stays (it allocates; it is only used for inserted nodes). `ns()`, `local()`, `prefix()`, `is()`, `display()` and `Eq`/`Hash` keep their meaning. Equality compares by pointer first, then by content; `Hash` is computed from the content, never the pointer, so it agrees with `Eq`. The derived `Debug` output changes shape (a wrapped struct), which no test depends on. | Names are immutable once built. Interning is per document, so a subtree copied between documents (`detach_as`, #18) carries its own `Arc`s. |
| `RawAttr.value: String`, `RawText.value: String`, `RawMarkup.raw: Box<str>` | `SStr`: an `Arc<str>` of the part's source plus a `u32` range (24 B, no allocation). When the decoded value differs from its source bytes (entities, CR/LF/tab normalization, text merged across an entity reference), the value is owned instead: the same type, with a buffer of its own. `value()`, `raw()` and `text()` still return `&str` / `Cow<str>`. | Values are only rewritten through the lens primitives, which install an owned `SStr`, and the dirty flags work exactly as today. The `Arc` keeps a value valid when its node is cloned or moved to another document. |
| `XmlDocument.source: Box<str>` | `Arc<str>`, shared by the values. `source() -> &str` is unchanged. | — |
| `ElementSpan { start, open_end, close: Option<(usize, usize)> }` stored as is | Stored as four `u32` offsets, with the two dirty flags packed into the same word. `span()` still returns the public `ElementSpan` with `usize` fields. | Parts are capped at 4 GiB, which zip64 aside is far above any real part. A larger part is refused with a typed error rather than truncated. |
| `attrs: Vec<RawAttr>` (grows while parsing) | A boxed slice, allocated once per start tag with the exact count. The lens's `set_attr`/`remove_attr` rebuild it; edits are rare. `attrs() -> &[RawAttr]` is unchanged. | — |
| `children: Vec<RawItem>` (grows by doubling while parsing) | Collected in a per-depth scratch vector and moved into an exact-capacity `Vec` when the element closes. The field becomes private to the module that defines `RawNode`, reachable only through `children()` and `children_mut()`. Stage 4 adds an arena (below). | Every reader and writer of children goes through the accessors, which §2.3 needs and which make stage 4 a change of lens internals only. |

**Measured** (spike `src/compact.rs`, same inputs as the baseline; full tables in the spike's
README):

- **Correctness:** on 1,589 fixture XML parts plus adversarial inputs, the compact tree passes T1
  byte for byte, is structurally identical to core's tree (names with prefixes, attributes, text,
  markup), and its full re-render is byte-identical to core's.
- **Sheets (10k–1M cells).** Three repeated runs at load 6–7, after the interner fix below:
  - fresh allocations per element fall from 5.29 to 2.05, and reallocations from 0.10 to about 0;
  - heap falls from 45–50 to 8.6–9.7 bytes per XML byte (1.44 GiB to 286 MiB at 1M cells);
  - with drop excluded, parsing is 1.35–1.8× faster (1M cells: 504–520 ms against 852–908 ms);
  - dropping is 3.5–6× faster (1M cells: 152–154 ms against 764–792 ms);
  - unedited serialization and full re-render are unchanged.
- **Text-heavy documents** (the largest fixture `word/document.xml`, 1.3 MiB):
  - 2.7× less memory and 2× fewer allocations;
  - parse time equal to core's (14.6–14.8 ms against 14.8–15.5 ms);
  - drop 5× faster.
- **The interner fix (round-1 m1).** Revision 1's spike was consistently 30–38% *slower* than
  core on that document (21.0 ms against 15.2 ms in the same session). The cause was the spike's
  per-parse name interner, which on every element and attribute formatted a key string
  containing the namespace URI and hashed it with SipHash. Text-heavy WordprocessingML has many
  attributes per element (rsids, `w14:paraId`), so it paid most.
  - The interner now keys on `(prefix, local)` borrowed from the source plus the address of the
    interned namespace URI, with FNV-1a. A lookup neither formats nor allocates.
  - That removed the regression and made sheets 1.6–1.8× faster at 100k–1M cells (before the fix:
    1.1–1.3×).
  - Stage 1's acceptance includes "no parse regression on the largest docx/pptx fixtures" so that
    this stays checked.
- **Node sizes:** `RawNode`/`RawItem` go from 144 B to 72 B, `RawAttr` from 72 B to 32 B, and
  `XName` from 48 B to 8 B.

**Stage 4 (optional, decided later):** an arena for children. One `Vec<RawItem>` per document
holds every element's children contiguously, and a node keeps a `(start, len)` range instead of
its own `Vec`. That removes the remaining ~1 allocation per element. It is not API-preserving:
`children()` is unaffected, but insertion through the lens must splice the arena or move the
node's children out on first edit. It is therefore staged after the format crates have moved to
deferred subtrees, and only if the numbers still call for it.

### 2. Deferred subtrees: "parse only the subtree you need"

This is a general Layer 2 mechanism; row-based `sheetData` is its first user.

#### 2.1 Request

- `ParseOptions::defer: &'static [QName]`, e.g. `&[sml::Row::QNAME]`. `QName` is a pair of
  `&'static str`, so `ParseOptions` stays `Copy`, `Eq` and lifetime-free.
- An element whose name is in `defer` is a **deferred node**:
  - the parser builds the node itself: name, attributes, span;
  - it does not build the children;
  - the content stays in the source until it is asked for.
- **Opt-in per part, not per package.** The package keeps one default `ParseOptions`.
  `Package::xml_part_with(name, &ParseOptions)` parses one part with other options, for example
  ooxml-xlsx's worksheets with `defer = &[sml:row]`. An already-parsed part keeps the options it
  was parsed with. Deferring package-wide would also defer `sheetData/row` in external-link parts,
  which is harmless but pointless, so it is not done.

#### 2.2 The skip: a non-building mode of the same parser loop

The content of a deferred element is consumed by **the same event loop** as everything else,
switched into a mode that builds nothing.

- **Identical checks.** Every check the building mode makes runs unchanged, in the same order and
  with the same error positions:
  - DocType refusal;
  - attribute checks (`with_checks(true)`, `normalized_value`);
  - general-entity and character-reference resolution;
  - prefix binding of element and attribute names, including `xmlns` declarations inside the
    skipped content;
  - end-tag matching;
  - the depth limit, counted from the absolute depth.
- **Nothing is allocated.** Values are checked but not stored. Names are resolved but not
  interned.
- **Same errors by construction.** Because it is one loop with one set of checks, an input that
  fails today fails with the same error and position. A separate validating scanner could not
  guarantee that, so none is written.
- **Measured** by the round-1 reviewer's probe, which implements this skip (1M cells, best of 7,
  load 11–23):

  | Pass | Time |
  |---|---|
  | quick-xml events only | 142–146 ms |
  | unvalidated row scan (spike) | 164–168 ms |
  | **validated skip + row index (no tree)** | **312–320 ms** |
  | compact full parse | 698–706 ms |
  | core `XmlDocument::parse` | 799–879 ms |

  Validation roughly doubles the scan, which revision 1 had called "a fraction" of it. The skip is
  still about 2.2× faster than a full compact parse, and it allocates almost nothing.

#### 2.3 Invariant: every `&mut` access to a deferred node expands it first

- A deferred node must never become dirty while unexpanded. If it did, today's serializer would
  render `own_dirty` with "no children" and write `<row …/>`, losing every cell.
- Therefore:
  - **Reads:** `children(&self)` expands on first use into a `OnceLock<Vec<RawItem>>`. Expansion
    cannot fail, because §2.2 already validated the content.
  - **Writes:** `children_mut(&mut self)`, and every `&mut` path that reaches a deferred node,
    expand it first. That includes `LensMut` on the deferred node itself, for example setting
    `row/@ht` without descending.
  - **No other access.** No code in ooxml-core reads or writes the `children` field directly. The
    field is private to the `RawNode` module. The following all go through the accessors:
    - the serializer (`write.rs`), `structurally_eq` and `force_rerender`;
    - the lens (`insert`, `insert_at` of #26, `remove_at`);
    - PR #14's `remove_root_child_with_leading_ws`;
    - PR #18's `detach_node` and `Detached::height`;
    - core/add-part's `apply` (`root.children.get_mut`, `e.attrs.iter_mut`).

    Stage 1b introduces the accessors, before any deferral exists, so stage 2 changes no caller.
- **Defence in depth.** If the serializer ever meets an unexpanded deferred node with `own_dirty`
  set, it renders the open tag, copies the content `src[open_end..close.0]` verbatim, and renders
  the close tag. A `debug_assert!` marks the invariant violation.
- **Acceptance (stage 2):**
  - a test that `ooxml-core` contains no direct `.children` access outside the accessor module,
    enforced by the field's privacy;
  - property tests that change only the attributes of deferred nodes, with no descent, and check
    that the output keeps their content.

#### 2.4 What a deferred node carries

`RawNode::children(&self)` has no access to the document, so a deferred node holds what
expansion needs:

- the source (`Arc<str>`, shared);
- the parse options (`Copy`);
- its **depth** (`u16`): expansion counts the limit from there;
- an `Arc` of the in-scope namespace bindings, shared by consecutive siblings;
- the `OnceLock` for the expanded children.

Estimated size per deferred row:

| Part | Size |
|---|---|
| node | 72 B |
| attribute slice (`r`, often `spans`, `ht`, …) | 32 B per attribute |
| `OnceLock<Vec<_>>` | about 32 B |
| two `Arc`s, options and depth | about 40 B |
| **total** | **about 150–250 B** |

For 100k rows that is 15–25 MiB, not the spike's 12 B of `(r, start, end)`; revision 1's "about
100 bytes" was low.

Non-expanding read accessors:

- `RawNode::is_deferred()`;
- `RawNode::content_span() -> Option<Range<usize>>`, so that a caller can scan raw content
  itself (a census, a bulk value read) without expanding;
- `RawNode::release_children(&mut self) -> bool`, which drops a clean expansion back to deferred.
  It returns `false`, and does nothing, if anything inside is dirty.

#### 2.5 sheetData and the row index (requirements on ooxml-xlsx, stage 2x)

- ooxml-xlsx parses worksheets with `defer = &[sml:row]` (§2.1). Each row is a deferred node.
  Cells are built only for rows that are read or written. Writes into existing rows expand them;
  new rows are inserted by the lens as today.
- **The row index of PR #29 must be lazy per row.** As written, `SheetIndex::build` computes
  every row's cell positions (`row_cells`) eagerly, calling `has_ac(row)` and
  `row.indexed_elements()`. On a deferred sheet that expands every row at open and erases this
  section's gain. Stage 2x requires the following of ooxml-xlsx:
  1. The sheet-level checks (`SheetErr`: a row without `r`, an invalid `r`, rows out of order) use
     only the row nodes' attributes, which are available without expansion.
  2. `RowEntry`'s cell positions, and its `RowErr`, are computed on the first lookup in that
     row. That is also when today's index reports `RowErr`, so the error semantics do not change.
  3. The bulk cell iterator does not expand rows. Clean rows are read by scanning
     `content_span()` with quick-xml; rows that are already expanded, or dirty, are read through
     the tree. Alternatively, it may release each clean row after reading it (`release_children`).
     Either way, reading every cell must not leave every row expanded.
- These are requirements for ooxml-xlsx to agree to, not decisions this ADR can make. They are
  recorded in the interaction table and must be agreed with the Excel owner before stage 2x
  starts.

#### 2.6 Measured (spike `src/rows.rs`, 1M cells, 100k rows)

- Indexing takes one scan, 156–171 ms unvalidated; validated, per §2.2, about 315 ms. It needs
  1.5 MiB for the spike's index entries; deferred row nodes add about 15–25 MiB (§2.4).
- Finding, expanding, editing and re-rendering 1,000 rows takes 9–22 ms over all runs.
- Splicing the output takes about 4 ms.
- The check confirms T2: every byte outside the edited rows is unchanged, and the output parses
  with core with the new values.
- Order of work for Phase6, per the baseline:
  1. the xlsx row index;
  2. the compression level (§4);
  3. deferred rows (§2), the only route to the memory goal.

  §1 helps every part, but for worksheets most of its effect is superseded by §2.

### 3. Census during the parse (single pass)

`ParseOptions::count: &'static [QName]` makes the parser count elements with those names,
deferred content included (the skip of §2.2 sees every start tag), in the same pass. The counts are reported by `XmlDocument::census(QName) ->
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

- **Which parts.** The level applies to every part that is **written fresh**:
  - dirty parsed parts;
  - `Replaced` parts;
  - parts **added** through core/add-part, which today writes them `Deflated` at the zip
    default.

  Raw-copied parts keep their original compressed bytes, so Layer 1's equality guarantee is
  untouched. A part originally `Stored` stays `Stored`, as today.
- **Validation.** `Level(n)` must be in `0..=9`, else `SaveOptions::new` returns
  `Err(InvalidCompressionLevel(n))`. The named presets map to 1 (`Fastest`), 3 (`Balanced`) and
  9 (`Smallest`).
- Measured on the rewritten 33 MiB sheet:

  | Level | Time | Compressed size |
  |---|---|---|
  | 1 | 72 ms | 5.8 MiB |
  | 3 | 217 ms | 3.3 MiB |
  | 6 (zip default) | 1,079 ms | 3.2 MiB |
  | 9 | 1,709 ms | 3.1 MiB |

- **Measured on real parts** (round-1 reviewer: every XML part of the fixture corpus, 1,590
  parts, 7.96 MB, compressed one by one):
  - level 3 gives 1.440 MB against 1.368 MB at level 6, so it is **5.3% larger**;
  - it is only 1.2× faster (63 ms against 76 ms).

  On small parts the speed gain is small. On large, regular parts (the 33 MiB sheet) it is
  5–6×, with output within 3%.
- **Default:** `Balanced` (level 3); the owner question below records this as the recommended
  answer. Rewriting is expensive exactly on large parts, where level 3 gains 5–6×, and the cost
  on typical parts is about 5% in size. Users who need maximum compression ask for `Smallest`.
- `to_bytes()`/`save()` keep their signatures and use the default. Changing the default changes
  the bytes of rewritten parts only. No oracle compares compressed payloads of rewritten parts,
  and T3 compares only untouched entries.

### 5. Read paths that do not cache

- **`Package::parse_part(&self, name) -> Result<XmlDocument>`** returns a fresh tree, owned by
  the caller. It is defined as parsing the part's **current content**, exactly what
  `read_part(name)` returns, in every state:
  - **Raw:** the original bytes; the part stays `Raw`.
  - **Parsed and clean:** the original bytes; the cached tree is untouched.
  - **Parsed and dirty:** the serialized tree.
  - **Replaced:** its bytes.
  - **Added** (core/add-part): its body.

  It parses with the package's options and the same metadata depth floor as `xml_part` (#16).
  core/add-part's internal `parse_raw` becomes this function, so there is one implementation.
  `parse_part` changes no state and does not bump the generation.
- **`Package::release_part(&mut self, name) -> bool`** drops a `Parsed` tree that has no dirty
  node back to `Raw`.
  - It returns `false`, and does nothing, if the tree is dirty, or if the part is `Replaced` or
    was added (there is no original to fall back to).
  - **It bumps the package generation** (round-1 M4). A core/add-part `RegistryPlan` made before
    the release recorded the part as parsed (`install: None`). Its `apply` would find a `Raw`
    part and skip the registry edit: today a `debug_assert!`, silently in release builds. With
    the generation bumped, such a plan is reported `Stale` and must be re-planned.
- **pptx.** `Presentation::slide` (read) can then stop retaining slides.
  - Recommended (owner question 3): keep `slide()`'s signature and caching, and add an owned
    read, `Presentation::read_slide(&self, i) -> Result<SlideDoc>`, built on `parse_part`,
    and/or `Presentation::release_slide(i)`. No LRU: it complicates borrow lifetimes and needs
    tuning.
  - Changing `Slide<'a>`, which holds `&'a XmlDocument`, into an owned view would break
    temporaries such as `pres.slide(i)?.shapes()`, an API change for ADR-0005's owner.
  - Only slides are read without caching. Layouts, masters and themes are read repeatedly for
    placeholder inheritance and stay cached.
- Layer 1 is unaffected: a part that was only read stays `Raw` and is raw-copied.

### 6. What does not change

- The serialization rule, the dirty-flag duties of the lens primitives, ADR-0002's slots and
  generated views, the depth limit and its lens-insert bound, DTD refusal, and the
  metadata-parsing floor.
- The `Package` part states, except for one new transition, `Parsed{clean} → Raw`
  (`release_part`, which bumps the generation).
- **Relation to core/add-part's states:**
  - `Replaced` parts and added parts are never released and are compressed with the save
    level (§4).
  - `parse_part` reads them as `read_part` does.

## Migration plan

Each stage is one PR, mergeable on its own. Every existing test stays green, and T1/T1b/T2 run
on all fixtures.

| Stage | Content | API impact |
|---|---|---|
| 0 | Land `crates/ooxml-bench` (#24, merged) as the yardstick. Copy today's parser into `ooxml-core/tests/support/frozen_parser.rs` as the oracle for stage 1 (a hand-kept copy, not generated code). Port the spike's checks against it: T1, same tree, rerender equality. | none |
| 1a | Interned `XName` (§1, row 1). | none (`XName` internals; `Debug` output) |
| 1b | `SStr` values, `Arc<str>` source, `u32` spans, boxed attribute slices, exact-size children, and **the `children`/`children_mut` accessors with the field made private** (§1, §2.3). | none: public accessors unchanged |
| 2 | Deferred subtrees (§2) as a non-building mode of the parser loop, plus `xml_part_with`, `is_deferred`, `content_span`, `release_children`; census (§3). | additive |
| 2x | ooxml-xlsx opts in for worksheets (`defer = &[sml:row]`, census of `f`), with PR #29's index made lazy per row (§2.5). | ooxml-xlsx internal; done by or with the Excel owner |
| 3 | `SaveOptions` and the default level (§4); `parse_part`/`release_part` with the generation bump (§5). | additive; default bytes of rewritten parts change |
| 4 | Optional children arena (§1), decided on stage 2's numbers. | lens internals only, thanks to stage 1b's accessors |

### Interaction with open work

| Work | What it touches | Effect of this ADR |
|---|---|---|
| core/add-part (staged `RegistryPlan`) | `apply` uses `a.value.clone_from(ct)`, `e.attrs.iter_mut()`, `root.children.get_mut` directly | Stage 1b: a mechanical change to the accessors and `SStr`. Stage 3: `release_part` bumps the generation, so a plan made earlier is `Stale` rather than skipping an edit (M4); added parts get the save level (§4); its `parse_raw` becomes `parse_part` (§5). |
| #14 `remove_part` | `remove_root_child_with_leading_ws` edits `root.children` | Stage 1b: through `children_mut`. |
| #18 `detach_as`/`insert_detached` | `detach_node` walks `n.children`; `Detached::height()`; builds `RawAttr { name: XName::new(..), value: a.value.clone() }` | Stage 1b: through the accessors (a deferred source subtree is expanded before copying, §2.3). `value.clone()` clones an `SStr`, whose `Arc` keeps the source part's text alive while the copy lives. |
| #19 pptx copy formatting | uses #18 | as #18 |
| #26 `insert_at` | `children.insert` | Stage 1b: through `children_mut`. |
| #29 xlsx row index | `SheetIndex::build` reads every row's children | Stage 2x requirements (§2.5): per-row lazy cell positions, sheet-level checks from row attributes only, bulk reads that do not leave every row expanded. To be agreed with the Excel owner. |
| ooxml-pptx (ADR-0005) | `Slide<'a>` holds `&'a XmlDocument` | Stage 3: an owned `read_slide` and/or `release_slide` is added beside `slide()`; slides only, with layouts and masters still cached (§5). |

### Acceptance for each stage

- `cargo bench -p ooxml-bench` numbers in the PR body against `main`, **in the same session on
  the same host**, best of N over at least 3 repetitions.
- Every existing oracle green.
- **Stage 1:**
  - the parse-equivalence test against the frozen parser (all fixtures and generated inputs);
  - **no parse-time regression** against `main` on the largest docx and pptx fixtures, and on the
    sheets (round-1 m1).
- **Stage 2:**
  - a property test that deferred and full parses serialize identically, with and without edits
    (attribute-only edits on deferred nodes included, §2.3);
  - a property test that errors are identical, kind and position, on corrupted inputs;
  - the depth-limit tests (10k/40k-deep inputs) still refuse at parse time even when the deep
    content is inside a deferred element;
  - no direct `children` access outside the accessor module.

### Targets

All targets are relative to `main` in the same session, and absolute on a quiet host.

- **Stage 1** (all parts):
  - heap of a parsed tree ≤ 1/4 of `main` (measured 1/5 on sheets, 1/2.7 on the largest
    `word/document.xml`);
  - drop time ≤ 1/3 of `main` (measured 1/3.5–1/6 on sheets, 1/5 on the largest `word/document.xml`);
  - parse time no worse than `main` on any measured input.

  The spike's sheet parse is 1.35–1.8× faster after the interner fix, but parse speed is not
  stage 1's target.
- **Stages 2 + 2x** (1M-cell sheet, quiet host):
  - `Workbook::from_bytes` ≤ 0.6 s;
  - heap ≤ 80 MiB;
  - open + edit 1,000 cells + save ≤ 0.5 s at level 3.

  The basis:
  - **Open time:** validated skip about 0.31 s, decompression about 0.05 s, shared strings and
    workbook parts about 0.1–0.2 s *(estimated from the baseline breakdown)*.
  - **Memory:** retained source text 33 MiB, deferred row nodes 15–25 MiB (§2.4), the
    shared-strings tree, and the expanded rows.

  The baseline's calamine full read takes 0.65 s and 77 MiB, so open within 2× of calamine is
  reachable.
- **Memory is not literally "bounded by rows touched".** The source text of every parsed part
  is retained for verbatim output, 33 MiB for this sheet, so memory is bounded by the size of
  the sheet's XML plus the rows touched. The Phase6 wording is met in the tree (no node per
  untouched cell), not in the source. Keeping only the compressed bytes and re-inflating at save
  is a possible later step and is not designed here.

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
  - The largest memory and drop-time costs of a parsed tree are cut without touching any format
    crate's code (stage 1). Parse time itself changes little.
  - Large sheets' trees become row-proportional (stage 2x), which is ADR-0001's stated redesign
    point. The retained source text stays proportional to the part.
  - Save of large edited parts becomes several times faster (stage 3).
  - pptx reads stop retaining every slide.
- **Positive:** the deferred-subtree mechanism is general. docx (body paragraphs) and pptx
  (slide shape trees) can use it later with the same guarantees.
- **Negative:**
  - `SStr` adds an atomic reference-count operation per parsed value (parse and drop). It was
    included in the spike's measurements, which still show parse no slower anywhere and drops
    3.5–6× faster.
  - A value copied into another document keeps that part's source text alive until the copy is
    dropped. This is bounded by the source size, and acceptable for copies of formatting
    subtrees.
- **Negative:**
  - Deferred nodes add a `OnceLock`, a depth and two `Arc`s per deferred element (§2.4), and a
    non-building mode of the parser loop. The parse-equivalence property tests are the guard.
  - Bugs there would surface as differences between deferred and full parses, which that test
    checks on every fixture.
- **Negative:** changing the default compression level changes the bytes of rewritten parts
  (only those). Tools that compare whole files byte for byte after an edit will see different
  output from before.

## Open questions for the project owner

Each question has a proposed answer, the round-1 reviewer's recommendation, awaiting the owner's
decision.

1. **Default compression for rewritten parts.** Proposed: `Balanced` (level 3). It is 5–6×
   faster on large parts, about 5% larger on typical parts, and `Smallest` remains available.
   Tests compare decompressed content, so none breaks.
2. **Stage 4 (children arena).** Proposed: do not rule it out; decide after stage 2's numbers.
   Stage 1b's accessors make it a lens-internal change.
3. **pptx read caching** (ADR-0005's owner). Proposed: keep `slide()` as it is (cached), and add
   an owned `read_slide(&self, i) -> SlideDoc` on `parse_part` and/or `release_slide(i)`. No
   LRU. Layouts and masters stay cached.
4. **Stage 2x requirements on PR #29** (§2.5). To be agreed with the Excel owner before stage 2x.

## Sources

- `docs/testing/performance-baseline-2026-11.md`, `crates/ooxml-bench` (PR #24).
- `spikes/core-performance/` (README, `src/compact.rs`, `src/rows.rs`, `src/main.rs`).
- `docs/adr/0010-opus-review-round1.md` and its probe (validated skip, real-corpus compression
  sizes, PR #29 and core/add-part source references).
- ADR-0001 "Large-sheet performance is explicitly a non-goal for Phase0 and a known future
  redesign point"; ADR-0002 post-acceptance notes (depth limit; detach/insert, PR #18).
- quick-xml 0.42 (`Reader::from_str`, borrowed events, `check_end_names`), zip 9.6 (deflate
  levels via `SimpleFileOptions::compression_level`).
