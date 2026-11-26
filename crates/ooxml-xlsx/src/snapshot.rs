//! Cell snapshots (ADR-0024 C2): a copy of one `c` node with its spans, cache
//! and dirty state, plus the cell's derived state in the workbook, and the
//! restore that puts both back.
//!
//! Visibility: the Journal (J0) lives in this crate, so the API is
//! `pub(crate)`; nothing here is new public API.

#![cfg_attr(not(test), allow(dead_code))] // the Journal (J0) is the first caller

use std::collections::HashMap;

use ooxml_core::{ElementSpec, RawAttr, RawNode};
use ooxml_schema::sml;

use crate::census::FCell;
use crate::error::{Error, Result};
use crate::formula::precedents::Prec;
use crate::graph::precedents_of;
use crate::reference::CellIndex;
use crate::workbook::Workbook;

/// Which `s` a restore leaves on the cell (ADR-0024 §4.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RestoreStyle {
    /// The snapshot's own `s`: a full restore.
    Snapshot,
    /// The cell's current `s`: a content restore.
    Current,
}

/// What an ancestor looked like when the snapshot was taken. A restore clears
/// an ancestor's dirty flags only if nothing below it is dirty and its own
/// tags were then as they are now: an ancestor that was dirty for a reason of
/// its own (another attribute, a changed child list) stays dirty.
#[derive(Clone, Debug)]
struct AncestorMark {
    own_dirty: bool,
    subtree_dirty: bool,
    dirty_children: usize,
    attrs: Vec<RawAttr>,
}

impl AncestorMark {
    fn of(n: &RawNode) -> Self {
        AncestorMark { own_dirty: n.own_dirty(), subtree_dirty: n.subtree_dirty(), dirty_children: n.dirty_child_count(), attrs: n.attrs().to_vec() }
    }

    /// The node's dirtiness came only from descendants, and its attributes
    /// are the ones it had. (Dirty at the snapshot with no dirty child means
    /// a reason of its own, which a restore cannot undo.)
    fn allows_clean(&self, n: &RawNode) -> bool {
        !self.own_dirty && !(self.subtree_dirty && self.dirty_children == 0) && n.attrs() == self.attrs.as_slice()
    }
}

/// A cell as it was (`None` node: the cell did not exist), with its derived
/// state.
#[derive(Clone, Debug)]
pub(crate) struct CellSnapshot {
    /// The `c` node, a clone with spans, cache and dirty flags.
    node: Option<RawNode>,
    /// The census entry and the dependency edges it had (`Prec`).
    formula: Option<(FCell, Prec)>,
    /// The cell is in the census's `vm` set.
    vm: bool,
    /// The cell's row-index entry: (0-based column, item index in its row).
    index_entry: Option<(u32, usize)>,
    /// Worksheet, `sheetData` and `row` as they were; `None` = absent.
    marks: [Option<AncestorMark>; 3],
}

impl CellSnapshot {
    /// The `c` node as it was; `None` for a cell that did not exist.
    pub(crate) fn node(&self) -> Option<&RawNode> {
        self.node.as_ref()
    }

    /// The census entry the cell had.
    pub(crate) fn formula(&self) -> Option<&FCell> {
        self.formula.as_ref().map(|(c, _)| c)
    }

    /// The cell's row-index entry (column, item) when it was taken.
    pub(crate) fn index_entry(&self) -> Option<(u32, usize)> {
        self.index_entry
    }

    pub(crate) fn has_vm(&self) -> bool {
        self.vm
    }
}

/// Item indices from the root to the `sheetData`, the row and the cell, as
/// far as they exist.
pub(crate) struct Spot {
    pub(crate) sheet_data: Option<usize>,
    pub(crate) row: Option<usize>,
    pub(crate) cell: Option<usize>,
    row_insert_at: usize,
    cell_insert_at: usize,
}

impl Workbook {
    pub(crate) fn cell_spot(&mut self, sheet: usize, at: CellIndex) -> Result<(String, Spot)> {
        self.check_poisoned()?;
        self.ensure_index(sheet)?;
        let part = self.sheets[sheet].part.clone().ok_or_else(|| Error::Malformed("worksheet part".into()))?;
        let doc = self.pkg.xml_part(&part)?;
        let index = self.indexes.get(&sheet).expect("built by ensure_index");
        let loc = crate::worksheet::locate(doc, index, &part, at)?;
        let sheet_data = doc.root().children().iter().position(|c| c.as_element().is_some_and(|e| e.is(sml::SheetData::QNAME)));
        let spot = Spot { sheet_data, row: loc.row, cell: loc.cell, row_insert_at: loc.row_insert_at, cell_insert_at: loc.cell_insert_at };
        Ok((part, spot))
    }

    /// A snapshot of the cell at `sheet!at`. A missing cell (`c` absent)
    /// gives a snapshot whose [`CellSnapshot::node`] is `None`; it still
    /// records the ancestors, so that a restore can tell a row the session
    /// created from one that was there. Reads only; `Err` for a sheet whose
    /// cells cannot be located (the refusals of any cell edit).
    pub(crate) fn snapshot_cell(&mut self, sheet: usize, at: CellIndex) -> Result<CellSnapshot> {
        let (part, spot) = self.cell_spot(sheet, at)?;
        let doc = self.pkg.xml_part(&part)?;
        let sd = spot.sheet_data.and_then(|i| doc.root().children()[i].as_element());
        let row = sd.zip(spot.row).and_then(|(sd, i)| sd.children()[i].as_element());
        let cell = row.zip(spot.cell).and_then(|(r, i)| r.children()[i].as_element());
        let marks = [Some(AncestorMark::of(doc.root())), sd.map(AncestorMark::of), row.map(AncestorMark::of)];
        let Some(cell) = cell else {
            return Ok(CellSnapshot { node: None, formula: None, vm: false, index_entry: None, marks });
        };
        let node = cell.clone();
        let formula = self.census.at(sheet, at).and_then(|fid| self.census.get(fid)).map(|c| {
            let census = &self.census;
            let prec = precedents_of(&self.env, census, c, &|s, si| census.master_of(s, si), &mut HashMap::new());
            (c.clone(), prec)
        });
        let vm = self.census.vm_cells.contains(&(sheet, at));
        let index_entry = spot.cell.map(|item| (at.col, item));
        Ok(CellSnapshot { node: Some(node), formula, vm, index_entry, marks })
    }

    /// Puts the cell at `sheet!at` back as `snapshot` holds it, with its
    /// derived state: the node (replaced, inserted, or, for a missing cell,
    /// removed together with a row the session created for it and that has
    /// nothing else), the census entry and dependency edges, the `vm` entry
    /// and the row index; `style_sensitive` is dropped unless the cell's formula is unchanged, and the shared-string registry's reference count follows the `t="s"` cells. `style` picks the
    /// snapshot's `s` or the current one (a content restore).
    ///
    /// An ancestor of the node loses its dirty flags only when no other
    /// descendant is dirty (ADR-0024 C2). `Err` leaves the workbook as it was.
    pub(crate) fn restore_cell(&mut self, sheet: usize, at: CellIndex, snapshot: &CellSnapshot, style: RestoreStyle) -> Result<()> {
        let (part, spot) = self.cell_spot(sheet, at)?;
        let marks = &snapshot.marks;
        let clean = |depth: usize, n: &RawNode| marks.get(depth).and_then(Option::as_ref).is_some_and(|m| m.allows_clean(n));
        let row_no = at.row + 1;
        // `t="s"` references the cell holds now (the registry's `count`)
        let shared_before = {
            let doc = self.pkg.xml_part(&part)?;
            let sd = spot.sheet_data.and_then(|i| doc.root().children()[i].as_element());
            let row = sd.zip(spot.row).and_then(|(sd, i)| sd.children()[i].as_element());
            row.zip(spot.cell).and_then(|(r, i)| r.children()[i].as_element()).is_some_and(is_shared_cell)
        };
        let doc = self.pkg.xml_part_mut(&part)?;
        let index = self.indexes.get_mut(&sheet).expect("built by ensure_index");
        let sd_at = spot.sheet_data;
        match (&snapshot.node, spot.cell, spot.row, sd_at) {
            // the cell is there: its node goes back
            (Some(node), Some(ci), Some(ri), Some(si)) => {
                let current_s = doc.root().children()[si].as_element().and_then(|sd| sd.children()[ri].as_element()).and_then(|r| r.children()[ci].as_element()).map(|c| c.attr(sml::Cell::ATTR_S).map(str::to_owned));
                let path = [si, ri, ci];
                doc.replace_element_at(&path, node.clone(), clean);
                if style == RestoreStyle::Current
                    && let Some(current_s) = current_s
                {
                    set_style_attr(doc, &path, current_s.as_deref());
                }
            }
            // the row is there, the cell is gone: insert it
            (Some(node), None, Some(ri), Some(si)) => {
                // no current cell, so a content restore has no current `s` either:
                // the snapshot's node goes in as it was
                doc.insert_element_at(&[si, ri], spot.cell_insert_at, node.clone());
                index.cell_inserted(row_no, at.col, spot.cell_insert_at);
            }
            // the row is gone as well: create it, then insert
            (Some(node), None, None, Some(si)) => {
                {
                    let ws = doc.lens_mut::<sml::Worksheet>()?;
                    let mut sd = ws.into_child_at::<sml::SheetData>(si).ok_or_else(|| Error::Malformed("sheetData".into()))?;
                    // refused here, before anything changed (the cell insert below cannot fail)
                    sd.check_insert_at::<sml::Row>(spot.row_insert_at)?;
                    let mut r = sml::RowViewMut(sd.insert_at::<sml::Row>(spot.row_insert_at).expect("checked above"));
                    r.set_row_index(row_no).expect("`r` is a row attribute");
                }
                index.row_inserted(row_no, spot.row_insert_at);
                doc.insert_element_at(&[si, spot.row_insert_at], 0, node.clone());
                index.cell_inserted(row_no, at.col, 0);
            }
            (Some(_), ..) => return Err(Error::Malformed("the restored cell has no sheetData".into())),
            // the cell did not exist: remove it
            (None, Some(ci), Some(ri), Some(si)) => {
                if style == RestoreStyle::Current {
                    // content restore of a missing cell: an empty `c` with the current `s`
                    empty_cell(doc, &[si, ri, ci]);
                } else {
                    doc.remove_element_at(&[si, ri, ci], clean);
                    index.cell_removed(row_no, at.col, ci);
                    let row = doc.root().children()[si].as_element().and_then(|sd| sd.children()[ri].as_element());
                    // a row the session created has no source span, whichever
                    // snapshot saw it (a snapshot taken later saw it as existing)
                    let session_row = row.is_some_and(|r| r.span().is_none())
                        && row.is_some_and(|r| r.children().is_empty() && r.attrs().iter().all(|a| a.name().is(sml::Row::ATTR_R)));
                    if session_row {
                        doc.remove_element_at(&[si, ri], clean);
                        index.row_removed(row_no, ri);
                    }
                }
            }
            (None, ..) => {}
        }
        // derived state
        let formula_kept = match (self.census.at(sheet, at).and_then(|f| self.census.get(f)), &snapshot.formula) {
            (None, None) => true,
            (Some(cur), Some((old, _))) => format!("{cur:?}") == format!("{old:?}"),
            _ => false,
        };
        if let Some(fid) = self.census.at(sheet, at) {
            self.census.remove(fid);
        }
        if let Some(g) = self.graph.as_mut() {
            g.forget_everything_closure(); // an `Everything` formula may be gone
        }
        if let Some((c, prec)) = &snapshot.formula {
            let fid = self.census.push(c.clone());
            if let Some(g) = self.graph.as_mut() {
                g.add(fid, prec.clone());
            }
        }
        if snapshot.vm {
            self.census.vm_cells.insert((sheet, at));
        } else {
            self.census.vm_cells.remove(&(sheet, at));
        }
        // the cache reads formula texts only: a restore that leaves the cell's
        // formula as it is leaves the answer as it is
        if !formula_kept {
            self.style_sensitive = None;
        }
        // keep the shared-string reference count exact (no registry: the
        // activation scan counts later)
        // (a content restore keeps the node's own `t`; a missing cell has none)
        let shared_after = snapshot.node.as_ref().is_some_and(is_shared_cell);
        let delta = i64::from(shared_after) - i64::from(shared_before);
        crate::registry::strings::SharedStrings::apply(self, &[], delta, delta != 0)?;
        Ok(())
    }
}

fn is_shared_cell(c: &RawNode) -> bool {
    c.attr(sml::Cell::ATTR_T).is_some_and(|t| t.trim() == "s")
}

/// Sets or removes `s` on the cell at `path`. Cannot fail: the path names a
/// `c` we just put there and `s` is one of its attributes.
fn set_style_attr(doc: &mut ooxml_core::XmlDocument, path: &[usize], s: Option<&str>) {
    let mut cell = doc.lens_mut_at::<sml::Cell>(path).expect("the restored `c`");
    match s {
        Some(s) => cell.set_attr(sml::Cell::ATTR_S, s).expect("`s` of a parsed attribute"),
        None => {
            cell.remove_attr(sml::Cell::ATTR_S).expect("`s` is a cell attribute");
        }
    }
}

/// Empties the cell at `path` but for `r` and `s`. Cannot fail, as above.
fn empty_cell(doc: &mut ooxml_core::XmlDocument, path: &[usize]) {
    let mut cell = doc.lens_mut_at::<sml::Cell>(path).expect("the located `c`");
    let n = cell.node().children().len();
    cell.remove_range(0..n);
    for q in [sml::Cell::ATTR_T, sml::Cell::ATTR_CM, sml::Cell::ATTR_VM, sml::Cell::ATTR_PH] {
        cell.remove_attr(q).expect("a cell attribute");
    }
}

#[cfg(test)]
mod tests {
    use ooxml_core::Package;

    use super::*;
    use crate::edit::CellOp;
    use crate::workbook::{CalcPolicy, TEMPLATE};

    const PART: &str = "xl/worksheets/sheet1.xml";
    const HEAD: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><dimension ref="A1:F6"/><sheetData>"#;
    const TAIL: &str = "</sheetData></worksheet>";

    fn sheet_xml() -> String {
        let rows = concat!(
            r#"<row r="1" spans="1:4"><c r="A1"><v>1</v></c><c r="B1"><v>2</v></c><c r="C1"><f>A1+B1</f><v>3</v></c><c r="D1"><f t="shared" ref="D1:D2" si="0">A1*2</f><v>2</v></c></row>"#,
            "\n",
            r#"<row r="2"><c r="A2"><v>10</v></c><c r="C2"><f>C1+1</f><v>4</v></c><c r="D2"><f t="shared" si="0"/><v>20</v></c><c r="E2" vm="1"><v>7</v></c></row>"#,
            "\n",
            r#"<row r="3"/>"#,
            "\n",
            r#"<row r="4"><c r="A4"><f t="array" ref="A4:A5">A1:A2*2</f><v>2</v></c></row>"#,
        );
        format!("{HEAD}{rows}{TAIL}")
    }

    fn open(xml: &str) -> Workbook {
        let mut pkg = Package::from_bytes(TEMPLATE).unwrap();
        pkg.set_part(PART, xml.as_bytes().to_vec()).unwrap();
        let mut wb = Workbook::from_bytes(pkg.to_bytes().unwrap()).unwrap();
        wb.policy = CalcPolicy::KeepStaleCaches; // the strip is the Journal's (C3), not under test
        wb
    }

    fn at(a1: &str) -> CellIndex {
        CellIndex::parse(a1).unwrap()
    }

    fn part(wb: &mut Workbook) -> String {
        wb.pkg.xml_part(PART).unwrap().serialize()
    }

    fn doc_dirty(wb: &mut Workbook) -> bool {
        wb.pkg.xml_part(PART).unwrap().is_dirty()
    }

    /// Everything a snapshot claims to hold, in comparable form.
    fn derived(wb: &mut Workbook, cell: &str) -> String {
        let fcell = wb.census.at(0, at(cell)).and_then(|f| wb.census.get(f)).map(|c| format!("{c:?}"));
        let vm = wb.census.vm_cells.contains(&(0, at(cell)));
        let located = wb.indexes[&0].locate(PART, at(cell)).map(|l| format!("{l:?}")).unwrap();
        // the dependents of every input cell, as positions
        let mut deps = Vec::new();
        for seed in ["A1", "B1", "A2", "C1", "C2", "D1", "D2", "A4"] {
            let d = wb.compute_dependents(0, at(seed));
            let mut cells: Vec<String> = d.cells.iter().map(|(_, c)| c.to_string()).collect();
            cells.sort();
            deps.push(format!("{seed}:{cells:?}"));
        }
        let rows = format!("{:?}", wb.indexes[&0].rows.as_ref().map(|r| r.iter().map(|e| (e.row, e.item, e.cells_end, e.cells.clone().ok())).collect::<Vec<_>>()));
        format!("{fcell:?} {vm} {located} {deps:?} {rows}")
    }

    fn write(wb: &mut Workbook, cell: &str, op: CellOp) {
        wb.edit_cell(0, at(cell), op).unwrap();
    }

    /// write → restore gives the input back, node, flags and derived state.
    fn round_trip(cell: &str, op: CellOp) {
        let xml = sheet_xml();
        let mut wb = open(&xml);
        wb.graph = None;
        let before = derived(&mut wb, cell);
        let snap = wb.snapshot_cell(0, at(cell)).unwrap();
        assert_eq!(snap.formula().is_some(), before.starts_with("Some"));
        write(&mut wb, cell, op);
        assert_ne!(part(&mut wb), xml, "{cell}: the write changed the part");
        wb.restore_cell(0, at(cell), &snap, RestoreStyle::Snapshot).unwrap();
        assert_eq!(part(&mut wb), xml, "{cell}");
        assert!(!doc_dirty(&mut wb), "{cell}: flags are clean again");
        assert!(wb.pkg.xml_part(PART).unwrap().dirty_counts_consistent());
        assert_eq!(derived(&mut wb, cell), before, "{cell}: derived state");
    }

    #[test]
    fn a_constant_cell_round_trips() {
        round_trip("A1", CellOp::Number("99".into()));
        round_trip("A1", CellOp::Text("x".into()));
        round_trip("A1", CellOp::Formula("B1*2".into()));
    }

    #[test]
    fn a_formula_cell_with_a_cache_round_trips() {
        round_trip("C1", CellOp::Number("5".into()));
        round_trip("C1", CellOp::Formula("A1-B1".into()));
        round_trip("C2", CellOp::RemoveFormula);
    }

    #[test]
    fn a_shared_member_round_trips() {
        round_trip("D2", CellOp::Number("5".into()));
        round_trip("D2", CellOp::Formula("A2+1".into()));
    }

    #[test]
    fn a_shared_master_round_trips_with_its_hand_off_target() {
        let xml = sheet_xml();
        let mut wb = open(&xml);
        let before = (derived(&mut wb, "D1"), derived(&mut wb, "D2"));
        // the hand-off turns D2 into the master: the Journal snapshots it too (ADR-0024 §4.4)
        let (d1, d2) = (wb.snapshot_cell(0, at("D1")).unwrap(), wb.snapshot_cell(0, at("D2")).unwrap());
        write(&mut wb, "D1", CellOp::Number("5".into()));
        assert_ne!(part(&mut wb), xml);
        wb.restore_cell(0, at("D1"), &d1, RestoreStyle::Snapshot).unwrap();
        wb.restore_cell(0, at("D2"), &d2, RestoreStyle::Snapshot).unwrap();
        assert_eq!(part(&mut wb), xml);
        assert!(!doc_dirty(&mut wb));
        assert_eq!((derived(&mut wb, "D1"), derived(&mut wb, "D2")), before);
    }

    #[test]
    fn an_array_anchor_round_trips() {
        // a write is refused on an array anchor, so change the node and the census by hand
        let xml = sheet_xml();
        let mut wb = open(&xml);
        let before = derived(&mut wb, "A4");
        let snap = wb.snapshot_cell(0, at("A4")).unwrap();
        {
            let doc = wb.pkg.xml_part_mut(PART).unwrap();
            let mut c = doc.lens_mut_at::<sml::Cell>(&[1, 6, 0]).unwrap();
            c.remove_range(1..2);
            c.set_attr(sml::Cell::ATTR_S, "0").unwrap();
        }
        let fid = wb.census.at(0, at("A4")).unwrap();
        wb.census.remove(fid);
        wb.restore_cell(0, at("A4"), &snap, RestoreStyle::Snapshot).unwrap();
        assert_eq!(part(&mut wb), xml);
        assert!(!doc_dirty(&mut wb));
        assert_eq!(derived(&mut wb, "A4"), before);
    }

    #[test]
    fn a_missing_cell_is_removed_again_and_the_session_row_with_it() {
        let xml = sheet_xml();
        let mut wb = open(&xml);
        // F6: no row 6 at all; B2: a row that stays; C3: a self-closing row that gains a child
        for cell in ["F6", "B2", "C3", "F1"] {
            let before = derived(&mut wb, cell);
            let snap = wb.snapshot_cell(0, at(cell)).unwrap();
            assert!(snap.node().is_none());
            write(&mut wb, cell, CellOp::Number("1".into()));
            assert_ne!(part(&mut wb), xml, "{cell}");
            wb.restore_cell(0, at(cell), &snap, RestoreStyle::Snapshot).unwrap();
            assert_eq!(part(&mut wb), xml, "{cell}");
            assert!(!doc_dirty(&mut wb), "{cell}");
            assert_eq!(derived(&mut wb, cell), before, "{cell}");
        }
    }

    #[test]
    fn a_row_that_was_there_stays_when_a_session_cell_goes() {
        let xml = sheet_xml();
        let mut wb = open(&xml);
        let (b2, e2) = (wb.snapshot_cell(0, at("B2")).unwrap(), wb.snapshot_cell(0, at("F6")).unwrap());
        write(&mut wb, "B2", CellOp::Number("1".into()));
        write(&mut wb, "F6", CellOp::Number("1".into()));
        // another cell shares F6's row: the row must stay
        write(&mut wb, "A6", CellOp::Number("2".into()));
        wb.restore_cell(0, at("F6"), &e2, RestoreStyle::Snapshot).unwrap();
        assert!(part(&mut wb).contains(r#"<row r="6"><c r="A6">"#));
        wb.restore_cell(0, at("B2"), &b2, RestoreStyle::Snapshot).unwrap();
        assert!(!part(&mut wb).contains(r#"r="B2""#));
        assert!(part(&mut wb).contains(r#"<row r="2">"#));
        assert!(wb.pkg.xml_part(PART).unwrap().dirty_counts_consistent());
    }

    #[test]
    fn a_cell_with_vm_gets_its_vm_entry_back() {
        let xml = sheet_xml();
        let mut wb = open(&xml);
        let snap = wb.snapshot_cell(0, at("E2")).unwrap();
        assert!(snap.has_vm());
        assert_eq!(snap.index_entry(), Some((4, 3)));
        assert!(snap.formula().is_none());
        write(&mut wb, "E2", CellOp::StyleOnly);
        wb.census.vm_cells.remove(&(0, at("E2")));
        wb.restore_cell(0, at("E2"), &snap, RestoreStyle::Snapshot).unwrap();
        assert!(wb.census.vm_cells.contains(&(0, at("E2"))));
        assert_eq!(part(&mut wb), xml);
    }

    #[test]
    fn a_content_restore_keeps_the_current_style() {
        let xml = sheet_xml();
        let mut wb = open(&xml);
        let snap = wb.snapshot_cell(0, at("C1")).unwrap();
        write(&mut wb, "C1", CellOp::Number("9".into()));
        // a style written by hand, as `set_style_index` leaves it
        {
            let doc = wb.pkg.xml_part_mut(PART).unwrap();
            let mut c = doc.lens_mut_at::<sml::Cell>(&[1, 0, 2]).unwrap();
            c.set_attr(sml::Cell::ATTR_S, "0").unwrap();
        }
        wb.restore_cell(0, at("C1"), &snap, RestoreStyle::Current).unwrap();
        let out = part(&mut wb);
        assert!(out.contains(r#"<c r="C1" s="0"><f>A1+B1</f><v>3</v></c>"#), "{out}");
        assert!(wb.census.get(wb.census.at(0, at("C1")).unwrap()).unwrap().has_cache, "the cache is back");
        // the full restore takes the snapshot's own style (none)
        wb.restore_cell(0, at("C1"), &snap, RestoreStyle::Snapshot).unwrap();
        assert_eq!(part(&mut wb), xml);
    }

    #[test]
    fn restoring_one_of_two_cells_of_a_row_keeps_the_ancestors_dirty() {
        let xml = sheet_xml();
        let mut wb = open(&xml);
        let (a, b) = (wb.snapshot_cell(0, at("A1")).unwrap(), wb.snapshot_cell(0, at("B1")).unwrap());
        write(&mut wb, "A1", CellOp::Number("7".into()));
        write(&mut wb, "B1", CellOp::Number("8".into()));
        wb.restore_cell(0, at("A1"), &a, RestoreStyle::Snapshot).unwrap();
        {
            let doc = wb.pkg.xml_part(PART).unwrap();
            let sd = &doc.root().children().iter().filter_map(|c| c.as_element()).find(|e| e.is(sml::SheetData::QNAME)).unwrap();
            let row = sd.elements().next().unwrap();
            assert!(row.subtree_dirty() && sd.subtree_dirty() && doc.root().subtree_dirty());
            assert_eq!(row.dirty_child_count(), 1);
            assert!(doc.is_dirty());
        }
        let half = part(&mut wb);
        assert!(half.contains("<v>8</v>") && half.contains(r#"<c r="A1"><v>1</v></c>"#));
        wb.restore_cell(0, at("B1"), &b, RestoreStyle::Snapshot).unwrap();
        assert!(!doc_dirty(&mut wb));
        assert_eq!(part(&mut wb), xml);
    }

    #[test]
    fn restore_order_does_not_matter_for_the_flags() {
        let xml = sheet_xml();
        let mut wb = open(&xml);
        let a = wb.snapshot_cell(0, at("A1")).unwrap();
        write(&mut wb, "A1", CellOp::Number("7".into()));
        // B1's snapshot is taken while the row is already dirty
        let b = wb.snapshot_cell(0, at("B1")).unwrap();
        write(&mut wb, "B1", CellOp::Number("8".into()));
        wb.restore_cell(0, at("B1"), &b, RestoreStyle::Snapshot).unwrap();
        wb.restore_cell(0, at("A1"), &a, RestoreStyle::Snapshot).unwrap();
        assert_eq!(part(&mut wb), xml);
        assert!(!doc_dirty(&mut wb));
    }

    #[test]
    fn a_restore_does_not_hide_a_change_of_the_rows_own_tag() {
        let xml = sheet_xml();
        let mut wb = open(&xml);
        let a = wb.snapshot_cell(0, at("A1")).unwrap();
        write(&mut wb, "A1", CellOp::Number("7".into()));
        {
            let doc = wb.pkg.xml_part_mut(PART).unwrap();
            let mut row = doc.lens_mut_at::<sml::Row>(&[1, 0]).unwrap();
            row.set_attr(sml::Row::ATTR_SPANS, "1:5").unwrap();
        }
        wb.restore_cell(0, at("A1"), &a, RestoreStyle::Snapshot).unwrap();
        assert!(doc_dirty(&mut wb));
        assert!(part(&mut wb).contains(r#"spans="1:5""#));
    }

    #[test]
    fn a_refusal_changes_nothing() {
        let bad = format!("{HEAD}<row r=\"2\"><c r=\"A2\"/></row><row r=\"1\"/>{TAIL}");
        let mut wb = open(&bad);
        let before = part(&mut wb);
        assert!(wb.snapshot_cell(0, at("A1")).is_err());
        let snap = CellSnapshot { node: None, formula: None, vm: false, index_entry: None, marks: [None, None, None] };
        assert!(wb.restore_cell(0, at("A1"), &snap, RestoreStyle::Snapshot).is_err());
        assert_eq!(part(&mut wb), before);
        assert!(!doc_dirty(&mut wb));
    }

    #[test]
    fn a_removed_sibling_of_an_ancestor_is_not_brought_back() {
        // B1: a barrier op between the write and the restore removed an element
        for (tail, remove) in [
            (r#"<sheetProtection sheet="1"/>"#, 0),
            (r#"<mergeCells count="1"><mergeCell ref="C3:D3"/></mergeCells>"#, 1),
        ] {
            let xml = format!("{HEAD}<row r=\"1\"><c r=\"A1\"><v>1</v></c></row></sheetData>{tail}</worksheet>");
            for snapshot_late in [false, true] {
                let mut wb = open(&xml);
                let early = wb.snapshot_cell(0, at("A1")).unwrap();
                if snapshot_late {
                    // the removal comes first; the snapshot sees a root that is dirty for another child
                    write(&mut wb, "B1", CellOp::Number("5".into()));
                }
                let snap = if snapshot_late { wb.snapshot_cell(0, at("A1")).unwrap() } else { early };
                write(&mut wb, "A1", CellOp::Number("7".into()));
                let mut ws = wb.worksheet_by_index(0).unwrap();
                if remove == 0 {
                    assert!(ws.unprotect_sheet().unwrap());
                } else {
                    ws.unmerge("C3:D3").unwrap();
                }
                let without = part(&mut wb);
                assert!(!without.contains("sheetProtection") && !without.contains("<mergeCell "), "{without}");
                wb.restore_cell(0, at("A1"), &snap, RestoreStyle::Snapshot).unwrap();
                let out = part(&mut wb);
                assert!(!out.contains("sheetProtection") && !out.contains("<mergeCell "), "late={snapshot_late}: {out}");
                assert!(doc_dirty(&mut wb));
            }
        }
    }

    #[test]
    fn a_session_row_goes_whichever_snapshot_saw_it() {
        // M1: F6's snapshot saw no row 6, A6's snapshot saw the one F6 created
        for a_first in [true, false] {
            let xml = sheet_xml();
            let mut wb = open(&xml);
            let f6 = wb.snapshot_cell(0, at("F6")).unwrap();
            write(&mut wb, "F6", CellOp::Number("1".into()));
            let a6 = wb.snapshot_cell(0, at("A6")).unwrap();
            write(&mut wb, "A6", CellOp::Number("2".into()));
            if a_first {
                wb.restore_cell(0, at("A6"), &a6, RestoreStyle::Snapshot).unwrap();
                wb.restore_cell(0, at("F6"), &f6, RestoreStyle::Snapshot).unwrap();
            } else {
                wb.restore_cell(0, at("F6"), &f6, RestoreStyle::Snapshot).unwrap();
                wb.restore_cell(0, at("A6"), &a6, RestoreStyle::Snapshot).unwrap();
            }
            assert_eq!(part(&mut wb), xml, "a_first={a_first}");
            assert!(!doc_dirty(&mut wb), "a_first={a_first}");
        }
    }

    #[test]
    fn a_restore_resets_the_everything_closure() {
        // m1: INDIRECT depends on everything; once restored, dependents are as if never written
        let xml = format!("{HEAD}<row r=\"1\"><c r=\"A1\"><v>1</v></c><c r=\"B1\"><f>A1+1</f><v>2</v></c></row>{TAIL}");
        let mut wb = open(&xml);
        let baseline = wb.compute_dependents(0, at("C5")).cells;
        assert!(baseline.is_empty());
        let snap = wb.snapshot_cell(0, at("A1")).unwrap();
        write(&mut wb, "A1", CellOp::Formula("INDIRECT(\"Z9\")".into()));
        assert!(!wb.compute_dependents(0, at("C5")).cells.is_empty());
        wb.restore_cell(0, at("A1"), &snap, RestoreStyle::Snapshot).unwrap();
        assert!(wb.compute_dependents(0, at("C5")).cells.is_empty());
    }

    #[test]
    fn a_refused_row_creation_changes_nothing() {
        // m2: the snapshot needs a `sheetData` the sheet no longer has; the refusal comes before any change
        let mut wb = open(&sheet_xml());
        let snap = wb.snapshot_cell(0, at("A1")).unwrap();
        let mut other = open(&format!("{HEAD}{TAIL}").replace("<sheetData></sheetData>", ""));
        let before = part(&mut other);
        assert!(other.restore_cell(0, at("A1"), &snap, RestoreStyle::Snapshot).is_err());
        assert_eq!(part(&mut other), before);
        assert!(!doc_dirty(&mut other));
        let _ = &mut wb;
    }

    #[test]
    fn a_snapshot_taken_after_the_removal_does_not_bring_it_back() {
        // round 2 m2: C1 is snapshotted and written, the barrier op removes `sheetProtection`,
        // then A1 is snapshotted (the root is dirty only for `sheetData`) and written
        let xml = format!("{HEAD}<row r=\"1\"><c r=\"A1\"><v>1</v></c><c r=\"C1\"><v>3</v></c></row></sheetData><sheetProtection sheet=\"1\"/></worksheet>");
        let mut wb = open(&xml);
        let c1 = wb.snapshot_cell(0, at("C1")).unwrap();
        write(&mut wb, "C1", CellOp::Number("9".into()));
        assert!(wb.worksheet_by_index(0).unwrap().unprotect_sheet().unwrap());
        let a1 = wb.snapshot_cell(0, at("A1")).unwrap();
        write(&mut wb, "A1", CellOp::Number("7".into()));
        wb.restore_cell(0, at("C1"), &c1, RestoreStyle::Snapshot).unwrap();
        wb.restore_cell(0, at("A1"), &a1, RestoreStyle::Snapshot).unwrap();
        let out = part(&mut wb);
        assert!(!out.contains("sheetProtection"), "{out}");
        assert!(doc_dirty(&mut wb));
    }

    // sst/@count and style_sensitive under restore

    const SST: &str = "xl/sharedStrings.xml";

    fn shared_book() -> Workbook {
        let sheet = format!(
            "{HEAD}{}{TAIL}",
            r#"<row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>0</v></c><c r="C1"><f>A2+1</f><v>2</v></c></row><row r="2"><c r="A2"><v>1</v></c><c r="B2" t="s"><v>0</v></c></row>"#
        );
        let sst = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="3" uniqueCount="1"><si><t>zero</t></si></sst>"#;
        let mut pkg = Package::from_bytes(TEMPLATE).unwrap();
        pkg.set_part(PART, sheet.into_bytes()).unwrap();
        pkg.set_part(SST, sst.as_bytes().to_vec()).unwrap();
        let mut wb = Workbook::from_bytes(pkg.to_bytes().unwrap()).unwrap();
        wb.policy = CalcPolicy::KeepStaleCaches;
        assert!(wb.shared_strings_active().unwrap());
        wb
    }

    fn sst_count(wb: &mut Workbook) -> (String, usize) {
        let sst = wb.pkg.xml_part(SST).unwrap().serialize();
        let n = crate::registry::scan_sheet(part(wb).as_bytes()).unwrap().shared as usize;
        (sst, n)
    }

    /// A shared-string cell turned into a number and back, a number turned
    /// into a shared string and back, and a missing cell: the written
    /// `count` is the recount, with or without a `to_bytes` in between.
    fn count_run(with_to_bytes: bool) -> (Vec<u8>, usize, String) {
        let mut wb = shared_book();
        let (a1, a2, c3) = (at("A1"), at("A2"), at("C3"));
        let snaps = [a1, a2, c3].map(|c| wb.snapshot_cell(0, c).unwrap());
        write(&mut wb, "A1", CellOp::Number("5".into()));
        write(&mut wb, "A2", CellOp::Shared(0));
        write(&mut wb, "C3", CellOp::Shared(0));
        if with_to_bytes {
            wb.to_bytes().unwrap();
        }
        for (c, snap) in [a1, a2, c3].into_iter().zip(&snaps) {
            wb.restore_cell(0, c, snap, RestoreStyle::Snapshot).unwrap();
            if with_to_bytes {
                wb.to_bytes().unwrap();
            }
        }
        let (sst, n) = sst_count(&mut wb);
        (wb.to_bytes().unwrap(), n, sst)
    }

    #[test]
    fn the_shared_string_count_follows_restores_whether_or_not_to_bytes_ran() {
        let (a, n, sst) = count_run(false);
        let (b, m, _) = count_run(true);
        assert_eq!(a, b, "byte-identical");
        assert_eq!((n, m), (3, 3));
        assert!(sst.contains(r#"count="3""#), "{sst}");
        // and a reopened copy (which recounts) writes the same
        let re = Workbook::from_bytes(a.clone()).unwrap();
        assert_eq!(re.to_bytes().unwrap(), a);
    }

    #[test]
    fn a_content_restore_adjusts_the_count_too() {
        let mut wb = shared_book();
        let snap = wb.snapshot_cell(0, at("A1")).unwrap();
        write(&mut wb, "A1", CellOp::Number("5".into()));
        assert!(sst_count(&mut wb).0.contains(r#"count="2""#));
        wb.restore_cell(0, at("A1"), &snap, RestoreStyle::Current).unwrap();
        let (sst, n) = sst_count(&mut wb);
        assert!(sst.contains(&format!(r#"count="{n}""#)), "{sst}");
        // a missing-cell content restore of a shared cell removes the reference
        let missing = wb.snapshot_cell(0, at("C3")).unwrap();
        write(&mut wb, "C3", CellOp::Shared(0));
        wb.restore_cell(0, at("C3"), &missing, RestoreStyle::Current).unwrap();
        let (sst, n) = sst_count(&mut wb);
        assert!(sst.contains(&format!(r#"count="{n}""#)), "{sst}");
    }

    #[test]
    fn style_sensitive_survives_a_restore_that_keeps_the_formula() {
        let mut wb = shared_book();
        let snap = wb.snapshot_cell(0, at("A2")).unwrap();
        write(&mut wb, "A2", CellOp::Number("9".into()));
        assert!(!wb.style_sensitive().unwrap());
        wb.restore_cell(0, at("A2"), &snap, RestoreStyle::Snapshot).unwrap();
        assert_eq!(wb.style_sensitive, Some(false), "a value-only restore keeps the cache");
        // a formula cell restored over itself
        let snap = wb.snapshot_cell(0, at("C1")).unwrap();
        wb.restore_cell(0, at("C1"), &snap, RestoreStyle::Snapshot).unwrap();
        assert_eq!(wb.style_sensitive, Some(false));
    }

    #[test]
    fn style_sensitive_is_dropped_when_the_formula_changes() {
        let mut wb = shared_book();
        // formula text changes
        let snap = wb.snapshot_cell(0, at("C1")).unwrap();
        write(&mut wb, "C1", CellOp::Formula(r#"CELL("format",A2)"#.into()));
        assert!(wb.style_sensitive().unwrap());
        wb.restore_cell(0, at("C1"), &snap, RestoreStyle::Snapshot).unwrap();
        assert_eq!(wb.style_sensitive, None, "formula text differs");
        assert!(!wb.style_sensitive().unwrap());
        // a formula comes into being (A2 had none)
        let snap = wb.snapshot_cell(0, at("A2")).unwrap();
        write(&mut wb, "A2", CellOp::Formula("INFO(\"x\")".into()));
        assert!(wb.style_sensitive().unwrap());
        wb.restore_cell(0, at("A2"), &snap, RestoreStyle::Snapshot).unwrap();
        assert_eq!(wb.style_sensitive, None, "presence differs");
        // a formula goes away
        let snap = wb.snapshot_cell(0, at("C1")).unwrap();
        write(&mut wb, "C1", CellOp::Number("1".into()));
        assert!(!wb.style_sensitive().unwrap());
        wb.restore_cell(0, at("C1"), &snap, RestoreStyle::Snapshot).unwrap();
        assert_eq!(wb.style_sensitive, None);
    }
}
