# ADR-0016: PowerPoint Phase 3 — formatting writes and links

- Status: **Proposed** (revision 1)
- Date: 2026-11-16
- Depends on: [ADR-0001](0001-lossless-round-trip-architecture.md) (Layer 3
  append-only registries), [ADR-0002](0002-schema-driven-code-generation.md)
  (generated typed setters for `[write]` entries; post-acceptance notes
  "detach/insert", "parser depth limit"),
  [ADR-0005](0005-pptx-phase0.md) (Phase 0/0.1 text edits, locks, format
  sources), ADR-0008 (PR #28, Proposed: inheritance chain with
  provenance, `Resolved`/`Source`, read APIs for links and notes),
  ADR-0011 §1 (PR #31, Proposed: the core part-adding transaction
  `Package::plan()` → `PartPlan` → `commit`, branch `core/add-part` to
  come), ADR-0012 (PR #30, Proposed: id and part-name rules, embedded
  template), ADR-0013 (PR #33, Proposed: table structure and cell
  properties).
- Evidence: [`spikes/pptx-phase3/`](../../spikes/pptx-phase3/README.md)
  (`formatting.py`; 619 distinct presentations, 1,269 slides; output in
  `results/`); python-pptx 1.0.2, Apache POI `47dcff76` and LibreOffice
  `2e885d0b` source, cited in §12.

## Context

Phases 0–1 write text and read resolved formatting with provenance
(ADR-0008: every effective value carries the part and element it comes
from). Phase 3 is the first phase that **writes formatting**: run and
paragraph properties, shape properties, hyperlinks, and three package
additions that earlier ADRs deferred here — notes slides (ADR-0008 §2
refuses `NoNotesSlide`), table style definitions (ADR-0013 §3 refuses
`TableStyleNotDefined`) and minimal theme edits.

Corpus (`formatting.py`, 619 distinct presentations, 1,269 slides; all
numbers from `results/formatting-619.txt`):

| Topic | Finding | Consequence |
|---|---|---|
| run properties | `a:rPr` attributes `sz` 10,134, `lang` 7,461, `spc` 5,043, `b` 2,192, `i` 657, `u` 545 (`none` 859 of all `u` values, `sng` 137), `strike` 531, `baseline` 281; children `latin` 8,199, `cs` 6,894, `solidFill` 2,807, `ea` 661, `effectLst` 530, `ln` 167 | setters for these; colour = the fill child of `a:rPr` |
| run colours | `srgbClr` 2,454, `schemeClr` 2,126, `prstClr` 362, `sysClr` 20; transforms `lumMod` 184, `lumOff` 69, `alpha` 32, `tint` 14, `satMod` 13 | scheme colours and transforms are first-class; a colour write replaces the whole colour element |
| fonts | explicit typefaces 18,802; theme references `+mn-lt` 405, `+mn-cs` 369, `+mn-ea` 321, `+mj-lt` 70, `+mj-ea` 53, `+mj-cs` 35 | `FontRef::Theme(Major/Minor, Latin/Ea/Cs)` |
| paragraphs | `algn` 2,082 (`ctr` 1,635, `l` 343, `r` 55, `just` 49), `marL` 1,476, `indent` 1,234, `lvl` 864; spacing `lnSpc` in percent 1,226 / points 94, `spcBef` percent 931 / points 772, `spcAft` points 755 / percent 212; bullets `buFont` 940, `buChar` 713, `buNone` 605, `buAutoNum` 206 (`arabicPeriod` 183), `buBlip` 30 | both spacing units; char/auto-number/none bullets; picture bullets need a relationship (refused, §4) |
| autofit | slide bodies: none 5,395, `spAutoFit` 658, `noAutofit` 265, `normAutofit` 247 (77 with `fontScale`/`lnSpcReduction`) | §5 |
| shape properties | `p:spPr` fill: `solidFill` 2,629, inherited 2,201, `noFill` 1,030, `blipFill` 212, `gradFill` 26; `a:ln` 3,445; `prstGeom` `rect` 2,582, `ellipse` 165, `roundRect` 118, … with adjustment values 205; `custGeom` 1,626; rotated 125, flipped 98; **825 `p:spPr` without `a:xfrm`** (placeholder position inherited) | §6 |
| groups | 1,552 groups with a transform: **216 with a child space that differs from their own** (scaled or offset), 23 rotated, 59 flipped | positions are written in the parent's coordinate space; group extents are never recomputed implicitly (§6) |
| hyperlinks | `a:hlinkClick` 177 (on `p:cNvPr` 94, on `a:rPr` 83), `hlinkMouseOver` 1; actions: none 69, `ppaction://hlinksldjump` 60, `ppaction://media` 14, `macro` 13, `hlinkshowjump?jump=…` 12, `noaction` 9, `hlinkfile` 1; relationship types `hyperlink` 70, `slide` 60; 48 links with `r:id=""`; external schemes `http` 44, `https` 15, `mailto` 6; `ahyp:hlinkClr val="tx"` 12; `highlightClick` 25, `tooltip` 1 | §7 |
| notes | 371 notes slides; **156 of 619 files have a notes master; 460 files have slides but no notes master** | creating a notes slide usually needs a notes master (§8) |
| table styles | `tableStyles.xml` has 0 definitions in 447 files, 1–5 in the rest; parts used `wholeTbl` 318, `firstRow`/`firstCol` 289, `lastRow` 283, `band1H` 257, `lastCol` 253, `band1V` 252, `band2H` 234, `band2V` 197, `tblBg` 51, corner cells ≤ 21 | §9 |
| themes | 12 colour slots: `srgbClr` 10,064, `sysClr` 1,504; `clrScheme` "Office" 640 …; theme fonts `Calibri`/`Calibri Light` most; **no theme part shared by two masters**; masters with the standard `bg1=lt1, tx1=dk1, bg2=lt2, tx2=dk2` map 688, non-standard 26; 3 slides override the map | §10 |

Reference implementations (§12) write formatting directly at the run,
paragraph or shape, as this ADR does, but differ in ways that matter for a
lossless editor: python-pptx's `recalculate_extents` resets a group's
child space to its own (`chOff = off`, `chExt = ext`), which moves the
other members of the 216 scaled groups; POI's `linkToExternal` removes the
old hyperlink relationship unconditionally, python-pptx's `drop_rel` only
when the part references it fewer than twice; python-pptx creates a
default notes master by relationship but never writes
`p:notesMasterIdLst`; neither edits themes or table style definitions.

## Decision

### 1. Scope and principles

**In Phase 3:** run and paragraph formatting writes (including character
ranges), body autofit settings, shape position/size/rotation/flip,
fill, line, preset geometry and adjustment values, hyperlink and action
writes on text and shapes, notes slide creation (and the notes master it
needs), table style definitions, theme colour and font edits.

**Not in Phase 3:** effects (`effectLst`, `effectDag`, 3-D), gradient
fills beyond two-or-more linear stops, picture/pattern fills, custom
geometry edits, text warps, picture bullets, font embedding, computing
autofit, chart/SmartArt formatting, master/layout formatting through the
slide API (layouts and masters are edited only through the explicit
theme/table-style calls).

**Principles.**

1. **Write the direct value, nowhere else.** A run setter writes the run's
   `a:rPr`; a paragraph setter its `a:pPr`; a shape setter its `p:spPr`
   (or `p:grpSpPr`). Inherited values (layout, master, theme, list
   styles) are never edited as a side effect. After a write, ADR-0008's
   resolver reports that property with `Source::Direct` (the element
   written), so provenance stays truthful.
2. **`clear_*` restores inheritance** by removing the direct attribute or
   child (and nothing else); a `Resolved` read afterwards shows the
   inherited source again.
3. **Typed setters from codegen.** The elements written move from
   `[read]` to `[write]` in `pml.toml`/`dml.toml` (`a:rPr`, `a:pPr`,
   `a:endParaRPr`, `a:defRPr`, `a:bodyPr`, `p:spPr`, `a:xfrm`, colour and
   fill elements, `a:ln`, `a:prstGeom`/`a:avLst`/`a:gd`, `a:hlinkClick`,
   `a:hlinkHover`, `a:hlinkMouseOver`, `a:tblStyle` parts, theme
   `a:clrScheme`/`a:fontScheme`); schema order of new children is the
   lens's slot placement (ADR-0002 §4).
4. **Locality.** A setter dirties only the element it changes; setting a
   value equal to the current one changes no byte (ADR-0005 rule).
5. **Locks.** Formatting does not move characters or paragraphs, so the
   ADR-0005 §6 text locks (character animations, comment anchors) do
   **not** apply to formatting writes; range formatting that **splits
   runs** keeps every character offset and paragraph index, so it is
   allowed under those locks too. The `mc:AlternateContent` rule applies.
6. **Atomicity.** Every setter and every package addition runs plan →
   validate → apply: all checks (values, schema placement, core
   `PartPlan` validation, depth) before the first change; a refusal leaves
   the package byte-identical.

### 2. Colours and fonts

```rust
pub enum Color {
    Rgb([u8; 3]),                      // a:srgbClr
    Scheme(SchemeColor),               // a:schemeClr: Tx1, Bg1, Tx2, Bg2, Accent1..6, Hlink, FolHlink, Dk1, Lt1, Dk2, Lt2, PhClr
    System(SystemColor, Option<[u8;3]>), // a:sysClr (@lastClr)
    Preset(PresetColor),               // a:prstClr
}
pub struct ColorSpec { pub color: Color, pub transforms: Vec<ColorTransform> } // lumMod, lumOff, tint, shade, alpha, satMod, …
pub enum FontRef { Typeface(String), Theme(ThemeFont, Script) } // "+mj-lt" etc.
```

- **clrMap awareness.** On slides, layouts and notes the mapped names
  `tx1`/`bg1`/`tx2`/`bg2` are what PowerPoint writes; they resolve through
  the slide's colour map (ADR-0008 §1.4). Writing `Dk1`/`Lt1`/… is
  allowed and bypasses the map (documented); `Color::scheme_for_text()`
  helpers produce the mapped names. `PhClr` only inside theme style
  matrices and table styles (`InvalidSchemeColor` elsewhere).
- **Transforms** are written as given, in the given order (they are
  order-sensitive, ECMA-376 §20.1.2.3); not computed.
- **A colour write replaces the colour element** of its context (the
  fill choice `a:solidFill`/`a:noFill`/`a:gradFill`/… of `a:rPr`, the
  `a:solidFill` of `a:ln`, …), keeping siblings.
- **Fonts** set `a:latin`/`a:ea`/`a:cs`/`a:sym@typeface`; replacing a
  typeface removes that element's `panose`, `pitchFamily` and `charset`
  (they described the old face; keeping them would mislead renderers).
  Theme references are written as the six `+mj-…`/`+mn-…` tokens.

### 3. Run formatting

`RunMut` setters (and `clear_*`): `size(pt ×100, 100..=400000,
ST_TextFontSize)`, `bold`, `italic`, `underline(ST_TextUnderlineType, 18
values incl. none)`, `strike(noStrike|sngStrike|dblStrike)`,
`baseline(percent)`, `caps(none|small|all)`, `spacing(pt ×100)`,
`kerning`, `lang`/`alt_lang`, `color(ColorSpec)` / `no_fill()`,
`highlight(ColorSpec)`, `font(FontRef, script)`.

- **Character ranges.** `ParagraphMut::format_range(start, end, f)`
  splits the runs at the range boundaries (the split-off parts copy the
  run's `a:rPr` through the Phase 0.1 copy primitive, so links are kept
  on both sides — the copy here does **not** skip `a:hlinkClick`, because
  it is the same run's text, and the relationship is shared by both
  halves, which is valid OPC) and applies `f` to every run inside.
  Ranges count characters as Phase 0 text does (`\v` for `a:br`, field
  text); a range that starts or ends inside a field is
  `RangeSplitsField`.
- **`a:endParaRPr`** gets the same setters (`ParagraphMut::end_format`):
  it decides the formatting of text typed at the end and the height of an
  empty line (ADR-0005 R2-5).

### 4. Paragraph formatting

`ParagraphMut` setters: `align(l|ctr|r|just|justLow|dist|thaiDist)`,
`level(0..=8)`, `margin_left`, `indent`, `rtl`, `font_align`,
`line_spacing` / `space_before` / `space_after` as `Spacing::Percent` or
`Spacing::Points` (both units are common), `bullet(Bullet)` with
`Bullet::None` (`a:buNone`), `Char { char, font: Option<FontRef>,
size: Option<BulletSize>, color: Option<ColorSpec> }`, `AutoNumber {
scheme, start_at }`, and `Bullet::Inherit` (removes every `bu*` child).
Picture bullets (`a:buBlip`, 30) need an image relationship:
`UnsupportedBullet` in Phase 3. Tab stops (`a:tabLst`) are written as a
whole list.

### 5. Autofit

`TextBodyMut::autofit(None | Normal | Shape)` writes the `a:bodyPr`
choice (`noAutofit`/`normAutofit`/`spAutoFit`). Phase 3 **does not
compute** fitted sizes (needs font metrics; python-pptx's `fit_text` reads
a font file). Consequences, documented on every setter that changes text
size or spacing:

- in a `normAutofit` body with `fontScale`/`lnSpcReduction` (77 bodies),
  the cached scale stays until PowerPoint refits (ADR-0005 Q4a);
  `TextBodyMut::reset_autofit_scale()` removes both attributes explicitly
  (the text then shows unscaled until refit);
- in `spAutoFit` bodies (658) the shape's `a:ext` is a cache of the text
  extent and is not recomputed.

### 6. Shape properties

- **Position and size** (EMU) are written to the shape's `a:xfrm` **in
  its parent's coordinate space** (the slide, or the enclosing group's
  child space `a:chOff`/`a:chExt`). `ShapeMut::set_rect(Rect)` takes that
  space; `ShapeMut::set_slide_rect(Rect)` converts slide coordinates
  through the enclosing groups' scaling and offsets (the inverse of
  ADR-0008's `slide_rect()`), refused with `TransformedGroup` when an
  enclosing group is rotated or flipped (23 + 59 groups).
- **Group extents are never recomputed implicitly.** python-pptx's
  `recalculate_extents` sets a group's `chOff`/`chExt` equal to its new
  `off`/`ext`; on the 216 corpus groups with a scaled or offset child
  space this visibly moves every other member. A member may therefore lie
  outside its group's box after a move (PowerPoint recomputes the box on
  the next interaction); `GroupMut::fit_to_children()` is the explicit
  call, which changes `off`/`ext` **and** `chOff`/`chExt` together so that
  members keep their slide positions.
- **Placeholders without `a:xfrm`** (825 `p:spPr`) inherit their position;
  writing a position creates `a:xfrm` with both `a:off` and `a:ext`
  (filled from the resolved inherited values, ADR-0008) and ends the
  inheritance — documented, and `clear_rect()` restores it.
- **Rotation** (`rot`, 60000ths of a degree), `flipH`/`flipV`.
- **Fill:** `ColorSpec` solid, `NoFill`, `Linear { angle, stops: Vec<(pos,
  ColorSpec)> }` (≥ 2 stops), `Inherit` (remove the fill choice).
  Picture and pattern fills: `UnsupportedFill`.
- **Line:** width (EMU), `ColorSpec` or `NoFill`, preset dash, cap,
  compound type; arrow heads (`headEnd`/`tailEnd`) type/width/length;
  `Inherit`.
- **Geometry:** `set_preset(ShapeType)` (the 187 presets of ECMA-376's
  `presetShapeDefinitions.xml`), `set_adjustment(name, value)` validated
  against that preset's `a:avLst` guide names (123 presets have
  adjustments; 205 corpus shapes set some); written as `a:gd name="adj1"
  fmla="val N"`. The definitions file ships in the ECMA-376 Part 1
  download (`OfficeOpenXML-DrawingMLGeometries.zip`); Phase 3 vendors it
  next to the XSDs under the same licence terms (ADR-0002 §1) and
  generates the name table with codegen. `custGeom` (1,626) is preserved,
  not edited (`UnsupportedGeometry`).

### 7. Hyperlinks and actions

Locations (ADR-0008 §3.1): runs and fields (`a:rPr/a:hlinkClick`,
`a:hlinkMouseOver`) and shapes (`p:cNvPr/a:hlinkClick`, `a:hlinkHover`).

```rust
pub enum LinkTarget {
    Url(String),                 // external relationship `…/hyperlink`, TargetMode External
    Slide(SlideRef),             // relationship `…/slide` to the target slide part + action ppaction://hlinksldjump
    ShowJump(Jump),              // r:id="" + action ppaction://hlinkshowjump?jump=nextslide|previousslide|firstslide|lastslide|lastslideviewed|endshow
    NoAction,                    // r:id="" + action ppaction://noaction
}
pub struct LinkSpec { pub target: LinkTarget, pub tooltip: Option<String>, pub highlight_click: bool, pub text_color: bool }
```

- **Relationships** are added through the core transaction (ADR-0011
  §1: `PartPlan::add_relationship`, `rId{max+1}`, no gap reuse; ADR-0012
  §3) from the part that holds the link (slide, layout, master or notes
  slide). A new link always gets its own relationship (no implicit
  sharing, ADR-0011 §1.2), even to an existing target.
- **Url** must be an absolute IRI or a relative reference with no
  control characters or spaces (encoded as `%20`); schemes are not
  restricted (PowerPoint stores `file:`, `mailto:`, custom schemes), but
  `javascript:` is `UnsafeLinkScheme`. The relationship `Target` is
  written escaped (ADR-0011 §1.2 target rules).
- **Slide** targets must be a slide of the same presentation;
  `SlideRef` is a `sldId` (stable across moves, ADR-0012 §3).
- **Removing or replacing** a link removes the element; its relationship
  is removed **only when no attribute of the part still carries that
  `Id`** (any attribute, any namespace — the PR #14 / ADR-0012 C4 rule; a
  run split by §3 shares a relationship between two runs). POI's
  `linkToExternal` removes it unconditionally; python-pptx's `drop_rel`
  counts only `r:id` attributes.
- **Text colour of a text link** (`text_color: true`) writes the
  Office 2019 extension `a:extLst/a:ext[@uri="{A12FA001-AC4F-418D-AE19-62706E023703}"]/ahyp:hlinkClr@val="tx"`
  (12 in the corpus): the run keeps its own colour instead of the theme's
  `hlink`. The `ahyp:` namespace is declared on the created `a:ext`
  (ADR-0011 §1.3 `declare_namespace` on created nodes). Without it,
  readers colour link text with `hlink` (ADR-0008 resolves which).
- `ppaction://media`, `macro`, `hlinkfile`, `ole`, `program` actions are
  preserved on read and not created (`UnsupportedAction`).

### 8. Notes slides

`Presentation::notes_mut_or_create(slide)` creates a notes slide when the
slide has none (ADR-0008 §2 refuses `NoNotesSlide`; ADR-0012 §4.6 refused
when there is no notes master — this ADR supersedes that refusal, and
ADR-0012's revision 2 is to point here):

1. **Notes master.** If the presentation has none (460 of 616 files with
   slides): add the notes master and its theme from the embedded template
   of ADR-0012 §8 (parts `notesMaster{N}.xml`, `theme{N}.xml`, their
   relationships, `Override`s), a presentation relationship, and
   `p:notesMasterIdLst/p:notesMasterId@r:id` in `p:presentation` (schema
   position after `p:sldMasterIdLst`). python-pptx creates the part and
   relationship but never writes `p:notesMasterIdLst`; PowerPoint
   requires the list to find the master (manual check Q2).
2. **Notes slide.** Part `notesSlide{N}.xml` (ADR-0012 C5 naming),
   relationships notes → notes master and notes → slide, slide → notes
   (C3), `Override`; shape tree with the notes master's `sldImg`, `body`
   and `sldNum` placeholders cloned as in ADR-0012 §4.1 (python-pptx's
   set), ids from 2.
3. The returned `NotesMut` is ADR-0008's API.

All parts and registry entries go into one `PartPlan`; the tree edits of
`presentation.xml` are checked with it before `commit`.

### 9. Table style definitions

- `Presentation::add_table_style(TableStyleDef) -> StyleId` appends an
  `a:tblStyle` to `ppt/tableStyles.xml` (created with its presentation
  relationship and `Override` when absent; its `@def` is then the new
  style's id). `TableStyleDef { name, parts }` with the parts the corpus
  uses — `wholeTbl`, `band1H`, `band2H`, `band1V`, `band2V`, `firstRow`,
  `lastRow`, `firstCol`, `lastCol` (corner cells and `tblBg` are
  preserved, not created) — each with `tcTxStyle` (bold, italic, font
  reference, colour) and `tcStyle` (fill `ColorSpec`/no fill, the six
  borders).
- **Style ids** are GUIDs. Phase 3 generates a deterministic one (UUID
  version 8 from SHA-256 of the style name and the existing ids in the
  file, so a rerun produces the same file) that differs from every id in
  `tableStyles.xml` and from the 74 built-in ids PowerPoint knows (the
  list LibreOffice ships in `predefined-table-styles.cxx`).
- **Built-in styles by id.** ADR-0013's `set_style` accepts, with an
  explicit `TableStyle::BuiltIn(guid)`, a GUID from that 74-entry list
  without adding a definition (8 corpus tables do this; PowerPoint and
  LibreOffice render them, POI does not); other undefined ids stay
  `TableStyleNotDefined`.
- Existing definitions are never modified (append-only, ADR-0001 Layer 3:
  other tables may use them).

### 10. Theme edits (minimal)

`ThemeMut` (from a master: `Presentation::master_theme_mut(master)`):
`set_color(slot: Dk1|Lt1|Dk2|Lt2|Accent1..6|Hlink|FolHlink, Rgb | System)`
and `set_major_font` / `set_minor_font` (latin, ea, cs typefaces; script
fonts `a:font@script` are kept — 27,129 entries in the corpus). The
change applies to every slide that uses the master (that is what a theme
is); ADR-0008 reads show it with `Source::Theme`. A theme part referenced
by more than one master or by a master and a notes master is edited only
with `allow_shared: true` (`SharedTheme`; 0 shared between masters in the
corpus). No theme part is created or renamed; style matrices
(`fmtScheme`) are not edited.

### 11. Errors

| Error | When |
|---|---|
| `ValueOutOfRange { attr }` | size, level, percentages, EMU outside the schema range |
| `InvalidSchemeColor` | `PhClr` outside a style matrix |
| `RangeSplitsField`, `RangeOutOfBounds` | §3 |
| `UnsupportedBullet`, `UnsupportedFill`, `UnsupportedGeometry`, `UnknownAdjustment { name }` | §4, §6 |
| `TransformedGroup` | `set_slide_rect` through a rotated/flipped group |
| `UnsafeLinkScheme`, `InvalidLinkTarget`, `UnsupportedAction` | §7 |
| `SlideNotFound` (by `sldId`) | link or notes target |
| `SharedTheme` | §10 |
| `TableStyleNotDefined` (ADR-0013) | an undefined, non-built-in id |
| Phase 0 refusals | `InsideAlternateContent`, `UnsupportedContent`, … |
| `Core(…)` | `PartPlan` validation, name/relationship conflicts, `TooDeep`, `NamespaceNotInScope` — all before the first change |

### 12. Reference implementations (source-verified)

| Rule | python-pptx 1.0.2 | POI `47dcff76` | ADR-0016 |
|---|---|---|---|
| font name | sets `a:latin@typeface` only (`text/text.py` `Font.name`) | `setFontFamily` picks the font group from the run's first character (`XSLFTextRun`, `FontGroup.getFontGroupFirst`) | explicit script (`latin`/`ea`/`cs`), drops `panose`/`pitchFamily`/`charset` |
| colour | `ColorFormat.rgb`/`theme_color`, `brightness` as `lumMod`/`lumOff` (`dml/color.py`) | `setFontColor`: solid paint only, applies the colour transform to an RGB (`XSLFTextRun`) | `ColorSpec` with scheme slots and ordered transforms, written as given |
| underline | enum incl. `none` | `setUnderlined(bool)`: `sng`/`none` only | all 18 values |
| group extents | `recalculate_extents` sets `chOff=off`, `chExt=ext` (`oxml/shapes/groupshape.py`) | `setAnchor` / `setInteriorAnchor` separately (`XSLFGroupShape`) | never implicit; explicit `fit_to_children` keeping member positions |
| external link | new external relationship; old one dropped if referenced < 2 times by `r:id` (`text/text.py` `_Hyperlink.address`, `opc/package.py` `drop_rel`) | `linkToExternal` removes the old relationship unconditionally; tooltip = URL (`XSLFHyperlink`) | remove only when no attribute references it |
| slide jump | `ActionSetting.target_slide`: relationship to the slide + `ppaction://hlinksldjump` (`action.py`) | `linkToSlide` (same); `linkToNextSlide` etc. → `ppaction://hlinkshowjump?jump=…` | same actions; slide by `sldId` |
| notes master | `NotesMasterPart.create_default` + presentation relationship, **no** `p:notesMasterIdLst` (`parts/presentation.py`, `parts/slide.py`) | `createNotesMaster` from its template (`XMLSlideShow`) | template part + relationship + `p:notesMasterId` |
| autofit | `auto_size`; `fit_text` computes a size from a font file | — | settings only; no computation |
| table styles, theme | none | read-only (`XSLFTableStyles`, `XSLFTheme`) | append definitions; minimal theme setters |
| built-in table style ids | — | — | LibreOffice's 74 ids (`oox/source/drawingml/table/predefined-table-styles.cxx`) |

### 13. Test oracles

- **T2 locality:** each setter changes only its element (byte diff
  inside it); package additions change only the registries (appended
  entries) and add new parts; every other part byte-identical.
- **Provenance oracle:** after each write, ADR-0008's resolver returns
  the written value with `Source::Direct` at the expected element; after
  `clear_*`, the same value and source as before the write (round trip
  on the corpus fixtures).
- **python-pptx read-back:** `font.size/bold/italic/underline`,
  `font.color.rgb`/`theme_color`/`brightness`, `font.name`, paragraph
  `alignment`/`line_spacing`/`space_before`/`space_after`/`level`, shape
  `left/top/width/height`/`rotation`, `fill`/`line` colour and width,
  `auto_shape_type` and `adjustments`, hyperlink `address` and
  `click_action.target_slide`, notes text — independent of the Rust
  model.
- **LibreOffice (pinned O3 workflow):** the edited deck converts to PDF;
  link URIs and slide jumps appear as PDF link annotations; notes appear
  in the notes export.
- **T4 refusals:** every row of §11, byte-identical.
- **Manual (owner):** PowerPoint opens a deck with each kind of write
  without repair; Q1–Q4.

## Consequences

- Positive: formatting can be written without touching inherited parts,
  and the provenance model of ADR-0008 reports exactly what was written.
- Positive: links, notes and table styles reuse the Phase 2 registry and
  transaction rules; no new allocation rule is introduced.
- Negative: no autofit computation: text may overflow until PowerPoint
  refits; `spAutoFit` shape sizes go stale.
- Negative: the codegen allowlist grows substantially (`[write]` for
  formatting, shape, link, table-style and theme elements) and a new
  vendored input (`presetShapeDefinitions.xml`) appears.
- Negative: `ahyp:hlinkClr` and the built-in table-style list encode
  Microsoft product behaviour that ECMA-376 does not specify.

## Alternatives considered

1. **Edit the inherited source** (e.g. change the layout placeholder's
   size when a slide's title size is set). Rejected: silent changes to
   every other slide using that layout; explicit theme/layout APIs only.
2. **python-pptx's implicit group-extent recalculation.** Rejected: moves
   members of scaled groups (216 in the corpus).
3. **Compute autofit** like python-pptx's `fit_text`. Rejected for Phase 3:
   font files and a text layout engine; PowerPoint refits anyway.
4. **Share relationships for equal link targets.** Rejected: ADR-0011
   "no implicit sharing"; removal would then need reference counting
   across links the caller did not touch.
5. **Copy PowerPoint's built-in table style definitions into
   `tableStyles.xml`.** Rejected: their content is Microsoft's and not in
   ECMA-376; referencing the id is what PowerPoint itself does.

## Open questions for the project owner

- **Q1** Part naming: ADR-0011 §1.2 picks the **smallest free** index for
  numbered names, ADR-0012 §2 C5 the **largest + 1**. One rule should be
  chosen in the core PR (`core/add-part`); this ADR works with either.
- **Q2** Does PowerPoint need `p:notesMasterIdLst` (python-pptx omits it)?
  Manual check on a python-pptx-created deck and ours.
- **Q3** `ahyp:hlinkClr` (Office 2019+): write it on request only (as
  proposed), or never?
- **Q4** Allowing built-in table-style GUIDs without definitions (§9):
  acceptable, given POI ignores them?

## Sources

- Spike: `spikes/pptx-phase3/formatting.py`, `results/formatting-619.txt`.
- python-pptx 1.0.2: `pptx/text/text.py` (`Font.name`, `_Hyperlink.address`,
  `_add_hlinkClick`, `_remove_hlinkClick`, `TextFrame.auto_size`, `fit_text`),
  `pptx/dml/color.py` (`brightness`), `pptx/action.py`
  (`ActionSetting.target_slide`, `_clear_click_action`),
  `pptx/opc/package.py` (`drop_rel`, `_rel_ref_count`),
  `pptx/oxml/shapes/groupshape.py` (`recalculate_extents`),
  `pptx/parts/presentation.py` (`notes_master_part`), `pptx/parts/slide.py`
  (`NotesMasterPart.create_default`).
- Apache POI `47dcff769bb395034dddfd59c4609659b6dbc6a4`,
  `poi-ooxml/src/main/java/org/apache/poi/xslf/usermodel/`:
  `XSLFTextRun.java` (`setFontColor`, `setFontFamily`, `setUnderlined`),
  `XSLFHyperlink.java` (`linkToExternal`, `linkToSlide`,
  `linkToRelativeSlide`), `XSLFGroupShape.java` (`setAnchor`,
  `setInteriorAnchor`), `XSLFTheme.java`, `XMLSlideShow.java`
  (`createNotesMaster`, `getNotesSlide`).
- LibreOffice `2e885d0bda9bc4ee9f3bde51905c4a7820b01f06`:
  `oox/source/drawingml/table/predefined-table-styles.cxx` (74 built-in ids).
- ECMA-376 Part 1, 5th edition: DrawingML text character and paragraph
  properties, colour transforms (order-sensitive), `presetShapeDefinitions.xml`
  (187 presets, 123 with adjustment guides) in
  `OfficeOpenXML-DrawingMLGeometries.zip`; vendored Transitional XSDs
  (`ST_TextFontSize` 100–400000, `ST_TextUnderlineType` 18 values).
