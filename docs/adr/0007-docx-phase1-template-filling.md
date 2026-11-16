# ADR-0007: Word Phase1: template filling

- Status: **Proposed** (revision 2)
- Date: 2026-11-16
- Depends on: [ADR-0001](0001-lossless-round-trip-architecture.md) (layers, oracles, fail
  honestly), [ADR-0002](0002-schema-driven-code-generation.md) (generated views, slots,
  primitives, allowlist, overrides; post-acceptance notes on the depth limit and on
  detach/insert, PR #18), [ADR-0004](0004-docx-phase0.md) (Word Phase0, revision 4 on
  `impl/docx-phase0`, PR #13: the API, gates W1–W3, run refusals R1–R4, the field
  classification)
- Evidence: `spikes/docx-phase1/` (scripts, README with every number quoted here), run on
  five corpora: the 26 fixtures, Apache POI (129), Open XML SDK non-Strict (444), docx4j (12)
  and, new, the Word-authored test templates of two template engines (212). Library behaviour
  is from source at pinned revisions (§Sources).
- Core dependencies:
  - PR #26 (`LensMut::insert_at` / `check_insert_at`), for the child-local edits of §1.2;
  - PR #18 (`detach_as` / `insert_detached`), for §3.2;
  - the additive core and generator changes of §7 and §8.2.

## Revision history

- **Revision 1**: initial draft.
- **Revision 2** (after `0007-opus-review-round1.md`: 4 Major, 10 Minor; spike re-run):
  - **M1:** `Match` carries the matched text and the document's edit generation.
    `replace_at` with a stale match is `Err(StaleMatch)` (§7, §10).
  - **M2:** "nothing fails after the checks" is now true by construction. Every insertion a
    call will make is dry-run against the unchanged tree first, with `check_insert_at`
    (PR #26) or a proposed additive `check_insert` (§7).
    - Producers' out-of-order children are refused before any write: 9 `w:pPr` and 1
      paragraph-mark `w:rPr` in the template corpus, and `settings.xml` in 18 template files
      and 1 SDK file.
    - Duplicate property elements are refused too (§4, §6.3; new spike `slot_order.py`).
  - **M3:** a replace edits only the content children that hold matched characters
    (§1.2). Content outside the match is byte-identical: `w:noBreakHyphen`, `w:softHyphen`,
    `w:cr`, `w:lastRenderedPageBreak` and the start tags of untouched `w:t`. A new oracle
    T7 (iv) checks this.
    - Revision 1's whole-run rewrite would have changed accepted cross-run needles in
      POI 4 / SDK 8 cases (soft or non-breaking hyphens) and POI 39 / SDK 128 cases
      (`lastRenderedPageBreak`).
  - **M4:** the **style** column of the per-operation W2 table is dropped. `SEQ \s` depends
    on heading levels, and the column rescued 1 file (reviewer's Q3). The other columns were
    audited switch by switch (§6.1).
  - **m1:** a `w:hint` difference is ignored only when every matched character in that run
    is ASCII or CJK. `w:lang/@w:val` is described as proofing and hyphenation.
  - **m2:** T7's reference classifier must be written independently, and spike counts are
    reference values only. The spike was aligned with the ADR: deleted text excluded, `w:rPr`
    child order significant, ruby bases descended, Phase1 run rule used in the `xml:space`
    count. Numbers were re-run (SDK default cross-run rate 58.2% → 57.5%; templates 85.4% →
    85.2%; placeholders unchanged at 511/517).
  - **m3:** inherited superseding attributes (§4), and the attribute name `w:cstheme`.
  - **m4:** `w:fitText` refuses the mark copy.
  - **m5:** §8.2's comparison rule reworded; slot counts explained.
  - **m6:** `set_paragraph_text` specified completely.
  - **m7:** row numbering in `CellRef`.
  - **m8:** `Story::sections()` reports whether a header is displayed.
  - **m9:** `updateFields` round trip.
  - **m10:** one scope argument, case folding, `AmbiguousStyle`, the POI citation.
  - The reviewer's answers to Q1–Q10 are recorded as **proposed defaults**, pending the
    owner's confirmation (§Open questions). Two change the scope: the `IF` rule is deferred
    (Q4) and the style column is dropped (Q3).

## Context

ADR-0004 makes one thing possible: replace the text of one run in `document.xml`, or append a
plain paragraph. The roadmap's Phase1 goal is **template filling**: a person writes a template
in Word with placeholders (`{{name}}`, `«Client»`, `[DATE]`) in the body, a letterhead
header, a footer, a table, a footnote or a comment, and a program fills it in. Four things
stop Phase0 from doing that:

1. **Placeholders are split across runs.** Word splits text into runs for editing-session
   ids (rsids) and proofing marks, not only for formatting. In the template corpus, 204 of
   517 placeholders span two or more runs (`spikes/docx-phase1`, D).
2. **Only the main story is editable.** Letterheads and footers are separate parts.
3. **Cells and empty paragraphs** have no run to put text into.
4. **Small formatting and style changes** ("make the total bold", "use the existing
   'Heading 2' style") are common in the same job.

Phase1 also has to decide the two field questions ADR-0004 left open (per-operation
`StaleFieldRisk`, `IF` in mail-merge templates) and the `updateFields` opt-in.

Everything ADR-0004 established stays: the document gates (W1 `TrackRevisionsOn`, W3
`DocumentProtected`, W2 `StaleFieldRisk`), the per-run refusals R1–R4, byte-local edits
through ADR-0002 primitives, refusals before the first mutation, and **no new IDs or
parts**.

## Decision

### 0. Shape of every Phase1 write

Every text write in this ADR edits, inside runs that pass R1–R4, **only the content children
that hold the characters being replaced** (§1.2), plus at most one new `w:r` in a paragraph
that has none (§3.2). Phase0's `set_run_text` keeps its whole-run meaning: there, the caller
supplies the run's entire text. The other writes are attribute or element writes inside
`w:rPr` or `w:pPr` (§4, §5), or one element in `settings.xml` (§6). Hence:

- the text mapping (`\t` → `w:tab`, `\n` → `w:br`), the `xml:space` rules and
  `T1 InvalidXmlChar` are ADR-0004's, applied to the written pieces only;
- each run's diff stays inside its own span, its start tag and `w:rPr` byte-identical. Within
  the run, content children that hold no replaced character are byte-identical (T2, T7 (iv));
- no element is moved. Emptied runs stay in place without content, and range markers
  (bookmarks, comment ranges, permissions) never move;
- Phase1 creates no `w:id`, `w14:paraId`, `w14:textId`, rsid, relationship, content type or
  part. It writes `settings.xml` only through the opt-in of §6.3.

**Atomicity (revision 2, review M2).** Each call runs in this order:

1. the document gates;
2. the checks on every target;
3. a dry run of **every insertion** the call will make, against the unchanged tree (§7);
4. only then the mutations.

A call that touches several runs or paragraphs is atomic: either everything passes and all of
it is written, or nothing changes and the first failure is returned. This includes the
placement failures of core's `insert` (`UnresolvedSibling`, `AlreadyPresent`), which revision 1
did not check in advance.

### 1. Cross-run find and replace

#### 1.1 Text model

Matching runs on **`Paragraph::text()`** of ADR-0004 §3.1, the same string callers already
read: the "all revisions accepted" view, field results included, instructions and deleted
text excluded, `\t` for `w:tab`, `\n` for `w:br`/`w:cr`. A match never spans paragraphs.
Matching is exact and case-sensitive on Unicode scalar values, leftmost first, non-overlapping
(the semantics of `str::replace`). The replacement text is never searched again. An empty
needle is `Err(EmptyNeedle)`. A needle or replacement with an XML-forbidden character is
`Err(InvalidXmlChar)`.

Every element of the paragraph's flow is classified as one of:

| Class | Elements | In a match |
|---|---|---|
| **text run** | a `w:r` that passes R1–R4 (ADR-0004 §6) and whose rPr has no `w:rPrChange` | contributes its characters |
| **transparent** | `w:proofErr`; a **collapsed bookmark** (`w:bookmarkStart` immediately followed by the `w:bookmarkEnd` with the same `w:id`, nothing between); a text run with no text | may lie inside a match; stays where it is |
| **anchor** | other `w:bookmarkStart`/`End`, `w:commentRangeStart`/`End`, `w:permStart`/`End`, `w:moveFromRangeStart`/`End`, `w:moveToRangeStart`/`End`, `w:customXml*RangeStart`/`End` | allowed at a match's edges; strictly inside a match → `MarkerInsideMatch` |
| **boundary** | the start or end of any wrapper (`w:hyperlink`, `w:smartTag`, `w:sdt`, `w:customXml`, `w:fldSimple`, `w:ins`, `w:del`, `w:moveFrom`, `w:moveTo`, `w:dir`, `w:bdo`); a run that fails R1–R4 (field characters, field results, `w:footnoteReference`, `w:commentReference`, `w:drawing`, page breaks, …), even when it adds no text; any other child of the paragraph (`m:oMath`, `mc:AlternateContent`, unknown elements) | a match that crosses one → `MatchCrossesBoundary { boundary }` |

A **run group** is a maximal sequence of text runs with the same parent element (`w:p` or one
`w:hyperlink`), separated only by transparent and anchor elements. A match is **accepted**
when all its characters lie in one run group, no anchor lies strictly inside it, and the
formatting rule (§1.3) holds.

Collapsed bookmarks are transparent because they mark a point, not a range. Word's
`_GoBack` bookmark ("last edit position") is the usual case: 92 of 95 `_GoBack` bookmarks in
the template corpus are collapsed. Leaving one where it is puts it after the replacement
text, a valid point. Without this rule 6 template placeholders were refused.

#### 1.2 Applying an accepted match (revision 2: child-local edits, review M3)

Revision 1 rewrote every touched run as a whole with `set_run_text`. That re-encodes content
the caller did not touch:

- `w:noBreakHyphen` and `w:softHyphen` become literal U+2011 and U+00AD in `w:t`;
- `w:cr` becomes `w:br`;
- `w:lastRenderedPageBreak` is dropped.

On the accepted cross-run needles of the spike (`M3_outside_match_content`), this happens in
POI 4 and SDK 8 cases with a soft or non-breaking hyphen outside the match, and in POI 39 and
SDK 128 cases with a `w:lastRenderedPageBreak`.

Revision 2 edits **only the content children that hold matched characters**. Each content
child of a run holds a fixed slice of the run's text, by ADR-0004 §3.1's mapping:

- `w:t`: its text;
- `w:tab`, `w:br`, `w:cr`, `w:noBreakHyphen`, `w:softHyphen`: one character each;
- `w:lastRenderedPageBreak`: none.

Let the match cover characters `s..e` and let `r₀ … rₖ` be the runs holding them, with `r₀`
holding `s`.

1. A content child is **affected** if its slice intersects `s..e`. A zero-length child
   (`w:lastRenderedPageBreak`) is affected only when it lies strictly inside the match: it
   marked a page break inside text that no longer exists. At the match's edges it is kept.
2. **In `r₀`**, the affected children are replaced by a sequence:
   - the part of the first affected child before `s` (only a `w:t` can have one);
   - the replacement;
   - if `k = 0`, the part of the last affected child after `e`.

   This sequence is mapped to pieces (`w:t`, `w:tab`, `w:br`) with ADR-0004 §3.2's table.
   - If the first affected child is a `w:t` and the first piece is text, that `w:t` is kept:
     `set_text`, start tag verbatim, `xml:space` as in ADR-0004 §3.2 step 6.
   - The other pieces are created with `insert_at` (PR #26) directly after it.
   - The remaining affected children are removed (`remove_at`).
3. **In `rᵢ`, `0 < i ≤ k`**, the affected children are removed. The exception is `rₖ`'s last
   affected child: if it is a `w:t` that holds text after `e`, it is kept and `set_text` to
   that suffix.
4. Unaffected children, `w:rPr` and the run's start tag are not touched.
5. A run left with no content child keeps its element (`<w:r …><w:rPr>…</w:rPr></w:r>`;
   `CT_R` content is all optional). Run indices stay valid for the lifetime of the
   `Document`, as in Phase0.
6. **Several matches in one paragraph** are planned together and applied from the last to the
   first, so earlier child indices stay valid. Each child is written at most once.
7. **No-op:** a match whose replacement equals the matched text changes nothing.

The replacement takes **`r₀`'s formatting**: the run that holds the first matched character.
That is what both template engines do with the run holding the start delimiter (docxtemplater
`parser.js:145-157`, docxtpl `template.py:91-111`). It is also predictable from the template:
format the first character of the placeholder.

Markers at the match edges stay outside or inside the replacement as they were outside or
inside the placeholder. A bookmark that starts exactly at `s` stays before the replacement,
and one that ends exactly at `e` stays after it. A bookmark that wraps exactly a placeholder
therefore wraps its value, which is how bookmark-based templates are read back.

**`xml:space`.** Only `w:t` elements that are written (kept with `set_text`, or created) get
`xml:space="preserve"`, where ADR-0004 §3.2 step 6 requires it (edge or double spaces). An
existing one is never removed.

One case is refused. A **kept** `w:t` can have leading or trailing whitespace **without**
`preserve`, outside the match. Writing it would add the attribute, and a space Word does not
show *(u)* would become visible: a change the caller did not ask for. This is
`Err(UnpreservedWhitespace)`. It affects 3 of 517 template placeholders. ADR-0004's open
question on how Word reads such text decides whether the refusal can be lifted (proposed
default Q2).

#### 1.3 Formatting rule

The `w:rPr` of `r₀ … rₖ` are compared canonically: as element trees, ignoring namespace
prefixes, attribute order and whitespace-only text. A missing `w:rPr` equals an empty one.
`ReplaceOptions::formatting` decides:

| `RunFormatting` | Accepts | Refuses |
|---|---|---|
| `Identical` | all canonically equal | any difference → `MixedFormatting` |
| **`IgnoreInvisible`** (default) | equal after dropping `w:noProof` and `w:lang/@w:val`; and `w:rFonts/@w:hint`, but **only** when every matched character of the run whose hint differs is ASCII or CJK (revision 2, review m1) | any other difference, including `w:lang/@w:eastAsia` and `@w:bidi`, which can change glyph selection *(u)* |
| `FirstRun` | everything | — |

Why the default ignores exactly these:

- `w:noProof` and `w:lang/@w:val` choose the proofing language of Latin text, and with
  `settings/w:autoHyphenation` on, the hyphenation dictionary *(u)*. They do not change the
  glyphs drawn. They can change where a line is hyphenated, as any edit can.
- `w:rFonts/@w:hint` tells Word which font slot to use for characters that belong to two
  ranges: quotes, ellipsis, dashes, U+00A0, some punctuation. For ASCII and for CJK
  ideographs, kana, hangul, CJK punctuation and fullwidth forms the slot is fixed whatever
  the hint, so for those characters the hint is invisible.
  - The reviewer's probe found 95 SDK needles where it was not invisible: `can’t`, `IC – L`,
    and U+00A0 in 3 template needles. These are now `MixedFormatting`.
  - Word sets the hint per run in East Asian documents: 27.4% of SDK run pairs differ only
    there.
- The replacement is new text and gets `r₀`'s value of each property anyway. So the only
  question is whether the *needle* looked uniform to the template author.

| Measured (`replace_sim.py`, revision 2) | fixtures | POI | SDK | templates |
|---|---|---|---|---|
| adjacent text runs with identical `rPr` | 77.8% | 75.6% | 45.8% | 91.2% |
| … differing only in `noProof`/`lang/@val`/`hint` | 0.2% | 3.7% | 27.9% | 2.4% |
| cross-run needles accepted: `Identical` / **default** / `FirstRun` | 58.7 / **58.9** / 76.2% | 63.3 / **66.5** / 85.3% | 36.1 / **57.5** / 82.7% | 83.1 / **85.2** / 93.0% |
| template placeholders accepted (default) | — | — | — | **511 / 517 (98.8%)** |

"Cross-run needles" are the ≤6-character spans centred on every run boundary. They are the
hardest case, since an arbitrary substring is far more likely to straddle a formatting change
than a placeholder is. Of the 6 refused placeholders:

- 3 are in or across content controls;
- 1 crosses a hyperlink;
- 1 is a field result;
- 1 is a real formatting mix.

#### 1.4 Refusals

All are checked for every match before any write. With `on_refused: Refuse` (the default),
one refused match refuses the whole call: `Err(ReplaceRefused { refused: Vec<Refusal> })`,
listing every refused match (paragraph, byte range, reason), and nothing changes. With
`on_refused: Skip` the accepted matches are written and the report lists the skipped ones.
Skipping is never silent: it is opt-in and reported.

| Reason | When |
|---|---|
| `InsideField`, `InsideRevision`, `UnsupportedContext`, `RunNotTextOnly` | a matched character is in a run that fails R1–R4 (ADR-0004 §6). A run inside a `w:smartTag` stays `UnsupportedContext` (§2.5) |
| `InsideRevision` | a matched run's `w:rPr` has `w:rPrChange` (a tracked formatting change) |
| `MatchCrossesBoundary { boundary }` | the match crosses a boundary element of §1.1 |
| `MarkerInsideMatch { marker, id }` | an anchor lies strictly inside the match |
| `MixedFormatting` | §1.3 |
| `UnpreservedWhitespace` | §1.2 |
| `Core(UnresolvedSibling)`, `Core(AlreadyPresent)` | the dry run of an insertion the match needs fails (§0, §7): an out-of-order or `mc:AlternateContent`-holding run |

`replace_at` with a match from before the last edit is the call error `StaleMatch` (§7).

The document gates W1, W3 and W2 come first, as in Phase0. W2 is evaluated per operation
(§6).

#### 1.5 How other libraries do it (source-verified, §Sources)

| Library | Cross-run replace | Where the value goes | What it does to markup inside the placeholder |
|---|---|---|---|
| python-docx 1.2.0 | none; `Paragraph.text` covers only `w:r`/`w:hyperlink` children | — | the `text` setter deletes every child except `pPr` (`oxml/text/paragraph.py:52-55`) |
| Apache POI XWPF | `searchText` only (direct `w:r`, resets on `w:tab`/`w:br`, no replace; `XWPFParagraph.java:1704-1766`) | caller's job | — |
| docx-rs 0.4.22 | none (builder library) | — | — |
| docx4j `VariablePrepare` | merges adjacent runs whose marshalled `rPr` strings are equal, after optionally stripping **all** `proofErr`, bookmarks, content controls and rsids from the whole package (`VariablePrepare.java:67-112`, `filter.xslt`); since 17.3.5 also joins `${…}` across formats into the run holding `${` | first run | removed everywhere, not only in the placeholder |
| docxtemplater 3.71 | lexes the whole part's text | run holding the start delimiter | deletes `w:p`, `w:br`, bookmarks inside a tag; moves the rest after the value (`file-type-config.js:73-78`, `parser.js:145-157`) |
| docxtpl 0.20 | regex over serialized XML | run holding `{{` | deletes everything between `</w:t>` and the next `<w:t>` inside a tag, including `ins`/`del` wrappers and bookmarks (`template.py:101-111`) |

No surveyed library refuses anything. Each either rewrites well beyond the placeholder
(docx4j's package-wide filter) or silently drops or moves bookmarks, revisions and comment
anchors inside it. ooxml-rs keeps the template engines' "first run's formatting" rule and
refuses the cases they silently damage.

### 2. Headers, footers, footnotes, endnotes and comments

#### 2.1 Stories

A **story** is one part: the main document, or a part the main part relates to with type
`…/header`, `…/footer`, `…/footnotes`, `…/endnotes` or `…/comments` (ADR-0004 §2: found by
relationship, any part name; external or dangling targets skipped). Every story part is
parsed at open into Layer 2 and **kept** (Phase0 parsed and dropped them for the field scan).
An unedited story stays a dirty-free `Parsed` part and is written as a raw copy (ADR-0001 M2),
so T3 still holds for it. The glossary document is not a story (passthrough).

| Kind | Root (SDK `Name`) | Paragraph containers walked |
|---|---|---|
| main | `w:CT_Document/w:document` | `w:body` |
| header / footer | `w:CT_HdrFtr/w:hdr`, `w:CT_HdrFtr/w:ftr` | the root |
| footnotes / endnotes | `w:CT_Footnotes/w:footnotes`, `w:CT_Endnotes/w:endnotes` | each `w:CT_FtnEdn/w:footnote` (`w:endnote`) |
| comments | `w:CT_Comments/w:comments` | each `w:CT_Comment/w:comment` |

Inside a container, the walk is ADR-0004 §3.1's: tables (nested too), block-level content
controls and `customXml` are descended; text boxes, drawings and `mc:AlternateContent` are
not flow. Each paragraph records its story, and its note or comment id when it has one.

**Gates.** W1, W3 and W2 are document-wide, as in Phase0, and W2 already scans every story.
R1's field depth is one counter per story, in document order. A DTD in any story already
refuses the open (O2).

#### 2.2 Story-specific refusals

- **Separator notes.** A `w:footnote`/`w:endnote` with `w:type` `separator`,
  `continuationSeparator` or `continuationNotice` is Word's note-area furniture, referenced
  from `settings.xml`. Its runs are `UnsupportedContext { ancestor: SeparatorNote }`. Every
  notes part has them (POI 37+37, SDK 134+134).
- **Note and comment marks.** `w:footnoteRef`, `w:endnoteRef` and `w:annotationRef` make
  their run fail R4. Producers put each in a run of its own: 136 of 136 SDK notes, and every
  comment that has one (1,148 of 1,153). The text runs after the mark are settable.

Measured settable text runs (`stories_cells.py`):

| | header | footer | footnotes | endnotes | comments |
|---|---|---|---|---|---|
| POI | 40/43 | 54/65 | 42/43 | 3/4 | 8/8 |
| SDK | 141/163 | 82/166 | 312/312 | 20/20 | 1,455/1,455 |
| templates | 113/114 | 243/251 | 4/4 | 0/0 | 4/4 |

Footer refusals are `PAGE`/`NUMPAGES` results (R1), which is right: those runs are Word's
to compute.

#### 2.3 Headers and sections

A header or footer part applies to every section that references it, and, by inheritance,
to later sections that have no reference of that type *(ECMA-376 Part 1 §17.10.5, u)*.
Editing the part changes all of them. `Story::sections()` reports this read-only, as
`SectionUse { section, kind, displayed }`, so a caller can tell:

- `section` is the section's index in document order;
- `kind` is the reference type (`default`/`first`/`even`) through which the section uses the
  part after inheritance;
- `displayed` (revision 2, review m8) is false for a `first` header in a section without
  `w:titlePg`, and for an `even` header when `settings/w:evenAndOddHeaders` is off. Word keeps
  such parts but does not show them *(u)*. In the corpora no part is referenced by two `w:sectPr`
references; 1 POI part and 12 template parts are referenced by none, which makes them
invisible. They are still stories and can be edited.

#### 2.4 Cross-part consistency

Editing text inside a note or comment changes no ID:

- `commentsExtended`'s link (`w15:paraId` = the `w14:paraId` of the comment's last
  paragraph) is untouched;
- so are `commentsIds`, `commentsExtensible` and `people.xml`;
- the separator ids in `settings/w:footnotePr` are untouched.

`w14:textId` is kept unchanged, pending the same owner check as in Phase0 (ADR-0004 open
question 1).

#### 2.5 Smart tags stay refused (revises the roadmap)

The roadmap planned text inside `w:smartTag` for Phase1 "via an `add_child` override". The
SDK data has **no** entry for `w:smartTag` at all (Office 2010 dropped smart tags). So no
generated `ElementSpec` exists, an `add_child` override has nothing to add, and a typed
`LensMut` descent through it is impossible without a new untyped core primitive (ADR-0004
Alternatives 9). The cost of keeping R3 is 30 runs in POI and 358 in SDK (0.13% and 1.4% of
text runs) and 0 in the templates. Smart-tag runs stay `UnsupportedContext`, and a smart tag
is a boundary for matches. Revisit when Phase4 needs untyped descent for other reasons.

### 3. Paragraph and table-cell text

#### 3.1 `set_paragraph_text`

`set_paragraph_text(p, text, &TextOptions { formatting })` makes the paragraph's text equal
`text`. `formatting` is §1.3's `RunFormatting`, default `IgnoreInvisible`; `set_cell_text`
takes the same options (revision 2, review m6).

- **Paragraph with at least one `w:r` anywhere in its flow** (directly or in a wrapper): this is
  §1 applied to the whole text `0..len`, with `on_refused: Refuse`, and §1.2's child-local
  edit. The same groups, boundaries, anchors and formatting rule apply, so the first run's
  formatting carries the new text. If the runs hold no text, the text goes into the first
  settable run. Any element outside the matched range that is neither a text run nor
  transparent nor an anchor refuses with `ParagraphNotSimple { boundary }`.
- **Paragraph with no `w:r` at all** (only `w:pPr` and transparent or anchor elements): §3.2.
  - `""` is a no-op: no empty run is created.
  - Any other child refuses with `ParagraphNotSimple`: a `w:hyperlink` or `w:fldSimple`
    without runs, `m:oMath`, a content control, `mc:AlternateContent`.

#### 3.2 A run in an empty paragraph

A paragraph with no `w:r` gets one new run, inserted with `p.insert::<w:r>()`. That puts it
at the end of the paragraph's content slot, after any range markers (a collapsed bookmark
stays before the text).

**Its `w:rPr` is a copy of the paragraph mark's run properties (`w:pPr/w:rPr`).** That is
what Word does when a user types into an empty paragraph *(u)*, and it is where template
authors put the formatting of an empty cell. In POI, 3,097 of 3,203 empty paragraphs in cells
carry mark properties: mostly fonts and sizes, and 145 `b`. python-docx's `_Cell.text` and
POI's `XWPFTableCell.setText` create an unformatted run instead (`table.py:273-284`,
`XWPFTableCell.java:439-445`).

- The mark's type is `w:CT_ParaRPr`: `w:CT_RPr` plus `w:ins`, `w:del`, `w:moveFrom`,
  `w:moveTo`, `w14:conflictIns`, `w14:conflictDel`, `w:oMath`, and a different
  `w:rPrChange` type. The common children have the same order (dry run, §8).
- The copy is `Lens<ParagraphMarkRunProperties>::detach_as::<RunProperties>(skip)` with
  those eight QNames skipped, then `insert_detached` into the new run (ADR-0002 note,
  PR #18). This needs one additive generator rule (§8.2).

The copy is refused, and nothing is written, when the mark's `w:rPr` has:

| Error | Condition | Count |
|---|---|---|
| `InsideRevision` | `w:ins`, `w:del`, `w:moveFrom`, `w:moveTo`, `w14:conflict*` or `w:rPrChange` (a tracked paragraph mark) | SDK 1 |
| `MarkHidden` | `w:vanish`, `w:specVanish` or `w:webHidden` on: the new text would be hidden | POI 5, SDK 2, fixtures 2 |
| `UnsupportedMarkProperties` | `w:eastAsianLayout` or `w:fitText` (revision 2, review m4). Both carry a `w:id` that groups runs, so a copy would join the group. Also any child outside the `w` namespace | 0 |
| `Core(UnresolvedSibling)`, `DuplicateProperty` | the mark's children are out of slot order, or a property occurs twice: a copy would carry the defect into the run (`slot_order.py`) | templates: 1 out of order, 56 duplicates in 15 files (`w:sz` twice) |
| `Core(NotDetachable { .. })` | any PR #18 refusal (relationship attribute, MC, ignorable namespace, …) | 0 |

Everything else on the mark is copied, including formatting meant only for the mark. An
example is a spacer cell whose mark has `w:sz w:val="2"` (1 pt): text written into it is 1 pt.
That is Word's own behaviour *(u)*, and it is listed in Consequences.

#### 3.3 `set_cell_text`

Tables are listed per story in document order, nested tables included. A cell is addressed as
`CellRef { story, table, row, cell }` (revision 2, review m7):

- `row` counts **every** `w:tr` of the table in document order, including rows inside
  row-level `w:sdt`/`w:customXml`, so it matches the visual row. Those wrapped rows are then
  refused.
- `cell` counts the row's `w:tc` in order, including cells inside cell-level wrappers, which
  are refused.

`Cell` also reports, read-only:

- its grid column, counting `w:gridBefore` (read from the raw `w:trPr`, which is not
  allowlisted);
- its `gridSpan`;
- its merge state.

`set_cell_text(cell, text)` is `set_paragraph_text` on the cell's paragraph when:

| Error | Refused when |
|---|---|
| `UnsupportedContext { ancestor }` | the row or cell is inside a row-/cell-level `w:sdt` or `w:customXml` |
| `MergedContinuation` | the cell's `w:vMerge` (or legacy `w:hMerge`) is `continue` (or has no `val`). Word shows the restart cell's content for the merged area *(u)*, so text here would be hidden or surprising. The `restart` cell is editable |
| `CellNotSimple { paragraphs, tables }` | the cell holds anything but exactly one paragraph (range markers aside): several paragraphs, a nested table, a block-level content control |

Deleting surplus paragraphs is not offered: it would delete the bookmarks, comment anchors
and section breaks they carry, which is Phase2 work. A caller can call
`set_paragraph_text` on each paragraph. `\n` in `text` becomes `w:br` (one paragraph with
line breaks), as in Phase0.

| Measured (`stories_cells.py`) | fixtures | POI | SDK | templates |
|---|---|---|---|---|
| cells | 667 | 16,235 | 6,567 | 672 |
| settable | 95.2% | 84.5% | 80.1% | 84.8% |
| settable, merge continuations excluded | 95.3% | 95.5% | 83.1% | 88.0% |
| empty paragraph with mark rPr copied / without mark rPr | 4 / 2 | 1,103 / 92 | 350 / 1,234 | 4 / 14 |

The main refusals are `CellNotSimple` (POI 395, SDK 584), merge continuations (POI 1,867,
most in one file; SDK 234), content controls (SDK 410) and `MixedFormatting` (POI 198).

### 4. Direct formatting (a small subset)

Phase1 writes these properties and no others. Every setter takes a `Prop<T>`: `Keep` (the
default), `Set(T)` or `Clear` (remove the element: the value is inherited again).

**Run properties (`w:rPr`):**

| Setter | Element (SDK `Name`) | Written form | Also removed when set (they would override the value) |
|---|---|---|---|
| `bold`, `bold_cs`, `italic`, `italic_cs`, `strike` | `w:CT_OnOff/w:b`, `…/w:bCs`, `…/w:i`, `…/w:iCs`, `…/w:strike` | `true` → `<w:b/>`; `false` → `<w:b w:val="0"/>` (ADR-0004 Q5; `0` is Word's form: SDK 549 against 0 `false`) | — |
| `underline` | `w:CT_Underline/w:u` | `w:val` from `ST_Underline` (`single`, `double`, `words`, `dotted`, …, `none`) | — (`w:color` and theme attributes of the underline colour are kept) |
| `color` | `w:CT_Color/w:color` | `w:val` = `RRGGBB` upper case, or `auto` | `w:themeColor`, `w:themeTint`, `w:themeShade` (they supersede `val`, *ECMA §17.3.2.6, u*; present on 718 SDK colours) |
| `size`, `size_cs` | `w:CT_HpsMeasure/w:sz`, `…/w:szCs` | half-points as an integer, 1..=3276 *(u: Word's 1638 pt limit)* | — |
| `highlight` | `w:CT_Highlight/w:highlight` | `ST_HighlightColor` (17 values) | — |
| `vert_align` | `w:CT_VerticalAlignRun/w:vertAlign` | `baseline`, `superscript`, `subscript` | — |
| `font(slot, name)` | `w:CT_Fonts/w:rFonts` | one of `w:ascii`, `w:hAnsi`, `w:eastAsia`, `w:cs` | the same slot's theme attribute on the same element: `w:asciiTheme`, `w:hAnsiTheme`, `w:eastAsiaTheme` or **`w:cstheme`** (lower-case `t`, as in the SDK's `CT_Fonts`). It supersedes the name *(ECMA §17.3.2.26, u)*; SDK 714 `asciiTheme`, 792 `eastAsiaTheme`. `w:hint` is kept |
| `run_style` | `w:CT_String253/w:rStyle` | §5 | — |

**Paragraph properties (`w:pPr`):**

| Setter | Element | Written form | Also removed when set |
|---|---|---|---|
| `alignment` | `w:CT_Jc/w:jc` | `left`, `center`, `right`, `both`, `distribute`. Reading also accepts `start`/`end` and the others in `ST_Jc` | — |
| `space_before`, `space_after` | `w:CT_Spacing/w:spacing` | `w:before`/`w:after` in twips | `w:beforeLines`/`w:beforeAutospacing` (resp. after), which supersede twips *(u)*: SDK 35 `*Lines`, 309 `*Autospacing`; inherited ones are cancelled (rule below) |
| `line_spacing` | `w:CT_Spacing/w:spacing` | `w:line` + `w:lineRule` (`auto` in 240ths, `exact`, `atLeast`) | — |
| `indent_left`, `indent_right` | `w:CT_Ind/w:ind` | `w:left`/`w:right` in twips | `w:start`/`w:end` and `w:leftChars`/`w:startChars` (resp. right), which supersede *(u)*: SDK 315 `leftChars`; inherited ones are cancelled (rule below) |
| `first_line`, `hanging` | `w:CT_Ind/w:ind` | `w:firstLine` or `w:hanging` | the other one, and `w:firstLineChars`/`w:hangingChars`; inherited ones are cancelled (rule below) |
| `keep_next`, `keep_lines` | `w:CT_OnOff/w:keepNext`, `…/w:keepLines` | as `b` | — |
| `paragraph_style` | `w:CT_String/w:pStyle` | §5 | — |

Rules:

- **Inherited superseding attributes (revision 2, review m3).** The removals in the tables act
  on the element being written. A superseding attribute can also be **inherited** from the
  paragraph's style chain (`w:pStyle` → `w:basedOn`… → the default paragraph style →
  `w:docDefaults/w:pPrDefault`).
  - **Indents and spacing.** When the chain has a non-zero `*Chars`/`*Lines` for the value
    being set, or `*Autospacing` on, the setter also writes that attribute with `0` (or the
    on/off off form) on the direct element. That is Word's own form for cancelling an inherited
    value: SDK has 90 direct `w:firstLineChars="0"`, and 4 paragraphs where the style has
    `*Chars` and the direct `ind` only twips (`formatting_styles.py`, "inherited").
  - **Fonts and colours.** Inherited theme attributes need no cancelling. Direct `w:ascii`
    without `w:asciiTheme` below a `docDefaults` `asciiTheme` is how Word itself writes a
    direct font: POI 7,193, SDK 1,249, templates 186 runs.
  - The chain walk is read-only, follows `w:basedOn` with a cycle check, and stops at 32
    levels *(fail closed: `StyleChainTooDeep`)*. It is not the Phase3 resolver: it only
    looks for the presence of these attributes.
  - `manual_check` gains an output with a style-level `firstLineChars` plus `first_line`.
- **Independent complex-script twins.** `bold` writes `w:b` only and `bold_cs` writes `w:bCs`
  (same for `i`, `sz`). Producers do not pair them: `b` without `bCs` in 1,973 of 3,578 SDK
  runs, `sz` without `szCs` in 12,615 of 16,480. python-docx also separates them
  (`text/font.py:46-80`). docx-rs always writes both (`run_property.rs:124-134`).
- **`left`/`right` for `jc`**, not `start`/`end`. Both are in ECMA-376 5th edition
  Transitional `ST_Jc`. The first edition, which Word 2007 reads, has only `left`/`right`
  *(u)*, and `left` is the commoner form in the corpora (POI and SDK: `left` 380, `start` 96;
  the producers of the `start` files were not checked).
- **No-op.** If the element already reads as the requested value (`<w:b w:val="true"/>`
  for `Set(true)`), nothing is written and the output is byte-identical. `Clear` on an absent
  element is a no-op.
- **Placement.** `get_or_insert` on `w:rPr`/`w:pPr`, then on the property element. Core
  places both by the generated slots (`w:rPr` 51 slots, `w:pPr` 36), so the order is
  schema-correct by construction (T6). Attribute writes and removals are core's
  `set_attr`/`remove_attr`. Nothing outside the target `w:rPr`/`w:pPr` changes.
- **Producer order and duplicates, checked before writing (revision 2, review M2).**
  Transitional `CT_RPr` allows its children in any order and repeated. Core's `insert`
  refuses a parent whose existing children do not fit the slots, or that has an
  `mc:AlternateContent` child (`UnresolvedSibling`).
  - Revision 1 relied on ADR-0002's POI-only count of 0. The corpora have such parents
    (`slot_order.py`, writable files):
    - templates: 9 `w:pPr` in 5 files, all docxtemplater outputs
      (`[sectPr, pStyle, bidi, jc, rPr]`), and 1 paragraph-mark `w:rPr`;
    - POI, SDK, docx4j and the fixtures: 0 `w:rPr`/`w:pPr` out of order (about 76,000 checked).
  - Every insertion of a call is therefore dry-run first (§7). A refused parent fails the
    whole call before its first change, with `Core(UnresolvedSibling)`.
  - A property element that occurs twice in one `w:rPr`/`w:pPr` refuses with
    `DuplicateProperty`: which copy wins is not specified *(fail closed)*. Templates have 56
    duplicated mark properties in 15 files.
- **Targets.** A run passes R1, R2 and R3 and has no `w:rPrChange`. R4 is not required:
  formatting a run that holds a tab or a page break is harmless. A paragraph is targetable
  when its `w:pPr` has no `w:pPrChange`, its mark is not a revision, it is not inside a
  block-level content control, and no run of it is inside a field (a TOC entry paragraph is
  the field's result). Each failure is the corresponding R-error.
- **Scope.** Whole runs, all runs of a paragraph (`format_paragraph_runs`) and whole
  paragraphs. The paragraph mark's own run properties (`w:pPr/w:rPr`) are not written.
  Formatting part of a run would mean splitting it into new runs: Phase2 (Alternatives 9).

### 5. Existing styles by id

Phase1 reads `styles.xml` (the main part's `…/styles` relationship) into Layer 2 read-only:
it stays dirty-free and is copied raw. It never writes it.

- `Document::styles()` lists `StyleInfo { id, kind, name, is_default, linked }` from
  `w:style`.
- `style_ids_by_name(name)` matches `w:name/@w:val` **case-insensitively** and returns
  every match. Case-insensitive means equal after Unicode default lower-casing of each `char`
  (Rust `char::to_lowercase`), not only ASCII folding. Built-in names are ASCII (`heading 1`),
  and custom names may be localized (revision 2, review m10). The lookup by name is the portable one. Localized Word derives `w:styleId`
  from the UI name (`Rubrik`, `Standardowy`, `Tlotextu`), but `w:name` keeps the built-in
  English name (`Title`, `Normal Table`) for built-in styles. Measured: id and name are
  unrelated, beyond case and spaces, for 2,937 POI, 1,104 SDK and 1,915 template styles.
  python-docx also looks up by name (`styles/styles.py:31-47`).
- `set_paragraph_style(p, Some(id))` and `set_run_style(r, Some(id))` write
  `w:pStyle`/`w:rStyle` when a `w:style` with that exact `w:styleId` exists and has the right
  `w:type`: `paragraph` (an absent `type` counts as paragraph *(u)*) or `character`. `None`
  removes the element (the default style applies).

| Error | When | Measured |
|---|---|---|
| `UnknownStyle { id, latent_only }` | no `w:style` with that id. `latent_only` is set when a `w:latentStyles/w:lsdException` has that name: Word knows the built-in style but this document has no definition, and creating one is Phase2 | latent-only entries: POI 19,173, SDK 61,571 |
| `UnknownStyle` | the package has no styles part | POI 4 packages |
| `StyleTypeMismatch { expected, found, linked }` | wrong type. `linked` names the linked style, if any (`w:link`), for the common "paragraph style id given as a run style" mistake | 0 in references |

Existing references to undefined ids are left as they are (22 `pStyle` in one POI file),
never repaired. Duplicate ids, exact or case-insensitive, occur 0 times, so exact matching
is unambiguous in the corpora. A duplicate id refuses with `AmbiguousStyle { id }`
*(fail closed; revision 2, review m10)*.
Applying a style does not clear direct formatting. POI and docx-rs write any id unchecked
(`XWPFParagraph.java:1438-1442`, `paragraph.rs:254-257`).

### 6. Fields: per-operation W2 and the `updateFields` opt-in

#### 6.1 Per-operation table

ADR-0004's W2 refuses every write when any story has a field outside `fields::SAFE`.
ADR-0004 revision 3 recorded the reviewer's recommendation: a table of which codes each
operation cannot make stale. Phase1 adopts it **for three columns**. `fields.rs` gets one
column per operation class. A code is allowed for an operation if it is in `SAFE` or in that
column, and every nested field must be allowed too.

| Operation class | Additional allowed codes | Why their cached results cannot change |
|---|---|---|
| **text**: `set_run_text`, `replace`, `set_paragraph_text`, `set_cell_text` | `SEQ`, `TC`, `XE`, `NOTEREF`, `LISTNUM`, `AUTONUM`, `AUTONUMLGL`, `AUTONUMOUT` | they depend on the number and order of fields or paragraphs, on their own instruction, on note numbering, or on outline levels. Text edits change none of these: `\n` becomes `w:br`, never a paragraph, and no style or outline level is written |
| **append**: `append_paragraph` (Phase0) | the same | as ADR-0004 rev 3. Appending adds a paragraph after every field, so nothing that precedes it is renumbered |
| **format**: §4 setters | the same, plus `NUMWORDS`, `NUMCHARS`, `INFO NUMWORDS`/`NUMCHARS`, `DOCPROPERTY Words`/`Characters`/`CharactersWithSpaces`, and `=` | formatting changes no characters and no numbers, and §4's subset writes no outline level, style or hidden text. `REF` is not added: its result copies the source's formatting *(u)*. `Lines`/`Pages` statistics are not added: they depend on layout |

**The style column is dropped (revision 2, review M4; proposed default Q3).** Revision 1
allowed `SEQ` under `set_paragraph_style`/`set_run_style`. But `SEQ id \s n` restarts the
sequence after each heading of level *n*
([MS-OI29500 §17.16.5.56](https://learn.microsoft.com/en-us/openspecs/office_standards/ms-oi29500/a591c060-2800-4699-b1f4-dbb0c4374557):
"the \s switch resets the sequence following a heading of the specified level"). Applying or
removing a heading style therefore changes its results. SDK `complex2005_12rtm.docx` has 20
`SEQ Figure \* ARABIC \s 1`. The column rescued 1 file, so the style setters (§5) are gated
by the document-wide W2 of ADR-0004, as in Phase0.

**Switch audit of the three columns (revision 2).** Every allowed code was re-checked for
arguments and switches that make it depend on what the operation changes.

| Code / switch | text | append | format | Reason |
|---|---|---|---|---|
| `SEQ` (plain, `\c`, `\r n`, `\h`, `\n`, `\*`, `\#`) | yes | yes | yes | order and count of `SEQ` fields only |
| `SEQ … \s n` | yes | yes | yes | depends on headings. No allowed operation writes a style or outline level, and an appended paragraph follows every field, so it can reset nothing |
| `SEQ id bookmark` | yes | yes | yes | the `SEQ` nearest before the bookmark. Bookmarks never move in Phase1 (§0) |
| `LISTNUM` (`\l`, `\s`) | yes | yes | yes | its own list of fields; no paragraph is added before it |
| `AUTONUM`, `AUTONUMLGL`, `AUTONUMOUT` | yes | yes | yes | paragraph count and outline levels before the field: unchanged |
| `TC`, `XE` | yes | yes | yes | instruction text only (their consumers, `TOC`/`INDEX`, stay refused) |
| `NOTEREF` (`\f`, `\h`, `\p`) | yes | yes | yes | note number and relative position: no note is added or moved |
| `NUMWORDS`, `NUMCHARS`, word/character `DOCPROPERTY` | — | — | yes | formatting changes no characters |
| `=` (incl. bookmark operands, `\#`) | — | — | yes | values unchanged by formatting |

Everything else stays refused for every operation: `TOC`, `REF`, `PAGEREF`, `STYLEREF`,
`IF` (§6.2), `INDEX`, `TOA`, `CITATION`, `BIBLIOGRAPHY`, and unknown codes. Tests enumerate
the whole table, with one unit document per (code or switch, operation) cell.

**Measured yield is small** (`fields_perop.py`). Of the 33 StaleFieldRisk files (POI 1,
SDK 30, docx4j 2; templates 0), the table lifts W2 for text edits in **1** (SDK
`table.docx`, `SEQ` only) and for formatting in 6. The 19 files with `TOC` + `PAGEREF`
(18 SDK, 1 docx4j) stay refused under every operation. The table is kept because it is small
and fully enumerable (proposed default Q3).

#### 6.2 `IF`: deferred (revision 2; proposed default Q4)

`IF` stays refused in Phase1. Revision 1 allowed `IF` when every nested field was allowed and
neither side of its comparison was a bare word, a bookmark reference that an edit could
change. That needed a tokenizer over the instruction recognizing:

- quoted strings (straight and typographic);
- numbers;
- the operators `= <> < > <= >=`;
- switches;
- a placeholder per nested field;
- bare words.

An operator had to be the second token. The design is kept here for the phase that adopts it.
The reviewer recommends deferring it:

- no `IF` in the corpora passes the rule. The 5 SDK fields are four
  `IF DATE \@ "yyyy" = "1998" …`, with a bare word, and one with an unbalanced quote;
- a new tokenizer is attack surface and maintenance cost.

The mail-merge case `{ IF { MERGEFIELD Gender } = "F" "Ms." "Mr." }` is handled by the
`updateFields` opt-in until real mail-merge templates (Q6) justify the rule. Mail-merge main
documents (`settings/w:mailMerge`: 1 POI, 1 SDK) re-evaluate `IF`/`MERGEFIELD` on every
merge *(u)*.

#### 6.3 `enable_update_fields_on_open`

`Document::enable_update_fields_on_open()` is the explicit opt-in that lifts W2 for every
operation of this `Document` from then on. It writes `<w:updateFields w:val="true"/>` into
`settings.xml`.

POI's `XWPFSettings.setUpdateFields` writes `w:val="on"` (`setVal(STOnOff1.ON)`, POI
`47dcff76` `XWPFSettings.java:372-376`, re-checked in revision 2). Its Javadoc states two
things *(secondary source, u)*:

- Word asks "This document contains fields that may refer to other files. Do you want to
  update the fields in this document?" if "Update automatic links at open" is enabled;
- the flag is removed after saving with changes in Word.

[MS-OI29500 §17.15.1.90](https://learn.microsoft.com/en-us/openspecs/office_standards/ms-oi29500/d8bc14c0-5eeb-49c1-9ba7-8edf4c33976f)
also says "Word does not roundtrip the updateFields element in all circumstances" (revision
2, review m9). The owner's check (Q5) covers:

- the prompt;
- whether headers, footers and the `PAGEREF`s inside a `TOC` are updated;
- whether the element survives an update-and-save;
- Word for Mac and Word Online.

Readers other than Word keep showing the stale results, and the API documentation says so
(ADR-0004 Alternatives 4).

- **Checks:** W1 and W3 (it is a write), not W2. Exactly one settings part must exist:
  - none → `NoSettingsPart` (creating a part is Phase2);
  - two settings relationships → `AmbiguousSettings` (fail closed; ADR-0004 rev 4 reads
    both);
  - two `w:updateFields` elements → `DuplicateSetting`.
- **Write:** `get_or_insert::<UpdateFieldsOnOpen>()` under `w:settings`, placed by the
  generated 103-slot order and dry-run first (§0, §7). `settings.xml` is refused with
  `Core(UnresolvedSibling)`, before any write, in these cases:
  - its children are out of slot order. That is 18 template files (LibreOffice-written:
    `w:hyphenationZone` after `w:compat`, …) and 1 SDK file (`m:mathPr` after `w:rsids`)
    (`slot_order.py`);
  - it has an `mc:AlternateContent` child.
  - An element that already reads as on (absent `val`, `true`, `1`, `on`) is left
    byte-identical: the call only records the opt-in. 2 template files already have it.
  - Any other value is set to `true`.
- **Explicit even when already on.** A file that already has `updateFields` on does **not**
  lift W2 by itself. The caller must still opt in, because the cost (stale results for
  non-Word readers) is the caller's to accept.
- R1 is unaffected: a write *inside* a field is still `InsideField`.
- `fldChar/@w:dirty` (per-field "result is stale") is not used: it occurs 0 times in the
  corpora and Word's handling of it is unverified (Alternatives 7).

### 7. API sketch (over ADR-0002 views)

```rust
// --- stories (§2) ---
#[non_exhaustive]
pub enum StoryKind { Main, Header, Footer, Footnotes, Endnotes, Comments }
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct StoryId { kind: StoryKind, part: PartName }   // part name identifies the story
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct StoryIndex(usize);                            // position in stories(); stable for the Document

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ParaRef { story: StoryIndex, index: usize }   // index in the story's paragraphs()
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct RunRef { para: ParaRef, run: usize }
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct CellRef { story: StoryIndex, table: usize, row: usize, cell: usize }

impl Document {
    // Phase0 API unchanged: paragraphs(), paragraph(i), set_run_text(p, r, text),
    // append_paragraph(text) address the main story.
    pub fn stories(&self) -> impl Iterator<Item = Story<'_>>;
    pub fn story(&self, id: &StoryId) -> Option<Story<'_>>;

    // §1
    // find uses opts.scope and opts.formatting, so each Match's status is what replace would
    // do with the same options (revision 2, review m10: one scope argument, not two).
    pub fn find(&self, needle: &str, opts: &ReplaceOptions) -> Result<Vec<Match>>;  // Err only for EmptyNeedle/InvalidXmlChar
    pub fn replace(&mut self, needle: &str, replacement: &str, opts: &ReplaceOptions) -> Result<ReplaceReport>;
    pub fn replace_at(&mut self, m: &Match, replacement: &str, opts: &ReplaceOptions) -> Result<()>;  // StaleMatch
    pub fn generation(&self) -> u64;     // +1 after every successful write (revision 2, review M1)
    // §0, §3
    pub fn set_run_text_at(&mut self, run: RunRef, text: &str) -> Result<()>;
    pub fn set_paragraph_text(&mut self, p: ParaRef, text: &str, opts: &TextOptions) -> Result<()>;
    pub fn set_cell_text(&mut self, cell: CellRef, text: &str, opts: &TextOptions) -> Result<()>;
    // §4
    pub fn format_run(&mut self, run: RunRef, f: &RunFormat) -> Result<()>;
    pub fn format_paragraph_runs(&mut self, p: ParaRef, f: &RunFormat) -> Result<()>;
    pub fn format_paragraph(&mut self, p: ParaRef, f: &ParagraphFormat) -> Result<()>;
    // §5
    pub fn styles(&self) -> impl Iterator<Item = StyleInfo<'_>>;
    pub fn style_ids_by_name(&self, name: &str) -> Vec<&str>;
    pub fn set_paragraph_style(&mut self, p: ParaRef, id: Option<&str>) -> Result<()>;
    pub fn set_run_style(&mut self, run: RunRef, id: Option<&str>) -> Result<()>;
    // §6
    pub fn enable_update_fields_on_open(&mut self) -> Result<()>;
    pub fn write_gate_for(&self, op: Operation) -> Result<()>;   // W1, W3, W2 for that operation
}

impl Story<'_> {
    pub fn id(&self) -> &StoryId;
    pub fn paragraphs(&self) -> impl Iterator<Item = Paragraph<'_>>;   // Phase0 Paragraph, plus:
    pub fn tables(&self) -> impl Iterator<Item = Table<'_>>;           // document order, nested included
    pub fn sections(&self) -> &[SectionUse];                           // headers/footers only (§2.3)
}
impl Paragraph<'_> {                     // additions to ADR-0004 §3
    pub fn para_ref(&self) -> ParaRef;
    pub fn container(&self) -> Container; // Body, Cell(CellRef), Note { id, kind }, Comment { id }
}

pub struct Match { pub para: ParaRef, pub range: Range<usize>,      // byte range in Paragraph::text()
                   pub text: String,                                 // the matched text
                   pub generation: u64,                              // Document::generation() at find time
                   pub status: Result<(), Refusal> }
pub struct TextOptions { pub formatting: RunFormatting }            // default IgnoreInvisible
pub struct ReplaceOptions { pub scope: Scope,                       // All | Story(StoryId) | Paragraph(ParaRef)
                            pub formatting: RunFormatting,           // Identical | IgnoreInvisible (default) | FirstRun
                            pub on_refused: OnRefused }              // Refuse (default) | Skip
pub struct ReplaceReport { pub replaced: usize, pub skipped: Vec<Refusal> }

#[derive(Default)]
pub struct RunFormat { pub bold: Prop<bool>, pub bold_cs: Prop<bool>, pub italic: Prop<bool>,
    pub italic_cs: Prop<bool>, pub strike: Prop<bool>, pub underline: Prop<Underline>,
    pub color: Prop<Color>, pub size: Prop<HalfPoints>, pub size_cs: Prop<HalfPoints>,
    pub highlight: Prop<Highlight>, pub vert_align: Prop<VertAlign>,
    pub fonts: FontSlots<Prop<String>> }
#[derive(Default)]
pub struct ParagraphFormat { pub alignment: Prop<Alignment>, pub space_before: Prop<Twips>,
    pub space_after: Prop<Twips>, pub line_spacing: Prop<LineSpacing>, pub indent_left: Prop<Twips>,
    pub indent_right: Prop<Twips>, pub first_line: Prop<FirstLine>,   // FirstLine::{Indent, Hanging}
    pub keep_next: Prop<bool>, pub keep_lines: Prop<bool> }
#[derive(Default)] pub enum Prop<T> { #[default] Keep, Set(T), Clear }
```

**Implementation over ADR-0002 views:**

- **Reads** walk the raw tree, as in ADR-0004 §3.4. The body walk becomes a story walk with
  one root per container, and its frames carry the story index.
- **Writes** start from `XmlDocument::lens_mut::<R>()` with `R` the story root (`Document`,
  `Header`, `Footer`, `Footnotes`, `Endnotes`, `Comments`). They follow stored paths with
  `into_child_at` through allowlisted entries only: `root → (footnote | endnote | comment)? →
  body? → (tbl → tr → tc)* → p → hyperlink* → r`. A settable run never has another ancestor.
- **Property writes** are `get_or_insert::<RunProperties>()` /
  `get_or_insert::<ParagraphProperties>()`, then the generated `*ViewMut` of the property.
- **The mark copy (§3.2)** is `detach_as` + `insert_detached`.
- **Settings** (§6.3) is `lens_mut::<Settings>()` then `get_or_insert::<UpdateFieldsOnOpen>()`.
- **Stale matches (revision 2, review M1).** `replace_at(m)` refuses with
  `Err(StaleMatch)` unless both of these hold:
  - `m.generation == self.generation()`;
  - `Paragraph::text()[m.range] == m.text`.

  Every successful write, of any kind, increments the generation, and a refused call does
  not. A caller that replaces several matches of one `find` therefore calls `replace` with
  `Scope::Paragraph`, or calls `find` again after each `replace_at`.
- **Atomicity (revision 2, review M2).** Every call plans all its writes over the read model,
  then **dry-runs every insertion** against the unchanged tree, then writes. The writes that
  can fail in core, and how each is checked in advance:
  - **Positional inserts** (the content children of §1.2): `check_insert_at::<C>(index)`
    (PR #26). It covers the index range, the depth limit, the local slot order against the
    nearest listed neighbours, `mc:AlternateContent` neighbours, the slot limits and the
    prefix.
  - **Slot-placed inserts** (`insert` and `get_or_insert` of `w:rPr`, `w:pPr`, properties,
    `w:updateFields`, the new run of §3.2): a new additive core method
    `LensMut::check_insert::<C>()`. It is a dry run of `insert_by`'s placement and returns
    exactly the error `insert` would: `UnresolvedSibling` for producer order or an AC child,
    `AlreadyPresent`, `AmbiguousSlot`, `NamespaceNotInScope`, `TooDeep`.
    `check_get_or_insert` is `Ok` when the child exists, else `check_insert`. It is proposed
    as a one-function addition next to PR #26's `check_insert_at`. Until it lands, the domain
    computes the same index from the generated slot table and uses `check_insert_at` +
    `insert_at`.
  - **Copies** (§3.2): `Detached::check_insert_under` (PR #18).
  - **Several inserts into one parent** in one call, such as `keep_next` and `keep_lines`
    into one `w:pPr`, are each checked on the **unchanged** parent. That is sufficient
    because:
    - the parents written here lower to single-QName slots only (`w:rPr` 51, `w:pPr` 36,
      `w:settings` 103, run content one free slot), and a call inserts each QName at most
      once per parent;
    - an insertion into its own slot keeps an existing valid greedy assignment valid, and
      `max_each` is counted per QName;
    - inserts into a node created by the same call cannot fail except by `NotAllowed`, which
      the types rule out.

  After a successful dry run the mutation itself cannot fail. The set-text, set-attribute and
  remove primitives have no failure modes left once `InvalidXmlChar` and the depth are checked.

### 8. Allowlist and overrides

#### 8.1 Allowlist (`codegen/allowlist/wml.toml`)

The full proposed file is `spikes/docx-phase1/wml-phase1.toml`. `allowlist_dryrun.sh` ran
it through the real generator: all 66 wml entries lower with no new diagnostic, and
`ooxml-schema` with feature `wml` compiles. Additions to ADR-0004 §4:

| Section | Entries | For |
|---|---|---|
| `[write]` (moved from `[read]`) | `w:CT_RPr/w:rPr` (51 slots), `w:CT_PPr/w:pPr` (36), `w:CT_Settings/w:settings` (103), `w:CT_OnOff/w:updateFields` | §4, §6.3 |
| `[write]` | `w:CT_OnOff/w:b`, `w:CT_OnOff/w:bCs`, `w:CT_OnOff/w:i`, `w:CT_OnOff/w:iCs`, `w:CT_OnOff/w:strike`, `w:CT_Underline/w:u`, `w:CT_Color/w:color`, `w:CT_HpsMeasure/w:sz`, `w:CT_HpsMeasure/w:szCs`, `w:CT_Highlight/w:highlight`, `w:CT_VerticalAlignRun/w:vertAlign`, `w:CT_Fonts/w:rFonts` | §4 run subset |
| `[write]` | `w:CT_Jc/w:jc`, `w:CT_Spacing/w:spacing`, `w:CT_Ind/w:ind`, `w:CT_OnOff/w:keepNext`, `w:CT_OnOff/w:keepLines` | §4 paragraph subset |
| `[write]` | `w:CT_String/w:pStyle`, `w:CT_String253/w:rStyle` (the `Name`s that are children of `CT_PPr` and `CT_RPr`; `w:CT_String253/w:pStyle` is the numbering-level one) | §5 |
| `[read]` | `w:CT_HdrFtr/w:hdr`, `w:CT_HdrFtr/w:ftr`, `w:CT_Footnotes/w:footnotes`, `w:CT_FtnEdn/w:footnote`, `w:CT_Endnotes/w:endnotes`, `w:CT_FtnEdn/w:endnote`, `w:CT_Comments/w:comments`, `w:CT_Comment/w:comment` (not `w:CT_FtnEdnSepRef/…`, the settings reference) | §2 roots and write paths |
| `[read]` | `w:CT_TcPr/w:tcPr`, `w:CT_VMerge/w:vMerge`, `w:CT_HMerge/w:hMerge`, `w:CT_DecimalNumber/w:gridSpan` | §3.3 merge state |
| `[read]` + `[detach]` | `w:CT_ParaRPr/w:rPr`; `w:CT_RPr/w:rPr` also in `[detach]` | §3.2 |
| `[read]` | `w:CT_Styles/w:styles`, `w:CT_Style/w:style`, `w:CT_StyleName/w:name` | §5 |

Markers (`w:proofErr`, bookmarks, comment ranges) are classified by QName on the raw walk,
as ADR-0004 does for wrappers. They need no generated view.

#### 8.2 One additive generator rule: `SameType` modulo required skips

PR #18 generates `SameType<S> for T` only between detachable entries of the **same** complex
type. `CT_ParaRPr` and `CT_RPr` differ, so the generator emits nothing for them. The dry run
on PR #18's tree confirmed it: `Detachable` for both, no `SameType`. Proposed (an
ADR-0002 post-acceptance note in the implementation PR, additive):

```toml
[[detach.same_type]]
from = "w:CT_ParaRPr/w:rPr"
to   = "w:CT_RPr/w:rPr"
skip = ["w:ins", "w:del", "w:moveFrom", "w:moveTo", "w14:conflictIns", "w14:conflictDel",
        "w:oMath", "w:rPrChange"]
```

- **Generation (wording corrected in revision 2, review m5).** The generator removes the
  `skip` QNames from **both** lowerings and compares what remains: same slots in the same
  order, same attributes, same completeness. If they match, it emits `SameType<From> for To`
  with `const REQUIRED_SKIP: &[QName]` = `skip`; otherwise it reports `SameTypeMismatch`, as
  today.
  - `w:rPrChange` occurs on **both** sides, with the same QName but different types
    (`CT_ParaRPrChange` vs `CT_RPrChange`). It is in `skip`, so it is removed from both for the
    comparison, and since `detach_as` must skip it, it never appears in the copy.
  - The other seven skipped QNames exist only in `CT_ParaRPr`. `w:oMath` is in the SDK's
    `CT_ParaRPr` only.
  - Counts: `CT_ParaRPr` has 58 child Names in 57 slots, because `w14:conflictIns` and
    `w14:conflictDel` share one choice slot (`max_total` 1). `CT_RPr` has 51 Names in 51
    slots. With the skips removed, both sides have the same 50 slots in the same order.
- **Runtime.** `detach_as` refuses with `NotDetachable { reason: RequiredSkipMissing }` when
  its `skip` argument does not cover `REQUIRED_SKIP`. The reflexive impl has an empty list,
  so PR #18's callers are unaffected.
- **No IDs are copied.** The skipped elements are the revision ones. Two kept elements carry a
  `w:id` that groups runs: `w:eastAsianLayout` and `w:fitText` (revision 2, review m4). The
  domain refuses both before copying (§3.2), so no id-bearing element is copied.

#### 8.3 Overrides

**None new.** ADR-0004's `set_default` of `w:val` = `true` on `w:CT_OnOff/` covers every new
on/off entry (`b`, `bCs`, `i`, `iCs`, `strike`, `keepNext`, `keepLines`; `updateFields`
already). Checked against the data:

- `ST_Jc` lists `left`/`right` and `start`/`end`, in both the SDK enum and the ECMA
  5th-edition XSD;
- `ST_HpsMeasure` is a union with universal measures (`"12pt"`). Phase1 writes integers
  only, and a non-integer read value is an `AttrError` with the raw string, never coerced
  (ADR-0002 §1.2);
- `CT_Underline/@val` is optional with no default. An absent `val` (4 cases in the corpora)
  reads as "unspecified"; the no-op rule compares raw strings.

The roadmap's planned `add_child` for `w:smartTag` is dropped (§2.5).

### 9. Test oracles

All automated in `crates/ooxml-docx/tests/` except the manual check. ADR-0004's T1–T6 stay
green on the main story. Phase1 extends them:

- **T1/T1b** unchanged; they already cover every XML part.
- **T2 (locality), extended to all stories and operations:**
  - *Runs:* every settable run in every story of every writable fixture gets the Phase0
    double edit.
  - *Replace:* each accepted `replace` in the T7 sweep must change the story part only inside
    the original spans of `r₀ … rₖ`. Each run's start tag and `w:rPr` are byte-identical,
    each **unaffected content child** of those runs is byte-identical (T7 (iv)), and every
    other part is byte-identical.
  - *Formatting:* each setter, for each `Set`/`Clear` on each targetable run or paragraph of
    the fixtures, changes bytes only inside that run's or paragraph's span. A `Set` of the
    value already there is byte-identical.
  - *Empty paragraph:* `set_cell_text` on it is a pure insertion inside the paragraph's span.
  - *Settings:* `enable_update_fields_on_open` changes only `settings.xml`, at one insertion
    point or inside the existing element.
- **T3 (Layer 1):** after any Phase1 edit, exactly the touched parts differ. `styles.xml` is
  never among them; `settings.xml` only after the opt-in.
- **T4 (refusals):** every error of §10 has a fixture or a unit document. A refused call
  leaves `to_bytes()` identical to the input. That includes:
  - a `replace` with one refused match among many accepted ones (atomicity);
  - `find` → another write → `replace_at` (`StaleMatch`);
  - two `replace_at` on one paragraph without a new `find` (`StaleMatch`);
  - `format_paragraph` setting two properties on a `w:pPr` whose existing children are out
    of order, built from the template corpus's `[sectPr, pStyle, bidi, jc, rPr]`. It must
    fail before the first property is written (revision 2, review M2);
  - `enable_update_fields_on_open` on an out-of-order `settings.xml`.
- **T5 (docx-rs read-back), extended.** docx-rs 0.4.22 exposes the main story, the header and
  footer parts that the **body-level** `w:sectPr` references (`header`/`firstHeader`/
  `evenHeader`, same for footers), and comments. It has **no footnote/endnote reader**.
  Measured agreement with an ElementTree reading of unedited files (`readback_stories.py`):
  - headers: 57/57 POI, 114/117 SDK, 139/139 templates;
  - footers: 53/53, 125/134, 131/131;
  - comments: 1,153/1,153 SDK.

  The SDK misses are text-box and content-control text, which T5 excludes as Phase0 does.
  After each edit, the edited text appears exactly once, at its place, and all other texts are
  unchanged. For formatting, docx-rs's `runProperty` (bold, italic, size, color, underline,
  highlight, vertAlign, fonts, style) and paragraph `property` (alignment, indent, spacing,
  style) must show the written value.
- **T5b (notes and unexposed headers).** Footnotes, endnotes and headers/footers not reached
  from the final section are read back with **`roxmltree`** (dev-dependency), an XML parser
  independent of quick-xml and of ooxml-core's tree. A 40-line text extractor per paragraph
  (`w:t`, `w:tab`, `w:br`) applies the same before/after assertions. It is not a WML
  implementation, only an independent parse.
- **T6 (slots):** after each edit, every edited element's children are in generated slot
  order. Formatting inserts now exercise `w:rPr`'s 51 and `w:pPr`'s 36 slots and `w:settings`'
  103. For `w:pPr`, T6 also checks the ECMA XSD sequence (`CT_PPrBase` is an `xsd:sequence`),
  an input independent of the SDK data. Transitional `CT_RPr` is an unbounded `xsd:choice`
  (`EG_RPrBase`, `wml.xsd` l.1818), so any order is valid there and only the SDK order,
  which is the one Word writes, is checked.
- **T7 (find/replace differential oracle, new).** For every paragraph of every story of the
  writable fixtures, three needle sets are classified: the 6-character span around each run
  boundary (2,511 needles in the fixtures), every `\w{2,}` word, and every placeholder-shaped
  token. Each needle is classified by:
  - (a) the production code, `find` over the RawNode model;
  - (b) a **reference classifier in the test**, written over `roxmltree` from §1.1–§1.3's
    tables and rules only, about 200 lines. Revision 2 (review m2): it is written by a
    different author or agent than the production code. It is **not** a port of
    `replace_sim.py`, so the production code, the reference and the spike are three
    independent readings of the ADR.

  Both must give the same outcome for every needle. Each accepted match is then applied in a
  fresh document and must satisfy:
  - (i) `Paragraph::text()` after = the text before with that range replaced (string
    oracle);
  - (ii) per character, the canonical `rPr` after = before outside the match, and `r₀`'s
    inside the replacement;
  - (iii) T2's locality;
  - (iv) **byte identity outside the match** (revision 2, review M3). Every content child of
    `r₀ … rₖ` that holds no matched character serializes to the same bytes as before.
    This covers `w:noBreakHyphen`, `w:softHyphen`, `w:cr`, `w:lastRenderedPageBreak` at
    the edges, and untouched `w:t`.

    (i) and (ii) compare characters and cannot see a re-encoding such as `w:noBreakHyphen`
    → U+2011; (iv) can.

  `replace_sim.py`'s per-fixture counts are **reference values**, reported next to the test
  results; a difference is investigated but does not fail the test (revision 2, review m2).
  The Python prototype of (i)–(ii) held for every match it checked: all accepted boundary
  needles and placeholders and the accepted cross-run words. That is 18,554 matches in the five
  corpora after the revision 2 re-run (fixtures 1,483, POI 4,643, SDK 10,698, docx4j 14,
  templates 1,716).
- **T8 (cells and styles):** `set_cell_text` on every cell of the fixtures gives §3.3's
  outcome distribution (fixtures: 635 of 667 settable). Every `w:style` of every fixture can
  be applied by id to a paragraph or run of matching type and is read back by docx-rs. Every
  wrong-type and unknown id refuses.
- **T9 (fields):**
  - the per-operation table is enumerated in unit documents, one per (code or switch,
    operation) cell of §6.1's audit, including `SEQ … \s 1`;
  - the style setters are gated by the document-wide W2;
  - `IF` is refused under every operation (§6.2);
  - after the opt-in, the W2 fixtures (`sdk-TOC2`, `sdk-complex2010`) accept every operation,
    and `settings.xml` has the element once.
- **Corpus cross-check** (throwaway, as ADR-0004 §7): run `replace` on every boundary needle,
  `set_cell_text` on every cell and one formatting set per run over POI, SDK, docx4j and the
  templates. Save, reopen, compare with the spike, no panic.
- **Manual (explicit gap):** `manual_check` gains outputs for:
  - a cross-run replace with a collapsed `_GoBack` bookmark inside;
  - a header and a footer edit;
  - a footnote and a comment edit;
  - an empty cell with mark formatting;
  - each formatting setter;
  - a style by id in a localized document (`Rubrik`);
  - the `updateFields` prompt on `sdk-TOC2`;
  - a run with unpreserved edge whitespace edited by `set_run_text`, for open question 2;
  - a style-level `firstLineChars` plus `first_line` (§4, review m3);
  - `updateFields`: whether the element survives Word's update-and-save, on Word for Mac and
    Word Online too (§6.3, review m9);
  - a replace next to a `w:noBreakHyphen`, which must keep it as an element (§1.2).

**Fixtures.** The current 26 have no template placeholders. The template corpus is not
vendored: docxtpl is LGPL-2.1 and docxtemplater's licence is unasserted by GitHub
(§Open questions 6). T7's boundary needles exercise the same code paths. A small set of
hand-built unit documents covers the placeholder cases: split `{{…}}`, `_GoBack` inside,
bookmark wrapping a placeholder, `MixedFormatting`, a hyperlink edge and an empty cell with
mark formatting.

### 10. Errors (additions to ADR-0004 §6)

| Variant | Section | Test |
|---|---|---|
| `EmptyNeedle` | §1.1 | unit |
| `ReplaceRefused { refused: Vec<Refusal> }` (with `MatchCrossesBoundary`, `MarkerInsideMatch`, `MixedFormatting`, `UnpreservedWhitespace`, or an R1–R3 reason per match) | §1.4 | T7 sweep; units for each reason |
| `ParagraphNotSimple { boundary }` | §3.1 | unit, fixtures |
| `MarkHidden`, `UnsupportedMarkProperties { child }` | §3.2 | `poi`/fixture paragraphs with `w:vanish` on the mark; unit |
| `MergedContinuation`, `CellNotSimple { paragraphs, tables }` | §3.3 | `poi-table-alignment`, fixtures; unit |
| `UnsupportedContext { ancestor: SeparatorNote }` | §2.2 | `sdk-ftEdNote` |
| `UnknownStyle { id, latent_only }`, `StyleTypeMismatch { expected, found, linked }`, `AmbiguousStyle { id }` | §5 | fixtures; `poi-55733` (no styles part); unit (duplicate id) |
| `StaleMatch` | §7 | unit (T4) |
| `StyleChainTooDeep` | §4 | unit (`basedOn` cycle, 33 levels) |
| `Core(UnresolvedSibling)`, `Core(AlreadyPresent)` (from the dry run) | §0, §4, §6.3, §7 | template-pattern unit documents; out-of-order `settings.xml` |
| `NoSettingsPart`, `AmbiguousSettings`, `DuplicateSetting` | §6.3 | units |
| `DuplicateProperty { name }` | §4 | unit |
| `NoSuchStory`, `NoSuchTable`, `NoSuchCell` | §7 | units |

## Alternatives considered

1. **Normalize runs first** (merge equal-`rPr` neighbours, drop `proofErr`, bookmarks and
   rsids): docx4j `VariablePrepare`. Rejected for the reason ADR-0004 gave. It rewrites most
   paragraphs and deletes data (bookmarks, rsids) the user did not touch. ooxml-rs reaches the
   same placeholders (98.8%) by composing per-run edits.
2. **Move non-text markup after the value** (docxtemplater) or **delete it** (docxtpl).
   Rejected. Bookmarks, comment anchors, revisions and permission ranges inside a placeholder
   would silently move or disappear. Refusing (`MarkerInsideMatch`, boundaries) costs 4 of
   1,358 template boundary needles and 0 accepted placeholders after the collapsed-bookmark
   rule.
3. **Delete runs emptied by a replace.** Cleaner output, still local. Rejected for Phase1:
   it renumbers runs, and ADR-0004's index API promises stable indices. Revisit with a handle
   API.
4. **Replacement in the formatting of the run holding most of the match**, or refuse every
   mix. The majority rule is not predictable from the template. Refusing every mix of
   invisible properties would lose 21 points of cross-run needles in SDK (57.5% → 36.1%).
   Chosen: first run, with invisible differences ignored by default.
5. **Text inside smart tags via an `add_child` override.** Impossible as specified (no SDK
   entry, §2.5). An untyped QName-checked descent in core is deferred until Phase4 needs one.
6. **Copy the mark's run properties child by child** with typed setters, or a generic raw-copy
   primitive. The first needs every one of 51 property entries in the allowlist; the second
   gives up ADR-0002's type guarantee. Chosen: the `SameType`-modulo-skip rule (§8.2), one
   reviewed entry.
7. **Per-field `w:dirty="true"` instead of `updateFields`.** It would mark only the fields an
   edit could affect. But ECMA describes it as "the result is stale" without saying the
   consumer updates it *(u)*, it occurs 0 times in 631 files, and finding *which* fields an
   edit affects is the dependency analysis that §6.1 shows is rarely decidable (`TOC`,
   `PAGEREF`). Deferred; open question 5.
8. **Range-aware `REF`:** allow text edits outside every bookmark that a `REF` names. It
   would lift W2 for 1 docx4j file (`Bookmark_crossrefs`, `REF` only) and for none of the SDK
   files, which all also have `TOC`/`PAGEREF`/`STYLEREF`. Not worth the bookmark-range
   machinery (`colFirst`/`colLast`, ranges across paragraphs) in Phase1.
9. **Formatting part of a run** (split the run, copy `w:rPr`). Needs `w:rPr` detach (§8.2's
   entry makes it possible) and changes run indices. Phase2, with the structural edits.
10. **`Document::new()`, a protection opt-in, moving the last `sectPr` on append** (listed
    under Phase1 by ADR-0004's revisions). Not template filling. `new()` needs its own
    decisions (template source, page size, East Asian defaults) and goes to a separate ADR.
    The protection opt-in and the `sectPr` move go to the Phase2 structure ADR (ADR-0011;
    proposed default Q10). The corpora have 4 protected POI files and no A2 case.
11. **Refuse runs with special content outside the match** (review M3's option (b)): a
    touched run holding `w:cr`, `w:noBreakHyphen` or `w:softHyphen` outside the match would
    be `UnsupportedRunContent`. It is smaller to implement, but it refuses matches that the
    child-local edit handles exactly (POI 4, SDK 8 needles), and it does not cover
    `w:lastRenderedPageBreak` (POI 39, SDK 128) unless that is refused too. Rejected for
    §1.2's child-local edit, which depends on PR #26's `insert_at`. If PR #26 is not
    accepted, (b) is the fallback.
12. **`replace_at` without a staleness check, or no `replace_at`** (review M1). Without the
    check, an old `Match` silently replaces a different range. Dropping `replace_at` loses
    "replace this occurrence after showing it to a user". Chosen: the generation counter plus
    the matched text (§7).

## Consequences

- **Positive:**
  - Template filling works across runs, stories and cells: 98.8% of real template
    placeholders.
  - Byte-local diffs, no IDs, no parts.
  - Every refusal is typed and the call stays atomic.
- **Positive:** the oracles include a second, independent implementation of the matching
  rules (T7) and an independent XML parser for notes (T5b). That is stronger than Phase0,
  which had one independent reader.
- **Positive:** formatting writes are schema-ordered by construction, and they remove the
  attributes that would silently override them (theme colours and fonts, character-unit
  indents, line-unit spacing). python-docx's `font.name` sets only `ascii` and `hAnsi`
  (`oxml/text/font.py:39-46`), so an existing `asciiTheme` keeps overriding it (inferred from
  that source).
- **Negative:**
  - Producers' out-of-order `w:pPr`/`settings.xml` refuse formatting and the `updateFields`
    opt-in (templates: 9 paragraphs; `settings.xml` in 18 files; SDK 1 file). The refusal
    comes before any write.
  - A copied mark can carry mark-only formatting into the text: a 1 pt spacer cell's text
    is 1 pt (§3.2).
  - Content controls stay closed (Phase3): about 7.6% of SDK cross-run needles and 410 SDK
    cells.
  - Smart tags stay closed: 1.4% of SDK text runs.
  - Multi-paragraph cells are refused (POI 395, SDK 584).
- **Negative:** documents with a `TOC` remain refused unless the caller opts into
  `updateFields`. That is 18 of 444 SDK files, and probably more among real reports. The
  per-operation table rescues 1 file for text edits and 6 for formatting.
- **Negative:** the `IgnoreInvisible` default puts the first run's `w:hint` and proofing
  language on the whole replacement. That can change the font slot of ambiguous characters
  (quotes) in the replacement text itself *(u)*. The *needle's* ambiguous characters no longer
  pass the rule (m1).
- **Negative:** the following additions are needed, and `roxmltree` becomes a
  dev-dependency:
  - one generator rule and one core refusal reason (§8.2);
  - one core dry-run method (`check_insert`, §7);
  - a dependency on PR #26.

## Open questions for the project owner

Revision 2 records the round-1 reviewer's recommendations as **proposed defaults**. They are
what this revision specifies, pending the owner's confirmation.

1. **Default formatting rule.** *Proposed default:* `IgnoreInvisible`, with revision 2's
   ASCII/CJK restriction on `w:hint`. It decides 21 points of SDK cross-run needles and 7
   template placeholders. `Identical` as the default would lose them without making
   placeholders safer.
2. **Unpreserved edge whitespace.** *Proposed default:* keep `UnpreservedWhitespace` (3 of 517
   placeholders) until the `manual_check` output shows how Word reads `<w:t> a</w:t>`. That
   answers ADR-0004's open question 1 too. If Word shows the space, the refusal is dropped.
3. **Per-operation W2.** *Proposed default:* ship the text, append and format columns (small,
   enumerable, testable) and drop the style column. That is done in §6.1.
4. **`IF` rule.** *Proposed default:* defer it until real mail-merge templates exist (Q6).
   That is done in §6.2.
5. **`updateFields` in Word.** *Proposed default:* keep the opt-in design, explicit even when
   the element is already on. The owner's check covers:
   - the prompt text;
   - updates of `TOC` page numbers and of fields in headers and footers;
   - whether `fldChar/@w:dirty` alone triggers an update (Alternatives 7);
   - whether the element survives Word's update-and-save;
   - Word for Mac and Word Online (review m9).
6. **Template fixtures.** *Proposed default:* vendor none from docxtpl (LGPL-2.1) or
   docxtemplater (licence unasserted). The owner would provide 3–5 Word 365 templates:
   Japanese, with a header, a table, footnotes, a split `{{…}}` and a `_GoBack`. Until then,
   hand-built unit documents stand in. This also answers ADR-0004's open question 4.
7. **`roxmltree` as a dev-dependency** for T5b and T7's reference classifier. *Proposed
   default:* yes, pinned to an exact version like docx-rs. T7's independence comes from who
   writes the reference (m2), not from the crate.
8. **§8.2's generator rule** as an ADR-0002 post-acceptance note, implemented with Phase1.
   *Proposed default:* yes, with revision 2's wording (m5) and the `w:fitText` refusal (m4).
   **New in revision 2:** the one-method core addition `check_insert` (§7) goes in the same
   note or next to PR #26.
9. **`jc` written as `left`/`right`.** *Proposed default:* yes. It is compatible with the
   first edition's `ST_Jc` and the commoner form in the corpora.
10. **Out of this ADR.** *Proposed default:*
    - `Document::new()` gets its own ADR;
    - the enforced-protection opt-in and lifting A2 go to the Phase2 structure ADR
      (ADR-0011, PR #31, which already lifts A2).

## Sources

- Spike and every number: `spikes/docx-phase1/README.md` (scripts `replace_sim.py`,
  `stories_cells.py`, `formatting_styles.py`, `fields_perop.py`, `readback/` +
  `readback_stories.py`, `wml-phase1.toml` + `allowlist_dryrun.sh`, `summarize.py`, and since
  revision 2 `slot_order.py`), run 2026-11-16 on Python 3.15 and rustc 1.99.1; re-run for
  revision 2.
- Review: `docs/adr/0007-opus-review-round1.md`.
- Microsoft [MS-OI29500]: §17.16.5.56 `SEQ` (the `\s` switch); §17.15.1.90 `updateFields`
  (round trip). Links are in §6.1 and §6.3.
- Core: PR #26 `core/insert-at` (`LensMut::insert_at`, `check_insert_at`).
- Corpora:
  - Apache POI `47dcff769bb395034dddfd59c4609659b6dbc6a4` `test-data/document/`;
  - Open XML SDK `431ab05cf160248cc3885a4a766026d4f8243792` test assets;
  - docx4j samples (as ADR-0004's corpus check);
  - python-docx-template `1f143fbe86c19ecb28c3205d5f4b1547c7e2d7ad` `tests/templates/`;
  - docxtemplater `e1d61c64cb4a37420e53e57a673f5f36a2e34400` `examples/`.
- python-docx `e45454602b53e8e572b179ccf1c91093ec9f4ed7` (1.2.0), under `src/docx/`:
  - `oxml/text/paragraph.py:52-55, 96-102`, `text/paragraph.py:145-168`;
  - `oxml/text/run.py:40-60, 286-301`;
  - `table.py:273-284`, `oxml/table.py:449-458`;
  - `text/font.py:46-80, 196-200, 275-278, 394-403`, `oxml/text/font.py:39-46, 79-147`;
  - `oxml/shared.py:27-35`, `styles/styles.py:31-47, 118-136`, `enum/text.py:10-60`.
- Apache POI XWPF `47dcff76`, `poi-ooxml/src/main/java/org/apache/poi/xwpf/usermodel/`:
  - `XWPFParagraph.java:119-170, 519-530, 1438-1442, 1704-1802`;
  - `XWPFRun.java:216-224, 292-294, 366-370, 958-980, 1053-1102`;
  - `XWPFTableCell.java:439-457`;
  - `XWPFDocument.java:1476-1478`, `XWPFSettings.java:360-380`,
    `XWPFStyles.java:192-220`.
- docx-rs 0.4.22 (crate source), under `src/`:
  - `reader/read_docx.rs` (headers/footers via the body-level `sectPr`, comments, no
    footnote reader);
  - `documents/elements/run_property.rs:74-78, 124-134, 240-275`;
  - `paragraph_property.rs:73-76, 222-233`, `paragraph.rs:254-257`;
  - `types/alignment_type.rs:28-52`.
- docx4j `plutext/docx4j@836eab29ea8b47536ba9320e9bdf3da434de1d2f`:
  - `VariablePrepare.java:67-308`;
  - `WordprocessingMLPackage.java:272-343`, `filter.xslt:64-139`;
  - `JaxbXmlPart.java:439-473`, `XmlUtils.java:843-897`.
- docxtemplater `e1d61c64` (3.71.1):
  - `es6/file-type-config.js:18-100`, `es6/lexer.js:351-437`, `es6/parser.js:130-158`;
  - `es6/join-uncorrupt.js:10-140`, `es6/modules/render.js:120-182`;
  - `es6/modules/space-preserve.js:18-109`, `es6/doc-utils.js:150-216`,
    `es6/filetypes.js:9-52`.
- python-docx-template `1f143fbe` (0.20.2): `docxtpl/template.py:39-111, 159-189, 289-301,
  333-508`.
- Open XML SDK data `431ab05c`: `schemas_openxmlformats_org_wordprocessingml_2006_main.json`
  (entries of §8.1, `CT_ParaRPr` vs `CT_RPr` children, enums `JustificationValues`,
  `UnderlineValues`, `HighlightColorValues`, `StyleValues`).
- ECMA-376 Part 4 5th edition Transitional XSD: `wml.xsd` (`ST_Jc`, `ST_HpsMeasure`,
  `CT_Color`, `CT_Underline`, `CT_Style`). Part 1 section numbers marked *(u)* are from
  memory and are to be confirmed.
- ADR-0001, ADR-0002 (+ post-acceptance notes; PR #18 `core/detach-insert`), ADR-0004
  revision 4 (PR #13 `impl/docx-phase0`), `docs/roadmap/docx.md`.
