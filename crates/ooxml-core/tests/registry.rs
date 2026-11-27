//! Layer 3 registry writes: `RegistryPlan` (add parts, `Default`s and
//! relationships, set content types, remove relationships, remove closed
//! sets of parts) and the one-step conveniences with their `check_*` twins.
//!
//! Every refusal must leave the package exactly as it was: same part
//! states, and `to_bytes` equal to the input entry by entry (name, method,
//! CRC-32, compressed bytes).

use std::assert_matches;
use std::io::{Cursor, Read, Write};

use ooxml_core::{
    Error, Package, PartNameRequest, PartStateKind, RegistryPlan, RegistryRefusal, RemoveRefusal, Target, XmlDocument,
};
use zip::ZipArchive;
use zip::write::SimpleFileOptions;

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../reference/round_trip_corpus/fixture_minimal.xlsx");
const WS_CT: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml";
const REL_WS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet";
const REL_DRAWING: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/drawing";
const REL_IMAGE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";
const REL_OTHER: &str = "urn:x-test:other";
const REL_CORE: &str = "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties";
const REL_HYPERLINK: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink";
const SHEET_XML: &[u8] = br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData/></worksheet>"#;

type Entry = (String, zip::CompressionMethod, u32, Vec<u8>);

fn entries(bytes: &[u8]) -> Vec<Entry> {
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

fn text(bytes: &[u8], name: &str) -> String {
    let mut ar = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut f = ar.by_name(name).unwrap_or_else(|e| panic!("{name}: {e}"));
    let mut v = String::new();
    f.read_to_string(&mut v).unwrap();
    v
}

/// A package from `(name, content)` entries, Deflated.
fn build(parts: &[(&str, &str)]) -> Vec<u8> {
    let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (n, c) in parts {
        zw.start_file(*n, SimpleFileOptions::default()).unwrap();
        zw.write_all(c.as_bytes()).unwrap();
    }
    zw.finish().unwrap().into_inner()
}

const CT_BASE: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
  <Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>
  <Override PartName="/xl/worksheets/sheet3.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>
  <Override PartName="/xl/drawings/drawing1.xml" ContentType="application/vnd.openxmlformats-officedocument.drawing+xml"/>
  <Override PartName="/xl/media/image1.png" ContentType="image/png"/>
</Types>"#;
const ROOT_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#;
/// The workbook names only sheet1 (rId1); its relationship rId3 to sheet3
/// is not referenced from workbook.xml.
const WORKBOOK: &str = r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="S1" sheetId="1" r:id="rId1"/></sheets></workbook>"#;
const WB_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>
  <Relationship Id="R44c83fbb" Type="urn:x-test:other" Target="worksheets/sheet1.xml"/>
  <Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet3.xml"/>
</Relationships>"#;
const SHEET3: &str = r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheetData/><drawing r:id="rId1"/></worksheet>"#;
const SHEET3_RELS: &str = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/drawing" Target="../drawings/drawing1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://example.com/" TargetMode="External"/></Relationships>"#;
const DRAWING: &str = r#"<xdr:wsDr xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><xdr:pic r:embed="rId1"/></xdr:wsDr>"#;
const DRAWING_RELS: &str = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/image1.png"/></Relationships>"#;

/// sheet1 (named in workbook.xml) and sheet3 (only related) with a drawing
/// and an image.
fn synthetic() -> Vec<u8> {
    build(&[
        ("[Content_Types].xml", CT_BASE),
        ("_rels/.rels", ROOT_RELS),
        ("xl/workbook.xml", WORKBOOK),
        ("xl/_rels/workbook.xml.rels", WB_RELS),
        ("xl/worksheets/sheet1.xml", std::str::from_utf8(SHEET_XML).unwrap()),
        ("xl/worksheets/sheet3.xml", SHEET3),
        ("xl/worksheets/_rels/sheet3.xml.rels", SHEET3_RELS),
        ("xl/drawings/drawing1.xml", DRAWING),
        ("xl/drawings/_rels/drawing1.xml.rels", DRAWING_RELS),
        ("xl/media/image1.png", "\u{89}PNG"),
    ])
}

fn states(pkg: &Package) -> Vec<(String, Option<PartStateKind>)> {
    pkg.part_names().map(|n| (n.to_owned(), pkg.state(n))).collect()
}

/// The refusal changed nothing: states and every saved entry.
fn assert_unchanged(pkg: &Package, input: &[u8], before: &[(String, Option<PartStateKind>)]) {
    assert_eq!(states(pkg), before, "part states changed");
    assert!(entries(&pkg.to_bytes().unwrap()) == entries(input), "saved entries changed");
}

fn refusal(e: Error) -> RegistryRefusal {
    match e {
        Error::RegistryRefused { reason, .. } => reason,
        e => panic!("not a registry refusal: {e}"),
    }
}

/// Entries equal except `changed`, which may differ, and `added`, which must
/// be appended at the end in this order.
fn assert_l1(input: &[u8], output: &[u8], changed: &[&str], added: &[&str]) {
    let (a, b) = (entries(input), entries(output));
    let names_b: Vec<&str> = b.iter().map(|e| e.0.as_str()).collect();
    let expected: Vec<&str> = a.iter().map(|e| e.0.as_str()).filter(|n| names_b.contains(n)).chain(added.iter().copied()).collect();
    assert_eq!(names_b, expected, "entry order");
    for x in &a {
        if let Some(y) = b.iter().find(|y| y.0 == x.0)
            && !changed.contains(&x.0.as_str())
        {
            assert!(x == y, "{} must pass through unchanged", x.0);
        }
    }
}

/// `out` is `orig` with exactly `inserted` placed before `</Root>`, after a
/// copy of the indentation of the last entry.
fn assert_appended(orig: &str, out: &str, close: &str) {
    let cut = orig.rfind(close).unwrap();
    assert!(out.starts_with(&orig[..cut]), "bytes before the insertion point changed:\n{out}");
    assert!(out.ends_with(&orig[cut..]), "bytes after the insertion point changed:\n{out}");
}

#[test]
fn add_part_registers_override_or_uses_default() {
    let input = std::fs::read(FIXTURE).unwrap();
    let mut pkg = Package::from_bytes(input.clone()).unwrap();
    let ct_before = text(&input, "[Content_Types].xml");
    let doc = XmlDocument::parse(SHEET_XML).unwrap();
    let name = pkg.add_part(PartNameRequest::Exact("/xl/worksheets/sheet2.xml".into()), WS_CT, doc).unwrap();
    assert_eq!(name, "xl/worksheets/sheet2.xml");
    // an XML part whose type the `xml` Default does not give: Override
    assert_eq!(pkg.content_type(&name), Some(WS_CT));
    // a part whose type a Default gives exactly: no Override
    pkg.add_part(PartNameRequest::Exact("customXml/item9.xml".into()), "application/xml", b"<a/>".as_slice()).unwrap();
    let out = pkg.to_bytes().unwrap();
    assert_l1(&input, &out, &["[Content_Types].xml"], &["xl/worksheets/sheet2.xml", "customXml/item9.xml"]);
    let ct = text(&out, "[Content_Types].xml");
    assert_appended(&ct_before, &ct, "</Types>");
    assert_eq!(ct.matches("<Override").count(), ct_before.matches("<Override").count() + 1);
    assert!(ct.contains(r#"<Override PartName="/xl/worksheets/sheet2.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>"#));
    assert_eq!(text(&out, "xl/worksheets/sheet2.xml").as_bytes(), SHEET_XML);
    let again = Package::from_bytes(out).unwrap();
    assert_eq!(again.content_type("xl/worksheets/sheet2.xml"), Some(WS_CT));
    assert_eq!(again.content_type("customXml/item9.xml"), Some("application/xml"));
}

#[test]
fn defaults_and_media() {
    let input = synthetic();
    let mut pkg = Package::from_bytes(input.clone()).unwrap();
    let mut plan = RegistryPlan::new(&pkg);
    // a Default for a new extension, then a part that it covers
    assert!(plan.add_default_content_type(&pkg, "jpeg", "image/jpeg").unwrap());
    assert!(!plan.add_default_content_type(&pkg, "JPEG", "image/jpeg").unwrap(), "same Default again: nothing to do");
    let img = plan.add_part(&pkg, PartNameRequest::Numbered("xl/media/image{}.jpeg".into()), "image/jpeg", vec![1u8, 2, 3]).unwrap();
    assert_eq!(img, "xl/media/image1.jpeg", "the template's own numbers only: image1.png is another template");
    assert_eq!(plan.content_type(&pkg, &img), Some("image/jpeg"));
    pkg.apply(plan).unwrap();
    let out = pkg.to_bytes().unwrap();
    let ct = text(&out, "[Content_Types].xml");
    assert_appended(CT_BASE, &ct, "</Types>");
    assert!(ct.contains(r#"<Default Extension="jpeg" ContentType="image/jpeg"/>"#));
    assert!(!ct.contains("/xl/media/image1.jpeg"), "covered by the Default: no Override");
    // refusals: a different Default; a Default that would retype a part
    let before = states(&pkg);
    let out2 = pkg.to_bytes().unwrap();
    assert_matches!(refusal(pkg.add_default_content_type("jpeg", "image/x-other").unwrap_err()), RegistryRefusal::DefaultExists { .. });
    let mut p2 = Package::from_bytes(build(&[
        ("[Content_Types].xml", CT_BASE),
        ("_rels/.rels", ROOT_RELS),
        ("xl/workbook.xml", WORKBOOK),
        ("xl/media/odd.bmp", "x"),
    ]))
    .unwrap();
    assert_matches!(refusal(p2.add_default_content_type("bmp", "image/bmp").unwrap_err()), RegistryRefusal::WouldRetypePart { .. });
    assert_matches!(refusal(p2.add_default_content_type("b.mp", "image/bmp").unwrap_err()), RegistryRefusal::InvalidExtension);
    assert_unchanged(&pkg, &out2, &before);
}

#[test]
fn numbered_names_take_max_plus_one() {
    let input = synthetic();
    let pkg = Package::from_bytes(input).unwrap();
    // sheet1 and sheet3 exist: max + 1, gaps are not filled
    assert_eq!(pkg.free_part_name(PartNameRequest::Numbered("xl/worksheets/sheet{}.xml".into())).unwrap(), "xl/worksheets/sheet4.xml");
    // case-insensitive template match
    assert_eq!(pkg.free_part_name(PartNameRequest::Numbered("XL/Worksheets/Sheet{}.XML".into())).unwrap(), "XL/Worksheets/Sheet4.XML");
    assert_eq!(pkg.free_part_name(PartNameRequest::Numbered("xl/charts/chart{}.xml".into())).unwrap(), "xl/charts/chart1.xml");
    assert_matches!(
        refusal(pkg.free_part_name(PartNameRequest::Numbered("xl/a{}b{}.xml".into())).unwrap_err()),
        RegistryRefusal::InvalidPartName { .. }
    );
    // an Override left for a missing part, and a dangling relationship
    // target, count as in use
    let ct = CT_BASE.replace("</Types>", r#"<Override PartName="/xl/worksheets/sheet7.xml" ContentType="application/xml"/></Types>"#);
    let rels = WB_RELS.replace("</Relationships>", r#"<Relationship Id="rId9" Type="urn:x" Target="worksheets/sheet9.xml"/></Relationships>"#);
    let pkg = Package::from_bytes(build(&[
        ("[Content_Types].xml", &ct),
        ("_rels/.rels", ROOT_RELS),
        ("xl/workbook.xml", WORKBOOK),
        ("xl/_rels/workbook.xml.rels", &rels),
        ("xl/worksheets/sheet1.xml", "<w/>"),
    ]))
    .unwrap();
    assert_eq!(pkg.free_part_name(PartNameRequest::Numbered("xl/worksheets/sheet{}.xml".into())).unwrap(), "xl/worksheets/sheet10.xml");
    assert!(pkg.part_name_in_use("xl/worksheets/sheet7.xml").is_some());
    assert!(pkg.part_name_in_use("XL/worksheets/SHEET9.xml").is_some());
    assert!(pkg.part_name_in_use("xl/worksheets/sheet8.xml").is_none());
}

#[test]
fn related_part_and_relationship_ids() {
    let input = synthetic();
    let mut pkg = Package::from_bytes(input.clone()).unwrap();
    let rels_before = text(&input, "xl/_rels/workbook.xml.rels");
    let checked = pkg
        .check_add_related_part(Some("xl/workbook.xml"), REL_WS, PartNameRequest::Numbered("xl/worksheets/sheet{}.xml".into()), WS_CT, SHEET_XML)
        .unwrap();
    assert_eq!(checked, ("xl/worksheets/sheet4.xml".to_owned(), "rId4".to_owned()), "max rIdN + 1; R44c83fbb ignored");
    assert_unchanged(&pkg, &input, &states(&Package::from_bytes(input.clone()).unwrap()));
    let got = pkg
        .add_related_part(Some("xl/workbook.xml"), REL_WS, PartNameRequest::Numbered("xl/worksheets/sheet{}.xml".into()), WS_CT, SHEET_XML)
        .unwrap();
    assert_eq!(got, checked);
    let out = pkg.to_bytes().unwrap();
    assert_l1(&input, &out, &["[Content_Types].xml", "xl/_rels/workbook.xml.rels"], &["xl/worksheets/sheet4.xml"]);
    let rels = text(&out, "xl/_rels/workbook.xml.rels");
    assert_appended(&rels_before, &rels, "</Relationships>");
    assert!(rels.contains(r#"<Relationship Id="rId4" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet4.xml"/>"#));
    let again = Package::from_bytes(out).unwrap();
    let r = again.relationships("xl/workbook.xml").unwrap();
    assert_eq!(r.resolve(r.by_id("rId4").unwrap()).as_deref(), Some("xl/worksheets/sheet4.xml"));
}

#[test]
fn plan_steps_see_earlier_steps() {
    let input = synthetic();
    let mut pkg = Package::from_bytes(input.clone()).unwrap();
    let mut plan = RegistryPlan::new(&pkg);
    let a = plan.add_part(&pkg, PartNameRequest::Numbered("xl/worksheets/sheet{}.xml".into()), WS_CT, SHEET_XML).unwrap();
    let b = plan.add_part(&pkg, PartNameRequest::Numbered("xl/worksheets/sheet{}.xml".into()), WS_CT, SHEET_XML).unwrap();
    assert_eq!((a.as_str(), b.as_str()), ("xl/worksheets/sheet4.xml", "xl/worksheets/sheet5.xml"));
    let id1 = plan.add_relationship(&pkg, Some("xl/workbook.xml"), REL_WS, Target::Part(a.clone())).unwrap();
    let id2 = plan.add_relationship(&pkg, Some("xl/workbook.xml"), REL_WS, Target::Part(b.clone())).unwrap();
    assert_eq!((id1.as_str(), id2.as_str()), ("rId4", "rId5"));
    // relationships from a part added in the same plan: its .rels is created
    let d1 = plan.add_relationship(&pkg, Some(&a), REL_DRAWING, Target::Part("xl/drawings/drawing1.xml".into())).unwrap();
    let d2 = plan.add_relationship(&pkg, Some(&a), REL_HYPERLINK, Target::External("https://example.com/x y".replace(' ', "%20"))).unwrap();
    assert_eq!((d1.as_str(), d2.as_str()), ("rId1", "rId2"));
    // a refused step leaves the plan as it was
    let err = plan.add_relationship(&pkg, Some(&a), REL_WS, Target::Part("xl/worksheets/nope.xml".into())).unwrap_err();
    assert_matches!(refusal(err), RegistryRefusal::TargetNotFound { .. });
    pkg.apply(plan).unwrap();
    let out = pkg.to_bytes().unwrap();
    assert_l1(
        &input,
        &out,
        &["[Content_Types].xml", "xl/_rels/workbook.xml.rels"],
        &["xl/worksheets/sheet4.xml", "xl/worksheets/sheet5.xml", "xl/worksheets/_rels/sheet4.xml.rels"],
    );
    let again = Package::from_bytes(out.clone()).unwrap();
    let r = again.relationships("xl/worksheets/sheet4.xml").unwrap();
    assert_eq!(r.resolve(r.by_id("rId1").unwrap()).as_deref(), Some("xl/drawings/drawing1.xml"));
    assert_eq!(r.by_id("rId2").unwrap().target, "https://example.com/x%20y");
    // the new .rels part is covered by the `rels` Default: no Override
    assert!(!text(&out, "[Content_Types].xml").contains("sheet4.xml.rels"));
}

#[test]
fn refusals_change_nothing() {
    let input = synthetic();
    let mut pkg = Package::from_bytes(input.clone()).unwrap();
    pkg.xml_part("xl/workbook.xml").unwrap(); // a Parsed, clean part must stay so
    let before = states(&pkg);
    let mut cases: Vec<(String, RegistryRefusal)> = Vec::new();
    let mut add = |pkg: &mut Package, name: &str, ct: &str, body: &[u8]| {
        let e = pkg.add_part(PartNameRequest::Exact(name.into()), ct, body).unwrap_err();
        cases.push((name.to_owned(), refusal(e)));
    };
    for bad in ["xl//a.xml", "xl/a.xml/", "xl/./a.xml", "xl/a%2Fb.xml", "xl/a b.xml", "xl/a%41.xml", "xl/a\\b.xml", "xl/a#b.xml"] {
        add(&mut pkg, bad, WS_CT, SHEET_XML);
    }
    add(&mut pkg, "XL/Worksheets/Sheet1.xml", WS_CT, SHEET_XML); // case
    add(&mut pkg, "xl/worksheets/sheet%31.xml", WS_CT, SHEET_XML); // percent-encoded '1' is refused as a name
    add(&mut pkg, "xl/workbook.xml/x.xml", WS_CT, SHEET_XML); // segment conflict
    add(&mut pkg, "xl/drawings/drawing1.xml", WS_CT, SHEET_XML);
    add(&mut pkg, "[Content_Types].xml", WS_CT, SHEET_XML);
    add(&mut pkg, "xl/_rels/x.xml.rels", WS_CT, SHEET_XML);
    add(&mut pkg, "xl/new.xml", "text", SHEET_XML);
    add(&mut pkg, "xl/new.xml", WS_CT, b"<not-closed>");
    for (name, r) in &cases {
        let ok = match r {
            RegistryRefusal::InvalidPartName { .. } => name.contains("//") || name.ends_with('/') || name.contains("/./") || name.contains('%') || name.contains(' ') || name.contains('\\') || name.contains('#'),
            RegistryRefusal::NameInUse { .. } => name.contains("heet1") || name.contains("workbook.xml/") || name.contains("drawing1"),
            RegistryRefusal::RegistryPart => name.contains("Content_Types") || name.contains("_rels"),
            RegistryRefusal::InvalidContentType | RegistryRefusal::MalformedXml { .. } => name == "xl/new.xml",
            _ => false,
        };
        assert!(ok, "{name}: unexpected {r:?}");
    }
    // relationships
    let e = |r: Result<String, Error>| refusal(r.unwrap_err());
    assert_matches!(e(pkg.add_relationship(Some("xl/nope.xml"), REL_WS, Target::Part("xl/workbook.xml".into()))), RegistryRefusal::PartNotFound);
    assert_matches!(e(pkg.add_relationship(Some("xl/workbook.xml"), "worksheet", Target::Part("xl/worksheets/sheet1.xml".into()))), RegistryRefusal::InvalidRelationshipType);
    assert_matches!(e(pkg.add_relationship(Some("xl/workbook.xml"), REL_WS, Target::Part("xl/missing.xml".into()))), RegistryRefusal::TargetNotFound { .. });
    assert_matches!(e(pkg.add_relationship(Some("xl/workbook.xml"), REL_WS, Target::External("has space".into()))), RegistryRefusal::InvalidTarget);
    assert_matches!(e(pkg.add_relationship(Some("[Content_Types].xml"), REL_WS, Target::Part("xl/workbook.xml".into()))), RegistryRefusal::RegistryPart);
    // remove_relationship: still referenced by the source (sheet3 names rId1)
    assert_matches!(refusal(pkg.remove_relationship(Some("xl/worksheets/sheet3.xml"), "rId1", &[REL_DRAWING]).unwrap_err()), RegistryRefusal::StillReferenced { .. });
    assert_matches!(refusal(pkg.remove_relationship(Some("xl/worksheets/sheet3.xml"), "rId9", &[REL_DRAWING]).unwrap_err()), RegistryRefusal::RelationshipNotFound);
    // set_content_type
    assert_matches!(refusal(pkg.set_content_type("xl/nope.xml", WS_CT).unwrap_err()), RegistryRefusal::PartNotFound);
    assert_matches!(refusal(pkg.set_content_type("xl/workbook.xml", "nope").unwrap_err()), RegistryRefusal::InvalidContentType);
    assert_unchanged(&pkg, &input, &before);
}

#[test]
fn stale_overrides_leftover_rels_and_replaced_registries() {
    // an Override left for a missing part with another type
    let ct = CT_BASE.replace("</Types>", r#"<Override PartName="/xl/new.xml" ContentType="application/x-old"/></Types>"#);
    let input = build(&[
        ("[Content_Types].xml", &ct),
        ("_rels/.rels", ROOT_RELS),
        ("xl/workbook.xml", WORKBOOK),
        ("xl/_rels/workbook.xml.rels", WB_RELS),
        ("xl/worksheets/sheet1.xml", "<w/>"),
        // a .rels left for a part that does not exist (review of ADR-0009, M6)
        ("xl/worksheets/_rels/sheet2.xml.rels", r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"/>"#),
    ]);
    let mut pkg = Package::from_bytes(input.clone()).unwrap();
    let before = states(&pkg);
    assert_matches!(
        refusal(pkg.add_part(PartNameRequest::Exact("xl/new.xml".into()), WS_CT, SHEET_XML).unwrap_err()),
        RegistryRefusal::NameInUse { .. }
    );
    assert_matches!(
        refusal(pkg.add_part(PartNameRequest::Exact("xl/worksheets/sheet2.xml".into()), WS_CT, SHEET_XML).unwrap_err()),
        RegistryRefusal::NameInUse { .. }
    );
    // Numbered: sheet3 is a (dangling) relationship target, so max + 1 = 4
    assert_eq!(pkg.free_part_name(PartNameRequest::Numbered("xl/worksheets/sheet{}.xml".into())).unwrap(), "xl/worksheets/sheet4.xml");
    assert_unchanged(&pkg, &input, &before);
    // a replaced [Content_Types].xml cannot take an Override
    let mut pkg = Package::from_bytes(input.clone()).unwrap();
    pkg.set_part("[Content_Types].xml", ct.into_bytes()).unwrap();
    assert_matches!(
        refusal(pkg.add_part(PartNameRequest::Exact("xl/other.xml".into()), WS_CT, SHEET_XML).unwrap_err()),
        RegistryRefusal::RegistryNotEditable { .. }
    );
    // but an addition that needs no registry change still works
    pkg.add_part(PartNameRequest::Exact("xl/plain.xml".into()), "application/xml", b"<p/>".as_slice()).unwrap();
}

#[test]
fn set_content_type_and_remove_relationship() {
    let input = synthetic();
    let mut pkg = Package::from_bytes(input.clone()).unwrap();
    const MACRO: &str = "application/vnd.ms-excel.sheet.macroEnabled.main+xml";
    pkg.set_content_type("xl/workbook.xml", MACRO).unwrap();
    pkg.set_content_type("xl/workbook.xml", MACRO).unwrap(); // no-op
    assert_eq!(pkg.content_type("xl/workbook.xml"), Some(MACRO));
    // a part covered by a Default gets an Override
    pkg.set_content_type("xl/worksheets/sheet3.xml", WS_CT).unwrap();
    // remove a relationship nobody refers to
    pkg.remove_relationship(Some("xl/workbook.xml"), "R44c83fbb", &[REL_OTHER]).unwrap();
    let out = pkg.to_bytes().unwrap();
    assert_l1(&input, &out, &["[Content_Types].xml", "xl/_rels/workbook.xml.rels"], &[]);
    let ct = text(&out, "[Content_Types].xml");
    assert_eq!(ct, CT_BASE.replace("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml", MACRO));
    let rels = text(&out, "xl/_rels/workbook.xml.rels");
    assert_eq!(rels, WB_RELS.replace("\n  <Relationship Id=\"R44c83fbb\" Type=\"urn:x-test:other\" Target=\"worksheets/sheet1.xml\"/>", ""));
}

#[test]
fn remove_closed_set_with_own_relationships() {
    let input = synthetic();
    let mut pkg = Package::from_bytes(input.clone()).unwrap();
    // workbook.xml refers to sheet1's rId1: removing sheet1 is refused
    let before = states(&pkg);
    assert_matches!(
        refusal(pkg.remove_parts(&["xl/worksheets/sheet1.xml"], &[REL_WS, "urn:x-test:other"]).unwrap_err()),
        RegistryRefusal::Remove(RemoveRefusal::StillReferenced { .. })
    );
    // a type the caller did not allow
    assert_matches!(
        refusal(pkg.remove_parts(&["xl/worksheets/sheet3.xml"], &[]).unwrap_err()),
        RegistryRefusal::Remove(RemoveRefusal::RelationshipTypeNotAllowed { .. })
    );
    // the drawing alone: sheet3 still names rId1
    assert_matches!(
        refusal(pkg.remove_parts(&["xl/drawings/drawing1.xml"], &[REL_DRAWING]).unwrap_err()),
        RegistryRefusal::Remove(RemoveRefusal::StillReferenced { .. })
    );
    assert_unchanged(&pkg, &input, &before);
    // sheet3 + its drawing as one set: the reference inside the set does not block
    let checked = pkg.check_remove_parts(&["xl/worksheets/sheet3.xml", "xl/drawings/drawing1.xml"], &[REL_WS, REL_DRAWING]).unwrap();
    assert_unchanged(&pkg, &input, &before);
    let removed = pkg.remove_parts(&["xl/worksheets/sheet3.xml", "xl/drawings/drawing1.xml"], &[REL_WS, REL_DRAWING]).unwrap();
    assert_eq!(removed, checked);
    assert_eq!(
        removed.parts,
        ["xl/worksheets/sheet3.xml", "xl/drawings/drawing1.xml", "xl/worksheets/_rels/sheet3.xml.rels", "xl/drawings/_rels/drawing1.xml.rels"]
    );
    assert_eq!(removed.relationships, [("xl/_rels/workbook.xml.rels".to_owned(), "rId3".to_owned())]);
    assert_eq!(removed.overrides, 2);
    assert_eq!(removed.kept_targets, [("xl/drawings/drawing1.xml".to_owned(), "xl/media/image1.png".to_owned(), REL_IMAGE.to_owned())]);
    // relationships parts that were only scanned stay Raw (no parse leaks into the output state)
    assert_eq!(pkg.state("_rels/.rels"), Some(PartStateKind::Raw));
    // the image is an orphan now; remove it too
    assert!(pkg.incoming_relationships("xl/media/image1.png").unwrap().is_empty());
    pkg.remove_parts(&["xl/media/image1.png"], &[]).unwrap();
    let out = pkg.to_bytes().unwrap();
    assert_l1(&input, &out, &["[Content_Types].xml", "xl/_rels/workbook.xml.rels"], &[]);
    let names: Vec<String> = entries(&out).into_iter().map(|e| e.0).collect();
    assert_eq!(names, ["[Content_Types].xml", "_rels/.rels", "xl/workbook.xml", "xl/_rels/workbook.xml.rels", "xl/worksheets/sheet1.xml"]);
    let ct = text(&out, "[Content_Types].xml");
    assert!(!ct.contains("sheet3") && !ct.contains("drawing1") && !ct.contains("image1") && ct.contains("<Default Extension=\"xml\""));
    // a removed name is not reused by a numbered name
    assert_eq!(pkg.free_part_name(PartNameRequest::Numbered("xl/worksheets/sheet{}.xml".into())).unwrap(), "xl/worksheets/sheet4.xml");
}

#[test]
fn plan_mixing_adds_and_removes() {
    let input = synthetic();
    let mut pkg = Package::from_bytes(input.clone()).unwrap();
    let mut plan = RegistryPlan::new(&pkg);
    plan.remove_parts(&pkg, &["xl/worksheets/sheet3.xml", "xl/drawings/drawing1.xml"], &[REL_WS, REL_DRAWING]).unwrap();
    // the removed parts are gone in the plan's view
    assert_matches!(
        refusal(plan.add_relationship(&pkg, Some("xl/workbook.xml"), REL_WS, Target::Part("xl/worksheets/sheet3.xml".into())).unwrap_err()),
        RegistryRefusal::ConflictsWithPlan
    );
    // Exact may reuse a name removed in the same plan (review m3: a VBA
    // project replaced in one plan); Numbered never does (below)
    let mut replaced = plan.clone();
    assert_eq!(replaced.add_part(&pkg, PartNameRequest::Exact("xl/worksheets/sheet3.xml".into()), WS_CT, SHEET_XML).unwrap(), "xl/worksheets/sheet3.xml");
    let mut pkg3 = Package::from_bytes(input.clone()).unwrap();
    let mut plan3 = RegistryPlan::new(&pkg3);
    plan3.remove_parts(&pkg3, &["xl/worksheets/sheet3.xml", "xl/drawings/drawing1.xml"], &[REL_WS, REL_DRAWING]).unwrap();
    plan3.add_part(&pkg3, PartNameRequest::Exact("xl/worksheets/sheet3.xml".into()), WS_CT, SHEET_XML).unwrap();
    pkg3.apply(plan3).unwrap();
    let out3 = pkg3.to_bytes().unwrap();
    let again3 = Package::from_bytes(out3.clone()).unwrap();
    assert_eq!(again3.read_part("xl/worksheets/sheet3.xml").unwrap(), SHEET_XML, "the new content");
    assert_eq!(again3.content_type("xl/worksheets/sheet3.xml"), Some(WS_CT));
    assert!(!again3.contains("xl/worksheets/_rels/sheet3.xml.rels"), "the old part's relationships went with it");
    assert_eq!(text(&out3, "[Content_Types].xml").matches("/xl/worksheets/sheet3.xml").count(), 1, "one Override");
    drop(replaced);
    let n = plan.add_part(&pkg, PartNameRequest::Numbered("xl/worksheets/sheet{}.xml".into()), WS_CT, SHEET_XML).unwrap();
    assert_eq!(n, "xl/worksheets/sheet4.xml");
    let id = plan.add_relationship(&pkg, Some("xl/workbook.xml"), REL_WS, Target::Part(n.clone())).unwrap();
    assert_eq!(id, "rId4", "rId3 was removed in this plan; ids are never reused");
    // the other order: a relationship to a part, then removing the part
    let mut plan2 = RegistryPlan::new(&pkg);
    plan2.add_relationship(&pkg, Some("xl/workbook.xml"), REL_IMAGE, Target::Part("xl/media/image1.png".into())).unwrap();
    assert_matches!(refusal(plan2.remove_parts(&pkg, &["xl/media/image1.png"], &[REL_IMAGE]).unwrap_err()), RegistryRefusal::ConflictsWithPlan);
    drop(plan2);
    pkg.apply(plan).unwrap();
    let out = pkg.to_bytes().unwrap();
    let again = Package::from_bytes(out).unwrap();
    let r = again.relationships("xl/workbook.xml").unwrap();
    assert!(r.by_id("rId3").is_none());
    assert_eq!(r.resolve(r.by_id("rId4").unwrap()).as_deref(), Some("xl/worksheets/sheet4.xml"));
    assert_eq!(again.content_type("xl/worksheets/sheet4.xml"), Some(WS_CT));
    assert!(!again.contains("xl/worksheets/sheet3.xml"));
}

#[test]
fn stale_plans_are_refused() {
    let input = synthetic();
    let mut pkg = Package::from_bytes(input.clone()).unwrap();
    let mut plan = RegistryPlan::new(&pkg);
    plan.add_part(&pkg, PartNameRequest::Exact("xl/a.xml".into()), WS_CT, SHEET_XML).unwrap();
    let _ = pkg.xml_part_mut("xl/workbook.xml").unwrap();
    let before = states(&pkg);
    assert_matches!(refusal(pkg.apply(plan).unwrap_err()), RegistryRefusal::StalePlan);
    assert_eq!(states(&pkg), before);
    // a plan for another package
    let other = Package::from_bytes(input.clone()).unwrap();
    let mut plan = RegistryPlan::new(&other);
    plan.add_part(&other, PartNameRequest::Exact("xl/a.xml".into()), WS_CT, SHEET_XML).unwrap();
    assert_matches!(refusal(pkg.apply(plan).unwrap_err()), RegistryRefusal::StalePlan);
    assert!(!pkg.contains("xl/a.xml"));
}

#[test]
fn fork_preserves_edits_but_keeps_registry_plans_local() {
    let mut original = Package::from_bytes(synthetic()).unwrap();
    original.add_part(
        PartNameRequest::Exact("xl/previous.xml".into()), WS_CT, SHEET_XML,
    ).unwrap();
    let baseline = original.to_bytes().unwrap();
    let mut candidate = original.fork();
    assert_eq!(candidate.to_bytes().unwrap(), baseline);
    let mut plan = RegistryPlan::new(&original);
    plan.add_part(&original, PartNameRequest::Exact("xl/unrelated.xml".into()), WS_CT, SHEET_XML).unwrap();
    assert_matches!(refusal(candidate.apply(plan).unwrap_err()), RegistryRefusal::StalePlan);
    candidate.add_part(
        PartNameRequest::Exact("xl/local.xml".into()), WS_CT, SHEET_XML,
    ).unwrap();
    assert!(candidate.contains("xl/local.xml"));
    assert_eq!(original.to_bytes().unwrap(), baseline);
}

#[test]
fn package_relationships_cache_and_new_xml_parts_stay_editable() {
    let input = synthetic();
    let mut pkg = Package::from_bytes(input).unwrap();
    let id = pkg
        .add_relationship(None, "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties", Target::Part("xl/media/image1.png".into()))
        .unwrap();
    assert_eq!(id, "rId2");
    assert!(pkg.package_relationships().by_id("rId2").is_some(), "cache updated");
    pkg.remove_relationship(None, "rId2", &[REL_CORE]).unwrap();
    assert!(pkg.package_relationships().by_id("rId2").is_none());
    // a part added as a tree can be edited further, and is always written
    let name = pkg.add_part(PartNameRequest::Exact("xl/t.xml".into()), "application/xml", XmlDocument::parse(b"<t/>").unwrap()).unwrap();
    assert_eq!(pkg.state(&name), Some(PartStateKind::Parsed { dirty: false }));
    assert_eq!(text(&pkg.to_bytes().unwrap(), "xl/t.xml"), "<t/>");
}

#[test]
fn staged_content_is_validated_with_the_registry_edits() {
    let input = synthetic();
    let mut pkg = Package::from_bytes(input.clone()).unwrap();
    // removing sheet1 needs workbook.xml to stop naming rId1: stage both
    let mut plan = RegistryPlan::new(&pkg);
    let wb = WORKBOOK.replace(r#"<sheet name="S1" sheetId="1" r:id="rId1"/>"#, "");
    plan.stage_part(&pkg, "xl/workbook.xml", wb.clone().into_bytes()).unwrap();
    let removed = plan.remove_parts(&pkg, &["xl/worksheets/sheet1.xml"], &[REL_WS, "urn:x-test:other"]).unwrap();
    assert_eq!(removed.relationships.len(), 2, "rId1 and R44c83fbb");
    pkg.apply(plan).unwrap();
    let out = pkg.to_bytes().unwrap();
    assert_eq!(text(&out, "xl/workbook.xml"), wb);
    assert!(!Package::from_bytes(out).unwrap().contains("xl/worksheets/sheet1.xml"));

    // the other order: content staged after a removal may not bring the Id back
    let pkg = Package::from_bytes(input.clone()).unwrap();
    let before = states(&pkg);
    let mut plan = RegistryPlan::new(&pkg);
    plan.remove_relationship(&pkg, Some("xl/workbook.xml"), "R44c83fbb", &[REL_OTHER]).unwrap();
    let wb = WORKBOOK.replace("<sheets>", r#"<x r:id="R44c83fbb"/><sheets>"#);
    assert_matches!(refusal(plan.stage_part(&pkg, "xl/workbook.xml", wb.into_bytes()).unwrap_err()), RegistryRefusal::ConflictsWithPlan);
    assert_matches!(refusal(plan.stage_part(&pkg, "xl/workbook.xml", b"<not".to_vec()).unwrap_err()), RegistryRefusal::MalformedXml { .. });
    assert_matches!(refusal(plan.stage_part(&pkg, "[Content_Types].xml", b"<x/>".to_vec()).unwrap_err()), RegistryRefusal::RegistryPart);
    drop(plan);
    assert_unchanged(&pkg, &input, &before);

    // a tree cloned from the parsed part keeps untouched bytes
    let mut pkg = Package::from_bytes(input.clone()).unwrap();
    let doc = pkg.xml_part("xl/worksheets/sheet3.xml").unwrap().clone();
    let mut plan = RegistryPlan::new(&pkg);
    plan.stage_part(&pkg, "xl/worksheets/sheet3.xml", doc).unwrap();
    pkg.apply(plan).unwrap();
    assert_eq!(text(&pkg.to_bytes().unwrap(), "xl/worksheets/sheet3.xml"), SHEET3);
}

#[test]
fn copy_relationships_keeps_ids() {
    let input = synthetic();
    let mut pkg = Package::from_bytes(input).unwrap();
    let mut plan = RegistryPlan::new(&pkg);
    let copy = plan.add_part(&pkg, PartNameRequest::Numbered("xl/worksheets/sheet{}.xml".into()), WS_CT, SHEET3.as_bytes()).unwrap();
    let drawing2 = plan
        .add_part(&pkg, PartNameRequest::Numbered("xl/drawings/drawing{}.xml".into()), "application/vnd.openxmlformats-officedocument.drawing+xml", DRAWING.as_bytes())
        .unwrap();
    assert_eq!(drawing2, "xl/drawings/drawing2.xml");
    let ids = plan.copy_relationships(&pkg, "xl/worksheets/sheet3.xml", &copy, &[("xl/drawings/drawing1.xml", &drawing2)], &[]).unwrap();
    assert_eq!(ids, ["rId1", "rId2"]);
    // a second copy into the same destination is refused
    assert_matches!(
        refusal(plan.copy_relationships(&pkg, "xl/worksheets/sheet3.xml", &copy, &[], &[]).unwrap_err()),
        RegistryRefusal::NameInUse { .. }
    );
    // the drawing's own image relationship, copied as well (shared image)
    plan.copy_relationships(&pkg, "xl/drawings/drawing1.xml", &drawing2, &[], &[]).unwrap();
    pkg.apply(plan).unwrap();
    let again = Package::from_bytes(pkg.to_bytes().unwrap()).unwrap();
    let r = again.relationships(&copy).unwrap();
    assert_eq!(r.resolve(r.by_id("rId1").unwrap()).as_deref(), Some("xl/drawings/drawing2.xml"));
    assert_eq!(r.by_id("rId2").unwrap().target, "https://example.com/");
    let r = again.relationships("xl/drawings/drawing2.xml").unwrap();
    assert_eq!(r.resolve(r.by_id("rId1").unwrap()).as_deref(), Some("xl/media/image1.png"));
    assert_eq!(again.incoming_relationships("xl/media/image1.png").unwrap().len(), 2);

    // Ids that are not rId1..rIdN are copied as they are
    let mut pkg = Package::from_bytes(synthetic()).unwrap();
    let mut plan = RegistryPlan::new(&pkg);
    let wb2 = plan.add_part(&pkg, PartNameRequest::Exact("xl/workbook2.xml".into()), "application/xml", WORKBOOK.as_bytes()).unwrap();
    let ids = plan.copy_relationships(&pkg, "xl/workbook.xml", &wb2, &[], &[]).unwrap();
    assert_eq!(ids, ["rId1", "R44c83fbb", "rId3"]);
    pkg.apply(plan).unwrap();
    let r = Package::from_bytes(pkg.to_bytes().unwrap()).unwrap().relationships("xl/workbook2.xml").unwrap();
    assert_eq!(r.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), ["rId1", "R44c83fbb", "rId3"]);
    assert_eq!(r.resolve(r.by_id("rId3").unwrap()).as_deref(), Some("xl/worksheets/sheet3.xml"));
}

#[test]
fn relationship_ids_stay_above_removed_ones_in_the_session() {
    let input = synthetic();
    let mut pkg = Package::from_bytes(input).unwrap();
    // sheet3's relationship rId3 is the largest; remove sheet3, then add a sheet
    pkg.remove_parts(&["xl/worksheets/sheet3.xml", "xl/drawings/drawing1.xml"], &[REL_WS, REL_DRAWING]).unwrap();
    let (_, id) = pkg
        .add_related_part(Some("xl/workbook.xml"), REL_WS, PartNameRequest::Numbered("xl/worksheets/sheet{}.xml".into()), WS_CT, SHEET_XML)
        .unwrap();
    assert_eq!(id, "rId4", "rId3 was removed in this session and is not reused");
    // also after remove_relationship in one plan and an add in the next
    pkg.remove_relationship(Some("xl/workbook.xml"), "rId4", &[REL_WS]).unwrap();
    let id = pkg.add_relationship(Some("xl/workbook.xml"), REL_IMAGE, Target::Part("xl/worksheets/sheet1.xml".into())).unwrap();
    assert_eq!(id, "rId5");
}
