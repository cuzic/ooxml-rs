対象: impl/xlsx-ej-c1a @ 4c312a33e5baf2a010228f5bc14ec8484b867a64
converged: yes

# PR #167 review, round 2 (T7 C1 slice A: report + plan entries, R1 still off)

Round-1 counts: Blocker 1 / Major 2 / Minor 3 / nit 2. Remaining after the
fix commit (`4c312a3`): Blocker 0 / Major 0 / Minor 0 / nit 0. New findings:
none. Converged.

The fix commit touches exactly three files
(`git show --stat`: edit.rs +245, tables.rs +27, worksheet.rs reindent),
and I read the full diff: the only production-code delta is the B1
predicate line plus comments (11 deletions total = B1's 2 replaced lines +
the D2 rewrap + the 8 reindented match arms, so nothing else moved). The
rest is seven new tests. Scope still holds: xlsx-only, no new `pub` item,
journal untouched, R1 still off (`report`/`TargetReport`/`content_style_same`
keep their non-test `expect(dead_code)`, and a clean clippy run proves no
non-test caller appeared — an `expect` would fail the build otherwise).

Gates I ran in the review clone: `cargo test -p ooxml-xlsx --lib`
(276 passed, 0 failed, 5 ignored — 269 + the 7 new tests),
`cargo test -p ooxml-xlsx --tests` (all 35 suites ok, 0 failed),
`cargo clippy -p ooxml-xlsx --all-targets --all-features -- -D warnings`
(clean), `cargo xtask codegen --check` (10 files up to date). Workspace
suite not run: the delta is xlsx-only with no codegen inputs (same
justification as round 1).

## Round-1 findings: verification

### B1 (Blocker): `Shared` compares `v` numerically — fixed, confirmed

- Fix in code: `crates/ooxml-xlsx/src/edit.rs:323-330` now compares the
  literal text, `v.text() == i.to_string()`, with the C5 comment. I checked
  `RawNode::text` (`crates/ooxml-core/src/xml/node.rs:422`): concatenated
  raw text children, no trimming, so padded/whitespace spellings compare
  not-same. The new predicate is strictly more conservative than the old
  one (every literal match was also a numeric match), so no previously
  not-same report flips to same.
- Test: `report_shared_compares_the_v_spelling_literally` patches a written
  cell's `v` to `00`, asserts `exists && !state_same && !validation_same`,
  and asserts the sequential `edit_cell` normalizes the spelling to the
  bare index (proving a skip would diverge). The patch surgery is sound:
  it searches for `r="A1"` with the closing quote, so `A10`-style cells
  cannot match, and it asserts the padded form is present before use.
- Mutant kill verified by execution: I temporarily restored the numeric
  `parse_u32` compare, the new test FAILED, then I restored the fix
  (`git status` clean afterwards) and the suite is green again.

### M1 (Major): string-reservation purity test — fixed, confirmed

- Test: `string_reservation_plans_change_no_byte_and_set_no_flag`
  (`edit.rs`, next to the other purity tests) does exactly what round 1
  asked: `plan_string_write` plus one `plan_write_value` `String` item,
  each through `plan_cell_write` + `report`, asserting bytes-identical,
  `assert_clean`, and the invisible activation
  (`registries.strings.is_some()`). It also asserts the plan is genuinely
  reservation-bearing (`extra.string.is_some()` and `CellOp::Shared`), so
  the `plan_registries` string branch runs asserted. Passes.

### M2 (Major): funnel pinning + slice-B note — fixed, confirmed

- Test: `single_header_writes_funnel_through_plan_with_headers`
  (`crates/ooxml-xlsx/src/tables.rs:1059`) proves both halves: a single
  `edit_cell` on header cell A1 renames the table (asserts
  `name="Renamed"` in table1.xml), while the same write through
  `plan_cell_write` refuses with `TableHeaderCell` and stays
  byte-identical + clean. Passes.
- Notes: the PR body carries the slice-B sentence ("plan single writes via
  `plan_with_headers`, never via `plan_cell_write`", naming the pinning
  test), and `plan_cell_write`'s doc comment (`edit.rs:556-558`) repeats
  it. Both verified by reading.

### m1 (Minor): `cell_state.rs` outside spec §2 — fixed, confirmed (note)

- No code change, as instructed. The PR body records the deviation
  (`rich_state_of_runs` lives in `cell_state.rs` to share the private
  `string_state_of_item` read side). Verified by reading the PR body.

### m2 (Minor): `report` can `Err` where the plan is `Ok` — fixed, confirmed (note)

- No code change, as instructed (the diff contains no `shared_text`/R2
  change). The PR body records the wart (`t="s"` target with an
  unparseable `sst`, mirroring R2's pre-existing mapping, out of slice-A
  scope). Verified by reading the PR body and the diff.

### m3 (Minor): four untested report branches — fixed, confirmed

All four tests exist in `edit.rs`, all pass, and each pins the branch
round 1 named:

- (a) `report_named_handoff_collision_reports_once_with_the_touched_veto`:
  master content write at D2 plus `StyleOnly` at the C2 handoff target;
  asserts exactly 2 reports, C2 reported once as named with
  `state_same && node_same` but `!validation_same` — the `touched` veto.
  This is the load-bearing one and it pins the veto directly.
- (b) `report_group_flags_a_data_table_cell`: minimal `dataTable` sheet,
  asserts content refuses with `ArrayAnchor` (as for an array anchor) and
  the `StyleOnly` report has the group flag set.
- (c) `report_inline_rich_with_escapable_text`: identical escaped runs
  (control char + literal `_xHHHH_`) report same; a raw `_x0001_` run
  against the stored `_x005F_x0001_` reports not-same, proving nothing
  decodes on either side.
- (d) `report_two_half_with_real_style_requests`: `EnsureDate` same-style
  vs changed-style cases plus `Format` resolving to the current xf vs a
  different one — both halves of the conjunction in both directions.

### n1 (nit): formatting — fixed, confirmed, minimal

- `crates/ooxml-xlsx/src/worksheet.rs:696-712`: pure reindent of the
  `plan_write_value` arms (no token changed — verified in the diff);
  the D2 `report_of` call is rewrapped. No other whitespace moved in the
  commit, and no workspace rustfmt run (only these two hunks are
  formatting). The one long added line (the 139-char `dataTable` XML
  literal) is a string rustfmt would not split, consistent with
  pre-existing 100+ char lines in the same file.

### n2 (nit): copied `assert_clean` — left as-is per the review's explicit option

- Confirmed untouched. Fine.

## New findings

None. Specifically hunted and cleared:

- Fix-commit production delta is one predicate line + comments; all other
  hunks are tests. No new refusal, no behavior change outside `report`
  (which is test-only in slice A), no journal/R1 wiring.
- Entity-encoded digits (`<v>&#48;</v>` decode to `0` at parse, so the
  literal compare reports same): considered, not a finding. It affects the
  round-1-accepted `Number` arm identically, and any skip-vs-sequential gap
  there is pre-existing R2/`state_of` decoded-text semantics on a
  pathological input, orthogonal to slice A.
- `t`-attribute strictness (`Some("s")` without trim) and duplicate-`v`
  first-match: both fail conservative (report not-same / apply), and the
  fix only narrows the same-set. No new risk.
- Every new report test goes through `report_of` (fresh open from bytes,
  bytes-identical + clean asserted) or asserts bytes + flags directly, so
  the m3 tests re-verify plan purity rather than assuming it.
