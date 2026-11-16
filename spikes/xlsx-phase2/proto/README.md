# spikes/xlsx-phase2/proto — ADR-0006 registry prototype

**Throwaway. Not for merge as code.** This is a standalone Cargo workspace (it is not a member
of the root workspace) that path-depends on `crates/ooxml-core` and `crates/ooxml-schema` and
does not modify them. It tests the edit-mode registry design of
[ADR-0006](../../../docs/adr/0006-xlsx-phase2-shared-strings-and-styles.md) on real files.

```sh
cd spikes/xlsx-phase2/proto
cargo build --release
./target/release/xlsx-phase2-proto ../../../reference/fixtures/xlsx ../../../reference/round_trip_corpus
./target/release/xlsx-phase2-proto "$SCRATCH/poi"     # POI test-data, see ../README.md
PROTO_OUT=/some/dir ./target/release/xlsx-phase2-proto FILE   # also writes the outputs
```

## What it does, per package (edit mode)

1. **Shared strings.** It appends one plain `si` to `sharedStrings.xml`, if the package has one.
   `uniqueCount` is set from the entries. `count` is recomputed from the `t="s"` cells of all
   worksheets. An attribute that was absent stays absent.
2. **Styles.** It appends one style chain:
   - a custom `numFmt`, with an id above every `numFmtId="…"` in *any* XML part of the
     package; `numFmts` is inserted if absent;
   - a bold red `font`;
   - a solid yellow `fill`;
   - a thin `border`;
   - an `xf` referencing them.

   Reserved leading entries are appended first when a table is empty: a default font, an
   empty border, `xf` 0, and fills 0/1. `xfId` is omitted when there is no `cellStyleXfs`.
   `count` is updated on grown tables.

   A second `xf` sets only the number format and copies `fontId`/`fillId`/`borderId` from the
   default style (`cellStyleXfs[0]`, else `cellXfs[0]`). This is ADR-0006 M1's "inherit".
3. **Cells.** On the first visible worksheet it writes a new row (the first free row number)
   with three cells:
   - `A` = the new shared string (or `inlineStr` when there is no sst);
   - `B` = 1234.5678 with the new `xf`;
   - `C` = 1234.5678 with the inherit `xf`.
4. **Save and verify:**
   - **(a)** Every pre-existing entry of every table (`numFmts`, `fonts`, `fills`, `borders`,
     `cellStyleXfs`, `cellXfs`, `cellStyles`, `dxfs`, `si`) is byte-identical at the same
     index. Every pre-existing cell of the sheet keeps `s`/`t`/`v`. Out-of-range input
     references are reported.
   - **(b)** T2-append byte locality on `styles.xml` and `sharedStrings.xml`. Only the open
     tags of grown tables (`count`) and the appended entries may differ; an inserted table is
     allowed whole, and a self-closing root is re-rendered whole.
   - **(c)** calamine 0.38 reads A and B (and C), and B's number-format code, bold font and
     yellow fill. C's font must equal the workbook's default font. Every other cell of every
     sheet reads as before (CR).
   - **(d)** `count` and `uniqueCount` agree with the entries and references.
   - **L1 (content).** Every other part is unchanged.
5. **Second session (M2).** It reopens the output, replays the style edit, and checks that the
   new `font`/`fill`/`border` bytes are identical to session 1's, and that `numFmt`/`xf`
   differ only in ids. If so, exact-form reuse would hit.

The style elements are not yet in the generated `sml` allowlist, so `src/specs.rs` hand-writes
the lens specs: `styleSheet`, `numFmts`/`numFmt`, `fonts`/`font`/`b`/`sz`/`color`/`name`,
`fills`/`fill`/`patternFill`/`fgColor`, `borders`/`border`/`left`…`diagonal`, `cellXfs`/`xf`.
That is the minimal allowlist Phase2 needs.

## Results (2026-11-16)

| | Fixtures (26 files) | POI `47dcff76` (366 files) |
|---|---|---|
| OK: (a)–(d), L1, CR, M1 and M2 checks pass | 21 | 330 |
| refused before any change | 3 | 5 |
| skipped: not a readable Transitional package (Strict, encrypted, corrupt zip, DTD, CRC, Excel 2007 beta namespace) | 2 | 31 |
| **failures** | **0** | **0** |
| string appended / style appended | 17 / 21 | 241 / 329 |
| `numFmts` inserted | 14 | 228 |
| M1: inherited `xf` reads the default font (calamine) | 21/21 | 329/329 (default ≠ Calibri 11 in 121) |
| M2: second session identical (ids aside) | 21/21 | 329/329 |

What was refused:

- 4 POI files with rows or cells without `r` (Phase0 `MissingReference`).
- `style-alternate-content.xlsx`: `mc:AlternateContent` entries inside `fonts`; `cellXfs` and
  `dxfs` have them too.

## Counterexamples to the ADR-0006 revision 1 assumptions

All are folded into ADR-0006 revision 2.

1. **The first new entry in an empty table becomes the default.** `47889.xlsx` has
   `<styleSheet … xml:space="preserve"/>` with no tables. Without padding, the new `xf` became
   index 0, the style of every cell without `s`, and the new font became the default font.
   Revision 1 only padded fills 0/1. `fixture_minimal` has a single fill, so `gray125` is
   padded first.
2. **`xfId="0"` dangles without `cellStyleXfs`.** This happens in 2 fixtures (`52348.xlsx`,
   `renamed_workbook_part.xlsx`) and 2 POI files. Omit `xfId`.
3. **`numFmtId` space is package-wide.** Three POI files:
   - `ConditionalFormattingSamples`: dxf ids 174–177 above `numFmts`' 173;
   - `dataValidationTableRange`: dxf 165, no `numFmts`;
   - `LIBRE_OFFICE-128382-0`: an undefined pivot id 165 above 161.

   Allocating from `numFmts` alone would collide, or would give a dangling pivot reference a
   meaning.
4. **Exact-form reuse must be id-aware.** `font`/`fill`/`border` re-serialise byte-identically
   in a second session. `numFmt` and `xf` bytes contain session-allocated ids, so reuse has to
   resolve bottom-up: reuse the parts first, then compare the `xf` built from the reused ids.
5. **Inheriting by index works.** Copying the default style's `fontId`/`fillId`/`borderId`
   gives cells that calamine reads in the workbook's default font in every package, including
   121 whose default is not Calibri 11 (Arial 10 ×44, Verdana 10 ×19, Arial 11 ×14,
   Calibri 12 ×11, 宋体, ＭＳ Ｐゴシック, …). A rust_xlsxwriter-default font would have looked
   different in all of them.
6. **`count` is wrong in the input** in 18 POI files. Recomputing it after the append was exact
   in every case. Pre-existing `t="s"` cells with an empty `<v/>` exist (`64508`, `64667`),
   which a strict range check must tolerate.
7. **No row to append to.** `style-alternate-content.xlsx` uses row 1,048,576. That is a test
   harness concern, not a registry one.

The verification code here (T2-append locality, index-stability comparison, calamine
projection) is the part worth porting into `crates/ooxml-xlsx/tests` when Phase2 is
implemented. The rest is prototype code.
