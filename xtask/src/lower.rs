//! Slot lowering, ADR-0002 §4.1 (revision 5). A port of
//! `spikes/adr-0002/py/slottable.py`'s `lower()`, extended with `max_total`.

use std::collections::{BTreeMap, BTreeSet};

use crate::sdk::{Particle, max_occurs, optional};
use crate::xsd::Xsd;

/// `None` = unbounded.
pub type Lim = Option<u32>;

pub const ANY: &str = "*ANY*";

fn mul(a: Lim, b: Lim) -> Lim {
    match (a, b) {
        (Some(a), Some(b)) => a.checked_mul(b),
        _ => None,
    }
}

fn max_lim(a: Lim, b: Lim) -> Lim {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.max(b)),
        _ => None,
    }
}

/// A lowered slot: element QNames as written in the SDK data (`x:c`), each
/// with its own limit; `*ANY*` stands for a wildcard.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LSlot {
    /// Insertion-ordered (name, max_each); a later duplicate overwrites the limit.
    pub names: Vec<(String, Lim)>,
    pub total: Lim,
    /// SDK `Namespace` of a merged `Any` particle (`None` = `##any`).
    pub any_ns: Option<String>,
    /// SDK Names (`x:CT_Cell/x:c`) of the element particles merged here.
    pub sdk_names: BTreeSet<String>,
}

impl LSlot {
    fn update(&mut self, other: &LSlot) {
        for (n, l) in &other.names {
            match self.names.iter_mut().find(|(m, _)| m == n) {
                Some(e) => e.1 = *l,
                None => self.names.push((n.clone(), *l)),
            }
        }
        if other.any_ns.is_some() {
            self.any_ns.clone_from(&other.any_ns);
        }
        self.sdk_names.extend(other.sdk_names.iter().cloned());
    }
    fn scale(&mut self, r: Lim) {
        for (_, l) in &mut self.names {
            *l = mul(*l, r);
        }
        self.total = mul(self.total, r);
    }
    pub fn max_each(&self) -> Lim {
        self.names.iter().map(|(_, l)| *l).reduce(max_lim).unwrap_or(Some(0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Unsupported {
    RepeatingMultiSlotGroup,
    MultiSlotAlternative,
    ExclusiveChoice,
    UnknownKind,
}

impl Unsupported {
    pub fn text(self) -> &'static str {
        match self {
            Unsupported::RepeatingMultiSlotGroup => "repeating multi-slot group",
            Unsupported::MultiSlotAlternative => "multi-slot alternative",
            Unsupported::ExclusiveChoice => "exclusive choice of repeating alternatives",
            Unsupported::UnknownKind => "unknown particle kind",
        }
    }
}

fn elem_qname(sdk_name: &str) -> &str {
    sdk_name.rsplit('/').next().unwrap_or(sdk_name)
}

/// `outer`: an enclosing particle (up to the slot boundary) repeats without bound.
pub fn lower(x: &Particle, outer: bool) -> Result<Vec<LSlot>, Unsupported> {
    match x.kind.as_deref().unwrap_or("Element") {
        "Element" => {
            let full = x.name.clone().unwrap_or_default();
            let m = max_occurs(x);
            Ok(vec![LSlot {
                names: vec![(elem_qname(&full).to_owned(), m)],
                total: m,
                any_ns: None,
                sdk_names: BTreeSet::from([full]),
            }])
        }
        "Any" => {
            let m = max_occurs(x);
            Ok(vec![LSlot {
                names: vec![(ANY.to_owned(), m)],
                total: m,
                any_ns: Some(x.namespace.clone().unwrap_or_else(|| "##any".into())),
                sdk_names: BTreeSet::new(),
            }])
        }
        "All" => {
            let mut s = LSlot::default();
            for i in &x.items {
                for sl in lower(i, outer)? {
                    let mut one = sl.clone();
                    for n in &mut one.names {
                        n.1 = Some(1);
                    }
                    s.update(&one);
                }
            }
            s.total = None;
            Ok(vec![s])
        }
        "Sequence" | "Group" => {
            let r = max_occurs(x);
            let mut out = Vec::new();
            for i in &x.items {
                out.extend(lower(i, outer || r.is_none())?);
            }
            if out.len() > 1 && (outer || r != Some(1)) {
                // (a? b? …)* == (a | b | …)*: a repeated sequence of optional parts is a free slot
                if (outer || r.is_none()) && matches!(r, None | Some(1)) && x.items.iter().all(optional) {
                    let mut u = LSlot::default();
                    for sl in &out {
                        u.update(sl);
                    }
                    for n in &mut u.names {
                        n.1 = None;
                    }
                    u.total = None;
                    return Ok(vec![u]);
                }
                return Err(Unsupported::RepeatingMultiSlotGroup);
            }
            if r != Some(1) {
                for s in &mut out {
                    s.scale(r);
                }
            }
            Ok(out)
        }
        "Choice" => {
            let rep = max_occurs(x);
            let mut s = LSlot { total: Some(0), ..LSlot::default() };
            for i in &x.items {
                let b = lower(i, outer || rep.is_none())?;
                if b.len() > 1 {
                    return Err(Unsupported::MultiSlotAlternative);
                }
                if let Some(b) = b.first() {
                    s.update(b);
                    s.total = max_lim(s.total, b.total);
                }
            }
            if rep == Some(1) && !outer && s.names.len() > 1 && s.names.iter().any(|(_, l)| l.is_none_or(|v| v > 1)) {
                return Err(Unsupported::ExclusiveChoice);
            }
            if rep != Some(1) {
                s.scale(rep);
            }
            Ok(vec![s])
        }
        _ => Err(Unsupported::UnknownKind),
    }
}

/// Every element QName (`x:c`) reachable in a particle, sorted.
pub fn all_names(x: &Particle, out: &mut BTreeSet<String>) {
    if x.kind.as_deref().unwrap_or("Element") == "Element"
        && let Some(n) = &x.name
    {
        out.insert(elem_qname(n).to_owned());
    }
    for i in &x.items {
        all_names(i, out);
    }
}

/// Every element SDK Name reachable in a particle.
pub fn all_sdk_names(x: &Particle, out: &mut BTreeSet<String>) {
    if x.kind.as_deref().unwrap_or("Element") == "Element"
        && let Some(n) = &x.name
    {
        out.insert(n.clone());
    }
    for i in &x.items {
        all_sdk_names(i, out);
    }
}

/// QNames in more than one slot (supported, but need an explicit slot).
pub fn has_ambiguous(slots: &[LSlot]) -> bool {
    let mut seen = BTreeSet::new();
    slots.iter().flat_map(|s| s.names.iter()).filter(|(q, _)| q != ANY).any(|(q, _)| !seen.insert(q.clone()))
}

/// SDK-vs-ECMA order check (`slottable.py`): does the SDK lowering permit an
/// order the XSD particle forbids? Returns the first offending pair.
pub fn ecma_disagreement(
    sdk_name: &str,
    slots: &[LSlot],
    xsd: &Xsd,
    prefixes: &BTreeMap<String, String>,
) -> Option<(String, String)> {
    let typ = sdk_name.split('/').next()?;
    let (p, ctn) = typ.split_once(':')?;
    let ns = prefixes.get(p)?;
    let xsl = xsd.ct_slots(ns, ctn)?;
    let local = |q: &str| q.rsplit(':').next().unwrap_or(q).to_owned();
    let mut xr: BTreeMap<String, usize> = BTreeMap::new();
    for (i, s) in xsl.iter().enumerate() {
        for q in s {
            xr.entry(local(q)).or_insert(i);
        }
    }
    // dict comprehension: insertion order of first occurrence, value of last
    let mut sr_order: Vec<String> = Vec::new();
    let mut sr: BTreeMap<String, usize> = BTreeMap::new();
    for (i, s) in slots.iter().enumerate() {
        for (q, _) in &s.names {
            if q.split(':').next() == Some(p) {
                let l = local(q);
                if !sr.contains_key(&l) {
                    sr_order.push(l.clone());
                }
                sr.insert(l, i);
            }
        }
    }
    let mut count: BTreeMap<String, usize> = BTreeMap::new();
    for s in slots {
        for (q, _) in &s.names {
            *count.entry(local(q)).or_default() += 1;
        }
    }
    let mut dup: BTreeSet<String> = count.into_iter().filter(|(_, c)| *c > 1).map(|(q, _)| q).collect();
    let mut xcount: BTreeMap<String, usize> = BTreeMap::new();
    for s in &xsl {
        for q in s {
            *xcount.entry(local(q)).or_default() += 1;
        }
    }
    dup.extend(xcount.into_iter().filter(|(_, c)| *c > 1).map(|(q, _)| q));
    let common: Vec<&String> =
        sr_order.iter().filter(|q| xr.contains_key(*q) && q.as_str() != ANY && !dup.contains(*q)).collect();
    for a in &common {
        for b in &common {
            if a == b {
                continue;
            }
            let (sa, sb, xa, xb) = (sr[*a] as i64, sr[*b] as i64, xr[*a] as i64, xr[*b] as i64);
            if (sa == sb && xa != xb) || (sa - sb) * (xa - xb) < 0 {
                return Some(((*a).clone(), (*b).clone()));
            }
        }
    }
    None
}
