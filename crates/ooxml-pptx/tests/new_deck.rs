//! `Presentation::new()` (ADR-0012 §8): the built-in template.

use std::time::{Duration, UNIX_EPOCH};

use ooxml_pptx::{NewOptions, Presentation};

const STANDARD: [&str; 11] = [
    "Title Slide",
    "Title and Content",
    "Section Header",
    "Two Content",
    "Comparison",
    "Title Only",
    "Blank",
    "Content with Caption",
    "Picture with Caption",
    "Title and Vertical Text",
    "Vertical Title and Text",
];

fn fixed() -> NewOptions {
    NewOptions { created: UNIX_EPOCH + Duration::from_secs(1_794_822_645) }
}

#[test]
fn eleven_standard_layouts_in_order_and_no_slides() {
    let mut p = Presentation::new().unwrap();
    assert_eq!(p.slide_count(), 0);
    let names: Vec<String> = p.layouts().unwrap().into_iter().map(|l| l.name.unwrap()).collect();
    assert_eq!(names, STANDARD);
    assert_eq!(p.layouts().unwrap()[6].layout_type.as_deref(), Some("blank"));
}

#[test]
fn reproducible_with_an_injected_time() {
    let a = Presentation::new_with(fixed()).unwrap().to_bytes().unwrap();
    let b = Presentation::new_with(fixed()).unwrap().to_bytes().unwrap();
    assert_eq!(a, b);
    let p = Presentation::from_bytes(a).unwrap();
    let core = String::from_utf8(p.package().read_part("docProps/core.xml").unwrap()).unwrap();
    assert!(core.contains("2026-11-16T09:50:45Z") && !core.contains("creator"));
}

#[test]
fn every_layout_makes_a_slide() {
    let mut p = Presentation::new_with(fixed()).unwrap();
    for l in p.layouts().unwrap() {
        p.add_slide(&l, None).unwrap();
    }
    assert_eq!(p.slide_count(), 11);
    let out = p.to_bytes().unwrap();
    let mut q = Presentation::from_bytes(out).unwrap();
    let app = String::from_utf8(q.package().read_part("docProps/app.xml").unwrap()).unwrap();
    assert!(app.contains("<Slides>11</Slides>"), "{app}");
    // first slide: title + subtitle, editable
    let mut s = q.slide_mut(0).unwrap();
    let phs = s.as_slide().placeholders();
    assert_eq!(phs.len(), 2);
    let path = phs[0].path().clone();
    s.shape_mut(&path).unwrap().text_body_mut().unwrap().set_text("Hello").unwrap();
    q.delete_slide(6).unwrap();
    q.duplicate_slide(0, None).unwrap();
    q.move_slide(0, 5).unwrap();
    Presentation::from_bytes(q.to_bytes().unwrap()).unwrap();
}
