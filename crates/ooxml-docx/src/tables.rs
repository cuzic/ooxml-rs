//! Word Phase2 tables (ADR-0011 §7): insert and delete rows, insert and
//! delete grid columns, merge cells. Every operation requires a consistent
//! grid, checks everything on the read model, dry-runs its first insert,
//! then writes.

use std::ops::Range;

use ooxml_core::{LensMut, QName, RawItem, RawNode, XmlDocument};
use ooxml_core::lens::Detached;
use ooxml_schema::wml::{
    Deleted, Inserted, GridAfter, GridBefore, GridColumn, GridSpan, NS_W, Paragraph as P, ParagraphProperties, TableCell,
    TableCellProperties, TableCellWidth, TableGrid, TablePropertyExceptions, TableRow, TableRowProperties,
    VerticalMerge,
};

use crate::document::{Document, node_at};
use crate::error::{Error, Result};
use crate::fields::Operation;
use crate::names::is;
use crate::story::StoryIndex;
use crate::revisions::{Marks, REVISION_NS, TrackedChanges, check_deletable_paragraph, check_path, delete_paragraph_runs, mark_paragraph, range_marker};

const fn w(local: &'static str) -> QName {
    QName::new(NS_W, local)
}
const VAL: QName = w("val");

/// A table of a story, in document order (nested tables included).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TableRef {
    story: StoryIndex,
    index: usize,
}

impl TableRef {
    /// Table `index` of `story` (checked on use).
    pub fn new(story: StoryIndex, index: usize) -> Self {
        TableRef { story, index }
    }

    /// The story.
    pub fn story(self) -> StoryIndex {
        self.story
    }

    /// The index among the story's tables.
    pub fn index(self) -> usize {
        self.index
    }
}

/// Which existing row a new row copies (ADR-0011 §7.1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RowTemplate {
    /// The row above the insertion point (the first row when inserting at 0).
    #[default]
    Above,
    /// The row below the insertion point (the last row when appending).
    Below,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VMerge {
    Restart,
    Continue,
}

#[derive(Clone, Debug)]
struct CellModel {
    item: usize,
    start: usize,
    span: usize,
    vmerge: Option<VMerge>,
}

#[derive(Clone, Debug)]
struct RowModel {
    item: usize,
    before: usize,
    after: usize,
    cells: Vec<CellModel>,
}

impl RowModel {
    fn cell_at(&self, col: usize) -> Option<&CellModel> {
        self.cells.iter().find(|c| c.start <= col && col < c.start + c.span)
    }
}

/// The grid of a table (ADR-0011 §7: consistent grid).
#[derive(Clone, Debug)]
struct Grid {
    /// The `w:gridCol`s: item index in `w:tblGrid`, and `w:w`.
    cols: Vec<(usize, Option<String>)>,
    rows: Vec<RowModel>,
    hmerge: bool,
}

fn grid_width(col: &(usize, Option<String>)) -> Option<u64> {
    col.1.as_deref()?.trim().parse().ok()
}

/// Existing preferred width, adjustable only when it and the spanned grid use twips.
fn span_width(tc: &RawNode, cols: &[(usize, Option<String>)]) -> Option<u64> {
    let tcw = child(child(tc, "tcPr")?, "tcW")?;
    if tcw.attr(w("type")) != Some("dxa") || !cols.iter().all(|col| grid_width(col).is_some()) {
        return None;
    }
    tcw.attr(w("w"))?.trim().parse().ok()
}

/// `w:val` of an `ST_DecimalNumber` element (0 when absent).
fn decimal(n: Option<&RawNode>) -> Result<usize> {
    match n.and_then(|e| e.attr(VAL)) {
        None => Ok(0),
        Some(v) => v.trim().parse().map_err(|_| Error::InconsistentGrid),
    }
}

fn child<'a>(n: &'a RawNode, local: &'static str) -> Option<&'a RawNode> {
    n.elements().find(|e| e.is(w(local)))
}

fn read_grid(tbl: &RawNode) -> Result<Grid> {
    let (_, grid) = tbl.indexed_elements().find(|(_, e)| is::<TableGrid>(e)).ok_or(Error::InconsistentGrid)?;
    let cols: Vec<(usize, Option<String>)> = grid
        .indexed_elements()
        .filter(|(_, e)| is::<GridColumn>(e))
        .map(|(i, e)| (i, e.attr(w("w")).map(str::to_owned)))
        .collect();
    let mut rows = Vec::new();
    let mut hmerge = false;
    for (i, e) in tbl.indexed_elements() {
        if is::<TableRow>(e) {
            let trpr = e.elements().find(|x| is::<TableRowProperties>(x));
            let before = decimal(trpr.and_then(|t| child(t, "gridBefore")))?;
            let after = decimal(trpr.and_then(|t| child(t, "gridAfter")))?;
            let mut cells = Vec::new();
            let mut col = before;
            for (k, c) in e.indexed_elements() {
                if is::<TableCell>(c) {
                    let tcpr = c.elements().find(|x| is::<TableCellProperties>(x));
                    let span = match tcpr.and_then(|t| child(t, "gridSpan")) {
                        Some(g) => decimal(Some(g))?.max(1),
                        None => 1,
                    };
                    let vmerge = tcpr.and_then(|t| child(t, "vMerge")).map(|v| match v.attr(VAL) {
                        Some("restart") => VMerge::Restart,
                        _ => VMerge::Continue,
                    });
                    hmerge |= tcpr.is_some_and(|t| child(t, "hMerge").is_some());
                    cells.push(CellModel { item: k, start: col, span, vmerge });
                    col += span;
                } else if c.is(w("sdt")) || c.is(w("customXml")) {
                    return Err(Error::UnsupportedContext { ancestor: if c.is(w("sdt")) { "w:sdt" } else { "w:customXml" } });
                }
            }
            if col + after != cols.len() {
                return Err(Error::InconsistentGrid);
            }
            rows.push(RowModel { item: i, before, after, cells });
        } else if e.is(w("sdt")) || e.is(w("customXml")) {
            return Err(Error::UnsupportedContext { ancestor: if e.is(w("sdt")) { "w:sdt" } else { "w:customXml" } });
        }
    }
    Ok(Grid { cols, rows, hmerge })
}

/// Revision elements (ADR-0011 §7, review M6) anywhere in `n`.
const REVISIONS: &[&str] = &[
    "ins", "del", "moveFrom", "moveTo", "rPrChange", "pPrChange", "sectPrChange", "tblPrChange", "trPrChange",
    "tcPrChange", "tblGridChange", "tblPrExChange", "numberingChange", "cellIns", "cellDel", "cellMerge",
];

fn find_desc<'a>(n: &'a RawNode, pred: &dyn Fn(&RawNode) -> bool) -> Option<&'a RawNode> {
    let mut stack = vec![n];
    while let Some(x) = stack.pop() {
        if pred(x) {
            return Some(x);
        }
        stack.extend(x.elements());
    }
    None
}

fn row_revision(tr: &RawNode) -> Option<&RawNode> {
    tr.elements()
        .filter(|x| is::<TableRowProperties>(x))
        .flat_map(RawNode::elements)
        .find(|x| ["ins", "del", "trPrChange"].iter().any(|l| x.is(w(l))))
}

fn check_revisions(n: &RawNode) -> Result<()> {
    if let Some(x) = find_desc(n, &|x| x.name().ns() == NS_W && REVISIONS.contains(&x.name().local())) {
        return Err(Error::RevisionInRange { element: format!("w:{}", x.name().local()) });
    }
    Ok(())
}

/// Rewriting cell properties leaves the cell's paragraph content untouched.
fn check_cell_property_revisions(tc: &RawNode) -> Result<()> {
    let revision = tc.elements()
        .filter(|x| is::<TableCellProperties>(x))
        .flat_map(RawNode::elements)
        .find(|x| ["tcPrChange", "cellIns", "cellDel", "cellMerge"].iter().any(|l| x.is(w(l))));
    if let Some(x) = revision {
        return Err(Error::RevisionInRange { element: format!("w:{}", x.name().local()) });
    }
    Ok(())
}

/// Refusals for content a delete or merge would remove (ADR-0011 §7, M6).
fn check_removable(n: &RawNode) -> Result<()> {
    if find_desc(n, &|x| x.is(w("footnoteReference")) || x.is(w("endnoteReference"))).is_some() {
        return Err(Error::WouldRemoveNote);
    }
    check_revisions(n)?;
    if find_desc(n, &|x| x.is(w("fldChar")) || x.is(w("fldSimple"))).is_some() {
        return Err(Error::InsideField);
    }
    if find_desc(n, &|x| {
        x.is(w("commentRangeStart")) || x.is(w("commentRangeEnd")) || x.is(w("commentReference"))
    })
    .is_some()
    {
        return Err(Error::AnchorWouldBreak { marker: "w:commentRangeStart".into() });
    }
    Ok(())
}

/// Range markers (`bookmark*`, `perm*`, `moveFromRange*`, …) in `nodes` whose
/// partner is not among them (ADR-0011 §7.2).
fn check_anchor_pairs(nodes: &[&RawNode]) -> Result<()> {
    let mut starts: Vec<(String, String)> = Vec::new();
    let mut ends: Vec<(String, String)> = Vec::new();
    for n in nodes {
        let mut stack = vec![*n];
        while let Some(x) = stack.pop() {
            let l = x.name().local();
            let paired = ["bookmark", "perm", "moveFromRange", "moveToRange"]
                .iter()
                .any(|b| l.strip_prefix(b).is_some_and(|r| r == "Start" || r == "End"))
                || (l.starts_with("customXml") && (l.ends_with("RangeStart") || l.ends_with("RangeEnd")));
            if x.name().ns() == NS_W && paired
            {
                let id = x.attr(w("id")).unwrap_or_default().to_owned();
                let kind = l.trim_end_matches("Start").trim_end_matches("End").to_owned();
                if l.ends_with("Start") {
                    starts.push((kind, id));
                } else {
                    ends.push((kind, id));
                }
            }
            stack.extend(x.elements());
        }
    }
    for s in &starts {
        if !ends.contains(s) {
            return Err(Error::AnchorWouldBreak { marker: format!("w:{}Start", s.0) });
        }
    }
    for e in &ends {
        if !starts.contains(e) {
            return Err(Error::AnchorWouldBreak { marker: format!("w:{}End", e.0) });
        }
    }
    Ok(())
}

/// Is a cell empty for a merge or a column delete: exactly one paragraph,
/// holding nothing but its `w:pPr` (ADR-0011 §7.4, fail closed: no runs).
fn cell_is_empty(tc: &RawNode) -> bool {
    let content: Vec<&RawNode> = tc.elements().filter(|e| !is::<TableCellProperties>(e)).collect();
    matches!(content[..], [p] if is::<P>(p) && p.elements().all(is::<ParagraphProperties>))
        && tc.children().iter().all(|c| !matches!(c, RawItem::Text(t) if !t.is_whitespace()))
}

struct RowInsertion {
    path: Vec<usize>,
    pos: usize,
    trpr: Option<Detached<TableRowProperties>>,
    tblprex: Option<Detached<TablePropertyExceptions>>,
    cells: Vec<InsertedCell>,
}

struct InsertedCell {
    tcpr: Option<Detached<TableCellProperties>>,
    continue_merge: bool,
    ppr: Option<Detached<ParagraphProperties>>,
}

impl RowInsertion {
    fn write(&self, xml: &mut XmlDocument, count: usize, mut marks: Option<&mut Marks<'_>>) -> Result<()> {
        let mut table = xml.lens_mut_at::<ooxml_schema::wml::Table>(&self.path)?;
        table.check_insert_at::<TableRow>(self.pos)?;
        for offset in 0..count {
            let mut row = table.insert_at::<TableRow>(self.pos + offset)?;
            if let Some(copy) = &self.tblprex {
                row.insert_detached(copy)?;
            }
            if let Some(copy) = &self.trpr {
                row.insert_detached(copy)?;
            }
            if let Some(marks) = marks.as_deref_mut() {
                let mut trpr = row.get_or_insert::<TableRowProperties>()?;
                marks.write(trpr.insert_with_ns::<Inserted>(REVISION_NS)?)?;
            }
            for cell in &self.cells {
                let mut tc = row.insert::<TableCell>()?;
                if cell.tcpr.is_some() || cell.continue_merge {
                    let mut pr = match &cell.tcpr {
                        Some(copy) => tc.insert_detached(copy)?,
                        None => tc.insert::<TableCellProperties>()?,
                    };
                    if cell.continue_merge {
                        pr.insert::<VerticalMerge>()?;
                    }
                }
                let mut p = tc.insert::<P>()?;
                if let Some(copy) = &cell.ppr {
                    p.insert_detached(copy)?;
                }
                if let Some(marks) = marks.as_deref_mut() {
                    mark_paragraph::<Inserted>(&mut p, marks)?;
                }
            }
        }
        Ok(())
    }
}

impl Document {
    /// Paths (from the story root) of the story's tables in document order,
    /// nested ones included, and the story's paragraph index of each
    /// table's first paragraph (for re-indexing).
    fn table_paths(&self, story: StoryIndex) -> Vec<Vec<usize>> {
        let root = self.story_root_node(story);
        let mut out = Vec::new();
        let mut stack: Vec<(Vec<usize>, &RawNode)> = vec![(Vec::new(), root)];
        while let Some((path, n)) = stack.pop() {
            let mut kids = Vec::new();
            for (i, c) in n.indexed_elements() {
                // block flow: body, cells, notes, comments, sdtContent, customXml, rows
                if c.name().ns() != NS_W {
                    continue;
                }
                let descend = matches!(
                    c.name().local(),
                    "body" | "tbl" | "tr" | "tc" | "sdt" | "sdtContent" | "customXml" | "footnote" | "endnote" | "comment"
                );
                if descend {
                    let mut p = path.clone();
                    p.push(i);
                    if c.is(w("tbl")) {
                        out.push(p.clone());
                    }
                    kids.push((p, c));
                }
            }
            stack.extend(kids.into_iter().rev());
        }
        out
    }

    /// Number of tables in a story (document order, nested included).
    pub fn table_count(&self, story: StoryIndex) -> usize {
        self.stories.get(story.0).map_or(0, |_| self.table_paths(story).len())
    }

    /// The table's row count (direct `w:tr` children).
    pub fn table_row_count(&self, t: TableRef) -> Option<usize> {
        let path = self.table_paths(t.story).get(t.index)?.clone();
        Some(node_at(self.story_root_node(t.story), &path).elements().filter(|e| is::<TableRow>(e)).count())
    }

    /// The table's grid column count (`w:tblGrid/w:gridCol`).
    pub fn table_grid_columns(&self, t: TableRef) -> Option<usize> {
        let path = self.table_paths(t.story).get(t.index)?.clone();
        let tbl = node_at(self.story_root_node(t.story), &path);
        Some(tbl.elements().filter(|e| is::<TableGrid>(e)).flat_map(|g| g.elements().filter(|e| is::<GridColumn>(e))).count())
    }

    fn locate_table(&self, t: TableRef, check_all_row_revisions: bool) -> Result<(Vec<usize>, Grid)> {
        if t.story.0 >= self.stories.len() {
            return Err(Error::NoSuchStory);
        }
        let path = self.table_paths(t.story).get(t.index).cloned().ok_or(Error::NoSuchTable(t.index))?;
        // tables reached only through tables, bodies and story roots (no content control)
        let root = self.story_root_node(t.story);
        let mut n = root;
        for &i in &path {
            n = n.children()[i].as_element().expect("element");
            if n.is(w("sdt")) || n.is(w("customXml")) || n.is(w("sdtContent")) {
                return Err(Error::UnsupportedContext { ancestor: "w:sdt" });
            }
        }
        for property in n.elements().filter(|n| n.is(w("tblPr")) || n.is(w("tblGrid"))) {
            check_revisions(property)?;
        }
        let grid = read_grid(n)?;
        if check_all_row_revisions {
            for tr in n.elements().filter(|x| is::<TableRow>(x)) {
                if let Some(revision) = row_revision(tr) {
                    return Err(Error::RevisionInRange { element: format!("w:{}", revision.name().local()) });
                }
            }
        }
        Ok((path, grid))
    }

    /// After a structural table edit: re-walk the story and give every
    /// paragraph from the table on a fresh generation.
    fn table_changed(&mut self, story: StoryIndex, table_path: &[usize]) {
        let first = {
            let data = &self.stories[story.0];
            data.index.paragraphs.iter().position(|p| data.index.path(p.frame).starts_with(table_path))
        };
        self.reindex_counted(story);
        let n = self.stories[story.0].index.paragraphs.len();
        self.generations[story.0].resize(n, 0);
        let from = first.unwrap_or(n);
        for i in from..n {
            self.bump(crate::story::ParaRef::new(story, i));
        }
    }

    fn table_gate(&self) -> Result<()> {
        self.write_gate_for(Operation::Structure)
    }

    /// Inserts `count` rows at row index `at` (0..=row count; ADR-0011 §7.1).
    /// Each row copies the template row's `w:trPr` (without `w:tblHeader`)
    /// and `w:tblPrEx`, and per cell its `w:tcPr` (without `w:vMerge`) and
    /// one empty paragraph whose `w:pPr` copies the template cell's first
    /// paragraph's (without `w:sectPr`, `w:pPrChange`, `w:numPr`). A new
    /// cell continues a vertical merge (`w:vMerge`) only when the template
    /// cell is in one and the row below the insertion point continues it at
    /// that grid column. No `w14:paraId`, rsid or attribute of `w:tr` is
    /// copied.
    ///
    /// Errors, before any change: [`Self::write_gate_for`]
    /// ([`Operation::Structure`]); [`Error::NoSuchTable`];
    /// [`Error::InconsistentGrid`]; [`Error::UnsupportedContext`] (row- or
    /// cell-level content controls, a table in one); [`Error::InsideRevision`]
    /// / [`Error::TrackedPropertyChange`] (revision elements in the template
    /// row's properties or its cells' first paragraphs' marks);
    /// [`Error::InvalidValue`] (`at` out of range, `count` 0); dry-run
    /// failures.
    pub fn insert_rows(&mut self, t: TableRef, at: usize, count: usize, template: RowTemplate) -> Result<()> {
        self.table_gate()?;
        let plan = self.plan_insert_rows(t, at, count, template, false)?;
        let part = &self.stories[t.story.0].id.part;
        plan.write(self.pkg.xml_part_mut(part)?, count, None)?;
        self.table_changed(t.story, &plan.path);
        Ok(())
    }

    fn plan_insert_rows(&self, t: TableRef, at: usize, count: usize, template: RowTemplate, tracked: bool) -> Result<RowInsertion> {
        let (path, grid) = self.locate_table(t, false)?;
        let n = grid.rows.len();
        if at > n || count == 0 || n == 0 {
            return Err(Error::InvalidValue { what: "row position", value: format!("{at} (+{count}) of {n}") });
        }
        let ti = match template {
            RowTemplate::Above => at.saturating_sub(1),
            RowTemplate::Below => at.min(n - 1),
        };
        let part = self.stories[t.story.0].id.part.clone();
        let doc = self.pkg.parsed_part(&part).expect("story parts are parsed");
        let tbl = node_at(doc.root(), &path);
        let pos = grid.rows.get(at).map_or_else(|| tbl.children().len(), |r| r.item);
        let trow = &grid.rows[ti];
        let tr = tbl.children()[trow.item].as_element().expect("row");
        if row_revision(tr).is_some() {
            return Err(Error::TrackedPropertyChange { element: "w:trPr" });
        }
        let mut tpath = path.clone();
        tpath.push(trow.item);
        if tracked {
            let index = &self.stories[t.story.0].index;
            index.check_row_revision_range(&tpath)?;
            index.check_row_revision_range(&[path.as_slice(), &[pos]].concat())?;
            check_path(doc.root(), &tpath)?;
            check_revisions(tr)?;
            if find_desc(tr, &|node| range_marker(node).is_some()).is_some() {
                return Err(Error::InsideRevision { context: crate::RunContext::Direct });
            }
        }
        let copy_child = |i: usize, base: &[usize]| -> Vec<usize> { base.iter().copied().chain([i]).collect() };
        let trpr = match tr.indexed_elements().find(|(_, e)| is::<TableRowProperties>(e)) {
            Some((i, _)) => {
                let p = copy_child(i, &tpath);
                Some(doc.lens_at::<TableRowProperties>(&p)?.detach_as::<TableRowProperties>(&[w("tblHeader")])?)
            }
            None => None,
        };
        let tblprex = match tr.indexed_elements().find(|(_, e)| is::<TablePropertyExceptions>(e)) {
            Some((i, e)) => {
                if e.elements().any(|x| x.is(w("tblPrExChange"))) {
                    return Err(Error::TrackedPropertyChange { element: "w:tblPrExChange" });
                }
                let p = copy_child(i, &tpath);
                Some(doc.lens_at::<TablePropertyExceptions>(&p)?.detach_as::<TablePropertyExceptions>(&[])?)
            }
            None => None,
        };
        // the row below the insertion point, for vMerge continuation
        let below = grid.rows.get(at);
        let mut cells = Vec::new();
        for c in &trow.cells {
            let tc = tr.children()[c.item].as_element().expect("cell");
            let mut cpath = tpath.clone();
            cpath.push(c.item);
            let tcpr = match tc.indexed_elements().find(|(_, e)| is::<TableCellProperties>(e)) {
                Some((i, e)) => {
                    for l in ["tcPrChange", "cellIns", "cellDel", "cellMerge"] {
                        if e.elements().any(|x| x.is(w(l))) {
                            return Err(Error::TrackedPropertyChange { element: "w:tcPr" });
                        }
                    }
                    let p = copy_child(i, &cpath);
                    Some(doc.lens_at::<TableCellProperties>(&p)?.detach_as::<TableCellProperties>(&[w("vMerge")])?)
                }
                None => None,
            };
            let continue_merge = c.vmerge.is_some()
                && below.and_then(|b| b.cells.iter().find(|x| x.start == c.start)).is_some_and(|x| x.vmerge == Some(VMerge::Continue));
            let ppr = match tc.indexed_elements().find(|(_, e)| is::<P>(e)) {
                Some((pi, para)) => match para.indexed_elements().find(|(_, e)| is::<ParagraphProperties>(e)) {
                    Some((i, e)) => {
                        if e.elements().find(|x| x.is(w("rPr"))).is_some_and(|m| {
                            m.elements().any(|x| ["ins", "del", "moveFrom", "moveTo", "rPrChange"].iter().any(|l| x.is(w(l))))
                        }) {
                            return Err(Error::RevisionInRange { element: "w:rPr (paragraph mark)".into() });
                        }
                        let mut p = cpath.clone();
                        p.push(pi);
                        p.push(i);
                        Some(doc.lens_at::<ParagraphProperties>(&p)?.detach_as::<ParagraphProperties>(&[w("sectPr"), w("pPrChange"), w("numPr")])?)
                    }
                    None => None,
                },
                None => None,
            };
            cells.push(InsertedCell { tcpr, continue_merge, ppr });
        }
        Ok(RowInsertion { path, pos, trpr, tblprex, cells })
    }

    /// Deletes the rows `range` (ADR-0011 §7.2). A vertical merge whose
    /// `restart` is removed while the first kept row below continues it gets
    /// `restart` there.
    ///
    /// Errors, before any change: as [`Self::insert_rows`];
    /// [`Error::TableWouldBeEmpty`]; [`Error::WouldRemoveNote`];
    /// [`Error::RevisionInRange`]; [`Error::InsideField`];
    /// [`Error::AnchorWouldBreak`] (a range marker whose partner is outside
    /// the removed rows, or any comment anchor or reference).
    pub fn delete_rows(&mut self, t: TableRef, range: Range<usize>) -> Result<()> {
        self.table_gate()?;
        let (path, grid) = self.locate_table(t, false)?;
        let n = grid.rows.len();
        if range.is_empty() || range.end > n {
            return Err(Error::InvalidValue { what: "row range", value: format!("{range:?} of {n}") });
        }
        if range.len() == n {
            return Err(Error::TableWouldBeEmpty);
        }
        let kind = self.stories[t.story.0].id.kind;
        let part = self.stories[t.story.0].id.part.clone();
        let doc = self.pkg.parsed_part(&part).expect("parsed");
        let tbl = node_at(doc.root(), &path);
        let removed: Vec<&RawNode> = grid.rows[range.clone()].iter().map(|r| tbl.children()[r.item].as_element().expect("row")).collect();
        for r in &removed {
            check_removable(r)?;
        }
        check_anchor_pairs(&removed)?;
        // vMerge repairs: (row item, cell item) of continues that become restarts
        let mut repairs = Vec::new();
        if let Some(next) = grid.rows.get(range.end) {
            for c in &next.cells {
                if c.vmerge != Some(VMerge::Continue) {
                    continue;
                }
                // Follow the original chain only through the deleted rows.
                // A gap or a different span ends the chain.
                let boundary = grid.rows[range.clone()].iter().rev().map(|row| {
                    row.cell_at(c.start)
                        .filter(|cell| cell.start == c.start && cell.span == c.span)
                        .and_then(|cell| cell.vmerge)
                }).find(|&merge| merge != Some(VMerge::Continue));
                if boundary == Some(Some(VMerge::Restart)) {
                    let tr = tbl.children()[next.item].as_element().expect("row");
                    if let Some(revision) = row_revision(tr) {
                        return Err(Error::RevisionInRange { element: format!("w:{}", revision.name().local()) });
                    }
                    let tc = tr.children()[c.item].as_element().expect("cell");
                    check_cell_property_revisions(tc)?;
                    repairs.push((next.item, c.item));
                }
            }
        }
        let xml = self.pkg.xml_part_mut(&part)?;
        let mut tl = crate::structure::table_lens(xml, kind, &path)?.ok_or(Error::NoSuchTable(t.index))?;
        for (ri, ci) in &repairs {
            let mut r = tl.nth_child_mut::<TableRow>(grid.rows.iter().position(|x| x.item == *ri).expect("row")).expect("row");
            let cpos = r.node().children()[..*ci].iter().filter(|x| x.as_element().is_some_and(is::<TableCell>)).count();
            let mut c = r.nth_child_mut::<TableCell>(cpos).expect("cell");
            let mut pr = c.child_mut::<TableCellProperties>().expect("tcPr with vMerge");
            pr.child_mut::<VerticalMerge>().expect("vMerge").set_attr(VAL, "restart")?;
        }
        for r in grid.rows[range].iter().rev() {
            tl.remove_at(r.item);
        }
        self.table_changed(t.story, &path);
        Ok(())
    }

    /// Inserts a grid column at `at_grid_col` (0..=column count; ADR-0011
    /// §7.3): a `w:gridCol` of `width` twips, and in each row a new cell at a
    /// cell boundary or the row's end, or a widened span (`gridSpan`,
    /// `gridBefore`, `gridAfter`) when the column falls inside one. The new
    /// cell's `w:tcPr` copies the left neighbour's (the right one's at
    /// column 0) without `gridSpan`, `vMerge`, `hMerge`; its `w:tcW` is the
    /// width. `w:tblW` is unchanged.
    ///
    /// Errors, before any change: as [`Self::insert_rows`];
    /// [`Error::UnsupportedContext`] (`w:hMerge`);
    /// [`Error::RevisionInRange`] (row properties or a widened cell).
    pub fn insert_column(&mut self, t: TableRef, at_grid_col: usize, width: u32) -> Result<()> {
        self.table_gate()?;
        let (path, grid) = self.locate_table(t, true)?;
        if grid.hmerge {
            return Err(Error::UnsupportedContext { ancestor: "w:hMerge" });
        }
        let ncols = grid.cols.len();
        if at_grid_col > ncols {
            return Err(Error::InvalidValue { what: "grid column", value: format!("{at_grid_col} of {ncols}") });
        }
        let kind = self.stories[t.story.0].id.kind;
        let part = self.stories[t.story.0].id.part.clone();
        let doc = self.pkg.parsed_part(&part).expect("parsed");
        let tbl = node_at(doc.root(), &path);
        enum RowEdit {
            NewCell { before_cell: Option<usize>, tcpr: Box<Option<ooxml_core::Detached<TableCellProperties>>>, ppr: Box<Option<ooxml_core::Detached<ParagraphProperties>>> },
            Widen { cell: usize, span: usize, width: Option<u64> },
            Before(usize),
            After(usize),
        }
        let mut edits = Vec::new();
        for row in &grid.rows {
            let tr = tbl.children()[row.item].as_element().expect("row");
            let end = ncols - row.after;
            let edit = if at_grid_col < row.before || (row.cells.is_empty() && at_grid_col <= row.before && row.before > 0) {
                RowEdit::Before(row.before + 1)
            } else if let Some(c) = row.cells.iter().find(|c| c.start < at_grid_col && at_grid_col < c.start + c.span) {
                let tc = tr.children()[c.item].as_element().expect("cell");
                check_cell_property_revisions(tc)?;
                let width = span_width(tc, &grid.cols[c.start..c.start + c.span])
                    .and_then(|old| old.checked_add(u64::from(width)));
                RowEdit::Widen { cell: c.item, span: c.span + 1, width }
            } else if at_grid_col > end {
                RowEdit::After(row.after + 1)
            } else {
                // a boundary: before the cell starting there, or at the row's end
                let next = row.cells.iter().find(|c| c.start == at_grid_col);
                let left = row.cells.iter().rev().find(|c| c.start + c.span == at_grid_col);
                let model = left.or(next).or(row.cells.first());
                let (tcpr, ppr) = match model {
                    Some(m) => {
                        let tc = tr.children()[m.item].as_element().expect("cell");
                        let mut cpath = path.clone();
                        cpath.extend([row.item, m.item]);
                        let tcpr = match tc.indexed_elements().find(|(_, e)| is::<TableCellProperties>(e)) {
                            Some((i, e)) => {
                                for l in ["tcPrChange", "cellIns", "cellDel", "cellMerge"] {
                                    if e.elements().any(|x| x.is(w(l))) {
                                        return Err(Error::TrackedPropertyChange { element: "w:tcPr" });
                                    }
                                }
                                let mut p = cpath.clone();
                                p.push(i);
                                Some(doc.lens_at::<TableCellProperties>(&p)?.detach_as::<TableCellProperties>(&[
                                    w("gridSpan"),
                                    w("vMerge"),
                                    w("hMerge"),
                                    w("tcW"),
                                ])?)
                            }
                            None => None,
                        };
                        let ppr = match tc.indexed_elements().find(|(_, e)| is::<P>(e)) {
                            Some((pi, para)) => match para.indexed_elements().find(|(_, e)| is::<ParagraphProperties>(e)) {
                                Some((i, e)) => {
                                    if e.elements().find(|x| x.is(w("rPr"))).is_some_and(|m| {
                                        m.elements().any(|x| ["ins", "del", "moveFrom", "moveTo", "rPrChange"].iter().any(|l| x.is(w(l))))
                                    }) {
                                        return Err(Error::RevisionInRange { element: "w:rPr (paragraph mark)".into() });
                                    }
                                    let mut p = cpath.clone();
                                    p.extend([pi, i]);
                                    Some(doc.lens_at::<ParagraphProperties>(&p)?.detach_as::<ParagraphProperties>(&[w("sectPr"), w("pPrChange"), w("numPr")])?)
                                }
                                None => None,
                            },
                            None => None,
                        };
                        (tcpr, ppr)
                    }
                    None => (None, None),
                };
                RowEdit::NewCell { before_cell: next.map(|c| c.item), tcpr: Box::new(tcpr), ppr: Box::new(ppr) }
            };
            edits.push((row.item, edit));
        }
        let grid_pos = grid.cols.get(at_grid_col).map(|(i, _)| *i);
        // dry run: the new gridCol
        let xml = self.pkg.xml_part_mut(&part)?;
        {
            let mut tl = crate::structure::table_lens(xml, kind, &path)?.ok_or(Error::NoSuchTable(t.index))?;
            let g = tl.nth_child_mut::<TableGrid>(0).expect("grid");
            let at = grid_pos.unwrap_or(g.node().children().len());
            g.check_insert_at::<GridColumn>(at)?;
        }
        let mut tl = crate::structure::table_lens(xml, kind, &path)?.ok_or(Error::NoSuchTable(t.index))?;
        {
            let mut g = tl.nth_child_mut::<TableGrid>(0).expect("grid");
            let at = grid_pos.unwrap_or(g.node().children().len());
            g.insert_at::<GridColumn>(at)?.set_attr(w("w"), &width.to_string())?;
        }
        for (row_k, (_, edit)) in edits.into_iter().enumerate() {
            let mut row = tl.nth_child_mut::<TableRow>(row_k).expect("row");
            match edit {
                RowEdit::Before(v) => {
                    row.get_or_insert::<TableRowProperties>()?.get_or_insert::<GridBefore>()?.set_attr(VAL, &v.to_string())?;
                }
                RowEdit::After(v) => {
                    row.get_or_insert::<TableRowProperties>()?.get_or_insert::<GridAfter>()?.set_attr(VAL, &v.to_string())?;
                }
                RowEdit::Widen { cell, span, width } => {
                    let k = row.node().children()[..cell].iter().filter(|x| x.as_element().is_some_and(is::<TableCell>)).count();
                    let mut c = row.nth_child_mut::<TableCell>(k).expect("cell");
                    let mut pr = c.child_mut::<TableCellProperties>().expect("tcPr");
                    pr.child_mut::<GridSpan>().expect("gridSpan").set_attr(VAL, &span.to_string())?;
                    if let Some(width) = width {
                        pr.child_mut::<TableCellWidth>().expect("tcW").set_attr(w("w"), &width.to_string())?;
                    }
                }
                RowEdit::NewCell { before_cell, tcpr, ppr } => {
                    let pos = before_cell.unwrap_or_else(|| {
                        row.node().indexed_elements().filter(|(_, e)| is::<TableCell>(e)).last().map_or(row.node().children().len(), |(i, _)| i + 1)
                    });
                    let mut tc = row.insert_at::<TableCell>(pos)?;
                    let mut pr = match &*tcpr {
                        Some(d) => tc.insert_detached(d)?,
                        None => tc.insert::<TableCellProperties>()?,
                    };
                    let mut tcw = pr.get_or_insert::<TableCellWidth>()?;
                    tcw.set_attr(w("w"), &width.to_string())?;
                    tcw.set_attr(w("type"), "dxa")?;
                    let mut p = tc.insert::<P>()?;
                    if let Some(d) = &*ppr {
                        p.insert_detached(d)?;
                    }
                }
            }
        }
        self.table_changed(t.story, &path);
        Ok(())
    }

    /// Deletes grid column `at_grid_col` (ADR-0011 §7.3): its `w:gridCol`,
    /// in each row the cell covering only that column (which must be empty:
    /// one paragraph with nothing but `w:pPr`), or one unit of the span,
    /// `gridBefore` or `gridAfter` covering it.
    ///
    /// Errors, before any change: as [`Self::insert_column`];
    /// [`Error::RevisionInRange`], [`Error::WouldRemoveNote`],
    /// [`Error::InsideField`], [`Error::AnchorWouldBreak`] (comment anchors);
    /// [`Error::CellNotEmpty`]; [`Error::RowWouldBeEmpty`];
    /// [`Error::TableWouldBeEmpty`] (the last grid column).
    pub fn delete_column(&mut self, t: TableRef, at_grid_col: usize) -> Result<()> {
        self.table_gate()?;
        let (path, grid) = self.locate_table(t, true)?;
        if grid.hmerge {
            return Err(Error::UnsupportedContext { ancestor: "w:hMerge" });
        }
        let ncols = grid.cols.len();
        if at_grid_col >= ncols {
            return Err(Error::InvalidValue { what: "grid column", value: format!("{at_grid_col} of {ncols}") });
        }
        if ncols == 1 {
            return Err(Error::TableWouldBeEmpty);
        }
        let kind = self.stories[t.story.0].id.kind;
        let part = self.stories[t.story.0].id.part.clone();
        let doc = self.pkg.parsed_part(&part).expect("parsed");
        let tbl = node_at(doc.root(), &path);
        enum RowEdit {
            Remove(usize),
            Narrow { cell: usize, span: usize, width: Option<u64> },
            Before(usize),
            After(usize),
        }
        let mut edits = Vec::new();
        for row in &grid.rows {
            let tr = tbl.children()[row.item].as_element().expect("row");
            let edit = if at_grid_col < row.before {
                RowEdit::Before(row.before - 1)
            } else if at_grid_col >= ncols - row.after {
                RowEdit::After(row.after - 1)
            } else {
                let c = row.cell_at(at_grid_col).expect("consistent grid");
                let tc = tr.children()[c.item].as_element().expect("cell");
                if c.span > 1 {
                    check_cell_property_revisions(tc)?;
                    let width = span_width(tc, &grid.cols[c.start..c.start + c.span])
                        .zip(grid_width(&grid.cols[at_grid_col]))
                        .and_then(|(old, removed)| old.checked_sub(removed));
                    RowEdit::Narrow { cell: c.item, span: c.span - 1, width }
                } else {
                    check_removable(tc)?;
                    if !cell_is_empty(tc) {
                        return Err(Error::CellNotEmpty);
                    }
                    if row.cells.len() == 1 {
                        return Err(Error::RowWouldBeEmpty);
                    }
                    RowEdit::Remove(c.item)
                }
            };
            edits.push(edit);
        }
        let xml = self.pkg.xml_part_mut(&part)?;
        let mut tl = crate::structure::table_lens(xml, kind, &path)?.ok_or(Error::NoSuchTable(t.index))?;
        {
            let mut g = tl.nth_child_mut::<TableGrid>(0).expect("grid");
            g.remove_at(grid.cols[at_grid_col].0);
        }
        for (k, edit) in edits.into_iter().enumerate() {
            let mut row = tl.nth_child_mut::<TableRow>(k).expect("row");
            let cell_k = |row: &LensMut<'_, TableRow>, item: usize| {
                row.node().children()[..item].iter().filter(|x| x.as_element().is_some_and(is::<TableCell>)).count()
            };
            match edit {
                RowEdit::Remove(item) => {
                    row.remove_at(item);
                }
                RowEdit::Narrow { cell, span, width } => {
                    let ck = cell_k(&row, cell);
                    let mut c = row.nth_child_mut::<TableCell>(ck).expect("cell");
                    let mut pr = c.child_mut::<TableCellProperties>().expect("tcPr");
                    if let Some(width) = width {
                        pr.child_mut::<TableCellWidth>().expect("tcW").set_attr(w("w"), &width.to_string())?;
                    }
                    if span == 1 {
                        pr.remove::<GridSpan>();
                    } else {
                        pr.child_mut::<GridSpan>().expect("gridSpan").set_attr(VAL, &span.to_string())?;
                    }
                }
                RowEdit::Before(v) | RowEdit::After(v) => {
                    let before = matches!(edit, RowEdit::Before(_));
                    let mut pr = row.child_mut::<TableRowProperties>().expect("trPr");
                    match (before, v) {
                        (true, 0) => drop(pr.remove::<GridBefore>()),
                        (true, v) => pr.child_mut::<GridBefore>().expect("gridBefore").set_attr(VAL, &v.to_string())?,
                        (false, 0) => drop(pr.remove::<GridAfter>()),
                        (false, v) => pr.child_mut::<GridAfter>().expect("gridAfter").set_attr(VAL, &v.to_string())?,
                    }
                }
            }
        }
        self.table_changed(t.story, &path);
        Ok(())
    }

    /// Merges the rectangle `rows` × `grid_cols` (ADR-0011 §7.4): in each
    /// row, the first cell gets `gridSpan` = the width and `w:tcW` = the sum
    /// of the grid columns (dxa); the row's other cells of the rectangle are
    /// removed; across rows, the first row's cell gets `vMerge="restart"`
    /// and the cells below `vMerge="continue"`. Every cell but the top-left
    /// must be empty (one paragraph with nothing but `w:pPr`).
    ///
    /// Errors, before any change: as [`Self::insert_column`];
    /// [`Error::MergeOverlaps`] (a cell or vertical merge crosses the
    /// rectangle's edge); [`Error::MergeNonEmpty`]; [`Error::RevisionInRange`],
    /// [`Error::WouldRemoveNote`], [`Error::InsideField`],
    /// [`Error::AnchorWouldBreak`] (comment anchors).
    pub fn merge_cells(&mut self, t: TableRef, rows: Range<usize>, grid_cols: Range<usize>) -> Result<()> {
        self.table_gate()?;
        let (path, grid) = self.locate_table(t, true)?;
        if grid.hmerge {
            return Err(Error::UnsupportedContext { ancestor: "w:hMerge" });
        }
        let (nrows, ncols) = (grid.rows.len(), grid.cols.len());
        if rows.is_empty() || grid_cols.is_empty() || rows.end > nrows || grid_cols.end > ncols || (rows.len() == 1 && grid_cols.len() == 1) {
            return Err(Error::InvalidValue { what: "merge rectangle", value: format!("{rows:?} × {grid_cols:?}") });
        }
        let kind = self.stories[t.story.0].id.kind;
        let part = self.stories[t.story.0].id.part.clone();
        let doc = self.pkg.parsed_part(&part).expect("parsed");
        let tbl = node_at(doc.root(), &path);
        let mut plan = Vec::new(); // per row: (first cell item, other cell items)
        for (k, row) in grid.rows[rows.clone()].iter().enumerate() {
            let tr = tbl.children()[row.item].as_element().expect("row");
            let inside: Vec<&CellModel> = row.cells.iter().filter(|c| c.start < grid_cols.end && c.start + c.span > grid_cols.start).collect();
            let aligned = inside.first().is_some_and(|c| c.start == grid_cols.start)
                && inside.last().is_some_and(|c| c.start + c.span == grid_cols.end)
                && inside.windows(2).all(|p| p[0].start + p[0].span == p[1].start);
            if !aligned {
                return Err(Error::MergeOverlaps);
            }
            // vertical merges crossing the rectangle's top or bottom edge
            for c in &inside {
                if k == 0 && c.vmerge == Some(VMerge::Continue) {
                    return Err(Error::MergeOverlaps);
                }
            }
            for (j, c) in inside.iter().enumerate() {
                let tc = tr.children()[c.item].as_element().expect("cell");
                if j > 0 {
                    check_removable(tc)?;
                } else {
                    check_cell_property_revisions(tc)?;
                    if k > 0 {
                        for ppr in tc.elements().filter(|p| is::<P>(p)).flat_map(RawNode::elements).filter(|ppr| is::<ParagraphProperties>(ppr)) {
                            check_revisions(ppr)?;
                        }
                    }
                }
                if (k, j) != (0, 0) && !cell_is_empty(tc) {
                    return Err(Error::MergeNonEmpty);
                }
            }
            plan.push((row.item, inside.iter().map(|c| c.item).collect::<Vec<_>>()));
        }
        if let Some(next) = grid.rows.get(rows.end)
            && next.cells.iter().any(|c| c.start < grid_cols.end && c.start + c.span > grid_cols.start && c.vmerge == Some(VMerge::Continue))
        {
            return Err(Error::MergeOverlaps);
        }
        let width = grid.cols[grid_cols.clone()].iter().try_fold(0u64, |sum, col| sum.checked_add(grid_width(col)?));
        let row_ks: Vec<usize> = rows.clone().collect();
        let xml = self.pkg.xml_part_mut(&part)?;
        let mut tl = crate::structure::table_lens(xml, kind, &path)?.ok_or(Error::NoSuchTable(t.index))?;
        for (n, (row_k, (_, items))) in row_ks.iter().zip(plan.iter()).enumerate() {
            let mut row = tl.nth_child_mut::<TableRow>(*row_k).expect("row");
            for &item in items[1..].iter().rev() {
                row.remove_at(item);
            }
            let first_k = row.node().children()[..items[0]].iter().filter(|x| x.as_element().is_some_and(is::<TableCell>)).count();
            let mut c = row.nth_child_mut::<TableCell>(first_k).expect("cell");
            let mut pr = c.get_or_insert::<TableCellProperties>()?;
            if grid_cols.len() > 1 {
                if let Some(wd) = width {
                    let mut tcw = pr.get_or_insert::<TableCellWidth>()?;
                    tcw.set_attr(w("w"), &wd.to_string())?;
                    tcw.set_attr(w("type"), "dxa")?;
                }
                pr.get_or_insert::<GridSpan>()?.set_attr(VAL, &grid_cols.len().to_string())?;
            }
            if rows.len() > 1 {
                let mut v = pr.get_or_insert::<VerticalMerge>()?;
                if n == 0 {
                    v.set_attr(VAL, "restart")?;
                } else {
                    v.remove_attr(VAL)?;
                }
            }
        }
        self.table_changed(t.story, &path);
        Ok(())
    }
}


impl TrackedChanges<'_> {
    /// Inserts template rows with row and paragraph-mark insertion revisions.
    pub fn insert_rows(&mut self, t: TableRef, at: usize, count: usize, template: RowTemplate) -> Result<()> {
        self.doc.tracked_write_gate(Operation::Structure)?;
        let plan = self.doc.plan_insert_rows(t, at, count, template, true)?;
        self.edit(t.story, |xml, marks| plan.write(xml, count, Some(marks)))?;
        self.doc.table_changed(t.story, &plan.path);
        Ok(())
    }

    /// Marks rows and their cell content as deleted. They remain in the table's read view.
    pub fn delete_rows(&mut self, t: TableRef, range: Range<usize>) -> Result<()> {
        self.doc.tracked_write_gate(Operation::Structure)?;
        let (path, grid) = self.doc.locate_table(t, false)?;
        let n = grid.rows.len();
        if range.is_empty() || range.end > n {
            return Err(Error::InvalidValue { what: "row range", value: format!("{range:?} of {n}") });
        }
        let root = self.doc.story_root_node(t.story);
        check_path(root, &path)?;
        let table = node_at(root, &path);
        let row_node = |row: &RowModel| table.children()[row.item].as_element().expect("row");
        if !grid.rows.iter().enumerate().any(|(i, row)| {
            !range.contains(&i) && !(Row { node: row_node(row) }).is_deleted()
        }) {
            return Err(Error::TableWouldBeEmpty);
        }
        let rows = &grid.rows[range];
        let removed: Vec<_> = rows.iter().map(row_node).collect();
        check_anchor_pairs(&removed)?;
        let index = &self.doc.stories[t.story.get()].index;
        let mut paragraphs = Vec::new();
        let mut row_paths = Vec::new();
        for (grid_row, row) in rows.iter().zip(removed) {
            let row_path = [path.as_slice(), &[grid_row.item]].concat();
            check_removable(row)?;
            let mut nodes = vec![(row_path, row)];
            while let Some((path, node)) = nodes.pop() {
                for (i, child) in node.indexed_elements() {
                    let child_path = [path.as_slice(), &[i]].concat();
                    match child.name().local() {
                        "p" if is::<P>(child) => {
                            check_deletable_paragraph(child)?;
                            paragraphs.push(child_path);
                        }
                        "tbl" | "tr" | "tc" if child.name().ns() == NS_W => {
                            nodes.push((child_path, child));
                        }
                        "tcPr" | "trPr" | "tblPrEx" | "tblPr" | "tblGrid" if child.name().ns() == NS_W => {}
                        _ => return Err(Error::UnsupportedContext { ancestor: "non-paragraph table cell content" }),
                    }
                }
                if is::<TableRow>(node) {
                    index.check_row_revision_range(&path)?;
                    row_paths.push(path);
                }
            }
        }
        self.edit(t.story, |xml, marks| {
            for path in &paragraphs {
                delete_paragraph_runs(xml, path, marks)?;
                mark_paragraph::<Deleted>(&mut xml.lens_mut_at::<P>(path)?, marks)?;
            }
            // Adding an outer trPr shifts the paths into its cells.
            for row_path in row_paths.iter().rev() {
                let mut row = xml.lens_mut_at::<TableRow>(row_path)?;
                marks.write(row.get_or_insert::<TableRowProperties>()?.insert_with_ns::<Deleted>(REVISION_NS)?)?;
            }
            Ok(())
        })?;
        self.doc.table_changed(t.story, &path);
        Ok(())
    }
}

/// A row retained in a table's read view, including rows marked as deleted.
#[derive(Clone, Copy, Debug)]
pub struct Row<'a> {
    node: &'a RawNode,
}

impl Row<'_> {
    /// Whether this row carries a deletion revision.
    pub fn is_deleted(&self) -> bool {
        self.node.elements().filter(|n| is::<TableRowProperties>(n))
            .flat_map(RawNode::elements).any(is::<Deleted>)
    }
}

impl Document {
    /// Looks up a direct row of a table by its stored index.
    pub fn table_row(&self, table: TableRef, row: usize) -> Option<Row<'_>> {
        let paths = self.table_paths(table.story);
        let path = paths.get(table.index)?;
        let node = node_at(self.story_root_node(table.story), path)
            .elements().filter(|n| is::<TableRow>(n)).nth(row)?;
        Some(Row { node })
    }
}
