# ADR-0023: xlsx range reads and writes at workbook scope, and the Python binding policy

- Status: **Proposed** (revision 4)
- Date: 2026-11-27
- Repository baseline: `213e27a` (main after PR #170)
- Depends on:
  - [ADR-0001](0001-lossless-round-trip-architecture.md): Layer 1–3, T1/T1b/T2, `Err` ⇒
    byte-identical output, fail closed.
  - [ADR-0003](0003-xlsx-phase1-formula-and-calc-state.md): the census and its edit block
    (§1), cache invalidation of dependents and `fullCalcOnLoad`, plan → validate → apply with
    poisoning (§3.1), the shared-formula hand-off (§4.2), the array/spill guards (§5), the
    P5-pre batch `write_cells`.
  - [ADR-0006](0006-xlsx-phase2-shared-strings-and-styles.md): the shared-string table and
    the style registry (number formats, `EnsureDate`).
  - [ADR-0009](0009-xlsx-phase3-sheet-operations.md): sheet lookup by name and position.
  - [ADR-0014](0014-xlsx-phase4-range-features.md) §3: merged ranges; writes to the anchor are
    allowed, writes to other cells refused.
  - [ADR-0018](0018-xlsx-phase7-structural-edits.md): structural edits exposed by the binding;
    G2 `copy_sheet` (out of scope here).
  - **ADR-0021** (browser XLSX APIs) and **ADR-0022** (Rust worksheet projection),
    both Proposed; their Rust work has landed on `main` (PR #170). This ADR reuses their Rust
    API names and follows their rules: Rust owns parsing, edit guards, formula-cache policy
    and serialization; adapters own API shape and value projection; native and Wasm entry
    points share classifiers and guards; an incomplete classification fails closed; never
    invent cached results; native poisoning is not rollback.
  - [ADR-0024](0024-xlsx-edit-journal.md) (Accepted): the Edit Journal. Range batches are
    journal-op batches (§4.1 multi-op calls, validated per §4.5); unchanged-cell skipping is
    R1/R2 (§7.1); reads in a session go through the View in normalized mode (§6.1); rollback
    is `truncate(seq)` (§10.3) with save-during-transaction refused (Q5 (A)); the J1/J2/J3
    delivery order gates the units below. Owner decision Q4 required this rewrite before
    round 2 (revision 3).
- Roadmap: [`docs/roadmap/xlsx.md`](../roadmap/xlsx.md) (no phase number: this is an API
  layer over Phase0–Phase7, not a new format phase).
- Review: [round 1](0023-opus-review-round1.md) (not converged: M1–M9, m1–m13); [round 2](0023-opus-review-round2.md) (not converged: M1, m1–m4); [round 3](0023-opus-review-round3.md) (not converged: M1, M2, m1–m3).

## Revision history

- **Revision 1**: initial draft.
- **Revision 2** (review round 1: M1–M9, m1–m13). Each fix is in its section; the mapping is
  in "Review response" at the end.
  - **M1:** a transaction has an owner thread; other threads' calls on the handle wait (GIL
    released) until it ends, so a rollback can only discard the owner's own edits (§11.2).
  - **M2:** exception categories are keyed by `(crate, kind)`; core `Io` maps to `OSError`
    subclasses; U1 audits caller-input errors raised as `Malformed`/`InvalidValue` (§8,
    §11.3).
  - **M3:** `clear_range` and the skip-unchanged batch clear or replace a **whole** shared
    group in one plan (multi-row), and still refuse a partial group (§6.3).
  - **M4:** in Python, `None` means "leave this coordinate unchanged"; clearing needs the
    `ooxml_rs.CLEAR` sentinel or `clear()`. `write_range` skips values equal to what the
    cell already reads as, so a read → modify → write round trip changes only edited cells
    (§6.2, §11.4).
  - **M5:** `RangeCell` always records formula presence, `cm`, `vm` and storage; classification
    runs on `RangeRead` and returns the blocking error when roles are incomplete (§5.1, §9).
  - **M6:** `CellRole` is a struct with independent merge and formula roles (§5.1).
  - **M7:** name resolution is a new small resolver sharing only the fold and the parsed
    expression with the precedents analysis; `AmbiguousName` is distinct; names with an
    invalid `localSheetId` make a match ambiguous (fail closed) (§7).
  - **M8:** a number format is classified as date, time, date-time or elapsed (`DateKind`);
    reads and writes follow the kind; elapsed formats read as durations (§5.2, §6.1).
  - **M9:** decided (no longer Q3): the binding is a separate Cargo workspace under
    `bindings/python/`, outside `crates/*`, with its own CI job; the root workspace and the
    AGENTS.md commands are unchanged (§10).
  - Minor fixes m1–m13 in their sections.
- **Revision 3** (review round 2: M1, m1–m4; owner decision Q4, ADR-0024 §13.4).
  - **M1:** rebuilt on the Edit Journal: §6.2/§6.3 batches are journal-op batches
    (multi-op calls); unchanged-cell dropping shares C1's per-target report with R1
    (no bespoke comparison); reads in a session go through the View in normalized mode
    (§5); `fork()` transactions become `truncate(seq)` with Q5 (A) (§11.2); Delivery
    re-sequenced after the J series.
  - **m1:** multi-group plans: the clearing set holds several groups; replaced-group
    detection stays in the C1 plan machinery shared with the journal append path.
  - **m2:** the read cell struct is named `RangeCell` (`streaming::ReadCell` already exists).
  - **m3:** `CLEAR` and `clear()` map to `ClearValue`, which never creates blank cells.
  - **m4:** baseline, sequencing and U5 ownership updated (ADR-0021 has landed).
- **Revision 4** (review round 3: M1, M2, m1–m3).
  - **M1:** the drop predicate is a reads-only state-equality extracted from C1's §4.4
    core and shared with R1 (not C1's post-validation report, which cannot see past the
    guards); the F13 relation is stated (no divergence: the sequential twin drops
    identically). `write_changed_cells`/`clear_range` exist on both `Workbook` (sequential
    twin, part A) and `Journal`.
  - **M2:** `formulas="formula"` returns `ooxml_rs.Formula` objects (round-trippable; Q8
    already requires explicit `Formula()` for writing); value-mode writes back against a
    formula cell drop on cache equality (a 0023-specific rule, sequential twin identical);
    identical formula writes drop via the shared report. Oracle 3 pins formula preservation.
  - **m1:** `clear_range` uses `ClearValue`; U2 adds the missing `CellCall` payloads with
    sequential references into oracle 1.
  - **m2:** cross-reference fixed to ADR-0024 §10.3. **m3:** Sources updated.

## Context

### Demand

The demand comes from PAD/UiPath migration (RPA tools) needing lossless, Office-free editing
that openpyxl/python-docx/python-pptx cannot give. Those tools run Excel, Word and PowerPoint
actions ("read range", "write range", "append", "get first free row", "read named cell",
"insert rows", "replace text", …). A migrated flow has to run without Office, and usually in
Python. openpyxl, the usual choice there, drops charts, drawings, form controls, x14
conditional formats and validations, and every formula's cached value
([`docs/roadmap/xlsx.md`](../roadmap/xlsx.md) §2). It also does not rewrite references when
rows or columns are inserted. ooxml-rs keeps unchanged parts as they are and already
rewrites references (ADR-0018 G1). It lacks two things, which this ADR adds:

1. **Owned, batch-shaped xlsx reads and writes at workbook scope.** Today a caller walks
   `Worksheet::cells()` and builds rows itself. There is no `used_range`, `read_range`,
   `first_free_row`, public defined-name struct, name resolution or stable error identifier.
   `Worksheet<'wb>` holds `&'wb mut Workbook`, and `CellRef<'a>`/`CellMut` borrow from it, so
   none of them can be held by a foreign-language object. A binding can hold only the
   `Workbook`.
2. **A Python binding.** The repository has no PyO3 or maturin configuration.

### What exists already, and what is pending

Read on current `main` (`213e27a`, post-#170). The ADR-0021/0022 Rust work has landed, so
the right column names shipped APIs:

| Need | Pre-existing | Landed in PR #170 |
|---|---|---|
| Batch write | `Worksheet::write_cells(&[(CellIndex, CellWrite)])`, all or nothing; `CellWrite::{Number, String, Formula, RemoveFormula}` | adds `CellWrite::{IsoDate, Error, Bool, Clear, FormulaWithResult}` and `write_cells_with_styles`; `clear_shared_formula_row` (one-row groups, `ClearingFormulaGroup::Shared`) |
| Single date write | `CellMut::set_datetime(ExcelDateTime)` (serial in the workbook's system, `EnsureDate` style rule) | `CellMut::set_iso_date` (`t="d"`) |
| Extent | — | `Worksheet::declared_range`, `declared_bound`, `stored_extent` |
| Classification | — | `CellRef::has_complex_value`, `has_non_scalar_content`, `ordinary_formula`, `shared_string_index`; `Worksheet::non_scalar_ranges`, `formula_result_ranges`, `array_formulas`, `shared_formula_at`, `translate_shared_formula`, `is_date_style`, `row_numbers`, `cells_before_row` |
| Defined names | private `sheet_ops::defined_names` | `Workbook::defined_name_nodes` (returns `RawNode`), `global_defined_names`, `global_defined_name_formulas`, `set_defined_name(s)` (scope `Option<u32>`), `set_global_defined_name(s)` |
| Sheets | `worksheet_by_name` (exact, then unique case-insensitive), `worksheet_by_index` (position in `<sheets>`), `sheet_names` | `worksheet_names`, `worksheet_identities` |
| Recovery | `Error::Poisoned`, `Workbook::edit_refusal` | `Workbook::fork`, `Package::fork`, `Package::generation` |
| Scalar classifier and batch I/O | — | **Wasm-private** in `ooxml-xlsx-wasm`: `classify_sheet`, `classify_cell`, `classify_scalar_value`, `classified_value`, `check_excel_edit`, `scalar_cell_write`, `unsupported`, the generation-keyed `scalar_read_ranges` cache; batch functions `sparse_cells`, `csv_scalar_cells`, `write_scalar_batch`, `write_excel_scalar_batch`, `defined_names`; errors reach JS as `to_string()` and adapter codes (`INVALID_VALUE`, `CELL_EDIT_REFUSED`, …) |

So about half of the request is pending in the ADR-0021 work. This ADR **must not** add
parallel APIs. It adds only what is missing, and it lifts the Wasm-private classifier into
`ooxml-xlsx` so that the Wasm adapter and the Python binding use one classifier (ADR-0021,
"Rust changes and transaction choice").

### Thread-safety, checked

A compile check (a throwaway crate outside the repository, asserting `T: Send` and `T: Sync`)
against `main` and against the ADR-0021 working tree gives the same result (review round 1
repeated it on `main`, adding `Mutex<Workbook>: Send + Sync`):

| Type | `Send` | `Sync` | Why |
|---|---|---|---|
| `ooxml_xlsx::Workbook` | yes | **no** | `graph::Graph::everything_closure: std::cell::OnceCell<Dependents>` |
| `ooxml_docx::Document` | yes | yes | — |
| `ooxml_pptx::Presentation` | yes | yes | — |
| `ooxml_xlsx::Error` | yes | yes | — |

No `Rc`, `RefCell` field or raw pointer is held by these types.

## Decision

### 1. Scope

In scope:

- **R (Rust, `ooxml-xlsx`, plus `kind()` in every format crate):** workbook-scoped batch
  reads and writes taking the sheet as an argument (§2–§6), defined names (§7), a stable
  error identifier (§8), and the lift of the Wasm-private classifier (§9).
- **P (Python):** the binding policy and the minimal API (§10–§12).

Out of scope, with pointers (§13): docx replace options and bookmarks, pptx text boxes and
pictures, `copy_sheet`, display-text rendering, sort/filter/find-and-replace/copy-paste.

Unchanged: every existing guard and refusal. Refusals (incomplete census, writes into merge
non-anchor cells, array/spill cells, rich-value cells, table headers, protected sheets, a
partial shared group, …) stay errors; `Err` ⇒ byte-identical; nothing here invents a cached
formula result.

### 2. Addressing: `SheetRef`, `Workbook::worksheet`, coordinates

```rust
pub enum SheetRef<'a> {
    /// Exact match first, then a unique case-insensitive match: the rules of
    /// `Workbook::worksheet_by_name` (Excel compares sheet names case-insensitively).
    Name(&'a str),
    /// 0-based position in `<sheets>` (hidden sheets, chartsheets, macro and
    /// dialog sheets counted), as `Workbook::worksheet_by_index` and `sheet_names()`.
    Index(usize),
}

impl Workbook {
    /// The one public entry from a `SheetRef` to a `Worksheet` (m7).
    pub fn worksheet_ref(&mut self, sheet: SheetRef<'_>) -> Result<Worksheet<'_>>;
}
```

(`worksheet_ref`, because `Workbook::worksheet()` already names the first visible sheet.)
Errors: `NoSuchSheet` (absent, or two names folding alike), `NotAWorksheet`. Every
workbook-scoped method in this ADR resolves its sheet through `worksheet_ref`, once, and then
delegates to the `Worksheet` method of the same name; the binding reaches the ADR-0018
`Worksheet` operations (`insert_rows`, …) the same way. **One matching rule for all native
entry points.** Journal calls resolve `SheetRef` to a `SheetId` at append; epochs absorb
renames within a session (revision 3). The Wasm `XlsxDocument::worksheet(name)` matches exactly. If that is
deliberate SheetJS/ExcelJS parity (U5 verifies it against the shipped adapter), the Wasm
adapter keeps an exact-match pre-check on its side and calls the shared Rust functions with
`SheetRef::Index`.

Rust coordinates stay 0-based `CellIndex`/`CellRange` (normalised top-left to bottom-right).
The Python binding converts (§11.4).

### 3. `used_range`

```rust
pub enum UsedRange { Values, Stored }
impl Worksheet<'_> { pub fn used_range(&self, mode: UsedRange) -> Result<Option<CellRange>>; }
impl Workbook     { pub fn used_range(&mut self, sheet: SheetRef<'_>, mode: UsedRange) -> Result<Option<CellRange>>; }
```

- **`Values`** (the default in the binding): the bounding box of every stored cell that
  **has content**, widened by every merged range whose anchor has content (m2). A cell has
  content when it has an `f` element, or when its value is not empty. This is the same
  "present" rule as the ADR-0021 row enumeration (`formula().is_some() || value != Empty`),
  so the two agree. An empty string is content (an explicit `t="s"`/`t="inlineStr"`/`t="str"`
  value); a style-only `<c r="B2" s="3"/>` is not. A formula with no cached value is content.
  A cell whose value cannot be decoded is an error (`InvalidValue`), not silently skipped.
- **`Stored`**: exactly `Worksheet::stored_extent()` as implemented (m1): **rows** from the
  first to the last `row` element (with or without cells), **columns** from stored cells only
  (styled-empty ones included); `None` when the sheet stores no cell at all, even if it has
  `row` elements. It is the nearest file-level equivalent of Excel's `UsedRange`, which
  includes formatted cells *(u)*.
- **`declared_bound`/`declared_range` are never used as the answer.** `dimension/@ref` is
  writer-maintained metadata: many writers emit `A1` whatever the data is. The doc comments
  of all four functions state the differences in one table.
- `None` when nothing qualifies. The box does not start at A1 unless content does.
- Cost (m6): one pass over the row/cell index. "Has content" is decided by a `pub(crate)`
  check that reads `t`, the presence of `v`/`is`/`f`, and for `t="s"` the item **length**
  through the shared-string item index (`shared_string_items`), without cloning the string
  that `CellRef::value()` would return.
- In a journal session the same functions run over the shadow with the View's normalized
  masking (§5 framing): `Values`-mode content checks test content, not caches, so pending
  strips do not change the answer.

### 4. `first_free_row`

```rust
impl Worksheet<'_> { pub fn first_free_row(&self, column: Option<u32>, mode: UsedRange) -> Result<Option<u32>>; }
impl Workbook     { pub fn first_free_row(&mut self, sheet: SheetRef<'_>, column: Option<u32>, mode: UsedRange) -> Result<Option<u32>>; }
```

- `column: None`: one past the last row of `used_range(mode)` (which, for `Values`, already
  includes merges with content, m2); `0` for an empty sheet.
- `column: Some(c)`: one past the last row that has a cell with content (per `mode`) in
  column `c`, or that is covered by a merge with content spanning column `c`; `0` when there
  is none. Not the first gap: rows below a gap are never overwritten by an append.
- `None` when that row would be past row 1,048,576 (the sheet is full). No error: the caller
  decides.

PAD's "Get first free row/column" is documented as relative to the used range; whether PAD
counts format-only cells is unverified *(u)* (Q6).

### 5. `read_range`

#### 5.1 API

```rust
pub struct ReadOptions {
    pub dates: DateRead,       // Typed (default) | Serial
    pub formulas: FormulaRead, // Omit (default) | Text
    pub max_cells: u64,        // default 1_000_000 (Q9, m4)
}

#[non_exhaustive]
pub struct RangeRead {
    pub range: CellRange,              // the range read (normalised)
    pub width: u32,                    // range columns
    pub cells: Vec<RangeCell>,          // row-major, len = rows × width, one per coordinate
    pub roles_incomplete: Option<&'static str>, // Some(kind) when the census or merge scan is incomplete (§5.6, m10)
}

#[non_exhaustive]
pub struct RangeCell {
    pub value: ReadValue,
    pub formula: FormulaText,    // §5.3
    pub role: CellRole,
    pub stored: bool,            // a `c` element exists at this coordinate
    pub has_formula: bool,       // the cell has an `f` element — always set, also with FormulaRead::Omit (M5)
    pub cell_metadata: bool,     // the cell carries `cm` (M5)
    pub rich_value: bool,        // the cell carries `vm` (e.g. an image in a cell)
    pub rich_text: bool,         // value came from a rich shared or inline string (runs)
    pub date_kind: Option<DateKind>, // the number format's kind (§5.2); None = not a date/time format
    pub style: u32,              // `s`, 0 for an absent cell
}

pub enum ReadValue {
    Blank,
    Number(f64),
    Text(String),
    Bool(bool),
    Error(String),            // "#DIV/0!", …
    DateTime(ExcelDateTime),  // DateRead::Typed, DateKind::{Date, DateTime}, or Time with a serial ≥ 1
    Time(ExcelDateTime),      // DateRead::Typed, DateKind::Time, serial in [0, 1): time-only value
    Duration(f64),            // DateRead::Typed, DateKind::Elapsed: days, may exceed 1
}

pub enum FormulaText {
    None,                       // no formula, or FormulaRead::Omit
    Text(String),               // file form, no leading '='
    Unavailable(&'static str),  // has a formula whose text cannot be given (§5.3, m3)
}

#[non_exhaustive]
pub struct CellRole {           // M6: the two roles are independent and may both be set
    pub merge: Option<MergeRole>,
    pub formula: Option<FormulaRole>,
}
pub enum MergeRole { Anchor(CellRange), Member { anchor: CellIndex } }
pub enum FormulaRole {
    SharedMaster(CellRange),
    SharedMember { master: Option<CellIndex> },        // None: no verified master (ambiguous group, missing master)
    ArrayAnchor { range: CellRange, dynamic: bool, data_table: bool },
    ArrayMember { anchor: CellIndex },
}

impl Worksheet<'_> { pub fn read_range(&mut self, range: CellRange, opts: &ReadOptions) -> Result<RangeRead>; }
impl Workbook     { pub fn read_range(&mut self, sheet: SheetRef<'_>, range: CellRange, opts: &ReadOptions) -> Result<RangeRead>; }
```

- **Journal sessions read through the View in normalized mode** (ADR-0024 §6.1; revision
  3). The `Workbook`/`Worksheet` readers above are the sequential reference (ADR-0024 part
  A): U4 implements the read logic once over a shared read interface, and the session path
  runs it over the shadow with normalized masking — pending strips (R2) read cacheless and
  pending dimension growth applies. Native callers without a session use the direct methods
  and see sequential state; both paths share code, never duplicated logic.

`CellValue` keeps its meaning (`Date(String)` is the raw `t="d"` text). `ReadValue` is a new
owned projection; it does not add a variant to `CellValue`. `header` is **not** a Rust
parameter: splitting off the first row is a binding concern (§12), and Rust has no header
semantics to own.

`&mut self` because date classification goes through the style registry reads
(`Worksheet::is_date` takes `&mut self` today). The read does not change the package: no
dirty part, no registry activation (ADR-0006 m5), T1 holds after any number of reads.

A cell with both roles is reported with both: a merged anchor that is also a shared master
has `merge: Some(Anchor(..))` and `formula: Some(SharedMaster(..))`. No role takes priority
over the other; consumers (the classifier of §9, a caller's "fill merged") read the one they
need.

#### 5.2 Values and date kinds

**`DateKind`** (M8) is a new classifier next to `is_date_format`, in
`registry/format.rs`, applied to the cell's number format:

- `Elapsed`: a custom code whose first section has an `[h]`, `[m]` or `[s]` token (`[h]:mm:ss`,
  `[mm]:ss`), or built-in 46.
- `Date`: date tokens only (`y`, `d`, `m` read as month, era/locale date tokens), no time
  token; built-ins 14–17, and the East Asian date ids among 27–36 and 50–58.
- `Time`: time tokens only (`h`, `s`, `m` read as minutes after `h` or before `s`, `AM/PM`,
  `A/P`); built-ins 18–21, 45, 47, and the East Asian time ids among 27–36 and 50–58.
- `DateTime`: both; built-in 22.
- Tokens inside quotes, after `\`/`_`/`*`, and inside other `[...]` sections (colors,
  locales, conditions) are ignored, as in `is_date_format`. The built-in table for the
  locale-dependent ids 27–36 and 50–58 is written in U4 with its source (ECMA-376 Part 1
  §18.8.30 and the locale lists it references), each id tested.
- `is_date_format(x)` stays as it is and equals `date_kind(x).is_some_and(|k| k != Elapsed)`
  for built-ins, `date_kind(x).is_some()` for custom codes (the existing calamine rule, which
  counts `[h]` as a date); a unit test pins this relation so the two cannot drift.

| Stored cell | `value` with `DateRead::Typed` | with `Serial` |
|---|---|---|
| absent, or present without `v`/`is` (style-only) | `Blank` | `Blank` |
| number, `date_kind` `None` | `Number` | `Number` |
| number, `Date` or `DateTime` | `DateTime(from_serial(v, date1904))`; `Number` when `from_serial` is `None` (negative, beyond 9999-12-31, serial 60 in the 1900 system) | `Number` |
| number, `Time` | serial in `[0, 1)`: `Time` (time of day); serial ≥ 1: `DateTime` (the value carries a day part that the format hides; it is not dropped); negative or out of range: `Number` | `Number` |
| number, `Elapsed` | `Duration(v)` for finite `v ≥ 0`; `Number` otherwise | `Number` |
| `t="s"`, `t="inlineStr"`, `t="str"` | `Text` (rich runs concatenated, `_xHHHH_` decoded, phonetic runs excluded, as `CellRef::value()`); `rich_text` set for runs | same |
| `t="b"` | `Bool` | same |
| `t="e"` | `Error` | same |
| `t="d"` (ISO 8601) | date or date-time → `DateTime`; time-only → `Time`; parsed with the validator behind `set_iso_date` (lifted to a shared `pub(crate)` parser); unparsable → `Err(InvalidValue)` | `Number`: the serial of the parsed value in the workbook's system (time-only: the fraction); unparsable → `Err(InvalidValue)` |

- **1900/1904.** `date1904` is read once per call (`Workbook::date1904`). The 1900 system
  keeps Excel's false 1900-02-29 (serial 60 → `Number`). The binding never calls
  `set_date1904` (existing serials would change meaning).
- `date_kind` is reported for every cell (also with `Serial`), so a caller can apply its own
  rule.
- Precision: `from_serial` rounds to milliseconds.

#### 5.3 Formula cells: cached value and formula text, both explicit

- `value` is **always the cached result** stored in the file, decoded by the table above
  (`CellRef::value()` already returns the cache). A formula with no cache reads as `Blank`
  with `has_formula: true`. Nothing evaluates a formula and nothing synthesises a cache.
  After a formula write in the same session, the written cell and every dependent whose cache
  was stripped (ADR-0003) read as `Blank` until a spreadsheet application recalculates. In a
  journal session the same holds for **pending** strips, which normalized mode masks (R2
  restores them only at save).
- `formula` (with `FormulaRead::Text`) is the cell's **effective** formula text in file form
  (no leading `=`, future-function prefixes as stored), decided **per cell**, never failing
  the whole read (m3):
  - ordinary formula, shared master, array/data-table/dynamic-array anchor: `Text(its text)`;
  - shared member with a verified master (`Worksheet::shared_formula_at` is `Some`): the
    master's text translated to the member's position with
    `Worksheet::translate_shared_formula`; `Unavailable("untranslatable")` if translation
    returns `None`;
  - shared member without a verified master (`shared_formula_at` is `None`: ambiguous group,
    missing master): `Unavailable("no shared master")` — never the member's own empty `f`;
  - array/spill members: `None` (no formula of their own; `role.formula` names the anchor);
  - census blocked (§5.6): ordinary and anchor texts are still read from the cell's `f`;
    shared members are `Unavailable("census incomplete")`.
- `FormulaRead::Omit` skips text lookup and translation (the fast path for value reads);
  `has_formula` is still set from the presence of `f`.

The two fields let the binding offer both of today's Python conventions without a second
read (§12, Q1).

#### 5.4 Merged cells, spills, rich values

- Merge: the anchor has its own value and `MergeRole::Anchor(range)`; the other coordinates
  get `MergeRole::Member { anchor }` and whatever they store (normally `Blank`). Values are
  **not** copied into members; a caller that wants "fill merged" does it from `role.merge`.
- Arrays, data tables, dynamic-array spills: every coordinate of the declared range gets its
  formula role, **including coordinates with no stored cell** (ADR-0021: guard spans as
  ranges). The members' cached values are returned as stored.
- `vm` cells: the cached value as stored (often an error), `rich_value: true`.
- Roles come from `merged_ranges`, the census ranges (`array_formulas`) and
  `shared_formula_at`: the same sources as `non_scalar_ranges` and `formula_result_ranges`.

#### 5.5 Out of range and size

- A range beyond Excel's grid is `Err(OutOfRange)`. A range beyond the stored data is filled
  with blank, unstored, role-less entries: the output is dense, one entry per coordinate.
- An area larger than `max_cells` is `Err(RangeTooLarge { cells, limit })` (new variant),
  before any allocation. Whole-column and whole-row requests in the binding are clipped to
  `used_range(Stored)` before calling Rust (§12), never by Rust.
- Memory (m4): a `RangeCell` is about 100 bytes plus string heap; 1,000,000 coordinates are
  about 100 MB in Rust before Python objects (several times that once converted). Hence the
  default of 1,000,000 (Q9); a caller reading more sets `max_cells` explicitly or reads in
  row bands.

#### 5.6 Incomplete classification

Reads keep working when edits are blocked (ADR-0003 §1). If the census is blocked
(`FormulaScanFailed`, `UnknownCellMetadata`, …), or the merge scan is incomplete (the
`non_scalar_ranges` errors), values are still returned, roles that depend on the missing
information are left `None`, and `roles_incomplete` is `Some(kind)` naming the blocking
error's `kind()` (§8). Plain value reads (the binding's `read_range`) still return values,
because a read of cached values is still correct; the binding exposes the flag. **Every
classifying consumer fails closed on it** (§9): `RangeRead::scalar` and the guards return the
blocking error, as `classify_sheet` does today.

#### 5.7 Performance

- One pass over the stored rows intersecting the range (row index), one pass over each row's
  cells intersecting the columns; absent coordinates cost one default entry.
- Shared strings: resolved through the generation-keyed shared-string item index
  (`shared_string_items`, `pub(crate)`, from the ADR-0021 work) or the loaded table; no
  per-cell table parse, no part re-read (m6).
- `DateKind` is memoised per style index within one call.
- Roles are precomputed per call from the range lists, not looked up per cell in the census.
- Target (measured and published, not a CI gate at first, as ADR-0018 Q10): `read_range` of a
  1,000,000-cell used range with `Omit` and `Typed` costs at most the time of
  `Workbook::open` of the same file, and at most 2× a bare `cells()` + `value()` traversal.
  Benchmarks go into `ooxml-bench`.

### 6. Writes at workbook scope

#### 6.1 Date, time and duration writes

The ADR-0021 work adds `Clear`, `Bool`, `IsoDate`, `Error` and `FormulaWithResult`. This ADR
adds two variants and marks `CellWrite` `#[non_exhaustive]` (m10; `CellWrite` keeps growing):

```rust
/// The serial of `value` in the workbook's date system, with a number format of `kind`:
/// the cell's current format is kept when its `DateKind` equals `kind`; otherwise it
/// becomes the built-in for `kind`: Date → 14, Time → 21 (`h:mm:ss`), DateTime → 22
/// (`m/d/yyyy h:mm`, Q4). `kind = Elapsed` is `InvalidValue` (use `Duration`).
/// Errors: `DateOutOfRange` plus those of `Number`.
DateTime(ExcelDateTime, DateKind),
/// A duration in days (finite, ≥ 0), as a number with an `Elapsed` format: the current
/// format is kept when it is `Elapsed`, otherwise built-in 46 (`[h]:mm:ss`).
Duration(f64),
```

- Style changes go through the existing `StyleRequest` path (a generalised `EnsureDate` that
  takes the target kind); every other part of the style is kept. Same guards, same
  atomicity as the other variants.
- `CellMut::set_datetime` is **unchanged** (an Accepted behaviour: any date/time format is
  kept, otherwise 14). `DateTime(v, Date)` differs from it only when the current format is a
  time or date-time format (then `set_datetime` keeps it, `DateTime(v, Date)` sets 14); the
  doc comments say so.
- `IsoDate` stays for callers that want `t="d"` (ExcelJS/SheetJS `cellDates`). The binding
  writes Python temporal values with `DateTime`/`Duration` (§11.4): what Excel itself
  stores and what Excel 2007 reads.

#### 6.2 Workbook-scoped batches

```rust
impl Workbook {
    pub fn write_cells(&mut self, sheet: SheetRef<'_>, writes: &[(CellIndex, CellWrite<'_>)]) -> Result<()>;
    pub fn write_cells_with_styles(&mut self, sheet: SheetRef<'_>, writes: &[(CellIndex, CellWrite<'_>)], styles: &[Option<u32>]) -> Result<()>;
    pub fn write_changed_cells(&mut self, sheet: SheetRef<'_>, writes: &[(CellIndex, CellWrite<'_>)]) -> Result<usize>;
    pub fn clear_range(&mut self, sheet: SheetRef<'_>, range: CellRange) -> Result<usize>;
}
impl Journal {
    /// Drops every write whose target already reads equal (below), then appends the rest
    /// as one multi-op journal call (§6.3 for shared groups). Returns how many cells were
    /// written.
    pub fn write_changed_cells(&mut self, sheet: SheetRef<'_>, writes: &[(CellIndex, CellWrite<'_>)]) -> Result<usize>;
    /// Clears the content of every stored cell with content in `range` (§6.3).
    pub fn clear_range(&mut self, sheet: SheetRef<'_>, range: CellRange) -> Result<usize>;
}
```

- `write_cells`/`write_cells_with_styles` resolve the sheet and delegate to the sequential
  `Worksheet` batch (ADR-0024 part A); same atomicity (every guard checked before apply),
  same `DuplicateCellWrite`/`SharedFormulaMaster` errors, no extent restriction (ADR-0021's
  "insert within the original range" rule is a SheetJS projection constraint, not a native
  one).
- `write_changed_cells`/`clear_range` exist twice (revision 4): the `Journal` methods
  append one multi-op journal call (ADR-0024 §4.1/§4.5), and the `Workbook` methods are
  their sequential twins (part A) running the same C1 plan without a log. Guards, atomicity,
  poison semantics and `truncate`/`Commit` behaviour come from the journal path; R1 then sees
  only changed cells, and its whole-call skip is a safety net, never the dropping mechanism.
- **"Reads equal"** (`write_changed_cells`, M4) is a **reads-only state-equality** evaluated
  per cell without planning: the §4.4 core (content with type and rich runs, or formula text
  with kind and every `f` attribute; `s`, with missing equal to `0`) extracted into one
  function shared with R1 — R1 calls it on validated plans (unchanged results), the pre-drop
  calls it on read state. Each write is resolved to its op first (§6.1, including
  DateKind-keeping), then compared; dropped when state-same, kept otherwise. This does not
  contradict F13: F13 forbids the generic skip hiding refusals the sequential path would
  raise (a divergence); here the drop is the API's defined semantics and the sequential twin
  drops identically, so journal and sequential agree cell for cell.
- A constant write against a formula cell drops when it equals the cell's **cached value**
  (same serial/value with the write's kind rules; revision 4, M2): dropping keeps formula
  and cache byte-identical, while applying would silently destroy the formula on a no-op
  round-trip. This rule is 0023-specific (R1's report lacks it); the sequential twin applies
  it identically. An explicit change still applies and replaces the formula, as the caller's
  edit. A formula write drops when its text, kind and attributes equal the cell's effective
  formula (the shared report covers it); otherwise it is the caller's edit and is never
  dropped. The comparison reads; it writes nothing and invents nothing.
- The point of `write_changed_cells`: a read → modify → write-back of a region changes only
  the cells the caller changed; unchanged merge members, table headers and array members are
  dropped before any guard sees them, so they do not refuse the batch (M4). A changed value
  in a guarded cell still refuses the whole call, byte-identical.

#### 6.3 Clearing and overwriting shared formula groups (M3)

`write_cells` refuses two writes into one shared group in one batch
(`SharedFormulaMaster`: "another cell of shared group … is written in the same batch",
ADR-0003 §4.2). Excel saves filled-down formulas as shared groups, so clearing or
overwriting a computed column would always refuse. Decision:

- When the cells of a journal call (`clear_range`, `write_changed_cells`, and through them
  the binding's `clear`, `write_range`, `append_rows`) cover **every** member and the master
  of one shared group, the group is cleared or replaced **as a whole in the same C1 plan**:
  the existing `ClearingFormulaGroup::Shared` mechanism of `clear_shared_formula_row` is
  generalised from one-row groups to any group whose `range` is verified (no
  `ambiguous_group`, every member inside the range, master present), and from a single group
  per plan to a **clearing set holding several groups** (m1; a range call routinely spans
  more than one fill-down column). Replaced-group detection stays in the C1 plan machinery
  the journal append path already runs (`replaced_groups`), shared with the sequential
  implementation rather than reimplemented. No hand-off is planned for such a group; every
  other guard (merges, arrays, headers, protection, census) still applies; dependents'
  caches are stripped as for any write.
- A call that covers **part** of a group still refuses (`SharedFormulaMaster`), as today.
  Writing a partial group cell-by-cell through separate calls is unchanged (hand-off).
- `write_cells` itself keeps today's rule (no group-wide semantics behind a plain batch);
  only the two range-shaped operations opt in.

`clear_range` writes `CellWrite::ClearValue` (m1: on a stored cell identical to `Clear`,
and a journal op already) **only to stored cells with content** (§3) in the
range, in one call, and returns how many it cleared; it never creates blank cells. Styles are
kept. Merge members normally have no content and are skipped; a merge member that does store
content refuses with `MergedCell`. Any other guarded cell with content (array member, rich
value, table header, …) refuses the whole call, byte-identical.

### 7. Defined names

```rust
#[non_exhaustive]
pub struct DefinedName {
    pub name: String,            // as stored, including the `_xlnm.` prefix of built-in names
    pub formula: String,         // the element text, file form
    pub scope: NameScope,
    pub comment: Option<String>,
    pub hidden: bool,
    pub builtin: bool,           // `_xlnm.` prefix, ASCII case-insensitive
}
pub enum NameScope {
    Workbook,
    Sheet(u32),                  // `localSheetId`, as the setters' `Option<u32>` (m10)
    Invalid(String),             // a `localSheetId` that is not a valid sheet position, as stored
}

#[non_exhaustive]
pub enum ResolvedName {
    Range { sheet: usize, range: CellRange },
}

impl Workbook {
    pub fn defined_names(&mut self) -> Result<Vec<DefinedName>>;          // document order, all scopes
    pub fn resolve_defined_name(&mut self, name: &str, scope: Option<SheetRef<'_>>) -> Result<ResolvedName>;
}
```

- `defined_names()` replaces the hand extraction in the Wasm adapter and the private tuple
  `sheet_ops::defined_names`. The ADR-0021 `defined_name_nodes` and `global_defined_names`
  stay (Q10). The setters (`set_defined_name(s)`) stay sequential-only for native use: there
  is no journal op for name writes (ADR-0024 Part B, P5, needs its own ADR), and a direct
  shadow edit would bypass the log, so the binding exposes reads only until P5 lands. The
  reads run over the shadow, which always holds current names.
- **Resolution (M7)** is a new, small function written for this API. It shares with
  `formula::precedents` only the case fold and the parsed expression of a name (`NameDef`'s
  expression); it does **not** reuse `Env::resolve`, whose conservative rules (a local name
  anywhere hides the global one when `own` is `None`; one invalid scope makes every
  same-folded name unresolvable; "ambiguous" and "absent" both `None`) serve dependency
  analysis, not lookup. The rules:
  1. Candidates are the names that fold equal to `name`.
  2. If any candidate has `NameScope::Invalid`, the lookup is
     `Err(AmbiguousName { name, reason: "invalid localSheetId" })`: Excel would repair the
     file, and which name it would keep is unknown (fail closed).
  3. With `scope: Some(s)`: the candidates local to `s`; if none, the global candidates.
     With `scope: None`: the global candidates only (a name local to some sheet does not
     hide the global one).
  4. Exactly one candidate in the chosen scope resolves; two (folding alike) are
     `Err(AmbiguousName { name, reason: "names fold alike" })`; none is
     `Err(NoSuchName(name))`.
- `ResolvedName::Range` is returned only when the name's text is **one sheet-qualified
  absolute area** (`Sheet1!$A$1:$B$3`, `'My sheet'!$C$5`) on a worksheet. A `$`-less row or
  column in a name is relative to the active cell, which a file-level API has no notion of.
  Everything else is `Err(NameNotARange { name, reason })` with `reason` one of
  `"constant"`, `"formula"`, `"multiple areas"`, `"relative reference"`,
  `"external reference"`, `"#REF!"`, `"3D reference"`, `"not a worksheet"`. No formula is
  evaluated (`OFFSET`/`INDEX` names are `"formula"`).
- Reading and writing a named cell is `resolve_defined_name` + `read_range` /
  `write_cells` (§12).

### 8. Stable error identifiers

`ooxml_core::Error` and each format crate's `Error` get:

```rust
impl Error {
    /// The variant name, e.g. "MergedCell". Stable: renaming a variant is a breaking change
    /// recorded in the owning ADR. In a format crate, `Core(e)` returns `e.kind()`.
    pub fn kind(&self) -> &'static str;
    /// The A1 cell the error is about, for variants that carry one (`cell` fields).
    pub fn cell(&self) -> Option<&str>;
}
```

- `kind()` is a plain `match` with no wildcard arm, so a new variant without a `kind` fails to
  compile inside its crate. A unit test builds one value of every variant constructible in
  tests and checks that the Debug output starts with `kind()`; for `Core(inner)` it checks the
  inner Debug output against `inner.kind()` (m5).
- `kind` strings are **not** unique across crates (`InvalidValue`, `InvalidXmlChar`,
  `MergedCell`, `NoStylesPart`, `NoSuchTable` exist in more than one, and xlsx
  `InvalidValue` is a decoding failure while docx `InvalidValue` is a caller-input error).
  Consumers key on `(crate, kind)`; a `Core` error is `("core", kind)` whatever crate wrapped
  it (M2). The crate is known from the handle type in a binding.
- **Audit (U1, M2):** every place that returns `Malformed` or `InvalidValue` for **caller
  input** rather than file content is listed in the U1 PR (known: `write_cells_with_styles`
  length mismatch, `set_shared_formula` range/result count mismatch, in the ADR-0021 work).
  Each is moved to a new caller-input variant `InvalidArgument { message }` in its crate, or
  kept and listed as an exception in the binding table with the reason. `Malformed` then
  means "the file", which the binding maps to `MalformedFileError`.
- ADR-0021/0022 adapters keep their own error-code vocabulary (`INVALID_VALUE`,
  `CELL_EDIT_REFUSED`, `WORKSHEET_READ_REFUSED`, `INVALID_INPUT`, …); if they want the Rust
  kind, each adapter keeps its own mapping table from `(crate, kind)` (m5). Nothing here
  changes those codes.
- New xlsx variants in this ADR: `RangeTooLarge { cells, limit }`, `NoSuchName(String)`,
  `AmbiguousName { name, reason: &'static str }`, `NameNotARange { name, reason: &'static str }`,
  `NonScalarCell { cell, reason: &'static str }` (§9), `InvalidArgument { message }` (audit).
  Each gets the `Err ⇒ byte-identical` test where an edit raises it.

### 9. Lifting the classifier (shared by Wasm and Python)

The Wasm-private scalar classifier becomes public in `ooxml-xlsx`, defined over `RangeRead`
so that there is one enumeration and one set of guards, and so that it **fails closed** (M5):

```rust
pub struct ScalarOptions { pub allow_error: bool, pub dates: ScalarDates, pub allow_merge_anchor: bool }

impl RangeRead {
    /// `classify_scalar_value` for the cell at `index`. Err(blocking error) when
    /// `roles_incomplete` is set: no scalar is returned from an incomplete classification.
    pub fn scalar(&self, index: usize, opts: &ScalarOptions) -> Result<Option<CellValue>>;
}
impl Worksheet<'_> {
    pub fn scalar_guards(&self) -> Result<Vec<CellRange>>;   // classify_sheet: fails closed on a blocked census
    pub fn read_scalar(&mut self, at: CellIndex, opts: &ScalarOptions) -> Result<Option<CellValue>>;
}
```

- `RangeRead::scalar` returns `Ok(None)` (not scalar) for a cell whose `has_formula`,
  `cell_metadata`, `rich_value` or `rich_text` is set, that has any `role.formula`, that is a
  merge member, or a merge anchor unless `allow_merge_anchor`; an error value unless
  `allow_error`; a non-finite number; a date-formatted number or a `t="d"` value under
  `ScalarDates::Unsupported`; otherwise `Ok(Some(value))`. The phonetic rule of
  `has_complex_value` is carried by `rich_text` (phonetic runs alone do not set it). These are
  exactly the inputs `classify_scalar_value`/`has_complex_value` read today; the fields exist
  in `RangeCell` whatever `ReadOptions` says, so a read with `FormulaRead::Omit` still
  classifies a formula cell as non-scalar.
- With `roles_incomplete` set, `scalar` returns the blocking error for every index, as
  `classify_sheet` does today via `non_scalar_ranges`. `RangeCell` has no public classifier of
  its own (a lone cell does not know whether its range's roles are complete).
- `allow_merge_anchor` excludes the anchor's merge **by range equality**, not by the position
  of the merge in `non_scalar_ranges` (the Wasm `classify_cell` removes by index, relying on
  `merged_ranges` coming first).
- `scalar_guards`/`read_scalar` keep the Wasm adapter's per-generation cache: the guard ranges
  are cached in the `Workbook` keyed by `(sheet, Package::generation())`, as
  `scalar_read_ranges` does now, so a single-cell probe does not rebuild them (ADR-0022's
  latency gates).
- The "no editable scalar representation" refusal becomes `Error::NonScalarCell { cell,
  reason }` instead of `InvalidValue` with a message, so bindings can branch on `kind()`.
- **Ownership:** U5 (this ADR's implementer) moves the helpers now that PR #170 is on
  `main`, so that the Wasm crate is switched in the same PR and its browser tests prove
  parity. Acceptance includes: with a blocked census, `read_scalar` and `scalar_guards` return
  the same `Err` as the current Wasm path; a merged anchor that is also a shared master is
  non-scalar. Exact names may be adjusted in that PR and recorded in an implementation note
  here.

### 10. Python binding: crate, packaging, CI (M9: decided)

**Location and workspace.** The binding is a **separate Cargo workspace** at
`bindings/python/` (outside the `crates/*` glob), with its own `[workspace]` table and its own
committed `Cargo.lock`. The root `Cargo.toml` also lists it in `exclude = ["bindings/python"]`
so a future glob cannot pick it up. Consequences:

- `cargo test --workspace --all-features`, `cargo clippy --workspace …` and
  `cargo xtask codegen --check` at the root never build PyO3 and never need Python or
  libpython: the AGENTS.md done-definition commands and every existing CI job are unchanged.
- The binding cannot inherit `[workspace.package]`/`[workspace.dependencies]`; it states
  `edition = "2024"`, `rust-version = "1.99"` and its license itself, and depends on the
  format crates by `path = "../../crates/ooxml-…"`. A CI step checks that its
  `rust-version`/`edition` equal the root's.

**Crate.** `bindings/python/Cargo.toml`: package `ooxml-py`, `[lib] name = "_native"`,
`crate-type = ["cdylib"]`, `test = false`, `doctest = false` (no Rust-side tests that would
link libpython; behaviour is tested from Python). Dependencies: `ooxml-core`, `ooxml-xlsx`,
`ooxml-docx`, `ooxml-pptx` (path), `pyo3` (exact version pinned). Feature
`fault-injection = ["ooxml-xlsx/fault-injection"]`, used only by the CI test build (m8).

**Packaging.** `bindings/python/pyproject.toml` (maturin, version pinned): `module-name =
"ooxml_rs._native"`, `python-source = "python"`, `features = ["pyo3/extension-module",
"pyo3/abi3-py310"]` (m8). The pure-Python package `bindings/python/python/ooxml_rs/`
(`__init__.py`, exception hierarchy, `Formula`, `CellError`, `CLEAR`, `py.typed`, `.pyi`
stubs) re-exports `_native`. `extension-module` is enabled only from `pyproject.toml`, never
as a crate feature. Whether `datetime`/`timedelta` conversion is available under the abi3
limited API at the pinned PyO3 version is checked in U7; if not, the binding converts through
Python-level constructors (`datetime.datetime(...)`) and records that in the implementation
note (m8).

**CI layout.** A new job `python` in `.github/workflows/ci.yml`, **no path filter** (runs on
every push and PR, so a Rust change that breaks the binding fails CI):

```yaml
  python:
    name: Python binding (${{ matrix.os }})
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-latest, windows-latest]
    runs-on: ${{ matrix.os }}
    defaults: { run: { working-directory: bindings/python } }
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@master
        with: { toolchain: "1.99", components: clippy }
      - uses: Swatinem/rust-cache@v2
        with: { workspaces: bindings/python }
      - uses: actions/setup-python@v5
        with: { python-version: "3.10" }          # the abi3 floor
      - run: python -m pip install -r requirements-ci.txt   # maturin, pytest: pinned
      - name: workspace metadata matches the root
        run: python -I ci/check_manifest.py ../../Cargo.toml Cargo.toml
      - run: cargo clippy --locked --all-targets --features fault-injection -- -D warnings
      - name: build wheels (release, and a fault-injection test build)
        run: |
          maturin build --locked --release -o dist
          maturin build --locked --release --features fault-injection -o dist-fi
      - name: test the release wheel in a fresh venv
        run: |
          python -m venv .venv-rel
          .venv-rel/bin/python -m pip install dist/*.whl pytest   # Scripts\ on Windows (shell: bash)
          .venv-rel/bin/python -I -m pytest tests -m "not fault_injection"
      - name: test the fault-injection wheel
        run: |
          python -m venv .venv-fi
          .venv-fi/bin/python -m pip install dist-fi/*.whl pytest
          .venv-fi/bin/python -I -m pytest tests -m fault_injection
```

(The venv paths differ on Windows; U7 writes the steps with `shell: bash` and a per-OS
`bin`/`Scripts` variable. clippy in this job needs Python, which `setup-python` provides.)
U7's acceptance includes the logs of this job passing on both runners, and of the unchanged
root `check` job. AGENTS.md's done-definition gains one line: a PR that touches
`bindings/python/`, or a crate it depends on, also needs the `python` job green (it runs on
every PR anyway; locally optional, as the python-pptx oracle is).

**Release.** Wheel publishing (PyPI name, macOS/aarch64 builds) is decided before the first
release (Q2); nothing is published by this ADR's PRs. Free-threaded CPython builds are not
targeted in the first release; the locking rule of §11.1 does not rely on the GIL, so they
can be added later.

The binding adds no parsing or editing logic of its own; any rule it needs is added in Rust
first.

### 11. Python binding: handles, threads, failures

#### 11.1 Handles and locking

- `Workbook`, `Document` and `Presentation` are `#[pyclass(frozen)]` wrappers around a
  `std::sync::Mutex<Slot<T>>` and a `std::sync::Condvar`, where `Slot<T>` holds the Rust
  value and the transaction state of §11.2. For xlsx, `T` is the `Journal` (revision 3):
  `open`/`from_bytes`/`new` open one, every §12 call appends journal ops, reads go through
  the View, and `save`/`to_bytes` commit. `Document` and `Presentation` have no journal and
  keep sequential values. `Mutex<Slot<T>>` is `Send + Sync` because each `T` is `Send`
  (Context), so the classes need **no `unsendable`** and work under a PyO3 version that
  requires `Sync` pyclasses. `unsendable` is rejected because a handle created on one thread
  would panic when touched from another, which RPA hosts with worker threads do.
- **The mutex is only ever locked, and the condvar only ever waited on, with the GIL
  released.** Every method does:
  1. with the GIL: convert Python arguments into owned Rust values (strings, write sources,
     coordinates); reject unsupported types with `TypeError`;
  2. release the GIL (`Python::allow_threads`, named `detach` in newer PyO3), lock, wait
     while another thread owns an open transaction (§11.2), run the Rust call, produce an
     **owned** result (`RangeRead`, `Vec<DefinedName>`, `Vec<u8>`, …), unlock;
  3. with the GIL: convert the owned result to Python objects.

  Never holding the GIL while blocked rules out the GIL/mutex deadlock. The closure captures
  `&Mutex<…>`/`&Condvar`, which are `Send`. This is why every Rust API in §3–§7 returns owned
  data.
- `open`, `from_bytes`, `save`, `to_bytes`, `read_range`, batch writes and structural edits
  all release the GIL. Calls on one handle are serialised; calls on different handles run in
  parallel.
- No Python object borrows into a handle: there is no `Worksheet`, `Cell` or docx `Match`
  object. Sheets are arguments (§11.4).
- `save(path)` calls `Workbook::save` / `Document::save` / `Presentation::save` directly
  (poison check, then the package's streaming write to a temporary file and rename, with the
  temporary file removed on failure), not `to_bytes` plus a second file write (m9).

#### 11.2 Failures, transactions, recovery

- **Refusals** (validation errors) leave the Rust object unchanged (`Err` ⇒ byte-identical
  across every crate). The binding raises and the handle stays usable. No staging is needed.
- **Poisoning** (`Error::Poisoned`: an apply step failed after validation, i.e. a bug;
  ADR-0003 §3.1) is **not** rollback (ADR-0021). Outside a transaction the handle stays
  poisoned: every later edit and save raises `PoisonedError` (Rust already refuses them), and
  the caller reopens from its last saved file.
- **Transactions (M1; revision 3: journal-based, ADR-0024 §10.3).**
  `with wb.transaction(): …` gives rollback to the state at entry.
  - Entry (GIL released): lock; wait on the condvar until no transaction is open; record
    `owner = std::thread::current().id()` and `entry = journal.current_seq()` — always a
    call boundary, since every call appends whole and entry happens between calls — in the
    slot; unlock. No fork, no snapshot.
  - While it is open, **only the owner thread** may use the handle. Every call from another
    thread waits on the condvar (GIL released) until the transaction ends. So no other
    thread's edit can be inside the transaction's span: a rollback discards only the owner's
    edits, and a commit contains only the owner's edits.
  - Exit without exception: clear the owner, `notify_all`. Exit with an exception:
    `journal.truncate(entry)` (replay- or fast-based per ADR-0024; structural ops truncate
    with `truncate_fast` only where ADR-0024 §10.3 allows), clear the owner, `notify_all`,
    re-raise.
  - `transaction()` on the owner thread while one is open is `RuntimeError` (no nesting). On
    another thread it waits, like any call.
  - The owner is an OS thread, so a transaction must be entered and exited on one thread
    (the context-manager protocol does this). A transaction left open (a generator never
    closed) blocks other threads; the docstring says so, and a handle's `__del__` does not
    try to recover it.
  - **Save inside a transaction is refused** (Q5 (A)): `save`/`to_bytes` return
    `TransactionOpen` while one is open. The caller exits first; there is no "commit on
    save".
  - Cost: no per-transaction copy; rollback replays or fast-truncates the span's ops.
    `Document` and `Presentation` have no poisoning and no transaction in this ADR.
- **Panics**: PyO3 turns a Rust panic into `PanicException`; the `std` mutex is then
  poisoned. Inside a transaction, on the owner thread, the binding recovers the guard
  (`PoisonError::into_inner`), truncates to the entry seq, clears the poison, releases
  ownership and re-raises. Outside a transaction the handle is dead: every later call raises
  `PoisonedError` with kind `("binding", "HandlePoisoned")`.
- The binding never retries, never falls back to a lossy path and never writes a partial
  file.

#### 11.3 Exceptions (M2)

- Hierarchy (pure Python, in `ooxml_rs`): `OoxmlError(Exception)` with attributes `crate`
  (`"core"`, `"xlsx"`, `"docx"`, `"pptx"` or `"binding"`), `kind` (the Rust `kind()` string,
  or a binding kind), `cell` (from `Error::cell()`, A1 or `None`), `sheet` (when the call had
  one) and the Rust `Display` text as the message.
- Category subclasses, chosen by a table in the binding keyed by **`(crate, kind)`**:
  - `RefusedError` — the edit was not made: e.g. xlsx `MergedCell`, `ArrayResultCell`,
    `ArrayAnchor`, `RichValueCell`, `TableHeaderCell`, `Protected`, `FormulaScanFailed`,
    `UnknownCellMetadata`, `IterativeCalculation`, `SharedFormulaMaster`, `NonScalarCell`,
    `StructureBlocked`; docx/pptx refusals likewise per crate.
  - `PoisonedError` — xlsx `Poisoned`, binding `HandlePoisoned`.
  - `NotFoundError(OoxmlError, KeyError)` — e.g. xlsx `NoSuchSheet`, `NoSuchName`,
    `NoSuchStyle`; docx/pptx `NoSuch…`.
  - `InvalidInputError(OoxmlError, ValueError)` — e.g. xlsx `InvalidReference`,
    `OutOfRange`, `InvalidNumber`, `StringTooLong`, `DateOutOfRange`, `DuplicateCellWrite`,
    `RangeTooLarge`, `NameNotARange`, `AmbiguousName`, `InvalidArgument`; docx
    `InvalidValue`.
  - `UnsupportedFileError` — core `EncryptedPackage`, `StrictNotSupported`, `DtdNotAllowed`.
  - `MalformedFileError` — e.g. xlsx `Malformed`, `MissingReference`, xlsx `InvalidValue`;
    core XML and package errors.
  - `OoxmlIOError(OoxmlError, OSError)` — core `Io`, carrying `errno`/`strerror`/`filename`
    from the `std::io::Error`; `OoxmlFileNotFoundError(OoxmlIOError, FileNotFoundError)` for
    `NotFound`, `OoxmlPermissionError(OoxmlIOError, PermissionError)` for
    `PermissionDenied`.
- A test enumerates every `(crate, kind)` (the list the `kind()` tests use, exported by a
  hidden `_native` function) and fails if one is missing from the table, so a new Rust
  variant cannot fall silently into the base class. Variant names are preserved verbatim in
  `kind`; there is **no** class per variant (about 300 variants across four crates would make
  the class list an API of its own).
- Python-side conversion failures raise the built-ins (`TypeError`, `ValueError`) before any
  Rust call.

#### 11.4 Value and coordinate mapping (xlsx)

- **Index conventions (m13), stated once in the module docstring:** rows and columns are
  **1-based** (openpyxl and the RPA tools count from 1) and A1 strings are accepted wherever
  a coordinate or range is; **sheets** are `str` (name, `SheetRef::Name` rules) or `int`
  (**0-based** position in `sheet_names()`, so `wb.sheet_names()[i]` and `sheet=i` agree).
  The mixed convention is deliberate: positions in Python lists are 0-based, spreadsheet
  coordinates 1-based.
- Read (`ReadValue` → Python): `Blank` → `None`; `Number` → `int` when integral and
  `|x| ≤ 2^53`, else `float`; `Text` → `str` (`""` stays `""`); `Bool` → `bool`; `Error` →
  `ooxml_rs.CellError` (a value object with `.code`, not an exception); `DateTime` →
  `datetime.datetime` (naive); `Time` → `datetime.time`; `Duration` → `datetime.timedelta`.
  A column with a `Time` format thus yields `time` for values below one day and `datetime`
  above (the value really has a day part), never `timedelta` (M8).
- Write (Python → `CellWrite`), checked in this order: `None` → **no write at that
  coordinate** (M4); `ooxml_rs.CLEAR` → `ClearValue` (m3: never creates a blank cell);
  `bool` → `Bool` (before `int`);
  `int`/`float` → `Number` (non-finite → `InvalidNumber` from Rust); `str` → `String`,
  **always text**, also when it starts with `=` (Q8); `ooxml_rs.Formula(text)` → `Formula`;
  `ooxml_rs.CellError(code)` → `Error`; `datetime.datetime` → `DateTime(v, DateTime)`;
  `datetime.date` → `DateTime(v, Date)`; `datetime.time` → `DateTime(v, Time)`;
  `datetime.timedelta` (≥ 0) → `Duration(days)`. tz-aware values → `ValueError`;
  microseconds are rounded to milliseconds. Anything else → `TypeError`.
- A value read and written back unchanged is dropped by `write_changed_cells` (§6.2), so
  `read_range` → edit → `write_range` changes only the edited cells, including on regions with
  merges, headers and formulas. Concretely: a constant written back against a formula cell
  with an equal cache is dropped (the formula survives); an identical `Formula` write is
  dropped by the shared report; anything else applies (an explicit change replaces the
  formula). A `formulas="formula"` read returns `ooxml_rs.Formula` objects (below), so it
  round-trips; a `str` starting with `=` stays text and never resurrects a formula (Q8).

### 12. Python binding: minimal API (first release)

Signatures are indicative; the binding PRs record the final ones in an implementation note
here. A larger surface (formats, colors, widths, tables, images) is a follow-up binding ADR.

**`ooxml_rs.Workbook`**

| Method | Rust |
|---|---|
| `Workbook.open(path)`, `Workbook.from_bytes(b)`, `Workbook.new()` | journal `open`, `from_bytes`, `new` (§11.1) |
| `save(path)`, `to_bytes()` | journal `save`, `to_bytes` (§11.1); refused inside a transaction (Q5 (A)) |
| `sheet_names() -> list[str]`, `date1904` (read-only property) | `sheet_names`, `date1904` |
| `add_sheet(name, position=None)`, `rename_sheet(sheet, new)`, `delete_sheet(sheet)`, `move_sheet(sheet, to)` | structural journal ops (needs J2) |
| `used_range(sheet, mode="values") -> str \| None` | §3 |
| `first_free_row(sheet, column=None, mode="values") -> int \| None` | §4 |
| `read_range(sheet, ref, *, header=False, formulas="value", dates=True) -> list[list]` (or `(header, rows)`) | §5 |
| `read_cell(sheet, ref)`, `read_formula(sheet, ref) -> Formula \| None` | §5 on one cell (a `Formula` round-trips; a `str` would stay text per Q8) |
| `write_cells(sheet, [(ref, value), …])` | one journal call (every non-`None` value written) |
| `write_range(sheet, top_left, rows)` | `write_changed_cells` (§6.2, §6.3) |
| `append_rows(sheet, rows, start_column="A", key_column=None)` | `first_free_row` + `write_changed_cells`, under one lock |
| `clear(sheet, ref) -> int` | `clear_range` (§6.3) |
| `insert_rows` / `delete_rows` / `insert_cols` / `delete_cols(sheet, at, count)` | structural journal ops (needs J2) via `worksheet_ref` (report as a dict) |
| `defined_names() -> list[DefinedName]`, `resolve_name(name, scope=None) -> (sheet_name, ref)` | §7 (reads over the shadow). Name writes await ADR-0024 Part B (P5); `set_defined_name` stays sequential-only for native use. |
| `edit_refusal() -> tuple[str, str] \| None` | `edit_refusal()` → `(crate, kind)` |
| `transaction()` | §11.2 |

- `read_range`: `formulas="value"` returns cached values; `"formula"` returns
  `ooxml_rs.Formula` objects for formula cells with text (round-trippable: writing one back
  is a `Formula` write, dropped when identical), the value elsewhere, and raises
  `RefusedError` kind `("binding", "FormulaUnavailable")` for an `Unavailable` cell (naming
  the cell); `"both"` returns `(value, Formula-or-None)` pairs and never raises for
  `Unavailable` (formula `None`). Default: Q1. `Formula` has `.text` (no `=`) and
  `str()` renders `"=" + text` for display parity with openpyxl.
- `header=True` returns `(header, rows)`: the first row converted with `str()`, `None` → `""`,
  duplicates kept (a list, not a dict). Turning rows into a DataTable is the host's job.
- `ref` may be a whole column or row (`"A:C"`, `"3:5"`); the binding clips it to
  `used_range(sheet, "stored")` before calling Rust (§5.5). An empty sheet returns `[]`.
- `resolve_name(name, scope=None)` looks at **global names only**; pass `scope=sheet` for a
  sheet's local name (which wins over a global one there). Whether the host should search
  local names when no scope is given is the host's decision (Q11).
- Merge fill, empty-row trimming and display text are not options (display text: §13).

**`ooxml_rs.Document`**: `open`, `from_bytes`, `save`, `to_bytes`, `paragraph_texts()`,
`append_paragraph(text)`, and **`replace(needle, replacement, *, on_refused="refuse") -> int`**
(and `replace_many([(needle, replacement), …])`, applied in order under one lock) over
`Document::replace` with default `ReplaceOptions`. With `on_refused="skip"` the skipped
matches are returned as plain dicts. **No `Match` object is exposed**: matches carry
paragraph generations that a later edit invalidates (`StaleMatch`), which a Python caller
cannot manage safely. (`paragraph_texts`, `replace_many` and `slide_texts` are compositions
in the binding over existing Rust APIs.)

**`ooxml_rs.Presentation`**: `open`, `from_bytes`, `save`, `to_bytes`, `slide_texts()`,
`replace_all(pattern, replacement) -> int`, `add_slide(layout_index, at)`, `delete_slide`,
`move_slide`, `duplicate_slide`.

### 13. Out of scope, with pointers

| Item | Where |
|---|---|
| docx replace options (case-insensitive, whole word) and bookmark read/write | ADR-0007/ADR-0011 addendum, later; the binding's `replace` gains keyword options then |
| pptx text box and picture insertion | ADR-0012 §4.5 (deferred there) |
| `copy_sheet` | ADR-0018 G2 (§4, delivery P7d); exposed by the binding once implemented |
| Display text (number-format rendering) | a new ADR; the ADR-0021 SSF work is the starting point |
| Sort, filter application, find-and-replace, copy/paste, autofill, shifting cell deletes | new ADRs; the last one revisits ADR-0018's partial-shift exclusion |
| Formula evaluation, recalculation, macros, PDF, UI state (selection, active cell) | not file-level operations; the host keeps refusing them |

## Prerequisite and sequencing

The ADR-0021 Rust work has landed on `main` (PR #170); the prerequisite of revision 2 is
met. Revision 3 adds the Edit Journal prerequisite (owner decision Q4, ADR-0024 §13.4): the
range batches (§6.2/§6.3), the View reads (§5) and the binding transactions (§11.2) are
journal-based, so the units that implement them wait for the J series. Order:

1. **Now (no journal needed):** U1 (error identifiers), then U6 (defined-name reads and
   resolver; the setters stay sequential-only) and U7 (binding workspace, CI job, exceptions,
   docx/pptx I/O over the sequential APIs). U9 (Python docx/pptx API) after U7.
2. **After J1:** U2 (workbook-scoped writes; the §6.2/§6.3 batches append journal-op calls),
   U3 (extent, sequential plus View-normalized), U4 (range reads, sequential plus
   View-normalized) → U4b (range writes) and U5 (classifier lift plus the Wasm switch;
   needs U4, not the journal).
3. **After J2:** the binding's structural rows (§12) become available as journal ops; U8
   covers them.
4. **After J3 (`truncate`):** U8 (Python xlsx API with `transaction()`).
5. **J1 exit requirement (cross-ADR, for the J owner to confirm):** J1 publishes the
   `Journal` type with `open`/`new`/`from_bytes`, keyed-op append, the View (echo and
   normalized), `Commit`/save, and a `current_seq` accessor for transaction entry. Without
   these, step 2 cannot start.

## Test oracles

1. **No-op round trip.** For every xlsx fixture: open, `used_range` (both modes),
   `first_free_row`, `read_range` of the stored extent (all option combinations),
   `defined_names`, `resolve_defined_name` of every name; `to_bytes` is byte-identical to the
   input (T1/T1b). Same through the Python binding (`open` → reads → `to_bytes`).
2. **Refusals are byte-identical.** Every new refusal and every existing refusal reached
   through the new workbook-scoped writes: a merge member with a changed value, an array
   member, an array anchor, a spill anchor and member, a `vm` cell, a table header, a
   protected sheet, an incomplete census, `DuplicateCellWrite`, **a range covering part of a
   shared group**, `clear_range` over a guarded cell, `DateTime(v, Elapsed)`. Each: `Err` of
   the expected `(crate, kind)` and `to_bytes` equal to the input, natively and through Python.
3. **Edit locality.** A batch through `Workbook::write_cells` gives the same bytes as through
   `Worksheet::write_cells` (T2 inherited). **Read → write-back:** `read_range` of a region
   containing merges, a table header, formulas, a fill-down shared column and blanks, then
   `write_range` of the same values with one cell changed: `Ok`, and the XML diff touches only
   that cell (plus the dependent-cache strips ADR-0003 requires). With `None` in place of
   values: no change at all (M4). A value-mode round-trip over formula cells keeps every
   formula whose cache still equals (M2); a formula-mode round-trip keeps every formula
   (identical `Formula` writes drop); changing one cache value replaces exactly that
   formula. The pre-drop predicate is the shared §4.4 state-equality — but the pre-drop is
   load-bearing, not an optimization: R1 only skips whole calls, so without it one changed
   cell would drag unchanged guarded cells through the guards and refuse the call.
4. **Dates.** 1900 and 1904 fixtures: serial 0, 1, 59, 60, 61, 2,958,465, a negative serial,
   a fractional serial with milliseconds; formats 14, 18–22, 45–47, `[h]:mm`, `[mm]:ss`,
   `h:mm AM/PM`, `yyyy-mm-dd hh:mm`, a locale-tagged date and an East Asian built-in id;
   `t="d"` dates, date-times and a time-only value; an unparsable `t="d"`. Expected
   `ReadValue`/`DateKind` per §5.2; a 25-hour `[h]:mm` value reads as `Duration`, not a date.
   Writes: Python `date`, `datetime`, `time` and `timedelta` written then read back equal
   (millisecond precision) in both systems, with the format kinds of §6.1 (a `time` is not
   shown as a date: format 21); serial 60 never becomes a date.
5. **Formula caches.** Cells with cached numbers, strings (`t="str"`), booleans, errors,
   dates and no cache; shared masters and members, an ambiguous shared group and a member
   without a master (`Unavailable`), array, data-table and dynamic-array anchors and members.
   Translated member texts are compared with expected values generated once by a spike with a
   pinned LibreOffice version and committed as a fixture (m11). After a formula write and
   after a value write with dependents, the written cell and stripped dependents read as
   `Blank` with `has_formula` as appropriate; no `v` is added anywhere (XML diff).
6. **Merged cells and overlapping roles.** Anchor value and role; members with and without
   stored cells; a merged anchor that is also a shared master, and a merge inside a spill
   range (both roles reported, M6); `first_free_row` with and without a column, with a merge
   whose anchor has content (m2); `clear_range` over a merge.
7. **Shared groups (M3).** `clear_range` and `write_range` over a full multi-row fill-down
   group succeed in one call, leave no orphan `f t="shared"` and strip dependents; over a
   partial group they refuse, byte-identical. A call spanning two full groups clears both
   (the clearing set, m1).
8. **Used range.** Style-only cells, rows without cells, a sheet with only empty `row`
   elements (`Stored` is `None`, m1), empty strings, a formula without a cache, a `dimension`
   of `A1` on a large sheet, a `dimension` larger than the data, data outside `dimension`.
9. **Defined names (M7).** Global and local names with the same text: `scope=None` resolves
   the global one, `scope=s` the local one; two names folding alike (`AmbiguousName`); an
   out-of-range `localSheetId` among same-folded names (`AmbiguousName`, "invalid
   localSheetId"); built-ins, hidden, comments, each `NameNotARange` reason.
10. **Classifier (M5).** With a blocked census, `RangeRead::scalar`, `read_scalar` and
    `scalar_guards` return the same error as the current Wasm path; a formula cell read with
    `FormulaRead::Omit` is non-scalar; a `cm` cell is non-scalar; the Wasm browser tests pass
    unchanged after U5.
11. **Errors (M2).** `kind()` covers every variant (exhaustive match + Debug test, including
    `Core`); the binding's category table covers every `(crate, kind)`; xlsx `InvalidValue`
    maps to `MalformedFileError` and docx `InvalidValue` to `InvalidInputError`; opening a
    missing path raises `FileNotFoundError`.
12. **Python threads and recovery (M1).** Two threads reading one handle while a third writes
    (serialised, no deadlock); `save` on one handle while another handle reads (parallel);
    thread B's `write_cells` issued while thread A's transaction is open completes only after
    A's transaction ends, and survives A's rollback; a second `transaction()` on A raises
    `RuntimeError`; a transaction rolled back after a refusal and after an injected apply
    failure (fault-injection wheel) leaves bytes equal to the state at entry (truncate to the
    entry seq); `save`/`to_bytes` inside a transaction is refused (Q5 (A)); a poisoned
    handle outside a transaction raises `PoisonedError` on edit and save.
13. **Mutation.** A deliberate mutation per unit (e.g. `used_range(Values)` counting
    style-only cells, a shared member returning its own empty `f`, `from_serial` called with
    `date1904 = false` on a 1904 workbook, `[h]` classified as `Time`, `None` mapped to
    `ClearValue`, a transaction that truncates to the wrong seq, `save` allowed inside a
    transaction, the binding locking with the GIL held) is caught by the tests.
14. **Performance.** §5.7's measurements in `ooxml-bench` (1,000,000-cell sheet; shared
    strings; a sheet with 10,000 distinct styles), published in the PR, not gated.

There is no Office in CI. PAD/UiPath behaviour this ADR relies on (used range with
format-only cells, first free row) is flagged *(u)* and checked by the owner manually.

## Delivery

One PR per unit.


- **U1 — error identifiers.** `kind()` and `cell()` on `ooxml_core::Error` and the
  xlsx/docx/pptx `Error`s, with the exhaustiveness tests; the caller-input audit and
  `InvalidArgument` (§8). No other behaviour change.
- **U2 — workbook-scoped writes.** `SheetRef`, `Workbook::worksheet_ref`,
  `Workbook::{write_cells, write_cells_with_styles}`, `CellWrite::{DateTime, Duration}` with
  `#[non_exhaustive]`, the `DateKind` classifier (shared with U4), oracles 2, 3 (first half)
  and the write half of 4. Adds the missing journal payloads (`IsoDate`, `FormulaWithResult`,
  `DateTime` with kind, `Duration`) to `CellCall` with sequential references drawn into
  oracle 1 (m1). After J1 (journal-op batches).
- **U3 — extent.** `UsedRange`, `used_range`, `first_free_row`, the four-way doc table
  (§3, §4), sequential plus View-normalized paths, oracles 6 (part), 8. After J1 (View).
- **U4 — range reads.** `ReadOptions`, `RangeRead`, `RangeCell`, `ReadValue`, `FormulaText`,
  `CellRole`, `read_range`, `RangeTooLarge`, the shared ISO-date parser, the `ooxml-bench`
  cases (§5), oracles 1, 4, 5, 6, 14. After J1 (View).
- **U4b — range writes.** `Journal::write_changed_cells`, `Journal::clear_range`, the C1
  per-target drop predicate shared with R1, the multi-group clearing set (§6.2, §6.3),
  oracles 3 (second half), 7. After U4 and J1.
- **U5 — classifier lift.** `ScalarOptions`, `RangeRead::scalar`,
  `Worksheet::scalar_guards`/`read_scalar` with the per-generation cache, `NonScalarCell`;
  the Wasm crate switched in the same PR (§9), oracle 10. After U4 (transitively after J1,
  but needs no journal API itself).
- **U6 — defined names.** `DefinedName`, `NameScope`, `defined_names`, `ResolvedName`,
  `resolve_defined_name`, `NoSuchName`, `AmbiguousName`, `NameNotARange` (§7), oracle 9.
- **U7 — binding workspace.** `bindings/python/` (Cargo workspace, `pyproject.toml`, Python
  package), root `exclude`, the CI job of §10, the exception hierarchy and `(crate, kind)`
  table, the handle/mutex/condvar/GIL rule, `open`/`from_bytes`/`new`/`save`/`to_bytes` and
  `sheet_names` for docx/pptx (sequential; xlsx sessions move to U8), the abi3 `datetime`
  check (§10, §11.1, §11.3), oracle 11. After U1.
- **U8 — Python xlsx API.** Journal sessions (`open`/`from_bytes`/`new`/`save`/`to_bytes`),
  the `Workbook` table of §12 over U2–U4b and U6, value mapping (§11.4), `transaction()`
  (§11.2), oracles 1–3, 12 through Python. After J3 (for `truncate`), U2–U4b, U6 and U7.
- **U9 — Python docx/pptx API.** `paragraph_texts`, `append_paragraph`, `replace`,
  `replace_many`, `slide_texts`, `replace_all` and the slide operations (§12).

Order: U1 → (U6, U7, U9) any time after; (U2, U3, U4) after J1 → (U4b, U5); U8 after J3
(and J2 for its structural rows), U2–U4b, U6, U7.

## Alternatives considered

- **Bind `Worksheet` as a Python object** (holding `Py<Workbook>` and a sheet index).
  Rejected: every call re-borrows the workbook anyway, `Worksheet<'wb>` cannot be held across
  calls, and a second handle type doubles the locking cases. Workbook-scoped methods with a
  sheet argument are what the Wasm `XlsxDocument` already does.
- **Compose reads in Python from per-cell calls.** Rejected: one GIL round trip and one lock
  per cell; RPA workloads read whole sheets.
- **Return `CellValue` from `read_range`.** Rejected: no date typing (its `Date` is the raw
  `t="d"` text), no formula information and no roles; widening it would change an existing
  public type that the ADR-0021 adapters already project.
- **A single-valued `CellRole` with a priority order.** Rejected (M6): merges and formula
  groups are independent mechanisms; any priority drops information a consumer needs.
- **Python `None` → `Clear`** (openpyxl's assignment semantics). Rejected (M4): a read →
  write-back would refuse on every merge member and table header, and would create a blank
  `c` for every `None` coordinate. Clearing is explicit (`CLEAR`, `clear()`).
- **Reject batches touching a shared group** (keep today's rule for range operations).
  Rejected (M3): filled-down columns are the common case; the whole-group plan already exists
  for one-row groups.
- **Reuse `Env::resolve` for name lookup.** Rejected (M7): its rules are conservative for
  dependency analysis, not lookup.
- **`#[pyclass(unsendable)]`.** Rejected: panics when a handle crosses threads; `Workbook` is
  `Send`, so it is not needed.
- **Make `Workbook: Sync`** (replace the `OnceCell` with `OnceLock`) **and rely on PyO3's
  borrow flags.** Not needed: concurrent use would raise "already borrowed" instead of
  waiting, and transactions need an owner anyway.
- **A transaction that holds the mutex for its whole duration.** Rejected: the guard would
  have to live across Python code that re-enters the handle on the same thread (deadlock) and
  is not `Send`; the owner + condvar scheme gives the same isolation.
- **Per-call fork or byte-snapshot staging** (ADR-0021's ExcelJS path). Rejected as the
  default: refusals are already byte-identical without it; poisoning is a bug path; forking
  copies every parsed part and a byte snapshot re-serialises and reloads on each call, which
  RPA loops of single-cell writes cannot afford. Transactions use `truncate(seq)` instead
  (revision 3).
- **Workbook-hosted range batches and `fork()` transactions** (this ADR's revision 2
  design). Rejected by owner decision Q4 (ADR-0024 §13.4): it duplicates the journal's batch
  validation, no-op skipping and rollback. Revision 3 builds the batches on journal ops, the
  skipping on the C1 report shared with R1, and transactions on `truncate(seq)`.
- **One Python exception class per Rust variant.** Rejected: about 300 classes to keep stable;
  `(crate, kind)` keeps every name, categories give useful `except` clauses.
- **Evaluate formulas or synthesise caches on read.** Rejected: ADR-0003 and ADR-0021 forbid
  inventing results; formula evaluation is a roadmap non-goal.
- **Use `dimension/@ref` as the used range.** Rejected: writer-dependent and often `A1`.
- **The binding as a member of the root workspace** (`crates/ooxml-py`). Rejected (M9):
  `cargo test --workspace --all-features` would build PyO3 test binaries that link libpython,
  which the root CI job (no `setup-python`) and contributors' and agents' sandboxes may lack;
  the AGENTS.md done-definition would silently gain a Python requirement. A separate
  workspace with a dedicated, unfiltered CI job keeps the root commands unchanged and still
  tests the binding on every change.
- **A separate repository for the binding.** Rejected: version skew with the Rust crates, and
  a Rust change could break it unnoticed.

## Consequences

- Rust gains a small owned read/write layer used by both the Wasm adapters and the Python
  binding; the Wasm-private classifier stops being private, fails closed in its public form,
  and its refusal gets a kind.
- `DateKind` makes date typing explicit; elapsed formats stop reading as dates in this API
  (the older `is_date`/`is_date_format` keep their rule for existing callers).
- Every format crate's error variant names become a stable public contract, consumed as
  `(crate, kind)`; caller-input errors get `InvalidArgument` where they used `Malformed`.
- The xlsx range units wait for the J series (revision 3): U2–U5 need J1's public `Journal`
  and View, U8 needs J2/J3; until then only U1, U6, U7 and U9 can land.
- Range-shaped writes can clear or replace whole shared groups; partial groups still refuse.
- The root workspace and its CI job are unchanged; a second Cargo workspace and a two-OS
  Python CI job are added, with a second `Cargo.lock` to keep current.
- Python callers get lossless editing with explicit, documented read semantics that differ
  from openpyxl's in known places: cached values instead of formula text (unless asked),
  dates typed by number-format kind, `None` meaning "unchanged", refusals raised instead of
  silent writes. Migrated flows have to handle `RefusedError`; the refusal rate on real files
  is the main operational risk, measured by oracle 1–2 runs over the corpus.
- A refused or poisoned edit never produces a file; rollback is available per block at the
  cost of one fork, and blocks other threads on that handle while open.

## Open questions for the project owner

Each has a proposed answer, used until the owner decides. (Revision 1's Q3, workspace layout,
is decided in §10; its Q7, who implements, belongs in the implementation PRs, not here.)

- **Q1. Default formula read mode in Python.** Proposed: `formulas="value"` (cached values,
  as PAD/UiPath "read range" return values). openpyxl hosts today get formula text
  (`data_only=False`); they pass `formulas="formula"`.
- **Q2. Python versions, platforms, distribution.** Proposed: abi3, CPython ≥ 3.10; CI wheels
  for Linux x86_64 and Windows x86_64; PyPI name and publishing (and macOS/aarch64) decided
  before the first release, not in these PRs.
- **Q4. Format for date-times with a time part.** Proposed: built-in 22 (`m/d/yyyy h:mm`) for
  `DateKind::DateTime`, 21 (`h:mm:ss`) for `Time`, 14 for `Date`, 46 for durations (§6.1).
  Alternative: a custom `yyyy-mm-dd hh:mm:ss` (openpyxl's default), which is locale-neutral
  but adds a `numFmt`.
- **Q5.** Closed by M8: time-only values are decided by the format kind (§5.2), not by the
  serial range alone.
- **Q6. Default used-range mode.** Proposed: `"values"` for `used_range`, `first_free_row` and
  `append_rows`; `"stored"` is available. To be checked against PAD on a file with
  format-only rows *(u)*.
- **Q8. Strings starting with `=`.** Proposed: always text; formulas only through
  `ooxml_rs.Formula`. The host translates its own convention.
- **Q9. `max_cells` default.** Proposed: 1,000,000 coordinates per `read_range` (≈100 MB in
  Rust, more once converted to Python objects; §5.5).
- **Q10.** Keep `Workbook::defined_name_nodes` public next to `defined_names`, or deprecate
  it? Proposed: keep until the Wasm adapter no longer uses it (to confirm with the adapter
  owner in U5).
- **Q11. Named cells without a scope.** PAD's "named cell" may refer to a sheet-local name.
  Proposed: the binding keeps `scope=None` = global only, and the host, which knows the
  flow's active sheet, passes `scope`. Alternative: with `scope=None`, fall back to a local
  name when exactly one sheet has one.
- **Q12. `write_cells` and unchanged values.** Proposed: `write_cells` always writes (an
  explicit single-cell write of a formula cell's cached value replaces the formula, as the
  caller asked); only `write_range`/`append_rows` skip unchanged values.

## Review response (round 1)

| Finding | Response |
|---|---|
| M1 | Fixed: transaction owner thread + condvar; other threads wait with the GIL released; no nesting; oracle 12 (§11.1, §11.2). |
| M2 | Fixed: `(crate, kind)` keys, core `Io` → `OSError` subclasses, U1 audit and `InvalidArgument`, oracle 11 (§8, §11.3). |
| M3 | Fixed with option (a): whole verified shared groups cleared/replaced in one plan by `clear_range` and `write_changed_cells`; partial groups still refuse; oracle 7 (§6.3). |
| M4 | Fixed: `None` = unchanged, `CLEAR` sentinel, `write_changed_cells` drops writes that read equal (never across a formula, never inventing caches); oracle 3 (§6.2, §11.4). |
| M5 | Fixed: `stored`, `has_formula` (always), `cell_metadata`, `rich_value` in `RangeCell`; classification on `RangeRead` returns the blocking error; merge-anchor exclusion by range equality; per-generation guard cache; oracle 10 (§5.1, §9). |
| M6 | Fixed: `CellRole { merge, formula }`, both reported; oracle 6 (§5.1). |
| M7 | Fixed: new resolver, shares only fold and parsed expression; scope rules restated; `AmbiguousName`; invalid scope fails closed; oracle 9 (§7). |
| M8 | Fixed: `DateKind` (date/time/date-time/elapsed); `ReadValue::{Time, Duration}`; write kinds pick 14/21/22/46; oracle 4 (§5.2, §6.1, §11.4). |
| M9 | Fixed: decided a separate workspace at `bindings/python/` + root `exclude`; CI job spelled out; root commands unchanged (§10). |
| m1 | Fixed: `Stored` restated to match `stored_extent` (rows from `row` elements, columns from cells, `None` without cells); oracle 8. |
| m2 | Fixed: `Values` includes merges whose anchor has content; `first_free_row(None)` inherits it (§3, §4). |
| m3 | Fixed: `FormulaText` per cell; `Unavailable` reasons; census-blocked behaviour (§5.3). |
| m4 | Fixed: memory estimate, default `max_cells` 1,000,000, Q9 (§5.5). |
| m5 | Fixed: adapter codes stay separate (mapping tables in adapters); `Core` handled in the Debug test (§8). |
| m6 | Fixed: no `Workbook::shared_strings` claim; content check via `pub(crate)` item index without cloning (§3, §5.7). |
| m7 | Fixed: `Workbook::worksheet_ref(SheetRef)` in U2 (§2). |
| m8 | Fixed: `module-name = "ooxml_rs._native"`, abi3 `datetime` check in U7, `fault-injection` feature and a separate test wheel (§10). |
| m9 | Fixed: `save` calls the crates' `save` (§11.1). |
| m10 | Fixed: `NameScope::Sheet(u32)`, `CellWrite` `#[non_exhaustive]`, `roles_incomplete: Option<&'static str>`. |
| m11 | Fixed: LibreOffice expectations generated once by a pinned spike and committed (oracle 5). |
| m12 | Fixed: "gap analysis" reference removed; "the ADR-0021 implementer" throughout; Q7 removed. |
| m13 | Fixed: index conventions stated (§11.4); scope-less named cells → Q11 (§12). |

No finding is rebutted.

## Sources

- `main` at `f88d02c`: `crates/ooxml-xlsx/src/{worksheet.rs, workbook.rs, setters.rs,
  edit.rs, error.rs, graph.rs, registry/dates.rs, registry/format.rs, sheet_ops/mod.rs,
  sheet_ops/grid.rs, formula/precedents.rs}`, `crates/ooxml-core/src/package.rs`,
  `crates/ooxml-docx/src/{replace.rs, error.rs}`, `crates/ooxml-pptx/src/{presentation.rs,
  replace.rs}`, `.github/workflows/ci.yml`, `Cargo.toml`.
- `main` at `213e27a` (post-#170): the Context table, ADR-0021, ADR-0022,
  `crates/ooxml-xlsx-wasm/src/lib.rs` (classifier, batch functions, error codes).
- ADR-0024 (Accepted) and the journal implementation: `crates/ooxml-xlsx/src/journal/op.rs`
  (`CellCall`), `edit.rs` (`CellWritePlan::report`, the C1 per-target report).
- Thread-safety: a compile-time `Send`/`Sync` assertion on `Workbook`, `Document`,
  `Presentation` and `ooxml_xlsx::Error`, run against both trees (Context), repeated by review
  round 1.
- ECMA-376 Part 1 §18.8.30 (built-in number formats), for `DateKind`'s built-in table.
- PyO3 user guide (pyclass thread-safety, `unsendable`, releasing the GIL, abi3) and maturin
  documentation (`module-name`, `python-source`, `features`), at the versions pinned by U7.

## Review response (round 2)

| Finding | Response |
|---|---|
| M1 | Fixed: rebuilt on the Edit Journal per owner decision Q4 (ADR-0024 §13.4). §6.2/§6.3 batches are journal-op batches (multi-op calls); unchanged-cell dropping shares C1's per-target report with R1; session reads go through the View in normalized mode (§5); transactions are `truncate(entry)` with Q5 (A) (§11.2); Delivery re-sequenced after the J series with a J1 exit requirement. |
| m1 | Fixed: the clearing set holds several groups per call; replaced-group detection stays in the shared C1 plan machinery; oracle 7 covers two groups (§6.3). |
| m2 | Fixed: `ReadCell` renamed to `RangeCell` (§5.1). |
| m3 | Fixed: `CLEAR` and `clear()` map to `ClearValue`, which never creates blank cells (§11.4). |
| m4 | Fixed: baseline is current main; sequencing rewritten (ADR-0021 landed, J order, U5 owner); `write_cells`/`insert` location fixed (transient note removed). |

## Review response (round 3)

| Finding | Response |
|---|---|
| M1 | Fixed: the drop predicate is a reads-only §4.4 state-equality extracted for sharing with R1 (not C1's post-validation report, which cannot see past the guards); the F13 relation is stated (no divergence: the new sequential twin drops identically); `write_changed_cells`/`clear_range` exist on both `Workbook` and `Journal` (§6.2, §6.3). |
| M2 | Fixed: `formulas="formula"` returns `ooxml_rs.Formula` objects (round-trippable; Q8 keeps `str` as text); value-mode writes back against formula cells drop on cache equality (a 0023-specific rule, sequential twin identical); identical `Formula` writes drop via the shared report; oracle 3 pins formula preservation (§6.2, §11.4, §12). |
| m1 | Fixed: `clear_range` uses `ClearValue`; U2 adds the missing `CellCall` payloads with sequential references into oracle 1 (§6.3, Delivery). |
| m2 | Fixed: cross-reference is ADR-0024 §10.3 (§11.2). |
| m3 | Fixed: Sources are current main plus ADR-0024 and the journal files. |
