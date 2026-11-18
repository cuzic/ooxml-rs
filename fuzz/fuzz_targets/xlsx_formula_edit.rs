#![no_main]

//! ooxml-xlsx Phase1 formula-aware edits (set_number/set_string/set_formula/remove_formula on any worksheet) with an independent dependency model.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    ooxml_robustness::check_xlsx_formula_input(data);
});
