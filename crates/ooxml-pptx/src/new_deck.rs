//! `Presentation::new()` from the built-in template (ADR-0012 §8): an
//! embedded `.pptx` authored in this repository
//! (`template/parts/`, built by `cargo xtask pptx-template`), opened
//! through the same path as any file. `docProps/core.xml` is regenerated
//! wholesale (created/modified, no creator), as ADR-0001 does for xlsx.

use std::time::{SystemTime, UNIX_EPOCH};

use ooxml_core::Package;

use crate::Presentation;
use crate::error::Result;

/// The template: one master with the 11 standard layouts (Title Slide …
/// Vertical Title and Text, PowerPoint's order), a notes master, 16:9, no
/// slides, no sections.
static TEMPLATE: &[u8] = include_bytes!("../template/template.pptx");

/// Options of [`Presentation::new_with`].
#[derive(Clone, Copy, Debug)]
pub struct NewOptions {
    /// `dcterms:created` and `dcterms:modified` of `docProps/core.xml`
    /// (injectable so tests and reproducible builds get identical output).
    pub created: SystemTime,
}

impl Default for NewOptions {
    fn default() -> Self {
        NewOptions { created: SystemTime::now() }
    }
}

/// W3CDTF UTC timestamp, e.g. `2026-11-16T09:30:00Z`.
pub(crate) fn w3cdtf(t: SystemTime) -> String {
    let secs = t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let (days, rem) = ((secs / 86_400) as i64, secs % 86_400);
    // Civil date from days since 1970-01-01 (H. Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem / 60 % 60, rem % 60)
}

fn core_xml(t: SystemTime) -> Vec<u8> {
    let now = w3cdtf(t);
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
         <cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" \
         xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:dcterms=\"http://purl.org/dc/terms/\" \
         xmlns:dcmitype=\"http://purl.org/dc/dcmitype/\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\">\
         <dcterms:created xsi:type=\"dcterms:W3CDTF\">{now}</dcterms:created>\
         <dcterms:modified xsi:type=\"dcterms:W3CDTF\">{now}</dcterms:modified>\
         </cp:coreProperties>"
    )
    .into_bytes()
}

impl Presentation {
    /// A new, empty presentation from the built-in template (16:9, the 11
    /// standard layouts, a notes master, no slides). Add slides with
    /// [`Self::add_slide`] and [`Self::layouts`] (`layouts()[6]` is Blank).
    pub fn new() -> Result<Self> {
        Self::new_with(NewOptions::default())
    }

    /// [`Self::new`] with options (the `core.xml` time).
    pub fn new_with(options: NewOptions) -> Result<Self> {
        let mut pkg = Package::from_bytes(TEMPLATE)?;
        pkg.set_part("docProps/core.xml", core_xml(options.created))?;
        Self::from_package(pkg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn dates() {
        assert_eq!(w3cdtf(UNIX_EPOCH), "1970-01-01T00:00:00Z");
        assert_eq!(w3cdtf(UNIX_EPOCH + Duration::from_secs(1_794_822_645)), "2026-11-16T09:50:45Z");
        ooxml_core::XmlDocument::parse(&core_xml(UNIX_EPOCH)).unwrap();
    }
}
