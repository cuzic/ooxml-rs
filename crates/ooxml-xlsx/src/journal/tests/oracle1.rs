//! Oracle 1 before save (ADR-0024 "Test oracles" 1 (i) and (iii), the task's
//! §5): a random sequence of keyed ops applied by the sequential API and
//! through the Journal's shadow gives equal projections, with equal
//! refusals and a Journal that a refusal leaves untouched.

use std::collections::HashMap;

use proptest::prelude::*;

use super::fixtures::{group_workbook, oracle_workbook, out_of_order_dependent_workbook};
use super::*;
use crate::journal::op::{ColCall, OpKind, RowCall, WriteValue};
use crate::workbook::ApplyStep;
use crate::journal::project::{self, BaseCell, Val as PVal};

#[derive(Clone, Copy, Debug)]
enum Fx {
    Oracle(&'static str),
    Group,
    /// `Sheet2` depends on `Sheet1!A1` and has its rows out of order.
    OutOfOrder,
}

impl Fx {
    fn bytes(self) -> Vec<u8> {
        match self {
            Fx::Oracle(dim) => oracle_workbook(dim),
            Fx::Group => group_workbook("A1:H6"),
            Fx::OutOfOrder => out_of_order_dependent_workbook(),
        }
    }

    /// Cells that keys repeat on, `Sheet1`.
    fn hot(self) -> &'static [&'static str] {
        match self {
            Fx::Oracle(_) => &["B5", "C3", "C1", "A3", "D7", "B6", "C4", "D1", "D2"],
            Fx::Group => &["C2", "D2", "E3", "C3", "G2", "G3", "A2"],
            Fx::OutOfOrder => &["A1", "A1", "B1", "A2"],
        }
    }
}

#[derive(Clone, Debug)]
enum Where {
    Hot(usize),
    Grid(u32, u32),
    Sheet2(u32, u32),
    Earlier(usize),
}

#[derive(Clone, Debug)]
enum Lit {
    Number(f64),
    Str(&'static str),
    Inline(&'static str),
    Rich(bool),
    Bool(bool),
    Error(&'static str),
    DateTime,
    Formula(&'static str),
    OwnFormula,
    RemoveFormula,
    ClearValue,
    Dynamic(&'static str),
}

#[derive(Clone, Debug)]
enum Value {
    Revert,
    Repeat,
    Lit(Lit),
}

#[derive(Clone, Debug)]
enum StyleK {
    Font(bool),
    Fill(bool),
    Border,
    NumFmt(bool),
    Format,
    Index(u32),
    /// An `xf` created earlier in the case: `base_xf_count + k` (P2).
    Session(u32),
    Named(bool),
}

#[derive(Clone, Debug)]
enum WVal {
    Number(f64),
    Str(&'static str),
    Formula(&'static str),
    Remove,
    ClearValue,
}

#[derive(Clone, Debug)]
enum GenOp {
    Content(Where, Value),
    Write(Vec<(Where, WVal)>, bool),
    Style(Where, StyleK),
    TwoHalf(Where, u8),
    Bait(u8),
    RowCol(Rc),
}

/// A row or column property op (values are indices into the tables in `resolve`).
#[derive(Clone, Debug)]
struct Rc {
    sheet2: bool,
    row: bool,
    /// rows `0..9`, columns `0..6`
    first: u32,
    len: u32,
    /// `0..100`: below 2 a full-width range, below 7 `first > last`
    shape: u8,
    prop: u8,
    val: u8,
}

/// `(op index, step selector, panic)`: the fault is injected before that op
/// (`fault-injection` only).
type Fault = Option<(usize, u8, bool)>;

#[derive(Clone, Debug)]
struct Case {
    fx: Fx,
    ops: Vec<GenOp>,
    fault: Fault,
}

fn where_s() -> impl Strategy<Value = Where> {
    prop_oneof![
        40 => (0usize..9).prop_map(Where::Hot),
        15 => (0u32..7, 0u32..8).prop_map(|(r, c)| Where::Grid(r, c)),
        10 => (0u32..2, 0u32..2).prop_map(|(r, c)| Where::Sheet2(r, c)),
        35 => (0usize..64).prop_map(Where::Earlier),
    ]
}

fn lit_s() -> impl Strategy<Value = Lit> {
    prop_oneof![
        15 => prop::sample::select(vec![1.0, 2.0, 3.0, 7.0, 5.0, 0.5, -3.25, 1e10, 100.0]).prop_map(Lit::Number),
        8 => prop::sample::select(vec!["x", "m", "y", "abc", "  pad ", "é"]).prop_map(Lit::Str),
        3 => prop::sample::select(vec!["x", "  pad "]).prop_map(Lit::Inline),
        3 => any::<bool>().prop_map(Lit::Rich),
        3 => any::<bool>().prop_map(Lit::Bool),
        3 => prop::sample::select(vec!["#N/A", "#DIV/0!", "#BAD"]).prop_map(Lit::Error),
        3 => Just(Lit::DateTime),
        3 => prop::sample::select(vec!["A1*2", "SUM(A1:A3)", "1+1", "1+", "B5*2"]).prop_map(Lit::Formula),
        2 => Just(Lit::OwnFormula),
        2 => Just(Lit::RemoveFormula),
        3 => Just(Lit::ClearValue),
        4 => prop::sample::select(vec!["1+1", "_xlfn.SEQUENCE(3)", "1+"]).prop_map(Lit::Dynamic),
    ]
}

fn value_s() -> impl Strategy<Value = Value> {
    prop_oneof![
        35 => Just(Value::Revert),
        15 => Just(Value::Repeat),
        50 => lit_s().prop_map(Value::Lit),
    ]
}

fn style_s() -> impl Strategy<Value = StyleK> {
    prop_oneof![
        any::<bool>().prop_map(StyleK::Font),
        any::<bool>().prop_map(StyleK::Fill),
        Just(StyleK::Border),
        any::<bool>().prop_map(StyleK::NumFmt),
        Just(StyleK::Format),
        prop::sample::select(vec![0u32, 1, 2, 3, 99]).prop_map(StyleK::Index),
        (0u32..3).prop_map(StyleK::Session),
        any::<bool>().prop_map(StyleK::Named),
    ]
}

fn wval_s() -> impl Strategy<Value = WVal> {
    prop_oneof![
        3 => prop::sample::select(vec![1.0, 7.0, 0.5]).prop_map(WVal::Number),
        2 => prop::sample::select(vec!["x", "abc"]).prop_map(WVal::Str),
        2 => prop::sample::select(vec!["A1*2", "1+1"]).prop_map(WVal::Formula),
        1 => Just(WVal::Remove),
        1 => Just(WVal::ClearValue),
    ]
}

fn rc_s() -> impl Strategy<Value = Rc> {
    (
        0u32..10,
        any::<bool>(),
        0u32..9,
        1u32..=3,
        0u8..100,
        0u8..3,
        0u8..4,
    )
        .prop_map(|(s, row, first, len, shape, prop, val)| Rc {
            sheet2: s == 0,
            row,
            first: if row { first } else { first % 6 },
            len,
            shape,
            prop,
            val,
        })
}

fn op_s() -> impl Strategy<Value = GenOp> {
    prop_oneof![
        42 => (where_s(), value_s()).prop_map(|(w, v)| GenOp::Content(w, v)),
        8 => (prop::collection::vec((where_s(), wval_s()), 1..=3), 0u32..10).prop_map(|(c, d)| GenOp::Write(c, d == 0)),
        26 => (where_s(), style_s()).prop_map(|(w, s)| GenOp::Style(w, s)),
        9 => (where_s(), 0u8..4).prop_map(|(w, k)| GenOp::TwoHalf(w, k)),
        7 => (0u8..5).prop_map(GenOp::Bait),
        8 => rc_s().prop_map(GenOp::RowCol),
    ]
}

#[cfg(feature = "fault-injection")]
fn fault_s() -> impl Strategy<Value = Fault> {
    prop_oneof![
        2 => Just(None),
        1 => (
            0usize..32,
            // the rarely reachable steps are chosen more often
            prop_oneof![1 => Just(0u8), 6 => Just(1u8), 2 => Just(2u8), 2 => Just(3u8), 1 => Just(4u8), 1 => Just(5u8), 4 => Just(6u8)],
            0u8..4
        )
            .prop_map(|(i, k, p)| Some((i, k, p == 0))),
    ]
}

#[cfg(not(feature = "fault-injection"))]
fn fault_s() -> impl Strategy<Value = Fault> {
    Just(None)
}

fn case_s() -> impl Strategy<Value = Case> {
    let fx = prop_oneof![
        3 => Just(Fx::Oracle("A1:C5")),
        1 => Just(Fx::Oracle("A1:A2")),
        1 => Just(Fx::Oracle("A1")),
        3 => Just(Fx::Group),
        1 => Just(Fx::OutOfOrder),
    ];
    (fx, prop::collection::vec(op_s(), 1..=32), fault_s())
        .prop_map(|(fx, ops, fault)| Case { fx, ops, fault })
}

/// Resolves a generated case against its fixture's Base.
struct Resolver {
    fx: Fx,
    base: HashMap<(usize, CellIndex), BaseCell>,
    targets: Vec<(usize, CellIndex)>,
    /// `cellXfs` count of the Base.
    base_xfs: u32,
    written: HashMap<(usize, CellIndex), CellCall>,
}

impl Resolver {
    fn new(fx: Fx) -> Self {
        let base = project::base_cells(&fx.bytes())
            .into_iter()
            .map(|c| ((c.sheet, c.at), c))
            .collect();
        let base_xfs = Workbook::from_bytes(fx.bytes())
            .and_then(|mut w| w.cell_format_count())
            .expect("a fixture opens");
        Resolver {
            fx,
            base,
            base_xfs,
            targets: Vec::new(),
            written: HashMap::new(),
        }
    }

    fn place(&mut self, w: &Where) -> (usize, CellIndex) {
        let p = match w {
            Where::Hot(i) => {
                let hot = self.fx.hot();
                (0, at(hot[i % hot.len()]))
            }
            Where::Grid(r, c) => (0, CellIndex { row: *r, col: *c }),
            Where::Sheet2(r, c) => (1, CellIndex { row: *r, col: *c }),
            Where::Earlier(i) if !self.targets.is_empty() => self.targets[i % self.targets.len()],
            Where::Earlier(_) => (0, at(self.fx.hot()[0])),
        };
        self.targets.push(p);
        p
    }

    fn revert(&self, p: (usize, CellIndex)) -> CellCall {
        // the oracle fixture's one inline string cell, so an inline snapshot
        // can come back (a full restore on `t="inlineStr"`)
        if matches!(self.fx, Fx::Oracle(_)) && p == (0, at("D2")) {
            return CellCall::StringInline("il".into());
        }
        match self.base.get(&p) {
            None => CellCall::RemoveFormula,
            Some(BaseCell {
                formula: Some(t), ..
            }) => CellCall::Formula(t.clone()),
            Some(BaseCell { value, .. }) => match value {
                PVal::Number(bits) => CellCall::Number(f64::from_bits(*bits)),
                PVal::Str { text, .. } => CellCall::String(text.clone()),
                PVal::Bool(b) => CellCall::Bool(*b),
                PVal::Error(e) => CellCall::Error(e.clone()),
                PVal::Empty | PVal::Cache => CellCall::RemoveFormula,
            },
        }
    }

    fn lit(&self, p: (usize, CellIndex), l: &Lit) -> CellCall {
        match l {
            Lit::Number(v) => CellCall::Number(*v),
            Lit::Str(s) => CellCall::String((*s).into()),
            Lit::Inline(s) => CellCall::StringInline((*s).into()),
            Lit::Rich(false) => {
                CellCall::RichString(vec![(bold(), "ab".into()), (Font::default(), "c".into())])
            }
            Lit::Rich(true) => {
                CellCall::RichString(vec![(bold(), "a".into()), (italic(), "bc".into())])
            }
            Lit::Bool(b) => CellCall::Bool(*b),
            Lit::Error(e) => CellCall::Error((*e).into()),
            Lit::DateTime => CellCall::DateTime(date()),
            Lit::Formula(t) => CellCall::Formula((*t).into()),
            Lit::OwnFormula => CellCall::Formula(
                self.base
                    .get(&p)
                    .and_then(|c| c.formula.clone())
                    .unwrap_or_else(|| "A1*2".into()),
            ),
            Lit::RemoveFormula => CellCall::RemoveFormula,
            Lit::ClearValue => CellCall::ClearValue,
            Lit::Dynamic(t) => CellCall::DynamicFormula((*t).into()),
        }
    }

    fn resolve(&mut self, op: &GenOp) -> OpInput {
        match op {
            GenOp::Content(w, v) => {
                let p = self.place(w);
                let call = match v {
                    Value::Revert => self.revert(p),
                    Value::Repeat => self
                        .written
                        .get(&p)
                        .cloned()
                        .unwrap_or(CellCall::Number(1.0)),
                    Value::Lit(l) => self.lit(p, l),
                };
                self.written.insert(p, call.clone());
                OpInput {
                    sheet: p.0,
                    kind: OpKind::Cell { at: p.1, call },
                }
            }
            GenOp::Write(cells, dup) => {
                let mut out: Vec<(CellIndex, WriteValue)> = Vec::new();
                for (w, v) in cells {
                    let (_, a) = self.place_on_sheet1(w);
                    let wv = match v {
                        WVal::Number(n) => WriteValue::Number(*n),
                        WVal::Str(s) => WriteValue::String((*s).into()),
                        WVal::Formula(t) => WriteValue::Formula((*t).into()),
                        WVal::Remove => WriteValue::RemoveFormula,
                        WVal::ClearValue => WriteValue::ClearValue,
                    };
                    out.push((a, wv));
                }
                if *dup {
                    let first = out[0].clone();
                    out.push(first);
                }
                OpInput {
                    sheet: 0,
                    kind: OpKind::WriteCells(out),
                }
            }
            GenOp::Style(w, s) => {
                let p = self.place(w);
                let call = match s {
                    StyleK::Font(b) => CellCall::Font(if *b { bold() } else { italic() }),
                    StyleK::Fill(b) => CellCall::Fill(if *b { red() } else { blue() }),
                    StyleK::Border => CellCall::Border(thin()),
                    StyleK::NumFmt(b) => {
                        CellCall::NumFormat(if *b { "0.00" } else { "0.000" }.into())
                    }
                    StyleK::Format => CellCall::Format(full_format()),
                    StyleK::Index(i) => CellCall::StyleIndex(*i),
                    StyleK::Session(k) => CellCall::StyleIndex(self.base_xfs + k),
                    StyleK::Named(b) => {
                        CellCall::NamedStyle(if *b { "Normal" } else { "Nope" }.into())
                    }
                };
                OpInput {
                    sheet: p.0,
                    kind: OpKind::Cell { at: p.1, call },
                }
            }
            GenOp::TwoHalf(w, k) => {
                let p = self.place(w);
                let call = match k {
                    0 => CellCall::NumberWithFormat(5.0, full_format()),
                    1 => CellCall::StringWithFormat("s".into(), Format::new().set_italic()),
                    2 => CellCall::FormulaWithFormat("A1*2".into(), full_format()),
                    _ => CellCall::DateTimeWithFormat(date(), Format::date()),
                };
                OpInput {
                    sheet: p.0,
                    kind: OpKind::Cell { at: p.1, call },
                }
            }
            GenOp::RowCol(rc) => {
                let sheet = usize::from(rc.sheet2);
                if rc.row {
                    let call = match rc.prop {
                        0 => RowCall::Height([20.0, 15.0, 500.0, f64::NAN][usize::from(rc.val)]),
                        1 => RowCall::Hidden(rc.val % 2 == 0),
                        _ => RowCall::Outline([0, 2, 8][usize::from(rc.val) % 3]),
                    };
                    return OpInput {
                        sheet,
                        kind: OpKind::Row {
                            row: rc.first,
                            call,
                        },
                    };
                }
                let (first, last) = match rc.shape {
                    0..2 => (0, 16_383),
                    2..7 => (rc.first + rc.len, rc.first),
                    _ => (rc.first, rc.first + rc.len - 1),
                };
                let call = match rc.prop {
                    0 => ColCall::Width([10.0, 20.0, 300.0][usize::from(rc.val) % 3]),
                    1 => ColCall::Hidden(rc.val % 2 == 0),
                    _ => ColCall::Outline([0, 2, 8][usize::from(rc.val) % 3]),
                };
                OpInput {
                    sheet,
                    kind: OpKind::Cols { first, last, call },
                }
            }
            GenOp::Bait(k) => {
                let (a1, call) = match (k, self.fx) {
                    (0, Fx::Oracle(_)) => ("C4", CellCall::Number(1.0)),
                    (0, Fx::Group) => ("B6", CellCall::Number(1.0)),
                    (1, _) => ("G3", CellCall::Number(4.0)),
                    (3, Fx::Oracle(_)) => ("C4", CellCall::ClearValue),
                    (3, _) => ("B6", CellCall::ClearValue),
                    (4, _) => ("G3", CellCall::ClearValue),
                    (_, _) => (
                        "A1",
                        CellCall::Number(if *k == 2 { f64::NAN } else { f64::INFINITY }),
                    ),
                };
                self.targets.push((0, at(a1)));
                OpInput {
                    sheet: 0,
                    kind: OpKind::Cell { at: at(a1), call },
                }
            }
        }
    }

    /// A position on `Sheet1` (`write_cells` is one sheet's call).
    fn place_on_sheet1(&mut self, w: &Where) -> (usize, CellIndex) {
        let w = match w {
            Where::Sheet2(r, c) => Where::Grid(*r, *c),
            w => w.clone(),
        };
        let p = self.place(&w);
        if p.0 == 0 { p } else { (0, p.1) }
    }
}

fn cases() -> u32 {
    std::env::var("OOXML_J0_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(256)
}

/// What a case did, for the coverage test.
#[derive(Default)]
struct Tally {
    ok: u32,
    refused: u32,
    rc_ok: u32,
    rc_refused: u32,
    clear_ok: u32,
    dynamic_ok: u32,
    session_ok: u32,
    fired: u32,
    recovered: u32,
    steps: std::collections::HashSet<String>,
    skips: u32,
    skip_content: u32,
    skip_style: u32,
    skip_twohalf: u32,
    skip_cells: u32,
}

const ALL_STEPS: [ApplyStep; 7] = [
    ApplyStep::Target,
    ApplyStep::LaterTarget,
    ApplyStep::Registries,
    ApplyStep::HandOff,
    ApplyStep::Index,
    ApplyStep::RangeFeature,
    ApplyStep::Registry,
];

/// The steps an op can reach, for a fault (`fault-injection`).
fn reachable(kind: &OpKind) -> Vec<ApplyStep> {
    use ApplyStep::*;
    match kind {
        OpKind::Row { .. } | OpKind::Cols { .. } => vec![RangeFeature],
        OpKind::Cell {
            call: CellCall::DynamicFormula(_),
            ..
        } => vec![Registry],
        OpKind::WriteCells(c) if c.len() >= 2 => vec![LaterTarget, Target, HandOff, Index],
        OpKind::WriteCells(_) => vec![Target, HandOff, Index],
        OpKind::Cell { call, .. } if crate::journal::op::writes_content(call) => {
            vec![Target, Index, HandOff, Registries]
        }
        OpKind::Cell { .. } => vec![Registries, Target],
    }
}

fn family(kind: &OpKind) -> (bool, bool, bool) {
    // (row/col, clear, dynamic)
    match kind {
        OpKind::Row { .. } | OpKind::Cols { .. } => (true, false, false),
        OpKind::Cell {
            call: CellCall::ClearValue,
            ..
        } => (false, true, false),
        OpKind::Cell {
            call: CellCall::DynamicFormula(_),
            ..
        } => (false, false, true),
        OpKind::WriteCells(c) => (
            false,
            c.iter().any(|(_, w)| matches!(w, WriteValue::ClearValue)),
            false,
        ),
        _ => (false, false, false),
    }
}

/// R1 coverage: when the Journal skipped the op, tally its kind.
fn tally_skip(tally: &mut Tally, p: &Pair, input: &OpInput) {
    if p.j.log().last().is_none_or(|o| o.applied) {
        return;
    }
    tally.skips += 1;
    match &input.kind {
        OpKind::WriteCells(_) => tally.skip_cells += 1,
        OpKind::Cell { call, .. } => match call {
            CellCall::DateTime(_)
            | CellCall::NumberWithFormat(..)
            | CellCall::StringWithFormat(..)
            | CellCall::FormulaWithFormat(..)
            | CellCall::DateTimeWithFormat(..) => tally.skip_twohalf += 1,
            CellCall::Format(_)
            | CellCall::NumFormat(_)
            | CellCall::Font(_)
            | CellCall::Fill(_)
            | CellCall::Border(_)
            | CellCall::StyleIndex(_)
            | CellCall::StyleXf(..)
            | CellCall::NamedStyle(_) => tally.skip_style += 1,
            _ => tally.skip_content += 1,
        },
        OpKind::Row { .. } | OpKind::Cols { .. } => unreachable!("row/col ops are never skipped"),
    }
}

/// Runs a case through a Pair. A fault op runs the Journal first: if it
/// returns `Internal` the sequential side skips the op and the Journal is as
/// it was before; otherwise the usual checks apply.
fn drive(case: &Case, tally: &mut Tally) -> Pair {
    let mut r = Resolver::new(case.fx);
    let mut p = Pair::new(&case.fx.bytes());
    let mut pending = case
        .fault
        .map(|(at, k, panic)| (at, ALL_STEPS[usize::from(k) % ALL_STEPS.len()], panic));
    for (i, op) in case.ops.iter().enumerate() {
        let input = r.resolve(op);
        let (rc, clear, dynamic) = family(&input.kind);
        // the fault goes to the first op at or after its index that can
        // reach its step
        let fault = pending.filter(|&(at, step, _)| {
            i >= at % case.ops.len() && reachable(&input.kind).contains(&step)
        });
        if fault.is_some() {
            pending = None;
        }
        let result = match fault {
            #[cfg(feature = "fault-injection")]
            Some((_, step, panic)) => {
                if panic {
                    p.j.shadow_mut().inject_apply_panic(step);
                } else {
                    p.j.shadow_mut().inject_apply_failure(step);
                }
                let before = state_of_journal(&p.j);
                let rj = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    p.j.append(Api::Native, input.clone())
                }));
                p.j.shadow_mut().fault = None;
                let internal = match &rj {
                    Err(_) => {
                        // the next call would recover; do it now
                        p.j.settle().expect("recovery by replay");
                        true
                    }
                    Ok(Err(JournalError::Internal { .. })) => {
                        assert!(!p.j.broken, "{input:?}: the recovery failed");
                        true
                    }
                    Ok(_) => false,
                };
                if internal {
                    tally.fired += 1;
                    tally.recovered += 1;
                    tally.steps.insert(format!("{step:?}"));
                    let after = state_of_journal(&p.j);
                    assert_eq!(after.log, before.log, "{input:?}: log");
                    assert_eq!(after.next_seq, before.next_seq, "{input:?}: next_seq");
                    assert_eq!(after.keys, before.keys, "{input:?}: keys");
                    assert_eq!(after.open, before.open, "{input:?}: open");
                    assert_eq!(after.tracked, before.tracked, "{input:?}: tracked");
                    // the written-back bytes are the same, part by part
                    let (pa, pb) = (
                        ooxml_core::Package::from_bytes(after.bytes.clone()).unwrap(),
                        ooxml_core::Package::from_bytes(before.bytes.clone()).unwrap(),
                    );
                    for n in pa.part_names() {
                        let (x, y) = (pa.read_part(n).ok(), pb.read_part(n).ok());
                        assert_eq!(x, y, "{input:?}: {n} differs after recovery");
                    }
                    continue;
                }
                // not a failure: finish what `Pair::step` does, with the
                // Journal's result in hand
                let skipped =
                    matches!(&rj, Ok(Ok(_))) && p.j.log().last().is_some_and(|o| !o.applied);
                if skipped {
                    // R1 skipped the op, so the fault never fired: re-arm it
                    pending = Some((i + 1, step, panic));
                }
                let rj = rj.unwrap();
                let rs = crate::journal::op::apply_seq(&mut p.seq, &input);
                p.applied.push(input.clone());
                match (&rs, &rj) {
                    (Ok(()), Ok(_)) => p.check_accepted(&input, &before),
                    (Err(es), Err(JournalError::Op(ej))) => assert_eq!(
                        discriminant(es),
                        discriminant(ej),
                        "{input:?}: refused with {es:?} sequentially, {ej:?} by the Journal"
                    ),
                    _ => panic!("{input:?}: sequential {rs:?}, Journal {rj:?}"),
                }
                rs
            }
            _ => p.step(input.clone()),
        };
        match result {
            Ok(()) => {
                tally.ok += 1;
                tally_skip(tally, &p, &input);
                tally.rc_ok += u32::from(rc);
                tally.clear_ok += u32::from(clear);
                tally.dynamic_ok += u32::from(dynamic);
                tally.session_ok += u32::from(matches!(
                    &input.kind,
                    OpKind::Cell { call: CellCall::StyleIndex(i), .. } if *i >= p.j.base_counts.cell_xfs
                ));
            }
            Err(_) => {
                tally.refused += 1;
                tally.rc_refused += u32::from(rc);
            }
        }
    }
    p
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(cases()))]

    /// Sequential application equals the Journal's shadow, op by op and in P.
    #[test]
    fn sequential_equals_shadow(case in case_s()) {
        let mut tally = Tally::default();
        drive(&case, &mut tally).finish();
    }
}

#[test]
fn the_generator_hits_every_op_family_and_some_refusals() {
    use proptest::strategy::ValueTree;
    use proptest::test_runner::{Config, TestRunner};
    // a fixed seed: the floors below are then not flaky
    let mut runner = TestRunner::deterministic();
    let _ = Config::default();
    let mut tally = Tally::default();
    let mut families = std::collections::HashSet::new();
    let mut full: HashMap<&'static str, u32> = HashMap::new();
    let mut content = 0;
    for _ in 0..256 {
        let case = case_s().new_tree(&mut runner).unwrap().current();
        for op in &case.ops {
            families.insert(std::mem::discriminant(op));
        }
        let p = drive(&case, &mut tally);
        for (k, n) in &p.j.stats().full {
            *full.entry(k).or_default() += n;
        }
        content += p.j.stats().content;
        p.finish();
    }
    // R2 is actually exercised: full restores on every string kind and on
    // formulas, and content-only restores. R1 absorbs the write-backs that
    // repeat the current value (they skip instead of apply-and-restore),
    // so the formula/shared floors sit below their R1-off levels; every
    // kind still restores.
    println!("full restores {full:?}, content {content}");
    let n = |k: &str| full.get(k).copied().unwrap_or(0);
    assert!(
        n("value") >= 10 && n("formula") >= 3 && n("shared") >= 1 && n("inline") >= 1,
        "full restores {full:?}"
    );
    assert!(content >= 10, "content restores {content}");
    assert_eq!(families.len(), 6);
    let t = &tally;
    println!(
        "ok {} refused {}; row/col ok {} refused {}; clear ok {}; dynamic ok {}; faults fired {} recovered {}; steps {:?}",
        t.ok, t.refused, t.rc_ok, t.rc_refused, t.clear_ok, t.dynamic_ok, t.fired, t.recovered, t.steps
    );
    println!(
        "skips {} (content {} style {} two-half {} cells {})",
        t.skips, t.skip_content, t.skip_style, t.skip_twohalf, t.skip_cells
    );
    // R1 fires on every kind (fail loudly if it never fires). Two-half
    // and whole-batch skips are rare under the fixed generator (fixed
    // two-half values, all-cells-must-match batches), so their floors sit
    // at "fires", not > 5; the seed is fixed, so they are still exact.
    assert!(t.skips > 50, "skips {}", t.skips);
    assert!(
        t.skip_content > 5 && t.skip_style > 5 && t.skip_twohalf > 2 && t.skip_cells > 0,
        "content {} style {} two-half {} cells {}",
        t.skip_content,
        t.skip_style,
        t.skip_twohalf,
        t.skip_cells
    );
    assert!(t.ok > 1000 && t.refused > 100, "ok {}, refused {}", t.ok, t.refused);
    assert!(t.rc_ok > 50 && t.rc_refused > 10, "row/col {} {}", t.rc_ok, t.rc_refused);
    assert!(t.clear_ok > 20, "clear {}", t.clear_ok);
    assert!(t.dynamic_ok > 10, "dynamic {}", t.dynamic_ok);
    assert!(t.session_ok > 10, "session xf {}", t.session_ok);
    #[cfg(feature = "fault-injection")]
    {
        assert!(t.fired > 10 && t.recovered > 10, "faults {} {}", t.fired, t.recovered);
        assert_eq!(t.steps.len(), 7, "steps fired: {:?}", t.steps);
    }
}
