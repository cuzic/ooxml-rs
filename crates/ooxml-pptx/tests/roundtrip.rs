//! ADR-0005 §7 L1, T1, T1b on all 23 fixtures: reading never changes a
//! byte; an unedited package passes through per entry (name, method,
//! CRC-32, compressed bytes); every slide part re-serializes to its source
//! (T1) and, force-re-rendered, re-parses to the same tree (T1b); after an
//! edit only the edited slide part differs.

#[path = "oracle/cases.rs"]
mod cases;

use std::io::{Cursor, Read};

use ooxml_core::{Package, XmlDocument};
use ooxml_pptx::{Presentation, ShapeKind, TextItem};
use zip::ZipArchive;

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

/// Reads everything the read API exposes.
fn read_all(pres: &mut Presentation) -> usize {
    let mut chars = 0;
    for i in 0..pres.slide_count() {
        let slide = pres.slide(i).unwrap();
        for s in slide.shapes() {
            let _ = (s.id(), s.name(), s.kind(), s.placeholder().map(|p| (p.kind(), p.idx())));
            if let Some(b) = s.text_body() {
                chars += b.text().len();
                for p in b.paragraphs() {
                    for it in p.items() {
                        if let TextItem::Field(f) = it {
                            let _ = f.field_type();
                        }
                    }
                }
            }
            if let Some(t) = s.table() {
                for row in t.rows() {
                    for c in row {
                        let _ = (c.is_merged().unwrap(), c.grid_span().unwrap(), c.row_span().unwrap());
                        chars += c.text_body().map_or(0, |b| b.text().len());
                    }
                }
            }
        }
    }
    chars
}

#[test]
fn l1_t1_reading_changes_nothing() {
    let mut slides = 0;
    let mut chars = 0;
    for f in cases::FIXTURES {
        let input = cases::read_fixture(f);
        let mut pres = Presentation::from_bytes(input.clone()).unwrap_or_else(|e| panic!("{f}: {e}"));
        assert!(pres.slides().all(|s| s.is_ok()), "{f}: dangling slide");
        chars += read_all(&mut pres);
        for i in 0..pres.slide_count() {
            let part = pres.slides().nth(i).unwrap().unwrap().part_name;
            let src = pres.package().read_part(&part).unwrap();
            let slide = pres.slide(i).unwrap();
            assert!(!slide.document().is_dirty());
            assert_eq!(slide.document().serialize().as_bytes(), &src[..], "{f} {part}: T1");
            slides += 1;
        }
        let out = pres.to_bytes().unwrap();
        assert_eq!(entries(&out), entries(&input), "{f}: L1 passthrough after reading");
    }
    eprintln!("L1/T1: {} fixtures, {slides} slide parts, {chars} bytes of text read", cases::FIXTURES.len());
    assert_eq!(slides, 79, "slide parts in the 23 fixtures (zipfile count)");
}

#[test]
fn t1b_force_rerender_reparses_to_the_same_tree() {
    for f in cases::FIXTURES {
        let pres = Presentation::from_bytes(cases::read_fixture(f)).unwrap();
        let parts: Vec<String> = pres.slides().map(|s| s.unwrap().part_name).collect();
        let mut pkg = Package::from_bytes(cases::read_fixture(f)).unwrap();
        for part in parts {
            let original = XmlDocument::parse(&pkg.read_part(&part).unwrap()).unwrap();
            let doc = pkg.xml_part_mut(&part).unwrap();
            doc.force_rerender();
            let again = XmlDocument::parse(doc.serialize().as_bytes()).unwrap_or_else(|e| panic!("{f} {part}: {e}"));
            assert!(again.root().structurally_eq(original.root()), "{f} {part}: T1b");
        }
    }
}

#[test]
fn l1_only_the_edited_slide_changes() {
    for f in cases::FIXTURES {
        let input = cases::read_fixture(f);
        let mut pres = Presentation::from_bytes(input.clone()).unwrap();
        // first slide with a plain run in a p:sp
        let mut target = None;
        'outer: for i in 0..pres.slide_count() {
            let slide = pres.slide(i).unwrap();
            for s in slide.shapes() {
                if *s.kind() == ShapeKind::Shape
                    && let Some(b) = s.text_body()
                    && let Some(pi) = b.paragraphs().iter().position(|p| !p.runs().is_empty())
                {
                    target = Some((i, s.path().clone(), pi, slide.info().part_name.clone()));
                    break 'outer;
                }
            }
        }
        let Some((i, path, pi, part)) = target else { continue };
        let mut slide = pres.slide_mut(i).unwrap();
        let mut shape = slide.shape_mut(&path).unwrap();
        let mut body = shape.text_body_mut().unwrap();
        match body.paragraph_mut(pi).unwrap().run_mut(0).unwrap().set_text("L1 edit") {
            Ok(()) => {}
            Err(e) => {
                eprintln!("{f}: edit refused ({e}); skipped");
                continue;
            }
        }
        let out = pres.to_bytes().unwrap();
        let (a, b) = (entries(&out), entries(&input));
        assert_eq!(a.len(), b.len());
        for (x, y) in a.iter().zip(&b) {
            assert_eq!(x.0, y.0, "{f}: entry order");
            if x.0 != part {
                assert_eq!(x, y, "{f}: {} must pass through", x.0);
            } else {
                assert_ne!(x.3, y.3, "{f}: the edited slide changes");
            }
        }
    }
}
