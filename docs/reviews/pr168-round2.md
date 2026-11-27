対象: impl/xlsx-ej-c1 @ 903c37531de1a80420e389e5f9fc4c24efa49d80
converged: yes

# PR #168 review, round 2 (fix verification + new-issue hunt)

Reviewer: pr168-reviewer-r2 (independent; fresh — did not write or
previously review this code). Scope: the PR's own commits above
origin/impl/xlsx-ej-c1a (slice B + round-1 review + fix commit 903c375);
slice A assumed correct.

Note up front: PR #168 reads closed-but-unmerged, slice B is not in main,
and slice A (#167) has since merged. This review covers the branch as
tasked; the branch needs a re-stack/re-PR regardless, which can carry
m5/m6/n4/n5.

New findings this round: Blocker 0 / Major 0 / minor 2 / nit 2.
Round-1 findings B1/M1/M2/m1-m4/n1-n3: all converged, each verified below
(in code, and by runs where stated).

## Round-1 fix verification

### B1 (Blocker): FIXED — staged-emptiness gate + fixed_44

- Gate (`crates/ooxml-xlsx/src/journal/mod.rs:128-139, 446-450`):
  `staged.is_empty()` conjoined into the R1 test; `Staged::Dynamic`
  never skips. Placement matches the round-1 prescription.
- Soundness and precision checked by reading: `StagedTables` has a single
  field `docs` (`crates/ooxml-xlsx/src/tables.rs:230-241`); staging fires
  iff a Text/Shared write renames a column (`old != text`,
  `tables.rs:391-445`, rename at 454-470); apply installs exactly the
  staged docs plus the cell plan (`tables.rs:488-501`). So staged-nonempty
  ⟺ a real rename: the gate vetoes exactly the B1 case, nothing more.
  `headers` is plan-time guard-only (`crates/ooxml-xlsx/src/edit.rs:648`).
  Only Text/Shared header writes can touch tables; every other op
  `continue`s past staging, and non-staged header writes are refused at
  plan time — both sides share the code, so no divergence.
- `fixed_44` passes (all-features, run here). With the gate removed it
  fails at the `last_applied` assert ("the staged rename is not the
  identity", `fixed.rs:755`), i.e. the op skips — exactly the B1 bug
  behavior. True gate test.
- `rezip_replace` pins the fixture shape (exactly one `name="Old"` in
  the table part), so fixture drift fails loudly. Whole-package
  recompression is harmless (both `Pair` sides read the same bytes).

### M1 (Major): FIXED — SharedBait + fault queue, both configs green

Ran the coverage test here in both configs (fixed seed):

- Default features: full restores shared 35, value 29, formula 11,
  inline 2; content 42; skips 230 (content 180, style 40, two-half 8,
  cells 2); families 7. PASS (round 1 was red here: value 9, shared 0).
- `--all-features`: shared 42, value 49, formula 11, inline 4;
  faults fired/recovered 57/57, steps 7/7; skips 241
  (194/37/5/5). PASS. The body's "57 fired/recovered" reproduces.
- Mechanism checked by reading: Oracle B1 holds shared `"x"`,
  Group A2 holds a number (`fixtures.rs`); odd bait occurrences write
  `"shbait"` away (applies — differs), even ones revert to the Base
  value (applies — differs from `"shbait"`), then R2 takes a full
  restore on `shared`/`value`. The pair structure is independent of
  the `fault-injection` cfg, so both configs are covered by
  construction, not luck. `"shbait"` never enters `written`, so
  `Repeat` cannot collide with it.
- Fault queue (`oracle1.rs:626-739`): bounded (at most +1 entry per
  case), terminates (re-arm at `i+1`, or carried to the next case in
  the coverage test); `at % len` is safe (`len >= 1`);
  `sequential_equals_shadow` uses a fresh queue per case, so no
  cross-case effect there. Re-arm on any unfired fault (refusal, R1
  skip, step never reached) is sound: fault steps are generic, so a
  later op is an equally good injection site.

### M2 (Major): CONVERGED on substance; tracking remainder is m5

- The body now carries the full before/after `append_cost` table with
  machine, `rustc -V`, profile, workload (10k rows, 2k ops), both run
  orders, per-kind rows and bare columns. The spec §4/§6.8 shape is met.
- Numbers: mixed is above the ±8% bar on 3 of 4 after-runs (+7…+19%);
  numbers/strings/write-back are at baseline in every after run;
  styles-only is +76…+78%. The over-bar residual is real and disclosed.
- The "whole residual is styles-only" attribution is evidence-backed:
  all per-kind after-runs agree; the guarded-disabled experiment (mixed
  83.3, numbers +45%, write-back +55%, styles unchanged) shows the
  shortcut restores content/write-back to baseline; the mechanism is
  code-verified (`StyleOnly` never takes the shortcut —
  `edit.rs:217` — and `result_anchor` is a linear scan over all live
  census cells, `census.rs:163-169`, ~10k entries on this bench); the
  mix arithmetic holds (mixed is 1/4 styles, `measure.rs:34-65`, so
  0.25 × ~20µs ≈ +5µs against observed +4…+12µs on the disclosed
  shared host). Weak spot, not a finding: the befores are single runs.
- No reasonable in-PR fix exists: the residual needs a census range
  index (shared core structure, J1-scale) or a plan-format/ADR-designed
  style shortcut (style ops skip every plan guard, `edit.rs:641-662`,
  so there is nothing sound to reuse in-PR). Both are disproportionate
  for a wiring+flip slice.
- Remainder: no follow-up issue was filed — the repo has no styles
  issue and no J1 batch tracking — although round 1 required one and
  the body claims "tracked as a C1 follow-up for the J1 batch".
  Per the pr166 "Track for J1" precedent this remainder is minor
  (m5), not a convergence blocker. J1's oracle-10 remeasurement may
  resurface the residual, but an append-cost re-run there is not
  guaranteed, so the issue should still be filed.

### m1 (minor): FIXED — floors have margin, none sits at its exact count

Shared 35/42 (was exactly 1 / red at 0); value 29/49; two-half 8/5
against > 2; whole-batch 2/5 against > 0. The "fires" levels stand as
accepted in round 1.

### m2 (minor): FIXED — comment reworded as prescribed

"Nothing mutates — only snapshot reads" (`mod.rs:430-434`) is accurate:
target collection is pure, the snapshot loop and `part_before` only
read (caches aside).

### m3 (minor): FIXED — full mutant table with corrections, spot-verified

The body lists every §5 mutant with verdict and killer (the task spec
itself lives outside the repo, so completeness is checked against
round 1's enumeration). Verified: all cited killer tests exist;
`check_accepted` enforces §4(a)(b) (bytes/snapshot/tracked) on every
skipped op (`tests/mod.rs:195-204`), which is what kills "skip takes
snapshot" via fixed_39; `content_seqs` asserts kill "skip pushes no
keys"; the three spec-entry corrections check out against the tests
(fixed_35 uses the `Shared` arm — D1 is shared rich — so no Text-only
mutant applies; fixed_36's member `f` is empty so text differs too;
fixed_39's formula identity skips under any text comparison);
fixed_43's dynamic targets missing A6 and the gate vetoes dynamic
skips anyway. Spot-run here: dropping the style conjunct from
`state_same` fails fixed_41 as the table says. Round-1 runs
(drop-(b) survival, `all`→`any`, drop-`exists`, formula-text-only)
stand; the table records the drop-(b) equivalence per round-10 n3.

### m4 (minor): FIXED — inline is equivalent, poison claim verified

`forget_activation_on_error` (`setters.rs:37-41`) collapses to the
inline exactly when `poisoned` is `None`. Poison is set only on apply
paths that return `Err` (`edit.rs`, `setters.rs`, `range.rs`,
`sheet_ops`, `workbook.rs`); plan and report only read (caches and
the known string activation aside), so on the skip path (plan `Ok` +
report `Ok`) `poisoned` is `None`. `accept` is entered after `settle`
(`append`) or on a fresh shadow (`replay`), so no stale poison
arrives either. The comment says this correctly.

### n1/n2/n3 (nits): FIXED

`op_of` mechanism comment, the fixed_37 body note ("do not fix back",
in Behavior notes), and the `tally_skip` invariant message are all
accurate as written.

## New findings

### m5 (minor): styles-residual follow-up issue not filed

The body's Oracle-10 section claims the residual is "tracked as a C1
follow-up for the J1 batch", but no such issue exists (repo issues
searched; no J1 batch tracking found anywhere). Round 1 required an
issue to point at.
Fix: file the issue (styles-only R1(b) census scan; Open Q3's two
directions) and link it from the body. Owner-side; minutes.

### m6 (minor): body's "conservative corner" sentence misdescribes the gate

The body says a same-value `set_attr` "applies a true no-op instead of
skipping (safe direction)". No `set_attr` exists on the cell-call/plan
path, and the gate is exact, not conservative: a non-empty stage always
carries a real column rename (see B1 verification above), so no true
no-op is ever forced to apply. As written, the sentence invites a
future "narrowing" of the imagined conservatism that would reintroduce
B1.
Fix: drop the sentence, or replace it with "the gate is exact: a
non-empty stage always carries a real rename".

### n4 (nit): Gates paragraph "42/42 fixed tests" is stale

43 fixed tests now (numbered to fixed_44, no fixed_20). Substance is
true — all pass in both configs, run here — just the count.

### n5 (nit): "structural, not luck" / "57 vs 24" wording

7/7 steps are robust but still seed-dependent (a `Registry` fault still
needs a later dynamic op; it now waits instead of dropping). "57 vs 24"
compares across generator-reshuffled case sets (SharedBait shifted all
downstream draws). Suggest "robust" and "57 fired/recovered (24
before, different case set)".

## Checked (no issue)

- Slice-A integration: the fix commit touches `journal/mod.rs`
  (gate + comments), `tables.rs` (one additive `is_empty`, no behavior
  change) and tests only. No slice-A file touched; no integration break.
- `GenOp::SharedBait` reshuffles the fixed-seed stream (new weight in
  `op_s`), so round-1 counts do not carry over — all floors re-verified
  by the runs above. Edge cases read clean: bait on a missing cell
  (tiny Oracle dims) applies then conditional-no-ops without restoring;
  on OutOfOrder A1 both occurrences refuse deterministically (the "or a
  skip" aside is imprecise but harmless); bait never pollutes `written`.
- Full runs here: `-p ooxml-xlsx --lib` default (279 pass) and
  `--all-features` (287 pass); `-p ooxml-xlsx --all-features` all
  targets green (cargo exit 0, 36 suites ok); `clippy -D warnings`
  clean; `xtask codegen --check` clean; 2000-case debug oracle-1 scale
  run green (fresh seed).
- The body's 20,000-case release run (154.9s) was not re-run here
  (cost); the 2k debug run corroborates generator robustness at scale.
- Round-1 open questions need no new disposition (Q1 non-blocking,
  Q2 fixed via M1, Q3 tracked via m5, Q4 agreed).
- The `write-back-heavy shows no R1 skip benefit` note is coherent: a
  revert changes content, so R1 correctly applies and R2 restores; no
  skips are expected and none are observed.

## Verdict

converged: yes (Blocker 0 / Major 0 / minor 2 / nit 2; every round-1
finding converged with evidence). Owner actions: re-stack/re-PR the
branch (PR closed, slice A merged); file the m5 issue.
