#![no_main]

//! Lens edit sequences on a small schema: Err => unchanged; output reparses.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    ooxml_robustness::check_lens_ops(data);
});
