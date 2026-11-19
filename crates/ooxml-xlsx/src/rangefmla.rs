//! Formulas stored by range features (data validation, conditional
//! formatting, hyperlink locations; ADR-0014 §5, §6, §4): lexed with the
//! Phase1 tokenizer and checked against ADR-0003 §2.4's sheet resolution.

use crate::error::{Error, Result};
use crate::formula::lexer::Prefix;
use crate::formula::parse::{Expr, parse};
use crate::formula::precedents::fold;
use crate::workbook::Workbook;

/// The longest formula a range feature takes (Excel's limit for
/// `formula1`/`formula2`, range references included).
pub(crate) const MAX_LEN: usize = 255;

impl Workbook {
    /// Checks a range-feature formula written for `sheet` and returns it
    /// without a leading `=`: it lexes, fits [`MAX_LEN`], holds only XML
    /// characters, and refers to no other sheet except through a defined
    /// name (`CrossSheetReference`; an unknown sheet is `UnknownSheet`).
    pub(crate) fn check_range_formula(&self, sheet: usize, text: &str) -> Result<String> {
        let t = text.strip_prefix('=').unwrap_or(text).to_owned();
        let bad = |reason: String| Error::InvalidFormula { text: t.clone(), reason };
        if t.is_empty() {
            return Err(bad("empty".into()));
        }
        if t.encode_utf16().count() > MAX_LEN {
            return Err(bad(format!("longer than {MAX_LEN} characters")));
        }
        if let Some((offset, ch)) = ooxml_core::value::first_invalid_xml_char(&t) {
            return Err(ooxml_core::Error::InvalidXmlChar { ch, offset }.into());
        }
        let e = parse(&t).map_err(|e| bad(format!("{} at byte {}", e.message, e.at)))?;
        let own = fold(&self.sheets[sheet].name);
        let mut first = None;
        let mut unsupported = false;
        visit(&e, &mut |expr| {
            let p = match expr {
                Expr::Opaque => {
                    unsupported = true;
                    return;
                }
                Expr::Func { name, .. } if name.starts_with("[QUALIFIED]") => {
                    unsupported = true;
                    return;
                }
                Expr::Ref(r) => &r.prefix,
                Expr::Name { prefix, .. } => prefix,
                _ => return,
            };
            if first.is_some() {
                return;
            }
            let other = match p {
                Prefix::None => return,
                Prefix::Sheet(n) if fold(n) == own => return,
                Prefix::Sheet(n) => n.clone(),
                Prefix::Sheets(a, _) => a.clone(),
                Prefix::External => String::new(),
            };
            first = Some(other);
        });
        if unsupported {
            return Err(bad("opaque reference or sheet-qualified function".into()));
        }
        match first {
            None => Ok(t),
            Some(n) if !n.is_empty() && !self.sheets.iter().any(|s| fold(&s.name) == fold(&n)) => Err(Error::UnknownSheet(n)),
            Some(_) => Err(Error::CrossSheetReference { text: t }),
        }
    }
}

pub(crate) fn visit(e: &Expr, f: &mut impl FnMut(&Expr)) {
    f(e);
    match e {
        Expr::Func { args, .. } | Expr::Union(args) => args.iter().for_each(|a| visit(a, f)),
        Expr::Unary(a) => visit(a, f),
        Expr::Binary(_, a, b) | Expr::Range(a, b) | Expr::Intersect(a, b) => {
            visit(a, f);
            visit(b, f);
        }
        Expr::Ref(_) | Expr::Name { .. } | Expr::Lit | Expr::Missing | Expr::Opaque => {}
    }
}

/// Between comparisons have two operands. Other comparisons have one.
pub(crate) fn check_second_formula(between: bool, has_second: bool) -> Result<()> {
    if between != has_second {
        return Err(Error::InvalidSetting {
            what: "formula2".into(),
            reason: "required for between and notBetween, forbidden for other operators".into(),
        });
    }
    Ok(())
}
