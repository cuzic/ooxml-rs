//! Journal tests: oracle 1 before save (J0a), the fixed cases, R2 and key
//! white-box tests, oracle 10's append cost.

mod fixed;
pub(crate) mod fixtures;
mod keys;
mod measure;
mod oracle1;
pub(crate) mod poi;
mod r2;

use std::collections::HashMap;
use std::mem::discriminant;

use super::ids::{ColId, RowId};
use super::op::{Api, CellCall, Key, OpInput, OpKind, WriteValue};
use super::project::{first_diff, project};
use super::state::tree;
use super::{CellKey, Journal, JournalError};
use crate::error::Error;
use crate::reference::CellIndex;
use crate::workbook::Workbook;
use crate::{Border, Color, ExcelDateTime, Fill, Font, Format, Side};

pub(crate) fn at(a1: &str) -> CellIndex {
    CellIndex::parse(a1).unwrap()
}

/// A single-cell op on sheet `sheet`.
pub(crate) fn cell(sheet: usize, a1: &str, call: CellCall) -> OpInput {
    OpInput {
        sheet,
        kind: OpKind::Cell { at: at(a1), call },
    }
}

/// A single-cell op on `Sheet1`.
pub(crate) fn c1(a1: &str, call: CellCall) -> OpInput {
    cell(0, a1, call)
}

pub(crate) fn num(a1: &str, v: f64) -> OpInput {
    c1(a1, CellCall::Number(v))
}

pub(crate) fn formula(a1: &str, t: &str) -> OpInput {
    c1(a1, CellCall::Formula(t.into()))
}

pub(crate) fn remove_formula(a1: &str) -> OpInput {
    c1(a1, CellCall::RemoveFormula)
}

pub(crate) fn write_cells(cells: &[(&str, WriteValue)]) -> OpInput {
    OpInput {
        sheet: 0,
        kind: OpKind::WriteCells(cells.iter().map(|(a, w)| (at(a), w.clone())).collect()),
    }
}

pub(crate) fn bold() -> Font {
    Font {
        bold: true,
        ..Font::default()
    }
}

pub(crate) fn italic() -> Font {
    Font {
        italic: true,
        ..Font::default()
    }
}

pub(crate) fn red() -> Fill {
    Fill::Pattern {
        pattern: "solid".into(),
        fg: Some(Color::rgb(0xFF0000)),
        bg: None,
    }
}

pub(crate) fn blue() -> Fill {
    Fill::Pattern {
        pattern: "solid".into(),
        fg: Some(Color::rgb(0x0000FF)),
        bg: None,
    }
}

pub(crate) fn thin() -> Border {
    let s = Some(Side {
        style: "thin".into(),
        color: None,
    });
    Border {
        left: s.clone(),
        right: s.clone(),
        top: s.clone(),
        bottom: s,
        ..Border::default()
    }
}

pub(crate) fn date() -> ExcelDateTime {
    ExcelDateTime::from_ymd(2024, 1, 2).unwrap()
}

pub(crate) fn full_format() -> Format {
    Format::new().set_bold().set_num_format("0.0%")
}

/// A sheet's cell back from its key under identity vectors (J0a never moves a line).
pub(crate) fn index_of_key(j: &Journal, k: &CellKey) -> (usize, CellIndex) {
    let (RowId::Base(r), ColId::Base(c)) = (k.row, k.col) else {
        panic!("J0a keeps identity vectors")
    };
    (
        j.sheet_index_of(k.sheet).expect("a sheet"),
        CellIndex {
            row: r - 1,
            col: c - 1,
        },
    )
}

/// What a refused op must leave alone (oracle 1, step 3).
#[derive(Debug, PartialEq)]
pub(crate) struct JournalState {
    log: usize,
    next_seq: u64,
    keys: HashMap<Key, Vec<u64>>,
    open: usize,
    tracked: Vec<(CellKey, bool)>,
    bytes: Vec<u8>,
}

pub(crate) fn state_of_journal(j: &Journal) -> JournalState {
    JournalState {
        log: j.log().len(),
        next_seq: j.next_seq(),
        keys: j.keys().clone(),
        open: j.open_snapshots(),
        tracked: j.tracked_differs(),
        bytes: j.shadow().to_bytes().unwrap(),
    }
}

/// The sequential reference and the Journal over the same Base, with
/// oracle 1's per-op assertions (no save).
pub(crate) struct Pair {
    pub(crate) seq: Workbook,
    pub(crate) j: Journal,
    pub(crate) applied: Vec<OpInput>,
}

impl Pair {
    pub(crate) fn new(base: &[u8]) -> Pair {
        Pair {
            seq: Workbook::from_bytes(base.to_vec()).unwrap(),
            j: Journal::open(base.to_vec()).unwrap(),
            applied: Vec::new(),
        }
    }

    /// One op on both sides, with (i), the refusal-leaves-nothing check and
    /// the write-back invariant. Returns the sequential result.
    pub(crate) fn step(&mut self, input: OpInput) -> Result<(), Error> {
        let before = state_of_journal(&self.j);
        let rs = super::op::apply_seq(&mut self.seq, &input);
        let rj = self.j.append(Api::Native, input.clone());
        self.applied.push(input.clone());
        match (&rs, &rj) {
            (Ok(()), Ok(_)) => self.check_written_back(&input),
            (Err(es), Err(JournalError::Op(ej))) => {
                assert_eq!(
                    discriminant(es),
                    discriminant(ej),
                    "{input:?}: refused with {es:?} sequentially, {ej:?} by the Journal"
                );
                assert_eq!(
                    state_of_journal(&self.j),
                    before,
                    "{input:?}: a refusal changed the Journal"
                );
            }
            _ => panic!("{input:?}: sequential {rs:?}, Journal {rj:?}"),
        }
        rs
    }

    /// Every tracked cell that is not changed has its snapshot's node.
    fn check_written_back(&mut self, input: &OpInput) {
        for (k, differs) in self.j.tracked_differs() {
            if differs {
                continue;
            }
            let Some(snap) = self.j.snapshot_node(&k).flatten().map(tree) else {
                continue;
            };
            let (sheet, at) = index_of_key(&self.j, &k);
            let cur = self.j.shadow_mut().snapshot_cell(sheet, at).unwrap();
            let cur = cur.node().map(tree);
            assert_eq!(
                cur.as_ref(),
                Some(&snap),
                "{input:?}: {k:?} is not changed but its node is not the snapshot's"
            );
        }
    }

    /// P(sequential) == P(shadow).
    pub(crate) fn finish(&self) {
        let a = project(&self.seq.to_bytes().unwrap());
        let b = project(&self.j.shadow().to_bytes().unwrap());
        if let Some(d) = first_diff(&a, &b) {
            panic!("P differs at {d}\nops: {:#?}", self.applied);
        }
    }
}

/// Runs `ops` through a [`Pair`] and compares the projections at the end.
pub(crate) fn run(base: &[u8], ops: impl IntoIterator<Item = OpInput>) -> Pair {
    let mut p = Pair::new(base);
    for op in ops {
        let _ = p.step(op);
    }
    p.finish();
    p
}
