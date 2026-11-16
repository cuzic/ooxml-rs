//! ADR-0010 (review round 2 N1, round 3 R3-1): the interner under hostile and normal input.
//!
//! `cargo run --release --bin hashdos -- dos <blocks>`: `2^blocks` distinct element names
//! crafted so that revision 2's unkeyed FNV-1a hash of the interner key agrees in the low 22
//! bits (the reviewer's construction: chained 4-letter blocks with a birthday collision each),
//! and the same number of random names of the same length. Reports time and, independent of
//! the host's load, **key equality calls per interned name** (the stage-1 acceptance measure).
//!
//! `cargo run --release --bin hashdos -- distinct <n>`: `n` distinct random names (the
//! front cache's worst case): std + front cache against core.
//!
//! `cargo run --release --bin hashdos -- docx`: the largest fixture `word/document.xml`
//! (best of 15, 5 rounds): core, FNV, std alone, std + front cache (the chosen form).
//!
//! The interner is generic over its hasher (`compact::parse_opts::<S>`), so one copy of
//! `compact.rs` serves every candidate.

use std::io::{Cursor, Read};
use std::path::Path;
use std::sync::atomic::Ordering::Relaxed;
use std::time::{Duration, Instant};

/// The crate-level hasher `compact::parse` uses (as in `main.rs`).
pub type NameHasher = std::hash::RandomState;

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
/// std's `RandomState`: SipHash-1-3, keyed per instance.
type SipState = std::hash::RandomState;

/// Key equality calls per name while parsing `d` (each name occurs once).
fn eq_per_name<S: std::hash::BuildHasher + Default>(d: &[u8], names: usize, front: bool) -> f64 {
    compact::KEY_EQ_CALLS.store(0, Relaxed);
    std::hint::black_box(compact::parse_opts::<S>(d, 256, front).unwrap());
    compact::KEY_EQ_CALLS.load(Relaxed) as f64 / names as f64
}

fn random_names(like: &[String], seed: u64) -> Vec<String> {
    let mut x = seed;
    like.iter()
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
        .collect()
}

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
            let good = random_names(&bad, 0x2545_F491_4F6C_DD1D);
            let (bd, gd) = (doc(&bad), doc(&good));
            let n = bad.len();
            println!("{n} names of {} bytes; part {:.1} MiB", bad[0].len(), bd.len() as f64 / 1_048_576.0);
            let reps = if blocks > 16 { 1 } else { 3 };
            let fb = if blocks > 16 { None } else { Some(best(reps, || compact::parse_opts::<FnvState>(&bd, 256, false).unwrap())) };
            let fg = best(reps, || compact::parse_opts::<FnvState>(&gd, 256, false).unwrap());
            let (sb, sg) = (best(reps, || compact::parse_opts::<SipState>(&bd, 256, false).unwrap()), best(reps, || compact::parse_opts::<SipState>(&gd, 256, false).unwrap()));
            let (cb, cg) = (best(reps, || compact::parse_with::<SipState>(&bd, 256).unwrap()), best(reps, || compact::parse_with::<SipState>(&gd, 256).unwrap()));
            let (kb, kg) = (best(reps, || ooxml_core::XmlDocument::parse(&bd).unwrap()), best(reps, || ooxml_core::XmlDocument::parse(&gd).unwrap()));
            println!(
                "time colliding / random: FNV {} / {fg:?} | std {sb:?} / {sg:?} | std+front {cb:?} / {cg:?} | core {kb:?} / {kg:?}",
                fb.map_or("skipped (quadratic)".to_string(), |d| format!("{d:?}"))
            );
            let fnv = if blocks > 16 { "skipped".to_string() } else { format!("{:.3} / {:.3}", eq_per_name::<FnvState>(&bd, n, false), eq_per_name::<FnvState>(&gd, n, false)) };
            println!(
                "key Eq calls per name, colliding / random: FNV {fnv} | std {:.3} / {:.3} | std+front {:.3} / {:.3}",
                eq_per_name::<SipState>(&bd, n, false),
                eq_per_name::<SipState>(&gd, n, false),
                eq_per_name::<SipState>(&bd, n, true),
                eq_per_name::<SipState>(&gd, n, true),
            );
        }
        "distinct" => {
            let n: usize = args.get(2).map_or(262_144, |a| a.parse().unwrap());
            let like: Vec<String> = (0..n).map(|_| "x".repeat(12)).collect();
            let names = random_names(&like, 0x9E37_79B9_7F4A_7C15);
            let d = doc(&names);
            for round in 0..3 {
                let c = best(3, || ooxml_core::XmlDocument::parse(&d).unwrap());
                let s = best(3, || compact::parse_opts::<SipState>(&d, 256, false).unwrap());
                let f = best(3, || compact::parse_with::<SipState>(&d, 256).unwrap());
                println!("round {round}: {n} distinct names: core {c:?} | std {s:?} | std+front {f:?} ({:.2}x core)", f.as_secs_f64() / c.as_secs_f64());
            }
        }
        _ => {
            let v = largest_document_xml();
            println!("largest fixture word/document.xml: {:.1} MiB", v.len() as f64 / 1_048_576.0);
            for round in 0..5 {
                let c = best(15, || ooxml_core::XmlDocument::parse(&v).unwrap());
                let f = best(15, || compact::parse_opts::<FnvState>(&v, 256, false).unwrap());
                let s = best(15, || compact::parse_opts::<SipState>(&v, 256, false).unwrap());
                let sf = best(15, || compact::parse_with::<SipState>(&v, 256).unwrap());
                println!("round {round}: core {c:?} | FNV {f:?} | std {s:?} | std+front {sf:?}");
            }
        }
    }
}
