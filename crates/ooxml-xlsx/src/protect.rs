//! Sheet protection (ADR-0014 §11): the options, and the SHA-512 password
//! hash of ECMA-376 Part 1 §18.2.29 / §18.3.1.85 (`algorithmName`,
//! `hashValue`, `saltValue`, `spinCount`).
//!
//! The hash is H(salt ‖ UTF-16LE(password)), then `spinCount` rounds of
//! H(previous ‖ LE32(i)) for i = 0, 1, …; the salt comes first (checked
//! against Excel's own output, review m9 of ADR-0014). SHA-512 and base64 are
//! implemented here (FIPS 180-4, RFC 4648) to keep the crate's dependencies
//! to the OS random number generator; both are tested against published
//! vectors.

use crate::error::{Error, Result};

/// Excel's default spin count.
pub const DEFAULT_SPIN_COUNT: u32 = 100_000;
/// The largest spin count accepted (POI's limit; Excel's UI writes 100,000).
pub const MAX_SPIN_COUNT: u32 = 10_000_000;

/// What `Worksheet::protect_sheet` writes: which actions stay **allowed** on
/// the protected sheet, and an optional password.
///
/// Defaults follow Excel's "Protect Sheet" dialog: selecting locked and
/// unlocked cells is allowed, everything else is not. Sheet protection is an
/// Excel UI setting; it does not stop edits through this crate.
#[derive(Clone)]
pub struct SheetProtection {
    password: Option<String>,
    salt: Option<[u8; 16]>,
    spin_count: u32,
    allow: Allow,
}

/// The password is redacted (PR #58 round 1 m4).
impl std::fmt::Debug for SheetProtection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SheetProtection")
            .field("password", &self.password.as_ref().map(|_| "<redacted>"))
            .field("salt", &self.salt)
            .field("spin_count", &self.spin_count)
            .field("allow", &self.allow)
            .finish()
    }
}

/// The actions a protected sheet still allows (`allow_*` = `true`).
/// Build one with struct-update syntax from the default:
/// `Allow { format_cells: true, ..Allow::default() }`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Allow {
    /// Edit objects (drawings, comments). Written as `objects`.
    pub objects: bool,
    /// Edit scenarios. Written as `scenarios`.
    pub scenarios: bool,
    /// Format cells.
    pub format_cells: bool,
    /// Format columns.
    pub format_columns: bool,
    /// Format rows.
    pub format_rows: bool,
    /// Insert columns.
    pub insert_columns: bool,
    /// Insert rows.
    pub insert_rows: bool,
    /// Insert hyperlinks.
    pub insert_hyperlinks: bool,
    /// Delete columns.
    pub delete_columns: bool,
    /// Delete rows.
    pub delete_rows: bool,
    /// Select locked cells.
    pub select_locked_cells: bool,
    /// Sort.
    pub sort: bool,
    /// Use autofilters.
    pub auto_filter: bool,
    /// Use PivotTable reports.
    pub pivot_tables: bool,
    /// Select unlocked cells.
    pub select_unlocked_cells: bool,
}

impl Default for Allow {
    fn default() -> Self {
        Allow {
            objects: false,
            scenarios: false,
            format_cells: false,
            format_columns: false,
            format_rows: false,
            insert_columns: false,
            insert_rows: false,
            insert_hyperlinks: false,
            delete_columns: false,
            delete_rows: false,
            select_locked_cells: true,
            sort: false,
            auto_filter: false,
            pivot_tables: false,
            select_unlocked_cells: true,
        }
    }
}

impl Default for SheetProtection {
    fn default() -> Self {
        SheetProtection { password: None, salt: None, spin_count: DEFAULT_SPIN_COUNT, allow: Allow::default() }
    }
}

impl SheetProtection {
    /// SheetProtection without a password, with Excel's default permissions.
    pub fn new() -> Self {
        Self::default()
    }

    /// Protects with a password (written as its SHA-512 hash; the password
    /// itself is never stored). An empty password means no password, as in
    /// Excel's dialog: no hash is written.
    pub fn password(mut self, password: &str) -> Self {
        self.password = (!password.is_empty()).then(|| password.to_owned());
        self
    }

    /// The salt for the password hash. Not secret; by default it comes from
    /// the operating system's random number generator. Byte-exact tests use
    /// this option.
    pub fn with_salt(mut self, salt: [u8; 16]) -> Self {
        self.salt = Some(salt);
        self
    }

    /// The hash's spin count (default 100,000; at most 10,000,000). Unused,
    /// and not checked, without a password.
    pub fn with_spin_count(mut self, spin_count: u32) -> Self {
        self.spin_count = spin_count;
        self
    }

    /// The actions that stay allowed.
    pub fn allow(mut self, allow: Allow) -> Self {
        self.allow = allow;
        self
    }

    /// The attributes of `sheetProtection` this protection writes, in schema
    /// order, plus the names it manages (to remove when not written). Errors:
    /// [`Error::InvalidSpinCount`], [`Error::Random`].
    pub(crate) fn attributes(&self) -> Result<Vec<(&'static str, String)>> {
        let mut out: Vec<(&'static str, String)> = Vec::new();
        if let Some(pw) = &self.password {
            if self.spin_count > MAX_SPIN_COUNT {
                return Err(Error::InvalidSpinCount(self.spin_count));
            }
            let salt = match self.salt {
                Some(s) => s,
                None => {
                    let mut s = [0u8; 16];
                    getrandom::fill(&mut s).map_err(|e| Error::Random(e.to_string()))?;
                    s
                }
            };
            let hash = password_hash(pw, &salt, self.spin_count);
            out.push(("algorithmName", "SHA-512".into()));
            out.push(("hashValue", base64(&hash)));
            out.push(("saltValue", base64(&salt)));
            out.push(("spinCount", self.spin_count.to_string()));
        }
        out.push(("sheet", "1".into()));
        // `1` = the action is protected (forbidden); written only when it
        // differs from the schema default.
        let a = &self.allow;
        for (name, allowed, default_protected) in FLAGS.iter().map(|(n, get, d)| (*n, get(a), *d)) {
            let protected = !allowed;
            if protected != default_protected {
                out.push((name, if protected { "1" } else { "0" }.into()));
            }
        }
        Ok(out)
    }
}

type Getter = fn(&Allow) -> bool;

/// `CT_SheetProtection`'s flags in schema order: (attribute, getter, schema
/// default of "protected").
pub(crate) const FLAGS: &[(&str, Getter, bool)] = &[
    ("objects", |a| a.objects, false),
    ("scenarios", |a| a.scenarios, false),
    ("formatCells", |a| a.format_cells, true),
    ("formatColumns", |a| a.format_columns, true),
    ("formatRows", |a| a.format_rows, true),
    ("insertColumns", |a| a.insert_columns, true),
    ("insertRows", |a| a.insert_rows, true),
    ("insertHyperlinks", |a| a.insert_hyperlinks, true),
    ("deleteColumns", |a| a.delete_columns, true),
    ("deleteRows", |a| a.delete_rows, true),
    ("selectLockedCells", |a| a.select_locked_cells, false),
    ("sort", |a| a.sort, true),
    ("autoFilter", |a| a.auto_filter, true),
    ("pivotTables", |a| a.pivot_tables, true),
    ("selectUnlockedCells", |a| a.select_unlocked_cells, false),
];

/// Every attribute `protect_sheet` manages: the hash (legacy and SHA forms)
/// and the flags. Others (unknown or future attributes) are kept.
pub(crate) const MANAGED: &[&str] = &[
    "password",
    "algorithmName",
    "hashValue",
    "saltValue",
    "spinCount",
    "sheet",
    "objects",
    "scenarios",
    "formatCells",
    "formatColumns",
    "formatRows",
    "insertColumns",
    "insertRows",
    "insertHyperlinks",
    "deleteColumns",
    "deleteRows",
    "selectLockedCells",
    "sort",
    "autoFilter",
    "pivotTables",
    "selectUnlockedCells",
];

/// ECMA-376's iterated SHA-512 password hash.
pub(crate) fn password_hash(password: &str, salt: &[u8], spin_count: u32) -> [u8; 64] {
    let mut buf: Vec<u8> = salt.to_vec();
    buf.extend(password.encode_utf16().flat_map(u16::to_le_bytes));
    let mut h = sha512(&buf);
    let mut round = [0u8; 68];
    for i in 0..spin_count {
        round[..64].copy_from_slice(&h);
        round[64..].copy_from_slice(&i.to_le_bytes());
        h = sha512(&round);
    }
    h
}

const K: [u64; 80] = [
    0x428a2f98d728ae22, 0x7137449123ef65cd, 0xb5c0fbcfec4d3b2f, 0xe9b5dba58189dbbc, 0x3956c25bf348b538, 0x59f111f1b605d019,
    0x923f82a4af194f9b, 0xab1c5ed5da6d8118, 0xd807aa98a3030242, 0x12835b0145706fbe, 0x243185be4ee4b28c, 0x550c7dc3d5ffb4e2,
    0x72be5d74f27b896f, 0x80deb1fe3b1696b1, 0x9bdc06a725c71235, 0xc19bf174cf692694, 0xe49b69c19ef14ad2, 0xefbe4786384f25e3,
    0x0fc19dc68b8cd5b5, 0x240ca1cc77ac9c65, 0x2de92c6f592b0275, 0x4a7484aa6ea6e483, 0x5cb0a9dcbd41fbd4, 0x76f988da831153b5,
    0x983e5152ee66dfab, 0xa831c66d2db43210, 0xb00327c898fb213f, 0xbf597fc7beef0ee4, 0xc6e00bf33da88fc2, 0xd5a79147930aa725,
    0x06ca6351e003826f, 0x142929670a0e6e70, 0x27b70a8546d22ffc, 0x2e1b21385c26c926, 0x4d2c6dfc5ac42aed, 0x53380d139d95b3df,
    0x650a73548baf63de, 0x766a0abb3c77b2a8, 0x81c2c92e47edaee6, 0x92722c851482353b, 0xa2bfe8a14cf10364, 0xa81a664bbc423001,
    0xc24b8b70d0f89791, 0xc76c51a30654be30, 0xd192e819d6ef5218, 0xd69906245565a910, 0xf40e35855771202a, 0x106aa07032bbd1b8,
    0x19a4c116b8d2d0c8, 0x1e376c085141ab53, 0x2748774cdf8eeb99, 0x34b0bcb5e19b48a8, 0x391c0cb3c5c95a63, 0x4ed8aa4ae3418acb,
    0x5b9cca4f7763e373, 0x682e6ff3d6b2b8a3, 0x748f82ee5defb2fc, 0x78a5636f43172f60, 0x84c87814a1f0ab72, 0x8cc702081a6439ec,
    0x90befffa23631e28, 0xa4506cebde82bde9, 0xbef9a3f7b2c67915, 0xc67178f2e372532b, 0xca273eceea26619c, 0xd186b8c721c0c207,
    0xeada7dd6cde0eb1e, 0xf57d4f7fee6ed178, 0x06f067aa72176fba, 0x0a637dc5a2c898a6, 0x113f9804bef90dae, 0x1b710b35131c471b,
    0x28db77f523047d84, 0x32caab7b40c72493, 0x3c9ebe0a15c9bebc, 0x431d67c49c100d4c, 0x4cc5d4becb3e42b6, 0x597f299cfc657e2a,
    0x5fcb6fab3ad6faec, 0x6c44198c4a475817,
];

/// SHA-512 (FIPS 180-4).
pub(crate) fn sha512(data: &[u8]) -> [u8; 64] {
    let mut h: [u64; 8] = [
        0x6a09e667f3bcc908,
        0xbb67ae8584caa73b,
        0x3c6ef372fe94f82b,
        0xa54ff53a5f1d36f1,
        0x510e527fade682d1,
        0x9b05688c2b3e6c1f,
        0x1f83d9abfb41bd6b,
        0x5be0cd19137e2179,
    ];
    let mut msg = data.to_vec();
    let bits = (data.len() as u128) * 8;
    msg.push(0x80);
    while msg.len() % 128 != 112 {
        msg.push(0);
    }
    msg.extend(bits.to_be_bytes());
    for block in msg.as_chunks::<128>().0 {
        let mut w = [0u64; 80];
        for (i, word) in block.as_chunks::<8>().0.iter().enumerate() {
            w[i] = u64::from_be_bytes(*word);
        }
        for i in 16..80 {
            let s0 = w[i - 15].rotate_right(1) ^ w[i - 15].rotate_right(8) ^ (w[i - 15] >> 7);
            let s1 = w[i - 2].rotate_right(19) ^ w[i - 2].rotate_right(61) ^ (w[i - 2] >> 6);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..80 {
            let s1 = e.rotate_right(14) ^ e.rotate_right(18) ^ e.rotate_right(41);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(28) ^ a.rotate_right(34) ^ a.rotate_right(39);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *x = x.wrapping_add(y);
        }
    }
    let mut out = [0u8; 64];
    for (chunk, x) in out.as_chunks_mut::<8>().0.iter_mut().zip(h) {
        *chunk = x.to_be_bytes();
    }
    out
}

/// Standard base64 with padding (RFC 4648 §4), as `xsd:base64Binary`.
pub(crate) fn base64(data: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], chunk.get(1).copied().unwrap_or(0), chunk.get(2).copied().unwrap_or(0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for (i, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            if i <= chunk.len() {
                out.push(A[((n >> shift) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    #[test]
    fn sha512_vectors() {
        // FIPS 180-4 examples (empty, "abc", the two-block message)
        assert_eq!(
            hex(&sha512(b"")),
            "cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e"
        );
        assert_eq!(
            hex(&sha512(b"abc")),
            "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"
        );
        assert_eq!(
            hex(&sha512(b"abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu")),
            "8e959b75dae313da8cf4f72814fc143f8f7779c6eb9f7fa17299aeadb6889018501d289e4900f7e4331b99dec4b5433ac7d329eeb6dd26545e96e55b874be909"
        );
        // 112 bytes: the length field no longer fits in the first block (hashlib)
        assert_eq!(hex(&sha512(&[b'a'; 112])), "c01d080efd492776a1c43bd23dd99d0a2e626d481e16782e75d54c2503b5dc32bd05f0f1ba33e568b88fd2d970929b719ecbb152f58f130a407c8830604b70ca");
    }

    #[test]
    fn base64_vectors() {
        // RFC 4648 §10
        for (i, o) in [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foob", "Zm9vYg=="), ("fooba", "Zm9vYmE="), ("foobar", "Zm9vYmFy")] {
            assert_eq!(base64(i.as_bytes()), o);
        }
    }

    /// Independent vectors computed with Python's `hashlib`
    /// (`h = sha512(salt + pw.encode('utf-16-le'))`, then for i in
    /// range(spin): `h = sha512(h + i.to_bytes(4, 'little'))`).
    #[test]
    fn password_hash_vectors() {
        for (pw, salt_hex, spin, expected) in HASHLIB_VECTORS {
            let salt: Vec<u8> = (0..salt_hex.len()).step_by(2).map(|i| u8::from_str_radix(&salt_hex[i..i + 2], 16).unwrap()).collect();
            assert_eq!(base64(&password_hash(pw, &salt, *spin)), *expected, "{pw:?} {spin}");
        }
    }

    const HASHLIB_VECTORS: &[(&str, &str, u32, &str)] = &[
        ("pwd", "000102030405060708090a0b0c0d0e0f", 100000, "/xmqd4kvup80L4OdIjJsKwwNHXo3+vF5g3PLZDMhba4AB3vsT6tZBlrqBas6PdX4dHDIyhZLgxQQEeg1u77yKg=="),
        ("", "00000000000000000000000000000000", 0, "C2y6yDjf5/R+ob0N8A7Cgv30VRDJIWEHLM+4QDU5DE2nQ9nDuVTqobD4b8mGGyPMbIZnqyMsEcaGQy67XIw/Jw=="),
        ("pässwörd 秘密", "00112233445566778899aabbccddeeff", 1, "abpaQA/ebs+hFj5VIU6JSnSd27UChGHry9CUBqhuCk+4rNC35EBrXB7eGKYui1JwgrfTkpi8j5haoLQCWHKbjw=="),
        ("x", "ffffffffffffffffffffffffffffffff", 10, "bibS9Zbj3DLCvqu9NXICkyUwAzJLwoaN8JCU81D+S3sTFIJtb98bRT3I06T5w84vq7TkMP2falSA4PuaOZGHuA=="),
    ];
}
