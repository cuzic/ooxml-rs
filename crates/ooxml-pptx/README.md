# ooxml-pptx

PowerPoint (PresentationML) for ooxml-rs, Phase 0 and 0.1
([ADR-0005](../../docs/adr/0005-pptx-phase0.md)): open and save any
`.pptx`/`.pptm`/`.ppsx`/`.potx` losslessly (untouched parts keep their
compressed bytes; untouched XML keeps its bytes), walk slides and shapes,
read text, and edit text **inside existing text bodies** — shapes, group
members and table cells.

```rust,no_run
use ooxml_pptx::Presentation;

let mut pres = Presentation::open("deck.pptx")?;
// The first placeholder of slide 0, if it has one (no panic on a slide without).
if let Some(title) = pres.slide(0)?.placeholders().first().map(|p| p.id()).transpose()? {
    let mut slide = pres.slide_mut(0)?;
    let mut shape = slide.shape_mut_by_id(title)?;
    shape.text_body_mut()?.set_text("New title")?;
    pres.save("deck.pptx")?;
}
# Ok::<(), ooxml_pptx::Error>(())
```

| Edit | Behaviour |
| --- | --- |
| `RunMut::set_text` | replaces `a:t`; `a:rPr` kept |
| `ParagraphMut::set_text` | first run keeps its `a:rPr`; `\v` → `a:br` + a run with a copy of that `a:rPr` (links stay on the first run); other runs/breaks removed |
| `TextBodyMut::set_text` | line *i* → paragraph *i* (formats kept); surplus removed; new ones copy the last paragraph's `a:pPr` and its run format (runs and `a:endParaRPr`) as subtrees, without links |
| `add_run`, `add_paragraph` | explicitly unformatted (inherit the list/placeholder style) |
| `add_break`, `remove_run`, `remove_paragraph`, `remove_field` | as named |
| `Presentation::find`, `Presentation::replace_all` | cross-run find/replace on the slides ([ADR-0008](../../docs/adr/0008-pptx-phase1-deck-reading.md) §4, Accepted): the run where a match starts keeps its format, covered runs are removed or trimmed, `\v` in the replacement → `a:br` + a run copying the start run's format; a match over a field, an animated/comment-anchored text, an AC shape or a merged cell is refused; all matches are checked before the first change (`OnRefusal::Skip`/`Abort`) |

Refused with a typed `Error`, before anything changes: edits that would
shift animated or comment-anchored text, remove a field implicitly, touch
an `mc:AlternateContent` branch or a merged cell, write content Phase 0
does not model, need a formatting copy that holds a relationship reference or markup-compatibility content, or
contain characters XML 1.0 cannot hold. Only direct values are read
(placeholder inheritance is Phase 1).

## Usage example

`examples/replace_text.rs` is a runnable walk-through on a committed fixture. It:

- lists every slide's shapes and their text;
- replaces a whole text body, one paragraph, and one run of a paragraph that has runs;
- saves to `OUT_DIR/replace_text.pptx`;
- shows two refusals: paragraphs referenced by animations, and a line break in paragraph text.

```sh
cargo run -p ooxml-pptx --example replace_text -- [OUT_DIR]   # default: <temp dir>/ooxml-rs-examples
```

```rust,no_run
use ooxml_pptx::{Error, Presentation};

let mut pres = Presentation::open("deck.pptx")?;
let id = pres.slide(0)?.placeholders().first().map(|p| p.id()).transpose()?.ok_or("slide 0 has no placeholder")?;
let mut slide = pres.slide_mut(0)?;
let mut shape = slide.shape_mut_by_id(id)?;
let mut body = shape.text_body_mut()?;
match body.paragraph_mut(0)?.set_text("New title") {
    Err(e @ Error::AnimatedParagraphs { .. }) => eprintln!("refused, nothing changed: {e}"),
    other => other?,
}
pres.save("deck-edited.pptx")?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

Tables (Phase 1b, [ADR-0013](../../docs/adr/0013-pptx-phase1b-tables-and-smartart.md)
§2–§3) through `ShapeMut::table_mut()`:

| Edit | Behaviour |
| --- | --- |
| `insert_row(at)`, `insert_column(at)` | height/width and cell formatting copied from above/left (else below/right); inside a merge the new cells join it; new `a16:rowId`/`colId` when the table has ids (never an id deleted in this session); the frame size follows |
| `delete_row(r)`, `delete_column(c)` | merges through it shrink (a span reaching 1 is removed); refused through a merge origin and for the only row/column |
| `merge(r1, c1, r2, c2)`, `unmerge(r, c)` | edge form (PowerPoint's); the other cells' paragraphs move to the origin; refused over a merged cell |
| `set_border(r, c, edge, line)`, `set_borders(line)` | both cells of a shared edge, the whole outline of a merged range |
| `set_fill`, `set_margin`, `set_anchor`, `set_flag`, `set_style`, `set_column_width`, `set_row_height` | `a:tcPr` / `a:tblPr`; styles defined in `tableStyles.xml` or one of the 74 built-in ids |

Structural edits are refused on a table a modern comment anchors into,
and on a table that is not well-formed; every refusal leaves the slide
unchanged.

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
  (`--site DIR` if it is installed into a directory). The cases include
  deck-wide `replace_all` calls (ADR-0008 §7 O5): `check.py` finds the
  matches itself, checks ooxml-pptx accepted exactly its matches minus the
  refused ones, and redoes the §4 edits on python-pptx's lxml tree.
- Phase 1b tables: `tests/tables.rs`, the merge-rule property test
  `tests/table_model.rs` (`TABLE_MODEL_CASES=N` for more cases), the
  python-pptx oracle `cargo run --release -p ooxml-pptx --example table_cases -- OUT`
  then `python3 -I crates/ooxml-pptx/tests/oracle/tables.py [--site DIR] OUT reference/fixtures/pptx`,
  and a corpus sweep `cargo run --release -p ooxml-pptx --example table_sweep -- [--out DIR] DIR...`.
- Manual check in PowerPoint:
  `cargo run -p ooxml-pptx --example pptx_manual_check -- OUT` writes edited
  files and `OUT/CHECKLIST.md`. `cargo xtask manual-check <outdir>` runs the
  Excel, PowerPoint and Word checks together into one bundle with a zip.
- `cargo run -p ooxml-pptx --example dump -- deck.pptx` prints the text model.
