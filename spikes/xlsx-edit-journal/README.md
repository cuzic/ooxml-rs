# spikes/xlsx-edit-journal

A minimal spike for [ADR-0024](../../docs/adr/0024-xlsx-edit-journal.md) (xlsx Edit Journal,
revision 7). It is the keyed-op slice of J0 and J1. It shows that ExcelJS-style cell values and
styles can be read and written through a Journal. It also checks that the ADR's rules hold on
real code, and it measures the costs that J0 must measure (Test oracle 10).

This is a standalone crate with its own `[workspace]`. The main workspace never builds it. It
uses `ooxml-xlsx` by path, **through its public API only**, and changes no crate. No Wasm or
JS shim is built: the ExcelJS shapes are mirrored in Rust types.

```sh
cd spikes/xlsx-edit-journal
cargo test --release                                  # oracles 1–3, 5, 8 + shadow-mode tests
EJ_MODE=compile cargo test --release                  # the same suite with the compiled Commit
EJ_CASES=20000 cargo test --release --test oracle1    # a longer proptest run
cargo test --release --features mutant_no_revert      # each mutant must fail (lists below)
EJ_MODE=compile cargo test --release --features mutant_merge_missing
cargo run --release --bin measure -- 10000 2000       # oracle 10: rows, ops
```

The default Commit is now **the serialization of the shadow** (section
[Commit = shadow serialization](#commit--shadow-serialization-alternative-9)). The compiled
Commit of §7 is kept as `Mode::Compile` (`EJ_MODE=compile`, or `Journal::open_with`); the
first part of this README describes it and its results.

## What it builds

| File | ADR | Content |
|---|---|---|
| `src/op.rs` | §4.1, §4.2 | `Op` covers keyed part A: content (`set_number`, `set_string`, `set_bool`, `set_error`, `set_formula`, `remove_formula`), style patches (full: `set_format`, `set_style_index`; partial: `set_num_format`, `set_font`, `set_fill`, `set_border`), and two-half ops (`set_datetime`, `set_number_with_format`). `Clear` is P1 and is refused with `Unsupported`. It also defines `Start`, `Origin::Session { seq, api, raw_input }`, and `apply_seq`, the one sequential call per op. |
| `src/journal.rs` | §3, §4.3, §4.5, §6.1, §10.3 | Base (`Arc<[u8]>`), log, key index, lazy shadow with replay, all-or-nothing calls (`append_call`), `truncate` (`TruncateBeforeCommit`), poison handling (`JournalInternal`), normalized and echo reads, echo records kept across a Commit, and Commit with its swap. |
| `src/normalize.rs` | §4.4 N1/N1s, §7.1 | Content folding (a full write drops earlier content back to the latest two-half op; a conditional op and a two-half op drop nothing), style composition (a full patch drops earlier patches), placement, and the greedy batch builder with the table's join rules. |
| `src/commit.rs` | §7, §9.2, §9.3 | Normalize, open the working copy from the Base, then compile batch by batch and skip no-ops (an existing cell only). The output is serialized once. Fault injection covers every step; a panic is caught. |
| `src/exceljs.rs` | §6.1, §13.3 | `EjWorkbook::load` / `get_worksheet` / `get_cell("B2")`, the `value()` / `set_value()` pair, `font` / `fill` / `num_fmt` / `style` get and set, the ExcelJS projection of a normalized read, and the adapter's `normalize(echo(x))` for the Reload law. |
| `src/project.rs` | oracle 1 | The projection P and the predicates that replace its exclusions. |
| `src/fixture.rs` | | The oracle workbook (merge, `col/@style` bold over missing cells, a `customFormat` row, formulas with caches and a calcChain, a sheet that refers to the first, `dimension` set by the caller) and the synthetic large sheet. |
| `src/bin/measure.rs` | oracle 10 | Measurements, with a counting global allocator. |
| `tests/oracle1.rs` | oracle 1 | Proptest and fixed cases, plus `normalize` idempotence. |
| `tests/laws.rs` | oracles 2, 5 | View law, per-group style echo, rollback, replay, Reload law, `Unsupported` P1, GetPut and all-no-op Journals. |
| `tests/atomicity.rs` | oracles 3, 8 | `Err` ⇒ unchanged for one-op and part-way calls, a poisoned shadow apply, Commit faults at every step, and truncation. |
| `src/shadow.rs` | Alternative 9 | Commit = shadow serialization with remedies 1–4 (see its section). |
| `tests/shadow_mode.rs` | new | History independence, replay equality, bisimulation after a save, `truncate_fast` = replay, locality, one check per remedy. |

### Notes on the slice

- **No structural ops, so the vectors are the identity.** `RowId::Base(r)` is row `r` for the
  whole session, the normal form has one epoch, and N3–N7 do not arise. Barrier ops are out of
  scope.
- **Batches run on the public API.** §7.1's batch is one `CellWritePlan` with
  `(CellOp, Extra { style })` per cell, which is crate-private. The spike runs each batch in
  three phases (see `commit.rs`):
  - phase A: style requests that resolve on the row/col default. These are style-only writes,
    and merged writes whose cell is missing, since one plan would create that cell with
    `missing_cell_style`.
  - phase B: content. One `write_cells` per sheet covers numbers, strings, formulas and
    `RemoveFormula` on cells whose start state is not `Formula`. Booleans, errors, two-half ops
    and formula-start cells get one call each, so two cells of a shared group never meet in one
    call.
  - phase C: patches that were merged after content on an existing cell.

  Because cells in a batch are distinct, these phases give the merged write's result.
- **Where `raw_input` is kept.** The task asked for `Origin::Session { …, raw_input }`, so it is
  kept in Rust. ADR §4.2 puts it in the adapter; nothing here depends on where it lives.
- **What P leaves out.** Shared-formula expansion is not needed because the oracle fixture has
  no shared group. Rich strings are absent from the fixture, and h1 is not modeled. The rest of
  each worksheet (`cols`, `mergeCells`, views) and every other part are compared
  byte-for-byte. `sharedStrings`, `styles`, `calcChain` and the calc flag are left out; each is
  replaced by its predicate.

## Results (2026-11-25)

### Tests

- **Oracle 1 (keyed).**
  - Coverage: 1,024 proptest cases per `cargo test` run, plus one run of **20,000 cases**
    (`EJ_CASES=20000`, 139 s). All pass.
  - Correction (ADR-0024 review round 7, B1): the cache predicate in `src/project.rs` checks
    only "a kept `v` is the Base's". The ADR's predicate also requires that no cell the formula
    reads differs from the Base; that half is not implemented, so these runs do not detect a
    stale cache kept on a dependent.
  - Generator: 1–32 ops per case on a 7×5 grid, plus hot cells (`B5`, `C3`, `C1`, `A3`, `D7`,
    `B6`) and `Sheet2`. It mixes value, formula, `remove_formula`, error and bool writes with
    style patches, both two-half ops and three refusals: a merged cell, an out-of-range `xf` and
    an unparsable formula. Each case runs against one of three `dimension`s: `A1:C5`, `A1:A2`
    or `A1`.
  - Idempotence: `normalize(normalize(log)) = normalize(log)` holds over 1,024 cases.
  - The ADR's fixed cases that keyed ops can express all pass:
    - `=1` then 5 on a formula cell;
    - `set_formula` then `remove_formula` on a Base constant, which leaves an empty cell;
    - `set_number` then `set_num_format` on missing `B5` under a bold column, which takes two
      batches;
    - X then Y and Y then X (value and fill, and three patches) on existing and missing cells,
      in a styled column and in a `customFormat` row. A value then a fill is 1 batch; three
      patches are 3 batches;
    - `set_datetime` / `set_number` in both orders on missing cells under a bold column;
    - `set_style_index(0)` on missing `B5`, which is not skipped;
    - 7 written back to `C3` under `dimension` `A1:A2`: the sequential side gives `A1:C3`, and
      the Journal keeps `A1:A2` and the Base bytes;
    - `remove_formula` then `set_style_index(1)`;
    - `set_number`, `remove_formula`, `set_fill` on a formula cell (the fill is kept);
    - `remove_formula` outside `dimension` `A1`, on a constant and on a missing cell;
    - a change and a revert under dependents;
    - a full patch inside a patch chain;
    - plus F1's counterexample below.
- **Oracle 2.**
  - View law, for number, string, bool, date, formula with a supplied result, and error.
  - A style patch does not end a value echo. A fill does not end a font echo. `style = {…}`
    ends every group's echo.
  - Rollback: `set x`, `set y`, `truncate` echoes `x` again.
  - Replay after a forced rebuild, a failed Commit and a truncation matches a sequential
    workbook read for read.
  - Reload law (`normalize(echo(x))`; a formula reloads without its result under Q1 (A)).
  - Echo records survive the save.
- **Oracle 3.**
  - Six one-op refusals leave the state (log, `seq`, key index, echo ends) and the shadow's
    `to_bytes` unchanged, with no replay. The refusals are `MergedCell`, `InvalidFormula`,
    `StyleIndexOutOfRange`, `InvalidNumber`, `InvalidValue` and `Unsupported`.
  - A three-op call refused at op 3 is undone. The shadow is rebuilt by replay and its bytes are
    equal.
  - A poisoned shadow apply (`inject_apply_failure`) gives `JournalInternal`, changes nothing,
    and leaves the shadow rebuilt.
  - Commit faults at normalize, open, a batch, a panic in a batch, serialize and before the
    swap. Each returns `JournalInternal` and leaves the Base and log unchanged. A retry gives
    the same output, and the next empty save returns it.
- **Oracles 5 and 8.**
  - Saving an empty Journal returns the Base. A Journal of 9 ops whose entries are all no-ops
    after folding (7 skipped) saves the Base byte-identically.
  - `truncate` then Commit is byte-equal to a Journal that never had the later ops.
    `TruncateBeforeCommit` changes nothing.

**Mutants (oracle 9, hand-written, `--features`).** Each one makes at least one test fail:

| Feature | Mutant | Fails |
|---|---|---|
| `mutant_noop_missing` | round 5 M1: `Index(0)` on a missing cell is a no-op | fixed case `B5`, proptest |
| `mutant_merge_missing` | round 2 M3: merge a patch into content on a `Missing` start | fixed `B5` case, X/Y, proptest |
| `mutant_style_last_wins` | B3: style patches folded last-wins | X/Y chains, proptest |
| `mutant_cond_last_wins` | round 2 M2: `remove_formula` drops earlier content | two fixed cases, proptest |
| `mutant_keep_shadow` | round 2 M4: shadow kept after a part-way refusal | part-way refusal test |
| `mutant_fold_at_append` | B2: fold the log at append | truncation test |
| `adr_literal_placement` | §7.1's placement read literally (F1) | F1's fixed case, proptest |

### Measurements (oracle 10, J0's share)

Environment:

- Machine: AMD EPYC 7313P (16 cores, shared), 125 GiB RAM, Linux 6.16.
- Build: rustc 1.99.1, release with `debug = "line-tables-only"`, single thread.
- Workload: wall-clock times from one run, each confirmed by two or more runs (spread ≤ 10%).
  Heap is counted by a global allocator.
- Fixture: a synthetic sheet of N rows. Columns A–J hold numbers; column K holds `=A{r}*2`
  with a cache, so every write in A strips one dependent.
- Ops: 2,000 random cells in A–J. Half are numbers, a quarter strings, and a quarter
  patches (`set_font` / `set_fill`).

| | 10,000 rows (110k cells, 400 KB) | 100,000 rows (1.1M cells, 4.1 MB) |
|---|---|---|
| open (`Journal::open` = `from_bytes` + style registry) | 245 ms | 2,526 ms |
| shadow heap after open, after first write, after 2,000 appends | 34.9, 36.1, 37.7 MiB | 345.6, 349.7, – MiB |
| append per op, Journal (shadow apply + start read + log) | **47–49 µs** | **365 µs** |
| the same ops on a bare `Workbook` (run second) | 70–73 µs | 639 µs |
| `truncate` to 1,000 + replay (`from_bytes` + 1,000 ops) | 295–307 ms | 3,221 ms |
| `normalize` + batches (2,001 ops → 1,992 entries, 2 batches) | 1.5–1.8 ms | 1.7 ms |
| Commit: open / apply / serialize | 160 / 168 / 70 ms = **419 ms** | 1,765 / 1,388 / 666 ms = **3,998 ms** |
| peak heap during Commit (shadow dropped) | 47.0 MiB | 446.9 MiB |
| reopen of the output (next shadow, §10.1) | 162 ms | 1,742 ms |
| sequential `to_bytes` | 69 ms | 684 ms |
| **save (Commit + reopen) / sequential `to_bytes`** | **8.4×** | **8.4×** |
| Commit of 10,000 distinct value writes: open / apply (one `write_cells`) / serialize | 159 / 294 / 66 ms | 1,816 / 1,888 / 704 ms |
| the same as 10,000 `set_number` + `to_bytes` | 180 + 67 ms | 1,601 + 669 ms |

Reading:

- **Append.** The Journal adds about 0.8 KB of heap per op (log, key index, echo ends) and no
  measurable time over the shadow's sequential call. The bare-`Workbook` row is slower in every
  run; it runs second, with the Journal's shadow alive, and the gap is not the Journal's doing.
  The per-op cost grows with the sheet (47 → 365 µs), as the sequential API's own cost does.
- **Replay.** It is dominated by `from_bytes` (~245 ms of the 295 ms at 110k cells).
- **Commit.** Normalizing is negligible. The cost is opening the Base, applying and
  serializing. At most one parsed workbook is alive, as §7.2 requires: the peak is
  1.35× one open workbook. A save costs 8.4× a sequential serialize, and 6× without the
  §10.1 reopen. See F6.
- **Memory.** The shadow is one parsed workbook: 35 MiB for 110k cells and 346 MiB for 1.1M
  cells (about 320 bytes per cell). The 1.1M-cell line "after 2,000 appends" was mis-sampled
  in the run and is left out.

## ADR へのフィードバック

The spike follows ADR-0024 as written. Where the ADR could not be followed, the spike made the
smallest change needed to proceed, and each such change sits behind a switch that restores the
literal ADR rule. The ADR text is not edited.

- **F1. §7.1's placement rule contradicts the Missing-start base rule.**
  - Placement: "a folded full entry takes the place of the key's first content op of its fold
    segment".
  - Base rule: the base of a missing cell is decided "skipping conditional content ops".
  - Counterexample: `B6` is missing in a `customFormat` row with `s=2` (a fill). The ops are
    `remove_formula`, `set_num_format("0.00")`, `set_number(1)`.
  - Sequentially, `remove_formula` does nothing, the patch creates the cell with the row's fill,
    and the number keeps it. Under the literal placement, the number goes to the
    `remove_formula`'s slot, creates the cell with `s` = 0, and the fill is lost.
  - The spike places a folded full entry at the segment's first **unconditional** content op
    (its own `seq` if there is none). With that rule, 20,000 cases pass.
  - `--features adr_literal_placement` restores the literal rule. Proptest then finds the case
    within about 1,000 cases.
- **F2. Commit needs crate-private plan types, and §9.2's style no-op needs a style API
  that resolves without applying.**
  - Batches: §7.1's merged `(CellOp, Extra { style })` write, and batched booleans, errors and
    two-half ops, exist only through `plan_writes` / `CellWritePlan`. `write_cells` carries no
    style request. So J1's Commit has to live inside `ooxml-xlsx`.
  - Style no-op: §9.2 skips a style entry whose resolved `xf` equals `s`. The public API cannot
    resolve a patch without applying it, so the spike tests only `Index(i)`. A plan-only style
    resolution is needed.
  - String no-op: telling "reads equal" apart for a shared **rich** string needs the `si` runs,
    which the public API does not expose.
- **F3. §6.1 ends an echo when "its key is written again", but there is one style key per
  cell.**
  - ExcelJS echoes per style group. After `cell.font = F; cell.fill = X`, `cell.font` is still
    `F`.
  - The spike keys echoes by field (`Value`, `Font`, `Fill`, `NumFmt`, `Border`; a full style
    sets all of them). The ADR should say whether this granularity belongs to the key index or
    to the adapter.
- **F4. Oracle 1's `sst` predicate "`sst/@count` = number of `t="s"` cells" fails on `main` for
  the sequential side.**
  - Writing a number over a shared-string cell (`B1`) leaves `count="2"` with one `t="s"`
    cell, because the string registry is not activated.
  - The spike checks `uniqueCount` and the index range only. Either the predicate is weakened
    or `main` is fixed.
- **F5. Oracle 1's predicate "`fullCalcOnLoad` set if any cache was stripped or formula
  written" fails on `main`.**
  - `set_formula` on a cell that has no dependents strips the cell's own cache but does not set
    the flag.
  - The spike checks the weaker form "a cache stripped from a cell that was not written ⇒
    flag".
- **F6. Save cost is 8.4× a sequential serialize, at both 110k and 1.1M cells (§7.2,
  oracle 10).**
  - ADR-0018 §3.8 targets 2×.
  - Opening the Base (≈ 2.3× a serialize) and the §10.1 reopen (≈ 2.3×) dominate. Without the
    reopen it is still 6×.
  - Owner decision needed: keep the working copy as the next shadow (§10.1 "left for
    measurement"), Alternative 9, or a different target for the Journal path.
- **F7. Batching does not pay on `main` (§7.1/§7.2).** One `write_cells` of 10,000 distinct
  cells applies in 294 ms; 10,000 `set_number` calls take 180 ms (1.1M cells: 1,888 vs
  1,601 ms). The Commit's gains come from folding and no-op skipping, not from batches. The ADR
  should not count batching as a cost win before `write_cells` scales.
- **F8. The ADR does not say how to tell P2 apart from a sequential refusal.**
  - A `set_style_index(i)` with Base `xf` count ≤ i < the shadow's count is P2 (`Unsupported`),
    while the sequential API accepts it. Oracle 1 (i), "refused iff the sequential API refuses",
    cannot hold for such indices.
  - The spike's generator uses only Base indices or ones out of range (99). The ADR should
    exclude part B ops from (i) explicitly.

## Commit = shadow serialization (Alternative 9)

Added 2026-11-25 on top of the slice above. The question: if Commit is "serialize the shadow"
(ADR-0024 Alternatives, item 9) and the §10.1 reopen is dropped, how much of that alternative's
cost can four remedies remove? Its listed costs are: the sequential calc strips and calcChain
removal of write-backs stay (no §9.2), and the output carries whatever the session did to the
shadow along the way (dirty nodes, registry entries, a grown `dimension`).

`Mode::Shadow` is the default; `Mode::Compile` is the §7 path, unchanged.

### Design

Commit is `shadow.to_bytes()` in spirit. `ooxml-xlsx` has no public API for remedies 2–4 on
the shadow itself, so they run as a **thin text layer** (`src/shadow.rs`, `serialize`). It
opens the Base lazily as a `Package` (nothing parsed), takes each part the shadow dirtied, puts
it through the layer, and calls `set_part` only where the result differs from the Base's
text. Every other part is raw-copied from the Base. The shadow is kept after the save. Its
differences from the new Base are carried as state (pins, strips, registry maps; see
[Carried state](#carried-state)).

1. **No-op judgment at append.** Before applying an op, `apply_tracked` reads the cell. If the
   cell exists and the op changes nothing, the op is logged (echo and `Origin` kept) but not
   applied. Content uses §9.2's test (the compile path's `is_noop_content`); style uses
   `Index(i) == s`. Other patches cannot be resolved without applying them (F2), so they are
   applied, and remedy 2 handles them.
2. **Restore on revert.** The first applied op on a cell in a session stores a snapshot
   (`Sem`: value or formula text and `f` attributes, `t`, rich flag, `s`; plus the Base cache).
   After each apply, if the cell equals the snapshot again (the cache is not compared), the
   cell is *restored*.
   - At save, a restored cell is written as **the Base's bytes** for that `<c>`, taken from the
     Base part's text.
   - A cell whose content equals the snapshot but whose style changed is written as the
     Base's bytes with `s` set (`Act::BaseStyle`). The shadow's node may have lost its cache
     (`set_formula` drops it) or kept `s="0"` from a reset; the Base's node has neither.
   - Every other written cell takes the shadow's bytes in a canonical form. Its registry
     indices are renumbered. `xf` 0 is written as the Base cell wrote it. An element that
     lost its children is written self-closing. These rules come from the counterexamples in
     [Results](#results-1).
   - `dimension` is the Base's element, grown only by this session's changed cells. A sheet
     with none keeps the Base bytes.
   - The View reads a restored formula cell with the Base's cache.
3. **Calc state deferred to the save.** The shadow runs `CalcPolicy::KeepStaleCaches`. It
   still sets `fullCalcOnLoad` and removes calcChain on its own copy, but the layer ignores
   those. At save, the **changed set** is the touched cells minus the restored ones. The
   strip set is the `Workbook::dependents` of the changed set's content changes, minus the
   changed set. (This is wrong, ADR-0024 review round 7, B1: a formula cell whose style
   changed while one of its inputs changed keeps its stale cache. ADR-0024 §7.1 R3 now
   subtracts only cells whose content changed; this spike's code is left as it was.) The layer
   then applies ADR-0003 §3.3 once:
   - strip `v`, `is` and `t` from each cell in the strip set;
   - set `fullCalcOnLoad` if the strip set is non-empty or the formula set changes. The
     layer adds the flag to the Base's `calcPr` text; otherwise it keeps the Base's
     `workbook.xml`;
   - remove calcChain (`Package::remove_part`) if a changed cell gained or lost a formula.

   Before the save, the View reads a pending strip as `Empty`.
4. **Collect sst/xf.** A session item is an `sst` item, `cellXfs` entry, font, fill, border or
   custom `numFmt` that the shadow added. The layer gathers the ones that are still referenced
   (by cells written and not restored, in (sheet, row, col) order). It appends them after the
   Base's items in that canonical order, and drops the rest. Base numbering is never changed.
   `sst/@count` is the Base's count plus the changed cells' `t="s"` delta.
   `uniqueCount` is the item count.

Replay (a stale shadow, a part-way refusal, `truncate`) runs the log through `apply_tracked`
from a fresh state, so a replayed shadow makes the same decisions.

`truncate_fast(t)` is the cheap undo the task allowed. It sets every cell that an op after
`t` wrote back to its snapshot, using ordinary writes (content, then `set_style_index`). It then
re-applies that cell's ops up to `t`. Remedies 2–4 hide what is left over (a restored node,
garbage items, a grown `dimension`, the shadow's calc flags). It falls back to replay when a
snapshot cannot be written back: a missing, empty, rich, inline or shared-formula cell.

#### Carried state

The kept shadow is not the output. The layer carries the difference until a later write
replaces it:

- **Pins.** Cells whose output is the Base's bytes (restored, or written in an earlier session
  and not since), with the Base cache for the View.
- **Strips.** Strips already made in an output. A shadow that never dirtied the sheet (for
  example `Sheet2`) still has the stale cache.
- **Registry maps.** For each session item already saved, a map from the shadow index to the
  Base index. `set_style_index(i)` for a Base `i` is translated to the shadow's index. The
  View translates back.

A save never reads the carried state for parts the shadow has not dirtied and that hold no
strips.

### Results

#### Tests (2026-11-25)

- **The whole earlier suite also runs in Shadow mode.** It is the default mode now, so
  `cargo test` covers oracle 1 (P-equal to the sequential reference, with its predicates), the
  View law, the Reload law, rollback, replay, `Err` ⇒ unchanged, Commit faults, GetPut and
  truncation. `EJ_MODE=compile` runs the same suite on the compile path, and it passes too.
- Five assertions are specific to one mode (batch and entry counts, the number of skipped
  no-ops, the number of replays). They are gated on `j.mode()`.
- In Shadow mode, a failed Commit keeps the shadow and the carried state. The Base and the log
  are unchanged, and a retry gives the same bytes (`atomicity.rs`).
- **New proptests (`tests/shadow_mode.rs`).** 512 cases per run by default. One run used
  `EJ_CASES=20000` for every proptest (see below).
  - **History independence, bytes.** The generator builds F, 1–16 keyed ops from oracle 1's
    generator (refusals included), and L, which is F plus noise. Noise blocks run on Base
    cells and each ends in the Base state:
    - a change of value (number, two strings, bool, error or formula) and a write-back;
    - a style patch (bold, italic, fill, `0.000`, border, index 2) and `set_style_index(base)`;
    - a write of the Base value;
    - a write of the Base index.

    `history_independence_bytes` puts noise only on cells F never writes.
    `history_independence_with_overwrites` also puts noise on cells F writes later (before
    F's first op there), and puts a different value (`99`) before F's full content ops. Both
    assert **byte-identical outputs**, P equality, oracle 1's predicates and locality, over
    the three `dimension`s.
  - **Replay equality.** The live shadow's output equals the output after
    `mark_shadow_stale` (a replay from the Base). It is checked again after a save and a
    second session, where the live side has carried state and the replay has none.
  - **Bisimulation after a save.** Run L1 and save (J1, shadow kept). Open the output (J2).
    Run the same L2 on both. The test compares refusals, normalized reads over the grid
    (before and after L2) and the next output. The outputs match in P and in bytes.
  - **`truncate_fast` = `truncate`.** The reads and the bytes match, with and without an
    earlier save, at a random `t`.
  - **Locality** (inside the history tests). Every Base cell that is not in the changed set
    keeps its bytes, except a dependent whose cache was stripped. A restored cell keeps the
    Base bytes byte-for-byte.
- **Fixed cases.**
  - No-op writes leave the shadow's worksheet clean.
  - A formula written away and back keeps the Base bytes and cache (the View reads the
    cache), and `dimension` does not keep the growth.
  - Strips are decided at save and carried to the next save on an undirtied sheet.
  - Collection and canonical numbering of `sst`/`xf`; the result equals the direct write.
  - A reset to `xf` 0 followed by a write gives the same bytes as the write alone.
- **Long run.** `EJ_CASES=20000`: `shadow_mode` (all six proptests, 119 s) and `oracle1` in
  Shadow mode (134 s). All pass (with the half cache predicate noted under oracle 1 above). The first long run of `shadow_mode` failed: see
  `fullCalcOnLoad` below.
- **Mutants.** Every remedy mutant fails. The seven earlier mutants still fail under
  `EJ_MODE=compile`.

| Feature | Removes | Fails |
|---|---|---|
| `mutant_no_noop` | remedy 1 | `noop_writes_leave_the_shadow_clean`, GetPut (skip count) |
| `mutant_no_revert` | remedy 2 | GetPut, oracle 1 (proptest + 2 fixed: the `dimension` predicate), `a_restored_cell…`, both history tests, replay, bisimulation, registry test |
| `mutant_no_defer` | remedy 3 (shadow strips at each write) | GetPut, `value_changed_and_reverted…`, both history tests, replay, bisimulation, `fast_truncate`, registry test |
| `mutant_no_gc` | remedy 4 (every session item kept, shadow order) | GetPut, both history tests, `fast_truncate`, registry and `xf` 0 tests |

Remedy 1 is invisible in the output once remedy 2 exists: a write of the same value is a
revert of length one. Its mutant is caught only by a white-box test and the counter. What it
buys is cost: no snapshot and no dirty part for a pure write-back.

#### Counterexamples found on the way (now fixed in the layer)

Each counterexample showed that **the shadow's state depends on the cell's (or the workbook's)
history**, not only on the final state:

- **`s="0"`.** `set_style_index(0)` on a cell without `s` writes `s="0"`. A later value write
  keeps it. The direct write has no `s`.
- **Lost cache.** `set_formula` drops the cache. A formula written away and back, then
  restyled, loses its Base cache in the shadow but not in the direct write. After a save, a
  `remove_formula` on it diverged from a replay.
- **Open/close form.** A node that loses its children (`remove_formula`) serializes as
  `<c …></c>`. A node parsed as `<c …/>` stays self-closing.
- **Carried strips.** A strip that a save made must survive a later style-only write to that
  cell.
- **`fullCalcOnLoad` is not monotone on the shadow.** The first version took the shadow's
  `workbook.xml` when the deferred rule wanted the flag, on the assumption that the shadow
  had set it at some write. In 20,000 cases, the replay and `truncate_fast` tests found
  histories (with a save in between) where the deferred rule needs the flag but the shadow
  never set it. Under `KeepStaleCaches` the shadow decides per write, from that moment's
  dependents and its own flag state. The layer now sets the flag on the Base's text.
- **Session `xf` indices.** After a save, a garbage `xf` the shadow kept can be revived by
  dedupe. The shadow's numbering then differs from that of a Journal opened from the output.

The layer handles each one. The View cannot hide the last, so a session item **not yet saved**
has no stable index (see below).

#### Measurements (same fixture and ops as above)

Two runs per size, same machine and build as before.

| | 10,000 rows (110k cells) | 100,000 rows (1.1M cells) |
|---|---|---|
| append per op, Shadow mode (no-op test + snapshot + revert check) | 49.5–51.5 µs | 362–363 µs |
| the same ops, Compile mode, same run | 46.5–49.1 µs | 372–402 µs |
| heap of 2,000 appends, Shadow vs Compile (≈ 2,000 snapshots) | +2.2 vs +1.6 MiB | +2.2 vs +1.6 MiB (Compile: earlier table) |
| **save, no reopen** | **116–122 ms** | **1,148–1,198 ms** |
| … calc decision (`dependents` of ~1,500 content changes) | 2.0 ms | 5.5 ms |
| … registries (1 new `xf` pair, ~50 `sst` items) | 2.2 ms | 4.6–5.1 ms |
| … sheets (shadow part serialized, Base part inflated, scan, rewrite) | 42–47 ms | 486–501 ms |
| … zip (one part recompressed, the rest raw-copied) | 66 ms | 647–680 ms |
| peak heap during the save | +20.8 MiB | +260 MiB |
| sequential `to_bytes` | 67–69 ms | 690–714 ms |
| **save / sequential `to_bytes`** | **1.68–1.80×** | **1.61–1.74×** |
| the compiled save (Commit + reopen), same runs | 8.3–8.6× | 8.1–8.3× |
| second session on the kept shadow (the same ops reversed: mostly no-ops), append / save | 8 µs/op, 107–112 ms (1.6×) | 10 µs/op, 483–501 ms (0.70×; output = Base) |
| `truncate` to 1,000: `truncate_fast` vs replay | 19–22 vs 305 ms (14–16×) | 116–121 vs 3,210 ms (27–28×) |

Reading:

- **Save.** The 2× target is met at both sizes. The extra cost over `to_bytes` is the text
  layer: inflating the Base part and two linear scans. The scans' `Vec<CellAt>` and the three
  part-sized strings cause the 260 MiB peak at 1.1M cells; a streaming scan would cut it. In
  core, remedies 2–4 would edit the tree before serializing, and this whole term would go.
- **Append.** No-op test, snapshot and revert check add 2–3 µs per op (5%) at 110k cells.
  At 1.1M cells the difference is below the noise.
- **Memory.** About 300 bytes per touched cell for the snapshot, 0.6 MiB for 2,000 cells.
  The shadow is not dropped at save, so the peak is one parsed workbook plus the save's text
  buffers. The compiled save had the working copy plus the Base.
- **Truncate.** Undoing from the snapshots is 14–28× faster than a replay and gives the same
  bytes. A replay costs `from_bytes`; the undo costs the reverted cells' writes.

### What core needs (each one replaces a piece of the layer)

| Needed in `ooxml-xlsx` / `ooxml-core` | What the spike does instead | Can the spike stand in fully? |
|---|---|---|
| **`plan_*` without apply**: validate an op and resolve its style request to an `xf` without changing the workbook (F2), so the no-op test runs **after** validation | content: §9.2 test on a read; style: `Index` only, other patches applied and caught by remedy 2 | No. A no-op skip can hide a refusal the sequential call would give, e.g. the same value written to a non-anchor cell of a merge (the fixture has none). |
| **`restore_cell(sheet, at, snapshot)`**: put back the original node (a clone of the parsed `<c>` with its span) and clear `own_dirty`/`subtree_dirty` up the chain when nothing else under them is dirty (`XmlDocument` needs a recount of dirty descendants) | text layer: Base bytes for restored cells, `Act::BaseStyle`, canonical form, View cache override, carried pins | Output: yes, by the tests. Shadow: no. Its nodes stay history-dependent (`s="0"`, lost cache, open/close form), so the layer carries pins forever. |
| **`CalcPolicy::Deferred`** + **`finish_calc(changed)`**: no strip, flag or calcChain step at write; one plan at save over the changed set (ADR-0018 §3.7 / ADR-0003 §3.3); plus a public `style_sensitive()` | `KeepStaleCaches` + own decision via `dependents()` + text strip; `fullCalcOnLoad` from the shadow's `workbook.xml`; calcChain via `Package::remove_part`; View masks pending strips | Mostly. Style-only changes strip nothing (style sensitivity is not public). `KeepStaleCaches` still changes the shadow's `workbook.xml` and calcChain, which the layer must ignore. |
| **`compact_registries(base_counts)`**: drop session `sst`/`xf`/font/fill/border/`numFmt` items no cell references and renumber the survivors canonically, in the shadow, rewriting the `s`/`v` of the cells that use them | text layer at save + carried maps + index translation for `set_style_index` and the View | Output: yes. View: no. An unsaved session `xf` has no stable index; the tests compare such reads by content. |
| **`dimension` policy**: grow at save from the changed set, not at each write | layer rewrites the `dimension` element | Yes |
| **a cheap per-part `serialize_into`** (or the tree edits above, so the shadow's own `to_bytes` is the output) | `read_part` + inflate the Base part + scan + `set_part` | Yes, at 1.6–1.8× |

With the first four in core, the shadow equals the output after a save, the carried state
goes away, the View has stable indices, and Commit is literally `to_bytes()`.

Correction (ADR-0024 review round 7, M1, M5): that last sentence overstates it. The layer also
writes every written cell in a canonical form and restores the cache of a cell whose content
came back under a new style, which the four APIs above do not do, and it opens the new Base at
every save, which hides that a kept shadow's spans point into the first Base. ADR-0024 §7.4
adds a content restore to `restore_cell`, a canonical cell form (C7), a rebase after save (C6)
and cell state reads (C8).

Correction (ADR-0024 review round 8, m4, M2, M5): the "second session" row above passes
because the text layer opens the new Base and carries pins, not because a kept shadow alone
would be right, so it is not evidence for C6. The layer never touched the shadow's census, so
restoring a cell's census entry with its node (C2) is untested here. `truncate_fast` fell back
to replay for shared-formula cells, so a cut across a shared-group hand-off is untested; the
ADR now makes the fast path fall back for any multi-target call or group cell (§10.3).

### ADR revision 8 へのフィードバック

- **F9. Alternative 9 with remedies 1–4 meets ADR-0018 §3.8's 2× target where §7 does not.**
  It saves in 1.6–1.8×, against 8.1–8.6× for compile + reopen. For the body (owner
  decision): make "serialize the shadow, keep it" the Commit of §7 and §10.1, with the four
  remedies as **normative rules**:
  - §9.2's no-op judgment moves to append;
  - "a cell back in its Base state keeps its Base bytes" (restore);
  - Q1 (A) is applied once at save over the changed set, defined as final ≠ Base;
  - session registry items are collected and numbered canonically after the Base's.

  The compiled Commit moves to Alternatives. It stays useful as an independent reference:
  oracle 1 holds in both modes.
- **F10. New test oracles for the body.**
  - History independence: two op lists with the same final cells give **the same bytes**.
  - Replay equality: live shadow = replay from the Base.
  - Bisimulation: the kept shadow and a Journal opened from the output stay in step, reads
    and bytes.
  - `truncate_fast` = replay.

  These replace the parts of oracle 5 that compile folding gave for free ("write then revert
  is byte-identical").
- **F11. The shadow's nodes depend on their history.** See the counterexamples: `s="0"`, a lost
  cache, the open/close form, carried strips, session `xf` indices. §3.2 should say that the
  shadow equals the sequential workbook **up to P**, not in bytes, and that remedy 2 needs
  `restore_cell` in core (table above). The spike's text layer should go in an appendix as
  evidence, not as the design.
- **F12. §6.1 and indices.** A session `xf` (and `sst`) index is not stable until a save numbers
  it. The View should not expose raw indices for unsaved items: return the resolved format
  (ExcelJS needs only that), or document the index as provisional. After a save,
  `set_style_index(i)` must be read in Base numbering (the spike translates it). F8's P2 rule
  then becomes "index ≥ the Base's `xf` count".
- **F13. The no-op test needs validation first (§9.2, §4.5).** A skipped no-op must still
  return the sequential refusal. This needs the plan-only API, or the order "validate, then
  judge". The spike could not test this, because its fixture has no such cell.
- **F14. §9.3 in this mode.** A failed save leaves the Base, the log, the shadow and the
  carried state unchanged (tested). "A fault in the shadow ends up in the output" (the
  Alternative's cost) is still true for a silent bug in a sequential call. Poisoning is still
  caught (`JournalInternal` + replay). Oracle 1 against the sequential reference is now the
  main guard, since no second engine re-derives the output.
- **For an appendix.** The measurement table, the carried-state mechanism, `truncate_fast`
  (14–28× faster than a replay), and the list of core APIs with their stand-ins.
