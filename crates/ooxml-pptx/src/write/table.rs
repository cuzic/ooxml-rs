//! Phase 1b (ADR-0013 §2, §3): table structure, cell properties, borders
//! and the table style.
//!
//! Every operation first parses the table into a grid model and checks it
//! (well-formedness, merge rules, comment and animation locks, format
//! copies, depth), then applies its edits to a **clone** of the slide tree
//! and installs the clone only when every edit succeeded: a refusal — ours
//! or the core's — leaves the slide untouched.

use std::collections::{BTreeMap, BTreeSet};

use ooxml_core::lens::NsDecl;
use ooxml_core::{Detached, ElementSpec, Lens, LensMut, QName, RawNode, XmlDocument};
use ooxml_schema::dml::{
    self, BodyProperties, BottomBorderLineProperties, ColIdIdentifier, EndParagraphRunProperties, Extension,
    ExtensionList, GridColumn, LeftBorderLineProperties, ListStyle, NoFill, Paragraph, ParagraphProperties,
    RgbColorModelHex, RightBorderLineProperties, RowIdIdentifier, SchemeColor, SolidFill, Table, TableCell,
    TableCellProperties, TableGrid, TableProperties, TableRow, TableStyleId, TopBorderLineProperties,
};
use ooxml_schema::pml;

use super::{CheckInsert, Cursor, Guard, LINKS, check_copies, check_depth, detach, node_at};
use crate::error::{Error, Result};

/// `a:ext@uri` of `a16:rowId` / `a16:colId` (ADR-0013 §2).
const ROW_ID_URI: &str = "{0D108BD9-81ED-4DB2-BD59-A6C34878D82A}";
const COL_ID_URI: &str = "{9D8B030D-6E8A-4147-A177-3AD203B41FA5}";
const A16_NS: &str = "http://schemas.microsoft.com/office/drawing/2014/main";

/// Ids of rows and columns deleted from one table in this session
/// (ADR-0013 §2: a new id is never one of them).
#[derive(Clone, Debug, Default)]
pub(crate) struct DeletedIds {
    rows: BTreeSet<u32>,
    cols: BTreeSet<u32>,
}

/// Per-slide-part state of table edits that lives as long as the
/// `Presentation`: deleted ids by graphic frame id.
#[derive(Clone, Debug, Default)]
pub(crate) struct TableSession {
    pub(crate) deleted: BTreeMap<u32, DeletedIds>,
}

// ---------------------------------------------------------------------------
// The grid model

/// A cell's merge attributes (`gridSpan`/`rowSpan` default 1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Attrs {
    gs: u32,
    rs: u32,
    hm: bool,
    vm: bool,
}

const PLAIN: Attrs = Attrs { gs: 1, rs: 1, hm: false, vm: false };

/// A merged rectangle: origin, extent, and whether it is written in the
/// edge form (covered edge cells carry the spans; ADR-0013 §2 review B1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Merge {
    r0: usize,
    c0: usize,
    nr: usize,
    nc: usize,
    edge: bool,
}

impl Merge {
    fn contains(&self, r: usize, c: usize) -> bool {
        (self.r0..self.r0 + self.nr).contains(&r) && (self.c0..self.c0 + self.nc).contains(&c)
    }
    fn two_d(&self) -> bool {
        self.nr > 1 && self.nc > 1
    }
    /// The canonical attributes of (r, c) in this merge, in the edge form.
    fn canon(&self, r: usize, c: usize) -> Attrs {
        let span = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
        if self.nr == 1 && self.nc == 1 {
            return PLAIN;
        }
        match (r == self.r0, c == self.c0) {
            (true, true) => Attrs { gs: span(self.nc), rs: span(self.nr), hm: false, vm: false },
            (true, false) => Attrs { gs: 1, rs: if self.two_d() { span(self.nr) } else { 1 }, hm: true, vm: false },
            (false, true) => Attrs { gs: if self.two_d() { span(self.nc) } else { 1 }, rs: 1, hm: false, vm: true },
            (false, false) => Attrs { gs: 1, rs: 1, hm: true, vm: true },
        }
    }
}

#[derive(Clone, Debug)]
struct Grid {
    nrows: usize,
    ncols: usize,
    cells: Vec<Vec<Attrs>>,
    merges: Vec<Merge>,
}

fn bad(msg: impl Into<String>) -> Error {
    core::hint::cold_path();
    Error::TableNotWellFormed(msg.into())
}

fn parse_span(n: &RawNode, local: &'static str) -> Result<u32> {
    match n.attr(QName::new("", local)) {
        None => Ok(1),
        Some(v) => v.trim().parse::<u32>().ok().filter(|&x| x >= 1).ok_or_else(|| bad(format!("{local}=\"{v}\""))),
    }
}

fn parse_flag(n: &RawNode, local: &'static str) -> Result<bool> {
    match n.attr(QName::new("", local)).map(str::trim) {
        None | Some("0" | "false") => Ok(false),
        Some("1" | "true") => Ok(true),
        Some(v) => Err(bad(format!("{local}=\"{v}\""))),
    }
}

fn cell_attrs(tc: &RawNode) -> Result<Attrs> {
    Ok(Attrs { gs: parse_span(tc, "gridSpan")?, rs: parse_span(tc, "rowSpan")?, hm: parse_flag(tc, "hMerge")?, vm: parse_flag(tc, "vMerge")? })
}

fn rows_of(tbl: &RawNode) -> Vec<(usize, &RawNode)> {
    tbl.indexed_elements().filter(|(_, e)| e.is(TableRow::QNAME)).collect()
}

fn cells_of(tr: &RawNode) -> Vec<(usize, &RawNode)> {
    tr.indexed_elements().filter(|(_, e)| e.is(TableCell::QNAME)).collect()
}

fn grid_of(tbl: &RawNode) -> Option<(usize, &RawNode)> {
    tbl.indexed_elements().find(|(_, e)| e.is(TableGrid::QNAME))
}

fn cols_of(tbl: &RawNode) -> Vec<(usize, &RawNode)> {
    grid_of(tbl).map_or_else(Vec::new, |(_, g)| g.indexed_elements().filter(|(_, e)| e.is(GridColumn::QNAME)).collect())
}

/// Parses and checks the table (ADR-0013 §2 "Well-formed").
fn parse_grid(tbl: &RawNode) -> Result<Grid> {
    let ncols = cols_of(tbl).len();
    let rows = rows_of(tbl);
    let nrows = rows.len();
    if ncols == 0 || nrows == 0 {
        return Err(bad("no rows or no grid columns"));
    }
    let mut cells = Vec::with_capacity(nrows);
    for (r, (_, tr)) in rows.iter().enumerate() {
        let row = cells_of(tr);
        if row.len() != ncols {
            return Err(bad(format!("row {r} has {} cells for {ncols} grid columns", row.len())));
        }
        cells.push(row.iter().map(|(_, tc)| cell_attrs(tc)).collect::<Result<Vec<_>>>()?);
    }
    let mut owner: Vec<Vec<Option<usize>>> = vec![vec![None; ncols]; nrows];
    let mut merges = Vec::new();
    for r in 0..nrows {
        for c in 0..ncols {
            let a = cells[r][c];
            if a.hm || a.vm || (a.gs == 1 && a.rs == 1) {
                continue;
            }
            let (nr, nc) = (a.rs as usize, a.gs as usize);
            if r + nr > nrows || c + nc > ncols {
                return Err(bad(format!("the merge at ({r}, {c}) leaves the grid")));
            }
            let k = merges.len();
            let mut m = Merge { r0: r, c0: c, nr, nc, edge: true };
            // edge spans: all present (edge form) or all absent (flag-only)
            let (mut present, mut absent) = (0, 0);
            for rr in r..r + nr {
                for cc in c..c + nc {
                    if owner[rr][cc].is_some() {
                        return Err(bad(format!("({rr}, {cc}) is covered by two merges")));
                    }
                    owner[rr][cc] = Some(k);
                    if (rr, cc) == (r, c) {
                        continue;
                    }
                    let x = cells[rr][cc];
                    if x.hm != (cc != c) || x.vm != (rr != r) {
                        return Err(bad(format!("({rr}, {cc}) has the wrong merge flags")));
                    }
                    let (span, other) = match (rr == r, cc == c) {
                        (true, false) => (x.rs, x.gs),
                        (false, true) => (x.gs, x.rs),
                        _ => (1, x.gs.max(x.rs)),
                    };
                    if other != 1 {
                        return Err(bad(format!("({rr}, {cc}) carries a span it cannot have")));
                    }
                    if rr != r && cc != c {
                        continue;
                    }
                    let want = if rr == r { nr } else { nc } as u32;
                    if !m.two_d() {
                        if span != 1 {
                            return Err(bad(format!("({rr}, {cc}) carries a span in a one-way merge")));
                        }
                    } else if span == want {
                        present += 1;
                    } else if span == 1 {
                        absent += 1;
                    } else {
                        return Err(bad(format!("({rr}, {cc}) carries a span other than its merge's")));
                    }
                }
            }
            if present > 0 && absent > 0 {
                return Err(bad(format!("the merge at ({r}, {c}) mixes edge spans and flag-only cells")));
            }
            m.edge = absent == 0;
            merges.push(m);
        }
    }
    for (r, row) in owner.iter().enumerate() {
        for (c, o) in row.iter().enumerate() {
            let a = cells[r][c];
            if o.is_none() && (a.hm || a.vm) {
                return Err(bad(format!("({r}, {c}) is flagged as covered but no merge covers it")));
            }
        }
    }
    Ok(Grid { nrows, ncols, cells, merges })
}

impl Grid {
    fn merge_at(&self, r: usize, c: usize) -> Option<&Merge> {
        self.merges.iter().find(|m| m.contains(r, c))
    }

    /// Target attributes after a structural change: `old(r, c)` maps a new
    /// cell to the old one it was (None: created), `merges` are the new
    /// merges, `touched` those whose cells are rewritten in the edge form.
    fn retarget(&self, nrows: usize, ncols: usize, old: impl Fn(usize, usize) -> Option<(usize, usize)>, merges: Vec<Merge>, touched: &[usize]) -> Grid {
        let mut cells = vec![vec![PLAIN; ncols]; nrows];
        for (r, row) in cells.iter_mut().enumerate() {
            for (c, a) in row.iter_mut().enumerate() {
                if let Some((or, oc)) = old(r, c) {
                    *a = self.cells[or][oc];
                }
            }
        }
        let mut merges = merges;
        for &k in touched {
            // `canon` writes the edge form: a touched flag-only merge is
            // converted (ADR-0013 §2)
            let m = merges[k];
            for (r, row) in cells.iter_mut().enumerate().skip(m.r0).take(m.nr) {
                for (c, a) in row.iter_mut().enumerate().skip(m.c0).take(m.nc) {
                    *a = m.canon(r, c);
                }
            }
        }
        merges.retain(|m| m.nr > 1 || m.nc > 1);
        Grid { nrows, ncols, cells, merges }
    }
}

// ---------------------------------------------------------------------------
// Public value types

/// A cell edge (ADR-0013 §3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    /// `a:lnL`.
    Left,
    /// `a:lnR`.
    Right,
    /// `a:lnT`.
    Top,
    /// `a:lnB`.
    Bottom,
}

/// A colour to write to a table cell fill or border.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TableColor {
    /// `a:srgbClr`.
    Rgb([u8; 3]),
    /// `a:schemeClr` (`tx1`, `bg1`, `accent1`, …).
    Scheme(String),
}

/// A cell border (ADR-0013 §3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Line {
    /// A solid line: width in EMU (`ST_LineWidth`, 0..=20116800) and colour.
    Solid {
        /// Width (EMU).
        width: i64,
        /// Colour.
        color: TableColor,
    },
    /// No line (`a:noFill`).
    None,
}

/// A cell fill (ADR-0013 §3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CellFill {
    /// `a:solidFill`.
    Solid(TableColor),
    /// `a:noFill`.
    None,
}

/// A `a:tblPr` flag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableFlag {
    /// `firstRow`.
    FirstRow,
    /// `firstCol`.
    FirstColumn,
    /// `lastRow`.
    LastRow,
    /// `lastCol`.
    LastColumn,
    /// `bandRow`.
    BandRows,
    /// `bandCol`.
    BandColumns,
}

/// A cell margin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    /// `marL`.
    Left,
    /// `marR`.
    Right,
    /// `marT`.
    Top,
    /// `marB`.
    Bottom,
}

const SCHEME: [&str; 17] = [
    "bg1", "tx1", "bg2", "tx2", "accent1", "accent2", "accent3", "accent4", "accent5", "accent6", "hlink", "folHlink", "phClr", "dk1",
    "lt1", "dk2", "lt2",
];

fn check_color(c: &TableColor) -> Result<()> {
    match c {
        TableColor::Scheme(s) if !SCHEME.contains(&s.as_str()) => Err(Error::ValueOutOfRange { what: "schemeClr@val" }),
        _ => Ok(()),
    }
}

// ---------------------------------------------------------------------------
// TableMut

/// A table (`a:tbl` in a graphic frame) opened for structural and property
/// edits (ADR-0013 §2, §3). Cell text is edited through
/// [`crate::ShapeMut::table_cell_mut`].
#[derive(Debug)]
pub struct TableMut<'a> {
    pub(super) doc: &'a mut XmlDocument,
    pub(super) frame: Vec<usize>,
    pub(super) tbl: Vec<usize>,
    pub(super) guard: Guard,
    pub(super) styles: &'a [String],
    pub(super) deleted: &'a mut DeletedIds,
}

/// A typed lens on the table, walking from the slide root.
fn tbl_lens<'d>(doc: &'d mut XmlDocument, path: &[usize]) -> Result<LensMut<'d, Table>> {
    match walk(doc, path)? {
        Cursor::Tbl(l) => Ok(l),
        _ => Err(Error::NotATable),
    }
}

fn frame_lens<'d>(doc: &'d mut XmlDocument, path: &[usize]) -> Result<LensMut<'d, pml::GraphicFrame>> {
    match walk(doc, path)? {
        Cursor::Frame(l) => Ok(l),
        _ => Err(Error::NotATable),
    }
}

fn walk<'d>(doc: &'d mut XmlDocument, path: &[usize]) -> Result<Cursor<'d>> {
    let mut cur = Cursor::Sld(doc.lens_mut::<pml::Slide>()?);
    for &i in path {
        cur = cur.step(i).ok_or(Error::NotATable)?;
    }
    Ok(cur)
}

fn set_or_remove(l: &mut LensMut<'_, TableCell>, local: &'static str, value: Option<String>) -> Result<()> {
    let q = QName::new("", local);
    match value {
        Some(v) => l.set_attr(q, &v)?,
        None => {
            l.remove_attr(q)?;
        }
    }
    Ok(())
}

/// Writes `want` over `have` on a cell, touching only what differs.
fn write_attrs(l: &mut LensMut<'_, TableCell>, have: Attrs, want: Attrs) -> Result<()> {
    if have.gs != want.gs {
        set_or_remove(l, "gridSpan", (want.gs != 1).then(|| want.gs.to_string()))?;
    }
    if have.rs != want.rs {
        set_or_remove(l, "rowSpan", (want.rs != 1).then(|| want.rs.to_string()))?;
    }
    if have.hm != want.hm {
        set_or_remove(l, "hMerge", want.hm.then(|| "1".to_owned()))?;
    }
    if have.vm != want.vm {
        set_or_remove(l, "vMerge", want.vm.then(|| "1".to_owned()))?;
    }
    Ok(())
}

/// The `a16` id of a row or grid column, if any (`Err` if malformed).
fn id_of(n: &RawNode, uri: &str, local: &'static str) -> Result<Option<u32>> {
    let found = n
        .elements()
        .filter(|e| e.is(ExtensionList::QNAME))
        .flat_map(|l| l.elements())
        .filter(|e| e.is(Extension::QNAME) && e.attr(QName::new("", "uri")).is_some_and(|u| u.eq_ignore_ascii_case(uri)))
        .flat_map(|e| e.elements())
        .find(|e| e.is(QName::new(A16_NS, local)));
    match found {
        None => Ok(None),
        Some(e) => {
            e.attr(QName::new("", "val")).and_then(|v| v.trim().parse::<u32>().ok()).map(Some).ok_or_else(|| bad(format!("a16:{local} without a numeric val")))
        }
    }
}

/// The ids of all rows (or columns): `None` when none has one; refused when
/// only some have one (ADR-0013 §2).
fn ids(nodes: &[(usize, &RawNode)], uri: &str, local: &'static str) -> Result<Option<Vec<u32>>> {
    let v: Vec<Option<u32>> = nodes.iter().map(|(_, n)| id_of(n, uri, local)).collect::<Result<_>>()?;
    match v.iter().filter(|x| x.is_some()).count() {
        0 => Ok(None),
        k if k == v.len() => {
            let ids: Vec<u32> = v.into_iter().flatten().collect();
            let mut sorted = ids.clone();
            sorted.sort_unstable();
            if sorted.windows(2).any(|w| w[0] == w[1]) {
                return Err(bad(format!("duplicate a16:{local}")));
            }
            Ok(Some(ids))
        }
        _ => Err(bad(format!("only some rows/columns carry a16:{local}"))),
    }
}

/// A new id: unused and not deleted in this session; `max + 1` when that
/// fits, else the smallest unused value ≥ 1.
fn new_id(used: &[u32], deleted: &BTreeSet<u32>) -> Result<u32> {
    let taken = |x: u32| used.contains(&x) || deleted.contains(&x);
    let max = used.iter().chain(deleted).copied().max().unwrap_or(0);
    if let Some(n) = max.checked_add(1) {
        return Ok(n);
    }
    (1..=u32::MAX).find(|&x| !taken(x)).ok_or(Error::TableNotWellFormed("no free a16 id".into()))
}

/// A covered cell whose paragraphs move to a merge's origin, and the format
/// of the one empty paragraph it keeps.
type KeptParagraph = ((usize, usize), Option<Detached<ParagraphProperties>>, Option<Detached<EndParagraphRunProperties>>);

/// Formatting copied into a new cell (ADR-0013 §2: `a:tcPr`, `a:bodyPr`,
/// `a:lstStyle`, the first paragraph's `a:pPr`/`a:endParaRPr`).
struct CellFormat {
    tcpr: Option<Detached<TableCellProperties>>,
    body: Option<Detached<BodyProperties>>,
    lst: Option<Detached<ListStyle>>,
    ppr: Option<Detached<ParagraphProperties>>,
    end: Option<Detached<EndParagraphRunProperties>>,
}

fn find_child(n: &RawNode, q: QName) -> Option<usize> {
    n.indexed_elements().find(|(_, e)| e.is(q)).map(|(i, _)| i)
}

fn sub(path: &[usize], more: &[usize]) -> Vec<usize> {
    let mut p = path.to_vec();
    p.extend_from_slice(more);
    p
}

impl TableMut<'_> {
    fn tbl_node(&self) -> &RawNode {
        node_at(self.doc.root(), &self.tbl).expect("table path")
    }

    fn grid(&self) -> Result<Grid> {
        parse_grid(self.tbl_node())
    }

    /// Structural edits are refused on a comment-anchored table and on
    /// animated text (ADR-0013 §2 "Comment anchors").
    fn structural(&self) -> Result<()> {
        if self.guard.table {
            return Err(Error::CommentAnchoredTable { shape_id: self.guard.shape_id });
        }
        Ok(())
    }

    /// Absolute path of cell (r, c).
    fn cell_path(&self, r: usize, c: usize) -> Option<Vec<usize>> {
        let tbl = self.tbl_node();
        let (ri, tr) = *rows_of(tbl).get(r)?;
        let (ci, _) = *cells_of(tr).get(c)?;
        Some(sub(&self.tbl, &[ri, ci]))
    }

    fn format_of(&self, r: usize, c: usize) -> Result<CellFormat> {
        let root = self.doc.root();
        let path = self.cell_path(r, c).ok_or(Error::CellNotFound { row: r, col: c })?;
        let tc = node_at(root, &path).expect("cell");
        let at = |n: &RawNode, base: &[usize], q: QName| find_child(n, q).map(|i| sub(base, &[i]));
        let tcpr = detach::<TableCellProperties, _>(root, at(tc, &path, TableCellProperties::QNAME), &[], "a:tcPr")?;
        let (mut body, mut lst, mut ppr, mut end) = (None, None, None, None);
        if let Some(bi) = find_child(tc, dml::TextBody::QNAME) {
            let bpath = sub(&path, &[bi]);
            let b = node_at(root, &bpath).expect("body");
            body = detach::<BodyProperties, _>(root, at(b, &bpath, BodyProperties::QNAME), &[], "a:bodyPr")?;
            lst = detach::<ListStyle, _>(root, at(b, &bpath, ListStyle::QNAME), &[], "a:lstStyle")?;
            if let Some(pi) = find_child(b, Paragraph::QNAME) {
                let ppath = sub(&bpath, &[pi]);
                let p = node_at(root, &ppath).expect("paragraph");
                ppr = detach::<ParagraphProperties, _>(root, at(p, &ppath, ParagraphProperties::QNAME), &[], "a:pPr")?;
                end = detach::<EndParagraphRunProperties, _>(root, at(p, &ppath, EndParagraphRunProperties::QNAME), &LINKS, "a:endParaRPr")?;
            }
        }
        Ok(CellFormat { tcpr, body, lst, ppr, end })
    }

    /// Copies must fit below the table: `tr`/`tc` are created (2 levels),
    /// then `txBody` (3), `a:p` (4); ids add `extLst/ext/a16:*` (4).
    fn check_formats(&self, fmts: &[CellFormat], extra_level: usize) -> Result<()> {
        check_depth(self.doc, &self.tbl, 4 + extra_level)?;
        for f in fmts {
            let lvl = |n: usize| n + extra_level;
            check_copies(self.doc, &self.tbl, lvl(2), f.tcpr.iter().map(|d| d as &dyn CheckInsert))?;
            check_copies(self.doc, &self.tbl, lvl(3), f.body.iter().map(|d| d as &dyn CheckInsert).chain(f.lst.iter().map(|d| d as &dyn CheckInsert)))?;
            check_copies(self.doc, &self.tbl, lvl(4), f.ppr.iter().map(|d| d as &dyn CheckInsert).chain(f.end.iter().map(|d| d as &dyn CheckInsert)))?;
        }
        Ok(())
    }

    /// Runs `f` on a clone of the slide and installs it on success.
    fn edit(&mut self, f: impl FnOnce(&mut XmlDocument, &[usize], &[usize]) -> Result<()>) -> Result<()> {
        let mut work = self.doc.clone();
        f(&mut work, &self.frame, &self.tbl)?;
        *self.doc = work;
        Ok(())
    }

    /// Checks that the table is well-formed for structural edits
    /// (ADR-0013 §2; [`Error::TableNotWellFormed`]), changing nothing.
    pub fn check(&self) -> Result<()> {
        let tbl = self.tbl_node();
        self.grid()?;
        ids(&rows_of(tbl), ROW_ID_URI, "rowId")?;
        ids(&cols_of(tbl), COL_ID_URI, "colId")?;
        Ok(())
    }

    /// Number of rows and grid columns.
    pub fn size(&self) -> (usize, usize) {
        let tbl = self.tbl_node();
        (rows_of(tbl).len(), cols_of(tbl).len())
    }

    // ------------------------------------------------------------ rows

    /// Inserts a row before row `at` (`at` = row count appends). The new
    /// row copies the height of the row above (else below); each new cell
    /// copies the formatting of the cell above (else below). Inside a
    /// vertical merge the new cells join it (ADR-0013 §2).
    pub fn insert_row(&mut self, at: usize) -> Result<()> {
        self.structural()?;
        let g = self.grid()?;
        if at > g.nrows {
            return Err(Error::CellNotFound { row: at, col: 0 });
        }
        let src = if at > 0 { at - 1 } else { 0 };
        let fmts: Vec<CellFormat> = (0..g.ncols).map(|c| self.format_of(src, c)).collect::<Result<_>>()?;
        let tbl = self.tbl_node();
        let rows = rows_of(tbl);
        let row_ids = ids(&rows, ROW_ID_URI, "rowId")?;
        let h = rows[src].1.attr(QName::new("", "h")).and_then(|v| v.trim().parse::<i64>().ok()).unwrap_or(0);
        self.check_formats(&fmts, 0)?;
        let new_row_id = row_ids.as_ref().map(|v| new_id(v, &self.deleted.rows)).transpose()?;
        // merges the new row falls inside grow by one row
        let mut merges = g.merges.clone();
        let mut touched = Vec::new();
        for (k, m) in merges.iter_mut().enumerate() {
            if m.r0 < at && at < m.r0 + m.nr {
                m.nr += 1;
                touched.push(k);
            } else if m.r0 >= at {
                m.r0 += 1;
            }
        }
        let target = g.retarget(g.nrows + 1, g.ncols, |r, c| match r.cmp(&at) {
            std::cmp::Ordering::Less => Some((r, c)),
            std::cmp::Ordering::Equal => None,
            std::cmp::Ordering::Greater => Some((r - 1, c)),
        }, merges, &touched);
        self.edit(|doc, frame, tbl| {
            {
                let mut t = tbl_lens(doc, tbl)?;
                let mut tr = t.insert_by::<TableRow>(|k, _| k == at)?;
                tr.set_attr(QName::new("", "h"), &h.to_string())?;
                for f in &fmts {
                    fill_new_cell(&mut tr.insert::<TableCell>()?, f)?;
                }
                if let Some(id) = new_row_id {
                    add_id::<TableRow, RowIdIdentifier>(&mut tr, ROW_ID_URI, id)?;
                }
            }
            apply_attrs(doc, tbl, &target)?;
            grow_frame(doc, frame, 0, h)
        })?;
        Ok(())
    }

    /// Deletes row `r`. Refused for the only row and for a row holding the
    /// origin of a vertical merge (`MergeOriginInRow`: unmerge first).
    pub fn delete_row(&mut self, r: usize) -> Result<()> {
        self.structural()?;
        let g = self.grid()?;
        if r >= g.nrows {
            return Err(Error::CellNotFound { row: r, col: 0 });
        }
        if g.nrows == 1 {
            return Err(Error::LastRow);
        }
        if g.merges.iter().any(|m| m.r0 == r && m.nr > 1) {
            return Err(Error::MergeOriginInRow { row: r });
        }
        let tbl = self.tbl_node();
        let rows = rows_of(tbl);
        let row_ids = ids(&rows, ROW_ID_URI, "rowId")?;
        let (item, tr) = rows[r];
        let h = tr.attr(QName::new("", "h")).and_then(|v| v.trim().parse::<i64>().ok()).unwrap_or(0);
        let mut merges = g.merges.clone();
        let mut touched = Vec::new();
        for (k, m) in merges.iter_mut().enumerate() {
            if m.r0 < r && r < m.r0 + m.nr {
                m.nr -= 1;
                touched.push(k);
            } else if m.r0 > r {
                m.r0 -= 1;
            }
        }
        let target = g.retarget(g.nrows - 1, g.ncols, |rr, c| Some((if rr < r { rr } else { rr + 1 }, c)), merges, &touched);
        self.edit(|doc, frame, tbl| {
            tbl_lens(doc, tbl)?.remove_at(item);
            apply_attrs(doc, tbl, &target)?;
            grow_frame(doc, frame, 0, -h)
        })?;
        if let Some(v) = row_ids {
            self.deleted.rows.insert(v[r]);
        }
        Ok(())
    }

    // ------------------------------------------------------------ columns

    /// Inserts a grid column before column `at` (`at` = column count
    /// appends): width from the column left (else right), each new cell's
    /// formatting from its left (else right) neighbour; inside a
    /// horizontal merge the new cells join it.
    pub fn insert_column(&mut self, at: usize) -> Result<()> {
        self.structural()?;
        let g = self.grid()?;
        if at > g.ncols {
            return Err(Error::CellNotFound { row: 0, col: at });
        }
        let src = if at > 0 { at - 1 } else { 0 };
        let fmts: Vec<CellFormat> = (0..g.nrows).map(|r| self.format_of(r, src)).collect::<Result<_>>()?;
        let tbl = self.tbl_node();
        let cols = cols_of(tbl);
        let col_ids = ids(&cols, COL_ID_URI, "colId")?;
        let w = cols[src].1.attr(QName::new("", "w")).and_then(|v| v.trim().parse::<i64>().ok()).unwrap_or(0);
        // conservatively the same levels as a new row's cells
        self.check_formats(&fmts, 0)?;
        let new_col_id = col_ids.as_ref().map(|v| new_id(v, &self.deleted.cols)).transpose()?;
        let mut merges = g.merges.clone();
        let mut touched = Vec::new();
        for (k, m) in merges.iter_mut().enumerate() {
            if m.c0 < at && at < m.c0 + m.nc {
                m.nc += 1;
                touched.push(k);
            } else if m.c0 >= at {
                m.c0 += 1;
            }
        }
        let target = g.retarget(g.nrows, g.ncols + 1, |r, c| match c.cmp(&at) {
            std::cmp::Ordering::Less => Some((r, c)),
            std::cmp::Ordering::Equal => None,
            std::cmp::Ordering::Greater => Some((r, c - 1)),
        }, merges, &touched);
        self.edit(|doc, frame, tbl| {
            {
                let mut t = tbl_lens(doc, tbl)?;
                let mut grid = t.child_mut::<TableGrid>().ok_or(Error::NotATable)?;
                let mut gc = grid.insert_by::<GridColumn>(|k, _| k == at)?;
                gc.set_attr(QName::new("", "w"), &w.to_string())?;
                if let Some(id) = new_col_id {
                    add_id::<GridColumn, ColIdIdentifier>(&mut gc, COL_ID_URI, id)?;
                }
            }
            for (r, f) in fmts.iter().enumerate() {
                let mut t = tbl_lens(doc, tbl)?;
                let mut tr = t.nth_child_mut::<TableRow>(r).ok_or(Error::NotATable)?;
                fill_new_cell(&mut tr.insert_by::<TableCell>(|k, _| k == at)?, f)?;
            }
            apply_attrs(doc, tbl, &target)?;
            grow_frame(doc, frame, w, 0)
        })?;
        Ok(())
    }

    /// Deletes grid column `c` and its cells. Refused for the only column
    /// and for a column holding the origin of a horizontal merge.
    pub fn delete_column(&mut self, c: usize) -> Result<()> {
        self.structural()?;
        let g = self.grid()?;
        if c >= g.ncols {
            return Err(Error::CellNotFound { row: 0, col: c });
        }
        if g.ncols == 1 {
            return Err(Error::LastColumn);
        }
        if g.merges.iter().any(|m| m.c0 == c && m.nc > 1) {
            return Err(Error::MergeOriginInColumn { col: c });
        }
        let tbl = self.tbl_node();
        let cols = cols_of(tbl);
        let col_ids = ids(&cols, COL_ID_URI, "colId")?;
        let (gitem, gc) = cols[c];
        let w = gc.attr(QName::new("", "w")).and_then(|v| v.trim().parse::<i64>().ok()).unwrap_or(0);
        let cell_items: Vec<usize> = rows_of(tbl).iter().map(|(_, tr)| cells_of(tr)[c].0).collect();
        let mut merges = g.merges.clone();
        let mut touched = Vec::new();
        for (k, m) in merges.iter_mut().enumerate() {
            if m.c0 < c && c < m.c0 + m.nc {
                m.nc -= 1;
                touched.push(k);
            } else if m.c0 > c {
                m.c0 -= 1;
            }
        }
        let target = g.retarget(g.nrows, g.ncols - 1, |r, cc| Some((r, if cc < c { cc } else { cc + 1 })), merges, &touched);
        self.edit(|doc, frame, tbl| {
            {
                let mut t = tbl_lens(doc, tbl)?;
                t.child_mut::<TableGrid>().ok_or(Error::NotATable)?.remove_at(gitem);
            }
            for (r, &item) in cell_items.iter().enumerate() {
                let mut t = tbl_lens(doc, tbl)?;
                t.nth_child_mut::<TableRow>(r).ok_or(Error::NotATable)?.remove_at(item);
            }
            apply_attrs(doc, tbl, &target)?;
            grow_frame(doc, frame, -w, 0)
        })?;
        if let Some(v) = col_ids {
            self.deleted.cols.insert(v[c]);
        }
        Ok(())
    }

    // ------------------------------------------------------------ merge

    /// Merges the rectangle (r1, c1)–(r2, c2) (inclusive) in the edge form.
    /// The other cells' paragraphs move to the origin in row-major order
    /// (empty cells skipped; a single empty origin paragraph is replaced);
    /// each covered cell keeps one empty paragraph.
    pub fn merge(&mut self, r1: usize, c1: usize, r2: usize, c2: usize) -> Result<()> {
        self.structural()?;
        self.guard.structure()?;
        let g = self.grid()?;
        if r1 > r2 || c1 > c2 || r2 >= g.nrows || c2 >= g.ncols {
            return Err(Error::RangeOutsideGrid);
        }
        if r1 == r2 && c1 == c2 {
            return Err(Error::RangeOutsideGrid);
        }
        for r in r1..=r2 {
            for c in c1..=c2 {
                if g.merge_at(r, c).is_some() {
                    return Err(Error::RangeContainsMerge);
                }
            }
        }
        let root = self.doc.root();
        // paragraphs to move, and what each covered cell keeps
        let origin = self.cell_path(r1, c1).expect("in grid");
        // a scratch copy for the move checks (they need a `LensMut`); an
        // origin without a:txBody gets one there, and in the edit only when
        // something moves (review #57 m2)
        let mut probe = self.doc.clone();
        let origin_has_body = find_child(node_at(root, &origin).expect("cell"), dml::TextBody::QNAME).is_some();
        let origin_body = ensure_body(&mut probe, &self.tbl, r1, c1)?;
        let mut keep: Vec<KeptParagraph> = Vec::new();
        for r in r1..=r2 {
            for c in c1..=c2 {
                if (r, c) == (r1, c1) {
                    continue;
                }
                let p = self.cell_path(r, c).expect("in grid");
                let tc = node_at(root, &p).expect("cell");
                let Some(bi) = find_child(tc, dml::TextBody::QNAME) else { continue };
                let body = node_at(tc, &[bi]).expect("body");
                let empty = body.elements().filter(|e| e.is(Paragraph::QNAME)).all(para_is_empty);
                let first = body.indexed_elements().find(|(_, e)| e.is(Paragraph::QNAME)).map(|(i, _)| sub(&p, &[bi, i]));
                let fmt = |q: QName| first.as_ref().and_then(|fp| find_child(node_at(root, fp).expect("p"), q).map(|i| sub(fp, &[i])));
                let ppr = detach::<ParagraphProperties, _>(root, fmt(ParagraphProperties::QNAME), &[], "a:pPr")?;
                let end = detach::<EndParagraphRunProperties, _>(root, fmt(EndParagraphRunProperties::QNAME), &LINKS, "a:endParaRPr")?;
                check_copies(self.doc, &sub(&p, &[bi]), 1, ppr.iter().map(|d| d as &dyn CheckInsert).chain(end.iter().map(|d| d as &dyn CheckInsert)))?;
                if !empty {
                    // every paragraph must be movable to the origin's body
                    let paras: Vec<usize> = body.indexed_elements().filter(|(_, e)| e.is(Paragraph::QNAME)).map(|(i, _)| i).collect();
                    let dest = body_lens_at(&mut probe, &origin_body)?;
                    for i in paras {
                        let src = Lens::<Paragraph>::at(root, &sub(&p, &[bi, i]))?.move_source();
                        dest.check_move_in_by::<Paragraph>(&src, |_, _| false)?;
                    }
                    keep.push(((r, c), ppr, end));
                }
            }
        }
        let origin_single_empty = !origin_has_body || {
            let b = node_at(root, &origin_body).expect("body");
            let ps: Vec<&RawNode> = b.elements().filter(|e| e.is(Paragraph::QNAME)).collect();
            ps.len() == 1 && para_is_empty(ps[0])
        };
        let (nr, nc) = (r2 - r1 + 1, c2 - c1 + 1);
        let mut merges = g.merges.clone();
        merges.push(Merge { r0: r1, c0: c1, nr, nc, edge: true });
        let k = merges.len() - 1;
        let target = g.retarget(g.nrows, g.ncols, |r, c| Some((r, c)), merges, &[k]);
        let tbl_path = self.tbl.clone();
        self.edit(|doc, frame, tbl| {
            let _ = frame;
            if !keep.is_empty() {
                ensure_body(doc, tbl, r1, c1)?;
            }
            let mut moved_any = false;
            for (rc, ppr, end) in &keep {
                let cell = cell_path_in(doc.root(), tbl, rc.0, rc.1).expect("cell");
                let bi = find_child(node_at(doc.root(), &cell).expect("cell"), dml::TextBody::QNAME).expect("body");
                let body_path = sub(&cell, &[bi]);
                loop {
                    let first = node_at(doc.root(), &body_path).and_then(|b| b.indexed_elements().find(|(_, e)| e.is(Paragraph::QNAME)).map(|(i, _)| i));
                    let Some(i) = first else { break };
                    let src = Lens::<Paragraph>::at(doc.root(), &sub(&body_path, &[i]))?.move_source();
                    body_lens_at(doc, &origin_body)?.move_in_by::<Paragraph>(&src, |_, _| false)?;
                    moved_any = true;
                }
                let mut b = body_lens_at(doc, &body_path)?;
                let mut p = b.insert::<Paragraph>()?;
                if let Some(f) = ppr {
                    p.insert_detached(f)?;
                }
                if let Some(f) = end {
                    p.insert_detached(f)?;
                }
            }
            if moved_any && origin_single_empty {
                let b = node_at(doc.root(), &origin_body).expect("body");
                let first = b.indexed_elements().find(|(_, e)| e.is(Paragraph::QNAME)).map(|(i, _)| i).expect("paragraph");
                body_lens_at(doc, &origin_body)?.remove_at(first);
            }
            apply_attrs(doc, &tbl_path, &target)
        })
    }

    /// Unmerges the merge whose origin is (r, c): spans and flags removed,
    /// text stays in the origin.
    pub fn unmerge(&mut self, r: usize, c: usize) -> Result<()> {
        self.structural()?;
        let g = self.grid()?;
        let k = g.merges.iter().position(|m| m.r0 == r && m.c0 == c).ok_or(Error::NotAMergeOrigin { row: r, col: c })?;
        let mut merges = g.merges.clone();
        merges[k].nr = 1;
        merges[k].nc = 1;
        let m = g.merges[k];
        let mut target = g.retarget(g.nrows, g.ncols, |r, c| Some((r, c)), merges, &[]);
        for row in target.cells.iter_mut().skip(m.r0).take(m.nr) {
            for a in row.iter_mut().skip(m.c0).take(m.nc) {
                *a = PLAIN;
            }
        }
        self.edit(|doc, _, tbl| apply_attrs(doc, tbl, &target))
    }

    // ------------------------------------------------------------ sizes

    /// Sets `a:gridCol@w` (EMU ≥ 0); the frame width follows.
    pub fn set_column_width(&mut self, c: usize, w: i64) -> Result<()> {
        if w < 0 {
            return Err(Error::ValueOutOfRange { what: "gridCol@w" });
        }
        let cols = cols_of(self.tbl_node());
        let (_, gc) = *cols.get(c).ok_or(Error::CellNotFound { row: 0, col: c })?;
        let old = gc.attr(QName::new("", "w")).and_then(|v| v.trim().parse::<i64>().ok()).unwrap_or(0);
        if old == w {
            return Ok(());
        }
        self.edit(|doc, frame, tbl| {
            {
                let mut t = tbl_lens(doc, tbl)?;
                let mut g = t.child_mut::<TableGrid>().ok_or(Error::NotATable)?;
                g.nth_child_mut::<GridColumn>(c).ok_or(Error::NotATable)?.set_attr(QName::new("", "w"), &w.to_string())?;
            }
            grow_frame(doc, frame, w - old, 0)
        })
    }

    /// Sets `a:tr@h` (EMU ≥ 0); the frame height follows.
    pub fn set_row_height(&mut self, r: usize, h: i64) -> Result<()> {
        if h < 0 {
            return Err(Error::ValueOutOfRange { what: "tr@h" });
        }
        let rows = rows_of(self.tbl_node());
        let (_, tr) = *rows.get(r).ok_or(Error::CellNotFound { row: r, col: 0 })?;
        let old = tr.attr(QName::new("", "h")).and_then(|v| v.trim().parse::<i64>().ok()).unwrap_or(0);
        if old == h {
            return Ok(());
        }
        self.edit(|doc, frame, tbl| {
            tbl_lens(doc, tbl)?.nth_child_mut::<TableRow>(r).ok_or(Error::NotATable)?.set_attr(QName::new("", "h"), &h.to_string())?;
            grow_frame(doc, frame, 0, h - old)
        })
    }

    // ------------------------------------------------------------ properties

    /// Sets (`Some`) or removes (`None`) the table style id: one defined in
    /// `ppt/tableStyles.xml` or one of PowerPoint's 74 built-in ids
    /// (case-insensitive), else `TableStyleNotDefined` (ADR-0013 §3).
    pub fn set_style(&mut self, id: Option<&str>) -> Result<()> {
        // the matched id's own spelling is written: the definition's, or the
        // built-in list's uppercase ST_Guid form (review #57 M1)
        let canon: Option<String> = match id {
            None => None,
            Some(id) => Some(
                self.styles
                    .iter()
                    .map(|s| s.trim())
                    .find(|s| s.eq_ignore_ascii_case(id.trim()))
                    .map(str::to_owned)
                    .or_else(|| crate::builtin_styles::builtin(id).map(str::to_owned))
                    .ok_or_else(|| Error::TableStyleNotDefined(id.to_owned()))?,
            ),
        };
        let id = canon.as_deref();
        // an inline a:tableStyle (the other branch of the tblPr choice) is
        // not replaced implicitly (review #57 m1)
        if self
            .tbl_node()
            .elements()
            .find(|e| e.is(TableProperties::QNAME))
            .is_some_and(|p| p.elements().any(|e| e.is(QName::new(crate::names::A_NS, "tableStyle"))))
        {
            return Err(Error::TableStyleInline);
        }
        let current = self
            .tbl_node()
            .elements()
            .find(|e| e.is(TableProperties::QNAME))
            .and_then(|p| p.elements().find(|e| e.is(TableStyleId::QNAME)))
            .map(|e| e.text().into_owned());
        if current.as_deref() == id {
            return Ok(());
        }
        self.edit(|doc, _, tbl| {
            let mut t = tbl_lens(doc, tbl)?;
            match id {
                Some(id) => {
                    let mut pr = t.get_or_insert::<TableProperties>()?;
                    pr.get_or_insert::<TableStyleId>()?.set_text(id)?;
                }
                None => {
                    if let Some(mut pr) = t.child_mut::<TableProperties>() {
                        pr.remove::<TableStyleId>();
                    }
                }
            }
            Ok(())
        })
    }

    /// Sets a `a:tblPr` flag (`true` writes `"1"`, `false` removes it: the
    /// schema default).
    pub fn set_flag(&mut self, flag: TableFlag, on: bool) -> Result<()> {
        let local = match flag {
            TableFlag::FirstRow => "firstRow",
            TableFlag::FirstColumn => "firstCol",
            TableFlag::LastRow => "lastRow",
            TableFlag::LastColumn => "lastCol",
            TableFlag::BandRows => "bandRow",
            TableFlag::BandColumns => "bandCol",
        };
        let q = QName::new("", local);
        let cur = self.tbl_node().elements().find(|e| e.is(TableProperties::QNAME)).and_then(|p| p.attr(q)).map(str::trim);
        if matches!(cur, Some("1" | "true")) == on && (on || cur.is_none()) {
            return Ok(());
        }
        self.edit(|doc, _, tbl| {
            let mut t = tbl_lens(doc, tbl)?;
            let mut pr = t.get_or_insert::<TableProperties>()?;
            if on {
                pr.set_attr(q, "1")?;
            } else {
                pr.remove_attr(q)?;
            }
            Ok(())
        })
    }

    /// Sets a cell margin (EMU, `ST_Coordinate32`) or removes it (`None`).
    pub fn set_margin(&mut self, r: usize, c: usize, side: Side, value: Option<i64>) -> Result<()> {
        if value.is_some_and(|v| !(i64::from(i32::MIN)..=i64::from(i32::MAX)).contains(&v)) {
            return Err(Error::ValueOutOfRange { what: "tcPr margin" });
        }
        let local = match side {
            Side::Left => "marL",
            Side::Right => "marR",
            Side::Top => "marT",
            Side::Bottom => "marB",
        };
        self.tcpr_edit(r, c, |pr| match value {
            Some(v) => pr.set_attr(QName::new("", local), &v.to_string()).map_err(Into::into),
            None => pr.remove_attr(QName::new("", local)).map(drop).map_err(Into::into),
        })
    }

    /// Sets the vertical anchor (`t`, `ctr`, `b`) or removes it.
    pub fn set_anchor(&mut self, r: usize, c: usize, anchor: Option<&str>) -> Result<()> {
        if anchor.is_some_and(|a| !matches!(a, "t" | "ctr" | "b" | "just" | "dist")) {
            return Err(Error::ValueOutOfRange { what: "tcPr@anchor" });
        }
        self.tcpr_edit(r, c, |pr| match anchor {
            Some(v) => pr.set_attr(QName::new("", "anchor"), v).map_err(Into::into),
            None => pr.remove_attr(QName::new("", "anchor")).map(drop).map_err(Into::into),
        })
    }

    /// Sets the cell fill, replacing any fill choice of `a:tcPr`.
    pub fn set_fill(&mut self, r: usize, c: usize, fill: &CellFill) -> Result<()> {
        if let CellFill::Solid(col) = fill {
            check_color(col)?;
        }
        self.tcpr_edit(r, c, |pr| {
            remove_fills(pr);
            match fill {
                CellFill::Solid(col) => write_color(&mut pr.insert::<SolidFill>()?, col),
                CellFill::None => pr.insert::<NoFill>().map(drop).map_err(Into::into),
            }
        })
    }

    fn tcpr_edit(&mut self, r: usize, c: usize, f: impl FnOnce(&mut LensMut<'_, TableCellProperties>) -> Result<()>) -> Result<()> {
        let g = self.grid()?;
        if r >= g.nrows || c >= g.ncols {
            return Err(Error::CellNotFound { row: r, col: c });
        }
        self.edit(|doc, _, tbl| {
            let mut t = tbl_lens(doc, tbl)?;
            let mut tr = t.nth_child_mut::<TableRow>(r).ok_or(Error::NotATable)?;
            let mut tc = tr.nth_child_mut::<TableCell>(c).ok_or(Error::NotATable)?;
            let mut pr = tc.get_or_insert::<TableCellProperties>()?;
            f(&mut pr)
        })
    }

    // ------------------------------------------------------------ borders

    /// Sets one edge of cell (r, c) — of its merged range, if any — on every
    /// cell along it and on the paired line of each neighbour across it,
    /// so a shared edge never differs (ADR-0013 §3).
    pub fn set_border(&mut self, r: usize, c: usize, edge: Edge, line: &Line) -> Result<()> {
        check_line(line)?;
        let g = self.grid()?;
        if r >= g.nrows || c >= g.ncols {
            return Err(Error::CellNotFound { row: r, col: c });
        }
        let writes = border_cells(&g, r, c, edge);
        self.edit(|doc, _, tbl| write_lines(doc, tbl, &writes, line))
    }

    /// Sets every edge of every cell to `line`.
    pub fn set_borders(&mut self, line: &Line) -> Result<()> {
        check_line(line)?;
        let g = self.grid()?;
        let mut writes = Vec::new();
        for r in 0..g.nrows {
            for c in 0..g.ncols {
                for e in [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom] {
                    writes.push((r, c, e));
                }
            }
        }
        self.edit(|doc, _, tbl| write_lines(doc, tbl, &writes, line))
    }
}

/// Every (cell, line) a border write touches.
fn border_cells(g: &Grid, r: usize, c: usize, edge: Edge) -> Vec<(usize, usize, Edge)> {
    let m = g.merge_at(r, c).copied().unwrap_or(Merge { r0: r, c0: c, nr: 1, nc: 1, edge: true });
    let rows = m.r0..m.r0 + m.nr;
    let cols = m.c0..m.c0 + m.nc;
    let mut out = Vec::new();
    match edge {
        Edge::Left => {
            for rr in rows {
                out.push((rr, m.c0, Edge::Left));
                if m.c0 > 0 {
                    out.push((rr, m.c0 - 1, Edge::Right));
                }
            }
        }
        Edge::Right => {
            let last = m.c0 + m.nc - 1;
            for rr in rows {
                out.push((rr, last, Edge::Right));
                if last + 1 < g.ncols {
                    out.push((rr, last + 1, Edge::Left));
                }
            }
        }
        Edge::Top => {
            for cc in cols {
                out.push((m.r0, cc, Edge::Top));
                if m.r0 > 0 {
                    out.push((m.r0 - 1, cc, Edge::Bottom));
                }
            }
        }
        Edge::Bottom => {
            let last = m.r0 + m.nr - 1;
            for cc in cols {
                out.push((last, cc, Edge::Bottom));
                if last + 1 < g.nrows {
                    out.push((last + 1, cc, Edge::Top));
                }
            }
        }
    }
    out
}

fn check_line(line: &Line) -> Result<()> {
    if let Line::Solid { width, color } = line {
        if !(0..=20_116_800).contains(width) {
            return Err(Error::ValueOutOfRange { what: "a:ln*@w" });
        }
        check_color(color)?;
    }
    Ok(())
}

fn write_lines(doc: &mut XmlDocument, tbl: &[usize], writes: &[(usize, usize, Edge)], line: &Line) -> Result<()> {
    for &(r, c, e) in writes {
        let mut t = tbl_lens(doc, tbl)?;
        let mut tr = t.nth_child_mut::<TableRow>(r).ok_or(Error::NotATable)?;
        let mut tc = tr.nth_child_mut::<TableCell>(c).ok_or(Error::NotATable)?;
        let mut pr = tc.get_or_insert::<TableCellProperties>()?;
        match e {
            Edge::Left => {
                pr.remove::<LeftBorderLineProperties>();
                fill_line(&mut pr.insert::<LeftBorderLineProperties>()?, line)?;
            }
            Edge::Right => {
                pr.remove::<RightBorderLineProperties>();
                fill_line(&mut pr.insert::<RightBorderLineProperties>()?, line)?;
            }
            Edge::Top => {
                pr.remove::<TopBorderLineProperties>();
                fill_line(&mut pr.insert::<TopBorderLineProperties>()?, line)?;
            }
            Edge::Bottom => {
                pr.remove::<BottomBorderLineProperties>();
                fill_line(&mut pr.insert::<BottomBorderLineProperties>()?, line)?;
            }
        }
    }
    Ok(())
}

fn fill_line<L: ooxml_core::ElementSpec>(l: &mut LensMut<'_, L>, line: &Line) -> Result<()>
where
    SolidFill: ooxml_core::ChildOf<L>,
    NoFill: ooxml_core::ChildOf<L>,
{
    match line {
        Line::Solid { width, color } => {
            l.set_attr(QName::new("", "w"), &width.to_string())?;
            write_color(&mut l.insert::<SolidFill>()?, color)
        }
        Line::None => l.insert::<NoFill>().map(drop).map_err(Into::into),
    }
}

fn write_color(f: &mut LensMut<'_, SolidFill>, c: &TableColor) -> Result<()> {
    match c {
        TableColor::Rgb([r, g, b]) => f.insert::<RgbColorModelHex>()?.set_attr(QName::new("", "val"), &format!("{r:02X}{g:02X}{b:02X}"))?,
        TableColor::Scheme(s) => f.insert::<SchemeColor>()?.set_attr(QName::new("", "val"), s)?,
    }
    Ok(())
}

/// Removes every fill choice child of `a:tcPr`.
fn remove_fills(pr: &mut LensMut<'_, TableCellProperties>) {
    const FILLS: [&str; 6] = ["noFill", "solidFill", "gradFill", "blipFill", "pattFill", "grpFill"];
    let items: Vec<usize> = pr
        .node()
        .indexed_elements()
        .filter(|(_, e)| e.name().ns() == crate::names::A_NS && FILLS.contains(&e.name().local()))
        .map(|(i, _)| i)
        .collect();
    for &i in items.iter().rev() {
        pr.remove_at(i);
    }
}

/// A paragraph with no text, field or break (python-pptx's empty).
fn para_is_empty(p: &RawNode) -> bool {
    fn text(n: &RawNode) -> bool {
        (n.is(dml::Text::QNAME) && !n.text().is_empty()) || n.elements().any(text)
    }
    !text(p) && !p.elements().any(|e| e.is(dml::Field::QNAME) || e.is(dml::Break::QNAME))
}

/// The path of cell (r, c)'s `a:txBody`, creating one (`a:bodyPr` and an
/// empty `a:p`) when the cell has none.
fn ensure_body(doc: &mut XmlDocument, tbl: &[usize], r: usize, c: usize) -> Result<Vec<usize>> {
    let cell = cell_path_in(doc.root(), tbl, r, c).ok_or(Error::CellNotFound { row: r, col: c })?;
    if let Some(i) = find_child(node_at(doc.root(), &cell).expect("cell"), dml::TextBody::QNAME) {
        return Ok(sub(&cell, &[i]));
    }
    {
        let mut t = tbl_lens(doc, tbl)?;
        let mut tr = t.nth_child_mut::<TableRow>(r).ok_or(Error::NotATable)?;
        let mut tc = tr.nth_child_mut::<TableCell>(c).ok_or(Error::NotATable)?;
        let mut b = tc.insert::<dml::TextBody>()?;
        b.insert::<BodyProperties>()?;
        b.insert::<Paragraph>()?;
    }
    let i = find_child(node_at(doc.root(), &cell).expect("cell"), dml::TextBody::QNAME).expect("created");
    Ok(sub(&cell, &[i]))
}

fn cell_path_in(root: &RawNode, tbl: &[usize], r: usize, c: usize) -> Option<Vec<usize>> {
    let t = node_at(root, tbl)?;
    let (ri, tr) = *rows_of(t).get(r)?;
    let (ci, _) = *cells_of(tr).get(c)?;
    Some(sub(tbl, &[ri, ci]))
}

fn body_lens_at<'d>(doc: &'d mut XmlDocument, path: &[usize]) -> Result<LensMut<'d, dml::TextBody>> {
    match walk(doc, path)? {
        Cursor::ABody(l) => Ok(l),
        _ => Err(Error::NoTextBody),
    }
}

/// Fills a new, empty `a:tc` with the copied formatting and an empty body.
fn fill_new_cell(tc: &mut LensMut<'_, TableCell>, f: &CellFormat) -> Result<()> {
    {
        let mut body = tc.insert::<dml::TextBody>()?;
        match &f.body {
            Some(d) => {
                body.insert_detached(d)?;
            }
            None => {
                body.insert::<BodyProperties>()?;
            }
        }
        if let Some(d) = &f.lst {
            body.insert_detached(d)?;
        }
        let mut p = body.insert::<Paragraph>()?;
        if let Some(d) = &f.ppr {
            p.insert_detached(d)?;
        }
        if let Some(d) = &f.end {
            p.insert_detached(d)?;
        }
    }
    if let Some(d) = &f.tcpr {
        tc.insert_detached(d)?;
    }
    Ok(())
}

/// `a:extLst/a:ext[@uri]/a16:{rowId|colId}@val`, `a16` declared on the id
/// element (PowerPoint's form).
fn add_id<P, I>(parent: &mut LensMut<'_, P>, uri: &str, id: u32) -> Result<()>
where
    P: ooxml_core::ElementSpec,
    ExtensionList: ooxml_core::ChildOf<P>,
    I: ooxml_core::ChildOf<Extension>,
{
    let mut lst = parent.get_or_insert::<ExtensionList>()?;
    let mut ext = lst.insert::<Extension>()?;
    ext.set_attr(QName::new("", "uri"), uri)?;
    let mut e = ext.insert_with_ns::<I>(&[NsDecl { prefix: "a16", uri: A16_NS }])?;
    e.set_attr(QName::new("", "val"), &id.to_string())?;
    Ok(())
}

/// Brings every cell's merge attributes to `target`.
fn apply_attrs(doc: &mut XmlDocument, tbl: &[usize], target: &Grid) -> Result<()> {
    for r in 0..target.nrows {
        for c in 0..target.ncols {
            let have = {
                let t = node_at(doc.root(), tbl).expect("table");
                let (_, tr) = rows_of(t)[r];
                cell_attrs(cells_of(tr)[c].1)?
            };
            let want = target.cells[r][c];
            if have != want {
                #[cfg(test)]
                fault::hit()?;
                let mut t = tbl_lens(doc, tbl)?;
                let mut tr = t.nth_child_mut::<TableRow>(r).ok_or(Error::NotATable)?;
                let mut tc = tr.nth_child_mut::<TableCell>(c).ok_or(Error::NotATable)?;
                write_attrs(&mut tc, have, want)?;
            }
        }
    }
    Ok(())
}

/// Grows (or shrinks) the frame's `p:xfrm/a:ext` by (dx, dy), not below 0.
fn grow_frame(doc: &mut XmlDocument, frame: &[usize], dx: i64, dy: i64) -> Result<()> {
    if dx == 0 && dy == 0 {
        return Ok(());
    }
    let mut f = frame_lens(doc, frame)?;
    let Some(mut x) = f.child_mut::<pml::Transform>() else { return Ok(()) };
    let Some(mut ext) = x.child_mut::<dml::Extents>() else { return Ok(()) };
    for (local, d) in [("cx", dx), ("cy", dy)] {
        if d == 0 {
            continue;
        }
        let q = QName::new("", local);
        let old = ext.node().attr(q).and_then(|v| v.trim().parse::<i64>().ok()).unwrap_or(0);
        ext.set_attr(q, &(old + d).max(0).to_string())?;
    }
    Ok(())
}

// Review #57 m4 (ADR-0005 R2-6): every element this module writes an
// attribute or text on has a complete attribute list, checked at compile
// time rather than refused at run time (`IncompleteSpec`).
const _: () = assert!(
    !TableCell::ATTRS_INCOMPLETE
        && !TableRow::ATTRS_INCOMPLETE
        && !GridColumn::ATTRS_INCOMPLETE
        && !TableProperties::ATTRS_INCOMPLETE
        && !TableStyleId::ATTRS_INCOMPLETE
        && !TableCellProperties::ATTRS_INCOMPLETE
        && !LeftBorderLineProperties::ATTRS_INCOMPLETE
        && !RightBorderLineProperties::ATTRS_INCOMPLETE
        && !TopBorderLineProperties::ATTRS_INCOMPLETE
        && !BottomBorderLineProperties::ATTRS_INCOMPLETE
        && !RgbColorModelHex::ATTRS_INCOMPLETE
        && !SchemeColor::ATTRS_INCOMPLETE
        && !Extension::ATTRS_INCOMPLETE
        && !RowIdIdentifier::ATTRS_INCOMPLETE
        && !ColIdIdentifier::ATTRS_INCOMPLETE
        && !dml::Extents::ATTRS_INCOMPLETE,
    "an element Phase 1b writes has an incomplete attribute list (ADR-0005 R2-6)"
);

#[cfg(test)]
pub(crate) mod fault {
    //! Fault injection inside an edit (review #57 m8, mutant X7): a failure
    //! after some writes must leave the slide untouched (clone-and-install).
    use std::cell::Cell;

    thread_local! {
        pub(crate) static FAIL_AT_WRITE: Cell<Option<usize>> = const { Cell::new(None) };
    }

    pub(super) fn hit() -> crate::Result<()> {
        FAIL_AT_WRITE.with(|f| match f.get() {
            Some(0) => {
                f.set(None);
                Err(crate::Error::TableNotWellFormed("injected fault".into()))
            }
            Some(n) => {
                f.set(Some(n - 1));
                Ok(())
            }
            None => Ok(()),
        })
    }

    #[test]
    fn a_failure_after_some_writes_leaves_the_slide_untouched() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../reference/fixtures/pptx/poi/table_test2.pptx");
        let input = std::fs::read(path).unwrap();
        let mut pres = crate::Presentation::from_bytes(input).unwrap();
        let before = pres.to_bytes().unwrap();
        // merging (3,1)-(4,2) writes four cells' attributes: fail at the third
        FAIL_AT_WRITE.set(Some(2));
        let r = {
            let mut s = pres.slide_mut(0).unwrap();
            let mut sh = s.shape_mut_by_id(4).unwrap();
            sh.table_mut().unwrap().merge(3, 1, 4, 2)
        };
        FAIL_AT_WRITE.set(None);
        assert!(r.is_err());
        assert_eq!(pres.to_bytes().unwrap(), before, "a failure inside an edit must leave the slide byte-identical");
    }
}
