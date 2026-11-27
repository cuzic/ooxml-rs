//! Scalar values from an OLE property-set stream in a BIFF compound file.

use crate::core_props::{heading_property_keys, w3cdtf_seconds};

const SUMMARY: [&str; 20] = [
    "",
    "CodePage",
    "Title",
    "Subject",
    "Author",
    "Keywords",
    "Comments",
    "Template",
    "LastAuthor",
    "RevNumber",
    "EditTime",
    "LastPrinted",
    "CreatedDate",
    "ModifiedDate",
    "PageCount",
    "WordCount",
    "CharCount",
    "",
    "Application",
    "DocSecurity",
];
const DOCUMENT_SUMMARY: [&str; 30] = [
    "",
    "CodePage",
    "Category",
    "PresentationFormat",
    "ByteCount",
    "LineCount",
    "ParagraphCount",
    "SlideCount",
    "NoteCount",
    "HiddenCount",
    "MultimediaClipCount",
    "ScaleCrop",
    "",
    "",
    "Manager",
    "Company",
    "LinksUpToDate",
    "CharacterCount",
    "",
    "SharedDoc",
    "",
    "",
    "HyperlinksChanged",
    "AppVersion",
    "",
    "",
    "ContentType",
    "ContentStatus",
    "Language",
    "Version",
];

fn u16_at(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(offset..)?.first_chunk().copied()?,
    ))
}

fn u32_at(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..)?.first_chunk().copied()?,
    ))
}

fn decode(bytes: &[u8], codepage: u16) -> Option<String> {
    if codepage == 1200 {
        let (units, rest) = bytes.as_chunks::<2>();
        if !rest.is_empty() {
            return None;
        }
        let units: Vec<_> = units.iter().map(|unit| u16::from_le_bytes(*unit)).collect();
        return Some(String::from_utf16_lossy(
            units.strip_suffix(&[0]).unwrap_or(&units),
        ));
    }
    let encoding = codepage::to_encoding(codepage)?;
    let bytes = bytes.strip_suffix(&[0]).unwrap_or(bytes);
    Some(encoding.decode_without_bom_handling(bytes).0.into_owned())
}

fn scalar(bytes: &[u8], codepage: u16, custom: bool) -> Option<(&'static str, String)> {
    let body = bytes.get(4..)?;
    Some(match u32_at(bytes, 0)? {
        2 => ("i2", (u16_at(body, 0)? as i16).to_string()),
        3 => ("i4", (u32_at(body, 0)? as i32).to_string()),
        4 => ("r4", f32::from_bits(u32_at(body, 0)?).to_string()),
        5 => ("r8", f64::from_le_bytes(*body.first_chunk()?).to_string()),
        11 => ("bool", (u16_at(body, 0)? != 0).to_string()),
        19 => ("uint", u32_at(body, 0)?.to_string()),
        kind @ (30 | 31) => {
            let count = u32_at(body, 0)? as usize;
            let length = if kind == 31 {
                count.checked_mul(2)?
            } else {
                count
            };
            let page = if kind == 31 { 1200 } else { codepage };
            ("string", decode(body.get(4..)?.get(..length)?, page)?)
        }
        64 => {
            let ticks = u64::from_le_bytes(*body.first_chunk()?);
            let unix_millis = i128::from(ticks / 10_000) - 11_644_473_600_000;
            let mut timestamp = w3cdtf_seconds(unix_millis.div_euclid(1000));
            let millis = unix_millis.rem_euclid(1000);
            if millis != 0 {
                timestamp.pop();
                timestamp.push_str(&format!(".{millis:03}Z"));
            }
            (if custom { "date" } else { "string" }, timestamp)
        }
        _ => return None,
    })
}

fn dictionary(mut bytes: &[u8], codepage: u16) -> Option<Vec<(u32, String)>> {
    let count = u32_at(bytes, 0)?;
    bytes = bytes.get(4..)?;
    let mut names = Vec::new();
    for _ in 0..count {
        let id = u32_at(bytes, 0)?;
        let length = u32_at(bytes, 4)? as usize;
        let width = if codepage == 1200 { 2 } else { 1 };
        let length = length.checked_mul(width)?;
        bytes = bytes.get(8..)?;
        names.push((id, decode(bytes.get(..length)?, codepage)?));
        let occupied = if width == 2 {
            length.checked_add(3)? & !3
        } else {
            length
        };
        bytes = bytes.get(occupied..)?;
    }
    names.sort_by_key(|(id, _)| *id);
    Some(names)
}

fn vector_string(bytes: &mut &[u8], wide: bool, codepage: u16) -> Option<String> {
    let count = u32_at(bytes, 0)? as usize;
    let width = if wide { 2 } else { 1 };
    let size = count.checked_mul(width)?;
    let data = bytes.get(4..)?;
    let page = if wide { 1200 } else { codepage };
    let text = decode(data.get(..size)?, page)?;
    let occupied = if wide {
        size.checked_add(3)? & !3
    } else {
        size
    };
    *bytes = data.get(occupied..)?;
    Some(text)
}

fn read_headings(bytes: &[u8], codepage: u16) -> Option<Vec<(String, usize)>> {
    if u32_at(bytes, 0)? != 0x100c {
        return None;
    }
    let count = u32_at(bytes, 4)?;
    if count % 2 != 0 {
        return None;
    }
    let base = bytes.len();
    let mut bytes = bytes.get(8..)?;
    let mut headings = Vec::new();
    for _ in 0..count / 2 {
        let wide = match u32_at(bytes, 0)? {
            30 => false,
            31 => true,
            _ => return None,
        };
        bytes = bytes.get(4..)?;
        let label = vector_string(&mut bytes, wide, codepage)?;
        // ANSI heading strings may be followed by up to three alignment bytes.
        if !wide && u32_at(bytes, 0) != Some(3) {
            let padding = (4 - (base - bytes.len()) % 4) % 4;
            bytes = bytes.get(padding..)?;
        }
        if u32_at(bytes, 0)? != 3 {
            return None;
        }
        let count = usize::try_from(u32_at(bytes, 4)? as i32).ok()?;
        headings.push((label, count));
        bytes = bytes.get(8..)?;
    }
    Some(headings)
}

fn read_titles(bytes: &[u8], codepage: u16) -> Option<Vec<String>> {
    let wide = match u32_at(bytes, 0)? {
        0x101e => false,
        0x101f => true,
        _ => return None,
    };
    let count = u32_at(bytes, 4)?;
    let mut bytes = bytes.get(8..)?;
    (0..count)
        .map(|_| vector_string(&mut bytes, wide, codepage))
        .collect()
}

fn read_section(
    data: &[u8],
    names: &[&str],
    custom: bool,
    document_summary: bool,
    fields: &mut Vec<(String, &'static str, String)>,
) -> Option<()> {
    let size = u32_at(data, 0)? as usize;
    let data = data.get(..size)?;
    let count = u32_at(data, 4)? as usize;
    let table = data.get(8..)?.get(..count.checked_mul(8)?)?;
    let mut offsets = table
        .as_chunks::<8>()
        .0
        .iter()
        .map(|entry| {
            let offset = u32_at(entry, 4)? as usize;
            (offset >= 8 + table.len() && offset < data.len())
                .then_some((offset, u32_at(entry, 0)?))
        })
        .collect::<Option<Vec<_>>>()?;
    offsets.sort_unstable();
    if offsets.windows(2).any(|pair| pair[0].0 == pair[1].0) {
        return None;
    }
    let entries = || {
        offsets
            .iter()
            .zip(
                offsets
                    .iter()
                    .skip(1)
                    .map(|&(offset, _)| offset)
                    .chain(std::iter::once(data.len())),
            )
            .map(|(&(offset, id), end)| (id, &data[offset..end]))
    };
    let codepage = entries()
        .find(|(id, _)| *id == 1)
        .and_then(|(_, value)| u16_at(value, 4))
        .filter(|page| *page != 0)
        .unwrap_or(1252);
    let custom_names = if custom {
        let (_, value) = entries().find(|(id, _)| *id == 0)?;
        dictionary(value, codepage)?
    } else {
        Vec::new()
    };
    if document_summary && !custom {
        let mut headings = None;
        let mut titles = None;
        for (id, value) in entries() {
            match id {
                12 => headings = Some(read_headings(value, codepage)?),
                13 => titles = Some(read_titles(value, codepage)?),
                _ => {}
            }
        }
        if let (Some(headings), Some(titles)) = (headings, titles)
            && !titles.is_empty()
        {
            let mut offset = 0_usize;
            for (label, count) in headings {
                let end = offset.saturating_add(count);
                if let Some((count_key, list_key)) = heading_property_keys(&label) {
                    fields.push((count_key.into(), "uint", count.to_string()));
                    fields.push((list_key.into(), "array", String::new()));
                    fields.extend(
                        titles[offset.min(titles.len())..end.min(titles.len())]
                            .iter()
                            .map(|value| (list_key.into(), "item", value.clone())),
                    );
                }
                offset = end;
            }
        }
    }
    for (id, value) in entries() {
        let name = if custom {
            custom_names
                .binary_search_by_key(&id, |(id, _)| *id)
                .ok()
                .map(|index| custom_names[index].1.as_str())
        } else {
            match id {
                0x8000_0000 => Some("Locale"),
                0x8000_0003 => Some("Behavior"),
                _ => names
                    .get(id as usize)
                    .copied()
                    .filter(|name| !name.is_empty()),
            }
        };
        if let (Some(name), Some((kind, text))) = (name, scalar(value, codepage, custom)) {
            if !custom && name == "AppVersion" && kind == "i4" {
                if let Ok(version) = text.parse::<i32>() {
                    let minor = format!("{:04}", version & 0xffff);
                    fields.push((
                        name.into(),
                        "string",
                        format!("{}.{}", version >> 16, &minor[minor.len() - 4..]),
                    ));
                }
            } else if !custom && name == "CodePage" {
                fields.push((name.into(), kind, codepage.to_string()));
            } else {
                fields.push((name.into(), kind, text));
            }
        }
    }
    Some(())
}

/// Reads property-set values, heading lists and property-set identifiers.
pub fn biff_document_properties(
    bytes: &[u8],
    document_summary: bool,
) -> Option<Vec<(String, &'static str, String)>> {
    if u16_at(bytes, 0)? != 0xfffe {
        return None;
    }
    let count = u32_at(bytes, 24)? as usize;
    if count == 0 || count > 2 {
        return None;
    }
    let mut fields = vec![(
        "SystemIdentifier".into(),
        "uint",
        u32_at(bytes, 4)?.to_string(),
    )];
    let names: &[&str] = if document_summary {
        &DOCUMENT_SUMMARY
    } else {
        &SUMMARY
    };
    let mut fmtids = Vec::with_capacity(count);
    for index in 0..count {
        let descriptor = 28 + index * 20;
        let id = bytes.get(descriptor..descriptor + 16)?;
        let offset = u32_at(bytes, descriptor + 16)? as usize;
        if offset < 28 + count * 20 {
            return None;
        }
        fmtids.push(
            id.iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>(),
        );
        read_section(
            bytes.get(offset..)?,
            names,
            index != 0,
            document_summary,
            &mut fields,
        )?;
    }
    if let [id] = fmtids.as_slice() {
        fields.push(("FMTID".into(), "string", id.clone()));
    } else {
        fields.push(("FMTID".into(), "array", String::new()));
        fields.extend(fmtids.into_iter().map(|id| ("FMTID".into(), "item", id)));
    }
    Some(fields)
}

fn put_u32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend(value.to_le_bytes());
}

fn put_unicode(bytes: &mut Vec<u8>, text: &str) -> Option<()> {
    let count = u32::try_from(text.encode_utf16().count())
        .ok()?
        .checked_add(1)?;
    put_u32(bytes, count);
    for unit in text.encode_utf16().chain(std::iter::once(0)) {
        bytes.extend(unit.to_le_bytes());
    }
    if count % 2 != 0 {
        bytes.extend([0, 0]);
    }
    Some(())
}

fn encode_scalar(kind: &str, text: &str) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    match kind {
        "lpwstr" => {
            put_u32(&mut bytes, 31);
            put_unicode(&mut bytes, text)?;
        }
        "bool" => {
            put_u32(&mut bytes, 11);
            put_u32(&mut bytes, u32::from(text == "true"));
        }
        "i4" => {
            put_u32(&mut bytes, 3);
            bytes.extend(text.parse::<i32>().ok()?.to_le_bytes());
        }
        "r8" => {
            put_u32(&mut bytes, 5);
            bytes.extend(text.parse::<f64>().ok()?.to_le_bytes());
        }
        "filetime" => {
            put_u32(&mut bytes, 64);
            let ticks = (text.parse::<i128>().ok()?.checked_add(11_644_473_600_000)?)
                .checked_mul(10_000)?;
            bytes.extend(u64::try_from(ticks).ok()?.to_le_bytes());
        }
        _ => return None,
    }
    Some(bytes)
}

fn encode_section(mut values: Vec<(u32, Vec<u8>)>) -> Option<Vec<u8>> {
    values.push((1, [2_u32.to_le_bytes(), 1200_u32.to_le_bytes()].concat()));
    values.sort_unstable_by_key(|(id, _)| *id);
    let count = u32::try_from(values.len()).ok()?;
    let mut offset = count.checked_mul(8)?.checked_add(8)?;
    let mut table = Vec::new();
    let mut body = Vec::new();
    for (id, bytes) in values {
        put_u32(&mut table, id);
        put_u32(&mut table, offset);
        offset = offset.checked_add(u32::try_from(bytes.len()).ok()?)?;
        body.extend(bytes);
    }
    let mut section = Vec::new();
    put_u32(&mut section, offset);
    put_u32(&mut section, count);
    section.extend(table);
    section.extend(body);
    Some(section)
}

fn encode_stream(sections: &[([u8; 16], Vec<u8>)]) -> Option<Vec<u8>> {
    let count = u32::try_from(sections.len()).ok()?;
    let mut stream = vec![0xfe, 0xff, 0, 0];
    // The origin operating system is unspecified.
    stream.extend([0; 20]);
    put_u32(&mut stream, count);
    let mut offset = 28 + count * 20;
    for (id, bytes) in sections {
        stream.extend(id);
        put_u32(&mut stream, offset);
        offset = offset.checked_add(u32::try_from(bytes.len()).ok()?)?;
    }
    for (_, bytes) in sections {
        stream.extend(bytes);
    }
    Some(stream)
}

fn encode_heading_lists(lists: &[Vec<&str>; 3]) -> Option<Vec<(u32, Vec<u8>)>> {
    let count = lists.iter().filter(|list| !list.is_empty()).count();
    if count == 0 {
        return Some(Vec::new());
    }
    let mut headings = Vec::new();
    put_u32(&mut headings, 0x100c);
    put_u32(&mut headings, u32::try_from(count * 2).ok()?);
    let mut titles = Vec::new();
    put_u32(&mut titles, 0x101f);
    put_u32(
        &mut titles,
        u32::try_from(lists.iter().map(Vec::len).sum::<usize>()).ok()?,
    );
    for (label, list) in ["Worksheets", "Named Ranges", "Charts"]
        .into_iter()
        .zip(lists)
    {
        if list.is_empty() {
            continue;
        }
        put_u32(&mut headings, 31);
        put_unicode(&mut headings, label)?;
        put_u32(&mut headings, 3);
        put_u32(&mut headings, u32::try_from(list.len()).ok()?);
        for title in list {
            put_unicode(&mut titles, title)?;
        }
    }
    Some(vec![(12, headings), (13, titles)])
}

/// Encodes BIFF8 document properties into the two OLE property streams.
/// Filetime fields use signed Unix milliseconds.
pub fn encode_biff_document_properties(
    fields: &[(String, String, String)],
) -> Option<[Option<Vec<u8>>; 2]> {
    let mut summary = Vec::new();
    let mut document = Vec::new();
    let mut custom = Vec::new();
    let mut dictionary = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut lists = [Vec::new(), Vec::new(), Vec::new()];
    for (name, kind, value) in fields {
        if kind == "item" {
            let index = match name.as_str() {
                "SheetNames" => 0,
                "DefinedNames" => 1,
                "ChartNames" => 2,
                _ => return None,
            };
            lists[index].push(value.as_str());
            continue;
        }
        if name.is_empty() || !seen.insert(name) {
            return None;
        }
        if matches!(
            name.as_str(),
            "SystemIdentifier"
                | "CodePage"
                | "FMTID"
                | "Thumbnail"
                | "DigSig"
                | "Worksheets"
                | "SheetNames"
                | "NamedRanges"
                | "DefinedNames"
                | "Chartsheets"
                | "ChartNames"
                | "HeadingPairs"
                | "TitlesOfParts"
                | "_PID_LINKBASE"
                | "_PID_HLINKS"
        ) {
            continue;
        }
        let target = DOCUMENT_SUMMARY
            .iter()
            .position(|key| *key == name)
            .map(|index| (&mut document, index))
            .or_else(|| {
                SUMMARY
                    .iter()
                    .position(|key| *key == name)
                    .map(|index| (&mut summary, index))
            });
        if let Some((entries, id)) = target {
            let data = match name.as_str() {
                "AppVersion" if kind == "lpwstr" => {
                    let (major, minor) = value.split_once('.').unwrap_or((value, "0"));
                    let version = (i32::from(major.parse::<i16>().ok()?) << 16)
                        | i32::from(minor.parse::<u16>().ok()?);
                    encode_scalar("i4", &version.to_string())?
                }
                "EditTime" | "LastPrinted" | "CreatedDate" | "ModifiedDate" => {
                    encode_scalar("filetime", value)?
                }
                "ByteCount"
                | "LineCount"
                | "ParagraphCount"
                | "SlideCount"
                | "NoteCount"
                | "HiddenCount"
                | "MultimediaClipCount"
                | "CharacterCount"
                | "AppVersion"
                | "PageCount"
                | "WordCount"
                | "CharCount"
                | "DocSecurity" => encode_scalar("i4", value)?,
                "ScaleCrop" | "LinksUpToDate" | "SharedDoc" | "HyperlinksChanged" => {
                    encode_scalar("bool", value)?
                }
                _ => encode_scalar("lpwstr", value)?,
            };
            entries.push((id as u32, data));
        } else {
            let id = u32::try_from(custom.len()).ok()?.checked_add(2)?;
            put_u32(&mut dictionary, id);
            put_unicode(&mut dictionary, name)?;
            custom.push((id, encode_scalar(kind, value)?));
        }
    }
    document.extend(encode_heading_lists(&lists)?);
    let summary = if summary.is_empty() {
        None
    } else {
        Some(encode_stream(&[(SUMMARY_ID, encode_section(summary)?)])?)
    };
    let document = if document.is_empty() && custom.is_empty() {
        None
    } else {
        let mut sections = vec![(DOCUMENT_SUMMARY_ID, encode_section(document)?)];
        if !custom.is_empty() {
            let mut names = u32::try_from(custom.len()).ok()?.to_le_bytes().to_vec();
            names.extend(dictionary);
            custom.push((0, names));
            sections.push((CUSTOM_ID, encode_section(custom)?));
        }
        Some(encode_stream(&sections)?)
    };
    Some([summary, document])
}

const SUMMARY_ID: [u8; 16] = [
    0xe0, 0x85, 0x9f, 0xf2, 0xf9, 0x4f, 0x68, 0x10, 0xab, 0x91, 0x08, 0x00, 0x2b, 0x27, 0xb3, 0xd9,
];
const DOCUMENT_SUMMARY_ID: [u8; 16] = [
    0x02, 0xd5, 0xcd, 0xd5, 0x9c, 0x2e, 0x1b, 0x10, 0x93, 0x97, 0x08, 0x00, 0x2b, 0x2c, 0xf9, 0xae,
];
const CUSTOM_ID: [u8; 16] = [
    0x05, 0xd5, 0xcd, 0xd5, 0x9c, 0x2e, 0x1b, 0x10, 0x93, 0x97, 0x08, 0x00, 0x2b, 0x2c, 0xf9, 0xae,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_aligned_unicode_and_packed_ansi_heading_values() {
        let mut headings = [0x100c_u32, 4, 30, 11].map(u32::to_le_bytes).concat();
        headings.extend_from_slice(b"Worksheets\0");
        headings.extend([3_u32, 2, 31, 7].map(u32::to_le_bytes).concat());
        for unit in "Charts\0".encode_utf16() {
            headings.extend(unit.to_le_bytes());
        }
        headings.extend_from_slice(&[0, 0]);
        headings.extend([3_u32, 1].map(u32::to_le_bytes).concat());
        assert_eq!(
            read_headings(&headings, 1252),
            Some(vec![("Worksheets".into(), 2), ("Charts".into(), 1)])
        );
        assert_eq!(read_headings(&headings[..headings.len() - 1], 1252), None);

        let mut titles = [0x101e_u32, 2, 3].map(u32::to_le_bytes).concat();
        titles.extend_from_slice(b"A1\0");
        titles.extend_from_slice(&2_u32.to_le_bytes());
        titles.extend_from_slice(b"\xe9\0");
        assert_eq!(
            read_titles(&titles, 1252),
            Some(vec!["A1".into(), "é".into()])
        );
    }

    #[test]
    fn scalar_string_lengths_and_codepages() {
        let ansi = [30, 0, 0, 0, 2, 0, 0, 0, 0xe9, 0];
        assert_eq!(scalar(&ansi, 1252, false), Some(("string", "é".into())));
        assert_eq!(scalar(&ansi[..9], 1252, false), None);
        let unicode = [31, 0, 0, 0, 2, 0, 0, 0, 0x0d, 0x54, 0, 0];
        assert_eq!(scalar(&unicode, 1252, false), Some(("string", "名".into())));
        assert_eq!(scalar(&unicode[..11], 1252, false), None);
    }
}
