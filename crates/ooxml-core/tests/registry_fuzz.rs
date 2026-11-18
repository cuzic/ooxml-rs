//! Randomized registry plans against a naive model (PR #38 review rounds
//! 2–3; the reviewer's model with `set_relationship_target`, session edits
//! before the plan, and re-parsed `read_part` content staged). Checked per
//! run: no new dangling relationship, no `r:id` without a relationship,
//! content types, untouched parts byte-identical (L1), `Numbered` names and
//! relationship `Id`s against the model, retargets kept (the last one wins),
//! a refused step leaves the plan as it was, and one batch plan equals the
//! same steps as one plan each. `FUZZ_ITERS` (default 3000) and `FUZZ_FROM`
//! choose the seeds.
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read, Write};
use ooxml_core::{Error, Package, PartNameRequest, RegistryPlan, Target, XmlDocument};
use ooxml_core::opc::{Relationships, TargetMode, resolve_target, rels_part_name};
use zip::ZipArchive;
use zip::write::SimpleFileOptions;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 { self.0 ^= self.0 << 13; self.0 ^= self.0 >> 7; self.0 ^= self.0 << 17; self.0 }
    fn n(&mut self, k: usize) -> usize { (self.next() % k as u64) as usize }
    fn p(&mut self, pct: u64) -> bool { self.next() % 100 < pct }
}
const RT: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/x";
const RT2: &str = "urn:t:implicit";
const RNS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
fn norm(s: &str) -> String { s.trim_start_matches('/').to_ascii_lowercase() }

fn entries(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let mut ar = ZipArchive::new(Cursor::new(bytes)).unwrap();
    (0..ar.len()).map(|i| { let mut f = ar.by_index(i).unwrap(); let mut v = Vec::new(); f.read_to_end(&mut v).unwrap(); (f.name().unwrap().into_owned(), v) }).collect()
}
fn content(ids: &[String]) -> String {
    let mut s = format!(r#"<s xmlns:r="{RNS}">"#);
    for i in ids { s.push_str(&format!(r#"<x r:id="{i}"/>"#)); }
    s.push_str("</s>"); s
}
fn rid_refs(xml: &[u8]) -> Vec<String> {
    let t = String::from_utf8_lossy(xml);
    t.split("r:id=\"").skip(1).map(|x| x.split('"').next().unwrap().to_owned()).collect()
}

fn gen_pkg(r: &mut Rng) -> Vec<u8> {
    let mut names: Vec<String> = Vec::new();
    for _ in 0..(2 + r.n(5)) {
        let n = match r.n(3) { 0 => format!("p/s{}.xml", 1 + r.n(9)), 1 => format!("p/S{}.xml", 1 + r.n(9)), _ => format!("m/i{}.png", 1 + r.n(5)) };
        if !names.iter().any(|x| norm(x) == norm(&n)) { names.push(n); }
    }
    let mut ct = String::from(r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/>"#);
    if r.p(50) { ct.push_str(r#"<Default Extension="png" ContentType="image/png"/>"#); }
    for n in &names { if r.p(40) || n.ends_with(".png") && !ct.contains("png") { ct.push_str(&format!(r#"<Override PartName="/{n}" ContentType="t/{}"/>"#, r.n(3))); } }
    if r.p(30) { ct.push_str(&format!(r#"<Override PartName="/p/s{}.xml" ContentType="t/left"/>"#, 10 + r.n(5))); }
    ct.push_str("</Types>");
    let mut files: Vec<(String, String)> = vec![("[Content_Types].xml".into(), ct)];
    files.push(("_rels/.rels".into(), format!(r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{RT}" Target="{}"/></Relationships>"#, names[0])));
    for n in &names {
        let xml = n.ends_with(".xml");
        let mut ids = Vec::new(); let mut all_ids: Vec<String> = Vec::new();
        if xml && r.p(70) {
            let mut rels = String::from(r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#);
            for k in 0..(1 + r.n(3)) {
                let id = if r.p(80) { format!("rId{}", 1 + k * (1 + r.n(3))) } else { format!("R{}x", k) };
                if all_ids.contains(&id) { continue; } all_ids.push(id.clone());
                let (t, ty) = match r.n(4) {
                    0 => (format!("p/s{}.xml", 1 + r.n(12)), RT),                // maybe dangling, folder-relative wrong -> dangling
                    1 => { let t = &names[r.n(names.len())]; (format!("../{}", t.to_ascii_uppercase()), RT) }
                    2 => { let t = &names[r.n(names.len())]; (format!("../{t}"), RT2) }
                    _ => ("https://e/x".into(), "EXT"),
                };
                if ty == "EXT" { rels.push_str(&format!(r#"<Relationship Id="{id}" Type="{RT}" Target="{t}" TargetMode="External"/>"#)); }
                else { rels.push_str(&format!(r#"<Relationship Id="{id}" Type="{ty}" Target="{t}"/>"#)); }
                if ty != RT2 || r.p(50) { ids.push(id); } else { ids.push(String::new()); }
            }
            rels.push_str("</Relationships>");
            ids.retain(|i| !i.is_empty());
            if r.p(20) { ids.push(format!("rId{}", 7 + r.n(3))); } // dangling r:id
            files.push((rels_part_name(Some(n)), rels));
        }
        files.push((n.clone(), if xml { content(&ids) } else { "PNG".into() }));
    }
    let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (n, c) in &files { zw.start_file(n.as_str(), SimpleFileOptions::default()).unwrap(); zw.write_all(c.as_bytes()).unwrap(); }
    zw.finish().unwrap().into_inner()
}

/// All (rels, id, resolved target) internal relationships + all r:id refs.
fn snapshot(bytes: &[u8]) -> (BTreeSet<String>, Vec<(String, String, String)>, Vec<(String, String)>, BTreeMap<String, Option<String>>) {
    let pkg = Package::from_bytes(bytes.to_vec()).expect("reopen");
    let names: BTreeSet<String> = pkg.part_names().map(norm).collect();
    let mut rels = Vec::new();
    let mut refs = Vec::new();
    let mut cts = BTreeMap::new();
    for n in pkg.part_names() {
        if n.contains("_rels/") || n == "[Content_Types].xml" { continue; }
        cts.insert(norm(n), pkg.content_type(n).map(str::to_owned));
        let r = pkg.relationships(n).unwrap();
        let mut seen = BTreeSet::new();
        for x in r.iter() {
            assert!(seen.insert(x.id.to_ascii_lowercase()), "duplicate id {} in rels of {n}", x.id);
            if x.target_mode == TargetMode::Internal { rels.push((norm(n), x.id.clone(), norm(&resolve_target(Some(n), &x.target)))); }
        }
        for id in rid_refs(&pkg.read_part(n).unwrap()) {
            if r.by_id(&id).is_none() { refs.push((norm(n), id)); }
        }
    }
    let _ = Relationships::empty(None);
    (names, rels, refs, cts)
}

#[derive(Debug)]
enum Op { Exact(String, String), Num(String, String), Rel(Option<String>, String), RmRel(Option<String>, String), Rm(Vec<String>), Stage(String, Vec<String>), SetCt(String, String), StageCur(String), Retarget(Option<String>, String, String) }

fn pick_ops(r: &mut Rng, pkg: &Package) -> Vec<Op> {
    let parts: Vec<String> = pkg.part_names().filter(|n| !n.contains("_rels") && !n.starts_with('[')).map(str::to_owned).collect();
    let mut created: Vec<String> = Vec::new();
    let mut ops = Vec::new();
    for _ in 0..(1 + r.n(7)) {
        let any = |r: &mut Rng, created: &Vec<String>| -> String {
            if !created.is_empty() && r.p(30) { created[r.n(created.len())].clone() } else { parts[r.n(parts.len())].clone() }
        };
        ops.push(match r.n(10) {
            0 => { let n = format!("p/s{}.xml", 1 + r.n(15)); created.push(n.clone()); Op::Exact(n, if r.p(50) { "application/xml".into() } else { "t/new".into() }) }
            1 => { let n = "p/s{}.xml".to_string(); created.push(format!("p/s{}.xml", 99)); Op::Num(n, if r.p(50) { "t/1".into() } else { "application/xml".into() }) }
            2 => Op::Rel(if r.p(15) { None } else { Some(any(r, &created)) }, any(r, &created)),
            3 => { let s = parts[r.n(parts.len())].clone(); let ids = pkg.relationships(&s).map(|x| x.iter().map(|y| y.id.clone()).collect::<Vec<_>>()).unwrap_or_default(); if ids.is_empty() { continue; } Op::RmRel(Some(s), ids[r.n(ids.len())].clone()) }
            4 | 5 => { let mut v = vec![parts[r.n(parts.len())].clone()]; if r.p(40) { v.push(parts[r.n(parts.len())].clone()); } Op::Rm(v) }
            6 => { let s = parts[r.n(parts.len())].clone(); if !s.ends_with(".xml") { continue; } let mut ids = rid_refs(&pkg.read_part(&s).unwrap()); if !ids.is_empty() { ids.remove(r.n(ids.len())); } Op::Stage(s, ids) }
            7 => Op::SetCt(parts[r.n(parts.len())].clone(), format!("t/{}", r.n(3))),
            8 => {
                let s = parts[r.n(parts.len())].clone();
                // StageCur reads the part as it was before the plan: not after
                // an earlier step removed or re-created that name (the model
                // error the round-3 review found at seed 33933)
                let touched = ops.iter().any(|o| match o {
                    Op::Stage(x, _) | Op::StageCur(x) => *x == s,
                    Op::Rm(v) => v.iter().any(|x| norm(x) == norm(&s)),
                    Op::Exact(x, _) => norm(x) == norm(&s),
                    _ => false,
                });
                if !s.ends_with(".xml") || touched { continue; }
                Op::StageCur(s)
            }
            _ => { let s = if r.p(15) { None } else { Some(parts[r.n(parts.len())].clone()) };
                   let rs = match &s { Some(x) => pkg.relationships(x).unwrap_or_else(|_| Relationships::empty(None)), None => pkg.package_relationships().clone() };
                   let ids: Vec<String> = rs.iter().filter(|y| y.target_mode == TargetMode::Internal).map(|y| y.id.clone()).collect(); if ids.is_empty() { continue; }
                   Op::Retarget(s, ids[r.n(ids.len())].clone(), any(r, &created)) }
        });
    }
    ops
}

fn run(plan: &mut RegistryPlan, pkg: &Package, op: &Op) -> Result<String, Error> {
    match op {
        Op::Exact(n, ct) => plan.add_part(pkg, PartNameRequest::Exact(n.clone()), ct, content(&[]).into_bytes()),
        Op::Num(t, ct) => plan.add_part(pkg, PartNameRequest::Numbered(t.clone()), ct, content(&[]).into_bytes()),
        Op::Rel(s, t) => plan.add_relationship(pkg, s.as_deref(), RT, Target::Part(t.clone())),
        Op::RmRel(s, id) => plan.remove_relationship(pkg, s.as_deref(), id, &[RT, RT2]).map(|_| String::new()),
        Op::Rm(v) => { let v: Vec<&str> = v.iter().map(String::as_str).collect(); plan.remove_parts(pkg, &v, &[RT, RT2]).map(|x| format!("{x:?}")) }
        Op::Stage(s, ids) => plan.stage_part(pkg, s, content(ids).into_bytes()).map(|_| String::new()),
        Op::SetCt(n, ct) => plan.set_content_type(pkg, n, ct).map(|_| String::new()),
        Op::StageCur(s) => { let d = XmlDocument::parse(&pkg.read_part(s).map_err(|e| e)?).map_err(|e| e)?; plan.stage_part(pkg, s, d).map(|_| String::new()) }
        Op::Retarget(s, id, t) => plan.set_relationship_target(pkg, s.as_deref(), id, &format!("/{t}")).map(|_| String::new()),
    }
}

#[test]
fn fuzz_plans() {
    let iters: u64 = std::env::var("FUZZ_ITERS").ok().and_then(|s| s.parse().ok()).unwrap_or(3000);
    let mut stats: BTreeMap<String, usize> = BTreeMap::new();
    let mut fails = 0;
    let from: u64 = std::env::var("FUZZ_FROM").ok().and_then(|s| s.parse().ok()).unwrap_or(1); for seed in from..=iters {
        let mut r = Rng(seed.wrapping_mul(0x9E3779B97F4A7C15) | 1);
        let input = gen_pkg(&mut r);
        let (names0, rels0, refs0, cts0) = snapshot(&input);
        let dangling0: BTreeSet<(String, String, String)> = rels0.iter().filter(|x| !names0.contains(&x.2)).cloned().collect();
        let mut pkg = Package::from_bytes(input.clone()).unwrap();
        let pre: Option<(String, Vec<String>, bool)> = if r.p(35) {
            let xs: Vec<String> = pkg.part_names().filter(|n| n.ends_with(".xml") && !n.contains("_rels") && !n.starts_with('[')).map(str::to_owned).collect();
            if xs.is_empty() { None } else { let s = xs[r.n(xs.len())].clone(); let mut ids = rid_refs(&pkg.read_part(&s).unwrap()); let pm = r.p(50); if !ids.is_empty() { if pm { ids.remove(0); } else { ids.remove(r.n(ids.len())); } } Some((s, ids, pm)) }
        } else { None };
        let do_pre = |p: &mut Package| if let Some((s, ids, pm)) = &pre { if *pm { let d = p.xml_part_mut(s).unwrap(); let mut l = d.lens_mut::<Any>().unwrap(); l.remove_at(0); let _ = ids; } else { p.set_part(s, content(ids).into_bytes()).unwrap(); } };
        do_pre(&mut pkg);
        let ops = pick_ops(&mut r, &pkg);
        let mut retargets: Vec<(String, String, String)> = Vec::new();
        let mut plan = RegistryPlan::new(&pkg);
        let mut log = Vec::new();
        let mut set_ct: BTreeMap<String, String> = BTreeMap::new();
        let mut added: BTreeMap<String, String> = BTreeMap::new();
        // naive model: every name ever seen; every id ever seen per source
        let mut seen_names: BTreeSet<String> = names0.clone();
        for x in &rels0 { seen_names.insert(x.2.clone()); }
        for (n, _) in ooxml_core::Package::from_bytes(input.clone()).unwrap().content_types().overrides() { seen_names.insert(norm(n)); }
        let mut seen_ids: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut staged_now: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut recreated: BTreeSet<String> = BTreeSet::new();
        {
            let p0 = Package::from_bytes(input.clone()).unwrap();
            for n in p0.part_names().map(str::to_owned).collect::<Vec<_>>() { if let Ok(r) = p0.relationships(&n) { for x in r.iter() { seen_ids.entry(norm(&n)).or_default().insert(x.id.to_ascii_lowercase()); } } }
            for x in p0.package_relationships().iter() { seen_ids.entry(String::new()).or_default().insert(x.id.to_ascii_lowercase()); }
        }
        for op in &ops {
            let before = format!("{plan:?}");
            let res = run(&mut plan, &pkg, op);
            match &res {
                Err(Error::RegistryRefused { reason, .. }) => {
                    let k = format!("{reason:?}"); let k = k.split([' ', '(', '{']).next().unwrap().to_owned();
                    *stats.entry(k).or_default() += 1;
                    if format!("{plan:?}") != before { println!("seed {seed}: refused step changed the plan: {op:?}"); fails += 1; }
                }
                Err(e) => { println!("seed {seed}: non-registry error {e}"); fails += 1; }
                Ok(v) => {
                    *stats.entry("Ok".into()).or_default() += 1;
                    match op {
                        Op::Num(..) => {
                            let max = seen_names.iter().filter_map(|n| n.strip_prefix("p/s")?.strip_suffix(".xml")?.parse::<u64>().ok()).max().unwrap_or(0);
                            if norm(v) != format!("p/s{}.xml", max + 1) { println!("seed {seed}: Numbered gave {v}, model p/s{}.xml", max + 1); fails += 1; }
                        }
                        Op::Rel(src, _) => {
                            let key = src.as_deref().map(norm).unwrap_or_default();
                            // M7: rId values in the source (original and staged content)
                            let mut extra: Vec<String> = Vec::new();
                            if let Some(sn) = src { if !recreated.contains(&norm(sn)) { if let Some(real) = pkg.part_names().find(|x| norm(x) == norm(sn)).map(str::to_owned) { if let Ok(b) = pkg.read_part(&real) { extra.extend(rid_refs(&b)); } } } }
                            if let Some(sn) = src { if let Some(ids) = staged_now.get(sn) { extra = ids.clone(); } }
                            let ids = seen_ids.entry(key.clone()).or_default();
                            for x in extra { ids.insert(x.to_ascii_lowercase()); }
                            let max = ids.iter().filter_map(|i| i.strip_prefix("rid")?.parse::<u64>().ok()).max().unwrap_or(0);
                            if v.to_ascii_lowercase() != format!("rid{}", max + 1) { println!("seed {seed}: rel id {v} for {key}, model rId{} ids={ids:?}\n  {}", max + 1, log.join("\n  ")); fails += 1; }
                            ids.insert(v.to_ascii_lowercase());
                        }
                        _ => {}
                    }
                    if let Op::Stage(sn, ids) = op { staged_now.insert(sn.clone(), ids.clone()); }
                    if let Op::StageCur(sn) = op { staged_now.insert(sn.clone(), rid_refs(&pkg.read_part(sn).unwrap())); }
                    if let Op::Retarget(sn, id, t) = op { let k = sn.as_deref().map(norm).unwrap_or_default(); retargets.retain(|x| !(x.0 == k && x.1 == *id)); retargets.push((k, id.clone(), norm(t))); }
                    if let Op::Exact(..) = op { recreated.insert(norm(v)); set_ct.remove(&norm(v)); }
                    match op { Op::Exact(..) | Op::Num(..) => { seen_names.insert(norm(v)); } _ => {} }
                    match op { Op::Exact(_, ct) | Op::Num(_, ct) => { added.insert(norm(v), ct.clone()); } Op::SetCt(n, ct) => { set_ct.insert(norm(n), ct.clone()); } _ => {} }
                }
            }
            log.push(format!("{op:?} -> {res:?}"));
        }
        pkg.apply(plan).unwrap();
        let out = pkg.to_bytes().unwrap();
        // differential: same ops, one plan per op (sequential), when every batch step was Ok
        if log.iter().all(|l| l.contains("-> Ok")) {
            let mut seq = Package::from_bytes(input.clone()).unwrap();
            do_pre(&mut seq);
            let mut seq_ok = true;
            for op in &ops {
                let mut p = RegistryPlan::new(&seq);
                match run(&mut p, &seq, op) { Ok(_) => seq.apply(p).unwrap(), Err(e) => { seq_ok = false; println!("seed {seed}: sequential refused {op:?}: {e}\n  pre={pre:?}\n  {}", log.join("\n  ")); fails += 1; break; } }
            }
            if seq_ok {
                *stats.entry("diff-compared".into()).or_default() += 1;
                let (a, b) = (entries(&out), entries(&seq.to_bytes().unwrap()));
                if a != b { println!("seed {seed}: batch and sequential outputs differ pre={pre:?}\n  {}", log.join("\n  ")); for (k, v) in &a { if b.get(k) != Some(v) { println!("  {k}: batch={:?}\n        seq={:?}", String::from_utf8_lossy(v), b.get(k).map(|x| String::from_utf8_lossy(x).into_owned())); } } for k in b.keys() { if !a.contains_key(k) { println!("  only seq: {k}"); } } fails += 1; }
            }
        }
        let (names1, rels1, refs1, cts1) = match std::panic::catch_unwind(|| snapshot(&out)) { Ok(s) => s, Err(_) => { println!("seed {seed}: reopen failed\n  {}", log.join("\n  ")); fails += 1; continue; } };
        let mut bad = Vec::new();
        for x in &rels1 { if !names1.contains(&x.2) && !dangling0.contains(x) { bad.push(format!("new dangling relationship {x:?}")); } }
        let refs0: BTreeSet<_> = refs0.into_iter().collect();
        // (a part re-added by Exact and then staged holds caller content)
        for x in &refs1 { if !refs0.contains(x) { bad.push(format!("r:id without relationship {x:?}")); } }
        for (n, ct) in &cts1 {
            let want = if let Some(c) = set_ct.get(n) { Some(c.clone()) } else if let Some(c) = added.get(n) { Some(c.clone()) } else { cts0.get(n).cloned().flatten() };
            if *ct != want && !(ct.is_none() && want.is_none()) || (ct.is_none() && want.is_some()) { bad.push(format!("content type of {n}: {ct:?}, want {want:?}")); }
        }
        // L1: original non-registry parts not staged pass through
        let (e0, e1) = (entries(&input), entries(&out));
        for (n, b) in &e0 { if n.contains("_rels") || n.starts_with('[') { continue; } if let Some(b1) = e1.get(n) && b1 != b && pre.as_ref().map_or(true, |p| p.0 != *n) && !ops.iter().any(|o| matches!(o, Op::Stage(s, _) | Op::Exact(s, _) | Op::StageCur(s) if s == n)) { bad.push(format!("{n} changed")); } }
        for (k, id, t) in &retargets { if let Some(x) = rels1.iter().find(|x| x.0 == *k && x.1 == *id) { if x.2 != *t { bad.push(format!("retarget lost {k} {id}: {} want {t}", x.2)); } } }
        if let Some((s, ids, _)) = &pre { if let Some(b1) = e1.get(s) { let got = rid_refs(b1); if !ops.iter().any(|o| matches!(o, Op::Stage(x, _) | Op::Exact(x, _) | Op::StageCur(x) if x == s)) && got != *ids { bad.push(format!("pre-edit of {s} lost: {got:?} want {ids:?}")); } } }
        if !bad.is_empty() { fails += 1; if fails < 12 { println!("seed {seed}: {bad:?}\n  {}", log.join("\n  ")); } }
    }
    println!("stats: {stats:?}\nfailures: {fails}/{iters}");
    assert_eq!(fails, 0);
}

enum Any {}
impl ooxml_core::ElementSpec for Any {
    const QNAME: ooxml_core::QName = ooxml_core::QName::new("", "s");
    const SDK_NAME: &'static str = "test:s";
    const ATTRS: &'static [ooxml_core::AttrSpec] = &[];
    const CONTENT: ooxml_core::Content = ooxml_core::Content::Unsupported(&[]);
}
