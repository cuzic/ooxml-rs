# PR #171 review round 1 (Opus via herdr-cocla)

- base: main 213e27a, head: e1acbe3
- verdict: NO (1 Major open)
- job: 01a21f2b-fca0-72cf-901f-bb666a7e2ebe

# PR #171 (U1 error identifiers) review round 1 @ e1acbe3

## Conclusion
converged: NO. There is one Major finding: the caller-input audit is incomplete. `kind()`/`cell()` themselves are correct: the match is exhaustive with no wildcard, every one of the 306 arms equals its variant name (checked by script), every variant appears in samples, and Core delegates correctly.

## Verification
- Base: origin/main 213e27a. Diff: 14 files, 2 commits.
- Exported the PR tree with git archive and ran the tests in it. Error tests in core/docx/pptx: OK. xlsx lib: 339 passed. phase2, phase2_part2, phase3_smoke, phase4 and phase5d: all OK.
- A probe test (deleted afterwards) checked the untested paths and the missed sites.

## Major
**M1: the caller-input audit is incomplete.** The ADR (§8) requires every caller-input `Malformed`/`InvalidValue` to be moved or listed in the PR. Three caller-input sites still return `Malformed` and are not listed in the PR body:
- `crates/ooxml-xlsx/src/sheet_ops/mod.rs:248` `set_defined_names`: an out-of-range sheet index gives `Malformed("defined name sheet is outside the workbook")`.
- `crates/ooxml-xlsx/src/sheet_ops/mod.rs:727` `add_sheet_with_id`: a reserved id gives `Malformed("sheetId {id} is reserved")`.
- `crates/ooxml-xlsx/src/core_props.rs:352` `add_custom_properties`: an entry with an empty or duplicate name, an unknown VT type, or a value that fails to parse gives `Malformed("invalid custom property ..")`.

Probe output: kind=Malformed, msg="malformed workbook: …" for all three. The binding would turn caller mistakes into `MalformedFileError` "malformed workbook", which breaks the §8 guarantee that `Malformed` means the file.

Fix: move these to `InvalidArgument{cell:None}` and pin each with a variant test and an Err⇒byte-identical test, or list them in the PR body as exceptions with the reason. `custom properties already exist` (core_props.rs:325) is a state refusal; either decision is fine, but state which one.

## Minor
**m1: `cell()` returns non-A1 text for `NameInUse`.** `crates/ooxml-xlsx/src/error.rs` `NameInUse { cell, .. } => Some(cell.as_str())`, but the constructors pass non-A1 strings:
- `"table or defined name"` (tables.rs:677, 692);
- `"defined name {name}"` (range.rs:328);
- `"Sheet!A1"` (range.rs:313).

§8 defines `cell()` as "The A1 cell". Python `OoxmlError.cell` would receive text like "table or defined name". Fix: return `None`, or split the field.

**m2: `InvalidArgument`'s Display drops the cell.** The format is `#[error("{message}")]`, whereas the old `InvalidValue` was `"cell {cell}: {message}"`.
- Repro: `set_iso_date("bogus")` used to print "cell B2: expected a UTC ISO date" and now prints "expected a UTC ISO date".
- Impact: the wasm `encode_row` and the Python message (both Display) lose the cell location, which goes against U1's "no other behaviour change".
- Fix: build the message from `cell` when it is `Some`.

**m3: three moved rejections are not pinned by tests.** The PR body says each moved rejection pins the variant, `cell()` and Err⇒byte-identical. These three are not:
- `worksheet.rs:1139` and `:1204`, a reversed range (`CellRange{first:D1,last:B1}`) for `set_shared_formula`/`set_shared_formula_master`;
- `setters.rs:21` `prepare_rich_text` (`set_rich_text(&[])`, an empty run).

The probe shows the behaviour is correct (InvalidArgument, cell None/A1, bytes unchanged); only the pins are missing.

## nit
- **n1:** `kind_matches_debug_name` uses `starts_with` only. In core, `Package`/`PackageTooLarge` and `Xml`/`XmlPartTooLarge`, and in docx, `AmbiguousStyle`/`AmbiguousStylesPart`, are prefixes of each other, so a copy-paste mistake (`PackageTooLarge` → "Package") would not be caught. Also check that the character after the prefix is not an identifier character.
- **n2:** The PR body says 14 sites; there are 17 construction sites.

## Verdict
NO. Fix M1 (move the 3 sites, or list them as exceptions). m1–m3 can be fixed alongside it and accepted on CI.
