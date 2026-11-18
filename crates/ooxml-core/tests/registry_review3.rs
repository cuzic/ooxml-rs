//! Regression tests for the round-3 review of PR #38 (docs/reviews/pr38-round3.md):
//! the surviving mutants R1, R3, R4, R15 (n10) and the nits n12, n13.

use std::assert_matches;
use std::io::{Cursor, Read, Write};

use ooxml_core::{
    Error, Package, ParseOptions, PartNameRequest, RegistryPlan, RegistryRefusal, RemoveRefusal,
    Target, XmlDocument,
};
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
const PLAIN: &str =
    r#"<s xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"/>"#;

fn rels(rows: &[(&str, &str, &str)]) -> String {
    let mut s = String::from(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
    );
    for (id, ty, t) in rows {
        let ext = if t.starts_with("http") {
            r#" TargetMode="External""#
        } else {
            ""
        };
        s.push_str(&format!(
            r#"<Relationship Id="{id}" Type="{ty}" Target="{t}"{ext}/>"#
        ));
    }
    s.push_str("</Relationships>");
    s
}

fn base(extra: &[(&str, &str)]) -> Vec<u8> {
    let mut v = vec![
        ("[Content_Types].xml", CT),
        ("_rels/.rels", ROOT),
        ("xl/workbook.xml", WB),
    ];
    v.extend_from_slice(extra);
    build(&v)
}

/// R3/R4: when a relationship is retargeted twice in one plan the last
/// target wins, for removal (R3) and for the view (R4): a→b→a, then
/// removing b is allowed and removing a is refused.
#[test]
fn r3_r4_last_retarget_wins() {
    let r = rels(&[("rId1", IMG, "media/a.png")]);
    let pkg = Package::from_bytes(base(&[
        ("xl/s.xml", EMBED),
        ("xl/_rels/s.xml.rels", &r),
        ("xl/media/a.png", "A"),
        ("xl/media/b.png", "B"),
    ]))
    .unwrap();
    let a_b_a = |pkg: &Package| {
        let mut plan = RegistryPlan::new(pkg);
        plan.set_relationship_target(pkg, Some("xl/s.xml"), "rId1", "media/b.png")
            .unwrap();
        plan.set_relationship_target(pkg, Some("xl/s.xml"), "rId1", "media/a.png")
            .unwrap();
        plan
    };
    // the view
    let plan = a_b_a(&pkg);
    assert_eq!(
        plan.incoming_relationships(&pkg, "xl/media/a.png")
            .unwrap()
            .len(),
        1
    );
    assert!(
        plan.incoming_relationships(&pkg, "xl/media/b.png")
            .unwrap()
            .is_empty()
    );
    // removal of b: nothing points at it any more
    let mut plan = a_b_a(&pkg);
    assert!(
        plan.remove_parts(&pkg, &["xl/media/b.png"], &[IMG])
            .unwrap()
            .relationships
            .is_empty()
    );
    let mut out = Package::from_bytes(pkg.to_bytes().unwrap()).unwrap();
    let mut plan2 = a_b_a(&out);
    plan2
        .remove_parts(&out, &["xl/media/b.png"], &[IMG])
        .unwrap();
    out.apply(plan2).unwrap();
    let rs = out.relationships("xl/s.xml").unwrap();
    assert_eq!(
        rs.resolve(rs.by_id("rId1").unwrap()).as_deref(),
        Some("xl/media/a.png")
    );
    drop(plan);
    // removal of a: rId1 (named by r:embed) points at it again
    let mut plan = a_b_a(&pkg);
    assert_matches!(
        refusal(
            plan.remove_parts(&pkg, &["xl/media/a.png"], &[IMG])
                .unwrap_err()
        ),
        RegistryRefusal::Remove(RemoveRefusal::StillReferenced { .. })
    );
}

/// R15: the `Id`s of external and fragment-only relationships removed with
/// a part count for the high-water of a part re-created under its name.
#[test]
fn r15_external_and_fragment_ids_count() {
    let r = rels(&[
        ("rId1", IMG, "media/a.png"),
        ("rId7", "urn:x:link", "https://e.example/x"),
        ("rId9", "urn:x:loc", "#Sheet2!A1"),
    ]);
    let wbr = rels(&[("rId1", "urn:x", "s.xml")]);
    let pkg = Package::from_bytes(base(&[
        ("xl/s.xml", PLAIN),
        ("xl/_rels/s.xml.rels", &r),
        ("xl/media/a.png", "A"),
        ("xl/_rels/workbook.xml.rels", &wbr),
    ]))
    .unwrap();
    let mut plan = RegistryPlan::new(&pkg);
    plan.remove_parts(&pkg, &["xl/s.xml"], &["urn:x"]).unwrap();
    plan.add_part(
        &pkg,
        PartNameRequest::Exact("xl/s.xml".into()),
        "application/xml",
        PLAIN.as_bytes().to_vec(),
    )
    .unwrap();
    assert_eq!(
        plan.add_relationship(
            &pkg,
            Some("xl/s.xml"),
            IMG,
            Target::Part("xl/media/a.png".into())
        )
        .unwrap(),
        "rId10"
    );
    // and content staged for it may not name the external one's Id
    let old = br#"<s xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><h r:id="rId7"/></s>"#;
    assert_matches!(
        refusal(plan.stage_part(&pkg, "xl/s.xml", old.to_vec()).unwrap_err()),
        RegistryRefusal::ConflictsWithPlan
    );
}

/// R1: an unedited tree is kept as the original entry only when that entry
/// can be read and equals its source; an entry that cannot be read (over
/// the size limit) means the staged tree is detached and saved.
#[test]
fn r1_unreadable_original_detaches() {
    let big = format!(
        r#"<s xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><!--{}--></s>"#,
        "x".repeat(4000)
    );
    let input = base(&[("xl/s.xml", &big)]);
    let mut options = ParseOptions::default();
    options.max_part_bytes = 1000;
    let mut pkg = Package::from_bytes_with(input, options).unwrap();
    assert!(
        pkg.read_part("xl/s.xml").is_err(),
        "the original entry is over the limit"
    );
    let mut plan = RegistryPlan::new(&pkg);
    plan.stage_part(
        &pkg,
        "xl/s.xml",
        XmlDocument::parse(PLAIN.as_bytes()).unwrap(),
    )
    .unwrap();
    pkg.apply(plan).unwrap();
    assert_eq!(
        text(&pkg.to_bytes().unwrap(), "xl/s.xml"),
        PLAIN,
        "the staged tree is saved"
    );
}

/// n12: a part target accepts one fragment delimiter. A fragment-only
/// location needs text after its delimiter and can contain literal `#`.
#[test]
fn n12_fragments() {
    let r = rels(&[
        ("rId1", IMG, "media/a.png"),
        ("rId2", "urn:x:loc", "#Sheet1!A1"),
        ("rId3", "urn:x:link", "https://e.example/x"),
    ]);
    let input = base(&[
        ("xl/s.xml", PLAIN),
        ("xl/_rels/s.xml.rels", &r),
        ("xl/media/a.png", "A"),
        ("xl/media/b.png", "B"),
    ]);
    let mut pkg = Package::from_bytes(input.clone()).unwrap();
    for (id, target) in [
        ("rId1", "media/b.png#a#b"),
        ("rId2", "#"),
        ("rId3", "https://e.example/x#a#b"),
    ] {
        assert_matches!(
            refusal(
                pkg.set_relationship_target(Some("xl/s.xml"), id, target)
                    .unwrap_err()
            ),
            RegistryRefusal::InvalidTarget,
            "{target}"
        );
        assert_eq!(pkg.to_bytes().unwrap(), input, "{target}");
    }
    let location = "#'a#b c'!A1";
    pkg.set_relationship_target(Some("xl/s.xml"), "rId2", location)
        .unwrap();
    let reopened = Package::from_bytes(pkg.to_bytes().unwrap()).unwrap();
    let rels = reopened.relationships("xl/s.xml").unwrap();
    assert_eq!(rels.by_id("rId2").unwrap().target, location);
}

/// n13: the package relationships part is found whatever its case, as the
/// registry finds it.
#[test]
fn n13_package_rels_any_case() {
    let input = build(&[
        ("[Content_Types].xml", CT),
        ("_rels/.RELS", ROOT),
        ("xl/workbook.xml", WB),
    ]);
    let mut pkg = Package::from_bytes(input).unwrap();
    assert_eq!(pkg.package_relationships().iter().count(), 1);
    assert_eq!(pkg.main_document_part().as_deref(), Some("xl/workbook.xml"));
    pkg.add_relationship(None, "urn:x:custom", Target::Part("xl/workbook.xml".into()))
        .unwrap();
    assert_eq!(pkg.package_relationships().iter().count(), 2);
    let again = Package::from_bytes(pkg.to_bytes().unwrap()).unwrap();
    assert_eq!(again.package_relationships().iter().count(), 2);
}
