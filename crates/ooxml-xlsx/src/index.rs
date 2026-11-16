//! A per-sheet index of `sheetData`: row number → item position, and per
//! row column → item position. Built once per worksheet (one pass, with the
//! same validation `locate` used to repeat on every call) and kept valid by
//! the only code that changes a sheet's structure: the target write of an
//! edit (`edit.rs` apply step v), which reports the row or cell it inserted.
//! Stripping cached values and the shared-formula hand-off change no
//! positions. Lookups are binary searches: O(log rows + log cells-in-row).

use ooxml_core::{ElementSpec, RawNode, XmlDocument};
use ooxml_core::name::MC_ALTERNATE_CONTENT;
use ooxml_schema::sml;

use crate::error::{Error, Result};
use crate::reference::CellIndex;

/// Why the whole sheet cannot be located (the first problem, in document
/// order, exactly as a full scan finds it).
#[derive(Clone, Debug)]
pub(crate) enum SheetErr {
    /// `mc:AlternateContent` in `worksheet` (no `sheetData`) or in `sheetData`.
    Alternate(&'static str),
    MissingRowR,
    BadRowR(String),
    RowsOutOfOrder,
}

/// Why one row's cells cannot be located.
#[derive(Clone, Debug)]
pub(crate) enum RowErr {
    Alternate,
    MissingCellR,
    BadCellR(String),
    WrongRow(String),
    CellsOutOfOrder,
}

#[derive(Debug)]
pub(crate) struct RowEntry {
    /// 1-based row number.
    pub(crate) row: u32,
    /// Item index of the `row` in `sheetData`.
    pub(crate) item: usize,
    /// (0-based column, item index in the row), ascending.
    pub(crate) cells: std::result::Result<Vec<(u32, usize)>, RowErr>,
}

#[derive(Debug)]
pub(crate) struct SheetIndex {
    /// `sheetData` exists.
    pub(crate) sheet_data: bool,
    /// Number of child items of `sheetData` (the append position).
    pub(crate) sd_len: usize,
    pub(crate) rows: std::result::Result<Vec<RowEntry>, SheetErr>,
}

/// Where a cell is (or would go) in `sheetData`; computed read-only.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Located {
    /// Item index of the `row` in `sheetData`, if it exists.
    pub(crate) row: Option<usize>,
    /// Item index of the `c` in the row, if it exists.
    pub(crate) cell: Option<usize>,
    /// Where a new row goes (item index in `sheetData`) if `row` is `None`.
    pub(crate) row_insert_at: usize,
}

fn has_ac(n: &RawNode) -> bool {
    n.elements().any(|c| c.is(MC_ALTERNATE_CONTENT))
}

fn row_cells(row: &RawNode, row_no: u32) -> std::result::Result<Vec<(u32, usize)>, RowErr> {
    if has_ac(row) {
        return Err(RowErr::Alternate);
    }
    let mut out: Vec<(u32, usize)> = Vec::new();
    for (i, c) in row.indexed_elements().filter(|(_, n)| n.is(sml::Cell::QNAME)) {
        let r = c.attr(sml::Cell::ATTR_R).ok_or(RowErr::MissingCellR)?;
        let idx = CellIndex::parse(r).map_err(|_| RowErr::BadCellR(r.to_owned()))?;
        if idx.row + 1 != row_no {
            return Err(RowErr::WrongRow(r.to_owned()));
        }
        if out.last().is_some_and(|&(p, _)| idx.col <= p) {
            return Err(RowErr::CellsOutOfOrder);
        }
        out.push((idx.col, i));
    }
    Ok(out)
}

impl SheetIndex {
    /// One pass over the sheet (ADR-0002 §4.2 rules, Phase0 review F6/N3
    /// alternate-content refusals).
    pub(crate) fn build(doc: &XmlDocument) -> Self {
        let root = doc.root();
        let Some(sd) = root.elements().find(|e| e.is(sml::SheetData::QNAME)) else {
            let rows = if has_ac(root) { Err(SheetErr::Alternate("worksheet")) } else { Ok(Vec::new()) };
            return SheetIndex { sheet_data: false, sd_len: 0, rows };
        };
        let sd_len = sd.children().len();
        if has_ac(sd) {
            return SheetIndex { sheet_data: true, sd_len, rows: Err(SheetErr::Alternate("sheetData")) };
        }
        let mut rows: Vec<RowEntry> = Vec::new();
        for (i, n) in sd.indexed_elements().filter(|(_, n)| n.is(sml::Row::QNAME)) {
            let Some(r) = n.attr(sml::Row::ATTR_R) else {
                return SheetIndex { sheet_data: true, sd_len, rows: Err(SheetErr::MissingRowR) };
            };
            let Some(r) = ooxml_core::value::parse_u32(r).filter(|r| *r >= 1) else {
                return SheetIndex { sheet_data: true, sd_len, rows: Err(SheetErr::BadRowR(r.to_owned())) };
            };
            if rows.last().is_some_and(|e| r <= e.row) {
                return SheetIndex { sheet_data: true, sd_len, rows: Err(SheetErr::RowsOutOfOrder) };
            }
            rows.push(RowEntry { row: r, item: i, cells: row_cells(n, r) });
        }
        SheetIndex { sheet_data: true, sd_len, rows: Ok(rows) }
    }

    fn sheet_error(e: &SheetErr, part: &str) -> Error {
        match e {
            SheetErr::Alternate("worksheet") => ooxml_core::Error::UnresolvedSibling(sml::Worksheet::QNAME).into(),
            SheetErr::Alternate(_) => ooxml_core::Error::UnresolvedSibling(sml::SheetData::QNAME).into(),
            SheetErr::MissingRowR => Error::MissingReference { part: part.to_owned(), what: "row".into() },
            SheetErr::BadRowR(r) => Error::InvalidReference(r.clone()),
            SheetErr::RowsOutOfOrder => Error::OutOfOrder(format!("rows in {part}")),
        }
    }

    fn row_error(e: &RowErr, part: &str, row: u32) -> Error {
        match e {
            RowErr::Alternate => ooxml_core::Error::UnresolvedSibling(sml::Row::QNAME).into(),
            RowErr::MissingCellR => Error::MissingReference { part: part.to_owned(), what: format!("cell in row {row}") },
            RowErr::BadCellR(r) => Error::InvalidReference(r.clone()),
            RowErr::WrongRow(r) => Error::InvalidReference(format!("{r} in row {row}")),
            RowErr::CellsOutOfOrder => Error::OutOfOrder(format!("cells in row {row} of {part}")),
        }
    }

    /// All rows, or the sheet-level error.
    pub(crate) fn rows(&self, part: &str) -> Result<&[RowEntry]> {
        self.rows.as_deref().map_err(|e| Self::sheet_error(e, part))
    }

    /// The cells of a row, or its error.
    pub(crate) fn cells<'a>(&self, entry: &'a RowEntry, part: &str) -> Result<&'a [(u32, usize)]> {
        entry.cells.as_deref().map_err(|e| Self::row_error(e, part, entry.row))
    }

    /// Finds the row and cell (same rules and errors as a full scan), plus
    /// the insertion position of a new row. The cell's own
    /// `mc:AlternateContent` checks are done by the caller on the node.
    pub(crate) fn locate(&self, part: &str, at: CellIndex) -> Result<Located> {
        let rows = self.rows(part)?;
        let row_no = at.row + 1;
        match rows.binary_search_by_key(&row_no, |e| e.row) {
            Err(k) => {
                let row_insert_at = rows.get(k).map_or(self.sd_len, |e| e.item);
                Ok(Located { row: None, cell: None, row_insert_at })
            }
            Ok(k) => {
                let e = &rows[k];
                let cells = self.cells(e, part)?;
                let cell = cells.binary_search_by_key(&at.col, |c| c.0).ok().map(|i| cells[i].1);
                Ok(Located { row: Some(e.item), cell, row_insert_at: e.item })
            }
        }
    }

    /// `sheetData` was inserted (empty) by a write.
    pub(crate) fn sheet_data_created(&mut self) {
        self.sheet_data = true;
        self.sd_len = 0;
        self.rows = Ok(Vec::new());
    }

    /// A row was inserted at item `item` of `sheetData`.
    pub(crate) fn row_inserted(&mut self, row_no: u32, item: usize) {
        self.sd_len += 1;
        if let Ok(rows) = &mut self.rows {
            // rows are ascending in item order too: only the tail shifts
            let from = rows.partition_point(|e| e.item < item);
            for e in &mut rows[from..] {
                e.item += 1;
            }
            let k = rows.partition_point(|e| e.row < row_no);
            rows.insert(k, RowEntry { row: row_no, item, cells: Ok(Vec::new()) });
        }
    }

    /// A cell was inserted at item `item` of the row `row_no`.
    pub(crate) fn cell_inserted(&mut self, row_no: u32, col: u32, item: usize) {
        if let Ok(rows) = &mut self.rows
            && let Ok(k) = rows.binary_search_by_key(&row_no, |e| e.row)
            && let Ok(cells) = &mut rows[k].cells
        {
            let from = cells.partition_point(|c| c.1 < item);
            for c in &mut cells[from..] {
                c.1 += 1;
            }
            let j = cells.partition_point(|c| c.0 < col);
            cells.insert(j, (col, item));
        }
    }
}

/// The `sheetData` element of a worksheet tree.
pub(crate) fn sheet_data(doc: &XmlDocument) -> Option<&RawNode> {
    doc.root().elements().find(|e| e.is(sml::SheetData::QNAME))
}

/// The `c` element at a located position.
pub(crate) fn located_cell<'d>(doc: &'d XmlDocument, loc: &Located) -> Option<&'d RawNode> {
    let (Some(ri), Some(ci)) = (loc.row, loc.cell) else { return None };
    sheet_data(doc)?.children()[ri].as_element()?.children()[ci].as_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    const NS: &str = r#"xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main""#;

    #[test]
    fn index_matches_the_tree_and_follows_inserts() {
        let xml = format!(r#"<worksheet {NS}><sheetData><row r="2"><c r="A2"/><c r="C2"/></row> <row r="5"/></sheetData></worksheet>"#);
        let doc = XmlDocument::parse(xml.as_bytes()).unwrap();
        let mut ix = SheetIndex::build(&doc);
        let a = |s| CellIndex::parse(s).unwrap();
        let l = ix.locate("p", a("C2")).unwrap();
        assert_eq!((l.row, l.cell), (Some(0), Some(1)));
        assert_eq!(ix.locate("p", a("A1")).unwrap().row_insert_at, 0);
        assert_eq!(ix.locate("p", a("A3")).unwrap().row_insert_at, 2);
        assert_eq!(ix.locate("p", a("A9")).unwrap().row_insert_at, 3);
        ix.row_inserted(3, 2);
        assert_eq!(ix.locate("p", a("A5")).unwrap().row, Some(3));
        ix.cell_inserted(2, 1, 1);
        let l = ix.locate("p", a("C2")).unwrap();
        assert_eq!(l.cell, Some(2));
        assert_eq!(ix.locate("p", a("B2")).unwrap().cell, Some(1));
    }

    #[test]
    fn errors_as_a_full_scan() {
        let bad = |sd: &str| {
            let xml = format!(r#"<worksheet {NS}><sheetData>{sd}</sheetData></worksheet>"#);
            SheetIndex::build(&XmlDocument::parse(xml.as_bytes()).unwrap()).locate("p", CellIndex::parse("A1").unwrap())
        };
        assert!(matches!(bad(r#"<row/>"#), Err(Error::MissingReference { .. })));
        assert!(matches!(bad(r#"<row r="2"/><row r="1"/>"#), Err(Error::OutOfOrder(_))));
        assert!(matches!(bad(r#"<row r="1"><c r="A2"/></row>"#), Err(Error::InvalidReference(_))));
        assert!(matches!(bad(r#"<row r="1"><c/></row>"#), Err(Error::MissingReference { .. })));
        assert!(bad(r#"<row r="1"/><row r="2"><c/></row>"#).is_ok(), "another row's problem does not matter");
    }
}
