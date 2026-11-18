//! `cargo xtask pptx-template [--check]`: builds ooxml-pptx's built-in
//! template (ADR-0012 §8) from the hand-written parts in
//! `crates/ooxml-pptx/template/parts/` (zip order in `parts/ORDER`) into
//! `crates/ooxml-pptx/template/template.pptx`.
//!
//! `--check` compares **part content** — names, order and uncompressed
//! bytes — not the zip bytes, so a change of the deflate implementation does
//! not fail CI (ADR-0012 review m10).

use std::io::{Read as _, Write as _};
use std::path::Path;

use zip::write::{SimpleFileOptions, ZipWriter};
use zip::{CompressionMethod, DateTime, ZipArchive};

use crate::Res;

const DIR: &str = "crates/ooxml-pptx/template";

fn parts(root: &Path) -> Res<Vec<(String, Vec<u8>)>> {
    let dir = root.join(DIR).join("parts");
    let order = std::fs::read_to_string(dir.join("ORDER"))?;
    let names: Vec<&str> = order.lines().filter(|l| !l.trim().is_empty()).collect();
    // every file under parts/ (but ORDER) is listed, and only once
    let mut on_disk = Vec::new();
    collect(&dir, &dir, &mut on_disk)?;
    on_disk.retain(|n| n != "ORDER");
    on_disk.sort();
    let mut listed: Vec<String> = names.iter().map(|s| (*s).to_owned()).collect();
    listed.sort();
    if on_disk != listed {
        return Err(format!("{DIR}/parts/ORDER does not list exactly the files in parts/").into());
    }
    names.iter().map(|n| Ok(((*n).to_owned(), std::fs::read(dir.join(n))?))).collect()
}

fn collect(base: &Path, d: &Path, out: &mut Vec<String>) -> Res<()> {
    for e in std::fs::read_dir(d)? {
        let p = e?.path();
        if p.is_dir() {
            collect(base, &p, out)?;
        } else {
            out.push(p.strip_prefix(base)?.to_string_lossy().replace('\\', "/"));
        }
    }
    Ok(())
}

pub fn run(root: &Path, check: bool) -> Res<bool> {
    let parts = parts(root)?;
    let target = root.join(DIR).join("template.pptx");
    if check {
        let Ok(bytes) = std::fs::read(&target) else {
            eprintln!("{DIR}/template.pptx is missing (run `cargo xtask pptx-template`)");
            return Ok(false);
        };
        let mut z = ZipArchive::new(std::io::Cursor::new(bytes))?;
        let mut found = Vec::new();
        for i in 0..z.len() {
            let mut f = z.by_index(i)?;
            let mut v = Vec::new();
            f.read_to_end(&mut v)?;
            found.push((f.name()?.into_owned(), v));
        }
        if found != parts {
            eprintln!("{DIR}/template.pptx is out of date (run `cargo xtask pptx-template`)");
            return Ok(false);
        }
        eprintln!("pptx-template --check: {} parts up to date", parts.len());
        return Ok(true);
    }
    let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated).last_modified_time(DateTime::default());
    let mut w = ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (n, b) in &parts {
        w.start_file(n.as_str(), opts)?;
        w.write_all(b)?;
    }
    std::fs::write(&target, w.finish()?.into_inner())?;
    eprintln!("wrote {DIR}/template.pptx ({} parts)", parts.len());
    Ok(true)
}
