//! Regressions for the fuzz findings of PR #36
//! (`docs/testing/fuzz-findings-2026-11.md` there): F-1 … F-6 and the
//! cheap Info items. F-7 (quadratic duplicate-name check) is fixed by
//! PR #14 (`core/remove-part`) and not repeated here.
//!
//! Also: every package under `reference/fixtures/**` still opens, has no
//! part refused by the new checks, and saves Layer-1 identical.

use std::alloc::{GlobalAlloc, Layout, System};
use std::assert_matches;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use ooxml_core::name::XMLNS_NS;
use ooxml_core::{
    AttrSpec, ChildOf, Content, ElementSpec, Error, Package, ParseOptions, QName, Slot, WellFormedness, XmlDocument,
};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

// ---- allocation probe (F-6, zip bomb) ----

/// Records the largest single allocation request made by the measuring
/// thread while it is armed. Allocations by other test threads running in
/// parallel are not counted (they made this test flaky).
struct Peak;
static LARGEST: AtomicUsize = AtomicUsize::new(0);
thread_local! {
    static ARMED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn armed() -> bool {
    ARMED.try_with(std::cell::Cell::get).unwrap_or(false)
}
/// Serializes the tests that measure allocations.
static MEASURE: Mutex<()> = Mutex::new(());

unsafe impl GlobalAlloc for Peak {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        if armed() {
            LARGEST.fetch_max(l.size(), Ordering::Relaxed);
        }
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        if armed() {
            LARGEST.fetch_max(n, Ordering::Relaxed);
        }
        unsafe { System.realloc(p, l, n) }
    }
}
#[global_allocator]
static GLOBAL: Peak = Peak;

/// Runs `f` and returns its result with the largest allocation it requested.
fn peak_alloc<T>(f: impl FnOnce() -> T) -> (T, usize) {
    let _g = MEASURE.lock().unwrap_or_else(|e| e.into_inner());
    LARGEST.store(0, Ordering::Relaxed);
    ARMED.with(|a| a.set(true));
    let r = f();
    ARMED.with(|a| a.set(false));
    (r, LARGEST.load(Ordering::Relaxed))
}

// ---- zip helpers ----

const CT: &[u8] = br#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Default Extension="bin" ContentType="application/octet-stream"/></Types>"#;

fn zip_of(parts: &[(&str, &[u8], CompressionMethod)]) -> Vec<u8> {
    let mut zw = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes, method) in parts {
        zw.start_file(*name, SimpleFileOptions::default().compression_method(*method).large_file(bytes.len() > u32::MAX as usize))
            .unwrap();
        zw.write_all(bytes).unwrap();
    }
    zw.finish().unwrap().into_inner()
}

/// Offset of the central-directory record of `name`.
fn cd_record(z: &[u8], name: &str) -> usize {
    (0..z.len() - 46)
        .find(|&i| z[i..].starts_with(b"PK\x01\x02") && z[i + 46..].starts_with(name.as_bytes()))
        .expect("central directory record")
}

/// Overwrites a 4-byte little-endian field of `name`'s central-directory
/// record (20 = compressed size, 24 = uncompressed size, 42 = local header offset).
fn patch_cd(z: &mut [u8], name: &str, field: usize, value: u32) {
    let at = cd_record(z, name) + field;
    z[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

/// (name, method, CRC-32, compressed bytes) of every entry.
fn raw_entries(bytes: &[u8]) -> Vec<(String, CompressionMethod, u32, Vec<u8>)> {
    let mut ar = ZipArchive::new(Cursor::new(bytes)).unwrap();
    (0..ar.len())
        .map(|i| {
            let mut f = ar.by_index_raw(i).unwrap();
            let mut raw = Vec::new();
            f.read_to_end(&mut raw).unwrap();
            (f.name().unwrap().into_owned(), f.compression(), f.crc32(), raw)
        })
        .collect()
}

// ---- small lens schema (F-3) ----

enum P {}
impl ElementSpec for P {
    const QNAME: QName = QName::new("urn:t", "p");
    const SDK_NAME: &'static str = "t:p";
    const ATTRS: &'static [AttrSpec] = &[];
    const CONTENT: Content = Content::Slots(&[Slot { names: &[QName::new("urn:t", "ext")], wildcard: None, max_each: Some(1), max_total: Some(1) }]);
}
enum Ext {}
impl ElementSpec for Ext {
    const QNAME: QName = QName::new("urn:t", "ext");
    const SDK_NAME: &'static str = "t:ext";
    const ATTRS: &'static [AttrSpec] = &[];
    const CONTENT: Content = Content::Empty;
}
impl ChildOf<P> for Ext {}

fn not_wf(src: &str) -> WellFormedness {
    match XmlDocument::parse(src.as_bytes()) {
        Err(Error::NotWellFormed { reason, .. }) => reason,
        other => panic!("{src:?}: expected NotWellFormed, got {:?}", other.map(|d| d.serialize())),
    }
}

// ---- F-1: BOMs and exact spans ----

#[test]
fn f1_second_byte_order_mark_is_refused() {
    assert_eq!(not_wf("\u{feff}\u{feff}<p xmlns=\"urn:t\"><x/>y</p>"), WellFormedness::ByteOrderMark);
    assert_matches!(
        XmlDocument::parse("\u{feff}\u{feff}<a/>".as_bytes()),
        Err(Error::NotWellFormed { position: 3, reason: WellFormedness::ByteOrderMark })
    );
}

/// Spans are exact with one BOM, and with U+FEFF elsewhere (where quick-xml
/// does not skip it): every element span covers `<…>` and an edit writes
/// exactly what is expected.
#[test]
fn f1_spans_are_exact_around_byte_order_marks() {
    for (src, want) in [
        ("\u{feff}<p xmlns=\"urn:t\"><x/>y</p>", "\u{feff}<p xmlns=\"urn:t\"><x/>y<ext/></p>"),
        (
            "\u{feff}<?xml version=\"1.0\"?>\u{feff}<p xmlns=\"urn:t\"><x/>y</p>",
            "\u{feff}<?xml version=\"1.0\"?>\u{feff}<p xmlns=\"urn:t\"><x/>y<ext/></p>",
        ),
        ("<p xmlns=\"urn:t\">\u{feff}<x/>\u{feff}</p>", "<p xmlns=\"urn:t\">\u{feff}<x/>\u{feff}<ext/></p>"),
    ] {
        let mut doc = XmlDocument::parse(src.as_bytes()).unwrap();
        let x = doc.root().elements().next().unwrap().span().unwrap();
        assert_eq!(&src[x.start..x.end()], "<x/>", "{src:?}");
        let r = doc.root().span().unwrap();
        assert!(src[r.start..r.open_end].starts_with("<p "), "{src:?}");
        doc.lens_mut::<P>().unwrap().insert::<Ext>().unwrap();
        assert_eq!(doc.serialize(), want);
    }
}

// ---- F-2: entry data outside the archive ----

#[test]
fn f2_entry_data_past_the_end_is_refused_at_open() {
    let z0 = zip_of(&[("[Content_Types].xml", CT, CompressionMethod::Stored), ("a.bin", b"0123456789", CompressionMethod::Stored)]);
    // Compressed size beyond the end.
    let mut z = z0.clone();
    patch_cd(&mut z, "a.bin", 20, 0x7FFF_0000);
    assert_matches!(Package::from_bytes(z), Err(Error::TruncatedEntry { ref part, .. }) if part == "a.bin");
    // Local header offset beyond the end.
    let mut z = z0.clone();
    patch_cd(&mut z, "a.bin", 42, 0x7FFF_0000);
    assert!(Package::from_bytes(z).is_err());
    // Untouched: opens and passes through.
    let pkg = Package::from_bytes(z0.clone()).unwrap();
    assert_eq!(raw_entries(&pkg.to_bytes().unwrap()), raw_entries(&z0));
}

// ---- F-3: namespace declarations through the lens ----

#[test]
fn f3_lens_refuses_namespace_declarations() {
    let src = r#"<p xmlns="urn:t" xmlns:o="urn:o"><o:z/></p>"#;
    for q in [
        QName::new(XMLNS_NS, "o"),
        QName::new(XMLNS_NS, "xmlns"),
        QName::new(XMLNS_NS, "new"),
        QName::new("", "xmlns"),
        QName::new("", "xmlns:o"),
    ] {
        let mut doc = XmlDocument::parse(src.as_bytes()).unwrap();
        let mut l = doc.lens_mut::<P>().unwrap();
        assert_matches!(l.set_attr(q, "urn:other"), Err(Error::NamespaceDeclaration(n)) if n == q);
        assert_matches!(l.remove_attr(q), Err(Error::NamespaceDeclaration(n)) if n == q);
        assert!(!doc.is_dirty());
        assert_eq!(doc.serialize(), src);
    }
    // Ordinary attributes are unaffected.
    let mut doc = XmlDocument::parse(src.as_bytes()).unwrap();
    doc.lens_mut::<P>().unwrap().set_attr(QName::new("", "xmlnsish"), "1").unwrap();
    doc.lens_mut::<P>().unwrap().set_attr(QName::new("urn:o", "x"), "1").unwrap();
    assert_eq!(doc.serialize(), r#"<p xmlns="urn:t" xmlns:o="urn:o" xmlnsish="1" o:x="1"><o:z/></p>"#);
}

// ---- F-4, F-5, Info ----

#[test]
fn f4_xml_declaration_not_at_the_start_is_refused() {
    for src in [
        "<a>x<?xml version=\"1.0\"?>y</a>",
        "<sst>r<?xml?>,</sst>",
        "<a><?xml encoding=\"latin1\"?></a>",
        " <?xml version=\"1.0\"?><a/>",
        "<!--c--><?xml version=\"1.0\"?><a/>",
        "<a/><?xml version=\"1.0\"?>",
    ] {
        assert_eq!(not_wf(src), WellFormedness::XmlDeclarationNotAtStart, "{src:?}");
    }
    for ok in ["<?xml version=\"1.0\"?><a/>", "\u{feff}<?xml version=\"1.0\"?><a/>", "<a><?xml-stylesheet href=\"x\"?></a>"] {
        XmlDocument::parse(ok.as_bytes()).unwrap_or_else(|e| panic!("{ok:?}: {e}"));
    }
}

#[test]
fn f5_empty_names_are_refused() {
    for src in ["< />", "<\r/>", "<\t\t/>", "<a>< /></a>", "<:a/>", "<a :b=\"1\"/>", "<a:b xmlns:a=\"u\"><: /></a:b>"] {
        assert_eq!(not_wf(src), WellFormedness::EmptyName, "{src:?}");
    }
}

#[test]
fn info_nul_and_empty_prefix_binding_are_refused() {
    assert_eq!(not_wf("<a>\0</a>"), WellFormedness::NullCharacter);
    assert_eq!(not_wf("<a b=\"\0\"/>"), WellFormedness::NullCharacter);
    assert_eq!(not_wf("<a xmlns:p=\"\"/>"), WellFormedness::EmptyPrefixBinding);
    // The default namespace may be undeclared.
    XmlDocument::parse(br#"<a xmlns="urn:x"><b xmlns=""/></a>"#).unwrap();
}

// ---- F-6 and the decompression limits ----

#[test]
fn f6_declared_size_never_drives_allocation() {
    let z0 = zip_of(&[("[Content_Types].xml", CT, CompressionMethod::Stored), ("a.bin", b"0123456789", CompressionMethod::Deflated)]);
    // Declared ~4 GB: above the default part limit, refused before reading.
    let mut z = z0.clone();
    patch_cd(&mut z, "a.bin", 24, 0xF000_0000);
    let pkg = Package::from_bytes(z).unwrap();
    let (r, peak) = peak_alloc(|| pkg.read_part("a.bin"));
    assert_matches!(r, Err(Error::PartTooLarge { .. }));
    assert!(peak < 1 << 20, "requested {peak} bytes");
    // Declared 256 MiB: under the limit, so it is read, but the declared
    // size is not allocated up front.
    let mut z = z0;
    patch_cd(&mut z, "a.bin", 24, 256 << 20);
    let pkg = Package::from_bytes(z).unwrap();
    let (_, peak) = peak_alloc(|| pkg.read_part("a.bin"));
    assert!(peak < 1 << 20, "requested {peak} bytes");
}

/// 64 MiB of spaces deflate to ~64 KiB: with a 16 MiB part limit the read
/// stops at the limit (no 64 MiB buffer); as `[Content_Types].xml` the open
/// fails; the package limit refuses at open from the declared sizes.
#[test]
fn zip_bomb_is_bounded_by_the_limits() {
    let mut bomb = b"<a>".to_vec();
    bomb.resize(64 << 20, b' ');
    bomb.extend_from_slice(b"</a>");
    let z = zip_of(&[("[Content_Types].xml", CT, CompressionMethod::Stored), ("bomb.xml", &bomb, CompressionMethod::Deflated)]);
    assert!(z.len() < 1 << 20, "the bomb is small: {} bytes", z.len());

    let opts = ParseOptions::default().with_max_part_bytes(16 << 20);
    let mut pkg = Package::from_bytes_with(z.clone(), opts).unwrap();
    let (r, peak) = peak_alloc(|| pkg.read_part("bomb.xml"));
    assert_matches!(r, Err(Error::PartTooLarge { ref part, limit }) if part == "bomb.xml" && limit == 16 << 20);
    assert!(peak <= 40 << 20, "requested {peak} bytes for a 16 MiB limit");
    assert_matches!(pkg.xml_part("bomb.xml"), Err(Error::PartTooLarge { .. }));
    // Raw passthrough is still fine: the part is never decompressed.
    assert_eq!(raw_entries(&pkg.to_bytes().unwrap()), raw_entries(&z));
    // Default limits: it reads.
    assert_eq!(Package::from_bytes(z.clone()).unwrap().read_part("bomb.xml").unwrap().len(), bomb.len());

    // Package limit.
    assert_matches!(
        Package::from_bytes_with(z, ParseOptions::default().with_max_package_bytes(32 << 20)),
        Err(Error::PackageTooLarge { limit, .. }) if limit == 32 << 20
    );

    // A bomb as [Content_Types].xml fails the open itself.
    let mut ct = CT.to_vec();
    ct.splice(ct.len() - "</Types>".len()..ct.len() - "</Types>".len(), std::iter::repeat_n(b' ', 32 << 20));
    let z = zip_of(&[("[Content_Types].xml", &ct, CompressionMethod::Deflated)]);
    assert_matches!(
        Package::from_bytes_with(z, ParseOptions::default().with_max_part_bytes(1 << 20)),
        Err(Error::PartTooLarge { ref part, .. }) if part == "[Content_Types].xml"
    );
}

// ---- every fixture still opens and round-trips ----

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            collect(&p, out);
        } else if p.extension().and_then(|x| x.to_str()).is_some_and(|x| {
            matches!(x, "xlsx" | "xlsm" | "pptx" | "pptm" | "ppsx" | "potx" | "docx" | "docm")
        }) {
            out.push(p);
        }
    }
}

#[test]
fn every_fixture_opens_parses_and_round_trips() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reference");
    let mut files = Vec::new();
    collect(&root.join("fixtures"), &mut files);
    collect(&root.join("round_trip_corpus"), &mut files);
    files.sort();
    assert!(files.len() >= 70, "{} fixture packages", files.len());
    let (mut opened, mut parsed) = (0, 0);
    for f in &files {
        let input = std::fs::read(f).unwrap();
        let mut pkg = match Package::from_bytes(input.clone()) {
            Ok(p) => p,
            Err(Error::EncryptedPackage | Error::StrictNotSupported(_)) => continue,
            Err(e) => panic!("{}: {e}", f.display()),
        };
        opened += 1;
        let names: Vec<String> = pkg.part_names().map(str::to_owned).collect();
        for n in &names {
            if !(n.ends_with(".xml") || n.ends_with(".rels")) {
                continue;
            }
            match pkg.xml_part(n) {
                Ok(_) => parsed += 1,
                Err(e @ (Error::NotWellFormed { .. } | Error::PartTooLarge { .. } | Error::PackageTooLarge { .. })) => {
                    panic!("{} {n}: refused by a new check: {e}", f.display())
                }
                Err(_) => {} // e.g. DtdNotAllowed in poi-ExternalEntityInText.docx: refused before this change too
            }
        }
        assert_eq!(raw_entries(&pkg.to_bytes().unwrap()), raw_entries(&input), "{}: Layer 1", f.display());
    }
    assert!(opened >= 65 && parsed > 1000, "opened {opened} packages, parsed {parsed} parts");
}
