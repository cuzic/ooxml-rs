#![no_main]

//! Typed lens and schemaless leaf edits: Err => unchanged; output reparses.

use libfuzzer_sys::fuzz_target;
use ooxml_core::XmlDocument;
use ooxml_robustness::lens::P;

fuzz_target!(|data: &[u8]| {
    ooxml_robustness::check_lens_ops(data);
    ooxml_robustness::xml::check_leaf_text(data);

    let [start, end, ..] = data else { return };
    let (start, end) = (usize::from(*start) % 10, usize::from(*end) % 10);
    let mut bulk = XmlDocument::parse(b"<p xmlns='urn:t'><head/> <!-- comment --><a/><b/><item k='0'/></p>").unwrap();
    let mut repeated = bulk.clone();
    let count = bulk.root().children().len();
    let removed = bulk.lens_mut::<P>().unwrap().remove_range(start..end);
    assert_eq!(removed, start <= end && end <= count);
    if removed {
        let mut lens = repeated.lens_mut::<P>().unwrap();
        for index in (start..end).rev() {
            assert!(lens.remove_at(index));
        }
    }
    assert_eq!(bulk.serialize(), repeated.serialize());
});
