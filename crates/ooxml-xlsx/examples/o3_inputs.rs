//! Writes the O3 inputs and outputs (ADR-0003 §7.1) for
//! `spikes/xlsx/o3_recalc.py`:
//!
//! ```text
//! cargo run -p ooxml-xlsx --example o3_inputs -- <dir>
//! python3 -I spikes/xlsx/o3_recalc.py <dir> reference/fixtures/xlsx/o3/expected.tsv
//! ```
//!
//! `<dir>/<stem>.input.xlsx` and `<dir>/<stem>.output.xlsx` per case.

#[path = "../tests/common/o2gen.rs"]
mod o2gen;
#[path = "../tests/common/o3cases.rs"]
mod o3cases;

fn main() {
    let dir = std::env::args().nth(1).expect("usage: o3_inputs <dir>");
    std::fs::create_dir_all(&dir).unwrap();
    let cases = o3cases::cases();
    for c in &cases {
        let stem = o3cases::file_stem(&c.id);
        std::fs::write(format!("{dir}/{stem}.input.xlsx"), &c.input).unwrap();
        std::fs::write(format!("{dir}/{stem}.output.xlsx"), &c.output).unwrap();
    }
    println!("{} cases written to {dir}", cases.len());
}
