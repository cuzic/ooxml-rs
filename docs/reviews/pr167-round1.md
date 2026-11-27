対象: impl/xlsx-ej-c1a @ c71ccb31a54cd5eb14d1860e7498d472ef4ae84f
converged: no

# PR #167 review, round 1 (T7 C1 slice A: report + plan entries, R1 still off)

Blocker 1 / Major 2 / Minor 3 / nit 2. Not converged: B1 is a wrong-report
predicate (demonstrated with a throwaway probe, deleted after use); M1 and M2
are cheap (one test, one test + a slice-B note).

## Checked

Gates I ran in the review clone: `cargo test -p ooxml-xlsx --lib`
(269 passed, 0 failed, 5 ignored, journal oracle1 `sequential_equals_shadow`
green), `cargo test -p ooxml-xlsx --tests` (all 35 suites green, existing suite
unmodified), `cargo clippy -p ooxml-xlsx --all-targets --all-features -- -D
warnings` (clean). Workspace suite and `codegen --check` not run (PR is
xlsx-only, no codegen inputs).

- Plan/apply split is behavior-preserving: `edit_cell_ext` already funneled
  through `write_with_headers`, which is now plan + apply
  (`plan_with_headers`/`apply_with_headers`, tables.rs:374/479); single setters,
  `write_cells` (via `plan_write_value`, worksheet.rs:691), `set_error` (via
  `error_value`, setters.rs:159), `set_string_inline` (via `plan_inline_write`,
  worksheet.rs:621), `write_rich_inner` (via `plan_rich_write`, setters.rs:73),
  and `set_dynamic_formula` (via `plan_dynamic_formula` + `MetaAction`,
  dynamic.rs:153/208) all delegate to the new plan fns with identical refusal
  order. Moved bodies are verbatim except the plan/apply cut.
- `apply_*` consume the plan: `apply_steps` re-locates via `plan_target` but
  takes the resolved style from the plan (`tp.style = w.style`, edit.rs:1097)
  and never re-plans guards/registries/hand-offs (`mutant_apply_replans`
  argued-absent stands; pre-existing, unchanged).
- V1 (plan purity) audited: `plan_writes` mutates only in-memory caches
  (index, contexts, `style_sensitive`, parsed-part cache, registry activation);
  `Styles::plan` works on a cloned `Planner` (`wb` used for reads,
  crates/ooxml-xlsx/src/registry/styles.rs:561); `SharedStrings::apply`
  early-returns without plans;
  activation never touches a doc. All 20 new tests assert bytes + flags.
- V2 (`write_cell` preserves h1/`cm`/`vm`, worksheet.rs:348) audited per op:
  only `f`/`v`/`is` are removed, `cm` is set only on the `DynamicFormula` path,
  `vm` is never written, and the reused-`f` path drops exactly the known
  attributes. The report arms are sound under skip-vs-apply+R2 (R2 restores the
  snapshot node iff post-apply §4.4 state equals it, journal/mod.rs:509):
  duplicate children, lonely `t`, non-reused `is` with unknown children, and
  the dropped formula cache all heal through the restore. I checked every arm
  against this condition; only B1 fails it.
- `group_or_array` matches the spec exactly (`FKind` has exactly the four
  variants, census.rs:39; plus `dynamic` and `result_anchor`). `exists` uses
  the same cell lookup as the ClearValue skip. `TargetReport`, `report`, and
  `validation_same = state_same && node_same && !touched` match spec §3.1/C6.
- No new `pub` item (only `pub(crate)` additions; the one `pub fn` in the diff
  is the moved `set_dynamic_formula`), no `ooxml-core` change, xlsx-only,
  journal untouched, R1 still off, oracle 1 unchanged.
- My own mutants (applied by hand, reverted): dropping the style conjunct is
  killed by `report_two_half_conjoins_content_and_style`; formula-text-only
  comparison is killed by `report_formula_compares_text_and_attributes`.

## Implementer deviations (judged)

- `Shared` compares `v` numerically ("the spelling of `v` is not compared"):
  **reject** (B1). A10's literal-text rule exists for exactly this reason.
- `edit_cell_ext` plans through `write_with_headers`' split rather than
  `plan_cell_write`: **accept** — it preserves the existing funnel (header
  renames keep working); spec §3.2 row 1's middle column is what is stale (M2).
- `rich_state_of_runs` lives in `cell_state.rs` (outside spec §2's file list):
  **accept** — forced, since `string_state_of_item` is private there and the
  predictor must share the read side (m1, record it).
- `Formula` does not compare `cm` (spec table says "same `cm`"): **accept** —
  sound, because `write_cell` never touches `cm` on the `Formula` path, so it
  is trivially the same; staying silent is correct.
- Named+handoff collision reported once as named (`touched` vetoes
  `validation_same`): **accept** — spec-silent but safe (R1 conjoins); needs
  the test in m3a.

## Findings

### B1 (Blocker): `Shared` reports same for a non-canonically spelled `v`

- Evidence: edit.rs:324-329 compares `parse_u32(&v.text()) == Some(*i)`, and
  `parse_u32` is `trim_xml_ws(s).parse().ok()`
  (crates/ooxml-core/src/value.rs), so `v`
  of `00` (or `02`, `+2`, padded) matches `Shared(0)`. Throwaway probe
  (deleted): a cell `<c t="s"><v>00</v></c>` planned with `Shared(0)` reports
  `exists && state_same && node_same && validation_same`, while the sequential
  `edit_cell` rewrites the spelling to `0`. R2 would not heal this
  (`Value { v: "0" } != Value { v: "00" }` under the literal `CellState`
  equality), and C7 does not touch `v` spellings, so a slice-B R1 skip leaves
  bytes the sequential call rewrites. This contradicts ADR-0024 §7.1 R1(c)
  ("skips only what R2 would restore") and the spec's A10-carry-over
  conservatism (C5), which the `Number` arm honors but this arm breaks.
- Fix: compare the literal text, `v.text() == i.to_string()`, and add a
  regression test with `v` of `00` asserting `!state_same` (plus apply-keeps
  bytes check). I validated this fix in the clone: the probe flips to
  not-same and the full `-p ooxml-xlsx --lib` suite stays green.

### M1 (Major): no committed purity test for string-reservation plans

- Evidence: spec §6.2 requires `to_bytes()`-identical before/after every plan
  fn with "each op kind incl. string/style/dynamic/headers". Style (incl. a
  real `FontOnly` resolution), dynamic, and headers are covered, but no new
  test plans through `plan_string_write`/`plan_write_value(String)` with a
  reservation-bearing `Extra`, so `SharedStrings::activate` + `reserve` + the
  `plan_registries` string branch run unasserted. My throwaway probe (deleted)
  confirmed the behavior is pure (bytes + flags hold, registry activates
  invisibly), so this is a test gap, not a behavior bug — but it is exactly
  the path slice B's C7 must mirror.
- Fix: commit a test that opens a workbook with an `sst` part, runs
  `plan_string_write` (and one `plan_write_value` `String` item) through
  `plan_cell_write` + `report`, and asserts bytes-identical plus `assert_clean`
  plus the activation side effect (`registries.strings.is_some()`).

### M2 (Major): spec §3.2 row 1 names the wrong plan fn for single writes

- Evidence: `plan_cell_write` (edit.rs:555) refuses header-cell writes
  (`TableHeaderCell`, empty `headers` set), while the real funnel
  (`edit_cell_ext`, edit.rs:536, through `plan_with_headers`, tables.rs:374)
  stages the rename and applies. The new doc comment says this correctly, but
  if slice B wires single setters to `plan_cell_write` per the spec table, a
  header-cell write refuses on the Journal side while the sequential call
  renames it — a refusal-equivalence break. Nothing pins the funnel today.
- Fix: add a test that a single `edit_cell` on a header cell renames (via the
  `write_with_headers` split) while `plan_cell_write` on the same cell
  refuses, and put a slice-B note in the PR body ("plan single writes via
  `plan_with_headers`, not `plan_cell_write`").

### m1 (Minor): `cell_state.rs` is outside spec §2's file list

- Evidence: `rich_state_of_runs` (cell_state.rs:215) is report code, and §2
  says the report lives in `edit.rs` and to touch nothing else. The placement
  is justified (single source with the private `string_state_of_item`), so
  accept it — but record the deviation in the PR body per the process rules.

### m2 (Minor): `report` can `Err` on inputs the plan accepts

- Evidence: `current_cell` (edit.rs:228) calls `state_of` unconditionally, and
  `shared_text` propagates `?` (journal/state.rs:187-203): a `t="s"` target in
  a workbook whose `sst` part fails to parse makes the report `Err` where the
  plan (e.g. `StyleOnly`, which never reads `sst`) is `Ok`. Spec §3.1 says
  "`Err` only for an inconsistent plan (a bug)". The same mapping exists on
  main today (R2's `write_back` maps `state_of` errors to `Internal`,
  journal/mod.rs:509), so this is consistent with a pre-existing wart on a
  corrupt input, not a new divergence class.
- Fix: accept + note in the PR body; do not harden `shared_text` here (that
  would change R2's behavior — out of slice-A scope).

### m3 (Minor): four untested report branches

- Evidence: (a) the named+handoff collision (`touched`, edit.rs:209-210/270)
  — no batch combines a master content write with `StyleOnly` at the new
  master; (b) `DataTable` kind in `group_or_array` (the flag test covers
  Normal/Shared/Array-dict/dynamic/result only); (c) `Text`/`InlineRich` with
  escapable text (only ASCII runs are covered; `_xHHHH_` interplay with
  `item_text` is asserted for `Text` but not for rich runs); (d) two-half
  ops with a real `EnsureDate`/`WithFormat` request (only `Index` is covered).
- Fix: one test per branch (small; (a) is the load-bearing one since it pins
  the `touched` veto).

### n1 (nit): new code not rustfmt-formatted

- Evidence: the `Ok(match w {` arms in `plan_write_value`
  (worksheet.rs:691-714) keep the pre-wrap indentation; the `D2` line in the
  formula test (edit.rs:1960) runs past the file's wrap width.
- Fix: fix the indentation/wrap by hand (do not run workspace rustfmt; the
  local newer rustfmt also flags pre-existing lines, so file-wide runs would
  add noise).

### n2 (nit): `assert_clean` copied into three test modules

- Evidence: identical helpers at edit.rs:1648, tables.rs:997, dynamic.rs:292.
- Fix (optional): share one helper; fine to leave.
