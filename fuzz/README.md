# ooxml-rs fuzz targets

cargo-fuzz project (nightly, libFuzzer). It is **not** a member of the
repository workspace, which builds on stable 1.99. The invariant checks live in
[`crates/ooxml-robustness`](../crates/ooxml-robustness) so that the regression
tests and the stable smoke test run exactly the same code.

| Target | Input | Checks |
| --- | --- | --- |
| `package_roundtrip` | raw zip bytes | `Package::from_bytes` → read every part → `to_bytes`: Layer 1 (name, method, CRC, compressed bytes) for every entry; output reopens and saves L1-identical; after `force_rerender` of one part, all other entries are L1-identical and the part reparses structurally equal |
| `xml_t1` | XML bytes | accepted ⇒ unedited `serialize()` == input (T1); a lower depth limit never accepts more; `Clone`/`Debug`/`Drop`/reads don't fail |
| `xml_t1b` | XML bytes | `force_rerender` → reparse is `structurally_eq` (T1b), and re-rendering is a fixed point |
| `lens_ops` | op stream (+ optional XML) | edit sequences on a small schema (`ooxml_robustness::lens`): every `Err` leaves the document unchanged (bytes, tree, dirty state); the output reparses with the same depth limit to the same tree |
| `xlsx_edit` | `[fixture][part][ops_len][ops][payload]` | a fixture from `reference/fixtures/xlsx` with one part replaced by the payload; `Workbook` open, reads, `set_number`/`set_string`: refusals change nothing, successes read back before and after save/reopen, other parts L1-identical, except exactly what ADR-0003 Phase1 may write (`phase1_oracle`: the dependents' `v`/`is`/`@t`, `calcPr/@fullCalcOnLoad`, and the calcChain part with its `Relationship`/`Override` when the formula set changed), plus ADR-0006 appends and count updates in the existing shared-string table after a string write |
| `docx_edit` | same layout, `reference/fixtures/docx` (plus `docx::targeted_inputs()`: A2, open field, `xml:space` shapes) | `Document` open, gates, every paragraph/run read, `set_run_text` (mostly on runs `check_settable` accepts) and `append_paragraph`: the outcome (Ok or which error) is predicted from the write gate, `check_settable`, the text's characters, the field depth and A2, and must match; refusals change nothing; an edited run keeps its attributes and `w:rPr` and gets `xml:space="preserve"` where needed; an appended paragraph lands inside `w:body` before a body-level `w:sectPr`, or receives the moved A2 section properties from the previous paragraph; successes read back (`\r\n`/`\r` as `\n`), change `document.xml` only inside the run's original span (T2), by one insertion (append; inside `w:body` if it was self-closing), or by removing the old A2 section and inserting the new paragraph, and the output reopens after every edit; other parts L1-identical |
| `docx_replace` | same layout, `reference/fixtures/docx` and synthetic main/header stories covering split runs, proofing, mixed formatting, fields, hyperlinks, bookmarks, tabs and breaks | ADR-0007 Phase1: `find` ranges against exact paragraph text matches; `replace`, `replace_at`, `replace_matches`; refusal atomicity, skipped ranges, counts, text readback, generations and stale matches, retained run properties, byte locality outside each written paragraph, and Layer 1 for other parts |
| `xlsx_formula_edit` | same layout, `reference/fixtures/xlsx/**` (incl. the Phase1 `synthetic/` workbooks) plus `xlsx_formula::formula_books()` (shared groups, array formulas, cross-sheet chains, calcChain, manual mode) | ooxml-xlsx Phase1 (ADR-0003): `set_number` / `set_string` / `set_formula` / `remove_formula` on any worksheet, mostly on formula cells and cells formulas read. An independent dependency model (plain A1 references, ranges, whole rows/columns, sheet prefixes, shared members translated from their master, array results; conservatively models a subset of the dependencies the library reports) must be contained in `Workbook::dependents`, and after an edit no model dependent keeps `<v>`; `fullCalcOnLoad` is set when there were dependents or the formula set changed (unless manual); `calcChain.xml` (by relationship) is removed exactly when the formula set changed, with its override; values read back; non-target cells preserve formula references, shared master ranges still cover each previously covered group, and cells outside the dependency set preserve their stored values; outside the edited sheet the Phase1 diff oracle allows only dependent cache changes, calculation metadata changes and shared-string appends for string writes and count updates when shared-string references change; all other ZIP entries stay byte-identical; the formula-cell census after each edit and after reopen equals the model's expectation; `Err` changes nothing, and after `Poisoned` everything is `Poisoned` |
| `pptx_edit` | same layout, `reference/fixtures/pptx` | `Presentation` open, read every slide, text edits (body/paragraph/run/table cell): refusals change nothing, `TextBodyMut::set_text` reads back, other parts L1-identical, output reopens |

Running (the toolchain and cargo-fuzz versions the findings were produced
with, also pinned in `.github/workflows/fuzz-smoke.yml`; `fuzz/Cargo.lock` is
committed because this directory is its own workspace):

```sh
rustup toolchain install nightly-2026-11-14 && cargo +nightly-2026-11-14 install cargo-fuzz --version 0.13.2 --locked
cargo run -p ooxml-robustness --example seed_corpus -- fuzz/corpus   # seeds from reference/fixtures (not committed)
# optional 2nd argument: keep at most N seeds per target (the CI smoke uses 64)
cd fuzz
cargo +nightly fuzz run xml_t1b corpus/xml_t1b -- -max_total_time=600
# triage on stable (one line per input: panic location + message):
cargo run -p ooxml-robustness --example replay -- xml_t1b fuzz/artifacts/xml_t1b/*
```

Every finding so far is fixed (PR #40, PR #14), so no inputs are filtered;
its reproducers run as ordinary regression tests. Findings, reproducers and
regression tests:
[`docs/testing/fuzz-findings-2026-11.md`](../docs/testing/fuzz-findings-2026-11.md).

`-max_total_time` counts from the end of libFuzzer's first pass over the
initial corpus: with the full seed set, `pptx_edit` spends minutes on that pass
alone, so thin the seeds for short runs.

The package seeds include synthetic variants the fixtures lack: every entry
ZIP64, data descriptors, data prepended to the zip, a damaged CFB signature,
and Strict namespaces in the parts only.

The format targets read fixtures from `reference/fixtures` (override with
`OOXML_FIXTURES=<dir>`). Note that cargo-fuzz builds with debug assertions, so
`debug_assert!`s in the crates are checked too.

F-8 (fixed by #67) affected cells whose references named a different row than
their enclosing row. Replacing the misplaced formula could duplicate its cell
reference and retain calcChain. Removing that formula could report success
while leaving the original formula in place. The regression test covers both
row directions. It checks workbook-wide `InvalidReference` and byte-identical
serialization after each refused write.

`docx_replace` takes needles from the input document at character boundaries.
It runs up to six edit steps per input, or one when the stored fixture ZIP
exceeds 512 KiB. Its formatting comparison checks for visible differences
under the default options. The stable tests also exercise exact refusal
reasons and byte identity for mixed formatting, fields, hyperlink edges,
and bookmark boundaries. The `docx_edit` oracle covers A2 moves and refusals
under ADR-0011 §6.
