# ADR-0024: xlsx Edit Journal — immutable base, typed journal, view, shadow commit

- Status: **Accepted** (revision 7; Opus review rounds 1–6, owner decisions 2026-11-25);
  revision 8 (Commit = serialization of the shadow, §7) **Proposed**: it awaits its Opus review
  and the owner's confirmation of Q7. Until Q7 is confirmed, revision 7's compiled Commit
  (now Appendix A) is the accepted decision.
- Date: 2026-11-25
- Repository baseline: `f88d02c` (main after PR #140). The spike of revision 8 builds against
  the same crates (PR #153 changed only this ADR).
- Depends on:
  - [ADR-0001](0001-lossless-round-trip-architecture.md): Layer 1–3, T1/T1b/T2 (locality),
    `Err` ⇒ byte-identical output, fail closed.
  - [ADR-0003](0003-xlsx-phase1-formula-and-calc-state.md): census, plan → validate → apply with
    poisoning (§3.1), the multi-cell write plan (§3.1a), dependents and cache strips (§3.2,
    §3.3), shared-group hand-off (§4.2), calcChain (§4.3), guards (§5).
  - [ADR-0006](0006-xlsx-phase2-shared-strings-and-styles.md): shared strings and styles,
    `sst/@count`, append-only registries.
  - [ADR-0009](0009-xlsx-phase3-sheet-operations.md): sheet operations, one `RegistryPlan` per
    operation (§2), `sheetId` and names for new sheets (§3).
  - [ADR-0014](0014-xlsx-phase4-range-features.md): range features and the allowed-diff data
    table (§2).
  - [ADR-0017](0017-xlsx-phase5-objects.md): object plans (§2.1), id allocators (§2.2).
  - [ADR-0018](0018-xlsx-phase7-structural-edits.md): structural edits, the reference model
    (§3.2), the location registry (§2.2), the cache rule (§3.7), in-place apply and the
    rejected clone (§2.1), performance (§3.8).
- Related, **not on `main`** (uncommitted ADRs on their plan branches):
  - **ADR-0021** (browser XLSX APIs compatible with SheetJS CE and ExcelJS) and **ADR-0022**
    (Rust worksheet projection and generated JavaScript adapters), branch
    `plan/xlsx-js-compat`. This ADR replaces parts of them (§13.3).
  - **ADR-0023** (xlsx range reads and writes at workbook scope, and the Python binding
    policy), branch `plan/xlsx-rpa-foundation`. It is rewritten on this basis now that this
    ADR is accepted (owner decision Q4, §13.4).
- Evidence: the annotated bibliography compiled for this ADR (67 entries, 58 verified against
  a primary source) is not in the repository. Entries cited here are listed in
  [Sources](#sources) under the same numbers (`[2-6]`, `[3-12]`, …). ExcelJS 4.4.0 behaviour
  is cited from its source (`lib/doc/worksheet.js`) and a probe run for review round 1.
  Revision 8 rests on the spike in [`spikes/xlsx-edit-journal/`](../../spikes/xlsx-edit-journal/README.md):
  commit `cb57de4` (the compiled Commit, J0/J1 keyed slice) and commit `ad74d32` (Commit =
  shadow serialization with the four rules of §7.1). Its findings are cited as F1–F14.
- Review: [round 1](0024-opus-review-round1.md), [round 2](0024-opus-review-round2.md),
  [round 3](0024-opus-review-round3.md), [round 4](0024-opus-review-round4.md),
  [round 5](0024-opus-review-round5.md), [round 6](0024-opus-review-round6.md); revision 8:
  round 7 onward (pending).

## Revision history

Section numbers in the entries for revisions 1–7 refer to the layout of their time. Revision 8
moved revision 7's §4.2 `start`, §4.4, §7, §7.1, §7.2 and the Commit-time part of §9.2 to
[Appendix A](#appendix-a-unimplemented-alternative--the-compiled-commit) (mapping in its entry).

- **Revision 1**: initial draft. Decides the open design items 4–7 of the research notes
  (§4.5, §10, Test oracle 1, §5.1/§5.6). Leaves the formula-cache policy, the bias table and
  the hidden-information table to the owner with defaults (Q1–Q3).
- **Revision 2** (Opus review round 1: B1–B3, M1–M5, m1–m6, n1). The main change is a
  **shadow workbook** (§3.2): each accepted op is applied at once to a `Workbook` through the
  sequential API, which is the validation ground and the source of normalized reads. Commit
  still compiles the normal form from the Base bytes on a fresh working copy.
  - **B1:** formula text crosses a structural op only through ADR-0018's own rewriter: at
    append on the shadow, at Commit on the working copy. A formula entry is written in its
    birth-epoch coordinates and never mapped by a step map (§5.3, §7 step 4, §8.1). Oracle 1
    has the double-shift case; oracle 9 has the mutant.
  - **B2:** the Journal is a log; nothing folds at append. Folding happens only inside
    `normalize`, on a copy, at Commit. The View's key index is derived from the log (§4.3).
    Oracle 8 truncates across repeated writes to one key.
  - **B3:** style writes are patches, composed by a rule; only a full replacement drops
    earlier patches (§4.4). Commit builds batches in which a cell appears once; a content
    write and one style patch share a cell's write; any further patch opens a new batch
    (§7.1). Oracle 1's generator has content + style on one cell and patch chains.
  - **M1:** §4.1 splits ops into part A (sequential API on `main`) and part B (needs a new
    Rust operation, listed as Delivery prerequisites; refused with `Unsupported` until then).
    Under Q2 (A) a splice is delete + insert + writes, all part A, so it is in oracle 1 now
    (§8.2).
  - **M2:** the validation ground is the shadow workbook: an op is refused iff the sequential
    API refuses it on the shadow, by construction. A structural op costs one ADR-0018
    operation at append and one at Commit; no dry run, no replay per append. ExcelJS
    `insertRow`/`insertRows`/`duplicateRow` all call `spliceRows`, so structural ops are
    frequent; their cost is measured in J0 (§4.5, Test oracle 10).
  - **M3:** oracle 1 compares a projection with fixed inclusion and exclusion rules. Each
    exclusion is replaced by a validity predicate checked on both sides (calc state, unused
    registry entries, calcChain). Oracle 5 is about the Journal alone and no longer conflicts
    (Test oracle 1).
  - **M4:** no `Arc`-shared copy-on-write. The working copy is `Workbook::from_bytes(Base)`.
    The shadow is dropped before the working copy opens, so at most one parsed copy is alive.
    The relation to ADR-0018's 289 ms / 2× clone figure is in §7.2.
  - **M5:** `truncate` below the last Commit is refused (`TruncateBeforeCommit`). Whether a
    transaction may contain a save is Q5 (§10.3).
  - **m1:** ExcelJS 4.4.0 `spliceRows` facts replace "unverified" (§8.2, Q2).
  - **m2:** keeping a supplied formula result is a separate option, Q1 (D), that changes
    ADR-0003 §3.1a.
  - **m3:** ids are logical positions of the session's first Base; vectors, not the file,
    give current positions. Ids pushed off the sheet by an insert die (§3.1, §5.2).
  - **m4:** the read set of N3 is replaced by a conservative rule on what may move (§4.4 N3,
    N7).
  - **m5:** a row or cell without `r` has the implicit position's id (the claim that keyed
    writes work there was wrong; corrected in revision 3, m4).
  - **m6:** a refused op in a buffered flush keeps the accepted prefix (§11; replaced in
    revision 3, M5).
  - **n1:** the item mapping in revision 1's entry is corrected (§5.1/§5.6).
  - `CommitInternal` is renamed `JournalInternal`, because it also covers flush, replay and
    a poisoned shadow (§12).
- **Revision 3** (Opus review round 2: M1–M5, m1–m4, n1). Commit's three reorderings (N3,
  N1, §7.1 merging) now read the key's **start state**, recorded from the shadow (§4.2); a
  call is atomic on the shadow however many operations it runs (§4.5).
  - **M1:** N3 (iv): an entry on a cell that held a formula at the start of its epoch, on any
    sheet, never moves; the claim "wider than ADR-0018 reads" is replaced by the read set.
    Oracle 1 gets 3-D, sheet-edge and `delete_sheet_strict` fixtures and fixed cases; oracle 9
    the mutant.
  - **M2:** N1 is "an unconditional full write drops earlier content ops"; `remove_formula`
    (also in `write_cells`) is conditional, drops nothing and compiles after its predecessor
    (§4.4 table, §7.1). Oracle 1 fixed case.
  - **M3:** a content op that creates a cell writes no `s`. Such a content entry is written
    before every patch on that cell; a chain starting on a missing cell with a patch does not
    move across a structural op (§4.4, §7.1). Generator: missing cells in formatted rows/cols.
  - **M4/M5:** a call (one op, a splice, a `write_cells`, a flush) is all-or-nothing:
    on a refusal the log, counters and shadow return to before the call, the shadow by replay
    (§4.5). A failed flush also **breaks** the Journal until truncate or reload (§11,
    `JournalBroken`); new Q6. Oracle 3 cases for both.
  - **m1:** registry indices read from the View are valid until the next Commit only; the
    Reload law excludes them (§6.1).
  - **m2:** P is a listed model, h1 and sheet views included; every unmodeled part is
    compared byte-for-byte; `dimension` and `app.xml` have explicit rules (Test oracle 1).
  - **m3:** "echo ended" is stored as the ending `seq` and cleared by truncation (§6.1, §6.3);
    oracle 2 has the rollback case.
  - **m4:** writes to rows or cells without `r` are refused, as by the sequential API (§3.1).
  - **n1:** `JournalInternal { seq, source }` everywhere.
- **Revision 4** (Opus review round 3: M1, M2, m1–m3, n1).
  - **M1:** an op of a two-half call (`set_datetime`, `*_with_format`) is outside folding:
    it is never dropped, drops nothing, and no fold reaches across it; the base of a missing
    cell is decided by the key's first op in `seq` order before folding (§4.4, §7.1). Oracle 1
    has both orders on a missing cell in a styled column; oracle 9 the mutant.
  - **M2:** the metadata part is modeled by its referenced blocks; its presence, relationship,
    content type and unused blocks are excluded with a predicate (Test oracle 1). §4.4 lists
    the workbook-scoped side effects of content ops against the exclusions. Oracle 1 fixed case.
  - **m1:** a flush refused part-way returns `JournalInternal` with the refusal as `source`;
    oracle 3 compares the internal state, not reads (§4.5, §11).
  - **m2:** a call's rollback lists what it restores (log, key index, `seq` counter, vectors,
    tombstones, echo ends, shadow); the id counter is not restored, ids the call took are
    discarded (§4.5, §9.3).
  - **m3:** `dimension` is in P: Commit sets each sheet's `dimension/@ref` to the shadow's
    (§7 step 4). A formula cell's value and `t` belong to the cache exclusion (Test oracle 1).
  - **n1:** oracle 9 list indentation.
- **Revision 5** (Opus review round 4: M1, M2, m1–m3, n1, n2).
  - **M1:** a style patch never merges into the write of a conditional content op
    (`remove_formula` skips the whole write, style included, on a cell without a formula); no
    later item for the conditional op's cell joins its batch (§7.1). Oracle 1 fixed cases; oracle 9 mutant.
  - **M2:** revision 4's "copy the shadow's `dimension`" is withdrawn. The working copy keeps
    `dimension` by the sequential API's own rule over the writes it performs, so a skipped
    no-op grows nothing; `dimension` moves to P's exclusions with predicates, and the Reload
    law gets a `!ref` exception (§6.1, §7, §9.2, Test oracle 1). Oracles 1, 4, 5 now hold
    together; fixed case: `dimension` `A1`, the Base value 7 written back to `C3`.
  - **m1:** the metadata predicate "adds no part" applies to the Journal side only.
  - **m2:** the base of a missing cell skips a conditional first op (it creates nothing).
  - **m3:** a two-half op joins a batch only when its cell is not yet in it (§7.1).
  - **n1/n2:** §12 `JournalInternal` includes a refused flush; §7 step 2 records nothing
    from the shadow any more, so a stale shadow needs no replay there.
- **Revision 6** (Opus review round 5: M1, m1–m3, n1).
  - **M1:** §9.2 skips an entry only when its cell **exists** in the working copy and its
    content and resolved `xf` equal the cell's; an entry on a missing cell is never a no-op,
    so it creates the cell as sequentially. P models an empty `c` as a cell (Test oracle 1).
    Oracle 1 fixed case: `set_style_index(0)` on missing `B5` under a bold `col/@style`;
    oracle 9 mutant: no-op test on a missing cell. `dimension` grows for such a write on both
    sides, as the predicate expects.
  - **m1:** the metadata predicate is weakened: on both sides a part or block no `c`
    references may remain (N4 drops only what op `k+1` kills); every `cm`/`vm` resolves, and a
    new part needs a `set_dynamic_formula` in the side's log. Fixed case: review's
    `SEQUENCE(B30)`, `insert_rows(20, 1)`, `delete_rows(5, 1)`.
  - **m2:** a "written cell" includes cells written by barrier ops (`set_hyperlink` with
    `text`, `merge_clearing`'s hidden cells); "Base bytes" means the `dimension` element's
    (§7 step 4, Test oracle 1).
  - **m3:** the stale-`dimension` fixed cases use a range (`A1:A2`), which SheetJS reads as
    `!ref`; the differential harness writes back a Base cell outside a stale range (§14).
    §6.1 says "every cell Commit wrote", not "changed".
  - **n1:** revision 5's M1 entry now matches §7.1's table.
- **Revision 7** (owner decisions 2026-11-25: Q1–Q6 all (A); Opus review round 6: m1–m3, n1).
  Status set to Accepted; round 6 converged (Blocker 0, Major 0), so no further round.
  - **Owner decisions:** Q1–Q6 are decided as their recommended option (A); the text that
    said "default" or "awaits the owner" now states the decision. P7 and P8 are not planned
    (§4.1, §8.2, §8.3, §10.3, §13.4, Open questions).
  - **m1:** P1 on a missing cell creates nothing, as in ExcelJS 4.4.0 and as Q3's "clear
    keeps h2" requires; P1 moves to N1's conditional row (§4.1, §4.4). Oracle 1 fixed cases when P1 lands.
  - **m2:** a written cell is one a call's plan targets; a cell a conditional op skips is not
    written, so it does not count for `dimension` (§7 step 4, Test oracle 1).
  - **m3:** P models metadata as a map from each `c` with `cm`/`vm` to its block's content,
    not as a set of blocks (Test oracle 1).
  - **n1:** a `set_dynamic_formula` entry compiles as its own call, outside the batches
    (§7 step 4, §7.1).
- **Revision 8** (spike `cb57de4` + `ad74d32`, findings F1–F14; owner direction 2026-11-25 on
  the save cost, recorded as Q7 for confirmation). Status: revision 7 stays Accepted; this
  revision is Proposed until its review converges and the owner confirms Q7.
  - **Commit = serialization of the kept shadow** (F6, F9). The compiled Commit saved in
    8.1–8.6× a sequential `to_bytes` (with the §10.1 reopen; 6× without), against
    ADR-0018 §3.8's 2× target; serializing the shadow under four rules saved in 1.6–1.8×.
    Revision 7's Alternative 9 becomes the decision (§7), with the four rules normative
    (§7.1): **R1** no-op judgment at append, after validation (F13); **R2** a cell back in its
    snapshot state is restored to the original node; **R3** calc state (Q1 (A)) deferred to
    the save and applied once over the changed set; **R4** session `sst`/`xf` items collected
    and numbered canonically after the Base's. The shadow is kept after a save; the §10.1
    reopen is not done (§10.1).
  - **Moved to Appendix A** (unimplemented alternative): revision 7's §4.2 `start` (A.1),
    §4.4 normalization N1–N7 (A.2), §7 compilation (A.3), §7.1 batches (A.4), the Commit-time
    no-op of §9.2 (A.5), §7.2 cost (A.6), oracle 1's `normalize` idempotence and the
    compile-only mutants (A.7). F1, F4, F5, F7 (and F2, F6) are recorded as that design's
    issues (A.8). The projection P stays in the body (oracle 1).
  - **New oracles** 11 (history independence, byte-identical), 12 (replay equality), 13
    (bisimulation after a save), 14 (`truncate_fast` = replay). The View, Reload and
    `Err` ⇒ byte-identical oracles stay. Oracle 1 compares the sequential reference with the
    Commit of §7 (no normalization step); the acceptance criterion "sequential = normalize, then
    apply" is replaced by history independence (oracle 11, Delivery).
  - **F3:** echoes end per style group (`numFmt`, `font`, `fill`, `border`, `alignment`,
    `protection`); the key index holds one style key per group (§4.1, §4.3, §6.1).
  - **F4/F5:** oracle 1's `sst/@count` and `fullCalcOnLoad` predicates are stated per side
    (§7.1 makes them exact on the Journal side; `main`'s sequential side keeps the weaker
    forms the spike checked) (Test oracle 1, A.8).
  - **F8:** part B ops are outside oracle 1 (i); P2 is "`set_style_index(i)` with `i` at or
    above the Base's `cellXfs` count" (§4.1, Test oracle 1).
  - **F11:** the shadow equals the sequential workbook **up to P**, not in bytes; its nodes
    depend on their write history; R2 needs `restore_cell` in core (§3.2, §7.4, Appendix B).
  - **F12:** session `xf` and `sst` indices are not exposed in the View before a save numbers
    them (§6.1).
  - **F14:** §9.3 for a failed save of the kept shadow.
  - **Core APIs** the spike stands in for are listed in §7.4 (C1–C5) and become Delivery
    prerequisites; the spike's text layer is evidence, not design (Appendix B).
  - Other text follows: §2, §3.2, §4.2–§4.5, §5.3–§5.5, §6, §8.1, §8.3, §9, §10, §13.2,
    Consequences, Alternatives 9, 11 and 12, Open questions Q7.

## Context

ADR-0021 and ADR-0022 built the SheetJS CE 0.20.3 and ExcelJS 4.4.0 compatible adapters on a
**state-based** mutation model:

- ExcelJS: every setter reopens the last committed bytes, applies one Rust edit and
  serializes a candidate (ADR-0021 "Mutation and ownership"). ADR-0022's implementation notes
  measured the cost on a 26,165-byte sparse workbook, ten scalar updates: Chromium medians
  68.8 ms reopening the snapshot, 2.9 ms writing, 9.1 ms serializing; Firefox 750, 28 and
  60 ms. "Reopening the snapshot dominates this workload."
- SheetJS: `write` diffs the caller's mutable objects against a baseline and replays the
  change set on a disposable workbook (ADR-0021 "Mutation and ownership").
- Handles, pending table edits and row splices are tracked by coordinates in the adapter and
  reconciled at each commit (ADR-0022 notes on table handles, row splices and zero-sheet
  workbooks).

The per-item compatibility work exposed four defects of that model. The labels are this ADR's:

| | Defect | Example |
|---|---|---|
| (a) | **No provenance.** A value read back cannot say whether it came from the file or from a setter in this session. | ExcelJS echoes a session value as assigned, but returns the normalized form after a reload [7-2]. A state-based model has only one form. |
| (b) | **Cost per edit, not per save.** Each edit pays reopen + serialize across the Wasm boundary. | The 68.8 ms / 750 ms reopen above, paid per setter. |
| (c) | **Atomicity is bolted on.** Native apply failures poison the workbook, and poison "cannot be treated as successful rollback" (ADR-0021 "Rust changes and transaction choice", after its review B1). Each consumer builds its own snapshot: byte snapshots (ADR-0021), `Workbook::fork()` per transaction (ADR-0023 §11.2). | |
| (d) | **Coordinates move.** After a structural edit, pending edits, handles and hidden per-cell information keyed by coordinates must be re-aligned by guesswork. | ADR-0022's row-splice notes clear and rewrite shared groups and arrays in two plans to move them. |

Diskin et al. [4-4] name the root cause: a state-based bidirectional transformation has to
*guess* the correspondence between old and new states, and misaligns. A **delta-based** design
keeps the edits themselves as the primary data. On 2026-11-25 the owner decided:

- xlsx adapters (and other consumers of the editing API) are designed on an **Edit Journal**:
  an immutable **Base** (the opened file), a typed **Journal** of edits with provenance, a
  **View** = Base ⊕ Journal, and a **Commit** that saves atomically (revisions 2–7: by
  compiling the Journal into Rust edit plans; revision 8: by serializing the shadow under four
  rules, Q7);
- the name is "Edit Journal";
- the per-item SheetJS/ExcelJS compatibility loop **stops** and is not resumed. Compatibility
  gaps are measured again after this design is implemented, with a differential harness
  (a follow-up, §14).

The demand is unchanged: browser applications (ADR-0021) and PAD/UiPath migration (ADR-0023)
need lossless, Office-free editing with the error behaviour of a familiar API.

### Prior art in one table

| Idea | Source | Used for |
|---|---|---|
| Read-only original + append-only edit layer | piece table [1-1], LSM tree [2-6], overlayfs whiteouts [2-8] | Base / Journal / View (§3, §4, §6) |
| Edits, not states, as the source of truth | delta lens [4-4], edit lens [4-3] | the Journal is the delta; Commit is `put` (§7) |
| Position maps across structural edits | ProseMirror `StepMap` [3-12] | id vectors updated per structural op (§5) |
| Logical rows/cols as a permutation of stable ids | Fluid `PermutationVector` [3-13]; order-statistic position maps [6-2] | id vectors (§5.2) |
| Reference rewriting as part of the structural transform | Google [3-15], Apple [3-16] | §8 |
| Commutation of patches | darcs [5-1] | normalization of the compiled Commit (Appendix A.2) |
| Proving transforms by machine, not by hand | Imine et al. [3-5] | the proptest oracles (Test oracles 1, 11) |
| Red-green trees; memoizing red nodes costs about 2× memory | Roslyn [1-6], rowan [1-7] | View facades (§6.2) |
| Demand-driven incremental computation | Adapton [6-7]; rebuilder taxonomy [6-5] | derived indexes (§6.3) |
| Copy-on-write commit by root switch | shadowing [2-7], MVCC [2-4] | atomic Commit (§9) |
| Hidden information follows a key, not a position | resourceful lens [4-2]; the InteropGrabBag copy leak [7-8] | Q3 |

## Decision

### 1. Scope

In scope:

- the Rust core of the Edit Journal in `ooxml-xlsx`: Base, shadow, Journal, View, Commit,
  compaction and truncation, for workbooks opened from bytes and for new workbooks (whose
  Base is the template package of `Workbook::new`, or ADR-0022's zero-sheet template);
- the core APIs Commit needs (§7.4, C1–C5);
- the **part A** operations of §4.1 (those with a sequential API on `main`) as journal ops.
  **Part B** ops join when their Rust operation exists (Delivery prerequisites); until then
  they are refused at append with `Unsupported { op }`;
- the contract that the Wasm adapters (ADR-0021/0022) and the Python binding (ADR-0023) build
  on.

Out of scope:

- **docx and pptx** (paragraphs or slides as a permutation). The model may generalize, but
  that is future work with its own ADR;
- concurrent writers, merging two Journals, collaborative editing: the Journal is one linear
  history of one writer, so only TP1-like properties are needed, never TP2 [3-2, 3-3];
- undo/redo as a feature (truncation, §10.3, is the only history operation);
- formula evaluation (ADR-0003's policy stays);
- the streaming read and write primitives of ADR-0022: they hold no Base;
- per-item compatibility fixes for SheetJS/ExcelJS (owner decision above).

The existing per-call Rust API (`Worksheet::set_*`, `write_cells`, `insert_rows`, …) stays. It
is the **sequential reference** of Test oracle 1, the engine of the shadow (§3.2), and the
API of native callers that want today's behaviour.

### 2. Model and vocabulary

```text
 Base bytes (immutable) ──open──► Shadow (Workbook, kept)  ◄── each accepted op: validate,
        │                           │                            R1 no-op test, apply,
        │                           │                            R2 snapshot / restore
        │                           ▼
        │                Journal: log of ops (seq, origin, epoch) + key index ──► View
        │                           │                                      (echo / normalized)
        │                           ▼ Commit (§7): R3 finish_calc → R4 compact_registries →
        │                             dimension → serialize the shadow once → swap
        └───────────────────────────────────────────────────── output bytes = new Base
                                                                (shadow kept, no reopen)
```

| Term | Meaning |
|---|---|
| **Base** | The opened package bytes, `Arc<[u8]>`. Never mutated (§3.1). |
| **Shadow** | A `Workbook` opened from the Base, with every accepted op applied by the sequential API under §7.1's rules. Serialized at Commit and kept after it; rebuildable from Base + Journal (§3.2). |
| **Op** | One typed journal operation: a keyed write, a barrier op or a structural op (§4.1). |
| **Log** | The Journal's ops in `seq` order. Truncation cuts it; nothing folds it (§4.3). |
| **Key index** | `key → [seq]`, derived from the log, for echo reads (§4.3, §6.1). |
| **Epoch** | The interval between two structural ops. Epoch `k` lies after structural op `k`. |
| **Id** | `SheetId`, `RowId`, `ColId`, `ObjectId`: stable identities, never reused (§5). |
| **Dead** | An id removed by a structural op; entries keyed on it are tombstones (§5.5). |
| **Snapshot** | A cell's original node, taken at its first applied write in an epoch (§4.4, R2). |
| **Changed set** | The cells (by id) whose state differs from their snapshot; what R3, `dimension` and locality are computed from (§4.4). |
| **Session item** | An `sst` item, `cellXfs` entry, font, fill, border or custom `numFmt` the shadow added since the Base (§4.4, R4). |
| **View** | The shadow's state plus the log's provenance, as adapters and native readers see it (§6). |
| **Commit mark** | The largest `seq` folded into the Base by the last successful Commit (§10). |
| **echo / normalized** | The two read modes: the caller's input as given, or the form a reload would give (§6.1). |

### 3. Base and shadow

#### 3.1 Base and Base ids

- The Base is the input bytes, behind an `Arc<[u8]>` (`Workbook::from_bytes` takes one). It is
  **never mutated**. An empty Journal saves it as is (GetPut, §6.1).
- **Base ids.** `RowId::Base(r)` names the row at logical position `r` (1-based) of the
  **first** Base of the session; likewise `ColId::Base(c)`. A row or cell without `r` has the
  implicit position Excel gives it, but the census blocks edits to such a sheet
  (`EditBlock::MissingReference`), so writes there are refused on the shadow, as by the
  sequential API, and so are structural ops (ADR-0018 §2.4; 15 corpus sheets, ADR-0018
  Context). A Base sheet is `SheetId::Base(sheetId)`, the `sheet/@sheetId` (ADR-0009 §3).
- After a Commit the Base is the output file, but ids keep their names: the current position
  of any id is its index in the vector (§5.2), never the number in its name.
- Every row of the grid (1..1,048,576) has an id, stored as one run (§5.2), so writing to an
  empty row needs no allocation.

#### 3.2 Shadow (decided)

- The shadow is `Workbook::from_bytes(Base)`. An op is **accepted** iff its sequential API
  call returns `Ok` on the shadow (validation first, then §7.1's R1; §4.5). An accepted op that
  R1 does not skip has then also been applied by that call.
- It is the only validation ground (§4.5), the only source of normalized reads (§6.1) and,
  since revision 8, the **source of the output** (§7). There is no second implementation of
  guards, and no step map applied to formula text: the shadow's ADR-0018 operation rewrites
  it.
- **It equals the sequential workbook of Base + log up to P (Test oracle 1), not in bytes**
  (F11). Two causes:
  - §7.1's rules change the state on purpose: R1 skips no-ops, R2 restores reverted cells, R3
    runs `CalcPolicy::Deferred` (no strips at write), R4 drops unused session items at save.
  - A sequential call leaves traces that depend on the cell's history, not only on its final
    state. The spike found five: `set_style_index(0)` writes `s="0"` where the direct write has
    no `s`; `set_formula` drops the Base cache, so a formula written away and back loses it; a
    node that lost its children serializes as `<c …></c>` where a parsed one stays `<c …/>`; a
    strip made by an earlier save must survive a later style-only write; a garbage session
    `xf` can be revived by dedupe and renumber later items (Appendix B).

  R2 removes the first four by putting the original node back (`restore_cell`, §7.4 C2), R4
  the fifth (`compact_registries`, C4). That the result is history-independent is an oracle
  (11), not an argument.
- It is a cache of Base + log. It is rebuilt by replaying the log on `from_bytes(Base)`:
  after a truncation that `truncate_fast` does not cover (§10.3), after a failed Commit that
  changed it (§9.3), after a call refused part-way (§4.5), and when an op poisons it (ADR-0003
  §3.1: an internal failure). Replay runs the ops through the same rules (validation, R1, R2),
  so a replayed shadow makes the same decisions; it re-applies only ops that were accepted on
  the same prefix, so a refusal during replay is `JournalInternal`. Structural ops record the
  `New` ids they allocated, and replay reuses them. Replay equality is Test oracle 12.
- Memory: the shadow is today's sequential `Workbook`; the Journal adds the Base bytes, the
  log, the vectors and one snapshot per touched cell (about 300 bytes each in the spike). It is
  not dropped at Commit (§7.3).

### 4. Journal

#### 4.1 Operations

| Class | Ops (sequential API on `main` = part A) | Key / ordering |
|---|---|---|
| Cell content | `set_number`, `set_string(_inline)`, `set_rich_string`, `set_bool`, `set_error`, `set_datetime`, `set_formula`, `set_dynamic_formula`, `remove_formula`, `write_cells` (one op per cell) | `(SheetId, RowId, ColId, Content)` |
| Cell style patch | `set_format`, `set_num_format`, `set_font`, `set_fill`, `set_border`, `set_style_index`, `set_named_style`; the style half of `*_with_format` and of `set_datetime` (`EnsureDate`) | `(SheetId, RowId, ColId, Style(group))`, one key per style group the patch sets (§4.3) |
| Row / column property | `set_row_height`, `set_row_hidden`, `set_row_outline`, `set_column_width`, `set_column_hidden`, `set_column_outline` | `(SheetId, RowId \| ColId, Prop)` |
| **Barrier** (ordered) | merges (`merge`, `merge_clearing`, `unmerge`), hyperlinks, validations, conditional formats, autofilter, sparklines, notes, threaded comments, images, tables, breaks, panes, sheet view/state/protection, workbook protection, `add_differential_format`, `set_calc_policy` | `seq`; ids anchor ranges (§5.4) |
| **Structural** (ordered) | `insert_rows`, `delete_rows`, `insert_cols`, `delete_cols`; splice under Q2 (A) (§8.2); `add_sheet`, `add_worksheet`, `delete_sheet(_strict)`, `rename_sheet`, `move_sheet` | `seq`; one ADR-0009/0018 operation each, a splice up to three, atomic as one call (§4.5) |

**Part B** (no sequential reference on `main`; refused with `Unsupported { op }` until the
prerequisite lands, Delivery). Part B ops are outside Test oracle 1 (i) (F8):

| Id | Op | Needs |
|---|---|---|
| P1 | clear a cell's value and formula, keeping its style (ExcelJS `cell.value = null`); on a **missing** cell it creates nothing, so the row/col default stays (ExcelJS 4.4.0 keeps it too; review round 6, m1) | a public write over the existing `CellOp::Clear` that skips a missing cell, as `RemoveFormula` skips a cell without a formula (`edit.rs` L419-421) |
| P2 | `set_style_index(i)` with `i` at or above the Base's `cellXfs` count, that is, an `xf` created in this session (F8, F12) | an `xf`-by-content style request (§6.1) |
| P3 | copy sheet | ADR-0018 §4 (P7d) |
| P4 | rename a table or table column | ADR-0018 §5–§6 |
| P5 | add, change or remove a defined name | a new ADR |
| P6 | move or copy cells or rows; cut/paste across sheets | a new ADR (reference semantics of a move) |
| P7 | formula with a supplied result | not planned: Q1 is decided (A) |
| P8 | splice that does not rewrite cell formulas | not planned: Q2 is decided (A) |

The sequential API accepts P2's indices (they are in range on a workbook that created them), so
"refused iff the sequential API refuses" cannot hold for part B; oracle 1's generator draws
`set_style_index` only from Base indices and from indices out of range on both sides.

#### 4.2 Ops and `Origin`

```rust
struct LoggedOp {
    seq: u64,             // global append counter, allocated only on acceptance
    origin: Origin,
    born: Epoch,          // the epoch whose coordinates the payload's text is written in
    applied: bool,        // false: a no-op skipped by R1 (§7.1); logged for echo and origin
    op: Op,               // typed payload; formula text kept as text (§8.1)
}
enum Origin {
    Base,                                       // never stored: absence of an op means Base
    Session { seq: u64, api: Api },
}
enum Api { Native, Python, SheetJs, ExcelJs }
```

`raw_input` (the caller's value exactly as given) is part of the origin conceptually, but it
is **stored by the adapter**, keyed by `seq` (a JS `Map` or a Python dict). Rust stores the
typed payload only. That keeps foreign values off the Wasm boundary (§11) and lets the
adapter echo values whose shape Rust does not model. (The spike keeps it in Rust; nothing
depends on where it lives.)

Revision 7's `start` field (the key's start state for normalization) is not needed by the
Commit of §7; it belongs to the compiled Commit (Appendix A.1).

#### 4.3 Log and key index (decided)

- The log is the **history**: every accepted op, in `seq` order, including those R1 skipped.
  Nothing is folded, replaced or removed at append. `truncate` cuts it (§10.3).
- The key index is the **View's** structure: `key → [seq]`, newest last. It is derived: an
  append pushes, a truncation pops what it cut.
- **Style keys are per group** (F3): `numFmt`, `font`, `fill`, `border`, `alignment`,
  `protection`, as ExcelJS echoes them (after `cell.font = F; cell.fill = X`, `cell.font` is
  still `F`). A partial patch pushes the key of its group (`set_num_format` → `numFmt`,
  `set_font` → `font`, …); a full patch (`set_format`, `set_style_index`) and
  `set_named_style` push every group's key; the style half of a two-half op pushes the groups
  it sets (`EnsureDate` → `numFmt`). A content op pushes only the `Content` key, so a style
  patch never ends a value echo. The granularity is in the key index, not in each adapter, so
  both adapters and the Python binding share one rule.
- A clear (P1) is an op like any other (a whiteout [2-8]).
- No folding exists in the body. Revision 7 folded a copy of the log at Commit (N1); that is
  Appendix A.2.

#### 4.4 Snapshots and the changed set (decided)

- **Snapshot.** The first write that R1 does not skip on a cell in an epoch stores the cell's
  node as it was before that write (`None` for a missing cell). In epoch 0 that is the Base's
  node; after a structural op it is the node as that op left it. A snapshot lives until the
  next Commit or until a structural op closes it (below). For a Base formula cell a carried
  strip (§7.1 R3) is part of the snapshot: its reference cache is "none".
- **Back in its snapshot state** means: the same content (value with its type and the rich
  flag; or formula text, `f` attributes and kind) and the same `s`. A formula's cached value
  is not compared (R2 restores it).
- **Changed set.** The cells whose current state is not their snapshot state, plus the cells
  of earlier epochs of this session that were not back in their snapshot state when a
  structural op closed their snapshot. Cells a barrier op writes (a hyperlink with `text`, the
  hidden cells `merge_clearing` clears) are cells like any other: they take a snapshot and
  join the set if they differ. A cell whose id dies leaves the set (its row is the structural
  op's allowed diff).
- **Epochs close snapshots.** A structural op on a sheet closes the snapshots of that sheet's
  cells, and a reference-rewriting structural op closes those of every formula cell whose text
  it rewrites. A closed cell keeps its changed-set membership; its next write takes a new
  snapshot. So R2 restores only within an epoch. A write-back that spans a structural op is
  compared up to P, not in bytes (Test oracle 11's scope). The spike has no structural ops; the
  rule is checked when J2 lands.
- **Session items.** The `sst` items, `cellXfs` entries, fonts, fills, borders and custom
  `numFmt`s the shadow added since the Base. R4 keeps those a cell references at save and
  drops the others. `dxfs` are not session items: a `dxf` a barrier op added stays (append-only,
  ADR-0006 §5.8 reuse-or-append), and its index is the shadow's.
- **Workbook-scoped side effects of content ops**, each matched to a rule: string writes
  append `sst` items (R4); two-half calls and patches append `xf`s and their font, fill, border
  and `numFmt` entries (R4); `set_dynamic_formula` creates or extends the metadata part (kept;
  oracle 1's metadata exclusion); formula and value writes strip caches, set
  `fullCalcOnLoad` and remove `calcChain.xml` (deferred to the save by R3); writes outside
  `dimension` grow it (decided at the save from the changed set, §7.2 step 4).

#### 4.5 Validation (decided)

- **At append, on the shadow.** An op is validated by the sequential API's own checks on the
  shadow, plus the adapter's own argument checks (ADR-0021 "scalar validation"). `Err` → that
  error is returned and the **Journal and shadow are unchanged** (ADR-0001 `Err` ⇒
  byte-identical for the shadow; no `seq`, no id allocated; Test oracle 3). `Ok` → R1 decides
  whether the op is applied (§7.1); either way it is appended. Refusal equivalence with the
  sequential API (Test oracle 1 (i)) holds by construction for part A.
- **Validation comes before the no-op judgment** (F13). A skipped no-op must still return the
  refusal the sequential call gives, for example the same value written to a non-anchor cell
  of a merge (`MergedCell`). With the plan-only API (§7.4 C1), the order is: plan (validates
  and resolves the style request) → `Err` is the refusal → `Ok` and a no-op is skipped →
  otherwise apply the plan. Until C1 lands, R1 is **off**: every accepted op is applied, and R2
  removes what a no-op left. The spike judged no-ops before applying, without validation; its
  fixture has no cell where that hides a refusal, so this rule is untested there.
- **Structural ops** are validated and applied by their one ADR-0009/0018 operation on the
  shadow, whose state already contains every earlier op. If an earlier write makes the
  operation refuse (a write into a cell that would become a non-empty merge anchor, a
  formula whose reference would be deleted, `ReferenceDeleted`), the op is refused. No dry run
  and no replay is needed.
- **Cost.** A keyed op costs its sequential call on the shadow, plus the no-op test, the
  snapshot at its cell's first write and the revert check: 2–3 µs per op (5%) at 110k cells,
  below the noise at 1.1M (spike `ad74d32`). No reopen, no serialize. A structural op costs one
  ADR-0018 operation (linear in the rows at or after `p` plus the rewritten formulas,
  ADR-0018 §3.8), once: Commit does not run it again. Structural ops are not rare: ExcelJS
  4.4.0's `insertRow`, `insertRows` and `duplicateRow` all call `spliceRows`
  (`lib/doc/worksheet.js` L423, L429, L469). The cost of `k` inserts on a large sheet is
  measured in J0 (Test oracle 10); no figure is claimed here.
- **A call is atomic.** One call may run several operations on the shadow: a splice (up to
  three, §8.2), a flush (§11). (`write_cells` is one sequential call, atomic by itself.) If
  any of them refuses, a splice returns that error, and a flush returns
  `JournalInternal { seq, source }` with the refusal as `source` (§11). Either way the call
  is undone: the log, the key index, the `seq` counter, the vectors (§5.2), the tombstone set
  (§5.5), the echo ends (§6.1), the snapshots and the shadow are as before the call. The **id
  counter is not restored**: ids the call allocated are discarded unissued, so "never reused"
  (§5.1) holds. The shadow is restored by **replay** from the Base (§3.2): `main` has no
  inverse operations and no `Workbook` snapshot, and a snapshot taken before every splice
  would be the clone ADR-0018 §2.1 rejected. A call of keyed ops only may be undone by
  `truncate_fast`'s cell restore instead (§10.3), with the same result (Test oracle 14).
  Replay costs the ops since the commit mark, on the refusal path only; it is measured in J0
  (oracle 10). Oracle 1's sequential reference of such a call is the same operations with the
  reference workbook restored (from its bytes) on a refusal.
- **Later ops never invalidate earlier ones**: each op was accepted on a state containing all
  earlier ops, so there is no write skew [2-5].
- **Invariant: an accepted Journal always saves.** A Commit that fails anyway is a bug:
  `Err(JournalInternal { seq, source })`, no output, Base and Journal unchanged (§9.3).
  Test oracle 3 asserts it over generated Journals.

### 5. Ids, epochs and position maps

#### 5.1 Id spaces (decided)

- `SheetId::{Base(u32), New(u64)}`, `RowId::{Base(u32), New(u64)}`, likewise `ColId`,
  `ObjectId`. `New` ids come from one monotonic counter per Journal, **never reused**, not
  even after truncation (§10.3), a call undone part-way (§4.5) or compaction (§10). Fluid reuses physical ids [3-13]; here
  reuse would let a new row inherit a dead row's tombstones and origins.
- Row and column ids are **scoped by sheet**: a key always includes its `SheetId`.
- A sheet is identified by `SheetId`, never by name. Rename changes the id → name map; the
  shadow's ADR-0009 operation rewrites formula texts that name the sheet.

#### 5.2 Permutation vectors

Per sheet and axis, the Journal holds the logical order of ids as a **counted B-tree of runs**
(order-statistic tree, [6-2]; piece-tree style [1-3]):

- a Base sheet starts as one run `Base(1)..=Base(1048576)` (16,384 for columns);
- `insert(p, n)` splits a run and adds a run of `n` `New` ids; the last `n` ids are pushed off
  the sheet and **die**. They can hold no content: ADR-0018 §3.2 refuses the insert
  (`ShiftOffSheet`) when the last `n` rows hold a `row` element or an object with content, and
  the shadow has every session write as such an element;
- `delete(p, n)` kills `n` ids and appends `n` `New` ids at the end, so the vector keeps the
  grid's length;
- index → id, id → index, insert and delete are O(log R) for R runs. R grows with the number
  of structural ops, not with the sheet size [6-2].

#### 5.3 Step maps

Each structural op records a `StepMap` (axis, kind, `p`, `n`) [3-12]. It is used to update the
vectors only. **No step map is applied to formula text**, in the View or at Commit (§8.1).
Keyed writes need no mapping: their ids are what makes them immune to structural edits.
(Appendix A's N3 (ii) also read the step map.)

#### 5.4 Id-anchored ranges

A range a barrier op names (a merge, a DV `sqref`, a table) is stored by its corner ids,
not by coordinates, so a stale handle can be detected (§5.5) and replay can apply the op at the
coordinates of its epoch (§3.2). The range's current extent is read from the shadow, whose
ADR-0018 operation applies §3.2's rules; the corner ids give the same answer (Test oracle 6).

#### 5.5 Dead ids

- A structural delete kills its ids. The shadow's ADR-0018 operation removes their cells; a
  write to a deleted cell is gone with it, as ProseMirror drops a step whose position was
  deleted [3-12]. The cell's snapshot is dropped and it leaves the changed set (§4.4).
- A later op that names a dead id (a stale handle) is refused at append (`StaleHandle { id }`).
- References in **formula text** to deleted cells are not dropped: they become `#REF!` or
  refuse, by ADR-0018's rules on the shadow (§8, Q2).
- Dead ids stay in a tombstone set until compaction (§10); the counter never reissues them
  (tombstones and GC separated, as Yjs does [3-9]).

#### 5.6 Copies and moves of ids (decided)

| Operation | Ids |
|---|---|
| rename sheet, move (reorder) sheet | same `SheetId` |
| delete sheet | `SheetId` dead |
| insert / delete rows or cols | surviving ids keep their identity; inserted rows get `New` ids; deleted ids die |
| splice (Q2 (A), §8.2) | as its delete + insert: deleted ids die, inserted rows get `New` ids |
| copy sheet (P3) | new `SheetId`; its vectors start as a copy of the source's vectors at that epoch (same row/col id values, new sheet scope) |
| copy rows or cells, within or across sheets (P6) | **new** ids at the target; content copied as new entries with the copying op's origin |
| cut/paste across sheets (P6) | copy + clear at the source |

### 6. View

#### 6.1 Reads and the two laws

- **normalized** (native, Python, and any reader that wants what the file will say): the
  shadow's read at the id's current position (vector index), with two save-time rules applied
  ahead of the save, so that the read is what a reload of the next output gives:
  - a formula cell in the **pending strip set** (R3 computed for the current state; computed on
    demand and cached until the next append; 2.0 ms at 110k cells and 5.5 ms at 1.1M for
    ~1,500 content changes in the spike) reads with no cached value;
  - a formula cell restored by R2 reads with its original cache (`restore_cell` puts it back).
- **echo** (adapters): if the key index has a live `Session` op for `key` whose echo has not
  ended, the adapter returns `raw_input[seq]`; otherwise the normalized read in the
  reference library's projection (ADR-0021 "Cell and range projection"). An op R1 skipped
  echoes like any other.
- An echo **ends** when its key is written again (per style group, §4.3), its id dies, or a
  structural op changes the shadow's text of that session formula (checked, after each
  reference-rewriting structural op, for the session formula ops since the commit mark). The
  end is stored as the ending op's `seq`; `truncate(t)` clears every end with `seq > t`, so the
  echo returns.
- **Session registry indices are not exposed** (F12). Before a save, a session item has no
  stable index: R4 numbers the survivors only at the save, and the spike found a garbage `xf`
  revived by dedupe that renumbers later items. So:
  - a style read returns the **resolved** format (numFmt code, font, fill, border, alignment,
    protection), which is all ExcelJS needs; `style_index()` returns the index only when it
    names a Base item (below the Base's `cellXfs` count) and otherwise says that the style is
    a session style without an index;
  - `sst` indices are never part of a read (strings are resolved);
  - after a save, the items are Base items in the new Base's numbering, and the shadow uses
    the same numbering (R4 compacts the shadow, §7.1), so `set_style_index(i)` takes `i` in
    Base numbering with no translation; an `i` at or above the Base's count is P2;
  - a `dxf` index returned by `add_differential_format` is the shadow's and the output's (dxfs
    are not compacted, §4.4).
- **`dimension` (`!ref`)** in the View is the one the next save writes (§7.2 step 4): the
  Base's element grown by the changed set and rewritten by structural ops. A write-back that
  R1 skips or R2 restores grows nothing. It need not cover a Base cell outside a stale Base
  `dimension` that the session only wrote back, or that a structural op moved (the sequential
  path behaves the same for the latter).

The lens laws [4-1] hold in this weakened form, stated separately:

- **View law** (PutGet in session): `get(append(j, set(k, x))) = echo(x)`, for every accepted
  `set`, until the echo ends.
- **Reload law** (PutGet across a save): `get(load(save(j)), k) = normalize(echo(x))`, where
  `normalize` is the adapter's projection of the saved value. It also holds for normalized
  reads: before a save, a normalized read equals the same read after the save and a reload
  (Test oracle 13). Session style indices are outside it, since they are not exposed.
- **GetPut**: `save(load(b))` with an empty Journal is `b` (ADR-0001 Layer 1), and a Journal
  whose ops are all skipped by R1 or restored by R2 changes no byte (§9.2).

#### 6.2 Facades

Cell and row facades handed to adapters are built on demand from the shadow and the vectors,
and are invalid after the next op. They are **not memoized**: rowan dropped memoization
because it about doubled memory [1-7]. An adapter that needs object identity (ExcelJS
handles, ADR-0021) caches its own JS objects by id, not by coordinate, so a handle survives
structural edits.

#### 6.3 Derived indexes

| Index | Rebuilder |
|---|---|
| index ↔ id | the vectors (always current) |
| key → `[seq]` | pushed at append, popped at truncation (§4.3) |
| echo ended | the ending `seq`, set by the triggers of §6.1, cleared by truncation |
| snapshots, changed set | at append (R2); dropped by a Commit, a closing structural op or a replay that rebuilds them (§4.4) |
| pending strip set | computed from the changed set on demand, dropped at each append (R3) |
| census, merges, arrays, tables, `!ref`, `sst` reverse lookup | the shadow's own (today's sequential machinery) |

### 7. Commit = serialization of the shadow (revision 8; Q7)

Commit writes the shadow out once. It does not open the Base again, does not compile the log,
and does not reopen its output. What makes the output equal to what a careful compilation would
write is four rules on the shadow, R1–R4, each a **normative** part of this ADR. Revision 7's
compiled Commit is Appendix A.

#### 7.1 The four rules (decided, normative)

| Rule | When | Statement |
|---|---|---|
| **R1** no-op at append | append, **after** validation (§4.5, F13) | An accepted op whose cell **exists** (`c` is present) and that changes nothing there is logged (origin and echo kept, `applied = false`) but not applied: a content op whose payload **reads equal** to the cell (ADR-0023 §6.2: same value kind and value, same formula text; a formula never reads equal to a constant), or a style patch whose resolved `xf` equals the cell's `s` (0 when the attribute is absent). An op on a **missing** cell is never a no-op, whatever the row/col default: the sequential call creates the cell (with `s`, or without it for a content op), and that `c` changes how the cell is formatted (review round 5, M1: `set_style_index(0)` on a missing cell under a bold `col/@style`). A conditional op on a missing cell is applied and does nothing there, as sequentially. |
| **R2** restore | append, after each applied op | If the cell is back in its snapshot state (§4.4), it is **restored**: `restore_cell` (§7.4 C2) puts the snapshot node back (the original parsed `c` with its spans and its cache) and clears the dirty flags up the chain where nothing else under them is dirty, and the cell leaves the changed set. A restored cell serializes as its original bytes, so Layer 1 can raw-copy a part whose every write was reverted. |
| **R3** calc state at the save | Commit (§7.2 step 2) | The shadow runs `CalcPolicy::Deferred` (C3): no cache strip, no `fullCalcOnLoad`, no calcChain change at write. At the save, `finish_calc(changed set)` applies Q1 (A) **once**, as if the changed set were one edit: the **strip set** is the ADR-0003 dependents (transitively) of the changed set's content changes, plus those of style changes when the workbook is style-sensitive (`style_sensitive()`, made public by C3), plus the strips ADR-0018 §3.7 requires of the session's structural ops, minus the changed set; each such cell loses `v`, `t` and an `is` under `f` (ADR-0003 §3.3 item 1); `fullCalcOnLoad` is set when the strip set has a cell or the set of formula cells changed (ADR-0003 §3.3 item 2, manual mode excepted); `calcChain.xml` is removed when a changed cell gained or lost a formula (ADR-0003 §4.3). A strip made by a save is **carried**: a later style-only write to that cell does not bring the cache back. |
| **R4** registries at the save | Commit (§7.2 step 3) | `compact_registries(base_counts)` (C4) keeps the session items (§4.4) that a cell references and drops the others. Survivors are appended after the Base's items in **canonical order**: cells in (sheet order, row, column) order, each item at its first reference, and a `cellXfs` entry's font, fill, border and `numFmt` at their first referencing `xf`. Base items keep their numbers. The shadow's `s` and `v` attributes are rewritten to the new numbers, so the shadow keeps the output's numbering after the save. `sst/@uniqueCount` is the item count; `sst/@count` is the Base's count plus the change in `t="s"` cells over the changed set. |

Consequences of the rules, each an oracle:

- **History independence** (Test oracle 11). For keyed ops within an epoch, the output depends
  only on each cell's final state, not on the writes that led to it: a write-back is free, an
  overwritten value leaves nothing, and registry numbering is canonical.
- R1 is invisible in the output once R2 exists (a write of the same value is a revert of length
  one). What it buys is cost: no snapshot and no dirty part for a pure write-back. Its mutant is
  caught by a white-box test (oracle 5).
- A missing rule is never silent: each rule has a mutant that a test catches (oracle 9; in
  spike `ad74d32` all four are caught, R2–R4 by oracles 5, 11 and 12, R1 by oracle 5's
  white-box test).

#### 7.2 Save steps

1. **Empty log** since the commit mark: return the Base bytes (GetPut).
2. **R3**: `finish_calc(changed set)` on the shadow.
3. **R4**: `compact_registries(base_counts)` on the shadow.
4. **`dimension`** (C5): each sheet's `dimension/@ref` is the Base's (as structural ops rewrote
   it, ADR-0018 §3.2) grown to cover every cell of the changed set on that sheet. A cell R1
   skipped, R2 restored or a conditional op left alone (`remove_formula` on a cell without a
   formula, P1 on a missing cell) grows nothing. On a sheet with no changed cell and no
   structural op, the `dimension` element keeps its Base bytes. A sheet without `dimension`
   stays so.
5. **Serialize the shadow once** (`to_bytes`): Layer 1 raw-copies every part with no dirty node;
   Layer 2 re-renders only dirty nodes (ADR-0001), so each changed part is "original byte spans +
   replaced pieces", as a rope [1-8].
6. **Swap**: only when every step and the serialization succeeded is the output returned and
   made the new Base; then §10.1. On a failure, §9.3: steps 2–4 changed the shadow, so after
   any of them it is marked stale and rebuilt by replay.

Commit **reuses** the sequential API, its plans and their tests; there is no second editing
engine and no second run of a structural op. What it adds is the four rules and the one-shot
serialization.

#### 7.3 Cost and memory (spike `ad74d32`)

Synthetic sheet (columns A–J numbers, K `=A{r}*2` with a cache), 2,000 random ops (half
numbers, a quarter strings, a quarter `set_font`/`set_fill`), AMD EPYC 7313P, rustc 1.99.1,
release, single thread, two runs per size:

| | 10,000 rows (110k cells) | 100,000 rows (1.1M cells) |
|---|---|---|
| save, no reopen | 116–122 ms | 1,148–1,198 ms |
| sequential `to_bytes` | 67–69 ms | 690–714 ms |
| **save / sequential `to_bytes`** | **1.68–1.80×** | **1.61–1.74×** |
| compiled save (Appendix A: open of the Base + apply + serialize + reopen), same runs | 8.3–8.6× | 8.1–8.3× |
| append per op (no-op test + snapshot + revert check) | 49.5–51.5 µs (+2–3 µs, 5%) | 362–363 µs (within noise) |
| peak heap during the save | +20.8 MiB | +260 MiB |
| `truncate` to 1,000 of 2,000 ops: `truncate_fast` vs replay | 19–22 vs 305 ms (14–16×) | 116–121 vs 3,210 ms (27–28×) |

- The save meets ADR-0018 §3.8's 2× target at both sizes. The term over `to_bytes` is the
  spike's text layer (inflating the Base part, two linear scans; Appendix B), which stands in
  for C2–C5. With them in core the shadow is edited before serializing, and that term goes; the
  260 MiB peak is the layer's part-sized buffers.
- At most **one parsed workbook** is alive: the shadow, before, during and after the save. No
  working copy, no reopen. No worksheet is cloned (ADR-0018 §2.1 rejected a staged clone, 289 ms
  for a 200k-formula sheet with the tree's memory doubled).
- Measured again in J0 against core APIs, not the text layer (Test oracle 10).

#### 7.4 Core APIs required

The spike uses `ooxml-xlsx` through its public API only and stands in for each of these with a
substitute (Appendix B). Each is a Delivery prerequisite of J1.

| Id | Needed in `ooxml-xlsx` / `ooxml-core` | Rule | The spike's substitute | Can the substitute stand in? |
|---|---|---|---|---|
| **C1** | **plan without apply**: validate an op and resolve its style request to an `xf` without changing the workbook (F2), so that the no-op test runs after validation | R1, §4.5 | content: R1's content test on a read; style: `set_style_index` only, other patches applied and restored by R2; judged **before** validation | No: a skipped no-op can hide a refusal (F13). Until C1, R1 is off (§4.5). |
| **C2** | **`restore_cell(sheet, at, snapshot)`**: put back the original `c` node (a clone with its spans and cache) and clear `own_dirty`/`subtree_dirty` up the chain when nothing else under them is dirty (`XmlDocument` needs a count of dirty descendants) | R2 | text layer at save: the Base's bytes for restored cells, a canonical form for others, View cache override, carried pins | Output: yes, by the tests. Shadow: no; its nodes stay history-dependent (F11), so the layer carries pins across saves. |
| **C3** | **`CalcPolicy::Deferred`** + **`finish_calc(changed)`**: no strip, flag or calcChain step at write; one plan at save over the changed set (ADR-0018 §3.7, ADR-0003 §3.3); plus a public **`style_sensitive()`** (today `pub(crate)`, `edit.rs`) | R3 | `KeepStaleCaches` + its own decision through `Workbook::dependents` + a text strip; `fullCalcOnLoad` added to the Base's `calcPr` text; calcChain by `Package::remove_part`; the View masks pending strips | Mostly: style-only changes strip nothing; `KeepStaleCaches` still changes the shadow's `workbook.xml` and calcChain, which the layer ignores. |
| **C4** | **`compact_registries(base_counts)`**: drop session `sst`/`xf`/font/fill/border/`numFmt` items no cell references and renumber the survivors canonically, in the shadow, rewriting the `s`/`v` of the cells that use them | R4, §6.1 | text layer at save + carried shadow → Base index maps + translation for `set_style_index` and the View | Output: yes. View: no; an unsaved session `xf` has no stable index (F12). |
| **C5** | **`dimension` at save**: grow from the changed set at save, not at each write (or let the save recompute it from the changed set) | §7.2 step 4, §6.1 | the layer rewrites the `dimension` element | Yes |

With C1–C5 in core the shadow equals the output after a save, the carried state goes away, the
View has stable indices, and Commit is literally `finish_calc` + `compact_registries` +
`dimension` + `to_bytes()`. A cheap per-part `serialize_into` is not needed once C2–C5 edit the
tree; the spike reached 1.6–1.8× without it.

### 8. Formulas and references

#### 8.1 Formula text written in the session (decided)

- A formula op keeps its text as given, plus its birth epoch. It is rewritten in **one
  place only**: by the ADR-0018 (or ADR-0009) operation of each later structural op that
  rewrites references, on the shadow at append. The output is the shadow's text.
- Example (Test oracle 1, fixed case): epoch 0 writes `A1 = B5*2`, then
  `insert_rows(3, 1)`. Both the shadow and the output hold `B6*2`. A step-map translation on
  top would give `B7*2` (oracle 9 mutant).
- The text is **not** converted to id form: Apple's id-based references [3-16] would
  re-render every formula, and ADR-0001 requires that a formula no edit touches keeps its
  bytes.
- Base formulas, names, CF/DV, charts and every other registry place (ADR-0018 §2.2) are
  rewritten by the same operation; the Journal adds nothing there.

#### 8.2 Two kinds of structural op

| Kind | Who issues it | Runs as |
|---|---|---|
| **Excel semantics** (`insert_rows`, `delete_cols`, …) | native, Python, adapters | one ADR-0018 operation |
| **Splice** `splice(axis, p, d, values[i])` (ExcelJS `spliceRows`/`spliceColumns`, and `insertRow`, `insertRows`, `duplicateRow`, which call it) | adapters | by **Q2 (A)** (decided): `delete(p, d)` if `d > 0`, then `insert(p, i)` if `i > 0`, then the `i` rows' cell writes as keyed ops of the splice's `seq`. Its sequential reference is the same three calls |

ExcelJS 4.4.0 (`lib/doc/worksheet.js` L483-548, probe in review round 1): `spliceRows` moves
rows' values, styles and heights, does **not** rewrite cell formulas (`A1 = B5*2` stays after
`spliceRows(2, 0, …)`, also after reload; `SUM(B2:B5)` moved to `C7` unchanged), **does**
rewrite defined names (`S!$B$5` → `S!$B$6`, via `definedNames.spliceRows`), and drops a note
on a moved cell. ADR-0022's notes already record that the adapters' splice and the Rust
whole-row API "have different semantics".

#### 8.3 Formula caches

The cache rule is **Q1 (A)** (decided): ADR-0018 §3.7 + ADR-0003 §3.3, applied once at the
save over the changed set (R3, §7.1).

### 9. Locality and atomicity

#### 9.1 Locality (ADR-0001)

- The output differs from the Base only inside the **union of the allowed-diff rows** (ADR-0014
  §2's data table, ADR-0018 oracle 1) of: each cell of the changed set; each cell of R3's strip
  set, `calcPr` and `calcChain.xml` as R3 decides; the registries' appended items and counts
  (R4); each sheet's `dimension` (§7.2 step 4); and each structural and barrier op. This is
  ADR-0001's T2 for a Journal, and Test oracle 4 checks it. A cell R1 skipped or R2 restored
  contributes no row.
- Parts no op reaches are raw-copied (Layer 1); nodes no op reaches, and restored nodes, keep
  their spans (Layer 2). A structural op parses every worksheet (ADR-0018 §2.1 step 1) but
  re-renders only the nodes it changes.

#### 9.2 No-ops and write-backs

R1 and R2 (§7.1) together make a read → modify → write-back of a region change only the cells
the caller changed, and make reverting a value remove its edit (ADR-0021's SheetJS rule): R1
skips the write of a value a cell already holds, and R2 restores a cell whose writes ended where
they started, its cache included, so a value changed and reverted under a dependent formula
strips nothing. Neither grows `dimension` (§7.2 step 4). The View still echoes each op until its
echo ends or compaction (§10.1). An op on a missing cell is never a no-op (R1). Revision 7's
Commit-time no-op test is Appendix A.5.

#### 9.3 `Err` ⇒ byte-identical

| Failure | Effect |
|---|---|
| append refused (§4.5) | Journal and shadow unchanged: same ops, `seq` counter, id counter, snapshots, View reads |
| a splice or flush refused part-way (§4.5) | the whole call undone: log, key index, `seq` counter, vectors, tombstones, echo ends, snapshots, shadow (by replay) as before it; ids it took are discarded; a flush returns `JournalInternal` and breaks the Journal (§11) |
| op poisons the shadow (internal) | the op is not appended; the shadow is rebuilt by replay; `JournalInternal` |
| save fails before step 2 of §7.2, or in a step that does not change the shadow | no output; Base, log, shadow, snapshots, changed set and carried strips unchanged; `JournalInternal`; a retried `save` gives the bytes the first would have given (F14, tested in the spike) |
| save fails in or after step 2–4 of §7.2 (bug, injected fault, panic) | no output; Base and log unchanged; the shadow is marked stale and rebuilt by replay, which gives back the state before the save (Test oracle 12); `JournalInternal`; a retry starts from scratch |
| poison inside a save step (ADR-0003 §3.1) | as the row above |

Atomicity is structural: the Base is not touched until the swap (MVCC [2-4], shadowing
[2-7]). This replaces ADR-0021's byte-snapshot staging and ADR-0023's per-transaction
`Workbook::fork()`.

What this mode gives up (F14): no second engine re-derives the output from the Base, so a
**silent** bug in a sequential call, or a history-dependent trace that R2 and R4 do not cover,
reaches the output. A poisoning is still caught (`JournalInternal` + replay). The guards are
Test oracle 1 against the sequential reference, 11 (history independence), 12 (replay from the
Base gives the same bytes as the live shadow) and 13 (bisimulation after a save).

### 10. Compaction, second save, truncation (decided)

#### 10.1 After a successful Commit

- The output bytes become the new Base. The shadow is **kept**; it is not reopened from the
  output (revision 7's §10.1 reopen is not done). After steps 2–4 of §7.2 it holds what the
  output holds, and Test oracle 13 checks that it behaves as a Journal opened from the output
  would: same refusals, same normalized reads, same next output.
- The log is emptied; the commit mark is set to the last `seq`. The snapshots and the changed
  set are cleared: the new Base is the reference of the next session. Carried strips (R3) stay
  with their cells.
- Session items are Base items now, in the shadow's numbering, which R4 made the output's.
- **Origins are not folded away.** Each live `Session` op leaves an **echo record**
  `(key → seq, api)`, so the adapter keeps echoing after save, as ExcelJS does for its
  in-memory objects [7-2]. An echo record ends as §6.1 says.
- **Ids survive compaction.** The vectors are kept (run-compressed), so ids that handles hold
  stay valid. Dead ids leave the vectors; the counter keeps them unissued.

#### 10.2 Second save

The second Commit's Base is the first Commit's output. An empty Journal saves it
byte-identically; a non-empty one is local to it. In the spike a second session of the same ops
reversed (mostly no-ops) saved the Base bytes at 1.1M cells.

#### 10.3 Truncation

- `Journal::truncate(seq)` drops every op with a larger `seq` from the log, pops the key
  index, and marks the shadow stale (rebuilt by replay on the next read or append). The id
  counter is not rolled back. This is how ADR-0023-style transactions
  (`with wb.transaction():`) roll back, without a snapshot of the parsed parts.
- **`truncate_fast(seq)`** gives the same result without a replay when every cut op is a keyed
  op: for each cell a cut op wrote, `restore_cell` puts back its snapshot (C2), then the cell's
  ops up to `seq` since the commit mark are applied again through §7.1's rules. It falls back to
  replay when a cut op is structural or barrier, or a cell has no snapshot in the current epoch.
  It must give the same reads and bytes as replay (Test oracle 14). In the spike (which
  writes snapshots back with ordinary writes, and falls back for missing, empty, rich, inline
  and shared-formula cells) it was 14–28× faster than replay (§7.3).
- `truncate(seq)` with `seq` below the commit mark is **refused**:
  `Err(TruncateBeforeCommit { seq, committed })`, Journal unchanged. Committed ops are in the
  Base and cannot be cut. A save inside an open transaction is refused by the binding and
  the adapters (Q5 (A)), so a transaction's rollback never meets this error.

### 11. The Wasm and binding boundary

- Nothing is serialized before Commit; an op costs its call on the shadow.
- Adapters **may buffer** appends and send them as one typed-array batch (at a getter, a
  save, or a size limit), but only ops that the JS-side classifier (ADR-0021 projection, ids
  from the View) proves acceptable. Any op that could be refused is flushed and appended
  synchronously, because the reference libraries throw at the call site.
- A flush is one call (§4.5). If op `i` is refused (a classifier bug, or a state-dependent
  refusal such as `StyleTableFull` that the classifier cannot see), the whole flush is
  undone and the Journal is **broken**: the call that caused the flush (a getter, `write`,
  `writeBuffer()`, a size limit hit by a setter) throws `JournalInternal { seq, source }`,
  where `seq` is the one op `i` would have had; every buffered call since the last flush is
  as if never made; every later call except `truncate` and reload throws
  `JournalBroken { at }` (Q6). The caller was told those calls succeeded, so continuing, or
  saving the earlier ops alone, would lose them silently.
- Very large sheets are serialized part by part with ADR-0010's serializer rules; a SAX-style
  rewrite of a part [7-5] is a later optimization, not part of this ADR.

### 12. Errors

| Error | When |
|---|---|
| (existing kinds) | append refused by the sequential API on the shadow: the same error |
| `Unsupported { op }` | a part B op whose prerequisite has not landed (§4.1) |
| `StaleHandle { id }` | an op names a dead id (§5.5) |
| `TruncateBeforeCommit { seq, committed }` | truncation below the commit mark (§10.3) |
| `JournalInternal { seq, source }` | a save, replay or shadow apply failed after acceptance (a bug), or a refused flush (§4.5, §9.3, §11) |
| `JournalBroken { at }` | any call but `truncate` or reload after a failed flush; `at` is the flush's start `seq` (§11, Q6) |

### 13. Relationship to other ADRs

#### 13.1 ADR-0001

Kept unchanged: Layers 1–3, T1/T1b/T2, `Err` ⇒ byte-identical. The Journal adds two
guarantees on top: locality is checked against the Base for the whole Journal (§9.1), and a
failed Commit cannot reach the caller as a poisoned state (§9.3).

#### 13.2 ADR-0003, ADR-0009, ADR-0014, ADR-0017, ADR-0018

Their operations run once, on the shadow at append; the output is the shadow's. Their refusals
keep their error kinds and their oracles keep applying. ADR-0003 §3.3 and ADR-0018 §3.7 run once
per save instead of once per write (R3, `CalcPolicy::Deferred`). Additions on `main`: P1 and P2
(§4.1), and C1–C5 (§7.4), all listed in Delivery.

#### 13.3 ADR-0021 and ADR-0022 (uncommitted): what is replaced

Neither ADR numbers its sections; sections are named by heading.

| ADR | Section / decision | Replaced by |
|---|---|---|
| 0021 | **"Mutation and ownership"**, ExcelJS bullet: per-setter staging from the last committed bytes, publish candidate bytes and read view | Journal append on the shadow + View (§3.2, §4, §6); Commit at `writeBuffer()` (§7) |
| 0021 | **"Mutation and ownership"**, SheetJS bullet: baseline diff of enumerable properties at `write`, disposable workbook per write | the adapter appends ops when it detects the change (or at `write`, as one batch, §11); Commit (§7). The rules "reverting a value removes that edit" and "an unchanged write returns a copy of the original bytes" are kept as §9.2 and GetPut |
| 0021 | **"Mutation and ownership"**, handle identity by load generation and coordinate | handles keyed by id (§6.2); reload starts a new Base |
| 0021 | **"Rust changes and transaction choice"**, paragraphs 2–3: byte-snapshot staging vs a Rust candidate | Commit by serializing the shadow (§7, §9.3); the comparison is closed |
| 0021 | **"Cell and range projection"**: "accept insertion only within [the original range]", "reject deletion or modification of `!ref`", "reject structural, range, removal … changes" | lifted as restrictions of the mechanism; whether each is lifted for an adapter is measured with the differential harness, not decided per item |
| 0021 | **"Acceptance criteria"** 4 (staging failure isolation) and the staging part of 5 | Test oracles 3 and 5 here |
| 0022 | **"Decision"**, paragraph 1: "Existing XLSX mutations continue through the lossless workbook and edit APIs. A future edit batch must validate before committing and preserve existing edit-refusal behavior." | §4.5 and §7: the edit batch is the Journal |
| 0022 | Implementation notes on the XLSX document adapters' **edit mechanism**: candidate documents per edit; table handles with per-handle pending edits; "byte-snapshot to Rust-held staging" measurements; "ExcelJS row splice with notes and formula ranges" (clear-and-rewrite of shared groups and arrays in two plans) | §4 ops, §5 ids, §8.2 splice, §7 Commit. The behaviours these notes pinned become differential-harness cases |
| 0022 | **Kept**: XLSB projection and generated option mappers ("Decision" paragraphs 2–4, "Spike", "Spike outcome", XLSB notes), "Streaming XLSX write/read primitives", "Node ExcelJS model container" | not affected (no Base or a different codec) |

Also kept from ADR-0021: Context, Upstream comparison, "First milestone" surfaces and option
matrix, "Distribution and naming", "Tree shaking", Wasm initialization, the projection
classification table (it becomes the echo/normalized projection of §6.1).

#### 13.4 ADR-0023 (uncommitted)

Affected:

- §6.2/§6.3 workbook-scoped batches and group-wise clears become batches of journal ops;
  `write_changed_cells` is §9.2 (R1, R2);
- §11.2 transactions become `truncate(seq)` (§10.3) instead of `Workbook::fork()`; a save
  inside a transaction is refused (Q5 (A));
- §5 `read_range` reads the View in normalized mode;
- §9 (lifting the classifier) and §10 (packaging, CI) are unaffected.

ADR-0023 is rewritten on this basis before its review round 2, now that this ADR is accepted
(owner decision Q4).

### 14. Compatibility work

No per-item SheetJS/ExcelJS fixes are resumed. After J4 (Delivery), a **differential
harness** runs the same op sequences through the reference libraries and through the
adapters and compares the results (among its cases: a Base cell outside a range-shaped stale
`dimension`, written back unchanged, which is absent from SheetJS `sheet_to_json` after
reload, §6.1); it is a follow-up with its own task, and its results
replace the per-item ledger.

## Test oracles

1. **Sequential equivalence (proptest)**: *sequential application ≡ Journal + Commit (§7)*,
   under the projection below. (Revisions 2–7 stated it as "≡ normalize, then compile";
   normalization is Appendix A.)
   - **Sequential reference**: open the fixture as a `Workbook` and call the public Rust API
     for each op immediately. Ops the API refuses are dropped (`Err` ⇒ byte-identical).
   - **Journal path**: append each op, then Commit.
   - **Assertions**: (i) a **part A** op is refused at append **iff** the sequential API
     refuses it, with the same error kind (part B ops are excluded: P2's indices are accepted by
     the sequential API, §4.1, F8); (ii) both Commit and the sequential save succeed; (iii) the
     two outputs have equal projections **P**; (iv) each output satisfies every validity
     predicate below, against its own Base.
   - **P** is a model extracted from each output; two outputs are equal iff their models are
     equal. **Modeled, compared as values**: per sheet, every cell's value (shared strings
     and rich runs resolved to text and runs), value type, and formula text and kind (shared
     members expanded to their own text; array and dynamic ranges); a formula cell's value
     and `t` are not here but in the cache exclusion below; every cell's **resolved**
     style (the `xf` with its numFmt code, font, fill, border, alignment, protection, named
     style and unknown attributes, by content, not index), where every present `c` is a cell,
     an empty one (no value, no formula) included, and a missing cell is absent from the
     model (a missing cell and an empty `c` with the same resolved style are different);
     row/col properties; **h1** (Q3:
     unknown attributes and extension children of `c`/`row`/`col`, as attribute maps and
     canonical XML); sheet views, panes and selections; every range feature and object (with
     `dxf` resolved); sheet list, order, names, states; defined names; every registry place of
     ADR-0018 §2.2; `docProps/app.xml` as canonical XML; for every
     `c` with `cm` or `vm`, the content of the metadata block each resolves to (a map
     cell → block content, not a set of blocks, so a cell pointed at another block of the
     same set differs; review round 6, m3); the part set, relationships and
     content types except `calcChain.xml` and the metadata part. **Not modeled**: every other
     part and element is compared byte-for-byte.
   - **P excludes**, each replaced by a predicate (on **both** sides unless the row says so):

     | Excluded | Why it may differ | Predicate instead |
     |---|---|---|
     | `sst` item order and unused items | R4 drops unused session items and numbers survivors canonically; the sequential registries are append-only | both: `@uniqueCount` = item count; every index in range. Journal side only: `@count` = the Base's count plus the change in `t="s"` cells (R4). The sequential side is not held to `@count`: on `main` a number written over a shared-string cell leaves `@count` unchanged (F4, A.8) |
     | order and unused entries of `cellXfs`, fonts, fills, borders, `numFmts`, `dxfs` | R4 keeps fewer `xf`s; R1/R2 append none for no-ops and write-backs | every `s` and `dxfId` in range; equality is through resolution in P. Journal side only: no unreferenced session item (R4) |
     | shared-group hand-off shape | R1 skips a write of a shared member's own text, which the sequential side applies, splitting the group (R2 compares `f` attributes, so a cell that was handed off is never restored) | per-cell formula text in P |
     | metadata part: presence, its relationship and content type, order and unreferenced types and blocks | a `set_dynamic_formula` on a cell a later structural op deletes leaves it on both sides; a restore (R2) of a dynamic-array cell can leave the block its session write added unreferenced | both: every `c/@cm` and `c/@vm` resolves to a block in range; a Base metadata part is kept; a Base without the part gains one only if the side's log has a `set_dynamic_formula`. Blocks and parts no `c` references may remain on either side |
     | `dimension/@ref` (SheetJS `!ref`) | the sequential call grows it on every write, a no-op included; the Journal grows it only by the changed set (§7.2 step 4) | both: present iff the sheet's Base (or new-sheet template) has it. Sequential side: covers every surviving cell it wrote, a written cell being one the plan of any call it ran targets, barrier ops included; a cell a conditional op skips (`remove_formula` without a formula, P1 on a missing cell) is not written. Journal side: covers every surviving cell of its changed set; on a sheet with no changed cell and no structural op, the `dimension` element has the Base's bytes |
     | presence of `calcChain.xml` | the sequential path removes it on any formula-set change, even a reverted one | present only if the side's formula-cell set equals its Base's, and then byte-equal to the Base's |
     | cached values `<v>` and `fullCalcOnLoad` | R2 restores reverted caches and R3 strips only over the changed set, so the Journal side strips less (Q1) | both: a kept `<v>` is byte-equal to the Base's for that cell id, and no cell it reads (ADR-0003 dependents, transitively) differs in P from the Base; the Journal's strip set ⊆ the sequential strip set. `fullCalcOnLoad` (not in manual mode) is set if a cache of a cell the side did not write was stripped or the side's formula-cell set changed (ADR-0003 §3.3 item 2); the stronger "or any formula written" of revision 7 does not hold on `main` (F5, A.8) |

     Why this does not hollow the equation: everything a consumer reads after load (values,
     formulas, resolved formats, structure, features, names) is in P, and `!ref` is bounded
     by its predicate. What is excluded is
     either storage layout (indices, order, unused items) or state the consumer recomputes
     (calcChain, caches), and each has a predicate that catches the one visible risk, a
     stale cache or a dangling index. The Journal side is further bounded by oracles 4, 5 and
     11. Any other difference fails.
   - **Generator**:
     - fixtures: a synthetic workbook with every guarded feature (merges, shared groups,
       arrays, CF/DV with relative formulas, a table with totals, names, a second sheet that
       refers to the first, sticky-end ranges), formula cells on the second sheet with a 3-D
       reference, a reference at the last row and column, and a reference that makes
       `delete_sheet_strict` refuse; rows with `customFormat` and `col/@style` over missing
       cells; a merge whose non-anchor cell holds a value (F13); and the
       committed POI fixtures ADR-0018 uses;
     - ops: 1–32 per case, drawn from every part A row of §4.1; structural ops (splice
       included) about 1 in 5; coordinates biased to boundaries (`p = r1`, `p = r2 + 1`, the
       last row, merge anchors, shared-group edges) and to cells earlier ops wrote;
       `set_style_index` only with Base indices or indices out of range on both sides (F8);
     - values: constants, formulas whose references hit the edited band, reverts to the Base
       value, repeated writes to one key, **content + style on one cell, chains of style
       patches** (fill, font, border; a full patch in the middle; `set_datetime` after a fill);
     - fixed cases: B1's `A1 = B5*2` then `insert_rows(3, 1)` (output `B6*2`); formula `=1` then
       constant 5 on a formula cell; a value changed and reverted under a dependent formula;
       constant 5 on `S2!A1` holding `SUM(Sheet1:S2!A5)`, then `Sheet1` `insert_rows(0, 1)`;
       the same with `Sheet1!A1048576`, and with `Sheet1!A5*2` then
       `delete_sheet_strict("Sheet1")` (all `Ok`); `set_formula("1+1")` then `remove_formula()`
       on a Base constant (empty cell); `set_number` then `set_num_format` on a missing cell in
       a styled column (the new `xf` has no column font); on missing `B1`/`B2` under a bold
       `col/@style`, `set_datetime(d)` then `set_number(5)` (`xf` numFmt 14 with the column
       font) and `set_number(5)` then `set_datetime(d)` (numFmt 14, default font);
       `set_dynamic_formula("1+1")` at `A5` then `delete_rows(4, 1)` on a Base without a
       metadata part (both pass); on the same Base,
       `set_dynamic_formula("_xlfn.SEQUENCE(B30)")` at `A5`, `insert_rows(20, 1)`,
       `delete_rows(5, 1)` (both sides keep a part no `c` references; both pass); under a bold
       `col/@style` on column B, `set_style_index(0)` on missing `B5` (both sides write
       `<c r="B5" s="0"/>`, default format; R1 does not skip it); a write at `D10` then
       `delete_rows(10, 1)` under `dimension` `A1:C5` (sequential `A1:D9`, Journal `A1:C5`:
       `D10`'s id is dead, so it is not in the changed set; both pass); under `dimension`
       `A1:A2` (a range, so SheetJS reads it as `!ref`), `set_number(7)` on `C3` holding the
       constant 7 (sequential `A1:C3`; R1 skips the no-op and the Journal keeps `A1:A2`, so
       oracle 5 gives the Base bytes, oracle 4 sees no row, and oracle 1 passes by the
       `dimension` predicate); under `dimension` `A1:C5`, a hyperlink with `text` at `D10`
       (both `A1:D10`) and, under `A1:A2`, `merge_clearing("B3:C3")` with a value in `C3`
       (both `A1:C3`);
       under `xf 1` bold, `remove_formula` then `set_style_index(1)` on `C3` holding the
       constant 7 (both `s="1"`), and `set_number`, `remove_formula`, `set_fill` on a formula
       cell (the fill is kept); `remove_formula` on a Base constant `C3` outside `dimension`
       `A1` and on missing `D10` (both sides keep `A1`); the same value written to a
       non-anchor cell of a merge (refused on both sides with `MergedCell`, though it reads
       equal: F13); a missing `B6` in a `customFormat` row, `remove_formula`,
       `set_num_format("0.00")`, `set_number(1)` (the row's fill is kept; F1). When P1 lands:
       P1 on missing `B5` under a bold `col/@style` (no `c` on either side), and
       `set_number(5)` then P1 on missing `B6` (an empty `c` without `s` on both sides).
       Fixtures with a `dimension` that does not cover every cell feed oracles 1, 4, 5 and 11.
   - **Shrinking**: proptest's sequence shrinking (drop ops), then a custom step that drops a
     whole epoch with its structural op, then coordinates toward the nearest boundary and
     values toward the simplest of their kind. Failing seeds are committed as
     `proptest-regressions` files.
2. **View and Reload laws.**
   - View law: for every accepted `set(k, x)`, `get(k)` in echo mode returns `x` immediately
     after, and until an echo-ending op of §6.1. An op R1 skipped echoes too.
   - Per group (F3): a style patch does not end a value echo; `set_fill` does not end a `font`
     echo; a full style ends every group's echo.
   - Replay: after a truncation, a failed Commit and a forced shadow rebuild, every key reads
     equal in normalized mode to a sequential workbook given the same accepted ops, up to the
     pending strips and restored caches of §6.1.
   - Rollback: `set(k, x)`, `set(k, y)`, `truncate(1)` echoes `x` again.
   - Reload law: after Commit, reopening the output and reading every touched key returns
     `normalize(echo(x))`; per adapter, `normalize` is its projection, checked against the
     pinned reference library in the differential harness. Normalized reads before the save
     equal those after a reload, `dimension` included (§6.1).
   - No View read exposes a session `xf` index before a save (F12).
3. **`Err` ⇒ byte-identical and "accepted ⇒ saves".**
   - Every refusal of a one-op call leaves the Journal and the shadow equal (ops, `seq` counter,
     id counter, snapshots, View reads, shadow `to_bytes`) to before the call, R1's candidate
     no-ops included (F13).
   - Fault injection (`Err` and panic, `catch_unwind`) at each step of §7.2 (`finish_calc`,
     `compact_registries`, `dimension`, serialize, swap), in `restore_cell`, in each structural
     operation's steps 3.1–3.5 of ADR-0018 §2.1 on the shadow, and in a shadow apply: no output,
     Base and log unchanged, the shadow unchanged or rebuilt by replay as §9.3 says, the next
     `save` of an empty Journal returns the Base bytes, and a retried Commit without the fault
     succeeds with the same output.
   - Part-way refusals: a splice whose delete succeeds and whose insert (`ShiftOffSheet`) or
     row write refuses returns that error, and a flush whose op `i` refuses (for each `i`)
     returns `JournalInternal` with it as `source`. Both leave the internal state as before
     the call (log, key index, `seq` counter, vectors, tombstones, echo ends, snapshots,
     shadow `to_bytes`); the id counter may only have grown. After the splice, View reads are
     also compared; after the flush every call but `truncate`/reload returns `JournalBroken`.
   - **Invariant**: every Journal accepted in oracle 1's generator saves with `Ok`. Any
     `JournalInternal` fails the test.
4. **Locality against the Base**: the byte diff of the output against the Base lies in the
   union of §9.1's allowed-diff rows (ADR-0014 §2, ADR-0018 oracle 1); a cell R1 skipped or R2
   restored contributes no row, and a restored cell keeps the Base's bytes byte-for-byte.
5. **GetPut and no-ops** (Journal path only): an empty Journal, and a Journal whose ops are all
   skipped or restored (write the Base value; write then revert; a value write under a
   dependent then revert; a formula written away and back, its cache kept), save
   byte-identically to the Base. White-box: an op R1 skips leaves the shadow's worksheet
   without a dirty node. The sequential path is not held to this; its difference is the
   excluded calc state and `dimension` of oracle 1.
6. **Maps and ids**: a marker written at an id's cell is read back from the shadow at the
   vector's index of that id, after any op sequence; `insert(p, n)` then `delete(p, n)` is the
   identity on survivors (ADR-0018 oracle 3's analogue); corner ids of §5.4 give the shadow's
   extent; no id is ever issued twice across appends, truncations, part-way refusals, replays and compactions.
7. **Compaction**: commit, then commit an empty Journal: byte-identical to the first output;
   echo records survive and end as §6.1 says; handles held across the save still resolve.
8. **Truncation**: `truncate(seq)` then Commit is byte-equal to a Journal that never had the
   later ops. The generator truncates between repeated writes to one key (`set(k,1)`,
   `set(k,2)`, `truncate(1)` reads 1), across structural ops and style chains.
   `truncate` below the commit mark returns `TruncateBeforeCommit` and changes nothing.
9. **Mutation checks** (hand-written, each must fail a test):
   - the four rules: R1 removed (oracle 5's white-box test); R1 judged before validation
     (oracle 1's merge fixed case, F13); R2 removed; R3 removed (the shadow strips at each
     write); R4 removed (every session item kept, in shadow order). Spike `ad74d32` has the
     four "removed" mutants (`mutant_no_noop`, `mutant_no_revert`, `mutant_no_defer`,
     `mutant_no_gc`); each fails (README "Commit = shadow serialization");
   - `restore_cell` that keeps the dirty flags; `dimension` grown by a restored or skipped
     cell; treating an op on a missing cell as a no-op when its `xf` equals `s = 0` or the
     row/col default (round 5 M1); a carried strip dropped by a later style-only write;
   - echo ended per cell instead of per group (F3); a session `xf` index exposed or accepted
     by `set_style_index` before a save (F12);
   - `truncate_fast` that re-applies the kept ops without restoring first;
   - kept from revision 7: keeping the shadow after a part-way refusal (round 2 M4); mapping
     session formula text through step maps (B1); folding at append (B2); reusing a dead id;
     publishing output before serialization succeeds; folding origins at compaction;
     truncating below the commit mark.

   The compiled Commit's mutants are in A.7.
10. **Measurements** (published, no CI gate): the ADR-0021 scalar and sparse browser fixtures
    (ten setters then save); ADR-0018 §3.8's two benchmarks through the Journal; `k` repeated
    row inserts at row 1 on the 1M-cell sheet (append cost); save cost (§7.2 steps 2–5)
    against a sequential `to_bytes`, target 2× (ADR-0018 §3.8; spike: 1.6–1.8× with the text
    layer, §7.3); append overhead of R1/R2; truncation (`truncate_fast` and replay) and
    part-way-refusal replay; peak memory and Wasm memory capacity, against the current paths.
11. **History independence (proptest, byte-identical).** On one Base, a log F and a log L that
    is F with **noise** inserted give **byte-identical** outputs, and also equal P, oracle 1's
    predicates and oracle 4's locality. Noise is a block of keyed ops on Base cells, within one
    epoch, that ends with every cell it wrote in the state it had before the block: a change of
    value (number, strings, bool, error or formula) and a write-back; a style patch (bold,
    italic, fill, number format, border, an index) and a write-back of the Base index; a write
    of the Base value; a write of the Base index. Two variants: noise only on cells F never
    writes; noise also on cells F writes later (before F's first op there), plus a different
    value written before each of F's full content ops. Over the `dimension`s of oracle 1's
    fixtures. A block that spans a structural op is compared up to P (§4.4), and barrier ops
    are not noise (they are not restored and their `dxfs` are append-only). This replaces
    revision 7's acceptance criterion "sequential = normalize, then apply" (Delivery).
12. **Replay equality.** The live shadow's output equals the output after the shadow is marked
    stale and rebuilt by replay from the Base; checked again after a save and a second session,
    where the live side has carried state and the replay has none.
13. **Bisimulation after a save.** Run L1 and save (J1, shadow kept). Open the output as a new
    Journal (J2). Run the same L2 on both. Refusals, normalized reads over the grid (before and
    after L2) and the next outputs are equal, the outputs in P and in bytes.
14. **`truncate_fast` = replay.** At a random cut, with and without an earlier save,
    `truncate_fast` and `truncate` (replay) give equal reads and byte-identical outputs.

## Delivery

Prerequisites (each its own task; the op stays `Unsupported` until it lands): **P1** before
J1 (ExcelJS clears); **P2** before J1 for session `xf` indices; **P3–P6** independently, each
joining oracle 1 with its own sequential reference when it lands. P7 and P8 are not planned
(Q1 (A), Q2 (A)). **C1–C5** (§7.4) before J1's Commit; until C1 lands, R1 is off (§4.5).

0. **J0** (native only): ids, vectors, log and key index (style groups), the shadow with R2 on
   C2, the projection P and its predicates, oracle 1's harness on keyed ops; the measurement
   spike of oracle 10 for append and save cost. The spike (`cb57de4`, `ad74d32`) did J0's
   keyed slice through the public API with the substitutes of §7.4; J0 redoes it on C1–C5.
1. **J1**: View (echo per group and normalized), Commit (§7) for keyed and barrier ops with
   R1–R4; oracles 2–5, 11 and 12. **Acceptance**: oracle 1 (sequential equivalence) and oracle
   11 (history independence, byte-identical) at 20,000 cases each, and every rule's mutant
   caught.
2. **J2**: structural ops (splice under Q2 (A) included); oracles 1 and 11 with structural ops,
   the epoch rule of §4.4; oracle 6.
3. **J3**: compaction, echo records, truncation with `truncate_fast`; oracles 7, 8, 13 and 14.
4. **J4**: the ExcelJS and SheetJS adapters switched to the Journal (replacing §13.3's
   sections); oracle 10.
5. Follow-up (own task, owner to confirm): the differential harness (§14).

## Alternatives considered

1. **Diff states at save** (state-based lens; XML 3-way merge [6-8]): has to guess the
   correspondence after structural edits; that guess is defect (d) [4-4]. Rejected.
2. **Keep byte-snapshot staging per setter** (ADR-0021): measured reopen cost per setter
   (Context), and no provenance. Rejected.
3. **A Rust-held candidate or `fork()` per edit or transaction** (ADR-0021's comparison,
   ADR-0023 §11.2): fixes atomicity only; ADR-0021's implementation notes found no consistent
   setter-latency gain across both browsers. Rejected for the Journal, which also fixes (a),
   (b) and (d).
4. **Coordinates as keys, shifted at each structural op** (OT style [3-1]): every structural
   op rewrites the pending edits; hidden information follows positions, not identities
   [4-2]. Rejected for ids; OT and id-based CRDTs differ mainly in where the work happens
   [3-6], and ids make keyed writes free.
5. **Id-form formulas throughout** [3-16]: every formula would be re-rendered at save,
   breaking ADR-0001 for untouched formulas. Rejected.
6. **General OT/CRDT** [3-7, 3-8, 3-9]: built for concurrent writers. One linear history
   needs only the TP1 condition [3-2]. Rejected.
7. **Memoized red facades**: about 2× memory [1-7]. Not the default.
8. **Validate on the View, without a shadow** (revision 1): re-implements every guard of
   ADR-0003/0009/0014/0017/0018 over Base + Journal, a second engine that can drift from the
   sequential API; a structural op needs every earlier epoch applied to validate (review M2).
   Rejected.
9. **Commit = serialize the shadow, without rules** (revision 7's Alternative 9 as written):
   write-backs keep the sequential strips and calcChain removal, garbage registry items stay,
   and the bytes depend on the history (F11). Each rule's mutant shows the difference (oracle
   9). Rejected; the decision (§7) is this with R1–R4.
10. **An `Arc`-shared copy-on-write working copy** (revision 1): not on `main`; `Package` and
    `Workbook` own their parsed parts. It would need an `ooxml-core` change and would still
    parse every worksheet for a structural op. Rejected for now (review M4).
11. **The compiled Commit** (revisions 2–7; Appendix A): normalize a copy of the log, open a
    working copy from the Base, compile epoch by epoch, serialize, reopen. Saved in 8.1–8.6× a
    sequential `to_bytes` (6× without the reopen) against the 2× target; batching gains nothing
    on `main` (F7); its placement rule needed a correction (F1). It needs no core API beyond
    P1/P2 and the plan types (F2), and it is an independent second engine. Not chosen (Q7 (B));
    kept as an appendix, and the spike keeps it runnable (`EJ_MODE=compile`) as a second
    reference for oracle 1.
12. **The compiled Commit, keeping the working copy as the next shadow** (F6, revision 7's
    §10.1 "left for measurement"): removes the reopen, but opening the Base alone costs about
    2.3× a sequential serialize, so the save stays above 2×. Not measured further. Rejected
    (Q7 (C)).

## Consequences

- Each edit costs its sequential call on the shadow plus R1's test and R2's snapshot and check
  (2–3 µs per op at 110k cells); no reopen or serialize per edit.
- A save costs R3, R4, `dimension` and one serialize of the kept shadow: 1.6–1.8× a sequential
  `to_bytes` in the spike, against 8.1–8.6× for the compiled Commit. A structural op runs once.
  No Base open and no reopen at save.
- The output depends on the final state of each cell, not on how it got there (oracle 11), so
  write-backs and reverts are free, as SheetJS-style region writes need.
- Core grows five APIs (C1–C5) before J1; without them the rules can only be emulated by a
  text layer that carries state across saves (Appendix B).
- Provenance is explicit, so echo-then-normalize behaviour is a read mode, not adapter code.
  Echoes end per style group.
- Atomicity no longer depends on each consumer: a failed Commit leaves the Base, and poison
  never escapes. A rollback, or a call refused part-way, costs a replay or a `truncate_fast`.
- No second engine re-derives the output: a silent bug or an uncovered history-dependent trace
  in the shadow reaches the output. Oracles 1, 11, 12 and 13 are the guard (§9.3).
- Session style indices are not readable before a save; adapters hold styles by content.
- Structural edits no longer move pending edits or hidden information by guesswork.
- Refusals equal the sequential API's by construction for part A, R1 included (validation
  first).
- Part B ops are refused until their prerequisites land, so some adapter behaviours wait.
- The adapters' per-item mechanisms in ADR-0021/0022 are superseded; their pinned behaviours
  must be re-established through the differential harness, and some may regress until then.

## Open questions for the project owner

**Owner decisions 2026-11-25** (Q1–Q6): every question is decided as its recommended option
(A). The options are kept below as the record of what was weighed. **Q7** (revision 8) awaits
the owner's confirmation.

- **Q1. Formula caches (`<v>`) at Commit.**
  - Options:
    - **(A)** apply ADR-0018 §3.7 for structural ops and ADR-0003 §3.3 for value writes,
      once, over the changed set (R3; revision 7: over the normal form): strip the caches of
      every formula touched, directly or transitively, and set `fullCalcOnLoad` (not in manual
      mode); untouched formulas keep their caches and bytes. A new formula has no cache
      (ADR-0003 §3.1a); a supplied result (ExcelJS `{formula, result}`) is echoed in the
      session (§6.1) but not written. `CalcPolicy::KeepStaleCaches` stays opt-in.
    - **(B)** keep every stale `<v>` and only set `fullCalcOnLoad` (make `KeepStaleCaches` the
      default).
    - **(C)** strip every formula cache in a touched sheet.
    - **(D)** (A), but a supplied result is written as `<v>` unless a later op puts the formula
      in the strip set. This changes ADR-0003 §3.1a and needs P7.
  - **Decided: (A).** It is the rule the sequential path already applies, so
    oracle 1 needs only the excluded calc-state difference; (B) shows stale values in
    LibreOffice, which ignores `fullCalcOnLoad` (ADR-0003 §3.3); (C) loses caches no edit
    affected; (D) trusts a caller-supplied value that nothing checks.
- **Q2. Bias, deleted positions, and splice.**

  Table (ADR-0018 §3.2, LibreOffice without `ExpandRefs`):

  | Reference | Insert before start | Insert at start | Insert strictly inside | Insert just after end | Delete covering it | Delete covering part |
  |---|---|---|---|---|---|---|
  | single cell (formula token) | moves | moves | – | unchanged | `#REF!` (prefixed form kept) | – |
  | range start/end (formula, `sqref`, merge, table) | moves | moves (not expanded) | expands | unchanged (not expanded) | `#REF!` / feature removed / refused per ADR-0018 §3.5–§3.6 | shrinks to the survivors |
  | whole rows/cols on the other axis | sticky | sticky | sticky | sticky | sticky | sticky |
  | range end at the sheet edge | pinned | pinned | pinned | – | deleted wins over pinning | pinned |
  | journal entry on a cell (keyed by id) | follows its id | follows its id | follows its id | follows its id | **dropped** (dead, §5.5) | – |
  | objects ADR-0018 refuses (chart series, print areas, pivot sources, …) | moves | moves | as range | unchanged | **refused** (`ReferenceDeleted`) | as ADR-0018 |

  - Options for **splice ops** (§8.2), the part that was open:
    - **(A)** a splice is `delete(p, d)` + `insert(p, i)` + writes, with the table above:
      formulas keep pointing at the same cells, references into the deleted rows become
      `#REF!`. Part A: verifiable by oracle 1 now;
    - **(B)** ExcelJS 4.4.0's behaviour (§8.2): cells move, cell formulas are not rewritten,
      defined names are, notes on moved cells are dropped. Needs P8;
    - **(C)** (B) by default with a per-call option for (A). Needs P8.
  - **Decided: (A).** Silent wrong references are the failure ADR-0018 was
    written to prevent ("places it cannot rewrite refuse instead of going stale"), and
    ooxml-rs fails closed rather than reproduce a data-losing behaviour (ExcelJS also drops
    notes). The difference from ExcelJS is recorded as an intended deviation in the
    differential harness.
- **Q3. Hidden Base information when a cell, row or column is overwritten, moved or copied.**

  Classes: **h1** unknown attributes and extension children of `c`/`row`/`col`; **h2** style
  index `s`; **h3** value-describing attributes (`t`, `cm`, `vm`, the cached value); **h4**
  row/col presentation (`ht`, `customHeight`, `hidden`, outline level); **h5** objects
  anchored to the cell (note, hyperlink, DV/CF membership); **h6** formula structure
  (shared-group membership, array range).

  Table (option (A)):

  | Operation | h1 | h2 | h3 | h4 | h5 | h6 |
  |---|---|---|---|---|---|---|
  | overwrite value in place | keep | keep | **drop** (describes the old value) | keep | keep | per ADR-0003 (hand-off) |
  | clear (P1) | keep | keep | drop | keep | keep | per ADR-0003 |
  | structural shift of surviving rows/cols | keep (follows the id) | keep | keep | keep | keep (moved by ADR-0018) | per ADR-0018 §3.3 |
  | copy sheet (P3) | keep | keep | keep | keep | per ADR-0018 §4 | keep |
  | copy cells/rows inside the session (P6) | **drop** | keep | drop for formulas, keep for constants | keep | **drop** | drop (written as own formulas) |
  | new cell (insert, splice) | – | the row/col default | – | – | – | – |

  A cell restored by R2 gets all of h1–h6 back from its snapshot.

  - Options:
    - **(A)** the table above: inherit along identity-preserving ops, drop on copies;
    - **(B)** inherit only `s` on every op (what a model-rebuilding library keeps);
    - **(C)** inherit everything, including on copies.
  - **Decided: (A).** Hidden data follows identity, the resourceful-lens rule
    [4-2]; a copy creates a new identity, and copying hidden data is the InteropGrabBag leak
    [7-8]. (B) loses data ADR-0001 promises to keep; (C) repeats the leak. Rows of (A) for
    part B ops apply when those ops land.
- **Q4. ADR-0023.** Rewrite ADR-0023 on the Edit Journal (§13.4) before its review round 2,
  or keep it as it is and adapt it after J3? **Decided: rewrite after this ADR is accepted**
  (the recommended option), so its transactions and batches are not built twice.
- **Q5. A save inside a transaction** (`with wb.transaction(): …; wb.save(); raise`).
  - Options:
    - **(A)** the binding and adapters refuse `save` while a transaction is open (an error at
      the `save` call; the Journal is unchanged);
    - **(B)** allow it; the rollback then fails with `TruncateBeforeCommit`, and the caller
      sees that the saved ops stay;
    - **(C)** allow it, and roll back by appending inverse ops after the commit mark.
  - **Decided: (A).** A rollback that cannot roll back is the silent failure M5
    found; (B) makes it loud but late; (C) needs an inverse for every op, which part B ops do
    not have. The core rule (§10.3) is the same under every option.
- **Q6. After a failed flush** (§11; a classifier bug or a state-dependent refusal).
  - Options:
    - **(A)** the flush is undone and the Journal is broken: every call but `truncate` and
      reload throws `JournalBroken` until the caller truncates (to any `seq` at or below the
      flush's start) or reloads;
    - **(B)** the flush is undone and the Journal stays usable, the caller having been told
      by one exception that the buffered calls since the last flush are gone;
    - **(C)** no buffering: every op is appended synchronously (the 1-call-per-setter cost).
  - **Decided: (A).** (B) lets a caller that catches and ignores the one
    exception save a file without edits it was told succeeded; (C) gives up §11's batching,
    which J0 measures before it is relied on.
- **Q7. How Commit produces the output** (revision 8; F6, F9).
  - Options:
    - **(A)** serialize the kept shadow under R1–R4 (§7), with no reopen; needs C1–C5 (§7.4).
      Spike: 1.6–1.8× a sequential `to_bytes`; history-independent bytes (oracle 11);
      `truncate_fast` 14–28× faster than replay;
    - **(B)** the compiled Commit of revisions 2–7 (Appendix A): 8.1–8.6× with the reopen, 6×
      without; F1 and F7 open; an independent second engine;
    - **(C)** (B) with the working copy kept as the next shadow (Alternative 12): no reopen,
      still above 2× because the Base is opened at every save.
  - **Default: (A).** It is the only option that meets ADR-0018 §3.8's 2× target, and the
    rules that make it correct are each covered by an oracle and a caught mutant. Its cost is
    the five core APIs and the loss of a second engine (§9.3), which oracles 1, 11, 12 and 13
    cover. The owner agreed on 2026-11-25 to take the fallback (shadow serialization) and
    reduce its drawbacks with remedies; this question records that direction for confirmation
    as the ADR's decision.

## Appendix A. Unimplemented alternative — the compiled Commit

Revisions 2–7 decided this Commit; revision 8 moved it here (Alternative 11, Q7 (B)). The text
is revision 7's, with section references renamed (A.1–A.6). The spike implements it as
`Mode::Compile` (commit `cb57de4`; `EJ_MODE=compile` in `ad74d32`) for keyed ops in one epoch.
A.8 lists what the spike found. In this appendix "Commit" means the compiled Commit; "§"
references point to the body, "A.n" to this appendix.

### A.1 Start state

The compiled Commit records, for each keyed op, the cell on the shadow just before the op:

```rust
struct LoggedOp { /* … as §4.2 … */ start: Option<Start> } // keyed ops only
enum Start { Formula, Missing, Other } // formula cell (incl. shared/array member), no `c`, else
```

N3 (iv) and A.4 read the key's **start state** in an epoch: the `start` of the key's first op
in that epoch.

### A.2 Normalization

`normalize(log)` produces the **normal form**: per epoch, a sequence of keyed entries and
barrier ops, using only these rules [5-1]:

| Rule | Statement |
|---|---|
| N1 | Content and row/col property keys, same epoch: an **unconditional full** op drops every earlier op on its key back to the key's latest two-half op (exclusive); a **conditional** op and a two-half op drop nothing, and a two-half op is never dropped (tables below). |
| N1s | Style keys, same epoch: patches are **composed**, not replaced (rules below). |
| N2 | Keyed entries on different keys commute. |
| N3 | A keyed entry of epoch `k` moves to epoch `k+1` across structural op `k+1` iff (i) its ids are live after op `k+1`; (ii) its payload holds no coordinate or sheet-name text, or op `k+1` rewrites no reference; (iii) it is outside the op's **guard zone**: on the edited sheet, the cells of every merge, table, array or data-table range, shared group, protected range and anchored object, and for an insert the last `n` rows or columns; (iv) its key's **start state** in epoch `k` (the `start` of the key's first op in that epoch) is not `Formula`, on **any** sheet, and is not `Missing` when its first op is a style patch. Otherwise it stays, together with every entry on its key. |
| N4 | A keyed entry on an id that op `k+1` kills is dropped (a tombstone; §5.5). |
| N5 | Structural ops never reorder among themselves. |
| N6 | An entry never moves to an epoch before its ids exist. |
| N7 | Barrier ops never move, and a keyed entry never moves past a barrier op on its sheet or of workbook scope. |

**What a structural op reads**, and which N3 condition covers it: every formula cell of every
sheet, rewritten or refused (`UnrewritableReference` for 3-D or unlexable formulas,
`ShiftOffSheet` for references pushed off the grid, `sheet_ops/grid.rs`), and counted by
`delete_sheet_strict` (`SheetInUse`): (iv); every registry place (ADR-0018 §2.2): changed only
by barrier ops, (N7); on the edited sheet, the features and the last `n` rows of ADR-0018
§2.4, §3.2, §3.3.2–§3.3.3, §3.5–§3.6: (iii); the row/col default a missing cell inherits
(`missing_cell_style`): (iv). Staying is always correct (Commit then applies the entry before
the structural op, as the sequential API did), so the rules may only lose batching, never
correctness. The normal form pushes every entry as late as N3 allows. N3 is the TP1
condition; it is tested, not argued (Test oracles 1, 9).

**Content folding (N1).**

| Kind | Ops | Reads the current content? |
|---|---|---|
| unconditional full | `set_number`, `set_string(_inline)`, `set_rich_string`, `set_bool`, `set_error`, `set_formula`, `set_dynamic_formula`, `write_cells` `Number`/`String`/`Formula` | no |
| conditional | `remove_formula`, `write_cells` `RemoveFormula` (a cell without a formula is left alone, `worksheet.rs`); P1 (a missing cell is left alone) | yes (P1: whether the cell exists) |
| two-half (not folded) | `set_datetime`, `*_with_format`: content and style request in one sequential call | its style half may (`EnsureDate`) |

**Two-half calls are not folded.** Such an op is one entry: its halves stay together, it is
never dropped by a later full content op or full patch, it drops nothing, and folding on its
key happens only between two of them. It compiles as one write in its own place in `seq`
order. Without this, a fold that replaces only its content half changes which op creates a
missing cell, and so the base its patch resolves on (review round 3, M1: `set_datetime` then
`set_number`, and the reverse, on a missing cell under a `col/@style`).

A conditional op is kept after the entries it follows and compiles in a later batch (A.4),
so it sees the same predecessor as sequentially (`set_formula` then `remove_formula` on a
Base constant leaves an empty cell, not the Base value). Composing it into one full write is
an optimization this ADR does not make.

**Style composition (N1s).** `StyleRequest` (`setters.rs`) has three kinds:

| Kind | Requests | Reads the current `xf`? |
|---|---|---|
| full | `Format` (unset groups from the default style), `Index(i)` with `i` a Base `xf`, P2's `xf` by content | no |
| partial | `NumFormatOnly`, `FontOnly`, `FillOnly`, `BorderOnly`, `Named` | yes (keeps the other groups) |
| conditional | `EnsureDate` (sets built-in 14 only if the current format is not a date) | yes |

Rule: per cell and epoch, keep the patches in `seq` order, except that a **full** patch drops
every earlier patch on that cell back to the latest two-half op (whose style half it never
drops). A partial or conditional patch drops nothing. The result
resolves to the same `xf` as the sequential chain, because a full patch ignores what was
before it and every kept patch sees the same predecessor chain. The sequential path may
append extra, unused `xf` entries; that is an excluded difference (Test oracle 1).
A content op never reads `s`; it writes `s` only by **creating** a cell, which then has no
`s` (0), while a patch on a missing cell starts from the row/col default
(`missing_cell_style`, `edit.rs`). A.4 keeps that order. Otherwise folding and composition
are independent, apart from the two-half rule above.

**Workbook-scoped side effects of content ops**, each matched to an exclusion of Test
oracle 1: string writes append `sst` items (registry exclusion); two-half calls and patches
append `xf`s and their font, fill, border and `numFmt` entries (registry exclusion);
`set_dynamic_formula` creates or extends the metadata part (metadata exclusion); formula and
value writes strip caches, set `fullCalcOnLoad` and remove `calcChain.xml` (calc-state
exclusions); writes outside `dimension` grow it (`dimension` exclusion). A dropped entry (N1, N4) can
leave such an effect on the sequential side only; no effect of the list is outside P's
exclusions or rules.

**Registry indices allocated in the session.** A payload that names a registry entry created
in this session (an `xf` from P2, a `dxf` returned by `add_differential_format`) is stored by
the entry's **content**, as the shadow holds it, not by index. Commit may allocate differently
(folding, N4, no-ops), and resolves it by reuse-or-append of identical bytes (ADR-0006 §5.8
for `dxf`; P2 for `xf`).

### A.3 Compilation

1. **Normalize** a copy of the log (A.2). The log itself is not changed.
2. **Drop the shadow**. Only the Base bytes, the log, the normal form and the vectors stay.
3. **Working copy**: `Workbook::from_bytes(Base)` (an existing call; census at open,
   ADR-0003 §1).
4. **Compile and apply, epoch by epoch**, on the working copy:
   - the keyed entries of epoch `k` → batches (A.4), each one `CellWritePlan` per sheet
     (ADR-0003 §3.1a), written at the coordinates of epoch `k` (id → index on the vector as it
     was in epoch `k`). Formula text is written **as given**, in its birth-epoch coordinates.
     A `set_dynamic_formula` entry is not batched: it also registers the metadata part
     (`apply_writes_with(plan, registry)`, `dynamic.rs`), so it compiles as its own
     sequential call, closing the batch before it (A.4);
   - each barrier op → its own sequential API call, in `seq` order relative to the batches;
   - structural op `k+1` → **exactly one** ADR-0009 or ADR-0018 operation, which rewrites every
     formula already written, as on the shadow;
   - each step's `Err` or poison stays inside the working copy;
   - `dimension/@ref` is kept by the working copy's own calls, by the sequential API's rule
     (`plan_target`, `worksheet.rs`: a written cell outside it grows it; a structural op
     rewrites it as a range, ADR-0018 §3.2). A **written cell** is one that the plan of any
     call Commit runs **targets** (passes `plan_target`): keyed entries, and barrier ops that
     write cells through the same plan (`set_hyperlink` with `text`, the hidden cells
     `merge_clearing` clears). A cell a conditional op skips (`remove_formula` on a cell
     without a formula, P1 on a missing cell) is not written. Commit does not
     copy the shadow's: an entry skipped as a no-op (A.5) or dropped (N1, N4) grows nothing,
     and on a sheet with no written cell and no structural op the `dimension` element keeps its
     Base bytes (oracles 4, 5). A sheet without `dimension` stays so.
5. **Serialize once**: Layer 1 raw-copies every part the working copy did not change; Layer 2
   re-renders only dirty nodes (ADR-0001), so each changed part is "original byte spans +
   replaced pieces", as a rope [1-8].
6. **Swap**: only when every step and the serialization succeeded is the output returned,
   the output becomes the Base, and compaction runs (§10). On any failure the working copy is
   dropped, the Base and log are unchanged, and the shadow is rebuilt by replay (§3.2).

Commit **reuses** the edit plans and their tests instead of adding a second editing engine.
What it adds is the order (epochs), the coordinate translation (ids → indices), folding,
and the one-shot serialization.

### A.4 Batches within an epoch

Entries are taken in `seq` order and placed greedily into the current batch, which is closed
when the next item cannot join it:

| Next item | Joins the current batch when |
|---|---|
| content entry for cell `c` | `c` is not yet in the batch, or `c`'s write there is `StyleOnly` with no content: they merge into one `(CellOp, Extra { style })` |
| two-half op for cell `c` | only when `c` is not yet in the batch (`Extra` holds one style request) |
| style patch for cell `c` | `c` is not yet in the batch, or `c`'s write there has content and no style request, **the key's start state is not `Missing`, and that content is not a conditional op**: they merge |
| conditional content entry (N1) | only when `c` is not yet in the batch; no later item for `c` joins it. `remove_formula` on a cell without a formula skips the whole write, a merged style request included (`edit.rs` L419-421), so a merged patch would be lost with `Ok` |
| any entry for cell `c` | otherwise never (`DuplicateCellWrite`, `edit.rs` L373-376); and never when the batch already writes another cell of `c`'s shared group (`SharedFormulaMaster`, ADR-0003 §4.2) |
| barrier op, or a `set_dynamic_formula` entry | never: the batch closes before it, and it runs alone (A.3 step 4) |

Entries of one key are placed in `seq` order; a folded full entry takes the place of the
key's **first** content op of its fold segment (A.2), so a patch made after the cell was
created stays after it. When the start state is `Missing`, the base is decided by the key's
first op in the epoch in `seq` order **before folding**, skipping conditional content ops
(on a missing cell they create nothing, as sequentially): if it is a content op that creates
the cell, that content is written alone first (no `s`, as sequentially) and every patch resolves on 0 (a merged
write would resolve on the row/col default, review round 2, M3); if it carries a style half
(a patch or a two-half op), it creates the cell with the row/col default, as sequentially.
A two-half op is never folded and no fold crosses it, so a fold can change the first op's
kind only when a full patch drops a first patch, and a full patch ignores its base: the
`xf` is the same either way, and reading before folding keeps the rule unique. A two-half
op is one write with both halves, as the sequential call is. Merging is otherwise sound because a content op never reads `s` and a style patch never reads the value. So
"write a value to an existing cell, then set a fill" is one write; "set a fill, then a font, then a
border" on one cell is three consecutive batches, each patch resolved on its predecessor, as
sequentially.

### A.5 No-op entries at Commit

At step 4, an entry is skipped as a **no-op** only when its cell **exists** (`c` is present) in
the working copy at that point and the entry changes nothing there: a content entry whose
payload **reads equal** to the cell (ADR-0023 §6.2: same value kind and value, same formula
text; a formula never reads equal to a constant), or a style batch entry whose resolved `xf`
equals the cell's `s` (0 when the attribute is absent). An entry on a **missing** cell is never
a no-op, whatever the row/col default: the sequential call creates the cell (with `s`, or
without it for a content op, A.2), and that `c` changes how the cell is formatted (review
round 5, M1: `set_style_index(0)` on a missing cell under a bold `col/@style`). A conditional
op on a missing cell is compiled as its own call, which does nothing there, as sequentially. A read → modify → write-back of a region changes only the cells the caller
changed, and reverting a value removes its edit (ADR-0021's SheetJS rule). A skipped entry
is not written, so it does not grow `dimension` either, unlike the sequential call (A.3
step 4). The View still echoes the op until compaction (revision 7's §10.1).

### A.6 Cost and memory

- At most **one parsed workbook** is alive at a time: the shadow before Commit, the working
  copy during it, the reopened output after it (revision 7's §10.1). The Base bytes and the output bytes
  are extra, as in today's open and save.
- No worksheet is cloned. ADR-0018 §2.1 rejected a staged clone (289 ms for a 200k-formula
  sheet, the tree's memory doubled); keeping the shadow alive next to the working copy would
  be that cost, which is why step 2 drops it.
- A save costs one open of the Base (census: 0.29 s for a 1M-cell sheet, ADR-0018 §3.8 citing
  ADR-0010), the compiled plans, one serialize, and one reopen of the output (revision 7's §10.1), against
  the sequential path's serialize alone. It is measured against ADR-0018 §3.8's 2× target in
  J0 (Test oracle 10). It failed: see A.8, F6.

Spike `cb57de4` measured it (same machine and workload as §7.3, one run each confirmed by two
or more within 10%):

| | 10,000 rows (110k cells) | 100,000 rows (1.1M cells) |
|---|---|---|
| Commit: open / apply / serialize | 160 / 168 / 70 ms = 419 ms | 1,765 / 1,388 / 666 ms = 3,998 ms |
| peak heap during Commit (shadow dropped) | 47.0 MiB | 446.9 MiB |
| reopen of the output (revision 7's §10.1) | 162 ms | 1,742 ms |
| sequential `to_bytes` | 69 ms | 684 ms |
| **save (Commit + reopen) / sequential `to_bytes`** | **8.4×** | **8.4×** |
| `normalize` + batches (2,001 ops → 1,992 entries, 2 batches) | 1.5–1.8 ms | 1.7 ms |

### A.7 Oracle parts of the compiled Commit

- Oracle 1 was stated as "sequential application ≡ normalize, then compile", with a second
  property `normalize(normalize(log)) = normalize(log)` (held over 1,024 cases in the spike).
  Oracle 11 replaces it as the acceptance criterion of the body.
- Mutants of this design (oracle 9 of revision 7): N3 moving an entry out of the guard zone
  rule; N3 moving an entry whose start state is a formula on another sheet (round 2 M1);
  folding a conditional content op last-wins (round 2 M2); merging content and patch on a
  cell the content creates (round 2 M3); folding the content half of a two-half op, or folding
  across it (round 3 M1); copying the shadow's `dimension` at Commit, or growing it for a
  skipped no-op (round 4 M2); treating an entry on a missing cell as a no-op (round 5 M1);
  merging a style patch into a conditional content op's write (round 4 M1); N7 moving an entry
  past a barrier; last-wins on style patches (B3); merging two style patches into one write.
  The spike's `--features` mutants for the keyed slice (`mutant_noop_missing`,
  `mutant_merge_missing`, `mutant_style_last_wins`, `mutant_cond_last_wins`,
  `mutant_keep_shadow`, `mutant_fold_at_append`, `adr_literal_placement`) each fail a test
  under `EJ_MODE=compile`.

### A.8 Issues found by the spike

Recorded as this design's issues; they are not fixed here because the design is not chosen.

- **F1. A.4's placement rule contradicts the `Missing`-start base rule.** Placement: "a folded
  full entry takes the place of the key's first content op of its fold segment"; base rule: the
  base of a missing cell is decided "skipping conditional content ops". Counterexample: `B6`
  missing in a `customFormat` row with `s=2` (a fill); `remove_formula`,
  `set_num_format("0.00")`, `set_number(1)`. Sequentially `remove_formula` does nothing, the
  patch creates the cell with the row's fill, and the number keeps it; under the literal
  placement the number takes the `remove_formula`'s slot, creates the cell with `s` = 0, and the
  fill is lost. The spike places a folded full entry at the segment's first **unconditional**
  content op (its own `seq` if there is none); 20,000 cases then pass, and the literal rule
  (`adr_literal_placement`) fails within about 1,000. The case is a fixed case of oracle 1 in
  the body.
- **F2. The compiled Commit needs crate-private plan types**, and A.5's style no-op needs a
  style API that resolves without applying. A.4's merged `(CellOp, Extra { style })` write,
  and batched booleans, errors and two-half ops, exist only through `plan_writes` /
  `CellWritePlan`; `write_cells` carries no style request. Telling "reads equal" apart for a
  shared **rich** string needs the `si` runs, which the public API does not expose. (The
  plan-without-apply part is C1 of the body.)
- **F4. Oracle 1's `sst/@count` predicate fails on `main` for the sequential side.** A number
  written over a shared-string cell leaves `count="2"` with one `t="s"` cell, because the
  string registry is not activated. The body states the predicate per side (Test oracle 1).
- **F5. Oracle 1's predicate "`fullCalcOnLoad` set if any cache was stripped or formula
  written" fails on `main`.** `set_formula` on a cell with no dependents strips the cell's own
  cache but does not set the flag (ADR-0003 §3.3 item 2 sets it only for a dependent or a
  formula-set change). The body uses ADR-0003's condition (Test oracle 1).
- **F6. Save cost 8.4× a sequential serialize** at 110k and 1.1M cells, against 2×. Opening the
  Base (≈ 2.3× a serialize) and the reopen (≈ 2.3×) dominate; without the reopen it is 6×.
  Resolved by Q7.
- **F7. Batching does not pay on `main`.** One `write_cells` of 10,000 distinct cells applies
  in 294 ms; 10,000 `set_number` calls take 180 ms (1.1M cells: 1,888 vs 1,601 ms). The gains
  come from folding and no-op skipping, not from batches; this design should not count batching
  as a cost win before `write_cells` scales.

## Appendix B. Spike evidence for §7 (commit `ad74d32`)

Evidence, not design: the spike runs §7.1's rules through `ooxml-xlsx`'s public API, so it
stands in for C1–C5 with a **text layer** (`src/shadow.rs`). Details are in the spike's README,
section "Commit = shadow serialization".

- **Text layer.** It opens the Base lazily as a `Package` (nothing parsed), takes each part the
  shadow dirtied, rewrites it, and calls `set_part` only where the result differs from the
  Base's text; every other part is raw-copied. Restored cells are written as the Base's bytes
  for that `c` (with `s` set where only the style changed); every other written cell takes the
  shadow's bytes in a canonical form (registry indices renumbered, `xf` 0 written as the Base
  cell wrote it, a childless element self-closing); `dimension` is the Base's element grown by
  the changed cells; R3 strips `v`/`is`/`t` in text and adds `fullCalcOnLoad` to the Base's
  `calcPr` text; R4 appends surviving items after the Base's in canonical order.
- **Carried state.** Because the kept shadow is not the output, the layer carries the difference
  across saves until a later write replaces it: **pins** (cells whose output is the Base's
  bytes, with the Base cache for the View), **strips** (made by a save; a shadow that never
  dirtied the sheet still holds the stale cache) and **registry maps** (shadow index → Base
  index for saved session items; `set_style_index(i)` and the View translate). C2 and C4 make
  all three unnecessary.
- **Counterexamples found on the way** (F11; each fixed in the layer, each a reason for C2 or
  C4): `set_style_index(0)` on a cell without `s` writes `s="0"`, which a later value write
  keeps; `set_formula` drops the cache, so a formula written away and back, then restyled,
  lost its Base cache (and a later `remove_formula` diverged from a replay); a node that loses
  its children serializes as `<c …></c>`; a strip made by a save must survive a later
  style-only write; `fullCalcOnLoad` is not monotone on a `KeepStaleCaches` shadow (20,000 cases
  found histories where the deferred rule needs the flag but the shadow never set it, so the
  layer sets it on the Base's text); after a save a garbage `xf` can be revived by dedupe, so
  the shadow's numbering differs from a Journal opened from the output (F12).
- **Tests.** The whole earlier suite runs in this mode (it is the default) and under
  `EJ_MODE=compile`. `tests/shadow_mode.rs` has oracles 11–14 as proptests (512 cases per run;
  one run at `EJ_CASES=20000`: all six proptests in 119 s, and `oracle1` in this mode in 134 s,
  all pass) and one fixed case per rule. A failed save keeps the shadow and the carried state;
  a retry gives the same bytes.
- **Mutants.** `mutant_no_noop` (R1) fails the white-box no-op test and the GetPut skip count;
  `mutant_no_revert` (R2) fails GetPut, oracle 1 (the `dimension` predicate), both history
  tests, replay, bisimulation and the registry test; `mutant_no_defer` (R3) fails GetPut, the
  revert-under-dependent case, both history tests, replay, bisimulation, `truncate_fast` and the
  registry test; `mutant_no_gc` (R4) fails GetPut, both history tests, `truncate_fast`, and the
  registry and `xf` 0 tests.
- **Save breakdown** (§7.3's runs): calc decision 2.0 / 5.5 ms; registries 2.2 / 4.6–5.1 ms;
  sheets (shadow part serialized, Base part inflated, scan, rewrite) 42–47 / 486–501 ms; zip
  (one part recompressed, the rest raw-copied) 66 / 647–680 ms, at 110k / 1.1M cells. Heap of
  2,000 appends: +2.2 MiB (snapshots) against +1.6 MiB in the compiled mode.
- **Limits.** Keyed ops only (no structural or barrier ops), one epoch; the no-op test runs
  before validation (F13); style no-ops only for `set_style_index`; `truncate_fast` writes
  snapshots back with ordinary writes and falls back to replay for missing, empty, rich, inline
  and shared-formula cells; `raw_input` is kept in Rust.

## Sources

Numbers follow the bibliography compiled for this ADR (Evidence, above).

- [1-1] Crowley, C. "Data Structures for Text Sequences." University of New Mexico, 1998
  (unpublished manuscript).
- [1-3] Lyu, P. "Text Buffer Reimplementation." VS Code Blog, 2018-03-23.
- [1-6] Lippert, E. "Persistence, Facades and Roslyn's Red-Green Trees." 2012-06-08.
- [1-7] rust-analyzer / rowan, and the rust-analyzer "Syntax" design document.
- [1-8] Boehm, Atkinson, Plass. "Ropes: An Alternative to Strings." SP&E 25(12), 1995.
  DOI 10.1002/spe.4380251203.
- [2-2] Fowler, M. "CQRS." 2011.
- [2-4] PostgreSQL documentation, "Concurrency Control — Introduction (MVCC)" and "Write-Ahead
  Logging".
- [2-5] Berenson et al. "A Critique of ANSI SQL Isolation Levels." SIGMOD '95. DOI
  10.1145/223784.223785.
- [2-6] O'Neil et al. "The Log-Structured Merge-Tree (LSM-Tree)." Acta Informatica 33(4), 1996.
  DOI 10.1007/s002360050048.
- [2-7] Rodeh, O. "B-trees, Shadowing, and Clones." ACM TOS 3(4), 2008. DOI
  10.1145/1326542.1326544.
- [2-8] Linux kernel documentation, "Overlay Filesystem".
- [3-1] Ellis, Gibbs. "Concurrency Control in Groupware Systems." SIGMOD '89. DOI
  10.1145/67544.66963.
- [3-2] Nichols et al. "High-Latency, Low-Bandwidth Windowing in the Jupiter Collaboration
  System." UIST '95. DOI 10.1145/215585.215706.
- [3-3] Ressel et al. "An Integrating, Transformation-Oriented Approach to Concurrency Control
  and Undo in Group Editors." CSCW '96. DOI 10.1145/240080.240305.
- [3-5] Imine et al. "Proving Correctness of Transformation Functions in Real-Time Groupware."
  ECSCW 2003. DOI 10.1007/978-94-010-0068-0_15.
- [3-6] Sun et al. "Real Differences between OT and CRDT under a General Transformation
  Framework …" PACMHCI 4(GROUP), 2020. DOI 10.1145/3375186.
- [3-7] Shapiro et al. "Conflict-Free Replicated Data Types." SSS 2011. DOI
  10.1007/978-3-642-24550-3_29.
- [3-8] Kleppmann, Beresford. "A Conflict-Free Replicated JSON Datatype." IEEE TPDS 28(10),
  2017. DOI 10.1109/TPDS.2017.2697382.
- [3-9] Nicolaescu et al. "Near Real-Time Peer-to-Peer Shared Editing on Extensible Data
  Types." GROUP '16. DOI 10.1145/2957276.2957310 (Yjs).
- [3-12] ProseMirror Guide ("Transformations") and reference manual (`prosemirror-transform`:
  `StepMap`, `Mapping`, `MapResult`).
- [3-13] Microsoft Fluid Framework, `@fluidframework/matrix` (SharedMatrix) README.
- [3-15] Danziger et al. "Systems and Methods for Mutations and Operational Transforms in a
  Collaborative Spreadsheet Environment." US 9,720,897 B2 (Google), 2017.
- [3-16] Coblenz, Flisakowski. "Reference Representation and Transformation in Collaborative
  Documents." US 9,684,646 B2 (Apple), 2017.
- [4-1] Foster et al. "Combinators for Bi-Directional Tree Transformations." POPL 2005; TOPLAS
  29(3), 2007. DOI 10.1145/1232420.1232424.
- [4-2] Bohannon et al. "Boomerang: Resourceful Lenses for String Data." POPL 2008. DOI
  10.1145/1328438.1328487.
- [4-3] Hofmann, Pierce, Wagner. "Edit Lenses." POPL 2012. DOI 10.1145/2103656.2103715.
- [4-4] Diskin, Xiong, Czarnecki. "From State- to Delta-Based Bidirectional Model
  Transformations: The Asymmetric Case." JOT 10, 2011. DOI 10.5381/jot.2011.10.1.a6.
- [5-1] Roundy, D. "Darcs." Haskell Workshop 2005. DOI 10.1145/1088348.1088349.
- [6-2] Bendre et al. "Towards a Holistic Integration of Spreadsheets with Databases: A
  Scalable Storage Engine for Presentational Data Management." ICDE 2018. DOI
  10.1109/ICDE.2018.00020.
- [6-5] Mokhov, Mitchell, Peyton Jones. "Build Systems à la Carte." PACMPL 2(ICFP), 2018. DOI
  10.1145/3236774.
- [6-7] Hammer et al. "Adapton: Composable, Demand-Driven Incremental Computation." PLDI
  2014. DOI 10.1145/2594291.2594324.
- [6-8] Lindholm, T. "A Three-Way Merge for XML Documents." DocEng 2004. DOI
  10.1145/1030397.1030399.
- [7-2] ExcelJS 4.x README.
- [7-5] Open XML SDK: "How to: Parse and read a large spreadsheet document".
- [7-8] Vajna, M. "InteropGrabBag in LibreOffice Writer." FOSDEM 2014.
