//! Errors of `ooxml-docx` (ADR-0004 §6).

use crate::RunContext;
use crate::replace::Refusal;

/// Errors from `ooxml-docx`.
///
/// Every refusal fires before any mutation: a failed call leaves the
/// document unchanged. The row names (`O1`, `W2`, …) refer to ADR-0004 §6.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// An error from `ooxml-core`. Notable cases:
    ///
    /// - O1 [`ooxml_core::Error::EncryptedPackage`] (CFB/OLE file);
    /// - O2 [`ooxml_core::Error::DtdNotAllowed`] (a parsed or scanned part
    ///   has a `<!DOCTYPE>`);
    /// - O3 [`ooxml_core::Error::StrictNotSupported`] (Strict relationship
    ///   type, or the main part's root in a Strict namespace);
    /// - O5 [`ooxml_core::Error::TooDeep`] (a parsed or scanned part nests
    ///   elements deeper than the core parser's limit, 256 by default);
    /// - A1 [`ooxml_core::Error::UnresolvedSibling`] (append into a body
    ///   with an `mc:AlternateContent` child);
    /// - [`ooxml_core::Error::WrongElement`] (the main part is not a
    ///   `w:document`).
    #[error(transparent)]
    Core(#[from] ooxml_core::Error),

    /// O4: the package has no `officeDocument` relationship, or its target
    /// part is missing.
    #[error("the package has no main document part (officeDocument relationship or its target is missing)")]
    MainPartMissing,

    /// The main part has no `w:body`, so there is nowhere to append.
    #[error("the main document part has no w:body")]
    NoBody,

    /// A2: the body has no `w:sectPr` of its own and ends with a paragraph
    /// whose `w:pPr` holds the last section's `w:sectPr`. A paragraph
    /// appended after it would fall into a new section with default page
    /// settings, so append is refused (Phase0 does not move `w:sectPr`).
    #[error("the body ends with a section break in its last paragraph; appending would start a new section")]
    SectionBreakAtEnd,

    /// W1: `w:trackRevisions` is on in `settings.xml`. An untracked edit
    /// would break the author's expectation that every change is recorded.
    #[error("track changes is on (settings w:trackRevisions); untracked edits are refused")]
    TrackRevisionsOn,

    /// W3: `w:documentProtection` is enforced with an editing restriction
    /// other than `none` (`readOnly`, `comments`, `trackedChanges`, `forms`,
    /// or a value ooxml-rs does not know).
    #[error("the document is protected (w:documentProtection edit={edit:?} is enforced)")]
    DocumentProtected {
        /// The `w:edit` value as written (empty if absent).
        edit: String,
    },

    /// W2: a story part has a field whose cached result depends on the
    /// document's content, or whose code ooxml-rs cannot classify, or field
    /// markup ooxml-rs cannot follow. Writes would leave its result silently
    /// stale.
    #[error("the document has fields whose cached results could go stale: {codes:?}")]
    StaleFieldRisk {
        /// The refused keys ([`crate::fields::field_key`]), sorted, without
        /// duplicates: the upper-cased code (`=` for formulas), with the
        /// classifying argument for `INFO`/`DOCPROPERTY` (`INFO NUMWORDS`)
        /// and `BARCODE \b`, or a marker such as
        /// [`crate::fields::MALFORMED`].
        codes: Vec<String>,
    },

    /// R1: the run is inside a complex field (instruction or result,
    /// between `fldChar begin` and its `end`, nesting included) or inside
    /// `w:fldSimple`, or it holds a `w:fldChar` itself. For
    /// `append_paragraph`: the body ends inside an unterminated field.
    #[error("the run is inside a field")]
    InsideField,

    /// R2: the run is inside a tracked change (`w:ins`, `w:del`,
    /// `w:moveFrom`, `w:moveTo`).
    #[error("the run is inside a tracked change ({context:?})")]
    InsideRevision {
        /// The revision context.
        context: RunContext,
    },

    /// R3: an ancestor between the run and `w:body` is something other than
    /// `w:p`, `w:hyperlink`, `w:tbl`, `w:tr` or `w:tc` (content controls,
    /// smart tags, custom XML, bidi wrappers, ruby).
    #[error("the run is inside {ancestor}, which Phase0 does not edit")]
    UnsupportedContext {
        /// The ancestor's qualified name, e.g. `w:sdt`.
        ancestor: &'static str,
    },

    /// R4: the run holds something other than text-only content
    /// (`w:rPr`, `w:t`, `w:tab`, a text-wrapping `w:br`, `w:cr`,
    /// `w:noBreakHyphen`, `w:softHyphen`, `w:lastRenderedPageBreak`,
    /// whitespace between them).
    #[error("the run holds {child}, not only text")]
    RunNotTextOnly {
        /// What was found: an element's `prefix:local` name, `comment`,
        /// `processing instruction` or `text`.
        child: String,
    },

    /// T1: the text contains a character XML 1.0 cannot carry.
    #[error("character U+{:04X} cannot be written to XML 1.0", u32::from(*ch))]
    InvalidXmlChar {
        /// The offending character.
        ch: char,
    },

    /// `find`/`replace` with an empty needle (ADR-0007 §1.1).
    #[error("the needle is empty")]
    EmptyNeedle,

    /// `replace` refused at least one match and `on_refused` is
    /// `Refuse`: nothing was written (ADR-0007 §1.4). Every refused match is
    /// listed.
    #[error("{} match(es) refused, nothing replaced (first: {})", refused.len(), refused.first().map(|r| r.reason.to_string()).unwrap_or_default())]
    ReplaceRefused {
        /// The refused matches, in document order.
        refused: Vec<Refusal>,
    },

    /// `replace_at` / `replace_matches` with a [`crate::Match`] from another
    /// document, from before the last write to its paragraph, or whose text
    /// is no longer at its range (ADR-0007 §7).
    #[error("the match is stale: its paragraph changed since find, or it belongs to another document")]
    StaleMatch,

    /// `replace_matches` with two matches that overlap.
    #[error("two matches overlap")]
    OverlappingMatches,

    /// No story at that index or with that id.
    #[error("no such story")]
    NoSuchStory,

    /// A property element to write occurs twice in one `w:rPr`/`w:pPr`
    /// (or `w:updateFields` twice in `w:settings`): which copy wins is not
    /// specified, so nothing is written (ADR-0007 §4, fail closed).
    #[error("{name} occurs more than once in its parent")]
    DuplicateProperty {
        /// The element, e.g. `w:sz`.
        name: String,
    },

    /// A value outside what Phase1 writes (an empty font name, …).
    #[error("invalid {what}: {value:?}")]
    InvalidValue {
        /// What was invalid.
        what: &'static str,
        /// The value.
        value: String,
    },

    /// The run's `w:rPr` (or the paragraph's `w:pPr`, or its mark's `w:rPr`)
    /// records a tracked property change; a direct write would overwrite
    /// the state rejecting the change must restore (ADR-0007 §4 targets).
    #[error("the target carries a tracked property change ({element})")]
    TrackedPropertyChange {
        /// `w:rPrChange` or `w:pPrChange`.
        element: &'static str,
    },

    /// The paragraph's style chain (`w:basedOn`) loops or is deeper than 32
    /// levels (ADR-0007 §4, fail closed).
    #[error("the style chain is cyclic or deeper than 32 levels")]
    StyleChainTooDeep,

    /// No `w:style` with this exact `w:styleId` (or no styles part).
    /// `latent_only`: Word knows a built-in style by that name
    /// (`w:latentStyles/w:lsdException`), but the document does not define
    /// it; creating one is Phase2 (ADR-0007 §5).
    #[error("no style with id {id:?}{}", if *latent_only { " (latent only)" } else { "" })]
    UnknownStyle {
        /// The id asked for.
        id: String,
        /// A latent-style entry has that name.
        latent_only: bool,
    },

    /// The style exists with another `w:type`.
    #[error("style {id:?} is a {found} style, not a {expected} style")]
    StyleTypeMismatch {
        /// The id.
        id: String,
        /// `paragraph` or `character`.
        expected: &'static str,
        /// The type found, as written (an absent type is `paragraph`).
        found: String,
        /// The linked style (`w:link`), if any: the usual fix when a
        /// paragraph style id is given as a run style.
        linked: Option<String>,
    },

    /// Two `w:style` elements have this id, exactly or ignoring case.
    #[error("style id {id:?} is defined more than once")]
    AmbiguousStyle {
        /// The id.
        id: String,
    },

    /// `enable_update_fields_on_open` on a package without a settings part
    /// (creating one is Phase2).
    #[error("the document has no settings part")]
    NoSettingsPart,

    /// The main part has more than one settings relationship.
    #[error("the document has more than one settings part")]
    AmbiguousSettings,

    /// `w:settings` has two `w:updateFields` elements.
    #[error("w:settings has {name} more than once")]
    DuplicateSetting {
        /// The element.
        name: &'static str,
    },

    /// No paragraph with that index.
    #[error("no paragraph {0}")]
    NoSuchParagraph(usize),

    /// No run with that index in the paragraph.
    #[error("paragraph {paragraph} has no run {run}")]
    NoSuchRun {
        /// Paragraph index.
        paragraph: usize,
        /// Run index within the paragraph.
        run: usize,
    },
}

/// `Result` with [`Error`].
pub type Result<T, E = Error> = std::result::Result<T, E>;
