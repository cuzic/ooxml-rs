use std::collections::HashSet;
use std::io::Cursor;
use std::path::Path;

use calamine::Reader;
use ooxml_core::{Package, QName, XmlDocument};
use ooxml_xlsx::{Anchor, CellIndex, EditAs, Error, Image, Size, Workbook};

const BLUE: &[u8] = include_bytes!("images/blue.png");
const ORANGE: &[u8] = include_bytes!("images/orange.png");
const XDR: &str = "http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing";
const SHEET: &str = "xl/worksheets/sheet1.xml";
const DRAWING: &str = "xl/drawings/drawing1.xml";

fn at(row: u32, col: u32) -> CellIndex {
    CellIndex::new(row, col).unwrap()
}

fn put(wb: &mut Workbook) -> ooxml_xlsx::Result<u32> {
    wb.worksheet()?.insert_image(
        Anchor::one_cell(at(1, 2), Size::Natural),
        &Image::from_bytes(BLUE)?,
    )
}

fn unchanged_except(before: &Package, after: &Package, allowed: &[&str]) {
    for name in before.part_names().chain(after.part_names()) {
        if !allowed.contains(&name) {
            assert_eq!(
                before.read_part(name).unwrap(),
                after.read_part(name).unwrap(),
                "{name}"
            );
        }
    }
}

#[test]
fn replacing_image_bytes_preserves_drawings_and_reindexes_media() {
    let mut wb = Workbook::new().unwrap();
    put(&mut wb).unwrap();
    let original = wb.to_bytes().unwrap();
    let part = "xl/media/image1.png";
    assert!(wb.replace_image_part(part, b"invalid".to_vec()).is_err());
    assert!(wb.replace_image_part(DRAWING, ORANGE.to_vec()).is_err());
    assert_eq!(wb.to_bytes().unwrap(), original);

    wb.replace_image_part(part, ORANGE.to_vec()).unwrap();
    let before = Package::from_bytes(original).unwrap();
    let after = Package::from_bytes(wb.to_bytes().unwrap()).unwrap();
    unchanged_except(&before, &after, &[part]);
    assert_eq!(after.read_part(part).unwrap(), ORANGE);

    wb.worksheet().unwrap().insert_image(
        Anchor::one_cell(at(4, 2), Size::Natural),
        &Image::from_bytes(ORANGE).unwrap(),
    ).unwrap();
    let images = wb.worksheet().unwrap().images().unwrap();
    assert_eq!(images.len(), 2);
    assert_eq!(images[0].media.as_ref().unwrap().part, images[1].media.as_ref().unwrap().part);
}

#[test]
fn drawing_registries_locality_and_independent_readback() {
    let mut wb = Workbook::new().unwrap();
    wb.worksheet()
        .unwrap()
        .cell_mut(0, 0)
        .unwrap()
        .set_number(42.0)
        .unwrap();
    let input = Package::from_bytes(wb.to_bytes().unwrap()).unwrap();
    assert_eq!(put(&mut wb).unwrap(), 2);
    let first = Package::from_bytes(wb.to_bytes().unwrap()).unwrap();
    unchanged_except(
        &input,
        &first,
        &[
            SHEET,
            DRAWING,
            "[Content_Types].xml",
            "xl/worksheets/_rels/sheet1.xml.rels",
            "xl/drawings/_rels/drawing1.xml.rels",
            "xl/media/image1.png",
        ],
    );
    let inserted_sheet = String::from_utf8(first.read_part(SHEET).unwrap()).unwrap();
    let drawing = inserted_sheet.find("<drawing ").unwrap();
    let end = drawing + inserted_sheet[drawing..].find("/>").unwrap() + 2;
    assert_eq!(
        format!("{}{}", &inserted_sheet[..drawing], &inserted_sheet[end..]).as_bytes(),
        input.read_part(SHEET).unwrap()
    );
    assert_eq!(
        first
            .content_types()
            .defaults()
            .iter()
            .find(|(ext, _)| ext == "png")
            .unwrap()
            .1,
        "image/png"
    );
    assert_eq!(first.relationships(SHEET).unwrap().iter().count(), 1);
    let first_anchor = first.read_part(DRAWING).unwrap();
    let old_rels = first
        .read_part("xl/drawings/_rels/drawing1.xml.rels")
        .unwrap();

    let mut ws = wb.worksheet().unwrap();
    let second = Anchor::two_cell(at(3, 1), at(5, 3)).edit_as(EditAs::Absolute);
    assert_eq!(
        ws.insert_image(
            second,
            &Image::from_bytes(ORANGE).unwrap().alt_text("logo _x0000_")
        )
        .unwrap(),
        3
    );
    assert_eq!(
        ws.insert_image(
            Anchor::absolute(0, 1, 200, 300),
            &Image::from_bytes(BLUE).unwrap()
        )
        .unwrap(),
        4
    );
    let images = ws.images().unwrap();
    assert_eq!(images.len(), 3);
    assert_eq!(images[1].anchor, second);
    assert_eq!(images[1].alt_text.as_deref(), Some("logo _x0000_"));
    assert_eq!(images[0].media, images[2].media);

    let output = wb.to_bytes().unwrap();
    let after = Package::from_bytes(output.clone()).unwrap();
    unchanged_except(
        &first,
        &after,
        &[
            DRAWING,
            "xl/drawings/_rels/drawing1.xml.rels",
            "xl/media/image2.png",
        ],
    );
    let new_anchor = after.read_part(DRAWING).unwrap();
    assert!(new_anchor.starts_with(first_anchor.strip_suffix(b"</xdr:wsDr>").unwrap()));
    let old_registry = XmlDocument::parse(&old_rels).unwrap();
    let new_registry = XmlDocument::parse(
        &after
            .read_part("xl/drawings/_rels/drawing1.xml.rels")
            .unwrap(),
    )
    .unwrap();
    let first_rel = old_registry
        .root()
        .elements()
        .next()
        .unwrap()
        .span()
        .unwrap();
    let retained_rel = new_registry
        .root()
        .elements()
        .next()
        .unwrap()
        .span()
        .unwrap();
    assert_eq!(
        old_registry.source()[first_rel.start..first_rel.end()],
        new_registry.source()[retained_rel.start..retained_rel.end()]
    );
    assert_eq!(after.relationships(DRAWING).unwrap().iter().count(), 2);

    let reader: calamine::Xlsx<_> = calamine::open_workbook_from_rs(Cursor::new(&output)).unwrap();
    let mut pictures = reader.pictures_with_metadata();
    pictures.sort_unstable_by_key(|picture| (picture.row, picture.col));
    assert_eq!(pictures.len(), 3);
    for (picture, (row, col, bytes)) in
        pictures
            .iter()
            .zip([(0, 0, BLUE), (1, 2, BLUE), (3, 1, ORANGE)])
    {
        assert_eq!((picture.row, picture.col), (row, col));
        assert_eq!(picture.sheet_name, "Sheet1");
        assert_eq!(picture.extension, "png");
        assert_eq!(picture.data, bytes);
    }

    if let Some(out) = std::env::var_os("OOXML_P5A_ORACLE") {
        std::fs::write(out, &output).unwrap();
    }

    let mut wb = Workbook::from_bytes(output).unwrap();
    let mut ws = wb.worksheet().unwrap();
    ws.remove_image(4).unwrap();
    assert_eq!(ws.images().unwrap().len(), 2);
    assert!(wb.package().contains("xl/media/image1.png"));
    wb.worksheet().unwrap().remove_image(2).unwrap();
    assert!(!wb.package().contains("xl/media/image1.png"));
    assert_eq!(put(&mut wb).unwrap(), 5);
    assert!(wb.package().contains("xl/media/image3.png"));
}

#[test]
fn rich_value_images_use_the_metadata_chain() {
    let bytes = include_bytes!(
        "../../../reference/fixtures/xlsx/calamine/pictures_in_cell_and_over_cell.xlsx"
    );
    let mut wb = Workbook::from_bytes(bytes.as_slice()).unwrap();
    let pkg = Package::from_bytes(bytes.as_slice()).unwrap();
    let baseline = wb.to_bytes().unwrap();
    let ws = wb.worksheet().unwrap();
    let images: Vec<_> = ws
        .cells()
        .filter_map(|cell| cell.unwrap().image().unwrap())
        .collect();
    assert_eq!(images.len(), 2);
    assert_eq!(
        images[0].media.bytes,
        pkg.read_part("xl/media/image1.png").unwrap()
    );
    assert_eq!(
        images[1].media.bytes,
        pkg.read_part("xl/media/image2.png").unwrap()
    );
    assert_eq!(images[1].alt_text.as_deref(), Some("Some text"));
    assert!(wb.to_bytes().unwrap() == baseline);
}

#[test]
fn unresolved_rich_value_preserves_the_workbook() {
    let bytes = include_bytes!(
        "../../../reference/fixtures/xlsx/calamine/pictures_in_cell_and_over_cell.xlsx"
    );
    let mut pkg = Package::from_bytes(bytes.as_slice()).unwrap();
    let name = "xl/richData/rdrichvalue.xml";
    let values = String::from_utf8(pkg.read_part(name).unwrap()).unwrap();
    pkg.set_part(name, values.replace("s=\"0\"", "s=\"999\"").into_bytes())
        .unwrap();
    let baseline = pkg.to_bytes().unwrap();
    let mut wb = Workbook::from_bytes(baseline.clone()).unwrap();
    let ws = wb.worksheet().unwrap();
    let first_image = ws
        .cells()
        .map(Result::unwrap)
        .find(|cell| cell.raw().attr(QName::new("", "vm")) == Some("1"))
        .unwrap();
    std::assert_matches!(
        first_image.image(),
        Err(Error::UnresolvedRichValue {
            step: "rich value structure"
        })
    );
    assert!(wb.to_bytes().unwrap() == baseline);
}

#[test]
fn equivalent_opc_targets_reuse_the_existing_relationship() {
    let mut wb = Workbook::new().unwrap();
    put(&mut wb).unwrap();
    let mut pkg = Package::from_bytes(wb.to_bytes().unwrap()).unwrap();
    let rels = "xl/drawings/_rels/drawing1.xml.rels";
    let xml = String::from_utf8(pkg.read_part(rels).unwrap()).unwrap();
    pkg.set_part(rels, xml.replace("image1.png", "%69mage1.png").into_bytes())
        .unwrap();
    let mut wb = Workbook::from_bytes(pkg.to_bytes().unwrap()).unwrap();
    assert_eq!(put(&mut wb).unwrap(), 3);
    assert_eq!(
        wb.package().relationships(DRAWING).unwrap().iter().count(),
        1
    );
    let images = wb.worksheet().unwrap().images().unwrap();
    assert_eq!(
        images[0].media.as_ref().unwrap().part,
        "xl/media/image1.png"
    );
    assert_eq!(images[0].media, images[1].media);
}

#[test]
fn refusals_keep_every_byte() {
    let baseline = Workbook::new().unwrap().to_bytes().unwrap();
    let mut wb = Workbook::from_bytes(baseline.clone()).unwrap();
    let mut ws = wb.worksheet().unwrap();
    let image = Image::from_bytes(BLUE).unwrap();
    std::assert_matches!(
        ws.insert_image(Anchor::Unresolved, &image),
        Err(Error::ImageAnchor)
    );
    std::assert_matches!(
        ws.insert_image(Anchor::absolute(0, 0, 0, 1), &image),
        Err(Error::ImageSize)
    );
    let long = image.clone().alt_text("🌱".repeat(128));
    std::assert_matches!(
        ws.insert_image(Anchor::one_cell(at(0, 0), Size::Natural), &long),
        Err(Error::InvalidSetting { .. })
    );
    std::assert_matches!(ws.remove_image(900), Err(Error::NoSuchImage(900)));
    assert_eq!(wb.to_bytes().unwrap(), baseline);
}

#[cfg(feature = "fault-injection")]
#[test]
fn registry_order_and_post_apply_poison() {
    use ooxml_xlsx::ApplyStep;
    let baseline = Workbook::new().unwrap().to_bytes().unwrap();
    let mut wb = Workbook::from_bytes(baseline.clone()).unwrap();
    wb.inject_apply_failure(ApplyStep::StaleRegistryPlan);
    std::assert_matches!(
        put(&mut wb),
        Err(Error::Core(ooxml_core::Error::RegistryRefused {
            reason: ooxml_core::RegistryRefusal::StalePlan,
            ..
        }))
    );
    assert_eq!(wb.to_bytes().unwrap(), baseline);
    assert_eq!(put(&mut wb).unwrap(), 2);

    for panic in [false, true] {
        let mut wb = Workbook::from_bytes(baseline.clone()).unwrap();
        if panic {
            wb.inject_apply_panic(ApplyStep::ImageDrawing);
            assert!(
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| put(&mut wb))).is_err()
            );
        } else {
            wb.inject_apply_failure(ApplyStep::ImageDrawing);
            std::assert_matches!(put(&mut wb), Err(Error::Poisoned(_)));
        }
        assert!(
            wb.package().contains(DRAWING),
            "the registry is applied before the worksheet insertion"
        );
        std::assert_matches!(wb.to_bytes(), Err(Error::Poisoned(_)));
    }
}

#[test]
fn svg_and_shared_media_survive_until_the_last_reference() {
    let mut wb = Workbook::new().unwrap();
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="12"/>"#;
    let image = Image::from_svg(svg.as_slice(), BLUE)
        .unwrap()
        .hyperlink("https://example.com/");
    let anchor = Anchor::one_cell(at(2, 2), Size::Natural);
    let id = wb
        .worksheet()
        .unwrap()
        .insert_image(anchor, &image)
        .unwrap();
    wb.add_sheet("Other", 1).unwrap();
    let other = wb
        .worksheet_by_name("Other")
        .unwrap()
        .insert_image(anchor, &image)
        .unwrap();
    let mut wb = Workbook::from_bytes(wb.to_bytes().unwrap()).unwrap();
    let all = wb.worksheet().unwrap().images().unwrap();
    assert_eq!(all[0].svg.as_ref().unwrap().bytes, svg);
    assert_eq!(
        wb.worksheet_by_name("Other").unwrap().images().unwrap()[0].svg,
        all[0].svg
    );
    wb.worksheet().unwrap().remove_image(id).unwrap();
    assert!(wb.package().contains("xl/media/image1.png"));
    assert!(wb.package().contains("xl/media/image1.svg"));
    wb.worksheet_by_name("Other")
        .unwrap()
        .remove_image(other)
        .unwrap();
    assert!(!wb.package().contains("xl/media/image1.png"));
    assert!(!wb.package().contains("xl/media/image1.svg"));
}

#[test]
fn poi_image_sweep() {
    let Some(root) = std::env::var_os("OOXML_P5A_POI") else {
        return;
    };
    let image = Image::from_bytes(BLUE).unwrap();
    let anchor = Anchor::one_cell(at(0, 0), Size::Natural);
    let (mut inputs, mut opened, mut sheets, mut accepted, mut refused, mut no_worksheet) =
        (0, 0, 0, 0, 0, 0);
    for entry in std::fs::read_dir(Path::new(&root)).unwrap() {
        let path = entry.unwrap().path();
        if !path.is_file() {
            continue;
        }
        inputs += 1;
        let Ok(mut wb) = Workbook::open(&path) else {
            continue;
        };
        opened += 1;
        let n = wb.sheet_names().count();
        let mut any = false;
        let mut edits = Vec::new();
        for index in 0..n {
            if wb.worksheet_by_index(index).is_err() {
                continue;
            }
            sheets += 1;
            any = true;
            let before = wb.to_bytes().unwrap();
            let mut ws = wb.worksheet_by_index(index).unwrap();
            match ws.insert_image(anchor, &image) {
                Ok(id) => {
                    accepted += 1;
                    edits.push((index, id));
                }
                Err(error) => {
                    refused += 1;
                    eprintln!(
                        "{} sheet {index}: {error}",
                        path.file_name().unwrap().to_string_lossy()
                    );
                    assert_eq!(wb.to_bytes().unwrap(), before, "{}", path.display());
                }
            }
        }
        if !any {
            no_worksheet += 1;
        }
        let mut reopened = Workbook::from_bytes(wb.to_bytes().unwrap())
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        for (index, id) in edits {
            let images = reopened
                .worksheet_by_index(index)
                .unwrap()
                .images()
                .unwrap();
            assert_eq!(
                images.iter().filter(|image| image.id == Some(id)).count(),
                1,
                "{}",
                path.display()
            );
            assert_eq!(images.last().unwrap().media.as_ref().unwrap().bytes, BLUE);
        }
        for part in reopened.package().part_names().filter(|part| {
            reopened.package().content_type(part)
                == Some("application/vnd.openxmlformats-officedocument.drawing+xml")
        }) {
            let Ok(doc) = XmlDocument::parse(&reopened.package().read_part(part).unwrap()) else {
                continue;
            };
            let mut nodes = vec![doc.root()];
            let mut seen = HashSet::new();
            while let Some(node) = nodes.pop() {
                nodes.extend(node.elements());
                if node.is(QName::new(XDR, "cNvPr"))
                    && let Some(id) = node.attr(QName::new("", "id"))
                    && let Ok(id) = id.parse::<u32>()
                {
                    assert!(
                        seen.insert(id),
                        "{}: {part}: duplicate id {id}",
                        path.display()
                    );
                }
            }
        }
    }
    assert!(inputs > 0 && accepted > 0);
    eprintln!(
        "POI image sweep: {inputs} files, {opened} opened, {no_worksheet} without worksheets, {sheets} worksheets, {accepted} inserted, {refused} refused"
    );
}

const MC: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";
const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const VML: &str = "xl/drawings/vmlDrawing1.vml";

fn with_vml(vml: &str) -> Package {
    use ooxml_core::{PartNameRequest, Target};
    let mut wb = Workbook::new().unwrap();
    put(&mut wb).unwrap();
    let mut pkg = Package::from_bytes(wb.to_bytes().unwrap()).unwrap();
    pkg.add_part(
        PartNameRequest::Exact(VML.into()),
        "application/vnd.openxmlformats-officedocument.vmlDrawing",
        vml.as_bytes().to_vec(),
    )
    .unwrap();
    let link = pkg
        .add_relationship(
            Some(SHEET),
            &format!("{REL}/vmlDrawing"),
            Target::Part(VML.into()),
        )
        .unwrap();
    let sheet = String::from_utf8(pkg.read_part(SHEET).unwrap()).unwrap();
    let insert = format!(
        "<legacyDrawingHF xmlns:r=\"{REL}\" r:id=\"{link}\"/><mc:AlternateContent xmlns:mc=\"{MC}\"><mc:Fallback><controls><control shapeId=\"201\"/></controls></mc:Fallback></mc:AlternateContent>"
    );
    pkg.set_part(
        SHEET,
        sheet
            .replace("</worksheet>", &format!("{insert}</worksheet>"))
            .into_bytes(),
    )
    .unwrap();
    let drawing = String::from_utf8(pkg.read_part(DRAWING).unwrap()).unwrap();
    let start = drawing.find("<xdr:oneCellAnchor").unwrap();
    let end = drawing.find("</xdr:wsDr>").unwrap();
    let group = drawing[start..end]
        .replace(" id=\"2\"", " id=\"500\"")
        .replace("<xdr:pic>", "<xdr:grpSp><xdr:pic>")
        .replace("</xdr:pic>", "</xdr:pic></xdr:grpSp>");
    pkg.set_part(DRAWING, format!("{}<mc:AlternateContent xmlns:mc=\"{MC}\"><mc:Fallback>{group}</mc:Fallback></mc:AlternateContent>{}", &drawing[..start], &drawing[end..]).into_bytes()).unwrap();
    pkg
}

#[test]
fn allocator_reads_unselected_drawing_branches_controls_and_vml() {
    const VML_NS: &str = "urn:schemas-microsoft-com:vml";
    const O: &str = "urn:schemas-microsoft-com:office:office";
    for (vml, expected) in [
        (
            format!(
                "<xml xmlns:v=\"{VML_NS}\" xmlns:alias=\"{O}\"><v:shape id=\"Button\" alias:spid=\"_x0000_s900\"/></xml>"
            ),
            901,
        ),
        (
            format!(
                "<xml xmlns:v=\"{VML_NS}\" xmlns:o=\"{O}\"><v:shape id=\"Button\" o:spid=\"_x0000_s902\"><br></v:shape></xml>"
            ),
            903,
        ),
        (
            format!("<xml xmlns:v=\"{VML_NS}\"><v:shape id=\"_x0000_s400\"/></xml>"),
            501,
        ),
    ] {
        let pkg = with_vml(&vml);
        let baseline = pkg.to_bytes().unwrap();
        let mut wb = Workbook::from_bytes(baseline.clone()).unwrap();
        let mut ws = wb.worksheet().unwrap();
        assert_eq!(ws.images().unwrap()[0].anchor, Anchor::Unresolved);
        std::assert_matches!(ws.remove_image(500), Err(Error::NoSuchImage(500)));
        assert!(wb.to_bytes().unwrap() == baseline);
        assert_eq!(put(&mut wb).unwrap(), expected);
        assert_eq!(wb.package().read_part(VML).unwrap(), vml.as_bytes());
        let before = pkg.read_part(DRAWING).unwrap();
        let after = wb.package().read_part(DRAWING).unwrap();
        assert!(after.starts_with(before.strip_suffix(b"</xdr:wsDr>").unwrap()));
    }
}

#[test]
fn container_and_registry_refusals_are_atomic() {
    let mut wb = Workbook::new().unwrap();
    put(&mut wb).unwrap();
    let mut damaged_drawing = Package::from_bytes(wb.to_bytes().unwrap()).unwrap();
    damaged_drawing
        .set_part(DRAWING, b"<broken>".to_vec())
        .unwrap();
    let mut max_id = with_vml("<xml/>");
    let xml = String::from_utf8(max_id.read_part(DRAWING).unwrap()).unwrap();
    max_id
        .set_part(
            DRAWING,
            xml.replace("id=\"500\"", "id=\"4294967295\"").into_bytes(),
        )
        .unwrap();

    for pkg in [damaged_drawing, with_vml("<xml><br></xml>"), max_id] {
        let baseline = pkg.to_bytes().unwrap();
        let mut wb = Workbook::from_bytes(baseline.clone()).unwrap();
        std::assert_matches!(put(&mut wb), Err(Error::PartNotEditable { .. }));
        assert!(wb.to_bytes().unwrap() == baseline);
    }

    let mut pkg = Package::from_bytes(Workbook::new().unwrap().to_bytes().unwrap()).unwrap();
    let sheet = String::from_utf8(pkg.read_part(SHEET).unwrap()).unwrap();
    let wrapper = format!(
        "<mc:AlternateContent xmlns:mc=\"{MC}\"><mc:Fallback><drawing xmlns:r=\"{REL}\" r:id=\"rId1\"/></mc:Fallback></mc:AlternateContent>"
    );
    pkg.set_part(
        SHEET,
        sheet
            .replace("</worksheet>", &format!("{wrapper}</worksheet>"))
            .into_bytes(),
    )
    .unwrap();
    let baseline = pkg.to_bytes().unwrap();
    let mut wb = Workbook::from_bytes(baseline.clone()).unwrap();
    std::assert_matches!(
        put(&mut wb),
        Err(Error::Core(ooxml_core::Error::UnresolvedSibling(_)))
    );
    assert!(wb.to_bytes().unwrap() == baseline);

    let baseline = Workbook::new().unwrap().to_bytes().unwrap();
    let mut wb = Workbook::from_bytes(baseline.clone()).unwrap();
    let image = Image::from_bytes(BLUE).unwrap().hyperlink("bad\0target");
    let result = wb
        .worksheet()
        .unwrap()
        .insert_image(Anchor::one_cell(at(0, 0), Size::Natural), &image);
    std::assert_matches!(
        result,
        Err(Error::Core(ooxml_core::Error::RegistryRefused { .. }))
    );
    assert!(wb.to_bytes().unwrap() == baseline);
}

#[test]
fn content_type_conflict_uses_an_override() {
    let mut pkg = Package::from_bytes(Workbook::new().unwrap().to_bytes().unwrap()).unwrap();
    pkg.add_default_content_type("png", "application/octet-stream")
        .unwrap();
    let mut wb = Workbook::from_bytes(pkg.to_bytes().unwrap()).unwrap();
    put(&mut wb).unwrap();
    assert_eq!(
        wb.package().content_type("xl/media/image1.png"),
        Some("image/png")
    );
    assert_eq!(
        wb.package()
            .content_types()
            .defaults()
            .iter()
            .find(|(ext, _)| ext == "png")
            .unwrap()
            .1,
        "application/octet-stream"
    );
    wb.worksheet().unwrap().remove_image(2).unwrap();
    assert!(
        !wb.package()
            .content_types()
            .overrides()
            .iter()
            .any(|(name, _)| name.ends_with("image1.png"))
    );
}

#[test]
fn removing_background_preserves_shared_drawing_media() {
    let mut wb = Workbook::new().unwrap();
    put(&mut wb).unwrap();
    let mut sheet = wb.worksheet().unwrap();
    let shared = sheet.set_background_image(&Image::from_bytes(BLUE).unwrap()).unwrap();
    assert_eq!(sheet.images().unwrap()[0].media.as_ref().unwrap().part, shared);
    assert!(sheet.remove_background_image().unwrap());
    assert!(sheet.background_image().unwrap().is_none());
    assert_eq!(sheet.images().unwrap()[0].media.as_ref().unwrap().bytes, BLUE);

    let dedicated = sheet.set_background_image(&Image::from_bytes(ORANGE).unwrap()).unwrap();
    assert!(sheet.remove_background_image().unwrap());
    let after = wb.to_bytes().unwrap();
    assert!(!wb.worksheet().unwrap().remove_background_image().unwrap());
    assert_eq!(wb.to_bytes().unwrap(), after);
    let package = Package::from_bytes(after).unwrap();
    assert!(package.part_names().any(|part| part == shared));
    assert!(!package.part_names().any(|part| part == dedicated));
}

#[test]
fn removing_background_with_invalid_relationship_refuses_atomically() {
    let mut wb = Workbook::new().unwrap();
    wb.worksheet().unwrap().set_background_image(&Image::from_bytes(BLUE).unwrap()).unwrap();
    let mut package = Package::from_bytes(wb.to_bytes().unwrap()).unwrap();
    let sheet = String::from_utf8(package.read_part(SHEET).unwrap()).unwrap();
    let (prefix, picture) = sheet.split_once("<picture ").unwrap();
    let (picture, suffix) = picture.split_once("/>").unwrap();
    let (attributes, _) = picture.split_once("id=\"").unwrap();
    package.set_part(SHEET, format!("{prefix}<picture {attributes}id=\"missing\"/>{suffix}").into_bytes()).unwrap();
    let original = package.to_bytes().unwrap();
    let mut wb = Workbook::from_bytes(original.clone()).unwrap();
    std::assert_matches!(
        wb.worksheet().unwrap().remove_background_image(),
        Err(Error::PartNotEditable { .. })
    );
    assert_eq!(wb.to_bytes().unwrap(), original);
}
