//! Expression tree over the lexer's tokens (ADR-0003 §2.1). Precedence as in
//! Excel: `:` > space (intersection) > `,` (union, only inside parentheses
//! or at the top level) > prefix `-`/`+`/`@` > `%` > `^` > `* /` > `+ -` >
//! `&` > comparisons.

use super::lexer::{LexError, Op, RefTok, TokKind, Token, lex};
use super::lexer::Prefix;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Expr {
    /// Number, string, boolean, error literal, array constant.
    Lit,
    /// An omitted function argument.
    Missing,
    Ref(RefTok),
    Name { prefix: Prefix, name: String },
    /// Structured, spill or otherwise opaque reference.
    Opaque,
    /// A function call; the name is upper-cased (`_XLFN.IFS`).
    Func { name: String, args: Vec<Expr> },
    Unary(Box<Expr>),
    Binary(Op, Box<Expr>, Box<Expr>),
    /// `:` between two operands that are not one reference token.
    Range(Box<Expr>, Box<Expr>),
    Union(Vec<Expr>),
    Intersect(Box<Expr>, Box<Expr>),
}

/// Lexes and parses a formula as stored in `f` (no leading `=`; a leading
/// `=` is an error, review R3-m2).
pub(crate) fn parse(text: &str) -> Result<Expr, LexError> {
    let toks = lex(text)?;
    let mut p = Parser { toks: &toks, pos: 0 };
    let e = p.expr(0, true)?;
    p.skip_ws();
    if p.pos != toks.len() {
        return Err(p.err("unexpected token"));
    }
    Ok(e)
}

struct Parser<'t> {
    toks: &'t [Token],
    pos: usize,
}

const BP_CMP: u8 = 10;
const BP_CONCAT: u8 = 20;
const BP_ADD: u8 = 30;
const BP_MUL: u8 = 40;
const BP_POW: u8 = 50;
const BP_PREFIX: u8 = 60;
const BP_PERCENT: u8 = 70;
const BP_UNION: u8 = 80;
const BP_ISECT: u8 = 90;
const BP_RANGE: u8 = 100;

impl Parser<'_> {
    fn err(&self, message: &'static str) -> LexError {
        LexError { at: self.toks.get(self.pos).map_or(usize::MAX, |t| t.start), message }
    }

    fn skip_ws(&mut self) {
        while matches!(self.toks.get(self.pos), Some(Token { kind: TokKind::Space, .. })) {
            self.pos += 1;
        }
    }

    fn peek(&mut self) -> Option<&TokKind> {
        self.skip_ws();
        self.toks.get(self.pos).map(|t| &t.kind)
    }

    /// Does the token at `i` start an operand?
    fn starts_operand(&self, i: usize) -> bool {
        matches!(
            self.toks.get(i).map(|t| &t.kind),
            Some(
                TokKind::Number
                    | TokKind::Str
                    | TokKind::Bool
                    | TokKind::ErrorLit
                    | TokKind::Ref(_)
                    | TokKind::Name { .. }
                    | TokKind::Func(_)
                    | TokKind::Opaque
                    | TokKind::Array
                    | TokKind::LParen
            )
        )
    }

    fn expr(&mut self, min_bp: u8, allow_union: bool) -> Result<Expr, LexError> {
        let mut lhs = self.prefix(allow_union)?;
        loop {
            // Intersection: whitespace between two operands.
            if matches!(self.toks.get(self.pos), Some(Token { kind: TokKind::Space, .. })) {
                let mut j = self.pos;
                while matches!(self.toks.get(j), Some(Token { kind: TokKind::Space, .. })) {
                    j += 1;
                }
                if self.starts_operand(j) {
                    if BP_ISECT < min_bp {
                        break;
                    }
                    self.pos = j;
                    let rhs = self.expr(BP_ISECT + 1, allow_union)?;
                    lhs = Expr::Intersect(Box::new(lhs), Box::new(rhs));
                    continue;
                }
            }
            let Some(k) = self.peek().cloned() else { break };
            let (bp, right_assoc) = match &k {
                TokKind::Op(Op::Range) => (BP_RANGE, false),
                TokKind::Op(Op::Percent) => (BP_PERCENT, false),
                TokKind::Op(Op::Pow) => (BP_POW, false),
                TokKind::Op(Op::Mul | Op::Div) => (BP_MUL, false),
                TokKind::Op(Op::Add | Op::Sub) => (BP_ADD, false),
                TokKind::Op(Op::Concat) => (BP_CONCAT, false),
                TokKind::Op(Op::Eq | Op::Ne | Op::Lt | Op::Le | Op::Gt | Op::Ge) => (BP_CMP, false),
                TokKind::Comma if allow_union => (BP_UNION, false),
                _ => break,
            };
            if bp < min_bp {
                break;
            }
            self.pos += 1;
            let next_min = if right_assoc { bp } else { bp + 1 };
            lhs = match k {
                TokKind::Op(Op::Percent) => Expr::Unary(Box::new(lhs)),
                TokKind::Op(Op::Range) => Expr::Range(Box::new(lhs), Box::new(self.expr(next_min, allow_union)?)),
                TokKind::Comma => {
                    let rhs = self.expr(next_min, allow_union)?;
                    match lhs {
                        Expr::Union(mut v) => {
                            v.push(rhs);
                            Expr::Union(v)
                        }
                        l => Expr::Union(vec![l, rhs]),
                    }
                }
                TokKind::Op(op) => Expr::Binary(op, Box::new(lhs), Box::new(self.expr(next_min, allow_union)?)),
                _ => unreachable!(),
            };
        }
        Ok(lhs)
    }

    fn prefix(&mut self, allow_union: bool) -> Result<Expr, LexError> {
        let Some(k) = self.peek().cloned() else { return Err(self.err("expected an operand")) };
        self.pos += 1;
        Ok(match k {
            TokKind::Number | TokKind::Str | TokKind::Bool | TokKind::ErrorLit | TokKind::Array => Expr::Lit,
            TokKind::Ref(r) => Expr::Ref(r),
            TokKind::Name { prefix, name } => Expr::Name { prefix, name },
            TokKind::Opaque => Expr::Opaque,
            TokKind::Op(Op::Sub | Op::Add | Op::At) => Expr::Unary(Box::new(self.expr(BP_PREFIX, allow_union)?)),
            TokKind::LParen => {
                let e = self.expr(0, true)?;
                if self.peek() != Some(&TokKind::RParen) {
                    return Err(self.err("expected ')'"));
                }
                self.pos += 1;
                e
            }
            TokKind::Func(name) => {
                let mut args = Vec::new();
                if self.peek() == Some(&TokKind::RParen) {
                    self.pos += 1;
                    return Ok(Expr::Func { name, args });
                }
                loop {
                    let arg = match self.peek() {
                        Some(TokKind::Comma | TokKind::RParen) => Expr::Missing,
                        _ => self.expr(0, false)?,
                    };
                    args.push(arg);
                    match self.peek() {
                        Some(TokKind::Comma) => self.pos += 1,
                        Some(TokKind::RParen) => {
                            self.pos += 1;
                            break;
                        }
                        _ => return Err(self.err("expected ',' or ')'")),
                    }
                }
                Expr::Func { name, args }
            }
            _ => return Err(self.err("expected an operand")),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shape(e: &Expr) -> String {
        match e {
            Expr::Lit => "lit".into(),
            Expr::Missing => "_".into(),
            Expr::Ref(r) => format!("ref:{}", super::super::lexer::render_area(&r.area)),
            Expr::Name { name, .. } => format!("name:{name}"),
            Expr::Opaque => "opaque".into(),
            Expr::Func { name, args } => format!("{name}({})", args.iter().map(shape).collect::<Vec<_>>().join(",")),
            Expr::Unary(a) => format!("u({})", shape(a)),
            Expr::Binary(op, a, b) => format!("({} {op:?} {})", shape(a), shape(b)),
            Expr::Range(a, b) => format!("({}:{})", shape(a), shape(b)),
            Expr::Union(v) => format!("U[{}]", v.iter().map(shape).collect::<Vec<_>>().join(";")),
            Expr::Intersect(a, b) => format!("({} ^^ {})", shape(a), shape(b)),
        }
    }

    fn p(s: &str) -> String {
        shape(&parse(s).unwrap())
    }

    #[test]
    fn precedence_and_structure() {
        assert_eq!(p("1+2*A1"), "(lit Add (lit Mul ref:A1))");
        assert_eq!(p("-A1^2"), "(u(ref:A1) Pow lit)", "negation binds tighter than ^ in Excel");
        assert_eq!(p("SUM(A1:B2, C3)"), "SUM(ref:A1:B2,ref:C3)");
        assert_eq!(p("SUM((A1,B2),C3)"), "SUM(U[ref:A1;ref:B2],ref:C3)");
        assert_eq!(p("Sheet1!$A$1:$B$2,Sheet1!$D$1"), "U[ref:$A$1:$B$2;ref:$D$1]");
        assert_eq!(p("SUM(A1:B5 B2:C3)"), "SUM((ref:A1:B5 ^^ ref:B2:C3))");
        assert_eq!(p(" A1 + B1 "), "(ref:A1 Add ref:B1)");
        assert_eq!(p("A1:INDEX(B:B,2)"), "(ref:A1:INDEX(ref:B:B,lit))");
        assert_eq!(p("IF(A1>0,,1)"), "IF((ref:A1 Gt lit),_,lit)");
        assert_eq!(p("NOW()"), "NOW()");
        assert_eq!(p("50%"), "u(lit)");
        assert!(parse("=A1").is_err(), "stored text never starts with '='");
        assert_eq!(p("A1&\"x\"<>B1"), "((ref:A1 Concat lit) Ne ref:B1)");
    }

    #[test]
    fn errors() {
        for bad in ["SUM(", "1+", "(1", "A1 )", "SUM(1 2", ",", "1,,2"] {
            assert!(parse(bad).is_err(), "{bad}");
        }
    }
}
