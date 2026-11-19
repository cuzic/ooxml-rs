#![no_main]

//! ooxml-docx Phase1 cross-run find/replace (find, replace, replace_at, replace_matches) over every story.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    ooxml_robustness::check_docx_replace_input(data);
});
