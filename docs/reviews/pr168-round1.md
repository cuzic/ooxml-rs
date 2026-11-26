対象: impl/xlsx-ej-c1 @ bc3eac6abd2567b962b61075ccebde2de447af01
converged: no

# PR #168 review, round 1 (slice B: R1 wiring + flip, stacked on #167)

Blocker 1 / Major 2 / minor 4 / nit 3. Scope: the PR's own commit only
(`origin/impl/xlsx-ej-c1a..HEAD`, one commit); slice A assumed correct.

## Findings

### B1 (Blocker): R1 skips a call whose plan staged a table-column rename

- Where: `crates/ooxml-xlsx/src/journal/mod.rs:432-436` (R1 test),
  `crates/ooxml-xlsx/src/tables.rs:374-475` (`plan_with_headers` staging).
- Evidence (reproduced end-to-end with a temporary probe, since removed):
  base workbook with table `T` over `A1:A2` where header cell `A1` holds
  `Old` but the `tableColumn/@name` is `Diverged` (a diverged third-party
  file, opens and edits without error). `set_string(A1, "Old")` through a
  `Pair`: the Journal logs `applied=false` (the cell report is all-same),
  the sequential side applies and renames the column to `Old`, and
  `finish()` fails with `P differs at xl/tables/table1.xml` (323 vs 328
  bytes). So a plan that is not the identity is skipped, and the Journal
  and sequential sides diverge in a byte-compared part.
- Why: staging (`tables.rs:381-462`) stages a rename whenever the written
  text differs from the column name, but the R1 test judges only the cell
  report. When the base is diverged (cell text == new text != column
  name), the report is all-same while `staged.docs` is non-empty, and the
  skip drops a real rename. ADR-0024 §7.1 R1(c) explicitly lists `tables`
  among what a skipped op must leave unchanged. Unreachable in oracle 1
  today (no fixture has tables), hence undetected.
- Fix: skip only when the staged work is empty. Add
  `pub(crate) fn is_empty(&self) -> bool` to `StagedTables`
  (`tables.rs:230`), and conjoin it into the R1 test for
  `Staged::Headers` (the `Staged::Dynamic` arm is safe via C3 —
  `DynamicFormula` never reports same — but the gate belongs at the same
  `if`). Add the probe above as a fixed case (diverged table base:
  identity header write is `applied`, projections equal). The condition
  is conservative in the `set_attr`-same-value corner (applies a true
  no-op), which is the safe direction.

### M1 (Major): coverage test fails without `--all-features` (regressed from slice A)

- Where: `crates/ooxml-xlsx/src/journal/tests/oracle1.rs:737-740`.
- Evidence: `cargo test -p ooxml-xlsx --lib
  journal::tests::oracle1::the_generator...` (default features) fails at
  line 737 with `full restores {"formula": 11, "inline": 3, "value": 9}`:
  `value` 9 < 10 and `shared` 0 < 1. On the slice-A base the same command
  passes (`value` 53, `inline` 23, `formula` 31, `shared` 8). CI is blind
  (it runs `--all-features`, where the test passes), but plain `cargo
  test` is now red. Skip floors pass in both configs (default: 243 skips,
  180/51/8/4). Note `shared` restores never fire at all without
  fault-injection, so a floor alone cannot fix that config.
- Fix: add generator bait that fires a shared-string write-back restore
  in both configs (preferred — also relieves the knife-edge `shared == 1`
  in the passing config, see m1), or gate the R2 floors per feature.
  Either way, run the full suite on default features before re-review.

### M2 (Major): oracle-10 report is incomplete and above the spec bar

- Evidence: the body reports "mixed Journal append ≈ +11–16%", against the
  spec §4 bar ("≈ old apply within noise; same bar as J0b-2's +5–7%
  within ±8% noise"). No CI gate, so not blocking on the number alone —
  but spec §4/§6.8 require the before/after `append_cost` table plus
  machine/`rustc -V`/profile, and the body gives only deltas ("release,
  10k rows"). The "content/write-back back at baseline" claim (via the
  `guarded` change) is unverifiable as stated.
- Fix: paste the full before/after table with machine, `rustc -V` and
  profile into the PR body; file the styles-only residual as a follow-up
  (Open Q3 covers direction, but there is no issue to point at).

### m1 (minor): floors are knife-edge even in the passing config

- Evidence (`--all-features`, fixed seed): `shared` restores == 1 against
  floor >= 1, whole-batch skips == 2 against floor > 0, two-half == 5
  against floor > 2. One generator/fixture perturbation breaks the test.
  The spec asked > 5 per skip kind; Open Q2 discloses the weaker floors.
- Fix: accept the "fires" levels for this PR (fixed seed still detects
  total breakage), but add bait as a follow-up so no floor sits at its
  exact count. This is the same bait as M1's fix.

### m2 (minor): "adjacent statements" comment overclaims

- Where: `crates/ooxml-xlsx/src/journal/mod.rs:417-420`. Between the R1
  test and the apply there are ~60 lines (target collection, the snapshot
  loop, `part_before`), not "adjacent statements". The code is safe —
  `snapshot_cell`/`state_of`/`sheet_part_text`/`cell_key` are reads plus
  in-memory caches (verified by reading; `string_state_of` reads the
  `sst` part directly and never activates the registry), so plan
  positions stay valid — but the comment (copied from spec §3.3, which has
  the same overstatement: snapshots must run after the test, since the
  skip path takes none) states a falsehood.
- Fix: reword to "nothing mutates between the R1 test and the apply —
  only snapshot reads — so the plan's positions stay valid".

### m3 (minor): mutant list missing from the PR body; drop-(b) spec entry is wrong

- Spec §6.7 requires every §5 mutant listed in the body with its failing
  test; "details in review thread on request" does not satisfy it.
- Verified by running: drop-R1(b)-only survives (as the body says), and
  the justification checks out — ADR round-10 n3 predicts exactly this
  equivalence ("C1's (c) looks at census kind and group"). The spec's
  "drop R1 (b) → must fail fixed 35" entry is therefore incorrect; the
  killing mutant is R1-by-P. Record that correction in the body next to
  the list. Also verified killed: `all`→`any` by fixed_42, drop-`exists`
  by fixed_37, formula-text-only by
  `report_formula_compares_text_and_attributes`, the poi `ca` test, and
  the oracle floors.

### m4 (minor): skip-path activation handling duplicates the helper

- Where: `crates/ooxml-xlsx/src/journal/mod.rs:441-443` sets
  `registries.strings = None` directly instead of using
  `forget_activation_on_error` (the `poisoned.is_none()` guard there is
  vacuous on this path, but the duplication can drift).
- Fix: route through the helper (as `plan_err` does) or comment why the
  direct assignment is equivalent.

### n1 (nit): `op_of` comment is stale

- Where: `crates/ooxml-xlsx/src/journal/mod.rs:714-716` still says indices
  are read "after `apply_seq` returned `Ok`". The skip path reads them
  with no apply at all, and the apply path uses `apply_with_headers` /
  `apply_writes_with`. The property still holds (a refused op never
  reaches `op_of`, via the plan now) — update the mechanism description.

### n2 (nit): fixed_37's deviation from the spec wording should be recorded

- Spec §4.36 says "same number twice on missing B5 → both applied", which
  is self-contradictory under R1 (the second write is an identity
  write-back and must skip). The test's reading (two different missing
  cells, plus the round-5-M1 style case) is correct. Accept; note it in
  the PR body so the next reader does not "fix" it back.

### n3 (nit): `tally_skip`'s row/col `unreachable!` is correct but brittle

- Where: `crates/ooxml-xlsx/src/journal/tests/oracle1.rs:581`. Fine today
  (row/col always `applied: true`); if J2 ever skips them the message
  should say which invariant broke. Optional.

## Open questions (disposition)

1. `edit_cell_ext` → `write_with_headers` delegation: verified
   non-blocking. Sequential single cells go `edit_cell_ext` →
   `write_with_headers` (`edit.rs:546-548`); the Journal goes `plan_call`
   → `plan_with_headers` (`mod.rs:344-351`), and P2 `set_cell_xf` funnels
   the same way (`setters.rs:146-148`). Same plan entry both sides.
2. Weakened skip floors: accepted for this PR (see m1), but M1 must be
   fixed regardless — the default-features failure is a real regression,
   not a floor-tuning choice.
3. Style report cost follow-up: agreed, not blocking (but see M2: the
   residual needs an issue, not just a question).
4. Equivalent mutants as defense-in-depth (C6): agreed. `!touched` is
   subsumed by (b) (a hand-off touches only shared-group cells, which (b)
   vetoes) and drop-(b) is equivalent per round-10 n3; both verified by
   runs above. Keep them.

## Checked (no issue)

- F13 order on every path: plan → `Err` → report → skip-or-apply; row/col
  bypass R1 with the sequential call; dynamic plans
  (`plan_dynamic_formula`) before the report. `plan_call` matches the
  sequential setters arm by arm (same prep fns, range check first as
  `cell_mut`, `check_editable` placed exactly as `write_cells`/
  `write_with_headers` place it).
- Whole-call (`all`), two-half conjunction in `state_same`, non-empty
  target rule (conditional skips and row/col stay `applied`); `C10`
  (skip consumes `seq`, pushes keys, no snapshots, no R2; fresh string
  activation forgotten).
- `targets()` deletion: the report covers named writes plus each
  hand-off's new master (dedup kept); R2/snapshot-failure handling
  unchanged.
- The `guarded` result-anchor shortcut (`edit.rs:211-218, 252-255`) is
  sound: unshifted non-style plans refuse result cells at
  `edit.rs:657-662`, and nothing mutates between plan and report.
- `into_registry` at apply time is the same pure constructor the
  sequential setter calls, seeing identical package state (only reads run
  between); its `Err` maps to `Op` on both sides, with the local `fresh`
  snapshots dropped — no new refusal, no leak.
- Fault re-arm (`oracle1.rs:653-658`) is index-correct (wrap-safe: `i+1`
  never wraps mid-loop) and fault floors still pass (24 fired, 7/7
  steps).
- Slice-B `fixed_24` change (conditional skip no longer tracked) follows
  the spec's replacement of B7; `replay`/`r2`/`keys` test updates are the
  minimal R1-induced changes with `applied` assertions added.
- Clippy `-D warnings` on `ooxml-xlsx` (all targets/features): clean, run
  here. Fixed tests 42/42 green; oracle-1 coverage green on
  `--all-features` (run here).

## Verdict

converged: no (Blocker 1 / Major 2 / minor 4 / nit 3). B1 needs a code fix
(staged-emptiness gate + regression test); M1 needs bait or per-feature
floors; M2 needs the full perf table in the body.
