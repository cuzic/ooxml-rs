//! Corpus sweep for SmartArt text (ADR-0013 §4, §7): for every SmartArt of
//! every `.pptx` under the given directories, the cache report (nodes,
//! consistent nodes, paired runs), then for every node `set_text` with a
//! two-line text: a success must reopen with the node's new paragraphs in
//! the data and in every cache slice; a refusal must leave the package
//! unchanged. With `--out DIR`, one output per SmartArt is saved with a
//! `cases.json` for `tests/oracle/smartart.py`.
//!
//! `cargo run --release -p ooxml-pptx --example smartart_sweep -- [--out DIR] DIR...`

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

const TEXT: &str = "Swept first\nSwept second";

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
    let (mut frames, mut nodes, mut shown, mut consistent, mut run_pairs) = (0, 0, 0, 0, 0);
    let mut cases = Vec::new();
    for f in &files {
        let Ok(src) = std::fs::read(f) else { continue };
        if !seen.insert(src.clone()) {
            continue;
        }
        let Ok(mut p0) = Presentation::from_bytes(src.clone()) else { continue };
        let base = p0.to_bytes().unwrap();
        for si in 0..p0.slide_count() {
            let Ok(slide) = p0.slide(si) else { continue };
            let ids: Vec<u32> = slide.shapes().iter().filter(|s| matches!(s.kind(), ShapeKind::GraphicFrame { .. })).filter_map(|s| s.id().ok()).collect();
            for id in ids {
                let mut p = Presentation::from_bytes(src.clone()).unwrap();
                let Ok(mut sa) = p.smart_art_mut(si, id) else { continue };
                frames += 1;
                match sa.cache_report() {
                    Ok(r) => {
                        nodes += r.nodes;
                        shown += r.shown;
                        consistent += r.consistent;
                        run_pairs += r.run_pairs;
                    }
                    Err(e) => *tally.entry(format!("report: {}", e.to_string().split(':').next().unwrap())).or_default() += 1,
                }
                let ids: Vec<String> = sa.nodes().unwrap().into_iter().map(|n| n.model_id).collect();
                let mut saved = false;
                for mid in ids {
                    let mut p = Presentation::from_bytes(src.clone()).unwrap();
                    let r = p.smart_art_mut(si, id).unwrap().set_text(&mid, TEXT);
                    let out = p.to_bytes().unwrap();
                    let key = match r {
                        Ok(()) => {
                            let mut q = Presentation::from_bytes(out.clone()).unwrap_or_else(|e| panic!("{}: does not reopen: {e}", f.display()));
                            let mut s = q.smart_art_mut(si, id).unwrap();
                            let n = s.nodes().unwrap().into_iter().find(|n| n.model_id == mid).unwrap();
                            assert_eq!(n.paragraphs, ["Swept first", "Swept second"], "{}", f.display());
                            s.cache_report().unwrap_or_else(|e| panic!("{}: cache inconsistent after the edit: {e}", f.display()));
                            if let Some(d) = &out_dir
                                && !saved
                            {
                                saved = true;
                                std::fs::create_dir_all(d).unwrap();
                                let name = format!("{:04}.pptx", cases.len());
                                std::fs::write(Path::new(d).join(&name), &out).unwrap();
                                cases.push(json!({"input": f.display().to_string(), "output": name, "slide": si, "frame": id, "node": mid, "text": TEXT}));
                            }
                            "set_text: ok".to_owned()
                        }
                        Err(e) => {
                            assert_eq!(out, base, "{}: refusal {e} changed the package", f.display());
                            format!("set_text: refused: {}", e.to_string().chars().take(80).collect::<String>())
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
    println!("{} decks, {frames} SmartArts, {nodes} nodes, {shown} shown in a cache, {consistent} consistent, {run_pairs} paired runs, {} outputs", seen.len(), cases.len());
    for (k, v) in tally {
        println!("{v:6}  {k}");
    }
}
