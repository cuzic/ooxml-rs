# ADR-0004: Word Phase0: text edits and paragraph append in `document.xml`

- Status: **Proposed** (revision 3)
- Date: 2026-11-16
- Depends on: [ADR-0001](0001-lossless-round-trip-architecture.md) (layers, oracles, fail-honestly
  stance), [ADR-0002](0002-schema-driven-code-generation.md) (generated views, slots, primitives,
  allowlist, overrides)
- Evidence: `docs/roadmap/docx.md` (gap analysis, design questions Q1–Q7), `spikes/docx/`
  (scripts, results of 2026-11-16, re-run for revision 2), `reference/fixtures/docx/` (26 fixtures),
  and the implementation `crates/ooxml-docx/` with its tests.

## Revision history

- **Revision 1**: initial draft.
- **Revision 2** (after `0004-opus-review-round1.md` and the project owner's decisions of
  2026-11-16; written together with the implementation, so every rule below is what
  `crates/ooxml-docx` does):
  - **M1** — W2 tracks fields with a stack over each story part in document order and classifies
    every nesting level; a nested field's result never leaks into the outer code (§6 W2). The
    spike was fixed and re-run; `sdk-complex2010` now also reports the outer `TC`. Unit tests cover
    `{ IF { MERGEFIELD } }`, `{ = { PAGE } - 1 }` and `{ HYPERLINK "{ REF }" }`.
  - **M2** — navigation is designed against the merged core API (§3.4): reads walk the untyped
    `RawNode` tree, which core already exposes; writes descend with `LensMut::into_child_at`
    through allowlisted entries only, which works for `[read]` entries because `ChildOf` exists
    between any two allowlisted entries (ADR-0002 post-acceptance note). No core addition is needed
    for M2. One unrelated additive core change was needed: `Package::parsed_part` (PR #9, merged). The
    `ChildOf` claim of revision 1 is corrected (§4).
  - **M3** — new gate W3 `DocumentProtected` (§6), with a POI fixture added.
  - **m1** text mapping table with `\r`, whitespace-only text nodes, comments/PIs, the own-text
    round trip and the no-op rule (§3.2); **m2** R1 state over the whole body in document order,
    fail closed on an unterminated `begin` (§6); **m3** denominators recounted with hyperlink runs
    (§6, §7); **m4** reach table and `RunContext` per variant (§3.1); **m5** the "safe" criterion
    restated, per-operation W2 left as an open question (§6, Q2); **m6** W1 fails closed on an
    invalid value, eight-way unit test (§6); **m7** T5 reads only `document.children`, and
    `xml:space` is checked structurally (§7); **m8** list of caches Phase0 leaves stale (§5);
    **m9** the DTD check is core's parser-level `DocType` event, verified with a DOCTYPE after a
    10 kB comment (§6 O2).
  - **Owner decisions applied**: the field classification is accepted (§6 W2; `AUTONUM*` moved
    to content-dependent, see there); `Document::new()` is out of Phase0 (edit path only);
    `w14:textId` is kept unchanged pending the owner's Word check; dropping
    `w:lastRenderedPageBreak` in an edited run is accepted; DTD refusal lives in core; docx-rs is a
    dev-dependency; rdocx evaluation is deferred. A manual-check generator is provided (§7).
- **Revision 3** (after `0004-opus-review-round2.md`, the first review of the implementation;
  again written with the code):
  - **N1** (deep nesting) — core's parser now refuses element nesting deeper than
    `ParseOptions::max_depth` (default 1024; the deepest real part measured is 33) with
    `Error::TooDeep` before the tree grows (core PR #16, ADR-0002 post-acceptance note). It is the
    new open refusal **O5** (§6). In ooxml-docx the body index no longer stores a full path per run
    (paragraphs and runs refer to a frame: parent, child index, wrapper), so it is linear in the
    number of visited elements, and the body walk, the field scan and the run-lens descent are
    iterative. The §7 note on deep tables is rewritten: the cost is bounded by the limit.
  - **N2** — W2 classifies `INFO` and `DOCPROPERTY` with their first argument (statistics such as
    `INFO NumWords` and `DOCPROPERTY Characters` are refused), `FILESIZE` is content-dependent, and
    the audit of every safe code for argument-dependent behaviour adds `BARCODE \b`, calculated
    `FORMTEXT` and `DOCPROPERTY` of a custom property linked to content (§6 W2, "Classification").
  - **m1** — field markup that cannot be followed is reported as `<malformed field>` and refused
    (§6 W2). **m2** — new append refusal **A2** `SectionBreakAtEnd` (§3.2, §6). **m3** —
    duplicated `trackRevisions`/`documentProtection` elements: any one that is on counts;
    `writeProtection` and Mark as Final are reported, not gated (§6, "Reported, not gated").
    **m4** — T2 checks that the start tag and `w:rPr` of an edited run are byte-identical, uses an
    inner double space in a piece without edge spaces, and re-reads the saved package (§7); T6's
    limits are stated. **m5** — `docProps/core.xml` and `docProps/custom.xml` added to §5. **m6** —
    the no-op rule accounts for `xml:space` (§3.2 step 3).
  - The reviewer's answers to owner questions 2 and 3 are recorded as the planned Phase1 direction
    (roadmap, Phase1; open questions below).
  - The review's adversarial probes P01–P12 are assertions in `tests/adversarial.rs`.

## Context

ADR-0001's Phase0 proved the three-layer design on Excel. Before that, the Word research note
(`docs/research/2026-11-16-word-powerpoint-applicability.md`) established three things:

- ADR-0002's generator works for WordprocessingML with no unsupported base-namespace content
  model. The slot order matched 171,537 real child lists.
- The hard part of Word is the domain layer, not code generation: cross-part IDs, fields,
  tracked changes and inheritance.
- A candidate first scope: replace text of existing runs, append paragraphs before `sectPr`, no
  new styles/numbering/IDs, `Err` inside fields and tracked changes.

This ADR turns that candidate into a concrete, testable Phase0 for the crate `ooxml-docx`. The
design questions it depends on were measured on 631 real files (129 Apache POI, 502 Open XML SDK)
and are answered in the roadmap (§3). Their decisions are restated here where Phase0 acts on them.

What Word users need first is **template filling**: open a document someone wrote in Word,
change some text, save it, and lose nothing. Word's difficulty for that use case mirrors
Excel's:

- **Cached values.** Field results (TOC, REF, PAGEREF, …) are caches, like `<v>` under `<f>`.
- **Revisions.** Tracked changes put text in wrappers whose edit semantics differ from plain text.
- **Run splits.** Text is split across runs for reasons invisible to the user. 60–76% of
  adjacent run pairs differ only in rsid (roadmap Q1).
- **ID uniqueness.** Real files do not hold the ID uniqueness the specification asks for
  (`w14:paraId` duplicated in 8 of 82 files that carry it, roadmap Q4).

Phase0 again sidesteps the hard cases and refuses them with typed errors.

## Decision

### 1. Scope

In scope:

1. **Open / save** an existing `.docx`, `.docm`, `.dotx` or `.dotm` through `ooxml-core::Package`
   (Layer 1). The main part is located by relationship, not by name (§2). `save` keeps the main
   part's content type unchanged and is atomic (temporary file + rename).
2. **Read:**
   - paragraphs of the main document's body flow in document order (reach: §3.1);
   - each paragraph's text ("all revisions accepted");
   - its runs, each with a `RunContext` and its complex-field state.
3. **Write, (a):** `set_run_text`, which replaces the content of one existing run whose context
   is `Direct` or `Hyperlink`, under the checks of §6.
4. **Write, (b):** `append_paragraph(text)`, which adds `<w:p><w:r>…</w:r></w:p>` as the last block
   of the body, before the body-level `w:sectPr` (or at the end if there is none). It has no
   `pPr`, no `rPr` and no IDs, so it takes the default paragraph style and joins the last section.
   Empty text appends an empty `<w:p/>`.
5. **Gates** that refuse whole documents or individual targets with typed errors (§6).

Out of scope (roadmap phases):

- **`Document::new()` (a document from scratch): deferred** by the owner's decision of
  2026-11-16. Phase0 is the edit path only. Revision 1's §1.5, its open question 2 (template) and
  oracle T7 are withdrawn.
- runs in headers, footers, notes or comments (Phase1);
- cross-run replace (Phase1);
- formatting (Phase1);
- new styles, numbering, tables, images, bookmarks, hyperlinks or sections (Phase2);
- comments, notes, tracked edits, SDT, fields (Phase3);
- drawings, OMML, charts, text boxes (Phase4);
- Strict, encrypted and DTD-carrying packages (refused).

**Phase0 creates no IDs and no parts.** It writes no `w14:paraId`, `w14:textId`, `w:rsid*`,
`w:id`, relationship, content-type entry or new part, so no Layer 3 registry is exercised. The
same move ADR-0001 made with `inlineStr`.

### 2. Parts and parse scope

| Part | How found | Layer 2? | Phase0 use |
|---|---|---|---|
| main document (`word/document.xml` usually) | `/_rels/.rels`, type `…/officeDocument/2006/relationships/officeDocument` | **yes, editable**, parsed once at open | all reads and writes |
| `settings.xml` | main part's rels, type `…/settings` | yes, **read-only** (stays dirty-free `Parsed` → raw copy, ADR-0001 M2) | `w:trackRevisions` (W1), `w:documentProtection` (W3), `w:updateFields` (reported, not written) |
| headers, footers, footnotes, endnotes, comments | main part's rels by type (any name: `poi-Bug60341` uses `footer.xml`); external or dangling targets are skipped | **transient**: parsed with core's parser, scanned for fields, discarded; the part stays `Raw` | field gate (§6 W2); a DTD in any of them refuses the open (O2) |
| everything else (styles, numbering, theme, media, glossary, charts, customXml, …) | — | no | Layer 1 raw passthrough, untouched and unparsed. A missing `styles.xml` is fine (`poi-55733`). |

Revision 1 proposed a separate `quick-xml` event scan for story parts. Using core's parser
instead gives the DTD refusal (O2) and namespace resolution for free, at the cost of building a
tree that is dropped right away; the cost is irrelevant at Phase0 sizes.

The main part's relationships part is derived from its name (`<dir>/_rels/<name>.rels`), as
ADR-0002 §6 does for the workbook. A package whose root relationship has a **Strict** type
(`http://purl.oclc.org/ooxml/…`, refused by core at `Package::from_bytes`), or whose main root
element is in a Strict namespace (refused by `ooxml-docx`), is `Core(StrictNotSupported)`.

### 3. Domain API (as implemented)

```rust
pub struct Document { /* Package, main part name, gates, body index */ }

impl Document {
    pub fn open(path: impl AsRef<Path>) -> Result<Self>;
    pub fn from_bytes(bytes: impl Into<Arc<[u8]>>) -> Result<Self>;
    pub fn save(&self, path: impl AsRef<Path>) -> Result<()>;   // atomic, ADR-0001 R2-M4
    pub fn to_bytes(&self) -> Result<Vec<u8>>;
    pub fn package(&self) -> &Package;                          // read-only, e.g. read_part
    pub fn main_part_name(&self) -> &str;

    pub fn write_gate(&self) -> Result<()>;                     // first of W1, W3, W2
    pub fn write_gates(&self) -> Vec<Error>;                    // all that apply
    pub fn update_fields_on_open(&self) -> Option<bool>;        // reported only
    pub fn write_protection(&self) -> Option<WriteProtection>;  // reported only (rev 3, m3)
    pub fn is_marked_final(&self) -> bool;                      // reported only (rev 3, m3)

    pub fn paragraph_count(&self) -> usize;
    pub fn paragraphs(&self) -> impl Iterator<Item = Paragraph<'_>>;
    pub fn paragraph(&self, index: usize) -> Option<Paragraph<'_>>;

    pub fn set_run_text(&mut self, paragraph: usize, run: usize, text: &str) -> Result<()>;
    pub fn append_paragraph(&mut self, text: &str) -> Result<usize>;
}

impl Paragraph<'_> {
    pub fn index(&self) -> usize;
    pub fn raw(&self) -> &RawNode;
    pub fn text(&self) -> String;            // runs() minus w:del / w:moveFrom runs
    pub fn runs(&self) -> impl Iterator<Item = Run<'_>>;
    pub fn run(&self, index: usize) -> Option<Run<'_>>;
}

impl Run<'_> {
    pub fn index(&self) -> usize;
    pub fn raw(&self) -> &RawNode;
    pub fn text(&self) -> String;
    pub fn context(&self) -> RunContext;
    pub fn in_field(&self) -> bool;          // complex field: instruction, result, or a fldChar run
    pub fn check_settable(&self) -> Result<()>;   // R1–R4, without the document gates
}

#[non_exhaustive]
pub enum RunContext {
    Direct, Hyperlink,                                  // can be settable
    Inserted, Deleted, MovedFrom, MovedTo,              // R2
    SmartTag, ContentControl, SimpleField, CustomXml, Bidi, Ruby,   // R3 (SimpleField: R1)
}
```

Revision 1's `paragraph_mut(i).set_run_text(r, text)` became `Document::set_run_text(p, r, text)`:
the mutable paragraph handle carried nothing but the index. Revision 1's `RunContext::FieldResult`
is replaced by `Run::in_field()`, because a field can enclose runs of any context (a hyperlink
inside a field result) and its state is not a wrapper.

Indices are stable for the lifetime of a `Document`: `set_run_text` never adds or removes runs or
paragraphs, and `append_paragraph` adds after every existing paragraph (the body walk is re-run
after an append).

#### 3.1 Reach (review m4)

| Element | `paragraphs()` | `runs()` | Context of the runs inside |
|---|---|---|---|
| `w:body`, `w:tbl`/`w:tr`/`w:tc` (nested tables too) | descends | — | (transparent) |
| `w:sdt`/`w:sdtContent` at block, row or cell level | descends | — | `ContentControl` |
| `w:customXml` at block, row or cell level | descends | — | `CustomXml` |
| `w:p` | yields | its flow | — |
| `w:r` | — | yields | — |
| `w:hyperlink` (also nested) | — | descends | `Hyperlink` |
| `w:ins` / `w:del` / `w:moveFrom` / `w:moveTo` | — | descends (deleted runs included) | `Inserted` / `Deleted` / `MovedFrom` / `MovedTo` |
| `w:smartTag` | — | descends | `SmartTag` |
| `w:sdt`/`w:sdtContent` in a paragraph | — | descends | `ContentControl` |
| `w:customXml` in a paragraph | — | descends | `CustomXml` |
| `w:fldSimple` | — | descends | `SimpleField` |
| `w:dir`, `w:bdo` | — | descends | `Bidi` |
| `w:r/w:ruby/w:rubyBase` | — | descends (after the enclosing run); `w:rt` is not read | `Ruby` |
| `w:drawing`, `w:pict`, `w:object`, `mc:AlternateContent`, `m:oMath*`, `w:txbxContent` | no | no | not body flow (text boxes are separate stories) |

When several wrappers enclose a run, the **outermost wrapper other than `w:hyperlink`** decides
its context, paragraph-level wrappers first; a run with only hyperlinks around it is
`Hyperlink`, with none `Direct`. Every variant is returned by some input (unit test `contexts`).
`Paragraph::text()` concatenates `Run::text()` of every run except those with a `w:del` or
`w:moveFrom` wrapper, so field results are included and instructions (`w:instrText`) are not.

`Run::text()` reads: `w:t` and `w:delText` as written; `w:tab` → `\t`; `w:br` and `w:cr` →
`\n`; `w:noBreakHyphen` → U+2011; `w:softHyphen` → U+00AD; everything else adds nothing.

#### 3.2 `set_run_text` semantics (review m1)

All writes go through ADR-0002 primitives, so the dirty-flag duties are theirs.

1. **Map `text` to run content:**

   | Input | Output |
   |---|---|
   | maximal substring without `\t`, `\n`, `\r` | `w:t` |
   | `\t` | `<w:tab/>` |
   | `\n`, `\r\n`, a lone `\r` | `<w:br/>` (a literal CR in `w:t` would read back as LF, XML 1.0 §2.11) |
   | U+0000–U+0008, U+000B, U+000C, U+000E–U+001F, U+FFFE, U+FFFF | `Err(InvalidXmlChar)`, nothing changed |
   | U+2011, U+00AD and every other character | literal in `w:t` |

2. **Content** of a run is every child element other than `w:rPr`. Whitespace-only text nodes
   (pretty printing, `poi-table-alignment`, `sdk-HelloO14`) are not content: they are ignored by
   the checks and kept by the edit. Comments, PIs and non-whitespace text directly in `w:r` are
   refused (R4).
3. **No-op:** if the run's content is already exactly the mapped sequence (only `w:t`, `w:tab`,
   attribute-less `w:br`) **and no `w:t` of the run needs `xml:space="preserve"` (step 6) without
   carrying it itself**, nothing changes and the output is byte-identical. Revision 3 (review
   round 2, m6) added the second condition: `<w:t> a</w:t>` written as `" a"` would otherwise stay
   as is, and Word drops its leading space *(u)*, so the caller's explicit text would not be what
   is shown. Such a run is rewritten (the fast path adds the attribute).
4. **Fast path:** if the content is one `w:t` and the mapping is one `w:t`, `set_text` on that
   `w:t`; its open tag stays verbatim unless `xml:space` must be added.
5. **Otherwise:** remove every content child (`remove_at`, last first), then `insert` the mapped
   children at the end of the content slot. `w:rPr` is never touched. A `w:cr`,
   `w:noBreakHyphen`, `w:softHyphen` or `w:lastRenderedPageBreak` in the run is replaced by the
   mapping (the owner accepted dropping `w:lastRenderedPageBreak`; §5 lists it with the other
   stale caches).
6. **`xml:space`:** the `set_text` primitive adds `xml:space="preserve"` for leading or trailing
   whitespace (ADR-0002 §4.2). The domain layer also adds it for two consecutive spaces. An
   existing `xml:space` is never removed. Producers' edge whitespace without `preserve` (296 such
   `w:t` in POI) is left as written in runs that are not edited; writing such a run's own text
   adds `preserve` (step 3).
7. **Own text:** `set_run_text(p, r, &run.text())` is a no-op for runs in the canonical form of
   step 3. Otherwise it rewrites the markup (`w:cr` → `w:br`, `w:noBreakHyphen` → literal
   U+2011, …) and `Run::text()` reads the same string back. T2 asserts both.
8. Attributes of the run and paragraph (`w:rsid*`, `w14:paraId`, `w14:textId`) are untouched
   (owner decision: keep `w14:textId` pending the Word check, §7).

**`append_paragraph`:** `body.insert::<w:p>()`. The body's slots are [35 block QNames, `sectPr`],
so the default `insert` lands before the body-level `sectPr`; a `sectPr` inside `pPr` (section
break) is a different element and not a slot anchor (`poi-Headers`). Then `p.insert::<w:r>()`
and the content as in step 1. The new nodes have no original span, so ADR-0001 T2's "insertion
point" rule applies.

**Last section in the last paragraph (revision 3, review round 2 m2).** When the body has no
`w:sectPr` of its own and its last block is a paragraph whose `w:pPr` holds a `w:sectPr` (or a
content control or custom-XML block that ends with one), that paragraph carries the last section's
properties. A paragraph inserted after it would belong to a new final section with default page
size, orientation and margins *(u)*: a landscape document would end with a portrait page. The
goal is to keep the existing section, and Phase0 has two ways to append into it, both rejected:
inserting before that paragraph changes the order of the text, and moving the `w:sectPr` into the
new paragraph edits an existing paragraph, which breaks append's "pure insertion" guarantee (T2).
So `append_paragraph` refuses with **A2** `SectionBreakAtEnd` and the document is unchanged.
Appending stays allowed when a body-level `w:sectPr` exists (the paragraph then ends an earlier
section), when a table follows the paragraph, and when there is no `w:sectPr` at all
(`poi-55733`). Phase1 may move the `w:sectPr` as an explicit, documented edit. There are no
occurrences in the corpora.

#### 3.3 Order of checks

`set_run_text`: document gates (W1, W3, W2) → index range → R1 → R2 → R3 → R4 → T1 → mutation.
`append_paragraph`: gates → T1 → `NoBody` → unterminated field at the end of the body
(`InsideField`) → A2 → A1 (core) → mutation. All refusals precede the first mutation.

#### 3.4 Navigation over the merged core API (review M2)

- **Reads** walk the raw tree: `RawNode::indexed_elements()` (public, untyped, with item indices)
  and QName tests against the generated markers' `QNAME` constants or, for context elements that
  have no generated view (`w:sdt`, `w:sdtContent`, `w:smartTag`, `w:customXml`, `w:dir`, `w:bdo`,
  `w:ruby`, `w:rubyBase`), hand-written `QName` constants. The walk runs once at open, with an
  explicit stack (no recursion), and records one frame (parent frame, child index, wrapper) per
  element it descends into; each paragraph and run stores its frame and field state. Paths and
  wrapper lists are rebuilt from the frames on demand, so the index is linear in the number of
  visited elements (revision 2 stored a full path per run: quadratic in the nesting depth, review
  round 2 N1).
  This is the "untyped traversal" the review asked for; core already has it.
- **Writes** start from `XmlDocument::lens_mut::<wml::Document>()` and follow a stored path with
  `LensMut::into_child_at::<C>(index)`, which checks the QName at that index (the "checked cast")
  and keeps the full lifetime. The only paths followed are `document → body → (tbl → tr → tc)* →
  p → hyperlink* → r`, plus `body → p` for append. All of these are allowlisted entries, so the
  `ChildOf` impls exist whether an entry is `[write]` or `[read]` (ADR-0002 post-acceptance note).
  A settable run never has another ancestor (R3), so no write ever needs to pass through an
  unlisted element.
- **Core addition:** none for M2. `Package::xml_part` takes `&mut self` (it may parse), so
  `Document::paragraphs(&self)` needed `Package::parsed_part(&self, name)`, an additive accessor
  for a tree already parsed: PR #9 (`core/package-parsed-part`, merged into main). It is
  useful to the Excel and PowerPoint crates in the same way.

### 4. Phase0 WML allowlist and overrides

`codegen/allowlist/wml.toml` (the layout chosen by the foundation implementation):

```toml
[write]
names = [
    "w:CT_Body/w:body", "w:CT_P/w:p", "w:CT_R/w:r",
    "w:CT_Text/w:t", "w:CT_Empty/w:tab", "w:CT_Br/w:br",
]
[read]
names = [
    "w:CT_Document/w:document", "w:CT_SectPr/w:sectPr", "w:CT_PPr/w:pPr", "w:CT_RPr/w:rPr",
    "w:CT_Hyperlink/w:hyperlink", "w:CT_Tbl/w:tbl", "w:CT_Row/w:tr", "w:CT_Tc/w:tc",
    "w:CT_Empty/w:cr", "w:CT_Empty/w:noBreakHyphen", "w:CT_Empty/w:softHyphen",
    "w:CT_Empty/w:lastRenderedPageBreak",
    "w:CT_FldChar/w:fldChar", "w:CT_Text/w:instrText", "w:CT_Text/w:delInstrText",
    "w:CT_SimpleField/w:fldSimple",
    "w:CT_RunTrackChange/w:ins", "w:CT_RunTrackChange/w:del",
    "w:CT_RunTrackChange/w:moveFrom", "w:CT_RunTrackChange/w:moveTo",
    "w:CT_Text/w:delText",
    "w:CT_Settings/w:settings", "w:CT_OnOff/w:trackRevisions", "w:CT_OnOff/w:updateFields",
    "w:CT_DocProtect/w:documentProtection",
]
```

Compared with revision 1: `w:delInstrText` (W2 fail-closed rule) and `w:documentProtection` (W3)
are added. Abstract bases (`w:CT_Text/` with `xml:space`, `w:CT_OnOff/` with `w:val`) are merged
by the generator's inheritance rule (ADR-0002 §3). All six write entries lower to slots without
ambiguity: `w:body` = [blocks, `sectPr`], `w:p` = [`pPr`, content], `w:r` = [`rPr`, content].
None of the 8 ECMA-order disagreements or the 12 unsupported models is involved, and no entry is
`attrs_incomplete` (`codegen/report.txt`).

**`ChildOf` is not a write permission (correction of revision 1).** `ChildOf` impls are
generated between *any* two allowlisted entries (e.g. `ChildOf<Run> for FieldChar`,
`ChildOf<Body> for InsertedRun`), so the compiler does not limit what the domain layer inserts.
What Phase0 writes is limited by `ooxml-docx`'s code (it inserts only `w:p`, `w:r`, `w:t`,
`w:tab`, `w:br`) and by the tests (T2, T6), not by the type system.

**Override (one), in `codegen/overrides.toml`:**

```toml
[[override]]
op = "set_default"
on = "w:CT_OnOff/"
attribute = "w:val"
value = "true"
```

With it, `TrackRevisionsView::val()` returns `Ok(true)` for `<w:trackRevisions/>` (`poi-delins`).
Without it the W1 test on `poi-delins` and the empty-element case of the W1 unit test fail
(review m6).

**Recorded for Phase1, not Phase0:** an `add_child` for `w:smartTag` under `w:CT_P/w:p`. It is an
ECMA Transitional element absent from the SDK's `CT_P`, seen 424 times in 25 files. Until then
smart tags are unlisted children, preserved, and their runs are read through the raw walk.

### 5. Cross-part policy and stale caches (Layer 3 analogue)

Phase0 needs no registry, by construction:

| Cross-part concern | Phase0 policy |
|---|---|
| Style IDs (`styles.xml`) | Not read. Appended paragraphs have no `pStyle`, so they use the default paragraph style. Edited runs keep their `rPr`. |
| Numbering (`numbering.xml`) | Not read. Appended paragraphs have no `numPr`. |
| Annotation IDs (bookmarks, comments, revisions) | None created. Edits never touch range markers: they are siblings of runs, and only run content changes. |
| `w14:paraId` / `w14:textId` / rsids | None created. Existing ones are left as they are, duplicates included (roadmap Q4). |
| Relationships, content types, new parts | None. `Package` writes back exactly the original part set (ADR-0001 Layer 1, T3). |
| Field results anywhere | Guarded by W2. Never recomputed, never marked dirty. |
| Package properties (`docProps/core.xml`, `docProps/app.xml`, `docProps/custom.xml`) | Never written. `docProps/custom.xml` is read (transiently, raw copy on save) for W2 and Mark as Final. |
| `settings.xml` | Read-only. `updateFields` is **not** written in Phase0 (opt-in setter in Phase1, roadmap Q2). |

**Caches Phase0 leaves stale (review m8).** An edit can make these out of date; Phase0 neither
updates nor removes them (except the one in the edited run):

| Cache | Where | Who recomputes it |
|---|---|---|
| `w:lastRenderedPageBreak` in the edited run | `document.xml` | dropped by `set_run_text` (§3.2 step 5) |
| `w:lastRenderedPageBreak` in every other run (POI: 202 in 19 files; SDK: 820 in 101) | `document.xml` | Word, at next layout and save |
| `w14:textId` of an edited paragraph | `document.xml` | Word on save *(u)*; kept pending the owner's check |
| rsids (`w:rsidR`, `w:rsidRPr`, …) | `document.xml`, `settings.xml` (`w:rsids`) | Word on save; they record editing sessions, and Phase0's edit is not one |
| Layout-dependent field results (`PAGE`, `NUMPAGES`, `SECTIONPAGES`) | any story | Word at layout (W2 treats them as safe) |
| Document statistics `Pages`, `Words`, `Characters`, `CharactersWithSpaces`, `Lines`, `Paragraphs` | `docProps/app.xml` | Word on save. W2 refuses `NUMWORDS`/`NUMCHARS` and `INFO`/`DOCPROPERTY` of these statistics, whose results would contradict the edit |
| `dcterms:modified`, `cp:lastModifiedBy`, `cp:revision` | `docProps/core.xml` | Word on save. Left stale on purpose: the safe fields `SAVEDATE`, `LASTSAVEDBY`, `REVNUM` show these values, so field results and properties stay consistent with each other (both describe the last save by Word) |
| Custom property values linked to content (`linkTarget`, a bookmark) | `docProps/custom.xml` | Word, on save or when the bookmark text changes *(u)*. `DOCPROPERTY` of such a property is refused by W2 |
| `w:proofState` (`spelling="clean"`, `grammar="clean"`) *(u)* | `settings.xml` | Word, when it re-proofs |
| Thumbnail (`docProps/thumbnail.*`), if present | package | Word on save |

Phase2's registries (roadmap §5) must keep ADR-0001's invariant: existing IDs are append-only,
never renumbered or "repaired".

### 6. Errors

`ooxml_docx::Error` is `#[non_exhaustive]`; core errors are wrapped as `Error::Core(..)`. Each
row is the variant, when it fires, and a fixture or unit test that triggers it. Errors fire before
any mutation, so a failed call leaves the tree untouched. At open:

| # | Variant | Condition | Test |
|---|---|---|---|
| O1 | `Core(EncryptedPackage)` | CFB magic `D0 CF 11 E0` | `poi-bug53475-password-is-pass` |
| O2 | `Core(DtdNotAllowed)` | core's parser sees a `DocType` event in the main part, `settings.xml` or a story part. This is the parser event, anywhere in the prolog, not a byte search of the first 4 kB (review m9, resolved in core) | `poi-ExternalEntityInText`; unit test with the DOCTYPE after a 10 kB comment, and one in a header |
| O3 | `Core(StrictNotSupported)` | Strict package relationship type, or Strict namespace on the main root | `sdk-strict-CommentRangeStart`; unit test (Strict root, Transitional relationship) |
| O4 | `MainPartMissing` | no `officeDocument` relationship, or its target is absent | unit tests |
| O5 | `Core(TooDeep { limit, position })` | the main part, `settings.xml` or a scanned story part nests elements deeper than core's `ParseOptions::max_depth` (default 1024; root = depth 1). Refused while parsing, before the tree grows (revision 3, review round 2 N1). Revision 2 accepted any depth: 5.7 kB of nested tables cost 4.7 GB at open and 20 kB aborted the process with a stack overflow. The deepest real part measured is 33 (POI `bug59058`), median 9 over 556 files | `adversarial.rs`: 10,000- and 40,000-deep nested tables are refused quickly; a document just under the limit opens and edits |
| — | `Core(WrongElement)` | the main part's root is not `w:document` | unit test |

Document-level write gates. Computed once at open. Reads still work; every write method returns
the first gate (W1, W3, W2 order); `write_gates()` lists all:

| # | Variant | Condition | Test |
|---|---|---|---|
| W1 | `TrackRevisionsOn` | **any** `settings/w:trackRevisions` is on (a duplicate is a schema violation; a later "on" is not hidden by an earlier "off", revision 3): `val` absent, `true`, `1`, `on`, **or not a valid `ST_OnOff` value (fail closed)** | `poi-delins`; unit test of all eight forms (absent element, empty element, `true`/`1`/`on`/`false`/`0`/`off`, invalid) |
| W3 | `DocumentProtected { edit }` | **any** `settings/w:documentProtection` (duplicates: the first restricted one is reported, revision 3) with `enforcement` on (an invalid value counts as on) and `edit` other than `none` (absent `edit` = no restriction; an unknown value counts as restricted). Covers `readOnly`, `comments`, `forms`, `trackedChanges`. `w:formatting`-only protection allows edits and passes. | `poi-documentProtection_readonly_no_password` (added); unit test of the variants |
| W2 | `StaleFieldRisk { codes }` | a story part (main, headers, footers, footnotes, endnotes, comments) has a field whose key is content-dependent or **not classifiable**, or **field markup that cannot be followed** (`<malformed field>`, revision 3) | `sdk-TOC2` (`PAGEREF`, `TOC`), `sdk-complex2010` (`=`, `SEQ`, `TC`); unit tests |

**W2 field scan (review M1).** Per story part, in document order, with a stack:

- `w:fldChar begin` pushes a field; `separate` marks the top field's instruction complete; `end`
  pops and classifies it.
- `w:instrText` is appended to the **top** field's instruction only while that field has not seen
  its own `separate`. Word writes the result of a field nested in an instruction as `w:instrText`
  after the nested field's `separate`, so it never leaks into the outer code (unit test with
  `{ PAGE { DATE } }` whose nested result reads `TOC`: safe).
- **Every level is classified**: `{ IF { MERGEFIELD Gender } … }` → `["IF"]`;
  `{ = { PAGE } - 1 }` → `["="]`; `{ HYPERLINK "{ REF url }" }` → `["REF"]`.
- Fields still open at the end of the part are classified too.
- A field whose own instruction contains `w:delInstrText` (a tracked edit of the code) is
  reported as `<revised instruction>` and refused: old and new code are interleaved and cannot be
  classified (fail closed). This is why `poi-delins` would be refused by W2 even with track
  changes off.
- `w:fldSimple` is classified by its `w:instr`.
- The code is `=` for a formula, else the first token up to whitespace or `\`, upper-cased. An
  empty instruction gives the empty code, which is refused.
- The **key** (`fields::field_key`, revision 3) is the code, except for `INFO` and `DOCPROPERTY`,
  whose key carries the first argument, unquoted and upper-cased (`INFO NUMWORDS`,
  `DOCPROPERTY CLIENT NAME`; the bare code if there is no literal argument, e.g. a nested field),
  and `BARCODE` with `\b` (`BARCODE \b`). A calculated text form field (`w:ffData/w:textInput/
  w:type w:val="calculated"` on its `begin`) has the key `FORMTEXT (calculated)`. Keys are what
  `StaleFieldRisk { codes }` reports.
- **Malformed markup fails closed (revision 3, review round 2 m1).** `<malformed field>` is
  reported, and the document refused, for a `w:fldChar` whose `w:fldCharType` is missing, not
  `w:`-qualified, or not exactly `begin`/`separate`/`end`; a `separate` or `end` with no open
  field; a `w:instrText`/`w:delInstrText` with no open field; and a `w:fldChar` inside an
  `mc:AlternateContent` that sits within a paragraph (directly or in a run) with no `w:p` in
  between. In the last case both branches would be counted by the scan, and the body walk, which
  does not enter `mc:AlternateContent`, would see the field's result runs as outside any field. A
  complete field in a text box's own paragraph inside `mc:AlternateContent` is classified
  normally, and so are fields in block-level `mc:AlternateContent`. Word does not write any of
  these forms (schema violations); none occurs in the fixtures.

**Classification (owner decision 2026-11-16: accepted).** A code is **safe** when its cached
result depends only on things Phase0 cannot change, or on pagination, which Word recomputes at
every layout (review m5's restatement of revision 1's criterion):

- pagination: `PAGE`, `NUMPAGES`, `SECTIONPAGES`, `SECTION`;
- date and time: `DATE`, `TIME`, `CREATEDATE`, `SAVEDATE`, `PRINTDATE`, `EDITTIME`;
- document properties and environment: `AUTHOR`, `TITLE`, `SUBJECT`, `KEYWORDS`, `COMMENTS`,
  `FILENAME`, `LASTSAVEDBY`, `TEMPLATE`, `DOCVARIABLE`, `USERNAME`, `USERINITIALS`,
  `USERADDRESS`, `REVNUM`;
- by argument (revision 3): `INFO X` is safe when `X` is itself a safe plain code (`INFO
  NumPages`, `INFO Author`); `DOCPROPERTY X` is safe unless `X` is a document statistic
  (`Words`, `Characters`, `CharactersWithSpaces`, `Lines`, `Paragraphs`, `Bytes`, and the `INFO`
  spellings `NumWords`, `NumChars`, `FileSize`; `Pages` is pagination and safe) or a custom
  property linked to content (`linkTarget` in `docProps/custom.xml`). `INFO` or `DOCPROPERTY`
  without a literal argument is refused;
- links and external content: `HYPERLINK`, `INCLUDEPICTURE`, `INCLUDETEXT`, `LINK`, `EMBED`;
- mail merge: `MERGEFIELD`, `MERGEREC`, `MERGESEQ`, `NEXT`, `NEXTIF`, `SKIPIF`, `GREETINGLINE`,
  `ADDRESSBLOCK`;
- form fields and user input: `FORMTEXT`, `FORMCHECKBOX`, `FORMDROPDOWN`, `ASK`, `FILLIN`, `SET`;
- self-contained display: `EQ`, `SYMBOL`, `MACROBUTTON`, `GOTOBUTTON`, `ADVANCE`, `PRIVATE`,
  `BARCODE` (without `\b`), `DISPLAYBARCODE`.

**Audit of the safe codes for argument-dependent results (revision 3, review round 2 N2).**
Revision 2 classified by the first token only, so `INFO NumWords` and `DOCPROPERTY Words` passed
although they return the same counts as the refused `NUMWORDS`. Every safe code was re-checked for
arguments or switches that make its result depend on the document's content:

| Code | Argument-dependent case | Decision |
|---|---|---|
| `INFO` | the argument is another field's name | classified as that field (`INFO NumWords` refused) |
| `DOCPROPERTY` | statistics; custom property linked to a bookmark | refused (the list above; `linkTarget` read from `docProps/custom.xml`) |
| `FILESIZE` | none, but the saved size changes with every Phase0 save | **moved to content-dependent**. `SAVEDATE` stays safe: it agrees with `docProps/core.xml`, which Phase0 also leaves as Word last saved it (§5); `FILESIZE` has no such counterpart and would show a size the file no longer has |
| `BARCODE` | `\b`: the argument is a bookmark whose text is encoded | `BARCODE \b` refused |
| `FORMTEXT` | calculated type: the result is an expression over other form fields' bookmarks | `FORMTEXT (calculated)` refused |
| `AUTHOR`, `TITLE`, `SUBJECT`, `KEYWORDS`, `COMMENTS` | an argument *sets* the property | safe: the value comes from the instruction, not from the content |
| `INCLUDETEXT` | `INCLUDETEXT "file" bookmark` | safe: the source is another file. Including the document itself is exotic and not detected *(u)* |
| `HYPERLINK`, `GOTOBUTTON`, `MACROBUTTON` | `\l` / jump target is a bookmark | safe: the bookmark is a target, the displayed text is the field's own |
| `NEXTIF`, `SKIPIF`, `ASK`, `SET` | comparisons and bookmark assignment | safe: merge data or user input; `SET`/`ASK` assign, and the fields that read the bookmark (`REF`) are refused |
| `EQ`, `DISPLAYBARCODE` | nested fields as arguments | each nested field is classified on its own |
| others (`PAGE`…, `DATE`…, `DOCVARIABLE`, mail merge, `SYMBOL`, `ADVANCE`, `PRIVATE`) | none | safe |

**Refused**: every other code. Known content-dependent ones: `TOC`, `REF`, `PAGEREF`, `NOTEREF`,
`SEQ`, `STYLEREF`, `=`, `IF`, `NUMWORDS`, `NUMCHARS`, `FILESIZE`, `INDEX`, `TOA`, `TC`, `XE`, `LISTNUM`,
`CITATION`, `BIBLIOGRAPHY`, `QUOTE`, `RD`, `COMPARE`, `AUTONUM`, `AUTONUMLGL`, `AUTONUMOUT`; and
unknown codes (POI's `M:USERCONTENT`). The table is `ooxml_docx::fields::{SAFE,
CONTENT_DEPENDENT, STATISTICS}` with `is_safe`. Changes from revision 1's list: `AUTONUM*` moved to refused (their result
counts paragraphs, like `SEQ`, so they fail the criterion; no occurrence in the corpora), and
`NUMBERING` dropped (not a Word field code).

Per target:

| # | Variant | Condition | Test |
|---|---|---|---|
| R1 | `InsideField` | `Run::in_field()` (between a `begin` and its `end` at any depth, instruction or result, or holding a `w:fldChar`), or a `w:fldSimple` wrapper. The depth is **one counter over the whole body in document order**, across paragraphs, cells and wrappers (review m2); an unterminated `begin` keeps the rest of the body inside (fail closed), which also refuses `append_paragraph` | `poi-FieldCodes`, `sdk-Field-more-field` (8 text runs), `poi-FldSimple`; unit test: a result spanning three paragraphs and a hyperlink in a table cell |
| R2 | `InsideRevision { context }` | a `w:ins`, `w:del`, `w:moveFrom` or `w:moveTo` wrapper | `poi-delins` with `trackRevisions` removed; unit test |
| R3 | `UnsupportedContext { ancestor }` | any wrapper other than `w:hyperlink` (§3.1): `w:sdt` (data-bound content is overwritten by Word from the custom XML store, roadmap row 16), `w:smartTag`, `w:customXml`, `w:dir`, `w:bdo`, `w:ruby` | `poi-smarttag-snippet` (8 runs), `sdk-coverpage1` (all runs); unit test for each |
| R4 | `RunNotTextOnly { child }` | content other than `w:t`, `w:tab`, `w:br` (no `type` or `textWrapping`, no `clear` or `none`), `w:cr`, `w:noBreakHyphen`, `w:softHyphen`, `w:lastRenderedPageBreak`; or a comment, PI or non-whitespace text in `w:r`. Examples: `w:drawing`, `w:sym`, `w:footnoteReference`, `w:fldChar`, `w:object`, `w:ruby`, `mc:AlternateContent`, page `w:br` | `poi-shapes-with-text` (`mc:AlternateContent`); unit test |
| T1 | `InvalidXmlChar { ch }` | text contains an XML-1.0-forbidden character | unit test |
| A1 | `Core(UnresolvedSibling)` | `append_paragraph` into a `w:body` with an `mc:AlternateContent` child | `sdk-HelloO14` |
| A2 | `SectionBreakAtEnd` | `append_paragraph` when the body has no `w:sectPr` of its own and ends with a paragraph carrying `w:pPr/w:sectPr` (§3.2, revision 3) | `adversarial.rs` (P01, also inside a content control) |
| — | `NoBody`, `NoSuchParagraph`, `NoSuchRun` | no `w:body` to append to; index out of range | unit tests |

**Reported, not gated (revision 3, review round 2 m3).**

- `settings/w:writeProtection` ("password to modify", "read-only recommended") locks saving over
  the file in Word, not editing its content. `Document::write_protection()` reports it
  (`recommended`, `has_password`; duplicates merged).
- Word's Mark as Final (`_MarkAsFinal` = true in `docProps/custom.xml`) is the author's
  suggestion, which Word lifts with one click. `Document::is_marked_final()` reports it.

Callers that want to treat either as a refusal can do so. Revision 3 does not gate them because
neither restricts editing in Word itself. SDK has one `write_protected.docx` (recommended); there
is no Mark as Final in the corpora.

R1 is checked before R2–R4, so a run inside a field inside a smart tag reports `InsideField`.
Text-box runs are not body flow, so revision 1's R3 fixture `poi-shapes-with-text` now shows up
as R4 on the runs that hold the shapes.

**Measured effect** (`spikes/docx/phase0_gate.py` revision 2; the Rust crate reproduces the
fixture numbers exactly in `tests/fixtures.rs`; "text run" = a run with a `w:t` in body flow):

| Corpus | Writable files | Refused files | Settable / text runs in writable files |
|---|---|---|---|
| fixtures (26) | 19 of 23 openable | W1 1, W3 1, W2 2; O1–O3 1 each | 3,418 / 3,455 (308 in `w:hyperlink`) |
| POI (129) | 102 | 13 not a zip, 1 corrupt, 2 encrypted, 1 DTD, 5 W1, 4 W3, 1 W2 | 27,763 / 28,067 (98.9%) |
| SDK non-Strict (444) | 404 | 30 W2, 6 W1, 2 encrypted, 2 Strict | 23,448 / 25,556 (91.8%) |

Revision 1's POI line ("15 are not zips or are corrupt") was wrong: 13 are not zips and 1 is
corrupt (review m3). The four W3 files in POI were writable under revision 1.

### 7. Test oracles (as implemented in `crates/ooxml-docx/tests/`)

Fixtures are `reference/fixtures/docx/*.docx`: 23 open as packages, 3 are O1–O3; 19 are writable.

- **T1 (prolog/epilog)** (`oracles.rs`): for **every** XML part (`.xml`, `.rels`) of the 23
  openable fixtures, a superset of what ooxml-docx parses: parse and serialize with zero edits,
  then compare byte-for-byte. Covers UTF-8 BOMs (`poi-Bug60341`, `poi-55733`, `sdk-HelloO14`)
  and pretty printing (`poi-table-alignment`).
- **T1b (parser/renderer)**: same parts, force every node dirty, render, re-parse, compare
  structurally.
- **T2 (locality)**:
  - every run with `check_settable() == Ok` in the writable fixtures (3,423: the 3,418 text runs
    plus 5 tab/break-only runs) is edited twice: with its own text, then with
    `"a  b\t {own} "`. The `a  b` piece has an inner double space and no edge space, so only the
    domain's own `preserve` rule covers it (revision 2's `" {own}\tx  y "` always reached core's
    edge rule, review round 2 m4). Runs are edited in document order in one document; each diff
    of the main part must lie within that run's original span `start..end()`, shifted by the
    length change of the earlier edits, and must leave the run's start tag and everything up to
    the end of its `w:rPr` byte-identical (revision 3). For runs already in canonical form
    (§3.2 step 3) the own-text edit must be byte-identical. `Run::text()` must read back the
    string written, and after the last edit the saved package is reopened and every edited run
    must read back its last text.
  - Mutation check (revision 3): dropping `w:rPr` in `write_pieces`, or applying the
    double-space `preserve` rule only to edge spaces, now fails T2 on the first fixture.
  - `append_paragraph` on the 18 writable fixtures without `mc:AlternateContent` in the body is a
    pure insertion of the expected bytes immediately before the body-level `<w:sectPr`, or
    before `</w:body>` when there is none (`poi-55733`).
- **T3 (Layer 1)**: open + save without edits gives identical (name, method, CRC-32, compressed
  payload) for every entry of every openable fixture, directory entries and stored entries
  included; after an append only the main part differs, and it keeps its compression method.
- **T4 (refusals)** (`fixtures.rs`, `gates.rs`): every row of §6 returns exactly that variant.
  On the four gated fixtures reads still work, both write methods return the gate error, and the
  saved package is entry-for-entry identical to the input.
- **T5 (independent read-back)** (`readback.rs`): `docx-rs = "=0.4.22"`, `default-features = false`,
  dev-dependency only.
  - **Procedure** (with review m7's corrections). Collect every `{"type":"text"}` value under
    `document.children` of `read_docx(..).json()` (not `sectionProperty`, which holds header and
    footer text). For up to four settable runs per writable fixture (first, quarter, middle, last;
    41 edits), write a unique marker and assert: it occurs exactly once; the texts before and after
    it equal the input's; the input's texts at its position are exactly the run's original `w:t`
    pieces. For append: the appended text is the last body text and all earlier texts are
    unchanged.
  - docx-rs ignores `xml:space` (its reader does not consult `preserve`), so T2 also checks that
    every `w:t` an edit wrote with edge or double spaces carries `xml:space="preserve"`.
  - **Why docx-rs.** An independent codebase (MIT, its own XML reader and model). On 490 real files
    it read 469 and panicked on 2 (`Strict01.docx` and a fuzz case, neither a Phase0 fixture).
    With order-preserving matching (review m7) 459 of 469 matched an ElementTree ground truth
    completely; the misses are mostly text boxes, which are outside Phase0.
  - **Why not the others** (unchanged from revision 1): python-docx misses `ins`, `fldSimple`,
    `sdt` and `smartTag` runs and needs Python in CI; docx-rust recovered 0 of 36 `w:ins` texts and
    fails on 9 valid packages; `ooxmlsdk` is deferred to Phase2 for compile cost.
- **T6 (schema order)**: after each T2 edit, the edited run's element children are in
  `w:CT_R` slot order under ADR-0002 §4.1's greedy assignment; after each append, so are the
  body's. Core's slot insertion makes this hold by construction, so T6 is a regression test of
  core and cannot detect a domain error (review round 2 m4).
- **Corpus cross-check** (a throwaway scan, not committed): the crate was run over both corpora
  (POI 129, SDK 444), editing up to 300 settable runs per writable file, appending, saving and
  reopening. No panic, no error. File-level outcomes equal the spike's, except that ooxml-rs
  opens POI's fuzz case `clusterfuzz-…-6733884933668864.docx`, which Python's `zipfile` rejects
  for a bad CRC in a part ooxml-rs never decompresses (raw copy). Settable runs are identical
  (POI 27,763, SDK 23,448); the text-run totals differ by one run per corpus that the spike's
  `iter()` reaches and the body walk does not (neither counts it as settable). POI's
  `deep-table-cell.docx` (5,000 nested tables, 15,000+ element levels) is refused at open since
  revision 3 (O5). Revision 2 opened it with 1.2 GB RSS and took 3–4 s per edit; the review
  showed the open cost, not only the edit cost, was quadratic, and that 40,000 levels overflowed
  the stack. Under the 1024 limit, path-based `LensMut` descent costs at most 1024 steps per
  edit, the body index is linear, and every walk is iterative or bounded by 1024 frames.
- **Manual (explicit gap, as in ADR-0001)**: there is no Word or LibreOffice in this sandbox.
  `cargo run -p ooxml-docx --example manual_check -- <outdir>` writes ten named outputs from
  fixtures (table cell, hyperlink run, run with `w:lastRenderedPageBreak`, paragraph with
  `w14:textId` in a commented document, append without `sectPr`, append to a multi-section
  document, whitespace/tab/break mapping, document with fields, headers/footers with fields,
  notes) and `CHECKLIST.md`. The owner opens them in current Microsoft 365 Word and checks for no
  repair dialog, the expected text, and unchanged fields, comments and notes. The checklist asks
  the `w14:textId` and `w:lastRenderedPageBreak` questions explicitly.

## Alternatives considered

1. **Cross-run find/replace in Phase0.** It is the feature users most want. It is deferred
   because its semantics (which separators a match may cross, whose formatting wins, what happens
   to bookmarks inside a match) is a design of its own (roadmap Q1). 24–40% of run splits are
   real formatting changes, so a naive "merge then replace" would change formatting the user
   never touched. Per-run replacement proves the Layer 2 path first.
2. **Normalize runs on open** (merge adjacent runs with identical `rPr`, drop `proofErr`), as
   several template engines do. Rejected: it rewrites most paragraphs of every document. That
   is the opposite of ADR-0001's locality, and it destroys rsid data that Word's compare feature
   uses *(u)*.
3. **Refuse any document with any field**, mirroring ADR-0001's "any formula" rule. Rejected as
   too blunt. Documents with only safe fields, typically `PAGE` in a footer, would be refused for
   no benefit.
4. **Write `updateFields` instead of refusing (W2).** Deferred to Phase1 as an opt-in. It brings
   `settings.xml` into Layer 2. It makes Word show a prompt on open *(u)*. It does nothing for
   non-Word readers, which keep showing stale results, the very failure ADR-0001 R2-M3 refused to
   accept silently. Making it explicit puts that trade in the caller's hands.
5. **Edit inside tracked changes as plain text.** Rejected: an edit inside `w:ins` changes
   proposed text without a revision mark. Inside `w:del` it changes text the reader never sees.
   With `trackRevisions` on, any untracked edit breaks the user's expectation. Tracked generation
   is Phase3.
6. **Give appended paragraphs fresh `w14:paraId`/`textId`/rsids.** Rejected for Phase0: it needs
   the paraId registry. Real files already violate uniqueness, so a correct allocator must scan
   every part, including both `mc:AlternateContent` branches. The attributes are optional.
7. **python-docx as the read-back oracle.** See T5. It remains a possible secondary check.
8. **Ignore document protection** (roadmap row 21 put protection in Phase4). Rejected in revision
   2 (review M3): an enforced "read only" or "forms only" document edited silently would look to
   its readers as if the author wrote the new text. Protection is not security (the hash only
   locks Word's UI), so an explicit opt-in to edit anyway is a reasonable Phase1 addition.
9. **Generic untyped `LensMut` descent in core** (review M2's proposal of `children_raw()` and
   `try_from_child`). Not needed: reads use the raw tree, and the only write paths go through
   allowlisted entries (§3.4). If Phase1 edits runs inside smart tags or content controls, either
   those wrappers get allowlist entries (with the `add_child` override for `w:smartTag`) or core
   gets a QName-checked `into_element_at::<C>(index)` without the `ChildOf` bound; that choice is
   Phase1's.

## Consequences

- **Positive:** the most common Word task, changing words in an existing document, works on 79%
  (POI) to 91% (SDK) of real files with byte-local diffs and an independent read-back. Every
  refusal is a typed error with a fixture or unit test.
- **Positive:** Phase0 exercises ADR-0002 on a second format with a different shape. Free slots
  hold the order, not keys as with Excel's rows. It needs one override, the one ADR-0002
  predicted.
- **Positive:** the gates (DTD, Strict, encryption, `trackRevisions`, protection,
  content-dependent fields) are format-level facts that Phase1+ will keep. Later phases lift them
  one at a time rather than discovering them.
- **Negative:** per-run replacement means a placeholder split across runs (`{{na` `me}}`) cannot
  be filled in Phase0. Callers must find the run that holds the whole placeholder. 5.7–24% of
  text paragraphs have multiple runs.
- **Negative:** headers, footers, notes and comments are read-only in Phase0, so template
  filling in a letterhead does not work until Phase1.
- **Negative:** `StaleFieldRisk` refuses documents with a TOC (18 of 444 SDK files) and
  mail-merge templates with `IF` rules. The way out is Phase1's opt-in `updateFields` and, maybe,
  per-operation classification (open question 2).
- **Negative:** dropping `w:lastRenderedPageBreak` from an edited run is a small, deliberate
  non-local semantic change. It stays within the run's span, so T2 still holds. Other layout and
  statistics caches stay stale (§5).
- **Negative:** the oracles prove internal consistency and agreement with one independent
  reader. They do not prove that Word opens the files cleanly. That needs the owner's manual
  check, as in ADR-0001.

## Open questions for the project owner

Resolved on 2026-11-16 and applied in revision 2: field classification (accepted; unknown codes
refused), DTD refusal location (core), allowlist layout (foundation's per-group files with
`[write]`/`[read]`), docx-rs as dev-dependency, `Document::new()` (deferred), rdocx (deferred),
`lastRenderedPageBreak` (dropping accepted).

Still open:

1. **Manual Word check** (§7): no repair dialog on the ten `manual_check` outputs; in particular
   whether keeping `w14:textId` on an edited paragraph is harmless (output `04`), and whether
   dropping `w:lastRenderedPageBreak` has a visible effect (output `03`).
2. **Per-operation W2** (review m5). Phase0 keeps the document-wide rule. Revision 3 records the
   round-2 reviewer's recommendation as the planned Phase1 direction (roadmap, Phase1): a
   per-operation check limited to `SEQ`, `TC`, `XE`, `NOTEREF`, `LISTNUM`, `AUTONUM*`, whose
   results cannot change under either Phase0 operation. `TOC`, `PAGEREF`, `REF` under append need
   more checks (outline levels of the default style, endnotes) and go with the `updateFields`
   opt-in. Awaiting the owner's confirmation.
3. **`IF` in mail-merge templates** (review M1 "判断"). Planned Phase1 direction (round-2
   reviewer's recommendation): `IF` is safe when every nested field is safe and neither side of
   the comparison is a bare bookmark name, decided by a tokenizer over the instruction; the
   `updateFields` opt-in covers the rest. Awaiting the owner's confirmation.
4. **Fixtures:** would the owner add one document saved by current Microsoft 365 Word? The newest
   producer here is Word 2016.

## Sources

- Roadmap and design questions: `docs/roadmap/docx.md`.
- Spike: `spikes/docx/` (`survey.py`, `design_facts.py`, `phase0_gate.py` revision 2,
  `readback/`, `readback_compare.py`), run 2026-11-16 on rustc 1.99.1 / Python 3.15.
- Implementation and tests: `crates/ooxml-docx/` (`src/body.rs` walk, `src/fields.rs` scan and
  table, `src/document.rs` API, `tests/`, `examples/manual_check.rs`).
- Reviews: `docs/adr/0004-opus-review-round1.md`, `docs/adr/0004-opus-review-round2.md`.
- Fixtures and provenance: `reference/fixtures/docx/README.md` (Apache POI trunk `47dcff76`,
  Open XML SDK `431ab05c`).
- Open XML SDK schema data (`431ab05c`): `w:CT_Body/w:body`, `w:CT_P/w:p`, `w:CT_R/w:r`,
  `w:CT_Text/`, `w:CT_OnOff/` (191 derived entries), `w:CT_DocProtect/w:documentProtection`,
  `MainDocumentPart.json`.
- ECMA-376 Transitional XSD: `wml.xsd` (`CT_OnOff`, `CT_Body`, `CT_DocProtect`),
  `shared-commonSimpleTypes.xsd` (`ST_OnOff`).
- docx-rs 0.4.22 (MIT): https://crates.io/crates/docx-rs, `docx-core/src/reader/read_docx.rs`.
- python-docx `e4545460`: `src/docx/oxml/text/paragraph.py` (`CT_P.text`).
- Apache POI XWPF trunk `47dcff76`: `XWPFParagraph.searchText`, `XWPFDocument.enforceUpdateFields`.
- ADR-0001, ADR-0002, `docs/research/2026-11-16-word-powerpoint-applicability.md`.
