//! Precedent analysis, sound by default (ADR-0003 §2.2–§2.4). A formula gets
//! a precise set of rectangles only if every construct in it is allowlisted;
//! anything else is [`Prec::Everything`].

use super::functions::{self, Rule};
use super::lexer::{Area, Prefix, RefTok};
use super::parse::{Expr, parse};
use super::translate::shift_area;
use crate::reference::{MAX_COLS, MAX_ROWS};

/// A rectangle on a sheet (index into `<sheets>`), inclusive, 0-based.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Rect {
    pub(crate) sheet: usize,
    pub(crate) r1: u32,
    pub(crate) c1: u32,
    pub(crate) r2: u32,
    pub(crate) c2: u32,
}

impl Rect {
    pub(crate) fn cell(sheet: usize, row: u32, col: u32) -> Self {
        Rect { sheet, r1: row, c1: col, r2: row, c2: col }
    }

    pub(crate) fn intersects(&self, o: &Rect) -> bool {
        self.sheet == o.sheet && self.r1 <= o.r2 && o.r1 <= self.r2 && self.c1 <= o.c2 && o.c1 <= self.c2
    }

    pub(crate) fn contains(&self, sheet: usize, row: u32, col: u32) -> bool {
        self.sheet == sheet && (self.r1..=self.r2).contains(&row) && (self.c1..=self.c2).contains(&col)
    }
}

/// A formula's precedents.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Prec {
    Everything,
    Cells(Vec<Rect>),
}

/// Scope of a defined name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Scope {
    Global,
    Local(usize),
    /// `localSheetId` names no sheet.
    Invalid,
}

/// A defined name, parsed once.
#[derive(Debug)]
pub(crate) struct NameDef {
    pub(crate) name_lc: String,
    pub(crate) scope: Scope,
    /// `None`: the text does not lex/parse.
    pub(crate) expr: Option<Expr>,
}

impl NameDef {
    pub(crate) fn new(name: &str, scope: Scope, text: &str) -> Self {
        NameDef { name_lc: name.to_lowercase(), scope, expr: parse(text).ok() }
    }
}

/// What the analysis needs to know about the workbook.
#[derive(Debug, Default)]
pub(crate) struct Env {
    /// Lower-cased sheet names in `<sheets>` order.
    pub(crate) sheets_lc: Vec<String>,
    pub(crate) names: Vec<NameDef>,
    /// Some formula assigns a name (`f/@bx`): every name use is `Everything`.
    pub(crate) bx_any: bool,
}

const MAX_NAME_DEPTH: u32 = 16;

/// "Depends on everything" while walking.
struct Everything;

type R = Result<(), Everything>;

impl Env {
    fn sheet(&self, name: &str) -> Option<usize> {
        let lc = name.to_lowercase();
        self.sheets_lc.iter().position(|s| *s == lc)
    }

    /// Does some defined name have this text (case-insensitive)?
    pub(crate) fn is_name(&self, text: &str) -> bool {
        let lc = text.to_lowercase();
        self.names.iter().any(|n| n.name_lc == lc)
    }

    /// §2.4: resolves a name use. `own`: the using formula's sheet, or the
    /// scope sheet of the name being analysed (`None` inside a global name).
    fn resolve(&self, prefix: &Prefix, name: &str, own: Option<usize>) -> Option<&NameDef> {
        let lc = name.to_lowercase();
        let mut matches = self.names.iter().filter(|n| n.name_lc == lc).peekable();
        matches.peek()?;
        let matches: Vec<&NameDef> = matches.collect();
        if matches.iter().any(|n| n.scope == Scope::Invalid) {
            return None;
        }
        let local = |s: usize| matches.iter().copied().find(|n| n.scope == Scope::Local(s));
        let global = || matches.iter().copied().find(|n| n.scope == Scope::Global);
        match prefix {
            Prefix::None => match own {
                Some(s) => local(s).or_else(global),
                // Inside a global name: ambiguous if any local name shares the text.
                None if matches.iter().any(|n| matches!(n.scope, Scope::Local(_))) => None,
                None => global(),
            },
            Prefix::Sheet(sheet) => local(self.sheet(sheet)?),
            Prefix::Sheets(..) | Prefix::External => None,
        }
    }

    /// Precedents of a formula on sheet `own`, translated by (`dr`, `dc`)
    /// (shared-formula members are analysed from their master's tree).
    pub(crate) fn precedents(&self, expr: &Expr, own: usize, dr: i64, dc: i64) -> Prec {
        let mut out = Vec::new();
        let mut w = Walker { env: self, out: &mut out, own: Some(own), dr, dc, in_name: false, depth: 0 };
        match w.walk(expr) {
            Ok(()) => Prec::Cells(out),
            Err(Everything) => Prec::Everything,
        }
    }
}

struct Walker<'e, 'o> {
    env: &'e Env,
    out: &'o mut Vec<Rect>,
    /// The sheet unqualified references and names refer to (`None` inside a
    /// global name: unqualified references are then `Everything`).
    own: Option<usize>,
    dr: i64,
    dc: i64,
    /// Inside a defined name: every component must be absolute (§2.3 f).
    in_name: bool,
    depth: u32,
}

fn area_rect(sheet: usize, a: &Area) -> Option<Rect> {
    let (r1, c1, r2, c2) = match *a {
        Area::Cell { col, row } => (row.idx, col.idx, row.idx, col.idx),
        Area::Range { c1, r1, c2, r2 } => (r1.idx, c1.idx, r2.idx, c2.idx),
        Area::Cols { c1, c2 } => (0, c1.idx, MAX_ROWS - 1, c2.idx),
        Area::Rows { r1, r2 } => (r1.idx, 0, r2.idx, MAX_COLS - 1),
        Area::RefError => return None,
    };
    Some(Rect { sheet, r1: r1.min(r2), c1: c1.min(c2), r2: r1.max(r2), c2: c1.max(c2) })
}

fn all_absolute(a: &Area) -> bool {
    match *a {
        Area::Cell { col, row } => col.abs && row.abs,
        Area::Range { c1, r1, c2, r2 } => c1.abs && r1.abs && c2.abs && r2.abs,
        Area::Cols { c1, c2 } => c1.abs && c2.abs,
        Area::Rows { r1, r2 } => r1.abs && r2.abs,
        Area::RefError => true,
    }
}

impl<'e> Walker<'e, '_> {
    /// Sheets a reference prefix designates.
    fn sheets(&self, p: &Prefix) -> Result<Vec<usize>, Everything> {
        match p {
            Prefix::None => self.own.map(|s| vec![s]).ok_or(Everything),
            Prefix::Sheet(s) => self.env.sheet(s).map(|s| vec![s]).ok_or(Everything),
            Prefix::Sheets(a, z) => {
                let (a, z) = (self.env.sheet(a).ok_or(Everything)?, self.env.sheet(z).ok_or(Everything)?);
                Ok((a.min(z)..=a.max(z)).collect())
            }
            Prefix::External => Err(Everything),
        }
    }

    /// The rectangles of a reference token (after translation).
    fn ref_rects(&self, r: &RefTok) -> Result<Vec<Rect>, Everything> {
        if let Area::Cell { col, row } = r.area
            && !col.abs
            && !row.abs
            && self.env.is_name(&super::lexer::render_area(&r.area))
        {
            return Err(Everything); // §2.4 name/reference collision (TAX2024)
        }
        if self.in_name && !all_absolute(&r.area) {
            return Err(Everything); // §2.3 (f)
        }
        let area = if self.in_name { r.area } else { shift_area(&r.area, self.dr, self.dc).ok_or(Everything)? };
        let sheets = self.sheets(&r.prefix)?;
        Ok(sheets.into_iter().filter_map(|s| area_rect(s, &area)).collect())
    }

    /// A literal rectangle on one sheet (§2.3 a): a reference token, or a
    /// name whose text is one.
    fn literal_rect(&mut self, e: &Expr) -> Result<Rect, Everything> {
        match e {
            Expr::Ref(r) if !matches!(r.prefix, Prefix::Sheets(..)) => match self.ref_rects(r)?.as_slice() {
                [one] => Ok(*one),
                _ => Err(Everything),
            },
            Expr::Name { prefix, name } => {
                let def = self.name_def(prefix, name)?;
                match &def.expr {
                    Some(Expr::Ref(r)) if !matches!(r.prefix, Prefix::Sheets(..)) => {
                        let mut scratch = Vec::new();
                        let sub = Walker {
                            env: self.env,
                            out: &mut scratch,
                            own: name_own(def),
                            dr: 0,
                            dc: 0,
                            in_name: true,
                            depth: self.depth + 1,
                        };
                        match sub.ref_rects(r)?.as_slice() {
                            [one] => Ok(*one),
                            _ => Err(Everything),
                        }
                    }
                    _ => Err(Everything),
                }
            }
            _ => Err(Everything),
        }
    }

    fn name_def(&self, prefix: &Prefix, name: &str) -> Result<&'e NameDef, Everything> {
        if self.env.bx_any || self.depth >= MAX_NAME_DEPTH {
            return Err(Everything);
        }
        let env: &'e Env = self.env;
        env.resolve(prefix, name, self.own).ok_or(Everything)
    }

    fn walk(&mut self, e: &Expr) -> R {
        match e {
            Expr::Lit | Expr::Missing => Ok(()),
            Expr::Opaque => Err(Everything),
            Expr::Ref(r) => {
                let rects = self.ref_rects(r)?;
                self.out.extend(rects);
                Ok(())
            }
            Expr::Name { prefix, name } => {
                let def = self.name_def(prefix, name)?;
                let expr = def.expr.as_ref().ok_or(Everything)?;
                let mut sub = Walker {
                    env: self.env,
                    out: &mut *self.out,
                    own: name_own(def),
                    dr: 0,
                    dc: 0,
                    in_name: true,
                    depth: self.depth + 1,
                };
                sub.walk(expr)
            }
            Expr::Unary(a) => self.walk(a),
            Expr::Binary(_, a, b) | Expr::Intersect(a, b) => {
                self.walk(a)?;
                self.walk(b)
            }
            Expr::Union(v) => v.iter().try_for_each(|x| self.walk(x)),
            Expr::Range(a, b) => {
                // §2.3 (b): only two literal reference tokens on one sheet.
                let (Expr::Ref(ra), Expr::Ref(rb)) = (&**a, &**b) else { return Err(Everything) };
                let (x, y) = (self.ref_rects(ra)?, self.ref_rects(rb)?);
                match (x.as_slice(), y.as_slice()) {
                    ([x], [y]) if x.sheet == y.sheet => {
                        self.out.push(Rect {
                            sheet: x.sheet,
                            r1: x.r1.min(y.r1),
                            c1: x.c1.min(y.c1),
                            r2: x.r2.max(y.r2),
                            c2: x.c2.max(y.c2),
                        });
                        Ok(())
                    }
                    _ => Err(Everything),
                }
            }
            Expr::Func { name, args } => match functions::rule(name).ok_or(Everything)? {
                Rule::Union => args.iter().try_for_each(|x| self.walk(x)),
                Rule::SumIf => {
                    let (range, criteria, sum) = match args.as_slice() {
                        [r, c] => (r, c, None),
                        [r, c, Expr::Missing] => (r, c, None),
                        [r, c, s] => (r, c, Some(s)),
                        _ => return Err(Everything),
                    };
                    let range_rect = self.literal_rect(range)?;
                    self.out.push(range_rect);
                    self.walk(criteria)?;
                    if let Some(s) = sum {
                        let s = self.literal_rect(s)?;
                        let h = range_rect.r2 - range_rect.r1;
                        let w = range_rect.c2 - range_rect.c1;
                        self.out.push(s);
                        self.out.push(Rect {
                            sheet: s.sheet,
                            r1: s.r1,
                            c1: s.c1,
                            r2: (s.r1 + h).min(MAX_ROWS - 1),
                            c2: (s.c1 + w).min(MAX_COLS - 1),
                        });
                    }
                    Ok(())
                }
            },
        }
    }
}

/// The sheet unqualified uses inside a name refer to.
fn name_own(def: &NameDef) -> Option<usize> {
    match def.scope {
        Scope::Local(s) => Some(s),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env() -> Env {
        Env {
            sheets_lc: ["sheet1", "sheet2", "sheet3", "defines"].map(String::from).to_vec(),
            names: vec![
                NameDef::new("Rate", Scope::Global, "Sheet2!$A$1"),
                NameDef::new("Rate", Scope::Local(0), "Sheet1!$B$1"),
                NameDef::new("Rel", Scope::Global, "Sheet1!A1"),
                NameDef::new("Bare", Scope::Global, "$A$1"),
                NameDef::new("NR", Scope::Local(3), "Defines!$C$3"),
                NameDef::new("NR", Scope::Global, "Sheet1!$C$3"),
                NameDef::new("TAX2024", Scope::Global, "Sheet1!$Z$1"),
                NameDef::new("Loop", Scope::Global, "Loop"),
                NameDef::new("Col", Scope::Global, "Sheet3!$A$1:$A$5"),
                NameDef::new("Bad", Scope::Invalid, "Sheet1!$A$1"),
            ],
            bx_any: false,
        }
    }

    fn p(e: &Env, s: &str, own: usize) -> Prec {
        e.precedents(&parse(s).unwrap(), own, 0, 0)
    }

    fn r(sheet: usize, a: &str) -> Rect {
        let c = crate::reference::CellRange::parse(a).unwrap();
        Rect { sheet, r1: c.first.row, c1: c.first.col, r2: c.last.row, c2: c.last.col }
    }

    #[test]
    fn precise_cases() {
        let e = env();
        assert_eq!(p(&e, "A1+Sheet2!B2", 0), Prec::Cells(vec![r(0, "A1"), r(1, "B2")]));
        assert_eq!(p(&e, "sheet2!B2", 0), Prec::Cells(vec![r(1, "B2")]), "case-insensitive sheets");
        assert_eq!(p(&e, "SUM(Sheet3:Sheet1!A1)", 0), Prec::Cells(vec![r(0, "A1"), r(1, "A1"), r(2, "A1")]));
        assert_eq!(p(&e, "Rate", 0), Prec::Cells(vec![r(0, "B1")]), "local shadows global");
        assert_eq!(p(&e, "rate", 1), Prec::Cells(vec![r(1, "A1")]));
        assert_eq!(p(&e, "Defines!NR", 0), Prec::Cells(vec![r(3, "C3")]));
        assert_eq!(p(&e, "SUMIF(A1:A10,\">0\",B1)", 0), Prec::Cells(vec![r(0, "A1:A10"), r(0, "B1"), r(0, "B1:B10")]));
        assert_eq!(p(&e, "SUMIF(Col,1,Col)", 0), Prec::Cells(vec![r(2, "A1:A5"), r(2, "A1:A5"), r(2, "A1:A5")]));
        assert_eq!(p(&e, "A1:B2:C3", 0), Prec::Cells(vec![r(0, "A1:C3")]));
        assert_eq!(p(&e, "INDEX(A1:A3,2)*NOW()", 0), Prec::Cells(vec![r(0, "A1:A3")]));
        assert_eq!(p(&e, "Sheet1!#REF!+1", 0), Prec::Cells(vec![]));
        assert_eq!(e.precedents(&parse("A1+$B$1").unwrap(), 0, 2, 3), Prec::Cells(vec![r(0, "D3"), r(0, "B1")]));
    }

    #[test]
    fn everything_cases() {
        let e = env();
        for s in [
            "LOOKUP(5,A1:A10,B1)",
            "SUMIF(A1:A10,\">0\",INDEX(B1:B10,5))",
            "SUMIF(INDEX(A:A,1):A3,1)",
            "A1:INDEX(B:B,2)",
            "Rate:Rel",
            "Nope!A1",
            "Sheet1:Nope!A1",
            "Rel",
            "Bare",
            "Defines!Rate",
            "[0]!Rate",
            "[1]Sheet1!A1",
            "TAX2024+1",
            "Loop",
            "Bad",
            "Unknown",
            "myFunc(A1)",
            "INDIRECT(\"A1\")",
            "OFFSET(A1,1,1)",
            "Table1[Col]",
            "A1#",
            "_xlfn.XLOOKUP(1,A:A,B:B)",
            "SUBTOTAL(9,A1:A3)",
        ] {
            assert_eq!(p(&e, s, 0), Prec::Everything, "{s}");
        }
        assert_eq!(e.precedents(&parse("A1").unwrap(), 0, -1, 0), Prec::Everything, "translation leaves the sheet");
        let bx = Env { bx_any: true, ..env() };
        assert_eq!(p(&bx, "Rate", 0), Prec::Everything);
        assert_eq!(p(&bx, "A1", 0), Prec::Cells(vec![r(0, "A1")]));
    }
}
