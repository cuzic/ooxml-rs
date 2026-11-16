# ADR-0010: Core performance: compact Layer 2 tree, deferred subtrees, save and read paths

- Status: **Accepted** (revision 4; review round 3: "Accepted after Minor only: yes")
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

- **Revision 3** (after `0010-opus-review-round2.md`: 1 Major, 6 Minor; taken over from the Word
  owner):
  - **N1 (HashDoS).** Revision 2's unkeyed FNV-1a made parsing quadratic on crafted names
    (18.8 MiB: 111 s). The interner's hasher is now **keyed with a per-process random seed**:
    `foldhash::fast::RandomState` (§1).
    - std's `RandomState` was weighed first, per the std-first policy: it is equally safe, but
      measured about 19% slower than core on the largest `word/document.xml`, which would fail
      stage 1's "no parse regression".
    - The spike is fixed and measured (`src/bin/hashdos.rs`).
    - Stage 1 gains a colliding-input test and a bound for inputs with many distinct names.
  - **N2:** `RawNode` moves to a leaf module (`xml/node.rs`), so privacy really reaches the
    serializer. An unexpanded deferred node whose own attributes changed (`own_dirty`) is a
    defined state with a defined rendering, self-closing nodes included. Attribute edits no
    longer expand (§2.3).
  - **N3:** stage 1b's accessors are operation-level (`insert_child`, `remove_child`,
    `splice_children`, `retain_children`, `set_children`); there is no `&mut Vec`. The
    inventory of direct accesses is completed across main, #18, #38 and #26.
  - **N4:** the targets are reconciled:
    - heap targets per part kind;
    - edit + save excludes open;
    - the open-time basis gains the shared strings, the census's second inflate and the
      deferred-row construction cost, and is marked to be confirmed by a stage-2 breakdown;
    - the 80 MiB heap target requires the shared strings to be read uncached.
  - **N5:** every `RowEntry` value derived from row content is lazy, and bulk reads should use
    `release_children` (or core's non-building mode), not a second decoder (§2.5).
  - **N6:** `ParseOptions` builders, `xml_part_with` and `parse_part_with`, the depth floor, a
    test for parse order, `Option<Box<Deferred>>` with a size assert, a `u32` depth, nested
    deferral (§2.1, §2.4, §5).
  - **N7:** #40 and #36 are in the interaction table.
  - **Nits:**
    - `SaveOptions` is validated in `to_bytes_with` too;
    - `release_part` invalidating unrelated plans is accepted and documented;
    - duplicate `XName`s for one URI are harmless.

- **Revision 4** (after `0010-opus-review-round3.md`: Minor only, "Accepted after Minor only:
  yes"; `origin/main` merged again). Status **Accepted**.
  - **R3-1 / Open question 5:** the interner uses **std's `RandomState` behind a 256-slot front
    cache**, with an intern cap of 16,384 names, as the reviewer recommended. There is no new
    dependency, and it parses as fast as core's parser. foldhash is withdrawn, and its
    description is corrected (seed source, its own HashDoS disclaimer, Zlib licence). The
    threat model is stated. Stage 1 judges collisions by **equality-call counts**
    (deterministic), not time ratios. The distinct-names bound is ≤ 4×. The parse target
    allows 3% noise. The spike is changed accordingly and re-measured (§1).
  - **R3-2:** an unexpanded `own_dirty` node renders exactly like the serializer: an empty
    content range gives `/>`, and the close tag is rendered from the name (option (a)).
  - **R3-3:** `with_prefixes` and every cross-document move expand deeply first. Only
    attribute-value edits and non-declaration attribute additions or removals stay
    unexpanded. A deferred node's source is always its own document's.
  - **R3-4:** `child_mut(i)` and `children_slice_mut()` (length-preserving) for descending;
    descending expands the node passed through, not the target.
  - **R3-5:** 80 MiB is a **peak**. Parts are inflated directly into the shared `Arc<str>`,
    and the input file is in the memory breakdown.
  - **Nits:** the interaction rows use `remove_child`/`insert_child`/`child_mut`;
    `with_max_depth` saturates at `u32::MAX`; the 72 B assert depends on packed flags;
    expanded spans are absolute; `release_children` ignores the node's own `own_dirty`. The
    reviewer's observation that prefix resolution is a linear scan of bindings (quadratic with
    many `xmlns:pN` on one element) predates this ADR and is recorded for the fuzz harness
    (#36), below.

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
| `XName { ns: Arc<str>, local: Box<str>, prefix: Option<Box<str>> }`, 48 B and 1–2 allocations per name | `XName(Arc<XNameData>)`: one thin pointer (8 B), interned per parse by `(prefix, local, ns)`. `XName::new` stays (it allocates; it is only used for inserted nodes). `ns()`, `local()`, `prefix()`, `is()`, `display()` and `Eq`/`Hash` keep their meaning. Equality compares by pointer first, then by content; `Hash` is computed from the content, never the pointer, so it agrees with `Eq`. The derived `Debug` output changes shape (a wrapped struct), which no test depends on. One URI can occur under two addresses (`xmlns=""`, an explicit binding of the `xml` URI), giving two `XName`s with equal content. That is harmless: `Eq` falls back to content, and each `XName` holds its URI's `Arc`, so an address is never reused while a name refers to it (round-2 nit). | Names are immutable once built. Interning is per document, so a subtree copied between documents (`detach_as`, #18) carries its own `Arc`s. |
| `RawAttr.value: String`, `RawText.value: String`, `RawMarkup.raw: Box<str>` | `SStr`: an `Arc<str>` of the part's source plus a `u32` range (24 B, no allocation). When the decoded value differs from its source bytes (entities, CR/LF/tab normalization, text merged across an entity reference), the value is owned instead: the same type, with a buffer of its own. `value()`, `raw()` and `text()` still return `&str` / `Cow<str>`. | Values are only rewritten through the lens primitives, which install an owned `SStr`, and the dirty flags work exactly as today. The `Arc` keeps a value valid when its node is cloned or moved to another document. |
| `XmlDocument.source: Box<str>` | `Arc<str>`, shared by the values. `source() -> &str` is unchanged. | — |
| `ElementSpan { start, open_end, close: Option<(usize, usize)> }` stored as is | Stored as four `u32` offsets, with the two dirty flags packed into the same word. `span()` still returns the public `ElementSpan` with `usize` fields. | Parts are capped at 4 GiB, which zip64 aside is far above any real part. A larger part is refused with a typed error rather than truncated. |
| `attrs: Vec<RawAttr>` (grows while parsing) | A boxed slice, allocated once per start tag with the exact count. The lens's `set_attr`/`remove_attr` rebuild it; edits are rare. `attrs() -> &[RawAttr]` is unchanged. | — |
| `children: Vec<RawItem>` (grows by doubling while parsing) | Collected in a per-depth scratch vector and moved into an exact-capacity `Vec` when the element closes. `RawNode` moves to a **leaf module** (`xml/node.rs`), where the field is private: the parser builds nodes through a `pub(super)` builder, and everyone else, `xml/write.rs` and `xml/mod.rs` included, goes through `children()` and the **operation-level** mutators `insert_child`, `remove_child`, `splice_children`, `retain_children`, `set_children` (revision 3, review N2, N3). No `&mut Vec` is handed out. Stage 4 adds an arena (below). | Every reader and writer of children goes through the accessors, which §2.3 needs. Because no caller holds a `&mut Vec`, stage 4 stays a change inside `xml/node.rs` and the lens. |

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
  - parse time equal to core's or faster (revision 4, std + front cache: 13.8 ms against
    15.4 ms; 14.5–14.6 ms against 14.6–15.1 ms in `hashdos`);
  - drop 5× faster.
- **The interner fix (round-1 m1).** Revision 1's spike was consistently 30–38% *slower* than
  core on that document (21.0 ms against 15.2 ms in the same session). The cause was the spike's
  per-parse name interner, which on every element and attribute formatted a key string
  containing the namespace URI and hashed it with SipHash. Text-heavy WordprocessingML has many
  attributes per element (rsids, `w14:paraId`), so it paid most.
  - The interner now keys on `(prefix, local)` borrowed from the source plus the address of the
    interned namespace URI. A lookup neither formats nor allocates.
- **The interner's hasher is keyed (revision 3, review N1).** The keys are element and attribute
  names, which the part's author controls.
  - Revision 2 hashed them with **unkeyed FNV-1a**. Names whose hashes agree in the low bits are
    cheap to construct (one 4-letter birthday collision per block, chained), and they put every
    name into one probe sequence, so parsing became **quadratic**: 18.8 MiB of such names took
    111 s against 0.37 s for random names.
  - core's current parser has no name hash table, so revision 2 would have **introduced** this
    denial of service.
  - **Threat model (revision 4, review R3-1).** Non-interactive processing of untrusted files:
    the attacker chooses the part's bytes but cannot observe timings while it parses. Every
    parse builds a new table with new keys, and neither hash values nor the table's iteration
    order leave the interner.
  - **Decision (revision 4, adopting the round-3 reviewer's recommendation; Open question 5):**
    **std's `RandomState`** (SipHash-1-3; each instance takes fresh keys from the OS) **behind a
    256-slot front cache**. No new dependency, as CLAUDE.md's std-first policy asks.
    - The front cache is a direct-mapped array of `(key, XName)` indexed by a cheap, unkeyed mix
      of the local name's length, first and last bytes, the prefix length and the URI's
      address. A hit is confirmed by comparing the key. An attacker who chooses names can only
      cause misses, and a miss is an ordinary lookup in the SipHash table, so the worst case is
      std alone: linear.
    - Real parts repeat a few hundred names, so most lookups never reach SipHash. That removes
      std's ~19% cost on text-heavy parts: the cache makes std **as fast as core's interner-free
      parser or faster**.
    - **Intern cap.** At most 16,384 names are interned per parse. Past that, a new name is
      built without being inserted, and lookups continue. Duplicate `XName`s are harmless
      (equality falls back to content, §1 row 1), so the cap changes no behaviour. It bounds the
      cost of parts made of many *distinct* names, where every name is a miss.
    - Revision 3's `foldhash::fast::RandomState` is withdrawn. Its seed is derived from ASLR
      addresses and the clock, not from the OS's random source (`seed.rs`), and its README says
      that "foldhash does not claim to provide HashDoS resistance against interactive
      attackers". Its licence is Zlib only (revision 3 said MIT/Zlib). It is not in the
      dependency tree today. It measured 1–7% slower than core, which revision 3's "no parse
      regression" would itself have failed.
  - **Measured** (`spikes/core-performance/src/bin/hashdos.rs`, revision 4, one session at load
    25–32 on 32 cores, best of 3 or 15; "colliding" = names crafted against revision 2's FNV;
    "random" = as many random names of the same length; "Eq/name" = key equality calls per
    interned name, independent of the host's load):

    | Input | FNV (revision 2) | std alone | **std + front cache + cap** | core |
    |---|---|---|---|---|
    | 65,536 names (4.2 MiB), colliding / random | 285 / 47 ms; Eq/name 127.9 / 0.09 | 45.6 / 44.9 ms; 0.095 / 0.093 | **38.4 / 41.9 ms; 0.094 / 0.093** | 17.2 / 16.9 ms |
    | 262,144 names (18.8 MiB), colliding / random | quadratic (not run) / 130 ms | 100 / 133 ms; 0.070 / 0.071 | **111 / 114 ms; 0.071 / 0.070** | 75 / 64 ms |
    | 262,144 distinct random names | — | 74–169 ms | **1.05–1.72× core** | 47–82 ms |
    | 65,536 distinct random names | — | 16–30 ms | **1.8–2.1× core** | 7.3–14 ms |
    | largest `word/document.xml` (1.3 MiB), best of 15, 5 rounds | 15.8–16.4 ms | 18.6–18.8 ms | **14.5–14.6 ms** | 14.6–15.1 ms |

    - Colliding names cost the same as random ones, in time and in equality calls. With
      revision 2's FNV they cost 1,370× more equality calls per name (511 per name without the
      cap; the cap also bounds that chain).
    - The round-3 reviewer measured the same ordering: std + front cache 13.3–13.9 ms, core
      14.3–14.9 ms, foldhash 15.3–15.6 ms, std alone 17.3–18.2 ms.
  - **Many distinct names are a cost of interning**, now bounded by the cap: 262,144 distinct
    names parse in 1.05–1.72× core's time (revision 3 with foldhash and no cap: 2.4–2.8×).
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
- **Builders (revision 3, review N6).** `ParseOptions` is `#[non_exhaustive]`, so other crates
  cannot write a struct literal. Core adds `with_defer(&'static [QName])` and
  `with_count(&'static [QName])` beside `with_max_depth`. #40's `max_part_bytes` and
  `max_package_bytes` get builders of their own, and the fields do not overlap.
- An element whose name is in `defer` is a **deferred node**:
  - the parser builds the node itself: name, attributes, span;
  - it does not build the children;
  - the content stays in the source until it is asked for.
- **Opt-in per part, not per package.** The package keeps one default `ParseOptions`.
  `Package::xml_part_with(name, &ParseOptions)` parses one part with other options, for example
  ooxml-xlsx's worksheets with `defer = &[sml:row]`.
  - It applies the same **metadata depth floor** as `ensure_parsed` (#16), so a caller cannot
    lower the depth limit below it.
  - An already-parsed part keeps the options it was parsed with. That is order-dependent: if any
    path calls `xml_part(sheet)` first, the deferral is silently lost. Only performance suffers,
    but stage 2x's acceptance includes a test that **after `Workbook::open`, every worksheet's
    root holds deferred rows**.
  - Deferring package-wide would also defer `sheetData/row` in external-link parts, which is
    harmless but pointless, so it is not done.
- **Nested deferral (N6).** When a deferred element is expanded, its descendants whose names are
  in `defer` are deferred again. Expansion is one level at a time, and laziness is kept.

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

#### 2.3 Invariant: a deferred node's *children* are never changed unexpanded

- A deferred node's children must never be changed while unexpanded. If they were, today's
  serializer would render `own_dirty` with "no children" and write `<row …/>`, losing every cell.
- Therefore:
  - **Reads:** `children(&self)` expands on first use into the node's `OnceLock<Vec<RawItem>>`.
    Expansion cannot fail, because §2.2 already validated the content.
  - **Child writes:** every operation-level mutator (`insert_child`, `remove_child`,
    `splice_children`, `retain_children`, `set_children`; §1) expands first. So does every
    lens path that **descends** into a deferred node.
  - **Attribute-only writes do not expand (revision 3, review N2, option b).** The lens's
    `set_attr`/`remove_attr` and #38's `apply` (`e.attrs.iter_mut()`, `own_dirty = true`) may
    change a deferred node's **own attribute values**, or add and remove its attributes other
    than namespace declarations. They leave the node unexpanded and `own_dirty`. That is a
    **defined state** with a defined rendering, not an invariant violation. It is also what
    makes `row/@ht` cheap.
  - **Everything else expands deeply first (revision 4, review R3-3).** That covers an
    operation that changes an element's name or prefix, and any move of a subtree into
    another document (#18's `detach_as`/`insert_detached`, whose `with_prefixes` rewrites the
    element's own prefix and every descendant's). The node and every deferred node inside it
    are expanded, nested deferrals included, before the operation runs: copied raw content
    would carry the source document's prefix bindings into a document where they may be
    unbound or bound to another URI.
  - **Invariant:** a deferred node's `Deferred.source` is the source of the document that holds
    it. `debug_assert!`ed when a subtree is inserted, so a cross-document deferred node cannot
    exist.
  - **Descending (revision 4, review R3-4).** The lens descends by `&mut` one level at a time
    (`LensMut::node_mut`, `mark_path`; #38's `apply`, edits after #26's `insert_at`). Stage 1b
    therefore adds `child_mut(i) -> Option<&mut RawItem>` and
    `children_slice_mut() -> &mut [RawItem]`. Both expand the node first and cannot change the
    number of children, so a stage-4 arena (a contiguous range) supports them. Descending
    expands the node descended **through**, never the node it lands on. For `row/@ht`, the lens
    takes `&mut` to a child of `sheetData`, which expands `sheetData`'s list (already built)
    but not the `row`.
- **Rendering an unexpanded, `own_dirty` deferred node** (`xml/write.rs`; revision 4, review
  R3-2 option (a): the same rules as the serializer's `own_dirty` path):
  - the re-rendered open tag (new attributes);
  - if the content range `source[open_end..close.0]` is **empty** (`<row r="1"></row>`, or
    self-closing in the source): `/>`, as the serializer writes an `own_dirty` node without
    children;
  - otherwise the content copied **verbatim from the node's own `Arc<str>` source**, then a
    close tag **rendered from the name** (`</row>`), not copied (`</row >` in the source
    becomes `</row>`).

  This is byte-identical to expanding and rendering clean children. Revision 3 copied the
  source's close tag and kept an empty `<row …></row>`. Both differ from the serializer's
  output, and the round-3 reviewer's `fallback.rs` probe showed stage 2's property test would
  fail on them. Every clean child, text and
  markup is emitted from its span, and together they cover `open_end..close.0` without gaps
  (verified by the round-2 reviewer; spans of expanded content are **absolute** offsets into
  the part's source, including inside nested deferrals, which T2 and this rendering rely on).
  The prefix bindings of the content do not change, because
  #40's lens refuses adding or removing `xmlns` declarations (`NamespaceDeclaration`), so the
  re-rendered open tag binds the same prefixes. No `debug_assert!` fires on this path.
- **Enforced by privacy, for real (N2).** `RawNode` is defined in the leaf module `xml/node.rs`.
  Its `children` field is private there, so `xml/write.rs`, `xml/parse.rs` and `xml/mod.rs` (for
  example `structurally_eq`, `force_rerender`, `node_at`/`node_at_mut`, `text()`) can no longer
  touch it. The parser builds nodes through a `pub(super)` builder. `attrs` and the dirty flags
  stay `pub(crate)`, which option (b) makes safe.
- **Every direct `children` access to migrate (N3)**, by PR. The compiler finds all of them once
  the field is private; the list tells each PR's owner the size of the change:

  | Where | Direct accesses |
  |---|---|
  | main, `lens.rs` | `Lens::node`; `LensMut::node_mut` and `mark_path` (L134, L234, L569: the main `&mut` descent); the `first` check (L267); `place` (L376); `set_text` (L538–548) |
  | main, `xml/mod.rs`, `xml/write.rs` | `node_at`/`node_at_mut` (L274, L280), `structurally_eq`, `force_rerender`, `text()`, the serializer |
  | #14 `remove_part` | `remove_root_child_with_leading_ws` |
  | #18 `detach_as` | `detach_node` (`out.children = children`), `with_prefixes` (`mem::take(&mut n.children)`), `ancestor_context`, `element_at`, `has_text`, `declarations`, `insert_detached` (L746–749), `Detached::height` |
  | #38 `RegistryPlan` | `element()`/`append_root_child` (`root.children.splice`), the `Id` attribute rewrite, `self.root.children`, `apply` |
  | #26 `insert_at` | `check_insert_at` (`&self.node().children`), `insert_at` (`children.insert`) |

  `splice_children`/`set_children` cover the whole-list rewrites of #18 and #38.
- **Acceptance (stage 2):**
  - `RawNode` lives in `xml/node.rs`, and the field's privacy is the proof that no other module
    reads `children` directly;
  - property tests that change only the attributes of deferred nodes, with no descent, and check
    that the output keeps their content, for self-closing rows too.

#### 2.4 What a deferred node carries

`RawNode::children(&self)` has no access to the document, so a deferred node holds what
expansion needs, **in a box** (revision 3, review N6): `RawNode` keeps one
`Option<Box<Deferred>>` (8 B, `None` for every ordinary node). The alternative layout is
`children: enum { Built(Vec<RawItem>), Deferred(Box<Deferred>) }`. Putting the payload inline
would bring every node back to about 150 B and erase stage 1's gain. A compile-time
`const { assert!(size_of::<RawNode>() <= 72) }` guards it. 72 B holds only with the two dirty
flags packed into the span's word (§1); as separate `bool`s the node is 80 B (round-3 reviewer's
`size/s.rs`), so the assert failing points there first. `Deferred` holds:

- the source (`Arc<str>`, shared);
- the parse options (`Copy`);
- its **depth** (`u32`; revision 2's `u16` would truncate a `with_max_depth` above 65,535):
  expansion counts the limit from there. `with_max_depth(usize)` **saturates** at `u32::MAX`
  (a limit that large refuses nothing a real part has; revision 4 nit);
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
  It returns `false`, and does nothing, if anything **inside** is dirty. The node's own
  `own_dirty` (an attribute-only edit) does not prevent release: its children are clean, and
  §2.3's rendering of an unexpanded `own_dirty` node is byte-identical (revision 4 nit).

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
  2. **Every `RowEntry` value derived from the row's content** is computed on the first lookup
     in that row (revision 3, review N5). That covers:
     - the cell positions and `RowErr`;
     - `alternate` (`has_ac`: an `mc:AlternateContent` inside the row);
     - `cells_end` and `misordered` (`row_tail`).

     That is also when today's index reports `RowErr`, so the error semantics do not change.
  3. The bulk cell iterator does not leave every row expanded. **Recommended:** expand a clean
     row, read it through the tree, and `release_children` it afterwards.
     - If ooxml-xlsx scans `content_span()` instead, it uses core's non-building mode (§2.2),
       exposed for that purpose, not a second value decoder of its own. A second decoder
       (entities, CR/LF normalization, text merged across entities) could disagree with the
       tree.
     - Rows already expanded, or dirty, are read through the tree.
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

`ParseOptions::count: &'static [QName]` (builder `with_count`) makes the parser count elements with those names,
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
- **Validation.** `Level(n)` must be in `0..=9`, else `Err(InvalidCompressionLevel(n))`.
  - `SaveOptions::new` checks it.
  - **So do `to_bytes_with` and `save_with`** (revision 3, nit): `compression` is a public field,
    and `#[non_exhaustive]` forbids building the struct elsewhere, but not assigning the field.
  - The named presets map to 1 (`Fastest`), 3 (`Balanced`) and 9 (`Smallest`).
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
- **`Package::parse_part_with(&self, name, &ParseOptions)`** (revision 3, review N6) is the
  same with explicit options, under the same depth floor. A worksheet read uncached therefore
  defers its rows too.
- **`Package::release_part(&mut self, name) -> bool`** drops a `Parsed` tree that has no dirty
  node back to `Raw`.
  - It returns `false`, and does nothing, if the tree is dirty, or if the part is `Replaced` or
    was added (there is no original to fall back to).
  - **It bumps the package generation** (round-1 M4). A core/add-part `RegistryPlan` made before
    the release recorded the part as parsed (`install: None`). Its `apply` would find a `Raw`
    part and skip the registry edit: today a `debug_assert!`, silently in release builds. With
    the generation bumped, such a plan is reported `Stale` and must be re-planned.
  - That also invalidates **unrelated** plans, for example a `release_slide` in the middle of
    planning. This is accepted and documented (round-2 nit). The alternative, `apply`
    re-parsing a `Raw` part when `install: None`, cannot fail, since those bytes parsed before,
    but it hides a surprising state change, so it is not chosen.
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
| 1b | `SStr` values, `Arc<str>` source, `u32` spans, boxed attribute slices, exact-size children, and **`RawNode` moved to `xml/node.rs` with the field private, `children()` and the operation-level mutators** (§1, §2.3). | none: public accessors unchanged; crate-internal callers (#14, #18, #26, #38) move to the mutators |
| 2 | Deferred subtrees (§2) as a non-building mode of the parser loop, plus the `with_defer`/`with_count` builders, `xml_part_with`, `is_deferred`, `content_span`, `release_children`; census (§3). | additive |
| 2x | ooxml-xlsx opts in for worksheets (`defer = &[sml:row]`, census of `f`), with PR #29's index made lazy per row (§2.5). | ooxml-xlsx internal; done by or with the Excel owner |
| 3 | `SaveOptions` and the default level (§4); `parse_part`/`parse_part_with`/`release_part` with the generation bump (§5). | additive; default bytes of rewritten parts change |
| 4 | Optional children arena (§1), decided on stage 2's numbers. | `xml/node.rs` and lens internals only, because stage 1b's mutators hand out no `&mut Vec` |

### Interaction with open work

| Work | What it touches | Effect of this ADR |
|---|---|---|
| core/add-part (staged `RegistryPlan`) | `apply` uses `a.value.clone_from(ct)`, `e.attrs.iter_mut()`, `root.children.get_mut` directly | Stage 1b: a mechanical change to the accessors (`child_mut` for `root.children.get_mut`, §2.3) and `SStr`. Stage 3: `release_part` bumps the generation, so a plan made earlier is `Stale` rather than skipping an edit (M4); added parts get the save level (§4); its `parse_raw` becomes `parse_part` (§5). |
| #14 `remove_part` | `remove_root_child_with_leading_ws` edits `root.children` | Stage 1b: through `remove_child`. |
| #18 `detach_as`/`insert_detached` | `detach_node` walks `n.children`; `Detached::height()`; builds `RawAttr { name: XName::new(..), value: a.value.clone() }` | Stage 1b: through the accessors (a deferred source subtree is expanded before copying, §2.3). `value.clone()` clones an `SStr`, whose `Arc` keeps the source part's text alive while the copy lives. |
| #19 pptx copy formatting | uses #18 | as #18 |
| #26 `insert_at` | `children.insert` | Stage 1b: through `insert_child` (and `child_mut`, §2.3, for edits after the insert). |
| #29 xlsx row index | `SheetIndex::build` reads every row's children | Stage 2x requirements (§2.5): per-row lazy cell positions, sheet-level checks from row attributes only, bulk reads that do not leave every row expanded. To be agreed with the Excel owner. |
| ooxml-pptx (ADR-0005) | `Slide<'a>` holds `&'a XmlDocument` | Stage 3: an owned `read_slide` and/or `release_slide` is added beside `slide()`; slides only, with layouts and masters still cached (§5). |
| #40 `fix/core-fuzz-findings` | adds `ParseOptions::max_part_bytes`/`max_package_bytes`; new parser checks (BOM, NUL, `PositionMismatch`, `EmptyName`, `EmptyPrefixBinding`, XML-declaration position); the lens's `NamespaceDeclaration` refusal | `defer`/`count` are laid out beside the new limits, each with a builder (§2.1). Every new check sits on the **shared** path of the parser loop, which §2.2's "same errors by construction" relies on: the round-2 reviewer's probe found 49 of 49 malformed inputs giving the same error in both modes. The `NamespaceDeclaration` refusal is a premise of §2.3's rendering of `own_dirty` deferred nodes. |
| #36 fuzz harness | fuzz targets and a corpus for core, xlsx and pptx | Stage 2's "identical errors" property test takes its inputs from the fuzz corpus as well as from mutated fixtures. Also a candidate input there (revision 4, round-3 observation, outside this ADR): many `xmlns:pN` declarations on one element used by many attributes, since prefix resolution (`Scope::lookup`) scans the bindings linearly. |

### Acceptance for each stage

- `cargo bench -p ooxml-bench` numbers in the PR body against `main`, **in the same session on
  the same host**, best of N over at least 3 repetitions.
- Every existing oracle green.
- **Stage 1:**
  - the parse-equivalence test against the frozen parser (all fixtures and generated inputs);
  - **no parse-time regression** against `main` on the largest docx and pptx fixtures, and on the
    sheets (round-1 m1);
  - **hostile names stay linear** (revision 3, N1; revision 4, R3-1: judged by a count, not a
    time ratio). The test counts the interner key's equality calls, which a test-only `Eq`
    wrapper exposes. On a part of 262,144 element names crafted to collide under an unkeyed
    hash (`hashdos.rs`'s FNV construction), equality calls per interned name are **≤ 1** and
    **≤ 1.5×** those on the same number of random names. Measured 0.071 against 0.070. This is
    deterministic, so CI can check it. The time ratios (colliding within 1.25× of random;
    4× the names at most 5× the time) are reported on a quiet host as reference values only;
  - **many distinct names have a bounded cost**: the interner holds at most 16,384 names (a
    unit test), and on 262,144 distinct random names parse time is at most **4×** `main`'s
    (revision 4, R3-1; measured 1.05–1.72× with the cap).
- **Stage 2:**
  - a property test that deferred and full parses serialize identically, with and without edits
    (attribute-only edits on deferred nodes included, §2.3);
  - a property test that errors are identical, kind and position, on corrupted inputs;
  - the depth-limit tests (10k/40k-deep inputs) still refuse at parse time even when the deep
    content is inside a deferred element;
  - no direct `children` access outside `xml/node.rs` (privacy);
  - stage 2x: after `Workbook::open`, every worksheet's root holds deferred rows (N6).

### Targets

All targets are relative to `main` in the same session, and absolute on a quiet host.

- **Stage 1** (revision 3, review N4: per part kind):
  - heap of a parsed tree: **≤ 1/4 of `main` for worksheets** (measured 1/5);
    **≤ 1/2.5 for docx and pptx parts** (measured 1/2.7 on the largest `word/document.xml`,
    1/2.85 on a pptx slide);
  - drop time ≤ 1/3 of `main` (measured 1/3.5–1/6 on sheets, 1/5 on the largest `word/document.xml`);
  - parse time no worse than `main` on any measured input, **within a 3% noise allowance**
    (revision 4, R3-1), and the two hostile-input bounds of the stage-1 acceptance.

  The spike's sheet parse is 1.35–1.8× faster after the interner fix, but parse speed is not
  stage 1's target.
- **Stages 2 + 2x** (1M-cell sheet, quiet host):
  - `Workbook::from_bytes` ≤ 0.6 s, **to be confirmed by a stage-2 breakdown measurement**
    before stage 2x starts (see the basis);
  - **peak** heap ≤ 80 MiB during open, with the shared strings read **uncached** (below).
    Peak, because the baseline (`PeakAlloc`, calamine's 77 MiB) is a peak (revision 4, review
    R3-5). This **requires** decompressing a part directly into its shared source: an
    `Arc::new_uninit_slice` of the declared size, filled by the inflater, UTF-8-checked, then
    used as the `Arc<str>`. The current `parse_with(&[u8])` copies the inflated bytes, so the
    33 MiB `Vec<u8>` and the 33 MiB `Arc<str>` would be alive at once (peak 85–95 MiB);
  - **edit 1,000 cells + save ≤ 0.5 s at level 3, open excluded** (revision 2's "open + edit +
    save ≤ 0.5 s" contradicted the 0.6 s open target).

  The basis:
  - **Open time** (revision 3, N4). The parts:
    - validated skip, about 0.31 s;
    - decompression, about 0.05 s;
    - constructing the 100k deferred row nodes. That is not in the skip's figure: the round-2
      reviewer's probe, building row nodes in today's representation, took 0.39–0.42 s for
      skip plus rows, at load 40–48;
    - the shared strings, 50–66 ms as a compact tree (88–100 ms in core);
    - the `Vec<String>` built from them.

    The census's second inflate of the sheet (`formula_census` re-reads it, about 48 ms) goes
    away with §3's in-parse census. The baseline's open residual is
    1.95 − 1.25 − 0.29 − 0.05 ≈ 0.36 s.

    0.6 s is plausible on a quiet host but has no margin. Stage 2 measures this breakdown, and
    the target is revised then if needed.
  - **Memory** (peak during open):
    - the input file, which `Package` holds as `Arc<[u8]>`: 3.4 MiB;
    - retained source text: 33 MiB, with no second copy (above);
    - deferred row nodes: 15–25 MiB (§2.4);
    - the expanded rows;
    - the shared strings, which are the reason for the second stage-2x requirement below. The
      1M-cell sheet's `sharedStrings.xml` (2.4 MiB) is a **23 MiB** compact tree (131 MiB in
      core), plus about 4 MiB of `Vec<String>`. Kept in `Package`, that would put the total at
      75–85 MiB before any row is expanded.
  - **Stage 2x therefore also requires** ooxml-xlsx to read `sharedStrings.xml` with
    `parse_part` (§5, uncached) into its `Vec<String>`, and to take the formula census during
    the worksheet parse (§3). That brings the estimate to about 60 MiB.

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
5. **The interner's hasher** (revision 3, N1). **Decided in revision 4 (coordinator, adopting
   the round-3 reviewer's recommendation):** `std::hash::RandomState` behind a 256-slot front
   cache, with an intern cap of 16,384 names (§1). There is no new dependency, and parsing is
   as fast as core's parser. Revision 3's `foldhash` proposal is withdrawn.

## Sources

- `docs/testing/performance-baseline-2026-11.md`, `crates/ooxml-bench` (PR #24).
- `spikes/core-performance/` (README, `src/compact.rs`, `src/rows.rs`, `src/main.rs`,
  `src/bin/hashdos.rs` since revision 3).
- `docs/adr/0010-opus-review-round2.md` and its probes (`probe2.rs`: the FNV collision
  construction; `probe3.rs`: the shared-strings tree size; `pr40/` + `skipprobe/`: error
  equality of the non-building mode on #40's parser).
- `docs/adr/0010-opus-review-round3.md` and its probes (`r3.rs`: hashers, front cache and
  equality counts; `compact_fc.rs`: the front-cache prototype; `fallback.rs`: rendering of
  unexpanded `own_dirty` nodes; `size/s.rs`: `RawNode` layouts).
- foldhash 0.2.0 (`seed.rs`, `fast.rs`, README; read for revision 3's proposal, withdrawn in
  revision 4).
- `docs/adr/0010-opus-review-round1.md` and its probe (validated skip, real-corpus compression
  sizes, PR #29 and core/add-part source references).
- ADR-0001 "Large-sheet performance is explicitly a non-goal for Phase0 and a known future
  redesign point"; ADR-0002 post-acceptance notes (depth limit; detach/insert, PR #18).
- quick-xml 0.42 (`Reader::from_str`, borrowed events, `check_end_names`), zip 9.6 (deflate
  levels via `SimpleFileOptions::compression_level`).
