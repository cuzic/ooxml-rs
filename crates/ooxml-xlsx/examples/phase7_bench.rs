//! ADR-0018 §3.8 workloads. Run each measurement in a fresh process
//! under `/usr/bin/time -v` to record peak resident memory.

use std::{hint::black_box, path::Path, time::Instant};

fn generate(path: &Path) {
    let mut wb = rust_xlsxwriter::Workbook::new();
    let ws = wb.add_worksheet();
    for row in 0..100_000 {
        ws.write_string(row, 0, format!("row {row}")).unwrap();
        for col in 1..10 {
            ws.write_number(row, col, f64::from(row * 10 + u32::from(col)))
                .unwrap();
        }
    }
    wb.save(path).unwrap();
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let [_, operation, path] = args.as_slice() else {
        panic!("usage: phase7_bench <generate|baseline|insert|delete|delete-99999> <input.xlsx>");
    };
    let path = Path::new(path);
    if operation == "generate" {
        generate(path);
        return;
    }

    let start = Instant::now();
    let mut wb = ooxml_xlsx::Workbook::open(path).unwrap();
    match operation.as_str() {
        "baseline" => {}
        "insert" => {
            black_box(wb.worksheet().unwrap().insert_rows(1, 1).unwrap());
        }
        "delete" => {
            black_box(wb.worksheet().unwrap().delete_rows(1, 100_000).unwrap());
        }
        "delete-99999" => {
            black_box(wb.worksheet().unwrap().delete_rows(0, 99_999).unwrap());
        }
        _ => panic!("unknown operation {operation}"),
    }
    black_box(wb.to_bytes().unwrap());
    println!("{operation} {:.3} s", start.elapsed().as_secs_f64());
}
