#![no_main]

//! XmlDocument::parse -> serialize == input (T1) for every accepted input.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    ooxml_robustness::check_t1(data);
});
