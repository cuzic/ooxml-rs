//! Corpus sweep for Phase 1b tables (ADR-0013 §7): for every table of
//! every `.pptx` under the given directories, every row/column insert and
//! delete position and a vertical merge of each pair of rows in column 0.
//! A success must reopen and still be well-formed; a refusal must leave the
//! package unchanged. With `--out DIR` the first success of each operation
//! per table is saved with a `cases.json` for `tests/oracle/tables.py`.
//!
//! `cargo run --release -p ooxml-pptx --example table_sweep -- [--out DIR] DIR...`

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use ooxml_pptx::{Presentation, ShapeKind};
use serde_json::json;

fn walk(d: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(d) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("pptx")) {
            out.push(p);
        }
    }
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let out_dir = args.iter().position(|a| a == "--out").map(|i| {
        let d = args.remove(i + 1);
        args.remove(i);
        d
    });
    let mut files = Vec::new();
    for d in &args {
        walk(Path::new(d), &mut files);
    }
    files.sort();
    let mut seen = std::collections::HashSet::new();
    let mut tally: BTreeMap<String, usize> = BTreeMap::new();
    let mut cases = Vec::new();
    let mut tables = 0;
    for f in &files {
        let Ok(src) = std::fs::read(f) else { continue };
        if !seen.insert(src.clone()) {
            continue;
        }
        let Ok(mut p0) = Presentation::from_bytes(src.clone()) else { continue };
        let base = p0.to_bytes().unwrap();
        for si in 0..p0.slide_count() {
            let Ok(slide) = p0.slide(si) else { continue };
            let ids: Vec<u32> = slide.shapes().iter().filter(|s| *s.kind() == ShapeKind::Table).filter_map(|s| s.id().ok()).collect();
            for id in ids {
                let mut probe = Presentation::from_bytes(src.clone()).unwrap();
                let Ok(mut s) = probe.slide_mut(si) else { continue };
                let Ok(mut sh) = s.shape_mut_by_id(id) else { continue };
                let Ok(t) = sh.table_mut() else { continue };
                let (rows, cols) = t.size();
                tables += 1;
                if let Err(e) = t.check() {
                    *tally.entry(format!("not well-formed: {}", e.to_string().chars().take(60).collect::<String>())).or_default() += 1;
                }
                let mut ops: Vec<(&str, usize, usize)> = Vec::new();
                ops.extend((0..=rows).map(|a| ("insert_row", a, 0)));
                ops.extend((0..rows).map(|a| ("delete_row", a, 0)));
                ops.extend((0..=cols).map(|a| ("insert_column", a, 0)));
                ops.extend((0..cols).map(|a| ("delete_column", a, 0)));
                ops.extend((0..rows.saturating_sub(1)).map(|a| ("merge", a, 0)));
                let mut saved = std::collections::HashSet::new();
                for (op, a, _) in ops {
                    let mut p = Presentation::from_bytes(src.clone()).unwrap();
                    let r = {
                        let mut s = p.slide_mut(si).unwrap();
                        let mut sh = s.shape_mut_by_id(id).unwrap();
                        let mut t = sh.table_mut().unwrap();
                        match op {
                            "insert_row" => t.insert_row(a),
                            "delete_row" => t.delete_row(a),
                            "insert_column" => t.insert_column(a),
                            "delete_column" => t.delete_column(a),
                            _ => t.merge(a, 0, a + 1, 0),
                        }
                    };
                    let out = p.to_bytes().unwrap();
                    let key = match r {
                        Ok(()) => {
                            let mut q = Presentation::from_bytes(out.clone()).unwrap_or_else(|e| panic!("{}: {op}({a}) does not reopen: {e}", f.display()));
                            let mut s = q.slide_mut(si).unwrap();
                            let mut sh = s.shape_mut_by_id(id).unwrap();
                            let t = sh.table_mut().unwrap();
                            t.check().unwrap_or_else(|e| panic!("{}: slide {si} table {id} {op}({a}): output not well-formed: {e}", f.display()));
                            if let Some(d) = &out_dir
                                && saved.insert(op)
                            {
                                std::fs::create_dir_all(d).unwrap();
                                let name = format!("{:05}.pptx", cases.len());
                                std::fs::write(Path::new(d).join(&name), &out).unwrap();
                                let args = if op == "merge" { json!({"r1": a, "c1": 0, "r2": a + 1, "c2": 0}) } else { json!({"at": a}) };
                                cases.push(json!({"input": f.display().to_string(), "output": name, "slide": si, "shape_id": id, "op": op, "args": args}));
                            }
                            format!("{op}: ok")
                        }
                        Err(e) => {
                            assert_eq!(out, base, "{}: {op}({a}): refusal {e} changed the package", f.display());
                            let s = e.to_string();
                            let s: String = s.split(':').next().unwrap().chars().take(70).collect();
                            format!("{op}: refused: {s}")
                        }
                    };
                    *tally.entry(key).or_default() += 1;
                }
            }
        }
    }
    if let Some(d) = &out_dir {
        std::fs::write(Path::new(d).join("cases.json"), serde_json::to_string_pretty(&cases).unwrap()).unwrap();
    }
    println!("{} decks, {tables} tables, {} outputs saved", seen.len(), cases.len());
    for (k, v) in tally {
        println!("{v:6}  {k}");
    }
}
