//! The repository's fixtures as structure-aware fuzzing seeds.
//!
//! Format-crate targets do not mutate zip bytes (a flipped bit in a
//! deflate stream only exercises the CRC check). Their input is
//!
//! ```text
//! [fixture: u8] [part: u8] [ops_len: u8] [ops: ops_len bytes] [payload…]
//! ```
//!
//! The fixture's parts are rebuilt into a fresh (stored) zip with part
//! `part` replaced by `payload` (`part == 0xFF`: nothing replaced), and
//! `ops` drive the reads and edits. Seeds carry each part's original bytes
//! as payload, so the fuzzer starts from real XML.

use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::input::Input;

/// A fixture with its parts decompressed, in zip order.
#[derive(Debug)]
pub struct Fixture {
    /// Path relative to the fixture directory.
    pub name: String,
    /// `(part name, content)`.
    pub parts: Vec<(String, Vec<u8>)>,
}

/// `$OOXML_FIXTURES`, else `reference/fixtures` of this repository.
pub fn fixtures_dir() -> PathBuf {
    std::env::var_os("OOXML_FIXTURES")
        .map_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reference/fixtures"), PathBuf::from)
}

fn collect(dir: &Path, exts: &[&str], out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect(&p, exts, out);
        } else if p.extension().and_then(|x| x.to_str()).is_some_and(|x| exts.contains(&x)) {
            out.push(p);
        }
    }
}

/// Every file with one of `exts` under `fixtures_dir()/sub`, sorted.
pub fn fixture_files(sub: &str, exts: &[&str]) -> Vec<PathBuf> {
    let mut v = Vec::new();
    collect(&fixtures_dir().join(sub), exts, &mut v);
    v.sort();
    v
}

/// Decompresses a zip into its parts (`None` if it is not a readable zip).
pub fn unzip(bytes: &[u8]) -> Option<Vec<(String, Vec<u8>)>> {
    let mut a = ZipArchive::new(Cursor::new(bytes)).ok()?;
    let mut parts = Vec::with_capacity(a.len());
    for i in 0..a.len() {
        let mut f = a.by_index(i).ok()?;
        let name = f.name().ok()?.into_owned();
        let mut buf = Vec::new();
        f.read_to_end(&mut buf).ok()?;
        parts.push((name, buf));
    }
    Some(parts)
}

fn load(sub: &str, exts: &[&str]) -> Vec<Fixture> {
    let base = fixtures_dir().join(sub);
    fixture_files(sub, exts)
        .into_iter()
        .filter_map(|p| {
            let parts = unzip(&std::fs::read(&p).ok()?)?;
            let name = p.strip_prefix(&base).unwrap_or(&p).to_string_lossy().into_owned();
            Some(Fixture { name, parts })
        })
        .collect()
}

/// The xlsx fixtures (`reference/fixtures/xlsx/**`).
pub static XLSX: LazyLock<Vec<Fixture>> = LazyLock::new(|| load("xlsx", &["xlsx", "xlsm"]));
/// The docx fixtures (`reference/fixtures/docx/**`).
pub static DOCX: LazyLock<Vec<Fixture>> = LazyLock::new(|| load("docx", &["docx", "docm", "dotx", "dotm"]));
/// The pptx fixtures (`reference/fixtures/pptx/**`).
pub static PPTX: LazyLock<Vec<Fixture>> = LazyLock::new(|| load("pptx", &["pptx", "pptm", "ppsx", "potx"]));

/// Writes parts into a new zip, every entry stored.
pub fn build_zip<'p>(parts: impl IntoIterator<Item = (&'p str, &'p [u8])>) -> Vec<u8> {
    let mut zw = ZipWriter::new(Cursor::new(Vec::new()));
    let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    for (name, bytes) in parts {
        // Duplicate names (a part replaced by a payload never renames, so
        // only from the fixture itself) are skipped by the writer's error.
        if zw.start_file(name, opts).is_ok() {
            zw.write_all(bytes).expect("in-memory write");
        }
    }
    zw.finish().expect("in-memory zip").into_inner()
}

/// Decodes a format-target input: the package bytes and the ops bytes.
pub fn decode<'a>(fixtures: &[Fixture], data: &'a [u8]) -> Option<(Vec<u8>, Input<'a>)> {
    if fixtures.is_empty() {
        return None;
    }
    let mut inp = Input::new(data);
    let fx = &fixtures[usize::from(inp.u8()) % fixtures.len()];
    let part = inp.u8();
    let ops_len = usize::from(inp.u8());
    let ops = Input::new(inp.take(ops_len));
    let payload = inp.rest();
    if crate::known::f1_double_bom(payload) {
        return None;
    }
    let replace = (part != 0xFF).then(|| usize::from(part) % fx.parts.len().max(1));
    let zip = build_zip(
        fx.parts
            .iter()
            .enumerate()
            .map(|(i, (n, b))| (n.as_str(), if Some(i) == replace { payload } else { b.as_slice() })),
    );
    Some((zip, ops))
}

/// Writes the seed corpus of a format target: one input per (fixture, XML
/// part) with the original part as payload, and one per fixture with no
/// replacement. Returns the number of files written.
pub fn write_seeds(fixtures: &[Fixture], out: &Path) -> std::io::Result<usize> {
    std::fs::create_dir_all(out)?;
    let mut n = 0;
    let ops: &[u8] = &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
    for (fi, fx) in fixtures.iter().enumerate().take(255) {
        let fi = fi as u8;
        let mut whole = vec![fi, 0xFF, ops.len() as u8];
        whole.extend_from_slice(ops);
        std::fs::write(out.join(format!("f{fi:03}-all")), whole)?;
        n += 1;
        for (pi, (name, bytes)) in fx.parts.iter().enumerate().take(255) {
            if !(name.ends_with(".xml") || name.ends_with(".rels")) || bytes.len() > 256 * 1024 {
                continue;
            }
            let mut seed = vec![fi, pi as u8, ops.len() as u8];
            seed.extend_from_slice(ops);
            seed.extend_from_slice(bytes);
            std::fs::write(out.join(format!("f{fi:03}-p{pi:03}")), seed)?;
            n += 1;
        }
    }
    Ok(n)
}
