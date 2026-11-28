//! Defined names: listing and resolution (ADR-0023 §7).
//!
//! [`Workbook::defined_names`] lists every `definedName` in document order;
//! [`Workbook::resolve_defined_name`] resolves one to a range. Resolution is
//! a new, small function: it shares with [`crate::formula::precedents`] only
//! the case fold and the parsed expression, not `Env::resolve` (whose
//! conservative rules serve dependency analysis, not lookup).

use ooxml_core::ElementSpec;
use ooxml_schema::sml;

use crate::census::SheetKind;
use crate::error::{Error, Result};
use crate::formula::lexer::{Area, Prefix};
use crate::formula::parse::{Expr, parse};
use crate::formula::precedents::fold;
use crate::reference::{CellIndex, CellRange, SheetRef, MAX_COLS, MAX_ROWS};
use crate::workbook::Workbook;

/// A defined name, as stored.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DefinedName {
    /// As stored, including the `_xlnm.` prefix of built-in names.
    pub name: String,
    /// The element text, file form.
    pub formula: String,
    /// Workbook scope, a sheet scope, or an invalid stored scope.
    pub scope: NameScope,
    /// The `comment` attribute, when present.
    pub comment: Option<String>,
    /// The `hidden` attribute (garbage reads as `false`).
    pub hidden: bool,
    /// `_xlnm.` prefix, ASCII case-insensitive.
    pub builtin: bool,
}

/// The scope of a [`DefinedName`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NameScope {
    /// No `localSheetId`: visible workbook-wide.
    Workbook,
    /// `localSheetId`, as the setters' `Option<u32>`: a 0-based position in
    /// `<sheets>`.
    Sheet(u32),
    /// A `localSheetId` that is not a valid sheet position, as stored
    /// (unparsable or out of range).
    Invalid(String),
}

/// A resolved defined name.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResolvedName {
    /// One sheet-qualified absolute area on a worksheet. `sheet` is the
    /// 0-based position in `<sheets>`.
    Range {
        /// 0-based position in `<sheets>`.
        sheet: usize,
        /// The area, corners normalized.
        range: CellRange,
    },
}

/// A `definedName` element before scope validation (which needs the sheet
/// count). The single extraction behind the public listing and the internal
/// tuple users.
#[derive(Clone, Debug)]
pub(crate) struct RawName {
    /// The `name` attribute (missing reads as empty).
    pub(crate) name: String,
    /// The element text, file form.
    pub(crate) text: String,
    /// The `localSheetId` attribute, as stored.
    pub(crate) local: Option<String>,
    /// The `comment` attribute, when present.
    pub(crate) comment: Option<String>,
    /// The `hidden` attribute (garbage reads as `false`).
    pub(crate) hidden: bool,
}

impl RawName {
    /// The parsed `localSheetId`, as the old tuple extraction read it
    /// (surrounding whitespace tolerated, garbage reads as absent).
    pub(crate) fn local_id(&self) -> Option<u32> {
        self.local.as_deref().and_then(|v| v.trim().parse().ok())
    }
}

/// Whether `name` is built-in (`_xlnm.` prefix, ASCII case-insensitive).
pub(crate) fn is_builtin(name: &str) -> bool {
    name.get(..6).is_some_and(|prefix| prefix.eq_ignore_ascii_case("_xlnm."))
}

/// `(child index, fields)` of every `definedName`, in document order.
pub(crate) fn raw_defined_names(doc: &ooxml_core::XmlDocument) -> Vec<(usize, RawName)> {
    use ooxml_core::QName;
    doc.root()
        .elements()
        .find(|node| node.is(sml::DefinedNames::QNAME))
        .into_iter()
        .flat_map(|names| names.indexed_elements())
        .filter(|(_, node)| node.is(sml::DefinedName::QNAME))
        .map(|(index, node)| {
            let attr = |key| node.attr(QName::new("", key)).map(str::to_owned);
            let name = attr("name").unwrap_or_default();
            let hidden = node
                .attr(QName::new("", "hidden"))
                .and_then(ooxml_core::value::parse_bool)
                .unwrap_or(false);
            (index, RawName { name, text: node.text().into_owned(), local: attr("localSheetId"), comment: attr("comment"), hidden })
        })
        .collect()
}

impl Workbook {
    /// Reads the workbook's defined names in document order, all scopes
    /// (workbook, sheet and invalid).
    pub fn defined_names(&mut self) -> Result<Vec<DefinedName>> {
        let document = self.pkg.xml_part(&self.workbook_part)?;
        let sheets = self.sheets.len();
        Ok(raw_defined_names(document)
            .into_iter()
            .map(|(_, raw)| {
                let scope = match raw.local {
                    None => NameScope::Workbook,
                    Some(id) => match id.trim().parse::<u32>() {
                        Ok(n) if (n as usize) < sheets => NameScope::Sheet(n),
                        _ => NameScope::Invalid(id),
                    },
                };
                let builtin = is_builtin(&raw.name);
                DefinedName { name: raw.name, formula: raw.text, scope, comment: raw.comment, hidden: raw.hidden, builtin }
            })
            .collect())
    }

    /// Resolves one defined name to a range (ADR-0023 §7 M7).
    ///
    /// Candidates are the names that fold equal to `name` (the precedents
    /// case fold). A candidate with an invalid scope fails the lookup closed
    /// ([`Error::AmbiguousName`]). With `scope: Some(s)` the candidates local
    /// to `s` win, else the global ones; with `scope: None` only the global
    /// candidates are considered. Exactly one candidate in the chosen scope
    /// resolves; two folding alike are [`Error::AmbiguousName`], none is
    /// [`Error::NoSuchName`]. The winner resolves only when its text is one
    /// sheet-qualified absolute area on a worksheet, else
    /// [`Error::NameNotARange`].
    pub fn resolve_defined_name(&mut self, name: &str, scope: Option<SheetRef<'_>>) -> Result<ResolvedName> {
        let all = self.defined_names()?;
        let folded = fold(name);
        let candidates: Vec<&DefinedName> = all.iter().filter(|n| fold(&n.name) == folded).collect();
        if candidates.iter().any(|n| matches!(n.scope, NameScope::Invalid(_))) {
            return Err(Error::AmbiguousName { name: name.to_owned(), reason: "invalid localSheetId" });
        }
        let chosen: Vec<&&DefinedName> = match scope {
            Some(sheet) => {
                let position = self.sheet_position(sheet)?;
                let local: Vec<&&DefinedName> =
                    candidates.iter().filter(|n| matches!(n.scope, NameScope::Sheet(id) if id as usize == position)).collect();
                if local.is_empty() {
                    candidates.iter().filter(|n| matches!(n.scope, NameScope::Workbook)).collect()
                } else {
                    local
                }
            }
            None => candidates.iter().filter(|n| matches!(n.scope, NameScope::Workbook)).collect(),
        };
        let winner = match chosen.as_slice() {
            [one] => one,
            [] => return Err(Error::NoSuchName(name.to_owned())),
            _ => return Err(Error::AmbiguousName { name: name.to_owned(), reason: "names fold alike" }),
        };
        self.classify_name(name, &winner.formula)
    }

    /// A [`SheetRef`] as a 0-based `<sheets>` position (name rules as
    /// [`Workbook::worksheet_by_name`]; any kind of sheet counts).
    fn sheet_position(&self, sheet: SheetRef<'_>) -> Result<usize> {
        match sheet {
            SheetRef::Name(name) => self.sheet_index(name),
            SheetRef::Index(i) if i < self.sheets.len() => Ok(i),
            SheetRef::Index(i) => Err(Error::NoSuchSheet(format!("#{i}"))),
        }
    }

    /// Classifies a winner's text: one sheet-qualified absolute area on a
    /// worksheet resolves, everything else is [`Error::NameNotARange`]. No
    /// formula is evaluated.
    fn classify_name(&self, name: &str, text: &str) -> Result<ResolvedName> {
        let not_a_range = |reason: &'static str| Error::NameNotARange { name: name.to_owned(), reason };
        // Excel's dangling forms: bare `#REF!` parses as a literal and
        // `#REF!$A$1` does not parse at all, but both mean gone.
        if text.trim().as_bytes().get(..5).is_some_and(|h| h.eq_ignore_ascii_case(b"#REF!")) {
            return Err(not_a_range("#REF!"));
        }
        let area = match parse(text).ok() {
            Some(Expr::Ref(reference)) => reference,
            Some(Expr::Union(_)) => return Err(not_a_range("multiple areas")),
            // An intersection is at most one area but needs evaluation.
            Some(Expr::Intersect(..)) => return Err(not_a_range("formula")),
            Some(Expr::Lit) => return Err(not_a_range("constant")),
            // A name, a call, an operator, an opaque/structured reference, or
            // text that does not parse: a formula, unevaluated.
            _ => return Err(not_a_range("formula")),
        };
        let sheet_name = match &area.prefix {
            Prefix::Sheets(..) => return Err(not_a_range("3D reference")),
            Prefix::External => return Err(not_a_range("external reference")),
            // No sheet is named: relative to the sheet in use, which a
            // file-level API has no notion of.
            Prefix::None => return Err(not_a_range("relative reference")),
            Prefix::Sheet(sheet) => sheet,
        };
        let range = match area.area {
            Area::Cell { col, row } => {
                if !(col.abs && row.abs) {
                    return Err(not_a_range("relative reference"));
                }
                CellRange {
                    first: CellIndex { row: row.idx, col: col.idx },
                    last: CellIndex { row: row.idx, col: col.idx },
                }
            }
            Area::Range { c1, r1, c2, r2 } => {
                if !(c1.abs && r1.abs && c2.abs && r2.abs) {
                    // A `$`-less row or column is relative to the active cell.
                    return Err(not_a_range("relative reference"));
                }
                let (r1, r2) = (r1.idx.min(r2.idx), r1.idx.max(r2.idx));
                let (c1, c2) = (c1.idx.min(c2.idx), c1.idx.max(c2.idx));
                CellRange { first: CellIndex { row: r1, col: c1 }, last: CellIndex { row: r2, col: c2 } }
            }
            // Absolute whole columns/rows are one absolute area: the full span.
            Area::Cols { c1, c2 } if c1.abs && c2.abs => {
                let (c1, c2) = (c1.idx.min(c2.idx), c1.idx.max(c2.idx));
                CellRange { first: CellIndex { row: 0, col: c1 }, last: CellIndex { row: MAX_ROWS - 1, col: c2 } }
            }
            Area::Rows { r1, r2 } if r1.abs && r2.abs => {
                let (r1, r2) = (r1.idx.min(r2.idx), r1.idx.max(r2.idx));
                CellRange { first: CellIndex { row: r1, col: 0 }, last: CellIndex { row: r2, col: MAX_COLS - 1 } }
            }
            Area::Cols { .. } | Area::Rows { .. } => return Err(not_a_range("relative reference")),
            Area::RefError => return Err(not_a_range("#REF!")),
        };
        // Stored text, so the precedents rule (unique fold, no exact-first).
        let folded = fold(sheet_name);
        let mut at = self.sheets.iter().enumerate().filter(|(_, s)| fold(&s.name) == folded).map(|(i, _)| i);
        let sheet = match (at.next(), at.next()) {
            (Some(i), None) => i,
            // Absent, or two sheets folding alike: dangling either way.
            _ => return Err(not_a_range("#REF!")),
        };
        if self.sheets[sheet].kind != SheetKind::Worksheet {
            return Err(not_a_range("not a worksheet"));
        }
        Ok(ResolvedName::Range { sheet, range })
    }
}
