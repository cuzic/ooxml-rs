#![no_main]

//! Leaf-text preflight, edit and serialization agree for arbitrary paths and values.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    ooxml_robustness::xml::check_leaf_text(data);
});
