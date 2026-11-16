//! Dev-only robustness harness for ooxml-rs (not published).
//!
//! The invariant checks here are shared by three users:
//!
//! - the cargo-fuzz targets in `fuzz/` (nightly, outside the workspace);
//! - `tests/fuzz_findings.rs`: one regression test per fuzz finding
//!   (`docs/testing/fuzz-findings-2026-11.md`);
//! - `tests/smoke.rs`: a bounded, deterministic stable-Rust property run
//!   over the fixtures (the CI-friendly fuzz smoke).
//!
//! Every `check_*` function takes arbitrary bytes and panics only when an
//! invariant of ADR-0001/0002 (or a crate README's contract) is broken; an
//! `Err` from the library under test is an accepted outcome unless the
//! contract says otherwise.

pub mod docx;
pub mod fixtures;
pub mod input;
pub mod known;
pub mod lens;
pub mod mutate;
pub mod package;
pub mod pptx;
pub mod xlsx;
pub mod xml;
pub mod zipcmp;

pub use docx::{check_docx, check_docx_input};
pub use lens::check_lens_ops;
pub use package::check_package;
pub use pptx::{check_pptx, check_pptx_input};
pub use xlsx::{check_xlsx, check_xlsx_input};
pub use xml::{check_t1, check_t1b};
