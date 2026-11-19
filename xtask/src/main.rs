//! `cargo xtask codegen [--check]` — ADR-0002's schema code generator.
//!
//! Inputs (all vendored, read in sorted order):
//! `reference/openxml-sdk-data/`, `reference/ecma-376-transitional-xsd/`,
//! `codegen/allowlist/*.toml`, `codegen/overrides.toml`, and
//! `reference/ecma-376-drawingml-geometries/`.
//!
//! Outputs: `crates/ooxml-schema/src/generated/*.rs`,
//! `crates/ooxml-core/src/generated/parts.rs`, the PPTX preset adjustment
//! table, and `codegen/report.txt`.
//! `--check` regenerates in memory, verifies the vendored checksums, and
//! fails on any difference.
//!
//! `cargo xtask manual-check <outdir>` runs the three formats' manual-check
//! examples and writes one bundle for the owner (see `manual_check.rs`).

mod config;
mod emit;
mod lower;
mod manual_check;
mod model;
mod pptx_template;
mod preset_geometry;
mod sdk;
mod stats;
mod xsd;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use sha2::{Digest, Sha256};

pub type Res<T> = Result<T, Box<dyn std::error::Error>>;

pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("workspace root").to_path_buf()
}

pub const SDK_DIR: &str = "reference/openxml-sdk-data";
pub const XSD_DIR: &str = "reference/ecma-376-transitional-xsd";
const SCHEMA_GEN_DIR: &str = "crates/ooxml-schema/src/generated";
const PARTS_FILE: &str = "crates/ooxml-core/src/generated/parts.rs";
const REPORT_FILE: &str = "codegen/report.txt";

/// Generates every output file: relative path → content.
pub fn generate(root: &Path) -> Res<BTreeMap<String, String>> {
    let sdk = sdk::Sdk::load(&root.join(SDK_DIR))?;
    let xsd = xsd::Xsd::load(&root.join(XSD_DIR))?;
    let overrides = config::load_overrides(&root.join("codegen/overrides.toml"))?;
    let allow = config::load_allowlists(&root.join("codegen/allowlist"))?;
    let mut diags = model::Diagnostics::default();
    let model = model::Model::new(&sdk, &xsd, &overrides, &mut diags);
    let ctx = emit::Ctx::new(&model, &allow.entries, &allow.unique_attrs, &mut diags)?;
    let mut out = BTreeMap::new();
    for g in config::GROUPS {
        out.insert(format!("{SCHEMA_GEN_DIR}/{g}.rs"), ctx.emit_group(g, &mut diags)?);
    }
    out.insert(format!("{SCHEMA_GEN_DIR}/mod.rs"), ctx.emit_mod()?);
    out.insert(PARTS_FILE.into(), emit::emit_parts(&model)?);
    // whole-schema diagnostics (default join, unresolved bases, ECMA order)
    for name in model.index.keys() {
        model.resolve(name, &mut diags);
    }
    let st = stats::compute(&model);
    out.insert(REPORT_FILE.into(), stats::report(&st, &diags, &ctx));
    out.insert(preset_geometry::OUTPUT.into(), preset_geometry::generate(root, &xsd)?);
    Ok(out)
}

/// Parses a PROVENANCE.md table: `| \`file\` | \`sha256\` |`.
fn provenance(dir: &Path) -> Res<BTreeMap<String, String>> {
    let text = std::fs::read_to_string(dir.join("PROVENANCE.md"))?;
    let mut m = BTreeMap::new();
    for line in text.lines() {
        let cells: Vec<&str> = line.split('|').map(str::trim).filter(|c| !c.is_empty()).collect();
        if let [f, h] = cells.as_slice()
            && let (Some(f), Some(h)) = (f.strip_circumfix('`', '`'), h.strip_circumfix('`', '`'))
            && h.len() == 64
        {
            m.insert(f.to_owned(), h.to_owned());
        }
    }
    Ok(m)
}

fn walk(dir: &Path, base: &Path, out: &mut Vec<String>) -> Res<()> {
    for e in std::fs::read_dir(dir)? {
        let p = e?.path();
        if p.is_dir() {
            walk(&p, base, out)?;
        } else {
            out.push(p.strip_prefix(base)?.to_string_lossy().replace('\\', "/"));
        }
    }
    Ok(())
}

/// Verifies vendored inputs against their PROVENANCE.md checksums.
fn check_inputs(root: &Path) -> Res<Vec<String>> {
    let mut problems = Vec::new();
    let inputs: [(&str, &[&str]); 3] = [
        (SDK_DIR, &["PROVENANCE.md"]),
        (XSD_DIR, &["PROVENANCE.md", "COPYRIGHT-NOTICE.txt"]),
        (preset_geometry::DIR, &["PROVENANCE.md", "COPYRIGHT-NOTICE.txt", "supplement.toml"]),
    ];
    for (d, own) in inputs {
        let dir = root.join(d);
        let table = provenance(&dir)?;
        let mut files = Vec::new();
        walk(&dir, &dir, &mut files)?;
        files.sort();
        for f in &files {
            if own.contains(&f.as_str()) {
                continue;
            }
            match table.get(f) {
                None => problems.push(format!("{d}/{f}: not listed in PROVENANCE.md")),
                Some(h) => {
                    let got: String = Sha256::digest(std::fs::read(dir.join(f))?).iter().map(|b| format!("{b:02x}")).collect();
                    if &got != h {
                        problems.push(format!("{d}/{f}: SHA-256 {got} != PROVENANCE.md {h}"));
                    }
                }
            }
        }
        for f in table.keys() {
            if !files.contains(f) {
                problems.push(format!("{d}/{f}: listed in PROVENANCE.md but missing"));
            }
        }
    }
    Ok(problems)
}

fn run() -> Res<bool> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let root = root();
    match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["codegen"] => {
            let problems = check_inputs(&root)?;
            if !problems.is_empty() {
                for p in &problems {
                    eprintln!("input check: {p}");
                }
                return Ok(false);
            }
            let out = generate(&root)?;
            for (path, content) in &out {
                let p = root.join(path);
                if std::fs::read_to_string(&p).ok().as_deref() != Some(content.as_str()) {
                    std::fs::create_dir_all(p.parent().unwrap())?;
                    std::fs::write(&p, content)?;
                    eprintln!("wrote {path}");
                }
            }
            remove_stale(&root, &out, true)?;
            Ok(true)
        }
        ["codegen", "--check"] => {
            let mut ok = true;
            for p in check_inputs(&root)? {
                eprintln!("input check: {p}");
                ok = false;
            }
            let out = generate(&root)?;
            for (path, content) in &out {
                if std::fs::read_to_string(root.join(path)).ok().as_deref() != Some(content.as_str()) {
                    eprintln!("out of date: {path} (run `cargo xtask codegen`)");
                    ok = false;
                }
            }
            if !remove_stale(&root, &out, false)? {
                ok = false;
            }
            if ok {
                eprintln!("codegen --check: inputs verified, {} generated files up to date", out.len());
            }
            Ok(ok)
        }
        ["manual-check", out] => manual_check::run(&root, Path::new(out)),
        ["pptx-template"] => pptx_template::run(&root, false),
        ["pptx-template", "--check"] => pptx_template::run(&root, true),
        ["stats"] => {
            let out = generate(&root)?;
            print!("{}", out[REPORT_FILE]);
            Ok(true)
        }
        _ => {
            eprintln!("usage: cargo xtask codegen [--check] | cargo xtask stats | cargo xtask manual-check <outdir> | cargo xtask pptx-template [--check]");
            Ok(false)
        }
    }
}

/// Generated files in `SCHEMA_GEN_DIR` that the generator no longer writes.
fn remove_stale(root: &Path, out: &BTreeMap<String, String>, delete: bool) -> Res<bool> {
    let mut clean = true;
    let dir = root.join(SCHEMA_GEN_DIR);
    if dir.is_dir() {
        let mut names = Vec::new();
        walk(&dir, &dir, &mut names)?;
        for n in names {
            let rel = format!("{SCHEMA_GEN_DIR}/{n}");
            if !out.contains_key(&rel) {
                if delete {
                    std::fs::remove_file(root.join(&rel))?;
                    eprintln!("removed stale {rel}");
                } else {
                    eprintln!("stale generated file: {rel}");
                    clean = false;
                }
            }
        }
    }
    Ok(clean)
}

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
