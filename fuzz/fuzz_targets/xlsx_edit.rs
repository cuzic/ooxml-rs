#![no_main]

//! ooxml-xlsx open/read/write on fixture-derived packages (one part replaced by the fuzz payload).

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    ooxml_robustness::check_xlsx_input(data);
});
