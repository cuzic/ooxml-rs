# Word Phase1 spike (evidence for ADR-0007)

Throwaway scripts. Nothing here is production code. Every `.docx` input is untrusted data:
the Python scripts only read it with `zipfile` + `ElementTree` (expat; no external entities)
under `python3 -I`, skip packages with a DTD in any part, and refuse element nesting deeper
than 256 like core (ADR-0004 O5). Run them from a directory other than the inputs'.
`p1common.py` is shared; scripts import it by putting their own directory on `sys.path`
(`-I` removes it). Nothing downloaded is executed.

## Corpora

| Name | Files | Source |
|---|---|---|
| `fx` | 26 (19 pass the Phase0 gates) | `reference/fixtures/docx/` |
| `poi` | 129 (101 pass) | Apache POI `test-data/document/*.docx`, trunk `47dcff76` (as ADR-0004) |
| `sdk` | 444 non-Strict (404 pass) | Open XML SDK test assets at `431ab05c`, outside `O14ISOStrict/` (as ADR-0004) |
| `d4j` | 12 (10 pass) | docx4j sample documents (as the Phase0 corpus check) |
| `tpl` | 212 (212 pass) | **new**: every `.docx` in two template engines' test suites: python-docx-template `1f143fbe` `tests/templates/` (40) and docxtemplater `e1d61c64` `examples/` (172). Word-authored templates with `{{…}}`/`{…}` tags; not vendored (licences: LGPL-2.1 and unasserted) |

"Pass" = no Phase0 document gate (encryption, DTD, Strict, depth, W1 `TrackRevisionsOn`,
W3 `DocumentProtected`, W2 `StaleFieldRisk`), computed by `p1common.gate`, a simplified port
of ADR-0004 §6 that reproduces its file-level outcomes (POI 101 + 1 depth-refused, SDK 404).

## Scripts

| Script | Measures | ADR-0007 |
|---|---|---|
| `replace_sim.py` | Builds each paragraph's text stream (all stories) with run groups and separators, and classifies replace targets with the proposed rule: **A** what separates adjacent text runs and how their `rPr` compare; **B** "boundary needles" (the ≤6-character span centred on each run boundary, the pure cross-run case); **C** every `\w{2,}` word; **D** template placeholders; **E** a Python prototype of the differential oracle (T7) on every accepted match | §1, §9 |
| `stories_cells.py` | Settable text runs per story kind, note/comment mark runs, header/footer reference sharing; per table cell the outcome of `set_cell_text`; what empty paragraphs' mark run properties hold | §2, §3 |
| `formatting_styles.py` | Lexical forms of the proposed direct-formatting subset, pairing of `b`/`bCs`, `i`/`iCs`, `sz`/`szCs`, theme and `*Chars`/`*Lines` attributes; styles: id vs name, undefined or wrong-type references, duplicates, latent-only styles | §4, §5 |
| `fields_perop.py` | For StaleFieldRisk files: which per-operation table entries and the `IF` rule would lift W2; `mailMerge`, `updateFields`, `fldChar/@dirty` presence | §6 |
| `readback/` (Rust, standalone workspace) + `readback_stories.py` | What docx-rs 0.4.22 reads outside `document.children`, compared with ElementTree per story part | §9 (T5) |
| `slot_order.py` (revision 2) | Parents Phase1 inserts into (`w:rPr`, paragraph-mark `w:rPr`, `w:pPr`, `w:settings`) whose existing children are out of SDK slot order, duplicated, or include `mc:AlternateContent` (review M2) | §4, §6.3, §7 |
| `wml-phase1.toml` + `allowlist_dryrun.sh` | The proposed allowlist run through the real generator on a scratch copy of the tree; prints each wml entry's lowering and diagnostics, then `cargo check -p ooxml-schema --features wml` | §8 |
| `summarize.py` | Percentages quoted in ADR-0007 from the JSON outputs | — |

```sh
C=<corpus dir>; S=spikes/docx-phase1; R=<results dir>
python3 -I $S/replace_sim.py   --json $R/rs_poi.json $C/poi
python3 -I $S/stories_cells.py --json $R/sc_poi.json $C/poi
python3 -I $S/summarize.py $R poi
python3 -I $S/formatting_styles.py $C/poi
python3 -I $S/fields_perop.py --list $C/sdk
(cd $S/readback && cargo build --release) && $S/readback/target/release/docx-phase1-readback-spike $R/rb $C/poi/*.docx \
  && python3 -I $S/readback_stories.py $R/rb $C/poi
$S/allowlist_dryrun.sh <impl/docx-phase0 checkout> $R/dry --no-detach
$S/allowlist_dryrun.sh <core/detach-insert checkout> $R/dry-detach
```

## Results (2026-11-16, Python 3.15, rustc 1.99.1; re-run for ADR-0007 revision 2)

Revision 2 aligned the spike with the ADR (review round 1, m2):

- deleted (`w:del`/`w:moveFrom`) text is no longer in the stream;
- `w:rPr` child order is significant in the comparison;
- ruby base runs are descended (context `Ruby`, refused);
- the `xml:space` count uses the Phase1 run rule;
- the `w:hint` difference counts as invisible only for ASCII/CJK matched characters (m1).

Section A is unchanged by these. B–E were re-run, and the tables below are the revision 2
numbers.

### A. Adjacent text runs (separated only by nothing, `proofErr` or range markers; all stories)

| | fx | POI | SDK | tpl |
|---|---|---|---|---|
| pairs | 1,935 | 5,741 | 12,035 | 1,299 |
| `rPr` canonically identical | 77.8% | 75.6% | 45.8% | 91.2% |
| differ only in `w:lang/@w:val`, `w:noProof` ("proofing") | 0 | 3.7% | 0.5% | 1.2% |
| differ also in `w:rFonts/@w:hint` ("hint") | 0.2% | 0.0% | 27.4% | 1.2% |
| differ visibly | 22.0% | 20.7% | 26.3% | 6.4% |

The SDK "hint" pairs come from East Asian documents, where Word sets `w:hint="eastAsia"` per
run. `w:lang/@w:eastAsia` and `@w:bidi` differences count as visible (they can change glyphs).
The denominator differs from roadmap Q1 (direct sibling runs of `document.xml` only).

### B–D. Replace targets under the proposed rule (§1)

| | fx | POI | SDK | tpl |
|---|---|---|---|---|
| boundary needles | 2,511 | 6,678 | 13,893 | 1,358 |
| accepted, identical `rPr` only | 58.7% | 63.3% | 36.1% | 83.1% |
| accepted, **default rule** (identical + proofing + hint on ASCII/CJK only) | 58.9% | 66.5% | 57.5% | 85.2% |
| accepted with `FirstRunFormatting` opt-in | 76.2% | 85.3% | 82.7% | 93.0% |
| words accepted (mostly single-run) | 99.6% | 99.7% | 98.4% | 99.5% |
| **template placeholders accepted** | — | — | — | **511 / 517 (98.8%)**, 204 of them cross-run, 0 visible-format mixes except 1 |

The 6 refused placeholders: 2 inside a content control (`UnsupportedContext`), 1 crossing a
content control, 1 crossing a hyperlink, 1 inside a field result, 1 `MixedFormatting`.
Boundary needles refused for other reasons (POI / SDK): `MatchCrossesBoundary` 10.2% / 6.7%
(hyperlink, smart tag, non-text run, math), `UnsupportedContext` 1.2% / 7.6% (content
controls), `InsideField` 3.0% / 0.8%, `MarkerInsideMatch` 0.2% / 1.9% (mostly
`commentRangeStart`). Collapsed bookmarks (start immediately followed by its end; 92 of 95
`_GoBack` in tpl) are treated as transparent points; without that rule 6 tpl placeholders were
`MarkerInsideMatch:bookmarkStart`. **E:** the per-character oracle held for every match it
checked (revision 2: fx 1,483; POI 4,643; SDK 10,698; d4j 14; tpl 1,716).

**Content outside the match (revision 2, review M3; `M3_outside_match_content`).** Revision 1's
whole-run rewrite would have touched content outside the match in these accepted
boundary-needle runs:

- `w:softHyphen`: POI 4, SDK 2;
- `w:noBreakHyphen`: SDK 6;
- `w:lastRenderedPageBreak`: fx 14, POI 39, SDK 128;
- templates and docx4j: 0.

Revision 2's child-local edit keeps all of them byte-identical.

`w:t` with edge whitespace and no `xml:space="preserve"` in settable runs: POI 132, SDK 15,
tpl 21; 3 tpl placeholder matches touch such a run.

### Stories (§2): settable text runs (Phase1 run rule; smart tags stay refused)

| | header | footer | footnotes | endnotes | comments | main |
|---|---|---|---|---|---|---|
| POI | 40/43 | 54/65 | 42/43 | 3/4 | 8/8 | 22,825/23,065 |
| SDK | 141/163 | 82/166 | 312/312 | 20/20 | 1,455/1,455 | 24,526/25,555 |
| tpl | 113/114 | 243/251 | 4/4 | 0/0 | 4/4 | 2,765/2,774 |

Footer refusals are field results (`PAGE`, `NUMPAGES`). Every normal footnote/endnote has its
`w:footnoteRef`/`w:endnoteRef` in a run of its own (POI 12/12, SDK 136/136), as does every
comment's `w:annotationRef` (POI 4/5, SDK 1,148/1,153 comments have one). Separator notes:
`separator` and `continuationSeparator` in every notes part, one `continuationNotice` (SDK).
No header or footer part is referenced by two `w:sectPr` references in any corpus; 1 POI and
12 tpl parts are referenced by none. Smart tags (not proposed for Phase1): POI 30, SDK 358 runs.

### Cells (§3): `set_cell_text` outcome per `w:tc`

| | fx | POI | SDK | tpl |
|---|---|---|---|---|
| cells | 667 | 16,235 | 6,567 | 672 |
| settable | 95.2% | 84.5% | 80.1% | 84.8% |
| settable, excluding vMerge/hMerge continuations | 95.3% | 95.5% | 83.1% | 88.0% |
| … of which empty paragraph, mark `rPr` copied into a new run | 4 | 1,103 | 350 | 4 |
| … of which empty paragraph without mark `rPr` | 2 | 92 | 1,234 | 14 |

Main refusals: several paragraphs or a nested table (`CellNotSimple`: POI 395, SDK 584),
merge continuations (POI 1,867, SDK 234), content controls (SDK 410), `MixedFormatting`
(POI 198). Empty paragraphs in all stories: mark `rPr` holds a hidden flag (`MarkHidden`)
in POI 5, SDK 2, fx 2; a revision in SDK 1.

### Formatting and styles (§4, §5)

- on/off `val` on the 16 common properties: absent (POI 4,529, SDK 10,785, tpl 452), `0`
  (70, 549, 30), `false` (25, 0, 10), `1` (7, 4, 12), `on` (3, 0, 0).
- `b` with `bCs`: POI 1,397 of 1,848, SDK 1,605 of 3,578; `sz` with `szCs`: POI 18,904 of
  18,926, SDK 3,865 of 16,480. Different values within a pair: SDK 13 (`b`), 18 (`i`), 62 (`sz`).
- `w:color`: `RRGGBB` upper case 5,017 / 7,022 / 398, `auto` 49 / 707 / 15, lower case 0 / 0 / 1;
  with `w:themeColor` (which supersedes `val`) 21 / 718 / 42.
- `w:rFonts` theme attributes (supersede the explicit font): `asciiTheme` 12 / 714 / 3,
  `eastAsiaTheme` 68 / 792 / 7. `w:hint` 5 / 8,890 / 98.
- `w:jc`: `left` 110 / 270 / 136, `start` 96 / 0 / 108, `end` 0 / 0 / 10, `right`, `center`,
  `both`, `distribute` (6, SDK).
- `w:ind` character units (supersede twips): `leftChars` 0 / 315 / 0, `firstLineChars`
  0 / 415 / 0; `start`/`end` 2 / 0 / 4. `w:spacing` `beforeLines`/`afterLines` 0 / 35 / 0,
  `*Autospacing` 178 / 309 / 130.
- Styles: styleId ≠ `w:name` (beyond case and spaces) for 2,937 / 1,104 / 1,915 styles,
  because localized Word derives ids from the UI name (`Rubrik` → `Title`,
  `Standardowy` → `Normal Table`). No duplicate ids, exact or case-insensitive. References to
  undefined ids: `pStyle` 22 in POI (one file, `60329.docx`), 0 elsewhere; wrong style type: 0.
  Latent styles without a definition: 19,173 / 61,571 / 35,784 (`lsdException` entries).
  Packages without a styles part: POI 4.

### Slot order of the parents Phase1 inserts into (revision 2, `slot_order.py`)

Writable files; parents with children out of SDK slot order (`order`), a listed QName twice
(`dup`) or an `mc:AlternateContent` child (`ac`):

| | fx | POI | SDK | d4j | tpl |
|---|---|---|---|---|---|
| run `w:rPr` checked / bad | 3,147 / 0 | 21,803 / 0 | 23,223 / 0 | 120 / 0 | 2,654 / 0 |
| paragraph-mark `w:rPr` checked / order / dup | 899 / 0 / 0 | 19,738 / 0 / 0 | 10,819 / 0 / 0 | 76 / 0 / 0 | 1,743 / 1 / 56 (15 files) |
| `w:pPr` checked / order | 1,075 / 0 | 20,552 / 0 | 17,648 / 0 | 178 / 0 | 2,381 / **9** (5 files) |
| `w:settings` checked / order / dup | 19 / 0 / 0 | 99 / 0 / 1* | 404 / 1 / 0 | 10 / 0 / 0 | 212 / **18** / 2 |

\* `w:activeWritingStyle`, which is repeatable: not a defect. The template `w:pPr` cases are
docxtemplater outputs (`[sectPr, pStyle, bidi, jc, rPr]`). The `w:settings` ones are
LibreOffice-written (`w:hyphenationZone` after `w:compat`) plus one SDK `m:mathPr` after
`w:rsids`.

### Inherited superseding attributes (revision 2, review m3; `formatting_styles.py`, "inherited")

| | POI | SDK | tpl |
|---|---|---|---|
| files whose `docDefaults` `rFonts` has `asciiTheme` | 72 | 392 | 82 |
| direct `rFonts/@ascii` without `asciiTheme` in those files | 7,193 | 1,249 | 186 |
| direct `ind/@firstLineChars="0"` (cancels an inherited value) | 0 | 90 | 0 |
| paragraph whose style has `*Chars` and whose direct `ind` has twips only | 0 | 4 | 0 |

### Fields (§6)

StaleFieldRisk files: POI 1, SDK 30, d4j 2, tpl 0. Under the proposed per-operation table
the following become writable: text edits 1 (SDK `table.docx`, `SEQ` only), formatting 6,
append 1. The style column (1 file) was dropped in ADR-0007 revision 2 because `SEQ \s`
depends on headings. `fields_perop.py` still reports it. The 18 files with `TOC` + `PAGEREF` stay refused under every
operation. `IF`: 5 occurrences (SDK), none passes the operand rule (deferred in revision 2)
(`IF DATE \@ "yyyy" = "1998" …`: a bare word as an operand; one unbalanced quote).
`settings/w:mailMerge`: POI 1, SDK 1. `w:updateFields`: 0 in POI/SDK, 2 in tpl (`true`).
`fldChar/@w:dirty`: 0 everywhere.

### docx-rs read-back of stories (§9, T5)

| | header parts exposed / text equal | footer parts exposed / text equal | comments equal | footnotes, endnotes |
|---|---|---|---|---|
| fx | 6 of 8 / 6 | 6 of 6 / 6 | 13 / 13 | no reader |
| POI | 57 of 60 / 57 | 53 of 54 / 53 | 5 / 5 | no reader |
| SDK | 117 of 156 / 114 | 134 of 177 / 125 | 1,153 / 1,153 | no reader |
| tpl | 139 of 166 / 139 | 131 of 156 / 131 | 4 / 4 | no reader |

docx-rs exposes only the header/footer parts referenced by the body-level `w:sectPr`
(`header`/`firstHeader`/`evenHeader`, same for footers), reads comments, and has no reader for
footnotes or endnotes (its `footnotes` field is filled by the writer only). The 12 SDK
differences are text in text boxes/`mc:AlternateContent` and content controls, which docx-rs
reads into the paragraph and Phase1 does not treat as flow. docx-rs returned an error on 2 SDK
files and 17 POI files and panicked on 1 POI and 1 SDK file; none of them passes the Phase0
gates. (Run the reader on `*.Dotx` too: two SDK files have an upper-case extension.)

### Allowlist dry run (§8)

All 66 wml entries of `wml-phase1.toml` lower without a diagnostic (the 16 wml diagnostics in
the report are the pre-existing `EcmaOrder`/`UnresolvedBase` ones for types not on the list),
and `ooxml-schema` with feature `wml` compiles. Lowering: `w:rPr` (`CT_RPr`) 51 slots,
`w:rPr` (`CT_ParaRPr`) 57, `w:pPr` 36, `w:settings` 103, `w:style` 22, `w:tcPr` 15, story roots
1 slot each. With PR #18's generator and the `[detach]` section, `Detachable` is generated for
both `rPr` entries and **no** `SameType` between them (different complex types); their child
lists differ only in `ins`, `del`, `moveFrom`, `moveTo`, `w14:conflictIns`, `w14:conflictDel`,
`oMath` and the `rPrChange` type, and agree in order otherwise.
