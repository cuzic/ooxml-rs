# ADR-0005: PowerPoint Phase 0 — text edits in existing shapes

- Status: **Accepted** (revision 3); revision 4 (Phase 0.1, §9) **Proposed**
- Date: 2026-11-16
- Depends on: [ADR-0001](0001-lossless-round-trip-architecture.md),
  [ADR-0002](0002-schema-driven-code-generation.md) (and its post-acceptance
  notes "wildcard parents / XML characters", core PR #10, and
  "detach/insert", core PR #18, for Phase 0.1)
- Background and evidence: [`docs/roadmap/pptx.md`](../roadmap/pptx.md),
  [`spikes/pptx/`](../../spikes/pptx/README.md),
  [`reference/fixtures/pptx/`](../../reference/fixtures/pptx/README.md)
- Review: [round 1](0005-opus-review-round1.md),
  [round 2](0005-opus-review-round2.md) (converged: Minor findings only)
- Implementation: `crates/ooxml-pptx` (milestones `pptx-phase0`, `pptx-phase0.1`)

## Revision history

- **Revision 1 (2026-11-16):** first proposal.
- **Revision 2 (2026-11-16):** addresses every finding of the round-1
  review and applies the project-owner decisions of 2026-11-16; written
  together with the implementation, so every rule below is what
  `ooxml-pptx` does and is tested. Disposition per finding:

  | Finding | Change | Where |
  |---|---|---|
  | B1 comment anchors never matched | anchors classified by list kind; monikers are flat siblings; unknown or unverifiable anchors lock the whole slide | §6 "Modern comments" |
  | B2 control characters | one character table for all setters; core backstop refuses non-XML characters in every primitive (core PR #10) | §3 "Text arguments" |
  | M1 empty/field-only paragraphs, multi-line format | `TextBody::set_text` reuses paragraphs; "format source" rule with attribute-only copies, `FormatNotCopyable` otherwise; counts | §3 "Format sources", §8 |
  | M2 field refusal inconsistent | one rule: no edit removes an `a:fld` implicitly; `remove_field` is the explicit path | §3 "Fields" |
  | M3 T4 fixtures | real vs synthetic inputs (in-memory, one documented XML edit each); `bug62513` corrected | §7 T4 |
  | M4 oracles | T3 = expected model on the whole deck + read agreement; python-pptx claim fixed; T2 for paragraph/body edits | §7 |
  | M5 allowlist/`ChildOf` gaps | wildcard `ChildOf` (core PR #10); `p:pic`/`p:cxnSp`/`p:contentPart` allowlisted; `allowlist_check.py` reads the real files; AC claim sourced from `probe.py` | §5 |
  | m1 allowlist/override format | files and sections as in ADR-0002's post-acceptance note; no overrides | §5 |
  | m2 `DmlPercent` | dropped from Phase 0 (owner default) | §5 |
  | m3 `\r` | normalised by `TextBody::set_text`, refused elsewhere | §3 |
  | m4 `app.xml` | "untouched; may become stale"; owner check | §6, §7 Manual |
  | m5 autofit counts, `spAutoFit` | corrected; `spAutoFit` added | §6 |
  | m6 numbers not reproducible | every quoted count comes from a committed script (`probe.py`, `percent.py`) | throughout |
  | m7 `a:p` content, AC representative | `UnsupportedContent`; AC entry = first shape of `mc:Fallback` | §3 |
  | m8 generator warning | generated code is warning-free under `clippy -D warnings` | §5 |
  | m9 fixture licensing boundary | fixtures read by path, never packaged (`include` in `Cargo.toml`) | §7 |
  | m10 API details | `idx() -> Result`; unreadable ids refuse; hyperlink behaviour; prerequisites | §3, §6, Context |

- **Revision 3 (2026-11-16): Accepted.** The round-2 review converged
  (Blocker 0, Major 0; "may be Accepted after the Minor fixes"). All seven
  Minor findings are fixed in code, tests and text:

  | Finding | Change | Where |
  |---|---|---|
  | R2-1 panic on a body without `a:p` | `UnsupportedContent("text body without a:p")` for every body-level write; T4 input | §3 "Paragraph content", §6, §7 T4 |
  | R2-2 T3 compared attributes only | the model's `a:rPr`/`a:pPr`/`a:endParaRPr` are the whole subtree (canonical form); tests show a dropped `a:sym` and a retargeted paragraph/run are noticed | §7 T3 |
  | R2-3 python `apply` mirrored Rust; one edit per body | `check.py` performs the edits on python-pptx's own lxml tree; a multi sweep applies three successive edits per body | §7 T3 |
  | R2-4 §8 wording; 156/394 | wording fixed; T2 body figure cited | §8 |
  | R2-5 new paragraphs had no `a:endParaRPr` | they get one copying the run format source (decision: copy, as PowerPoint's Enter does) | §3 "Edits", "Format sources" |
  | R2-6 `IncompleteSpec` missing from the list | listed; ruled out by a `const` assert in `write.rs` | §3 "Losslessness rules" |
  | R2-7 not every §6 condition in T4 | every sub-condition raised in `tests/refusals.rs` (synthetic inputs) | §7 T4 |
  | m9 leftover | top-level README: `reference/fixtures/**` are third-party files under their own licences | `README.md` |

  Q2 (copy-subtree primitive) is answered by the review's recommendation and
  taken up as Phase 0.1 in a separate revision; Phase 0 as accepted here
  copies attribute-only formats.

- **Revision 4 (2026-11-16): Phase 0.1 — subtree copy of format sources**
  (§9; proposed for review). Format sources are copied as whole subtrees
  through the new core primitive `Lens::detach_as` /
  `LensMut::insert_detached` (ADR-0002 post-acceptance note
  "detach/insert"), skipping `a:hlinkClick`/`a:hlinkMouseOver`, instead of
  refusing every source with child elements. Rewritten: §3 "Format
  sources", "Hyperlinks", the prefix rule; §6 rows `FormatNotCopyable`,
  `Core(NamespaceNotInScope)`; §7 T2/T3/T4/Manual figures; §8; Alternative
  4; coordinator default "copy-subtree in Phase 2" (overridden); Q2.

## Context

ADR-0001 proved its three layers on an Excel Phase 0; ADR-0002 generates the
typed lens from the Open XML SDK data plus ECMA defaults. The foundation
(`ooxml-core`, `ooxml-schema`, `cargo xtask codegen`) is merged on `main`,
which is the prerequisite the top-level README set for starting Word and
PowerPoint; the project owner started both workstreams on 2026-11-16 (review
m10). This ADR picks the first PowerPoint scope and fixes its API,
allowlist, cross-part policy, error cases and test oracles.

What makes PowerPoint different from the Excel Phase 0 (counts from
`spikes/pptx/` over 105 real files, 546 slide parts):

- Text lives in a deep tree (`p:spTree` → `p:sp`/`p:grpSp`/`p:graphicFrame`
  → `p:txBody` → `a:p` → `a:r` → `a:t`). Order is document order / z-order.
- 16.5 % of text paragraphs are split into several runs (`runs.py`).
- Formatting is mostly **inherited** (layout → master → theme): 66 % of
  placeholder positions and 57 % of placeholder font sizes are not in the
  slide part (`placeholders.py`).
- **Empty placeholders are the common case**: 1,737 of 3,418 `p:txBody`
  (51 %) hold one empty paragraph; 2,510 of 6,121 paragraphs have no run
  (`probe.py`).
- Other nodes and parts point **into** slide text: animations
  (`p:bldP@build`, `p:txEl/p:pRg`, `p:charRg`) and modern comments
  (`ac:txMk@cp,@len`); shape ids are referenced by animations, connectors
  and comments.
- `mc:AlternateContent` sits directly under `p:sld` (transitions) and under
  `p:spTree` (Office 2016 charts).

## Decision

### 1. Scope

**In scope (Phase 0):**

1. Open and save any PresentationML package (`.pptx`, `.pptm`, `.ppsx`,
   `.potx`) with ADR-0001 Layer 1 passthrough for every part not edited.
2. List slides in presentation order (`p:sldIdLst`), with `sldId` and part
   name.
3. Walk the shapes of a slide: `p:sp`, `p:grpSp` (recursively),
   `p:graphicFrame` with `a:tbl`; `p:pic`, `p:cxnSp`, other graphic frames,
   `p:contentPart` as opaque shapes; each `mc:AlternateContent` in a shape
   tree as **one** opaque, read-only entry.
4. Read: shape id/name, **direct** placeholder `type`/`idx`, text body →
   paragraphs → items (`Run`, `Break`, `Field` with cached text, `Other`).
5. Write, only into text bodies that already exist (`p:txBody` of a `p:sp`,
   `a:txBody` of a non-merged table cell; group members included): run
   text, paragraph text, body text, add run/break/paragraph, remove
   run/paragraph/field (§3).

**Out of scope (later phases, see roadmap):** new slides, shapes, parts or
ids; notes slides (Phase 1); formatting writes (Phase 3); resolved
(inherited) values (Phase 1); cross-run find/replace (Phase 1); creating a
text body; transitions, timing, comments, charts, SmartArt (Phase 4 or
passthrough); Strict OOXML and encrypted packages (`Err`, as ADR-0001);
`Presentation::new()` (Phase 2, with "add slide").

### 2. Parts parsed and edited

| Part | Phase 0 use | State after save |
|---|---|---|
| `/_rels/.rels`, `[Content_Types].xml` | read (locate presentation) | Raw |
| presentation part (`officeDocument` rel) + its rels | Layer 2 parse, read-only | dirty-free `Parsed` → raw copy |
| each slide part (`sldId@r:id`) + its rels | Layer 2 parse on first access; **the only parts ever edited** | `Parsed` (raw copy if not dirty) |
| modern comment parts of a slide opened for editing (rel type `http://schemas.microsoft.com/office/2018/10/relationships/comments`) | parsed into a throwaway tree outside the package (§6) | Raw |
| everything else | untouched | Raw |

The presentation part is found like ADR-0002 §6 finds the workbook. Its
root must be `p:presentation` (`NotAPresentation` otherwise; any content
type variant is accepted). A `sldId` whose `r:id` is missing from the rels
or points to a missing part is listed as `Err(DanglingSlide)` (never
skipped); the other slides stay usable.

### 3. Domain API (`ooxml-pptx`, as implemented)

```rust
impl Presentation {
    pub fn open(path) -> Result<Self>;  pub fn from_bytes(Vec<u8>) -> Result<Self>;
    pub fn save(&self, path) -> Result<()>;  pub fn to_bytes(&self) -> Result<Vec<u8>>;
    pub fn slide_count(&self) -> usize;
    pub fn slides(&self) -> impl Iterator<Item = Result<SlideInfo>>;   // sldIdLst order
    pub fn slide(&mut self, i) -> Result<Slide<'_>>;                     // parse lazily, never dirties
    pub fn slide_mut(&mut self, i) -> Result<SlideMut<'_>>;              // + scans timing/comments
}
pub struct SlideInfo { pub index: usize, pub slide_id: u32, pub part_name: String }

impl Slide<'a> {
    pub fn shapes(&self) -> Vec<ShapeRef<'a>>;            // depth-first, group before members
    pub fn shape_by_id(&self, id: u32) -> Result<Option<ShapeRef<'a>>>; // AmbiguousShapeId
    pub fn placeholders(&self) -> Vec<ShapeRef<'a>>;
}
impl ShapeRef<'a> {
    pub fn path(&self) -> &ShapePath;  pub fn kind(&self) -> &ShapeKind;  pub fn depth(&self) -> usize;
    pub fn id(&self) -> Result<u32>;   pub fn name(&self) -> Option<&'a str>;
    pub fn placeholder(&self) -> Option<PlaceholderRef<'a>>;   // kind() (absent → obj), idx() -> Result<u32> (absent → 0)
    pub fn text_body(&self) -> Option<TextBodyRef<'a>>;  pub fn table(&self) -> Option<TableRef<'a>>;
}
pub enum ShapeKind { Shape, Group, Table, GraphicFrame { uri }, Picture, Connector, ContentPart, AlternateContent, Other(String) }
// TextBodyRef::{paragraphs, text}, ParagraphRef::{items, runs, text}, TextItem::{Run, Break, Field, Other},
// RunRef::text, FieldRef::{field_type, text}, TableRef::{rows, cell}, CellRef::{text_body, is_merged, grid_span, row_span}

impl SlideMut<'_> { pub fn shape_mut(&mut self, &ShapePath) -> Result<ShapeMut<'_>>; pub fn shape_mut_by_id(&mut self, u32) -> Result<ShapeMut<'_>>; }
impl ShapeMut<'_> { pub fn text_body_mut(&mut self) -> Result<TextBodyMut<'_>>; pub fn table_cell_mut(&mut self, row, col) -> Result<TextBodyMut<'_>>; }
impl TextBodyMut<'_> {
    pub fn paragraph_mut(&mut self, i) -> Result<ParagraphMut<'_>>;
    pub fn add_paragraph(&mut self, text) -> Result<ParagraphMut<'_>>;   // unformatted
    pub fn remove_paragraph(&mut self, i) -> Result<()>;
    pub fn set_text(&mut self, text) -> Result<()>;                      // reuses paragraphs
}
impl ParagraphMut<'_> {
    pub fn run_mut(&mut self, i) -> Result<RunMut<'_>>;                  // i-th a:r
    pub fn set_text(&mut self, text) -> Result<()>;
    pub fn add_run(&mut self, text) -> Result<()>;                       // unformatted
    pub fn add_break(&mut self) -> Result<()>;
    pub fn remove_run(&mut self, i) -> Result<()>;  pub fn remove_field(&mut self, i) -> Result<()>;
}
impl RunMut<'_> { pub fn set_text(&mut self, text) -> Result<()>; }
```

Domain types wrap the generated views where they carry meaning
(`PlaceholderShapeView` for `type`/`idx` defaults, `TableCellView` for
merges, `SlideIdView`); every write goes through typed `LensMut`s obtained
by a typed descent `p:sld → p:cSld → p:spTree → (p:grpSp)* → p:sp →
p:txBody` or `… p:graphicFrame → a:graphic → a:graphicData → a:tbl → a:tr →
a:tc → a:txBody`, each step a generated `ChildOf` edge.

**Losslessness rules.**

- Reading never writes; `slide_mut`/`shape_mut`/`*_mut` dirty nothing until
  a setter changes something; setting a run to its current text changes no
  byte.
- Each setter runs **all** checks of §6 and of this section on the current
  tree before its first primitive call; the remaining core refusals
  (`UnresolvedSibling`, `AlreadyPresent`) are ruled out by the content check
  below, `IncompleteSpec` (attribute writes on an element whose generated
  attribute list is incomplete) by a `const` assert over every element
  Phase 0 writes (`write.rs`, review R2-6), and `NamespaceNotInScope` can
  only come from the first insertion.
  A refused edit leaves the package byte-identical (tested for every
  refusal, §7 T4).
- `a:t` declares no `xml:space` and none of its 4,980 occurrences in the
  corpus carries one, although 1,074 have edge whitespace (209 are
  whitespace only, `probe.py`): `set_text` adds none.
- New elements take the in-scope DrawingML prefix (ADR-0002 §3); none in
  scope: `Error::Core(NamespaceNotInScope)`. Copied formats (Phase 0.1)
  get every prefix chosen at the destination by `insert_detached`; before
  the first change the setter checks that each namespace the copies use
  has a prefix bound at the paragraph (`Paragraph::set_text`) or body
  (`TextBody::set_text`) and not re-bound below it, so this refusal still
  happens before any change.

**Text arguments (review B2, m3).** One table for every setter, checked
first:

| API | `\v` | `\n` | `\r` | other non-XML 1.0 `Char` |
|---|---|---|---|---|
| `RunMut::set_text`, `ParagraphMut::add_run` | `LineBreakInRun` | `LineBreakInRun` | `LineBreakInRun` | `InvalidXmlChar` |
| `ParagraphMut::set_text`, `TextBodyMut::add_paragraph` | `a:br` | `LineBreakInParagraph` | `LineBreakInParagraph` | `InvalidXmlChar` |
| `TextBodyMut::set_text` | `a:br` | new paragraph | `\r\n` and `\r` → `\n` | `InvalidXmlChar` |

Tab is an XML character and is written as is. As a backstop, `ooxml-core`'s
`LensMut::set_text`/`set_attr` refuse any non-XML character
(`Error::InvalidXmlChar`, ADR-0002 post-acceptance note, core PR #10), so no
primitive can emit a part that is not well-formed, whatever a format crate
does.

**Paragraph content (review m7).** Writes accept only the children Phase 0
models, in schema order: `a:p` = `[a:pPr] (a:r | a:br | a:fld)* [a:endParaRPr]`,
`a:r` = `[a:rPr] a:t`, body = `a:bodyPr [a:lstStyle] a:p+`. Anything else
(`a14:m`, `mc:AlternateContent`, out-of-order children, a body without
`a:p` — review R2-1) makes every write to that paragraph or body
`UnsupportedContent` (0 such children and 0 bodies without `a:p` in the corpus;
`probe.py`: `ac_under_a_p`, `ac_under_txBody`, `other_children_of_a_p` all 0).

**Edits.**

- `RunMut::set_text`: replaces `a:t`; `a:rPr` kept.
- `ParagraphMut::set_text(text)`: split on `\v`; the first run keeps its
  `a:rPr` and gets segment 0 (even if empty); other runs and breaks are
  removed; each later segment becomes `a:br` + a run (no run for an empty
  segment). A paragraph without runs gets a run for a non-empty segment 0.
  New runs follow the *format source* rule.
- `TextBodyMut::set_text(text)` (review M1): line *i* goes into the existing
  paragraph *i* through `ParagraphMut::set_text` (so `a:pPr`, the first
  run's `a:rPr` and `a:endParaRPr` stay); surplus paragraphs are removed;
  missing ones are appended after the last `a:p`, each with a copy of the
  last existing paragraph's `a:pPr` attributes, runs formatted from its run
  format source, and an `a:endParaRPr` copying that same source (review
  R2-5: PowerPoint's Enter carries the end-of-paragraph format over, and an
  empty line's height comes from it).
- `add_run`, `add_paragraph`: the **explicitly unformatted** path — no
  `a:rPr`/`a:pPr`; the text takes the list/placeholder style, as
  python-pptx's `add_run`/`add_paragraph`. Inserted before `a:endParaRPr` /
  after the last `a:p`.
- `add_break` appends `a:br` (no `a:rPr`); `remove_run(i)` removes the i-th
  `a:r`; `remove_paragraph(i)` never the last; `remove_field(i)` removes the
  i-th `a:fld`.

**Format sources (review M1; Phase 0.1, §9).** A run created by a
format-preserving edit gets an `a:rPr` that is a **subtree copy** of the
paragraph's first `a:rPr`, or of `a:endParaRPr` when the paragraph has no
run (what PowerPoint does when one types into an empty paragraph), without
its `a:hlinkClick`/`a:hlinkMouseOver` children; a paragraph created by
`TextBody::set_text` gets a copy of the last paragraph's `a:pPr` and an
`a:endParaRPr` copied (the same way) from the last paragraph's run format
source. Copies are made with `Lens::detach_as::<C>(skip)` and inserted
with `LensMut::insert_detached` (ADR-0002 note "detach/insert";
`a:endParaRPr` ↔ `a:rPr` are generated `SameType`). The edit is refused
with `FormatNotCopyable(element)` only where `detach_as` refuses: a
relationship (`r:`) attribute in the kept subtree (e.g. `a:blipFill` or
`a:buBlip` with `r:embed`), Markup Compatibility content, or a namespace
declaration inside the source. A source with neither attributes nor
child elements (after the skip) produces no element. The source is
consulted only when a new run or paragraph is needed — for every new
paragraph, even an empty one, since it carries `a:endParaRPr`.
(Phase 0, revision 3, copied attributes only and refused every source
with child elements; §9 has the before/after figures.)

**Fields (review M2).** No edit removes an `a:fld` implicitly:
`ParagraphMut::set_text` on a paragraph with a field, `TextBodyMut::set_text`
when any paragraph has one, and `remove_paragraph` of a paragraph with one
are `FieldInParagraph`. Field text cannot be written (no API). The explicit
path is `remove_field(i)` followed by the text edit. `remove_run` indexes
`a:r` only and never touches a field.

**Shapes in `mc:AlternateContent` (review m7).** One `ShapeKind::AlternateContent`
entry per AC; its id/name are those of the first shape of `mc:Fallback`,
else of the first `mc:Choice`; for `shape_by_id` it carries the ids of the
top-level shapes of all branches, once (`Of16-01.pptx`: id 7 in both
branches is one entry). Any path through an AC, and `shape_mut_by_id` of
such an id, is `InsideAlternateContent`.

**Hyperlinks (review m10; Phase 0.1).** A first run with `a:hlinkClick`
keeps it under `Paragraph::set_text`, so its segment (the text before the
first `\v`) is the link; links on removed runs disappear (their
relationships stay, unreferenced, which is harmless). New runs and new
paragraphs' `a:endParaRPr` copy the format **without** `a:hlinkClick` /
`a:hlinkMouseOver`: a link is not spread to new text, and no `r:id` is
duplicated (`shapes.pptx` slide 5, T4 `hyperlinks_stay_on_their_run`).

### 4. Inheritance in Phase 0

Unchanged: only **direct** values (the slide's own `p:ph`; XSD defaults only
for absent attributes of that element). Resolution is Phase 1; whether
non-placeholder text resolves `p:txStyles/p:otherStyle` is deferred to
Phase 1 (coordinator default, Q6).

### 5. Allowlist and overrides

`codegen/allowlist/pml.toml` and `dml.toml`, in the `[write]`/`[read]`
`names = [...]` format of ADR-0002's post-acceptance note (review m1). An
entry's feature group is the file it is listed in; entries from the PML
schema file go in `pml.toml` even when their complex type is DrawingML
(`a:CT_TextBody/p:txBody`, `a:CT_NonVisualDrawingProps/p:cNvPr`).

| File | `[write]` | `[read]` |
|---|---|---|
| `pml.toml` | `a:CT_TextBody/p:txBody` | `p:presentation`, `p:sldIdLst`, `p:sldId`, `p:sld`, `p:cSld`, `p:spTree`, `p:grpSp`, `p:nvGrpSpPr`, `p:sp`, `p:nvSpPr`, `p:cNvPr`, `p:nvPr`, `p:ph`, `p:graphicFrame`, `p:nvGraphicFramePr`, **`p:pic`, `p:nvPicPr`, `p:cxnSp`, `p:nvCxnSpPr`, `p:contentPart`** (review M5) |
| `dml.toml` | `a:txBody`, `a:p`, `a:r`, `a:t`, `a:br` | `a:graphic`, `a:graphicData`, `a:tbl`, `a:tr`, `a:tc`, `a:fld`, **`a:rPr`, `a:endParaRPr`, `a:pPr`** |

- `a:rPr`/`a:pPr`/`a:endParaRPr` are **read** entries: Phase 0.1 inserts
  them only as detached copies (`insert_detached`, a generic primitive);
  Phase 0 inserted an empty `a:rPr`/`a:pPr` and copied attributes with the generic
  `LensMut` primitives (allowed for read entries); typed formatting setters
  are Phase 3.
- `p:cNvPr` ids of every shape kind are read structurally (first `nv*`
  child, then its `cNvPr`), which also covers `p14:nvContentPartPr`.
- **Wildcard parent (review M5).** `a:graphicData`'s content is `xsd:any`,
  so no `ChildOf<GraphicData>` existed. Core PR #10 adds the rule "SDK-listed
  children of a wildcard parent whose namespace the wildcard admits are
  children" (generator) and lets the generated const check accept them
  (`content_admits`). The generated `ChildOf<GraphicData>` impls are
  `Table`, `Graphic`, and cross-feature `Slide`, `Presentation`,
  `ContentPart` (schema-valid under `##any`, unused). `Lens::cast` does
  *not* solve this (it re-types a node with the same QName). The domain
  code checks `graphicData@uri == …/drawingml/2006/table` before
  descending.
- **No overrides** (review m2, owner default "no percentage override/type
  in Phase 0"): the only allowlisted default that does not fit its SDK type
  is `p:presentation@serverZoom` (`"50%"` vs `Int32Value`); it stays
  dropped with its `DefaultMismatch` line in `codegen/report.txt`, and
  Phase 0 never reads it. `DmlPercent` and the `set_type` vocabulary are
  decided with Phase 3.
- `spikes/pptx/allowlist_check.py` now reads these two files (it cannot
  drift from them); the "no `mc:AlternateContent` child at the write
  targets" claim comes from `probe.py` (0 under `p:txBody`, `a:txBody`,
  `a:p`). The generated code compiles under `pml`, `dml` and both, with no
  warning under `clippy -D warnings` (review m8).

### 6. Cross-part policy and preconditions (`Err` cases)

Phase 0 creates no part, relationship, content type, slide id or shape id,
so no Layer 3 registry is written. It protects references **into** the
edited text. `slide_mut` scans the slide's `p:timing` elements (anywhere
under `p:sld`, including inside AC branches) and its modern comment parts;
`shape_mut` computes, for the shape **and its enclosing groups** (for a
cell: the graphic frame), which locks apply.

**Animations.** `p:bldP` with `build` ≠ `whole` (absent = `whole`) and
`p:spTgt/p:txEl/p:pRg` lock the paragraph structure of `spid`;
`p:txEl/p:charRg` (or any other `p:txEl` child) locks every text edit of
`spid`. An `spid` that is not a number locks every shape.

**Modern comments (review B1).** A `p188:cm`'s anchor is its child other
than `p188:pos`, `replyLst`, `txBody`, `extLst`; anchor lists hold
**flat sibling monikers** (`<ac:deMkLst><pc:docMk/><pc:sldMk …/><ac:spMk id="2" …/></ac:deMkLst>`
in `modern-comments.pptx`; `ac:spMk` and `ac:graphicFrameMk` are leaf
elements in the SDK data). Classification:

- `pc:sldMkLst`, or `ac:deMkLst` with no `txMk` element and no `cp`/`len`
  attribute below it: slide or whole-drawing anchor — no lock;
- any other anchor (`ac:txMkLst`, `ac:txBodyMkLst`, `ac:tcMkLst`,
  `ac:trMkLst`, `ac:gridColMkLst`, `p188:unknownAnchor`, unknown names, or
  `deMkLst` with offsets): every `@id` of an `ac:*` moniker below it is
  locked (shape, group or graphic frame id); if it has none, **every**
  shape of the slide is locked;
- a `p188:cm` without an anchor, an unknown child of `p188:cmLst`, a
  comment relationship whose part is missing, or a comment part that does
  not parse: **every** shape of the slide is locked.

| Error | Raised when |
|---|---|
| `Core(EncryptedPackage)`, `Core(StrictNotSupported)`, `StrictConformance` | CFB file; Strict namespaces (core); `p:presentation@conformance="strict"` |
| `NotAPresentation` | no `officeDocument` part, or its root is not `p:presentation` |
| `SlideIndexOutOfRange`, `DanglingSlide { slide_id }` | bad index; `sldId@r:id` unresolved |
| `ShapeNotFound`, `AmbiguousShapeId { id }` | 0 / >1 entries carry the id (AC = one entry; ids inside `a:graphicData` are not shape ids) |
| `InvalidShapeId` | the target's or an enclosing group's `cNvPr@id` is missing or not a number (review m10; corpus 0) |
| `InsideAlternateContent` | the target is (inside) an `mc:AlternateContent` |
| `NoTextBody`, `NotATable`, `CellNotFound`, `MergedCell { row, col }` | no `p:txBody`/`a:txBody`; not a table frame; no such cell; `hMerge`/`vMerge` true |
| `ParagraphNotFound`, `RunNotFound`, `FieldNotFound` | index out of range |
| `FieldInParagraph { paragraph }` | the edit would remove an `a:fld` implicitly (§3 Fields) |
| `UnsupportedContent` | §3 Paragraph content (incl. a body without `a:p`) |
| `LastParagraph` | removing the only `a:p` |
| `FormatNotCopyable(element)` | §3 Format sources: the source holds an `r:` attribute outside the skipped links, Markup Compatibility content, or a namespace declaration |
| `AnimatedParagraphs { shape_id }` | `add_paragraph`, `remove_paragraph`, or `TextBody::set_text` that changes the paragraph count, on a paragraph-locked shape |
| `AnimatedCharacters { shape_id }` | any text edit on a character-locked shape |
| `CommentAnchoredText { shape_id }` | any text edit on a comment-locked shape |
| `InvalidXmlChar`, `LineBreakInRun`, `LineBreakInParagraph` | §3 Text arguments |
| `Core(NamespaceNotInScope)` | no DrawingML prefix in scope for a new element; a copied format uses a namespace with no stable prefix at the paragraph/body (§3) |

Not refused, documented (cosmetic, not data loss):

- **Autofit caches** (`probe.py`): `a:normAutofit` with `fontScale` 43, with
  `lnSpcReduction` 34, with either 49; `a:spAutoFit` 374 (the shape's
  `a:xfrm` is then a cache of the text extent) (review m5). Both are left as
  they are; when PowerPoint refits is the owner's manual check (Q4a).
- **`docProps/app.xml`** is untouched; its `Words`, `Paragraphs` and
  `TitlesOfParts` (e.g. `sample.pptx` lists the slide titles) **may become
  stale** after an edit (review m4); owner check Q4c.
- **Legacy comments** anchor by slide position (`p:pos`): unaffected.
- **Hyperlinks** (§3).

### 7. Test oracles

Fixtures: the 23 files of `reference/fixtures/pptx/` (79 slide parts). Tests
read them by relative path; they are never packaged into the crate
(`include` lists only `src`, `Cargo.toml`, `README.md`), so the
MPL/Apache files never enter the `MIT OR Apache-2.0` `.crate` (review m9).

- **L1:** every fixture opened, everything readable read, saved: entries
  equal (name, method, CRC-32, compressed bytes, order). After one run edit,
  every entry but the edited slide is equal.
- **T1 / T1b:** every slide part re-serializes to its source unedited (T1);
  force-re-rendered, it re-parses to a structurally equal tree (T1b).
- **T2 (locality):** for every text body (shapes and cells) of every
  fixture, on a fresh copy each time: every run set to its own text (no
  byte changes) and to a new text (diff inside that `a:t`); every
  paragraph's `set_text` (diff inside the `a:p`); `add_run` (pure insertion
  before `a:endParaRPr` or the close tag); `remove_run` and
  `remove_paragraph` (output = source minus exactly that span); body
  `set_text` (diff inside the body); `add_paragraph` (pure insertion
  between the last `a:p` and the close tag). Refused edits must leave the
  part unchanged. 2026-11-16: 3,303 checked edits, 515 runs (Phase 0); with
  Phase 0.1, 3,676 checked edits and no `FormatNotCopyable` (§9).
- **T3 (expected model; review M4).** The model is the whole deck: every
  slide in order, every `p:sp` text body and table cell outside AC, per
  paragraph the `a:pPr`/`a:endParaRPr` attributes and items
  `["r", text, rPr] | ["br"] | ["fld", cached text]`, where `a:pPr`,
  `a:endParaRPr` and `a:rPr` are compared as their **whole subtree**
  (attributes and canonical child elements, review R2-2), so a lost
  `a:solidFill`/`a:latin`/`a:sym` is a difference.
  Cases: per fixture one *sweep* that edits **every** editable body once
  with a rotating set of nine operations, one *multi sweep* that applies
  three successive edits to every body, each chosen from the body's state
  after the previous one and addressing the last paragraph/run (stale
  indices, review R2-3), plus named cases (2026-11-16: 48 cases, 1,393
  applied edits; Phase 0.1: 49 cases incl. `hyperlink-cells`, 1,580
  applied edits; refused ones are dropped and covered by T4). Tests prove
  the check notices an edit retargeted to another shape, paragraph or run,
  and an output whose kept `a:rPr` lost its `a:sym` (which an
  attribute-only model would not). Checks:
  1. in `cargo test`: `ooxml-pptx(output) == apply(edits, ooxml-pptx(input))`
     on the whole deck — an edit on the wrong shape, cell, paragraph or run
     changes the model elsewhere (a test proves the check notices a
     retargeted edit);
  2. **independent, python-pptx 1.0.2** (pinned in
     `crates/ooxml-pptx/tests/oracle/requirements.txt`; accepted by the
     owner): `examples/oracle_cases.rs` writes the outputs, `cases.json`
     and ooxml-pptx's models; `tests/oracle/check.py` checks
     `python-pptx(output) == python-pptx(edit(input))`,
     `python-pptx(output) == ooxml-pptx(output)` and
     `python-pptx(input) == ooxml-pptx(input)`. It runs in the separate CI
     job `pptx-readback`, so the Rust job is not slowed down.
     The independence (review R2-3) is in both halves: the reader is
     python-pptx, and `edit` does not compute on the JSON model as the Rust
     `apply` does — it opens the input with python-pptx, finds each body by
     shape id / cell in python-pptx's shape tree, and performs the edit on
     its lxml elements from the rules of §3 (keep the first run, remove
     other runs and breaks, deep-copy the format source, insert before
     `a:endParaRPr`, …); the expected model is read back from that tree.

  python-pptx facts (corrected, review M4): `_Paragraph.text` includes
  `a:br` as `"\v"` and field text (`content_children` = `a:r`, `a:br`,
  `a:fld`); the round-1 spike read `p.runs` only. python-pptx does not
  list shapes inside `mc:AlternateContent`, which matches the model, and
  renames slide parts when its slides collection is built, so
  `readback.py` reads part names from `p:sldIdLst` first. `ooxmlsdk` as a
  second in-cargo parse check (revision 1) is dropped: it shares the SDK
  schema data with the generator, so it adds no independence.
- **T4 (refusals; review M3, R2-7):** every error **and every
  sub-condition** of §6 is raised in `tests/refusals.rs` and the package
  must be byte-identical afterwards.
  Real inputs: `Of16-01.pptx` (AC, id 7 in both branches), `45545_Comment.pptx`
  (`bldP build="p"` + `p:pRg` on shape 584707), `table_test2.pptx` (16
  merged cells), `shapes.pptx` (connectors, groups, pictures, tables),
  `layouts.pptx` (field-only paragraph), `sample.pptx` (last paragraph,
  index errors, `a:sym`/`a:spcAft` format sources),
  `sample_pptx_grouping_issues.pptx` (`a:endParaRPr` with children),
  `modern-comments.pptx` (whole-shape anchor: allowed), `bug62513.pptx`
  (ids 0 ×3 in OLE fallbacks are **not** duplicate shape ids). **Synthetic
  inputs**, each one documented XML edit of a real file made in memory:
  `conformance="strict"`, dangling `r:id`, duplicated `cNvPr@id`, missing
  `cNvPr@id`, `a14:m` in a paragraph, `p:pRg` → `p:charRg`, `ac:deMkLst` →
  `ac:txMkLst` + `ac:txMk`, unknown anchor, malformed comment part,
  DrawingML namespace declared per element (`NamespaceNotInScope`); a CFB
  signature; an `.xlsx` (`NotAPresentation`); and (review R2-1, R2-7) a
  Strict package relationship (`Core(StrictNotSupported)`), a body without
  `a:p`, an `a:endParaRPr` before a run (out of order), a group without
  `cNvPr@id` (`InvalidShapeId` for its member), an unknown `p:txEl` child,
  a non-numeric `spid` (locks another shape), a comment relationship to a
  missing part, and a `p188:cm` without anchor. Character rules are tested
  per setter × (`\v`, `\n`, `\r`, U+0000, U+0001, U+001F, U+FFFE).
- **Manual (owner decision):** `cargo run -p ooxml-pptx --example
  manual_check -- <outdir>` writes thirteen named outputs (title/body, autofit,
  empty placeholder fill, table cells, group member, slide outside every
  section, stale `app.xml`, comment-anchored shape, animated paragraph run
  edit, removed field; Phase 0.1: formatted new lines, formatted empty
  paragraph fill, hyperlink run) and `CHECKLIST.md` with "opens without repair" and
  the owner questions Q4a–c as observation items. The owner opens them in
  current Microsoft 365 PowerPoint.

### 8. Consequences

- Positive: Phase 0 needs only ADR-0002's primitives plus two additive core
  rules (core PR #10), no override and no Layer 3 machinery.
- Positive: every reference into slide text that the corpus shows
  (animations, modern comments, fields, AC branches) is preserved
  byte-for-byte or refused with a named error, and unknown comment anchors
  are refused rather than guessed.
- (Phase 0.1 removes this; kept for the record, §9.) Negative in Phase 0 —
  **format copies are attribute-only**. Corpus frequency of sources with child
  elements (`probe.py`): first `a:rPr` of 1,665 of 3,512 paragraphs with
  runs (47 %), `a:pPr` of 2,333 of 6,121 paragraphs (38 %), `a:endParaRPr`
  of 469 of 2,510 run-less paragraphs (19 %). So `\v` in `Paragraph::set_text`,
  growing a body with `TextBody::set_text`, and filling ~1 in 5 empty
  paragraphs are refused with `FormatNotCopyable` that often; in the T2
  sweep 217 of 599 two-segment `Paragraph::set_text` calls and 156 of 394
  two-line `TextBody::set_text` calls (40 %, the template-filling case)
  were. Not affected: run text, and paragraph/body text that only rewrites
  paragraphs which already have a run (review R2-4: filling an *empty*
  paragraph needs a new run and is refused when its `a:endParaRPr` has
  children, 469 of 2,510). `add_run`/`add_paragraph` remain as explicit
  unformatted paths. See Q2.
- Negative: decks with "build by paragraph" animations refuse paragraph
  structure edits on those shapes even when the animation would survive.
- Negative: autofit scale and `spAutoFit` geometry can be stale after an
  edit; `app.xml` statistics can be stale (cosmetic; manual check).
- Negative: python-pptx brings Python into CI (separate job, pinned).

### 9. Phase 0.1 — subtree copy of format sources (revision 4)

**Why now.** In the Phase 0 T2 sweep, 217 of 599 two-segment
`Paragraph::set_text` calls (36 %) and 156 of 394 two-line
`TextBody::set_text` calls (40 %, the template-filling case) were refused
with `FormatNotCopyable`; in the corpus 47 % of first `a:rPr`, 38 % of
`a:pPr` and 19 % of `a:endParaRPr` have child elements (§8). A copy inside
one part needs no new part, relationship, content type or id, so it stays
inside the Phase 0 boundary (no Layer 3 writes). The round-2 review
recommended it (Q2) and sketched the API; the coordinator default "Phase 2"
is overridden on that evidence.

**Core primitive** (ADR-0002 post-acceptance note "detach/insert", core PR
#18): `Lens::detach_as::<C: SameType<S>>(skip) -> Detached<C>` (owned,
span-less copy carrying namespace URIs, renamed to `C`'s QName; refuses MC
content, `r:` attributes and namespace declarations) and
`LensMut::insert_detached[_by]` (placed like `insert_by`, every prefix
re-chosen at the destination before any change). `SameType` is generated
for allowlisted pairs sharing an SDK complex type (`a:endParaRPr` ↔
`a:rPr`).

**Rules in `ooxml-pptx`** (§3 "Format sources", "Hyperlinks"):

| Copy | Source | Skip | Refused with |
|---|---|---|---|
| new run's `a:rPr` | first `a:rPr`, else `a:endParaRPr` | `a:hlinkClick`, `a:hlinkMouseOver` | `FormatNotCopyable("a:rPr" / "a:endParaRPr")` |
| new paragraph's `a:pPr` | last paragraph's `a:pPr` | — | `FormatNotCopyable("a:pPr")` |
| new paragraph's `a:endParaRPr` | last paragraph's run format source | links | as for `a:rPr` |

All copies of one edit are made during planning (before any change), and
their namespaces are checked against the paragraph/body scope (§3), so a
refused edit still leaves the package byte-identical.

**Effect** (same fixtures and sweeps as §7):

| Measure | Phase 0 (rev 3) | Phase 0.1 |
|---|---|---|
| T2: two-segment `Paragraph::set_text` refused `FormatNotCopyable` | 217 / 599 | 0 / 599 |
| T2: two-line `TextBody::set_text` refused `FormatNotCopyable` | 156 / 394 | 0 / 394 |
| T2: checked edits | 3,303 | 3,676 |
| T3: applied edits (cases) | 1,393 (48) | 1,580 (49) |
| T3: `FormatNotCopyable` among attempted edits | 184 | 0 |

**Oracles.** T3's model already compares the whole format subtree (review
R2-2), so the copies are checked in Rust and, independently, by
`check.py`, which deep-copies the source element in python-pptx's lxml
tree and removes the link children. A named case `hyperlink-cells`
covers the link skip; disabling the skip in `check.py` makes it fail.
T4 (`tests/refusals.rs`): `format_sources` (real sources with child
elements are copied: `a:endParaRPr` with fill, `a:rPr` with `a:sym`, `a:pPr`
with `a:spcAft`), `hyperlinks_stay_on_their_run` (real), `format_not_copyable`
(synthetic `a:blipFill`/`a:buBlip` with `r:embed`, `mc:AlternateContent`,
namespace declaration), `copied_namespaces_must_be_in_scope_before_any_change`
(synthetic; plus a positive control). Manual check: items 11–13.

**Still not copied.** A format source that references a relationship
(picture fill, picture bullet; 0 in the T2/T3 sweeps of the 23 fixtures)
would need a new relationship or a shared one (Layer 3) — later phase.

## Alternatives considered

1. **Start with "add slide".** Needs five registries at once (roadmap §3.5).
   Deferred to Phase 2.
2. **python-pptx semantics for `Paragraph::set_text`** (drop all runs, new
   run without `a:rPr`). Rejected: loses visible formatting in the 58 % of
   multi-run paragraphs whose runs differ visibly. Kept as the explicit
   `add_run`/`add_paragraph`.
3. **New runs without `a:rPr` when the source is not copyable** (review M1
   option a). Rejected: silently differs from what PowerPoint does in
   ~19–47 % of the cases above; an error plus the explicit unformatted path
   is honest (ADR-0001 "fail rather than guess").
4. **A subtree-copy primitive now** (review M1 option c). Deferred by the
   coordinator for Phase 0; taken up as Phase 0.1 (§9) on the counts in §8
   and the round-2 review's recommendation.
5. **Resolve inheritance in Phase 0.** Rejected: needs its own oracle.
6. **Edit both AC branches.** Not in Phase 0: branches may hold different
   element types.

## Coordinator defaults (2026-11-16, recorded as such)

- No percentage override/type in Phase 0 (`serverZoom` `set_type` dropped).
- Copy-subtree primitive deferred to Phase 2 (§3 Format sources) —
  **overridden** on 2026-11-16 (round 3): Phase 0.1 (§9), on the round-2
  review's recommendation and the T2 figures; to be confirmed by the owner.
- Keep looking for a real text-anchored modern-comment fixture in
  third-party corpora; not blocking (synthetic input in T4).
- Master `otherStyle` inheritance question deferred to Phase 1.

## Open questions for the project owner

- **Q1** (resolved: no `DmlPercent`/override in Phase 0.)
- **Q2** (answered by the round-2 review: yes, as Phase 0.1 — §9, with its
  own core PR #18.) Given §8's counts (47 % / 38 % / 19 % of format sources have child
  elements), should the copy-subtree primitive (`LensMut::insert_copy`,
  sketched in the core README) move from Phase 2 into a Phase 0.1, so that
  `\v`, multi-line body text and empty-paragraph fill copy formatting the
  way PowerPoint does instead of refusing?
- **Q3** (resolved: python-pptx in CI is the independent read-back.)
- **Q4** Manual check (`examples/manual_check.rs`): (a) when PowerPoint
  recomputes `normAutofit` after a text change; (b) whether a slide outside
  every `p14:section` is accepted, repaired or placed somewhere (Phase 2
  input); (c) whether stale `app.xml` counts/titles matter.
- **Q5** A real text-anchored modern comment (`ac:txMk`) is still wanted
  (0 in 105 files); may we create one in PowerPoint if third-party corpora
  have none?
- **Q6** (deferred to Phase 1: `otherStyle` for non-placeholder text.)

## Sources

- Roadmap and evidence: `docs/roadmap/pptx.md`; scripts `spikes/pptx/`
  (`survey.py`, `placeholders.py`, `percent.py`, `runs.py`, `probe.py`,
  `allowlist_check.py`, `readback.py`, `ppy_add_slide_check.py`)
- Implementation and oracles: `crates/ooxml-pptx/` (`tests/roundtrip.rs`,
  `tests/locality.rs`, `tests/oracle.rs`, `tests/oracle/*`,
  `tests/refusals.rs`, `examples/oracle_cases.rs`, `examples/manual_check.rs`)
- Fixtures: `reference/fixtures/pptx/README.md`
- Open XML SDK data `431ab05c` (`p188:CT_Comment`, `oac:CT_ShapeMoniker`,
  `oac:CT_GraphicFrameMoniker`); ECMA-376 5th ed. Transitional XSDs;
  ECMA-376 Part 3 (Markup Compatibility)
- python-pptx 1.0.2: `oxml/text.py` (`content_children`), `text/text.py`
  (`_Paragraph.text`), `parts/presentation.py` (`rename_slide_parts`)
