//! The closed-world scan (ADR-0009 §6): before a rename or a delete, every
//! XML part outside the operation's closed set is scanned for the sheet's
//! name in sheet-prefix form. A hit must lie in a location this version
//! rewrites (or leaves on purpose), or in a known non-reference; any other
//! hit refuses the operation.

use quick_xml::Reader;
use quick_xml::XmlVersion;
use quick_xml::events::Event;

use super::names::mentions_prefix;
use crate::error::{Error, Result};

/// What a part is, for the scan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PartKind {
    /// A worksheet listed in `<sheets>`: `f` text and `hyperlink/@location`
    /// are rewritten; cell values are not references.
    Worksheet,
    /// The workbook part: `definedName` text is rewritten.
    Workbook,
    /// Text that is never a reference (shared strings, comments, threaded
    /// comments, persons, document properties).
    NonReference,
    /// A relationships part: `#Sheet!` targets are retargeted; external
    /// targets name other resources.
    Relationships,
    /// Anything else: every hit refuses.
    Other,
}

/// Is a hit at (element, attribute) of a part of this kind allowed?
fn allowed(kind: PartKind, element: &str, attribute: Option<&str>) -> bool {
    match kind {
        PartKind::NonReference => true,
        PartKind::Worksheet => matches!(
            (element, attribute),
            ("f", None) | ("hyperlink", Some("location")) | ("v", None) | ("t", None)
        ),
        PartKind::Workbook => matches!((element, attribute), ("definedName", None)),
        PartKind::Relationships => matches!((element, attribute), ("Relationship", Some("Target"))),
        PartKind::Other => false,
    }
}

/// Scans one part. `Err(UnrewritableReference)` on a hit outside the
/// allowed places. A part that does not parse as XML is scanned as text
/// (UTF-8, and UTF-16LE if it has that BOM), and any hit refuses
/// (review R2-m3).
pub(crate) fn scan_part(part: &str, bytes: &[u8], kind: PartKind, name: &str) -> Result<()> {
    let refuse = |location: String| Err(Error::UnrewritableReference { part: part.to_owned(), location });
    let decode = |s: &str| crate::escape::decode(s).into_owned();
    let mut reader = Reader::from_reader(bytes);
    let mut stack: Vec<String> = Vec::new();
    let as_text = || -> String {
        match bytes {
            [0xFF, 0xFE, rest @ ..] => {
                let units: Vec<u16> = rest.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
                String::from_utf16_lossy(&units)
            }
            _ => String::from_utf8_lossy(bytes).into_owned(),
        }
    };
    // Text arrives in pieces (entity references are separate events): it is
    // collected per element and checked when the element's content ends.
    let mut buf = String::new();
    let flush = |buf: &mut String, stack: &[String]| -> Result<()> {
        if !buf.is_empty() {
            let el = stack.last().map_or("", String::as_str);
            let hit = mentions_prefix(&decode(buf), name) && !allowed(kind, el, None);
            buf.clear();
            if hit {
                return refuse(format!("<{el}> text"));
            }
        }
        Ok(())
    };
    loop {
        match reader.read_event() {
            Ok(ev @ (Event::Start(_) | Event::Empty(_))) => {
                flush(&mut buf, &stack)?;
                let (Event::Start(e) | Event::Empty(e)) = &ev else { unreachable!() };
                let local = e.local_name().into_inner().to_owned();
                for a in e.attributes().with_checks(true) {
                    let Ok(a) = a else { return scan_text(part, &as_text(), name) };
                    let Ok(v) = a.normalized_value(XmlVersion::Implicit1_0) else { return scan_text(part, &as_text(), name) };
                    if mentions_prefix(&decode(&v), name) {
                        let an = a.key.local_name().into_inner().to_owned();
                        if !allowed(kind, &local, Some(&an)) {
                            return refuse(format!("<{local}> attribute {an}"));
                        }
                    }
                }
                if matches!(ev, Event::Start(_)) {
                    stack.push(local);
                }
            }
            Ok(Event::End(_)) => {
                flush(&mut buf, &stack)?;
                stack.pop();
            }
            Ok(Event::Text(t)) => buf.push_str(&t.xml_content(XmlVersion::Implicit1_0)),
            Ok(Event::CData(t)) => buf.push_str(&t.xml_content(XmlVersion::Implicit1_0)),
            Ok(Event::GeneralRef(r)) => match r.resolve_char_ref() {
                Ok(Some(c)) => buf.push(c),
                _ => buf.push_str(match &*r {
                    "lt" => "<",
                    "gt" => ">",
                    "amp" => "&",
                    "apos" => "'",
                    "quot" => "\"",
                    _ => "\u{fffd}",
                }),
            },
            Ok(Event::Eof) => return flush(&mut buf, &stack),
            Ok(_) => {}
            Err(_) => return scan_text(part, &as_text(), name),
        }
    }
}

fn scan_text(part: &str, text: &str, name: &str) -> Result<()> {
    if mentions_prefix(text, name) {
        return Err(Error::UnrewritableReference { part: part.to_owned(), location: "a part that is not well-formed XML".into() });
    }
    Ok(())
}
