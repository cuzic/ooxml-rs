//! Commit = serialize the shadow (ADR-0024 Alternative 9), with the four
//! remedies of README "Commit = shadow serialization":
//!
//! 1. no-op judgment at append (`apply_tracked`);
//! 2. a cell that returns to its Base state is restored (pinned) to its Base
//!    bytes at save;
//! 3. calculation state is deferred: the shadow runs `KeepStaleCaches`, and
//!    the strip set, `fullCalcOnLoad` and the calcChain removal are decided
//!    once at save from the cells whose final state differs from the Base;
//! 4. `sst` / `xf` items added on the shadow and not referenced at save are
//!    dropped; the survivors are numbered canonically after the Base's.
//!
//! `ooxml-xlsx` has no public API to restore a node, undo a dirty flag,
//! defer the calculation rules or compact a registry, so 2–4 run as a thin
//! byte-level layer over the shadow's serialization (`serialize`). The Base's
//! own bytes are the source of every restored piece. README lists the core
//! API each piece stands in for.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use ooxml_core::{Package, PartStateKind, QName, RawNode, XmlDocument};
use ooxml_xlsx::{CalcPolicy, CellIndex, CellValue, Workbook};

use crate::commit::{noop_op, read_cell};
use crate::op::{CellKey, Op, Patch, apply_seq};

pub const STYLES: &str = "xl/styles.xml";
pub const SST: &str = "xl/sharedStrings.xml";
pub const WORKBOOK: &str = "xl/workbook.xml";

/// The policy the shadow runs in this mode (remedy 3). The mutant puts the
/// sequential rule back: caches are stripped at each write, also for writes
/// that are later reverted.
pub const SHADOW_POLICY: CalcPolicy = if cfg!(feature = "mutant_no_defer") { CalcPolicy::StripDependentCaches } else { CalcPolicy::KeepStaleCaches };

/// What a cell holds, for "is it back to its Base state" (remedy 2). The
/// cached value of a formula is kept for the View but not compared.
#[derive(Clone, Debug, PartialEq)]
pub struct Sem {
    pub value: CellValue,
    pub formula: Option<String>,
    pub f_attrs: Vec<(String, String)>,
    pub t: Option<String>,
    pub rich: bool,
    pub s: u32,
    /// A shared member's own text (expanded from its master), else `None`.
    pub expanded: Option<String>,
}

impl Sem {
    /// Same content (value, or formula text and `f` attributes; type).
    pub fn same_content(&self, o: &Sem) -> bool {
        if self.formula.is_some() || o.formula.is_some() {
            return self.formula == o.formula && self.f_attrs == o.f_attrs;
        }
        self.value == o.value && self.t == o.t && self.rich == o.rich
    }
    /// No value and no formula (an empty `c`).
    pub fn is_empty_content(&self) -> bool {
        self.formula.is_none() && matches!(self.value, CellValue::Empty)
    }
    pub fn same_state(&self, o: &Sem) -> bool {
        self.same_content(o) && self.s == o.s
    }
    /// Same content as P sees it (a shared member's text expanded; group
    /// attributes ignored): what R3's strip set is seeded from in the fixed
    /// rule (README "Revision 8 verification", b).
    pub fn same_meaning(&self, o: &Sem) -> bool {
        let text = |s: &Sem| s.expanded.clone().or_else(|| s.formula.clone());
        if self.formula.is_some() || o.formula.is_some() {
            return text(self) == text(o);
        }
        self.value == o.value && self.t == o.t && self.rich == o.rich
    }
}

fn local<'a>(n: &'a RawNode, name: &str) -> Option<&'a RawNode> {
    n.elements().find(|e| e.name().local() == name)
}

pub fn read_sem(wb: &mut Workbook, k: CellKey) -> ooxml_xlsx::Result<Option<Sem>> {
    let ws = wb.worksheet_by_index(k.sheet)?;
    let Some(c) = ws.cell(k.row, k.col)? else { return Ok(None) };
    let raw = c.raw();
    let f_attrs: Vec<(String, String)> = local(raw, "f").map(|f| f.attrs().iter().map(|a| (a.name().local().to_owned(), a.value().to_owned())).collect()).unwrap_or_default();
    let rich = raw.elements().any(|e| e.name().local() == "is" && e.elements().any(|r| r.name().local() == "r"));
    let member = f_attrs.iter().any(|(a, v)| a == "t" && v == "shared") && c.formula().is_some_and(|f| f.is_empty());
    let sem = Sem {
        value: c.value()?,
        formula: c.formula(),
        f_attrs,
        t: raw.attr(QName::new("", "t")).map(str::to_owned),
        rich,
        s: c.style_index()?,
        expanded: None,
    };
    let expanded = if member { expanded_formula(wb, k)? } else { None };
    Ok(Some(Sem { expanded, ..sem }))
}

/// The registry numbering of a shadow: counts at its origin, and the Base
/// index each committed session item got (shadow index → Base index).
#[derive(Clone, Debug, Default)]
pub struct RegState {
    pub origin_sst: u32,
    /// cellXfs, fonts, fills, borders
    pub origin: [u32; 4],
    pub origin_numfmts: BTreeSet<u32>,
    pub sst_map: HashMap<u32, u32>,
    pub maps: [HashMap<u32, u32>; 4],
    pub numfmt_map: HashMap<u32, u32>,
}

const CONTAINERS: [&str; 4] = ["cellXfs", "fonts", "fills", "borders"];
const ID_ATTRS: [&str; 3] = ["fontId", "fillId", "borderId"];

fn part_text(pkg: &Package, name: &str) -> ooxml_xlsx::Result<Option<String>> {
    if !pkg.contains(name) {
        return Ok(None);
    }
    Ok(Some(String::from_utf8(pkg.read_part(name)?).map_err(|e| ooxml_xlsx::Error::Malformed(e.to_string()))?))
}

fn child_count(doc: &XmlDocument, name: &str) -> u32 {
    local(doc.root(), name).map_or(0, |c| c.elements().count() as u32)
}

fn numfmt_ids(doc: &XmlDocument) -> BTreeSet<u32> {
    local(doc.root(), "numFmts").map(|n| n.elements().filter_map(|e| e.attr(QName::new("", "numFmtId"))?.parse().ok()).collect()).unwrap_or_default()
}

impl RegState {
    pub fn at_origin(wb: &Workbook) -> ooxml_xlsx::Result<RegState> {
        let pkg = wb.package();
        let mut r = RegState::default();
        if let Some(t) = part_text(pkg, STYLES)? {
            let d = XmlDocument::parse(t.as_bytes())?;
            for (i, c) in CONTAINERS.iter().enumerate() {
                r.origin[i] = child_count(&d, c);
            }
            r.origin_numfmts = numfmt_ids(&d);
        }
        if let Some(t) = part_text(pkg, SST)? {
            r.origin_sst = XmlDocument::parse(t.as_bytes())?.root().elements().count() as u32;
        }
        Ok(r)
    }
}

/// The deferred calculation decision (remedy 3).
#[derive(Clone, Debug, Default)]
pub struct Calc {
    /// Cells whose cache is stripped (R3).
    pub strips: BTreeSet<CellKey>,
    pub formula_set_changes: bool,
    /// calcChain is removed.
    pub chain: bool,
}

impl Calc {
    pub fn needs_full_calc(&self) -> bool {
        !self.strips.is_empty() || self.formula_set_changes
    }
}

/// Per-shadow state. It lives as long as the shadow: a rebuild by replay
/// starts it again from the Base.
#[derive(Clone, Debug, Default)]
pub struct ShadowState {
    /// First-touch snapshot in this session (vs the Base); `None` = missing.
    pub touched: HashMap<CellKey, Option<Sem>>,
    /// Touched keys whose state equals the snapshot again (remedy 2).
    pub reverted: HashSet<CellKey>,
    /// Restored cells of earlier sessions on this shadow, not touched since:
    /// the output keeps the Base bytes. Value: the Base's cached value.
    pub carried_pins: HashMap<CellKey, CellValue>,
    /// Caches stripped by earlier saves that the shadow still holds.
    pub carried_strips: HashSet<CellKey>,
    /// Every cell applied on this shadow since its origin.
    pub ever: BTreeSet<CellKey>,
    pub reg: RegState,
    /// Cached `Calc` of this session (dropped by every apply).
    pub calc: Option<Calc>,
    pub date1904: bool,
    /// Counters for the measurements.
    pub noops: usize,
    // ---- epochs (revision 8 §4.4; item c of README "Revision 8 verification")
    /// Carried marks: set when a structural op closes a snapshot of a cell
    /// that is not back in its snapshot state. Sticky until the save.
    pub marks: HashMap<CellKey, Mark>,
    /// Structural strip records (§4.2 `calc.strips`), at current positions.
    pub records: BTreeSet<CellKey>,
    /// A structural op of this session met its calcChain condition.
    pub chain_removed: bool,
    /// R4's running `t="s"` delta of closed epochs, minus the `t="s"` cells
    /// structural ops deleted.
    pub closed_sst_delta: i64,
    /// This session's structural ops (positions → Base ids).
    pub structural: Vec<crate::op::Structural>,
    /// Per sheet: the worksheet's text right after the last structural op
    /// that closed its snapshots (`None`: the Base's). Snapshots of the
    /// current epoch are cells of this text.
    pub epoch_src: Vec<Option<std::sync::Arc<String>>>,
    /// The Base this state's shadow was opened from, and its sheet texts.
    pub base: Option<std::sync::Arc<[u8]>>,
    pub base_texts: Option<std::sync::Arc<Vec<String>>>,
    /// Wall time spent in structural ops' materialization and record (the
    /// substitutes for C2 and C3's structural part), for the measurements.
    pub t_structural: std::time::Duration,
}

/// A carried mark (§4.4): `content` if the content differed when the
/// snapshot was closed; `fx` if the formula-ness did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mark {
    pub content: bool,
    pub fx: bool,
}

impl ShadowState {
    pub fn new(wb: &mut Workbook) -> ooxml_xlsx::Result<ShadowState> {
        Ok(ShadowState { reg: RegState::at_origin(wb)?, date1904: wb.date1904()?, ..Default::default() })
    }

    pub fn with_base(wb: &mut Workbook, base: std::sync::Arc<[u8]>) -> ooxml_xlsx::Result<ShadowState> {
        let n = wb.sheet_names().count();
        Ok(ShadowState { base: Some(base), epoch_src: vec![None; n], ..Self::new(wb)? })
    }

    /// Whether the state carries text-layer state from an earlier save.
    pub fn has_carried(&self) -> bool {
        !self.carried_pins.is_empty() || !self.carried_strips.is_empty() || self.reg.maps.iter().any(|m| !m.is_empty()) || !self.reg.sst_map.is_empty()
    }

    fn base_texts(&mut self, wb: &Workbook) -> ooxml_xlsx::Result<std::sync::Arc<Vec<String>>> {
        if self.base_texts.is_none() {
            let base = self.base.clone().ok_or_else(|| malformed("no Base"))?;
            let pkg = Package::from_bytes(base.to_vec())?;
            let parts = sheet_parts(wb)?;
            let texts = parts.iter().map(|p| part_text(&pkg, p).map(|t| t.unwrap_or_default())).collect::<Result<Vec<_>, _>>()?;
            self.base_texts = Some(std::sync::Arc::new(texts));
        }
        Ok(self.base_texts.clone().expect("set"))
    }

    /// The text a snapshot of the current epoch on sheet `i` is a cell of.
    fn epoch_text(&mut self, wb: &Workbook, i: usize) -> ooxml_xlsx::Result<std::sync::Arc<String>> {
        if let Some(Some(t)) = self.epoch_src.get(i) {
            return Ok(t.clone());
        }
        Ok(std::sync::Arc::new(self.base_texts(wb)?[i].clone()))
    }

    /// Where the cell at current position `k` was in the Base (`None`: a
    /// cell of an inserted band).
    pub fn base_pos(&self, k: CellKey) -> Option<CellKey> {
        self.structural.iter().rev().try_fold(k, |k, s| s.unmap(k))
    }

    /// The changed set (§4.4): carried marks and current differences.
    pub fn changed(&self) -> BTreeSet<CellKey> {
        let mut c = self.effective();
        c.extend(self.marks.keys().copied());
        c
    }

    /// The Base `xf` index of a shadow index (a session item not yet saved
    /// keeps its shadow index: README, bisimulation).
    pub fn base_xf(&self, s: u32) -> u32 {
        let base_count = self.reg.origin[0] + self.reg.maps[0].len() as u32;
        match self.reg.maps[0].get(&s) {
            _ if s < self.reg.origin[0] => s,
            Some(b) => *b,
            // unsaved: the shadow's index, moved past the Base's if a save
            // left garbage below it
            None if s >= base_count => s,
            None => base_count + s,
        }
    }

    pub fn is_pinned(&self, k: CellKey) -> bool {
        self.reverted.contains(&k) || self.carried_pins.contains_key(&k)
    }

    /// Changed cells of this session, sorted.
    pub fn effective(&self) -> BTreeSet<CellKey> {
        self.touched.keys().filter(|k| !self.reverted.contains(k)).copied().collect()
    }

    /// The deferred calculation decision, computed once per session state.
    ///
    /// R3 of ADR-0024 revision 8: the strip set is the dependents
    /// (transitively) of the content-changed cells, and **nothing is
    /// subtracted**: every cell of it that holds `f` and a `<v>` in the output
    /// is stripped. The two mutants put back the earlier wordings.
    pub fn calc(&mut self, wb: &mut Workbook) -> ooxml_xlsx::Result<&Calc> {
        if self.calc.is_none() {
            let eff = self.effective();
            let names: Vec<String> = wb.sheet_names().map(str::to_owned).collect();
            let mut c = Calc::default();
            let mut content_changed_set = BTreeSet::new();
            let mut candidates = BTreeSet::new();
            for &k in &eff {
                let snap = self.touched[&k].clone();
                let now = read_sem(wb, k)?;
                let (had_f, has_f) = (snap.as_ref().is_some_and(|s| s.formula.is_some()), now.as_ref().is_some_and(|s| s.formula.is_some()));
                c.formula_set_changes |= had_f != has_f;
                let content_changed = match (&snap, &now) {
                    // revision 8 seeds R3 from §4.4's state equality (raw `f`
                    // attributes): a hand-off's new master then seeds its
                    // dependents, which the sequential side does not strip
                    (Some(a), Some(b)) if cfg!(feature = "adr_r3_seed_raw") => !a.same_content(b),
                    (Some(a), Some(b)) => !a.same_meaning(b),
                    (None, None) => false,
                    // a missing cell's content is empty: a style patch that
                    // creates an empty `c` changes no content (README, c)
                    (None, Some(x)) | (Some(x), None) => cfg!(feature = "adr_missing_is_content") || !x.is_empty_content(),
                };
                // a style-only change strips nothing here: the public API
                // does not say whether the workbook is style-sensitive
                if content_changed {
                    content_changed_set.insert(k);
                    for (sh, at) in wb.dependents(&names[k.sheet], CellIndex { row: k.row, col: k.col })? {
                        candidates.insert(CellKey::new(names.iter().position(|n| *n == sh).expect("sheet"), at.row, at.col));
                    }
                }
            }
            // carried content marks seed like current content changes
            for (&k, m) in &self.marks {
                c.formula_set_changes |= m.fx;
                if m.content && !cfg!(feature = "mutant_drop_marks") {
                    content_changed_set.insert(k);
                    for (sh, at) in wb.dependents(&names[k.sheet], CellIndex { row: k.row, col: k.col })? {
                        candidates.insert(CellKey::new(names.iter().position(|n| *n == sh).expect("sheet"), at.row, at.col));
                    }
                }
            }
            c.formula_set_changes |= self.chain_removed;
            c.chain = c.formula_set_changes;
            // structural strip records (§4.2 `calc.strips`)
            if !cfg!(feature = "mutant_drop_records") {
                candidates.extend(self.records.iter().copied());
            }
            for d in candidates {
                if cfg!(feature = "mutant_r3_subtract_changed") && eff.contains(&d) {
                    continue;
                }
                if cfg!(feature = "mutant_r3_subtract_content") && content_changed_set.contains(&d) {
                    continue;
                }
                if self.holds_cache_at_save(wb, d)? {
                    c.strips.insert(d);
                }
            }
            self.calc = Some(c);
        }
        Ok(self.calc.as_ref().expect("computed"))
    }

    /// Whether the output (before R3) holds `f` and a cached value at `k`:
    /// a restored or pinned cell holds the Base's node, a content-restored
    /// cell (`Act::BaseStyle`) the Base's content, any other the shadow's.
    pub fn holds_cache_at_save(&self, wb: &mut Workbook, k: CellKey) -> ooxml_xlsx::Result<bool> {
        if self.carried_strips.contains(&k) {
            return Ok(false);
        }
        let now = read_sem(wb, k)?;
        let Some(now) = now else { return Ok(false) };
        if now.formula.is_none() {
            // R3 as written in revision 8 strips only cells holding `f`; an
            // array anchor's result cells hold none, so their caches stay
            // stale (README "Revision 8 verification", b). The fix: a result
            // cell of an anchor in the strip set is stripped if it holds `v`.
            return Ok(!cfg!(feature = "adr_r3_formula_only") && now.value != CellValue::Empty && in_array_result(wb, k)?);
        }
        if cfg!(feature = "mutant_r2_no_derived") {
            // round 8 M2's analogue: R3 reads the shadow's own node (its
            // `has_cache`), not the state the restore put back
            return Ok(now.value != CellValue::Empty);
        }
        if let Some(snap) = self.touched.get(&k) {
            if let Some(snap) = snap
                && snap.same_content(&now)
            {
                return Ok(snap.value != CellValue::Empty);
            }
            return Ok(now.value != CellValue::Empty);
        }
        if let Some(v) = self.carried_pins.get(&k) {
            return Ok(*v != CellValue::Empty);
        }
        Ok(now.value != CellValue::Empty)
    }

    /// Strips that apply to the output: this session's and carried ones.
    pub fn all_strips(&mut self, wb: &mut Workbook) -> ooxml_xlsx::Result<BTreeSet<CellKey>> {
        let mut s = self.calc(wb)?.strips.clone();
        s.extend(self.carried_strips.iter().copied());
        Ok(s)
    }

    /// The state after a successful save to the new Base.
    /// The Base's cached value of a formula cell whose content the session
    /// did not change: the output keeps the Base's bytes for it, while the
    /// shadow's `set_formula` may have dropped the cache.
    pub fn base_cache(&self, wb: &mut Workbook, k: CellKey) -> ooxml_xlsx::Result<Option<CellValue>> {
        if let Some(t) = self.touched.get(&k) {
            return Ok(match (t, read_sem(wb, k)?) {
                (Some(snap), Some(now)) if snap.formula.is_some() && now.same_content(snap) => Some(snap.value.clone()),
                _ => None,
            });
        }
        Ok(self.carried_pins.get(&k).cloned())
    }

    pub fn after_commit(&mut self, wb: &mut Workbook, calc: &Calc, reg: RegState) -> ooxml_xlsx::Result<()> {
        // every cell whose output is the Base's content keeps the Base cache
        // in the View (`Act::Base` / `Act::BaseStyle`)
        let keys: Vec<CellKey> = self.touched.keys().copied().collect();
        for k in keys {
            if let Some(v) = self.base_cache(wb, k)? {
                self.carried_pins.insert(k, v);
            } else if self.reverted.contains(&k) {
                let cache = self.touched[&k].as_ref().map_or(CellValue::Empty, |s| s.value.clone());
                self.carried_pins.insert(k, cache);
            }
        }
        self.reverted.clear();
        for k in &calc.strips {
            self.carried_strips.insert(*k);
        }
        self.touched.clear();
        self.calc = None;
        self.reg = reg;
        Ok(())
    }
}

/// Whether a cell holds a shared or array formula, or lies in an array's
/// result range (R1 (b) of revision 8).
fn group_cell(wb: &mut Workbook, k: CellKey) -> ooxml_xlsx::Result<bool> {
    let ws = wb.worksheet_by_index(k.sheet)?;
    for c in ws.cells() {
        let c = c?;
        let Some(f) = local(c.raw(), "f") else { continue };
        let t = f.attr(QName::new("", "t"));
        let at = CellIndex::parse(c.reference())?;
        if (at.row, at.col) == (k.row, k.col) && matches!(t, Some("shared" | "array")) {
            return Ok(true);
        }
        if t == Some("array")
            && let Some((a, b)) = f.attr(QName::new("", "ref")).and_then(range_of)
            && (a.row..=b.row).contains(&k.row)
            && (a.col..=b.col).contains(&k.col)
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Whether `k` is a result cell (not the anchor) of an array formula.
fn in_array_result(wb: &mut Workbook, k: CellKey) -> ooxml_xlsx::Result<bool> {
    let ws = wb.worksheet_by_index(k.sheet)?;
    for c in ws.cells() {
        let c = c?;
        let Some(f) = local(c.raw(), "f") else { continue };
        if f.attr(QName::new("", "t")) == Some("array")
            && let Some((a, b)) = f.attr(QName::new("", "ref")).and_then(range_of)
            && (a.row..=b.row).contains(&k.row)
            && (a.col..=b.col).contains(&k.col)
            && (a.row, a.col) != (k.row, k.col)
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The cell a write to `k` hands its shared group off to (ADR-0003 §4.2,
/// `plan_handoff`: the first member in (row, column) order), if `k` is a
/// master with members. A stand-in for C1's target report (§4.4).
pub fn handoff_target(wb: &mut Workbook, k: CellKey) -> ooxml_xlsx::Result<Option<CellKey>> {
    let Some(sem) = read_sem(wb, k)? else { return Ok(None) };
    let get = |n: &str| sem.f_attrs.iter().find(|(a, _)| a == n).map(|(_, v)| v.clone());
    if get("t").as_deref() != Some("shared") || get("ref").is_none() {
        return Ok(None);
    }
    let si = get("si");
    let ws = wb.worksheet_by_index(k.sheet)?;
    let mut members = Vec::new();
    for c in ws.cells() {
        let c = c?;
        let Some(f) = local(c.raw(), "f") else { continue };
        if f.attr(QName::new("", "t")) == Some("shared") && f.attr(QName::new("", "si")).map(str::to_owned) == si && f.attr(QName::new("", "ref")).is_none() {
            let at = CellIndex::parse(c.reference())?;
            members.push((at.row, at.col));
        }
    }
    Ok(members.into_iter().min().map(|(row, col)| CellKey::new(k.sheet, row, col)))
}

/// The expanded text of a shared member (for the round 8 M1 mutant, which
/// judges R1 by P).
fn expanded_formula(wb: &mut Workbook, k: CellKey) -> ooxml_xlsx::Result<Option<String>> {
    let ws = wb.worksheet_by_index(k.sheet)?;
    let Some(c) = ws.cell(k.row, k.col)? else { return Ok(None) };
    let Some(f) = c.formula() else { return Ok(None) };
    if !f.is_empty() {
        return Ok(Some(f));
    }
    let si = local(c.raw(), "f").and_then(|f| f.attr(QName::new("", "si"))).map(str::to_owned);
    for c in ws.cells() {
        let c = c?;
        let Some(fe) = local(c.raw(), "f") else { continue };
        if fe.attr(QName::new("", "ref")).is_some() && fe.attr(QName::new("", "si")).map(str::to_owned) == si {
            let at = CellIndex::parse(c.reference())?;
            return Ok(Some(crate::project::translate(&fe.text(), i64::from(k.row) - i64::from(at.row), i64::from(k.col) - i64::from(at.col))));
        }
    }
    Ok(None)
}

fn writes_content(op: &Op) -> bool {
    matches!(op, Op::Content(..) | Op::DateTime(..) | Op::NumberWithFormat(..) | Op::WriteCells(_))
}

/// R1 (revision 8, judged on the shadow before the call). In the spike R1
/// runs **before** validation (C1 does not exist), so it is off by default,
/// as ADR-0024 §4.5 says until C1 lands; `r1_unvalidated` turns the old
/// spike's R1 on (F13: it hides refusals). A call is skipped only as a
/// whole, never on a group cell, never on a missing cell.
fn r1_skips(wb: &mut Workbook, st: &ShadowState, op: &Op) -> ooxml_xlsx::Result<bool> {
    if cfg!(feature = "mutant_r1_skip_group")
        && let Op::Content(k, crate::op::Content::Formula(t)) = op
        && read_sem(wb, *k)?.is_some_and(|s| !s.f_attrs.is_empty())
        && expanded_formula(wb, *k)?.as_deref() == Some(t.strip_prefix('=').unwrap_or(t))
    {
        // round 8 M1: judged by P, a group cell's own text written back reads equal
        return Ok(true);
    }
    if !cfg!(feature = "r1_unvalidated") || cfg!(feature = "mutant_no_noop") {
        return Ok(false);
    }
    let parts: Vec<Op> = match op {
        Op::WriteCells(w) => w.iter().map(|(k, c)| Op::Content(*k, c.clone())).collect(),
        _ => vec![op.clone()],
    };
    for p in &parts {
        let k = p.key();
        let cell = read_cell(wb, k)?;
        if cell.is_none() || (!cfg!(feature = "mutant_r1_group_cells") && group_cell(wb, k)?) {
            return Ok(false);
        }
        let style_noop = matches!(p, Op::Style(_, Patch::Index(i)) if cell.as_ref().is_some_and(|c| c.s == *i));
        if !(style_noop || noop_op(&cell, p, st.date1904)) {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Applies one op on the shadow with R1 and R2. Returns whether it was
/// applied (`false` = a no-op that only the log records).
pub fn apply_tracked(wb: &mut Workbook, st: &mut ShadowState, op: &Op) -> ooxml_xlsx::Result<bool> {
    if let Op::Structural(s) = op {
        let t = std::time::Instant::now();
        let r = apply_structural(wb, st, *s);
        st.t_structural += t.elapsed();
        return r.map(|()| true);
    }
    // an op names a Base `xf`; after a save the shadow numbers it otherwise
    let translated;
    let op = match op {
        Op::Style(at, Patch::Index(i)) if *i >= st.reg.origin[0] => match st.reg.maps[0].iter().find(|(_, b)| **b == *i) {
            Some((s, _)) => {
                translated = Op::Style(*at, Patch::Index(*s));
                &translated
            }
            None => op,
        },
        _ => op,
    };
    if r1_skips(wb, st, op)? {
        st.noops += 1;
        return Ok(false);
    }
    // targets (§4.4): the cells the op names, plus a hand-off's new master
    let mut targets = op.keys();
    if writes_content(op) && !cfg!(feature = "mutant_no_handoff_target") {
        for k in op.keys() {
            if let Some(h) = handoff_target(wb, k)?
                && !targets.contains(&h)
            {
                targets.push(h);
            }
        }
    }
    let mut snaps = Vec::new();
    for &k in &targets {
        if st.touched.contains_key(&k) {
            continue;
        }
        let mut s = read_sem(wb, k)?;
        // the snapshot is the Base's state: a carried pin's cache, or none
        // where a save stripped it
        if let Some(s) = &mut s {
            if st.carried_strips.contains(&k) && s.formula.is_some() {
                s.value = CellValue::Empty;
            } else if let Some(v) = st.carried_pins.get(&k) {
                s.value = v.clone();
            }
        }
        snaps.push((k, s));
    }
    apply_seq(wb, op)?;
    for (k, s) in snaps {
        st.touched.insert(k, s);
    }
    st.calc = None;
    for k in targets {
        st.carried_pins.remove(&k);
        st.carried_strips.remove(&k);
        st.ever.insert(k);
        let now = read_sem(wb, k)?;
        let back = match (&st.touched[&k], &now) {
            (Some(a), Some(b)) => a.same_state(b),
            (None, None) => true,
            _ => false,
        };
        if back && !cfg!(feature = "mutant_no_revert") {
            st.reverted.insert(k);
            if cfg!(feature = "mutant_restore_clears_mark") {
                // round 8 M3: a restore that also clears the carried mark
                st.marks.remove(&k);
            }
        } else {
            st.reverted.remove(&k);
        }
    }
    Ok(true)
}

// ------------------------------------------------------ structural ops

fn sheet_parts(wb: &Workbook) -> ooxml_xlsx::Result<Vec<String>> {
    // `worksheet_by_index` needs `&mut`; the part names are in the package's
    // workbook relationships, read through a clone-free path
    let pkg = wb.package();
    let wbx = part_text(pkg, WORKBOOK)?.ok_or_else(|| malformed("workbook"))?;
    let rels = part_text(pkg, "xl/_rels/workbook.xml.rels")?.ok_or_else(|| malformed("rels"))?;
    let mut out = Vec::new();
    let mut pos = 0;
    while let Some(i) = wbx[pos..].find("<sheet ") {
        let a = pos + i;
        let e = a + wbx[a..].find('>').expect("tag");
        let tag = &wbx[a..e];
        let rid = attr(tag, "r:id").ok_or_else(|| malformed("r:id"))?;
        let r = rels.find(&format!("Id=\"{rid}\"")).ok_or_else(|| malformed("rel"))?;
        let rs = rels[..r].rfind('<').expect("tag");
        let re = r + rels[r..].find('>').expect("tag");
        let target = attr(&rels[rs..re], "Target").ok_or_else(|| malformed("Target"))?;
        out.push(format!("xl/{}", target.trim_start_matches("/xl/")));
        pos = e;
    }
    Ok(out)
}

/// Cells holding a cached value: formula cells with `v`, and array result
/// cells with `v`.
pub fn cached_cells(wb: &mut Workbook) -> ooxml_xlsx::Result<BTreeSet<CellKey>> {
    let n = wb.sheet_names().count();
    let mut out = BTreeSet::new();
    for i in 0..n {
        let ws = wb.worksheet_by_index(i)?;
        let mut arrays = Vec::new();
        let mut cells = Vec::new();
        for c in ws.cells() {
            let c = c?;
            let at = CellIndex::parse(c.reference())?;
            let f = local(c.raw(), "f");
            if let Some(f) = f
                && f.attr(QName::new("", "t")) == Some("array")
                && let Some(r) = f.attr(QName::new("", "ref")).and_then(range_of)
            {
                arrays.push(r);
            }
            cells.push((at, f.is_some(), local(c.raw(), "v").is_some()));
        }
        for (at, has_f, has_v) in cells {
            let in_array = arrays.iter().any(|(a, b)| (a.row..=b.row).contains(&at.row) && (a.col..=b.col).contains(&at.col));
            if has_v && (has_f || in_array) {
                out.insert(CellKey::new(i, at.row, at.col));
            }
        }
    }
    Ok(out)
}

/// The R2 restores made physical (the substitute for C2 at an epoch
/// boundary): the shadow's bytes with every restored cell put back as its
/// snapshot's bytes, and `dimension` put back to the epoch's (C5: writes do
/// not grow it). The caller reopens the shadow from them (C6's reparse
/// fallback).
fn materialize(wb: &mut Workbook, st: &mut ShadowState) -> ooxml_xlsx::Result<Vec<u8>> {
    let parts = sheet_parts(wb)?;
    let mut pkg = Package::from_bytes(wb.to_bytes()?)?;
    for (i, p) in parts.iter().enumerate() {
        let src = part_text(&pkg, p)?.ok_or_else(|| malformed(p))?;
        let esrc = st.epoch_text(wb, i)?;
        let mut acts: HashMap<String, Act> = st.reverted.iter().filter(|k| k.sheet == i).map(|k| (a1(*k), Act::Base)).collect();
        // a content restore (only `s` differs) puts the snapshot's node back
        // with the current `s`, its cache included (C2; review round 7, M5)
        let content_back: Vec<(CellKey, u32)> = {
            let mut v = Vec::new();
            for (k, snap) in st.touched.iter().filter(|(k, _)| k.sheet == i && !st.reverted.contains(k)) {
                if let (Some(snap), Some(now)) = (snap, read_sem(wb, *k)?)
                    && now.same_content(snap)
                {
                    v.push((*k, now.s));
                }
            }
            v
        };
        for (k, x) in content_back {
            acts.insert(a1(k), Act::BaseStyle(x));
        }
        let text = sheet_text(&src, &esrc, i, &acts, &HashSet::new(), &[]);
        if text != src {
            pkg.set_part(p, text)?;
        }
    }
    Ok(pkg.to_bytes()?)
}

/// A structural op on the shadow (revision 8 §4.4, §4.2 `calc`, R3, R4):
///
/// 1. close every open snapshot in the workbook: a cell not back in its
///    snapshot state gets a carried mark (content, formula-ness);
/// 2. make the epoch's restores physical and reopen (C2/C6 substitute);
/// 3. record the op's own strips on a copy run under
///    `StripDependentCaches` (the C3 substitute: the cells whose cache the
///    sequential op strips, by position after the op), whether it moved or
///    deleted a formula cell (calcChain), and the `t="s"` cells it deleted;
/// 4. apply the op on the shadow (`KeepStaleCaches`).
///
/// `Err` (the op's refusal) leaves the shadow and the state unchanged.
fn apply_structural(wb: &mut Workbook, st: &mut ShadowState, s: crate::op::Structural) -> ooxml_xlsx::Result<()> {
    let n = wb.sheet_names().count();
    let closes = |k: &CellKey| !cfg!(feature = "mutant_close_own_sheet") || k.sheet == s.sheet();
    // 1. closing marks (read before anything changes)
    let mut new_marks: Vec<(CellKey, Mark)> = Vec::new();
    let mut sst_delta = 0i64;
    for (k, snap) in st.touched.iter().filter(|(k, _)| closes(k)) {
        if st.reverted.contains(k) {
            continue;
        }
        let now = read_sem(wb, *k)?;
        let content = match (snap, &now) {
            (Some(a), Some(b)) if cfg!(feature = "adr_r3_seed_raw") => !a.same_content(b),
            (Some(a), Some(b)) => !a.same_meaning(b),
            (None, None) => false,
            (None, Some(x)) | (Some(x), None) => cfg!(feature = "adr_missing_is_content") || !x.is_empty_content(),
        };
        let f = |x: &Option<Sem>| x.as_ref().is_some_and(|x| x.formula.is_some());
        let tss = |x: &Option<Sem>| x.as_ref().is_some_and(|x| x.t.as_deref() == Some("s"));
        sst_delta += i64::from(tss(&now)) - i64::from(tss(snap));
        new_marks.push((*k, Mark { content, fx: f(snap) != f(&now) }));
    }
    // 2. restores made physical
    let mbytes = materialize(wb, st)?;
    // 3. the op's own record, on a copy
    let mut copy = Workbook::from_bytes(mbytes.clone())?;
    copy.set_calc_policy(CalcPolicy::StripDependentCaches);
    let before = cached_cells(&mut copy)?;
    let mut moved_formula = false;
    let mut tss_deleted = 0i64;
    {
        let ws = copy.worksheet_by_index(s.sheet())?;
        for c in ws.cells() {
            let c = c?;
            let at = CellIndex::parse(c.reference())?;
            let k = CellKey::new(s.sheet(), at.row, at.col);
            if local(c.raw(), "f").is_some() && s.map(k) != Some(k) {
                moved_formula = true;
            }
            if s.map(k).is_none() && c.raw().attr(QName::new("", "t")) == Some("s") {
                tss_deleted += 1;
            }
        }
    }
    s.apply(&mut copy)?;
    let after = cached_cells(&mut copy)?;
    let record: Vec<CellKey> = before.iter().filter_map(|k| s.map(*k)).filter(|k| !after.contains(k)).collect();
    // 4. the shadow
    let mut next = Workbook::from_bytes(mbytes)?;
    set_shadow_policy(&mut next);
    s.apply(&mut next).map_err(|e| malformed(format!("the shadow refused a structural op its copy accepted: {e}")))?;
    *wb = next;
    let texts: Vec<String> = {
        let parts = sheet_parts(wb)?;
        parts.iter().map(|p| part_text(wb.package(), p).map(|t| t.unwrap_or_default())).collect::<Result<_, _>>()?
    };
    // the state follows the op
    let remap_set = |set: &BTreeSet<CellKey>| -> BTreeSet<CellKey> { set.iter().filter_map(|k| s.map(*k)).collect() };
    for (k, m) in new_marks {
        let e = st.marks.entry(k).or_default();
        e.content |= m.content;
        e.fx |= m.fx;
    }
    // a mark whose id dies leaves the changed set, but a formula it gained
    // or lost still changed the formula-cell set (calcChain; README, c)
    if !cfg!(feature = "adr_dead_mark_drops_fx") && st.marks.iter().any(|(k, m)| m.fx && s.map(*k).is_none()) {
        st.chain_removed = true;
    }
    st.marks = st.marks.drain().filter_map(|(k, m)| s.map(k).map(|n| (n, m))).collect();
    st.records = remap_set(&st.records);
    st.records.extend(record);
    st.ever = remap_set(&st.ever);
    st.touched = st.touched.drain().filter(|(k, _)| !closes(k)).filter_map(|(k, v)| s.map(k).map(|n| (n, v))).collect();
    st.reverted = st.reverted.drain().filter(|k| !closes(k)).filter_map(|k| s.map(k)).collect();
    st.chain_removed |= moved_formula;
    st.closed_sst_delta += sst_delta - if cfg!(feature = "mutant_no_sst_removed") { 0 } else { tss_deleted };
    st.structural.push(s);
    for (i, text) in texts.into_iter().enumerate().take(n) {
        if !cfg!(feature = "mutant_close_own_sheet") || i == s.sheet() {
            st.epoch_src[i] = Some(std::sync::Arc::new(text));
        }
    }
    st.calc = None;
    Ok(())
}

// ------------------------------------------------------------ text layer

fn attr_span(tag: &str, name: &str) -> Option<(usize, usize)> {
    let pat = format!(" {name}=\"");
    let a = tag.find(&pat)? + pat.len();
    Some((a, a + tag[a..].find('"')?))
}

fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    attr_span(tag, name).map(|(a, b)| &tag[a..b])
}

fn set_attr(tag: &str, name: &str, value: &str) -> String {
    match attr_span(tag, name) {
        Some((a, b)) => format!("{}{value}{}", &tag[..a], &tag[b..]),
        None => tag.to_owned(),
    }
}

/// One `<c>` element of a worksheet's text.
struct CellAt<'a> {
    start: usize,
    open_end: usize,
    end: usize,
    r: &'a str,
}

fn cells(text: &str) -> Vec<CellAt<'_>> {
    let mut out = Vec::new();
    let Some(sd) = text.find("<sheetData") else { return out };
    let stop = text[sd..].find("</sheetData>").map_or(sd, |e| sd + e);
    let mut pos = sd;
    while let Some(i) = text[pos..stop].find("<c ") {
        let start = pos + i;
        let open_end = start + text[start..].find('>').expect("tag end") + 1;
        let end = if text.as_bytes()[open_end - 2] == b'/' { open_end } else { open_end + text[open_end..].find("</c>").expect("close") + 4 };
        let r = attr(&text[start..open_end], "r").unwrap_or("");
        out.push(CellAt { start, open_end, end, r });
        pos = end;
    }
    out
}

/// Removes the cached value (`v`, `is`) and `t` of one cell (as
/// `strip_cells` in `edit.rs`).
fn strip_cell(c: &str) -> String {
    let open_end = c.find('>').expect("tag") + 1;
    let tag = &c[..open_end];
    let tag = match attr_span(tag, "t") {
        Some((a, b)) => format!("{}{}", &tag[..a - 4], &tag[b + 1..]),
        None => tag.to_owned(),
    };
    let mut body = c[open_end..].to_owned();
    for (o, cl) in [("<v>", "</v>"), ("<is>", "</is>")] {
        while let Some(a) = body.find(o) {
            let b = a + body[a..].find(cl).expect("close") + cl.len();
            body.replace_range(a..b, "");
        }
    }
    body = body.replace("<v/>", "");
    format!("{tag}{body}")
}

fn a1(k: CellKey) -> String {
    CellIndex { row: k.row, col: k.col }.to_string()
}

fn range_of(r: &str) -> Option<(CellIndex, CellIndex)> {
    let (a, b) = r.split_once(':').unwrap_or((r, r));
    Some((CellIndex::parse(a).ok()?, CellIndex::parse(b).ok()?))
}

/// What the output holds for one cell the shadow wrote.
#[derive(Clone, Debug)]
enum Act {
    /// The Base's bytes: a restored cell, or one written in an earlier
    /// session and not since.
    Base,
    /// Given bytes (a cell restored to a later epoch's snapshot, renumbered).
    Text(String),
    /// The Base's bytes with `s` set: the content is the Base's, only the
    /// style changed (the shadow's node may have lost the cache or carry
    /// `s="0"`; the Base's node has neither problem).
    BaseStyle(u32),
    /// The shadow's bytes in canonical form (C7), registry indices
    /// renumbered. `zero`: the Base node (by id) has `s` (`Some(true)`), has
    /// none (`Some(false)`), or there is none (`None`): `xf` 0 is written as
    /// the Base cell wrote it; a created cell keeps the shadow's form.
    Shadow { xf: u32, sst: Option<u32>, zero: Option<bool> },
}

fn set_s(tag: &str, x: u32, zero: Option<bool>) -> String {
    if x == 0 && zero.is_none() {
        return match attr_span(tag, "s") {
            Some(_) => set_attr(tag, "s", "0"),
            None => tag.to_owned(),
        };
    }
    let tag = match attr_span(tag, "s") {
        Some((a, b)) => format!("{}{}", &tag[..a - 4], &tag[b + 1..]),
        None => tag.to_owned(),
    };
    if x == 0 && zero == Some(false) {
        return tag;
    }
    let cut = if tag.ends_with("/>") { tag.len() - 2 } else { tag.len() - 1 };
    format!("{} s=\"{x}\"{}", &tag[..cut], &tag[cut..])
}

/// One sheet's output text: the shadow's text with each written cell as its
/// `Act` says, caches stripped, and `dimension` = the Base's grown by this
/// session's changed cells only.
fn sheet_text(src: &str, base: &str, sheet: usize, acts: &HashMap<String, Act>, strips: &HashSet<String>, eff: &[CellKey]) -> String {
    let needs_base = acts.values().any(|a| !matches!(a, Act::Shadow { .. }));
    let base_cells: HashMap<&str, &str> = if needs_base { cells(base).into_iter().filter(|c| acts.contains_key(c.r)).map(|c| (c.r, &base[c.start..c.end])).collect() } else { HashMap::new() };
    let mut out = String::with_capacity(src.len());
    let mut last = 0;
    // dimension
    let dim_at = |t: &str| t.find("<dimension").map(|a| (a, a + t[a..].find('>').expect("tag") + 1));
    if let (Some((sa, sb)), Some((ba, bb))) = (dim_at(src), dim_at(base)) {
        let belem = &base[ba..bb];
        let mut elem = belem.to_owned();
        if let Some((mut lo, mut hi)) = attr(belem, "ref").and_then(range_of) {
            let mut grew = false;
            for k in eff.iter().filter(|k| k.sheet == sheet) {
                if !(lo.row..=hi.row).contains(&k.row) || !(lo.col..=hi.col).contains(&k.col) {
                    grew = true;
                    lo = CellIndex { row: lo.row.min(k.row), col: lo.col.min(k.col) };
                    hi = CellIndex { row: hi.row.max(k.row), col: hi.col.max(k.col) };
                }
            }
            if grew {
                elem = set_attr(belem, "ref", &format!("{lo}:{hi}"));
            }
        }
        out.push_str(&src[..sa]);
        out.push_str(&elem);
        last = sb;
    }
    if acts.is_empty() && strips.is_empty() {
        out.push_str(&src[last..]);
        return out;
    }
    for c in cells(src) {
        let (act, strip) = (acts.get(c.r), strips.contains(c.r));
        if act.is_none() && !strip {
            continue;
        }
        let base_of = || base_cells.get(c.r).map(|b| (*b).to_owned()).expect("the cell exists in the Base");
        let mut s = match act {
            None => src[c.start..c.end].to_owned(),
            Some(Act::Base) => base_of(),
            Some(Act::Text(t)) => t.clone(),
            Some(Act::BaseStyle(x)) => {
                let b = base_of();
                let oe = b.find('>').expect("tag") + 1;
                format!("{}{}", set_s(&b[..oe], *x, Some(true)), &b[oe..])
            }
            Some(Act::Shadow { xf, sst, zero }) => {
                let oe = c.open_end - c.start;
                let mut tag = set_s(&src[c.start..c.open_end], *xf, *zero);
                let mut body = src[c.start + oe..c.end].to_owned();
                if body == "</c>" {
                    // a node that lost its children keeps the open/close form
                    tag = format!("{}/>", tag.trim_end_matches('>'));
                    body.clear();
                }
                if let Some(i) = sst {
                    let a = body.find("<v>").expect("v") + 3;
                    let b = a + body[a..].find("</v>").expect("v");
                    body.replace_range(a..b, &i.to_string());
                }
                tag + &body
            }
        };
        if strip {
            s = strip_cell(&s);
        }
        out.push_str(&src[last..c.start]);
        out.push_str(&s);
        last = c.end;
    }
    out.push_str(&src[last..]);
    out
}

/// Splices `(start, end, text)` into `s` (non-overlapping).
fn splice(s: &str, mut edits: Vec<(usize, usize, String)>) -> String {
    edits.sort_by_key(|e| e.0);
    let mut out = String::with_capacity(s.len() + edits.iter().map(|e| e.2.len()).sum::<usize>());
    let mut last = 0;
    for (a, b, t) in edits {
        out.push_str(&s[last..a]);
        out.push_str(&t);
        last = b;
    }
    out.push_str(&s[last..]);
    out
}

/// Appends `items` to container `name` of `doc` (text `s`), setting its
/// `count`; creates the container after the root's start tag if absent.
fn append_items(doc: &XmlDocument, s: &str, name: &str, items: &[String], extra_attrs: &[(&str, String)], edits: &mut Vec<(usize, usize, String)>) {
    if items.is_empty() {
        return;
    }
    let joined: String = items.concat();
    match local(doc.root(), name) {
        Some(c) => {
            let sp = c.span().expect("Base node");
            let n = c.elements().count() + items.len();
            let mut tag = s[sp.start..sp.open_end].to_owned();
            if sp.is_self_closing() {
                tag = format!("{}>", tag.trim_end_matches("/>").trim_end());
            }
            tag = set_attr(&tag, "count", &n.to_string());
            for (a, v) in extra_attrs {
                tag = set_attr(&tag, a, v);
            }
            match sp.close {
                Some((cs, _)) => {
                    edits.push((sp.start, sp.open_end, tag));
                    edits.push((cs, cs, joined));
                }
                None => edits.push((sp.start, sp.open_end, format!("{tag}{joined}</{name}>"))),
            }
        }
        None => {
            let sp = doc.root().span().expect("root");
            edits.push((sp.open_end, sp.open_end, format!(r#"<{name} count="{}">{joined}</{name}>"#, items.len())));
        }
    }
}

fn items_of(doc: &XmlDocument, s: &str, name: Option<&str>) -> Vec<String> {
    let node = match name {
        Some(n) => local(doc.root(), n),
        None => Some(doc.root()),
    };
    node.map(|c| c.elements().map(|e| { let sp = e.span().expect("parsed"); s[sp.start..sp.end()].to_owned() }).collect()).unwrap_or_default()
}

/// The result of `serialize`: the bytes and the registry numbering that the
/// shadow keeps if the save succeeds.
pub struct Serialized {
    pub bytes: Vec<u8>,
    pub reg: RegState,
    pub calc: Calc,
    pub effective: Vec<CellKey>,
    pub parts_rewritten: usize,
    pub t_calc: std::time::Duration,
    pub t_registries: std::time::Duration,
    pub t_sheets: std::time::Duration,
    pub t_zip: std::time::Duration,
}

fn malformed(s: impl std::fmt::Display) -> ooxml_xlsx::Error {
    ooxml_xlsx::Error::Malformed(s.to_string())
}

/// Commit in this mode: the shadow's parts, through remedies 2–4, written
/// over a package opened (lazily, nothing parsed) from the Base, so every
/// part whose text equals the Base's is raw-copied.
pub fn serialize(wb: &mut Workbook, st: &mut ShadowState, base: &[u8]) -> ooxml_xlsx::Result<Serialized> {
    let t0 = std::time::Instant::now();
    let calc = st.calc(wb)?.clone();
    let strips = st.all_strips(wb)?;
    let t_calc = t0.elapsed();
    let t1 = std::time::Instant::now();
    let mut out = Package::from_bytes(base.to_vec())?;
    // the changed set: carried marks and current differences (§4.4)
    let eff: Vec<CellKey> = st.changed().into_iter().collect();
    let cur: Vec<CellKey> = st.effective().into_iter().collect();
    let structural = !st.structural.is_empty();
    let mut reg = st.reg.clone();

    // ---- registries (remedy 4): references of the cells this shadow wrote
    let mut xf_refs: Vec<u32> = Vec::new();
    let mut sst_refs: Vec<u32> = Vec::new();
    let mut cell_refs: Vec<(CellKey, u32, Option<u32>)> = Vec::new();
    for &k in &st.ever {
        // a cell restored to a later epoch's snapshot may reference a
        // session item (its snapshot does): its references count (R4)
        if st.is_pinned(k) && !(structural && st.reverted.contains(&k)) {
            continue;
        }
        let ws = wb.worksheet_by_index(k.sheet)?;
        let Some(c) = ws.cell(k.row, k.col)? else { continue };
        let s = c.style_index()?;
        let raw = c.raw();
        let sst = (raw.attr(QName::new("", "t")) == Some("s")).then(|| local(raw, "v").and_then(|v| v.text().trim().parse::<u32>().ok())).flatten();
        cell_refs.push((k, s, sst));
        if s >= reg.origin[0] && !xf_refs.contains(&s) {
            xf_refs.push(s);
        }
        if let Some(i) = sst.filter(|i| *i >= reg.origin_sst)
            && !sst_refs.contains(&i)
        {
            sst_refs.push(i);
        }
    }
    if cfg!(feature = "mutant_no_gc") {
        // every session item, in shadow order
        let d = part_text(wb.package(), STYLES)?.map(|t| XmlDocument::parse(t.as_bytes())).transpose()?;
        xf_refs = (reg.origin[0]..d.as_ref().map_or(0, |d| child_count(d, "cellXfs"))).collect();
        let n = part_text(wb.package(), SST)?.map(|t| XmlDocument::parse(t.as_bytes()).map(|d| d.root().elements().count() as u32)).transpose()?.unwrap_or(0);
        sst_refs = (reg.origin_sst..n).collect();
    }
    let new_xfs: Vec<u32> = xf_refs.into_iter().filter(|s| !reg.maps[0].contains_key(s)).collect();
    let new_sst: Vec<u32> = sst_refs.into_iter().filter(|i| !reg.sst_map.contains_key(i)).collect();

    if !new_xfs.is_empty() {
        let shadow_t = part_text(wb.package(), STYLES)?.ok_or_else(|| malformed("no styles part"))?;
        let base_t = part_text(&out, STYLES)?.ok_or_else(|| malformed("no styles part"))?;
        let (sd, bd) = (XmlDocument::parse(shadow_t.as_bytes())?, XmlDocument::parse(base_t.as_bytes())?);
        let shadow_items: Vec<Vec<String>> = CONTAINERS.iter().map(|c| items_of(&sd, &shadow_t, Some(c))).collect();
        let base_counts: Vec<u32> = CONTAINERS.iter().map(|c| child_count(&bd, c)).collect();
        let mut added: [Vec<String>; 4] = Default::default();
        // numFmts: by id
        let shadow_nf: HashMap<u32, String> = local(sd.root(), "numFmts")
            .map(|n| n.elements().filter_map(|e| { let id = e.attr(QName::new("", "numFmtId"))?.parse().ok()?; let sp = e.span()?; Some((id, shadow_t[sp.start..sp.end()].to_owned())) }).collect())
            .unwrap_or_default();
        let mut next_nf = numfmt_ids(&bd).into_iter().max().unwrap_or(163).max(163) + 1;
        let mut added_nf: Vec<String> = Vec::new();
        for s in &new_xfs {
            let mut xf = shadow_items[0].get(*s as usize).cloned().ok_or_else(|| malformed(format!("xf {s}")))?;
            let tag_end = xf.find('>').expect("tag") + 1;
            let mut tag = xf[..tag_end].to_owned();
            for (ci, a) in ID_ATTRS.iter().enumerate() {
                let c = ci + 1;
                let Some(id) = attr(&tag, a).and_then(|v| v.parse::<u32>().ok()) else { continue };
                if id < reg.origin[c] {
                    continue;
                }
                let to = match reg.maps[c].get(&id) {
                    Some(t) => *t,
                    None => {
                        let t = base_counts[c] + added[c].len() as u32;
                        added[c].push(shadow_items[c].get(id as usize).cloned().ok_or_else(|| malformed(format!("{a} {id}")))?);
                        reg.maps[c].insert(id, t);
                        t
                    }
                };
                tag = set_attr(&tag, a, &to.to_string());
            }
            if let Some(id) = attr(&tag, "numFmtId").and_then(|v| v.parse::<u32>().ok())
                && id >= 164
                && !reg.origin_numfmts.contains(&id)
            {
                let to = match reg.numfmt_map.get(&id) {
                    Some(t) => *t,
                    None => {
                        let t = next_nf;
                        next_nf += 1;
                        let e = shadow_nf.get(&id).ok_or_else(|| malformed(format!("numFmt {id}")))?;
                        added_nf.push(set_attr(e, "numFmtId", &t.to_string()));
                        reg.numfmt_map.insert(id, t);
                        t
                    }
                };
                tag = set_attr(&tag, "numFmtId", &to.to_string());
            }
            xf.replace_range(..tag_end, &tag);
            reg.maps[0].insert(*s, base_counts[0] + added[0].len() as u32);
            added[0].push(xf);
        }
        let mut edits = Vec::new();
        append_items(&bd, &base_t, "numFmts", &added_nf, &[], &mut edits);
        for (i, c) in CONTAINERS.iter().enumerate() {
            append_items(&bd, &base_t, c, &added[i], &[], &mut edits);
        }
        out.set_part(STYLES, splice(&base_t, edits))?;
    }

    // sst: new items, and `count` moved by the changed cells' `t="s"`
    let delta: i64 = st.closed_sst_delta
        + cur
        .iter()
        .map(|k| {
            let was = st.touched[k].as_ref().is_some_and(|s| s.t.as_deref() == Some("s"));
            let is = cell_refs.iter().any(|(c, _, i)| c == k && i.is_some());
            i64::from(is) - i64::from(was)
        })
        .sum::<i64>();
    if !new_sst.is_empty() || delta != 0 {
        let shadow_t = part_text(wb.package(), SST)?.ok_or_else(|| malformed("no sst part"))?;
        let base_t = part_text(&out, SST)?.ok_or_else(|| malformed("no sst part (Base)"))?;
        let (sd, bd) = (XmlDocument::parse(shadow_t.as_bytes())?, XmlDocument::parse(base_t.as_bytes())?);
        let items = items_of(&sd, &shadow_t, None);
        let base_n = bd.root().elements().count() as u32;
        let mut added = Vec::new();
        for i in &new_sst {
            reg.sst_map.insert(*i, base_n + added.len() as u32);
            added.push(items.get(*i as usize).cloned().ok_or_else(|| malformed(format!("si {i}")))?);
        }
        let root = bd.root();
        let count = root.attr(QName::new("", "count")).and_then(|c| c.parse::<i64>().ok()).unwrap_or(0) + delta;
        let sp = root.span().expect("root");
        let mut tag = set_attr(&base_t[sp.start..sp.open_end], "count", &count.to_string());
        tag = set_attr(&tag, "uniqueCount", &(base_n as usize + added.len()).to_string());
        let mut edits = vec![(sp.start, sp.open_end, tag)];
        if let Some((cs, _)) = sp.close {
            edits.push((cs, cs, added.concat()));
        } else if !added.is_empty() {
            return Err(malformed("self-closing sst"));
        }
        out.set_part(SST, splice(&base_t, edits))?;
    }
    let t_registries = t1.elapsed();

    // ---- worksheets (remedies 2, 3 and the renumbering of 4)
    let t2 = std::time::Instant::now();
    let names: Vec<String> = (0..wb.sheet_names().count()).map(|i| wb.worksheet_by_index(i).map(|w| w.part_name().to_owned())).collect::<Result<_, _>>()?;
    let mut parts_rewritten = 0;
    for (i, p) in names.iter().enumerate() {
        // after a structural op the shadow was reopened: every sheet is a
        // candidate, compared with the Base's text below
        let dirty = structural || matches!(wb.package().state(p), Some(PartStateKind::Parsed { dirty: true }));
        let strips_here: HashSet<String> = strips.iter().filter(|k| k.sheet == i).map(|k| a1(*k)).collect();
        if !dirty && strips_here.is_empty() {
            continue;
        }
        let base_t = part_text(&out, p)?.ok_or_else(|| malformed(p))?;
        let src = if dirty { part_text(wb.package(), p)?.expect("part") } else { base_t.clone() };
        // snapshots of the current epoch are cells of the epoch's text
        let esrc = if structural || cfg!(feature = "mutant_no_rebase") {
            // the mutant: restores read the shadow's origin (the first Base),
            // as a kept shadow without C6 would (review round 7, M1)
            st.epoch_text(wb, i)?.as_ref().clone()
        } else {
            base_t.clone()
        };
        // the Base node's `s` (by id) of the cells C7 writes
        let needed: HashSet<String> = cell_refs.iter().filter(|c| c.0.sheet == i).filter_map(|c| st.base_pos(c.0)).map(a1).collect();
        let base_s: HashMap<String, bool> = if needed.is_empty() {
            HashMap::new()
        } else {
            cells(&base_t).into_iter().filter(|c| needed.contains(c.r)).map(|c| (c.r.to_owned(), attr(&base_t[c.start..c.open_end], "s").is_some())).collect()
        };
        let mut acts: HashMap<String, Act> = HashMap::new();
        for k in st.reverted.iter().chain(st.carried_pins.keys()).filter(|k| k.sheet == i) {
            acts.insert(a1(*k), Act::Base);
        }
        for (k, s, sst) in cell_refs.iter().filter(|c| c.0.sheet == i) {
            let xf = if *s >= reg.origin[0] { reg.maps[0][s] } else { *s };
            let sst = sst.map(|v| if v >= reg.origin_sst { reg.sst_map[&v] } else { v });
            let zero = st.base_pos(*k).and_then(|b| base_s.get(&a1(b)).copied());
            let act = match st.touched.get(k) {
                // restored (or content-restored) to this epoch's snapshot,
                // after a structural op: the snapshot's bytes with the
                // current `s`, renumbered (R4), in canonical form (C7)
                Some(Some(snap)) if structural && (st.reverted.contains(k) || read_sem(wb, *k)?.is_some_and(|now| now.same_content(snap))) => {
                    let cs = cells(&esrc);
                    let Some(c) = cs.iter().find(|c| c.r == a1(*k)) else { return Err(malformed(format!("no snapshot bytes for {k:?}"))) };
                    let mut tag = set_s(&esrc[c.start..c.open_end], xf, zero);
                    let mut body = esrc[c.open_end..c.end].to_owned();
                    if body == "</c>" {
                        tag = format!("{}/>", tag.trim_end_matches('>'));
                        body.clear();
                    }
                    if let Some(i) = sst {
                        let a = body.find("<v>").expect("v") + 3;
                        let b = a + body[a..].find("</v>").expect("v");
                        body.replace_range(a..b, &i.to_string());
                    }
                    Act::Text(tag + &body)
                }
                // a carried mark: canonical form (C7), a later epoch's restore included
                None if st.marks.contains_key(k) => Act::Shadow { xf, sst, zero },
                // restored in an earlier epoch of this session: the node is
                // the restored one, as the structural ops left it
                None if structural => continue,
                // written in an earlier session and not since
                None => Act::Base,
                Some(Some(snap)) if read_sem(wb, *k)?.is_some_and(|now| now.same_content(snap)) => Act::BaseStyle(xf),
                Some(_) => Act::Shadow { xf, sst, zero },
            };
            acts.insert(a1(*k), act);
        }
        let text = sheet_text(&src, &esrc, i, &acts, &strips_here, &eff);
        if text != base_t {
            parts_rewritten += 1;
            out.set_part(p, text)?;
        }
    }
    let t_sheets = t2.elapsed();

    // ---- calculation state (remedy 3)
    if structural {
        // structural ops may rewrite defined names: the shadow's
        // `workbook.xml`, without the calc flag the shadow set on its own
        let base_wb = part_text(&out, WORKBOOK)?.ok_or_else(|| malformed("workbook"))?;
        let mut sw = part_text(wb.package(), WORKBOOK)?.ok_or_else(|| malformed("workbook"))?;
        if !base_wb.contains(r#"fullCalcOnLoad="1""#) {
            sw = sw.replace(r#" fullCalcOnLoad="1""#, "");
        }
        if sw != base_wb {
            out.set_part(WORKBOOK, sw)?;
        }
    }
    let wb_base = part_text(&out, WORKBOOK)?.ok_or_else(|| malformed("workbook"))?;
    if calc.needs_full_calc() && !wb_base.contains(r#"fullCalcOnLoad="1""#) && !wb_base.contains(r#"calcMode="manual""#) {
        // set on the Base's text: the shadow's own flag is not a superset
        // (a later write can need it when no earlier one did; long proptest)
        let text = match wb_base.find("<calcPr") {
            Some(at) => {
                let end = at + wb_base[at..].find('>').expect("tag");
                let cut = if wb_base.as_bytes()[end - 1] == b'/' { end - 1 } else { end };
                format!("{} fullCalcOnLoad=\"1\"{}", &wb_base[..cut], &wb_base[cut..])
            }
            None => {
                // no `calcPr`: take the shadow's, which placed one by the schema
                let shadow_wb = part_text(wb.package(), WORKBOOK)?.expect("workbook");
                if !shadow_wb.contains(r#"fullCalcOnLoad="1""#) {
                    return Err(malformed("no calcPr to set fullCalcOnLoad on"));
                }
                shadow_wb
            }
        };
        out.set_part(WORKBOOK, text)?;
    }
    let chain = out.part_names().find(|n| n.ends_with("calcChain.xml")).map(str::to_owned);
    if calc.chain
        && let Some(cc) = chain
    {
        out.remove_part(&cc, &[ooxml_core::opc::REL_CALC_CHAIN])?;
    }
    let t3 = std::time::Instant::now();
    let bytes = out.to_bytes()?;
    Ok(Serialized { bytes, reg, calc, effective: eff, parts_rewritten, t_calc, t_registries, t_sheets, t_zip: t3.elapsed() })
}

/// For tests: the keys a save restores to the Base, by sheet part.
pub fn pins_by_sheet(st: &ShadowState) -> BTreeMap<usize, Vec<CellKey>> {
    let mut m: BTreeMap<usize, Vec<CellKey>> = BTreeMap::new();
    for k in st.reverted.iter().chain(st.carried_pins.keys()) {
        m.entry(k.sheet).or_default().push(*k);
    }
    m
}

pub fn set_shadow_policy(wb: &mut Workbook) {
    wb.set_calc_policy(SHADOW_POLICY);
}
