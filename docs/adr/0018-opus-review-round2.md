# ADR-0018 review (Opus, round 2)

対象: plan/xlsx-phase7 @ a200174d031b21d7033ebbabbb89c25243a5fa90

Scope: revision 2 of `docs/adr/0018-xlsx-phase7-structural-edits.md` and `spikes/xlsx-phase7/`, against
[round 1](0018-opus-review-round1.md). Read against `origin/main` @ `1000645`: `edit.rs`, `graph.rs`,
`formula/translate.rs`, `sheet_ops/scan.rs`, `tables.rs`, ADR-0009 §6, and `sml.xsd`.

Spike: I reran `python3 -I spikes/xlsx-phase7/structural_survey.py --summary /tmp/poi-xlsx
reference/fixtures/xlsx/calamine`. All new numbers reproduce:

- 787 sheets without an XX place;
- `sim_shared_all_{ins,del}` 286 same / 6 split;
- protection 11 sheets, 7 of them locked, `lockStructure` in 4 workbooks;
- `codeName` on 93 sheets (32 files), 33 of them in VBA packages;
- the extension-URI census.

The new all-positions simulation carries the master hand-off and `#REF!` equality, and it
matches my round-1 probe.

MS-XLSX: I fetched the worksheet extension table ([MS-XLSX] §2.2.4.12 "Worksheet",
learn.microsoft.com `07d607af-5618-4ca2-b683-6a78dc0d9627`, updated 2026-11-18). M2 quotes it.

## Conclusion first

Round 1's findings are resolved in substance:

- B1: the scope column, other-sheet apply and the allowed-diff rows are in place.
- M1: sticky ends and clipping follow LO.
- M2: one strip rule.
- M3: a fixed apply order and the composed `CellWritePlan`.
- M5: `codeName` is dropped on copy.
- M4: the element-based closed world is the right shape.

The owner's decisions Q1–Q4, Q7, Q10, Q11 and Q13 are applied in §2–§8 and in the oracles, with
the small inconsistencies listed in m7 and m8. Q12 now fails closed.

The revision still leaves three Majors. They are new or residual, and each has a small fix:

1. **M1:** The scope rule is not applied to the X/XX formula rows. The prefix scan also accepts a
   hit in any registry place, whether or not that place was dispatched. B1's silent-stale path
   is therefore still open for form controls, chartEx, ctrlProps and webExtensions on other
   sheets.
2. **M2:** Two `EXTENSIONS` GUIDs are wrong. MS-XLSX maps `{01252117-…}` to **ignoredErrors**,
   not protectedRanges. Also, content under a *known* URI that matches no row is not caught.
   So "a wrong URI fails closed" does not hold.
3. **M3:** The registry is not a superset of ADR-0009 §6's table. It drops the internal
   relationship targets `#Sheet!A1`, which are positions. It also has no sheet-rename/delete
   rule column, yet P7-pre switches ADR-0009's scan to `LOCATIONS`.

**Verdict: no** (0 Blocker, 3 Major, 8 Minor, 2 Nit).

## Status of round 1

| Finding | Status | Note |
|---|---|---|
| B1 | **Resolved, with a residual (M1)** | Rows L4, L14, L17, L19–L21, L37, L46, L47 and L55 are `(any)`. §2.1 step 3.2 and oracle 1 include other sheets. Oracle 2 has second-sheet cases. The `(any)` marks stop at the rows whose rule is R/D. |
| M1 | **Resolved** | §3.2 table (L621-623) and §3.4. It matches `refupdat.cxx` L270-276 and `token.cxx` L3044. Edge precedence: m1. |
| M2 | **Resolved** | §3.7 is computable on the current core. `Graph` stores every precedent as a resolved `Rect`, names and tables included (`graph.rs` L41-56, L100-114). The rule is `Graph::dependents(census, vec![whole edited sheet])` plus the `Everything` closure (`graph.rs` L119-142). See m2 for `ROW()` detection. |
| M3 | **Resolved in substance** | §2.1 order and §2.1.1. The invariant as stated is false in three places, and the duplicated work is listed in m3 and m4. |
| M4 | **Partly resolved (M2)** | The element-based scan, the GUID exclusion and the `EXTENSIONS` table are sound in shape. Its contents and one rule are wrong. |
| M5 | **Resolved** | §4.2, L66, oracle 1 copy row, XL 13. |
| m1–m14, n1–n7 | Resolved, except where noted below | m3 (hyperlink `location`) → refusal; m9 → Q12 refuses; m12 → notes only; m14 → refusal. |
| Declined m2 (two-group split) | **Accepted** | 6 corpus groups; writing members out is correct and simpler. |
| Declined Q11 advice | **Accepted** | The owner decided. §2.1.1 specifies the composition, and oracle 8 covers a refusal raised by the composed plan. |
| Declined m4 alternative | **Accepted, but the justification is wrong (m5)** | |

---

## Major

### M1. The X/XX formula rows have no `(any)` scope, and the prefix scan accepts any registry place

**Problem.**

- §2.2 (L380-386) says "Every row whose syntax can hold a sheet-prefixed reference (`Formula`,
  `Location`) is **`any`**".
- The table does not mark these `Formula` rows `(any)`:
  - L30 (`controlPr/@linkedCell`, `@listFillRange`);
  - L42 (VML `x:FmlaLink`/`x:FmlaRange`/…);
  - L48 (chartEx `cx:f`);
  - L56 (ctrlProps `@fmlaLink`/`@fmlaRange`);
  - L70 (webExtensions `xm:f`).
- The ADR ↔ data test (L429-430) makes the table the source of truth. These rows are therefore
  dispatched only for the edited sheet's own parts.
- Example: a check box on `Summary` linked to `Data!$B$3` is not refused by an insert into
  `Data`.
- The sheet-prefix scan (L398-401) then finds `Data!` in a registry place and accepts it.
- These rows are X/XX, so they are meant to *refuse*. Here they neither refuse nor rewrite: the
  link goes silently stale. This is B1's failure mode for the rows B1's fix skipped.
- ADR-0009 §6 lists exactly these places for sheet renames: "form controls: ctrlProps
  `@fmlaLink`…; VML `x:FmlaLink`…; `controlPr/@linkedCell`". The cross-sheet case is real
  there.

**Corpus.** The corpus has no such link today:

- every `x:Fmla*` text is a `FmlaMacro` macro name (n1);
- ctrlProps formulas are empty.

That is why the spike cannot show the gap.

**Fix.**

1. Mark L30 (its `Formula` attributes), L42, L48, L56 and L70 `(any)`. For X rows the `any`
   dispatch means "refuse if a token names the edited sheet and would change".
2. Change the scan's acceptance rule: a prefix hit is accepted only in a registry place **that
   the dispatcher visited for that part with a rule other than `–`**.
3. Have the ADR ↔ data test assert that every `Formula`/`Location` row has scope `any`. Do this
   as a compile-time `const` check rather than by convention.
4. Add oracle 2 cases: a VML control and a ctrlProp on a second sheet linked to the edited
   sheet must refuse.

### M2. `EXTENSIONS`: wrong GUIDs, missing URIs, and known-URI content that matches no row

**Problem.** MS-XLSX §2.2.4.12 lists the worksheet `extLst` URIs as follows:

| URI | Element (MS-XLSX) | ADR-0018 L437-445 |
|---|---|---|
| `{78C0D931-6437-407D-A8EE-F0AAD7539E65}` | conditionalFormattings | ✓ |
| `{CCE6A557-97BC-4B89-ADB6-D9C93CAAB3DF}` | dataValidations | ✓ |
| `{05C60535-1F16-4FD2-B633-F4F36F0B64E0}` | sparklineGroups | ✓ |
| `{A8765BA9-456A-4DAB-B4F3-ACF838C121DE}` | slicerList | listed without a URI |
| `{FC87AEE6-9EDD-4A0A-B7FB-166176984837}` | **protectedRanges** | **missing** |
| `{01252117-D84E-4E92-8308-4BE1C098FCBB}` | **ignoredErrors** | **mapped to protectedRanges (L68)** |
| `{F7C9EE02-42E1-4005-9D12-6889AFFD525C}` | webExtensions | listed without a URI |
| `{7E03D99C-DC04-49d9-9315-930204A7B6E9}` | timelineRefs | listed without a URI |
| `{EB79DEF2-80B8-43E5-95BD-54CBDDF9020C}` | — (not a worksheet URI) | mapped to ignoredErrors (L69) |

The same page also extends:

- `cfRule/extLst` with `{B025F937-…}` (✓);
- `sortState/extLst` with `{0FC57C36-975A-43D2-8416-536B9B585D1B}` (richSortCondition);
- `filterColumn/extLst` with `{68452572-C921-441B-96FD-77FB3E787301}`.

Those last two are not listed.

L447-449 says "A wrong URI fails closed, as an unknown URI". That is true for a *missing*
URI. It is false for a *swapped* one:

- Under rev 2, `{01252117-…}` is known.
- Its real content (`x14:ignoredErrors/x14:ignoredError/xm:sqref`) does not match L68's path
  (`x14:protectedRanges/…`).
- The scan recognises unprefixed positions **only in registry places** (L402-407), and
  reference-shaped content refuses only under an **unknown** URI (L411-413).
- The `xm:sqref` is therefore neither rewritten nor refused.

The same silent path opens whenever Microsoft adds a reference-bearing child under an existing
URI, such as a new x14 CF feature.

**Fix.**

1. Replace the table with MS-XLSX's list above, cite the page, and drop the *(u)* marks.
   - L68 = `{FC87AEE6-…}`, L69 = `{01252117-…}`.
   - Give webExtensions, slicerList and timelineRefs their URIs.
   - Add the `sortState` and `filterColumn` ext URIs, and refuse them while they hold a `ref`.
2. Apply the reference-shape test to **known** URIs too. Inside any `ext`, an element or
   attribute named `f`, `sqref`, `ref` or `r` that is not a registry place of that URI refuses
   (`UnrewritableReference { part, location: uri + path }`).
3. Add a unit case: the `ignoredErrors` content under its real URI must be rewritten (L69), and
   under a deliberately swapped mapping must refuse.

### M3. The registry is not a superset of ADR-0009 §6's table, although P7-pre makes ADR-0009 use it

**Problem.**

- §2.2 says the registry "extends ADR-0009 §6's table" (L373-374).
- P7-pre has "the completeness scan of §2.2 reading [LOCATIONS]" replace `sheet_ops/scan.rs`
  for ADR-0009's rename and delete (L1221-1224).
- But ADR-0009 §6's table (ADR-0009 L540-616) has rows with no counterpart among L1–L70:
  - **`#Sheet!…` targets of Internal relationships** in sheet and drawing `.rels` (shape
    hyperlinks), rewritten today with `set_relationship_target`. ADR-0009 counts 18 of 838
    corpus renames. They are **positions** (`#Data!A5`), so G1 must rewrite them with scope
    `any`, or refuse.
    - As written, the prefix scan hits them, finds no registry place and refuses G1 on those
      sheets. That is the "unknown place" refusal, not a decided rule.
    - Once `allowed()` (`scan.rs` L54-72, including the `Relationships`/`Target` arm) is
      replaced, **sheet rename regresses** from rewrite to refusal.
  - `rangeSet/@sheet` and `dataRef/@sheet` (raw names). L53 and L28 carry only `@ref`.
  - `app.xml` worksheet titles and "Named Ranges" titles (`'1'!Print_Area`), which ADR-0009
    rewrites. ADR-0018 lists `docProps` as a known non-reference (L549-550), which contradicts
    that.
- The registry has columns G1, Copy and table/column rename. It has **no sheet-rename and no
  sheet-delete column**. ADR-0009's operations cannot read their rule from it.

**Fix.**

1. Add rows:
   - L71: relationship `Target="#Sheet!ref"` (sheet and drawing `.rels`), `Location`, scope
     `any`. G1: R (through `set_relationship_target` in the operation's `RegistryPlan`); X when
     the target is deleted, like L14. Copy: kept.
   - L72: `@sheet` raw names (`worksheetSource`, `rangeSet`, `dataRef`).
   - L73: `app.xml` titles and named-range titles. G1: – ; sheet rename: R.
2. Add the columns **sheet rename** and **sheet delete**, filled from ADR-0009 §6.
3. Require an ADR-0009 ↔ `LOCATIONS` test: every place `scan.rs::allowed()` accepts today maps
   to a row whose rename rule is R.
4. Make P7-pre's acceptance criterion "ADR-0009's corpus measurement is unchanged: 0 refusals in
   838 renames" (ADR-0009 L617).

---

## Minor

- **m1 (edge precedence and views).**
  - §3.2 needs one sentence on precedence: **wholly deleted wins over pinning**.
    - Example: deleting rows `p..max` under `A1048570:A1048576` gives `#REF!`/removal, not a
      pinned range. LO marks this `UR_INVALID` before applying the sticky end
      (`refupdat.cxx` L252-266).
    - Otherwise `A2:A1048576` with `p ≤ 2` and the deleted band reaching the edge is ambiguous.
  - "a single cell … pushed past the edge: refused" (L623) also catches `activeCell`/
    `topLeftCell` (L9, L11). Those are clamped on delete, so clamp them on insert as well. The
    corpus has 0 at the edge, but a user who saved with the cursor on the last row would
    otherwise block every row insert.
- **m2 (strip rule details).**
  - The `ROW`/`COLUMN`/`CELL`/`ADDRESS`-without-argument test (L891-893) cannot come from the
    census, which has no function flags. The pre-filter (§3.3.1) skips lexing a formula whose
    precedents miss the edited sheet, yet `=ROW()` has no precedents.
  - State that every formula cell on the edited sheet at or after `p` is lexed for these calls.
    That is cheap: one lex per moved formula.
  - §3.7's last bullet applies "the same rule" to renames, which strips every formula using the
    table. §5 (L1059-1060) says renames seed only `Everything`. Pick one; §5's is correct,
    because names do not change values.
  - For copies (§4.5), say explicitly that stripping the copy's own formulas is a deliberate
    over-approximation.
- **m3 (the apply invariant as stated is false).**
  - "Steps 1–3 … do not add or remove children" (L311) is broken three times:
    - strips remove `v`/`is`;
    - a shared member leaving its group gains a text child;
    - **the `cols` split of an insert (Q1) adds a `col` element in step 3.2** (L301).
  - The first two are harmless, because recorded indices address `sheetData/row` and
    `row/c` only.
  - Restate the invariant: "no child is added to or removed from any element whose children
    are addressed by a recorded index".
  - Move the `cols` split to step 3.5.
  - Add oracle 10: a mutation that moves a strip after step 3.4 must fail.
- **m4 (work already on main).**
  - The ADR presents §2.1.1's folding of `calcPr` and calcChain into the caller's
    `RegistryPlan` as new. It exists: `apply_writes_with` and `apply_writes_with_insertion`
    (`edit.rs` L470-489, used by `tables.rs` L468, L661).
  - That code stages the `CellWritePlan`'s **own** `workbook.xml` clone (`edit.rs` L479-481). A
    G1 plan that also stages `workbook.xml` (names) would stage the part twice. The real P7-pre
    work is to merge `calcPr` into the operation's clone and to add the index map. Say so.
  - "`#REF!` pass-through in ADR-0003 §4.2's translation" (Delivery, L1227) is already done:
    `translate.rs` L47 keeps `Area::RefError`, with a test at L102. Bare `#REF!` is an error
    literal and is copied. Drop it from P7-pre, or reword it as a test.
  - Also state that the composed plan's dependent strips are subsumed by §3.7's rule. Its
    `dependents_of_seeds` would otherwise be asked about post-edit cells against a pre-edit
    graph.
- **m5 (declined m4: the justification).**
  - "both readings agree while the anchor survives" (L738-739) is true only while **both**
    candidate anchors survive.
  - Example: with `sqref="B5:B10 A3:A4"`, deleting row 3 removes the bounding-box anchor `A3`
    and keeps the first-range anchor `B5`.
  - Rev 2 rewrites positionally, as if the anchor survived. That is wrong under the
    bounding-box reading.
  - Refuse when **either** the first range's top-left **or** the bounding box's top-left is
    deleted (multi-range `sqref`, relative references). The decision to defer to XL 6 then
    holds.
- **m6 (`mc:AlternateContent` inside range features).**
  - MS-XLSX §2.2.4.12 extends `sortState` and `filterColumn` with `mc:AlternateContent`. The
    Choice holds x14 `sortCondition` (with `@ref`), `iconFilter` and `customFilters`.
  - L31 covers AC only at the top level, in `sheetData` and in rows. The scan tokenises only
    registry places. An AC Choice `sortCondition/@ref` would therefore stay stale while its
    Fallback is rewritten.
  - The corpus has no AC there (probe: sheet AC parents are `controls` 164, `oleObjects` 7 and
    the top level 2).
  - Add to L31: AC anywhere inside a registry element of an edited part → X.
- **m7 (Q2 consistency).**
  - §8 `ObjectInDeletedRange` still reads "an anchor in deleted rows or columns" (L1107). Since
    Q2 it means "an object fully inside the deleted band".
  - The partial-delete rule (marker → first surviving row, offset 0; L838-839) is undefined for
    `editAs="oneCell"`/`oneCellAnchor`, where `to` "moves by the same amount", and for a note's
    `x:Anchor` corner in the deleted band. Pick a rule (refuse, or the same clamp) for each.
- **m8 (Q13 consistency).**
  - `copy_sheet` refuses under `lockStructure`, but ADR-0009's implemented add, delete, move
    and rename sheet ignore it (no `lockStructure` in `sheet_ops/`). Either extend Q13 to them,
    as a P7-pre item that changes ADR-0009's behaviour, or record why copy differs.
  - P7d's delivery item does not mention the `lockStructure` refusal.
  - State whether `rename_table_column`'s header write on a protected sheet with a locked
    header cell is refused. Q13 currently covers only G1 and copy.

## Nit

- **n1.** L42 corpus "57 / 4": all 57 are `x:FmlaMacro` macro names (`[0]!Checkbox_Int_Click`,
  `mv-calculator-final-2-20-2013.xlsm` etc.). `FmlaMacro` is not among L42's elements. The
  spike's regex (`structural_survey.py` L802) counts any `x:Fmla\w+`. The count for L42's
  elements is 0.
- **n2.** Sources (L1359-1360): cite the MS-XLSX page above instead of "URIs as recalled".

## Owner decisions: consistency check

| Q | Applied consistently? |
|---|---|
| Q1 | Yes (§3.4, §1, Q1). Step placement of the `cols` split: m3. |
| Q2 | Mostly. §8 wording and the oneCell/note corners: m7. |
| Q3 | Yes: §4.4, L21/L47 copy column, XL 12. |
| Q4 | Yes. |
| Q7 | Yes (§3.6.5, L50). |
| Q10 | Yes (§3.8, reported, not gated). |
| Q11 | Yes: §3.6.1, §2.1.1, P7b, oracle 8. m4 has the duplicated-staging detail. |
| Q12 | **Fail-closed** (§3.3.5 L754-757, §8, Q12). |
| Q13 | G1 and copy yes (§2.4, L67, §4.1, §8, oracle 7). ADR-0009's operations and the column-rename header: m8. Attribute defaults match `sml.xsd` L2893-2903 (`sheet` default false; `insertRows`, … default true = locked). |

## Verdict

Accepted after Minor only: **no** (0 Blocker, 3 Major). M1–M3 are small, mechanical fixes to
the registry's data and to one scan rule. Revision 3 should address them, and round 3 can be
limited to M1–M3 and their tests.
