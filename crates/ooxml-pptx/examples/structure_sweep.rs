//! Corpus sweep for Phase 2 (ADR-0012 §10): for every `.pptx` under the
//! given directories and every slide, `delete_slide` and `duplicate_slide`;
//! a success must reopen with the expected slide count, a refusal must
//! leave the package unchanged. With `--out <dir>`, the first successful
//! output of each file and operation is saved for an external reader
//! (python-pptx). Prints a summary by outcome.
//!
//! `cargo run --release -p ooxml-pptx --example structure_sweep -- [--out DIR] DIR...`

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use ooxml_pptx::Presentation;

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
    let (mut decks, mut slides, mut saved) = (0, 0, 0);
    let mut manifest = String::new();
    for f in &files {
        let Ok(src) = std::fs::read(f) else { continue };
        if !seen.insert(src.clone()) {
            continue;
        }
        let Ok(p0) = Presentation::from_bytes(src.clone()) else {
            *tally.entry("open refused".into()).or_default() += 1;
            continue;
        };
        let base = p0.to_bytes().unwrap();
        let n = p0.slide_count();
        decks += 1;
        slides += n;
        for op in ["delete", "duplicate"] {
            let mut wrote = false;
            for i in 0..n {
                let mut p = Presentation::from_bytes(src.clone()).unwrap();
                let r = if op == "delete" { p.delete_slide(i) } else { p.duplicate_slide(i, None).map(|_| ()) };
                let out = p.to_bytes().unwrap();
                let key = match r {
                    Ok(()) => {
                        let want = if op == "delete" { n - 1 } else { n + 1 };
                        match Presentation::from_bytes(out.clone()) {
                            Ok(q) if q.slide_count() == want => {
                                if let Some(d) = &out_dir
                                    && !wrote
                                {
                                    std::fs::create_dir_all(d).unwrap();
                                    std::fs::write(Path::new(d).join(format!("{saved:04}-{op}.pptx")), &out).unwrap();
                                    manifest.push_str(&format!("{saved:04}-{op}.pptx\t{i}\t{}\n", f.display()));
                                    saved += 1;
                                    wrote = true;
                                }
                                format!("{op}: ok")
                            }
                            Ok(_) => panic!("{}: {op} {i}: wrong slide count after reopen", f.display()),
                            Err(e) => panic!("{}: {op} {i}: output does not reopen: {e}", f.display()),
                        }
                    }
                    Err(e) => {
                        assert_eq!(out, base, "{}: {op} {i}: refusal {e} changed the package", f.display());
                        let s = e.to_string();
                        let s = s.split(" (").next().unwrap().to_owned();
                        format!("{op}: refused: {}", s.chars().take(110).collect::<String>())
                    }
                };
                *tally.entry(key).or_default() += 1;
            }
        }
    }
    if let Some(d) = &out_dir {
        std::fs::write(Path::new(d).join("manifest.tsv"), manifest).unwrap();
    }
    println!("{decks} decks, {slides} slides, {saved} outputs saved");
    for (k, v) in tally {
        println!("{v:6}  {k}");
    }
}
