//! Writes the T3 oracle cases (ADR-0005 §7) and the find/replace cases
//! (ADR-0008 §7 O5) for the independent python-pptx
//! check: `cases.json`, one edited `<case>.pptx` per case, and
//! ooxml-pptx's own model of each input and output.
//!
//! ```text
//! cargo run -p ooxml-pptx --example oracle_cases -- OUTDIR
//! python3 -I crates/ooxml-pptx/tests/oracle/check.py OUTDIR reference/fixtures/pptx
//! ```

#[path = "../tests/oracle/cases.rs"]
mod cases;
#[path = "../tests/oracle/model.rs"]
mod model;
#[path = "../tests/oracle/notes_creation_case.rs"]
mod notes_creation_case;
#[path = "../tests/oracle/shape_properties_case.rs"]
mod shape_properties_case;
#[path = "../tests/oracle/links_case.rs"]
mod links_case;

use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = PathBuf::from(std::env::args().nth(1).ok_or("usage: oracle_cases OUTDIR")?);
    std::fs::create_dir_all(&out)?;
    let mut cases = cases::all_cases();
    cases.extend(cases::replace_cases()); // ADR-0008 §7 O5 (review #48 M1)
    for c in &cases {
        std::fs::write(out.join(format!("{}.pptx", c.name)), &c.output)?;
        let mut input = ooxml_pptx::Presentation::from_bytes(cases::read_fixture(&c.fixture))?;
        std::fs::write(out.join(format!("{}.input.ooxml.json", c.name)), serde_json::to_string(&model::dump(&mut input))?)?;
        let mut output = ooxml_pptx::Presentation::from_bytes(c.output.clone())?;
        std::fs::write(out.join(format!("{}.ooxml.json", c.name)), serde_json::to_string(&model::dump(&mut output))?)?;
    }
    std::fs::write(out.join("cases.json"), serde_json::to_string_pretty(&cases::cases_json(&cases))?)?;
    formatting_case(&out)?;
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../reference/fixtures/pptx")
        .join(notes_creation_case::FIXTURE);
    let notes = notes_creation_case::create(&source)?;
    notes.save(out.join(format!("{}.pptx", notes_creation_case::NAME)))?;
    let mut shapes = ooxml_pptx::Presentation::from_bytes(cases::read_fixture(shape_properties_case::FIXTURE))?;
    shape_properties_case::edit(&mut shapes)?;
    shapes.save(out.join(format!("{}.pptx", shape_properties_case::NAME)))?;
    let mut links = ooxml_pptx::Presentation::from_bytes(cases::read_fixture(links_case::FIXTURE))?;
    links_case::edit(&mut links)?;
    links.save(out.join(format!("{}.pptx", links_case::NAME)))?;
    let edits: usize = cases.iter().map(|c| c.edits.len()).sum();
    println!("{} cases, {edits} edits written to {}", cases.len(), out.display());
    Ok(())
}

fn formatting_case(out: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    use ooxml_core::value::DmlPercent;
    use ooxml_pptx::{
        AutoFitMode, ColorSpec, ColorTransform, FontRef, FormatChange, Presentation, RunFill,
        RunFormat, SchemeColor, Spacing, WriteColor,
    };
    use ooxml_schema::dml::{TextAlignmentTypeValues, TextUnderlineValues};

    let mut deck = Presentation::from_bytes(cases::read_fixture("poi/sample.pptx"))?;
    let mut slide = deck.slide_mut(0)?;
    let id = slide.as_slide().shapes().into_iter().find(|shape| shape.text_body().is_some())
        .ok_or("formatting fixture has no text shape")?.id()?;
    let mut shape = slide.shape_mut_by_id(id)?;
    let mut body = shape.text_body_mut()?;
    body.autofit(AutoFitMode::Shape)?;
    let mut para = body.paragraph_mut(0)?;
    para.align(TextAlignmentTypeValues::Center)?;
    para.level(1)?;
    para.line_spacing(Spacing::Percent(DmlPercent::Thousandths(120_000)))?;
    para.space_before(Spacing::Points(600))?;
    para.space_after(Spacing::Points(300))?;
    let rgb = ColorSpec::from(WriteColor::Rgb([0x12, 0x34, 0x56]));
    para.format_range(0, 5, &RunFormat {
        latin: Some(FormatChange::Set(FontRef::Typeface("Arial".into()))),
        size: Some(FormatChange::Set(2800)),
        bold: Some(FormatChange::Set(true)),
        italic: Some(FormatChange::Set(true)),
        underline: Some(FormatChange::Set(TextUnderlineValues::Single)),
        fill: Some(FormatChange::Set(RunFill::Solid(rgb))),
        ..RunFormat::default()
    })?;
    para.run_mut(1)?.color(ColorSpec {
        color: WriteColor::Scheme(SchemeColor::Accent1),
        transforms: vec![ColorTransform::LuminanceModulation(80_000), ColorTransform::LuminanceOffset(20_000)],
    })?;
    deck.save(out.join("text-formatting.pptx"))?;
    Ok(())
}
