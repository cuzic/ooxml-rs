//! Cell edits in formula-bearing workbooks (ADR-0003 §3–§4): plan and
//! validate without changing anything, then apply in place in a fixed order.
//! A failure while applying is a bug (the plan proved success); it poisons
//! the workbook so that no partial edit can be saved (§3.1).

use std::collections::{HashMap, HashSet};

use ooxml_core::{ElementSpec, XmlDocument};
use ooxml_schema::sml;

use crate::census::{FCell, FKind, SheetKind};
use crate::error::{Error, Result};
use crate::formula::parse::parse;
use crate::formula::precedents::Rect;
use crate::formula::translate::translate;
use crate::graph::{Graph, precedents_of};
use crate::reference::{CellIndex, CellRange};
use crate::workbook::{CalcPolicy, REL_CALC_CHAIN, Workbook};
use crate::index::{Located, SheetIndex, located_cell, sheet_data};
use crate::worksheet::{locate, plan_target, write_cell};

/// What a setter writes into the target cell.
#[derive(Clone, Debug)]
pub(crate) enum CellOp {
    /// `<v>` text of a number.
    Number(String),
    /// Inline-string text, already `_xHHHH_`-escaped.
    Text(String),
    /// Formula text, without `=`.
    Formula(String),
    RemoveFormula,
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
    /// `workbook.xml` with `calcPr/@fullCalcOnLoad`, cloned from the
    /// current state.
    staged: Option<XmlDocument>,
    /// `calcChain.xml`, to remove.
    chain: Option<String>,
}

/// Positions of `c` elements to strip, per sheet.
#[derive(Debug, Default)]
struct StripPlan {
    sheets: Vec<(String, Vec<(usize, usize)>)>,
    fids: Vec<usize>,
    anchors: Vec<usize>,
}

fn xml_char_ok(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..)
}

impl Workbook {
    fn graph(&mut self) -> &Graph {
        if self.graph.is_none() {
            self.graph = Some(Graph::build(&self.env, &self.census));
        }
        self.graph.as_ref().expect("built")
    }

    /// Dependents of a write to `sheet!at` (§3.2).
    pub(crate) fn compute_dependents(&mut self, sheet: usize, at: CellIndex) -> DepSet {
        self.graph();
        let g = self.graph.as_ref().expect("built");
        let d = g.dependents(&self.census, vec![Rect::cell(sheet, at.row, at.col)]);
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

    /// Plans, validates and applies one cell edit: a [`CellWritePlan`] of
    /// one cell, applied in place.
    pub(crate) fn edit_cell(&mut self, sheet: usize, at: CellIndex, op: CellOp) -> Result<()> {
        let plan = self.plan_writes(vec![(sheet, at, op)])?;
        self.apply_writes(plan)
    }

    /// Plans and validates the writes of one operation together (ADR-0003
    /// addendum P5-pre, §3.1a): every guard of §3.1 for **all** cells, the
    /// union of their dependents, the cache strips, the shared-formula
    /// hand-offs, the `calcPr` change (a clone of `workbook.xml`) and the
    /// calcChain removal. Nothing is changed (parsing a part for inspection
    /// is allowed, as in §3.1).
    ///
    /// The census is accumulated inside the plan: a cell is written at most
    /// once (`DuplicateCellWrite`), at most one cell of each shared-formula
    /// group is touched (`SharedFormulaMaster`), and dependents are taken
    /// from the graph before any of the writes, which is a superset of what
    /// the later writes leave (a formula written earlier in the batch has no
    /// cached value; its own dependents are in the union).
    pub(crate) fn plan_writes(&mut self, writes: Vec<(usize, CellIndex, CellOp)>) -> Result<CellWritePlan> {
        self.check_poisoned()?;
        if let Some(b) = &self.census.block {
            return Err(b.to_error());
        }
        let mut planned: Vec<PlannedWrite> = Vec::with_capacity(writes.len());
        let mut seen: HashSet<(usize, CellIndex)> = HashSet::new();
        let mut groups: HashSet<(usize, u32)> = HashSet::new();
        let mut handoffs = Vec::new();
        let mut deps = DepSet::default();
        let (mut dep_formulas, mut dep_results, mut dep_cells) = (HashSet::new(), HashSet::new(), HashSet::new());
        let mut formula_set_changes = false;
        for (sheet, at, op) in writes {
            let part = self.sheets[sheet].part.clone().expect("worksheet");
            let cell = at.to_string();
            if !seen.insert((sheet, at)) {
                return Err(Error::DuplicateCellWrite { cell });
            }

            // ---- guards on the cell ----
            let ctx = self.contexts.get(&sheet).expect("context loaded with the worksheet");
            if let Some(m) = ctx.merges.iter().find(|m| m.contains(at)) {
                return Err(Error::MergedCell { cell, range: m.to_string() });
            }
            if let Some(t) = ctx.tables.iter().find(|t| t.rows.is_some_and(|r| r.contains(at))) {
                return Err(Error::TableHeaderCell { cell, table: t.name.clone() });
            }
            let target = self.census.at(sheet, at);
            if let Some(fid) = target {
                let c = self.census.get(fid).expect("alive");
                if matches!(c.kind, FKind::Array | FKind::DataTable) || c.dynamic {
                    return Err(Error::ArrayAnchor { cell });
                }
            }
            if let Some(a) = self.census.result_anchor(sheet, at) {
                let anchor = self.census.get(a).expect("alive").at.to_string();
                return Err(Error::ArrayResultCell { cell, anchor });
            }
            if let CellOp::Formula(text) = &op {
                let bad = |reason: String| Error::InvalidFormula { text: text.clone(), reason };
                if let Some(ch) = text.chars().find(|c| !xml_char_ok(*c)) {
                    return Err(bad(format!("character U+{:04X} cannot be stored in XML", ch as u32)));
                }
                parse(text).map_err(|e| bad(format!("{} at byte {}", e.message, e.at)))?;
                // the file form of future functions and dynamic-array operators
                // (ADR-0003 addendum P5-pre, ADR-0017 Q7)
                match crate::formula::future::check(text) {
                    Ok(()) => {}
                    Err(crate::formula::future::Refusal::MissingPrefix { name, prefix }) => return Err(Error::MissingFunctionPrefix { name, prefix }),
                    Err(crate::formula::future::Refusal::Operator { operator, file_form }) => return Err(Error::DynamicArrayOperator { operator, file_form }),
                }
            }
            if matches!(op, CellOp::RemoveFormula) && target.is_none() {
                continue;
            }

            // ---- the target in Layer 2 (validated here; re-located at apply) ----
            self.ensure_index(sheet)?;
            let index = self.indexes.get(&sheet).expect("built");
            plan_target(self.pkg.xml_part_mut(&part)?, index, &part, at, &op)?;
            formula_set_changes |= match op {
                CellOp::Formula(_) => target.is_none(),
                _ => target.is_some(),
            };

            // ---- shared groups: one cell per group per batch; hand-off (§4.2) ----
            if let Some(fid) = target {
                let c = self.census.get(fid).expect("alive").clone();
                if let FKind::Shared { si, master } = c.kind {
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
            let d = self.compute_dependents(sheet, at);
            for fid in d.formulas {
                if dep_formulas.insert(fid) {
                    deps.formulas.push(fid);
                }
            }
            for (fid, r) in d.results {
                if dep_results.insert(fid) {
                    deps.results.push((fid, r));
                }
            }
            for c in d.cells {
                if dep_cells.insert(c) {
                    deps.cells.push(c);
                }
            }
            planned.push(PlannedWrite { sheet, part, at, op, target });
        }
        // a shared member being written whose group's master is handed off
        // was refused above (one cell per group)
        let written: HashSet<(usize, CellIndex)> = planned.iter().map(|w| (w.sheet, w.at)).collect();

        // ---- dependents' guards ----
        let others: Vec<_> = deps.cells.iter().filter(|c| !written.contains(c)).copied().collect();
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

        // ---- strip plan ----
        let strip = match self.policy {
            CalcPolicy::StripDependentCaches => self.plan_strip(&deps, &written)?,
            CalcPolicy::KeepStaleCaches => StripPlan::default(),
        };

        // ---- calcPr on a clone of workbook.xml (cloned from the current state) ----
        let needs_calc = !others.is_empty() || formula_set_changes;
        let staged = if needs_calc && !self.calc.manual && !self.calc.full_calc_on_load {
            let mut doc: XmlDocument = self.pkg.xml_part(&self.workbook_part)?.clone();
            {
                let mut wb = doc.lens_mut::<sml::Workbook>()?;
                let mut calc = wb.get_or_insert::<sml::CalculationProperties>()?;
                calc.set_attr(sml::CalculationProperties::ATTR_FULL_CALC_ON_LOAD, "1")?;
            }
            Some(doc)
        } else {
            None
        };

        // ---- calcChain ----
        let chain = if formula_set_changes { self.calc_chain.clone() } else { None };
        if let Some(p) = &chain {
            self.pkg.check_remove_part(p, &[REL_CALC_CHAIN])?;
        }
        Ok(CellWritePlan { writes: planned, strip, handoffs, staged, chain })
    }

    /// Applies a plan on its own: the calcChain removal and `workbook.xml`
    /// in place, then the in-place part (§3.1 apply order).
    pub(crate) fn apply_writes(&mut self, plan: CellWritePlan) -> Result<()> {
        // Poisoned *before* apply and cleared only on success: a panic inside
        // apply (a bug) that a caller catches also leaves the workbook
        // poisoned (review R3-m3).
        self.poisoned = Some("an edit was interrupted while being applied".into());
        let result = self.apply_steps(plan, false);
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
    #[cfg_attr(not(test), expect(dead_code, reason = "the entry for Phase5 operations (ADR-0017 §2.1); exercised by tests"))]
    pub(crate) fn apply_writes_with(&mut self, plan: CellWritePlan, mut registry: ooxml_core::RegistryPlan) -> Result<()> {
        if let Some(doc) = &plan.staged {
            registry.stage_part(&self.pkg, &self.workbook_part, doc.clone())?;
        }
        if let Some(p) = &plan.chain {
            registry.remove_parts(&self.pkg, &[p.as_str()], &[REL_CALC_CHAIN])?;
        }
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
        let result = self.apply_steps(plan, true);
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
    fn plan_strip(&mut self, deps: &DepSet, written: &HashSet<(usize, CellIndex)>) -> Result<StripPlan> {
        let mut cells: HashMap<usize, HashSet<CellIndex>> = HashMap::new();
        let mut rects: HashMap<usize, Vec<Rect>> = HashMap::new();
        let mut plan = StripPlan::default();
        for &fid in &deps.formulas {
            let c = self.census.get(fid).expect("alive");
            if !written.contains(&(c.sheet, c.at)) && c.has_cache {
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
            let positions = strip_positions(doc, self.indexes.get(&s).expect("built"), &part, s, &want, &rs)?;
            if !positions.is_empty() {
                plan.sheets.push((part, positions));
            }
        }
        Ok(plan)
    }

    /// The in-place steps of §3.1, in order. With `registry_done`, the
    /// calcChain removal and `workbook.xml` were applied by
    /// `Package::apply`; only the in-memory state follows them.
    fn apply_steps(&mut self, plan: CellWritePlan, registry_done: bool) -> Result<()> {
        use crate::workbook::ApplyStepHook as Step;
        let CellWritePlan { writes, strip, handoffs, staged, chain } = plan;
        if registry_done {
            if chain.is_some() {
                self.calc_chain = None;
            }
            if staged.is_some() {
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
            let doc = self.pkg.xml_part_mut(p)?;
            for &(ri, ci) in positions {
                let ws = doc.lens_mut::<sml::Worksheet>()?;
                let sd = ws.into_child_by::<sml::SheetData>(|_| true).ok_or_else(|| Error::Malformed("sheetData".into()))?;
                let row = sd.into_child_at::<sml::Row>(ri).ok_or_else(|| Error::Malformed("planned row".into()))?;
                let cell = row.into_child_at::<sml::Cell>(ci).ok_or_else(|| Error::Malformed("planned cell".into()))?;
                let mut c = sml::CellViewMut(cell);
                // every `v` and `is`, duplicates included (schema-invalid,
                // but a reader would see a survivor), as `write_cell` does
                // (review R4-M1)
                while c.remove_cell_value() {}
                while c.remove_inline_string() {}
                c.remove_data_type()?;
            }
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
        // (v) the target cells, each located again on the current tree: an
        // earlier write of the plan may have inserted its row or cell
        for (k, w) in writes.iter().enumerate() {
            self.fault_point(if k == 0 { Step::Target } else { Step::LaterTarget })?;
            let index = self.indexes.get(&w.sheet).expect("built in plan_writes");
            let written = plan_target(self.pkg.xml_part_mut(&w.part)?, index, &w.part, w.at, &w.op).and_then(|tp| write_cell(self.pkg.xml_part_mut(&w.part)?, &tp, w.at, &w.op));
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
        // (vi) the index
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
        for w in writes {
            if let Some(fid) = w.target {
                self.census.remove(fid);
            }
            if let CellOp::Formula(text) = w.op {
                let c = FCell {
                    sheet: w.sheet,
                    at: w.at,
                    kind: FKind::Normal,
                    range: None,
                    volatile: false,
                    dynamic: false,
                    spill_error: false,
                    ambiguous_group: false,
                    has_cache: false,
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
    fn fault_point(&mut self, step: crate::workbook::ApplyStepHook) -> Result<()> {
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
    fn fault_point(&mut self, _: crate::workbook::ApplyStepHook) -> Result<()> {
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
) -> Result<Vec<(usize, usize)>> {
    let mut out = Vec::new();
    let mut found = HashSet::new();
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
                out.push((e.item, ci));
            }
        }
    }
    if let Some(c) = want.iter().find(|c| !found.contains(c)) {
        return Err(Error::Malformed(format!("{part}: the formula census and the tree disagree about {c}")));
    }
    Ok(out)
}
