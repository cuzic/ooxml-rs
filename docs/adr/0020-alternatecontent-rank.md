# ADR-0020: Inserting next to `mc:AlternateContent` (the AC-rank rule)

- Status: **Draft** (revision 1)
- Date: 2026-11-22
- Depends on:
  - [ADR-0001](0001-lossless-round-trip-architecture.md): `Err ⇒ byte-identical`, T2 locality,
    fail closed.
  - [ADR-0002](0002-schema-driven-code-generation.md) §4.1 (slots, greedy assignment), §4.2
    (primitives), §4.3 (unlisted siblings and Markup Compatibility: the Phase0 refusal and the
    recorded "later rule"), §1.3 (`mce_wrapped`).
  - Core PRs #44 (move) and #49 (same-part copy): `MoveRefusal::AlternateContent`,
    `DetachRefusal::AlternateContent`.
- Unblocks: [ADR-0019](0019-pptx-phase4a-modern-comments.md) §3 (slide `p:extLst` next to an
  AC-wrapped transition), the docx roadmap Phase 4 item "ADR-0002's AC-rank rule"
  ([`docs/roadmap/docx.md`](../roadmap/docx.md)), the pptx roadmap's Phase 4 transition/timing
  writes ([`docs/roadmap/pptx.md`](../roadmap/pptx.md)), and the xlsx refusals listed in §9.
- Evidence: [`spikes/ac-rank/`](../../spikes/ac-rank/README.md) (`ac_rank.py`, output in
  `results/ac-rank.txt`): 378 xlsx, 123 docx and 102 pptx distinct packages, ranked with the
  **generated** slot tables of `main` (`d066ff1`).
- Review: none yet.

## Revision history

- **Revision 1**: initial draft.

## Context

### What ADR-0002 decided

ADR-0002 §4.3 treats a child that is in no slot as *unlisted*: it is attached to the listed
sibling before it and never decides a position. `mc:AlternateContent` (AC) cannot be treated that
way, because its branches carry listed elements. The review counterexample (ADR-0002 round 1,
M1): children `[pageMargins, drawing, legacyDrawing, AC{controls}, extLst]`; appending `picture`
(rank 32) after the AC gives `controls(34), picture(32)` once a consumer processes the AC.

So ADR-0002 made two rules:

- **Phase0 rule (in force):** any insert into a parent that has an AC child returns
  `Err(UnresolvedSibling)`. In code: `LensMut::place_skipping` refuses on the first AC child, and
  `plan_insert_at` refuses when the nearest listed neighbour is an AC or when a limited slot is
  counted (`crates/ooxml-core/src/lens.rs`).
- **"Later rule" (recorded, never designed):** an AC's rank is the minimum slot rank of the first
  listed element across its `mc:Choice`/`mc:Fallback` branches, with a diagnostic if the branches
  disagree.

### Why it matters now

Review after review hit the Phase0 refusal, or a format-level copy of it, in all three formats.
Each format crate now pre-checks for AC itself:

| Format | Where | Today |
|---|---|---|
| xlsx | `x:worksheet` with `AC{controls}`: `add_note` (P5c, PR #121 review) and the P7-pre/P7a worksheet inserts; `x:workbook` with `AC{x15ac:absPath}`: `set_autofilter` creating `definedNames` (ADR-0014, PR #58 M3: 57 refusals in the sweep), `calcPr` (ADR-0003); `Worksheet::plan_target`/`check_insertable` | `Core(UnresolvedSibling)` |
| xlsx | style tables with AC entries (`push_reserved`), shared strings, `sheetData`/`row`/`c` with AC, a wrapped `drawing`/`legacyDrawing` | `UnresolvedSibling` (index or relationship reasons, not ranks) |
| docx | `Document::append_paragraph` into a `w:body` with an AC (`sdk-HelloO14.docx`); `format_runs`/`set_run_properties` on a run that wraps a drawing (`format.rs` `run_container`); property inserts into a `w:rPr`/`w:pPr` with an AC child (`Planner::existing`) | `Core(UnresolvedSibling)` |
| pptx | `p:sld` `p:extLst` insert next to an AC-wrapped transition (ADR-0019 §3); `add_table_style` into an `a:tblStyleLst` with an AC child; theme slots and `notesMasterIdLst` wrapped in AC | `Core(UnresolvedSibling)` / `UnsupportedContent` |
| all | edits of anything **inside** a branch: pptx shapes/text (`InsideAlternateContent`), docx comment anchors and controls, xlsx G1 rewrites | format-specific |

(Functions and files are listed in §9 and under Sources.)

### Why the recorded rule is not enough

The recorded rule gives an AC one **point** rank. Three problems:

1. **A branch can hold several listed elements.** If a branch holds `[legacyDrawing(29),
   controls(34)]`, the point rank is 29, and `picture(32)` is placed after the AC: the
   counterexample again. The AC occupies an **interval** of ranks, not a point.
2. **Branches can disagree.** With `Choice{controls(34)}` and `Fallback{legacyDrawing(29)}`, the
   minimum is 29, and `picture(32)` after the AC is wrong for every consumer that takes the
   Choice. "Minimum plus a diagnostic" is unsound; the result has to hold for **every** branch,
   because the library cannot know which one a consumer takes.
3. **It says nothing about occurrence limits.** A slide whose `p:transition` exists only inside an
   AC has `max_each = 1` for `transition`. Counting direct children only, `get_or_insert` would
   write a second `p:transition` beside the AC. The corpus has 18 such slides, layouts and
   masters (below).

There is also a codegen hazard: ADR-0002 §1.3's `mce_wrapped` override **removes** the wrapped
child from the parent's particle (`xtask/src/model.rs`, `Override::MceWrapped`). If it were ever
applied (no active override uses it today), the wrapped child would become unlisted and its AC
unranked, so the rank information this ADR needs would be gone.

### What the corpus shows

`spikes/ac-rank/ac_rank.py` reads the slot tables and the `ChildOf` impls from the generated Rust,
so it ranks exactly as the lens does.

| | xlsx | docx | pptx |
|---|---:|---:|---:|
| packages read (distinct) | 378 | 123 | 102 |
| packages with an AC | 89 | 10 | 25 |
| ACs (direct children of some element) | 466 | 34 | 51 |
| of them under a parent with a generated slot table | 276 | 23 | 43 |
| ranked ACs whose branches **disagree** in rank | 0 | 0 | 0 |
| ranked ACs with a branch spanning **several slots** | 0 | 0 | 0 |
| ACs with **no** listed element in any branch | 0 | 0 | 0 |
| ACs with an **empty** branch | 171 | 2 | 0 |
| nested AC directly in a branch | 0 | 0 | 0 |
| `Requires` prefix not declared in the part | 0 | 0 | 0 |
| ACs without `mc:Fallback` | 248 | 0 | 0 |
| typed inserts at AC parents (all refused today) | 622 | 440 | 97 |
| … lifted by this ADR | **256** | **411** | **41** |
| … `AlreadyPresent` from direct children (correct refusal) | 366 | 27 | 38 |
| … `AlreadyPresent` only through a branch | 0 | 2 | **18** |
| … refused by this ADR's straddle or order checks | 0 | 0 | 0 |

"Typed inserts" are all pairs (parent instance with an AC child, typed child `C` with
`ChildOf<parent>`), simulated as an append. Wildcard-only parents (`a:graphicData`, 23 in pptx)
are counted apart.

Where the ACs are (instances):

- **xlsx:** `x:workbook` 79 (`x15ac:absPath`, Choice only); `xdr:wsDr` 5 parents holding 170 ACs
  (`xdr:twoCellAnchor`, Fallback empty); `x:worksheet` 2 (`x:controls`, Choice only); `x:fonts`,
  `x:cellXfs`, `x:dxfs` 1 each (`style-alternate-content.xlsx`); `a:p` 1. No generated spec:
  `c:chartSpace` 16, `x:controls` 2 (164 ACs, one per `x:control`), `x:oleObjects` 4, 3 Strict
  workbooks.
- **docx:** `w:r` 20 (`w:drawing` / `w:pict`), `w:body` 1 (`w:p`, Fallback empty), `w:lvl` 1,
  `pic:pic` 1. No generated spec: `c:chartSpace` 6, `wp:anchor` 4, `w:fonts` 1.
- **pptx:** `a:graphicData` 23 (`p:oleObj`), `p:sld` 12, `p:sldLayout` 4, `p:sldMaster` 2 (all
  `p:transition` in both branches), `p:pic` 1, `p:spTree` 1 (Choice `p:graphicFrame`, Fallback
  `p:pic`: same slot). No generated spec: `c:chartSpace` 8.

What the numbers say:

- In real files an AC wraps **one** element kind, and both branches agree. The interval rule
  below equals the recorded point rule on every corpus AC (0 mis-placements). The cases where they
  differ (several slots in a branch, disagreeing branches) occur only in hand-built input, and
  there the point rule is wrong.
- Counting through the branches matters: 20 inserts that would otherwise duplicate a
  `maxOccurs = 1` child (`p:transition` 18, `pic:blipFill` 1, `w:numFmt` 1).
- Empty branches are common (the 170 `xdr` anchors with `<mc:Fallback/>`), so an empty branch
  must not make an AC unrankable.
- The refusals the reviews hit are lifted by the rule alone: `definedNames` in 59 workbooks,
  `w:p` in `sdk-HelloO14.docx`, `p:extLst` on all 12 transition slides, run content in all 20
  drawing runs.

## Decision

### 1. Scope

This ADR replaces ADR-0002 §4.3's Phase0 rule and its recorded "later rule" with the rule below,
for every insert that goes through `ooxml-core`'s placement code: `insert`, `insert_by`,
`insert_in_slot`, `get_or_insert` (and their `_with_ns` forms), `insert_at`, `check_insert_at`,
and the destination placement of `move_in`/`move_in_by`. It also lists, per format, which existing
refusals may then be lifted (§9).

Out of scope, and still refused (§7): edits **inside** a branch, moves into or out of an AC, and
detaching or copying an AC.

### 2. Terms

- **Branch:** an `mc:Choice` or `mc:Fallback` child of an AC.
- **Branch member:** an element child of a branch. It is *listed* in parent `P` if it has a
  candidate slot in `P`'s slot table by ADR-0002 §4.1's `candidates` rule (named slots first,
  wildcard slots only if no slot names it). This is the rule the lens already uses for direct
  children.
- **Interval** of an AC: a pair `[lo, hi]` of slot indices (defined in §3), or *none*.
- A sibling with interval `[lo, hi]` (a listed element has `lo = hi` = its assigned slot) **may
  precede** a new child of slot `s` iff `hi ≤ s`, and **may follow** it iff `lo ≥ s`.

### 3. Rank of an AC sibling: an interval, the union over branches

The greedy left-to-right assignment of ADR-0002 §4.1 is extended. Let `cur` be the slot reached
before the AC.

1. For each branch, run the same greedy assignment over its **listed** members, starting at
   `cur`. Unlisted members are skipped, exactly as unlisted direct children are. The branch's
   interval is `[first assigned slot, last assigned slot]`, or *none* if it has no listed member.
   A member that has no candidate `≥` the slot before it makes the parent unorderable
   (`Err(UnresolvedSibling)`, as for direct children today).
2. The AC's interval is the **union** of the branch intervals that are not *none*:
   `[min lo, max hi]`.
3. The greedy assignment continues after the AC with `cur = hi`.

Why the union: whichever branch a consumer takes, the processed child list must be in schema
order. A new child placed before the AC is valid for every branch iff its slot is `≤` every
branch's `lo`; placed after, iff its slot is `≥` every branch's `hi`. The union interval states
exactly that; it is not a conservative approximation. For the same reason, the rule needs no
knowledge of which `Requires` namespaces a consumer understands.

The cases the brief asked for:

| Case | Rule | Corpus |
|---|---|---|
| Branch holding several listed elements | The branch interval covers all of them (`[29, 34]` for `[legacyDrawing, controls]`); an insert of a slot strictly inside is refused (§4, *straddle*). | 0 |
| Branches that disagree | Union; no refusal by itself (§6). | 0 of 342 ranked ACs |
| Empty branch (`<mc:Fallback/>`) | Contributes no interval: a consumer that takes it sees nothing there, which constrains nothing. | 173 |
| Unknown `Requires` namespaces | Irrelevant to the rank: every branch counts, understood or not. A `Requires` prefix that is not declared in scope is malformed MCE and refuses (§3.1). | 0 prefixes undeclared in their part |
| Nested AC directly inside a branch | Recursion: the nested AC's union interval takes part in the branch's greedy assignment like a listed member. | 0 |
| AC whose branches hold only unlisted or vendor elements | Interval *none*. **Refused** (`UnresolvedSibling`) when the insert would have to be positioned relative to it, i.e. always for an insert into that parent (Q2). | 0 |
| Same name in two slots (ADR-0002 §4.1, `w:tbl`) | The greedy assignment already picks the lowest candidate `≥ cur`; it does so inside branches too. | — |
| Wildcard slots | `candidates` applies inside branches as it does to direct children. | — |

#### 3.1 Malformed AC

The rule assumes an AC of the shape ECMA-376 Part 3 §10.2 requires. Otherwise the parent is
refused (`UnresolvedSibling`):

- an AC with no `mc:Choice`, more than one `mc:Fallback`, a `mc:Fallback` that is not last, or an
  element child that is not a branch;
- an `mc:Choice` without `Requires`, or with a `Requires` prefix that is not bound in scope.

The corpus has none of these (every AC is `Choice` or `Choice + Fallback`).

### 4. Insert position

`place_skipping` (behind `insert_by`, `insert`, `insert_in_slot`, `get_or_insert`, `move_in*`)
walks the children once, as today, treating a ranked AC as a sibling with an interval:

- The new child goes before the first sibling that **may not precede** it (`hi > s`), after
  running the caller's `before` callback over the same-slot listed siblings as today. If that
  sibling also **may not follow** it (`lo < s`), the insert is refused: `Err(UnresolvedSibling)`
  (*straddle*). With `lo = hi`, straddle is impossible; it needs a multi-slot branch or
  disagreeing branches.
- An AC with `lo = hi = s` is a member of the new child's slot. Since a slot's names may appear
  in any order (ADR-0002 §4.1), the new child may go on either side of it. It is **not** passed to
  the `before` callback, and it does not count in `index_in_slot`: callbacks are written against
  real `C` siblings (row and column numbers, ids), and every existing callback would have to
  learn to skip an AC (Q4). An append therefore lands after it; a callback that returns `true` for
  a sibling after the AC puts the new child before that sibling, after the AC.
- An AC with interval *none* is refused (Q2).
- Whitespace and dirty duties are ADR-0002 §4.2's: no whitespace is added, the AC is not touched,
  so the byte diff is the new element at the insertion point (T2).

`insert_at` / `check_insert_at` keep their local check, with an AC neighbour read through its
interval: the nearest ranked sibling before `index` must satisfy `hi ≤ s`, the nearest after
must satisfy `lo ≥ s`. "Nearest" skips unlisted children as today; an unranked AC refuses.

### 5. Occurrence limits: count through the branches

For `max_each` and `max_total`, an AC contributes, for the new child's slot `s`, the **maximum
over its branches** of the members assigned to `s` (and, for `max_each`, of the members with the
new child's QName); a nested AC contributes recursively. Both branches are always consulted.

The maximum, not the sum, because a consumer sees one branch; not the Choice alone, because a
consumer may take the Fallback, which may hold more. An element inside a branch therefore
**does** count toward the parent's limits. The result is `Err(AlreadyPresent(q))`, as for a
direct child. `get_or_insert` still returns only a **direct** existing child; when the child
exists only inside an AC, it returns `AlreadyPresent` instead of writing a duplicate (the 18
transition cases). Reading through the AC is the format crates' job (ADR-0008's branch choice).

### 6. Disagreeing branches: no refusal, no runtime diagnostic

ADR-0002 asked for "a diagnostic if branches disagree". This ADR drops it:

- The union is exact (§3), so disagreement only **widens** the interval. Inserts outside it are
  correct for every branch; inserts inside it are refused as *straddle*.
- `ooxml-core` has no logging channel and no warning type. A warning that callers cannot act on
  would be API surface with no use; the measurable question ("how often do branches disagree")
  is answered by the spike (0 of 342) and can be re-run.
- Refusing every parent whose AC branches disagree (alternative A5) would refuse inserts that are
  provably safe.

### 7. What stays refused

| Operation | Today | After this ADR | Why |
|---|---|---|---|
| Insert, remove or edit **inside** a branch (any depth) | Unreachable through the typed lens (no `ChildOf` for branches); format crates refuse (`InsideAlternateContent`, `UnsupportedContext`, `UnsupportedControl`, …) | **Unchanged** | One branch edited alone de-synchronises the others; editing all branches consistently means mapping between different markups (`wps` vs VML, `p14` vs base transitions), which is per-format semantics, not ranking. A format ADR may decide a both-branch edit for a specific element, as ADR-0009's P3 note did for `controls` attribute rewrites. |
| Move into or out of an AC (`MoveRefusal::AlternateContent`) | Refused | **Unchanged** | Same reason. A move **between siblings of an AC** is an ordinary insert at the destination and follows §4. |
| Detach / copy a subtree that contains an AC, or an AC itself (`DetachRefusal::AlternateContent`) | Refused | **Unchanged** | Copying both branches with their `Requires`/`Ignorable` bindings is a separate design (#49). |
| Insert that straddles a ranked AC (§4) | Refused (any AC) | Refused | No position is valid for every branch. |
| Insert into a parent with an unranked or malformed AC (§3, §3.1) | Refused | Refused (Q2) | Fail closed. |
| Removing a whole AC child (`remove_at`) | Allowed | **Unchanged** | No ordering question. |
| Format pre-checks for reasons other than rank (index-stable tables, relationship targets inside AC, cell grids in AC) | Refused | **Unchanged** unless §9 says otherwise | Not a ranking problem. |

### 8. API impact

- **Core only, for the rule.** The change is internal to `LensMut::place_skipping`,
  `plan_insert_at` and the helpers they share. No public signature changes, no new public item is
  required.
- **Errors.** The refusals keep `Error::UnresolvedSibling(parent)` (Q3); its doc comment and
  message change from "mc:AlternateContent sibling or out-of-order children" to "out-of-order
  children, or an `mc:AlternateContent` sibling whose branches allow no position".
  `AlreadyPresent` gains the branch-count cause. `Error` is `#[non_exhaustive]`, so a later
  dedicated variant stays possible.
- **Generated code.** Nothing changes: `cargo xtask codegen --check` is clean, no generated public
  name changes. ADR-0002 §1.3's `mce_wrapped` is **redefined** for future use: it removes the
  typed insert (`ChildOf` impl and the `get_or_insert_*`/`insert_*` accessors) for the wrapped
  child but **keeps the name in its slot**, so the slot table still ranks the AC that wraps it.
  No active override uses `mce_wrapped`, so this changes no generated file today (Q5).
- **Format crates.** The rule lifts nothing by itself where a format crate pre-checks; each crate
  removes or narrows its own pre-check (§9). Generated names are untouched there too.

### 9. Which refusals each format can lift

"Lift" means: the format crate removes its AC pre-check for that operation and relies on the
core rule; the core then refuses only straddle, unranked/malformed AC, and limits.

| Format | Operation (refusal today) | Decision | Corpus / fixture |
|---|---|---|---|
| xlsx | Worksheet-level inserts next to `AC{controls}` (`plan_target`, `check_insertable`; `add_note`'s `legacyDrawing` (P5c); P7-pre/P7a worksheet inserts) | **Lift.** `check_insertable` stops copying the core refusal and calls the core check. | 2 POI sheets; `reference/fixtures/xlsx/poi/62629_target.xlsm` |
| xlsx | `x:workbook` inserts next to `AC{x15ac:absPath}`: `definedNames` (`set_autofilter`, ADR-0014), `calcPr` (ADR-0003) | **Lift.** | 59 workbooks gain `definedNames`; 13 fixtures |
| xlsx | Style tables with AC entries (`push_reserved`), shared strings with AC | **Stays.** Index semantics (an AC is one entry only when index-stable, ADR-0006), not rank. A follow-up may allow appends after an index-stable AC (Q6). | `style-alternate-content.xlsx` |
| xlsx | `sheetData`/`row`/`c` containing AC; a `drawing`/`legacyDrawing` wrapped in AC; G1/G2 rewrites (ADR-0018) | **Stays.** Edits inside branches or grid indexes. | 0 grids with AC |
| xlsx | `xdr:wsDr` anchor append (raw splice before `</xdr:wsDr>`, ADR-0017) | **No change needed**; it may move to the lens later (anchors next to `AC{twoCellAnchor}` are same-slot, `ok` in all 5 parents). | 170 ACs |
| docx | `Document::append_paragraph` into a `w:body` with an AC | **Lift** (it already relies on the core; nothing to remove). | `sdk-HelloO14.docx` |
| docx | `format_runs`/`set_run_properties` on a run that wraps a drawing (`run_container`) | **Lift.** `w:rPr` is slot 0 of `w:r`; the AC is in the content slot. | 20 runs, 6 files |
| docx | Property inserts into a `w:rPr`/`w:pPr` with an AC child (`Planner::existing`) | **Lift.** | 0 in corpus; synthetic test |
| docx | Comment anchors, content controls and fields inside AC | **Stays** (inside a branch). | — |
| pptx | `p:sld` `p:extLst` insert next to an AC-wrapped transition (ADR-0019 §3) | **Lifted by the core** (ADR-0019 already goes through the lens). | 12 slides, 7 files; `bug68703.pptx` |
| pptx | `p:timing` insert next to an AC-wrapped transition (roadmap Phase 4) | **Allowed by the core**; the operation itself is Phase 4 work. | 9 ok, 3 present |
| pptx | `p:transition` insert where the transition exists only inside an AC | **`AlreadyPresent`** (was `UnresolvedSibling`); writing a transition is Phase 4 and edits both branches. | 18 |
| pptx | `add_table_style` into an `a:tblStyleLst` with an AC child | **Lift**, with the `styleId` uniqueness check extended to branch members. | 0 in corpus; synthetic test |
| pptx | Shapes, text, formatting, links, notes body inside AC (`InsideAlternateContent`); theme slots or values wrapped in AC; `notesMasterIdLst` wrapped in AC (the new `p:notesMasterId` would go inside the branch) | **Stays** (inside a branch). | — |

### 10. Invariants

- **`Err ⇒ byte-identical`.** Every new refusal is decided in the planning walk, before any node
  is created, as today. Format crates that drop a pre-check keep their plan → validate → apply
  order; their existing refusal tests stay, with AC cases moved from "refused" to "applied"
  where §9 lifts them.
- **T2 locality.** The AC is never touched; the diff is the inserted element (ADR-0002 §4.2).
- **Fail closed.** Unranked, malformed and straddled ACs refuse; branches are counted for limits.

## Test oracles

1. **Core unit tests**, inline XML, for each row of §3's table and §3.1: single-slot AC before,
   after and in the new child's slot; a branch with two slots (insert below, at `lo`, strictly
   inside, at `hi`, above); disagreeing branches (the §Context example); an empty Fallback; a
   nested AC; an AC with only vendor elements; each malformed shape; same-name-in-two-slots
   inside a branch. Each `insert_by`, `insert_at` and `check_insert_at` result is compared with a
   brute-force oracle: for **every** branch choice, substitute the branch, check the processed
   child list with ADR-0002 §4.1's greedy assignment and the limits. The rule must accept iff
   every substitution is valid (and the position must be one such that it is).
2. **Limits:** `get_or_insert::<Transition>` on `bug68703.pptx` slides returns `AlreadyPresent`;
   `max_total` counted through branches with branch sizes 0/1/2.
3. **`Err ⇒ byte-identical`** for each refusal: straddle, unranked, malformed, limit through
   a branch.
4. **Corpus sweep** (CI or the spike): re-run `ac_rank.py`; the Rust implementation's outcome for
   every (parent, typed child) pair in `results/ac-rank.txt` equals the spike's.
5. **Mutation** (AGENTS.md): at least (a) "point rank = min lo" instead of the interval, (b)
   ignore branches for limits, (c) treat an AC that has an empty branch as unranked; each must
   fail a test.
6. **Per-format lifts:** for every §9 "Lift" row, a test that succeeds on the named fixture and
   whose output reopens and round-trips (the existing oracles: python-pptx for pptx, docx-rs /
   python-docx readback for docx, the xlsx readers), plus a straddle or limit case that still
   refuses byte-identically.

## Delivery

Each unit is one PR, reviewed until converged (no Blocker/Major), in order. U2–U4 are
independent of each other after U1.

0. **U0: this ADR and the spike.** Docs and `spikes/ac-rank/` only.
   - **Acceptance:** review converged; owner answers to the open questions recorded; ADR-0002
     §4.3 gets a note pointing here (added when this ADR is Accepted).
1. **U1: core rule.** `place_skipping`, `plan_insert_at`, the interval/branch helpers, the
   malformed-AC check, the error doc changes; ADR-0002 §4.3 note; the `mce_wrapped`
   redefinition in `xtask` (keep the slot name, drop the typed insert) with its codegen unit test.
   - **Acceptance:** AGENTS.md's definition of done; oracles 1–3 and 5; oracle 4 on the committed
     fixtures; `cargo xtask codegen --check` clean with **no** generated diff; the existing AC
     refusal tests that assert the old blanket refusal are updated case by case, each change
     named in the PR body; the python-pptx oracle green.
2. **U2: xlsx lifts** (§9 xlsx "Lift" rows).
   - **Acceptance:** `add_note` on `62629_target.xlsm`, `set_autofilter` creating `definedNames`
     on a workbook with `AC{absPath}`, a `calcPr` insert; `check_insertable` delegates to the core;
     byte-identical refusal tests for the rows that stay; the ADR-0003/0014/0017 notes updated.
3. **U3: docx lifts** (§9 docx).
   - **Acceptance:** `append_paragraph` on `sdk-HelloO14.docx` succeeds and reads back; run
     formatting on the drawing runs of `sdk-complex2010.docx`/`poi-shapes-with-text.docx`;
     property inserts next to a synthetic AC in `w:rPr`; the docx oracle job green; ADR-0004/0007
     notes updated.
4. **U4: pptx lifts** (§9 pptx).
   - **Acceptance:** ADR-0019's `p:extLst` insert on `bug68703.pptx` slides (if P4a-1 has not
     already landed it); `add_table_style` next to a synthetic AC with the extended `styleId`
     check; `p:transition` insert → `AlreadyPresent`; python-pptx oracle green; ADR-0016/0019
     notes updated.

## Alternatives considered

- **A1. Keep the blanket refusal.** Simple and safe, but it refuses 708 typed inserts in the
  corpus that are provably valid, and every format grows its own pre-checks (it already has).
- **A2. The recorded point rule** (minimum first-listed rank). Equal to this ADR's rule on every corpus AC, but
  wrong for multi-slot branches and disagreeing branches (§Context), and silent about limits. The
  interval costs the same walk.
- **A3. Rank by one branch** (the Choice, or the first branch whose `Requires` the library
  "understands", as ADR-0008 reads). Valid only for consumers that make the same choice; the
  library writes files for every consumer.
- **A4. Treat an AC as unlisted** (attached to the previous sibling). This is the counterexample
  ADR-0002 round 1 M1 refuted.
- **A5. Refuse when branches disagree.** Sound but refuses safe inserts; the union already makes
  disagreement safe (§6). Zero corpus cases either way.
- **A6. Process MCE on load** (pick a branch and drop the rest), or expand the AC into the parent.
  Lossy; violates ADR-0001.
- **A7. Ask the caller where to put the new child relative to each AC** (an explicit
  `before_ac`/`after_ac` argument). Pushes schema knowledge to every caller; the slot table
  already decides it.

## Consequences

- Positive: 708 corpus inserts lifted across the three formats, including every refusal the
  reviews of this week hit that is about **position** (`definedNames`, `legacyDrawing`, `w:p`,
  run properties, slide `p:extLst`). Limits become correct next to AC (18 transitions no longer
  at risk of duplication once Phase 4 writes them).
- Positive: one rule in the core replaces several per-format pre-checks; new format work no longer
  needs its own AC refusal for positioning.
- Negative: placement now inspects branch contents. The cost is O(size of the AC's branches) per
  AC per insert, and an `xdr:wsDr` can hold 170 ACs. Appends of unlimited slots via `insert_at`
  stay O(1) apart from the nearest neighbours.
- Negative: the `before` callback does not see same-slot ACs (Q4), so a caller cannot put a new
  child "just before this AC". No current caller needs it.
- Neutral: edits inside branches stay refused; the formats keep those refusals and their error
  variants.

## Open questions for the project owner

1. **Q1. Disagreeing branches: widen or refuse?** Use the union interval and refuse only a
   straddling insert (§6), or refuse every insert into a parent whose AC branches disagree (A5).
   **Recommendation:** widen. The union is exact, so it accepts only inserts that are valid for
   every branch; the corpus has 0 disagreeing ACs, so the choice only affects hand-made input.
2. **Q2. An AC whose branches hold no listed element: refuse or treat as unlisted?** Treating it
   as unlisted matches ADR-0002's handling of a direct vendor child; refusing is fail-closed.
   **Recommendation:** refuse. It costs nothing measured (0 in 603 packages), and such an AC
   hides content whose position the library cannot know. Relax later with a fixture if one
   appears.
3. **Q3. Error variant: reuse `UnresolvedSibling` or add one?** A new variant
   (`AlternateContentOrder { parent }`) would tell callers *why*; reusing keeps format-crate
   `match` arms and documented refusals unchanged. **Recommendation:** reuse
   `UnresolvedSibling`, with the doc change of §8; add a variant only if a format needs to tell
   the cases apart.
4. **Q4. Should the `before` callback see a same-slot AC?** **Recommendation:** no (§4). All
   current callbacks compare attributes of real `C` siblings; passing an AC would change their
   behaviour silently. If a need appears, add a separate `insert_by_with_ac` later.
5. **Q5. Redefine `mce_wrapped` (keep the rank, drop the typed insert)?** The current definition
   would erase exactly the information this ADR uses. **Recommendation:** yes, in U1; it changes
   no generated file today.
6. **Q6. Style-table appends next to an index-stable AC (xlsx).** ADR-0006 already treats an
   index-stable AC as one entry; appending after it is now position-safe. Lift in U2, or leave
   for a separate ADR-0006 note? **Recommendation:** leave it out of this ADR: it changes index
   arithmetic (`count`, `xfId` numbering), not ranks, and the corpus has one such file.
7. **Q7. Order of U2–U4.** **Recommendation:** U4 (pptx) first if ADR-0019's P4a-1 is in flight,
   since it depends on the rule; otherwise U2 (xlsx), which has the most corpus refusals (59
   `definedNames`, the P5c `add_note` case).

## Sources

- ECMA-376 Part 3 (Markup Compatibility and Extensibility), §10.2 (`AlternateContent`,
  `Choice`, `Fallback`, `Requires`).
- ADR-0002 §4.1–§4.3 and its review round 1 (M1) and round 2 (MC namespace in core).
- `crates/ooxml-core/src/lens.rs` (`place_skipping`, `plan_insert_at`),
  `crates/ooxml-core/src/lens/move_subtree.rs`, `crates/ooxml-core/src/lens/same_part.rs`,
  `crates/ooxml-core/src/error.rs`; `xtask/src/model.rs` (`Override::MceWrapped`).
- Format pre-checks: `crates/ooxml-xlsx/src/worksheet.rs` (`check_insertable`, `plan_target`),
  `crates/ooxml-docx/src/format.rs` (`Planner::existing`, `run_container`),
  `crates/ooxml-docx/src/document.rs` (`append_paragraph`),
  `crates/ooxml-pptx/src/write/table_style.rs`, `crates/ooxml-pptx/src/slides/notes.rs`,
  `crates/ooxml-pptx/src/write/theme.rs`.
- [`spikes/ac-rank/`](../../spikes/ac-rank/README.md) and `results/ac-rank.txt`.
