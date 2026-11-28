//! A1-style cell references and ranges.

use std::fmt;

use crate::error::{Error, Result};

/// Number of rows in a worksheet (Excel 2007+).
pub const MAX_ROWS: u32 = 1 << 20;
/// Number of columns in a worksheet (Excel 2007+).
pub const MAX_COLS: u32 = 1 << 14;

/// A cell position, 0-based (`A1` = row 0, column 0).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CellIndex {
    /// 0-based row.
    pub row: u32,
    /// 0-based column.
    pub col: u32,
}

impl CellIndex {
    /// A checked position.
    pub fn new(row: u32, col: u32) -> Result<Self> {
        if row >= MAX_ROWS || col >= MAX_COLS {
            return Err(Error::OutOfRange { row, col });
        }
        Ok(Self { row, col })
    }

    /// Parses `B2` or `$B$2`.
    pub fn parse(s: &str) -> Result<Self> {
        let bad = || Error::InvalidReference(s.to_owned());
        let t = s.strip_prefix('$').unwrap_or(s);
        let letters = t.bytes().take_while(u8::is_ascii_alphabetic).count();
        if letters == 0 || letters > 3 {
            return Err(bad());
        }
        let (col_s, rest) = t.split_at(letters);
        let row_s = rest.strip_prefix('$').unwrap_or(rest);
        if row_s.is_empty() || !row_s.bytes().all(|b| b.is_ascii_digit()) || row_s.starts_with('0') {
            return Err(bad());
        }
        let col = col_s.bytes().fold(0u32, |acc, b| acc * 26 + u32::from(b.to_ascii_uppercase() - b'A' + 1));
        let row: u32 = row_s.parse().map_err(|_| bad())?;
        Self::new(row - 1, col - 1).map_err(|_| bad())
    }

    /// Column letters (`0` → `A`, `26` → `AA`).
    pub fn column_name(col: u32) -> String {
        let mut out = Vec::new();
        let mut n = col + 1;
        while n > 0 {
            out.push(b'A' + ((n - 1) % 26) as u8);
            n = (n - 1) / 26;
        }
        out.reverse();
        String::from_utf8(out).expect("ASCII")
    }
}

impl fmt::Display for CellIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", Self::column_name(self.col), self.row + 1)
    }
}

/// A rectangular range (`B1:E1`), inclusive, or a single cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CellRange {
    /// Top-left.
    pub first: CellIndex,
    /// Bottom-right.
    pub last: CellIndex,
}

impl CellRange {
    /// Parses `A1:D2` or `A1` (`$` allowed). Corners are normalized.
    pub fn parse(s: &str) -> Result<Self> {
        let (a, b) = s.split_once(':').unwrap_or((s, s));
        let (a, b) = (CellIndex::parse(a)?, CellIndex::parse(b)?);
        Ok(Self {
            first: CellIndex { row: a.row.min(b.row), col: a.col.min(b.col) },
            last: CellIndex { row: a.row.max(b.row), col: a.col.max(b.col) },
        })
    }

    /// Does the range contain `c`?
    pub fn contains(&self, c: CellIndex) -> bool {
        (self.first.row..=self.last.row).contains(&c.row) && (self.first.col..=self.last.col).contains(&c.col)
    }

    /// The smallest range containing `self` and `c`.
    pub fn union(&self, c: CellIndex) -> Self {
        Self {
            first: CellIndex { row: self.first.row.min(c.row), col: self.first.col.min(c.col) },
            last: CellIndex { row: self.last.row.max(c.row), col: self.last.col.max(c.col) },
        }
    }
}

impl fmt::Display for CellRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.first == self.last { write!(f, "{}", self.first) } else { write!(f, "{}:{}", self.first, self.last) }
    }
}

/// How workbook-scoped APIs address a sheet (ADR-0023 §2; introduced early
/// for [`crate::Workbook::resolve_defined_name`], the rest of §2 lands in U2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SheetRef<'a> {
    /// Exact match first, then a unique case-insensitive match: the rules of
    /// [`crate::Workbook::worksheet_by_name`] (Excel compares sheet names
    /// case-insensitively).
    Name(&'a str),
    /// 0-based position in `<sheets>` (hidden sheets, chartsheets, macro and
    /// dialog sheets counted), as [`crate::Workbook::worksheet_by_index`] and
    /// `sheet_names()`.
    Index(usize),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a1() {
        assert_eq!(CellIndex::parse("A1").unwrap(), CellIndex { row: 0, col: 0 });
        assert_eq!(CellIndex::parse("$AB$12").unwrap(), CellIndex { row: 11, col: 27 });
        assert_eq!(CellIndex::parse("XFD1048576").unwrap(), CellIndex { row: MAX_ROWS - 1, col: MAX_COLS - 1 });
        for bad in ["", "A", "1", "A0", "XFE1", "A1048577", "A01", "A1B", "AAAA1", "A-1"] {
            assert!(CellIndex::parse(bad).is_err(), "{bad}");
        }
        assert_eq!(CellIndex::column_name(25), "Z");
        assert_eq!(CellIndex::column_name(26), "AA");
        assert_eq!(CellIndex::column_name(16383), "XFD");
        assert_eq!(CellIndex { row: 1, col: 702 }.to_string(), "AAA2");
    }

    #[test]
    fn ranges() {
        let r = CellRange::parse("B1:E15").unwrap();
        assert!(r.contains(CellIndex::parse("C3").unwrap()));
        assert!(!r.contains(CellIndex::parse("A1").unwrap()));
        assert_eq!(r.union(CellIndex::parse("A20").unwrap()).to_string(), "A1:E20");
        assert_eq!(CellRange::parse("A1").unwrap().to_string(), "A1");
        assert_eq!(CellRange::parse("D4:B2").unwrap().to_string(), "B2:D4");
    }
}
