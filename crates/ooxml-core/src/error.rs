//! The crate-wide error type.

use crate::name::QName;

/// Errors from `ooxml-core`.
///
/// Lens write refusals (`AlreadyPresent`, `UnresolvedSibling`, …) are the
/// "fail honestly rather than silently produce a corrupt file" cases of
/// ADR-0001 / ADR-0002 §4.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// I/O failure.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// The zip container could not be read or written.
    #[error("zip error: {0}")]
    Zip(#[from] zip::result::ZipError),
    /// The file is an OLE/CFB compound file: an encrypted (password
    /// protected) or legacy binary Office document, not an OPC zip package.
    #[error("the file is a CFB/OLE compound file (encrypted or legacy Office document), not an OOXML package")]
    EncryptedPackage,
    /// The package uses ECMA-376 Strict namespaces; only Transitional is
    /// supported (ADR-0001 scope).
    #[error("the package uses ECMA-376 Strict namespaces ({0}); only Transitional is supported")]
    StrictNotSupported(String),
    /// No part with that name exists.
    #[error("no part named {0:?}")]
    PartNotFound(String),
    /// Adding new parts to a package is not implemented yet (ADR-0001 Phase0).
    #[error("adding a new part ({0:?}) is not supported yet")]
    NewPartUnsupported(String),
    /// A part, or OPC metadata, is malformed.
    #[error("malformed package: {0}")]
    Package(String),
    /// A part is not well-formed XML (or uses a construct the parser refuses).
    #[error("XML error at byte {position}: {message}")]
    Xml {
        /// Byte offset in the part.
        position: usize,
        /// Description.
        message: String,
    },
    /// The part contains a `<!DOCTYPE …>` declaration. OOXML parts never
    /// use DTDs, so any part with one is refused (no entity expansion, no
    /// external entities).
    #[error("DTD (<!DOCTYPE>) at byte {position} is not allowed in an OOXML part")]
    DtdNotAllowed {
        /// Byte offset of the declaration.
        position: usize,
    },
    /// Elements are nested deeper than the limit
    /// ([`ParseOptions::max_depth`](crate::xml::ParseOptions::max_depth),
    /// default [`DEFAULT_MAX_DEPTH`](crate::xml::DEFAULT_MAX_DEPTH)).
    /// Refused while parsing, before the tree grows, so a few kilobytes of
    /// hostile input cannot make later recursive code (`Clone`, `Drop`,
    /// `Debug`, serialization, format-crate walks) exhaust memory or the
    /// stack. A lens insert that would exceed the document's limit is
    /// refused the same way.
    #[error("element nesting at byte {position} exceeds the depth limit of {limit}")]
    TooDeep {
        /// The limit in force.
        limit: usize,
        /// Byte offset of the start tag that would exceed it (parse), or of
        /// the parent's start tag in the original source for a refused
        /// insert (0 if the parent was itself inserted).
        position: usize,
    },
    /// The part is not UTF-8 encoded.
    #[error("unsupported XML encoding: {0}")]
    UnsupportedEncoding(String),
    /// The root element is not the one requested.
    #[error("expected element {expected}, found {{{found_ns}}}{found_local}")]
    WrongElement {
        /// Requested name.
        expected: QName,
        /// Namespace of the element found.
        found_ns: String,
        /// Local name of the element found.
        found_local: String,
    },
    /// The child's limit (`max_each` or `max_total` of its slot) is reached.
    #[error("{0} cannot be inserted: its slot is full")]
    AlreadyPresent(QName),
    /// The child's QName is in two slots; use `insert_in_slot`.
    #[error("{0} appears in more than one slot; use insert_in_slot")]
    AmbiguousSlot(QName),
    /// The parent has an `mc:AlternateContent` child, or its existing
    /// children violate the schema order, so no safe insertion point exists.
    #[error("cannot position a new child of {0}: mc:AlternateContent sibling or out-of-order children")]
    UnresolvedSibling(QName),
    /// The parent's content model could not be lowered to slots.
    #[error("the content model of {0} is unsupported for insertion")]
    UnsupportedContentModel(QName),
    /// The name is not allowed by the content model (or the requested slot).
    #[error("{child} is not allowed here in {parent}")]
    NotAllowed {
        /// Parent element.
        parent: QName,
        /// Rejected child.
        child: QName,
    },
    /// The element is not a text-only element.
    #[error("{0} does not have text content")]
    NotText(QName),
    /// The element's spec is marked `attrs_incomplete` (unresolved base class).
    #[error("{0}'s attribute list is incomplete in the schema data; writes are refused")]
    IncompleteSpec(QName),
    /// [`LensMut::move_in_by`](crate::LensMut::move_in_by) refused the move
    /// (ADR-0002 post-acceptance note "same-part move").
    #[error("cannot move {name}: {reason}")]
    NotMovable {
        /// Why.
        reason: MoveRefusal,
        /// The element, prefix or namespace concerned.
        name: String,
    },
    /// No usable prefix is bound to the namespace at this point.
    #[error("namespace {0:?} is not in scope with a usable prefix")]
    NamespaceNotInScope(String),
    /// A namespace declaration requested for a new element
    /// ([`NsDecl`](crate::lens::NsDecl)) is not acceptable: an empty or
    /// reserved URI, a prefix that is not an NCName or starts with `xml`, or
    /// a prefix or URI given twice.
    #[error("invalid namespace declaration xmlns:{prefix}={uri:?}: {reason}")]
    InvalidNamespaceDeclaration {
        /// The requested prefix.
        prefix: String,
        /// The requested URI.
        uri: String,
        /// Why it was refused.
        reason: &'static str,
    },
    /// A text or attribute value contains a character that XML 1.0 does not
    /// allow (`Char` production: C0 controls other than tab/LF/CR,
    /// U+FFFE, U+FFFF). Writing it would make the part not well-formed.
    #[error("character U+{:04X} at byte {offset} is not allowed in XML 1.0", u32::from(*ch))]
    InvalidXmlChar {
        /// The offending character.
        ch: char,
        /// Its byte offset in the value.
        offset: usize,
    },
    /// [`Lens::detach_as`](crate::Lens::detach_as) refused the subtree
    /// (ADR-0002 post-acceptance note "detach/insert").
    #[error("cannot detach: {reason} ({name})")]
    NotDetachable {
        /// Why.
        reason: DetachRefusal,
        /// The offending element or attribute, in Clark notation (`{ns}local`;
        /// just `local` without a namespace).
        name: String,
    },
    /// [`LensMut::insert_at`](crate::LensMut::insert_at): the child index is
    /// beyond the parent's children.
    #[error("insertion index {index} is beyond the {len} children")]
    InsertIndexOutOfRange {
        /// The requested index.
        index: usize,
        /// The number of child items.
        len: usize,
    },
    /// A typed read found an invalid lexical value.
    #[error(transparent)]
    Attr(#[from] crate::schema::AttrError),
    /// [`Package::remove_part`](crate::Package::remove_part) refused: a
    /// precondition does not hold (nothing was changed).
    #[error("cannot remove part {part:?}: {reason}")]
    RemovePartRefused {
        /// The part.
        part: String,
        /// Which precondition failed.
        reason: crate::package::RemoveRefusal,
    },
    /// The part is not well-formed XML in a way the underlying reader
    /// (quick-xml) would let through. Refused at parse time so that the tree
    /// never holds something a re-render could not write back, and element
    /// spans are always exact.
    #[error("XML at byte {position} is not well-formed: {reason}")]
    NotWellFormed {
        /// Byte offset in the part.
        position: usize,
        /// What is wrong.
        reason: WellFormedness,
    },
    /// [`LensMut::set_attr`](crate::LensMut::set_attr) or
    /// [`LensMut::remove_attr`](crate::LensMut::remove_attr) on a namespace
    /// declaration (`xmlns`, `xmlns:*`). Declarations decide what every name
    /// in the subtree means, so they are managed by ooxml-core, never written
    /// through the lens.
    #[error("{0} is a namespace declaration; the lens never writes namespace declarations")]
    NamespaceDeclaration(QName),
    /// A zip entry's stored data does not lie inside the archive (bogus
    /// compressed size or local-header offsets). Copying it through would
    /// write different bytes, so the package is refused at open.
    #[error("zip entry {part:?} is truncated: its data ends at byte {end}, past the end of the archive ({len} bytes)")]
    TruncatedEntry {
        /// The entry name.
        part: String,
        /// Where its data would end (saturated at `u64::MAX`).
        end: u64,
        /// The archive length.
        len: u64,
    },
    /// A part decompresses (or declares that it decompresses) to more than
    /// [`ParseOptions::max_part_bytes`](crate::xml::ParseOptions::max_part_bytes).
    #[error("part {part:?} decompresses to more than the limit of {limit} bytes")]
    PartTooLarge {
        /// The part.
        part: String,
        /// The limit in force.
        limit: u64,
    },
    /// The parts' declared decompressed sizes add up to more than
    /// [`ParseOptions::max_package_bytes`](crate::xml::ParseOptions::max_package_bytes).
    #[error("the package's parts declare {size} decompressed bytes in total, more than the limit of {limit}")]
    PackageTooLarge {
        /// The sum of the declared sizes (saturated at `u64::MAX`).
        size: u64,
        /// The limit in force.
        limit: u64,
    },
}

/// Why a part is [`Error::NotWellFormed`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum WellFormedness {
    /// A byte order mark after the first one (only one, at byte 0, is a BOM;
    /// U+FEFF is not allowed before the root element).
    ByteOrderMark,
    /// An XML declaration (`<?xml …?>`) anywhere but at the start of the
    /// document.
    XmlDeclarationNotAtStart,
    /// An element or attribute with an empty local name or an empty prefix
    /// (`< />`, `<:a/>`).
    EmptyName,
    /// A U+0000 character (not allowed anywhere in XML 1.0).
    NullCharacter,
    /// `xmlns:p=""`: Namespaces in XML 1.0 forbids undeclaring a prefix.
    EmptyPrefixBinding,
    /// The reader reported a construct at a position that does not match
    /// the source text; refused rather than recording a wrong span.
    PositionMismatch,
}

impl std::fmt::Display for WellFormedness {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            WellFormedness::ByteOrderMark => "a byte order mark (U+FEFF) after the start of the document",
            WellFormedness::XmlDeclarationNotAtStart => "an XML declaration that is not at the start of the document",
            WellFormedness::EmptyName => "an empty element or attribute name or prefix",
            WellFormedness::NullCharacter => "a U+0000 character",
            WellFormedness::EmptyPrefixBinding => "a namespace prefix bound to the empty string",
            WellFormedness::PositionMismatch => "the reader's position does not match the source",
        })
    }
}

/// `Result` with [`Error`].
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Why a same-part move was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum MoveRefusal {
    /// The part's root element cannot be moved.
    Root,
    /// The destination is the moved element or inside it.
    IntoItself,
    /// A prefix the subtree uses without declaring it (on an element or
    /// attribute, the default namespace, or named in a Markup Compatibility
    /// attribute value such as `mc:Ignorable` or `mc:Choice/@Requires`)
    /// means another namespace, or nothing, at the destination; the moved
    /// bytes would change meaning.
    NamespaceBinding,
    /// A namespace the subtree uses is ignorable (`mc:Ignorable`) at one
    /// place and not at the other.
    IgnorableNamespace,
    /// The subtree has text that inherits `xml:space`, and the inherited
    /// value differs at the destination.
    XmlSpace,
    /// The source or the destination is inside `mc:AlternateContent` (a
    /// `mc:Choice` / `mc:Fallback` branch): consumers reading the other
    /// branch would see the element vanish or appear.
    AlternateContent,
}

impl std::fmt::Display for MoveRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            MoveRefusal::Root => "the root element",
            MoveRefusal::IntoItself => "into itself",
            MoveRefusal::NamespaceBinding => "a prefix is bound differently at the destination",
            MoveRefusal::IgnorableNamespace => "a namespace is ignorable at only one place",
            MoveRefusal::XmlSpace => "inherited xml:space differs",
            MoveRefusal::AlternateContent => "inside mc:AlternateContent",
        })
    }
}

/// Why [`Lens::detach_as`](crate::Lens::detach_as) refused a subtree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum DetachRefusal {
    /// An `mc:AlternateContent` element in the subtree (ADR-0002 §4.3:
    /// branches are not edited).
    AlternateContent,
    /// Another Markup Compatibility element or attribute (`mc:Ignorable`
    /// and friends name prefixes, which a copy re-chooses).
    MarkupCompatibility,
    /// An attribute that holds a relationship id (`r:id`, `r:embed`, the
    /// Strict `r:` namespace, VML `o:relid`; `name::RELATIONSHIP_ATTRIBUTES`):
    /// a copy would add a second reference to a relationship (Layer 3).
    RelationshipAttribute,
    /// A namespace declaration on an element of the subtree.
    NamespaceDeclaration,
    /// An element or attribute in a namespace that an ancestor's
    /// `mc:Ignorable` lists, at the source (`detach_as`) or at the
    /// destination (`insert_detached`): the copy would lose or gain
    /// ignorability.
    IgnorableNamespace,
    /// `xml:space` in effect differs between source and destination
    /// (inherited from ancestors) and the subtree has text: whitespace would
    /// change meaning.
    InheritedXmlSpace,
    /// An attribute listed in [`crate::Detachable::UNIQUE_ATTRS`] (a
    /// document-unique id such as DrawingML `smtId`) has a non-default
    /// value: a copy would duplicate it.
    UniqueAttribute,
    /// An extension list (`extLst`, any namespace) in the kept subtree: its
    /// content is open, so ids or references inside cannot be reviewed.
    ExtensionList,
}

impl std::fmt::Display for DetachRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            DetachRefusal::AlternateContent => "mc:AlternateContent in the subtree",
            DetachRefusal::MarkupCompatibility => "markup-compatibility element or attribute in the subtree",
            DetachRefusal::RelationshipAttribute => "relationship (r:) attribute in the subtree",
            DetachRefusal::NamespaceDeclaration => "namespace declaration in the subtree",
            DetachRefusal::IgnorableNamespace => "namespace made ignorable by an ancestor's mc:Ignorable",
            DetachRefusal::InheritedXmlSpace => "text under a different inherited xml:space",
            DetachRefusal::UniqueAttribute => "a document-unique id attribute with a non-default value",
            DetachRefusal::ExtensionList => "an extension list (extLst) in the subtree",
        })
    }
}
