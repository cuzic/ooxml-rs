//! `Package::from_bytes` → read → save → reopen.

use ooxml_core::{Package, XmlDocument};

use crate::zipcmp::assert_l1_except;

/// Opens arbitrary bytes as a package and checks, if it opens:
///
/// - every part can be read (or fails with an `Err`), and parts that parse
///   can be serialized;
/// - `to_bytes` with zero edits is Layer-1 identical to the input
///   (parsing parts for inspection does not change the output);
/// - the output opens again and saves Layer-1 identical to itself;
/// - after `force_rerender` of one parsed part (`pick` chooses which), every
///   other entry is still Layer-1 identical and the re-rendered part parses
///   to a structurally equal tree.
pub fn check_package(data: &[u8], pick: u8) {
    let Ok(mut pkg) = Package::from_bytes(data.to_vec()) else { return };
    let names: Vec<String> = pkg.part_names().map(str::to_owned).collect();
    let _ = pkg.main_document_part();
    let _ = pkg.content_types().defaults().len();
    let mut parsed = Vec::new();
    for name in &names {
        let _ = pkg.content_type(name);
        let _ = pkg.relationships(name);
        if pkg.read_part(name).is_err() {
            continue;
        }
        if let Ok(doc) = pkg.xml_part(name) {
            let _ = doc.serialize();
            parsed.push(name.clone());
        }
    }
    let Ok(out) = pkg.to_bytes() else { return };
    assert_l1_except(data, &out, &[], "zero-edit save");
    let reopened = Package::from_bytes(out.clone()).unwrap_or_else(|e| panic!("zero-edit output does not reopen: {e}"));
    let out2 = reopened.to_bytes().unwrap_or_else(|e| panic!("reopened package does not save: {e}"));
    assert_l1_except(&out, &out2, &[], "second zero-edit save");

    if parsed.is_empty() {
        return;
    }
    let target = &parsed[usize::from(pick) % parsed.len()];
    let original = pkg.xml_part(target).expect("parsed above").clone();
    pkg.xml_part_mut(target).expect("parsed above").force_rerender();
    let Ok(edited) = pkg.to_bytes() else { return };
    assert_l1_except(data, &edited, &[target], "save after force_rerender");
    let reopened = Package::from_bytes(edited).unwrap_or_else(|e| panic!("edited output does not reopen: {e}"));
    let bytes = reopened.read_part(target).unwrap_or_else(|e| panic!("re-rendered part {target:?} unreadable: {e}"));
    let doc = XmlDocument::parse_with(&bytes, &pkg.parse_options())
        .unwrap_or_else(|e| panic!("re-rendered part {target:?} does not parse: {e}"));
    assert!(doc.root().structurally_eq(original.root()), "re-rendered part {target:?} is not structurally equal");
}
