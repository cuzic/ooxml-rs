//! Prints the text model of a presentation as JSON (the same model the
//! T3 oracle compares; see `tests/oracle/model.rs`).
//!
//! `cargo run -p ooxml-pptx --example dump -- deck.pptx`

#[path = "../tests/oracle/model.rs"]
mod model;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).ok_or("usage: dump <file.pptx>")?;
    let mut pres = ooxml_pptx::Presentation::open(path)?;
    println!("{}", serde_json::to_string_pretty(&model::dump(&mut pres))?);
    Ok(())
}
