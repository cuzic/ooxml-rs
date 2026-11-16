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

## Not in Phase0

Headers/footers/notes/comments editing, cross-run replace, formatting, new structure, and
`Document::new()` (from scratch) — see `docs/roadmap/docx.md`.

Nesting depth is bounded by `ooxml-core`'s parser limit (256 levels by default; real documents reach about
30), so a hostile file of nested tables is refused at open with `Core(TooDeep)` instead of costing
gigabytes or the stack. Below the limit the body index is linear and every walk is iterative.

## Tests

`cargo test -p ooxml-docx` runs the ADR-0004 §7 oracles on `reference/fixtures/docx/` (T1, T1b,
T2 over all 3,423 settable runs, T3, T4, T5 read-back with `docx-rs` 0.4.22, T6). For the owner's
manual check in Microsoft 365 Word:

```sh
cargo run -p ooxml-docx --example manual_check -- <outdir>   # ten outputs + CHECKLIST.md
```
