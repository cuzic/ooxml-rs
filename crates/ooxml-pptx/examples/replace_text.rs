//! Walk the slides and shapes of a deck, replace text at the run, paragraph
//! and text-body level, save, and show how refusals look.
//!
//! ```sh
//! cargo run -p ooxml-pptx --example replace_text -- [OUT_DIR]
//! ```
//!
//! `OUT_DIR` defaults to `<temp dir>/ooxml-rs-examples`. Inputs are fixtures
//! committed under `reference/fixtures/pptx/`.

use std::path::PathBuf;

use ooxml_pptx::{Error, Presentation, ShapeKind};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../reference/fixtures/pptx");

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = std::env::args().nth(1).map_or_else(|| std::env::temp_dir().join("ooxml-rs-examples"), PathBuf::from);
    std::fs::create_dir_all(&out)?;

    // 1. Read: every slide, every shape with text.
    let input = format!("{FIXTURES}/poi/sample.pptx");
    let mut pres = Presentation::open(&input)?;
    println!("opened {input}: {} slides", pres.slide_count());
    let mut targets = Vec::new(); // (slide, shape id) of shapes with a text body
    // (slide, shape id, paragraph index ≥ 1) of a paragraph that has a run, for
    // the run-level edit (review of PR #43, m1: not every paragraph has one)
    let mut run_target = None;
    for i in 0..pres.slide_count() {
        let slide = pres.slide(i)?;
        println!("slide {i} ({})", slide.info().part_name);
        for shape in slide.shapes() {
            let Some(body) = shape.text_body() else { continue };
            let id = shape.id()?;
            let kind = if shape.placeholder().is_some() { "placeholder" } else { "shape" };
            println!("  {kind} {id} {:?}: {:?}", shape.name().unwrap_or(""), body.text());
            if matches!(shape.kind(), ShapeKind::Shape) {
                if run_target.is_none() && !targets.is_empty() {
                    let paras = body.paragraphs();
                    run_target = (1..paras.len()).find(|&k| !paras[k].runs().is_empty()).map(|k| (i, id, k));
                }
                targets.push((i, id));
            }
        }
    }

    // 2. Edit at three levels. Every refusal comes before any change.
    if let Some(&(slide, id)) = targets.first() {
        let mut s = pres.slide_mut(slide)?;
        let mut shape = s.shape_mut_by_id(id)?;
        let mut body = shape.text_body_mut()?;
        // Whole body: one paragraph per line; formatting of existing paragraphs is kept.
        body.set_text("Replaced title")?;
        println!("slide {slide}, shape {id}: text body set");
    }
    if let Some((slide, id, k)) = run_target {
        let mut s = pres.slide_mut(slide)?;
        let mut shape = s.shape_mut_by_id(id)?;
        let mut body = shape.text_body_mut()?;
        // One paragraph: the first run keeps its formatting, other runs go.
        body.paragraph_mut(0)?.set_text("First paragraph, replaced")?;
        println!("slide {slide}, shape {id}: paragraph 0 set");
        // One run: only its `a:t` changes.
        body.paragraph_mut(k)?.run_mut(0)?.set_text("first run replaced")?;
        println!("slide {slide}, shape {id}: paragraph {k}, run 0 set");
    }
    let saved = out.join("replace_text.pptx");
    pres.save(&saved)?;
    println!("saved {} (untouched slides and parts are copied as they were)", saved.display());

    // 3. Refusals: typed errors, nothing changed.
    let animated = format!("{FIXTURES}/poi/45545_Comment.pptx");
    let mut pres = Presentation::open(&animated)?;
    let result = pres.slide_mut(0)?.shape_mut_by_id(584_707)?.text_body_mut()?.set_text("one line");
    match result {
        Err(e @ Error::AnimatedParagraphs { .. }) => println!("refused (animation targets paragraphs): {e}"),
        other => println!("unexpected: {other:?}"),
    }
    match pres.slide_mut(0)?.shape_mut_by_id(584_707)?.text_body_mut()?.paragraph_mut(0)?.set_text("a\nb") {
        Err(e @ Error::LineBreakInParagraph) => println!("refused (a paragraph holds no line break): {e}"),
        other => println!("unexpected: {other:?}"),
    }
    Ok(())
}
