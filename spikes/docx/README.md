# Word spike (evidence for `docs/roadmap/docx.md` and ADR-0004)

Throwaway scripts. Nothing here is production code. All `.docx` inputs are untrusted data:
the Python scripts only read them with `zipfile`/`ElementTree` under `python3 -I`, and skip
any part that has a DTD. Run the scripts from a directory other than the inputs' directory.

Corpora used (not committed; the 25 chosen files are in `reference/fixtures/docx/`):

- Apache POI `test-data/document/*.docx`: 129 files, trunk `47dcff76` (2026-11-13).
- .NET Open XML SDK test assets `*.docx|*.docm|*.dotx`: 502 files at `431ab05c`. "SDK
  (non-Strict)" below excludes the 58 files under `O14ISOStrict/`, which leaves 444.

| Script | What it measures | Used in |
|---|---|---|
| `survey.py <dir> [--json out]` | Per-file feature inventory: parts, fields by code, tracked-change elements, SDT kinds and parents, `mc:AlternateContent` parents, range markers before `w:tblPr`, header/footer reference types, ID duplicates. Used to choose the fixtures. | fixtures README, roadmap §2 |
| `design_facts.py <dir>...` | The design questions: runs split with identical vs. different `w:rPr` and what separates them (Q1); field shapes such as nesting, spanning paragraphs and split instructions (Q2); text inside revisions (Q3); ID collisions and the commentEx↔paraId link (Q4); `ST_OnOff` lexical forms (Q5); `sectPr` placement (Q6). | roadmap §3, ADR-0004 |
| `phase0_gate.py <dir or file>... [--list]` | Applies ADR-0004's open-time gates and per-run checks; reports how many files and text runs Phase0 can edit. | ADR-0004 §6, Consequences |
| `readback/` (Rust, standalone workspace) + `readback_compare.py` | Runs candidate independent readers (`docx-rs` 0.4.22 with `default-features = false`, `docx-rust` 0.1.11) on files, then compares their extracted text with an ElementTree ground truth, broken down by wrapper (`ins`, `hyperlink`, `sdtContent`, …). | ADR-0004 §7 (oracle T5) |

```sh
python3 -I spikes/docx/phase0_gate.py reference/fixtures/docx --list
cd spikes/docx/readback && cargo build --release && \
  ./target/release/docx-readback-spike /tmp/rb ../../../reference/fixtures/docx/*.docx && \
  python3 -I ../readback_compare.py /tmp/rb ../../../reference/fixtures/docx/*.docx
```

Results as of 2026-11-16 (rustc 1.99.1, Python 3.15) are quoted in the roadmap and ADR-0004.
`readback/` is not a member of the root workspace (it declares its own `[workspace]`), so its
dependencies do not enter ooxml-rs's lockfile.

### Revision 2 (2026-11-16, ADR-0004 rev 2)

`phase0_gate.py` now follows the round-1 review: fields are tracked with a stack and every nesting
level is classified (M1), the field state runs over the whole body in document order (m2), every
reachable run is counted, hyperlink runs included (m3), and the `DocumentProtected` gate is
applied (M3). Results on the same corpora:

| Corpus | Files writable | Refused (files) | Settable / text runs in writable files |
|---|---|---|---|
| fixtures (26) | 20 | 1 each: Encrypted, DTD, Strict, TrackRevisionsOn, DocumentProtected; 2 StaleFieldRisk | 3,418 / 3,455 (308 in `w:hyperlink`) |
| POI (129) | 102 | 13 NotZip, 1 Corrupt, 2 Encrypted, 1 DTD, 5 TrackRevisionsOn, 4 DocumentProtected, 1 StaleFieldRisk | 27,763 / 28,067 (98.9%) |
| SDK non-Strict (444) | 404 | 30 StaleFieldRisk, 6 TrackRevisionsOn, 2 Encrypted, 2 Strict | 23,448 / 25,556 (91.8%) |

The file-level SDK numbers do not change. In `complex2010.docx` the stack now also reports the
outer `TC` (its instruction holds a nested `SEQ`). Runs inside text boxes and `mc:AlternateContent`
are no longer counted: they are not body flow. `crates/ooxml-docx/tests/fixtures.rs` asserts the
same per-fixture counts from the Rust implementation.
