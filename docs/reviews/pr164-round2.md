# PR #164 review, round 2 (J0b-1)

対象: impl/xlsx-ej-j0b1 @ 0c43db3
converged: yes

Reviewer: Claude (Opus). Round 2 covers only the round 1 fixes (65 lines), so the parent session reviewed them directly.

## Round 1 findings
- **M1 fixed.** `replay()` sets `broken` for the whole replay and restores the previous value only on `Ok`; `recover`, `settle` and `rebuild` all go through it. A panic part-way leaves the Journal refusing with `Broken` (ADR §3.2, §9.3), which the new test `a_panic_during_replay_leaves_the_journal_broken` checks, including that the next `append` is refused.
- **m1 fixed.** `restore_failure_after_a_new_cell_recovers` kills the `tracked.clear()` mutant.
- **m2 answered in the PR body** (append cost before/after, full-width column op, mutant table).
- **n1, n2 fixed**; n3 left (a release-mode check would cost a comparison per replayed op).
- m3, m4: follow-ups (C2 `sst/@count`; P2 wiring).

## New
- n1: `rebuild()` on a Journal that is already broken restores `broken = true` after a successful replay, so the Journal stays refused. Conservative and consistent with "Broken is terminal" until J3's `truncate` defines the way out; the test's comment ("an explicit rebuild succeeds") could say that the Journal stays broken.
- n2: mixed append is about +5–7% at 10k rows on a loaded machine (noise ±8%); strings-only about +7%, explained by C8's two shared-string lookups per tracked string cell. Within noise for mixed; revisit with J1's save-cost measurement if it persists.
- Surviving mutants #6 (equivalent), #8 (redundant), #13 (unreachable) are argued; #17 (call-site use of `part_unchanged`) has only a unit test — Minor, fixed cases for a no-op on an unreadable sheet cover the path only indirectly.
