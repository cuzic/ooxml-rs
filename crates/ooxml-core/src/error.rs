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
    /// No usable prefix is bound to the namespace at this point.
    #[error("namespace {0:?} is not in scope with a usable prefix")]
    NamespaceNotInScope(String),
    /// A typed read found an invalid lexical value.
    #[error(transparent)]
    Attr(#[from] crate::schema::AttrError),
}

/// `Result` with [`Error`].
pub type Result<T, E = Error> = std::result::Result<T, E>;
