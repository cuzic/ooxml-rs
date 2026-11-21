# ADR-0015: Word Phase3: review features (comments, notes, tracked changes, content controls, fields, effective formatting)

- Status: **Accepted** (revision 4; `0015-opus-review-round3.md` approved acceptance after Minor only)
- Date: 2026-11-16
- Depends on:
  - [ADR-0001](0001-lossless-round-trip-architecture.md): layers, T1/T2, Layer 3's
    append-only invariant.
  - [ADR-0002](0002-schema-driven-code-generation.md): slots, primitives, allowlist, and the
    post-acceptance notes (depth limit; detach/insert, PR #18).
  - [ADR-0004](0004-docx-phase0.md): Word Phase0, Accepted. Gates W1–W3, R1–R4.
  - [ADR-0007](0007-docx-phase1-template-filling.md) (Word Phase1, Accepted rev 3): stories
    beyond the body, the per-operation W2 table, and `updateFields`.
  - ADR-0011 (Word Phase2, PR #31, Accepted rev 4, not yet merged):
    - the OPC part plan (§1);
    - the Word registries (§2), including the annotation-id union and the `w14:paraId`
      registry, which is designed there but not activated;
    - the `sectPr` copy and move procedure that lifts A2 (§6);
    - run splitting and paragraph insertion (§9);
    - the structure column of W2 (§10).
  - The core registry transaction of PR #38 (`core/add-part`): `RegistryPlan`, `stage_part` and
    `Package::apply` (§1).
  - PR #45 (`core/insert-with-ns`): inserts that declare namespaces on the new element.
  - **New in revision 3 (N2):** a small core API for text replacement in schemaless parts,
    `XmlDocument::set_leaf_text` with its `check_` twin (§6.2), as its own core PR.
- Evidence:
  - `spikes/docx-phase3/` (`review.py`, `summarize.py`, README with every number).
  - Corpora: fixtures (23 open), Apache POI (111), Open XML SDK non-Strict (440), docx4j (12),
    template-engine files (212).
  - Library behaviour from source at pinned revisions (§12, Sources).

## Revision history

- **Revision 1:** initial proposal.
- **Revision 2** (after `0015-opus-review-round1.md`):
  - **B1 (spike namespaces).** Revision 1's spike read `commentsExtended` and `people.xml` only in
    the 2012 `w15` namespace. Most files, including the fixture `sdk-Comment010`, use the Office
    2013 **pre-release** namespace `…/word/2010/11/wordml`. The spike now reads both. Every
    comment claim is redone:
    - `commentsExtended` is complete, not partial;
    - there are 469 replies, 52 of them replies to replies;
    - `done` is per comment;
    - 430 of 503 people match exactly.

    Writes go only to 2012-namespace companion parts; a pre-release part makes replies and
    resolve refuse with `LegacyCommentsExtended` (§3.2). The acceptance fixture becomes
    `poi-testComment`; `sdk-Comment010` becomes the refusal test (roadmap updated).
  - **M1 (paragraph-mark insertion).** A tracked paragraph appended after a paragraph marks the
    **previous** paragraph's mark as inserted, as Word does; never the last paragraph of a
    container (§5.2).
  - **M2 (table rows).** Tracked row insertion and deletion also wrap the row's runs in
    `ins`/`del` (with `delText`) and mark every paragraph mark (§5.2).
  - **M3 (existing property changes).** A run or paragraph that already has `rPrChange` or
    `pPrChange` is refused, and ADR-0004's R2 is extended to all nine revision kinds, so the
    original snapshot is never overwritten (§5.2).
  - **M4 (toggle properties).** Toggle properties follow [MS-OI29500] 2.1.258: overriding
    inheritance along `basedOn`, a reset at the paragraph style, a toggle by the character
    style, absolute direct formatting. Revision 1's XOR along chains is withdrawn (§8.2).
  - **M5 (core-properties bindings).** A bound-control write to `docProps/core.xml` requires the
    `updateFields` opt-in when a field reads the same property, else `StaleFieldRisk`. The
    binding statistics are corrected (§6.2).
  - **Minor findings:**
    - m1: reply anchor ordering;
    - m2: resolve applies to the thread;
    - m3: `delete_comment` details, orphan ids in the union, T-CMT as a delta;
    - m4: session high-water marks and `AnnotationIdsExhausted`;
    - m5: the annotation-union evidence;
    - m6: `paraId` scan scope;
    - m7: `mc:Ignorable`;
    - m8: refusals for tracked replacement;
    - m9: SDT details;
    - m10: resolver details and oracles;
    - m11: notes created in pairs, separators, and cross-references.
  - **§1** is rewritten against PR #38's `RegistryPlan`.
  - **Owner questions:** the reviewer's recommendations are recorded as proposed answers.
- **Revision 3** (after `0015-opus-review-round2.md`: 2 Major, 8 Minor):
  - **N1 (paragraph-mark takeover).** A tracked paragraph inserted after P takes **P's
    original `pPr` verbatim** (`sectPr`, `numPr` and the mark's `rPr` included). P keeps a copy
    without `sectPr` and `pPrChange`, with `ins` first in its mark `rPr`. Rejecting restores the
    input; accepting gives Word's Enter shape. A caller asking for other formatting on the new
    paragraph is refused in a session. T-REV compares the rejected view's `pPr` sequence, not
    only text (§5.2, §11).
  - **N2 (store edits).** There is no untyped write in core, and ADR-0004 Alternative 9 rejected
    an untyped `LensMut`. Phase3 requests one narrow core API instead:
    `XmlDocument::set_leaf_text(path, value)` and `check_set_leaf_text`, which replace the text
    of one element without element children, addressed by namespace URI, local name and
    position. It changes no structure and no attribute (§6.2). §1 and Q1 no longer say "no core
    API beyond PR #38 and PR #45".
  - **Minor findings:**
    - n1: `delete_comment` is refused on pre-release `commentsExtended` too;
    - n2: the revision kinds are ADR-0011 rev 2 §2's list, and an ADR-0004 post-acceptance note
      records the extended R2;
    - n3: numbering overrides the paragraph style ([MS-OI29500] 2.1.229), toggle defaults
      ([MS-OI29500] 2.1.230);
    - n4: `mc:Ignorable` checked by URI and actual prefix, `mc` declared, `NotDetachable`;
    - n5: `w:date` local time with `Z`, reference-only parents, the `commentsExtended`
      relationship and content types, *(u)* on row insertion, the read view of deleted rows;
    - n6: allowlist rows;
    - n7: `RegistryPlan` has no `add_related_part`; no `xml_part_mut` while planning; PR #38's
      M2/M7 fixes are prerequisites;
    - n8: editorial.

- **Revision 4** (after `0015-opus-review-round3.md`: Minor only; **Accepted**):
  - **r1:** N's `pPr` is a `sectPr`-less detached copy plus the `sectPr` **moved** by ADR-0011
    §6's procedure (references re-set through typed setters), not a verbatim `detach_as` copy,
    which refuses `r:id`. Cross-reference fixed to ADR-0011 §6. `NotDetachable` (e.g. an
    Ignorable-namespace element in the mark `rPr`) refuses the append.
  - **r2:** allowlist: `w:CT_ParaRPr/w:rPr` moves to `[write]`; `[detach]` uses ADR-0011's
    `w:CT_PPr/w:pPr` and `w:CT_SectPr/w:sectPr`; a P without `pPr` gets `pPr` and `rPr` created.
  - **r3:** the merge-on-reject behaviour is *(u)*; T-REV's rejected view is defined; a Word
    manual check (Reject All after appends behind a section-break paragraph and a heading).
  - **r4:** `set_leaf_text` is closed to schemaless parts in code (`TypedNamespace`), with
    `set_text`'s dirty flags, `NilElement`, and store parse failures mapped to
    `UnsupportedBinding`.
  - **r5:** P with revisions → `InsideRevision`; "following paragraph" = next sibling `w:p`;
    N's `pPr` in the first-in-container case; consecutive inserts; headings' `w:next`.
  - **r6:** `tblPrExChange` added to ADR-0011 §2's list (rev 4) and to the annotation union.
  - **r7:** how 2.1.230's even-count rule composes with the style rules *(u)*, with a T-FMT case.
  - **r8:** offset-carrying date types, Consequences cleanup, ADR-0007 link, PR #38's fixes
    (`958c8ac`).

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

- **Comments** (both `w15` namespaces counted; revision 2):
  - Range start, range end and a reference run anchor 1,195 of 1,215 SDK comments.
  - **Two namespaces for the same parts.** `commentsExtended` uses the pre-release
    `…/word/2010/11/wordml` in 54 of 59 SDK files and in `sdk-Comment010`; the 2012 `w15` in 5 SDK
    files, `poi-testComment` and 4 template files. `people.xml` is pre-release in all 25 SDK files.
  - `commentsExtended` is complete: 1,089 of 1,090 entries point at a comment's **last**
    paragraph, and only one comment lacks an entry.
  - Replies are common: 469 in 43 SDK files, 52 of them replies to replies.
  - `done` is per comment: parent and reply differ in 48 cases.
  - Replies carry their own anchoring. The reply's start follows the parent's start (460). Its
    end and reference follow the parent's reference (421 of 461).
  - `people.xml`: 430 of 503 comment authors match a person exactly.
- **`w14:paraId`:** on 90% of comment paragraphs, but on 5% of body paragraphs. Duplicates occur
  in 17 files.
- **Revisions:**
  - dates always `YYYY-MM-DDTHH:MM:SSZ` when present;
  - paragraph-mark revisions are common. Outside tracked table rows they never sit on the last
    paragraph of a container;
  - tracked row deletions wrap every run in `w:del` and mark every paragraph mark;
  - ids never collide with comment or bookmark ids in Word-written files.
- **Notes:**
  - two separator id conventions, (`-1`, `0`) and (`0`, `1`);
  - footnote and endnote id spaces are separate;
  - reference styles often have localized ids.
- **Content controls:**
  - 463 SDK data bindings, by store:
    - core properties: 161;
    - extended properties: 82;
    - cover-page properties: 166, of which 141 have no custom XML part in the package;
    - other: 54;
  - negative and duplicate `w:id`s.
- **Fields:**
  - complex fields dominate (658 to 118);
  - 114 complex fields have no cached result;
  - `w:dirty` was never seen.
- **Formatting:**
  - `basedOn` chains up to 5 deep;
  - toggle properties in styles in hundreds of files;
  - numbering-level `rPr` in 164 files;
  - conditional table formatting is frequent, while the cached `cnfStyle` is rare (19 files);
  - 357 `tblLook` elements carry only the hex `w:val`;
  - every theme-colored `w:color` in the stories caches its RGB (1,230 SDK), which serves as an
    oracle.

## Decision

### 1. Prerequisite: the core registry transaction (PR #38)

Phase3 operations edit **existing** parts (the story, `comments.xml`, `settings.xml`, a custom
XML part, `docProps/core.xml`) and **add** parts and registry entries in the same step. PR #38
provides that transaction, and Phase3 uses it as follows.

1. **Plan.** `let mut plan = RegistryPlan::new(&pkg)`. Every step is validated against the
   package plus the earlier steps, and a refused step leaves the plan unchanged.
2. **Edits of existing parts.** For each part to edit, take a clone of the tree
   (`pkg.xml_part(name)?.clone()`) and edit it with the lens. The lens refuses before changing
   anything (`check_*` twins, PR #45; slots; depth limit). Then stage it with
   `plan.stage_part(&pkg, name, doc)`.
3. **New parts and relationships.**
   - New parts are added with `plan.add_part` (`comments.xml`, `footnotes.xml` and
     `endnotes.xml`, `commentsExtended.xml`), and each is related with `plan.add_relationship`.
     `RegistryPlan` has no `add_related_part` (that is `Package::add_related_part`, outside a
     plan; n7). Content types follow from `add_part`.
4. **Apply.** `pkg.apply(plan)` cannot fail except `StalePlan`, which arises when the package
   changed since the plan started. ooxml-docx holds `&mut Document` for the whole operation, so
   that cannot happen from outside. Between `RegistryPlan::new` and `apply`, ooxml-docx reads
   parts only through `Package::xml_part` / `parsed_part`, never `xml_part_mut`, which touches
   the package and would make the plan stale (n7). Dropping the plan is the validate-only twin
   of the whole operation.
5. **Registries.** ooxml-docx's own registries (§2) hand out values while planning. They advance
   their high-water marks **only when `apply` succeeds**. A plan that is dropped releases nothing
   that anything could reference.

**Core prerequisites (revision 3, N2 and n7):**

- PR #38 with its round-1 fixes **M2** (staging a tree that is not dirty) and **M7** (a new
  `rId` must not pick up a dangling `r:id` left in a part), both fixed in `958c8ac` (r8). Phase3 creates relationships from
  `document.xml` to `comments`, `footnotes`, `endnotes` and `commentsExtended`, which is the M7
  case. PR #38's M3 and M4 do not affect Phase3: it calls neither `remove_parts` nor
  `copy_relationships`.
- PR #45 (`*_with_ns`).
- One new core API, `XmlDocument::set_leaf_text` (§6.2), for the custom XML and core-properties
  stores. Everything else goes through the typed lens.

### 2. Registries (activating and extending ADR-0011 §2)

| Registry | Scope scanned | Allocation | Evidence |
|---|---|---|---|
| **Annotation ids** (ADR-0011 rev 2): comments, revisions (every kind of ADR-0011 rev 4 §2, plus `tblPrExChange`; n2, r6), bookmarks, permissions, move ranges | the union of **definitions and references** over every story, both branches of every `mc:AlternateContent`, text boxes, and `comments.xml`. Ids that are only referenced (dangling anchors) count. | max + 1 over the union and the **session high-water mark**, so an id freed earlier in the session is never handed out again (m4). `AnnotationIdsExhausted` when max + 1 would exceed 2³¹ − 1 (ADR-0011). | Cross-kind collisions: 0 in Word-written files (SDK: 72 files with comments and bookmarks, 11 with bookmarks and revisions, 8 with comments and revisions), so the union matches Word's single id space (m5). Collisions only in 2 docxtemplater outputs. |
| **`w14:paraId`** (designed in ADR-0011, activated here) | every paragraph and row in every story, both AC branches, text boxes and the glossary document; plus every `paraId`/`paraIdParent` in `commentsExtended` and `commentsIds` (both namespaces) | random value in `[1, 0x7FFFFFFF]` absent from the scope, redrawn on collision ([MS-DOCX] 2.6.2.3) | Duplicates occur in 17 files, mostly AC Fallback copies or template loops. They are tolerated and avoided. |
| **`w14:textId`** | — | `77777777`, always together with a `paraId` ([MS-DOCX] 2.6.2.4) | 1,516 of 1,603 comment paragraphs that have a `paraId` |
| **Durable ids** (`w16cid:durableId`) | `commentsIds.xml`, `commentsExtensible.xml` | random value in `[1, 0x7FFFFFFF]` absent from both | 0 values ≥ 0x80000000 |
| **Note ids** | per kind: `footnotes.xml` (separators included) plus `footnoteReference` in every story; likewise endnotes | max + 1 per kind and session high-water mark. Footnotes and endnotes are separate spaces; ids are never reused. | Equal footnote and endnote ids in 11 files. Separators are (`-1`, `0`) or (`0`, `1`); ADR-0011's note rows agree. |
| **SDT ids** (`sdtPr/w:id`) | every story | random signed 32-bit value, non-zero, absent from the scope | negative values in 16 files, duplicates in 6 |
| **People** | `people.xml` (2012 namespace only, §3.2) | appended when a new author is absent, by exact string | 430 of 503 authors match exactly |

Every scan is lazy (ADR-0011), and every value is checked against its scope again at validation
(T-REG). Existing duplicates are never repaired.

### 3. Comments

#### 3.1 Operations

```rust
impl Document {
    pub fn add_comment(&mut self, range: TextRange, c: NewComment) -> Result<CommentId>;
    pub fn reply(&mut self, parent: CommentId, c: NewComment) -> Result<CommentId>;
    pub fn set_resolved(&mut self, thread: CommentId, done: bool) -> Result<()>;
    pub fn delete_comment(&mut self, id: CommentId) -> Result<()>;   // with its replies
    pub fn comments(&self) -> impl Iterator<Item = CommentRef<'_>>; // author, date, text, parent, done, anchor
}
pub struct NewComment { pub author: String, pub initials: Option<String>, pub date: Option<LocalDateTime>, pub paragraphs: Vec<String> }
pub struct LocalDateTime { pub wall_clock: NaiveDateTime, pub utc_offset_minutes: i32 } // local time and its offset (§3.2, r8)
```

- **`TextRange`** is a start and an end position at run boundaries in one story, possibly in
  different paragraphs. Runs are split as needed with ADR-0011 §9.2's `split_run`, and each split
  must pass R1–R4.
- **Reading** resolves `commentsExtended` in **both** namespaces (2012 and the 2010/11
  pre-release). It reports the parent at any depth, since replies to replies occur (52), and
  `done` **per comment**. A comment without an entry has no parent and is not done.

#### 3.2 What `add_comment` writes, in one transaction

- **Story.** In this order:
  1. `w:commentRangeStart w:id` before the first run of the range;
  2. `w:commentRangeEnd w:id` after the last run;
  3. right after the end, a run with `w:rStyle` = the comment-reference style (§3.5) and
     `w:commentReference w:id`.

  This is the shape of 1,195 of 1,215 SDK comments, and the shape python-docx writes (§12).
- **`comments.xml`.** `w:comment w:id w:author w:date w:initials` with one `w:p` per paragraph:
  - each paragraph has `pStyle` = the comment-text style, `w14:paraId` and `w14:textId`;
  - the first paragraph starts with a run with the reference style and `w:annotationRef`.

  If the part is missing, it is created through §1: `word/comments.xml`, a relationship from the
  main part, and an `Override`. Its root declares `w` and `w14` and lists `w14` in
  `mc:Ignorable` (m7).
- **Companion parts, by namespace (B1).**

  | `commentsExtended` in the package | `add_comment` | `reply`, `set_resolved` | `delete_comment` (n1) |
  |---|---|---|---|
  | absent | creates it in the 2012 namespace with a `w15:commentEx w15:paraId=<last paragraph> w15:done="0"` entry | work on the new part | works |
  | present, 2012 namespace | appends the entry | work | works, removing the entries |
  | present, 2010/11 pre-release namespace | writes **no** entry; the comment works without one, and readers tolerate the absence | refused: `LegacyCommentsExtended` | refused: `LegacyCommentsExtended`. Almost every comment there has a pre-release `commentEx`, and leaving it would point at no paragraph (T-CMT) |

  A created `commentsExtended` part is related from the main part with type
  `http://schemas.microsoft.com/office/2011/relationships/commentsExtended` and has content type
  `application/vnd.openxmlformats-officedocument.wordprocessingml.commentsExtended+xml`, as in
  `poi-testComment` (n5).

  Writing pre-release-namespace entries is rejected, because whether Word 2013 RTM and later
  read that namespace is *(u)*. A second `commentsExtended` relationship is never created next
  to an existing one.
  - **`commentsIds` / `commentsExtensible`:** an entry with a new durable id is written only if
    the part exists and uses the namespace this ADR knows (`w16cid`, `w16cex`). Neither part is
    created.
  - **`people.xml`:** `w15:person w15:author` with `w15:presenceInfo w15:providerId="None"
    w15:userId=<author>` is appended only if the part exists in the 2012 namespace and no person
    has exactly that author. The part is not created.
- **Dates (revision 3, n5).** Word writes `w:date` as **local wall-clock time with a `Z`
  suffix**, and the true UTC in `w16cex:dateUtc` (`poi-testComment`: `w:date="2021-05-20T10:57:00Z"`,
  `dateUtc="2021-05-20T02:57:00Z"`). ooxml-docx does the same *(u)*:
  - `w:date` is the caller's local date-time (`NewComment::date` carries both the local
    wall-clock time and its UTC offset), else the process's local time, written
    `YYYY-MM-DDTHH:MM:SSZ`;
  - `w16cex:dateUtc` is the UTC value, written only where a `commentsExtensible` entry is written.

  Revisions' `w:date` (§5.2) follows the same rule.
- **Markup Compatibility (m7, n4).** Adding `w14:paraId` to an original paragraph, or `w15`/`w14`
  elements anywhere, requires the **namespace URI** to be bound in scope to a prefix that is
  listed in `mc:Ignorable` on the part's root. The check goes URI → in-scope prefix → that
  prefix's presence in `mc:Ignorable`, never by a prefix string: PR #45's `NsDecl` reuses an
  in-scope prefix for the URI, or renames a requested prefix that is bound to another URI
  (`w141`, `w152`), and in `sdk-Comment010` `w15` is bound to the pre-release namespace.
  Otherwise the operation is refused (`NotIgnorable`), since changing the root's start tag is an
  original-element edit. Created parts and created elements declare the prefix **and `mc`**
  through `NsDecl` (PR #45; `set_attr` of `mc:Ignorable` needs `mc` in scope), and list in
  `mc:Ignorable` the prefix **actually chosen** for the URI.

#### 3.3 Replies, resolve and delete

Adding a comment or replying is refused if a new range boundary or an existing thread anchor,
respectively, is inside `mc:AlternateContent`.

- **`reply(parent, …)`** writes a new `w:comment` and its `commentEx` with `w15:paraIdParent` =
  the parent's last-paragraph `paraId`.
  - **Anchors** follow the corpus shape (m1):
    - the reply's range start goes after the last existing range start of the thread (parent or
      sibling replies) that is adjacent to the parent's start;
    - its range end and reference run go, as a pair, after the reference run of the thread's last
      reply, or after the parent's reference run if there is none;
    - **a parent with a reference run only** (no range; SDK 18 comments) gets a reply with a
      reference run only, placed after the parent's (or the last reply's) reference run. The
      corpora have 7 such replies (n5).
  - **A parent without a `paraId`** on its last paragraph gets one: an attribute edit of that
    original paragraph. It is refused (`ParentNotAddressable`) unless `w14` is declared and
    ignorable on the `comments.xml` root (§3.2).
  - **Replies to replies are not created in Phase3** (`NestedReply`). Reading handles any depth.
- **`set_resolved(thread, done)`** writes the same `w15:done` to the parent and every reply of the
  thread (m2), creating missing entries under the namespace rule of §3.2 and the `paraId` rule
  above *(u: Word 365's behaviour; manual check)*. Reading still reports `done` per comment.
- **`delete_comment(id)`** removes:
  - the comment and its replies from `comments.xml`;
  - their `commentEx`, `commentId` and `commentExtensible` entries;
  - their range start and end;
  - their `commentReference` elements. The run holding a reference is removed only if nothing
    but `rPr` is left in it (m3).

  It is refused when an anchor lies inside a revision (`w:ins`/`w:del`, SDK: 51 references),
  because that would edit a revision (R2). Freed ids are not reused (§2's high-water mark), and
  `people.xml` entries stay.

#### 3.4 Gates

| Gate | Effect on comments |
|---|---|
| W1 (`trackRevisions`) | Does not apply. Comments are not tracked changes, and Word records them while tracking. |
| W3 (`documentProtection`) | `comments` and `none` allow comments; `readOnly` and `forms` refuse them. `trackedChanges` is *(u)*; it is refused until the manual check answers it. |
| W2 (fields) | Text column: anchors add no text, so no field result changes. |
| R1–R4 | Apply to the runs at the range boundaries, because they may be split. |

#### 3.5 Styles by name

The comment and note styles are looked up by their built-in **name**, never by style id:
`annotation text`, `annotation reference`, `footnote text`, `footnote reference`,
`endnote text`, `endnote reference`. Localized documents use ids such as `a5`, `af0` and
`Caratteredellanota` (spike: notes).

If the style does not exist, it is created through ADR-0011 §8:

- **Until the owner provides a Word 365 document** (Open question 4): minimal definitions, with
  the id derived from the name (e.g. `CommentText`, `CommentReference`, `FootnoteText`,
  `FootnoteReference`).
- **Afterwards:** definitions copied from that document.

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
- **Ids:** max + 1 per kind over the part (separators included), over every reference in every
  story, and over the session high-water mark. The separators are never touched.
- **Missing part.**
  - Footnotes and endnotes are created **as a pair**: both parts exist together in 162 of 162 SDK
    files.
  - Separators use the (`-1`, `0`) convention: `separator` id `-1`, `continuationSeparator`
    id `0`. Template outputs and Word 2010+ write it *(u)*.
  - `settings.xml` gains `w:footnotePr` and `w:endnotePr` listing the separators, in schema
    order, as the corpora always have.
- **Gates:** W1 and W3 apply, as for any text edit. W2 uses the structure column, because notes
  renumber and shift pagination. R1–R4 apply at `at`.
- **Not in Phase3:** custom marks (`w:customMarkFollows`), changing note numbering properties,
  and converting between footnotes and endnotes.

### 5. Tracked changes (generation)

#### 5.1 Mode

```rust
let mut t = doc.track_changes(Author::new("Reviewer"), date)?; // a session; date: Option<LocalDateTime> (§3.1)
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
| Replace a run's text | `w:del w:id w:author w:date` around the old run and `w:ins` around a new run with the same `rPr`. In the old run, **every** `w:t` becomes `w:delText` and every `w:instrText` becomes `w:delInstrText`, at any depth in the run. | The old run keeps its rsids and `rPr`. Renaming re-renders that run, which is T2-local. Refused (m8) when the run holds anything but text: `commentReference`, `footnoteReference`, `endnoteReference`, `drawing`, `fldChar`, `sym`, `object` (`RunNotTextOnly`, as R4). |
| Insert text or runs | `w:ins` around the new runs | |
| Insert a paragraph N after paragraph P (M1, N1) | `w:ins` around N's runs. **P's** paragraph mark becomes inserted: `w:ins` as the **first** child of `P/w:pPr/w:rPr`, since `CT_ParaRPr` puts `ins`/`del`/`moveFrom`/`moveTo` before `EG_RPrBase`. N takes over P's original mark, **with its properties** (the rule below). When N is first in its container, or follows a table, N's own mark is inserted instead: N's `pPr` is then only `w:rPr` holding that `ins`, and N gets no properties from P. This requires N's **next sibling to be a `w:p`**; a table, several tables or a block `w:sdt` after N refuses (`NoFollowingParagraph`; e.g. a body that ends with a table and then `w:sectPr`, written by other tools; r5). If P has no `pPr` (POI 5,427, SDK 3,784 paragraphs), `pPr` and its `rPr` are created for the `ins` (r2). | Matches Word. Outside tracked rows, the corpora never have a paragraph-mark revision on a container's last paragraph. Rejecting the change removes N's text and P's inserted mark; P and N merge, and the merged paragraph is expected to keep N's mark, which carries P's original `pPr`, so the input comes back *(u: ECMA-376 does not say which mark survives; manual check, r3)*. |
| Delete a paragraph (D3c implementation) | `w:del` in the paragraph mark and around each run, with `w:t` → `w:delText` | Requires a following paragraph in the same container. Refuses a paragraph carrying `sectPr` to preserve its section break. |
| Delete text | `w:del` around the runs, with `delText` | Splits at run boundaries (ADR-0011 §9.2) |
| Change run formatting | `w:rPrChange w:id w:author w:date` holding a copy of the old `rPr`, made with `detach_as` as `CT_RPrOriginal` | Refused when the run already has an `rPrChange` (M3) |
| Change paragraph properties | `w:pPrChange` with the old `pPr` as `CT_PPrBase`: without `sectPr`, `rPr` and `pPrChange` | Refused when the paragraph already has a `pPrChange` (M3) |
| Insert table rows (M2) | `w:trPr/w:ins` on each new row, `w:ins` around every run in its cells, and `w:ins` in every paragraph mark of its cells | `trPr/ins` and the marks match the SDK's tracked rows; wrapping the runs in `ins` is by symmetry with deletion *(u: the one inserted row in the corpora has no text run; n5)* |
| Delete table rows (M2) | `w:trPr/w:del` on each row, every run wrapped in `w:del` with `w:t` → `w:delText`, and `w:del` in every paragraph mark of its cells | All 3 SDK files with tracked deleted rows have this shape. ADR-0004's accepted view (`Paragraph::text`) then excludes the row's text, because the text is `delText` inside `w:del`. ADR-0007's table read view (`tables()`, `CellRef`) still lists the row, as it lists every `w:tr` (n5); `Row::is_deleted()` reports `trPr/del`. |

- **Which mark properties go where (revision 3, N1).** The paragraph properties (`pPr`:
  `sectPr`, `numPr`, the mark's `rPr`, everything) belong to the paragraph mark. When N takes
  over P's mark:
  - **N's `pPr` carries P's original properties** (r1): a `detach_as` copy of P's `pPr`
    **without `sectPr`** (PR #18 refuses a subtree with `r:` attributes, and the
    `headerReference`/`footerReference` of a `sectPr` have `r:id`: SDK 38 of 174 paragraph
    `sectPr`, templates 15 of 37), plus P's `sectPr` **moved** to N by ADR-0011 **§6**'s
    procedure, the one that lifts A2: copy without the references, check each `r:id` against the
    relationships, set the references again through typed setters, remove the original. The
    references are moved, never duplicated. When PR #44 (same-part move) is merged, the whole
    original `pPr` is moved to N instead, and P's new `pPr` is the `sectPr`-less copy. Either
    way N carries P's `sectPr` and numbering, and the section break stays at the end of the
    inserted text, as with Word's Enter.
  - **P's `pPr` becomes the `sectPr`-less copy**, with `ins` first in its mark `rPr`.
  - Any `NotDetachable` from the copy (e.g. an element of an Ignorable namespace in the mark
    `rPr`: POI 12, SDK 7) refuses the append, before any write.
  - **P with revisions is refused** (r5): `InsideRevision` when P carries `pPrChange`, a
    paragraph-mark revision (`ins`, `del`, `moveFrom`, `moveTo`, `rPrChange` in the mark `rPr`)
    or a `sectPrChange` (SDK 2 `pPrChange`; mark revisions POI 22, SDK 48). Nothing of P's is
    dropped or copied with its `w:id`.
  - Rejecting removes P's inserted mark; the merged paragraph is expected to keep N's mark, i.e.
    P's original `pPr`, section break included *(u, r3)*. T-REV's **rejected view** is defined
    accordingly: remove every paragraph-mark `ins` written by the session, merging that
    paragraph with the next and keeping the **next** paragraph's `pPr`; remove inserted runs.
    Accepting gives Word's Enter shape, except for headings: Word's Enter applies the style's
    `w:next` (SDK 2,633, POI 66 paragraphs have a style whose `next` is another style), while N
    keeps P's style. So "Word's Enter shape" holds only when `w:next` names the style itself or
    is absent.
  - **Consecutive inserts** at the start of a container: the second is refused as touching the
    session's own earlier revision (extended R2). The API documentation says so.
  - A session call that asks for **other** properties on N (a style id, ADR-0011 §9.1's
    `sectPr`-less copy) is refused (`TrackedParagraphFormatting`). Writing those through a
    `pPrChange` on N would need Word's handling of a `pPrChange` on a taken-over mark, which is
    *(u)*.
- **ADR-0004's R2 is extended (M3, n2).** A run or paragraph inside, or carrying, any revision
  kind of **ADR-0011 rev 4 §2's list, plus `tblPrExChange`** (`CT_TblPrExChange` has a `w:id`
  but is missing from that list; r6) is refused for every Phase0–3 edit. That list covers the
  run and paragraph kinds (`ins`, `del`, `moveFrom`, `moveTo`, `rPrChange`, `pPrChange`,
  `numberingChange`, `sectPrChange`, the paragraph-mark `ins`/`del`), the table kinds
  (`trPr/ins|del`, `tblPrChange`, `tblPrExChange`, `trPrChange`, `tcPrChange`,
  `tblGridChange`, `cellIns`, `cellDel`, `cellMerge`) and the range kinds (`move*Range*`,
  `customXml*Range*`). The corpora have no table property revision, but Word writes them when
  table formatting changes under track changes, and ADR-0011's row and cell edits must not pass
  over them.
  - A run inside a paragraph that carries `pPrChange` is refused too: the paragraph is the
    revision's scope.
  - A second change can therefore never overwrite the original snapshot that rejecting must
    restore. Keeping the snapshot and updating only id, author and date is the alternative;
    Word's behaviour there is *(u)*, so it is not done.
  - This changes Accepted ADR-0004's behaviour: Phase0's `set_run_text` newly refuses a run
    carrying `rPrChange` (Phase1's `replace` already does, ADR-0007 §1.1). The implementation
    PR adds an ADR-0004 post-acceptance note recording it.
- **Property snapshots and `mc:Ignorable` (n4).** `detach_as` refuses a subtree with an element
  in a namespace the ancestors list as Ignorable (`NotDetachable`). A run whose `rPr` holds such
  an element (`w14:ligatures`, …; 10 runs in 4 files, `p3_rpr_ext.py`) therefore cannot get an
  `rPrChange`, and a formatting change on it in a session is refused with that error.
- **Attributes.**
  - `w:id` comes from the annotation union (§2).
  - `w:author` is the session's author and must be non-empty.
  - `w:date` is written as for comments (§3.2, n5): local wall-clock time with `Z` *(u)*.
  - rsids and `w16du:dateUtc` are not written; the latter was never seen in the corpora.
- **Still refused:**
  - edits touching existing revisions (extended R2), including a session's own earlier
    revisions;
  - moves;
  - numbering changes;
  - section property changes;
  - accept and reject, which are later in the roadmap (Q3).
- **W2 still applies** per ADR-0007/0011 columns.
- **Reading.** `Run::revision()` reports author, date and kind. ADR-0004's accepted view is
  unchanged.

### 6. Content controls (SDT)

#### 6.1 Editing existing controls

R3 is lifted for these kinds; every other kind stays refused:

| Kind (`sdtPr` child) | Operation | What changes |
|---|---|---|
| `w:text` (plain text) | `set_text` | The content becomes one run with the content's first run's `rPr`. `multiLine` decides whether `\n` maps to `w:br` or is refused. |
| no kind element (rich text) | `set_text` on a single-paragraph control; runs inside follow R1–R4 as body runs do | Most controls (SDK 1,010) |
| `w:date` | `set_date(date)` | `w:date/@w:fullDate` (ISO) and the displayed text, formatted with `w:dateFormat` and `w:lid` *(u: Word's format engine; Open question 6)*. Automatic formatting supports Gregorian calendars. A bound date control writes to the store in the form `w:storeMappedDataAs` names (`dateTime`, `date` or `text`). |
| `w14:checkbox` | `set_checked(bool)` | `w14:checked/@w14:val` and the glyph run from `w14:checkedState`/`uncheckedState` (font and char) |
| `w:dropDownList`, `w:comboBox` | `select(value)` | The **store** gets the item's `value`; the **display** is its `displayText`, or `value` when `displayText` is absent. A combo box also accepts free text. An existing `w:lastValue` is updated to the selected value or free text. |
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

`w:dataBinding` has `w:storeItemID`, `w:xpath` and `w:prefixMappings`.

- **What a write updates.** Setting a bound control's value updates the store **and** the cached
  content of **every** control bound to the same node. Readers other than Word see only the
  cached content; Word refreshes from the store on open *(u)*.
- **Node identity (m9).** Two bindings name the same node when the store matches and their
  xpaths are equal after resolving prefixes to namespace URIs (through `prefixMappings`) and
  normalizing absent `[1]` positions. The SDK core-properties bindings mix `ns0:title` and
  `ns1:title`.
- **Mixed controls.** When controls of different kinds share a node, each renders its display by
  its own kind's rule.

| Store (SDK counts) | Where | Phase3 |
|---|---|---|
| A custom XML part whose `itemProps` `ds:itemID` equals `storeItemID` (cover page 25, other 37) | `customXml/itemN.xml` | Supported. The xpath must be the subset Word writes: absolute steps `/p:a[1]/p:b[1]`, prefixes from `prefixMappings`, optional `[n]` positions, no other predicates. The target must be one element whose content is text; it is replaced, and nothing else in the part changes. Anything else is `UnsupportedBinding`. |
| `{6C3C8BC8-F283-45AE-878A-BAB7291924A1}`, core properties (161) | `docProps/core.xml` | Supported for the text properties `dc:title`, `dc:subject`, `dc:creator`, `cp:keywords`, `dc:description`, `cp:category`, `cp:contentStatus`, **under the M5 rule below**. |
| `{6668398D-A668-4E3E-A5EB-62B293D839F1}`, extended properties (82) | `docProps/app.xml` | Read only (`UnsupportedBinding` on write) |
| Cover-page properties `{55AF091B-…}` with no part in the package (141), or any other store without a part (17) | — | Refused: there is no store to write |

The supported custom XML part row includes these vendored store vocabularies.

| Vendored custom XML store vocabulary | Namespace URI |
|---|---|
| Cover page properties | `http://schemas.microsoft.com/office/2006/coverPageProps` |
| SharePoint content type | `http://schemas.microsoft.com/office/2006/metadata/contentType` |
| SharePoint InfoPath form template | `http://schemas.microsoft.com/office/2006/metadata/customXsn` |
| SharePoint long properties | `http://schemas.microsoft.com/office/2006/metadata/longProperties` |
| SharePoint property metadata | `http://schemas.microsoft.com/office/2006/metadata/properties/metaAttributes` |
| Bibliography | `http://schemas.openxmlformats.org/officeDocument/2006/bibliography` |

- **Core properties and fields (M5).** ADR-0004 classifies `TITLE`, `SUBJECT`, `AUTHOR`,
  `KEYWORDS`, `COMMENTS` and `DOCPROPERTY Title` (etc.) as safe because Phase0 never writes
  `docProps`. A bound write changes the property those fields read. So:
  - if any story (complex fields and `fldSimple`) has a field that reads the written property,
    the write requires the caller's `updateFields` opt-in (ADR-0007). Without it, it is refused
    with `StaleFieldRisk`;
  - `dcterms:modified`, `cp:lastModifiedBy` and `cp:revision` stay as Word last saved them
    (ADR-0004 §5).
- **How the store is edited (revision 3, N2).** The custom XML part and `docProps/core.xml`
  have no WML schema: custom XML is arbitrary, and the generator's input has the part types but
  not the `cp`/`dc` elements. Core offers only the typed `LensMut`; `XmlDocument::root_mut` is
  crate-private, and ADR-0004 Alternative 9 rejected an untyped `LensMut` descent. Two ways were
  weighed:
  - (a) **a narrow core API (chosen):**

    ```rust
    pub struct LeafStep<'a> { pub ns: &'a str, pub local: &'a str, pub index: usize } // index: 0-based among same-name element siblings
    impl XmlDocument {
        pub fn check_set_leaf_text(&self, path: &[LeafStep<'_>], value: &str) -> Result<()>;
        pub fn set_leaf_text(&mut self, path: &[LeafStep<'_>], value: &str) -> Result<()>;
    }
    ```

    The path starts at the root element (its first step must match the root). The target must
    exist (`NoSuchElement`) and have no element children (`NotLeaf`); `value` must be XML 1.0
    characters (`InvalidXmlChar`). Comments or processing instructions inside the target refuse
    (`NotLeaf`), and `xml:space` is left as is.
    - **Closed to schemaless parts, in code (r4).** Core keeps a static list of the namespaces
      the generator has schemas for (WML, SML, PML, DML, VML, MC and the MS extensions such as
      `w14`), next to `name.rs`'s `RELATIONSHIP_ATTRIBUTES`. If the root or any element on the
      path is in one of them, both calls refuse with `TypedNamespace`. `set_leaf_text` therefore
      cannot write text into `w:pPr` or `<w:b/>` of `document.xml`, where `set_text`'s
      `E::CONTENT == Text` check would have refused. The core PR's acceptance tests include that
      refusal on `document.xml`.
    - **Dirty flags as `set_text`:** ancestors get `subtree_dirty`; the target gets `own_dirty`
      only when it was self-closing (it needs a close tag). Otherwise its start tag stays
      verbatim (T2). `core.xml` has 3 self-closing and 1,348 open/close empty elements.
    - **`xsi:nil="true"`** on the target refuses (`NilElement`): text would make the instance
      invalid, and the API changes no attribute. 0 in the corpora.

    It adds no element and no attribute, so within schemaless parts it is the text half of
    `set_text` without the `ElementSpec`. This revisits ADR-0004 Alternative 9 only for
    **text replacement in parts without a schema**; untyped descent, insertion and attribute
    writes stay rejected. It is a core PR of its own, listed in §1's prerequisites.
  - (b) **byte splicing** of the original part, staged with `stage_part` as bytes: no core
    change, but ooxml-docx would have to locate the element in the source itself (entities,
    CDATA, namespace resolution) and guarantee T2 on its own. Rejected.

  A store that cannot be parsed (`UnsupportedEncoding` for a UTF-16 store, `DtdNotAllowed`) makes
  the bound write `UnsupportedBinding` (r4); reads of the cached content still work.

  ooxml-docx maps the binding's xpath (§6.2's subset) to `LeafStep`s by resolving its prefixes
  through `prefixMappings` and its `[n]` positions (absent = `[1]`), edits a clone of the parsed
  store with `set_leaf_text` after `check_set_leaf_text`, and stages it with `stage_part`
  (§1). `docProps/core.xml` is found through the package's core-properties relationship, not by
  its usual name (n8).
- **Not in Phase3:** `w15:dataBinding` (rich-text binding, in either `w15` namespace) is refused,
  and new custom XML stores are not created.

#### 6.3 Creating controls

`wrap_in_control(range | paragraphs, ControlSpec { kind: PlainText | RichText | Date | Checkbox
| DropDown(items), tag, alias, lock, binding: Option<Binding> }) -> SdtId` does the following:

- It wraps existing runs (inline) or whole paragraphs (block) in a new `w:sdt` with `w:id` (§2),
  `w:tag` and `w:alias`, and the kind element.
- It adds no placeholder: the current text is the content.
- A binding may target only an existing store (§6.2), and the store's current value must equal
  the wrapped text, else `BindingMismatch`.
- Row and cell controls are not created.
- The kind elements in `w14` (`w14:checkbox` with `w14:checked`, `w14:checkedState`,
  `w14:uncheckedState`) are declared on the created `w:sdt` through PR #45's `*_with_ns` inserts,
  with `w14` listed in that element's `mc:Ignorable` (m7).

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

#### Open question: date-valued DOCPROPERTY refresh (post-acceptance, #139)

Date-valued DOCPROPERTY fields are W2-safe and currently use an empty cache and
`w:dirty="true"` on the begin marker. Word's first-open behavior with that
marker alone is unverified. Manual check `53-field-docproperty-createtime.docx`
records what Word displays before and after F9. The owner will decide whether
to keep the empty cache or populate it with a date formatted as Word displays it.
LibreOffice 25.2.7.2 (`C.UTF-8`, UTC) exported this case as
`Created: 11/22/2003` in both text and PDF, without a manual field update.

### 8. Effective formatting (read-only resolver)

The scalar formatting API omits properties with child elements, such as `tabs` and `pBdr`.

```rust
impl Run<'_> { pub fn effective(&self) -> Result<EffectiveRun>; }
impl Paragraph<'_> { pub fn effective(&self) -> Result<EffectiveParagraph>; }
pub struct EffectiveRun { pub bold: bool, pub italic: bool, pub size_half_points: u32, pub fonts: Fonts, pub color: Color, pub underline: Underline, pub caps: bool, pub small_caps: bool, pub strike: bool, pub vanish: bool, pub vert_align: VertAlign, pub highlight: Option<Highlight>, pub lang: Langs, /* … */ pub provenance: Provenance }
```

Each property records the level that set it (`provenance`), for debugging and for the
differential tests.

#### 8.1 Order

The order follows the ECMA-376 Part 1 style hierarchy (§17.7.2) as Word implements it
([MS-OI29500] 2.1.229). Each later level overrides the earlier:

1. **Document defaults:** `docDefaults/rPrDefault`, `pPrDefault`. When they are absent, Word's
   application defaults apply (Times New Roman 10 pt *(u)*), reported as
   `Provenance::ApplicationDefault`; resolution is never refused for this. The corpora always
   have them when a styles part exists, and 6 template files have only `rPrDefault`.
2. **Table style** of the enclosing table:
   - its `pPr`/`rPr`;
   - then the conditional `tblStylePr` blocks that apply to the cell. These are computed from
     the cell position and `tblLook` (`firstRow`, `lastRow`, `firstColumn`, `lastColumn`,
     `noHBand`, `noVBand`, band sizes), in the order whole table → bands → first/last column →
     first/last row → corner cells *(u)*. `tblLook` is often only the hex `w:val` (357 SDK), and
     that bitmask is decoded. Bands are counted in **grid columns**, taking `gridSpan` and
     `vMerge` into account (m10).
   - `cnfStyle` is a cache and is ignored for resolution (19 files have it).
3. **Paragraph style chain,** root first (`basedOn`).
4. **Numbering level** `pPr` (and `rPr` for the number itself, not for the paragraph's runs).
   This applies when the paragraph has `numPr` directly or through its style chain. Numbering
   comes **after** the paragraph style, so its indentation overrides the style's: "Word applies
   the properties from a paragraph style … before it applies the properties from a numbering
   style" ([MS-OI29500] 2.1.229, n3). A differential test covers it.
5. **Character style chain** of the run's `rStyle`, root first.
6. **Direct formatting:** `pPr`, then `rPr`.

#### 8.2 Rules

- **Toggle properties** (`b`, `bCs`, `i`, `iCs`, `caps`, `smallCaps`, `strike`, `dstrike`,
  `outline`, `shadow`, `emboss`, `imprint`, `vanish`; M4) are resolved as follows:
  1. **Within one style chain** (`basedOn`), a derived style's value **overrides** its base's,
     as for every other property. No XOR is applied along the chain.
  2. **The paragraph style resets.** Its resolved value, if present, sets the property
     ([MS-OI29500] 2.1.258, Part 1 §17.7.8: "a value of 1 resets the property's state to True
     instead of toggling it").
  3. **The character style toggles** the value resolved so far: on `true` it flips the value,
     on `false` it leaves the value unchanged.
  4. **The table style's** interaction with the paragraph and character styles is *(u)*; it is
     settled by differential tests. Word before 2016 does not display toggle properties of table
     styles correctly ([MS-OI29500] 2.1.230 (c)), which is one more reason not to rely on old
     renderings for it.
  5. **Direct formatting** sets the value absolutely.
  6. **Document defaults (n3, [MS-OI29500] 2.1.230).** When `docDefaults` sets a toggle
     property to true, the effective value is true if and only if the number of levels that set
     it to false is even; and a level with no value takes the `docDefaults` value, not the
     property's default. T-FMT adds a synthetic `docDefaults` `b=1` case.
  7. **Composition (r7) *(u)*.** When `docDefaults` sets the toggle property to true, rule 6
     alone decides (the even-count rule over every level that sets it, styles and direct
     formatting included); otherwise rules 1–5 apply. The two can disagree: `docDefaults`
     `b=1`, paragraph style `b=0`, character style `b=0` is false under rules 2–3 and true under
     the even count. T-FMT includes exactly this case and the manual check settles it.

  The corpus cases that drive this: `[b=1 → b=0]` paragraph-style chains in 17 SDK
  Microsoft-template files and `caps` in 2. T-FMT adds synthetic cases: `[1, 1]` chains, `b` in
  both the paragraph and the character style, and `b` in both the table and the paragraph style.
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
| comments (add, reply, resolve, delete) | not applied | `none`, `comments` (`trackedChanges` *(u)*, refused for now) | text (no change) |
| footnotes and endnotes | applied (tracked notes are not generated in Phase3) | `none` | structure |
| tracked-change session edits | **lifted** | `none`, `trackedChanges` | per edit (text or structure) |
| SDT value edits | applied | `none`, `forms` *(u)* | text; a core-properties binding needs `updateFields` when fields read the property (§6.2) |
| wrapping in a new SDT | applied | `none` | text |
| inserting a field | applied | `none` | text for safe codes; content-dependent codes need `updateFields` |
| effective formatting | — (read) | — | — |

### 10. Allowlist (outline)

- **WML `[write]`** gains:
  - comments: `w:CT_Comments/w:comments`, `w:CT_Comment/w:comment`, `w:CT_Markup/w:commentReference`,
    `w:CT_MarkupRange/w:commentRangeStart`, `w:CT_MarkupRange/w:commentRangeEnd`,
    `w:CT_Empty/w:annotationRef`;
  - notes: `w:CT_Footnotes/w:footnotes`, `w:CT_FtnEdn/w:footnote`, `w:CT_FtnEdnRef/w:footnoteReference`,
    `w:CT_Empty/w:footnoteRef`, `w:CT_Endnotes/w:endnotes`, `w:CT_FtnEdn/w:endnote`,
    `w:CT_FtnEdnRef/w:endnoteReference`, `w:CT_Empty/w:endnoteRef`; in `settings.xml`,
    `w:CT_FtnDocProps/w:footnotePr`, `w:CT_EdnDocProps/w:endnotePr` and their separator
    references `w:CT_FtnEdnSepRef/w:footnote`, `w:CT_FtnEdnSepRef/w:endnote` (n6);
  - revisions: `w:CT_RunTrackChange/w:ins`, `w:CT_RunTrackChange/w:del`, `w:CT_Text/w:delText`,
    `w:CT_Text/w:delInstrText`, `w:CT_RPrChange/w:rPrChange`, `w:CT_PPrChange/w:pPrChange`,
    `w:CT_TrackChange/w:ins` and `w:CT_TrackChange/w:del` (paragraph mark, `trPr`; n6);
    `w:CT_ParaRPr/w:rPr` moves from `[read]` (ADR-0007 §8) to `[write]`, for the mark's `ins`
    (r2); `w:CT_PPr/w:pPr` is already `[write]` (ADR-0007);
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
- **`[detach]`** uses ADR-0011's `w:CT_PPr/w:pPr` and `w:CT_SectPr/w:sectPr` entries for the
  mark takeover (§5.2, r1), and gains the snapshot types of `rPrChange`/`pPrChange`, reached by `detach_as`'s
  type conversion: `w:CT_RPr` → `w:CT_RPrOriginal` and `w:CT_PPr` → `w:CT_PPrBase`. Each is
  reviewed for document-unique ids (none expected).
- **`w14` checkbox state elements** for created checkboxes: `w14:checkedState` and
  `w14:uncheckedState`.

### 11. Test oracles

- **T1–T6, T-OPC and T-REG** (ADR-0011) on every output.
- **T-CMT (comment consistency).** The checks are on the **delta** from the input: an operation
  must not newly break any of them. Inputs already contain dangling anchors (2) and an orphan
  `commentEx` (`Comment040`).
  - every `commentReference`/range id names a comment;
  - every created comment has exactly one start, end and reference;
  - every `commentEx`/`commentId` `paraId` is the last paragraph of a comment;
  - every **created** `paraIdParent` names a comment that is not itself a reply (inputs have 52
    replies to replies, which are kept; n8);
  - `commentsIds` and `commentsExtensible` agree on durable ids;
  - **acceptance:** `poi-testComment` (2012-namespace companions), with a reply added and
    resolved, and all parts agreeing.
  - **refusal test:** `sdk-Comment010` (pre-release namespace) must refuse `reply`,
    `set_resolved` and `delete_comment` with `LegacyCommentsExtended` and leave the package
    unchanged, while `add_comment` succeeds without a `commentEx`.
- **T-REV:**
  - docx-rs read-back of `ins`/`del` (author, date, text) on a tracked edit of `poi-delins`
    with its `trackRevisions` kept on (roadmap);
  - the accepted view (ADR-0004 `Paragraph::text`) equals the new text, and the rejected view
    equals the old text;
  - a tracked append at the end of the body, and inside a table cell, puts the inserted mark on
    the previous paragraph (M1);
  - **`pPr` round trip (N1):** tracked appends after a paragraph carrying `sectPr`, and after one
    carrying `numPr`. The rejected view's sequence of `pPr` (canonical, per paragraph) must equal
    the input's, not only its text; the accepted view must have the `sectPr` on the new
    paragraph;
  - a body ending with a table and `w:sectPr` refuses a tracked append there
    (`NoFollowingParagraph`);
  - tracked row deletion: the accepted view drops the row's text (M2).
- **T-NOTE:**
  - every reference resolves within its kind;
  - separators are unchanged;
  - `settings` lists the separators.
- **T-SDT:**
  - after a bound edit, every control bound to the same (store, xpath) shows the store's value;
  - the store part changed only at the target element (T2 on the custom XML part). The same
    check is the acceptance test of `set_leaf_text` in its core PR (N2), together with
    `NotLeaf`, `NoSuchElement`, and namespace-prefix variants of one path.
- **T-FLD:**
  - ADR-0004's scanner classifies every inserted field as its `FieldSpec`;
  - content-dependent fields carry `w:dirty`.
- **T-FMT (resolver differential).** These oracles need no LibreOffice (m10):
  - the RGB cached with every theme-colored `w:color` (1,230 SDK) checks the theme, tint and shade
    arithmetic;
  - the 19 files with `cnfStyle` check the computed conditional formatting;
  - the toggle cases of §8.2 are added.

  The original checks:
  - a hand-computed table over the fixtures and synthetic cases: toggle chains (§8.2), missing styles,
    conditional table formatting, numbering indentation, theme fonts and colors.
  - LibreOffice is the reference where available; it is not available in this sandbox. The
    O3 LibreOffice workflow (PR #21) can export the same documents to compare.
- **Manual:** `manual_check` outputs for each operation in Word:
  - a reply thread;
  - a resolved comment;
  - a footnote in a document without a footnotes part;
  - a tracked replacement;
  - a bound date control;
  - a TOC with `updateFields`;
  - **Reject All** (r3) after tracked appends behind a paragraph with a section break and behind
    a heading: which paragraph's `pPr` (section break, style) the merged paragraph keeps.

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
| effective formatting | none: `Font.bold` reads direct `rPr` only (`text/font.py:39-48`); `style.base_style` exposes the chain (`styles/style.py:172`) | none: `XWPFRun.isBold` reads direct `rPr` (`XWPFRun.java:325-328`); `XWPFStyles.getUsedStyleList` walks `basedOn`/`link`/`next` to copy styles, not to resolve (`XWPFStyles.java:234-270`) | none: styles keep `based_on` (`elements/style.rs:22, 83`) but nothing resolves | resolver with toggle rules per [MS-OI29500], conditional table formatting and theme (§8) |

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
8. **Write `commentEx` entries in the pre-release namespace** when the existing part uses it.
   This would make replies work on most comment-bearing files (54 of 59 SDK files). Rejected:
   whether Word 2013 RTM and later read that namespace is *(u)*, and a reply that a consumer
   cannot see is worse than a refusal. It is revisited if the owner's manual check shows that
   current Word reads it.
9. **XOR toggle properties along every style chain** (revision 1). Withdrawn: it contradicts
   [MS-OI29500] 2.1.258 and overriding inheritance (§8.2).

## Consequences

- **Positive:**
  - Review workflows (comment, reply, resolve, track an edit, fill a form) work on existing Word
    files with every id fresh and every companion part consistent.
  - The effective-formatting resolver unlocks semantic reads, such as heading detection by
    effective size and weight, that no compared library offers.
  - The registries designed in ADR-0011 (annotation union, `paraId`) get their first real
    users.
  - The core transaction is shared with xlsx and pptx.
- **Negative:**
  - Phase3 depends on PR #38 (`RegistryPlan`, with its M2 and M7 fixes), PR #45 (`*_with_ns`
    inserts) and a new core API (`set_leaf_text`, §6.2), and on ADR-0011's registries and
    `split_run`.
  - Replies, resolve and delete are refused on files whose `commentsExtended` uses the
    pre-release namespace: 54 of 59 SDK files with that part, and `sdk-Comment010`. Adding
    comments still works there.
  - About 40 allowlist entries, and four MS-extension namespaces whose schema data must be
    available to the generator.
  - A tracked paragraph appended after a heading keeps the heading's style, where Word's Enter
    would apply the style's `w:next` (§5.2).
  - Several behaviours are Word's and are not specified:
    - the separator convention;
    - date formatting;
    - toggle precedence;
    - theme tint and shade;
    - the semantics of `temporary`;

    - which mark survives a rejected paragraph-mark insertion;
    - the composition of the `docDefaults` toggle rule with the style rules.

    They are marked *(u)*, covered by differential and manual checks, and listed as open
    questions.
  - Accept/reject, moves, nested replies, new custom XML stores and rich-text
  bindings stay out of scope.

## Open questions for the project owner

Each question has the round-1 reviewer's recommendation as its proposed answer, awaiting the
owner's decision.

1. **Core transaction (§1).** Proposed: PR #38's `RegistryPlan` (with its M2 and M7 fixes), with
   `stage_part` of lens-edited clones, plus **one** new core API for schemaless stores,
   `XmlDocument::set_leaf_text` / `check_set_leaf_text` (§6.2, N2), in its own core PR.
2. **`w14:textId` for new paragraphs.** Proposed: `77777777`, always paired with a `paraId`
   ([MS-DOCX] 2.6.2.4).
3. **Companion parts.** Proposed: append only when the part exists, and only in the 2012
   namespace. Create `commentsExtended` only when no `commentsExtended` relationship exists
   (§3.2).
4. **Built-in style definitions.** Proposed: copy them from an owner-provided Word 365 document;
   until then, minimal definitions found by name, with ids derived from the names (§3.5).
5. **Note separators for new parts.** Proposed: (`-1`, `0`), with footnotes and endnotes created
   as a pair.
6. **Date formatting** (owner decision, 2026-11-20). Support Word picture tokens
   `y`, `yy`, `yyyy`, `M`, `MM`, `MMM`, `MMMM`, `d`, `dd`, `H`, `HH`, `h`, `hh`, `m`,
   `mm`, `s`, `ss`, `AM/PM`, and quoted or escaped literals. English month names are
   available for English `w:lid` values. Other tokens, including Japanese era and weekday
   names, and month names with non-English `w:lid` values return `UnsupportedDateFormat`.
   Callers can supply text with `set_date_with_display`. DATE fields remain a D3e task.
   The D3d post-acceptance note below records Gregorian calendar support and
   the handling of unquoted non-ASCII literals.
7. **Core-properties binding.** Proposed: allowed, but it requires `updateFields` when a field
   reads the written property (§6.2, M5).
8. **Application defaults** without `docDefaults`. Proposed: Times New Roman 10 pt, reported as
   `Provenance::ApplicationDefault`; never refused.
9. **MS-extension schemas** (`w14`, `w15`, `w16cid`, `w16cex`) in the generator's input: are they
   in the pinned SDK data, or do they need an override file? (Open.)
10. **docx4j** as a further formatting reference (its `PropertyResolver`): add its source at a
    pinned revision to the spike before implementation? (Open.)

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
- **ADRs and PRs:**
  - ADR-0001, ADR-0002, ADR-0004 (Accepted), [ADR-0007](0007-docx-phase1-template-filling.md)
    (Accepted rev 3), ADR-0011 (PR #31, Accepted rev 4, not yet merged);
  - PR #14 (`remove_part`), PR #18 (`detach_as`), PR #26 (`insert_at`), PR #21 (O3 LibreOffice
    workflow), PR #38 (`RegistryPlan`), PR #45 (`*_with_ns` inserts);
  - `docs/roadmap/docx.md` (Phase3, Q3, Q4);
  - the round-1 review `0015-opus-review-round1.md` and its probes; the round-2 review
    `0015-opus-review-round2.md` (probes `p1_revkinds.py`, `p2_fx.py`, `p3_rpr_ext.py`).
- **Microsoft specifications:**
  - [MS-OI29500] 2.1.258 (Part 1 §17.7.8, toggle properties in paragraph styles);
  - [MS-OI29500] 2.1.229 (Part 1 §17.7.2, style hierarchy: numbering after paragraph styles),
    https://learn.microsoft.com/en-us/openspecs/office_standards/ms-oi29500/f20aa1bd-46a8-4833-bf1c-ee47082e12ba;
  - [MS-OI29500] 2.1.230 (Part 1 §17.7.3, toggle properties: `docDefaults`, table styles),
    https://learn.microsoft.com/en-us/openspecs/office_standards/ms-oi29500/f7130225-2368-48f3-acae-a9d278d0fb25;
  - [MS-DOCX] 2.6.2.3–2.6.2.4 (`paraId`, `textId`), 2.8.3.2 (`CT_CommentsIds`).
- **Specifications:**
  - ECMA-376 Part 1: comments §17.13.4; footnotes and endnotes §17.11; revisions §17.13.5;
    structured document tags §17.5.2; fields §17.16; style hierarchy §17.7.2; toggle properties
    §17.7.3. Section numbers are from memory and marked *(u)* where a rule depends on them.
  - [MS-DOCX] for `w14`, `w15`, `w16cid` and `w16cex` (`commentsExtended`, `commentsIds`,
    `commentsExtensible`, `people`, `paraId`, `checkbox`).

## Post-acceptance note (D3a, Issue #90)

The public read interfaces are `Run::effective` and `Paragraph::effective`.
`EffectiveProperties` exposes resolved scalar WordprocessingML values by
element and attribute name, each with a `Provenance`. The scalar view omits container properties such
as `tabs` and `pBdr` and excludes revision markers from paragraph-mark formatting.
Empty on/off properties resolve to `1`. Numbering can follow `numStyleLink`
and stops when it revisits a numbering instance. The run view also
provides typed toggle and half-point size accessors. The paragraph view
returns the paragraph mark's run formatting separately. A cyclic or longer
than 64 style chain returns `StyleCycle`.

`Document::add_footnote` and `Document::add_endnote` accept a
`RunPosition` in the main document body, including table cells. Anchors in
other stories return `UnsupportedContext`. They reject empty note bodies, invalid or ambiguous part
relationships and ids, and id exhaustion before writing. A single registry
transaction commits the reference, definition, style changes, and any new
parts and separator settings. New definitions use a separate signed id
space per kind and treat zero as reserved. Missing styles have minimal
definitions under the portable built-in names (§3.5), with superscript for
new reference styles. The new note-related elements stay in the `[read]`
allowlist because it already generates the `ChildOf` edges used for typed
insertion and dedicated mutable views are unnecessary.

## Post-acceptance note (D3b, Issue #94)

The SDK input under `reference/openxml-sdk-data/schemas` includes the 2010
`wordml`, 2012 `wordml`, 2016 `wordml_cid`, and 2018 `wordml_cex` schemas.
D3b uses the existing WML allowlist group and requires no schema overrides.

`Document::add_comment` takes an inclusive run range via `TextRange::new`.
`TextRange::with_offsets` takes UTF-8 byte offsets in the boundary runs,
with an exclusive end. The shared Phase2 splitter preserves run properties.
The range may cross paragraphs in the main body, including table cells.
Other stories and text boxes are refused pending manual verification of Word support.
Comments can be read with
`Document::comments`, which returns text, author, initials, date, parent,
resolution and the anchor elements. The reader accepts 2012 and pre-release
commentsExtended metadata.

Comment edits stage the story, definitions, styles and companions through a
single registry plan. Existing non-ignorable extension sites are refused.
A new paragraph can declare its own compatibility namespaces if the
comments root has no usable binding. Original parent paragraphs must have
an ignorable binding before a paragraph id can be added. New comment
paragraphs pair a fresh `paraId` with `textId="77777777"`. Missing styles
use the built-in names and sizes 20 (annotation text) and 16 (annotation
reference), in half-points for both script settings.

Comments and replies leave the field text unchanged. They are allowed while
tracking is enabled and with `none` or `comments` document protection.
`trackedChanges` protection is refused pending the Word manual check.
For deletion, a durable id is followed through commentsIds when a mapping
exists. Entries in commentsExtensible with no corresponding mapping are
preserved because the package does not identify their owning comment.

Resolution accepts any comment id in a thread and follows the parent chain
to the root before setting the thread's state. A missing or cyclic parent
chain is refused. New commentsExtensible entries require a commentsIds
mapping staged in the same edit. Comment dates require local and UTC years
in 0000–9999. Replies to block-level range starts return
`UnsupportedContext`.

## Post-acceptance note (D3c, Issue #98)

`Document::track_changes(Author, Option<LocalDateTime>)` borrows the document
for a tracked editing session. It exposes run replacement, insertion at a
`RunPosition`, UTF-8 text-range deletion, paragraph and row edits, and
direct formatting of runs and paragraphs. The session allows
`trackedChanges` protection and lifts W1 while applying the usual W2
operation column. Revision dates use the comment date writer, defaulting to the
process-local time at session start.
Each edit stages a story tree, then commits the tree and its annotation id
high-water mark together after successful validation.

`Run::revision()` reads the nearest enclosing content revision, falling
back to the run's own formatting change. `Document::table_row` returns a row view whose `Row::is_deleted` reports
a row deletion without changing table indices. Text edits,
including Phase0's `set_run_text`, check the enclosing property revisions
as well as revisions on the run. The story walk records active move and
custom XML revision ranges.

For N1, the same-part move transfers the original `pPr` from the preceding
paragraph to the new paragraph, including any section references. The
preceding paragraph receives a detached copy without `sectPr` and with
the inserted mark first in its `rPr`. A tracked paragraph deletion uses a
deleted paragraph mark and deleted runs and requires a following paragraph
in the same container. It refuses to delete a paragraph with a section
break. Inserting empty table rows uses the Phase2 row
template, so the inserted row and its paragraph marks carry revisions.
Deleted rows retain their cell content and marks in the stored XML.
Row deletion marks nested rows as well, checks each run against the
text-only allowlist, and requires at least one live row to remain in the
target table. Tracked row edits refuse move and custom XML revision ranges
on the target rows, the template row, or the row insertion position.

The SDK names the child of `pPrChange` `CT_PPrExtended`. The snapshot
writes the paragraph properties without section, paragraph-mark, or
previous-revision properties. The generator emits `SameType` only for
identical complex types. D3c uses local typed projections for detaching
the two snapshots. Existing generated public names are preserved.

## Post-acceptance note (schemaless leaf text, Issue #105)

Core implements the §6.2 (a) prerequisite independently of D3d. Both
`XmlDocument::check_set_leaf_text` and `set_leaf_text` resolve `LeafStep`
by namespace URI and local name, with zero-based indexing among matching
element siblings. The root has index zero. An empty path or a missing
step returns `NoSuchElement`. The shared preflight returns `TypedNamespace`
for a matched element in a namespace reserved for structured editing.
Codegen collects all target namespaces from the vendored SDK and
Transitional XSD schemas and excludes the custom XML store vocabularies
listed in §6.2. Core also refuses Markup Compatibility, the OPC namespaces
used by relationship parts and `[Content_Types].xml`, and any namespace
under `http://purl.oclc.org/ooxml/` (Strict). Preflight returns `NotLeaf` for
non-text children and `NilElement` when a present `xsi:nil` is neither
`false` nor `0` after XML whitespace trimming. Invalid replacement text
returns the existing `InvalidXmlChar`.

A same-value edit preserves the source spelling. For a changed value,
the target and ancestors become subtree-dirty. An originally self-closing
target becomes own-dirty when it gains text, using the serializer's
attribute-spelling preservation. An empty replacement clears the text.
Open/close tags of other parsed targets and `xml:space` are preserved.
Acceptance tests cover byte-local edits and byte-identical refusals.


### Post-acceptance note: D3d (Issue #106)

- `Document::content_controls` lists controls in story and XML order. `ControlRef`
  addresses controls independently of optional and duplicate stored IDs. Value edits
  stage the selected store and all matching stories in one `RegistryPlan`. A locked
  or unrenderable peer causes the whole edit to be refused.
- The value editor supports direct text runs in inline controls and controls with a
  single paragraph, including single-paragraph row and cell controls. Rich text can
  include hyperlinks and passive range markers. The editor reuses the first run
  and its properties and preserves the structure of such hyperlinks and markers. For active placeholders, it uses the control's run
  properties or removes the placeholder's run style from the first run.
- Input dates are naive date-times interpreted as UTC. Defaults are `M/d/yyyy`,
  `en-US`, and `dateTime` mapping. Bound checkboxes write `true` or `false`.
- For §6.1 and Open question 6, `set_date` accepts the Gregorian calendar variants
  and rejects other calendar values with `UnsupportedDateFormat`. Use
  `set_date_with_display` to supply the formatted value. Non-ASCII picture
  characters, including full-width letters such as `ｙｙｙｙ` and the date
  separators `年`, `月`, and `日`, are literals.
- The wrapper takes a same-paragraph `TextRange` or adjacent sibling paragraphs. It
  moves the selected elements within their existing part, preserving their spans.
  Checkbox wrapping (§6.3) preserves the selected text. Callers can select a run
  containing the desired glyph or call `set_checked` to refresh it.
  It refuses moves across section breaks and enclosing contexts that would change
  the meaning of the range. A private projection writes the newly created
  `sdtPr` children in Transitional XSD order. Existing generated public names stay
  stable.
- Tracked row deletion refuses `TrackedPropertyChange { element: "w:vMerge" }`
  when an untracked deletion would have to promote a surviving continuation to
  the start of its vertical merge. This preserves the existing merge in the
  rejected view and avoids an orphaned continuation in the accepted view (#99 m1).

### D3e implementation note (Issue #107)

- §7 emits five runs in the target paragraph. All five copy the direct
  anchor's run properties so Word has the same formatting on the field code
  and result when it updates the field. At paragraph end, the final element
  supplies the properties when it is a direct, unrevised run. For a standalone
  TOC placeholder, pass the end of an empty paragraph. Paragraphs inside
  block-level w:sdt or w:customXml are refused with UnsupportedContext.
- The default pagination cache is 1. The caller can set a numeric cache
  with the pagination-only insertion method. Date and time caches use the
  local clock and the shared Gregorian picture formatter with English
  month names.
- DOCPROPERTY reads matching custom values first, then built-in values
  through the package's core/extended relationships. A permitted but absent
  built-in property has an empty cache. The built-in CREATETIME,
  LASTSAVEDTIME, and LASTPRINTED properties and custom vt:filetime values
  get an empty cache and a dirty begin marker for Word to format on update.
  These properties remain W2-safe and require no session opt-in.
- REF caches the matched bookmark's current text and refuses insertion
  inside that bookmark with InvalidValue. PAGEREF has a question-mark
  cache until pagination. SEQ identifiers begin with a Unicode alphabetic
  character. Subsequent characters may be Unicode alphanumeric characters
  or underscores. Invalid identifiers return InvalidValue. The cache counts
  prior completed simple and complex fields with the same normalized
  identifier in the current story, ignoring switches such as \r, \c, and \h
  until the field is updated.
- Content-dependent insertion requires the session opt-in from ADR-0007
  even when the input settings already request updates. Those fields carry
  the dirty flag on the begin marker. Word's treatment of that flag and
  full-width date picture letters remains unverified *(u)*.
