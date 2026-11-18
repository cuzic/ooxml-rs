#![no_main]

//! Package::from_bytes -> read every part -> save -> reopen; Layer-1 invariants (see ooxml_robustness::check_package).

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // The part to force-re-render is picked from the length, so that seeds
    // stay plain zip files.
    ooxml_robustness::check_package(data, (data.len() % 251) as u8);
});
