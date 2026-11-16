# ooxml-docx

Lossless editing of existing Word documents (`.docx`, `.docm`, `.dotx`, `.dotm`) on top of
[`ooxml-core`](../ooxml-core). This is Word **Phase0**,
[ADR-0004](../../docs/adr/0004-docx-phase0.md) (revision 2):

- read the paragraphs of the main document's body flow, their text, and their runs with a
  `RunContext` (direct, hyperlink, revision, content control, …) and field state;
- replace the text of one existing run: `Document::set_run_text(paragraph, run, text)`
  (`\t` → `w:tab`, `\n`/`\r\n`/`\r` → `w:br`, `xml:space="preserve"` where needed);
- append a plain paragraph before the body's `w:sectPr`: `Document::append_paragraph(text)`;
- refuse, with a typed `Error` and before any change, what Phase0 cannot do safely.

Untouched parts are copied byte for byte. An edit changes the main part only inside the edited
run (or at the insertion point). Phase0 creates no IDs, styles, relationships or parts.

```rust,no_run
use ooxml_docx::Document;

let mut doc = Document::open("letter.docx")?;
doc.write_gate()?;
let hit = doc.paragraphs().find_map(|p| {
    p.runs().find(|r| r.text() == "{{name}}" && r.check_settable().is_ok()).map(|r| (p.index(), r.index()))
});
if let Some((p, r)) = hit {
    doc.set_run_text(p, r, "Ada Lovelace")?;
}
doc.append_paragraph("Regards,\tthe team")?;
doc.save("letter-filled.docx")?;
# Ok::<(), ooxml_docx::Error>(())
```

## Word Phase1, part 1 (ADR-0007 §1, §2, §6.1, §7)

- **Stories.** `Document::stories()` lists the main story, headers, footers, footnotes,
  endnotes and comments (found by relationship). Each has paragraphs and runs like the main
  story; `Story::sections()` tells which sections use a header or footer, and whether Word shows
  it. Separator notes are never settable. `set_run_text_at(RunRef, text)` edits a run of any
  story.
- **Cross-run find and replace.** `find`, `replace`, `replace_at`, `replace_matches` match on
  `Paragraph::text()` across runs (rsid, proofing and `_GoBack` splits), give the replacement the
  formatting of the run holding the first matched character, and change only the run content
  children that hold matched characters (`w:noBreakHyphen`, `w:cr`, untouched `w:t`, and a
  `w:lastRenderedPageBreak` at the match edges stay byte-identical). Wrapper edges, fields,
  revisions, content controls, smart tags and range markers inside a match refuse it with a typed
  `RefusalReason`; mixed formatting is refused unless it is invisible
  (`RunFormatting::IgnoreInvisible`, the default). Calls are atomic (every insertion is dry-run
  first), `OnRefused::Skip` reports skipped matches, and a `Match` used after its paragraph
  changed is `StaleMatch` (per-paragraph generations plus a document id).
- **Per-operation W2.** `write_gate_for(Operation)`: text edits and appends also accept `SEQ`,
  `TC`, `XE`, `NOTEREF`, `LISTNUM`, `AUTONUM*`; formatting also the word/character statistics and
  `=`. `write_gate()` stays the strictest gate.

```rust,no_run
use ooxml_docx::{Document, ReplaceOptions};

let mut doc = Document::open("template.docx")?;
let report = doc.replace("{{client}}", "ACME Corp.", &ReplaceOptions::default())?; // every story
assert!(report.skipped.is_empty());
doc.save("filled.docx")?;
# Ok::<(), ooxml_docx::Error>(())
```

## Word Phase1, part 2 (ADR-0007 §4, §5, §6.3)

- **Direct formatting.** `format_run`, `format_paragraph_runs` and `format_paragraph` take a
  `RunFormat` / `ParagraphFormat` of `Prop<T>` values (`Keep`, `Set`, `Clear`): bold, italic,
  strike and their complex-script twins, underline, colour, size, highlight, vertical alignment,
  font slots; alignment, spacing, line spacing, indents, first line/hanging, keep with
  next/together.
  - Writing what is already there is a no-op, and the output is byte-identical.
  - Setting a value removes the attributes that would override it (theme colours and fonts,
    `*Chars`/`*Lines`/`*Autospacing`). Superseding values inherited from the style chain are
    cancelled with `0`.
  - Elements go into their schema slot. Every insertion is dry-run first, so a producer's
    out-of-order `w:pPr` or `w:settings` refuses before any write.
- **Styles by id.** `styles()`, `style_ids_by_name()` (case-insensitive, the portable lookup in
  localized documents), `set_paragraph_style` and `set_run_style`, with `UnknownStyle`
  (`latent_only`), `StyleTypeMismatch` (`linked`) and `AmbiguousStyle`. `styles.xml` is
  read-only.
- **`enable_update_fields_on_open()`** writes `w:updateFields` and lifts W2 for every later
  operation of this `Document`. Word updates fields on open; other readers keep the stale
  results.

```rust,no_run
use ooxml_docx::{Document, Prop, RunFormat, RunRef, ParaRef, StoryIndex};

let mut doc = Document::open("report.docx")?;
let total = RunRef::new(ParaRef::new(StoryIndex::MAIN, 3), 0);
doc.format_run(total, &RunFormat { bold: Prop::Set(true), ..RunFormat::default() })?;
if let Some(id) = doc.style_ids_by_name("heading 2").first().map(|s| s.to_string()) {
    doc.set_paragraph_style(ParaRef::new(StoryIndex::MAIN, 2), Some(&id))?;
}
doc.save("report-formatted.docx")?;
# Ok::<(), ooxml_docx::Error>(())
```

## Word Phase2, part 1 (ADR-0011 §4, §6, §9.1, §10)

- **`insert_paragraph(Place::Before | After, text, ParagraphSpec)`**: a new paragraph in the
  same container as an existing one: body, table cell, header, footer, note or comment.
  - `ParagraphSpec::LikeNeighbour` copies the neighbour's `w:pPr` without `w:sectPr` and
    `w:pPrChange`, so a list continues.
  - `Style(id)` writes an existing paragraph style.
  - Later paragraphs move up by one and get new generations, so earlier `Match`es on them
    are stale.
- **`insert_section_break(after, kind)`** splits the section that holds a body paragraph.
  - The earlier half gets a copy of the section's `w:sectPr`. The header and footer
    references are written again after their `r:id`s are checked.
  - The later half gets `w:type` and keeps numbering continuous: `pgNumType/@start` is
    removed and `lnNumType` `newSection` becomes `continuous`.
- **`append_paragraph`** no longer refuses a body that ends with the last section's
  `w:sectPr` in its last paragraph (ADR-0004 A2): the `w:sectPr` moves to the new paragraph.
- **`insert_hyperlink(position, text, target)`**, where the target is external
  (`HyperlinkTarget::External`) or a bookmark (`Bookmark`).
  - An external target gets a new external relationship of its own, through core's
    `RegistryPlan`.
  - `xmlns:r` is declared on the hyperlink only when no prefixed binding is in scope.
  - The run gets the "Hyperlink" character style when the document has one.
- **Structure column of W2:** structural edits accept only `TC` and `XE` beyond the safe
  fields; anything else needs the `updateFields` opt-in.

## Word Phase2, part 2 (ADR-0011 §3, §7)

- **Tables** (`TableRef`, by document order in a story; nested tables included):
  - `insert_rows` copies a template row's properties: no `tblHeader`, no ids, no rsids, and a
    vertical merge continues only into a merge;
  - `delete_rows` refuses note references, revisions, fields and broken anchors, and repairs
    vertical merges;
  - `insert_column` / `delete_column` work on grid columns and widen or narrow spans,
    `gridBefore` and `gridAfter`;
  - `merge_cells` merges empty cells (`gridSpan` + `vMerge`).

  Every operation needs a consistent grid (`InconsistentGrid`).
- **Images.** `insert_image(position, bytes, ImageSize, alt)` inserts an inline PNG, JPEG or
  GIF picture.
  - The format and size come from the bytes.
  - An identical media part and an existing relationship are reused. Otherwise the part and
    relationship are added through core's registry; no new `Default` content type is added.
  - The drawing id is fresh across every story.
  - `wp`, `r`, `a` and `pic` are declared only where they are not in scope.

## Usage example

`examples/fill_runs.rs` is a runnable walk-through on committed fixtures. It:

- lists runs with their `RunContext` and whether each can be set;
- replaces one run's text;
- appends a paragraph;
- saves to `OUT_DIR/fill_runs.docx` and re-reads it;
- shows the gates (track changes on; TOC/PAGEREF fields) and a run inside a field.

```sh
cargo run -p ooxml-docx --example fill_runs -- [OUT_DIR]   # default: <temp dir>/ooxml-rs-examples
```

```rust,no_run
use ooxml_docx::{Document, Error};

let mut doc = Document::open("letter.docx")?;
match doc.write_gate() {
    Err(e @ (Error::TrackRevisionsOn | Error::DocumentProtected { .. } | Error::StaleFieldRisk { .. })) => {
        eprintln!("read-only here: {e}");
    }
    other => {
        other?;
        doc.set_run_text(0, 0, "Filled in")?; // R1–R4 refusals come back as Err, nothing changed
        doc.append_paragraph("Appended paragraph")?;
        doc.save("letter-filled.docx")?;
    }
}
# Ok::<(), Error>(())
```

## Refusals (ADR-0004 §6)

| When | Error |
|---|---|
| open | `Core(EncryptedPackage)`, `Core(DtdNotAllowed)`, `Core(StrictNotSupported)`, `MainPartMissing`, `Core(TooDeep)` (nesting deeper than core's limit, 256) |
| any write (`write_gate()`) | `TrackRevisionsOn`, `DocumentProtected { edit }`, `StaleFieldRisk { codes }` (codes carry the argument for `INFO`/`DOCPROPERTY`, which is an allow-list; `INCLUDETEXT`/`INCLUDEPICTURE`/`LINK` need a literal source; `<malformed field>` for field markup the body walk cannot follow) |
| `set_run_text` (`Run::check_settable()`) | `InsideField`, `InsideRevision`, `UnsupportedContext`, `RunNotTextOnly` |
| text | `InvalidXmlChar` |
| `append_paragraph` | `Core(UnresolvedSibling)` (`mc:AlternateContent` in the body), `SectionBreakAtEnd` (last section's `sectPr` in the last paragraph), `InsideField` (unterminated field), `NoBody` |

Reads always work after a gate refusal. `write_protection()` ("password to modify", "read-only
recommended") and `is_marked_final()` are reported, not enforced.

## Not yet

Paragraph and cell text (ADR-0007 §3, part 3), bookmarks, lists and new styles (ADR-0011, later
parts), and `Document::new()` (from scratch) — see `docs/roadmap/docx.md`.

Nesting depth is bounded by `ooxml-core`'s parser limit (256 levels by default; real documents reach about
30), so a hostile file of nested tables is refused at open with `Core(TooDeep)` instead of costing
gigabytes or the stack. Below the limit the body index is linear and every walk is iterative.

## Tests

`cargo test -p ooxml-docx` runs the ADR-0004 §7 oracles on `reference/fixtures/docx/` (T1, T1b,
T2 over all 3,423 settable runs, T3, T4, T5 read-back with `docx-rs` 0.4.22, T6) and ADR-0007's
T7: every match of three needle sets in every story is classified by the production code and by
an independent reference classifier (`tests/reference/`, over `roxmltree`), which must agree;
accepted matches are applied and checked for the string oracle, per-character run identity,
locality and byte identity outside the match (`OOXML_T7_FULL=1` applies every match on the
largest fixture too). For the owner's
manual check in Microsoft 365 Word:

```sh
cargo run -p ooxml-docx --example docx_manual_check -- <outdir>   # outputs + CHECKLIST.md
```

`cargo xtask manual-check <outdir>` runs the Excel, PowerPoint and Word checks together and
writes one bundle (per-format folders, an index `CHECKLIST.md`, `manual-check.zip`).
