//! Minimal DOCX packages for unit tests that exercise private edit APIs.

use std::io::{Cursor, Write};

use zip::ZipWriter;
use zip::write::SimpleFileOptions;

use ooxml_schema::wml::NS_W;

pub(crate) fn docx(body: &str, header: Option<&str>) -> Vec<u8> {
    const OPC: &str = "http://schemas.openxmlformats.org/package/2006";
    const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
    const WML: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml";
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let mut put = |name, xml: String| {
        zip.start_file(name, SimpleFileOptions::default()).unwrap();
        zip.write_all(xml.as_bytes()).unwrap();
    };
    put(
        "[Content_Types].xml",
        format!(
            "<Types xmlns='{OPC}/content-types'><Default Extension='rels' ContentType='application/vnd.openxmlformats-package.relationships+xml'/><Default Extension='xml' ContentType='application/xml'/><Override PartName='/word/document.xml' ContentType='{WML}.document.main+xml'/><Override PartName='/word/settings.xml' ContentType='{WML}.settings+xml'/><Override PartName='/word/header.xml' ContentType='{WML}.header+xml'/></Types>"
        ),
    );
    put(
        "_rels/.rels",
        format!(
            "<Relationships xmlns='{OPC}/relationships'><Relationship Id='rId1' Type='{REL}/officeDocument' Target='word/document.xml'/></Relationships>"
        ),
    );
    put(
        "word/document.xml",
        format!("<w:document xmlns:w='{NS_W}'><w:body>{body}</w:body></w:document>"),
    );
    put(
        "word/settings.xml",
        format!("<w:settings xmlns:w='{NS_W}'/>"),
    );
    let header_rel = header
        .map(|_| format!("<Relationship Id='rId2' Type='{REL}/header' Target='header.xml'/>"))
        .unwrap_or_default();
    put(
        "word/_rels/document.xml.rels",
        format!(
            "<Relationships xmlns='{OPC}/relationships'><Relationship Id='rId1' Type='{REL}/settings' Target='settings.xml'/>{header_rel}</Relationships>"
        ),
    );
    if let Some(content) = header {
        put(
            "word/header.xml",
            format!("<w:hdr xmlns:w='{NS_W}'>{content}</w:hdr>"),
        );
    }
    zip.finish().unwrap().into_inner()
}
