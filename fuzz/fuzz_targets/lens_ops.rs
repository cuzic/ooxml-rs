#![no_main]

//! Typed lens and schemaless leaf edits: Err => unchanged; output reparses.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    ooxml_robustness::check_lens_ops(data);
    ooxml_robustness::xml::check_leaf_text(data);
});
