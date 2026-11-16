//! One regression test per fuzz finding of
//! `docs/testing/fuzz-findings-2026-11.md`. Each test states the behaviour
//! the contract asks for, so it fails until the finding is fixed; they are
//! `#[ignore]`d to keep CI green. After a fix, remove the `#[ignore]` and
//! the matching filter in `src/known.rs`.
//!
//! Run them: `cargo test -p ooxml-robustness --test fuzz_findings -- --ignored`

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use ooxml_core::name::XMLNS_NS;
use ooxml_core::{Package, QName, XmlDocument};
use ooxml_robustness::fixtures::{XLSX, build_zip};
use ooxml_robustness::lens::{Ext, P};
use ooxml_robustness::xml::canon_eq;
use ooxml_robustness::zipcmp::assert_l1_except;

/// Records the largest single allocation request (F-6).
struct Peak;
static LARGEST: AtomicUsize = AtomicUsize::new(0);
unsafe impl GlobalAlloc for Peak {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        LARGEST.fetch_max(l.size(), Ordering::Relaxed);
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        LARGEST.fetch_max(n, Ordering::Relaxed);
        unsafe { System.realloc(p, l, n) }
    }
}
#[global_allocator]
static GLOBAL: Peak = Peak;

const CT: &[u8] = br#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="bin" ContentType="application/octet-stream"/></Types>"#;

/// A two-entry stored zip whose central-directory record for `a.bin` is
/// patched: `field` is the offset in the record (20 = compressed size,
/// 24 = uncompressed size).
fn patched_zip(field: usize, value: u32) -> Vec<u8> {
    let mut z = build_zip([("[Content_Types].xml", CT), ("a.bin", &b"0123456789"[..])]);
    let cd = z
        .windows(4)
        .enumerate()
        .filter(|(_, w)| *w == b"PK\x01\x02")
        .map(|(i, _)| i)
        .find(|&i| z[i + 46..].starts_with(b"a.bin"))
        .expect("central directory record of a.bin");
    z[cd + field..cd + field + 4].copy_from_slice(&value.to_le_bytes());
    z
}

/// F-1 (High): a part that starts with two UTF-8 BOMs is parsed with every
/// element span 3 bytes off (quick-xml skips the second BOM without
/// counting it; `parse.rs` only checks this with a `debug_assert`). Any
/// edit then writes the wrong source ranges.
#[test]
#[ignore = "fuzz finding F-1, see docs/testing/fuzz-findings-2026-11.md"]
fn f1_double_bom_edit_writes_well_formed_xml() {
    let src = "\u{feff}\u{feff}<p xmlns=\"urn:t\"><item/>x</p>";
    let Ok(mut doc) = XmlDocument::parse(src.as_bytes()) else { return }; // refusing is fine
    let child = doc.root().elements().next().unwrap().span().unwrap();
    assert_eq!(&src[child.start..child.end()], "<item/>", "span of <item/>");
    doc.lens_mut::<P>().unwrap().insert::<Ext>().unwrap();
    let out = doc.serialize();
    assert_eq!(out, "\u{feff}\u{feff}<p xmlns=\"urn:t\"><item/>x<ext/></p>");
}

/// F-1 through ooxml-xlsx: the write succeeds and the saved workbook no
/// longer opens.
#[test]
#[ignore = "fuzz finding F-1, see docs/testing/fuzz-findings-2026-11.md"]
fn f1_double_bom_xlsx_write_saves_a_workbook_that_reopens() {
    let fx = XLSX.iter().find(|f| f.name.ends_with("51850.xlsx")).expect("fixture poi/51850.xlsx");
    let sheet = "xl/worksheets/sheet1.xml";
    let parts: Vec<(String, Vec<u8>)> = fx
        .parts
        .iter()
        .map(|(n, b)| {
            let mut v = b.clone();
            if n == sheet {
                v.splice(0..0, *b"\xEF\xBB\xBF\xEF\xBB\xBF");
            }
            (n.clone(), v)
        })
        .collect();
    let zip = build_zip(parts.iter().map(|(n, b)| (n.as_str(), b.as_slice())));
    let Ok(mut wb) = ooxml_xlsx::Workbook::from_bytes(zip) else { return };
    assert_eq!(wb.worksheet_part(), sheet);
    if wb.worksheet().unwrap().cell_mut(0, 0).unwrap().set_number(7.0).is_err() {
        return; // refusing is fine
    }
    let out = wb.to_bytes().unwrap();
    let mut re = ooxml_xlsx::Workbook::from_bytes(out).expect("edited workbook reopens");
    let v = re.worksheet().unwrap().cell(0, 0).unwrap().unwrap().value().unwrap();
    assert_eq!(v, ooxml_xlsx::CellValue::Number(7.0));
}

/// F-2 (Medium): an entry whose compressed size reaches past the end of
/// the archive is accepted, and the zero-edit save writes a different
/// entry (Layer 1 broken) instead of refusing.
#[test]
#[ignore = "fuzz finding F-2, see docs/testing/fuzz-findings-2026-11.md"]
fn f2_entry_past_end_of_archive_is_refused_or_copied_unchanged() {
    let z = patched_zip(20, 0x7FFF_0000);
    let Ok(pkg) = Package::from_bytes(z.clone()) else { return };
    let Ok(out) = pkg.to_bytes() else { return };
    assert_l1_except(&z, &out, &[], "F-2");
}

/// F-3 (Medium): `set_attr` / `remove_attr` on an `xmlns:*` declaration
/// are not refused. Removing a declaration still in use makes the output
/// not well-formed; rebinding it silently moves existing names to another
/// namespace (the tree and the written bytes disagree).
#[test]
#[ignore = "fuzz finding F-3, see docs/testing/fuzz-findings-2026-11.md"]
fn f3_lens_writes_to_namespace_declarations_keep_output_meaning() {
    let src = r#"<p xmlns="urn:t" xmlns:o="urn:o"><o:z/></p>"#;
    let decl = QName::new(XMLNS_NS, "o");
    for edit in [0, 1] {
        let mut doc = XmlDocument::parse(src.as_bytes()).unwrap();
        let r = {
            let mut l = doc.lens_mut::<P>().unwrap();
            if edit == 0 { l.remove_attr(decl).map(drop) } else { l.set_attr(decl, "urn:other") }
        };
        if r.is_err() {
            continue; // refusing is fine
        }
        let out = doc.serialize();
        let re = XmlDocument::parse(out.as_bytes()).unwrap_or_else(|e| panic!("edit {edit}: {out:?} does not parse: {e}"));
        assert!(canon_eq(re.root(), doc.root()), "edit {edit}: {out:?} means something else than the tree");
    }
}

/// F-4 (Low): an XML declaration inside the root element is accepted (not
/// well-formed) and dropped from the tree, so re-rendering loses it and the
/// text around it becomes two items (T1b fails).
#[test]
#[ignore = "fuzz finding F-4, see docs/testing/fuzz-findings-2026-11.md"]
fn f4_xml_declaration_inside_content_is_refused_or_kept() {
    for src in ["<a>x<?xml version=\"1.0\"?>y</a>", "<sst>r<?xml?>,</sst>"] {
        let Ok(doc) = XmlDocument::parse(src.as_bytes()) else { continue };
        let mut r = doc.clone();
        r.force_rerender();
        let out = r.serialize();
        let re = XmlDocument::parse(out.as_bytes()).unwrap();
        assert!(re.root().structurally_eq(doc.root()), "{src:?} re-renders as {out:?}");
    }
}

/// F-5 (Low): an element with an empty name (`< />`, `<\r/>`) is accepted;
/// re-rendering writes `</>`, which no parser reads.
#[test]
#[ignore = "fuzz finding F-5, see docs/testing/fuzz-findings-2026-11.md"]
fn f5_empty_element_name_is_refused() {
    for src in ["< />", "<\r/>", "<a>< /></a>", "<a:b xmlns:a=\"u\"><: /></a:b>"] {
        if let Ok(doc) = XmlDocument::parse(src.as_bytes()) {
            panic!("{src:?} accepted as {:?}", doc.root());
        }
    }
}

/// F-6 (Medium): reading a part allocates its *declared* uncompressed size
/// up front (`Vec::with_capacity(file.size())` in `Package::read_original`).
/// A 10-byte entry declared as ~4 GB requests ~4 GB; libFuzzer reports it
/// as an OOM, and without overcommit (or on 32-bit) the process aborts.
#[test]
#[ignore = "fuzz finding F-6, see docs/testing/fuzz-findings-2026-11.md"]
fn f6_declared_size_does_not_drive_allocation() {
    let z = patched_zip(24, 0xF000_0000);
    let Ok(pkg) = Package::from_bytes(z) else { return };
    LARGEST.store(0, Ordering::Relaxed);
    let _ = pkg.read_part("a.bin");
    let peak = LARGEST.load(Ordering::Relaxed);
    assert!(peak < 64 << 20, "reading a 10-byte part requested a {peak}-byte allocation");
}

/// A package of `n` empty stored entries (plus `[Content_Types].xml`).
fn many_entries(n: usize) -> Vec<u8> {
    let names: Vec<String> = (0..n).map(|i| format!("p/{i:07}")).collect();
    build_zip(std::iter::once(("[Content_Types].xml", CT)).chain(names.iter().map(|s| (s.as_str(), &b""[..]))))
}

/// F-7 (Medium): `Package::from_bytes` checks for duplicate part names by
/// comparing every entry with every earlier one (`package.rs:123`), so
/// opening is quadratic in the entry count: a 6 MB zip of 65,535 empty
/// entries took 30 s to open in a release build (the zip crate alone reads
/// its directory in 36 ms). Measured as a ratio so the test does not depend
/// on machine speed: 4x the entries must cost well under 16x the time.
#[test]
#[ignore = "fuzz finding F-7, see docs/testing/fuzz-findings-2026-11.md"]
fn f7_open_is_not_quadratic_in_the_entry_count() {
    let time = |n: usize| {
        let z = many_entries(n);
        let t = std::time::Instant::now();
        Package::from_bytes(z).expect("opens");
        t.elapsed().as_secs_f64()
    };
    let (small, large) = (time(4_000), time(16_000));
    assert!(large < small * 8.0, "4,000 entries: {small:.3}s, 16,000 entries: {large:.3}s (quadratic)");
}
