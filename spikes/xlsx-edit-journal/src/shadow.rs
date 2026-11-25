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
}

impl Sem {
    /// Same content (value, or formula text and `f` attributes; type).
    pub fn same_content(&self, o: &Sem) -> bool {
        if self.formula.is_some() || o.formula.is_some() {
            return self.formula == o.formula && self.f_attrs == o.f_attrs;
        }
        self.value == o.value && self.t == o.t && self.rich == o.rich
    }
    pub fn same_state(&self, o: &Sem) -> bool {
        self.same_content(o) && self.s == o.s
    }
}

fn local<'a>(n: &'a RawNode, name: &str) -> Option<&'a RawNode> {
    n.elements().find(|e| e.name().local() == name)
}

pub fn read_sem(wb: &mut Workbook, k: CellKey) -> ooxml_xlsx::Result<Option<Sem>> {
    let ws = wb.worksheet_by_index(k.sheet)?;
    let Some(c) = ws.cell(k.row, k.col)? else { return Ok(None) };
    let raw = c.raw();
    let f_attrs = local(raw, "f").map(|f| f.attrs().iter().map(|a| (a.name().local().to_owned(), a.value().to_owned())).collect()).unwrap_or_default();
    let rich = raw.elements().any(|e| e.name().local() == "is" && e.elements().any(|r| r.name().local() == "r"));
    Ok(Some(Sem {
        value: c.value()?,
        formula: c.formula(),
        f_attrs,
        t: raw.attr(QName::new("", "t")).map(str::to_owned),
        rich,
        s: c.style_index()?,
    }))
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
    /// Formula cells whose cache is stripped: dependents of the changed cells
    /// that are not themselves changed.
    pub strips: BTreeSet<CellKey>,
    pub formula_set_changes: bool,
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
}

impl ShadowState {
    pub fn new(wb: &mut Workbook) -> ooxml_xlsx::Result<ShadowState> {
        Ok(ShadowState { reg: RegState::at_origin(wb)?, date1904: wb.date1904()?, ..Default::default() })
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
    pub fn calc(&mut self, wb: &mut Workbook) -> ooxml_xlsx::Result<&Calc> {
        if self.calc.is_none() {
            let eff = self.effective();
            let names: Vec<String> = wb.sheet_names().map(str::to_owned).collect();
            let mut c = Calc::default();
            for &k in &eff {
                let snap = self.touched[&k].clone();
                let now = read_sem(wb, k)?;
                let (had_f, has_f) = (snap.as_ref().is_some_and(|s| s.formula.is_some()), now.as_ref().is_some_and(|s| s.formula.is_some()));
                c.formula_set_changes |= had_f != has_f;
                let content_changed = match (&snap, &now) {
                    (Some(a), Some(b)) => !a.same_content(b),
                    _ => true,
                };
                // a style-only change strips nothing here: the public API
                // does not say whether the workbook is style-sensitive
                if content_changed {
                    for (sh, at) in wb.dependents(&names[k.sheet], CellIndex { row: k.row, col: k.col })? {
                        let d = CellKey::new(names.iter().position(|n| *n == sh).expect("sheet"), at.row, at.col);
                        if !eff.contains(&d) {
                            c.strips.insert(d);
                        }
                    }
                }
            }
            self.calc = Some(c);
        }
        Ok(self.calc.as_ref().expect("computed"))
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

/// Applies one op on the shadow with remedies 1 and 2. Returns whether it
/// was applied (`false` = a no-op that only the log records).
pub fn apply_tracked(wb: &mut Workbook, st: &mut ShadowState, op: &Op) -> ooxml_xlsx::Result<bool> {
    let k = op.key();
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
    if !cfg!(feature = "mutant_no_noop") {
        let cell = read_cell(wb, k)?;
        let style_noop = matches!(op, Op::Style(_, Patch::Index(i)) if cell.as_ref().is_some_and(|c| c.s == *i));
        if style_noop || noop_op(&cell, op, st.date1904) {
            st.noops += 1;
            return Ok(false);
        }
    }
    let first = !st.touched.contains_key(&k);
    let snap = if first {
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
        Some(s)
    } else {
        None
    };
    apply_seq(wb, op)?;
    if let Some(s) = snap {
        st.touched.insert(k, s);
    }
    st.carried_pins.remove(&k);
    st.carried_strips.remove(&k);
    st.ever.insert(k);
    st.calc = None;
    let now = read_sem(wb, k)?;
    let back = match (&st.touched[&k], &now) {
        (Some(a), Some(b)) => a.same_state(b),
        (None, None) => true,
        _ => false,
    };
    if back && !cfg!(feature = "mutant_no_revert") {
        st.reverted.insert(k);
    } else {
        st.reverted.remove(&k);
    }
    Ok(true)
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
#[derive(Clone, Copy, Debug)]
enum Act {
    /// The Base's bytes: a restored cell, or one written in an earlier
    /// session and not since.
    Base,
    /// The Base's bytes with `s` set: the content is the Base's, only the
    /// style changed (the shadow's node may have lost the cache or carry
    /// `s="0"`; the Base's node has neither problem).
    BaseStyle(u32),
    /// The shadow's bytes in canonical form, registry indices renumbered.
    /// `in_base`: `xf` 0 is written as the Base cell wrote it (with or
    /// without `s`; a style reset on the shadow leaves `s="0"`); a created
    /// cell keeps the shadow's form.
    Shadow { xf: u32, sst: Option<u32>, in_base: bool },
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
    let needs_base = acts.values().any(|a| !matches!(a, Act::Shadow { in_base: false, .. }));
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
            Some(Act::BaseStyle(x)) => {
                let b = base_of();
                let oe = b.find('>').expect("tag") + 1;
                format!("{}{}", set_s(&b[..oe], *x, Some(true)), &b[oe..])
            }
            Some(Act::Shadow { xf, sst, in_base }) => {
                let oe = c.open_end - c.start;
                let zero = in_base.then(|| { let b = base_of(); attr(&b[..b.find('>').expect("tag")], "s").is_some() });
                let mut tag = set_s(&src[c.start..c.open_end], *xf, zero);
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
    let eff: Vec<CellKey> = st.effective().into_iter().collect();
    let mut reg = st.reg.clone();

    // ---- registries (remedy 4): references of the cells this shadow wrote
    let mut xf_refs: Vec<u32> = Vec::new();
    let mut sst_refs: Vec<u32> = Vec::new();
    let mut cell_refs: Vec<(CellKey, u32, Option<u32>)> = Vec::new();
    for &k in &st.ever {
        if st.is_pinned(k) {
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
    let delta: i64 = eff
        .iter()
        .map(|k| {
            let was = st.touched[k].as_ref().is_some_and(|s| s.t.as_deref() == Some("s"));
            let is = cell_refs.iter().any(|(c, _, i)| c == k && i.is_some());
            i64::from(is) - i64::from(was)
        })
        .sum();
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
        let dirty = matches!(wb.package().state(p), Some(PartStateKind::Parsed { dirty: true }));
        let strips_here: HashSet<String> = strips.iter().filter(|k| k.sheet == i).map(|k| a1(*k)).collect();
        if !dirty && strips_here.is_empty() {
            continue;
        }
        let base_t = part_text(&out, p)?.ok_or_else(|| malformed(p))?;
        let src = if dirty { part_text(wb.package(), p)?.expect("part") } else { base_t.clone() };
        let mut acts: HashMap<String, Act> = HashMap::new();
        for k in st.reverted.iter().chain(st.carried_pins.keys()).filter(|k| k.sheet == i) {
            acts.insert(a1(*k), Act::Base);
        }
        for (k, s, sst) in cell_refs.iter().filter(|c| c.0.sheet == i) {
            let xf = if *s >= reg.origin[0] { reg.maps[0][s] } else { *s };
            let act = match st.touched.get(k) {
                // written in an earlier session and not since
                None => Act::Base,
                Some(Some(snap)) if read_sem(wb, *k)?.is_some_and(|now| now.same_content(snap)) => Act::BaseStyle(xf),
                Some(snap) => Act::Shadow { xf, sst: sst.map(|v| if v >= reg.origin_sst { reg.sst_map[&v] } else { v }), in_base: snap.is_some() },
            };
            acts.insert(a1(*k), act);
        }
        let text = sheet_text(&src, &base_t, i, &acts, &strips_here, &eff);
        if text != base_t {
            parts_rewritten += 1;
            out.set_part(p, text)?;
        }
    }
    let t_sheets = t2.elapsed();

    // ---- calculation state (remedy 3)
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
    if calc.formula_set_changes
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
