//! Writes the T3 oracle cases (ADR-0005 §7) for the independent python-pptx
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

use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = PathBuf::from(std::env::args().nth(1).ok_or("usage: oracle_cases OUTDIR")?);
    std::fs::create_dir_all(&out)?;
    let cases = cases::all_cases();
    for c in &cases {
        std::fs::write(out.join(format!("{}.pptx", c.name)), &c.output)?;
        let mut input = ooxml_pptx::Presentation::from_bytes(cases::read_fixture(&c.fixture))?;
        std::fs::write(out.join(format!("{}.input.ooxml.json", c.name)), serde_json::to_string(&model::dump(&mut input))?)?;
        let mut output = ooxml_pptx::Presentation::from_bytes(c.output.clone())?;
        std::fs::write(out.join(format!("{}.ooxml.json", c.name)), serde_json::to_string(&model::dump(&mut output))?)?;
    }
    std::fs::write(out.join("cases.json"), serde_json::to_string_pretty(&cases::cases_json(&cases))?)?;
    let edits: usize = cases.iter().map(|c| c.edits.len()).sum();
    println!("{} cases, {edits} edits written to {}", cases.len(), out.display());
    Ok(())
}
