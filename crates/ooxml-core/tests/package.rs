//! Layer 1 oracles: passthrough equality (name, method, CRC-32, compressed
//! bytes), parse-without-edit passthrough, same-path save, refusals.

use std::assert_matches;
use std::io::{Cursor, Read, Write};
use std::path::PathBuf;

use ooxml_core::{Error, Package, PartStateKind, QName};
use zip::ZipArchive;
use zip::write::SimpleFileOptions;

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../reference/round_trip_corpus/fixture_minimal.xlsx");
const SHEET1: &str = "xl/worksheets/sheet1.xml";
const SML: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";

/// (name, compression method, CRC-32, compressed payload) per entry.
fn entries(bytes: &[u8]) -> Vec<(String, zip::CompressionMethod, u32, Vec<u8>)> {
    let mut ar = ZipArchive::new(Cursor::new(bytes)).unwrap();
    (0..ar.len())
        .map(|i| {
            let mut f = ar.by_index_raw(i).unwrap();
            let mut raw = Vec::new();
            f.read_to_end(&mut raw).unwrap();
            (f.name().unwrap().into_owned(), f.compression(), f.crc32(), raw)
        })
        .collect()
}

fn decompressed(bytes: &[u8], name: &str) -> Vec<u8> {
    let mut ar = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut f = ar.by_name(name).unwrap();
    let mut v = Vec::new();
    f.read_to_end(&mut v).unwrap();
    v
}

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ooxml-core-test-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn unchanged_package_is_passed_through() {
    let input = std::fs::read(FIXTURE).unwrap();
    let pkg = Package::from_bytes(input.clone()).unwrap();
    assert!(pkg.part_names().all(|n| pkg.state(n) == Some(PartStateKind::Raw)));
    let out = pkg.to_bytes().unwrap();
    assert_eq!(entries(&out), entries(&input), "name, method, CRC and compressed bytes must match, in order");
}

#[test]
fn parsing_for_inspection_does_not_change_output() {
    let input = std::fs::read(FIXTURE).unwrap();
    let mut pkg = Package::from_bytes(input.clone()).unwrap();
    for name in ["xl/workbook.xml", "xl/sharedStrings.xml", SHEET1] {
        let doc = pkg.xml_part(name).unwrap();
        assert!(!doc.is_dirty());
        assert_eq!(pkg.state(name), Some(PartStateKind::Parsed { dirty: false }));
    }
    assert_eq!(entries(&pkg.to_bytes().unwrap()), entries(&input));
}

#[test]
fn only_the_replaced_part_changes() {
    let input = std::fs::read(FIXTURE).unwrap();
    let mut pkg = Package::from_bytes(input.clone()).unwrap();
    let core = b"<?xml version=\"1.0\"?><cp:coreProperties xmlns:cp=\"urn:x\"/>".to_vec();
    pkg.set_part("docProps/core.xml", core.clone()).unwrap();
    let out = pkg.to_bytes().unwrap();
    let (a, b) = (entries(&input), entries(&out));
    assert_eq!(a.len(), b.len());
    for (x, y) in a.iter().zip(&b) {
        assert_eq!(x.0, y.0, "order preserved");
        if x.0 == "docProps/core.xml" {
            assert_ne!(x.3, y.3);
        } else {
            assert_eq!(x, y, "{} must pass through", x.0);
        }
    }
    assert_eq!(decompressed(&out, "docProps/core.xml"), core);
    assert_matches!(pkg.set_part("xl/new.xml", b"<a/>".to_vec()), Err(Error::NewPartUnsupported(_)));
}

#[test]
fn edited_xml_part_is_reserialized_and_others_pass_through() {
    let input = std::fs::read(FIXTURE).unwrap();
    let mut pkg = Package::from_bytes(input.clone()).unwrap();
    // A generic edit: the root's own attribute (no schema needed for this).
    let doc = pkg.xml_part_mut(SHEET1).unwrap();
    let mut lens = doc.lens_mut::<Worksheet>().unwrap();
    lens.set_attr(QName::new("", "probe"), "1").unwrap();
    assert_eq!(pkg.state(SHEET1), Some(PartStateKind::Parsed { dirty: true }));
    let out = pkg.to_bytes().unwrap();
    for (x, y) in entries(&input).iter().zip(&entries(&out)) {
        if x.0 != SHEET1 {
            assert_eq!(x, y);
        }
    }
    let sheet = String::from_utf8(decompressed(&out, SHEET1)).unwrap();
    assert!(sheet.contains(r#" probe="1">"#), "{sheet}");
}

/// `Worksheet` stand-in: only the root QName matters for `lens_mut`.
enum Worksheet {}
impl ooxml_core::ElementSpec for Worksheet {
    const QNAME: QName = QName::new(SML, "worksheet");
    const SDK_NAME: &'static str = "test:worksheet";
    const ATTRS: &'static [ooxml_core::AttrSpec] = &[];
    const CONTENT: ooxml_core::Content = ooxml_core::Content::Unsupported(&[]);
}

#[test]
fn save_to_the_same_path_is_safe() {
    let dir = temp_dir("same-path");
    let path = dir.join("book.xlsx");
    std::fs::copy(FIXTURE, &path).unwrap();
    let mut pkg = Package::open(&path).unwrap();
    pkg.xml_part_mut(SHEET1).unwrap().lens_mut::<Worksheet>().unwrap().set_attr(QName::new("", "probe"), "2").unwrap();
    pkg.save(&path).unwrap();
    // no temp file left behind
    let names: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(names, ["book.xlsx"]);
    let saved = std::fs::read(&path).unwrap();
    let original = std::fs::read(FIXTURE).unwrap();
    for (x, y) in entries(&original).iter().zip(&entries(&saved)) {
        if x.0 != SHEET1 {
            assert_eq!(x, y, "{} must be copied from the in-memory source", x.0);
        }
    }
    let mut again = Package::open(&path).unwrap();
    let root = again.xml_part(SHEET1).unwrap().root();
    assert_eq!(root.attr(QName::new("", "probe")), Some("2"));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn opc_registries() {
    let pkg = Package::open(FIXTURE).unwrap();
    assert_eq!(pkg.main_document_part().as_deref(), Some("xl/workbook.xml"));
    assert_eq!(
        pkg.content_type(SHEET1),
        Some("application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml")
    );
    assert_eq!(pkg.content_type("_rels/.rels"), Some("application/vnd.openxmlformats-package.relationships+xml"));
    let rels = pkg.relationships("xl/workbook.xml").unwrap();
    let r1 = rels.by_id("rId1").unwrap();
    assert_eq!(rels.resolve(r1).as_deref(), Some(SHEET1));
    assert!(pkg.relationships(SHEET1).unwrap().iter().next().is_none());
    // the generated parts table knows the worksheet relationship
    let ws = ooxml_core::parts::by_relationship_type(&r1.rel_type).next().unwrap();
    assert_eq!(ws.name, "WorksheetPart");
    assert_eq!(ws.root_element, Some("worksheet"));
    assert_eq!(ws.root_namespace, Some(SML));
}

#[test]
fn cfb_is_reported_as_encrypted() {
    let mut cfb = vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
    cfb.resize(512, 0);
    assert_matches!(Package::from_bytes(cfb), Err(Error::EncryptedPackage));
}

#[test]
fn strict_packages_are_refused() {
    let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let o = SimpleFileOptions::default();
    zw.start_file("[Content_Types].xml", o).unwrap();
    zw.write_all(br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"/>"#).unwrap();
    zw.start_file("_rels/.rels", o).unwrap();
    zw.write_all(
        br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://purl.oclc.org/ooxml/officeDocument/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#,
    )
    .unwrap();
    let bytes = zw.finish().unwrap().into_inner();
    assert_matches!(Package::from_bytes(bytes), Err(Error::StrictNotSupported(_)));
}

/// The docx plan's XXE probe (Apache POI `ExternalEntityInText.docx`): the
/// package opens, the part with the DTD is refused, nothing is fetched, and
/// the untouched part still passes through.
#[test]
fn xxe_probe_part_is_refused() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../reference/fixtures/docx/poi-ExternalEntityInText.docx");
    let input = std::fs::read(path).unwrap();
    let mut pkg = Package::from_bytes(input.clone()).unwrap();
    assert_matches!(pkg.xml_part("word/document.xml"), Err(Error::DtdNotAllowed { .. }));
    assert_eq!(pkg.state("word/document.xml"), Some(PartStateKind::Raw));
    assert_eq!(entries(&pkg.to_bytes().unwrap()), entries(&input));
}
