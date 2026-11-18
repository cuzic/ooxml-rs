#![no_main]

//! ooxml-pptx open/read/text edits on fixture-derived packages (one part replaced by the fuzz payload).

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    ooxml_robustness::check_pptx_input(data);
});
