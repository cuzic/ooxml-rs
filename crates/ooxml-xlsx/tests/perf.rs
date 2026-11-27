//! Timing tests for the per-cell API (ignored by default; run with
//! `cargo test --release -p ooxml-xlsx --test perf -- --ignored --nocapture`).
//! They print timings and assert only correctness, never a time budget.

use std::fmt::Write as _;
use std::io::{Cursor, Write};
use std::time::Instant;

use ooxml_xlsx::{CellValue, Workbook};

/// A one-sheet workbook of `rows` × `cols` numeric cells.
fn sheet(rows: u32, cols: u32) -> Vec<u8> {
    let mut sd = String::with_capacity(rows as usize * cols as usize * 24);
    for r in 1..=rows {
        let _ = write!(sd, r#"<row r="{r}">"#);
        for c in 0..cols {
            let _ = write!(sd, r#"<c r="{}{r}"><v>{}</v></c>"#, ooxml_xlsx::CellIndex::column_name(c), r * 100 + c);
        }
        sd.push_str("</row>");
    }
    let sml = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
    let files = [
        ("[Content_Types].xml", r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#.to_string()),
        ("_rels/.rels", r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#.to_string()),
        ("xl/workbook.xml", format!(r#"<workbook xmlns="{sml}" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="S" sheetId="1" r:id="rId1"/></sheets></workbook>"#)),
        ("xl/_rels/workbook.xml.rels", r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#.to_string()),
        ("xl/worksheets/sheet1.xml", format!(r#"<worksheet xmlns="{sml}"><sheetData>{sd}</sheetData></worksheet>"#)),
    ];
    let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (n, b) in files {
        zw.start_file(n, zip::write::SimpleFileOptions::default()).unwrap();
        zw.write_all(b.as_bytes()).unwrap();
    }
    zw.finish().unwrap().into_inner()
}

/// xorshift for reproducible "random" cells.
fn rng(seed: &mut u64) -> u64 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    *seed
}

fn read_all_per_cell(rows: u32, cols: u32, sample: Option<u32>) {
    let bytes = sheet(rows, cols);
    let t = Instant::now();
    let mut wb = Workbook::from_bytes(bytes).unwrap();
    let open = t.elapsed();
    let ws = wb.worksheet().unwrap();
    let t = Instant::now();
    let mut n = 0u64;
    let row_limit = sample.unwrap_or(rows);
    for r in 0..row_limit {
        for c in 0..cols {
            let v = ws.cell(r, c).unwrap().unwrap().value().unwrap();
            assert_eq!(v, CellValue::Number(f64::from((r + 1) * 100 + c)));
            n += 1;
        }
    }
    let el = t.elapsed();
    let per = el.as_secs_f64() / n as f64;
    println!(
        "read per cell {rows}x{cols}: open {open:?}; {n} cells in {el:?} ({:.2} µs/cell; all {} cells ≈ {:.2} s)",
        per * 1e6,
        u64::from(rows) * u64::from(cols),
        per * f64::from(rows) * f64::from(cols)
    );
}

#[test]
#[ignore = "timing (run with --release --ignored --nocapture)"]
fn read_all_cells_100k() {
    read_all_per_cell(10_000, 10, std::env::var("PERF_SAMPLE_ROWS").ok().map(|s| s.parse().unwrap()));
}

#[test]
#[ignore = "timing (run with --release --ignored --nocapture)"]
fn read_all_cells_1m() {
    read_all_per_cell(100_000, 10, std::env::var("PERF_SAMPLE_ROWS").ok().map(|s| s.parse().unwrap()));
}

#[test]
#[ignore = "timing (run with --release --ignored --nocapture)"]
fn write_100k_new_cells() {
    let t = Instant::now();
    let mut wb = Workbook::new().unwrap();
    {
        let mut ws = wb.add_worksheet().unwrap();
        for r in 0..10_000u32 {
            for c in 0..10u32 {
                ws.write_number(r, c, f64::from(r * 10 + c)).unwrap();
            }
        }
    }
    let write = t.elapsed();
    let t = Instant::now();
    let out = wb.to_bytes().unwrap();
    println!("write 100k new cells: {write:?}; serialize {:?} ({} bytes)", t.elapsed(), out.len());
    let mut again = Workbook::from_bytes(out).unwrap();
    let ws = again.worksheet().unwrap();
    assert_eq!(ws.cell(9_999, 9).unwrap().unwrap().value().unwrap(), CellValue::Number(99_999.0));
}

#[test]
#[ignore = "timing (run with --release --ignored --nocapture)"]
fn random_edits_1000_on_1m() {
    let bytes = sheet(100_000, 10);
    let t0 = Instant::now();
    let mut wb = Workbook::from_bytes(bytes).unwrap();
    let open = t0.elapsed();
    let t = Instant::now();
    let mut seed = 0x9E37_79B9_7F4A_7C15u64;
    {
        let mut ws = wb.worksheet().unwrap();
        for k in 0..1000u32 {
            let r = (rng(&mut seed) % 100_000) as u32;
            let c = (rng(&mut seed) % 10) as u32;
            ws.cell_mut(r, c).unwrap().set_number(f64::from(k)).unwrap();
        }
    }
    let edits = t.elapsed();
    let t = Instant::now();
    let out = wb.to_bytes().unwrap();
    let save = t.elapsed();
    println!(
        "1M cells: open {open:?}; 1,000 random edits {edits:?} ({:.2} ms/edit); save {save:?}; total {:?} ({} bytes)",
        edits.as_secs_f64() * 1e3 / 1000.0,
        t0.elapsed(),
        out.len()
    );
}

fn read_all_bulk(rows: u32, cols: u32) {
    let bytes = sheet(rows, cols);
    let mut wb = Workbook::from_bytes(bytes).unwrap();
    let ws = wb.worksheet().unwrap();
    let t = Instant::now();
    let mut n = 0u64;
    let mut sum = 0f64;
    for c in ws.cells() {
        if let CellValue::Number(v) = c.unwrap().value().unwrap() {
            sum += v;
        }
        n += 1;
    }
    assert_eq!(n, u64::from(rows) * u64::from(cols));
    println!("read bulk {rows}x{cols}: {n} cells in {:?} (checksum {sum})", t.elapsed());
}

#[test]
#[ignore = "timing (run with --release --ignored --nocapture)"]
fn read_all_cells_bulk_100k() {
    read_all_bulk(10_000, 10);
}

#[test]
#[ignore = "timing (run with --release --ignored --nocapture)"]
fn read_all_cells_bulk_1m() {
    read_all_bulk(100_000, 10);
}

/// PR #29 review m3: one wide row (new cells ascending and descending) and
/// rows inserted in descending order.
#[test]
#[ignore = "timing (run with --release --ignored --nocapture)"]
fn wide_row_and_descending_rows() {
    for desc in [false, true] {
        let mut wb = Workbook::new().unwrap();
        let t = Instant::now();
        {
            let mut ws = wb.add_worksheet().unwrap();
            for k in 0..16_384u32 {
                let c = if desc { 16_383 - k } else { k };
                ws.write_number(0, c, f64::from(c)).unwrap();
            }
        }
        println!("16,384 cells in one row ({}): {:?}", if desc { "descending" } else { "ascending" }, t.elapsed());
        let mut again = Workbook::from_bytes(wb.to_bytes().unwrap()).unwrap();
        assert_eq!(again.worksheet().unwrap().cells().count(), 16_384);
    }
    let mut wb = Workbook::new().unwrap();
    let t = Instant::now();
    {
        let mut ws = wb.add_worksheet().unwrap();
        for r in (0..40_000u32).rev() {
            ws.write_number(r, 0, f64::from(r)).unwrap();
        }
    }
    println!("40k rows descending: {:?}", t.elapsed());
}

#[test]
#[ignore = "timing (run with --release --ignored --nocapture)"]
fn open_workbook_phases() {
    use ooxml_core::Package;
    use std::sync::Arc;

    let bytes: Arc<[u8]> = std::env::var_os("OOXML_PERF_WORKBOOK")
        .map_or_else(|| sheet(10_000, 10), |path| std::fs::read(path).unwrap())
        .into();
    let mut samples = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
    for trial in 0..7 {
        let start = Instant::now();
        let opened = Workbook::from_bytes(bytes.clone()).unwrap();
        let total = start.elapsed();

        let start = Instant::now();
        let mut package = Package::from_bytes(bytes.clone()).unwrap();
        let opc = start.elapsed();
        let start = Instant::now();
        package.xml_part(opened.worksheet_part()).unwrap();
        let worksheet_xml = start.elapsed();

        let start = Instant::now();
        let pre_parsed = Workbook::from_package(package).unwrap();
        let derive_workbook = start.elapsed();
        assert!(opened.worksheet_names().eq(pre_parsed.worksheet_names()));
        if trial >= 2 {
            for (sample, duration) in samples.iter_mut().zip([total, opc, worksheet_xml, derive_workbook]) {
                sample.push(duration);
            }
        }
    }
    for (label, sample) in ["normal open", "OPC open", "first visible worksheet unzip and XML parse", "workbook from pre-parsed package"]
        .into_iter()
        .zip(&mut samples)
    {
        sample.sort_unstable();
        println!("{label}: {:?}", sample[sample.len() / 2]);
    }
}
