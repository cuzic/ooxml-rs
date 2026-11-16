//! Regression tests for the round-1 review of PR #38 (M1–M7, m1–m10) and
//! for the mutants that survived it. Each test names the finding it pins.

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
const HL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink";
const COMMENTS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/comments";
const STYLES: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles";
const WB: &str = r#"<workbook xmlns="urn:x" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"/>"#;
const EMBED: &str = r#"<s xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><pic r:embed="rId1"/></s>"#;

fn rels(rows: &[(&str, &str, &str)]) -> String {
    let mut s = String::from(r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#);
    for (id, ty, target) in rows {
        let ext = if target.starts_with("http") { r#" TargetMode="External""# } else { "" };
        s.push_str(&format!(r#"<Relationship Id="{id}" Type="{ty}" Target="{target}"{ext}/>"#));
    }
    s.push_str("</Relationships>");
    s
}

fn base(extra: &[(&str, &str)]) -> Vec<u8> {
    let mut v = vec![("[Content_Types].xml", CT), ("_rels/.rels", ROOT), ("xl/workbook.xml", WB)];
    v.extend_from_slice(extra);
    build(&v)
}

// ------------------------------------------------------------ M1

/// M1 (PR #14 N17 inherited): a source named `xl/a%2541.xml` is found from
/// its relationships part's raw name, so its `r:embed` still refuses.
#[test]
fn m1_percent_encoded_source_is_scanned() {
    for src in ["xl/plain.xml", "xl/a%2541.xml"] {
        let file = src.rsplit('/').next().unwrap();
        let r = rels(&[("rId1", IMG, "media/i.png")]);
        let rels_name = format!("xl/_rels/{file}.rels");
        let mut pkg = Package::from_bytes(base(&[(src, EMBED), (&rels_name, &r), ("xl/media/i.png", "PNG")])).unwrap();
        let e = pkg.remove_parts(&["xl/media/i.png"], &[IMG]).unwrap_err();
        assert_matches!(refusal(e), RegistryRefusal::Remove(RemoveRefusal::StillReferenced { .. }), "{src}");
        let inc = pkg.incoming_relationships("xl/media/i.png").unwrap();
        assert_eq!(inc.len(), 1, "{src}");
        assert_eq!(inc[0].source.as_deref(), Some(src), "the real source name");
        // remove_part (PR #14) runs the same validation
        assert_matches!(pkg.remove_part("xl/media/i.png", &[IMG]), Err(Error::RemovePartRefused { reason: RemoveRefusal::StillReferenced { .. }, .. }));
    }
}

/// M1: base folder decoded once; with a decoy at the double-decoded path,
/// only the right relationship goes.
#[test]
fn m1_base_folder_is_decoded_once() {
    let r = rels(&[("rId1", IMG, "media/i.png")]);
    let unreferenced = r#"<s/>"#;
    let input = base(&[
        ("x%2541/s.xml", unreferenced),
        ("x%2541/_rels/s.xml.rels", &r),
        ("x%2541/media/i.png", "PNG"),
        ("xa/media/i.png", "PNG2"),
    ]);
    let mut pkg = Package::from_bytes(input).unwrap();
    // the decoy has no incoming relationship
    assert!(pkg.incoming_relationships("xa/media/i.png").unwrap().is_empty());
    let removed = pkg.check_remove_parts(&["xa/media/i.png"], &[IMG]).unwrap();
    assert!(removed.relationships.is_empty(), "the decoy removal touches no relationship");
    let removed = pkg.remove_parts(&["x%2541/media/i.png"], &[IMG]).unwrap();
    assert_eq!(removed.relationships, vec![("x%2541/_rels/s.xml.rels".to_owned(), "rId1".to_owned())]);
    let out = pkg.to_bytes().unwrap();
    assert!(!text(&out, "x%2541/_rels/s.xml.rels").contains("rId1"));
}

// ------------------------------------------------------------ M2

/// M2: a freshly parsed (not dirty) tree staged for an existing part is
/// what is saved and read back.
#[test]
fn m2_staged_fresh_tree_is_saved() {
    let r = rels(&[("rId1", IMG, "media/i.png")]);
    let mut pkg = Package::from_bytes(base(&[("xl/s.xml", EMBED), ("xl/_rels/s.xml.rels", &r), ("xl/media/i.png", "PNG")])).unwrap();
    let mut plan = RegistryPlan::new(&pkg);
    let fresh = br#"<s xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"/>"#;
    plan.stage_part(&pkg, "xl/s.xml", XmlDocument::parse(fresh).unwrap()).unwrap();
    plan.remove_parts(&pkg, &["xl/media/i.png"], &[IMG]).unwrap();
    pkg.apply(plan).unwrap();
    assert_eq!(pkg.read_part("xl/s.xml").unwrap(), fresh);
    let out = pkg.to_bytes().unwrap();
    assert_eq!(text(&out, "xl/s.xml").as_bytes(), fresh);
    assert!(!text(&out, "xl/_rels/s.xml.rels").contains("rId1"));
    // an unedited clone of the current tree is still raw-copied (L1)
    let input = base(&[("xl/s.xml", EMBED)]);
    let mut pkg = Package::from_bytes(input.clone()).unwrap();
    let doc = pkg.xml_part("xl/s.xml").unwrap().clone();
    let mut plan = RegistryPlan::new(&pkg);
    plan.stage_part(&pkg, "xl/s.xml", doc).unwrap();
    pkg.apply(plan).unwrap();
    let raw = |b: &[u8]| {
        let mut ar = ZipArchive::new(Cursor::new(b.to_vec())).unwrap();
        let f = ar.by_name("xl/s.xml").unwrap();
        (f.crc32(), f.compressed_size())
    };
    assert_eq!(raw(&input), raw(&pkg.to_bytes().unwrap()));
}

/// R28: staging a part twice — the view and the saved part are the later
/// content.
#[test]
fn staging_twice_keeps_the_later_content() {
    let r = rels(&[("rId1", IMG, "media/i.png")]);
    let mut pkg = Package::from_bytes(base(&[("xl/s.xml", "<s/>"), ("xl/_rels/s.xml.rels", &r), ("xl/media/i.png", "PNG")])).unwrap();
    let mut plan = RegistryPlan::new(&pkg);
    plan.stage_part(&pkg, "xl/s.xml", b"<s/>".to_vec()).unwrap();
    plan.stage_part(&pkg, "xl/s.xml", EMBED.as_bytes().to_vec()).unwrap();
    assert_matches!(
        refusal(plan.remove_parts(&pkg, &["xl/media/i.png"], &[IMG]).unwrap_err()),
        RegistryRefusal::Remove(RemoveRefusal::StillReferenced { .. }),
        "the scan reads the later content"
    );
    pkg.apply(plan).unwrap();
    assert_eq!(pkg.read_part("xl/s.xml").unwrap(), EMBED.as_bytes());
}

/// R1: the view reads the content of a part added earlier in the plan.
#[test]
fn view_reads_planned_part_content() {
    let pkg = Package::from_bytes(base(&[("xl/media/i.png", "PNG")])).unwrap();
    let mut plan = RegistryPlan::new(&pkg);
    let a = plan.add_part(&pkg, PartNameRequest::Exact("xl/a.xml".into()), "application/xml", EMBED.as_bytes()).unwrap();
    let id = plan.add_relationship(&pkg, Some(&a), IMG, Target::Part("xl/media/i.png".into())).unwrap();
    assert_eq!(id, "rId2", "rId1 is used by an attribute of the planned source (M7)");
    let staged = br#"<s xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><x r:id="rId2"/></s>"#;
    plan.stage_part(&pkg, &a, staged.to_vec()).unwrap();
    assert_matches!(
        refusal(plan.remove_relationship(&pkg, Some(&a), "rId2", &[IMG]).unwrap_err()),
        RegistryRefusal::StillReferenced { .. },
        "the scan reads the planned part's staged content"
    );
}

// ------------------------------------------------------------ M3

/// M3: an unreadable relationships part counts as mentioning the part:
/// `remove_parts` and `remove_part` refuse alike; `incoming_relationships`
/// fails closed.
#[test]
fn m3_unreadable_rels_fail_closed() {
    let broken = r#"<Relationships xmlns='http://schemas.openxmlformats.org/package/2006/relationships'><Relationship Id='rId1' Type='x:y' Target='media/i.png'/>"#;
    let pkg = Package::from_bytes(base(&[("xl/s.xml", "<s/>"), ("xl/_rels/s.xml.rels", broken), ("xl/media/i.png", "PNG")])).unwrap();
    assert_matches!(
        refusal(pkg.check_remove_parts(&["xl/media/i.png"], &[IMG, "x:y"]).unwrap_err()),
        RegistryRefusal::Remove(RemoveRefusal::RegistryNotEditable { .. })
    );
    assert_matches!(pkg.check_remove_part("xl/media/i.png", &[IMG, "x:y"]), Err(Error::RemovePartRefused { reason: RemoveRefusal::RegistryNotEditable { .. }, .. }));
    assert_matches!(refusal(pkg.incoming_relationships("xl/media/i.png").unwrap_err()), RegistryRefusal::RegistryNotEditable { .. });
    // a part it does not mention is unaffected
    assert!(pkg.incoming_relationships("xl/workbook.xml").is_ok());
    // a relationships part that is one only after decoding (%5Frels)
    let pkg = Package::from_bytes(base(&[("xl/%5Frels/s.xml.rels", &rels(&[("rId1", IMG, "media/i.png")])), ("xl/s.xml", "<s/>"), ("xl/media/i.png", "PNG")])).unwrap();
    assert!(pkg.check_remove_parts(&["xl/media/i.png"], &[IMG]).is_err());
    assert!(pkg.incoming_relationships("xl/media/i.png").is_err());
}

// ------------------------------------------------------------ M4, M6

/// M4 (a): src added in the plan, with a planned relationship; (b) an
/// existing src with a planned external relationship; (R22) a relationship
/// the plan removed is not copied.
#[test]
fn m4_copy_reads_the_plans_view() {
    let r = rels(&[("rId1", IMG, "media/i.png"), ("rId3", HL, "https://old/")]);
    let pkg = Package::from_bytes(base(&[("xl/s.xml", EMBED), ("xl/_rels/s.xml.rels", &r), ("xl/media/i.png", "PNG")])).unwrap();
    let mut plan = RegistryPlan::new(&pkg);
    let a = plan.add_part(&pkg, PartNameRequest::Exact("xl/a.xml".into()), "application/xml", WB.as_bytes()).unwrap();
    let id = plan.add_relationship(&pkg, Some(&a), IMG, Target::Part("xl/media/i.png".into())).unwrap();
    let b = plan.add_part(&pkg, PartNameRequest::Exact("xl/b.xml".into()), "application/xml", WB.as_bytes()).unwrap();
    assert_eq!(plan.copy_relationships(&pkg, &a, &b, &[], &[]).unwrap(), vec![id]);
    let mut plan = RegistryPlan::new(&pkg);
    let ext = plan.add_relationship(&pkg, Some("xl/s.xml"), HL, Target::External("https://e/".into())).unwrap();
    plan.remove_relationship(&pkg, Some("xl/s.xml"), "rId3", &[HL]).unwrap();
    let b = plan.add_part(&pkg, PartNameRequest::Exact("xl/b.xml".into()), "application/xml", WB.as_bytes()).unwrap();
    assert_eq!(plan.copy_relationships(&pkg, "xl/s.xml", &b, &[], &[]).unwrap(), vec!["rId1".to_owned(), ext]);
}

/// M6: unresolvable internal targets and fragments are copied verbatim
/// within a folder; a fragment on a part target is kept; outside the folder
/// a verbatim copy would change meaning and is refused. `skip_types`.
#[test]
fn m6_copy_targets_verbatim_fragments_and_type_filter() {
    for (t, expected) in [
        ("#_ftn1", "#_ftn1"),
        ("media/missing.png", "media/missing.png"),
        ("NULL", "NULL"),
        ("media/i.png#frag", "media/i.png#frag"),
        ("media/I.PNG", "media/i.png"),
    ] {
        let r = rels(&[("rId9", HL, t), ("rId8", COMMENTS, "media/i.png")]);
        let pkg = Package::from_bytes(base(&[("xl/s.xml", "<s/>"), ("xl/_rels/s.xml.rels", &r), ("xl/media/i.png", "PNG")])).unwrap();
        let mut plan = RegistryPlan::new(&pkg);
        let b = plan.add_part(&pkg, PartNameRequest::Exact("xl/b.xml".into()), "application/xml", b"<s/>".to_vec()).unwrap();
        let ids = plan.copy_relationships(&pkg, "xl/s.xml", &b, &[], &[COMMENTS]);
        assert_eq!(ids.as_deref().ok(), Some(&["rId9".to_owned()][..]), "{t}: {ids:?}");
        let out = {
            let mut pkg = Package::from_bytes(base(&[("xl/s.xml", "<s/>"), ("xl/_rels/s.xml.rels", &r), ("xl/media/i.png", "PNG")])).unwrap();
            let mut plan = RegistryPlan::new(&pkg);
            plan.add_part(&pkg, PartNameRequest::Exact("xl/b.xml".into()), "application/xml", b"<s/>".to_vec()).unwrap();
            plan.copy_relationships(&pkg, "xl/s.xml", "xl/b.xml", &[], &[COMMENTS]).unwrap();
            pkg.apply(plan).unwrap();
            pkg.to_bytes().unwrap()
        };
        let copied = text(&out, "xl/_rels/b.xml.rels");
        assert!(copied.contains(&format!(r#"Target="{expected}""#)), "{t}: {copied}");
        assert!(!copied.contains("rId8"), "comments skipped");
        // another folder: a part target is re-relativised, a verbatim one refused
        let pkg = Package::from_bytes(base(&[("xl/s.xml", "<s/>"), ("xl/_rels/s.xml.rels", &r), ("xl/media/i.png", "PNG")])).unwrap();
        let mut plan = RegistryPlan::new(&pkg);
        plan.add_part(&pkg, PartNameRequest::Exact("xl/d/b.xml".into()), "application/xml", b"<s/>".to_vec()).unwrap();
        let res = plan.copy_relationships(&pkg, "xl/s.xml", "xl/d/b.xml", &[], &[COMMENTS]);
        let resolves = matches!(t, "media/i.png#frag" | "media/I.PNG");
        assert_eq!(res.is_ok(), resolves, "{t} into another folder: {res:?}");
    }
}

// ------------------------------------------------------------ M5

/// M5: `remove_relationship` needs the type on the caller's allowlist
/// (styles/sharedStrings are referenced by type).
#[test]
fn m5_remove_relationship_needs_an_allowed_type() {
    let r = rels(&[("rId1", STYLES, "styles.xml")]);
    let pkg = Package::from_bytes(base(&[("xl/_rels/workbook.xml.rels", &r), ("xl/styles.xml", "<styleSheet/>")])).unwrap();
    assert_matches!(
        refusal(pkg.check_remove_relationship(Some("xl/workbook.xml"), "rId1", &[]).unwrap_err()),
        RegistryRefusal::Remove(RemoveRefusal::RelationshipTypeNotAllowed { .. })
    );
    pkg.check_remove_relationship(Some("xl/workbook.xml"), "rId1", &[STYLES]).unwrap();
}

/// m5 / ADR-0012: a relationship added earlier in the plan can be removed
/// in the same plan.
#[test]
fn remove_a_relationship_added_in_the_plan() {
    let mut pkg = Package::from_bytes(base(&[("xl/s.xml", "<s/>"), ("xl/media/i.png", "PNG")])).unwrap();
    let mut plan = RegistryPlan::new(&pkg);
    let a = plan.add_relationship(&pkg, Some("xl/s.xml"), IMG, Target::Part("xl/media/i.png".into())).unwrap();
    let b = plan.add_relationship(&pkg, Some("xl/s.xml"), COMMENTS, Target::Part("xl/media/i.png".into())).unwrap();
    plan.remove_relationship(&pkg, Some("xl/s.xml"), &b, &[COMMENTS]).unwrap();
    assert_matches!(refusal(plan.remove_relationship(&pkg, Some("xl/s.xml"), &b, &[COMMENTS]).unwrap_err()), RegistryRefusal::RelationshipNotFound);
    pkg.apply(plan).unwrap();
    let r = pkg.relationships("xl/s.xml").unwrap();
    assert!(r.by_id(&a).is_some() && r.by_id(&b).is_none());
}

// ------------------------------------------------------------ M7

/// M7: a new `Id` never adopts a dangling `r:*` (or VML `o:relid`) value of
/// the source.
#[test]
fn m7_new_ids_avoid_dangling_references() {
    let s = r#"<s xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:o="urn:schemas-microsoft-com:office:office"><a r:id="rId2"/><b o:relid="RID3"/></s>"#;
    let pkg = Package::from_bytes(base(&[("xl/s.xml", s), ("xl/media/i.png", "P")])).unwrap();
    let mut plan = RegistryPlan::new(&pkg);
    let a = plan.add_relationship(&pkg, Some("xl/s.xml"), IMG, Target::Part("xl/media/i.png".into())).unwrap();
    let b = plan.add_relationship(&pkg, Some("xl/s.xml"), IMG, Target::Part("xl/media/i.png".into())).unwrap();
    assert_eq!((a.as_str(), b.as_str()), ("rId4", "rId5"));
    // an XML source that does not parse cannot be scanned
    let pkg = Package::from_bytes(base(&[("xl/s.xml", "<s a=b/>"), ("xl/media/i.png", "P")])).unwrap();
    assert_matches!(
        refusal(pkg.check_add_relationship(Some("xl/s.xml"), IMG, Target::Part("xl/media/i.png".into())).unwrap_err()),
        RegistryRefusal::SourceNotScannable { .. }
    );
    // a non-XML source is not scanned
    let pkg = Package::from_bytes(base(&[("xl/media/i.png", "P<"), ("xl/media/j.png", "P")])).unwrap();
    assert_eq!(pkg.check_add_relationship(Some("xl/media/i.png"), IMG, Target::Part("xl/media/j.png".into())).unwrap(), "rId1");
}

// ------------------------------------------------------------ retarget

/// ADR-0009 R2-M1: a hyperlink's `#Sheet!A1` target follows a renamed sheet.
#[test]
fn set_relationship_target_rewrites_one_target() {
    let r = rels(&[("rId1", HL, "#Old!A1"), ("rId2", IMG, "media/i.png"), ("rId3", HL, "https://a/")]);
    let input = base(&[("xl/s.xml", "<s/>"), ("xl/_rels/s.xml.rels", &r), ("xl/media/i.png", "P"), ("xl/media/j.png", "P")]);
    let mut pkg = Package::from_bytes(input).unwrap();
    let mut plan = RegistryPlan::new(&pkg);
    plan.set_relationship_target(&pkg, Some("xl/s.xml"), "rId1", "#'New Name'!A1").unwrap();
    plan.set_relationship_target(&pkg, Some("xl/s.xml"), "rId2", "media/j.png").unwrap();
    plan.set_relationship_target(&pkg, Some("xl/s.xml"), "rId3", "https://b/").unwrap();
    assert_matches!(refusal(plan.set_relationship_target(&pkg, Some("xl/s.xml"), "rId2", "media/none.png").unwrap_err()), RegistryRefusal::TargetNotFound { .. });
    assert_matches!(refusal(plan.set_relationship_target(&pkg, Some("xl/s.xml"), "rId9", "#x").unwrap_err()), RegistryRefusal::RelationshipNotFound);
    assert_matches!(refusal(plan.set_relationship_target(&pkg, Some("xl/s.xml"), "rId1", "a b").unwrap_err()), RegistryRefusal::InvalidTarget);
    // the view sees the new target
    assert_eq!(plan.incoming_relationships(&pkg, "xl/media/j.png").unwrap().len(), 1);
    assert!(plan.incoming_relationships(&pkg, "xl/media/i.png").unwrap().is_empty());
    pkg.apply(plan).unwrap();
    let out = pkg.to_bytes().unwrap();
    let t = text(&out, "xl/_rels/s.xml.rels");
    assert!(t.contains(r##"Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="#'New Name'!A1""##), "{t}");
    assert!(t.contains(r#"Target="media/j.png""#) && t.contains(r#"Target="https://b/" TargetMode="External""#), "{t}");
    // package relationships cache follows
    let mut pkg = Package::from_bytes(base(&[("xl/other.xml", "<w/>")])).unwrap();
    pkg.set_relationship_target(None, "rId1", "xl/other.xml").unwrap();
    assert_eq!(pkg.main_document_part().as_deref(), Some("xl/other.xml"));
}

// ------------------------------------------------------------ minors

/// m2 / R17 R19 R20 R36: every mutating path makes earlier plans stale; a
/// plan for another package is refused; parse-only access does not.
#[test]
fn every_change_makes_plans_stale() {
    let input = base(&[("xl/s.xml", "<s/>"), ("xl/media/i.png", "P")]);
    type Change = fn(&mut Package);
    let changes: [(&str, Change); 6] = [
        ("set_part", |p| p.set_part("xl/s.xml", b"<t/>".to_vec()).unwrap()),
        ("xml_part_mut", |p| { p.xml_part_mut("xl/s.xml").unwrap(); }),
        ("remove_part", |p| drop(p.remove_part("xl/media/i.png", &[]).unwrap())),
        ("apply", |p| drop(p.add_part(PartNameRequest::Exact("xl/n.xml".into()), "application/xml", b"<n/>".to_vec()).unwrap())),
        ("remove_parts", |p| drop(p.remove_parts(&["xl/media/i.png"], &[]).unwrap())),
        ("set_relationship_target", |p| p.set_relationship_target(None, "rId1", "xl/s.xml").unwrap()),
    ];
    for (what, change) in changes {
        let mut pkg = Package::from_bytes(input.clone()).unwrap();
        let mut plan = RegistryPlan::new(&pkg);
        plan.add_part(&pkg, PartNameRequest::Exact("xl/z.xml".into()), "application/xml", b"<z/>".to_vec()).unwrap();
        change(&mut pkg);
        assert_matches!(refusal(pkg.apply(plan.clone()).unwrap_err()), RegistryRefusal::StalePlan, "{what}");
        assert_matches!(
            refusal(plan.add_part(&pkg, PartNameRequest::Exact("xl/y.xml".into()), "application/xml", b"<y/>".to_vec()).unwrap_err()),
            RegistryRefusal::StalePlan,
            "{what}"
        );
    }
    let mut pkg = Package::from_bytes(input.clone()).unwrap();
    let other = Package::from_bytes(input.clone()).unwrap();
    assert_matches!(refusal(pkg.apply(RegistryPlan::new(&other)).unwrap_err()), RegistryRefusal::StalePlan, "another package");
    let plan = RegistryPlan::new(&pkg);
    pkg.xml_part("xl/s.xml").unwrap();
    pkg.apply(plan).unwrap();
}

/// m4 / R13 R14: a part removed earlier in the plan no longer blocks a
/// `Default`; a part added earlier does; the view's content type of a
/// removed part is `None`.
#[test]
fn defaults_see_the_plans_view() {
    let pkg = Package::from_bytes(base(&[("xl/a.foo", "F")])).unwrap();
    let mut plan = RegistryPlan::new(&pkg);
    assert_matches!(refusal(plan.add_default_content_type(&pkg, "foo", "x/y").unwrap_err()), RegistryRefusal::WouldRetypePart { .. });
    plan.remove_parts(&pkg, &["xl/a.foo"], &[]).unwrap();
    assert_eq!(plan.content_type(&pkg, "xl/a.foo"), None);
    assert!(plan.add_default_content_type(&pkg, "foo", "x/y").unwrap());
    let mut plan = RegistryPlan::new(&pkg);
    plan.add_part(&pkg, PartNameRequest::Exact("xl/b.bar".into()), "x/bar", b"B".to_vec()).unwrap();
    plan.add_default_content_type(&pkg, "bar", "x/bar").unwrap();
    assert_eq!(plan.content_type(&pkg, "xl/b.bar"), Some("x/bar"));
}

/// m5: `set_content_type` on a part added in the plan.
#[test]
fn set_content_type_of_a_planned_part() {
    let mut pkg = Package::from_bytes(base(&[])).unwrap();
    let mut plan = RegistryPlan::new(&pkg);
    let a = plan.add_part(&pkg, PartNameRequest::Exact("xl/a.bin".into()), "x/one", b"A".to_vec()).unwrap();
    let b = plan.add_part(&pkg, PartNameRequest::Exact("xl/b.png".into()), "image/png", b"B".to_vec()).unwrap();
    plan.set_content_type(&pkg, &a, "x/two").unwrap();
    plan.set_content_type(&pkg, &b, "image/x-png").unwrap();
    pkg.apply(plan).unwrap();
    let out = Package::from_bytes(pkg.to_bytes().unwrap()).unwrap();
    assert_eq!(out.content_type("xl/a.bin"), Some("x/two"));
    assert_eq!(out.content_type("xl/b.png"), Some("image/x-png"));
    assert_eq!(text(&pkg.to_bytes().unwrap(), "[Content_Types].xml").matches("/xl/a.bin").count(), 1);
}

/// A4: an `Id` differing only in case collides (`RID2` blocks `rId2`).
#[test]
fn id_case_collision() {
    let r = rels(&[("rId1", IMG, "media/i.png"), ("RID2", IMG, "media/i.png")]);
    let pkg = Package::from_bytes(base(&[("xl/s.xml", "<s/>"), ("xl/_rels/s.xml.rels", &r), ("xl/media/i.png", "P")])).unwrap();
    assert_eq!(pkg.check_add_relationship(Some("xl/s.xml"), IMG, Target::Part("xl/media/i.png".into())).unwrap(), "rId3");
    let r = rels(&[("rId1", IMG, "media/i.png"), ("rid3", IMG, "media/i.png")]);
    let pkg = Package::from_bytes(base(&[("xl/s.xml", "<s/>"), ("xl/_rels/s.xml.rels", &r), ("xl/media/i.png", "P")])).unwrap();
    // max is 3 (prefix case-insensitive): next is rId4
    assert_eq!(pkg.check_add_relationship(Some("xl/s.xml"), IMG, Target::Part("xl/media/i.png".into())).unwrap(), "rId4");
}

/// A11: Numbered skips a name removed earlier in the same plan.
#[test]
fn numbered_skips_names_removed_in_the_plan() {
    let pkg = Package::from_bytes(base(&[("xl/p9.xml", "<p/>")])).unwrap();
    let mut plan = RegistryPlan::new(&pkg);
    plan.remove_parts(&pkg, &["xl/p9.xml"], &[]).unwrap();
    assert_eq!(plan.add_part(&pkg, PartNameRequest::Numbered("xl/p{}.xml".into()), "application/xml", b"<p/>".to_vec()).unwrap(), "xl/p10.xml");
}

/// A16: a source inside the removed set is not scanned (its reference goes
/// with it).
#[test]
fn sources_in_the_set_are_not_scanned() {
    let r = rels(&[("rId1", IMG, "media/i.png")]);
    let wb_rels = rels(&[("rId1", IMG, "s.xml")]);
    let mut pkg = Package::from_bytes(base(&[
        ("xl/_rels/workbook.xml.rels", &wb_rels),
        ("xl/s.xml", EMBED),
        ("xl/_rels/s.xml.rels", &r),
        ("xl/media/i.png", "P"),
    ]))
    .unwrap();
    let removed = pkg.remove_parts(&["xl/s.xml", "xl/media/i.png"], &[IMG]).unwrap();
    assert_eq!(removed.parts.len(), 3);
}

/// m10: a fragment-only target is not a part (no fake `kept_targets` entry,
/// not counted as a name in use).
#[test]
fn fragment_only_targets_name_no_part() {
    let r = rels(&[("rId1", HL, "#Sheet1!A1")]);
    let pkg = Package::from_bytes(base(&[("xl/drawings/d.xml", "<d/>"), ("xl/drawings/_rels/d.xml.rels", &r)])).unwrap();
    let removed = pkg.check_remove_parts(&["xl/drawings/d.xml"], &[]).unwrap();
    assert!(removed.kept_targets.is_empty(), "{removed:?}");
    assert_eq!(pkg.part_name_in_use("xl/drawings"), Some("part \"xl/drawings/d.xml\"".into()));
    assert!(pkg.incoming_relationships("xl/drawings").unwrap().is_empty());
}

/// m3 / R3 R5: remove then re-add a relationship in one plan; the removed
/// `Id` is not reused, and cannot be removed twice.
#[test]
fn remove_then_add_in_one_plan() {
    let r = rels(&[("rId1", IMG, "media/i.png")]);
    let mut pkg = Package::from_bytes(base(&[("xl/s.xml", "<s/>"), ("xl/_rels/s.xml.rels", &r), ("xl/media/i.png", "P")])).unwrap();
    let mut plan = RegistryPlan::new(&pkg);
    plan.remove_relationship(&pkg, Some("xl/s.xml"), "rId1", &[IMG]).unwrap();
    assert_matches!(refusal(plan.remove_relationship(&pkg, Some("xl/s.xml"), "rId1", &[IMG]).unwrap_err()), RegistryRefusal::RelationshipNotFound);
    assert_eq!(plan.add_relationship(&pkg, Some("xl/s.xml"), IMG, Target::Part("xl/media/i.png".into())).unwrap(), "rId2");
    pkg.apply(plan).unwrap();
    assert_eq!(pkg.incoming_relationships("xl/media/i.png").unwrap().len(), 1);
}

/// m1: a staged big tree does not make later steps slow (shared, not
/// cloned per step). Generous bound; the regression was ~135 ms per step.
#[test]
fn steps_do_not_clone_staged_trees() {
    let mut big = String::from(r#"<worksheet xmlns="urn:x"><sheetData>"#);
    for i in 0..30_000 {
        big.push_str(&format!(r#"<row r="{i}"><c r="A{i}"><v>{i}</v></c></row>"#));
    }
    big.push_str("</sheetData></worksheet>");
    let mut pkg = Package::from_bytes(base(&[("xl/s.xml", &big), ("xl/media/i.png", "P")])).unwrap();
    let doc = pkg.xml_part("xl/s.xml").unwrap().clone();
    let mut plan = RegistryPlan::new(&pkg);
    plan.stage_part(&pkg, "xl/s.xml", doc).unwrap();
    let t = std::time::Instant::now();
    for _ in 0..200 {
        plan.add_relationship(&pkg, Some("xl/workbook.xml"), IMG, Target::Part("xl/media/i.png".into())).unwrap();
    }
    assert!(t.elapsed() < std::time::Duration::from_secs(5), "{:?}", t.elapsed());
}

/// M3: a relationships part whose zip entry cannot be read at all (CRC
/// mismatch) counts as mentioning every part.
#[test]
fn m3_unreadable_entry_fails_closed() {
    let r = rels(&[("rId1", IMG, "media/i.png")]);
    let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (n, c) in [("[Content_Types].xml", CT), ("_rels/.rels", ROOT), ("xl/workbook.xml", WB), ("xl/s.xml", "<s/>"), ("xl/_rels/s.xml.rels", r.as_str()), ("xl/media/i.png", "PNG")] {
        zw.start_file(n, stored).unwrap();
        zw.write_all(c.as_bytes()).unwrap();
    }
    let mut bytes = zw.finish().unwrap().into_inner();
    // corrupt the stored relationships text (its file name stays intact in the headers)
    let at = bytes.windows(r.len()).position(|w| w == r.as_bytes()).unwrap();
    let k = at + r.find("media").unwrap();
    bytes[k..k + 5].copy_from_slice(b"XXXXX");
    let pkg = Package::from_bytes(bytes).unwrap();
    assert!(pkg.read_part("xl/_rels/s.xml.rels").is_err(), "the entry must be unreadable for this test");
    assert!(pkg.check_remove_parts(&["xl/media/i.png"], &[IMG]).is_err());
    assert!(pkg.incoming_relationships("xl/media/i.png").is_err());
}

/// m5: a relationship appended earlier in the plan to an existing `.rels`
/// part can be removed again in the same plan.
#[test]
fn remove_a_relationship_appended_in_the_plan() {
    let r = rels(&[("rId1", IMG, "media/i.png")]);
    let mut pkg = Package::from_bytes(base(&[("xl/s.xml", "<s/>"), ("xl/_rels/s.xml.rels", &r), ("xl/media/i.png", "P")])).unwrap();
    let mut plan = RegistryPlan::new(&pkg);
    let id = plan.add_relationship(&pkg, Some("xl/s.xml"), COMMENTS, Target::Part("xl/media/i.png".into())).unwrap();
    plan.remove_relationship(&pkg, Some("xl/s.xml"), &id, &[COMMENTS]).unwrap();
    pkg.apply(plan).unwrap();
    let rs = pkg.relationships("xl/s.xml").unwrap();
    assert_eq!(rs.iter().count(), 1);
    assert!(rs.by_id(&id).is_none());
}

/// m2: registry parts parsed for planning use the package's parse options
/// (a raised depth limit lets a deep `.rels` be edited).
#[test]
fn planning_parses_with_the_package_options() {
    let depth = ooxml_core::DEFAULT_MAX_DEPTH + 20;
    let deep = format!("{}{}", "<x:e xmlns:x=\"urn:x\">".repeat(depth - 5), "</x:e>".repeat(depth - 5));
    let r = format!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{IMG}" Target="media/i.png"/>{deep}</Relationships>"#
    );
    let input = base(&[("xl/s.xml", "<s/>"), ("xl/_rels/s.xml.rels", &r), ("xl/media/i.png", "P")]);
    let mut options = ooxml_core::ParseOptions::default();
    options.max_depth = depth + 5;
    let pkg = Package::from_bytes_with(input, options).unwrap();
    pkg.check_add_relationship(Some("xl/s.xml"), IMG, Target::Part("xl/media/i.png".into())).unwrap();
}
