# ADR-0012: PowerPoint Phase 2 — slides and shapes as units

- Status: **Proposed** (revision 1)
- Date: 2026-11-16
- Depends on: [ADR-0001](0001-lossless-round-trip-architecture.md) (Layer 1
  passthrough, Layer 3 append-only registries, From-scratch path),
  [ADR-0002](0002-schema-driven-code-generation.md) (lens, allowlist,
  insertion; post-acceptance note "detach/insert", PR #18),
  [ADR-0005](0005-pptx-phase0.md) (Phase 0/0.1: references into slide text,
  refusals, oracles); ADR-0008 (Phase 1, deck reading and inheritance,
  in progress on `plan/pptx-phase1`) is **not** a prerequisite: Phase 2
  needs only direct values, and where it reads a layout it reads it
  directly (§4.1).
- Background: [`docs/roadmap/pptx.md`](../roadmap/pptx.md) §3.5, §3.6,
  §4 Phase 2
- Evidence: [`spikes/pptx-phase2/`](../../spikes/pptx-phase2/README.md)
  (`units.py`, 91 distinct files, 522 slides); python-pptx 1.0.2 and Apache
  POI (commit `47dcff76`) source, cited per rule in §10
- Core prerequisites: PR #14 (`Package::remove_part`, open) and the new
  core primitives of §2 (a separate core PR)

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
6. Notes: `notes_text()` / `set_notes_text(text)`, creating the notes
   slide when the presentation has a notes master.
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

### 2. Prerequisites in `ooxml-core` (separate core PR)

Phase 2 is a client of four package-level operations that do not exist
yet, plus PR #14. Each comes as `check_*(&self, …)` + the mutating call,
with **one** shared validation, so that a format crate can check every
step of a multi-part edit before the first change (the plan → validate →
apply discipline of ADR-0003 and the `check_insert_under` pattern of
PR #18):

| Id | Operation | Rules |
|---|---|---|
| C1 | `add_part(name, content_type, bytes)` | `name` absent under OPC part-name equivalence (ASCII case-insensitive, percent-decoded; the rule of PR #14); content type registered by an existing `Default` for the extension when that `Default` carries exactly `content_type`, else by a new `Override` appended to `[Content_Types].xml`. Never edits or removes an existing `Default`/`Override`. |
| C2 | `add_default_content_type(ext, content_type)` | only if no `Default` for `ext` exists **and** no existing part with that extension lacks an `Override` (adding the `Default` would otherwise change an existing part's type): refused otherwise. |
| C3 | `add_relationship(source, rel_type, target, mode) -> Id` | appends to the source's `.rels` (creating it as a new part when absent); the new `Id` is `rId{max N + 1}` over the source's `rIdN` ids (other ids ignored, never reused); `Internal` targets must exist. |
| C4 | `remove_relationship(source, id)` | refused when any attribute of the source part, after XML unescaping, equals `id` (the any-attribute rule of PR #14 M1). |
| C5 | `unused_part_name(template)` | `template` like `/ppt/slides/slide{}.xml`; returns the name with **the largest existing index + 1** among names equivalent to the template's pattern (never reuses an index present in the input or removed in this session). POI's lowest-unused rule (bug 55791) is collision-free too; `max + 1` additionally never reuses a name whose part was removed earlier in the same session (a zip entry may still be pending). |
| C6 | `remove_part_with_relationships(name, allowed_rel_types)` | PR #14's `remove_part` without the `HasOwnRelationships` refusal: the part's own `.rels` goes with it, and the call returns the internal targets it referenced so the caller can decide (§6). Same refusals otherwise. |

ADR-0001's interim note (PR #14) allows removing a registry entry only
together with its part, for one named caller. Phase 2 needs the same for
slides, notes, comments and media. This ADR therefore asks for a
**post-acceptance note to ADR-0001** that replaces the interim one:
*a registry entry (`[Content_Types]` `Override`, relationship,
`p:sldId`, section entry) may be removed only together with the thing it
registers, only after it is shown that nothing remaining refers to it
(by relationship `Id`, any attribute, or a format-level id such as
`sldId`), and no remaining entry is ever renumbered or reused.* The
append-only rule for **allocation** stays absolute.

### 3. Registries and allocation rules

| Registry | Owner | New value | Never |
|---|---|---|---|
| part names | core (C5) | `max index + 1` per template (`slide`, `notesSlide`, `image`, `media`, `comment`) | rename an existing part (python-pptx's `rename_slide_parts`) |
| content types | core (C1, C2) | `Override` per new part; `Default` per new media extension (C2 conditions) | edit or remove a `Default` |
| relationship ids | core (C3) | `rId{max + 1}` per source part | reuse a gap (python-pptx `_next_rId`) |
| `p:sldId@id` | `ooxml-pptx` | `max(existing, 255) + 1`; above 2,147,483,647: `SlideIdsExhausted` | search for a gap (python-pptx's fallback reuses a deleted slide's id, which modern comments `pc:sldMk@sldId` and sections may still name) |
| shape ids (`cNvPr@id`) | `ooxml-pptx`, per slide part | `max(every numeric unprefixed @id in the part) + 1` — includes ids inside `mc:AlternateContent` branches and `a:graphicData` (python-pptx's `max_shape_id` reads `//@id`); above `u32::MAX`: `ShapeIdsExhausted` | the lowest free id (POI `allocateShapeId`): a freed id may still be named by an animation, connector or comment |
| `p14:creationId@val` | `ooxml-pptx` | for a duplicated slide: `max(over all slides) + 1` (u32, wrapping refused) | copy the original's (modern comments anchor by `sldId` + `creationId`) |
| sections | `ooxml-pptx` | membership only (§5) | create, rename or reorder sections |

Existing duplicate shape ids (none outside AC in the corpus, id 7 in both
AC branches of `Of16-01.pptx`) are tolerated on read, as in Phase 0; new
ids are above all of them.

### 4. Operations

All operations run **plan → validate → apply**: every refusal of §9 and
every core `check_*` of §2 for every step happens before the first change;
the package is byte-identical after a refusal (tested like ADR-0005 T4).
Each is one call; there is no partially-applied state visible to the
caller.

#### 4.1 `add_slide(layout: LayoutRef, at: Option<usize>) -> SlideMut`

`Presentation::layouts()` lists `(master index, layout part, p:cSld@name,
p:sldLayout@type)` for every master in `p:sldMasterIdLst`; the caller
picks one (by name, type or index). Steps:

1. part `/ppt/slides/slide{N}.xml` (C5), `Override` (C1);
2. slide `.rels`: one `slideLayout` relationship (C3);
3. presentation relationship (C3) and a `p:sldId` (§3) inserted into
   `p:sldIdLst` at `at` (the list is created in schema order if absent:
   `insert_by` on `p:presentation`);
4. slide XML: `p:sld/p:cSld/p:spTree` with the group's `nvGrpSpPr` (id 1)
   and `grpSpPr`, then **one placeholder per layout placeholder except
   `dt`, `ftr`, `sldNum`** (both libraries skip these), each with
   `p:nvSpPr` (`p:cNvPr id name`, `p:cNvSpPr` with `a:spLocks
   noGrp="1"`, `p:nvPr/p:ph` copying only `type`, `orient`, `sz`, `idx`),
   an empty `p:spPr` (position inherited), and for text-bearing types a
   `p:txBody` with `a:bodyPr`, `a:lstStyle` and one empty `a:p` —
   python-pptx's shape, **not** POI's full copy (which carries the
   layout's prompt text "Click to add title" into the slide as real text);
   then `p:clrMapOvr/a:masterClrMapping`. Ids `2, 3, …`; names
   "Title 1", "Content Placeholder 2", … (PowerPoint's scheme, as
   python-pptx);
5. section membership (§5); `app.xml` (§7).

The new slide is returned as a Phase 0 `SlideMut`, so its placeholders are
filled with the Phase 0/0.1 text API.

#### 4.2 `delete_slide(index)`

Validate (all before any change): the slide is not a jump-link target of
another slide (`SlideIsLinkTarget`), not in a custom show
(`SlideInCustomShow`), the presentation has no `p:showPr/p:sldRg`
(`ShowRangeByPosition`), and every relationship type of the slide is in
the cascade set of §6 (`UnsupportedSlidePart(type)`). Then remove its
`p:sldId`, its section entry (the section stays, possibly empty: 12 of 17
sections in the corpus are empty), the presentation relationship (C4),
and the slide part with its relationships and orphaned targets (§6);
update `app.xml` (§7).

#### 4.3 `move_slide(from, to)`

Reorders `p:sldId` elements (the only change to `presentation.xml`
besides sections). Refused with `ShowRangeByPosition` when a positional
show range exists. Sections: §5.

#### 4.4 `duplicate_slide(index, at) -> SlideMut`

Allowed when the slide's relationship types are all in `slideLayout`,
`notesSlide`, `image`, `media`, `audio`, `video`, `hdphoto`, `hyperlink`
(external), `slide` (jump links), `tags` (457/522 slides); anything else
(charts, diagrams, OLE, comments, controls, …) is `UnsupportedSlidePart`
(their parts would need cloning with their own trees). The new slide part
is a byte copy of the original with: `p14:creationId` (if present) set to
a new value (§3); its `.rels` a copy of the original's with the same
`Id`s (relationship ids are per source part, so the copied XML stays
valid), pointing at the **same** layout, media and tag parts (shared, as
in 15 corpus files), except the `notesSlide` relationship, which points at
a copy of the notes slide (new part, its `slide` back-relationship to the
new slide, the same notes master). Modern and legacy comments are not
copied (they are about the original). Shape ids are per slide, so they
stay; animations in the copy keep referring to the copy's shapes.

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
  `/ppt/media/image{N}.{ext}` (C5) with a `Default` for the extension
  (C2) or an `Override`; an `image` relationship from the slide (C3); a
  `p:pic` with `p:blipFill/a:blip r:embed`, `a:stretch/a:fillRect`,
  `a:xfrm` from `rect` (when `rect` has no size: the pixel size at 96 dpi
  from the image header, the python-pptx rule).

Shape ids: §3. Names: "TextBox {id-1}", "Picture {id-1}" (PowerPoint's
scheme).

#### 4.6 Notes

`notes_text()` reads the notes slide's body placeholder (`p:ph type="body"`).
`set_notes_text(text)` writes it with the Phase 0 `TextBody::set_text`
rules; when the slide has no notes slide it creates one from the notes
master (part `notesSlide{N}`, relationships to the notes master and back
to the slide, placeholders `sldImg`, `body`, `sldNum` cloned like §4.1,
the python-pptx set), refused with `NoNotesMaster` when the presentation
has none (36 of 91 files have one; the template of §8 does).

### 5. Sections (`p14:sectionLst`)

Read from `p:presentation/p:extLst/p:ext[@uri="{521415D9-36F7-43E2-AB2F-B90AF26B5E84}"]/p14:sectionLst`
(the only placement in the corpus); anything else that looks like a
section list (another `uri`, inside `mc:AlternateContent`) makes every
slide-structure operation `UnsupportedSections`. Invariant checked before
and kept after every operation: **the concatenation of the sections'
`p14:sldId` lists equals `p:sldIdLst`** (true in all 4 files; a deck
violating it is refused with `SectionsInconsistent`).

- add / duplicate at position *i*: the new id joins the section of the
  slide at *i − 1*; at position 0, the first section (PowerPoint's
  "add slide" behaviour); with no sections, nothing.
- delete: the id leaves its section; the section stays, even if empty.
- move: the id leaves its section and joins the section of its new
  predecessor (first section at position 0).

What PowerPoint does with a slide outside every section was owner question
ADR-0005 Q4b (manual check pending); the rules above never create one.

### 6. Orphan removal on delete (cascade)

`delete_slide` removes the slide part with its own relationships (C6) and
then, for every internal target it referenced, removes the target **only
if no remaining relationship anywhere in the package resolves to it**
(scan of every `.rels`, OPC equivalence) and its relationship type is in
the cascade set; recursively for the removed targets' own targets:

| Type | Removed when orphaned | Notes |
|---|---|---|
| `notesSlide` | yes | its relationships to the notes master (kept: still referenced) and back to the slide (removed with it) |
| `comments` (legacy), modern comments (2018) | yes | `commentAuthors.xml`/`authors.xml` kept (append-only) |
| `image`, `media`, `audio`, `video`, `hdphoto` | yes, only when unreferenced (15 shared) | never a part also used by a layout/master |
| `chart` (+ its `package`, `chartStyle`, `chartColorStyle`, `chartUserShapes`), `diagram*`, `oleObject`, `package`, `vmlDrawing`, `tags`, `themeOverride` | yes, as a tree | their own internal targets follow the same rule |
| `slideLayout` | never (layouts are kept even when unused: 959 unused layouts in the corpus) | |
| `hyperlink` (external), `slide` (jump links out of the deleted slide) | relationship only | |
| anything else (`control`, `chartEx`, unknown) | — | `UnsupportedSlidePart(type)`: 3 of 522 slides |

The whole cascade is planned (C6 + `check_*` for each step) before the
first removal. `Default` content types are never removed (other parts may
use them).

### 7. `docProps/app.xml`

Phase 2 is the first writer of `app.xml` (Phase 0: untouched, may be
stale). It updates, when present, only the language-independent counts
that PowerPoint keeps exact (86/86 files): `Slides`, `Notes` (slides with
a notes slide), `HiddenSlides` (`p:sld@show="0"`, 3 slides in the corpus).
`TitlesOfParts`/`HeadingPairs` are **left unchanged**: the "slide titles"
group is identifiable only by a localized label (`Slide Titles`,
`Folientitel`, `Bildrubriker`, `幻灯片标题`, …) or by position, and a wrong
guess would corrupt the font/theme lists in the same vector. They may
become stale, as `Words`/`Paragraphs` already may after Phase 0 edits —
owner question Q2 (manual check: does PowerPoint care; ADR-0005 Q4c is
the same question for text). A missing `app.xml` or element is not
created.

### 8. `Presentation::new()` — the built-in template

As ADR-0001's From-scratch path: an embedded template `.pptx` opened
through the same `Package::open` → edit path; no separate writer.

- **Authored in this repository**, not copied from python-pptx's
  `default.pptx` or POI: the template's XML parts are committed as text
  under `crates/ooxml-pptx/template/` (license: the repository's), and the
  `.pptx` bytes are built from them by an `xtask` command with a
  `--check` mode, like codegen (CI fails when the bytes drift from the
  sources).
- Content: one master with a theme ("Office"-like, fonts `+mj-lt`/
  `+mn-lt`), seven layouts (`title`, `obj` "Title and Content",
  `secHead`, `twoObj`, `titleOnly`, `blank`, `objTx`), a notes master
  with its theme, `presProps`, `viewProps`, `tableStyles` (empty list),
  `docProps/core.xml` and `app.xml`, 16:9 (`12192000 × 6858000`), **zero
  slides** (`p:sldIdLst` absent), no sections.
- `core.xml` is regenerated wholesale on `new()` (created/modified now,
  no creator), as in ADR-0001 for xlsx.
- Acceptance: python-pptx opens it and its outputs; LibreOffice (the
  pinned O3 workflow) converts it; the owner opens it in PowerPoint
  without repair (manual check, §10).

### 9. Refusals (`Err`, nothing changed)

| Error | When |
|---|---|
| `LayoutNotFound` | the layout is not reachable from a master in `p:sldMasterIdLst` |
| `SlideIndexOutOfRange` (Phase 0) | bad `index`/`at` |
| `SlideIdsExhausted`, `ShapeIdsExhausted` | §3 |
| `SlideIsLinkTarget { from_slide }` | delete of a slide another slide links to |
| `SlideInCustomShow` | delete of a slide named in `p:custShowLst` |
| `ShowRangeByPosition` | delete/move with `presProps` `p:showPr/p:sldRg` |
| `UnsupportedSlidePart(rel_type)` | delete/duplicate with a relationship type outside §6 / §4.4 |
| `UnsupportedSections`, `SectionsInconsistent` | §5 |
| `NoNotesMaster` | §4.6 |
| `UnsupportedImage` | §4.5 |
| `InsideAlternateContent`, `InvalidShapeId`, … (Phase 0) | the target slide's own checks |
| `Core(…)` | any C1–C6 refusal (name collision, content type conflict, a remaining reference, …) — all from `check_*`, before any change |

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
- **T4 refusals:** every row of §9, byte-identical afterwards; synthetic
  inputs for custom shows, show ranges, inconsistent sections, exhausted
  ids, a `control` relationship.
- **Manual (owner):** a deck built with `Presentation::new()` and each
  operation applied to `sample.pptx`, `slide-sections.pptx` and
  `airplane-transition.pptx` (only `slide2.xml`), opened in Microsoft 365
  PowerPoint: opens without repair; sections as expected; app.xml
  staleness visible or not.

**Comparison with the reference implementations** (source-verified):

| Rule | python-pptx 1.0.2 | POI `47dcff76` | ADR-0012 |
|---|---|---|---|
| slide id | `max(256−1, used) + 1`, gap search above 2³¹−1 (`oxml/presentation.py` `_next_id`) | `max(maxId, 255) + 1` (`XMLSlideShow.createSlide`) | `max + 1`, refuse above 2³¹−1 |
| slide part name | `slide{len(sldIdLst)+1}` (`parts/presentation.py` `_next_slide_partname`) **and renames all slide parts** (`rename_slide_parts`) | lowest unused index (`OPCPackage.getUnusedPartIndex`, `PackagePartCollection.getUnusedPartIndex` = `nextClearBit(1)`, bug 55791) | `max index + 1`, never rename |
| rId | first unused `rIdN` from `len+1` downward, reuses gaps (`opc/package.py` `_next_rId`) | `POIXMLDocumentPart` relation counter | `max + 1`, no reuse |
| shape id | `max(//@id) + 1` (`oxml/shapes/groupshape.py` `max_shape_id`) | lowest free id (`XSLFSheet.allocateShapeId`, `nextClearBit(1)`) | `max(every @id) + 1` |
| placeholders of a new slide | `type`/`orient`/`sz`/`idx` only, no `spPr`; skips date/footer/slide number (`shapes/shapetree.py` `clone_layout_placeholders`, `slide.py` `iter_cloneable_placeholders`) | full XML copy of layout text shapes incl. prompt text; skips the same three (`XSLFSlideLayout.copyLayout`) | python-pptx's shape |
| delete slide | not available | removes sldId, notes, charts, pictures (by relation counter), not sections/custom shows/links (`XMLSlideShow.removeSlide`) | §4.2, §6 |
| move slide | not available | reorders `sldIdLst` only (`setSlideOrder`) | + sections |
| picture dedup | SHA-1 of the blob (`package.py` `_find_by_sha1`) | checksum + bytes (`findPictureData`) | SHA-256 + byte equality |
| media name | first gap in `image{N}` (`package.py` `next_image_partname`) | lowest unused `/ppt/media/image#` | `max + 1` |
| `app.xml` | not handled | not updated | counts only (§7) |

### 11. API sketch

```rust
impl Presentation {
    pub fn new() -> Result<Self>;                                   // §8
    pub fn layouts(&self) -> Result<Vec<LayoutInfo>>;                // master, part, name, type
    pub fn add_slide(&mut self, layout: &LayoutInfo, at: Option<usize>) -> Result<SlideMut<'_>>;
    pub fn delete_slide(&mut self, index: usize) -> Result<()>;
    pub fn move_slide(&mut self, from: usize, to: usize) -> Result<()>;
    pub fn duplicate_slide(&mut self, index: usize, at: Option<usize>) -> Result<SlideMut<'_>>;
}
impl SlideMut<'_> {
    pub fn add_text_box(&mut self, rect: Emu4, text: &str) -> Result<ShapeMut<'_>>;
    pub fn add_picture(&mut self, rect: Rect, bytes: &[u8]) -> Result<ShapeMut<'_>>;
    pub fn notes_text(&self) -> Option<String>;
    pub fn set_notes_text(&mut self, text: &str) -> Result<()>;
}
```

The allowlist grows by the elements written (`p:sldIdLst`/`p:sldId` →
`[write]`, `p:pic` and its non-visual/blip children, `p:clrMapOvr`,
`p14:sectionLst`/`p14:section`/`p14:sldIdLst`/`p14:sldId`, notes slide
elements); Phase 2 creates whole new slide parts as complete XML
documents (not through the lens: a new part has no original to preserve)
and edits existing parts only through the lens.

## Consequences

- Positive: decks can be generated, reordered and pruned without any
  existing part being renamed or any existing id renumbered, so modern
  comments, animations, sections and links that point into untouched
  slides stay valid by construction.
- Positive: every package-level step is checked before the first change;
  a refused operation leaves the file byte-identical.
- Negative: four new core primitives (C1–C4, C6) and an ADR-0001 note are
  prerequisites; Phase 2 cannot start before them.
- Negative: deleting a slide that another slide links to, or that is in a
  custom show, is refused rather than repaired; duplicating slides with
  charts/diagrams/OLE is refused (65 of 522 slides).
- Negative: `app.xml` title lists can become stale (§7).
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

- **Q1** Section placement of a new or moved slide (§5: the predecessor's
  section) — PowerPoint's behaviour is assumed; confirm in the manual
  check.
- **Q2** Stale `TitlesOfParts`/`HeadingPairs` after add/delete (§7): does
  PowerPoint care (ADR-0005 Q4c is the same question for text)?
- **Q3** Is refusing delete of a jump-link target (instead of removing
  the link) acceptable for Phase 2?
- **Q4** Template content (§8): the seven layouts listed, 16:9 — or 4:3,
  or the full eleven-layout set?

## Sources

- Spike: `spikes/pptx-phase2/units.py` (this ADR's tables), roadmap §3.5.
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
