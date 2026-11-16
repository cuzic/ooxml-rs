# ADR-0006: xlsx Phase2 — shared-string and style registries

- Status: **Proposed** (revision 1)
- Date: 2026-11-16
- Depends on: [ADR-0001](0001-lossless-round-trip-architecture.md) (Layer 3, "two seed
  modes"), [ADR-0002](0002-schema-driven-code-generation.md) (lens, allowlist, §4.3 insertion),
  [ADR-0003](0003-xlsx-phase1-formula-and-calc-state.md) (plan → validate → apply, dependency
  invalidation)
- Roadmap: [`docs/roadmap/xlsx.md`](../roadmap/xlsx.md) §2.3 (B1–B5), §2.5 (D1–D7), §6 Phase2
- Evidence: [`spikes/xlsx-phase2/`](../../spikes/xlsx-phase2/README.md)

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
   - One Hancom file wraps a `font` *entry* in `mc:AlternateContent` inside `fonts`, so font
     indices there depend on MCE processing.
8. `fills[0]` = `none` / `fills[1]` = `gray125` hold in 96% of packages. The rest are POI- or
   unknown-produced files (two have no `fills` at all).
9. 95 of 342 packages (28%) have **no `sharedStrings.xml`**. 20 use the 1904 date system (19
   written by Excel).

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
  raw XML, and nothing else. The only exception is the *exact-form reuse* rule for shared
  strings (§4.3), which compares bytes, not meaning.
- **Two seed modes.**
  - **From scratch** (`Workbook::new()`): the template's entries are seeded *as values*. The
    template is ours, and its `styles.xml` is built from `Format::default()`. Any new `Format`
    equal to one of them reuses its index, which is full rust_xlsxwriter-style dedup.
  - **Edit** (opened file): the existing entries are seeded as opaque and never matched. New
    entries are deduplicated among themselves by value.
  - The mode is fixed at open and is not a user option.
- **Index lookups are per table.**
  - A new `Format` resolves to a font index, fill index, border index and `numFmtId`, each
    deduplicated in its own table, then to an `xf` built from those ids, deduplicated in
    `cellXfs`.
  - A new `xf` may reference a new font *or an existing one*. "Existing" here means only the
    from-scratch seeds. In edit mode a new `xf` always references new or built-in parts, never
    an opaque existing font.
- **Counting entries.**
  - The index of a new entry is the number of *entries* already in the table. An
    `mc:AlternateContent` child counts as one entry, because MCE selects exactly one branch.
  - Appending into a table that has an `mc:AlternateContent` child is refused with
    `Err(UnresolvedSibling)`, the same rule as ADR-0002 §4.3 and the Phase0 dry run. The index
    arithmetic would depend on a reader's MCE support. This affects one corpus file, and only
    its `fonts`.

### 3. Value types

```rust
pub struct Format {            // Hash + Eq; builder methods as rust_xlsxwriter
    pub num_format: NumFormat, // General | Builtin(u16) | Code(String)
    pub font: Font,            // name, size, bold, italic, underline, strike, vert_align,
                               // color, family, charset, scheme, condense, extend
    pub fill: Fill,            // None | Pattern { pattern, fg: Color, bg: Color }
    pub border: Border,        // left/right/top/bottom/diagonal: (style, Color); diag up/down
    pub alignment: Alignment,  // horizontal, vertical, rotation, wrap, indent, shrink,
                               // reading_order
    pub protection: Protection,// locked (default true), hidden
    pub quote_prefix: bool,
}
pub enum Color { Default, Auto, Rgb(u32 /*ARGB*/), Indexed(u8), Theme { index: u8, tint: Tint } }
pub struct Tint(f64);          // in [-1, 1]; Eq/Hash on the bit pattern of the normalised value
```

- **Model.** The model is rust_xlsxwriter's feature set, with three deliberate differences:
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
2. **Keys.**
   - A plain string is keyed by its text: the caller's string, before `_xHHHH_` escaping.
   - A rich string is keyed by its run list `[(Font-subset, text)]`. Its XML is `si/r/rPr` +
     `t`, where `rPr` (`CT_RPrElt`) uses `rFont` for the font name, as Excel writes it.
   - Plain and rich never collide, even if the flattened text is equal.
3. **Exact-form reuse in edit mode.**
   - A new *plain* string may reuse an existing `si` only if that `si` is byte-for-byte the
     canonical form the registry would write for it: a single `t` child, `xml:space="preserve"`
     iff the text has outer whitespace, the same `_xHHHH_` escaping, and no `r`/`rPh`/
     `phoneticPr`/attributes.
   - This is a byte comparison against our own serialisation, not a semantic match. It cannot
     merge two entries that differ in anything a reader could see.
   - Rich and phonetic existing items are never reused.
   - It deviates from ADR-0001's literal "dedup only newly-added entries". The justification:
     reusing an entry that is exactly what we would have appended changes no meaning, saves
     table growth on repeated values, and never renumbers anything.
   - Open question Q1 asks the owner to confirm. The fallback is to switch it off, a one-line
     policy.
   - Building the index costs one pass over the existing `si`. Phase0 already decodes every
     `si` for reading.
4. **Whitespace and escapes.** The Phase0 rules apply:
   - `xml:space="preserve"` iff outer whitespace;
   - `_xHHHH_` for characters XML cannot carry;
   - `_x005F_` protection;
   - at most 32,767 UTF-16 units.
5. **`uniqueCount` and `count`.**
   - `uniqueCount` is set to the number of `si` entries after the append, counting an
     `mc:AlternateContent` child as one.
   - `count` is recomputed as the number of `t="s"` cells in all worksheets. This can be done
     because Phase1's census already scans every worksheet once at open. The census counts
     `t="s"` cells as well, and the registry adjusts the number by each write's delta (+1 for a
     new `t="s"` cell, −1 for a `t="s"` cell overwritten with something else).
   - Recomputing rather than adding a delta to the stored value corrects the 7% of files whose
     `count` is already wrong, instead of carrying the error forward.
   - An absent attribute stays absent (8 and 7 corpus files): adding it is not needed and would
     touch a byte nobody asked for.
6. **No `sharedStrings.xml`.** 28% of the corpus has none.
   - `set_string` keeps writing `inlineStr` (Phase0 behaviour).
   - `set_rich_string` writes an inline rich string (`is/r`, valid per `CT_Rst`).
   - `Workbook::shared_strings_available()` tells the caller which happened.
   - When Phase3 can add parts, an opt-in `create_shared_strings()` creates the part.
   - The from-scratch template ships an empty `sharedStrings.xml` (`count="0"
     uniqueCount="0"`), so from-scratch files always use shared strings.
   - Open question Q2: should the template do this, or stay inline as rust_xlsxwriter's
     `constant_memory` mode does?
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
3. **Number formats.**
   - `NumFormat::Builtin(id)` is written as that id, *unless the file redefines that id* in
     `numFmts`. Then it is `Err(BuiltinNumFormatRedefined { id })` (32 corpus files redefine
     some id below 164).
   - `NumFormat::Code(s)` uses the built-in id when `s` is exactly a locale-independent
     built-in code (ECMA-376 §18.8.30 table, as openpyxl's `BUILTIN_FORMATS`) *and the file
     does not redefine that id*. Otherwise it gets a new custom id:
     `max(163, largest numFmtId in the file, largest id handed out) + 1`.
   - Custom codes are deduplicated among new entries only. An existing custom `numFmt` with the
     same code is not reused (16 files define built-in codes as customs, 3 have duplicate
     codes). This is the edit-mode rule again.
   - If `numFmts` is absent (234/340), it is inserted as the first child of `styleSheet`
     through the ADR-0002 insertion primitive. An `mc:AlternateContent` sibling there makes the
     write `Err(UnresolvedSibling)`.
4. **Fills 0 and 1.**
   - When a new fill would get index 0 or 1, because the file has fewer than two fills (2
     corpus files have none), the missing mandatory `none` / `gray125` entries are appended
     first. This keeps rust_xlsxwriter's invariant without touching existing entries.
   - Existing non-canonical entries at 0/1 (13 files written by POI, ONLYOFFICE or an unnamed
     producer) are left alone.
   - A `Format` whose fill is `None` maps to index 0 only in from-scratch mode. In edit mode,
     index 0 is used only when it is literally `<fill><patternFill patternType="none"/></fill>`
     (exact-form check); otherwise a new `none` fill is appended.
5. **New `xf` attributes.**
   - `xfId="0"`: the first `cellStyleXfs` entry, which is `Normal` in every corpus file that has
     `cellStyles` (`builtinId="0"`). A file whose `xfId=0` is not `Normal` is still valid; the
     cell is then attached to that style.
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
     - It then appends one `xf` that copies the style's `cellStyleXfs` entry ids (`numFmtId`,
       `fontId`, `fillId`, `borderId`, alignment, protection) with `xfId` = that entry's index
       and no `apply*` flags: the cell shows the style unchanged, as Excel does when a style is
       applied.
     - These `xf` entries are deduplicated per style name among new entries.
     - Named style + overrides (`Format` on top of a style) is open question Q3.
   - **Number format only:** `CellMut::set_num_format(code)` is a convenience for
     `Format::default().set_num_format(code)`. In edit mode it does **not** inherit the cell's
     current font or fill, because the existing `xf` is opaque. The docs say so, and
     `set_format_preserving` (copy the existing `xf` with one id changed) is open question Q4.
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

### 6. Dates and times

- **Writing.** `set_datetime(ExcelDateTime)` writes a serial number. The conversion is
  rust_xlsxwriter's (`src/datetime.rs`):
  - days since 1899-12-31, plus 1 for dates after 1900-02-28 (Excel's false 1900-02-29, serial
    60);
  - 1904 epoch when `workbookPr/@date1904` is `1`/`true` (20 corpus files);
  - the time as a fraction of a day.
  - Dates before the epoch, and serials ≥ 2958466 (after 9999-12-31), are
    `Err(DateOutOfRange)`. openpyxl's `to_excel` uses the same 60-day adjustment
    (`utils/datetime.py`).
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
3. **Style-only writes** (`set_format`, `set_style_index`, `set_named_style`) do not change a
   value, so they seed no invalidation. The exception is any formula that reads formatting:
   `CELL(…)` with `"format"`, `"color"`, `"parentheses"`, `"prefix"`, or with no info type.
   - Phase1's tokenizer marks `CELL` as not on its precise allowlist.
   - Phase2 adds one rule: a style-only write seeds invalidation of every formula whose text
     calls `CELL` or `INFO`, as if the cell's value had changed.
   - This is conservative and costs nothing in workbooks without `CELL`.
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
  can.

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

1. **L1** for every part other than the edited sheet, `styles.xml` and `sharedStrings.xml`.
   A registry part that did not grow (e.g. a value write whose `Format` dedups to an entry
   added earlier in the session) is L1 as well.
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
3. **Index stability.** Every `s` attribute and every `t="s"` cell index in every sheet of the
   input still refers to an entry whose bytes are unchanged. This is checked by resolving each
   index in input and output and comparing the entry XML.
4. **CR (values and styles) with calamine 0.38.**
   - `worksheet_range`: every edited cell reads the new value, and dates read as
     `Data::DateTime` with the right serial under both epochs.
   - `worksheet_style`: every edited cell's `Style` (font bold/italic/size/name/colour as ARGB
     after calamine's theme and tint resolution, fill pattern and colour, borders, number
     format code, alignment) equals the `Format`.
   - Every other cell's `Style` is unchanged.
5. **From scratch vs rust_xlsxwriter (RX).** A format matrix is written twice: by ooxml-xlsx
   from the template, and by rust_xlsxwriter 0.102 (dev-dependency, like calamine).
   - Both outputs are compared through calamine (`worksheet_range` + `worksheet_style` equal).
   - `styles.xml` is also compared after normalising attribute order (rust_xlsxwriter's own
     `compare_xlsx_files` approach). Differences are allowed only where the template
     legitimately differs (the template's theme and `x14ac` declarations), listed in the test.
6. **Edit-mode growth bound.** On `61605.xlsx`, `style-alternate-content.xlsx` (its `fonts`
   cannot grow: expect `Err(UnresolvedSibling)` for a new font and success for a fill-only
   format), `52348.xlsx` (redefined ids 0–2), `ExcelPivotTableSample.xlsx` and
   `fixture_minimal`:
   - one new `Format` adds exactly one `xf`, and at most one each of `numFmt`, `font`, `fill`,
     `border`;
   - writing the same `Format` again adds nothing;
   - a second, different `Format` sharing the font adds no font.
7. **Dedup collision regressions** (from the spike). On synthetic copies of fixtures with two
   existing `xf`s that differ only in `xfId` or `apply*` flags, a new `Format` equal to their
   projection must create a *new* entry, never reuse either. A new plain string equal to an
   existing rich or phonetic `si` must append; one equal to an existing exact-form plain `si`
   must reuse it.
8. **Refusals leave everything unchanged.** After each `Err` in §5 and §4, `save` is L1 for
   every entry, as in Phase0.
9. **Mutation testing**, as in Phase0 review rounds 1–2. Mutants include: dedup against
   existing entries; built-in id without the redefinition check; custom id starting at 164
   regardless of existing ids; `count` delta instead of recompute; `uniqueCount` not updated;
   rich vs plain key collision; tint compared lexically; fills 0/1 padding removed; `Builtin`
   numFmt mapping on a redefined id. Every mutant must be killed or argued equivalent.
10. **XL (owner).** Outputs of 5 and 6, plus:
    - (a) a 1904 workbook with dates;
    - (b) a file with `numFmts` inserted;
    - (c) a `knownFonts="1"` workbook after adding a font and a row (does Excel complain or
      recompute descent?);
    - (d) shared strings appended to a file whose `count` was wrong.

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
- **Reuse existing entries by exact form for styles too.** This is possible with the same
  byte-comparison rule as §4.3. Not adopted: Excel's `apply*` patterns rarely match a
  canonical form, so the benefit is small, and it would require canonicalising every existing
  font/fill/border. It stays available as a later optimisation (Q1 covers both).
- **Tint as rust_xlsxwriter's enumerated shades only.** This is rejected: it cannot express
  8 of the 21 tints in the corpus. `Theme(color, shade)` survives as a constructor.
- **Adjust `sst/@count` by delta.** This is rejected in favour of recompute (§4.5): delta
  preserves an already-wrong value (7% of files). Recompute is free given Phase1's census.
- **Garbage-collect unreferenced entries.** This is rejected by Layer 3 (never compact).

## Consequences

- Existing entries are never matched, so in edit mode the registries can grow by one entry per
  distinct new `Format`/string per session, even when an equivalent entry already exists.
  Excel tolerates this (41 corpus files already contain exact duplicates) and compacts on its
  own save.
- `styles.xml` and `sharedStrings.xml` become Layer 2 parts once a registry is active. The
  allowlist grows by:
  - the `numFmts`/`fonts`/`fills`/`borders`/`cellXfs`/`dxfs` tables and their entry types;
  - `rPr`/`r` under `si`.

  All are write entries for creating new nodes. Existing nodes are never edited, only counted
  and, for §4.3, compared.
- `count`/`uniqueCount` maintenance depends on Phase1's census. If Phase2 lands first, the
  census must include `t="s"` counting (a few lines in the same scan).
- Callers that want "same look as before, different number format" have no lossless one-liner
  in edit mode until Q4 is decided.

## Open questions for the project owner

- **Q1 (exact-form reuse).** In edit mode, may a new plain string reuse an existing `si` that
  is byte-identical to what would be appended (§4.3)? Default proposed: yes for shared strings,
  no for styles.
- **Q2 (template sst).** Should `Workbook::new()`'s template carry an empty
  `sharedStrings.xml`, so that from-scratch strings are shared (proposed), or stay inline?
- **Q3 (named style + overrides).** Should `set_named_style_with(name, &Format)` exist in
  Phase2 (an `xf` with `xfId` = style, `apply*` = 1 for the overridden groups), or wait for
  Phase4's named-style work?
- **Q4 (preserving edits).** Should `set_num_format`/`set_font_color` on a cell in edit mode
  copy the cell's *existing* `xf` (opaque) and change one id, so the rest of its look is kept?
  This needs a raw `xf` clone with one attribute changed: lossless, but it is the first time a
  registry would derive a new entry from an existing one.
- **Q5 (`knownFonts`).** If the XL check ("Test oracles" 10(c)) shows Excel relies on `x14ac:knownFonts` +
  `dyDescent`, should new rows get `x14ac:dyDescent` copied from a neighbour, or should
  `knownFonts` be removed when rows are added? This also affects Phase0.
- **Q6 (phonetic runs).** Japanese workbooks carry `rPh`/`phoneticPr` (29 corpus sst). New
  strings get none, so furigana-based sorting in Excel treats them as unread. Acceptable for
  Phase2?

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
  `spikes/xlsx-phase2/registry_survey.py`.
