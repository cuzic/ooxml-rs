//! Read the runs of a Word document, replace the text of one run, append a
//! paragraph, save, and show the document-level gates and per-run refusals.
//!
//! ```sh
//! cargo run -p ooxml-docx --example fill_runs -- [OUT_DIR]
//! ```
//!
//! `OUT_DIR` defaults to `<temp dir>/ooxml-rs-examples`. Inputs are fixtures
//! committed under `reference/fixtures/docx/`.

use std::path::PathBuf;

use ooxml_docx::{Document, Error};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../reference/fixtures/docx");

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = std::env::args().nth(1).map_or_else(|| std::env::temp_dir().join("ooxml-rs-examples"), PathBuf::from);
    std::fs::create_dir_all(&out)?;

    // 1. Read: paragraphs of the body flow and their runs.
    let input = format!("{FIXTURES}/poi-table-alignment.docx");
    let mut doc = Document::open(&input)?;
    println!("opened {input}: {} paragraphs", doc.paragraph_count());
    let mut target = None;
    for p in doc.paragraphs().take(6) {
        println!("  paragraph {}: {:?}", p.index(), p.text());
        for r in p.runs() {
            let settable = match r.check_settable() {
                Ok(()) => "settable".to_owned(),
                Err(e) => format!("not settable: {e}"),
            };
            println!("    run {} {:?} {:?} ({settable})", r.index(), r.context(), r.text());
            if target.is_none() && r.check_settable().is_ok() && !r.text().trim().is_empty() {
                target = Some((p.index(), r.index()));
            }
        }
    }

    // 2. Write. Document-level gates (track changes, protection, fields
    //    whose results could go stale) are checked first.
    doc.write_gate()?;
    if let Some((p, r)) = target {
        doc.set_run_text(p, r, "Filled in by ooxml-rs\twith a tab")?;
        println!("set paragraph {p}, run {r}");
    }
    let new = doc.append_paragraph("Appended paragraph")?;
    println!("appended paragraph {new}");
    let saved = out.join("fill_runs.docx");
    doc.save(&saved)?;
    println!("saved {} (only word/document.xml was rewritten)", saved.display());
    let again = Document::open(&saved)?;
    println!("  re-read last paragraph: {:?}", again.paragraph(new).map(|p| p.text()));

    // 3. Gates and refusals: typed errors, nothing changed.
    for (name, why) in [("poi-delins.docx", "track changes on"), ("sdk-TOC2.docx", "TOC and PAGEREF fields")] {
        let mut doc = Document::open(format!("{FIXTURES}/{name}"))?;
        match doc.append_paragraph("x") {
            Err(e @ (Error::TrackRevisionsOn | Error::StaleFieldRisk { .. })) => println!("{name} ({why}): refused: {e}"),
            other => println!("{name}: unexpected {other:?}"),
        }
    }
    let doc = Document::open(format!("{FIXTURES}/sdk-Field-more-field.docx"))?;
    if let Some(r) = doc.paragraphs().flat_map(|p| p.runs().collect::<Vec<_>>()).find(|r| r.in_field() && !r.text().is_empty()) {
        println!("a run inside a field: {:?} -> {}", r.text(), r.check_settable().unwrap_err());
    }
    Ok(())
}
