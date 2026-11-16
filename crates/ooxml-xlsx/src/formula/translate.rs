//! Moving a formula text from one cell to another (ADR-0003 §4.2): every
//! relative row/column component of every reference token is shifted; quoted
//! sheet names, strings, array constants, names and structured references
//! are copied unchanged. A component that would leave the sheet, a text that
//! does not lex, or a reference token that collides with a defined name is a
//! failure, never "keep the original".

use super::lexer::{Area, Comp, TokKind, lex, render_area};
use crate::reference::{MAX_COLS, MAX_ROWS};

/// Why a translation failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TranslateError {
    Lex(String),
    OutOfBounds(String),
    NameCollision(String),
}

impl std::fmt::Display for TranslateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TranslateError::Lex(m) => write!(f, "the formula does not lex ({m})"),
            TranslateError::OutOfBounds(r) => write!(f, "{r} would leave the sheet"),
            TranslateError::NameCollision(r) => write!(f, "{r} is both a cell reference and a defined name"),
        }
    }
}

/// Shifts a component; `None` if it leaves the sheet.
pub(crate) fn shift(c: Comp, delta: i64, max: u32) -> Option<Comp> {
    if c.abs {
        return Some(c);
    }
    let v = i64::from(c.idx) + delta;
    (0..i64::from(max)).contains(&v).then_some(Comp { idx: v as u32, abs: false })
}

/// Shifts an area by (rows, cols); `None` if any component leaves the sheet.
pub(crate) fn shift_area(a: &Area, dr: i64, dc: i64) -> Option<Area> {
    let r = |c| shift(c, dr, MAX_ROWS);
    let c = |c| shift(c, dc, MAX_COLS);
    Some(match *a {
        Area::Cell { col, row } => Area::Cell { col: c(col)?, row: r(row)? },
        Area::Range { c1, r1, c2, r2 } => Area::Range { c1: c(c1)?, r1: r(r1)?, c2: c(c2)?, r2: r(r2)? },
        Area::Cols { c1, c2 } => Area::Cols { c1: c(c1)?, c2: c(c2)? },
        Area::Rows { r1, r2 } => Area::Rows { r1: r(r1)?, r2: r(r2)? },
        Area::RefError => Area::RefError,
    })
}

/// Translates `text` by (`dr` rows, `dc` columns). `is_name(token)` says
/// whether an unqualified reference-shaped token is also a defined name.
pub(crate) fn translate(text: &str, dr: i64, dc: i64, is_name: &dyn Fn(&str) -> bool) -> Result<String, TranslateError> {
    let toks = lex(text).map_err(|e| TranslateError::Lex(e.message.into()))?;
    if (dr, dc) == (0, 0) {
        return Ok(text.to_owned());
    }
    let mut out = String::with_capacity(text.len() + 8);
    let mut last = 0;
    for t in &toks {
        let TokKind::Ref(r) = &t.kind else { continue };
        let tok_text = &text[t.start..t.end];
        // A reference-shaped token that is also a defined name, with or
        // without a sheet prefix (`Sheet1!TAX2024`), cannot be moved
        // (ADR-0003 §2.4; review R3-m1).
        if let Area::Cell { col, row } = r.area
            && !col.abs
            && !row.abs
            && is_name(&render_area(&r.area))
        {
            return Err(TranslateError::NameCollision(tok_text.to_owned()));
        }
        let area = shift_area(&r.area, dr, dc).ok_or_else(|| TranslateError::OutOfBounds(tok_text.to_owned()))?;
        out.push_str(&text[last..t.start + r.area_offset]);
        out.push_str(&render_area(&area));
        last = t.end;
    }
    out.push_str(&text[last..]);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tr(s: &str, dr: i64, dc: i64) -> Result<String, TranslateError> {
        translate(s, dr, dc, &|t| t.eq_ignore_ascii_case("TAX2024"))
    }

    #[test]
    fn shifts_relative_components_only() {
        assert_eq!(tr("A1+$B$2+C$3+$D4", 1, 1).unwrap(), "B2+$B$2+D$3+$D5");
        assert_eq!(tr("SUM(A:A,1:1,$A:B)", 2, 1).unwrap(), "SUM(B:B,3:3,$A:C)");
        assert_eq!(tr("'Q1 2024'!A1&\"A1\"", 1, 0).unwrap(), "'Q1 2024'!A2&\"A1\"");
        assert_eq!(tr("'It''s'!A1", 1, 1).unwrap(), "'It''s'!B2");
        assert_eq!(tr("'a!b'!A1", 0, 1).unwrap(), "'a!b'!B1");
        assert_eq!(tr("Sheet1:Sheet3!A1", 1, 0).unwrap(), "Sheet1:Sheet3!A2");
        assert_eq!(tr("[1]Sheet1!A1", 1, 0).unwrap(), "[1]Sheet1!A2");
        assert_eq!(tr("LOG10(A1)", 1, 0).unwrap(), "LOG10(A2)", "LOG10( is a function");
        assert_eq!(tr("INDIRECT(\"A1\")+Table1[Col]+Name1", 5, 5).unwrap(), "INDIRECT(\"A1\")+Table1[Col]+Name1");
        assert_eq!(tr("{1,2}+A1", 1, 0).unwrap(), "{1,2}+A2");
        assert_eq!(tr("Sheet1!#REF!+A1", 1, 0).unwrap(), "Sheet1!#REF!+A2");
        assert_eq!(tr("A1", 0, 0).unwrap(), "A1");
    }

    #[test]
    fn failures() {
        assert!(matches!(tr("A1", -1, 0), Err(TranslateError::OutOfBounds(_))));
        assert!(matches!(tr("XFD1", 0, 1), Err(TranslateError::OutOfBounds(_))));
        assert!(matches!(tr("A1048576", 1, 0), Err(TranslateError::OutOfBounds(_))));
        assert_eq!(tr("$A$1", -5, -5).unwrap(), "$A$1");
        assert!(matches!(tr("TAX2024+A1", 1, 1), Err(TranslateError::NameCollision(_))));
        assert!(matches!(tr("Sheet1!TAX2024+A1", 1, 1), Err(TranslateError::NameCollision(_))));
        assert!(matches!(tr("\"abc", 1, 1), Err(TranslateError::Lex(_))));
    }

    #[test]
    fn composition() {
        // T(M-N) . T(N-O) = T(M-O) when no step fails (ADR-0003 §4.2 step 4).
        let o = "A1+$B1+C$2+SUM(D1:E2)";
        let n = tr(o, 0, 3).unwrap();
        assert_eq!(tr(&n, 4, -1).unwrap(), tr(o, 4, 2).unwrap());
    }
}
