//! Element nesting depth limit (`ParseOptions::max_depth`, `Error::TooDeep`).
//!
//! Deep nesting is refused while parsing with a typed error, before the tree
//! grows: hostile input of a few kilobytes must not be able to exhaust memory
//! or overflow the stack in later recursive code (serialization, `Drop`,
//! format-crate walks).

use std::assert_matches;
use std::io::{Cursor, Write};
use std::time::{Duration, Instant};

use ooxml_core::{DEFAULT_MAX_DEPTH, Error, Package, ParseOptions, XmlDocument};
use zip::write::SimpleFileOptions;

/// `depth` nested `<a>` elements; the innermost one is self-closing when
/// `empty_leaf`, otherwise it has text.
fn nested(depth: usize, empty_leaf: bool) -> Vec<u8> {
    let mut s = String::with_capacity(depth * 8);
    for _ in 0..depth - 1 {
        s.push_str("<a>");
    }
    if empty_leaf {
        s.push_str("<a/>");
    } else {
        s.push_str("<a>x</a>");
    }
    for _ in 0..depth - 1 {
        s.push_str("</a>");
    }
    s.into_bytes()
}

#[test]
fn default_limit_is_inclusive() {
    for empty_leaf in [false, true] {
        let doc = XmlDocument::parse(&nested(DEFAULT_MAX_DEPTH, empty_leaf)).unwrap();
        // Unedited: still byte-identical (T1).
        assert_eq!(doc.serialize().as_bytes(), nested(DEFAULT_MAX_DEPTH, empty_leaf));
        let err = XmlDocument::parse(&nested(DEFAULT_MAX_DEPTH + 1, empty_leaf)).unwrap_err();
        // The offending start tag is the (limit+1)-th `<a`.
        assert_matches!(err, Error::TooDeep { limit: DEFAULT_MAX_DEPTH, position } if position == DEFAULT_MAX_DEPTH * 3);
    }
}

#[test]
fn deep_hostile_input_is_refused_quickly() {
    // 100,000 levels overflowed the stack (recursive `Drop`) before the
    // limit; 1,000,000 levels is ~7 MB of XML.
    for depth in [10_000, 100_000, 1_000_000] {
        let xml = nested(depth, false);
        let t = Instant::now();
        let err = XmlDocument::parse(&xml).unwrap_err();
        assert_matches!(err, Error::TooDeep { limit: DEFAULT_MAX_DEPTH, .. });
        assert!(t.elapsed() < Duration::from_secs(2), "depth {depth}: {:?}", t.elapsed());
    }
}

#[test]
fn limit_is_configurable() {
    let opts = ParseOptions::default().with_max_depth(4);
    XmlDocument::parse_with(&nested(4, true), &opts).unwrap();
    assert_matches!(XmlDocument::parse_with(&nested(5, true), &opts), Err(Error::TooDeep { limit: 4, position: 12 }));
    // Siblings do not add depth.
    XmlDocument::parse_with(b"<a><b/><b><c/></b><b><c><d/></c></b></a>", &opts).unwrap();

    let opts = ParseOptions::default().with_max_depth(4000);
    XmlDocument::parse_with(&nested(4000, false), &opts).unwrap();
    assert_matches!(XmlDocument::parse_with(&nested(4001, false), &opts), Err(Error::TooDeep { limit: 4000, .. }));
}

fn package_with(part: &[u8]) -> Vec<u8> {
    let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let o = SimpleFileOptions::default();
    zw.start_file("[Content_Types].xml", o).unwrap();
    zw.write_all(
        br#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/></Types>"#,
    )
    .unwrap();
    zw.start_file("deep.xml", o).unwrap();
    zw.write_all(part).unwrap();
    zw.finish().unwrap().into_inner()
}

#[test]
fn package_uses_its_parse_options() {
    let bytes = package_with(&nested(DEFAULT_MAX_DEPTH + 1, false));

    let mut pkg = Package::from_bytes(bytes.clone()).unwrap();
    assert_eq!(pkg.parse_options(), ParseOptions::default());
    assert_matches!(pkg.xml_part("deep.xml"), Err(Error::TooDeep { limit: DEFAULT_MAX_DEPTH, .. }));
    // The refused part stays raw and the package still saves byte-for-byte.
    assert_eq!(pkg.to_bytes().unwrap().len(), bytes.len());

    let mut pkg = Package::from_bytes_with(bytes, ParseOptions::default().with_max_depth(2048)).unwrap();
    pkg.xml_part("deep.xml").unwrap();
}
