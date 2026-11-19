//! Qualified names.
//!
//! Two shapes exist on purpose:
//!
//! - [`QName`] — a `'static` (namespace URI, local name) pair. Generated
//!   schema tables and hand-written code use it to *ask* for a name.
//! - [`XName`] — a name as it appears in a parsed document: the resolved
//!   namespace URI, the local name, and the **original prefix** (needed when
//!   a dirty node is re-rendered, ADR-0001 post-acceptance note).
//!
//! Prefixes are per-document spelling and are never compared
//! (ADR-0002 §3, "QName identity").

use std::fmt;
use std::sync::Arc;

/// `http://www.w3.org/XML/1998/namespace` — always bound to `xml`, never declared.
pub const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";
/// `http://www.w3.org/2000/xmlns/` — the namespace of `xmlns` / `xmlns:*` declarations.
pub const XMLNS_NS: &str = "http://www.w3.org/2000/xmlns/";
/// Markup Compatibility (`mc:`), acted on by the lens runtime (ADR-0002 §4.3).
pub const MC_NS: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";
/// Office document relationships (`r:`), used for `r:id`-style attributes.
pub const R_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
/// Strict (ECMA-376 Part 1, Strict) counterpart of [`R_NS`].
pub const R_NS_STRICT: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships";
/// VML Office namespace (`o:`), whose `o:relid` is a relationship id.
pub const VML_O_NS: &str = "urn:schemas-microsoft-com:office:office";
/// Attributes that hold a relationship id: every attribute in a
/// relationships namespace (`(ns, None)`), plus single names elsewhere
/// (`(ns, Some(local))`). Used by `Lens::detach_as` to refuse copies that
/// would duplicate a reference into Layer 3.
pub const RELATIONSHIP_ATTRIBUTES: &[(&str, Option<&str>)] =
    &[(R_NS, None), (R_NS_STRICT, None), (VML_O_NS, Some("relid"))];
/// OPC package relationships (`_rels/*.rels` root namespace).
pub const PACKAGE_RELATIONSHIPS_NS: &str =
    "http://schemas.openxmlformats.org/package/2006/relationships";
/// OPC content types (`[Content_Types].xml` root namespace).
pub const CONTENT_TYPES_NS: &str = "http://schemas.openxmlformats.org/package/2006/content-types";
/// Namespace URI prefix shared by every ECMA-376 **Strict** namespace and
/// relationship type. Packages using it are refused (ADR-0001 scope).
pub const STRICT_NS_PREFIX: &str = "http://purl.oclc.org/ooxml/";

/// `mc:AlternateContent`.
pub const MC_ALTERNATE_CONTENT: QName = QName::new(MC_NS, "AlternateContent");
/// `xml:space`.
pub const XML_SPACE: QName = QName::new(XML_NS, "space");

/// A `'static` qualified name: namespace URI plus local name.
///
/// An empty `ns` means "no namespace" (e.g. SpreadsheetML's unprefixed
/// attributes `r`, `s`, `t`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct QName {
    /// Namespace URI (`""` for no namespace).
    pub ns: &'static str,
    /// Local name.
    pub local: &'static str,
}

impl QName {
    /// Builds a name; usable in `const` items.
    pub const fn new(ns: &'static str, local: &'static str) -> Self {
        Self { ns, local }
    }

    /// `const` equality (stable `const` code cannot call `PartialEq`).
    pub const fn const_eq(self, other: QName) -> bool {
        crate::schema::const_str_eq(self.ns, other.ns)
            && crate::schema::const_str_eq(self.local, other.local)
    }
}

impl fmt::Display for QName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.ns.is_empty() {
            f.write_str(self.local)
        } else {
            write!(f, "{{{}}}{}", self.ns, self.local)
        }
    }
}

/// A name as found in (or inserted into) a document.
///
/// `xmlns` declarations are stored as attributes in [`XMLNS_NS`]: the
/// default declaration has local name `xmlns` and no prefix, `xmlns:p` has
/// prefix `xmlns` and local name `p` (the usual DOM convention).
///
/// One thin pointer (ADR-0010 stage 1a): the parser interns names per
/// document, so equal names usually share one allocation. Equality compares
/// the pointer first, then the content; the hash is computed from the
/// content only, so it agrees with equality.
#[derive(Clone)]
pub struct XName(Arc<XNameData>);

#[derive(Debug, PartialEq, Eq, Hash)]
struct XNameData {
    ns: Arc<str>,
    local: Box<str>,
    prefix: Option<Box<str>>,
}

impl PartialEq for XName {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0) || *self.0 == *other.0
    }
}

impl Eq for XName {}

impl std::hash::Hash for XName {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl fmt::Debug for XName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("XName").field("ns", &self.0.ns).field("local", &self.0.local).field("prefix", &self.0.prefix).finish()
    }
}

impl XName {
    /// Builds an owned name from its parts.
    pub fn new(ns: Arc<str>, local: &str, prefix: Option<&str>) -> Self {
        XName(Arc::new(XNameData { ns, local: local.into(), prefix: prefix.map(Into::into) }))
    }

    /// Resolved namespace URI (`""` for no namespace).
    pub fn ns(&self) -> &str {
        &self.0.ns
    }

    /// Local name.
    pub fn local(&self) -> &str {
        &self.0.local
    }

    /// Original (or chosen) prefix; `None` means unprefixed.
    pub fn prefix(&self) -> Option<&str> {
        self.0.prefix.as_deref()
    }

    /// True if this name is `q` (namespace URI and local name; prefix ignored).
    pub fn is(&self, q: QName) -> bool {
        *self.0.ns == *q.ns && *self.0.local == *q.local
    }

    /// True for `xmlns` / `xmlns:*` declarations.
    pub fn is_namespace_declaration(&self) -> bool {
        &*self.0.ns == XMLNS_NS
    }

    /// The prefix a namespace declaration declares: `None` for the default
    /// namespace (`xmlns="…"`), `Some(p)` for `xmlns:p`. `None` also for
    /// non-declarations; check [`Self::is_namespace_declaration`] first.
    pub fn declared_prefix(&self) -> Option<&str> {
        if self.is_namespace_declaration() && self.0.prefix.is_some() {
            Some(&self.0.local)
        } else {
            None
        }
    }

    /// Displays the name as written in markup (`prefix:local` or `local`),
    /// without building an intermediate `String`.
    pub fn display(&self) -> impl fmt::Display + '_ {
        fmt::from_fn(move |f| match &self.0.prefix {
            Some(p) => write!(f, "{p}:{}", self.0.local),
            None => f.write_str(&self.0.local),
        })
    }
}
