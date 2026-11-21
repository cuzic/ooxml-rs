# ADR-0006: xlsx Phase2 — shared-string and style registries

- Status: **Accepted** (revision 3; review round 2 converged: Accepted after Minor only)
- Date: 2026-11-16
- Depends on: [ADR-0001](0001-lossless-round-trip-architecture.md) (Layer 3, "two seed
  modes"), [ADR-0002](0002-schema-driven-code-generation.md) (lens, allowlist, §4.3 insertion),
  [ADR-0003](0003-xlsx-phase1-formula-and-calc-state.md) (plan → validate → apply, dependency
  invalidation)
- Roadmap: [`docs/roadmap/xlsx.md`](../roadmap/xlsx.md) §2.3 (B1–B5), §2.5 (D1–D7), §6 Phase2
- Evidence: [`spikes/xlsx-phase2/`](../../spikes/xlsx-phase2/README.md) (corpus survey), and the
  throwaway Rust prototype `spikes/xlsx-phase2/proto/` on branch `spike/xlsx-phase2-proto`
  (draft PR, not for merge as code)
- Review: [round 1](0006-opus-review-round1.md), [round 2](0006-opus-review-round2.md)
- **Post-acceptance note (2026-11-16, owner decision):** Q1–Q6 are decided as their proposed
  defaults: exact-form reuse (Q1), an empty `sharedStrings.xml` in the template (Q2), named
  style + overrides deferred to Phase4 (Q3), the preserving setters in Phase2 (Q4),
  `x14ac:knownFonts` left untouched (Q5), and no phonetic runs on new strings (Q6). XL items
  10(c) and 10(e) can still reopen Q5 and Q2.

## Revision history

- **Revision 1**: initial draft.
- **Revision 2** (after `0006-opus-review-round1.md`, and checked against the prototype):
  - **M1:** a `Format` group the caller does not set is *inherited* from the workbook's default
    style by index (`cellStyleXfs[0]`, else `cellXfs[0]`), not filled with rust_xlsxwriter's
    Calibri 11 (§3, §5.5).
  - **M2:** exact-form reuse applies to styles as well as strings, bottom-up over the whole
    chain (`numFmt`, `font`, `fill`, `border`, `xf`), so repeated sessions stop growing the
    tables. Excel's table limits are guarded (§2, §5.10).
  - **M3:** a new custom `numFmtId` is above every `numFmtId` used anywhere in the package, not
    only in `numFmts` (§5.3).
  - **M4:** the `mc:AlternateContent` facts are corrected (the Hancom file has AC entries in
    `fonts`, `cellXfs` and `dxfs`). The counting rule is stated precisely, index-dependent
    operations on tables it does not cover are refused, and oracle 6 is fixed (§2).
  - **M5:** style-only writes are value writes for invalidation purposes whenever any cell
    formula or defined name calls a formatting-reading function (§7.3).
  - **Minor findings m1–m10** are folded in where they apply (missing tables and foreign
    namespaces, dangling references, named-style `apply*`, date ranges, the calamine-visible
    projection, `count` maintenance, prefixes, absent `count`, `Tint`/size equality, the spike
    README).
  - The reviewer's recommendations for Q1–Q6 are recorded as proposed defaults pending the
    owner's confirmation.
- **Revision 3** (after `0006-opus-review-round2.md`, which converged with Minor findings
  only). Status: **Accepted**.
  - **n1:** `sst/@count`, the dangling-reference check, oracle 3 and the formatting-function
    scan cover **every sheet part the Phase1 census scans** (worksheets, macro sheets, dialog
    sheets), not only worksheets. Regression file: `64420.xlsm`, where the macro sheet holds a
    `t="s"` cell. The prototype's check (d) now counts independently, by content type.
  - **n2:** §5.3 no longer forbids reusing existing custom `numFmt`s. Reuse follows §2's
    exact-form rule, and the safety argument is stated.
  - **n3:** `set_named_style` `xf`s are deduplicated by their bytes, not by style name.
  - **n4:** reserved positions (`fills[0]`/`fills[1]`) are reuse targets only when they hold
    the canonical `none`/`gray125`.
  - **n5:** the edit-mode builder starting point (Calibri 11) is documented, with
    `with_default_font_of` as the documented pattern and XL item 10(f). Partial font and
    alignment setters are recorded for Phase4.
  - **n6:** the `numFmtId` scan follows the attribute's lexical rules. It selects parts by XML
    content type, decodes UTF-16, refuses parts it cannot decode, and refuses ids above 65535.
  - **n7:** UDF and `_xll.` behaviour is stated. Style-only writes also trigger on functions
    outside ADR-0003's precise allowlist, and the `CELL` info types are listed fully.
  - **n8:** references to reserved entries count as resolved by the reserved-entry padding.
    The dangling check runs after it.
  - **n9:** the index-stable definition is made precise.
  - **n10:** the limits are marked as inferred mappings. 65,490 applies to `cellXfs` +
    `cellStyleXfs`, the limits sit in a `const` table, and there is a new XL item.
  - **n11:** the `count` wording is unified: the census keeps the number for every write.
  - Q2's reasoning is corrected: rust_xlsxwriter writes an sst only when there are strings.
  - Nits: fill `None` wording, alignment/protection copied as bytes, "default style" = Normal
    (`cellStyleXfs[0]`), `dxfs` have no "inherit", one exact-form map per table with the
    smallest index winning, and the session-2 comparison does not exercise reuse itself.

## Context

ADR-0001 deferred two things to "a later ADR", and this is that ADR:

- **Shared strings.** Phase0 writes every new string as `t="inlineStr"` and never touches
  `sharedStrings.xml`.
- **Styles.** Phase0 creates no style entries. A write keeps the cell's `s`, so a value cannot
  be given a number format, font, fill or border. That also rules out dates, which are numbers
  with a date number format.

ADR-0001 recorded the key constraint when it deferred them (from review B2). A registry
needs **two seed modes**, not one:

- **From scratch:** dedup by value, as rust_xlsxwriter does.
- **Edit:** freeze existing entries as opaque, dedup only newly added entries, append-only.

The two modes share the append-only registry shape. They do not share the dedup logic. Layer
3's invariant also holds: *existing indices are append-only and never renumbered or
compacted*.

### What the reference libraries do (verified in source)

- **rust_xlsxwriter 0.102.0** (`src/workbook.rs`):
  - `Format` is `Hash + Eq` and is deduplicated through a `HashMap<Format, u32>`
    (`format_xf_index`).
  - Fonts, fills, borders and number formats are deduplicated separately just before save
    (`prepare_fonts`, `prepare_fills`, `prepare_borders`, `prepare_num_formats`).
  - Fills start at 2 "since there are 2 default fills: patternType="none" and
    patternType="gray125"".
  - Custom number formats start at 164 ("User defined number formats in Excel start from index
    164") and are shared between `xf` and `dxf` formats.
  - `set_num_format("0.00")` stores the string. A built-in id is used only through
    `set_num_format_index`.
  - Theme colours are `Color::Theme(color, shade)`, written as a fixed table of `tint` strings
    (`src/color.rs`, `attributes`).
  - Shared strings: `SharedStringsTable` (`src/shared_strings_table.rs`) is a
    `HashMap<Arc<str>, u32>` plus `count`/`unique_count`, incremented per reference. A rich
    string is keyed by its `<r>…</r>` XML (`src/worksheet.rs`, `get_rich_string`;
    `src/shared_strings.rs`).
  - Dates (`src/datetime.rs`) handle Excel's false 1900-02-29. The 1904 epoch exists
    internally (`set_1904_date`) and is not a public workbook option.
  - It has no edit path, so it never meets an existing table.
- **openpyxl 3.1.5**:
  - Loading `styles.xml` rebuilds the tables as `IndexedList`s (`styles/stylesheet.py`,
    `apply_stylesheet`).
  - It renumbers custom number formats from 164 and maps custom codes that equal a built-in
    code to the built-in id (`_normalise_numbers`, `BUILTIN_FORMATS_MAX_SIZE = 164` in
    `styles/numbers.py`).
  - Saving writes a new stylesheet (`write_stylesheet`).
  - Strings are written inline (`cell/_writer.py`: `attrs['t'] = "inlineStr"`).
  - In other words, openpyxl's edit path *is* a rebuild. That is the style of renumbering
    Layer 3 forbids.
- **calamine 0.38.0** reads styles (`Xlsx::worksheet_style`, `src/style.rs`). It resolves
  `theme` + `tint` to RGB through the theme part and swaps theme indices 0↔1 and 2↔3
  relative to `clrScheme` order (`src/xlsx/style_parser.rs`). It reads `workbookPr/@date1904`
  (`has_1904_epoch`). This makes it the independent read-back oracle for both values and
  styles.

### What the corpus shows

`spikes/xlsx-phase2/registry_survey.py` over 342 Transitional POI packages and the 24 committed
fixtures (full table in the spike README). The facts that drive decisions:

1. **15% of existing `cellXfs` entries (859/5686, in 53/340 packages) cannot be expressed by a
   value `Format`.**
   - Almost all of them because of `xfId` ≠ 0 (47 packages), which attaches the cell to a named
     style, or `pivotButton` (6).
   - Gradient fills, font `outline`/`shadow` and odd colour attributes do not occur.
   - So a `Format` model can describe most entries, but not all. Reading must be able to say
     "not expressible".
2. **Deduplicating new formats against existing entries by value would silently merge
   distinct entries.**
   - In 75/340 packages, two existing `cellXfs` map to the same `Format` projection although
     their XML differs (388 groups).
   - 263 of those groups differ only in `apply*` flags. 115 differ in `xfId`, a different named
     style and so a real difference in meaning.
   - Exact duplicates exist anyway: 41 packages already have identical `xf` entries, and 53
     identical fonts. Excel itself does not keep these tables minimal.
3. **Custom number formats do not reliably start at 164.**
   - 32 packages define a custom `numFmt` with an id below 164, overriding a "built-in" id.
     One (`52348.xlsx`) redefines ids 0–2.
   - 16 define a custom format whose code equals a built-in code.
   - Mapping "0.00" to built-in id 2 is therefore wrong in a file that redefines id 2.
4. **`numFmts` is absent in 234/340 `styles.xml`.** The first custom number format must insert
   the element in its schema slot (first child of `styleSheet`).
5. **`sst/@count` is wrong in 7% of packages (18/247), 12 of them written by Excel.**
   `uniqueCount` is wrong in 3, and either attribute may be absent. calamine uses `uniqueCount`
   only as a capacity hint (`reserve`) and builds the table from the `si` list
   (`read_shared_strings`); it never reads `count`.
6. **9% of shared-string tables contain the same flattened text twice**, 11 of them with
   different XML (rich vs plain, phonetic, spacing). A by-text lookup would pick one silently.
7. **Vendor extensions are common and must stay untouched.**
   - `x14ac:knownFonts` (172/340) and `mc:Ignorable` (188).
   - x14/x15 `styleSheet/extLst` (186/94).
   - Custom `indexedColors` palettes (13), which change the meaning of `indexed` colours.
   - One Hancom file (`style-alternate-content.xlsx`) wraps whole *entries* in
     `mc:AlternateContent`: 6 inside `fonts`, 7 inside `cellXfs` (22 `xf` + 7 AC = `count`
     29) and 11 inside `dxfs`. Every one has a `Choice` and a `Fallback` with exactly one entry
     each (review M4). Indices in those tables depend on MCE processing.
8. `fills[0]` = `none` / `fills[1]` = `gray125` hold in 96% of packages. The rest are POI- or
   unknown-produced files (two have no `fills` at all).
9. 95 of 342 packages (28%) have **no `sharedStrings.xml`**. 20 use the 1904 date system (19
   written by Excel).
10. **`numFmtId` is a package-wide id space** (review M3).
    - `ConditionalFormattingSamples.xlsx` (Excel): `numFmts` ends at 173, `dxfs/dxf/numFmt`
      uses 174–177 with other codes.
    - `LIBRE_OFFICE-128382-0.xlsx`: `numFmts` holds 160 and 161, while `pivotTable1.xml` and
      `pivotCacheDefinition1.xml` refer to `numFmtId="165"`, which is not defined anywhere.
    - A new id allocated from `numFmts` alone would collide in the first file, and in the
      second it would give a dangling pivot reference a meaning.
11. **The workbook's default font is not Calibri 11 in 135/340 packages** (40%; Arial 10 in 47,
    Verdana 10 in 19, Arial 11 in 13, Calibri 12 in 11, East Asian fonts in others; review M1).
    Excel formats a cell from its cell `xf` alone ("In Office, only the cell xf record defines
    the formatting applied to a cell", MS-OE376, quoted in LibreOffice's
    `sc/source/filter/oox/stylesbuffer.cxx`), so a new `xf` with a Calibri 11 font *looks*
    different in such a workbook.
12. **`styles.xml` is not always complete.**
    - `47889.xlsx`: an empty `<styleSheet …/>` with no tables at all.
    - `sample-beta.xlsx`: the Excel 2007 beta namespace
      (`http://schemas.microsoft.com/office/excel/2006/2`), not SpreadsheetML.
    - `52348.xlsx` and `renamed_workbook_part.xlsx`: no `cellStyleXfs`, so `xfId="0"` would
      dangle. `no_drawing_patriarch.xlsx` already has a dangling `xfId`.
    - `fixture_minimal`: one fill only (index 1 missing).

### Prototype results

A throwaway prototype (`spikes/xlsx-phase2/proto/`, branch `spike/xlsx-phase2-proto`) runs the
edit-mode registry on every fixture and the POI corpus. It uses `ooxml-core`'s lens with
hand-written specs for the style elements. Per package it:

- appends one shared string and one style chain (`numFmt`, `font`, `fill`, `border`, `xf`);
- writes them to a new last row;
- adds a second `xf` that sets only the number format and inherits font, fill and border from
  the default style (M1);
- saves.

It does **not** implement reuse itself. The session-2 comparison shows that reuse *would*
match (ids aside); the reopen oracle in 6 tests the real thing. It then checks (a) index
stability, (b) T2-append byte locality, (c) calamine values,
number-format codes, bold font, fill colour and CR of every other cell, and (d) `count` /
`uniqueCount`. It also replays the same edit in a second session to see whether exact-form
reuse would hit (M2).

| Result | Fixtures (26 files) | POI (366 files) |
|---|---|---|
| all checks (a)–(d), L1, CR pass | 21 | 330 |
| refused before any change | 3 (2 rows without `r`, `style-alternate-content`: AC in `fonts`) | 5 (4 rows without `r`, `style-alternate-content`) |
| skipped (not a readable Transitional package: Strict, encrypted, corrupt zip, DTD, beta namespace) | 2 | 31 |
| failures | 0 | 0 |
| `numFmts` had to be inserted | 14 | 228 |
| M1: inherited-font `xf` reads (calamine) the workbook default font, not Calibri 11 | 21/21 | 329/329 (default font ≠ Calibri 11 in 121 of them) |
| M2: a second session serialises byte-identical `font`/`fill`/`border` and id-only-different `numFmt`/`xf` | 21/21 | 329/329 |
| M3: a `numFmtId` above the `styles.xml` maximum is used elsewhere (dxf or pivot) | 0 | 3 (`ConditionalFormattingSamples` dxf 177 > 173; `dataValidationTableRange` dxf 165, no `numFmts`; `LIBRE_OFFICE-128382-0` pivot 165 > 161) |
| table empty or absent, so a default entry must come first (index 0) | 1 (`fixture_minimal`: fill 1) | 1 (`47889`: fonts, fills, borders, cellXfs) |
| no `cellStyleXfs`: the new `xf` must omit `xfId` | 2 | 2 |

Counterexamples the prototype found to revision 1, beyond the review:

- **An empty table makes the first new entry index 0, the default.** In `47889.xlsx` the first
  appended `xf` became index 0: the style of every cell without `s`. Revision 1 only padded
  fills 0/1. Revision 2 pads every table whose index 0 has a fixed meaning (§5.4).
- **Exact-form reuse cannot compare `numFmt` and `xf` bytes directly.** Their bytes contain ids
  allocated in the session (a new `numFmtId`; the `fontId` etc. of new entries). Reuse has to
  run bottom-up: first find the reused `font`/`fill`/`border`/`numFmt` ids, then compare the
  `xf` built from them (§2).
- **Row placement in a full sheet.** `style-alternate-content.xlsx` uses row 1,048,576, so a
  "new last row" does not exist. The registries do not care, but the test harness must pick
  free cells, not "last row + 1".


### `x14ac:knownFonts`

The XLSB counterpart, `BrtKnownFonts` ([MS-XLSB] 2.4.695), "specifies that typographical descent
information is stored", and "MUST be present if and only if a `BrtRwDescent` record is stored
for every non-empty row". The XLSX attribute (`x14ac:knownFonts`, [MS-XLSX] 2.5.3 lists it as a
global attribute) presumably carries the same claim about `x14ac:dyDescent` on rows. This is an
inference: the XLSX text was not checked.

Phase0 already inserts rows without `x14ac:dyDescent` into such workbooks. Phase2 does not make
this worse: it adds `font` entries, not rows. It is recorded as an XL-check item ("Test oracles" 10(c)) and an
open question.

## Decision

### 1. Scope

In scope:

- **Shared-string registry.** `set_string` writes `t="s"` when the workbook has a
  `sharedStrings.xml`. Rich strings are supported as well.
- **Style registry** over `numFmts`, `fonts`, `fills`, `borders` and `cellXfs`, with a
  value-typed `Format`. `dxfs` gets the same registry shape (used from Phase4: conditional
  formats, tables).
- **Writes with a format**, style-only writes, applying an existing style by index or by named
  style.
- **Dates and times** (`set_datetime`), with 1900 and 1904 epochs. Also `set_bool` and
  `set_error`.
- **Reading formats:** `CellRef::format()` returns a `Format` or "not expressible", and
  `CellRef::is_date()`.
- Both seed modes: `Workbook::new()` (template) and opened files.

Out of scope (later phases):

- Creating or editing named styles (`cellStyleXfs`, `cellStyles`): roadmap D3, Phase4.
- Writing phonetic runs (`rPh`).
- Gradient fills.
- Compacting or garbage-collecting tables (never; Layer 3).
- Creating a `sharedStrings.xml` or `styles.xml` part in a file that has none. That needs new
  parts, `.rels` and `[Content_Types].xml` (Phase3). Until then, §4.6 and §5.9 define fallbacks.
- Column and row default styles (`col/@style`, `row/@s`): Phase4 (D6).

### 2. Registries (Layer 3)

`ooxml-xlsx::Workbook` owns two registries, `SharedStrings` and `Styles`.

- **Lazy activation.** They are created on the first call that needs them.
  - Until then, `styles.xml` and `sharedStrings.xml` stay as Phase0 left them: raw, or
    dirty-free `Parsed`, and L1-copied.
  - A registry, once active, holds its part as a Layer 2 tree. Only appended nodes and changed
    `count` attributes are dirty.
- **Append-only.**
  - New entries are created nodes appended as the last child of their table (`si` under
    `sst`; `font` under `fonts`; …).
  - No existing entry is moved, removed, reparsed into a typed value or re-serialised.
  - Indices handed out are `existing_len + k`.
- **Existing entries are opaque** in both modes. A registry knows how many there are and their
  raw XML, and nothing else. They are never parsed into values and never matched by meaning.
  The one comparison allowed is *exact-form reuse* (below), which compares bytes against our
  own serialisation.
- **Two seed modes.**
  - **From scratch** (`Workbook::new()`): the template's entries are seeded *as values*. The
    template is ours, and its `styles.xml` is built from `Format::default()`. Any new `Format`
    equal to one of them reuses its index, which is full rust_xlsxwriter-style dedup.
  - **Edit** (opened file): the existing entries are seeded as opaque. New entries are
    deduplicated among themselves by value, and against existing entries only by exact form.
  - The mode is fixed at open and is not a user option.
- **Index lookups are per table, bottom-up.**
  - A new `Format` resolves first to a `numFmtId`, a font index, a fill index and a border
    index, each in its own table. Only then is the `xf` built from those ids and resolved in
    `cellXfs`.
  - A group the caller did not set is not resolved at all: the new `xf` takes the default
    style's id for it (§3 "inherit", review M1).
- **Exact-form reuse** (review M2; Q1). An existing entry is reused, instead of appending, when
  it is byte-for-byte what the registry would append:
  - `font`, `fill`, `border`, `si`: the entry's source bytes equal the canonical serialisation
    of the value. The prefix and namespace declarations are those the entry would get in that
    table (§5.11).
  - `numFmt`: an entry with id ≥ 164 whose `formatCode` attribute equals the canonical code
    and that has no other attributes or children. The id itself is an identity, not content.
  - `xf`: the bytes equal the canonical `xf` built from the ids resolved above. So an `xf` is
    reused only when its whole chain is reused.
  - Excel-written entries rarely match, because Excel's `apply*` patterns and attribute order
    differ from the canonical form. So this does not merge entries that differ in anything a
    reader sees (Context fact 2 still holds).
  - Entries an earlier ooxml-rs session appended *do* match: the serialisation is
    deterministic. The prototype confirms this on 329/329 POI packages. Repeated batch runs
    therefore stop growing the tables.
  - Cost: one pass over each table on activation, building a `HashMap<bytes, index>`.
    - There is one map **per table**: `dxf/font` and `fonts/font` give omission different
      meanings.
    - If the same bytes occur twice, the smallest index wins.
  - Reserved positions are reuse targets only for their reserved meaning (review n4):
    `fills[0]` only when it is the canonical `none` fill, and `fills[1]` only when it is the
    canonical `gray125` fill. 10 POI files have `lightGray` or `darkGray` at `fills[1]`,
    byte-identical to a canonical `lightGray` fill. `fonts[0]`, `borders[0]` and `cellXfs[0]`
    are ordinary reuse targets: their reserved meaning is their content.
- **Counting entries and `mc:AlternateContent`** (review M4).
  - A table is *index-stable* when:
    - every `mc:AlternateContent` child has a `Fallback`;
    - every branch (`Choice` and `Fallback`) contains exactly one element of the table's entry
      type, matched by QName (`x:font` and `font` are the same; `x14:font` is not), and no
      other element. Whitespace, comments and PIs are allowed;
    - the table has no child element that is neither an entry nor such an AC.

    In an index-stable table each AC child counts as one entry, whichever branch a reader
    picks. Every corpus AC is of this shape (review n9).
  - An `mc:AlternateContent` directly under `styleSheet` that wraps a whole table does not
    make that table "absent". Inserting the table (§5.4) is refused, and so is reading the
    default style when it is `cellStyleXfs` that is wrapped. There is no corpus example.
  - Reading the default style (`cellStyleXfs[0]`, §3) and writing `xfId="0"` are
    index-dependent too. They are refused when `cellStyleXfs` is not index-stable.
  - In any other table with AC children, no index can be computed. Every index-dependent
    operation on it is refused with `Err(UnresolvedSibling)`: reuse, range checks
    (`set_style_index`), `count`/`uniqueCount` maintenance, and the dangling-reference check.
  - **Appending** to a table with *any* AC child is refused (`Err(UnresolvedSibling)`), as the
    ADR-0002 §4.3 insertion rule and Phase0's dry run already do. Range checks and reuse in an
    index-stable table are allowed.
  - In `style-alternate-content.xlsx` this means:
    - any new `Format` is refused, because `cellXfs` has AC children whatever groups the
      `Format` sets;
    - `set_style_index(n)` succeeds for `n < 29` (22 `xf` + 7 AC);
    - shared strings are unaffected (its sst has no AC).
- **Dangling references** (review m2).
  - An existing reference at or beyond a table's end would silently start pointing at the new
    entry once something is appended. Examples: a cell `s`, an `xf`'s `fontId`, a
    `cellStyle/@xfId`, a pivot `numFmtId`.
  - Before the first append to a table, the registry checks the references it can see:
    - `s` on every cell, row and column of **every sheet part the Phase1 census scans**
      (worksheets, macro sheets, dialog sheets; review n1);
    - the id attributes inside `styles.xml`;
    - pivot `numFmtId`s, which are avoided by §5.3's id allocation anyway.
  - If any of them is out of range, the append is refused with
    `Err(DanglingReference { table, index })`. Reuse and range checks are still allowed.
  - **Reserved entries come first** (review n8). A reference to a reserved position
    (index 0, or `fills[1]`) of an empty or absent table is resolved by §5.4's padding: the
    padding gives it exactly the default meaning a reader already assumes for it, for example a
    cell `s="0"` with no `cellXfs`. The dangling check therefore runs on the table length
    **after** the reserved entries are added.
  - The prototype found no out-of-range cell `s` or `fontId` in the corpus. `xfId` in
    `no_drawing_patriarch.xlsx` dangles, but `cellStyleXfs` never grows in Phase2.

### 3. Value types

```rust
pub struct Format {                    // Hash + Eq; builder methods as rust_xlsxwriter
    pub num_format: Option<NumFormat>, // None = inherit; General | Builtin(u16) | Code(String)
    pub font: Option<Font>,            // None = inherit; name, size, bold, italic, underline,
                                       // strike, vert_align, color, family, charset, scheme,
                                       // condense, extend
    pub fill: Option<Fill>,            // None = inherit; NoFill | Pattern { pattern, fg, bg }
    pub border: Option<Border>,        // None = inherit; sides (style, Color); diag up/down
    pub alignment: Option<Alignment>,  // None = inherit
    pub protection: Option<Protection>,// None = inherit
    pub quote_prefix: bool,
}
pub enum Color { Default, Auto, Rgb(u32 /*ARGB*/), Indexed(u8), Theme { index: u8, tint: Tint } }
pub struct Tint(f64);   // finite, in [-1, 1], -0.0 stored as 0.0; Eq/Hash on the bits
pub struct FontSize(f64); // finite, 1..=409, Eq/Hash on the bits; serialised shortest round-trip
```

- **Inherit** (review M1).
  - Every group is optional. `Format::new()` sets nothing: a cell written with it looks like
    the workbook's default style.
  - "Default style" means the workbook's Normal style, `cellStyleXfs[0]`. It can differ from
    the style of a cell without `s` (`cellXfs[0]`): 3 POI files have a different `fontId`
    there, and 10 a different alignment. Normal is the intended base.
  - Builders set a whole group, starting from the inherited value where that is known:
    - from scratch, the template's default (Calibri 11, as rust_xlsxwriter);
    - in edit mode, the group is opaque. `set_bold()` therefore starts from
      `Font::default()` (rust_xlsxwriter's Calibri 11), **not** from the workbook font. The
      same holds for alignment: `set_align(Center)` starts from the default alignment, so the
      `vertical="center"` that Japanese Excel writes in `cellStyleXfs[0]` (41 POI files) is
      not kept.
  - **Documented pattern** (review n5): in edit mode, start from
    `Format::with_default_font_of(&Workbook)`. It copies the workbook's default font into the
    value when that font is expressible, so a bold font keeps the workbook's face and size. The
    API docs show this pattern, and XL item 10(f) checks it. Partial font and alignment setters
    (copy the default entry's bytes and replace one child) are recorded for Phase4.
  - **Post-acceptance note (issue #115, owner decision 2026-11-21).** `Font::default()`
    keeps rust_xlsxwriter's `<scheme val="minor"/>`. A real Excel 365 (16.0.20326) shows such
    a font as the workbook theme's minor font, not the written `Calibri`. In a workbook
    without `xl/theme/theme1.xml` (POI 62272), it shows Excel's own default (Aptos Narrow).
    The behaviour is kept: when the workbook's default font is itself a `scheme="minor"`
    font (Excel's own templates), the new font then shows the same face as the body text,
    as Excel's own styles do. When the default font has no scheme (POI 62272: Arial 10, no
    theme), it does not match; `with_default_font_of` is the pattern for that case. The
    documentation and XL item 10(f) now say so.
  - **Resolution of an unset group.** The new `xf` takes the corresponding id (`numFmtId`,
    `fontId`, `fillId`, `borderId`) by index, and the child elements (`alignment`,
    `protection`) as a byte copy, from the default style: `cellStyleXfs[0]`, or `cellXfs[0]`
    when there is no `cellStyleXfs`. No existing entry is parsed or matched.
  - `dxfs` have no "inherit". In a `dxf`, an omitted group means "do not override", and there
    are no ids to copy. Phase4 must not copy default-style ids into a `dxf`.
  - Its `apply*` flag stays unset, as in an Excel-written `xf` that follows its style.
  - The prototype checks that calamine reads such an `xf`'s font as the default font. It did in
    329/329 POI packages, of which 121 have a default other than Calibri 11.
  - Excel and LibreOffice format a cell from the cell `xf`, so the copied id is what they show
    (Context fact 11).
- **Equality** (review m9).
  - `Tint` and `FontSize` reject NaN, infinities and out-of-range values at construction. They
    normalise `-0.0` to `0.0` and compare bits. Two tints that print differently in Excel's
    files (`0.39997558519241921` vs `0.399975585192419`) are different values, which is
    harmless: they are never compared with existing entries except byte-wise.
  - `NumFormat::Code("0.00")` and `NumFormat::Builtin(2)` are different `Format` values. They
    resolve to the same `numFmtId` (§5.3), so they produce one `xf`: `xf` dedup is on resolved
    ids.

- **Model.** The model is rust_xlsxwriter's feature set, with four deliberate differences
  (the fourth being "inherit", above):
  1. **Colours are raw.** `Theme { index, tint }` takes any tint, not only rust_xlsxwriter's
     `shade` table. Excel writes at least 21 spellings (8 outside that table). `Indexed(n)` is
     kept as a number, because 13 corpus files redefine the palette.
     - Convenience constructors give rust_xlsxwriter's `Theme(color, shade)` values and the
       same tint strings.
     - The theme index is the *attribute* index (0 = `lt1`, 1 = `dk1`, as calamine documents),
       not the `clrScheme` position.
  2. **`NumFormat::Builtin(id)` and `Code(s)` are distinct.** §5.3 says when a code becomes a
     built-in id.
  3. **No checkbox, hyperlink style or `pivotButton`.** Those are not cell formats a caller
     sets.
- **Canonical serialisation.** Each value has exactly one XML form:
  - attribute order as rust_xlsxwriter writes it;
  - default values omitted;
  - `apply*` flags set exactly for the non-default groups, as rust_xlsxwriter does.
  - Dedup compares values. The canonical form makes the output deterministic and gives the
    byte-level oracles ("Test oracles") something fixed to compare against.
- **Reading.**
  - `CellRef::format()` returns `Ok(FormatRead::Expressible(Format))`, or
    `Ok(FormatRead::NotExpressible { style_index, reasons })` when the cell's `xf`, or the
    font/fill/border it uses, has anything outside the model.
  - The reasons are those the spike counts: `xfId` ≠ 0, `pivotButton`, `justifyLastLine`,
    `relativeIndent`, unknown children or attributes, `mc:AlternateContent`.
  - `apply*` flags are ignored when reading. That is what makes a projection lossy, which is
    exactly why it is never used to *match* an existing entry (§2, Context fact 2).
  - `CellRef::style_index()` (Phase0) stays the lossless handle.

### 4. Shared-string registry

1. **Activation.** The first `set_string`/`set_rich_string` on a workbook that has a
   `sharedStrings.xml` part (located through the workbook relationship, as Phase0 does).
   - **Behaviour change** (review m6): Phase0's `set_string` writes `inlineStr`. In Phase2 it
     writes `t="s"` when the workbook has an sst. The new `set_string_inline` keeps Phase0's
     behaviour (it does not exist yet; Phase2 adds it).
   - Other writes that only *remove* a `t="s"` reference (`set_number` over a shared-string
     cell) do **not** activate the registry. The part stays L1 and its `count` may become
     stale by one. This is tolerated: 7% of the corpus already has a wrong `count`, and no
     reader we know of uses it.
2. **Keys.**
   - A plain string is keyed by its text: the caller's string, before `_xHHHH_` escaping.
   - A rich string is keyed by its run list `[(Font-subset, text)]`. Its XML is `si/r/rPr` +
     `t`, where `rPr` (`CT_RPrElt`) uses `rFont` for the font name, as Excel writes it.
   - Plain and rich never collide, even if the flattened text is equal.
3. **Exact-form reuse** (§2; Q1). A new string reuses an existing `si` only if that `si` is
   byte-for-byte the canonical form the registry would write for it.
   - For a plain string that means a single `t` child, `xml:space="preserve"` iff the text has
     outer whitespace, the same `_xHHHH_` escaping, and no `r`/`rPh`/`phoneticPr`/attributes.
   - For a rich string it means the same runs and the same canonical `rPr`.
   - This is a byte comparison against our own serialisation, not a semantic match. It cannot
     merge two entries that differ in anything a reader could see. Phonetic existing items
     never match, because we never write phonetic runs.
   - It refines ADR-0001's "dedup only newly-added entries". Reusing an entry that is exactly
     what we would have appended changes no meaning, and it never renumbers anything.
   - The reviewer's caveat: most items in Japanese Excel files carry `phoneticPr`, so the gain
     there is small.
   - It needs an index-stable sst (§2).
4. **Whitespace and escapes.** The Phase0 rules apply:
   - `xml:space="preserve"` iff outer whitespace;
   - `_xHHHH_` for characters XML cannot carry;
   - `_x005F_` protection;
   - at most 32,767 UTF-16 units.
5. **`uniqueCount` and `count`.**
   - `uniqueCount` is set to the number of `si` entries after the append (§2 counting).
   - `count` is the number of `t="s"` cells in **every sheet part the Phase1 census scans**:
     worksheets, macro sheets and dialog sheets (review n1). Counting worksheets only breaks
     a correct `count` written by Excel: in `64420.xlsm` the macro sheet holds one of the
     three `t="s"` cells. That file is a regression test.
   - One rule (review n11): the census counts `t="s"` cells at open. Phase2 adds this to the
     scan; `census.rs` on `impl/xlsx-phase1` does not count them yet. **Every** cell write in
     the session keeps the number current, whether it is a Phase0, Phase1 or Phase2 write,
     and whether or not the registry is active. When the sst registry is active, the number
     is written to `sst/@count` on save. When it is not, the part stays L1 (§4.1).
   - The prototype counts independently, by content type over all parts, and finds `count`
     exact after every append (rerun after the n1 fix).
   - Writing the census number, rather than adding a delta to the stored value, corrects the
     7% of files whose `count` is already wrong instead of carrying the error forward.
   - An absent attribute stays absent (8 and 7 corpus files): adding it is not needed and would
     touch a byte nobody asked for.
6. **No `sharedStrings.xml`.** 28% of the corpus has none.
   - `set_string` keeps writing `inlineStr` (Phase0 behaviour).
   - `set_rich_string` writes an inline rich string (`is/r`, valid per `CT_Rst`).
   - `Workbook::shared_strings_available()` tells the caller which happened.
   - When Phase3 can add parts, an opt-in `create_shared_strings()` creates the part.
   - The from-scratch template ships an empty `sharedStrings.xml` (`count="0"
     uniqueCount="0"`), so from-scratch files always use shared strings.
   - This is the proposed default for Q2, subject to XL item 10(e).
7. **Old strings are never removed.**
   - Overwriting a `t="s"` cell leaves its `si` in place. Unreferenced items are valid
     (nothing in §18.4 requires every item to be referenced). Whether Excel drops them on its
     next save is not verified, and it does not matter for correctness.
   - `set_string_inline` remains for callers who want Phase0's byte footprint.

### 5. Style registry

1. **Activation.** The first call that needs a new style entry: a write with a `Format`, or
   `set_format`.
2. **Tables and order.**
   - A new `Format` produces, in order: its `numFmt` (if custom and new), `font`, `fill`,
     `border`, then `xf` in `cellXfs`. Each is deduplicated per table (§2).
   - `count` on every table that grew is set to its new entry count. Tables that did not grow
     are untouched, including a `count` that was already wrong (≤ 2 corpus files per table).
   - A grown table **without** a `count` attribute keeps it absent (review m8), as for the sst.
     A table the registry inserts gets one.
3. **Number formats.**
   - `NumFormat::Builtin(id)` is written as that id, *unless the file redefines that id* in
     `numFmts`. Then it is `Err(BuiltinNumFormatRedefined { id })` (32 corpus files redefine
     some id below 164).
   - `NumFormat::Code(s)` uses the built-in id when `s` is exactly a locale-independent
     built-in code (ECMA-376 §18.8.30 table, as openpyxl's `BUILTIN_FORMATS`) *and the file
     does not redefine that id*. Otherwise it gets a new custom id.
   - **The new id is above every `numFmtId` used anywhere in the package** (review M3):
     `max(163, every numFmtId="N" attribute in any XML part, every id handed out) + 1`.
     - "Any XML part" covers everywhere ids are used outside `numFmts`: `styles.xml` (`xf`,
       `dxf/numFmt`, x14 `dxfs` in `extLst`), pivot tables and pivot cache definitions, and
       worksheet `extLst` (x14 conditional formats).
     - A scan for the attribute is enough for a maximum, because an over-estimate is
       harmless. The scan runs once on activation. It must not *under*-estimate (review n6):
       - it follows the attribute's lexical rules: `numFmtId`, optional whitespace, `=`,
         optional whitespace, then `"` or `'`;
       - it selects parts by XML content type (`…/xml`, `…+xml`), not by file name (one corpus
         file has a part named `xl\styles.xml`);
       - it decodes UTF-16 parts. A part that cannot be decoded refuses the write; it is never
         skipped.
       - A resulting id above 65535 is refused (`Err(NumFmtIdExhausted)`). The XLSB `ifmt` is
         2 bytes, and the exact Excel ceiling is not verified.
     - Charts' `c:numFmt` has no id (only `formatCode`/`sourceLinked`), so it is out of the
       scan. A chart with `sourceLinked="1"` displays the cell's number format, so a
       style-only write can change how a chart looks. That is expected.
     - The prototype does exactly this. It found 3 POI packages where a package-only scan would
       have collided or given a dangling pivot reference a meaning (Context fact 10).
   - Custom codes are reused **per §2's exact-form rule** (review n2): an existing `numFmt` with
     id ≥ 164, the same `formatCode`, and no other attributes or children.
     - This is safe for three reasons:
       - the same code displays the same way;
       - an id ≥ 164 cannot be a built-in redefinition (those are the files that redefine
         ids < 164);
       - with duplicate codes (3 files) any of them displays the same, and the smallest id
         wins.
     - Without this reuse, a numFmt and its `xf` would grow every session (review n2; M2
       again).
     - Codes equal to a built-in code are still **not** mapped to the built-in id when the
       file defines them as customs (16 files). The built-in id is used only as stated
       above.
   - If `numFmts` is absent (234/340), it is inserted as the first child of `styleSheet`
     through the ADR-0002 insertion primitive. An `mc:AlternateContent` sibling there makes the
     write `Err(UnresolvedSibling)`.
4. **Reserved leading entries** (revision 2, from the prototype).
   - Index 0 has a fixed meaning in `fonts` (the default font), `borders` (no border), `cellXfs`
     (the style of every cell without `s`) and `fills`. `fills` index 1 is fixed as well
     (`gray125`).
   - When a table is empty or absent, its reserved entries are appended first: a default font
     copied from the template, an empty border, an `xf` with all ids 0, and `none` /
     `gray125` fills. Only then is the new entry appended. Otherwise the first new entry would
     *become* the default.
     - `47889.xlsx` (no tables at all) hit this for four tables.
     - `fixture_minimal` has one fill only, so `gray125` is appended first.
   - Missing tables are inserted in schema order through the ADR-0002 insertion primitive
     (review m1). The prototype inserted `numFmts` in 228 POI packages and all four tables in
     `47889.xlsx`.
   - A `styleSheet` whose root is not SpreadsheetML (`sample-beta.xlsx`, Excel 2007 beta
     namespace) makes format writes `Err(UnsupportedStylesPart)`. Reads ignore its styles, as
     today.
   - Existing non-canonical entries at 0/1 (13 files written by POI, ONLYOFFICE or an unnamed
     producer) are left alone.
   - A `Format` with `Some(Fill::NoFill)` reuses index 0 only when `fills[0]` is the canonical
     `none` fill (the §2 reserved-position rule); otherwise a new `none` fill is appended.
5. **New `xf` attributes.**
   - `xfId="0"`: the first `cellStyleXfs` entry, which is `Normal` in every corpus file that has
     `cellStyles` (`builtinId="0"`). A file whose `xfId=0` is not `Normal` is still valid; the
     cell is then attached to that style.
   - When `cellStyleXfs` is empty or absent (`52348.xlsx`, `renamed_workbook_part.xlsx`),
     `xfId` is omitted instead of written dangling (review m1).
   - Unset groups copy the default style's ids (§3 "inherit").
   - `apply*` flags in rust_xlsxwriter's pattern.
   - No `x14ac`, `xr` or other extension attributes. They are optional (`mc:Ignorable`).
   - `fonts/@x14ac:knownFonts` is left as it is. See Context; this is XL-check item 10(c) under "Test oracles".
6. **Applying existing styles.** These do not create or match registry entries:
   - **By index:** `CellMut::set_style_index(n)` sets `s="n"` after checking
     `n < cellXfs entry count`. This is the lossless way to copy a style from another cell
     (`cell.style_index()`).
   - **By named style:** `CellMut::set_named_style("Good")` finds `cellStyles/cellStyle[@name]`.
     - The name must be unique: duplicates (4 corpus files) give `Err(AmbiguousStyleName)`, a
       missing name gives `Err(NoSuchStyle)`.
     - It then appends one `xf` with `xfId` = the style's `cellStyleXfs` index.
     - Each group (number format, font, fill, border, alignment, protection) comes from the
       style **only if the style's entry applies it**. That is the case when its `apply*`
       attribute is absent or `1`.
     - A group the style does not apply (`apply*="0"`, e.g. Good/Bad/Neutral with
       `applyNumberFormat="0"` in 6 corpus files) keeps the cell's current id for that group,
       copied from the cell's existing `xf` by index (review m3). That is how Excel applies a
       style: it does not touch the cell's number format.
     - `apply*` flags are written for the groups taken from the style.
     - These `xf` entries are deduplicated by their **bytes** (§2's exact-form key), not by
       style name (review n3). Two cells with different number formats that get "Good"
       produce two `xf`s.
     - Named style + overrides (`Format` on top of a style) is deferred to Phase4 (Q3, proposed
       default).
   - **Preserving edits** (Q4, adopted as proposed default): `CellMut::set_num_format(code)`,
     `set_font(…)`, `set_fill(…)`, `set_border(…)` derive a new `xf` from the cell's
     **current** `xf`.
     - They copy its bytes and replace only the changed id attribute and the matching
       `apply*` attribute.
     - The rest of its look is kept losslessly, including features a `Format` cannot express.
     - It is the same kind of derivation as `set_named_style`: ids are copied by index, and
       nothing is parsed into a value.
     - Exact-form reuse applies to the derived bytes.
7. **Interaction with `s` on writes.**
   - `set_number`/`set_string` (Phase0) keep `s`.
   - `set_*_with_format(value, &Format)` replaces it.
   - `set_format(&Format)` changes only `s`, and the value is untouched.
   - `set_datetime` keeps `s` if the existing `xf` is a date format (§6), else sets the default
     date format.
8. **`dxfs`.** Same registry shape: append-only, edit-mode dedup among new entries, a
   `DxfFormat` value type. No Phase2 API uses it; Phase4 (conditional formats, D5) does. It is
   specified here so that Phase4 does not invent a second registry.
9. **No `styles.xml`** (2 corpus files). Format writes return `Err(NoStylesPart)` until Phase3.
10. **Excel's limits** (review M2). Appending is refused with
    `Err(StyleTableFull { table, limit })` past these limits, kept in a `const` table so they
    can be replaced. Microsoft's table names the limits, but its mapping onto tables is
    **inferred** (review n10):
    - 65,490 "unique cell formats/cell styles", applied to `cellXfs` + `cellStyleXfs`
      (conservative: `53568.xlsx` has 3089 `cellStyleXfs`);
    - 256 "fill styles" (`fills`) and 256 "line weight and styles" (`borders`, inferred);
    - 512 "unique font types per workbook" (`fonts`, inferred: it may mean font names);
    - 200 number formats (conservative: Excel's documented limit is 200–250 depending on the
      language version).

    XL item 10(g) opens files above each inferred limit to check. Exact-form reuse keeps
    repeated runs far below them: the corpus maximum is 41 fonts, 36 fills, 74 borders, 482
    `cellXfs` and 12 `numFmts`.
11. **Prefixes** (review m7). New nodes take the namespace and prefix of the table they are
    appended to, by ADR-0002 §3's prefix rules as applied by the insertion primitive. For
    example, `x:font` in an `x:`-prefixed `styles.xml`. The canonical bytes used for
    exact-form reuse are computed in that context.

### 6. Dates and times

- **Writing.** `set_datetime(ExcelDateTime)` writes a serial number. The conversion is
  rust_xlsxwriter's (`src/datetime.rs`):
  - days since 1899-12-31, plus 1 for dates after 1900-02-28 (Excel's false 1900-02-29, serial
    60);
  - 1904 epoch when `workbookPr/@date1904` is `1`/`true` (20 corpus files);
  - the time as a fraction of a day.
  - The range is defined by date, not serial (review m4):
    - 1900 system: 1899-12-31 (serial 0, which rust_xlsxwriter accepts) to 9999-12-31;
    - 1904 system: 1904-01-01 to 9999-12-31 (serial 2957003);
    - a time-only value (serial in [0, 1)) is allowed in both systems.
    - Anything else is `Err(DateOutOfRange)`.
  - openpyxl's `to_excel` uses the same 60-day adjustment (`utils/datetime.py`).
- **Default format.** rust_xlsxwriter has none: the caller passes a `Format`. Phase2 offers
  `Format::date()` = `NumFormat::Builtin(14)` (`mm-dd-yy`), which Excel localises to the
  user's short-date format. It falls back to `Code("yyyy-mm-dd")` when the file redefines
  id 14 (§5.3). `set_datetime` without a format uses it.
- **Reading.**
  - `CellRef::is_date()` is true when the cell's number format is a date or time format. The
    rule is calamine's (`src/formats.rs`):
    - built-in ids 14–22, 27–36, 45, 47 and 50–58 are dates (46, `[h]:mm:ss`, is a duration);
    - a custom code is classified by `detect_custom_number_format`.
    - An id that the file redefines in `numFmts` is classified by its code, not by the built-in
      table.
  - `CellValue::Number` is unchanged, and `CellRef::datetime()` converts using the workbook's
    epoch.
  - In the 1900 system, serial 60 (Excel's nonexistent 1900-02-29) converts to
    `Err(NonexistentDate)`. The number stays readable through `value()`.
  - `t="d"` cells (ISO strings) keep reading as `CellValue::Date` (Phase0).

### 7. Interaction with Phase1 (ADR-0003)

1. **Plan → validate → apply.**
   - Registry appends are part of the plan of the write that needs them.
   - A write that refuses (any Phase0/Phase1 guard, `UnresolvedSibling`, `NoStylesPart`, a
     redefined built-in id, …) leaves both registries and their parts unchanged.
   - Validation includes the dry-run insertion check (Phase0's `check_insertable`) for every
     appended entry and for an inserted `numFmts`.
   - Apply uses ADR-0003 §3.1's staged clones for `styles.xml` and `sharedStrings.xml` as well.
2. **Value writes** (`set_string` via sst, `set_datetime`, `set_bool`, `set_error`, writes with
   a format) are value writes in ADR-0003 §3.2. Their dependents lose their cached values
   exactly as for `set_number`. The shared-string index is not a value; what matters is the
   cell's text.
3. **Style-only writes** (`set_format`, `set_style_index`, `set_named_style`, the preserving
   setters) do not change a value. They may still change a formula result through functions
   that read formatting:
   - `CELL` with `"format"`, `"color"`, `"parentheses"`, `"prefix"`, `"protect"` or no info
     type (the rule covers `CELL` with any argument);
   - XLM `GET.CELL` and relatives, typically inside a defined name in an `.xlsm`;
   - `INFO` is kept on the list conservatively.

   Rule (review M5, following ADR-0003 §2's "not proven safe ⇒ depends on everything"):
   - If **any** cell formula **or any `definedName`**, on any sheet part the census scans
     (macro sheets included; review n1), contains one of `CELL`, `INFO`, `GET.CELL` or
     `GET.FORMULA`, **or any function that is not on ADR-0003's precise allowlist** (UDFs,
     `_xll.` add-in functions, unknown functions; review n7), a style-only write is treated
     exactly like a value write to that cell. The check is a token scan of the census texts.
     Phase1's dependency analysis then follows names as it already does (`precedents.rs`,
     `name_def`).
   - Excel itself does not recalculate UDFs or `CELL` on a pure formatting change. This rule
     is stricter than Excel, by ADR-0003's "not proven safe ⇒ depends on everything".
   - A consequence: when a macro sheet's formulas are `Everything`, a style-only write is
     refused with ADR-0003's `Err(MacroSheetDependent)`, exactly as a value write would be.
   - Otherwise a style-only write seeds no invalidation.
   - The check is one pass over texts the census already holds.
4. **`calcChain.xml`** is unaffected: Phase2 never creates or removes formula cells.
5. **`fullCalcOnLoad`** is set only when Phase1 invalidates something.

### 8. Vendor extensions in `styles.xml`

- Untouched:
  - `mc:Ignorable` and its namespace declarations;
  - `x14ac:knownFonts`;
  - `styleSheet/extLst` (x14 slicer styles, x15 timeline styles, …);
  - `colors/indexedColors`, `mruColors`;
  - `tableStyles`;
  - `cellStyleXfs`, `cellStyles`.
- New entries carry no extension markup.
- A table with an `mc:AlternateContent` child cannot grow (§2). Other tables in the same part
  can, but a new `Format` always needs a `cellXfs` entry, so it is refused when `cellXfs` has
  one.

### 9. API sketch

```rust
let f = Format::new().set_bold().set_num_format("0.00%").set_background_color(Color::Rgb(0xFFFF00));
ws.cell_mut(1, 1)?.set_number_with_format(0.25, &f)?;
ws.cell_mut(1, 2)?.set_string("x")?;                     // t="s" if the workbook has an sst
ws.cell_mut(1, 3)?.set_datetime(ExcelDateTime::from_ymd(2026, 11, 16)?)?;
ws.cell_mut(1, 4)?.set_format(&f)?;                      // style only
ws.cell_mut(1, 5)?.set_style_index(ws.cell(0, 0)?.unwrap().style_index()?)?;
ws.cell_mut(1, 6)?.set_named_style("Good")?;
ws.cell_mut(1, 7)?.set_rich_string(&[(&bold_font, "a"), (&Font::default(), "b")])?;
match ws.cell(0, 0)?.unwrap().format()? { FormatRead::Expressible(f) => …, FormatRead::NotExpressible { .. } => … }
```

## Test oracles

Oracle names as in the roadmap §6. Every write test runs on fixtures from both seed modes.

1. **L1** for every part other than the edited sheet and an *active* registry part. A registry
   that was never activated leaves its part L1. An active one may change only by T2-append,
   which can mean only its `count` changed, when every entry was reused (review m6).
2. **T2-append (new; byte locality for registry parts).** The diff between the original and
   output registry part is confined to:
   - (a) one insertion region per grown table, at that table's original close tag (or, for a
     self-closing table, the table's own span), containing only the new entries in order;
   - (b) the `count` attribute value of each grown table, and `sst/@count`/`@uniqueCount`;
   - (c) for an inserted `numFmts`, the insertion point before the first child of
     `styleSheet`.

   Every other byte is identical and in the same relative place, by the same algorithm as
   Phase0's `assert_t2`. In particular every existing entry's span is byte-identical, which
   *is* the append-only invariant.
3. **Index stability.**
   - Every `s` attribute and every `t="s"` cell index in every sheet part of the input
     (worksheets, macro sheets, dialog sheets) still refers
     to an entry whose bytes are unchanged. This is checked by resolving each index in input
     and output and comparing the entry XML.
   - A reference that was out of range in the input is still out of range in the output
     (review m2). An append that would end that, by giving it a meaning, must have been
     refused.
   - (Prototype check (a).)
4. **CR (values and styles) with calamine 0.38.**
   - `worksheet_range`: every edited cell reads the new value, and dates read as
     `Data::DateTime` with the right serial under both epochs.
   - `worksheet_style`: every edited cell's `Style` equals the **projection** of the `Format`
     onto what calamine exposes (review m5):
     - font bold/italic/underline/strike/size/name/colour (ARGB after calamine's theme and tint
       resolution);
     - fill pattern and colours;
     - borders;
     - number format code;
     - horizontal/vertical alignment, wrap, indent, rotation, shrink;
     - protection.
   - Not covered by calamine, so covered by T2-append's byte check of the canonical form and by
     RX instead: `vertAlign`, `scheme`, `charset`, `condense`/`extend`, `readingOrder`,
     `quotePrefix`, and theme vs RGB.
   - Colours are not compared in files with a custom `indexedColors` palette (13 corpus files).
     calamine resolves `indexed` against the default palette (`style_parser.rs`).
   - Inherited groups must read like the default style's (prototype: 329/329).
   - Every other cell's `Style` is unchanged.
5. **From scratch vs rust_xlsxwriter (RX).** A format matrix is written twice: by ooxml-xlsx
   from the template, and by rust_xlsxwriter 0.102 (dev-dependency, like calamine).
   - Both outputs are compared through calamine (`worksheet_range` + `worksheet_style` equal).
   - `styles.xml` is also compared after normalising attribute order (rust_xlsxwriter's own
     `compare_xlsx_files` approach). Differences are allowed only where the template
     legitimately differs (the template's theme and `x14ac` declarations), listed in the test.
6. **Edit-mode growth bound.** On `61605.xlsx`, `52348.xlsx` (redefined ids 0–2, no
   `cellStyleXfs`: `xfId` omitted), `ExcelPivotTableSample.xlsx`, `47889.xlsx` (empty
   `styleSheet`: reserved entries first) and `fixture_minimal` (one fill: `gray125` first):
   - one new `Format` adds exactly one `xf`, and at most one each of `numFmt`, `font`, `fill`,
     `border`, plus the reserved entries of §5.4 where a table was empty;
   - writing the same `Format` again adds nothing;
   - **save, reopen, and write the same `Format` again: nothing is added** (exact-form reuse
     across sessions, review M2);
   - a second, different `Format` sharing the font adds no font.

   On `style-alternate-content.xlsx` (review M4):
   - every new `Format` is `Err(UnresolvedSibling)`, because `cellXfs` has AC entries;
   - `set_style_index(n)` succeeds for `n < 29` and fails for `n ≥ 29`;
   - `set_string` still appends to its sst.
7. **Dedup collision regressions** (from the spike). On synthetic copies of fixtures with two
   existing `xf`s that differ only in `xfId` or `apply*` flags, a new `Format` equal to their
   projection must create a *new* entry, never reuse either. A new plain string equal to an
   existing rich or phonetic `si` must append; one equal to an existing exact-form plain `si`
   must reuse it.
8. **Refusals leave everything unchanged.** After each `Err` in §5 and §4, `save` is L1 for
   every entry, as in Phase0.
8a. **`numFmtId` collisions** (review M3).
    - `ConditionalFormattingSamples.xlsx`: a new custom format gets id 178, not 174.
    - `LIBRE_OFFICE-128382-0.xlsx`: never 165.
    - In both, every pre-existing `numFmtId` reference resolves to the same code before and
      after (pivot references stay undefined).
9. **Mutation testing**, as in Phase0 review rounds 1–2. Every mutant must be killed or argued
   equivalent. Mutants include:
   - dedup against existing entries by value;
   - exact-form reuse switched off (killed by the reopen oracle in 6);
   - reuse comparing `xf` bytes before resolving ids;
   - built-in id without the redefinition check;
   - custom id starting at 164 regardless of existing ids;
   - id maximum over `numFmts` only (killed by 8a);
   - `count` written as "stored value + this session's delta" instead of the census number;
   - `count` from worksheets only (killed by `64420.xlsm`);
   - existing custom `numFmt` not reused (killed by the reopen oracle);
   - `fills[1]` `lightGray` reused (killed by a `64450.xlsx` regression);
   - `uniqueCount` not updated;
   - rich vs plain key collision;
   - tint compared lexically;
   - reserved-entry padding removed (killed by `47889`, `fixture_minimal`);
   - `xfId="0"` written without `cellStyleXfs`;
   - an unset group resolved to Calibri 11 instead of inherited;
   - the AC index-stability check removed;
   - the style-only invalidation rule ignoring `definedName`s.
10. **XL (owner).** Outputs of 5 and 6, plus:
    - (a) a 1904 workbook with dates;
    - (b) a file with `numFmts` inserted;
    - (c) a `knownFonts="1"` workbook after adding a font and a row (does Excel complain or
      recompute descent?);
    - (d) shared strings appended to a file whose `count` was wrong;
    - (e) a from-scratch file whose template carries an empty
      `<sst count="0" uniqueCount="0"/>` before any string is written (Q2);
    - (f) the inherited-font `xf` in an Arial 10 workbook (61605 or 62272): looks like its
      neighbours. Also a `with_default_font_of(…).set_bold()` cell there: Arial 10 bold;
    - (g) files with 300 fills, 300 borders, 600 fonts and 260 number formats: does Excel open
      them, and which limit does it report (§5.10)?

## Alternatives considered

- **Dedup new formats against existing entries by value (`Format` projection).** This is
  rejected:
  - In 75/340 packages it would silently pick one of two distinct existing entries. In 115
    groups the candidates differ in which named style the cell belongs to.
  - It also requires parsing every existing entry into a `Format`, which 15% of entries cannot
    be.
- **Rebuild `styles.xml` and `sharedStrings.xml` on save (openpyxl).** This is rejected by
  Layer 3: it renumbers ids, needs a remap of every `s` in every sheet, and drops extensions.
- **Keep inline strings forever (openpyxl's writer).** Simple, and it never touches the sst.
  It is rejected as the default because rust_xlsxwriter parity and file size favour shared
  strings, and Excel stores strings in the sst itself. It is kept as `set_string_inline`, and
  as the fallback when there is no sst part.
- **Never reuse existing entries (revision 1 for styles).** This is rejected (review M2): every
  session would append a new chain, and a daily batch would reach Excel's 256-fill/border
  limit in under a year. Exact-form reuse (§2) prevents that without matching anything Excel
  wrote.
- **Fill unset groups with rust_xlsxwriter's defaults (revision 1).** This is rejected (review
  M1): 121 of 329 prototype packages have another default font. Copying the default style's
  ids is lossless and needs no parsing.
- **Allocate `numFmtId`s above `numFmts` only (revision 1).** This is rejected (review M3):
  dxf and pivot ids share the id space (Context fact 10).
- **Tint as rust_xlsxwriter's enumerated shades only.** This is rejected: it cannot express
  8 of the 21 tints in the corpus. `Theme(color, shade)` survives as a constructor.
- **Adjust `sst/@count` by delta.** This is rejected in favour of recompute (§4.5): delta
  preserves an already-wrong value (7% of files). Recompute is free given Phase1's census.
- **Garbage-collect unreferenced entries.** This is rejected by Layer 3 (never compact).

## Consequences

- Excel-written entries are never matched by meaning. A new `Format` equivalent to an existing
  Excel-written entry still appends once, because the bytes differ.
  - Excel tolerates such duplicates: 41 corpus files already contain exact duplicates.
  - Entries ooxml-rs appended are reused across sessions, so growth is bounded by the number
    of *distinct* formats ever written.
  - Excel's limits are guarded (§5.10).
- `styles.xml` and `sharedStrings.xml` become Layer 2 parts once a registry is active. The
  allowlist grows by:
  - the `numFmts`/`fonts`/`fills`/`borders`/`cellXfs`/`dxfs` tables and their entry types;
  - `rPr`/`r` under `si`.

  All are write entries for creating new nodes. The prototype's hand-written specs
  (`proto/src/specs.rs`) list the minimal set: `styleSheet`, `numFmts`/`numFmt`, `fonts`/`font`
  with `b`/`sz`/`color`/`name`, `fills`/`fill`/`patternFill`/`fgColor`,
  `borders`/`border`/`left`…`diagonal`, `cellXfs`/`xf`. Existing nodes are never edited. They
  are only counted, compared byte-wise for exact-form reuse (§2), and byte-copied by the
  preserving setters (§5.6).
- `count`/`uniqueCount` maintenance depends on Phase1's census. If Phase2 lands first, the
  census must include `t="s"` counting (a few lines in the same scan).
- The preserving setters (§5.6) derive from opaque existing `xf`s by copying bytes. Their
  T2-append regions therefore contain bytes that come from an existing entry, which is still
  append-only.
- The writer needs the Phase1 census for `t="s"` counts, cell-`s` range checks (§2 dangling
  references) and the formatting-function scan (§7.3). If Phase2 lands first, those additions
  go into the census.

## Open questions for the project owner

Each question carries the reviewer's recommendation (round 1) as the **proposed default**,
pending the owner's confirmation.

- **Q1 (exact-form reuse).** May a new entry reuse an existing one that is byte-identical to
  what would be appended?
  - *Proposed default: yes, for strings and for styles.* For styles the whole bottom-up chain
    must match (§2). Reuse is off in tables that are not index-stable.
- **Q2 (template sst).** Should `Workbook::new()`'s template carry an empty
  `sharedStrings.xml`?
  - *Proposed default: yes.* It is **not** rust_xlsxwriter's behaviour: 0.102.0 writes an sst
    only when the workbook has at least one string (`workbook.rs`
    `has_sst_table = … count > 0`, `packager.rs`). An empty template sst is our choice, made
    so that from-scratch strings are shared without needing Phase3's part addition.
  - Oracle 5 (RX) lists the part-list difference (an empty `sharedStrings.xml`) as an allowed
    difference for string-free matrices.
  - Alternatively, once Phase3 lands, an empty sst can be dropped on save.
  - XL item 10(e) checks that Excel opens an empty sst cleanly. If not, the template gets no
    sst until Phase3 can create the part on the first string.
- **Q3 (named style + overrides).** *Proposed default: defer to Phase4.* The `apply*="0"`
  rule for plain `set_named_style` (§5.6) is in Phase2.
- **Q4 (preserving edits).** *Proposed default: in Phase2*, as `set_num_format` /
  `set_font` / `set_fill` / `set_border` deriving from the cell's current `xf` by byte copy
  (§5.6).
- **Q5 (`knownFonts`).** *Proposed default: leave the attribute untouched*, also when rows are
  added, until XL item 10(c) says otherwise.
  - If a fix is needed, copying `x14ac:dyDescent` from a neighbouring row is preferred over
    removing `knownFonts`, which could change rendering in other tools.
  - This also affects Phase0.
- **Q6 (phonetic runs).** *Proposed default: accepted for Phase2*, documented: new strings
  carry no reading, so furigana-based sorting in Excel treats them as unset.

## Sources

- ADR-0001 (Layer 3, "two seed modes", From-scratch path), ADR-0002 §4.2–§4.3, ADR-0003 §3.
- ECMA-376 Part 1, 5th ed.:
  - §18.4 (shared strings: `sst`, `si`, `r`, `rPh`);
  - §18.8 (styles: `numFmts`, `fonts`, `fills`, `borders`, `cellStyleXfs`, `cellXfs`,
    `cellStyles`, `dxfs`, `colors`);
  - §18.8.30 (built-in number formats);
  - §18.8.19 (`color`, `tint`).
  - Part 3 (MCE).
- [MS-XLSB] 2.4.695 `BrtKnownFonts`:
  <https://learn.microsoft.com/en-us/openspecs/office_file_formats/ms-xlsb/346d5d60-0dd3-4fdb-bcdc-20fd7837ebd6>.
  [MS-XLSX] global attributes (§2.5.3 lists `knownFonts`, `dyDescent`):
  <https://msopenspecs.microsoft.com/files/MS-XLSX/%5bMS-XLSX%5d-161114.docx>.
  `x14ac:knownFonts` on `fonts`: Open XML SDK `Fonts.KnownFonts`,
  <https://learn.microsoft.com/dotnet/api/documentformat.openxml.spreadsheet.fonts>.
- rust_xlsxwriter 0.102.0 (`jmcnamara/rust_xlsxwriter@d850973d`):
  - `src/workbook.rs` (`format_xf_index`, `prepare_fonts`, `prepare_fills`, `prepare_borders`,
    `prepare_num_formats`);
  - `src/format.rs` (`Format`, `Font`, `set_num_format`, `set_num_format_index`);
  - `src/color.rs` (`Color::Theme`, `attributes`);
  - `src/styles.rs`;
  - `src/shared_strings_table.rs`, `src/shared_strings.rs`;
  - `src/worksheet.rs` (`get_rich_string`, `use_inline_strings`);
  - `src/datetime.rs`.
- openpyxl 3.1.5 (PyPI sdist):
  - `openpyxl/styles/stylesheet.py` (`apply_stylesheet`, `_normalise_numbers`,
    `write_stylesheet`);
  - `openpyxl/styles/numbers.py` (`BUILTIN_FORMATS`, `BUILTIN_FORMATS_MAX_SIZE`);
  - `openpyxl/utils/indexed_list.py`;
  - `openpyxl/cell/_writer.py` (inline strings);
  - `openpyxl/utils/datetime.py` (`to_excel`, `from_excel`).
- calamine 0.38.0:
  - `src/style.rs`;
  - `src/xlsx/style_parser.rs` (theme index swap, `apply_tint`);
  - `src/xlsx/mod.rs` (`worksheet_style`, `has_1904_epoch`, `read_shared_strings`);
  - `src/formats.rs` (`builtin_format_by_code`, `detect_custom_number_format`).
- Apache POI `test-data/spreadsheet` @ `47dcff769bb395034dddfd59c4609659b6dbc6a4` (Apache-2.0),
  and the committed fixtures in `reference/fixtures/xlsx/`. Measured with
  `spikes/xlsx-phase2/registry_survey.py` and the prototype `spikes/xlsx-phase2/proto/`
  (branch `spike/xlsx-phase2-proto`).
- Microsoft, "Excel specifications and limits" (unique cell formats 65,490; fill styles 256;
  line weight and styles 256; 512 fonts per workbook; 200–250 number formats):
  <https://support.microsoft.com/en-us/office/excel-specifications-and-limits-1672b34d-7043-467e-8e27-269d656771c3>.
- LibreOffice `sc/source/filter/oox/stylesbuffer.cxx` (`Xf::importXf`, quoting MS-OE376: "only
  the cell xf record defines the formatting applied to a cell"):
  <https://github.com/LibreOffice/core/blob/master/sc/source/filter/oox/stylesbuffer.cxx>
  (cited from review round 1).
