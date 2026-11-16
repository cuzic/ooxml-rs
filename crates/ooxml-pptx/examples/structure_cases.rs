//! Writes the Phase 2 structure-oracle cases (ADR-0012 §10): every
//! operation on fixture decks, saved to `<dir>/NNN.pptx`, with
//! `<dir>/cases.json` describing input, operation and the parts the
//! operation may change. `tests/oracle/structure.py` checks them with
//! python-pptx and its own model. Refused operations are skipped (the
//! refusals are covered in `tests/structure.rs`).

use std::path::Path;

use ooxml_pptx::Presentation;
use serde_json::{Value, json};

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../reference/fixtures/pptx");

const CHANGED: &[&str] = &[
    "ppt/presentation.xml",
    "ppt/_rels/presentation.xml.rels",
    "[Content_Types].xml",
    "docProps/app.xml",
    "ppt/viewProps.xml",
    "ppt/_rels/viewProps.xml.rels",
];

fn main() {
    let dir = std::env::args().nth(1).expect("usage: structure_cases <out-dir>");
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
    // the built-in template (ADR-0012 §8), as an input of its own
    let new_deck = Presentation::new_with(ooxml_pptx::NewOptions { created: std::time::UNIX_EPOCH }).unwrap().to_bytes().unwrap();
    std::fs::write(Path::new(&dir).join("new.pptx"), &new_deck).unwrap();
    files.insert(0, "@new.pptx".into());
    let mut cases: Vec<Value> = Vec::new();
    for f in &files {
        let src = match f.strip_prefix('@') {
            Some(n) => std::fs::read(Path::new(&dir).join(n)).unwrap(),
            None => std::fs::read(format!("{FIX}/{f}")).unwrap(),
        };
        let mut p0 = Presentation::from_bytes(src.clone()).unwrap();
        let n = p0.slide_count();
        let layouts = p0.layouts().unwrap();
        let mut ops: Vec<(&str, Value)> = Vec::new();
        for (k, l) in layouts.iter().enumerate().take(if f.starts_with('@') { 11 } else { 3 }) {
            let at = if k == 0 { Value::Null } else { json!(k.min(n)) };
            ops.push(("add", json!({"layout_part": l.part, "layout": l.name, "at": at})));
        }
        for i in 0..n {
            ops.push(("delete", json!({"index": i})));
            ops.push(("duplicate", json!({"index": i, "at": null})));
        }
        if n > 1 {
            ops.push(("move", json!({"from": 0, "to": n - 1})));
            ops.push(("move", json!({"from": n - 1, "to": 0})));
        }
        for (op, args) in ops {
            let mut p = Presentation::from_bytes(src.clone()).unwrap();
            let r = match op {
                "add" => {
                    let l = p.layouts().unwrap().into_iter().find(|l| l.part == args["layout_part"].as_str().unwrap()).unwrap();
                    p.add_slide(&l, args["at"].as_u64().map(|v| v as usize)).map(|_| ())
                }
                "delete" => p.delete_slide(args["index"].as_u64().unwrap() as usize),
                "duplicate" => p.duplicate_slide(args["index"].as_u64().unwrap() as usize, None).map(|_| ()),
                _ => p.move_slide(args["from"].as_u64().unwrap() as usize, args["to"].as_u64().unwrap() as usize),
            };
            if r.is_err() {
                continue;
            }
            let out = format!("{:03}.pptx", cases.len());
            p.save(Path::new(&dir).join(&out)).unwrap();
            cases.push(json!({"input": f, "output": out, "op": op, "args": args, "changed": CHANGED}));
        }
    }
    std::fs::write(Path::new(&dir).join("cases.json"), serde_json::to_string_pretty(&cases).unwrap()).unwrap();
    println!("{} cases", cases.len());
}
