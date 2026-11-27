//! Ids and the per-sheet, per-axis permutation vectors (ADR-0024 §5.1, §5.2).
//!
//! A row or column id names a line of the sheet for the whole session. The
//! vector of an axis lists, position by position, the id that sits there; an
//! insert or delete rewrites it, and a key made of ids follows its line.
//! Updates are O(R) in the number of runs (a `Vec` of runs, not §5.2's
//! counted B-tree; same API, decision A7).

use std::marker::PhantomData;

/// A sheet by its `sheet/@sheetId` (`Base`) or minted in the session (`New`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum SheetId {
    Base(u32),
    New(u64),
}

/// A row: `Base(r)` is the 1-based row of the first Base; `New` is minted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum RowId {
    Base(u32),
    New(u64),
}

/// A column: `Base(c)` is the 1-based column; `New` is minted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum ColId {
    Base(u32),
    New(u64),
}

/// The structural epoch of a Base; always `Epoch(0)` in J0.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct Epoch(pub(crate) u32);

/// Mints ids: monotone, one per Journal, never reset.
#[derive(Clone, Debug, Default)]
pub(crate) struct IdCounter {
    next: u64,
}

impl IdCounter {
    pub(crate) fn new() -> Self {
        IdCounter { next: 0 }
    }

    /// Takes `n` ids and returns the first.
    pub(crate) fn take(&mut self, n: u64) -> u64 {
        let first = self.next;
        self.next += n;
        first
    }
}

/// An id of a line axis.
pub(crate) trait AxisId: Copy + Eq + std::hash::Hash {
    fn base(n: u32) -> Self;
    fn new(n: u64) -> Self;
    fn as_base(self) -> Option<u32>;
    fn as_new(self) -> Option<u64>;
}

macro_rules! axis_id {
    ($t:ident) => {
        impl AxisId for $t {
            fn base(n: u32) -> Self {
                $t::Base(n)
            }
            fn new(n: u64) -> Self {
                $t::New(n)
            }
            fn as_base(self) -> Option<u32> {
                if let $t::Base(n) = self {
                    Some(n)
                } else {
                    None
                }
            }
            fn as_new(self) -> Option<u64> {
                if let $t::New(n) = self { Some(n) } else { None }
            }
        }
    };
}
axis_id!(RowId);
axis_id!(ColId);

/// Consecutive ids.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Run {
    Base { first: u32, len: u32 },
    New { first: u64, len: u32 },
}

impl Run {
    fn len(self) -> u32 {
        match self {
            Run::Base { len, .. } | Run::New { len, .. } => len,
        }
    }

    /// The first `at` ids and the rest.
    fn split(self, at: u32) -> (Run, Run) {
        match self {
            Run::Base { first, len } => (
                Run::Base { first, len: at },
                Run::Base {
                    first: first + at,
                    len: len - at,
                },
            ),
            Run::New { first, len } => (
                Run::New { first, len: at },
                Run::New {
                    first: first + u64::from(at),
                    len: len - at,
                },
            ),
        }
    }
}

/// The ids of one axis of one sheet, by position.
#[derive(Clone, Debug)]
pub(crate) struct AxisVec<I: AxisId> {
    runs: Vec<Run>,
    /// `starts[i]` = the position of the first id of `runs[i]`.
    starts: Vec<u32>,
    len: u32,
    _id: PhantomData<I>,
}

impl<I: AxisId> AxisVec<I> {
    /// `Base(1)..=Base(len)`.
    pub(crate) fn base(len: u32) -> Self {
        let mut v = AxisVec {
            runs: vec![Run::Base { first: 1, len }],
            starts: Vec::new(),
            len,
            _id: PhantomData,
        };
        v.restart();
        v
    }

    pub(crate) fn len(&self) -> u32 {
        self.len
    }

    pub(crate) fn run_count(&self) -> usize {
        self.runs.len()
    }

    fn restart(&mut self) {
        self.runs.retain(|r| r.len() > 0);
        self.starts.clear();
        let mut at = 0;
        for r in &self.runs {
            self.starts.push(at);
            at += r.len();
        }
        debug_assert_eq!(at, self.len);
    }

    /// The id at the 0-based `index` (`index < len`).
    pub(crate) fn id_at(&self, index: u32) -> I {
        debug_assert!(index < self.len);
        let i = self.starts.partition_point(|&s| s <= index) - 1;
        let off = index - self.starts[i];
        match self.runs[i] {
            Run::Base { first, .. } => I::base(first + off),
            Run::New { first, .. } => I::new(first + u64::from(off)),
        }
    }

    /// The position of `id`; `None` if it is not on the axis (dead, or never issued).
    pub(crate) fn index_of(&self, id: I) -> Option<u32> {
        for (r, &start) in self.runs.iter().zip(&self.starts) {
            match *r {
                Run::Base { first, len } => {
                    if let Some(n) = id.as_base()
                        && (first..first + len).contains(&n)
                    {
                        return Some(start + (n - first));
                    }
                }
                Run::New { first, len } => {
                    if let Some(n) = id.as_new()
                        && (first..first + u64::from(len)).contains(&n)
                    {
                        return Some(start + u32::try_from(n - first).expect("below `len`"));
                    }
                }
            }
        }
        None
    }

    /// Makes `at` a run boundary and returns the index of the run that starts there.
    fn cut(&mut self, at: u32) -> usize {
        let i = self.starts.partition_point(|&s| s < at);
        if i < self.starts.len() && self.starts[i] == at {
            return i;
        }
        if i == self.starts.len() && at == self.len {
            return i;
        }
        // `at` is inside runs[i - 1]
        let (a, b) = self.runs[i - 1].split(at - self.starts[i - 1]);
        self.runs[i - 1] = a;
        self.runs.insert(i, b);
        self.starts.insert(i, at);
        i
    }

    /// Removes the positions `[at, at + n)` and returns the runs that were there.
    fn cut_out(&mut self, at: u32, n: u32) -> Vec<Run> {
        let a = self.cut(at);
        let b = self.cut(at + n);
        let dead: Vec<Run> = self.runs.drain(a..b).collect();
        self.len -= n;
        self.restart();
        dead
    }

    fn splice_in(&mut self, at: u32, run: Run) {
        let i = self.cut(at);
        self.runs.insert(i, run);
        self.len += run.len();
        self.restart();
    }

    fn ok(&self, at: u32, n: u32) -> bool {
        let ok = at <= self.len && n <= self.len - at;
        debug_assert!(
            ok,
            "structural caller must validate: at={at} n={n} len={}",
            self.len
        );
        ok
    }

    /// Inserts `n` new ids at `at`; the last `n` ids are pushed off the end
    /// and returned (they die).
    pub(crate) fn insert(&mut self, at: u32, n: u32, ids: &mut IdCounter) -> Vec<Run> {
        if !self.ok(at, n) || n == 0 {
            return Vec::new();
        }
        self.splice_in(
            at,
            Run::New {
                first: ids.take(u64::from(n)),
                len: n,
            },
        );
        self.cut_out(self.len - n, n)
    }

    /// Kills the ids at `[at, at + n)` and appends `n` new ids at the end.
    pub(crate) fn delete(&mut self, at: u32, n: u32, ids: &mut IdCounter) -> Vec<Run> {
        if !self.ok(at, n) || n == 0 {
            return Vec::new();
        }
        let dead = self.cut_out(at, n);
        self.splice_in(
            self.len,
            Run::New {
                first: ids.take(u64::from(n)),
                len: n,
            },
        );
        dead
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::reference::{MAX_COLS, MAX_ROWS};

    /// The obvious model: a `Vec` of ids.
    struct Naive {
        ids: Vec<RowId>,
        counter: IdCounter,
    }

    impl Naive {
        fn new(len: u32) -> Self {
            Naive {
                ids: (1..=len).map(RowId::Base).collect(),
                counter: IdCounter::new(),
            }
        }

        fn fresh(&mut self, n: u32) -> Vec<RowId> {
            let first = self.counter.take(u64::from(n));
            (0..u64::from(n)).map(|i| RowId::New(first + i)).collect()
        }

        fn insert(&mut self, at: u32, n: u32) -> Vec<RowId> {
            let new = self.fresh(n);
            let at = at as usize;
            self.ids.splice(at..at, new);
            self.ids.split_off(self.ids.len() - n as usize)
        }

        fn delete(&mut self, at: u32, n: u32) -> Vec<RowId> {
            let dead: Vec<RowId> = self.ids.drain(at as usize..(at + n) as usize).collect();
            let new = self.fresh(n);
            self.ids.extend(new);
            dead
        }
    }

    fn ids_of(runs: &[Run]) -> Vec<RowId> {
        runs.iter()
            .flat_map(|r| -> Vec<RowId> {
                match *r {
                    Run::Base { first, len } => (first..first + len).map(RowId::Base).collect(),
                    Run::New { first, len } => {
                        (0..u64::from(len)).map(|i| RowId::New(first + i)).collect()
                    }
                }
            })
            .collect()
    }

    fn assert_same(v: &AxisVec<RowId>, naive: &Naive) {
        assert_eq!(v.len() as usize, naive.ids.len());
        for (i, id) in naive.ids.iter().enumerate() {
            assert_eq!(v.id_at(i as u32), *id, "id_at({i})");
            assert_eq!(v.index_of(*id), Some(i as u32), "index_of({id:?})");
        }
        let mut seen = std::collections::HashSet::new();
        assert!(
            naive.ids.iter().all(|id| seen.insert(*id)),
            "ids are unique"
        );
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 256, failure_persistence: None, ..ProptestConfig::default() })]

        #[test]
        fn axis_vec_agrees_with_a_naive_vec(ops in prop::collection::vec((any::<bool>(), 0u32..=64, 0u32..=64), 1..24)) {
            let mut v = AxisVec::<RowId>::base(64);
            let mut naive = Naive::new(64);
            let mut counter = IdCounter::new();
            let mut all_dead: Vec<RowId> = Vec::new();
            for (insert, at, n) in ops {
                let at = at.min(64);
                let n = n.min(64 - at);
                let (dead, want) = if insert { (v.insert(at, n, &mut counter), naive.insert(at, n)) } else { (v.delete(at, n, &mut counter), naive.delete(at, n)) };
                prop_assert_eq!(ids_of(&dead), want.clone());
                all_dead.extend(want);
                assert_same(&v, &naive);
                // dead ids are not on the axis
                for d in &all_dead {
                    prop_assert_eq!(v.index_of(*d), None);
                }
            }
        }

        #[test]
        fn insert_then_delete_at_the_same_place_keeps_the_survivors(at in 0u32..=64, n in 0u32..=64) {
            let n = n.min(64 - at);
            let mut v = AxisVec::<RowId>::base(64);
            let before: Vec<RowId> = (0..64).map(|i| v.id_at(i)).collect();
            let mut ids = IdCounter::new();
            v.insert(at, n, &mut ids);
            v.delete(at, n, &mut ids);
            for i in 0..64 - n {
                prop_assert_eq!(v.id_at(i), before[i as usize]);
            }
            for i in 64 - n..64 {
                prop_assert!(v.id_at(i).as_new().is_some());
            }
        }
    }

    #[test]
    fn an_insert_at_the_top_of_a_full_axis_kills_the_last_row() {
        let mut ids = IdCounter::new();
        let mut v = AxisVec::<RowId>::base(MAX_ROWS);
        assert_eq!(v.run_count(), 1);
        let dead = v.insert(0, 1, &mut ids);
        assert_eq!(
            dead,
            vec![Run::Base {
                first: MAX_ROWS,
                len: 1
            }]
        );
        assert_eq!(v.len(), MAX_ROWS);
        assert_eq!(v.id_at(0), RowId::New(0));
        assert_eq!(v.id_at(1), RowId::Base(1));
        assert_eq!(v.index_of(RowId::Base(MAX_ROWS)), None);
        assert_eq!(v.index_of(RowId::Base(MAX_ROWS - 1)), Some(MAX_ROWS - 1));
        assert_eq!(v.run_count(), 2);
    }

    #[test]
    fn a_delete_appends_new_ids_at_the_end() {
        let mut ids = IdCounter::new();
        let mut v = AxisVec::<ColId>::base(MAX_COLS);
        let dead = v.delete(2, 3, &mut ids);
        assert_eq!(dead, vec![Run::Base { first: 3, len: 3 }]);
        assert_eq!(v.id_at(2), ColId::Base(6));
        assert_eq!(v.id_at(MAX_COLS - 3), ColId::New(0));
        assert_eq!(v.id_at(MAX_COLS - 1), ColId::New(2));
        assert_eq!(v.index_of(ColId::Base(3)), None);
        // never reused: the counter moved on
        let again = v.insert(0, 1, &mut ids);
        assert_eq!(again, vec![Run::New { first: 2, len: 1 }]);
        assert_eq!(v.id_at(0), ColId::New(3));
    }
}
