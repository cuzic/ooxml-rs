# ADR-0011: Word Phase2: structure (new parts, new IDs, append-only registries)

- Status: **Proposed** (revision 1)
- Date: 2026-11-16
- Depends on:
  - [ADR-0001](0001-lossless-round-trip-architecture.md): Layer 3 and its append-only
    invariant;
  - [ADR-0002](0002-schema-driven-code-generation.md): slots, primitives, allowlist, and the
    post-acceptance notes (depth limit; detach/insert, PR #18);
  - [ADR-0004](0004-docx-phase0.md): Word Phase0 rev 4, PR #13;
  - [ADR-0007](0007-docx-phase1-template-filling.md): Word Phase1, PR #25, under review;
  - PR #14 (`Package::remove_part`, the interim registry-removal note in ADR-0001).
- Coordinates with: ADR-0006 (xlsx Phase2, PR #22), which defers part creation to xlsx
  Phase3 (ADR-0009, reserved). The xlsx roadmap's Phase3 scope asks for the same OPC services
  this ADR designs (§1).
- Evidence: `spikes/docx-phase2/`: `registries.py`, `needs.py` and a README with every
  number. Corpora: fixtures (26), Apache POI (129), Open XML SDK non-Strict (442), docx4j
  (12), template-engine test files (212). Library behaviour comes from source at pinned
  revisions (§Sources).

## Context

Phases 0 and 1 edit what exists. They create no part, no relationship, no content-type
entry and no ID (ADR-0004 §1, ADR-0007 §0). Phase2 is the first Word phase that **adds**
things: table rows and columns, lists, sections, inline images, hyperlinks, bookmarks, new
headers and footers, new styles. Each one needs a fresh value in some ID space, a new part, or
both:

| Feature | New IDs | New parts / registry entries |
|---|---|---|
| inline image | `wp:docPr/@id`; relationship id | media part (`word/media/imageN.ext`), relationship, content-type `Default` |
| hyperlink (external) | relationship id | relationship with `TargetMode="External"` |
| bookmark | annotation `w:id`, bookmark name | — |
| list | `numId`, `abstractNumId`, `nsid` | often a new `numbering.xml` + relationship + `Override` |
| header / footer | relationship id | `word/headerN.xml` + relationship + `Override` |
| style | `w:styleId` | (rarely) a new `styles.xml` |
| section break | — | — (references to existing header parts are copied) |
| table row / column / merge | — (Phase2 writes no `w14:paraId`) | — |

ADR-0001's Layer 3 invariant applies to all of them: **existing indices are append-only and
never renumbered or compacted.**

The corpus measurements set the hard requirements:

- **Real files do not keep the uniqueness the specification asks for, so a registry must
  never assume it.** Duplicate `w14:paraId` in 5 template outputs and 1 POI file; duplicate
  bookmark ids and names in template outputs; a bookmark and a comment sharing id `0` in 2
  files.
- **Producers use the whole value range.** `docPr` ids up to 1,073,741,825, and relationship
  ids of the form `R44c83fbb49ca402c` in 16 files.
- **A first list or image usually needs new package entries.** Only 21 of 101 writable POI
  files and 92 of 404 SDK files have a numbering part. Only 15 of 101 POI files have a `png`
  content-type `Default`.

## Decision

### 1. Core: the OPC part-adding API (Layer 3, `ooxml-core::Package`)

`ooxml-core` owns the format-agnostic registries: `[Content_Types].xml`, the relationships
parts and the part-name space (ADR-0001 Layer 3). PR #14 added a removal that edits them as
Layer 2 trees. Phase2 needs the additions. The API is designed for Word and for xlsx Phase3
alike: new worksheets, `sharedStrings.xml`/`styles.xml` where absent, and part names that do
not assume `sheetN.xml` (xlsx roadmap, Phase3).

#### 1.1 Operations

```rust
impl Package {
    /// A plan of additions. Nothing changes until `commit`; every check runs first.
    pub fn plan(&mut self) -> PartPlan<'_>;
}

impl PartPlan<'_> {
    /// Reserve a part name. `Exact` fails if taken; `Numbered` picks the smallest free
    /// `{dir}/{stem}{n}.{ext}`, n >= 1.
    pub fn add_part(&mut self, name: PartNameRequest, content_type: &str, body: PartBody) -> Result<PartName>;
    /// A relationship from `source` (None = the package) to an internal part or an external
    /// URI. Returns the allocated Id.
    pub fn add_relationship(&mut self, source: Option<&PartName>, rel_type: &str, target: Target) -> Result<RelId>;
    /// Validate everything, then apply. After a successful validation, `commit` cannot fail.
    pub fn commit(self) -> Result<Committed>;
}

pub enum PartNameRequest { Exact(String), Numbered { dir: String, stem: String, ext: String } }
pub enum PartBody { Bytes(Vec<u8>), Xml(XmlDocument) }      // Xml: becomes a Parsed, dirty part
pub enum Target { Internal(PartName), External(String) }
```

The format crates use the plan inside their own two-phase writes: check everything, then
mutate (ADR-0004 §3.3).

- A Word image insert plans the media part and its relationship, and the docx crate checks
  its own Layer 2 edits.
- Only then are `commit` and the tree edits run.

`commit` applies registry edits and new parts. The format crate applies its own tree edits,
and these cannot fail after their checks, as in ADR-0007 §7.

#### 1.2 Rules

- **Part names** follow OPC equivalence: ASCII case-insensitive and percent-decoded, as PR #14
  does. A name is free when no part, no `Override` and no relationship target (dangling ones
  included) uses it. `Numbered` takes the **smallest free n ≥ 1**: the corpora have
  non-contiguous and non-numbered names (`word/footer.xml`; header numbers up to 20), so
  "count + 1" is wrong. The xlsx needs are the same.
- **Relationship ids:** `rId{n}`. Here `n` is 1 + the largest numeric suffix among the
  source's ids that match `rId\d+` (ASCII case-insensitive), advanced past any id equal to
  `rId{n}` ignoring case. Ids of another shape, such as `R44c83fbb49ca402c` (16 files), are
  left alone and never collide with `rIdN`. Gaps are **not** reused. A gap may be a
  relationship that another tool removed while a stale `r:id` survived somewhere.
  Measured: 1,666/1,673 POI and 6,629/6,654 SDK ids have the `rIdN` shape; 0 duplicates.
  The largest id allocated is `rId321`.
- **Relationships part creation:** a source part without a `.rels` part gets one. That is
  itself a new part, with a `rels` `Default` added if missing; every corpus file has one.
- **Content types:**
  - XML parts always get an `Override`. 48 of 212 template files have no `xml` `Default`, and
    the `xml` `Default` is `application/xml` anyway.
  - Binary media get a `Default` for their lower-cased extension when the extension has none.
    That is how producers register media: `Default` in 106 SDK files, `Override` in 2.
  - If the extension already has a `Default` with a **different** type, the part gets an
    `Override` instead.
  - `Default` extensions are compared case-insensitively.
  - Nothing is ever removed or rewritten.
- **Registry edits** are Layer 2 edits of `[Content_Types].xml` and the relationships part:
  - new `Default`, `Override` and `Relationship` elements are appended as the last child;
  - registry parts that are not touched stay raw (T3).
  - A registry part that was replaced wholesale (`set_part`) or does not parse is refused,
    as `RegistryNotEditable` is in PR #14.
- **New parts are appended** to the zip after every existing entry: Deflated, no extra field,
  no directory entries. `[Content_Types].xml` keeps its position.
- **Targets** are written relative to the source part's directory (`media/image3.png` from
  `word/document.xml`), as Word writes them. The resolution reuses `opc::resolve_target`
  backwards, and a round-trip check is part of validation.
- **No implicit sharing.** `add_part` never deduplicates. Content-based reuse is a format
  decision (§5).

#### 1.3 What else core needs (additive notes to ADR-0002)

1. **Namespace declarations on created elements.** Word declares `a:` on `a:graphic` and
   `pic:` on `pic:pic`, not on the root. `a` is declared on the main part's root in 1 of
   101 POI and 1 of 404 SDK writable files. So inserting a picture needs
   `LensMut::declare_namespace(prefix, uri)` on a **created** node. It is refused on an
   original node, which would make its start tag dirty. It is also refused when the prefix is
   already bound to another URI in scope, or is `xml`/`xmlns`. ADR-0002 says "Phase0 never
   declares namespaces"; this lifts that for created nodes only. The prefix rules of §3 then
   find the new binding.
2. **`SameType` modulo required skips** (ADR-0007 §8.2) is reused for table rows (§7).

No core change is needed for relationship attributes in copies. PR #18's `detach_as` refuses
them (`RelationshipAttribute`), and the one Phase2 copy that needs them, a section break
(§6), skips `headerReference`/`footerReference` in the copy. It then writes them again with
typed `set_attr(r:id, …)` of ids that already exist in the same part's relationships.

#### 1.4 Layer 3 invariant, restated for additions

- Additions never change an existing entry. An allocated value is fresh in its whole scope:
  absent from every place §2 lists, existing duplicates included.
- Existing duplicates are never repaired.
- Removal is still limited to PR #14's interim note. Phase2's Word operations remove only
  content (rows, cells), never registry entries. Unreferenced relationships, media and
  numbering definitions stay, which is valid: 10 SDK `abstractNum` are unreferenced, and 13
  template files have relationships their source part never mentions.

### 2. Word registries (`ooxml-docx`), lazily activated

Every registry scans its scope when first needed, as ADR-0006 §2 does for xlsx, and
allocates from that scan plus what it has already handed out.

| Registry | Scope scanned | Allocation | Evidence |
|---|---|---|---|
| **Drawing ids** (`wp:docPr/@id`) | all story parts (document, headers, footers, notes, comments), **both** branches of every `mc:AlternateContent`, and text boxes. Not the glossary document, which is a document of its own | max + 1. If that would exceed 2³¹ − 1, the smallest free value ≥ 1 *(ECMA types it `unsignedInt`; Word's own limit is (u))*. `pic:cNvPr/@id` is written as `0`, as Word does | duplicates within or across parts: 0 in all corpora; AC-branch duplicates in 9 files (legitimate); largest max + 1 1,073,741,826 |
| **Annotation ids** (`w:id` of bookmarks, comment ranges and references, the comments part, revisions, `*PrChange`, `numberingChange`, move ranges, `customXml*Range`, permissions) | the **union** over every story part and `comments.xml` | max + 1 over the union, never a gap | cross-category collisions: 0 in POI/SDK, 2 template outputs. The union is the conservative reading (roadmap Q4). Largest max + 1: 137. Footnote/endnote ids are a separate space (Phase3; their separators are `-1`/`0`) |
| **Bookmark names** | every `bookmarkStart/@w:name` in every story | none: the caller names it. It must be absent ignoring case, at most 40 characters, a letter followed by letters, digits or `_` *(Word's rule, u)*. Names starting with `_` (hidden) are refused for callers | exact duplicates: POI 1 file (LibreOffice `__DdeLink__`), 8 template outputs; case-only duplicates: 0; over 40 characters: 1 |
| **Numbering** (`numId`, `abstractNumId`) | `numbering.xml` | max + 1 for each. `numId` 0 is reserved ("no numbering": 58 SDK `numPr` use it) and never allocated | duplicates, gaps, dangling references: 0 everywhere; largest max + 1: 100 / 104 |
| **`nsid`** (`abstractNum`'s hex id) | all `abstractNum/w:nsid` | random 8-hex-digit value absent from the set | duplicates: 0 |
| **Style ids** | `styles.xml` `w:styleId` (and `stylesWithEffects.xml`, read-only, §8) | caller's name, checked; see §8 | duplicate ids: 0 (ADR-0007 spike) |
| **Relationship ids, part names, content types** | core (§1) | §1.2 | §1.2 |

**No `w14:paraId`/`textId` in Phase2.** New paragraphs and rows get none, as in Phase0:
both are optional, and Word assigns them on its next save *(u)*. The registry that would
allocate them (random, < 0x80000000, absent from every part including `commentsExtended`,
measured fresh on the first draw in every file) is designed here but not activated. Comments
need it in Phase3. Copies never carry an existing `paraId`/`textId` (§7.1).

### 3. Inline images

`insert_image(run_position, image: &[u8], size: ImageSize, alt: &str) -> Result<DrawingId>`
inserts `w:r/w:drawing/wp:inline` at a run position (§9). The position is the end of a
paragraph or after a given run.

- **Formats:** PNG, JPEG, GIF. The type is sniffed from the magic bytes, never taken from a
  name. Pixel size and DPI come from the header (PNG `IHDR`/`pHYs`, JPEG SOF/JFIF,
  GIF). `ImageSize::Auto` uses 96 DPI when the file has no DPI *(Word's default, u)*;
  `ImageSize::Emu { cx, cy }` overrides it. Anything else is `UnsupportedImage`. SVG, which
  needs an `asvg:svgBlip` extension and a PNG fallback (1 SDK file), is Phase4.
- **Media part:** `word/media/image{n}.{ext}` with the smallest free n (§1.2), its relationship
  from the story part, and its content type. Reuse is in two layers, as in all three libraries
  (§13):
  - **package-wide**, a media part with the same bytes, matched by SHA-256 and then a full
    byte comparison, is reused;
  - **per story part**, an existing `…/image` relationship to it is reused (get-or-add);
    otherwise a new relationship is added.

  Producers share media parts (POI 2, SDK 6, template 8 files) and reuse one `rId` for
  several drawings (POI 4, SDK 7 files). Hashing reads only parts targeted by image
  relationships.
- **XML written:** the minimal form Word writes:
  - `wp:inline` with `distT/B/L/R="0"`;
  - `wp:extent`, `wp:effectExtent` (zeros);
  - `wp:docPr id descr`, `wp:cNvGraphicFramePr/a:graphicFrameLocks noChangeAspect="1"`;
  - `a:graphic` (declares `a`), with `a:graphicData uri=".../picture"`;
  - `pic:pic` (declares `pic`) with `pic:nvPicPr(cNvPr id="0" name, cNvPicPr)`,
    `pic:blipFill(a:blip r:embed, a:stretch/a:fillRect)`, and
    `pic:spPr(a:xfrm(off, ext), a:prstGeom prst="rect"/a:avLst)`.

  `wp` must be in scope on the story root; otherwise `NamespaceNotInScope`, which applies to
  POI 5 and SDK 6 writable files.
- **Where:** a new run, after the given run or at the end of the paragraph (`insert_by` in
  `w:p`'s content slot). The run gets no `rPr`. The paragraph must pass ADR-0007's paragraph
  checks, so no image is inserted inside a field result, a revision or a content control.
- **Gates:** W1 and W3. W2 per ADR-0007 §6 with a new **structure** column (§10).

### 4. Hyperlinks

- **External:** `w:hyperlink r:id` with a new relationship (`…/hyperlink`,
  `TargetMode="External"`) **per hyperlink**. Producers never share one (0 files), and Word
  writes one per link (POI 5, SDK 8 files have several relationships with the same URL). The
  URI must be absolute (`scheme:`), and the relationship stores it as given. It is never
  normalised.
- **Internal:** `w:hyperlink w:anchor="name"` to an existing bookmark (the name compared
  ignoring case), else `UnknownBookmark`.
- **Operations:**
  - `insert_hyperlink(position, text, target)` creates a new `w:hyperlink` holding one new
    run with `w:rStyle` "Hyperlink" when that character style exists (ADR-0007 §5), else no
    `rPr`.
  - `replace_with_hyperlink(match, text, target)` combines ADR-0007's `replace` with run
    splitting (§9). The matched text is removed; `r₀` is split at the match position (§9.2);
    the hyperlink is inserted between the two halves, its run carrying a copy of `r₀`'s
    `w:rPr` plus `rStyle`.
- No runs are ever moved into a wrapper. That would need a move primitive (Alternatives 3).

### 5. Bookmarks

- **Operation:** `add_bookmark(name, range)`. The range is a match (ADR-0007 §1) or whole
  paragraphs.
- **Allocation:** the id comes from the annotation registry and the name is checked (§2).
- **Placement:** `bookmarkStart` and `bookmarkEnd` are inserted as siblings of the runs, at
  run boundaries. A boundary inside a run splits the run first (§9.2). Both markers must be
  in one paragraph or in two paragraphs of the same story and container (body, cell or note);
  otherwise `BookmarkSpansContainers`.
- **Not created:** table-column bookmarks (`colFirst`/`colLast`).
- **Removal:** `remove_bookmark(name)` removes both markers. The id is never reused: the
  registry's max stays.

### 5a. Lists and numbering

- **`create_list(template: ListTemplate) -> NumId`** (`Bullet`, `Decimal`; Open question 8)
  makes the following additions.
  - **Numbering part:** created if absent, through the §1 plan. It is `word/numbering.xml`,
    or the smallest free numbered name if that is taken, with an `Override` of the
    WordprocessingML numbering type and a `…/numbering` relationship from the main part. 80
    of 101 writable POI files and 312 of 404 SDK files need this for their first list.
  - **`w:abstractNum`:** a new one with `abstractNumId` = max + 1, a fresh `nsid`,
    `multiLevelType="hybridMultilevel"` and the template's nine `w:lvl`. The generated slots
    of `CT_Numbering` place it after the existing `abstractNum` and before the first `num`.
    Producers keep that order: no violation in any corpus.
  - **`w:num`:** a new one, appended, with `numId` = max + 1 pointing at it.
- **`apply_list(p, num, level)`** writes `w:pPr/w:numPr` (`w:ilvl`, `w:numId`) with
  `get_or_insert`.
  - The `numId` must exist.
  - The paragraph must pass ADR-0007's paragraph checks. A `w:pPrChange` refuses
    (`InsideRevision`).
- **`restart_list(p)`** restarts numbering at a paragraph that already has a `numId`.
  - It appends a new `w:num` with the same `abstractNumId` and
    `w:lvlOverride w:ilvl=<p's level>/w:startOverride w:val=<start>`. Producers do this:
    `startOverride` POI 7, SDK 15, template 24.
  - It reassigns `numId` on `p` and on every later paragraph of the same story that uses the
    old `numId`, which is what Word's "Restart at 1" does *(u)*.
- **`remove_from_list(p)`** removes `w:numPr`. When the paragraph's style carries numbering,
  it writes `w:numId w:val="0"` instead. That is Word's form (58 SDK and 43 template `numPr`
  use it). `numId` 0 is never allocated (§2).
- **Untouched:** existing `abstractNum` with `styleLink`/`numStyleLink` (POI 28, SDK 18),
  `numPicBullet` and `numIdMacAtCleanup` (POI 3, SDK 13, template 2 files). New lists never
  link to styles.
- **Gate column:** structure (§10). Numbering changes `LISTNUM`, `AUTONUM*`, `REF \n`/`\r`/`\w`
  and outline-based `TOC` levels.

### 6. Sections

- **`insert_section_break(after: ParaRef, kind)`** (`nextPage`, `continuous`, `evenPage`,
  `oddPage`) splits the section S that contains the paragraph. The paragraph gets a copy of
  S's `w:sectPr` in its `w:pPr`, and S's own `w:sectPr` stays where it is. The copy is made by
  `detach_as` with `headerReference`/`footerReference` skipped. New references with the same
  `w:type` and `r:id` values are then inserted with typed setters, after checking each id in
  the part's relationships (§1.3).
  - The copy keeps S's header/footer references and page setup, so both halves look like S
    did.
  - Only `w:type` is set on the copy.
  - The copy is refused when S's `sectPr` has `w:sectPrChange` (`InsideRevision`) or when the
    paragraph already ends a section (`AlreadySectionEnd`).
  - The paragraph must be a body paragraph, not in a table or content control
    (`UnsupportedContext`).
- **Lifts ADR-0004's A2:** `append_paragraph` may now move the last section's `sectPr`.
  Moving is copy + remove of an element without annotation ids, by the same copy procedure.
- **Header inheritance:** sections without references inherit from the previous section
  (POI 88 of 123, SDK 480 of 613 `sectPr`). Because the break copies the references, the two
  halves keep their headers in every case. Inheritance is not consulted.

### 7. Tables

All table operations require a **consistent grid**: every row's `gridBefore` + Σ `gridSpan`
+ `gridAfter` equals the number of `gridCol`. This held for every row in every corpus, once
cells inside cell-level content controls are counted. Otherwise the operation fails with
`InconsistentGrid`. A table without `tblGrid` (1 POI file) is refused too.

Rows or tables holding cell-level `w:sdt`/`w:customXml` (SDK 56 files) are refused
(`UnsupportedContext`), as are row-level revisions (`trPr/ins|del`, SDK 3) and any target row
inside a revision.

#### 7.1 Insert rows

`insert_rows(table, at, count, template: RowTemplate)` builds each new row from a **template
row** (`Above` by default, or `Below`):

- `w:tr` keeps no attributes. In particular `w14:paraId`/`textId` and rsids are not copied
  (rows with `w14:paraId`: SDK 54).
- `w:trPr` and `w:tblPrEx` are copied (`detach_as`). The copy is refused if they contain
  `ins`/`del`/`trPrChange`.
- Per cell:
  - a `w:tc` with a copy of its `w:tcPr`, refused if it has `tcPrChange` or
    `cellIns`/`cellDel`/`cellMerge`;
  - one `w:p` with a copy of the template cell's first paragraph's `w:pPr`, minus `sectPr`,
    `pPrChange` and `numPr`, plus its mark run properties;
  - no runs.
- Text is then set with `set_cell_text` (ADR-0007 §3).
- **Vertical merges:** a new row's cell takes `vMerge="continue"` only when the template
  cell is in a merge **and** the row below the insertion point continues it at that grid
  column. Otherwise `vMerge` is dropped, which ends the merge cleanly.

#### 7.2 Delete rows

`delete_rows(table, range)` removes whole `w:tr` elements. It is refused when:

- a removed row holds an anchor whose partner lies outside the removed range, or a comment
  anchor or reference (`AnchorWouldBreak`);
- a removed row holds a field begin or end whose partner is outside (`InsideField`);
- the table would have no rows (`TableWouldBeEmpty`).

Vertical merges are repaired as follows. When a removed row holds a merge's `restart` and the
first kept row below continues it, that cell's `vMerge` becomes `restart`: one attribute
write.

#### 7.3 Insert and delete columns

`insert_column(table, at_grid_col, width: Twips)` makes the following changes.

- **Grid:** a new `w:gridCol` with that width is inserted into `tblGrid`.
- **Rows:** in every row, the grid column `at` either
  - falls at a cell boundary or at the row's end: a new cell is inserted there (copy of the
    left neighbour's `tcPr`, `tcW` set to the width), or
  - falls inside a cell's span (`gridSpan` > 1) or in `gridBefore`/`gridAfter`: that span is
    widened by one.
- **Widths:** `tblW` is not changed. When `tblW` is fixed (`w:type="dxa"`), Word reflows *(u)*.

`delete_column` removes the `gridCol`. In each row it removes the cell that covers only that
column, refused if the cell is not empty, or narrows the span that covers it.

#### 7.4 Merge cells

`merge_cells(table, rows, grid_cols)` merges a rectangle:

- horizontally, the first cell of each row gets `gridSpan` = the width, and the other cells
  of the rectangle are removed;
- vertically, the first row's cell gets `vMerge="restart"` and the cells below
  `vMerge="continue"`.

The rectangle's cells other than the top-left one must be **empty**: one paragraph, no runs
with text, no anchors. Otherwise `MergeNonEmpty`. Word would move their content into the
merged cell; ooxml-rs has no move primitive (Alternatives 3).

The rectangle's edges must not cut an existing merge (`MergeOverlaps`). `split_cell` is the
inverse for cells that `merge_cells` could have produced.

### 8. Styles

`add_style(StyleSpec { kind, name, based_on, next, run: RunFormat, paragraph: ParagraphFormat })`
appends a `w:style` to `styles.xml`:

- **Kinds:** paragraph or character. Table and numbering styles are Phase4.
- **Id:** the name with every character outside `[A-Za-z0-9]` removed *(Word's apparent rule,
  u; python-docx removes spaces only, §Sources)*. A collision (ignoring case) with an existing
  id gets the smallest numeric suffix that is free. The caller may also pass an explicit id,
  which is checked.
- **Name:** must be new ignoring case among `w:name`. A name equal to a **latent** built-in
  style (`Heading 3`, …, `lsdException`) is refused (`BuiltInStyleName`). Word would expect
  its built-in definition, which ooxml-rs cannot reproduce *(u)*. Measured: 19,173 POI and
  61,571 SDK latent-only entries.
- **References:** `based_on` and `next` must name existing styles of the same kind.
- **Formatting:** written with ADR-0007 §4's subset into `w:rPr`/`w:pPr` of the new style.
  `customStyle="1"` and `w:qFormat` are set.
- **`stylesWithEffects.xml`** (POI 18, SDK 66, template 27 files) is not updated. It is the
  Word 2010 copy for older readers, and leaving the new style out of it is what Word 2013+
  does when it saves *(u)*. Open question 5.
- **Missing styles part** (POI 4 files): created with `docDefaults` from a fixed minimal
  template. The template is not specified here; the owner question goes together with
  `Document::new()`.

### 9. Run splitting and paragraph insertion (the primitives the features share)

#### 9.1 New paragraphs

`insert_paragraph(after|before: ParaRef, text, ParagraphSpec)` inserts a `w:p` in the same
container: body, cell or note. Its `pPr` is either a copy of a neighbour's `w:pPr`, minus
`sectPr` and `pPrChange`, with `numPr` kept (so lists continue), or a style id. The text is
set as ADR-0007 §3.2 does, with mark properties copied.

ADR-0004's append is the special case "after the last body paragraph".

#### 9.2 Splitting a run

`split_run(run, at)` sets the run's text to its prefix with `set_run_text`. It then inserts
after it a new `w:r` with a copy of the run's `w:rPr` (`detach_as`, refused with
`rPrChange`) and the suffix.

- Rsid attributes are not copied.
- The run must pass R1–R4.
- Run indices after the split shift by one. The index API (ADR-0004) documents it, and
  `split_run` returns the new index.

### 10. Gates for structural operations

- **W1 and W3** apply to every operation.
- **W2** gets a **structure** column in ADR-0007 §6's per-operation table. Structural edits
  change paragraph counts, numbering, outline and pagination, so its additional allowed set
  is only `TC`, `XE`, `NOTEREF` *(conservative)*. Everything else needs the `updateFields`
  opt-in.
- Images, hyperlinks, bookmarks and styles are classified like this:

  | Operation | Column |
  |---|---|
  | `insert_image` | format: an image adds no paragraph and no text, but changes pagination |
  | `insert_hyperlink`, `add_bookmark` | text |
  | `add_style` | none (it changes no story), except W1/W3 |
  | everything else | structure |
- **A new bookmark changes no field result.** A `REF` to a name that does not exist yet
  shows an error text, and Word updates it only when fields update *(u)*. Bookmark names
  that a `REF`/`PAGEREF` instruction already mentions are refused (`NameReferencedByField`).

### 11. Allowlist and overrides

- **WML `[write]` additions:**
  - tables: `w:CT_Tbl/w:tbl`, `w:CT_Row/w:tr`, `w:CT_Tc/w:tc`, `w:CT_TblGrid/w:tblGrid`,
    `w:CT_TblGridCol/w:gridCol`, `w:CT_TcPr/w:tcPr`, `w:CT_VMerge/w:vMerge`,
    `w:CT_DecimalNumber/w:gridSpan`, `w:CT_TblWidth/w:tcW`;
  - hyperlinks and bookmarks: `w:CT_Hyperlink/w:hyperlink`,
    `w:CT_Bookmark/w:bookmarkStart`, `w:CT_MarkupRange/w:bookmarkEnd`;
  - sections: `w:CT_SectPr/w:sectPr`, `w:CT_SectType/w:type`;
  - lists: `w:CT_NumPr/w:numPr`, `w:CT_DecimalNumber/w:ilvl`, `w:CT_DecimalNumber/w:numId`,
    and, for `numbering.xml`, `w:CT_Numbering/w:numbering`, `w:CT_AbstractNum/w:abstractNum`,
    `w:CT_Num/w:num`, `w:CT_Lvl/w:lvl`, `w:CT_NumLvl/w:lvlOverride`,
    `w:CT_DecimalNumber/w:startOverride` and the `w:lvl` children used by the list templates;
  - styles: `w:CT_Styles/w:styles`, `w:CT_Style/w:style`, `w:CT_StyleName/w:name`,
    `w:CT_String253/w:basedOn`, `w:CT_String253/w:next`, `w:CT_RPrStyle/w:rPr`,
    `w:CT_PPrStyle/w:pPr`;
  - images: `w:CT_Drawing/w:drawing`.
- **`[detach]`** gains `w:CT_TrPr/w:trPr`, `w:CT_TcPr/w:tcPr`, `w:CT_PPr/w:pPr` and
  `w:CT_SectPr/w:sectPr` (the last always copied with its header/footer references skipped,
  §6). Each review checks that the
  subtree holds no document-unique id once the listed skips apply.
- **New groups `wp` and `pic`:** `wp:CT_Inline/wp:inline`, `wp:CT_PositiveSize2D/wp:extent`,
  `wp:CT_EffectExtent/wp:effectExtent`, `wp:CT_NonVisualDrawingProps/wp:docPr`,
  `wp:CT_NonVisualGraphicFrameProperties/wp:cNvGraphicFramePr`, and `pic:CT_Picture/pic:pic`
  with its children. These sit in the allowlist file of the group the SDK assigns them to.
- **Shared DrawingML `a:` entries** (`a:graphic`, `a:graphicData`, `a:blip`, `a:stretch`,
  `a:xfrm`, `a:prstGeom`, `a:graphicFrameLocks`) are shared with PowerPoint's `dml.toml`, and
  ADR-0005's owner must review them.
- **Overrides:** none new are expected. The exact SDK `Name`s and the dry run through the
  generator are the implementation PR's first commit, as ADR-0007 §8 did. They are **not**
  verified in this revision (Open question 7).

### 12. Test oracles

- **T1–T6** for every changed part:
  - T2: changes in a story part lie at the insertion points and inside the spans of edited
    elements; `[Content_Types].xml` and `.rels` changes are appended elements only.
  - T3: every part not named by the operation is byte-identical. New parts are additional
    entries at the end of the zip.
- **T-OPC (shared with xlsx Phase3), on every output:**
  - every relationship's internal target exists;
  - every part has exactly one content type, by `Override` or by `Default`, and every
    `Override` names an existing part;
  - relationship ids are unique per source, ignoring case;
  - no new part is an orphan;
  - every `r:id`/`r:embed` written resolves in its part's relationships;
  - the zip has no duplicate names (OPC equivalence).
- **T-REG (registries):**
  - every allocated value is absent from its whole scope (§2) before the write;
  - after the write it occurs exactly once, AC branches included;
  - every existing value is unchanged: no id moved, no duplicate repaired;
  - property tests over the corpora run 100 random operations per writable fixture and
    re-check the scopes after each.
- **T5 (docx-rs read-back):**
  - tables: rows, cells, `gridSpan`, `vMerge`;
  - numbering: `abstractNums`, `numberings`, a paragraph's `numberingProperty`;
  - images: `drawing` with the relationship id and the EMU size written;
  - hyperlinks: anchor or relationship target;
  - bookmarks: id and name;
  - sections: `sectionProperty`.

  docx-rs reads all of these (spike: JSON of `Complex01.docx`).
- **T7 (independent OPC check):** a second implementation of T-OPC over `roxmltree` in the
  tests, so core's registry code is not its own oracle.
- **`ooxmlsdk` validation** (roadmap Phase2, ADR-0002 Alternatives 1): evaluated when the
  implementation starts; if the compile cost is acceptable, every output must parse.
- **Manual:** `manual_check` outputs in Word for each operation:
  - an image in a header;
  - a list restart;
  - a section break in a landscape document;
  - merges;
  - a new header on section 2 of 3;
  - a style named like a localized built-in.

### 13. How the libraries do it (source-verified; §Sources for pins)

| Concern | python-docx 1.2.0 | Apache POI XWPF | docx-rs 0.4.22 | ADR-0011 |
|---|---|---|---|---|
| rId | first gap (`opc/rel.py:104-111`); reuses a rel with the same type and target (`:31-47`, `:70-87`) | count+1, then a forward cursor with a collision check (`PackageRelationshipCollection.java:209-229`) | fixed or per-type strings (`rId1`…`rId8`, `rIdHeader{n}`, `rIdImage{n}`); customXml `rId{i+8}` can collide with footnotes' `rId8` (`document_rels.rs:56-117`) | max numeric + 1, never a gap (§1.2) |
| part names | first free number among all parts (`opc/package.py:109-121`); images by first gap regardless of extension (`package.py:95-110`) | `getNextPartNumber` probes names (`POIXMLDocumentPart.java:614-647`), but headers use the rel count + 1 and can hit an existing name (`XWPFHeaderFooterPolicy.java:185, 237`) | order counters; media always `.png` (`zipper/mod.rs:80-103`, `image_collector.rs:137`) | smallest free n under OPC equivalence, also avoiding dangling Overrides and targets |
| content types | rebuilt from a whitelist on save; the original is not kept (`opc/pkgwriter.py:80-99`) | Default for a new (extension, type) pair, else Override (`ContentTypeManager.java:152-164`) | fixed Default list incl. non-standard `image/jpg` (`content_types.rs:156-172`) | POI's rule, appended to the existing part |
| image dedup | package-wide SHA-1; rel per part (`package.py:68-93`, `parts/story.py:27-39`) | checksum + byte compare; rel per part (`XWPFDocument.java:1596-1680`) | (length, crc32) + bytes, package-global; rel per part (`image_collector.rs:15-70`) | SHA-256 + bytes, package-wide; rel get-or-add per part |
| `docPr` id | max + 1 of `//@id` in **one part** (`parts/story.py:76-88`): a header and the body can collide | lowest free id document-wide (`IdentifierManager.java:120-129`); an existing duplicate is silently re-reserved elsewhere (`:58-114`) | always `1` (`drawing.rs:132`) | max + 1 over all stories and AC branches |
| `pic:cNvPr` id | 0 (`oxml/shape.py:147-151`) | 0 (`XWPFRun.java:1416-1440`) | 0 (`pic.rs:224`) | 0 (as Word: SDK 265 of 285) |
| bookmarks | no API | no API | caller's id and name, unchecked (`bookmark_start.rs:14-19`) | annotation union max + 1; name checked |
| comment ids (Phase3) | max + 1 over comments, first gap above 2³¹ − 1 (`oxml/comments.py:68-87`) | caller's (`XWPFComments.java:286-292`) | caller's | union registry (§2) |
| numbering | no list API; numbering.xml cannot be created (`parts/numbering.py:11-14`) | `addNum`: count + 1, **no collision check** (`XWPFNumbering.java:145-153`); `abstractNum` max + 1; creates the part (`XWPFDocument.java:1017-1030`) | caller's ids; always emits a built-in num 1 that a caller's 1 duplicates (`numberings.rs:32-46`) | max + 1 each, `numId` 0 reserved, `nsid` fresh; creates the part through §1 |
| tables | `add_row`/`add_column` keep `tblGrid` (`table.py:37-55`); `merge` moves content into the top cell (`oxml/table.py:661-735`) | `createRow` copies row 0's cell count; `addNewCol` ignores `tblGrid`; new tables have none (`XWPFTable.java:233-307`) | caller sets everything; no checks (`table.rs:36-69`) | consistent-grid precondition; merges only of empty cells |
| hyperlinks | read-only; the generic `relate_to` would share a rel per URL | new external rel each time (`XWPFParagraph.java:1542-1558`) | new global id each time (`hyperlink.rs:34-50`) | one rel per hyperlink (producers never share) |
| headers | `next_partname` + `relate_to` + schema-ordered reference (`section.py:316-325, 446-450`) | rel-count naming, body-level `sectPr` only | count + 1 | §1 names, references copied on breaks |
| section break | clones the sentinel `sectPr` into a new last paragraph, strips the sentinel's references (`oxml/section.py:186-195`) | none | none | copy without references, then the same references rewritten (§6) |
| styles | styleId = name without spaces (special cases for headings, caption), collisions unchecked (`oxml/styles.py:16-30, 280-290`) | appended unchecked (`XWPFStyles.java:201-206`) | caller's | id derived and checked, latent built-in names refused |
| `w14:paraId` | none | none | thread-local counter `00000001…`, duplicates reassigned before writing (`paragraph_id.rs`, `mod.rs:963-981`) | none in Phase2 (Alternatives 4) |
| `stylesWithEffects.xml` | carried unchanged from its template | not referenced | not referenced | left unchanged (Open question 5) |

Every library either allocates from a partial scope (python-docx's `docPr`, POI's header
names and `numId`) or does not allocate at all (docx-rs). None of them checks that a value
it hands out is absent everywhere it must be. That check is what T-REG tests.

## Alternatives considered

1. **Count + 1 for part names and `rId`s** (POI's header naming and `addNum`, docx-rs; §13). Wrong here:
   - `rIdN` sets that are not `1..n` occur in 4 files and other id shapes in 16;
   - header numbers go up to 20 with gaps, and `word/footer.xml` exists.

   Max + 1 and smallest-free-name are as cheap and never collide.
2. **Reuse gaps** (first free `rId`, first free `numId`). Rejected for ids: a gap may be a
   removed entry whose stale reference survives (ADR-0001's reason for append-only).
   Accepted for part *names*, where §1.2 also checks dangling `Override`s and targets.
3. **A move primitive** (cut and paste a subtree within a part, ids kept because nothing is
   duplicated). It would allow merging non-empty cells and wrapping existing runs into a
   hyperlink. Deferred. It needs a core design of its own: spans, dirty flags, and the depth
   limit at the destination.
4. **Generate `w14:paraId`/`textId` for new paragraphs and rows.** Not needed. Both are
   optional, Word adds them, and the corpora show the placeholder `textId` `77777777` on most
   paragraphs (SDK 3,070 of 3,255). Deferred to Phase3, where comments need `paraId`.
5. **Deduplicate external hyperlink relationships.** Producers never share them. A shared
   relationship would make "edit this link's target" change other links. Rejected.
6. **Repair existing duplicates** (bookmark names, `paraId`). Rejected by Layer 3. The
   scans tolerate them, and allocation avoids every existing value.
7. **Write `stylesWithEffects.xml` too.** Deferred: unverified effect *(u)*; Open question 5.
8. **Core owns all registries, including Word's.** Rejected, as in ADR-0001: core owns only
   the OPC registries; ID spaces with format meaning belong to the format crate.

## Consequences

- **Positive:**
  - Structure can be added to real Word files without renumbering or repairing anything.
  - Every new id is fresh in a scope that is measured to be the right one: no `docPr`
    collision across parts in any corpus, so the union scan is sufficient.
  - The OPC API serves xlsx Phase3 unchanged.
- **Positive:** T-OPC and T-REG apply to both formats and are independent of the code they
  check (T7).
- **Negative:** three additive core changes:
  - the part plan;
  - namespace declarations on created nodes;
  - `SameType` modulo skips (shared with ADR-0007).

  Each needs its own review.
- **Negative:** these are refused in Phase2 and wait for a move primitive:
  - merging non-empty cells;
  - wrapping existing text into a hyperlink without splitting into new runs;
  - deleting rows that hold half of a bookmark.
- **Negative:** the allowlist grows by roughly 60 entries across `wml`, `wp`, `pic` and `a`.
  The DrawingML part is shared with PowerPoint and needs that owner's review.

## Open questions for the project owner

1. **Core part-adding API (§1).** Is the `PartPlan` shape right for xlsx Phase3 as well? It
   should be reviewed together with ADR-0009 before either is implemented.
2. **docPr ids above 2³¹.** Is "smallest free" an acceptable fallback, or should it be
   refused? Word's handling of ids ≥ 2³¹ is unverified.
3. **Bookmark name rules** (≤ 40 characters, letter first, no `_` prefix for callers): please
   confirm against Word.
4. **Style ids from names** (§8): strip everything outside ASCII alphanumerics, or keep
   Unicode letters? Localized Word keeps them (`Tlotextu` → `Tělo textu` *(u)*).
5. **`stylesWithEffects.xml`:** leave new styles out (proposed), or append them there too?
6. **Header on a later section** (§6): copying references preserves both halves' headers.
   A `set_header(section, …)` that starts a new header for one section would also set
   explicit references on the following inheriting sections. Should that be in Phase2?
7. **Allowlist dry run** for the ~60 new entries, and whether the `wp`/`pic` groups go into
   `dml.toml` or a new `wpd.toml`.
8. **List templates:** which built-in bullet and decimal definitions should `create_list`
   write? Copy Word 365's `numbering.xml` defaults from an owner-provided document (preferred)
   or define our own.

## Sources

- Spike: `spikes/docx-phase2/README.md`, `registries.py`, `needs.py`; run 2026-11-16,
  Python 3.15.
- Library source (read, not executed):
  - python-docx `e45454602b53e8e572b179ccf1c91093ec9f4ed7` (`src/docx/`);
  - Apache POI `47dcff769bb395034dddfd59c4609659b6dbc6a4`
    (`poi-ooxml/src/main/java/org/apache/poi/`: `openxml4j/opc/PackageRelationshipCollection.java`,
    `openxml4j/opc/PackagePart.java`, `openxml4j/opc/internal/ContentTypeManager.java`,
    `ooxml/POIXMLDocumentPart.java`, `ooxml/util/IdentifierManager.java`,
    `xwpf/usermodel/{XWPFDocument,XWPFRun,XWPFNumbering,XWPFTable,XWPFParagraph,XWPFStyles,XWPFComments,XWPFRelation}.java`,
    `xwpf/model/XWPFHeaderFooterPolicy.java`);
  - docx-rs 0.4.22 crate source (`src/documents/…`, `src/zipper/mod.rs`).

  Line ranges are given in §13.
- ADR-0001 (Layer 3), ADR-0002 (§3 prefix rules, §4 slots, post-acceptance notes), ADR-0004,
  ADR-0006 (xlsx Phase2; part creation deferred to Phase3), ADR-0007 (PR #25), PR #14
  (`remove_part`), PR #18 (`detach_as`/`insert_detached`), `docs/roadmap/docx.md` §5
  (registries), `docs/roadmap/xlsx.md` Phase3.
- ECMA-376 Part 1 (`CT_NonVisualDrawingProps/@id`, `ST_DrawingElementId`; bookmarks
  §17.13.6; numbering §17.9). Section numbers are from memory and marked *(u)* where a rule
  depends on them. ECMA-376 Part 2 (OPC: part-name equivalence, content types,
  relationships).
