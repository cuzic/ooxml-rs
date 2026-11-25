# ADR-0024: xlsx Edit Journal — immutable base, typed journal, view, shadow commit

- Status: **Accepted** (revision 7; Opus review rounds 1–6, owner decisions 2026-11-25);
  revision 8 (Commit = serialization of the shadow, §7) **Accepted** (owner confirmed Q7 as
  (A) reached through (A′), 2026-11-25): amended after Opus review
  rounds 7 (1 Blocker, 5 Major), 8 (6 Major) and 9 (2 Major), then **spike-verified**: instead
  of a tenth review round the owner had the spike run revision 8's rules, and the four rule
  wordings that failed there are corrected (Appendix B.2). Opus review round 10 checked the
  document against the spike and converged (0 Blocker, 0 Major; its 5 Minor and 6 nit are
  applied). The owner confirmed Q7 on 2026-11-25, so revision 8's sections are the decision
  and revision 7's compiled Commit (Appendix A) is a rejected alternative, kept as a test-only
  second reference for oracle 1 until J2 (Q7's side question).
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
  shadow serialization with the four rules of §7.1). Its findings are cited as F1–F14. Review
  round 7 showed that part of that evidence is weaker than revision 8 first claimed (Appendix B.1,
  "What the spike does not show"). Commits `5eb021d`..`6339047` then ran revision 8's rules,
  with a mutant for each, on shared and array formulas, row and column structural ops and a
  second session (spike README, section "Revision 8 verification"; Appendix B.2).
- Review: [round 1](0024-opus-review-round1.md), [round 2](0024-opus-review-round2.md),
  [round 3](0024-opus-review-round3.md), [round 4](0024-opus-review-round4.md),
  [round 5](0024-opus-review-round5.md), [round 6](0024-opus-review-round6.md); revision 8:
  [round 7](0024-opus-review-round7.md), [round 8](0024-opus-review-round8.md) and
  [round 9](0024-opus-review-round9.md) (none converged; all addressed below); then the spike
  verification of revision 8 (Appendix B.2) in place of a tenth round.

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
  - **New oracles** (F10) 11 (history independence, byte-identical), 12 (replay equality), 13
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
  - **Core APIs** the spike stands in for are listed in §7.4 (C1–C5, C6–C8 after round 7) and
    become Delivery prerequisites; the spike's text layer is evidence, not design (Appendix B).
  - Other text follows: §2, §3.2, §4.2–§4.5, §5.3–§5.5, §6, §8.1, §8.3, §9, §10, §13.2,
    Consequences, Alternatives 9, 11 and 12, Open questions Q7.
  - **Amended after Opus review round 7** (B1, M1–M5, m1–m7, n1–n3). The spike's code is not
    changed; where it was weaker than claimed, Appendix B and §7.3 say so, and the rules are
    corrected with fixed cases and mutants for J1/J2.
    - **B1:** R3's strip set subtracts only the cells whose **content** differs from their
      snapshot, not the whole changed set: a formula cell restyled while an input changed is
      stripped (§7.1). The spike had the bug, and its oracle 1 cache predicate implemented only
      the first half ("a kept `v` is the Base's"), so its 20,000 cases do not cover this
      (Appendix B, §7.3). J0 must implement the whole predicate (Delivery); oracle 1 fixed case,
      oracle 9 mutant.
    - **M1:** new **C6 `rebase_after_save`**: the serialization records the new spans, the
      shadow's parse source and the package's original become the output, and every dirty flag
      is cleared. Without it a snapshot taken in a second session is a dirty node whose spans
      point into the first Base, and a restore would copy pre-save bytes (§7.2 step 7, §7.4,
      §10.1). Oracles 12 and 13 fixed case; oracle 9 mutant.
    - **M2:** snapshots and the changed set cover every cell a call's plan **targets**, the new
      master of a shared-group hand-off included (§4.4). R1's "invisible" claim excludes shared
      and array members (§7.1); oracle 11's noise is restricted to cells R2 can restore. Oracle 1
      fixed cases (write to a shared master and back; a shared member's own text).
    - **M3:** under `CalcPolicy::Deferred` a structural op returns its ADR-0018 §3.7 strip set
      (ids) and whether it moved or deleted a formula cell; the Journal records both and R3
      uses them (C3, §4.2, §7.1). Conservative, to be verified in J2; J2 fixed cases.
    - **M4:** `truncate_fast` re-applies only the cell's ops after its current snapshot was
      taken (§10.3). Oracle 14's generator cuts across a kept structural op (J3).
    - **M5:** the spike's text layer canonicalized cells that were not reverted (`s="0"`,
      self-closing form) and restored the cache of a formula whose content came back under a new
      style, which C1–C5 do not provide. **C2** now restores content with the current style, and
      new **C7** writes changed cells in a canonical form; §7.4 tables which API removes each
      F11 trace. Oracle 11 gets a style round trip after F's content ops.
    - **m1:** R1 is "C1's plan is the identity on the cell under §4.4's state equality" (both
      halves of a two-half op); absent `s` equals `s="0"` (§4.4, §7.1).
    - **m2:** `sst/@count` subtracts the `t="s"` cells a structural op deleted (R4; J2).
    - **m3:** C5 is one rule: Journal writes never touch `dimension`; structural ops rewrite it;
      the save grows it from the changed set.
    - **m4:** the shadow is marked stale before step 2 (§7.2, §9.3); a replay that fails is
      `JournalBroken` (§12).
    - **m5:** sections rewritten in revision 8 are headed "(proposed, revision 8)"; Q7 states the
      facts the owner needs and asks whether the compiled Commit stays as a test-only reference.
    - **m6:** §7.4's table covers the snapshot read, state reads, `restore_cell(None)`, a dry-run
      strip query and the registries' reverse lookups (C1–C8).
    - **m7:** a normalized read asks the strip question per cell; oracle 10 measures alternating
      setters and getters (§6.1).
    - **n1–n3:** F10 cited; oracle 11/13 scopes defined; §9.3's first save row is "before
      step 2".
  - **Amended after Opus review round 8** (M1–M6, m1–m6, n1–n4). Several round-7 fixes had
    opened new holes; the corrections prefer conservative rules and fallbacks to new machinery.
    No spike code changed; the spike README notes what it does not show.
    - **M1:** R1 is judged by §4.4's state equality plus "changes nothing a later validation
      reads" (reported by C1), and never skips a write on a shared-group or array cell. Revision
      8's R1-by-P skipped a shared member's own text, so the shadow's group stayed whole while the
      sequential one split, and a later batch was refused on one side only. R1 is now an
      optimization: output and refusals are the same with it off (§4.5, §7.1, Consequences).
      Oracle 1 fixed case (member written back, then master and member in one `write_cells`);
      oracle 9 mutant.
    - **M2:** a snapshot holds the cell's derived state (census entry, dependency edges, `vm`,
      row index) and `restore_cell` puts it back (§4.4, R2, C2). Oracle 12 compares the shadows'
      derived state white-box; oracle 1 and oracle 14 fixed cases; oracle 9 mutant.
    - **M3:** changed-set membership is a sticky **carried mark** from earlier epochs plus the
      current-epoch difference; a restore ends only the latter (§4.4, R2, §6.1, §7.2 step 4,
      §9.1, §9.2, oracle 4). The Journal-side `dimension` predicate also covers every targeted
      cell that differs from its Base cell, independently of the changed set. J2 fixed case
      (`F20` across `insert_rows`); oracle 9 mutant.
    - **M4:** C6 records the output span of **every** node of each re-rendered part (clean
      nodes move too), with a named fallback (reparse the part and rebuild its census) to be
      chosen in J1; until C6 lands, Q7 (A′) reopens the output. Oracle 12 fixed case
      (`A1`/`A3` then `A5` in a second session); oracle 9 mutant; cost in oracle 10.
    - **M5:** `truncate_fast` falls back to replay when a cut or re-applied op belongs to a call
      with more than one target or targets a shared-group or array cell (§10.3). Oracle 14
      generator case (hand-off cut, J3); oracle 9 mutant.
    - **M6:** oracle 1's cache predicate gains a **structural direction**: every cell the
      sequential side's structural ops stripped (white-box record, by id) that still holds `f`
      in the Journal's output has no cache there. The generator draws position-dependent
      formulas, whole-column references, named ranges and 3-D ranges (J2); fixed cases; the
      "structural strip records dropped" mutant is now caught by generated cases.
    - **m1:** `StructuralCalc.strips` is the op's own ADR's strip set (ADR-0018 §3.7 or ADR-0009
      §12 per sheet op); `formula_moved` is renamed `removes_calc_chain` and covers
      `delete_sheet` (§4.2, R3, oracle 1); J2 fixed case with `move_sheet` and a 3-D range.
    - **m2:** `would_strip(cell)` is defined as membership in R3's strip set, with no conditions
      of its own (§6.1).
    - **m3:** C2 is a J0 prerequisite; §10.1's minimal part moves to J1; oracle 10 measures
      §7.2 steps 2–7 (Delivery).
    - **m4:** §10.2 says the spike's second session rests on the text layer, not C6; §7.3 says
      C6 and C7 are in the save J0 measures, not in the spike's figures.
    - **m5:** C7 stays in `ooxml-xlsx` (an empty-element node for a childless changed `c`), so
      Layer 2's default output is unchanged; C2 and C6 change `ooxml-core` and must pass every
      format's ADR-0001 tests and the python-pptx oracle (§7.4, Q7).
    - **m6:** Q7 adds that (B)'s spike evidence uses the same half cache predicate, the size of
      C1–C8 by crate and invariant, that R1 is optional, and option **(A′)** ((A) with a reopen
      after each save until C6 lands).
    - **n1–n4:** R1 now uses §4.4's state equality and requires the rest of the node (h1,
      `cm`/`vm`) unchanged, so the partial list of what P models is gone; `sst/@count` is defined when the Base's
      `sst` or its `@count` is absent; R4 says what can reference a session item; "carried
      strip" is replaced by "a strip made by a save (`f` without `v`)".
  - **Amended after Opus review round 9** (M1, M2, m1–m5, n1, n2). Both Majors were stale caches
    with `Ok` from a rule stated on state that disagreed with the sequential history; both are
    fixed by the simpler, more conservative rule the review proposed. No spike code changed.
    - **M1:** **every** structural op (row/col or sheet op) closes **every** open snapshot of the
      workbook, not only those of its own sheet and of the formulas whose text it rewrites. A
      snapshot on another sheet survived `insert_rows`, so R2 put back the Base formula with its
      pre-op cache and derived state in a later epoch (`A!X = MATCH(7,B!A:A,0)`, a constant,
      `insert_rows` on `B`, the text written back). A snapshot is taken at a cell's first write
      while it has no open snapshot (the "in an epoch" wording is gone; §2, §4.4). Oracle 1's
      structural direction gains a clause independent of the sequential side's record (a
      structures-only probe run); J2 fixed cases; oracle 9 mutant.
    - **M2:** R3 no longer subtracts the content-changed cells: it strips every cell of the strip
      set that holds both `f` and `<v>` at the save, which is what the sequential `plan_strip`
      does. A hand-off's new master is content-changed (new `ref` and text) but keeps its Base
      `<v>`, so it was never stripped when its input changed (§2, §4.4, §6.1, R3). Oracle 1 fixed
      case; oracle 9 mutant.
    - **m1:** C4 returns a registry part whose session items all dropped (counts back to the
      Base's) to its Base node, clean, so R2's "Layer 1 raw-copies a reverted part" holds for
      `sharedStrings.xml` and `styles.xml` too.
    - **m2:** row/col property ops are in §9.1's allowed diff and make `truncate_fast` fall back to
      replay (§10.3).
    - **m3:** C7 applies to every changed cell whose node is not the Base's, a cell restored to a
      later epoch's snapshot included.
    - **m4:** J0's oracle 1 compares the unsaved shadow with the sequential workbook (P and
      refusals); the save-side predicates, the cache predicate in full included, move to J1 with
      the Commit; the save cost is measured in J1.
    - **m5:** `truncate` cuts only at call boundaries; R1 skips a multi-cell call only as a whole
      (§10.3, §7.1).
    - **n1:** C7's `xf` 0 rule depends only on the snapshot and the final state.
    - **n2:** Q7's facts add that M1 and M2 are errors only (A) can have.
- **Revision 8, spike-verified** (spike commits `5eb021d`..`6339047`; Appendix B.2). Instead of
  a tenth review round the owner had the spike run revision 8's rules, with a mutant for each
  rule and each round 7–9 detail, on shared and array formulas, row and column structural ops
  and a second session. Each rule is now stated as the spike runs it. Status unchanged:
  Proposed until the owner confirms Q7.
  - **R3, strip test:** also strips a result cell of an array (or spill) anchor in the strip set
    that holds `<v>` (`set_number(A2, 99)` under `G2:G3 = {A2:A3*2}` left `G3`'s cache) (§7.1,
    §6.1).
  - **R3, seeds:** "content-changed" is decided as P compares content (a shared member's text
    expanded, value, type; group attributes ignored); §4.4's raw equality stays for R1 and R2
    (`set_formula(E3, "$A3+E$1")` then `write_cells([D2: 1, E3: 2])` stripped a dependent of the
    hand-off's new master on the Journal side only) (§4.4, §7.1). With this seeding round 7's
    subtraction and "nothing subtracted" are equivalent (an equivalent mutant); the simpler
    statement is kept.
  - **R3, calcChain:** removed when the formula-cell set, counted by id with dead ids included,
    differs from the Base's (`A5` was `=1+1`, `set_number(A5, 5)`, `delete_rows(4, 1)` kept the
    Base's calcChain) (§4.4, §7.1, oracle 1).
  - **§4.4, missing cells:** a missing cell's content is empty; a cell created by a style patch
    is a style-only change (`set_font` on missing `B!A7` stripped a whole-column reader).
  - **Oracle 1:** the cache predicate's second half reads "differs in content", not "differs in
    P" (a style-only input change strips nothing on `main`); an array result cell's `<v>` is a
    cache; fixed cases for the four findings.
  - **R1** stays off until C1: the spike's R1 before validation accepted writes the sequential
    API refuses (`MergedCell`, `ArrayResultCell`); the cost of R1 off is 25 instead of 8 µs/op on
    a write-back-heavy second session (§4.5, Q7).
  - **Oracle 11:** byte-identical within an epoch; across one, P-equal (259 of 385 spanning
    blocks byte-identical), as §4.4 says (§7.1, oracle 11).
  - **Measurements** (§7.3): save (A) 2.01× in one run (was 1.6–1.8×; the extra is the epoch
    support in the text layer), so the 2× target is reached only within noise, only with the kept
    shadow, and depends on C6; J1 must show it on C2–C8 (oracle 10). (A′) 4.64× for save and
    reopen, measured (replaces the ~4× estimate in Q7), plus the dependency-graph rebuild the next
    session pays (81 vs 25 µs/op, about 1.7× a `to_bytes` at 2,000 ops); append 49.8 µs/op; `truncate_fast` 43 vs replay
    307 ms; `insert_rows` through the Journal 799 vs 149 ms bare (stand-in overhead).
  - **Verified now**, moved out of the open lists (§4.2, §4.4, §4.5, R3, R4, §10.2, §10.3, Q7,
    Appendix B.2): the full cache predicate, shared and array hand-off, R1 narrowing and refusal
    equivalence, structural strip records, workbook-wide snapshot closing and carried marks,
    `dimension`, `sst/@count` after deletes, calcChain on a moved formula, `truncate_fast` with
    its fallbacks, the second session under (A) and (A′). **Still unproven**: sheet ops and their
    records, splice, barrier ops, P1, the round 8 M5 hand-off cut, the white-box derived-state
    comparison of oracles 12 and 14, a real C6 span walk, style-sensitive workbooks, 1.1M-cell
    measurements; `set_dynamic_formula` (spill result cells, the metadata predicate, `vm` as derived state; the fixtures have no dynamic arrays); rich runs and shared rich strings (C8); the row and column property ops (§10.3); call-boundary truncation (`TruncateInsideCall`, review round 9, m5); the return of a registry part to its Base node (C4; the text layer compares texts); a second session after a session with a structural op under (A) (the spike always reopens after such a save); the white-box comparison of a real `Deferred` op's record (C3); `fullCalcOnLoad` for a structural op whose formulas hold no cache (`needs_full_calc`, R3; review round 10, m3). Appendix B.2 lists how the spike stands in for
    C1–C8.
  - **Oracle 9** gains the `adr_*` mutants and records the two equivalent mutants.
- **Revision 8, Opus review round 10** (converged: 0 Blocker, 0 Major, 5 Minor, 6 nit; all
  applied, `0024-opus-review-round10.md`). The 2× target is stated as reached within noise
  (2.01×, one run) and the `ad74d32` "meets 2× at both sizes" as historical (m1); (A′)'s cost
  includes the next session's dependency-graph rebuild, and the §7.3 rows say whether they
  include the reopen (m2); R3 sets `fullCalcOnLoad` when a structural op meets its ADR's
  condition, recorded as `StructuralCalc.needs_full_calc`, and oracle 1 checks the flag in both
  directions (m3); the still-unproven lists are completed (m4); C6 is a prerequisite of (A), not
  of J1, which may ship under (A′) (m5); nits n1–n6.
- **Revision 8, owner decision** (2026-11-25). Status: **Accepted**. Q7 is decided as (A)
  reached through (A′): J1 may ship with the reopen after each save, and C6 then switches the
  save to the kept shadow under oracles 12 and 13. Q7's side question is decided yes: the
  compiled Commit (Appendix A, in the spike) stays a test-only second reference for oracle 1
  until J2, then is dropped unless it found a bug the sequential reference did not. Sections
  headed "(proposed, revision 8)" lose that mark; no rule changes.

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
- the core APIs Commit needs (§7.4, C1–C8);
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
        │                             dimension → canonical cells → serialize once → swap
        └───────────────────────────────────────────────────── output bytes = new Base
                                                     (shadow kept and rebased on it, no reopen)
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
| **Snapshot** | A cell's node as it was before the first applied call that targets it while it has no open snapshot; every structural op closes every open snapshot (§4.4, R2). |
| **Changed set** | The cells (by id) that carry a mark from an earlier epoch of this session or whose state differs from their snapshot; what R3, `dimension` and locality are computed from (§4.4). Its **content-changed** part (content differs, not only `s`) is what R3's strip set starts from (with style-only changes when style-sensitive). |
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

#### 3.2 Shadow (revision 8)

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
    state. The spike found six: `set_style_index(0)` writes `s="0"` where the direct write has
    no `s`; `set_formula` drops the Base cache, so a formula written away and back loses it; a
    node that lost its children serializes as `<c …></c>` where a parsed one stays `<c …/>`; a
    strip made by an earlier save must survive a later style-only write; a garbage session
    `xf` can be revived by dedupe and renumber later items; `fullCalcOnLoad` is not monotone
    under `KeepStaleCaches` (Appendix B).

  R2 removes these traces only for a cell that is **back** in its snapshot state. The others
  need more than R2 (review round 7, M5): §7.4's table "F11 traces" names the core API that
  removes each one (C2, C3, C4, C6, C7). In the spike a text layer did all of it at save,
  including canonicalizing cells that were not reverted, which C1–C5 alone would not have
  done. That the result is history-independent is an oracle (11), not an argument.
- It is a cache of Base + log. It is rebuilt by replaying the log on `from_bytes(Base)`:
  after a truncation that `truncate_fast` does not cover (§10.3), after a failed Commit that
  changed it (§9.3), after a call refused part-way (§4.5), and when an op poisons it (ADR-0003
  §3.1: an internal failure). Replay runs the ops through the same rules (validation, R1, R2),
  so a replayed shadow makes the same decisions; it re-applies only ops that were accepted on
  the same prefix, so a refusal during replay is `JournalInternal`. Structural ops record the
  `New` ids they allocated, and replay reuses them. Replay equality is Test oracle 12.
- Memory: the shadow is today's sequential `Workbook`; the Journal adds the Base bytes, the
  log, the vectors and one snapshot per touched cell (about 300 bytes each in the spike). It is
  not dropped at Commit; C6 rebases it on the output (§7.2 step 7, §7.3).

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
    calc: Option<StructuralCalc>, // structural ops only (C3, §7.1 R3, R4)
}
struct StructuralCalc {
    strips: Vec<CellId>,  // the cells the op's own ADR strips sequentially: ADR-0018 §3.7 for
                          // row/col ops, ADR-0009 §12's row for each sheet op (add, delete,
                          // rename, move); computed before the edit, recorded by id
    removes_calc_chain: bool, // the op meets its ADR's condition for removing calcChain.xml
                          // (ADR-0018 §3.7: a formula cell moved or deleted; ADR-0009 §5/§12:
                          // delete_sheet)
    needs_full_calc: bool, // the op meets its ADR's condition for fullCalcOnLoad (ADR-0018
                          // §3.7: any formula among the op's dependents, cached or not; not
                          // in manual mode). `strips` alone cannot carry it: a formula without
                          // `<v>` is stripped of nothing (review round 10, m3)
    sst_cells_removed: u64, // `t="s"` cells the op deleted (R4's `sst/@count`)
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

`calc` is what a structural op returns under `CalcPolicy::Deferred` (C3; review round 7, M3,
m2; review round 8, m1 for sheet ops): at save the pre-edit census, names and tables are gone,
so the op's own strip decision (ADR-0018 §3.7, or ADR-0009 §12 for `add_sheet`'s 3-D and
`SHEET()`/`SHEETS()` seeds, `move_sheet`'s 3-D membership change, `rename_sheet`'s newly
resolving names, `delete_sheet`'s dependents) must be kept from when it ran. Replay re-runs the op on the same prefix and records the same value
(oracle 12); truncation drops it with the op. The spike verified the record for row and
column ops (Appendix B.2: it runs the op on a copy and records the cells whose cache it removed,
whether it moved or deleted a formula cell, and the `t="s"` cells it deleted;
`mutant_drop_records` fails 10 tests). The record of a real `Deferred` op is not compared
white-box, and sheet ops are not run: those parts stay **to be verified in J2**.

#### 4.3 Log and key index (revision 8)

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

#### 4.4 Snapshots and the changed set (revision 8)

- **Targets.** A call's **targets** are the cells its plan targets (passes `plan_target`), the
  "written cells" of review round 6, m2: the op's own cell; every cell of a `write_cells`; the
  cells a barrier op writes through the same plan (a hyperlink with `text`, the hidden cells
  `merge_clearing` clears); and the cell a shared-group **hand-off** turns into the new master
  (ADR-0003 §4.2, `plan_handoff`), although no op names it (review round 7, M2). C1 reports them
  with the plan. A cell a conditional op skips is not a target.
- **Snapshot.** Before the first call that R1 does not skip and whose plan targets a cell
  **that has no open snapshot**, the cell's node is stored as it was (`snapshot_cell`, C2: a clone with its spans and
  dirty state; `None` for a missing cell). In epoch 0 that is the Base's node (after a save, the
  new Base's, by C6); after a structural op it is the node as that op left it. A snapshot lives
  until the next Commit or until the next structural op, which closes it (below); the cell's
  next write then takes a new one (review round 9, M1: "the first call in an epoch" read
  literally would also take a new snapshot without closing the old one, so no carried mark). For a formula cell whose
  cache a save stripped (the node has `f` and no `v`), or that has a pending structural strip
  (§4.2 `calc`), the snapshot's cache is "none" (a restore never brings that cache back).
  The snapshot also holds the cell's **derived state** in the shadow: its census entry (kind,
  text, range, `has_cache`, `results_stripped`, `volatile`, `dynamic`), its dependency edges,
  its `vm` entry and its row-index entry; `restore_cell` puts these back with the node (C2;
  review round 8, M2).
- **Back in its snapshot state** means: the same content and the same `s`. Content is the value
  with its type and the rich runs (C8; a rich and a plain string with the same text differ), or
  the formula text, kind and every `f` attribute (`t`, `ref`, `si`, `ca`, `aca`, …, read by C8).
  A missing `s` equals `s="0"` (review round 7, m1). A formula's cached value is not compared
  (R2 restores it). "Content back, `s` different" is a **content restore** (C2 with the current
  `s`; review round 7, M5). This raw equality is R1's and R2's; R3 compares content as P does
  (below).
- **A missing cell's content is empty.** For content, a `None` snapshot compares as a cell with
  no value and no formula, so a style patch that creates an empty `c` is a **style-only** change,
  not a content change; the created `c` still differs from `None` in presence, so it is in the
  changed set and R2 does not remove it (spike verification: `set_font` on missing `B!A7` read as a content change stripped
  `A!A1 = MATCH(7,B!A:A,0)`, which the sequential side keeps; `adr_missing_is_content`).
- **Changed set.** Each cell has two parts of membership, kept separately (review round 8,
  M3):
  - a **carried mark**, set when a structural op closes the cell's snapshot while the cell is
    not back in its snapshot state (with a **carried content mark** if its content differed).
    A carried mark is sticky: nothing in the session clears it but the cell's death; a save
    clears all marks (§10.1);
  - a **current difference**: the cell's current state is not its snapshot state in the
    current epoch.

  The changed set is the cells with either part. Every target is a cell like any other: it
  takes a snapshot and joins the set if it differs, a hand-off's new master included. A cell
  whose id dies leaves the set (its row is the structural op's allowed diff); only whether its
  formula-ness differed from the Base's is kept, for R3's calcChain test. The
  **content-changed** cells are those with a carried content mark or whose content (not only
  `s`) differs from their snapshot now, **content as P sees it**: value with its type and rich
  runs, formula text and kind with a shared member's text expanded to its own; the `f` attributes
  of a group (`t="shared"`, `ref`, `si`) are ignored. A carried content mark is set by the same
  comparison. They are where R3's strip set starts (their dependents); R3 subtracts nothing
  (review round 9, M2). The raw equality above is not used here (spike verification: seeded
  from raw `f` attributes, a hand-off's new master is content-changed and **its** dependents
  are stripped, which the sequential side does not do: `set_formula(E3, "$A3+E$1")` on the group
  `C2:E3`, then `write_cells([D2: 1, E3: 2])`, strips `Sheet2!A1 = Sheet1!C2*2` on the Journal
  side only, against oracle 1's "Journal strip set ⊆ sequential"; `adr_r3_seed_raw`). A hand-off's
  new master is therefore content-changed only if its expanded text changed; it is still in the
  changed set (its `f` attributes changed) and still stripped as a dependent of a changed input
  (R3). A restore (R2) ends only the current difference; a
  carried mark stays, so a cell written in epoch 0, closed by a structural op and written back
  to its epoch-1 snapshot stays in the changed set and keeps growing `dimension` (the epoch-1
  snapshot is not the Base's state).
- **Epochs close snapshots.** **Every** structural op (`insert_rows`, `delete_cols`, …, a
  splice, and every sheet op: `add_sheet`, `delete_sheet`, `rename_sheet`, `move_sheet`) closes
  **every open snapshot in the workbook**, on every sheet, whatever the cell holds now. A closed
  cell that is not back in its snapshot state gets its carried mark (and carried content mark if
  its content differs); a cell that is back was already restored by R2 and gets none. Its next
  write takes a new snapshot. So R2 restores only within an epoch (review round 9, M1: revision 8
  closed only the snapshots of the op's own sheet and of the formulas whose text it rewrote. A
  cell on another sheet, `A!X = MATCH(7,B!A:A,0)` with a cache, written to 9, then
  `insert_rows(B, 3, 1)`, then its text written back, was restored to its Base node with the
  pre-insert cache and derived state, and nothing recorded a strip for it, since it was a
  constant when the op ran; `add_sheet`, `move_sheet`, `rename_sheet` and `delete_sheet` change a
  formula's meaning on another sheet without rewriting its text in the same way). What is lost is
  R2 across epochs on cells the structural op did not touch; such a write-back is compared up
  to P, which oracle 11 already does. A write-back that
  spans a structural op is compared up to P, not in bytes (Test oracle 11's scope). The node an
  op leaves still holds a cache §3.7 strips; that strip is recorded by the op (§4.2 `calc`) and
  applied at the save whatever later happens to the cell's style (R3). The spike verified this
  rule for row and column ops (Appendix B.2: the three round 9 M1 cases, `mutant_close_own_sheet`,
  `mutant_restore_clears_mark`; oracle 11 across epochs: byte-identical within an epoch, P-equal
  across one, 259 of 385 such blocks byte-identical, the others differing by the carried mark).
  Sheet ops (`add_sheet`, `move_sheet`, `rename_sheet`, `delete_sheet`) are not run and stay
  **to be verified in J2**.
- **Session items.** The `sst` items, `cellXfs` entries, fonts, fills, borders and custom
  `numFmt`s the shadow added since the Base. R4 keeps those a cell references at save and
  drops the others. `dxfs` are not session items: a `dxf` a barrier op added stays (append-only,
  ADR-0006 §5.8 reuse-or-append), and its index is the shadow's.
- **Workbook-scoped side effects of content ops**, each matched to a rule: string writes
  append `sst` items (R4); two-half calls and patches append `xf`s and their font, fill, border
  and `numFmt` entries (R4); `set_dynamic_formula` creates or extends the metadata part (kept;
  oracle 1's metadata exclusion); formula and value writes strip caches, set
  `fullCalcOnLoad` and remove `calcChain.xml` (deferred to the save by R3); writes outside
  `dimension` grow it on `main`, but not on the Journal's shadow (C5: decided at the save from
  the changed set, §7.2 step 4).

#### 4.5 Validation (revision 8)

- **At append, on the shadow.** An op is validated by the sequential API's own checks on the
  shadow, plus the adapter's own argument checks (ADR-0021 "scalar validation"). `Err` → that
  error is returned and the **Journal and shadow are unchanged** (ADR-0001 `Err` ⇒
  byte-identical for the shadow; no `seq`, no id allocated; Test oracle 3). `Ok` → R1 decides
  whether the op is applied (§7.1); either way it is appended. Refusal equivalence with the
  sequential API (Test oracle 1 (i)) holds by construction for part A **because R1 skips only
  ops that leave unchanged every state a later validation reads** (census kind and group,
  dependency edges, merges, tables, arrays, `cm`/`vm`; §7.1 R1). An op that would change any of
  it is applied, as sequentially, so the shadow and the sequential workbook validate later ops
  on the same state (review round 8, M1: revision 8 first let R1 skip a shared member's own text
  written back, which left the group whole on the shadow while the sequential call split it, and
  a later `write_cells` of the master and that member was refused only on the Journal side).
- **Validation comes before the no-op judgment** (F13). A skipped no-op must still return the
  refusal the sequential call gives, for example the same value written to a non-anchor cell
  of a merge (`MergedCell`). With the plan-only API (§7.4 C1), the order is: plan (validates
  and resolves the style request) → `Err` is the refusal → `Ok` and a no-op is skipped →
  otherwise apply the plan. Until C1 lands, R1 is **off**: every accepted op is applied, and R2
  removes what a no-op left, except on a shared or array cell, where the write splits the group
  and R2 cannot restore it (§7.1, review round 7, M2); R1 never skips such a write either, so
  both sides split the group in every case. The spike verification confirms it (Appendix B.2):
  the spike's earlier R1, judged before validation (`r1_unvalidated`), accepts `set_number(B6, 5)`
  on a merge's non-anchor cell holding 5 and `set_number(G3, 4)` on an array result cell holding
  4, which the sequential API refuses (`MergedCell`, `ArrayResultCell`). With R1 off, refusals,
  GetPut and the history oracles hold (R2 removes the write-backs). The cost of R1 off was
  measured on a second session made mostly of write-backs: 25 µs per op appended against 8 µs
  with R1 (§7.3).
- **Structural ops** are validated and applied by their one ADR-0009/0018 operation on the
  shadow, whose state already contains every earlier op. If an earlier write makes the
  operation refuse (a write into a cell that would become a non-empty merge anchor, a
  formula whose reference would be deleted, `ReferenceDeleted`), the op is refused. No dry run
  and no replay is needed.
- **Cost.** A keyed op costs its sequential call on the shadow, plus the no-op test, the
  snapshot at its cell's first write and the revert check: 2–3 µs per op (5%) at 110k cells,
  below the noise at 1.1M (spike `ad74d32`, with R1 on). While R1 is off (until C1), a
  write-back-heavy session pays 25 µs per op instead of 8 (§7.3). No reopen, no serialize. A structural op costs one
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
  would be the clone ADR-0018 §2.1 rejected. A call of keyed ops only, within `truncate_fast`'s
  fast-path conditions (§10.3), may be undone by its cell restore instead, with the same result
  (Test oracle 14).
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
  - a formula cell (or array result cell) in the **pending strip set** (R3 for the current state) reads with no cached
    value. A read asks this **per cell** (`would_strip(cell)`, C3), defined as "the cell is in
    R3's strip set (§7.1) for the current state", every condition taken from R3 and nowhere
    else (seeds compared as P compares content, style-only dependents when `style_sensitive()`,
    holding `f` and `<v>` or a result cell of an array (or spill) anchor in the strip set
    holding `<v>`, nothing
    subtracted; review round 8, m2; review round 9, M2; Appendix B.2), not
    by computing the whole set: the spike computed the whole set after
    each append (2.0 ms at 110k cells and 5.5 ms at 1.1M for ~1,500 content changes), which an
    ExcelJS-style setter/getter alternation would pay per getter (about 8 s for 1,500 pairs at
    1.1M cells; review round 7, m7). The per-cell cost and the alternating load are measured in
    J1, with C3 (Test oracle 10);
  - a formula cell restored by R2 reads with its original cache (`restore_cell` puts it back),
    unless it is in the pending strip set.
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
  R1 skips, or that R2 restores on a cell without a carried mark (§4.4), grows nothing. It need
  not cover a Base cell outside a stale Base
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
| snapshots, changed set | at append (R2), for every target of a call (§4.4); snapshots dropped by a Commit, a closing structural op or a replay that rebuilds them; carried marks set by a closing structural op and dropped only by the cell's death, a Commit or a replay that rebuilds them |
| pending strip set | asked per cell from the changed set and the recorded structural strips (C3 `would_strip`); the whole set only at the save (R3) |
| census, merges, arrays, tables, `!ref`, `sst` reverse lookup | the shadow's own (today's sequential machinery) |

### 7. Commit = serialization of the shadow (revision 8; Q7)

Commit writes the shadow out once. It does not open the Base again, does not compile the log,
and does not reopen its output. What makes the output equal to what a careful compilation would
write is four rules on the shadow, R1–R4, plus the canonical cell form of C7; since the owner confirmed Q7 (2026-11-25) each is a
**normative** part of this ADR. Revision 7's compiled Commit is Appendix A.

#### 7.1 The four rules (revision 8; normative)

| Rule | When | Statement |
|---|---|---|
| **R1** no-op at append | append, **after** validation (§4.5, F13) | An accepted op is logged (origin and echo kept, `applied = false`) but not applied when (a) every cell it targets **exists** (`c` is present), (b) **no target is a shared-group cell (master or member) or an array cell**, and (c) C1's plan for it leaves every target **in its current state under §4.4's state equality** (content with its type, rich runs, formula text, kind and every `f` attribute; `s`, a missing `s` equal to `s="0"`), leaves every other part of each target's node as it is (h1, `cm`/`vm`; review round 8, n1), and changes nothing else a later validation reads (census kind and group, dependency edges, merges, tables, arrays, `cm`/`vm`; C1 reports it). A two-half op (`set_datetime`, `*_with_format`) is a no-op only if both halves are, and a call with several ops (a `write_cells` of several cells, a splice's writes) is skipped only as a **whole**, when every op of it is a no-op; otherwise every op of it is applied, so the shadow never runs a part of a call the sequential side ran whole (review round 9, m5). So a plain string written over a rich string with the same text is applied (the plan drops the runs), and a shared member's own text written back is applied, splitting the group as the sequential call does (review round 8, M1: revision 8 judged R1 by P, which skipped that write and left the shadow's group shape, the input of later validation, different from the sequential one). Condition (b) is deliberately coarse: whether a write on a group cell leaves the group's shape unchanged is not worth deciding, since R1 only saves cost. An op on a **missing** cell is never a no-op, whatever the row/col default: the sequential call creates the cell (with `s`, or without it for a content op), and that `c` changes how the cell is formatted (review round 5, M1: `set_style_index(0)` on a missing cell under a bold `col/@style`). A conditional op on a missing cell is applied and does nothing there, as sequentially. |
| **R2** restore | append, after each applied call, for each of its targets | If a target is back in its snapshot state (§4.4), it is **restored**: `restore_cell` (§7.4 C2) puts the snapshot node back (the parsed `c` with its spans, its cache and its own dirty state as snapshotted; for a `None` snapshot it removes the `c`) **together with the cell's derived state** (census entry, dependency edges, `vm` entry, row index; review round 8, M2), clears an ancestor's dirty flags only when no other descendant is dirty, and the cell's **current difference** ends. A carried mark from an earlier epoch (§4.4) is **not** cleared: such a cell stays in the changed set, content-changed if its carried mark says so (review round 8, M3). If only its **content** is back (`s` differs), `restore_cell` puts the snapshot node and derived state back with the current `s` (a content restore): the cache comes back, and the cell stays in the changed set as a style-only change, so R3 still strips it when an input changed (review round 7, M5). Because C6 rebases the shadow on every output, a snapshot's spans point into the current Base, so a restored cell serializes as its bytes there and Layer 1 can raw-copy a part whose every write was reverted, in the first session and in every later one (review round 7, M1); for the registry parts this relies on R4's return to the Base node (review round 9, m1). |
| **R3** calc state at the save | Commit (§7.2 step 2) | The shadow runs `CalcPolicy::Deferred` (C3): no cache strip, no `fullCalcOnLoad`, no calcChain change at write, by keyed, barrier **or structural** ops. At the save, `finish_calc` applies Q1 (A) **once**, as if the session were one edit. **Seeds**: the **content-changed** cells (§4.4: content compared as P compares it, a shared member's text expanded, group attributes ignored; a carried content mark counts; a missing cell's content is empty, so a cell a style patch created is style-only); plus the style-only changed cells when the workbook is style-sensitive (`style_sensitive()`, made public by C3). The **strip set** is the ADR-0003 dependents (transitively) of the seeds, plus every live id of the session's structural strip records (§4.2 `calc`: ADR-0018 §3.7 or ADR-0009 §12 per op). **Strip test**: every cell of the strip set that holds both `f` and `<v>` at the save, **and every result cell of an array (or spill) anchor in the strip set that holds `<v>`** (`Workbook::dependents` lists them), is stripped; each loses `v`, `t` and an `is` (ADR-0003 §3.3 item 1). **Nothing is subtracted.** This is the sequential `plan_strip`'s own rule (strip every dependent that has a cache; `edit.rs` L740-747): the cells it skips, those the same plan writes, get new content there, a formula without a cache (ADR-0003 §3.1a) or a constant without `f`, and on the Journal side a cell whose last write was such content holds no `f`+`<v>` either, so it is left alone by the same test. A style-only changed cell, a content-restored cell, an R2-restored cell and a hand-off's new master are stripped like any other dependent (review round 7, B1: revision 8 first subtracted the whole changed set, so a formula cell restyled while its input changed kept a stale cache; review round 9, M2: after `set_number(D2, 1)` handed a group off to `C2`, `set_number(A2, 99)` must strip `C2`, which keeps its Base `<v>`, `edit.rs` L809-822 (iv)). With the seeds compared as P compares them, round 7's "subtract the content-changed cells" gives the same output as subtracting nothing: a content-changed cell's last write left it without `f`+`<v>`, and a hand-off's new master is no longer content-changed (the spike's `mutant_r3_subtract_content` is an equivalent mutant; it fails only together with raw seeding). The ADR keeps the simpler statement, nothing subtracted. Spike verification (Appendix B.2) found two wordings of revision 8 that failed and are fixed above: the strip test without array result cells (`set_number(A2, 99)` under `G2:G3 = {A2:A3*2}` stripped `G2` and left `G3`'s `<v>4</v>`; `adr_r3_formula_only`), and seeds compared by §4.4's raw equality (§4.4; `adr_r3_seed_raw`). `fullCalcOnLoad` is set when the strip set has a cell, the session's cell writes changed the set of formula cells (a content-changed cell, dead ids included, gained or lost a formula), **or** a session structural op met its ADR's `fullCalcOnLoad` condition (`calc.needs_full_calc`: ADR-0018 §3.7 sets the flag when any formula is among the op's dependents, **cached or not**, `sheet_ops/grid.rs`), and in no other case (the Base's flag is kept) (ADR-0003 §3.3 item 2, manual mode excepted). The structural clause is needed because a formula with no `<v>` (as openpyxl writes one) is stripped of nothing, so it is in no strip record, yet the sequential op sets the flag for it (review round 10, m3). The spike does not record `needs_full_calc`; it sets the flag whenever the session removes calcChain instead (it counts `chain_removed` as a formula-set change). That is not the same condition: it misses an uncached formula that only shifts with no formula cell moved, and it is stronger than `main` for an op that deletes formula cells no surviving formula reads (`main` removes calcChain there without setting the flag). The spike's oracle 1 checked the flag in one direction only, so neither difference showed; this clause is first run in J2 (Appendix B.2). `calcChain.xml` is removed when the **formula-cell set, counted by id with dead ids included, differs from the Base's** (a cell whose content change gained or lost a formula counts even after a structural op killed its id; ADR-0003 §4.3) **or** a session structural op met its ADR's calcChain condition (`calc.removes_calc_chain`: ADR-0018 §3.7 for a moved or deleted formula cell, ADR-0009 for `delete_sheet`; review round 7, M3; review round 8, m1). A dead id counts with the formula-ness its last write gave it; a Base formula cell deleted by a structural op is covered by `removes_calc_chain` (so oracle 1's "a Base formula cell whose id died is a difference" and this count give the same result; review round 10, n2). (Spike verification: `A5` was `=1+1`; `set_number(A5, 5)`, `delete_rows(4, 1)`: the op deletes no formula, since `A5` is a constant by then, and `A5`'s change died with its id, so revision 8's "a content-changed cell gained or lost a formula" kept the Base's calcChain, which lists `A5`; `adr_dead_mark_drops_fx`.) The structural part is conservative (it strips every recorded cell that still holds a formula, which is what the sequential path did at the op); the spike verified it for row and column ops, and sheet ops stay **to be verified in J2**. A strip made by a save stays: it is in the tree (C3 edits the shadow, C6 rebases it), so the stripped node (`f` without `v`) is the new Base's and a later style-only write to that cell does not bring the cache back. |
| **R4** registries at the save | Commit (§7.2 step 3) | `compact_registries(base_counts)` (C4) keeps the session items (§4.4) that something in the shadow references and drops the others. In part A only a `c/@s` or a `c/@v` of a `t="s"` cell can reference a session item: no part A op sets `row/@s` or `col/@style`, and Base rows and columns reference Base items only (review round 8, n3); C4 still scans `row/@s` and `col/@style` so a later op that sets them needs no change. Survivors are appended after the Base's items in **canonical order**: cells in (sheet order, row, column) order, each item at its first reference, and a `cellXfs` entry's font, fill, border and `numFmt` at their first referencing `xf`. Base items keep their numbers. The shadow's `s` and `v` attributes are rewritten to the new numbers, and its reverse lookups (`sst` text → index, `xf` content → index) are rebuilt (C4), so the shadow keeps the output's numbering after the save. A registry part (`sharedStrings.xml`, `styles.xml`) whose session items were all dropped and whose counts are back to the Base's is put back to its Base node, clean (C4; review round 9, m1: `main`'s string write appends an `si` and sets `sst/@count` on every write, `registry/strings.rs` L240-246, so without this a write-and-revert Journal re-renders the part); a part that keeps a session item is in the allowed diff (§9.1). `sst/@uniqueCount` is the item count; `sst/@count` is the Base's count plus the change in the number of `t="s"` cells (the Base's count is its `@count`; if the Base's `sst` has no `@count`, the output has none either; if the Base has no `sst` part, the Base's count is 0; review round 8, n2): the shadow keeps that change as a running delta (each applied write and restore adds its cell's change; a structural op subtracts the `t="s"` cells it deletes, `calc.sst_cells_removed`), so cells removed by `delete_rows`, `delete_cols` or `delete_sheet` are counted (review round 7, m2; verified by the spike for row deletes, `mutant_no_sst_removed`; `delete_sheet` is to be verified in J2). |

Consequences of the rules, each an oracle:

- **History independence** (Test oracle 11). For keyed ops within an epoch, on cells R2 can
  restore (not shared or array members, nor cells with `f` attributes `set_formula` does not
  write), the output depends only on each cell's final state, not on the writes that led to
  it: a write-back is free, an overwritten value leaves nothing, a style round trip leaves
  nothing (C2's content restore, C7), and registry numbering is canonical. Across a structural
  op the output is equal in P, not in bytes: a cell written before the op and written back after
  it keeps its carried mark (`dimension`, and a strip the epoch-0 history did not need). The
  spike measured both (Appendix B.2).
- R1 is invisible in the output and in later refusals once R2 exists (a write of the same value
  is a revert of length one): it skips only what R2 would restore, and never a write on a
  shared or array cell, which splits the group on both sides (R2 cannot restore that, because
  the `f` attributes differ; review round 7, M2; review round 8, M1). What R1 buys is cost: no
  snapshot and no dirty part for a pure write-back. So R1 is an optimization, not a rule the
  output depends on: R1 off (the state until C1) gives the same output. Its "removed" mutant is
  caught only by a white-box test (oracle 5); its "skips on a group cell" mutant (`mutant_r1_skip_group`, judged on P) by oracle 1's
  fixed case of a shared member written back and then written with its master (§4.5).
- **Canonical cell form** (C7, at the save): every changed cell whose node is not the Base's
  (that is, not restored to its epoch-0 snapshot; a cell with a carried mark that R2 restored to
  a later epoch's snapshot is included, review round 9, m3) is written in one form whatever its
  history: an `s` that resolves to `xf` 0 is absent if the cell's Base node has no `s` or the
  cell has no Base node, and `s="0"` otherwise, whatever `s` the Base node had (review round 9,
  n1); a childless element is self-closing. Without it `set_number(9)`, `set_font(bold)`, `set_style_index(0)`
  on a cell without `s` writes `s="0"`, while `set_number(9)` alone writes none (review round 7,
  M5). The spike's text layer did this; C1–C5 did not.
- A missing rule is never silent: each rule has a mutant that a test catches (oracle 9; in
  spike `ad74d32` all four "rule removed" mutants are caught, R2–R4 by oracles 5, 11 and 12, R1
  by oracle 5's white-box test). A rule stated **wrongly** is a different matter: the spike's
  R3 subtracted the whole changed set (B1) and no test caught it, because the spike's oracle 1
  cache predicate was only its first half. Oracle 9 therefore also lists mutants of the
  rules' details (the B1, M1, M2, M4 and M5 mutants of review round 7). The spike verification
  of revision 8 (Appendix B.2) ran a mutant for each rule and each detail fixed in rounds 7–9;
  every one is caught except two equivalent ones (round 7's subtraction under P-compared seeds;
  R1 (b) in a spike whose R1 never judges a group cell), and it found four wordings that failed
  or were ambiguous, each now kept as a mutant (`adr_*`, oracle 9).

#### 7.2 Save steps

1. **Empty log** since the commit mark: return the Base bytes (GetPut).
2. **Mark the shadow stale** (before anything changes it, so a panic in a later step leaves the
   mark; review round 7, m4), then **R3**: `finish_calc` on the shadow (C3).
3. **R4**: `compact_registries(base_counts)` on the shadow (C4).
4. **`dimension`** (C5): the shadow's writes never touch `dimension`; only structural ops
   rewrite it (ADR-0018 §3.2). Here each sheet's `dimension/@ref` (the Base's, as structural ops
   rewrote it) is grown to cover every cell of the changed set on that sheet, carried marks
   included (§4.4). A cell R1 skipped, a cell R2 restored that has no carried mark, or a cell a
   conditional op left alone (`remove_formula` on a cell without a formula, P1 on a missing
   cell) grows nothing. On a sheet with no changed cell and no
   structural op, the `dimension` element keeps its Base bytes. A sheet without `dimension`
   stays so.
5. **Canonical cells** (C7): each changed cell is put in the canonical form of §7.1.
6. **Serialize the shadow once** (`to_bytes`, recording the new span of every node of each
   re-rendered part for C6): Layer 1
   raw-copies every part with no dirty node; Layer 2 re-renders only dirty nodes (ADR-0001), so
   each changed part is "original byte spans + replaced pieces", as a rope [1-8].
7. **Swap**: `rebase_after_save` (C6) makes the output the shadow's parse source and the
   package's original, and clears every dirty flag; then the stale mark is cleared, the output
   is returned and made the new Base, and §10.1 follows. Only when every step succeeded is the
   output returned. On a failure (or panic) in steps 2–7 the stale mark stays and the shadow is
   rebuilt by replay from the old Base (§9.3).

Commit **reuses** the sequential API, its plans and their tests; there is no second editing
engine and no second run of a structural op. What it adds is the four rules, the canonical
cell form, the one-shot serialization and the rebase.

#### 7.3 Cost and memory (spike `ad74d32`; revision 8 rules, `6339047`)

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

- At `ad74d32` (historical; the revision 8 table below supersedes it) the save met ADR-0018
  §3.8's 2× target at both sizes. The term over `to_bytes` is the
  spike's text layer (inflating the Base part, two linear scans; Appendix B), which stands in
  for C2–C8. With them in core the shadow is edited before serializing, and that term goes; the
  260 MiB peak is the layer's part-sized buffers.
- At most **one parsed workbook** is alive: the shadow, before, during and after the save. No
  working copy, no reopen. No worksheet is cloned (ADR-0018 §2.1 rejected a staged clone, 289 ms
  for a 200k-formula sheet with the tree's memory doubled).
- Measured again in J1, with the Commit, against core APIs, not the text layer (Test oracle 10;
  review round 9, m4). C6 (rebase) and
  C7 (canonical cells) add work the spike did not do at all; they are included in the save J1
  measures (§7.2 steps 2–7), not in the figures above (review round 8, m4). C6 is linear in
  the nodes of each re-rendered part (§7.4), not only in the re-rendered nodes.
- **What these numbers do not show** (review round 7). They are the cost of the spike's rules
  at `ad74d32`, and those rules were not all correct: its R3 subtracted the whole changed set
  (B1), and the 20,000 oracle 1 cases that passed checked only the first half of the cache
  predicate. It also ran keyed ops only, one epoch, no shared formulas or arrays, and R1 before
  validation (Appendix B.1). The table below is the remeasurement with revision 8's rules.

**Revision 8 rules** (spike `6339047`: R1 off, R3 as corrected above, epochs and structural ops
supported in the text layer; 10,000 rows, same fixture, ops and machine; **one run** per row;
Appendix B.2):

| | revision 8 rules | `ad74d32` (above) |
|---|---|---|
| append per op (2,000 ops) | 49.8 µs | 49.5–51.5 µs |
| save, kept shadow (Q7 (A)) | 135 ms = **2.01×** a sequential `to_bytes` (calc 2.3, registries 2.3, sheets 60, zip 67 ms) | 116–122 ms (1.68–1.80×) |
| save, reopen after each save (Q7 (A′)) | 155 ms + reopen 158 ms = 313 ms = **4.64×** (save and reopen; the dependency-graph rebuild on the reopened shadow is paid by the next session's appends, below) | — |
| second session (the same ops reversed, mostly write-backs), (A) | append 25 µs/op (R1 off: each write-back is applied, then restored by R2), save 129 ms (1.91×) | 8 µs/op with R1, 107–112 ms |
| second session, (A′) | append 81 µs/op (includes the dependency-graph build on the reopened shadow: about 112 ms over 2,000 ops more than (A)'s 25 µs/op, about 1.7× a `to_bytes`), save 142 ms (2.11×; save only, reopen not included) | — |
| `truncate` to 1,000 of 2,000: `truncate_fast` vs replay | 43 vs 307 ms (7×) | 19–22 vs 305 ms |
| `insert_rows(0, 1)` through the Journal | 799 ms per op; the same op on a bare `Workbook`: 149 ms | — |

- **The 2× target is reached only with the kept shadow, and only within noise** (2.01×, one
  run): the spike does not show it is met. The rise from 1.7× to 2.0× is the epoch support in
  the text layer (Base-node lookups by id for C7, R3's per-candidate cache test), a term that
  goes when C2–C8 are in core; J1 must show the target on C2–C8 (oracle 10). (A′) reopens the
  output (one `from_bytes`, about 2.3× a `to_bytes`) and costs 4.64× for save and reopen,
  measured (Q7 used an estimate of about 4×), plus the dependency-graph rebuild the next
  session pays on the reopened shadow (81 vs 25 µs/op, about 1.7× a `to_bytes` at 2,000 ops).
  So the target depends on C6 (or its reparse fallback limited to the re-rendered parts);
  without C6, (A′) meets correctness, not the target.
- **R1 off costs append time, not output.** A write-back-heavy session pays 25 instead of
  8 µs per op until C1 lets R1 back on (§4.5).
- **The structural figure bounds the spike, not the design.** The 799 ms are the stand-ins
  (serialize the shadow, splice the restored cells back, reopen it, run the op on a copy for
  its record): 5.4× the bare op. In core C2 restores in the tree and C3 returns the record
  from the op itself, so the design's cost is the bare op (§4.5).
- Not measured: 1.1M cells with revision 8's rules, a real C6 span walk, C1–C8 in core
  (all J1).

#### 7.4 Core APIs required

The spike uses `ooxml-xlsx` through its public API only and stands in for each of these with a
substitute (Appendix B). C1–C5, C7 and C8 are Delivery prerequisites of J1's Commit (C2 of J0,
C3's structural part of J2). C6 is a prerequisite of Q7 (A), not of J1: J1 may ship under (A′),
reopening the shadow after each save, before C6 lands (review round 10, m5).
C6–C8 and the extensions of C2–C4 were added after review round 7 (M1, M3, M5, m6).

| Id | Needed in `ooxml-xlsx` / `ooxml-core` | Rule | The spike's substitute | Can the substitute stand in? |
|---|---|---|---|---|
| **C1** | **plan without apply**: validate an op and resolve its style request to an `xf` without changing the workbook (F2), so that the no-op test runs after validation; the plan reports its **targets**, a hand-off's new master included (§4.4), whether a target is a shared-group or array cell, and whether it changes any target's §4.4 state or any state a later validation reads (census kind and group, dependency edges, merges, tables, arrays, `cm`/`vm`) (R1; review round 8, M1) | R1, §4.4, §4.5 | content: R1's content test on a read (plain values and plain formulas only); style: `set_style_index` only, other patches applied and restored by R2; judged **before** validation; in the verification (B.2), R1 off and `handoff_target` (before a content write on a shared master, the first member in (row, column) order, as `plan_handoff` picks it, is snapshotted and tracked) | No for R1: a skipped no-op hides a refusal (F13, shown by the spike: `MergedCell`, `ArrayResultCell`). Until C1, R1 is off (§4.5). Targets: yes for shared groups (`mutant_no_handoff_target` is caught), not for barrier ops. |
| **C2** | **`snapshot_cell(sheet, at)`**: a clone of the `c` node with its spans, cache and dirty state, or `None` for a missing cell. The snapshot also holds the cell's **derived state**: its census `FCell` (kind, text, range, `has_cache`, `results_stripped`, `volatile`, `dynamic`), its dependency edges, its `vm` entry and its row-index entry. **`restore_cell(sheet, at, snapshot, style)`**: put that node back, with `style` = the snapshot's `s` (full restore) or the current `s` (content restore, §4.4), **and put the derived state back** (replace the census entry and dependency edges, the `vm` entry and the row index; drop the `style_sensitive` cache; review round 8, M2: without it a restored formula keeps `has_cache: false` and R3's strip skips its restored cache, or a constant restored to a formula is missing from the census and later refusals differ); restore its own dirty state and clear an ancestor's `own_dirty`/`subtree_dirty` only when no other descendant is dirty (`XmlDocument` needs a count of dirty descendants). With `None` it removes the `c`, and a `row` element the session created for it if that row then has no `c` and no attribute the session set; otherwise the row stays (and is in the allowed diff) | R2, §10.3 | text layer at save: the Base's bytes for restored cells, the Base's bytes with `s` set for content restores (`Act::BaseStyle`), View cache override, carried pins; at an epoch boundary (verification, B.2) `materialize`: the shadow is serialized, restored cells are spliced back as their snapshots' bytes, `dimension` is set back to the epoch's, and the shadow is reopened from that | Output: yes, by the tests (keyed ops, shared and array formulas, row and column ops across epochs). Shadow: no; its nodes stay history-dependent (F11), so the layer carries pins across saves. The layer never touched the shadow's census, so the derived-state restore is untested. |
| **C3** | **`CalcPolicy::Deferred`** + **`finish_calc()`**: no strip, flag or calcChain step at write, structural ops included; one plan at the save (§7.1 R3); a structural op under `Deferred` **returns** its ADR-0018 §3.7 strip set by id, whether it moved or deleted a formula cell, whether it met its ADR's `fullCalcOnLoad` condition (`needs_full_calc`: any formula among its dependents, cached or not), and the `t="s"` cells it deleted (§4.2 `StructuralCalc`); a per-cell **`would_strip(cell)`** and a dry-run `pending_strips()` that do not change the shadow (§6.1); a public **`style_sensitive()`** (today `pub(crate)`, `edit.rs`) | R3, R4, §6.1 | `KeepStaleCaches` + its own decision through `Workbook::dependents` + a text strip; `fullCalcOnLoad` added to the Base's `calcPr` text; calcChain by `Package::remove_part`; the View masks pending strips (whole set after each append) | At `ad74d32`, no for the strip set: the spike subtracted the whole changed set (B1). In the verification (B.2) the spike runs R3 as §7.1 states it; the structural record is taken by running the op on a copy of the materialized shadow under `StripDependentCaches` (only cells holding a cache are recorded, as `plan_strip` does), so the record of a real `Deferred` op is not compared; `needs_full_calc` is not recorded (the spike sets `fullCalcOnLoad` whenever calcChain is removed instead, §7.1 R3). Style-only changes strip nothing in the spike (no style-sensitive workbook). |
| **C4** | **`compact_registries(base_counts)`**: drop session `sst`/`xf`/font/fill/border/`numFmt` items no cell references and renumber the survivors canonically, in the shadow, rewriting the `s`/`v` of the cells that use them, and **rebuild the reverse lookups** the shadow uses for dedupe (`sst` text → index, `xf` content → index); a part left with no session item and with `count`/`uniqueCount` (and the other registries' counts) equal to the Base's is replaced by its Base node (after C6, the current Base's) and its dirty flags cleared (review round 9, m1) | R4, §6.1 | text layer at save + carried shadow → Base index maps + translation for `set_style_index` and the View | Output: yes. View: no; an unsaved session `xf` has no stable index (F12). |
| **C5** | **`dimension` at save**: in a Journal's shadow, cell writes do not touch `dimension`; structural ops rewrite it as ADR-0018 §3.2 says; the save grows it by the changed set (§7.2 step 4). One rule, no "or recompute" | §7.2 step 4, §6.1 | the layer rewrites the `dimension` element | Yes |
| **C6** | **`rebase_after_save(output)`**: during the save's serialization record, for **every** node and text of each part Layer 2 re-rendered, the span it occupies in the output, not only for the re-rendered nodes: Layer 2 copies clean nodes and clean start/end tags from `src[span]`, so every clean node after the first change moves (a node inside a raw-copied subtree gets its old span plus that subtree's offset delta). Raw-copied parts keep their spans. Then make the output the parse source of every `XmlDocument` and the original of the `Package` (Layer 1's raw copy), and clear every dirty flag. Afterwards "dirty" means "differs from the current Base" again (review round 7, M1; review round 8, M4). Cost: linear in the nodes of the re-rendered parts (oracle 10). If J1 finds the span walk too intrusive in `ooxml-core`, the fallback, with the same oracles, is to reparse each re-rendered part from the output and rebuild that sheet's census and indexes as at open (snapshots are dropped at the save, so no node identity is needed); the choice is recorded in J1. Until C6 lands, the shadow is reopened from the output after a save (Q7 (A′)) | R2, §7.2 step 7, §10.1 | none needed: the layer opened the **new** Base lazily at every save and carried pins; in the verification (B.2) also (A′) `reopen_after_save` (`EJ_REOPEN=1`), and a session with a structural op always reopens after its save | No for the span walk, which the spike does not have. The rebase's **effect** is tested in both modes: the second-session cases of oracles 12 and 13 pass under (A) and (A′), and `mutant_no_rebase` (restores read the first Base) fails them. |
| **C7** | **canonical cell form at save**, in `ooxml-xlsx` only: for each changed cell whose node is not the Base's (a cell restored to a later epoch's snapshot included; review round 9, m3), an `s` resolving to `xf` 0 is absent if the Base node has no `s` or there is no Base node, and `s="0"` otherwise (review round 9, n1), and a changed `c` with no children is replaced by a fresh empty-element node (own-dirty, rendered `<c …/>`). Layer 2's default rendering is **not** changed, so docx and pptx output is unaffected (§7.1, review round 7, M5; review round 8, m5) | §7.1, oracle 11 | the text layer wrote every written cell in a canonical form (`xf` 0 as the Base wrote it, self-closing) | Output: yes, for the cases tested. Not provided by C1–C5. |
| **C8** | **cell state reads**: a cell's `f` attributes (`t`, `ref`, `si`, `ca`, `aca`, …) as a typed read, and its rich runs, those of a **shared** rich string (`si`) included, which §4.4's state equality and R1's P comparison need and the public API does not expose (F2) | R1, R2, §4.4 | `Sem` (value or formula text, `f` attributes, `t`, rich flag, `s`) read through the cell's raw node; the rich flag only for inline strings | Partly: no runs, and no shared rich strings (the fixture has none). A shared member's expanded text (for R3's seeds and P) comes from a reference shifter for plain A1 references (`project::translate`). |

C2 (the dirty-descendant count) and C6 (parse source, spans, the package's original) change
`ooxml-core`'s Layer 2, which docx and pptx share. Each must pass every format's ADR-0001 tests
and the python-pptx oracle, with byte-identical docx/pptx output for unchanged inputs (review
round 8, m5). C7 stays in `ooxml-xlsx`.

**F11 traces and the API that removes each** (review round 7, M5):

| Trace (Appendix B) | Removed by |
|---|---|
| `s="0"` from `set_style_index(0)` on a cell without `s` | R2 (C2) when the cell is back; C7 otherwise |
| a formula written away and back loses its cache | C2 full restore; C2 content restore when only `s` differs |
| `<c …></c>` for a node that lost its children | R2 (C2) when the cell is back; C7 otherwise |
| a strip made by a save lost by a later style-only write | C3 strips in the tree; C6 makes the stripped node the new Base's |
| a garbage session `xf` revived by dedupe | C4 (drop, renumber, rebuild the reverse lookup) |
| `fullCalcOnLoad` not monotone on the shadow | C3 (`Deferred` sets no flag at write; `finish_calc` decides once) |

With C1–C8 in core the shadow equals the output after a save, the carried state goes away, the
View has stable indices, and Commit is `finish_calc` + `compact_registries` + `dimension` +
canonical cells + `to_bytes()` + `rebase_after_save`. A cheap per-part `serialize_into` is not
needed once these edit the tree; the spike reached 1.6–1.8× without it (2.0× with its epoch
support, §7.3).

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

The cache rule is **Q1 (A)** (decided): ADR-0018 §3.7 + ADR-0003 §3.3. Revision 8 (Q7)
applies it once at the save over the changed set and the structural ops' recorded strips
(R3, §7.1).

### 9. Locality and atomicity

#### 9.1 Locality (ADR-0001)

- The output differs from the Base only inside the **union of the allowed-diff rows** (ADR-0014
  §2's data table, ADR-0018 oracle 1) of: each cell of the changed set (every target that
  differs, a hand-off's new master included, §4.4); each cell of R3's strip
  set, `calcPr` and `calcChain.xml` as R3 decides; the registries' appended items and counts
  (R4); each sheet's `dimension` (§7.2 step 4); the row or column of each row/col property op
  (review round 9, m2); and each structural and barrier op. This is
  ADR-0001's T2 for a Journal, and Test oracle 4 checks it. A cell R1 skipped, or R2 restored
  without a carried mark (§4.4), contributes no row; a restored cell with a carried mark is in
  the changed set and has its row.
- Parts no op reaches are raw-copied (Layer 1); nodes no op reaches, and restored nodes, keep
  their spans (Layer 2). A structural op parses every worksheet (ADR-0018 §2.1 step 1) but
  re-renders only the nodes it changes.

#### 9.2 No-ops and write-backs

R1 and R2 (§7.1) together make a read → modify → write-back of a region change only the cells
the caller changed, and make reverting a value remove its edit (ADR-0021's SheetJS rule): R1
skips the write of a value a cell already holds, and R2 restores a cell whose writes ended where
they started, its cache included, so a value changed and reverted under a dependent formula
strips nothing. Neither grows `dimension` (§7.2 step 4), except that a cell with a carried
mark from an earlier epoch stays in the changed set (§4.4). The View still echoes each op until its
echo ends or compaction (§10.1). An op on a missing cell is never a no-op (R1). Revision 7's
Commit-time no-op test is Appendix A.5.

#### 9.3 `Err` ⇒ byte-identical

| Failure | Effect |
|---|---|
| append refused (§4.5) | Journal and shadow unchanged: same ops, `seq` counter, id counter, snapshots, View reads |
| a splice or flush refused part-way (§4.5) | the whole call undone: log, key index, `seq` counter, vectors, tombstones, echo ends, snapshots, shadow (by replay) as before it; ids it took are discarded; a flush returns `JournalInternal` and breaks the Journal (§11) |
| op poisons the shadow (internal) | the op is not appended; the shadow is rebuilt by replay; `JournalInternal` |
| save fails before step 2 of §7.2 | no output; Base, log, shadow, snapshots and changed set unchanged; `JournalInternal`; a retried `save` gives the bytes the first would have given (F14, tested in the spike) |
| save fails in steps 2–7 of §7.2 (bug, injected fault, panic) | no output; Base and log unchanged; the shadow was marked stale at the start of step 2, before anything changed it, so a panic leaves the mark too (review round 7, m4); it is rebuilt by replay from the Base, which gives back the state before the save (Test oracle 12); `JournalInternal`; a retry starts from scratch |
| the replay of that rebuild (or of any rebuild, §3.2) fails | the failure is deterministic (a bug in a sequential call or a rule), so a retry would fail again: the call returns `JournalInternal` and the Journal is **broken** (`JournalBroken`, §12): only `truncate` and reload are accepted, as after a failed flush (§11) |
| poison inside a save step (ADR-0003 §3.1) | as the second row |

Atomicity is structural: the Base is not touched until the swap (MVCC [2-4], shadowing
[2-7]). This replaces ADR-0021's byte-snapshot staging and ADR-0023's per-transaction
`Workbook::fork()`.

What this mode gives up (F14): no second engine re-derives the output from the Base, so a
**silent** bug in a sequential call, or a history-dependent trace that R2 and R4 do not cover,
reaches the output. A poisoning is still caught (`JournalInternal` + replay). The guards are
Test oracle 1 against the sequential reference, 11 (history independence), 12 (replay from the
Base gives the same bytes as the live shadow) and 13 (bisimulation after a save).

### 10. Compaction, second save, truncation (revision 8)

#### 10.1 After a successful Commit

- The output bytes become the new Base. The shadow is **kept**; it is not reopened from the
  output (revision 7's §10.1 reopen is not done). After steps 2–5 of §7.2 its tree holds what
  the output holds, and after step 7 (`rebase_after_save`, C6) its parse source, spans and the
  package's original are the output's and no node is dirty. Without C6 the spans of a node
  kept from before the save would still point into the old Base, and a second session that
  writes a saved cell and back would restore the **pre-save** bytes with `Ok` (review round 7,
  M1). Test oracle 13 checks that the shadow behaves as a Journal opened from the output would:
  same refusals, same normalized reads, same next output.
- The log is emptied; the commit mark is set to the last `seq`. The snapshots, the changed
  set and its carried marks, the structural `calc` records and the `sst/@count` delta are
  cleared: the new Base is
  the reference of the next session. Strips made by the save are in the new Base (C3, C6).
- Session items are Base items now, in the shadow's numbering, which R4 made the output's.
- **Origins are not folded away.** Each live `Session` op leaves an **echo record**
  `(key → seq, api)`, so the adapter keeps echoing after save, as ExcelJS does for its
  in-memory objects [7-2]. An echo record ends as §6.1 says.
- **Ids survive compaction.** The vectors are kept (run-compressed), so ids that handles hold
  stay valid. Dead ids leave the vectors; the counter keeps them unissued.

#### 10.2 Second save

The second Commit's Base is the first Commit's output. An empty Journal saves it
byte-identically; a non-empty one is local to it. In the spike a second session of the same ops
reversed (mostly no-ops) saved the Base bytes at 1.1M cells; that result rests on the text
layer opening the new Base and carrying pins, so it is **not** evidence for C6 (Appendix B;
review round 8, m4). The verification (Appendix B.2) ran oracle 12's two second-session fixed
cases (round 7 M1, round 8 M4) and their bisimulation under both (A) and (A′): byte-equal to the
first session's output; `mutant_no_rebase` fails all three. This tests what the rebase must
achieve, not C6's span walk, which the spike does not have. A′'s cost is measured (§7.3).

#### 10.3 Truncation

- `Journal::truncate(seq)` drops every op with a larger `seq` from the log, pops the key
  index, and marks the shadow stale (rebuilt by replay on the next read or append). The id
  counter is not rolled back. This is how ADR-0023-style transactions
  (`with wb.transaction():`) roll back, without a snapshot of the parsed parts.
- **`truncate_fast(seq)`** gives the same result without a replay when every cut op is a keyed
  op: for each cell a cut op targeted, `restore_cell` puts back its snapshot (C2; a `None`
  snapshot removes the cell), then the ops that targeted that cell **from the one that took
  its current snapshot** up to `seq` are applied again through §7.1's rules (none when that op
  is itself cut). Ops before the snapshot are already in the snapshot; re-applying them would apply birth-epoch text on top of a node a
  structural op has rewritten (review round 7, M4: `A1 = B5*2`, `insert_rows(3, 1)` kept, a
  write to `A1` cut, gives `B5*2` instead of `B6*2`). It falls back to replay when a cut op is
  structural, barrier or a row/col property op (which targets no cell, so no restore would undo
  it; review round 9, m2), when a cell has **no snapshot record** in the current epoch (a `None`
  snapshot is a record: the cell was missing), or when **any cut op or any op it would
  re-apply belongs to a call with more than one target, or targets a shared-group or array
  cell** (review round 8,
  M5: a kept write to a shared master hands off to `C2`, a cut write to `C2` hands off again;
  restoring `C2` and re-applying the kept op on a master that is already a constant leaves
  `C2` a member of a group without a master, where replay makes it the master). A
  `write_cells` of more than one cell is therefore a replay too; the fast path covers single-cell keyed ops on
  ordinary cells, the common case it was measured on.
  It must give the same reads and bytes as replay (Test oracle 14). In the spike (which
  writes snapshots back with ordinary writes, and falls back for missing, empty, rich, inline
  and shared-formula cells) it was 14–28× faster than replay (§7.3). The verification (Appendix
  B.2) ran it across epochs with re-application from the current epoch and the fallbacks above
  (replay for any cut structural op or `write_cells`, and for any cut or re-applied op on a
  group cell): 3,000 random cuts, 790 on the fast path, all equal to replay in reads and bytes;
  the round 7 M4 case passes, and `mutant_tf_from_mark` and `mutant_tf_no_restore` fail it; 43 vs
  307 ms at 10,000 rows. The round 8 M5 hand-off cut is not reached, since the spike falls back
  for every group cell, and barrier and row/col property ops are not run.
- **Call boundaries.** `truncate(seq)` and `truncate_fast(seq)` accept only a `seq` at a call
  boundary: the last op of a call (a `write_cells`, a splice, a flush) or the commit mark. A
  `seq` inside a call is refused with `TruncateInsideCall { seq, call_end }` and changes nothing:
  cutting there would leave a partial call that was never accepted, against §3.2's "re-applies
  only ops that were accepted on the same prefix", and a hand-off's choice and
  `SharedFormulaMaster` are decided per call (review round 9, m5). Adapter transactions hold
  call-boundary `seq`s, so they never meet it.
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
| `TruncateInsideCall { seq, call_end }` | truncation at a `seq` inside a call of several ops (§10.3) |
| `JournalInternal { seq, source }` | a save, replay or shadow apply failed after acceptance (a bug), or a refused flush (§4.5, §9.3, §11) |
| `JournalBroken { at }` | any call but `truncate` or reload after a failed flush (`at` is the flush's start `seq`; §11, Q6), or after a replay that failed (`at` is the commit mark; §9.3, review round 7, m4) |

### 13. Relationship to other ADRs

#### 13.1 ADR-0001

Kept unchanged: Layers 1–3, T1/T1b/T2, `Err` ⇒ byte-identical. The Journal adds two
guarantees on top: locality is checked against the Base for the whole Journal (§9.1), and a
failed Commit cannot reach the caller as a poisoned state (§9.3).

#### 13.2 ADR-0003, ADR-0009, ADR-0014, ADR-0017, ADR-0018

Their operations run once, on the shadow at append; the output is the shadow's. Their refusals
keep their error kinds and their oracles keep applying. ADR-0003 §3.3 and ADR-0018 §3.7 run once
per save instead of once per write (R3, `CalcPolicy::Deferred`). Additions on `main`: P1 and P2
(§4.1), and C1–C8 (§7.4), all listed in Delivery.

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
     | `sst` item order and unused items | R4 drops unused session items and numbers survivors canonically; the sequential registries are append-only | both: `@uniqueCount` = item count; every index in range. Journal side only: `@count` = the Base's count plus (the number of `t="s"` cells in the output minus that in the Base), counted directly over every sheet, so cells removed by structural ops count (R4; review round 7, m2); absent iff the Base's `sst` has no `@count`; a Base without an `sst` part counts as 0 (review round 8, n2). The sequential side is not held to `@count`: on `main` a number written over a shared-string cell leaves `@count` unchanged (F4, A.8) |
     | order and unused entries of `cellXfs`, fonts, fills, borders, `numFmts`, `dxfs` | R4 keeps fewer `xf`s; R1/R2 append none for no-ops and write-backs | every `s` and `dxfId` in range; equality is through resolution in P. Journal side only: no unreferenced session item (R4) |
     | shared-group hand-off shape | the sequential side writes `si` and `ref` of a split group by its own history; R2 compares `f` attributes, so a cell that was handed off is never restored on the Journal side either, and both sides split the same groups (R1 never skips a write on a group cell, review round 8, M1), but the split groups' attribute bytes are not part of P | per-cell formula text in P; the hand-off's new master is in the Journal's changed set (§4.4), so oracle 4 and the `dimension` predicate cover it |
     | metadata part: presence, its relationship and content type, order and unreferenced types and blocks | a `set_dynamic_formula` on a cell a later structural op deletes leaves it on both sides; a restore (R2) of a dynamic-array cell can leave the block its session write added unreferenced | both: every `c/@cm` and `c/@vm` resolves to a block in range; a Base metadata part is kept; a Base without the part gains one only if the side's log has a `set_dynamic_formula`. Blocks and parts no `c` references may remain on either side |
     | `dimension/@ref` (SheetJS `!ref`) | the sequential call grows it on every write, a no-op included; the Journal grows it only by the changed set (§7.2 step 4) | both: present iff the sheet's Base (or new-sheet template) has it. Sequential side: covers every surviving cell it wrote, a written cell being one the plan of any call it ran targets, barrier ops included; a cell a conditional op skips (`remove_formula` without a formula, P1 on a missing cell) is not written. Journal side: covers every surviving cell of its changed set **and**, without reference to the changed set, every surviving cell that a keyed or barrier op of the log targeted and whose value, type, resolved style, presence or formula-ness (has `f` or not) differs from the Base cell with the same id, or that has no Base cell (review round 8, M3; formula text is left out because a structural op rewrites it without a write); on a sheet with no changed cell and no structural op, the `dimension` element has the Base's bytes |
     | presence of `calcChain.xml` | the sequential path removes it on any formula-set change, even a reverted one | present only if the side's formula-cell set equals its Base's (by id: a Base formula cell whose id died is a difference) and no structural op of the side met its ADR's calcChain condition (a formula cell moved or deleted, ADR-0018 §3.7; `delete_sheet`, ADR-0009; review round 7, M3; review round 8, m1), and then byte-equal to the Base's |
     | cached values `<v>` and `fullCalcOnLoad` | R2 restores reverted caches and R3 strips only over the changed set, so the Journal side strips less (Q1) | both: a kept `<v>` (of a formula cell or of an array result cell) is byte-equal to the Base's for that cell id, **and** no cell it reads (ADR-0003 dependents, transitively) differs **in content** from the aligned Base cell: value, type, formula text with shared members expanded; a style difference counts only when the workbook is style-sensitive (both halves are required; the spike checked only the first until its verification, review round 7, B1; "differs in P", the earlier wording, fails on the **sequential** side, which strips nothing for a style-only change of an input, Appendix B.2); the Journal's strip set ⊆ the sequential strip set. **Structural direction** (review round 8, M6), checked by id against the sequential reference's own record: the harness records, at each structural op of the sequential side, the cells that op's ADR strips there (ADR-0018 §3.7 or ADR-0009 §12, white-box, by id); every such cell that is alive and holds `f` in the Journal's output has no cached value there. This covers what "no cell it reads differs" cannot: `ROW()`, `CELL`, `ADDRESS`, `OFFSET`/`INDIRECT` with text, whole-column references whose content shifts, shrunk ranges and names, and references turned `#REF!`, all stale with every read cell equal. **Probe clause**
     (review round 9, M1), independent of the sequential side's record, which lists only cells
     that held a formula when the op ran: the harness also runs a **probe**, the Base with only
     the log's structural ops applied in order (no keyed or barrier ops), recording at each the
     cells its ADR strips (ADR-0018 §3.7 or ADR-0009 §12, by id, white-box); every cell that a
     log op targeted and that holds `f` and a `<v>` in the Journal's output, and that the probe
     stripped, is a failure (such a `<v>` can only be the Base's, by the first half, and the
     probe shows a structural op of the log made it stale). A case whose probe refuses a
     structural op skips this clause, and such cases are counted. `fullCalcOnLoad` (not in manual mode), **in both directions**: on each side it is set if and only if the Base had it, a cache of a cell the side did not write was stripped, the side's cell writes changed its formula-cell set (ADR-0003 §3.3 item 2), or a structural op of the side met its ADR's `fullCalcOnLoad` condition (any formula among its dependents, cached or not; ADR-0018 §3.7; review round 10, m3); the one-directional "set if" of revision 8 caught neither a missing nor an extra flag; the stronger "or any formula written" of revision 7 does not hold on `main` (F5, A.8) |

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
     - formulas in the fixtures and the values (J2): position-dependent ones (`ROW()`,
       `COLUMN()`, `CELL`, `ADDRESS`, `OFFSET` and `INDIRECT` with text), whole-column and
       whole-row references, named ranges read through `ROWS`/`SUM`, and 3-D ranges, so that the
       structural direction of the cache predicate is exercised by generated cases, not only by
       fixed ones (review round 8, M6);
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
       `col/@style` on column B, `set_style_index(0)` on missing `B5` (the sequential side writes
       `<c r="B5" s="0"/>`, the Journal `<c r="B5"/>` by C7; both are a present cell with `xf` 0,
       default format; R1 does not skip it); a write at `D10` then
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
       equal: F13), and `set_number(G3, 4)` on an array result cell holding 4 (refused on both
       sides with `ArrayResultCell`; F13, §4.5;
       `f13_same_value_on_merge_non_anchor_and_array_result`); a missing `B6` in a `customFormat` row, `remove_formula`,
       `set_num_format("0.00")`, `set_number(1)` (the row's fill is kept; F1). When P1 lands:
       P1 on missing `B5` under a bold `col/@style` (no `c` on either side), and
       `set_number(5)` then P1 on missing `B6` (an empty `c` without `s` on both sides).
       For J2 (review round 10, m3): on sheet `A`, `=B!A1` with no `<v>`, then
       `insert_rows(B, 0, 1)` (`fullCalcOnLoad` on both sides, though nothing is stripped).
       Added after review round 7: `C1 = A1*2` with cache 2, `set_number(A1, 5)` then
       `set_font(C1, italic)` (both sides strip `C1`'s cache; B1); a shared master given a
       constant and then its original text (a hand-off on both sides; the new master's row is in
       the Journal's allowed diff and `dimension`; M2); `set_string("abc")` over
       a rich string "abc" (applied on both sides, the runs dropped; m1). For J2: `insert_rows`
       before a row holding a formula with a calcChain (no `calcChain.xml` on either side), then
       a style patch on the moved formula cell (no cache on either side; M3); `delete_rows` over
       a `t="s"` cell written earlier as a number, and over a Base `t="s"` cell (`@count` by the
       Journal predicate; m2).
       Added after review round 8: on a shared group `C2:E3` with master `D2`, the member `E3`'s
       own text written back (`set_formula(E3, "$A3+E$1")`; R1 does not skip it, both sides split
       the group), then `write_cells([D2: 1, E3: 2])` (the same result on both sides, `Ok`; M1);
       `F = A1*2` with a cache, a constant written to `F` and then its text written back, then
       `set_number(A1, 5)` (no cache for `F` on either side: the restore put back
       `has_cache`; M2). For J2: under `dimension` `A1:C5`,
       `set_number(F20, 6)`, `insert_rows(100, 1)`, `set_number(F20, 7)`, `set_number(F20, 6)`
       (`F20` is in both sides' `dimension`; M3); `ROW()` below an `insert_rows`,
       `MATCH(x, A:A, 0)` over a shifted column, `ROWS(MyRange)` over a shrinking delete, a
       reference turned `#REF!`, each followed by a style patch on the formula cell (no cache on
       either side; M6); `move_sheet` that moves a sheet into or out of a 3-D range read by a
       formula with a cache, and `delete_sheet` with a calcChain (both sides strip and drop
       calcChain alike; m1).
       Added after review round 9: on the shared group `C2:E3` with master `D2` (member text
       `$A{r}+{c}$1`), `set_number(D2, 1)` (the group hands off to `C2`, `=$A2+C$1` with the Base
       cache) and then `set_number(A2, 99)`, and the same two ops in the reverse order: no cache
       for `C2` on either side (M2). For J2, with sheets `A` and `B`: `A!X = MATCH(7,B!A:A,0)`
       with a cache, `set_number(A!X, 9)`, `insert_rows(B, 3, 1)`, `set_formula(A!X,
       "MATCH(7,B!A:A,0)")` (no cache on either side; caught by the probe clause, not by the
       sequential record); `A!X = B!A5*2` with a cache, `set_number(A!X, 9)`,
       `insert_rows(B, 3, 1)`, `set_formula(A!X, "B!A5*2")` (same text, now the old `A4`; no
       cache on either side, and `A!X` is content-changed); the same pattern with `add_sheet` and
       `move_sheet` into a 3-D range, `rename_sheet` making a name resolve, and a formula through
       a name over `B` (the restored dependency edges are the new ones on both sides); and a cell
       on `A` set from 5 to 9, `insert_rows(B, 3, 1)`, then 10 and 9 (`A!X` keeps its carried mark
       and its row in `dimension`) (M1).
       Added by the spike verification of revision 8 (Appendix B.2), the first four each failing
       its `adr_*` mutant: on the array `G2:G3 = {A2:A3*2}`, `set_number(A2, 99)` (no cache for `G2` or
       `G3` on either side); on the shared group `C2:E3`, `set_formula(E3, "$A3+E$1")` then
       `write_cells([D2: 1, E3: 2])`, with `Sheet2!A1 = Sheet1!C2*2` (its cache kept on both
       sides; the Journal strip set ⊆ the sequential one); `A5` holding `=1+1` and listed in
       calcChain, `set_number(A5, 5)`, `delete_rows(4, 1)` (no `calcChain.xml` on either side);
       `set_font` on missing `B!A7` under `A!A1 = MATCH(7,B!A:A,0)` (the cache kept on both
       sides); a style-only change of an input (the dependent's cache kept on both sides).
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
     `compact_registries`, `dimension`, canonical cells, serialize, `rebase_after_save`, swap),
     in `restore_cell`, in each structural operation's steps 3.1–3.5 of ADR-0018 §2.1 on the
     shadow, and in a shadow apply: no output, Base and log unchanged, the shadow unchanged or
     rebuilt by replay as §9.3 says (a panic in steps 2–7 leaves the stale mark), the next
     `save` of an empty Journal returns the Base bytes, and a retried Commit without the fault
     succeeds with the same output. A fault injected into the replay itself (every attempt)
     leaves the Journal `JournalBroken` until `truncate` or reload (§9.3).
   - Part-way refusals: a splice whose delete succeeds and whose insert (`ShiftOffSheet`) or
     row write refuses returns that error, and a flush whose op `i` refuses (for each `i`)
     returns `JournalInternal` with it as `source`. Both leave the internal state as before
     the call (log, key index, `seq` counter, vectors, tombstones, echo ends, snapshots,
     shadow `to_bytes`); the id counter may only have grown. After the splice, View reads are
     also compared; after the flush every call but `truncate`/reload returns `JournalBroken`.
   - **Invariant**: every Journal accepted in oracle 1's generator saves with `Ok`. Any
     `JournalInternal` fails the test.
4. **Locality against the Base**: the byte diff of the output against the Base lies in the
   union of §9.1's allowed-diff rows (ADR-0014 §2, ADR-0018 oracle 1); a cell R1 skipped, or R2
   restored without a carried mark (§4.4), contributes no row and keeps the Base's bytes
   byte-for-byte (in epoch 0 every restore is of this kind). A cell restored to a later
   epoch's snapshot keeps its carried mark and its row (review round 8, M3).
5. **GetPut and no-ops** (Journal path only): an empty Journal, and a Journal whose ops are all
   skipped or restored (write the Base value; write then revert; a string, and a style patch,
   written then reverted, so the registry parts return to their Base nodes, review round 9, m1;
   a value write under a dependent then revert; a formula written away and back, its cache
   kept), save
   byte-identically to the Base, in the first session and after a save (C6). White-box: an op R1 skips leaves the shadow's worksheet
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
     cell, or by the shadow's writes (C5); treating an op on a missing cell as a no-op when its
     `xf` equals `s = 0` or the row/col default (round 5 M1); a strip made by a save dropped by a later
     style-only write;
   - review round 7: R3 subtracting the whole changed set instead of the content-changed cells
     (B1; oracle 1's fixed case, with the full cache predicate); no `rebase_after_save` (M1;
     oracles 12 and 13's second-session fixed case); snapshots for the op's own cell only, not
     every target (M2; oracle 1's hand-off fixed case through oracle 4 and `dimension`);
     structural strip records dropped, and calcChain kept after a formula moves (M3; J2 fixed
     cases); `truncate_fast` re-applying from the commit mark instead of from the snapshot (M4;
     oracle 14 across a structural op, J3); C7 off (M5; oracle 11's style round trip); the
     stale mark set after step 2 (m4; oracle 3's panic case); `sst/@count` not decremented by a
     structural delete (m2; J2);
   - review round 8: R1 skipping a write on a shared or array cell (judged by P) (M1; oracle
     1's shared-member-then-batch fixed case); `restore_cell` that does not put back the derived
     state (M2; oracle 1's `has_cache` fixed case, oracle 12's white-box census comparison,
     oracle 14's `delete_sheet_strict` case); a restore that clears the carried mark (M3;
     oracle 1's `F20` case through the `dimension` predicate, J2); C6 that moves only the
     re-rendered nodes' spans (M4; oracle 12's `A1`/`A3`/`A5` second-session case); a
     `truncate_fast` without the multi-target and group-cell fallback (M5; oracle 14's hand-off
     cut, J3); structural strip records dropped, caught by generated cases through the
     structural direction of the cache predicate, not only by a fixed case (M6; J2); sheet ops
     recording no strips, or `delete_sheet` not removing calcChain (m1; J2);
   - review round 9: a structural op that closes only its own sheet's snapshots and those of
     the formulas it rewrites (M1; the J2 fixed cases on another sheet, through the probe
     clause); R3 subtracting a hand-off's new master (M2; oracle 1's `D2`/`A2` hand-off case;
     subtracting the content-changed cells is caught only together with raw seeding, below); C4 that leaves a reverted registry part dirty (m1; oracle 5
     with a string write and revert); `truncate_fast` without the row/col property fallback (m2;
     oracle 14); C7 skipping a cell restored to a later epoch's snapshot (m3; oracle 11, J2); a
     `truncate` inside a call accepted (m5; oracle 8); R1 skipping part of a `write_cells` (m5;
     oracle 1 with a `write_cells` of a write-back and a change);
   - spike verification of revision 8 (Appendix B.2), the wordings that failed, kept as
     mutants: R3's strip test without array result cells (`adr_r3_formula_only`; the `G2:G3`
     case); R3 seeded by §4.4's raw equality (`adr_r3_seed_raw`; the round 8 M1 and round 7 M2
     cases, the group proptest); a cell created by a style patch counted as a content change
     (`adr_missing_is_content`); a dead id's formula change dropped from the calcChain test
     (`adr_dead_mark_drops_fx`); C7 keeping `s="0"` on a created cell (`mutant_c7_created_keeps_s0`).
     **Equivalent mutants**, recorded so they are not mistaken for gaps: round 7's "subtract the
     content-changed cells" (`mutant_r3_subtract_content`) is equivalent to "subtract nothing" under
     P-compared seeds (§7.1 R3), and caught only together with `adr_r3_seed_raw`; R1 (b) dropped
     (`mutant_r1_group_cells`) is unobservable in a spike whose R1 judges only plain formulas, so
     J1 must test it with C1's R1 on a group cell (it may be equivalent in J1 too, since C1's
     (c) reports census kind and group; if so J1 records it as equivalent; review round 10, n3);
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
    row inserts at row 1 on the 1M-cell sheet (append cost); save cost (§7.2 steps 2–7)
    against a sequential `to_bytes`, target 2× (ADR-0018 §3.8; spike: 1.6–1.8× with the text
    layer, 2.01× with revision 8's rules and epochs, 4.64× with a reopen (A′), §7.3), with C6's span walk over every node of the re-rendered parts (or its reparse
    fallback) and C7 included (review round 8, m3, M4); append overhead of R1/R2; a normalized read's
    `would_strip` and an alternating setter/getter load (1,500 pairs at 1.1M cells; §6.1,
    review round 7, m7); truncation (`truncate_fast` and replay) and part-way-refusal replay;
    peak memory and Wasm memory capacity, against the current paths.
11. **History independence (proptest, byte-identical).** On one Base, a log F and a log L that
    is F with **noise** inserted give **byte-identical** outputs, and also equal P, oracle 1's
    predicates and oracle 4's locality. Noise is a block of keyed ops, within one epoch, on cells
    that hold their Base content when the block starts (named by id, so the definition holds
    after a structural op) and that R2 can restore: not shared or array members or masters, and
    not cells whose `f` has attributes `set_formula` does not write (review round 7, M2). Every
    cell the block targets (C1) ends in the state it had before the block. Blocks: a change of
    value (number, strings, bool, error or formula) and a write-back; a style patch (bold,
    italic, fill, number format, border, an index) and a write-back of the Base index; a write
    of the Base value; a write of the Base index. Three variants: noise only on cells F never
    writes; noise also on cells F writes later (before F's first op there), plus a different
    value written before each of F's full content ops; and a **style round trip after F's
    content ops** (after one of F's content ops on a cell, a patch and then the index the cell
    had, including `set_style_index(0)` on a cell without `s`; review round 7, M5). Noise on
    the excluded cells is a fourth variant compared in P only. Over the `dimension`s of oracle
    1's fixtures, which include shared groups and arrays from J0 on. Barrier ops are not noise
    (they are not restored and their `dxfs` are append-only). **Across structural ops**, stated as
    the spike verified it (Appendix B.2): noise inside an epoch, on cells not touched since that
    epoch's structural op, is **byte-identical** (5,000 cases); a block that **spans** a
    structural op (a change before it, the write-back after it, at the moved position) is
    **P-equal** and meets oracle 1's predicates, and is byte-identical only when the carried mark
    leaves no trace (259 of 385 cases in the spike; the others differ by `dimension` or a strip,
    as §4.4 says). The test asserts bytes for the first kind and P for the second. This replaces
    revision 7's acceptance criterion "sequential = normalize, then apply" (Delivery).
12. **Replay equality.** The live shadow's output equals the output after the shadow is marked
    stale and rebuilt by replay from the Base; checked again after a save and a second session,
    where the live side was rebased (C6) and the replay opened the output. White-box, the two
    shadows' derived state is compared too: census entries (kind, text, range, `has_cache`,
    `results_stripped`), dependency edges, `vm` entries and row indexes, so that later refusals
    and strips agree, not only the output (review round 8, M2); the spike has no such comparison
    (its shadow never restores a node in memory, Appendix B.2), so it is first run in J1. Fixed cases: (review round 7,
    M1) session 1 sets `A1` from 5 to 6 and saves; session 2 writes 7 and then 6 to `A1` (R2
    restores it) and saves: the output keeps 6, byte-equal to session 1's output; (review round
    8, M4) session 1 writes `A1` and `A3` and saves; session 2 writes `A5` (later in the same
    part, a clean node whose span C6 moved) and back, and saves: byte-equal to session 1's
    output.
13. **Bisimulation after a save.** Run L1 and save (J1, shadow kept). Open the output as a new
    Journal (J2). Run the same L2 on both. Refusals, normalized reads over the grid (the
    fixture's used range plus one row and one column around it; review round 7, n2) before and
    after L2, and the next outputs are equal, the outputs in P and in bytes. Fixed case: oracle
    12's second-session write-back (M1).
14. **`truncate_fast` = replay.** At a random cut, with and without an earlier save,
    `truncate_fast` and `truncate` (replay) give equal reads and byte-identical outputs. From J3
    the generator also cuts keyed ops after a **kept** structural op on cells written before
    it (review round 7, M4: `A1 = B5*2`, `insert_rows(3, 1)`, a cut write to `A1` gives
    `B6*2`), and, with shared groups in the fixture, keeps a write to a shared master and cuts a
    later write to the new master (review round 8, M5; both sides replay, by §10.3's
    conditions). Fixed case for the derived-state restore (review round 8, M2): a constant
    written over `S2!A1 = Sheet1!A5*2` and cut by `truncate_fast`; then
    `delete_sheet_strict("Sheet1")` is refused as after a replay (the restored census entry
    references `Sheet1` again; J3). Besides bytes and reads, the census and indexes of the two
    shadows are compared (white-box, as in oracle 12; not run by the spike, Appendix B.2).

## Delivery

Prerequisites (each its own task; the op stays `Unsupported` until it lands): **P1** before
J1 (ExcelJS clears); **P2** before J1 for session `xf` indices; **P3–P6** independently, each
joining oracle 1 with its own sequential reference when it lands. P7 and P8 are not planned
(Q1 (A), Q2 (A)). **C2** (with its derived-state restore) before J0, which builds R2 on it;
**C1, C3–C5, C7 and C8** (§7.4) before J1's Commit, except C3's structural part, before J2;
**C6** before (A) replaces (A′): J1 may ship without it, and until C6 lands the shadow is
reopened from the output after each save (Q7 (A′); review round 8, m3; review round 10, m5);
until C1 lands, R1 is off (§4.5).

0. **J0** (native only): ids, vectors, log and key index (style groups), the shadow with R2 on
   C2, the projection P, and oracle 1's harness on keyed ops **without a save**: the shadow and
   the sequential workbook are compared in P, before any save, together with refusal
   equivalence (i). The save-side predicates (`dimension`, the cache predicate, `sst/@count`,
   calcChain) need the Commit and C1's targets and C3, so they join in J1 (review round 9, m4).
   The measurement spike of oracle 10 for append cost; the save cost is measured in J1. The spike (`cb57de4`, `ad74d32`) did J0's
   keyed slice through the public API with the substitutes of §7.4; J0 redoes it on C1–C8.
   J0's fixtures include shared groups and arrays (the spike's `group_workbook` has a shared
   group, an array and a merge since its verification, Appendix B.2).
1. **J1**: View (echo per group and normalized), Commit (§7) for keyed and barrier ops with
   R1–R4, and the minimal part of §10.1 that oracles 5, 12 and 13's second session need
   (empty the log, set the commit mark, drop snapshots and marks, C6 or its reopen stand-in);
   oracles 2–5, 11 and 12, and oracle 13's fixed case; oracle 1's save-side predicates.
   **Acceptance**: oracle 1 (sequential equivalence, with the save) and oracle
   11 (history independence, byte-identical) at 20,000 cases each, and every rule's mutant
   caught. Oracle 1's cache predicate is implemented **in full**: a kept `<v>` is the Base's
   **and** no cell it reads (transitively) differs in content from the Base (the structural direction
   joins in J2). The spike implemented only the first half, which is why B1 of review round 7
   passed 20,000 cases (Appendix B).
2. **J2**: structural ops (splice under Q2 (A) included); oracles 1 and 11 with structural ops,
   the epoch rule of §4.4; oracle 6.
3. **J3**: echo records and the rest of §10.1, truncation with `truncate_fast`; oracles 7, 8,
   13 (generated) and 14.
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
    kept as an appendix, and the spike keeps it runnable (`EJ_MODE=compile`). Whether J0/J1 keep
    it as a test-only second reference for oracle 1 is Q7's side question.
12. **The compiled Commit, keeping the working copy as the next shadow** (F6, revision 7's
    §10.1 "left for measurement"): removes the reopen, but opening the Base alone costs about
    2.3× a sequential serialize, so the save stays above 2×. Not measured further. Rejected
    (Q7 (C)).

## Consequences

- Each edit costs its sequential call on the shadow plus R1's test and R2's snapshot and check
  (2–3 µs per op at 110k cells at `ad74d32`, with R1; while R1 is off until C1, 25 µs per op on
  a write-back-heavy session, §7.3); no reopen or serialize per edit.
- A save costs R3, R4, `dimension`, the canonical cell form, one serialize of the kept shadow
  and the rebase: 2.01× a sequential `to_bytes` (one run, so at the 2× target only within
  noise) in the spike's text-layer stand-in with revision 8's rules (1.6–1.8× before epoch
  support; without C6/C7 in core; remeasured in J1), 4.64× while the output is reopened
  instead of rebased (Q7 (A′)), plus about 1.7× of dependency-graph rebuild in the next
  session, against 8.1–8.6× for the
  compiled Commit. A structural op runs once.
  No Base open and no reopen at save.
- The output depends on the final state of each cell, not on how it got there (oracle 11), so
  write-backs and reverts are free, as SheetJS-style region writes need.
- Core grows eight APIs or API extensions (C1–C8): C1–C5, C7 and C8 before J1 (C2 before J0,
  C3's structural part before J2), C6 before (A) (J1 may ship under (A′) without it); C2 and C6 change `ooxml-core`'s Layer 2, shared with docx and pptx);
  without them the rules can only be emulated by a text layer that carries state across saves
  and does more than the rules say (Appendix B).
- Provenance is explicit, so echo-then-normalize behaviour is a read mode, not adapter code.
  Echoes end per style group.
- Atomicity no longer depends on each consumer: a failed Commit leaves the Base, and poison
  never escapes. A rollback, or a call refused part-way, costs a replay or a `truncate_fast`.
- No second engine re-derives the output: a silent bug or an uncovered history-dependent trace
  in the shadow reaches the output. Oracles 1, 11, 12 and 13 are the guard (§9.3).
- Session style indices are not readable before a save; adapters hold styles by content.
- Structural edits no longer move pending edits or hidden information by guesswork.
- Refusals equal the sequential API's by construction for part A, R1 included: validation runs
  first, and R1 skips only ops that change nothing a later validation reads, never a write on a
  shared or array cell (§4.5, §7.1; review round 8, M1). R1 is an optimization; the output and
  the refusals are the same with it off.
- Part B ops are refused until their prerequisites land, so some adapter behaviours wait.
- The adapters' per-item mechanisms in ADR-0021/0022 are superseded; their pinned behaviours
  must be re-established through the differential harness, and some may regress until then.

## Open questions for the project owner

**Owner decisions 2026-11-25** (Q1–Q6): every question is decided as its recommended option
(A). The options are kept below as the record of what was weighed. **Owner decision 2026-11-25**
(Q7, revision 8): (A) reached through (A′), with the side question answered yes.

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
    - **(A)** serialize the kept shadow under R1–R4 and the canonical cell form (§7), with no
      reopen; needs C1–C8 (§7.4), none of which exists on `main` today;
    - **(B)** the compiled Commit of revisions 2–7 (Appendix A): 8.1–8.6× with the reopen, 6×
      without; F1 and F7 open; an independent second engine; needs no core API beyond P1/P2
      and the plan types (F2);
    - **(A′)** (A), but after each save the shadow is reopened from the output (revision 7's
      §10.1) until C6 lands; then (A). It avoids the risks of C6 (review round 7, M1; review
      round 8, M4) at **4.64×** a sequential `to_bytes` per save (measured in the verification:
      155 ms save + 158 ms reopen at 10,000 rows; revision 8 estimated about 4×), **plus** the
      dependency-graph rebuild on the next write after each save (second session 81 vs 25 µs/op,
      about 1.7× a `to_bytes` at 2,000 ops), and lets J1 ship before C6. C6 then becomes a later cost optimization with oracles 12 and 13 as its
      guard;
    - **(C)** (B) with the working copy kept as the next shadow (Alternative 12): no reopen,
      still above 2× because the Base is opened at every save.
  - **Facts for the decision** (review round 7, m5):
    - The cost of (A) was measured with the spike's **text layer** standing in for the core
      APIs: 1.6–1.8× at `ad74d32`, **2.01×** with revision 8's rules and epoch support (§7.3).
      The 2× target is reached only within noise (2.01×, one run) and **only with the kept
      shadow**, so it depends on C6 (or its reparse fallback limited to the re-rendered parts)
      and J1 must show it on C2–C8; (A′) is 4.64× plus the next session's dependency-graph
      rebuild (about 1.7×). C1–C8 are not
      implemented; J1 measures (A) again on them (oracle 10), with C6's span recording and C7
      included.
    - **R1 is off until C1 lands** (§4.5): until then every write-back is applied and removed by
      R2, which costs a snapshot and a dirty part, and a write-back of a shared member's own text
      splits the group (as it does sequentially). The spike shows why it must be off (an R1
      before validation accepts writes the sequential API refuses with `MergedCell` and
      `ArrayResultCell`) and what it costs: 25 instead of 8 µs per op on a second session of
      write-backs.
    - **Spike evidence after verification** (Appendix B.2). The spike at `ad74d32` was weaker
      than revision 8 first said (B1 passed 20,000 cases under a half cache predicate; keyed ops,
      one epoch, no groups, R1 before validation). The verification then ran revision 8's rules
      with the full cache predicate on shared and array formulas, row and column structural ops
      and a second session under (A) and (A′), with a mutant per rule and per round 7–9 detail
      (20,000 cases for oracle 1 and the one-epoch oracles 11–14, 5,000 for groups and structural
      ops). It found four rule wordings that failed or were ambiguous (array result cells, raw
      seeding, the dead-id calcChain test, a cell created by a style patch) and fixed each by
      stating the rule as the sequential plan runs it; none needed new machinery. **Still
      unproven**: sheet ops and their records, splice, barrier ops, P1, the round 8 M5 hand-off
      cut, the white-box derived-state comparison of oracles 12 and 14, a real C6 span walk,
      style-sensitive workbooks, and measurements at 1.1M cells; also `set_dynamic_formula`,
      rich runs and shared rich strings, row and column property ops, call-boundary truncation,
      a registry part's return to its Base node, a second session after a structural op under
      (A), a real `Deferred` op's record, and `fullCalcOnLoad` over uncached formulas
      (Appendix B.2).
    - (A) has no second engine; oracles 1, 11, 12 and 13 are its guard (§9.3).
    - **The evidence for (B) is discounted the same way** (review round 8, m6): the spike's
      `EJ_MODE=compile` oracle 1 uses the same half cache predicate (`src/project.rs`), so its
      20,000 cases do not check stale caches on dependents either. (B) strips through the
      sequential API, so it does not have B1's error, but that is an argument, not a test
      result. (B) also ran keyed ops only and one epoch; the spike's oracle 1 now checks the full
      cache predicate in both modes, but the verification reran (B) only through its mutant list
      (each compiled-Commit mutant still fails; Appendix B.2), not as a long run.
    - **Size of C1–C8** (no line estimate is made; listed by crate and the invariant each must
      keep): C1 plan without apply (`ooxml-xlsx` edit plans; a plan has no side effect) — the
      largest refactor, since every write path must split; C2 snapshot and restore
      (`ooxml-core`: a dirty-descendant count in `XmlDocument`; `ooxml-xlsx`: census, dependency
      edges and indexes restored with the node; the census stays a function of the tree); C3
      `Deferred`, `finish_calc`, `would_strip` and the structural records (`ooxml-xlsx` edit and
      the ADR-0009/0018 operations; Q1 (A) applied once); C4 registry compaction (`ooxml-xlsx`
      styles and shared strings; Base items keep their numbers, reverse lookups stay
      consistent); C5 `dimension` (`ooxml-xlsx`, small); C6 rebase (`ooxml-core` writer and
      `Package`; every span points into the current parse source, docx/pptx output unchanged);
      C7 canonical cells (`ooxml-xlsx` only, review round 8, m5); C8 state reads (`ooxml-xlsx`,
      read-only). C2 and C6 must pass every format's ADR-0001 tests and the python-pptx oracle.
    - **Errors only (A) can have** (review round 9, n2): M1 (a snapshot restored across a
      structural op on another sheet) and M2 (R3's subtraction missing a hand-off's new master)
      were stale caches with `Ok` from rules defined on **state** (restore to a snapshot,
      subtract a set) that disagreed with the sequential **history**. (B) strips through the
      sequential API and keeps no snapshots, so it has neither kind. Both were fixed by making the
      rule more conservative (close every snapshot; subtract nothing); the recommendation is
      unchanged, but this kind of error is (A)'s standing risk, and oracles 1 and 11 are its guard.
      The verification's raw-seeding and dead-id calcChain failures are of the same kind and were
      found by oracle 1 in the spike, before any core work.
    - **R1 is optional** after review round 8 (M1): it skips only what R2 would restore and
      never a write on a group cell, so the output and the refusals do not depend on it; C1 is still
      needed for the targets it reports (a hand-off's new master, §4.4), which the changed set
      depends on, and for R1's cost saving.
  - **Side question for (A):** keep the compiled Commit as a **test-only** second reference
    for oracle 1 in J0/J1 (it exists in the spike, keyed ops and one epoch), relaxing §9.3's
    "no second engine" for tests only? Recommended: yes until J2, then drop it unless it finds
    a bug the sequential reference does not.
  - **Decided (owner, 2026-11-25): (A), reached through (A′)**; side question: yes (the compiled
    Commit stays a test-only reference until J2). Recommended was the same: **(A)**, **reached through (A′)** if C6 is not ready when J1's
    Commit is: (A′) is the same design with a reopen, so nothing is built twice. (A) is the only
    option that can meet ADR-0018 §3.8's 2× target, and each rule has an oracle and a mutant to
    catch it (the round-7 to round-9 mutants and the verification's `adr_*` mutants included,
    all run in the spike). Its cost is eight core APIs or extensions and the loss of a second engine
    (§9.3). The owner agreed on 2026-11-25 to take the fallback (shadow serialization) and
    reduce its drawbacks with remedies; this question asks the owner to confirm that direction
    as the ADR's decision with the facts above.

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

## Appendix B. Spike evidence for §7

Evidence, not design: the spike runs §7.1's rules through `ooxml-xlsx`'s public API, so it
stands in for C1–C8 with a **text layer** (`src/shadow.rs`). B.1 is the run revision 8 was
first written on; B.2 is the later run of revision 8's own rules.

### B.1 Commit = shadow serialization (commit `ad74d32`)

Details are in the spike's README, section "Commit = shadow serialization". B.2 closes part of
"What the spike does not show" below; the list is kept as it was written.

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
  all pass) and one fixed case per rule. The oracle 1 cache predicate in the spike
  (`src/project.rs`) checks only "a kept `v` is the Base's", not "no cell it reads differs",
  so these passes say nothing about stale caches on dependents (below). A failed save keeps the shadow and the carried state;
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
- **Limits.** Keyed ops only (no structural or barrier ops), one epoch; no shared formulas or
  arrays in the fixture; the no-op test runs before validation (F13); style no-ops only for
  `set_style_index`; `truncate_fast` writes snapshots back with ordinary writes and falls back
  to replay for missing, empty, rich, inline and shared-formula cells; `raw_input` is kept in
  Rust.
- **What the spike does not show** (review round 7). Revision 8 first cited the spike as
  evidence for more than it covers:
  - **B1.** The spike's R3 (`src/shadow.rs`, `calc()`) subtracts the whole changed set from the
    strip set, as revision 8's R3 did. With `C1 = A1*2` (cache 2), `set_number(A1, 5)` and
    `set_font(C1, italic)`, it writes `<f>A1*2</f><v>2</v>` with `Ok`, and its `oracle1()`
    passes the case because its cache predicate has only its first half. The 20,000 passing
    cases therefore do not cover the case; §7.1 R3 is corrected, and the spike's code is not.
  - **M1.** The text layer opened the **new** Base lazily at every save and carried pins, so it
    never restored a node whose spans pointed into an older Base. Its second-session tests pass
    for that reason, not because the shadow alone would have been right; C6 is new.
  - **M2.** No shared group or array in the fixture: hand-off targets, R1 on a shared member
    and noise on such cells are untested.
  - **M3, M4.** No structural op and one epoch: R3's structural part, calcChain after a
    formula moves and `truncate_fast` across an epoch are untested (J2, J3).
  - **M5.** The layer wrote **every** written cell in a canonical form and restored the cache of
    a cell whose content came back under a new style (`Act::BaseStyle`). Neither is in C1–C5 as
    revision 8 listed them, so "Commit is literally `to_bytes()`" and the history-independence
    result held for the layer, not for those APIs; C2's content restore and C7 are new.
  - **Review round 8.** Further paths the spike never ran: the text layer restored at save and
    never touched the shadow's census, so the derived-state restore of C2 is untested (M2); one
    epoch, so the carried mark is untested (M3); C6's span walk does not exist in the spike
    (M4); `truncate_fast` fell back for shared-formula cells, so the hand-off cut is untested
    (M5); the structural direction of the cache predicate has no implementation (M6).
  - **Review round 9.** Also never run by the spike: snapshots closed by a structural op on
    another sheet and the probe clause (no structural ops; M1); a hand-off's new master in R3's
    strip set (no shared groups; M2); the return of a registry part to its Base node (the text
    layer compared texts instead; m1); call-boundary truncation (m5).
  - The rules of §7.1 and §7.4 are stated to be correct regardless of these gaps; the fixed
    cases and mutants that check them are in Test oracles 1, 3, 9, 11–14, for J0–J3.

### B.2 Revision 8 verification (commits `5eb021d`..`6339047`)

After review rounds 7–9 the owner stopped review-only iteration and had the spike run revision
8's rules (§4.4, §4.5, §7.1 R1–R4, §7.2, §10.3, oracles 1, 4, 5, 11–14). Each rule and each
detail fixed in rounds 7–9 has a mutant (a `--features` switch, `mutants.sh`) that must fail a
test. Where a rule as written failed, the written rule stays behind an `adr_*` switch and the
spike runs the fixed rule by default. Details are in the spike's README, section "Revision 8
verification"; new tests `tests/groups.rs` (shared group, array, merge: `group_workbook`),
`tests/structural.rs` (row and column ops on two sheets: `struct_workbook`, and the oracle
workbook), `tests/sessions.rs` (second session).

- **How the spike stands in for C1–C8.**

  | API | Stand-in | Limit |
  |---|---|---|
  | C1 targets | `handoff_target`: before a content write on a shared master, the first member in (row, column) order, as `plan_handoff` picks it, is snapshotted and tracked | shared groups only; no validation before R1, so R1 is off |
  | C2 within an epoch | the text layer: a restored cell is written at save as its snapshot's bytes | the shadow's own node keeps its history (F11) until the next epoch boundary |
  | C2 at an epoch boundary | `materialize`: before a structural op the shadow is serialized, restored and content-restored cells are spliced back as their snapshots' bytes, `dimension` is set back to the epoch's, and the shadow is reopened | one serialize and two parses per structural op |
  | C3 structural record | the op runs first on a copy of the shadow under `StripDependentCaches`; the cells whose cache it removed (by new position), whether it moved or deleted a formula cell, and the `t="s"` cells it deleted are its `calc` record | only cells holding a cache are recorded, as `plan_strip` does; a real `Deferred` op's record is not compared; no `needs_full_calc`: the spike sets `fullCalcOnLoad` whenever calcChain is removed, which is not main's condition (§7.1 R3) |
  | C4, C5, C7 | the text layer at save, as in B.1, with Base-node lookups by id across epochs | — |
  | C6 | (A) kept shadow + the text layer's carried state; (A′) reopen after each save (`EJ_REOPEN=1`) | no span walk; a session with a structural op always reopens after its save |
  | C8 | `Sem` with a shared member's text expanded by a reference shifter for plain A1 references | plain references only; no rich runs |
  | ids | positions remapped through each structural op for the key index, echo records, marks, records and written cells | rows and columns; no sheet op |

  The harness's own stand-ins: P expands shared members the same way, array result cells count
  as caches, and the sequential side's and the probe's strip records are taken by diffing the
  cached cells before and after each structural op.

- **Verified** ("held": every fixed case passes, the proptests pass, and the rule's mutant fails):
  - oracle 1's **full cache predicate**, both halves, with "differs in content" (oracle 1;
    `project::stale_cache_check`; `mutant_half_cache_predicate`); read as "differs in P" it fails
    on the sequential side (`a_style_only_input_change_keeps_the_cache_on_both_sides`);
  - R3 without subtraction on round 7 B1 (`mutant_r3_subtract_changed` fails);
  - **shared and array hand-off**: hand-off targets (`m2_round7_master_constant_then_text_back`,
    `mutant_no_handoff_target`), round 9 M2 in both orders (`m2_round9_handoff_then_input_both_orders`),
    R1 narrowing and **refusal equivalence** with R1 off (`m1_round8_member_write_back_then_batch`,
    `mutant_r1_skip_group`; `f13_same_value_on_merge_non_anchor_and_array_result`, `r1_unvalidated`),
    R3 reading the restored state (`m2_round8_restored_formula_then_input`, `mutant_r2_no_derived`,
    the spike's analogue of round 8 M2);
  - **structural strip records** (`mutant_drop_records`, also through generated cases),
    **workbook-wide snapshot closing** and **carried marks** (the three round 9 M1 cases in
    `tests/structural.rs`; `mutant_close_own_sheet`, `mutant_restore_clears_mark`; the probe
    refused 0 of 2,270 cases with an accepted structural op);
  - **`dimension`** (`m3_round8_f20`, `a_write_then_its_row_deleted`), **`sst/@count` after
    deletes** (`m2_round7_sst_count_after_a_delete`, `mutant_no_sst_removed`), **calcChain on a
    moved formula** (`m3_round7_insert_before_a_formula_then_restyle_it`);
  - oracle 11 across epochs (byte-identical within an epoch, P-equal across one: 259 of 385
    spanning blocks byte-identical; `history_across_a_structural_op_is_p_equal`) and oracle 12
    across epochs;
  - **`truncate_fast` with its fallbacks** across epochs (`fast_truncate_equals_replay_across_epochs`,
    `m4_round7_fast_truncate_after_a_kept_insert`; `mutant_tf_from_mark`, `mutant_tf_no_restore`);
  - **second session** under (A) and (A′) (`tests/sessions.rs`; `mutant_no_rebase`); the whole
    earlier suite also passes with `EJ_REOPEN=1`.

  Long runs: oracle 1 and the one-epoch oracles 11–14 at 20,000 cases (also under (A′));
  groups and structural ops at 5,000 cases on each fixture.
- **Rule wordings that failed or were ambiguous**, each fixed in the body and kept as a mutant
  (oracle 9): R3's strip test missed **array result cells** (`adr_r3_formula_only`); R3 seeded by
  §4.4's **raw** equality stripped a hand-off master's dependents (`adr_r3_seed_raw`); the
  **calcChain** test lost a dead id's formula change (`adr_dead_mark_drops_fx`, found by a 5,000-case
  run); a **cell created by a style patch** read as a content change (`adr_missing_is_content`).
  C7 as written held; the spike had not followed it for a created cell (`mutant_c7_created_keeps_s0`).
  The longer runs also found three spike bugs, each now a fixed case.
- **Equivalent mutants.** `mutant_r3_subtract_content` (round 7's R3) gives the same output as
  revision 8's R3 once seeds are compared as P compares them; it fails only together with
  `adr_r3_seed_raw`. So round 7's subtraction and revision 8's "nothing subtracted" are
  equivalent after the seeding fix, and §7.1 keeps the simpler statement. `mutant_r1_group_cells`
  (R1 (b) dropped) is unobservable because the spike's R1 judges only plain formulas; J1 tests
  it with C1.
- **Measurements**: §7.3, second table.
- **Still unproven**: sheet ops (`add_sheet`, `move_sheet`, `rename_sheet`, `delete_sheet`) and
  their §12 records; splice; barrier ops; P1; the round 8 M5 hand-off cut (the spike replays for
  any group cell); the white-box census and dependency comparison of oracles 12 and 14 (the
  spike's shadow never restores a node in memory, so there is nothing to compare); a real C6
  span walk; style-sensitive workbooks; measurements at 1.1M cells with revision 8's rules;
  `set_dynamic_formula` (spill result cells, the metadata predicate, `vm` as derived state; the fixtures have no dynamic arrays); rich runs and shared rich strings (C8); the row and column property ops (§10.3); call-boundary truncation (`TruncateInsideCall`, review round 9, m5); the return of a registry part to its Base node (C4; the text layer compares texts); a second session after a session with a structural op under (A) (the spike always reopens after such a save); the white-box comparison of a real `Deferred` op's record (C3); `fullCalcOnLoad` for a structural op whose formulas hold no cache (`needs_full_calc`, R3; review round 10, m3).

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
