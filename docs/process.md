# Development process

This is how work on ooxml-rs is actually done. It describes current practice, not a policy
wish-list.

## Units of work

- **One PR per milestone.** A design (ADR plus its spike; occasionally a larger prototype is a separate draft PR), an implementation phase, or a single core
  primitive (e.g. `remove_part`, `insert_at`, the depth limit). Core changes needed by a format
  phase go in their own PR, so they are reviewed on their own.
- **Several agents work in parallel**, each in its own `git worktree` and branch
  (`plan/…`, `impl/…`, `core/…`, `perf/…`, `fix/…`, `docs/…`, `chore/…`, `ci/…`, `test/…`, `spike/…`). Shared checkouts are not
  switched between branches.
- **Stacked PRs** say so in their title ("stacked on #14, merge after #14").

## Who does what

- **Claude** designs (ADRs, spikes), reviews PRs adversarially and decides merges.
- **Codex** implements against Accepted ADRs, fixes review findings and resolves merge
  conflicts, from task specs written as `[codex]` Issues (new work) or `## [codex] タスク仕様`
  comments on the PR (fixes, merging main).
- **CI and scripts** run the mechanical checks.

The shared rules are in `AGENTS.md`; Claude-specific workflow is in `CLAUDE.md` and
`.claude/skills/` (`review-pr`, `merge-pr`, `codex-task`).

## Design first

- A phase starts with an **ADR** (`docs/adr/`, status Proposed) and a **spike**
  (`spikes/<area>/`): throwaway scripts or crates that measure the fixtures and real-world
  corpora (Apache POI, Open XML SDK, docx4j, template engines, LibreOffice, python-pptx test
  files). Numbers in an ADR come from a spike or a cited source.
- **Third-party files are untrusted input.** Spikes read them only (Python with `-I`,
  `zipfile` + ElementTree, no entity resolution, DTD-carrying parts skipped) and never execute
  anything downloaded.
- **Library comparisons** (python-docx, Apache POI, docx-rs, openpyxl, rust_xlsxwriter, …) cite
  source files and lines at pinned revisions.
- Claims that depend on Office behaviour that could not be checked are flagged as unverified (some ADRs use a *(u)* marker).

## Adversarial review rounds

- Every ADR, and every implementation PR from #11 onward, is reviewed by an independent Opus reviewer that tries
  to break it: probes, mutation tests, corpus scans, and the actual code of the branches it
  interacts with.
- Findings are graded **Blocker / Major / Minor**. The author fixes them in a new revision, with
  a revision-history entry mapping finding → fix, and a new round follows.
- Rounds continue until a round finds **no Blocker or Major**. The ADR is then marked Accepted,
  or the implementation PR is merged. Remaining Minors may go to a follow-up PR.
- A fix round that addresses only Minors and nits does not need another review: passing CI is
  enough. Mutation runs and corpus sweeps are read from CI or script output rather than rerun
  inside a review.
- **Review files are committed** to the reviewed PR's branch by the reviewer, so the author can
  read them, and each names the head SHA it reviewed. Merging checks that commits after that SHA
  are only finding-ID fixes, review files and merges of main. Paths: `docs/adr/NNNN-opus-review-roundK.md`
  for ADRs (ADR-0001's predate the numbering: `opus-review-roundK.md`), and `docs/reviews/prNN-roundK.md` or `<area>-impl-roundK.md` for implementation PRs.
- The project owner decides open questions; reviewers' recommendations are recorded as proposed
  answers until then.

## Verification

- **CI** (`.github/workflows/ci.yml`): `cargo test --workspace --all-features`,
  `cargo clippy --workspace --all-targets --all-features -- -D warnings`, and
  `cargo xtask codegen --check`, plus the PowerPoint reader oracle (python-pptx).
- **Oracles** from ADR-0001 (T1/T1b/T2/T3 and per-format ones) run over the fixtures under
  `reference/fixtures/`.
- **Manual Microsoft 365 checks** are done by the owner. Each format crate's `manual_check`
  example writes named outputs; `cargo xtask manual-check <outdir>` bundles all three with a checklist. There is no Office in CI, and this gap is stated
  in each ADR.
- **Recalculation** for xlsx Phase1 is checked with a manually triggered, pinned LibreOffice
  workflow (`.github/workflows/o3-recalc.yml`).

## Git conventions

- **Merge, not rebase.** Branches are brought up to date by merging `origin/main` and pushing
  normally. No force-pushes to PR branches; generated files are regenerated on conflict.
- **Small commits.** Commits written by agents carry a `Co-Authored-By` trailer.
- **Codex does not merge.** It opens and updates PRs. Claude (or the owner) merges following
  `.claude/skills/merge-pr`: base is `main`, all CI checks green, head pinned with
  `--match-head-commit`, stacked PRs retargeted to `main` before their base branch is deleted.

Note: ADRs were reviewed until converged from the start (ADR-0001, ADR-0002). The rule was
extended to implementation PRs later: the foundation PRs #6, #9 and #10 merged on CI alone, and
Phase0 implementations merged on CI were reviewed afterwards (e.g.
`docs/reviews/xlsx-phase0-impl-round*.md`).
