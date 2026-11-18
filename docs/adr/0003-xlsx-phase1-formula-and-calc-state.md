# ADR-0003: xlsx Phase1 — formula-aware edits and calculation state

- Status: **Accepted** (revision 5; review round 4: Accepted after the Minor fixes below)
- Date: 2026-11-16
- Depends on: [ADR-0001](0001-lossless-round-trip-architecture.md),
  [ADR-0002](0002-schema-driven-code-generation.md)
- Roadmap: [`docs/roadmap/xlsx.md`](../roadmap/xlsx.md) §4 (edit hazards), §6 (why this is
  Phase1; the owner accepted this ordering on 2026-11-16)
- **Post-acceptance addendum (2026-11-16, P5-pre; requested by ADR-0017 §2.1 and Q7):** §3.1a
  (multi-cell write plan, its registry-transaction form, `Worksheet::write_cells`) and §4.4
  (future-function file form in `set_formula`). §3.1's single-cell edit is now the one-cell case
  of §3.1a; its behaviour and output are unchanged.

## Revision history

- **Revision 1**: initial draft.
- **Revision 2** (after `0003-opus-review-round1.md`):
  - **B1:** the dependency rule is now sound by default. A formula gets a precise precedent set
    only when every token in it is on an allowlist of constructs whose reads are fully visible in
    the text. Anything else depends on everything. The six counterexamples (a)–(f) are now
    explicit rules (§2.3).
  - **B2:** three oracles that do not consult the implementation's own graph (§7.1):
    - O1: hand-written expected dependent sets on synthetic workbooks;
    - O2: a perturbation oracle with an independent test-only evaluator;
    - O3: a recalculation oracle with committed values recalculated offline.
    Mutation testing of the rules is added as well.
  - **M1:** `remove_part` is stated as a narrow exception to ADR-0001's append-only invariant,
    with preconditions and whitespace handling (§4.3).
  - **M2:** plan → validate → apply, with staged copies of the touched parts, so an `Err` never
    leaves a partial edit (§3.1).
  - **M3:** shared-master promotion defines the new `ref`, the `Err` conditions and
    out-of-bounds translation; its oracle no longer relies on calamine alone (§4.2).
  - **Minor findings:** m1–m9 are folded in where they apply, each marked inline.
  - The guard table follows the Phase0 owner decisions and coordinator defaults: the first
    visible worksheet, and refusal of every cell of a merged range (relaxed by ADR-0014 §3 for
    the anchor: see the guard table).
- **Revision 3** (after `0003-opus-review-round2.md`; implemented in the same milestone,
  `xlsx-phase1`):
  - **R2-M1 (four soundness holes):**
    1. *Spill growth.* A dependent dynamic-array anchor, and any formula cell whose cached value
       is `#SPILL!`, seeds its **potential spill area** (anchor to the sheet's bottom-right
       corner), not its current `ref` (§3.2).
    2. *`LOOKUP` and undocumented reads.* The allowlist criterion is now "shown by evidence to
       read only inside its argument rectangles" (documentation, LibreOffice source, or an XL
       check), not "not documented to resize". The table is explicit, small and committed, with
       the evidence for each entry; `LOOKUP` is not on it (§2.2).
    3. *Computed `sum_range`.* `SUMIF`/`AVERAGEIF` need **literal** rectangles in `range` and
       `sum_range`; a computed reference there gives `Everything`. The general rule for
       reference-returning functions in position-dependent arguments is written down (§2.3 a).
    4. *Name and sheet resolution* is fully specified: sheet-qualified names, `[0]!`,
       case-insensitive sheet and name matching, unknown sheets and 3-D endpoints, reversed 3-D
       ranges, name/reference collisions (`TAX2024`), and `f/@bx` (§2.4).
  - **R2-M2:** O3 runs on the O1 synthetic workbooks and on a fixed-seed sample of O2 workbooks,
    not only on real fixtures. LibreOffice is not available in the development sandbox, so the
    data is produced by a committed owner/CI script (`spikes/xlsx/o3_recalc.py`, plus a manual
    GitHub Actions workflow), and the test that consumes it is `#[ignore]`d with that reason
    until the data is committed. O2's generator covers names, sheet-name spelling and `SUMIF`
    resizing; O1 has the R2-M1 cases (§7.1).
  - **R2-M3:** no per-edit clone of sheet parts. **Validate-then-apply**: the plan proves that
    every fallible step will succeed (the only clone is of the small `workbook.xml`), apply runs
    in place in a fixed order, and a failure inside apply (a bug by construction) **poisons** the
    workbook so that nothing partial can be saved. Measured in §3.1.
  - **R2-m1:** census failures and `r`-less formula sheets refuse edits workbook-wide; validate
    cross-checks the census against Layer 2 (§1, §3.1).
  - **R2-m2:** the public API change is written down (§6): `Worksheet` borrows the workbook,
    `worksheet_by_name`/`worksheet_by_index`, table parts read for every sheet that is opened,
    `UnknownCellMetadata` applies to every edit.
  - **R2-m3:** "first remaining member", the composition argument, quoted sheet names with `''`
    and `!`, function names shaped like references (§4.2).
  - **R2-m4:** iterative calculation refuses edits with dependents; `autoNoTable` is an XL item
    (§3.3, §7.4).
  - **R2-m5:** the roadmap Phase1 section now matches this ADR.
  - **R2-m6:** O3 baseline diff, volatile exclusion, versions recorded, licence note, several
    values per edit; O1 workbooks are produced by a committed stdlib-only Python generator
    (§7.1).
  - **R2-m7:** the claim is scoped to **cell-formula cached values**; chart, pivot and
    external-link caches are out of scope (Consequences).
  - The tokenizer differential against openpyxl is replaced, for this milestone, by committed
    hand-written token expectations plus a parse check over the committed fixtures' formula
    texts and an optional local corpus sweep; the POI-wide differential corpus is deferred
    (Deviations, below).

- **Revision 4** (after `0003-opus-review-round3.md`, the review of revision 3 and of its
  implementation, and after the PR #14 core reviews):
  - **R3-M1 (resolution holes):** three more "ambiguous ⇒ `Everything`" rules in §2.4:
    (a) an unqualified reference or name **inside any defined name**, local or global, depends
    on everything (Excel reads it on the using sheet, LibreOffice relative to the calculating
    sheet); (b) two masters sharing one `si` on a sheet make the whole group `Everything` and
    its hand-off an `Err` (§1, §4.2); (c) sheet names and defined names that **fold alike**
    (a broad Unicode fold, e.g. the Kelvin sign and `k`) are ambiguous and resolve to nothing.
  - **R3-M2 (oracle):** the allowed-diff oracle of §7 is byte-exact: the output must equal the
    input with exactly the allowed edits applied (§7, first paragraph). The reviewer's two
    mutants (strip also removes `s`; strip sets `row/@ht`) now fail.
  - **Minors:** sheet-qualified name/reference collisions fail translation (R3-m1); stored
    formula text never starts with `=` (R3-m2); the workbook is poisoned *before* apply and
    cleared on success, so a panic in apply also poisons, and `package()` is documented as not
    a save path (R3-m3); the cost claim is corrected and the closure of `Everything` formulas is
    cached, with the worst case measured (R3-m4); O3 file names, workflow and volatile exclusion
    match the implementation (R3-m5); regression tests for each probe (R3-m6).
  - **Core API:** §4.3 matches the `remove_part(name, allowed_rel_types)` contract after the PR
    #14 round-1 and round-2 reviews (relationship-type allowlist, any-attribute rule, OPC
    part-name equivalence), so the ADR-0001 interim note's pointer to §4.3 holds.
  - **O3 workflow** is split into its own PR (`ci/o3-libreoffice`, LibreOffice pinned to an exact
    archived release) so that it exists on the default branch. The data was generated with it
    (run 43821205800) and committed before Phase1 merges.

- **Revision 5** (after `0003-opus-review-round4.md`: 1 Major in the implementation, Minors;
  the ADR is accepted with this revision):
  - **R4-M1:** stripping a dependent removes **every** `v` and `is` (schema-invalid duplicates
    included), as the target write does (§3.3). O1 fixture `o1_duplicate_caches.xlsx`
    (duplicate `v` on a plain, a `t="str"` and a CSE result dependent; two `is`).
  - **R4-m4:** §2.4's reason for treating references inside names as `Everything` is corrected:
    O3 observed LibreOffice binding them to the **name's scope sheet**; Excel's behaviour is
    unverified (XL item). The rule is unchanged.
  - **R4-m5:** stale text about O3 and the open questions updated; the open questions are
    closed (below).
  - **R4-m6:** a test injects a **panic** at each apply step and checks that the workbook is
    poisoned afterwards (`catch_unwind`).
  - **R4-m3:** O3's volatile exclusion also scans defined names.
  - **R4-m1, R4-m2, R4-m7** are recorded as known limits (§7.1, §7.4).

## Context

ADR-0001's Phase0 refuses **every** value edit to a workbook in which any sheet part contains a
formula (its R2-M3/R3-M1 fix, matched by QName per its 2026-11-16 note). The reason is sound: a
value edit can leave a formula's cached result (`c/v`) wrong on a sheet that ooxml-rs never
parsed. Readers other than Excel (calamine, pandas, openpyxl with `data_only=True`) then return
that wrong value with no error.

ADR-0001 ends that section with an explicit open decision: "A future phase must pick, and write
down, one of: delete `calcChain.xml` … whenever a formula cell is touched, set `<calcPr
fullCalcOnLoad="1"/>` …, or incrementally maintain the dependency graph." This ADR makes that
decision.

**How much this blocks.** Measured with `spikes/xlsx/formula_stats.py` on Apache POI
`test-data/spreadsheet` @ `47dcff76`:

- **126 of 341** readable Transitional packages (37%) contain formulas.
- Of those, 115 have `calcChain.xml` and 11 do not. The `app.xml` producers of those 11 are
  LibreOffice 4, Apache POI 2, Microsoft Excel 2 (`clone_sheet`, `CVLKRA-KYC_…`), ONLYOFFICE 1,
  and not recorded 2 (review m4).
- Formula kinds: 48 use shared formulas, 6 array formulas, 1 a dynamic array (`c/@cm`, now
  counted by the script).
- Constructs that matter for dependency analysis: 13 use defined names in formulas, 10
  structured references, 23 cross-sheet references, 6 external references `[n]`, 2
  `INDIRECT`/`OFFSET`.

The corpus is a test corpus, so the real-world share of formula-bearing workbooks is probably
higher (inference, not measured).

**What the reference implementations do:**

- **rust_xlsxwriter**:
  - writes formulas with a cached result of 0 unless told otherwise;
  - never writes `calcChain.xml`;
  - sets `calcPr fullCalcOnLoad="1"`, except in manual calculation mode (`src/workbook.rs`);
  - never writes shared formulas.
- **openpyxl**:
  - drops `calcChain.xml` on every save;
  - writes formulas without cached values;
  - in effect sets `fullCalcOnLoad` on every save: it is the `CalcProperties` default and is
    applied even when the loaded `calcPr` lacks it (`workbook/properties.py`, review m4);
  - expands shared formulas into plain ones on read (`worksheet/_reader.py`, `Translator`).
- **umya-spreadsheet** drops `calcChain.xml` and does not write `fullCalcOnLoad`.
- **Apache POI** keeps `calcChain.xml`:
  - it removes an entry when a formula is removed (`XSSFWorkbook.onDeleteFormula` →
    `CalculationChain.removeItem`, which moves an inherited `i` onto the next entry);
  - it promotes the next member of a shared group when its master loses its formula
    (`XSSFSheet.onDeleteFormula`).
- **Excel**:
  - repairs ("Found unreadable content") when `calcChain.xml` lists a cell that holds no formula;
  - opens files without `calcChain.xml` routinely.
- **LibreOffice** (source-checked, review m3):
  - recalculates on load every formula cell without a cached value, even under its default
    "never recalculate" setting for OOXML (`sc/source/filter/oox/formulabuffer.cxx`: `SetDirty`
    plus `ScRecalcMode::ONLOAD_ONCE`);
  - **never reads `fullCalcOnLoad`** (`workbooksettings.cxx` has no reference to it).

Facts from the spec that shape the design (ECMA-376 Part 1, §18.3.1.40 `f`, §18.6 calcChain,
§18.2.2 `calcPr`):

- Only the **anchor** cell of an array formula (`f t="array" ref="…"`) or data table
  (`t="dataTable"`) carries `f`. The other cells of `ref` carry only `v`, which is still a
  computed result. Dynamic-array spills are stored the same way, with `c/@cm` on the anchor
  pointing into `metadata.xml`. **A census of `f` elements alone misses these result cells.**
- A data-table `f` usually has **no text**; its inputs are the attributes `r1`/`r2`
  (`dt2D`, `dtr`) and the formulas on the table's edge.
- A shared-formula group's text is stored only on its master (`t="shared" si="n" ref="…"` with
  text). Members carry `t="shared" si="n"` with no text and are read relative to the master.
  Excel itself writes groups whose master is not the top-left cell of `ref` (`51710.xlsx`:
  master `E60`, `ref="C60:M83"`, 44 such groups).
- `calcChain` entries omit `i` when it equals the previous entry's sheet id.
- `v` is optional under a formula: a formula cell without `v` is valid, and Excel computes it.
- `f/@ca="1"` marks a cell Excel always recalculates (volatile).

## Decision

### 1. Census at open (replaces Phase0's yes/no scan)

At `open`, every worksheet, macro-sheet and dialog-sheet part is scanned with quick-xml events.
No Layer 2 tree is built, and the parts stay `Raw` or dirty-free `Parsed` (ADR-0001 M2). The
scan builds a `FormulaIndex`:

- **Formula cells**: elements whose QName is SML main + `f` (ADR-0001 note). For each: sheet,
  cell ref, kind (`normal`/`shared`/`array`/`dataTable`), `si`, `ref`, `ca`, `bx`, the `c/@cm`
  and `c/@vm` of its cell, its cached value (`c/@t` + `v`), and the formula text. For shared
  members, the text is resolved through the master (§4.2): the **first** master of the `si` on
  the sheet, as LibreOffice does. A sheet with two or more masters for one `si` marks that whole
  group ambiguous: its cells depend on everything and it cannot be handed off (R3-M1 b).
- **Result ranges**: for `array` and `dataTable` anchors, the whole `ref`; every cell in it is a
  *result cell*.
- **`cm` anchors** (review m8): a cell is a dynamic-array anchor only when all of these hold:
  - it has `cm`;
  - its `f` has `t="array"`;
  - `cm` points (1-based) at a `cellMetadata` block whose `rc/@t` names the `XLDAPR` metadata
    type and whose `rc/@v` (0-based) selects a `futureMetadata name="XLDAPR"` block with
    `xda:dynamicArrayProperties fDynamic="1"`. `metadata.xml` is read with the same event
    scan; no allowlist entries are needed for it.

  Its `ref` is its current spill range. Any other `cm` makes **every** edit to that workbook
  return `Err(UnknownCellMetadata)` (R2-m2): metadata Phase1 does not model is not guessed at.
- **Defined names**: `workbook.xml` `definedName` text, with `localSheetId`.
- **Calculation state**: whether `calcChain.xml` is present, and `calcPr`'s attributes.

**When the census cannot be complete** (R2-m1), every edit (value, formula, removal) to the
workbook returns the same `Err`, and reads still work:

- a sheet part cannot be scanned (not well-formed, UTF-16, DTD): `Err(FormulaScanFailed)`, as in
  Phase0;
- an SML `f` element outside `worksheet/sheetData/row/c`: `Err(FormulaScanFailed)`;
- a sheet part that contains formulas has a `row` or `c` without `r` (`59746_NoRowNums.xlsx`
  shape): `Err(MissingReference)`. Formulas located only by position cannot be indexed, and
  their caches could not be removed. A sheet without formulas and without `r` refuses only edits
  to itself (Phase0).

The index lives as long as the `Workbook` and is updated by every call that changes formulas
(§4). Cost: one decompress and event scan per sheet, which Phase0 already pays.

### 2. Precedents: sound by default (review B1)

**Principle.** Every formula has a precedent set `P(f)`. If the analysis cannot show that
`P(f)` covers every cell the formula can read, then `P(f) = Everything`. Precise sets are an
**allowlist**: a formula gets one only if every token is a construct listed in §2.2, and every
rule of §2.3 and §2.4 is applied. "Not proven safe" therefore always means "depends on
everything". It never means "depends on what the text happens to mention".

#### 2.1 Tokenizer

The tokenizer is hand-written in `ooxml-xlsx`. It lexes, it does not evaluate. Its output is a
token stream (references, names, function calls, operators, literals), and a small parser
builds the expression tree that §2.2–§2.4 and §4.2 work on.

- **References** are recognised as whole tokens: `A1`, `$A$1`, `A1:B2`, `A:A`, `$1:$3`, each
  optionally prefixed by a sheet (`Sheet1!`, `'Q1 2024'!`, `'It''s'!`, `'a!b'!`), a 3-D sheet
  span (`Sheet1:Sheet3!`, `'Jan:Feb'!`) or an external book (`[1]Sheet1!`, `'[1]S 1'!`, `[0]!`).
  Quoted sheet names are one unit: `''` is an escaped quote and `!` inside the quotes is part of
  the name (R2-m3).
- **A name-shaped token followed by `(` is a function**, even when it would be a valid A1
  reference (`LOG10(` is column `LOG`, row 10; R2-m3). openpyxl (`FUNC`) and calamine
  (`next_is_paren`) do the same.
- A token that is a valid A1 reference **and** equals a defined name (case-insensitive,
  `TAX2024`) is kept as a reference token with a *collision* mark (§2.4).
- Structured references (`Table1[Col]`, `[@Col]`, `[[#This Row],[A]]`) are one opaque token.
- Space is the intersection operator only between two operands; otherwise it is ignored.
- It never panics (property test and a sweep over every formula text in the committed
  fixtures). Any lexing or parsing failure gives `Everything` for that formula and makes any
  translation of it fail (§4.2).
- Expected tokens for a committed set of formula texts (every shape above, plus the formula texts
  of the committed fixtures) are checked in tests. No Python runs at test time.

#### 2.2 What can yield a precise precedent set

| Construct | Precedents |
|---|---|
| A1 reference or range with literal corners, optional sheet (`Sheet1!A1`, `'My Sheet'!A1:B2`, `$A$1`), whole columns and rows (`A:A`, `1:1`) | That rectangle on that sheet (the formula's own sheet if unqualified); sheet resolution per §2.4 |
| 3-D reference with literal sheet names (`Sheet1:Sheet3!A1`) | That rectangle on every sheet between the two endpoints in workbook order (§2.4) |
| `:` between two literal rectangles on the same sheet (`A1:B2:C3`) | Their bounding rectangle |
| Union (`,` inside parentheses) and intersection (space) of the above | The union of the operands' precedents (a superset for intersection) |
| A **defined name** whose text, analysed by the same rules, is precise, and which has **no relative component** | That name's precedents (resolution per §2.4). Recursion has a depth limit of 16; deeper gives `Everything` |
| A call to a function on the **value-function table** with arguments that are themselves precise, respecting the table's per-argument rule | The union of the arguments' precedents |
| Literals (numbers, strings, booleans, error literals such as `#REF!`, array constants) and arithmetic, comparison, text and `%` operators | Nothing |

The **value-function table** is a committed Rust table (`formula/functions.rs`): built-in
function name → rule, each with its evidence. **Criterion (R2-M1 2):** a function is on the table
only if there is evidence that it reads **no cell outside the rectangles of the references passed
to it** (Microsoft documentation, LibreOffice source, or an XL check). Absence of documented
resizing is *not* evidence. "Not shown" means "not on the table", which means `Everything`.

- **Scalar functions** (math, text, logical, information, date): their arguments are values; a
  reference argument is only dereferenced. Evidence: Microsoft's argument descriptions (numbers,
  text, logical values).
- **Aggregates** (`SUM`, `SUMPRODUCT`, `SUMSQ`, `PRODUCT`, `MIN`, `MAX`, `MINA`, `MAXA`,
  `AVERAGE`, `AVERAGEA`, `COUNT`, `COUNTA`, `COUNTBLANK`, `MEDIAN`, `MODE`, `LARGE`, `SMALL`,
  `STDEV`/`STDEVP`/`VAR`/`VARP`, `RANK`, `COUNTIF`): read exactly the cells of the passed
  ranges. Evidence: Microsoft docs; LibreOffice iterates the passed range (`ScInterpreter`
  `IterateParameters`, `IterateParametersIf` without a third range).
- **`SUMIFS`, `AVERAGEIFS`, `COUNTIFS`, `_xlfn.MINIFS`, `_xlfn.MAXIFS`**: every range must have
  the size of the first; otherwise `#VALUE!`. Evidence: Microsoft docs; LibreOffice
  `IterateParametersIfs` ("All reference ranges must be of same dimension and size").
- **`VLOOKUP`, `HLOOKUP`, `MATCH`**: read inside the passed table/array. Evidence: LibreOffice
  `CalculateLookup` checks the index against the passed range before reading.
- **`SUMIF`, `AVERAGEIF`**: the resizing rule of §2.3 (a).
- **`INDEX`, `CHOOSE`, `IF`, `IFERROR`, `IFNA`, `_xlfn.IFS`, `_xlfn.SWITCH`**: in value position
  their result lies inside their arguments' precedents. When a reference result is used where a
  rectangle's *position* matters, §2.3 (a)/(b) give `Everything`.
- **`ROW`, `COLUMN`, `ROWS`, `COLUMNS`**: read no cell value; their reference argument
  contributes nothing beyond its own precedents (kept, harmlessly).
- **Volatile functions without `ca`** (`NOW`, `TODAY`, `RAND`, `RANDBETWEEN`): no precedents.
  Their caches are stale by nature and no edit makes them more stale.
- **Not on the table**, each a deliberate exclusion: `LOOKUP` (LibreOffice `ScLookup` reads the
  result vector past its end, review R2-M1 2), `OFFSET`, `INDIRECT`, `CELL`, `INFO`,
  `GETPIVOTDATA`, `SUBTOTAL`/`AGGREGATE` (they skip hidden rows and nested subtotals, i.e.
  depend on state outside the cells), the database functions (`DSUM`, …: their criteria ranges
  name columns by header text), every reference-returning `_xlfn.` function (`XLOOKUP`,
  `ANCHORARRAY`, `SINGLE`, `FILTER`, …), `LET`/`LAMBDA`, VBA UDFs (`myFunc(A1)` in
  `testNames.xlsm`), add-in functions (`_xll.`), and every function not listed.

Structured references (`Table1[Col]`: resolution needs table parts; deferred to Phase5) and
external references `[n]…` give `Everything`. For `[n]` the cached values belong to another
workbook, but a link can point back at this file, so it is not assumed safe.

#### 2.3 Rules for the counterexamples of reviews B1 and R2-M1

- **(a) Implicit resizing and position-dependent arguments**:
  - `SUMIF(range, criteria, sum_range)` and `AVERAGEIF(…)`: the precedent from `sum_range` is
    the rectangle that starts at `sum_range`'s top-left cell and has `range`'s shape (Microsoft
    "SUMIF function"), unioned with `sum_range` itself.
  - `range` and `sum_range` must be **literal** rectangles: a reference token, or a defined
    name whose text is a single literal reference. A computed reference there (`INDEX(…)`,
    `CHOOSE(…)`, `IF(…)`, a `:` expression, a union) gives `Everything` (R2-M1 3). Without this
    rule, `SUMIF(A1:A10,">0",INDEX(B1:B10,5))` would get `B1:B10`, but reads `B5:B14`.
  - General rule: a reference-returning function (`INDEX`, `CHOOSE`, `IF`, `IFERROR`, `IFNA`,
    `IFS`, `SWITCH`) whose result lands in an argument whose **position** determines what is
    read gives `Everything`. On the current table, the only such arguments are the `range` and
    `sum_range` of `SUMIF`/`AVERAGEIF` and both sides of `:`.
- **(b) Range operator with a computed side**: `A1:INDEX(…)`, `A1:CHOOSE(…)`, `Name1:Name2`,
  `A1:IF(…)`, and generally any `:` whose operands are not both literal rectangles on the same
  sheet, give `Everything`.
- **(c) UDFs and add-ins**: not on the table, so `Everything`.
- **(d) Data tables**: a `t="dataTable"` anchor and every result cell of its `ref` give
  `Everything`.
- **(e) Blocked spills**: any formula cell whose cached value is `#SPILL!` (`t="e"`), with or
  without `cm` (R2-M1 1: whether Excel always writes `cm` on a blocked anchor is unverified),
  gives `Everything`, because its result depends on the emptiness of cells no token names.
- **(f) Relative names**: a defined name with any reference component lacking `$` (row or
  column) gives `Everything` until a translation rule for "relative to the using cell" is
  specified and tested.
- **Volatile flag**: `f/@ca="1"` (or `true`) gives `Everything`.

Shared formulas: a member's precedents are the master's text **translated** to the member
(§4.2's translation, never calamine's lexical approximation), then analysed. If translating a
member fails, that member gives `Everything`.

#### 2.4 Name and sheet resolution (R2-M1 4)

- **Sheet names** are matched against `<sheets>` **case-insensitively**, as Excel does, with a
  deliberately broad fold (upper case, then lower case). Excel never writes two sheet names that
  differ only in case, but an input can: when two sheet names fold alike (`Data`/`DATA`, the
  Kelvin sign U+212A and `k`), a reference to either is **ambiguous and gives `Everything`**
  (review R3-M1 c). A broader fold than Excel's can only add `Everything`, never a wrong precise
  set. `worksheet_by_name` prefers an exact match and refuses an ambiguous folded one.
- **Unknown sheet names** (not in `<sheets>`, e.g. a misspelling, a sheet deleted by another
  tool) and **unknown 3-D endpoints** give `Everything`, never "no precedents".
- **3-D spans** cover every sheet between the two endpoints in workbook order, inclusive,
  whichever endpoint comes first (`Sheet3:Sheet1!A1` = `Sheet1:Sheet3!A1`). Chartsheets in the
  span hold no cells and are ignored.
- **Unqualified names** (`Rate`) resolve, case-insensitively, to the local name of the
  formula's own sheet (`localSheetId` = that sheet's 0-based position in `<sheets>`), else to
  the global name. No match gives `Everything`; two names in the same scope that fold alike
  (`Rate`/`RATE`) are ambiguous and give `Everything` (R3-M1 c).
- **Inside a defined name** (local or global), an unqualified reference (`$A$1`) or an
  unqualified name gives `Everything` (R3-M1 a). The readers disagree on which sheet it means:
  O3 observed LibreOffice binding it to the **name's own scope sheet** (`o1_name_scope`:
  `S1!C1 = S2!Loc` computed `S2!A1`, review R4-m4), while Excel's documented UI behaviour
  suggests the using cell's sheet (unverified, XL item). Since the answer depends on the
  reader, the safe side is taken. Excel itself always writes sheet-qualified references in names, so Excel files
  lose no precision.
- **Sheet-qualified names** (`Defines!NR_To_A1`, POI `56737.xlsx`) resolve **only** to the local
  name of that sheet. No local match gives `Everything` (whether Excel falls back to a global
  name is unverified, so the safe side is taken).
- **`[0]!Name`** refers to this workbook, but like every `[n]` prefix it gives `Everything` in
  this phase (a possible precision gain later).
- A `definedName` whose `localSheetId` names no sheet is unusable: any formula using that name
  text gives `Everything`.
- **Name/reference collisions**: a token that is a valid A1 reference and also equals a defined
  name (case-insensitive; `TAX2024` is column `TAX` ≤ `XFD`) gives `Everything`. Excel 2007+
  does not let users create such names, but converters and third-party writers can. Translating
  such a token fails (§4.2), with or without a sheet prefix (`Sheet1!TAX2024`, R3-m1).
- **Assigning formulas** (`f/@bx="1"`, ECMA §18.3.1.40: the formula assigns a value to a name;
  XLM): if any formula cell in the workbook has `bx` true, every formula that uses a defined
  name gives `Everything`.

### 3. Dependency-based cache invalidation for value edits

#### 3.1 Plan → validate → apply, in place (reviews M2, R2-M3)

Every editing call (value write, formula write, formula removal, master hand-off) runs in two
phases. **The plan computes everything and proves that apply cannot fail; apply mutates in place
in a fixed order.** No part is cloned except `workbook.xml`.

1. **Plan and validate** (no observable change; parsing a part for inspection is allowed,
   because a dirty-free parsed part is still raw-copied):
   - every guard of §5, including those that look at the dependent set `D` (§3.2);
   - the target cell's Layer 2 position, and that its insertion is possible: rows and cells
     ascending with `r` (as Phase0), `sheetData` present or insertable (no
     `mc:AlternateContent` child of `worksheet`), no element other than `row` in `sheetData` and
     no `mc:AlternateContent` in the target row;
   - the Layer 2 position of every dependent that still has a cached value, found in **one pass
     over each affected sheet's `sheetData`**. Every dependent formula cell of the census must be
     found there, with the same `r`; a disagreement between the two parsers (e.g. a duplicate
     `r`) is `Err(Malformed)` (R2-m1). Each affected sheet must open in Layer 2 (no Strict, no
     `MissingReference`, no `OutOfOrder`);
   - the `calcPr` change is made on a **clone of `workbook.xml`** (typically a few KB). A
     refusal there (`calcPr` would go next to an `mc:AlternateContent` child of `workbook`:
     `UnresolvedSibling`; 78/333 corpus `workbook.xml` have one, though none of the 2 formula
     workbooks lacking `calcPr` does) is an ordinary `Err` before anything changed;
   - `calcChain.xml` removal (§4.3) is checked with
     `Package::check_remove_part(calc_chain, &[REL_CALC_CHAIN])`, which runs the same validation
     as `remove_part` without changing anything;
   - the shared-formula hand-off texts (§4.2) are computed; a failed translation is
     `Err(SharedFormulaMaster)`.
2. **Apply**, in this order: (i) `remove_part(calc_chain, &[REL_CALC_CHAIN])`, which updates the in-memory
   content types and relationships together with the parts; (ii) the staged `workbook.xml` is
   moved into the package; (iii) `v`/`t` removal at the planned positions, sheet by sheet;
   (iv) the shared-formula hand-off; (v) the target cell write; (vi) the `FormulaIndex` and
   dependency-graph update, once, at the end.

Every step of apply has been shown to succeed by the plan, so an `Err` from apply is a bug. It
is still handled: the error is returned as `Err(Poisoned)`, and the `Workbook` is **poisoned**:
every later edit and every `save`/`to_bytes` returns `Err(Poisoned)`, so a partial edit can
never reach a file. The workbook is marked poisoned *before* apply starts and cleared only when
it succeeds, so a panic inside apply that a caller catches also leaves it poisoned (R3-m3).
`Workbook::package()` is for inspection; its documentation says not to save through it while
poisoned. Tests inject a failure before each apply step (cargo feature `fault-injection`) for
value edits, a shared-master hand-off, a workbook without calcChain and `KeepStaleCaches`
(§7.3).

**Cost (measured, `examples/phase1_bench.rs`, release build, this sandbox):** one worksheet of
20,000 rows; column A holds values and B–K hold the chains `B=A+1`, `C=B+1`, … (200,000
formulas, 9.7 MB of XML). "Worst case" (`WORST=1`): `B1` caches `#SPILL!`, so it depends on
everything and seeds its potential spill area, i.e. the whole sheet below and right of it, and
every edit's dependent set contains every formula (review R3-m4).

| Step | Typical | Worst case |
|---|---|---|
| Staged clone of the sheet tree (revision 2's design), per edit | 289 ms | 280 ms |
| `open` (census of all sheets + Layer 2 parse of the first sheet) | 677 ms | 670 ms |
| First edit (builds the graph; typical: strips 10 dependents, worst: 200,000) | 121 ms | 476 ms |
| Each of 1,000 edits on new rows | 2.0 ms | 6.7 ms |
| An edit whose dependents are already stripped | 1.3 ms | 5.3 ms |
| `save` | 176 ms | 175 ms |

The cost of an edit is proportional to the size of its dependent set, **including dependents
that already lost their cached values** (they are visited, not rewritten), plus one pass over the
rows of each sheet that still has a cached dependent. The closure of the `Everything` formulas
is the same for every edit, so it is computed once per graph and reused (it is reset when a
formula is added); without that cache the worst case cost about 90 ms per edit. A batch API is therefore
not needed for Phase1's performance; Phase6 (large sheets) may still add one. **Addendum
(P5-pre):** Phase5's operations need one for **atomicity**: §3.1a.

#### 3.1a Multi-cell write plan (post-acceptance addendum, P5-pre)

ADR-0017 §2.1 needs the cell writes of one operation (a table's header, totals and column cells; a
dynamic-array anchor) validated together, before anything changes, and its `calcPr` change and
calcChain removal inside the operation's registry transaction (PR #38's `RegistryPlan`).

1. **`CellWritePlan`** (crate-internal; `Workbook::plan_writes(Vec<(usize, CellIndex, CellOp, Extra)>)`) runs
   §3.1's step 1 for **all** cells: every guard of §5 for each cell, the union of their
   dependents (§3.2) and the guards on it, the strip positions, the shared-formula hand-offs,
   the `calcPr` change on a clone of `workbook.xml` (cloned from the part's current state) and
   the calcChain removal check. Any refusal is an ordinary `Err`; nothing changed.
2. **Dependents and guards.** The plan computes the union of dependents from the graph before
   the batch:
   - a cell appears at most once (`Err(DuplicateCellWrite)`);
   - at most one cell of each shared-formula group is written (`Err(SharedFormulaMaster)`;
     write them in separate calls), so a hand-off is independent of other writes;
   - the dependent-cache strips and the dependent-cell tests for `fullCalcOnLoad` and
     `IterativeCalculation` exclude cells whose values or formulas the plan replaces.
     A new formula has no cache. Its downstream dependents are reached by seeding the
     formula cell itself. The formula census is updated during apply.
3. **Apply, in place** (`apply_writes`): §3.1's order — calcChain, `workbook.xml`, strips,
   hand-offs, Phase2 registry changes, then the cells in the given order, then the census and graph. Strips and hand-offs
   use positions planned on the tree before any write. Each cell is **located again** on the
   current tree (the row index is updated after every write), so two writes into the same new
   row create one row. Locating again cannot fail: the plan checked every cell's insertion, and
   rows and cells inserted by the plan are ordinary `row`/`c` elements in order.
4. **Apply in a registry transaction** (`apply_writes_with(plan, registry)`, the form Phase5
   uses): the staged `workbook.xml` (`stage_part`) and the calcChain removal (`remove_parts(&[calcChain],
   &[REL_CALC_CHAIN])`) are folded into the caller's `RegistryPlan`, which the caller creates
   **after** `plan_writes` (whose Layer 2 checks use `xml_part_mut`, which makes an earlier plan
   stale). A refusal while folding is an ordinary `Err`. Then the workbook is poisoned and
   `Package::apply` runs; its only error, `StalePlan`, is returned before anything changes, so the
   poison is cleared and it is an ordinary `Err` (ADR-0017 r2-m5). Then the remaining in-place steps of item 3
   run in place.
5. **Fault injection** (feature `fault-injection`): before `Package::apply`, before each in-place
   step, and before the **second** cell of a plan (`ApplyStep::LaterTarget`: a partly applied
   plan), as an `Err` and as a panic; each poisons the workbook. A stale registry plan is injected
   too and must not poison.
6. **Public API:** one entry, `Worksheet::write_cells(&[(CellIndex, CellWrite)])`, all or
   nothing. `CellWrite` is `Number`, `String`, `Formula` or `RemoveFormula`.
   `String` follows `set_string`: it uses the shared-string table when present and falls back
   to an inline string otherwise. Planning checks the registry slot, reuses canonical entries
   from the table and from earlier writes in the plan, and totals the changes in shared-string
   references from target writes and cache strips.

Oracles: the batch uses the same cell-write rules and registry allocation order as the
single-cell setters. Calculation effects can depend on the order of individual calls.
For example, after replacing a formula with `INDIRECT("A1")`, a later individual write can
see that formula as a dependent and set `fullCalcOnLoad`. In an iterative workbook, writing
that formula before a value can make the value write fail with `IterativeCalculation`.
The batch excludes its own formula writes from these dependent-cell checks. The registry
transaction gives the same bytes as the in-place apply. Every planning refusal leaves the
original bytes intact. O1, O2 and O3 are unchanged.

#### 3.2 Dependents

A value write (`set_number`, `set_string`, later Phase2 setters) to a cell X that is **not** a
result cell (writes into result cells are refused, §5):

1. Seeds = {X}.
2. Dependents = every formula `f` with `P(f) = Everything` or `P(f)` containing a seed.
3. Iterate to a fixed point:
   - a dependent formula cell becomes a seed;
   - a dependent array or data-table anchor makes all of its result cells dependents and seeds;
   - a dependent **dynamic-array anchor**, and a dependent formula cell whose cached value is
     `#SPILL!`, make all of their current result cells dependents, and seed their **potential
     spill area**: every cell from the anchor to the sheet's last row and column (R2-M1 1).
     Any formula reading any cell of that area becomes a dependent. (Example: `D1 =
     SEQUENCE(A1)` spilling `D1:D5`, `E1 = D6*2`; writing `A1` makes `E1` a dependent.)

`set_formula`, formula removal and formula → value overwrites use the same seeds (the cell's
value changes). The graph is built lazily from §1 on the first edit and cached; formula changes
update it.

#### 3.3 Changes

1. **Each dependent cell** (formula cell or result cell) that still has a cached value, in its
   sheet's Layer 2 tree:
   - remove `v`;
   - remove `t` **whatever its value** (`str`, `b`, `e`, `n`, and the LibreOffice
     `inlineStr`-under-`f` shape of `52575_main.xlsx`, whose `is` child is removed too), because
     under a formula `t` describes only the cached value (review m2);
   - leave `f` and every other attribute untouched.

   T2's allowed region for a dependent is its **whole `c` span** (review m1):
   - a `t` removal re-renders the open tag;
   - a result cell that loses its only child `v` keeps its close tag per ADR-0002 §4.2, or
     becomes self-closing when it is also `own_dirty`. Either way only that `c`'s bytes change.

   Dependents on any worksheet are written through the same Layer 2 path. A dependent that has
   no `v` and no `t` is not touched (no dirt).
2. **`workbook.xml`** (Layer 2; write allowlist entry `x:CT_CalcProperties/x:calcPr`), only when
   the edit has at least one dependent or changes the set of formula cells: set
   `fullCalcOnLoad="1"` on `calcPr`, inserting `calcPr` in its schema slot if absent.
   - An existing `1` or `true` is left as is (review m5).
   - In **manual** calculation mode (`calcMode="manual"`, 2 corpus files), `fullCalcOnLoad` is
     **not** set. That respects the user's choice, as rust_xlsxwriter does. Dependents still lose
     their caches, so no reader sees a stale value; Excel users press F9. What Excel shows there
     until F9 is an XL item (§7.4).
   - `calcMode="autoNoTable"` is treated like automatic. Whether `fullCalcOnLoad` also
     recalculates data tables there is an XL item (R2-m4).
   - **Iterative calculation** (`calcPr iterate="1"`/`true`, 2 corpus files: `47889`, `57535`):
     an edit with a non-empty dependent set returns `Err(IterativeCalculation)` (R2-m4). In
     iterative mode a circular formula starts from its cached value, so removing it changes the
     result Excel computes (e.g. a counter `A1 = A1 + B1`). This is conservative: an edit
     whose dependents contain no cycle would be safe, and can be allowed later with a cycle
     check and its own tests.
3. **`calcChain.xml`** is not touched by value writes to non-formula cells: the set of formula
   cells did not change, so every entry still names a formula cell.

**Consequences for readers.** A dependent reads as *no cached value* (calamine: empty), never as
a stale one, in every reader:

- Excel: `fullCalcOnLoad` makes it recalculate, and cells without `v` are computed anyway.
- LibreOffice: cells without `v` are recalculated on load (Context).
- Formulas that do not depend on X keep their cached values and their bytes.
- Once `fullCalcOnLoad` is set, Excel asks "save changes?" when such a file is closed (known
  from rust_xlsxwriter output; review m5).

An opt-in `CalcPolicy::KeepStaleCaches` skips the `v`/`t` removal (it still sets
`fullCalcOnLoad`), for callers who open the result only in Excel and want minimal diffs. It is
**Excel-only**: LibreOffice ignores `fullCalcOnLoad`, so it shows the stale values (review m3).
It is never the default.

### 4. Formula writes and removals

#### 4.1 Plain formulas

- **`set_formula(text)`** writes `<f>text</f>` with **no** `v` and no `t` (an existing `f`,
  `v`, `t` and `is` are replaced; `s` is kept).
  - The cell becomes a formula cell. Its dependents are invalidated as in §3, and
    `fullCalcOnLoad` is handled as in §3.3.
  - Text is stored as given; one leading `=` is dropped for convenience. Stored formula text
    never starts with `=`, so `"==A1"` is `Err(InvalidFormula)` (R3-m2).
  - The text must lex (§2.1); otherwise `Err(InvalidFormula)` and nothing changes. Excel would
    repair a file with an unparsable formula, and Phase1 cannot check more than lexing.
  - Functions that need `_xlfn.` are the caller's responsibility in Phase1.
  - The written formula is never a dynamic array: it has no `cm`, so Excel treats it as a
    legacy (implicitly intersected) formula. `set_array_formula` and dynamic arrays are out of
    scope (roadmap Phase5).
- **Overwriting a formula cell with a value**, or **`remove_formula()`**, removes `f` (and `v`,
  `t`, `is` for a removal), writes the value if any and invalidates dependents.

#### 4.2 Shared groups (review M3)

**Translation.** Moving a formula text from cell A to cell B shifts every **relative** row and
column component of every reference token by (B − A):

- literal references and ranges, the reference parts of 3-D and external references, and
  relative components inside the formula, not inside names;
- quoted sheet names are one unit, with `''` escapes and `!` inside the quotes (R2-m3);
- string literals, array constants and structured references are copied unchanged (a structured
  reference is relative to its table, not to the cell);
- a function name shaped like a reference (`LOG10(`) is not a reference (R2-m3).

Translation **fails** (`Err`, never "keep the original text", unlike calamine's out-of-bounds
behaviour) when:

- the text does not lex; or
- any shifted component leaves the sheet: row < 1 or > 1,048,576, column < A or > XFD; or
- a reference token collides with a defined name (§2.4), with or without a sheet prefix; or
- the group has two masters for its `si` on the sheet (§1, R3-M1 b).

`INDIRECT` and other `Everything` constructs are not a failure: their text is copied unchanged
(their string arguments are not references), and their precedents stay `Everything`.

**Overwriting a member** (with a value, another formula, or a removal) removes only that
member's `f`.

**Overwriting the master**:

1. The new master is the **first** remaining member in row-major order (R2-m3; "next" was
   ambiguous when members precede the old master).
2. Its `f` gets the master text translated from the old master to it.
3. The new `ref` is the **bounding rectangle of the remaining members**, the new master
   included. The new master need not be its top-left cell: Excel writes that shape itself
   (`51710.xlsx`). This equals POI's `onDeleteFormula` result when the remaining members fill
   that rectangle row-major from the new master. Otherwise POI's formula can exclude members and
   ours does not.
4. Every remaining member keeps its meaning. Its text is *new master text translated by (member −
   new master)*, which equals *old text translated by (member − old master)*: translation by
   (N − O) is exactly N's own original meaning, and for each member M and each component c, if
   c + (N − O) and c + (M − O) are both in range then T(M − N) ∘ T(N − O) = T(M − O). So an
   intermediate result can leave the sheet only if N's own original formula did, which step 2
   already refuses (R2-m3). The plan still checks every member's translation from the new
   master.
5. If the old master was the only member, the group disappears.

`Err(SharedFormulaMaster)` is returned **only** when a translation of step 2 or step 4 fails.

#### 4.3 calcChain removal and `remove_part` (review M1)

Writes that change the set of formula cells (any of the above) make `calcChain.xml` stale.

**Decision: remove the part.** That means the `calcChain.xml` part, its relationship in the
workbook part's `.rels`, and its `Override` in `[Content_Types].xml`; entries are not maintained.

- Excel rebuilds the chain on load.
- Files without a chain are the norm from openpyxl and rust_xlsxwriter.
- A partially maintained chain is the one state known to trigger a repair; an absent one is known
  not to.

This needs `Package::remove_part(name, allowed_rel_types)` and
`Package::check_remove_part(name, allowed_rel_types)` in `ooxml-core`
(separate additive core PR with its own tests). It is an **explicit, narrow exception** to
ADR-0001 Layer 3's "registries are append-only" invariant. On acceptance of this ADR, ADR-0001
gets a post-acceptance note with this text:

> A registry entry may be removed only together with the part it registers; only if the caller
> explicitly allows the type of every relationship to that part (a part found by relationship
> type is referenced implicitly); and only if no attribute of a removed relationship's source
> part, in any namespace and after XML unescaping, equals the removed `Id`. Part names follow
> OPC equivalence (ASCII case-insensitive, percent-decoded targets). No remaining entry is ever
> renumbered.

Until this ADR is accepted, ADR-0001 carries an interim note with these same conditions (PR #14);
on acceptance this text replaces it.

`remove_part(name, allowed_rel_types)` contract (as revised by the PR #14 reviews, rounds 1–3):

- **Part names** follow OPC equivalence everywhere (the part, source parts, `_rels` folders, its
  own `.rels`, relationship targets, `Override` part names): ASCII case-insensitive and
  percent-decoded; XML whitespace around `Target` and `Id` is collapsed. Entries that are
  equivalent are a duplicate part name, refused at open. Only the root `_rels/.rels` holds
  package relationships.
- **Precondition**: the part exists and is not a registry part; it has **no relationships part
  of its own**, else `Err` (cascading deletion is out of scope); every
  relationship to it has a type the caller allows (a part found by relationship type is
  referenced implicitly; ooxml-xlsx allows only the calcChain type); no attribute of a removed
  relationship's source part, in any namespace (`r:id`, VML `o:relid`, …), equals the removed
  `Id` after XML unescaping; every registry part to change is editable in place (a broken
  `.rels` that does not mention the part is ignored); and no `.rels` mentions the part's file
  name without a relationship resolving to it (a safety net against spellings this code does
  not resolve). Refusals are a typed `RemoveRefusal`.
- **Scope**: every `.rels` part in the package is scanned. Every internal relationship that
  resolves to the part is removed. Every `Override` for the part is removed; a `Default` is
  never touched.
- **Whitespace**: the removed element **and the whitespace-only text node immediately before
  it, if any**, are removed, so indented `.rels` and content-types parts stay tidy. T2's allowed
  diff for these parts is exactly that element plus that text node.
- **Atomicity**: `check_remove_part(&self, …)` and `remove_part` run one validation that changes
  nothing (not even part states); after it the removal cannot fail. The in-memory content-type
  and package-relationship caches are updated with the parts (R2-M3). Until this ADR is
  accepted, ADR-0001 carries an interim note recording this exception.

#### 4.4 Future functions (post-acceptance addendum, P5-pre; ADR-0017 Q7)

- `set_formula` (and `write_cells`) take the **file form**. A known future function written
  without its prefix is refused with `Err(MissingFunctionPrefix { name, prefix })`, because
  Excel would show `#NAME?` until the formula is re-entered. This changes Phase1's contract for
  such texts, which were accepted before.
- The table is a fixed copy of rust_xlsxwriter 0.102's `future_functions` (161 names):
  `_xlfn.`, and `_xlfn._xlws.` for `FILTER`, `SORT` and `PY`. Both `_xlfn.FILTER` and
  `_xlws.FILTER` have incomplete prefixes and are refused. The opt-in helper adds the missing
  prefix. The prefix check covers only functions listed in the fixed table.
- The spill (`A1#`) and implicit-intersection (`@`) operators are refused
  (`Err(DynamicArrayOperator)`); their file forms are `_xlfn.ANCHORARRAY(…)` and
  `_xlfn.SINGLE(…)` (ADR-0017 review m7).
- `ooxml_xlsx::add_future_prefixes(text)` converts a formula as typed (string literals left
  alone). Parameters of `LAMBDA` (`_xlpm.`) are not checked or added: they are names, not
  function calls.

### 5. Guards

| Situation | Result |
|---|---|
| Write into a result cell of an array/data-table/spill range | `Err(ArrayResultCell)` |
| Write to an array/data-table anchor or a `cm` anchor (value or formula) | `Err(ArrayAnchor)` (array editing is Phase5) |
| Workbook has a `cm` that is not an XLDAPR dynamic-array anchor | `Err(UnknownCellMetadata)` for every edit (§1) |
| Census incomplete (scan failure, `f` outside a cell, formula sheet without `r`) | `Err(FormulaScanFailed)` / `Err(MissingReference)` for every edit (§1) |
| Write into a table header row | `Err(TableHeaderCell)` (as Phase0; header sync is Phase5) |
| Write to a `vm` cell, or an edit whose dependents include one | `Err(RichValueCell)` (as Phase0) |
| An edit would make a macro-sheet or dialog-sheet formula a dependent | `Err(MacroSheetDependent)`: XLM caches are not modelled |
| Iterative calculation and a non-empty dependent set | `Err(IterativeCalculation)` (§3.3) |
| `set_formula` text that does not lex | `Err(InvalidFormula)` |
| A value or formula write to a **non-anchor** cell of a merged range (ADR-0014 §3: the anchor is writable; style-only writes are allowed on every cell of the range), missing `r`, Strict, encrypted | unchanged from Phase0, except the anchor relaxation |
| A translation of §4.2 fails | `Err(SharedFormulaMaster)` |
| A previous apply failed | `Err(Poisoned)` for every edit and save (§3.1) |

Every guard is checked in §3.1's plan, before anything changes.

### 6. Scope changes to ADR-0002 §5/§6 and the public API (reviews m6, R2-m2)

Phase1 adds, as allowlist entries and code paths:

- **Layer 2 writes to any worksheet**, not only the first visible one, for dependent `v`/`t`
  removal and for writes through `Workbook::worksheet_by_name` (case-insensitive) and
  `Workbook::worksheet_by_index` (roadmap Phase1 scope 1). The index is the 0-based position in
  `<sheets>`, hidden sheets included; an index or name that designates a chartsheet, macro sheet
  or dialog sheet returns `Err(NotAWorksheet)`.
- **Write** entries: `x:CT_CalcProperties/x:calcPr`, `x:CT_CellFormula/x:f` (moves from
  `[read]` to `[write]`).
- **Read** entries: `x:CT_DefinedNames/x:definedNames`, `x:CT_DefinedName/x:definedName`.
  `metadata.xml` and macro/dialog sheets are read by the census's event scan only.
- **Layer 2 writes to `workbook.xml`** (`calcPr` only), and to the workbook `.rels` and
  `[Content_Types].xml` through `remove_part` only.
- **Table parts** are read for every worksheet that is opened for editing (Phase0 read only the
  first visible sheet's).

**Public API changes** (the crate is pre-1.0):

- `Worksheet<'wb>` borrows the whole `Workbook` mutably (it held only the sheet's tree and a
  read-only context), so that a cell edit can reach other sheets, `workbook.xml` and the
  package. `CellRef`/`CellMut` are unchanged in shape.
- `CellMut` gains `set_formula` and `remove_formula`; `set_number`/`set_string` now also work
  on formula cells (§4.1).
- `Workbook` gains `worksheet_by_name`, `worksheet_by_index`, `sheet_names`,
  `set_calc_policy`/`calc_policy`, and `dependents(sheet, cell)` (the computed set, for
  inspection and the O1 tests).
- `Workbook::edit_refusal()` now reports only workbook-wide refusals (§1, §5). `FormulaPresent`
  is no longer returned and is removed from `Error`; new variants: `ArrayResultCell`,
  `ArrayAnchor`, `UnknownCellMetadata`, `MacroSheetDependent`, `IterativeCalculation`,
  `SharedFormulaMaster`, `InvalidFormula`, `NotAWorksheet`, `NoSuchSheet`, `Poisoned`.

### 7. Acceptance (testable)

All tests below sit on top of the Phase0 suite (roadmap §7). Every one checks T1/T1b on touched
parts, L1 on the rest, and "`Err` ⇒ nothing changed".

**Where Phase1 intentionally touches more bytes than Phase0**, the tests assert exactly this
allowed set, and nothing else, **byte for byte** (review R3-M2): the expected output is built
from the input by applying only these edits, and compared with the actual output.

- the edited cell's `c` span (and a new `row`/`c`, `dimension`, as Phase0's T2 region);
- each dependent that had a cached value: its `c` span with exactly the `t` attribute and the
  `v`/`is` children removed (every other attribute, the `f` element, the row tag and the bytes
  between cells unchanged; a re-rendered start tag that lost its last child self-closes,
  ADR-0002 §4.2);
- for a master hand-off, the new master's `c` span (its exact text is asserted separately);
- in `workbook.xml`, exactly `fullCalcOnLoad="1"` added to `calcPr` (or one inserted
  `<calcPr fullCalcOnLoad="1"/>`);
- for a calcChain removal: the part is gone, and `[Content_Types].xml` and the workbook `.rels`
  each equal the input minus one element and the whitespace text right before it.

The synthetic O1 workbooks give every formula cell a style (`s`) and every row `spans`/`ht`, so
that a corrupting edit to those would be caught.

#### 7.1 Oracles that do not consult the implementation (reviews B2, R2-M2)

- **O1 — hand-written dependent sets.** Small synthetic workbooks under
  `reference/fixtures/xlsx/synthetic/`, written by a committed **stdlib-only** Python generator
  (`build.py`: raw XML + `zipfile`, so it can write cached values; openpyxl cannot). The
  generated files are committed; no Python runs at test time. Each case has an expected
  dependent set per edit, **written by hand** in the test (and in the directory's README).
  Required cases:
  - cross-sheet (ADR-0001 R2-M3's `Sheet2!B1 = Sheet1!A1*2`, edited on `Sheet1`);
  - a local and a global name with the same name, and a sheet-qualified local name
    (`56737.xlsx` shape);
  - a 3-D reference (also reversed), `A:A` and `1:1`;
  - a lower-case sheet name in a formula, and an unknown sheet name;
  - a three-step transitive chain across sheets;
  - each of §2.3 (a)–(f), plus `ca="1"`, a UDF call, `[1]` external, `LOOKUP`, a computed
    `sum_range`, a name colliding with a reference, `bx`;
  - an array formula and its result cells; a dynamic-array anchor with a reader below its
    current spill range (R2-M1 1); a `#SPILL!` cell;
  - formulas that must **not** become dependents. Their cached `v` must survive byte-identical,
    so a "clear everything" implementation fails too.
- **O2 — perturbation oracle (soundness).** A property test with a deterministic seeded
  generator (no extra dependency):
  - It generates random workbooks in an evaluable subset: literal references and ranges,
    cross-sheet and 3-D references, `+ - * /`, `SUM`, `MIN`, `MAX`, `IF`, `INDEX`, `SUMIF` with
    mismatched shapes, absolute global, local and sheet-qualified names; sheet names are
    rendered with random case and optional quotes; `$` and spaces are random (R2-M2).
  - A **test-only evaluator over the AST** gives ground truth. It never sees the tokenizer under
    test.
  - For each random edit of X, it evaluates the workbook with X set to several different values.
    Every formula whose value changes for any of them **must** be in the computed dependent set
    (computed ⊇ observed).
  - Over-approximation is allowed. A missed dependent fails the test with the seed and the
    workbook printed.
- **O3 — recalculation oracle** (R2-M2, R2-m6). Inputs: every O1 workbook, a fixed-seed sample
  of 200 O2 workbooks, and the acceptance edits of §7.2 on real fixtures. For each edit, two or
  three values of different magnitude are written, each giving its own output.
  - `examples/o3_inputs.rs` writes, deterministically, every input and output to a directory;
    `spikes/xlsx/o3_recalc.py` recalculates each with LibreOffice headless and hard recalc
    (`OOXMLRecalcMode` = always, set in a throw-away profile), re-saves, reads every formula
    cell's value with `zipfile` + `xml.etree`, and writes `reference/fixtures/xlsx/o3/expected.tsv`
    with the LibreOffice version and each file's FNV-1a hash. The `workflow_dispatch` GitHub
    Actions job `.github/workflows/o3-recalc.yml` (its own PR, so that it exists on the default
    branch) installs an **exact LibreOffice release** from the Document Foundation archive (input,
    default 25.2.7.2), runs the same script against the dispatched branch, checks it, and uploads
    the TSV. The owner reviews and commits it. LibreOffice is not needed at test time. The data
    was generated by run 43821205800 (LibreOffice 25.2.7.2) and is committed with its provenance
    in `reference/fixtures/xlsx/o3/README.md`.
  - The test (`tests/phase1_o3.rs`) regenerates the outputs in process and checks: **every formula
    cell that kept its `v` in an output, and whose input cached value agreed with LibreOffice's
    recalculation of the input (baseline), holds LibreOffice's recalculated value of the
    output.** Cells that disagree already in the input (LibreOffice ≠ Excel, POI-written values,
    manual mode) are thereby excluded automatically. Workbooks with any volatile formula (a
    volatile function in any formula text, shared masters included, or `ca="1"`) are skipped
    entirely, which excludes every transitive dependent of a volatile cell without using the
    dependency code under test (R3-m5).
  - The data is committed (`reference/fixtures/xlsx/o3/expected.tsv`, from workflow run
    43821205800, LibreOffice 25.2.7.2; provenance in the README there), so the test runs in plain
    `cargo test`. Without the data it fails with instructions to regenerate it. Each row carries a
    hash of its workbook, so a change to the O3 inputs fails the test until the data is
    regenerated.
  - Licence: the expected values are facts computed from the Apache-2.0 POI fixtures and from
    synthetic workbooks; LibreOffice's (MPL-2.0) output is not covered by the MPL. They sit
    next to the fixtures' `NOTICE`, with the generating script and commit recorded.
- **Mutation check.** The rules of §2.3/§2.4 and the special cases of §2.2 are written so that
  each one is a separate branch; disabling any one of them must fail at least one O1 or O2 test.
  This is checked by `cargo-mutants` on the precedent module (an owner/CI step, not part of
  `cargo test`). During implementation, disabling the `sum_range` resize and reversing
  local-over-global name shadowing were each checked by hand to fail O1 and O2.
- Each O1/O3 edit writes values that differ in magnitude from the original (never the same value
  back). O2 evaluates several values per edit, so a dependent that ignores one change still
  shows up for another.

#### 7.2 Fixture tests

1. **Value edit, shared formulas** — `shared_formulas.xlsx`: write a precedent of the group.
   - T2 confined to the edited cell, the dependents' `c` spans, and `calcPr`.
   - CR (calamine): dependents read empty, every other cell unchanged. The formula text is
     identical for all formula cells.
   - `calcChain.xml` L1.
2. **Array results** — `MatrixFormulaEvalTestData.xlsx`:
   - writing a precedent of an array formula clears `v` from every cell of its `ref`;
   - writing a non-anchor result cell → `Err(ArrayResultCell)`; the anchor → `Err(ArrayAnchor)`.
3. **Dynamic array** — `xlookup.xlsx`:
   - writing into the spill range → `Err(ArrayResultCell)`;
   - writing a precedent: the anchor and the spill cells lose `v`; `metadata.xml` L1.
4. **No calcChain** — `70005-countifs.xlsx` (LibreOffice, data descriptors): the value edit
   succeeds; no `calcChain.xml` is created; `fullCalcOnLoad` is set on the existing `calcPr`.
5. **Conservative path** — a synthetic workbook with `INDIRECT`: every formula cell downstream
   of it becomes a dependent. (Revision 2 planned a POI file; a synthetic one keeps the fixture
   set small and has hand-written expectations.)
6. **Formula write + calcChain removal** — `shared_formulas.xlsx`: `set_formula` on an empty
   cell.
   - The output has no `calcChain.xml`.
   - The workbook `.rels` and `[Content_Types].xml` each lose exactly one element plus its
     preceding whitespace text (§4.3).
   - All other parts L1, except `workbook.xml` (`calcPr`) and the sheet.
7. **Master hand-off**:
   - `shared_formulas.xlsx` (1-D group);
   - `NewStyleConditionalFormattings.xlsx`, 2-D group `D2:U17`: after the master `D2` is
     overwritten, `ref="D2:U17"` and the master is `E2`;
   - a synthetic group whose master is not at the top-left (`51710.xlsx` shape, rebuilt
     synthetically rather than adding the 1 MB file);
   - a synthetic group with a quoted sheet name `'Q1 2024'!A1` and a relative reference that
     would leave the sheet (→ `Err(SharedFormulaMaster)`, nothing changed).

   Oracle: hand-written expected texts for every remaining member of the synthetic groups, and
   for the fixtures, calamine's shared-formula expansion of the output equals its expansion of
   the input for every remaining member (calamine is a second, independent translator; where it
   is known to differ, e.g. shifting inside `'…'`, the synthetic hand-written cases decide).
8. **Tokenizer** — no panic on every formula text in the committed fixtures and on random
   strings; hand-written expected tokens for the shapes of §2.1.
9. **Manual calculation mode** — a synthetic `calcMode="manual"` workbook: dependents lose `v`,
   and `fullCalcOnLoad` is not added.
10. **Read-back** — for every successful edit above, calamine reads no formula cell whose
    cached value differs from the value it had before **unless** that cell is now empty
    (no stale value can be observed).

#### 7.3 Failure atomicity

For each `Err` case above, `save` after the error gives L1 for every entry, and no part is left
dirty. With `--features fault-injection`, a failure injected before each apply step of §3.1
returns `Err(Poisoned)`, and every later edit, `save` and `to_bytes` return `Err(Poisoned)`.

#### 7.4 XL (owner, Microsoft 365 Excel)

- The outputs of 7.2 items 1, 2, 3, 6, 7 and 9 open without a repair dialog and show
  recalculated values (`examples/manual_check.rs` writes them with a checklist).
- For item 9, record what Excel shows before F9.
- `autoNoTable` with a data table (R2-m4), and `LOOKUP` with a short result vector (R2-M1 2):
  record Excel's behaviour; neither blocks this ADR, because both are already on the safe side.
- Unqualified references inside a defined name (§2.4, R4-m4): which sheet Excel binds them to.
- Sheet names that differ only in width (`Ａ` / `A`, R4-m7): whether Excel treats them as the
  same name.

## Alternatives considered

1. **Delete `calcChain.xml` and strip every cached value in the workbook on any edit**
   (openpyxl's effective behaviour).
   - Simple and safe for Excel.
   - It destroys every cached value a non-Excel reader relies on, even for formulas unrelated
     to the edit, and touches every formula cell in every sheet, defeating ADR-0001's locality
     (T2).
   - §3 behaves like this only for the formulas whose precedents are `Everything` and their
     transitive dependents. Formulas with precise precedents keep their caches (review m9).
2. **Only set `fullCalcOnLoad="1"`, keep stale caches** (what ADR-0001 listed as one option,
   and close to rust_xlsxwriter's stance).
   - Excel is fine.
   - calamine and pandas silently read wrong values, and so does LibreOffice, which ignores the
     flag.
   - This is the failure ADR-0001 R2-M3 was written to prevent, so it is only offered opt-in
     (`KeepStaleCaches`).
3. **Recompute dependents (embed an evaluator).** The only option that keeps every reader
   correct *and* keeps caches. Excel's function library is enormous (hundreds of functions,
   behaviour that depends on locale and version); this is a project of its own (roadmap C8).
   Rejected for Phase1. The dependency graph built here is the part an evaluator would reuse,
   and O2's test evaluator is deliberately tiny and test-only.
4. **Maintain `calcChain.xml` entries (POI's approach) instead of removing the part.** It keeps
   the part, but needs correct `i` inheritance and ordering, and gives Excel nothing it needs
   (it rebuilds the chain). It may be added later behind the same API if a user needs it.
5. **A denylist dependency rule** (revision 1: precise unless a listed construct appears).
   Rejected by review B1: every construct the list forgets becomes a silent stale value.
6. **Bounding-box rule for `:` with computed sides** (review B1 alternative). Sound, but needs
   per-function proofs that reference results stay inside the arguments. Deferred; `Everything`
   for now (§2.3 b).
7. **Staged clones of every touched part** (revision 2, §3.1). Simple and atomic, but one
   clone of the edited sheet per call: about 330 ms for a 9 MB sheet, so 1,000 writes cost
   minutes (review R2-M3). Replaced by validate-then-apply with poisoning; only the small
   `workbook.xml` is cloned.
8. **An undo log** (record each removed node and attribute, roll back on failure). Atomic
   without clones, but every apply step would need an inverse, and the inverse of a
   `remove_part` is an insertion into append-only registries. Not needed while apply cannot
   fail.
9. **Keep Phase0's refusal and do registries first.** Leaves the majority of real workbooks
   uneditable. See roadmap §6 for the value × risk comparison; the owner chose this ADR's order.

## Consequences

- Positive:
  - Editing works on formula-bearing workbooks, which is most real ones, and no reader ever sees
    a stale **cell-formula cached value** (`c/v` under `f`, and the result cells of array,
    data-table and spill ranges). Other caches are out of scope and unchanged from Phase0
    (R2-m7; roadmap §4): chart `c:numCache`/`c:strCache` (fixture `57362.xlsx`), pivot caches
    and pivot output cells (Excel itself refreshes them only with `refreshOnLoad`), and the
    `sheetDataSet` of an `externalLink` that points back at this workbook.
  - Untouched formulas keep their bytes and cached values whenever their precedents are
    precise.
  - The dependency rule fails safe: a gap in the allowlist costs cached values (visible
    blanks), never correctness.
  - Three oracles, independent of the implementation, check the one failure that matters (a
    missed dependent); O3 compares with a real engine on synthetic workbooks too.
  - Edits cost time proportional to the dependent set (already-stripped dependents are visited
    but not rewritten), not to the size of the sheet; the worst case is measured in §3.1.
- Positive: the tokenizer, translation and dependency graph are reusable by Phase3 (sheet rename
  rewrites), Phase7 (row/column insertion) and any future evaluator.
- Negative: dependents lose their cached values until a calculating application recalculates.
  Read-only pipelines that open the output directly see blanks for those cells. This is honest
  but visible. `KeepStaleCaches` exists for Excel-only workflows.
- Negative: allowlist-by-default is conservative. One unknown function (a UDF, an unreviewed
  `_xlfn.`, `LOOKUP`) or one `INDIRECT` makes every edit invalidate that formula and everything
  downstream of it, and one dependent dynamic array invalidates every reader below and to the
  right of it.
  - The round-1 review's heuristic estimate: about 20 of 127 formula workbooks have at least one
    such formula.
  - Growing the value-function table, reviewed one function at a time with evidence and O1/O2
    tests, reduces this.
- Negative: removing `calcChain.xml` brings two Layer 3 registry parts into Layer 2's write
  scope, under a written exception to ADR-0001's append-only invariant (§4.3). The core
  `remove_part` API is a prerequisite.
- Negative: O3 needs LibreOffice somewhere (not in the development sandbox), and LibreOffice is
  not Excel; the XL check remains the last word for Excel behaviour. The O3 data is keyed on
  the exact output bytes, so any change to what an edit writes (even a semantically neutral
  one) needs the workflow re-run (review R4-m2; keying on the output minus caches is a possible
  follow-up).
- Negative: an apply failure (a bug) poisons the `Workbook`; the caller must re-open the file.
- Negative: Excel asks "save changes?" on close for files with `fullCalcOnLoad` set.
- Negative: iterative-calculation workbooks refuse edits that have dependents (§3.3).

## Open questions (closed on acceptance)

Closed with revision 5, following the round-3 and round-4 reviews' recommendations:

1. `KeepStaleCaches` stays, **not** the default, with a documented warning that readers which
   trust caches see stale values.
2. O3's expected data is generated by the manual CI workflow and committed (`expected.tsv`,
   run 43821205800).
3. No threshold or warning: `Workbook::dependents` lets callers check before editing.
4. Manual calculation mode keeps the current behaviour (no `fullCalcOnLoad` added); the XL
   check of §7.4 still records Excel's behaviour.

Known limits recorded by review round 4 (no design change):

- **R4-m1:** the independent oracles do not reach every rule. O3 compares formula cells only, so
  array/spill **result** cells and the potential-spill-area rule are protected by O1's
  hand-written expectations alone; the range operator `a:b` between two reference tokens is
  covered by unit tests only (O2's generator does not produce it); relative references inside
  names are covered by O1 and unit tests (O2/O3 cannot evaluate them).
- **R4-m7:** `fold` is not shown to be a superset of Excel's sheet-name matching: a sheet
  `Ａ` (full-width) and a sheet `A` resolve precisely to different sheets. If Japanese Excel
  ignores width when matching names, Excel would see them as ambiguous. Excel does not write
  such files itself; XL item: "sheet names that differ only in width".

(Revision 1's open question 2, the `_xlfn.` allowlist, is answered by §2.2.)

## Deviations from revision 2 in this revision

- The tokenizer's differential test against openpyxl's `Tokenizer` over deduplicated POI
  formula texts (revision 2 §2.1) is replaced, for this milestone, by hand-written token
  expectations for every shape of §2.1, a parse check over every formula text of the committed
  fixtures, and an optional (ignored) sweep over a locally extracted corpus file. Run on the 2,157
  distinct formula texts of the POI `test-data/spreadsheet` corpus, 2,154 parse; the other 3 are
  `['file:///…']Sheet!A1` external references, refused and therefore `Everything`. Committing the
  corpus and an openpyxl-generated expectation file remains a follow-up.
- `cargo fuzz` is replaced by a seeded random-input property test (no nightly toolchain or
  extra dependency); a fuzz target can be added later.
- §7.2 items 5 and 7 use synthetic workbooks instead of adding a POI `INDIRECT` file and
  `51710.xlsx` (1 MB) as fixtures.

## Sources

- ADR-0001: Layer 3 "Phase0 policy", R2-M3, R3-M1, and the 2026-11-16 notes. ADR-0002: §4.2
  primitives, §4.3, §5 allowlist, §6. Reviews: `docs/adr/0003-opus-review-round1.md`,
  `docs/adr/0003-opus-review-round2.md`.
- ECMA-376 Part 1, 5th ed. (2016): §18.3.1.40 (`f`), §18.6 (calculation chain), §18.2.2
  (`calcPr`), §18.17 (formulas); Transitional `sml.xsd` `CT_CellFormula`.
- Microsoft Support, "SUMIF function" (`sum_range` resizing):
  <https://support.microsoft.com/en-us/office/sumif-function-169b8c99-c05c-4483-a712-1697a653039b>.
- Apache POI @ `47dcff769bb395034dddfd59c4609659b6dbc6a4`:
  - `poi-ooxml/src/main/java/org/apache/poi/xssf/usermodel/XSSFWorkbook.java`
    (`onDeleteFormula`);
  - `poi-ooxml/src/main/java/org/apache/poi/xssf/model/CalculationChain.java` (`removeItem`);
  - `poi-ooxml/src/main/java/org/apache/poi/xssf/usermodel/XSSFSheet.java` (`onDeleteFormula`,
    shared-master promotion);
  - test data `51710.xlsx`, `52575_main.xlsx`, `testNames.xlsm`.
- LibreOffice/core master `38fb197b`: `sc/source/filter/oox/formulabuffer.cxx`,
  `workbookfragment.cxx` (`recalcFormulaCells`), `workbooksettings.cxx`.
- LibreOffice/core master `2e885d0b`: `sc/source/core/tool/interpr1.cxx` (`ScLookup`: the
  result vector is read past its end; `IterateParametersIfs`: all ranges must have the same
  size; `IterateParametersIf`: the `sum_range` resize; `CalculateLookup`: the index is checked
  against the passed range).
- Microsoft Support, "LOOKUP function", "SUMIFS function", "COUNTIFS function".
- POI test data `56737.xlsx` (sheet-qualified local names), `47889.xlsx`, `57535.xlsx`
  (`iterate="1"`).
- rust_xlsxwriter 0.102.0: `src/workbook.rs` (`fullCalcOnLoad`, manual mode), `src/formula.rs`.
- calamine 0.38.0: `src/xlsx/mod.rs` (`expand_shared_formula_with_offset_into`).
- openpyxl 3.1.5:
  - `openpyxl/worksheet/_reader.py` (shared formula translation);
  - `openpyxl/formula/tokenizer.py`, `openpyxl/formula/translate.py`;
  - `openpyxl/utils/formulas.py` (`FORMULAE`);
  - `openpyxl/workbook/properties.py` (`CalcProperties`);
  - openpyxl issue #501 (relative references in names).
- umya-spreadsheet 3.1.1: `src/writer/xlsx/workbook.rs` (`calcPr`).
- Corpus measurements: `spikes/xlsx/formula_stats.py` (now also counts `c/@cm` and `ca`);
  fixtures in `reference/fixtures/xlsx/`.
