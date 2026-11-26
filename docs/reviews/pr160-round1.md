# PR #160 review, round 1 (C5 `dimension` rule)

対象: impl/xlsx-ej-c5 @ b6bba6b
converged: yes

Reviewer: Claude (Opus). The diff is small (147 lines), so the parent session reviewed it directly.

## Verdict
No Blocker or Major. Converged.

## Checked
- The only cell-write site that grows `dimension` is `write_cell` applying `TargetPlan::dimension`, reached only from the apply loop in `edit.rs` (the single `plan_target` → `write_cell` call). `plan_target` at the plan stage (`edit.rs` L437) and again at apply still parses `dimension/@ref`, so a malformed ref refuses with `Error::Malformed` whether or not the flag is set (oracle 1 (i)).
- Structural ops (`sheet_ops/grid/features.rs`) do not read the flag, so ADR-0018 §3.2 rewriting is unchanged; test compares frozen vs unfrozen bytes.
- `grow_dimension`: no element → `Ok` with nothing created; `grown == range` → no `set_reference`, so the Base bytes stay (mutant (d) is detected thanks to the non-canonical `a1:f3` Base ref). Matches §7.2 step 4.
- Projection P already drops `dimension` (`journal/project.rs`), consistent with Delivery 0 (save-side predicate in J1).

## Minor / nit
- n1: `grow_dimension` takes a `usize` sheet index like `canonicalize_cells`; J1's Commit will map `SheetId` to it. No change needed.
