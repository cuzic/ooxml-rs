# PowerPoint Phase 1 spike

Throwaway evidence for [ADR-0008](../../docs/adr/0008-pptx-phase1-deck-reading.md)
and the Phase 1 section of [`docs/roadmap/pptx.md`](../../docs/roadmap/pptx.md).
Not production code.

All scripts read untrusted `.pptx` files with `zipfile` + `xml.etree` (no
external entities, nothing executed) and must be run with `python3 -I`
from a directory that does **not** contain the files. They import
`common.py` by adding their own directory to `sys.path` explicitly. Run on
2026-11-16 with Python 3.15; python-pptx 1.0.2 installed with
`python3 -m pip install --target <scratch dir> python-pptx==1.0.2` (a
library under test, not part of the build).

## Corpora (downloaded to scratch, not committed)

| Corpus | Files | Licence | Pin |
|---|---:|---|---|
| roadmap corpus (`spikes/pptx/README.md`): Apache POI `test-data/slideshow` (83 readable of 94), Open XML SDK `TestFiles` (8), python-pptx `features/steps/test_files` (11), LibreOffice (3) | 116 (105 readable) | Apache-2.0 / MIT / MIT / MPL-2.0 | as in `reference/fixtures/pptx/README.md` |
| LibreOffice `sd/qa/unit/data/pptx`, the other 417 files | 417 | MPL-2.0 | `LibreOffice/core` `2e885d0bda9b` |
| ShapeCrawler `ShapeCrawler.Tests/Assets/**/*.pptx` | 100 | MIT | `ShapeCrawler/ShapeCrawler` `2ab474918ac8` |
| **Total** | **633** (619 readable presentations) | | |

"105" results are on the roadmap corpus only (comparable with the
roadmap's numbers), "633" on all three.

## Scripts and results

| Script | Usage | Result (committed in `results/`) |
|---|---|---|
| `chain.py` | `chain.py <dirs/files…> [--geometry out.json]` | a Python model of ADR-0008 §1: placeholder matching (and where POI / python-pptx pick differently), the part that supplies geometry, `bodyPr@anchor`, run `sz`/`b`/latin/fill, paragraph `algn`/`marL`/bullet; how often the contested steps decide a value (Q6 `otherStyle` vs `defaultTextStyle`, split by `p:cNvSpPr@txBox` (rev 2), placeholders' own `p:style/a:fontRef` (rev 2), dt/ftr/sldNum category, `a:pPr/a:defRPr`, `p:style/a:fontRef`, `a:defPPr`, level fall-back); theme fonts and colours through `clrMap`/`clrMapOvr`; notes runs. `chain-105.txt`, `chain-633.txt` |
| `ppy_geometry.py` | `ppy_geometry.py <site-dir> <geometry.json> <dirs/files…>` | resolved placeholder geometry vs python-pptx's `left/top/width/height`, keyed by (file, `sldId`, shape id). `ppy_geometry-105.txt`, `ppy_geometry-633.txt` |
| `features.py` | `features.py <dirs/files…>` | notes slides, hyperlinks/actions, sections, legacy and modern comments, transitions (direct / `mc:AlternateContent`, `Requires`, kinds, attributes), timing (node types, preset classes, builds, targets). `features-105.txt`, `features-633.txt` |
| `ppy_links.py` | `ppy_links.py <site-dir> <dirs/files…>` | shape click actions, run hyperlinks and notes text vs python-pptx 1.0.2. `ppy_links-633.txt` |
| `findreplace.py` | `findreplace.py <dirs/files…>` | cross-run find/replace: match classes over 1–3-word probes, and a reference model of ADR-0008 §4 checked against two invariants (text = `str.replace`; per-character format); rev 2 adds replacements containing `\v` and the `LinkSplitByBreak` refusal. `findreplace-105.txt`, `findreplace-633.txt` |
| `allowlist_p1.py` + `allowlist_phase1.toml` | `allowlist_p1.py <SDK data dir> <ECMA XSD dir> codegen/allowlist allowlist_phase1.toml` | the proposed 124 read entries exist, 97 descent edges are `ChildOf` pairs, percentage-union attributes / default mismatches on allowlisted entries. `allowlist_p1.txt` |
| (trial codegen) | copy of the tree with `allowlist_phase1.toml` merged into `codegen/allowlist/{pml,dml}.toml`, then `cargo xtask codegen`, `cargo check -p ooxml-schema --no-default-features --features pml` / `dml` / `pml,dml`, `cargo clippy … -D warnings`, `cargo test -p ooxml-pptx` | report diff `codegen-report.diff` (rerun for rev 2): 124 new `Read` lines, **no** new diagnostic (57, unchanged); all builds and tests pass |
| `../pptx/percent.py` (Phase 0 script, rerun) | `percent.py <ECMA XSD dir> <SDK data dir> <files…>` | 0 genuine `"NN%"` values in 633 files (the 2 hits are `p:tag@val`, a string attribute that the scan matches by name). `percent-633.txt` |

Revision 2 (after the round-1 review of ADR-0008) reran `chain.py`,
`findreplace.py`, `allowlist_p1.py` and the trial codegen; the review's own
probes are in its scratch directory and are cited in the ADR as "review
probe N".
