#![no_main]

//! ooxml-docx open/gates/read/set_run_text/append_paragraph on fixture-derived packages (one part replaced by the fuzz payload).

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    ooxml_robustness::check_docx_input(data);
});
