# ADR-0008: PowerPoint Phase 1 — read the whole deck

> **要約（日本語）**
> Phase 1 は「デッキ全体を読む」段階。(1) スライド → レイアウト → マスター →
> テーマの継承を解決し、テキスト書式・本文枠の設定・位置とサイズを
> **どのパートのどの要素から来た値か**（provenance）付きで返す。
> プレースホルダーの対応付けは idx を優先し、idx が重複・欠落する
> 実ファイル（LibreOffice 出力など）は type と系列（title / body / dt / ftr /
> sldNum）で決める。(2) 既存のノートの本文を Phase 0/0.1 と同じ規則で編集する。
> (3) ハイパーリンク・動作設定、セクション、コメント（旧形式と p188）、
> 画面切り替え（`mc:AlternateContent` の分岐選択つき）とアニメーションの
> 要約を読む。(4) ラン境界をまたぐ検索・置換を、置換文字列が一致開始位置の
> ランの書式を受け継ぐ規則で定義する。根拠は 633 ファイル（読めた
> プレゼンテーション 619）を `spikes/pptx-phase1/` で数えた結果と、
> python-pptx・Apache POI・LibreOffice のソースと ECMA-376 の本文。
> テキスト以外の要素は書き込まない。

- Status: **Accepted** (revision 3)
- Date: 2026-11-16
- Review: [round 1](0008-opus-review-round1.md) (not converged: 5 Major, 12
  Minor; all addressed in revision 2), [round 2](0008-opus-review-round2.md)
  (converged: Minor only, "Accepted after the Minor fixes"; fixed in
  revision 3). Owner decision 2026-11-16 on Q1–Q6: see Open questions
- Depends on: [ADR-0001](0001-lossless-round-trip-architecture.md),
  [ADR-0002](0002-schema-driven-code-generation.md) (with its post-acceptance
  notes "wildcard parents / XML characters", "parser depth limit" and
  "detach/insert", core PR #18),
  [ADR-0005](0005-pptx-phase0.md) (revision 3 Accepted; revision 4 §9,
  "Phase 0.1 — subtree copy of format sources", PR #19, Proposed)
- Background and evidence: [`docs/roadmap/pptx.md`](../roadmap/pptx.md) §3.1,
  §3.3, §3.4 and Phase 1; [`spikes/pptx-phase1/`](../../spikes/pptx-phase1/README.md)
  (scripts and committed outputs in `results/`)

## Revision history

- **Revision 1 (2026-11-16):** first proposal.
- **Revision 2 (2026-11-16):** addresses every finding of the round-1
  review. Disposition:

  | Finding | Change | Where |
  |---|---|---|
  | M1 generated views return XSD defaults, so "first element that sets" stopped on defaults | "set" = the attribute (or a member of the element group) is present in the raw element; resolvers read presence, never `*_or` accessors; XSD defaults are never returned; affected attributes listed | §1.3 "What *set* means", §7 S10 |
  | M2 placeholders' own `p:style/a:fontRef` ignored | step 4 applies to placeholders too, defined precisely (own `p:style` only; typeface per script, colour child) | §1.3, §7 S11, M1 |
  | M3 East Asian / complex-script theme fonts "resolved" to `""` | an empty or missing theme typeface is `Unresolved(ScriptDependent)` with the theme's script fonts; measured for `ea` | §1.4, §5 `Font` |
  | M4 specified vs displayed values (autofit, hyperlink colour) | values are *as specified*, stated; autofit returned as its own lookup, inherited scales `Unresolved`; link colour rule exposed | §1.6, §5 |
  | M5 `` in a replacement moved the kept suffix out of a link | suffix goes to the last new run only when the start run has no link; otherwise `LinkSplitByBreak`; copy source is the start run; spike extended with `` replacements | §4 rule 5, evidence |
  | m1 Q1 framing: ECMA separates text boxes from shapes | measured per `txBox`; manual check deck has text box, shape, placeholder with `p:style` | §1.3, §7 M1 |
  | m2 `masterClrMapping` + layout override contradicts ECMA text | rule kept with POI and LibreOffice's test file as evidence, ECMA disagreement stated, manual check | §1.4, §7 M6 |
  | m3 paragraph `a:defRPr` unverified in PowerPoint | stated; manual check | §1.3, §7 M5 |
  | m4 elements used but not allowlisted | 6 entries added (`a:sym`, `a:buClrTx`, `a:buSzTx`, `a:buSzPts`, `a:buFontTx`, `a:custGeom`); other fills read by name only | §6 |
  | m5 `prstGeom`/`custGeom`; placeholders in groups | geometry choice group; group-member placeholders do not inherit geometry | §1.5 |
  | m6 which type selects `txStyles`; `hdr`/`sldImg`; ties | slide placeholder's own type selects `txStyles`, layout type selects the master placeholder; families fixed; ties visible | §1.2, §1.3 |
  | m7 notes chain | spelled out | §1.3 |
  | m8 no place for "unresolved" | `Lookup<T> = Set \| Unset \| Unresolved(Reason)` | §5 |
  | m9 MCE `Requires` | prefixes resolved to URIs in scope; `p159` understood | §3.4 |
  | m10 oracle independence; POI does resolve text | claim fixed; optional POI differential; mechanical vs semantic expectations separated | §7 |
  | m11 invalid `lvl` | level-dependent steps `Unresolved(InvalidLevel)` | §1.3 |
  | m12 `p:cTn` percentages unused | 11 overrides, `p:cTn` left as `DefaultMismatch` | §6 |
  | Q1–Q6 recommendations | recorded as proposed defaults | Open questions |

- **Revision 3 (2026-11-16): Accepted.** The round-2 review converged
  (Major 0). Its ten Minor findings are fixed:

  | Finding | Change | Where |
  |---|---|---|
  | r2-m1 attributes of a group's member element were looked up per attribute | read only from the element that won the group; absent → that element's XSD default, `Set` with `defaulted: true` | §1.3, §1.6, §5, §7 S12 |
  | r2-m2 three POI statements | master placeholder chosen by the slide type in POI; POI never uses `defaultTextStyle` for placeholders (step 8 rests on ECMA only, with counts); POI pinned to `47dcff769bb3`; O2b table extended | §1.2, §1.3, §7 O2b, Sources |
  | r2-m3 spike's `LinkSplitByBreak` saw `a:hlinkClick` only | spike fixed and rerun; O5 has an `a:hlinkMouseOver`-only case (implementation: PR #48) | §4, §7 O5 |
  | r2-m4 `p:cNvSpPr` and the 47/77 split | `p:cNvSpPr@txBox` read raw; table header fixed | §6 |
  | r2-m5 stale numbers | colour counts and autofit chains recounted | §1.4, §1.6 |
  | r2-m6 notes and layout/master contexts | colour map, theme and geometry for notes; chains for layout/master shapes | §1.7 |
  | r2-m7 `themeOverride` | not applied; theme lookups on such a slide `Unresolved(ThemeOverride)` | §1.4 |
  | r2-m8 explicit idx missing from the layout | added to M2 with counts | §7 M2 |
  | r2-m9 invalid `lvl` | resolution stops at the first level-dependent step | §1.3 |
  | r2-m10 hyperlink colour scope | `a:hlinkClick` only; non-URL actions in M7 | §1.6, §7 M7 |
  | Owner decision 2026-11-16 | Q1–Q5 defaults accepted (Q1 with r2-m2's corrections); Q6 synthetic until a real fixture arrives | Open questions |

### Post-acceptance note (2026-11-16): implementation of §1 and §3 (PR #56)

Recorded after review [#56 round 1](../reviews/pr56-round1.md). The
decisions above are unchanged; this note lists where the implementation
departs from the §5 sketch, and what PR #56 leaves to the stacked notes PR.

- **Resolver methods live on `DeckSlide`** (`run_props(shape, body, para,
  item)`, `para_props`, `body_props`, `geometry`, `outline`,
  `placeholder_match`, `color_map`, `slide_rect`), not on
  `ShapeRef`/`RunRef`. Resolution needs the slide's chain (layout, master,
  theme, colour map); putting it on the Phase 0 refs would change Phase 0's
  `Slide` API. The review accepted this.
- **Enumerated values are `EnumValue<'a, E>`** of the generated simple types
  (ADR-0002 owner decision Q2): `algn`, `u`, `strike`, `cap`, `anchor`,
  `wrap`, `vert`, `buAutoNum@type`, transition `spd`, `presetClass`,
  `nodeType`, `bldP@build`, p188 comment `status`.
- **Group-winner attributes** (`rot`/`flipH`/`flipV`, `pitchFamily`/`charset`,
  `startAt`) are `WinnerAttr<T> { value: Result<T, AttrError>, defaulted }`,
  one flag per attribute; `Resolved::defaulted` is used only for an autofit
  scale defaulted on the shape itself. Invalid values keep their raw text.
- **`xsd_default`** is `RunProps::xsd_default(attr)`, `ParaProps::…`,
  `BodyProps::…` (by attribute name) instead of a `PropertyId` enum.
- **Q4 helper:** `RunProps::script_font_candidate(&slot)` with
  `script_for_lang(lang)`; it returns a candidate `(script, typeface)` and
  never turns `Unresolved(ScriptDependent)` into `Set`.
- **`ParagraphRef::end_props`** is `run_props(…, RunItem::End)`.
- **`Effect::click_index`** counts `clickEffect` nodes of the main sequence
  (each starts the next click), not `clickPar` nodes. PowerPoint does not
  always write `clickPar` on its click groups: most files have none, 30
  occur in 8 slides of the corpus. Counting `clickEffect` matches a
  structural reference (one main-sequence `p:par` waiting for a click per
  click) on all 299 effects of the corpus (review #56 round 2).
- **`Outline::Preset`** is `EnumValue<ShapeTypeValues>` and
  **`Color::theme_slot`** is `EnumValue<ColorSchemeIndexValues>` (#56 r2-m1).
- **Values from a master placeholder report the slide placeholder's own
  `MatchKind`** (§1.2 "every resolved value … reports the `MatchKind`");
  `MasterFamily` is the kind of a layout placeholder's master match (§1.7).
- **Synthetic decks (§7 O2)** are built in memory from `poi/sample.pptx`
  with documented XML edits in `tests/resolve.rs` / `tests/deckread.rs`,
  not by a committed `build_phase1.py` into `reference/fixtures/pptx/synthetic/`;
  each test states the rule it isolates (expectations are by construction).
- **Deferred to the stacked notes PR** (ADR-0008 §1.7, §2;
  branch `impl/pptx-phase1-notes`): `Deck::layouts()`/`masters()` and the
  notes/layout/master resolution contexts, links on notes, layouts and
  masters, and synthetic deck S8 (notes). The POI differential (O2b) is
  specified by `tests/oracle/poi_known_differences.md` but not wired (no
  JVM in CI).

### Post-acceptance note (2026-11-16): notes and contexts (§1.7, §2; PR stacked on #56)

- **Contexts are `Sheet<'a>`** (`SheetKind::{Slide, Notes, Layout, Master}`)
  with the resolver methods; `DeckSlide`'s resolvers delegate to
  `DeckSlide::sheet()`. `DeckSlide::notes_sheet()`, `Deck::layouts()` and
  `Deck::masters()` (masters in `p:sldMasterIdLst` order, then their
  layouts in `p:sldLayoutIdLst` order) give the other contexts.
- **Notes placeholders:** the notes-master placeholder is reported as
  `Role::NotesMaster` with the notes match kind (`Type`/`Family`, §1.2
  step 3; `Orphan` when the notes master has none). `p:notesStyle` is
  `TextStyleKind::Notes`. The colour map sources are `NotesOverride` and
  `NotesMaster`.
- **Non-placeholder shapes on notes slides** resolve steps 1–4 only; §1.3
  "Notes" excludes `p:defaultTextStyle`, and a notes master has no
  `otherStyle`. The corpus has no such text: 0 of 395 notes runs.
- **Master placeholders** report `MatchKind::OnMaster`: they inherit from no
  placeholder.
- **Notes text and editing:** `Presentation::notes(i)` → `NotesRef`,
  `Presentation::notes_mut(i)` → `NotesMut::body_mut()` (the Phase 0
  `TextBodyMut`), `DeckSlide::notes()`. Without a notes slide the error is
  `NoNotesSlide { slide_id }`.
  - The notes body is the first top-level `p:sp` with `p:ph@type="body"`. A
    body inside `mc:AlternateContent` is used only when there is no other
    body, and it is refused for editing (`InsideAlternateContent`).
    python-pptx never sees such a body.
- **Find/replace:** `Scope::SlidesAndNotes` (each slide followed by its
  notes body; slides without notes or without a body are skipped) and
  `Scope::Notes(i)` (`NoNotesSlide`/`NoNotesBody`). `TextLocation::notes`
  marks a notes match. A failed apply also restores the notes parts.
- **Links on any part:** `PartLinks` from `DeckSlide::link_reader()`,
  `DeckSlide::notes_link_reader()` or `Deck::link_reader(&sheet)`.

Phase 1 builds on Phase 0.1. Wherever this ADR says "the Phase 0 rules", it
means ADR-0005 §3 and §6 **as amended by revision 4 §9** (format sources are
copied as whole subtrees without `a:hlinkClick`/`a:hlinkMouseOver`). If PR
#19 changes those rules in review, §2 and §4 follow them by reference; no
rule here depends on a detail of §9 beyond "a new run's `a:rPr` is a
link-free subtree copy of a format source".

## Context

Phase 0 reads and writes text in slide parts and returns only values written
on the element itself (ADR-0005 §4). On the 633-file corpus of the Phase 1
spike (the roadmap's 105 files, the remaining 417 files of LibreOffice
`sd/qa/unit/data/pptx` and the 100 ShapeCrawler test decks; 619 readable
presentations, 1,272 slides, `spikes/pptx-phase1/README.md`):

- **64 %** of slide placeholders take their position and size from the
  layout (360) or the master (459); only 451 of 1,272 carry their own
  `a:xfrm` (`chain-633.txt`).
- Only **38 %** of runs in placeholders (1,439 of 3,836) set their own font
  size; the rest come from the layout placeholder (1,023), the master
  placeholder (111) or the master's `p:txStyles` (1,250).
- In non-placeholder text, 822 of 8,647 runs take `sz` from
  `p:defaultTextStyle` and 154 can only get it from the master's
  `p:otherStyle` — the question ADR-0005 deferred (Q6).
- Notes slides exist for 351 of 1,272 slides; links occur on 178 runs and
  shapes; 21 decks have sections; 83 slides wrap their transition in
  `mc:AlternateContent`; comments are rare (11 legacy, 1 modern).
- 6.2 % (roadmap corpus) to 14.5 % (633 files) of literal matches of
  1–3-word phrases cross a run boundary (`findreplace-*.txt`), so Phase 0's
  run-level `set_text` is not enough for search-and-replace.

Three reference implementations resolve parts of the chain, and they
disagree (sources at the end):

| Question | python-pptx 1.0.2 | Apache POI XSLF (`47dcff769bb3`) | LibreOffice oox (`2e885d0bda9b`) | ECMA-376 Part 1 |
|---|---|---|---|---|
| slide → layout placeholder | first layout placeholder with the same `idx` (absent = 0), top-level shapes only (`shapes/placeholder.py` `_BaseSlidePlaceholder._base_placeholder`, `LayoutPlaceholders.get`) | by `idx` **only if `@idx` is present**, then by exact `type`, idx-0 preferred (`XSLFSheet.getPlaceholder`, `initPlaceholders`) | by index for text styles (`PPTShape::findPlaceholderByIndex`); geometry by a type/idx priority (`findPlaceholder`) | `idx` "is used … to match a placeholder on one template/master to another" (§19.3.1.36); no algorithm |
| layout → master placeholder | mapped type (`ctrTitle→title`, others→`body`) (`LayoutPlaceholder._base_placeholder`) | mapped type (`XSLFShape.getPlaceholderType`) | by index | — |
| what is resolved | geometry only | text properties (`ParagraphPropertyFetcher`, `CharacterPropertyFetcher`), colours (`mapSchemeColor`) | everything (renders) | — |
| non-placeholder text | — | `defaultTextStyle`, then master `otherStyle` | `defaultTextStyle` only; `otherStyle` only for shapes **without** a text body (`pptshape.cxx` l. 510–520) | "`otherStyle` … for text within a slide shape but not within a text box. Text box styling is handled from within the `bodyStyle` element" (§19.3.1.35, note) |
| dt / ftr / sldNum style | — | `otherStyle` (`getDefaultMasterStyle`) | none from `txStyles` (`getSubTypeTextListStyle`) → `defaultTextStyle` | — |

So the chain cannot be copied from one library; each rule below states its
evidence, and every resolved value carries its **source** so a wrong rule
is visible to the caller and to the oracles.

## Decision

**Scope.** In: the inheritance chain with provenance (§1), editing existing
notes (§2), reading links, sections, comments, transitions and a timing
summary (§3), cross-run find/replace (§4), and the allowlist and
`DmlPercent` overrides they need (§6). Out, although the roadmap listed
them under Phase 1: **SmartArt text** (`dgm` data parts: own part graph and
drawing cache; 19 graphic frames in 7 of the roadmap's files) and **table
structure** beyond Phase 0's cell reads (grid, spans and merges are already
read by `TableRef`/`CellRef`; table styles belong with Phase 3). Both move
to a follow-up "Phase 1b" ADR so that this one stays reviewable. Nothing in
Phase 1 creates a part, relationship, content type or id.

### 1. Inheritance chain with provenance

#### 1.1 Parts

| Part | Reached by | Phase 1 use | After save |
|---|---|---|---|
| slide layout | the slide's single `…/relationships/slideLayout` relationship | Layer 2 parse, read-only | raw copy (never dirty) |
| slide master | the layout's `…/slideMaster` relationship | read-only | raw copy |
| theme | the master's `…/theme` relationship | read-only | raw copy |
| notes slide | the slide's `…/notesSlide` relationship | read; **edited** (§2) | `Parsed` (raw copy if not dirty) |
| notes master, its theme | the notes slide's `…/notesMaster` relationship | read-only | raw copy |
| comment parts, comment authors / authors | slide / presentation relationships (§3.3) | read-only | raw copy |

Parts are parsed once per `Presentation` on first use (one master serves
many slides: 655 masters, 1,271 complete chains). A missing or unparsable
link fails only the calls that need it: `Error::BrokenChain { from, missing:
Layout | Master | Theme | NotesMaster }` (corpus: 1 layout without a master
in a fuzz file; 45 notes slides without a notes-master relationship, all in
one ShapeCrawler deck, `sc027-031.pptx`). Phase 0 reads and writes on that
slide keep working.

#### 1.2 Placeholder matching

A placeholder's **effective idx** is `@idx` or 0 (XSD default), its
**effective type** `@type` or `obj`; whether `@idx`/`@type` is *present* is
read from the raw element (§1.3 "What *set* means"), never from the
generated view's defaulted accessor. Its **family** is `title` for
`title`/`ctrTitle`; `dt`, `ftr`, `sldNum`, `hdr`, `sldImg` for themselves;
`body` for everything else (`body`, `obj`, `subTitle`, `chart`, `tbl`,
`clipArt`, `dgm`, `media`, `pic`) — python-pptx's and POI's master mapping,
with `sldImg` (notes only, no text) kept apart.

**Slide (or notes slide) → layout (notes master)**, over every placeholder
of the layout's shape tree in document order, group members included:

1. Candidates with the same effective idx. Among them: exact type
   (`MatchKind::Idx`), else same family (`IdxFamily`). If several share the
   idx, the tie is broken the same way (`IdxTieType`, `IdxTieFamily`).
2. If there are idx candidates but none of the slide's family: when the
   slide's `@idx` is **present**, the first candidate is used and reported
   as `IdxTypeMismatch` (an explicit idx is PowerPoint's own link); when
   `@idx` is absent (idx only by default), go to step 3.
3. Exact type over all layout placeholders, idx 0 preferred (`Type`), then
   same family (`Family`).
4. Otherwise **orphan**: the slide placeholder resolves from the master
   directly by family (`Orphan`), as POI does; python-pptx returns `None`.

Wherever a step picks "the first" of several equal candidates (a tie that
type and family do not break, or two `obj` placeholders of a two-content
layout in step 3), the choice is a guess: `PlaceholderMatch` carries
`ambiguous: true` and the number of candidates, so callers see it.

Notes slides match the notes master by step 3 only (python-pptx
`NotesSlidePlaceholder._base_placeholder`; LibreOffice also requires equal
type on notes pages).

**Layout → master**: the first master placeholder of the same family as
the *layout* placeholder (python-pptx's `LayoutPlaceholder._base_placeholder`
uses the layout's type; POI's `XSLFShape.fetchShapeProperty`, l. 341–362,
uses `getPlaceholderType` of the **slide** placeholder — review r2-m2a). The
two differ only when the matched layout placeholder is of another family
(`IdxTypeMismatch`, 0 in the corpus); the difference is in O2b's table. An
orphan uses the slide placeholder's family.
None (masters without placeholders exist: 111 slide placeholders, mostly
LibreOffice-made decks) → the master-placeholder steps are skipped; the
master's `p:txStyles` still apply.

Measured (`chain-633.txt`, 1,272 slide placeholders; spike labels `idx`,
`idx(family)`, `idx+type`, `idx+family`, `type`, `family`, `orphan`):

| `MatchKind` | Count | Note |
|---|---:|---|
| `Idx` | 1,076 | |
| `IdxFamily` | 58 | 50 `body`→`obj`, 2 `ctrTitle`↔`title`… (roadmap's "49 idx-match/type-mismatch") |
| `IdxTieType` / `IdxTieFamily` | 25 / 44 | 69 placeholders whose layout has **several placeholders with idx 0** (title and body without `@idx`, e.g. LibreOffice 24.2 output `pres-with-notes.pptx`); "first by idx" would give the subtitle the title's position |
| `IdxTypeMismatch` | 0 | explicit idx pointing at another family never occurs |
| absent `@idx`, idx-0 candidates of another family → `Type`/`Family`/`Orphan` | 6 | e.g. `subTitle` without `@idx` vs a layout whose only idx-0 placeholder is the title |
| `Type` / `Family` | 6 / 2 | incl. `sldNum` with `idx="4294967295"` (5): python-pptx finds nothing |
| `Orphan` | 61 | |
| different layout placeholder than POI / python-pptx would pick | 56 / 67 | all in the tie and absent-idx rows above; 0 / 2 in the roadmap corpus |

Every resolved value from a layout or master placeholder reports the
`MatchKind`, so `IdxFamily` (a `body` slide placeholder inheriting from an
`obj` layout placeholder) is visible.

#### 1.3 Property chains

**What *set* means (review M1).** A property is *set* on an element when
the raw element **has the attribute** (for an element group: has a child
that is a member of the group, below). The resolvers test presence on the
raw node (`RawNode::attr` / `Lens::attr`) and parse the present value with
the attribute's generated simple type; they never call the generated
`*_or` accessors, which return the XSD default for an absent attribute and
would stop the chain at the first element of the right type. XSD defaults
are **never** returned as resolved values: a chain that ends without a
value is `Lookup::Unset` (§5), and the documented XSD default is available
separately (`xsd_default()`) for callers who want the specification's
fallback. Attributes on the chains whose XSD default would otherwise leak
(vendored Transitional XSDs), in two kinds:

- **Inherited one by one** (each walks the chain on its own):
  `a:CT_TextCharacterProperties` `cap="none"`, `dirty`, `err`, `smtClean`,
  `smtId`; `a:CT_TextBodyProperties` `upright="false"`. The review's probe
  found 37 slide runs whose `a:rPr` has no `cap` while a layout, master or
  `txStyles` level sets `cap="all"` (`60810.pptx`): a defaulted accessor
  would have reported `none` from the run.
- **Attributes of a group's member element** (review r2-m1): they are read
  **only from the element that won the group** and never looked up further
  along the chain. If that element lacks the attribute, the result is its
  XSD default as `Set`, with the winning element as `source` and
  `defaulted: true`. These are `a:CT_TextNormalAutofit` `fontScale`,
  `lnSpcReduction` (autofit group); `a:CT_TextAutonumberBullet` `startAt`
  (bullet type); `a:CT_TextFont` `pitchFamily`, `charset` (`a:latin` etc.);
  `a:CT_Transform2D`/`a:CT_GroupTransform2D` `rot`, `flipH`, `flipV` (the
  `xfrm` unit, §1.5). A slide `<a:normAutofit/>` under a layout
  `fontScale="90000"` means 100 %, not 90 % (2 `fontScale` and 4
  `lnSpcReduction` cases in the corpus, `bullet-indent.pptx`,
  `slide-section-test.pptx`; review probe 2). Leaks of `startAt`, `rot`,
  `flip*`: 0 in the corpus.
- **Direct only** (never inherited): `p:CT_Placeholder` `type="obj"`,
  `idx="0"`, `orient`, `sz`, `hasCustomPrompt` (§1.2 step 2 needs presence);
  `a:CT_Hyperlink` `history`, `highlightClick`, `endSnd`, `action`,
  `tooltip`, `invalidUrl`, `tgtFrame` (§3.1).

The **level** of a paragraph is `a:pPr@lvl` (default 0, 0–8); "list style
at level L" is the list style's `a:lvl{L+1}pPr` (§21.1.2.4.13). The level
element is used **only at that exact level**: POI falls back to lower
levels in `txStyles` (`getDefaultMasterStyle`), which mattered once in 633
files (`txStyles.level_missing_lower_present`); Phase 1 does not, and the
manual check covers it. `a:defPPr` is **not** consulted (POI and
LibreOffice do not; 24 non-trivial `defPPr` in the corpus, Q3). An invalid
`lvl` (not an integer 0–8, review m11) is reported as `AttrError` on
`ParaProps::level`; steps 1–2 still apply, and the lookup **stops** at the
first level-dependent step it reaches (step 3) with
`Lookup::Unresolved(InvalidLevel)` — no level is guessed, and step 4 is not
consulted, since a value there would hide a step 3 value that cannot be
checked (review r2-m9).

Run properties of an `a:r` or `a:fld` (and of `a:endParaRPr` for the
paragraph-end format) are looked up in this order; the **first element that
sets the property decides**, including when its value is invalid (returned
as `AttrError` with the raw string, ADR-0002 §1.2 "never lossy"):

| # | Placeholder text | Non-placeholder text (`p:sp` text box or shape) | `Source` |
|---|---|---|---|
| 1 | `a:rPr` | `a:rPr` | `Direct` |
| 2 | `a:pPr/a:defRPr` of the paragraph | same | `ParagraphDefault` |
| 3 | shape `a:lstStyle`, level L, `a:defRPr` | same | `ShapeListStyle` |
| 4 | the shape's own `p:style/a:fontRef` (below) | same | `ShapeStyle` |
| 5 | layout placeholder `a:lstStyle`, level L | presentation `p:defaultTextStyle`, level L | `Placeholder{Layout}` / `PresentationDefault` |
| 6 | master placeholder `a:lstStyle`, level L | master `p:txStyles/p:otherStyle`, level L (Q1) | `Placeholder{Master}` / `MasterTextStyle{Other}` |
| 7 | master `p:txStyles`, chosen by the **slide placeholder's own** family: `titleStyle` (title), `otherStyle` (dt, ftr, sldNum, hdr), `bodyStyle` (body); none for `sldImg` | — | `MasterTextStyle` |
| 8 | presentation `p:defaultTextStyle`, level L (ECMA §19.2.1.8 only: POI's `ParagraphPropertyFetcher.getThemeProps` returns `null` for placeholders, review r2-m2b; decides `marL` 327, `b` 5, `sz` 4, fill 4, latin 3, `algn` 3 values in the corpus) | — | `PresentationDefault` |

**Step 4, precisely (review M2).** Only the `p:style` of the shape itself
(a layout or master placeholder's `p:style` is not consulted: 25 layout
placeholders in 11 files have one; manual check M1). `a:fontRef@idx` =
`major` or `minor` supplies the typefaces `+mj-lt`/`+mj-ea`/`+mj-cs` (or
`+mn-*`) for `a:latin`/`a:ea`/`a:cs` respectively, resolved by §1.4;
`idx="none"` supplies no typeface. A colour child of `a:fontRef` supplies a
solid fill; without one, no fill. `a:fontRef` supplies nothing else.

**Notes (review m7).** For a notes placeholder the chain is: steps 1–4;
the matched notes-master placeholder's `a:lstStyle` (by type, §1.2); then,
for the **body** family only, `p:notesMaster/p:notesStyle` (LibreOffice
`getSubTypeTextListStyle` applies `notesStyle` to body types only; POI's
`notesStyle` fallback applies it to every type). `p:defaultTextStyle` is not
consulted for notes. dt/ftr/sldNum/hdr on notes end after the notes-master
placeholder. 281 of 395 notes runs take `sz` from `notesStyle`.

**Paragraph properties** use the same chain without steps 2 and 4, reading
the attributes and children of `a:pPr` / the level element itself.
**Body properties** (`a:bodyPr` attributes and the autofit group, §1.6) use
slide → layout placeholder → master placeholder (`anchor`: 121 / 139 / 618,
326 unset). **Geometry**: §1.5. Table-cell text stops after step 3:
table-style text (`tableStyles.xml` `a:tcTxStyle`) is a Phase 3 topic and
is `Lookup::Unresolved(TableStyle)`, never guessed.

**Groups of child elements.** Where a property is an XSD choice of
elements, the first element in the chain that holds **any** member of the
group decides: fill (`a:noFill`/`a:solidFill`/`a:gradFill`/`a:blipFill`/`a:pattFill`/`a:grpFill`),
bullet type (`a:buNone`/`a:buAutoNum`/`a:buChar`/`a:buBlip`), bullet colour
(`a:buClrTx`/`a:buClr`), bullet size (`a:buSzTx`/`a:buSzPct`/`a:buSzPts`),
bullet font (`a:buFontTx`/`a:buFont`), each of `a:lnSpc`, `a:spcBef`,
`a:spcAft`, and each of `a:latin`, `a:ea`, `a:cs`, `a:sym` separately.
Hyperlinks are **not** inherited (§3.1).

**Why this order** (each with its evidence; counts are values that the
step decides and that would change without it, `chain-633.txt`):

- Step 2 follows §21.1.2.3.2 ("default run level text properties for the
  text runs within a containing paragraph") and POI
  (`fetchParagraphDefaultRunProp`); it decides 23 values in 11 paragraphs.
  Whether PowerPoint honours a paragraph's own `a:defRPr` is **not
  verified** (review m3; python-pptx's author is unsure too) — manual
  check M5.
- Step 4 follows §20.1.4.1.17 (`fontRef`: "which themed font to use along
  with a choice of color"). It is what makes text in an inserted autoshape
  white (880 of 4,001 non-placeholder text shapes have a `fontRef`), and
  PowerPoint writes it on placeholders when a shape quick style is applied
  (`customGeo.pptx`, PowerPoint 14: a `ctrTitle` with
  `<a:fontRef idx="minor"><a:schemeClr val="lt1"/></a:fontRef>` and an
  accent fill — white minor-font text, not `titleStyle`'s `tx1`/`+mj-lt`).
  32 slide placeholders have their own `fontRef`. Step 4 decides 452 fills
  and 91 typefaces (placeholders: 71 and 105 runs supplied). Its position
  before the inherited list styles is part of manual check M1.
- Steps 5–6 for non-placeholder text (Q6 of ADR-0005; review m1). The ECMA
  note (§19.3.1.35) separates **text boxes** (`p:cNvSpPr@txBox="1"`,
  1,139 of 4,001 non-placeholder text shapes) → `bodyStyle` from **other
  shapes** (2,862) → `otherStyle`. Measured where the contested tail is
  reached (`sz`: 417 runs in text boxes, 564 in shapes; latin: 1,046 /
  276):
  - POI's order vs LibreOffice's (`defaultTextStyle` only): **154** sizes
    differ (`otherStyle` supplies a size `defaultTextStyle` lacks, e.g.
    18 pt in `alterman_security.pptx`);
  - `otherStyle` before `defaultTextStyle`: 564 typefaces and 91 sizes
    differ;
  - the ECMA reading: text boxes with `bodyStyle` differ in 415 of 417
    sizes and 526 typefaces (body text is typically 32 pt with bullets,
    not how PowerPoint shows a new text box); shapes with `otherStyle`
    only differ in 305 of 564 sizes and 88 typefaces.

  Decision: POI's order for both kinds, `otherStyle` reported as its own
  `Source`, and the owner's check M1 on a deck with a text box, a shape and
  a placeholder with `p:style`, in which the readings disagree (Q1).
  `txBox` is exposed (`ShapeRef::is_text_box`) so a different outcome can
  be expressed per kind.
- dt/ftr/sldNum use `otherStyle` (POI). LibreOffice skips `txStyles` for
  them, and also for `pic`/`tbl`/`chart` placeholders (`pptshape.cxx`
  `getSubTypeTextListStyle`); the dt/ftr/sldNum difference shows in 52
  values (51 typefaces, 1 size) and is in manual check M1. `txStyles` is
  selected by the slide placeholder's own type (POI
  `getDefaultMasterStyle` reads the shape's `p:ph`), the master
  placeholder by the layout's (above); with `IdxFamily`/`IdxTypeMismatch`
  matches the two can differ, which `Source` shows.

#### 1.4 Theme references

- **Fonts (review M3).** A typeface `+mj-lt`, `+mj-ea`, `+mj-cs`,
  `+mn-lt`, `+mn-ea`, `+mn-cs` refers to the theme's
  `a:fontScheme/a:majorFont|a:minorFont` `a:latin|a:ea|a:cs@typeface`
  (§19.3.1.52 example, §20.1.4.1.24–25; POI `XSLFTextRun` l. 807–832). A
  **non-empty** theme typeface is the resolved value. An **empty** (`""`)
  or missing one is not a value: Office's own themes leave `a:ea`/`a:cs`
  empty and list script-specific `a:font script="Jpan"|"Hang"|"Hans"|…`
  entries instead, and which one applies depends on the run's language and
  the application's editing languages. Such a reference is
  `Lookup::Unresolved(ScriptDependent { slot, script_fonts })`, carrying
  the theme's `(script, typeface)` list. Measured (review probe 4, 633
  files): of 581 masters whose text styles or placeholders refer to
  `+mn-ea`/`+mj-ea`, **548** have an empty theme `a:ea` (major and minor
  alike), with 972 `Jpan` script entries; for the latin slot, 4,021 of
  4,025 references resolve and the 4 others (`bug64693.pptx`, an
  `a:latin typeface="+mn-ea"`) are `ScriptDependent` too. A helper outside
  `Lookup` maps `lang`/`altLang` to a script (`ja`→`Jpan`, `ko`→`Hang`,
  `zh-CN`→`Hans`, `zh-TW`/`zh-HK`→`Hant`) and picks a candidate from the
  list — documented as a guess, never reported as `Set` (Q4).
- **Colours.** `a:schemeClr@val` names a theme slot through the effective
  colour map of the **slide being read** (an inherited colour is evaluated
  in the slide's context): the slide's `p:clrMapOvr/a:overrideClrMapping`,
  else the layout's, else the master's `p:clrMap`. The map is applied to
  every name it has (`bg1`, `tx1`, `bg2`, `tx2`, `accent1–6`, `hlink`,
  `folHlink`); `dk1`… are theme slots already; `phClr` is
  `Lookup::Unresolved(PlaceholderColor)`. **Evidence and a disagreement
  (review m2):** ECMA §19.3.1.7 says that with `a:masterClrMapping` on the
  slide "the color scheme defined by the master is used" (also §20.1.6.6),
  which would skip a layout's override. POI (`XSLFSlide`/`XSLFSlideLayout`/
  `XSLFSheet.mapSchemeColor`: slide override, else the layout's, else the
  master's) and LibreOffice's test deck `layout-clrmap-override.pptx` use
  the layout's override, and so does Phase 1. All 10 slides in the corpus
  that inherit a layout override carry `a:masterClrMapping`; in 6 the
  layout's map differs from the master's. Manual check M6 decides.
  Corpus (`chain-633.txt`, revision 2 rules): 5,616 scheme colours, all
  resolve (4,848 through `bg/tx` mapping, 768 direct); 3 slides override
  the map themselves. **`themeOverride`** (review r2-m7): a slide or layout
  may relate to a theme-override part (`…/relationships/themeOverride`; 1 in
  the corpus, `60810.pptx` slide 18, a `a:clrScheme` equal to the master's
  theme). Phase 1 does not apply it: on a part with such a relationship,
  every theme-dependent lookup (scheme colours, `+mj`/`+mn` fonts) is
  `Lookup::Unresolved(ThemeOverride)`.
  `a:srgbClr`, `a:sysClr` (`@lastClr`), `a:prstClr`, `a:hslClr`,
  `a:scrgbClr` are returned as written. **Colour transforms** (`a:lumMod`,
  `a:tint`, `a:alpha`, …) are returned as a list of
  `(name, DmlPercent | raw)` and **not applied**; computing the final RGB
  (§20.1.2.3) is Phase 3 with formatting writes.

#### 1.5 Geometry, and what is not resolved

**Geometry** (`a:xfrm`/`p:xfrm`: `off`+`ext` with `rot`, `flipH`, `flipV`,
as one unit; an `a:xfrm` lacking `off` or `ext` supplies nothing — 0 in the
corpus) uses slide → layout placeholder → master placeholder. The shape's
outline is the XSD choice `a:prstGeom | a:custGeom` (review m5a): the first
element in the chain with **either** decides, so a slide `a:custGeom` is
never overridden by a layout `rect` (0 such slides in the corpus).
**Placeholders in groups** (review m5b): a group member's `a:xfrm` is in its
group's child coordinate space, so geometry is **not** inherited across a
group boundary in either direction — a slide placeholder inside a group, or
one matched to a layout placeholder inside a group, gets
`Lookup::Unresolved(GroupCoordinates)` for geometry (text properties still
resolve). 0 such placeholders in the corpus.

**Not resolved in Phase 1:** table-style text and cell fills, theme style
matrices (`a:fillRef`, `a:lnRef`, `a:effectRef` → shape fill/line),
backgrounds, SmartArt and chart text formatting, and slide-space rectangles
of shapes inside rotated or flipped groups (`slide_rect()` composes
`a:chOff`/`a:chExt` scaling and returns `Err(TransformedGroup)` if any
enclosing group has `rot`, `flipH` or `flipV`).

#### 1.6 Specified values, not displayed values (review M4)

Every `RunProps`/`ParaProps`/`BodyProps` value is the value **specified**
by the chain. It is not the value PowerPoint displays where the
application adjusts it at layout time. Two such adjustments are exposed as
inputs, not applied:

- **Autofit.** `BodyProps::autofit` is a lookup of the group
  `a:noAutofit | a:normAutofit | a:spAutoFit` along the body chain (§1.3).
  The kind inherits. The `normAutofit@fontScale`/`@lnSpcReduction`
  numbers are a layout cache computed for **one** shape (64 slide
  placeholders in the corpus carry them, e.g. `fontScale="92500"
  lnSpcReduction="10000"` in `60810.pptx`): set on the shape itself they are
  `Set` (absent on the winning element → its XSD default, 100 %/0 %,
  `defaulted: true`, §1.3); when the winning `normAutofit` is a layout's or
  master's, they are `Unresolved(InheritedAutofitScale)` (review probe 2:
  4 slide bodies inherit a `fontScale` this way, all in `tdf128550.pptx`,
  from the master). The displayed size is
  `sz × fontScale` and the line spacing is reduced by `lnSpcReduction`; the
  caller decides whether to apply them. If no element of the chain has the
  group, the result is `Unset`. ECMA §21.1.2.1.2–4 says noAutofit is then
  implied; inheriting the group from layout/master, as Phase 1 does, is
  what POI and PowerPoint-written layouts suggest but is not verified
  (manual check M7).
- **Hyperlink colour.** PowerPoint draws linked text in the theme's
  `hlink` colour, not the run's fill, unless the link carries
  `a:hlinkClick/a:extLst/…/ahyp:hlinkClr val="tx"` (namespace
  `http://schemas.microsoft.com/office/drawing/2018/hyperlinkcolor`; the
  review cites [MS-ODRAWXML] and Microsoft's support article, not
  re-verified here; manual check M7). Of 83 linked runs, 33 have their own
  `a:solidFill` and 12 have `ahyp:hlinkClr` (review probe 5).
  `Link::color_rule()` returns `ThemeHyperlink` or `RunColor` from that
  extension (read raw); `RunProps::fill` stays the specified fill. The rule
  is about `a:hlinkClick` only: `a:hlinkMouseOver` does not change the
  colour, so `color_rule()` is `None` for a run with only a mouse-over link
  (review r2-m10). Linked runs whose action is not a URL (in the corpus:
  `hlinksldjump` 18, `macro` 11, `hlinkshowjump` 1, `hlinkfile` 1) are
  assumed to follow the same rule; manual check M7 confirms.

#### 1.7 Notes, layouts and masters as contexts (review r2-m6)

**Notes slides.** The colour map is the notes slide's `p:clrMapOvr`, else
the notes master's `p:clrMap`; the theme is the notes master's; geometry
is notes slide → notes-master placeholder (by type, §1.2); text as §1.3
"Notes".

**Shapes on layouts and masters** (`LayoutRef`/`MasterRef` shapes, read by
the same API) resolve in their own part's context:

- a layout placeholder: steps 1–4 on the layout shape → the master
  placeholder of its family → master `txStyles` (by its own family) →
  `defaultTextStyle`; geometry layout → master placeholder; colour map the
  layout's `p:clrMapOvr`, else the master's `p:clrMap`;
- a master placeholder: steps 1–4 → master `txStyles` → `defaultTextStyle`;
  geometry its own; colour map the master's;
- a non-placeholder shape on a layout or master: steps 1–4 →
  `defaultTextStyle` → master `otherStyle`.

POI skips steps 2–5 for shapes on a master (`fetchProperty` checks
`sheet instanceof XSLFSlideMaster`); that difference is in O2b's table.

### 2. Notes: read and edit existing notes slides

- `Slide::notes()` → `Option<NotesRef>` (none for 921 of 1,272 slides);
  `Presentation::notes_mut(slide_index) -> Result<NotesMut>`, refused with
  `NoNotesSlide` when there is none (creating a notes slide needs a part, a
  relationship pair and a content type: Phase 3).
- The **notes body** is the first `p:sp` whose `p:ph@type="body"` in the
  notes shape tree, as python-pptx's `notes_placeholder` (340 notes slides
  have exactly one; 11 have none → `NoNotesBody`; none has two).
- `NotesMut::body_mut() -> Result<TextBodyMut>` gives **exactly** the Phase 0
  text API (`ParagraphMut::set_text`, `TextBodyMut::set_text`, `add_*`,
  `remove_*`, `RunMut::set_text`) with all its rules: the text-argument
  table, fields, `UnsupportedContent`, format sources (Phase 0.1 subtree
  copy), `FormatNotCopyable`, `NamespaceNotInScope`, `TooDeep`, and "a
  refused edit leaves the package byte-identical". The descent is
  `p:notes → p:cSld → p:spTree → p:sp → p:txBody` with generated `ChildOf`
  edges (`p:notes` is a new read entry, §6).
- Locks: notes parts carry no `p:timing` (0 in 351) and no comment anchors
  point into notes, so the animation and comment locks of ADR-0005 §6 never
  apply; the `mc:AlternateContent` rule does (0 in the corpus).
- Only the notes part becomes dirty; the notes master, the slide and the
  relationships are untouched. A notes slide whose back relationship does
  not point at its slide (1 file, `sc054-074_google_slides.pptx`) is still edited; Phase 1
  does not validate it.
- Notes text reads: `NotesRef::text()` with the Phase 0 conventions (`\v`
  for `a:br`, field cached text, `\n` between paragraphs); preview against
  python-pptx `notes_slide.notes_text_frame.text`: 340 of 340 equal
  (`ppy_links-633.txt`).

### 3. Read APIs

#### 3.1 Hyperlinks and actions

Locations: `a:rPr/a:hlinkClick` and `a:hlinkMouseOver` on runs and fields
(§21.1.2.3.5–6), `p:cNvPr/a:hlinkClick` and `a:hlinkHover` on shapes
(§20.1.2.2.8, §20.1.2.2.23), on slides and notes. Layout/master links (5
in the corpus) are read through the same API on layout/master shapes.

```rust
pub struct Link<'a> {
    pub event: LinkEvent,                 // Click | Hover
    pub target: LinkTarget<'a>,
    pub rel_id: Option<&'a str>,          // "" is normalised to None
    pub action: Option<&'a str>,          // raw @action
    pub tooltip: Option<&'a str>,
    pub highlight_click: bool, pub end_sound: bool, pub history: bool,
}
pub enum LinkTarget<'a> {
    Url(&'a str),                                   // no action, external relationship
    Part(PartName),                                 // no action, internal relationship
    Slide { slide_id: u32, index: usize },          // ppaction://hlinksldjump
    Jump(ShowJump),                                 // ppaction://hlinkshowjump?jump=…
    CustomShow { id: &'a str, return_after: bool }, // ppaction://customshow?id=…&return=…
    Presentation(&'a str), File(&'a str), Program(&'a str), // hlinkpres / hlinkfile / program (+ r:id)
    Macro(&'a str), Ole(&'a str), Media, NoAction,  // macro?name= / ole?verb= / media / noaction
    Unknown(&'a str),                               // any other action, raw
    Dangling { rel_id: &'a str },                   // r:id not in the part's relationships
}
pub enum ShowJump { First, Last, Next, Previous, LastViewed, EndShow }
```

The vocabulary is python-pptx's `ActionSetting.action` (`action.py`) plus
`media` and `noaction`, which occur in the corpus. Measured
(`features-633.txt`): 84 run links and 94 shape links on slides; no action
→ URL 65, → internal part 4; `hlinksldjump` 60 (all resolve to a listed
slide); `hlinkshowjump` 12; `macro` 13; `media` 14; `noaction` 9;
`hlinkfile` 1. An empty `r:id=""` with an action means "no relationship"
(python-pptx raises `KeyError` on 16 such runs). Preview against
python-pptx's `click_action` / `run.hyperlink.address`: 92 shape links and
46 run links agree, 0 differ (`ppy_links-633.txt`).

API: `RunRef::link(LinkEvent)`, `FieldRef::link`, `ShapeRef::action(LinkEvent)`,
`Slide::links()` (every link with its shape path / paragraph / item), and
`Link::color_rule() -> LinkColor { ThemeHyperlink, RunColor }` (§1.6). The
boolean fields are the link's own attributes with their XSD defaults
(direct values; links are never inherited, so §1.3's presence rule does not
apply to them).

#### 3.2 Sections (p14)

`Presentation::sections() -> Result<Option<Sections>>` reads
`p:presentation/p:extLst/p:ext[@uri="{521415D9-36F7-43E2-AB2F-B90AF26B5E84}"]/p14:sectionLst`
(the SDK lists `p14:sectionLst` as a child of `p:CT_PresentationExtension/p:ext`).

```rust
pub struct Section<'a> { pub name: &'a str, pub id: &'a str, pub slide_ids: Vec<u32>, pub slides: Vec<Option<usize>> }
pub struct Sections<'a> { pub sections: Vec<Section<'a>>, pub unsectioned: Vec<usize>, pub duplicated: Vec<u32>, pub unknown: Vec<u32> }
```

`slides[i]` is `None` for an id that is not in `p:sldIdLst`. Corpus: 21
decks, 59 sections, 35 empty, and **no** unsectioned slide, duplicate,
unknown id or order disagreement with `p:sldIdLst` — the consistency fields
exist for Phase 2, which must keep them empty. An `ext` with another `uri`
is ignored.

#### 3.3 Comments, both formats

```rust
pub enum Comment<'a> { Legacy(LegacyComment<'a>), Modern(ModernComment<'a>) }
pub struct LegacyComment<'a> {        // p:cm (§19.4.1)
    pub author: Result<LegacyAuthor<'a>, UnknownAuthor>, // p:cmAuthor by @authorId (§19.4.2)
    pub created: Option<&'a str>, pub idx: Option<u32>,
    pub pos: Option<(i64, i64)>,      // p:pos, raw; units are not specified by §19.4.5
    pub text: &'a str,                // p:text
    pub parent: Option<(u32, u32)>,   // p:extLst/…/p15:threadingInfo/p15:parentCm (authorId, idx)
}
pub struct ModernComment<'a> {        // p188:cm
    pub id: &'a str, pub author: Result<ModernAuthor<'a>, UnknownAuthor>, // p188:author by GUID
    pub created: Option<&'a str>, pub status: Option<&'a str>,
    pub anchor: Anchor,               // ADR-0005 §6 classification
    pub pos: Option<(i64, i64)>, pub body: Option<TextBodyRef<'a>>,
    pub replies: Vec<Reply<'a>>,      // p188:replyLst/p188:reply (id, author, created, body)
}
pub enum Anchor { Slide, Shapes(Vec<u32>), Text { shape_id: u32, cp: u32, len: u32 }, Other(String) }
```

`Slide::comments()` returns legacy comments (slide relationship
`…/relationships/comments`, authors from the presentation's
`commentAuthors`) then modern ones (`http://schemas.microsoft.com/office/2018/10/relationships/comments`,
authors from `…/2018/10/relationships/authors`). The anchor classifier is
the one Phase 0 uses for locks (`refs.rs`), so reading and locking cannot
disagree. A part that cannot be read gives `Err` for that slide's comments
only. Other `p188:cm` attributes (`tags`, `likes`, `startDate`, `dueDate`,
`assignedTo`, `complete`, `priority`, `title`) are reachable through the
generated view. Corpus: 11 legacy comments in 5 decks, 1 modern comment
with 2 replies — so the oracle is hand-built (§7).

#### 3.4 Transitions

`Slide::transition() -> Result<Option<Transition>>`:

```rust
pub struct Transition<'a> {
    pub branch: Branch<'a>,              // Direct | Choice { requires: &'a str } | Fallback
    pub kind: Option<TransitionKind<'a>>, // the child element: QName + raw attributes (p:fade, p14:vortex, p15:prstTrans@prst, …)
    pub speed: Speed,                    // @spd, default fast (XSD)
    pub duration_ms: Option<u32>,        // @p14:dur
    pub advance_on_click: bool,          // @advClick, default true (§19.3.1.50)
    pub advance_after_ms: Option<u32>,   // @advTm
    pub sound: Option<SoundAction<'a>>,  // p:sndAc
}
```

A `p:transition` directly under `p:sld` (240 in the corpus) is `Direct`.
For `mc:AlternateContent` (83 slides, all with a `mc:Fallback`), the first
`mc:Choice` whose `Requires` prefixes are all **understood** is taken, else
the Fallback (ECMA-376 Part 3). Each prefix in `Requires` is resolved to a
namespace URI with the declarations in scope at that `mc:Choice` and
compared **by URI**, never by prefix text (review m9); an undeclared prefix
makes that Choice not understood. Understood URIs:
`http://schemas.microsoft.com/office/powerpoint/2010/main` (p14),
`…/2012/main` (p15) and `…/2015/09/main` (p159, morph; not in the SDK data,
but Phase 1 reports the kind as a raw QName anyway, so it can name it —
review Q5). So the morph slide reports `p159:morph` from its Choice, as
current PowerPoint shows it. `Slide::transition_branches()` returns every
branch.
Corpus `Requires`: `p14` 81, `p15` 1, `p159` 1. Layouts (509) and masters
(63) also carry transitions; whether PowerPoint applies them to a slide
without its own is unverified (manual check M3), so `Slide::transition`
returns the slide's own only and `Layout::transition`/`Master::transition`
exist separately.

#### 3.5 Timing summary

`Slide::timing() -> Result<Option<Timing>>`, read-only, built on the same
walk as Phase 0's lock scan (`refs.rs`):

```rust
pub struct Timing<'a> { pub effects: Vec<Effect<'a>>, pub builds: Vec<Build<'a>> }
pub struct Effect<'a> {
    pub ctn_id: Option<u32>, pub preset_class: Option<&'a str>, // entr exit emph path verb mediacall
    pub preset_id: Option<u32>, pub preset_subtype: Option<u32>,
    pub node_type: Option<&'a str>,     // clickEffect withEffect afterEffect …
    pub click_index: Option<usize>,      // ordinal of the enclosing clickPar in the main sequence (see the post-acceptance note: clickEffect count)
    pub targets: Vec<Target>,
}
pub enum Target { Shape(u32), Paragraphs(u32, u32, u32) /* spid, st, end */, Chars(u32, u32, u32), Background(u32), Other { spid: Option<u32>, element: String } }
pub enum Build<'a> { Paragraph { spid: u32, build: &'a str, group: Option<u32> }, Graphic { spid: u32 }, Other(String) }
```

Corpus: 378 `p:timing`, effects `entr` 220, `exit` 30, `mediacall` 19,
`path` 17, `emph` 13; 113 targets with `p:txEl`; 123 `p:bldP`, 3
`p:bldGraphic`. Anything unknown becomes `Other`, never an error; an
`mc:AlternateContent` inside timing (0 in the corpus) follows §3.4's branch
rule.

### 4. Cross-run find/replace

```rust
impl Presentation {
    pub fn find(&mut self, pattern: &str, opts: &FindOptions) -> Result<Vec<Match>>;
    pub fn replace_all(&mut self, pattern: &str, replacement: &str, opts: &ReplaceOptions) -> Result<ReplaceReport>;
}
pub struct FindOptions { pub scope: Scope /* Slides | SlidesAndNotes | Slide(i) | Notes(i) */, pub ascii_case_insensitive: bool }
pub struct ReplaceOptions { pub find: FindOptions, pub mixed_format: MixedFormat /* Allow | Refuse */, pub on_refusal: OnRefusal /* Skip | Abort */ }
pub struct Match { pub location: TextLocation /* part, shape path or cell, paragraph */, pub range: Range<usize> /* bytes in the paragraph text */, pub items: Range<usize> }
pub struct ReplaceReport { pub replaced: Vec<Match>, pub skipped: Vec<(Match, Error)> }
```

**Domain.** Every text body Phase 0 can address on the slides in scope:
`p:sp` text bodies including group members, table cells, and (with
`SlidesAndNotes`/`Notes`) the notes body. Layouts, masters, SmartArt,
charts and comment bodies are never searched. Shapes inside
`mc:AlternateContent` are searched, and every match there is skipped with
`InsideAlternateContent`.

**Paragraph text** is the concatenation of its items as in Phase 0:
`a:r` text, `a:br` = `"\v"`, `a:fld` = cached text. Matches never cross
paragraphs. Matching is literal and case-sensitive by default
(`ascii_case_insensitive` folds ASCII letters only, so byte offsets stay
valid), left to right and non-overlapping (`str::match_indices`). The
pattern must be non-empty and must not contain `\n`, `\r` or `\v`
(`InvalidPattern`).

**Edit of one match** (consistent with ADR-0005 §3: "the first run keeps its
format", fields are never removed implicitly):

1. A match that covers any part of an `a:fld` → `FieldInMatch`; of an
   `a:br` → `BreakInMatch`.
2. The **start run** (the `a:r` where the match starts) keeps its `a:rPr`
   (including an `a:hlinkClick`, as Phase 0's `Paragraph::set_text`) and
   becomes `prefix + replacement (+ suffix if the match ends in it)`.
3. Runs fully covered by the match are removed; a partly covered last run
   is trimmed and keeps its `a:rPr`.
4. A touched run left with empty text is removed, unless no `a:r` would
   remain in the paragraph; then the start run stays with an empty `a:t`
   (as `Paragraph::set_text("")`).
5. Replacement text follows `Paragraph::set_text`'s row of the text-argument
   table (`\n`, `\r` → `LineBreakInParagraph`; other non-XML characters →
   `InvalidXmlChar`). A `\v` splits it (revision 2, review M5): the start
   run keeps `prefix + segment 0`; each later segment becomes an `a:br`
   followed by a new run (none for an empty segment unless the suffix goes
   there) whose `a:rPr` is the Phase 0.1 subtree copy, without links, of
   the **start run's** `a:rPr` — not the paragraph's first `a:rPr` as in
   `Paragraph::set_text`, because the replacement takes the start run's
   format; so `FormatNotCopyable` can arise from the start run. The suffix
   (the start run's text after a match that ends inside it) is appended to
   the last new run. That would move kept characters out of a link, so a
   match that **ends inside a start run with `a:hlinkClick` or
   `a:hlinkMouseOver`**, with a `\v` in the replacement, is refused with
   `LinkSplitByBreak` (copying the link would duplicate its `r:id`, which
   ADR-0005 rev 4 rules out). A match that ends at the end of a linked run
   is fine: only replacement text goes into the link-free runs.
6. `MixedFormat::Refuse` refuses (`MixedFormatInMatch`) a match whose covered
   runs differ in `a:rPr` other than the proofing attributes `lang`,
   `altLang`, `err`, `dirty`, `smtClean`, `smtId`, `noProof`, `bmk`, or in
   their `a:hlinkClick`. The default `Allow` takes the start run's format,
   which is what typing over a selection does.
7. Locks of ADR-0005 §6 apply per shape: character-level animation
   (`AnimatedCharacters`) and text-anchored comments (`CommentAnchoredText`)
   refuse; paragraph-level animation does not (the paragraph count never
   changes).

No "merge proofing-only runs first" step (roadmap §3.4 proposed one): rules
2–4 already produce the start run's visible format, and merging would
rewrite runs the match does not touch (T2 locality).

**Atomicity.** `replace_all` plans every match of the scope (all checks of
§4 and ADR-0005 §3/§6) before the first change. `OnRefusal::Skip`
(default) applies the others and lists each refused match with its error;
`OnRefusal::Abort` changes nothing if any match is refused. Within a
paragraph, matches are applied right to left so earlier offsets stay valid.
Only parts that receive a replacement become dirty.

**Evidence** (`findreplace-105.txt` / `findreplace-633.txt`; every 1-, 2-
and 3-word phrase of every slide paragraph as a pattern):

| Match class | 105 files (46,093 matches) | 633 files (59,716) |
|---|---:|---:|
| inside one run | 93.1 % | 84.7 % |
| across runs differing only in proofing attributes | 2.9 % | 4.0 % |
| across visibly different runs | 3.2 % | 10.4 % |
| across identical runs | 0.1 % | 0.1 % |
| covers an `a:br` / an `a:fld` (refused) | 0.4 % / 0.2 % | 0.5 % / 0.3 % |
| (subset) across runs with different `a:hlinkClick` | 0.1 % (35) | 0.1 % (39) |

The spike's reference model of rules 1–5 was applied to 42,933 / 55,611
pattern–paragraph pairs, as many deletions, and (revision 2) replacements
containing a line break (`"X\v" + reversed pattern` and `"\v"`; pairs with
a refused match are skipped by the model). Both invariants held every time:
**T** the new paragraph text equals `str.replace` on the old one; **F**
every kept character keeps its old `a:rPr`, every inserted character before
the first `\v` has the start run's `a:rPr`, and every inserted character
after it has the start run's `a:rPr` without links (an inserted `a:br` has
none). Replacements with `\v`: 85,742 / 111,064 applied with both
invariants holding (358 / 506 in paragraphs that contain a linked run);
124 / 158 refused with `LinkSplitByBreak`. Revision 3 reran the spike with
`a:hlinkMouseOver` counted as a link (review r2-m3; the earlier spike saw
`a:hlinkClick` only, so a mouse-over-only start run would have broken F):
the counts are unchanged, since no probe match in the corpus ends inside a
mouse-over-only run; the case is covered by a synthetic test in the
implementation (PR #48, `rule_5_link_split_by_break`). (Revision 1's figure for the
link subset, 0.2 %, double-counted matches; corrected to 0.1 %.)

### 5. API sketch over the ADR-0002 views

Hand-written domain types wrap generated views, as in Phase 0. Views of
different elements with one SDK complex type (`a:rPr`, `a:endParaRPr`,
`a:defRPr`; `a:pPr`, `a:defPPr`, `a:lvl1pPr`…`a:lvl9pPr`; `a:lstStyle`,
`p:titleStyle`, `p:bodyStyle`, `p:otherStyle`, `p:notesStyle`,
`p:defaultTextStyle`) are unified by small enums in `ooxml-pptx`
(`CharPropsRef`, `ParaPropsRef`, `ListStyleRef`) with a delegating
`macro_rules!`; no generator change. Presence is tested on the raw node and
values are parsed with the generated attribute specs (§1.3 "What *set*
means"); the defaulted `*_or` accessors are not used by resolvers.

```rust
impl Presentation {
    /// Parses the presentation, every slide, its chain parts and notes once;
    /// later calls are cheap. Never dirties anything.
    pub fn deck(&mut self) -> Result<Deck<'_>>;
    pub fn notes_mut(&mut self, slide: usize) -> Result<NotesMut<'_>>;          // §2
    pub fn sections(&mut self) -> Result<Option<Sections<'_>>>;                 // §3.2
    pub fn find(..); pub fn replace_all(..);                                    // §4
}
impl<'a> Deck<'a> {
    pub fn slide(&self, i: usize) -> Result<Slide<'a>>;   // Phase 0 Slide plus:
    pub fn layouts(&self) -> Vec<LayoutRef<'a>>; pub fn masters(&self) -> Vec<MasterRef<'a>>;
    pub fn slide_size(&self) -> Option<(i64, i64)>;       // p:sldSz
}
impl<'a> Slide<'a> {
    pub fn layout(&self) -> Result<LayoutRef<'a>>;  pub fn master(&self) -> Result<MasterRef<'a>>;  pub fn theme(&self) -> Result<ThemeRef<'a>>;
    pub fn color_map(&self) -> Result<Resolved<'a, ColorMap>>;
    pub fn notes(&self) -> Option<NotesRef<'a>>;  pub fn comments(&self) -> Result<Vec<Comment<'a>>>;
    pub fn transition(&self) -> Result<Option<Transition<'a>>>;  pub fn timing(&self) -> Result<Option<Timing<'a>>>;
    pub fn links(&self) -> Vec<(TextLocation, Link<'a>)>;
}
impl<'a> ShapeRef<'a> {
    pub fn placeholder_match(&self) -> Result<Option<PlaceholderMatch<'a>>>;   // layout + master + MatchKind + ambiguous
    pub fn is_text_box(&self) -> bool;                                          // p:cNvSpPr@txBox (§1.3, Q1)
    pub fn geometry(&self) -> Result<Lookup<'a, Xfrm>>;                         // §1.5
    pub fn outline(&self) -> Result<Lookup<'a, Outline<'a>>>;                   // Preset(&str) | Custom (a:custGeom)
    pub fn body_props(&self) -> Result<BodyProps<'a>>;                          // each field a Lookup; autofit §1.6
    pub fn action(&self, ev: LinkEvent) -> Option<Link<'a>>;
    pub fn slide_rect(&self) -> Result<Option<Rect>>;                           // §1.5
}
impl<'a> ParagraphRef<'a> { pub fn props(&self) -> Result<ParaProps<'a>>; pub fn end_props(&self) -> Result<RunProps<'a>>; }
impl<'a> RunRef<'a> { pub fn props(&self) -> Result<RunProps<'a>>; pub fn link(&self, ev: LinkEvent) -> Option<Link<'a>>; }

/// One property after the chain (review m8).
pub enum Lookup<'a, T> {
    Set(Resolved<'a, T>),   // an element of the chain sets it
    Unset,                  // no element sets it: the application's default applies; never invented
    Unresolved(Reason<'a>), // the chain cannot decide in Phase 1
}
pub enum Reason<'a> {
    TableStyle, PlaceholderColor /* phClr */, InvalidLevel, GroupCoordinates, ThemeOverride,
    InheritedAutofitScale { from: Source<'a> },
    ScriptDependent { slot: ThemeFontSlot /* MajorEa, MinorCs, … */, script_fonts: Vec<(&'a str, &'a str)> },
}
pub struct Resolved<'a, T> {
    pub value: Result<T, AttrError>,
    pub source: Source<'a>,
    pub defaulted: bool,    // the winning group member lacked the attribute: its XSD default (§1.3, r2-m1)
}
pub enum Source<'a> {
    Direct,                                     // on the element asked about
    ParagraphDefault, ShapeListStyle { level: u8 }, ShapeStyle,
    Placeholder { role: Role /* Layout | Master | NotesMaster */, part: &'a PartName, shape_id: u32, matched: MatchKind, level: Option<u8> },
    MasterTextStyle { part: &'a PartName, style: TextStyleKind /* Title | Body | Other | Notes */, level: u8 },
    PresentationDefault { level: u8 },
}
pub struct RunProps<'a> {                       // values as *specified* (§1.6), every field a Lookup
    pub size: Lookup<'a, u32>,                  // sz, 1/100 pt; autofit's fontScale not applied
    pub bold, italic, underline, strike, caps, baseline /* DmlPercent */, spacing, kern, lang, …
    pub latin: Lookup<'a, Font<'a>>, pub east_asian, pub complex_script, pub symbol,
    pub fill: Lookup<'a, Fill<'a>>,             // a linked run is drawn per Link::color_rule()
}
pub struct Font<'a> { pub typeface: &'a str, pub theme: Option<ThemeFontRef<'a>> /* major/minor, slot, theme part */ }
pub enum Fill<'a> { None, Solid(Color<'a>), Other(&'a str) /* gradFill, blipFill, pattFill, grpFill: element name */ }
pub struct Color<'a> { pub spec: ColorSpec<'a>, pub mapped: Option<SchemeMapping<'a>>, pub base_rgb: Option<[u8; 3]>, pub transforms: Vec<ColorTransform<'a>> }
pub struct Autofit { pub kind: AutofitKind /* None | Normal | Shape */, pub font_scale: Lookup<'static, DmlPercent>, pub line_spacing_reduction: Lookup<'static, DmlPercent> }
pub fn xsd_default(property: PropertyId) -> Option<&'static str>;  // the specification's fallback, for callers who want it
```

Every `Lookup` keeps borrowing the parsed parts, so the API is zero-copy;
the `Deck` borrow is shared, and `notes_mut`/`replace_all` take
`&mut Presentation` (a `Deck` must be dropped first, as Phase 0's `Slide`
before `slide_mut`).

New errors: `BrokenChain`, `NoNotesSlide`, `NoNotesBody`, `InvalidPattern`,
`FieldInMatch`, `BreakInMatch`, `MixedFormatInMatch`, `LinkSplitByBreak`,
`TransformedGroup`; reused: everything in ADR-0005 §6.

### 6. Allowlist and overrides

**Allowlist.** 124 new `[read]` entries, no new `[write]` entry (Phase 1
writes text only, through Phase 0's write entries and core's
`insert_detached`). The exact list is
[`spikes/pptx-phase1/allowlist_phase1.toml`](../../spikes/pptx-phase1/allowlist_phase1.toml);
`p14:` and `p188:` entries go in `pml.toml` (the feature group is the file,
ADR-0002 post-acceptance note).

| Group | `pml.toml` (47) | `dml.toml` (77) |
|---|---|---|
| parts | `p:CT_SlideLayout/p:sldLayout`, `p:CT_SlideMaster/p:sldMaster`, `p:CT_NotesSlide/p:notes`, `p:CT_NotesMaster/p:notesMaster` | `a:CT_OfficeStyleSheet/a:theme` |
| text styles | `p:CT_SlideMasterTextStyles/p:txStyles`, `a:CT_TextListStyle/p:{titleStyle,bodyStyle,otherStyle,notesStyle,defaultTextStyle}` | `a:CT_TextListStyle/a:lstStyle`, `a:CT_TextParagraphProperties/a:{defPPr,lvl1pPr…lvl9pPr}`, `a:CT_TextCharacterProperties/a:defRPr` |
| paragraph/run/body properties | — | `a:CT_TextSpacing/a:{lnSpc,spcBef,spcAft}`, `a:CT_TextSpacingPercent/a:spcPct`, `a:CT_TextSpacingPoint/a:spcPts`, `a:CT_TextNoBullet/a:buNone`, `a:CT_TextAutonumberBullet/a:buAutoNum`, `a:CT_TextCharBullet/a:buChar`, `a:CT_TextBlipBullet/a:buBlip`, `a:CT_TextBulletSizePercent/a:buSzPct`, `a:CT_TextFont/a:{buFont,latin,ea,cs,sym}`, `a:CT_Color/a:buClr`, revision 2 (review m4): `a:CT_TextBulletColorFollowText/a:buClrTx`, `a:CT_TextBulletSizeFollowText/a:buSzTx`, `a:CT_TextBulletSizePoint/a:buSzPts`, `a:CT_TextBulletTypefaceFollowText/a:buFontTx`, `a:CT_TextBodyProperties/a:bodyPr`, `a:CT_TextNormalAutofit/a:normAutofit`, `a:CT_TextNoAutofit/a:noAutofit`, `a:CT_TextShapeAutofit/a:spAutoFit` |
| colour | `a:CT_ColorMapping/p:clrMap`, `a:CT_ColorMappingOverride/p:clrMapOvr` | `a:CT_ColorMapping/a:overrideClrMapping`, `a:CT_EmptyElement/a:masterClrMapping`, `a:CT_SolidColorFillProperties/a:solidFill`, `a:CT_NoFillProperties/a:noFill`, `a:CT_{SRgbColor/a:srgbClr, SchemeColor/a:schemeClr, SystemColor/a:sysClr, ScRgbColor/a:scrgbClr, PresetColor/a:prstClr, HslColor/a:hslClr}` |
| geometry, shape style | `a:CT_ShapeProperties/p:spPr`, `a:CT_GroupShapeProperties/p:grpSpPr`, `a:CT_Transform2D/p:xfrm`, `a:CT_ShapeStyle/p:style`, `p:CT_SlideSize/p:sldSz` | `a:CT_Transform2D/a:xfrm`, `a:CT_GroupTransform2D/a:xfrm`, `a:CT_Point2D/a:{off,chOff}`, `a:CT_PositiveSize2D/a:{ext,chExt}`, `a:CT_PresetGeometry2D/a:prstGeom`, `a:CT_CustomGeometry2D/a:custGeom` (revision 2, review m5a), `a:CT_FontReference/a:fontRef` |
| theme | — | `a:CT_BaseStyles/a:themeElements`, `a:CT_ColorScheme/a:clrScheme`, `a:CT_Color2/a:{dk1,lt1,dk2,lt2,accent1…accent6,hlink,folHlink}`, `a:CT_FontScheme/a:fontScheme`, `a:CT_FontCollection/a:{majorFont,minorFont}`, `a:CT_SupplementalFont/a:font` |
| links | — | `a:CT_Hyperlink/a:{hlinkClick,hlinkMouseOver,hlinkHover}` |
| transitions, timing | `p:CT_SlideTransition/p:transition`, `p:CT_TransitionSoundAction/p:sndAc`, `p:CT_SlideTiming/p:timing`, `p:CT_BuildList/p:bldLst`, `p:CT_TLBuildParagraph/p:bldP`, `p:CT_TLCommonTimeNodeData/p:cTn`, `p:CT_TLShapeTargetElement/p:spTgt`, `p:CT_TLTextTargetElement/p:txEl`, `p:CT_IndexRange/p:{pRg,charRg}` | — |
| comments | `p:CT_CommentList/p:cmLst`, `p:CT_Comment/p:cm`, `a:CT_Point2D/p:pos`, `xsd:string/p:text`, `p:CT_CommentAuthorList/p:cmAuthorLst`, `p:CT_CommentAuthor/p:cmAuthor`, `p188:CT_CommentList/p188:cmLst`, `p188:CT_Comment/p188:cm`, `a:CT_Point2D/p188:pos`, `a:CT_TextBody/p188:txBody`, `p188:CT_CommentReplyList/p188:replyLst`, `p188:CT_CommentReply/p188:reply`, `p188:CT_AuthorList/p188:authorLst`, `p188:CT_Author/p188:author` | — |
| sections | `p:CT_PresentationExtensionList/p:extLst`, `p:CT_PresentationExtension/p:ext`, `p14:CT_SectionList/p14:sectionLst`, `p14:CT_Section/p14:section`, `p14:CT_SectionSlideIdList/p14:sldIdLst`, `p14:CT_SectionSlideIdListEntry/p14:sldId` | — |

Read **raw** (QName + attribute strings, as Phase 0's `refs.rs` does), not
allowlisted: `p:cNvSpPr@txBox` for `ShapeRef::is_text_box` (one boolean;
review r2-m4), transition kinds (40 SDK children of `p:transition` plus
`p15:prstTrans`; `p159:morph` is not in the SDK data), timing containers
(`p:par`, `p:seq`, `p:childTnLst`, …), comment anchor monikers (`ac:*`,
`pc:*`), colour transforms, the fills other than `a:solidFill`/`a:noFill`
(`a:gradFill`, `a:blipFill`, `a:pattFill`, `a:grpFill`: only their presence
and name matter, `Fill::Other`), and the `ahyp:hlinkClr` extension (§1.6).
Each would add entries for one string.

Checked (`allowlist_p1.txt`): all 124 names exist and are new; the 97
parent → child steps the resolver walks are SDK children of allowlisted
parents, so every one is a generated `ChildOf` (incl. `p:ext →
p14:sectionLst` and `p:cNvPr → a:hlinkHover`, which is the shape's name for
the mouse-over link). A trial `cargo xtask codegen` with the list merged
(`codegen-report.diff`, rerun for revision 2): 124 new `Read` lines and **no** new diagnostic (57,
unchanged; no unsupported content model); `ooxml-schema` builds with
`pml`, `dml` and both, `clippy -D warnings` is clean, and `ooxml-pptx`'s
Phase 0 tests pass.

**Overrides: `DmlPercent` and `set_type`.** The roadmap (§3.2) deferred the
percentage type to "when its element enters the allowlist"; Phase 1 is that
point, because it **reads** percentage values (`baseline`, line spacing,
bullet size, autofit) and allowlists elements with percentage defaults.
`ooxml-core` gains `SimpleType::DmlPercent` and the value type
`DmlPercent::{Thousandths(i32), Literal { percent: f64 }}` (ADR-0002 §1.2's
union type): reads accept the integer form and the `"-?\d+(\.\d+)?%"` form;
the default-join accepts `"NN%"` defaults for it. Phase 1 writes no
percentage. `codegen/overrides.toml` gets one `set_type { type = "DmlPercent" }`
per attribute below (evidence: this section, `percent-633.txt`,
`allowlist_p1.txt`; the overrides on a concrete name follow ADR-0002 §1.3,
the one on `a:CT_TextCharacterProperties/` is inherited by `a:rPr`,
`a:endParaRPr`, `a:defRPr`):

| `on` | Attributes | XSD type (default) | Effect |
|---|---|---|---|
| `a:CT_TextCharacterProperties/` | `baseline` | `ST_Percentage` | typed read of a resolved run property |
| `a:CT_TextSpacingPercent/a:spcPct` | `val` | `ST_TextSpacingPercentOrPercentString` | line/paragraph spacing |
| `a:CT_TextBulletSizePercent/a:buSzPct` | `val` | `ST_TextBulletSizePercent` (string-only pattern in the XSD; the SDK and every file use integers) | bullet size |
| `a:CT_TextNormalAutofit/a:normAutofit` | `fontScale`, `lnSpcReduction` | (`100%`, `0%`) | defaults adopted |
| `a:CT_HslColor/a:hslClr` | `sat`, `lum` | `ST_Percentage` | colour spec |
| `a:CT_ScRgbColor/a:scrgbClr` | `r`, `g`, `b` | `ST_Percentage` | colour spec |
| `p:CT_Presentation/p:presentation` | `serverZoom` | (`50%`) | resolves the Phase 0 `DefaultMismatch` left open by ADR-0005 §5 |

11 overrides; three defaults move from `DefaultMismatch` to adopted (39 →
36 in `codegen/report.txt`). Revision 1 also listed `p:cTn` `spd`/`accel`/
`decel`; Phase 1 never reads them (§3.5's `Effect` has no such field), so,
following ADR-0002 §1.3 ("adopted only via an override with evidence" when
needed), they stay dropped with their `DefaultMismatch` lines (review m12).
`serverZoom` is not read either; it is kept because it settles the Phase 0
item ADR-0005 §5 left open, on the same evidence. Real files: 0 values in the `"NN%"` form among 633
files (`percent-633.txt`; its 2 hits are `p:tag@val`, a plain string), so
the union only matters for conformant files from other producers — which
is why it is a type, not an assumption.

### 7. Test oracles

Fixtures: the 23 files of `reference/fixtures/pptx/` plus new **hand-built**
decks `reference/fixtures/pptx/synthetic/*.pptx`, each generated by a
committed script (`crates/ooxml-pptx/tests/fixtures/build_phase1.py`,
standard library only) from `python-pptx/minimal.pptx` with one documented
XML edit per rule, so every expected value is set at exactly one chain
level. The expected tables have two kinds of entries, kept apart (review
m10c): **mechanical** ones (which element holds the value — derived by the
build script from the edit it made) and **semantic** ones (which step wins
when two are set — each tied to a manual check M1–M7 or to a POI/LibreOffice
difference table below), so an author's guess cannot hide as a fact.

- **O1 geometry vs python-pptx** (independent reader): `ShapeRef::geometry`
  of every slide placeholder in every fixture equals python-pptx's
  `left/top/width/height`, except a committed table of cases where §1.2
  deliberately differs, each with its hand-checked expected value and the
  reason. Preview: 1,266 of 1,272 equal; 5 that python-pptx leaves `None`
  (no layout placeholder with the same idx, e.g. `idx="4294967295"`) and
  §1.2 resolves through the type or the master; 1 idx-0 tie (`tdf143129.pptx`:
  python-pptx gives the subtitle the title's position) — roadmap corpus:
  710 of 711, 1 orphan (`ppy_geometry-*.txt`). This checks the `xfrm`
  lookup where python-pptx and §1.2 agree on the match; the contested
  matches (ties, absent idx) rest on the hand-checked table and M2 (review
  m10b).
- **O2 text properties, hand-built:** synthetic decks S1 one per chain step (run,
  paragraph default, shape list style, `fontRef`, layout and master
  placeholder, each `txStyles` kind, `defaultTextStyle`), S2 levels 0–8 and
  a missing level, S3 idx ties / no idx / family mismatch / orphan /
  `4294967295`, S4 dt/ftr/sldNum category, S5 non-placeholder conflict
  (`defaultTextStyle` vs `otherStyle` vs `bodyStyle` all different), S6
  `clrMapOvr` on slide and on layout, S7 `+mj`/`+mn` incl. an empty `ea`,
  S8 notes (`notesStyle`, body and non-body), S9 invalid values
  (`AttrError` stops the search; invalid `lvl`), and (revision 2) S10 a
  layout `lvl1pPr/defRPr cap="all"` under a slide `<a:rPr lang="…"/>` (an
  XSD default must not stop the chain, M1), S11 a placeholder with its own
  `p:style/a:fontRef` and one inheriting from a layout placeholder with
  `p:style` (M2), S12 autofit set on the slide, inherited, absent, and a slide `<a:normAutofit/>` under a layout `fontScale="90000"` (100 %, `defaulted`; r2-m1) (M4),
  S13 linked runs with and without `ahyp:hlinkClr` (M4), S14 a slide
  `a:custGeom` under a layout `prstGeom`, and a placeholder in a group (m5). Each has a committed JSON table `(slide, shape, paragraph, run,
  property) → (value, source)`. Plus the roadmap's two real fixtures,
  `ph-inherit-props.pptx` and `placeholder-layout-color.pptx`, with
  hand-checked tables. Optional, not gating: render S1–S9 with LibreOffice
  (`soffice --convert-to pdf`) and compare sizes/fonts/colours where
  LibreOffice's rules agree with §1.3 (they differ on S4/S5 by design).
- **O2b text properties vs POI (optional, review m10a).** POI
  (`47dcff769bb3`) resolves text properties too
  (`CharacterPropertyFetcher.fetchProperty`: run → `pPr/defRPr` →
  `fetchShapeProperty` (shape `lstStyle`; for placeholders layout, then
  master) → `defaultTextStyle` for **non-placeholders only** →
  `getDefaultMasterStyle`), so it is an independent reader for `sz`, `b`,
  latin typeface, fill and `algn` on every fixture paragraph. Its known
  differences are committed as a table and the job fails only on a
  difference outside it: level fall-back in `txStyles`; matching without
  `@idx`; no `fontRef` step; `notesStyle` for every notes type; the master
  placeholder chosen by the slide's type (§1.2); **no step 8 for
  placeholders** (the corpus values listed in §1.3); steps 2–5 skipped on
  masters (§1.7). It runs in a separate, non-gating Java CI
  job, because the sandbox has no JVM.
- **O3 links vs python-pptx:** every shape `click_action` (action, address,
  target slide id) and run `hyperlink.address` equals ours, excluding runs
  where python-pptx raises (empty `r:id`), as in the preview.
- **O4 notes:** L1 (zero-edit save byte-identical, after reading every
  notes part), T1/T1b on notes parts, T2 locality sweep on every notes
  body (same edit set as ADR-0005 §7), T3 expected model read back by
  python-pptx (`notes_text_frame`) and applied independently on its lxml
  tree (extending `check.py`), T4 `NoNotesSlide`, `NoNotesBody` and every
  Phase 0 refusal raised on a notes body.
- **O5 find/replace:** in `cargo test`, invariants T and F (§4) on every
  paragraph of every fixture for a probe set of 1–3-word phrases, plus
  deletion and replacements containing `\v` (review M5), and
  `LinkSplitByBreak` on `shapes.pptx`/`with_japanese.pptx` linked runs and
  on a synthetic run that has only `a:hlinkMouseOver` (review r2-m3); T2 locality (only touched runs change); refusals byte-identical.
  Independently in the `pptx-readback` job: `check.py` performs the same
  edits on python-pptx's lxml tree from the rules of §4 and compares the
  whole-deck model; python-pptx's paragraph text equals Python's
  `str.replace` on the input text.
- **O6 sections, comments, transitions, timing:** expected tables on real
  fixtures (`bug65551.pptx` 10 sections, `slide-sections.pptx` 2,
  `45545_Comment.pptx` legacy comments, `modern-comments.pptx` 1 comment
  with 2 replies, `airplane-transition.pptx` `p15:prstTrans`,
  `animation.pptx`, `bug68703.pptx`, `picture-transparency.pptx`) and
  synthetic ones for what the corpus lacks (legacy reply via
  `p15:parentCm`, unknown author, text-anchored modern comment, `p159`
  Choice, unknown action, dangling `r:id`, section with unknown id).
- **Losslessness:** reading everything (`deck()`, all resolvers, notes,
  comments, timing) on every fixture leaves the package byte-identical
  (ADR-0001 Layer 1).
- **Manual (owner, current Microsoft 365 PowerPoint),** an
  `examples/manual_check_phase1.rs` writing the synthetic decks and a
  `CHECKLIST.md`: **M1** S4/S5/S11 — which size, typeface and colour
  PowerPoint shows for (i) a text box (`txBox="1"`), (ii) a shape and
  (iii) a placeholder with `p:style`, and for dt/ftr/sldNum, when
  `defaultTextStyle`, `otherStyle`, `bodyStyle` and `fontRef` disagree,
  and whether a layout placeholder's `p:style` reaches the slide; **M2** the idx-0 tie, implicit-idx, and explicit-idx-missing-from-the-layout (review r2-m8: 49 slide placeholders have an `@idx` no layout placeholder has, mostly `4294967295` — title 23, obj 10, body 7, sldNum 5, ctrTitle/ftr/pic 1 each; check a `4294967295` title with and without a layout title)
  decks — which layout placeholder PowerPoint links (seen by "Reset
  slide"); **M3** a layout transition with a slide that has none; **M4** a
  notes edit with a formatted multi-line body opens without repair; **M5**
  a `txStyles` level gap, a `defPPr` with a size, and a paragraph whose
  size and colour are set only in its own `a:pPr/a:defRPr` (review m3);
  **M6** a slide with `a:masterClrMapping` whose layout overrides the map
  (which colours `tx1`/`bg1` show, review m2); **M7** a body with no autofit
  element anywhere in its chain vs one inheriting `normAutofit` (does text
  shrink?), and linked runs with and without `ahyp:hlinkClr val="tx"`
  (review M4), including links whose action is not a URL
  (`hlinksldjump`, `macro`, `hlinkshowjump`, `hlinkfile`; review r2-m10).

### 8. Consequences

- Positive: every inherited value says where it came from; a wrong rule is
  visible in the output and in the oracle tables instead of silently
  producing a plausible font size.
- Positive: Phase 1 adds no Layer 3 writes: the only edited parts are
  slides (find/replace) and existing notes slides, with Phase 0/0.1 rules.
- Positive: `DmlPercent` lands with the first reads that need it, and the
  Phase 0 `serverZoom` mismatch is closed.
- Negative: the non-placeholder chain (Q1) and the dt/ftr/sldNum category
  follow POI pending the owner's PowerPoint check; LibreOffice renders up to
  154 + 52 corpus values differently.
- Negative: values are as specified, not as displayed (§1.6): colours are
  returned with their transforms unapplied, autofit scales and the
  hyperlink colour rule are inputs for the caller, and East Asian theme
  fonts are mostly `Unresolved(ScriptDependent)`.
- Negative: 124 more generated read views (compile time of `pml`/`dml`
  grows; measured in the trial build, not a blocker).

## Alternatives considered

1. **python-pptx's matching (first idx).** Wrong for the 69 idx-0 ties
   (subtitles and bodies would get the title's geometry and style).
2. **POI's matching (idx only when explicit, then type).** Finds no layout
   placeholder for 56 slide placeholders that §1.2 matches (no `@idx`, and
   no or a different `@type`); equal to §1.2 on the roadmap corpus.
3. **LibreOffice's non-placeholder chain (`defaultTextStyle` only).**
   Leaves 154 run sizes unresolved where the master has an answer; kept as
   the documented alternative for Q1.
4. **The ECMA note (`bodyStyle` for text boxes, `otherStyle` for other
   shapes).** Changes 415 of 417 contested text-box sizes and 305 of 564
   shape sizes; the text-box half contradicts how PowerPoint shows a new
   text box (to be confirmed by M1).
5. **Return only the effective value, no provenance.** Rejected: the
   reference readers disagree with each other (Context table) and only POI
   resolves the text chain at all, so provenance is the way to audit the
   rules.
6. **Merge proofing-only runs before replacing.** Rejected (§4): same
   visible result, worse locality.
7. **Allowlist every transition kind and timing node.** Rejected (§6): ~60
   entries to read one QName and a few strings.
8. **Apply colour transforms now.** Deferred: needs the §20.1.2.3 HSL
   formulas and their own oracle; reading the transforms loses nothing.

## Open questions for the project owner

**Owner decision 2026-11-16:** the proposed defaults of **Q1–Q5** below are
accepted, Q1 with the POI corrections of review r2-m2 (POI uses the slide
placeholder's type for the master placeholder and never applies
`defaultTextStyle` to placeholders; §1.2, §1.3). The manual checks M1–M7
remain the way to revisit a default if PowerPoint behaves differently.
**Q6:** the owner has been asked for a real modern-comment fixture; until
it arrives, O6 uses synthetic inputs.

Each item records its default (the round-1 review's recommendation):

- **Q1** (ADR-0005 Q6) Non-placeholder text. *Default:* `defaultTextStyle`
  then `otherStyle` (POI) for text boxes and shapes alike, with `Source`
  and `is_text_box()` exposed; manual check M1 on a text box, a shape and a
  placeholder with `p:style` decides. Alternatives: `defaultTextStyle` only
  (LibreOffice), or the ECMA note (`bodyStyle` for text boxes, `otherStyle`
  for shapes).
- **Q2** dt/ftr/sldNum placeholders. *Default:* `otherStyle` (POI); 52
  corpus values differ from LibreOffice; M1 decides.
- **Q3** `a:defPPr`, `txStyles` level gaps, paragraph `a:defRPr`.
  *Default:* the first two not consulted, the third consulted (§21.1.2.3.2,
  POI); M5 decides all three.
- **Q4** East Asian / complex-script fonts. *Default:* an empty theme
  typeface is `Unresolved(ScriptDependent)` with the theme's script fonts;
  a separate, documented helper maps `lang`/`altLang` to a script and picks
  a candidate, never reported as `Set` (§1.4).
- **Q5** Transitions. *Default:* layouts/masters reported separately (M3);
  MCE `Requires` compared by URI, with `p159` understood so morph is
  reported (§3.4).
- **Q6** Modern comments. *Default:* yes — the owner creates one modern
  comment anchored to text in PowerPoint as a real fixture; synthetic
  fixtures alone cannot show how `ac:txMk@cp`/`@len` count (UTF-16 units?
  paragraph separators?). Until then O6 uses synthetic inputs.

## Sources

- Evidence: `spikes/pptx-phase1/` (`chain.py`, `ppy_geometry.py`,
  `features.py`, `ppy_links.py`, `findreplace.py`, `allowlist_p1.py`,
  `allowlist_phase1.toml`, `results/`); `spikes/pptx/percent.py`.
- ECMA-376 Part 1, 5th edition (2016): §19.2.1.8 `defaultTextStyle`;
  §19.3.1.5 `bodyStyle`; §19.3.1.6 `clrMap`; §19.3.1.7 `clrMapOvr`;
  §19.3.1.26–27 `notes`, `notesMaster`; §19.3.1.35 `otherStyle` (and its
  note on text boxes); §19.3.1.36 `ph`; §19.3.1.49 `titleStyle`; §19.3.1.50
  `transition` (`advClick` default); §19.3.1.52 `txStyles` (`+mj-lt`
  example); §19.4.1–6 legacy comments; §20.1.2.2.8 `cNvPr`; §20.1.2.2.23
  `hlinkHover`; §20.1.4.1.17 `fontRef`; §20.1.4.1.24–25 `majorFont`,
  `minorFont`; §21.1.2.2.2 `defPPr`; §21.1.2.2.7 `pPr`; §21.1.2.3.2
  `defRPr`; §21.1.2.3.5–6 `hlinkClick`, `hlinkMouseOver`; §21.1.2.4.12–13
  `lstStyle`, `lvl1pPr`. ECMA-376 Part 3 (Markup Compatibility) for
  `mc:Choice`/`mc:Fallback` selection.
- python-pptx 1.0.2: `shapes/placeholder.py` (`_InheritsDimensions`,
  `_BaseSlidePlaceholder._base_placeholder`,
  `LayoutPlaceholder._base_placeholder`,
  `NotesSlidePlaceholder._base_placeholder`), `shapes/shapetree.py`
  (`LayoutPlaceholders.get`, `MasterPlaceholders.get`), `action.py`
  (`ActionSetting.action`, `target_slide`), `text/text.py`
  (`_Hyperlink.address`).
- Apache POI XSLF (`apache/poi` `47dcff769bb3`, the copy the spike read): `XSLFSheet.getPlaceholder`,
  `initPlaceholders`, `mapSchemeColor`; `XSLFShape.fetchShapeProperty`,
  `getPlaceholderType`; `XSLFTextParagraph.getDefaultMasterStyle`;
  `XSLFTextRun` (theme fonts); `model/ParagraphPropertyFetcher`,
  `model/CharacterPropertyFetcher`; `XSLFSlide`/`XSLFSlideLayout`/`XSLFNotes`
  `mapSchemeColor`.
- LibreOffice `oox/source/ppt/pptshape.cxx` at `2e885d0bda9b`
  (`getSubTypeTextListStyle`, `findPlaceholder`, `findPlaceholderByIndex`,
  the non-placeholder branch around l. 510).
- Open XML SDK data `431ab05c` (`p:CT_PresentationExtension/p:ext`
  children, `p:CT_SlideTransition/p:transition` children and `p14:dur`,
  `p188:CT_Comment` attributes and anchors, `a:CT_NonVisualDrawingProps`
  children).
