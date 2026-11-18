//! Stable-Rust fuzz smoke: every check of the cargo-fuzz targets, run on
//! deterministic mutations of the fixtures (no libFuzzer, no nightly).
//!
//! Bounded: `OOXML_SMOKE_ITERS` mutations per seed (default 1, a few
//! seconds in a debug build). The `fuzz-smoke` workflow runs it with a
//! larger value in release mode. Every finding is fixed (no filters remain),
//! so a failure here is a new problem: replay the
//! printed input with `cargo run -p ooxml-robustness --example replay`.

use ooxml_robustness::fixtures::{DOCX, Fixture, PPTX, XLSX};
use ooxml_robustness::mutate::{Rng, mutate};

fn iters() -> usize {
    std::env::var("OOXML_SMOKE_ITERS").ok().and_then(|v| v.parse().ok()).unwrap_or(1)
}

/// Runs `check` and, on a panic, writes the input to the target dir and
/// re-raises with its path.
fn guarded(name: &str, input: &[u8], check: impl FnOnce(&[u8]) + std::panic::UnwindSafe) {
    if let Err(e) = std::panic::catch_unwind(|| check(input)) {
        let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("smoke-failures");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(format!("{name}-{:016x}", fxhash(input)));
        let _ = std::fs::write(&path, input);
        eprintln!("smoke failure in {name}; input saved to {}", path.display());
        std::panic::resume_unwind(e);
    }
}

fn fxhash(b: &[u8]) -> u64 {
    b.iter().fold(0xcbf2_9ce4_8422_2325, |h, &x| (h ^ u64::from(x)).wrapping_mul(0x100_0000_01b3))
}

fn xml_parts(fx: &[Fixture]) -> Vec<&[u8]> {
    fx.iter()
        .flat_map(|f| f.parts.iter())
        .filter(|(n, b)| (n.ends_with(".xml") || n.ends_with(".rels")) && b.len() <= 32 * 1024)
        .map(|(_, b)| b.as_slice())
        .collect()
}

#[test]
fn xml_t1_and_t1b() {
    let mut rng = Rng::new(0x7031);
    let mut parts = xml_parts(&XLSX);
    parts.extend(xml_parts(&PPTX));
    assert!(parts.len() > 100, "fixtures not found");
    for p in parts {
        ooxml_robustness::check_t1(p);
        ooxml_robustness::check_t1b(p);
        for _ in 0..iters() * 4 {
            let m = mutate(&mut rng, p);
            guarded("xml_t1", &m, ooxml_robustness::check_t1);
            guarded("xml_t1b", &m, ooxml_robustness::check_t1b);
        }
    }
}

#[test]
fn lens_ops() {
    let mut rng = Rng::new(0x1e75);
    for _ in 0..iters() * 2000 {
        let n = rng.below(96);
        let input = rng.bytes(n);
        guarded("lens_ops", &input, ooxml_robustness::check_lens_ops);
    }
}

#[test]
fn package_roundtrip() {
    let mut rng = Rng::new(0x9ac);
    let files = ooxml_robustness::fixtures::fixture_files("", &["xlsx", "xlsm", "pptx", "docx"]);
    assert!(files.len() > 50, "fixtures not found");
    for f in files {
        let bytes = std::fs::read(&f).unwrap();
        ooxml_robustness::check_package(&bytes, 0);
        for i in 0..iters() {
            let m = mutate(&mut rng, &bytes);
            guarded("package_roundtrip", &m, |d| ooxml_robustness::check_package(d, i as u8));
        }
    }
}

/// Fixture-based inputs: for each fixture, a few parts replaced by a
/// mutation of themselves, with random edit ops.
fn format_inputs(fx: &[Fixture], rng: &mut Rng, per_fixture: usize) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    for (fi, f) in fx.iter().enumerate().take(255) {
        out.push(vec![fi as u8, 0xFF, 32].into_iter().chain(rng.bytes(32)).collect());
        let xml: Vec<usize> =
            (0..f.parts.len().min(255)).filter(|&i| f.parts[i].0.ends_with(".xml") || f.parts[i].0.ends_with(".rels")).collect();
        for _ in 0..per_fixture {
            if xml.is_empty() {
                break;
            }
            let pi = xml[rng.below(xml.len())];
            let mut input = vec![fi as u8, pi as u8, 24];
            input.extend(rng.bytes(24));
            input.extend(mutate(rng, &f.parts[pi].1));
            out.push(input);
        }
    }
    out
}

#[test]
fn xlsx_edit() {
    let mut rng = Rng::new(0x715c);
    assert!(XLSX.len() > 20, "fixtures not found");
    for input in format_inputs(&XLSX, &mut rng, iters() * 3) {
        guarded("xlsx_edit", &input, ooxml_robustness::check_xlsx_input);
    }
}

#[test]
fn pptx_edit() {
    let mut rng = Rng::new(0x9977);
    assert!(PPTX.len() > 20, "fixtures not found");
    for input in format_inputs(&PPTX, &mut rng, iters()) {
        guarded("pptx_edit", &input, ooxml_robustness::check_pptx_input);
    }
}

#[test]
fn docx_edit() {
    let mut rng = Rng::new(0xd0c5);
    assert!(DOCX.len() > 20, "fixtures not found");
    for input in ooxml_robustness::docx::targeted_inputs() {
        guarded("docx_edit", &input, ooxml_robustness::check_docx_input);
    }
    for input in format_inputs(&DOCX, &mut rng, iters() * 2) {
        guarded("docx_edit", &input, ooxml_robustness::check_docx_input);
    }
}

#[test]
fn xlsx_formula_edit() {
    let mut rng = Rng::new(0xf0a1);
    let fx = ooxml_robustness::xlsx_formula::formula_fixtures();
    assert!(fx.len() > 30, "fixtures not found");
    for input in format_inputs(fx, &mut rng, iters() * 3) {
        guarded("xlsx_formula_edit", &input, ooxml_robustness::check_xlsx_formula_input);
    }
    let [ok, deps, chains, refused] = ooxml_robustness::xlsx_formula::STATS.each_ref().map(|a| a.load(std::sync::atomic::Ordering::Relaxed));
    eprintln!("xlsx_formula_edit: {ok} successful edits, {deps} model dependents checked, {chains} calcChain decisions, {refused} refusals");
    assert!(ok > 0 && deps > 0, "the oracles never fired");
}

#[test]
fn docx_replace() {
    let mut rng = Rng::new(0x5e1a);
    let fx = ooxml_robustness::docx_replace::replace_fixtures();
    assert!(fx.len() > 25, "fixtures not found");
    for input in format_inputs(fx, &mut rng, iters() * 2) {
        guarded("docx_replace", &input, ooxml_robustness::check_docx_replace_input);
    }
    let [calls, applied, skipped, refused, stale, changed] =
        ooxml_robustness::docx_replace::STATS.each_ref().map(|a| a.load(std::sync::atomic::Ordering::Relaxed));
    eprintln!("docx_replace: {calls} calls with applied matches ({applied} matches, {changed} paragraphs changed), {skipped} skipped, {refused} refused calls, {stale} stale matches refused");
    assert!(applied > 0 && changed > 0, "the oracles never fired");
}
