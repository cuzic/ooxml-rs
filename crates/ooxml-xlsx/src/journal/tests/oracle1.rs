//! Oracle 1 before save (ADR-0024 "Test oracles" 1 (i) and (iii), the task's
//! §5): a random sequence of keyed ops applied by the sequential API and
//! through the Journal's shadow gives equal projections, with equal
//! refusals and a Journal that a refusal leaves untouched.

use std::collections::HashMap;

use proptest::prelude::*;

use super::fixtures::{group_workbook, oracle_workbook};
use super::*;
use crate::journal::op::{OpKind, WriteValue};
use crate::journal::project::{self, BaseCell, Val as PVal};

#[derive(Clone, Copy, Debug)]
enum Fx {
    Oracle(&'static str),
    Group,
}

impl Fx {
    fn bytes(self) -> Vec<u8> {
        match self {
            Fx::Oracle(dim) => oracle_workbook(dim),
            Fx::Group => group_workbook("A1:H6"),
        }
    }

    /// Cells that keys repeat on, `Sheet1`.
    fn hot(self) -> &'static [&'static str] {
        match self {
            Fx::Oracle(_) => &["B5", "C3", "C1", "A3", "D7", "B6", "C4", "D1"],
            Fx::Group => &["C2", "D2", "E3", "C3", "G2", "G3", "A2"],
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
    Named(bool),
}

#[derive(Clone, Debug)]
enum WVal {
    Number(f64),
    Str(&'static str),
    Formula(&'static str),
    Remove,
}

#[derive(Clone, Debug)]
enum GenOp {
    Content(Where, Value),
    Write(Vec<(Where, WVal)>, bool),
    Style(Where, StyleK),
    TwoHalf(Where, u8),
    Bait(u8),
}

#[derive(Clone, Debug)]
struct Case {
    fx: Fx,
    ops: Vec<GenOp>,
}

fn where_s() -> impl Strategy<Value = Where> {
    prop_oneof![
        40 => (0usize..8).prop_map(Where::Hot),
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
        any::<bool>().prop_map(StyleK::Named),
    ]
}

fn wval_s() -> impl Strategy<Value = WVal> {
    prop_oneof![
        3 => prop::sample::select(vec![1.0, 7.0, 0.5]).prop_map(WVal::Number),
        2 => prop::sample::select(vec!["x", "abc"]).prop_map(WVal::Str),
        2 => prop::sample::select(vec!["A1*2", "1+1"]).prop_map(WVal::Formula),
        1 => Just(WVal::Remove),
    ]
}

fn op_s() -> impl Strategy<Value = GenOp> {
    prop_oneof![
        45 => (where_s(), value_s()).prop_map(|(w, v)| GenOp::Content(w, v)),
        8 => (prop::collection::vec((where_s(), wval_s()), 1..=3), 0u32..10).prop_map(|(c, d)| GenOp::Write(c, d == 0)),
        30 => (where_s(), style_s()).prop_map(|(w, s)| GenOp::Style(w, s)),
        10 => (where_s(), 0u8..4).prop_map(|(w, k)| GenOp::TwoHalf(w, k)),
        7 => (0u8..3).prop_map(GenOp::Bait),
    ]
}

fn case_s() -> impl Strategy<Value = Case> {
    let fx = prop_oneof![
        3 => Just(Fx::Oracle("A1:C5")),
        1 => Just(Fx::Oracle("A1:A2")),
        1 => Just(Fx::Oracle("A1")),
        3 => Just(Fx::Group),
    ];
    (fx, prop::collection::vec(op_s(), 1..=32)).prop_map(|(fx, ops)| Case { fx, ops })
}

/// Resolves a generated case against its fixture's Base.
struct Resolver {
    fx: Fx,
    base: HashMap<(usize, CellIndex), BaseCell>,
    targets: Vec<(usize, CellIndex)>,
    written: HashMap<(usize, CellIndex), CellCall>,
}

impl Resolver {
    fn new(fx: Fx) -> Self {
        let base = project::base_cells(&fx.bytes())
            .into_iter()
            .map(|c| ((c.sheet, c.at), c))
            .collect();
        Resolver {
            fx,
            base,
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
            GenOp::Bait(k) => {
                let (a1, call) = match (k, self.fx) {
                    (0, Fx::Oracle(_)) => ("C4", CellCall::Number(1.0)),
                    (0, Fx::Group) => ("B6", CellCall::Number(1.0)),
                    (1, _) => ("G3", CellCall::Number(4.0)),
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

proptest! {
    #![proptest_config(ProptestConfig::with_cases(cases()))]

    /// Sequential application equals the Journal's shadow, op by op and in P.
    #[test]
    fn sequential_equals_shadow(case in case_s()) {
        let base = case.fx.bytes();
        let mut r = Resolver::new(case.fx);
        let mut p = Pair::new(&base);
        for op in &case.ops {
            let input = r.resolve(op);
            let _ = p.step(input);
        }
        p.finish();
    }
}

#[test]
fn the_generator_hits_every_op_family_and_some_refusals() {
    use proptest::strategy::ValueTree;
    use proptest::test_runner::{Config, TestRunner};
    let mut runner = TestRunner::new(Config {
        failure_persistence: None,
        ..Config::default()
    });
    let (mut ok, mut refused, mut families) = (0, 0, std::collections::HashSet::new());
    for _ in 0..200 {
        let case = case_s().new_tree(&mut runner).unwrap().current();
        let mut r = Resolver::new(case.fx);
        let mut p = Pair::new(&case.fx.bytes());
        for op in &case.ops {
            families.insert(std::mem::discriminant(op));
            match p.step(r.resolve(op)) {
                Ok(()) => ok += 1,
                Err(_) => refused += 1,
            }
        }
    }
    assert_eq!(families.len(), 5);
    assert!(ok > 1000 && refused > 100, "ok {ok}, refused {refused}");
}
