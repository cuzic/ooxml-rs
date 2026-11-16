//! Produces a small named set of edited fixtures plus `CHECKLIST.md` for
//! the project owner's manual check in Microsoft 365 Word (ADR-0004 §7,
//! "Manual"). Nothing here runs or renders the documents.
//!
//! ```sh
//! cargo run -p ooxml-docx --example manual_check -- <outdir>
//! ```

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use ooxml_docx::{Document, Paragraph, Run, RunContext};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../reference/fixtures/docx");

struct Case {
    file: &'static str,
    fixture: &'static str,
    /// What was done, filled in by the edit.
    done: String,
    check: &'static str,
}

type Pick = fn(&Paragraph<'_>, &Run<'_>) -> bool;

/// Replaces the text of the first settable run matching `pick`; returns a
/// description.
fn set_first(doc: &mut Document, pick: Pick, text: &str) -> String {
    let (p, r, old, ptext) = doc
        .paragraphs()
        .find_map(|p| {
            p.runs()
                .find(|r| r.check_settable().is_ok() && !r.text().trim().is_empty() && pick(&p, r))
                .map(|r| (p.index(), r.index(), r.text(), p.text()))
        })
        .expect("the fixture has a matching run");
    doc.set_run_text(p, r, text).expect("settable");
    format!("Paragraph {p} (\"{}\"), run {r}: \"{}\" → \"{}\"", short(&ptext), short(&old), text.escape_debug())
}

fn short(s: &str) -> String {
    let s: String = s.chars().take(60).collect();
    s.escape_debug().to_string()
}

fn has_child(r: &Run<'_>, local: &str) -> bool {
    r.raw().elements().any(|c| c.name().local() == local)
}

fn has_text_id(p: &Paragraph<'_>) -> bool {
    p.raw().attrs().iter().any(|a| a.name().local() == "textId")
}

fn run(out: &Path) -> Result<Vec<Case>, Box<dyn std::error::Error>> {
    std::fs::create_dir_all(out)?;
    let mut cases = Vec::new();
    let mut make = |file: &'static str, fixture: &'static str, check: &'static str, f: &dyn Fn(&mut Document) -> String| -> Result<(), Box<dyn std::error::Error>> {
        let mut doc = Document::open(PathBuf::from(FIXTURES).join(format!("{fixture}.docx")))?;
        doc.write_gate()?;
        let done = f(&mut doc);
        doc.save(out.join(file))?;
        Document::open(out.join(file))?; // the output reopens through the same gates
        cases.push(Case { file, fixture, done, check });
        Ok(())
    };

    make(
        "01-table-cell-run.docx",
        "poi-table-alignment",
        "The cell shows the new text; table layout, alignment and the other cells are unchanged.",
        &|d| set_first(d, |_, _| true, "EDITED cell text"),
    )?;
    make(
        "02-hyperlink-run.docx",
        "poi-bug59058",
        "The link text is the new text and the link still works (Ctrl+click); its style is unchanged.",
        &|d| set_first(d, |_, r| r.context() == RunContext::Hyperlink, "EDITED link text"),
    )?;
    make(
        "03-run-with-lastRenderedPageBreak.docx",
        "poi-bug59058",
        "lastRenderedPageBreak effect: the edited run held a w:lastRenderedPageBreak, which Phase0 drops. \
         Check that pagination looks normal (no blank page, no missing page break) around the edited text.",
        &|d| set_first(d, |_, r| has_child(r, "lastRenderedPageBreak"), "EDITED (lastRenderedPageBreak dropped)"),
    )?;
    make(
        "04-paragraph-with-textId.docx",
        "poi-testComment",
        "textId question: the edited paragraph keeps its w14:textId although its text changed. \
         Check for no repair dialog, that the comment is still attached, and (optional) save in Word and \
         see whether Word rewrites w14:textId of that paragraph.",
        &|d| set_first(d, |p, _| has_text_id(p), "EDITED, w14:textId kept"),
    )?;
    make(
        "05-append-without-sectPr.docx",
        "poi-55733",
        "The body has no w:sectPr: the new paragraph is the last thing in the body. It shows after \"TEST\".",
        &|d| {
            d.append_paragraph("APPENDED at the end of a body without sectPr").unwrap();
            "Appended one paragraph".into()
        },
    )?;
    make(
        "06-append-multi-section.docx",
        "poi-Headers",
        "The new paragraph is at the end of the last section, with that section's header; the three sections and \
         their headers are unchanged.",
        &|d| {
            d.append_paragraph("APPENDED to the last section").unwrap();
            "Appended one paragraph".into()
        },
    )?;
    make(
        "07-whitespace-tab-break.docx",
        "poi-checkboxes",
        "Leading/trailing and double spaces are kept (xml:space=\"preserve\"), the tab is a tab, the line break is a \
         line break (not a new paragraph). The form fields still work.",
        &|d| {
            let s = set_first(d, |_, _| true, "  two leading spaces,  a double space,\ta tab and\na line break ");
            d.append_paragraph("Appended:\tTab\nBreak").unwrap();
            format!("{s}; appended \"Appended:\\tTab\\nBreak\"")
        },
    )?;
    make(
        "08-document-with-fields.docx",
        "sdk-Field-more-field",
        "Only the edited text changed. The DATE, AUTHOR and PAGE fields keep their results (Alt+F9 shows the codes \
         unchanged).",
        &|d| set_first(d, |_, _| true, "EDITED in a document with fields"),
    )?;
    make(
        "09-headers-footers-with-fields.docx",
        "poi-PageSpecificHeadFoot",
        "Headers/footers (first, even, default) and their PAGE fields are unchanged; content controls are intact.",
        &|d| {
            let s = set_first(d, |_, _| true, "EDITED body text");
            d.append_paragraph("APPENDED paragraph").unwrap();
            format!("{s}; appended one paragraph")
        },
    )?;
    make(
        "10-footnotes-endnotes.docx",
        "sdk-ftEdNote",
        "Footnotes and endnotes and their reference marks are unchanged.",
        &|d| set_first(d, |_, _| true, "EDITED text near notes"),
    )?;
    Ok(cases)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = PathBuf::from(std::env::args().nth(1).ok_or("usage: manual_check <outdir>")?);
    let cases = run(&out)?;
    let mut md = String::new();
    writeln!(md, "# ooxml-docx Phase0: manual check in Microsoft 365 Word\n")?;
    writeln!(md, "Generated by `cargo run -p ooxml-docx --example manual_check -- {}`.\n", out.display())?;
    writeln!(md, "For every file:\n")?;
    writeln!(md, "- [ ] Opens with **no repair dialog** (\"Word found unreadable content…\").")?;
    writeln!(md, "- [ ] The edited or appended text is shown as described; nothing else changed (compare with the original fixture in `reference/fixtures/docx/`).")?;
    writeln!(md, "- [ ] Comments, fields, headers/footers, notes and formatting look as in the original.")?;
    writeln!(md, "- [ ] Optional: save in Word, reopen: still no dialog.\n")?;
    for c in &cases {
        writeln!(md, "## `{}`\n", c.file)?;
        writeln!(md, "- Source: `reference/fixtures/docx/{}.docx`", c.fixture)?;
        writeln!(md, "- Edit: {}", c.done)?;
        writeln!(md, "- [ ] No repair dialog")?;
        writeln!(md, "- [ ] {}\n", c.check)?;
    }
    writeln!(md, "## Questions to answer\n")?;
    writeln!(md, "1. **w14:textId** (`04`): is keeping the old textId on an edited paragraph harmless (no dialog, comment intact)? If Word rewrites it on save, note that.")?;
    writeln!(md, "2. **lastRenderedPageBreak** (`03`): any visible effect of dropping it from the edited run?")?;
    std::fs::write(out.join("CHECKLIST.md"), md)?;
    for c in &cases {
        println!("{}  ({})", out.join(c.file).display(), c.fixture);
    }
    println!("{}", out.join("CHECKLIST.md").display());
    Ok(())
}
