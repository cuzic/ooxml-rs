//! Writes the Phase 1b table cases for the independent python-pptx
//! read-back (ADR-0013 §7): every table of every fixture, each row and
//! column insert/delete position, a 2-row merge where one fits, and a
//! border; saved to `<dir>/NNN.pptx` with `<dir>/cases.json`. Refused
//! operations are skipped (refusals are covered in `tests/tables.rs`).

use std::path::Path;

use ooxml_pptx::{Edge, Line, Presentation, ShapeKind, TableColor};
use serde_json::{Value, json};

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../reference/fixtures/pptx");

fn main() {
    let dir = std::env::args().nth(1).expect("usage: table_cases <out-dir>");
    std::fs::create_dir_all(&dir).unwrap();
    let mut files: Vec<String> = Vec::new();
    for d in ["libreoffice", "open-xml-sdk", "poi", "project", "python-pptx"] {
        for e in std::fs::read_dir(format!("{FIX}/{d}")).unwrap() {
            let p = e.unwrap().path();
            if p.extension().is_some_and(|x| x == "pptx") {
                files.push(format!("{d}/{}", p.file_name().unwrap().to_str().unwrap()));
            }
        }
    }
    files.sort();
    let mut cases: Vec<Value> = Vec::new();
    for f in &files {
        let src = std::fs::read(format!("{FIX}/{f}")).unwrap();
        let mut p0 = Presentation::from_bytes(src.clone()).unwrap();
        for si in 0..p0.slide_count() {
            let Ok(slide) = p0.slide(si) else { continue };
            let tables: Vec<u32> = slide.shapes().iter().filter(|s| *s.kind() == ShapeKind::Table).filter_map(|s| s.id().ok()).collect();
            for id in tables {
                let Some((rows, cols)) = size(&src, si, id) else { continue };
                let mut ops: Vec<(&str, Value)> = Vec::new();
                for a in 0..=rows {
                    ops.push(("insert_row", json!({"at": a})));
                }
                for a in 0..rows {
                    ops.push(("delete_row", json!({"at": a})));
                }
                for a in 0..=cols {
                    ops.push(("insert_column", json!({"at": a})));
                }
                for a in 0..cols {
                    ops.push(("delete_column", json!({"at": a})));
                }
                for r in 0..rows.saturating_sub(1) {
                    ops.push(("merge", json!({"r1": r, "c1": 0, "r2": r + 1, "c2": 0})));
                }
                ops.push(("border", json!({"r": 0, "c": 0})));
                for (op, args) in ops {
                    let mut p = Presentation::from_bytes(src.clone()).unwrap();
                    let r = {
                        let mut s = p.slide_mut(si).unwrap();
                        let mut sh = s.shape_mut_by_id(id).unwrap();
                        let Ok(mut t) = sh.table_mut() else { continue };
                        let n = |k: &str| args[k].as_u64().unwrap() as usize;
                        match op {
                            "insert_row" => t.insert_row(n("at")),
                            "delete_row" => t.delete_row(n("at")),
                            "insert_column" => t.insert_column(n("at")),
                            "delete_column" => t.delete_column(n("at")),
                            "merge" => t.merge(n("r1"), n("c1"), n("r2"), n("c2")),
                            _ => t.set_border(0, 0, Edge::Bottom, &Line::Solid { width: 12700, color: TableColor::Rgb([0xC0, 0, 0]) }),
                        }
                    };
                    if r.is_err() {
                        continue;
                    }
                    let out = format!("{:04}.pptx", cases.len());
                    p.save(Path::new(&dir).join(&out)).unwrap();
                    cases.push(json!({"input": f, "output": out, "slide": si, "shape_id": id, "op": op, "args": args}));
                }
            }
        }
    }
    std::fs::write(Path::new(&dir).join("cases.json"), serde_json::to_string_pretty(&cases).unwrap()).unwrap();
    println!("{} cases", cases.len());
}

fn size(src: &[u8], si: usize, id: u32) -> Option<(usize, usize)> {
    let mut p = Presentation::from_bytes(src.to_vec()).ok()?;
    let mut s = p.slide_mut(si).ok()?;
    let mut sh = s.shape_mut_by_id(id).ok()?;
    let t = sh.table_mut().ok()?;
    Some(t.size())
}
