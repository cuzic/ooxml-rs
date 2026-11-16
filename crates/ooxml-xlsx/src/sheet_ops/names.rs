//! Sheet-name rules, quoting, and the rewriting of sheet prefixes in
//! formulas, locations and `#Sheet!` targets (ADR-0009 §3, §5, §6).

use crate::error::Error;
use crate::formula::lexer::{Prefix, TokKind, lex};
use crate::formula::precedents::fold;

/// ADR-0009 §3 sheet-name rules (uniqueness is checked by the caller).
pub(crate) fn validate_sheet_name(name: &str) -> Result<(), Error> {
    let bad = |reason: &'static str| Err(Error::InvalidSheetName { name: name.to_owned(), reason });
    let units = name.encode_utf16().count();
    if units == 0 {
        return bad("empty");
    }
    if units > 31 {
        return bad("longer than 31 characters");
    }
    if name.chars().any(|c| matches!(c, ':' | '\\' | '/' | '?' | '*' | '[' | ']' | '\u{0}' | '\u{3}')) {
        return bad("contains one of : \\ / ? * [ ] or a control character Excel refuses");
    }
    if name.starts_with('\'') || name.ends_with('\'') {
        return bad("begins or ends with an apostrophe");
    }
    if name.eq_ignore_ascii_case("History") {
        return bad("History is reserved");
    }
    if ooxml_core::value::first_invalid_xml_char(name).is_some() {
        return bad("contains a character XML cannot carry");
    }
    Ok(())
}

fn looks_like_a1(s: &str) -> bool {
    let letters = s.bytes().take_while(u8::is_ascii_alphabetic).count();
    (1..=3).contains(&letters) && s.len() > letters && s.bytes().skip(letters).all(|b| b.is_ascii_digit())
}

fn looks_like_r1c1(s: &str) -> bool {
    let u = s.to_ascii_uppercase();
    let b = u.as_bytes();
    let digits = |from: usize| b[from..].iter().take_while(|c| c.is_ascii_digit()).count();
    match b.first() {
        Some(b'R') => {
            let i = 1 + digits(1);
            i == b.len() || (b[i] == b'C' && i + 1 + digits(i + 1) == b.len())
        }
        Some(b'C') => 1 + digits(1) == b.len(),
        _ => false,
    }
}

/// Does a sheet name need quotes in a formula? Conservative (ADR-0009 §6
/// "Quoting"): unquoted only for `[A-Za-z_][A-Za-z0-9_.]*` that is not an
/// A1 or R1C1 reference, `R`/`C`, `TRUE` or `FALSE`.
pub(crate) fn needs_quotes(name: &str) -> bool {
    let mut chars = name.chars();
    let plain = chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.');
    !plain || looks_like_a1(name) || looks_like_r1c1(name) || name.eq_ignore_ascii_case("TRUE") || name.eq_ignore_ascii_case("FALSE")
}

/// `Name` or `'My Name'` (quotes doubled), for a one-sheet prefix.
pub(crate) fn quote_sheet(name: &str) -> String {
    if needs_quotes(name) { format!("'{}'", name.replace('\'', "''")) } else { name.to_owned() }
}

/// `A:B` or `'A:B'` — a 3-D span is quoted as a whole.
fn quote_span(a: &str, z: &str) -> String {
    if needs_quotes(a) || needs_quotes(z) {
        format!("'{}:{}'", a.replace('\'', "''"), z.replace('\'', "''"))
    } else {
        format!("{a}:{z}")
    }
}

/// What a structural operation does to sheet prefixes.
#[derive(Debug)]
pub(crate) enum PrefixEdit<'a> {
    /// Rename the sheet folding to `old` to `new`.
    Rename { old: String, new: &'a str },
    /// Delete the sheet at `deleted`; `order` is the folded names of every
    /// sheet in workbook order before the delete, `worksheet` whether each is
    /// a worksheet (3-D endpoints move to worksheets only).
    Delete { deleted: usize, order: Vec<String>, names: Vec<String>, worksheet: Vec<bool> },
}

/// A token whose prefix names the sheet in a way this crate cannot rewrite.
#[derive(Debug)]
pub(crate) struct Unrewritable(pub(crate) &'static str);

/// The prefix text of a token (`Sheet1`, `'My Sheet'`, `'A:B'`), parsed into
/// its sheets; `None` for external prefixes.
fn split_prefix(p: &str) -> Option<(String, Option<String>)> {
    let inner = match p.strip_circumfix('\'', '\'') {
        Some(q) => q.replace("''", "'"),
        None => p.to_owned(),
    };
    if inner.starts_with('[') {
        return None;
    }
    match inner.split_once(':') {
        Some((a, z)) => Some((a.to_owned(), Some(z.to_owned()))),
        None => Some((inner, None)),
    }
}

impl PrefixEdit<'_> {
    fn position(&self, name: &str) -> Option<usize> {
        match self {
            PrefixEdit::Delete { order, .. } => {
                let f = fold(name);
                let mut it = order.iter().enumerate().filter(|(_, s)| **s == f);
                match (it.next(), it.next()) {
                    (Some((i, _)), None) => Some(i),
                    _ => None,
                }
            }
            PrefixEdit::Rename { .. } => None,
        }
    }

    /// The new prefix text for an old one; `Ok(None)` = unchanged;
    /// `Ok(Some(None))` = the whole token becomes `#REF!`.
    fn rewrite_prefix(&self, prefix_text: &str, can_ref: bool) -> Result<Option<Option<String>>, Unrewritable> {
        let Some((a, z)) = split_prefix(prefix_text) else { return Ok(None) };
        match self {
            PrefixEdit::Rename { old, new } => {
                let hit = |s: &str| fold(s) == *old;
                match z {
                    None if hit(&a) => Ok(Some(Some(quote_sheet(new)))),
                    None => Ok(None),
                    Some(z) if hit(&a) || hit(&z) => {
                        let a2 = if hit(&a) { (*new).to_owned() } else { a };
                        let z2 = if hit(&z) { (*new).to_owned() } else { z };
                        Ok(Some(Some(quote_span(&a2, &z2))))
                    }
                    Some(_) => Ok(None),
                }
            }
            PrefixEdit::Delete { deleted, names, worksheet, .. } => match z {
                None => match self.position(&a) {
                    Some(p) if p == *deleted => {
                        if can_ref { Ok(Some(None)) } else { Err(Unrewritable("a qualified function call on the deleted sheet")) }
                    }
                    _ => Ok(None),
                },
                Some(z) => {
                    let (Some(pa), Some(pz)) = (self.position(&a), self.position(&z)) else { return Ok(None) };
                    if pa != *deleted && pz != *deleted {
                        return Ok(None); // inside the span, or outside it: the text stays
                    }
                    if !can_ref {
                        return Err(Unrewritable("a qualified function call on the deleted sheet"));
                    }
                    // move each deleted endpoint inward to the next worksheet
                    let step: isize = if pz >= pa { 1 } else { -1 };
                    let inward = |from: usize, to: usize, dir: isize| -> Option<usize> {
                        let mut i = from as isize;
                        loop {
                            if i != *deleted as isize && worksheet[i as usize] {
                                return Some(i as usize);
                            }
                            if i == to as isize {
                                return None;
                            }
                            i += dir;
                        }
                    };
                    let na = if pa == *deleted { inward(pa, pz, step) } else { Some(pa) };
                    let nz = if pz == *deleted { inward(pz, pa, -step) } else { Some(pz) };
                    match (na, nz) {
                        (Some(x), Some(y)) if x == y => Ok(Some(Some(quote_sheet(&names[x])))),
                        (Some(x), Some(y)) => Ok(Some(Some(quote_span(&names[x], &names[y])))),
                        _ => Ok(Some(None)),
                    }
                }
            },
        }
    }
}

/// Rewrites the sheet prefixes of a formula (cell formula, defined name).
/// `Ok(None)`: unchanged. A text that does not lex refuses when it mentions
/// the sheet's name at all.
pub(crate) fn rewrite_formula(text: &str, edit: &PrefixEdit<'_>, mentions: &dyn Fn(&str) -> bool) -> Result<Option<String>, Unrewritable> {
    let Ok(tokens) = lex(text) else {
        return if mentions(text) { Err(Unrewritable("a formula that does not lex")) } else { Ok(None) };
    };
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    let mut changed = false;
    for t in &tokens {
        let tok = &text[t.start..t.end];
        // (prefix length in the token, whole token may become #REF!)
        let region = match &t.kind {
            TokKind::Ref(r) if !matches!(r.prefix, Prefix::None | Prefix::External) => Some((r.area_offset - 1, true)),
            TokKind::Ref(_) => None,
            TokKind::Name { prefix, name } if !matches!(prefix, Prefix::None | Prefix::External) => Some((tok.len() - name.len() - 1, true)),
            TokKind::Func(f) if f.starts_with("[QUALIFIED]") => {
                let n = f.len() - "[QUALIFIED]".len();
                Some((tok.len() - n - 2, false)).filter(|(k, _)| tok.as_bytes().get(*k) == Some(&b'!'))
            }
            _ => None,
        };
        let Some((k, can_ref)) = region else { continue };
        let Some(new_prefix) = edit.rewrite_prefix(&tok[..k], can_ref)? else { continue };
        out.push_str(&text[last..t.start]);
        match new_prefix {
            Some(p) => {
                out.push_str(&p);
                out.push_str(&tok[k..]);
            }
            None => out.push_str("#REF!"),
        }
        last = t.end;
        changed = true;
    }
    if !changed {
        return Ok(None);
    }
    out.push_str(&text[last..]);
    // invariant (ADR-0009 M1): every rewritten text lexes
    if lex(&out).is_err() {
        return Err(Unrewritable("the rewritten formula would not lex"));
    }
    Ok(Some(out))
}

/// Rewrites a location (`Sheet1!A1`, `'My Sheet'!A1:B2`; `hyperlink/@location`,
/// `#…` targets without the `#`). Only renames rewrite locations; `Ok(None)`
/// = unchanged.
pub(crate) fn rewrite_location(loc: &str, edit: &PrefixEdit<'_>) -> Option<String> {
    let PrefixEdit::Rename { .. } = edit else { return None };
    // the prefix ends at the last '!' outside quotes
    let mut in_q = false;
    let mut bang = None;
    let b = loc.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'\'' if in_q && b.get(i + 1) == Some(&b'\'') => i += 1,
            b'\'' => in_q = !in_q,
            b'!' if !in_q => bang = Some(i),
            _ => {}
        }
        i += 1;
    }
    let k = bang?;
    match edit.rewrite_prefix(&loc[..k], true) {
        Ok(Some(Some(p))) => Some(format!("{p}{}", &loc[k..])),
        _ => None,
    }
}

/// Does `text` mention the sheet `name` in sheet-prefix form (`Name!`,
/// `'Name'!`, or as a 3-D endpoint `Name:X!`, `'X:Name'!`)? Both are folded
/// as ADR-0003 rev 4 §2.4 folds; over-matching only refuses more.
pub(crate) fn mentions_prefix(text: &str, name: &str) -> bool {
    let t = fold(text);
    let n = fold(name);
    if n.is_empty() {
        return false;
    }
    let quoted = n.replace('\'', "''");
    let is_name_char = |c: char| c.is_alphanumeric() || matches!(c, '_' | '.' | '\\' | '?');
    for pat in [n.as_str(), quoted.as_str()] {
        let mut from = 0;
        while let Some(off) = t[from..].find(pat) {
            let s = from + off;
            let e = s + pat.len();
            from = s + pat.len().max(1);
            let before = t[..s].chars().next_back();
            let after = &t[e..];
            // quoted forms: '…name…'!  (the quote may be after a span)
            if matches!(before, Some('\'') | Some(':')) {
                let close = after.find("'!");
                if close.is_some_and(|c| c == 0 || after[..c].starts_with(':') && !after[1..c].contains('\'')) {
                    return true;
                }
            }
            if before.is_some_and(is_name_char) {
                continue;
            }
            if after.starts_with('!') {
                return true;
            }
            if let Some(rest) = after.strip_prefix(':') {
                let k = rest.find(|c: char| !is_name_char(c)).unwrap_or(rest.len());
                if k > 0 && rest[k..].starts_with('!') {
                    return true;
                }
            }
            if before == Some(':') && after.starts_with('!') {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rename(old: &str, new: &'static str) -> PrefixEdit<'static> {
        PrefixEdit::Rename { old: fold(old), new }
    }

    fn rw(text: &str, e: &PrefixEdit<'_>) -> Option<String> {
        rewrite_formula(text, e, &|_| true).unwrap()
    }

    #[test]
    fn quoting() {
        for (n, q) in [("Sheet1", false), ("My Sheet", true), ("A1", true), ("R1C1", true), ("R", true), ("c", true), ("TRUE", true), ("_x", false), ("Q1.2024", false), ("1abc", true), ("Данные", true), ("RC", true), ("RCX", false)] {
            assert_eq!(needs_quotes(n), q, "{n}");
        }
        assert_eq!(quote_sheet("It's"), "'It''s'");
    }

    #[test]
    fn rename_rewrites_only_prefixes() {
        let e = rename("Sheet1", "New Name");
        assert_eq!(rw("SUM(Sheet1!A1:B2)+sheet1!$C$3", &e).as_deref(), Some("SUM('New Name'!A1:B2)+'New Name'!$C$3"));
        assert_eq!(rw("Sheet1:Sheet3!A1", &e).as_deref(), Some("'New Name:Sheet3'!A1"));
        assert_eq!(rw("'Sheet1'!Local+Sheet1!#REF!", &e).as_deref(), Some("'New Name'!Local+'New Name'!#REF!"));
        assert_eq!(rw("Sheet1!F(1)", &e).as_deref(), Some("'New Name'!F(1)"));
        assert_eq!(rw("\"Sheet1!A1\"&[1]Sheet1!A1&Sheet11!A1", &e), None);
        let e = rename("My Sheet", "Plain");
        assert_eq!(rw("'my sheet'!A1+'My Sheet:Z'!B2", &e).as_deref(), Some("Plain!A1+Plain:Z!B2"));
        assert!(rewrite_formula("Sheet1!A1&\"x", &rename("Sheet1", "X"), &|t| t.contains("Sheet1")).is_err());
        assert_eq!(rewrite_formula("\"x", &rename("Sheet1", "X"), &|_| false).unwrap(), None);
    }

    #[test]
    fn delete_rewrites_to_ref_errors() {
        let order: Vec<String> = ["s1", "s2", "chart", "s3"].iter().map(|s| fold(s)).collect();
        let names: Vec<String> = ["S1", "S2", "Chart", "S3"].iter().map(|s| s.to_string()).collect();
        let e = |d| PrefixEdit::Delete { deleted: d, order: order.clone(), names: names.clone(), worksheet: vec![true, true, false, true] };
        assert_eq!(rw("SUM(S2!A1:B2)+S2!Name+S2!#REF!+S1!A1", &e(1)).as_deref(), Some("SUM(#REF!)+#REF!+#REF!+S1!A1"));
        // `S2` looks like a cell reference, so sheet names like it are quoted
        assert_eq!(rw("SUM(S1:S3!A1)", &e(0)).as_deref(), Some("SUM('S2:S3'!A1)"));
        assert_eq!(rw("SUM(S1:S3!A1)", &e(3)).as_deref(), Some("SUM('S1:S2'!A1)"), "the chart sheet is skipped");
        assert_eq!(rw("SUM(S1:S3!A1)", &e(1)), None, "inside the span: unchanged");
        assert_eq!(rw("SUM(S2:S3!A1)", &e(1)).as_deref(), Some("SUM('S3'!A1)"));
        assert_eq!(rw("SUM(S3:S2!A1)", &e(1)).as_deref(), Some("SUM('S3'!A1)"));
        assert_eq!(rw("SUM(S2:S2!A1)", &e(1)).as_deref(), Some("SUM(#REF!)"));
        assert!(rewrite_formula("S2!F(1)", &e(1), &|_| true).is_err());
    }

    #[test]
    fn locations_and_mentions() {
        let e = rename("Old", "New Name");
        assert_eq!(rewrite_location("Old!A1", &e).as_deref(), Some("'New Name'!A1"));
        assert_eq!(rewrite_location("'old'!A1:B2", &e).as_deref(), Some("'New Name'!A1:B2"));
        assert_eq!(rewrite_location("Other!A1", &e), None);
        assert_eq!(rewrite_location("MyName", &e), None);
        for t in ["Old!A1", "x+'old'!A1", "Old:X!A1", "'A:Old'!B1", "'A b:Old'!B1", "SUM(X:Old!A1)"] {
            assert!(mentions_prefix(t, "Old"), "{t}");
        }
        for t in ["Older!A1", "XOld!A1", "Old A1", "Old: hello", "\"Old\""] {
            assert!(!mentions_prefix(t, "Old"), "{t}");
        }
        assert!(mentions_prefix("'It''s'!A1", "It's"));
    }
}
