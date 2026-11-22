# ADR-0020: Inserting next to `mc:AlternateContent` (the AC-rank rule)

- Status: **Draft** (revision 2)
- Date: 2026-11-22
- Depends on:
  - [ADR-0001](0001-lossless-round-trip-architecture.md): `Err ⇒ byte-identical`, T2 locality,
    fail closed.
  - [ADR-0002](0002-schema-driven-code-generation.md) §4.1 (slots, greedy assignment), §4.2
    (primitives), §4.3 (unlisted siblings and Markup Compatibility: the Phase0 refusal and the
    recorded "later rule"), §1.3 (`mce_wrapped`).
  - Core PRs #44 (move) and #49 (same-part copy): `MoveRefusal::AlternateContent`,
    `DetachRefusal::AlternateContent`.
- Interacts with: open PR #144 (ADR-0019 P4a-1), which adds its own AC placement to the core
  (§8.1).
- Unblocks: [ADR-0019](0019-pptx-phase4a-modern-comments.md) §3 (slide `p:extLst` next to an
  AC-wrapped transition), the docx roadmap Phase 4 item "ADR-0002's AC-rank rule"
  ([`docs/roadmap/docx.md`](../roadmap/docx.md)), the pptx roadmap's Phase 4 transition/timing
  writes ([`docs/roadmap/pptx.md`](../roadmap/pptx.md)), and the refusals listed in §9.
- Evidence: [`spikes/ac-rank/`](../../spikes/ac-rank/README.md) (`ac_rank.py`, output in
  `results/ac-rank.txt`): 378 xlsx, 123 docx and 102 pptx distinct packages, ranked with the
  **generated** slot tables of `main` (`d066ff1`).
- Review: [round 1](0020-opus-review-round1.md) (not converged: M1–M4, m1–m5).

## Revision history

- **Revision 1**: initial draft.
- **Revision 2** (review round 1; coordinator decisions of 2026-11-22). Each fix is in its
  section.
  - **M1:** inserts that take a `before` callback or a positional index (`insert_by`,
    `insert_in_slot`, `insert_at`, `move_in_by` and their `check_*`/`_with_ns` forms) **refuse**
    when an AC's interval contains the new child's slot. Only a plain append (`insert`,
    `get_or_insert`, `move_in`, `check_insert`, `check_get_or_insert`) may land next to such an
    AC. §4.4 lists the existing callers; U1 tests each. This replaces revision 1's Q4.
  - **M2:** a format check that looks up an *existing direct child* must also look in the
    branches before its AC pre-check is removed. docx property Clear and attribute removal for a
    property that exists inside a branch stay refused (§9).
  - **M3:** §8.1 settles PR #144: U1 is the one core mechanism, #144 adds no public core API and
    is reworked onto U1. Delivery order U1 → #144 rework → U4 → U2 → U3. This replaces revision
    1's Q7.
  - **M4:** a non-empty branch with no listed member makes the AC **unranked** (refused); a branch
    that starts with unlisted members starts at the slot of the sibling before the AC (§3).
  - **m1:** how `insert_at` reads an AC neighbour, and that an unranked, malformed or same-slot AC
    anywhere in the parent refuses it (§4.3).
  - **m2:** "exact" is per AC; correlated `Requires` across sibling ACs are over-refused, safely
    (§3.2; 6 corpus parents).
  - **m3:** the spike now refuses unranked and malformed ACs, checks `Requires` binding in scope,
    and counts the new cases; counts re-run.
  - **m4:** `check_insertable` (xlsx, child lists not yet in the tree) delegates to a new public
    core function, named in §8; the shared-strings refusal keeps its own guard (§9).
  - **m5:** `mce_wrapped` redefinition restricted to children Office never writes directly
    (§8, Q5).

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
| docx | `Document::append_paragraph` into a `w:body` with an AC (`sdk-HelloO14.docx`); `format_runs`/`set_run_properties` on a run that wraps a drawing (`format.rs` `run_container`); property edits in a `w:rPr`/`w:pPr` with an AC child (`Planner::existing`) | `Core(UnresolvedSibling)` |
| pptx | `p:sld` `p:extLst` insert next to an AC-wrapped transition (ADR-0019 §3); `add_table_style` into an `a:tblStyleLst` with an AC child; theme slots and `notesMasterIdLst` wrapped in AC | `Core(UnresolvedSibling)` / `UnsupportedContent` |
| all | edits of anything **inside** a branch: pptx shapes/text (`InsideAlternateContent`), docx comment anchors and controls, xlsx G1 rewrites | format-specific |

(Functions and files are listed in §4.4, §9 and under Sources.)

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

Two more things the recorded rule does not address:

- **Callers that count positions.** Several format callers compute a position within a slot by
  counting siblings, some including an AC and some not (§4.4). Any rule that lets such an insert
  land next to an AC in the same slot can silently move content (review M1).
- **`mce_wrapped` erases the rank.** ADR-0002 §1.3's override **removes** the wrapped child from
  the parent's particle (`xtask/src/model.rs`, `Override::MceWrapped`). Applied (no active
  override uses it today), the wrapped child would become unlisted and its AC unranked.

### What the corpus shows

`spikes/ac-rank/ac_rank.py` reads the slot tables and the `ChildOf` impls from the generated Rust,
so it ranks exactly as the lens does, and simulates this ADR's rule (revision 2) for every typed
insert at a parent with an AC.

| | xlsx | docx | pptx |
|---|---:|---:|---:|
| packages read (distinct) | 378 | 123 | 102 |
| packages with an AC | 89 | 10 | 25 |
| ACs (direct children of some element) | 466 | 34 | 51 |
| of them under a parent with a generated slot table | 276 | 23 | 43 |
| ranked ACs whose branches **disagree** in rank | 0 | 0 | 0 |
| ranked ACs with a branch spanning **several slots** | 0 | 0 | 0 |
| malformed ACs (§3.1), incl. `Requires` prefix not bound in scope | 0 | 0 | 0 |
| non-empty branches with no listed member (unranked AC) | 0 | 0 | 0 |
| branches that start with an unlisted member | 0 | 0 | 0 |
| ACs with an **empty** branch | 171 | 2 | 0 |
| ACs without `mc:Fallback` (an implicit empty branch) | 248 | 0 | 0 |
| nested AC directly in a branch | 0 | 0 | 0 |
| parents with ≥ 2 ACs sharing a `Requires` set (§3.2) | 6 | 0 | 0 |
| typed inserts at AC parents (all refused today) | 622 | 440 | 97 |
| … placed by a **plain append** under this ADR | **256** | **411** | **41** |
| … of those, also by callback/positional forms (no AC in the slot) | 235 | 0 | 35 |
| … `AlreadyPresent` from direct children (correct refusal) | 366 | 27 | 38 |
| … `AlreadyPresent` only through a branch | 0 | 2 | **18** |
| … refused as straddle, disorder, unranked or malformed | 0 | 0 | 0 |

"Typed inserts" are all pairs (parent instance with an AC child, typed child `C` with
`ChildOf<parent>`). Wildcard-only parents (`a:graphicData`, 23 in pptx) are counted apart.

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

- In real files an AC wraps **one** element kind, both branches agree, and every member is
  listed. The interval rule equals the recorded point rule on every corpus AC (0 mis-placements);
  where they differ (several slots in a branch, disagreeing branches) only hand-built input
  reaches, and there the point rule is wrong. The fail-closed cases (malformed, unranked, leading
  unlisted members) cost nothing measured.
- Counting through the branches matters: 20 inserts that would otherwise duplicate a
  `maxOccurs = 1` child (`p:transition` 18, `pic:blipFill` 1, `w:numFmt` 1).
- Empty branches are common (the 170 `xdr` anchors with `<mc:Fallback/>`), so an empty branch
  must not make an AC unrankable.
- **Same-slot ACs are where M1 bites:** every docx case (run content next to a wrapped drawing,
  body content next to a wrapped paragraph), the `p:spTree` case, and 21 xlsx (anchors, style
  entries, `a:p`). There only a plain append lands; positional inserts stay refused.
- The refusals the reviews hit are lifted: `definedNames` in 59 workbooks, `append_paragraph` on
  `sdk-HelloO14.docx` (a plain append), and the slide `p:extLst` for comments on all 12 slides
  (another slot than the AC).

## Decision

### 1. Scope

This ADR replaces ADR-0002 §4.3's Phase0 rule and its recorded "later rule" with the rule below,
for every insert that goes through `ooxml-core`'s placement code (§4), and it is the **only** AC
placement mechanism in the core (§8.1). It also lists, per format, which existing refusals may
then be lifted (§9).

Out of scope, and still refused (§7): edits **inside** a branch, moves into or out of an AC, and
detaching or copying an AC.

### 2. Terms

- **Branch:** an `mc:Choice` or `mc:Fallback` child of an AC. An AC without `mc:Fallback` is
  read as having an implicit **empty** Fallback.
- **Member:** an element child of a branch (comments, PIs and text in a branch are ignored, as
  they are among direct children). A member is *listed* in parent `P` if it has a candidate slot
  in `P`'s slot table by ADR-0002 §4.1's `candidates` rule (named slots first, wildcard slots only
  if no slot names it): the rule the lens already applies to direct children.
- **`cur`:** in the greedy left-to-right assignment, the slot of the last listed sibling (or AC
  `hi`) before the point considered; **−1** ("before every slot") when there is none.
- **Interval** of an AC: a pair `[lo, hi]` (§3), or *empty* when every branch is empty.
- A sibling with interval `[lo, hi]` (a listed element has `lo = hi` = its assigned slot) **may
  precede** a new child of slot `s` iff `hi ≤ s`, and **may follow** it iff `lo ≥ s`. An AC
  **is in slot `s`** iff `lo ≤ s ≤ hi`.

### 3. Rank of an AC sibling: an interval, the union over branches

The greedy assignment of ADR-0002 §4.1 is extended. At an AC, with `cur` as defined above:

1. The AC is first checked for well-formedness (§3.1).
2. For each branch:
   - **empty** → no interval;
   - otherwise run the greedy assignment over its members, starting at `cur`: a listed member is
     assigned the lowest candidate `≥` the slot before it; a nested AC takes part with its own
     interval (recursively, from the same running slot); an **unlisted member before the first
     listed one** makes the branch start at `lo = cur` (it is attached to the sibling before the
     AC, as ADR-0002 §4.3 attaches a direct unlisted child); an unlisted member after a listed one
     is attached to that member and changes nothing;
   - a **non-empty branch with no listed member** (only unlisted or vendor elements, or only
     nested ACs that are themselves empty) makes the AC **unranked**;
   - a member with no candidate `≥` the slot before it makes the parent unorderable.
3. The AC's interval is the **union** of its branches' intervals: `[min lo, max hi]`. An AC whose
   branches are all empty has an empty interval and constrains nothing (like an unlisted child);
   its members do not exist for any consumer.
4. The assignment continues after the AC with `cur = hi`.

An insert into a parent with an **unranked** or **malformed** AC, or whose child list (ACs read by
interval) admits no assignment, returns `Err(UnresolvedSibling)`.

Why the union: whichever branch a consumer takes, the processed child list must be in schema
order. A new child placed before the AC is valid for every branch iff its slot is `≤` every
branch's `lo`; placed after, iff its slot is `≥` every branch's `hi`. For one AC whose members are
all listed this is exact; with a leading unlisted member, `lo = cur` keeps that member next to the
sibling it is attached to, so a new child can never be put between them. The rule needs no
knowledge of which `Requires` namespaces a consumer understands.

The cases:

| Case | Rule | Corpus |
|---|---|---|
| Branch holding several listed elements | The branch interval covers all of them (`[29, 34]` for `[legacyDrawing, controls]`); an insert of a slot strictly inside is refused (§4, *straddle*). | 0 |
| Branches that disagree | Union; no refusal by itself (§6). | 0 of 342 ranked ACs |
| Empty branch (`<mc:Fallback/>`), or no Fallback | No interval: a consumer that takes it sees nothing there. | 171 + 2 empty; 248 without Fallback |
| Unknown `Requires` namespaces | Irrelevant to the rank: every branch counts, understood or not. A prefix not bound in scope is malformed (§3.1). | 0 unbound |
| Nested AC directly inside a branch | Recursion, as a member. | 0 |
| Non-empty branch with only unlisted or vendor members | AC **unranked** → refused (Q2). | 0 |
| Branch whose first member(s) are unlisted | `lo = cur` (the slot of the sibling before the AC, −1 if none). | 0 |
| Same name in two slots (ADR-0002 §4.1, `w:tbl`) | The greedy assignment picks the lowest candidate `≥` the running slot, inside branches too. | — |
| Wildcard slots | `candidates` applies inside branches as it does to direct children. | — |

#### 3.1 Malformed AC

The rule assumes the shape ECMA-376 Part 3 requires for `AlternateContent`. Otherwise the AC is
**malformed** and every insert into its parent refuses (`UnresolvedSibling`):

- an AC with no `mc:Choice`, more than one `mc:Fallback`, a `mc:Fallback` that is not last, or an
  element child that is not a branch;
- an `mc:Choice` without `Requires`, or with a `Requires` prefix that is not bound **in scope** at
  that `mc:Choice` (the lens has the scope: `declarations(root, path)`).

The corpus has none (every AC is `Choice` or `Choice + Fallback`, every prefix bound in scope).

#### 3.2 Correlated sibling ACs

Two sibling ACs whose Choices require the same namespaces are resolved **together** by a consumer:
it takes the Choice in both or the Fallback in both. The union of §3 treats each AC
independently, so it may refuse a combination no consumer would produce (example: `AC1{Choice:
X(5), Fallback: empty}` then `AC2{Choice: empty, Fallback: Y(3)}` reads as disorder, though no
consumer sees `X` and `Y` together). This is an **over-refusal, on the safe side**, and the ADR
keeps it: modelling correlation needs `Requires` semantics the core does not have. The corpus has
6 such parents, all xlsx, and the correlation changes no outcome:

| File | Parent | ACs |
|---|---|---:|
| `58325_db.xlsx` | `xdr:wsDr` | 3 |
| `bug64512_embed.xlsx` | `xdr:wsDr` | 2 |
| `mv-calculator-final-2-20-2013.xlsm` | `xdr:wsDr` | 163 |
| `style-alternate-content.xlsx` | `x:fonts` | 6 |
| `style-alternate-content.xlsx` | `x:cellXfs` | 7 |
| `style-alternate-content.xlsx` | `x:dxfs` | 11 |

### 4. Insert position

#### 4.1 Plain append: `insert`, `get_or_insert`, `move_in`, `check_insert`, `check_get_or_insert` (and `_with_ns`)

`place_skipping` walks the children once, as today, treating a ranked AC as a sibling with an
interval:

- The new child goes before the first sibling that **may not precede** it (`hi > s`). If that
  sibling also **may not follow** it (`lo < s`), the insert is refused: `Err(UnresolvedSibling)`
  (*straddle*). With `lo = hi`, straddle is impossible; it needs a multi-slot branch, disagreeing
  branches, or a leading unlisted member.
- An AC **in slot `s`** with `lo = hi = s` is a member of the new child's slot. Since a slot's
  names may appear in any order (ADR-0002 §4.1), an append lands after it, at the end of the
  slot.
- Whitespace and dirty duties are ADR-0002 §4.2's: no whitespace is added, the AC is not touched,
  so the byte diff is the new element at the insertion point (T2).

#### 4.2 Callback and positional forms: refused when an AC is in the slot

`insert_by`, `insert_in_slot`, `insert_at`, `move_in_by` and their `check_*` and `_with_ns`
forms let the caller choose a position **within** the slot, by a `before(index_in_slot, node)`
callback or an item index. When any AC of the parent **is in slot `s`**, they return
`Err(UnresolvedSibling)`, whatever the callback or index (coordinator decision for review M1).

Why: the meaning of "the k-th sibling in the slot" next to an AC is not defined. Existing callers
count differently (§4.4): docx counts every element except `w:pPr`, the AC included; pptx and
xlsx compare the lens's `index_in_slot`, which never sees the AC. Either convention silently
moves content under the other (review M1: `[pPr, r0, AC{r}, r1, r2]`, `After(r1)` lands after
`r2`), and "just after `r1`" cannot be expressed relative to an AC at all. Refusing keeps every
existing caller correct without auditing its counting.

When no AC is in slot `s`, these forms behave as today and as §4.1 for ACs in other slots: the
callback still sees only same-slot listed siblings, and `index_in_slot` is unchanged.

`insert` is `insert_by(|_, _| false)` today; U1 gives it (and `get_or_insert`, `move_in`) an
internal append mode so that it is not refused by this section.

#### 4.3 `insert_at` / `check_insert_at` reading of ACs

`insert_at` checks locally (O(1) for an unlimited slot), so it has no greedy `cur`. It reads ACs
as follows:

1. If the parent has an AC child, it scans the children once and refuses
   (`UnresolvedSibling`) if **any** AC is malformed or unranked, or **is in slot `s`** (§4.2),
   whether or not it is adjacent to `index`. This matches `insert_by`, which refuses the whole
   parent in those cases.
2. The nearest ranked sibling before `index` must satisfy `hi ≤ s`, the nearest after `lo ≥ s`
   ("nearest" skips unlisted children and empty ACs, as today). For an AC neighbour, the
   interval is read **without** `cur`: `lo` = the minimum and `hi` = the maximum over **every
   candidate slot of every listed member of every branch** (as review F1 requires for a direct
   neighbour with several candidates), and `lo = −1` if a branch starts with an unlisted member.
   This is never narrower than the greedy interval, so the local check cannot accept what
   `insert_by` would refuse.
3. Limits are counted as today by the full walk (§5).

To keep the O(1) path for parents without an AC, U1 keeps an internal per-node bit "may have an AC
child", set by the parser and by the insert primitives (never cleared except by a rescan). It is
not public and may be stale-true, never stale-false.

#### 4.4 Existing callers of the callback and positional forms

All of them refuse, after U1, when an AC is in the slot they insert into; before U1 they refused
whenever the parent had an AC. U1's acceptance includes, **per call site**, a test that inserts
next to a same-slot AC, gets the refusal, and saves byte-identical output.

| Crate | Call sites (`file:line` on `d066ff1`) | Position source |
|---|---|---|
| docx | `images.rs:249/281` (`insert_image`; `k` from `run_insertion_point`, `images.rs:385`) | in-slot count, AC **included** |
| docx | `structure.rs:643/661` (hyperlinks; `k` from `structure.rs:624`, `before_slot_child`) | in-slot count, AC **included** |
| docx | `bookmarks.rs:409/421`, `controls/wrap.rs:249/295` (`move_in_by`/`insert_by` with constant callbacks) | callback |
| docx | `insert_field.rs:222`, `bookmarks.rs:160/167/389`, `comments.rs:607/611/631`, `revisions.rs:675/694/720/725/859/930`, `controls.rs:672`, `controls/wrap.rs:240/337`, `tables.rs:296/298/709/715/739`, `notes.rs:37`, `structure.rs:356/361/489/492/498/739/756`, `replace.rs:750–769`, `format.rs:689/867/960`, `styles.rs:278` | item index |
| pptx | `slides.rs:378/533/994`, `write.rs:201/1079`, `write/formatting/range.rs:201`, `write/table.rs:613/709/718/860`, `replace.rs:540/543` | lens `index_in_slot` |
| pptx | `write/shape_properties.rs:919` | item index |
| xlsx | `sheet_ops/mod.rs:563/692` | own count of `x:sheet` / `index_in_slot` |
| xlsx | `layout.rs:589/646/666/978`, `worksheet.rs:257/270/358/368` | item index |
| xlsx | `hyperlink.rs:199`, `range.rs:107/270/317/393/443`, `layout.rs:209/574/577/644` (`check_insert_by(\|_, _\| false)`, dry runs of a plain append) | none: **switch to `check_insert`/`check_get_or_insert`** in U1 so they follow §4.1 |

Most of these never meet an AC in their slot in the corpus (only the docx rows do: run content and
body content), but each gets its test.

### 5. Occurrence limits: count through the branches

For `max_each` and `max_total`, an AC contributes, for the new child's slot `s`, the **maximum
over its branches** of the members assigned to `s` (and, for `max_each`, of the members with the
new child's QName); a nested AC contributes recursively. Both branches are always consulted; an
empty branch contributes 0.

The maximum, not the sum, because a consumer sees one branch; not the Choice alone, because a
consumer may take the Fallback. An element inside a branch therefore **does** count toward the
parent's limits. The result is `Err(AlreadyPresent(q))`, as for a direct child. `get_or_insert`
still returns only a **direct** existing child; when the child exists only inside an AC, it
returns `AlreadyPresent` instead of writing a duplicate (the 18 transition cases). Reading through
the AC is the format crates' job (ADR-0008's branch choice).

### 6. Disagreeing branches: no refusal, no runtime diagnostic

ADR-0002 asked for "a diagnostic if branches disagree". This ADR drops it:

- The union (§3) only **widens** the interval. Inserts outside it are correct for every branch;
  inserts inside it are refused as *straddle*.
- `ooxml-core` has no logging channel and no warning type. A warning that callers cannot act on
  would be API surface with no use; the measurable question ("how often do branches disagree")
  is answered by the spike (0 of 342) and can be re-run.
- Refusing every parent whose AC branches disagree (alternative A5) would refuse inserts that are
  provably safe.

### 7. What stays refused

| Operation | Today | After this ADR | Why |
|---|---|---|---|
| Insert, remove or edit **inside** a branch (any depth) | Unreachable through the typed lens (no `ChildOf` for branches); format crates refuse (`InsideAlternateContent`, `UnsupportedContext`, `UnsupportedControl`, …) | **Unchanged** | One branch edited alone de-synchronises the others; editing all branches consistently means mapping between different markups (`wps` vs VML, `p14` vs base transitions), which is per-format semantics, not ranking. A format ADR may decide a both-branch edit for a specific element, as ADR-0009's P3 note did for `controls` attribute rewrites. |
| Callback or positional insert into a slot an AC is in (§4.2) | Refused (any AC) | **Refused** | Position within the slot is undefined next to an AC. |
| Move into or out of an AC (`MoveRefusal::AlternateContent`) | Refused | **Unchanged** | Same reason as edits inside. A move **between siblings of an AC** is an insert at the destination and follows §4 (`move_in` §4.1, `move_in_by` §4.2). |
| Detach / copy a subtree that contains an AC, or an AC itself (`DetachRefusal::AlternateContent`) | Refused | **Unchanged** | Copying both branches with their `Requires`/`Ignorable` bindings is a separate design (#49). |
| Insert that straddles a ranked AC (§4.1) | Refused (any AC) | Refused | No position is valid for every branch. |
| Insert into a parent with an unranked or malformed AC (§3, §3.1) | Refused | Refused (Q2) | Fail closed. |
| Removing a whole AC child (`remove_at`) | Allowed | **Unchanged** | No ordering question. |
| Format pre-checks for reasons other than rank (index-stable tables, relationship targets inside AC, cell grids in AC) | Refused | **Unchanged** unless §9 says otherwise | Not a ranking problem. |

### 8. API impact

- **Core, for the rule.** Internal to `LensMut::place_skipping`, `plan_insert_at` and their
  helpers, plus the internal append mode (§4.2) and the internal "may have an AC child" bit
  (§4.3). No existing public signature changes.
- **One new public core function** (review m4), for checks on child lists that are not in the
  tree yet (xlsx `check_insertable`, `worksheet.rs:297`, where a cell's `f`/`v`/`is` are about to
  be replaced, and `edit.rs:698` for `si`):

  ```rust
  /// `Ok(())` exactly when appending a `q` child (ADR-0020 §4.1) to the element at
  /// `parent_path` of `doc` would succeed if its children for which `skip` returns
  /// `true` were absent. `E` is the parent's spec. Changes nothing.
  pub fn check_append_among<E: ElementSpec>(
      doc: &XmlDocument, parent_path: &[usize], q: QName, skip: impl FnMut(&RawNode) -> bool,
  ) -> Result<()>
  ```

  It takes the document and path (like `Lens::check_insert_under`) so that `Requires` bindings
  are checked in scope (§3.1). xlsx's `check_insertable` becomes a thin wrapper over it, so the
  rule has one implementation. This is a hand-written core item, not a generated name.
- **Errors.** The refusals keep `Error::UnresolvedSibling(parent)` (Q3); its doc comment and
  message change from "mc:AlternateContent sibling or out-of-order children" to "out-of-order
  children, or an `mc:AlternateContent` sibling that allows no position (straddled, in the slot
  of a positional insert, unranked or malformed)". `AlreadyPresent` gains the branch-count cause.
  `Error` is `#[non_exhaustive]`, so a later dedicated variant stays possible.
- **Generated code.** Nothing changes: `cargo xtask codegen --check` is clean, no generated public
  name changes.
- **`mce_wrapped` redefinition (review m5, Q5).** ADR-0002 §1.3's `mce_wrapped` is redefined to
  remove the typed insert for the wrapped child (its `ChildOf` impl and the `insert_*` /
  `get_or_insert_*` accessors) but **keep the name in its slot**, so the slot table still ranks
  the AC that wraps it. It may be applied **only** to children Office never writes as direct
  children (such as `x15ac:absPath` in `x:workbook`); never to children that also occur directly
  (`legacyDrawing`, `controls`: applying it there would remove `ChildOf<Worksheet> for
  LegacyDrawing` and `add_note`'s insert path). Applying it removes a public impl, so each use
  goes through the normal generated-diff review. No active override uses it, so this changes no
  generated file today; U1 rewrites the `xtask/src/model.rs` unit test (which applies it to
  `legacyDrawing`) to an allowed child and to the new semantics.
- **Format crates.** The rule lifts nothing by itself where a format crate pre-checks; each crate
  removes or narrows its own pre-check (§9). Generated names are untouched there too.

#### 8.1 PR #144 (ADR-0019 P4a-1)

Open PR #144 adds a second AC placement to the core: a public
`LensMut::insert_after_alternate_content` and a `rank_alternate_content` flag on
`place_skipping`, with a point rank (minimum over each branch's first element) and no limit
counting through branches. Two rules and an extra public method in the core would be removed
again by U1, which would change a public name. Decision (coordinator, 2026-11-22):

| | PR #144 as opened | After this ADR |
|---|---|---|
| Core mechanism | `insert_after_alternate_content` + `rank_alternate_content` flag | **U1 only**; #144 adds **no public core API** and no core rule |
| Rank | point (min of first members) | interval, union over branches (§3) |
| Limits | direct children only | through branches (§5) |
| pptx call sites (`p:sld` `p:extLst`, `p:cSld` `p:extLst`) | `insert_after_alternate_content::<…ExtensionList>()` | `get_or_insert`/`insert` (§4.1): the AC holds `p:transition`, another slot, so the append lands after it |
| Merge order | — | after U1; #144 also waits for the owner's F8 fixture, so U1 does not delay it |

Delivery order: **U1 → #144 rework → U4 (pptx) → U2 (xlsx) → U3 (docx)**.

### 9. Which refusals each format can lift

"Lift" means: the format crate removes its AC pre-check for that operation and relies on the
core rule; the core then refuses only straddle, same-slot positional inserts, unranked or
malformed ACs, and limits.

**General rule (review M2).** A format check that decides by looking up an *existing direct
child* (to clear it, edit it, remove an attribute, or test uniqueness) must, when its AC
pre-check is removed, also look in every branch of the parent's ACs. If the looked-up kind
exists in a branch, the operation stays refused (the pre-check stays for that kind) unless a
format ADR decides a both-branch edit. Otherwise a Clear or removal returns `Ok` and changes
nothing while consumers still read the value from the branch.

| Format | Operation (refusal today) | Decision | Corpus / fixture |
|---|---|---|---|
| xlsx | Worksheet-level inserts next to `AC{controls}` (`plan_target`, `check_insertable`; `add_note`'s `legacyDrawing` (P5c); P7-pre/P7a worksheet inserts) | **Lift.** `check_insertable` delegates to `check_append_among` (§8); `check_insert_by(\|_, _\| false)` dry runs switch to `check_insert` (§4.4). | 2 POI sheets; `reference/fixtures/xlsx/poi/62629_target.xlsm` |
| xlsx | `x:workbook` inserts next to `AC{x15ac:absPath}`: `definedNames` (`set_autofilter`, ADR-0014), `calcPr` (ADR-0003) | **Lift.** A `definedNames` inside a branch counts (§5). | 59 workbooks gain `definedNames`; 13 fixtures |
| xlsx | Style tables with AC entries (`push_reserved`) | **Stays.** Index semantics (an AC is one entry only when index-stable, ADR-0006), not rank. Guard: `registry/mod.rs` `Table::has_ac` and `registry/styles.rs` `push_reserved` (Q6). | `style-alternate-content.xlsx` |
| xlsx | Shared strings with an AC child | **Stays.** After `check_insertable` delegates, the rank rule alone would allow an `si` append after a same-slot AC; the refusal is kept by `registry/strings.rs` `SharedStrings::activate` (`UnresolvedSibling(SharedStringTable)`) and `Table::has_ac`, independent of the core. U2 keeps `sst_with_alternate_content_refuses_shared_writes`. | 0 in corpus |
| xlsx | `sheetData`/`row`/`c` containing AC; a `drawing`/`legacyDrawing` wrapped in AC; G1/G2 rewrites (ADR-0018) | **Stays.** Edits inside branches or grid indexes; `index.rs` guards are unchanged. | 0 grids with AC |
| xlsx | `xdr:wsDr` anchor append (raw splice before `</xdr:wsDr>`, ADR-0017) | **No change**; through the lens it would be a plain append next to same-slot ACs (allowed), but there is no reason to move it now. | 170 ACs |
| docx | `Document::append_paragraph` into a `w:body` with an AC | **Lift** (it already relies on the core and uses `insert`, a plain append). | `sdk-HelloO14.docx` |
| docx | `format_runs`/`set_run_properties` on a run that wraps a drawing (`run_container`) | **Lift**, with the general rule: `w:rPr` is slot 0 of `w:r`, the AC is in the content slot, so `check_insert_at::<RunProperties>` passes; refused if a branch holds a `w:rPr`. | 20 runs, 6 files |
| docx | Property **inserts** into a `w:rPr`/`w:pPr` with an AC child (`Planner::existing`) | **Lift only for property kinds (QNames) that occur in no branch** of that element's ACs. | 0 in corpus; synthetic test |
| docx | Property **Clear**, **Set of an existing value** and **attribute removal** where the property exists inside a branch | **Stays refused** (review M2): the pre-check stays for those kinds. Inserting such a kind is `AlreadyPresent` by §5 anyway. | synthetic test |
| docx | `insert_image`, hyperlinks, and every positional insert next to a same-slot AC (§4.4) | **Stays refused** (§4.2). | 20 runs, 1 body |
| docx | Comment anchors, content controls and fields inside AC | **Stays** (inside a branch). | — |
| pptx | `p:sld` / `p:cSld` `p:extLst` insert next to an AC-wrapped transition (ADR-0019 §3) | **Lift** through #144's rework onto U1 (§8.1). | 12 slides, 7 files; `bug68703.pptx` |
| pptx | `p:timing` insert next to an AC-wrapped transition (roadmap Phase 4) | **Allowed by the core**; the operation itself is Phase 4 work. | 9 ok, 3 present |
| pptx | `p:transition` insert where the transition exists only inside an AC | **`AlreadyPresent`** (was `UnresolvedSibling`); writing a transition is Phase 4 and edits both branches. | 18 |
| pptx | `add_table_style` into an `a:tblStyleLst` with an AC child | **Lift for the append** (plain `insert`), with the `styleId` uniqueness check extended to branch members (general rule). | 0 in corpus; synthetic test |
| pptx | Positional inserts into a `p:spTree` (or other slot) that an AC is in | **Stays refused** (§4.2). | 1 (`Of16-01.pptx`) |
| pptx | Shapes, text, formatting, links, notes body inside AC (`InsideAlternateContent`); theme slots or values wrapped in AC; `notesMasterIdLst` wrapped in AC (the new `p:notesMasterId` would go inside the branch) | **Stays** (inside a branch). | — |

### 10. Invariants

- **`Err ⇒ byte-identical`.** Every new refusal is decided in the planning walk, before any node
  is created, as today. Format crates that drop a pre-check keep their plan → validate → apply
  order; their existing refusal tests stay, with AC cases moved from "refused" to "applied" only
  where §9 lifts them.
- **T2 locality.** The AC is never touched; the diff is the inserted element (ADR-0002 §4.2).
- **Fail closed.** Unranked, malformed and straddled ACs refuse; positional inserts next to a
  same-slot AC refuse; branches are counted for limits; existing-child lookups look in branches.

## Test oracles

1. **Core unit tests**, inline XML, for each row of §3's table, §3.1 and §3.2: single-slot AC
   before, after and in the new child's slot; a branch with two slots (insert below, at `lo`,
   strictly inside, at `hi`, above); disagreeing branches (the §Context example); an empty
   Fallback and a missing Fallback; a nested AC; a branch with only vendor members; a branch that
   starts with a vendor member (an insert between the previous sibling and the AC refuses); each
   malformed shape, including a `Requires` prefix bound only on a sibling; same-name-in-two-slots
   inside a branch; the correlated pair of §3.2 (refused). Every `insert`, `insert_by`,
   `insert_at` and `check_*` result is compared with a brute-force oracle: for **every**
   combination of branch choices, substitute the branches, check the processed child list with
   ADR-0002 §4.1's greedy assignment and the limits; a plain append must be accepted only if every
   combination is valid, and its position must be one such.
2. **Positional forms (§4.2):** for each form, a same-slot AC refuses regardless of callback or
   index; an AC in another slot does not.
3. **Per-caller refusals (§4.4):** one test per listed call site (grouped by function where several
   lines are one operation): a same-slot AC, the operation refuses, the saved output is
   byte-identical to the input.
4. **Limits:** `get_or_insert::<Transition>` on `bug68703.pptx` slides returns `AlreadyPresent`;
   `max_total` counted through branches with branch sizes 0/1/2.
5. **`Err ⇒ byte-identical`** for each core refusal: straddle, unranked, malformed, positional
   same-slot, limit through a branch.
6. **Corpus sweep** (CI or the spike): re-run `ac_rank.py`; the Rust implementation's outcome for
   every (parent, typed child) pair in `results/ac-rank.txt` equals the spike's, for the append
   and the positional forms.
7. **Mutation** (AGENTS.md): at least (a) point rank `= min lo` instead of the interval,
   (b) ignore branches for limits, (c) treat an AC with an empty branch as unranked, (d) skip the
   same-slot refusal of §4.2, (e) `lo = first listed` instead of `cur` for a leading unlisted
   member; each must fail a test.
8. **Per-format lifts:** for every §9 "Lift" row, a test that succeeds on the named fixture and
   whose output reopens and round-trips (python-pptx for pptx, the docx readback oracle, the xlsx
   readers), plus, for every "Stays" row touched by a lift, a case that still refuses
   byte-identically (docx: Clear of a `w:b` that exists only inside a branch of `w:rPr`).

## Delivery

Each unit is one PR, reviewed until converged (no Blocker/Major), in this order (§8.1):
**U0 → U1 → #144 rework → U4 → U2 → U3.**

0. **U0: this ADR and the spike.** Docs and `spikes/ac-rank/` only.
   - **Acceptance:** review converged; owner answers recorded; ADR-0002 §4.3 gets a note pointing
     here (added when this ADR is Accepted).
1. **U1: core rule.** `place_skipping`, `plan_insert_at`, the interval/branch helpers, the
   malformed-AC check with in-scope `Requires`, the append mode and the positional refusal (§4.2),
   the internal AC bit (§4.3), `check_append_among` (§8), the error doc changes; ADR-0002 §4.3
   note; the `mce_wrapped` redefinition in `xtask` with its rewritten unit test.
   - **Acceptance:** AGENTS.md's definition of done; oracles 1, 2, 4, 5 and 7; oracle 3 for every
     §4.4 call site; oracle 6 on the committed fixtures; the `check_insert_by(|_, _| false)` dry
     runs of §4.4 switched to `check_insert`; `cargo xtask codegen --check` clean with **no**
     generated diff; the existing AC refusal tests that assert the old blanket refusal are
     updated case by case, each change named in the PR body; the python-pptx oracle green.
2. **#144 rework** (not a unit of this ADR; recorded for the order). #144 drops
   `insert_after_alternate_content` and the `rank_alternate_content` flag and uses U1's
   `get_or_insert`/`insert`; merges after U1 and after the owner's F8 fixture.
3. **U4: pptx lifts** (§9 pptx).
   - **Acceptance:** `add_table_style` next to a synthetic AC with the extended `styleId` check;
     `p:transition` insert → `AlreadyPresent`; positional `p:spTree` inserts on `Of16-01.pptx`
     still refuse byte-identically; python-pptx oracle green; ADR-0016/0019 notes updated.
4. **U2: xlsx lifts** (§9 xlsx).
   - **Acceptance:** `add_note` on `62629_target.xlsm`; `set_autofilter` creating `definedNames`
     on a workbook with `AC{absPath}`; a `calcPr` insert; `check_insertable` delegates to
     `check_append_among`; the shared-string and style-table refusals still refuse
     byte-identically (existing tests kept); ADR-0003/0014/0017 notes updated.
5. **U3: docx lifts** (§9 docx).
   - **Acceptance:** `append_paragraph` on `sdk-HelloO14.docx` succeeds and reads back; run
     formatting on the drawing runs of `sdk-complex2010.docx`/`poi-shapes-with-text.docx`;
     property inserts next to a synthetic AC in `w:rPr`; Clear, Set-existing and attribute
     removal of a property that exists only in a branch refuse byte-identically; `insert_image`
     and hyperlink next to a same-slot AC refuse byte-identically; the docx oracle job green;
     ADR-0004/0007 notes updated.

## Alternatives considered

- **A1. Keep the blanket refusal.** Simple and safe, but it refuses 708 typed appends in the
  corpus that are provably valid, and every format grows its own pre-checks (it already has).
- **A2. The recorded point rule** (minimum first-listed rank). Equal to this ADR's rule on every
  corpus AC, but wrong for multi-slot branches and disagreeing branches (§Context), and silent
  about limits. The interval costs the same walk. PR #144's rule is this alternative (§8.1).
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
- **A8. Define `index_in_slot` next to an AC** (count the AC, or pass it to the callback).
  Revision 1 chose "do not count, do not pass"; review M1 showed docx callers count it, pptx and
  xlsx do not. Any single definition silently breaks one group; refusing (§4.2) breaks none.
- **A9. Leave unlisted members out of the branch interval** (revision 1). It can put a new child
  between a vendor member and the sibling it is attached to (review M4); fail closed instead.

## Consequences

- Positive: 708 corpus appends placed across the three formats (256 / 411 / 41), including the
  position refusals the reviews hit (`definedNames`, `legacyDrawing`, `append_paragraph`, run
  properties, slide `p:extLst`). Limits become correct next to AC (18 transitions no longer at
  risk of duplication once Phase 4 writes them).
- Positive: one rule in the core replaces several per-format pre-checks and PR #144's second rule;
  new format work no longer needs its own AC refusal for positioning.
- Negative: positional inserts next to a same-slot AC stay refused, which covers every docx
  run-content and body-content case in the corpus (`insert_image`, hyperlinks); lifting them
  needs a defined in-slot index, a later decision.
- Negative: placement now inspects branch contents, O(size of the AC's branches) per AC per
  insert (an `xdr:wsDr` can hold 170 ACs); `insert_at` on a parent with an AC scans the children
  once.
- Negative: one new public core function (`check_append_among`).
- Neutral: edits inside branches stay refused; the formats keep those refusals and their error
  variants.

## Open questions for the project owner

- **Q1. Disagreeing branches: widen or refuse?** Use the union interval and refuse only a
  straddling insert (§6), or refuse every insert into a parent whose AC branches disagree (A5).
  **Recommendation:** widen. Inserts outside the union are valid for every branch; the corpus
  has 0 disagreeing ACs, so the choice only affects hand-made input.
- **Q2. Unranked ACs: refuse?** An AC with a non-empty branch that holds no listed member (only
  vendor elements), and, by extension (review M4), a branch that starts with unlisted members
  (`lo = cur`, §3). Treating them as unlisted would match ADR-0002's handling of a direct vendor
  child; refusing is fail-closed. **Recommendation:** refuse, and `lo = cur` for leading
  unlisted members. It costs nothing measured (0 in 603 packages), and such content has a
  position the library cannot know. Relax later with a fixture if one appears.
- **Q3. Error variant: reuse `UnresolvedSibling` or add one?** A new variant
  (`AlternateContentOrder { parent }`) would tell callers *why*; reusing keeps format-crate
  `match` arms and documented refusals unchanged. **Recommendation:** reuse
  `UnresolvedSibling`, with the doc change of §8; add a variant only if a format needs to tell
  the cases apart.
- **Q5. Redefine `mce_wrapped` (keep the rank, drop the typed insert), limited to children Office
  never writes directly?** The current definition would erase exactly the information this ADR
  uses. **Recommendation:** yes, in U1, with the restriction of §8 (e.g. `x15ac:absPath`, never
  `legacyDrawing`/`controls`); it changes no generated file today.
- **Q6. Style-table appends next to an index-stable AC (xlsx).** ADR-0006 already treats an
  index-stable AC as one entry; appending after it is now position-safe. Lift in U2, or leave
  for a separate ADR-0006 note? **Recommendation:** leave it out of this ADR: it changes index
  arithmetic (`count`, `xfId` numbering), not ranks, and the corpus has one such file.

### Decided by the coordinator (2026-11-22)

- **Q4 (revision 1: should the `before` callback see a same-slot AC?).** Replaced: callback and
  positional forms **refuse** when an AC is in the slot; only plain appends land next to it
  (§4.2, review M1).
- **Q7 (revision 1: order of U2–U4).** Replaced: **U1 → #144 rework → U4 → U2 → U3**; #144 adds
  no public core API (§8.1, review M3).

## Sources

- ECMA-376 Part 3 (Markup Compatibility and Extensibility): `AlternateContent`, `Choice`,
  `Fallback`, `Requires`.
- ADR-0002 §4.1–§4.3 and its review round 1 (M1) and round 2 (MC namespace in core); this ADR's
  [review round 1](0020-opus-review-round1.md).
- `crates/ooxml-core/src/lens.rs` (`place_skipping`, `plan_insert_at`, `check_insert*`,
  `check_insert_under`), `crates/ooxml-core/src/lens/move_subtree.rs`,
  `crates/ooxml-core/src/lens/same_part.rs`, `crates/ooxml-core/src/error.rs`;
  `xtask/src/model.rs` (`Override::MceWrapped`).
- Format code: `crates/ooxml-xlsx/src/worksheet.rs` (`check_insertable`, `plan_target`),
  `crates/ooxml-xlsx/src/edit.rs`, `crates/ooxml-xlsx/src/registry/{mod,strings,styles}.rs`,
  `crates/ooxml-docx/src/format.rs` (`Planner::existing`, `run_container`),
  `crates/ooxml-docx/src/document.rs` (`append_paragraph`), `crates/ooxml-docx/src/images.rs`,
  `crates/ooxml-docx/src/structure.rs`, `crates/ooxml-pptx/src/write/table_style.rs`,
  `crates/ooxml-pptx/src/slides/notes.rs`, `crates/ooxml-pptx/src/write/theme.rs`, and the call
  sites of §4.4.
- PR #144 (`impl/pptx-phase4a1-add-comment`).
- [`spikes/ac-rank/`](../../spikes/ac-rank/README.md) and `results/ac-rank.txt`.
