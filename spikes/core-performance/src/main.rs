//! ADR-0010 spike: compact Layer 2 tree and lazy `sheetData`, measured
//! against `ooxml_core::XmlDocument` on the ooxml-bench inputs.
//!
//! ```sh
//! cd spikes/core-performance && cargo run --release
//! ```

mod compact;
mod rows;

use std::alloc::{GlobalAlloc, Layout, System};
use std::io::{Cursor, Read};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::time::{Duration, Instant};

use ooxml_core::XmlDocument;

struct Counting;
static CUR: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
static COUNT: AtomicUsize = AtomicUsize::new(0);
static REALLOCS: AtomicUsize = AtomicUsize::new(0);

// SAFETY: forwards to `System` unchanged; only counts.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(l) };
        COUNT.fetch_add(1, Relaxed);
        add(l.size());
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) };
        CUR.fetch_sub(l.size(), Relaxed);
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        let q = unsafe { System.realloc(p, l, n) };
        REALLOCS.fetch_add(1, Relaxed);
        add(n);
        CUR.fetch_sub(l.size(), Relaxed);
        q
    }
}
fn add(n: usize) {
    let now = CUR.fetch_add(n, Relaxed) + n;
    PEAK.fetch_max(now, Relaxed);
}

#[global_allocator]
static A: Counting = Counting;

/// Best of `n` runs, excluding the drop of the result (dropped after the
/// clock stops; drop is measured separately).
fn best<T>(n: usize, mut f: impl FnMut() -> T) -> Duration {
    (0..n)
        .map(|_| {
            let t = Instant::now();
            let v = std::hint::black_box(f());
            let dt = t.elapsed();
            drop(v);
            dt
        })
        .min()
        .unwrap()
}

fn mib(n: usize) -> String {
    format!("{:.1} MiB", n as f64 / 1048576.0)
}
fn ms(d: Duration) -> String {
    format!("{:.1} ms", d.as_secs_f64() * 1e3)
}

fn entries(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    (0..z.len())
        .filter_map(|i| {
            let mut f = z.by_index(i).ok()?;
            let mut v = Vec::new();
            f.read_to_end(&mut v).ok()?;
            Some((f.name().ok()?.into_owned(), v))
        })
        .collect()
}

fn generate_xlsx(cells: u32) -> Vec<u8> {
    let mut wb = rust_xlsxwriter::Workbook::new();
    let ws = wb.add_worksheet();
    for r in 0..cells / 10 {
        ws.write_string(r, 0, format!("row {r}")).unwrap();
        for c in 1..10u16 {
            ws.write_number(r, c, f64::from(r * 10 + u32::from(c))).unwrap();
        }
    }
    wb.save_to_buffer().unwrap()
}

/// Every XML part of every fixture (docx, xlsx, pptx).
fn fixture_parts() -> Vec<(String, Vec<u8>)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reference/fixtures");
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| ["docx", "xlsx", "pptx"].contains(&x.to_str().unwrap_or(""))) {
                let Ok(bytes) = std::fs::read(&p) else { continue };
                if zip::ZipArchive::new(Cursor::new(&bytes[..])).is_err() {
                    continue;
                }
                for (n, v) in entries(&bytes) {
                    if n.ends_with(".xml") || n.ends_with(".rels") {
                        out.push((format!("{}:{n}", p.file_name().unwrap().to_string_lossy()), v));
                    }
                }
            }
        }
    }
    out
}

/// T1, T1b-equivalence and structural equality against core.
fn check(name: &str, bytes: &[u8]) -> Result<bool, String> {
    let Ok(core) = XmlDocument::parse(bytes) else { return Ok(false) }; // core refuses (DTD, depth, …): skip
    let mut c = compact::parse(bytes, 256).map_err(|e| format!("{name}: compact refused what core accepts: {e}"))?;
    if c.serialize().as_bytes() != bytes {
        return Err(format!("{name}: T1 (unedited serialize) differs"));
    }
    if !compact::same_as_core(&c.root, core.root()) {
        return Err(format!("{name}: tree differs from core"));
    }
    let mut core2 = core.clone();
    core2.force_rerender();
    c.force_rerender();
    if c.serialize() != core2.serialize() {
        return Err(format!("{name}: full re-render differs from core's"));
    }
    Ok(true)
}

fn main() {
    // ---- correctness ------------------------------------------------------
    let parts = fixture_parts();
    let mut checked = 0;
    for (n, b) in &parts {
        if check(n, b).unwrap_or_else(|e| panic!("{e}")) {
            checked += 1;
        }
    }
    let adversarial: &[&[u8]] = &[
        b"<a xmlns=\"u\" xmlns:p=\"v\"><p:b p:x=\"1&amp;2\" y=\"a\tb\">t&lt;u&#65;&#x42;v<![CDATA[c]]>w<!--c--><?pi x?></p:b><c/></a>",
        b"\xEF\xBB\xBF<?xml version=\"1.0\"?>\r\n<a x=\"line\nbreak\">a\r\nb</a>\r\n",
        b"<a><b xmlns=\"\"><c/></b></a>",
    ];
    for (i, b) in adversarial.iter().enumerate() {
        assert!(check(&format!("adversarial {i}"), b).unwrap_or_else(|e| panic!("{e}")));
    }
    println!("Correctness: {checked} fixture XML parts and {} adversarial inputs — T1 byte-identical, same tree as core, full re-render identical to core's.\n", adversarial.len());

    // ---- inputs -------------------------------------------------------------
    let mut inputs: Vec<(String, Vec<u8>)> = Vec::new();
    for n in [10_000u32, 100_000, 1_000_000] {
        let x = generate_xlsx(n);
        let sheet = entries(&x).into_iter().find(|(k, _)| k == "xl/worksheets/sheet1.xml").unwrap().1;
        inputs.push((format!("sheet1.xml, {}k cells", n / 1000), sheet));
    }
    let deck = std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reference/fixtures/pptx/poi/bug62513.pptx")).unwrap();
    let biggest_slide = entries(&deck).into_iter().filter(|(k, _)| k.starts_with("ppt/slides/slide")).max_by_key(|(_, v)| v.len()).unwrap();
    inputs.push((format!("pptx slide ({})", biggest_slide.0), biggest_slide.1));
    let all: Vec<u8> = parts.iter().filter(|(n, _)| n.ends_with("word/document.xml")).max_by_key(|(_, v)| v.len()).unwrap().1.clone();
    inputs.push(("largest fixture word/document.xml".into(), all));

    println!("| Input | XML | Tree | Parse (no drop) | Allocations (+ reallocations) | Allocs/element (+ reallocs) | Heap retained | Heap/XML byte | Drop | Unedited serialize | Full re-render |");
    println!("|---|---|---|---|---|---|---|---|---|---|---|");
    for (label, xml) in &inputs {
        let elements = {
            let d = XmlDocument::parse(xml).unwrap();
            fn count(n: &ooxml_core::RawNode) -> usize {
                1 + n.elements().map(count).sum::<usize>()
            }
            count(d.root())
        };
        let reps = if xml.len() > 10_000_000 { 2 } else { 5 };
        // core
        let t_parse = best(reps, || XmlDocument::parse(xml).unwrap());
        let (c0, r0, h0) = (COUNT.load(Relaxed), REALLOCS.load(Relaxed), CUR.load(Relaxed));
        let d = XmlDocument::parse(xml).unwrap();
        let (allocs, reallocs, heap) = (COUNT.load(Relaxed) - c0, REALLOCS.load(Relaxed) - r0, CUR.load(Relaxed) - h0);
        let t_ser = best(reps, || d.serialize());
        let mut dd = d.clone();
        dd.force_rerender();
        let t_rr = best(reps, || dd.serialize());
        drop(dd);
        drop(d);
        let t_drop = (0..reps)
            .map(|_| {
                let d = XmlDocument::parse(xml).unwrap();
                let t = Instant::now();
                drop(d);
                t.elapsed()
            })
            .min()
            .unwrap();
        println!(
            "| {label} | {} | core `XmlDocument` | {} | {allocs} (+ {reallocs}) | {:.2} (+ {:.2}) | {} | {:.1} | {} | {} | {} |",
            mib(xml.len()),
            ms(t_parse),
            allocs as f64 / elements as f64,
            reallocs as f64 / elements as f64,
            mib(heap),
            heap as f64 / xml.len() as f64,
            ms(t_drop),
            ms(t_ser),
            ms(t_rr)
        );
        // compact
        let t_parse = best(reps, || compact::parse(xml, 256).unwrap());
        let (c0, r0, h0) = (COUNT.load(Relaxed), REALLOCS.load(Relaxed), CUR.load(Relaxed));
        let d = compact::parse(xml, 256).unwrap();
        let (allocs, reallocs, heap) = (COUNT.load(Relaxed) - c0, REALLOCS.load(Relaxed) - r0, CUR.load(Relaxed) - h0);
        let t_ser = best(reps, || d.serialize());
        let t_rr = {
            let mut dd = compact::parse(xml, 256).unwrap();
            dd.force_rerender();
            best(reps, || dd.serialize())
        };
        drop(d);
        let t_drop = (0..reps)
            .map(|_| {
                let d = compact::parse(xml, 256).unwrap();
                let t = Instant::now();
                drop(d);
                t.elapsed()
            })
            .min()
            .unwrap();
        println!(
            "| | | compact (spike) | {} | {allocs} (+ {reallocs}) | {:.2} (+ {:.2}) | {} | {:.1} | {} | {} | {} |",
            ms(t_parse),
            allocs as f64 / elements as f64,
            reallocs as f64 / elements as f64,
            mib(heap),
            heap as f64 / xml.len() as f64,
            ms(t_drop),
            ms(t_ser),
            ms(t_rr)
        );
    }
    println!("\n(Heap retained includes the source text: core keeps a `Box<str>` copy, compact an `Arc<str>` copy.)");
    println!(
        "\nNode sizes: core `RawNode`/`RawItem` {}/{} bytes, `RawAttr` {}, `XName` {}; compact `Node`/`Item` {}/{}, `Attr` {}, `Name` {}, `SStr` {}.",
        size_of::<ooxml_core::RawNode>(),
        size_of::<ooxml_core::RawItem>(),
        size_of::<ooxml_core::RawAttr>(),
        size_of::<ooxml_core::XName>(),
        size_of::<compact::Node>(),
        size_of::<compact::Item>(),
        size_of::<compact::Attr>(),
        size_of::<compact::Name>(),
        size_of::<compact::SStr>()
    );

    // ---- lazy sheetData -----------------------------------------------------
    println!("\n## Lazy sheetData (1M cells, 100k rows)\n");
    let src = std::str::from_utf8(&inputs[2].1).unwrap();
    let (c0, h0) = (COUNT.load(Relaxed) + REALLOCS.load(Relaxed), CUR.load(Relaxed));
    let (sheet, t_index) = rows::time(|| rows::LazySheet::index(src).unwrap());
    let (allocs, heap) = (COUNT.load(Relaxed) + REALLOCS.load(Relaxed) - c0, CUR.load(Relaxed) - h0);
    let t_index_best = best(3, || rows::LazySheet::index(src).unwrap().rows.len());
    println!("| Step | Time | Heap | Allocations |\n|---|---|---|---|");
    println!("| index rows (quick-xml scan, no tree) | {} (first {}) | {} | {allocs} |", ms(t_index_best), ms(t_index), mib(heap));
    let n_rows = sheet.rows.len();
    let targets: Vec<u32> = (0..1000u32).map(|i| 1 + i * (n_rows as u32 / 1000)).collect();
    let (edited, t_edit) = rows::time(|| {
        let mut edited = Vec::new();
        for &r in &targets {
            let i = sheet.find(r).unwrap();
            let (frag, mut row) = sheet.materialize(i).unwrap();
            assert!(rows::set_value(&mut row, 1, "-1.5"));
            let text = compact::render(&row, &frag);
            edited.push((i, sheet.rows[i].start, sheet.rows[i].end, text));
        }
        edited
    });
    let mut edited = edited;
    let (out, t_splice) = rows::time(|| rows::splice(src, &mut edited));
    println!("| find + materialize + edit + re-render 1,000 rows | {} | | |", ms(t_edit));
    println!("| splice into the source (save, before deflate) | {} | | |", ms(t_splice));
    // verify: the result parses, edited cells changed, bytes outside edited rows unchanged
    let check = XmlDocument::parse(out.as_bytes()).unwrap();
    let sd = check.root().elements().find(|e| e.name().local() == "sheetData").unwrap();
    let mut seen = 0;
    for row in sd.elements() {
        let r: u32 = row.attrs().iter().find(|a| a.name().local() == "r").unwrap().value().parse().unwrap();
        if targets.contains(&r) {
            let b = row.elements().nth(1).unwrap();
            assert_eq!(b.elements().find(|e| e.name().local() == "v").unwrap().text(), "-1.5");
            seen += 1;
        }
    }
    assert_eq!(seen, targets.len());
    let mut at = 0usize;
    let mut o = 0usize;
    for (_, s, e, text) in &edited {
        assert_eq!(&src[at..*s as usize], &out[o..o + (*s as usize - at)], "T2: bytes outside edited rows");
        o += *s as usize - at + text.len();
        at = *e as usize;
    }
    assert_eq!(&src[at..], &out[o..]);
    println!("\n## Deflate level for a rewritten part (the 1M-cell sheet1.xml, {})\n", mib(src.len()));
    println!("| Level | Time | Compressed size |\n|---|---|---|");
    for level in [Some(1i64), Some(3), None, Some(9)] {
        let mut size = 0;
        let t = best(2, || {
            use std::io::Write;
            let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
            let o = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated).compression_level(level);
            z.start_file("x.xml", o).unwrap();
            z.write_all(out.as_bytes()).unwrap();
            size = z.finish().unwrap().into_inner().len();
        });
        println!("| {} | {} | {} |", level.map_or("default (6)".into(), |l| l.to_string()), ms(t), mib(size));
    }
    println!("\nLazy-sheet check: the output parses with core, the 1,000 edited cells read -1.5, and every byte outside the edited rows is unchanged (T2).");
}
