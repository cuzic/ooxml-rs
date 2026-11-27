# PR review, round 1 (P1 `clear_value`)

対象: impl/xlsx-ej-p1 @ 2aa23e0
converged: yes

Reviewer: Claude (Opus). The diff is small (~210 lines), so the parent session reviewed it directly.

## Verdict
No Blocker or Major.

## Checked
- `CellOp::ClearValue` shares every plan/apply arm with `Clear` (`plan_target`, `reusable`, `write_cell`); `merge_clearing` keeps `Clear`, which creates missing cells (ADR-0014 §3).
- The missing-cell skip sits after the guards, like `RemoveFormula`'s: a merged non-anchor, table header, array anchor/result or `vm` cell refuses whether or not the cell exists, and a skipped cell creates no row, `c` or `dimension` growth (ADR-0024 §7.2 step 4 "not written").
- Same-cell writes in one batch are refused (`DuplicateCellWrite`), so the skip decided on the pre-batch index cannot hide an earlier write of the same batch.
- Shared master hand-off, member removal, dependent cache strip and calcChain removal come from the shared formula path (tests cover them).

## Minor / nit
- n1: under a `WriteIndexMap` (structural composite) the skip is not applied (`unshifted`), so a `ClearValue` there would create the cell. No caller passes `ClearValue` with an index map today; if J2 composes it with a splice, decide then.
- n2: `CellWrite` is a public enum without `#[non_exhaustive]`; the new variant breaks downstream exhaustive matches. Recorded in the PR body (pre-1.0).
