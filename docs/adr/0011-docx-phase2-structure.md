# ADR-0011: Word Phase2: structure (new parts, new IDs, append-only registries)

- Status: **Accepted** (revision 4; review round 3: "Accepted after Minor only: yes")
- Date: 2026-11-16
- Depends on:
  - [ADR-0001](0001-lossless-round-trip-architecture.md): Layer 3 and its append-only
    invariant;
  - [ADR-0002](0002-schema-driven-code-generation.md): slots, primitives, allowlist, and the
    post-acceptance notes (depth limit; detach/insert, PR #18, merged; `insert_at`, PR #26,
    merged);
  - [ADR-0004](0004-docx-phase0.md): Word Phase0, Accepted (PR #13 merged);
  - [ADR-0007](0007-docx-phase1-template-filling.md): Word Phase1, **Accepted** (revision 3,
    PR #25 merged);
  - PR #14 (`Package::remove_part`, the interim registry-removal note in ADR-0001; merged);
  - **PR #45** (`core/insert-with-ns`): `NsDecl` and the `*_with_ns` inserts with check twins
    (§1, "What else core needs" 1);
  - **the core registry API on `core/add-part`** (`RegistryPlan` / `Package::apply` and the
    one-step operations with `check_*` twins; **PR #38**). By the coordinator's decision of
    2026-11-16 (review B1) it is the **single source of truth** for:
    - part naming;
    - relationship ids;
    - `Default` vs `Override`;
    - dangling targets.

    This ADR states only what Word needs from it (§1).
- Coordinates with: ADR-0006 (xlsx Phase2, Accepted, PR #22 merged), ADR-0009 (xlsx Phase3,
  PR #27) and ADR-0012 (pptx Phase2, Accepted revision 3, PR #30 merged), which use the same
  core API.
- Evidence: `spikes/docx-phase2/`: `registries.py`, `needs.py` and a README with every
  number. Corpora: fixtures (26), Apache POI (129), Open XML SDK non-Strict (442), docx4j
  (12), template-engine test files (212). Library behaviour comes from source at pinned
  revisions (§Sources). Revision 2 adds `review1.py` for the facts behind the review findings.

## Revision history

- **Revision 1:** initial draft.
- **Revision 2** (after `0011-opus-review-round1.md`: B1, M1–M6, m1–m12; branch merged with
  `origin/main` per m12):
  - **B1:** revision 1's `PartPlan` and its rules (§1.1–§1.2) are withdrawn. The core API on
    `core/add-part` is the single source of truth. §1 now lists only Word's requirements on it,
    and Word's media are registered by core's rule (an existing matching `Default`, else an
    `Override`). Word never adds a `Default`, as in ADR-0009.
  - **M1:** a section break writes `w:type` on the **later** `sectPr`, as python-docx does.
    Restart settings (`pgNumType/@start`, line and note numbering restarts) stay only on the
    earlier half (§6).
  - **M2:** `a` is declared locally on `a:graphicFrameLocks` too, as Word does. One general
    rule: declare on each outermost created element that uses a namespace not in scope (§3).
  - **M3:** every registry's scope is definitions **and references**, so a new value never
    adopts a dangling reference: the 22 undefined `pStyle` in POI `60329.docx`, for example
    (§2).
  - **M4:** a style id that would be empty after ASCII stripping becomes `a` / `a{n}`, as
    localized Word writes (§8).
  - **M5:** `insert_column` does not copy `gridSpan`/`vMerge`/`hMerge`; column 0 copies from
    the right; legacy `hMerge` tables are refused (§7.3).
  - **M6:** deleting rows or columns and merging refuse when the range holds a
    footnote/endnote reference or a revision element. `insert_rows` drops revision elements
    from copied mark properties (§7).
  - **m1–m11:** see the sections; the m-numbers are noted in place.
  - The reviewer's answers to Q1–Q8 are recorded as **proposed defaults** pending the owner
    (§Open questions).

- **Revision 3** (after `0011-opus-review-round2.md`: 1 Major, 8 Minor; `origin/main` merged
  again):
  - **N1:** namespace declarations become a core **insert-with-declaration** primitive with
    a check twin. Revision 2's `declare_namespace` on an already created node could not work,
    because core picks a new element's prefix when it creates it and refuses an unbound one.
    The exact requirement is in §1. The core PR is assigned separately.
  - **N2:** the order is dry run, then plan, then apply, then tree edits. That avoids
    `StalePlan`.
  - **N3:** T-OPC is judged against the input.
  - **N4:** ooxml-docx refuses an allocated `rId` that equals an `r:*` value in the source
    part. The case-sensitivity note was stale and is removed.
  - **N5:** PR #38 is cited.
  - **N6:** wording.
  - **N7:** `w:CT_NumLvl/w:lvlOverride` is added to `[detach]`.
  - **N8:** the out-of-scope manual check is removed.
  - **N9:** `titlePg` and note-numbering restarts are documented, and `NOTEREF` is removed
    from the structure column.

- **Revision 4** (after `0011-opus-review-round3.md`: Minor only, "Accepted after Minor only:
  yes"; `origin/main` merged again). Status **Accepted**.
  - **R3-1:** §1 follows PR #45: a URI with **any** prefixed binding in scope is reused and
    nothing is declared; `insert_by_with_ns` / `insert_with_ns` / `get_or_insert_with_ns`
    suffice, so no positional variant is required; the check twins return the declarations
    they would write (PR #45 is being changed to do so), used by tests only. PR #45 is cited
    in Depends on, Consequences and Sources.
  - **R3-2:** the dry run checks the outer `w:r` (or `w:hyperlink`) and a depth bound
    (paragraph depth + subtree height ≤ `max_depth`; 9 for an image). Lazy registry scans
    happen in step 1, and nothing calls `xml_part_mut` between the plan and `apply`.
  - **R3-3:** `w:hyperlink` is inserted with `NsDecl(r)` (§4); a POI file without `r` on the
    root joins the tests.
  - **R3-4:** status lines updated (ADR-0007 and ADR-0012 Accepted; #14, #18, #22, #25, #26, #30
    merged). The enforced-protection opt-in from ADR-0007 Q10 is placed out of Phase2 with a
    destination (Q9).

## Context

Phases 0 and 1 edit what exists. They create no part, no relationship, no content-type
entry and no ID (ADR-0004 §1, ADR-0007 §0). Phase2 is the first Word phase that **adds**
things: table rows and columns, lists, sections, inline images, hyperlinks, bookmarks, new
headers and footers, new styles. Each one needs a fresh value in some ID space, a new part, or
both:

| Feature | New IDs | New parts / registry entries |
|---|---|---|
| inline image | `wp:docPr/@id`; relationship id | media part (`word/media/imageN.ext`), relationship, content type (core's rule, §1) |
| hyperlink (external) | relationship id | relationship with `TargetMode="External"` |
| bookmark | annotation `w:id`, bookmark name | — |
| list | `numId`, `abstractNumId`, `nsid` | often a new `numbering.xml` + relationship + `Override` |
| header / footer | relationship id | **not in Phase2** (proposed default Q6); section breaks reuse existing parts |
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

**Out of Phase2 (revision 2, review m8):**

- creating header or footer parts and setting a header for one section (Q6);
- creating whole tables, as opposed to editing existing ones;
- creating a styles part;
- **the enforced-protection opt-in** handed over by ADR-0007 (Alternatives 10, Q10; revision 4,
  review R3-4). ADR-0004's W3 (`DocumentProtected`) stays a refusal in Phase2. An opt-in that
  edits a document whose `w:documentProtection` is enforced is a policy decision (whose
  restriction is being bypassed, and whether `settings.xml` is touched), not a structure
  edit. It goes to the later document-settings ADR together with `Document::new()`'s
  settings; proposed default Q9. ADR-0007's other hand-over, lifting A2, **is** in Phase2
  (§6).

The roadmap's earlier Phase2 list named them; they move to a later structure ADR.

## Decision

### 1. Core registries: what Word needs from the `core/add-part` API

`ooxml-core` owns the format-agnostic registries: `[Content_Types].xml`, the relationships
parts and the part-name space (ADR-0001 Layer 3).

**Revision 1 is withdrawn (review B1).** Revision 1 proposed its own `PartPlan` API with its
own rules (smallest free name, implicit `Default`s). Those conflicted with the API being built
on `core/add-part` and with ADR-0009 and ADR-0012. The core API is now the single source of
truth for:

- naming parts;
- allocating relationship ids;
- choosing between `Default` and `Override`;
- treating dangling targets.

This ADR does not restate those rules, and links to the core PR once it is up. The API, as of
the branch:

- `RegistryPlan::new(&Package)` collects steps, each validated completely:
  - `add_part(name: PartNameRequest, content_type, content)`;
  - `add_relationship(source, type, Target)`;
  - `add_default_content_type`;
  - `set_content_type`.
- `Package::apply(plan)` cannot fail, except with `StalePlan`. One-step conveniences have
  `check_*` twins.
- `PartNameRequest::Numbered("dir/stem{}.ext")` is **one more than the largest number in
  use**: in parts, `Override`s, relationship targets (dangling ones included), names removed
  in the session, and names planned earlier. Gaps are never filled.
- `add_part` registers the content type with nothing when an existing `Default` for the
  extension has exactly that type, and with an `Override` otherwise.

**What Word Phase2 requires of it:**

1. **At most one new part per operation:**
   - the media part of `insert_image` (§3);
   - `numbering.xml` for the first list (§5a).

   **Order (revision 3, review N2):**
   1. the Layer 2 dry run on the story parts (ADR-0007 §7; for Phase2 inserts, the outer
      check and the depth bound of item "What else core needs" 1). It may call
      `Package::xml_part_mut`, which advances the package's generation. **Every lazy scan
      happens here too**: the §2 registries' scans of other stories (`docPr` ids in headers,
      footers, notes) and item 4's scan of `r:*` values (revision 4, review R3-2);
   2. the `RegistryPlan` for the new part and relationships. It records the package's id and
      generation; the relationship id and the part name are decided here;
   3. `Package::apply` (cannot fail except `StalePlan`). **No `xml_part_mut` is called
      between 2 and 3**: anything read there goes through `parsed_part` (`&self`) or
      `xml_part`, which do not advance the generation (PR #38, `package.rs`);
   4. the tree edits, which write the decided `r:id` and cannot fail after step 1.

   Building the plan **before** the dry run would make `apply` refuse with `StalePlan`.
   ADR-0012's alternative stages a cloned story tree in the same plan (`stage_part`). It is
   not used here, because it would clone a possibly large `document.xml` for every image or
   list; step 4 is already proven not to fail.
2. **Media content types:** `add_part` with the sniffed type (`image/png`, `image/jpeg`,
   `image/gif`). An existing matching `Default` is used, otherwise core writes an `Override`.
   Word Phase2 **never adds a `Default`**, consistent with ADR-0009. Word reads media
   registered by `Override`: 2 SDK and 13 template files do so. Proposed default Q1.
3. **Names:** `Exact("word/numbering.xml")`, falling back to
   `Numbered("word/numbering{}.xml")` if the name is in use. Media use
   `Numbered("word/media/image{}.<ext>")`. The extension is that of an existing `Default`
   for the type when there is one (`jpg` vs `jpeg`, review m5), else the canonical one.
4. **Relationship ids:** core's rule. Two inputs to the core rules table, from Word's evidence:
   - ids of another shape (`R44c83fbb49ca402c`, 16 files) occur and must never collide;
   - **ooxml-docx requires** (revision 3, review N4) that the id core allocates does not equal,
     ignoring ASCII case, any value of an `r:`-namespace attribute in the source part. This
     is the referenced-but-undefined case of M3. If it does, the operation is refused with
     `Err(RelIdReferenced)` before anything changes, and T-REG checks it. Measured: 0 such
     dangling `r:*` among 2,805 attributes, so the refusal is a safety net, not a code path
     the corpora reach. Core's `next_relationship_id` already compares ids ignoring ASCII
     case (PR #38), so revision 2's case-sensitivity remark is withdrawn.
5. **Dangling targets** are real: 26 internal targets in 19 SDK files resolve to no part
   (`review1.py`). Core already avoids their names.

**What else core needs** (additive notes to ADR-0002, separate from `core/add-part`):

1. **Insert with namespace declaration (revision 3, review N1; core PR assigned separately).**
   Core decides a new element's prefix **when it creates it** (`choose_element_prefix`) and
   refuses with `NamespaceNotInScope` when no binding is in scope. So revision 2's
   `declare_namespace` on an already created node could never be reached for `a:graphic`,
   `pic:pic` or `a:graphicFrameLocks`. Their element names are in namespaces that the document
   root does not declare:
   - `a` is undeclared on the root of POI 112/113, SDK 440/442 and template 208/210 main
     parts;
   - `pic` on POI 109/113, SDK 440/442 and template 199/210;
   - `wp` on POI 6/113 and SDK 6/442.

   (Reviewer's `probe2-ns.out`.) **PR #45** (`core/insert-with-ns`) provides this
   (revision 4, review R3-1). This ADR relies on it as follows:
   - **`NsDecl { prefix, uri }`** and the `_with_ns` inserts. ooxml-docx uses
     **`insert_by_with_ns`, `insert_with_ns` and `get_or_insert_with_ns`**. Every Phase2
     insert is either `insert_by` on an existing node (the outer `w:r`, §3; `w:hyperlink`,
     §4, placed before the second half of the split by a `before` predicate) or an insert
     under a node created in the same call. So a positional `insert_at` variant is **not
     needed**. If one ever is, it is PR #26's `insert_at` gaining a `_with_ns` twin through
     the same plan, which PR #45's ADR-0002 note already lists as an extension point.
   - **Prefix choice (PR #45's rule).** If the URI already has **any** prefixed binding in
     scope, that binding is used and nothing is declared. Otherwise the new element gets
     `xmlns:<p>`: the requested prefix if it is unbound in scope, else the first free
     `<p>1`, `<p>2`, …. Existing bindings are never shadowed. This writes no redundant
     declaration, which is better than revision 3's "same prefix, same URI" rule.
   - **Check twins** (`check_insert_by_with_ns`, …) run the same planning function as the
     insert, so they cannot disagree. PR #45 is being changed so that they **return the
     declarations the insert would write**. ooxml-docx uses that only in tests (T2 expects
     exactly those bytes). Attribute prefixes are chosen by `choose_attr_prefix` and are not
     needed in advance. Revision 3's "returns the prefixes actually used" requirement is
     withdrawn.
   - Descendants created under the new element find the binding through the existing
     `choose_element_prefix` / `choose_attr_prefix`. Depth and dirty rules are those of
     `insert_by`. The element is a created node (`own_dirty`), and no original node's start
     tag changes.

   **Where ooxml-docx declares** (§3, §4): on **each outermost created element** whose own
   name, or an attribute name in its created subtree, is in a namespace that has no prefixed
   binding at its insertion point. This is Word's shape **when that namespace is not
   already in scope**; when it is, the existing prefix is used (PR #45). For `insert_image`:
   - `wp:inline` declares `wp` (when the root lacks it) and `r` (when the root lacks it;
     `r:embed` sits below it);
   - `a:graphicFrameLocks` declares `a`;
   - `a:graphic` declares `a`;
   - `pic:pic` declares `pic`.
   - `a:blip`, `a:stretch`, `a:xfrm`, … are created under `a:graphic`, inside the binding.

   For `insert_hyperlink` (§4), `w:hyperlink` is inserted with `NsDecl(r)`: `r:id` needs a
   non-empty prefix bound to the relationships namespace (review R3-3).

   **Dry run (revision 4, review R3-2).** A check twin is called on an existing node, so
   before anything changes only the **outermost** insert can be checked:
   - `check_insert_by::<R>` (or `check_insert_by_with_ns`) on the target paragraph for the
     outer `w:r`, and on the paragraph for `w:hyperlink`;
   - a **depth bound**: the paragraph's depth + the height of the created subtree ≤
     `max_depth`. The height is 9 for an image
     (`w:r/w:drawing/wp:inline/a:graphic/a:graphicData/pic:pic/pic:spPr/a:prstGeom/a:avLst`)
     and, for a hyperlink, 2 (`w:hyperlink/w:r`) plus the height of the `w:rPr` it carries
     (a copy of `r₀`'s, §4; computed from the source node before anything changes).

   The inner `*_with_ns` inserts use constant `NsDecl`s, so `InvalidNamespaceDeclaration`
   cannot occur, and with the depth bound checked they fail only with `NotAllowed`, which
   ADR-0007 §7 already excludes for nodes created in the same call.

   ADR-0013 (pptx) needs the same primitive for `a16:rowId`; ADR-0009 and ADR-0012 create
   new parts as whole byte strings.
2. **`SameType` modulo required skips** (ADR-0007 §8.2), reused for table rows (§7).

No core change is needed for relationship attributes in copies. PR #18's `detach_as` refuses
them (`RelationshipAttribute`). The one Phase2 copy that needs them is a section break (§6):
it skips `headerReference`/`footerReference` in the copy, then writes them again with typed
`set_attr(r:id, …)` of ids that already exist in the same part's relationships.

#### 1.1 Layer 3 invariant, restated for additions

- Additions never change an existing entry.
- An allocated value is fresh in its whole scope: absent from every **definition and every
  reference** §2 lists, existing duplicates included.
- Existing duplicates and dangling references are never repaired.
- Removal is limited to the core rules (PR #14's interim note). Phase2's Word operations
  remove only content (rows, cells), never registry entries. Unreferenced relationships,
  media and numbering definitions stay, which is valid: 10 SDK `abstractNum` are
  unreferenced, and 13 template files have relationships their source part never mentions.

### 2. Word registries (`ooxml-docx`), lazily activated

Every registry scans its scope when first needed, as ADR-0006 §2 does for xlsx, and
allocates from that scan plus what it has already handed out.

**Scopes are definitions and references (revision 2, review M3).** A value counts as used
when anything **defines** it or **refers** to it. Otherwise a new definition could adopt a
dangling reference and silently change existing content. For example, POI `60329.docx` has
22 `w:pStyle` naming the undefined style `EmptyCellLayoutStyle`, and `add_style` with that
name would restyle 22 paragraphs.

| Registry | Scope scanned: definitions ∪ references | Allocation | Evidence |
|---|---|---|---|
| **Drawing ids** | every `@id` of `wp:docPr` **and of every DrawingML `*:cNvPr`** (`pic:`, `wps:`, `a:`; review m1) in all story parts, both branches of every `mc:AlternateContent`, and text boxes. Not the glossary document, which is a document of its own | max + 1. Above 2³¹ − 1: refused with `DrawingIdsExhausted`, never a gap (review m1, proposed default Q2; ADR-0012's `ShapeIdsExhausted`). New `pic:cNvPr/@id` is `0`, as Word writes | `docPr` duplicates within or across parts: 0; AC-branch duplicates in 9 files (legitimate); other `cNvPr` ids: `wps` POI 8 / tpl 10 / fx 6, DrawingML main SDK 29 (`review1.py`); largest max + 1: 1,073,741,826 |
| **Annotation ids** | `w:id` of exactly these elements, in every story part and `comments.xml`: `bookmarkStart`/`End`, `commentRangeStart`/`End`/`commentReference`, `w:comment`, `ins`/`del`/`moveFrom`/`moveTo`, `rPrChange`/`pPrChange`/`sectPrChange`/`tblPrChange`/`trPrChange`/`tcPrChange`/`tblGridChange`/`numberingChange`, `cellIns`/`cellDel`/`cellMerge`, `move*RangeStart`/`End`, `customXml*Range*`, `permStart`/`permEnd`. **Not** `w:sdtPr/w:id`, which is random and can be negative (SDK 1,948, 21 negative). Non-numeric values (`permStart`'s `ST_String`) are ignored for the max. The glossary document is included (harmless). Review m2 | max + 1 over the numeric union, never a gap; above 2³¹ − 1 `AnnotationIdsExhausted` | cross-category collisions: 0 in POI/SDK, 2 template outputs. The union is the conservative reading (roadmap Q4). Largest max + 1: 137. Footnote/endnote ids are a separate space (Phase3; separators `-1`/`0`) |
| **Bookmark names** | every `bookmarkStart/@w:name`, plus every name a `REF`/`PAGEREF`/`NOTEREF`/`HYPERLINK \l` instruction or a `w:hyperlink/@w:anchor` mentions | none: the caller names it. It must be absent ignoring case (Unicode lower-casing), at most 40 characters, and a **Unicode letter** followed by letters, digits or `_` *(Word's UI rule; review m4, proposed default Q3)*. Caller names starting with `_` are refused | exact duplicates: POI 1 file (LibreOffice `__DdeLink__`), 8 template outputs; case-only: 0; over 40 characters: 1; Cyrillic names exist (POI `WordWithAttachments.docx`) |
| **Numbering** | `numId`: `w:num/@w:numId` ∪ `w:numIdMacAtCleanup/@w:val` ∪ every `w:numPr/w:numId` in the stories, styles and numbering parts. `abstractNumId`: definitions ∪ `w:num/w:abstractNumId` | max + 1 for each. `numId` 0 is reserved ("no numbering": 58 SDK `numPr`) and never allocated (review m3) | duplicates, gaps, dangling references: 0 everywhere; `numIdMacAtCleanup` ≥ max `numId` in 4 SDK files; largest max + 1: 100 / 104 |
| **`nsid`** | all `abstractNum/w:nsid` | random 8-hex-digit value absent from the set | duplicates: 0 |
| **Style ids** | `w:styleId` in `styles.xml` ∪ every `pStyle`/`rStyle`/`tblStyle` in the stories and styles, `basedOn`/`next`/`link`, `numbering.xml`'s `w:lvl/w:pStyle`, `styleLink`/`numStyleLink` (`stylesWithEffects.xml` read-only, §8) | derived from the name, checked; see §8 | undefined references: POI 22 `pStyle` + 1 `link`, template 8 `link` (`review1.py`); duplicate ids: 0 |
| **Relationship ids, part names, content types** | core (§1) | core | §1 |

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
- **Media part:** named, related and typed through the core API (§1). Reuse is in two layers,
  as in all three libraries (§13):
  - **package-wide**, an existing media part is reused when it has the same bytes (SHA-256,
    then a full byte comparison) **and** a content type equal to the sniffed one (review m5).
    A part edited in this session is hashed by its current bytes;
  - **per story part**, an existing `…/image` relationship to it is reused (get-or-add);
    otherwise a new relationship is added.

  Producers share media parts (POI 2, SDK 6, template 8 files) and reuse one `rId` for
  several drawings (POI 4, SDK 7 files). Hashing reads only parts targeted by image
  relationships.
  - **Later phases:** replacing or deleting an image must copy-on-write a shared media part,
    after counting its references in all stories and the glossary (review m5). Phase2 only
    adds.
- **XML written:** the minimal form Word writes:
  - `wp:inline` with `distT/B/L/R="0"`;
  - `wp:extent`, `wp:effectExtent` (zeros);
  - `wp:docPr id descr`;
  - `wp:cNvGraphicFramePr/a:graphicFrameLocks noChangeAspect="1"`, where
    `a:graphicFrameLocks` **declares `a` itself** (revision 2, review M2);
  - `a:graphic` (declares `a`), with `a:graphicData uri=".../picture"`;
  - `pic:pic` (declares `pic`) with `pic:nvPicPr(cNvPr id="0" name, cNvPicPr)`,
    `pic:blipFill(a:blip r:embed, a:stretch/a:fillRect)`, and
    `pic:spPr(a:xfrm(off, ext), a:prstGeom prst="rect"/a:avLst)`.

  **Declaration rule (revisions 2 and 3, reviews M2, N1).** Each outermost created element
  whose name, or an attribute name in its created subtree, uses a namespace (`a`, `pic`, `wp`,
  `r`) not bound at its insertion point is inserted **with** that declaration, using core's
  insert-with-declaration primitive (§1). `a:graphicFrameLocks` is the child of a sibling of
  `a:graphic`, so `a:graphic`'s declaration does not reach it. Word declares `a` on both:
  - measured, `a:graphicFrameLocks` carries its own `xmlns:a` in fx 34/34, POI 60/60,
    SDK 290/291, docx4j 6/6 and template 40/49;
  - the 10 without one sit in files that declare `a` on the root.

  With the rule, revision 1's `NamespaceNotInScope` for a story root without `wp` (POI 6/113,
  SDK 6/442 main parts) is gone: `wp:inline` is inserted declaring `wp`, and `r` when the
  root lacks it. Every insert of the operation is dry-run with the check twin (§1) before the
  first write.
- **Where:** a new run, after the given run or at the end of the paragraph (`insert_by` in
  `w:p`'s content slot). The run gets no `rPr`. The paragraph must pass ADR-0007's paragraph
  checks, so no image is inserted inside a field result, a revision or a content control.
- **Gates:** W1 and W3. W2 per ADR-0007 §6 with a new **structure** column (§10).

### 4. Hyperlinks

- **External:** `w:hyperlink r:id` with a new relationship (`…/hyperlink`,
  `TargetMode="External"`) **per hyperlink**. Producers never share one (0 files), and Word
  writes one per link (POI 5, SDK 8 files have several relationships with the same URL). The
  URI must be absolute (`scheme:`). It is stored as given, never normalised. Core refuses a
  target with whitespace or control characters (`InvalidTarget`). The caller percent-encodes;
  ooxml-docx does not do it silently (review m6).
- **Internal:** `w:hyperlink w:anchor="name"` to an existing bookmark (the name compared
  ignoring case), else `UnknownBookmark`.
- **Operations:**
  - `insert_hyperlink(position, text, target)` creates a new `w:hyperlink` holding one new
    run with the character style **named** "Hyperlink" when it exists, found by `w:name`
    (ADR-0007 §5), else no `rPr`. Its id is localized: `a`/`aN` in Japanese and Russian Word
    (review m6).
  - A position inside an existing `w:hyperlink` is refused (`NestedHyperlink`).
  - `replace_with_hyperlink(match, text, target)` combines ADR-0007's `replace` with run
    splitting (§9). The matched text is removed; `r₀` is split at the match position (§9.2);
    the hyperlink is inserted between the two halves, its run carrying a copy of `r₀`'s
    `w:rPr` plus `rStyle`.
- No runs are ever moved into a wrapper. That would need a move primitive (Alternatives 3).
- **Namespace (revision 4, review R3-3).** `w:hyperlink` is inserted with `NsDecl(r)`
  (`insert_by_with_ns`, §1). When `r` already has a prefixed binding in scope nothing is
  declared; otherwise the hyperlink element carries `xmlns:r`. Measured (reviewer's
  `probe_r.out`, Transitional only): story parts without a prefixed `r` binding on the root
  are POI 7 (`document.xml` 6, `footer.xml` 1), SDK 6, template 2 (`footer.xml`). None has a
  `headerReference`, so §6's section-break rewrite is not affected. Tests include a POI file
  without `r` (`55733.docx`).

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
  registry's max stays. It is refused (review m4) when the name occurs more than once (tpl 8,
  POI 1 file), or when the name is mentioned by a `REF`/`PAGEREF`/`NOTEREF`/`HYPERLINK \l`
  instruction or a `w:hyperlink/@w:anchor` (`BookmarkReferenced`).

### 5a. Lists and numbering

- **`create_list(template: ListTemplate) -> NumId`** (`Bullet`, `Decimal`; Open question 8)
  makes the following additions.
  - **Numbering part:** created if absent, through the core API (§1). It is
    `word/numbering.xml`, or core's next numbered name if that is in use, with the
    WordprocessingML numbering content type and a `…/numbering` relationship from the main
    part. 80
    of 101 writable POI files and 312 of 404 SDK files need this for their first list.
  - **`w:abstractNum`:** a new one with `abstractNumId` = max + 1, a fresh `nsid`,
    `multiLevelType="hybridMultilevel"` and the template's nine `w:lvl`. The generated slots
    of `CT_Numbering` place it after the existing `abstractNum` and before the first `num`.
    Producers keep that order: no violation in any corpus.
  - **`w:num`:** a new one, with `numId` = max + 1 over §2's scope, pointing at it. It is
    inserted at the end of the `num` slot, which is before `w:numIdMacAtCleanup` (review m3).
- **`apply_list(p, num, level)`** writes `w:pPr/w:numPr` (`w:ilvl`, `w:numId`) with
  `get_or_insert`.
  - The `numId` must exist.
  - The paragraph must pass ADR-0007's paragraph checks. A `w:pPrChange` refuses
    (`InsideRevision`).
- **`restart_list(p)`** restarts numbering at a paragraph that has its own `w:numPr/w:numId`.
  - It adds a new `w:num` with the same `abstractNumId` and
    `w:lvlOverride w:ilvl=<p's level>/w:startOverride w:val=<start>`. Producers do this:
    `startOverride` POI 7, SDK 15, template 24.
  - The old `num`'s `lvlOverride`s for the **other** levels are copied (`detach_as`), so their
    look does not change (review m3). `num` with `lvlOverride`: POI 9, SDK 18, template 26.
  - It reassigns `numId` on `p` and on every later paragraph of the same story that carries
    the old `numId` **in its own `numPr`**, which is what Word's "Restart at 1" does *(u)*.
  - Paragraphs numbered only through their style are left alone, and the result for them is
    stated in the API documentation.
  - If any paragraph to reassign fails ADR-0007's paragraph checks, the whole call is refused,
    before any write.
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
  `oddPage`) splits the section S that contains the paragraph p.
  - The **earlier half** (ending at p) gets a copy of S's `w:sectPr` in p's `w:pPr`. The copy
    is made by `detach_as` with `headerReference`/`footerReference` skipped. References with
    the same `w:type` and `r:id` are then inserted with typed setters, after each id is checked
    in the part's relationships (§1). The copy keeps S's header/footer references, page setup,
    start type and restart settings, so the earlier half looks exactly as S did.
  - **`w:type` = kind is written on the later half**, S's original `w:sectPr` (revision 2,
    review M1). `w:type` says how a section starts relative to the previous one *(ECMA-376
    Part 1 §17.6.22)*, so the boundary between the halves is the later half's start.
    python-docx writes the kind on the later (sentinel) `sectPr` too (`document.py:146-147`,
    `oxml/document.py:63-71`).
  - **Restart settings stay only in the earlier half (review M1).** On the later half,
    `w:pgNumType/@w:start` is removed, so page numbers continue across the break.
    `w:lnNumType/@w:restart` is set to `continuous` when it is `newSection`. Note numbering
    (`w:footnotePr`/`w:endnotePr` `w:numRestart`) is left as it is: it is per-section by
    nature *(u)*.
  - **What the later half does not keep (revision 3, review N9):**
    - `w:titlePg` stays on both halves, so the first page after the break shows the
      first-page header, as when a break is inserted in Word *(u)*. Measured `sectPr` with
      `titlePg`: POI 6, SDK 71, template 12.
    - With `numRestart` `eachSect` or `eachPage` (SDK 20 `sectPr`, settings 3), footnote
      numbers after the break change. That is why `NOTEREF` is not allowed in the structure
      column (§10).
    - Both are documented on the API.
    - Measured `sectPr` with `pgNumType/@start`: POI 6, SDK 23, template 11. SDK has 20
      `footnotePr/numRestart` and 1 `endnotePr/numRestart` (`review1.py`).
    - Word's behaviour is a `manual_check` item.
  - The break is refused when S's `w:sectPr` has `w:sectPrChange` (`InsideRevision`; both
    halves are written now), and when p already ends a section (`AlreadySectionEnd`).
  - p must be a body paragraph, not in a table or content control (`UnsupportedContext`).
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
inside a revision. A table with legacy `w:hMerge` is refused for column operations and
merges (review M5).

**Deleting operations refuse notes and revisions (revision 2, review M6).** `delete_rows`,
`delete_column` and `merge_cells` remove content. They are refused when the removed range
holds:

- a `w:footnoteReference` or `w:endnoteReference` (`WouldRemoveNote`): removing it would
  renumber the later notes, make `NOTEREF` results stale and orphan the note. Measured: table
  rows holding one in POI 3 and SDK 3 files (`review1.py`);
- any revision element: run-level `ins`/`del`/`moveFrom`/`moveTo`, a paragraph-mark
  `ins`/`del`, or any `*PrChange`/`cellIns`/`cellDel`/`cellMerge` (`InsideRevision`).
  Removing it would accept or reject a tracked change silently. W1 does not catch this when
  `trackRevisions` is off. Measured: rows holding revisions in SDK 3 files.

#### 7.1 Insert rows

`insert_rows(table, at, count, template: RowTemplate)` builds each new row from a **template
row** (`Above` by default, or `Below`):

- `w:tr` keeps no attributes. In particular `w14:paraId`/`textId` and rsids are not copied
  (rows with `w14:paraId`: SDK 54).
- `w:trPr` and `w:tblPrEx` are copied (`detach_as`), **without `w:tblHeader`** (review m7).
  Otherwise a row inserted below a heading row would also repeat on every page. Measured:
  `tblHeader` rows POI 33, SDK 138. The copy is refused if they contain
  `ins`/`del`/`trPrChange`.
- Per cell:
  - a `w:tc` with a copy of its `w:tcPr`, refused if it has `tcPrChange` or
    `cellIns`/`cellDel`/`cellMerge`;
  - one `w:p` with a copy of the template cell's first paragraph's `w:pPr`, minus `sectPr`,
    `pPrChange` and `numPr`, and with its mark run properties minus `ins`/`del`/`moveFrom`/
    `moveTo`/`rPrChange` (review M6);
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
  - falls at a cell boundary or at the row's end: a new cell is inserted there, or
  - falls inside a cell's span (`gridSpan` > 1) or in `gridBefore`/`gridAfter`: that span is
    widened by one.
- **The new cell's `tcPr` (revision 2, review M5)** is a copy of the left neighbour's, or the
  right neighbour's at grid column 0. The copy leaves out:
  - `gridSpan`, `vMerge` and `hMerge`: a span would break the grid, and a merge would create
    an unintended one. Measured `gridSpan` > 1: POI 7,534 and SDK 1,658 cells;
  - `tcPrChange` and `cellIns`/`cellDel`/`cellMerge`;
  - `tcW`, which is set to the width.

  Its paragraph is built as in §7.1.
- **Widths:** `tblW` is not changed. When `tblW` is fixed (`w:type="dxa"`), Word reflows *(u)*.

`delete_column` removes the `gridCol`. In each row it removes the cell that covers only that
column, refused if the cell is not empty, or narrows the span that covers it. It is also
refused when a row would be left with no `w:tc` (`RowWouldBeEmpty`): Word treats a `w:tr`
without cells as a broken table *(u)*.

#### 7.4 Merge cells

`merge_cells(table, rows, grid_cols)` merges a rectangle:

- horizontally, the first cell of each row gets `gridSpan` = the width and `tcW` = the sum of
  the merged cells' widths (review m7). The other cells of the rectangle are removed;
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
- **Id (revision 2, review M4; proposed default Q4):**
  - Start from the name with every character outside `[A-Za-z0-9]` removed. That is Word's
    rule as observed: `Tělo textu` → `Tlotextu`, `Überschrift 1 Zchn` → `berschrift1Zchn`.
  - If that is **empty** (a fully Cyrillic or CJK name), the id is `a`, or `a` + the
    smallest free number. That is close to how localized Word names such styles; Word also
    uses letter suffixes (`aa`, `ab`: POI 33, SDK 27; revision 3, review N6). Measured: names that
    strip to empty, POI 89 / template 30 / fixtures 7; ids of the form `a`/`aN`, POI 131,
    SDK 131, fixtures 37 (`review1.py`).
  - A stripped id that starts with a digit is kept: `ST_String` allows it, and Word writes
    such ids *(u)*.
  - A collision with §2's scope, which includes dangling references (M3), compared ignoring
    case, gets the smallest numeric suffix that is free.
  - The caller may pass an explicit id, which is checked against the same scope.
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
  is only `TC`, `XE` *(conservative; `NOTEREF` removed in revision 3, review N9: a section
  break can renumber notes; 0 corpus files use `NOTEREF`)*. Everything else needs the `updateFields`
  opt-in.
- Images, hyperlinks, bookmarks and styles are classified like this:

  | Operation | Column |
  |---|---|
  | `insert_image` | format: an image adds no paragraph and no text, but changes pagination |
  | `insert_hyperlink`, `add_bookmark` | text |
  | `add_style` | none (it changes no story), except W1/W3 |
  | `split_run` | none: the text and its formatting are unchanged (review m9) |
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
- **`[detach]`** gains the following entries (revision 3, review N7, adds the last one):
  - `w:CT_TrPr/w:trPr`, `w:CT_TcPr/w:tcPr`, `w:CT_PPr/w:pPr`;
  - `w:CT_SectPr/w:sectPr`, always copied with its header/footer references skipped (§6);
  - `w:CT_NumLvl/w:lvlOverride`, for `restart_list` (§5a). Each review checks that the
  subtree holds no document-unique id once the listed skips apply.
- **New file `codegen/allowlist/wpd.toml`** for the `wp` and `pic` entries, which only Word
  uses: `wp:CT_Inline/wp:inline`, `wp:CT_PositiveSize2D/wp:extent`,
  `wp:CT_EffectExtent/wp:effectExtent`, `wp:CT_NonVisualDrawingProps/wp:docPr`,
  `wp:CT_NonVisualGraphicFrameProperties/wp:cNvGraphicFramePr`, and `pic:CT_Picture/pic:pic`
  with its children. pptx uses `p:pic` and xlsx `xdr:pic`. Proposed default Q7.
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
  - every relationship **added by the operation** has an internal target that exists. Like
    the content-type check, this is a difference against the input (revision 3, review N3):
    the SDK has 26 dangling targets in 19 files;
  - every part **added or changed by the operation** has exactly one content type, by
    `Override` or by `Default`. The check is a difference against the input: POI has 1 file
    with an untyped part from its producer (review m10). Every new `Override` names an
    existing part;
  - no new part name equals an existing relationship target, dangling ones included
    (review m10);
  - relationship ids are unique per source, ignoring case, among the ids added by the
    operation and against the input's ids;
  - no new part is an orphan;
  - every `r:id`/`r:embed` written resolves in its part's relationships;
  - the zip has no duplicate names (OPC equivalence).
- **T-REG (registries):**
  - every allocated value is absent from its whole scope (§2), **definitions and references**,
    before the write (review M3);
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
  - a section break in a landscape document, and in a section with
    `pgNumType/@start` (M1);
  - `continuous` and `nextPage` breaks, checked at the boundary (M1);
  - merges;
  - a style named like a localized built-in.

### 13. How the libraries do it (source-verified; §Sources for pins)

| Concern | python-docx 1.2.0 | Apache POI XWPF | docx-rs 0.4.22 | ADR-0011 |
|---|---|---|---|---|
| rId | first gap (`opc/rel.py:104-111`); reuses a rel with the same type and target (`:31-47`, `:70-87`) | count+1, then a forward cursor with a collision check (`PackageRelationshipCollection.java:209-229`) | fixed or per-type strings (`rId1`…`rId8`, `rIdHeader{n}`, `rIdImage{n}`); customXml `rId{i+8}` can collide with footnotes' `rId8` (`document_rels.rs:56-117`) | core's rule (`core/add-part`, §1) |
| part names | first free number among all parts (`opc/package.py:109-121`); images by first gap regardless of extension (`package.py:95-110`) | `getNextPartNumber` probes names (`POIXMLDocumentPart.java:614-647`), but headers use the rel count + 1 and can hit an existing name (`XWPFHeaderFooterPolicy.java:185, 237`) | order counters; media always `.png` (`zipper/mod.rs:80-103`, `image_collector.rs:137`) | core's `Numbered`: max + 1, avoiding dangling targets and Overrides (§1) |
| content types | rebuilt from a whitelist on save; the original is not kept (`opc/pkgwriter.py:80-99`) | Default for a new (extension, type) pair, else Override (`ContentTypeManager.java:152-164`) | fixed Default list incl. non-standard `image/jpg` (`content_types.rs:156-172`) | core's rule: existing matching `Default`, else `Override`; Word adds no `Default` (§1) |
| image dedup | package-wide SHA-1; rel per part (`package.py:68-93`, `parts/story.py:27-39`) | checksum + byte compare; rel per part (`XWPFDocument.java:1596-1680`) | (length, crc32) + bytes, package-global; rel per part (`image_collector.rs:15-70`) | SHA-256 + bytes, package-wide; rel get-or-add per part |
| `docPr` id | max + 1 of `//@id` in **one part** (`parts/story.py:76-88`): a header and the body can collide | lowest free id document-wide (`IdentifierManager.java:120-129`); an existing duplicate is silently re-reserved elsewhere (`:58-114`) | always `1` (`drawing.rs:132`) | max + 1 over all `docPr`/`cNvPr` ids in all stories and AC branches; refuse above 2³¹ − 1 |
| `pic:cNvPr` id | 0 (`oxml/shape.py:147-151`) | 0 (`XWPFRun.java:1416-1440`) | 0 (`pic.rs:224`) | 0 (as Word: SDK 265 of 285) |
| bookmarks | no API | no API | caller's id and name, unchecked (`bookmark_start.rs:14-19`) | annotation union max + 1; name checked |
| comment ids (Phase3) | max + 1 over comments, first gap above 2³¹ − 1 (`oxml/comments.py:68-87`) | caller's (`XWPFComments.java:286-292`) | caller's | union registry (§2) |
| numbering | no list API; numbering.xml cannot be created (`parts/numbering.py:11-14`) | `addNum`: count + 1, **no collision check** (`XWPFNumbering.java:145-153`); `abstractNum` max + 1; creates the part (`XWPFDocument.java:1017-1030`) | caller's ids; always emits a built-in num 1 that a caller's 1 duplicates (`numberings.rs:32-46`) | max + 1 each, `numId` 0 reserved, `nsid` fresh; creates the part through §1 |
| tables | `add_row`/`add_column` keep `tblGrid` (`table.py:37-55`); `merge` moves content into the top cell (`oxml/table.py:661-735`) | `createRow` copies row 0's cell count; `addNewCol` ignores `tblGrid`; new tables have none (`XWPFTable.java:233-307`) | caller sets everything; no checks (`table.rs:36-69`) | consistent-grid precondition; merges only of empty cells |
| hyperlinks | read-only; the generic `relate_to` would share a rel per URL | new external rel each time (`XWPFParagraph.java:1542-1558`) | new global id each time (`hyperlink.rs:34-50`) | one rel per hyperlink (producers never share) |
| headers | `next_partname` + `relate_to` + schema-ordered reference (`section.py:316-325, 446-450`) | rel-count naming, body-level `sectPr` only | count + 1 | not created in Phase2; references copied on breaks |
| section break | clones the sentinel `sectPr` into a new last paragraph, strips the sentinel's references (`oxml/section.py:186-195`) | none | none | copy into the earlier half, references rewritten; `w:type` on the later half, as python-docx (§6) |
| styles | styleId = name without spaces (special cases for headings, caption), collisions unchecked (`oxml/styles.py:16-30, 280-290`) | appended unchecked (`XWPFStyles.java:201-206`) | caller's | ASCII-stripped id, `a`/`aN` when empty, checked against definitions and references; latent built-in names refused |
| `w14:paraId` | none | none | thread-local counter `00000001…`, duplicates reassigned before writing (`paragraph_id.rs`, `mod.rs:963-981`) | none in Phase2 (Alternatives 4) |
| `stylesWithEffects.xml` | carried unchanged from its template | not referenced | not referenced | left unchanged (Open question 5) |

Every library either allocates from a partial scope (python-docx's `docPr`, POI's header
names and `numId`) or does not allocate at all (docx-rs). None of them checks that a value
it hands out is absent everywhere it must be. That check is what T-REG tests.

## Alternatives considered

1. **Count + 1 for part names and `rId`s** (POI's header naming and `addNum`, docx-rs; §13).
   Wrong here:
   - `rIdN` sets that are not `1..n` occur in 4 files, and other id shapes in 16;
   - header numbers go up to 20 with gaps, and `word/footer.xml` exists.

   Core's max + 1 (§1) does not collide.
2. **Reuse gaps** (first free `rId`, `numId`, part number). Rejected for ids and, since
   revision 2, for part names too, following core (review B1). A gap may be a removed entry
   whose stale reference survives, which is ADR-0001's reason for append-only. Revision 1's
   "smallest free name" contradicted this.
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
9. **A Word-specific part plan in core** (revision 1's `PartPlan`). Withdrawn (B1). One
   core API serves Word, xlsx and pptx, and its rules table is decided once, by the core PR
   with the owners of ADR-0009 and ADR-0012.
10. **Scopes of definitions only** (revision 1). A new style, list or relationship id could
    adopt a dangling reference and change existing content without a write to it (M3).
    Rejected.

## Consequences

- **Positive:**
  - Structure can be added to real Word files without renumbering or repairing anything.
  - Every new id is fresh in a scope that is measured to be the right one: no `docPr`
    collision across parts in any corpus, so the union scan is sufficient.
  - One core registry API serves Word, xlsx and pptx (§1).
  - New values never adopt a dangling reference (M3).
- **Positive:** T-OPC and T-REG apply to both formats and are independent of the code they
  check (T7).
- **Negative:** this ADR depends on the core registry API (`core/add-part`, PR #38) and
  on two additive lens changes, each with its own review:
  - insert with namespace declaration, plus its check twins (PR #45, §1);
  - `SameType` modulo skips (shared with ADR-0007).
- **Negative:** these are refused in Phase2 and wait for a move primitive:
  - merging non-empty cells;
  - wrapping existing text into a hyperlink without splitting into new runs;
  - deleting rows that hold half of a bookmark.
- **Negative:** deleting and merging operations refuse ranges with footnote references or
  revisions. Restarting a list leaves paragraphs numbered only through their style
  unchanged.
- **Negative:** the allowlist grows by roughly 60 entries across `wml`, `wp`, `pic` and `a`.
  The DrawingML part is shared with PowerPoint and needs that owner's review.

## Open questions for the project owner

Revision 2 records the round-1 reviewer's recommendations as **proposed defaults**. They are
what this revision specifies, pending the owner's confirmation.

1. **Core API.** *Proposed default:* no Word-specific `PartPlan` (B1). The core registry API
   on `core/add-part` is the single source of truth (coordinator decision, 2026-11-16). One
   core rules table, agreed with ADR-0009 and ADR-0012, covers:
   - the `Default` policy (Word adds none);
   - part names (max + 1, avoiding dangling targets, `Override`s and removed names);
   - the case sensitivity of `rId`;
   - whether `r:*` references in the source count as used.
2. **`docPr` ids above 2³¹ − 1.** *Proposed default:* refuse with `DrawingIdsExhausted`.
   The observed max is 1,073,741,826, far below.
3. **Bookmark names.** *Proposed default:* at most 40 characters; a Unicode letter followed by
   letters, digits or `_`; compared ignoring case; callers may not start a name with `_`.
   A Word check is added to `manual_check`.
4. **Style ids.** *Proposed default:* strip to ASCII alphanumerics (`Tlotextu`,
   `berschrift1Zchn`); when empty, `a` + the smallest free number (M4).
5. **`stylesWithEffects.xml`.** *Proposed default:* new styles are not added to it.
6. **Headers for a single section.** *Proposed default:* not in Phase2. A later design covers
   explicit references on the following sections, `titlePg` and `evenAndOddHeaders`.
7. **Allowlist.** *Proposed default:*
   - `wp`/`pic` entries in a new `wpd.toml`;
   - `a:*` entries in `dml.toml`, reviewed by ADR-0005's owner;
   - the dry run as the implementation PR's first commit.
8. **List templates.** *Proposed default:* copy them from the `numbering.xml` of a Word 365
   document the owner provides, committed as a fixture.
   - Issue a new `nsid`.
   - Drop `w:tmpl`, `numStyleLink`/`styleLink` and `w15:restartNumberingAfterBreak`.
   - Keep the bullet `lvlText` (U+F0B7, …) and its `rFonts` (Symbol, Wingdings, Courier New).
9. **Enforced-protection opt-in** (ADR-0007 Q10, handed over; revision 4, review R3-4).
   *Proposed default:* not in Phase2. W3 keeps refusing; the opt-in goes to the later
   document-settings ADR. The corpora have 4 protected POI files.

## Implementation notes (post-acceptance)

### Part 1 (§4, §6, §9.1, §10)

Recorded with the implementation (`impl/docx-phase2`); additive or clarifying.

- **Generations.** A paragraph insert moves later paragraphs: they get fresh
  `Document::generation` values, so a `Match` found before the insert is stale (ADR-0007 §7).
  Generations are now values never used before in the `Document`, not counters per
  paragraph.
- **`insert_paragraph`.**
  - The neighbour must pass ADR-0007 §4's paragraph checks. A neighbour in a block-level
    content control or custom XML is `UnsupportedContext`.
  - The new run has no `w:rPr`. Copying the paragraph mark's run properties needs ADR-0007
    §8.2's `SameType` rule, which comes with Phase1 part 3; it will be added then.
  - `ParagraphSpec::Plain` writes no `w:pPr`.
- **Section breaks.**
  - Only paragraphs that are direct children of the main body can end a section. A section
    break inside a table between the paragraph and S is refused.
  - A missing `w:sectPr` is `NoSectionProperties`.
  - A `w:sectPrChange` is `TrackedPropertyChange`.
  - A reference whose `r:id` is not a relationship of the part is `RelIdReferenced`.
  - `w:printerSettings r:id` makes the copy `NotDetachable`.
- **A2 lifted.** `append_paragraph` moves the `w:sectPr` by copy and remove. The old
  paragraph keeps an empty `w:pPr`. Inside a content control, A2 still refuses.
- **Hyperlinks.**
  - `w:history="1"` is written, as Word does.
  - Core's relationship-id allocation already skips ids used by the source part's
    attributes (PR #38, M7), so ooxml-docx's `RelIdReferenced` check is a safety net that
    the tests cannot reach (equivalent mutant).
  - The URI check accepts `scheme:` as RFC 3986 defines it, with any rest; core refuses
    whitespace and control characters.
- **Corpus cross-check (throwaway, 821 files).**
  - Gate and per-run outputs are byte-identical to `main`.
  - The sweep ran, per file: a paragraph after each of the first 60 paragraphs of every
    story; up to 3 section breaks; a hyperlink at the end of each of the first 60 body
    paragraphs; then an append.
  - Results: 13,565 paragraphs, 1,997 breaks, 14,143 hyperlinks and 774 appends written.
    After saving and reopening there was no panic, no reopen failure and no text mismatch.

### Part 2 (§3, §7)

- **Addressing.** A table is a `TableRef`: its index among a story's `w:tbl` in document order,
  nested tables and tables in content controls included. A table inside a content control is
  refused.
- **Paragraph marks of copied rows.** §7.1 says a copied paragraph mark drops its revision
  elements. `detach_as` can skip only direct children, and the mark's `w:rPr` sits below
  `w:pPr`, so a template paragraph whose mark carries a revision element is refused
  (`RevisionInRange`) instead. The same applies to new cells of `insert_column`.
- **Copied row properties.** A `w:trPr` holding `ins`, `del` or `trPrChange`, or a `w:tcPr`
  holding `tcPrChange`, `cellIns`, `cellDel` or `cellMerge`, is `TrackedPropertyChange`.
- **`delete_rows` and fields.** It refuses any `w:fldChar`/`w:fldSimple` in the removed rows
  (`InsideField`), even when the field is complete inside them. This is stricter than §7.2 and
  fails closed.
- **"Empty" for `delete_column` and `merge_cells`:** exactly one paragraph holding nothing but
  `w:pPr`. That is stricter than "no runs with text, no anchors", and fails closed.
- **Widths.** `merge_cells` writes `w:tcW` = the sum of the merged grid columns (dxa), when
  every one of them has a numeric width and the sum is representable. `insert_column`'s new
  cell gets `w:tcW` = the width (dxa). When a column edit changes a cell span and every grid
  column in the original span has a numeric width, the operation adds or subtracts the edited
  column's width from an existing numeric dxa `w:tcW`. When the width cannot be calculated,
  the existing `w:tcW` is preserved. Column edits also preserve non-dxa widths.
- **Tracked changes (PR #66 and issue #78).** Column edits and merges refuse row-property
  revisions anywhere in the table. `delete_rows` also refuses row-property revisions
  (`ins`, `del`, `trPrChange`) in a row where it repairs a vertical merge. For retained
  cells whose properties are rewritten, the cell-level check covers `tcPrChange`,
  `cellIns`, `cellDel` and `cellMerge` directly under `w:tcPr`. Tracked paragraph
  text in those cells is preserved. A retained merge continuation must have an empty
  paragraph with revision-free paragraph properties.
- **Revisions in removed cells.** `delete_column` and `merge_cells` share the removal checks
  of `delete_rows` for revisions, notes, fields and comment anchors. All three use
  `RevisionInRange` for revision elements.
- **Generations.** After a table edit, every paragraph from the table on gets a fresh
  generation.
- **Images.**
  - **Extent.** A PNG density rounding to zero on either axis falls back to 96 DPI.
    Both automatic and explicit dimensions must be in 1–27273042316900 EMU.
  - **Allowlist.** The `wp` and `pic` entries are in `wml.toml`, not a new `wpd.toml` group
    (Q7): the generator's groups are cargo features, and only ooxml-docx uses these entries.
    The shared `a:` entries are in `dml.toml`, where `a:graphic` and `a:graphicData` move from
    read to write. ADR-0005's owner should review them.
  - **Media reuse** compares content type and bytes directly; there is no SHA-256 index
    (§3's hash is an optimisation).
  - **Part names.** Core numbers media per name pattern, so after `image1.png` a GIF is
    `image1.gif`.
  - **DPI.** A PNG `pHYs` value is rounded to whole dots per inch (5670 px/m → 144 dpi).
  - **Drawing ids** are scanned at call time over every story part's tree, which includes the
    session's own insertions.
- **Corpus cross-check (821 files).**
  - Gate outputs are byte-identical to `main`.
  - Per table: a row inserted at 1 and deleted again (510 times), and a grid column inserted
    at 1 and deleted again (508 times). All undos succeeded.
  - One image per story: 2,192 inserted.
  - After saving and reopening there was no panic, no reopen failure and no text mismatch.

## Sources

- Spike: `spikes/docx-phase2/README.md`, `registries.py`, `needs.py` and, since revision 2,
  `review1.py`; run 2026-11-16, Python 3.15.
- Review: `docs/adr/0011-opus-review-round1.md`.
- Core registry API: PR #38, branch `core/add-part` (`crates/ooxml-core/src/package/add.rs`).
- Core "insert with namespace declaration" (§1): **PR #45** (`core/insert-with-ns`,
  `crates/ooxml-core/src/lens.rs`, `tests/insert_ns.rs`, its ADR-0002 note). That note's
  "ADR-0011 §1.3" means §1 "What else core needs", item 1 (fixed on the PR #45 side).
- Review: `docs/adr/0011-opus-review-round3.md` (probes `probe_r.py`, `probe_r2.py`).
- Review: `docs/adr/0011-opus-review-round2.md`.
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
  ADR-0006 (xlsx Phase2; part creation deferred to Phase3), ADR-0007 (Accepted, PR #25), PR #14
  (`remove_part`), PR #18 (`detach_as`/`insert_detached`), `docs/roadmap/docx.md` §5
  (registries), `docs/roadmap/xlsx.md` Phase3.
- ECMA-376 Part 1 (`CT_NonVisualDrawingProps/@id`, `ST_DrawingElementId`; bookmarks
  §17.13.6; numbering §17.9). Section numbers are from memory and marked *(u)* where a rule
  depends on them. ECMA-376 Part 2 (OPC: part-name equivalence, content types,
  relationships).

### Post-acceptance note (PR #62, round 1)

- **M1:** In an ADR-0004 A2 document, inserting after the last body-flow
  paragraph moves its final `w:sectPr` to the new paragraph, as append
  does. The requested paragraph style or copied properties still apply.
  If the final paragraph is in a wrapper or table, the insertion returns
  `SectionBreakAtEnd`. Inserting after an earlier section-ending paragraph
  places the new paragraph in the following section.
- **M2:** A paragraph that starts inside a complex field is an
  `InsideField` target for paragraph, hyperlink and section-break insertion
  even when it has no runs.
- **m1–m3:** Splitting after the last paragraph of a section returns
  `AlreadySectionEnd`. Paragraph and hyperlink insertion refuse separator,
  continuation separator and continuation notice notes. A section split
  invalidates the generation of each paragraph whose section properties it
  changes.

### Part 3 (§5, §5a, §8)

| Operation | W2 column |
|---|---|
| `add_bookmark` | Text |
| `remove_bookmark` | Structure |
| `create_list`, `apply_list`, `restart_list`, `remove_from_list` | Structure |
| `add_style` | W1/W3 only |

- `BookmarkRange` takes a current `Match` or inclusive first/last paragraph
  references. Splits retain formatting and omit run revision-session attributes
  on the new halves. The annotation maximum survives removals for the lifetime
  of the open document. Name collisions ignore case. The field scan joins
  complex-field instruction fragments and also checks simple fields. Ambiguous
  marker pairs refuse, as do markers that the editable story tree cannot reach.
- `create_list` writes nine levels for either `Bullet` or `Decimal`. Selection
  of the numbering part and registration of a new one use the main part's
  relationships. `restart_list` takes an explicit start value and copies
  overrides for the other levels. It checks every affected paragraph before
  applying the new instance. `remove_from_list` follows the paragraph's style
  chain when deciding whether to suppress inherited numbering.
- `add_style` writes paragraph and character styles using the Phase1 formatting
  planner. It includes existing style references from numbering and stories in
  collision checks. The ECMA style run properties use `CT_RPr`, which includes
  highlight alongside the other Phase1 properties.
- **Open question:** the minimal `styles.xml` template remains part of the
  `Document::new()` design. Until that template is specified, adding a style to
  a document without a styles part returns `NoStylesPart`.
- The generated editable case is checked with python-docx using
  `crates/ooxml-docx/tests/oracle/phase2_registries.py`. The Rust tests check
  unchanged ZIP bytes on refusal, unchanged nodes and entries on success, and
  the appended OPC entries using an independent XML parser.
