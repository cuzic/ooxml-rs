//! Names share storage within a parse. The direct-mapped cache filters
//! repeated names before they reach the randomly keyed table.

use std::collections::HashMap;
use std::hash::RandomState;
use std::sync::Arc;

use crate::name::XName;

const FRONT: usize = 256;
const INTERN_CAP: usize = 16_384;

type FrontCache<'s> = [Option<(Key<'s>, XName)>; FRONT];

#[derive(Clone, Copy, Hash)]
struct Key<'s> {
    prefix: Option<&'s str>,
    local: &'s str,
    // Scope and cached XNames keep the URI allocation alive.
    ns: usize,
}

impl Key<'_> {
    fn same(self, other: Self) -> bool {
        self.ns == other.ns && self.local == other.local && self.prefix == other.prefix
    }

    fn slot(self) -> usize {
        let local = self.local.as_bytes();
        let mix = local.len().wrapping_mul(31)
            ^ usize::from(*local.last().unwrap_or(&0))
            ^ (usize::from(*local.first().unwrap_or(&0)) << 3)
            ^ self.prefix.map_or(0, |p| p.len().wrapping_mul(7))
            ^ (self.ns >> 4);
        mix & (FRONT - 1)
    }
}

impl PartialEq for Key<'_> {
    fn eq(&self, other: &Self) -> bool {
        #[cfg(test)]
        EQ_CALLS.with(|calls| calls.set(calls.get() + 1));
        self.same(*other)
    }
}

impl Eq for Key<'_> {}

pub(super) struct Interner<'s> {
    map: HashMap<Key<'s>, XName, RandomState>,
    front: Box<FrontCache<'s>>,
}

impl Default for Interner<'_> {
    fn default() -> Self {
        Self {
            map: HashMap::new(),
            front: Box::new([const { None }; FRONT]),
        }
    }
}

impl<'s> Interner<'s> {
    pub(super) fn get(&mut self, ns: &Arc<str>, local: &'s str, prefix: Option<&'s str>) -> XName {
        let key = Key {
            prefix,
            local,
            ns: Arc::as_ptr(ns).cast::<u8>().addr(),
        };
        let slot = &mut self.front[key.slot()];
        if let Some((cached, name)) = slot
            && cached.same(key)
        {
            return name.clone();
        }

        let name = if let Some(name) = self.map.get(&key) {
            name.clone()
        } else {
            let name = XName::new(ns.clone(), local, prefix);
            if self.map.len() < INTERN_CAP {
                self.map.insert(key, name.clone());
            }
            name
        };
        *slot = Some((key, name.clone()));
        name
    }
}

#[cfg(test)]
thread_local! {
    static EQ_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_checks_complete_key_and_reuses_interned_names() {
        let ns: Arc<str> = Arc::from("urn:test");
        let mut interner = Interner::default();
        // Equal length and end bytes put these two ordinary names in one slot.
        let first = interner.get(&ns, "one", Some("p"));
        let other = interner.get(&ns, "ore", Some("p"));
        assert_eq!(other.local(), "ore");
        let again = interner.get(&ns, "one", Some("p"));
        assert!(std::ptr::eq(first.local(), again.local()));

        let other_prefix = interner.get(&ns, "one", Some("q"));
        assert_eq!(other_prefix.prefix(), Some("q"));
        let other_ns: Arc<str> = Arc::from("urn:other");
        assert_eq!(interner.get(&other_ns, "one", Some("p")).ns(), "urn:other");
    }

    #[test]
    fn full_cache_preserves_names_and_bounds_table_growth() {
        let names: Vec<_> = (0..262_144).map(|i| format!("n{i:06x}z")).collect();
        let ns: Arc<str> = Arc::from("urn:test");
        let mut interner = Interner::default();
        EQ_CALLS.with(|calls| calls.set(0));
        for local in &names {
            let name = interner.get(&ns, local, Some("p"));
            assert_eq!(name.local(), local);
            assert_eq!(name.ns(), &*ns);
            assert_eq!(name.prefix(), Some("p"));
        }
        assert_eq!(interner.map.len(), INTERN_CAP);
        EQ_CALLS.with(|calls| assert!(calls.get() <= names.len()));
    }
}
