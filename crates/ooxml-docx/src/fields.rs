//! Field-code classification and the `StaleFieldRisk` scan (ADR-0004 §6 W2).
//!
//! Fields are tracked with a stack in document order. A field's code is the
//! `w:instrText` seen while that field is the innermost open one and before
//! its own `separate`. Word writes the result of a field nested in another
//! field's instruction as `w:instrText`; that result arrives after the
//! nested field's `separate`, so it never leaks into the outer code. Every
//! nesting level is classified (review round 1, M1). `INFO`, `DOCPROPERTY`,
//! `BARCODE` and `FORMTEXT` are classified with their arguments (review
//! round 2, N2), and field markup that cannot be followed is reported as
//! [`MALFORMED`] (review round 2, m1).

use std::collections::BTreeSet;

use ooxml_core::RawNode;
use ooxml_core::name::MC_ALTERNATE_CONTENT;
use ooxml_schema::wml::{DeletedFieldCode, FieldChar, FieldCode, Paragraph, SimpleField};

use crate::body::next_element;
use crate::names::{FF_DATA, TEXT_INPUT, TEXT_INPUT_TYPE, VAL, is};

/// Codes whose cached result cannot be made wrong by a Phase0 edit: it
/// depends only on document properties, the date or time, the environment,
/// external sources, mail-merge data, user input or the field's own
/// arguments, or on pagination, which Word recomputes at every layout.
///
/// `INFO` and `DOCPROPERTY` are not listed: they are classified with their
/// first argument ([`field_key`], [`is_safe`]). `BARCODE` is safe only
/// without `\b` (bookmark argument), `FORMTEXT` only when not calculated.
pub const SAFE: &[&str] = &[
    // pagination (recomputed by Word at layout)
    "PAGE", "NUMPAGES", "SECTIONPAGES", "SECTION",
    // date and time
    "DATE", "TIME", "CREATEDATE", "SAVEDATE", "PRINTDATE", "EDITTIME",
    // document properties and environment
    "AUTHOR", "TITLE", "SUBJECT", "KEYWORDS", "COMMENTS", "FILENAME", "LASTSAVEDBY", "TEMPLATE", "DOCVARIABLE",
    "USERNAME", "USERINITIALS", "USERADDRESS", "REVNUM",
    // links and external content
    "HYPERLINK", "INCLUDEPICTURE", "INCLUDETEXT", "LINK", "EMBED",
    // mail merge
    "MERGEFIELD", "MERGEREC", "MERGESEQ", "NEXT", "NEXTIF", "SKIPIF", "GREETINGLINE", "ADDRESSBLOCK",
    // form fields and user input
    "FORMTEXT", "FORMCHECKBOX", "FORMDROPDOWN", "ASK", "FILLIN", "SET",
    // self-contained display fields
    "EQ", "SYMBOL", "MACROBUTTON", "GOTOBUTTON", "ADVANCE", "PRIVATE", "BARCODE", "DISPLAYBARCODE",
];

/// Codes whose cached result depends on the document's content (headings,
/// bookmarks, counts, other fields, the saved file). Listed for
/// documentation and tests: any code not in [`SAFE`] is refused, listed
/// here or not.
pub const CONTENT_DEPENDENT: &[&str] = &[
    "TOC", "REF", "PAGEREF", "NOTEREF", "SEQ", "STYLEREF", "=", "IF", "NUMWORDS", "NUMCHARS", "FILESIZE", "INDEX",
    "TOA", "TC", "XE", "LISTNUM", "CITATION", "BIBLIOGRAPHY", "QUOTE", "RD", "COMPARE", "AUTONUM", "AUTONUMLGL",
    "AUTONUMOUT",
];

/// Document statistics (`docProps/app.xml` names and their `INFO`
/// spellings): `DOCPROPERTY` / `INFO` with one of these as argument returns
/// the same counts as `NUMWORDS` / `NUMCHARS` and is refused. `PAGES` /
/// `NUMPAGES` are pagination and stay safe.
pub const STATISTICS: &[&str] = &[
    "NUMWORDS", "NUMCHARS", "FILESIZE", "WORDS", "CHARACTERS", "CHARACTERSWITHSPACES", "LINES", "PARAGRAPHS",
    "BYTES",
];

/// Reported for a field whose own instruction contains `w:delInstrText`:
/// the old and new codes are interleaved, so the field is not classified.
pub const REVISED_INSTRUCTION: &str = "<revised instruction>";

/// Reported for field markup ooxml-rs cannot follow (review round 2, m1): a
/// `w:fldChar` whose `w:fldCharType` is missing or not `begin`/`separate`/
/// `end`, a `separate`/`end` or `w:instrText` with no open field, or a
/// `w:fldChar` inside an `mc:AlternateContent` that sits within a paragraph
/// (each branch would be counted).
pub const MALFORMED: &str = "<malformed field>";

/// Key of a calculated text form field (`w:ffData/w:textInput/w:type
/// w:val="calculated"`): its result is an expression over other form
/// fields' bookmarks.
pub const CALCULATED_FORM_FIELD: &str = "FORMTEXT (calculated)";

/// Splits an instruction into its code (see [`field_code`]) and the rest.
fn split_code(instruction: &str) -> (String, &str) {
    let s = instruction.trim_matches(|c: char| matches!(c, ' ' | '\t' | '\r' | '\n'));
    if let Some(rest) = s.strip_prefix('=') {
        return ("=".into(), rest);
    }
    let end = s.find(|c: char| c.is_whitespace() || c == '\\').unwrap_or(s.len());
    (s[..end].to_uppercase(), &s[end..])
}

/// The field code of an instruction: `=` for a formula, else the first
/// token (up to whitespace or `\`), upper-cased. Empty for an empty
/// instruction (which is not safe).
pub fn field_code(instruction: &str) -> String {
    split_code(instruction).0
}

/// The first argument after the code: a quoted string without its quotes,
/// or a token up to whitespace or `\`. `None` if the instruction goes
/// straight to a switch, ends, or has an unterminated quote.
fn first_argument(rest: &str) -> Option<&str> {
    let rest = rest.trim_start();
    let arg = if let Some(q) = rest.strip_prefix('"') {
        &q[..q.find('"')?]
    } else {
        &rest[..rest.find(|c: char| c.is_whitespace() || c == '\\').unwrap_or(rest.len())]
    };
    let arg = arg.trim();
    (!arg.is_empty()).then_some(arg)
}

/// The classification key of an instruction (review round 2, N2): the code
/// ([`field_code`]), except
///
/// - `INFO` and `DOCPROPERTY`: `"<CODE> <ARGUMENT>"` with the first
///   argument upper-cased and unquoted (`INFO NUMWORDS`,
///   `DOCPROPERTY CHARACTERS`); the bare code when there is no argument
///   (a nested field as argument, say), which is refused;
/// - `BARCODE` with a `\b` switch (its argument is a bookmark):
///   `BARCODE \b`, refused.
pub fn field_key(instruction: &str) -> String {
    let (code, rest) = split_code(instruction);
    match code.as_str() {
        "INFO" | "DOCPROPERTY" => match first_argument(rest) {
            Some(arg) => format!("{code} {}", arg.to_uppercase()),
            None => code,
        },
        "BARCODE" if rest.split_whitespace().any(|t| t == "\\b") => "BARCODE \\b".into(),
        _ => code,
    }
}

/// Is this key (from [`field_key`]) safe under Phase0?
///
/// - `INFO X` is safe when `X` is itself a safe plain code (`INFO NumPages`,
///   `INFO Author`); `INFO NumWords`, `INFO FileSize` are not.
/// - `DOCPROPERTY X` is safe unless `X` is one of the [`STATISTICS`].
///   Custom properties linked to a bookmark are refused separately (they
///   need `docProps/custom.xml`).
/// - Everything else: listed in [`SAFE`].
pub fn is_safe(key: &str) -> bool {
    if let Some(arg) = key.strip_prefix("INFO ") {
        return !matches!(arg, "INFO" | "DOCPROPERTY") && !arg.contains(' ') && SAFE.contains(&arg);
    }
    if let Some(arg) = key.strip_prefix("DOCPROPERTY ") {
        return !STATISTICS.contains(&arg);
    }
    SAFE.contains(&key)
}

struct Open {
    instruction: String,
    separated: bool,
    revised: bool,
    calculated: bool,
}

/// What encloses the node being visited, innermost last: used to tell a
/// `w:fldChar` in a paragraph-level `mc:AlternateContent` from one in a
/// paragraph of a text box inside it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Scope {
    Paragraph,
    AlternateContent,
}

/// Collects the key ([`field_key`]) of every field in a story part (complex
/// fields at every nesting level and `w:fldSimple`), in document order.
/// Unterminated fields are classified too. The walk is iterative, so the
/// depth of the part costs heap, not stack.
#[derive(Default)]
pub(crate) struct FieldScan {
    stack: Vec<Open>,
    codes: Vec<String>,
    scopes: Vec<Scope>,
}

impl FieldScan {
    pub(crate) fn scan(root: &RawNode) -> Vec<String> {
        let mut s = FieldScan::default();
        // (node, index of the next child to visit, pushed a scope)
        let mut walk: Vec<(&RawNode, usize, bool)> = vec![(root, 0, s.enter(root))];
        while let Some((node, next, _)) = walk.last_mut() {
            match next_element(node, *next) {
                Some((i, child)) => {
                    *next = i + 1;
                    let scoped = s.enter(child);
                    walk.push((child, 0, scoped));
                }
                None => {
                    if let Some((_, _, true)) = walk.pop() {
                        s.scopes.pop();
                    }
                }
            }
        }
        for open in std::mem::take(&mut s.stack) {
            s.finish(open);
        }
        s.codes
    }

    fn finish(&mut self, open: Open) {
        let key = match () {
            () if open.revised => REVISED_INSTRUCTION.into(),
            () if open.calculated && field_code(&open.instruction) == "FORMTEXT" => CALCULATED_FORM_FIELD.into(),
            () => field_key(&open.instruction),
        };
        self.codes.push(key);
    }

    /// Visits `node` before its children; returns whether a scope was
    /// pushed (popped after the children).
    fn enter(&mut self, node: &RawNode) -> bool {
        if is::<SimpleField>(node) {
            self.codes.push(field_key(node.attr(SimpleField::ATTR_W_INSTR).unwrap_or_default()));
        } else if is::<FieldChar>(node) {
            if self.scopes.last() == Some(&Scope::AlternateContent) {
                self.codes.push(MALFORMED.into());
            }
            match node.attr(FieldChar::ATTR_W_FLD_CHAR_TYPE) {
                Some("begin") => self.stack.push(Open {
                    instruction: String::new(),
                    separated: false,
                    revised: false,
                    calculated: is_calculated(node),
                }),
                Some("separate") => match self.stack.last_mut() {
                    Some(top) => top.separated = true,
                    None => self.codes.push(MALFORMED.into()),
                },
                Some("end") => match self.stack.pop() {
                    Some(open) => self.finish(open),
                    None => self.codes.push(MALFORMED.into()),
                },
                _ => self.codes.push(MALFORMED.into()),
            }
        } else if is::<FieldCode>(node) || is::<DeletedFieldCode>(node) {
            match self.stack.last_mut() {
                Some(top) if !top.separated => {
                    top.instruction.push_str(&node.text());
                    top.revised |= is::<DeletedFieldCode>(node);
                }
                Some(_) => {}
                None => self.codes.push(MALFORMED.into()),
            }
        } else if is::<Paragraph>(node) {
            self.scopes.push(Scope::Paragraph);
            return true;
        } else if node.is(MC_ALTERNATE_CONTENT) && !self.scopes.is_empty() {
            // Inside a paragraph (directly or in a run). A text box's own
            // paragraphs inside it push `Paragraph` again, so only markup
            // between this element and the next `w:p` is affected.
            self.scopes.push(Scope::AlternateContent);
            return true;
        }
        false
    }
}

/// `w:fldChar/w:ffData/w:textInput/w:type w:val="calculated"`.
fn is_calculated(fld_char: &RawNode) -> bool {
    fld_char
        .elements()
        .filter(|c| c.is(FF_DATA))
        .flat_map(|d| d.elements().filter(|c| c.is(TEXT_INPUT)))
        .flat_map(|t| t.elements().filter(|c| c.is(TEXT_INPUT_TYPE)))
        .any(|t| t.attr(VAL) == Some("calculated"))
}

/// The refused keys of a scan: sorted, without duplicates. `linked` holds
/// the upper-cased names of custom properties linked to content
/// (`docProps/custom.xml` `linkTarget`); `DOCPROPERTY` of one is refused.
pub(crate) fn refused(codes: impl IntoIterator<Item = String>, linked: &[String]) -> Vec<String> {
    codes
        .into_iter()
        .filter(|c| !is_safe(c) || c.strip_prefix("DOCPROPERTY ").is_some_and(|a| linked.iter().any(|l| l == a)))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes() {
        assert_eq!(field_code(" PAGE \\* MERGEFORMAT "), "PAGE");
        assert_eq!(field_code("=SUM(ABOVE)"), "=");
        assert_eq!(field_code(" = 1+1 "), "=");
        assert_eq!(field_code("toc \\o \"1-3\""), "TOC");
        assert_eq!(field_code("REF\\h"), "REF");
        assert_eq!(field_code(""), "");
        assert!(!is_safe(""));
        assert_eq!(field_key(" INFO NumWords "), "INFO NUMWORDS");
        assert_eq!(field_key("DOCPROPERTY  \"Characters\" \\* MERGEFORMAT"), "DOCPROPERTY CHARACTERS");
        assert_eq!(field_key("DOCPROPERTY \"Client Name\""), "DOCPROPERTY CLIENT NAME");
        assert_eq!(field_key("DOCPROPERTY \\* MERGEFORMAT"), "DOCPROPERTY");
        assert_eq!(field_key("DOCPROPERTY \"unterminated"), "DOCPROPERTY");
        assert_eq!(field_key("BARCODE \\b addr \\u"), "BARCODE \\b");
        assert_eq!(field_key("BARCODE \"98052\" \\u"), "BARCODE");
        for safe in ["INFO NUMPAGES", "INFO AUTHOR", "DOCPROPERTY PAGES", "DOCPROPERTY TITLE", "DOCPROPERTY CLIENT NAME", "BARCODE"] {
            assert!(is_safe(safe), "{safe}");
        }
        for refused in ["INFO", "DOCPROPERTY", "INFO NUMWORDS", "INFO FILESIZE", "INFO TOC", "INFO INFO", "INFO DOCPROPERTY", "DOCPROPERTY WORDS", "DOCPROPERTY BYTES", "FILESIZE", "BARCODE \\b", CALCULATED_FORM_FIELD, MALFORMED] {
            assert!(!is_safe(refused), "{refused}");
        }
        for s in STATISTICS {
            assert!(!is_safe(&format!("DOCPROPERTY {s}")) && !is_safe(&format!("INFO {s}")), "{s}");
        }
        for c in CONTENT_DEPENDENT {
            assert!(!is_safe(c), "{c} must not be safe");
        }
    }
}
