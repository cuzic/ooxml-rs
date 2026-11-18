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
| any write (`write_gate()`) | `TrackRevisionsOn`, `DocumentProtected { edit }`, `StaleFieldRisk { codes }` (codes carry the argument for `INFO`/`DOCPROPERTY`, which is an allow-list; `INCLUDETEXT`/`INCLUDEPICTURE`/`LINK` need a literal source, and refuse as `(self reference)` when an item token names a bookmark of the document; a custom property linked to content (`linkTarget`) refuses `DOCPROPERTY`, even when it shadows a built-in name; `<malformed field>` for field markup the body walk cannot follow) |
| `set_run_text` (`Run::check_settable()`) | `InsideField`, `InsideRevision`, `UnsupportedContext`, `RunNotTextOnly` |
| text | `InvalidXmlChar` |
| `append_paragraph` | `Core(UnresolvedSibling)` (`mc:AlternateContent` in the body), `SectionBreakAtEnd` (last section's `sectPr` in the last paragraph), `InsideField` (unterminated field), `NoBody` |

Reads always work after a gate refusal. `write_protection()` ("password to modify", "read-only
recommended") and `is_marked_final()` are reported, not enforced.

## Not yet

Paragraph and cell text, formatting, styles and the `updateFields` opt-in (the rest of
ADR-0007), new structure, and `Document::new()` (from scratch) — see `docs/roadmap/docx.md`.

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
