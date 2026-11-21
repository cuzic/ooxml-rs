# ADR-0019: PowerPoint Phase 4a — writing modern comments

- Status: **Accepted** (revision 4, owner decisions 2026-11-21): converged
  in review round 2; Q1–Q4 and Q6–Q8 as recommended, Q5 the alternative
- Date: 2026-11-21
- Depends on:
  - [ADR-0001](0001-lossless-round-trip-architecture.md): T2 locality,
    `Err ⇒ byte-identical`, Layer 3 append-only registries.
  - [ADR-0002](0002-schema-driven-code-generation.md): `[write]` allowlist
    entries, and the AC rank "later rule" for insertions next to
    `mc:AlternateContent`.
  - [ADR-0005](0005-pptx-phase0.md) §6: the modern-comment anchor classifier
    and the text locks.
  - [ADR-0008](0008-pptx-phase1-deck-reading.md) §3.3: `Slide::comments()`,
    which reads both comment kinds through the same classifier.
  - [ADR-0012](0012-pptx-phase2-slides-and-shapes.md): the transaction shape
    of §2, the id rules of §3 (`sldId`, shape ids including `ac:*Mk@id`,
    `p14:creationId`), comment handling on duplicate (§4.4) and in the
    delete cascade (§6).
  - [ADR-0016](0016-pptx-phase3-formatting-and-links.md): the P3e note on
    UUID v8 for table style ids, a precedent this ADR does *not* follow
    (§5, Q1).
  - [ADR-0017](0017-xlsx-phase5-objects.md) §5 and its P5c note (PR #121):
    threaded comments and persons in xlsx. Its lessons on identity, GUIDs,
    timestamps and ownership are reused here.
  - [ADR-0015](0015-docx-phase3-review-features.md) §3: docx comments, for
    the operation set, namespace declarations and "convert is the UI's job".
  - The core registry transaction (`RegistryPlan`, PR #38).
- Evidence:
  - [`spikes/pptx-phase4a/`](../../spikes/pptx-phase4a/README.md)
    (`comments_survey.py`): 748 distinct presentations and 1,580 slides,
    output in `results/comments-748.txt`.
  - The PowerPoint-made fixture `reference/fixtures/pptx/project/powerpoint-shape-comment.pptx`
    (issue #114, PR #123).
  - LibreOffice `2e885d0b` export source (§11).
  - Open XML SDK schema data in `reference/openxml-sdk-data/`.
- Review: [round 1](0019-opus-review-round1.md) (not converged: M1;
  m1–m10, n1–n3), [round 2](0019-opus-review-round2.md) (converged:
  m11–m14, n4–n5 applied in revision 3)

## Revision history

- **Revision 1:** initial draft.
- **Revision 2** (after `0019-opus-review-round1.md`, not converged: one
  Major, M1; m1–m10, n1–n3; coordinator decisions recorded):

  | Finding | Change | Where |
  |---|---|---|
  | M1 deleting the last thread removed every `p188:commentRel` (`drop_comment_rel` ignores `r:id`) | a targeted `drop_comment_rel_for(rid)`: only the `p:ext` whose `commentRel@r:id` equals the removed relationship's `Id`; the add-time refusal for two modern relationships stays; a two-relationship test and a mutation | §6, §11.1, §11.2, §11.7 |
  | m1 the `Local` form comes from a hand-edited fixture | stated as "accepted by PowerPoint", not "written by"; P4a-0 F8 (signed out, author kept as written) decides the default form | Context, §4, Delivery |
  | m2 `Exact` reuses a name removed in the session | §3.1 corrected; add → delete-last → add test | §3.1, §11.2 |
  | m3 adding `p14:creationId` underspecified | `insert_creation_id`: position of a new `p:cSld/p:extLst`, local `p14` declaration, refusal of a `creationId` elsewhere or not a u32; locality row | §3.2, §9, §11.1 |
  | m4 invariants fail on tolerated input problems | I3/I4/I6/I12 compare input and output and fail only on new violations | §11.3 |
  | m5 no LibreOffice tooling for pptx | new `odp_annotations` key in `tools/lo-check` (P4a-1); P4a-0 baseline run; fallback oracle if 25.2 does not import p188 | §11.5, Delivery |
  | m6 input validation and existing threads | `lang`, `initials`, `created` range, `\r` normalised; existing unsupported anchors allowed when untouched; `closed` threads | §4, §5, §9 |
  | m7 `authorLst` has no `extLst` | unknown children (including `p188:extLst`) refused; URI-based namespace rule for the authors part | §4, §9 |
  | m8 `delete_slide` and unreadable parts | `delete_slide` reserves what it can read and skips the rest (no new `Err`); the add-time scan refuses on an unreadable part | §5, §8, §9 |
  | m9 sweep expectations | per-variant allowlist; any other refusal fails the sweep | §11.6 |
  | m10 missing fixture and check | F9 (two threads on one slide), M10 (the `Account` author opens the file) | Delivery, §11.8, Q1, Q5 |
  | n1 provenance of the 4 v4 ids | 1 from the PowerPoint fixture, 3 from `modern-comments.pptx` | Context |
  | n2 spike counts both AC branches | README note | spike README |
  | n3 initials for Japanese names | *(u)*, checked by F2 and M4 | §4 |

- **Revision 3** (after `0019-opus-review-round2.md`: converged, Minor and
  nits only). Status: Proposed, awaiting the owner's decisions on Q1–Q8.

  | Finding | Change | Where |
  |---|---|---|
  | m11 text rules differ from ADR-0005, so read-back text with `a:br` cannot be written | comment text follows `TextBodyMut::set_text`'s table: `\v` → `a:br`, `\r\n`/`\r` → `\n`, `\t` allowed; the rationale is that precedent | §5 |
  | m12 no same-value rule for text edits | normalised text equal to the stored text → `Ok`, byte-identical; tests for `a\nb` and `a\r\nb` | §5, §11.2 |
  | m13 the id scan refused too broadly | only modern comment and authors parts; refused only when a part does not parse; ids collected from parts with AC or unknown children; legacy parts not scanned (no GUIDs); the add/delete asymmetry stated | §5, §8, §9 |
  | m14 "edited elements satisfy every invariant" brings back I3/I6 false positives | created elements satisfy every invariant; edited elements only for what was written | §11.3 |
  | n4 the fallback LibreOffice oracle cannot be byte-identical (LibreOffice rewrites comment parts) | fallback is "opens and re-saves without error" only | §11.5 |
  | n5 "survey lists as non-conforming" is not an output of the survey | committed allowlist file of (deck, error) pairs | §11.6 |

- **Revision 4** (owner decisions, 2026-11-21). Status: Accepted.

  | Decision | Change | Where |
  |---|---|---|
  | Q1–Q4, Q6–Q8 as recommended, with their written fallbacks | recorded | Owner decisions |
  | Q5: the alternative | `Account` authors match on (`userId`, `providerId`), `Local` authors on name; kind from `providerId` (`"None"` = Local); a differing display name reuses the author unedited; tests, mutations and invariant I13 | §4, §11.2, §11.3, §11.7, Owner decisions |

## Context

Phase 1 reads comments of both kinds (ADR-0008 §3.3, `deckread.rs`).
Phase 0's text locks classify modern-comment anchors (`refs.rs`).
Phase 2 keeps comments consistent when slides are deleted, duplicated or
moved: `slides.rs` drops comment relationships and `p188:commentRel` on
duplicate, cascades comment parts on delete, and counts `ac:*Mk@id` in
`next_shape_id`. Nothing writes a comment yet. The PowerPoint roadmap
(`docs/roadmap/pptx.md` §4) lists "modern comment writes (author registry,
GUIDs)" as a Phase 4 item. This ADR is that item.

PowerPoint has two comment formats:

| | Legacy (ECMA-376 §19.4) | Modern (`p188`, PowerPoint 2021 / Microsoft 365) |
|---|---|---|
| comment part | `ppt/comments/commentN.xml`, `p:cmLst/p:cm` | `ppt/comments/modernComment_{sldId:X}_{cId:X}.xml`, `p188:cmLst/p188:cm` |
| slide → part | `…/officeDocument/2006/relationships/comments` | `http://schemas.microsoft.com/office/2018/10/relationships/comments`, **plus** `p:sld/p:extLst/p:ext{6950BFC3-…}/p188:commentRel r:id` |
| authors | `ppt/commentAuthors.xml`, `p:cmAuthor id name initials lastIdx clrIdx` (+ `p15:presenceInfo`) | `ppt/authors.xml`, `p188:author id name initials userId providerId` (GUID ids) |
| anchor | a slide position `p:pos` | moniker lists: `pc:sldMkLst`, `ac:deMkLst`, `ac:txMkLst`, … |
| replies | flat, `p15:threadingInfo/p15:parentCm` | `p188:replyLst/p188:reply` |
| status | none | `@status` = `active` / `resolved` / `closed` |

### What the corpus shows

From `results/comments-748.txt` (748 presentations from POI, LibreOffice,
ShapeCrawler, Open XML SDK, python-pptx and the fixtures; 84 Strict packages
skipped):

| Topic | Finding | Consequence |
|---|---|---|
| frequency | legacy comments in **5** decks (7 parts, 11 `p:cm`); modern comments in **2** decks (2 parts, 2 `p188:cm`, 2 replies); **0** decks and 0 slides have both | corpus evidence is thin. P4a-0 (§12) makes PowerPoint fixtures for every form we write, as #114 did for shape anchors |
| registries outlive comments | 32 decks have `commentAuthors.xml` and no legacy comment; 1 has `authors.xml` and no modern comment; `tdf147586.pptx` has both registries and no comment | authors are append-only. Deleting comments never removes an author (§6) |
| part name | 2/2 `modernComment_{sldId}_{p14:creationId}` in upper-case hex without padding, both matching the slide | §3.1 |
| content types | comments `application/vnd.ms-powerpoint.comments+xml` (2/2); authors `application/vnd.ms-powerpoint.authors+xml` at `ppt/authors.xml` (3/3) | §3.1, §4 |
| slide reference | 2/2 slides carry exactly one `p188:commentRel` in `p:sld/p:extLst/p:ext uri="{6950BFC3-D8DA-4A85-94F7-54DA5524770B}"`, whose `r:id` matches the relationship, and `p:extLst` is the slide's last child | §3.2 |
| namespaces | the root declares `a`, `r`, `p188`; `ac`/`pc` are declared **locally on each moniker** (2 `ac`, 4 `pc`) | new parts copy this form byte for byte (§3.1) |
| `mc:AlternateContent` | **0** in comment parts, in author parts and on slides with comments; no presentation `p:extLst` URI relates to comments (the corpus has `{EFAFB233-…}` 291, `{2D200454-…}` 56, `{521415D9-…}` 21) | nothing is written to `presentation.xml`; AC in a comment or author part is refused (§9) |
| anchors | 2/2 `ac:deMkLst[pc:docMk, pc:sldMk, ac:spMk]` on a top-level `p:sp`; `sldMk@sldId`/`@cId` match the slide; `spMk@creationId` = the shape's `a16:creationId` (PowerPoint file) or all-zero (LibreOffice-touched file); no `p188:pos`, no text offsets, no `ac:context` | §2 |
| `p188:cm` | children `ANCHOR, [replyLst,] txBody`; no `status`, no `p188:extLst`, no other attribute; `txBody` = `a:bodyPr, a:lstStyle, a:p+`, each `a:p` = `a:r(a:rPr lang, a:t)` (4/4 bodies) | §3.3, §5 |
| ids | 4/4 comment and reply ids are braced, upper-case RFC 4122 **v4** GUIDs (1 from the PowerPoint fixture, 3 from `modern-comments.pptx`, whose producer is PowerPoint 16 but whose authors lack required attributes); 3/3 author ids are braced and upper-case but **not** v4 (version nibbles `A`, `3`, `F`; 2/3 have a non-RFC variant) | random v4 for comments and replies. PowerPoint's author-id derivation is unknown *(u)*, so authors get v4 too (§4) |
| timestamps | `created` 4/4 and legacy `dt` 8/11 are `YYYY-MM-DDTHH:MM:SS.mmm` with no zone; the other 3 come from LibreOffice (9 digits) | millisecond precision, no zone (§5) |
| authors | PowerPoint files: all five attributes; `providerId` `AD` (`userId` `S::<upn>::<guid>`, `tdf147586`), or `None` with `userId` = name in the project fixture, whose identity was **edited by hand** (so this form is *accepted* by PowerPoint, not shown to be *written* by it); legacy `p15:presenceInfo`: `AD` 13, `None` 4 (`userId` = name, all from PowerPoint), `Windows Live` 3; `libreoffice/modern-comments.pptx` lacks `userId`/`providerId` (required by the SDK schema) and does not open in PowerPoint 365 (#114; the causal link is *(u)*) | `userId`/`providerId` are always written; the local form is decided by P4a-0 F8 (§4) |
| legacy colour | `clrIdx` ≠ `id` in 37 of 63 legacy authors | a legacy writer could not derive `clrIdx`; modern authors have no colour index |
| creation ids | slides: 820/1,580 have `p14:creationId`; drawing elements: 1,217/7,629 have `a16:creationId`, 2,738/7,629 are nested in groups | §2, §3.2: both cases are common and must be decided |

The schema (SDK data, `schemas_microsoft_com_office_powerpoint_2018_8_main.json`)
says:

- `p188:author` requires `id`, `name`, `userId` and `providerId`.
- `p188:cm` and `p188:reply` require `id`, `authorId` and `created`.
- `p188:cm` has exactly one anchor from a choice of ten. These are `pc:sldMkLst`,
  `pc:sldLayoutMkLst`, `pc:sldMasterMkLst`, `ac:deMkLst`, `ac:txBodyMkLst`,
  `ac:txMkLst`, `ac:tcMkLst`, `ac:trMkLst`, `ac:gridColMkLst` and
  `p188:unknownAnchor`.
- The anchor is followed by `pos?`, `replyLst?`, `txBody?` and `extLst?`.
- `status` ∈ {`active`, `resolved`, `closed`}.
- `pc:sldMk` requires `sldId` (`cId` is optional).
- `ac:spMk`/`grpSpMk` require `id` (`creationId` is optional).
- The text-range context `ac:context` requires a **`hash`** whose algorithm
  is not documented.

### Reference implementations

- **LibreOffice** `2e885d0b` (`sd/source/filter/eppt/pptx-epptooxml.cxx`)
  writes a modern part next to the legacy one for every slide
  (L2342–2355). Its modern comments have **no anchor**, which the schema
  requires (L2046–2079). It writes no `p188:commentRel` and no
  `userId`/`providerId` (L1828–1853). Its ids are random GUIDs, and authors
  are keyed by name only (L1855–1864).
- **Apache POI** `47dcff76` writes legacy comments only (`XSLFComments`).
- **python-pptx** 1.0.2 neither reads nor writes comments.
- **No reference writer** produces what PowerPoint produces. The
  PowerPoint-made fixture and the P4a-0 fixtures are therefore the
  specification, and the oracles (§11) check against them.

## Decision

### 1. Scope

**In Phase 4a**, all operations on modern comments of one slide:

| Operation | Notes |
|---|---|
| add a comment thread | anchors: the slide, or a top-level drawing element by shape id (§2) |
| reply to a thread | one level, as `p188:replyLst` allows |
| edit the text of a thread or a reply | plain text only (§5) |
| resolve / unresolve a thread | `@status` (§5) |
| delete a reply; delete a thread with its replies | the last thread removes the part (§6) |
| find or add an author | inside the comment's own transaction (§4) |

**Not in Phase 4a** (refused or not offered):

- **Text-range anchors** (`ac:txMkLst`). PowerPoint's `ac:context@hash` is
  required and its algorithm is unknown. A wrong hash is silently wrong,
  so these anchors are refused (`UnsupportedCommentAnchor`).
- **Layout, master, table-cell, table-row and column anchors.**
- **Anchors on shapes nested in groups.** They come later, after a PowerPoint
  fixture shows their moniker chain (P4a-0 F4).
- **Writing legacy comments.** Legacy comments stay read-only: they cannot
  be edited, deleted or converted (§7).
- **Mentions, likes, tasks** (`assignedTo`, `startDate`, `dueDate`,
  `priority`, `complete`, `title`, `tags`). These are kept as they are,
  never written.

**Principles** (ADR-0001):

- One plan per operation (ADR-0012 §2). Every change is a staged clone or a
  new part in one `RegistryPlan`, applied once. **No in-place edit follows
  `apply`**, so there is no poisoning step (contrast ADR-0017 §2.1).
- Every refusal happens before `apply`, so `Err ⇒ byte-identical`.
- **T2 locality.** Only the parts listed for an operation in §11 change, and
  within them only the touched elements.
- **Fail closed.** A comment part or authors part that this ADR cannot fully
  account for is not edited (§9).
- **Never repair.** An existing part in a form we would not write (for
  example LibreOffice's anchorless comments, or no `commentRel`) is left as
  it is. Edits to it are allowed only where §9 says so.

### 2. Anchors

```rust
pub enum CommentAnchor {
    /// pc:sldMkLst [pc:docMk, pc:sldMk]
    Slide,
    /// ac:deMkLst [pc:docMk, pc:sldMk, ac:<kind>Mk id creationId?]
    Shape(u32),
}
```

- **`pc:sldMk`** carries `sldId` (the slide's `p:sldId@id`) and `cId` (the
  slide's `p14:creationId@val`, which §3.2 guarantees exists).
- **`Shape(id)`** resolves the id like `shape_mut_by_id`. These Phase 0
  errors apply: `ShapeNotFound`, `AmbiguousShapeId`, `InvalidShapeId`, and
  `InsideAlternateContent` for a target inside AC. The shape must be a
  **direct child of `p:spTree`**. A nested one is `UnsupportedCommentAnchor`
  until P4a-0 F4 shows PowerPoint's chain (2,738 of 7,629 corpus drawing
  elements are nested).
- **Moniker by element kind:** `p:sp` → `ac:spMk`, `p:pic` → `ac:picMk`,
  `p:cxnSp` → `ac:cxnSpMk`, `p:graphicFrame` → `ac:graphicFrameMk`,
  `p:grpSp` → `ac:grpSpMk` (all in the SDK data). Only `spMk` is
  corpus-evidenced. The other four are written only after P4a-0 F4 confirms
  them, and until then they are `UnsupportedCommentAnchor`. `p:contentPart`
  is always refused.
- **`creationId`** on the moniker is the shape's
  `p:cNvPr/a:extLst/a:ext{FF2B5EF4-…}/a16:creationId@id`, copied verbatim
  (matches 1/1 PowerPoint file). If the shape has none (6,412 of 7,629 drawing
  elements), the attribute is **omitted**, which the schema allows. The
  shape is not edited. Q3 covers this, with manual check M3.
- **Slide anchors** have no corpus example. The `pc:sldMkLst` form and
  whether PowerPoint adds `p188:pos` are confirmed by P4a-0 F1 before P4a-1
  merges. Until then this section is the hypothesis: no `pos`.
- **ADR-0005 §6 and ADR-0008 §3.3.** Both anchor kinds classify as *safe*:
  `sldMkLst`, and `deMkLst` without text offsets. So a new comment adds no
  text lock, and `Slide::comments()` reads it back as `Anchor::Slide` or
  `Anchor::Shapes([id])`. A test asserts both, so the writer and the shared
  classifier cannot drift.
- **`next_shape_id`** already counts every `ac:*Mk@id` (ADR-0012 §3). New
  monikers name existing shapes only, so the allocation is unchanged.

### 3. Parts, relationships and the slide

#### 3.1 The comment part

- **One part per slide.** When the slide already has one modern comments
  relationship, new threads are appended to that part. When it has none, a
  part is created.
- **Name:** `PartNameRequest::Exact("ppt/comments/modernComment_{sldId:X}_{cId:X}.xml")`,
  in upper-case hex without padding (2/2). `Exact` allows a name removed earlier in the same plan or session
  (`add.rs`: "A name removed earlier in the plan or session may be
  reused"). So after §6 removes a slide's last thread, the next comment on
  the same slide asks for, and gets, the **same** name: `sldId` and `cId`
  are unchanged. Every other conflict (an existing part, or a dangling
  relationship target from an earlier session) is refused
  (`Core(RegistryRefused { NameInUse })`) and nothing is renamed. Such a
  conflict needs a stale part with exactly that `sldId`/`cId` pair.
- **Content type:** `Override` `application/vnd.ms-powerpoint.comments+xml`.
- **Relationship:** slide → part, type
  `http://schemas.microsoft.com/office/2018/10/relationships/comments`,
  `rId{max+1}`.
- **Bytes of a new part**, PowerPoint's form: an XML declaration with
  `standalone="yes"` and CRLF, then
  `<p188:cmLst xmlns:a="…" xmlns:r="…" xmlns:p188="…">`. In each anchor,
  `ac:`/`pc:` are declared on the anchor list and on each `pc:` moniker
  exactly as in the fixture.
- **Appending to an existing part:**
  - The new `p188:cm` goes at the end of `p188:cmLst`. If the list ends
    with `p188:extLst` (schema: `cm*` only; 0 in the corpus), it is refused.
  - The new element declares `p188`, `a`, `ac`, `pc` locally unless the
    root binds that prefix to the same URI. If a prefix is bound to another
    URI in scope, the operation is refused (`CommentPartNotEditable`). This
    is the xlsx review M3 lesson, ADR-0017 §5: decide by URI, not by prefix
    string.

#### 3.2 The slide

A **new** part needs two edits to the slide part, both made in one staged
clone:

1. **`p188:commentRel`.** If the slide has no `p:extLst`, append one as the
   last child of `p:sld` (2/2 slides have it last). Then append
   `<p:ext uri="{6950BFC3-D8DA-4A85-94F7-54DA5524770B}"><p188:commentRel xmlns:p188="…" r:id="rIdN"/></p:ext>`.
   - If `r` is not in scope at that point, it is declared locally.
   - An existing `p:ext` with that URI on a slide without a modern
     relationship is refused (`CommentPartNotEditable`).
   - A preceding AC sibling (an AC-wrapped transition or timing,
     `bug68703.pptx`) is handled by ADR-0002's "later rule" through the
     lens. If the lens cannot order the insertion, the operation is refused
     (`Core(UnresolvedSibling)`).
2. **`p14:creationId`.** A slide without one gets one: 760 of 1,580 corpus
   slides lack it, and so does every slide made by `add_slide`
   (`new_slide_xml`). The part name and `pc:sldMk@cId` need it (Q4). A new
   function `insert_creation_id` does it, because the existing
   `set_creation_id` only rewrites an existing value and refuses a slide
   without `p:cSld/p:extLst`:
   - **Where.** At `p:cSld/p:extLst/p:ext{BB962C8B-B14F-4D97-AF65-F5344CB8AC3E}/p14:creationId val`,
     where PowerPoint writes it (ADR-0012 post-acceptance note).
     - A missing `p:cSld/p:extLst` is inserted through the lens, which puts it
       after `bg?, spTree, custDataLst?, controls?` (schema order: it is
       `p:cSld`'s last child).
     - The new `p:ext` is appended at the end of an existing `p:cSld/p:extLst`.
   - **Namespace.** `p14` is declared on the `p14:creationId` element
     itself, as PowerPoint writes it (2/2 fixtures). This is
     `<p14:creationId xmlns:p14="…/powerpoint/2010/main" val="…"/>`.
   - **Value.** ADR-0012 §3: the maximum over all slides and notes slides,
     plus 1. Overflow is `CreationIdsExhausted`.
   - **Refused (fail closed, `UnsupportedContent`, as ADR-0012):**
     - a `p14:creationId` anywhere else in the slide;
     - a `p:ext{BB962C8B-…}` without `p14:creationId`;
     - an existing value that is not a decimal u32. Its `cId` cannot be
       written, and replacing it would change an id other tools may hold.
       The same refusal applies when an existing, well-placed value is
       read for the part name and `sldMk@cId`.

Appending to an existing part does **not** touch the slide.

### 4. Authors

```rust
pub struct CommentAuthor {
    pub name: String,
    pub identity: AuthorIdentity,
    pub initials: Option<String>,
}
pub enum AuthorIdentity {
    /// default: userId = name, providerId = "None"; final form from P4a-0 F8
    Local,
    /// e.g. "AD" and "S::<upn>::<guid>"; provider_id must not be "None"
    Account { user_id: String, provider_id: String },
}
```

- **The registry** is the part that the presentation's relationship of
  type `http://schemas.microsoft.com/office/2018/10/relationships/authors`
  points to.
  - If it is absent, it is created in the same plan:
    `Exact("ppt/authors.xml")` (3/3), CT
    `application/vnd.ms-powerpoint.authors+xml`, root
    `p188:authorLst` declaring `a`, `r`, `p188` (PowerPoint's form).
  - More than one such relationship, or a target that is not
    `p188:authorLst`, is refused (`AuthorsPartNotEditable`).
  - `p188:CT_AuthorList` is `author*` only (SDK data). Any other child of
    `authorLst`, including a `p188:extLst`, or `mc:AlternateContent`
    anywhere in the part, is refused (`AuthorsPartNotEditable`). This is
    the same rule as `cmLst` (§3.1).
- **The `Local` form.** `userId` = name and `providerId="None"` is accepted
  by PowerPoint: the hand-edited project fixture opens and keeps its
  comment, and the legacy `p15:presenceInfo` has this form 4 times in
  PowerPoint files. Whether PowerPoint **writes** it for a signed-out user
  is *(u)*. P4a-0 F8 records it, and its form replaces this default by a
  post-acceptance note before P4a-1 merges.
- **Kind of an author** (Q5, owner decision). An author's kind follows
  from its `providerId`:
  - `providerId="None"` is **Local**;
  - any other non-empty `providerId` is **Account**;
  - an author without `providerId` (LibreOffice's form) has no kind and
    never matches. A new entry is added instead, and the old one is not
    repaired.

  The caller's kind is its `AuthorIdentity`. `Account { provider_id:
  "None", .. }` is refused (`InvalidCommentAuthor`), because it would be
  Local by this definition.
- **Matching** (Q5, owner decision). The comparison is case-sensitive and
  exact.
  - **Account:** an existing Account author is reused when `userId` and
    `providerId` are equal. **`name` is not compared.**
  - **Local:** an existing Local author is reused when `name` is equal.
    `userId` is not compared, so an author written in F8's form (whatever
    `userId` PowerPoint uses) still matches by name.
  - **Display name differs** (Account): the existing author is reused and
    its `name` is **not** edited. The authors part stays byte-identical,
    and the comment shows the stored name. Renaming an author is an edit of
    an existing entry, which §4 never makes (append-only). Read-back via
    `Slide::comments()` reports the stored name.
  - `initials` is not part of either key, and a reused author's initials
    are never changed.
  - Two existing authors with the same key: the first in document order is
    used.
  - Manual check M10 checks the Account rule in PowerPoint: the same
    account under a different display name is treated as the same user,
    and PowerPoint adds no second author for it.
- **New author:**
  - `id` is a new GUID (§5).
  - `initials` is the caller's value, else the upper-cased first letter of
    the first and of the last whitespace-separated word of `name`. This
    gives `AL`, `JM`, `CA` for the corpus names. A one-word name gives one
    letter, and an empty result omits the attribute.
  - Appended at the end of `authorLst`.
  - **Namespaces, by URI (§3.1, ADR-0017 M3):** the new element declares
    `p188` locally unless the root binds that prefix to the same URI. If
    the prefix is bound to another URI in scope, the operation is refused
    (`AuthorsPartNotEditable`).
  - Initials for non-Latin names (`山田 太郎` gives `山太`) may not match
    PowerPoint's rule *(u)*. P4a-0 F2 uses a Japanese second author, and M4
    records PowerPoint's initials.
- **Validation** (`InvalidCommentAuthor { reason }`):
  - `name`, `userId` and `providerId` must be non-empty and free of
    characters invalid in XML;
  - a caller's `initials` must also be non-empty and free of such
    characters, and must not contain whitespace.
- **No colour index.** Modern authors have none. Legacy `clrIdx` (≠ `id`
  in 37 of 63) is irrelevant because legacy authors are never written.
- **Append-only.** Authors are never removed or edited, even when their
  last comment is deleted (32 corpus decks keep a registry with no comment of its kind).
  `commentAuthors.xml` is never touched.
- **No separate `author()` call.** This differs from xlsx's
  `Workbook::person`. The author is found or added inside the comment's
  plan, so a refused comment leaves no orphan author and there is one
  transaction (alternatives, 5).

### 5. Ids, timestamps, text and status

**Ids (Q1).** Comment, reply and new author ids are random RFC 4122 v4
GUIDs, braced and upper-case. This is PowerPoint's form for comments
(4/4), LibreOffice's for all three, and ADR-0017 Q4's decision for xlsx.

- Bytes come from `getrandom`, already a workspace dependency.
- A failure is `Error::Random(String)`, as in xlsx.
- **Uniqueness:** each candidate is checked, case-insensitively, against
  every `p188:cm@id`, `p188:reply@id` and `p188:author@id` in **every**
  part with the **modern** comments content type
  (`application/vnd.ms-powerpoint.comments+xml`) or the authors content
  type. Every such part is scanned, not just the related ones (ADR-0017
  review m10). Legacy comment and author parts are not scanned: they
  carry no GUIDs.
  - The scan is a read, not an edit. It collects `@id`s even from parts
    that §9 would refuse to **edit** (AC, unknown children, children out of
    order).
  - Only a part that does not **parse** (malformed, or past the depth
    limit) refuses the add or reply, even on another slide, because its
    ids are unknown. The error is `CommentPartNotEditable` /
    `AuthorsPartNotEditable` with `reason` = `"id scan: <part> does not
    parse"`. This is fail closed, and acceptable because the API is new.
  - The check also covers
  every id given out or removed in this session (a session set, seeded on
  first use like P5c). Up to 32 draws are made, then `Error::Random`.
- A `#[cfg(test)]` hook injects a deterministic byte source, as in xlsx.
- **Why not UUID v8 (ADR-0016 P3e).** Table style ids are document-local
  names, where reproducible output helps. Comment ids are identities that
  co-authoring and PowerPoint's comment service match across copies of a
  file. A content-derived id would collide when the same template is
  commented twice, or when slides with comments are reused between decks
  *(u)*.
- PowerPoint's author ids look derived from the account (not v4) *(u)*.
  Since the derivation is unknown, a v4 id is used. Only uniqueness is
  required.

**Timestamps (Q6).** `created` is the caller's `SystemTime`, or now,
formatted in **UTC at millisecond precision without a zone**:
`YYYY-MM-DDTHH:MM:SS.mmm`. This is PowerPoint's form in 4/4 modern and
8/11 legacy timestamps. It differs from xlsx P5c (seconds) because
PowerPoint writes milliseconds. Whether PowerPoint means UTC or local time
is *(u)*. P4a-0 F7 records it from a VM set to UTC+9, and §5 changes by a
post-acceptance note if PowerPoint writes local time. A `created` outside
years 1970–9999 UTC (before the Unix epoch, or with more than four year
digits) is refused (`InvalidCommentTime`). A reply's `created`
is not required to follow its parent's (0 corpus violations; not enforced).

**Text.** A comment's text is a `&str`.

- `\n` separates paragraphs, as PowerPoint and LibreOffice write one
  `a:p` per line.
- **The character rules are ADR-0005's `TextBodyMut::set_text` table**
  (`text.rs`), so that text read with `Slide::comments()` can be written
  back unchanged:
  - `\r\n` and a lone `\r` become `\n`;
  - `\v` becomes an `a:br` inside the paragraph. The reader returns
    `a:br` as `\u{b}` (`read.rs`), and §5's plain body allows `a:br`;
  - `\t` is kept as text;
  - any other character invalid in XML is `InvalidXmlChar`.
- A new body is
  `<p188:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:rPr lang="…"/><a:t>…</a:t></a:r></a:p>…</p188:txBody>`.
  - `lang` is the caller's `NewComment::lang`, else `en-US`. PowerPoint
    always writes `lang` (4/4). A caller's `lang` must match BCP 47's
    shape `[A-Za-z]{2,8}(-[A-Za-z0-9]{1,8})*`; anything else is
    `InvalidCommentLang`.
  - An empty line is `<a:p><a:endParaRPr lang="…"/></a:p>`.
- Empty text is refused (`EmptyCommentText`; PowerPoint cannot post one).

**Editing text** replaces the `a:p` children of the target's
`p188:txBody`. `a:bodyPr`/`a:lstStyle` are kept, as is the first run's
`a:rPr` (as ADR-0005's paragraph `set_text` keeps it). It is refused
(`CommentBodyNotPlain { id }`) when:

- the body contains anything except `a:bodyPr`, `a:lstStyle`, `a:p`, and
  inside `a:p` the elements `a:pPr`, `a:r`, `a:br` and `a:endParaRPr`, with
  `a:r` = `a:rPr` (attributes only) + `a:t`. This covers mentions and links,
  whose markup is not in the corpus *(u)*;
- the thread has a `p188:extLst`, which may carry mention or task data
  that points into the text *(u)*.

Other attributes (`likes`, `tags`, task attributes) are kept.

**Same text.** If the normalised new text equals the text
`Slide::comments()` returns for the target, `set_comment_text` returns
`Ok` and changes nothing: the output is byte-identical. Without this rule a
rewrite would drop the later paragraphs' `a:pPr`/`a:rPr`. The comparison
uses the normalised string, so `a\r\nb` equals a stored `a\nb`. The
body checks above run first, so a body that is not plain is still refused.

**Status (Q7).**

- `set_resolved(true)` writes `status="resolved"` on the `p188:cm`.
- `set_resolved(false)` removes a `resolved` value, back to the corpus's
  form for an open thread (2/2 have no `status`). An absent status or
  `active` is a no-op that leaves the bytes unchanged.
- P4a-0 F3 records what PowerPoint writes on reopen. If PowerPoint writes
  `active`, a post-acceptance note switches to it.
- `closed` (meaning undocumented) is refused (`CommentClosed`) by **every**
  operation on that thread or its replies: reply, edit, resolve and
  unresolve, delete. A closed thread may belong to a workflow (tasks) this
  ADR does not model.
- Reply `status` is never written.

**Existing threads with anchors §1 cannot write** (`ac:txMkLst`, table,
layout or master anchors, `p188:unknownAnchor`, a nested shape): reply,
edit, resolve and delete are **allowed**. These operations never read or
write the anchor element: the anchor's bytes stay identical (a locality
check), and ADR-0005's text locks keep using it. A `p188:cm` with **no**
anchor (LibreOffice's form) is allowed on the same terms.

### 6. Delete

- **Deleting a reply** removes that `p188:reply`, and the `p188:replyLst`
  when it becomes empty.
- **Deleting a thread** removes the `p188:cm` with its replies.
- **Deleting the last `p188:cm` of a part** removes the part instead of
  leaving an empty `cmLst`. The plan has these steps:
  1. `remove_parts([part], [MODERN_COMMENTS_REL])` removes the part and
     its `Override`;
  2. the slide relationship is removed;
  3. only the `p:ext{6950BFC3-…}` whose `p188:commentRel@r:id` equals the
     removed relationship's `Id` is removed, and `p:extLst` too when it
     becomes empty. This uses a new targeted function
     `drop_comment_rel_for(doc, rid)`. The existing `drop_comment_rel`
     removes **every** `commentRel`, which is right for a duplicate (all
     comment relationships are dropped there) but would cut another part's
     reference on a slide with two modern relationships (review M1). A
     `p:ext` holding other children besides that `commentRel` is refused
     (`CommentPartNotEditable`, 0 in the corpus).
     If no `commentRel` names the `Id` (LibreOffice's form), the slide
     is not edited.
  The slide's `p14:creationId` stays. What PowerPoint does is checked by
  manual check M6.
- **Ids of deleted comments** stay in the session set and are never reused
  (§5).
- **Authors are never removed** (§4).

### 7. Legacy comments and mixed decks (Q2)

Corpus: legacy comments in 5 decks. There are 0 decks with both kinds, and
0 slides with both.

- **Phase 4a writes modern comments only.** Writing legacy comments is not
  offered:
  - the roadmap item is modern;
  - PowerPoint 365 creates only modern ones;
  - legacy replies need `p15` threading;
  - `clrIdx`/`lastIdx` are per-author counters.
- **A slide with a legacy comments relationship** refuses every Phase 4a
  write on that slide (`LegacyCommentsOnSlide { slide }`). Whether
  PowerPoint shows, merges or drops a slide carrying both is unknown, and
  the corpus has no such slide.
- **Other slides of a deck with legacy comments** may get modern comments.
  The two registries coexist, as `tdf147586.pptx` already shows.
  - P4a-0 F6 checks this in PowerPoint: `45545_Comment.pptx` with a modern
    comment added on a slide without legacy comments.
  - If PowerPoint repairs, converts or warns, a post-acceptance note
    widens the refusal to the whole deck. That is Q2's alternative.
- **Converting legacy comments to modern is not offered.** It is lossy
  (`p:pos` has no moniker; author identities are guessed) and it is
  PowerPoint's UI action. This mirrors ADR-0017 §5 (`NoteExists`) and
  ADR-0015 (no conversion of pre-release `commentsExtended`).
- **Writing both**, as LibreOffice does, is rejected (alternatives, 2).

### 8. Interaction with Phases 0–3

- **ADR-0012 `delete_slide`.** It already cascades both comment types and
  keeps both author registries. The session id set (§5) is seeded from
  the package by whichever comes first: the first comment write, or a
  `delete_slide` that removes a modern comment part. So the ids of a
  deleted slide's comments stay reserved for the session, as removed thread
  GUIDs do in xlsx P5c.
  - **No new `Err` in `delete_slide`.** When it seeds the set, it uses the
    same scan as §5 (modern comment and authors parts, ids collected from
    every part that parses) and **skips** a part that does not parse. That
    is not a refusal there.
  - The reservation is insurance for the deterministic test source: two
    random v4 ids collide with probability about 2^-122.
  - **The asymmetry is intended.** An add or reply creates a new id, so it
    fails closed when some ids are unknown. `delete_slide` creates none:
    it only reserves ids, so an existing operation must not gain an error
    for a part it does not need.
- **`duplicate_slide`.** It already drops comments and gives the copy a new
  `p14:creationId`, so a comment added to the copy gets a different part name.
- **`move_slide`.** Monikers name `sldId`/`cId`, not positions, so nothing
  changes.
- **ADR-0005 locks.** New anchors are safe (§2). Comment parts written in
  this session are rescanned after `apply` (`finish` → `rescan`), so later
  text edits see them.
- **`next_shape_id`.** No change (§2). No phase deletes shapes yet. When one
  does, deleting a comment-anchored shape needs its own rule (out of scope;
  ADR-0012 §3 already asks for a per-slide high-water mark then).
- **ADR-0016 P3e** uses UUID v8 for table styles. The two allocators are
  independent, and comment ids never take part in style-id collision checks.

### 9. Errors

New variants. Every one of them is raised before `apply`, so each has an
`Err ⇒ byte-identical` test.

| Error | Raised when |
|---|---|
| `CommentNotFound { id }` | no `p188:cm`/`p188:reply` with that id (case-insensitive) in the slide's modern comment parts; a thread operation (reply, resolve) given a reply id |
| `UnsupportedCommentAnchor(String)` | text-range anchor requested; a nested shape; a moniker kind not yet confirmed (§2); `p:contentPart`; the target inside AC (also `InsideAlternateContent`, Phase 0's error, which takes precedence) |
| `LegacyCommentsOnSlide { slide }` | §7 |
| `CommentPartNotEditable { part, reason }` | the part is missing, unreadable, beyond the depth limit, or its root is not `p188:cmLst`; an unknown child of `cmLst`; `mc:AlternateContent` anywhere in it; the part is the target of more than one relationship, or of a relationship from another slide (ownership ambiguous, ADR-0017 P5c note); the slide has two or more modern relationships (add only); `p:ext{6950BFC3-…}` without a relationship; a prefix bound to another URI (§3.1); the edited `p188:cm`/`p188:reply` has children out of schema order; a modern comment part in the package-wide id scan that does not parse (`reason` "id scan: …", §5); deleting the last thread when the `p:ext` holding its `commentRel` has other children (§6) |
| `AuthorsPartNotEditable { part, reason }` | the same structural reasons for the authors part, including any child of `authorLst` other than `p188:author` (§4); two authors relationships; a duplicate author `id`; an authors part in the id scan that does not parse (§5) |
| `InvalidCommentAuthor { reason }` | §4 validation |
| `EmptyCommentText` | §5 |
| `CommentBodyNotPlain { id }` | §5 |
| `CommentClosed { id }` | any operation on a `closed` thread or its replies (§5) |
| `InvalidCommentLang`, `InvalidCommentTime` | §5 |
| `Random(String)` | `getrandom` failure, or 32 colliding draws (§5) |

Reused: `UnsupportedContent` (§3.2: a `p14:creationId` outside its place,
or not a u32), `SlideIndexOutOfRange`, `ShapeNotFound`, `AmbiguousShapeId`,
`InvalidShapeId`, `InsideAlternateContent`, `InvalidXmlChar`,
`CreationIdsExhausted`, `StrictConformance`, and `Core(…)`
(`RegistryRefused`, `UnresolvedSibling`, `TooDeep`, `NamespaceNotInScope`).

**Not refused:**

- an existing part without `p188:commentRel` (LibreOffice's form): edits
  inside it are allowed, and the slide is not repaired;
- authors without `userId`/`providerId`;
- unknown attributes on untouched comments;
- an unresolved `authorId` on an existing comment (`Slide::comments()`
  already reports it).

### 10. API sketch

```rust
pub struct NewComment {
    pub author: CommentAuthor,
    pub text: String,
    pub created: Option<SystemTime>,
    pub lang: Option<String>,
}
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct CommentId(String); // braced upper-case GUID

impl Presentation {
    pub fn add_comment(&mut self, slide: usize, anchor: CommentAnchor, c: NewComment) -> Result<CommentId>;
    pub fn reply_to_comment(&mut self, slide: usize, thread: &CommentId, c: NewComment) -> Result<CommentId>;
    pub fn set_comment_text(&mut self, slide: usize, id: &CommentId, text: &str) -> Result<()>; // thread or reply
    pub fn set_comment_resolved(&mut self, slide: usize, thread: &CommentId, resolved: bool) -> Result<()>;
    pub fn delete_comment(&mut self, slide: usize, id: &CommentId) -> Result<()>; // thread: with replies
}
```

- `CommentId::parse` accepts the forms `Slide::comments()` returns.
- Ids are compared case-insensitively.
- The slide index scopes the lookup, so each operation touches one
  slide's parts only.

**Allowlist (ADR-0002).** These `[read]` entries become `[write]`:
`p188:CT_CommentList/p188:cmLst`, `p188:CT_Comment/p188:cm`,
`p188:CT_CommentReplyList/p188:replyLst`,
`p188:CT_CommentReply/p188:reply`, `a:CT_TextBody/p188:txBody`,
`p188:CT_AuthorList/p188:authorLst` and `p188:CT_Author/p188:author`.

- `p188:CT_CommentRelationship/p188:commentRel` is added as `[write]`.
- The `ac:`/`pc:` monikers stay outside codegen. They are written from
  fixed templates and read as `RawNode` (as `refs.rs` does), because their
  lists are `xsd:any`.
- Existing generated public names do not change. `cargo xtask codegen
  --check` and a name-diff of `generated/pml.rs` in P4a-1 confirm it.

### 11. Test oracles

1. **Locality** (byte-exact). Every part not listed below is L1 (identical),
   and appended registry entries are checked as "original + appended", as
   in PR #38.

   | Operation | Allowed changes |
   |---|---|
   | add, new part | slide: one `p:ext` (+ `p:extLst`); when the slide had no `p14:creationId`, one `p:ext{BB962C8B-…}` in `p:cSld/p:extLst`, and that `p:extLst` itself when the slide had none; slide `.rels` +1; new comment part; CT +1 `Override` (+1 for a new authors part); authors: +1 `p188:author` or a new part with presentation `.rels` +1 |
   | add, existing part | comment part: +1 `p188:cm`; authors as above |
   | reply | comment part: +1 `p188:reply` (+ `p188:replyLst`); authors as above |
   | edit text | the target's `p188:txBody` `a:p` children |
   | resolve / unresolve | one `status` attribute |
   | delete | comment part: the removed element(s); for the last thread, part and `Override` gone, slide `.rels` −1, the **one** `p:ext` naming that relationship (and an emptied `p:extLst`) gone; every other `commentRel` byte-identical |

   `presentation.xml`, `commentAuthors.xml`, legacy comment parts and
   `docProps/*` never change.
2. **`Err ⇒ byte-identical`** for every row of §9, on real files where
   they exist and on in-memory derivations (`SYNTHETIC:`, as in
   `tests/refusals.rs`) otherwise. Examples:
   - text anchor;
   - nested shape (`sample_pptx_grouping_issues.pptx`);
   - legacy slide (`45545_Comment.pptx`);
   - AC in a comment part;
   - a shared comment part;
   - `closed`;
   - a mention-like body;
   - a forced `Random` failure;
   - an unreadable comment part on another slide, for add (refused) and
     for `delete_slide` (still `Ok`, §8);
   - `\r\n` text read back as `\n`, `\v` read back as `\u{b}` (an
     `a:br`); out-of-range `created`; bad `lang` and `initials`;
   - a `p14:creationId` outside `p:cSld/p:extLst`, and one that is not a
     u32.

   Author-key tests (Q5): an `Account` author with the same `userId` and
   `providerId` but a different `name` reuses the existing entry (the
   authors part stays byte-identical, and the comment's `authorId` is the
   existing id); the same `userId` under another `providerId` adds an
   entry; a `Local` author with an equal name but another `userId` is
   reused; an existing author without `providerId` is never reused;
   `Account { provider_id: "None" }` is `InvalidCommentAuthor`.

   Three `Ok` tests are named here because they guard M1, m2 and m12:
   - **same text**: on a thread whose stored text is `a\nb`,
     `set_comment_text("a\nb")` and `set_comment_text("a\r\nb")` both
     return `Ok` and leave the output byte-identical;
   - **two modern relationships on one slide** (`SYNTHETIC:` from the
     project fixture: a second part and `commentRel`). Deleting the last
     thread of one part removes that part, its relationship and its
     `commentRel` only. The other part, relationship and `commentRel` are
     byte-identical;
   - **add → delete the last thread → add** on one slide in one session.
     The part is created again under the same name, the slide's new `rId`
     is above the removed one, and there is exactly one `Override` for it.
3. **XML invariant checker** (new: `crates/ooxml-pptx/tests/oracle/comments.py`,
   standard library only, `python3 -I`). It is written from the
   invariants below, not from the Rust code (ADR-0017's id-space lesson), and
   checks over every output:
   - I1: every modern comments relationship resolves to a part with the
     comments CT whose root is `p188:cmLst`;
   - I2: a slide whose comment part this session created has exactly one
     `commentRel` with that `r:id`;
   - I3: every `authorId` resolves in the single authors part;
   - I4: every id is a braced upper-case GUID, unique package-wide,
     case-insensitively;
   - I5: `pc:sldMk@sldId`/`@cId` equal the slide's;
   - I6: every `ac:*Mk@id` names a shape on the slide, and its `creationId`
     (if any) equals that shape's `a16:creationId`;
   - I7: `created` matches `\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d\.\d{3}` on new
     elements;
   - I8: `status` ∈ the enum;
   - I9: child order and required attributes of every `p188:*` element, read
     from the SDK data JSON;
   - I10: new part names follow §3.1;
   - I11: CT `Override` ↔ part existence;
   - I12: `p14:creationId` values are unique across slides;
   - I13: no author this session **added** shares a key (§4: `userId` +
     `providerId` for Account, `name` for Local) with an author that
     already existed or that this session added earlier. A new comment or
     reply whose author key matches an existing author uses that author's
     id.

   **I3, I4, I6, I12 and I13 compare input and output.** The checker takes
   both packages and fails only on a violation that the input does not
   already have. §9 tolerates some existing problems, which are not
   failures:
   - an unresolved `authorId` (I3);
   - duplicate ids (I4);
   - the all-zero `creationId` of `modern-comments.pptx` (I6);
   - duplicate `p14:creationId`s from slide-copying tools (I12).
   **Elements this session created** must satisfy every invariant outright.
   **Elements it edited** (a `p188:cm` that gained a reply, a new status
   or a new body) are checked only for what was written: the new
   attribute, child or body. So replying to a thread with an all-zero
   `creationId` (I6) or an unresolved `authorId` (I3) passes.

   It runs in the CI python-pptx job on the `oracle_cases` outputs. There,
   python-pptx 1.0.2 also opens every output, and its slide and shape lists
   equal the input's: python-pptx ignores comments, so this proves the rest
   of the deck is intact.
4. **Read-back:** `Slide::comments()` (ADR-0008) returns the written
   author, text, `created`, status, replies and `Anchor::Slide`/`Shapes([id])`.
   This uses the same crate, so it confirms the round trip, not
   correctness.
5. **LibreOffice 25.2.7.2** (the pinned `libreoffice-check` workflow). This
   is an independent reader, not PowerPoint. The tool does not exist yet:
   `tools/lo-check/check.py`'s `comments` key reads `.docx` through
   python-docx (L280–282), and cannot be used for pptx.
   - **New key `odp_annotations`** (implemented in P4a-1). It converts the
     case to ODP and compares, for each slide, `office:annotation` in
     `content.xml`: the count, `dc:creator` and the text, against
     `expected.json`.
   - **Baseline first (P4a-0).** Run the unedited
     `project/powerpoint-shape-comment.pptx` and `libreoffice/modern-comments.pptx`
     through the key, to confirm that 25.2 imports `p188` comments, and to
     record how replies come out (threaded or flat) *(u)*.
   - **If 25.2 does not import them,** the oracle is reduced to "opens and
     re-saves without error", and a post-acceptance note says so.
     LibreOffice rewrites the comment parts when it saves pptx
     (`pptx-epptooxml.cxx` L2342–2355), so a byte comparison of them is
     not possible.
6. **Corpus sweep** over the 748 decks of the spike, adding the committed
   fixtures:
   - on slide 1 (or the first slide that §7 allows), add a slide comment
     and a comment on the first top-level `p:sp`, reply, edit, resolve,
     unresolve and delete;
   - save, reopen, and run 1, 3 and 4;
   - refusals must be byte-identical and are counted by variant;
   - **allowlist.** The allowed refusals are a committed file of
     (deck, error) pairs, `crates/ooxml-pptx/tests/data/comment_sweep_allowlist.txt`:
     one `<deck sha256> <step> <variant>` per line, each with a reason
     comment. Any refusal not listed fails the sweep. So does a listed pair
     that no longer occurs, so the list cannot go stale, and so does any
     `Ok` that breaks oracle 1 or 3. The variants expected to appear in it:
     - `ShapeNotFound`, when the slide has no top-level `p:sp` (the shape
       step only);
     - `UnsupportedContent`, for a `p14:creationId` outside its place or not
       a u32 (§3.2);
     - `CreationIdsExhausted`;
     - `CommentPartNotEditable` / `AuthorsPartNotEditable`, for the decks
       whose existing parts §9 refuses to edit.

     These are never listed: `InsideAlternateContent` and
     `UnsupportedCommentAnchor`, because the step picks a top-level `p:sp`
     outside AC; and `LegacyCommentsOnSlide`, because the step skips such
     slides. A separate case asserts that refusal on `45545_Comment.pptx`.
7. **Mutation tests** (hand-made, each must fail a test):
   - the uniqueness scan limited to related parts;
   - lower-case GUIDs;
   - `created` with seconds or with `Z`;
   - lower-case hex in the part name;
   - `replyLst` written after `txBody`;
   - the Account key including `name` (a renamed account would get a
     second entry);
   - the Account key without `providerId`;
   - the Local key including `userId`;
   - `commentRel` with the wrong `r:id`;
   - the `creationId` addition skipped;
   - an empty `replyLst` kept;
   - `commentRel` removal not filtered by `r:id` (M1; caught by the
     two-relationship test);
   - `p14` declared on the root instead of on `p14:creationId`;
   - `\r` written raw.
8. **PowerPoint (manual, Microsoft 365, owner VM).** No Office in CI, as in
   every ADR. Every case must open without repair, show the comment in the
   Comments pane, and survive a PowerPoint save:
   - **M1** a slide comment on `poi/sample.pptx` (no `creationId`, no
     authors part);
   - **M2** a shape comment on the title of the project fixture (an
     existing part and author);
   - **M3** a shape comment on a shape without `a16:creationId` (Q3:
     still attached after save?);
   - **M4** reply by a second, `Account` author with a Japanese name
     (record PowerPoint's initials, n3);
   - **M5** resolve, then unresolve;
   - **M6** delete the last thread (the slide shows no comment; the part is
     gone after a PowerPoint save);
   - **M7** edit text;
   - **M8** a modern comment on slide 2 of `45545_Comment.pptx` (§7);
   - **M9** a comment on a slide made by `add_slide`;
   - **M10** open the M4 file signed in as the `Account` author's own
     account. Record whether the comment counts as the user's own (edit,
     resolve, delete offered), and whether PowerPoint adds a second
     `p188:author` with the same `userId`/`providerId` on save. Then repeat
     with the account's display name changed in Office, and confirm the
     same (Q1, Q5).

   The cases are added to `examples/manual_check.rs`, with their entries in
   `tools/lo-check/expected.json`.

## Delivery

Three PRs, in order, each reviewed until converged:

0. **P4a-0: PowerPoint fixtures**, made the #114 way: COM plus the Comments
   pane through UI Automation, identity anonymised. Docs and fixtures only.
   - F1: a slide-level comment, with no shape selected.
   - F2: a reply by a second author.
   - F3: a resolved thread, and the same thread reopened.
   - F4: comments on a picture, a connector, a table frame, a group, and a
     shape inside a group.
   - F5: a text-range comment (kept as a refusal fixture).
   - F6: `45545_Comment.pptx` with a modern comment added on a slide without
     legacy comments.
   - F7: a comment made with the VM's zone at UTC+9.
   - F8: a comment made **signed out** (local account, a throwaway name),
     with the author kept exactly as PowerPoint wrote it, not anonymised.
     It decides §4's `Local` form (review m1).
   - F9: two threads on one slide, the second added after the first, to see
     the order of `p188:cm` in the part (§3.1 appends).
   - The LibreOffice baseline run of §11.5 on the two existing fixtures.

   **Acceptance:**
   - the fixtures are in `reference/fixtures/pptx/project/` with
     provenance and SHA-256 in the README;
   - `comments_survey.py` is rerun, and its results replace the *(u)* items
     of §2, §5 and §7 in a post-acceptance note before P4a-1 merges;
   - any form that differs from this ADR is decided there.
1. **P4a-1: allowlist, ids, authors, add.** §2–§4, §5 (ids, timestamps,
   new text), and `add_comment` for the slide anchor and the moniker kinds
   P4a-0 confirmed.
   - Includes the `odp_annotations` key in `tools/lo-check` (§11.5).
   - **Acceptance:** AGENTS.md's definition of done; oracles 1–7 (add
     only); the python job runs `comments.py`; manual cases M1–M3, M9;
     generated names unchanged.
2. **P4a-2: reply, edit, resolve, delete.** §5 (edit, status), §6 and the
   remaining §9 rows.
   - **Acceptance:** oracles 1–7 for every operation, including the
     two-relationship and add → delete-last → add tests; the
     `odp_annotations` check on the new cases; manual cases M4–M8, M10.

## Alternatives considered

1. **Write legacy comments** (POI's only form). It needs a slide position
   instead of an anchor, `clrIdx`/`lastIdx` bookkeeping, and `p15` replies,
   and PowerPoint 365 no longer creates them. Rejected (§7).
2. **Write both kinds per slide** (LibreOffice `2e885d0b`). Every edit then
   has to keep two parts and two registries in agreement. A reader that
   shows both would show duplicates *(u)*. LibreOffice's modern half is
   also schema-invalid (no anchor), so it is not a usable model. Rejected.
3. **Convert legacy comments on write.** Lossy (`p:pos`, author
   identities), and it edits parts the caller did not ask to change (T2).
   Rejected (§7).
4. **Deterministic UUID v8 ids** (ADR-0016 P3e). Reproducible output, but
   it risks cross-file id collisions for identities that services match
   (§5). Rejected; this is Q1.
5. **A separate author-registry call** (`Workbook::person` in xlsx). That
   makes two transactions and allows an orphan author when the comment is
   refused. Rejected: the author is found or added in the comment's plan
   (§4).
6. **Repair foreign parts** (add `commentRel`, `userId`, an anchor):
   edits outside the operation, T2. Rejected (§1, §9 "not refused").
7. **Text-range anchors without `ac:context`**: the `hash` is required,
   and a guessed context could attach the comment to the wrong text with no
   error. Rejected until the algorithm is known (§1).
8. **A slide anchor without `p14:creationId`** (`cId` omitted, part named
   `…_0`). It avoids one slide edit, but no PowerPoint example has this
   form (2/2 have `cId`). Rejected; this is Q4.

## Consequences

- Comments can be added, answered, resolved and removed on existing decks
  with only the comment part, the authors part and (for a first comment)
  the slide changing. No reference library offers modern comment writes.
- Slides made by `add_slide` get a `p14:creationId` the first time they are
  commented on (§3.2).
- Text-range comments, comments on nested shapes and legacy comments stay
  read-only. The refusals say so explicitly.
- Correctness against PowerPoint rests on the P4a-0 fixtures and the manual
  checks. The corpus has only two modern comments, so the corpus sweep
  proves consistency, not PowerPoint acceptance.
- Comment output is not reproducible byte for byte across runs (random
  ids, `now`) unless the caller passes `created`. Ids stay random (Q1).
- The `p188` allowlist moves to `[write]`. Monikers stay template-written,
  so a future anchor kind needs a fixture and a template, not codegen.

## Open questions for the project owner (decided)

The questions as asked, with the recommendation given. The owner's
answers are in the next section.

- **Q1. Id generation.** *Recommended:* random v4 with a test-only
  deterministic source, checked against every comment and author id in the
  package and in the session. This is PowerPoint's form (4/4) and ADR-0017
  Q4. The alternative is UUID v8 derived from content, as P3e does.
  *Caveat:* PowerPoint's **author** ids look derived from the account
  (0/3 are v4). A new `Account` author with a random id might not be
  recognised as the signed-in user. M10 checks this. If it fails, author
  ids need PowerPoint's derivation, and comment and reply ids are
  unaffected.
- **Q2. Mixed decks.** *Recommended:* refuse only on slides that have
  legacy comments (`LegacyCommentsOnSlide`), and allow other slides. Widen
  to the whole deck if P4a-0 F6 / M8 shows PowerPoint warning, repairing or
  converting.
- **Q3. Shapes without `a16:creationId`** (84 % of corpus drawing
  elements). *Recommended:* omit `creationId` on the moniker and leave the
  shape untouched. If M3 shows PowerPoint detaching the comment, add an
  `a16:creationId` to the shape in the same plan (a post-acceptance note).
- **Q4. Slides without `p14:creationId`** (48 % of corpus slides, and every
  `add_slide` slide). *Recommended:* add one (ADR-0012 §3's rule), so the
  part name and `pc:sldMk@cId` take PowerPoint's form.
- **Q5. Author identity key, by kind.** For `Local` authors `userId` =
  name, so "all three fields" and "`userId` + `providerId`" behave
  identically. The choice only matters for `Account` authors whose display
  name changed.
  - *Recommended:* all of `name`, `userId` and `providerId` equal, as in
    xlsx P5c. Existing authors are never edited.
  - *Trade-off:* PowerPoint probably identifies the user by `userId` +
    `providerId` (presence). Then a renamed account gets two entries under
    the recommendation, and two entries for one account may be worse than
    one entry with a stale name.
  - *Alternative:* key `Account` authors by (`userId`, `providerId`) and
    `Local` authors by name.
  - Decide after M10 shows what PowerPoint does with the same account
    under a different name.
- **Q6. Timestamps.** *Recommended:* UTC, millisecond precision, no zone
  suffix, unless P4a-0 F7 shows PowerPoint writing local time.
- **Q7. Unresolve.** *Recommended:* remove `status="resolved"` (the
  corpus's open form), unless P4a-0 F3 shows PowerPoint writing `active`.
- **Q8. Legacy comment edits** (delete or edit an existing `p:cm`).
  *Recommended:* not planned. Revisit if a user asks. Deletion would need
  `lastIdx` and `p15:parentCm` rules.

## Owner decisions (2026-11-21)

- **Q1–Q4 and Q6–Q8: accepted as recommended,** with the fallbacks already
  written:
  - **Q1:** random v4 ids, with a test-only deterministic source, checked
    against the package and the session. If M10 shows that a random author
    id is not recognised as the signed-in user, author ids need PowerPoint's
    derivation (a post-acceptance note); comment and reply ids are
    unaffected.
  - **Q2:** refuse only on slides with legacy comments. Widen to the whole
    deck if F6 / M8 shows PowerPoint warning, repairing or converting.
  - **Q3:** omit `creationId` on the moniker. Add an `a16:creationId` to
    the shape if M3 shows PowerPoint detaching the comment.
  - **Q4:** add a `p14:creationId` to slides that lack one (§3.2).
  - **Q6:** UTC, millisecond precision, no zone, unless F7 shows local
    time.
  - **Q7:** unresolve removes `status="resolved"`, unless F3 shows
    PowerPoint writing `active`.
  - **Q8:** legacy comment edits are not planned.
- **Q5: the alternative.** `Account` authors match on (`userId`,
  `providerId`), and `Local` authors on `name`.
  - The kind follows from `providerId`: `"None"` is Local, and anything
    else is Account.
  - An Account author whose stored display name differs from the caller's
    is reused, and its name is not edited.
  - §4, §11.2, §11.3 (I13) and §11.7 implement this, and M10 checks it in
    PowerPoint.

## Sources

- ECMA-376 Part 1 (5th ed.) §19.4 (legacy comments and authors).
- [MS-PPTX] modern comments: the `p188` element and attribute names, checked
  against `reference/openxml-sdk-data/schemas/schemas_microsoft_com_office_powerpoint_2018_8_main.json`,
  `…_powerpoint_2013_main_command.json` and `…_drawing_2013_main_command.json`.
- Fixtures: `reference/fixtures/pptx/project/powerpoint-shape-comment.pptx`
  (PowerPoint 16.0.20326, #114/#123) and
  `reference/fixtures/pptx/libreoffice/modern-comments.pptx`.
- Corpus and survey: `spikes/pptx-phase4a/README.md`, which gives pins and
  reproduction.
- LibreOffice `2e885d0bda9bc4ee9f3bde51905c4a7820b01f06`,
  `sd/source/filter/eppt/pptx-epptooxml.cxx` L1828–1864, L1988–2085,
  L2342–2355. Apache POI `47dcff769bb395034dddfd59c4609659b6dbc6a4`
  (`XSLFComments`). python-pptx 1.0.2.
- Code in this repository at `988cff1`: `crates/ooxml-pptx/src/deckread.rs`
  (§3.3 reader), `refs.rs` (anchor classifier),
  `slides.rs` (`next_shape_id`, `drop_comment_rel`, `set_creation_id`,
  `COMMENT_TYPES`), `crates/ooxml-xlsx/src/comments/threads.rs`
  (P5c persons and GUIDs).
