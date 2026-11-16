//! ADR-0003 §3.1 measurement (review R2-M3): cost of Phase1 edits on a large
//! formula sheet, compared with revision 2's per-edit staged clone.
//!
//! `cargo run --release -p ooxml-xlsx --example phase1_bench [rows]`
//!
//! `WORST=1`: `B1` caches `#SPILL!` (depends on everything and seeds its
//! potential spill area, i.e. the whole sheet below and right of it): the
//! worst case for the dependents search (review R3-m4).

use std::fmt::Write as _;
use std::io::{Cursor, Write};
use std::time::Instant;

fn book(rows: u32, worst: bool) -> Vec<u8> {
    let mut sd = String::with_capacity(rows as usize * 400);
    for r in 1..=rows {
        let _ = write!(sd, r#"<row r="{r}"><c r="A{r}"><v>{r}</v></c>"#);
        for (i, c) in "BCDEFGHIJK".chars().enumerate() {
            let prev = (b'A' + i as u8) as char;
            if worst && r == 1 && c == 'B' {
                let _ = write!(sd, r#"<c r="B1" t="e"><f>_xlfn.SEQUENCE(A1)</f><v>#SPILL!</v></c>"#);
                continue;
            }
            let _ = write!(sd, r#"<c r="{c}{r}"><f>{prev}{r}+1</f><v>{}</v></c>"#, r as usize + i + 1);
        }
        sd.push_str("</row>");
    }
    let sml = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
    let files = [
        ("[Content_Types].xml", r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#.to_string()),
        ("_rels/.rels", r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#.to_string()),
        ("xl/workbook.xml", format!(r#"<workbook xmlns="{sml}" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="S" sheetId="1" r:id="rId1"/></sheets><calcPr calcId="191029"/></workbook>"#)),
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

fn main() {
    let rows: u32 = std::env::args().nth(1).map_or(20_000, |s| s.parse().unwrap());
    let worst = std::env::var("WORST").is_ok_and(|v| v == "1");
    let bytes = book(rows, worst);
    let t = Instant::now();
    let mut wb = ooxml_xlsx::Workbook::from_bytes(bytes).unwrap();
    let open = t.elapsed();
    let sheet_len = wb.package().read_part("xl/worksheets/sheet1.xml").unwrap().len();
    let doc = wb.package().parsed_part("xl/worksheets/sheet1.xml").unwrap().clone();
    let t = Instant::now();
    for _ in 0..5 {
        std::hint::black_box(doc.clone());
    }
    let clone = t.elapsed() / 5;
    let t = Instant::now();
    wb.worksheet().unwrap().cell_mut(0, 0).unwrap().set_number(-1.0).unwrap();
    let first = t.elapsed();
    let t = Instant::now();
    let n = 1000.min(rows - 1);
    for r in 1..=n {
        wb.worksheet().unwrap().cell_mut(r, 0).unwrap().set_number(f64::from(r) * 2.0).unwrap();
    }
    let thousand = t.elapsed();
    // Edits whose dependents are already stripped (row 1 again).
    let t = Instant::now();
    for k in 0..100 {
        wb.worksheet().unwrap().cell_mut(0, 0).unwrap().set_number(f64::from(k)).unwrap();
    }
    let again = t.elapsed() / 100;
    let t = Instant::now();
    let out = wb.to_bytes().unwrap();
    let save = t.elapsed();
    println!("rows {rows}, formulas {}, sheet {:.1} MB, worst case {worst}", rows * 10, sheet_len as f64 / 1e6);
    println!("open (census)                         {open:?}");
    println!("clone of the sheet tree (rev 2)       {clone:?} per edit");
    println!("first edit (graph build + 10 strips)  {first:?}");
    println!("{n} edits on new rows (10 strips each) {thousand:?} ({:?} each)", thousand / n);
    println!("edit with dependents already stripped {again:?}");
    println!("save                                  {save:?} ({} bytes)", out.len());
}
