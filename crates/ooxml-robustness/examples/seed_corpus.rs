//! Writes the cargo-fuzz seed corpora from `reference/fixtures/**` (the
//! corpora are generated at run time and never committed).
//!
//! ```text
//! cargo run -p ooxml-robustness --example seed_corpus -- fuzz/corpus [max-per-target]
//! ```
//!
//! `max-per-target` keeps every n-th seed so that at most that many remain
//! per target (libFuzzer runs the whole initial corpus before
//! `-max_total_time` starts counting; the CI smoke uses a small value).
//!
//! Besides the fixtures, the package corpus gets synthetic variants the
//! fixtures lack: every entry ZIP64, data descriptors (streamed writer),
//! data prepended to the zip (self-extractor layout), a damaged CFB
//! signature, and Strict namespaces in the parts only (Transitional rels).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use ooxml_robustness::fixtures::{DOCX, PPTX, XLSX, fixture_files, fixtures_dir, unzip, write_seeds};
use ooxml_robustness::mutate::Rng;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

type Parts = Vec<(String, Vec<u8>)>;

fn rezip(parts: &Parts, zip64: bool) -> Vec<u8> {
    let mut zw = ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (n, b) in parts {
        let o = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated).large_file(zip64);
        zw.start_file(n.as_str(), o).unwrap();
        std::io::Write::write_all(&mut zw, b).unwrap();
    }
    zw.finish().unwrap().into_inner()
}

/// Streamed: every entry gets a data descriptor (flag bit 3).
fn rezip_streamed(parts: &Parts) -> Vec<u8> {
    let mut zw = ZipWriter::new_stream(Vec::new());
    for (n, b) in parts {
        zw.start_file(n.as_str(), SimpleFileOptions::default().compression_method(CompressionMethod::Deflated)).unwrap();
        std::io::Write::write_all(&mut zw, b).unwrap();
    }
    zw.finish().unwrap().into_inner()
}

/// The synthetic package variants of one fixture.
fn variants(bytes: &[u8], parts: &Parts) -> Vec<(&'static str, Vec<u8>)> {
    let mut v = vec![("zip64", rezip(parts, true)), ("descriptor", rezip_streamed(parts))];
    let mut sfx = b"MZ\x90\x00self-extractor stub ".repeat(4);
    sfx.extend_from_slice(bytes);
    v.push(("prepended", sfx));
    let mut cfb = vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE0]; // last byte damaged
    cfb.extend_from_slice(bytes);
    v.push(("cfb-damaged", cfb));
    let strict: Parts = parts
        .iter()
        .map(|(n, b)| {
            let b = if n.ends_with(".rels") || n == "[Content_Types].xml" {
                b.clone()
            } else {
                String::from_utf8_lossy(b)
                    .replace("http://schemas.openxmlformats.org/spreadsheetml/2006/main", "http://purl.oclc.org/ooxml/spreadsheetml/main")
                    .replace("http://schemas.openxmlformats.org/wordprocessingml/2006/main", "http://purl.oclc.org/ooxml/wordprocessingml/main")
                    .replace("http://schemas.openxmlformats.org/presentationml/2006/main", "http://purl.oclc.org/ooxml/presentationml/main")
                    .replace("http://schemas.openxmlformats.org/drawingml/2006/main", "http://purl.oclc.org/ooxml/drawingml/main")
                    .into_bytes()
            };
            (n.clone(), b)
        })
        .collect();
    v.push(("strict-parts", rezip(&strict, false)));
    v
}

/// Keeps every n-th file of `dir` so that at most `max` remain.
fn thin(dir: &Path, max: usize) -> std::io::Result<usize> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)?.flatten().map(|e| e.path()).collect();
    files.sort();
    let step = files.len().div_ceil(max.max(1)).max(1);
    for (i, f) in files.iter().enumerate() {
        if i % step != 0 {
            std::fs::remove_file(f)?;
        }
    }
    Ok(files.len().div_ceil(step))
}

fn main() -> std::io::Result<()> {
    let out = PathBuf::from(std::env::args_os().nth(1).unwrap_or_else(|| "fuzz/corpus".into()));
    let max: Option<usize> = std::env::args().nth(2).map(|v| v.parse().expect("max-per-target is a number"));
    let dir = |t: &str| -> std::io::Result<PathBuf> {
        let d = out.join(t);
        std::fs::create_dir_all(&d)?;
        Ok(d)
    };

    // Whole packages: every fixture of every format, plus the round-trip corpus.
    let mut packages = fixture_files("", &["xlsx", "xlsm", "pptx", "pptm", "ppsx", "potx", "docx", "docm"]);
    let rt = fixtures_dir().join("../round_trip_corpus");
    if let Ok(rd) = std::fs::read_dir(&rt) {
        packages.extend(rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x != "md")));
    }
    let pkg_dir = dir("package_roundtrip")?;
    let mut xml_parts: BTreeSet<Vec<u8>> = BTreeSet::new();
    for (i, p) in packages.iter().enumerate() {
        let bytes = std::fs::read(p)?;
        std::fs::write(pkg_dir.join(format!("{i:03}-{}", file_name(p))), &bytes)?;
        let parts = unzip(&bytes).unwrap_or_default();
        // Synthetic variants of the small fixtures only (fast seeds).
        if !parts.is_empty() && bytes.len() <= 64 * 1024 {
            for (tag, v) in variants(&bytes, &parts) {
                std::fs::write(pkg_dir.join(format!("{i:03}-{tag}-{}", file_name(p))), v)?;
            }
        }
        for (name, part) in parts {
            if (name.ends_with(".xml") || name.ends_with(".rels")) && part.len() <= 64 * 1024 {
                xml_parts.insert(part);
            }
        }
    }
    println!("package_roundtrip: {} packages + {} synthetic variants", packages.len(), std::fs::read_dir(&pkg_dir)?.count() - packages.len());

    for t in ["xml_t1", "xml_t1b"] {
        let d = dir(t)?;
        for (i, part) in xml_parts.iter().enumerate() {
            std::fs::write(d.join(format!("{i:05}")), part)?;
        }
        println!("{t}: {} XML parts", xml_parts.len());
    }

    // Lens: every base document with a few random op sequences.
    let d = dir("lens_ops")?;
    let mut rng = Rng::new(1);
    for doc in 0..ooxml_robustness::lens::BASE_DOCS.len() as u8 {
        for k in 0..8u8 {
            let mut seed = vec![k, doc];
            seed.extend(rng.bytes(64));
            std::fs::write(d.join(format!("base{doc}-{k}")), seed)?;
        }
    }

    println!("xlsx_edit: {} inputs", write_seeds(&XLSX, &dir("xlsx_edit")?)?);
    println!("pptx_edit: {} inputs", write_seeds(&PPTX, &dir("pptx_edit")?)?);
    println!("docx_edit: {} inputs", write_seeds(&DOCX, &dir("docx_edit")?)?);
    if let Some(max) = max {
        for t in ["package_roundtrip", "xml_t1", "xml_t1b", "lens_ops", "xlsx_edit", "pptx_edit", "docx_edit"] {
            println!("{t}: thinned to {}", thin(&out.join(t), max)?);
        }
    }
    Ok(())
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}
