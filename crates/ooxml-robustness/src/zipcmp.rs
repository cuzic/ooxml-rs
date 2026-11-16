//! Layer-1 oracle on raw zip entries (ADR-0001): name, compression method,
//! CRC-32 and compressed bytes.

use std::io::{Cursor, Read};

use zip::{CompressionMethod, ZipArchive};

/// One zip entry as stored (not decompressed).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawEntry {
    /// Entry name.
    pub name: String,
    /// Compression method.
    pub method: CompressionMethod,
    /// CRC-32 from the central directory.
    pub crc32: u32,
    /// The compressed bytes.
    pub compressed: Vec<u8>,
}

/// Reads every entry of a zip without decompressing it.
pub fn raw_entries(bytes: &[u8]) -> zip::result::ZipResult<Vec<RawEntry>> {
    let mut archive = ZipArchive::new(Cursor::new(bytes))?;
    let mut out = Vec::with_capacity(archive.len());
    for i in 0..archive.len() {
        let mut f = archive.by_index_raw(i)?;
        let name = f.name()?.into_owned();
        let (method, crc32) = (f.compression(), f.crc32());
        let mut compressed = Vec::new();
        f.read_to_end(&mut compressed)?;
        out.push(RawEntry { name, method, crc32, compressed });
    }
    Ok(out)
}

/// Asserts Layer 1 between two packages: same entry names in the same
/// order, and every entry not in `except` byte-identical as stored.
///
/// `input` entries that the raw reader cannot read make the check a no-op
/// (the package reader got past them by another route); the output was
/// written by us and must always be readable.
pub fn assert_l1_except(input: &[u8], output: &[u8], except: &[&str], what: &str) {
    let Ok(a) = raw_entries(input) else { return };
    let b = raw_entries(output).unwrap_or_else(|e| panic!("{what}: written package is not a readable zip: {e}"));
    let names_a: Vec<&str> = a.iter().map(|e| e.name.as_str()).collect();
    let names_b: Vec<&str> = b.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names_a, names_b, "{what}: entry names/order changed");
    for (x, y) in a.iter().zip(&b) {
        if except.contains(&x.name.as_str()) {
            continue;
        }
        assert_eq!(
            (x.method, x.crc32, x.compressed.len()),
            (y.method, y.crc32, y.compressed.len()),
            "{what}: entry {:?} changed (method, crc, compressed length)",
            x.name
        );
        assert!(x.compressed == y.compressed, "{what}: entry {:?} compressed bytes changed", x.name);
    }
}
