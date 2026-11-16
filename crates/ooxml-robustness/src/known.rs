//! Filters for fuzz findings that are already filed
//! (`docs/testing/fuzz-findings-2026-11.md`), so that a fuzzing campaign
//! keeps looking for new problems instead of rediscovering these.
//!
//! Each filter only skips inputs of the filed shape. Set
//! `OOXML_FUZZ_ALL=1` to disable every filter (e.g. to check whether a fix
//! landed: the targets must then pass on the filed reproducers). Remove a
//! filter together with the fix of its finding.

use std::sync::LazyLock;

use ooxml_core::RawNode;

static ENABLED: LazyLock<bool> = LazyLock::new(|| std::env::var_os("OOXML_FUZZ_ALL").is_none());

/// Are the filters on?
pub fn enabled() -> bool {
    *ENABLED
}

const BOM: &[u8] = b"\xEF\xBB\xBF";

/// F-1: a part that starts with two UTF-8 BOMs gets element spans 3 bytes
/// off (`parse.rs` debug assertion; corrupt output on edit).
pub fn f1_double_bom(xml: &[u8]) -> bool {
    enabled() && xml.strip_prefix(BOM).is_some_and(|r| r.starts_with(BOM))
}

/// F-2: a zip entry whose stored data reaches past the end of the archive
/// (bogus compressed size or local-header offsets): the zero-edit save
/// silently changes it.
pub fn f2_entry_out_of_bounds(zip: &[u8]) -> bool {
    use std::io::Cursor;
    if !enabled() {
        return false;
    }
    let Ok(mut a) = zip::ZipArchive::new(Cursor::new(zip)) else { return false };
    (0..a.len()).any(|i| match a.by_index_raw(i) {
        Ok(f) => f.data_start().is_none_or(|d| d.saturating_add(f.compressed_size()) > zip.len() as u64),
        Err(_) => true,
    })
}

/// F-3: lens writes to `xmlns` declarations (the lens harness does not
/// generate them while this is on).
pub fn f3_skip_xmlns_attrs() -> bool {
    enabled()
}

/// F-4: an XML declaration after the start of the document (inside the
/// root element it is dropped from the tree).
pub fn f4_inner_xml_decl(xml: &[u8]) -> bool {
    enabled()
        && xml.windows(6).enumerate().any(|(i, w)| {
            i > 3 && w.starts_with(b"<?xml") && matches!(w[5], b' ' | b'\t' | b'\r' | b'\n' | b'?')
        })
}

/// F-5: an element with an empty local name or empty prefix (`< />`).
pub fn f5_empty_name(root: &RawNode) -> bool {
    enabled()
        && (root.name().local().is_empty()
            || root.name().prefix() == Some("")
            || root.elements().any(f5_empty_name))
}

/// F-6: an entry whose declared uncompressed size is huge (reading it
/// allocates the declared size up front; libFuzzer reports an OOM).
pub fn f6_huge_declared_size(zip: &[u8]) -> bool {
    use std::io::Cursor;
    if !enabled() {
        return false;
    }
    let Ok(mut a) = zip::ZipArchive::new(Cursor::new(zip)) else { return false };
    (0..a.len()).any(|i| a.by_index_raw(i).is_ok_and(|f| f.size() > 256 << 20))
}
