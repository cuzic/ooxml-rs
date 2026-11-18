#![no_main]

//! XmlDocument::parse -> force_rerender -> reparse is structurally equal (T1b).

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    ooxml_robustness::check_t1b(data);
});
