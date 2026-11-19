//! Shared A4 / 16:10 case for python-pptx and the manual-check bundle.

use std::path::Path;

use ooxml_core::Package;
use ooxml_pptx::Presentation;

pub const FIXTURE: &str = "python-pptx/shp-connector-props.pptx";
pub const NAME: &str = "33-p3d-create-notes-a4";

pub fn create(input: &Path) -> Result<Presentation, Box<dyn std::error::Error>> {
    let mut pkg = Package::open(input)?;
    let part = "ppt/presentation.xml";
    let original = String::from_utf8(pkg.read_part(part)?)?;
    let resized = original
        .replace(
            r#"<p:sldSz cx="9144000" cy="6858000" type="screen4x3"/>"#,
            r#"<p:sldSz cx="12192000" cy="7620000"/>"#,
        )
        .replace(
            r#"<p:notesSz cx="6858000" cy="9144000"/>"#,
            r#"<p:notesSz cx="7560000" cy="10692000"/>"#,
        );
    if !resized.contains(r#"<p:sldSz cx="12192000" cy="7620000"/>"#)
        || !resized.contains(r#"<p:notesSz cx="7560000" cy="10692000"/>"#)
    {
        return Err("the notes-creation fixture dimensions changed".into());
    }
    pkg.set_part(part, resized.into_bytes())?;
    let mut deck = Presentation::from_bytes(pkg.to_bytes()?)?;
    deck.notes_mut_or_create(0)?
        .body_mut()?
        .set_text("Speaker notes for an A4 page.\nCentred 16:10 slide image.")?;
    Ok(deck)
}
