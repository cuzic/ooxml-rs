//! Error conversion: Rust errors become Python exceptions (ADR-0023 §11.3).
//!
//! The category table is keyed by `(crate, kind)` and covers every surfacing
//! kind (a test fails if one is missing). A wrapped core error surfaces as
//! `("core", kind)` whatever crate wrapped it.

use std::collections::BTreeMap;

use pyo3::prelude::*;
use pyo3::types::{PyDict, PyType};
use pyo3::PyTypeInfo as _;

pyo3::import_exception!(ooxml_rs, OoxmlError);
pyo3::import_exception!(ooxml_rs, RefusedError);
pyo3::import_exception!(ooxml_rs, PoisonedError);
pyo3::import_exception!(ooxml_rs, NotFoundError);
pyo3::import_exception!(ooxml_rs, InvalidInputError);
pyo3::import_exception!(ooxml_rs, UnsupportedFileError);
pyo3::import_exception!(ooxml_rs, MalformedFileError);
pyo3::import_exception!(ooxml_rs, OoxmlIOError);
pyo3::import_exception!(ooxml_rs, OoxmlFileNotFoundError);
pyo3::import_exception!(ooxml_rs, OoxmlPermissionError);

/// The Python category (exception class) of a `(crate, kind)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Category {
    Refused,
    Poisoned,
    NotFound,
    InvalidInput,
    UnsupportedFile,
    MalformedFile,
    /// Split at raise time by `errno`: NotFound/Permission/base.
    Io,
}

/// `(crate, kind)` → category. One entry per surfacing kind; `__table_keys`
/// plus the coverage test keep it complete. Judgement calls (see the U7 PR):
/// Rule: the operation cannot proceed on a valid file -> Refused
/// (`Unsupported*`, exhaustion, `Random`/`*Randomness`, ambiguous file
/// state (`Ambiguous*`), missing optional parts the library does not
/// create: `NoStylesPart`, `NoSettingsPart`, `NoTextBody`, `NoNotesSlide`,
/// `NoNotesBody`, `NoSectionProperties`); the file breaks a format
/// invariant -> MalformedFile; the caller's pick is wrong (`NotA*`,
/// `MasterIndexOutOfRange`) or mixed caller/file image and name families
/// (`ImageSize`, `NameInUse`) -> InvalidInput.
static TABLE: &[((&str, &str), Category)] = &[
    // --- core ---
    (("core", "Io"), Category::Io),
    (("core", "Zip"), Category::MalformedFile),
    (("core", "EncryptedPackage"), Category::UnsupportedFile),
    (("core", "StrictNotSupported"), Category::UnsupportedFile),
    (("core", "PartNotFound"), Category::MalformedFile),
    (("core", "NewPartUnsupported"), Category::Refused),
    (("core", "Package"), Category::MalformedFile),
    (("core", "Xml"), Category::MalformedFile),
    (("core", "DtdNotAllowed"), Category::UnsupportedFile),
    (("core", "TooDeep"), Category::MalformedFile),
    (("core", "UnsupportedEncoding"), Category::MalformedFile),
    (("core", "WrongElement"), Category::MalformedFile),
    (("core", "AlreadyPresent"), Category::Refused),
    (("core", "AmbiguousSlot"), Category::Refused),
    (("core", "UnresolvedSibling"), Category::Refused),
    (("core", "UnsupportedContentModel"), Category::Refused),
    (("core", "NotAllowed"), Category::Refused),
    (("core", "NotText"), Category::MalformedFile),
    (("core", "IncompleteSpec"), Category::Refused),
    (("core", "NotMovable"), Category::Refused),
    (("core", "NamespaceNotInScope"), Category::Refused),
    (("core", "InvalidNamespaceDeclaration"), Category::Refused),
    (("core", "InvalidXmlChar"), Category::InvalidInput),
    (("core", "NoSuchElement"), Category::MalformedFile),
    (("core", "NotLeaf"), Category::MalformedFile),
    (("core", "TypedNamespace"), Category::MalformedFile),
    (("core", "NilElement"), Category::MalformedFile),
    (("core", "NotDetachable"), Category::Refused),
    (("core", "InsertIndexOutOfRange"), Category::InvalidInput),
    (("core", "WrongDocument"), Category::Refused),
    (("core", "Attr"), Category::MalformedFile),
    (("core", "RemovePartRefused"), Category::Refused),
    (("core", "RegistryRefused"), Category::Refused),
    (("core", "NotWellFormed"), Category::MalformedFile),
    (("core", "NamespaceDeclaration"), Category::Refused),
    (("core", "TruncatedEntry"), Category::MalformedFile),
    (("core", "XmlPartTooLarge"), Category::MalformedFile),
    (("core", "PartTooLarge"), Category::MalformedFile),
    (("core", "PackageTooLarge"), Category::MalformedFile),
    // --- xlsx ---
    (("xlsx", "InvalidStructuralBand"), Category::InvalidInput),
    (("xlsx", "ShiftOffSheet"), Category::Refused),
    (("xlsx", "StructureBlocked"), Category::Refused),
    (("xlsx", "ReferenceDeleted"), Category::Refused),
    (("xlsx", "ArrayFormulaSplit"), Category::Refused),
    (("xlsx", "InvalidSparklineGroup"), Category::InvalidInput),
    (("xlsx", "PartNotEditable"), Category::Refused),
    (("xlsx", "Protected"), Category::Refused),
    (("xlsx", "NoteExists"), Category::Refused),
    (("xlsx", "ThreadedComment"), Category::Refused),
    (("xlsx", "NoSuchNote"), Category::NotFound),
    (("xlsx", "ImageFormat"), Category::InvalidInput),
    (("xlsx", "ImageSize"), Category::InvalidInput),
    (("xlsx", "ImageAnchor"), Category::InvalidInput),
    (("xlsx", "NoSuchImage"), Category::NotFound),
    (("xlsx", "UnresolvedRichValue"), Category::MalformedFile),
    (("xlsx", "NoWorkbook"), Category::MalformedFile),
    (("xlsx", "NoVisibleWorksheet"), Category::MalformedFile),
    (("xlsx", "Malformed"), Category::MalformedFile),
    (("xlsx", "FormulaScanFailed"), Category::Refused),
    (("xlsx", "MergedCell"), Category::Refused),
    (("xlsx", "RichValueCell"), Category::Refused),
    (("xlsx", "TableHeaderCell"), Category::Refused),
    (("xlsx", "MissingReference"), Category::MalformedFile),
    (("xlsx", "OutOfOrder"), Category::MalformedFile),
    (("xlsx", "InvalidReference"), Category::InvalidInput),
    (("xlsx", "OutOfRange"), Category::InvalidInput),
    (("xlsx", "InvalidNumber"), Category::InvalidInput),
    (("xlsx", "StringTooLong"), Category::InvalidInput),
    (("xlsx", "InvalidValue"), Category::MalformedFile),
    (("xlsx", "InvalidArgument"), Category::InvalidInput),
    (("xlsx", "SheetAdditionUnsupported"), Category::Refused),
    (("xlsx", "ArrayResultCell"), Category::Refused),
    (("xlsx", "ArrayAnchor"), Category::Refused),
    (("xlsx", "UnknownCellMetadata"), Category::Refused),
    (("xlsx", "MacroSheetDependent"), Category::Refused),
    (("xlsx", "IterativeCalculation"), Category::Refused),
    (("xlsx", "SharedFormulaMaster"), Category::Refused),
    (("xlsx", "InvalidFormula"), Category::InvalidInput),
    (("xlsx", "NoSuchSheet"), Category::NotFound),
    (("xlsx", "NotAWorksheet"), Category::InvalidInput),
    (("xlsx", "NoSuchName"), Category::NotFound),
    (("xlsx", "AmbiguousName"), Category::InvalidInput),
    (("xlsx", "NameNotARange"), Category::InvalidInput),
    (("xlsx", "Poisoned"), Category::Poisoned),
    (("xlsx", "SheetNamingUnsupported"), Category::Refused),
    (("xlsx", "DuplicateCellWrite"), Category::InvalidInput),
    (("xlsx", "MissingFunctionPrefix"), Category::InvalidInput),
    (("xlsx", "DynamicArrayOperator"), Category::InvalidInput),
    (("xlsx", "InvalidSheetName"), Category::InvalidInput),
    (("xlsx", "DuplicateSheetName"), Category::InvalidInput),
    (("xlsx", "LastVisibleSheet"), Category::Refused),
    (("xlsx", "SharedWorkbook"), Category::Refused),
    (("xlsx", "SheetHasUnsupportedParts"), Category::Refused),
    (("xlsx", "SheetIsPivotSource"), Category::Refused),
    (("xlsx", "TableInUse"), Category::Refused),
    (("xlsx", "TableRange"), Category::InvalidInput),
    (("xlsx", "InvalidTableName"), Category::InvalidInput),
    (("xlsx", "DuplicateColumnName"), Category::InvalidInput),
    (("xlsx", "HeaderNotText"), Category::InvalidInput),
    (("xlsx", "ColumnInUse"), Category::Refused),
    (("xlsx", "NoSuchTable"), Category::NotFound),
    (("xlsx", "CustomViewReferencesSheet"), Category::Refused),
    (("xlsx", "UnrewritableReference"), Category::Refused),
    (("xlsx", "ReversesThreeDReference"), Category::Refused),
    (("xlsx", "BinaryReferencesPossible"), Category::Refused),
    (("xlsx", "SheetInUse"), Category::Refused),
    (("xlsx", "NoStylesPart"), Category::Refused),
    (("xlsx", "UnsupportedStylesPart"), Category::Refused),
    (("xlsx", "StyleTableFull"), Category::Refused),
    (("xlsx", "NumFmtIdExhausted"), Category::Refused),
    (("xlsx", "BuiltinNumFormatRedefined"), Category::Refused),
    (("xlsx", "InvalidFormat"), Category::InvalidInput),
    (("xlsx", "InvalidNumFormat"), Category::InvalidInput),
    (("xlsx", "StyleIndexOutOfRange"), Category::InvalidInput),
    (("xlsx", "NoSuchStyle"), Category::NotFound),
    (("xlsx", "DanglingReference"), Category::MalformedFile),
    (("xlsx", "NonexistentDate"), Category::InvalidInput),
    (("xlsx", "AmbiguousStyleName"), Category::Refused),
    (("xlsx", "NotExpressible"), Category::Refused),
    (("xlsx", "DateOutOfRange"), Category::InvalidInput),
    (("xlsx", "MergeSingleCell"), Category::InvalidInput),
    (("xlsx", "MergeOverlap"), Category::Refused),
    (("xlsx", "MergeWouldHideValues"), Category::Refused),
    (("xlsx", "NoSuchMerge"), Category::NotFound),
    (("xlsx", "TableOverlap"), Category::Refused),
    (("xlsx", "PivotOverlap"), Category::Refused),
    (("xlsx", "QueryTableOverlap"), Category::Refused),
    (("xlsx", "AutoFilterExists"), Category::Refused),
    (("xlsx", "FilterActive"), Category::Refused),
    (("xlsx", "DuplicateBuiltInName"), Category::InvalidInput),
    (("xlsx", "NameInUse"), Category::InvalidInput),
    (("xlsx", "InvalidSpinCount"), Category::InvalidInput),
    (("xlsx", "CrossSheetReference"), Category::InvalidInput),
    (("xlsx", "UnknownSheet"), Category::NotFound),
    (("xlsx", "ValidationOverlap"), Category::Refused),
    (("xlsx", "ListTooLong"), Category::InvalidInput),
    (("xlsx", "ListItemHasComma"), Category::InvalidInput),
    (("xlsx", "HyperlinkOverlap"), Category::Refused),
    (("xlsx", "InvalidUrl"), Category::InvalidInput),
    (("xlsx", "PivotConditionalFormat"), Category::Refused),
    (("xlsx", "NoSuchRule"), Category::NotFound),
    (("xlsx", "NothingToFreeze"), Category::Refused),
    (("xlsx", "PageLayoutView"), Category::Refused),
    (("xlsx", "AmbiguousColumns"), Category::Refused),
    (("xlsx", "InvalidOutlineLevel"), Category::InvalidInput),
    (("xlsx", "InvalidHeaderFooter"), Category::InvalidInput),
    (("xlsx", "InvalidSetting"), Category::InvalidInput),
    (("xlsx", "Random"), Category::Refused),
    // --- docx ---
    (("docx", "SdtIdRandomness"), Category::Refused),
    (("docx", "UnsupportedControl"), Category::Refused),
    (("docx", "ControlLocked"), Category::Refused),
    (("docx", "UnsupportedBinding"), Category::Refused),
    (("docx", "BindingMismatch"), Category::Refused),
    (("docx", "UnsupportedDateFormat"), Category::Refused),
    (("docx", "NoFollowingParagraph"), Category::Refused),
    (("docx", "TrackedParagraphFormatting"), Category::Refused),
    (("docx", "BookmarkSpansContainers"), Category::Refused),
    (("docx", "BookmarkReferenced"), Category::Refused),
    (("docx", "DuplicateBookmarkName"), Category::InvalidInput),
    (("docx", "NameReferencedByField"), Category::Refused),
    (("docx", "InvalidBookmark"), Category::InvalidInput),
    (("docx", "InvalidCommentPart"), Category::MalformedFile),
    (("docx", "InvalidTextRange"), Category::InvalidInput),
    (("docx", "NoSuchComment"), Category::NotFound),
    (("docx", "LegacyCommentsExtended"), Category::Refused),
    (("docx", "NestedReply"), Category::InvalidInput),
    (("docx", "ParentNotAddressable"), Category::Refused),
    (("docx", "NotIgnorable"), Category::Refused),
    (("docx", "CommentIdRandomness"), Category::Refused),
    (("docx", "AnnotationIdsExhausted"), Category::Refused),
    (("docx", "StyleCycle"), Category::MalformedFile),
    (("docx", "InvalidNotePart"), Category::MalformedFile),
    (("docx", "NoteIdsExhausted"), Category::Refused),
    (("docx", "InvalidNumbering"), Category::MalformedFile),
    (("docx", "UnknownNumId"), Category::NotFound),
    (("docx", "NumberingIdsExhausted"), Category::Refused),
    (("docx", "NoStylesPart"), Category::MalformedFile),
    (("docx", "BuiltInStyleName"), Category::InvalidInput),
    (("docx", "DuplicateStyleName"), Category::InvalidInput),
    (("docx", "StyleIdInUse"), Category::InvalidInput),
    (("docx", "MainPartMissing"), Category::MalformedFile),
    (("docx", "NoBody"), Category::MalformedFile),
    (("docx", "SectionBreakAtEnd"), Category::Refused),
    (("docx", "TrackedSectionBreakDeletion"), Category::Refused),
    (("docx", "TrackRevisionsOn"), Category::Refused),
    (("docx", "DocumentProtected"), Category::Refused),
    (("docx", "StaleFieldRisk"), Category::Refused),
    (("docx", "InsideField"), Category::Refused),
    (("docx", "InsideRevision"), Category::Refused),
    (("docx", "UnsupportedContext"), Category::Refused),
    (("docx", "RunNotTextOnly"), Category::Refused),
    (("docx", "InvalidXmlChar"), Category::InvalidInput),
    (("docx", "EmptyNeedle"), Category::InvalidInput),
    (("docx", "ReplaceRefused"), Category::Refused),
    (("docx", "StaleMatch"), Category::Refused),
    (("docx", "OverlappingMatches"), Category::Refused),
    (("docx", "NoSuchStory"), Category::NotFound),
    (("docx", "DuplicateProperty"), Category::InvalidInput),
    (("docx", "InvalidValue"), Category::InvalidInput),
    (("docx", "TrackedPropertyChange"), Category::Refused),
    (("docx", "StyleChainTooDeep"), Category::MalformedFile),
    (("docx", "UnknownStyle"), Category::NotFound),
    (("docx", "StyleTypeMismatch"), Category::InvalidInput),
    (("docx", "AmbiguousStyle"), Category::Refused),
    (("docx", "AmbiguousStylesPart"), Category::Refused),
    (("docx", "NoSettingsPart"), Category::Refused),
    (("docx", "AmbiguousSettings"), Category::Refused),
    (("docx", "DuplicateSetting"), Category::InvalidInput),
    (("docx", "AlreadySectionEnd"), Category::Refused),
    (("docx", "NoSectionProperties"), Category::Refused),
    (("docx", "RelIdReferenced"), Category::Refused),
    (("docx", "UnknownBookmark"), Category::NotFound),
    (("docx", "NestedHyperlink"), Category::InvalidInput),
    (("docx", "InconsistentGrid"), Category::MalformedFile),
    (("docx", "NoSuchTable"), Category::NotFound),
    (("docx", "WouldRemoveNote"), Category::Refused),
    (("docx", "RevisionInRange"), Category::Refused),
    (("docx", "AnchorWouldBreak"), Category::Refused),
    (("docx", "TableWouldBeEmpty"), Category::Refused),
    (("docx", "RowWouldBeEmpty"), Category::Refused),
    (("docx", "CellNotEmpty"), Category::Refused),
    (("docx", "MergeOverlaps"), Category::Refused),
    (("docx", "MergeNonEmpty"), Category::Refused),
    (("docx", "UnsupportedImage"), Category::Refused),
    (("docx", "DrawingIdsExhausted"), Category::Refused),
    (("docx", "NoSuchParagraph"), Category::NotFound),
    (("docx", "NoSuchRun"), Category::NotFound),
    // --- pptx ---
    (("pptx", "UnsafeLinkScheme"), Category::InvalidInput),
    (("pptx", "InvalidLinkUrl"), Category::InvalidInput),
    (("pptx", "UnsupportedAction"), Category::Refused),
    (("pptx", "ReadOnlyLinkTarget"), Category::Refused),
    (("pptx", "SmartArtLinkUnsupported"), Category::Refused),
    (("pptx", "LinkHasSound"), Category::Refused),
    (("pptx", "ForeignLinkSlide"), Category::Refused),
    (("pptx", "LinkSlideNotFound"), Category::NotFound),
    (("pptx", "UnsupportedBullet"), Category::Refused),
    (("pptx", "InvalidSchemeColor"), Category::InvalidInput),
    (("pptx", "RangeOutOfBounds"), Category::InvalidInput),
    (("pptx", "RangeSplitsField"), Category::Refused),
    (("pptx", "NotAPresentation"), Category::MalformedFile),
    (("pptx", "StrictConformance"), Category::UnsupportedFile),
    (("pptx", "SlideIndexOutOfRange"), Category::InvalidInput),
    (("pptx", "DanglingSlide"), Category::MalformedFile),
    (("pptx", "ShapeNotFound"), Category::NotFound),
    (("pptx", "AmbiguousShapeId"), Category::Refused),
    (("pptx", "InvalidShapeId"), Category::MalformedFile),
    (("pptx", "InsideAlternateContent"), Category::Refused),
    (("pptx", "NoTextBody"), Category::Refused),
    (("pptx", "NotATable"), Category::InvalidInput),
    (("pptx", "CellNotFound"), Category::NotFound),
    (("pptx", "MergedCell"), Category::Refused),
    (("pptx", "ParagraphNotFound"), Category::NotFound),
    (("pptx", "RunNotFound"), Category::NotFound),
    (("pptx", "FieldNotFound"), Category::NotFound),
    (("pptx", "FieldInParagraph"), Category::Refused),
    (("pptx", "UnsupportedContent"), Category::Refused),
    (("pptx", "LastParagraph"), Category::Refused),
    (("pptx", "AnimatedParagraphs"), Category::Refused),
    (("pptx", "AnimatedCharacters"), Category::Refused),
    (("pptx", "CommentAnchoredText"), Category::Refused),
    (("pptx", "InvalidXmlChar"), Category::InvalidInput),
    (("pptx", "LineBreakInRun"), Category::InvalidInput),
    (("pptx", "LineBreakInParagraph"), Category::InvalidInput),
    (("pptx", "FormatNotCopyable"), Category::Refused),
    (("pptx", "LayoutNotFound"), Category::NotFound),
    (("pptx", "SlideIdsExhausted"), Category::Refused),
    (("pptx", "ShapeIdsExhausted"), Category::Refused),
    (("pptx", "CreationIdsExhausted"), Category::Refused),
    (("pptx", "SlideIsReferenced"), Category::Refused),
    (("pptx", "SlideInCustomShow"), Category::Refused),
    (("pptx", "ShowRangeByPosition"), Category::Refused),
    (("pptx", "UnsupportedSlidePart"), Category::Refused),
    (("pptx", "UnsupportedSections"), Category::Refused),
    (("pptx", "SectionsInconsistent"), Category::MalformedFile),
    (("pptx", "TableNotWellFormed"), Category::MalformedFile),
    (("pptx", "CommentAnchoredTable"), Category::Refused),
    (("pptx", "MergeOriginInRow"), Category::Refused),
    (("pptx", "MergeOriginInColumn"), Category::Refused),
    (("pptx", "RangeContainsMerge"), Category::Refused),
    (("pptx", "RangeOutsideGrid"), Category::Refused),
    (("pptx", "NotAMergeOrigin"), Category::InvalidInput),
    (("pptx", "LastRow"), Category::Refused),
    (("pptx", "LastColumn"), Category::Refused),
    (("pptx", "MasterIndexOutOfRange"), Category::InvalidInput),
    (("pptx", "SharedTheme"), Category::Refused),
    (("pptx", "TableStyleNotDefined"), Category::NotFound),
    (("pptx", "TableStyleInline"), Category::Refused),
    (("pptx", "ValueOutOfRange"), Category::InvalidInput),
    (("pptx", "UnsupportedFill"), Category::Refused),
    (("pptx", "UnsupportedGeometry"), Category::Refused),
    (("pptx", "UnknownAdjustment"), Category::NotFound),
    (("pptx", "InvalidPattern"), Category::InvalidInput),
    (("pptx", "FieldInMatch"), Category::Refused),
    (("pptx", "BreakInMatch"), Category::Refused),
    (("pptx", "MixedFormatInMatch"), Category::Refused),
    (("pptx", "LinkSplitByBreak"), Category::Refused),
    (("pptx", "NoNotesSlide"), Category::Refused),
    (("pptx", "NotesWithoutMaster"), Category::MalformedFile),
    (("pptx", "NoNotesBody"), Category::Refused),
    (("pptx", "TransformedGroup"), Category::Refused),
    (("pptx", "BrokenChain"), Category::Refused),
    (("pptx", "NotSmartArt"), Category::InvalidInput),
    (("pptx", "SmartArtNodeNotFound"), Category::NotFound),
    (("pptx", "SmartArtCacheInconsistent"), Category::MalformedFile),
    (("pptx", "SmartArtCacheUnresolved"), Category::MalformedFile),
    (("pptx", "SmartArtCacheShared"), Category::Refused),
];

/// The category of a `(crate, kind)`. Unknown pairs (unreachable: the
/// coverage test pins the table against `__surface_kinds`) raise the base.
fn categorize(crate_name: &str, kind: &str) -> Option<Category> {
    TABLE.iter().find(|((c, k), _)| *c == crate_name && *k == kind).map(|(_, cat)| *cat)
}

/// What the binding needs from a crate error.
pub trait CrateError: std::fmt::Display {
    /// The crate label, or `"core"` for a wrapped core error.
    fn crate_name(&self) -> &'static str;
    /// The variant name (`kind()`).
    fn error_kind(&self) -> &'static str;
    /// The A1 cell, when the error carries one.
    fn error_cell(&self) -> Option<&str>;
    /// The OS error, for `Io` (including wrapped).
    fn io_error(&self) -> Option<&std::io::Error>;
}

impl CrateError for ooxml_core::Error {
    fn crate_name(&self) -> &'static str {
        "core"
    }
    fn error_kind(&self) -> &'static str {
        self.kind()
    }
    fn error_cell(&self) -> Option<&str> {
        self.cell()
    }
    fn io_error(&self) -> Option<&std::io::Error> {
        match self {
            ooxml_core::Error::Io(e) => Some(e),
            _ => None,
        }
    }
}

macro_rules! impl_format_error {
    ($ty:ty, $name:literal, $core:path) => {
        impl CrateError for $ty {
            fn crate_name(&self) -> &'static str {
                match self {
                    $core(_) => "core",
                    _ => $name,
                }
            }
            fn error_kind(&self) -> &'static str {
                self.kind()
            }
            fn error_cell(&self) -> Option<&str> {
                self.cell()
            }
            fn io_error(&self) -> Option<&std::io::Error> {
                match self {
                    $core(e) => e.io_error(),
                    _ => None,
                }
            }
        }
    };
}

impl_format_error!(ooxml_xlsx::Error, "xlsx", ooxml_xlsx::Error::Core);
impl_format_error!(ooxml_docx::Error, "docx", ooxml_docx::Error::Core);
impl_format_error!(ooxml_pptx::Error, "pptx", ooxml_pptx::Error::Core);

/// Raises the category exception for a crate error. `sheet` is the call's
/// sheet (when it had one); `filename` the call's path, if any.
pub fn convert<E: CrateError>(
    py: Python<'_>,
    error: E,
    sheet: Option<&str>,
    filename: Option<&str>,
) -> PyErr {
    let crate_name = error.crate_name();
    let kind = error.error_kind().to_owned();
    let cell = error.error_cell().map(str::to_owned);
    let message = error.to_string();
    let kwargs = PyDict::new(py);
    kwargs.set_item("crate", crate_name).expect("str");
    kwargs.set_item("kind", &kind).expect("str");
    kwargs.set_item("cell", cell.as_deref()).expect("cell");
    kwargs.set_item("sheet", sheet).expect("sheet");
    let mut io_kind = None;
    if categorize(crate_name, &kind) == Some(Category::Io)
        && let Some(io) = error.io_error()
    {
        io_kind = Some(io.kind());
        // On Windows raw_os_error() is a Win32 code, not an errno: derive
        // errno from the kind (POSIX values, as CPython reports them) and
        // pass the raw code as winerror.
        #[cfg(windows)]
        let errno = errno_from_kind(io.kind());
        #[cfg(not(windows))]
        let errno = io.raw_os_error();
        kwargs.set_item("errno", errno).expect("errno");
        let strerror = io
            .raw_os_error()
            .map(|code| std::io::Error::from_raw_os_error(code).to_string())
            .unwrap_or_else(|| message.clone());
        kwargs.set_item("strerror", strerror).expect("strerror");
        kwargs.set_item("filename", filename).expect("filename");
        #[cfg(windows)]
        kwargs.set_item("winerror", io.raw_os_error()).expect("winerror");
    }
    let class = category_class(py, categorize(crate_name, &kind), io_kind);
    PyErr::from_value(class.call((message,), Some(&kwargs)).expect("exception __init__"))
}

/// POSIX errno for an I/O kind (Windows: CPython reports these values).
#[cfg(windows)]
fn errno_from_kind(kind: std::io::ErrorKind) -> Option<i32> {
    use std::io::ErrorKind::*;
    match kind {
        NotFound => Some(2),
        PermissionDenied => Some(13),
        AlreadyExists => Some(17),
        InvalidInput | InvalidData => Some(22),
        Interrupted => Some(4),
        BrokenPipe => Some(32),
        // TimedOut/ConnectionRefused have no portable errno (Linux-only
        // values differ on Windows); file operations never raise them.
        _ => None,
    }
}

/// The Python class for a category (shared by `convert` and `__raise` so
/// the mapping pins verify the real selection).
fn category_class(
    py: Python<'_>,
    category: Option<Category>,
    io_kind: Option<std::io::ErrorKind>,
) -> Bound<'_, PyType> {
    match category {
        Some(Category::Refused) => RefusedError::type_object(py),
        Some(Category::Poisoned) => PoisonedError::type_object(py),
        Some(Category::NotFound) => NotFoundError::type_object(py),
        Some(Category::InvalidInput) => InvalidInputError::type_object(py),
        Some(Category::UnsupportedFile) => UnsupportedFileError::type_object(py),
        Some(Category::MalformedFile) => MalformedFileError::type_object(py),
        Some(Category::Io) if io_kind == Some(std::io::ErrorKind::NotFound) => {
            OoxmlFileNotFoundError::type_object(py)
        }
        Some(Category::Io) if io_kind == Some(std::io::ErrorKind::PermissionDenied) => {
            OoxmlPermissionError::type_object(py)
        }
        Some(Category::Io) => OoxmlIOError::type_object(py),
        // Unreachable (coverage test); fail closed on the base class.
        None => OoxmlError::type_object(py),
    }
}

/// Raises a binding-level error (`crate="binding"`).
pub fn convert_binding(
    py: Python<'_>,
    kind: &'static str,
    poisoned: bool,
    message: String,
) -> PyErr {
    let kwargs = PyDict::new(py);
    kwargs.set_item("crate", "binding").expect("str");
    kwargs.set_item("kind", kind).expect("str");
    kwargs.set_item("cell", py.None()).expect("cell");
    kwargs.set_item("sheet", py.None()).expect("sheet");
    let class = if poisoned {
        PoisonedError::type_object(py)
    } else {
        RefusedError::type_object(py)
    };
    PyErr::from_value(class.call((message,), Some(&kwargs)).expect("exception __init__"))
}

/// `(crate, [kinds])` for the table-coverage test.
#[pyfunction]
pub fn __kinds() -> BTreeMap<&'static str, Vec<&'static str>> {
    BTreeMap::from([
        ("core", ooxml_core::__surface_kinds()),
        ("xlsx", ooxml_xlsx::__surface_kinds()),
        ("docx", ooxml_docx::__surface_kinds()),
        ("pptx", ooxml_pptx::__surface_kinds()),
    ])
}

/// The table's keys for the coverage test.
#[pyfunction]
pub fn __table_keys() -> Vec<(String, String)> {
    TABLE.iter().map(|((c, k), _)| ((*c).to_owned(), (*k).to_owned())).collect()
}

/// Raises the category exception for a `(crate, kind)` (test hook: pins the
/// mapping behaviourally without fabricating Rust errors).
#[pyfunction]
pub fn __raise(py: Python<'_>, crate_name: &str, kind: &str) -> PyResult<()> {
    let kwargs = PyDict::new(py);
    kwargs.set_item("crate", crate_name).expect("str");
    kwargs.set_item("kind", kind).expect("str");
    kwargs.set_item("cell", py.None()).expect("cell");
    kwargs.set_item("sheet", py.None()).expect("sheet");
    let class = category_class(py, categorize(crate_name, kind), None);
    Err(PyErr::from_value(
        class.call((format!("{crate_name}::{kind} (raised by the test hook)"),), Some(&kwargs)).expect("exception __init__"),
    ))
}
