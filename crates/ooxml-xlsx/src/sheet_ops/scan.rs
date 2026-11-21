//! The closed-world scan (ADR-0009 §6): before a rename or a delete, every
//! XML part outside the operation's closed set is scanned for the sheet's
//! name in sheet-prefix form. A hit must lie in a location this version
//! rewrites (or leaves on purpose), or in a known non-reference; any other
//! hit refuses the operation.
//!
use quick_xml::events::Event;
use quick_xml::name::ResolveResult;
use quick_xml::{NsReader, XmlVersion};

use super::names::mentions_prefix;
use crate::error::{Error, Result};
use crate::formula::lexer::{Prefix, TokKind, lex};
use crate::locations::{self, PartKind};

const SML: &str = crate::SML_NS;
/// One open element: namespace URI and local name.
type El = (String, String);

fn is(e: Option<&El>, ns: &str, local: &str) -> bool {
    e.is_some_and(|(n, l)| n == ns && l == local)
}

/// Is a hit in the text of the innermost element of `stack` (or in its
/// attribute `attribute`) at a place this version rewrites or ignores?
fn allowed(kind: PartKind, stack: &[El], attribute: Option<&str>, uri: Option<&str>) -> bool {
    let at = |k: usize| stack.len().checked_sub(k + 1).and_then(|i| stack.get(i));
    if locations::find(kind, stack, attribute, uri).is_some_and(|row| row.rename != locations::Rule::Unaffected) {
        return true;
    }
    match (kind, attribute) {
        (PartKind::NonReference, _) => true,
        // cached cell values and inline-string text are not references
        (PartKind::Worksheet, None) if is(at(0), SML, "v") => is(at(1), SML, "c") && is(at(2), SML, "row"),
        (PartKind::Worksheet, None) if is(at(0), SML, "t") => {
            (is(at(1), SML, "is") && is(at(2), SML, "c")) || (is(at(1), SML, "r") && is(at(2), SML, "is") && is(at(3), SML, "c"))
        }
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
pub(crate) fn scan_part(part: &str, bytes: &[u8], kind: PartKind, name: &str, check_extension_positions: bool) -> Result<()> {
    let refuse = |location: String| Err(Error::UnrewritableReference { part: part.to_owned(), location });
    let check = |value: &str, stack: &[El], attribute: Option<&str>, uri: Option<&str>, supported_target: bool| {
        if guid_shaped(value) {
            return Ok(());
        }
        let field = attribute.or_else(|| stack.last().map(|(_, local)| local.as_str()));
        let position = check_extension_positions && uri.is_some()
            && field.is_some_and(|field| matches!(field, "f" | "sqref" | "ref" | "r"))
            && lex(value).is_ok_and(|tokens| tokens.iter().any(|token| {
                matches!(&token.kind, TokKind::Ref(reference) if reference.prefix == Prefix::None)
            }));
        if (position || (!name.is_empty() && mentions_prefix(value, name))) && !(supported_target && allowed(kind, stack, attribute, uri)) {
            let location = describe(stack, attribute);
            return refuse(uri.map_or_else(|| location.clone(), |uri| format!("{uri} {location}")));
        }
        Ok(())
    };
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
    let mut extensions = Vec::<(usize, String)>::new();
    // Text arrives in pieces (entity references are separate events): it is
    // collected per element and checked when the element's content ends.
    let mut buf = String::new();
    let flush = |buf: &mut String, stack: &[El], uri: Option<&str>| -> Result<()> {
        if !buf.is_empty() {
            check(&decode(buf), stack, None, uri, true)?;
            buf.clear();
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
                flush(&mut buf, &stack, extensions.last().map(|(_, uri)| uri.as_str()))?;
                let local = e.local_name().into_inner().to_owned();
                stack.push((ns, local));
                if is(stack.last(), SML, "ext") {
                    let uri = e.attributes().flatten().find(|a| a.key.0 == "uri")
                        .and_then(|a| a.normalized_value(XmlVersion::Implicit1_0).ok().map(|v| v.into_owned()))
                        .unwrap_or_default();
                    extensions.push((stack.len(), uri));
                }
                // an External `#Sheet!…` target names this workbook, but only
                // Internal targets are retargeted (review R2-m1)
                let external = kind == PartKind::Relationships
                    && e.attributes().flatten().any(|a| a.key.local_name().into_inner() == "TargetMode" && a.normalized_value(XmlVersion::Implicit1_0).is_ok_and(|v| v.trim() == "External"));
                for a in e.attributes().with_checks(true) {
                    let Ok(a) = a else { return scan_text(part, &as_text(), name) };
                    let Ok(v) = a.normalized_value(XmlVersion::Implicit1_0) else { return scan_text(part, &as_text(), name) };
                    let attr = a.key.0;
                    let supported_target = a.key.prefix().is_none() && !(external && v.trim_start().starts_with('#'));
                    check(&decode(&v), &stack, Some(attr), extensions.last().map(|(_, uri)| uri.as_str()), supported_target)?;
                }
                if matches!(ev, Event::Empty(_)) {
                    if extensions.last().is_some_and(|(depth, _)| *depth == stack.len()) { extensions.pop(); }
                    stack.pop();
                }
            }
            Event::End(_) => {
                flush(&mut buf, &stack, extensions.last().map(|(_, uri)| uri.as_str()))?;
                if extensions.last().is_some_and(|(depth, _)| *depth == stack.len()) { extensions.pop(); }
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
            Event::DocType(_) => return scan_text(part, &as_text(), name),
            Event::Eof => return flush(&mut buf, &stack, extensions.last().map(|(_, uri)| uri.as_str())),
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

fn guid_shaped(value: &str) -> bool {
    let value = value.trim();
    let value = value.strip_circumfix('{', '}').unwrap_or(value);
    value.len() == 36 && value.bytes().enumerate().all(|(index, byte)| {
        if matches!(index, 8 | 13 | 18 | 23) { byte == b'-' } else { byte.is_ascii_hexdigit() }
    })
}
