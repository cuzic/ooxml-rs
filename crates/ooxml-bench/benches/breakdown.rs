//! Timing and allocation breakdowns behind the hotspots of
//! `docs/testing/performance-baseline-2026-11.md` (no sampling profiler is
//! available on the benchmark host: `perf` has no kernel support there).
//!
//! ```sh
//! cargo bench -p ooxml-bench --bench breakdown
//! ```

use std::io::{Cursor, Read};
use std::time::{Duration, Instant};

use ooxml_bench::{PeakAlloc, fmt_bytes, fmt_dur, generate_deck};
use ooxml_core::{Package, RawAttr, RawItem, RawNode, RawText, XName, XmlDocument};

#[global_allocator]
static ALLOC: PeakAlloc = PeakAlloc::new();

const COLS: u32 = 10;

fn generate_xlsx(cells: u32) -> Vec<u8> {
    let mut wb = rust_xlsxwriter::Workbook::new();
    let ws = wb.add_worksheet();
    for r in 0..cells / COLS {
        ws.write_string(r, 0, format!("row {r}")).unwrap();
        for c in 1..COLS {
            ws.write_number(r, c as u16, f64::from(r * COLS + c)).unwrap();
        }
    }
    wb.save_to_buffer().unwrap()
}

fn part(bytes: &[u8], name: &str) -> Vec<u8> {
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut f = z.by_name(name).unwrap();
    let mut v = Vec::new();
    f.read_to_end(&mut v).unwrap();
    v
}

/// Best of `n` runs, **excluding** the drop of the result (dropped after
/// the clock stops; review PR #24 M1). Drop is measured on its own where
/// it matters.
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

/// Best of `n` drops of a value made by `make` (untimed).
fn best_drop<T>(n: usize, mut make: impl FnMut() -> T) -> Duration {
    (0..n)
        .map(|_| {
            let v = make();
            let t = Instant::now();
            drop(v);
            t.elapsed()
        })
        .min()
        .unwrap()
}

fn count(node: &RawNode, acc: &mut (usize, usize, usize)) {
    acc.0 += 1;
    acc.1 += node.attrs().len();
    for c in node.children() {
        match c {
            RawItem::Element(e) => count(e, acc),
            RawItem::Text(_) => acc.2 += 1,
            RawItem::Markup(_) => {}
        }
    }
}

/// Stage 1 comparison, with file I/O and input generation outside the timer.
fn stage1() {
    use std::fmt::Write as _;

    let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reference/fixtures");
    let mut inputs = Vec::new();
    for (label, package, entry) in [
        ("Largest docx document", "docx/poi-bug59058.docx", "word/document.xml"),
        ("Largest pptx slide", "pptx/libreoffice/slide-sections.pptx", "ppt/slides/slide5.xml"),
    ] {
        inputs.push((label, part(&std::fs::read(fixtures.join(package)).unwrap(), entry)));
    }
    for (label, cells) in [("10k cells", 10_000), ("100k cells", 100_000), ("1M cells", 1_000_000)] {
        inputs.push((label, part(&generate_xlsx(cells), "xl/worksheets/sheet1.xml")));
    }
    let mut distinct = String::from("<r>");
    for i in 0..262_144_u32 {
        // Multiplication by an odd constant is a permutation of u32.
        write!(distinct, "<n{:08x}/>", i.wrapping_mul(2_654_435_761)).unwrap();
    }
    distinct.push_str("</r>");
    inputs.push(("262,144 distinct names", distinct.into_bytes()));

    println!("| Input | XML | Parse (best of 5) | Drop (best of 5) | Retained heap |");
    println!("|---|---|---|---|---|");
    for (label, bytes) in inputs {
        let parse = best(5, || XmlDocument::parse(&bytes).unwrap());
        let drop_time = best_drop(5, || XmlDocument::parse(&bytes).unwrap());
        let before = ALLOC.current();
        let doc = XmlDocument::parse(&bytes).unwrap();
        let heap = ALLOC.current() - before;
        std::hint::black_box(&doc);
        println!(
            "| {label} | {} | {} | {} | {} |",
            fmt_bytes(bytes.len()), fmt_dur(parse), fmt_dur(drop_time), fmt_bytes(heap),
        );
    }
}

fn main() {
    if std::env::args().any(|arg| arg == "--stage1") {
        stage1();
        return;
    }
    println!("## Type sizes\n");
    println!("| Type | Bytes |\n|---|---|");
    for (name, size) in [
        ("RawNode", size_of::<RawNode>()),
        ("RawItem", size_of::<RawItem>()),
        ("RawAttr", size_of::<RawAttr>()),
        ("RawText", size_of::<RawText>()),
        ("XName", size_of::<XName>()),
    ] {
        println!("| `{name}` | {size} |");
    }

    let inputs: Vec<(u32, Vec<u8>)> = [10_000u32, 100_000, 1_000_000].into_iter().map(|n| (n, generate_xlsx(n))).collect();

    println!("\n## XML tree cost (sheet1.xml)\n");
    println!("| Cells | XML | Elements | Attributes | Text nodes | Allocations (+ reallocations) | Heap after parse | Heap / XML byte | Allocs / element (+ reallocs) | quick-xml events only | `XmlDocument::parse` (no drop) | drop | parse / events | drop / parse |");
    println!("|---|---|---|---|---|---|---|---|---|---|---|---|---|---|");
    for (n, bytes) in &inputs {
        let sheet = part(bytes, "xl/worksheets/sheet1.xml");
        let events = best(3, || {
            let mut r = quick_xml::Reader::from_reader(&sheet[..]);
            let mut k = 0usize;
            loop {
                match r.read_event().unwrap() {
                    quick_xml::events::Event::Eof => break,
                    quick_xml::events::Event::Start(e) | quick_xml::events::Event::Empty(e) => k += e.attributes().count() + 1,
                    _ => k += 1,
                }
            }
            k
        });
        let parse = best(3, || XmlDocument::parse(&sheet).unwrap());
        let (a0, r0, h0) = (ALLOC.count(), ALLOC.reallocs(), ALLOC.current());
        let doc = XmlDocument::parse(&sheet).unwrap();
        let (allocs, reallocs, heap) = (ALLOC.count() - a0, ALLOC.reallocs() - r0, ALLOC.current() - h0);
        let mut acc = (0, 0, 0);
        count(doc.root(), &mut acc);
        drop(doc);
        let dropt = best_drop(3, || XmlDocument::parse(&sheet).unwrap());
        println!(
            "| {n} | {} | {} | {} | {} | {allocs} (+ {reallocs}) | {} | {:.1} | {:.2} (+ {:.2}) | {} | {} | {} | {:.1}× | {:.0}% |",
            fmt_bytes(sheet.len()),
            acc.0,
            acc.1,
            acc.2,
            fmt_bytes(heap),
            heap as f64 / sheet.len() as f64,
            allocs as f64 / acc.0 as f64,
            reallocs as f64 / acc.0 as f64,
            fmt_dur(events),
            fmt_dur(parse),
            fmt_dur(dropt),
            parse.as_secs_f64() / events.as_secs_f64(),
            100.0 * dropt.as_secs_f64() / parse.as_secs_f64()
        );
    }

    println!("\n## `Workbook::from_bytes` phases\n");
    println!("All columns exclude dropping the result; the last column is the drop of the opened `Workbook`.\n");
    println!("| Cells | `Package::from_bytes` | decompress sheet1.xml | parse sheet1.xml | quick-xml scan of sheet1.xml | `Workbook::from_bytes` total | drop of the `Workbook` |");
    println!("|---|---|---|---|---|---|---|");
    for (n, bytes) in &inputs {
        let pkg = best(3, || Package::from_bytes(bytes.clone()).unwrap());
        let p = Package::from_bytes(bytes.clone()).unwrap();
        let dec = best(3, || p.read_part("xl/worksheets/sheet1.xml").unwrap());
        let sheet = p.read_part("xl/worksheets/sheet1.xml").unwrap();
        let parse = best(3, || XmlDocument::parse(&sheet).unwrap());
        let scan = best(3, || {
            let mut r = quick_xml::Reader::from_reader(&sheet[..]);
            let mut f = 0usize;
            loop {
                match r.read_event().unwrap() {
                    quick_xml::events::Event::Eof => break,
                    quick_xml::events::Event::Start(e) if e.local_name().as_ref() == "f" => f += 1,
                    _ => {}
                }
            }
            f
        });
        let total = best(3, || ooxml_xlsx::Workbook::from_bytes(bytes.clone()).unwrap());
        let wb_drop = best_drop(3, || ooxml_xlsx::Workbook::from_bytes(bytes.clone()).unwrap());
        println!("| {n} | {} | {} | {} | {} | {} | {} |", fmt_dur(pkg), fmt_dur(dec), fmt_dur(parse), fmt_dur(scan), fmt_dur(total), fmt_dur(wb_drop));
    }

    println!("\n## Per-call cost of the cell API (after open)\n");
    println!("| Cells (rows) | `cell(0, 0)` | `cell(last row, 0)` | `cell_mut(last row, 1).set_number` (one edit) | `to_bytes` after one edit | of which: serialize sheet1.xml | of which: deflate sheet1.xml (zip default level) | deflate at level 1 |");
    println!("|---|---|---|---|---|---|---|---|");
    for (n, bytes) in &inputs {
        let rows = n / COLS;
        let mut wb = ooxml_xlsx::Workbook::from_bytes(bytes.clone()).unwrap();
        let (first, last) = {
            let ws = wb.worksheet().unwrap();
            (best(5, || ws.cell(0, 0).unwrap().is_some()), best(5, || ws.cell(rows - 1, 0).unwrap().is_some()))
        };
        let mut k = 0.0;
        let edit = best(5, || {
            k += 1.0;
            wb.worksheet().unwrap().cell_mut(rows - 1, 1).unwrap().set_number(k).unwrap()
        });
        let save = best(3, || wb.to_bytes().unwrap());
        let part_name = wb.worksheet_part().to_owned();
        let ser = best(3, || wb.package().read_part(&part_name).unwrap());
        let xml = wb.package().read_part(&part_name).unwrap();
        let deflate = |level: Option<i64>| {
            best(3, || {
                use std::io::Write;
                let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
                let o = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated).compression_level(level);
                z.start_file("x.xml", o).unwrap();
                z.write_all(&xml).unwrap();
                z.finish().unwrap().into_inner().len()
            })
        };
        println!(
            "| {n} ({rows}) | {} | {} | {} | {} | {} | {} | {} |",
            fmt_dur(first),
            fmt_dur(last),
            fmt_dur(edit),
            fmt_dur(save),
            fmt_dur(ser),
            fmt_dur(deflate(None)),
            fmt_dur(deflate(Some(1)))
        );
    }

    println!("\n## From-scratch writes: cost of one row (10 cells) after k rows (single measurement each)\n");
    println!("| k (rows already written) | one more row |");
    println!("|---|---|");
    let mut wb = ooxml_xlsx::Workbook::new().unwrap();
    let mut ws = wb.add_worksheet().unwrap();
    let mut written = 0u32;
    for k in [100u32, 1000, 5000, 10_000] {
        while written < k {
            for c in 0..COLS {
                ws.write_number(written, c, 1.0).unwrap();
            }
            written += 1;
        }
        let t = Instant::now();
        for c in 0..COLS {
            ws.write_number(written, c, 1.0).unwrap();
        }
        written += 1;
        println!("| {k} | {} |", fmt_dur(t.elapsed()));
    }

    println!("\n## PowerPoint: 1,000 generated slides\n");
    let base = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../reference/fixtures/pptx/python-pptx/ph-inherit-props.pptx")).unwrap();
    let deck = generate_deck(&base, 1000, 10, 5);
    let xml: usize = (0..1000).map(|k| part(&deck, &format!("ppt/slides/gen{k}.xml")).len()).sum();
    let h0 = ALLOC.current();
    let t = Instant::now();
    let mut p = ooxml_pptx::Presentation::from_bytes(deck.clone()).unwrap();
    let open = t.elapsed();
    let t = Instant::now();
    for i in 0..p.slide_count() {
        p.slide(i).unwrap();
    }
    let parse = t.elapsed();
    let retained = ALLOC.current() - h0;
    let t = Instant::now();
    let mut shapes = 0;
    for i in 0..p.slide_count() {
        shapes += p.slide(i).unwrap().shapes().len();
    }
    let walk = t.elapsed();
    ALLOC.reset_peak();
    let hp = ALLOC.current();
    let t = Instant::now();
    let mut chars = 0;
    for i in 0..p.slide_count() {
        chars += p.slide(i).unwrap().shapes().iter().filter_map(|s| s.text_body()).map(|b| b.text().len()).sum::<usize>();
    }
    let text = t.elapsed();
    let text_peak = ALLOC.peak() - hp;
    println!("| Step | Time | Note |\n|---|---|---|");
    println!("| `Presentation::from_bytes` | {} | |", fmt_dur(open));
    println!("| parse all 1,000 slides (`slide(i)`, first call) | {} | {} of slide XML; {} retained in the package after parsing ({:.0}× the XML) |", fmt_dur(parse), fmt_bytes(xml), fmt_bytes(retained), retained as f64 / xml as f64);
    println!("| `shapes()` on every slide (already parsed) | {} | {shapes} shapes |", fmt_dur(walk));
    println!("| `shapes()` + `text_body().text()` on every slide | {} | {chars} bytes of text; transient heap peak {} |", fmt_dur(text), fmt_bytes(text_peak));
}
