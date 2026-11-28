//! Errors of the PowerPoint domain API (ADR-0005 §6).

/// Errors from `ooxml-pptx`.
///
/// Every refusal is raised **before** the first change to the tree, so a
/// refused edit leaves the package exactly as it was (ADR-0005 §3).
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The target uses a script or data scheme.
    #[error("unsafe hyperlink scheme")]
    UnsafeLinkScheme,
    /// The target is not a valid IRI reference.
    #[error("invalid hyperlink URL")]
    InvalidLinkUrl,
    /// This action has no supported write representation.
    #[error("unsupported hyperlink action")]
    UnsupportedAction,
    /// A destination returned by the link reader cannot be written directly.
    #[error("read-only hyperlink target; use a presentation-scoped slide target for slide links")]
    ReadOnlyLinkTarget,
    /// Link editing is unavailable for SmartArt text.
    #[error("hyperlink editing is unsupported for SmartArt text")]
    SmartArtLinkUnsupported,
    /// Replacing or clearing this link would remove its sound reference.
    #[error("a hyperlink with a sound cannot be replaced or cleared")]
    LinkHasSound,
    /// A slide link target belongs to a different presentation session.
    #[error("hyperlink target belongs to another presentation")]
    ForeignLinkSlide,
    /// No slide in the presentation has this id.
    #[error("no hyperlink target slide with id {slide_id}")]
    LinkSlideNotFound {
        /// Stable slide id.
        slide_id: u32,
    },
    /// This bullet edit requires support for picture relationships.
    #[error("picture bullet operation is unsupported")]
    UnsupportedBullet,
    /// Placeholder colour is confined to style matrices.
    #[error("placeholder scheme colour is invalid for direct text formatting")]
    InvalidSchemeColor,
    /// A formatting range is reversed or exceeds the paragraph length.
    #[error("formatting range out of bounds")]
    RangeOutOfBounds,
    /// A formatting range starts or ends inside a field.
    #[error("formatting range splits a field")]
    RangeSplitsField,

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

    // ---- Phase 2 (ADR-0012 §9): slides as units ----
    /// The layout is not reachable from a master in `p:sldMasterIdLst`
    /// through its `p:sldLayoutIdLst`.
    #[error("layout {0:?} is not a layout of a master of this presentation")]
    LayoutNotFound(String),
    /// No `p:sldId@id` is left below 2,147,483,648 (`ST_SlideId`).
    #[error("no slide id is left (ST_SlideId ends at 2147483647)")]
    SlideIdsExhausted,
    /// No shape id is left below `i32::MAX`.
    #[error("no shape id is left on this slide")]
    ShapeIdsExhausted,
    /// No `p14:creationId` value is left (u32).
    #[error("no p14:creationId value is left")]
    CreationIdsExhausted,
    /// Delete of a slide that another part references: a relationship from
    /// outside the cascade, a Slide Zoom or other numeric `sldId`
    /// reference, or a relationships part that cannot be read
    /// (`rel_type` `"?"`).
    #[error("the slide is referenced from {from_part:?} ({rel_type})")]
    SlideIsReferenced {
        /// The referring part.
        from_part: String,
        /// The relationship type, `"sldId"` for an id-valued attribute,
        /// `"?"` for an unreadable relationships part.
        rel_type: String,
    },
    /// Delete of a slide named in a custom show (`p:custShowLst`).
    #[error("the slide is part of a custom show")]
    SlideInCustomShow,
    /// `presProps` has a slide show range by position (`p:showPr/p:sldRg`):
    /// positions must not change.
    #[error("the presentation has a slide show range by position (p:sldRg)")]
    ShowRangeByPosition,
    /// Delete/duplicate of a slide with a relationship of a type outside
    /// ADR-0012 §6 / §4.4 (charts, OLE, controls, comments on duplicate, …).
    #[error("the slide has a part of type {0:?}, which this operation does not support")]
    UnsupportedSlidePart(String),
    /// A section list ADR-0012 §5 does not model (another placement,
    /// inside `mc:AlternateContent`, several lists).
    #[error("the presentation's section list is not in a supported form")]
    UnsupportedSections,
    /// The sections' slide lists do not concatenate to `p:sldIdLst`.
    #[error("the section lists do not match the slide list")]
    SectionsInconsistent,
    // ---- Phase 1b (ADR-0013 §5): tables ----
    /// The table is not well-formed for structural edits (short rows,
    /// overlapping or broken merges, partial `a16` ids, …).
    #[error("the table is not well-formed: {0}")]
    TableNotWellFormed(String),
    /// A structural edit on a table a modern comment anchors into.
    #[error("table {shape_id} is anchored by a modern comment (rows, columns or cells)")]
    CommentAnchoredTable {
        /// The graphic frame's id.
        shape_id: u32,
    },
    /// Deleting a row that holds the origin of a vertical merge.
    #[error("row {row} holds the origin of a merge; unmerge first")]
    MergeOriginInRow {
        /// The row.
        row: usize,
    },
    /// Deleting a column that holds the origin of a horizontal merge.
    #[error("column {col} holds the origin of a merge; unmerge first")]
    MergeOriginInColumn {
        /// The column.
        col: usize,
    },
    /// `merge` over a range that already contains a merged cell.
    #[error("the range contains a merged cell")]
    RangeContainsMerge,
    /// `merge` over a range that leaves the grid or is a single cell.
    #[error("the range is outside the grid or a single cell")]
    RangeOutsideGrid,
    /// `unmerge` on a cell that is not a merge origin.
    #[error("cell ({row}, {col}) is not the origin of a merge")]
    NotAMergeOrigin {
        /// Row.
        row: usize,
        /// Column.
        col: usize,
    },
    /// Deleting the only row.
    #[error("cannot delete the only row")]
    LastRow,
    /// Deleting the only column.
    #[error("cannot delete the only column")]
    LastColumn,
    /// The master index does not identify an entry in the presentation.
    #[error("slide master index {index} is out of range")]
    MasterIndexOutOfRange {
        /// Zero-based index in the master list.
        index: usize,
    },
    /// Another slide, notes or handout master uses the same theme.
    #[error("theme is shared by multiple masters")]
    SharedTheme,
    /// A table style id neither defined in `ppt/tableStyles.xml` nor
    /// built in.
    #[error("table style {0:?} is not defined")]
    TableStyleNotDefined(String),
    /// `set_style` on a table whose `a:tblPr` holds an inline
    /// `a:tableStyle` (the other branch of the schema choice), which is not
    /// replaced implicitly.
    #[error("the table has an inline a:tableStyle")]
    TableStyleInline,
    /// A value outside its schema range.
    #[error("value out of range for {what}")]
    ValueOutOfRange {
        /// The attribute.
        what: &'static str,
    },
    /// The requested shape fill is outside the supported choices.
    #[error("unsupported shape fill")]
    UnsupportedFill,
    /// The shape has custom geometry.
    #[error("cannot edit custom geometry as a preset")]
    UnsupportedGeometry,
    /// The adjustment is absent from the current preset definition.
    #[error("unknown preset adjustment {name:?}")]
    UnknownAdjustment {
        /// Guide name.
        name: String,
    },
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
    /// The slide has no notes slide (creating one is not part of Phase 1,
    /// ADR-0008 §2).
    #[error("slide id {slide_id} has no notes slide")]
    NoNotesSlide {
        /// `p:sldId@id`.
        slide_id: u32,
    },
    /// Existing notes slides have no presentation-level notes master relationship.
    #[error("the presentation has notes slides but no notes master relationship")]
    NotesWithoutMaster,
    /// The notes slide has no body placeholder (`p:ph@type="body"`).
    #[error("the notes slide has no body placeholder")]
    NoNotesBody,
    /// `slide_rect` of a shape inside a group with `rot`, `flipH` or
    /// `flipV`: the rectangle is not axis-aligned in slide space
    /// (ADR-0008 §1.5).
    #[error("an enclosing group (id {group_id:?}) is rotated or flipped")]
    TransformedGroup {
        /// `cNvPr@id` of that group.
        group_id: Option<u32>,
    },
    /// A link of the slide → layout → master → theme chain is missing or
    /// does not parse (ADR-0008 §1.1). Only resolver calls that need the
    /// link fail; Phase 0 reads and writes on that slide keep working.
    #[error("{from}: the {missing:?} of the chain is missing or unreadable")]
    BrokenChain {
        /// The part whose link is broken (the slide).
        from: String,
        /// Which link.
        missing: crate::ChainLink,
    },


    // ---- Phase 1b (ADR-0013 §4/§5): SmartArt ----
    /// The graphic frame is not a SmartArt, or its data part is missing.
    #[error("the shape is not a SmartArt with a data model")]
    NotSmartArt,
    /// No `node`/`asst` point with this `modelId`.
    #[error("no SmartArt node {0:?}")]
    SmartArtNodeNotFound(String),
    /// The drawing cache does not pair with the data model (ADR-0013 §4
    /// invariants).
    #[error("the SmartArt drawing cache does not match its data: {0}")]
    SmartArtCacheInconsistent(String),
    /// `dsp:dataModelExt@relId` does not resolve from the slide.
    #[error("the SmartArt drawing cache reference does not resolve")]
    SmartArtCacheUnresolved,
    /// The drawing part is referenced from somewhere else as well.
    #[error("the SmartArt drawing cache is shared")]
    SmartArtCacheShared,
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
            Error::NoNotesBody => Error::NoNotesBody,
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

impl Error {
    /// The variant name, e.g. `"MergedCell"`. Stable: renaming a variant is a
    /// breaking change recorded in the owning ADR (ADR-0023 §8).
    pub fn kind(&self) -> &'static str {
        match self {
            Error::UnsafeLinkScheme => "UnsafeLinkScheme",
            Error::InvalidLinkUrl => "InvalidLinkUrl",
            Error::UnsupportedAction => "UnsupportedAction",
            Error::ReadOnlyLinkTarget => "ReadOnlyLinkTarget",
            Error::SmartArtLinkUnsupported => "SmartArtLinkUnsupported",
            Error::LinkHasSound => "LinkHasSound",
            Error::ForeignLinkSlide => "ForeignLinkSlide",
            Error::LinkSlideNotFound { .. } => "LinkSlideNotFound",
            Error::UnsupportedBullet => "UnsupportedBullet",
            Error::InvalidSchemeColor => "InvalidSchemeColor",
            Error::RangeOutOfBounds => "RangeOutOfBounds",
            Error::RangeSplitsField => "RangeSplitsField",
            Error::Core(e) => e.kind(),
            Error::NotAPresentation(..) => "NotAPresentation",
            Error::StrictConformance => "StrictConformance",
            Error::SlideIndexOutOfRange { .. } => "SlideIndexOutOfRange",
            Error::DanglingSlide { .. } => "DanglingSlide",
            Error::ShapeNotFound => "ShapeNotFound",
            Error::AmbiguousShapeId { .. } => "AmbiguousShapeId",
            Error::InvalidShapeId => "InvalidShapeId",
            Error::InsideAlternateContent => "InsideAlternateContent",
            Error::NoTextBody => "NoTextBody",
            Error::NotATable => "NotATable",
            Error::CellNotFound { .. } => "CellNotFound",
            Error::MergedCell { .. } => "MergedCell",
            Error::ParagraphNotFound(..) => "ParagraphNotFound",
            Error::RunNotFound(..) => "RunNotFound",
            Error::FieldNotFound(..) => "FieldNotFound",
            Error::FieldInParagraph { .. } => "FieldInParagraph",
            Error::UnsupportedContent(..) => "UnsupportedContent",
            Error::LastParagraph => "LastParagraph",
            Error::AnimatedParagraphs { .. } => "AnimatedParagraphs",
            Error::AnimatedCharacters { .. } => "AnimatedCharacters",
            Error::CommentAnchoredText { .. } => "CommentAnchoredText",
            Error::InvalidXmlChar { .. } => "InvalidXmlChar",
            Error::LineBreakInRun => "LineBreakInRun",
            Error::LineBreakInParagraph => "LineBreakInParagraph",
            Error::FormatNotCopyable(..) => "FormatNotCopyable",
            Error::LayoutNotFound(..) => "LayoutNotFound",
            Error::SlideIdsExhausted => "SlideIdsExhausted",
            Error::ShapeIdsExhausted => "ShapeIdsExhausted",
            Error::CreationIdsExhausted => "CreationIdsExhausted",
            Error::SlideIsReferenced { .. } => "SlideIsReferenced",
            Error::SlideInCustomShow => "SlideInCustomShow",
            Error::ShowRangeByPosition => "ShowRangeByPosition",
            Error::UnsupportedSlidePart(..) => "UnsupportedSlidePart",
            Error::UnsupportedSections => "UnsupportedSections",
            Error::SectionsInconsistent => "SectionsInconsistent",
            Error::TableNotWellFormed(..) => "TableNotWellFormed",
            Error::CommentAnchoredTable { .. } => "CommentAnchoredTable",
            Error::MergeOriginInRow { .. } => "MergeOriginInRow",
            Error::MergeOriginInColumn { .. } => "MergeOriginInColumn",
            Error::RangeContainsMerge => "RangeContainsMerge",
            Error::RangeOutsideGrid => "RangeOutsideGrid",
            Error::NotAMergeOrigin { .. } => "NotAMergeOrigin",
            Error::LastRow => "LastRow",
            Error::LastColumn => "LastColumn",
            Error::MasterIndexOutOfRange { .. } => "MasterIndexOutOfRange",
            Error::SharedTheme => "SharedTheme",
            Error::TableStyleNotDefined(..) => "TableStyleNotDefined",
            Error::TableStyleInline => "TableStyleInline",
            Error::ValueOutOfRange { .. } => "ValueOutOfRange",
            Error::UnsupportedFill => "UnsupportedFill",
            Error::UnsupportedGeometry => "UnsupportedGeometry",
            Error::UnknownAdjustment { .. } => "UnknownAdjustment",
            Error::InvalidPattern => "InvalidPattern",
            Error::FieldInMatch => "FieldInMatch",
            Error::BreakInMatch => "BreakInMatch",
            Error::MixedFormatInMatch => "MixedFormatInMatch",
            Error::LinkSplitByBreak => "LinkSplitByBreak",
            Error::NoNotesSlide { .. } => "NoNotesSlide",
            Error::NotesWithoutMaster => "NotesWithoutMaster",
            Error::NoNotesBody => "NoNotesBody",
            Error::TransformedGroup { .. } => "TransformedGroup",
            Error::BrokenChain { .. } => "BrokenChain",
            Error::NotSmartArt => "NotSmartArt",
            Error::SmartArtNodeNotFound(..) => "SmartArtNodeNotFound",
            Error::SmartArtCacheInconsistent(..) => "SmartArtCacheInconsistent",
            Error::SmartArtCacheUnresolved => "SmartArtCacheUnresolved",
            Error::SmartArtCacheShared => "SmartArtCacheShared",
        }
    }

    /// The A1 cell the error is about, for variants that carry one.
    pub fn cell(&self) -> Option<&str> {
        match self {
            Error::UnsafeLinkScheme => None,
            Error::InvalidLinkUrl => None,
            Error::UnsupportedAction => None,
            Error::ReadOnlyLinkTarget => None,
            Error::SmartArtLinkUnsupported => None,
            Error::LinkHasSound => None,
            Error::ForeignLinkSlide => None,
            Error::LinkSlideNotFound { .. } => None,
            Error::UnsupportedBullet => None,
            Error::InvalidSchemeColor => None,
            Error::RangeOutOfBounds => None,
            Error::RangeSplitsField => None,
            Error::Core(e) => e.cell(),
            Error::NotAPresentation(..) => None,
            Error::StrictConformance => None,
            Error::SlideIndexOutOfRange { .. } => None,
            Error::DanglingSlide { .. } => None,
            Error::ShapeNotFound => None,
            Error::AmbiguousShapeId { .. } => None,
            Error::InvalidShapeId => None,
            Error::InsideAlternateContent => None,
            Error::NoTextBody => None,
            Error::NotATable => None,
            Error::CellNotFound { .. } => None,
            Error::MergedCell { .. } => None,
            Error::ParagraphNotFound(..) => None,
            Error::RunNotFound(..) => None,
            Error::FieldNotFound(..) => None,
            Error::FieldInParagraph { .. } => None,
            Error::UnsupportedContent(..) => None,
            Error::LastParagraph => None,
            Error::AnimatedParagraphs { .. } => None,
            Error::AnimatedCharacters { .. } => None,
            Error::CommentAnchoredText { .. } => None,
            Error::InvalidXmlChar { .. } => None,
            Error::LineBreakInRun => None,
            Error::LineBreakInParagraph => None,
            Error::FormatNotCopyable(..) => None,
            Error::LayoutNotFound(..) => None,
            Error::SlideIdsExhausted => None,
            Error::ShapeIdsExhausted => None,
            Error::CreationIdsExhausted => None,
            Error::SlideIsReferenced { .. } => None,
            Error::SlideInCustomShow => None,
            Error::ShowRangeByPosition => None,
            Error::UnsupportedSlidePart(..) => None,
            Error::UnsupportedSections => None,
            Error::SectionsInconsistent => None,
            Error::TableNotWellFormed(..) => None,
            Error::CommentAnchoredTable { .. } => None,
            Error::MergeOriginInRow { .. } => None,
            Error::MergeOriginInColumn { .. } => None,
            Error::RangeContainsMerge => None,
            Error::RangeOutsideGrid => None,
            Error::NotAMergeOrigin { .. } => None,
            Error::LastRow => None,
            Error::LastColumn => None,
            Error::MasterIndexOutOfRange { .. } => None,
            Error::SharedTheme => None,
            Error::TableStyleNotDefined(..) => None,
            Error::TableStyleInline => None,
            Error::ValueOutOfRange { .. } => None,
            Error::UnsupportedFill => None,
            Error::UnsupportedGeometry => None,
            Error::UnknownAdjustment { .. } => None,
            Error::InvalidPattern => None,
            Error::FieldInMatch => None,
            Error::BreakInMatch => None,
            Error::MixedFormatInMatch => None,
            Error::LinkSplitByBreak => None,
            Error::NoNotesSlide { .. } => None,
            Error::NotesWithoutMaster => None,
            Error::NoNotesBody => None,
            Error::TransformedGroup { .. } => None,
            Error::BrokenChain { .. } => None,
            Error::NotSmartArt => None,
            Error::SmartArtNodeNotFound(..) => None,
            Error::SmartArtCacheInconsistent(..) => None,
            Error::SmartArtCacheUnresolved => None,
            Error::SmartArtCacheShared => None,
        }
    }
}


/// One value of every variant (for the kind()/cell() tests and the binding table test).
#[doc(hidden)]
pub fn __error_samples() -> Vec<(Error, Option<&'static str>)> {
    vec![
        (Error::UnsafeLinkScheme, None),
        (Error::InvalidLinkUrl, None),
        (Error::UnsupportedAction, None),
        (Error::ReadOnlyLinkTarget, None),
        (Error::SmartArtLinkUnsupported, None),
        (Error::LinkHasSound, None),
        (Error::ForeignLinkSlide, None),
        (Error::LinkSlideNotFound { slide_id: 0 }, None),
        (Error::UnsupportedBullet, None),
        (Error::InvalidSchemeColor, None),
        (Error::RangeOutOfBounds, None),
        (Error::RangeSplitsField, None),
        (Error::Core(ooxml_core::Error::EncryptedPackage), None),
        (Error::NotAPresentation(String::from("x")), None),
        (Error::StrictConformance, None),
        (Error::SlideIndexOutOfRange { index: 0, count: 0 }, None),
        (Error::DanglingSlide { slide_id: 0 }, None),
        (Error::ShapeNotFound, None),
        (Error::AmbiguousShapeId { id: 0 }, None),
        (Error::InvalidShapeId, None),
        (Error::InsideAlternateContent, None),
        (Error::NoTextBody, None),
        (Error::NotATable, None),
        (Error::CellNotFound { row: 0, col: 0 }, None),
        (Error::MergedCell { row: 0, col: 0 }, None),
        (Error::ParagraphNotFound(0), None),
        (Error::RunNotFound(0), None),
        (Error::FieldNotFound(0), None),
        (Error::FieldInParagraph { paragraph: 0 }, None),
        (Error::UnsupportedContent(String::from("x")), None),
        (Error::LastParagraph, None),
        (Error::AnimatedParagraphs { shape_id: 0 }, None),
        (Error::AnimatedCharacters { shape_id: 0 }, None),
        (Error::CommentAnchoredText { shape_id: 0 }, None),
        (Error::InvalidXmlChar { ch: 'x', offset: 0 }, None),
        (Error::LineBreakInRun, None),
        (Error::LineBreakInParagraph, None),
        (Error::FormatNotCopyable("x"), None),
        (Error::LayoutNotFound(String::from("x")), None),
        (Error::SlideIdsExhausted, None),
        (Error::ShapeIdsExhausted, None),
        (Error::CreationIdsExhausted, None),
        (Error::SlideIsReferenced { from_part: String::from("x"), rel_type: String::from("x") }, None),
        (Error::SlideInCustomShow, None),
        (Error::ShowRangeByPosition, None),
        (Error::UnsupportedSlidePart(String::from("x")), None),
        (Error::UnsupportedSections, None),
        (Error::SectionsInconsistent, None),
        (Error::TableNotWellFormed(String::from("x")), None),
        (Error::CommentAnchoredTable { shape_id: 0 }, None),
        (Error::MergeOriginInRow { row: 0 }, None),
        (Error::MergeOriginInColumn { col: 0 }, None),
        (Error::RangeContainsMerge, None),
        (Error::RangeOutsideGrid, None),
        (Error::NotAMergeOrigin { row: 0, col: 0 }, None),
        (Error::LastRow, None),
        (Error::LastColumn, None),
        (Error::MasterIndexOutOfRange { index: 0 }, None),
        (Error::SharedTheme, None),
        (Error::TableStyleNotDefined(String::from("x")), None),
        (Error::TableStyleInline, None),
        (Error::ValueOutOfRange { what: "x" }, None),
        (Error::UnsupportedFill, None),
        (Error::UnsupportedGeometry, None),
        (Error::UnknownAdjustment { name: String::from("x") }, None),
        (Error::InvalidPattern, None),
        (Error::FieldInMatch, None),
        (Error::BreakInMatch, None),
        (Error::MixedFormatInMatch, None),
        (Error::LinkSplitByBreak, None),
        (Error::NoNotesSlide { slide_id: 0 }, None),
        (Error::NotesWithoutMaster, None),
        (Error::NoNotesBody, None),
        (Error::TransformedGroup { group_id: None }, None),
        (Error::BrokenChain { from: String::from("x"), missing: crate::ChainLink::Master }, None),
        (Error::NotSmartArt, None),
        (Error::SmartArtNodeNotFound(String::from("x")), None),
        (Error::SmartArtCacheInconsistent(String::from("x")), None),
        (Error::SmartArtCacheUnresolved, None),
        (Error::SmartArtCacheShared, None),
    ]
}

/// Every error kind as it surfaces to callers (`Core` excluded: a wrapped
/// core error surfaces as `("core", kind)`).
#[doc(hidden)]
pub fn __surface_kinds() -> Vec<&'static str> {
    __error_samples().into_iter().filter(|(e, _)| !matches!(e, Error::Core(_))).map(|(e, _)| e.kind()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_matches_debug_name() {
        for (error, _) in __error_samples() {
            if matches!(error, Error::Core(_)) {
                continue;
            }
            let debug = format!("{error:?}");
            let kind = error.kind();
            assert!(debug.starts_with(kind), "{debug}");
            // A prefix of another variant's name (Package/PackageTooLarge)
            // must not pass: the next character ends the identifier.
            assert!(debug[kind.len()..].chars().next().is_none_or(|c| !c.is_alphanumeric() && c != '_'), "{debug}");
        }
        let inner = ooxml_core::Error::EncryptedPackage;
        let (k, dbg) = (inner.kind().to_string(), format!("{inner:?}"));
        assert_eq!(Error::Core(inner).kind(), k);
        assert!(dbg.starts_with("EncryptedPackage"));
    }

    #[test]
    fn samples_cover_every_variant() {
        // A new variant without a sample would escape __surface_kinds and
        // fall silently into the binding's base class (ADR-0023 m2): the
        // deduped sample kinds (minus the Core sample, which surfaces as
        // core) must equal the kind() arms minus the delegating Core arm.
        let source = include_str!("error.rs");
        let start = source.find("pub fn kind(").expect("kind fn");
        let end = start + source[start..].find("pub fn cell(").expect("cell fn");
        let arms = source[start..end].matches("=>").count();
        let mut kinds: Vec<_> = __error_samples()
            .into_iter()
            .filter(|(e, _)| !matches!(e, Error::Core(_)))
            .map(|(e, _)| e.kind())
            .collect();
        kinds.sort_unstable();
        kinds.dedup();
        assert_eq!(kinds.len(), arms - 1, "a variant lacks a sample");
    }

    #[test]
    fn cell_reports_carrying_variants() {
        for (error, expected) in __error_samples() {
            assert_eq!(error.cell(), expected);
        }
    }
}

