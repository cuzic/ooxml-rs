//! A small deterministic mutator for the stable smoke test (no libFuzzer,
//! no external crates): byte flips, deletions, duplications and splices of
//! XML-significant tokens.

/// xorshift64*: deterministic, seedable, good enough for mutation choices.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    /// A generator; seed 0 is replaced by a fixed constant.
    pub fn new(seed: u64) -> Self {
        Rng(if seed == 0 { 0x9E37_79B9_7F4A_7C15 } else { seed })
    }

    /// Next value.
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform-ish in `0..n` (`n > 0`).
    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }

    /// Random bytes.
    pub fn bytes(&mut self, n: usize) -> Vec<u8> {
        (0..n).map(|_| self.next_u64() as u8).collect()
    }
}

const TOKENS: &[&str] = &[
    "<", ">", "/>", "</", "<a>", "</a>", "<x:a>", "xmlns=\"\"", " xmlns:x=\"urn:x\"", " a=\"1\"", " a='&#9;'", "&amp;",
    "&#0;", "&#xD;", "&#x10FFFF;", "&lt;", "&bogus;", "<![CDATA[", "]]>", "<!--", "-->", "<?pi?>", "<?xml version=\"1.0\"?>",
    "<!DOCTYPE a>", "\u{feff}", "\r", "\r\n", "\t", "\u{0}", "\u{b}", "\u{FFFE}", "\u{1F600}", "=", "\"", "'", ":",
    "xml:space=\"preserve\"", "mc:AlternateContent",
];

/// Applies 1–4 random mutations to `data`.
pub fn mutate(rng: &mut Rng, data: &[u8]) -> Vec<u8> {
    let mut v = data.to_vec();
    for _ in 0..=rng.below(4) {
        let len = v.len();
        match rng.below(6) {
            0 if len > 0 => {
                let i = rng.below(len);
                v[i] ^= 1 << rng.below(8);
            }
            1 if len > 0 => {
                let i = rng.below(len);
                let n = rng.below((len - i).min(64) + 1);
                v.drain(i..i + n);
            }
            2 if len > 0 => {
                let i = rng.below(len);
                let n = rng.below((len - i).min(64) + 1);
                let chunk = v[i..i + n].to_vec();
                let at = rng.below(v.len() + 1);
                v.splice(at..at, chunk);
            }
            3 => {
                let at = rng.below(len + 1);
                let n = rng.below(8) + 1;
                let b = rng.bytes(n);
                v.splice(at..at, b);
            }
            4 if len > 0 => v.truncate(rng.below(len)),
            _ => {
                let at = rng.below(len + 1);
                let t = TOKENS[rng.below(TOKENS.len())].as_bytes();
                v.splice(at..at, t.iter().copied());
            }
        }
    }
    v
}
