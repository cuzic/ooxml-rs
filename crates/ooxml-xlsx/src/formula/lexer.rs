//! A reference-aware lexer for cell-formula text (ADR-0003 §2.1).
//!
//! It recognises references as whole tokens (`Sheet1!$A$1:B2`, `'Q1 2024'!A:A`,
//! `Sheet1:Sheet3!1:1`, `[1]Book!A1`), keeps the original byte span of every
//! token (translation re-renders only reference tokens and copies everything
//! else), and never panics: anything it does not understand is a [`LexError`],
//! which callers turn into "depends on everything" or a refused translation.

use crate::reference::{MAX_COLS, MAX_ROWS};

/// A row or column component of a reference: 0-based index, `$` or not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Comp {
    pub(crate) idx: u32,
    pub(crate) abs: bool,
}

/// The area part of a reference token (after any sheet prefix).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Area {
    /// `A1`, `$A$1`.
    Cell { col: Comp, row: Comp },
    /// `A1:B2` (as written; corners not normalised).
    Range { c1: Comp, r1: Comp, c2: Comp, r2: Comp },
    /// `A:C` (whole columns).
    Cols { c1: Comp, c2: Comp },
    /// `1:3` (whole rows).
    Rows { r1: Comp, r2: Comp },
    /// `#REF!` after a sheet prefix (`Sheet1!#REF!`): refers to nothing.
    RefError,
}

/// What precedes the `!` of a reference or name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Prefix {
    /// No prefix: the formula's own sheet (or, for names, the usual scope).
    None,
    /// `Sheet1!`, `'My Sheet'!` (unescaped name).
    Sheet(String),
    /// `Sheet1:Sheet3!`, `'Jan:Feb'!`.
    Sheets(String, String),
    /// `[n]…!`: another workbook (or `[0]`, this one).
    External,
}

/// A reference token.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RefTok {
    pub(crate) prefix: Prefix,
    /// Byte offset (in the token) where the area starts; the prefix text is
    /// copied verbatim on translation.
    pub(crate) area_offset: usize,
    pub(crate) area: Area,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Op {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
    Concat,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    /// `:` between two operands that the lexer could not merge into one
    /// reference token (`A1:INDEX(…)`, `Name1:Name2`).
    Range,
    /// Postfix `%`.
    Percent,
    /// Prefix `@` (implicit intersection).
    At,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum TokKind {
    Number,
    Str,
    Bool,
    /// An error literal such as `#N/A`.
    ErrorLit,
    Ref(RefTok),
    /// A defined name, optionally qualified (`Sheet1!Name`, `[0]!Name`).
    Name { prefix: Prefix, name: String },
    /// A function name; the `(` is part of the token.
    Func(String),
    /// A structured reference (`Table1[Col]`, `[@Col]`) or a spill
    /// reference (`A1#`): opaque, depends on everything.
    Opaque,
    /// `{…}`.
    Array,
    Op(Op),
    LParen,
    RParen,
    Comma,
    /// Whitespace (an intersection operator only between two operands).
    Space,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Token {
    pub(crate) kind: TokKind,
    pub(crate) start: usize,
    pub(crate) end: usize,
}

/// Why lexing failed (for messages only).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LexError {
    pub(crate) at: usize,
    pub(crate) message: &'static str,
}

const ERRORS: &[&str] = &[
    "#GETTING_DATA", "#CONNECT!", "#BLOCKED!", "#UNKNOWN!", "#DIV/0!", "#VALUE!", "#SPILL!", "#FIELD!", "#CALC!",
    "#NAME?", "#NULL!", "#BUSY!", "#REF!", "#NUM!", "#N/A",
];

fn is_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\r' | b'\n')
}

/// Characters that may appear in a name, a function name or an unquoted
/// sheet name (besides non-ASCII characters, which all may).
pub(crate) fn is_name_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '.' | '\\' | '?')
}

fn is_name_start(c: char) -> bool {
    c.is_alphabetic() || matches!(c, '_' | '\\')
}

/// Parses `$?LETTERS` at `s`; returns (component, bytes used).
fn col_at(s: &[u8]) -> Option<(Comp, usize)> {
    let abs = s.first() == Some(&b'$');
    let i = usize::from(abs);
    let n = s[i..].iter().take_while(|b| b.is_ascii_alphabetic()).count();
    if n == 0 || n > 3 {
        return None;
    }
    let v = s[i..i + n].iter().fold(0u32, |acc, b| acc * 26 + u32::from(b.to_ascii_uppercase() - b'A' + 1));
    (1..=MAX_COLS).contains(&v).then_some((Comp { idx: v - 1, abs }, i + n))
}

/// Parses `$?DIGITS` at `s`; returns (component, bytes used).
fn row_at(s: &[u8]) -> Option<(Comp, usize)> {
    let abs = s.first() == Some(&b'$');
    let i = usize::from(abs);
    let n = s[i..].iter().take_while(|b| b.is_ascii_digit()).count();
    if n == 0 || n > 7 || s[i] == b'0' {
        return None;
    }
    let v: u32 = std::str::from_utf8(&s[i..i + n]).ok()?.parse().ok()?;
    (1..=MAX_ROWS).contains(&v).then_some((Comp { idx: v - 1, abs }, i + n))
}

fn cell_at(s: &[u8]) -> Option<(Comp, Comp, usize)> {
    let (c, i) = col_at(s)?;
    let (r, j) = row_at(&s[i..])?;
    Some((c, r, i + j))
}

/// Is the byte after a reference a token boundary (so `A1B` or `A1.x` are
/// names, not references)?
fn ends_token(rest: &str) -> bool {
    match rest.chars().next() {
        None => true,
        Some(c) => !(is_name_char(c) || matches!(c, '$' | '[' | '!' | '(' | '\'' | '"')),
    }
}

/// An area (`A1`, `A1:B2`, `A:B`, `1:2`) at the start of `s` that ends at a
/// token boundary; returns (area, bytes used).
pub(crate) fn area_at(s: &str) -> Option<(Area, usize)> {
    let b = s.as_bytes();
    // Longest first: A1:B2, A:B, 1:2, A1.
    if let Some((c1, r1, i)) = cell_at(b) {
        if b.get(i) == Some(&b':')
            && let Some((c2, r2, j)) = cell_at(&b[i + 1..])
            && ends_token(&s[i + 1 + j..])
        {
            return Some((Area::Range { c1, r1, c2, r2 }, i + 1 + j));
        }
        if ends_token(&s[i..]) {
            return Some((Area::Cell { col: c1, row: r1 }, i));
        }
        return None;
    }
    if let Some((c1, i)) = col_at(b)
        && b.get(i) == Some(&b':')
        && let Some((c2, j)) = col_at(&b[i + 1..])
        && ends_token(&s[i + 1 + j..])
    {
        return Some((Area::Cols { c1, c2 }, i + 1 + j));
    }
    if let Some((r1, i)) = row_at(b)
        && b.get(i) == Some(&b':')
        && let Some((r2, j)) = row_at(&b[i + 1..])
        && ends_token(&s[i + 1 + j..])
    {
        return Some((Area::Rows { r1, r2 }, i + 1 + j));
    }
    None
}

/// Lexes a formula (without a leading `=`).
pub(crate) fn lex(text: &str) -> Result<Vec<Token>, LexError> {
    let mut out = Vec::new();
    let b = text.as_bytes();
    let mut i = 0;
    let err = |at: usize, message: &'static str| LexError { at, message };
    while i < b.len() {
        let start = i;
        let rest = &text[i..];
        let c = rest.chars().next().expect("non-empty");
        let kind = match c {
            _ if is_ws(b[i]) => {
                while i < b.len() && is_ws(b[i]) {
                    i += 1;
                }
                out.push(Token { kind: TokKind::Space, start, end: i });
                continue;
            }
            '"' => {
                i += 1;
                loop {
                    match b.get(i) {
                        None => return Err(err(start, "unterminated string")),
                        Some(b'"') if b.get(i + 1) == Some(&b'"') => i += 2,
                        Some(b'"') => {
                            i += 1;
                            break;
                        }
                        Some(_) => i += 1,
                    }
                }
                TokKind::Str
            }
            '{' => {
                i += 1;
                loop {
                    match b.get(i) {
                        None => return Err(err(start, "unterminated array constant")),
                        Some(b'"') => {
                            i += 1;
                            loop {
                                match b.get(i) {
                                    None => return Err(err(start, "unterminated string in array")),
                                    Some(b'"') if b.get(i + 1) == Some(&b'"') => i += 2,
                                    Some(b'"') => {
                                        i += 1;
                                        break;
                                    }
                                    Some(_) => i += 1,
                                }
                            }
                        }
                        Some(b'}') => {
                            i += 1;
                            break;
                        }
                        Some(b'{') => return Err(err(i, "nested array constant")),
                        Some(_) => i += 1,
                    }
                }
                TokKind::Array
            }
            '#' => {
                let e = ERRORS
                    .iter()
                    .find(|e| rest.as_bytes().get(..e.len()).is_some_and(|h| h.eq_ignore_ascii_case(e.as_bytes())))
                    .ok_or_else(|| err(i, "unknown error literal"))?;
                i += e.len();
                TokKind::ErrorLit
            }
            '(' => {
                i += 1;
                TokKind::LParen
            }
            ')' => {
                i += 1;
                TokKind::RParen
            }
            ',' => {
                i += 1;
                TokKind::Comma
            }
            '+' | '-' | '*' | '/' | '^' | '&' | '=' | '%' | '@' => {
                i += 1;
                TokKind::Op(match c {
                    '+' => Op::Add,
                    '-' => Op::Sub,
                    '*' => Op::Mul,
                    '/' => Op::Div,
                    '^' => Op::Pow,
                    '&' => Op::Concat,
                    '=' => Op::Eq,
                    '%' => Op::Percent,
                    _ => Op::At,
                })
            }
            '<' => {
                let (op, n) = match b.get(i + 1) {
                    Some(b'=') => (Op::Le, 2),
                    Some(b'>') => (Op::Ne, 2),
                    _ => (Op::Lt, 1),
                };
                i += n;
                TokKind::Op(op)
            }
            '>' => {
                let (op, n) = if b.get(i + 1) == Some(&b'=') { (Op::Ge, 2) } else { (Op::Gt, 1) };
                i += n;
                TokKind::Op(op)
            }
            ':' => {
                i += 1;
                TokKind::Op(Op::Range)
            }
            '\'' => {
                // Quoted sheet (or external book + sheet) prefix: '…'!
                let mut j = i + 1;
                let mut name = String::new();
                loop {
                    match text[j..].chars().next() {
                        None => return Err(err(start, "unterminated quoted sheet name")),
                        Some('\'') if b.get(j + 1) == Some(&b'\'') => {
                            name.push('\'');
                            j += 2;
                        }
                        Some('\'') => {
                            j += 1;
                            break;
                        }
                        Some(ch) => {
                            name.push(ch);
                            j += ch.len_utf8();
                        }
                    }
                }
                if b.get(j) != Some(&b'!') {
                    return Err(err(j, "quoted sheet name not followed by '!'"));
                }
                j += 1;
                let prefix = if name.starts_with('[') {
                    Prefix::External
                } else if let Some((a, z)) = name.split_once(':') {
                    Prefix::Sheets(a.to_owned(), z.to_owned())
                } else {
                    Prefix::Sheet(name)
                };
                let (kind, n) = after_bang(text, j, prefix, start)?;
                i = j + n;
                kind
            }
            '[' => {
                // `[n]` external prefix, or a structured reference `[…]`.
                let digits = b[i + 1..].iter().take_while(|b| b.is_ascii_digit()).count();
                let external_end = (digits > 0 && b.get(i + 1 + digits) == Some(&b']'))
                    .then(|| {
                        let j = i + 2 + digits;
                        text[j..].find(|ch: char| !(is_name_char(ch) || ch == ':'))
                            .map_or(text.len(), |k| j + k)
                    })
                    .filter(|&k| b.get(k) == Some(&b'!'));
                if let Some(k) = external_end {
                    let j = k + 1;
                    let (kind, n) = after_bang(text, j, Prefix::External, start)?;
                    i = j + n;
                    kind
                } else {
                    i = skip_brackets(b, i).ok_or_else(|| err(start, "unbalanced '['"))?;
                    TokKind::Opaque
                }
            }
            _ if c.is_ascii_digit() || c == '.' || c == '$' => {
                if let Some((area, n)) = area_at(rest) {
                    i += n;
                    spill_or(TokKind::Ref(RefTok { prefix: Prefix::None, area_offset: 0, area }), b, &mut i)
                } else if c == '$' {
                    return Err(err(i, "'$' outside a reference"));
                } else {
                    i += number_len(rest).ok_or_else(|| err(i, "bad number"))?;
                    TokKind::Number
                }
            }
            _ if is_name_start(c) => {
                // Unquoted sheet prefix?  Name chars up to '!' (or a 3-D 'A:B!').
                let k = rest.find(|ch: char| !(is_name_char(ch) || ch == ':')).unwrap_or(rest.len());
                if rest.as_bytes().get(k) == Some(&b'!') && !rest[..k].starts_with(':') {
                    let sheet = &rest[..k];
                    let prefix = match sheet.split_once(':') {
                        Some((a, z)) if !a.is_empty() && !z.is_empty() => Prefix::Sheets(a.to_owned(), z.to_owned()),
                        Some(_) => return Err(err(i, "bad 3-D reference")),
                        None => Prefix::Sheet(sheet.to_owned()),
                    };
                    let j = i + k + 1;
                    let (kind, n) = after_bang(text, j, prefix, start)?;
                    i = j + n;
                    kind
                } else {
                    let name_len = rest.find(|ch: char| !is_name_char(ch)).unwrap_or(rest.len());
                    let next = rest[name_len..].chars().next();
                    if next == Some('(') {
                        // A function, even if shaped like a reference (LOG10).
                        i += name_len + 1;
                        TokKind::Func(rest[..name_len].to_ascii_uppercase())
                    } else if let Some((area, n)) = area_at(rest) {
                        i += n;
                        spill_or(TokKind::Ref(RefTok { prefix: Prefix::None, area_offset: 0, area }), b, &mut i)
                    } else if next == Some('[') {
                        // Table1[Col]
                        i = skip_brackets(b, i + name_len).ok_or_else(|| err(start, "unbalanced '['"))?;
                        TokKind::Opaque
                    } else {
                        let name = &rest[..name_len];
                        i += name_len;
                        if name.eq_ignore_ascii_case("TRUE") || name.eq_ignore_ascii_case("FALSE") {
                            TokKind::Bool
                        } else {
                            TokKind::Name { prefix: Prefix::None, name: name.to_owned() }
                        }
                    }
                }
            }
            _ => return Err(err(i, "unexpected character")),
        };
        out.push(Token { kind, start, end: i });
    }
    Ok(out)
}

/// `A1#`: a spill reference. Its read area is not in the text.
fn spill_or(kind: TokKind, b: &[u8], i: &mut usize) -> TokKind {
    if b.get(*i) == Some(&b'#') && !b.get(*i + 1).is_some_and(|c| c.is_ascii_alphabetic()) {
        *i += 1;
        TokKind::Opaque
    } else {
        kind
    }
}

/// What follows `prefix!` at `j`: an area, `#REF!`, or a name. Returns the
/// token kind and the bytes used after `j`.
fn after_bang(text: &str, j: usize, prefix: Prefix, start: usize) -> Result<(TokKind, usize), LexError> {
    let rest = &text[j..];
    let area_offset = j - start;
    if rest.as_bytes().get(..5).is_some_and(|h| h.eq_ignore_ascii_case(b"#REF!")) {
        return Ok((TokKind::Ref(RefTok { prefix, area_offset, area: Area::RefError }), 5));
    }
    if let Some((area, n)) = area_at(rest) {
        let mut i = j + n;
        let kind = spill_or(TokKind::Ref(RefTok { prefix, area_offset, area }), text.as_bytes(), &mut i);
        return Ok((kind, i - j));
    }
    let n = rest.find(|ch: char| !is_name_char(ch)).unwrap_or(rest.len());
    match rest.chars().next() {
        Some(c) if n > 0 && is_name_start(c) => {
            if rest[n..].starts_with('(') {
                // `[1]!Func(` / `Sheet1!Func(`: an external or macro-sheet
                // function call; never on the value-function table.
                return Ok((TokKind::Func(format!("[QUALIFIED]{}", rest[..n].to_ascii_uppercase())), n + 1));
            }
            Ok((TokKind::Name { prefix, name: rest[..n].to_owned() }, n))
        }
        _ => Err(LexError { at: j, message: "nothing usable after '!'" }),
    }
}

/// Skips a balanced `[…]` group starting at `i`; returns the end offset.
fn skip_brackets(b: &[u8], mut i: usize) -> Option<usize> {
    let mut depth = 0usize;
    while i < b.len() {
        match b[i] {
            b'\'' if depth > 0 => {
                // `'` escapes the next character inside structured references.
                i += 1;
            }
            b'[' => depth += 1,
            b']' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Length of a number literal (`1`, `1.5`, `.5`, `1E+10`).
fn number_len(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    let mut i = b.iter().take_while(|c| c.is_ascii_digit()).count();
    if b.get(i) == Some(&b'.') {
        i += 1;
        i += b[i..].iter().take_while(|c| c.is_ascii_digit()).count();
    }
    if i == 0 || (i == 1 && b[0] == b'.') {
        return None;
    }
    if matches!(b.get(i), Some(b'e' | b'E')) {
        let mut j = i + 1;
        if matches!(b.get(j), Some(b'+' | b'-')) {
            j += 1;
        }
        let d = b[j..].iter().take_while(|c| c.is_ascii_digit()).count();
        if d == 0 {
            return None;
        }
        i = j + d;
    }
    Some(i)
}

/// Renders a component.
pub(crate) fn col_name(c: Comp) -> String {
    format!("{}{}", if c.abs { "$" } else { "" }, crate::reference::CellIndex::column_name(c.idx))
}

pub(crate) fn row_name(r: Comp) -> String {
    format!("{}{}", if r.abs { "$" } else { "" }, r.idx + 1)
}

/// Renders an area as A1 text.
pub(crate) fn render_area(a: &Area) -> String {
    match *a {
        Area::Cell { col, row } => format!("{}{}", col_name(col), row_name(row)),
        Area::Range { c1, r1, c2, r2 } => format!("{}{}:{}{}", col_name(c1), row_name(r1), col_name(c2), row_name(r2)),
        Area::Cols { c1, c2 } => format!("{}:{}", col_name(c1), col_name(c2)),
        Area::Rows { r1, r2 } => format!("{}:{}", row_name(r1), row_name(r2)),
        Area::RefError => "#REF!".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(s: &str) -> Vec<TokKind> {
        lex(s).unwrap().into_iter().map(|t| t.kind).collect()
    }

    fn texts(s: &str) -> Vec<&str> {
        lex(s).unwrap().iter().map(|t| &s[t.start..t.end]).collect()
    }

    fn c(idx: u32, abs: bool) -> Comp {
        Comp { idx, abs }
    }

    #[test]
    fn references_are_whole_tokens() {
        assert_eq!(texts("Sheet1!$A$1:B2+'Q1 2024'!A:A*Sheet1:Sheet3!1:1"), [
            "Sheet1!$A$1:B2",
            "+",
            "'Q1 2024'!A:A",
            "*",
            "Sheet1:Sheet3!1:1"
        ]);
        let k = kinds("'It''s'!B2");
        assert_eq!(k, [TokKind::Ref(RefTok {
            prefix: Prefix::Sheet("It's".into()),
            area_offset: 8,
            area: Area::Cell { col: c(1, false), row: c(1, false) },
        })]);
        let k = kinds("'a!b'!A1");
        assert!(matches!(&k[0], TokKind::Ref(RefTok { prefix: Prefix::Sheet(s), .. }) if s == "a!b"));
        let k = kinds("'Jan:Feb'!$C$3");
        assert!(matches!(&k[0], TokKind::Ref(RefTok { prefix: Prefix::Sheets(a, z), .. }) if a == "Jan" && z == "Feb"));
        assert!(matches!(&kinds("[1]Sheet1!A1")[0], TokKind::Ref(RefTok { prefix: Prefix::External, .. })));
        assert!(matches!(&kinds("'[1]S 1'!A1")[0], TokKind::Ref(RefTok { prefix: Prefix::External, .. })));
        assert!(matches!(&kinds("[0]!Name")[0], TokKind::Name { prefix: Prefix::External, .. }));
        assert!(matches!(&kinds("Sheet1!#REF!")[0], TokKind::Ref(RefTok { area: Area::RefError, .. })));
        assert_eq!(kinds("$A:$C")[0], TokKind::Ref(RefTok {
            prefix: Prefix::None,
            area_offset: 0,
            area: Area::Cols { c1: c(0, true), c2: c(2, true) }
        }));
        assert_eq!(kinds("$1:3")[0], TokKind::Ref(RefTok {
            prefix: Prefix::None,
            area_offset: 0,
            area: Area::Rows { r1: c(0, true), r2: c(2, false) }
        }));
        assert!(matches!(&kinds("Defines!NR_To_A1")[0], TokKind::Name { prefix: Prefix::Sheet(s), name } if s == "Defines" && name == "NR_To_A1"));
    }

    #[test]
    fn functions_names_and_literals() {
        assert_eq!(kinds("LOG10(A1)")[0], TokKind::Func("LOG10".into()));
        assert_eq!(kinds("_xlfn.XLOOKUP(1)")[0], TokKind::Func("_XLFN.XLOOKUP".into()));
        assert!(matches!(&kinds("TAX2024")[0], TokKind::Ref(_)), "a reference token; collisions are decided with the name table");
        assert!(matches!(&kinds("A1B")[0], TokKind::Name { .. }));
        assert!(matches!(&kinds("A1.x")[0], TokKind::Name { .. }));
        assert!(matches!(&kinds("XFE1")[0], TokKind::Name { .. }), "beyond XFD");
        assert!(matches!(&kinds("R1C1")[0], TokKind::Name { .. }));
        assert_eq!(kinds("TRUE")[0], TokKind::Bool);
        assert_eq!(kinds("Table1[[#This Row],[A]]")[0], TokKind::Opaque);
        assert_eq!(kinds("[@Col]")[0], TokKind::Opaque);
        assert_eq!(kinds("A1#")[0], TokKind::Opaque);
        assert_eq!(texts(r#""a""b"&{1,"}";2}"#), [r#""a""b""#, "&", r#"{1,"}";2}"#]);
        assert_eq!(texts("1.5E+3%"), ["1.5E+3", "%"]);
        assert_eq!(texts("#DIV/0!"), ["#DIV/0!"]);
        assert_eq!(texts("A1 <>B1"), ["A1", " ", "<>", "B1"]);
        assert_eq!(texts("A1:INDEX(B:B,2)"), ["A1", ":", "INDEX(", "B:B", ",", "2", ")"]);
    }

    #[test]
    fn errors_never_panic() {
        for bad in ["'abc", "\"abc", "{1,2", "#FOO", "$", "Sheet1!", "[1", "a[b", "~", "1E", "'x'A1", "[1]Sheet1A1"] {
            assert!(lex(bad).is_err(), "{bad}");
        }
        // Arbitrary bytes: no panic.
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let alphabet: Vec<char> = "A1$:!'\"[]{}(),#@%.Ee+-*/^&=<> \u{3042}_\\?xZ9;".chars().collect();
        for _ in 0..20_000 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let len = (seed % 12) as usize;
            let s: String = (0..len)
                .map(|k| alphabet[((seed >> (k * 5)) as usize + k * 7) % alphabet.len()])
                .collect();
            let _ = lex(&s);
        }
    }

    #[test]
    fn render() {
        for s in ["$A$1", "B2:C3", "A:$C", "$1:2", "XFD1048576", "S!#REF!"] {
            match &kinds(s)[0] {
                TokKind::Ref(r) => assert_eq!(render_area(&r.area), s.trim_start_matches("S!")),
                k => panic!("{s}: {k:?}"),
            }
        }
    }
}
