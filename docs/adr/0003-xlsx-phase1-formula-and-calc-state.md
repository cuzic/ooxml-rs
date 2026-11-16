# ADR-0003: xlsx Phase1 — formula-aware edits and calculation state

- Status: **Proposed**
- Date: 2026-11-16
- Depends on: [ADR-0001](0001-lossless-round-trip-architecture.md),
  [ADR-0002](0002-schema-driven-code-generation.md)
- Roadmap: [`docs/roadmap/xlsx.md`](../roadmap/xlsx.md) §4 (edit hazards), §6 (why this is
  Phase1)

## Context

ADR-0001's Phase0 refuses **every** value edit to a workbook in which any worksheet contains a
formula (its R2-M3/R3-M1 fix). The reason is sound: a value edit can leave a formula's cached
result (`c/v`) wrong on a sheet that ooxml-rs never parsed, and readers other than Excel
(calamine, pandas, openpyxl with `data_only=True`) return that wrong value with no error. ADR-0001
ends that section with an explicit open decision: "A future phase must pick, and write down, one
of: delete `calcChain.xml` … whenever a formula cell is touched, set `<calcPr
fullCalcOnLoad="1"/>` …, or incrementally maintain the dependency graph."

How much this blocks, measured (`spikes/xlsx/formula_stats.py`, Apache POI
`test-data/spreadsheet` @ `47dcff76`): **126 of 341** readable Transitional packages (37%)
contain formulas. Of those, 115 have `calcChain.xml` and 11 do not (LibreOffice- and POI-written
files). 48 use shared formulas, 6 array formulas, 1 a dynamic array. 13 use defined names in
formulas, 10 structured references, 23 cross-sheet references, 6 external references `[n]`, 2
`INDIRECT`/`OFFSET`. The corpus is a test corpus, so the real-world share of formula-bearing
workbooks is probably higher (inference, not measured).

What the reference implementations do:

- **rust_xlsxwriter** writes formulas with a cached result of 0 unless told otherwise, never
  writes `calcChain.xml`, and sets `calcPr fullCalcOnLoad="1"` (`src/workbook.rs`). It never
  writes shared formulas.
- **openpyxl** drops `calcChain.xml` on every save and writes formulas without cached values
  (cached values are only available by loading with `data_only=True`, which drops formulas
  instead). It expands shared formulas into plain ones on read (`worksheet/_reader.py`,
  `Translator`).
- **umya-spreadsheet** drops `calcChain.xml` and does not write `fullCalcOnLoad`.
- **Apache POI** keeps `calcChain.xml` and removes an entry when a formula is removed from a
  cell (`XSSFWorkbook.onDeleteFormula` → `CalculationChain.removeItem`, which moves an inherited
  `i` sheet id onto the next entry), and when a shared-formula master loses its formula it
  promotes the next member of the group to master with translated text (`XSSFSheet.onDeleteFormula`).
- **Excel**: "Found unreadable content" repairs are reported when `calcChain.xml` lists a cell
  that holds no formula (the reason POI maintains it). Excel opens files without `calcChain.xml`
  routinely (every openpyxl- and rust_xlsxwriter-written file).

Facts from the spec that shape the design (ECMA-376 Part 1, §18.3.1.40 `f`, §18.6 calcChain,
§18.2.2 `calcPr`):

- Only the **anchor** cell of an array formula (`f t="array" ref="…"`) or data table
  (`t="dataTable"`) carries `f`. The other cells of `ref` carry only `v`, which is still a
  computed result. Dynamic-array spills are stored the same way, with `c/@cm` on the anchor
  pointing into `metadata.xml`. **A census of `f` elements alone misses these result cells.**
- A shared-formula group's text is stored only on its master (`t="shared" si="n" ref="…"` with
  text); members carry `t="shared" si="n"` with no text and are interpreted relative to the
  master.
- `calcChain` entries omit `i` when it equals the previous entry's sheet id.
- `v` is optional under a formula: a formula cell without `v` is valid; Excel computes it.

## Decision

### 1. Census at open (replaces Phase0's yes/no scan)

At `open`, every worksheet and macro-sheet part is scanned (quick-xml events, no Layer 2 tree,
parts stay `Raw`/dirty-free `Parsed`, ADR-0001 M2) and a `FormulaIndex` is built:

- Formula cells: elements with QName (SML main, `f`). **Matching is by QName**, never by the
  byte prefix `<f`, which also matches `<formula>`, `<from>`, `<filterColumn>` (roadmap §5).
- For each: sheet, cell ref, kind (`normal`/`shared`/`array`/`dataTable`), `si`, `ref`, `cm`,
  and the formula text (for shared members: resolved through the master, see §3).
- **Result ranges**: for `array` and `dataTable` anchors, the whole `ref`; every cell in it is a
  *result cell*. A cell with `cm` is treated as a dynamic-array anchor; its `ref` is its spill
  range.
- Defined names (`workbook.xml` `definedName` text, with `localSheetId`).
- `calcChain.xml` presence and the `calcPr` element's attributes.

The index is cached for the `Workbook`'s life and updated by every formula-changing call (§4).
Cost: one decompress + event scan per sheet, which Phase0 already pays.

### 2. Reference-only formula tokenizer

A hand-written tokenizer in `ooxml-xlsx` (not an evaluator) that turns formula text into
**precedents**:

- `Cell`/`Range` with optional sheet (`Sheet1!A1`, `'My Sheet'!A1:B2`, `$A$1`), whole columns
  and rows (`A:A`, `1:1`), 3-D references (`Sheet1:Sheet3!A1`, expanded via workbook sheet
  order);
- `Name(n)` for defined names; resolved through §1's name table, recursively, with a depth
  limit; a name whose text is itself only references resolves to them;
- `Everything`, the **conservative token**, produced by: `INDIRECT`, `OFFSET`, `INDEX` used with
  a reference result, `CELL`, `INFO`, any `_xlfn.`/`_xlws.` function not on an allowlist of
  value-only functions, structured references (`Table1[Col]`; resolving them needs table parts —
  deferred), external references `[n]…` (their cached values belong to another workbook, but
  `[0]` means this one), unknown names, a name defined with a non-reference expression, and any
  text the tokenizer fails to lex;
- strings, numbers, operators, function names: ignored for dependency purposes.

Shared formulas: a member's precedents are the master's precedents shifted by the member's
offset (relative parts only), the same translation calamine performs in `expand_shared_formula`.

The tokenizer must never panic on any input (fuzz target), and returns `Everything` rather than
an error for anything it does not understand. Correctness is checked differentially against
openpyxl's `Tokenizer` (a port of the widely used Bachtal tokenizer) on every formula text in the
POI corpus, with expected token streams generated offline and committed as test data (no Python
at test time).

### 3. Dependency-based cache invalidation for value edits

A value write (`set_number`, `set_string`, later P2 setters) to a cell X that is **not** a
formula cell and **not** a result cell:

1. Seeds = {X}. Dependents = every formula whose precedents contain a seed, or `Everything`;
   iterate to a fixed point (a dependent formula cell becomes a seed; an array/dataTable/spill
   anchor that becomes dependent makes all of its result cells dependents too). The graph is
   built lazily from §1 on the first edit and cached.
2. For every dependent cell, in its sheet's Layer 2 tree: remove `v`, and remove `t` when it
   described the cached result (`str`, `b`, `e`; a numeric result has no `t`). A dependent that
   carries `vm` is not modified; the edit is refused instead (§5). The `f` element and all other
   attributes stay byte-identical (it is a child removal, so only that `c` is `subtree_dirty`;
   `own_dirty` only for the `t` removal).
3. In `workbook.xml` (Layer 2, new write allowlist entry `x:CT_CalcProperties/x:calcPr`): set
   `fullCalcOnLoad="1"` on `calcPr`, inserting `calcPr` in its schema slot if absent.
4. `calcChain.xml` is **not touched**: the set of formula cells did not change, so every entry
   still names a formula cell.

Consequences for readers: a dependent formula reads as *no cached value* (calamine: empty), never
as a stale one. Formulas that do not depend on X keep their cached values and their bytes.
Excel, and any reader that honours `fullCalcOnLoad`, recomputes everything on open.

An opt-in `CalcPolicy::KeepStaleCaches` skips step 2 (still sets `fullCalcOnLoad`), for callers
who will only open the result in Excel and want minimal diffs. It is never the default.

Writes **into** a result cell (non-anchor cell of an array, data-table or spill range) return
`Err(ArrayResultCell)`. Excel itself forbids editing part of an array.

### 4. Formula writes and removals

- `set_formula(text)` on a cell: writes `<f>text</f>` with **no** `v` (and no `t`); the cell
  becomes a formula cell; dependents of the cell are invalidated as in §3; `fullCalcOnLoad` set.
  Text is stored as given (no leading `=`); functions that need `_xlfn.` are the caller's
  responsibility in Phase1 (rust_xlsxwriter's automatic prefixing is a later convenience).
  `set_array_formula` and dynamic arrays are out of scope (roadmap Phase5).
- Overwriting a formula cell with a value (or clearing its formula) removes `f`, writes the
  value, invalidates dependents.
- Shared groups: overwriting a **member** removes only that member's `f`. Overwriting the
  **master** first promotes the next member in row-major order inside `ref` to master — its `f`
  gets the translated text and a `ref` covering the remaining group, exactly as POI's
  `XSSFSheet.onDeleteFormula` — then proceeds. If the master's text yields `Everything` during
  translation (unlexable), the write returns `Err(SharedFormulaMaster)` instead.
- Writes that change the formula-cell set (any of the above) make `calcChain.xml` stale.
  **Decision: remove the part** — the `calcChain.xml` part, its relationship in the workbook
  part's `.rels`, and its `Override` in `[Content_Types].xml` — rather than maintaining entries.
  Rationale: Excel rebuilds the chain on load; files without it are the norm from openpyxl and
  rust_xlsxwriter; a partially maintained chain is the one state known to trigger repair, while
  an absent one is known not to. POI-style entry maintenance (`removeItem` with `i` inheritance)
  remains possible later if a user needs the chain kept.
- This needs one new `ooxml-core` capability: `Package::remove_part(name)` that removes the part,
  every relationship targeting it from the source part's `.rels` (Layer 2 edit of the rels
  part), and its content-type `Override`. It is format-agnostic, so it belongs to core
  (ADR-0001 Layer 3 ownership); this ADR only states the requirement.

### 5. Guards that stay or are added

| Situation | Result |
|---|---|
| Write into a result cell of an array/data-table/spill range | `Err(ArrayResultCell)` |
| Write to an array/data-table anchor or a `cm` anchor (value or formula) | `Err(ArrayAnchor)` (array editing is Phase5) |
| Write into a table header row | `Err(TableHeaderCell)` (roadmap Q5; header sync is Phase5) |
| Write to a `vm` cell, or an edit whose dependents include one | `Err(RichValueCell)` (roadmap Q4) |
| Workbook contains a macro sheet with formulas, and an edit would make one of its formulas a dependent | `Err(MacroSheetDependent)`: XLM caches are not modelled |
| Write into a merged non-anchor cell, missing `r`, Strict, encrypted | unchanged from Phase0 |

### 6. What Phase1 does not do

No evaluation (dependent caches are removed, not recomputed); no structured-reference
resolution (treated as `Everything`); no chart cache (`c:numCache`) refresh — roadmap §4 records
it as a documented staleness; no pivot cache refresh (Excel-normal); no external-workbook
updates.

### 7. Acceptance (testable)

On top of the Phase0 suite (roadmap §7), all with T1/T1b on touched parts and L1 on the rest:

1. **Value edit, shared formulas** — `shared_formulas.xlsx`: write a value into a precedent of
   the shared group. Byte diff confined to the edited cell, the `v`/`t` of exactly the cells the
   oracle predicts, and `calcPr`. calamine: dependents read empty, others unchanged;
   `worksheet_formula` identical for all formula cells. `calcChain.xml` L1.
2. **Array results** — `MatrixFormulaEvalTestData.xlsx`: write a precedent of an array formula;
   all cells of its `ref` lose `v`. Write into a non-anchor result cell → `Err(ArrayResultCell)`.
3. **Dynamic array** — `xlookup.xlsx`: write into the spill range → `Err(ArrayResultCell)`;
   write a precedent → anchor and spill cells lose `v`; `metadata.xml` L1.
4. **No calcChain** — `70005-countifs.xlsx` (LibreOffice, data descriptors): value edit succeeds;
   no `calcChain.xml` is created; `fullCalcOnLoad` set on the existing `calcPr`.
5. **Conservative path** — a formula using `INDIRECT` (POI corpus files with `volatile_ref`
   counted in `formula_stats.py`, one to be added as a fixture): every formula cell in the
   workbook becomes a dependent.
6. **Formula write + calcChain removal** — `shared_formulas.xlsx`: `set_formula` on an empty
   cell; output has no `calcChain.xml`, workbook rels and `[Content_Types].xml` each lose exactly
   one element (byte diff confined to it); all other parts L1.
7. **Master promotion** — overwrite a shared master; the next member's `f` gets text equal to
   calamine's expanded formula for that cell in the input, and `ref` covering the rest; calamine
   expansion of every remaining member is unchanged.
8. **Tokenizer** — no panic on all formula texts in the POI corpus and under `cargo fuzz` for a
   fixed time budget; reference tokens agree with the committed openpyxl-derived expectations.
9. **XL** (owner): outputs of 1, 2, 3, 6, 7 open in Excel without repair and show recomputed
   values.

## Alternatives considered

1. **Delete `calcChain.xml` and strip every cached value in the workbook on any edit**
   (openpyxl's effective behaviour). Simple and safe for Excel, but it destroys every cached
   value a non-Excel reader relies on, even for formulas unrelated to the edit, and touches every
   formula cell in every sheet, defeating ADR-0001's locality (T2). Kept as the behaviour that
   §3 degrades to when the analysis says `Everything`, not as the default.
2. **Only set `fullCalcOnLoad="1"`, keep stale caches** (what ADR-0001 listed as one option, and
   close to rust_xlsxwriter's stance). Excel is fine; calamine/pandas silently read wrong values.
   This is the failure ADR-0001 R2-M3 was written to prevent, so it is only offered opt-in
   (`KeepStaleCaches`).
3. **Recompute dependents (embed an evaluator).** The only option that keeps every reader
   correct *and* keeps caches. Excel's function library is enormous (hundreds of functions,
   locale and version-dependent behaviour); this is a project of its own (roadmap C8). Rejected for
   Phase1; the dependency graph built here is the part an evaluator would reuse.
4. **Maintain `calcChain.xml` entries (POI's approach) instead of removing the part.** Keeps the
   part, but requires correct `i` inheritance handling and ordering, and gives no benefit Excel
   needs (it rebuilds the chain). May be added later behind the same API if a user needs it.
5. **Keep Phase0's refusal and do registries first.** Leaves the majority of real workbooks
   uneditable; see roadmap §6 for the value × risk comparison.

## Consequences

- Positive: editing works on formula-bearing workbooks, which is most real ones, without any
  reader ever seeing a stale value; untouched formulas keep bytes and cached values.
- Positive: the tokenizer and dependency graph are reusable by Phase3 (sheet rename rewrites),
  Phase7 (row/column insertion) and any future evaluator.
- Negative: dependents lose their cached values until Excel (or another calculating
  application) recalculates. Read-only pipelines that open the output directly will see blanks
  for those cells. This is honest but visible; `KeepStaleCaches` exists for Excel-only workflows.
- Negative: conservative tokens (`INDIRECT`, structured references, unknown names) make a single
  such formula turn every edit into "invalidate all formulas". Structured-reference resolution
  (Phase5, with tables) reduces this.
- Negative: removing `calcChain.xml` when formulas change means two Layer 3 registry parts
  (`[Content_Types].xml`, workbook `.rels`) are edited, so they enter Layer 2's write scope. The
  core `remove_part` API is a prerequisite.
- Negative: whether LibreOffice computes formula cells that have no cached value on load under
  its default "never recalculate" setting for Excel 2007+ files is **unverified**; to be checked
  manually before acceptance.

## Open questions

1. Should `KeepStaleCaches` exist at all, or should callers who want it be told to use
   `fullCalcOnLoad` alone via a lower-level API?
2. Allowlist of "value-only" `_xlfn.` functions for the tokenizer: start empty (every `_xlfn.`
   is conservative) or seed from rust_xlsxwriter's future-function list?
3. Threshold behaviour: if the dependent set exceeds N cells, is a warning (via a returned
   report) wanted?

## Sources

- ADR-0001 (Layer 3 "Phase0 policy", R2-M3, R3-M1), ADR-0002 (§4.2 primitives, §5 allowlist).
- ECMA-376 Part 1, 5th ed. (2016): §18.3.1.40 (`f`), §18.6 (calculation chain), §18.2.2
  (`calcPr`), §18.17 (formulas).
- Apache POI @ `47dcff769bb395034dddfd59c4609659b6dbc6a4`:
  `poi-ooxml/src/main/java/org/apache/poi/xssf/usermodel/XSSFWorkbook.java` (`onDeleteFormula`),
  `poi-ooxml/src/main/java/org/apache/poi/xssf/model/CalculationChain.java` (`removeItem`),
  `poi-ooxml/src/main/java/org/apache/poi/xssf/usermodel/XSSFSheet.java` (`onDeleteFormula`,
  shared-master promotion).
- rust_xlsxwriter 0.102.0 `src/workbook.rs` (`fullCalcOnLoad`), `src/formula.rs`.
- calamine 0.38.0 `src/xlsx/mod.rs` (`expand_shared_formula`).
- openpyxl 3.1.5 `openpyxl/worksheet/_reader.py` (shared formula translation),
  `openpyxl/formula/tokenizer.py`, `openpyxl/workbook/properties.py` (`CalcProperties`).
- umya-spreadsheet 3.1.1 `src/writer/xlsx/workbook.rs` (`calcPr`).
- Corpus measurements: `spikes/xlsx/formula_stats.py`; fixtures in `reference/fixtures/xlsx/`.
