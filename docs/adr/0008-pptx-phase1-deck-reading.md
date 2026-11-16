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

- Status: **Proposed**
- Date: 2026-11-16
- Depends on: [ADR-0001](0001-lossless-round-trip-architecture.md),
  [ADR-0002](0002-schema-driven-code-generation.md) (with its post-acceptance
  notes "wildcard parents / XML characters", "parser depth limit" and
  "detach/insert", core PR #18),
  [ADR-0005](0005-pptx-phase0.md) (revision 3 Accepted; revision 4 §9,
  "Phase 0.1 — subtree copy of format sources", PR #19, Proposed)
- Background and evidence: [`docs/roadmap/pptx.md`](../roadmap/pptx.md) §3.1,
  §3.3, §3.4 and Phase 1; [`spikes/pptx-phase1/`](../../spikes/pptx-phase1/README.md)
  (scripts and committed outputs in `results/`)

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

| Question | python-pptx 1.0.2 | Apache POI XSLF (trunk) | LibreOffice oox (`2e885d0bda9b`) | ECMA-376 Part 1 |
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
**effective type** `@type` or `obj`. Its **family** is `title` for
`title`/`ctrTitle`; `dt`, `ftr`, `sldNum`, `hdr` for themselves; `body` for
everything else (`body`, `obj`, `subTitle`, `chart`, `tbl`, `clipArt`,
`dgm`, `media`, `pic`, `sldImg`) — python-pptx's and POI's master mapping.

**Slide (or notes slide) → layout (notes master)**, over every placeholder
of the layout's shape tree in document order, group members included:

1. Candidates with the same effective idx. Among them: exact type
   (`MatchKind::Idx`), else same family (`IdxFamily`). If several share the
   idx, the tie is broken the same way (`IdxTieType`, `IdxTieFamily`).
2. If there are idx candidates but none of the slide's family: when the
   slide's `@idx` is **explicit**, the first candidate is used and reported
   as `IdxTypeMismatch` (an explicit idx is PowerPoint's own link); when the
   idx is only the default 0, go to step 3.
3. Exact type over all layout placeholders, idx 0 preferred (`Type`), then
   same family (`Family`).
4. Otherwise **orphan**: the slide placeholder resolves from the master
   directly by family (`Orphan`), as POI does; python-pptx returns `None`.

Notes slides match the notes master by step 3 only (python-pptx
`NotesSlidePlaceholder._base_placeholder`; LibreOffice also requires equal
type on notes pages).

**Layout → master**: the first master placeholder of the same family.
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
| implicit idx 0 of another family → `Type`/`Family`/`Orphan` | 6 | e.g. `subTitle` without `@idx` vs a layout whose only idx-0 placeholder is the title |
| `Type` / `Family` | 6 / 2 | incl. `sldNum` with `idx="4294967295"` (5): python-pptx finds nothing |
| `Orphan` | 61 | |
| different layout placeholder than POI / python-pptx would pick | 56 / 67 | all in the tie and implicit-idx rows above; 0 / 2 in the roadmap corpus |

Every resolved value from a layout or master placeholder reports the
`MatchKind`, so `IdxFamily` (a `body` slide placeholder inheriting from an
`obj` layout placeholder) is visible.

#### 1.3 Property chains

The **level** of a paragraph is `a:pPr@lvl` (default 0, 0–8); "list style
at level L" is the list style's `a:lvl{L+1}pPr` (§21.1.2.4.13). The level
element is used **only at that exact level**: POI falls back to lower
levels in `txStyles` (`getDefaultMasterStyle`), which mattered once in 633
files (`txStyles.level_missing_lower_present`); Phase 1 does not, and the
manual check covers it. `a:defPPr` is **not** consulted (POI and
LibreOffice do not; 24 non-trivial `defPPr` in the corpus, Q3).

Run properties of an `a:r` or `a:fld` (and of `a:endParaRPr` for the
paragraph-end format) are looked up in this order; the **first element that
sets the property decides**, including when its value is invalid (returned
as `AttrError` with the raw string, ADR-0002 §1.2 "never lossy"):

| # | Placeholder text | Non-placeholder text (`p:sp` text box or shape) | `Source` |
|---|---|---|---|
| 1 | `a:rPr` | `a:rPr` | `Direct` |
| 2 | `a:pPr/a:defRPr` of the paragraph | same | `ParagraphDefault` |
| 3 | shape `a:lstStyle`, level L, `a:defRPr` | same | `ShapeListStyle` |
| 4 | — | shape `p:style/a:fontRef` (font: `major`/`minor` → `+mj-*`/`+mn-*`; colour: its colour child) | `ShapeStyle` |
| 5 | layout placeholder `a:lstStyle`, level L | presentation `p:defaultTextStyle`, level L | `Placeholder{Layout}` / `PresentationDefault` |
| 6 | master placeholder `a:lstStyle`, level L | master `p:txStyles/p:otherStyle`, level L (Q1) | `Placeholder{Master}` / `MasterTextStyle{Other}` |
| 7 | master `p:txStyles`: `titleStyle` (family title), `otherStyle` (dt, ftr, sldNum, hdr), `bodyStyle` (body) | — | `MasterTextStyle` |
| 8 | presentation `p:defaultTextStyle`, level L | — | `PresentationDefault` |

For **notes** placeholders, steps 5–7 become the notes-master
placeholder's `a:lstStyle`, then `p:notesMaster/p:notesStyle` (POI's `notesStyle` fallback, LibreOffice
`getNotesTextStyle`); 281 of 395 notes runs take `sz` from `notesStyle`.
**Paragraph properties** use the same chain without steps 2 and 4, reading
the attributes and children of `a:pPr` / the level element itself.
**Body properties** (`a:bodyPr` attributes and the autofit group) use slide
→ layout placeholder → master placeholder (`anchor`: 121 / 139 / 618, 326
unset). **Geometry** (`a:xfrm`/`p:xfrm`: `off`+`ext` with `rot`, `flipH`,
`flipV`, as one unit; an `a:xfrm` lacking `off` or `ext` supplies nothing —
0 in the corpus) and `a:prstGeom` use the same three steps. Table-cell text
stops after step 3: table-style text (`tableStyles.xml` `a:tcTxStyle`) is a
Phase 3 topic and is reported as `Unresolved::TableStyle`, never guessed.

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
- Step 4 follows §20.1.4.1.17 (`fontRef`: "which themed font to use along
  with a choice of color") and is what makes text in an inserted autoshape
  white: 880 of 4,001 non-placeholder text shapes have a `fontRef`; it
  decides 381 fills and 51 typefaces. Its position before
  `defaultTextStyle` is part of manual check M1.
- Steps 5–6 for non-placeholder text (Q6 of ADR-0005). The contested tail
  is reached for `sz` by 981 runs and for the latin typeface by 1,322. With
  POI's order the result differs from LibreOffice's (`defaultTextStyle`
  only) in **154** runs (`otherStyle` supplies a size that
  `defaultTextStyle` lacks, e.g. 18 pt in `alterman_security.pptx`); the
  reverse order (`otherStyle` first) would change 564 typefaces and 91
  sizes; the ECMA note's `bodyStyle` would change 976 of 981 sizes (body
  text is typically 32 pt with bullets, which is not how PowerPoint shows a
  new text box). Decision: POI's order, `otherStyle` reported as its own
  `Source`, and the owner's PowerPoint check M1 on a hand-built deck in
  which the three readings give three different results (Q1).
- dt/ftr/sldNum use `otherStyle` (POI). LibreOffice skips `txStyles` for
  them; the difference shows in 52 values (51 typefaces, 1 size) and is in
  manual check M1.

#### 1.4 Theme references

- **Fonts.** A typeface `+mj-lt`, `+mj-ea`, `+mj-cs`, `+mn-lt`, `+mn-ea`,
  `+mn-cs` is resolved to the theme's `a:fontScheme/a:majorFont|a:minorFont`
  `a:latin|a:ea|a:cs@typeface` (§19.3.1.52 example, §20.1.4.1.24–25; POI
  `XSLFTextRun` l. 807–832). Of 4,025 theme references among resolved
  latin typefaces in the corpus, 4,021 resolve; 4 (`bug64693.pptx`, `a:latin typeface="+mn-ea"`) land on
  an empty theme `a:ea typeface=""` and are returned as such, not replaced.
  Empty `ea`/`cs` typefaces are normal in themes (all six themes of that
  file): the script-specific `a:font script="Jpan"` entries apply instead,
  which needs the run's language → script mapping, i.e. application
  behaviour. Phase 1 lists them (`Theme::script_fonts`) and does not apply
  them (Q4).
- **Colours.** `a:schemeClr@val` names a theme slot through the effective
  colour map of the **slide being read** (an inherited colour is evaluated
  in the slide's context): the slide's `p:clrMapOvr/a:overrideClrMapping`,
  else the layout's, else the master's `p:clrMap` (§19.3.1.6–7; POI
  `XSLFSlide`/`XSLFSlideLayout`/`XSLFSheet.mapSchemeColor`). The map is
  applied to every name it has (`bg1`, `tx1`, `bg2`, `tx2`, `accent1–6`,
  `hlink`, `folHlink`); `dk1`… are theme slots already; `phClr` is returned
  unresolved. Corpus: 5,567 scheme colours, all resolve (4,870 through
  `bg/tx` mapping); 3 slides override the map themselves and 10 more
  inherit a layout's override.
  `a:srgbClr`, `a:sysClr` (`@lastClr`), `a:prstClr`, `a:hslClr`,
  `a:scrgbClr` are returned as written. **Colour transforms** (`a:lumMod`,
  `a:tint`, `a:alpha`, …) are returned as a list of
  `(name, DmlPercent | raw)` and **not applied**; computing the final RGB
  (§20.1.2.3) is Phase 3 with formatting writes.

#### 1.5 Not resolved in Phase 1

Table-style text and cell fills, theme style matrices (`a:fillRef`,
`a:lnRef`, `a:effectRef` → shape fill/line), backgrounds, SmartArt and
chart text formatting, and slide-space rectangles of shapes inside rotated
or flipped groups (`slide_rect()` composes `a:chOff`/`a:chExt` scaling and
returns `Err(TransformedGroup)` if any enclosing group has `rot`, `flipH`
or `flipV`).

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
`Slide::links()` (every link with its shape path / paragraph / item).

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
the Fallback (ECMA-376 Part 3). Understood = the namespaces whose
transition elements are in the vendored SDK data: `p14`, `p15`. So
`Requires="p159"` (morph, 1 slide) yields the Fallback, as an Office 2013
reader would. `Slide::transition_branches()` returns every branch.
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
    pub click_index: Option<usize>,      // ordinal of the enclosing clickPar in the main sequence
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
   table: `\v` splits it — `a:br`, then a new run whose `a:rPr` is the
   Phase 0.1 copy of the start run's `a:rPr` without links; `\n`, `\r` →
   `LineBreakInParagraph`; other non-XML characters → `InvalidXmlChar`.
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
| (subset) across runs with different `a:hlinkClick` | 0.2 % | 0.2 % |

The spike's reference model of rules 1–4 was applied to 42,933 / 55,611
pattern–paragraph pairs (and as many deletions; pairs with a refused match
are skipped by the model), and both invariants held every time: **T** the
new paragraph text equals `str.replace` on the old one; **F** every kept
character keeps its old `a:rPr` and every inserted character has the start
run's `a:rPr`.

### 5. API sketch over the ADR-0002 views

Hand-written domain types wrap generated views, as in Phase 0. Views of
different elements with one SDK complex type (`a:rPr`, `a:endParaRPr`,
`a:defRPr`; `a:pPr`, `a:defPPr`, `a:lvl1pPr`…`a:lvl9pPr`; `a:lstStyle`,
`p:titleStyle`, `p:bodyStyle`, `p:otherStyle`, `p:notesStyle`,
`p:defaultTextStyle`) are unified by small enums in `ooxml-pptx`
(`CharPropsRef`, `ParaPropsRef`, `ListStyleRef`) with a delegating
`macro_rules!`; no generator change.

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
    pub fn placeholder_match(&self) -> Result<Option<PlaceholderMatch<'a>>>;   // layout + master + MatchKind
    pub fn geometry(&self) -> Result<Option<Resolved<'a, Xfrm>>>;
    pub fn preset_geometry(&self) -> Result<Option<Resolved<'a, &'a str>>>;
    pub fn body_props(&self) -> Result<BodyProps<'a>>;                         // each field Option<Resolved<…>>
    pub fn action(&self, ev: LinkEvent) -> Option<Link<'a>>;
    pub fn slide_rect(&self) -> Result<Option<Rect>>;                           // §1.5
}
impl<'a> ParagraphRef<'a> { pub fn props(&self) -> Result<ParaProps<'a>>; pub fn end_props(&self) -> Result<RunProps<'a>>; }
impl<'a> RunRef<'a> { pub fn props(&self) -> Result<RunProps<'a>>; pub fn link(&self, ev: LinkEvent) -> Option<Link<'a>>; }

pub struct Resolved<'a, T> { pub value: Result<T, AttrError>, pub source: Source<'a> }
pub enum Source<'a> {
    Direct,                                     // on the element asked about
    ParagraphDefault, ShapeListStyle { level: u8 }, ShapeStyle,
    Placeholder { role: Role /* Layout | Master | NotesMaster */, part: &'a PartName, shape_id: u32, matched: MatchKind, level: Option<u8> },
    MasterTextStyle { part: &'a PartName, style: TextStyleKind /* Title | Body | Other | Notes */, level: u8 },
    PresentationDefault { level: u8 },
}
pub struct RunProps<'a> {                       // every field: Option<Resolved<'a, …>>; None = no part sets it
    pub size: Option<Resolved<'a, u32>>,         // sz, 1/100 pt
    pub bold, italic, underline, strike, caps, baseline /* DmlPercent */, spacing, kern, lang, …
    pub latin: Option<Resolved<'a, Font<'a>>>, pub east_asian, pub complex_script, pub symbol,
    pub fill: Option<Resolved<'a, Fill<'a>>>,
}
pub struct Font<'a> { pub typeface: &'a str, pub theme: Option<ThemeFontRef<'a>> /* major/minor, script, theme part */ }
pub enum Fill<'a> { None, Solid(Color<'a>), Other(&'a str) /* gradFill, blipFill, pattFill, grpFill: element name */ }
pub struct Color<'a> { pub spec: ColorSpec<'a>, pub mapped: Option<SchemeMapping<'a>>, pub base_rgb: Option<[u8; 3]>, pub transforms: Vec<ColorTransform<'a>> }
```

`None` means no element in the chain sets the property: the value is the
application's default, which Phase 1 never invents. Every `Resolved` keeps
borrowing the parsed parts, so the API is zero-copy; the `Deck` borrow is
shared, and `notes_mut`/`replace_all` take `&mut Presentation` (a `Deck`
must be dropped first, as Phase 0's `Slide` before `slide_mut`).

New errors: `BrokenChain`, `NoNotesSlide`, `NoNotesBody`, `InvalidPattern`,
`FieldInMatch`, `BreakInMatch`, `MixedFormatInMatch`, `TransformedGroup`;
reused: everything in ADR-0005 §6.

### 6. Allowlist and overrides

**Allowlist.** 118 new `[read]` entries, no new `[write]` entry (Phase 1
writes text only, through Phase 0's write entries and core's
`insert_detached`). The exact list is
[`spikes/pptx-phase1/allowlist_phase1.toml`](../../spikes/pptx-phase1/allowlist_phase1.toml);
`p14:` and `p188:` entries go in `pml.toml` (the feature group is the file,
ADR-0002 post-acceptance note).

| Group | `pml.toml` (48) | `dml.toml` (70) |
|---|---|---|
| parts | `p:CT_SlideLayout/p:sldLayout`, `p:CT_SlideMaster/p:sldMaster`, `p:CT_NotesSlide/p:notes`, `p:CT_NotesMaster/p:notesMaster` | `a:CT_OfficeStyleSheet/a:theme` |
| text styles | `p:CT_SlideMasterTextStyles/p:txStyles`, `a:CT_TextListStyle/p:{titleStyle,bodyStyle,otherStyle,notesStyle,defaultTextStyle}` | `a:CT_TextListStyle/a:lstStyle`, `a:CT_TextParagraphProperties/a:{defPPr,lvl1pPr…lvl9pPr}`, `a:CT_TextCharacterProperties/a:defRPr` |
| paragraph/run/body properties | — | `a:CT_TextSpacing/a:{lnSpc,spcBef,spcAft}`, `a:CT_TextSpacingPercent/a:spcPct`, `a:CT_TextSpacingPoint/a:spcPts`, `a:CT_TextNoBullet/a:buNone`, `a:CT_TextAutonumberBullet/a:buAutoNum`, `a:CT_TextCharBullet/a:buChar`, `a:CT_TextBlipBullet/a:buBlip`, `a:CT_TextBulletSizePercent/a:buSzPct`, `a:CT_TextFont/a:{buFont,latin,ea,cs}`, `a:CT_Color/a:buClr`, `a:CT_TextBodyProperties/a:bodyPr`, `a:CT_TextNormalAutofit/a:normAutofit`, `a:CT_TextNoAutofit/a:noAutofit`, `a:CT_TextShapeAutofit/a:spAutoFit` |
| colour | `a:CT_ColorMapping/p:clrMap`, `a:CT_ColorMappingOverride/p:clrMapOvr` | `a:CT_ColorMapping/a:overrideClrMapping`, `a:CT_EmptyElement/a:masterClrMapping`, `a:CT_SolidColorFillProperties/a:solidFill`, `a:CT_NoFillProperties/a:noFill`, `a:CT_{SRgbColor/a:srgbClr, SchemeColor/a:schemeClr, SystemColor/a:sysClr, ScRgbColor/a:scrgbClr, PresetColor/a:prstClr, HslColor/a:hslClr}` |
| geometry, shape style | `a:CT_ShapeProperties/p:spPr`, `a:CT_GroupShapeProperties/p:grpSpPr`, `a:CT_Transform2D/p:xfrm`, `a:CT_ShapeStyle/p:style`, `p:CT_SlideSize/p:sldSz` | `a:CT_Transform2D/a:xfrm`, `a:CT_GroupTransform2D/a:xfrm`, `a:CT_Point2D/a:{off,chOff}`, `a:CT_PositiveSize2D/a:{ext,chExt}`, `a:CT_PresetGeometry2D/a:prstGeom`, `a:CT_FontReference/a:fontRef` |
| theme | — | `a:CT_BaseStyles/a:themeElements`, `a:CT_ColorScheme/a:clrScheme`, `a:CT_Color2/a:{dk1,lt1,dk2,lt2,accent1…accent6,hlink,folHlink}`, `a:CT_FontScheme/a:fontScheme`, `a:CT_FontCollection/a:{majorFont,minorFont}`, `a:CT_SupplementalFont/a:font` |
| links | — | `a:CT_Hyperlink/a:{hlinkClick,hlinkMouseOver,hlinkHover}` |
| transitions, timing | `p:CT_SlideTransition/p:transition`, `p:CT_TransitionSoundAction/p:sndAc`, `p:CT_SlideTiming/p:timing`, `p:CT_BuildList/p:bldLst`, `p:CT_TLBuildParagraph/p:bldP`, `p:CT_TLCommonTimeNodeData/p:cTn`, `p:CT_TLShapeTargetElement/p:spTgt`, `p:CT_TLTextTargetElement/p:txEl`, `p:CT_IndexRange/p:{pRg,charRg}` | — |
| comments | `p:CT_CommentList/p:cmLst`, `p:CT_Comment/p:cm`, `a:CT_Point2D/p:pos`, `xsd:string/p:text`, `p:CT_CommentAuthorList/p:cmAuthorLst`, `p:CT_CommentAuthor/p:cmAuthor`, `p188:CT_CommentList/p188:cmLst`, `p188:CT_Comment/p188:cm`, `a:CT_Point2D/p188:pos`, `a:CT_TextBody/p188:txBody`, `p188:CT_CommentReplyList/p188:replyLst`, `p188:CT_CommentReply/p188:reply`, `p188:CT_AuthorList/p188:authorLst`, `p188:CT_Author/p188:author` | — |
| sections | `p:CT_PresentationExtensionList/p:extLst`, `p:CT_PresentationExtension/p:ext`, `p14:CT_SectionList/p14:sectionLst`, `p14:CT_Section/p14:section`, `p14:CT_SectionSlideIdList/p14:sldIdLst`, `p14:CT_SectionSlideIdListEntry/p14:sldId` | — |

Read **raw** (QName + attribute strings, as Phase 0's `refs.rs` does), not
allowlisted: transition kinds (40 SDK children of `p:transition` plus
`p15:prstTrans`; `p159:morph` is not in the SDK data), timing containers
(`p:par`, `p:seq`, `p:childTnLst`, …), comment anchor monikers (`ac:*`,
`pc:*`), colour transforms. Each would add many entries for one string.

Checked (`allowlist_p1.txt`): all 118 names exist and are new; the 91
parent → child steps the resolver walks are SDK children of allowlisted
parents, so every one is a generated `ChildOf` (incl. `p:ext →
p14:sectionLst` and `p:cNvPr → a:hlinkHover`, which is the shape's name for
the mouse-over link). A trial `cargo xtask codegen` with the list merged
(`codegen-report.diff`): 118 new `Read` lines and **no** new diagnostic (57,
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
| `p:CT_TLCommonTimeNodeData/p:cTn` | `spd`, `accel`, `decel` | (`100%`, `0%`, `0%`) | defaults adopted |
| `p:CT_Presentation/p:presentation` | `serverZoom` | (`50%`) | resolves the Phase 0 `DefaultMismatch` left open by ADR-0005 §5 |

Six defaults move from `DefaultMismatch` to adopted (39 → 33 in
`codegen/report.txt`). Real files: 0 values in the `"NN%"` form among 633
files (`percent-633.txt`; its 2 hits are `p:tag@val`, a plain string), so
the union only matters for conformant files from other producers — which
is why it is a type, not an assumption.

### 7. Test oracles

Fixtures: the 23 files of `reference/fixtures/pptx/` plus new **hand-built**
decks `reference/fixtures/pptx/synthetic/*.pptx`, each generated by a
committed script (`crates/ooxml-pptx/tests/fixtures/build_phase1.py`,
standard library only) from `python-pptx/minimal.pptx` with one documented
XML edit per rule, so every expected value is set at exactly one chain
level and the expected `source` is known by construction.

- **O1 geometry vs python-pptx** (independent reader): `ShapeRef::geometry`
  of every slide placeholder in every fixture equals python-pptx's
  `left/top/width/height`, except a committed table of cases where §1.2
  deliberately differs, each with its hand-checked expected value and the
  reason. Preview: 1,266 of 1,272 equal; 5 that python-pptx leaves `None`
  (no layout placeholder with the same idx, e.g. `idx="4294967295"`) and
  §1.2 resolves through the type or the master; 1 idx-0 tie (`tdf143129.pptx`:
  python-pptx gives the subtitle the title's position) — roadmap corpus:
  710 of 711, 1 orphan (`ppy_geometry-*.txt`).
- **O2 text properties, hand-built** (no reference library resolves the
  text chain independently): synthetic decks S1 one per chain step (run,
  paragraph default, shape list style, `fontRef`, layout and master
  placeholder, each `txStyles` kind, `defaultTextStyle`), S2 levels 0–8 and
  a missing level, S3 idx ties / no idx / family mismatch / orphan /
  `4294967295`, S4 dt/ftr/sldNum category, S5 non-placeholder conflict
  (`defaultTextStyle` vs `otherStyle` vs `bodyStyle` all different), S6
  `clrMapOvr` on slide and on layout, S7 `+mj`/`+mn` incl. an empty `ea`,
  S8 notes (`notesStyle`), S9 invalid values (`AttrError` stops the
  search). Each has a committed JSON table `(slide, shape, paragraph, run,
  property) → (value, source)`. Plus the roadmap's two real fixtures,
  `ph-inherit-props.pptx` and `placeholder-layout-color.pptx`, with
  hand-checked tables. Optional, not gating: render S1–S9 with LibreOffice
  (`soffice --convert-to pdf`) and compare sizes/fonts/colours where
  LibreOffice's rules agree with §1.3 (they differ on S4/S5 by design).
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
  deletion; T2 locality (only touched runs change); refusals byte-identical.
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
  `CHECKLIST.md`: **M1** S4/S5 — which size/typeface PowerPoint shows for
  a text box and for dt/ftr/sldNum when `defaultTextStyle`, `otherStyle`,
  `bodyStyle` and `fontRef` disagree; **M2** the idx-0 tie and implicit-idx
  decks — which layout placeholder PowerPoint links (seen by "Reset
  slide"); **M3** a layout transition with a slide that has none; **M4** a
  notes edit with a formatted multi-line body opens without repair; **M5**
  a `txStyles` level gap and a `defPPr` with a size.

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
- Negative: colours are returned with their transforms unapplied; a caller
  that needs the final RGB waits for Phase 3.
- Negative: 118 more generated read views (compile time of `pml`/`dml`
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
4. **The ECMA note (`bodyStyle` for text boxes).** Changes 976 of 981
   contested sizes, against observed PowerPoint behaviour (to be confirmed
   by M1).
5. **Return only the effective value, no provenance.** Rejected: no
   oracle resolves the text chain independently, so provenance is the only
   way to audit the rules.
6. **Merge proofing-only runs before replacing.** Rejected (§4): same
   visible result, worse locality.
7. **Allowlist every transition kind and timing node.** Rejected (§6): ~60
   entries to read one QName and a few strings.
8. **Apply colour transforms now.** Deferred: needs the §20.1.2.3 HSL
   formulas and their own oracle; reading the transforms loses nothing.

## Open questions for the project owner

- **Q1** (ADR-0005 Q6) Non-placeholder text: `defaultTextStyle` then
  `otherStyle` (POI; proposed), `defaultTextStyle` only (LibreOffice), or
  `bodyStyle` (ECMA note)? Proposed answer by manual check M1; the code
  keeps the rule in one place.
- **Q2** dt/ftr/sldNum placeholders: `otherStyle` (POI; proposed) or no
  `txStyles` (LibreOffice)? Also M1.
- **Q3** `a:defPPr` and `txStyles` level gaps: not consulted (proposed);
  M5 decides whether PowerPoint uses them.
- **Q4** East Asian fonts: is listing the theme's script fonts enough for
  Phase 1, or should `+mn-ea` be resolved per run language (needs a
  language → script table)?
- **Q5** Transitions on layouts/masters (M3): report separately (proposed)
  or inherit into `Slide::transition`?
- **Q6** Modern comments: still no real text-anchored comment (ADR-0005
  Q5); synthetic fixtures are proposed for O6. May one be created in
  PowerPoint?

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
- Apache POI XSLF (trunk, 2026-11-16): `XSLFSheet.getPlaceholder`,
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
