//! The dependency graph (ADR-0003 §3.2): precedents of every formula cell,
//! indexed by row bucket so that a seed finds its readers without scanning
//! every formula, and the fixed-point search for dependents.

use std::collections::{BTreeMap, HashMap};

use crate::census::{Census, FCell, FKind};
use crate::formula::parse::{Expr, parse};
use crate::formula::precedents::{Env, Prec, Rect};
use crate::reference::{CellRange, MAX_COLS, MAX_ROWS};

const BUCKET_ROWS: u32 = 64;
/// Rectangles spanning more buckets than this go to the `wide` list.
const MAX_BUCKETS: u32 = 64;

#[derive(Debug, Default)]
pub(crate) struct Graph {
    /// Formula ids whose precedents are `Everything`.
    everything: Vec<usize>,
    /// (sheet, row bucket) -> (rectangle, formula id).
    buckets: BTreeMap<(usize, u32), Vec<(Rect, usize)>>,
    wide: Vec<(Rect, usize)>,
}

/// The result of a dependents search.
#[derive(Debug, Default)]
pub(crate) struct Dependents {
    /// Formula cells (ids), in discovery order.
    pub(crate) formulas: Vec<usize>,
    /// Result ranges (array, data table, current spill) of dependent anchors,
    /// with the anchor's id.
    pub(crate) results: Vec<(usize, Rect)>,
}

fn rect_of(sheet: usize, r: CellRange) -> Rect {
    Rect { sheet, r1: r.first.row, c1: r.first.col, r2: r.last.row, c2: r.last.col }
}

/// Precedents of one formula cell (§2.3: `ca`, data tables and `#SPILL!`
/// give `Everything`; shared members are their master's tree translated).
pub(crate) fn precedents_of(
    env: &Env,
    census: &Census,
    c: &FCell,
    master_of: &dyn Fn(usize, u32) -> Option<usize>,
    masters: &mut HashMap<usize, Option<Expr>>,
) -> Prec {
    if c.volatile || c.spill_error || c.kind == FKind::DataTable {
        return Prec::Everything;
    }
    match c.kind {
        FKind::Shared { si, master: false } => {
            let Some(mid) = master_of(c.sheet, si) else { return Prec::Everything };
            let m = census.get(mid).expect("alive");
            let expr = masters.entry(mid).or_insert_with(|| parse(&m.text).ok());
            match expr {
                Some(e) => env.precedents(
                    e,
                    c.sheet,
                    i64::from(c.at.row) - i64::from(m.at.row),
                    i64::from(c.at.col) - i64::from(m.at.col),
                ),
                None => Prec::Everything,
            }
        }
        _ => match parse(&c.text) {
            Ok(e) => env.precedents(&e, c.sheet, 0, 0),
            Err(_) => Prec::Everything,
        },
    }
}

impl Graph {
    pub(crate) fn build(env: &Env, census: &Census) -> Self {
        let mut g = Graph::default();
        let mut masters = HashMap::new();
        let by_si: HashMap<(usize, u32), usize> = census
            .alive()
            .filter_map(|(fid, c)| match c.kind {
                FKind::Shared { si, master: true } => Some(((c.sheet, si), fid)),
                _ => None,
            })
            .collect();
        let lookup = |sheet: usize, si: u32| by_si.get(&(sheet, si)).copied();
        for (fid, c) in census.alive() {
            let p = precedents_of(env, census, c, &lookup, &mut masters);
            g.add(fid, p);
        }
        g
    }

    pub(crate) fn add(&mut self, fid: usize, p: Prec) {
        match p {
            Prec::Everything => self.everything.push(fid),
            Prec::Cells(rects) => {
                for r in rects {
                    let (b1, b2) = (r.r1 / BUCKET_ROWS, r.r2 / BUCKET_ROWS);
                    if b2 - b1 >= MAX_BUCKETS {
                        self.wide.push((r, fid));
                    } else {
                        for b in b1..=b2 {
                            self.buckets.entry((r.sheet, b)).or_default().push((r, fid));
                        }
                    }
                }
            }
        }
    }

    /// Every formula (transitively) reading a seed, plus result ranges.
    /// Removed formulas (dead ids) are skipped.
    pub(crate) fn dependents(&self, census: &Census, seeds: Vec<Rect>) -> Dependents {
        let mut seen = vec![false; census.cells.len()];
        let mut out = Dependents::default();
        let mut queue = seeds;
        let mark = |fid: usize, seen: &mut Vec<bool>, out: &mut Dependents, queue: &mut Vec<Rect>| {
            if seen[fid] {
                return;
            }
            let Some(c) = census.get(fid) else { return };
            seen[fid] = true;
            out.formulas.push(fid);
            queue.push(Rect::cell(c.sheet, c.at.row, c.at.col));
            if let Some(r) = c.result_range() {
                let r = rect_of(c.sheet, r);
                out.results.push((fid, r));
                queue.push(r);
            }
            if c.dynamic || c.spill_error {
                // R2-M1 (1): the potential spill area, to the sheet's end.
                if let Some(r) = c.range {
                    out.results.push((fid, rect_of(c.sheet, r)));
                }
                queue.push(Rect { sheet: c.sheet, r1: c.at.row, c1: c.at.col, r2: MAX_ROWS - 1, c2: MAX_COLS - 1 });
            }
        };
        for &fid in &self.everything {
            mark(fid, &mut seen, &mut out, &mut queue);
        }
        while let Some(seed) = queue.pop() {
            let (b1, b2) = (seed.r1 / BUCKET_ROWS, seed.r2 / BUCKET_ROWS);
            let mut hits = Vec::new();
            for (_, v) in self.buckets.range((seed.sheet, b1)..=(seed.sheet, b2)) {
                hits.extend(v.iter().filter(|(r, fid)| !seen[*fid] && r.intersects(&seed)).map(|(_, f)| *f));
            }
            hits.extend(self.wide.iter().filter(|(r, fid)| !seen[*fid] && r.intersects(&seed)).map(|(_, f)| *f));
            for fid in hits {
                mark(fid, &mut seen, &mut out, &mut queue);
            }
        }
        out
    }
}
