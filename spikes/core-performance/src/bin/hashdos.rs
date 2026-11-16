//! ADR-0010 rev 3 (review round 2 N1): the interner's hasher under hostile and normal input.
//!
//! `cargo run --release --bin hashdos -- dos <blocks>`: `2^blocks` distinct element names
//! crafted so that revision 2's unkeyed FNV-1a hash of the interner key agrees in the low 22
//! bits (the reviewer's construction: chained 4-letter blocks with a birthday collision each),
//! and the same number of random names of the same length. Every hasher parses both.
//!
//! `cargo run --release --bin hashdos -- docx`: the largest fixture `word/document.xml`
//! (best of 15, 5 rounds), every hasher and core.
//!
//! The interner is generic over its hasher (`compact::parse_with::<S>`), so one copy of
//! `compact.rs` serves every candidate.

use std::io::{Cursor, Read};
use std::path::Path;
use std::time::{Duration, Instant};

/// The crate-level hasher `compact::parse` uses (as in `main.rs`).
pub type NameHasher = foldhash::fast::RandomState;

#[path = "../compact.rs"]
#[expect(dead_code)]
mod compact;

/// Revision 2's interner hasher: FNV-1a, unkeyed.
#[derive(Default)]
struct Fnv(u64);
impl std::hash::Hasher for Fnv {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        let mut h = if self.0 == 0 { 0xcbf2_9ce4_8422_2325 } else { self.0 };
        for b in bytes {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        self.0 = h;
    }
}
type FnvState = std::hash::BuildHasherDefault<Fnv>;
/// std's `RandomState`: SipHash-1-3, keyed per process.
type SipState = std::hash::RandomState;
/// ahash's `RandomState`: keyed per instance from a random source.
type AhashState = ahash::RandomState;
/// foldhash's `fast::RandomState`: keyed with a per-process random seed.
type FoldState = foldhash::fast::RandomState;

fn best<T>(n: usize, mut f: impl FnMut() -> T) -> Duration {
    (0..n)
        .map(|_| {
            let t = Instant::now();
            let v = std::hint::black_box(f());
            let dt = t.elapsed();
            drop(v);
            dt
        })
        .min()
        .unwrap()
}

const OFF: u64 = 0xcbf2_9ce4_8422_2325;
const P: u64 = 0x0100_0000_01b3;
fn step(mut h: u64, bytes: &[u8]) -> u64 {
    if h == 0 {
        h = OFF;
    }
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(P);
    }
    h
}

/// Names whose FNV-1a state over the interner key (`None` prefix first) agrees in the low `k`
/// bits: one birthday collision per 4-letter block, blocks chained.
fn colliding_names(blocks: usize, k: u32) -> Vec<String> {
    let mask = (1u64 << k) - 1;
    let mut s = step(0, &0isize.to_ne_bytes());
    let alpha = b"abcdefghijklmnopqrstuvwxyz";
    let mut pairs: Vec<[String; 2]> = Vec::new();
    for _ in 0..blocks {
        let mut seen: std::collections::HashMap<u64, String> = std::collections::HashMap::new();
        let mut found = None;
        'o: for a in alpha {
            for b in alpha {
                for c in alpha {
                    for d in alpha {
                        let blk = [*a, *b, *c, *d];
                        let h = step(s, &blk);
                        let st = String::from_utf8(blk.to_vec()).unwrap();
                        if let Some(prev) = seen.get(&(h & mask)) {
                            found = Some((prev.clone(), st, h));
                            break 'o;
                        }
                        seen.insert(h & mask, st);
                    }
                }
            }
        }
        let (x, y, h) = found.expect("collision");
        s = h;
        pairs.push([x, y]);
    }
    let mut out = vec![String::new()];
    for p in &pairs {
        out = out.iter().flat_map(|o| p.iter().map(move |q| format!("{o}{q}"))).collect();
    }
    out
}

fn doc(names: &[String]) -> Vec<u8> {
    let mut s = String::from("<r>");
    for n in names {
        s.push('<');
        s.push_str(n);
        s.push_str("/>");
    }
    s.push_str("</r>");
    s.into_bytes()
}

fn largest_document_xml() -> Vec<u8> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reference/fixtures/docx");
    let mut best_part = Vec::new();
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.extension().is_some_and(|x| x == "docx")
            && let Ok(mut z) = zip::ZipArchive::new(Cursor::new(std::fs::read(&p).unwrap()))
            && let Ok(mut f) = z.by_name("word/document.xml")
        {
            let mut v = Vec::new();
            f.read_to_end(&mut v).unwrap();
            if v.len() > best_part.len() {
                best_part = v;
            }
        }
    }
    best_part
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str).unwrap_or("dos") {
        "dos" => {
            let blocks: usize = args.get(2).map_or(16, |a| a.parse().unwrap());
            let bad = colliding_names(blocks, 22);
            let mut x = 0x2545_F491_4F6C_DD1Du64;
            let good: Vec<String> = bad
                .iter()
                .map(|n| {
                    n.bytes()
                        .map(|_| {
                            x ^= x << 13;
                            x ^= x >> 7;
                            x ^= x << 17;
                            (b'a' + (x % 26) as u8) as char
                        })
                        .collect()
                })
                .collect();
            let (bd, gd) = (doc(&bad), doc(&good));
            println!("{} names of {} bytes; part {:.1} MiB", bad.len(), bad[0].len(), bd.len() as f64 / 1_048_576.0);
            let reps = if blocks > 16 { 1 } else { 3 };
            let fb = if blocks > 16 { None } else { Some(best(reps, || compact::parse_with::<FnvState>(&bd, 256).unwrap())) };
            let fg = best(reps, || compact::parse_with::<FnvState>(&gd, 256).unwrap());
            let (sb, sg) = (best(reps, || compact::parse_with::<SipState>(&bd, 256).unwrap()), best(reps, || compact::parse_with::<SipState>(&gd, 256).unwrap()));
            let (ab, ag) = (best(reps, || compact::parse_with::<AhashState>(&bd, 256).unwrap()), best(reps, || compact::parse_with::<AhashState>(&gd, 256).unwrap()));
            let (ob, og) = (best(reps, || compact::parse_with::<FoldState>(&bd, 256).unwrap()), best(reps, || compact::parse_with::<FoldState>(&gd, 256).unwrap()));
            let (cb, cg) = (best(reps, || ooxml_core::XmlDocument::parse(&bd).unwrap()), best(reps, || ooxml_core::XmlDocument::parse(&gd).unwrap()));
            println!(
                "colliding / random: FNV {} / {fg:?} | std RandomState {sb:?} / {sg:?} | ahash {ab:?} / {ag:?} | foldhash {ob:?} / {og:?} | core {cb:?} / {cg:?}",
                fb.map_or("skipped (quadratic)".to_string(), |d| format!("{d:?}"))
            );
        }
        _ => {
            let v = largest_document_xml();
            println!("largest fixture word/document.xml: {:.1} MiB", v.len() as f64 / 1_048_576.0);
            for round in 0..5 {
                let c = best(15, || ooxml_core::XmlDocument::parse(&v).unwrap());
                let f = best(15, || compact::parse_with::<FnvState>(&v, 256).unwrap());
                let s = best(15, || compact::parse_with::<SipState>(&v, 256).unwrap());
                let a = best(15, || compact::parse_with::<AhashState>(&v, 256).unwrap());
                let o = best(15, || compact::parse_with::<FoldState>(&v, 256).unwrap());
                println!("round {round}: core {c:?} | FNV {f:?} | std RandomState {s:?} | ahash {a:?} | foldhash {o:?}");
            }
        }
    }
}
