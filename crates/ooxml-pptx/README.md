# ooxml-pptx

PowerPoint (PresentationML) for ooxml-rs, Phase 0
([ADR-0005](../../docs/adr/0005-pptx-phase0.md)): open and save any
`.pptx`/`.pptm`/`.ppsx`/`.potx` losslessly (untouched parts keep their
compressed bytes; untouched XML keeps its bytes), walk slides and shapes,
read text, and edit text **inside existing text bodies** — shapes, group
members and table cells.

```rust,no_run
use ooxml_pptx::Presentation;

let mut pres = Presentation::open("deck.pptx")?;
let title = pres.slide(0)?.placeholders()[0].id()?;
let mut slide = pres.slide_mut(0)?;
let mut shape = slide.shape_mut_by_id(title)?;
shape.text_body_mut()?.set_text("New title")?;
pres.save("deck.pptx")?;
# Ok::<(), ooxml_pptx::Error>(())
```

| Edit | Behaviour |
| --- | --- |
| `RunMut::set_text` | replaces `a:t`; `a:rPr` kept |
| `ParagraphMut::set_text` | first run keeps its `a:rPr`; `\v` → `a:br`; other runs/breaks removed |
| `TextBodyMut::set_text` | line *i* → paragraph *i* (formats kept); surplus removed; new ones copy the last paragraph's `a:pPr` attributes and its run format (runs and `a:endParaRPr`) |
| `add_run`, `add_paragraph` | explicitly unformatted (inherit the list/placeholder style) |
| `add_break`, `remove_run`, `remove_paragraph`, `remove_field` | as named |
| `Presentation::find`, `Presentation::replace_all` | cross-run find/replace on the slides ([ADR-0008](../../docs/adr/0008-pptx-phase1-deck-reading.md) §4, under review): the run where a match starts keeps its format, covered runs are removed or trimmed, `\v` in the replacement → `a:br` + a run copying the start run's format; a match over a field, an animated/comment-anchored text, an AC shape or a merged cell is refused; all matches are checked before the first change (`OnRefusal::Skip`/`Abort`) |

Refused with a typed `Error`, before anything changes: edits that would
shift animated or comment-anchored text, remove a field implicitly, touch
an `mc:AlternateContent` branch or a merged cell, write content Phase 0
does not model, need a formatting copy with child elements (not in Phase 0), or
contain characters XML 1.0 cannot hold. Only direct values are read
(placeholder inheritance is Phase 1).

Development:

- `cargo test -p ooxml-pptx` — L1/T1/T1b (`tests/roundtrip.rs`), T2
  locality (`tests/locality.rs`), T3 expected model (`tests/oracle.rs`),
  T4 refusals (`tests/refusals.rs`), find/replace invariants and rules
  (`tests/replace.rs`; `OOXML_PPTX_CORPUS=dir:dir … -- --ignored` sweeps a
  local corpus) over `reference/fixtures/pptx/`
  (third-party files, read by path, never packaged).
- Independent read-back (CI job `pptx-readback`):
  `cargo run -p ooxml-pptx --example oracle_cases -- OUT` then
  `python3 -I crates/ooxml-pptx/tests/oracle/check.py OUT reference/fixtures/pptx`
  with python-pptx from `tests/oracle/requirements.txt`
  (`--site DIR` if it is installed into a directory).
- Manual check in PowerPoint:
  `cargo run -p ooxml-pptx --example pptx_manual_check -- OUT` writes edited
  files and `OUT/CHECKLIST.md`. `cargo xtask manual-check <outdir>` runs the
  Excel, PowerPoint and Word checks together into one bundle with a zip.
- `cargo run -p ooxml-pptx --example dump -- deck.pptx` prints the text model.
