//! O3 (ADR-0003 §7.1, R2-M2, R2-m6): LibreOffice's hard recalculation as an
//! independent engine. The expected values are produced offline (owner or
//! CI; LibreOffice is not available in the development sandbox):
//!
//! ```text
//! cargo run -p ooxml-xlsx --example o3_inputs -- /tmp/o3
//! python3 -I spikes/xlsx/o3_recalc.py /tmp/o3 reference/fixtures/xlsx/o3/expected.tsv
//! ```
//!
//! Check: every formula cell that **kept** its cached value in an output,
//! and whose cached value in the input agreed with LibreOffice's
//! recalculation of the input (baseline), holds LibreOffice's recalculated
//! value of the output. Workbooks with a volatile formula (a volatile
//! function in any formula text, or `ca="1"`) are skipped entirely, so that
//! no transitive dependent of a volatile cell is compared (review R3-m5;
//! no dependency code is used for this).

mod common;

use std::collections::{BTreeMap, HashMap};

use common::o3cases::{cases, file_stem, fnv1a};
use common::p1::{cell_spans, facts, sheet_parts};
use common::part;

const EXPECTED: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../reference/fixtures/xlsx/o3/expected.tsv");

/// file name -> (fnv, cell -> (t, value))
type Expected = HashMap<String, (String, HashMap<String, (String, String)>)>;

fn load() -> Option<Expected> {
    let text = std::fs::read_to_string(EXPECTED).ok()?;
    let mut out: Expected = HashMap::new();
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let f: Vec<&str> = line.splitn(5, '\t').collect();
        let [file, h, cell, t, v] = f[..] else { panic!("bad line {line:?}") };
        let e = out.entry(file.to_owned()).or_insert_with(|| (h.to_owned(), HashMap::new()));
        e.1.insert(cell.to_owned(), (t.to_owned(), v.replace("\\n", "\n")));
    }
    Some(out)
}

/// Formula cells with a cached value: sheet!cell -> (formula text, t, v).
fn kept(bytes: &[u8]) -> BTreeMap<String, (String, String, String)> {
    let mut out = BTreeMap::new();
    for (sheet, p) in sheet_parts(bytes) {
        for (at, span) in cell_spans(&part(bytes, &p)) {
            let f = facts(&span);
            let (Some(text), true) = (f.f.clone(), f.has_v) else { continue };
            let v = span.split("<v>").nth(1).and_then(|s| s.split("</v>").next()).unwrap_or_default().to_owned();
            out.insert(format!("{sheet}!{at}"), (text, f.t.unwrap_or_else(|| "n".into()), v));
        }
    }
    out
}

fn same(a: &(String, String), b: &(String, String)) -> bool {
    match (a.1.parse::<f64>(), b.1.parse::<f64>()) {
        (Ok(x), Ok(y)) if a.0 == "n" && b.0 == "n" => (x - y).abs() <= 1e-9 * x.abs().max(y.abs()).max(1.0),
        _ => a == b,
    }
}

const VOLATILE: &[&str] = &["NOW(", "TODAY(", "RAND(", "RANDBETWEEN(", "OFFSET(", "INDIRECT(", "CELL(", "INFO("];

/// Any formula text (shared masters included) with a volatile function, or
/// any `ca="1"`, in any sheet part.
fn has_volatile(bytes: &[u8]) -> bool {
    sheet_parts(bytes).values().any(|p| {
        let x = String::from_utf8(part(bytes, p)).unwrap();
        let up = x.to_ascii_uppercase();
        VOLATILE.iter().any(|f| up.contains(f)) || x.contains(r#"ca="1""#) || x.contains(r#"ca="true""#)
    })
}

#[test]
fn o3_case_list_is_deterministic() {
    let a: Vec<(String, u64, u64)> = cases().iter().map(|c| (c.id.clone(), fnv1a(&c.input), fnv1a(&c.output))).collect();
    let b: Vec<(String, u64, u64)> = cases().iter().map(|c| (c.id.clone(), fnv1a(&c.input), fnv1a(&c.output))).collect();
    assert_eq!(a, b);
    assert!(a.len() > 600, "{} cases", a.len());
}

#[test]
#[ignore = "needs reference/fixtures/xlsx/o3/expected.tsv: run the o3_inputs example, then spikes/xlsx/o3_recalc.py with LibreOffice (owner/CI job; LibreOffice is not in the dev sandbox)"]
fn o3_kept_values_match_libreoffice_recalculation() {
    let expected = load().expect(
        "reference/fixtures/xlsx/o3/expected.tsv is missing: run `cargo run -p ooxml-xlsx --example o3_inputs -- DIR` \
         and `python3 -I spikes/xlsx/o3_recalc.py DIR reference/fixtures/xlsx/o3/expected.tsv` with LibreOffice",
    );
    let (mut compared, mut skipped, mut failures) = (0, 0, Vec::new());
    for c in cases() {
        let stem = file_stem(&c.id);
        let (inp, out) = (format!("{stem}.input.xlsx"), format!("{stem}.output.xlsx"));
        let (Some((hi, lo_in)), Some((ho, lo_out))) = (expected.get(&inp), expected.get(&out)) else {
            panic!("{}: no expected data (regenerate)", c.id)
        };
        assert_eq!(*hi, format!("{:016x}", fnv1a(&c.input)), "{}: input changed since the data was generated", c.id);
        assert_eq!(*ho, format!("{:016x}", fnv1a(&c.output)), "{}: output changed since the data was generated", c.id);
        if has_volatile(&c.input) {
            skipped += 1;
            continue;
        }
        let before = kept(&c.input);
        for (cell, (text, t, v)) in kept(&c.output) {
            // Baseline: the input's cached value agreed with LibreOffice.
            let Some((ti, vi)) = before.get(&cell).map(|b| (b.1.clone(), b.2.clone())) else { continue };
            let Some(lo_i) = lo_in.get(&cell) else { continue };
            if !same(&(ti, vi), lo_i) {
                continue;
            }
            compared += 1;
            match lo_out.get(&cell) {
                Some(lo) if same(&(t.clone(), v.clone()), lo) => {}
                lo => failures.push(format!("{}: {cell} = {text} kept {t}:{v}, LibreOffice {lo:?}", c.id)),
            }
        }
    }
    assert!(failures.is_empty(), "{} stale values:\n{}", failures.len(), failures.join("\n"));
    assert!(compared > 100, "only {compared} cells compared ({skipped} cases skipped as volatile)");
}
