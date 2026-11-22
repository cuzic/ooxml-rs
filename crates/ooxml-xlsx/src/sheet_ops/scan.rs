//! The closed-world scan (ADR-0009 §6): before a rename or a delete, every
//! XML part outside the operation's closed set is scanned for the sheet's
//! name in sheet-prefix form. A hit must lie in a location this version
//! rewrites (or leaves on purpose), or in a known non-reference; any other
//! hit refuses the operation.
//!
use quick_xml::events::Event;
use quick_xml::name::ResolveResult;
use quick_xml::{NsReader, XmlVersion};

use super::names::{PrefixEdit, mentions_prefix, rewrite_formula};
use crate::error::{Error, Result};
use crate::formula::lexer::{Prefix, TokKind, lex};
use crate::locations::{self, PartKind, Rule, Syntax};

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
    if locations::find(kind, stack, attribute, uri).is_some_and(locations::Location::accepts_sheet_prefix) {
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
pub(crate) fn scan_part(part: &str, bytes: &[u8], kind: PartKind, name: &str, edit: Option<&PrefixEdit<'_>>) -> Result<()> {
    scan(part, bytes, kind, name, edit, false, edit.is_none())
}

pub(super) fn scan_grid_part(part: &str, bytes: &[u8], kind: PartKind, name: &str, own: bool) -> Result<()> {
    // UTF-16 and character references can spell a delimiter without a
    // literal ASCII exclamation mark at the position being checked.
    let utf8 = !matches!(bytes, [0xff, 0xfe, ..] | [0xfe, 0xff, ..]);
    if utf8 && !bytes.contains(&b'!') && !bytes.windows(2).any(|w| w == b"&#")
        && !(own && bytes.windows(3).any(|w| w == b"ext"))
    {
        return Ok(());
    }
    scan(part, bytes, kind, name, None, true, own)
}

fn scan(part: &str, bytes: &[u8], kind: PartKind, name: &str, edit: Option<&PrefixEdit<'_>>, grid: bool, check_positions: bool) -> Result<()> {
    let refuse = |location: String| Err(Error::UnrewritableReference { part: part.to_owned(), location });
    let check = |value: &str, stack: &[El], attribute: Option<&str>, uri: Option<&str>, supported_target: bool| {
        if guid_shaped(value) {
            return Ok(());
        }
        let field = attribute.or_else(|| stack.last().map(|(_, local)| local.as_str()));
        let position = check_positions && uri.is_some()
            && field.is_some_and(|field| matches!(field, "f" | "sqref" | "ref" | "r"))
            && lex(value).is_ok_and(|tokens| tokens.iter().any(|token| {
                matches!(&token.kind, TokKind::Ref(reference) if reference.prefix == Prefix::None)
            }));
        let unknown_position = position && !(supported_target && locations::find(kind, stack, attribute, uri).is_some());
        let mut unknown_prefix = false;
        if let Some(edit) = edit && mentions_prefix(value, name) {
            unknown_prefix = !(supported_target && allowed(kind, stack, attribute, uri));
            if supported_target && let Some(row) = locations::find(kind, stack, attribute, uri)
                && row.syntax == Syntax::Formula
                && row.sheet_rule(matches!(edit, PrefixEdit::Delete { .. })) == Rule::Refuse
            {
                unknown_prefix = !matches!(rewrite_formula(value, edit, &|text| mentions_prefix(text, name)), Ok(None));
            }
        }
        if grid && value.contains('!') && mentions_prefix(value, name) {
            unknown_prefix = !supported_target || !(locations::find(kind, stack, attribute, uri).is_some()
                || allowed(kind, stack, attribute, uri));
        }
        if unknown_position || unknown_prefix {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structural_scan_checks_ignored_errors_uri() {
        let scan = |uri| {
            let xml = format!(
                r#"<worksheet xmlns="{SML}" xmlns:x14="{}" xmlns:xm="{}">
                    <extLst><ext uri="{uri}"><x14:ignoredErrors><x14:ignoredError>
                        <xm:sqref>A1:B2 C3</xm:sqref>
                    </x14:ignoredError></x14:ignoredErrors></ext></extLst>
                </worksheet>"#,
                crate::x14::NS_X14, crate::x14::NS_XM
            );
            scan_part("sheet.xml", xml.as_bytes(), PartKind::Worksheet, "", None)
        };
        scan("{01252117-D84E-4E92-8308-4BE1C098FCBB}").unwrap();
        std::assert_matches!(scan("{FC87AEE6-9EDD-4A0A-B7FB-166176984837}"), Err(Error::UnrewritableReference { .. }));
    }

    #[test]
    fn structural_scan_checks_the_sort_state_extension_path() {
        let scan = |uri| {
            let xml = format!(r#"<worksheet xmlns="{SML}">
                <autoFilter ref="A1:A5"><sortState ref="A1:A5"><extLst>
                    <ext uri="{uri}"><rd:richSortCondition
                        xmlns:rd="http://schemas.microsoft.com/office/spreadsheetml/2017/richdata2" ref="A2:A5"/>
                    </ext>
                </extLst></sortState></autoFilter>
            </worksheet>"#);
            scan_part("sheet.xml", xml.as_bytes(), PartKind::Worksheet, "", None)
        };
        scan("{0FC57C36-975A-43D2-8416-536B9B585D1B}").unwrap();
        std::assert_matches!(scan("{68452572-C921-441B-96FD-77FB3E787301}"), Err(Error::UnrewritableReference { .. }));
    }

    #[test]
    fn adr_0009_legacy_scan_places_still_accept_sheet_prefixes() {
        let worksheet = |body| format!(r#"<worksheet xmlns="{SML}">{body}</worksheet>"#);
        let cases = [
            (PartKind::Worksheet, worksheet("<sheetData><row><c><f>Data!A1</f><v>Data!A1</v><is><t>Data!A1</t><r><t>Data!A1</t></r></is></c></row></sheetData>")),
            (PartKind::Worksheet, worksheet(r#"<hyperlinks><hyperlink location="Data!A1"/></hyperlinks>"#)),
            (PartKind::Workbook, format!(r#"<workbook xmlns="{SML}"><definedNames><definedName>Data!A1</definedName></definedNames></workbook>"#)),
            (PartKind::Chart, format!(r#"<chart xmlns="{}"><f>Data!A1</f></chart>"#, locations::CHART_NS)),
            (PartKind::Chart, format!(r#"<chart xmlns="{}"><f>Data!A1</f></chart>"#, locations::C15_NS)),
            (PartKind::Relationships, r##"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Target="#Data!A1"/></Relationships>"##.into()),
            (PartKind::NonReference, "<t>Data!A1</t>".into()),
        ];
        let edit = PrefixEdit::Rename { old: "data".into(), new: "New" };
        for (kind, xml) in cases {
            scan_part("part.xml", xml.as_bytes(), kind, "Data", Some(&edit)).unwrap();
        }
    }

    #[test]
    fn extension_guid_in_reference_field_is_not_a_position() {
        let guid = "DEADBEEF-A123-B123-C123-DEADBEEFDEAD";
        let xml = format!(r#"<worksheet xmlns="{SML}"><extLst><ext uri="unknown">
            <unknown ref="{guid}"/><unknown ref="{{{guid}}}"/>
        </ext></extLst></worksheet>"#);
        assert!(lex(guid).unwrap().iter().any(|token| matches!(&token.kind, TokKind::Ref(_))));
        scan_part("sheet.xml", xml.as_bytes(), PartKind::Worksheet, "", None).unwrap();
        assert!(!guid_shaped("{01252117-D84E-4E92-8308-4BE1C098FCBG}"));
    }
}
