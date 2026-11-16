//! Formula detection by element name (ADR-0001 R3-M1, post-acceptance note
//! of 2026-11-16): an element whose namespace is SpreadsheetML main and whose
//! local name is `f`. Never a byte search for `<f`, which also matches
//! `<formula>`, `<from>`, `<fill>`, `<filterColumn>` (roadmap §5).

use quick_xml::NsReader;
use quick_xml::events::Event;
use quick_xml::name::ResolveResult;

use crate::SML_NS;

/// Does this sheet part contain a SpreadsheetML `f` element anywhere?
/// `Err(reason)` if it cannot be decided (not UTF-8, not well-formed, DTD).
pub(crate) fn contains_sml_formula(xml: &[u8]) -> Result<bool, String> {
    if xml.starts_with(&[0xFF, 0xFE]) || xml.starts_with(&[0xFE, 0xFF]) {
        return Err("UTF-16 parts are not supported".into());
    }
    let mut reader = NsReader::from_reader(xml);
    loop {
        match reader.read_resolved_event() {
            Ok((ResolveResult::Bound(ns), Event::Start(e) | Event::Empty(e)))
                if ns.into_inner() == SML_NS && e.local_name().into_inner() == "f" =>
            {
                return Ok(true);
            }
            Ok((_, Event::DocType(_))) => return Err("DTD (<!DOCTYPE>) is not allowed".into()),
            Ok((_, Event::Eof)) => return Ok(false),
            Ok(_) => {}
            Err(e) => return Err(format!("XML error at byte {}: {e}", reader.buffer_position())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NS: &str = r#"xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main""#;

    #[test]
    fn by_qname_not_bytes() {
        let yes = format!(r#"<worksheet {NS}><sheetData><row r="1"><c r="A1"><f>1+1</f><v>2</v></c></row></sheetData></worksheet>"#);
        assert_eq!(contains_sml_formula(yes.as_bytes()), Ok(true));
        let prefixed = r#"<x:worksheet xmlns:x="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><x:f/></x:worksheet>"#;
        assert_eq!(contains_sml_formula(prefixed.as_bytes()), Ok(true));
        let no = format!(
            r#"<worksheet {NS} xmlns:xm="http://schemas.microsoft.com/office/excel/2006/main"><conditionalFormatting><cfRule><formula>A1</formula></cfRule></conditionalFormatting><fill/><from/><filterColumn/><xm:f>A1</xm:f></worksheet>"#
        );
        assert_eq!(contains_sml_formula(no.as_bytes()), Ok(false));
        let other_ns = r#"<w xmlns="urn:other"><f/></w>"#;
        assert_eq!(contains_sml_formula(other_ns.as_bytes()), Ok(false));
        assert!(contains_sml_formula(b"<!DOCTYPE a><a/>").is_err());
        assert!(contains_sml_formula(b"<a><b></a>").is_err());
    }
}
