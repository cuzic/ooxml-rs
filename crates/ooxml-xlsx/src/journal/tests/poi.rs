//! Oracle 1 over the committed POI fixtures (ADR-0024 "Test oracles" 1, the
//! task's §4.3): every fixture that opens for editing gets random keyed ops
//! through J0a's [`Pair`] (op by op, and P at the end).
//!
//! `OOXML_J0B_POI_CASES` sets the cases per fixture (default 16).
//!
//! A refusal that both sides agree on is fine (`MissingReference`,
//! `NotAWorksheet`, an `mc:AlternateContent` style table, pivot and table
//! guards). Anything else that differs is a bug, not a case to filter out.

use std::collections::HashMap;
use std::path::PathBuf;

use ooxml_core::Package;
use proptest::prelude::*;
use proptest::test_runner::{Config, FileFailurePersistence, TestError, TestRunner};

use super::fixtures::meta_workbook;
use super::*;
use crate::census::SheetKind;
use crate::journal::op::{ColCall, OpKind, RowCall, WriteValue};
use crate::journal::project::{self, BaseCell, Val as PVal};

fn poi_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../reference/fixtures/xlsx/poi")
}

/// The committed POI packages that open and that edits do not refuse.
pub(crate) fn fixtures() -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    let mut paths: Vec<PathBuf> = std::fs::read_dir(poi_dir())
        .expect("the POI fixture directory")
        .map(|e| e.expect("an entry").path())
        .filter(|p| p.extension().is_some_and(|e| e == "xlsx" || e == "xlsm"))
        .collect();
    paths.sort();
    for p in paths {
        let bytes = std::fs::read(&p).expect("a fixture");
        let editable =
            Workbook::from_bytes(bytes.clone()).is_ok_and(|wb| wb.edit_refusal().is_none());
        if editable {
            let name = p
                .file_name()
                .expect("a name")
                .to_string_lossy()
                .into_owned();
            out.push((name, bytes));
        }
    }
    out
}

fn cases() -> u32 {
    std::env::var("OOXML_J0B_POI_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(16)
}

// ---- the generator -------------------------------------------------------

#[derive(Clone, Debug)]
enum Where {
    Hot(usize),
    /// Around the sheet's extent: the first number picks a boundary (`0`: the
    /// last row, `1`: the row after it, `2`: the last column, `3`: the column
    /// after it; the other coordinate is uniform) or, from `4`, a uniform point
    /// in the extent plus one; the others are the row and column numbers.
    Dim(u8, u32, u32),
    Earlier(usize),
}

/// Which sheet: an index into the worksheet list (sheet 0 three times), or a
/// non-worksheet.
#[derive(Clone, Debug)]
enum Sh {
    Ws(usize),
    Other(usize),
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
    /// `0..min(count, 8)` (`true`) or `count + 50` (`false`), by the number.
    Index(bool, u32),
    /// An `xf` created earlier in the case: `formats + k` (P2).
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

/// A row or column property op.
#[derive(Clone, Debug)]
struct Rc {
    row: bool,
    /// Reduced modulo the sheet's extent plus two.
    first: u32,
    len: u32,
    /// `0..100`: below 2 a full-width range, below 7 `first > last`.
    shape: u8,
    prop: u8,
    val: u8,
}

#[derive(Clone, Debug)]
enum GenOp {
    Content(Sh, Where, Value),
    Write(Sh, Vec<(Where, WVal)>, bool),
    Style(Sh, Where, StyleK),
    TwoHalf(Sh, Where, u8),
    RowCol(Sh, Rc),
}

fn sh_s() -> impl Strategy<Value = Sh> {
    prop_oneof![
        95 => (0usize..64).prop_map(Sh::Ws),
        5 => (0usize..64).prop_map(Sh::Other),
    ]
}

fn where_s() -> impl Strategy<Value = Where> {
    prop_oneof![
        50 => (0usize..64).prop_map(Where::Hot),
        20 => (0u8..8, 0u32..1000, 0u32..1000).prop_map(|(k, r, c)| Where::Dim(k, r, c)),
        30 => (0usize..64).prop_map(Where::Earlier),
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
        2 => prop::sample::select(vec!["1+1", "_xlfn.SEQUENCE(3)", "1+"]).prop_map(Lit::Dynamic),
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
        (0u32..3).prop_map(StyleK::Session),
        (any::<bool>(), 0u32..8).prop_map(|(a, b)| StyleK::Index(a, b)),
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
    (any::<bool>(), 0u32..1000, 1u32..=3, 0u8..100, 0u8..3, 0u8..4)
        .prop_map(|(row, first, len, shape, prop, val)| Rc { row, first, len, shape, prop, val })
}

fn op_s() -> impl Strategy<Value = GenOp> {
    prop_oneof![
        45 => (sh_s(), where_s(), value_s()).prop_map(|(s, w, v)| GenOp::Content(s, w, v)),
        8 => (sh_s(), prop::collection::vec((where_s(), wval_s()), 1..=3), 0u32..10)
            .prop_map(|(s, c, d)| GenOp::Write(s, c, d == 0)),
        30 => (sh_s(), where_s(), style_s()).prop_map(|(s, w, k)| GenOp::Style(s, w, k)),
        10 => (sh_s(), where_s(), 0u8..4).prop_map(|(s, w, k)| GenOp::TwoHalf(s, w, k)),
        8 => (sh_s(), rc_s()).prop_map(|(s, r)| GenOp::RowCol(s, r)),
    ]
}

fn ops_s() -> impl Strategy<Value = Vec<GenOp>> {
    prop::collection::vec(op_s(), 1..=24)
}

// ---- resolving against a fixture's Base -----------------------------------

/// What the generator knows about one package.
struct Info {
    base: HashMap<(usize, CellIndex), BaseCell>,
    /// Worksheet indices, sheet 0's first (and again, so it weighs three times).
    worksheets: Vec<usize>,
    others: Vec<usize>,
    /// Up to 12 hot cells per sheet.
    hot: HashMap<usize, Vec<CellIndex>>,
    /// Per sheet: (last row, last column), 0-based: the end of the `dimension`
    /// element when there is one, else the Base's cell maxima.
    extent: HashMap<usize, (u32, u32)>,
    /// `cellXfs` count.
    formats: u32,
}

/// The last cell of a worksheet part's `dimension/@ref`.
fn dimension_end(bytes: &[u8], part: &str) -> Option<(u32, u32)> {
    let mut pkg = Package::from_bytes(bytes.to_vec()).expect("package");
    let doc = pkg.xml_part(part).ok()?;
    let d = doc
        .root()
        .elements()
        .find(|e| e.name().local() == "dimension")?;
    let r = d.attr(ooxml_core::QName::new("", "ref"))?;
    let end = CellIndex::parse(r.split_once(':').map_or(r, |(_, b)| b)).ok()?;
    Some((end.row, end.col))
}

/// The first non-anchor cell of each merge of a worksheet part.
fn merge_cells(bytes: &[u8], part: &str) -> Vec<CellIndex> {
    let mut pkg = Package::from_bytes(bytes.to_vec()).expect("package");
    let Ok(doc) = pkg.xml_part(part) else {
        return Vec::new();
    };
    doc.root()
        .elements()
        .filter(|e| e.name().local() == "mergeCells")
        .flat_map(|m| m.elements().cloned().collect::<Vec<_>>())
        .filter_map(|m| {
            let r = m.attr(ooxml_core::QName::new("", "ref"))?.to_owned();
            let (a, b) = r.split_once(':')?;
            let (a, b) = (CellIndex::parse(a).ok()?, CellIndex::parse(b).ok()?);
            // the first cell after the anchor, in row-major order
            if b.col > a.col {
                Some(CellIndex {
                    row: a.row,
                    col: a.col + 1,
                })
            } else if b.row > a.row {
                Some(CellIndex {
                    row: a.row + 1,
                    col: a.col,
                })
            } else {
                None
            }
        })
        .collect()
}

impl Info {
    fn new(bytes: &[u8]) -> Info {
        let mut wb = Workbook::from_bytes(bytes.to_vec()).expect("a workbook");
        let cells = project::base_cells(bytes);
        let mut worksheets = Vec::new();
        let mut others = Vec::new();
        for (i, s) in wb.sheets.iter().enumerate() {
            if s.kind == SheetKind::Worksheet {
                worksheets.push(i);
            } else {
                others.push(i);
            }
        }
        let mut hot: HashMap<usize, Vec<CellIndex>> = HashMap::new();
        let mut extent: HashMap<usize, (u32, u32)> = HashMap::new();
        for &sheet in &worksheets {
            let mine: Vec<&BaseCell> = cells.iter().filter(|c| c.sheet == sheet).collect();
            let e = extent.entry(sheet).or_default();
            for c in &mine {
                *e = (e.0.max(c.at.row), e.1.max(c.at.col));
            }
            if let Some(d) = wb.sheets[sheet]
                .part
                .as_deref()
                .and_then(|part| dimension_end(bytes, part))
            {
                *e = d;
            }
            let rank = |c: &BaseCell| match (&c.value, c.group, c.formula.is_some()) {
                (_, true, _) => 0,
                (_, _, true) => 1,
                (PVal::Str { .. }, ..) => 2,
                (PVal::Number(_), ..) => 3,
                (PVal::Bool(_), ..) => 4,
                _ => 5,
            };
            let mut ranked: Vec<&BaseCell> = mine;
            ranked.sort_by_key(|c| rank(c));
            let mut list: Vec<CellIndex> = ranked.iter().take(12).map(|c| c.at).collect();
            if let Some(part) = wb.sheets[sheet].part.clone() {
                list.extend(merge_cells(bytes, &part));
            }
            hot.insert(sheet, list);
        }
        let mut order = Vec::new();
        if let Some(first) = worksheets.first() {
            order.extend([*first; 3]);
        }
        order.extend(worksheets.iter().skip(1).copied());
        let formats = wb.cell_format_count().expect("the format count");
        Info {
            base: cells.into_iter().map(|c| ((c.sheet, c.at), c)).collect(),
            worksheets: order,
            others,
            hot,
            extent,
            formats,
        }
    }
}

struct Resolver<'a> {
    info: &'a Info,
    targets: Vec<(usize, CellIndex)>,
    written: HashMap<(usize, CellIndex), CellCall>,
}

impl<'a> Resolver<'a> {
    fn new(info: &'a Info) -> Self {
        Resolver {
            info,
            targets: Vec::new(),
            written: HashMap::new(),
        }
    }

    fn sheet(&self, s: &Sh) -> usize {
        match s {
            Sh::Other(i) if !self.info.others.is_empty() => {
                self.info.others[i % self.info.others.len()]
            }
            Sh::Ws(i) | Sh::Other(i) if !self.info.worksheets.is_empty() => {
                self.info.worksheets[i % self.info.worksheets.len()]
            }
            _ => 0,
        }
    }

    fn place(&mut self, sheet: usize, w: &Where) -> (usize, CellIndex) {
        let p = match w {
            Where::Hot(i) => match self.info.hot.get(&sheet).filter(|h| !h.is_empty()) {
                Some(h) => (sheet, h[i % h.len()]),
                None => (sheet, CellIndex { row: 0, col: 0 }),
            },
            Where::Dim(k, r, c) => {
                let (mr, mc) = self.info.extent.get(&sheet).copied().unwrap_or((0, 0));
                let (ur, uc) = (r % (mr + 2), c % (mc + 2));
                let (row, col) = match k {
                    0 => (mr, uc),
                    1 => (mr + 1, uc),
                    2 => (ur, mc),
                    3 => (ur, mc + 1),
                    _ => (ur, uc),
                };
                (sheet, CellIndex { row, col })
            }
            Where::Earlier(i) if !self.targets.is_empty() => self.targets[i % self.targets.len()],
            Where::Earlier(_) => (sheet, CellIndex { row: 0, col: 0 }),
        };
        self.targets.push(p);
        p
    }

    /// The call that writes the Base's content of `p` back, as near as the
    /// calls allow. An empty `c` is restored exactly by `ClearValue` (P1). A raw
    /// value (`t="d"` and the like) is only cleared, not restored; a formula-less
    /// array result is refused on both sides (`ArrayResultCell`); a cell the
    /// Base lacks gets `RemoveFormula`, which keeps a value an earlier op wrote.
    fn revert(&self, p: (usize, CellIndex)) -> CellCall {
        match self.info.base.get(&p) {
            None => CellCall::RemoveFormula,
            Some(BaseCell {
                formula: Some(t), ..
            }) => CellCall::Formula(t.clone()),
            Some(BaseCell { value, inline, .. }) => match value {
                PVal::Number(bits) => CellCall::Number(f64::from_bits(*bits)),
                PVal::Str { text, .. } if *inline => CellCall::StringInline(text.clone()),
                PVal::Str { text, .. } => CellCall::String(text.clone()),
                PVal::Bool(b) => CellCall::Bool(*b),
                PVal::Error(e) => CellCall::Error(e.clone()),
                PVal::Empty | PVal::Cache => CellCall::ClearValue,
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
                self.info
                    .base
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
            GenOp::Content(s, w, v) => {
                let sheet = self.sheet(s);
                let p = self.place(sheet, w);
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
            GenOp::Write(s, cells, dup) => {
                let sheet = self.sheet(s);
                let mut out: Vec<(CellIndex, WriteValue)> = Vec::new();
                for (w, v) in cells {
                    // `write_cells` is one sheet's call
                    let (_, a) = self.place(sheet, w);
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
                    sheet,
                    kind: OpKind::WriteCells(out),
                }
            }
            GenOp::Style(s, w, k) => {
                let sheet = self.sheet(s);
                let p = self.place(sheet, w);
                let call = match k {
                    StyleK::Font(b) => CellCall::Font(if *b { bold() } else { italic() }),
                    StyleK::Fill(b) => CellCall::Fill(if *b { red() } else { blue() }),
                    StyleK::Border => CellCall::Border(thin()),
                    StyleK::NumFmt(b) => {
                        CellCall::NumFormat(if *b { "0.00" } else { "0.000" }.into())
                    }
                    StyleK::Format => CellCall::Format(full_format()),
                    StyleK::Index(true, n) => {
                        CellCall::StyleIndex(n % self.info.formats.clamp(1, 8))
                    }
                    StyleK::Session(k) => CellCall::StyleIndex(self.info.formats + k),
                    StyleK::Index(false, _) => CellCall::StyleIndex(self.info.formats + 50),
                    StyleK::Named(b) => {
                        CellCall::NamedStyle(if *b { "Normal" } else { "Nope" }.into())
                    }
                };
                OpInput {
                    sheet: p.0,
                    kind: OpKind::Cell { at: p.1, call },
                }
            }
            GenOp::RowCol(s, rc) => {
                let sheet = self.sheet(s);
                let (mr, mc) = self.info.extent.get(&sheet).copied().unwrap_or((0, 0));
                if rc.row {
                    let call = match rc.prop {
                        0 => RowCall::Height([20.0, 15.0, 500.0, f64::NAN][usize::from(rc.val)]),
                        1 => RowCall::Hidden(rc.val % 2 == 0),
                        _ => RowCall::Outline([0, 2, 8][usize::from(rc.val) % 3]),
                    };
                    return OpInput {
                        sheet,
                        kind: OpKind::Row { row: rc.first % (mr + 3), call },
                    };
                }
                let f = rc.first % (mc + 3);
                let (first, last) = match rc.shape {
                    0..2 => (0, 16_383),
                    2..7 => (f + rc.len, f),
                    _ => (f, f + rc.len - 1),
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
            GenOp::TwoHalf(s, w, k) => {
                let sheet = self.sheet(s);
                let p = self.place(sheet, w);
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
        }
    }
}

fn runner() -> TestRunner {
    TestRunner::new(Config {
        cases: cases(),
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct(
            "proptest-regressions/journal/tests/poi.txt",
        ))),
        ..Config::default()
    })
}

/// Accepted ops of the newer groups: session `xf` index, `ClearValue`,
/// `DynamicFormula`, row and column properties.
#[derive(Clone, Copy, Default)]
struct Kinds {
    session: u32,
    clear: u32,
    dynamic: u32,
    rowcol: u32,
}

/// Runs the generator over one fixture and returns (ops ok, ops refused, kinds).
fn run_fixture(name: &str, bytes: &[u8]) -> (u32, u32, Kinds) {
    let info = Info::new(bytes);
    let (ok, refused) = (std::cell::Cell::new(0u32), std::cell::Cell::new(0u32));
    let kinds = std::cell::Cell::new(Kinds::default());
    let result = runner().run(&ops_s(), |ops| {
        let mut r = Resolver::new(&info);
        let mut p = Pair::new(bytes);
        for op in &ops {
            let input = r.resolve(op);
            let mut k = kinds.get();
            let (is_session, is_clear, is_dyn, is_rc) = match &input.kind {
                OpKind::Cell { call: CellCall::StyleIndex(i), .. } => (*i >= info.formats, false, false, false),
                OpKind::Cell { call: CellCall::ClearValue, .. } => (false, true, false, false),
                OpKind::Cell { call: CellCall::DynamicFormula(_), .. } => (false, false, true, false),
                OpKind::Row { .. } | OpKind::Cols { .. } => (false, false, false, true),
                _ => (false, false, false, false),
            };
            match p.step(input) {
                Ok(()) => {
                    ok.set(ok.get() + 1);
                    k.session += u32::from(is_session);
                    k.clear += u32::from(is_clear);
                    k.dynamic += u32::from(is_dyn);
                    k.rowcol += u32::from(is_rc);
                    kinds.set(k);
                }
                Err(_) => refused.set(refused.get() + 1),
            }
        }
        p.finish();
        Ok(())
    });
    if let Err(e) = result {
        match e {
            TestError::Fail(reason, ops) => panic!("{name}: {reason}\nops: {ops:#?}"),
            TestError::Abort(reason) => panic!("{name}: aborted: {reason}"),
        }
    }
    (ok.get(), refused.get(), kinds.get())
}

#[test]
fn poi_fixture_list_is_complete() {
    let names: Vec<String> = fixtures().into_iter().map(|(n, _)| n).collect();
    println!("{} fixtures: {names:?}", names.len());
    assert!(
        names.len() >= 25,
        "only {} fixtures open for editing",
        names.len()
    );
}

const SESSION_FLOOR: u32 = 10;

#[test]
fn sequential_equals_shadow_on_the_poi_fixtures() {
    let (mut ok, mut refused) = (0, 0);
    // one thread per fixture: the runs are independent
    let fx = fixtures();
    let results: Vec<(String, (u32, u32, Kinds))> = std::thread::scope(|s| {
        let hs: Vec<_> = fx
            .iter()
            .map(|(name, bytes)| s.spawn(move || (name.clone(), run_fixture(name, bytes))))
            .collect();
        hs.into_iter()
            .map(|h| h.join().unwrap_or_else(|e| std::panic::resume_unwind(e)))
            .collect()
    });
    let mut kinds = Kinds::default();
    for (name, (o, r, k)) in results {
        kinds.session += k.session;
        kinds.clear += k.clear;
        kinds.dynamic += k.dynamic;
        kinds.rowcol += k.rowcol;
        println!("{name}: {o} ok, {r} refused");
        // every fixture runs a share of its ops: a guard that refuses all of
        // them hides in the total. Two fixtures have sheets without `r`, which
        // an edit refuses (`MissingReference`) nearly always, so they are
        // exempt (PR 162 review, round 1 m5).
        let exempt = ["59746_NoRowNums", "InlineString"]
            .iter()
            .any(|e| name.contains(e));
        if !exempt {
            assert!(
                o >= cases(),
                "{name}: only {o} ops accepted ({r} refused) in {} cases",
                cases()
            );
        }
        ok += o;
        refused += r;
    }
    println!("total: {ok} ok, {refused} refused");
    assert!(ok > 1000 && refused > 100, "ok {ok}, refused {refused}");
    println!(
        "accepted: session xf {}, ClearValue {}, DynamicFormula {}, row/col {}",
        kinds.session, kinds.clear, kinds.dynamic, kinds.rowcol
    );
    // the newer op groups must keep going through (PR 165 review, round 1 m2)
    assert!(
        kinds.session > SESSION_FLOOR
            && kinds.clear > 0
            && kinds.dynamic > 0
            && kinds.rowcol > 0,
        "accepted ops of the newer groups: session {}, clear {}, dynamic {}, row/col {}",
        kinds.session,
        kinds.clear,
        kinds.dynamic,
        kinds.rowcol
    );
}

/// P is a function of the content: a package and its save without edits
/// project alike (a GetPut sanity check on P, self-test (p)).
#[test]
fn p_is_unchanged_by_a_save_without_edits() {
    for (name, bytes) in fixtures() {
        let saved = Workbook::from_bytes(bytes.clone())
            .and_then(|wb| wb.to_bytes())
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        if let Some(d) = first_diff(&project(&bytes), &project(&saved)) {
            panic!("{name}: P differs after a save without edits: {d}");
        }
    }
}

// ---- the metadata fixture (§4.2) ------------------------------------------

#[test]
fn meta_fixture_pairs() {
    let base = meta_workbook();
    let mut p = Pair::new(&base);
    // a content write on the rich-value cell
    assert!(matches!(
        p.step(num("E2", 1.0)),
        Err(Error::RichValueCell { .. })
    ));
    // a write into a dynamic array's result cell
    assert!(matches!(
        p.step(num("F3", 1.0)),
        Err(Error::ArrayResultCell { .. })
    ));
    // the new content calls refuse the same way on both sides
    assert!(matches!(
        p.step(c1("E2", CellCall::ClearValue)),
        Err(Error::RichValueCell { .. })
    ));
    assert!(matches!(
        p.step(c1("F3", CellCall::ClearValue)),
        Err(Error::ArrayResultCell { .. })
    ));
    let _ = p.step(c1("E2", CellCall::DynamicFormula("1+1".into())));
    // row and column properties go through on both sides
    p.step(OpInput { sheet: 0, kind: OpKind::Row { row: 1, call: RowCall::Height(20.0) } })
        .expect("a row height");
    p.step(OpInput { sheet: 0, kind: OpKind::Cols { first: 4, last: 5, call: ColCall::Width(12.0) } })
        .expect("a column width");
    // a style call keeps `vm`
    p.step(c1("E2", CellCall::Font(bold())))
        .expect("set_font on E2");
    p.finish();
    // the font is the only change P sees, and `vm` is still resolved
    let d = first_diff(&project(&base), &project(&p.seq.to_bytes().unwrap()))
        .expect("a font changes P");
    assert!(
        d.starts_with("xl/worksheets/sheet1.xml/cell/1,4:"),
        "{}",
        &d[..80]
    );
    assert!(d.contains("XLRICHVALUE"), "vm is resolved on both sides");
}

#[test]
fn meta_fixture_formula_without_ca_differs_from_the_base() {
    let base = meta_workbook();
    let mut p = Pair::new(&base);
    p.step(formula("E5", "A1+0")).expect("set_formula on E5");
    p.finish();
    // `ca` is gone on both sides alike, and P sees it, on that cell
    let d = first_diff(&project(&base), &project(&p.j.shadow().to_bytes().unwrap()))
        .expect("losing ca is a difference");
    assert!(d.starts_with("xl/worksheets/sheet1.xml/cell/4,4:"), "{d}");
    assert!(d.contains("ca: true"), "the Base side has ca: {d}");
}
