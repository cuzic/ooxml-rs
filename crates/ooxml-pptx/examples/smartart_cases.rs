//! Writes the SmartArt cases for `tests/oracle/smartart.py` (ADR-0013 §7):
//! every node of every SmartArt in the fixtures gets a two-line text;
//! outputs in `<dir>/NNNN.pptx` with `<dir>/cases.json`. Refused edits are
//! skipped (refusals are covered in `tests/smartart.rs`).

use std::path::Path;

use ooxml_pptx::{Presentation, ShapeKind};
use serde_json::{Value, json};

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../reference/fixtures/pptx");
const TEXT: &str = "Oracle first\nOracle second";

fn main() {
    let dir = std::env::args().nth(1).expect("usage: smartart_cases <out-dir>");
    std::fs::create_dir_all(&dir).unwrap();
    let mut files: Vec<String> = Vec::new();
    for d in ["libreoffice", "open-xml-sdk", "poi", "python-pptx"] {
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
            let ids: Vec<u32> = slide.shapes().iter().filter(|s| matches!(s.kind(), ShapeKind::GraphicFrame { .. })).filter_map(|s| s.id().ok()).collect();
            for id in ids {
                let mut probe = Presentation::from_bytes(src.clone()).unwrap();
                let Ok(mut sa) = probe.smart_art_mut(si, id) else { continue };
                let nodes = sa.nodes().unwrap();
                for (k, n) in nodes.iter().enumerate() {
                    // every setter, on every node (review #63 m4)
                    for op in ["set_text", "set_paragraph_text", "set_run_text"] {
                        let mut p = Presentation::from_bytes(src.clone()).unwrap();
                        let mut sa = p.smart_art_mut(si, id).unwrap();
                        // run edits need a run: give the node one first
                        if op == "set_run_text" && sa.set_text(&n.model_id, "seed").is_err() {
                            continue;
                        }
                        let text = if op == "set_text" { TEXT.to_owned() } else { format!("Oracle {op} {k}") };
                        let r = match op {
                            "set_text" => sa.set_text(&n.model_id, &text),
                            "set_paragraph_text" => sa.set_paragraph_text(&n.model_id, 0, &text),
                            _ => sa.set_run_text(&n.model_id, 0, 0, &text),
                        };
                        if r.is_err() {
                            continue;
                        }
                        let out = format!("{:04}.pptx", cases.len());
                        p.save(Path::new(&dir).join(&out)).unwrap();
                        cases.push(json!({"input": f, "output": out, "slide": si, "frame": id, "node": n.model_id, "op": op, "text": text}));
                    }
                }
                let mut p = Presentation::from_bytes(src.clone()).unwrap();
                if p.smart_art_mut(si, id).unwrap().drop_drawing_cache().is_ok() {
                    let out = format!("{:04}.pptx", cases.len());
                    p.save(Path::new(&dir).join(&out)).unwrap();
                    cases.push(json!({"input": f, "output": out, "slide": si, "frame": id, "op": "drop_drawing_cache"}));
                }
            }
        }
    }
    std::fs::write(Path::new(&dir).join("cases.json"), serde_json::to_string_pretty(&cases).unwrap()).unwrap();
    println!("{} cases", cases.len());
}
