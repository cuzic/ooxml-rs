# ADR-0018 review (Opus, round 1)

対象: plan/xlsx-phase7 @ 9349cae309a0f7ee437d6227e56d67448e36a39c

Scope: PR #110 `docs/adr/0018-xlsx-phase7-structural-edits.md` (Proposed, revision 1) and
`spikes/xlsx-phase7/` (worktree `plan-xlsx-phase7`).
Read against: ADR-0003 rev 5 (+ §3.1a P5-pre), ADR-0009 rev 4, ADR-0010, ADR-0014 rev 3,
ADR-0017 rev 3, and `origin/main` @ `1000645` (`crates/ooxml-xlsx/src/{edit.rs,sheet_ops/,formula/,tables/}`,
`crates/ooxml-core/src/{lens.rs,xml/node.rs,xml/write.rs}`).

Corpus: POI `test-data/spreadsheet` @ `47dcff76` (366 files in `/tmp/poi-xlsx`) + the 2 calamine
fixtures. I reran `python3 -I spikes/xlsx-phase7/structural_survey.py --summary` (5 s) and
**every number in the Context table and in the registry's corpus column reproduced** (343
packages, 813 worksheets, 1,502,954 cells, 286/6 shared-group split, 798 and 773 blockers, …).
Additional probes (scratch scripts, not committed) are cited below as "probe".

Libraries checked at the pinned revisions: POI `FormulaShifter.java` (L356-383, L418-505 match the
ADR), `XSSFRowColShifter.java` (L60-302; L105-136 rewrites a shared master with no member check,
as the ADR says), `XSSFRowShifter.java`, `XSSFSheet.java` (L3285 is the "not implemented yet"
comment; `shiftRows` does re-fill calculated columns of overlapping tables but leaves table `ref`
alone, consistent with the ADR); LibreOffice `refupdat.cxx` L187-282 and `token.cxx`
L2809-3420. The citations are accurate. Two of them say more than the ADR uses (M1, M2).

## Conclusion first

The overall design holds up: a registry kept as data and checked against the XSD, one rewriter
per syntax, LibreOffice's reference model without `ExpandRefs`, shared groups re-checked one
member at a time, and refusal by default. I reran the shared-group simulation at **every** edit
position, not only mid-group, and the 98% figure holds (m2). Four things are unsound or
unworkable as written:

1. **Formula places on *other* sheets other than cell formulas are not rewritten.** These are
   CF, DV, x14 and sparkline formulas, and table formulas. The registry rows are keyed to the
   edited sheet. The apply step and the locality oracle list only `f` text and
   `hyperlink/@location` for other sheets. The closed-world scan accepts those places because
   they are registry rows. A cross-sheet DV list source or sparkline would therefore keep
   pointing at the old cells **silently** (B1).
2. Ranges that end at the sheet edge are refused (`ShiftOffSheet`) instead of being kept sticky.
   This alone refuses **every column insert in 43 of 343 packages** (`col max="16384"`) (M1).
3. Seeds miss formulas whose text does not change but whose value does. These are whole-column
   references, names and tables whose range grew or shrank, and `MATCH`/`INDEX` over shifted
   content. LibreOffice flags exactly these (M2).
4. The apply order uses child indices computed before rows are removed. The ADR also does not
   say how `CellWritePlan` fits into a G1 transaction (M3).

There are two more Majors: the closed world for unprefixed positions is not defined, and
extension schemas are outside the completeness test (M4); and copying a sheet duplicates
`codeName` (M5).

**Verdict: no** (1 Blocker, 5 Major, 14 Minor, 7 Nit).

---

## Blocker

### B1. CF, DV, x14, sparkline and table formulas on other sheets are left stale

**Problem.** A1 references in these places can point at another sheet:

- `dataValidation/formula1` (a list source `=Lists!$A$1:$A$20` is the textbook case);
- `cfRule/formula`;
- x14 `xm:f`: Excel 2010+ writes cross-sheet DV and CF here;
- sparkline data `xm:f`: always sheet-prefixed, and usually on another sheet;
- table `calculatedColumnFormula` and `totalsRowFormula`.

Yet the ADR rewrites them only on the edited sheet:

- Registry rows L16–L21 and L37 (ADR L304-309, L325) have part "worksheet" or "table". In
  contrast, L4 and L14 say "(any sheet)".
- §2.1 step 3.2 (L223): "every other sheet: rewritten formula texts and `hyperlink/@location`s".
- Oracle 1, "Other parts" (L829): "other sheets: changed `f` text, seeded `v`/`is`/`t`,
  `hyperlink/@location`". This gives no allowance for a DV/CF/x14/sparkline change on another
  sheet, or for another sheet's table formulas.
- §3.6.1 only discusses tables on the edited sheet.
- The closed-world scan (§2.2, L254-259) refuses only "a hit outside a registry row". An x14
  `xm:f` on Sheet2 that names the edited sheet **is** a registry row (L20), so the scan passes
  it. If the dispatcher follows §2.1, nothing rewrites it.

This is exactly the failure the ADR exists to prevent ("a place that holds a reference and is
not rewritten silently points at the wrong cell", L50-52).

**Evidence (spike summary).** `x14.ref_sheet_prefixed` 15 texts in 2 files; sparklines 11 in 1
file (their `xm:f` are sheet-prefixed); `dv.name_use` 14 in 4 files, which reach other sheets
through names (names themselves are handled). ADR-0009's implementation refuses these places
today (scan.rs L54-72 has no DV/CF/x14 arm). P7-pre removes those refusals (Delivery 0), so the
gap opens exactly when the registry lands.

**Fix.**

- Give every `Formula`-syntax row a scope column: `own-sheet` or `any-part`. Mark L16/L17,
  L19-L21, L37, L46 and L47 as `any-part` for the "reference names the edited sheet" case.
  Keep the `own-sheet` anchor for unprefixed tokens (§2.3 already defines "refers to the edited
  sheet" this way).
- Make the dispatcher walk every part that holds a row, not "the edited sheet + `f` elsewhere".
- Extend §2.1 step 3.2 and oracle 1's "Other parts" with: other sheets' CF/DV blocks and
  `extLst` content, sparkline `xm:f`, and other tables' formula elements.
- Removal rules then also apply across sheets. A sparkline whose *data* range is wholly
  deleted needs a rule; propose refusal (`ReferenceDeleted`), as for chart series.
- Oracle 2: require each `any-part` row to have a fixture case on a **second** sheet.

---

## Major

### M1. Ranges that end at the last row or column are refused instead of kept sticky. Column inserts in 43 files fail.

**Problem.** §3.2 (L416-417) keeps only a range spanning **the whole axis** sticky. For an
insert it refuses (`ShiftOffSheet`) when "any other component [is] pushed past row 1,048,576 or
column XFD". The insert refusals (L434-438) add "any … other registry range that reaches there".
`cols` follows the range rule (§3.4, L535).

**Evidence.**

- Probe: **113 `col` elements with `max="16384"` and `min > 1`, in 43 packages**. The spike's
  `col_to_XFD` reports 106 in 44, and the ADR records that number (L100) but does not use it.
  Every `insert_cols` on those sheets is refused under the ADR's rule. This is not counted in
  the "Blockers" row (L103) or in Consequences (L934).
- LibreOffice keeps a range end at the maximum sticky **whatever the start**:
  - `refupdat.cxx` L270-276 for non-formula ranges: "End was sticky, but start may have been
    moved. Only on range."
  - `token.cxx` L2956, L2967 and L3044 (`IsEndRowSticky`) for formulas.
  - For a range end that would move past the maximum, it cuts the end at the maximum
    (`lcl_MoveEnd`, L49-67, `bCut`) and does not refuse.
  - The ADR cites L263-268 but not the next branch.
- The same pattern turns up as DV and CF on a column minus its header (`A2:A1048576`). Probe:
  2 `selection` instances in this corpus. It is common in files written by Excel, and the
  ADR's own fixture plan should include it.

**Fix.** Replace the last two rows of the §3.2 table:

- a component **equal to** the maximum is sticky (the range's start still moves or shrinks);
- a range end pushed past the maximum is **clipped** to it;
- only a single cell, or a range **start**, pushed past the maximum is refused. LO and Excel
  give `#REF!` there; refusing is fine.

Keep the "content pushed off" refusal for `row`/`c` elements and for objects with content
(merges, tables, notes, anchors). Do not apply it to `cols`, `dimension`, `selection`, CF/DV
`sqref` or `ignoredErrors`. Add `p == max − n`, sticky-end and clip cases to oracle 10. Recount
the G1 blockers in the spike with the new rule.

### M2. Seeds miss formulas whose value changes while their text does not

**Problem.** §3.7 (L634-640) seeds four sets: formulas with changed text, formula cells that
moved, dependents of deleted cells, and `Everything`. Three kinds of formula change value with
no text change and no deleted precedent:

1. **Sticky whole-column or whole-row references.** On a row insert, `A:A` keeps its text, but
   the content inside it shifts. `MATCH(x,A:A,0)`, `INDEX(A:A,5)`, `LOOKUP`, array positions and
   `OFFSET(A:A,…)` then return different results. LibreOffice marks exactly this case
   `mbValueChanged = true`: `token.cxx` L3338-3346, "In entire col/row, values are shifted
   within the reference, which affects all positional results like in MATCH or matrix
   positions". The ADR cites those lines only as "sticky, unchanged". The corpus has 1,644 such
   texts in 5 files (`cell.ref_cols`).
2. **Names whose range grew or shrank.** `=ROWS(MyRange)` or `=SUM(Print_Area)`: the name's text
   changes, the using formula's text does not.
3. **Tables whose `ref` grew or shrank** (§3.6.1). `=ROWS(Table1)` and `=SUM(Table1[Qty])`: the
   structured reference is unchanged by design (§3.3.1, L454-455), but its extent changed.

ADR-0003 and its rebuild strip caches so that non-recalculating readers (LibreOffice, calamine,
the CR oracle) do not show stale values. `fullCalcOnLoad` covers Excel only. This is the
soundness property ADR-0003 §3.2 is built on.

**Fix.** Add to §3.7 the census dependents of every area on the edited sheet that

- (a) lies at or after `p` and is sticky (whole axis, or end at the maximum, after M1);
- (b) belongs to a defined name whose text changed; or
- (c) belongs to a table whose `ref` changed.

(b) and (c) go through the census's name and table precedents. Where the census does not
resolve them, seed `Everything`-style for that name or table. Add an oracle-2 case:
`MATCH` over `A:A`, `ROWS(name)` and `ROWS(Table1)` before and after an insert.

### M3. The apply order uses pre-edit child indices, and composing with `CellWritePlan` is unspecified

**Problem.**

- §2.1 step 3 (L221-224) first removes rows and cells on the edited sheet (3.1). Only then does
  it run "cache strips of the Phase1 seeds … on any sheet" (3.3), with "positions validated in
  step 1" (L220).
- Phase1's strip plan stores **child indices** of the tree as it was planned:
  - `StripPlan.sheets: Vec<(String, Vec<(usize, usize)>)>` (`edit.rs` L120-127);
  - `strip_cells` walks `into_child_at::<Row>(ri)` (`edit.rs` L131-136).
- After step 3.1 those indices on the edited sheet point at other rows or cells. The step then
  strips the wrong cells' caches (silent), or fails with `Malformed("planned row")` after
  `Package::apply` (poisoned). "In-place steps cannot refuse" (L234) does not hold for this
  order.
- §3.6.1 (L570-572) and §6 (L780) write table header cells "through the `CellWritePlan`".
  - That plan validates positions against the current tree and carries **its own**
    `RegistryApply`, including `sst` refs and a staged `workbook.xml` (`edit.rs` L106-116, L285).
  - The header cells of a newly inserted table column do not exist before the edit.
  - The G1 plan also stages `sharedStrings.xml` for `count` (L213-214).
  - The ADR does not say whose `sst`/`calcPr` change wins, or how the two registry changes form
    the "one `RegistryPlan` per operation" of §2.1.
  - This repeats the shape of ADR-0017 round-1 B1.
- Staging `sharedStrings.xml` as a "small part" (L213) is also a misnomer. `sst` can be
  megabytes, and Phase2 already applies `count` changes through `RegistryApply.refs_delta`
  (`edit.rs` L442-443).

**Fix.**

- Fix the in-place order and say why it is safe. One option is to do everything that addresses
  existing nodes by index first, in post-order by index: (1) strips; (2) formula text and `f`
  attributes; (3) `r` renumbering. Then do (4) removals in descending index order, and last
  (5) insertions of new header cells.
- Alternatively, have step 1 compute positions in post-edit coordinates and state that.
- Define how G1 and `CellWritePlan` compose: a `plan_writes` variant that takes an index map
  (old → new) and contributes its `sst`/`calcPr`/calcChain changes to the operation's single
  plan.
- Or (simpler for P7b) refuse inserting columns inside a table until that exists. This is
  Q11's alternative, and I recommend it.
- Route `sst/@count` through the Phase2 registry, not a staged clone.
- Fault injection (oracle 8) must sit between steps (1)…(5) on a fixture where strips and
  removals share a sheet.

### M4. The closed world for unprefixed positions is not defined, and extension places are outside every test

**Problem.**

- §2.2 (L255-259): for G1 the scan looks for "any reference token in the edited sheet's own
  parts". An unprefixed A1 token cannot be recognised in arbitrary XML text.
  - GUIDs contain cell-shaped chunks (`{9B4F274F-F774-…}` has `F774`). So do `xr:uid`, `x14:id`
    and colours.
  - The rule as written either refuses most sheets or has to whitelist so much that it misses
    real places.
- The schema completeness test reads only `sml.xsd` (L260-265). `reference/` bundles no
  x14/x15/xr schemas: a grep of `reference/**/*.xsd` finds no `xm:sqref`. So extension places
  are covered by neither the XSD test nor a well-defined scan. Places that hold unprefixed
  positions and are absent from the registry:
  - `x14:protectedRanges/x14:protectedRange/xm:sqref`;
  - `x14:ignoredErrors/x14:ignoredError/xm:sqref`;
  - x15 `webExtensions` bindings (`xm:f`);
  - `x15:timelineRefs`/`x14:slicerList` (relationships only; fine, but unlisted).
  - These come from MS-XLSX's worksheet extension list, cited from memory and not re-fetched in
    this review. None of them is in the corpus. The corpus' worksheet ext URIs are only mac
    `PLV` (29), CF (2), DV (3), sparklines (1) and the cfRule `x14:id` link (2).

**Fix.** Define the closed world by **element**, not by token:

- For every part the operation edits, every `ext/@uri` (at any depth) must be either a registry
  row or a `NOT_APPLICABLE` entry with a reason. An unknown URI on a part the operation must
  change → `UnrewritableReference { part, location: uri }`.
- Keep the token scan only for the sheet-prefix form, where ADR-0009's scan is proven.
- Add a hand-written `EXTENSIONS` table (URI → rows), with an ADR ↔ data test like L266. Rows:
  protectedRanges and ignoredErrors with `xm:sqref`, `Sqref`, D/R like L23/L24;
  `webExtensions`: X.
- Record the mac `PLV` URI as `NOT_APPLICABLE`.

### M5. `copy_sheet` duplicates `sheetPr/@codeName`

**Problem.** §4.2 (L692-699) starts from the source's bytes and lists what changes: `r:id`,
`tabSelected`, `xr:uid`, formulas. `sheetPr/@codeName` is kept.

- ADR-0009 L437 decided that a new sheet gets **no** `codeName`, so that Excel creates the
  document module.
- A duplicate `codeName` in a package with `vbaProject.bin` names one VBA document module for
  two sheets. We cannot update that binary.

**Evidence (probe).** 92 sheets in 31 packages have `codeName`; 33 of those sheets are in the 8
packages with a VBA project. All of them count as "copyable" in the 773.

**Fix.**

- Remove `codeName` from the copy, as ADR-0009 does for new sheets, and add it to oracle 1's copy
  row.
- In a package with a VBA project, either refuse (`SheetHasUnsupportedParts`, fail-closed, until
  an XL check shows Excel adds the module), or make it Q13 for the owner.
- Add XL item 13: an xlsm copy opens with macros intact.

---

## Minor

- **m1 (blocker counts and X/XX contradictions).**
  - "798 of 813 (98%)" (L103) and "2% … refuse every G1 edit (pivots, controls,
    `dataConsolidate`)" (L934) do not match the registry:
    - pivots are X only when crossed or moved (L52, §3.6.5);
    - `oleObjects`/`controls` are **XX** (L30; 6 in 5 files) but not counted;
    - nor are the 15 sheets without `r` or the shared workbook.
  - L27 (`scenarios`) and L29 (`webPublishItems`) are **X** in the table but "refused when
    present" in §3.5 (L557). Pick one.
  - Recompute the spike's `sheet_no_blocker` from the registry's XX set.
- **m2 (shared-group simulation: what it measures).**
  - The spike inserts or deletes only in the middle of each group (`simulate_shared`, L438-469).
    Probe: every position from the lowest referenced row to the last member + 1 (41,910 insert
    and 41,616 delete cases) gives the **same 6 groups** split. Conclusion 4 holds; say so.
  - But with the spike's shortcut "any `#REF!` in the master ⇒ split" (L462), the all-positions
    run splits **74 of 294** groups on delete. An example is deleting the header row that
    `$A3*C$2` refers to: every member becomes `#REF!` alike, and the group can stay whole.
  - §3.3.2 step 3 must state that `#REF!` tokens compare equal under translation (ADR-0003 §4.2
    must translate them, not refuse). Otherwise deleting a header row splits a quarter of the
    groups.
  - The simulation also skips self-prefixed references (L394) and column edits. Cross-sheet
    shared groups are 0 in the corpus (probe: no shared master text contains `!`); note that
    they split at `p`.
- **m3 (`hyperlink/@location` left stale).** "a deleted target is left as written and reported"
  (L548, L14) means `Sheet1!A5` now points at the old row 6. That is the silent stale pointer
  the ADR rules out. ADR-0009 §5 left the text only because the target *sheet* no longer
  exists. Clamp it as for views, or refuse.
- **m4 (CF/DV anchor with multi-range `sqref`).** §3.3.4 (L499-501) takes the top-left of the
  **first** range. With 11 multi-range CF `sqref`s and 6 multi-range DV `sqref`s in the corpus,
  the bounding-box top-left is an equally plausible reading. Excel normalises to it on save as
  far as I recall; LO's OOXML import source was not in the downloaded set, so this is not
  verified. Until XL item 6 settles it, refuse re-anchoring a multi-range `sqref` whose formula
  has relative references.
- **m5 (sml.xsd places missing from the registry).** The count of 56 is right (ST_Ref 22,
  ST_CellRef 17, ST_Formula 9, ST_Sqref 7, ST_RefA 1). Mapping them all, these have no row and
  are not named as `NOT_APPLICABLE`:
  - `CT_SheetPr/@syncRef` (xsd L2314);
  - `CT_OleSize/@ref` (workbook `oleSize`, L4369; **2 packages** in the corpus);
  - `CT_VolTopicRef/@r` (`volatileDependencies.xml`, L4079);
  - `CT_CustomSheetView/@topLeftCell` (L2569; implied by L25, but name it);
  - `brk/@min`/`@max` (ST unsignedInt, but positions on the other axis, e.g. `max="16383"`; see
    M1);
  - workbook `webPublishObjects`.

  The completeness test would catch the ST-typed ones. Decide their rules now so that P7-pre
  does not stall.
- **m6 (allowed-diff gaps).** Beyond B1:
  - `docProps/app.xml` when a `Print_Area`/`Print_Titles` name is removed (Q12; `TitlesOfParts`
    lists them);
  - removal of a `comments` or VML part (with its rel and CT) when the last note goes;
  - `codeName` (M5);
  - an empty `extLst`/`ext` removed by the container rule on **other** sheets.
- **m7 (core feasibility).** The approach is feasible on the current core, with two caveats:
  - `write.rs` L21-63 copies children's spans under a re-rendered tag, as §3.8 says.
  - Attribute spelling is preserved only after `LensMut::preserve_attribute_spelling()` is
    called on that node (`lens.rs` L1235; `node.rs` L68). §3.8 should state that every
    renumbered element opts in.
  - There is no public bulk child removal: `LensMut::remove_at` is a `Vec::remove`
    (`node.rs` L96). Deleting `k` of `n` rows is therefore O(k·n). For example, 100k rows at row
    2 of a 1M-row sheet is ~10¹¹ moves.
  - Add a public `retain`/`remove_range` on `LensMut` (the crate-internal `splice_children` and
    `retain_children` exist, `node.rs` L100-106) to P7-pre, with a criterion case for
    `delete_rows(1, 100_000)`.
- **m8 (seed over-approximation).** "every formula cell that moved" (L637) strips nearly every
  cache on an insert near the top: 11,149 normal + 19,558 shared formula cells in the corpus.
  It also contradicts oracle 9's "values match" for formula cells. Seed only formulas whose
  tokens are position-dependent (`ROW()`, `COLUMN()`, `CELL(…)` without a reference, and
  `INDIRECT`/`OFFSET` already `Everything`), plus M2's sets.
- **m9 (Q12 is not fail-closed).** Dropping a union member and removing the name contradicts
  "defaults refuse rather than guess" (L938-939) and the names rule two lines above. Excel's
  form is *(u)*, and the corpus has 13 `S!#REF!` names. Default to `Sheet!#REF!` like other
  names (§3.3.5), or refuse, and move Q12 to "Need the owner's decision".
- **m10 (owner decisions not surfaced).**
  - Sheet protection: probe found 11 protected sheets (5 packages), 7 with `insertRows` locked
    (4 packages), and `lockStructure` in 4 workbooks (relevant to `copy_sheet`).
  - Whether the library honours these locks is a policy choice. Neither this ADR nor ADR-0009
    states it. Add Q13, with refusal (`Protected`) as the proposed default.
- **m11 (dependencies stated as available).**
  - §3.6.4 (L611) says "ADR-0017 §7.2's explicit refresh is available", but P5e is not
    implemented.
  - Sparkline removal (§3.5) and dynamic-array anchors (§3.3.3) are P5d territory. Say that
    P7a uses only ADR-0014's container rule and refuses `cm` anchors.
  - For P7d, the closed set of **drawing and chart** relationships is unspecified:
    `themeOverride`, the embedded `package` (`c:externalData`), user-shape images, SmartArt's
    five diagram parts and drawing hyperlinks. Give the table rows, or "any other: refuse".
- **m12 (VML move/size flags).** §3.6.3 (L598-599) uses `x:MoveWithCells`/`x:SizeWithCells`
  without stating their polarity. Their naming is a known trap: presence vs value, and Excel's
  "Don't move or size" default for notes. Cite the spec section and add an XL item.
- **m13 (table rename and the data model).** Data-model sources name tables in
  `connections.xml` (`x15:rangePr/@sourceName`, `WorksheetConnection_…!Table1`). Add a row
  (refuse) rather than rely on the scan. Also, L55 says "–" for renames while §5 (L756) lists
  connection parameters among the rewritten places.
- **m14 (merge anchor hand-off).** "non-anchor cells are empty (Phase0 guard)" (L547) holds only
  for our own writes. Third-party files can carry values there. Check at validation, and refuse
  or report when the new anchor is non-empty.

## Nit

- **n1.** L40, Copy "N (§4.3)": §4.3 renumbers shape ids, not `x:Row`/`x:Column`. It should be
  "copied".
- **n2.** Oracle 1, copy row "— (source is L1)": L1 is a registry id. Write "source unchanged
  (§4.5)".
- **n3.** L9's 81 / 53 counts `sheetView/@topLeftCell` only; `pane/@topLeftCell` is not counted.
- **n4.** §2.2 "there is no third state" (L252-253), but the rule column has R, D, X, XX, N,
  copied and "part removed".
- **n5.** Fact 5 says "14 (cells and names)". The spike also finds 1 sheet-prefixed `#REF!` in
  x14.
- **n6.** `lexer.rs` L28-29's `Area::RefError` is the **prefixed** form only. Bare `#REF!` is
  an error literal. Reword L429.
- **n7.** L10: non-frozen split panes keep `xSplit`/`ySplit` in twips. State that they are
  unchanged.

---

## Recommendations (Q1–Q12)

| Q | Recommendation |
|---|---|
| Q1 | **Default as proposed** (blank; split `col`). Copying the neighbour's format can come later without breaking anything. |
| Q2 | **Refuse, as proposed.** |
| Q3 | **As proposed**, both as XL items. |
| Q4 | **Not duplicated**, but record that Excel is widely reported to create local copies (XL 12). |
| Q5 | Acceptable as a default; verify with XL. |
| Q6 | **As proposed.** |
| Q7 | **Refuse, as proposed.** `refreshOnLoad` changes behaviour on open. |
| Q8 | **Refuse, as proposed.** Apply the same to sparkline data ranges (B1). |
| Q9 | **As proposed.** Add m2's `#REF!` equality. Optionally split into two groups at `p` rather than writing members out. |
| Q10 | 2× open+save is reasonable **for an insert**. Add a delete-100k-rows case (m7) with its own target. |
| Q11 | **Refuse until an XL check** (M3). This removes the `CellWritePlan` composition from P7b. |
| Q12 | Do **not** default to dropping. Use `#REF!` or refuse (m9), and move it to the owner's list. |
| Q13 (new) | Sheet protection and `lockStructure`: refuse by default (m10). |

## Additions to the test and delivery plans

- Oracle 1:
  - B1's other-sheet places;
  - `codeName` (M5);
  - m6's parts.
- Oracle 2:
  - every `any-part` row on a second sheet (B1);
  - sticky-end ranges and `col max=16384` (M1);
  - `MATCH(…,A:A,0)`, `ROWS(name)`, `ROWS(Table1)` cache seeding (M2).
- Oracle 8: fault injection between each of M3's ordered in-place steps, on a sheet where
  strips and removals coincide.
- Oracle 7 explicit cases:
  - a package with `col max="16384"` (any of the 43);
  - an xlsm with `codeName` (M5);
  - `NewStyleConditionalFormattings.xlsx` (x14 link ext);
  - a cross-sheet x14 DV/CF file (`x14.ref_sheet_prefixed`, 2 files).
- Spike:
  - all-position shared simulation with `#REF!` equality (m2);
  - G1 blocker count from the registry's XX set with M1's clip rule (m1);
  - ext URI census per part (M4).
- Delivery:
  - P7-pre gains the `EXTENSIONS` table (M4) and the bulk-removal lens API (m7);
  - P7a must include B1's cross-sheet rows;
  - P7b drops table column insert per Q11.

## Verdict

Accepted after Minor only: **no** (1 Blocker, 5 Major). Revision 2 should address B1 and M1–M5,
and round 2 should follow.
