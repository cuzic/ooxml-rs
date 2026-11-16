//! ooxml-rs performance baseline (`docs/testing/performance-baseline-2026-11.md`).
//!
//! ```sh
//! cargo bench -p ooxml-bench --bench baseline              # everything
//! cargo bench -p ooxml-bench --bench baseline -- xlsx      # cases whose id contains "xlsx"
//! OOXML_BENCH_MAX=100000 cargo bench -p ooxml-bench ...    # largest generated size (default 1,000,000 cells)
//! ```
//!
//! A custom harness instead of criterion: every case also reports its heap
//! peak (counting global allocator), several cases take seconds per run,
//! and the output is one Markdown table for the report. Each case is timed
//! at least 3 times and until a 3 s budget is spent (at most 15 runs); a
//! single run over the budget is reported as is (runs = 1). Quadratic
//! cases are skipped at a size where they are predicted to exceed 60 s.

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::time::Duration;

use ooxml_bench::{PeakAlloc, Sample, fmt_bytes, fmt_dur, generate_deck, measure};
use ooxml_core::{Package, RawNode, XmlDocument};

#[global_allocator]
static ALLOC: PeakAlloc = PeakAlloc::new();

const BUDGET: Duration = Duration::from_secs(3);
/// A case whose predicted time at the next size (previous median × the
/// size ratio squared, for cases that scale quadratically) exceeds this is
/// skipped, with the prediction reported instead.
const SKIP_AFTER: Duration = Duration::from_secs(60);

/// Predicted time at `n` from a quadratic case measured at `prev_n`.
fn quadratic(prev: Option<(u32, Duration)>, n: u32) -> Option<Duration> {
    prev.map(|(pn, d)| d.mul_f64((f64::from(n) / f64::from(pn)).powi(2)))
}
const COLS: u32 = 10;

struct Bench {
    filter: Option<String>,
    rows: Vec<String>,
}

impl Bench {
    fn wants(&self, id: &str) -> bool {
        self.filter.as_deref().is_none_or(|f| id.contains(f))
    }

    /// Runs and records a case; returns the sample (None if filtered out).
    fn case<T>(&mut self, id: &str, input: &str, units: Option<(f64, &str)>, f: impl FnMut() -> T) -> Option<Sample> {
        if !self.wants(id) {
            return None;
        }
        let s = measure(&ALLOC, BUDGET, 3, 15, f);
        let rate = units.map_or(String::new(), |(n, u)| format!("{:.1} {u}/s", n / s.median.as_secs_f64()));
        let row = format!(
            "| `{id}` | {input} | {} | {} | {} | {} | {rate} |",
            fmt_dur(s.median),
            fmt_dur(s.min),
            s.runs,
            fmt_bytes(s.peak_bytes)
        );
        eprintln!("{row}");
        self.rows.push(row);
        Some(s)
    }

    fn skipped(&mut self, id: &str, input: &str, why: &str) {
        if self.wants(id) {
            let row = format!("| `{id}` | {input} | skipped: {why} | | | | |");
            eprintln!("{row}");
            self.rows.push(row);
        }
    }
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixtures(ext: &str) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    let mut stack = vec![root().join("reference/fixtures")];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case(ext)) {
                out.push((p.file_name().unwrap().to_string_lossy().into_owned(), std::fs::read(&p).unwrap()));
            }
        }
    }
    out.sort();
    out
}

/// `cells` cells, `COLS` per row: column A a unique string (shared string
/// table), the rest numbers. Written by rust_xlsxwriter.
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
    use std::io::Read;
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut f = z.by_name(name).unwrap();
    let mut v = Vec::new();
    f.read_to_end(&mut v).unwrap();
    v
}

/// Every `c/v` (and `c/is/t`) under `sheetData`, walked on the raw tree.
fn raw_cells(sheet: &RawNode) -> (usize, f64) {
    let (mut n, mut sum) = (0, 0.0);
    for sd in sheet.elements().filter(|e| e.name().local() == "sheetData") {
        for row in sd.elements() {
            for c in row.elements() {
                n += 1;
                if let Some(v) = c.elements().find(|e| e.name().local() == "v") {
                    sum += v.text().parse::<f64>().unwrap_or(0.0);
                }
            }
        }
    }
    (n, sum)
}

fn main() {
    let filter = std::env::args().skip(1).find(|a| !a.starts_with('-'));
    let max: u32 = std::env::var("OOXML_BENCH_MAX").ok().and_then(|v| v.parse().ok()).unwrap_or(1_000_000);
    let mut b = Bench { filter, rows: Vec::new() };
    let sizes: Vec<u32> = [10_000, 100_000, 1_000_000].into_iter().filter(|n| *n <= max).collect();

    // ---- inputs -----------------------------------------------------------
    let xlsx_fixtures = fixtures("xlsx");
    let pptx_fixtures = fixtures("pptx");
    let mut generated = Vec::new();
    for &n in &sizes {
        let label = format!("{}k cells", n / 1000);
        let bytes = generate_xlsx(n);
        eprintln!("generated {label}: {} (sheet1.xml {})", fmt_bytes(bytes.len()), fmt_bytes(part(&bytes, "xl/worksheets/sheet1.xml").len()));
        generated.push((n, label, bytes));
    }
    let base_deck = std::fs::read(root().join("reference/fixtures/pptx/python-pptx/ph-inherit-props.pptx")).unwrap();
    let decks: Vec<(usize, Vec<u8>)> = [100usize, 1000].into_iter().map(|n| (n, generate_deck(&base_deck, n, 10, 5))).collect();
    for (n, d) in &decks {
        eprintln!("generated deck {n} slides: {}", fmt_bytes(d.len()));
    }

    // ---- core -------------------------------------------------------------
    let all_fixtures: Vec<&(String, Vec<u8>)> = xlsx_fixtures.iter().chain(&pptx_fixtures).collect();
    let fixture_bytes: usize = all_fixtures.iter().map(|(_, b)| b.len()).sum();
    b.case(
        "core/package-passthrough/fixtures",
        &format!("{} xlsx+pptx fixtures, {}", all_fixtures.len(), fmt_bytes(fixture_bytes)),
        Some((fixture_bytes as f64 / 1e6, "MB")),
        || {
            for (_, bytes) in &all_fixtures {
                if let Ok(p) = Package::from_bytes(bytes.clone()) {
                    std::hint::black_box(p.to_bytes().unwrap());
                }
            }
        },
    );
    for (_, label, bytes) in &generated {
        b.case("core/package-passthrough/generated", &format!("{label}, {}", fmt_bytes(bytes.len())), Some((bytes.len() as f64 / 1e6, "MB")), || {
            Package::from_bytes(bytes.clone()).unwrap().to_bytes().unwrap()
        });
        b.case("core/package-parse-sheet-passthrough", &format!("{label}: open, parse sheet1.xml, save"), None, || {
            let mut p = Package::from_bytes(bytes.clone()).unwrap();
            p.xml_part("xl/worksheets/sheet1.xml").unwrap();
            p.to_bytes().unwrap()
        });
        let sheet = part(bytes, "xl/worksheets/sheet1.xml");
        let mb = sheet.len() as f64 / 1e6;
        let input = format!("{label}: sheet1.xml {}", fmt_bytes(sheet.len()));
        b.case("core/xml-parse", &input, Some((mb, "MB")), || XmlDocument::parse(&sheet).unwrap());
        let doc = XmlDocument::parse(&sheet).unwrap();
        b.case("core/xml-serialize-unedited", &input, Some((mb, "MB")), || doc.serialize());
        let mut dirty = doc.clone();
        dirty.force_rerender();
        b.case("core/xml-serialize-rerender", &input, Some((mb, "MB")), || dirty.serialize());
        b.case("core/xml-clone", &input, Some((mb, "MB")), || doc.clone());
        let cells = raw_cells(doc.root()).0 as f64;
        b.case("core/raw-walk-all-cells", &input, Some((cells, "cells")), || raw_cells(doc.root()));
    }

    // ---- xlsx: open, read --------------------------------------------------
    let mut prev_read: Option<(u32, Duration)> = None;
    for (n, label, bytes) in &generated {
        let n = *n;
        let cells = f64::from(n);
        b.case("xlsx/open/ooxml", label, Some((cells, "cells")), || ooxml_xlsx::Workbook::from_bytes(bytes.clone()).unwrap());
        b.case("xlsx/read-all/calamine", label, Some((cells, "cells")), || {
            use calamine::Reader;
            let mut wb: calamine::Xlsx<_> = calamine::open_workbook_from_rs(Cursor::new(bytes.clone())).unwrap();
            let range = wb.worksheet_range_at(0).unwrap().unwrap();
            range.rows().flatten().filter(|c| !matches!(c, calamine::Data::Empty)).count()
        });
        b.case("xlsx/read-all/ooxml-raw-walk", &format!("{label} (open + Package raw tree walk, no public API)"), Some((cells, "cells")), || {
            let mut p = Package::from_bytes(bytes.clone()).unwrap();
            raw_cells(p.xml_part("xl/worksheets/sheet1.xml").unwrap().root())
        });
        let id = "xlsx/read-all/ooxml-cell-api";
        if let Some(p) = quadratic(prev_read, n).filter(|p| *p > SKIP_AFTER) {
            b.skipped(id, label, &format!("predicted ≈ {} (quadratic: Worksheet::cell scans every row)", fmt_dur(p)));
        } else if let Some(s) = b.case(id, &format!("{label} (open + Worksheet::cell for every cell)"), Some((cells, "cells")), || {
            let mut wb = ooxml_xlsx::Workbook::from_bytes(bytes.clone()).unwrap();
            let ws = wb.worksheet().unwrap();
            let mut k = 0usize;
            for r in 0..n / COLS {
                for c in 0..COLS {
                    k += usize::from(ws.cell(r, c).unwrap().unwrap().value().is_ok());
                }
            }
            k
        }) {
            prev_read = Some((n, s.median));
        }
    }

    // ---- xlsx: edit N existing cells and save -------------------------------
    for (n, label, bytes) in &generated {
        let rows = n / COLS;
        for edits in [100u32, 1000] {
            // spread over the sheet: every (rows/edits)-th row, column B
            let targets: Vec<(u32, u32)> = (0..edits).map(|i| ((i * (rows / edits)).min(rows - 1), 1)).collect();
            let input = format!("{label}, {edits} edits");
            b.case("xlsx/edit-save/ooxml", &input, Some((f64::from(edits), "edits")), || {
                let mut wb = ooxml_xlsx::Workbook::from_bytes(bytes.clone()).unwrap();
                {
                    let mut ws = wb.worksheet().unwrap();
                    for &(r, c) in &targets {
                        ws.cell_mut(r, c).unwrap().set_number(-1.5).unwrap();
                    }
                }
                wb.to_bytes().unwrap()
            });
            b.case("xlsx/edit-save/umya", &input, Some((f64::from(edits), "edits")), || {
                let mut book = umya_spreadsheet::reader::xlsx::read_reader(Cursor::new(bytes.clone()), true).unwrap();
                let ws = book.get_sheet_mut(&0).unwrap();
                for &(r, c) in &targets {
                    ws.get_cell_mut((c + 1, r + 1)).set_value_number(-1.5);
                }
                let mut out = Cursor::new(Vec::new());
                umya_spreadsheet::writer::xlsx::write_writer(&book, &mut out).unwrap();
                out.into_inner()
            });
        }
    }

    // ---- xlsx: write from scratch -------------------------------------------
    let mut prev_write: Option<(u32, Duration)> = None;
    for &n in &sizes {
        let label = format!("{}k cells", n / 1000);
        let cells = f64::from(n);
        b.case("xlsx/write-new/rust_xlsxwriter", &label, Some((cells, "cells")), || generate_xlsx(n));
        b.case("xlsx/write-new/rust_xlsxwriter-constant-memory", &label, Some((cells, "cells")), || {
            let mut wb = rust_xlsxwriter::Workbook::new();
            let ws = wb.add_worksheet_with_constant_memory().unwrap();
            for r in 0..n / COLS {
                ws.write_string(r, 0, format!("row {r}")).unwrap();
                for c in 1..COLS {
                    ws.write_number(r, c as u16, f64::from(r * COLS + c)).unwrap();
                }
            }
            wb.save_to_buffer().unwrap()
        });
        let id = "xlsx/write-new/ooxml";
        if let Some(p) = quadratic(prev_write, n).filter(|p| *p > SKIP_AFTER) {
            b.skipped(id, &label, &format!("predicted ≈ {} (quadratic: each write scans every row)", fmt_dur(p)));
        } else if let Some(s) = b.case(id, &label, Some((cells, "cells")), || {
            let mut wb = ooxml_xlsx::Workbook::new().unwrap();
            {
                let mut ws = wb.add_worksheet().unwrap();
                for r in 0..n / COLS {
                    ws.write_string(r, 0, &format!("row {r}")).unwrap();
                    for c in 1..COLS {
                        ws.write_number(r, c, f64::from(r * COLS + c)).unwrap();
                    }
                }
            }
            wb.to_bytes().unwrap()
        }) {
            prev_write = Some((n, s.median));
        }
    }

    // ---- pptx ---------------------------------------------------------------
    let pptx_bytes: usize = pptx_fixtures.iter().map(|(_, b)| b.len()).sum();
    let pptx_input = format!("{} pptx fixtures, {}", pptx_fixtures.len(), fmt_bytes(pptx_bytes));
    let all_decks: Vec<(String, &[u8])> = std::iter::once(None)
        .chain(decks.iter().map(Some))
        .map(|d| match d {
            None => (pptx_input.clone(), &[][..]),
            Some((n, bytes)) => (format!("generated deck: {n} slides × 10 shapes × 5 paragraphs, {}", fmt_bytes(bytes.len())), &bytes[..]),
        })
        .collect();
    for (input, deck) in &all_decks {
        let inputs: Vec<&[u8]> = if deck.is_empty() { pptx_fixtures.iter().map(|(_, b)| &b[..]).collect() } else { vec![deck] };
        b.case("pptx/open", input, None, || {
            inputs.iter().filter_map(|d| ooxml_pptx::Presentation::from_bytes(d.to_vec()).ok()).count()
        });
        b.case("pptx/read-all-text", &format!("{input} (open + every slide's shapes' text)"), None, || {
            let mut chars = 0usize;
            for d in &inputs {
                let Ok(mut p) = ooxml_pptx::Presentation::from_bytes(d.to_vec()) else { continue };
                for i in 0..p.slide_count() {
                    let Ok(s) = p.slide(i) else { continue };
                    chars += s.shapes().iter().filter_map(|sh| sh.text_body()).map(|t| t.text().len()).sum::<usize>();
                }
            }
            chars
        });
        b.case("pptx/set-text-all-save", &format!("{input} (open + set_text on every text body + save)"), None, || {
            let (mut ok, mut refused) = (0usize, 0usize);
            let mut out = 0usize;
            for d in &inputs {
                let Ok(mut p) = ooxml_pptx::Presentation::from_bytes(d.to_vec()) else { continue };
                for i in 0..p.slide_count() {
                    let Ok(mut s) = p.slide_mut(i) else { continue };
                    let paths: Vec<_> = s.as_slide().shapes().iter().filter(|sh| sh.text_body().is_some()).map(|sh| sh.path().clone()).collect();
                    for path in paths {
                        match s.shape_mut(&path).and_then(|mut sh| sh.text_body_mut()?.set_text("Replaced text")) {
                            Ok(()) => ok += 1,
                            Err(_) => refused += 1,
                        }
                    }
                }
                out += p.to_bytes().unwrap().len();
            }
            (ok, refused, out)
        });
    }

    println!("| Case | Input | Median | Min | Runs | Heap peak | Throughput |");
    println!("|---|---|---|---|---|---|---|");
    for r in &b.rows {
        println!("{r}");
    }
}
