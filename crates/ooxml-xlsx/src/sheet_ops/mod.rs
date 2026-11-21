//! Phase3 sheet operations on opened workbooks (ADR-0009): add, move,
//! hide/unhide, tab colour, rename, delete.
//!
//! Every operation is **one** `ooxml-core` registry transaction
//! ([`RegistryPlan`]): each part it changes is a staged clone carrying all of
//! that part's changes (sheet list, names, views, Phase1 cache stripping,
//! `calcPr`, rewritten formulas), together with the part additions and the
//! closed-set removal. Nothing in the package changes until the plan is
//! applied; a refusal leaves it byte-identical. After apply, every index
//! (census, sheet list, names) is rebuilt from the package; a failed rebuild
//! poisons the workbook (ADR-0009 §2).

mod app_xml;
mod names;

mod scan;

/// The one sheet-prefix quoting rule (ADR-0009 §6 "Quoting"), shared by
/// every writer of `Sheet!` prefixes (issue #112).
pub(crate) use names::quote_sheet;

use std::collections::{BTreeMap, HashSet};

use ooxml_core::opc::TargetMode;
use ooxml_core::{ElementSpec, LensMut, Package, PartNameRequest, RegistryPlan, Target, XmlDocument};
use ooxml_schema::sml::{self, SheetStateValues};

use crate::census::{FKind, SheetKind};
use crate::edit::strip_cells;
use crate::error::{Error, Result};
use crate::formula::lexer::{Prefix, TokKind, lex};
use crate::formula::precedents::{Rect, fold};
use crate::reference::{MAX_COLS, MAX_ROWS};
use crate::workbook::{CalcPolicy, REL_CALC_CHAIN, REL_WORKSHEET, Workbook};
use app_xml::Listed;
use names::{PrefixEdit, Unrewritable, rewrite_formula, rewrite_location, validate_sheet_name};
use crate::locations::{self, PartKind, Rule, Syntax};
use scan::scan_part;

const CT_WORKSHEET: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml";
const REL_REVISIONS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/revisionHeaders";
const CT_CHART: &str = "application/vnd.openxmlformats-officedocument.drawingml.chart+xml";
const CT_ACTIVEX: &str = "application/vnd.ms-office.activeX";
const R_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

/// A new worksheet's content (the embedded template's sheet, not selected).
const NEW_SHEET: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><dimension ref="A1"/><sheetViews><sheetView workbookViewId="0"/></sheetViews><sheetFormatPr defaultRowHeight="{h}"/><sheetData/><pageMargins left="0.7" right="0.7" top="0.75" bottom="0.75" header="0.3" footer="0.3"/></worksheet>"#;

/// Relationship types (last URI segment) a deleted worksheet's parts may
/// have and that go with it (ADR-0009 §5 cascade table).
const CASCADE: &[&str] = &[
    "printerSettings",
    "comments",
    "vmlDrawing",
    "threadedComment",
    "table",
    "tableSingleCells",
    "customProperty",
    "image",
    "drawing",
    "chart",
    "chartStyle",
    "chartColorStyle",
    "chartUserShapes",
];

/// A sheet's visibility.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SheetState {
    /// Shown as a tab.
    Visible,
    /// Hidden; the user can unhide it.
    Hidden,
    /// Hidden; only code can unhide it.
    VeryHidden,
}

/// One place a rename or delete rewrote (returned for auditing).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RewrittenLocation {
    /// The part.
    pub part: String,
    /// Where in it (`B2 formula`, `definedName Foo`, `hyperlink A1`,
    /// `relationship rId3`).
    pub location: String,
    /// The text before.
    pub before: String,
    /// The text after.
    pub after: String,
}

/// The staged clones of one operation, one per part.
#[derive(Default)]
struct Change {
    docs: BTreeMap<String, XmlDocument>,
    /// Parts rewritten at the byte level (charts: only the `c:f` text
    /// changes, every other byte is kept).
    bytes: BTreeMap<String, Vec<u8>>,
    rewritten: Vec<RewrittenLocation>,
    removed_shared_refs: i64,
}

impl Change {
    fn doc(&mut self, pkg: &mut Package, part: &str) -> Result<&mut XmlDocument> {
        if !self.docs.contains_key(part) {
            let d = pkg.xml_part(part)?.clone();
            self.docs.insert(part.to_owned(), d);
        }
        Ok(self.docs.get_mut(part).expect("inserted"))
    }
}

/// OPC part-name equivalence (ASCII case-insensitive, percent-decoded).
fn same_name(a: &str, b: &str) -> bool {
    fn norm(s: &str) -> Vec<u8> {
        let b = s.trim_start_matches('/').as_bytes();
        let mut out = Vec::with_capacity(b.len());
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'%'
                && let Some(v) = b.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u8::from_str_radix(h, 16).ok())
            {
                out.push(v.to_ascii_lowercase());
                i += 3;
                continue;
            }
            out.push(b[i].to_ascii_lowercase());
            i += 1;
        }
        out
    }
    norm(a) == norm(b)
}

fn malformed(what: &str) -> Error {
    Error::Malformed(what.into())
}

fn last_segment(rel_type: &str) -> &str {
    rel_type.rsplit('/').next().unwrap_or(rel_type)
}

/// Child indices of the `sheet` elements in `<sheets>`.
fn sheet_children(doc: &XmlDocument) -> Result<Vec<usize>> {
    let sheets = doc.root().elements().find(|e| e.is(sml::Sheets::QNAME)).ok_or_else(|| malformed("no <sheets>"))?;
    Ok(sheets.indexed_elements().filter(|(_, e)| e.is(sml::Sheet::QNAME)).map(|(i, _)| i).collect())
}

fn sheets_lens(doc: &mut XmlDocument) -> Result<LensMut<'_, sml::Sheets>> {
    let wb = doc.lens_mut::<sml::Workbook>()?;
    wb.into_child_by::<sml::Sheets>(|_| true).ok_or_else(|| malformed("no <sheets>"))
}

fn names_lens(doc: &mut XmlDocument) -> Result<Option<LensMut<'_, sml::DefinedNames>>> {
    let wb = doc.lens_mut::<sml::Workbook>()?;
    Ok(wb.into_child_by::<sml::DefinedNames>(|_| true))
}

/// `(child index, name, localSheetId, text)` of every `definedName`.
fn defined_names(doc: &XmlDocument) -> Vec<(usize, String, Option<u32>, String)> {
    let Some(dn) = doc.root().elements().find(|e| e.is(sml::DefinedNames::QNAME)) else { return Vec::new() };
    dn.indexed_elements()
        .filter(|(_, e)| e.is(sml::DefinedName::QNAME))
        .map(|(i, e)| {
            let local = e.attr(ooxml_core::QName::new("", "localSheetId")).and_then(|v| v.trim().parse().ok());
            (i, e.attr(ooxml_core::QName::new("", "name")).unwrap_or_default().to_owned(), local, e.text().into_owned())
        })
        .collect()
}

/// Remaps `definedName/@localSheetId` (positions); `None` removes the name.
fn remap_local_names(doc: &mut XmlDocument, map: &dyn Fn(u32) -> Option<u32>) -> Result<()> {
    let names = defined_names(doc);
    let Some(mut dn) = names_lens(doc)? else { return Ok(()) };
    let mut removes = Vec::new();
    for (i, _, local, _) in &names {
        let Some(old) = local else { continue };
        match map(*old) {
            None => removes.push(*i),
            Some(new) if new != *old => {
                let n = dn.nth_child_mut_at(*i).ok_or_else(|| malformed("definedName"))?;
                sml::DefinedNameViewMut(n).set_local_sheet_id(new)?;
            }
            Some(_) => {}
        }
    }
    for i in removes.into_iter().rev() {
        dn.remove_at(i);
    }
    Ok(())
}

/// Lens on the child element at item index `i` (any type the parent allows).
trait NthAt<'a, E: ElementSpec> {
    fn nth_child_mut_at<C: ooxml_core::ChildOf<E>>(&mut self, i: usize) -> Option<LensMut<'_, C>>;
}

impl<'a, E: ElementSpec> NthAt<'a, E> for LensMut<'a, E> {
    fn nth_child_mut_at<C: ooxml_core::ChildOf<E>>(&mut self, i: usize) -> Option<LensMut<'_, C>> {
        let k = self.node().children()[..i].iter().filter(|c| c.as_element().is_some_and(|e| e.is(C::QNAME))).count();
        self.nth_child_mut::<C>(k)
    }
}

impl Workbook {
    /// Refusals common to every structural operation.
    fn structural_guard(&mut self) -> Result<()> {
        self.check_editable()?;
        let rels = self.pkg.relationships(&self.workbook_part)?;
        if rels.by_type(REL_REVISIONS).next().is_some() {
            return Err(Error::SharedWorkbook);
        }
        Ok(())
    }

    fn sheet_structure_guard(&mut self, operation: &'static str) -> Result<()> {
        self.structural_guard()?;
        let workbook = self.pkg.xml_part(&self.workbook_part)?;
        if workbook.root().elements().any(|node| {
            node.is(sml::WorkbookProtection::QNAME)
                && node.attr(ooxml_core::QName::new("", "lockStructure")).is_some_and(|value| matches!(value.trim(), "1" | "true"))
        }) {
            return Err(Error::Protected { scope: "workbook", operation });
        }
        Ok(())
    }

    fn sheet_pos(&self, name: &str) -> Result<usize> {
        if let Some(i) = self.sheets.iter().position(|s| s.name == name) {
            return Ok(i);
        }
        let f = fold(name);
        let mut it = self.sheets.iter().enumerate().filter(|(_, s)| fold(&s.name) == f).map(|(i, _)| i);
        match (it.next(), it.next()) {
            (Some(i), None) => Ok(i),
            _ => Err(Error::NoSuchSheet(name.to_owned())),
        }
    }

    fn check_new_name(&self, name: &str, except: Option<usize>) -> Result<()> {
        validate_sheet_name(name)?;
        let f = fold(name);
        if self.sheets.iter().enumerate().any(|(i, s)| Some(i) != except && fold(&s.name) == f) {
            return Err(Error::DuplicateSheetName(name.to_owned()));
        }
        if self.env.is_name(name) {
            return Err(Error::InvalidSheetName { name: name.to_owned(), reason: "equals a defined name" });
        }
        Ok(())
    }

    /// Formula ids whose text (or whose shared master's text) satisfies
    /// `pred`, members of matching shared groups included. A defined name
    /// whose text satisfies it seeds every formula (any could use it).
    fn formulas_where(&self, pred: &dyn Fn(&str) -> bool) -> Vec<usize> {
        if self.env_texts().iter().any(|t| pred(t)) {
            return self.census.alive().map(|(i, _)| i).collect();
        }
        let mut groups = HashSet::new();
        let mut out = Vec::new();
        for (i, c) in self.census.alive() {
            if !c.text.is_empty() && pred(&c.text) {
                out.push(i);
                if let FKind::Shared { si, .. } = c.kind {
                    groups.insert((c.sheet, si));
                }
            }
        }
        for (i, c) in self.census.alive() {
            if let FKind::Shared { si, master: false } = c.kind
                && groups.contains(&(c.sheet, si))
            {
                out.push(i);
            }
        }
        out
    }

    fn env_texts(&self) -> Vec<String> {
        self.pkg
            .parsed_part(&self.workbook_part)
            .map(|d| defined_names(d).into_iter().map(|(_, _, _, t)| t).collect())
            .unwrap_or_default()
    }

    /// ADR-0003 invalidation for a structural edit: dependents of the seeds
    /// (and every `Everything` formula) lose their cached values in the
    /// staged clones, and `calcPr/@fullCalcOnLoad` is set; ADR-0003 §5
    /// guards apply (ADR-0009 §12). Sheets in `gone` are skipped.
    fn invalidate(&mut self, change: &mut Change, fids: &[usize], rects: Vec<Rect>, gone: &[usize]) -> Result<()> {
        let deps = self.dependents_of_seeds(fids, rects);
        let live: Vec<usize> = deps.formulas.iter().copied().filter(|f| self.census.get(*f).is_some_and(|c| !gone.contains(&c.sheet))).collect();
        for &fid in &live {
            let c = self.census.get(fid).expect("alive");
            if matches!(self.sheets[c.sheet].kind, SheetKind::MacroSheet | SheetKind::DialogSheet) {
                return Err(Error::MacroSheetDependent { sheet: self.sheets[c.sheet].name.clone(), cell: c.at.to_string() });
            }
        }
        if let Some(&(_, c)) = deps.cells.iter().filter(|(s, _)| !gone.contains(s)).find(|k| self.census.vm_cells.contains(k)) {
            return Err(Error::RichValueCell { cell: c.to_string() });
        }
        if live.is_empty() {
            return Ok(());
        }
        if self.calc.iterate {
            return Err(Error::IterativeCalculation);
        }
        if self.policy == CalcPolicy::StripDependentCaches {
            let strip = self.plan_strip(&deps, &HashSet::new())?;
            for (part, positions) in &strip.sheets {
                if gone.iter().any(|&g| self.sheets[g].part.as_deref() == Some(part.as_str())) {
                    continue;
                }
                change.removed_shared_refs += strip_cells(change.doc(&mut self.pkg, part)?, positions)?;
            }
        }
        if !self.calc.manual && !self.calc.full_calc_on_load {
            let wb_part = self.workbook_part.clone();
            let doc = change.doc(&mut self.pkg, &wb_part)?;
            let mut wb = doc.lens_mut::<sml::Workbook>()?;
            let mut calc = wb.get_or_insert::<sml::CalculationProperties>()?;
            calc.set_attr(sml::CalculationProperties::ATTR_FULL_CALC_ON_LOAD, "1")?;
        }
        Ok(())
    }

    /// Updates the optional count in the same plan as the sheet changes.
    fn stage_shared_string_count(&mut self, change: &mut Change) -> Result<()> {
        if change.removed_shared_refs == 0 {
            return Ok(());
        }
        let rels = self.pkg.relationships(&self.workbook_part)?;
        let Some(part) = rels.by_type(crate::workbook::REL_SHARED_STRINGS).next().and_then(|r| rels.resolve(r)) else {
            return Ok(());
        };
        let count = ooxml_core::QName::new("", "count");
        if !self.pkg.contains(&part) || self.pkg.xml_part(&part)?.root().attr(count).is_none() {
            return Ok(());
        }
        let mut refs = 0;
        for p in crate::registry::census_sheet_parts(self) {
            refs += crate::registry::scan_sheet(&self.pkg.read_part(&p)?)?.shared as i64;
        }
        let remaining = refs - change.removed_shared_refs;
        change.doc(&mut self.pkg, &part)?.lens_mut::<sml::SharedStringTable>()?
            .set_attr(count, &remaining.to_string())?;
        Ok(())
    }

    /// Stages the change's clones and runs `steps`, applies, rebuilds.
    fn commit(&mut self, mut change: Change, scan_extensions: bool, steps: impl FnOnce(&mut RegistryPlan, &Package) -> Result<()>) -> Result<Vec<RewrittenLocation>> {
        let mut plan = RegistryPlan::new(&self.pkg);
        // staged content first: the removals' reference scans read it
        self.stage_shared_string_count(&mut change)?;
        for (part, doc) in change.docs.iter().filter(|_| scan_extensions) {
            let ct = self.pkg.content_type(part).unwrap_or_default();
            let kind = if ct.eq_ignore_ascii_case(CT_WORKSHEET) {
                PartKind::Worksheet
            } else if *part == self.workbook_part {
                PartKind::Workbook
            } else {
                locations::additional_part(&ct.to_ascii_lowercase())
            };
            if matches!(kind, PartKind::Worksheet | PartKind::Workbook | PartKind::Table)
                && doc.source().contains("extLst")
            {
                scan_part(part, doc.serialize().as_bytes(), kind, "", None)?;
            }
        }
        let Change { docs, bytes, rewritten, .. } = change;
        for (part, doc) in docs {
            plan.stage_part(&self.pkg, &part, doc)?;
        }
        for (part, b) in bytes {
            plan.stage_part(&self.pkg, &part, b)?;
        }
        steps(&mut plan, &self.pkg)?;
        self.finish(plan, Change { rewritten, ..Change::default() })
    }

    /// Stages the change's clones into `plan`, then applies and rebuilds
    /// under poisoning (ADR-0009 §2 "Poisoning"): the workbook is poisoned
    /// before apply and the poison is cleared only after the rebuild
    /// succeeded, so a failure or a panic in between leaves it refusing
    /// every edit and save.
    fn finish(&mut self, mut plan: RegistryPlan, change: Change) -> Result<Vec<RewrittenLocation>> {
        for (part, doc) in change.docs {
            plan.stage_part(&self.pkg, &part, doc)?;
        }
        for (part, b) in change.bytes {
            plan.stage_part(&self.pkg, &part, b)?;
        }
        self.poisoned = Some("a structural edit was interrupted".into());
        let r = (|| -> Result<()> {
            self.fault_point(crate::workbook::ApplyStep::StructuralApply)?;
            self.pkg.apply(plan)?;
            self.fault_point(crate::workbook::ApplyStep::StructuralRebuild)?;
            self.rebuild()
        })();
        match r {
            Ok(()) => {
                self.poisoned = None;
                Ok(change.rewritten)
            }
            Err(e) => {
                let msg = match &e {
                    Error::Poisoned(m) => m.clone(),
                    e => e.to_string(),
                };
                self.poisoned = Some(msg.clone());
                Err(Error::Poisoned(msg))
            }
        }
    }

    /// `activeTab`/`firstSheet` of every `workbookView`, remapped; an active
    /// tab that is not visible moves to the next visible sheet.
    /// `firstSheet` (the first tab shown in the tab strip, not a sheet
    /// reference) is mapped with `first_map`, kept at most `activeTab`, and
    /// never written when absent (review of PR #53 m5).
    fn remap_views(doc: &mut XmlDocument, map: &dyn Fn(u32) -> u32, visible: &[bool]) -> Result<Option<u32>> {
        Self::remap_views_with(doc, map, map, visible)
    }

    fn remap_views_with(doc: &mut XmlDocument, map: &dyn Fn(u32) -> u32, first_map: &dyn Fn(u32) -> u32, visible: &[bool]) -> Result<Option<u32>> {
        let wb = doc.lens_mut::<sml::Workbook>()?;
        let Some(mut bv) = wb.into_child_by::<sml::BookViews>(|_| true) else { return Ok(None) };
        let n = bv.node().elements().filter(|e| e.is(sml::WorkbookViewElement::QNAME)).count();
        let mut first_active = None;
        for k in 0..n {
            let mut v = sml::WorkbookViewElementViewMut(bv.nth_child_mut::<sml::WorkbookViewElement>(k).expect("counted"));
            let view = v.as_view();
            let (active, first) = (view.active_tab().unwrap_or(0), view.first_sheet().unwrap_or(0));
            let has_first = view.0.raw().attr(ooxml_core::QName::new("", "firstSheet")).is_some();
            let mut na = map(active).min(visible.len().saturating_sub(1) as u32);
            if !visible.get(na as usize).copied().unwrap_or(false) {
                let fwd = (na as usize..visible.len()).find(|&i| visible[i]);
                let back = (0..na as usize).rev().find(|&i| visible[i]);
                na = fwd.or(back).unwrap_or(0) as u32;
            }
            let nf = first_map(first).min(na);
            if na != active {
                v.set_active_tab(na)?;
            }
            if has_first && nf != first {
                v.set_first_sheet(nf)?;
            }
            first_active.get_or_insert(na);
        }
        Ok(first_active)
    }

    /// `tabSelected` only on the active sheet (ADR-0009 review m11).
    fn select_tab(&mut self, change: &mut Change, active: Option<usize>, skip: &[usize]) -> Result<()> {
        for (i, s) in self.sheets.clone().iter().enumerate() {
            if s.kind != SheetKind::Worksheet || skip.contains(&i) {
                continue;
            }
            let Some(part) = &s.part else { continue };
            let want = Some(i) == active;
            let has = {
                let d = change.docs.get(part).map_or_else(|| self.pkg.xml_part(part).map(|d| d.root().clone()), |d| Ok(d.root().clone()))?;
                d.elements()
                    .find(|e| e.is(sml::SheetViews::QNAME))
                    .is_some_and(|v| v.elements().any(|e| matches!(e.attr(ooxml_core::QName::new("", "tabSelected")).map(str::trim), Some("1" | "true"))))
            };
            if has == want {
                continue;
            }
            let doc = change.doc(&mut self.pkg, part)?;
            let ws = doc.lens_mut::<sml::Worksheet>()?;
            let Some(mut views) = ws.into_child_by::<sml::SheetViews>(|_| true) else { continue };
            let n = views.node().elements().filter(|e| e.is(sml::SheetViewElement::QNAME)).count();
            for k in 0..n {
                let mut v = sml::SheetViewElementViewMut(views.nth_child_mut::<sml::SheetViewElement>(k).expect("counted"));
                if want && k == 0 {
                    v.set_tab_selected(true)?;
                } else if !want {
                    v.remove_tab_selected()?;
                }
            }
        }
        Ok(())
    }

    /// Formulas with a 3-D reference or a `SHEET`/`SHEETS` call (ADR-0009
    /// §4, §7 seeds; a superset of "whose span changes membership").
    fn three_d_and_sheet_callers(&self) -> Vec<usize> {
        self.formulas_where(&|t| {
            lex(t).is_ok_and(|toks| {
                toks.iter().any(|k| match &k.kind {
                    TokKind::Ref(r) => matches!(r.prefix, Prefix::Sheets(..)),
                    TokKind::Name { prefix: Prefix::Sheets(..), .. } => true,
                    TokKind::Func(f) => matches!(f.as_str(), "SHEET" | "SHEETS" | "_XLFN.SHEET" | "_XLFN.SHEETS"),
                    _ => false,
                })
            })
        })
    }

    /// The sheets as `docProps/app.xml` lists them (kind, `veryHidden`).
    fn listed(&mut self) -> Result<Vec<Listed>> {
        let doc = self.pkg.xml_part(&self.workbook_part)?;
        let kids = sheet_children(doc)?;
        let sheets = doc.root().elements().find(|e| e.is(sml::Sheets::QNAME)).expect("checked");
        Ok(kids
            .iter()
            .zip(&self.sheets)
            .map(|(&k, s)| {
                let e = sheets.children()[k].as_element().expect("element");
                let vh = e.attr(ooxml_core::QName::new("", "state")).is_some_and(|v| v.trim() == "veryHidden");
                Listed { name: s.name.clone(), kind: s.kind, very_hidden: vh }
            })
            .collect())
    }

    /// Stages the `docProps/app.xml` update (ADR-0009 §11) when its shape is
    /// recognized and no structure would have to be invented; otherwise
    /// app.xml stays as it is.
    fn stage_app_xml(&mut self, change: &mut Change, before: &[Listed], after: &[Listed], edit: Option<&PrefixEdit<'_>>, deleted: Option<&str>) -> Result<()> {
        let rels = self.pkg.package_relationships().clone();
        let Some(part) = rels.by_type(app_xml::REL_EXTENDED).next().and_then(|r| rels.resolve(r)).filter(|p| self.pkg.contains(p)) else {
            return Ok(());
        };
        let Ok(doc) = self.pkg.xml_part(&part) else { return Ok(()) };
        if let Some(new) = app_xml::update(doc, before, after, edit, deleted)? {
            change.docs.insert(part, new);
        }
        Ok(())
    }

    // ------------------------------------------------------------ add

    /// Adds an empty worksheet named `name` at `position` (0-based in
    /// workbook order; `position` = sheet count appends). ADR-0009 §4:
    /// `sheetId` = 1 + the largest id in use (or removed in this session),
    /// part `max + 1`, `localSheetId`/`activeTab`/`firstSheet` renumbered,
    /// formulas with 3-D references, `SHEET`/`SHEETS` and every
    /// `Everything` formula lose their cached values.
    ///
    /// `docProps/app.xml` is kept consistent when its shape is recognized
    /// (ADR-0009 §11); otherwise it is left byte-identical.
    pub fn add_sheet(&mut self, name: &str, position: usize) -> Result<()> {
        self.sheet_structure_guard("add_sheet")?;
        self.check_new_name(name, None)?;
        if position > self.sheets.len() {
            return Err(Error::NoSuchSheet(format!("#{position}")));
        }
        let wb_part = self.workbook_part.clone();
        let mut change = Change::default();
        let seeds = self.three_d_and_sheet_callers();
        self.invalidate(&mut change, &seeds, Vec::new(), &[])?;
        // ids
        let sheet_id = self.next_sheet_id()?;
        let dir = self.worksheet_dir();
        let row_height = self.default_row_height()?;
        let content = NEW_SHEET.replace("{h}", &row_height);
        let pos32 = position as u32;
        {
            let doc = change.doc(&mut self.pkg, &wb_part)?;
            remap_local_names(doc, &|i| Some(if i >= pos32 { i + 1 } else { i }))?;
            let mut visible: Vec<bool> = self.sheets.iter().map(|s| s.visible).collect();
            visible.insert(position, true);
            Self::remap_views(doc, &|i| if i >= pos32 { i + 1 } else { i }, &visible)?;
        }
        // the plan: part, relationship, then the <sheet> with its r:id
        let mut plan = RegistryPlan::new(&self.pkg);
        let part = plan.add_part(&self.pkg, PartNameRequest::Numbered(format!("{dir}/sheet{{}}.xml")), CT_WORKSHEET, XmlDocument::parse(content.as_bytes())?)?;
        let rid = plan.add_relationship(&self.pkg, Some(&wb_part), REL_WORKSHEET, Target::Part(part.clone()))?;
        {
            let doc = change.doc(&mut self.pkg, &wb_part)?;
            let mut sheets = sheets_lens(doc)?;
            let mut seen = 0usize;
            // `r:id` needs the relationships namespace in scope: declared on
            // the new element when the workbook does not bind it (PR #45)
            let decl = [ooxml_core::NsDecl { prefix: "r", uri: R_NS }];
            let mut s = sml::SheetViewMut(sheets.insert_by_with_ns::<sml::Sheet>(
                |_, n| {
                    let hit = n.is(sml::Sheet::QNAME) && seen == position;
                    if n.is(sml::Sheet::QNAME) {
                        seen += 1;
                    }
                    hit
                },
                &decl,
            )?);
            s.set_name(name)?;
            s.set_sheet_id(sheet_id)?;
            s.0.set_attr(ooxml_core::QName::new(R_NS, "id"), &rid)?;
        }
        let before = self.listed()?;
        let mut after = before.clone();
        after.insert(position, Listed { name: name.to_owned(), kind: SheetKind::Worksheet, very_hidden: false });
        self.stage_app_xml(&mut change, &before, &after, None, None)?;
        self.finish(plan, change).map(drop)
    }

    fn next_sheet_id(&mut self) -> Result<u32> {
        let doc = self.pkg.xml_part(&self.workbook_part)?;
        let mut max = 0u32;
        let wb = doc.root();
        if let Some(sheets) = wb.elements().find(|e| e.is(sml::Sheets::QNAME)) {
            for s in sheets.elements() {
                if let Some(v) = s.attr(ooxml_core::QName::new("", "sheetId")).and_then(|v| v.trim().parse::<u32>().ok()) {
                    max = max.max(v);
                }
            }
        }
        if let Some(cv) = wb.elements().find(|e| e.is(sml::CustomWorkbookViews::QNAME)) {
            for v in cv.elements() {
                if let Some(id) = v.attr(ooxml_core::QName::new("", "activeSheetId")).and_then(|v| v.trim().parse::<u32>().ok()) {
                    max = max.max(id);
                }
            }
        }
        if let Some(cc) = self.calc_chain.clone() {
            let bytes = self.pkg.read_part(&cc)?;
            if let Ok(d) = XmlDocument::parse(&bytes) {
                for c in d.root().elements() {
                    if let Some(i) = c.attr(ooxml_core::QName::new("", "i")).and_then(|v| v.trim().parse::<u32>().ok()) {
                        max = max.max(i);
                    }
                }
            }
        }
        max = self.removed_sheet_ids.iter().copied().fold(max, u32::max);
        max.checked_add(1).ok_or(Error::Malformed("sheetId exhausted".into()))
    }

    /// The folder of the existing worksheets if they share one.
    fn worksheet_dir(&self) -> String {
        let dirs: HashSet<&str> = self
            .sheets
            .iter()
            .filter(|s| s.kind == SheetKind::Worksheet)
            .filter_map(|s| s.part.as_deref())
            .map(|p| p.rsplit_once('/').map_or("", |(d, _)| d))
            .collect();
        match dirs.into_iter().collect::<Vec<_>>()[..] {
            [d] if !d.is_empty() => d.to_owned(),
            _ => "xl/worksheets".to_owned(),
        }
    }

    fn default_row_height(&mut self) -> Result<String> {
        let part = self.sheets[self.first_visible_index()].part.clone().expect("worksheet");
        let doc = self.pkg.xml_part(&part)?;
        let h = doc
            .root()
            .elements()
            .find(|e| e.is(ooxml_core::QName::new(crate::SML_NS, "sheetFormatPr")))
            .and_then(|e| e.attr(ooxml_core::QName::new("", "defaultRowHeight")))
            .and_then(|v| v.trim().parse::<f64>().ok())
            .filter(|h| h.is_finite() && *h > 0.0 && *h < 410.0)
            .unwrap_or(15.0);
        Ok(format!("{h}"))
    }

    fn first_visible_index(&self) -> usize {
        self.sheets.iter().position(|s| s.visible && s.kind == SheetKind::Worksheet).unwrap_or(0)
    }

    // ------------------------------------------------------------ move

    /// Moves the sheet at `from` to position `to` (ADR-0009 §7). The
    /// `<sheet>` element keeps its bytes; positions in `localSheetId`,
    /// `activeTab` and `firstSheet` follow. A move that changes the order of
    /// a 3-D reference's endpoints is refused
    /// ([`Error::ReversesThreeDReference`]). `firstSheet` (the first tab
    /// shown in the tab strip) stays, clamped to the active tab.
    ///
    /// `docProps/app.xml` is kept consistent when its shape is recognized
    /// (ADR-0009 §11); otherwise it is left byte-identical.
    pub fn move_sheet(&mut self, from: usize, to: usize) -> Result<()> {
        self.sheet_structure_guard("move_sheet")?;
        let n = self.sheets.len();
        if from >= n || to >= n {
            return Err(Error::NoSuchSheet(format!("#{}", from.max(to))));
        }
        if from == to {
            return Ok(());
        }
        let mut perm: Vec<usize> = (0..n).collect();
        let s = perm.remove(from);
        perm.insert(to, s);
        // new position of each old position
        let mut newpos = vec![0u32; n];
        for (p, &old) in perm.iter().enumerate() {
            newpos[old] = p as u32;
        }
        self.check_three_d_order(&newpos)?;
        let mut change = Change::default();
        let seeds = self.three_d_and_sheet_callers();
        self.invalidate(&mut change, &seeds, Vec::new(), &[])?;
        let wb_part = self.workbook_part.clone();
        let visible: Vec<bool> = perm.iter().map(|&o| self.sheets[o].visible).collect();
        {
            let doc = change.doc(&mut self.pkg, &wb_part)?;
            let kids = sheet_children(doc)?;
            let sheets_ix = doc.root().indexed_elements().find(|(_, e)| e.is(sml::Sheets::QNAME)).map(|(i, _)| i).expect("checked");
            let src = ooxml_core::Lens::<sml::Sheet>::at(doc.root(), &[sheets_ix, kids[from]])?.move_source();
            {
                // core's checked same-part move (#44): slot and order checks,
                // the element keeps its bytes
                let mut sheets = sheets_lens(doc)?;
                sheets.move_in_by::<sml::Sheet>(&src, |k, _| k == to)?;
            }
            remap_local_names(doc, &|i| Some(newpos.get(i as usize).copied().unwrap_or(i)))?;
            // the tab strip's first tab is a position, not a sheet: kept
            // (clamped to the active tab), as POI's setSheetOrder does
            Self::remap_views_with(doc, &|i| newpos.get(i as usize).copied().unwrap_or(i), &|f| f, &visible)?;
        }
        let before = self.listed()?;
        let after: Vec<Listed> = perm.iter().map(|&o| before[o].clone()).collect();
        self.stage_app_xml(&mut change, &before, &after, None, None)?;
        self.commit(change, false, |_, _| Ok(())).map(drop)
    }

    /// Refuses a permutation that reverses any 3-D reference.
    fn check_three_d_order(&self, newpos: &[u32]) -> Result<()> {
        let pos = |name: &str| {
            let f = fold(name);
            let mut it = self.sheets.iter().enumerate().filter(|(_, s)| fold(&s.name) == f);
            match (it.next(), it.next()) {
                (Some((i, _)), None) => Some(i),
                _ => None,
            }
        };
        let texts: Vec<String> = self.census.alive().map(|(_, c)| c.text.clone()).chain(self.env_texts()).collect();
        for t in &texts {
            let Ok(toks) = lex(t) else { continue };
            for k in toks {
                let p = match &k.kind {
                    TokKind::Ref(r) => &r.prefix,
                    TokKind::Name { prefix, .. } => prefix,
                    _ => continue,
                };
                if let Prefix::Sheets(a, z) = p
                    && let (Some(pa), Some(pz)) = (pos(a), pos(z))
                    && pa != pz
                    && (pa < pz) != (newpos[pa] < newpos[pz])
                {
                    return Err(Error::ReversesThreeDReference(t[k.start..k.end].to_owned()));
                }
            }
        }
        Ok(())
    }

    // ------------------------------------------------------------ hide, tab colour

    /// Sets a sheet's visibility (ADR-0009 §8). Hiding the last visible
    /// sheet is [`Error::LastVisibleSheet`]; hiding the active sheet moves
    /// the active tab to the next visible sheet; a hidden sheet is never
    /// `tabSelected`.
    ///
    /// `docProps/app.xml` is kept consistent when its shape is recognized
    /// (ADR-0009 §11); otherwise it is left byte-identical.
    pub fn set_sheet_state(&mut self, sheet: &str, state: SheetState) -> Result<()> {
        self.structural_guard()?;
        let i = self.sheet_pos(sheet)?;
        let mut visible: Vec<bool> = self.sheets.iter().map(|s| s.visible).collect();
        visible[i] = state == SheetState::Visible;
        if !visible.iter().zip(&self.sheets).any(|(v, s)| *v && s.kind == SheetKind::Worksheet) {
            return Err(Error::LastVisibleSheet);
        }
        let wb_part = self.workbook_part.clone();
        let mut change = Change::default();
        let active = {
            let doc = change.doc(&mut self.pkg, &wb_part)?;
            let kids = sheet_children(doc)?;
            {
                let mut sheets = sheets_lens(doc)?;
                let mut s = sml::SheetViewMut(sheets.nth_child_mut_at::<sml::Sheet>(kids[i]).ok_or_else(|| malformed("sheet"))?);
                match state {
                    SheetState::Visible => {
                        s.remove_state()?;
                    }
                    SheetState::Hidden => s.set_state(SheetStateValues::Hidden)?,
                    SheetState::VeryHidden => s.set_state(SheetStateValues::VeryHidden)?,
                }
            }
            Self::remap_views(doc, &|x| x, &visible)?
        };
        if state != SheetState::Visible {
            self.select_tab(&mut change, active.map(|a| a as usize), &[])?;
        }
        let before = self.listed()?;
        let mut after = before.clone();
        after[i].very_hidden = state == SheetState::VeryHidden;
        self.stage_app_xml(&mut change, &before, &after, None, None)?;
        self.commit(change, false, |_, _| Ok(())).map(drop)
    }

    /// Sets (`Some(0xRRGGBB)`) or removes (`None`) a worksheet's tab colour
    /// (`sheetPr/tabColor/@rgb`).
    pub fn set_tab_color(&mut self, sheet: &str, rgb: Option<u32>) -> Result<()> {
        self.structural_guard()?;
        let i = self.sheet_pos(sheet)?;
        if self.sheets[i].kind != SheetKind::Worksheet {
            return Err(Error::NotAWorksheet(self.sheets[i].name.clone()));
        }
        let part = self.sheets[i].part.clone().expect("worksheet");
        let mut change = Change::default();
        {
            let doc = change.doc(&mut self.pkg, &part)?;
            let mut ws = doc.lens_mut::<sml::Worksheet>()?;
            match rgb {
                Some(c) => {
                    let pr = ws.get_or_insert::<sml::SheetProperties>()?;
                    let mut pr = sml::SheetPropertiesViewMut(pr);
                    pr.get_or_insert_tab_color()?.set_rgb(&format!("FF{:06X}", c & 0xFF_FFFF))?;
                }
                None => {
                    if let Some(pr) = ws.child_mut::<sml::SheetProperties>() {
                        sml::SheetPropertiesViewMut(pr).remove_tab_color();
                    }
                }
            }
        }
        self.commit(change, false, |_, _| Ok(())).map(drop)
    }

    // ------------------------------------------------------------ rename

    /// Renames a sheet and rewrites every reference to it that this version
    /// knows (ADR-0009 §6): cell formulas on every worksheet, defined names,
    /// chart `c:f`/`c15:f`, `hyperlink/@location`, `#Sheet!…` relationship
    /// targets and a pivot cache's `worksheetSource/@sheet`. Any other
    /// mention of the old name in sheet-prefix form (data validation,
    /// conditional formatting, x14 `xm:f`, table formulas, form controls,
    /// `[0]Sheet!` self-references, parts that do not parse, …) refuses the
    /// rename ([`Error::UnrewritableReference`]) instead of leaving a broken
    /// reference, as does a consolidation `rangeSet`/`dataRef` naming the
    /// sheet. Returns what was rewritten.
    ///
    /// `docProps/app.xml` is kept consistent when its shape is recognized
    /// (ADR-0009 §11); otherwise it is left byte-identical.
    pub fn rename_sheet(&mut self, sheet: &str, new_name: &str) -> Result<Vec<RewrittenLocation>> {
        self.sheet_structure_guard("rename_sheet")?;
        let i = self.sheet_pos(sheet)?;
        let old = self.sheets[i].name.clone();
        if old == new_name {
            return Ok(Vec::new());
        }
        self.check_new_name(new_name, Some(i))?;
        if self.pkg.part_names().any(|p| self.pkg.content_type(p).is_some_and(|ct| ct.starts_with(CT_ACTIVEX))) {
            return Err(Error::BinaryReferencesPossible);
        }
        let edit = PrefixEdit::Rename { old: fold(&old), new: new_name };
        self.closed_world_scan(&old, &[], &edit)?;
        let mut change = Change::default();
        let mentions = |t: &str| names::mentions_prefix(t, &old);
        self.rewrite_everywhere(&mut change, &edit, &mentions, &[], &[])?;
        self.rewrite_registered_formulas(&mut change, &edit, &mentions, &[])?;
        self.rewrite_raw_sheet_names(&mut change, &old, new_name)?;
        // Phase1: CELL/INFO callers and Everything (ADR-0009 §6)
        let seeds = self.formulas_where(&|t| {
            lex(t).is_ok_and(|toks| toks.iter().any(|k| matches!(&k.kind, TokKind::Func(f) if f == "CELL" || f == "INFO")))
        });
        self.invalidate(&mut change, &seeds, Vec::new(), &[])?;
        let wb_part = self.workbook_part.clone();
        {
            let doc = change.doc(&mut self.pkg, &wb_part)?;
            let kids = sheet_children(doc)?;
            let mut sheets = sheets_lens(doc)?;
            let mut s = sml::SheetViewMut(sheets.nth_child_mut_at::<sml::Sheet>(kids[i]).ok_or_else(|| malformed("sheet"))?);
            s.set_name(&crate::escape::encode(new_name))?;
        }
        let targets = self.hyperlink_targets(&edit);
        // a name with a control character cannot be written into a
        // relationship Target (ADR-0009 §6, review R3-m4)
        if let Some((src, id, _, _)) = targets.first()
            && new_name.chars().any(char::is_control)
        {
            return Err(Error::UnrewritableReference {
                part: ooxml_core::opc::rels_part_name(src.as_deref()),
                location: format!("relationship {id} (a control character cannot be written into a Target)"),
            });
        }
        let listed_before = self.listed()?;
        let mut listed_after = listed_before.clone();
        listed_after[i].name = new_name.to_owned();
        if locations::sheet_rewrites(73, false).next().is_some() {
            self.stage_app_xml(&mut change, &listed_before, &listed_after, Some(&edit), None)?;
        }
        for (src, id, before, after) in &targets {
            change.rewritten.push(RewrittenLocation {
                part: ooxml_core::opc::rels_part_name(src.as_deref()),
                location: format!("relationship {id}"),
                before: before.clone(),
                after: after.clone(),
            });
        }
        self.commit(change, true, |plan, pkg| {
            for (src, id, _, after) in &targets {
                plan.set_relationship_target(pkg, src.as_deref(), id, after)?;
            }
            Ok(())
        })
    }

    /// `(source, Id, old target, new target)` of every internal `#Sheet!…`
    /// relationship target to rewrite.
    fn hyperlink_targets(&self, edit: &PrefixEdit<'_>) -> Vec<(Option<String>, String, String, String)> {
        let mut out = Vec::new();
        if locations::sheet_rewrites(71, matches!(edit, PrefixEdit::Delete { .. })).next().is_none() {
            return out;
        }
        let mut sources: Vec<Option<String>> = vec![None];
        sources.extend(self.pkg.part_names().filter(|p| !p.ends_with(".rels")).map(|p| Some(p.to_owned())));
        for src in sources {
            let rels = match &src {
                None => self.pkg.package_relationships().clone(),
                Some(s) => match self.pkg.relationships(s) {
                    Ok(r) => r,
                    Err(_) => continue,
                },
            };
            for r in rels.iter().filter(|r| r.target_mode == TargetMode::Internal) {
                let t = r.target.trim_matches(['\t', '\n', '\r', ' ']);
                if let Some(loc) = t.strip_prefix('#')
                    && let Some(new) = rewrite_location(loc, edit)
                {
                    out.push((src.clone(), r.id.clone(), t.to_owned(), format!("#{new}")));
                }
            }
        }
        out
    }

    /// Rewrites cell formulas (worksheets except `skip`), defined names and
    /// (renames only) `hyperlink/@location`s into the change.
    fn rewrite_everywhere(&mut self, change: &mut Change, edit: &PrefixEdit<'_>, mentions: &dyn Fn(&str) -> bool, skip: &[usize], skip_parts: &[String]) -> Result<()> {
        fn refuse(part: &str, location: String) -> impl FnOnce(Unrewritable) -> Error + use<> {
            let part = part.to_owned();
            move |Unrewritable(why)| Error::UnrewritableReference { part, location: format!("{location} ({why})") }
        }
        let mut done = Vec::new();
        let deleting = matches!(edit, PrefixEdit::Delete { .. });
        let rewrites = |id| locations::sheet_rewrites(id, deleting).next().is_some();
        // defined names
        let wb_part = self.workbook_part.clone();
        let dn = if rewrites(32) { defined_names(self.pkg.xml_part(&wb_part)?) } else { Vec::new() };
        let mut name_edits = Vec::new();
        for (ci, name, local, text) in &dn {
            // a name local to a deleted sheet goes with it: not a rewrite
            // (review of PR #53, m2)
            if local.is_some_and(|l| skip.contains(&(l as usize))) {
                continue;
            }
            if let Some(new) = rewrite_formula(text, edit, mentions).map_err(refuse(&wb_part, format!("definedName {name}")))? {
                name_edits.push((*ci, name.clone(), text.clone(), new));
            }
        }
        if !name_edits.is_empty() {
            let doc = change.doc(&mut self.pkg, &wb_part)?;
            let mut l = names_lens(doc)?.ok_or_else(|| malformed("definedNames"))?;
            for (ci, name, before, after) in name_edits {
                let n = l.nth_child_mut_at::<sml::DefinedName>(ci).ok_or_else(|| malformed("definedName"))?;
                sml::DefinedNameViewMut(n).set_text(&after)?;
                done.push(RewrittenLocation { part: wb_part.clone(), location: format!("definedName {name}"), before, after });
            }
        }
        // cell formulas and hyperlink locations
        for (si, s) in self.sheets.clone().iter().enumerate() {
            if s.kind != SheetKind::Worksheet || skip.contains(&si) {
                continue;
            }
            let Some(part) = &s.part else { continue };
            let (cells, links) = {
                let doc = self.pkg.xml_part(part)?;
                let root = doc.root();
                let mut cells = Vec::new();
                if rewrites(4) && let Some(sd) = root.elements().find(|e| e.is(sml::SheetData::QNAME)) {
                    for (ri, row) in sd.indexed_elements().filter(|(_, e)| e.is(sml::Row::QNAME)) {
                        for (ci, c) in row.indexed_elements().filter(|(_, e)| e.is(sml::Cell::QNAME)) {
                            if let Some(f) = c.elements().find(|e| e.is(sml::CellFormula::QNAME)) {
                                let text = crate::escape::decode(&f.text()).into_owned();
                                let r = c.attr(sml::Cell::ATTR_R).unwrap_or("?").to_owned();
                                if let Some(new) = rewrite_formula(&text, edit, mentions).map_err(refuse(part, format!("{r} formula")))? {
                                    cells.push((ri, ci, r, text, new));
                                }
                            }
                        }
                    }
                }
                let mut links = Vec::new();
                if rewrites(14) && let Some(hl) = root.elements().find(|e| e.is(sml::Hyperlinks::QNAME)) {
                    for (hi, h) in hl.indexed_elements().filter(|(_, e)| e.is(sml::Hyperlink::QNAME)) {
                        if let Some(loc) = h.attr(ooxml_core::QName::new("", "location"))
                            && let Some(new) = rewrite_location(loc, edit)
                        {
                            links.push((hi, h.attr(ooxml_core::QName::new("", "ref")).unwrap_or("?").to_owned(), loc.to_owned(), new));
                        }
                    }
                }
                (cells, links)
            };
            if cells.is_empty() && links.is_empty() {
                continue;
            }
            let doc = change.doc(&mut self.pkg, part)?;
            for (ri, ci, r, before, after) in cells {
                let ws = doc.lens_mut::<sml::Worksheet>()?;
                let sd = ws.into_child_by::<sml::SheetData>(|_| true).ok_or_else(|| malformed("sheetData"))?;
                let row = sd.into_child_at::<sml::Row>(ri).ok_or_else(|| malformed("row"))?;
                let cell = row.into_child_at::<sml::Cell>(ci).ok_or_else(|| malformed("cell"))?;
                let mut c = sml::CellViewMut(cell);
                c.cell_formula_mut().ok_or_else(|| malformed("f"))?.set_text(&crate::escape::encode(&after))?;
                done.push(RewrittenLocation { part: part.clone(), location: format!("{r} formula"), before, after });
            }
            for (hi, r, before, after) in links {
                let ws = doc.lens_mut::<sml::Worksheet>()?;
                let hl = ws.into_child_by::<sml::Hyperlinks>(|_| true).ok_or_else(|| malformed("hyperlinks"))?;
                let h = hl.into_child_at::<sml::Hyperlink>(hi).ok_or_else(|| malformed("hyperlink"))?;
                sml::HyperlinkViewMut(h).set_location(&after)?;
                done.push(RewrittenLocation { part: part.clone(), location: format!("hyperlink {r}"), before, after });
            }
        }
        // chart series formulas (`c:f`): spliced into the original bytes
        let charts: Vec<String> = self
            .pkg
            .part_names()
            .filter(|p| !skip_parts.iter().any(|s| s == p))
            .filter(|p| self.pkg.content_type(p).is_some_and(|ct| ct.split(';').next().unwrap_or_default().trim().eq_ignore_ascii_case(CT_CHART)))
            .map(str::to_owned)
            .collect();
        for part in charts {
            let bytes = self.pkg.read_part(&part)?;
            let Ok(doc) = XmlDocument::parse(&bytes) else {
                // quick-xml may read what the tree parser refuses (DTD, depth):
                // a mention there cannot be rewritten
                if mentions(&String::from_utf8_lossy(&bytes)) {
                    return Err(Error::UnrewritableReference { part, location: "a chart this version cannot parse".into() });
                }
                continue;
            };
            let src = doc.source();
            let mut edits: Vec<(std::ops::Range<usize>, String, String)> = Vec::new();
            let mut stack = vec![doc.root()];
            while let Some(n) = stack.pop() {
                stack.extend(n.elements());
                if !locations::sheet_rewrites(47, deleting).any(|row| row.matches_chart_element(n.name())) {
                    continue;
                }
                let text = n.text().into_owned();
                let Some(new) = rewrite_formula(&text, edit, mentions).map_err(refuse(&part, "c:f".into()))? else { continue };
                let span = n.span().ok_or_else(|| malformed("c:f span"))?;
                let Some((close, _)) = span.close else { return Err(malformed("c:f without text")) };
                if n.elements().next().is_some() {
                    return Err(Error::UnrewritableReference { part: part.clone(), location: "c:f with child elements".into() });
                }
                edits.push((span.open_end..close, text, new));
            }
            if edits.is_empty() {
                continue;
            }
            edits.sort_by_key(|e| e.0.start);
            let mut out = String::with_capacity(src.len());
            let mut last = 0;
            for (r, before, after) in edits {
                out.push_str(&src[last..r.start]);
                for c in after.chars() {
                    match c {
                        '&' => out.push_str("&amp;"),
                        '<' => out.push_str("&lt;"),
                        '>' => out.push_str("&gt;"),
                        c => out.push(c),
                    }
                }
                last = r.end;
                done.push(RewrittenLocation { part: part.clone(), location: "c:f".into(), before, after });
            }
            out.push_str(&src[last..]);
            change.bytes.insert(part, out.into_bytes());
        }
        change.rewritten.extend(done);
        Ok(())
    }

    /// Raw-name locations (no `!`, so the scan cannot see them; ADR-0009 §6
    /// table, review of PR #53 B2): a pivot cache's `worksheetSource/@sheet`
    /// is rewritten; a consolidation `rangeSet/@sheet` or `dataRef/@sheet`
    /// naming the sheet refuses. A source with `r:id` is another workbook.
    fn rewrite_raw_sheet_names(&mut self, change: &mut Change, old: &str, new: &str) -> Result<()> {
        let of = fold(old);
        let parts: Vec<String> = self
            .pkg
            .part_names()
            .filter(|p| self.pkg.content_type(p).is_some_and(|ct| ct.to_ascii_lowercase().ends_with("xml")))
            .map(str::to_owned)
            .collect();
        for p in parts {
            let bytes = self.pkg.read_part(&p)?;
            // pre-filter on the element names, which XML cannot escape: the
            // name itself may be written with entities (`R&amp;D`, `D&#97;ta`)
            // or `_xHHHH_` (PR #53 review R2-B1)
            if !["worksheetSource", "rangeSet", "dataRef"].iter().any(|e| bytes.windows(e.len()).any(|w| w == e.as_bytes())) {
                continue;
            }
            let pivot = self.pkg.content_type(&p).is_some_and(|ct| ct.ends_with("pivotCacheDefinition+xml"));
            let Ok(d) = XmlDocument::parse(&bytes) else {
                if pivot {
                    return Err(Error::UnrewritableReference { part: p, location: "a pivot cache this version cannot parse".into() });
                }
                continue;
            };
            let mut rewrite = None;
            let mut stack = vec![(d.root(), Vec::<(String, String)>::new())];
            while let Some((n, mut path)) = stack.pop() {
                path.push((n.name().ns().to_owned(), n.name().local().to_owned()));
                stack.extend(n.elements().map(|child| (child, path.clone())));
                let local = n.name().local();
                if !matches!(local, "worksheetSource" | "rangeSet" | "dataRef") || n.name().ns() != crate::SML_NS {
                    continue;
                }
                let Some(sheet) = n.attr(ooxml_core::QName::new("", "sheet")) else { continue };
                if fold(&crate::escape::decode(sheet)) != of || n.attrs().iter().any(|a| a.name().local() == "id" && a.name().ns() == R_NS) {
                    continue;
                }
                let at_source = pivot && locations::find(PartKind::PivotCache, &path, Some("sheet"), None)
                    .is_some_and(|row| row.id == 72 && row.rename == Rule::Rewrite);
                if !at_source {
                    return Err(Error::UnrewritableReference { part: p, location: format!("<{local}> attribute sheet") });
                }
                rewrite = Some(crate::escape::decode(sheet).into_owned());
            }
            if let Some(before) = rewrite {
                let doc = change.doc(&mut self.pkg, &p)?;
                let def = doc.lens_mut::<sml::PivotCacheDefinition>()?;
                let src = def.into_child_by::<sml::CacheSource>(|_| true).ok_or_else(|| malformed("cacheSource"))?;
                let ws = src.into_child_by::<sml::WorksheetSource>(|_| true).ok_or_else(|| malformed("worksheetSource"))?;
                // ST_Xstring, as `sheet/@name` (review R2 nit)
                sml::WorksheetSourceViewMut(ws).set_sheet(&crate::escape::encode(new))?;
                change.rewritten.push(RewrittenLocation { part: p, location: "worksheetSource sheet".into(), before, after: new.to_owned() });
            }
        }
        Ok(())
    }

    /// Stages formulas in the additional ADR-0018 locations. Legacy writers
    /// retain their existing report labels and serialization order.
    fn rewrite_registered_formulas(
        &self, change: &mut Change, edit: &PrefixEdit<'_>, mentions: &dyn Fn(&str) -> bool, skip: &[String],
    ) -> Result<()> {
        for part in self.pkg.part_names().filter(|part| !skip.iter().any(|p| p == part)) {
            let ct = self.pkg.content_type(part).unwrap_or_default();
            let ct = ct.split(';').next().unwrap_or_default().trim().to_ascii_lowercase();
            let kind = if ct == CT_WORKSHEET && self.sheets.iter().any(|sheet| sheet.part.as_deref() == Some(part)) {
                PartKind::Worksheet
            } else if ct == CT_CHART {
                PartKind::Chart
            } else {
                locations::additional_part(&ct)
            };
            if matches!(kind, PartKind::Other | PartKind::ChartEx) {
                continue;
            }
            let bytes = if let Some(doc) = change.docs.get(part) {
                doc.serialize().into_bytes()
            } else if let Some(bytes) = change.bytes.get(part) {
                bytes.clone()
            } else {
                self.pkg.read_part(part)?
            };
            if kind == PartKind::Chart && !bytes.windows(b"pivotSource".len()).any(|window| window == b"pivotSource") {
                continue;
            }
            if kind == PartKind::Worksheet && ![b"conditionalFormatting".as_slice(), b"dataValidations", b"controls", b"extLst"].iter().any(|tag| bytes.windows(tag.len()).any(|window| window == *tag)) {
                continue;
            }
            let doc = match XmlDocument::parse(&bytes) {
                Ok(doc) => doc,
                Err(_) if mentions(&String::from_utf8_lossy(&bytes)) => {
                    return Err(Error::UnrewritableReference { part: part.to_owned(), location: "a part this version cannot parse".into() });
                }
                Err(_) => continue,
            };
            let mut ranges = Vec::new();
            collect_registered_formulas(doc.source(), doc.root(), &mut Vec::new(), None, kind, part, edit, mentions, &mut ranges)?;
            if ranges.is_empty() {
                continue;
            }
            let source = doc.source();
            let mut xml = String::with_capacity(source.len());
            let mut cursor = 0;
            ranges.sort_by_key(|(range, _)| range.start);
            for (range, rewrite) in ranges {
                xml.push_str(&source[cursor..range.start]);
                xml.push_str(&quick_xml::escape::escape(&rewrite.after));
                cursor = range.end;
                change.rewritten.push(rewrite);
            }
            xml.push_str(&source[cursor..]);
            if kind == PartKind::Chart {
                change.bytes.insert(part.to_owned(), xml.into_bytes());
            } else {
                change.docs.insert(part.to_owned(), XmlDocument::parse(xml.as_bytes())?);
            }
        }
        Ok(())
    }

    /// The closed-world scan over every XML part except `skip` (ADR-0009
    /// §6): a mention of `name` in sheet-prefix form outside the places
    /// this version rewrites (or leaves on purpose) refuses.
    fn closed_world_scan(&self, name: &str, skip: &[String], edit: &PrefixEdit<'_>) -> Result<()> {
        let worksheets: HashSet<&str> = self.sheets.iter().filter(|s| s.kind == SheetKind::Worksheet).filter_map(|s| s.part.as_deref()).collect();
        for p in self.pkg.part_names() {
            if skip.iter().any(|s| s == p) || Some(p) == self.calc_chain.as_deref() {
                continue;
            }
            let ct = self.pkg.content_type(p).unwrap_or_default();
            let essence = ct.split(';').next().unwrap_or_default().trim().to_ascii_lowercase();
            let xmlish = essence.ends_with("/xml") || essence.ends_with("+xml") || essence.contains("vmldrawing") || p.ends_with(".rels");
            if !xmlish {
                continue;
            }
            let lower = p.to_ascii_lowercase();
            let kind = if worksheets.contains(p) {
                PartKind::Worksheet
            } else if p == self.workbook_part {
                PartKind::Workbook
            } else if essence == CT_CHART {
                PartKind::Chart
            } else if lower.ends_with(".rels") {
                PartKind::Relationships
            } else if lower.starts_with("docprops/")
                || essence.ends_with("sharedstrings+xml")
                || essence.contains("comments")
                || essence.contains("threadedcomments")
                || essence.ends_with("person+xml")
                // another workbook's names and sheets (review R2 nit)
                || essence.ends_with("externallink+xml")
            {
                PartKind::NonReference
            } else {
                locations::additional_part(&essence)
            };
            scan_part(p, &self.pkg.read_part(p)?, kind, name, Some(edit))?;
        }
        Ok(())
    }

    // ------------------------------------------------------------ delete

    /// Deletes a worksheet with the parts it owns (ADR-0009 §5): drawings,
    /// charts, comments, VML, tables, printer settings, images, unless
    /// another part also uses them. References to it become a bare `#REF!`;
    /// 3-D endpoints move inward; names local to it are removed;
    /// `calcChain.xml` is removed. Refused for the last visible sheet,
    /// pivot sources, tables used elsewhere, sheets with parts outside the
    /// cascade table, and references this version cannot rewrite. Names
    /// local to the sheet go with it and are not reported as rewritten.
    ///
    /// `docProps/app.xml` is kept consistent when its shape is recognized
    /// (ADR-0009 §11); otherwise it is left byte-identical.
    pub fn delete_sheet(&mut self, sheet: &str) -> Result<Vec<RewrittenLocation>> {
        self.delete(sheet, false)
    }

    /// [`Self::delete_sheet`], but refused ([`Error::SheetInUse`]) when
    /// anything outside the sheet still refers to it: a reference the
    /// lenient delete would rewrite, a 3-D span through it, a
    /// `hyperlink/@location` or a `#Sheet!` target. Its own local names
    /// (e.g. a print area) do not count.
    pub fn delete_sheet_strict(&mut self, sheet: &str) -> Result<Vec<RewrittenLocation>> {
        self.delete(sheet, true)
    }

    fn delete(&mut self, sheet: &str, strict: bool) -> Result<Vec<RewrittenLocation>> {
        self.sheet_structure_guard("delete_sheet")?;
        let d = self.sheet_pos(sheet)?;
        let entry = self.sheets[d].clone();
        match entry.kind {
            SheetKind::Worksheet => {}
            SheetKind::MacroSheet | SheetKind::DialogSheet | SheetKind::Chartsheet | SheetKind::Other => {
                return Err(Error::NotAWorksheet(entry.name.clone()));
            }
        }
        let part = entry.part.clone().expect("worksheet");
        let mut visible: Vec<bool> = self.sheets.iter().map(|s| s.visible).collect();
        visible.remove(d);
        let kinds: Vec<SheetKind> = self.sheets.iter().enumerate().filter(|(i, _)| *i != d).map(|(_, s)| s.kind).collect();
        if !visible.iter().zip(&kinds).any(|(v, k)| *v && *k == SheetKind::Worksheet) {
            return Err(Error::LastVisibleSheet);
        }
        let sheet_id = self.sheet_id_of(d)?;
        // views keyed by sheetId
        {
            let doc = self.pkg.xml_part(&self.workbook_part)?;
            if let Some(cv) = doc.root().elements().find(|e| e.is(sml::CustomWorkbookViews::QNAME))
                && cv.elements().any(|v| v.attr(ooxml_core::QName::new("", "activeSheetId")).and_then(|x| x.trim().parse::<u32>().ok()) == Some(sheet_id))
            {
                return Err(Error::CustomViewReferencesSheet);
            }
        }
        let set = self.closed_set(&part)?;
        self.check_tables_and_pivots(&entry.name, &set)?;
        let mut skip = set.clone();
        skip.extend(self.calc_chain.clone());
        // rewrites
        let order: Vec<String> = self.sheets.iter().map(|s| fold(&s.name)).collect();
        let names: Vec<String> = self.sheets.iter().map(|s| s.name.clone()).collect();
        let worksheet: Vec<bool> = self.sheets.iter().map(|s| s.kind == SheetKind::Worksheet).collect();
        let edit = PrefixEdit::Delete { deleted: d, order, names, worksheet };
        self.closed_world_scan(&entry.name, &skip, &edit)?;
        let mut change = Change::default();
        if set.iter().any(|p| same_name(p, &part)) {
            change.removed_shared_refs = crate::registry::scan_sheet(&self.pkg.read_part(&part)?)?.shared as i64;
        }
        let ename = entry.name.clone();
        let mentions = move |t: &str| names::mentions_prefix(t, &ename);
        self.rewrite_everywhere(&mut change, &edit, &mentions, &[d], &skip)?;
        self.rewrite_registered_formulas(&mut change, &edit, &mentions, &skip)?;
        if strict {
            // everything that refers to the sheet counts, including what
            // the lenient delete leaves alone: 3-D spans through it,
            // hyperlink locations and `#Sheet!` targets (review of PR #53, m2)
            let count = change.rewritten.len() + self.other_references(d, &entry.name)?;
            if count > 0 {
                return Err(Error::SheetInUse { sheet: entry.name.clone(), count });
            }
        }
        // Phase1: everything that read the sheet, 3-D/SHEET callers, Everything
        let whole = Rect { sheet: d, r1: 0, c1: 0, r2: MAX_ROWS - 1, c2: MAX_COLS - 1 };
        let mut seeds = self.three_d_and_sheet_callers();
        seeds.retain(|f| self.census.get(*f).is_some_and(|c| c.sheet != d));
        self.invalidate(&mut change, &seeds, vec![whole], &[d])?;
        // workbook.xml
        let wb_part = self.workbook_part.clone();
        let d32 = d as u32;
        let active = {
            let doc = change.doc(&mut self.pkg, &wb_part)?;
            let kids = sheet_children(doc)?;
            sheets_lens(doc)?.remove_at(kids[d]);
            remap_local_names(doc, &|i| match i.cmp(&d32) {
                std::cmp::Ordering::Less => Some(i),
                std::cmp::Ordering::Equal => None,
                std::cmp::Ordering::Greater => Some(i - 1),
            })?;
            Self::remap_views(doc, &|i| if i > d32 { i - 1 } else { i }, &visible)?
        };
        // the active sheet after the delete, as an old position
        let active_old = active.map(|a| if a as usize >= d { a as usize + 1 } else { a as usize });
        self.select_tab(&mut change, active_old, &[d])?;
        let before = self.listed()?;
        let mut after = before.clone();
        after.remove(d);
        if locations::sheet_rewrites(73, true).next().is_some() {
            self.stage_app_xml(&mut change, &before, &after, None, Some(&entry.name))?;
        }
        let calc_chain = self.calc_chain.clone();
        let set_refs: Vec<String> = set.clone();
        let removed_table_max = self.removed_table_max(&set);
        let out = self.commit(change, true, move |plan, pkg| {
            let names: Vec<&str> = set_refs.iter().map(String::as_str).collect();
            plan.remove_parts(pkg, &names, &[REL_WORKSHEET])?;
            if let Some(cc) = &calc_chain {
                plan.remove_parts(pkg, &[cc.as_str()], &[REL_CALC_CHAIN])?;
            }
            Ok(())
        })?;
        self.removed_sheet_ids.push(sheet_id);
        self.forget_removed_tables(&set, removed_table_max);
        Ok(out)
    }

    /// References to sheet `d` that a delete does not rewrite: formulas and
    /// names with a 3-D span strictly through it, hyperlink locations and
    /// `#Sheet!` relationship targets.
    fn other_references(&mut self, d: usize, name: &str) -> Result<usize> {
        let f = fold(name);
        let pos_of = |s: &str| self.sheets.iter().position(|x| fold(&x.name) == fold(s));
        let mut n = 0;
        let texts: Vec<String> = self.census.alive().filter(|(_, c)| c.sheet != d).map(|(_, c)| c.text.clone()).chain(self.env_texts()).collect();
        for t in &texts {
            let Ok(toks) = lex(t) else { continue };
            for k in toks {
                let p = match &k.kind {
                    TokKind::Ref(r) => &r.prefix,
                    TokKind::Name { prefix, .. } => prefix,
                    _ => continue,
                };
                if let Prefix::Sheets(a, z) = p
                    && let (Some(pa), Some(pz)) = (pos_of(a), pos_of(z))
                    && pa.min(pz) < d
                    && d < pa.max(pz)
                {
                    n += 1;
                }
            }
        }
        for (i, s) in self.sheets.clone().iter().enumerate() {
            if i == d || s.kind != SheetKind::Worksheet {
                continue;
            }
            let Some(part) = &s.part else { continue };
            let doc = self.pkg.xml_part(part)?;
            if let Some(hl) = doc.root().elements().find(|e| e.is(sml::Hyperlinks::QNAME)) {
                n += hl.elements().filter_map(|h| h.attr(ooxml_core::QName::new("", "location"))).filter(|l| names::mentions_prefix(l, name)).count();
            }
        }
        n += self.hyperlink_targets(&PrefixEdit::Rename { old: f, new: "x" }).len();
        Ok(n)
    }

    fn sheet_id_of(&mut self, pos: usize) -> Result<u32> {
        let doc = self.pkg.xml_part(&self.workbook_part)?;
        let kids = sheet_children(doc)?;
        let sheets = doc.root().elements().find(|e| e.is(sml::Sheets::QNAME)).expect("checked");
        let s = sheets.children()[kids[pos]].as_element().expect("element");
        s.attr(ooxml_core::QName::new("", "sheetId")).and_then(|v| v.trim().parse().ok()).ok_or_else(|| malformed("sheet without sheetId"))
    }

    /// The sheet and every part reachable from it through the cascade
    /// relationship types that no part outside the set reaches.
    fn closed_set(&self, sheet_part: &str) -> Result<Vec<String>> {
        let mut set = vec![sheet_part.to_owned()];
        let mut unsupported = Vec::new();
        let mut i = 0;
        while i < set.len() {
            let p = set[i].clone();
            i += 1;
            let rels = self.pkg.relationships(&p)?;
            for r in rels.iter() {
                if r.target_mode == TargetMode::External {
                    continue;
                }
                if r.target.trim().starts_with('#') {
                    continue;
                }
                let ty = last_segment(&r.rel_type);
                if ty == "hyperlink" {
                    continue;
                }
                if !CASCADE.contains(&ty) {
                    unsupported.push(r.rel_type.clone());
                    continue;
                }
                let Some(t) = rels.resolve(r) else { continue };
                let Some(canon) = self.pkg.part_names().find(|n| same_name(n, &t)).map(str::to_owned) else { continue };
                if !set.contains(&canon) {
                    set.push(canon);
                }
            }
        }
        if !unsupported.is_empty() {
            unsupported.sort();
            unsupported.dedup();
            return Err(Error::SheetHasUnsupportedParts { rel_types: unsupported });
        }
        // keep parts that something outside the set uses (fixpoint)
        loop {
            let mut keep = None;
            for p in set.iter().skip(1) {
                let inc = self.pkg.incoming_relationships(p)?;
                if inc.iter().any(|r| r.source.as_ref().is_none_or(|s| !set.iter().any(|x| same_name(x, s)))) {
                    keep = Some(p.clone());
                    break;
                }
            }
            match keep {
                Some(k) => set.retain(|x| *x != k),
                None => break,
            }
        }
        Ok(set)
    }

    /// `TableInUse` / `SheetIsPivotSource` (ADR-0009 §5).
    ///
    /// A table of the closed set is in use when a part outside the set
    /// names it in a structured reference (`Name[`, any text or attribute:
    /// cell formulas, names, other tables' formulas, data validation, …) or
    /// an x15 table slicer cache points at its id; uses on the deleted sheet
    /// itself go with it (review of PR #53, m3). A pivot cache that takes its
    /// data from the sheet — `worksheetSource/@sheet`, or `@name` naming one
    /// of its tables or a defined name that refers to it (M2) — refuses; so
    /// does a consolidation `rangeSet`/`dataRef` naming it.
    fn check_tables_and_pivots(&self, sheet_name: &str, set: &[String]) -> Result<()> {
        let mut tables: Vec<(String, Option<String>)> = Vec::new();
        for p in set {
            if self.pkg.content_type(p).is_some_and(|ct| ct.ends_with("table+xml"))
                && let Ok(d) = XmlDocument::parse(&self.pkg.read_part(p)?)
            {
                let id = d.root().attr(ooxml_core::QName::new("", "id")).map(str::to_owned);
                for a in ["name", "displayName"] {
                    if let Some(n) = d.root().attr(ooxml_core::QName::new("", a)) {
                        tables.push((n.to_owned(), id.clone()));
                    }
                }
            }
        }
        let sf = fold(sheet_name);
        let wb_names = self.pkg.parsed_part(&self.workbook_part).map(defined_names).unwrap_or_default();
        // names that refer to the sheet, directly or through other names
        // (`Src2 = Src`, `Src = Data!A1`; review R2-m2). The set only grows,
        // so a cycle of names ends the loop.
        let mut names_on_sheet: Vec<String> = wb_names.iter().filter(|(_, _, _, t)| names::mentions_prefix(t, sheet_name)).map(|(_, n, _, _)| fold(n)).collect();
        loop {
            let more: Vec<String> = wb_names
                .iter()
                .filter(|(_, n, _, _)| !names_on_sheet.contains(&fold(n)))
                .filter(|(_, _, _, t)| {
                    lex(t).is_ok_and(|toks| toks.iter().any(|k| matches!(&k.kind, TokKind::Name { name, .. } if names_on_sheet.contains(&fold(name)))))
                })
                .map(|(_, n, _, _)| fold(n))
                .collect();
            if more.is_empty() {
                break;
            }
            names_on_sheet.extend(more);
        }
        let parts: Vec<String> = self.pkg.part_names().filter(|p| !set.iter().any(|x| x == p)).map(str::to_owned).collect();
        for p in parts {
            let ct = self.pkg.content_type(&p).unwrap_or_default().to_ascii_lowercase();
            let xmlish = ct.ends_with("xml") || p.ends_with(".rels");
            if !xmlish || p.to_ascii_lowercase().starts_with("docprops/") || ct.contains("sharedstrings") || ct.contains("comments") {
                continue;
            }
            let bytes = self.pkg.read_part(&p)?;
            let Ok(d) = XmlDocument::parse(&bytes) else {
                let text = fold(&String::from_utf8_lossy(&bytes));
                if let Some((t, _)) = tables.iter().find(|(t, _)| text.contains(&format!("{}[", fold(t)))) {
                    return Err(Error::TableInUse(t.clone()));
                }
                if ct.ends_with("pivotcachedefinition+xml") && text.contains(&sf) {
                    return Err(Error::SheetIsPivotSource(sheet_name.to_owned()));
                }
                continue;
            };
            let mut stack = vec![d.root()];
            while let Some(n) = stack.pop() {
                stack.extend(n.elements());
                let attr = |a: &'static str| n.attr(ooxml_core::QName::new("", a));
                let texts = std::iter::once(n.text().into_owned()).chain(n.attrs().iter().map(|a| a.value().to_owned()));
                for t in texts {
                    let ft = fold(&t);
                    if let Some((name, _)) = tables.iter().find(|(name, _)| ft.contains(&format!("{}[", fold(name)))) {
                        return Err(Error::TableInUse(name.clone()));
                    }
                }
                match n.name().local() {
                    "tableSlicerCache" => {
                        if let Some((name, _)) = tables.iter().find(|(_, id)| id.is_some() && attr("tableId") == id.as_deref()) {
                            return Err(Error::TableInUse(name.clone()));
                        }
                    }
                    "worksheetSource" if !n.attrs().iter().any(|a| a.name().local() == "id" && a.name().ns() == R_NS) => {
                        let s = attr("sheet").map(|v| fold(&crate::escape::decode(v)));
                        let nm = attr("name").map(fold);
                        let by_table = nm.as_ref().is_some_and(|nm| tables.iter().any(|(t, _)| fold(t) == *nm));
                        let by_name = nm.as_ref().is_some_and(|nm| names_on_sheet.contains(nm));
                        if s.as_deref() == Some(sf.as_str()) || by_table || by_name {
                            return Err(Error::SheetIsPivotSource(sheet_name.to_owned()));
                        }
                    }
                    "rangeSet" | "dataRef" if attr("sheet").map(|v| fold(&crate::escape::decode(v))).as_deref() == Some(sf.as_str()) => {
                        return Err(Error::UnrewritableReference { part: p.clone(), location: format!("<{}> attribute sheet", n.name().local()) });
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }
}

type FormulaRewrite = (std::ops::Range<usize>, RewrittenLocation);

#[expect(clippy::too_many_arguments, reason = "the traversal carries the same dispatch context at each node")]
fn collect_registered_formulas(
    source: &str, node: &ooxml_core::RawNode, path: &mut Vec<(String, String)>, extension: Option<&str>, kind: PartKind,
    part: &str, edit: &PrefixEdit<'_>, mentions: &dyn Fn(&str) -> bool, ranges: &mut Vec<FormulaRewrite>,
) -> Result<()> {
    path.push((node.name().ns().to_owned(), node.name().local().to_owned()));
    let extension = if node.is(ooxml_core::QName::new(crate::SML_NS, "ext")) {
        Some(node.attr(ooxml_core::QName::new("", "uri")).unwrap_or_default())
    } else {
        extension
    };
    let mut rewrite = |attr: Option<&str>, before: &str| -> Result<()> {
        let Some(row) = locations::find(kind, path, attr, extension) else { return Ok(()) };
        if !mentions(before) {
            return Ok(());
        }
        if matches!(row.id, 4 | 32 | 47) || row.syntax != Syntax::PivotSource
            && match edit { PrefixEdit::Rename { .. } => row.rename, PrefixEdit::Delete { .. } => row.delete } != Rule::Rewrite
        {
            return Ok(());
        }
        if attr == Some("val") && node.attr(ooxml_core::QName::new("", "type")) != Some("formula") {
            return Err(Error::UnrewritableReference { part: part.to_owned(), location: "<cfvo> attribute val".into() });
        }
        let mut location = path.iter().map(|(_, name)| name.as_str()).collect::<Vec<_>>().join("/");
        if let Some(attr) = attr {
            location.push('@');
            location.push_str(attr);
        }
        let refuse = || Error::UnrewritableReference { part: part.to_owned(), location: location.clone() };
        let after = match row.syntax {
            Syntax::PivotSource => {
                let PrefixEdit::Rename { new, .. } = edit else { return Err(refuse()) };
                let (book, source) = before.rsplit_once(']').ok_or_else(refuse)?;
                let (sheet, pivot) = source.rsplit_once('!').ok_or_else(refuse)?;
                if !mentions(&format!("{sheet}!")) || names::needs_quotes(new) {
                    return Err(refuse());
                }
                format!("{book}]{new}!{pivot}")
            }
            _ => {
                let Some(after) = rewrite_formula(before, edit, mentions).map_err(|_| refuse())? else { return Ok(()) };
                after
            }
        };
        let span = node.span().ok_or_else(refuse)?;
        let range = if let Some(attr) = attr {
            let mut reader = quick_xml::Reader::from_str(&source[span.start..span.open_end]);
            let start = match reader.read_event().map_err(|_| refuse())? {
                quick_xml::events::Event::Start(e) | quick_xml::events::Event::Empty(e) => e,
                _ => return Err(refuse()),
            };
            start.attributes().flatten().find(|a| a.key.0 == attr)
                .and_then(|a| match a.value {
                    std::borrow::Cow::Borrowed(s) => source.substr_range(s).map(Into::into),
                    std::borrow::Cow::Owned(_) => None,
                }).ok_or_else(refuse)?
        } else {
            let (close, _) = span.close.ok_or_else(refuse)?;
            if node.children().iter().any(|item| !matches!(item, ooxml_core::xml::RawItem::Text(_))) {
                return Err(refuse());
            }
            span.open_end..close
        };
        ranges.push((range, RewrittenLocation { part: part.to_owned(), location, before: before.to_owned(), after }));
        Ok(())
    };
    for attr in node.attrs().iter().filter(|attr| attr.name().ns().is_empty()) {
        rewrite(Some(attr.name().local()), attr.value())?;
    }
    if locations::find(kind, path, None, extension).is_some_and(|row| !matches!(row.id, 4 | 32 | 47)) {
        rewrite(None, &node.text())?;
    }
    for child in node.elements() {
        if path.len() == 1 && kind == PartKind::Worksheet && child.is(sml::SheetData::QNAME) {
            continue;
        }
        collect_registered_formulas(source, child, path, extension, kind, part, edit, mentions, ranges)?;
    }
    path.pop();
    Ok(())
}
