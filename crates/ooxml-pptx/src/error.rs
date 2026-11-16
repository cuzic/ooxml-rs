//! Errors of the PowerPoint domain API (ADR-0005 §6).

/// Errors from `ooxml-pptx`.
///
/// Every refusal is raised **before** the first change to the tree, so a
/// refused edit leaves the package exactly as it was (ADR-0005 §3).
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// An error from the package / XML layer (`ooxml-core`): I/O, zip,
    /// `EncryptedPackage`, `StrictNotSupported`, malformed XML, and the
    /// lens refusals of ADR-0002 (`NamespaceNotInScope`, `UnresolvedSibling`,
    /// `InvalidXmlChar`, …).
    #[error(transparent)]
    Core(#[from] ooxml_core::Error),
    /// The package's `officeDocument` part is missing or is not a
    /// PresentationML `p:presentation`.
    #[error("not a presentation: {0}")]
    NotAPresentation(String),
    /// `p:presentation@conformance="strict"` (ADR-0001 scope: Transitional only).
    #[error("the presentation declares Strict conformance; only Transitional is supported")]
    StrictConformance,
    /// A slide index past the end of `p:sldIdLst`.
    #[error("slide index {index} is out of range ({count} slides)")]
    SlideIndexOutOfRange {
        /// Requested index.
        index: usize,
        /// Number of slides.
        count: usize,
    },
    /// A `p:sldId` whose `r:id` is missing from the presentation's
    /// relationships or points to a part that does not exist.
    #[error("slide id {slide_id} has no slide part (dangling r:id)")]
    DanglingSlide {
        /// `p:sldId@id`.
        slide_id: u32,
    },
    /// No shape at the given path or with the given id.
    #[error("shape not found")]
    ShapeNotFound,
    /// More than one shape on the slide carries this `p:cNvPr@id`.
    #[error("shape id {id} occurs on more than one shape")]
    AmbiguousShapeId {
        /// The duplicated id.
        id: u32,
    },
    /// The shape's own `cNvPr@id` (or that of an enclosing group) is
    /// missing or not a number, so references to it cannot be checked.
    #[error("a shape on the edit path has no valid cNvPr@id")]
    InvalidShapeId,
    /// The target is inside `mc:AlternateContent`: editing one branch would
    /// desynchronise the others.
    #[error("the shape is inside mc:AlternateContent and is read-only")]
    InsideAlternateContent,
    /// The shape has no `p:txBody` (or the cell no `a:txBody`); creating one
    /// is out of Phase 0 scope.
    #[error("the shape has no text body")]
    NoTextBody,
    /// The shape is not a graphic frame holding an `a:tbl`.
    #[error("the shape is not a table")]
    NotATable,
    /// No cell at (row, col).
    #[error("no table cell at row {row}, column {col}")]
    CellNotFound {
        /// Row index (`a:tr`, 0-based).
        row: usize,
        /// Column index (`a:tc` within the row, 0-based).
        col: usize,
    },
    /// The cell is covered by a merge (`hMerge`/`vMerge`); its text is not shown.
    #[error("table cell at row {row}, column {col} is covered by a merge")]
    MergedCell {
        /// Row index.
        row: usize,
        /// Column index.
        col: usize,
    },
    /// No paragraph with that index.
    #[error("no paragraph {0}")]
    ParagraphNotFound(usize),
    /// No run (`a:r`) with that index in the paragraph.
    #[error("no run {0}")]
    RunNotFound(usize),
    /// No field (`a:fld`) with that index in the paragraph.
    #[error("no field {0}")]
    FieldNotFound(usize),
    /// The edit would remove an `a:fld` (slide number, date, …), whose text
    /// is a cache Office computes. Remove the field explicitly with
    /// `ParagraphMut::remove_field` first.
    #[error("paragraph {paragraph} contains a field (a:fld) that this edit would remove")]
    FieldInParagraph {
        /// Index of the paragraph holding the field.
        paragraph: usize,
    },
    /// A text body, paragraph or run has a child Phase 0 does not model
    /// (`a14:m`, `mc:AlternateContent`, out-of-order children, …).
    #[error("unsupported content in {0}")]
    UnsupportedContent(String),
    /// Removing the only `a:p` (`minOccurs = 1`).
    #[error("cannot remove the last paragraph of a text body")]
    LastParagraph,
    /// A paragraph-structure edit on a shape whose paragraphs are animated
    /// (`p:bldP@build` ≠ `whole`, or `p:txEl/p:pRg`): indices would shift.
    #[error("paragraphs of shape {shape_id} are referenced by animations")]
    AnimatedParagraphs {
        /// The edited shape's id.
        shape_id: u32,
    },
    /// A text edit on a shape whose characters are animated (`p:txEl/p:charRg`).
    #[error("characters of shape {shape_id} are referenced by animations")]
    AnimatedCharacters {
        /// The edited shape's id.
        shape_id: u32,
    },
    /// A text edit on a shape that a modern comment anchors to a text range
    /// (or whose anchor could not be verified): `cp`/`len` would shift.
    #[error("text of shape {shape_id} is anchored by a modern comment (or the slide's comment anchors are unverifiable)")]
    CommentAnchoredText {
        /// The edited shape's id.
        shape_id: u32,
    },
    /// The text contains a character XML 1.0 does not allow.
    #[error("character U+{:04X} at byte {offset} is not allowed in XML 1.0", u32::from(*ch))]
    InvalidXmlChar {
        /// The offending character.
        ch: char,
        /// Its byte offset in the argument.
        offset: usize,
    },
    /// `\n`, `\r` or `\v` passed to a run-level setter.
    #[error("line breaks are not allowed in run text; use add_break or Paragraph/TextBody set_text")]
    LineBreakInRun,
    /// `\n` or `\r` passed to a paragraph-level setter (`\v` becomes `a:br`).
    #[error("paragraph breaks are not allowed in paragraph text; use TextBody::set_text or add_paragraph")]
    LineBreakInParagraph,
    /// A new run or paragraph needs a copy of an existing `a:rPr` /
    /// `a:endParaRPr` / `a:pPr` that the copy primitive refuses
    /// (`ooxml_core::Lens::detach_as`, any `DetachRefusal`): a relationship
    /// id (`r:`, Strict `r:`, VML `o:relid`) outside the skipped direct
    /// links (e.g. a picture fill or bullet, or a link inside `a:defRPr`),
    /// a document-unique `smtId` or effect container name, an extension
    /// list, Markup Compatibility
    /// content, a namespace declaration, a namespace made ignorable by an
    /// ancestor's `mc:Ignorable`, or text under an inherited
    /// `xml:space="preserve"`. Use `add_run` / `add_paragraph` for
    /// explicitly unformatted text.
    #[error("the formatting of {0} cannot be copied (relationship id, unique id or name, extension list, markup compatibility, namespace declaration or scope)")]
    FormatNotCopyable(&'static str),
    /// A find/replace pattern that is empty or contains `\n`, `\r` or `\v`
    /// (matches never cross paragraphs or line breaks; ADR-0008 §4).
    #[error("the search pattern is empty or contains a line break")]
    InvalidPattern,
    /// A match covers (part of) an `a:fld`, whose text Office computes
    /// (ADR-0008 §4 rule 1).
    #[error("the match covers a field (a:fld)")]
    FieldInMatch,
    /// A match covers an `a:br` (ADR-0008 §4 rule 1).
    #[error("the match covers a line break (a:br)")]
    BreakInMatch,
    /// [`crate::MixedFormat::Refuse`]: the match covers runs whose `a:rPr`
    /// differ beyond the proofing attributes, or in their links.
    #[error("the match covers runs with different formatting")]
    MixedFormatInMatch,
    /// A replacement with `\v` for a match that ends inside a run with a
    /// hyperlink: the rest of that run would move into a new run without
    /// the link (ADR-0008 §4 rule 5).
    #[error("a line break in the replacement would split a hyperlink run")]
    LinkSplitByBreak,
}

impl Error {
    /// A fresh copy of a refusal that applies to every match of one text
    /// body (the error itself is not `Clone`: `Core` wraps I/O errors).
    pub(crate) fn duplicate(&self) -> Error {
        match self {
            Error::ShapeNotFound => Error::ShapeNotFound,
            Error::AmbiguousShapeId { id } => Error::AmbiguousShapeId { id: *id },
            Error::InvalidShapeId => Error::InvalidShapeId,
            Error::InsideAlternateContent => Error::InsideAlternateContent,
            Error::NoTextBody => Error::NoTextBody,
            Error::NotATable => Error::NotATable,
            Error::CellNotFound { row, col } => Error::CellNotFound { row: *row, col: *col },
            Error::MergedCell { row, col } => Error::MergedCell { row: *row, col: *col },
            Error::AnimatedCharacters { shape_id } => Error::AnimatedCharacters { shape_id: *shape_id },
            Error::CommentAnchoredText { shape_id } => Error::CommentAnchoredText { shape_id: *shape_id },
            Error::UnsupportedContent(s) => Error::UnsupportedContent(s.clone()),
            other => Error::UnsupportedContent(other.to_string()),
        }
    }
}

impl From<ooxml_core::AttrError> for Error {
    fn from(e: ooxml_core::AttrError) -> Self {
        Error::Core(e.into())
    }
}

/// `Result` with [`Error`].
pub type Result<T, E = Error> = std::result::Result<T, E>;
