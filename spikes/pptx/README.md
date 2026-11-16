# PowerPoint planning spike

Throwaway evidence for [`docs/roadmap/pptx.md`](../../docs/roadmap/pptx.md) and
[ADR-0005](../../docs/adr/0005-pptx-phase0.md). Not production code.

All scripts read untrusted `.pptx` files with `zipfile` + `xml.etree` and must
be run with `python3 -I` from a directory that does **not** contain the
files. Nothing in the files is executed. Run on 2026-11-16 with Python 3.15.

Corpus used (not committed, 105 readable files): Apache POI
`test-data/slideshow/*.pptx` (83 readable of 94), Open XML SDK `TestFiles`
(8), python-pptx `features/steps/test_files` (11), LibreOffice
`sd/qa/unit/data/pptx` (3). The committed subset is
[`reference/fixtures/pptx/`](../../reference/fixtures/pptx/README.md).

| Script | Usage | Result quoted in the docs |
|---|---|---|
| `survey.py` | `survey.py <dirs/files…> [--json out]` | feature counts (roadmap §1, fixture README) |
| `placeholders.py` | `placeholders.py <dirs/files…>` | 711 slide placeholders: 687 idx match (49 type differs), 2 type-only, 22 orphan; 239 own `a:xfrm`; 801/1,883 runs with `sz` (roadmap §3.1) |
| `percent.py` | `percent.py <ECMA XSD dir> <SDK data dir> [pptx…]` | 66 percentage-union attributes (`total 66`; 61 typed `Int32Value` by the SDK); 0 values in `"NN%"` form (roadmap §3.2; the earlier "81" was a miscount, ADR-0005 review m6) |
| `runs.py` | `runs.py <dirs/files…>` | 597/3,611 multi-run paragraphs, 252 proofing-only, run-crossing probes (roadmap §3.4) |
| `allowlist_check.py` | `allowlist_check.py <SDK data dir> <ECMA XSD dir> <../adr-0002/py/slottable.py> <codegen/allowlist>` (reads `pml.toml`/`dml.toml`, so it cannot drift from the allowlist) | ADR-0005 §5 allowlist: names exist, slot lowering, cross-feature `ChildOf`, the one `serverZoom` mismatch |
| `readback.py` | `readback.py <site-dir with python-pptx 1.0.2> <file.pptx>` | superseded by `crates/ooxml-pptx/tests/oracle/readback.py` (ADR-0005 rev 2 T3: paragraph text with breaks and fields, rPr/pPr attributes, placeholders, part names read before python-pptx renames them); kept as the round-1 evidence |
| `probe.py` | `probe.py <dirs/files…>` | ADR-0005 rev 2 counts: empty paragraphs and their `a:endParaRPr`, first-run `a:rPr`/`a:pPr` with child elements (format copy), `a:t` whitespace, autofit, `p:bldP`/`p:pRg`/`p:charRg`, `ac:txMk`, `mc:AlternateContent` under text elements (output of 2026-11-16 in the ADR) |
| `ppy_add_slide_check.py` | `ppy_add_slide_check.py <site-dir> <in.pptx> <out.pptx>` | python-pptx renames slide parts instead of picking an unused name (roadmap §3.5) |

python-pptx was installed for the last two with
`python3 -m pip install --target <scratch dir> python-pptx==1.0.2`
(it pulls lxml, Pillow, XlsxWriter). It is a library under test here, not
part of the build.
