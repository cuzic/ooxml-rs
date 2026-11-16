//! Field-code classification and the `StaleFieldRisk` scan (ADR-0004 §6 W2).
//!
//! Fields are tracked with a stack in document order. A field's code is the
//! `w:instrText` seen while that field is the innermost open one and before
//! its own `separate`. Word writes the result of a field nested in another
//! field's instruction as `w:instrText`; that result arrives after the
//! nested field's `separate`, so it never leaks into the outer code. Every
//! nesting level is classified (review round 1, M1).

use std::collections::BTreeSet;

use ooxml_core::RawNode;
use ooxml_schema::wml::{DeletedFieldCode, FieldChar, FieldCode, SimpleField};

use crate::names::is;

/// Codes whose cached result cannot be made wrong by a Phase0 edit: it
/// depends only on document properties, the date or time, the environment,
/// external sources, mail-merge data, user input or the field's own
/// arguments, or on pagination, which Word recomputes at every layout.
pub const SAFE: &[&str] = &[
    // pagination (recomputed by Word at layout)
    "PAGE", "NUMPAGES", "SECTIONPAGES", "SECTION",
    // date and time
    "DATE", "TIME", "CREATEDATE", "SAVEDATE", "PRINTDATE", "EDITTIME",
    // document properties and environment
    "AUTHOR", "TITLE", "SUBJECT", "KEYWORDS", "COMMENTS", "FILENAME", "FILESIZE", "LASTSAVEDBY", "TEMPLATE",
    "DOCPROPERTY", "DOCVARIABLE", "USERNAME", "USERINITIALS", "USERADDRESS", "INFO", "REVNUM",
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
/// bookmarks, counts, other fields). Listed for documentation and tests:
/// any code not in [`SAFE`] is refused, listed here or not.
pub const CONTENT_DEPENDENT: &[&str] = &[
    "TOC", "REF", "PAGEREF", "NOTEREF", "SEQ", "STYLEREF", "=", "IF", "NUMWORDS", "NUMCHARS", "INDEX", "TOA",
    "TC", "XE", "LISTNUM", "CITATION", "BIBLIOGRAPHY", "QUOTE", "RD", "COMPARE", "AUTONUM", "AUTONUMLGL",
    "AUTONUMOUT",
];

/// Reported for a field whose own instruction contains `w:delInstrText`:
/// the old and new codes are interleaved, so the field is not classified.
pub const REVISED_INSTRUCTION: &str = "<revised instruction>";

/// The field code of an instruction: `=` for a formula, else the first
/// token (up to whitespace or `\`), upper-cased. Empty for an empty
/// instruction (which is not safe).
pub fn field_code(instruction: &str) -> String {
    let s = instruction.trim_matches(|c: char| matches!(c, ' ' | '\t' | '\r' | '\n'));
    if s.starts_with('=') {
        return "=".into();
    }
    s.split(|c: char| c.is_whitespace() || c == '\\').next().unwrap_or_default().to_uppercase()
}

/// Is this code safe under Phase0?
pub fn is_safe(code: &str) -> bool {
    SAFE.contains(&code)
}

struct Open {
    instruction: String,
    separated: bool,
    revised: bool,
}

/// Collects the code of every field in a story part (complex fields at every
/// nesting level and `w:fldSimple`), in document order. Unterminated fields
/// are classified too.
#[derive(Default)]
pub(crate) struct FieldScan {
    stack: Vec<Open>,
    codes: Vec<String>,
}

impl FieldScan {
    pub(crate) fn scan(root: &RawNode) -> Vec<String> {
        let mut s = FieldScan::default();
        s.visit(root);
        for open in std::mem::take(&mut s.stack) {
            s.finish(open);
        }
        s.codes
    }

    fn finish(&mut self, open: Open) {
        self.codes.push(if open.revised { REVISED_INSTRUCTION.into() } else { field_code(&open.instruction) });
    }

    fn visit(&mut self, node: &RawNode) {
        if is::<SimpleField>(node) {
            self.codes.push(field_code(node.attr(SimpleField::ATTR_W_INSTR).unwrap_or_default()));
        } else if is::<FieldChar>(node) {
            match node.attr(FieldChar::ATTR_W_FLD_CHAR_TYPE) {
                Some("begin") => self.stack.push(Open { instruction: String::new(), separated: false, revised: false }),
                Some("separate") => {
                    if let Some(top) = self.stack.last_mut() {
                        top.separated = true;
                    }
                }
                Some("end") => {
                    if let Some(open) = self.stack.pop() {
                        self.finish(open);
                    }
                }
                _ => {}
            }
        } else if (is::<FieldCode>(node) || is::<DeletedFieldCode>(node))
            && let Some(top) = self.stack.last_mut()
            && !top.separated
        {
            top.instruction.push_str(&node.text());
            top.revised |= is::<DeletedFieldCode>(node);
        }
        for child in node.elements() {
            self.visit(child);
        }
    }
}

/// The refused codes of a scan: sorted, without duplicates.
pub(crate) fn refused(codes: impl IntoIterator<Item = String>) -> Vec<String> {
    codes.into_iter().filter(|c| !is_safe(c)).collect::<BTreeSet<_>>().into_iter().collect()
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
        for c in CONTENT_DEPENDENT {
            assert!(!is_safe(c), "{c} must not be safe");
        }
    }
}
