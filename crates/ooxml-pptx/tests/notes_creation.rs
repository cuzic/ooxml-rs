//! ADR-0016 §8: notes provisioning, its registry transaction and locality.

use std::assert_matches;
use std::path::Path;

use ooxml_core::{Package, PartNameRequest, QName, RegistryPlan, XmlDocument};
use ooxml_pptx::{Error, Presentation};
use ooxml_schema::pml;

const FRESH: &str = "python-pptx/shp-connector-props.pptx";
const EXISTING: &str = "python-pptx/sld-notes.pptx";
const MAIN: &str = "ppt/presentation.xml";
const MAIN_RELS: &str = "ppt/_rels/presentation.xml.rels";
const APP: &str = "docProps/app.xml";
const MASTER: &str = "ppt/notesMasters/notesMaster1.xml";
const THEME: &str = "ppt/theme/theme2.xml";
const NOTES: &str = "ppt/notesSlides/notesSlide1.xml";
const SLIDE: &str = "ppt/slides/slide1.xml";
const SLIDE_RELS: &str = "ppt/slides/_rels/slide1.xml.rels";
const LIST: &str = "<p:notesMasterIdLst><p:notesMasterId r:id=\"rId4\"/></p:notesMasterIdLst>";

fn fixture(name: &str) -> Package {
    Package::open(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../reference/fixtures/pptx")
            .join(name),
    )
    .unwrap()
}

fn xml(pkg: &Package, part: &str) -> String {
    String::from_utf8(pkg.read_part(part).unwrap()).unwrap()
}

fn replace(pkg: &mut Package, part: &str, from: &str, to: &str) {
    let original = xml(pkg, part);
    assert!(original.contains(from), "{part} is missing {from:?}");
    pkg.set_part(part, original.replacen(from, to, 1).into_bytes())
        .unwrap();
}

fn deck(pkg: &Package) -> Presentation {
    Presentation::from_bytes(pkg.to_bytes().unwrap()).unwrap()
}

fn target(pkg: &Package, source: &str, kind: &str) -> (String, String) {
    let rel_type =
        format!("http://schemas.openxmlformats.org/officeDocument/2006/relationships/{kind}");
    let rels = pkg.relationships(source).unwrap();
    let mut found = rels.by_type(&rel_type);
    let rel = found.next().unwrap();
    assert!(
        found.next().is_none(),
        "multiple {kind} relationships from {source}"
    );
    let part = rels.resolve(rel).unwrap();
    assert!(pkg.contains(&part));
    (rel.id.clone(), part)
}

fn assert_list(pkg: &Package, master: &str) {
    let (rid, target) = target(pkg, MAIN, "notesMaster");
    assert_eq!(target, master);
    let source = xml(pkg, MAIN);
    let doc = XmlDocument::parse(source.as_bytes()).unwrap();
    let root = doc.lens::<pml::Presentation>().unwrap();
    let list = root.child::<pml::NotesMasterIdList>().unwrap();
    let entry = list.child::<pml::NotesMasterId>().unwrap();
    assert_eq!(
        entry.attr(pml::NotesMasterId::ATTR_R_ID),
        Some(rid.as_str())
    );
}

fn assert_only_changed(before: &Package, after: &Package, exceptions: &[&str]) {
    for part in before.part_names().filter(|p| !exceptions.contains(p)) {
        assert_eq!(
            before.read_part(part).unwrap(),
            after.read_part(part).unwrap(),
            "{part}"
        );
    }
}

#[test]
fn creates_master_and_notes_in_one_edit_with_local_changes() {
    let original = fixture(FRESH);
    let mut p = deck(&original);
    p.notes_mut_or_create(0)
        .unwrap()
        .body_mut()
        .unwrap()
        .set_text("First line\nSecond line")
        .unwrap();
    let bytes = p.to_bytes().unwrap();
    let mut saved = Presentation::from_bytes(bytes.clone()).unwrap();
    assert_eq!(
        saved.notes(0).unwrap().unwrap().text(),
        "First line\nSecond line"
    );
    assert!(saved.notes(1).unwrap().is_none());
    saved.ensure_notes_slide(0).unwrap();
    assert_eq!(saved.to_bytes().unwrap(), bytes);

    let pkg = saved.package();
    assert_only_changed(
        &original,
        pkg,
        &["[Content_Types].xml", MAIN, MAIN_RELS, APP, SLIDE_RELS],
    );
    assert_eq!(
        xml(pkg, APP),
        xml(&original, APP).replacen("<Notes>0</Notes>", "<Notes>1</Notes>", 1)
    );
    assert_list(pkg, MASTER);
    let source = xml(pkg, MAIN);
    let doc = XmlDocument::parse(source.as_bytes()).unwrap();
    let root = doc.lens::<pml::Presentation>().unwrap();
    let list = root.child::<pml::NotesMasterIdList>().unwrap();
    let span = list.raw().span().unwrap();
    let master_list = doc
        .root()
        .elements()
        .find(|node| node.name().local() == "sldMasterIdLst")
        .unwrap();
    assert_eq!(master_list.span().unwrap().end(), span.start);
    let mut without_list = source;
    without_list.replace_range(span.start..span.end(), "");
    assert_eq!(without_list, xml(&original, MAIN));
    assert_eq!(target(pkg, MASTER, "theme").1, THEME);
    assert_eq!(target(pkg, SLIDE, "notesSlide").1, NOTES);
    assert_eq!(target(pkg, NOTES, "notesMaster").1, MASTER);
    assert_eq!(target(pkg, NOTES, "slide").1, SLIDE);

    let notes = XmlDocument::parse(&pkg.read_part(NOTES).unwrap()).unwrap();
    let root = notes.lens::<pml::NotesSlide>().unwrap();
    let tree = root
        .child::<pml::CommonSlideData>()
        .unwrap()
        .child::<pml::ShapeTree>()
        .unwrap();
    let placeholders: Vec<_> = tree
        .children::<pml::Shape>()
        .map(|shape| {
            let nv = shape.child::<pml::NonVisualShapeProperties>().unwrap();
            let id = nv.child::<pml::NonVisualDrawingProperties>().unwrap();
            let ph = nv
                .child::<pml::ApplicationNonVisualDrawingProperties>()
                .unwrap()
                .child::<pml::PlaceholderShape>()
                .unwrap();
            (
                id.attr(QName::new("", "id")).unwrap(),
                ph.attr(QName::new("", "type")).unwrap(),
                ph.attr(QName::new("", "idx")).unwrap(),
            )
        })
        .collect();
    assert_eq!(
        placeholders,
        [
            ("2", "sldImg", "2"),
            ("3", "body", "3"),
            ("4", "sldNum", "5")
        ]
    );
}

#[test]
fn relationship_only_repairs_list_and_reuses_master_for_new_notes() {
    let mut input = fixture(EXISTING);
    replace(&mut input, MAIN, LIST, "");
    let mut p = deck(&input);
    p.ensure_notes_slide(0).unwrap();
    let repaired = p.package();
    assert_list(repaired, MASTER);
    assert_eq!(xml(repaired, MAIN), xml(&fixture(EXISTING), MAIN));
    assert_eq!(repaired.part_names().count(), input.part_names().count());
    assert_only_changed(&input, repaired, &[MAIN]);

    // A deck with a master and an existing notes slide keeps both.
    let bytes = p.to_bytes().unwrap();
    p.ensure_notes_slide(0).unwrap();
    assert_eq!(p.to_bytes().unwrap(), bytes);
    p.ensure_notes_slide(1).unwrap();
    let saved = p.package();
    assert_list(saved, MASTER);
    for part in [MAIN_RELS, MASTER, THEME, NOTES] {
        assert_eq!(xml(saved, part), xml(&input, part), "{part}");
    }
    assert_eq!(
        target(saved, "ppt/notesSlides/notesSlide2.xml", "notesMaster").1,
        MASTER
    );
    assert_eq!(
        xml(saved, APP),
        xml(&input, APP).replace("<Notes>1</Notes>", "<Notes>2</Notes>")
    );

    // Repair and create can also happen together on a slide without notes.
    let mut missing_list = deck(&input);
    missing_list.ensure_notes_slide(1).unwrap();
    assert_eq!(missing_list.to_bytes().unwrap(), p.to_bytes().unwrap());
}

#[test]
fn numbered_parts_use_the_maximum_existing_number() {
    let mut input = fixture(FRESH);
    let template = fixture(EXISTING);
    let mut plan = RegistryPlan::new(&input);
    for (src, dst) in [
        (MASTER, "ppt/notesMasters/notesMaster8.xml"),
        (THEME, "ppt/theme/theme10.xml"),
    ] {
        plan.add_part(
            &input,
            PartNameRequest::Exact(dst.into()),
            template.content_type(src).unwrap(),
            template.read_part(src).unwrap(),
        )
        .unwrap();
    }
    input.apply(plan).unwrap();
    let mut p = deck(&input);
    p.ensure_notes_slide(0).unwrap();
    assert_list(p.package(), "ppt/notesMasters/notesMaster9.xml");
    assert_eq!(
        target(p.package(), "ppt/notesMasters/notesMaster9.xml", "theme").1,
        "ppt/theme/theme11.xml"
    );

    let mut input = Package::from_bytes(p.to_bytes().unwrap()).unwrap();
    let mut plan = RegistryPlan::new(&input);
    let orphan = "ppt/notesSlides/notesSlide5.xml";
    plan.add_part(
        &input,
        PartNameRequest::Exact(orphan.into()),
        input.content_type(NOTES).unwrap(),
        input.read_part(NOTES).unwrap(),
    )
    .unwrap();
    input.apply(plan).unwrap();
    let mut p = deck(&input);
    p.ensure_notes_slide(1).unwrap();
    assert_eq!(
        target(p.package(), "ppt/slides/slide2.xml", "notesSlide").1,
        "ppt/notesSlides/notesSlide6.xml"
    );
    for part in [
        "ppt/notesMasters/notesMaster8.xml",
        "ppt/theme/theme10.xml",
        NOTES,
        orphan,
    ] {
        assert_eq!(xml(p.package(), part), xml(&input, part));
    }
}

#[test]
fn refuses_notes_without_master_and_preserves_input() {
    let mut input = fixture(EXISTING);
    replace(
        &mut input,
        MAIN_RELS,
        "<Relationship Id=\"rId4\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesMaster\" Target=\"notesMasters/notesMaster1.xml\"/>",
        "",
    );
    let bytes = input.to_bytes().unwrap();
    let mut p = Presentation::from_bytes(bytes.clone()).unwrap();
    for index in [0, 1] {
        assert_matches!(p.ensure_notes_slide(index), Err(Error::NotesWithoutMaster));
        assert_matches!(p.notes_mut_or_create(index), Err(Error::NotesWithoutMaster));
        assert_eq!(p.to_bytes().unwrap(), bytes);
    }
}

#[test]
fn invalid_index_and_unusable_parts_are_byte_identical_on_error() {
    let mut external = fixture(EXISTING);
    replace(
        &mut external,
        MAIN_RELS,
        "Target=\"notesMasters/notesMaster1.xml\"",
        "Target=\"https://example.com/master\" TargetMode=\"External\"",
    );
    let mut dangling = fixture(EXISTING);
    replace(
        &mut dangling,
        MAIN_RELS,
        "Target=\"notesMasters/notesMaster1.xml\"",
        "Target=\"notesMasters/missing.xml\"",
    );
    let mut wrong_notes = fixture(EXISTING);
    wrong_notes
        .set_part(NOTES, xml(&wrong_notes, SLIDE).into_bytes())
        .unwrap();
    for (pkg, index) in [
        (external, 1),
        (dangling, 1),
        (wrong_notes, 0),
        (fixture(FRESH), 2),
    ] {
        let bytes = pkg.to_bytes().unwrap();
        let mut p = Presentation::from_bytes(bytes.clone()).unwrap();
        assert!(p.notes_mut_or_create(index).is_err());
        assert_eq!(p.to_bytes().unwrap(), bytes);
    }
}

#[test]
fn refuses_allocator_exhaustion_after_staging_without_changes() {
    let mut pkg = fixture(FRESH);
    let mut plan = RegistryPlan::new(&pkg);
    plan.add_part(
        &pkg,
        PartNameRequest::Exact("ppt/theme/theme18446744073709551615.xml".into()),
        pkg.content_type("ppt/theme/theme1.xml").unwrap(),
        pkg.read_part("ppt/theme/theme1.xml").unwrap(),
    )
    .unwrap();
    pkg.apply(plan).unwrap();
    let bytes = pkg.to_bytes().unwrap();
    let mut p = Presentation::from_bytes(bytes.clone()).unwrap();
    assert_matches!(p.ensure_notes_slide(0), Err(Error::Core(_)));
    assert_eq!(p.to_bytes().unwrap(), bytes);
}

#[test]
fn dangling_notes_relationship_is_refused_without_changes() {
    let mut pkg = fixture(FRESH);
    replace(
        &mut pkg,
        SLIDE_RELS,
        "</Relationships>",
        r#"<Relationship Id="rId99" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesSlide" Target="../notesSlides/notesSlide7.xml"/></Relationships>"#,
    );
    let before = pkg.to_bytes().unwrap();
    let mut p = Presentation::from_bytes(before.clone()).unwrap();
    assert_matches!(p.ensure_notes_slide(0), Err(Error::UnsupportedContent(ref message)) if message.contains("notesSlide relationship rId99"));
    assert_matches!(p.notes_mut_or_create(0), Err(Error::UnsupportedContent(ref message)) if message.contains("notesSlide relationship rId99"));
    assert_eq!(p.to_bytes().unwrap(), before);
}

#[test]
fn master_without_body_still_provides_editable_notes() {
    let mut pkg = fixture(EXISTING);
    replace(&mut pkg, MASTER, "type=\"body\"", "type=\"ftr\"");
    let mut p = deck(&pkg);
    p.notes_mut_or_create(1)
        .unwrap()
        .body_mut()
        .unwrap()
        .set_text("Editable body")
        .unwrap();
    let mut saved = Presentation::from_bytes(p.to_bytes().unwrap()).unwrap();
    let notes = saved.notes(1).unwrap().unwrap();
    assert_eq!(notes.text(), "Editable body");
    let sp = notes.body_shape().unwrap();
    let shape_properties = sp
        .raw()
        .elements()
        .find(|node| node.name().local() == "spPr")
        .unwrap();
    assert_eq!(shape_properties.elements().count(), 0);
    assert_eq!(
        saved.package().read_part(MASTER).unwrap(),
        pkg.read_part(MASTER).unwrap()
    );
}

#[test]
fn wrapped_master_list_is_refused_with_context_and_without_changes() {
    let mut pkg = fixture(EXISTING);
    let wrapped = format!(
        r#"<mc:AlternateContent xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><mc:Choice Requires="p">{LIST}</mc:Choice><mc:Fallback/></mc:AlternateContent>"#
    );
    replace(&mut pkg, MAIN, LIST, &wrapped);
    let before = pkg.to_bytes().unwrap();
    let mut p = Presentation::from_bytes(before.clone()).unwrap();
    assert_matches!(p.ensure_notes_slide(1), Err(Error::UnsupportedContent(ref message)) if message.contains("notesMasterIdLst inside mc:AlternateContent"));
    assert_eq!(p.to_bytes().unwrap(), before);
}
