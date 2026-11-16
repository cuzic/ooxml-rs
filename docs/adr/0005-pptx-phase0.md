# ADR-0005: PowerPoint Phase 0 — text edits in existing shapes

- Status: **Proposed**
- Date: 2026-11-16
- Depends on: [ADR-0001](0001-lossless-round-trip-architecture.md),
  [ADR-0002](0002-schema-driven-code-generation.md)
- Background and evidence: [`docs/roadmap/pptx.md`](../roadmap/pptx.md),
  [`spikes/pptx/`](../../spikes/pptx/README.md),
  [`reference/fixtures/pptx/`](../../reference/fixtures/pptx/README.md)

## Context

ADR-0001 proved its three layers on an Excel Phase 0; ADR-0002 generates the
typed lens from the Open XML SDK data plus ECMA defaults, and its revision 5
checked that the slot rules hold for PresentationML (0 unsupported base
models; 0 order violations over the POI slides). This ADR picks the first
PowerPoint scope and fixes its API, allowlist, overrides, cross-part policy,
error cases and test oracles, in the same shape as ADR-0001's Excel Phase 0.

What makes PowerPoint different from the Excel Phase 0 (numbers from the
roadmap, 105 real files):

- Text lives in a deep tree (`p:spTree` → `p:sp`/`p:grpSp`/`p:graphicFrame`
  → `p:txBody` → `a:p` → `a:r` → `a:t`), not in a flat keyed grid. Order inside
  `a:p` and `p:spTree` is document order / z-order chosen by the caller.
- 16.5 % of text paragraphs are split into several runs; a phrase often
  crosses a run boundary.
- Formatting is mostly **inherited** (layout → master → theme): 66 % of
  placeholder positions and 57 % of placeholder font sizes are not in the
  slide part.
- Other parts and nodes point **into** a slide's text: animations
  (`p:bldP@build="p"`, `p:txEl/p:pRg`, `p:charRg`) and modern comments
  (`ac:txMk@cp,@len`) refer to paragraph and character positions; shape ids
  are referenced by animations, connectors and comments.
- `mc:AlternateContent` sits directly under `p:sld` (transitions, 7 files)
  and under `p:spTree` (Office 2016 charts, 3 files).

## Decision

### 1. Scope

**In scope (Phase 0):**

1. Open and save any PresentationML package (`.pptx`, `.pptm`, `.ppsx`,
   `.potx`) with ADR-0001 Layer 1 passthrough for every part not edited.
2. List slides in presentation order (`p:sldIdLst`), with `sldId` and part
   name.
3. Walk the shapes of a slide: `p:sp`, `p:grpSp` (recursively),
   `p:graphicFrame` containing `a:tbl` (cells), and report `p:pic`,
   `p:cxnSp`, other graphic frames and `p:contentPart` as opaque shapes.
   Report shapes inside `mc:AlternateContent` as opaque, read-only shapes.
4. Read: shape id/name, **direct** placeholder `type`/`idx`, text body →
   paragraphs → items (`Run(text)`, `Break`, `Field{type, cached text}`).
5. Write, only into text bodies that already exist:
   - `Run::set_text`;
   - `Paragraph::set_text` (first run keeps its `a:rPr`, other runs and
     breaks are removed; `\v` becomes `a:br`);
   - `Paragraph::add_run(text)` (new `a:r` **without** `a:rPr`, before
     `a:endParaRPr`), `Paragraph::add_break()`;
   - `TextBody::add_paragraph(text)` (appended after the last `a:p`),
     `TextBody::set_text(text)` (split on `\n`; first paragraph keeps its
     `a:pPr` and first run's `a:rPr`, other paragraphs removed);
   - `remove_run`, `remove_paragraph` (never the last `a:p`).
6. The same edits on table cells (`a:tc/a:txBody`) and on group members.

**Out of scope (later phases, see roadmap):** new slides, shapes, parts or
ids of any kind; notes slides (Phase 1); formatting writes (`a:rPr`/`a:pPr`
attributes, Phase 3); resolved (inherited) values (Phase 1); cross-run
find/replace (Phase 1); creating a text body in a shape that has none;
transitions, timing, comments, charts, SmartArt (Phase 4 or passthrough);
Strict OOXML (`Err`, as ADR-0001); encrypted packages (`Err`, as ADR-0001).

There is no from-scratch path in Phase 0. `Presentation::new()` (embedded
template, same code path) is Phase 2, together with "add slide", because a
blank template has no shapes to edit.

### 2. Parts parsed and edited

Following ADR-0002 §5 "parse scope":

| Part | Phase 0 use | State after save |
|---|---|---|
| `/_rels/.rels`, `[Content_Types].xml` | read (locate presentation) | Raw |
| presentation part (`officeDocument` rel; content type varies by `.pptx/.pptm/.ppsx/.potx`) + its rels | Layer 2 parse, read-only | dirty-free `Parsed` → raw copy |
| each slide part (via `sldId@r:id` → presentation rels) + its rels | Layer 2 parse when first accessed; **the only parts that are ever edited** | `Parsed` or `Replaced` |
| modern comment parts of an edited slide (rel type `http://schemas.microsoft.com/office/2018/10/relationships/comments`) | event scan only (§6), no tree | Raw |
| everything else (layouts, masters, themes, notes, media, charts, embeddings, fonts, VBA, custom XML) | untouched | Raw |

The presentation part is found exactly like ADR-0002 §6 finds the workbook:
package `officeDocument` relationship → part → its rels part derived from the
part name. A `sldId` whose `r:id` is missing from the rels, or points to a
part that does not exist, makes `slides()` return `Err(DanglingSlide)` for
that entry (not a silent skip).

### 3. Domain API sketch (`ooxml-pptx`)

Domain types wrap ADR-0002's generated views (`*View` / `*ViewMut`), as
`ooxml-xlsx` wraps them for cells. Names are indicative.

```rust
pub struct Presentation { /* Package + slide index (sldId, rId, part name) */ }

impl Presentation {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, Error>;
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, Error>;
    pub fn save(&mut self, path: impl AsRef<Path>) -> Result<(), Error>; // ADR-0001: temp file + rename
    pub fn slides(&self) -> impl Iterator<Item = Result<SlideInfo, Error>>; // order of sldIdLst
    pub fn slide(&mut self, index: usize) -> Result<Slide<'_>, Error>;    // parses lazily
    pub fn slide_mut(&mut self, index: usize) -> Result<SlideMut<'_>, Error>;
}

pub struct SlideInfo { pub slide_id: u32, pub part_name: PartName }

impl Slide<'_> {
    /// Depth-first in document order (= z-order), group members after their group.
    pub fn shapes(&self) -> impl Iterator<Item = ShapeRef<'_>>;
    /// Err(AmbiguousShapeId) if the id occurs on more than one shape outside AC.
    pub fn shape_by_id(&self, id: u32) -> Result<Option<ShapeRef<'_>>, Error>;
    pub fn placeholders(&self) -> impl Iterator<Item = ShapeRef<'_>>;
}

pub struct ShapePath(/* child-index path from p:spTree, see ADR-0002 "Node id" */);

impl ShapeRef<'_> {
    pub fn path(&self) -> ShapePath;
    pub fn id(&self) -> Result<u32, AttrError>;
    pub fn name(&self) -> Option<&str>;
    pub fn kind(&self) -> ShapeKind;          // Shape | Group | Table | Picture | Connector | GraphicFrame(uri) | ContentPart | InAlternateContent
    pub fn placeholder(&self) -> Option<PlaceholderRef<'_>>; // direct p:ph only
    pub fn text_body(&self) -> Option<TextBodyRef<'_>>;
    pub fn table(&self) -> Option<TableRef<'_>>;
}

impl PlaceholderRef<'_> {
    pub fn kind(&self) -> EnumValue<'_, PlaceholderValues>; // absent @type → obj (XSD default)
    pub fn idx(&self) -> u32;                               // absent @idx → 0 (XSD default)
}

impl TextBodyRef<'_> {
    pub fn paragraphs(&self) -> impl Iterator<Item = ParagraphRef<'_>>;
    pub fn text(&self) -> String; // paragraphs joined by '\n', a:br as '\v', fields by cached text
}
pub enum TextItem<'a> { Run(RunRef<'a>), Break, Field(FieldRef<'a>), Other(QName) /* a14:m */ }

impl SlideMut<'_> {
    pub fn shape_mut(&mut self, path: &ShapePath) -> Result<ShapeMut<'_>, Error>;
    pub fn shape_mut_by_id(&mut self, id: u32) -> Result<ShapeMut<'_>, Error>;
}
impl ShapeMut<'_> {
    pub fn text_body_mut(&mut self) -> Result<TextBodyMut<'_>, Error>;       // Err(NoTextBody)
    pub fn table_cell_mut(&mut self, row: usize, col: usize) -> Result<TextBodyMut<'_>, Error>;
}
impl TextBodyMut<'_> {
    pub fn paragraph_mut(&mut self, i: usize) -> Result<ParagraphMut<'_>, Error>;
    pub fn add_paragraph(&mut self, text: &str) -> Result<ParagraphMut<'_>, Error>;
    pub fn remove_paragraph(&mut self, i: usize) -> Result<(), Error>;
    pub fn set_text(&mut self, text: &str) -> Result<(), Error>;
}
impl ParagraphMut<'_> {
    pub fn run_mut(&mut self, i: usize) -> Result<RunMut<'_>, Error>;    // i-th a:r
    pub fn add_run(&mut self, text: &str) -> Result<RunMut<'_>, Error>;
    pub fn add_break(&mut self) -> Result<(), Error>;
    pub fn remove_run(&mut self, i: usize) -> Result<(), Error>;
    pub fn set_text(&mut self, text: &str) -> Result<(), Error>;
}
impl RunMut<'_> { pub fn set_text(&mut self, text: &str) -> Result<(), Error>; }
```

Rules that matter for losslessness:

- As in ADR-0001, reading never writes: `slide()`/`shapes()`/`text()` leave
  the part dirty-free; `slide_mut()`/`shape_mut()`/`text_body_mut()` do not
  dirty anything until a setter runs.
- Each setter checks **all** of §6's preconditions before its first
  primitive call, so a refused edit leaves the tree untouched (no partial
  edits).
- `add_run` creates `<a:r><a:t>…</a:t></a:r>` with no `a:rPr`: the run
  inherits the paragraph/list/placeholder style, the same as python-pptx's
  `add_run`. Copying a neighbour's `a:rPr` needs a subtree-clone primitive
  that ADR-0002 does not have (open question Q2).
- `set_text` uses ADR-0002's `set_text` on `a:t`. `a:t` declares no
  `xml:space` (SDK `xsd:string/a:t`, 4,848 `a:t` in the corpus, none with
  `xml:space`, 1,070 with edge spaces kept by plain XML parsing), so none is
  added.
- New `a:p`, `a:r`, `a:t`, `a:br` take the prefix of the in-scope binding for
  the DrawingML namespace (ADR-0002 §3; in practice `a:`, declared on
  `p:sld`). If none is in effect: `Err(NamespaceNotInScope)`.

### 4. Inheritance in Phase 0

Phase 0 exposes **only direct values** (§3 `PlaceholderRef` reads the slide's
own `p:ph`; XSD defaults apply only to absent attributes of that element). No
API returns a layout/master value, and no write copies one into the slide.
Resolution (`Resolved<T> { value, source }`, idx-then-type matching) is
Phase 1, designed in the roadmap §3.1.

### 5. Allowlist and overrides

Proposed contents of `codegen/allowlist/pml.toml` and
`codegen/allowlist/dml.toml` (SDK `Name`s, ADR-0002 §5 unit). Checked
against the SDK data by `spikes/pptx/allowlist_check.py`: all exist, all
lower to slots with no unsupported model, and none has an AC child in the
corpus at the write targets.

**`pml.toml`**

| SDK Name | Mode | Why |
|---|---|---|
| `p:CT_Presentation/p:presentation` | read | root of slide list |
| `p:CT_SlideIdList/p:sldIdLst` | read | |
| `p:CT_SlideIdListEntry/p:sldId` | read | `id`, `r:id` |
| `p:CT_Slide/p:sld` | read | slide root (no inserts: AC may be a child) |
| `p:CT_CommonSlideData/p:cSld` | read | |
| `p:CT_GroupShape/p:spTree` | read | shape tree |
| `p:CT_GroupShape/p:grpSp` | read | groups |
| `p:CT_GroupShapeNonVisual/p:nvGrpSpPr` | read | group id/name |
| `p:CT_Shape/p:sp` | read | |
| `p:CT_ShapeNonVisual/p:nvSpPr` | read | |
| `a:CT_NonVisualDrawingProps/p:cNvPr` | read | `id`, `name` |
| `p:CT_ApplicationNonVisualDrawingProps/p:nvPr` | read | |
| `p:CT_Placeholder/p:ph` | read | `type` (default `obj`), `idx` (default 0) |
| `p:CT_GraphicalObjectFrame/p:graphicFrame` | read | tables |
| `p:CT_GraphicalObjectFrameNonVisual/p:nvGraphicFramePr` | read | |
| `a:CT_TextBody/p:txBody` | **write** | `insert::<Paragraph>`, `remove` |

**`dml.toml`**

| SDK Name | Mode | Why |
|---|---|---|
| `a:CT_GraphicalObject/a:graphic` | read | |
| `a:CT_GraphicalObjectData/a:graphicData` | read | `uri`; `a:tbl` via the `Any` slot |
| `a:CT_Table/a:tbl` | read | |
| `a:CT_TableRow/a:tr` | read | |
| `a:CT_TableCell/a:tc` | read | `gridSpan`, `rowSpan`, `hMerge`, `vMerge` |
| `a:CT_TextBody/a:txBody` | **write** | table cell text |
| `a:CT_TextParagraph/a:p` | **write** | slots `[pPr] [r, br, fld, a14:m] [endParaRPr]` |
| `a:CT_RegularTextRun/a:r` | **write** | slots `[rPr] [t]` |
| `xsd:string/a:t` | **write** | `set_text` |
| `a:CT_TextLineBreak/a:br` | **write** | `add_break`, `\v` |
| `a:CT_TextField/a:fld` | read | `type`, cached `a:t` |

Not allowlisted on purpose: `a:rPr`, `a:pPr`, `a:endParaRPr`, `a:bodyPr`,
`a:lstStyle` (their QNames are still in the parents' `CONTENT`, so slot
positions are correct; no typed accessors are needed in Phase 0), and
`p:timing` (§6 walks it untyped).

Notes for the generator owner:

- `a:CT_TextBody/p:txBody` is in the PML schema file but its children are
  DML, so `impl ChildOf<PTxBody> for Paragraph` is cross-feature and gets
  `#[cfg(all(feature = "pml", feature = "dml"))]`; likewise
  `p:graphicFrame → a:graphic`. `ooxml-pptx` enables `pml` and `dml` (and
  `a14` only if `a14:m` gets a marker; Phase 0 does not need it).
- The SDK lists `a:tbl` (and, noisily, `p:sld`/`p:presentation`) as known
  children of `a:graphicData`, whose content is a wildcard. Only
  `ChildOf<GraphicData> for Table` is wanted; the domain code checks
  `graphicData@uri == "http://schemas.openxmlformats.org/drawingml/2006/table"`
  before viewing the child as a table.

**Overrides (`codegen/overrides.toml`)** — exactly one:

```toml
[[set_type]]
on = "p:CT_Presentation/p:presentation"
attribute = "serverZoom"
type = "DmlPercent"   # ST_Percentage union: int (1/1000 %) | "NN%"
reason = "XSD type a:ST_Percentage, default \"50%\"; SDK says Int32Value, so the default is dropped with DefaultMismatch otherwise"
evidence = "ECMA-376 Part 1 pml.xsd CT_Presentation; docs/roadmap/pptx.md §3.2 (0 of 105 files use the % form; serverZoom absent in all)"
```

This requires `ooxml-core` to provide the `DmlPercent` simple type (ADR-0002
§1.2 already anticipates "union simple types" in the core). If the
foundation prefers not to add it before Phase 3, the alternative is to
accept the generator's `DefaultMismatch` diagnostic for this one read-only
attribute (Phase 0 never reads `serverZoom`); see Q1. No `mce_wrapped`
override is needed: `p:transition` occurs both directly (205 slides) and
inside AC (12), so it is not MCE-only.

### 6. Cross-part policy and preconditions (`Err` cases)

Phase 0 creates **no** part, relationship, content type, slide id or shape
id, so no Layer 3 registry is written. What it must protect are references
**into** the edited text. Before an edit, the domain layer checks (cheap
scans of the already-parsed slide tree, plus an event scan of the slide's
modern comment parts):

| Error | Raised when | Why |
|---|---|---|
| `Error::EncryptedPackage`, `Error::StrictConformance` | as ADR-0001 (`p:presentation@conformance="strict"` or Strict namespace URIs) | |
| `Error::NotAPresentation` | `officeDocument` target is not a PML presentation | |
| `Error::DanglingSlide { slide_id }` | `sldId@r:id` unresolved | no silent skip |
| `Error::ShapeNotFound`, `Error::AmbiguousShapeId { id }` | id-based addressing finds 0 / >1 shapes (AC branches count as one) | duplicates exist (`bug62513.pptx`) |
| `Error::InsideAlternateContent` | target shape is inside `mc:Choice`/`mc:Fallback` | editing one branch desyncs the other (`Of16-01.pptx`) |
| `Error::NoTextBody` | shape has no `p:txBody` / cell has no `a:txBody` | creating one is out of scope |
| `Error::MergedCell { row, col }` | `a:tc@hMerge` or `@vMerge` true | text in a covered cell is not shown |
| `Error::FieldText` / `Error::FieldInParagraph` | writing a field, or `Paragraph::set_text`/`remove_run` would remove an `a:fld` | field text is an Office-computed cache |
| `Error::LastParagraph` | removing the only `a:p` (`p:txBody` requires ≥ 1) | schema `minOccurs=1` |
| `Error::AnimatedParagraphs { shape_id }` | a **paragraph-structure** edit (add/remove/`TextBody::set_text`) on a shape that the slide's `p:timing` (including inside AC) references with `p:bldP@build` ≠ `whole`, `p:txEl/p:pRg` or `p:txEl/p:charRg` | paragraph indices would shift (`45545_Comment.pptx`: `bldP build="p"` + 97 `p:pRg` in the corpus) |
| `Error::AnimatedCharacters { shape_id }` | any text edit on a shape referenced by `p:txEl/p:charRg` | character offsets would shift |
| `Error::CommentAnchoredText { shape_id }` | any text edit on a shape that a modern comment of this slide anchors with `ac:txMk` (under an `ac:spMk@id` = shape id) | `cp`/`len` offsets would shift |
| `Error::InvalidXmlChar` | text contains a character not allowed in XML 1.0 (other than `\n`, `\v` handled as below) | |
| `Error::LineBreakInRun` | `\n` or `\v` passed to `Run::set_text`; `\n` to `Paragraph::set_text` | use `add_break` / `TextBody::set_text` |
| ADR-0002 errors | `UnresolvedSibling` (AC child in an insert target, never observed), `AlreadyPresent`, `NamespaceNotInScope` | |

Not refused, documented instead:

- **Autofit caches** (`a:normAutofit@fontScale`, `@lnSpcReduction`, 43 in the
  corpus) are left as they are; longer text may overflow until PowerPoint
  refits the box (**unverified** when it does; Q4). This is cosmetic, not
  data loss, which is why it is not an `Err`.
- **`docProps/app.xml`** is untouched (no counts change in Phase 0).
- **Legacy comments** anchor by slide position (`p:pos`), not text, so they
  are unaffected.

The animation and comment checks mirror ADR-0001's formula scan: they read
bytes, never change a part's state, and an untouched part stays Raw.

### 7. Test oracles

Fixture set: `reference/fixtures/pptx/` (23 third-party files; roadmap §1).

- **L1 (passthrough):** open + save with no edits; every part's
  (name, method, CRC32, compressed bytes) equal (ADR-0001 Layer 1). After an
  edit, the same holds for every part except the edited slide.
- **T1 / T1b (per slide part):** as ADR-0001, on every slide part of every
  fixture (T1: zero-edit byte identity; T1b: force-dirty re-render,
  re-parse, structural equality).
- **T2 (locality):** for every `a:r` in every editable text body of every
  fixture: `set_text` to the same value, then to a different value; the
  byte diff is confined to that `a:t`'s span (or, if it was `<a:t/>`, to its
  open tag, the ADR-0002 self-closing case). For `add_paragraph`/`add_run`,
  the diff is a pure insertion at the slot boundary ADR-0002 §4.2 defines
  (after the last `a:p` / before `a:endParaRPr`). For `remove_*`, the diff
  removes exactly the removed span.
- **T3 (independent read-back):** for every edited output, a reader that
  shares no code with ooxml-rs must read the same slide texts as
  `ooxml-pptx` does. Choice:
  - **Primary: python-pptx 1.0.2** via `spikes/pptx/readback.py` (to move
    under `crates/ooxml-pptx/tests/oracle/` in the implementation round),
    run in a separate CI job with a pinned virtualenv; the Rust test
    compares its JSON with `ooxml-pptx`'s own dump. Justification: it is
    the most used independent PresentationML implementation, written on
    lxml with hand-written element classes (no shared schema data with
    ooxml-rs), it opens all 23 fixtures today, and it is what "does another
    tool see my text" means for most users. Limitations, recorded so tests
    do not over-claim: it reads `a:r` only (no field text), and it does not
    see shapes inside `mc:AlternateContent`.
  - **Secondary, inside `cargo test`: `ooxmlsdk` 0.15** as a dev-dependency
    (the ADR-0002 "test oracle" role): every edited part must parse, and
    run texts must match. It is Rust and needs no Python, but it shares the
    SDK schema data with our generator, so it is **not** independent on
    schema interpretation; it is a parse check, not the oracle of record.
  - Rejected: a second hand-written Rust reader in this repo (not
    independent), LibreOffice headless (not available in the sandbox, as
    ADR-0001 records).
- **T4 (error cases):** each `Err` in §6 has at least one fixture-based test
  that also asserts the package bytes are unchanged after the refused edit:
  `Of16-01.pptx` (AC), `45545_Comment.pptx` (animated paragraphs),
  `modern-comments.pptx` (comment anchors: `ac:spMk` exists; a text anchor
  `ac:txMk` needs a fixture, Q5), `bug62513.pptx` (duplicate ids),
  `layouts.pptx` (fields), `table_test2.pptx` (16 merged-away cells, 13 spans) and `shapes.pptx`
  (merged cells), `with_japanese.pptx` (plain table), `sample.pptx` (last
  paragraph).
- **Manual:** as ADR-0001, opening outputs in PowerPoint without a repair
  dialog is checked by the project owner on a recorded sample; the sandbox
  has no PowerPoint.

### 8. Consequences

- Positive: the first PowerPoint feature needs only primitives ADR-0002
  already defines (`insert`, `remove`, `set_text`), one override, and no
  Layer 3 machinery; it reuses ADR-0001's oracles unchanged.
- Positive: every reference into slide text that the corpus shows
  (animations, modern comments, fields, AC branches) is either preserved
  byte-for-byte or refused with a named error.
- Negative: new runs have no formatting of their own; callers who need the
  neighbour's look must wait for a clone primitive or Phase 3.
- Negative: decks with "build by paragraph" animations refuse paragraph
  structure edits on those shapes even when the animation would survive.
- Negative: autofit scale can be stale after an edit (cosmetic).
- Negative: python-pptx as T3 brings Python into CI.

## Alternatives considered

1. **Start with "add slide".** Highest visible value, but needs five
   registries at once (roadmap §3.5) and decisions without an oracle
   (sections, `app.xml`). Deferred to Phase 2.
2. **python-pptx semantics for `Paragraph::set_text`** (drop all runs, new
   run without `a:rPr`). Rejected: it loses visible formatting in the 58 % of
   multi-run paragraphs whose runs differ visibly and in every single-run
   paragraph with explicit formatting.
3. **Resolve inheritance already in Phase 0.** Rejected: the semantics need
   their own oracle and are not needed for in-place text edits.
4. **Treat AC-wrapped shapes as editable by editing both branches.** Not in
   Phase 0: branches may hold different element types (`graphicFrame` vs
   `pic`), so "the same edit" is not defined.

## Open questions for the project owner

- **Q1** Add `DmlPercent` to `ooxml-core` now (one `set_type` in Phase 0), or
  tolerate one `DefaultMismatch` diagnostic until Phase 3?
- **Q2** Should ADR-0002 gain a `clone_subtree`/`insert_clone` primitive so
  `add_run` can copy the neighbour's `a:rPr` (and Phase 2 can clone layout
  placeholders)?
- **Q3** Is Python (python-pptx) acceptable in CI as the T3 oracle of
  record, with `ooxmlsdk` as the in-cargo parse check?
- **Q4** Can the owner check in PowerPoint (a) when autofit is recomputed,
  (b) whether a slide outside every `p14:section` is repaired, (c) whether
  stale `app.xml` counts matter (Phase 2)?
- **Q5** A fixture with a modern comment anchored to text (`ac:txMk`) is not
  yet in the set (`modern-comments.pptx` anchors to a shape only); may we create them in
  PowerPoint, or should we search other third-party corpora?
- **Q6** Should Phase 1 resolve `otherStyle` for non-placeholder text (needs
  a PowerPoint check) or stop at `defaultTextStyle`?

## Sources

- Roadmap and evidence: `docs/roadmap/pptx.md`; scripts `spikes/pptx/`
  (`survey.py`, `placeholders.py`, `percent.py`, `runs.py`,
  `allowlist_check.py`, `readback.py`, `ppy_add_slide_check.py`)
- Fixtures: `reference/fixtures/pptx/README.md` (Apache POI, python-pptx,
  Open XML SDK, LibreOffice; licences in `reference/fixtures/pptx/LICENSES/`)
- Open XML SDK data `431ab05c`; ECMA-376 5th ed. Transitional XSDs (as
  ADR-0002)
- python-pptx 1.0.2 source: `shapes/placeholder.py`, `text/text.py`,
  `oxml/text.py`, `shapes/shapetree.py`, `parts/presentation.py`,
  `oxml/presentation.py`
- Apache POI trunk `47dcff76`: `XSLFSheet.getPlaceholder`,
  `XMLSlideShow.createSlide`, `XSLFComments`
- ECMA-376 Part 3 (Markup Compatibility) for AC branch selection
