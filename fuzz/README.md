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
| `xlsx_edit` | `[fixture][part][ops_len][ops][payload]` | a fixture from `reference/fixtures/xlsx` with one part replaced by the payload; `Workbook` open, reads, `set_number`/`set_string`: refusals change nothing, successes read back before and after save/reopen, other parts L1-identical |
| `pptx_edit` | same layout, `reference/fixtures/pptx` | `Presentation` open, read every slide, text edits (body/paragraph/run/table cell): refusals change nothing, `TextBodyMut::set_text` reads back, other parts L1-identical, output reopens |

Running:

```sh
rustup toolchain install nightly && cargo +nightly install cargo-fuzz
cargo run -p ooxml-robustness --example seed_corpus -- fuzz/corpus   # seeds from reference/fixtures (not committed)
cd fuzz
cargo +nightly fuzz run xml_t1b corpus/xml_t1b -- -max_total_time=600
# triage on stable (one line per input: panic location + message):
cargo run -p ooxml-robustness --example replay -- xml_t1b fuzz/artifacts/xml_t1b/*
```

Known findings are skipped by the filters in
[`crates/ooxml-robustness/src/known.rs`](../crates/ooxml-robustness/src/known.rs)
(`OOXML_FUZZ_ALL=1` turns them off). Findings, reproducers and regression tests:
[`docs/testing/fuzz-findings-2026-11.md`](../docs/testing/fuzz-findings-2026-11.md).

The format targets read fixtures from `reference/fixtures` (override with
`OOXML_FIXTURES=<dir>`). Note that cargo-fuzz builds with debug assertions, so
`debug_assert!`s in the crates are checked too.
