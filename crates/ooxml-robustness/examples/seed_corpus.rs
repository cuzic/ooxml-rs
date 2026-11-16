//! Writes the cargo-fuzz seed corpora from `reference/fixtures/**` (the
//! corpora are generated at run time and never committed).
//!
//! ```text
//! cargo run -p ooxml-robustness --example seed_corpus -- fuzz/corpus
//! ```

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use ooxml_robustness::fixtures::{PPTX, XLSX, fixture_files, fixtures_dir, unzip, write_seeds};
use ooxml_robustness::mutate::Rng;

fn main() -> std::io::Result<()> {
    let out = PathBuf::from(std::env::args_os().nth(1).unwrap_or_else(|| "fuzz/corpus".into()));
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
        for (name, part) in unzip(&bytes).unwrap_or_default() {
            if (name.ends_with(".xml") || name.ends_with(".rels")) && part.len() <= 64 * 1024 {
                xml_parts.insert(part);
            }
        }
    }
    println!("package_roundtrip: {} packages", packages.len());

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
    Ok(())
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}
