# Synthetic xlsx workbooks (ADR-0003 oracle O1)

Small workbooks written by [`build.py`](build.py) (Python standard library only: raw XML and
`zipfile`, fixed timestamps, so the output is byte-for-byte reproducible). They are original
work of this project (same licence as the repository). Tests never run Python; the generated
files are committed.

Regenerate: `python3 -I reference/fixtures/xlsx/synthetic/build.py`.

Each workbook has **hand-written** expected dependent sets, written from Excel's semantics and
ADR-0003 §2–§3, not from the implementation. `crates/ooxml-xlsx/tests/phase1_o1.rs` checks
`Workbook::dependents` against them and then checks the edited output: every dependent lost
`v`/`t`, every other cell kept its bytes, calamine reads no stale value.

## `o1_precise.xlsx` — sheets `Sheet1`, `Sheet2`, `Sheet3`, `Defines`

Names: `Rate` (global) = `Sheet1!$A$3`; `Rate` (local to `Sheet2`) = `Sheet2!$A$1`; `NR`
(local to `Defines`) = `Sheet1!$A$2`; `NR` (global) = `Sheet2!$A$1`.

| Cell | Formula | Construct |
|---|---|---|
| Sheet1!B1 | `A1*3` | same sheet |
| Sheet1!C1 | `7*2` | never a dependent |
| Sheet1!D1 | `Sheet2!A1+1` | cross sheet |
| Sheet1!E1 | `SUM(A2:A3)` | range |
| Sheet2!B1 | `Sheet1!A1*2` | ADR-0001 R2-M3's case |
| Sheet2!C1 | `B1+1` | transitive |
| Sheet2!D1 | `Rate*2` | local name shadows global |
| Sheet3!A1 | `Sheet2!C1*2` | three-step chain across sheets |
| Sheet3!B1 | `'sheet2'!A1` | lower-case, quoted sheet name |
| Sheet3!C1 | `SUM(Sheet1:Sheet2!A1)` | 3-D |
| Sheet3!D1 | `SUM(Sheet2:Sheet1!A2)` | reversed 3-D |
| Sheet3!E1 | `SUM(Sheet1!A:A)` | whole column |
| Sheet3!F1 | `SUM(Sheet1!3:3)` | whole row |
| Sheet3!G1 | `Rate` | global name (no local on Sheet3) |
| Sheet3!H1 | `Defines!NR` | sheet-qualified local name (POI `56737.xlsx` shape) |
| Sheet3!I1 | `NR` | global name with a same-named local elsewhere |
| Sheet3!J1 | `SUMIF(Sheet1!A1:A3,">0",Sheet1!B1)` | `sum_range` resized to `B1:B3` |
| Sheet3!K1 | `LEN("A1")+ROUND(PI(),0)` | a reference inside a string is not a reference |

| Edit | Expected dependents |
|---|---|
| Sheet1!A1 | Sheet1!B1, Sheet2!B1, Sheet2!C1, Sheet3!A1, Sheet3!C1, Sheet3!E1, Sheet3!J1 |
| Sheet1!A2 | Sheet1!E1, Sheet3!D1, Sheet3!E1, Sheet3!H1, Sheet3!J1 |
| Sheet1!A3 | Sheet1!E1, Sheet3!E1, Sheet3!F1, Sheet3!G1, Sheet3!J1 |
| Sheet2!A1 | Sheet1!D1, Sheet2!D1, Sheet3!B1, Sheet3!C1, Sheet3!I1 |
| Sheet1!B2 | Sheet3!J1 |
| Sheet1!B3 | Sheet3!F1, Sheet3!J1 |
| Defines!A1, Sheet3!Z9 | none |

## `o1_everything_<tag>.xlsx` — one construct that depends on everything

`Sheet1`: `A1:A3` = 1, 2, 3; `B1` = the construct; `C1` = `5*2` (never a dependent);
`D1` = `B1+1`. Editing `Sheet1!Z100`, which no formula names, must give exactly `B1` and `D1`.

| Tag | `B1` | Rule (ADR-0003) |
|---|---|---|
| `indirect` | `INDIRECT("A1")` | not on the table |
| `offset` | `OFFSET(A1,0,0)` | not on the table |
| `lookup` | `LOOKUP(2,A1:A3,E1)` | reads past the result vector (R2-M1 2) |
| `sumif_computed` | `SUMIF(A1:A3,">0",INDEX(E1:E3,1))` | computed `sum_range` (R2-M1 3) |
| `range_computed` | `SUM(A1:INDEX(E1:E3,2))` | `:` with a computed side (§2.3 b) |
| `udf` | `myFunc(A1)` | UDF (§2.3 c) |
| `external` | `[1]Sheet1!A1` | external reference |
| `unknown_sheet` | `Nope!A1` | unknown sheet (§2.4) |
| `structured` | `SUM(Table1[Col])` | structured reference |
| `relative_name` | `Rel` = `Sheet1!A1` | relative name (§2.3 f) |
| `name_collision` | `TAX2024` = name | name/reference collision (§2.4) |
| `volatile_ca` | `A1+0` with `ca="1"` | volatile flag |
| `spill_error` | `_xlfn.SEQUENCE(3)` cached `#SPILL!` | blocked spill (§2.3 e) |
| `data_table` | `f t="dataTable"` | data table (§2.3 d) |
| `bx_name` | `Total+0`, and `F1` has `bx="1"` | assigning formula (§2.4) |

## `o1_arrays.xlsx`

`A1:A3` = 1, 2, 3; `A9` = 4. `B1` = `{A1:A3*2}` (CSE, `ref="B1:B3"`); `C1` = `B2+1`;
`D1` = `A1:A3+0`, a dynamic array (`cm="1"`, XLDAPR, `ref="D1:D3"`); `E1` = `D5*2` (below the
current spill: in the potential spill area, R2-M1 1); `F1` = `A9*2`; `G1` = `1+1`.

| Edit | Expected dependents |
|---|---|
| A1 | B1, B2, B3, C1, D1, D2, D3, E1 |
| A9 | F1 |
| B2, B3, D3 | `Err(ArrayResultCell)` |
| B1, D1 | `Err(ArrayAnchor)` |

## `o1_shared.xlsx`, `o1_shared_oob.xlsx`

`Sheet2`: `B2:B5` shared group `A2*2` (master `B2`); `C2:E3` shared group `$A2+D$1` whose master
`D2` is **not** the top-left cell (`51710.xlsx` shape); `H9` = `SUM(B2:B5)`.

| Edit | Expected dependents |
|---|---|
| Sheet2!A3 | B3, C3, D3, E3, H9 |
| Sheet2!D1 | D2, D3 |

Master hand-off (`phase1_formulas.rs`): overwriting `B2` makes `B3` the master with `A3*2` and
`ref="B3:B5"`; overwriting `D2` makes `C2` the master with `$A2+C$1` and `ref="C2:E3"`.

`o1_shared_oob.xlsx`: sheet `Q1 2024`; `Sheet2!G2:G3` shared group `'Q1 2024'!A1+G2`, master
`G3`. The member `G2` translates to row 0, so its precedents are `Everything` (edit
`'Q1 2024'!A2` → G2, G3), and overwriting the master `G3` is `Err(SharedFormulaMaster)`.

## `o1_name_scope.xlsx` (review R3-M1 a)

`S1!C1` = `S2!Loc`, where `Loc` is local to `S2` with the unqualified text `$A$1`. Excel reads it
on the using sheet (`S1!A1`); LibreOffice relative to the calculating sheet. ooxml-xlsx does not
guess: `C1` depends on everything. Edit `S1!Z9` → `S1!C1`; `D1` (`2*3`) is never a dependent.
The other round-3 shapes (two masters sharing an `si`, sheet and defined names that fold alike,
the Kelvin sign) are built in `tests/phase1_review_r3.rs`.

## Calculation modes and refusals

| File | Shape | Expected |
|---|---|---|
| `o1_manual.xlsx` | `calcMode="manual"`, `B1` = `A1*2` | edit A1: B1 loses `v`; `fullCalcOnLoad` not added |
| `o1_no_calcpr.xlsx` | no `calcPr`, no calcChain | edit A1: `<calcPr fullCalcOnLoad="1"/>` inserted |
| `o1_iterate.xlsx` | `iterate="1"`, `A1` = `A1+B1` | edit B1: `Err(IterativeCalculation)`; edit C9: fine |
| `o1_macro.xlsx` | XLM macro sheet `Macro1!A1` = `Sheet1!A1+1` | edit Sheet1!A1: `Err(MacroSheetDependent)` |
| `o1_unknown_cm.xlsx` | `cm` pointing at a non-XLDAPR type | every edit: `Err(UnknownCellMetadata)` |
