//! The closed-world scan (ADR-0009 §6): before a rename or a delete, every
//! XML part outside the operation's closed set is scanned for the sheet's
//! name in sheet-prefix form. A hit must lie in a location this version
//! rewrites (or leaves on purpose), or in a known non-reference; any other
//! hit refuses the operation.
//!
//! Locations are matched by **namespace and path**, exactly as the rewriter
//! reaches them (review of PR #53, B1): `f` is allowed only as
//! `worksheet/sheetData/row/c/f` in SpreadsheetML, or as `c:f`/`c15:f` in a
//! chart; an x14 `xm:f` (data validation, conditional formatting,
//! sparklines) or any other `f` refuses.

use quick_xml::events::Event;
use quick_xml::name::ResolveResult;
use quick_xml::{NsReader, XmlVersion};

use super::names::mentions_prefix;
use crate::error::{Error, Result};

const SML: &str = crate::SML_NS;
pub(crate) const CHART_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/chart";
pub(crate) const C15_NS: &str = "http://schemas.microsoft.com/office/drawing/2012/chart";
const RELS_NS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";

/// What a part is, for the scan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PartKind {
    /// A worksheet listed in `<sheets>`: cell formulas and
    /// `hyperlink/@location` are rewritten; cell values are not references.
    Worksheet,
    /// The workbook part: `definedName` text is rewritten.
    Workbook,
    /// Text that is never a reference (shared strings, comments, threaded
    /// comments, persons, document properties).
    NonReference,
    /// A relationships part: `#Sheet!` targets are retargeted; external
    /// targets name other resources.
    Relationships,
    /// A chart part: `c:f` and `c15:f` text is rewritten.
    Chart,
    /// Anything else: every hit refuses.
    Other,
}

/// One open element: namespace URI and local name.
type El = (String, String);

fn is(e: Option<&El>, ns: &str, local: &str) -> bool {
    e.is_some_and(|(n, l)| n == ns && l == local)
}

/// Is a hit in the text of the innermost element of `stack` (or in its
/// attribute `attribute`) at a place this version rewrites or ignores?
fn allowed(kind: PartKind, stack: &[El], attribute: Option<&str>) -> bool {
    let at = |k: usize| stack.len().checked_sub(k + 1).and_then(|i| stack.get(i));
    match (kind, attribute) {
        (PartKind::NonReference, _) => true,
        // worksheet/sheetData/row/c/f (the cell formula the rewriter edits)
        (PartKind::Worksheet, None) if is(at(0), SML, "f") => {
            is(at(1), SML, "c") && is(at(2), SML, "row") && is(at(3), SML, "sheetData") && stack.len() == 5
        }
        // cached cell values and inline-string text are not references
        (PartKind::Worksheet, None) if is(at(0), SML, "v") => is(at(1), SML, "c") && is(at(2), SML, "row"),
        (PartKind::Worksheet, None) if is(at(0), SML, "t") => {
            (is(at(1), SML, "is") && is(at(2), SML, "c")) || (is(at(1), SML, "r") && is(at(2), SML, "is") && is(at(3), SML, "c"))
        }
        (PartKind::Worksheet, Some("location")) => is(at(0), SML, "hyperlink") && is(at(1), SML, "hyperlinks"),
        (PartKind::Workbook, None) => is(at(0), SML, "definedName") && is(at(1), SML, "definedNames"),
        (PartKind::Chart, None) => is(at(0), CHART_NS, "f") || is(at(0), C15_NS, "f"),
        (PartKind::Relationships, Some("Target")) => is(at(0), RELS_NS, "Relationship"),
        _ => false,
    }
}

fn describe(stack: &[El], attribute: Option<&str>) -> String {
    let el = stack.last().map_or("", |(_, l)| l.as_str());
    match attribute {
        Some(a) => format!("<{el}> attribute {a}"),
        None => format!("<{el}> text"),
    }
}

/// Scans one part. `Err(UnrewritableReference)` on a hit outside the
/// allowed places. A part that does not parse as XML is scanned as text
/// (UTF-8, and UTF-16LE if it has that BOM), and any hit refuses
/// (review R2-m3).
pub(crate) fn scan_part(part: &str, bytes: &[u8], kind: PartKind, name: &str) -> Result<()> {
    let refuse = |location: String| Err(Error::UnrewritableReference { part: part.to_owned(), location });
    let decode = |s: &str| crate::escape::decode(s).into_owned();
    let as_text = || -> String {
        match bytes {
            [0xFF, 0xFE, rest @ ..] => {
                let units: Vec<u16> = rest.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
                String::from_utf16_lossy(&units)
            }
            _ => String::from_utf8_lossy(bytes).into_owned(),
        }
    };
    let mut reader = NsReader::from_reader(bytes);
    let mut stack: Vec<El> = Vec::new();
    // Text arrives in pieces (entity references are separate events): it is
    // collected per element and checked when the element's content ends.
    let mut buf = String::new();
    let flush = |buf: &mut String, stack: &[El]| -> Result<()> {
        if !buf.is_empty() {
            let hit = mentions_prefix(&decode(buf), name) && !allowed(kind, stack, None);
            buf.clear();
            if hit {
                return refuse(describe(stack, None));
            }
        }
        Ok(())
    };
    loop {
        let (ns, ev) = match reader.read_resolved_event() {
            Ok(x) => x,
            Err(_) => return scan_text(part, &as_text(), name),
        };
        let ns = match ns {
            ResolveResult::Bound(n) => n.into_inner().to_owned(),
            _ => String::new(),
        };
        match ev {
            Event::Start(ref e) | Event::Empty(ref e) => {
                flush(&mut buf, &stack)?;
                let local = e.local_name().into_inner().to_owned();
                stack.push((ns, local));
                for a in e.attributes().with_checks(true) {
                    let Ok(a) = a else { return scan_text(part, &as_text(), name) };
                    let Ok(v) = a.normalized_value(XmlVersion::Implicit1_0) else { return scan_text(part, &as_text(), name) };
                    if mentions_prefix(&decode(&v), name) {
                        let an = a.key.local_name().into_inner().to_owned();
                        let attr_ok = a.key.prefix().is_none() && allowed(kind, &stack, Some(&an));
                        if !attr_ok {
                            return refuse(describe(&stack, Some(&an)));
                        }
                    }
                }
                if matches!(ev, Event::Empty(_)) {
                    stack.pop();
                }
            }
            Event::End(_) => {
                flush(&mut buf, &stack)?;
                stack.pop();
            }
            Event::Text(t) => buf.push_str(&t.xml_content(XmlVersion::Implicit1_0)),
            Event::CData(t) => buf.push_str(&t.xml_content(XmlVersion::Implicit1_0)),
            Event::GeneralRef(r) => match r.resolve_char_ref() {
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
            Event::Eof => return flush(&mut buf, &stack),
            _ => {}
        }
    }
}

fn scan_text(part: &str, text: &str, name: &str) -> Result<()> {
    if mentions_prefix(text, name) {
        return Err(Error::UnrewritableReference { part: part.to_owned(), location: "a part that is not well-formed XML".into() });
    }
    Ok(())
}
