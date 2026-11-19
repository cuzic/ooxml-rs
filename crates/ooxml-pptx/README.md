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
| `TableMut::set_fill`, `TableMut::set_border` | set a table cell fill or border using `TableColor::Rgb` or `TableColor::Scheme` for solid colours |

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
| `add_text_box`, `add_picture`, `ensure_notes_slide` | not yet (ADR-0012 §4.5–§4.6, a follow-up PR) |

Ids are never reused: `p:sldId` stays above every id seen in the session,
part numbers and `rId`s above every one in use or removed. Sections stay
equal to `p:sldIdLst`; `docProps/app.xml` `Slides`/`Notes`/`HiddenSlides`
are kept exact.

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

SmartArt text (Phase 1b, [ADR-0013](../../docs/adr/0013-pptx-phase1b-tables-and-smartart.md)
§4) through `Presentation::smart_art_mut(slide, frame_id)`: `nodes()`,
`set_text`, `set_paragraph_text`, `set_run_text` edit the data model's
node and mirror the edit into its slice of every drawing-cache shape that
shows it (refused when the cache does not pair with the data);
`drop_drawing_cache()` removes the cache instead; `cache_report()` checks
the pairing without changing anything.

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
  (`--site DIR` if it is installed into a directory).
- Phase 2: `tests/structure.rs` (locality, refusals, sweep of every
  fixture slide) and `tests/new_deck.rs`; the independent structure oracle
  `cargo run --release -p ooxml-pptx --example structure_cases -- OUT` then
  `python3 -I crates/ooxml-pptx/tests/oracle/structure.py [--site DIR] OUT reference/fixtures/pptx`;
  a corpus sweep `cargo run --release -p ooxml-pptx --example structure_sweep -- [--out DIR] DIR...`.
- The template: hand-written parts in `template/parts/`, built into
  `template/template.pptx` by `cargo xtask pptx-template` (`--check` in CI
  compares part names, order and content).

  (`--site DIR` if it is installed into a directory). The cases include
  deck-wide `replace_all` calls (ADR-0008 §7 O5): `check.py` finds the
  matches itself, decides the paragraph-local refusals (rules 1, 5, 6)
  itself and compares them with ooxml-pptx's, kind included, and redoes the §4 edits on python-pptx's lxml tree. ADR-0008 O1/O3:
  `cargo run -p ooxml-pptx --example resolve_dump -- OUT.json` then
  `python3 -I crates/ooxml-pptx/tests/oracle/resolve_check.py OUT.json reference/fixtures/pptx`
  (known geometry differences in `tests/oracle/geometry_known_differences.json`;
  the POI table for O2b is `tests/oracle/poi_known_differences.md`).
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
