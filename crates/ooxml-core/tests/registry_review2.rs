//! Regression tests for the round-2 review of PR #38 (N1, N2, n3–n5, and
//! the surviving mutants N28, N29, N37).

use std::assert_matches;
use std::io::{Cursor, Read, Write};

use ooxml_core::{Error, Package, PartNameRequest, RegistryPlan, RegistryRefusal, RemoveRefusal, Target, XmlDocument};
use zip::ZipArchive;
use zip::write::SimpleFileOptions;

fn build(parts: &[(&str, &str)]) -> Vec<u8> {
    let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (n, c) in parts {
        zw.start_file(*n, SimpleFileOptions::default()).unwrap();
        zw.write_all(c.as_bytes()).unwrap();
    }
    zw.finish().unwrap().into_inner()
}

fn text(bytes: &[u8], name: &str) -> String {
    let mut ar = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut f = ar.by_name(name).unwrap_or_else(|e| panic!("{name}: {e}"));
    let mut v = String::new();
    f.read_to_string(&mut v).unwrap();
    v
}

fn refusal(e: Error) -> RegistryRefusal {
    match e {
        Error::RegistryRefused { reason, .. } => reason,
        e => panic!("not a registry refusal: {e:?}"),
    }
}

const CT: &str = r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="png" ContentType="image/png"/></Types>"#;
const ROOT: &str = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#;
const IMG: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";
const WB: &str = r#"<workbook xmlns="urn:x" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"/>"#;
const EMBED: &str = r#"<s xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><pic r:embed="rId1"/></s>"#;
const PLAIN: &str = r#"<s xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"/>"#;

fn rels(rows: &[(&str, &str, &str)]) -> String {
    let mut s = String::from(r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#);
    for (id, ty, t) in rows {
        let ext = if t.starts_with("http") { r#" TargetMode="External""# } else { "" };
        s.push_str(&format!(r#"<Relationship Id="{id}" Type="{ty}" Target="{t}"{ext}/>"#));
    }
    s.push_str("</Relationships>");
    s
}

fn base(extra: &[(&str, &str)]) -> Vec<u8> {
    let mut v = vec![("[Content_Types].xml", CT), ("_rels/.rels", ROOT), ("xl/workbook.xml", WB)];
    v.extend_from_slice(extra);
    build(&v)
}

fn embed_pkg() -> Package {
    let r = rels(&[("rId1", IMG, "media/i.png")]);
    Package::from_bytes(base(&[("xl/s.xml", EMBED), ("xl/_rels/s.xml.rels", &r), ("xl/media/i.png", "PNG")])).unwrap()
}

/// N1: content re-parsed from a part changed by `set_part` / `xml_part_mut`
/// and staged is what is saved (validated content == saved content).
#[test]
fn n1_restaged_changed_part_is_saved() {
    // the reviewer's spoof: set_part, then stage a re-parse of read_part
    let mut pkg = embed_pkg();
    pkg.set_part("xl/s.xml", PLAIN.as_bytes().to_vec()).unwrap();
    let mut plan = RegistryPlan::new(&pkg);
    plan.stage_part(&pkg, "xl/s.xml", XmlDocument::parse(&pkg.read_part("xl/s.xml").unwrap()).unwrap()).unwrap();
    plan.remove_parts(&pkg, &["xl/media/i.png"], &[IMG]).unwrap();
    pkg.apply(plan).unwrap();
    let out = pkg.to_bytes().unwrap();
    assert_eq!(text(&out, "xl/s.xml"), PLAIN, "the validated content is saved");
    assert_eq!(pkg.read_part("xl/s.xml").unwrap(), PLAIN.as_bytes());
    // the same through xml_part_mut
    let mut pkg = embed_pkg();
    {
        let doc = pkg.xml_part_mut("xl/s.xml").unwrap();
        let mut root = doc.lens_mut::<Any>().unwrap();
        root.remove_at(0);
    }
    let edited = pkg.read_part("xl/s.xml").unwrap();
    let mut plan = RegistryPlan::new(&pkg);
    plan.stage_part(&pkg, "xl/s.xml", XmlDocument::parse(&edited).unwrap()).unwrap();
    plan.remove_parts(&pkg, &["xl/media/i.png"], &[IMG]).unwrap();
    pkg.apply(plan).unwrap();
    assert_eq!(text(&pkg.to_bytes().unwrap(), "xl/s.xml").as_bytes(), &edited[..]);
}

/// A generic spec for the probe's root edit (`<s>` with any children).
enum Any {}
impl ooxml_core::ElementSpec for Any {
    const QNAME: ooxml_core::QName = ooxml_core::QName::new("", "s");
    const SDK_NAME: &'static str = "test:s";
    const ATTRS: &'static [ooxml_core::AttrSpec] = &[];
    const CONTENT: ooxml_core::Content = ooxml_core::Content::Unsupported(&[]);
}

/// N2: removal reads the plan's targets (a `set_relationship_target`
/// earlier in the plan wins) — the four cases of the review's table.
#[test]
fn n2_removal_sees_retargets() {
    let r = rels(&[("rId1", IMG, "media/a.png")]);
    let pkg_with = |src: &str| Package::from_bytes(base(&[("xl/s.xml", src), ("xl/_rels/s.xml.rels", &r), ("xl/media/a.png", "A"), ("xl/media/b.png", "B")])).unwrap();
    // P2: retarget a->b, remove b, source refers to rId1: refused
    let pkg = pkg_with(EMBED);
    let mut plan = RegistryPlan::new(&pkg);
    plan.set_relationship_target(&pkg, Some("xl/s.xml"), "rId1", "media/b.png").unwrap();
    assert_matches!(
        refusal(plan.remove_parts(&pkg, &["xl/media/b.png"], &[IMG]).unwrap_err()),
        RegistryRefusal::Remove(RemoveRefusal::StillReferenced { .. })
    );
    // P2d: no r:id: the retargeted relationship goes with b
    let mut pkg = pkg_with(PLAIN);
    let mut plan = RegistryPlan::new(&pkg);
    plan.set_relationship_target(&pkg, Some("xl/s.xml"), "rId1", "media/b.png").unwrap();
    let removed = plan.remove_parts(&pkg, &["xl/media/b.png"], &[IMG]).unwrap();
    assert_eq!(removed.relationships, vec![("xl/_rels/s.xml.rels".to_owned(), "rId1".to_owned())]);
    pkg.apply(plan).unwrap();
    assert!(pkg.relationships("xl/s.xml").unwrap().by_id("rId1").is_none());
    // P2c: retarget a->b, remove a (no r:id): rId1 stays, pointing at b
    let mut pkg = pkg_with(PLAIN);
    let mut plan = RegistryPlan::new(&pkg);
    plan.set_relationship_target(&pkg, Some("xl/s.xml"), "rId1", "media/b.png").unwrap();
    let removed = plan.remove_parts(&pkg, &["xl/media/a.png"], &[IMG]).unwrap();
    assert!(removed.relationships.is_empty());
    pkg.apply(plan).unwrap();
    let rs = pkg.relationships("xl/s.xml").unwrap();
    assert_eq!(rs.resolve(rs.by_id("rId1").unwrap()).as_deref(), Some("xl/media/b.png"));
    // P2b: as P2c with r:embed: allowed (rId1 no longer points at a)
    let pkg = pkg_with(EMBED);
    let mut plan = RegistryPlan::new(&pkg);
    plan.set_relationship_target(&pkg, Some("xl/s.xml"), "rId1", "media/b.png").unwrap();
    assert!(plan.remove_parts(&pkg, &["xl/media/a.png"], &[IMG]).unwrap().relationships.is_empty());
}

/// n3: `set_relationship_target` refuses what `add_relationship` refuses,
/// and writes a part target in canonical relative form.
#[test]
fn n3_retarget_validates_like_add() {
    let r = rels(&[("rId3", IMG, "media/a.png")]);
    let input = base(&[("xl/s.xml", PLAIN), ("xl/_rels/s.xml.rels", &r), ("xl/media/a.png", "A"), ("xl/media/b.png", "B")]);
    let pkg = Package::from_bytes(input.clone()).unwrap();
    for t in ["../[Content_Types].xml", "_rels/s.xml.rels", "../_rels/.rels"] {
        let mut plan = RegistryPlan::new(&pkg);
        assert_matches!(refusal(plan.set_relationship_target(&pkg, Some("xl/s.xml"), "rId3", t).unwrap_err()), RegistryRefusal::RegistryPart, "{t}");
    }
    for (t, written) in [("media/%62.png", "media/b.png"), ("media/B.PNG", "media/b.png"), ("/xl/media/b.png", "media/b.png"), ("media/b.png#y", "media/b.png#y")] {
        let mut pkg = Package::from_bytes(input.clone()).unwrap();
        pkg.set_relationship_target(Some("xl/s.xml"), "rId3", t).unwrap();
        let out = Package::from_bytes(pkg.to_bytes().unwrap()).unwrap();
        assert_eq!(out.relationships("xl/s.xml").unwrap().by_id("rId3").unwrap().target, written, "{t}");
    }
}

/// n4: a part re-created with `Exact` continues above the Ids of the
/// relationships part removed with the old one, and content staged for it
/// may not name those Ids.
#[test]
fn n4_recreated_part_keeps_the_id_high_water() {
    let r = rels(&[("rId1", IMG, "media/a.png"), ("rId2", IMG, "media/a.png"), ("rId3", IMG, "media/a.png")]);
    let wbr = rels(&[("rId9", "urn:x", "s.xml")]);
    let mut pkg = Package::from_bytes(base(&[("xl/s.xml", EMBED), ("xl/_rels/s.xml.rels", &r), ("xl/media/a.png", "A"), ("xl/_rels/workbook.xml.rels", &wbr)])).unwrap();
    let mut plan = RegistryPlan::new(&pkg);
    plan.remove_parts(&pkg, &["xl/s.xml"], &["urn:x"]).unwrap();
    plan.add_part(&pkg, PartNameRequest::Exact("xl/s.xml".into()), "application/xml", PLAIN.as_bytes().to_vec()).unwrap();
    assert_eq!(plan.add_relationship(&pkg, Some("xl/s.xml"), IMG, Target::Part("xl/media/a.png".into())).unwrap(), "rId4");
    let old = br#"<s xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><p r:embed="rId3"/></s>"#;
    assert_matches!(refusal(plan.stage_part(&pkg, "xl/s.xml", old.to_vec()).unwrap_err()), RegistryRefusal::ConflictsWithPlan);
    pkg.apply(plan).unwrap();
    assert_eq!(pkg.add_relationship(Some("xl/s.xml"), IMG, Target::Part("xl/media/a.png".into())).unwrap(), "rId5");
}

/// n5: `Package::relationships` finds a `.rels` that differs in case, as
/// the registry operations do.
#[test]
fn n5_case_differing_rels() {
    let r = rels(&[("rId1", IMG, "../media/a.png")]);
    let mut pkg = Package::from_bytes(base(&[("xl/ws/Sheet1.xml", EMBED), ("xl/ws/_rels/sheet1.xml.rels", &r), ("xl/media/a.png", "A"), ("xl/media/b.png", "B")])).unwrap();
    assert_eq!(pkg.relationships("xl/ws/Sheet1.xml").unwrap().iter().count(), 1);
    let id = pkg.add_relationship(Some("xl/ws/Sheet1.xml"), IMG, Target::Part("xl/media/b.png".into())).unwrap();
    let rs = pkg.relationships("xl/ws/Sheet1.xml").unwrap();
    assert_eq!(rs.resolve(rs.by_id(&id).unwrap()).as_deref(), Some("xl/media/b.png"));
}

/// N28, N29, N37: retargeting relationships added earlier in the plan (to
/// an existing `.rels`, to a new one, and a package relationship).
#[test]
fn retarget_relationships_added_in_the_plan() {
    let r = rels(&[("rId1", IMG, "media/a.png")]);
    let mut pkg = Package::from_bytes(base(&[("xl/s.xml", PLAIN), ("xl/_rels/s.xml.rels", &r), ("xl/t.xml", PLAIN), ("xl/media/a.png", "A"), ("xl/media/b.png", "B"), ("xl/other.xml", WB)])).unwrap();
    let mut plan = RegistryPlan::new(&pkg);
    let appended = plan.add_relationship(&pkg, Some("xl/s.xml"), IMG, Target::Part("xl/media/a.png".into())).unwrap();
    let fresh = plan.add_relationship(&pkg, Some("xl/t.xml"), IMG, Target::Part("xl/media/a.png".into())).unwrap();
    let pkg_rel = plan.add_relationship(&pkg, None, "urn:x:custom", Target::Part("xl/workbook.xml".into())).unwrap();
    plan.set_relationship_target(&pkg, Some("xl/s.xml"), &appended, "media/b.png").unwrap();
    plan.set_relationship_target(&pkg, Some("xl/t.xml"), &fresh, "media/b.png").unwrap();
    plan.set_relationship_target(&pkg, None, &pkg_rel, "xl/other.xml").unwrap();
    let inc = plan.incoming_relationships(&pkg, "xl/media/b.png").unwrap();
    assert_eq!(inc.len(), 2, "the view sees both retargets");
    pkg.apply(plan).unwrap();
    for (src, id) in [("xl/s.xml", &appended), ("xl/t.xml", &fresh)] {
        let rs = pkg.relationships(src).unwrap();
        assert_eq!(rs.resolve(rs.by_id(id).unwrap()).as_deref(), Some("xl/media/b.png"), "{src}");
    }
    let p = pkg.package_relationships();
    assert_eq!(p.resolve(p.by_id(&pkg_rel).unwrap()).as_deref(), Some("xl/other.xml"), "package relationships cache");
    let again = Package::from_bytes(pkg.to_bytes().unwrap()).unwrap();
    let p = again.package_relationships();
    assert_eq!(p.resolve(p.by_id(&pkg_rel).unwrap()).as_deref(), Some("xl/other.xml"));
}
