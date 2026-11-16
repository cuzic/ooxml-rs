# ADR-0007: Word Phase1: template filling

- Status: **Proposed** (revision 1)
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

Every text write in this ADR is a **composition of ADR-0004's `set_run_text`** (§3.2 there)
on runs that pass R1–R4, plus at most one new `w:r` in a paragraph that has none (§3.2
below). The other writes are attribute or element writes inside `w:rPr` or `w:pPr` (§4, §5),
or one element in `settings.xml` (§6). Hence:

- the text mapping (`\t` → `w:tab`, `\n` → `w:br`), the `xml:space` rules, the no-op rule
  and `T1 InvalidXmlChar` are ADR-0004's, unchanged;
- each run's diff stays inside its own span, its start tag and `w:rPr` byte-identical (T2);
- no element is moved or deleted: emptied runs stay in place without content, and range
  markers (bookmarks, comment ranges, permissions) never move;
- Phase1 creates no `w:id`, `w14:paraId`, `w14:textId`, rsid, relationship, content type or
  part. It writes `settings.xml` only through the opt-in of §6.3.

Each call checks the document gates, then every target, then mutates (ADR-0004 §3.3 order).
A call that touches several runs or paragraphs is **atomic**: either every target passes and
all are written, or nothing changes.

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

#### 1.2 Applying an accepted match

Let the match cover characters `s..e` of the paragraph text, and let runs `r₀ … rₖ` be the
runs that hold them (`r₀` holds `s`).

1. `r₀`'s new text is its text before `s`, then the replacement, then (if `k = 0`) its text
   after `e`.
2. For `0 < i ≤ k`, `rᵢ`'s new text is its text with the matched characters removed (a
   prefix; for `rₖ` the rest stays).
3. Every changed run is written with ADR-0004's `set_run_text` algorithm. A run left with
   empty text keeps its element and `w:rPr` and has no content children (`<w:r …><w:rPr>…
   </w:rPr></w:r>`; `CT_R` content is all optional). Run indices therefore stay valid for the
   lifetime of the `Document`, as in Phase0.
4. When several matches fall in one paragraph, the per-run texts are computed for all of them
   first and each run is written once.

The replacement takes **`r₀`'s formatting**: the run that holds the first matched character.
That is what both template engines do with the run holding the start delimiter (docxtemplater
`parser.js:145-157`, docxtpl `template.py:91-111`), and it is predictable from the template:
format the first character of the placeholder.

Markers at the match edges stay outside or inside the replacement as they were outside or
inside the placeholder: a bookmark that starts exactly at `s` stays before the replacement,
one that ends exactly at `e` stays after it. A bookmark that wraps exactly a placeholder
therefore wraps its value, which is how bookmark-based templates are read back.

**`xml:space`.** Step 3 adds `xml:space="preserve"` where ADR-0004 §3.2 step 6 requires it
(edge or double spaces), and never removes one. One case is refused instead. A changed run
whose `w:t` has leading or trailing whitespace **without** `preserve` would get the attribute
when it is rewritten. If that whitespace lies outside the match, a space Word does not show
*(u)* would become visible, a change the caller did not ask for. This is
`Err(UnpreservedWhitespace)`. It affects 3 of 517 template placeholders. ADR-0004's open
question on how Word reads such text decides whether the refusal can be lifted (§Open
questions 2).

`w:lastRenderedPageBreak` in a changed run is dropped, as in Phase0 (owner decision of
2026-11-16).

#### 1.3 Formatting rule

The `w:rPr` of `r₀ … rₖ` are compared canonically: as element trees, ignoring namespace
prefixes, attribute order and whitespace-only text. A missing `w:rPr` equals an empty one.
`ReplaceOptions::formatting` decides:

| `RunFormatting` | Accepts | Refuses |
|---|---|---|
| `Identical` | all canonically equal | any difference → `MixedFormatting` |
| **`IgnoreInvisible`** (default) | equal after dropping `w:noProof`, `w:lang/@w:val` and `w:rFonts/@w:hint` | any other difference, including `w:lang/@w:eastAsia` and `@w:bidi`, which can change glyph selection *(u)* |
| `FirstRun` | everything | — |

Why the default ignores exactly these:

- `w:noProof` and `w:lang/@w:val` choose the proofing language of Latin text. They do not
  change what is drawn.
- `w:rFonts/@w:hint` tells Word which font slot to use for characters that belong to two
  ranges (quotes, some punctuation). Word sets it per run in East Asian documents: 27.4% of
  SDK run pairs differ only there.
- The replacement is new text. It gets `r₀`'s value of each property anyway, so the only
  question is whether the *needle* looked uniform to the template author, and these three
  properties are invisible.

| Measured (`replace_sim.py`) | fixtures | POI | SDK | templates |
|---|---|---|---|---|
| adjacent text runs with identical `rPr` | 77.8% | 75.6% | 45.8% | 91.2% |
| … differing only invisibly | 0.2% | 3.7% | 27.9% | 2.4% |
| cross-run needles accepted: `Identical` / **default** / `FirstRun` | 58.7 / **58.9** / 76.2% | 63.3 / **66.5** / 85.3% | 36.1 / **58.2** / 82.7% | 83.1 / **85.4** / 93.0% |
| template placeholders accepted (default) | — | — | — | **511 / 517 (98.8%)** |

"Cross-run needles" are the ≤6-character spans centred on every run boundary: the hardest
case, since an arbitrary substring is far more likely to straddle a formatting change than a
placeholder. Of the 6 refused placeholders, 3 are in or across content controls, 1 crosses a
hyperlink, 1 is a field result, and 1 is a real formatting mix.

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
Editing the part changes all of them. `Story::sections()` reports, read-only, which sections
(by index in document order) and which types (`default`/`first`/`even`) use the part after
inheritance, so a caller can tell. In the corpora no part is referenced by two `w:sectPr`
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

`set_paragraph_text(p, text)` makes the paragraph's text equal `text`:

- **Paragraph with runs:** it is §1 applied to the whole text `0..len`, with
  `on_refused: Refuse`. The same groups, boundaries, anchors and formatting rule apply, so the
  first run's formatting carries the new text. A paragraph whose runs have no text puts it into
  its first settable run.
- **Paragraph without runs:** see §3.2.

Anything in the paragraph that is not a text run or a transparent or anchor element refuses:
`ParagraphNotSimple`, carrying the boundary.

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
| `UnsupportedMarkProperties` | `w:eastAsianLayout` (its `w:id` groups runs; a copy would join the group), or any child outside the `w` namespace | 0 |
| `Core(NotDetachable { .. })` | any PR #18 refusal (relationship attribute, MC, ignorable namespace, …) | 0 |

#### 3.3 `set_cell_text`

Tables are listed per story in document order, nested tables included. A cell is addressed as
`CellRef { story, table, row, cell }`, where `row` is the index among the table's `w:tr`
children and `cell` the index among that row's `w:tc` children. `Cell` also reports its grid
column (with `w:gridBefore`), `gridSpan` and merge state, read-only.

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
| `font(slot, name)` | `w:CT_Fonts/w:rFonts` | one of `w:ascii`, `w:hAnsi`, `w:eastAsia`, `w:cs` | the same slot's theme attribute (`w:asciiTheme`, …; it supersedes the name, *ECMA §17.3.2.26, u*; SDK 714 `asciiTheme`, 792 `eastAsiaTheme`). `w:hint` is kept |
| `run_style` | `w:CT_String253/w:rStyle` | §5 | — |

**Paragraph properties (`w:pPr`):**

| Setter | Element | Written form | Also removed when set |
|---|---|---|---|
| `alignment` | `w:CT_Jc/w:jc` | `left`, `center`, `right`, `both`, `distribute`. Reading also accepts `start`/`end` and the others in `ST_Jc` | — |
| `space_before`, `space_after` | `w:CT_Spacing/w:spacing` | `w:before`/`w:after` in twips | `w:beforeLines`/`w:beforeAutospacing` (resp. after), which supersede twips *(u)*: SDK 35 `*Lines`, 309 `*Autospacing` |
| `line_spacing` | `w:CT_Spacing/w:spacing` | `w:line` + `w:lineRule` (`auto` in 240ths, `exact`, `atLeast`) | — |
| `indent_left`, `indent_right` | `w:CT_Ind/w:ind` | `w:left`/`w:right` in twips | `w:start`/`w:end` and `w:leftChars`/`w:startChars` (resp. right), which supersede *(u)*: SDK 315 `leftChars` |
| `first_line`, `hanging` | `w:CT_Ind/w:ind` | `w:firstLine` or `w:hanging` | the other one, and `w:firstLineChars`/`w:hangingChars` |
| `keep_next`, `keep_lines` | `w:CT_OnOff/w:keepNext`, `…/w:keepLines` | as `b` | — |
| `paragraph_style` | `w:CT_String/w:pStyle` | §5 | — |

Rules:

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
- **Producer order and duplicates.** Transitional `CT_RPr` allows its children in any order
  and repeated. An existing `w:rPr` whose children do not fit the SDK slots makes the insert
  `Core(UnresolvedSibling)` (ADR-0002 §4.1). ADR-0002's corpus check found 0 such `w:rPr` or
  `w:pPr` in POI. A property element that occurs twice in one `w:rPr`/`w:pPr` refuses with
  `DuplicateProperty`: which copy wins is not specified *(fail closed)*.
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
  every match. The lookup by name is the portable one. Localized Word derives `w:styleId`
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
is unambiguous in the corpora; a duplicate refuses with `UnknownStyle` *(fail closed)*.
Applying a style does not clear direct formatting. POI and docx-rs write any id unchecked
(`XWPFParagraph.java:1438-1442`, `paragraph.rs:254-257`).

### 6. Fields: per-operation W2 and the `updateFields` opt-in

#### 6.1 Per-operation table

ADR-0004's W2 refuses every write when any story has a field outside `fields::SAFE`.
Revision 3 recorded the reviewer's recommendation: a table of which codes each operation
cannot make stale. Phase1 adopts it. `fields.rs` gets one column per operation class. A code
is allowed for an operation if it is in `SAFE` or in that column; every nested field must be
allowed too.

| Operation class | Additional allowed codes | Why their cached results cannot change |
|---|---|---|
| **text**: `set_run_text`, `replace`, `set_paragraph_text`, `set_cell_text` | `SEQ`, `TC`, `XE`, `NOTEREF`, `LISTNUM`, `AUTONUM`, `AUTONUMLGL`, `AUTONUMOUT` | they depend on the number and order of fields or paragraphs, on their own instruction, or on note numbering. Text edits change none of these: `\n` becomes `w:br`, never a paragraph |
| **append**: `append_paragraph` (Phase0) | the same | as ADR-0004 rev 3. Appending adds a paragraph after every field, so earlier `AUTONUM`/`LISTNUM` counts are unchanged |
| **format**: §4 setters | the same, plus `NUMWORDS`, `NUMCHARS`, `INFO NUMWORDS`/`NUMCHARS`, `DOCPROPERTY Words`/`Characters`/`CharactersWithSpaces`, and `=` | formatting changes no characters and no numbers. `REF` is not added: its result copies the source's formatting *(u)*. `Lines`/`Pages` statistics are not added: they depend on layout |
| **style**: §5 setters | `SEQ`, `TC`, `XE`, `NOTEREF` | a style can change outline level and numbering (`TOC`, `STYLEREF`, `AUTONUM*`, `LISTNUM`, `REF \n`) and hidden text (counts) |

`TOC`, `REF`, `PAGEREF`, `STYLEREF`, `IF` (except under §6.2), `INDEX`, `TOA`, `CITATION`,
`BIBLIOGRAPHY` and unknown codes stay refused for every operation. Tests enumerate the whole
table, with one unit document per (code, operation) cell.

**Measured yield is small** (`fields_perop.py`). Of the 33 StaleFieldRisk files (POI 1,
SDK 30, docx4j 2; templates 0), the table lifts W2 for text edits in **1** (SDK `table.docx`,
`SEQ` only), for formatting in 6 and for style edits in 1. The 19 files with `TOC` +
`PAGEREF` (18 SDK, 1 docx4j) stay refused under every operation. The table is kept because it is small and
fully enumerable. Whether it is worth shipping before `updateFields` is §Open question 3.

#### 6.2 `IF`

`IF` is allowed for text, append and format (not style) when:

- (a) every nested field is allowed for that operation, and
- (b) its comparison is `operand op operand` where neither operand is a **bare word**.

A bare word is a bookmark reference whose text the edit could change. For (b), a tokenizer
over the instruction recognizes:

- quoted strings (straight and typographic quotes, as ADR-0004 rev 4);
- numbers;
- the operators `= <> < > <= >=`;
- switches (`\* …`, `\@ …`, `\# …`);
- a placeholder for each nested field;
- bare words.

An operator must be the second token. The mail-merge pattern `{ IF { MERGEFIELD Gender } =
"F" "Ms." "Mr." }` passes. `{ IF Total > 100 … }` (bookmark `Total`) and anything the
tokenizer cannot read are refused. The 5 `IF` fields in the corpora (all SDK) are all
refused: four `IF DATE \@ "yyyy" = "1998" …` (a bare word) and one with an unbalanced quote.
So the rule is tested by unit documents only. Mail-merge main documents
(`settings/w:mailMerge`: 1 POI, 1 SDK) re-evaluate `IF`/`MERGEFIELD` on every merge *(u)*.

#### 6.3 `enable_update_fields_on_open`

`Document::enable_update_fields_on_open()` is the explicit opt-in that lifts W2 for every
operation of this `Document` from then on. It writes `<w:updateFields w:val="true"/>` into
`settings.xml` (POI's `enforceUpdateFields` writes `w:val="on"`, `XWPFSettings.java:372-376`).
Word then offers to update all fields when it opens the file *(u: owner to confirm the prompt,
and whether headers, footers and `PAGEREF` in a `TOC` are updated)*. Readers other than Word
keep showing the stale results; the API documentation says so (ADR-0004 Alternatives 4).

- **Checks:** W1 and W3 (it is a write), not W2. Exactly one settings part must exist:
  - none → `NoSettingsPart` (creating a part is Phase2);
  - two settings relationships → `AmbiguousSettings` (fail closed; ADR-0004 rev 4 reads
    both);
  - two `w:updateFields` elements → `DuplicateSetting`.
- **Write:** `get_or_insert::<UpdateFieldsOnOpen>()` under `w:settings`, placed by the
  generated 103-slot order. An `mc:AlternateContent` child of `w:settings` makes this
  `Core(UnresolvedSibling)`.
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
    pub fn find(&self, needle: &str, scope: Scope) -> Result<Vec<Match>>;  // Err only for EmptyNeedle/InvalidXmlChar
    pub fn replace(&mut self, needle: &str, replacement: &str, opts: &ReplaceOptions) -> Result<ReplaceReport>;
    pub fn replace_at(&mut self, m: &Match, replacement: &str, opts: &ReplaceOptions) -> Result<()>;
    // §0, §3
    pub fn set_run_text_at(&mut self, run: RunRef, text: &str) -> Result<()>;
    pub fn set_paragraph_text(&mut self, p: ParaRef, text: &str) -> Result<()>;
    pub fn set_cell_text(&mut self, cell: CellRef, text: &str) -> Result<()>;
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
                   pub status: Result<(), Refusal> }
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
- **Atomicity** (§0) needs no transaction support: every check runs over the read model
  first, and the writes cannot fail afterwards except on core invariants (`TooDeep`,
  `NamespaceNotInScope`). Those are pre-checked with `Detached::check_insert_under` and the
  depth of the target path.

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

- **Generation.** The generator emits `SameType<From> for To` with
  `const REQUIRED_SKIP: &[QName]` only if the two lowerings are equal once the `skip` QNames
  are removed from `from`: same slots in the same order, same attributes, same
  completeness. `w:rPrChange` is listed because its type differs between the two. Otherwise
  it reports `SameTypeMismatch`, as today.
- **Runtime.** `detach_as` refuses with `NotDetachable { reason: RequiredSkipMissing }` when
  its `skip` argument does not cover `REQUIRED_SKIP`. The reflexive impl has an empty list,
  so PR #18's callers are unaffected.
- **No IDs are copied.** No id-bearing element survives the skips. `w:eastAsianLayout`
  (`w:id`) is refused by the domain before copying (§3.2).

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
    the original spans of `r₀ … rₖ`, each run's start tag and `w:rPr` byte-identical, every
    other part byte-identical.
  - *Formatting:* each setter, for each `Set`/`Clear` on each targetable run or paragraph of
    the fixtures, changes bytes only inside that run's or paragraph's span. A `Set` of the
    value already there is byte-identical.
  - *Empty paragraph:* `set_cell_text` on it is a pure insertion inside the paragraph's span.
  - *Settings:* `enable_update_fields_on_open` changes only `settings.xml`, at one insertion
    point or inside the existing element.
- **T3 (Layer 1):** after any Phase1 edit, exactly the touched parts differ. `styles.xml` is
  never among them; `settings.xml` only after the opt-in.
- **T4 (refusals):** every error of §10 has a fixture or a unit document. A refused call
  leaves `to_bytes()` identical to the input. That includes a `replace` with one refused
  match among many accepted ones (atomicity).
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
  - (b) a **reference classifier in the test**, written over `roxmltree` straight from §1.1's
    tables (about 200 lines, a port of `replace_sim.py`'s `Stream`).

  Both must give the same outcome for every needle. Each accepted match is then applied in a
  fresh document and must satisfy:
  - (i) `Paragraph::text()` after = the text before with that range replaced (string
    oracle);
  - (ii) per character, the canonical `rPr` after = before outside the match, and `r₀`'s
    inside the replacement;
  - (iii) T2's locality.

  Per-fixture outcome counts must equal `replace_sim.py`'s, as ADR-0004's `fixtures.rs` does
  for Phase0's spike. The Python prototype of (i)–(ii) held for every match it checked: all
  accepted boundary needles and placeholders and the accepted cross-run words, 18,652 matches
  in the five corpora (spike E: fixtures 1,483, POI 4,643, SDK 10,793, docx4j 14, templates
  1,719).
- **T8 (cells and styles):** `set_cell_text` on every cell of the fixtures gives §3.3's
  outcome distribution (fixtures: 635 of 667 settable). Every `w:style` of every fixture can
  be applied by id to a paragraph or run of matching type and is read back by docx-rs. Every
  wrong-type and unknown id refuses.
- **T9 (fields):** the per-operation table is enumerated in unit documents, one per (code,
  operation). `IF` is tested with the mail-merge form, a bookmark operand, a nested refused
  field and an unbalanced quote. After the opt-in, W2 fixtures (`sdk-TOC2`,
  `sdk-complex2010`) accept every operation, and `settings.xml` has the element once.
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
  - a run with unpreserved edge whitespace edited by `set_run_text`, for open question 2.

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
| `UnknownStyle { id, latent_only }`, `StyleTypeMismatch { expected, found, linked }` | §5 | fixtures; `poi-55733` (no styles part) |
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
   invisible properties would lose 22 points of cross-run needles in SDK (58.2% → 36.1%).
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
    The protection opt-in and the `sectPr` move stay open; the corpora have 4 protected POI
    files and no A2 case.

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
  - Content controls stay closed (Phase3): about 7.6% of SDK cross-run needles and 410 SDK
    cells.
  - Smart tags stay closed: 1.4% of SDK text runs.
  - Multi-paragraph cells are refused (POI 395, SDK 584).
- **Negative:** documents with a `TOC` remain refused unless the caller opts into
  `updateFields`. That is 18 of 444 SDK files, and probably more among real reports. The
  per-operation table rescues 1 to 6 files per operation class.
- **Negative:** the `IgnoreInvisible` default puts the first run's `w:hint` and proofing
  language on the whole replacement. That can change the font slot of ambiguous characters
  (quotes) in the replacement text *(u)*.
- **Negative:** one generator rule and one core refusal reason are added (§8.2), and
  `roxmltree` becomes a dev-dependency.

## Open questions for the project owner

1. **Default formatting rule.** Is `IgnoreInvisible` (ignore `noProof`, `w:lang/@w:val`,
   `w:rFonts/@w:hint`) acceptable as the default? Or should the default be `Identical`, with
   the invisible rule opt-in? It decides 22 points of SDK cross-run needles and 7 template
   placeholders.
2. **Unpreserved edge whitespace.** Does Word show `<w:t> a</w:t>` (no `xml:space`) with the
   leading space? The `manual_check` output answers this and ADR-0004's open question 1
   together. If Word shows it, `UnpreservedWhitespace` is dropped.
3. **Per-operation W2.** Ship §6.1's table in Phase1 (small, enumerable, rescues 1–6 files
   per class in the corpora), or ship only `updateFields` and drop the table?
4. **`IF` rule.** Keep it with unit documents only (no corpus case passes), or defer it
   until a real mail-merge template is available?
5. **`updateFields` in Word.** Please confirm in current Word:
   - the prompt text;
   - that `TOC` page numbers and fields in headers/footers are updated;
   - whether `fldChar/@w:dirty` alone triggers an update.

   That decides Alternatives 7.
6. **Template fixtures.** May a few docxtemplater `examples/*.docx` be vendored? Their licence
   is unasserted on GitHub and needs checking. docxtpl's are LGPL-2.1, so probably not. Or
   could the owner provide 3–5 Word 365 templates (Japanese included) with split
   placeholders, a letterhead, a filled table and footnotes? This also answers ADR-0004's open
   question 4.
7. **`roxmltree` as a dev-dependency** for T5b and T7's reference classifier.
8. **§8.2's generator rule** (`SameType` modulo required skips) as an ADR-0002
   post-acceptance note, implemented with Phase1.
9. **`jc` written as `left`/`right`** rather than `start`/`end`.
10. **Out of this ADR:** `Document::new()` (its own ADR?), the enforced-protection opt-in,
    and lifting A2. Where should they go?

## Sources

- Spike and every number: `spikes/docx-phase1/README.md` (scripts `replace_sim.py`,
  `stories_cells.py`, `formatting_styles.py`, `fields_perop.py`, `readback/` +
  `readback_stories.py`, `wml-phase1.toml` + `allowlist_dryrun.sh`, `summarize.py`), run
  2026-11-16 on Python 3.15 and rustc 1.99.1.
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
