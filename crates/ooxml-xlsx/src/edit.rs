//! Cell edits in formula-bearing workbooks (ADR-0003 §3–§4): plan and
//! validate without changing anything, then apply in place in a fixed order.
//! A failure while applying is a bug (the plan proved success); it poisons
//! the workbook so that no partial edit can be saved (§3.1).

use std::collections::{HashMap, HashSet};

use ooxml_core::{ElementSpec, QName, XmlDocument};
use ooxml_schema::sml;

use crate::census::{FCell, FKind, SheetKind};
use crate::error::{Error, Result};
use crate::formula::parse::parse;
use crate::formula::precedents::Rect;
use crate::formula::translate::translate;
use crate::graph::{Graph, precedents_of};
use crate::reference::{CellIndex, CellRange};
use crate::registry::strings::{SharedStrings, StringPlan};
use crate::registry::styles::{StylePlan, StyleRequest, Styles};
use crate::workbook::{CalcPolicy, REL_CALC_CHAIN, Workbook};
use crate::index::{Located, SheetIndex, located_cell, sheet_data};
use crate::worksheet::{TargetPlan, locate, plan_target, write_cell};

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
    /// Removes the value and formula (`v`, `is`, `f`, `t`) of an existing
    /// cell, keeping its style: `merge_clearing` (ADR-0014 §3). Unlike
    /// `RemoveFormula`, it also applies to a cell without a formula.
    Clear,
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

/// What the apply phase installs in the registries.
#[derive(Debug)]
struct RegistryApply {
    string: Option<StringPlan>,
    /// Change in the number of `t="s"` cells (target and stripped cells).
    refs_delta: i64,
    styles: Option<StylePlan>,
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

/// Positions of `c` elements to strip, per sheet.
#[derive(Debug, Default)]
pub(crate) struct StripPlan {
    pub(crate) sheets: Vec<(String, Vec<(usize, usize)>)>,
    /// Stripped cells that had `t="s"`.
    shared_refs: i64,
    fids: Vec<usize>,
    anchors: Vec<usize>,
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

    /// Plans, validates and applies one cell edit.
    pub(crate) fn edit_cell(&mut self, sheet: usize, at: CellIndex, op: CellOp) -> Result<()> {
        self.edit_cell_ext(sheet, at, op, Extra::default())
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

    /// [`Self::edit_cell`] with Phase2 registry work.
    pub(crate) fn edit_cell_ext(&mut self, sheet: usize, at: CellIndex, op: CellOp, extra: Extra) -> Result<()> {
        self.edit_cell_mode(sheet, at, op, extra, false)
    }

    /// Runs every check of [`Self::edit_cell`] without applying anything
    /// (parsing and index building aside, which change no bytes).
    pub(crate) fn check_edit_cell(&mut self, sheet: usize, at: CellIndex, op: CellOp) -> Result<()> {
        self.edit_cell_mode(sheet, at, op, Extra::default(), true)
    }

    fn edit_cell_mode(&mut self, sheet: usize, at: CellIndex, op: CellOp, extra: Extra, dry_run: bool) -> Result<()> {
        self.check_editable()?;
        let part = self.sheets[sheet].part.clone().expect("worksheet");
        let cell = at.to_string();
        let style_only = matches!(op, CellOp::StyleOnly);
        if style_only {
            return self.edit_style_only(sheet, &part, at, extra);
        }

        // ---- guards on the cell ----
        let ctx = self.contexts.get(&sheet).expect("context loaded with the worksheet");
        // Phase4 (ADR-0014 §3): the anchor of a merged range is writable, as
        // in Excel; a value in any other cell would be hidden.
        if let Some(m) = ctx.merges.iter().find(|m| m.contains(at) && m.first != at) {
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
        }
        if matches!(op, CellOp::RemoveFormula) && target.is_none() {
            return Ok(());
        }

        // ---- the target in Layer 2 ----
        self.ensure_index(sheet)?;
        let index = self.indexes.get(&sheet).expect("built");
        let mut target_plan = plan_target(self.pkg.xml_part_mut(&part)?, index, &part, at, &op)?;
        let formula_set_changes = match op {
            CellOp::Formula(_) => target.is_none(),
            _ => target.is_some(),
        };
        // a formula is written or removed: the style-sensitivity answer may change
        let formulas_change = matches!(op, CellOp::Formula(_)) || target.is_some();

        // ---- shared-formula hand-off (§4.2) ----
        let handoff = match target.map(|f| (f, self.census.get(f).expect("alive").clone())) {
            Some((_, c @ FCell { kind: FKind::Shared { si, master: true }, .. })) => self.plan_handoff(&c, si, sheet, &part)?,
            _ => None,
        };

        // ---- dependents and their guards ----
        let deps = self.compute_dependents(sheet, at);
        let others: Vec<_> = deps.cells.iter().filter(|&&c| c != (sheet, at)).copied().collect();
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
            CalcPolicy::StripDependentCaches => self.plan_strip(&deps, sheet, at)?,
            CalcPolicy::KeepStaleCaches => StripPlan::default(),
        };

        // ---- calcPr on a clone of workbook.xml ----
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

        // ---- registries (ADR-0006) ----
        let reg = self.plan_registries(&part, at, &mut target_plan, &op, extra, strip.shared_refs)?;

        if dry_run {
            return Ok(());
        }

        if formulas_change {
            self.style_sensitive = None;
        }
        // ---- apply ----
        // Poisoned *before* apply and cleared only on success: a panic inside
        // apply (a bug) that a caller catches also leaves the workbook
        // poisoned (review R3-m3).
        self.poisoned = Some("an edit was interrupted while being applied".into());
        let result = self.apply(sheet, &part, at, &target_plan, &op, target, chain, staged, &strip, handoff, reg);
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

    /// The style-only path: the cell keeps its value; dependents are
    /// invalidated only in a style-sensitive workbook (§7).
    fn edit_style_only(&mut self, sheet: usize, part: &str, at: CellIndex, extra: Extra) -> Result<()> {
        self.ensure_index(sheet)?;
        let index = self.indexes.get(&sheet).expect("built");
        let mut target_plan = plan_target(self.pkg.xml_part_mut(part)?, index, part, at, &CellOp::StyleOnly)?;
        let (strip, staged) = if self.style_sensitive()? {
            let deps = self.compute_dependents(sheet, at);
            let others: Vec<_> = deps.cells.iter().filter(|&&c| c != (sheet, at)).copied().collect();
            for &fid in &deps.formulas {
                let c = self.census.get(fid).expect("alive");
                if (c.sheet, c.at) != (sheet, at) && matches!(self.sheets[c.sheet].kind, SheetKind::MacroSheet | SheetKind::DialogSheet) {
                    return Err(Error::MacroSheetDependent { sheet: self.sheets[c.sheet].name.clone(), cell: c.at.to_string() });
                }
            }
            if let Some(&(_, c)) = others.iter().find(|k| self.census.vm_cells.contains(k)) {
                return Err(Error::RichValueCell { cell: c.to_string() });
            }
            if self.calc.iterate && !others.is_empty() {
                return Err(Error::IterativeCalculation);
            }
            let strip = match self.policy {
                CalcPolicy::StripDependentCaches => self.plan_strip(&deps, sheet, at)?,
                CalcPolicy::KeepStaleCaches => StripPlan::default(),
            };
            let staged = if !others.is_empty() && !self.calc.manual && !self.calc.full_calc_on_load {
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
            (strip, staged)
        } else {
            (StripPlan::default(), None)
        };
        let reg = self.plan_registries(part, at, &mut target_plan, &CellOp::StyleOnly, extra, strip.shared_refs)?;
        self.poisoned = Some("an edit was interrupted while being applied".into());
        let result = self.apply(sheet, part, at, &target_plan, &CellOp::StyleOnly, None, None, staged, &strip, None, reg);
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
    fn plan_registries(&mut self, part: &str, at: CellIndex, target_plan: &mut TargetPlan, op: &CellOp, extra: Extra, stripped_shared: i64) -> Result<RegistryApply> {
        let (old_t_s, current_s) = {
            let doc = self.pkg.xml_part(part)?;
            match located_cell(doc, &target_plan.loc) {
                Some(c) => (
                    c.attr(sml::Cell::ATTR_T).is_some_and(|t| t.trim() == "s"),
                    c.attr(sml::Cell::ATTR_S).and_then(ooxml_core::value::parse_u32).unwrap_or(0),
                ),
                None => (false, missing_cell_style(doc, &target_plan.loc, at)),
            }
        };
        let styles = match &extra.style {
            Some(req) => {
                self.styles()?;
                let st = self.registries.styles.take().expect("activated");
                let plan = st.plan(self, req, current_s);
                self.registries.styles = Some(st);
                let plan = plan?;
                target_plan.style = Some(plan.s);
                Some(plan)
            }
            None => None,
        };
        let mut refs_delta = -stripped_shared;
        if !matches!(op, CellOp::StyleOnly) {
            refs_delta += i64::from(matches!(op, CellOp::Shared(_))) - i64::from(old_t_s);
        }
        if let Some(sp) = &extra.string {
            let reg = self.registries.strings.as_ref().ok_or_else(|| Error::Malformed("shared-string plan without a registry".into()))?;
            let doc = self.pkg.xml_part(&reg.part)?;
            crate::worksheet::check_insertable::<sml::SharedStringTable>(doc.root().elements(), sml::SharedStringItem::QNAME)?;
            let _ = sp;
        }
        Ok(RegistryApply { string: extra.string, refs_delta, styles })
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
    /// dependent that still has a cached value (§3.1, R2-m1).
    pub(crate) fn plan_strip(&mut self, deps: &DepSet, sheet: usize, at: CellIndex) -> Result<StripPlan> {
        let mut cells: HashMap<usize, HashSet<CellIndex>> = HashMap::new();
        let mut rects: HashMap<usize, Vec<Rect>> = HashMap::new();
        let mut plan = StripPlan::default();
        for &fid in &deps.formulas {
            let c = self.census.get(fid).expect("alive");
            if (c.sheet, c.at) != (sheet, at) && c.has_cache {
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
            let (positions, shared) = strip_positions(doc, self.indexes.get(&s).expect("built"), &part, s, &want, &rs)?;
            plan.shared_refs += shared;
            if !positions.is_empty() {
                plan.sheets.push((part, positions));
            }
        }
        Ok(plan)
    }

    #[expect(clippy::too_many_arguments)]
    fn apply(
        &mut self,
        sheet: usize,
        part: &str,
        at: CellIndex,
        target_plan: &TargetPlan,
        op: &CellOp,
        target: Option<usize>,
        chain: Option<String>,
        staged: Option<XmlDocument>,
        strip: &StripPlan,
        handoff: Option<HandOff>,
        reg: RegistryApply,
    ) -> Result<()> {
        use crate::workbook::ApplyStepHook as Step;
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
        // (iii) dependents
        self.fault_point(Step::Strip)?;
        for (p, positions) in &strip.sheets {
            strip_cells(self.pkg.xml_part_mut(p)?, positions)?;
        }
        // (iv) shared-formula hand-off
        self.fault_point(Step::HandOff)?;
        if let Some(h) = &handoff {
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
        // (v) registries
        self.fault_point(Step::Registries)?;
        if let Some(p) = reg.styles {
            if let Some(doc) = p.staged {
                *self.pkg.xml_part_mut(&p.next.part)? = doc;
            }
            self.registries.styles = Some(p.next);
        }
        SharedStrings::apply(self, reg.string.as_ref(), reg.refs_delta)?;
        // (vi) the target cell
        self.fault_point(Step::Target)?;
        let effect = match write_cell(self.pkg.xml_part_mut(part)?, target_plan, at, op) {
            Ok(effect) => effect,
            Err(e) => {
                // The tree may have changed before the failure (unreachable
                // after a successful plan): keep the index true to the tree,
                // since reads still work on a poisoned workbook (PR #29 m4).
                if let Ok(doc) = self.pkg.xml_part(part) {
                    let index = SheetIndex::build(doc);
                    self.indexes.insert(sheet, index);
                }
                return Err(e);
            }
        };
        if let Some(index) = self.indexes.get_mut(&sheet) {
            effect.update(index, at);
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
        if let Some(h) = handoff
            && let Some(Some(c)) = self.census.cells.get_mut(h.new_master)
        {
            if let FKind::Shared { si, .. } = c.kind {
                c.kind = FKind::Shared { si, master: true };
            }
            c.text = h.text;
            c.range = Some(h.range);
        }
        if let Some(fid) = target {
            self.census.remove(fid);
        }
        if let CellOp::Formula(text) = op {
            let c = FCell {
                sheet,
                at,
                kind: FKind::Normal,
                range: None,
                volatile: false,
                dynamic: false,
                spill_error: false,
                ambiguous_group: false,
                has_cache: false,
                results_stripped: false,
                text: text.clone(),
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
