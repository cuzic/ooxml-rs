# ADR-0004: Word Phase0: text edits and paragraph append in `document.xml`

- Status: **Proposed**
- Date: 2026-11-16
- Depends on: [ADR-0001](0001-lossless-round-trip-architecture.md) (layers, oracles, fail-honestly
  stance), [ADR-0002](0002-schema-driven-code-generation.md) (generated views, slots, primitives,
  allowlist, overrides)
- Evidence: `docs/roadmap/docx.md` (gap analysis, design questions Q1–Q7), `spikes/docx/`
  (scripts, results of 2026-11-16), `reference/fixtures/docx/` (25 fixtures)

## Context

ADR-0001's Phase0 proved the three-layer design on Excel. Before that, the Word research note
(`docs/research/2026-11-16-word-powerpoint-applicability.md`) established three things:

- ADR-0002's generator works for WordprocessingML with no unsupported base-namespace content
  model. The slot order matched 171,537 real child lists.
- The hard part of Word is the domain layer, not code generation: cross-part IDs, fields,
  tracked changes and inheritance.
- A candidate first scope: replace text of existing runs, append paragraphs before `sectPr`, no
  new styles/numbering/IDs, `Err` inside fields and tracked changes.

This ADR turns that candidate into a concrete, testable Phase0 for a new crate `ooxml-docx`. The
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
   part's content type unchanged.
2. **Read:**
   - paragraphs of the main document's body in document order, descending into tables;
   - each paragraph's text;
   - its runs, each with a `RunContext` saying where it sits.
3. **Write, (a):** `set_run_text`, which replaces the text of one existing run whose context is
   `Direct` or `Hyperlink`, under the checks of §6.
4. **Write, (b):** `append_paragraph(text)`, which adds `<w:p><w:r>…</w:r></w:p>` as the last block
   of the body, before the body-level `w:sectPr` (or at the end if there is none). It has no
   `pPr`, no `rPr` and no IDs, so it takes the default paragraph style and joins the last section.
5. **From scratch:** `Document::new()` opens an embedded template package through the same code
   path (ADR-0001's "degenerate case"). It allows only `append_paragraph` and `save`.
   `docProps/core.xml` is regenerated via `Package::set_part`, as in ADR-0001.
6. **Gates** that refuse whole documents or individual targets with typed errors (§6).

Out of scope (roadmap phases):

- runs in headers, footers, notes or comments (Phase1);
- cross-run replace (Phase1);
- formatting (Phase1);
- new styles, numbering, tables, images, bookmarks, hyperlinks or sections (Phase2);
- comments, notes, tracked edits, SDT, fields (Phase3);
- drawings, OMML, charts (Phase4);
- Strict, encrypted and DTD-carrying packages (refused).

**Phase0 creates no IDs and no parts.** It writes no `w14:paraId`, `w14:textId`, `w:rsid*`,
`w:id`, relationship, content-type entry or new part, so no Layer 3 registry is exercised. The
same move ADR-0001 made with `inlineStr`.

### 2. Parts and parse scope

| Part | How found | Layer 2? | Phase0 use |
|---|---|---|---|
| main document (`word/document.xml` usually) | `/_rels/.rels`, type `…/officeDocument/2006/relationships/officeDocument` | **yes, editable** | all reads and writes |
| `settings.xml` | main part's rels, type `…/settings` | yes, **read-only** (stays dirty-free `Parsed` → raw copy, ADR-0001 M2) | `w:trackRevisions` (gate), `w:updateFields` (reported, not written) |
| headers, footers, footnotes, endnotes, comments | main part's rels (any name: `poi-Bug60341` uses `footer.xml`) | **no**: a `quick-xml` event scan at open, like ADR-0001's formula scan | field gate (§6, `StaleFieldRisk`) |
| everything else (styles, numbering, theme, media, glossary, charts, customXml, …) | — | no | Layer 1 raw passthrough, untouched and unparsed. A missing `styles.xml` is fine (`poi-55733`). |

The main part's relationships part is derived from its name (`<dir>/_rels/<name>.rels`), as
ADR-0002 §6 does for the workbook. A package whose root relationship has the **Strict** type
(`http://purl.oclc.org/ooxml/officeDocument/relationships/officeDocument`), or whose main root
element is in the Strict WML namespace, is `Err(StrictNotSupported)` at open.

### 3. Domain API sketch (over ADR-0002 views)

Generated views come from the allowlist in §4: `BodyView`/`BodyViewMut`, `PView`/`PViewMut`,
`RView`/`RViewMut`, `TView`/`TViewMut`, … . Domain types wrap them. The names follow the
`View`-suffix convention of ADR-0002 §3.

```rust
pub struct Document { /* Package, main part name, Gates (computed once at open) */ }

impl Document {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, Error>;
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, Error>;
    pub fn new() -> Self;                                  // embedded template, §1.5
    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), Error>; // atomic, ADR-0001 R2-M4
    pub fn to_bytes(&self) -> Result<Vec<u8>, Error>;

    /// Ok(()) if writes are allowed; otherwise the document-level gate (§6, W1–W2).
    pub fn write_gate(&self) -> Result<(), Error>;

    /// Paragraphs of w:body in document order, descending into w:tbl/w:tr/w:tc and
    /// w:sdt/w:sdtContent, but NOT into drawings, VML, OLE objects or
    /// mc:AlternateContent (text boxes are not body flow).
    pub fn paragraphs(&self) -> impl Iterator<Item = Paragraph<'_>>;
    pub fn paragraph_mut(&mut self, index: usize) -> Result<ParagraphMut<'_>, Error>;

    /// Returns the new paragraph's index. Err per §6 (W1, W2, A1, T1).
    pub fn append_paragraph(&mut self, text: &str) -> Result<usize, Error>;
}

pub struct Paragraph<'a> { /* PView<'a>, index */ }
impl Paragraph<'_> {
    pub fn index(&self) -> usize;
    /// "All revisions accepted" text: w:t, w:tab ('\t'), w:br/w:cr ('\n'),
    /// w:noBreakHyphen ('\u{2011}') of runs reachable through w:hyperlink, w:ins,
    /// w:moveTo, w:smartTag, w:sdt/w:sdtContent, w:fldSimple, w:customXml and field
    /// results; excluding w:delText, w:moveFrom, w:instrText and anything inside
    /// w:drawing / w:pict / w:object / mc:AlternateContent.
    pub fn text(&self) -> String;
    pub fn runs(&self) -> impl Iterator<Item = Run<'_>>;   // the runs text() reads, in order
}

pub struct Run<'a> { /* RView<'a>, RunContext */ }
impl Run<'_> {
    pub fn text(&self) -> String;
    pub fn context(&self) -> RunContext;
    /// Why set_run_text would fail for this run, without trying (§6, R1–R4).
    pub fn check_settable(&self) -> Result<(), Error>;
}

#[non_exhaustive]
pub enum RunContext {
    Direct, Hyperlink,                       // settable in Phase0
    Inserted, Deleted, MovedFrom, MovedTo,   // w:ins / w:del / w:moveFrom / w:moveTo
    SmartTag, ContentControl, SimpleField, CustomXml, Bidi, // w:dir / w:bdo
    FieldResult,                             // between fldChar separate and end
}

pub struct ParagraphMut<'a> { /* PViewMut<'a> */ }
impl ParagraphMut<'_> {
    /// `run` indexes Paragraph::runs(). Err per §6.
    pub fn set_run_text(&mut self, run: usize, text: &str) -> Result<(), Error>;
}
```

Indices are stable for the lifetime of a `Document` because Phase0's only structural change
(`append_paragraph`) adds after every existing paragraph. Internally a paragraph is addressed by
ADR-0002's child-index path, which append does not invalidate either, since it inserts after
every existing block and before `sectPr`.

**`set_run_text` semantics** (all writes go through ADR-0002 primitives, so the dirty-flag
duties are theirs):

1. Map `text` to run content:
   - maximal substrings without `\t`/`\n` → `w:t`;
   - `\t` → `<w:tab/>`;
   - `\n` → `<w:br/>`;
   - characters not allowed in XML 1.0 (U+0000–U+0008, U+000B, U+000C, U+000E–U+001F,
     U+FFFE, U+FFFF) → `Err(InvalidXmlChar)`.
2. **Fast path:** if the run's content (children other than `w:rPr`) is exactly one `w:t` and the
   mapping is one `w:t`, call `set_text` on that `w:t`. Its open tag stays verbatim unless
   `xml:space` must be added.
3. **Otherwise:** remove every content child (`remove_at`), then `insert` the mapped children.
   `w:rPr` is never touched. A `w:lastRenderedPageBreak` in the run is therefore dropped: it is a
   layout cache of where Word last broke the page, and it is stale once the text changes. This is
   the Word analogue of ADR-0001 recomputing `<dimension>`.
4. **`xml:space`:** the `set_text` primitive adds `xml:space="preserve"` for leading or trailing
   whitespace (ADR-0002 §4.2). The domain layer also adds it when the segment contains two
   consecutive whitespace characters. 296 `w:t` in POI have edge whitespace **without**
   `preserve` (written by other producers); Phase0 never removes an existing `xml:space`.
5. Attributes of the run and paragraph (`w:rsid*`, `w14:paraId`, `w14:textId`) are untouched
   (open question 3).

**`append_paragraph`:** `body.insert::<P>()`. The body's slots are [35 block QNames, `sectPr`],
so the default `insert` lands before `sectPr`. Then `p.insert::<R>()`, then content as in
`set_run_text` step 1. The new nodes have no original span, so ADR-0001 T2's "insertion point"
rule applies.

### 4. Phase0 WML allowlist and overrides

Proposed contents for the WML allowlist. The file is owned by the foundation workstream: ADR-0002
§5 names `codegen/allowlist.toml`, and the brief for this round names `codegen/allowlist/wml.toml`,
so the owner picks the layout. Entries are SDK `Name`s. Abstract bases (`w:CT_Text/` with
`xml:space`, `w:CT_OnOff/` with `w:val`) are merged by the generator's inheritance rule (ADR-0002
§3) and are not listed.

```toml
# Proposed by ADR-0004 (Word Phase0). Feature group: `wml`.
[write]   # insert / remove / set_text / set_attr allowed through views
names = [
  "w:CT_Body/w:body",                  # insert w:p (free slot, lands before sectPr)
  "w:CT_P/w:p",                        # created by append; insert w:r
  "w:CT_R/w:r",                        # content replaced by set_run_text
  "w:CT_Text/w:t",                     # set_text (+ xml:space from w:CT_Text/)
  "w:CT_Empty/w:tab",                  # '\t'
  "w:CT_Br/w:br",                      # '\n'
]
[read]    # typed reads only; writes through these views return Err
names = [
  "w:CT_Document/w:document",
  "w:CT_SectPr/w:sectPr",              # slot anchor; sectPr-in-pPr detection
  "w:CT_PPr/w:pPr",
  "w:CT_RPr/w:rPr",                    # presence only; never re-rendered
  "w:CT_Hyperlink/w:hyperlink",
  "w:CT_Tbl/w:tbl", "w:CT_Row/w:tr", "w:CT_Tc/w:tc",
  "w:CT_Empty/w:cr", "w:CT_Empty/w:noBreakHyphen", "w:CT_Empty/w:softHyphen",
  "w:CT_Empty/w:lastRenderedPageBreak",
  "w:CT_FldChar/w:fldChar", "w:CT_Text/w:instrText", "w:CT_SimpleField/w:fldSimple",
  "w:CT_RunTrackChange/w:ins", "w:CT_RunTrackChange/w:del",
  "w:CT_RunTrackChange/w:moveFrom", "w:CT_RunTrackChange/w:moveTo",
  "w:CT_Text/w:delText",
  "w:CT_Settings/w:settings",
  "w:CT_OnOff/w:trackRevisions", "w:CT_OnOff/w:updateFields",
]
```

The `[write]`/`[read]` split is this ADR's proposal. ADR-0002's Phase0 allowlist marks "read
only" in comments instead. With either form, generated `ChildOf` impls exist exactly for
Body→P, P→R, R→T, R→Tab and R→Br among write entries, which are all inserts Phase0 makes. All
six write entries lower to slots without ambiguity:

- `w:body` = [blocks, `sectPr`];
- `w:p` = [`pPr`, content];
- `w:r` = [`rPr`, content].

None of the 8 ECMA-order disagreements or the 12 unsupported models is involved, and no entry is
`attrs_incomplete`. Context elements (`w:sdt`, `w:smartTag`, `w:customXml`, `w:dir`, `w:bdo`,
`w:drawing`, `w:pict`, `w:object`, `mc:AlternateContent`) are recognized by QName only and need
no generated views.

**Overrides (one):**

```toml
[[set_default]]
on = "w:CT_OnOff/"
attribute = "w:val"
value = "true"
reason = "WML on/off elements mean 'on' when val is omitted; the XSD (CT_OnOff, ST_OnOff) gives no default and leaves it to prose"
evidence = "reference/fixtures/docx/poi-delins.docx (settings: <w:trackRevisions/>); ECMA-376 Part 1, description of each on/off element's val (e.g. w:b)"
```

This is the example override ADR-0002 §1.3 already records, and Phase0 needs it. Without it,
`<w:trackRevisions/>` (`poi-delins`) would read as "no value" and the `TrackRevisionsOn` gate
would let an edit through. Reads of on/off properties are `Option<bool>` (roadmap Q5); Phase0
reads only the two settings flags.

**Recorded for Phase1, not Phase0:** an `add_child` for `w:smartTag` under `w:CT_P/w:p`. It is
an ECMA Transitional element absent from the SDK's `CT_P`, seen 424 times in 25 files. With the
override, runs inside smart tags become typed. Until then they are unlisted children, attached to
the previous sibling and preserved (ADR-0002 §4.3).

### 5. Cross-part policy (Layer 3 analogue)

Phase0 needs no registry, by construction:

| Cross-part concern | Phase0 policy |
|---|---|
| Style IDs (`styles.xml`) | Not read. Appended paragraphs have no `pStyle`, so they use the default paragraph style. Edited runs keep their `rPr`. |
| Numbering (`numbering.xml`) | Not read. Appended paragraphs have no `numPr`. |
| Annotation IDs (bookmarks, comments, revisions) | None created. Edits never touch range markers: they are siblings of runs, and only run content changes. |
| `w14:paraId` / `w14:textId` / rsids | None created. Existing ones are left as they are, duplicates included (roadmap Q4: 8 of 82 files have duplicates). |
| Relationships, content types, new parts | None. `Package` writes back exactly the original part set (ADR-0001 Layer 1). |
| Field results anywhere | Guarded by the `StaleFieldRisk` gate (§6 W2). Never recomputed, never marked dirty. |
| `settings.xml` | Read-only. `updateFields` is **not** written in Phase0. The opt-in setter is Phase1 (roadmap Q2). |

Phase2's registries (roadmap §5) must keep ADR-0001's invariant: existing IDs are append-only,
never renumbered or "repaired".

### 6. Errors

Each row is the variant, when it fires, and a fixture that triggers it. Errors fire before any
mutation, so a failed call leaves the tree untouched. At open:

| # | Variant | Condition | Fixture |
|---|---|---|---|
| O1 | `EncryptedPackage` (core) | CFB magic `D0 CF 11 E0` | `poi-bug53475-password-is-pass` |
| O2 | `DtdNotAllowed` (proposed for core) | a part ooxml-rs parses or scans contains `<!DOCTYPE`. Never parsed with entities left unexpanded (XXE probe) | `poi-ExternalEntityInText` |
| O3 | `StrictNotSupported` | Strict root relationship type or Strict WML namespace on the main root | `sdk-strict-CommentRangeStart` |
| O4 | `MainPartMissing` | no `officeDocument` relationship, or its target is absent | (unit test) |

Document-level write gates. Computed once at open. Reads still work; every write method
returns the error:

| # | Variant | Condition | Fixture |
|---|---|---|---|
| W1 | `TrackRevisionsOn` | `settings/w:trackRevisions` is on (absent `val`, `true`, `1`, `on`) | `poi-delins` |
| W2 | `StaleFieldRisk { codes }` | any story part (main, headers, footers, footnotes, endnotes, comments) has a complex field or `fldSimple` whose code, after joining split `instrText`, is content-dependent (`TOC`, `REF`, `PAGEREF`, `NOTEREF`, `SEQ`, `STYLEREF`, `=`, `IF`, `NUMWORDS`, `NUMCHARS`, `INDEX`, `TOA`, `TC`, `XE`, `LISTNUM`, `CITATION`, `BIBLIOGRAPHY`, `QUOTE`, `RD`, `COMPARE`) or **not classifiable** | `sdk-TOC2`, `sdk-complex2010` |

Fields treated as safe in W2 are layout, metadata or input fields, whose cached result an edit
elsewhere cannot make wrong in a way a reader could detect:

- `PAGE`, `NUMPAGES`, `SECTIONPAGES`, `SECTION`;
- `DATE`, `TIME`, `CREATEDATE`, `SAVEDATE`, `PRINTDATE`, `EDITTIME`;
- document-property fields;
- `HYPERLINK`;
- mail-merge fields;
- `FORMTEXT`, `FORMCHECKBOX`, `FORMDROPDOWN`;
- `EQ`, `SYMBOL`, `MACROBUTTON`, `GOTOBUTTON`;
- `INCLUDEPICTURE`, `INCLUDETEXT`, `LINK`, `EMBED`;
- `AUTONUM*`, `ASK`, `FILLIN`, `SET`.

The authoritative list is `CONTENT_DEPENDENT`/`SAFE` in `spikes/docx/phase0_gate.py`, to be
ported as a table.

Per target:

| # | Variant | Condition | Fixture |
|---|---|---|---|
| R1 | `InsideField` | the run lies between a `fldChar begin` and its `end` (instruction or result), or inside `w:fldSimple` | `poi-FieldCodes`, `sdk-Field-more-field` |
| R2 | `InsideRevision` | the run's context is `Inserted`, `Deleted`, `MovedFrom` or `MovedTo` | (`poi-delins` is already refused by W1. Unit-test a copy with the flag off.) |
| R3 | `UnsupportedContext { ancestor }` | an ancestor between the run and `w:body` is anything other than `w:p`, `w:hyperlink`, `w:tbl`, `w:tr`, `w:tc`. This covers `w:sdt` (data-bound content is overwritten by Word from the custom XML store, roadmap row 16), `w:smartTag`, `w:customXml`, `w:dir`, `w:bdo`, text boxes and `mc:AlternateContent` | `sdk-coverpage1`, `poi-PageSpecificHeadFoot` (SDT), `poi-smarttag-snippet`, `poi-shapes-with-text` |
| R4 | `RunNotTextOnly { child }` | the run has a child other than `w:rPr`, `w:t`, `w:tab`, `w:br` (no `type` or `textWrapping`), `w:cr`, `w:noBreakHyphen`, `w:softHyphen`, `w:lastRenderedPageBreak`. Examples: `w:drawing`, `w:sym`, `w:footnoteReference`, `w:fldChar`, `w:object`, `mc:AlternateContent`, page `w:br` | (unit test; 6 runs in the corpora) |
| T1 | `InvalidXmlChar { ch }` | text contains an XML-1.0-forbidden character | (unit test) |
| A1 | `UnresolvedSibling` (core, ADR-0002 §4.3) | `append_paragraph` into a `w:body` that has an `mc:AlternateContent` child | `sdk-HelloO14` |

Measured effect (`spikes/docx/phase0_gate.py`):

- **SDK (non-Strict):** 404 of 444 files are writable. Refused: 30 `StaleFieldRisk`, 6
  `TrackRevisionsOn`, 2 encrypted, 2 Strict. In writable files, 23,162 of 24,558 text runs
  (94%) are settable. The rest are mostly SDT context (1,347) and inside fields (44).
- **POI:** 106 of 129 files are writable. Of the rest, 15 are not zips or are corrupt. In
  writable files, 27,407 of 27,682 text runs (99%) are settable.

### 7. Test oracles

Fixtures are `reference/fixtures/docx/*.docx`. 22 open as packages; the other 3 are O1–O3.

- **T1 (prolog/epilog):** per ADR-0001, for every XML part ooxml-rs parses in the 22 fixtures
  (main, settings), parse and serialize with zero edits, then compare byte-for-byte. Covers a
  UTF-8 BOM (`poi-Bug60341`, `poi-55733`, `sdk-HelloO14`) and pretty-printed parts
  (`poi-table-alignment`).
- **T1b (parser/renderer):** force every node dirty, render, re-parse, compare structurally,
  for the same parts plus every story part.
- **T2 (locality):**
  - For every run in the writable fixtures with `check_settable() == Ok` (3,110 runs across 19
    files), call `set_run_text` with the run's own text and then with a different text that has
    leading whitespace and an embedded `\t`.
  - Each time, the byte diff of the main part against the input must lie within that run's
    original span (`start..close.1`).
  - For `append_paragraph` on each writable fixture, the diff is a pure insertion at the
    position immediately before the body-level `<w:sectPr`, or before `</w:body>` when there is
    none (`poi-55733`).
- **T3 (Layer 1):** every part other than the main part has identical (name, compression method,
  CRC32, compressed payload), including zip directory entries (`poi-table-alignment`) and stored
  entries.
- **T4 (refusals):** every row in §6 with a fixture returns exactly that variant. Reads still
  succeed after W1/W2. The saved output, if the caller saves anyway, is byte-identical per T3 for
  all parts, because a refused write changes nothing.
- **T5 (independent read-back):** `docx-rs = { version = "=0.4.22", default-features = false }`
  as a dev-dependency.
  - **Procedure.** Write a unique marker string with `set_run_text`, save, and call
    `docx_rs::read_docx(&bytes)?.json()`. Collect every `"text"` value in document order.
    Assert that the marker occurs exactly once. Assert that the list with the marker removed
    equals the input's list with the edited run's original pieces removed. For append, assert
    that the appended text is the last body text.
  - **Why docx-rs.** It is an independent codebase (MIT, its own XML reader and model). On 490
    real files it read 469 and panicked on 2 (`Strict01.docx` and a fuzz case, neither a Phase0
    fixture). Against an ElementTree ground truth it recovered 57,615/57,616 plain, 1,096/1,096
    hyperlink, 108/108 `w:ins`, 53/53 `fldSimple` and 506/506 smart-tag `w:t` texts
    (`spikes/docx/readback/`, `readback_compare.py`). With `default-features = false` its tree
    is 39 crates, with no `image`.
  - **Why not the others.**
    - python-docx: `Paragraph.text` reads only `w:r|w:hyperlink` children (source), so it misses
      `ins`, `fldSimple`, `sdt` and `smartTag` runs. It also needs a Python toolchain in CI, and
      it is not installed here. A python-docx check script stays optional for Phase1.
    - docx-rust: on the fixtures it recovered only 2,295 of 3,785 plain and 0 of 36 `w:ins`
      `w:t` texts, and it fails to parse 9 valid packages.
    - `ooxmlsdk`: ADR-0002 already lists it as a possible "does it parse" oracle. Compile cost
      is high, so it is deferred to Phase2.
- **T6 (schema order):** for every node an edit made dirty, its element children appear in
  generated-slot order under the greedy assignment of ADR-0002 §4.1. This is the Rust port of
  `spikes/adr-0002/py/corpus_check.py`, run on outputs.
- **T7 (from scratch):** `Document::new()`, append three paragraphs (one with edge whitespace,
  one with `\t` and `\n`), save. docx-rs reads exactly those three texts, and T1/T1b/T6 pass on
  the output.
- **Manual (explicit gap, as in ADR-0001):** there is no Word or LibreOffice in this sandbox,
  and no docker access. The project owner opens a sample of T2/T7 outputs in Word and checks
  for no repair dialog, the expected text, and unchanged fields and comments. The sample is one
  output per writable fixture plus the T7 output.

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
   too blunt. 69 of the 510 writable files (16 POI, 53 SDK) contain only safe fields, typically
   `PAGE` in a footer, and would be refused for no benefit.
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

## Consequences

- **Positive:** the most common Word task, changing words in an existing document, works on
  91–93% of real files with byte-local diffs and an independent read-back. Every refusal is a
  typed error with a fixture.
- **Positive:** Phase0 exercises ADR-0002 on a second format with a different shape. Free slots
  hold the order, not keys as with Excel's rows. It needs one override, the one ADR-0002
  predicted.
- **Positive:** the gates (DTD, Strict, encryption, `trackRevisions`, content-dependent fields)
  are format-level facts that Phase1+ will keep. Later phases lift them one at a time rather than
  discovering them.
- **Negative:** per-run replacement means a placeholder split across runs (`{{na` `me}}`) cannot
  be filled in Phase0. Callers must find the run that holds the whole placeholder. 5.7–24% of
  text paragraphs have multiple runs.
- **Negative:** headers, footers, notes and comments are read-only in Phase0, so template
  filling in a letterhead does not work until Phase1.
- **Negative:** `StaleFieldRisk` refuses documents with a TOC, which is common in long real
  documents (18 of 444 SDK files). The way out is Phase1's opt-in `updateFields`.
- **Negative:** dropping `w:lastRenderedPageBreak` from an edited run is a small, deliberate
  non-local semantic change. It stays within the run's span, so T2 still holds.
- **Negative:** the oracles prove internal consistency and agreement with one independent
  reader. They do not prove that Word opens the files cleanly. That needs the owner's manual
  check, as in ADR-0001.

## Open questions for the project owner

1. **Field classification (W2).** Are `PAGE`/`NUMPAGES`/`DATE`/`HYPERLINK` acceptable as "safe",
   and are unknown codes rightly refused? (POI's only W2 refusal is a file using
   `M:USERCONTENT`.)
2. **`Document::new()` template.**
   - Source: hand-authored, as ADR-0001's xlsx template was, or a blank document saved by the
     owner's Word, which is more realistic and carries Word's default styles and theme?
   - Page size: A4 or Letter.
   - Default East Asian font settings.
3. **`w14:textId` on edited paragraphs.** Phase0 leaves it unchanged. [MS-DOCX] describes it as a
   text-version identifier used by co-authoring *(u)*. Should Phase0 instead remove it from a
   paragraph it edits? This needs a Word check.
4. **`w:lastRenderedPageBreak`.** Is dropping it inside an edited run acceptable (§3, step 3), or
   should `RunNotTextOnly` refuse such runs instead? It occurs in 19 POI files.
5. **DTD refusal in `ooxml-core`.** It is proposed for all formats (O2). The foundation owner
   decides where it lives.
6. **Allowlist file layout and the `[write]`/`[read]` split** (§4) are for the foundation owner.
7. **Fixtures.** Is committing Apache-2.0 (with its NOTICE) and MIT third-party test files under
   `reference/fixtures/docx/` acceptable? Would the owner add one document saved by current
   Microsoft 365 Word? The newest producer here is Word 2016.
8. **rdocx.** `rdocx` 0.17 claims byte-preserving round-trip editing of unknown XML. Should it be
   evaluated, as `ooxmlsdk` was for ADR-0002, before Phase1?

## Sources

- Roadmap and design questions: `docs/roadmap/docx.md`.
- Spike: `spikes/docx/` (`survey.py`, `design_facts.py`, `phase0_gate.py`, `readback/`,
  `readback_compare.py`), run 2026-11-16 on rustc 1.99.1 / Python 3.15.
- Fixtures and provenance: `reference/fixtures/docx/README.md` (Apache POI trunk `47dcff76`,
  Open XML SDK `431ab05c`).
- Open XML SDK schema data (`431ab05c`): `w:CT_Body/w:body`, `w:CT_P/w:p`, `w:CT_R/w:r`,
  `w:CT_Text/`, `w:CT_OnOff/` (191 derived entries), `MainDocumentPart.json`.
- ECMA-376 Transitional XSD: `wml.xsd` (`CT_OnOff`, `CT_Body`), `shared-commonSimpleTypes.xsd`
  (`ST_OnOff`).
- docx-rs 0.4.22 (MIT): https://crates.io/crates/docx-rs, `docx-core/src/reader/read_docx.rs`.
- python-docx `e4545460`: `src/docx/oxml/text/paragraph.py` (`CT_P.text`).
- Apache POI XWPF trunk `47dcff76`: `XWPFParagraph.searchText`, `XWPFDocument.enforceUpdateFields`.
- ADR-0001, ADR-0002, `docs/research/2026-11-16-word-powerpoint-applicability.md`.
