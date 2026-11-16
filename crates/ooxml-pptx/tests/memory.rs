//! Memory bound of the shape walk (PR #16 round-2 review n2): listing the
//! shapes of a slide must cost O(shapes), not O(shapes × group depth). Own
//! test binary: a counting global allocator measures the peak of live heap
//! bytes while `shapes()`, `shape_by_id` and `slide_mut` + `shape_mut_by_id`
//! run on a slide whose shapes sit 250 groups deep.

#[path = "oracle/cases.rs"]
mod cases;

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

use ooxml_core::Package;
use ooxml_pptx::Presentation;

struct Counting;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(l) };
        if !p.is_null() {
            let now = LIVE.fetch_add(l.size(), Relaxed) + l.size();
            PEAK.fetch_max(now, Relaxed);
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) };
        LIVE.fetch_sub(l.size(), Relaxed);
    }
}

#[global_allocator]
static A: Counting = Counting;

/// Peak of live bytes above the level at the start, while `f` runs.
fn peak_during<T>(f: impl FnOnce() -> T) -> (T, usize) {
    let base = LIVE.load(Relaxed);
    PEAK.store(base, Relaxed);
    let r = f();
    (r, PEAK.load(Relaxed) - base)
}

const DEPTH: usize = 250; // groups; the shapes are at element depth 254
const SHAPES: usize = 20_000;

fn deep_deck() -> Vec<u8> {
    let mut x = String::from(concat!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
        r#"<p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">"#,
        r#"<p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>"#
    ));
    for k in 0..DEPTH {
        x.push_str(&format!(r#"<p:grpSp><p:nvGrpSpPr><p:cNvPr id="{}" name="g"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>"#, 10 + k));
    }
    for k in 0..SHAPES {
        x.push_str(&format!(r#"<p:sp><p:nvSpPr><p:cNvPr id="{}" name="s"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr/></p:sp>"#, 1000 + k));
    }
    x.push_str(&"</p:grpSp>".repeat(DEPTH));
    x.push_str("</p:spTree></p:cSld></p:sld>");
    let mut pkg = Package::from_bytes(cases::read_fixture("poi/sample.pptx")).unwrap();
    pkg.set_part("ppt/slides/slide1.xml", x.into_bytes()).unwrap();
    pkg.to_bytes().unwrap()
}

#[test]
fn shape_walk_memory_is_linear_in_shapes() {
    let mut p = Presentation::from_bytes(deep_deck()).unwrap();
    p.slide(0).unwrap(); // parse outside the measurement
    let (n, peak) = peak_during(|| {
        let s = p.slide(0).unwrap();
        let shapes = s.shapes();
        assert_eq!(shapes.len(), DEPTH + SHAPES);
        assert_eq!(shapes.last().unwrap().depth(), DEPTH);
        assert_eq!(shapes.last().unwrap().path().as_slice().len(), 2 + DEPTH + 1, "full paths are still available");
        shapes.len()
    });
    let per_shape = peak / n;
    eprintln!("shapes(): peak {peak} bytes for {n} shapes ({per_shape} B/shape)");
    // One full path per shape was ~ (2 + 250 + 1) × 8 B ≈ 2 KiB per shape.
    assert!(per_shape < 256, "shapes() must not store a full path per shape: {per_shape} B/shape");

    let (_, peak) = peak_during(|| {
        let s = p.slide(0).unwrap();
        assert!(s.shape_by_id(1000 + SHAPES as u32 - 1).unwrap().is_some());
    });
    assert!(peak / n < 256, "shape_by_id: {} B/shape", peak / n);

    let (_, peak) = peak_during(|| {
        let mut s = p.slide_mut(0).unwrap();
        assert!(s.shape_mut_by_id(1000).is_ok());
    });
    eprintln!("slide_mut + shape_mut_by_id: peak {peak} bytes");
    assert!(peak / n < 256, "slide_mut + shape_mut_by_id: {} B/shape", peak / n);
}
