//! Element nesting depth limit (`ParseOptions::max_depth`, `Error::TooDeep`).
//!
//! Deep nesting is refused while parsing with a typed error, before the tree
//! grows: hostile input of a few kilobytes must not be able to exhaust memory
//! or overflow the stack in later recursive code (serialization, `Drop`,
//! format-crate walks).

use std::assert_matches;
use std::io::{Cursor, Write};
use std::time::{Duration, Instant};

use ooxml_core::{
    AttrSpec, ChildOf, Content, DEFAULT_MAX_DEPTH, ElementSpec, Error, Package, ParseOptions, QName, Slot,
    XmlDocument,
};
use zip::write::SimpleFileOptions;

/// `<a>` whose only children are `<a>` (for lens descent and inserts).
enum A {}
impl ElementSpec for A {
    const QNAME: QName = QName::new("", "a");
    const SDK_NAME: &'static str = "a";
    const ATTRS: &'static [AttrSpec] = &[];
    const CONTENT: Content =
        Content::Slots(&[Slot { names: &[QName::new("", "a")], wildcard: None, max_each: None, max_total: None }]);
}
impl ChildOf<A> for A {}

/// Runs `f` on a thread with a `kib` KiB stack; a stack overflow aborts the
/// test process, which fails the test run.
fn on_stack(kib: usize, f: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new().stack_size(kib * 1024).spawn(f).unwrap().join().unwrap();
}

/// Half the 2 MiB default thread stack (`cargo test`, most runtimes).
const STACK_KIB: usize = 1024;

/// `depth` nested `<a>` elements; the innermost one is self-closing when
/// `empty_leaf`, otherwise it has text. Every `<a>` but the innermost has an
/// attribute and a namespace-free name, as in the review's probe.
fn nested(depth: usize, empty_leaf: bool) -> Vec<u8> {
    let mut s = String::with_capacity(depth * 8);
    for _ in 0..depth - 1 {
        s.push_str("<a k=\"v\">");
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
        assert_matches!(err, Error::TooDeep { limit: DEFAULT_MAX_DEPTH, position } if position == DEFAULT_MAX_DEPTH * 9);
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
    assert_matches!(XmlDocument::parse_with(&nested(5, true), &opts), Err(Error::TooDeep { limit: 4, position: 36 }));
    // Siblings do not add depth.
    XmlDocument::parse_with(b"<a><b/><b><c/></b><b><c><d/></c></b></a>", &opts).unwrap();

    // Raised above the default (see the warning on `max_depth`).
    let opts = ParseOptions::default().with_max_depth(1000);
    on_stack(8 * 1024, move || {
        XmlDocument::parse_with(&nested(1000, false), &opts).unwrap();
        assert_matches!(XmlDocument::parse_with(&nested(1001, false), &opts), Err(Error::TooDeep { limit: 1000, .. }));
    });
}

fn package_with(part: &[u8]) -> Vec<u8> {
    let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let o = SimpleFileOptions::default();
    zw.start_file("[Content_Types].xml", o).unwrap();
    zw.write_all(
        br#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/></Types>"#,
    )
    .unwrap();
    zw.start_file("_rels/.rels", o).unwrap();
    zw.write_all(br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="urn:t" Target="deep.xml"/></Relationships>"#).unwrap();
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

    let mut pkg = Package::from_bytes_with(bytes, ParseOptions::default().with_max_depth(DEFAULT_MAX_DEPTH + 1)).unwrap();
    pkg.xml_part("deep.xml").unwrap();
}

/// A tree operation under test.
type Op = fn(&[u8]);

/// Review PR #16 M1: every recursive operation on a tree at exactly the
/// default limit runs on a 1 MiB stack (half the default thread stack), in
/// whatever profile the tests are built with (debug by default, where frames
/// are largest).
#[test]
fn recursive_operations_at_the_limit_fit_a_small_stack() {
    let xml = nested(DEFAULT_MAX_DEPTH, false);
    let ops: Vec<(&str, Op)> = vec![
        ("parse + drop", |x| drop(XmlDocument::parse(x).unwrap())),
        ("clone + drop", |x| {
            let d = XmlDocument::parse(x).unwrap();
            let c = d.clone();
            drop(d);
            drop(c);
        }),
        ("Debug", |x| assert!(!format!("{:?}", XmlDocument::parse(x).unwrap()).is_empty())),
        ("pretty Debug", |x| assert!(!format!("{:#?}", XmlDocument::parse(x).unwrap()).is_empty())),
        ("serialize", |x| assert_eq!(XmlDocument::parse(x).unwrap().serialize().as_bytes(), x)),
        ("force_rerender + serialize + reparse", |x| {
            let mut d = XmlDocument::parse(x).unwrap();
            let before = d.root().clone();
            d.force_rerender();
            let again = XmlDocument::parse(d.serialize().as_bytes()).unwrap();
            assert!(before.structurally_eq(again.root()));
        }),
        ("structurally_eq", |x| {
            let (a, b) = (XmlDocument::parse(x).unwrap(), XmlDocument::parse(x).unwrap());
            assert!(a.root().structurally_eq(b.root()));
        }),
        ("lens descent + read", |x| {
            let d = XmlDocument::parse(x).unwrap();
            let mut l = d.lens::<A>().unwrap();
            for _ in 1..DEFAULT_MAX_DEPTH {
                l = l.child::<A>().unwrap();
            }
            assert_eq!(l.path().len(), DEFAULT_MAX_DEPTH - 1);
            assert_eq!(l.text(), "x");
        }),
        ("lens_mut descent + edit + dirty serialize", |x| {
            let mut d = XmlDocument::parse(x).unwrap();
            {
                let mut l = d.lens_mut::<A>().unwrap();
                for _ in 2..DEFAULT_MAX_DEPTH {
                    l = l.into_child_at::<A>(0).unwrap();
                }
                // Remove the innermost element: every ancestor turns dirty.
                assert!(l.remove_at(0));
            }
            let out = d.serialize();
            assert!(out.contains(r#"<a k="v"></a>"#));
            XmlDocument::parse(out.as_bytes()).unwrap();
        }),
        ("Package Debug", |x| {
            let mut pkg = Package::from_bytes(package_with(x)).unwrap();
            pkg.xml_part("deep.xml").unwrap();
            assert!(!format!("{pkg:?}").is_empty());
        }),
    ];
    for (name, op) in ops {
        let xml = xml.clone();
        let t = std::thread::Builder::new().name(name.into()).stack_size(STACK_KIB * 1024);
        t.spawn(move || op(&xml)).unwrap().join().unwrap_or_else(|_| panic!("{name}"));
    }
}

/// Review PR #16 m2: a lens insert cannot nest deeper than the limit the
/// document was parsed with, so edited output always parses again.
#[test]
fn lens_inserts_respect_the_limit() {
    on_stack(STACK_KIB, || {
        // Insert at depth == limit: refused, tree unchanged.
        let xml = nested(DEFAULT_MAX_DEPTH, true);
        let mut d = XmlDocument::parse(&xml).unwrap();
        {
            let mut l = d.lens_mut::<A>().unwrap();
            for _ in 1..DEFAULT_MAX_DEPTH {
                l = l.into_child_at::<A>(0).unwrap();
            }
            assert_matches!(l.insert::<A>(), Err(Error::TooDeep { limit: DEFAULT_MAX_DEPTH, .. }));
        }
        assert!(!d.is_dirty());
        // One level less: allowed, and the result parses with the default.
        let xml = nested(DEFAULT_MAX_DEPTH - 1, true);
        let mut d = XmlDocument::parse(&xml).unwrap();
        {
            let mut l = d.lens_mut::<A>().unwrap();
            for _ in 2..DEFAULT_MAX_DEPTH {
                l = l.into_child_at::<A>(0).unwrap();
            }
            l.insert::<A>().unwrap();
        }
        XmlDocument::parse(d.serialize().as_bytes()).unwrap();
    });
    // The limit is the document's own.
    let opts = ParseOptions::default().with_max_depth(3);
    let mut d = XmlDocument::parse_with(&nested(3, true), &opts).unwrap();
    let mut l = d.lens_mut::<A>().unwrap().into_child_at::<A>(0).unwrap().into_child_at::<A>(0).unwrap();
    assert_matches!(l.insert::<A>(), Err(Error::TooDeep { limit: 3, .. }));
    let mut d = XmlDocument::parse_with(&nested(2, true), &opts).unwrap();
    let mut l = d.lens_mut::<A>().unwrap().into_child_at::<A>(0).unwrap();
    let mut inner = l.insert::<A>().unwrap();
    // The new element is at the limit; nothing can go below it.
    assert_matches!(inner.insert::<A>(), Err(Error::TooDeep { limit: 3, position: 0 }));
}

/// Review PR #16 m1: a lowered limit never applies to OPC metadata.
#[test]
fn metadata_parts_never_use_a_lowered_limit() {
    let mut pkg = Package::from_bytes_with(package_with(b"<a/>"), ParseOptions::default().with_max_depth(1)).unwrap();
    pkg.xml_part("[Content_Types].xml").unwrap();
    pkg.xml_part("_rels/.rels").unwrap();
    pkg.xml_part("deep.xml").unwrap();
    assert_matches!(
        Package::from_bytes_with(package_with(b"<a><a/></a>"), ParseOptions::default().with_max_depth(1)).unwrap().xml_part("deep.xml"),
        Err(Error::TooDeep { limit: 1, .. })
    );
}

/// Review PR #16 m5: `Package::open_with`.
#[test]
fn open_with_reads_from_a_path() {
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("depth_limit_open_with.zip");
    std::fs::write(&path, package_with(&nested(DEFAULT_MAX_DEPTH + 1, false))).unwrap();
    let mut pkg = Package::open(&path).unwrap();
    assert_matches!(pkg.xml_part("deep.xml"), Err(Error::TooDeep { limit: DEFAULT_MAX_DEPTH, .. }));
    let opts = ParseOptions::default().with_max_depth(DEFAULT_MAX_DEPTH + 1);
    let mut pkg = Package::open_with(&path, opts).unwrap();
    assert_eq!(pkg.parse_options(), opts);
    pkg.xml_part("deep.xml").unwrap();
    std::fs::remove_file(&path).unwrap();
}
