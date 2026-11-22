//! ADR-0011 §3: inline images. Unit documents (T4: Err ⇒ byte-identical),
//! T-OPC on the added part and relationships, T-REG on drawing ids, T5
//! read-back with docx-rs, and a fixture sweep.

mod common;

use std::assert_matches;

use common::*;
use ooxml_docx::{Document, DrawingId, Error, HyperlinkTarget, ImageSize, ParaRef, RunPosition, RunRef, StoryIndex};

/// A 4×2 PNG (header only: the sniffer reads IHDR and pHYs), 144 dpi.
fn png() -> Vec<u8> {
    let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x04\0\0\0\x02\x08\x06\0\0\0\0\0\0\0".to_vec();
    b.extend(b"\0\0\0\x09pHYs\0\0\x16\x26\0\0\x16\x26\x01\0\0\0\0"); // 5670 px/m = 144 dpi
    b.extend(b"\0\0\0\0IEND\xaeB`\x82");
    b
}

fn gif() -> Vec<u8> {
    b"GIF89a\x0a\0\x05\0\0\0\0;".to_vec()
}

fn p(i: usize) -> ParaRef {
    ParaRef::new(StoryIndex::MAIN, i)
}

fn open(body: &str) -> Document {
    Document::from_bytes(package(&document_xml(body), &[])).unwrap()
}

fn read(doc: &Document, name: &str) -> String {
    String::from_utf8(doc.package().read_part(name).unwrap_or_default()).unwrap()
}

fn body_of(doc: &Document) -> String {
    let s = read(doc, "word/document.xml");
    let a = s.find("<w:body>").unwrap() + 8;
    let b = s.rfind("</w:body>").unwrap();
    s[a..b].to_owned()
}

fn refused<T: std::fmt::Debug>(doc: &mut Document, f: impl FnOnce(&mut Document) -> Result<T, Error>) -> Error {
    let before = doc.to_bytes().unwrap();
    let e = f(doc).unwrap_err();
    assert_eq!(doc.to_bytes().unwrap(), before, "Err ⇒ byte-identical");
    e
}

#[test]
fn inline_picture_xml_part_and_relationship() {
    let mut doc = open("<w:p><w:r><w:t>a</w:t></w:r></w:p>");
    let id = doc.insert_image(RunPosition::EndOf(p(0)), &png(), ImageSize::Auto, "A chart").unwrap();
    assert_eq!(id, DrawingId(1));
    // 4×2 px at 144 dpi = 25400×12700 EMU
    let wp = "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing";
    let a = "http://schemas.openxmlformats.org/drawingml/2006/main";
    let pic = "http://schemas.openxmlformats.org/drawingml/2006/picture";
    assert_eq!(
        body_of(&doc),
        format!(
            r#"<w:p><w:r><w:t>a</w:t></w:r><w:r><w:drawing><wp:inline xmlns:wp="{wp}" distT="0" distB="0" distL="0" distR="0"><wp:extent cx="25400" cy="12700"/><wp:effectExtent l="0" t="0" r="0" b="0"/><wp:docPr id="1" name="Picture 1" descr="A chart"/><wp:cNvGraphicFramePr><a:graphicFrameLocks xmlns:a="{a}" noChangeAspect="1"/></wp:cNvGraphicFramePr><a:graphic xmlns:a="{a}"><a:graphicData uri="{pic}"><pic:pic xmlns:pic="{pic}"><pic:nvPicPr><pic:cNvPr id="0" name="Picture 1"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed="rId1"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="25400" cy="12700"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"#
        )
    );
    // T-OPC: the media part exists, is typed exactly once, and the relationship resolves
    assert!(doc.package().contains("word/media/image1.png"));
    assert_eq!(doc.package().content_type("word/media/image1.png"), Some("image/png"));
    let rels = read(&doc, "word/_rels/document.xml.rels");
    assert!(rels.contains(r#"Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/image1.png""#), "{rels}");
    let ct = read(&doc, "[Content_Types].xml");
    assert_eq!(ct.matches("image1.png").count(), 1, "{ct}");
    assert!(!ct.contains(r#"Extension="png""#), "Word Phase2 never adds a Default");
    // the same bytes again: the part and the relationship are reused; the id increments
    let id2 = doc.insert_image(RunPosition::EndOf(p(0)), &png(), ImageSize::Emu { cx: 914_400, cy: 914_400 }, "").unwrap();
    assert_eq!(id2, DrawingId(2));
    assert_eq!(read(&doc, "word/_rels/document.xml.rels").matches("relationships/image").count(), 1);
    assert_eq!(body_of(&doc).matches(r#"r:embed="rId1""#).count(), 2);
    assert!(body_of(&doc).contains(r#"<wp:extent cx="914400" cy="914400"/>"#));
    // a different image: a new part
    doc.insert_image(RunPosition::EndOf(p(0)), &gif(), ImageSize::Auto, "").unwrap();
    // core numbers per name pattern (`image{}.gif`): one more than the largest in use
    assert!(doc.package().part_names().any(|n| n.starts_with("word/media/image") && n.ends_with(".gif")));
    // the output reopens; docx-rs reads three drawings
    let out = doc.to_bytes().unwrap();
    let re = Document::from_bytes(out.clone()).unwrap();
    assert_eq!(re.paragraph(0).unwrap().text(), "a");
    let json: String = docx_rs::read_docx(&out).unwrap().json().split_whitespace().collect();
    assert_eq!(json.matches(r#""type":"drawing""#).count(), 3, "{json}");
}

#[test]
fn declarations_reuse_existing_bindings_and_ids_are_document_wide() {
    let wp = "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing";
    let a = "http://schemas.openxmlformats.org/drawingml/2006/main";
    let pic = "http://schemas.openxmlformats.org/drawingml/2006/picture";
    let doc_xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="{W_NS}" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:wp="{wp}" xmlns:a="{a}" xmlns:pic="{pic}"><w:body><w:p><w:r><w:t>x</w:t></w:r></w:p></w:body></w:document>"#
    );
    // a header with a drawing id 41 and a text-box cNvPr id 57
    let hdr = format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:hdr xmlns:w="{W_NS}" xmlns:wp="{wp}" xmlns:a="{a}"><w:p><w:r><w:drawing><wp:anchor><wp:docPr id="41" name="x"/><a:graphic><a:graphicData uri="u"><wps:wsp xmlns:wps="urn:wps"><wps:cNvPr id="57" name="y"/></wps:wsp></a:graphicData></a:graphic></wp:anchor></w:drawing></w:r></w:p></w:hdr>"#);
    let mut doc = Document::from_bytes(package(&doc_xml, &[("header", "header1.xml", &hdr)])).unwrap();
    assert_eq!(doc.insert_image(RunPosition::EndOf(p(0)), &png(), ImageSize::Auto, "").unwrap(), DrawingId(58));
    let b = body_of(&doc);
    assert!(b.contains("<wp:inline distT=") && b.contains("<a:graphicFrameLocks noChangeAspect") && b.contains("<pic:pic>"), "{b}");
    // in the header: the media part is reused, with a relationship of the header's own
    let h = doc.stories().nth(1).unwrap().index();
    assert_eq!(doc.insert_image(RunPosition::EndOf(ParaRef::new(h, 0)), &png(), ImageSize::Auto, "").unwrap(), DrawingId(59));
    assert!(read(&doc, "word/_rels/header1.xml.rels").contains(r#"Target="media/image1.png""#));
    assert_eq!(doc.package().part_names().filter(|n| n.starts_with("word/media/")).count(), 1);
}

#[test]
fn existing_default_extension_and_refusals() {
    // a Default for image/jpeg with extension jpg is used, no Override written
    let jpg = [0xFF, 0xD8, 0xFF, 0xC0, 0, 11, 8, 0, 4, 0, 8, 1, 1, 0x11, 0, 0xFF, 0xD9];
    let input = package(&document_xml("<w:p/>"), &[]);
    let ct = String::from_utf8(part(&input, "[Content_Types].xml")).unwrap().replace("<Default Extension=\"xml\"", "<Default Extension=\"jpg\" ContentType=\"image/jpeg\"/><Default Extension=\"xml\"");
    let mut doc = Document::from_bytes(with_part(&input, "[Content_Types].xml", ct.as_bytes())).unwrap();
    doc.insert_image(RunPosition::EndOf(p(0)), &jpg, ImageSize::Auto, "").unwrap();
    assert!(doc.package().contains("word/media/image1.jpg"));
    assert!(!read(&doc, "[Content_Types].xml").contains("image1.jpg"));
    // refusals
    let mut doc = open("<w:p/>");
    assert_matches!(refused(&mut doc, |d| d.insert_image(RunPosition::EndOf(p(0)), b"<svg/>", ImageSize::Auto, "")), Error::UnsupportedImage);
    assert_matches!(refused(&mut doc, |d| d.insert_image(RunPosition::EndOf(p(0)), &png(), ImageSize::Emu { cx: 0, cy: 1 }, "")), Error::InvalidValue { .. });
    assert_matches!(refused(&mut doc, |d| d.insert_image(RunPosition::EndOf(p(0)), &png(), ImageSize::Auto, "\u{1}")), Error::InvalidXmlChar { .. });
    let full = r#"<w:p><w:r><w:drawing><wp:inline xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing"><wp:docPr id="2147483647" name="x"/></wp:inline></w:drawing></w:r></w:p>"#;
    let mut doc = open(full);
    assert_matches!(refused(&mut doc, |d| d.insert_image(RunPosition::EndOf(p(0)), &png(), ImageSize::Auto, "")), Error::DrawingIdsExhausted);
    let mut doc = open("<w:p><w:r><w:t>a</w:t></w:r></w:p>");
    doc.insert_hyperlink(RunPosition::EndOf(p(0)), "l", &HyperlinkTarget::External("https://e/".into())).unwrap();
    assert_matches!(refused(&mut doc, |d| d.insert_image(RunPosition::After(RunRef::new(p(0), 1)), &png(), ImageSize::Auto, "")), Error::NestedHyperlink);
    let mut doc = open(r#"<w:sdt><w:sdtContent><w:p/></w:sdtContent></w:sdt>"#);
    assert_matches!(refused(&mut doc, |d| d.insert_image(RunPosition::EndOf(p(0)), &png(), ImageSize::Auto, "")), Error::UnsupportedContext { .. });
}

/// Fixture sweep: an image at the end of the first paragraph that allows one
/// in every story of every writable fixture, the same bytes each time. The
/// output reopens with the same texts; one media part is added; only story
/// parts, `.rels` and `[Content_Types].xml` change; drawing ids are unique.
#[test]
fn fixture_sweep() {
    let mut ok = 0;
    for stem in writable() {
        let input = fixture(stem);
        let mut doc = Document::from_bytes(input.clone()).unwrap();
        let texts: Vec<Vec<String>> = doc.stories().map(|s| s.paragraphs().map(|p| p.text()).collect()).collect();
        let stories: Vec<StoryIndex> = doc.stories().map(|s| s.index()).collect();
        let mut ids = Vec::new();
        for s in stories {
            for i in 0..doc.story_at(s).unwrap().paragraph_count() {
                let snap = doc.to_bytes().unwrap();
                match doc.insert_image(RunPosition::EndOf(ParaRef::new(s, i)), &png(), ImageSize::Auto, "sweep") {
                    Ok(id) => {
                        ok += 1;
                        ids.push(id);
                        break;
                    }
                    Err(_) => assert_eq!(doc.to_bytes().unwrap(), snap, "{stem}"),
                }
            }
        }
        let mut sorted = ids.clone();
        sorted.dedup();
        assert_eq!(sorted.len(), ids.len(), "{stem}: drawing ids are unique");
        let out = doc.to_bytes().unwrap();
        let re = Document::from_bytes(out.clone()).unwrap();
        let got: Vec<Vec<String>> = re.stories().map(|s| s.paragraphs().map(|p| p.text()).collect()).collect();
        assert_eq!(got, texts, "{stem}");
        let new: Vec<String> = part_names(&out).into_iter().filter(|n| !part_names(&input).contains(n)).collect();
        assert!(new.len() <= 1 + doc.stories().count(), "{stem}: {new:?}");
        assert!(new.iter().all(|n| n.starts_with("word/media/") || n.ends_with(".rels")), "{stem}: {new:?}");
        let story_parts: Vec<String> = doc.stories().map(|s| s.id().part().to_owned()).collect();
        for (a, b) in entries(&out).iter().zip(entries(&input)) {
            if a != &b {
                assert!(story_parts.contains(&a.0) || a.0.ends_with(".rels") || a.0 == "[Content_Types].xml", "{stem}: {} changed", a.0);
            }
        }
    }
    eprintln!("image sweep: {ok} images");
    assert!(ok > 30, "{ok}");
}

#[test]
fn image_extent_bounds_and_zero_rounded_dpi() {
    const MAX: u64 = 27_273_042_316_900;
    let at = RunPosition::EndOf(p(0));
    let mut doc = open("<w:p/>");
    let mut image = png();
    let density = image.windows(4).position(|bytes| bytes == b"pHYs").unwrap() + 4;
    image[density..density + 4].copy_from_slice(&19u32.to_be_bytes());
    doc.insert_image(at, &image, ImageSize::Auto, "").unwrap();
    let body = body_of(&doc);
    assert!(body.contains(r#"<wp:extent cx="38100" cy="19050"/>"#), "{body}");
    assert!(body.contains(r#"<a:ext cx="38100" cy="19050"/>"#), "{body}");

    doc.insert_image(at, &image, ImageSize::Emu { cx: MAX, cy: MAX }, "").unwrap();
    let body = body_of(&doc);
    for name in ["wp:extent", "a:ext"] {
        assert!(body.contains(&format!(r#"<{name} cx="{MAX}" cy="{MAX}"/>"#)), "{body}");
    }
    for (cx, cy) in [(MAX + 1, 1), (1, MAX + 1), (0, 1), (1, 0)] {
        assert_matches!(
            refused(&mut doc, |d| d.insert_image(at, &image, ImageSize::Emu { cx, cy }, "")),
            Error::InvalidValue { what: "image size", .. }
        );
    }

    // 20 px/m rounds to 1 DPI. Test the adjacent pixel widths around the EMU limit.
    image[density..density + 4].copy_from_slice(&20u32.to_be_bytes());
    let pixels = u32::try_from(MAX / 914_400).unwrap();
    image[16..20].copy_from_slice(&pixels.to_be_bytes());
    doc.insert_image(at, &image, ImageSize::Auto, "").unwrap();
    image[16..20].copy_from_slice(&(pixels + 1).to_be_bytes());
    assert_matches!(
        refused(&mut doc, |d| d.insert_image(at, &image, ImageSize::Auto, "")),
        Error::InvalidValue { what: "image size", .. }
    );
}

#[test]
fn positioned_image_and_hyperlink_refuse_alternate_content_across_slots() {
    let wrap = |member: &str| {
        format!(r#"<mc:AlternateContent xmlns:mc="{}"><mc:Choice Requires="w">{member}</mc:Choice></mc:AlternateContent>"#,
                ooxml_core::name::MC_NS)
    };
    let run = |text: &str| format!("<w:r><w:t>{text}</w:t></w:r>");
    let (r0, r1, r2) = (run("r0"), run("r1"), run("r2"));
    for (body, after) in [
        (format!("<w:p><w:pPr/>{r0}{}{r1}{r2}</w:p>", wrap("")), 1),
        (format!("<w:p>{}{r0}{r1}</w:p>", wrap("<w:pPr/>")), 0),
        (format!("<w:p>{r0}{}{r1}</w:p>", wrap("<w:r/>")), 0),
    ] {
        let mut doc = open(&body);
        let at = RunPosition::After(RunRef::new(p(0), after));
        assert_matches!(
            refused(&mut doc, |doc| doc.insert_image(at, &png(), ImageSize::Auto, "")),
            Error::Core(ooxml_core::Error::UnresolvedSibling(_))
        );
        assert_matches!(
            refused(&mut doc, |doc| doc.insert_hyperlink(at, "link", &HyperlinkTarget::External("https://example.com/".into()))),
            Error::Core(ooxml_core::Error::UnresolvedSibling(_))
        );
    }
}
