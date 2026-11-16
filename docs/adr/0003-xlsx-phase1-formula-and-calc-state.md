# ADR-0003: xlsx Phase1 — formula-aware edits and calculation state

- Status: **Proposed** (revision 2)
- Date: 2026-11-16
- Depends on: [ADR-0001](0001-lossless-round-trip-architecture.md),
  [ADR-0002](0002-schema-driven-code-generation.md)
- Roadmap: [`docs/roadmap/xlsx.md`](../roadmap/xlsx.md) §4 (edit hazards), §6 (why this is
  Phase1; the owner accepted this ordering on 2026-11-16)

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
    visible worksheet, and refusal of every cell of a merged range.

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
  cell ref, kind (`normal`/`shared`/`array`/`dataTable`), `si`, `ref`, `ca`, the `c/@cm` of
  its cell, and the formula text. For shared members, the text is resolved through the master
  (§4.2).
- **Result ranges**: for `array` and `dataTable` anchors, the whole `ref`; every cell in it is a
  *result cell*.
- **`cm` anchors** (review m8): a cell is a dynamic-array anchor only when all of these hold:
  - it has `cm`;
  - its `f` has `t="array"`;
  - `cm` points at a `cellMetadata` block whose `futureMetadata` is `XLDAPR` with
    `fDynamic="1"`.

  Its `ref` is its spill range. Any other `cm` makes value edits to that workbook return
  `Err(UnknownCellMetadata)`: metadata Phase1 does not model is not guessed at.
- **Defined names**: `workbook.xml` `definedName` text, with `localSheetId`.
- **Calculation state**: whether `calcChain.xml` is present, and `calcPr`'s attributes.

The index lives as long as the `Workbook` and is updated by every call that changes formulas
(§4). Cost: one decompress and event scan per sheet, which Phase0 already pays.

### 2. Precedents: sound by default (review B1)

**Principle.** Every formula has a precedent set `P(f)`. If the analysis cannot show that
`P(f)` covers every cell the formula can read, then `P(f) = Everything`. Precise sets are an
**allowlist**: a formula gets one only if every token is a construct listed in §2.2, and every
rule of §2.3 is applied. "Not proven safe" therefore always means "depends on everything". It
never means "depends on what the text happens to mention".

#### 2.1 Tokenizer

The tokenizer is hand-written in `ooxml-xlsx`. It lexes, it does not evaluate. Its output is a
token stream: references, names, function calls with argument structure, operators, literals.

- It never panics (fuzz target).
- Any lexing failure gives `Everything` for that formula.
- It is checked differentially against openpyxl's `Tokenizer` on formula texts. Those texts are
  **committed** together with the expected tokens (review m7):
  - deduplicated texts from the 126 formula-bearing POI packages;
  - Apache-2.0, attributed in a `NOTICE` next to them;
  - capped at 1 MB by sampling if larger, and the sampling is recorded.

  No Python runs at test time.

#### 2.2 What can yield a precise precedent set

| Construct | Precedents |
|---|---|
| A1 reference or range with literal corners, optional sheet (`Sheet1!A1`, `'My Sheet'!A1:B2`, `$A$1`), whole columns and rows (`A:A`, `1:1`) | That rectangle on that sheet (the formula's own sheet if unqualified) |
| 3-D reference with literal sheet names (`Sheet1:Sheet3!A1`) | That rectangle on every sheet from `Sheet1` to `Sheet3` in workbook order |
| Union (`,` inside a reference argument) and intersection (space) of the above | The union of the operands' precedents (a superset for intersection) |
| A **defined name** whose text, tokenized by the same rules, is precise, and which has **no relative component** (every column and row part of every reference carries `$`) | That name's precedents. Local names (`localSheetId`) shadow global ones on their sheet. Recursion has a depth limit of 16; deeper gives `Everything` |
| A call to a function on the **value-function allowlist**, with arguments that are themselves precise | The union of the arguments' precedents |
| Literals and arithmetic, comparison and text operators | Nothing |

The **value-function allowlist** is a committed table: built-in function name → precise or
special rule.

- **Seed**: the built-in, non-`_xlfn.` functions in openpyxl's `FORMULAE`, minus every function
  that returns a reference used as one, reads outside its arguments, or is volatile in a way
  that matters (see below).
- **`_xlfn.` / `_xlws.` functions**: on the list only when individually reviewed, with a
  one-line reason each. Functions that return references (`_xlfn.XLOOKUP`, `_xlfn.ANCHORARRAY`,
  `_xlfn.SINGLE`, …) are **excluded**. `LET`/`LAMBDA` are excluded: their local names would
  otherwise look like unknown defined names, which already give `Everything`, so this only makes
  the rule explicit.
- **Everything not on the table gives `Everything`.** That covers VBA UDFs (`myFunc(A1)` in
  `testNames.xlsm`), add-in functions (`_xll.`), `INDIRECT`, `OFFSET`, `CELL`, `INFO`,
  `GETPIVOTDATA`, unknown names, structured references (`Table1[Col]`: resolution needs table
  parts; deferred to Phase5), and external references `[n]…`. For `[n]` the cached values
  belong to another workbook, but a link can point back at this file, so it is not assumed safe.
- `INDEX`, `CHOOSE`, `IF`, `IFS`, `SWITCH` **are** on the table: in value position their
  result lies inside their arguments' precedents. Their reference results are handled by the
  `:` rule below.

#### 2.3 Rules for the counterexamples of review B1

- **(a) Implicit resizing**:
  - `SUMIF(range, criteria, sum_range)` and `AVERAGEIF(…)`: the precedent from `sum_range` is
    the rectangle that starts at `sum_range`'s top-left cell and has `range`'s shape (Microsoft
    "SUMIF function"), unioned with `sum_range` itself.
  - If `range` or `sum_range` is not a precise rectangle, the result is `Everything`.
  - Any other function documented to resize an argument is kept off the allowlist until it has
    such a rule.
- **(b) Range operator with a computed side**: `A1:INDEX(…)`, `A1:CHOOSE(…)`, `Name1:Name2`,
  `A1:IF(…)`, and generally any `:` whose operand is not a literal cell reference, give
  `Everything`. The review's alternative, the bounding box of both sides, is sound but needs
  per-function knowledge that a reference result lies inside the arguments. `Everything` is
  simpler and the case is rare; it can be refined later with its own O1 tests.
- **(c) UDFs and add-ins**: not on the allowlist, so `Everything`.
- **(d) Data tables**: a `t="dataTable"` anchor and every result cell of its `ref` give
  `Everything`.
- **(e) Blocked spills**: a `cm` anchor whose cached value is `#SPILL!` (or whose cached `t="e"`
  value cannot be shown to be something else) gives `Everything`. Its result depends on the
  emptiness of cells no token names.
- **(f) Relative names**: a defined name with any reference component lacking `$` (row or
  column) gives `Everything` until a translation rule for "relative to the using cell" is
  specified and tested.
- **Volatile flag**: `f/@ca="1"` (or `true`) gives `Everything`. Volatile functions without `ca`
  (`NOW`, `RAND`, …) are on the allowlist with no precedents. Their caches are stale by nature,
  independent of any edit, and no edit makes them more stale.

Shared formulas: a member's precedents are the master's text **translated** to the member
(§4.2's translation, never calamine's lexical approximation), then analysed. If translating a
member fails, that member gives `Everything`.

### 3. Dependency-based cache invalidation for value edits

#### 3.1 Plan → validate → apply (review M2)

Every editing call (value write, formula write, formula removal, master promotion) runs in three
stages. Nothing in the `Workbook` changes unless all three succeed.

1. **Plan.**
   - Compute the dependent set `D` (§3.2).
   - Compute the full list of changes: the edited cell, `v`/`t` removals, the `calcPr` change,
     master promotion, `calcChain` removal.
2. **Validate.** Return `Err` before any change if any of these fails:
   - every guard of §5, including those that look at `D` (`RichValueCell`,
     `MacroSheetDependent`);
   - every insertion the plan needs is possible (e.g. `calcPr` next to an
     `mc:AlternateContent` sibling of `workbook` → `UnresolvedSibling`; 78/333 corpus
     `workbook.xml` have an AC child, though none of the 2 formula workbooks lacking `calcPr`
     does).
3. **Apply.**
   - Changes are made on **staged clones** of the touched `XmlDocument`s (`XmlDocument` is
     `Clone`), and the clones are swapped into the package only when every change succeeded.
   - A core primitive that fails mid-apply therefore leaves the package as it was.
   - The cost is one clone per touched part per call. Phase6 (large sheets) may replace this
     with a dry-run insertion API in core.

Test: after every `Err` in the suite, `save` gives L1 for every entry (as ADR-0001 R2-m1).

#### 3.2 Dependents

A value write (`set_number`, `set_string`, later Phase2 setters) to a cell X that is **not** a
formula cell and **not** a result cell:

1. Seeds = {X}.
2. Dependents = every formula `f` with `P(f) = Everything` or `P(f)` containing a seed.
3. Iterate to a fixed point:
   - a dependent formula cell becomes a seed;
   - a dependent array, data-table or spill anchor makes all of its result cells dependents and
     seeds.

The graph is built lazily from §1 on the first edit and cached.

#### 3.3 Changes

1. **Each dependent cell** (formula cell or result cell), in its sheet's Layer 2 tree:
   - remove `v`;
   - remove `t` **whatever its value** (`str`, `b`, `e`, `n`, and the LibreOffice
     `inlineStr`-under-`f` shape of `52575_main.xlsx`), because under a formula `t` describes
     only the cached value (review m2);
   - leave `f` and every other attribute untouched.

   T2's allowed region for a dependent is its **whole `c` span** (review m1):
   - a `t` removal re-renders the open tag;
   - a result cell that loses its only child `v` keeps its close tag per ADR-0002 §4.2, or
     becomes self-closing when it is also `own_dirty`. Either way only that `c`'s bytes change.

   Dependents on sheets other than the first visible one are written through the same Layer 2
   path. Phase1 opens any worksheet for writing (§6).
2. **`workbook.xml`** (Layer 2; write allowlist entry `x:CT_CalcProperties/x:calcPr`): set
   `fullCalcOnLoad="1"` on `calcPr`, inserting `calcPr` in its schema slot if absent.
   - An existing `1` or `true` is left as is (review m5).
   - In **manual** calculation mode (`calcMode="manual"`, 2 corpus files), `fullCalcOnLoad` is
     **not** set. That respects the user's choice, as rust_xlsxwriter does. Dependents still lose
     their caches, so no reader sees a stale value; Excel users press F9. Whether Excel shows
     blanks or zeros there until F9 is an XL-check item (§7.4).
3. **`calcChain.xml`** is not touched: the set of formula cells did not change, so every entry
   still names a formula cell.

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

Writes **into** a result cell (a non-anchor cell of an array, data-table or spill range) return
`Err(ArrayResultCell)`. Excel itself forbids editing part of an array.

### 4. Formula writes and removals

#### 4.1 Plain formulas

- **`set_formula(text)`** writes `<f>text</f>` with **no** `v` and no `t`.
  - The cell becomes a formula cell. Its dependents are invalidated as in §3, and
    `fullCalcOnLoad` is handled as in §3.3.
  - Text is stored as given, without a leading `=`.
  - Functions that need `_xlfn.` are the caller's responsibility in Phase1.
  - `set_array_formula` and dynamic arrays are out of scope (roadmap Phase5).
- **Overwriting a formula cell with a value**, or clearing its formula, removes `f`, writes the
  value and invalidates dependents.

#### 4.2 Shared groups (review M3)

**Translation.** Moving a formula text from cell A to cell B shifts every **relative** row and
column component of every reference token by (B − A):

- literal references and ranges, the reference parts of 3-D references, and relative components
  inside the formula, not inside names;
- quoted sheet names (`'Q1 2024'!A1`) are skipped as a unit;
- string literals are skipped.

Translation **fails** (`Err`, never "keep the original text", unlike calamine's out-of-bounds
behaviour) when:

- the text does not lex; or
- any shifted component leaves the sheet: row < 1 or > 1,048,576, column < A or > XFD.

`INDIRECT`, structured references and other `Everything` constructs are not a failure: they are
copied unchanged, and their precedents stay `Everything`.

**Overwriting a member** removes only that member's `f`.

**Overwriting the master**:

1. The new master is the next remaining member in row-major order.
2. Its `f` gets the master text translated from the old master to it.
3. The new `ref` is the **bounding rectangle of the remaining members**, the new master included.
   The new master need not be its top-left cell: Excel writes that shape itself
   (`51710.xlsx`). This equals POI's `onDeleteFormula` result when the remaining members fill
   that rectangle row-major from the new master. Otherwise POI's formula can exclude members and
   ours does not.
4. Every remaining member keeps its meaning. Its text is *new master text translated by (member −
   new master)*, which equals *old text translated by (member − old master)*, because
   translations compose when none fails. The plan checks that no member's translation from the
   new master fails.
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

This needs `Package::remove_part(name)` in `ooxml-core`. It is a separate, additive core change
with its own tests, and it is an **explicit, narrow exception** to ADR-0001 Layer 3's
"registries are append-only" invariant. On acceptance of this ADR, ADR-0001 gets a
post-acceptance note with this text:

> A registry entry may be removed only together with the part it registers, and only if no part
> in the package references the entry's id (a relationship id is referenced by `r:id`-style
> attributes in the source part; `calcChain`'s is referenced nowhere). No remaining entry is ever
> renumbered.

`remove_part` contract:

- **Precondition**: the part has **no relationships part of its own**, else `Err`; cascading
  deletion is out of scope. Its relationship id is referenced nowhere, else `Err`. For
  `calcChain`, ooxml-xlsx checks that `workbook.xml` has no attribute equal to the id in the
  `r` namespace.
- **Scope**: every `.rels` part in the package is scanned. Every internal relationship that
  resolves to the part is removed. The part's `Override` is removed; a `Default` is never
  touched.
- **Whitespace**: the removed element **and the whitespace-only text node immediately before
  it, if any**, are removed, so indented `.rels` and content-types parts stay tidy. T2's allowed
  diff for these parts is exactly that element plus that text node.

### 5. Guards

| Situation | Result |
|---|---|
| Write into a result cell of an array/data-table/spill range | `Err(ArrayResultCell)` |
| Write to an array/data-table anchor or a `cm` anchor (value or formula) | `Err(ArrayAnchor)` (array editing is Phase5) |
| Workbook has a `cm` that is not an XLDAPR dynamic-array anchor | `Err(UnknownCellMetadata)` for value edits (§1) |
| Write into a table header row | `Err(TableHeaderCell)` (as Phase0; header sync is Phase5) |
| Write to a `vm` cell, or an edit whose dependents include one | `Err(RichValueCell)` (as Phase0) |
| An edit would make a macro-sheet formula a dependent | `Err(MacroSheetDependent)`: XLM caches are not modelled |
| Write into any cell of a merged range (top-left included), missing `r`, Strict, encrypted | unchanged from Phase0 |
| A translation of §4.2 fails | `Err(SharedFormulaMaster)` |

Every guard is checked in §3.1's validate stage.

### 6. Scope changes to ADR-0002 §5/§6 (review m6)

Phase1 adds, as allowlist entries and code paths:

- **Layer 2 writes to any worksheet**, not only the first visible one, for dependent `v`/`t`
  removal and for writes through a new `Workbook::worksheet_by_name` / `worksheet_by_index`
  (roadmap Phase1 scope 1; index in workbook order, hidden sheets included).
- **Write** entries: `x:CT_CalcProperties/x:calcPr`, `x:CT_CellFormula/x:f` (moves from
  `[read]` to `[write]`).
- **Read** entries: `x:CT_DefinedNames/x:definedNames`, `x:CT_DefinedName/x:definedName`; the
  `metadata.xml` path needed for §1's `cm` rule (`cellMetadata`, `futureMetadata`, the XLDAPR
  extension element); the macro-sheet root for `MacroSheetDependent`.
- **Layer 2 writes to `workbook.xml`** (`calcPr` only), and to the workbook `.rels` and
  `[Content_Types].xml` through `remove_part` only.

Table-part reads already exist (Phase0 table-header guard).

### 7. Acceptance (testable)

All tests below sit on top of the Phase0 suite (roadmap §7). Every one checks T1/T1b on touched
parts, L1 on the rest, and "`Err` ⇒ nothing changed".

#### 7.1 Oracles that do not consult the implementation (review B2)

- **O1 — hand-written dependent sets.** Small synthetic workbooks, built by a committed
  generator script, each with an expected dependent set per edit, **written by hand** in the
  test. Required cases:
  - cross-sheet (ADR-0001 R2-M3's `Sheet2!B1 = Sheet1!A1*2`, edited on `Sheet1`);
  - a local and a global name with the same name;
  - a 3-D reference, `A:A` and `1:1`;
  - a three-step transitive chain across sheets;
  - each of §2.3 (a)–(f), plus `ca="1"`, a UDF call, `[1]` external;
  - an array formula and its result cells;
  - formulas that must **not** become dependents. Their cached `v` must survive byte-identical,
    so a "clear everything" implementation fails too.
- **O2 — perturbation oracle (soundness).** A property test:
  - It generates random workbooks in an evaluable subset: literal references and ranges,
    cross-sheet and 3-D references, `+ - * /`, `SUM`, `MIN`, `MAX`, `IF`, `INDEX`, `SUMIF` with
    mismatched shapes, absolute names. Generation works from an AST, and the formula text is
    rendered from it.
  - A **test-only evaluator over the AST** gives ground truth. It never sees the tokenizer under
    test.
  - For each random edit of X, it evaluates the workbook with X set to several different values.
    Every formula whose value changes for any of them **must** be in the computed dependent set
    (computed ⊇ observed).
  - Over-approximation is allowed. A missed dependent fails the test with a shrunk
    counterexample.
- **O3 — recalculation oracle on real files.** For each acceptance edit on a real fixture, the
  output is recalculated **offline** by LibreOffice headless with hard recalc
  (`OOXMLRecalcMode`=always), using a script in `spikes/xlsx/`. The recalculated value of every
  formula cell is committed as expected data. The test checks that every formula cell that
  **kept** its `v` holds the recalculated value.
  - Cells where LibreOffice and Excel are known to differ are listed with a reason.
  - LibreOffice is not needed at test time.
  - It is not available in the current sandbox, so generating the data is an owner or CI step
    (see Open questions).
- **Mutation check.** Each rule of §2.3 and each special case of §2.2 is behind an internal
  switch in test builds. Turning any one of them off must fail at least one O1 or O2 test.
  `cargo-mutants` on the precedent module is a complement, not a replacement.
- Every edit writes a value that **changes** the dependents' results (never the same value
  back), so a missed dependent cannot hide.

#### 7.2 Fixture tests

1. **Value edit, shared formulas** — `shared_formulas.xlsx`: write a precedent of the group.
   - T2 confined to the edited cell, the **O1-listed** dependents' `c` spans, and `calcPr`.
   - CR: dependents read empty, others unchanged. `worksheet_formula` is identical for all
     formula cells.
   - `calcChain.xml` L1. O3 holds.
2. **Array results** — `MatrixFormulaEvalTestData.xlsx`:
   - writing a precedent of an array formula clears `v` from every cell of its `ref`;
   - writing a non-anchor result cell → `Err(ArrayResultCell)`. O3 holds.
3. **Dynamic array** — `xlookup.xlsx`:
   - writing into the spill range → `Err(ArrayResultCell)`;
   - writing a precedent: the anchor and the spill cells lose `v`; `metadata.xml` L1. O3 holds.
4. **No calcChain** — `70005-countifs.xlsx` (LibreOffice, data descriptors): the value edit
   succeeds; no `calcChain.xml` is created; `fullCalcOnLoad` is set on the existing `calcPr`.
5. **Conservative path** — one POI file with `INDIRECT` is added as a fixture: every formula
   cell becomes a dependent.
6. **Formula write + calcChain removal** — `shared_formulas.xlsx`: `set_formula` on an empty
   cell.
   - The output has no `calcChain.xml`.
   - The workbook `.rels` and `[Content_Types].xml` each lose exactly one element plus its
     preceding whitespace text (§4.3).
   - All other parts L1.
7. **Master promotion**:
   - `shared_formulas.xlsx` (1-D group);
   - `NewStyleConditionalFormattings.xlsx`, 2-D group `D2:U17`: after the master `D2` is
     overwritten, `ref="D2:U17"` and the master is `E2`;
   - `51710.xlsx`, added as a fixture: master not at the top-left;
   - a synthetic group with a quoted sheet name `'Q1 2024'!A1` and a relative reference that
     would leave the sheet (→ `Err(SharedFormulaMaster)`, nothing changed).

   Oracle: openpyxl's `Translator` output for every remaining member, generated offline and
   committed. calamine's expansion is compared too. Where the two disagree (e.g. calamine
   shifting inside `'…'`), a hand-written expectation decides. O3 also runs on the promoted
   outputs.
8. **Tokenizer** — no panic on every committed formula text and under `cargo fuzz` for a fixed
   time budget. Reference tokens agree with the committed openpyxl expectations.
9. **Manual calculation mode** — a synthetic `calcMode="manual"` workbook: dependents lose `v`,
   and `fullCalcOnLoad` is not added.

#### 7.3 Failure atomicity

For each `Err` case above (and an injected failure in the apply stage), `save` after the error
gives L1 for every entry.

#### 7.4 XL (owner, Microsoft 365 Excel)

- The outputs of 7.2 items 1, 2, 3, 6, 7 and 9 open without a repair dialog and show
  recalculated values.
- For item 9, record what Excel shows before F9.

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
7. **Keep Phase0's refusal and do registries first.** Leaves the majority of real workbooks
   uneditable. See roadmap §6 for the value × risk comparison; the owner chose this ADR's order.

## Consequences

- Positive:
  - Editing works on formula-bearing workbooks, which is most real ones, and no reader ever sees
    a stale value.
  - Untouched formulas keep their bytes and cached values whenever their precedents are
    precise.
  - The dependency rule fails safe: a gap in the allowlist costs cached values (visible
    blanks), never correctness.
  - Three oracles, independent of the implementation, check the one failure that matters (a
    missed dependent).
- Positive: the tokenizer, translation and dependency graph are reusable by Phase3 (sheet rename
  rewrites), Phase7 (row/column insertion) and any future evaluator.
- Negative: dependents lose their cached values until a calculating application recalculates.
  Read-only pipelines that open the output directly see blanks for those cells. This is honest
  but visible. `KeepStaleCaches` exists for Excel-only workflows.
- Negative: allowlist-by-default is conservative. One unknown function (a UDF, an unreviewed
  `_xlfn.`) or one `INDIRECT` makes every edit invalidate that formula and everything
  downstream of it.
  - The review's heuristic estimate: about 20 of 127 formula workbooks have at least one such
    formula.
  - Growing the value-function table, reviewed one function at a time with O1/O2 tests, reduces
    this.
- Negative: removing `calcChain.xml` brings two Layer 3 registry parts into Layer 2's write
  scope, under a written exception to ADR-0001's append-only invariant (§4.3). The core
  `remove_part` API is a prerequisite.
- Negative: O3 needs LibreOffice somewhere (not in this sandbox), and LibreOffice is not Excel.
  The XL check remains the last word for Excel behaviour.
- Negative: Excel asks "save changes?" on close for files with `fullCalcOnLoad` set.

## Open questions

1. Should `KeepStaleCaches` exist at all, or should callers who want it use `fullCalcOnLoad`
   alone through a lower-level API?
2. Where is O3's expected data generated: on the owner's machine (LibreOffice installed), or in
   a CI job with LibreOffice that only regenerates on demand?
3. Threshold behaviour: if the dependent set exceeds N cells, is a warning (a returned report)
   wanted?
4. Manual calculation mode (§3.3 step 2): confirm "do not set `fullCalcOnLoad`" after the XL
   check of §7.4.

(Revision 1's open question 2, the `_xlfn.` allowlist, is answered by §2.2: start from
non-`_xlfn.` built-ins, add `_xlfn.` functions individually, and never add reference-returning
ones.)

## Sources

- ADR-0001: Layer 3 "Phase0 policy", R2-M3, R3-M1, and the 2026-11-16 notes. ADR-0002: §4.2
  primitives, §4.3, §5 allowlist, §6. Review: `docs/adr/0003-opus-review-round1.md`.
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
