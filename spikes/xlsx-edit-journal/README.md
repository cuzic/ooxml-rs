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
cargo test --release                                  # oracles 1–3, 5, 8 (keyed part)
EJ_CASES=20000 cargo test --release --test oracle1    # a longer proptest run
cargo test --release --features mutant_merge_missing  # each mutant must fail (list below)
cargo run --release --bin measure -- 10000 2000       # oracle 10: rows, ops
```

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
