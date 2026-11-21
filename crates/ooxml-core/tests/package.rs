//! Layer 1 oracles: passthrough equality (name, method, CRC-32, compressed
//! bytes), parse-without-edit passthrough, same-path save, refusals.

use std::assert_matches;
use std::io::{Cursor, Read, Write};
use std::path::PathBuf;

use ooxml_core::{Error, LeafStep, Package, PartStateKind, QName, RemoveRefusal};
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
fn leaf_text_refuses_opc_metadata_without_changing_package_bytes() {
    let input = std::fs::read(FIXTURE).unwrap();
    let mut pkg = Package::from_bytes(input.clone()).unwrap();
    for (part, ns, root, leaf) in [
        ("_rels/.rels", "http://schemas.openxmlformats.org/package/2006/relationships", "Relationships", "Relationship"),
        ("[Content_Types].xml", "http://schemas.openxmlformats.org/package/2006/content-types", "Types", "Default"),
    ] {
        let path = [LeafStep { ns, local: root, index: 0 }, LeafStep { ns, local: leaf, index: 0 }];
        let before = pkg.read_part(part).unwrap();
        let doc = pkg.xml_part_mut(part).unwrap();
        assert!(doc.root().elements().any(|child| child.name().local() == leaf));
        assert_matches!(doc.check_set_leaf_text(&path, "new"), Err(Error::TypedNamespace));
        assert_matches!(doc.set_leaf_text(&path, "new"), Err(Error::TypedNamespace));
        assert!(!doc.is_dirty());
        assert_eq!(pkg.read_part(part).unwrap(), before);
    }
    assert_eq!(pkg.to_bytes().unwrap(), input);
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

#[test]
fn parsed_part_reads_without_parsing() {
    let mut pkg = Package::open(FIXTURE).unwrap();
    assert!(pkg.parsed_part(SHEET1).is_none(), "raw parts are not parsed by the accessor");
    assert!(pkg.parsed_part("no/such/part.xml").is_none());
    let root_name = pkg.xml_part(SHEET1).unwrap().root().name().local().to_owned();
    assert_eq!(pkg.parsed_part(SHEET1).map(|d| d.root().name().local()), Some(root_name.as_str()));
    assert_eq!(pkg.state(SHEET1), Some(PartStateKind::Parsed { dirty: false }));
    pkg.set_part(SHEET1, b"<x/>".to_vec()).unwrap();
    assert!(pkg.parsed_part(SHEET1).is_none(), "replaced parts have no tree");
}

// ---- remove_part (ADR-0003 §4.3) ----

const SHARED_FORMULAS: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../reference/fixtures/xlsx/poi/shared_formulas.xlsx");

/// A small package with indented registries, a calcChain and a part that is
/// still referenced by `r:id`.
fn indented_package() -> Vec<u8> {
    let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let o = SimpleFileOptions::default();
    let files: [(&str, &str); 6] = [
        (
            "[Content_Types].xml",
            "<?xml version=\"1.0\"?>\n<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\n  <Default Extension=\"xml\" ContentType=\"application/xml\"/>\n  <Override PartName=\"/xl/workbook.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml\"/>\n  <Override PartName=\"/xl/calcChain.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.calcChain+xml\"/>\n  <Override PartName=\"/xl/linked.xml\" ContentType=\"application/xml\"/>\n</Types>\n",
        ),
        (
            "_rels/.rels",
            "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"xl/workbook.xml\"/></Relationships>",
        ),
        (
            "xl/_rels/workbook.xml.rels",
            "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\n  <Relationship Id=\"rId1\" Type=\"urn:t\" Target=\"linked.xml\"/>\n  <Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/calcChain\" Target=\"calcChain.xml\"/>\n  <Relationship Id=\"rId3\" Type=\"urn:x\" Target=\"http://example.com/calcChain.xml\" TargetMode=\"External\"/>\n</Relationships>",
        ),
        (
            "xl/workbook.xml",
            "<workbook xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><x r:id=\"rId1\"/><y id=\"rId22\"/></workbook>",
        ),
        ("xl/calcChain.xml", "<calcChain xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"/>"),
        ("xl/linked.xml", "<a/>"),
    ];
    for (n, b) in files {
        zw.start_file(n, o).unwrap();
        zw.write_all(b.as_bytes()).unwrap();
    }
    zw.finish().unwrap().into_inner()
}

const CALC_CHAIN: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/calcChain";
const CORE_PROPS: &str = "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties";
const CT_MIN: &str = r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="png" ContentType="image/png"/><Default Extension="vml" ContentType="application/vnd.openxmlformats-officedocument.vmlDrawing"/></Types>"#;
const PKG_RELS: &str = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/></Relationships>"#;

fn zip_of(files: &[(&str, &str)]) -> Vec<u8> {
    let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (n, b) in files {
        zw.start_file(*n, SimpleFileOptions::default()).unwrap();
        zw.write_all(b.as_bytes()).unwrap();
    }
    zw.finish().unwrap().into_inner()
}

fn states(pkg: &Package) -> Vec<(String, Option<PartStateKind>)> {
    pkg.part_names().map(|n| (n.to_owned(), pkg.state(n))).collect()
}

/// A refusal changes nothing: same part states, L1 output.
fn assert_refused(pkg: &mut Package, input: &[u8], name: &str, allowed: &[&str], want: impl Fn(&RemoveRefusal) -> bool) {
    let before = states(pkg);
    match pkg.check_remove_part(name, allowed) {
        Err(Error::RemovePartRefused { reason, .. }) => assert!(want(&reason), "check {name}: {reason:?}"),
        r => panic!("check {name}: {r:?}"),
    }
    match pkg.remove_part(name, allowed) {
        Err(Error::RemovePartRefused { reason, .. }) => assert!(want(&reason), "remove {name}: {reason:?}"),
        r => panic!("remove {name}: {r:?}"),
    }
    assert_eq!(states(pkg), before, "{name}: part states unchanged");
    assert_eq!(entries(&pkg.to_bytes().unwrap()), entries(input), "{name}: output unchanged");
}

#[test]
fn remove_part_drops_the_part_its_relationship_and_override_only() {
    let input = std::fs::read(SHARED_FORMULAS).unwrap();
    let mut pkg = Package::from_bytes(input.clone()).unwrap();
    let before = states(&pkg);
    pkg.check_remove_part("xl/calcChain.xml", &[CALC_CHAIN]).unwrap();
    assert_eq!(states(&pkg), before, "a check changes no state");
    let removed = pkg.remove_part("XL/calcchain.XML", &[CALC_CHAIN]).unwrap();
    assert_eq!(removed.relationships, vec![("xl/_rels/workbook.xml.rels".to_owned(), "rId5".to_owned())]);
    assert!(removed.content_type_override);
    assert!(!pkg.contains("xl/calcChain.xml"));
    assert_eq!(pkg.content_type("xl/calcChain.xml"), Some("application/xml"), "only the Default remains");
    assert!(pkg.relationships("xl/workbook.xml").unwrap().by_id("rId5").is_none());
    assert!(pkg.relationships("xl/workbook.xml").unwrap().by_id("rId4").is_some(), "no renumbering");

    let out = pkg.to_bytes().unwrap();
    let (a, b) = (entries(&input), entries(&out));
    let changed = ["[Content_Types].xml", "xl/_rels/workbook.xml.rels"];
    let expect: Vec<_> = a.iter().filter(|e| e.0 != "xl/calcChain.xml").collect();
    assert_eq!(b.len(), expect.len());
    for (x, y) in expect.iter().zip(&b) {
        assert_eq!(x.0, y.0, "order kept");
        if !changed.contains(&x.0.as_str()) {
            assert_eq!(*x, y, "{} must be raw-copied", x.0);
        }
    }
    let ct = String::from_utf8(decompressed(&input, "[Content_Types].xml")).unwrap();
    let override_el = r#"<Override PartName="/xl/calcChain.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.calcChain+xml"/>"#;
    assert_eq!(String::from_utf8(decompressed(&out, "[Content_Types].xml")).unwrap(), ct.replacen(override_el, "", 1));
    let rels = String::from_utf8(decompressed(&input, "xl/_rels/workbook.xml.rels")).unwrap();
    let rel_el = r#"<Relationship Id="rId5" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/calcChain" Target="calcChain.xml"/>"#;
    assert_eq!(String::from_utf8(decompressed(&out, "xl/_rels/workbook.xml.rels")).unwrap(), rels.replacen(rel_el, "", 1));
    let re = Package::from_bytes(out).unwrap();
    assert!(!re.contains("xl/calcChain.xml"));
}

#[test]
fn remove_part_takes_the_preceding_whitespace_with_it() {
    let input = indented_package();
    let mut pkg = Package::from_bytes(input.clone()).unwrap();
    pkg.remove_part("xl/calcChain.xml", &[CALC_CHAIN]).unwrap();
    let out = pkg.to_bytes().unwrap();
    let ct = String::from_utf8(decompressed(&out, "[Content_Types].xml")).unwrap();
    assert_eq!(
        ct,
        "<?xml version=\"1.0\"?>\n<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\n  <Default Extension=\"xml\" ContentType=\"application/xml\"/>\n  <Override PartName=\"/xl/workbook.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml\"/>\n  <Override PartName=\"/xl/linked.xml\" ContentType=\"application/xml\"/>\n</Types>\n"
    );
    let rels = String::from_utf8(decompressed(&out, "xl/_rels/workbook.xml.rels")).unwrap();
    assert_eq!(
        rels,
        "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\n  <Relationship Id=\"rId1\" Type=\"urn:t\" Target=\"linked.xml\"/>\n  <Relationship Id=\"rId3\" Type=\"urn:x\" Target=\"http://example.com/calcChain.xml\" TargetMode=\"External\"/>\n</Relationships>",
        "the external relationship with a similar target is kept"
    );
    assert_eq!(decompressed(&out, "_rels/.rels"), decompressed(&input, "_rels/.rels"));
}

#[test]
fn remove_part_refusals_change_nothing() {
    let input = indented_package();
    let mut pkg = Package::from_bytes(input.clone()).unwrap();
    // Still referenced by r:id in the source part (any attribute whose value
    // equals the Id counts; `id="rId22"` is not equal to either Id).
    assert_refused(&mut pkg, &input, "xl/linked.xml", &["urn:t"], |r| {
        matches!(r, RemoveRefusal::StillReferenced { source, id } if source == "xl/workbook.xml" && id == "rId1")
    });
    assert_matches!(pkg.remove_part("xl/missing.xml", &[]), Err(Error::PartNotFound(_)));
    assert_refused(&mut pkg, &input, "[Content_Types].xml", &[], |r| *r == RemoveRefusal::RegistryPart);
    assert_refused(&mut pkg, &input, "xl/_rels/workbook.xml.rels", &[], |r| *r == RemoveRefusal::RegistryPart);
    assert_refused(&mut pkg, &input, "xl/workbook.xml", &[], |r| matches!(r, RemoveRefusal::HasOwnRelationships { .. }));
    // m1: a type the caller did not allow (implicit references by type).
    assert_refused(&mut pkg, &input, "xl/calcChain.xml", &[], |r| matches!(r, RemoveRefusal::RelationshipTypeNotAllowed { rel_type, .. } if rel_type == CALC_CHAIN));
    // m2: a replaced registry that must change.
    let ct = decompressed(&input, "[Content_Types].xml");
    pkg.set_part("[Content_Types].xml", ct).unwrap();
    let replaced = pkg.to_bytes().unwrap();
    assert_refused(&mut pkg, &replaced, "xl/calcChain.xml", &[CALC_CHAIN], |r| matches!(r, RemoveRefusal::RegistryNotEditable { part, .. } if part == "[Content_Types].xml"));
    assert!(pkg.contains("xl/calcChain.xml"));
}

/// M1: VML `o:relid` and escaped values are references too.
#[test]
fn remove_part_sees_every_reference_form() {
    let vml = zip_of(&[
        ("[Content_Types].xml", CT_MIN),
        ("_rels/.rels", PKG_RELS),
        ("xl/workbook.xml", "<w/>"),
        ("docProps/core.xml", "<c/>"),
        ("xl/drawings/vmlDrawing1.vml", r#"<xml xmlns:v="urn:schemas-microsoft-com:vml" xmlns:o="urn:schemas-microsoft-com:office:office"><v:shape><v:imagedata o:relid="rId1" o:title=""/></v:shape></xml>"#),
        ("xl/drawings/_rels/vmlDrawing1.vml.rels", r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="urn:image" Target="../media/image1.png"/></Relationships>"#),
        ("xl/media/image1.png", "PNG"),
    ]);
    let mut pkg = Package::from_bytes(vml.clone()).unwrap();
    assert_refused(&mut pkg, &vml, "xl/media/image1.png", &["urn:image"], |r| matches!(r, RemoveRefusal::StillReferenced { id, .. } if id == "rId1"));
    for src in [
        r#"<w xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><x r:id="rId&#57;"/></w>"#,
        r#"<w xmlns:r="http://purl.oclc.org/ooxml/officeDocument/relationships"><x r:id="rId9"/></w>"#,
        r#"<w><x anything="rId9"/></w>"#,
    ] {
        let b = zip_of(&[
            ("[Content_Types].xml", CT_MIN),
            ("_rels/.rels", PKG_RELS),
            ("docProps/core.xml", "<c/>"),
            ("xl/workbook.xml", src),
            ("xl/_rels/workbook.xml.rels", r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId9" Type="urn:x" Target="linked.xml"/></Relationships>"#),
            ("xl/linked.xml", "<c/>"),
        ]);
        let mut pkg = Package::from_bytes(b.clone()).unwrap();
        assert_refused(&mut pkg, &b, "xl/linked.xml", &["urn:x"], |r| matches!(r, RemoveRefusal::StillReferenced { id, .. } if id == "rId9"));
    }
    // A source that is not XML cannot be shown reference-free.
    let b = zip_of(&[
        ("[Content_Types].xml", CT_MIN),
        ("_rels/.rels", PKG_RELS),
        ("docProps/core.xml", "<c/>"),
        ("xl/workbook.xml", "\u{1}binary<<"),
        ("xl/_rels/workbook.xml.rels", r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId9" Type="urn:x" Target="linked.xml"/></Relationships>"#),
        ("xl/linked.xml", "<c/>"),
    ]);
    let mut pkg = Package::from_bytes(b.clone()).unwrap();
    assert_refused(&mut pkg, &b, "xl/linked.xml", &["urn:x"], |r| matches!(r, RemoveRefusal::SourceNotScannable { .. }));
}

/// M2 + m6: `_rels/.rels` edited through a lens, then a removal from it:
/// no panic, and the package relationship cache is updated.
#[test]
fn remove_part_from_package_rels_after_a_lens_edit() {
    let b = zip_of(&[("[Content_Types].xml", CT_MIN), ("_rels/.rels", PKG_RELS), ("xl/workbook.xml", "<w/>"), ("docProps/core.xml", "<c/>")]);
    // plain case: the cache loses rId2
    let mut pkg = Package::from_bytes(b.clone()).unwrap();
    assert!(pkg.package_relationships().by_id("rId2").is_some());
    pkg.remove_part("docProps/core.xml", &[CORE_PROPS]).unwrap();
    assert!(pkg.package_relationships().by_id("rId2").is_none(), "package_rels cache updated");
    assert!(pkg.package_relationships().by_id("rId1").is_some());
    assert_eq!(pkg.main_document_part().as_deref(), Some("xl/workbook.xml"));
    let out = pkg.to_bytes().unwrap();
    assert!(!String::from_utf8(decompressed(&out, "_rels/.rels")).unwrap().contains("rId2"));
    // after a lens edit that leaves another Relationship without Type
    let mut pkg = Package::from_bytes(b).unwrap();
    {
        let d = pkg.xml_part_mut("_rels/.rels").unwrap();
        let mut root = d.lens_mut::<Rels>().unwrap();
        root.nth_child_mut::<Rel>(0).unwrap().remove_attr(QName::new("", "Type")).unwrap();
    }
    pkg.check_remove_part("docProps/core.xml", &[CORE_PROPS]).unwrap();
    pkg.remove_part("docProps/core.xml", &[CORE_PROPS]).unwrap();
    assert!(!pkg.contains("docProps/core.xml"));
}

/// m2/m3/m4/m5: an unrelated broken `.rels` does not block; case-insensitive
/// own `.rels`; every duplicate Override goes; several relationships to the
/// part in one and in several `.rels` parts all go.
#[test]
fn remove_part_edge_cases() {
    let b = zip_of(&[
        ("[Content_Types].xml", r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/calcChain.xml" ContentType="a/b"/><Override PartName="/XL/calcchain.xml" ContentType="a/c"/></Types>"#),
        ("_rels/.rels", PKG_RELS),
        ("xl/workbook.xml", "<w/>"),
        ("docProps/core.xml", "<c/>"),
        ("xl/_rels/workbook.xml.rels", r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId8" Type="urn:x" Target="calcChain.xml"/><Relationship Id="rId9" Type="urn:x" Target="/xl/calcChain.xml"/></Relationships>"#),
        ("docProps/_rels/core.xml.rels", r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="urn:x" Target="../xl/calcChain.xml"/></Relationships>"#),
        ("xl/calcChain.xml", "<c/>"),
        ("zz/_rels/orphan.xml.rels", "<notRels/>"),
        ("xl/foo.xml", "<f/>"),
        ("xl/_rels/FOO.xml.rels", r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"/>"#),
    ]);
    let mut pkg = Package::from_bytes(b.clone()).unwrap();
    assert_refused(&mut pkg, &b, "xl/foo.xml", &[], |r| matches!(r, RemoveRefusal::HasOwnRelationships { rels } if rels == "xl/_rels/FOO.xml.rels"));
    let removed = pkg.remove_part("xl/calcChain.xml", &["urn:x"]).unwrap();
    assert_eq!(removed.relationships.len(), 3);
    assert_eq!(pkg.content_type("xl/calcChain.xml"), Some("application/xml"), "both overrides gone (cache)");
    let out = pkg.to_bytes().unwrap();
    let ct = String::from_utf8(decompressed(&out, "[Content_Types].xml")).unwrap();
    assert!(!ct.to_ascii_lowercase().contains("calcchain"), "{ct}");
    for r in ["xl/_rels/workbook.xml.rels", "docProps/_rels/core.xml.rels"] {
        assert!(!String::from_utf8(decompressed(&out, r)).unwrap().contains("Relationship "), "{r}");
    }
    assert_eq!(decompressed(&out, "zz/_rels/orphan.xml.rels"), b"<notRels/>");
    // A broken .rels that names the part's file blocks (it might refer to it).
    let b2 = zip_of(&[
        ("[Content_Types].xml", CT_MIN),
        ("_rels/.rels", PKG_RELS),
        ("xl/workbook.xml", "<w/>"),
        ("docProps/core.xml", "<c/>"),
        ("xl/calcChain.xml", "<c/>"),
        ("zz/_rels/orphan.xml.rels", "<broken calcChain.xml"),
    ]);
    let mut pkg = Package::from_bytes(b2.clone()).unwrap();
    assert_refused(&mut pkg, &b2, "xl/calcChain.xml", &[CALC_CHAIN], |r| matches!(r, RemoveRefusal::RegistryNotEditable { part, .. } if part == "zz/_rels/orphan.xml.rels"));
}

const PR_NS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
enum Rels {}
enum Rel {}
impl ooxml_core::ElementSpec for Rels {
    const QNAME: QName = QName::new(PR_NS, "Relationships");
    const SDK_NAME: &'static str = "test:Relationships";
    const ATTRS: &'static [ooxml_core::AttrSpec] = &[];
    const CONTENT: ooxml_core::Content = ooxml_core::Content::Slots(&[ooxml_core::Slot {
        names: &[QName::new(PR_NS, "Relationship")],
        wildcard: None,
        max_each: None,
        max_total: None,
    }]);
}
impl ooxml_core::ElementSpec for Rel {
    const QNAME: QName = QName::new(PR_NS, "Relationship");
    const SDK_NAME: &'static str = "test:Relationship";
    const ATTRS: &'static [ooxml_core::AttrSpec] = &[];
    const CONTENT: ooxml_core::Content = ooxml_core::Content::Empty;
}
impl ooxml_core::ChildOf<Rels> for Rel {}

fn wb_rels_to(target: &str, extra: &str) -> String {
    format!(r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId2" Type="{CALC_CHAIN}" Target="{target}"{extra}/></Relationships>"#)
}

/// Round 2 N1: OPC part-name equivalence. A `_RELS` folder in another case
/// is a relationships folder; targets match after percent-decoding and ASCII
/// case folding. No relationship to the removed part survives an `Ok`.
#[test]
fn remove_part_uses_opc_part_name_equivalence() {
    for (rels, entry, target) in [
        ("xl/_rels/workbook.xml.rels", "xl/calcChain.xml", "calc%43hain.xml"),
        ("xl/_rels/workbook.xml.rels", "xl/calc%E7%94%BB.xml", "calc画.xml"),
        ("xl/_rels/workbook.xml.rels", "xl/calc画.xml", "calc%E7%94%BB.xml"),
        ("xl/_rels/workbook.xml.rels", "xl/calcChain.xml", "CALCCHAIN.xml"),
        ("xl/_RELS/workbook.xml.rels", "xl/calcChain.xml", "calcChain.xml"),
    ] {
        let r = wb_rels_to(target, "");
        let b = zip_of(&[("[Content_Types].xml", CT_MIN), ("_rels/.rels", PKG_RELS), ("xl/workbook.xml", "<w/>"), (rels, &r), (entry, "<c/>")]);
        let mut pkg = Package::from_bytes(b).unwrap();
        let removed = pkg.remove_part(entry, &[CALC_CHAIN]).unwrap();
        assert_eq!(removed.relationships, vec![(rels.to_owned(), "rId2".to_owned())], "{rels} {entry} {target}");
        let out = pkg.to_bytes().unwrap();
        assert!(!String::from_utf8(decompressed(&out, rels)).unwrap().contains("rId2"), "{entry} {target}");
    }
}

/// Round 2 N1/N4: an external relationship with the same target text is
/// never removed (mutant J).
#[test]
fn remove_part_keeps_external_relationships_with_the_same_target() {
    let r = format!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId2" Type="{CALC_CHAIN}" Target="calcChain.xml"/><Relationship Id="rId3" Type="urn:ext" Target="calcChain.xml" TargetMode="External"/></Relationships>"#
    );
    let b = zip_of(&[("[Content_Types].xml", CT_MIN), ("_rels/.rels", PKG_RELS), ("xl/workbook.xml", "<w/>"), ("xl/_rels/workbook.xml.rels", &r), ("xl/calcChain.xml", "<c/>")]);
    let mut pkg = Package::from_bytes(b).unwrap();
    let removed = pkg.remove_part("xl/calcChain.xml", &[CALC_CHAIN]).unwrap();
    assert_eq!(removed.relationships.len(), 1);
    let out = String::from_utf8(decompressed(&pkg.to_bytes().unwrap(), "xl/_rels/workbook.xml.rels")).unwrap();
    assert!(out.contains("rId3") && !out.contains("rId2"), "{out}");
}

/// Round 2 N4: a Replaced `.rels` that still holds a relationship to the
/// part is refused (mutant I: without the guard the part would go and the
/// relationship would stay); a source with a DTD is not scannable (mutant H).
#[test]
fn remove_part_guards_replaced_rels_and_dtd_sources() {
    let r = wb_rels_to("calcChain.xml", "");
    let b = zip_of(&[("[Content_Types].xml", CT_MIN), ("_rels/.rels", PKG_RELS), ("xl/workbook.xml", "<w/>"), ("xl/_rels/workbook.xml.rels", &r), ("xl/calcChain.xml", "<c/>")]);
    let mut pkg = Package::from_bytes(b).unwrap();
    pkg.set_part("xl/_rels/workbook.xml.rels", r.clone()).unwrap();
    let replaced = pkg.to_bytes().unwrap();
    assert_refused(&mut pkg, &replaced, "xl/calcChain.xml", &[CALC_CHAIN], |x| {
        matches!(x, RemoveRefusal::RegistryNotEditable { part, .. } if part == "xl/_rels/workbook.xml.rels")
    });
    let b = zip_of(&[
        ("[Content_Types].xml", CT_MIN),
        ("_rels/.rels", PKG_RELS),
        ("xl/workbook.xml", "<!DOCTYPE w><w/>"),
        ("xl/_rels/workbook.xml.rels", &r),
        ("xl/calcChain.xml", "<c/>"),
    ]);
    let mut pkg = Package::from_bytes(b.clone()).unwrap();
    assert_refused(&mut pkg, &b, "xl/calcChain.xml", &[CALC_CHAIN], |x| matches!(x, RemoveRefusal::SourceNotScannable { .. }));
}

/// Round 2 N2: only the root `_rels/.rels` holds package relationships; a
/// nested `foo/_rels/.rels` must not touch the package-relationships cache.
#[test]
fn remove_part_nested_dot_rels_is_not_the_package() {
    let b = zip_of(&[
        ("[Content_Types].xml", CT_MIN),
        ("_rels/.rels", PKG_RELS),
        ("xl/workbook.xml", "<w/>"),
        ("docProps/core.xml", "<c/>"),
        ("foo/_rels/.rels", r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="urn:x" Target="/x.xml"/></Relationships>"#),
        ("x.xml", "<x/>"),
    ]);
    let mut pkg = Package::from_bytes(b).unwrap();
    pkg.remove_part("x.xml", &["urn:x"]).unwrap();
    assert_eq!(pkg.main_document_part().as_deref(), Some("xl/workbook.xml"));
    assert_eq!(pkg.package_relationships().iter().count(), 2);
    let out = pkg.to_bytes().unwrap();
    assert!(!String::from_utf8(decompressed(&out, "foo/_rels/.rels")).unwrap().contains("rId1"));
}

/// Round 2 N5: Strict and Transitional calcChain types are distinct strings.
#[test]
fn calc_chain_relationship_types() {
    use ooxml_core::opc::{REL_CALC_CHAIN, REL_CALC_CHAIN_STRICT};
    assert_eq!(REL_CALC_CHAIN, CALC_CHAIN);
    let rs = format!(r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId2" Type="{REL_CALC_CHAIN_STRICT}" Target="calcChain.xml"/></Relationships>"#);
    let b = zip_of(&[("[Content_Types].xml", CT_MIN), ("_rels/.rels", PKG_RELS), ("xl/workbook.xml", "<w/>"), ("xl/_rels/workbook.xml.rels", &rs), ("xl/calcChain.xml", "<c/>")]);
    let pkg = Package::from_bytes(b).unwrap();
    assert_matches!(pkg.check_remove_part("xl/calcChain.xml", &[REL_CALC_CHAIN]), Err(Error::RemovePartRefused { reason: RemoveRefusal::RelationshipTypeNotAllowed { .. }, .. }));
    pkg.check_remove_part("xl/calcChain.xml", &[REL_CALC_CHAIN, REL_CALC_CHAIN_STRICT]).unwrap();
}

const IMAGE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";

fn sheet_with_image(sheet_entry: &str, sheet_rels: &str, target: &str, id: &str) -> Vec<u8> {
    let rels = format!(r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="{id}" Type="{IMAGE}" Target="{target}"/></Relationships>"#);
    zip_of(&[
        ("[Content_Types].xml", CT_MIN),
        ("_rels/.rels", PKG_RELS),
        ("xl/workbook.xml", "<w/>"),
        (sheet_entry, r#"<ws xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><pic r:embed="rId1"/></ws>"#),
        (sheet_rels, &rels),
        ("xl/media/i.png", "PNG"),
    ])
}

/// Round 3 N7 (a): the source part of a `.rels` is found under OPC
/// equivalence (percent-encoded entry vs IRI `.rels` name, both ways), so
/// its `r:embed` is seen; likewise a part's own `.rels` (R1, R2).
#[test]
fn remove_part_finds_sources_under_part_name_equivalence() {
    for (sheet, rels) in [("xl/sheet%E7%94%BB.xml", "xl/_rels/sheet画.xml.rels"), ("xl/sheet画.xml", "xl/_rels/sheet%E7%94%BB.xml.rels")] {
        let b = sheet_with_image(sheet, rels, "media/i.png", "rId1");
        let mut pkg = Package::from_bytes(b.clone()).unwrap();
        assert_refused(&mut pkg, &b, "xl/media/i.png", &[IMAGE], |r| matches!(r, RemoveRefusal::StillReferenced { id, .. } if id == "rId1"));
        assert_refused(&mut pkg, &b, sheet, &[], |r| matches!(r, RemoveRefusal::HasOwnRelationships { .. }));
    }
}

/// Round 3 N7 (b), (c): XML whitespace around `Target` and `Id` collapses
/// (xsd:anyURI, xsd:ID): such a relationship still points at the part, and
/// its Id is still referenced (R5, R10).
#[test]
fn remove_part_trims_target_and_id() {
    for (target, id) in [(" media/i.png", "rId1"), ("media/i.png&#10;", "rId1"), ("media/i.png", " rId1 ")] {
        let b = sheet_with_image("xl/sheet.xml", "xl/_rels/sheet.xml.rels", target, id);
        let mut pkg = Package::from_bytes(b.clone()).unwrap();
        assert_refused(&mut pkg, &b, "xl/media/i.png", &[IMAGE], |r| matches!(r, RemoveRefusal::StillReferenced { .. }));
    }
    for target in [" calcChain.xml", "calcChain.xml "] {
        let r = wb_rels_to(target, "");
        let b = zip_of(&[("[Content_Types].xml", CT_MIN), ("_rels/.rels", PKG_RELS), ("xl/workbook.xml", "<w/>"), ("xl/_rels/workbook.xml.rels", &r), ("xl/calcChain.xml", "<c/>")]);
        let mut pkg = Package::from_bytes(b).unwrap();
        assert_eq!(pkg.remove_part("xl/calcChain.xml", &[CALC_CHAIN]).unwrap().relationships.len(), 1, "{target:?}");
    }
}

/// Round 3 N7 safety net: a `.rels` that mentions the part's file name in a
/// spelling that does not resolve to it refuses the removal.
#[test]
fn remove_part_refuses_unresolved_mentions() {
    for target in ["xl\\calcChain.xml", "%2E/calcChain.xml"] {
        let r = wb_rels_to(target, "");
        let b = zip_of(&[("[Content_Types].xml", CT_MIN), ("_rels/.rels", PKG_RELS), ("xl/workbook.xml", "<w/>"), ("xl/_rels/workbook.xml.rels", &r), ("xl/calcChain.xml", "<c/>")]);
        let mut pkg = Package::from_bytes(b.clone()).unwrap();
        assert_refused(&mut pkg, &b, "xl/calcChain.xml", &[CALC_CHAIN], |x| matches!(x, RemoveRefusal::UnresolvedMention { .. }));
    }
}

/// Round 3 N8: entries equal under percent-decoding are a duplicate part
/// (refused at open); a percent-encoded Override is removed from the XML
/// and from the cache (R3, R4, mutant Y).
#[test]
fn percent_equivalent_names() {
    let b = zip_of(&[("[Content_Types].xml", CT_MIN), ("_rels/.rels", PKG_RELS), ("xl/a画.xml", "<a/>"), ("xl/a%E7%94%BB.xml", "<a/>")]);
    assert_matches!(Package::from_bytes(b), Err(Error::Package(_)));
    let ct = r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/c%E7%94%BB.xml" ContentType="urn:B"/></Types>"#;
    let b = zip_of(&[("[Content_Types].xml", ct), ("_rels/.rels", PKG_RELS), ("xl/workbook.xml", "<w/>"), ("xl/c画.xml", "<c/>")]);
    let mut pkg = Package::from_bytes(b).unwrap();
    let removed = pkg.remove_part("xl/c画.xml", &[]).unwrap();
    assert!(removed.content_type_override);
    assert!(pkg.content_types().overrides().is_empty(), "cache: {:?}", pkg.content_types().overrides());
    assert!(!String::from_utf8(decompressed(&pkg.to_bytes().unwrap(), "[Content_Types].xml")).unwrap().contains("Override"));
}

/// Round 3 N9 (mutant Z): a relative target in a nested `dir/_rels/.rels`
/// resolves against `dir`.
#[test]
fn nested_dot_rels_relative_target() {
    let b = zip_of(&[
        ("[Content_Types].xml", CT_MIN),
        ("_rels/.rels", PKG_RELS),
        ("xl/workbook.xml", "<w/>"),
        ("foo/_rels/.rels", r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="urn:x" Target="x.xml"/></Relationships>"#),
        ("foo/x.xml", "<x/>"),
    ]);
    let mut pkg = Package::from_bytes(b).unwrap();
    let removed = pkg.remove_part("foo/x.xml", &["urn:x"]).unwrap();
    assert_eq!(removed.relationships, vec![("foo/_rels/.rels".to_owned(), "rId1".to_owned())]);
}

fn calc_chain_pkg(rels: &str, extra: &[(&str, &str)]) -> Vec<u8> {
    let mut files = vec![
        ("[Content_Types].xml", CT_MIN),
        ("_rels/.rels", PKG_RELS),
        ("xl/workbook.xml", r#"<w xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><x r:id="rId3"/></w>"#),
        ("xl/_rels/workbook.xml.rels", rels),
        ("xl/calcChain.xml", "<c/>"),
    ];
    files.extend_from_slice(extra);
    zip_of(&files)
}

fn rels_of(items: &str) -> String {
    format!(r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{items}</Relationships>"#)
}

/// Round 4 N12: the safety net works per relationship on the parsed tree:
/// also when another relationship in the same `.rels` matches (S1), and for
/// file names written with character references (S2) or percent-encoding
/// (mutants AH, AK).
#[test]
fn safety_net_is_per_relationship() {
    for odd in [r#"xl\calcChain.xml"#, "%2E/calcChain.xml", r#"xl\calc&#67;hain.xml"#, "%2E/calc&#x43;hain.xml", r#"xl\calc%43hain.xml"#] {
        let r = rels_of(&format!(
            r#"<Relationship Id="rId2" Type="{CALC_CHAIN}" Target="calcChain.xml"/><Relationship Id="rId3" Type="{CALC_CHAIN}" Target="{odd}"/>"#
        ));
        let b = calc_chain_pkg(&r, &[]);
        let mut pkg = Package::from_bytes(b.clone()).unwrap();
        assert_refused(&mut pkg, &b, "xl/calcChain.xml", &[CALC_CHAIN], |x| matches!(x, RemoveRefusal::UnresolvedMention { .. }));
    }
    // a percent-encoded part name, spelled as an IRI with a backslash
    let r = rels_of(r#"<Relationship Id="rId9" Type="urn:x" Target="xl\c画.xml"/>"#);
    let b = zip_of(&[("[Content_Types].xml", CT_MIN), ("_rels/.rels", PKG_RELS), ("xl/workbook.xml", "<w/>"), ("xl/_rels/workbook.xml.rels", &r), ("xl/c%E7%94%BB.xml", "<c/>")]);
    let mut pkg = Package::from_bytes(b.clone()).unwrap();
    assert_refused(&mut pkg, &b, "xl/c%E7%94%BB.xml", &["urn:x"], |x| matches!(x, RemoveRefusal::UnresolvedMention { .. }));
}

/// Round 4 N13: no false refusals for another existing part with the same
/// file name, an external target, a type URI containing the name, or a
/// directory entry; calcChain is unaffected.
#[test]
fn safety_net_does_not_over_refuse() {
    let r = rels_of(&format!(
        r#"<Relationship Id="rId2" Type="{CALC_CHAIN}" Target="calcChain.xml"/><Relationship Id="rId4" Type="urn:x" Target="other/calcChain.xml"/><Relationship Id="rId5" Type="urn:x" Target="http://example.com/calcChain.xml" TargetMode="External"/>"#
    ));
    let b = calc_chain_pkg(&r, &[("xl/other/calcChain.xml", "<c/>"), ("zz/_rels/z.xml.rels", r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="urn:x" Target="http://example.com/calcChain.xml" TargetMode="External"/></Relationships>"#)]);
    let mut pkg = Package::from_bytes(b).unwrap();
    assert_eq!(pkg.remove_part("xl/calcChain.xml", &[CALC_CHAIN]).unwrap().relationships.len(), 1);
    // a type URI ending in the part's (extension-less) file name
    let r = rels_of(r#"<Relationship Id="rId7" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/a.png"/>"#);
    let b = zip_of(&[("[Content_Types].xml", CT_MIN), ("_rels/.rels", PKG_RELS), ("xl/workbook.xml", "<w/>"), ("xl/_rels/workbook.xml.rels", &r), ("xl/media/a.png", "P"), ("xl/x/image", "I")]);
    let mut pkg = Package::from_bytes(b).unwrap();
    pkg.remove_part("xl/x/image", &[]).unwrap();
    // a directory entry
    let b = zip_of(&[("[Content_Types].xml", CT_MIN), ("_rels/.rels", PKG_RELS), ("xl/workbook.xml", "<w/>"), ("xl/", "")]);
    let mut pkg = Package::from_bytes(b).unwrap();
    pkg.remove_part("xl/", &[]).unwrap();
}

/// Round 4 N14 / round 5 N17: `%5Frels` / `.rel%73` spellings (invalid OPC
/// names) are relationships parts after decoding; their source cannot be
/// attributed reliably, so one that mentions the part refuses the removal,
/// consistently with `HasOwnRelationships`.
#[test]
fn percent_encoded_rels_names_are_scanned() {
    for rels in ["xl/%5Frels/sheet.xml.rels", "xl/_rels/sheet.xml.rel%73"] {
        let b = sheet_with_image("xl/sheet.xml", rels, "media/i.png", "rId1");
        let mut pkg = Package::from_bytes(b.clone()).unwrap();
        assert_refused(&mut pkg, &b, "xl/media/i.png", &[IMAGE], |r| matches!(r, RemoveRefusal::RegistryNotEditable { .. }));
        assert_refused(&mut pkg, &b, "xl/sheet.xml", &[], |r| matches!(r, RemoveRefusal::HasOwnRelationships { .. }));
    }
}

/// Round 4 N15: whitespace around `TargetMode` (AI) and `Type` (AJ) is
/// collapsed; a padded `Id` in the package `.rels` leaves the cache (AD).
#[test]
fn padded_attributes() {
    let r = rels_of(&format!(
        r#"<Relationship Id="rId2" Type=" {CALC_CHAIN} " Target="calcChain.xml"/><Relationship Id="rId5" Type="urn:e" Target="calcChain.xml" TargetMode=" External "/>"#
    ));
    let b = calc_chain_pkg(&r, &[]);
    let mut pkg = Package::from_bytes(b).unwrap();
    let removed = pkg.remove_part("xl/calcChain.xml", &[CALC_CHAIN]).unwrap();
    assert_eq!(removed.relationships, vec![("xl/_rels/workbook.xml.rels".to_owned(), "rId2".to_owned())]);
    let pkg_rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/><Relationship Id=" rId2 " Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/></Relationships>"#;
    let b = zip_of(&[("[Content_Types].xml", CT_MIN), ("_rels/.rels", pkg_rels), ("xl/workbook.xml", "<w/>"), ("docProps/core.xml", "<c/>")]);
    let mut pkg = Package::from_bytes(b).unwrap();
    pkg.remove_part("docProps/core.xml", &[CORE_PROPS]).unwrap();
    assert_eq!(pkg.package_relationships().iter().count(), 1, "the padded Id left the cache");
}

/// Round 4 N11: the duplicate-name check at open is linear (small always-on
/// size; the 50k version below is ignored by default).
fn open_many(n: usize) -> std::time::Duration {
    let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let o = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, body) in [("[Content_Types].xml", CT_MIN), ("_rels/.rels", PKG_RELS)] {
        zw.start_file(name, o).unwrap();
        zw.write_all(body.as_bytes()).unwrap();
    }
    for i in 0..n {
        zw.start_file(format!("x/p{i}.xml"), o).unwrap();
    }
    let b = zw.finish().unwrap().into_inner();
    let t = std::time::Instant::now();
    Package::from_bytes(b).unwrap();
    t.elapsed()
}

#[test]
fn open_duplicate_check_is_linear_small() {
    let d = open_many(5_000);
    assert!(d < std::time::Duration::from_secs(5), "{d:?}");
}

#[test]
#[ignore = "timing: run with --release --ignored"]
fn open_duplicate_check_is_linear_50k() {
    let d = open_many(50_000);
    println!("open with 50,000 entries: {d:?}");
    assert!(d < std::time::Duration::from_secs(2), "{d:?}");
}


/// Round 5 N17: the source part and the base folder come from the raw
/// `.rels` name: `%25` (an encoded `%`) is decoded once, not twice.
#[test]
fn source_and_base_are_derived_from_the_raw_name() {
    // source xl/a%2541.xml (the character name `a%41.xml`) still refers to rId1
    let b = sheet_with_image("xl/a%2541.xml", "xl/_rels/a%2541.xml.rels", "media/i.png", "rId1");
    let mut pkg = Package::from_bytes(b.clone()).unwrap();
    assert_refused(&mut pkg, &b, "xl/media/i.png", &[IMAGE], |r| {
        matches!(r, RemoveRefusal::StillReferenced { source, id } if source == "xl/a%2541.xml" && id == "rId1")
    });
    // a folder x%2541/: the relative target resolves inside it and is removed
    let rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="urn:x" Target="media/i.png"/></Relationships>"#;
    let b = zip_of(&[
        ("[Content_Types].xml", CT_MIN),
        ("_rels/.rels", PKG_RELS),
        ("xl/workbook.xml", "<w/>"),
        ("x%2541/s.xml", "<s/>"),
        ("x%2541/_rels/s.xml.rels", rels),
        ("x%2541/media/i.png", "P"),
    ]);
    let mut pkg = Package::from_bytes(b).unwrap();
    let removed = pkg.remove_part("x%2541/media/i.png", &["urn:x"]).unwrap();
    assert_eq!(removed.relationships, vec![("x%2541/_rels/s.xml.rels".to_owned(), "rId1".to_owned())]);
}

/// Round 5 N18: a `.rels` whose source name contains `%2F` is still a
/// relationships part (recognised on the raw name) and its relationships are
/// scanned.
#[test]
fn rels_of_a_source_with_percent_2f_are_scanned() {
    let b = sheet_with_image("xl/a%2Fb.xml", "xl/_rels/a%2Fb.xml.rels", "media/i.png", "rId1");
    let mut pkg = Package::from_bytes(b.clone()).unwrap();
    assert_refused(&mut pkg, &b, "xl/media/i.png", &[IMAGE], |r| matches!(r, RemoveRefusal::StillReferenced { .. }));
}

/// Round 5 N19: the remaining mutant gaps (AH, AM, AP, AQ, AT).
#[test]
fn round5_gaps() {
    // AH: an unreadable .rels mentioning the part percent-encoded refuses
    let b = calc_chain_pkg(&rels_of(""), &[("zz/_rels/z.xml.rels", "<broken calc%43hain.xml")]);
    let mut pkg = Package::from_bytes(b.clone()).unwrap();
    assert_refused(&mut pkg, &b, "xl/calcChain.xml", &[CALC_CHAIN], |x| matches!(x, RemoveRefusal::RegistryNotEditable { .. }));
    // AM: `%+1` is not an escape, so these two names are distinct parts
    let b = zip_of(&[("[Content_Types].xml", CT_MIN), ("_rels/.rels", PKG_RELS), ("x/a%+1.xml", "<a/>"), ("x/a%01.xml", "<a/>")]);
    Package::from_bytes(b).unwrap();
    // AP: a directory entry is not refused by a target ending in `/`
    let r = rels_of(r#"<Relationship Id="rId9" Type="urn:x" Target="other/"/>"#);
    let b = zip_of(&[("[Content_Types].xml", CT_MIN), ("_rels/.rels", PKG_RELS), ("xl/workbook.xml", "<w/>"), ("xl/_rels/workbook.xml.rels", &r), ("xl/sub/", "")]);
    Package::from_bytes(b).unwrap().remove_part("xl/sub/", &[]).unwrap();
    // AQ: nor by an unreadable .rels
    let b = zip_of(&[("[Content_Types].xml", CT_MIN), ("_rels/.rels", PKG_RELS), ("xl/workbook.xml", "<w/>"), ("zz/_rels/z.xml.rels", "<broken"), ("xl/sub/", "")]);
    Package::from_bytes(b).unwrap().remove_part("xl/sub/", &[]).unwrap();
    // AT: a `%5Frels` relationships part is itself a registry part
    let b = zip_of(&[("[Content_Types].xml", CT_MIN), ("_rels/.rels", PKG_RELS), ("xl/workbook.xml", "<w/>"), ("xl/%5Frels/x.xml.rels", "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"/>")]);
    let mut pkg = Package::from_bytes(b.clone()).unwrap();
    assert_refused(&mut pkg, &b, "xl/%5Frels/x.xml.rels", &[], |x| *x == RemoveRefusal::RegistryPart);
}
