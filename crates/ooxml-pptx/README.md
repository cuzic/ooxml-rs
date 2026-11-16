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

Phase 1 reads ([ADR-0008](../../docs/adr/0008-pptx-phase1-deck-reading.md)
§1, §3): `Presentation::deck()` parses every slide's layout, master and
theme once and returns a `Deck`; `DeckSlide` resolves run, paragraph and
body properties, geometry and outline through the inheritance chain, each
value a `Lookup` (`Set` with its `Source`, `Unset`, or `Unresolved` with a
`Reason`), and reads links/actions, comments (legacy and p188),
transitions (MCE branch by namespace URI) and a timing summary;
`Deck::sections()` reads p14 sections. Values are as specified, not as
displayed (§1.6); enumerated values are `EnumValue`s of the generated
types; `slide_rect`, `ShapeRef::is_text_box`, `RunProps::xsd_default` and
the Q4 helper `RunProps::script_font_candidate` complete §1. Notes, layout
and master contexts (§1.7) are `Sheet`s (`DeckSlide::notes_sheet`,
`Deck::layouts`, `Deck::masters`); existing notes slides are read with
`Presentation::notes` and edited through `Presentation::notes_mut` with the
Phase 0 text API (§2); find/replace reaches notes with
`Scope::SlidesAndNotes` / `Scope::Notes`. Creating notes slides is not part of
Phase 1.

Development:

- `cargo test -p ooxml-pptx` — L1/T1/T1b (`tests/roundtrip.rs`), T2
  locality (`tests/locality.rs`), T3 expected model (`tests/oracle.rs`),
  T4 refusals (`tests/refusals.rs`), find/replace invariants and rules
  (`tests/replace.rs`), the ADR-0008 resolver (`tests/resolve.rs`) and read
  APIs (`tests/deckread.rs`) over `reference/fixtures/pptx/`; with
  `OOXML_PPTX_CORPUS=dir:dir … -- --ignored`, `replace.rs` sweeps a local
  corpus and `resolve.rs` compares 76 source counters with the spike model
  (third-party files, read by path, never packaged).
- Independent read-back (CI job `pptx-readback`):
  `cargo run -p ooxml-pptx --example oracle_cases -- OUT` then
  `python3 -I crates/ooxml-pptx/tests/oracle/check.py OUT reference/fixtures/pptx`
  with python-pptx from `tests/oracle/requirements.txt`
  (`--site DIR` if it is installed into a directory). The cases include
  deck-wide `replace_all` calls (ADR-0008 §7 O5): `check.py` finds the
  matches itself, decides the paragraph-local refusals (rules 1, 5, 6)
  itself and compares them with ooxml-pptx's, kind included, and redoes the §4 edits on python-pptx's lxml tree. ADR-0008 O1/O3:
  `cargo run -p ooxml-pptx --example resolve_dump -- OUT.json` then
  `python3 -I crates/ooxml-pptx/tests/oracle/resolve_check.py OUT.json reference/fixtures/pptx`
  (known geometry differences in `tests/oracle/geometry_known_differences.json`;
  the POI table for O2b is `tests/oracle/poi_known_differences.md`).
- Manual check in PowerPoint:
  `cargo run -p ooxml-pptx --example pptx_manual_check -- OUT` writes edited
  files and `OUT/CHECKLIST.md`. `cargo xtask manual-check <outdir>` runs the
  Excel, PowerPoint and Word checks together into one bundle with a zip.
- `cargo run -p ooxml-pptx --example dump -- deck.pptx` prints the text model.
