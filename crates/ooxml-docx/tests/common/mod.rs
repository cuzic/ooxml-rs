//! Shared test helpers. Fixtures are untrusted data: they are only read
//! through ooxml-rs, the `zip` crate and docx-rs, never executed.
#![allow(dead_code, reason = "each test binary uses a different subset")]

use std::io::{Cursor, Read, Write};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

pub const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../reference/fixtures/docx");

/// Every fixture (file stem).
pub const ALL: &[&str] = &[
    "poi-55733",
    "poi-61745",
    "poi-bug53475-password-is-pass",
    "poi-bug59058",
    "poi-Bug60341",
    "poi-checkboxes",
    "poi-delins",
    "poi-documentProtection_readonly_no_password",
    "poi-ExternalEntityInText",
    "poi-FieldCodes",
    "poi-FldSimple",
    "poi-Headers",
    "poi-PageSpecificHeadFoot",
    "poi-shapes-with-text",
    "poi-smarttag-snippet",
    "poi-table-alignment",
    "poi-testComment",
    "sdk-Comment010",
    "sdk-complex2010",
    "sdk-coverpage1",
    "sdk-equation",
    "sdk-Field-more-field",
    "sdk-ftEdNote",
    "sdk-HelloO14",
    "sdk-strict-CommentRangeStart",
    "sdk-TOC2",
];

/// Fixtures that fail to open (O1–O3).
pub const UNOPENABLE: &[&str] =
    &["poi-bug53475-password-is-pass", "poi-ExternalEntityInText", "sdk-strict-CommentRangeStart"];

/// Fixtures refused by a document-level gate (W1–W3).
pub const GATED: &[&str] = &["poi-delins", "poi-documentProtection_readonly_no_password", "sdk-TOC2", "sdk-complex2010"];

pub fn openable() -> impl Iterator<Item = &'static str> {
    ALL.iter().copied().filter(|f| !UNOPENABLE.contains(f))
}

/// Openable fixtures with no document-level gate.
pub fn writable() -> impl Iterator<Item = &'static str> {
    openable().filter(|f| !GATED.contains(f))
}

pub fn fixture(stem: &str) -> Vec<u8> {
    std::fs::read(format!("{DIR}/{stem}.docx")).unwrap_or_else(|e| panic!("{stem}: {e}"))
}

/// (name, method, CRC-32, compressed payload) of every zip entry.
pub fn entries(bytes: &[u8]) -> Vec<(String, CompressionMethod, u32, Vec<u8>)> {
    let mut z = ZipArchive::new(Cursor::new(bytes)).unwrap();
    (0..z.len())
        .map(|i| {
            let mut f = z.by_index_raw(i).unwrap();
            let mut payload = Vec::new();
            f.read_to_end(&mut payload).unwrap();
            (f.name().unwrap().into_owned(), f.compression(), f.crc32(), payload)
        })
        .collect()
}

/// Decompressed content of a part.
pub fn part(bytes: &[u8], name: &str) -> Vec<u8> {
    let mut z = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut f = z.by_name(name).unwrap();
    let mut out = Vec::new();
    f.read_to_end(&mut out).unwrap();
    out
}

pub fn part_names(bytes: &[u8]) -> Vec<String> {
    let z = ZipArchive::new(Cursor::new(bytes)).unwrap();
    z.file_names().map(|n| n.unwrap().to_string()).collect()
}

/// A copy of the package with `name` replaced by `content` (other entries
/// raw-copied).
pub fn with_part(bytes: &[u8], name: &str, content: &[u8]) -> Vec<u8> {
    let mut z = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut w = ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..z.len() {
        let f = z.by_index_raw(i).unwrap();
        if f.name().unwrap() == name {
            drop(f);
            w.start_file(name, SimpleFileOptions::default()).unwrap();
            w.write_all(content).unwrap();
        } else {
            w.raw_copy_file(f).unwrap();
        }
    }
    w.finish().unwrap().into_inner()
}

pub const W_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

/// A minimal package: `word/document.xml` with `body` as the body's inner
/// XML, optional `word/settings.xml` (inner XML of `w:settings`) and
/// optional `word/header1.xml` (inner XML of `w:hdr`).
pub fn docx(body: &str, settings: Option<&str>, header: Option<&str>) -> Vec<u8> {
    let mut w = ZipWriter::new(Cursor::new(Vec::new()));
    let o = SimpleFileOptions::default();
    let mut put = |name: &str, s: &str| {
        w.start_file(name, o).unwrap();
        w.write_all(s.as_bytes()).unwrap();
    };
    put(
        "[Content_Types].xml",
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/settings.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml"/><Override PartName="/word/header1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/></Types>"#,
    );
    put(
        "_rels/.rels",
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
    );
    put(
        "word/_rels/document.xml.rels",
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings" Target="settings.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml"/></Relationships>"#,
    );
    put(
        "word/document.xml",
        &format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="{W_NS}" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><w:body>{body}</w:body></w:document>"#
        ),
    );
    if let Some(s) = settings {
        put("word/settings.xml", &format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:settings xmlns:w="{W_NS}">{s}</w:settings>"#));
    }
    if let Some(h) = header {
        put("word/header1.xml", &format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:hdr xmlns:w="{W_NS}">{h}</w:hdr>"#));
    }
    w.finish().unwrap().into_inner()
}

/// Smallest changed region between `a` and `b`: `(start, end_in_a,
/// end_in_b)`, or `None` if equal.
pub fn diff_range(a: &[u8], b: &[u8]) -> Option<(usize, usize, usize)> {
    if a == b {
        return None;
    }
    let pre = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    let max_suf = a.len().min(b.len()) - pre;
    let suf = a.iter().rev().zip(b.iter().rev()).take(max_suf).take_while(|(x, y)| x == y).count();
    Some((pre, a.len() - suf, b.len() - suf))
}
