//! Turning fuzzer bytes into decisions, strings and numbers.
//!
//! Every reader returns a value even when the input is exhausted (zeros),
//! so a harness never has to handle "not enough bytes".

/// A forward-only reader over fuzzer bytes.
#[derive(Clone, Debug)]
pub struct Input<'a> {
    data: &'a [u8],
}

/// Interesting fragments for text and attribute values: XML specials,
/// characters XML 1.0 cannot hold, line ends, `_xHHHH_` escapes, BOMs,
/// supplementary-plane characters.
const ATOMS: &[&str] = &[
    "a", "Z", "0", " ", "  ", "\t", "\n", "\r", "\r\n", "&", "<", ">", "\"", "'", "]]>", "&amp;", "&#13;", "<x/>",
    "_x000D_", "_x005F_", "_x005F_x0041_", "_xD83D_", "\u{b}", "\u{0}", "\u{1}", "\u{1f}", "\u{7f}", "\u{85}",
    "\u{a0}", "é", "日本語", "\u{1F600}", "\u{FFFE}", "\u{FFFF}", "\u{FEFF}", "\u{2028}", "\u{FFFD}", "\u{D7FF}",
    "\u{E000}", "1", "-1", "1E+308", "NaN", "preserve", "default", "\u{c}", "\u{9}x\u{9}", "\u{b}\u{b}",
];

impl<'a> Input<'a> {
    /// Wraps fuzzer bytes.
    pub fn new(data: &'a [u8]) -> Self {
        Input { data }
    }

    /// True when every byte has been consumed.
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// The next byte, or 0.
    pub fn u8(&mut self) -> u8 {
        match self.data.split_first() {
            Some((&b, rest)) => {
                self.data = rest;
                b
            }
            None => 0,
        }
    }

    /// The next two bytes (little endian), or 0.
    pub fn u16(&mut self) -> u16 {
        u16::from_le_bytes([self.u8(), self.u8()])
    }

    /// The next four bytes (little endian), or 0.
    pub fn u32(&mut self) -> u32 {
        u32::from_le_bytes([self.u8(), self.u8(), self.u8(), self.u8()])
    }

    /// `n` bytes (fewer if the input ends).
    pub fn take(&mut self, n: usize) -> &'a [u8] {
        let (head, rest) = self.data.split_at(n.min(self.data.len()));
        self.data = rest;
        head
    }

    /// Everything not consumed yet.
    pub fn rest(&mut self) -> &'a [u8] {
        std::mem::take(&mut self.data)
    }

    /// A short string assembled from [`ATOMS`] and arbitrary scalar values.
    pub fn string(&mut self) -> String {
        let n = self.u8() % 12;
        let mut s = String::new();
        for _ in 0..n {
            let b = self.u8();
            if usize::from(b) < ATOMS.len() * 4 {
                s.push_str(ATOMS[usize::from(b) % ATOMS.len()]);
            } else if let Some(c) = char::from_u32(self.u32() % 0x11_0000) {
                s.push(c);
            }
        }
        s
    }

    /// A short string of characters every writer must accept: letters,
    /// digits, spaces (also at the edges and doubled), tab, LF, CR, and a
    /// few non-ASCII ones.
    pub fn printable(&mut self) -> String {
        const CHARS: &[char] = &['a', 'b', 'Z', '0', ' ', ' ', '\t', '\n', '\r', '-', '&', '<', '>', '"', '\'', 'é', '日', '\u{1F600}', '\u{2011}', '\u{AD}'];
        let n = self.u8() % 12;
        (0..n).map(|_| CHARS[usize::from(self.u8()) % CHARS.len()]).collect()
    }

    /// A number: special values first, then arbitrary bit patterns.
    pub fn f64(&mut self) -> f64 {
        const SPECIAL: &[f64] = &[
            0.0,
            -0.0,
            1.0,
            -1.5,
            42.0,
            0.1,
            1e308,
            -1e308,
            5e-324,
            f64::MAX,
            f64::MIN_POSITIVE,
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            123_456_789_012_345_680.0,
        ];
        let b = self.u8();
        match SPECIAL.get(usize::from(b)) {
            Some(&v) => v,
            None => f64::from_bits(u64::from(self.u32()) << 32 | u64::from(self.u32())),
        }
    }
}
