//! Cell edits in formula-bearing workbooks (ADR-0003 §3–§4): plan and
//! validate without changing anything, then apply in place in a fixed order.
//! A failure while applying is a bug (the plan proved success); it poisons
//! the workbook so that no partial edit can be saved (§3.1).

use std::collections::{HashMap, HashSet};

use ooxml_core::{ElementSpec, QName, RawNode, XmlDocument};
use ooxml_schema::sml;

use crate::cell_state::{FormulaState, rich_state_of_runs};
use crate::census::{FCell, FKind, SheetKind};
use crate::error::{Error, Result};
use crate::journal::state::{CellState, Content, state_of};
use crate::formula::parse::parse;
use crate::formula::precedents::Rect;
use crate::formula::structural::{Axis, Shift};
use crate::formula::translate::translate;
use crate::graph::{Graph, precedents_of};
use crate::reference::{CellIndex, CellRange, MAX_COLS, MAX_ROWS};
use crate::registry::strings::{SharedStrings, StringPlan};
use crate::registry::styles::{StylePlan, StyleRequest, Styles};
use crate::workbook::{CalcPolicy, REL_CALC_CHAIN, Workbook};
use crate::index::{Located, SheetIndex, located_cell, sheet_data};
use crate::worksheet::{TargetPlan, locate, plan_target, write_cell};

pub(crate) type FormulaCache = Option<(sml::CellValues, String)>;

/// What a setter writes into the target cell.
#[derive(Clone, Debug)]
pub(crate) enum CellOp {
    /// `<v>` text of a number.
    Number(String),
    /// Explicit UTC ISO 8601 date text (`t="d"`).
    Date(String),
    /// Inline-string text, already `_xHHHH_`-escaped.
    Text(String),
    /// Formula text, without `=`.
    Formula(String, FormulaCache),
    ArrayFormula(String, CellRange, Option<u32>, FormulaCache),
    SharedFormula(String, u32, Option<CellRange>, FormulaCache),
    /// Uncached dynamic array (ADR-0024 C1 plan entry; the Journal path).
    /// [`CellOp::ArrayFormula`] with `Some(cm)` is the cached form.
    DynamicFormula(String, u32),
    RemoveFormula,
    /// Removes the value and formula (`v`, `is`, `f`, `t`) of an existing
    /// cell, keeping its style: `merge_clearing` (ADR-0014 §3). Unlike
    /// `RemoveFormula`, it also applies to a cell without a formula.
    Clear,
    /// The public `clear_value` (ADR-0024 P1): as `Clear`, but a cell that
    /// does not exist is left alone (nothing is created), as `RemoveFormula`
    /// leaves a cell without a formula.
    ClearValue,
    /// `t="s"` with this shared-string index (Phase2).
    Shared(u32),
    /// `t="b"`.
    Bool(bool),
    /// `t="e"` with this error value (`#N/A`, …).
    Error(&'static str),
    /// `t="inlineStr"` with rich runs (`is/r`), for a workbook without a
    /// shared-string table (ADR-0006 §4.6).
    InlineRich(Vec<crate::registry::strings::RichRun>),
    /// Only the cell's `s` changes (Phase2).
    StyleOnly,
}

/// Phase2 parts of an edit (ADR-0006): a shared-string plan made by the
/// caller and a style request.
#[derive(Debug, Default)]
pub(crate) struct Extra {
    pub(crate) string: Option<StringPlan>,
    pub(crate) style: Option<StyleRequest>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ClearingFormulaGroup {
    StaticArray(usize, CellIndex),
    Shared(usize, u32),
}

/// What the apply phase installs in the registries.
#[derive(Debug, Default)]
pub(crate) struct RegistryApply {
    strings: Vec<StringPlan>,
    /// Change in the number of `t="s"` cells (target and stripped cells).
    pub(crate) refs_delta: i64,
    pub(crate) refs_changed: bool,
    styles: Option<StylePlan>,
    patched_styles: HashMap<(usize, u32), (std::sync::Arc<crate::Format>, u32)>,
}

/// The cells an edit invalidates.
#[derive(Debug, Default)]
pub(crate) struct DepSet {
    pub(crate) formulas: Vec<usize>,
    /// (anchor id, current result range).
    pub(crate) results: Vec<(usize, Rect)>,
    /// Every dependent cell (formula cells and result cells).
    pub(crate) cells: Vec<(usize, CellIndex)>,
}

/// The shared-formula hand-off (§4.2).
#[derive(Debug)]
struct HandOff {
    new_master: usize,
    text: String,
    range: CellRange,
    loc: Located,
}

/// One cell of a [`CellWritePlan`].
#[derive(Debug)]
pub(crate) struct PlannedWrite {
    sheet: usize,
    part: String,
    at: CellIndex,
    op: CellOp,
    /// The formula the write replaces, if any.
    target: Option<usize>,
    style: Option<u32>,
}

/// The edited sheet and its old-to-new coordinate map. The staged
/// worksheet already reflects this map when cell targets are planned.
pub(crate) struct WriteIndexMap<'a> {
    pub(crate) sheet: usize,
    pub(crate) axis: Axis,
    pub(crate) shift: Shift,
    pub(crate) worksheet: &'a mut XmlDocument,
}

impl WriteIndexMap<'_> {
    fn predecessor(&self, at: CellIndex) -> Option<CellIndex> {
        let old = match self.axis {
            Axis::Row => CellIndex { row: self.shift.before(at.row)?, ..at },
            Axis::Column => CellIndex { col: self.shift.before(at.col)?, ..at },
        };
        CellIndex::new(old.row, old.col).ok()
    }

    fn successor(&self, at: CellIndex) -> Option<CellIndex> {
        Some(match self.axis {
            Axis::Row => CellIndex { row: self.shift.after(at.row, MAX_ROWS).ok()??, ..at },
            Axis::Column => CellIndex { col: self.shift.after(at.col, MAX_COLS).ok()??, ..at },
        })
    }
}

/// The Phase1 multi-cell write plan (ADR-0003 addendum P5-pre; ADR-0017
/// §2.1): everything one operation's cell writes need, validated together.
/// Its `calcPr` change and calcChain removal can go into a registry
/// transaction ([`Workbook::apply_writes_with`]); the rest is applied in
/// place.
#[derive(Debug)]
pub(crate) struct CellWritePlan {
    writes: Vec<PlannedWrite>,
    strip: StripPlan,
    handoffs: Vec<(String, HandOff)>,
    /// Whether the operation needs `calcPr/@fullCalcOnLoad`.
    full_calc_on_load: bool,
    /// `calcChain.xml`, to remove.
    chain: Option<String>,
    registry: RegistryApply,
    shifted_sheet: Option<usize>,
}

/// Positions of `c` elements to strip, per sheet.
#[derive(Debug, Default)]
pub(crate) struct StripPlan {
    pub(crate) sheets: Vec<(String, Vec<(usize, usize)>)>,
    /// Stripped cells that had `t="s"`.
    pub(crate) shared_refs: i64,
    fids: Vec<usize>,
    anchors: Vec<usize>,
}

/// C1's per-target report (ADR-0024 §7.4 C1): what R1's conditions (a/b/c)
/// are judged from. Computed from a plan over the unmutated workbook.
#[derive(Clone, Debug)]
pub(crate) struct TargetReport {
    pub(crate) sheet: usize,
    pub(crate) at: CellIndex,
    /// False only for a hand-off's new master (named by no op).
    pub(crate) named: bool,
    /// R1 (a): the `c` exists now (same lookup as the ClearValue skip).
    pub(crate) exists: bool,
    /// R1 (b): shared-group cell (master or member), array/data-table cell
    /// (anchor or result), or dynamic cell.
    pub(crate) group_or_array: bool,
    /// R1 (c), first half: the plan leaves the target's §4.4 state unchanged:
    /// `content_same && style_same` (two-half ops: both halves).
    pub(crate) state_same: bool,
    /// R1 (c), second half: every other part of the target's node is unchanged
    /// (h1: unknown attributes/children; `cm`/`vm`).
    pub(crate) node_same: bool,
    /// R1 (c), third half: nothing a later validation reads changes:
    /// `state_same && node_same && no hand-off touches the target`.
    pub(crate) validation_same: bool,
}

impl CellWritePlan {
    /// Reports every target: one per `PlannedWrite`, plus each hand-off's new
    /// master (from `HandOff.new_master`, resolved to `(sheet, at)` through
    /// the census). A cell both named and handed off (a `StyleOnly` write at
    /// the new master's position) is reported once, as named. Conditionally
    /// skipped cells have no `PlannedWrite` and so are not targets. Reads
    /// only; the plan did not mutate, so current state is pre-plan state.
    /// `Err` only for an inconsistent plan (a bug).
    ///
    /// Nothing may run between the plan and the report: R1's (b) flag
    /// reuses the plan guards' answer instead of re-scanning the census
    /// (an unshifted plan that accepted a non-style write refused result
    /// cells, so none is there; `StyleOnly` and hand-off masters, which
    /// the guards never judged, are scanned).
    pub(crate) fn report(&self, wb: &mut Workbook) -> Result<Vec<TargetReport>> {
        let masters: Vec<(usize, CellIndex)> = self
            .handoffs
            .iter()
            .map(|(_, h)| {
                wb.census
                    .get(h.new_master)
                    .map(|c| (c.sheet, c.at))
                    .ok_or_else(|| Error::Malformed("a hand-off names no cell".into()))
            })
            .collect::<Result<_>>()?;
        let unshifted = self.shifted_sheet.is_none();
        let mut out = Vec::with_capacity(self.writes.len() + masters.len());
        for w in &self.writes {
            let touched = masters.contains(&(w.sheet, w.at));
            // the guards judged every unshifted non-style write; anything
            // else is scanned below
            let guarded = unshifted && !matches!(w.op, CellOp::StyleOnly);
            out.push(report_write(wb, w, touched, guarded)?);
        }
        for (sheet, at) in &masters {
            if out
                .iter()
                .any(|r: &TargetReport| r.sheet == *sheet && r.at == *at)
            {
                continue;
            }
            out.push(report_master(wb, *sheet, *at)?);
        }
        Ok(out)
    }
}

/// The target's current cell: existence, §4.4 state, the R1 (b) flag, and a
/// clone of the `c` (reads need no tree borrow held across `state_of`).
/// With `guarded`, the plan's guards already refused result cells here,
/// so the census is not scanned again (see [`CellWritePlan::report`]).
fn current_cell(
    wb: &mut Workbook,
    sheet: usize,
    part: &str,
    at: CellIndex,
    guarded: bool,
) -> Result<(bool, CellState, bool, Option<RawNode>)> {
    wb.ensure_index(sheet)?;
    let node = {
        let doc = wb.pkg.xml_part(part)?;
        let index = wb.indexes.get(&sheet).expect("built");
        let loc = locate(doc, index, part, at)?;
        located_cell(doc, &loc).cloned()
    };
    let state = state_of(wb, sheet, at, node.as_ref())?;
    let grouped = wb.census.at(sheet, at).and_then(|f| wb.census.get(f)).is_some_and(|c| {
        !matches!(c.kind, FKind::Normal) || c.dynamic
    }) || (!guarded && wb.census.result_anchor(sheet, at).is_some());
    Ok((node.is_some(), state, grouped, node))
}

/// The report entry of one planned write.
fn report_write(
    wb: &mut Workbook,
    w: &PlannedWrite,
    touched: bool,
    guarded: bool,
) -> Result<TargetReport> {
    let (exists, state, group_or_array, node) = current_cell(wb, w.sheet, &w.part, w.at, guarded)?;
    let (content_same, style_same) = content_style_same(w, &state, node.as_ref())?;
    // always the conjunction (two-half ops: both halves), so a mutant
    // dropping either conjunct is natural
    let state_same = content_same && style_same;
    // `write_cell` keeps unknown attributes/children and `cm`/`vm` on every
    // path but the `cm`-setting ones (audit V2)
    let node_same = !matches!(w.op, CellOp::DynamicFormula(..) | CellOp::ArrayFormula(_, _, Some(_), _));
    Ok(TargetReport {
        sheet: w.sheet,
        at: w.at,
        named: true,
        exists,
        group_or_array,
        state_same,
        node_same,
        validation_same: state_same && node_same && !touched,
    })
}

/// The report entry of a hand-off's new master: the hand-off rewrites its
/// `f`, so its state is never the plan's (R1 (b) vetoes it anyway: it is a
/// shared member).
fn report_master(wb: &mut Workbook, sheet: usize, at: CellIndex) -> Result<TargetReport> {
    let part = wb
        .sheets
        .get(sheet)
        .and_then(|s| s.part.clone())
        .ok_or_else(|| Error::Malformed("a hand-off names no sheet".into()))?;
    // the guards never judged the new master: scan
    let (exists, _, group_or_array, _) = current_cell(wb, sheet, &part, at, false)?;
    Ok(TargetReport {
        sheet,
        at,
        named: false,
        exists,
        group_or_array,
        state_same: false,
        node_same: true,
        validation_same: false,
    })
}

/// Whether the op leaves the target's content and style (§4.4) as `state`
/// holds them (`s`: missing = 0, as `state_of` reads it). `node` is the
/// target's current `c`, if any. Reads only.
fn content_style_same(
    w: &PlannedWrite,
    state: &CellState,
    node: Option<&RawNode>,
) -> Result<(bool, bool)> {
    let style_same = w.style.is_none_or(|s| s == state.s);
    let content_same = match &w.op {
        // A10: the literal `v` text (conservative: `5.0` over `5` applies)
        CellOp::Number(t) => {
            matches!(&state.content, Content::Value { t: ct, v } if ct == "n" && v == t)
        }
        CellOp::Text(e) => {
            let single_t = node
                .and_then(|c| c.elements().find(|e| e.is(sml::InlineString::QNAME)))
                .is_some_and(|is| {
                    let mut t = is.elements().filter(|e| e.name().local() == "t");
                    matches!((t.next(), t.next()), (Some(_), None))
                });
            single_t
                && matches!(&state.content, Content::Str { inline: true, text, string }
                    if text == e && string.runs.is_none() && string.phonetic.is_none())
        }
        // the same item (`sst` is append-only); the `v` spelling is
        // compared literally (C5: `00` over `0` applies, as sequentially)
        CellOp::Shared(i) => node.is_some_and(|c| {
            c.attr(sml::Cell::ATTR_T) == Some("s")
                && c.elements()
                    .find(|e| e.is(sml::CellValue::QNAME))
                    .is_some_and(|v| v.text() == i.to_string())
        }),
        CellOp::Bool(b) => {
            let want = if *b { "1" } else { "0" };
            matches!(&state.content, Content::Value { t: ct, v } if ct == "b" && v == want)
        }
        CellOp::Error(e) => {
            matches!(&state.content, Content::Value { t: ct, v } if ct == "e" && v.as_str() == *e)
        }
        CellOp::InlineRich(runs) => {
            let text: String = runs.iter().map(|r| r.escaped.as_str()).collect();
            let predicted = rich_state_of_runs(runs)?;
            let no_extras = node
                .and_then(|c| c.elements().find(|e| e.is(sml::InlineString::QNAME)))
                .is_some_and(|is| {
                    let kids: Vec<_> = is.elements().collect();
                    kids.len() == runs.len() && kids.iter().all(|e| e.name().local() == "r")
                });
            no_extras
                && matches!(&state.content, Content::Str { inline: true, text: ct, string }
                    if ct == &text && string == &predicted)
        }
        // a plain `f` with text `t`: same text, normal kind, no `f`
        // attributes. `cm` is not compared (`write_cell` keeps it, so it is
        // trivially the same). A shared member's own text therefore differs
        // (its `t`/`si` attributes), so it applies and splits as sequentially.
        // A caller-supplied cache is never skipped: skipping would drop the
        // cache write (the Journal only plans uncached formulas).
        CellOp::Formula(t, cache) => {
            let plain = FormulaState {
                text: t.clone(),
                t: "normal".into(),
                reference: None,
                si: None,
                ca: false,
                aca: false,
                dt2d: false,
                dtr: false,
                del1: false,
                del2: false,
                bx: false,
                r1: None,
                r2: None,
                other: Vec::new(),
            };
            cache.is_none() && matches!(&state.content, Content::Formula { f, .. } if f == &plain)
        }
        // C3: never skipped (metadata-block identity is not compared)
        CellOp::DynamicFormula(..) => false,
        // New writers the Journal never plans (browser adapters): never
        // skipped, so a future plan input cannot silently drop them. These
        // arms are currently unreachable from journal/op.rs::plan_call, which
        // maps no call to these ops; they exist as fail-safe defaults.
        CellOp::Date(..) | CellOp::ArrayFormula(..) | CellOp::SharedFormula(..) => false,
        // C4: a target has a formula; removing `f` and the cache always changes content
        CellOp::RemoveFormula => false,
        // removes nothing from an already empty cell
        CellOp::Clear | CellOp::ClearValue => state.content == Content::Empty,
        CellOp::StyleOnly => true,
    };
    Ok((content_same, style_same))
}

/// Removes the cached value (`v`, `is`, `t`) of the planned cells of one
/// sheet tree (ADR-0003 §3.1; also used on Phase3's staged clones).
pub(crate) fn strip_cells(doc: &mut XmlDocument, positions: &[(usize, usize)]) -> Result<i64> {
    let mut shared = 0;
    for &(ri, ci) in positions {
        let ws = doc.lens_mut::<sml::Worksheet>()?;
        let sd = ws.into_child_by::<sml::SheetData>(|_| true).ok_or_else(|| Error::Malformed("sheetData".into()))?;
        let row = sd.into_child_at::<sml::Row>(ri).ok_or_else(|| Error::Malformed("planned row".into()))?;
        let cell = row.into_child_at::<sml::Cell>(ci).ok_or_else(|| Error::Malformed("planned cell".into()))?;
        shared += i64::from(cell.node().attr(sml::Cell::ATTR_T).is_some_and(|t| t.trim() == "s"));
        let mut c = sml::CellViewMut(cell);
        // every `v` and `is`, duplicates included (schema-invalid, but a
        // reader would see a survivor), as `write_cell` does (review R4-M1)
        while c.remove_cell_value() {}
        while c.remove_inline_string() {}
        c.remove_data_type()?;
    }
    Ok(shared)
}

fn set_full_calc_on_load(doc: &mut XmlDocument) -> Result<()> {
    let mut workbook = doc.lens_mut::<sml::Workbook>()?;
    let mut calc = workbook.get_or_insert::<sml::CalculationProperties>()?;
    calc.set_attr(sml::CalculationProperties::ATTR_FULL_CALC_ON_LOAD, "1")?;
    Ok(())
}

fn xml_char_ok(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..)
}

pub(crate) fn validate_formula_text(text: &str) -> Result<()> {
    let bad = |reason: String| Error::InvalidFormula { text: text.to_owned(), reason };
    if let Some(ch) = text.chars().find(|c| !xml_char_ok(*c)) {
        return Err(bad(format!("character U+{:04X} cannot be stored in XML", ch as u32)));
    }
    parse(text).map_err(|e| bad(format!("{} at byte {}", e.message, e.at)))?;
    match crate::formula::future::check(text) {
        Ok(()) => Ok(()),
        Err(crate::formula::future::Refusal::MissingPrefix { name, prefix }) => Err(Error::MissingFunctionPrefix { name, prefix }),
        Err(crate::formula::future::Refusal::Operator { operator, file_form }) => Err(Error::DynamicArrayOperator { operator, file_form }),
    }
}

fn registry_cell_state(doc: &XmlDocument, at: CellIndex, target: &TargetPlan) -> (bool, u32) {
    match located_cell(doc, &target.loc) {
        Some(c) => (
            c.attr(sml::Cell::ATTR_T).is_some_and(|t| t.trim() == "s"),
            c.attr(sml::Cell::ATTR_S).map(|value| ooxml_core::value::parse_u32(value).unwrap_or(0))
                .unwrap_or_else(|| missing_cell_style(doc, &target.loc, at)),
        ),
        None => (false, missing_cell_style(doc, &target.loc, at)),
    }
}

impl Workbook {
    /// Whether the workbook requests full recalculation on open.
    pub fn full_calc_on_load(&self) -> bool {
        self.calc.full_calc_on_load
    }

    /// Sets the workbook's recalculation-on-open flag.
    pub fn set_full_calc_on_load(&mut self, enabled: bool) -> Result<()> {
        self.check_editable()?;
        if enabled == self.calc.full_calc_on_load {
            return Ok(());
        }
        let part = self.workbook_part.clone();
        self.poisoning(|wb| {
            let doc = wb.pkg.xml_part_mut(&part)?;
            if enabled {
                set_full_calc_on_load(doc)?;
            } else {
                let mut root = doc.lens_mut::<sml::Workbook>()?;
                if let Some(mut calc) = root.child_mut::<sml::CalculationProperties>() {
                    calc.remove_attr(sml::CalculationProperties::ATTR_FULL_CALC_ON_LOAD)?;
                }
            }
            wb.calc.full_calc_on_load = enabled;
            Ok(())
        })
    }

    fn graph(&mut self) -> &Graph {
        if self.graph.is_none() {
            self.graph = Some(Graph::build(&self.env, &self.census));
        }
        self.graph.as_ref().expect("built")
    }

    /// Dependents of a write to `sheet!at` (§3.2).
    pub(crate) fn compute_dependents(&mut self, sheet: usize, at: CellIndex) -> DepSet {
        self.dependents_of_seeds(&[], vec![Rect::cell(sheet, at.row, at.col)])
    }

    /// Dependents of a structural edit (ADR-0009 §12): the formulas `fids`
    /// themselves (their text or meaning changes), everything reading them
    /// or the `rects`, and the closure of every `Everything` formula.
    pub(crate) fn dependents_of_seeds(&mut self, fids: &[usize], mut rects: Vec<Rect>) -> DepSet {
        self.graph();
        let g = self.graph.as_ref().expect("built");
        for &fid in fids {
            if let Some(c) = self.census.get(fid) {
                rects.push(Rect::cell(c.sheet, c.at.row, c.at.col));
                if let Some(r) = c.result_range() {
                    rects.push(Rect { sheet: c.sheet, r1: r.first.row, c1: r.first.col, r2: r.last.row, c2: r.last.col });
                }
            }
        }
        let mut d = g.dependents(&self.census, rects);
        for &fid in fids {
            if self.census.get(fid).is_some() && !d.formulas.contains(&fid) {
                d.formulas.push(fid);
                if let Some(r) = self.census.get(fid).and_then(|c| c.result_range().map(|r| (c.sheet, r))) {
                    let (sheet, r) = r;
                    d.results.push((fid, Rect { sheet, r1: r.first.row, c1: r.first.col, r2: r.last.row, c2: r.last.col }));
                }
            }
        }
        let mut cells = Vec::new();
        for &fid in &d.formulas {
            let c = self.census.get(fid).expect("alive");
            cells.push((c.sheet, c.at));
        }
        for (_, r) in &d.results {
            for row in r.r1..=r.r2 {
                for col in r.c1..=r.c2 {
                    cells.push((r.sheet, CellIndex { row, col }));
                }
            }
        }
        DepSet { formulas: d.formulas, results: d.results, cells }
    }

    /// Activates the shared-string registry if the workbook has a
    /// shared-string part; `false` if it has none (ADR-0006 §4.6).
    pub(crate) fn shared_strings_active(&mut self) -> Result<bool> {
        self.check_poisoned()?;
        if self.registries.strings.is_none() {
            self.registries.strings = SharedStrings::activate(self)?;
        }
        Ok(self.registries.strings.is_some())
    }

    /// The style registry, activated on first use.
    pub(crate) fn styles(&mut self) -> Result<&Styles> {
        self.check_poisoned()?;
        if self.registries.styles.is_none() {
            self.registries.styles = Some(Styles::activate(self)?);
        }
        Ok(self.registries.styles.as_ref().expect("activated"))
    }

    /// Does some formula or defined name use a function whose result can
    /// depend on a cell's format (`CELL`, `INFO`, `GET.CELL`,
    /// `GET.FORMULA`), or a function that is not on Phase1's allowlist
    /// (ADR-0006 §7)? Then a style-only write is handled like a value write.
    pub(crate) fn style_sensitive(&mut self) -> Result<bool> {
        if let Some(v) = self.style_sensitive {
            return Ok(v);
        }
        let v = self.scan_style_sensitive()?;
        self.style_sensitive = Some(v);
        Ok(v)
    }

    /// One pass over the census's formula texts and the defined names.
    fn scan_style_sensitive(&mut self) -> Result<bool> {
        use crate::formula::lexer::{TokKind, lex};
        let sensitive = |text: &str| match lex(text) {
            Err(_) => true,
            Ok(toks) => toks.iter().any(|t| match &t.kind {
                TokKind::Func(name) => {
                    matches!(name.as_str(), "CELL" | "INFO" | "GET.CELL" | "GET.FORMULA")
                        || crate::formula::functions::rule(name).is_none()
                }
                TokKind::Opaque => false,
                _ => false,
            }),
        };
        if self.census.alive().any(|(_, c)| sensitive(&c.text)) {
            return Ok(true);
        }
        let wb = sml::WorkbookView(self.pkg.xml_part(&self.workbook_part)?.lens::<sml::Workbook>()?);
        Ok(wb.defined_names().is_some_and(|dn| dn.defined_name_iter().any(|d| sensitive(&d.0.text()))))
    }

    /// Plans and applies one cell edit.
    pub(crate) fn edit_cell(&mut self, sheet: usize, at: CellIndex, op: CellOp) -> Result<()> {
        self.edit_cell_ext(sheet, at, op, Extra::default())
    }

    /// Plans and applies one cell edit with Phase2 registry work.
    pub(crate) fn edit_cell_ext(&mut self, sheet: usize, at: CellIndex, op: CellOp, extra: Extra) -> Result<()> {
        self.write_with_headers(vec![(sheet, at, op, extra)])
    }

    /// Runs all the cell checks without applying the plan.
    pub(crate) fn check_edit_cell(
        &mut self,
        sheet: usize,
        at: CellIndex,
        op: CellOp,
    ) -> Result<()> {
        self.plan_cell_write(sheet, at, op, Extra::default())
            .map(|_| ())
    }

    /// Plans one cell edit: validates without applying (C1's plan entry for
    /// a single write). Header renames are not staged (a header write
    /// refuses), so [`edit_cell_ext`](Self::edit_cell_ext) plans through
    /// [`write_with_headers`](Self::write_with_headers)'s split instead.
    /// Slice-B note: the Journal must plan single writes via
    /// `plan_with_headers` for the same reason, never via this fn (a
    /// header-cell write refuses here but renames through the funnel).
    pub(crate) fn plan_cell_write(
        &mut self,
        sheet: usize,
        at: CellIndex,
        op: CellOp,
        extra: Extra,
    ) -> Result<CellWritePlan> {
        self.plan_writes(vec![(sheet, at, op, extra)])
    }

    /// Validates all writes, dependent cache strips and registry changes
    /// before applying any of them (ADR-0003 §3.1a).
    pub(crate) fn plan_writes(&mut self, writes: Vec<(usize, CellIndex, CellOp, Extra)>) -> Result<CellWritePlan> {
        self.plan_table_writes(writes, &HashSet::new(), false)
    }

    pub(crate) fn plan_table_writes(
        &mut self,
        writes: Vec<(usize, CellIndex, CellOp, Extra)>,
        headers: &HashSet<(usize, CellIndex)>,
        new_table: bool,
    ) -> Result<CellWritePlan> {
        self.plan_cell_writes(writes, headers, new_table, None, RegistryApply::default(), None)
    }

    /// Plans writes at post-edit coordinates. With an index map, every write
    /// must target the structurally edited sheet. The caller stages that
    /// worksheet and plans shared-formula materialization, dependent cache
    /// strips and calcChain removal.
    /// Changes to shared strings are carried forward in the same registry.
    pub(crate) fn plan_writes_after(
        &mut self,
        edit: Option<WriteIndexMap<'_>>,
        writes: Vec<(usize, CellIndex, CellOp, Extra)>,
        headers: &HashSet<(usize, CellIndex)>,
        registry: RegistryApply,
    ) -> Result<CellWritePlan> {
        if let Some(edit) = &edit && writes.iter().any(|(sheet, ..)| *sheet != edit.sheet) {
            return Err(Error::Malformed("writes composed with a structural edit must target the edited sheet".into()));
        }
        self.plan_cell_writes(writes, headers, false, edit, registry, None)
    }

    /// Clears the stored cells of an exact static array in one write plan.
    pub(crate) fn clear_static_array_row(&mut self, sheet: usize, range: CellRange) -> Result<()> {
        let exact = self.census.at(sheet, range.first).and_then(|id| self.census.get(id))
            .is_some_and(|formula| formula.kind == FKind::Array && !formula.dynamic && formula.range == Some(range));
        if !exact || range.first.row != range.last.row {
            return Err(Error::ArrayAnchor { cell: range.first.to_string() });
        }
        let positions = self.stored_cells_in_static_array_row(sheet, range)?;
        let writes = positions.into_iter().map(|at| (sheet, at, CellOp::Clear, Extra::default())).collect();
        let plan = self.plan_cell_writes(writes, &HashSet::new(), false, None, RegistryApply::default(),
            Some(ClearingFormulaGroup::StaticArray(sheet, range.first)))?;
        self.apply_writes(plan)
    }

    /// Clears the cells of one verified shared group in one write plan.
    pub(crate) fn clear_shared_formula_row(
        &mut self, sheet: usize, master: CellIndex, columns: &[u32],
    ) -> Result<()> {
        let error = |reason: &str| Error::SharedFormulaMaster { cell: master.to_string(), reason: reason.into() };
        let Some(formula) = self.census.at(sheet, master).and_then(|id| self.census.get(id)) else {
            return Err(error("expected a shared formula master"));
        };
        let FKind::Shared { si, master: true } = formula.kind else {
            return Err(error("expected a shared formula master"));
        };
        let range = formula.range.filter(|range| range.first == master && range.last.row == master.row)
            .ok_or_else(|| error("the shared formula group must be contained in one row"))?;
        if formula.ambiguous_group {
            return Err(error("ambiguous shared formula group"));
        }
        let mut expected = columns.iter().copied().collect::<HashSet<_>>();
        if expected.len() != columns.len() {
            return Err(error("duplicate shared formula members"));
        }
        let mut positions = vec![master];
        for (_, member) in self.census.alive().filter(|(_, cell)| cell.sheet == sheet &&
            cell.kind == FKind::Shared { si, master: false })
        {
            if !range.contains(member.at) || !expected.remove(&member.at.col) {
                return Err(error("expected every shared formula member within its range"));
            }
            positions.push(member.at);
        }
        if !expected.is_empty() {
            return Err(error("unexpected shared formula members"));
        }
        let writes = positions.into_iter().map(|at| (sheet, at, CellOp::Clear, Extra::default())).collect();
        let plan = self.plan_cell_writes(writes, &HashSet::new(), false, None, RegistryApply::default(),
            Some(ClearingFormulaGroup::Shared(sheet, si)))?;
        self.apply_writes(plan)
    }

    fn stored_cells_in_static_array_row(&mut self, sheet: usize, range: CellRange) -> Result<Vec<CellIndex>> {
        self.ensure_index(sheet)?;
        let part = self.sheets[sheet].part.as_deref().expect("worksheet has a part");
        let index = self.indexes.get(&sheet).expect("indexed worksheet");
        let rows = index.target_rows(part)?;
        let position = rows.binary_search_by_key(&(range.first.row + 1), |entry| entry.row)
            .map_err(|_| Error::Malformed(format!("{part}: array anchor row is missing")))?;
        Ok(index.target_cells(&rows[position], part)?.iter()
            .map(|&(col, _)| CellIndex { row: range.first.row, col })
            .filter(|&at| range.contains(at))
            .collect())
    }

    fn plan_cell_writes(
        &mut self,
        writes: Vec<(usize, CellIndex, CellOp, Extra)>,
        headers: &HashSet<(usize, CellIndex)>,
        new_table: bool,
        mut edit: Option<WriteIndexMap<'_>>,
        mut registry: RegistryApply,
        clearing_group: Option<ClearingFormulaGroup>,
    ) -> Result<CellWritePlan> {
        self.check_editable()?;
        let staged_index = edit.as_ref().map(|edit| SheetIndex::build(edit.worksheet));
        let mut appends = registry.strings.iter().filter_map(StringPlan::reservation).collect();

        // A shared group can be replaced together when all of its members
        // receive shared formula writes. No old member then needs a hand-off.
        let shared_targets: HashSet<_> = writes.iter()
            .filter(|(_, _, op, _)| matches!(op, CellOp::SharedFormula(..)))
            .map(|(sheet, at, _, _)| (*sheet, *at))
            .collect();
        let mut replaced_groups = HashMap::new();
        if !shared_targets.is_empty() && edit.is_none() {
            for (_, formula) in self.census.alive() {
                if let FKind::Shared { si, .. } = formula.kind {
                    let covered = !formula.ambiguous_group && shared_targets.contains(&(formula.sheet, formula.at));
                    *replaced_groups.entry((formula.sheet, si)).or_insert(true) &= covered;
                }
            }
        }

        let mut planned: Vec<PlannedWrite> = Vec::with_capacity(writes.len());
        let mut seen: HashSet<(usize, CellIndex)> = HashSet::new();
        let mut groups: HashSet<(usize, u32)> = HashSet::new();
        let mut handoffs = Vec::new();
        let mut deps = if new_table { self.dependents_of_seeds(&[], Vec::new()) } else { DepSet::default() };
        let mut dep_formulas: HashSet<_> = deps.formulas.iter().copied().collect();
        let mut dep_results: HashSet<_> = deps.results.iter().map(|(fid, _)| *fid).collect();
        let mut dep_cells: HashSet<_> = deps.cells.iter().copied().collect();
        let mut formula_set_changes = false;
        for (sheet, at, mut op, mut extra) in writes {
            let part = self.sheets[sheet].part.clone().expect("worksheet");
            let cell = at.to_string();
            if !seen.insert((sheet, at)) {
                return Err(Error::DuplicateCellWrite { cell });
            }

            let unshifted = edit.is_none();
            let shifted = edit.as_mut().filter(|edit| edit.sheet == sheet);
            let before = shifted.as_ref().map_or(Some(at), |edit| edit.predecessor(at));
            let style_only = matches!(op, CellOp::StyleOnly);
            // ---- guards on value and formula writes ----
            let target = if style_only { None } else { before.and_then(|old| self.census.at(sheet, old)) };
            if !style_only {
                self.load_context(sheet)?;
                let ctx = self.contexts.get(&sheet).expect("context loaded with the worksheet");
                if let Some(m) = before.and_then(|old| ctx.merges.iter().find(|m| m.contains(old) && m.first != old)) {
                    return Err(Error::MergedCell { cell, range: m.to_string() });
                }
                if !headers.contains(&(sheet, at)) && let Some(t) = ctx.tables.iter().find(|t| t.rows.is_some_and(|r| before.is_some_and(|old| r.contains(old)))) {
                    return Err(Error::TableHeaderCell { cell, table: t.name.clone() });
                }
                if let Some(fid) = target {
                    let c = self.census.get(fid).expect("alive");
                    if (matches!(c.kind, FKind::Array | FKind::DataTable) || c.dynamic) &&
                        clearing_group != Some(ClearingFormulaGroup::StaticArray(sheet, c.at)) {
                        return Err(Error::ArrayAnchor { cell });
                    }
                }
                if let Some(a) = before.and_then(|old| self.census.result_anchor(sheet, old)) {
                    let old = self.census.get(a).expect("alive").at;
                    if clearing_group != Some(ClearingFormulaGroup::StaticArray(sheet, old))
                        && let Some(anchor) = shifted.as_ref().map_or(Some(old), |edit| edit.successor(old))
                    {
                        return Err(Error::ArrayResultCell { cell, anchor: anchor.to_string() });
                    }
                }
                if let CellOp::ArrayFormula(_, range, ..) = &op {
                    let overlaps = |other: CellRange| {
                        range.first.row <= other.last.row && other.first.row <= range.last.row
                            && range.first.col <= other.last.col && other.first.col <= range.last.col
                    };
                    if let Some(merged) = ctx.merges.iter().find(|m| overlaps(**m)) {
                        return Err(Error::MergedCell { cell, range: merged.to_string() });
                    }
                    if let Some((_, other)) = self.census.alive().find(|(_, f)| {
                        f.sheet == sheet && (f.at != at && range.contains(f.at)
                            || (matches!(f.kind, FKind::Array | FKind::DataTable) || f.dynamic)
                                && f.range.is_some_and(overlaps))
                    }) {
                        return Err(Error::ArrayResultCell { cell, anchor: other.at.to_string() });
                    }
                }
                if let CellOp::Formula(text, _) | CellOp::DynamicFormula(text, _) | CellOp::ArrayFormula(text, ..) | CellOp::SharedFormula(text, _, Some(_), _) = &op {
                    validate_formula_text(text)?;
                }
                if matches!(op, CellOp::RemoveFormula) && target.is_none() {
                    continue;
                }
                if matches!(op, CellOp::ClearValue) && unshifted {
                    self.ensure_index(sheet)?;
                    if self.indexes.get(&sheet).expect("built").locate(&part, at)?.cell.is_none() {
                        continue;
                    }
                }
            }
            if let Some(string) = &mut extra.string {
                let reg = self.registries.strings.as_ref().ok_or_else(|| Error::Malformed("shared-string plan without a registry".into()))?;
                reg.reserve(string, &mut appends);
                op = CellOp::Shared(string.index);
            }

            // ---- the target in Layer 2 (validated here; re-located at apply) ----
            let (doc, index) = match shifted {
                Some(edit) => (&mut *edit.worksheet, staged_index.as_ref().expect("staged sheet index")),
                None => {
                    self.ensure_index(sheet)?;
                    (self.pkg.xml_part_mut(&part)?, self.indexes.get(&sheet).expect("built"))
                }
            };
            let mut target_plan = plan_target(doc, index, &part, at, &op)?;
            let old_cell = registry_cell_state(doc, at, &target_plan);
            self.plan_registries(old_cell, &mut target_plan, &op, extra, &mut registry)?;
            formula_set_changes |= unshifted && match op {
                CellOp::Formula(..) => target.is_none(),
                CellOp::DynamicFormula(..) | CellOp::ArrayFormula(..) | CellOp::SharedFormula(..) => true,
                _ => target.is_some(),
            };

            // ---- shared groups: one cell per group per batch; hand-off (§4.2) ----
            if unshifted && let Some(fid) = target {
                let c = self.census.get(fid).expect("alive").clone();
                if let FKind::Shared { si, master } = c.kind
                    && replaced_groups.get(&(sheet, si)) != Some(&true)
                    && clearing_group != Some(ClearingFormulaGroup::Shared(sheet, si))
                {
                    if master && matches!(op, CellOp::DynamicFormula(..)) {
                        return Err(Error::SharedFormulaMaster { cell, reason: "dynamic array would replace a shared formula master".into() });
                    }
                    if master && matches!(op, CellOp::ArrayFormula(..)) {
                        return Err(Error::SharedFormulaMaster { cell, reason: "array would replace a shared formula master".into() });
                    }
                    if !groups.insert((sheet, si)) {
                        return Err(Error::SharedFormulaMaster {
                            cell,
                            reason: format!("another cell of shared group {si} is written in the same batch"),
                        });
                    }
                    if master && let Some(h) = self.plan_handoff(&c, si, sheet, &part)? {
                        handoffs.push((part.clone(), h));
                    }
                }
            }

            // ---- dependents (union) ----
            let d = if !unshifted {
                DepSet::default()
            } else if let CellOp::ArrayFormula(_, range, cm, _) = &op {
                let (r2, c2) = if cm.is_some() {
                    (MAX_ROWS - 1, MAX_COLS - 1)
                } else {
                    (range.last.row, range.last.col)
                };
                self.dependents_of_seeds(&[], vec![Rect { sheet, r1: at.row, c1: at.col, r2, c2 }])
            } else if matches!(op, CellOp::DynamicFormula(..)) {
                self.dependents_of_seeds(&[], vec![Rect { sheet, r1: at.row, c1: at.col, r2: MAX_ROWS - 1, c2: MAX_COLS - 1 }])
            } else if !style_only || self.style_sensitive()? {
                self.compute_dependents(sheet, at)
            } else {
                DepSet::default()
            };
            for fid in d.formulas {
                let f = self.census.get(fid).expect("alive");
                if (!style_only || (f.sheet, f.at) != (sheet, at)) && dep_formulas.insert(fid) {
                    deps.formulas.push(fid);
                }
            }
            for (fid, r) in d.results {
                if dep_results.insert(fid) {
                    deps.results.push((fid, r));
                }
            }
            for c in d.cells {
                if (!style_only || c != (sheet, at)) && dep_cells.insert(c) {
                    deps.cells.push(c);
                }
            }
            planned.push(PlannedWrite { sheet, part, at, op, target, style: target_plan.style });
        }
        // a shared member being written whose group's master is handed off
        // was refused above (one cell per group)
        let written: HashSet<_> = planned.iter().filter(|w| !matches!(w.op, CellOp::StyleOnly)).map(|w| (w.sheet, w.at)).collect();

        // ---- dependents' guards ----
        let has_other_dependents = self.check_write_dependents(&deps, &written, edit.as_ref())?;

        // ---- strip plan ----
        let strip = match self.policy {
            CalcPolicy::StripDependentCaches => self.plan_strip(&deps, &written)?,
            CalcPolicy::KeepStaleCaches if self.validate_strips => {
                // validity only: the strip itself is deferred (the Journal's)
                self.plan_strip(&deps, &written)?;
                StripPlan::default()
            }
            CalcPolicy::KeepStaleCaches => StripPlan::default(),
        };

        let needs_calc = edit.is_some() || new_table || has_other_dependents || formula_set_changes;
        let full_calc_on_load = needs_calc && !self.calc.manual && !self.calc.full_calc_on_load;

        // ---- calcChain ----
        let chain = if formula_set_changes { self.calc_chain.clone() } else { None };
        if let Some(p) = &chain {
            self.pkg.check_remove_part(p, &[REL_CALC_CHAIN])?;
        }
        registry.refs_delta -= strip.shared_refs;
        registry.refs_changed |= strip.shared_refs != 0;
        Ok(CellWritePlan {
            writes: planned, strip, handoffs, full_calc_on_load, chain, registry,
            shifted_sheet: edit.map(|edit| edit.sheet),
        })
    }

    // The census tracks pre-edit coordinates, and the writes target the staged sheet.
    pub(crate) fn check_write_dependents(
        &self,
        deps: &DepSet,
        written: &HashSet<(usize, CellIndex)>,
        edit: Option<&WriteIndexMap<'_>>,
    ) -> Result<bool> {
        let post_edit = |sheet, old| match edit.filter(|edit| edit.sheet == sheet) {
            Some(edit) => edit.successor(old),
            None => Some(old),
        };
        let others: Vec<_> = deps.cells.iter().copied().filter(|&(sheet, old)| {
            post_edit(sheet, old).is_some_and(|at| !written.contains(&(sheet, at)))
        }).collect();
        for &fid in &deps.formulas {
            let c = self.census.get(fid).expect("alive");
            if matches!(self.sheets[c.sheet].kind, SheetKind::MacroSheet | SheetKind::DialogSheet) {
                return Err(Error::MacroSheetDependent { sheet: self.sheets[c.sheet].name.clone(), cell: c.at.to_string() });
            }
        }
        if let Some(&(_, c)) = others.iter().find(|k| self.census.vm_cells.contains(k)) {
            return Err(Error::RichValueCell { cell: c.to_string() });
        }
        if self.calc.iterate && !others.is_empty() {
            return Err(Error::IterativeCalculation);
        }

        Ok(!others.is_empty())
    }

    /// Applies a plan on its own: the calcChain removal and `workbook.xml`
    /// in place, then the in-place part (§3.1 apply order).
    pub(crate) fn apply_writes(&mut self, plan: CellWritePlan) -> Result<()> {
        if plan.writes.is_empty() {
            return Ok(());
        }
        let staged = if plan.full_calc_on_load {
            let mut doc = self.pkg.xml_part(&self.workbook_part)?.clone();
            set_full_calc_on_load(&mut doc)?;
            Some(doc)
        } else {
            None
        };
        // Poisoned *before* apply and cleared only on success: a panic inside
        // apply (a bug) that a caller catches also leaves the workbook
        // poisoned (review R3-m3).
        self.poisoned = Some("an edit was interrupted while being applied".into());
        let result = self.apply_steps(plan, false, staged);
        self.finish_apply(result)
    }

    /// Applies a plan as part of a registry transaction (ADR-0017 §2.1):
    /// the `calcPr` change and the calcChain removal are folded into
    /// `registry` (a staged `workbook.xml`, `remove_parts`), which the
    /// caller built **after** [`Self::plan_writes`] with every other step
    /// of its operation; then `Package::apply` runs, then the in-place part.
    ///
    /// A refusal while folding is an ordinary `Err` (nothing changed).
    /// `StalePlan` from `Package::apply` is also an ordinary `Err`: it is
    /// returned before anything changes, so the poison is cleared.
    pub(crate) fn apply_writes_with(&mut self, plan: CellWritePlan, registry: ooxml_core::RegistryPlan) -> Result<()> {
        self.apply_writes_with_insertion(plan, registry, None, |_| Ok(()))
    }

    pub(crate) fn apply_writes_with_insertion(
        &mut self,
        plan: CellWritePlan,
        mut registry: ooxml_core::RegistryPlan,
        mut staged_workbook: Option<XmlDocument>,
        insert: impl FnOnce(&mut Self) -> Result<()>,
    ) -> Result<()> {
        if plan.full_calc_on_load {
            let doc = match &mut staged_workbook {
                Some(doc) => doc,
                None => staged_workbook.insert(self.pkg.xml_part(&self.workbook_part)?.clone()),
            };
            set_full_calc_on_load(doc)?;
        }
        if let Some(doc) = staged_workbook {
            registry.stage_part(&self.pkg, &self.workbook_part, doc)?;
        }
        if let Some(p) = &plan.chain {
            registry.remove_parts(&self.pkg, &[p.as_str()], &[REL_CALC_CHAIN])?;
        }
        self.apply_registry_edit(registry, |wb| {
            insert(wb)?;
            if let Some(sheet) = plan.shifted_sheet {
                let part = wb.sheets[sheet].part.as_ref().expect("worksheet");
                wb.indexes.insert(sheet, SheetIndex::build(wb.pkg.xml_part(part)?));
            }
            let structural = plan.shifted_sheet.is_some();
            wb.apply_steps(plan, true, None)?;
            if structural {
                wb.rebuild()?;
            }
            Ok(())
        })
    }

    /// Applies a validated registry transaction before the checked in-place edits.
    pub(crate) fn apply_registry_edit(
        &mut self,
        registry: ooxml_core::RegistryPlan,
        in_place: impl FnOnce(&mut Self) -> Result<()>,
    ) -> Result<()> {
        use crate::workbook::ApplyStepHook as Step;
        self.poisoned = Some("an edit was interrupted while being applied".into());
        #[cfg(feature = "fault-injection")]
        if self.fault == Some((Step::StaleRegistryPlan, false)) {
            // a step-1 ordering bug: the package changes after the plan
            self.fault = None;
            let wb = self.workbook_part.clone();
            self.pkg.xml_part_mut(&wb)?;
        }
        if let Err(e) = self.fault_point(Step::Registry) {
            return self.finish_apply(Err(e));
        }
        if let Err(e) = self.pkg.apply(registry) {
            // only StalePlan, returned before anything changed
            self.poisoned = None;
            return Err(e.into());
        }
        let result = in_place(self);
        self.finish_apply(result)
    }

    fn finish_apply(&mut self, result: Result<()>) -> Result<()> {
        match result {
            Ok(()) => {
                self.poisoned = None;
                Ok(())
            }
            Err(e) => {
                let msg = e.to_string();
                self.poisoned = Some(msg.clone());
                Err(Error::Poisoned(msg))
            }
        }
    }

    /// Plans the registry side of an edit: the style (on a staged clone of
    /// `styles.xml`), the shared-string append (dry-run checked) and the
    /// change in `t="s"` cells. Changes nothing.
    fn plan_registries(&mut self, (old_t_s, current_s): (bool, u32), target_plan: &mut TargetPlan, op: &CellOp, extra: Extra, registry: &mut RegistryApply) -> Result<()> {
        if let Some(req) = &extra.style {
            let patch = match req {
                StyleRequest::Patch(format) => Some(((std::sync::Arc::as_ptr(format) as usize, current_s), format)),
                _ => None,
            };
            target_plan.style = Some(if let Some((_, index)) = patch.and_then(|(key, _)| registry.patched_styles.get(&key)) {
                *index
            } else {
                self.styles()?;
                let current_plan = registry.styles.take();
                let st = self.registries.styles.take().expect("activated");
                let (base, staged) = match &current_plan {
                    Some(plan) => (&plan.next, plan.staged.as_ref()),
                    None => (&st, None),
                };
                let plan = base.plan(self, req, current_s, staged);
                self.registries.styles = Some(st);
                let plan = plan?;
                let index = plan.s;
                registry.styles = Some(plan);
                if let Some((key, format)) = patch {
                    // Retain the allocation so its identity cannot be reused during this plan.
                    registry.patched_styles.insert(key, (format.clone(), index));
                }
                index
            });
        }
        if !matches!(op, CellOp::StyleOnly) {
            let delta = i64::from(matches!(op, CellOp::Shared(_))) - i64::from(old_t_s);
            registry.refs_delta += delta;
            registry.refs_changed |= delta != 0;
        }
        if let Some(sp) = extra.string {
            let reg = self.registries.strings.as_ref().expect("reserved");
            let doc = self.pkg.xml_part(&reg.part)?;
            crate::worksheet::check_insertable::<sml::SharedStringTable>(doc.root().elements(), sml::SharedStringItem::QNAME)?;
            if sp.will_append() {
                registry.strings.push(sp);
            }
        }
        Ok(())
    }

    fn plan_handoff(&mut self, m: &FCell, si: u32, sheet: usize, part: &str) -> Result<Option<HandOff>> {
        let mut members: Vec<(usize, CellIndex)> = self
            .census
            .alive()
            .filter(|(_, c)| c.sheet == m.sheet && c.kind == FKind::Shared { si, master: false })
            .map(|(i, c)| (i, c.at))
            .collect();
        if m.ambiguous_group {
            return Err(Error::SharedFormulaMaster {
                cell: m.at.to_string(),
                reason: format!("two masters share si {si} on this sheet"),
            });
        }
        if members.is_empty() {
            return Ok(None);
        }
        members.sort_by_key(|&(_, at)| at);
        let (new_master, n) = members[0];
        let env = &self.env;
        let is_name = |t: &str| env.is_name(t);
        let fail = |reason: String| Error::SharedFormulaMaster { cell: m.at.to_string(), reason };
        let d = |a: CellIndex, b: CellIndex| (i64::from(b.row) - i64::from(a.row), i64::from(b.col) - i64::from(a.col));
        let (dr, dc) = d(m.at, n);
        let text = translate(&m.text, dr, dc, &is_name).map_err(|e| fail(e.to_string()))?;
        let mut range = CellRange { first: n, last: n };
        for &(_, at) in &members[1..] {
            let (dr, dc) = d(n, at);
            translate(&text, dr, dc, &is_name).map_err(|e| fail(format!("member {at}: {e}")))?;
            range = range.union(at);
        }
        let doc = self.pkg.xml_part(part)?;
        let loc = locate(doc, self.indexes.get(&sheet).expect("built"), part, n)?;
        let found = located_cell(doc, &loc)
            .and_then(|c| c.elements().find(|e| e.is(sml::CellFormula::QNAME)))
            .is_some();
        if !found {
            return Err(Error::Malformed(format!("{part}: shared formula member {n} not found")));
        }
        Ok(Some(HandOff { new_master, text, range, loc }))
    }

    /// One pass over each affected sheet's `sheetData`, finding every
    /// dependent that still has a cached value (§3.1, R2-m1). Cells the
    /// plan writes are not stripped: they get new content.
    pub(crate) fn plan_strip(&mut self, deps: &DepSet, written: &HashSet<(usize, CellIndex)>) -> Result<StripPlan> {
        let mut cells: HashMap<usize, HashSet<CellIndex>> = HashMap::new();
        let mut rects: HashMap<usize, Vec<Rect>> = HashMap::new();
        let mut plan = StripPlan::default();
        for &fid in &deps.formulas {
            let c = self.census.get(fid).expect("alive");
            if c.has_cache && !written.contains(&(c.sheet, c.at)) {
                cells.entry(c.sheet).or_default().insert(c.at);
                plan.fids.push(fid);
            }
        }
        for &(fid, r) in &deps.results {
            if !self.census.get(fid).expect("alive").results_stripped {
                rects.entry(r.sheet).or_default().push(r);
                plan.anchors.push(fid);
            }
        }
        let mut sheets: Vec<usize> = cells.keys().chain(rects.keys()).copied().collect();
        sheets.sort_unstable();
        sheets.dedup();
        for s in sheets {
            let part = self.sheets[s].part.clone().expect("formula sheet");
            let want = cells.remove(&s).unwrap_or_default();
            let rs = rects.remove(&s).unwrap_or_default();
            self.ensure_index(s)?;
            let doc = self.pkg.xml_part(&part)?;
            let index = self.indexes.get(&s).expect("built");
            let (positions, shared) = strip_positions(doc, index, &part, s, &want, &rs)?;
            plan.shared_refs += shared;
            if !positions.is_empty() {
                plan.sheets.push((part, positions));
            }
        }
        Ok(plan)
    }

    /// The in-place steps of §3.1, in order. With `registry_done`, the
    /// calcChain removal and `workbook.xml` were applied by
    /// `Package::apply`; only the in-memory state follows them.
    fn apply_steps(&mut self, plan: CellWritePlan, registry_done: bool, staged: Option<XmlDocument>) -> Result<()> {
        use crate::workbook::ApplyStepHook as Step;
        let CellWritePlan { writes, strip, handoffs, full_calc_on_load, chain, registry, shifted_sheet } = plan;
        if registry_done {
            if chain.is_some() {
                self.calc_chain = None;
            }
            if full_calc_on_load {
                self.calc.full_calc_on_load = true;
            }
        } else {
            // (i) calcChain
            self.fault_point(Step::RemoveCalcChain)?;
            if let Some(p) = &chain {
                self.pkg.remove_part(p, &[REL_CALC_CHAIN])?;
                self.calc_chain = None;
            }
            // (ii) workbook.xml
            self.fault_point(Step::CalcPr)?;
            if let Some(doc) = staged {
                *self.pkg.xml_part_mut(&self.workbook_part.clone())? = doc;
                self.calc.full_calc_on_load = true;
            }
        }
        // (iii) dependents, at the positions planned on the tree before any
        // write of this plan (writes come after)
        self.fault_point(Step::Strip)?;
        for (p, positions) in &strip.sheets {
            strip_cells(self.pkg.xml_part_mut(p)?, positions)?;
        }
        // (iv) shared-formula hand-offs
        self.fault_point(Step::HandOff)?;
        for (part, h) in &handoffs {
            let doc = self.pkg.xml_part_mut(part)?;
            let (Some(ri), Some(ci)) = (h.loc.row, h.loc.cell) else { return Err(Error::Malformed("hand-off".into())) };
            let ws = doc.lens_mut::<sml::Worksheet>()?;
            let sd = ws.into_child_by::<sml::SheetData>(|_| true).ok_or_else(|| Error::Malformed("sheetData".into()))?;
            let row = sd.into_child_at::<sml::Row>(ri).ok_or_else(|| Error::Malformed("planned row".into()))?;
            let cell = row.into_child_at::<sml::Cell>(ci).ok_or_else(|| Error::Malformed("planned cell".into()))?;
            let mut c = sml::CellViewMut(cell);
            let mut f = c.cell_formula_mut().ok_or_else(|| Error::Malformed("hand-off f".into()))?;
            f.set_reference(&h.range.to_string())?;
            f.set_text(&h.text)?;
        }
        // (v) Phase2 registry updates.
        self.fault_point(Step::Registries)?;
        if let Some(p) = registry.styles {
            if let Some(doc) = p.staged {
                *self.pkg.xml_part_mut(&p.next.part)? = doc;
            }
            self.registries.styles = Some(p.next);
        }
        SharedStrings::apply(self, &registry.strings, registry.refs_delta, registry.refs_changed)?;

        // (vi) the target cells, each located again on the current tree: an
        // earlier write of the plan may have inserted its row or cell
        for (k, w) in writes.iter().enumerate() {
            self.fault_point(if k == 0 { Step::Target } else { Step::LaterTarget })?;
            let index = self.indexes.get(&w.sheet).expect("built in plan_writes");
            let written = plan_target(self.pkg.xml_part_mut(&w.part)?, index, &w.part, w.at, &w.op).and_then(|mut tp| {
                tp.style = w.style;
                // planned above (so a malformed ref still refuses), not applied
                if self.freeze_dimension {
                    tp.dimension = None;
                }
                write_cell(self.pkg.xml_part_mut(&w.part)?, &tp, w.at, &w.op)
            });
            let effect = match written {
                Ok(effect) => effect,
                Err(e) => {
                    // The tree may have changed before the failure (unreachable
                    // after a successful plan): keep the index true to the tree,
                    // since reads still work on a poisoned workbook (PR #29 m4).
                    if let Ok(doc) = self.pkg.xml_part(&w.part) {
                        let index = SheetIndex::build(doc);
                        self.indexes.insert(w.sheet, index);
                    }
                    return Err(e);
                }
            };
            if let Some(index) = self.indexes.get_mut(&w.sheet) {
                effect.update(index, w.at);
            }
        }
        // (vii) the index
        self.fault_point(Step::Index)?;
        for &fid in &strip.fids {
            if let Some(Some(c)) = self.census.cells.get_mut(fid) {
                c.has_cache = false;
            }
        }
        for &fid in &strip.anchors {
            if let Some(Some(c)) = self.census.cells.get_mut(fid) {
                c.results_stripped = true;
            }
        }
        for (_, h) in handoffs {
            if let Some(Some(c)) = self.census.cells.get_mut(h.new_master) {
                if let FKind::Shared { si, .. } = c.kind {
                    c.kind = FKind::Shared { si, master: true };
                }
                c.text = h.text;
                c.range = Some(h.range);
            }
        }
        if shifted_sheet.is_some() {
            return Ok(());
        }
        for w in writes {
            if w.target.is_some() || matches!(w.op, CellOp::Formula(..) | CellOp::DynamicFormula(..) | CellOp::ArrayFormula(..) | CellOp::SharedFormula(..)) {
                self.style_sensitive = None;
            }
            if let Some(fid) = w.target {
                self.census.remove(fid);
            }
            let (text, kind, range, dynamic, has_cache) = match w.op {
                CellOp::DynamicFormula(text, _) => (text, FKind::Array, Some(CellRange { first: w.at, last: w.at }), true, false),
                CellOp::ArrayFormula(text, range, cm, cache) => (text, FKind::Array, Some(range), cm.is_some(), cache.is_some()),
                CellOp::Formula(text, cache) => (text, FKind::Normal, None, false, cache.is_some()),
                CellOp::SharedFormula(text, si, range, cache) => (
                    text, FKind::Shared { si, master: range.is_some() }, range, false, cache.is_some(),
                ),
                _ => continue,
            };
            {
                let c = FCell {
                    sheet: w.sheet,
                    at: w.at,
                    kind,
                    range,
                    volatile: false,
                    dynamic,
                    spill_error: false,
                    ambiguous_group: false,
                    has_cache,
                    results_stripped: false,
                    text,
                };
                let prec = self.graph.as_ref().map(|_| {
                    let census = &self.census;
                    precedents_of(&self.env, census, &c, &|s, si| census.master_of(s, si), &mut HashMap::new())
                });
                let fid = self.census.push(c);
                if let (Some(g), Some(p)) = (self.graph.as_mut(), prec) {
                    g.add(fid, p);
                }
            }
        }
        Ok(())
    }

    #[cfg(feature = "fault-injection")]
    pub(crate) fn fault_point(&mut self, step: crate::workbook::ApplyStepHook) -> Result<()> {
        if let Some((at, panic)) = self.fault
            && at == step
        {
            self.fault = None;
            assert!(!panic, "injected panic before {step:?}");
            return Err(Error::Malformed(format!("injected failure before {step:?}")));
        }
        Ok(())
    }

    #[cfg(not(feature = "fault-injection"))]
    #[inline(always)]
    pub(crate) fn fault_point(&mut self, _: crate::workbook::ApplyStepHook) -> Result<()> {
        Ok(())
    }
}

/// Positions (row item, cell item) in `sheetData` of the wanted cells that
/// have a `v`, `t` or `is`. Every wanted formula cell must be found.
fn strip_positions(
    doc: &XmlDocument,
    index: &SheetIndex,
    part: &str,
    sheet: usize,
    want: &HashSet<CellIndex>,
    rects: &[Rect],
) -> Result<(Vec<(usize, usize)>, i64)> {
    let mut out = Vec::new();
    let mut shared = 0i64;
    let mut found = HashSet::new();
    index.check_edit()?;
    let rows = index.rows(part)?;
    let sd = sheet_data(doc);
    // The rows to visit: those of wanted formula cells and those inside the
    // result rectangles, found by binary search in the index (not a pass
    // over every row).
    let mut visit: Vec<usize> = Vec::new();
    for c in want {
        if let Ok(k) = rows.binary_search_by_key(&(c.row + 1), |e| e.row) {
            visit.push(k);
        }
    }
    for x in rects.iter().filter(|x| x.sheet == sheet) {
        let lo = rows.partition_point(|e| e.row < x.r1 + 1);
        let hi = rows.partition_point(|e| e.row <= x.r2 + 1);
        visit.extend(lo..hi);
    }
    visit.sort_unstable();
    visit.dedup();
    for k in visit {
        let e = &rows[k];
        let r0 = e.row - 1;
        let cells = index.cells(e, part)?;
        let row = sd.and_then(|sd| sd.children()[e.item].as_element()).expect("indexed row");
        for &(col, ci) in cells {
            let idx = CellIndex { row: r0, col };
            if !(want.contains(&idx) || rects.iter().any(|x| x.contains(sheet, idx.row, idx.col))) {
                continue;
            }
            found.insert(idx);
            let c = row.children()[ci].as_element().expect("indexed cell");
            let has = c.attr(sml::Cell::ATTR_T).is_some()
                || c.elements().any(|e| e.is(sml::CellValue::QNAME) || e.is(sml::InlineString::QNAME));
            if has {
                shared += i64::from(c.attr(sml::Cell::ATTR_T).is_some_and(|t| t.trim() == "s"));
                out.push((e.item, ci));
            }
        }
    }
    if let Some(c) = want.iter().find(|c| !found.contains(c)) {
        return Err(Error::Malformed(format!("{part}: the formula census and the tree disagree about {c}")));
    }
    Ok((out, shared))
}

/// The style a missing cell shows (PR #50 review M4): its row's `s` when
/// the row has `customFormat="1"`, else the `style` of the `col` range that
/// covers the column, else 0. A preserving setter, `set_named_style` or
/// `set_datetime` on a cell that does not exist yet starts from it, so the
/// new cell keeps the look of its row or column.
fn missing_cell_style(doc: &XmlDocument, loc: &Located, at: CellIndex) -> u32 {
    let parse = ooxml_core::value::parse_u32;
    let on = |v: Option<&str>| matches!(v.map(str::trim), Some("1" | "true"));
    if let Some(ri) = loc.row
        && let Some(sd) = sheet_data(doc)
        && let Some(row) = sd.children().get(ri).and_then(|i| i.as_element())
        && on(row.attr(QName::new("", "customFormat")))
    {
        // `CT_Row/@s` defaults to 0 (PR #50 review round 2, m4)
        return row.attr(QName::new("", "s")).and_then(parse).unwrap_or(0);
    }
    let col = at.col + 1;
    doc.root()
        .elements()
        .filter(|e| e.name().ns() == crate::SML_NS && e.name().local() == "cols")
        .flat_map(|cols| cols.elements())
        .filter(|c| c.name().ns() == crate::SML_NS && c.name().local() == "col")
        .find(|c| {
            let min = c.attr(QName::new("", "min")).and_then(parse);
            let max = c.attr(QName::new("", "max")).and_then(parse);
            matches!((min, max), (Some(a), Some(b)) if a <= col && col <= b)
        })
        .and_then(|c| c.attr(QName::new("", "style")).and_then(parse))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    //! The registry path of a [`CellWritePlan`] (ADR-0003 addendum P5-pre,
    //! ADR-0017 §2.1). The public `write_cells` uses the in-place path; the
    //! registry path is for Phase5's operations, so it is tested here.

    use super::*;

    fn synthetic(name: &str) -> Vec<u8> {
        std::fs::read(format!("{}/../../reference/fixtures/xlsx/synthetic/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
    }

    fn sheet(wb: &Workbook, name: &str) -> usize {
        wb.sheets.iter().position(|s| s.name == name).unwrap()
    }

    fn writes(wb: &Workbook) -> Vec<(usize, CellIndex, CellOp, Extra)> {
        let s1 = sheet(wb, "Sheet1");
        vec![
            (s1, CellIndex::parse("A1").unwrap(), CellOp::Number("5".into()), Extra::default()),
            (s1, CellIndex::parse("B12").unwrap(), CellOp::Formula("A1*7".into(), None), Extra::default()),
            (s1, CellIndex::parse("C12").unwrap(), CellOp::Text("t".into()), Extra::default()),
        ]
    }

    #[test]
    fn mixed_cell_ops_accumulate_styles_like_single_writes() {
        let input = Workbook::new().unwrap().to_bytes().unwrap();
        let mut batch = Workbook::from_bytes(input.clone()).unwrap();
        let sheet = sheet(&batch, "Sheet1");
        let writes = || {
            let at = |cell| CellIndex::parse(cell).unwrap();
            let style = |code| Extra {
                style: Some(StyleRequest::Format(Box::new(crate::Format::new().set_num_format(code)))),
                ..Extra::default()
            };
            vec![
                (sheet, at("A1"), CellOp::Bool(true), style("0.00")),
                (sheet, at("B1"), CellOp::Error("#N/A"), style("0.000")),
                (sheet, at("C1"), CellOp::StyleOnly, style("0.00")),
                (sheet, at("D1"), CellOp::Clear, Extra::default()),
                (sheet, at("E1"), CellOp::InlineRich(vec![crate::registry::strings::RichRun {
                    font: None,
                    escaped: "rich".into(),
                }]), Extra::default()),
            ]
        };

        let plan = batch.plan_writes(writes()).unwrap();
        batch.apply_writes(plan).unwrap();
        let mut single = Workbook::from_bytes(input).unwrap();
        for (sheet, at, op, extra) in writes() {
            single.edit_cell_ext(sheet, at, op, extra).unwrap();
        }
        assert_eq!(batch.to_bytes().unwrap(), single.to_bytes().unwrap());
    }

    /// Folding `calcPr` and the calcChain removal into a registry
    /// transaction gives the same package as the in-place path.
    #[test]
    fn the_registry_path_matches_the_in_place_path() {
        for name in ["o1_precise.xlsx", "o1_no_calcpr.xlsx", "o1_manual.xlsx"] {
            let input = synthetic(name);
            let mut a = Workbook::from_bytes(input.clone()).unwrap();
            let p = a.plan_writes(writes(&a)).unwrap();
            a.apply_writes(p).unwrap();
            let mut b = Workbook::from_bytes(input.clone()).unwrap();
            let p = b.plan_writes(writes(&b)).unwrap();
            let reg = ooxml_core::RegistryPlan::new(&b.pkg);
            b.apply_writes_with(p, reg).unwrap();
            assert_eq!(a.to_bytes().unwrap(), b.to_bytes().unwrap(), "{name}");
            assert_eq!(b.calc_chain, None, "{name}");
            // the workbook stays usable on both paths
            b.edit_cell(sheet(&b, "Sheet1"), CellIndex::parse("A2").unwrap(), CellOp::Number("1".into())).unwrap();
        }
    }

    #[test]
    fn shifted_write_plan_checks_the_old_cell_and_inserts_at_the_new_coordinate() {
        let input = synthetic("o1_arrays.xlsx");
        let mut book = Workbook::from_bytes(input).unwrap();
        let before = book.to_bytes().unwrap();
        let sheet = sheet(&book, "Sheet1");
        let part = book.sheets[sheet].part.clone().unwrap();
        let old_sheet = book.pkg.xml_part(&part).unwrap().serialize();
        // Insert B. Preserve the old array anchor at C1 and its result span.
        let shifted = old_sheet.replace("r=\"G", "r=\"H")
            .replace("r=\"F", "r=\"G")
            .replace("r=\"E", "r=\"F")
            .replace("r=\"D", "r=\"E")
            .replace("r=\"C", "r=\"D")
            .replace("r=\"B", "r=\"C")
            .replace("ref=\"B1:B3\"", "ref=\"C1:C3\"")
            .replace("ref=\"D1:D3\"", "ref=\"E1:E3\"")
            .replace("<f>B2+1</f>", "<f>C2+1</f>")
            .replace("<f>D5*2</f>", "<f>E5*2</f>");
        let mut worksheet = XmlDocument::parse(shifted.as_bytes()).unwrap();
        fn map(sheet: usize, worksheet: &mut XmlDocument) -> WriteIndexMap<'_> {
            WriteIndexMap {
                sheet,
                axis: Axis::Column,
                shift: Shift::Insert { at: 1, count: 1 },
                worksheet,
            }
        }
        let write = |at| vec![(sheet, CellIndex::parse(at).unwrap(), CellOp::Text("header".into()), Extra::default())];
        std::assert_matches!(
            book.plan_writes_after(Some(map(sheet, &mut worksheet)), write("C1"), &HashSet::new(), RegistryApply::default()),
            Err(Error::ArrayAnchor { cell }) if cell == "C1"
        );
        assert_eq!(book.to_bytes().unwrap(), before);
        let plan = book.plan_writes_after(
            Some(map(sheet, &mut worksheet)), write("B1"), &HashSet::new(), RegistryApply::default(),
        ).unwrap();
        let mut workbook = book.pkg.xml_part(&book.workbook_part).unwrap().clone();
        workbook.lens_mut::<sml::Workbook>().unwrap()
            .get_or_insert::<sml::WorkbookProperties>().unwrap()
            .set_attr(sml::WorkbookProperties::ATTR_DATE1904, "1").unwrap();
        let mut registry = ooxml_core::RegistryPlan::new(&book.pkg);
        registry.stage_part(&book.pkg, &part, worksheet).unwrap();
        let chain = book.calc_chain.as_deref().unwrap();
        registry.remove_parts(&book.pkg, &[chain], &[REL_CALC_CHAIN]).unwrap();
        book.apply_writes_with_insertion(plan, registry, Some(workbook), |_| Ok(())).unwrap();
        let workbook = book.pkg.xml_part(&book.workbook_part).unwrap().serialize();
        assert!(workbook.contains("date1904=\"1\""), "{workbook}");
        assert!(workbook.contains("fullCalcOnLoad=\"1\""), "{workbook}");
        let xml = book.pkg.xml_part(&part).unwrap().serialize();
        assert!(xml.contains("<c r=\"B1\" t=\"inlineStr\"><is><t>header</t></is></c>"), "{xml}");
        assert!(xml.contains("<c r=\"C1\" s=\"0\"><f t=\"array\" ref=\"C1:C3\""), "{xml}");
        assert!(book.census.at(sheet, CellIndex::parse("C1").unwrap()).is_some());
        assert!(book.calc_chain.is_none());
    }

    #[test]
    fn shifted_write_plan_rejects_other_sheets_before_shared_formula_handoff() {
        let mut book = Workbook::from_bytes(synthetic("o1_shared.xlsx")).unwrap();
        book.add_sheet("Shifted", 1).unwrap();
        let shifted = sheet(&book, "Shifted");
        let other = sheet(&book, "Sheet2");
        let shifted_part = book.sheets[shifted].part.clone().unwrap();
        let other_part = book.sheets[other].part.clone().unwrap();
        let original = book.pkg.xml_part(&other_part).unwrap().serialize();
        let shared = original.replace(">A2*2</f>", ">Shifted!A2*2</f>");
        assert_ne!(shared, original);
        let mut setup = ooxml_core::RegistryPlan::new(&book.pkg);
        setup.stage_part(&book.pkg, &other_part, XmlDocument::parse(shared.as_bytes()).unwrap()).unwrap();
        book.apply_registry_edit(setup, |_| Ok(())).unwrap();
        book.rebuild().unwrap();
        let before = book.to_bytes().unwrap();

        // Inserting a row in Shifted also rewrites the staged master in Sheet2.
        let mut staged_other = ooxml_core::RegistryPlan::new(&book.pkg);
        let rewritten = shared.replace("Shifted!A2*2", "Shifted!A3*2");
        staged_other.stage_part(&book.pkg, &other_part, XmlDocument::parse(rewritten.as_bytes()).unwrap()).unwrap();
        let mut worksheet = book.pkg.xml_part(&shifted_part).unwrap().clone();
        let write = |sheet, at| (sheet, CellIndex::parse(at).unwrap(), CellOp::Text("new".into()), Extra::default());
        let mut plan = |book: &mut Workbook, writes| book.plan_writes_after(
            Some(WriteIndexMap {
                sheet: shifted, axis: Axis::Row, shift: Shift::Insert { at: 0, count: 1 }, worksheet: &mut worksheet,
            }),
            writes, &HashSet::new(), RegistryApply::default(),
        );
        std::assert_matches!(
            plan(&mut book, vec![write(shifted, "A1"), write(other, "B2")]),
            Err(Error::Malformed(message)) if message.contains("must target the edited sheet")
        );
        assert_eq!(book.to_bytes().unwrap(), before);
        let plan = plan(&mut book, vec![write(shifted, "A1")]).unwrap();

        staged_other.stage_part(&book.pkg, &shifted_part, worksheet).unwrap();
        book.apply_writes_with_insertion(plan, staged_other, None, |_| Ok(())).unwrap();
        assert!(book.pkg.xml_part(&shifted_part).unwrap().serialize().contains("<t>new</t>"));
        assert_eq!(book.pkg.xml_part(&other_part).unwrap().serialize(), rewritten);
    }

    #[test]
    fn shifted_write_skips_old_master_dependents_and_chain() {
        let mut book = Workbook::from_bytes(synthetic("o1_shared.xlsx")).unwrap();
        let index = sheet(&book, "Sheet2");
        let part = book.sheets[index].part.clone().unwrap();
        let mut shifted = Workbook::from_bytes(synthetic("o1_shared.xlsx")).unwrap();
        shifted.worksheet().unwrap().insert_rows(2, 1).unwrap();
        let mut worksheet = shifted.pkg.xml_part(&part).unwrap().clone();
        let at = CellIndex::parse("B2").unwrap();
        let plan = book.plan_writes_after(
            Some(WriteIndexMap { sheet: index, axis: Axis::Row, shift: Shift::Insert { at: 2, count: 1 }, worksheet: &mut worksheet }),
            vec![(index, at, CellOp::Number("1".into()), Extra::default())],
            &HashSet::new(),
            RegistryApply::default(),
        ).unwrap();
        assert!(plan.handoffs.is_empty());
        assert!(plan.strip.sheets.is_empty());
        assert!(plan.chain.is_none());
    }

    #[test]
    fn shifted_dependent_guard_excludes_cells_written_at_their_new_coordinates() {
        let mut book = Workbook::from_bytes(synthetic("o1_iterate.xlsx")).unwrap();
        let sheet = sheet(&book, "Sheet1");
        let before = book.to_bytes().unwrap();
        assert!(book.calc.iterate);
        let part = book.sheets[sheet].part.clone().unwrap();
        let mut worksheet = book.pkg.xml_part(&part).unwrap().clone();
        let edit = WriteIndexMap {
            sheet, axis: Axis::Row, shift: Shift::Insert { at: 0, count: 1 }, worksheet: &mut worksheet,
        };
        // The structural caller computes dependents using the old census.
        let deps = book.compute_dependents(sheet, CellIndex::parse("B1").unwrap());
        let old = CellIndex::parse("A1").unwrap();
        assert_eq!(deps.cells, [(sheet, old)]);
        let written = HashSet::from([(sheet, CellIndex::parse("A2").unwrap())]);
        assert!(!book.check_write_dependents(&deps, &written, Some(&edit)).unwrap());
        std::assert_matches!(
            book.check_write_dependents(&deps, &HashSet::new(), Some(&edit)),
            Err(Error::IterativeCalculation)
        );
        // The rich-value guard also uses the old coordinate for census lookup.
        book.census.vm_cells.insert((sheet, old));
        assert!(!book.check_write_dependents(&deps, &written, Some(&edit)).unwrap());
        std::assert_matches!(
            book.check_write_dependents(&deps, &HashSet::new(), Some(&edit)),
            Err(Error::RichValueCell { cell }) if cell == "A1"
        );
        assert_eq!(book.to_bytes().unwrap(), before);
    }

    #[test]
    fn shifted_row_plan_reports_array_cells_at_post_edit_coordinates() {
        let mut book = Workbook::from_bytes(synthetic("o1_arrays.xlsx")).unwrap();
        let before = book.to_bytes().unwrap();
        let sheet = sheet(&book, "Sheet1");
        let mut worksheet = XmlDocument::parse(format!(
            r#"<worksheet xmlns="{}"><sheetData/></worksheet>"#, crate::SML_NS
        ).as_bytes()).unwrap();
        let mut write = |at| book.plan_writes_after(
            Some(WriteIndexMap {
                sheet, axis: Axis::Row, shift: Shift::Insert { at: 0, count: 1 }, worksheet: &mut worksheet,
            }),
            vec![(sheet, CellIndex::parse(at).unwrap(), CellOp::Text("value".into()), Extra::default())],
            &HashSet::new(), RegistryApply::default(),
        );
        std::assert_matches!(write("B2"), Err(Error::ArrayAnchor { cell }) if cell == "B2");
        std::assert_matches!(write("B3"), Err(Error::ArrayResultCell { cell, anchor })
            if cell == "B3" && anchor == "B2");
        assert_eq!(book.to_bytes().unwrap(), before);
    }

    #[test]
    fn shifted_write_plan_folds_shared_strings_into_the_structural_registry() {
        let mut input = rust_xlsxwriter::Workbook::new();
        input.add_worksheet().write_string(0, 0, "removed").unwrap();
        let mut book = Workbook::from_bytes(input.save_to_buffer().unwrap()).unwrap();
        let sheet = sheet(&book, "Sheet1");
        let part = book.sheets[sheet].part.clone().unwrap();
        let original = book.pkg.xml_part(&part).unwrap().serialize();
        let shifted = original.replace(r#"<c r="A1" t="s"><v>0</v></c>"#, "");
        assert_ne!(shifted, original);
        let mut worksheet = XmlDocument::parse(shifted.as_bytes()).unwrap();
        assert!(book.shared_strings_active().unwrap());
        let strings = book.registries.strings.as_ref().unwrap();
        let header = strings.plan("header", "header").unwrap();
        let mut prior = strings.plan("already planned", "already planned").unwrap();
        strings.reserve(&mut prior, &mut HashMap::new());
        let prior_index = prior.index;
        let prior_write = strings.plan("already planned", "already planned").unwrap();

        let edits = RegistryApply {
            strings: vec![prior],
            refs_delta: -1,
            refs_changed: true,
            ..RegistryApply::default()
        };
        let plan = book.plan_writes_after(
            Some(WriteIndexMap { sheet, axis: Axis::Row, shift: Shift::Delete { at: 0, count: 1 }, worksheet: &mut worksheet }),
            vec![
                (sheet, CellIndex::parse("A1").unwrap(), CellOp::Shared(header.index), Extra { string: Some(header), style: None }),
                (sheet, CellIndex::parse("B1").unwrap(), CellOp::Shared(prior_index), Extra { string: Some(prior_write), style: None }),
            ],
            &HashSet::new(),
            edits,
        ).unwrap();
        let mut registry = ooxml_core::RegistryPlan::new(&book.pkg);
        registry.stage_part(&book.pkg, &part, worksheet).unwrap();
        book.apply_writes_with(plan, registry).unwrap();

        let strings = book.pkg.xml_part("xl/sharedStrings.xml").unwrap().serialize();
        assert!(strings.contains(r#"count="2" uniqueCount="3""#), "{strings}");
        assert_eq!(strings.matches("<t>already planned</t>").count(), 1);
        let sheet = book.pkg.xml_part(&part).unwrap().serialize();
        assert!(sheet.contains(r#"<c r="A1" t="s"><v>2</v></c>"#), "{sheet}");
        assert!(sheet.contains(r#"<c r="B1" t="s"><v>1</v></c>"#), "{sheet}");
    }

    #[test]
    fn calc_flag_is_applied_to_the_callers_workbook() {
        let input = synthetic("o1_no_calcpr.xlsx");
        let mut book = Workbook::from_bytes(input).unwrap();
        let plan = book.plan_writes(writes(&book)).unwrap();
        let mut workbook = book.pkg.xml_part(&book.workbook_part).unwrap().clone();
        let mut lens = workbook.lens_mut::<sml::Workbook>().unwrap();
        let mut properties = lens.get_or_insert::<sml::WorkbookProperties>().unwrap();
        properties.set_attr(sml::WorkbookProperties::ATTR_DATE1904, "1").unwrap();
        let registry = ooxml_core::RegistryPlan::new(&book.pkg);
        book.apply_writes_with_insertion(plan, registry, Some(workbook), |_| Ok(())).unwrap();
        let xml = book.pkg.xml_part(&book.workbook_part).unwrap().serialize();
        assert!(xml.contains("date1904=\"1\""), "{xml}");
        assert!(xml.contains("fullCalcOnLoad=\"1\""), "{xml}");
    }

    /// A refusal while folding (here: the caller's transaction already
    /// removes `calcChain.xml`, so the plan's removal conflicts) is an
    /// ordinary `Err`: nothing changed, no poison.
    #[test]
    fn a_fold_refusal_changes_nothing() {
        let input = synthetic("o1_precise.xlsx");
        let mut wb = Workbook::from_bytes(input.clone()).unwrap();
        let p = wb.plan_writes(writes(&wb)).unwrap();
        let chain = wb.calc_chain.clone().expect("o1_precise has a calcChain");
        let mut reg = ooxml_core::RegistryPlan::new(&wb.pkg);
        reg.remove_parts(&wb.pkg, &[chain.as_str()], &[REL_CALC_CHAIN]).unwrap();
        let e = wb.apply_writes_with(p, reg);
        assert!(matches!(e, Err(Error::Core(_))), "{e:?}");
        assert!(wb.poisoned.is_none());
        assert_eq!(wb.to_bytes().unwrap(), Workbook::from_bytes(input).unwrap().to_bytes().unwrap());
    }

    /// `StalePlan` (an ordering bug in step 1, injected) is returned before
    /// anything changes: an ordinary `Err`, no poison (ADR-0017 r2-m5).
    #[cfg(feature = "fault-injection")]
    #[test]
    fn a_stale_registry_plan_is_an_ordinary_error() {
        use crate::workbook::ApplyStep;
        let input = synthetic("o1_precise.xlsx");
        let mut wb = Workbook::from_bytes(input.clone()).unwrap();
        let p = wb.plan_writes(writes(&wb)).unwrap();
        let reg = ooxml_core::RegistryPlan::new(&wb.pkg);
        wb.inject_apply_failure(ApplyStep::StaleRegistryPlan);
        let e = wb.apply_writes_with(p, reg);
        assert!(matches!(e, Err(Error::Core(ooxml_core::Error::RegistryRefused { reason: ooxml_core::RegistryRefusal::StalePlan, .. }))), "{e:?}");
        assert!(wb.poisoned.is_none());
        let out = wb.to_bytes().unwrap();
        assert_eq!(out, Workbook::from_bytes(input).unwrap().to_bytes().unwrap(), "nothing changed");
        // planning again works
        let p = wb.plan_writes(writes(&wb)).unwrap();
        let reg = ooxml_core::RegistryPlan::new(&wb.pkg);
        wb.apply_writes_with(p, reg).unwrap();
    }

    // ---------- C1's report and plan purity (slice A) ----------

    use crate::registry::format::Font;
    use crate::registry::strings::RichRun as PlannedRichRun;
    use crate::registry::styles::StyleRequest;

    fn at(a1: &str) -> CellIndex {
        CellIndex::parse(a1).unwrap()
    }

    /// Every parsed part is clean: no dirty node or flag.
    fn assert_clean(wb: &Workbook) {
        for name in wb.pkg.part_names() {
            if let Some(doc) = wb.pkg.parsed_part(name) {
                assert!(!doc.is_dirty(), "{name} is dirty");
                assert!(!doc.root().subtree_dirty(), "{name} has a dirty subtree");
            }
        }
    }

    /// Plans one write and reports it, on a fresh open of `wb`'s bytes
    /// (the setup writes above legitimately dirty the tree). The plan and
    /// the report change nothing: bytes identical, no dirty flag.
    fn report_of(
        wb: &Workbook,
        sheet: usize,
        a1: &str,
        op: CellOp,
        extra: Extra,
    ) -> Vec<TargetReport> {
        let bytes = wb.to_bytes().unwrap();
        let mut fresh = Workbook::from_bytes(bytes.clone()).unwrap();
        assert_clean(&fresh);
        let plan = fresh.plan_cell_write(sheet, at(a1), op, extra).unwrap();
        let out = plan.report(&mut fresh).unwrap();
        assert_eq!(
            fresh.to_bytes().unwrap(),
            bytes,
            "the plan and the report change nothing"
        );
        assert_clean(&fresh);
        out
    }

    fn single(r: Vec<TargetReport>) -> TargetReport {
        assert_eq!(r.len(), 1, "one target: {r:?}");
        r.into_iter().next().unwrap()
    }

    #[test]
    fn report_number_is_the_literal_v_text() {
        let mut wb = Workbook::new().unwrap();
        let sheet = sheet(&wb, "Sheet1");
        wb.worksheet().unwrap().write_number(0, 0, 5.0).unwrap();
        let t = single(report_of(
            &wb,
            sheet,
            "A1",
            CellOp::Number("5".into()),
            Extra::default(),
        ));
        assert!(t.named && t.exists && !t.group_or_array);
        assert!(t.state_same && t.node_same && t.validation_same);
        // A10: `5.0` over `5` applies
        let t = single(report_of(
            &wb,
            sheet,
            "A1",
            CellOp::Number("5.0".into()),
            Extra::default(),
        ));
        assert!(!t.state_same && !t.validation_same);
        // a missing cell is never the same, and does not exist
        let t = single(report_of(
            &wb,
            sheet,
            "B5",
            CellOp::Number("5".into()),
            Extra::default(),
        ));
        assert!(!t.exists && !t.state_same && !t.validation_same);
    }

    #[test]
    fn report_text_needs_the_one_t_with_the_text() {
        let mut wb = Workbook::new().unwrap();
        let sheet = sheet(&wb, "Sheet1");
        wb.worksheet()
            .unwrap()
            .cell_mut(0, 0)
            .unwrap()
            .set_string_inline("a<b")
            .unwrap();
        wb.worksheet()
            .unwrap()
            .cell_mut(1, 0)
            .unwrap()
            .set_string_inline("x\x01y")
            .unwrap();
        // entities and `_xHHHH_` round-trip: the written bytes read back equal
        for (a1, v) in [("A1", "a<b"), ("A2", "x\x01y")] {
            let planned = crate::escape::encode(v).into_owned();
            let t = single(report_of(
                &wb,
                sheet,
                a1,
                CellOp::Text(planned),
                Extra::default(),
            ));
            assert!(t.state_same && t.validation_same, "{a1}");
        }
        let t = single(report_of(
            &wb,
            sheet,
            "A1",
            CellOp::Text("a<c".into()),
            Extra::default(),
        ));
        assert!(!t.state_same && !t.validation_same);
    }

    #[test]
    fn report_plain_over_rich_drops_the_runs() {
        let mut wb = Workbook::new().unwrap();
        let sheet = sheet(&wb, "Sheet1");
        let bold = Font {
            bold: true,
            ..Font::default()
        };
        wb.worksheet()
            .unwrap()
            .cell_mut(0, 0)
            .unwrap()
            .set_rich_string(&[(&bold, "ab"), (&Font::default(), "c")])
            .unwrap();
        // shared or inline, the runs differ from a plain write
        let t = single(report_of(
            &wb,
            sheet,
            "A1",
            CellOp::Text("abc".into()),
            Extra::default(),
        ));
        assert!(!t.state_same && !t.validation_same);
    }

    #[test]
    fn report_shared_compares_the_index() {
        let mut wb = Workbook::new().unwrap();
        let sheet = sheet(&wb, "Sheet1");
        wb.edit_cell(sheet, at("A1"), CellOp::Shared(2)).unwrap();
        let t = single(report_of(
            &wb,
            sheet,
            "A1",
            CellOp::Shared(2),
            Extra::default(),
        ));
        assert!(t.state_same && t.validation_same);
        let t = single(report_of(
            &wb,
            sheet,
            "A1",
            CellOp::Shared(3),
            Extra::default(),
        ));
        assert!(!t.state_same && !t.validation_same);
    }

    #[test]
    fn report_shared_compares_the_v_spelling_literally() {
        let mut wb = Workbook::new().unwrap();
        let sheet = sheet(&wb, "Sheet1");
        wb.worksheet().unwrap().write_string(0, 0, "zero").unwrap();
        // a foreign spelling of the same shared index
        let part = wb.sheets[sheet].part.clone().unwrap();
        let bytes = wb.to_bytes().unwrap();
        let mut pkg = ooxml_core::Package::from_bytes(bytes).unwrap();
        let xml = String::from_utf8(pkg.read_part(&part).unwrap()).unwrap();
        let a1 = xml.find("r=\"A1\"").expect("the written cell");
        let v = xml[a1..]
            .split("<v>")
            .nth(1)
            .and_then(|s| s.split("</v>").next())
            .expect("a shared index");
        let index: u32 = v.parse().expect("a numeric index");
        let padded = format!("<v>0{v}</v>");
        let tail = xml[a1..].replacen(&format!("<v>{v}</v>"), &padded, 1);
        let patched = format!("{}{tail}", &xml[..a1]);
        assert!(patched.contains(&padded), "{patched}");
        pkg.set_part(&part, patched.into_bytes()).unwrap();
        let wb = Workbook::from_bytes(pkg.to_bytes().unwrap()).unwrap();
        let t = single(report_of(
            &wb,
            sheet,
            "A1",
            CellOp::Shared(index),
            Extra::default(),
        ));
        assert!(t.exists && !t.group_or_array);
        assert!(!t.state_same && !t.validation_same, "a padded `v` applies");
        // the sequential write normalizes the spelling, so a skip would diverge
        let bytes = wb.to_bytes().unwrap();
        let mut applied = Workbook::from_bytes(bytes).unwrap();
        applied.edit_cell(sheet, at("A1"), CellOp::Shared(index)).unwrap();
        let xml = applied.pkg.xml_part(&part).unwrap().serialize();
        assert!(xml.contains(&format!("<v>{index}</v>")), "{xml}");
        assert!(!xml.contains(&padded), "{xml}");
    }

    #[test]
    fn report_bool_and_error_are_literal() {
        let mut wb = Workbook::new().unwrap();
        let sheet = sheet(&wb, "Sheet1");
        wb.worksheet()
            .unwrap()
            .cell_mut(0, 0)
            .unwrap()
            .set_bool(true)
            .unwrap();
        wb.worksheet()
            .unwrap()
            .cell_mut(1, 0)
            .unwrap()
            .set_error("#N/A")
            .unwrap();
        let t = single(report_of(
            &wb,
            sheet,
            "A1",
            CellOp::Bool(true),
            Extra::default(),
        ));
        assert!(t.state_same && t.validation_same);
        let t = single(report_of(
            &wb,
            sheet,
            "A1",
            CellOp::Bool(false),
            Extra::default(),
        ));
        assert!(!t.state_same);
        let t = single(report_of(
            &wb,
            sheet,
            "A2",
            CellOp::Error("#N/A"),
            Extra::default(),
        ));
        assert!(t.state_same && t.validation_same);
        let t = single(report_of(
            &wb,
            sheet,
            "A2",
            CellOp::Error("#DIV/0!"),
            Extra::default(),
        ));
        assert!(!t.state_same);
    }

    #[test]
    fn report_inline_rich_compares_runs() {
        let mut wb = Workbook::new().unwrap();
        let sheet = sheet(&wb, "Sheet1");
        let bold = Font {
            bold: true,
            ..Font::default()
        };
        let runs = vec![
            PlannedRichRun {
                font: Some(bold.clone()),
                escaped: "ab".into(),
            },
            PlannedRichRun {
                font: None,
                escaped: "c".into(),
            },
        ];
        wb.edit_cell(sheet, at("A1"), CellOp::InlineRich(runs.clone()))
            .unwrap();
        let t = single(report_of(
            &wb,
            sheet,
            "A1",
            CellOp::InlineRich(runs),
            Extra::default(),
        ));
        assert!(t.state_same && t.node_same && t.validation_same);
        let other = vec![PlannedRichRun {
            font: None,
            escaped: "abc".into(),
        }];
        let t = single(report_of(
            &wb,
            sheet,
            "A1",
            CellOp::InlineRich(other),
            Extra::default(),
        ));
        assert!(!t.state_same && !t.validation_same);
        // same text and shape, other run properties: the runs compare
        let italic = Font {
            italic: true,
            ..Font::default()
        };
        let reprops = vec![
            PlannedRichRun {
                font: Some(italic),
                escaped: "ab".into(),
            },
            PlannedRichRun {
                font: None,
                escaped: "c".into(),
            },
        ];
        let t = single(report_of(
            &wb,
            sheet,
            "A1",
            CellOp::InlineRich(reprops),
            Extra::default(),
        ));
        assert!(!t.state_same && !t.validation_same);
        // plain text over the rich runs differs (runs and single `t` both)
        let t = single(report_of(
            &wb,
            sheet,
            "A1",
            CellOp::Text("abc".into()),
            Extra::default(),
        ));
        assert!(!t.state_same && !t.validation_same);
    }

    #[test]
    fn report_inline_rich_with_escapable_text() {
        let mut wb = Workbook::new().unwrap();
        let sheet = sheet(&wb, "Sheet1");
        // a control character and a literal `_xHHHH_`, as `plan_rich_write`
        // escapes them
        let runs = vec![
            PlannedRichRun {
                font: None,
                escaped: crate::escape::encode("x\x01y").into_owned(),
            },
            PlannedRichRun {
                font: None,
                escaped: crate::escape::encode("_x0001_").into_owned(),
            },
        ];
        wb.edit_cell(sheet, at("A1"), CellOp::InlineRich(runs.clone()))
            .unwrap();
        let t = single(report_of(
            &wb,
            sheet,
            "A1",
            CellOp::InlineRich(runs),
            Extra::default(),
        ));
        assert!(t.state_same && t.node_same && t.validation_same);
        // a raw `_x0001_` is not the stored `_x005F_x0001_`: nothing
        // decodes (else both would read as U+0001 and compare same)
        let raw = vec![
            PlannedRichRun {
                font: None,
                escaped: crate::escape::encode("x\x01y").into_owned(),
            },
            PlannedRichRun {
                font: None,
                escaped: "_x0001_".into(),
            },
        ];
        let t = single(report_of(
            &wb,
            sheet,
            "A1",
            CellOp::InlineRich(raw),
            Extra::default(),
        ));
        assert!(!t.state_same && !t.validation_same);
    }

    #[test]
    fn report_formula_compares_text_and_attributes() {
        let mut wb = Workbook::new().unwrap();
        let s0 = sheet(&wb, "Sheet1");
        wb.worksheet()
            .unwrap()
            .cell_mut(1, 1)
            .unwrap()
            .set_formula("A1*2")
            .unwrap();
        let t = single(report_of(
            &wb,
            s0,
            "B2",
            CellOp::Formula("A1*2".into(), None),
            Extra::default(),
        ));
        assert!(t.state_same && t.node_same && t.validation_same);
        let t = single(report_of(
            &wb,
            s0,
            "B2",
            CellOp::Formula("A1*3".into(), None),
            Extra::default(),
        ));
        assert!(!t.state_same);
        // a shared member's own text differs (its `t`/`si` attributes)
        let g = Workbook::from_bytes(synthetic("o1_shared.xlsx")).unwrap();
        let gs = sheet(&g, "Sheet2");
        let t = single(report_of(
            &g,
            gs,
            "E3",
            CellOp::Formula("$A3+E$1".into(), None),
            Extra::default(),
        ));
        assert!(t.group_or_array && !t.state_same && !t.validation_same);
        // the master's own text differs too: same stored text, but the
        // group attributes are not a plain `f` (plus the hand-off target)
        let r = report_of(
            &g,
            gs,
            "D2",
            CellOp::Formula("$A2+D$1".into(), None),
            Extra::default(),
        );
        assert_eq!(r.len(), 2);
        assert!(r[0].group_or_array && !r[0].state_same && !r[0].validation_same);
    }

    #[test]
    fn report_dynamic_and_remove_are_never_same() {
        let mut wb = Workbook::new().unwrap();
        let sheet = sheet(&wb, "Sheet1");
        wb.worksheet()
            .unwrap()
            .cell_mut(0, 0)
            .unwrap()
            .set_formula("1+1")
            .unwrap();
        // C3: metadata-block identity is not compared
        let t = single(report_of(
            &wb,
            sheet,
            "A1",
            CellOp::DynamicFormula("1+1".into(), 1),
            Extra::default(),
        ));
        assert!(!t.state_same && !t.node_same && !t.validation_same);
        // C4: removing `f` and the cache always changes content
        let t = single(report_of(
            &wb,
            sheet,
            "A1",
            CellOp::RemoveFormula,
            Extra::default(),
        ));
        assert!(!t.state_same && !t.validation_same);
    }

    #[test]
    fn report_clear_is_same_only_on_empty() {
        let mut wb = Workbook::new().unwrap();
        let sheet = sheet(&wb, "Sheet1");
        wb.worksheet()
            .unwrap()
            .cell_mut(0, 0)
            .unwrap()
            .set_number(5.0)
            .unwrap();
        let t = single(report_of(
            &wb,
            sheet,
            "A1",
            CellOp::ClearValue,
            Extra::default(),
        ));
        assert!(!t.state_same, "removes the value");
        wb.worksheet()
            .unwrap()
            .cell_mut(0, 0)
            .unwrap()
            .clear_value()
            .unwrap();
        let t = single(report_of(
            &wb,
            sheet,
            "A1",
            CellOp::ClearValue,
            Extra::default(),
        ));
        assert!(t.exists && t.state_same && t.node_same && t.validation_same);
    }

    #[test]
    fn report_style_compares_the_resolved_s() {
        let mut wb = Workbook::new().unwrap();
        let sheet = sheet(&wb, "Sheet1");
        let index = |i| Extra {
            string: None,
            style: Some(StyleRequest::Index(i)),
        };
        let t = single(report_of(&wb, sheet, "A1", CellOp::StyleOnly, index(0)));
        assert!(t.state_same && t.validation_same, "missing s equals 0");
        // an existing cell without `s` reads the same 0
        wb.worksheet().unwrap().cell_mut(1, 1).unwrap().set_number(5.0).unwrap();
        let t = single(report_of(&wb, sheet, "B2", CellOp::StyleOnly, index(0)));
        assert!(t.state_same && t.validation_same, "an s-less node equals 0");
        let bold = Font {
            bold: true,
            ..Font::default()
        };
        wb.worksheet()
            .unwrap()
            .cell_mut(0, 0)
            .unwrap()
            .set_font(&bold)
            .unwrap();
        let t = single(report_of(&wb, sheet, "A1", CellOp::StyleOnly, index(1)));
        assert!(t.state_same && t.validation_same);
        let t = single(report_of(&wb, sheet, "A1", CellOp::StyleOnly, index(0)));
        assert!(!t.state_same && !t.validation_same);
        // a patch resolving to the current xf is the same style
        wb.worksheet()
            .unwrap()
            .cell_mut(1, 0)
            .unwrap()
            .set_font(&Font::default())
            .unwrap();
        let s = wb
            .snapshot_cell(sheet, at("A2"))
            .unwrap()
            .node()
            .unwrap()
            .attr(ooxml_core::QName::new("", "s"))
            .unwrap()
            .to_owned();
        let t = single(report_of(
            &wb,
            sheet,
            "A2",
            CellOp::StyleOnly,
            Extra {
                string: None,
                style: Some(StyleRequest::FontOnly(Box::default())),
            },
        ));
        assert!(t.state_same, "resolves to s={s} again");
    }

    #[test]
    fn report_two_half_conjoins_content_and_style() {
        let mut wb = Workbook::new().unwrap();
        let sheet = sheet(&wb, "Sheet1");
        wb.worksheet()
            .unwrap()
            .cell_mut(0, 0)
            .unwrap()
            .set_number(5.0)
            .unwrap();
        let bold = Font {
            bold: true,
            ..Font::default()
        };
        wb.worksheet()
            .unwrap()
            .cell_mut(0, 0)
            .unwrap()
            .set_font(&bold)
            .unwrap();
        let index = |i| Extra {
            string: None,
            style: Some(StyleRequest::Index(i)),
        };
        let t = single(report_of(
            &wb,
            sheet,
            "A1",
            CellOp::Number("5".into()),
            index(1),
        ));
        assert!(t.state_same && t.validation_same);
        // same content, other style: the style conjunct vetoes
        let t = single(report_of(
            &wb,
            sheet,
            "A1",
            CellOp::Number("5".into()),
            index(0),
        ));
        assert!(!t.state_same && !t.validation_same);
        // other content, same style: the content conjunct vetoes
        let t = single(report_of(
            &wb,
            sheet,
            "A1",
            CellOp::Number("6".into()),
            index(1),
        ));
        assert!(!t.state_same && !t.validation_same);
    }

    #[test]
    fn report_two_half_with_real_style_requests() {
        use crate::registry::dates::ExcelDateTime;
        let mut wb = Workbook::new().unwrap();
        let sheet = sheet(&wb, "Sheet1");
        let date1904 = wb.date1904().unwrap();
        let d = ExcelDateTime::from_ymd(2026, 1, 1).unwrap();
        let serial = d.serial(date1904).unwrap();
        let serial_text = crate::worksheet::format_number(serial);
        let ensure_date = || Extra {
            string: None,
            style: Some(StyleRequest::EnsureDate),
        };
        // the same serial on a date-formatted cell: both halves same
        wb.worksheet().unwrap().cell_mut(0, 0).unwrap().set_datetime(d).unwrap();
        let t = single(report_of(
            &wb,
            sheet,
            "A1",
            CellOp::Number(serial_text.clone()),
            ensure_date(),
        ));
        assert!(t.state_same && t.validation_same);
        // the same serial on a non-date format: `EnsureDate` changes the style
        wb.worksheet().unwrap().cell_mut(1, 0).unwrap().set_number(serial).unwrap();
        let t = single(report_of(
            &wb,
            sheet,
            "A2",
            CellOp::Number(serial_text),
            ensure_date(),
        ));
        assert!(!t.state_same && !t.validation_same);
        // a `Format` request resolving to the current xf is the same style
        let two = crate::Format::new().set_num_format("0.00");
        let three = crate::Format::new().set_num_format("0.000");
        wb.worksheet()
            .unwrap()
            .cell_mut(0, 1)
            .unwrap()
            .set_number_with_format(5.0, &two)
            .unwrap();
        let with = |f: &crate::Format| Extra {
            string: None,
            style: Some(StyleRequest::Format(Box::new(f.clone()))),
        };
        let t = single(report_of(
            &wb,
            sheet,
            "B1",
            CellOp::Number("5".into()),
            with(&two),
        ));
        assert!(t.state_same && t.validation_same);
        let t = single(report_of(
            &wb,
            sheet,
            "B1",
            CellOp::Number("5".into()),
            with(&three),
        ));
        assert!(!t.state_same && !t.validation_same);
    }

    #[test]
    fn report_conditional_skips_are_not_targets() {
        let mut wb = Workbook::new().unwrap();
        wb.worksheet()
            .unwrap()
            .cell_mut(0, 0)
            .unwrap()
            .set_number(5.0)
            .unwrap();
        let sheet = sheet(&wb, "Sheet1");
        for (op, a1) in [(CellOp::RemoveFormula, "A1"), (CellOp::ClearValue, "Z9")] {
            let bytes = wb.to_bytes().unwrap();
            let mut fresh = Workbook::from_bytes(bytes.clone()).unwrap();
            let plan = fresh
                .plan_cell_write(sheet, at(a1), op, Extra::default())
                .unwrap();
            assert!(plan.report(&mut fresh).unwrap().is_empty());
            assert_eq!(fresh.to_bytes().unwrap(), bytes);
            assert_clean(&fresh);
        }
    }

    #[test]
    fn report_group_flags_cover_masters_members_arrays_and_dynamic() {
        let plain = |wb: &Workbook, sheet: usize, a1: &str| {
            single(report_of(
                wb,
                sheet,
                a1,
                CellOp::StyleOnly,
                Extra::default(),
            ))
            .group_or_array
        };
        let wb = Workbook::new().unwrap();
        let s0 = sheet(&wb, "Sheet1");
        assert!(!plain(&wb, s0, "A1"));
        assert!(!report_of(&wb, s0, "A1", CellOp::StyleOnly, Extra::default())[0].exists);
        let g = Workbook::from_bytes(synthetic("o1_shared.xlsx")).unwrap();
        let s1 = sheet(&g, "Sheet2");
        assert!(plain(&g, s1, "D2"), "master");
        assert!(plain(&g, s1, "E3"), "member");
        assert!(plain(&g, s1, "B2"), "master of si 0");
        assert!(!plain(&g, s1, "H9"), "a normal formula");
        let a = Workbook::from_bytes(synthetic("o1_arrays.xlsx")).unwrap();
        let s2 = sheet(&a, "Sheet1");
        assert!(plain(&a, s2, "B1"), "array anchor");
        assert!(plain(&a, s2, "B2"), "array result");
        // a dynamic anchor: StyleOnly plans where content refuses
        let mut wb = Workbook::new().unwrap();
        wb.worksheet()
            .unwrap()
            .cell_mut(0, 0)
            .unwrap()
            .set_dynamic_formula("1+1")
            .unwrap();
        let s3 = sheet(&wb, "Sheet1");
        assert!(plain(&wb, s3, "A1"), "dynamic");
    }

    #[test]
    fn report_group_flags_a_data_table_cell() {
        let wb = Workbook::new().unwrap();
        let part = wb.sheets[0].part.clone().unwrap();
        let bytes = wb.to_bytes().unwrap();
        let mut pkg = ooxml_core::Package::from_bytes(bytes).unwrap();
        let sheet = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="{ns}"><sheetData><row r="4"><c r="C4"><f t="dataTable" ref="C4:C5" r1="A1" r2="A2"/></c></row></sheetData></worksheet>"#,
            ns = crate::SML_NS
        );
        pkg.set_part(&part, sheet.into_bytes()).unwrap();
        let mut wb = Workbook::from_bytes(pkg.to_bytes().unwrap()).unwrap();
        // content refuses, as for an array anchor ...
        std::assert_matches!(
            wb.plan_cell_write(
                0,
                at("C4"),
                CellOp::Number("1".into()),
                Extra::default()
            ),
            Err(Error::ArrayAnchor { .. })
        );
        // ... so StyleOnly plans where content refuses, and the flag is set
        let t = single(report_of(&wb, 0, "C4", CellOp::StyleOnly, Extra::default()));
        assert!(t.exists && t.group_or_array);
    }

    #[test]
    fn report_handoff_names_the_new_master() {
        let g = Workbook::from_bytes(synthetic("o1_shared.xlsx")).unwrap();
        let sheet = sheet(&g, "Sheet2");
        let bytes = g.to_bytes().unwrap();
        let mut fresh = Workbook::from_bytes(bytes.clone()).unwrap();
        assert_clean(&fresh);
        let plan = fresh
            .plan_cell_write(
                sheet,
                at("D2"),
                CellOp::Number("1".into()),
                Extra::default(),
            )
            .unwrap();
        let r = plan.report(&mut fresh).unwrap();
        assert_eq!(r.len(), 2);
        assert!(r[0].named && r[0].at == at("D2") && !r[0].state_same);
        assert!(!r[1].named && r[1].at == at("C2"));
        assert!(r[1].exists && r[1].group_or_array);
        assert!(!r[1].state_same && !r[1].validation_same);
        assert_eq!(fresh.to_bytes().unwrap(), bytes);
        assert_clean(&fresh);
    }

    #[test]
    fn report_named_handoff_collision_reports_once_with_the_touched_veto() {
        let g = Workbook::from_bytes(synthetic("o1_shared.xlsx")).unwrap();
        let sheet = sheet(&g, "Sheet2");
        let bytes = g.to_bytes().unwrap();
        let mut fresh = Workbook::from_bytes(bytes.clone()).unwrap();
        assert_clean(&fresh);
        // a master content write (hands off to C2) plus a style write at C2
        let plan = fresh
            .plan_writes(vec![
                (sheet, at("D2"), CellOp::Number("1".into()), Extra::default()),
                (sheet, at("C2"), CellOp::StyleOnly, Extra::default()),
            ])
            .unwrap();
        let r = plan.report(&mut fresh).unwrap();
        assert_eq!(r.len(), 2, "the new master is reported once, as named: {r:?}");
        assert!(r[0].named && r[0].at == at("D2"));
        assert!(r[1].named && r[1].at == at("C2"));
        assert!(r[1].state_same && r[1].node_same, "style kept is same: {r:?}");
        assert!(!r[1].validation_same, "the hand-off vetoes the skip: {r:?}");
        assert_eq!(fresh.to_bytes().unwrap(), bytes);
        assert_clean(&fresh);
    }

    #[test]
    fn string_reservation_plans_change_no_byte_and_set_no_flag() {
        // a workbook with an `sst` part, its registry not yet active
        let mut wb = Workbook::new().unwrap();
        let sheet = sheet(&wb, "Sheet1");
        assert!(wb.registries.strings.is_none());
        let bytes = wb.to_bytes().unwrap();
        // `plan_string_write` reserves through the (activating) registry
        let (op, extra) = wb.plan_string_write("hello").unwrap();
        assert!(matches!(op, CellOp::Shared(_)), "{op:?}");
        assert!(extra.string.is_some(), "a reservation-bearing plan");
        assert!(wb.registries.strings.is_some(), "the registry activates invisibly");
        let plan = wb.plan_cell_write(sheet, at("A1"), op, extra).unwrap();
        plan.report(&mut wb).unwrap();
        // and one `plan_write_value` String item, as `write_cells` plans it
        let (op, extra) = wb
            .plan_write_value(at("B2"), crate::worksheet::CellWrite::String("world"))
            .unwrap();
        let plan = wb.plan_cell_write(sheet, at("B2"), op, extra).unwrap();
        plan.report(&mut wb).unwrap();
        assert_eq!(wb.to_bytes().unwrap(), bytes, "string plans change nothing");
        assert_clean(&wb);
    }

    #[test]
    fn plans_and_reports_change_no_byte_and_set_no_flag() {
        let mut wb = Workbook::new().unwrap();
        let sheet = sheet(&wb, "Sheet1");
        wb.worksheet()
            .unwrap()
            .cell_mut(0, 0)
            .unwrap()
            .set_formula("1+1")
            .unwrap();
        let style = Extra {
            string: None,
            style: Some(StyleRequest::Index(0)),
        };
        let ops = [
            (CellOp::Number("1".into()), Extra::default(), "B1"),
            (CellOp::Text("t".into()), Extra::default(), "B2"),
            (CellOp::Shared(0), Extra::default(), "B3"),
            (CellOp::Bool(true), Extra::default(), "B4"),
            (CellOp::Error("#N/A"), Extra::default(), "B5"),
            (
                CellOp::InlineRich(vec![PlannedRichRun {
                    font: None,
                    escaped: "r".into(),
                }]),
                Extra::default(),
                "B6",
            ),
            (CellOp::Formula("1+2".into(), None), Extra::default(), "B7"),
            (CellOp::RemoveFormula, Extra::default(), "A1"),
            (CellOp::ClearValue, Extra::default(), "B8"),
            (CellOp::StyleOnly, style, "A1"),
            (
                CellOp::DynamicFormula("1+1".into(), 1),
                Extra::default(),
                "B9",
            ),
        ];
        for (op, extra, a1) in ops {
            let _ = report_of(&wb, sheet, a1, op, extra);
        }
    }

    #[test]
    fn refused_plans_are_byte_identical() {
        // an array result cell refuses (validates first, changes nothing)
        let mut a = Workbook::from_bytes(synthetic("o1_arrays.xlsx")).unwrap();
        let before = a.to_bytes().unwrap();
        std::assert_matches!(
            a.plan_cell_write(0, at("B2"), CellOp::Number("4".into()), Extra::default()),
            Err(Error::ArrayResultCell { .. })
        );
        assert_eq!(a.to_bytes().unwrap(), before);
        assert_clean(&a);
        // an unparsable formula, a dangling style index
        let mut wb = Workbook::new().unwrap();
        let before = wb.to_bytes().unwrap();
        std::assert_matches!(
            wb.plan_cell_write(0, at("A1"), CellOp::Formula("1+".into(), None), Extra::default()),
            Err(Error::InvalidFormula { .. })
        );
        std::assert_matches!(
            wb.plan_cell_write(
                0,
                at("A1"),
                CellOp::StyleOnly,
                Extra {
                    string: None,
                    style: Some(StyleRequest::Index(99))
                },
            ),
            Err(Error::StyleIndexOutOfRange { .. })
        );
        assert_eq!(wb.to_bytes().unwrap(), before);
        assert_clean(&wb);
    }

    /// Fault injection (an `Err` and a panic) before `Package::apply` and
    /// before each in-place step poisons the workbook (ADR-0017 §2.1).
    #[cfg(feature = "fault-injection")]
    #[test]
    fn faults_on_the_registry_path_poison() {
        use std::panic::{AssertUnwindSafe, catch_unwind};

        use crate::workbook::ApplyStep;
        let input = synthetic("o1_precise.xlsx");
        for step in [ApplyStep::Registry, ApplyStep::Strip, ApplyStep::HandOff, ApplyStep::Target, ApplyStep::LaterTarget, ApplyStep::Index] {
            for panic in [false, true] {
                let mut wb = Workbook::from_bytes(input.clone()).unwrap();
                let p = wb.plan_writes(writes(&wb)).unwrap();
                let reg = ooxml_core::RegistryPlan::new(&wb.pkg);
                if panic {
                    wb.inject_apply_panic(step);
                    let r = catch_unwind(AssertUnwindSafe(|| wb.apply_writes_with(p, reg)));
                    assert!(r.is_err(), "{step:?}: the injected panic fired");
                } else {
                    wb.inject_apply_failure(step);
                    assert!(matches!(wb.apply_writes_with(p, reg), Err(Error::Poisoned(_))), "{step:?}");
                }
                assert!(matches!(wb.to_bytes(), Err(Error::Poisoned(_))), "{step:?} panic={panic}");
            }
        }
    }
}
