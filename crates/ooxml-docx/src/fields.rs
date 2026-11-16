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
//! [`MALFORMED`] (review round 2, m1). Revision 4 (review round 3): fields
//! whose file source is not a literal are refused (N3), field markers are
//! checked against the body walk's grammar (m7), and `DOCPROPERTY` uses an
//! allow-list (m9).

use std::collections::BTreeSet;

use ooxml_core::RawNode;
use ooxml_core::name::MC_ALTERNATE_CONTENT;
use ooxml_schema::wml::{DeletedFieldCode, FieldChar, FieldCode, Paragraph, SimpleField};

use crate::body::{Mode, descend, next_element};
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

/// Reported for field markup ooxml-rs cannot follow (review rounds 2 and 3,
/// m1 and m7):
///
/// - a `w:fldChar` whose `w:fldCharType` is missing or not `begin`/
///   `separate`/`end`;
/// - a `separate`/`end` or `w:instrText` with no open field;
/// - a `w:fldChar` or `w:instrText` that is not a child of a `w:r` the body
///   walk reaches (directly in `w:p`, under another child of `w:r`, …);
/// - a `separate`, `end` or `w:instrText` in a different *container* than
///   its field's `begin`. A container is a region the body walk reads as one
///   flow: the body (or story root), or the inside of an element the walk
///   does not enter (a text box's `w:txbxContent`, ruby text `w:rt`,
///   `m:oMath`, a branch of `mc:AlternateContent`, …). A field begun in a
///   text box and ended in the body would otherwise leave its result runs
///   outside any field for R1;
/// - a `w:fldChar` inside an `mc:AlternateContent` that sits within a
///   paragraph (each branch would be counted).
pub const MALFORMED: &str = "<malformed field>";

/// Codes whose argument names a file the result is taken from. When that
/// argument is not a plain literal (a nested field such as
/// `{ INCLUDETEXT "{ FILENAME \p }" bookmark }`, which includes the document
/// itself, or no argument at all), the key gets [`NON_LITERAL_SOURCE`] and
/// is refused (review round 3, N3). A literal path that happens to name the
/// document itself is not detected *(u)*.
pub const FILE_SOURCE: &[&str] = &["INCLUDETEXT", "INCLUDEPICTURE", "LINK"];

/// Suffix of the key of a [`FILE_SOURCE`] field whose source is not a
/// literal.
pub const NON_LITERAL_SOURCE: &str = " (non-literal source)";

/// Built-in `DOCPROPERTY` names whose values Phase0 cannot change (review
/// round 3, m9: an allow-list). Upper-cased. `PAGES` is pagination. Names
/// not listed here are refused unless they are custom properties in
/// `docProps/custom.xml` without `linkTarget` and not a [`STATISTICS`] name.
pub const DOCPROPERTY_SAFE: &[&str] = &[
    "AUTHOR", "CATEGORY", "COMMENTS", "COMPANY", "CREATETIME", "HYPERLINKBASE", "KEYWORDS", "LASTPRINTED",
    "LASTSAVEDBY", "LASTSAVEDTIME", "MANAGER", "NAMEOFAPPLICATION", "PAGES", "REVISIONNUMBER", "SECURITY", "SUBJECT",
    "TEMPLATE", "TITLE", "TOTALEDITINGTIME",
];

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

/// Characters taken as a quotation mark around an argument: the straight
/// quote and the typographic ones Word's AutoFormat produces when a field
/// code is typed by hand (review round 3, m9). Whether Word itself reads the
/// typographic ones as quotes is *(u)*; treating them as quotes makes the
/// argument match the lists, and anything unexpected is refused anyway.
const QUOTES: &[char] = &['"', '\u{201C}', '\u{201D}', '\u{201E}', '\u{201F}', '\u{FF02}'];

/// The first argument after the code: a quoted string without its quotes
/// (any of [`QUOTES`] opens and closes), or a token up to whitespace or
/// `\`. `None` if the instruction goes straight to a switch, ends, or has
/// an unterminated quote.
fn first_argument(rest: &str) -> Option<&str> {
    let rest = rest.trim_start();
    let arg = if let Some(q) = rest.strip_prefix(QUOTES) {
        &q[..q.find(QUOTES)?]
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
/// - `BARCODE` with a `\b` switch (its argument is a bookmark), in any
///   case and spacing (`bm\b`, `\B`): `BARCODE \b`, refused;
/// - a [`FILE_SOURCE`] code without a literal first argument:
///   `<CODE> (non-literal source)`, refused.
pub fn field_key(instruction: &str) -> String {
    key(instruction, false)
}

/// [`field_key`], where `nested` says the instruction contained a nested
/// field (whose result is not part of `instruction`).
fn key(instruction: &str, nested: bool) -> String {
    let (code, rest) = split_code(instruction);
    match code.as_str() {
        "INFO" | "DOCPROPERTY" if nested => code,
        "INFO" | "DOCPROPERTY" => match first_argument(rest) {
            Some(arg) => format!("{code} {}", arg.to_uppercase()),
            None => code,
        },
        "BARCODE" if rest.to_lowercase().contains("\\b") => "BARCODE \\b".into(),
        c if FILE_SOURCE.contains(&c) && (nested || first_argument(rest).is_none()) => {
            format!("{code}{NON_LITERAL_SOURCE}")
        }
        _ => code,
    }
}

/// Is this key (from [`field_key`]) safe under Phase0?
///
/// - `INFO X` is safe when `X` is itself a safe plain code (`INFO NumPages`,
///   `INFO Author`); `INFO NumWords`, `INFO FileSize` are not.
/// - `DOCPROPERTY X` is safe when `X` is in [`DOCPROPERTY_SAFE`] (an
///   allow-list). Custom properties are decided with `docProps/custom.xml`
///   when the document is opened.
/// - Everything else: listed in [`SAFE`].
pub fn is_safe(key: &str) -> bool {
    if let Some(arg) = key.strip_prefix("INFO ") {
        return !matches!(arg, "INFO" | "DOCPROPERTY") && !arg.contains(' ') && SAFE.contains(&arg);
    }
    if let Some(arg) = key.strip_prefix("DOCPROPERTY ") {
        return DOCPROPERTY_SAFE.contains(&arg);
    }
    SAFE.contains(&key)
}

struct Open {
    instruction: String,
    separated: bool,
    revised: bool,
    calculated: bool,
    /// A field began inside this field's instruction (review round 3, N3).
    nested: bool,
    /// Container of the `begin` (see [`MALFORMED`]).
    container: u32,
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

/// How the scan reads an element's children: as the body walk does, or —
/// inside an element the body walk does not enter — `Foreign`, where
/// paragraphs and runs (also outside a paragraph, as in ruby text or
/// math) are still recognized so that complete fields in
/// text boxes and similar stories are classified.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ScanMode {
    Walk(Mode),
    Foreign,
}

impl ScanMode {
    /// The mode for `child`'s children, or `None` if `child` starts a new
    /// container.
    fn descend(self, child: &RawNode) -> Option<ScanMode> {
        match self {
            ScanMode::Walk(m) => descend(m, child).map(|(m, _)| ScanMode::Walk(m)),
            ScanMode::Foreign if is::<Paragraph>(child) => Some(ScanMode::Walk(Mode::Inline)),
            ScanMode::Foreign if is::<ooxml_schema::wml::Run>(child) => Some(ScanMode::Walk(Mode::Run)),
            ScanMode::Foreign => None,
        }
    }
}

impl FieldScan {
    pub(crate) fn scan(root: &RawNode) -> Vec<String> {
        let mut s = FieldScan::default();
        let root_mode = if is::<ooxml_schema::wml::Document>(root) { Mode::Document } else { Mode::Block };
        let mut containers = 0u32;
        // (node, index of the next child to visit, mode for its children,
        // container, pushed a scope)
        let mut walk: Vec<(&RawNode, usize, ScanMode, u32, bool)> = vec![(root, 0, ScanMode::Walk(root_mode), 0, false)];
        while let Some((node, next, mode, container, _)) = walk.last_mut() {
            let (mode, container) = (*mode, *container);
            match next_element(node, *next) {
                Some((i, child)) => {
                    *next = i + 1;
                    s.markup(child, mode, container);
                    let (child_mode, child_container) = match mode.descend(child) {
                        Some(m) => (m, container),
                        None => {
                            containers += 1;
                            (ScanMode::Foreign, containers)
                        }
                    };
                    let scoped = s.scope(child);
                    walk.push((child, 0, child_mode, child_container, scoped));
                }
                None => {
                    if let Some((.., true)) = walk.pop() {
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
            () => key(&open.instruction, open.nested),
        };
        self.codes.push(key);
    }

    fn malformed(&mut self) {
        self.codes.push(MALFORMED.into());
    }

    /// Field markup in `node`, a child of an element whose children are
    /// read in `parent` mode, in `container`.
    fn markup(&mut self, node: &RawNode, parent: ScanMode, container: u32) {
        // Only a direct child of a run counts for the body walk (and, by the
        // same rule, in foreign containers).
        let in_run = matches!(parent, ScanMode::Walk(Mode::Run));
        if is::<SimpleField>(node) {
            self.codes.push(field_key(node.attr(SimpleField::ATTR_W_INSTR).unwrap_or_default()));
        } else if is::<FieldChar>(node) {
            if !in_run || self.scopes.last() == Some(&Scope::AlternateContent) {
                self.malformed();
            }
            match node.attr(FieldChar::ATTR_W_FLD_CHAR_TYPE) {
                Some("begin") => {
                    if let Some(top) = self.stack.last_mut()
                        && !top.separated
                    {
                        top.nested = true;
                    }
                    self.stack.push(Open {
                        instruction: String::new(),
                        separated: false,
                        revised: false,
                        calculated: is_calculated(node),
                        nested: false,
                        container,
                    });
                }
                Some("separate") => match self.stack.last_mut() {
                    Some(top) => {
                        top.separated = true;
                        if top.container != container {
                            self.malformed();
                        }
                    }
                    None => self.malformed(),
                },
                Some("end") => match self.stack.pop() {
                    Some(open) => {
                        if open.container != container {
                            self.malformed();
                        }
                        self.finish(open);
                    }
                    None => self.malformed(),
                },
                _ => self.malformed(),
            }
        } else if is::<FieldCode>(node) || is::<DeletedFieldCode>(node) {
            if !in_run {
                self.malformed();
            }
            match self.stack.last_mut() {
                Some(top) if top.container != container => self.malformed(),
                Some(top) if !top.separated => {
                    top.instruction.push_str(&node.text());
                    top.revised |= is::<DeletedFieldCode>(node);
                }
                Some(_) => {}
                None => self.malformed(),
            }
        }
    }

    /// Pushes a scope for `node` if it opens one; returns whether it did.
    fn scope(&mut self, node: &RawNode) -> bool {
        if is::<Paragraph>(node) {
            self.scopes.push(Scope::Paragraph);
            true
        } else if node.is(MC_ALTERNATE_CONTENT) && !self.scopes.is_empty() {
            // Inside a paragraph (directly or in a run). A text box's own
            // paragraphs inside it push `Paragraph` again, so only markup
            // between this element and the next `w:p` is affected.
            self.scopes.push(Scope::AlternateContent);
            true
        } else {
            false
        }
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

/// Custom properties of `docProps/custom.xml`, upper-cased.
#[derive(Debug, Default)]
pub(crate) struct CustomNames {
    /// Linked to content (`linkTarget`, a bookmark).
    pub linked: Vec<String>,
    /// Every name.
    pub all: Vec<String>,
}

/// The refused keys of a scan: sorted, without duplicates. `DOCPROPERTY X`
/// is safe when `X` is in [`DOCPROPERTY_SAFE`], or when it is a custom
/// property that is not linked to content and not a [`STATISTICS`] name.
pub(crate) fn refused(codes: impl IntoIterator<Item = String>, custom: &CustomNames) -> Vec<String> {
    let safe = |c: &str| match c.strip_prefix("DOCPROPERTY ") {
        Some(a) => {
            is_safe(c)
                || (custom.all.iter().any(|n| n == a)
                    && !custom.linked.iter().any(|n| n == a)
                    && !STATISTICS.contains(&a))
        }
        None => is_safe(c),
    };
    codes.into_iter().filter(|c| !safe(c)).collect::<BTreeSet<_>>().into_iter().collect()
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
        assert_eq!(field_key("DOCPROPERTY \u{201C}Words\u{201D}"), "DOCPROPERTY WORDS");
        assert_eq!(field_key("INFO \u{FF02}NumWords\u{FF02}"), "INFO NUMWORDS");
        assert_eq!(field_key("BARCODE bm\\b"), "BARCODE \\b");
        assert_eq!(field_key("BARCODE bm \\B \\u"), "BARCODE \\b");
        assert_eq!(field_key("INCLUDETEXT \"c:\\\\a.docx\" bm"), "INCLUDETEXT");
        assert_eq!(field_key("INCLUDETEXT \\* MERGEFORMAT"), "INCLUDETEXT (non-literal source)");
        assert_eq!(key("INCLUDETEXT \"\" Client", true), "INCLUDETEXT (non-literal source)");
        assert_eq!(key("LINK Excel.Sheet.12 \"\" \"R1C1\"", true), "LINK (non-literal source)");
        assert_eq!(key("DOCPROPERTY  \\* MERGEFORMAT", true), "DOCPROPERTY");
        for safe in ["INFO NUMPAGES", "INFO AUTHOR", "DOCPROPERTY PAGES", "DOCPROPERTY TITLE", "DOCPROPERTY COMPANY", "BARCODE", "INCLUDETEXT", "LINK"] {
            assert!(is_safe(safe), "{safe}");
        }
        for refused in ["INFO", "DOCPROPERTY", "INFO NUMWORDS", "INFO FILESIZE", "INFO TOC", "INFO INFO", "INFO DOCPROPERTY", "DOCPROPERTY WORDS", "DOCPROPERTY BYTES", "DOCPROPERTY CLIENT NAME", "DOCPROPERTY \u{201C}WORDS\u{201D}", "INCLUDETEXT (non-literal source)", "FILESIZE", "BARCODE \\b", CALCULATED_FORM_FIELD, MALFORMED] {
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
