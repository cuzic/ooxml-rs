# ADR-0015: Word Phase3: review features (comments, notes, tracked changes, content controls, fields, effective formatting)

- Status: **Proposed** (revision 1)
- Date: 2026-11-16
- Depends on:
  - [ADR-0001](0001-lossless-round-trip-architecture.md): layers, T1/T2, Layer 3's
    append-only invariant.
  - [ADR-0002](0002-schema-driven-code-generation.md): slots, primitives, allowlist, and the
    post-acceptance notes (depth limit; detach/insert, PR #18).
  - [ADR-0004](0004-docx-phase0.md): Word Phase0, Accepted. Gates W1–W3, R1–R4.
  - ADR-0007 (Word Phase1, PR #25): stories beyond the body, the per-operation W2 table, and
    `updateFields`.
  - ADR-0011 (Word Phase2, PR #31):
    - the OPC part plan (§1);
    - the Word registries (§2), including the annotation-id union and the `w14:paraId`
      registry, which is designed there but not activated;
    - run splitting and paragraph insertion (§9);
    - the structure column of W2 (§10).
  - The core transaction API being designed on `core/add-part`. It generalizes ADR-0011's
    `PartPlan` and is required by §1 below.
- Evidence:
  - `spikes/docx-phase3/` (`review.py`, `summarize.py`, README with every number).
  - Corpora: fixtures (23 open), Apache POI (111), Open XML SDK non-Strict (440), docx4j (12),
    template-engine files (212).
  - Library behaviour from source at pinned revisions (§12, Sources).

## Context

The roadmap's Phase3 is "review and semantics" (`docs/roadmap/docx.md`, Phase3). It is what a
reviewer or an automated review tool does to a Word document:

- comment on text, reply to comments and resolve them;
- add footnotes and endnotes;
- make edits that show up as tracked changes;
- fill in and create content controls;
- insert fields;
- ask what formatting a run really has.

Phases 0–2 refuse most of this. W1 refuses every write while `trackRevisions` is on. R2 refuses
edits inside revisions. R3 refuses runs inside content controls. W2 refuses documents whose
fields could go stale.

Each feature touches several parts and several id spaces at once. A reply alone touches:

- `comments.xml` (a new `w:comment`);
- `commentsExtended.xml` (`w15:commentEx` with `w15:paraIdParent`);
- `commentsIds.xml` and `commentsExtensible.xml` (a durable id and a UTC date, when those parts
  exist);
- `people.xml` (the author);
- the story (range start, range end and the reference run).

It needs an annotation id, a `w14:paraId`, a durable id and sometimes new parts, relationships
and content types. **All or nothing** is the only acceptable outcome: a comment whose
`commentEx` is missing is still a comment, but a `commentEx` without its comment, or a reference
to a comment that does not exist, is a broken file. The corpora contain 2 dangling comment
anchors, written by other tools.

The spike's corpus findings that shape the design (the README has every number):

- **Comments.**
  - Range start, range end and a reference run anchor 1,195 of 1,215 SDK comments.
  - `commentsExtended` is **partial**: 1,081 comments in 54 SDK files have no `commentEx`.
  - Every `commentEx`, `commentId` and `paraIdParent` points at the **last** paragraph of a
    comment.
  - Replies carry their own full anchoring.
  - `people.xml` authors do not match `w:author` strings exactly (25 SDK files).
- **`w14:paraId`:** on 90% of comment paragraphs, but on 5% of body paragraphs. Duplicates occur
  in 17 files.
- **Revisions:**
  - dates always `YYYY-MM-DDTHH:MM:SSZ` when present;
  - paragraph-mark revisions are common;
  - ids never collide with comment ids.
- **Notes:**
  - two separator id conventions, (`-1`, `0`) and (`0`, `1`);
  - footnote and endnote id spaces are separate;
  - reference styles often have localized ids.
- **Content controls:**
  - 463 SDK data bindings, of which 401 point at **Office built-in stores** (core and extended
    properties) rather than at custom XML parts;
  - negative and duplicate `w:id`s.
- **Fields:**
  - complex fields dominate (658 to 118);
  - 114 complex fields have no cached result;
  - `w:dirty` was never seen.
- **Formatting:**
  - `basedOn` chains up to 5 deep;
  - toggle properties in styles in hundreds of files;
  - numbering-level `rPr` in 164 files;
  - conditional table formatting is frequent, while the cached `cnfStyle` is rare (19 files).

## Decision

### 1. Prerequisite: a core transaction that spans parts

Phase3 operations edit **existing** parts (the story, `comments.xml`, `settings.xml`, a custom
XML part) and **add** parts and registry entries in the same step. ADR-0011's `PartPlan` covers
the additions. Phase3 needs the plan to cover the edits too. The requirements on the
`core/add-part` design are:

1. **One staging area for both.** It holds new parts, new relationships and content types, and
   **checked Layer 2 edits** of existing parts. A checked edit is a `LensMut` operation whose
   refusals (`check_insert_at`, PR #26; slots; depth limit) all run before anything changes.
2. **`commit` cannot fail after validation.** Validation runs every check: core's OPC rules
   (ADR-0011 §1.2), every staged edit's check, and the format crate's registry freshness checks
   (§2). After that, commit only applies.
3. **On any error before commit, nothing is visible.** Not in the trees, not in the registries,
   not in the zip. Registry allocations made while planning are released, which is safe because
   nothing referenced them yet.
4. **Edits of a part that does not exist yet** (a new `comments.xml`) are expressed as its
   initial tree (`PartBody::Xml`), not as edits.

ooxml-docx builds every Phase3 operation as one transaction. When `core/add-part` lands, this ADR
is checked against its final API before implementation. The API names below are the docx side
only.

### 2. Registries (activating and extending ADR-0011 §2)

| Registry | Scope scanned | Allocation | Evidence |
|---|---|---|---|
| **Annotation ids** (ADR-0011): comments, revisions, bookmarks, permissions, move ranges | the union over every story and `comments.xml` | max + 1 over the union, never a gap. Comment and revision ids come from here. | Comment ids start above 0 in 19 files and have gaps in 28. Revision ids never collide with comment ids; comment ids collide with bookmark ids in 2 template outputs, and the union avoids both. |
| **`w14:paraId`** (designed in ADR-0011, activated here) | every paragraph and row in every story; `commentsExtended`, `commentsIds` and `paraIdParent` values | random value in `[1, 0x7FFFFFFF]`, absent from the scope *(ECMA-376 MS extension: < 0x80000000)*; redrawn on collision | 0 values ≥ 0x80000000; duplicates in 17 files, tolerated and avoided |
| **`w14:textId`** | — | written as `77777777` on new paragraphs that get a `paraId` (Open question 2) | comments: 1,516 of 1,603 paragraphs with `paraId` carry it |
| **Durable ids** (`w16cid:durableId`) | `commentsIds.xml`, `commentsExtensible.xml` | random value in `[1, 0x7FFFFFFF]`, absent from both | 0 values ≥ 0x80000000 |
| **Note ids** | per kind: `footnotes.xml` (separators included) plus `footnoteReference` in every story; likewise endnotes | max + 1 per kind. Footnotes and endnotes are separate spaces; ids are never reused. | Equal footnote and endnote ids in 11 files. Separators are (`-1`, `0`) or (`0`, `1`). |
| **SDT ids** (`sdtPr/w:id`) | every story | random signed 32-bit value, non-zero, absent from the scope | negative values in 16 files, duplicates in 6 |
| **People** | `people.xml` | appended when a new author is absent, by exact string (§3.4) | authors not matching exactly in 25 SDK files |

Every scan is lazy (ADR-0011), and every value is checked against its scope again at validation
(T-REG). Existing duplicates are never repaired.

### 3. Comments

#### 3.1 Operations

```rust
impl Document {
    pub fn add_comment(&mut self, range: TextRange, c: NewComment) -> Result<CommentId>;
    pub fn reply(&mut self, parent: CommentId, c: NewComment) -> Result<CommentId>;
    pub fn set_resolved(&mut self, id: CommentId, done: bool) -> Result<()>;
    pub fn delete_comment(&mut self, id: CommentId) -> Result<()>;   // with its replies
    pub fn comments(&self) -> impl Iterator<Item = CommentRef<'_>>; // author, date, text, parent, done, anchor
}
pub struct NewComment { pub author: String, pub initials: Option<String>, pub date: Option<UtcDateTime>, pub paragraphs: Vec<String> }
```

`TextRange` is a start and an end position at run boundaries in one story, possibly in different
paragraphs. Runs are split as needed with ADR-0011 §9.2's `split_run`, and the split must pass
R1–R4. The read side resolves the parent through `commentEx`. It falls back to "no parent" and
"not done" when a comment has no entry, which is the case in 1,081 SDK comments.

#### 3.2 What `add_comment` writes, in one transaction

- **Story.**
  - `w:commentRangeStart w:id` goes before the first run of the range.
  - `w:commentRangeEnd w:id` goes after the last run.
  - Right after the end, a run with `w:rStyle` = the comment-reference style (§3.5) and
    `w:commentReference w:id`.

  This is the shape of 1,195 of 1,215 SDK comments, and the shape python-docx writes (§12).
- **`comments.xml`.** `w:comment w:id w:author w:date w:initials` with one `w:p` per paragraph:
  - each paragraph has `pStyle` = the comment-text style, `w14:paraId` and `w14:textId`;
  - the first paragraph starts with a run with the reference style and `w:annotationRef`.

  If the part is missing, it is created through §1: `word/comments.xml`, a relationship from the
  main part, and an `Override`. The namespace declarations are those of the template in §3.5.
- **`commentsExtended.xml`.** `w15:commentEx w15:paraId=<last paragraph's paraId> w15:done="0"`.
  - It is created if missing: it is the part that carries replies and the resolved state, and
    Word 2013+ writes it *(u)*.
  - Entries are appended for created comments only; existing comments without an entry stay as
    they are.
- **`commentsIds.xml` / `commentsExtensible.xml`.** An entry with a new durable id is written
  **only if the part exists**: `w16cid:commentId` with `paraId` and `durableId`; and
  `w16cex:commentExtensible` with the same durable id and `dateUtc`.
  - They are not created, because Word 2016+/2018+ adds them on save *(u)*. `commentsIds` occurs
    in 4 corpus files and `commentsExtensible` in 2. Open question 3.
  - When one exists and the other does not, only the existing one gets an entry.
- **`people.xml`.** A `w15:person w15:author` with `w15:presenceInfo w15:providerId="None"
  w15:userId=<author>` is appended **if the part exists** and no person has exactly that author
  string. The part is not created.
- **Dates.** The caller's, or "now". Written in UTC as `YYYY-MM-DDTHH:MM:SSZ`, the only shape in
  the corpora. Fractional seconds are not written.

#### 3.3 Replies, resolve and delete

- **`reply(parent, …)`** writes a new `w:comment` and its `commentEx` with
  `w15:paraIdParent` = the parent's last-paragraph `paraId`. Its anchors are placed **inside**
  the parent's: range start right after the parent's start, range end right after the parent's
  end, and its reference run right after the parent's reference run. That is the shape of the
  corpus replies.
  - A parent whose last paragraph has no `w14:paraId` gets one. This is an attribute edit of an
    existing paragraph's start tag, so it is local, and T2 permits it.
  - When `w14` is not declared in scope, the edit is refused (`ParentNotAddressable`), because
    declaring a namespace on an original node is refused by ADR-0011 §1.3. How often that
    happens is not measured; the implementation's first commit adds it to the spike.
  - Replies to replies are refused (`NestedReply`): Word's UI threads one level *(u)*, and the
    corpus has none.
- **`set_resolved`** sets `w15:done` on the comment's `commentEx`, creating the entry if it is
  missing. Replies inherit the state *(u)*: only the parent is set.
- **`delete_comment`** removes the comment, its replies, their anchors in every story, and their
  `commentEx`, `commentId`, `commentExtensible` entries.
  - Freed ids are not reused (Layer 3), and `people.xml` entries stay.
  - Removal of registry entries follows PR #14's interim rule; for these parts the entries are
    content, not OPC registries.

#### 3.4 Gates

| Gate | Effect on comments |
|---|---|
| W1 (`trackRevisions`) | Does not apply. Comments are not tracked changes, and Word records them while tracking. |
| W3 (`documentProtection`) | `comments` and `none` allow comments; `readOnly`, `forms` and `trackedChanges` refuse them *(u: whether Word permits comments under `trackedChanges`)* |
| W2 (fields) | Text column: anchors add no text, so no field result changes |
| R1–R4 | Apply to the runs at the range boundaries, because they may be split. A range may span runs inside fields and revisions as long as no split falls inside them. |

#### 3.5 Styles by name

The comment styles are looked up by their built-in **name** (`annotation text`,
`annotation reference`, `footnote reference`, `footnote text`, …), never by style id. Localized
documents use ids such as `a5`, `af0` and `Caratteredellanota` (spike: notes). If the style does
not exist, it is created through ADR-0011 §8 with Word's built-in definition. Open question 4
covers whether these definitions come from an owner-provided Word 365 document.

### 4. Footnotes and endnotes

```rust
impl Document {
    pub fn add_footnote(&mut self, at: RunPosition, paragraphs: Vec<String>) -> Result<NoteId>;
    pub fn add_endnote(&mut self, at: RunPosition, paragraphs: Vec<String>) -> Result<NoteId>;
}
```

- **Reference run** at `at`: `w:rStyle` = the reference style (by name, §3.5), plus
  `w:footnoteReference w:id`.
- **Note.** `w:footnote w:id` with one paragraph per string:
  - each paragraph has `pStyle` = the footnote-text style;
  - the first starts with a run with the reference style and `w:footnoteRef`, then a run with a
    space and the text, as Word writes it *(u)*.
- **Ids:** max + 1 over the part (separators included) and over every reference in every story.
  The separators are never touched.
- **Missing part.** Created with Word 2010+'s separator convention: `separator` id `-1`,
  `continuationSeparator` id `0` *(u: which convention current Word writes; both occur)*.
  `settings.xml` also gains `w:footnotePr` (or `w:endnotePr`) listing them, in schema order. The
  corpora always have it when the part exists.
- **Gates:** W1 and W3 apply, as for any text edit. W2 uses the structure column, because notes
  renumber and shift pagination. R1–R4 apply at `at`.
- **Not in Phase3:** custom marks (`w:customMarkFollows`), changing note numbering properties,
  and converting between footnotes and endnotes.

### 5. Tracked changes (generation)

#### 5.1 Mode

```rust
let mut t = doc.track_changes(Author::new("Reviewer"), date)?; // a session
t.set_run_text(p, r, "new")?;     // del old + ins new
t.append_paragraph("text")?;      // ins of runs and of the paragraph mark
t.set_run_properties(p, r, ...)?; // rPrChange
t.insert_rows(...)?;              // trPr/ins
```

- Inside a session, **W1 is lifted**: the edits are tracked, which is what the author expects
  when `trackRevisions` is on.
- Outside a session W1 still refuses.
- A session does not turn `trackRevisions` on or off.

#### 5.2 What is written

| Edit | Markup | Notes |
|---|---|---|
| Replace a run's text | `w:del w:id w:author w:date` around the old run, whose `w:t` becomes `w:delText` and whose `instrText` becomes `delInstrText`; `w:ins` around a new run with the same `rPr` | The old run keeps its rsids and `rPr`. Only its text elements are renamed. A rename re-renders that run (T2-local). |
| Insert text, runs or a paragraph | `w:ins` around new runs. A new paragraph also gets `w:pPr/w:rPr/w:ins` (paragraph-mark insertion). | Paragraph-mark revisions: POI 20, SDK 56 |
| Delete text | `w:del` around the runs, with `delText` | Splits at run boundaries (ADR-0011 §9.2) |
| Change run formatting | `w:rPrChange w:id w:author w:date` holding a copy of the old `rPr` (`detach_as`, minus an existing `rPrChange`) | Refused if the run already has an `rPrChange` from another author *(Word merges; Phase3 does not)* |
| Change paragraph properties | `w:pPrChange` with the old `pPr`, minus `sectPr`, `rPr` and `pPrChange` | |
| Insert or delete table rows | `w:trPr/w:ins` or `w:trPr/w:del` | |

- **Attributes.**
  - `w:id` comes from the annotation union (§2).
  - `w:author` is the session's author and must be non-empty.
  - `w:date` is UTC `YYYY-MM-DDTHH:MM:SSZ`.
  - rsids and `w16du:dateUtc` are not written; the latter was never seen in the corpora.
- **Still refused:**
  - edits inside existing revisions (R2), including a session's own earlier revisions, because
    Word would merge them *(u)*;
  - moves (`moveFrom`/`moveTo`);
  - numbering changes;
  - section property changes;
  - accept and reject, which are later in the roadmap (Q3) because of moves, numbering and
    paragraph-mark merges.
- **W2 still applies** per ADR-0007/0011 columns. A tracked edit changes the "all revisions
  accepted" view, which fields compute from *(u)*.
- **Reading.** `Run::revision()` reports author, date and kind. ADR-0004's "accepted view" text is
  unchanged.

### 6. Content controls (SDT)

#### 6.1 Editing existing controls

R3 is lifted for these kinds; every other kind stays refused:

| Kind (`sdtPr` child) | Operation | What changes |
|---|---|---|
| `w:text` (plain text) | `set_text` | The content becomes one run with the content's first run's `rPr`. `multiLine` decides whether `\n` maps to `w:br` or is refused. |
| no kind element (rich text) | `set_text` on a single-paragraph control; runs inside follow R1–R4 as body runs do | Most controls (SDK 1,010) |
| `w:date` | `set_date(date)` | `w:date/@w:fullDate` (ISO) and the displayed text, formatted with `w:dateFormat` and `w:lid` *(u: Word's format engine; Open question 6)* |
| `w14:checkbox` | `set_checked(bool)` | `w14:checked/@w14:val` and the glyph run from `w14:checkedState`/`uncheckedState` (font and char) |
| `w:dropDownList`, `w:comboBox` | `select(value)` | The text becomes the matching `listItem@displayText`. A combo box also accepts free text. |
| `w:picture`, `w:docPartObj`, `w:docPartList`, `w:citation`, `w:bibliography`, `w:equation`, `w:group`, `w15:repeatingSection` | — | Refused (`UnsupportedControl`) |

- **Placeholders.** When `w:showingPlcHdr` is present (SDK 1,673), setting a value removes it
  and replaces the placeholder runs. The new run takes `rPr` from `sdtPr/w:rPr` if present (SDK
  346), else the placeholder run's `rPr` minus `rStyle` *(u: Word drops the placeholder style)*.
- **`w:temporary`** (SDK 968): the control is kept. Removing it on edit is a UI behaviour of
  Word's, not a file rule *(u)*.
- **Locks.** `contentLocked` and `sdtContentLocked` refuse value edits (`ControlLocked`).
  `sdtLocked` allows them.
- **Gates.** W1 and W3 apply. Under `forms` protection, controls are the editable regions *(u)*,
  so `forms` allows these value edits. W2 uses the text column.

#### 6.2 Data binding

`w:dataBinding` has `w:storeItemID`, `w:xpath` and `w:prefixMappings`. Setting a bound control's
value updates the store **and** the cached content of **every** control bound to the same
(store, xpath). Readers other than Word see only the cached content; Word refreshes from the
store on open *(u)*.

| Store | Where | Phase3 |
|---|---|---|
| A custom XML part whose `itemProps` `ds:itemID` equals `storeItemID` | `customXml/itemN.xml` | Supported. The xpath must be the subset Word writes: absolute steps `/p:a[1]/p:b[1]`, with prefixes from `prefixMappings`, optional `[n]` positions and no other predicates. The target must be one element whose content is text; it is replaced, and nothing else in the part changes. Anything else is `UnsupportedBinding`. |
| `{6C3C8BC8-F283-45AE-878A-BAB7291924A1}`, core properties | `docProps/core.xml` | Supported for text properties (`dc:title`, `dc:subject`, `dc:creator`, `cp:keywords`, `dc:description`, `cp:category`, `cp:contentStatus`). This is the most frequent binding (SDK 401). It interacts with ADR-0004 §5's stale `core.xml` policy; Open question 7. |
| `{6668398D-A668-4E3E-A5EB-62B293D839F1}`, extended properties | `docProps/app.xml` | Read only (`UnsupportedBinding` on write) *(u: which properties Word binds)* |
| Any other store, or no matching part | — | Refused |

- Bindings with `w15:dataBinding` (rich-text binding, Word 2013+) are refused.
- Creating new custom XML stores is not in Phase3.

#### 6.3 Creating controls

`wrap_in_control(range | paragraphs, ControlSpec { kind: PlainText | RichText | Date | Checkbox
| DropDown(items), tag, alias, lock, binding: Option<Binding> }) -> SdtId` does the following:

- It wraps existing runs (inline) or whole paragraphs (block) in a new `w:sdt` with `w:id` (§2),
  `w:tag` and `w:alias`, and the kind element.
- It adds no placeholder: the current text is the content.
- A binding may target only an existing store (§6.2), and the store's current value must equal
  the wrapped text, else `BindingMismatch`.
- Row and cell controls are not created.
- The kind elements in `w14` need `w14` declared in scope. `declare_namespace` on created nodes
  covers that (ADR-0011 §1.3).

### 7. Inserting fields

```rust
impl Document {
    pub fn insert_field(&mut self, at: RunPosition, f: FieldSpec) -> Result<()>;
}
pub enum FieldSpec {
    Page, NumPages, SectionPages,
    Date { format: String }, Time { format: String },
    DocProperty { name: String },            // allow-list of ADR-0004 rev 4
    Ref { bookmark: String, hyperlink: bool }, PageRef { bookmark: String, hyperlink: bool },
    Seq { identifier: String },
    Toc { levels: (u8, u8), hyperlinks: bool },
}
```

- **Always complex fields.** `fldChar begin`, then `instrText xml:space="preserve"` with the
  instruction, `fldChar separate`, the result runs, and `fldChar end`.
  - Complex fields are what Word writes, outnumbering `fldSimple` 658 to 118 in the SDK corpus.
  - They hold a multi-run result, and they are what ADR-0004's scanner and every reader handle.
- **The instruction** is built from the enum, never from caller text. Arguments are quoted with
  straight quotes, and `"` and `\` inside them are refused. Switches come from a fixed set, so
  W2's classifier always classifies the field the same way it was meant (ADR-0004 rev 4).
- **The cached result** is what the field shows until updated. For `Page`, `NumPages` and
  `SectionPages` it is the caller's number or `1`.
  - `Date`/`Time` are computed with `format`, using the same formatter as §6.1.
  - `DocProperty` takes the property's current value.
  - `Ref`/`PageRef` take the bookmark's current text, or `?` for `PageRef`.
  - `Seq` takes the count of earlier `SEQ` fields with the identifier plus 1.
  - `Toc` gets a placeholder paragraph.
  - The bookmark must exist (`Ref`/`PageRef`).
- **Freshness.** Every inserted field whose code is content-dependent under W2 (`Ref`, `PageRef`,
  `Seq`, `Toc`) gets `w:dirty="true"` on its begin `fldChar`.
  - The operation also requires the caller's `updateFields` opt-in (ADR-0007). Without it, the
    operation is refused, because the result would be stale by construction.
  - `w:dirty` alone is not relied on: it was never seen in the corpora, and Word's handling of it
    is *(u)*.
- **Gates.** W1 and W3 apply. W2: inserting a safe field is a text edit. Inserting a
  content-dependent field turns the document into one W2 refuses for later untracked edits,
  unless `updateFields` stays set. That is the intended consequence and is documented on the
  operation.
- **Not in Phase3:** `IF`, `=`, mail-merge fields, `INCLUDETEXT`, form fields, and editing
  existing field instructions.

### 8. Effective formatting (read-only resolver)

```rust
impl Run<'_> { pub fn effective(&self) -> Result<EffectiveRun>; }
impl Paragraph<'_> { pub fn effective(&self) -> Result<EffectiveParagraph>; }
pub struct EffectiveRun { pub bold: bool, pub italic: bool, pub size_half_points: u32, pub fonts: Fonts, pub color: Color, pub underline: Underline, pub caps: bool, pub small_caps: bool, pub strike: bool, pub vanish: bool, pub vert_align: VertAlign, pub highlight: Option<Highlight>, pub lang: Langs, /* … */ pub provenance: Provenance }
```

Each property records the level that set it (`provenance`), for debugging and for the
differential tests.

#### 8.1 Order

The order follows the ECMA-376 Part 1 style hierarchy *(§17.7.2; section numbers from memory,
u)*. Each later level overrides the earlier:

1. **Document defaults:** `docDefaults/rPrDefault`, `pPrDefault`. When they are absent, Word's
   application defaults apply *(u: Times New Roman 10 pt; Open question 8)*. The corpora always
   have them when a styles part exists, and 6 template files have only `rPrDefault`.
2. **Table style** of the enclosing table:
   - its `pPr`/`rPr`;
   - then the conditional `tblStylePr` blocks that apply to the cell. These are computed from
     the cell position and `tblLook` (`firstRow`, `lastRow`, `firstColumn`, `lastColumn`,
     `noHBand`, `noVBand`, band sizes), in the order whole table → bands → first/last column →
     first/last row → corner cells *(u)*.
   - `cnfStyle` is a cache and is ignored for resolution (19 files have it).
3. **Numbering level** `pPr` (and `rPr` for the number itself, not for the paragraph's runs).
   This applies when the paragraph has `numPr` directly or through its style chain. The
   precedence of the numbering `pPr` against the paragraph style's `pPr` (indentation) is
   *(u: Word lets numbering indentation override the style's)*. It is covered by a differential
   test.
4. **Paragraph style chain,** root first (`basedOn`).
5. **Character style chain** of the run's `rStyle`, root first.
6. **Direct formatting:** `pPr`, then `rPr`.

#### 8.2 Rules

- **Toggle properties** (`b`, `bCs`, `i`, `iCs`, `caps`, `smallCaps`, `strike`, `dstrike`,
  `outline`, `shadow`, `emboss`, `imprint`, `vanish`):
  - within styles (table, paragraph and character chains), each occurrence **toggles** the
    inherited value (XOR);
  - direct formatting sets the value absolutely;
  - `w:val="0"` in a style sets false rather than toggling *(u)*.
- **Chains** are followed root first and guarded against cycles (0 observed; refused with
  `StyleCycle` rather than looping) and against depth beyond 64.
- **A missing style** referenced by `pStyle`/`rStyle` falls back to the default style of that
  type (POI: 22 references in 1 file). A `basedOn` naming a style of another type is ignored
  (1 POI file).
- **Theme fonts** (`asciiTheme`, `hAnsiTheme`, `eastAsiaTheme`, `cstheme`) resolve through
  `theme1.xml` `a:fontScheme` major/minor fonts.
  - Script-specific fonts (`a:font script="Jpan"`) are chosen through `settings/w:themeFontLang`
    *(u: Word's script selection for eastAsia)*.
  - Theme colors (`themeColor`, `themeTint`, `themeShade`) resolve through `a:clrScheme` and
    `settings/w:clrSchemeMapping`. The tint and shade arithmetic is Word's *(u)* and is covered by
    differential tests.
  - Without a theme part (8 POI, 53 template files), theme references fall back to the direct
    font attribute next to them.
- **Paragraph-mark `rPr`** (`pPr/rPr`) is resolved separately as the paragraph mark's
  properties. It never applies to the paragraph's runs.
- **Out of scope:** fields' formatting switches (`\* MERGEFORMAT`), complex-script detection per
  character (the resolver returns both the Latin and the complex-script values), and
  `stylesWithEffects.xml`, which is not read.

### 9. Gates for Phase3 operations

| Operation | W1 `trackRevisions` | W3 protection allows | W2 column |
|---|---|---|---|
| comments (add, reply, resolve, delete) | not applied | `none`, `comments` | text (no change) |
| footnotes and endnotes | applied (tracked notes are not generated in Phase3) | `none` | structure |
| tracked-change session edits | **lifted** | `none`, `trackedChanges` | per edit (text or structure) |
| SDT value edits | applied | `none`, `forms` *(u)* | text |
| wrapping in a new SDT | applied | `none` | text |
| inserting a field | applied | `none` | text for safe codes; content-dependent codes need `updateFields` |
| effective formatting | — (read) | — | — |

### 10. Allowlist (outline)

- **WML `[write]`** gains:
  - comments: `w:CT_Comments/w:comments`, `w:CT_Comment/w:comment`, `w:CT_Markup/w:commentReference`,
    `w:CT_MarkupRange/w:commentRangeStart`, `w:CT_MarkupRange/w:commentRangeEnd`,
    `w:CT_Empty/w:annotationRef`;
  - notes: `w:CT_Footnotes/w:footnotes`, `w:CT_FtnEdn/w:footnote`, `w:CT_FtnEdnRef/w:footnoteReference`,
    `w:CT_Empty/w:footnoteRef` (and the endnote counterparts), `w:CT_FtnDocProps/w:footnotePr`;
  - revisions: `w:CT_RunTrackChange/w:ins`, `w:CT_RunTrackChange/w:del`, `w:CT_Text/w:delText`,
    `w:CT_Text/w:delInstrText`, `w:CT_RPrChange/w:rPrChange`, `w:CT_PPrChange/w:pPrChange`,
    `w:CT_TrackChange/w:ins` (paragraph mark, `trPr`);
  - content controls: `w:CT_SdtRun/w:sdt`, `w:CT_SdtBlock/w:sdt`, `w:CT_SdtPr/w:sdtPr`,
    `w:CT_SdtContentRun/w:sdtContent`, `w:CT_SdtContentBlock/w:sdtContent`, `w:CT_DecimalNumber/w:id`,
    `w:CT_String/w:tag`, `w:CT_String/w:alias`, `w:CT_SdtText/w:text`, `w:CT_SdtDate/w:date`,
    `w:CT_SdtDropDownList/w:dropDownList`, `w:CT_SdtComboBox/w:comboBox`;
  - fields: `w:CT_FldChar/w:fldChar`, `w:CT_Text/w:instrText`.
- **New groups:**
  - `w14` (`w14:checkbox`, `w14:checked`);
  - `w15` (`w15:commentsEx`, `w15:commentEx`, `w15:people`, `w15:person`, `w15:presenceInfo`);
  - `w16cid` (`w16cid:commentsIds`, `w16cid:commentId`);
  - `w16cex` (`w16cex:commentsExtensible`, `w16cex:commentExtensible`).

  Whether these MS-extension schemas are in the SDK data the generator reads is checked in the
  implementation PR's first commit (Open question 9).
- **`[detach]`** gains `w:CT_RPr/w:rPr` and `w:CT_PPr/w:pPr` for `rPrChange`/`pPrChange` copies,
  each reviewed for document-unique ids (none expected).

### 11. Test oracles

- **T1–T6, T-OPC and T-REG** (ADR-0011) on every output.
- **T-CMT (comment consistency), on every output:**
  - every `commentReference`/range id names a comment;
  - every created comment has exactly one start, end and reference;
  - every `commentEx`/`commentId` `paraId` is the last paragraph of a comment;
  - every `paraIdParent` names a comment that is not itself a reply;
  - `commentsIds` and `commentsExtensible` agree on durable ids;
  - acceptance: `sdk-Comment010` with a reply added, with all five parts agreeing (roadmap).
- **T-REV:**
  - docx-rs read-back of `ins`/`del` (author, date, text) on a tracked edit of `poi-delins`
    with its `trackRevisions` kept on (roadmap);
  - the accepted view (ADR-0004 `Paragraph::text`) equals the new text, and the rejected view
    equals the old text.
- **T-NOTE:**
  - every reference resolves within its kind;
  - separators are unchanged;
  - `settings` lists the separators.
- **T-SDT:**
  - after a bound edit, every control bound to the same (store, xpath) shows the store's value;
  - the store part changed only at the target element (T2 on the custom XML part).
- **T-FLD:**
  - ADR-0004's scanner classifies every inserted field as its `FieldSpec`;
  - content-dependent fields carry `w:dirty`.
- **T-FMT (resolver differential):**
  - a hand-computed table over the fixtures and synthetic cases: XOR chains, missing styles,
    conditional table formatting, numbering indentation, theme fonts and colors.
  - LibreOffice is the reference where available; it is not available in this sandbox. The
    O3 LibreOffice workflow (PR #21) can export the same documents to compare.
- **Manual:** `manual_check` outputs for each operation in Word:
  - a reply thread;
  - a resolved comment;
  - a footnote in a document without a footnotes part;
  - a tracked replacement;
  - a bound date control;
  - a TOC with `updateFields`.

### 12. How the libraries do it (source-verified)

| Concern | python-docx 1.2.0 (`e4545460`) | Apache POI XWPF (`47dcff76`) | docx-rs 0.4.22 | ADR-0015 |
|---|---|---|---|---|
| comment id | max + 1 over `w:comment` only, first gap above 2³¹ − 1 (`oxml/comments.py:68-87`) | caller's (`XWPFComments.java:286-292`) | caller's (`Comment::new(id)`, `elements/comment.rs:59`) | annotation union max + 1 (§2) |
| comment content and anchors | `CommentText` paragraph, `CommentReference` + `annotationRef` run (`oxml/comments.py:32-61`); range start before the first run, range end + reference run after the last (`text/run.py:176-186`, `oxml/text/run.py:92-102,150-163`); date now in UTC (`comments.py:60`) | `createComments` creates the part (`XWPFDocument.java:997-1010`); anchors are left to the caller | range start/end via `Paragraph::add_comment_start/end` (`elements/paragraph.rs:236-246`) | python-docx's shape, styles by name (§3.2, §3.5) |
| replies, `commentsExtended` | none (no reference to `commentsExtended`, `commentsIds` or `people` in `src/docx`) | none | `parent_comment_id`; `commentEx` from the comment's paragraph `paraId`s (`documents/mod.rs:983-997, 2325-2345`): one entry per paragraph rather than for the last one, and a reply whose parent is not in the map gets no entry (source reading, not executed) | last-paragraph `paraId`, all companions kept in agreement (§3.2–3.3) |
| `commentsIds`, `commentsExtensible`, `people` | none | none | none | entries when the part exists |
| `w14:paraId` | none | none | thread-local counter from `00000001` (`paragraph_id.rs:1-18`) | random, absent from the scope (§2) |
| footnote/endnote ids | no notes API | count of footnotes + endnotes, then +1 while taken, **one space for both kinds** (`FootnoteEndnoteIdManager.java:41-58`) | process-global counter from 1 (`footnote_id.rs`) | max + 1 per kind over part and references |
| footnote reference | — | run with `rStyle` `FootnoteReference` by id (`XWPFParagraph.java:1932-1942`) | `Run::add_footnote_reference` (`elements/run.rs:366`) | style found by name, created if missing |
| tracked changes | none (reads ignore `w:ins` text, roadmap Q3) | reads `ins` runs (`XWPFParagraph.java:151-160`); no API to create | `Insert`/`Delete` with default author `unnamed` and date `1970-01-01T00:00:00Z` (`elements/insert.rs:69-77`); ids from a process-global counter from 0 (`history_id.rs`), not checked against existing ids | session author and date, ids from the union, paragraph-mark and `rPrChange` handled (§5) |
| content controls | none | read-only `XWPFSDT`/`XWPFAbstractSDT` (constructors and getters only, `XWPFSDT.java:29-52`) | `StructuredDataTag` with `DataBinding { xpath, prefix_mappings, store_item_id }` (`elements/structured_data_tag.rs:117-151`, `elements/data_binding.rs:9-30`); the store is not updated | value edits per kind, store and every bound control updated together (§6) |
| fields | none | `fldSimple` only (`createFieldRun`, `XWPFParagraph.java:1565-1570`; `XWPFFieldRun.setFieldInstruction`, `XWPFFieldRun.java:45`) | complex fields with an optional `w:dirty` (`elements/run.rs:194-200`, `elements/fld_char.rs:25`); typed instructions for TOC, PAGE, PAGEREF, HYPERLINK, TC (`elements/instr_*.rs`) | complex fields from a closed enum; `dirty` + `updateFields` for content-dependent codes (§7) |
| effective formatting | none: `Font.bold` reads direct `rPr` only (`text/font.py:39-48`); `style.base_style` exposes the chain (`styles/style.py:172`) | none: `XWPFRun.isBold` reads direct `rPr` (`XWPFRun.java:325-328`); `XWPFStyles.getUsedStyleList` walks `basedOn`/`link`/`next` to copy styles, not to resolve (`XWPFStyles.java:234-270`) | none: styles keep `based_on` (`elements/style.rs:22, 83`) but nothing resolves | resolver with XOR toggles, conditional table formatting and theme (§8) |

None of the three keeps the five comment parts consistent, allocates note or revision ids
against the existing file, or resolves effective formatting. Those are the parts of Phase3 with
no prior art in these libraries. The roadmap's mention of docx4j's property resolver is not
reviewed here: its source is not in the spike's pinned set (Open question 10).

## Alternatives considered

1. **Write `fldSimple` for inserted fields.** It is smaller and has no begin/end bookkeeping.
   Rejected: complex fields are what Word writes (658 to 118), they carry multi-run results,
   and POI's `fldSimple`-only API is the outlier.
2. **Create every comment companion part** (`commentsIds`, `commentsExtensible`, `people`) on
   the first comment. Rejected for now: they occur in 2–4 files, Word adds them on save *(u)*, and
   writing them means guessing Word's durable-id and presence semantics. `commentsExtended` is
   created because replies and the resolved state live only there.
3. **Comment ids from comments only** (python-docx). Rejected: comment and bookmark ids collide
   in 2 template outputs, and ADR-0011 chose the union.
4. **Track edits by rewriting the whole paragraph** as `del` of all old runs and `ins` of all
   new ones. It is simpler, but it destroys run boundaries, rsids and comment anchors, and makes
   reviews unreadable. Rejected for per-run deltas.
5. **Resolve formatting by reading `cnfStyle`.** Rejected: it is a cache present in 19 files,
   while conditional table styles occur in far more. It is computed instead.
6. **Edit bound controls' cached content only.** Rejected: Word refreshes from the store on open
   *(u)*, and the edit would be reverted visibly. Store and cache are updated together.
7. **Lift W1 for every edit while `trackRevisions` is on** (silently track). Rejected: the
   caller must choose an author and opt in to tracking; a session makes that explicit.

## Consequences

- **Positive:**
  - Review workflows (comment, reply, resolve, track an edit, fill a form) work on existing Word
    files with every id fresh and every companion part consistent.
  - The effective-formatting resolver unlocks semantic reads, such as heading detection by
    effective size and weight, that no compared library offers.
- **Positive:**
  - The registries designed in ADR-0011 (annotation union, `paraId`) get their first real
    users.
  - The core transaction is shared with xlsx and pptx.
- **Negative:**
  - Phase3 depends on the core transaction API (`core/add-part`) and on ADR-0011's registries
    and `split_run`.
  - About 40 allowlist entries, and four MS-extension namespaces whose schema data must be
    available to the generator.
- **Negative:** several behaviours are Word's and are not specified:
  - the separator convention;
  - date formatting;
  - toggle and numbering precedence;
  - theme tint and shade;
  - the semantics of `temporary`.

  They are marked *(u)*, covered by differential and manual checks, and listed as open
  questions.
- **Negative:** accept/reject, moves, nested replies, new custom XML stores and rich-text
  bindings stay out of scope.

## Open questions for the project owner

1. **Core transaction (§1):** confirm that the `core/add-part` design covers staged Layer 2
   edits of existing parts, not only additions.
2. **`w14:textId` for new paragraphs:** write the placeholder `77777777` (proposed, matching
   1,516 of 1,603 comment paragraphs) or omit it?
3. **Companion parts:** create `commentsIds`/`commentsExtensible`/`people` too (Word 365 does
   *(u)*), or only append to them when present (proposed)?
4. **Built-in style definitions** for comment and note styles: copy them from an owner-provided
   Word 365 document (preferred), or define our own?
5. **Note separators for new parts:** (`-1`, `0`) as proposed, or (`0`, `1`)? Current Word's
   choice needs checking.
6. **Date formatting** for date controls and DATE fields: implement Word's picture syntax for
   the common patterns (`yyyy/MM/dd`, `d MMMM yyyy`, Japanese era formats?) or require the
   caller to pass the display text?
7. **Core-properties binding** (§6.2): editing `docProps/core.xml` through a bound control
   conflicts with ADR-0004 §5's "leave `core.xml` as Word last saved it". Allow it for bound
   properties only (proposed)?
8. **Application defaults** when `docDefaults` is missing: Times New Roman 10 pt *(u)*, or
   refuse to resolve?
9. **MS-extension schemas** (`w14`, `w15`, `w16cid`, `w16cex`) in the generator's input: are
   they in the pinned SDK data, or do they need an override file?
10. **docx4j** as a further reference for formatting resolution (its `PropertyResolver`): add
    its source at a pinned revision to the spike before implementation?

## Sources

- **Spike:** `spikes/docx-phase3/README.md`, `review.py`, `summarize.py`, `out/tables.md`; run
  2026-11-16 with Python 3.15, over fixtures (23 open), POI (111), SDK non-Strict (440), docx4j
  (12) and templates (212).
- **Library source** (read, not executed):
  - python-docx `e45454602b53e8e572b179ccf1c91093ec9f4ed7`: `src/docx/oxml/comments.py`,
    `comments.py`, `document.py`, `text/run.py`, `oxml/text/run.py`, `text/font.py`,
    `styles/style.py`.
  - Apache POI `47dcff769bb395034dddfd59c4609659b6dbc6a4`, in
    `poi-ooxml/src/main/java/org/apache/poi/xwpf/usermodel/`: `XWPFComments`, `XWPFComment`,
    `XWPFDocument`, `XWPFParagraph`, `XWPFRun`, `XWPFFootnotes`, `XWPFEndnotes`,
    `FootnoteEndnoteIdManager`, `XWPFSDT`, `XWPFAbstractSDT`, `XWPFFieldRun`, `XWPFStyles`.
  - docx-rs 0.4.22 crate source: `src/documents/mod.rs`, `paragraph_id.rs`, `footnote_id.rs`,
    `history_id.rs`, and in `elements/`: `comment.rs`, `comment_extended.rs`, `paragraph.rs`,
    `insert.rs`, `run.rs`, `fld_char.rs`, `structured_data_tag.rs`, `data_binding.rs`,
    `style.rs`.

  Line ranges are given in §12.
- **ADRs and PRs:** ADR-0001, ADR-0002, ADR-0004 (Accepted), ADR-0007 (PR #25), ADR-0011
  (PR #31), PR #14 (`remove_part`), PR #18 (`detach_as`), PR #26 (`insert_at`), PR #21 (O3
  LibreOffice workflow), `docs/roadmap/docx.md` (Phase3, Q3, Q4).
- **Specifications:**
  - ECMA-376 Part 1: comments §17.13.4; footnotes and endnotes §17.11; revisions §17.13.5;
    structured document tags §17.5.2; fields §17.16; style hierarchy §17.7.2; toggle properties
    §17.7.3. Section numbers are from memory and marked *(u)* where a rule depends on them.
  - [MS-DOCX] for `w14`, `w15`, `w16cid` and `w16cex` (`commentsExtended`, `commentsIds`,
    `commentsExtensible`, `people`, `paraId`, `checkbox`).
