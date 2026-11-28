# PR #172 review round 1 (Opus via herdr-cocla)

- base: main cd24426, head: 4a7ff1f
- verdict: YES (m1/m2/n1/n2 only, fixed alongside)
- job: 01a2200c-16d0-7de3-8bb9-bbdcb9f0820b

# PR #172 (U6 defined names) review round 1 @ 4a7ff1f

## Conclusion
converged: YES. No Blocker or Major findings. There are two Minor findings, both inaccurate `NameNotARange` reasons; the lookup still fails closed in every case.

## Verification
- Base: origin/main cd24426. Diff: 8 files, +602/−79.
- Exported the PR tree with git archive (probe directory deleted afterwards). xlsx lib: 340 passed. tests/defined_names: 9 passed.
- Probe: 15 classifications, listed under the findings below.

## Per-scope check
1. **Error variants: OK.** `NoSuchName`, `AmbiguousName` and `NameNotARange` have `kind()` arms (no wildcard), `cell()` arms returning None, and entries in samples; `error_kinds_cover_the_new_variants` pins them. Since #171 is merged, the identifier-boundary check also applies to them.
2. **SheetRef split: clean.** Only the enum is added in `reference.rs`; `worksheet_ref` is not. The private `sheet_position` resolves names through `sheet_index`, the same exact-first-then-unique-fold rule as `worksheet_by_name`, and accepts any sheet kind, which is right for scopes. When U2 adds `worksheet_ref`, it needs its own `NotAWorksheet` check and should not reuse `sheet_position`; note this in the U2 task spec.
3. **Resolution: matches M7.**
   - Rule 1: candidates are the names that `fold` equal to the request.
   - Rule 2: if any candidate has `Invalid` scope, the lookup fails before scope selection, so it fails closed for both `Some` and `None`.
   - Rule 3: `Some` takes the local candidates, falling back to the global ones; `None` takes global only.
   - Rule 4: one candidate resolves, two are ambiguous, none is `NoSuchName`.

   An unparsable `localSheetId` is already refused when the file is opened, so `Invalid` in practice holds an out-of-range id; the test pins this.
4. **Tuple extraction replaced: behaviour preserved.** At all 7 call sites, `RawName::local_id()` (trim, then parse; garbage reads as None) matches the old `defined_name_scope`, and `global_*` and `print_references` use the same filters. For wasm `defined_names`, a scope that only differs by whitespace or leading zeros now comes out canonical (" 1" → "1"), but the SheetJS adapter applies `Number(scope)`, so its output is unchanged. Invalid scopes and `""` are kept raw as before.
5. **Oracle 9: covered.** Same-text global and local names, fold-alike duplicates, invalid `localSheetId`, built-in, hidden, comment, every reason, chartsheet, and agreement with the setters.

## Minor
**m1: the common dangling-name forms do not get the `"#REF!"` reason.** `defined_names.rs` `classify_name` only maps `Area::RefError` (`Sheet1!#REF!`) and an unknown sheet to `"#REF!"`.

| Name text | Probe result |
|---|---|
| `#REF!` | `NameNotARange{reason:"constant"}` (parsed as `Expr::Lit`) |
| `#REF!$A$1` | `"formula"` (fails to parse) |

These are the forms Excel itself writes when it breaks a name after deleting a range or sheet. ADR §7 has the `"#REF!"` reason precisely for dangling names, so this misleads callers such as RPA error messages. Fix: map an `#REF!` error literal, and a text that starts with `#REF!`, to `"#REF!"`, and add both forms to the X cases of `every_not_a_range_reason`.

**m2: whole columns or rows written absolutely are reported as `"relative reference"`.** `Area::Cols { .. } | Area::Rows { .. } => return Err(not_a_range("relative reference"))`, so `Sheet1!$A:$A` and `Sheet1!$1:$1` both get `"relative reference"` (probe). They contain no relative component, and named ranges over whole columns are common, so the error message points the user at the wrong cause.

Two options:
- (a) Following ADR §7's "one sheet-qualified absolute area", return a `Range` when every component is `$` (A1:A1048576), leaving size to U3's `RangeTooLarge`.
- (b) Keep refusing, but say so in the ADR (a post-acceptance note on the bucket) and change the reason or doc wording, for example by stating in `NameNotARange`'s doc comment that `"relative reference"` includes whole rows and columns.

(a) is preferable; pick one either way.

## nit
- **n1:** An intersection (`Sheet1!$A$1 Sheet1!$A$1:$B$2`) is reported as `"multiple areas"`. The result of an intersection is at most one area, and since it is not evaluated, `"formula"` is the accurate reason. Fine to leave as is if the ADR records it as a bucket beyond its letter.
- **n2:** `Sheet1!$A$1:Sheet1!$B$2` (`Expr::Range`, the same sheet on both sides) and an out-of-grid row (`$A$1048577`) are both `"formula"`. Not a real problem, but adding them as cases in the doc comment or tests would make the buckets explicit.
- Other observations, all fine: `(Sheet1!$A$1)` and text with surrounding spaces resolve; `'Sheet1'!` and `sheet1!` resolve by fold; `[0]Sheet1!` is `"external reference"`.

## Verdict
YES. m1 and m2 can be fixed alongside each other and accepted on CI; for m2, the owner or ADR picks (a) or (b).
