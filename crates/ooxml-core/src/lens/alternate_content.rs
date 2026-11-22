//! ADR-0020 slot intervals and occurrence limits for AlternateContent.

use super::{LensMut, declarations, effective};
use crate::name::{MC_ALTERNATE_CONTENT, MC_NS, QName};
use crate::{ElementSpec, Error, RawNode, Result, Slot};

const CHOICE: QName = QName::new(MC_NS, "Choice");
const FALLBACK: QName = QName::new(MC_NS, "Fallback");
const REQUIRES: QName = QName::new("", "Requires");

#[derive(Clone, Copy)]
pub(super) enum RankAssignment {
    Greedy(Option<usize>),
    AllCandidates,
}

#[derive(Clone, Copy)]
pub(super) struct Interval {
    // None represents the rank before the first slot.
    pub(super) lo: Option<usize>,
    pub(super) hi: usize,
    pub(super) same_q: u32,
    pub(super) same_slot: u32,
}

impl Interval {
    pub(super) fn contains(self, slot: usize) -> bool {
        self.lo <= Some(slot) && slot <= self.hi
    }
}

pub(super) fn candidate_interval(candidates: &[usize]) -> Option<Interval> {
    Some(Interval {
        lo: Some(*candidates.first()?),
        hi: *candidates.last()?,
        same_q: 0,
        same_slot: 0,
    })
}

pub(super) struct AlternateRanks<'a> {
    pub(super) root: &'a RawNode,
    pub(super) slots: &'a [Slot],
    pub(super) target: usize,
    pub(super) name: QName,
    pub(super) parent: QName,
}

impl AlternateRanks<'_> {
    pub(super) fn read<E: ElementSpec>(
        &self,
        ac: &RawNode,
        path: &mut Vec<usize>,
        assignment: RankAssignment,
    ) -> Result<Option<Interval>> {
        let mut result: Option<Interval> = None;
        let mut choice = false;
        let mut fallback = false;
        for (index, branch) in ac.indexed_elements() {
            if fallback {
                return Err(Error::UnresolvedSibling(self.parent));
            }
            path.push(index);
            if branch.is(CHOICE) {
                let scope = declarations(self.root, path);
                let Some(requires) = branch.attr(REQUIRES) else {
                    return Err(Error::UnresolvedSibling(self.parent));
                };
                let mut prefixes = requires.split_ascii_whitespace().peekable();
                if prefixes.peek().is_none()
                    || prefixes
                        .any(|prefix| prefix != "xml" && effective(&scope, Some(prefix)).is_none_or(str::is_empty))
                {
                    return Err(Error::UnresolvedSibling(self.parent));
                }
                choice = true;
            } else if branch.is(FALLBACK) && choice {
                fallback = true;
            } else {
                return Err(Error::UnresolvedSibling(self.parent));
            }
            if let Some(r) = self.branch::<E>(branch, path, assignment)? {
                result = Some(match result {
                    None => r,
                    Some(previous) => Interval {
                        lo: previous.lo.min(r.lo),
                        hi: previous.hi.max(r.hi),
                        same_q: previous.same_q.max(r.same_q),
                        same_slot: previous.same_slot.max(r.same_slot),
                    },
                });
            }
            path.pop();
        }
        if !choice {
            return Err(Error::UnresolvedSibling(self.parent));
        }
        Ok(result)
    }

    fn branch<E: ElementSpec>(
        &self,
        branch: &RawNode,
        path: &mut Vec<usize>,
        mut assignment: RankAssignment,
    ) -> Result<Option<Interval>> {
        let mut members = branch.indexed_elements().peekable();
        if members.peek().is_none() {
            return Ok(None);
        }
        let start = match assignment {
            RankAssignment::Greedy(cur) => cur,
            RankAssignment::AllCandidates => None,
        };
        let mut result: Option<Interval> = None;
        let mut leading_unlisted = false;
        for (index, member) in members {
            let interval = if member.is(MC_ALTERNATE_CONTENT) {
                path.push(index);
                let interval = self.read::<E>(member, path, assignment)?;
                path.pop();
                // An empty nested AC supplies no listed or unlisted members.
                interval
            } else {
                let candidates = LensMut::<E>::candidates(self.slots, &member.name);
                if candidates.is_empty() {
                    leading_unlisted |= result.is_none();
                    continue;
                }
                match assignment {
                    RankAssignment::Greedy(cur) => {
                        let rank = candidates
                            .into_iter()
                            .find(|&rank| Some(rank) >= cur)
                            .ok_or(Error::UnresolvedSibling(self.parent))?;
                        Some(Interval {
                            lo: Some(rank),
                            hi: rank,
                            same_q: u32::from(rank == self.target && member.is(self.name)),
                            same_slot: u32::from(rank == self.target),
                        })
                    }
                    RankAssignment::AllCandidates => candidate_interval(&candidates),
                }
            };
            if let Some(r) = interval {
                if let RankAssignment::Greedy(cur) = &mut assignment {
                    *cur = Some(r.hi);
                }
                result = Some(match result {
                    None => Interval {
                        lo: if leading_unlisted { start } else { r.lo },
                        ..r
                    },
                    Some(previous) => Interval {
                        lo: previous.lo.min(r.lo),
                        hi: previous.hi.max(r.hi),
                        same_q: previous.same_q + r.same_q,
                        same_slot: previous.same_slot + r.same_slot,
                    },
                });
            }
        }
        result.map(Some).ok_or(Error::UnresolvedSibling(self.parent))
    }
}
