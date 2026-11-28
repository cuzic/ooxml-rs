# PR #171 review round 2 (Opus via herdr-cocla)

- head: d60f71a
- verdict: YES (all fixed; n3 nit only)
- job: 01a21f4a-ec7f-7945-8ef9-5cce4d25ede2

# PR #171 review round 2 @ d60f71a (e1acbe3..d60f71a)

## Conclusion
converged: YES. M1 and m1–m3 are all fixed, and so are n1 and n2. No new Blocker or Major.

## Verification
- Exported the PR tree with git archive (probe directory deleted afterwards).
- Error tests in core/docx/pptx: OK. xlsx lib: 339 passed. phase2, phase2_part2, phase3_smoke, phase4 and phase5d: all OK.
- Mutation n1, `PackageTooLarge` → "Package": killed by the new check (core error.rs:549).
- Mutation m2, dropping the prefix from `set_iso_date`: killed (phase2.rs:679).

## Status of round-1 findings
- **M1, FIXED.** The four sites now return `InvalidArgument{cell:None}`:
  - `sheet_ops/mod.rs:248` (`set_defined_names` scope);
  - `:727` (reserved sheetId);
  - `core_props.rs:352` (custom property validation);
  - `core_props.rs:325` (already exists).

  `caller_scope_and_id_rejections_change_nothing` (phase3_smoke) covers `set_defined_name`, which calls `set_defined_names`, and sheetId 0/1. The core_props unit test covers an empty name, a duplicate name, an unknown type, a bad i4 and the already-exists case. All pin `cell()`=None and byte-identical output.

  Re-audit: I checked all 82 remaining `Malformed` sites. They are file decoding, internal invariants (`plan_writes_after`, `canonicalize_cells`, `cell_spot`), exhaustion (sheetId, si, custom property count, numFmtId) or the private journal module. I found no further public caller-input site. wasm and JS do not depend on the moved variants or messages.
- **m1, FIXED.** The `NameInUse` arm of `cell()` returns None and the samples were updated to match. The other 11 variants that carry a cell are all built from a `CellIndex`, so their claim to hold an A1 cell holds.
- **m2, FIXED.** Every constructor with `Some(cell)` (dynamic×2, setters×4, streaming×4) uses `format!("cell {at}: …")`. The prefix is pinned in phase2.rs:679 and the mutation is killed.
- **m3, FIXED.** phase4 `reversed_shared_formula_ranges_are_rejected` uses try_on, which checks the bytes. The streaming unit test covers no runs and an empty run; both go through `prepare_rich_text`. The public `set_rich_text(&[])` has no direct pin, but it runs the same function and the probe confirmed it in round 1.
- **n1, FIXED.** All four crates check that the character after the kind does not continue an identifier. The mutation is killed.
- **n2, FIXED.** The PR body now gives 21 sites and lists them by category.

## New findings
- **n3 (nit):** the cell prefix is a convention each constructor must follow (documented only in the `message` doc comment). A future `Some(cell)` constructor can forget it, and only `set_iso_date` pins it. Moving it into `#[error]` (for example `#[error("{}{message}", cell.as_deref().map(|c| format!("cell {c}: ")).unwrap_or_default())]`) would make it structural. Optional.

## Verdict
YES. CI passing is enough to merge.
