# Architecture decision records

ADRs are proposed and reviewed in PRs. A review round's findings are committed next to the ADR
(`NNNN-opus-review-roundK.md`), and an ADR is marked Accepted once a round finds no Blocker or
Major issue. See [`docs/process.md`](../process.md).

The status column is the status **on `main`** as of 2026-11-16. ADRs that are still in review
live only on their PR branch until it merges.

| ADR | Title | Status on `main` | Reviews |
|---|---|---|---|
| [0001](0001-lossless-round-trip-architecture.md) | Lossless round-trip architecture for OOXML read/write/edit | **Accepted** (rev 3) | [r1](opus-review-round1.md), [r2](opus-review-round2.md), [r3](opus-review-round3.md) |
| [0002](0002-schema-driven-code-generation.md) | Schema-driven code generation for the typed lens | **Accepted** (rev 5) | [r1](0002-opus-review-round1.md), [r2](0002-opus-review-round2.md), [r3](0002-opus-review-round3.md), [r4](0002-opus-review-round4.md) |
| [0003](0003-xlsx-phase1-formula-and-calc-state.md) | xlsx Phase1: formula-aware edits and calculation state | Proposed (rev 2); rev 3 and its implementation are in [#20](https://github.com/cuzic/ooxml-rs/pull/20) | [r1](0003-opus-review-round1.md); later rounds in #20 |
| [0004](0004-docx-phase0.md) | Word Phase0: text edits and paragraph append | **Accepted** (rev 4); Minor follow-ups in [#34](https://github.com/cuzic/ooxml-rs/pull/34) | [r1](0004-opus-review-round1.md), [r2](0004-opus-review-round2.md), [r3](0004-opus-review-round3.md), [r4](0004-opus-review-round4.md) |
| [0005](0005-pptx-phase0.md) | PowerPoint Phase 0: text edits in existing shapes | **Accepted** (rev 3) | [r1](0005-opus-review-round1.md), [r2](0005-opus-review-round2.md), [r3](0005-opus-review-round3.md) |
| [0006](0006-xlsx-phase2-shared-strings-and-styles.md) | xlsx Phase2: shared-string and style registries | **Accepted** (rev 3) | [r1](0006-opus-review-round1.md), [r2](0006-opus-review-round2.md) |
| 0007 | Word Phase1: template filling | not on `main`: Proposed in [#25](https://github.com/cuzic/ooxml-rs/pull/25) | in the PR |
| 0008 | PowerPoint Phase 1: read the whole deck | not on `main`: Proposed in [#28](https://github.com/cuzic/ooxml-rs/pull/28) | in the PR |
| 0009 | xlsx Phase3: sheet operations on existing files | not on `main`: Proposed in [#27](https://github.com/cuzic/ooxml-rs/pull/27) | in the PR |
| 0010 | Core performance: compact Layer 2 tree, deferred subtrees, save and read paths | not on `main`: Proposed in [#32](https://github.com/cuzic/ooxml-rs/pull/32) | in the PR |
| 0011 | Word Phase2: structure (new parts, new IDs, append-only registries) | not on `main`: Proposed in [#31](https://github.com/cuzic/ooxml-rs/pull/31) | in the PR |
| 0012 | PowerPoint Phase 2: slides and shapes as units | not on `main`: Proposed in [#30](https://github.com/cuzic/ooxml-rs/pull/30) | in the PR |
| 0013 | PowerPoint Phase 1b: table structure and SmartArt text | not on `main`: Proposed in [#33](https://github.com/cuzic/ooxml-rs/pull/33) | in the PR |
| 0014 | xlsx Phase4: range features on existing worksheets | not on `main`: Proposed in [#35](https://github.com/cuzic/ooxml-rs/pull/35) | in the PR |
| 0015 | Word Phase3: review features | not on `main`: Proposed in [#39](https://github.com/cuzic/ooxml-rs/pull/39) | in the PR |
| 0016 | PowerPoint Phase 3: formatting writes and links | not on `main`: Proposed in [#37](https://github.com/cuzic/ooxml-rs/pull/37) | in the PR |

Reviews of implementation and core PRs that have no ADR of their own are in
[`docs/reviews/`](../reviews/), for example `pr16-round*.md` (core depth limit),
`pr21-round*.md` (O3 workflow), `pr24-round*.md` (benchmark) and `xlsx-phase0-impl-round*.md`.
