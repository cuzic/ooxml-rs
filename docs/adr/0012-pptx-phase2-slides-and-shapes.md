# ADR-0012: PowerPoint Phase 2 — slides and shapes as units

- Status: **Accepted** (revision 3)
- Date: 2026-11-16
- Depends on: [ADR-0001](0001-lossless-round-trip-architecture.md) (Layer 1
  passthrough, Layer 3 append-only registries, From-scratch path),
  [ADR-0002](0002-schema-driven-code-generation.md) (lens, allowlist,
  insertion; post-acceptance note "detach/insert", PR #18),
  [ADR-0005](0005-pptx-phase0.md) (Phase 0/0.1: references into slide text,
  refusals, oracles); ADR-0008 (Phase 1, deck reading and inheritance,
  PR #28) is **not** a prerequisite for its rules, but Phase 2 reuses its
  read APIs (`layouts()` → `LayoutRef`, `sections()`, `notes_mut`) instead
  of defining parallel ones (§11, review m8).
- Background: [`docs/roadmap/pptx.md`](../roadmap/pptx.md) §3.5, §3.6,
  §4 Phase 2
- Evidence: [`spikes/pptx-phase2/`](../../spikes/pptx-phase2/README.md)
  (`units.py`, 91 distinct files, 522 slides; corpus pinned by content in
  `results/corpus-sha256.txt`, output in `results/units-91.txt`);
  python-pptx 1.0.2 and Apache POI (commit `47dcff76`) source, cited per
  rule in §10
- Review: [round 1](0012-opus-review-round1.md),
  [round 2](0012-opus-review-round2.md) (converged: Accepted after Minor only)
- Core prerequisite: the registry transaction of the core PR built on
  branch `core/add-part` (`RegistryPlan`, `Package::apply`; §2), which also
  carries the single ADR-0001 note "registry writes"

## Revision history

- **Revision 1:** initial draft.
- **Revision 2** (after `0012-opus-review-round1.md`, not converged:
  B1, M1–M4, m1–m13). The coordinator decided that the core PR on
  `core/add-part` is the single source of truth for package-level
  writes; §2 now refers to its API instead of defining C1–C6.

  | Finding | Change | Where |
  |---|---|---|
  | B1 validate-before-change impossible with separate primitives | every operation builds its tree edits on clones, stages them (`stage_part`) in **one** `RegistryPlan` with the additions/removals, and applies once; the core's reference scans read the staged content | §2, §4 |
  | M1 duplicate with the same `Id`s | `RegistryPlan::copy_relationships` (same `Id`s, targets re-relativized, `target_map` for the copied notes/tags) | §2, §4.4 |
  | M2 narrow reference check on delete | generalized through `incoming_relationships` (`SlideIsReferenced { from_part, rel_type }`), `viewProps` outline entries removed with their relationship, `sldId` value references (sections updated; Slide Zoom and unknown numeric `sldId` attributes refuse) | §4.2 |
  | M3 ADR-0001 note | one note, in the core PR; this ADR lists its requirements only | §2 |
  | M4 C1–C6 vs the core API | replaced by the core API names | §2, §3 |
  | m1 shape-id maximum | includes numeric `@spid` (incl. `_x0000_s####`) and `ac:*Mk@id` of the slide's comment parts; cap `i32::MAX` | §3 |
  | m2 `sldId` reuse in a session | session high-water mark | §3 |
  | m3 external/unresolvable targets | relationship-only removal | §6 |
  | m4, m5 comments and tags on duplicate | comments dropped (relationship removed in the plan), tags copied | §4.4 |
  | m6 show range | add/duplicate refused too | §4 |
  | m7 placeholder rules | stated | §4.1 |
  | m8 ADR-0008 overlap | `ensure_notes_slide()` only; text through ADR-0008's `NotesMut`; `LayoutRef` | §4.6, §11 |
  | m9 `app.xml` | `Notes` 86/86 and `HiddenSlides` 86/86 measured; `show="false"`; created notes count; unreadable → untouched | §7 |
  | m10 template | `--check` by part content; injectable `core.xml` time; authored without copying; `p:notesSz`, `notesMasterIdLst` | §8 |
  | m11 position 0 | joins the section of the slide now at position 0 | §5 |
  | m12 corpus pin | SHA-256 list committed | spike |
  | m13 comment part template | removed | §3 |
  | Q1–Q4 | the review's recommendations recorded as proposed defaults | Open questions |

- **Revision 3** (after `0012-opus-review-round2.md`: converged, Accepted
  after Minor only, N1 required before acceptance). Status: Accepted.

  | Finding | Change | Where |
  |---|---|---|
  | N1 comment relationships cannot be removed from a copy (`remove_relationship` only sees existing `.rels`) | `duplicate_slide` of a slide (or its notes) with a `comments` relationship is refused (`UnsupportedSlidePart(comments)`) in Phase 2; a relationship-type filter on `copy_relationships` is requested from the core PR (#38), and the refusal is lifted only when that filter lands | §2, §4.4, §9 |
  | N2 `remove_parts` allowed types | listed in full, `slide` included; the `remove_relationship` step is optional | §4.2, §6 |
  | N3 notes and tags relationships on duplicate | the notes copy's types checked with the same set; tags copied per distinct target; a tags part with its own relationships refused | §4.4 |
  | N4 unreadable `.rels` | a core requirement: `incoming_relationships` must refuse (not skip) a relationship part it cannot read; `delete_slide` is refused | §2, §4.2 |
  | N5 viewProps allowlist | `p:outlineViewPr`/`p:sldLst`/`p:sld` added; an emptied `p:sldLst` is kept | §4.2, §11 |
  | N6 high-water holder | the `Presentation` value; lost with it; shape ids need none in Phase 2 | §3 |
  | N7 media content type | `Default` reused / `Override` / fallback to `Override` on `WouldRetypePart` | §4.5 |
  | N8 self-jump links | not remapped: the copy's link still targets the original; manual check | §4.4, Consequences |
  | N9 ADR-0016 notes API, `app.xml` `Notes` | one creation API (`ensure_notes_slide`, extended by ADR-0016 to create the notes master); every creation path counts `Notes` | §4.6, §7 |
  | Q5 | the review's recommendation recorded | Open questions |

- **Post-acceptance note (2026-11-16, owner decision):** Q1–Q5's proposed
  defaults are accepted — the predecessor's section; stale
  `TitlesOfParts` accepted; deleting a referenced slide is refused; the
  16:9 template with the 11 standard layouts; footers not reproduced.
- **Post-acceptance note (2026-11-16, implementation — no change to the
  decisions):** found while implementing `impl/pptx-phase2`:
  - `p14:creationId` sits in `p:cSld/p:extLst/p:ext`
    (`{BB962C8B-…}`; 1,159 of 1,159 corpus slides), not in `p:sld/p:extLst`;
    notes slides carry it in the same place (140 of 471), and the
    duplicated notes slide gets the next value as §4.4 says. A
    `creationId` anywhere else is refused (`UnsupportedContent`) rather
    than copied unchanged.
  - Until ADR-0008 (PR #28) is implemented, `LayoutRef` is defined in
    this crate with the fields §4.1 needs (master index and part, layout
    part, `p:cSld@name`, `@type`); ADR-0008 adopts it.
  - `move_slide` rebuilds the moved `p:sldId` from `id` and `r:id`; a
    `p:sldId` with children or other attributes (0 in the corpus) is
    refused (`UnsupportedContent`). `duplicate_slide(index, None)`
    places the copy right after the original.
  - Shape-id allocation is exposed as `Presentation::next_shape_id(slide)`
    for `add_text_box`/`add_picture`.
  - Cascade relationship types outside §6 found in the wider corpus
    (619 decks): `customXml` (45 slides), `control` (15),
    `legacyDrawing` (1), `chartEx` (4) — refused as §6 says.

## Context

Phase 0/0.1 (ADR-0005) edits text **inside** existing slides and never
creates or removes a part, relationship, content type, slide id or shape
id. Phase 2 is the first PowerPoint phase that does: it adds, deletes,
reorders and duplicates slides, adds text boxes and pictures, and builds a
presentation from a built-in template. Every one of these touches ADR-0001
Layer 3 registries that so far were read-only.

What the corpus says (`units.py` over the POI slideshow test data and the
committed fixtures, 91 distinct readable files, 522 slides, 4,089 shapes
outside `mc:AlternateContent`; numbers from the run of 2026-11-16 quoted
in the spike README):

| Topic | Finding | Consequence |
|---|---|---|
| `p:sldId@id` | lowest id ≠ 256 in 26 files; gaps in 18; **not ascending in presentation order in 20** (reordered decks); max ≥ 1000 in 2 | ids are identities, not positions: allocate `max + 1`, never renumber, never infer order from ids |
| presentation `rId`s | all `rIdN`; contiguous `1..n` in 89, gaps in 2 | allocate an unused `rIdN` (`max + 1`), never renumber |
| slide part names | 522/522 are `ppt/slides/slideN.xml`; **N ≠ position** in 1 file (`airplane-transition.pptx` has only `slide2.xml`); notes parts are **not** numbered like their slide in 101 cases | part names carry no order; pick an unused name; never rename |
| content types | 522/522 slides typed by `Override`; slide media typed by `Default` 149, by `Override` 8 | new slides get an `Override`; new media reuse or add a `Default` for the extension |
| layouts | 1,136 layout parts, 959 used by no slide; 9 files with > 1 master | adding a slide must name its layout explicitly (by master + name/type) |
| notes | 227 of 522 slides have a notes slide; none is shared; every notes slide's back-relationship points to its own slide; 36 files have a notes master | a notes slide belongs to one slide: delete/duplicate it with the slide |
| media | 157 media parts used by slides, **15 shared by > 1 slide**, 1 also used by a layout/master | delete removes a media part only when **nothing** else references it |
| sections (`p14:sectionLst`) | 4 files, 17 sections, **12 empty**; no slide outside every section; section order always equals `sldIdLst` order | keep the invariant "concatenated sections = `sldIdLst`"; empty sections are legal and kept |
| custom shows, show range | `p:custShowLst` 0; `presProps` `p:showPr/p:sldRg` 0 | reference slides by `r:id` / by position: refuse delete/reorder that would invalidate them (rare) |
| slide-to-slide links | 22 slide→slide relationships; 3 slides are jump-link targets | deleting a link target is refused (the other slide keeps a dangling relationship) |
| slide relationship types | `slideLayout` 522, `notesSlide` 227, `image` 198, `hyperlink` 44, `chart` 28, `vmlDrawing` 24, `slide` 22, `oleObject` 19, `diagram*` 18 each, `package` 5, `comments` 3, `control` 2, `tags` 2, `chartEx`, `audio`, `video`, `media`, `hdphoto`, `themeOverride` | 519/522 slides deletable with the cascade of §6; 457/522 duplicable with the conservative type set of §4.4 |
| shape ids | **no duplicate `cNvPr@id` within a slide outside AC**; ids sparse (max > 4·count + 10) on 257 slides; ids above 10⁴ on 146 slides; 5 slides have a non-shape `@id` above the largest shape id | allocate above **every** numeric `@id` in the part, as python-pptx does |
| `p14:creationId` | 172 slides | a duplicated slide needs a new one |
| `docProps/app.xml` | `Slides` equals the slide count in 86/86 files that have it (4 lack it, 1 has no `app.xml`); a "slide titles" heading pair in 78, its count equals the slide count in 77; the heading labels are **localized** (`Slide Titles`, `Folientitel`, `Bildrubriker`, `幻灯片标题`, …) | PowerPoint keeps the counts exact: Phase 2 maintains `Slides`/`Notes`/`HiddenSlides`; the localized title lists are not identifiable reliably (§7) |

The two reference libraries (source read at python-pptx 1.0.2 and POI
`47dcff76`, §10) agree on slide ids (`max(existing, 255) + 1`) and differ
on everything else: python-pptx **renames** every slide part to
`slide1..N` when the slide collection is built, reuses `rId` gaps, cannot
delete or move slides, and computes shape ids as `max(//@id) + 1`; POI
picks the lowest unused part index, reuses the lowest free shape id
(`nextClearBit(1)`), copies layout placeholders **with** their prompt text,
and removes a deleted slide's notes/charts/pictures but not its section
membership, custom-show entries or incoming links. Neither touches
`app.xml`. ooxml-rs can follow neither wholesale: renaming parts violates
ADR-0001 Layer 1, and reusing freed ids can silently retarget references
(animations `p:spTgt@spid`, connectors, modern comments) that the
lossless model promised to keep.

## Decision

### 1. Scope

**In Phase 2:**

1. `add_slide(layout, at)` — a new slide from a layout of a master listed
   in `p:sldMasterIdLst`, inserted at a position (default: the end).
2. `delete_slide(index)` — with the cascade of §6.
3. `move_slide(from, to)`.
4. `duplicate_slide(index, at)` — within the same presentation.
5. On a slide: `add_text_box(rect, text)` and `add_picture(rect, bytes)`
   (PNG, JPEG, GIF).
6. Notes: `ensure_notes_slide(slide)` when the presentation has a notes
   master; notes text through ADR-0008's `NotesMut`.
7. `Presentation::new()` from an embedded, repository-authored template
   (§8).
8. Section membership (`p14:sectionLst`) on add, delete, move, duplicate;
   `docProps/app.xml` counts (§7).

**Not in Phase 2:** copying slides between presentations (needs master,
layout and theme import; Phase 4), new layouts/masters/themes, charts,
tables, SmartArt, OLE objects as new content, group shapes, connectors,
creating a notes master when none exists, custom shows and the slide show
range (only respected), modern comments (only respected), SVG and other
image types, renaming or reordering sections, `Presentation::new()` with
a caller's template other than through `open`.

### 2. Prerequisites in `ooxml-core`: the registry transaction

All package-level writes go through the core registry transaction of the
PR built on `core/add-part` (the single source of truth; links to the PR
replace these names once it is up). Phase 2 uses:

| Core API (`RegistryPlan`, applied by `Package::apply`) | Phase 2 use |
|---|---|
| `add_part(pkg, PartNameRequest::Numbered("ppt/slides/slide{}.xml"), ct, content)` | new slide, notes slide, media, tags parts; numbering is **one more than the largest number in use** (existing parts, `Override`s, relationship targets including dangling ones, names removed in this session, names planned earlier); gaps never filled |
| `add_default_content_type(pkg, ext, ct)` | media extensions (`DefaultExists`, `WouldRetypePart` refusals) |
| `add_relationship(pkg, source, type, Target::Part \| External)` | presentation → slide, slide → layout/notes/media, notes → master/slide; `rId{max+1}`, gaps not reused |
| `copy_relationships(pkg, src, dst, target_map)` | duplicate: the copy's relationships with the **same `Id`s**, internal targets re-relativized for the new part, `target_map` sending `notesSlide`/`tags` to their copies (review M1) |
| `remove_relationship(pkg, source, id)` | the presentation → slide relationship; refused while any attribute of the (staged) source still carries the `Id` |
| `remove_parts(pkg, names, allowed_rel_types)` | the deleted slide and its cascade (§6) **as one set**; the caller lists the relationship types it allows (the interim note's type allowlist; the full list is in §6, `slide` included) |
| `stage_part(pkg, name, content)` | the edited `presentation.xml`, `app.xml`, `viewProps.xml` (and new slide XML) as trees cloned from the parsed parts and edited through the lens; later steps' reference scans read the staged content (review B1) |
| `Package::incoming_relationships(name)`, `part_name_in_use(name)` | the pre-delete reference check (§4.2); must **refuse**, not skip, a relationship part it cannot read (requested below) |

**Transaction shape (every operation, §4).** (1) Clone the parsed trees
that change (`presentation.xml`, sections live in it, `app.xml`,
`viewProps.xml`, the slide itself for shape edits) and edit the clones
through the lens; build new parts as `XmlDocument`s. (2) Run every
ADR-0012 refusal (§9) and record every step in one `RegistryPlan`:
`stage_part` for each edited clone, `add_part`/`add_relationship`/
`copy_relationships` for additions, `remove_relationship`/`remove_parts`
for removals — the plan only reads `&Package`. (3) `Package::apply(plan)`,
which cannot fail after the plan validated. Every refusal, ADR-0012's or
the core's, happens in (1)–(2); the package is then byte-identical,
including `presentation.xml` (T4, §10).

**Requirements on the core PR's ADR-0001 note "registry writes"** (one
note, written there, replacing PR #14's interim note; review M3): a
registry entry is removed only together with the part it registers, or
as a relationship no remaining attribute refers to; removals of parts are
validated **as a set** (relationships from inside the set to inside the
set go with it; any relationship from outside refuses); the caller must
allow each removed relationship's **type** (implicit references by type
stay protected); reference scans read the **staged** content of the same
transaction; no remaining entry is ever renumbered or reused. Format-level
ids (`sldId`, shape ids) are pptx rules (§3, §4.2), not part of the note.

**Requested from the core PR** (beyond what the branch has at the time of
writing): `copy_relationships` should copy an internal target that
resolves to no part (e.g. `Target="#_ftn1"` hyperlinks, 4 in
`with_japanese.pptx`) **verbatim** instead of refusing, so such slides can
be duplicated; until then they are `Core(…)` refusals. Two more
requests (review round 2):

- **N1:** a relationship-type filter on `copy_relationships` (e.g.
  `skip_types: &[&str]`: relationships of those types are not written;
  their `Id`s stay unused, never reallocated). The coordinator has asked
  the #38 owner for it. Until it lands, `duplicate_slide` refuses slides
  with comments (§4.4); once it lands, the comment relationships are
  skipped instead and that refusal goes.
- **N4:** `Package::incoming_relationships` must fail closed: a
  relationship part that cannot be read or parsed is an error (today it is
  skipped), so a reference hidden in a broken `.rels` cannot be missed
  (ADR-0005 §6, "unreadable means locked"). Until then, `ooxml-pptx`
  checks that every `.rels` part parses before `delete_slide`, and refuses
  otherwise.

### 3. Registries and allocation rules

| Registry | Owner | New value | Never |
|---|---|---|---|
| part names | core (`PartNameRequest::Numbered`) | largest number in use + 1 per template (`slide`, `notesSlide`, `media/image`, `media/media`, `tags/tag`); the reason is that a stale reference (a dangling relationship, an external tool's history) to a removed or gap name must never resolve to a new, unrelated part | rename an existing part (python-pptx's `rename_slide_parts`) |
| content types | core | `Override` per new XML part; `Default` per new media extension (core conditions) | edit or remove a `Default` |
| relationship ids | core | `rId{max+1}` per source part; duplicated parts keep the original `Id`s (`copy_relationships`) | reuse a gap (python-pptx `_next_rId`) |
| `p:sldId@id` | `ooxml-pptx` | `max(existing, **every id allocated or removed in this session**, 255) + 1` (session high-water mark, review m2); refused above `ST_SlideId`'s maximum 2,147,483,647 (`SlideIdsExhausted`). Reuse across sessions (an id deleted and saved, then allocated by a later session) cannot be prevented; the new slide's `p14:creationId` differs, so modern comments (`pc:sldMk` carries both) still do not attach to it. The session is the lifetime of the `Presentation` value that holds the high-water mark (review N6): giving the package up (`into_package()`) and opening it again loses it, with the same limit as reuse across sessions | search for a gap (python-pptx's fallback) |
| shape ids (`cNvPr@id`) | `ooxml-pptx`, per slide part | `max + 1` over: every numeric unprefixed `@id` in the part (python-pptx's `//@id`, which covers ids inside `mc:AlternateContent` and `a:graphicData`), every numeric `@spid` and the number of `_x0000_s####` `@spid`s (timing, `p:control`, `p:oleObj`), and every `ac:*Mk@id` in the slide's modern comment parts (review m1: on 11 corpus slides these exceed every `@id`, e.g. `p:control spid="3074"`); refused above `i32::MAX` (`ShapeIdsExhausted`; POI reads ids as `int`) | the lowest free id (POI `allocateShapeId`) |
| `p14:creationId@val` | `ooxml-pptx` | for a duplicated slide: `max(over all slides) + 1` (u32; refused on overflow) | copy the original's |
| sections | `ooxml-pptx` | membership only (§5) | create, rename or reorder sections |

Existing duplicate shape ids (none outside AC in the corpus, id 7 in both
AC branches of `Of16-01.pptx`) are tolerated on read, as in Phase 0; new
ids are above all of them. Shape ids need no session high-water mark in
Phase 2, which deletes no shapes; a later phase that deletes shapes adds
one per slide.

### 4. Operations

Every operation is **one** registry transaction (§2): clones edited,
every refusal of §9 and every core step recorded in one `RegistryPlan`,
one `Package::apply`. The package is byte-identical after a refusal,
`presentation.xml` included (tested like ADR-0005 T4). There is no
partially-applied state visible to the caller.

**Positional show range.** When `presProps` has `p:showPr/p:sldRg` (a
slide show range by position; 0 in the corpus), every operation that
changes positions — add, delete, move, duplicate — is refused with
`ShowRangeByPosition` (review m6).

#### 4.1 `add_slide(layout: &LayoutRef, at: Option<usize>) -> SlideMut`

The layout comes from ADR-0008's `layouts()` (`LayoutRef`: master, part,
`p:cSld@name`, `p:sldLayout@type`), reached from a master in
`p:sldMasterIdLst` through the master's `p:sldLayoutIdLst`
(`LayoutNotFound` otherwise). Steps:

1. part `ppt/slides/slide{}.xml` (`add_part`, `Numbered`; `Override`);
2. one `slideLayout` relationship from it (`add_relationship`);
3. presentation relationship (`add_relationship`) and a `p:sldId` (§3)
   inserted into the cloned `presentation.xml`'s `p:sldIdLst` at `at`
   (the list is created in schema order if absent: `insert_by` on
   `p:presentation`), staged (`stage_part`);
4. slide XML: `p:sld/p:cSld/p:spTree` with the group's `nvGrpSpPr` (id 1)
   and `grpSpPr`, then the placeholders, then
   `p:clrMapOvr/a:masterClrMapping`. **Placeholder rules** (review m7):
   - one per placeholder **directly** in the layout's `p:spTree`
     (placeholders in groups or inside `mc:AlternateContent` are not
     cloned), in layout order, **except `dt`, `ftr`, `sldNum`** (both
     libraries skip these; PowerPoint adds them to new slides of a deck
     where "apply to all" footers are on — not reproduced, Q5);
   - each `p:sp` has `p:nvSpPr` (`p:cNvPr id name`, `p:cNvSpPr` with
     `a:spLocks noGrp="1"`, `p:nvPr/p:ph` copying only `type`, `orient`,
     `sz`, `idx`) and an empty `p:spPr` (position inherited);
   - a `p:txBody` (`a:bodyPr`, `a:lstStyle`, one empty `a:p`) only for
     the types python-pptx gives one: `title`, `ctrTitle`, `subTitle`,
     `body`, `obj` (including the implicit `obj` of a `p:ph` without
     `type`); pictures, charts, tables, media placeholders get none;
   - ids `2, 3, …`; names in PowerPoint's scheme as python-pptx
     ("Title 1", "Content Placeholder 2", …), prefixed "Vertical " when
     `orient="vert"`;
   - layouts with several placeholders of one type and idx (ADR-0008's
     `IdxTie*` cases) are cloned as found; each clone keeps its own
     `type`/`idx` pair, so ADR-0008 matches it to the same layout
     placeholder.

   This is python-pptx's shape, **not** POI's full copy (which carries the
   layout's prompt text "Click to add title" into the slide as real
   text);
5. section membership (§5); `app.xml` (§7).

The new slide is returned as a Phase 0 `SlideMut`, so its placeholders are
filled with the Phase 0/0.1 text API.

#### 4.2 `delete_slide(index)`

Validate (all inside the transaction, before `apply`):

- **References by relationship** (review M2): every internal relationship
  that resolves to the slide part (`Package::incoming_relationships`) must
  come from `presentation.xml` (removed by this operation), from the
  slide's own notes slide (removed with it), or from another part of the
  cascade (§6); `viewProps.xml`'s outline entry
  (`p:outlineViewPr/p:sldLst/p:sld r:id`, view state only) is removed
  together with its relationship (staged `viewProps.xml`; an emptied
  `p:sldLst` is kept — `sld` is `minOccurs="0"` in
  `CT_OutlineViewSlideList`, review N5). Any other
  source — another slide's jump link, a layout's or master's action
  button, another slide's notes, a SmartArt `diagramData` hyperlink — is
  `SlideIsReferenced { from_part, rel_type }`. A relationship part that
  cannot be read or parsed refuses the delete
  (`SlideIsReferenced { from_part: <that part>, rel_type: "?" }`; fail
  closed, review N4, §2).
- **References by `sldId` value:** the section lists are updated (§5);
  Slide Zoom objects (`pslz:sldZmObj@sldId`, any slide) and any other
  numeric `sldId` attribute equal to the slide's id found in a slide's or
  the presentation's `p:extLst` or inside `mc:AlternateContent` are
  `SlideIsReferenced` (fail closed).
- custom shows (`SlideInCustomShow`), the show range (§4 intro), and the
  cascade types of §6 (`UnsupportedSlidePart(type)`).

Then, in the same plan: stage `presentation.xml` without the `p:sldId`
and the section entry (the section stays, possibly empty: 12 of 17
corpus sections are empty), `remove_relationship` for presentation →
slide, `remove_parts` for the slide and its cascade (§6) with the allowed
types of §6 (which include `slide`: `remove_parts` checks the type of
every remaining relationship into the set, the presentation's and
viewProps' included, before it looks at removals already in the plan),
stage `app.xml` (§7) and `viewProps.xml` if changed; apply. The explicit
`remove_relationship` for presentation → slide is optional:
`remove_parts` removes a relationship from a remaining part whose staged
content no longer names its `Id` (review N2).

#### 4.3 `move_slide(from, to)`

Reorders `p:sldId` elements (the only change to `presentation.xml`
besides sections), staged as one edit. Sections: §5.

#### 4.4 `duplicate_slide(index, at) -> SlideMut`

Allowed when the relationship types of the slide **and of its notes
slide** (review N3) are all in `slideLayout`, `notesSlide`,
`notesMaster`, `image`, `media`, `audio`, `video`, `hdphoto`,
`hyperlink`, `slide` (jump links; the notes' back-relationship), `tags`
(457/522 corpus slides); anything else (charts, diagrams, OLE, controls,
…) is `UnsupportedSlidePart` (their parts would need cloning with their
own trees). **Comments** (legacy or modern, 3 corpus slides) are
`UnsupportedSlidePart(comments)` in Phase 2 (review N1): a copy must not
share the original's comment part (modern comments name the original's
`sldId`/`creationId` in `pc:sldMk`), and the core cannot drop a copied
relationship from a `.rels` created in the same plan. When the core's
`copy_relationships` gains the type filter requested in §2, the comment
relationships are skipped instead and the refusal goes. A `tags` part
that has relationships of its own (rare) is
`UnsupportedSlidePart(tags)`. In one plan:

1. the new slide part: the original's XML (cloned tree), with
   `p14:creationId` (if present) set to a new value (§3);
2. a copy of the notes slide (new part; its own `p14:creationId`, if any,
   renewed the same way) and a copy of each `tags` part of the slide or
   its notes (PowerPoint duplicates tags; add-ins key them per slide,
   review m5) — one copy per **distinct target**, so two `r:id`s naming
   one tags part map to one copy in `target_map` (review N3);
3. `copy_relationships(original, new, target_map)` with the notes and
   tags copies in `target_map`: the copy keeps every `Id`, so its XML's
   `r:id`/`r:embed`/`r:link` stay valid; layout and media stay **shared**
   (15 shared media in the corpus); then the same for the notes copy
   (notes master shared, its `slide` relationship mapped to the new
   slide, its tags mapped to their copies);
4. presentation relationship, `p:sldId` at `at`, sections (§5), `app.xml`.

**Jump links to the slide itself** (review N8): a `slide` relationship
from the original to itself (a "repeat this slide" button) is **not**
remapped: the copy's link still targets the original, like every other
jump link it copies. Whether PowerPoint remaps it is unverified; it is an
item of the manual check (§10). `ppaction://hlinkshowjump?jump=…`
actions (next, previous, first, last) have no relationship and are
unaffected.

Shape ids are per slide, so they stay; animations in the copy keep
referring to the copy's shapes.

#### 4.5 `add_text_box(rect, text)` and `add_picture(rect, bytes)`

On a `SlideMut`, appended at the end of `p:spTree` (top of z-order):

- text box: `p:sp` with `p:cNvSpPr txBox="1"`, `a:xfrm` from `rect`
  (EMU), `a:prstGeom prst="rect"`, `p:txBody` with `a:bodyPr wrap="none"`
  and the text through the Phase 0 `TextBodyMut::set_text` rules (no
  formatting: inherits the master's `otherStyle`, ADR-0005 Q6);
- picture: the image type from its magic bytes (PNG, JPEG, GIF; anything
  else `UnsupportedImage`); **dedup by SHA-256** against every existing
  part with an image content type (python-pptx `_find_by_sha1`, POI
  `findPictureData` by checksum + bytes), else a new
  `ppt/media/image{}.{ext}` (`add_part`, `Numbered`) whose content type
  is chosen like this (review N7): an existing `Default` for the extension
  with the same type → nothing added; a `Default` with another type (e.g.
  `bin`) → an `Override`; no `Default` → `add_default_content_type`, and
  when the core refuses it with `WouldRetypePart` (a part with that
  extension and no `Override` exists) → an `Override` instead, not an
  error; an `image`
  relationship from the slide (`add_relationship`); the slide's edited
  clone staged in the same plan; a
  `p:pic` with `p:blipFill/a:blip r:embed`, `a:stretch/a:fillRect`,
  `a:xfrm` from `rect` (when `rect` has no size: the pixel size at 96 dpi
  from the image header, the python-pptx rule).

Shape ids: §3. Names: "TextBox {id-1}", "Picture {id-1}" (PowerPoint's
scheme).

#### 4.6 Notes

Phase 2 adds only `ensure_notes_slide(slide)`: when the slide has no notes
slide, it creates one from the notes master (part `notesSlide{N}`,
relationships to the notes master and back to the slide, slide → notes,
placeholders `sldImg`, `body`, `sldNum` cloned like §4.1 — the
python-pptx set; unlike §4.1, `sldNum` **is** cloned there, as notes pages
show it by default) and increments `app.xml` `Notes` (§7). Text is read
and written through ADR-0008's `notes_mut` / `NotesMut` (review m8); a
created notes slide always has a body placeholder, so ADR-0008's
`NoNotesBody` cannot occur on it. A presentation without a notes master
(36 of 91 files have one; the template of §8 does) is `NoNotesMaster` in
Phase 2; creating a notes master is specified in ADR-0016 §8 (Phase 3),
which supersedes this refusal when implemented. There is **one** notes
creation API (review N9): ADR-0016 extends `ensure_notes_slide` to create
the notes master when missing instead of adding a second entry point.

### 5. Sections (`p14:sectionLst`)

Read from `p:presentation/p:extLst/p:ext[@uri="{521415D9-36F7-43E2-AB2F-B90AF26B5E84}"]/p14:sectionLst`
(the only placement in the corpus); anything else that looks like a
section list (another `uri`, inside `mc:AlternateContent`) makes every
slide-structure operation `UnsupportedSections`. Invariant checked before
and kept after every operation: **the concatenation of the sections'
`p14:sldId` lists equals `p:sldIdLst`** (true in all 4 files; a deck
violating it is refused with `SectionsInconsistent`).

- add / duplicate at position *i* > 0: the new id joins the section of
  the slide at *i − 1* (PowerPoint's "new slide after the selection");
  at position 0: the section of the slide **now** at position 0, i.e.
  the following slide's (review m11: an empty first section is never
  filled implicitly); with no slides or no sections, nothing.
- delete: the id leaves its section; the section stays, even if empty.
- move: the id leaves its section and joins, by the same rule, the
  section of its new predecessor (of its new successor at position 0).
- Position alone cannot say "first slide of section *k*"; a later
  `move_slide_to_section` can add that.

What PowerPoint does with a slide outside every section was owner question
ADR-0005 Q4b (manual check pending); the rules above never create one.

### 6. Orphan removal on delete (cascade)

`delete_slide` passes to `remove_parts` (one set) the slide and every
part it reaches, recursively through their own internal relationships,
whose relationship type is in the cascade set **and** to which no
relationship from outside the set points (`incoming_relationships`, OPC
equivalence); `allowed_rel_types` is that set, so implicit references by
type stay protected (§2). Listed in full (review N2), it is: `slide` (the
presentation's and viewProps' relationships to the deleted slide, and
the notes' back-relationship), `notesSlide`, `comments` (legacy) and the
modern comments type, `image`, `media`, `audio`, `video`, `hdphoto`,
`chart`, `package`, `chartStyle`, `chartColorStyle`, `chartUserShapes`,
`diagramData`, `diagramLayout`, `diagramQuickStyle`, `diagramColors`,
`diagramDrawing`, `oleObject`, `vmlDrawing`, `tags`, `themeOverride` (each
under its transitional and, where defined, strict/Microsoft URI). Only **internal** targets that resolve to a part
cascade; external targets (`image`/`video`/`oleObject` links with
`TargetMode="External"`, 1 + 7 in `56812.pptx`) and internal targets that
resolve to no part (`#_ftn1` hyperlinks) go away as relationships only
(review m3):

| Type | Removed when orphaned | Notes |
|---|---|---|
| `notesSlide` | yes | its relationships to the notes master (kept: still referenced) and back to the slide (removed with it) |
| `comments` (legacy), modern comments (2018) | yes | `commentAuthors.xml`/`authors.xml` kept (append-only) |
| `image`, `media`, `audio`, `video`, `hdphoto` | yes, only when unreferenced (15 shared) | never a part also used by a layout/master |
| `chart` (+ its `package`, `chartStyle`, `chartColorStyle`, `chartUserShapes`), `diagram*`, `oleObject`, `package`, `vmlDrawing`, `tags`, `themeOverride` | yes, as a tree | their own internal targets follow the same rule |
| `slideLayout` | never (layouts are kept even when unused: 959 unused layouts in the corpus) | |
| `hyperlink` (any mode), `slide` (jump links out of the deleted slide), any external target | relationship only | |
| anything else (`control`, `chartEx`, unknown) | — | `UnsupportedSlidePart(type)`: 3 of 522 slides |

The whole cascade is one `remove_parts` call in the operation's plan.
`Default` content types are never removed (other parts may use them).

### 7. `docProps/app.xml`

Phase 2 is the first writer of `app.xml` (Phase 0: untouched, may be
stale). It updates, when present, only the language-independent counts
that PowerPoint keeps exact — measured on the 86 files that have them
(4 lack `Slides`, 1 has no `app.xml`): `Slides` = slide count in 86/86,
`Notes` = notes slides in 86/86, `HiddenSlides` = slides with
`p:sld@show` `0` or `false` in 86/86 (review m9). Add/duplicate/delete
adjust `Slides`, `Notes` (a slide with notes; `ensure_notes_slide` adds
one) and `HiddenSlides`. The rule holds for every path that creates a
notes slide, including ADR-0016's (Phase 3) creation of a notes master
through the same `ensure_notes_slide` (review N9). `TitlesOfParts`/`HeadingPairs` are **left
unchanged**: the "slide titles" group is identifiable only by a localized
label (`Slide Titles`, `Folientitel`, `Bildrubriker`, `幻灯片标题`, …) or
by position, and a wrong guess would corrupt the font/theme lists in the
same vector; untouched, the two vectors stay consistent with each other.
A missing `app.xml` or element is not created; an `app.xml` that cannot
be read or parsed is left untouched (not a refusal).

### 8. `Presentation::new()` — the built-in template

As ADR-0001's From-scratch path: an embedded template `.pptx` opened
through the same `Package::open` → edit path; no separate writer.

- **Authored in this repository**, not copied from python-pptx's
  `default.pptx`, POI or Microsoft's theme files: the XML parts are
  written by hand under `crates/ooxml-pptx/template/` (license: the
  repository's); colour values and font names are factual choices, not
  copied markup. The `.pptx` bytes are built from them by an `xtask`
  command whose `--check` compares **part content** (names, order and
  uncompressed bytes), not zip bytes, so a deflate implementation update
  does not fail CI (review m10).
- Content (proposed default, Q4): 16:9 (`12192000 × 6858000`), one master
  with its theme, **PowerPoint's standard 11 layouts in the standard
  order** — Title Slide, Title and Content, Section Header, Two Content,
  Comparison, Title Only, Blank, Content with Caption, Picture with
  Caption, Title and Vertical Text, Vertical Title and Text — so that
  index-based code (`layouts()[6]` = Blank) behaves as with python-pptx's
  default template; a notes master with its theme, `p:notesMasterIdLst`,
  `p:notesSz`, `presProps`, `viewProps`, `tableStyles` (empty list),
  `docProps/core.xml` and `app.xml`, **zero slides** (`p:sldIdLst`
  absent), no sections.
- `core.xml` is regenerated wholesale on `new()` (created/modified, no
  creator), as in ADR-0001 for xlsx; the time is injectable
  (`Presentation::new_with(NewOptions { created })`) so tests and
  reproducible builds get byte-identical output.
- Acceptance: python-pptx opens it and its outputs; LibreOffice (the
  pinned O3 workflow) converts it; the owner opens it in PowerPoint
  without repair (manual check, §10).

### 9. Refusals (`Err`, nothing changed)

| Error | When |
|---|---|
| `LayoutNotFound` | the layout is not reachable from a master in `p:sldMasterIdLst` |
| `SlideIndexOutOfRange` (Phase 0) | bad `index`/`at` |
| `SlideIdsExhausted`, `ShapeIdsExhausted` | §3 |
| `SlideIsReferenced { from_part, rel_type }` | delete of a slide that another part references (relationship from outside the cascade, Slide Zoom, unknown numeric `sldId` reference), or with an unreadable relationship part (§4.2) |
| `SlideInCustomShow` | delete of a slide named in `p:custShowLst` |
| `ShowRangeByPosition` | add/delete/move/duplicate with `presProps` `p:showPr/p:sldRg` |
| `UnsupportedSlidePart(rel_type)` | delete/duplicate with a relationship type outside §6 / §4.4; duplicate of a slide with comments or a tags part with relationships (§4.4) |
| `UnsupportedSections`, `SectionsInconsistent` | §5 |
| `NoNotesMaster` | §4.6 |
| `UnsupportedImage` | §4.5 |
| `InsideAlternateContent`, `InvalidShapeId`, … (Phase 0) | the target slide's own checks |
| `Core(…)` | any refusal of the registry transaction (name collision, content type conflict, a remaining reference, a relationship type not allowed, …) — all before `apply` |

### 10. Test oracles and evidence

- **L1 / locality (ADR-0005 T2 style):** after each operation, every
  part not involved is byte-identical (name, method, CRC, compressed
  bytes); `presentation.xml` differs only inside `p:sldIdLst` and the
  section list; `[Content_Types].xml` and `.rels` only by appended/removed
  entries (no reordering, no renumbering: diff check of every existing
  `Id`, `sldId`, part name and shape id — the roadmap's acceptance rule).
- **Structural oracle (`tests/oracle/structure.py`, python, independent
  of the Rust code):** every internal relationship target exists; every
  part has exactly one content type; no part is unreachable from the
  package relationships except those that were unreachable in the input;
  `sldId`s unique and in 256…2³¹−1; sections concatenate to `sldIdLst`;
  per slide, shape ids unique among new shapes and above every pre-existing
  `@id`; each notes slide has one back-relationship to its slide.
- **Read-back (python-pptx 1.0.2, as ADR-0005 T3):** slide count, order
  (by `sldId`), layout names, placeholder types/idx, text, picture
  hashes, notes text; part names read before python-pptx renames them.
- **Expected model:** each operation is applied to the Phase 0 deck model
  in Rust and independently in python (python-pptx cannot delete or
  move slides, so the python side edits `sldIdLst` and relationships with
  lxml directly, as `check.py` does for text).
- **T4 refusals:** every row of §9, byte-identical afterwards (all parts,
  `presentation.xml` included); synthetic inputs for custom shows, show
  ranges, inconsistent sections, exhausted ids, a `control` relationship,
  a layout action button linking to the slide, a Slide Zoom, and a case
  the **core** refuses inside the transaction after the pptx edits were
  staged (review B1); a slide with comments on duplicate (N1), a tags
  part with its own relationships (N3), an unreadable `.rels` on delete
  (N4).
- **Manual (owner):** a deck built with `Presentation::new()` and each
  operation applied to `sample.pptx`, `slide-sections.pptx` and
  `airplane-transition.pptx` (only `slide2.xml`), opened in Microsoft 365
  PowerPoint: opens without repair; sections as expected; app.xml
  staleness visible or not; and, for review N8, PowerPoint's own
  duplicate of a slide with a jump link to itself — whether its copy
  links to itself or to the original (ADR-0012 keeps the original).

**Comparison with the reference implementations** (source-verified):

| Rule | python-pptx 1.0.2 | POI `47dcff76` | ADR-0012 |
|---|---|---|---|
| slide id | `max(256−1, used) + 1`, gap search above 2³¹−1 (`oxml/presentation.py` `_next_id`) | `max(maxId, 255) + 1` (`XMLSlideShow.createSlide`) | session high-water + 1, refuse above 2³¹−1 |
| slide part name | `slide{len(sldIdLst)+1}` (`parts/presentation.py` `_next_slide_partname`) **and renames all slide parts** (`rename_slide_parts`) | lowest unused index (`OPCPackage.getUnusedPartIndex`, `PackagePartCollection.getUnusedPartIndex` = `nextClearBit(1)`, bug 55791) | `max index + 1`, never rename |
| rId | first unused `rIdN` from `len+1` downward, reuses gaps (`opc/package.py` `_next_rId`) | `POIXMLDocumentPart` relation counter | `max + 1`, no reuse |
| shape id | `max(//@id) + 1` (`oxml/shapes/groupshape.py` `max_shape_id`) | lowest free id (`XSLFSheet.allocateShapeId`, `nextClearBit(1)`) | max over `@id`, `@spid`, comment monikers, + 1 |
| placeholders of a new slide | `type`/`orient`/`sz`/`idx` only, no `spPr`; skips date/footer/slide number (`shapes/shapetree.py` `clone_layout_placeholders`, `slide.py` `iter_cloneable_placeholders`) | full XML copy of layout text shapes incl. prompt text; skips the same three (`XSLFSlideLayout.copyLayout`) | python-pptx's shape |
| delete slide | not available | removes sldId, notes, charts, pictures (by relation counter), not sections/custom shows/links (`XMLSlideShow.removeSlide`) | §4.2, §6 |
| move slide | not available | reorders `sldIdLst` only (`setSlideOrder`) | + sections |
| picture dedup | SHA-1 of the blob (`package.py` `_find_by_sha1`) | checksum + bytes (`findPictureData`) | SHA-256 + byte equality |
| media name | first gap in `image{N}` (`package.py` `next_image_partname`) | lowest unused `/ppt/media/image#` | `max + 1` |
| `app.xml` | not handled | not updated | counts only (§7) |

### 11. API sketch

```rust
impl Presentation {
    pub fn new() -> Result<Self>;                                    // §8
    pub fn new_with(options: NewOptions) -> Result<Self>;            // injectable core.xml time
    // layouts(): ADR-0008 (`Vec<LayoutRef>`); sections(): ADR-0008
    pub fn add_slide(&mut self, layout: &LayoutRef<'_>, at: Option<usize>) -> Result<SlideMut<'_>>;
    pub fn delete_slide(&mut self, index: usize) -> Result<()>;
    pub fn move_slide(&mut self, from: usize, to: usize) -> Result<()>;
    pub fn duplicate_slide(&mut self, index: usize, at: Option<usize>) -> Result<SlideMut<'_>>;
    pub fn ensure_notes_slide(&mut self, slide: usize) -> Result<()>; // text: ADR-0008 notes_mut
}
impl SlideMut<'_> {
    pub fn add_text_box(&mut self, rect: Rect, text: &str) -> Result<ShapeMut<'_>>;
    pub fn add_picture(&mut self, rect: Rect, bytes: &[u8]) -> Result<ShapeMut<'_>>;
}
```

The allowlist grows by the elements written (`p:sldIdLst`/`p:sldId` →
`[write]`, `p:pic` and its non-visual/blip children, `p:clrMapOvr`,
`p14:sectionLst`/`p14:section`/`p14:sldIdLst`/`p14:sldId`, notes slide
elements, and for the delete of an outline entry `p:outlineViewPr`/
`p:sldLst`/`p:sld` with remove, review N5); new slide parts are built as complete `XmlDocument`s (no
original to preserve) and existing parts are edited only through the lens
on clones staged in the transaction.

## Consequences

- Positive: decks can be generated, reordered and pruned without any
  existing part being renamed or any existing id renumbered, so modern
  comments, animations, sections and links that point into untouched
  slides stay valid by construction.
- Positive: every package-level step is checked before the first change;
  a refused operation leaves the file byte-identical.
- Negative: the core registry transaction (with `stage_part` and
  `copy_relationships`) and its ADR-0001 note are prerequisites; Phase 2
  cannot start before that PR is merged.
- Negative: deleting a slide that another part references, or that is in
  a custom show, is refused rather than repaired; duplicating slides with
  charts/diagrams/OLE is refused (65 of 522 slides).
- Negative: `app.xml` title lists can become stale (§7).
- Negative: slides with comments cannot be duplicated until the core's
  `copy_relationships` type filter lands (§4.4, 3 of 522 corpus slides).
- Negative: a duplicated slide's jump link to itself keeps pointing to
  the original (§4.4), unverified against PowerPoint.
- Negative: `max + 1` allocation makes ids and part numbers grow
  monotonically over many edit sessions (no compaction, by design).

## Alternatives considered

1. **python-pptx's renaming of slide parts** to keep `slide1..N` in
   order. Rejected: ADR-0001 Layer 1 (renaming forces rewriting every
   relationship that names the part and breaks byte-identity of untouched
   parts); the corpus shows names carry no order anyway.
2. **Lowest free shape id / part index / `rId` (POI, python-pptx rIds).**
   Rejected for ids that other XML may still name after a deletion (shape
   ids, `sldId`); kept out for part names and `rId`s for uniformity and to
   avoid reusing a name removed earlier in the same session.
3. **Repair references on delete** (drop the custom-show entry, turn a
   jump link into nothing). Deferred: 0 custom shows and 3 link targets in
   the corpus; an explicit API can come later.
4. **Rewrite `TitlesOfParts`** from slide titles. Rejected for Phase 2:
   localized labels (§7).
5. **Use python-pptx's `default.pptx` as the template.** Rejected: an
   external MIT file in the crate needs its own notice, and its content is
   not under our control; a repository-authored template with a `--check`
   build is reproducible.

## Open questions for the project owner

Each has a **proposed default** (the round-1 review's recommendation),
used unless the owner decides otherwise.

- **Q1** Section placement (§5). *Default:* the predecessor's section;
  at position 0 the following slide's section; empty sections stay empty;
  confirm with ADR-0005 Q4b in the manual check.
- **Q2** Stale `TitlesOfParts`/`HeadingPairs` (§7). *Default:* accepted
  (python-pptx never updates `app.xml` and its output opens in
  PowerPoint, which rebuilds `app.xml` on save); counts only; manual check
  together with ADR-0005 Q4c.
- **Q3** Deleting a referenced slide. *Default:* refuse
  (`SlideIsReferenced`); an explicit `delete_slide_unlinking` can come in
  a later phase.
- **Q4** Template. *Default:* 16:9 with PowerPoint's standard 11 layouts in
  the standard order (§8).
- **Q5** Footers on new slides in decks with "apply to all" footers
  (§4.1). *Default (round-2 review's recommendation):* not reproduced.
  Reproducing it would mean inferring the "apply to all" state from
  `p:hf` and the master's `dt`/`ftr`/`sldNum` placeholder text, i.e.
  writing a guess; callers can add footers explicitly with the Phase 0
  text API. If the manual check shows a visible difference, an
  `add_slide_with_footers` can be considered in a later phase.

## Sources

- Spike: `spikes/pptx-phase2/units.py` (this ADR's tables;
  `results/units-91.txt`, corpus by content in
  `results/corpus-sha256.txt`), roadmap §3.5.
- Core: the registry transaction on branch `core/add-part`
  (`crates/ooxml-core/src/package/add.rs`: `RegistryPlan`,
  `PartNameRequest`, `stage_part`, `copy_relationships`, `remove_parts`,
  `incoming_relationships`; PR to come).
- python-pptx 1.0.2: `pptx/oxml/presentation.py` (`_next_id`),
  `pptx/parts/presentation.py` (`add_slide`, `_next_slide_partname`,
  `rename_slide_parts`), `pptx/opc/package.py` (`next_partname`,
  `_next_rId`), `pptx/oxml/shapes/groupshape.py` (`max_shape_id`),
  `pptx/shapes/shapetree.py` (`clone_layout_placeholders`,
  `clone_placeholder`), `pptx/slide.py` (`add_slide`,
  `iter_cloneable_placeholders`, `SlideLayouts.remove`),
  `pptx/package.py` (`_find_by_sha1`, `next_image_partname`).
- Apache POI, commit `47dcff769bb395034dddfd59c4609659b6dbc6a4`,
  `poi-ooxml/src/main/java/org/apache/poi/`: `xslf/usermodel/XMLSlideShow.java`
  (`createSlide`, `findNextAvailableFileNameIndex`, `setSlideOrder`,
  `removeSlide`, `addPicture`, `findPictureData`), `xslf/usermodel/XSLFSheet.java`
  (`allocateShapeId`), `xslf/usermodel/XSLFSlideLayout.java` (`copyLayout`),
  `openxml4j/opc/PackagePartCollection.java` (`getUnusedPartIndex`),
  `ooxml/POIXMLDocumentPart.java` (`removeRelation`).
- ECMA-376 Transitional XSDs (vendored, `reference/ecma-376-transitional-xsd/`):
  `pml.xsd` `ST_SlideId` (`unsignedInt`, 256 ≤ id < 2147483648),
  `dml-main.xsd` `ST_DrawingElementId` (`unsignedInt`, the type of
  `cNvPr@id`); ECMA-376 Part 2 (OPC: part names, content types,
  relationships).
