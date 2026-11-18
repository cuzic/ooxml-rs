//! Lexical parsing and formatting of simple-type values, used by generated
//! typed accessors.
//!
//! Parsers return `None` for a value outside the lexical space; callers turn
//! that into an [`AttrError`](crate::AttrError) that keeps the raw string.

use core::fmt::NumBuffer;

/// Strips leading/trailing XML whitespace (space, tab, CR, LF).
pub fn trim_xml_ws(s: &str) -> &str {
    s.trim_matches([' ', '\t', '\r', '\n'])
}

/// Does `s` start or end with XML whitespace?
pub fn has_edge_ws(s: &str) -> bool {
    trim_xml_ws(s).len() != s.len()
}

/// Is `c` an XML 1.0 `Char` (`#x9 | #xA | #xD | [#x20-#xD7FF] |
/// [#xE000-#xFFFD] | [#x10000-#x10FFFF]`)? Surrogates cannot occur in a
/// Rust `char`.
pub const fn is_xml_char(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{FFFD}' | '\u{10000}'..)
}

/// The first character of `s` that is not an XML 1.0 `Char`, with its
/// byte offset.
pub fn first_invalid_xml_char(s: &str) -> Option<(usize, char)> {
    s.char_indices().find(|&(_, c)| !is_xml_char(c))
}

/// Identity parser for string-valued attributes.
pub fn parse_str(s: &str) -> Option<&str> {
    Some(s)
}

/// `xsd:boolean`.
pub fn parse_bool(s: &str) -> Option<bool> {
    match trim_xml_ws(s) {
        "true" | "1" => Some(true),
        "false" | "0" => Some(false),
        _ => None,
    }
}

/// WordprocessingML on/off: `xsd:boolean` plus `on` / `off`.
pub fn parse_on_off(s: &str) -> Option<bool> {
    match trim_xml_ws(s) {
        "true" | "1" | "on" => Some(true),
        "false" | "0" | "off" => Some(false),
        _ => None,
    }
}

/// VML `t` / `f` / `true` / `false`.
pub fn parse_true_false(s: &str) -> Option<bool> {
    match trim_xml_ws(s) {
        "t" | "true" => Some(true),
        "f" | "false" => Some(false),
        _ => None,
    }
}

/// VML true/false that may also be blank: blank reads as `None` inside `Some`.
pub fn parse_true_false_blank(s: &str) -> Option<Option<bool>> {
    match trim_xml_ws(s) {
        "" => Some(None),
        other => parse_true_false(other).map(Some),
    }
}

macro_rules! int_parsers {
    ($($name:ident: $t:ty),*) => {$(
        #[doc = concat!("Parses an integer into `", stringify!($t), "` (XML whitespace ignored).")]
        pub fn $name(s: &str) -> Option<$t> {
            trim_xml_ws(s).parse().ok()
        }
    )*};
}
int_parsers!(parse_u8: u8, parse_u16: u16, parse_u32: u32, parse_u64: u64,
             parse_i8: i8, parse_i16: i16, parse_i32: i32, parse_i64: i64);

/// `xsd:double`: decimal or scientific notation, `INF`, `-INF`, `NaN`.
/// Rust-only spellings such as `inf` or `infinity` are rejected.
pub fn parse_f64(s: &str) -> Option<f64> {
    let t = trim_xml_ws(s);
    match t {
        "INF" | "+INF" => Some(f64::INFINITY),
        "-INF" => Some(f64::NEG_INFINITY),
        "NaN" => Some(f64::NAN),
        _ if crate::schema::const_is_double(t) => t.parse().ok(),
        _ => None,
    }
}

/// `xsd:float`.
pub fn parse_f32(s: &str) -> Option<f32> {
    parse_f64(s).map(|v| v as f32)
}

/// Formats `xsd:boolean` the way Office writes it (`1` / `0`).
pub fn format_bool(v: bool) -> &'static str {
    if v { "1" } else { "0" }
}

/// Formats VML true/false (`t` / `f`).
pub fn format_true_false(v: bool) -> &'static str {
    if v { "t" } else { "f" }
}

/// Formats an `xsd:double` (`INF`, `-INF`, `NaN` for non-finite values).
pub fn format_f64(v: f64) -> String {
    if v.is_nan() {
        "NaN".into()
    } else if v == f64::INFINITY {
        "INF".into()
    } else if v == f64::NEG_INFINITY {
        "-INF".into()
    } else {
        format!("{v}")
    }
}

macro_rules! int_formatters {
    ($($name:ident: $t:ty),*) => {$(
        #[doc = concat!("Formats a `", stringify!($t), "` via `NumBuffer` and passes the text to `f`.")]
        pub fn $name<R>(v: $t, f: impl FnOnce(&str) -> R) -> R {
            let mut buf = NumBuffer::new();
            f(v.format_into(&mut buf))
        }
    )*};
}
int_formatters!(with_u8: u8, with_u16: u16, with_u32: u32, with_u64: u64,
                with_i8: i8, with_i16: i16, with_i32: i32, with_i64: i64);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsing() {
        assert_eq!(parse_bool(" 1 "), Some(true));
        assert_eq!(parse_bool("yes"), None);
        assert_eq!(parse_u32("42"), Some(42));
        assert_eq!(parse_u32("-1"), None);
        assert_eq!(parse_f64("inf"), None);
        assert_eq!(parse_f64("-INF"), Some(f64::NEG_INFINITY));
        assert_eq!(parse_f64("1.5e3"), Some(1500.0));
        assert_eq!(parse_true_false_blank(""), Some(None));
        with_u32(123, |s| assert_eq!(s, "123"));
        with_i32(-7, |s| assert_eq!(s, "-7"));
        assert_eq!(format_f64(2.5), "2.5");
    }
}

/// A DrawingML percentage (`ST_Percentage` and its Transitional unions,
/// ADR-0002 §1.2, ADR-0008 §6): either an integer in thousandths of a
/// percent (`50000` = 50 %, what Office writes) or the literal `"NN%"` form
/// the XSD also allows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DmlPercent {
    /// `val="50000"`: thousandths of a percent.
    Thousandths(i32),
    /// `val="50%"` / `val="-12.5%"`: percent.
    Literal {
        /// The value in percent.
        percent: f64,
    },
}

impl DmlPercent {
    /// The value in percent (`Thousandths(50000)` → 50.0).
    pub fn percent(self) -> f64 {
        match self {
            DmlPercent::Thousandths(t) => f64::from(t) / 1000.0,
            DmlPercent::Literal { percent } => percent,
        }
    }
}

/// Parses a [`DmlPercent`]: an `xsd:int`, or `-?\d+(\.\d+)?%`.
pub fn parse_dml_percent(s: &str) -> Option<DmlPercent> {
    let t = trim_xml_ws(s);
    if let Some(num) = t.strip_suffix('%') {
        return crate::schema::const_is_percent_literal(t).then(|| num.parse().ok()).flatten().map(|percent| DmlPercent::Literal { percent });
    }
    parse_i32(t).map(DmlPercent::Thousandths)
}

/// Formats a [`DmlPercent`] the way Office writes it (thousandths; a
/// literal is rounded) and passes the text to `f`.
pub fn with_dml_percent<R>(v: DmlPercent, f: impl FnOnce(&str) -> R) -> R {
    let t = match v {
        DmlPercent::Thousandths(t) => t,
        DmlPercent::Literal { percent } => (percent * 1000.0).round() as i32,
    };
    with_i32(t, f)
}

