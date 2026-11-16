//! Support code for the ooxml-rs performance baseline
//! (`cargo bench -p ooxml-bench`, see `docs/testing/performance-baseline-2026-11.md`).
//!
//! - [`PeakAlloc`]: a counting global allocator (current and peak live heap
//!   bytes) so that every measurement also reports its peak memory.
//! - [`measure`]: wall-clock timing (median of several runs) plus the heap
//!   peak of the first run above the live heap at its start.
//! - [`generate_deck`]: a large `.pptx` built from a small fixture by adding
//!   generated slides.
//!
//! Nothing here is used by the library crates; this crate is never
//! published and has no tests (`cargo test` builds none of its targets).

use std::alloc::{GlobalAlloc, Layout, System};
use std::io::{Cursor, Read, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// A global allocator that counts live heap bytes and their peak.
pub struct PeakAlloc {
    current: AtomicUsize,
    peak: AtomicUsize,
    count: AtomicUsize,
    reallocs: AtomicUsize,
}

impl PeakAlloc {
    /// A new counter (use as `#[global_allocator] static A: PeakAlloc = PeakAlloc::new();`).
    pub const fn new() -> Self {
        PeakAlloc {
            current: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
            count: AtomicUsize::new(0),
            reallocs: AtomicUsize::new(0),
        }
    }

    /// Live heap bytes now.
    pub fn current(&self) -> usize {
        self.current.load(Ordering::Relaxed)
    }

    /// Highest live heap bytes since the last [`Self::reset_peak`].
    pub fn peak(&self) -> usize {
        self.peak.load(Ordering::Relaxed)
    }

    /// Number of fresh allocations (`alloc`, `alloc_zeroed`) so far.
    pub fn count(&self) -> usize {
        self.count.load(Ordering::Relaxed)
    }

    /// Number of reallocations (a growing `Vec` or `String`) so far.
    pub fn reallocs(&self) -> usize {
        self.reallocs.load(Ordering::Relaxed)
    }

    /// Sets the peak to the current live bytes.
    pub fn reset_peak(&self) {
        self.peak.store(self.current(), Ordering::Relaxed);
    }

    fn add(&self, n: usize) {
        let now = self.current.fetch_add(n, Ordering::Relaxed) + n;
        self.peak.fetch_max(now, Ordering::Relaxed);
    }

    fn sub(&self, n: usize) {
        self.current.fetch_sub(n, Ordering::Relaxed);
    }
}

impl Default for PeakAlloc {
    fn default() -> Self {
        Self::new()
    }
}

// SAFETY: every method forwards to `System` with the caller's layout and
// pointer unchanged; the counters do not affect the allocation.
unsafe impl GlobalAlloc for PeakAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: forwarded unchanged.
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            self.count.fetch_add(1, Ordering::Relaxed);
            self.add(layout.size());
        }
        p
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: forwarded unchanged.
        let p = unsafe { System.alloc_zeroed(layout) };
        if !p.is_null() {
            self.count.fetch_add(1, Ordering::Relaxed);
            self.add(layout.size());
        }
        p
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: forwarded unchanged.
        unsafe { System.dealloc(ptr, layout) };
        self.sub(layout.size());
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: forwarded unchanged.
        let p = unsafe { System.realloc(ptr, layout, new_size) };
        if !p.is_null() {
            self.reallocs.fetch_add(1, Ordering::Relaxed);
            // Count the new block before releasing the old one: a moving
            // realloc holds both for a moment (conservative peak).
            self.add(new_size);
            self.sub(layout.size());
        }
        p
    }
}

/// One measurement.
#[derive(Clone, Debug)]
pub struct Sample {
    /// Median wall time over the runs.
    pub median: Duration,
    /// Fastest run.
    pub min: Duration,
    /// Number of timed runs.
    pub runs: usize,
    /// Heap peak of the warm-up run (or the single cold run), above the
    /// live heap at its start; deterministic for these workloads.
    pub peak_bytes: usize,
}

/// Times `f` **including dropping its result** (the total cost a caller
/// pays for the operation), at least `min_runs` times and until `budget`
/// is used up, at most `max_runs` times.
///
/// One untimed warm-up run comes first. If the warm-up itself takes at
/// least `budget`, it is reported as the only sample (`runs == 1`, a cold
/// run) instead of repeating a slow case.
pub fn measure<T>(alloc: &PeakAlloc, budget: Duration, min_runs: usize, max_runs: usize, mut f: impl FnMut() -> T) -> Sample {
    let mut times = Vec::new();
    let mut peak_bytes = 0;
    let mut run = |alloc: &PeakAlloc, peak: &mut usize| {
        let base = alloc.current();
        alloc.reset_peak();
        let t = Instant::now();
        drop(std::hint::black_box(f()));
        let dt = t.elapsed();
        *peak = alloc.peak().saturating_sub(base);
        dt
    };
    let warm = run(alloc, &mut peak_bytes);
    if warm >= budget {
        return Sample { median: warm, min: warm, runs: 1, peak_bytes };
    }
    let started = Instant::now();
    loop {
        let mut p = 0;
        let dt = run(alloc, &mut p);
        times.push(dt);
        let spent = started.elapsed();
        if times.len() >= max_runs || (times.len() >= min_runs && spent >= budget) || dt >= budget {
            break;
        }
    }
    let runs = times.len();
    let min = *times.iter().min().expect("at least one run");
    times.sort();
    Sample { median: times[runs / 2], min, runs, peak_bytes }
}

/// Reads every entry of a zip into (name, bytes).
fn entries(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    (0..z.len())
        .map(|i| {
            let mut f = z.by_index(i).expect("entry");
            let mut v = Vec::new();
            f.read_to_end(&mut v).expect("read");
            (f.name().expect("utf-8 name").into_owned(), v)
        })
        .collect()
}

/// A slide with a title placeholder, `shapes - 1` text boxes of
/// `paragraphs` two-run paragraphs each.
fn slide_xml(n: usize, shapes: usize, paragraphs: usize) -> String {
    let mut sp = String::new();
    sp.push_str(&format!(
        r#"<p:sp><p:nvSpPr><p:cNvPr id="2" name="Title {n}"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:rPr lang="en-US" dirty="0"/><a:t>Generated slide {n}</a:t></a:r></a:p></p:txBody></p:sp>"#
    ));
    for s in 1..shapes {
        let id = s + 2;
        sp.push_str(&format!(
            r#"<p:sp><p:nvSpPr><p:cNvPr id="{id}" name="TextBox {id}"/><p:cNvSpPr txBox="1"/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="{x}" y="{y}"/><a:ext cx="3000000" cy="400000"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr><p:txBody><a:bodyPr wrap="square"/><a:lstStyle/>"#,
            x = 100000 * s,
            y = 300000 * s
        ));
        for p in 0..paragraphs {
            sp.push_str(&format!(
                r#"<a:p><a:r><a:rPr lang="en-US" b="1" dirty="0"/><a:t>Shape {s} paragraph {p}: </a:t></a:r><a:r><a:rPr lang="en-US" dirty="0"/><a:t>lorem ipsum dolor sit amet, consectetur adipiscing elit</a:t></a:r></a:p>"#
            ));
        }
        sp.push_str("</p:txBody></p:sp>");
    }
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>{sp}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>"#
    )
}

/// A deck: `base` (a small `.pptx` with `ppt/slideLayouts/slideLayout1.xml`)
/// plus `slides` generated slides, each with `shapes` shapes of
/// `paragraphs` paragraphs. Parts are deflated.
pub fn generate_deck(base: &[u8], slides: usize, shapes: usize, paragraphs: usize) -> Vec<u8> {
    let mut parts = entries(base);
    let find = |parts: &mut Vec<(String, Vec<u8>)>, name: &str| -> usize {
        parts.iter().position(|(n, _)| n == name).unwrap_or_else(|| panic!("{name} in base deck"))
    };
    let (mut ids, mut rels, mut overrides) = (String::new(), String::new(), String::new());
    let mut new_parts = Vec::new();
    for k in 0..slides {
        let name = format!("ppt/slides/gen{k}.xml");
        ids.push_str(&format!(r#"<p:sldId id="{}" r:id="rIdGen{k}"/>"#, 10_000 + k));
        rels.push_str(&format!(
            r#"<Relationship Id="rIdGen{k}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/gen{k}.xml"/>"#
        ));
        overrides.push_str(&format!(
            r#"<Override PartName="/{name}" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/>"#
        ));
        new_parts.push((name, slide_xml(k, shapes, paragraphs).into_bytes()));
        new_parts.push((
            format!("ppt/slides/_rels/gen{k}.xml.rels"),
            br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/></Relationships>"#.to_vec(),
        ));
    }
    let i = find(&mut parts, "ppt/presentation.xml");
    let s = String::from_utf8(parts[i].1.clone()).expect("utf-8").replacen("</p:sldIdLst>", &format!("{ids}</p:sldIdLst>"), 1);
    parts[i].1 = s.into_bytes();
    let i = find(&mut parts, "ppt/_rels/presentation.xml.rels");
    let s = String::from_utf8(parts[i].1.clone()).expect("utf-8").replacen("</Relationships>", &format!("{rels}</Relationships>"), 1);
    parts[i].1 = s.into_bytes();
    let i = find(&mut parts, "[Content_Types].xml");
    let s = String::from_utf8(parts[i].1.clone()).expect("utf-8").replacen("</Types>", &format!("{overrides}</Types>"), 1);
    parts[i].1 = s.into_bytes();
    parts.extend(new_parts);
    let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let o = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, data) in parts {
        w.start_file(name, o).expect("zip");
        w.write_all(&data).expect("zip");
    }
    w.finish().expect("zip").into_inner()
}

/// Human-readable bytes.
pub fn fmt_bytes(n: usize) -> String {
    const K: f64 = 1024.0;
    let n = n as f64;
    match () {
        () if n >= K * K * K => format!("{:.2} GiB", n / (K * K * K)),
        () if n >= K * K => format!("{:.1} MiB", n / (K * K)),
        () if n >= K => format!("{:.1} KiB", n / K),
        () => format!("{n} B"),
    }
}

/// Human-readable duration.
pub fn fmt_dur(d: Duration) -> String {
    let s = d.as_secs_f64();
    match () {
        () if s >= 1.0 => format!("{s:.2} s"),
        () if s >= 1e-3 => format!("{:.2} ms", s * 1e3),
        () => format!("{:.1} µs", s * 1e6),
    }
}
