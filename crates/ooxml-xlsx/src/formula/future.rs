//! Excel "future functions" (ADR-0003 addendum P5-pre, ADR-0017 Q7).
//!
//! Functions added to Excel after the 2007 file format are stored with a
//! prefix: `_xlfn.XLOOKUP(…)`, `_xlfn._xlws.FILTER(…)`. Written without it,
//! Excel shows `#NAME?` until the user re-enters the formula. `set_formula`
//! takes the **file form**, so it refuses a known future function without
//! its prefix ([`check`]); [`add_future_prefixes`] converts a formula as a
//! user would type it.
//!
//! The table is a fixed copy of rust_xlsxwriter 0.102's `future_functions`
//! (`src/formula.rs`): kind 0 = `_xlfn.`, 1 = `_xlfn.` (dynamic-array
//! functions), 2 = `_xlfn._xlws.`.

use super::lexer::{Op, TokKind, lex};

/// `(name, kind)`; kind 2 needs `_xlfn._xlws.`.
const FUTURE: &[(&str, u8)] = &[
    ("ACOTH", 0), ("ACOT", 0), ("AGGREGATE", 0), ("ARABIC", 0), ("ARRAYTOTEXT", 0), ("BASE", 0), ("BETA.DIST", 0), ("BETA.INV", 0),
    ("BINOM.DIST.RANGE", 0), ("BINOM.DIST", 0), ("BINOM.INV", 0), ("BITAND", 0), ("BITLSHIFT", 0), ("BITOR", 0), ("BITRSHIFT", 0),
    ("BITXOR", 0), ("CEILING.MATH", 0), ("CEILING.PRECISE", 0), ("CHISQ.DIST.RT", 0), ("CHISQ.DIST", 0), ("CHISQ.INV.RT", 0),
    ("CHISQ.INV", 0), ("CHISQ.TEST", 0), ("COMBINA", 0), ("CONCAT", 0), ("CONFIDENCE.NORM", 0), ("CONFIDENCE.T", 0), ("COTH", 0),
    ("COT", 0), ("COVARIANCE.P", 0), ("COVARIANCE.S", 0), ("CSCH", 0), ("CSC", 0), ("DAYS", 0), ("DECIMAL", 0), ("ERF.PRECISE", 0),
    ("ERFC.PRECISE", 0), ("EXPON.DIST", 0), ("F.DIST.RT", 0), ("F.DIST", 0), ("F.INV.RT", 0), ("F.INV", 0), ("F.TEST", 0),
    ("FIELDVALUE", 0), ("FILTERXML", 0), ("FLOOR.MATH", 0), ("FLOOR.PRECISE", 0), ("FORECAST.ETS.CONFINT", 0),
    ("FORECAST.ETS.SEASONALITY", 0), ("FORECAST.ETS.STAT", 0), ("FORECAST.ETS", 0), ("FORECAST.LINEAR", 0), ("FORMULATEXT", 0),
    ("GAMMA.DIST", 0), ("GAMMA.INV", 0), ("GAMMALN.PRECISE", 0), ("GAMMA", 0), ("GAUSS", 0), ("HYPGEOM.DIST", 0), ("IFNA", 0),
    ("IFS", 0), ("IMAGE", 0), ("IMCOSH", 0), ("IMCOT", 0), ("IMCSCH", 0), ("IMCSC", 0), ("IMSECH", 0), ("IMSEC", 0), ("IMSINH", 0),
    ("IMTAN", 0), ("ISFORMULA", 0), ("ISOMITTED", 0), ("ISOWEEKNUM", 0), ("LET", 0), ("LOGNORM.DIST", 0), ("LOGNORM.INV", 0),
    ("MAXIFS", 0), ("MINIFS", 0), ("MODE.MULT", 0), ("MODE.SNGL", 0), ("MUNIT", 0), ("NEGBINOM.DIST", 0), ("NORM.DIST", 0),
    ("NORM.INV", 0), ("NORM.S.DIST", 0), ("NORM.S.INV", 0), ("NUMBERVALUE", 0), ("PDURATION", 0), ("PERCENTILE.EXC", 0),
    ("PERCENTILE.INC", 0), ("PERCENTRANK.EXC", 0), ("PERCENTRANK.INC", 0), ("PERMUTATIONA", 0), ("PHI", 0), ("POISSON.DIST", 0),
    ("PQSOURCE", 0), ("PYTHON_STR", 0), ("PYTHON_TYPE", 0), ("PYTHON_TYPENAME", 0), ("QUARTILE.EXC", 0), ("QUARTILE.INC", 0),
    ("QUERYSTRING", 0), ("RANK.AVG", 0), ("RANK.EQ", 0), ("RRI", 0), ("SECH", 0), ("SEC", 0), ("SHEETS", 0), ("SHEET", 0),
    ("SKEW.P", 0), ("STDEV.P", 0), ("STDEV.S", 0), ("T.DIST.2T", 0), ("T.DIST.RT", 0), ("T.DIST", 0), ("T.INV.2T", 0), ("T.INV", 0),
    ("T.TEST", 0), ("TEXTAFTER", 0), ("TEXTBEFORE", 0), ("TEXTJOIN", 0), ("UNICHAR", 0), ("UNICODE", 0), ("VALUETOTEXT", 0),
    ("VAR.P", 0), ("VAR.S", 0), ("WEBSERVICE", 0), ("WEIBULL.DIST", 0), ("XMATCH", 0), ("XOR", 0), ("Z.TEST", 0),
    ("ANCHORARRAY", 1), ("BYCOL", 1), ("BYROW", 1), ("CHOOSECOLS", 1), ("CHOOSEROWS", 1), ("DROP", 1), ("EXPAND", 1), ("HSTACK", 1),
    ("LAMBDA", 1), ("MAKEARRAY", 1), ("MAP", 1), ("RANDARRAY", 1), ("REDUCE", 1), ("SCAN", 1), ("SEQUENCE", 1), ("SINGLE", 1),
    ("SORTBY", 1), ("SWITCH", 1), ("TAKE", 1), ("TEXTSPLIT", 1), ("TOCOL", 1), ("TOROW", 1), ("UNIQUE", 1), ("VSTACK", 1),
    ("WRAPCOLS", 1), ("WRAPROWS", 1), ("XLOOKUP", 1), ("FILTER", 2), ("SORT", 2), ("PY", 2),
];

/// The file prefix a future function needs (`_xlfn.` or `_xlfn._xlws.`).
fn prefix_of(bare: &str) -> Option<&'static str> {
    FUTURE.iter().find(|(n, _)| *n == bare).map(|(_, k)| if *k == 2 { "_xlfn._xlws." } else { "_xlfn." })
}

/// A formula `set_formula` refuses (ADR-0003 addendum, P5-pre).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// A known future function without (all of) its prefix.
    MissingPrefix { name: String, prefix: &'static str },
    /// The spill (`A1#`) or implicit-intersection (`@`) operator: the
    /// file forms are `_xlfn.ANCHORARRAY(…)` and `_xlfn.SINGLE(…)`.
    Operator { operator: char, file_form: &'static str },
}

// The lexer uppercases function names. Prefix matching is case-insensitive.
fn split_prefix(name: &str) -> (&'static str, &str) {
    for (upper, canonical) in [("_XLFN._XLWS.", "_xlfn._xlws."), ("_XLFN.", "_xlfn."), ("_XLWS.", "_xlws.")] {
        if let Some(bare) = name.strip_prefix(upper) {
            return (canonical, bare);
        }
    }
    ("", name)
}

/// Checks the file form of a formula. A text that does not lex is left to
/// the caller's own lexing (this returns `Ok`).
pub(crate) fn check(text: &str) -> Result<(), Refusal> {
    let Ok(toks) = lex(text) else { return Ok(()) };
    for t in &toks {
        match &t.kind {
            TokKind::Func(name) => {
                let (have, bare) = split_prefix(name);
                if let Some(need) = prefix_of(bare)
                    && have != need
                {
                    return Err(Refusal::MissingPrefix { name: bare.to_owned(), prefix: need });
                }
            }
            TokKind::Opaque if text[t.start..t.end].ends_with('#') => {
                return Err(Refusal::Operator { operator: '#', file_form: "_xlfn.ANCHORARRAY(…)" });
            }
            TokKind::Op(Op::At) => return Err(Refusal::Operator { operator: '@', file_form: "_xlfn.SINGLE(…)" }),
            _ => {}
        }
    }
    Ok(())
}

/// Adds the file prefixes to the future functions of a formula as a user
/// types it (`XLOOKUP(A1,B:B,C:C)` → `_xlfn.XLOOKUP(A1,B:B,C:C)`, `FILTER`
/// → `_xlfn._xlws.FILTER`). String literals and names that only look like
/// functions are left alone; an existing correct prefix is kept. The spill
/// and `@` operators are not rewritten.
///
/// Errors: the text does not lex (`Err` with the lexer's message).
pub fn add_future_prefixes(text: &str) -> Result<String, String> {
    let toks = lex(text).map_err(|e| format!("{} at byte {}", e.message, e.at))?;
    let mut out = String::with_capacity(text.len() + 16);
    let mut last = 0;
    for t in &toks {
        let TokKind::Func(name) = &t.kind else { continue };
        let (have, bare) = split_prefix(name);
        if let Some(need) = prefix_of(bare)
            && have != need
        {
            out.push_str(&text[last..t.start]);
            out.push_str(need);
            last = t.start + have.len();
        }
    }
    out.push_str(&text[last..]);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_rust_xlsxwriters() {
        assert_eq!(FUTURE.len(), 161, "rust_xlsxwriter 0.102 has 161 entries");
        assert_eq!(prefix_of("FILTER"), Some("_xlfn._xlws."));
        assert_eq!(prefix_of("XLOOKUP"), Some("_xlfn."));
        assert_eq!(prefix_of("SUM"), None);
    }

    #[test]
    fn check_refuses_missing_prefixes_and_dynamic_operators() {
        assert_eq!(check("XLOOKUP(A1,B:B,C:C)"), Err(Refusal::MissingPrefix { name: "XLOOKUP".into(), prefix: "_xlfn." }));
        assert_eq!(check("_xlfn.FILTER(A:A,B:B)"), Err(Refusal::MissingPrefix { name: "FILTER".into(), prefix: "_xlfn._xlws." }));
        assert_eq!(check("SUM(A1#)"), Err(Refusal::Operator { operator: '#', file_form: "_xlfn.ANCHORARRAY(…)" }));
        assert_eq!(check("@A1:A3"), Err(Refusal::Operator { operator: '@', file_form: "_xlfn.SINGLE(…)" }));
        for ok in ["_xlfn.XLOOKUP(A1,B:B,C:C)", "_XLFN._XLWS.SORT(A1:A3)", "SUM(A1:A3)", "\"XLOOKUP(\"&A1", "T1[@Col]", "IF(A1=#N/A,1,2)", "FILTERED+1"] {
            assert_eq!(check(ok), Ok(()), "{ok}");
        }
    }

    #[test]
    fn prefixes_are_added_outside_strings() {
        assert_eq!(add_future_prefixes("XLOOKUP(A1,B:B,C:C)").unwrap(), "_xlfn.XLOOKUP(A1,B:B,C:C)");
        assert_eq!(add_future_prefixes("sum(filter(A:A,B:B))+\"XLOOKUP(\"").unwrap(), "sum(_xlfn._xlws.filter(A:A,B:B))+\"XLOOKUP(\"");
        assert_eq!(add_future_prefixes("_xlfn.FILTER(A:A,B:B)").unwrap(), "_xlfn._xlws.FILTER(A:A,B:B)");
        assert_eq!(add_future_prefixes("_xlfn.IFS(A1,1)").unwrap(), "_xlfn.IFS(A1,1)");
        for t in ["XLOOKUP(A1,B:B,C:C)", "LET(x,1,x+SEQUENCE(3))", "IFS(A1,TEXTJOIN(\",\",TRUE,B:B))"] {
            assert_eq!(check(&add_future_prefixes(t).unwrap()), Ok(()), "{t}");
        }
    }
}
