# ADR-0016: PowerPoint Phase 3 — formatting writes and links

- Status: **Accepted** (revision 5)
- Date: 2026-11-16
- Depends on: [ADR-0001](0001-lossless-round-trip-architecture.md) (Layer 3
  append-only registries), [ADR-0002](0002-schema-driven-code-generation.md)
  (generated typed setters for `[write]` entries; post-acceptance notes
  "detach/insert", "parser depth limit"),
  [ADR-0005](0005-pptx-phase0.md) (Phase 0/0.1 text edits, locks, format
  sources), [ADR-0008](0008-pptx-phase1-deck-reading.md) (Accepted rev 3:
  inheritance chain with provenance, `Resolved`/`Source`, read APIs for
  links and notes), the
  core registry transaction of PR #38 (branch `core/add-part`:
  `RegistryPlan`, `stage_part`, `Package::apply`; ADR-0001 note "registry
  writes"), [ADR-0012](0012-pptx-phase2-slides-and-shapes.md) (Accepted rev 3:
  transaction shape §2, id and part-name rules, embedded template,
  `ensure_notes_slide`), [ADR-0013](0013-pptx-phase1b-tables-and-smartart.md)
  (Accepted rev 4: table structure and cell properties, built-in table
  style ids), and the core PR #49 (`core/detach-same-part`, head
  `397bd86`: the same-part clone of §3; ADR-0002 post-acceptance note
  "same-part copy").
- Evidence: [`spikes/pptx-phase3/`](../../spikes/pptx-phase3/README.md)
  (`formatting.py`; 619 distinct presentations, 1,269 slides; output in
  `results/`); python-pptx 1.0.2, Apache POI `47dcff76` and LibreOffice
  `2e885d0b` source, cited in §12.
- Review: [round 1](0016-opus-review-round1.md) (not converged: M1, M2,
  m1–m14), [round 2](0016-opus-review-round2.md) (not converged: N1;
  N2–N7), [round 3](0016-opus-review-round3.md) (converged: Accepted
  after Minor only)

## Revision history

- **Revision 1:** initial draft.
- **Revision 2** (after `0016-opus-review-round1.md`):

  | Finding | Change | Where |
  |---|---|---|
  | M1 stale transaction model (`PartPlan`, ADR-0012 C3–C5) | every write is ADR-0012 §2's transaction: clones edited through the lens, one `RegistryPlan` (`stage_part` + additions/removals), one `apply`; Q1 removed (core: max + 1) | §1, §7–§10, header |
  | M2 run split cannot use Phase 0.1's `detach_as` (refuses `r:` ids, `a:extLst`) | a **same-part copy mode** is a core requirement (follow-up core PR to #18's API); formatting validated before splitting (a `RunFormat` value, not a closure); no split when nothing changes; `smtId` refused | §3, Consequences |
  | m1 notes master detection | by the presentation → `notesMaster` relationship; list added when missing; refused when notes slides exist without a master | §8 |
  | m2 4:3 decks | the template's notes master has a 4:3 variant; other ratios fit `sldImg` into the box | §8 |
  | m3, m4 `app.xml` `Notes`, single notes API | `ensure_notes_slide` (ADR-0012) extended; `Notes` counted | §8 |
  | m5 graphicFrame, connectors | `p:graphicFrame/p:xfrm` position only; connector routes not recomputed | §6, Consequences |
  | m6 group box | manual check; identity `xfrm` for groups without one | §6, §13 |
  | m7 geometry | `custGeom` refused; old `avLst` removed; `hf`/`vf` documented | §6 |
  | m8 definitions provenance | Part 1 download, own directory, SHA-256, `codegen --check`, only a name/default table generated | §6 |
  | m9 table styles | `@def` default `{5C22544A-…}`, uppercase GUIDs, "Layer 3" claim dropped | §9 |
  | m10 `hlinkMkLst` | link removal/replacement follows ADR-0005's lock; new slide jumps block deleting their target | §7, Consequences |
  | m11 bullet semantics | per group (`buClr`, `buSz*`, `buFont`): `None` = remove; picture bullet relationships | §4 |
  | m12 underline line/fill | `a:uLn`/`a:uLnTx`/`a:uFill`/`a:uFillTx` | §3 |
  | m13 schemes | `vbscript:` and `data:` refused too | §7 |
  | m14 `themeOverride` | stated | §10 |
  | Q1–Q4 | the review's recommendations recorded | Open questions |

- **Revision 3** (after `0016-opus-review-round2.md`, not converged: one
  Major, N1; N2–N7 Minor):

  | Finding | Change | Where |
  |---|---|---|
  | N1 `ahyp:hlinkClr` declares its own namespace (14/14), which #18's detach refuses | the same-part copy is a **span-preserving clone** checked like #44's move (free prefixes, `mc:Ignorable`, inherited `xml:space` mean the same at the destination); core PR `core/detach-same-part` | §3, §11, Q3 |
  | N2 whole `a:extLst` kept | only `a:ext` URIs on an allowlist (`{A12FA001-…}`, `ahyp:hlinkClr`); others refuse | §3 |
  | N3 which refusals remain | listed: unique attributes (`smtId` ≠ 0, `a:effectDag`/`a:cont@name`) and MC stay; relationship ids, allowlisted extensions, declarations are lifted | §3 |
  | N4 pre-checks before the first split | both boundary runs copyable, `RangeSplitsField`, values; "already has the value" compares the run's own `a:rPr`; set + clear of one item refused; T4 case | §3, §13 |
  | N5 notes master vs `notesSz` | placeholders scaled to the deck's `p:notesSz`, `sldImg` fitted to `p:sldSz`; the 4:3 variant is dropped | §8 |
  | N6 provenance details | outer zip hash, full hashes in `PROVENANCE.md`, edition dates, `THIRD-PARTY-NOTICES`, root element `presetShapeDefinitons` | §6 |
  | N7 `tableStyles.xml` detection, link replacement order | by relationship, `Exact` name, `NameInUse` → `Core`; add → edit → stage → remove | §7, §9 |
  | nits | ADR-0012 linked; `ahyp` declared on `ahyp:hlinkClr` itself; 12 (slides) / 14 (with layouts and masters) | header, §7 |

- **Revision 4** (alignment with the core PR #49 `core/detach-same-part`,
  head `397bd86`; no change of rule). #49's review round 1 (P49-m1) was
  made on `276879f`, a prefix-re-choosing variant of #18's detach that
  kept declarations only inside allowed extensions; #49 has since been
  rewritten as the span-preserving clone this ADR's §3 describes
  (`cff3623`: declarations anywhere in the copy travel with its bytes), so
  §3 stands. Changes: §3 names #49's API
  (`Lens::detach_same_part_as`, `SamePartOptions::keep_extension`,
  `lens::ext_uri::HYPERLINK_COLOR`, `LensMut::insert_same_part_by`, the
  check twin `SamePartDetached::check_insert_under`, `WrongDocument`), and
  records P49-m2: inside a transaction the copy and its check are taken
  from the **staged clone**, not from the package's parsed tree.

- **Revision 5** (after `0016-opus-review-round3.md`: converged,
  Accepted after Minor only). Status: Accepted.

  | Finding | Change | Where |
  |---|---|---|
  | R3-m1 `Source::Theme`, override "as source" | no such `Source`: after a theme edit the referring element's `Source` is unchanged and the new value shows in the resolved `Font.theme`/`Color`; parts with `themeOverride` stay `Unresolved(ThemeOverride)` (ADR-0008 §1.4) | §10 |
  | R3-m2 `TableStyle::BuiltIn` | dropped: ADR-0013 §3's `set_style(Some(id))` already accepts the 74 built-in ids (case-insensitive); Phase 3 only keeps generated ids clear of them; Q4 closed | §9, Q4 |
  | R3-m3 header | ADR-0008/0013 Accepted and linked; #49 (`397bd86`, ADR-0002 note "same-part copy") in Depends on and Sources; origin/main merged | header, Sources |
  | R3-m4 notes master row | no 4:3 variant | §12 |
  | nits | copies taken from the document being edited; MC attribute values dropped from the checks (MC is refused); §11 mapping of #49's refusals; `remove_relationship` types; corpus populations; `RunFormat` items as `Set(v)`/`Clear` | §3, §4, §7, §8, §9, §11 |

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
   allowed under those locks too (`p:txEl` uses `p:charRg`/`p:pRg`
   offsets, `oac:txMk` uses `cp`/`len`, and `oac:context@hash` hashes
   plain text only — MS-ODRAWXML `CT_TextCharRangeContext`; no reference
   names a run). Link removal and replacement are **not** formatting for
   this rule (§7). The `mc:AlternateContent` rule applies.
6. **Atomicity.** Every write is one transaction in ADR-0012 §2's shape:
   (1) clone the parsed trees that change and edit the clones through the
   lens (new parts built as `XmlDocument`s); (2) run every refusal and
   record every step in one core `RegistryPlan` — `stage_part` for each
   edited clone, `add_part(Numbered)`/`add_relationship`/
   `remove_relationship` for registry changes; (3) `Package::apply`. A
   setter that touches only one part's tree and no registry is the
   degenerate case (validate, then edit in place). A refusal, ours or the
   core's, leaves the package byte-identical.

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
`highlight(ColorSpec)`, `font(FontRef, script)`, and underline
appearance `underline_line(LineSpec | FollowText)` /
`underline_fill(ColorSpec | FollowText)` — the `a:uLnTx`/`a:uLn` and
`a:uFillTx`/`a:uFill` choices (review m12; `a:uFillTx` occurs 93 times).
`underline(...)` changes only `@u`; `clear_underline()` removes `@u`
**and** the four underline appearance children, so no underline colour
of the old value survives to mix with an inherited one.

- **Character ranges.** `ParagraphMut::format_range(start, end, &RunFormat)`
  takes the formatting as a **value** (`RunFormat`: one optional field per
  setter above, each `Set(v)` or `Clear`, so an item cannot be both set
  and cleared), not a closure, so that it is
  validated completely (ranges, enums, colours) **before** any run is
  split; a `ValueOutOfRange` never leaves a split behind (review M2).
  Then it splits the runs at the range boundaries and writes the values
  to every run inside. A run whose properties already have every value
  of the `RunFormat` is not split (principle 4); adjacent runs are never
  merged (that would keep the characters but change unrelated bytes).
  Ranges count characters as Phase 0 text does (`\v` for `a:br`, field
  text); a range that starts or ends inside a field is
  `RangeSplitsField`.
- **Copying `a:rPr` into the split-off run** needs a **same-part copy**,
  a core requirement of this ADR (reviews M2, round 2 N1), implemented by
  the core PR #49 (`core/detach-same-part`: `Lens::detach_same_part_as`
  with `SamePartOptions::keep_extension(uri)`, inserted by
  `LensMut::insert_same_part_by`, check twin
  `SamePartDetached::check_insert_under`). It is **not** a variant of
  Phase 0.1's `Lens::detach_as` (PR #18), which drops prefixes and
  re-chooses them at the destination and therefore refuses relationship
  ids, `a:extLst` and namespace declarations. It is a **span-preserving
  clone**: the `a:rPr` node is cloned with its spans, so its bytes —
  prefixes and the declarations inside it included — are written
  unchanged, and the clone is checked exactly like PR #44's same-part
  move (`check_move_in_by`): every free prefix of the subtree (element
  and attribute names; MC is refused, so no MC attribute values occur,
  and QNames inside other attribute values are not checked — #49's
  documented limit) is bound to the same namespace at the
  destination, `mc:Ignorable` status and inherited `xml:space` are the
  same, and the depth limit holds. For a split the clone is a sibling in
  the same `a:p`, so these checks always pass; they guard other uses.
  This keeps the corpus case: all 14 `ahyp:hlinkClr` (12 on slides, 14
  with layouts and masters) declare `xmlns:ahyp` on themselves, which
  `detach_as` would refuse.
  **Lifted** for the same-part clone: relationship ids (`r:`, Strict
  `r:`, `o:relid`: same part, same `.rels`, still valid), namespace
  declarations inside the subtree, and `a:ext` whose `uri` is on an
  **allowlist** (round 2 N2), today only
  `{A12FA001-AC4F-418D-AE19-62706E023703}` (`ahyp:hlinkClr`).
  **Kept** (`FormatNotCopyable`, round 2 N3): any other `a:ext` (its
  content is open and may hold values that must stay unique); the
  `[detach] unique_attrs` — a non-zero `smtId` ("unique throughout the
  presentation", ECMA-376 §21.1.2.3.9; 0 in the corpus) and
  `a:effectDag`/`a:cont@name` ("unique across all effect trees and
  effect containers", §20.1.8.20/25; 0 inside `a:rPr` in the corpus);
  and Markup Compatibility inside the subtree. Two runs then share one
  hyperlink relationship, which §7's removal rule already handles (the
  relationship is removed only when no attribute names it). Until the
  core PR lands, `format_range` on a run with a link or an `a:extLst` is
  `FormatNotCopyable`.
- **Checks before the first split** (round 2 N4). A range splits at
  most two runs (its start and its end). Before either split, all of
  these are checked: the `RunFormat` values; `RangeSplitsField`; that
  **both** boundary runs' `a:rPr` can be cloned (above). "Already has every value" (principle 4) compares
  the run's **own** `a:rPr`, not resolved values: a run bold by
  inheritance still gets a direct `b="1"`, so ADR-0008 then reports it
  as `Source::Direct`, as principle 1 promises. The copies and their
  check are taken from **the document being edited**: in a transaction
  the clone that is staged (principle 6), in principle 6's degenerate
  case (one part's tree, no registry change — a `format_range` that adds
  or removes no link) the parsed tree edited in place. #49 binds a copy
  to the document it was taken from, and a clone is another document, so
  a copy from the parsed tree inserted into the clone is
  `Error::WrongDocument` (#49 review P49-m2; a false refusal, never a
  wrong write).
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
The bullet **type** group (`buNone`/`buAutoNum`/`buChar`/`buBlip`) is
always replaced. For each of the three appearance groups — colour
(`buClrTx`/`buClr`), size (`buSzTx`/`buSzPct`/`buSzPts`) and font
(`buFontTx`/`buFont`) — `Some(v)` writes that choice and `None`
**removes** the group's direct child, so the value is inherited again
(review m11); keeping an old explicit value would mix the new bullet
with the old one's appearance. Picture bullets (`a:buBlip`, 30) need an
image relationship: creating one is `UnsupportedBullet` in Phase 3.
Replacing an existing `a:buBlip` removes the element; its image
relationship is then removed only when no attribute of the part still
carries its `Id`, as in §7 (`remove_relationship` with the allowed
type `…/relationships/image`), and the image part stays (removing parts is
not a Phase 3 operation; an unreferenced media part is harmless, as in
Phase 0.1). Tab stops (`a:tabLst`) are written as a whole list.

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

- **Position and size** (EMU) are written to the shape's `a:xfrm` (for
  a `p:graphicFrame` — tables, charts, SmartArt, 328 in the corpus — its
  `p:xfrm`, which has no `p:spPr`; review m5: `set_rect` and rotation
  only, fill/line/geometry are `UnsupportedContent` there) **in its
  parent's coordinate space** (the slide, or the enclosing group's
  child space `a:chOff`/`a:chExt`). `ShapeMut::set_rect(Rect)` takes that
  space; `ShapeMut::set_slide_rect(Rect)` converts slide coordinates
  through the enclosing groups' scaling and offsets (the inverse of
  ADR-0008's `slide_rect()`), refused with `TransformedGroup` when an
  enclosing group is rotated or flipped (23 + 59 groups).
- **Group extents are never recomputed implicitly.** python-pptx's
  `recalculate_extents` sets a group's `chOff`/`chExt` equal to its new
  `off`/`ext`; on the 216 corpus groups with a scaled or offset child
  space this visibly moves every other member. A member may therefore lie
  outside its group's box after a move (we expect PowerPoint to
  recompute the box on the next interaction; **unverified**, a manual
  check item, review m6); `GroupMut::fit_to_children()` is the explicit
  call, which changes `off`/`ext` **and** `chOff`/`chExt` together so that
  members keep their slide positions. A group without `a:xfrm` (6 in the
  corpus) gets one first as the identity transform (`off = chOff`,
  `ext = chExt`, from the members' box).
- **Connectors** attached to a shape (`a:stCxn`/`a:endCxn`, 50) keep
  their stored route when the shape moves: Phase 3 does not reroute
  (like `spAutoFit` sizes, the route is a cache PowerPoint recomputes).
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
  `presetShapeDefinitions.xml`, exactly the 187 values of `ST_ShapeType`
  in `dml-main.xsd`), `set_adjustment(name, value)` validated against
  that preset's `a:avLst` guide names (123 presets have adjustments; 205
  corpus shapes set some; all 467 corpus `prstGeom/a:avLst/a:gd@name`
  values are names of their preset, so validation rejects no existing
  file); written as `a:gd name="adj1" fmla="val N"`. Some presets
  (`hexagon`, `star5`, `pentagon`, …) list `hf`/`vf` among their
  adjustments: these are aspect-ratio corrections PowerPoint writes
  itself, accepted like any adjustment and documented as such (review
  m7). Changing the preset **removes the old `a:avLst` entries** (their
  names belong to the old preset). `set_preset` on a shape with
  `a:custGeom` (1,626) is `UnsupportedGeometry` (custom geometry is
  preserved, never replaced implicitly).
  **Post-acceptance note (issue #113).** `set_adjustment` writes the
  preset's **whole** guide list: missing guides (including `hf`/`vf`) are
  inserted with the preset's defaults, in the preset's order, before the
  named value is set. A real PowerPoint 365 (16.0.20326) refuses to open an
  `upArrow` or `rightArrow` whose `a:avLst` holds only `adj1` (or only
  `adj2`), and opens the same shape with both guides or with an empty list.
- **Preset definitions provenance** (review m8). The file is not in the
  Part 4 download the vendored XSDs come from: it is in ECMA-376 **Part
  1**, 5th edition (the PDF cover says October 2016; Ecma's download
  is named `…_december_2016`, the date Part 4's `PROVENANCE.md` also
  uses — both name the same 5th edition),
  `ECMA-376-1_5th_edition_december_2016.zip` (43,501,721 bytes, SHA-256
  `9d0bcad9cf06054785b03762fcfadbf6bab7e54a5f9d69434e34b7fd464d4129`) →
  `OfficeOpenXML-DrawingMLGeometries.zip` (SHA-256
  `a8517838a3a60492d267d1ff92e1ebd831adc04584de79a38a5532a68902abf6`) →
  `presetShapeDefinitions.xml` (SHA-256
  `2f7c868d857c1e3c4b5a6068759fe0e07d77ad58377a6618d1b02ba3507b6939`).
  It is vendored unmodified in its own directory,
  `reference/ecma-376-drawingml-geometries/`, with a `PROVENANCE.md`
  (edition, URL, the three full hashes) and `COPYRIGHT-NOTICE.txt`. Its
  root element is spelled `presetShapeDefinitons` (sic) in the file;
  codegen matches that name. The Part 1
  edition has no copyright page either (page 2 of the PDF is blank, the
  zip has no notice file; checked 2026-11-16), so, as for Part 4, Ecma's
  default copyright notice applies (text copyright policy v3), whose
  derivative (iv) permits implementations of the standard. Codegen
  generates only a table of preset names, adjustment names and default
  values from it (`codegen --check` verifies the hash); the XML is not
  embedded in any crate. The generated table is a derivative under the
  notice's (iv), whose condition is to carry the notice: the crate that
  holds it lists the notice in its `THIRD-PARTY-NOTICES`, as ADR-0002 §2
  does for the schema-derived code (round 2 N6).

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

- **Relationships** are added through the core transaction
  (`RegistryPlan::add_relationship`, `rId{max+1}`, no gap reuse; ADR-0012
  §3) from the part that holds the link (slide, layout, master or notes
  slide), with the part's edited clone staged (`stage_part`) in the same
  plan (principle 6). A new link always gets its own relationship (no
  implicit sharing), even to an existing target.
- **Url** must be an absolute IRI or a relative reference with no
  control characters or spaces (encoded as `%20`); schemes are not
  restricted (PowerPoint stores `file:`, `mailto:`, custom schemes), but
  `javascript:`, `vbscript:` and `data:` (case-insensitive, after
  trimming) are `UnsafeLinkScheme` (review m13): PowerPoint asks before
  following them, but a library should not produce them. The
  relationship `Target` is written escaped (the core's target rules).
- **Slide** targets must be a slide of the same presentation;
  `SlideRef` is a `sldId` (stable across moves, ADR-0012 §3). The new
  `slide` relationship makes the target undeletable under ADR-0012 §4.2
  (`SlideIsReferenced`), whether the link is on a slide, layout, master
  or notes slide (review m10).
- **Removing or replacing** a link removes the element; its relationship
  is removed **only when no attribute of the part still carries that
  `Id`**: `RegistryPlan::remove_relationship` on the staged clone (with
  the allowed types `…/relationships/hyperlink` and `…/relationships/slide`,
  the two a link uses), which
  the core refuses while any attribute (any namespace) still names it, so
  Phase 3 only asks when its own count is zero (a run split by §3 shares
  a relationship between two runs). **Replacing** a link runs in this
  order, which the core requires (round 2 N7): `add_relationship` for the
  new target; edit the clone to the new `Id`; `stage_part`; then
  `remove_relationship` for the old `Id` (`stage_part` refuses content
  naming an `Id` removed earlier in the plan, and the removal's scan reads
  the staged content). POI's `linkToExternal` removes it
  unconditionally; python-pptx's `drop_rel` counts only `r:id`
  attributes.
- **Comment anchors on links** (review m10). A modern comment can anchor
  to a hyperlink (`oac:hlinkMkLst`), which ADR-0005 §6 locks as "any
  other anchor". Removing or replacing a link is not a formatting write
  for principle 5: on a shape or text body with such an anchor it is
  refused with ADR-0005's lock error. Adding a link where none was is
  allowed (no anchor can name it yet).
- **Text colour of a text link** (`text_color: true`) writes the
  Office 2019 extension `a:extLst/a:ext[@uri="{A12FA001-AC4F-418D-AE19-62706E023703}"]/ahyp:hlinkClr@val="tx"`
  (12 on slides, 14 with layouts and masters): the run keeps its own
  colour instead of the theme's `hlink`. The `ahyp:` namespace is
  declared on the created `ahyp:hlinkClr` itself, as PowerPoint does in
  14/14 (namespace declaration on created nodes: `insert_with_ns`, core
  PR #45). Without it, readers colour link text with
  `hlink` (ADR-0008 resolves which).
- `ppaction://media`, `macro`, `hlinkfile`, `ole`, `program` actions are
  preserved on read and not created (`UnsupportedAction`).

### 8. Notes slides

There is **one** notes-creation API (review m4, ADR-0012 review N9):
ADR-0012 §4.6's `Presentation::ensure_notes_slide(slide)`, which Phase 3
extends so that it also creates the notes master when the presentation
has none (ADR-0012's `NoNotesMaster` refusal is superseded). Text is
read and written through ADR-0008's `notes_mut` / `NotesMut`;
`notes_mut_or_create(slide)` is only a convenience that calls
`ensure_notes_slide` and then `notes_mut`. Everything below is one
transaction (principle 6).

1. **Is there a notes master?** Decided by the presentation → `notesMaster`
   **relationship**, not by `p:notesMasterIdLst` (review m1): python-pptx
   writes the relationship and part but no list, and the schema allows
   at most one `p:notesMasterId` (`CT_NotesMasterIdList`, `pml.xsd`).
   - Relationship and list present: use it.
   - Relationship present, list missing (python-pptx output): stage
     `presentation.xml` with a `p:notesMasterIdLst/p:notesMasterId`
     naming the **existing** relationship; nothing else is added.
   - No relationship, but notes slides exist (their own `notesMaster`
     relationships point somewhere or nowhere; `sc027-031.pptx`,
     ShapeCrawler output, has 45 notes slides and no master): refused
     (`NotesWithoutMaster`), because a new master would not be the one
     the existing notes use.
   - Neither (460 of the spike's 616 files with slides, of 619): add the notes master and its
     theme from the embedded template of ADR-0012 §8 (`add_part`
     `Numbered` for `notesMaster{}.xml` and `theme{}.xml`, their
     relationships, `Override`s), a presentation → `notesMaster`
     relationship, and stage `presentation.xml` with
     `p:notesMasterIdLst/p:notesMasterId@r:id` (schema position after
     `p:sldMasterIdLst`). The list is always written (Q2).
2. **Page and slide size** (review m2, round 2 N5). The template's
   notes master is laid out for its own `p:notesSz` (6858000 × 9144000)
   and 16:9 slides. Of the 449 decks without a notes master in the
   round-2 review's 606 files (619 deduplicated by SHA-256), 392
   use that `notesSz`; the rest use A4 (35), Letter (15), landscape (2)
   or others (5), and their `sldSz` is 16:9 in 271, 4:3 in 127 and other
   ratios in 51. The created notes master is therefore adapted to the
   deck: every placeholder's `a:xfrm` is scaled by the ratio of the
   deck's `p:notesSz` to the template's (x and y separately), and the
   `sldImg` box is then replaced by the largest box of the deck's
   `p:sldSz` ratio inside the scaled box, centred in it. 16:9 and 4:3 are
   special cases of this rule, so the template carries **no** 4:3
   variant (dropped in revision 3; it would have been an orphan part
   inside `Presentation::new()`'s package). PowerPoint also fits the
   slide image to the deck's ratio; its exact placement for unusual
   sizes is not specified, and the manual check looks at one A4 deck
   with 16:10 slides.
3. **Notes slide.** Part `notesSlide{}.xml` (`add_part` `Numbered`, max +
   1), relationships notes → notes master and notes → slide, slide →
   notes, `Override`; shape tree with the notes master's `sldImg`,
   `body` and `sldNum` placeholders cloned as in ADR-0012 §4.6
   (python-pptx's set), ids from 2.
4. **`docProps/app.xml`**: `Notes` is incremented, as for every
   notes-creation path (ADR-0012 §7, review m3).

### 9. Table style definitions

- `Presentation::add_table_style(TableStyleDef) -> StyleId` appends an
  `a:tblStyle` to `ppt/tableStyles.xml` (edited on a clone and staged,
  principle 6). The part is found through the presentation's
  `tableStyles` **relationship**, not by name (round 2 N7, like m1). When
  there is none (50 of the round-2 review's 606 deduplicated files), it is created
  (`add_part(PartNameRequest::Exact("ppt/tableStyles.xml"))`,
  presentation relationship, `Override`; a dangling relationship or a
  left-over `Override` using that name makes the core refuse with
  `NameInUse`, reported as `Core(…)`) with `@def`
  set to `{5C22544A-7EE6-4342-B048-85BDC9FD1C3A}` (Medium Style 2 –
  Accent 1, the `@def` of 538 corpus files) — **not** the new style's
  id, which would change the default of PowerPoint's "Insert Table" for
  the whole deck (review m9). `TableStyleDef { name, parts }` with the parts the corpus
  uses — `wholeTbl`, `band1H`, `band2H`, `band1V`, `band2V`, `firstRow`,
  `lastRow`, `firstCol`, `lastCol` (corner cells and `tblBg` are
  preserved, not created) — each with `tcTxStyle` (bold, italic, font
  reference, colour) and `tcStyle` (fill `ColorSpec`/no fill, the six
  borders).
- **Style ids** are GUIDs. Phase 3 generates a deterministic one (UUID
  version 8 from SHA-256 of the style name and the existing ids in the
  file, so a rerun produces the same file) that differs from every id in
  `tableStyles.xml` and from the 74 built-in ids PowerPoint knows (the
  list LibreOffice ships in `predefined-table-styles.cxx`; 74 counted
  again at LibreOffice master). It is written in `ST_Guid`'s form,
  `\{[0-9A-F]{8}-[0-9A-F]{4}-[0-9A-F]{4}-[0-9A-F]{4}-[0-9A-F]{12}\}`
  (`shared-commonSimpleTypes.xsd`): braces and **uppercase** hex (UUID
  libraries format lowercase by default).
- **Built-in styles by id** are ADR-0013 §3's: `set_style(Some(id))`
  already accepts the 74 built-in ids (compared case-insensitively)
  without a definition. Phase 3 only guarantees that the ids it generates
  never equal one of them (above).
- Existing definitions are never modified: other tables may use them.
  This is this ADR's own rule; `tableStyles.xml` is a format part, not one
  of ADR-0001's Layer 3 registries (review m9).

### 10. Theme edits (minimal)

`ThemeMut` (from a master: `Presentation::master_theme_mut(master)`):
`set_color(slot: Dk1|Lt1|Dk2|Lt2|Accent1..6|Hlink|FolHlink, Rgb | System)`
and `set_major_font` / `set_minor_font` (latin, ea, cs typefaces; script
fonts `a:font@script` are kept — 27,129 entries in the corpus). The
change applies to every slide that uses the master (that is what a theme
is). ADR-0008 has no theme `Source`: after the edit, a read of an element
that refers to the theme keeps its `Source` (e.g. `Direct` for a
`schemeClr` written on the run) and shows the new value in the resolved
`Font.theme` / `Color` (`mapped`, `base_rgb`); §13's provenance oracle
checks exactly that. A theme part referenced
by more than one master or by a master and a notes master is edited only
with `allow_shared: true` (`SharedTheme`; 0 shared between masters in the
corpus). No theme part is created or renamed; style matrices
(`fmtScheme`) are not edited. The edited theme is a clone staged in one
transaction (principle 6). A slide, layout, notes slide or chart with a
`themeOverride` keeps its override: theme edits do not reach colours or
fonts the override replaces (review m14). ADR-0008 does not apply
`themeOverride` (§1.4): theme-dependent lookups on such a part stay
`Unresolved(ThemeOverride)`, and Phase 3 does not change that.

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
| `NotesWithoutMaster` | §8: notes slides exist but no presentation → `notesMaster` relationship |
| `FormatNotCopyable` (Phase 0.1) | §3: a boundary run whose `a:rPr` the same-part clone refuses at the source — #49's `NotDetachable` with `UniqueAttribute` (non-zero `smtId`, `a:effectDag`/`a:cont@name`), `ExtensionList` (an `a:ext` not on the allowlist), `MarkupCompatibility` or `AlternateContent`; links and `a:extLst` until #49 lands |
| ADR-0005 lock errors | §7: removing/replacing a link anchored by a modern comment (`oac:hlinkMkLst`) |
| `SharedTheme` | §10 |
| `TableStyleNotDefined` (ADR-0013) | an undefined, non-built-in id |
| Phase 0 refusals | `InsideAlternateContent`, `UnsupportedContent`, … |
| `Core(…)` | `RegistryPlan` validation (name/relationship conflicts, a remaining reference), `TooDeep`, and #49's destination refusals (`NotDetachable` with `NamespaceBinding`, `IgnorableNamespace`, `InheritedXmlSpace`; `WrongDocument`), which a split never meets (the clone is a sibling in the same document) and so indicate a bug — all before `apply` |

### 12. Reference implementations (source-verified)

| Rule | python-pptx 1.0.2 | POI `47dcff76` | ADR-0016 |
|---|---|---|---|
| font name | sets `a:latin@typeface` only (`text/text.py` `Font.name`) | `setFontFamily` picks the font group from the run's first character (`XSLFTextRun`, `FontGroup.getFontGroupFirst`) | explicit script (`latin`/`ea`/`cs`), drops `panose`/`pitchFamily`/`charset` |
| colour | `ColorFormat.rgb`/`theme_color`, `brightness` as `lumMod`/`lumOff` (`dml/color.py`) | `setFontColor`: solid paint only, applies the colour transform to an RGB (`XSLFTextRun`) | `ColorSpec` with scheme slots and ordered transforms, written as given |
| underline | enum incl. `none` | `setUnderlined(bool)`: `sng`/`none` only | all 18 values |
| group extents | `recalculate_extents` sets `chOff=off`, `chExt=ext` (`oxml/shapes/groupshape.py`) | `setAnchor` / `setInteriorAnchor` separately (`XSLFGroupShape`) | never implicit; explicit `fit_to_children` keeping member positions |
| external link | new external relationship; old one dropped if referenced < 2 times by `r:id` (`text/text.py` `_Hyperlink.address`, `opc/package.py` `drop_rel`) | `linkToExternal` removes the old relationship unconditionally; tooltip = URL (`XSLFHyperlink`) | remove only when no attribute references it |
| slide jump | `ActionSetting.target_slide`: relationship to the slide + `ppaction://hlinksldjump` (`action.py`) | `linkToSlide` (same); `linkToNextSlide` etc. → `ppaction://hlinkshowjump?jump=…` | same actions; slide by `sldId` |
| notes master | `NotesMasterPart.create_default` + presentation relationship, **no** `p:notesMasterIdLst` (`parts/presentation.py`, `parts/slide.py`) | `createNotesMaster` from its template (`XMLSlideShow`) | detected by relationship; template part scaled to `p:notesSz`, `sldImg` fitted to `p:sldSz`, + relationship + `p:notesMasterId` |
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
- **T4 refusals:** every row of §11, byte-identical; including a
  `format_range` with an invalid value on a multi-run range (no split
  left behind), a range whose **second** boundary run is not copyable
  (the first boundary is not split either, round 2 N4), and a refusal by
  the core after the edits were staged.
- **Manual (owner):** PowerPoint opens a deck with each kind of write
  without repair (this covers Q2: a deck whose notes master we created);
  a group whose member was moved outside its box — does PowerPoint
  recompute the box on the next interaction (§6)? Q3–Q4.

## Consequences

- Positive: formatting can be written without touching inherited parts,
  and the provenance model of ADR-0008 reports exactly what was written.
- Positive: links, notes and table styles reuse the Phase 2 registry and
  transaction rules; no new allocation rule is introduced.
- Negative: no autofit computation: text may overflow until PowerPoint
  refits; `spAutoFit` shape sizes go stale; connector routes to a moved
  shape go stale too (§6).
- Negative: range formatting of runs with links or `a:extLst` needs the
  same-part clone in core (§3, `core/detach-same-part`); until that core
  PR lands it is `FormatNotCopyable`.
- Negative: a slide jump created by Phase 3 makes its target slide
  undeletable under ADR-0012 §4.2 until the link is removed (§7).
- Negative: the codegen allowlist grows substantially (`[write]` for
  formatting, shape, link, table-style and theme elements) and a new
  vendored input (`presetShapeDefinitions.xml`, ECMA-376 Part 1) appears,
  with its own provenance (§6).
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
4. **Share relationships for equal link targets.** Rejected: no implicit
   sharing; removal would then need reference counting across links the
   caller did not touch.
5. **Copy PowerPoint's built-in table style definitions into
   `tableStyles.xml`.** Rejected: their content is Microsoft's and not in
   ECMA-376; referencing the id is what PowerPoint itself does.

## Open questions for the project owner

Each has a **proposed default** (the round-1 review's recommendation),
used unless the owner decides otherwise. The former Q1 (smallest free
vs. largest + 1 part number) is settled by the core ADR-0001 note
"registry writes": largest + 1.

- **Q2** `p:notesMasterIdLst` (python-pptx omits it). *Default:* always
  written (§8): it is where the schema puts it, PowerPoint always writes
  it (0 corpus files with a notes master part lack it), and with
  detection by relationship a python-pptx deck gets the list added. Not
  an acceptance gate: the general "opens without repair" manual check
  covers it.
- **Q3** `ahyp:hlinkClr` (Office 2019+). *Default:* written on request
  only (`text_color: true`). Older readers ignore an `a:ext` they do not
  know; split runs keep it through the same-part clone (§3), which
  preserves its own `xmlns:ahyp` declaration and is allowlisted by its
  extension URI.
- **Q4** Built-in table-style GUIDs without definitions (§9). *Closed:*
  decided in ADR-0013 §3 (accepted, owner decision 2026-11-16): the 74
  built-in ids are accepted; other undefined ids stay
  `TableStyleNotDefined`.

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
  properties, colour transforms (order-sensitive), `smtId` uniqueness
  (§21.1.2.3.9), `presetShapeDefinitions.xml` (187 presets, 123 with
  adjustment guides) in `OfficeOpenXML-DrawingMLGeometries.zip` of
  `ECMA-376-1_5th_edition_december_2016.zip`; vendored Transitional XSDs
  (`ST_TextFontSize` 100–400000, `ST_TextUnderlineType` 18 values,
  `ST_Guid`, `CT_NotesMasterIdList`).
- MS-ODRAWXML `CT_TextCharRangeContext` (`oac:context@hash` over plain
  text).
- Core: PR #38 (`core/add-part`, `RegistryPlan`, ADR-0001 note "registry
  writes"); PR #18 (`detach_as`, ADR-0002 note "detach/insert"); PR #45
  (`core/insert-with-ns`, `insert_with_ns`); PR #49
  (`core/detach-same-part`, head `397bd86`: `detach_same_part_as`,
  `SamePartOptions`, `insert_same_part_by`, ADR-0002 note "same-part
  copy").

### Post-acceptance note — P3a direct text formatting

P3a implements §§2–5 on `RunMut`, `ParagraphMut` and `TextBodyMut`.
The write inputs `WriteColor`, `BulletSpec` and `AutoFitMode` coexist
with the Phase 1 resolver's `Color`, `Bullet` and `Autofit` results.
`RunFormat` supplies one optional `FormatChange` per property, including
one per font slot, and is also the input to `ParagraphMut::end_format`.
The resolver exposes the winning highlight, underline appearance,
paragraph font alignment, tab list and bullet appearance with their source.
Run ranges address Unicode scalar values, preserve field contents and
breaks, and split regular runs only where the direct properties change.
A covered field or break receives direct formatting in its own `a:rPr`
and stays intact. A boundary inside cached field text is rejected.
Formatting setters and clears, including attribute-only edits and
autofit reset, reject `mc:AlternateContent` inside the edited property
element with `InsideAlternateContent`. Replacing an explicit underline
line removes properties outside the requested `LineSpec`, including
custom dashes, joins, alignment and extensions.
Clearing underline also removes the direct underline appearance. A
`RunFormat` that clears underline and sets its appearance at the same
time is rejected before editing.

Single-part edits use a cloned tree when writing can fail after an
insertion. Replacing a picture bullet stages the slide or notes part
and removes its image relationship only if the staged part has no
remaining reference. The media part is retained. `ValueOutOfRange`
reuses the existing pptx error with its `what` field. No generated
public names were changed.

### Post-acceptance decision — P3b group fitting at integer EMU precision

Section 6 calls for fitting the group to its members while preserving
their slide rectangles. For some scaled groups no integer group
transform can satisfy both requirements with the child bounds as
`chOff`/`chExt`. Consider a group on the slide with horizontal
`off = chOff = 0`, `ext = 2`, and `chExt = 4`. Its children have
`(x, cx)` of `(1, 1)` and `(2, 2)` in group coordinates. The resolver
rounds each child's slide rectangle to `(1, 1)`.

The child bounds require `chOff = 1` and `chExt = 3`. Preserving the
first child's slide x requires `off = 1`. The first child's width rounds
to 1 only if the new `ext` is at least 2, while the second child's x
rounds to 1 only if the new `ext` is at most 1.

The fit uses the bounds of the immediate children's `a:off` and `a:ext`
in group coordinates. For each axis, set `chOff'` to the minimum and
`chExt'` to the difference between the maximum and minimum. Preserve the
old affine map by computing these values with exact rational arithmetic:

- `off' = off + (chOff' - chOff) * ext / chExt`
- `ext' = chExt' * ext / chExt`

Reject without changing bytes if either result is fractional, either
child extent in the calculation is zero, or arithmetic overflows. The
counterexample above is rejected because both new values are fractional
(`off' = 1/2` and `ext' = 3/2`). A successful fit preserves every
descendant's `slide_rect()`, regardless of resolver rounding.

For a group with nonzero rotation or either flip, only an unchanged box
is accepted. With valid nonzero extents, unit scaling (`ext == chExt`)
always admits an exact fit for an unrotated, unflipped group. For a
group without `a:xfrm`, create the identity transform using the child
bounds for both coordinate spaces.

### Post-acceptance decision — P3b preset definitions

The Ecma XML is vendored byte for byte. It contains two identical
`upDownArrow` definitions and omits `upArrow`. Codegen collapses identical
definitions and rejects conflicting duplicates. A handwritten supplement
provides only the `upArrow` adjustment defaults, `adj1 = adj2 = 50000`.
They are checked against the in-file `downArrow` definition because
`upArrow` is its vertical mirror. Supplement entries cannot overlap the
XML, and the combined names must equal the 187 XSD `ST_ShapeType` values.

### Post-acceptance note — P3b shape property APIs

Shape outline edits update only fields supplied in `LineSpec`, retaining
other line attributes and children. The outline lens opts into preserving
the source spelling of unchanged attributes even when an edited start tag is
re-rendered. Passing `None` removes the direct
outline. `ShapeFill::Inherit` removes the direct fill.

`clear_rect` removes the entire direct transform on placeholders and
returns `UnsupportedContent` on other shapes. A transform missing its
offset or extent still inherits geometry. Editing an absent or incomplete
placeholder transform fills missing rotation and flip attributes from
inheritance and preserves the source spelling of existing unedited
attributes. Rotation and flip edits use the inherited rectangle.
`set_rect` uses the supplied rectangle without validating the inherited
offset and extent.

Equal rotation and flip writes leave the part unchanged. The first
transform edit on a group without a transform derives its child space
from member bounds. A group rectangle edit changes its outer box while
preserving the child coordinate space. If a legacy transform omits
`chOff` or `chExt`, a rectangle edit stores the previous fallback value
as an explicit child offset or extent.

Group fit uses `ValueOutOfRange { what: "exact group fit" }` for
unrepresentable affine fits. A fit that changes the box of a rotated or
flipped group returns `TransformedGroup` before exactness is checked.

### Post-acceptance note — P3c hyperlinks and actions

- `RunMut::set_link`, `FieldMut::set_link` and `ShapeMut::set_action`
  take a click or hover event and an optional `LinkSpec`. Passing
  `None` clears that event. Reapplying the same settings preserves the
  existing link element and its relationship.
- `Presentation::slide_link_target(sldId)` creates the write target
  `LinkTarget::SlideId`, scoped to the open presentation. The existing
  read variant `LinkTarget::Slide { slide_id, index }` continues to
  describe the resolved destination. The setter returns
  `ForeignLinkSlide` for a target from another session and
  `LinkSlideNotFound` for a missing slide id. The read variants `Slide`
  and `Part` return `ReadOnlyLinkTarget` when passed to a setter.
- For link writes, `InvalidLinkUrl`, `ReadOnlyLinkTarget` and
  `ForeignLinkSlide` replace §11's `InvalidLinkTarget`.
  `LinkSlideNotFound` replaces §11's `SlideNotFound` for link targets.
- URL writes accept a nonempty IRI reference. Supply spaces as
  `%20`. The setter returns `InvalidLinkUrl` for malformed references
  or bidirectional formatting characters (U+200E–U+200F,
  U+202A–U+202E, U+2066–U+2069), and `UnsafeLinkScheme` for the
  three schemes listed in §7.
- The staged registry removal uses core's any-attribute scan. A
  `StillReferenced` result keeps the old relationship and allows the
  edit to commit. Relationships of other types remain owned by their
  respective features. Replacing or clearing a link with an `a:snd`
  child returns `LinkHasSound` and preserves the sound relationship.
  SmartArt text link edits return `SmartArtLinkUnsupported`.
- Q3 uses the §7 default. A text link writes the colour extension when
  `text_color` is true. Replacing or clearing an anchored link returns
  `CommentAnchoredText`. Unknown comment anchors also lock link
  replacement and removal under ADR-0005 §6. A known text-range anchor
  (`ac:txMkLst`) locks text edits. Unsupported action targets return
  `UnsupportedAction`.

### Post-acceptance note — P3d notes creation

`Presentation::ensure_notes_slide(index)` implements §8. `notes_mut_or_create`
runs the same registry transaction and returns an editable `NotesMut`.
The relationship identifies the master. A missing list is inserted in schema
order, and an existing list entry is updated if its relationship ID disagrees.
With existing notes and no master relationship, both entry points return
`NotesWithoutMaster`. The Notes count is updated with the existing §7 policy
of ADR-0012 for readable, present counters.

The new master uses the embedded notes-master/theme pair. Its placeholder
coordinates are scaled independently on each axis, rounded to the nearest
EMU. The slide image's fitted extent rounds down to stay inside the scaled
box, and positioning rounds down when centering lands on half an EMU.
Absent or unusable dimensions use the template dimensions for that size.
Each new notes slide uses the master's placeholder order and inherits its
geometry. A master without a body placeholder still gets an editable body
on the new notes slide. Manual case 33 uses an A4 page and a 16:10 slide.
