# ooxml-pptx

PowerPoint (PresentationML) for ooxml-rs, Phase 0, 0.1
([ADR-0005](../../docs/adr/0005-pptx-phase0.md)) and Phase 2
([ADR-0012](../../docs/adr/0012-pptx-phase2-slides-and-shapes.md)): open and save any
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
| `ParagraphMut::set_text` | first run keeps its `a:rPr`; `\v` → `a:br` + a run with a copy of that `a:rPr` (links stay on the first run); other runs/breaks removed |
| `TextBodyMut::set_text` | line *i* → paragraph *i* (formats kept); surplus removed; new ones copy the last paragraph's `a:pPr` and its run format (runs and `a:endParaRPr`) as subtrees, without links |
| `add_run`, `add_paragraph` | explicitly unformatted (inherit the list/placeholder style) |
| `add_break`, `remove_run`, `remove_paragraph`, `remove_field` | as named |

Refused with a typed `Error`, before anything changes: edits that would
shift animated or comment-anchored text, remove a field implicitly, touch
an `mc:AlternateContent` branch or a merged cell, write content Phase 0
does not model, need a formatting copy that holds a relationship reference or markup-compatibility content, or
contain characters XML 1.0 cannot hold. Only direct values are read
(placeholder inheritance is Phase 1).

Slides as units (Phase 2). Each operation is one registry transaction:
refused before any change, otherwise only the slide list, the parts it
adds or removes and the registries change.

| Operation | Behaviour |
| --- | --- |
| `Presentation::new()`, `new_with(NewOptions)` | built-in template: 16:9, the 11 standard layouts in PowerPoint's order, a notes master, no slides |
| `layouts()` | every layout of every master in `p:sldMasterIdLst` (`LayoutRef`) |
| `add_slide(&layout, at)` | new `slide{N}.xml` (largest number + 1), placeholders cloned python-pptx's way (`type`/`orient`/`sz`/`idx`; no date, footer, slide number) |
| `move_slide(from, to)` | only `p:sldIdLst` and sections change |
| `delete_slide(i)` | removes the slide, its notes and every part only it uses (charts, media, …); refused when anything else references the slide |
| `duplicate_slide(i, at)` | same relationship `Id`s; layout and media shared, notes and tags copied, new `p14:creationId`; comments not copied; slides with charts, diagrams or OLE refused |
| `next_shape_id(i)` | one more than every id, `spid` and comment moniker of the slide |

Ids are never reused: `p:sldId` stays above every id seen in the session,
part numbers and `rId`s above every one in use or removed. Sections stay
equal to `p:sldIdLst`; `docProps/app.xml` `Slides`/`Notes`/`HiddenSlides`
are kept exact.

## Usage example

`examples/replace_text.rs` is a runnable walk-through on a committed fixture. It:

- lists every slide's shapes and their text;
- replaces a whole text body, one paragraph and one run;
- saves to `OUT_DIR/replace_text.pptx`;
- shows two refusals: paragraphs referenced by animations, and a line break in paragraph text.

```sh
cargo run -p ooxml-pptx --example replace_text -- [OUT_DIR]   # default: <temp dir>/ooxml-rs-examples
```

```rust,no_run
use ooxml_pptx::{Error, Presentation};

let mut pres = Presentation::open("deck.pptx")?;
let id = pres.slide(0)?.placeholders()[0].id()?;
let mut slide = pres.slide_mut(0)?;
let mut shape = slide.shape_mut_by_id(id)?;
let mut body = shape.text_body_mut()?;
match body.paragraph_mut(0)?.set_text("New title") {
    Err(e @ Error::AnimatedParagraphs { .. }) => eprintln!("refused, nothing changed: {e}"),
    other => other?,
}
pres.save("deck-edited.pptx")?;
# Ok::<(), Error>(())
```

Development:

- `cargo test -p ooxml-pptx` — L1/T1/T1b (`tests/roundtrip.rs`), T2
  locality (`tests/locality.rs`), T3 expected model (`tests/oracle.rs`),
  T4 refusals (`tests/refusals.rs`) over `reference/fixtures/pptx/`
  (third-party files, read by path, never packaged).
- Independent read-back (CI job `pptx-readback`):
  `cargo run -p ooxml-pptx --example oracle_cases -- OUT` then
  `python3 -I crates/ooxml-pptx/tests/oracle/check.py OUT reference/fixtures/pptx`
  with python-pptx from `tests/oracle/requirements.txt`
  (`--site DIR` if it is installed into a directory).
- Phase 2: `tests/structure.rs` (locality, refusals, sweep of every
  fixture slide) and `tests/new_deck.rs`; the independent structure oracle
  `cargo run --release -p ooxml-pptx --example structure_cases -- OUT` then
  `python3 -I crates/ooxml-pptx/tests/oracle/structure.py [--site DIR] OUT reference/fixtures/pptx`;
  a corpus sweep `cargo run --release -p ooxml-pptx --example structure_sweep -- [--out DIR] DIR...`.
- The template: hand-written parts in `template/parts/`, built into
  `template/template.pptx` by `cargo xtask pptx-template` (`--check` in CI
  compares part names, order and content).
- Manual check in PowerPoint:
  `cargo run -p ooxml-pptx --example pptx_manual_check -- OUT` writes edited
  files and `OUT/CHECKLIST.md`. `cargo xtask manual-check <outdir>` runs the
  Excel, PowerPoint and Word checks together into one bundle with a zip.
- `cargo run -p ooxml-pptx --example dump -- deck.pptx` prints the text model.
