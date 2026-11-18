//! Frozen Layer 2 parser and serializer from main at 3b5013b.

use ooxml_core::{
    ParseOptions,
    error::{Error, Result, WellFormedness},
    name::XMLNS_NS,
};
use std::{fmt, ops::Range, sync::Arc};

#[path = "frozen_parser.rs"]
mod parse;
#[path = "frozen_writer.rs"]
mod write;

pub(super) struct XmlDocument {
    source: Box<str>,
    root: RawNode,
}
impl XmlDocument {
    pub(super) fn parse(bytes: &[u8]) -> Result<Self> {
        parse::parse(bytes, &ParseOptions::default())
    }
    pub(super) fn root(&self) -> &RawNode {
        &self.root
    }
    fn source(&self) -> &str {
        &self.source
    }
    pub(super) fn force_rerender(&mut self) {
        self.root.force_rerender();
    }
    pub(super) fn serialize(&self) -> String {
        write::serialize(self)
    }
}

/// Where a parsed element sits in its part's source text (byte offsets).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ElementSpan {
    /// Offset of the open tag's `<`.
    pub start: usize,
    /// Just past the open tag's final `>` (past `/>` if self-closing).
    pub open_end: usize,
    /// `(start, end)` of the close tag; `None` iff the element was self-closing.
    pub close: Option<(usize, usize)>,
}

impl ElementSpan {
    /// End of the whole element (past the close tag, or past `/>`).
    pub fn end(&self) -> usize {
        self.close.map_or(self.open_end, |(_, e)| e)
    }
}

/// An attribute: resolved name (with original prefix) and unescaped value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawAttr {
    pub(crate) name: XName,
    pub(crate) value: String,
}

impl RawAttr {
    /// Name.
    pub fn name(&self) -> &XName {
        &self.name
    }
    /// Unescaped value (attribute-value normalized per XML 1.0).
    pub fn value(&self) -> &str {
        &self.value
    }
}

/// Character data (text or CDATA).
#[derive(Clone, Debug)]
pub struct RawText {
    pub(crate) value: String,
    /// Original bytes; `None` for text written through the lens (escaped on output).
    pub(crate) span: Option<Range<usize>>,
    pub(crate) cdata: bool,
}

impl RawText {
    /// Decoded text (entity and character references resolved, EOLs normalized).
    pub fn value(&self) -> &str {
        &self.value
    }
    /// Was it a `<![CDATA[…]]>` section?
    pub fn is_cdata(&self) -> bool {
        self.cdata
    }
    /// Original span, if parsed and unchanged.
    pub fn span(&self) -> Option<Range<usize>> {
        self.span.clone()
    }
}

/// Kinds of markup kept verbatim.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkupKind {
    /// `<!-- … -->`
    Comment,
    /// `<? … ?>`
    ProcessingInstruction,
}

/// A comment or processing instruction inside the root element, kept as its
/// original markup and always emitted verbatim.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawMarkup {
    pub(crate) kind: MarkupKind,
    pub(crate) raw: Box<str>,
}

impl RawMarkup {
    /// Kind.
    pub fn kind(&self) -> MarkupKind {
        self.kind
    }
    /// The original markup, delimiters included.
    pub fn raw(&self) -> &str {
        &self.raw
    }
}

/// A child item of an element. Order is preserved.
#[derive(Clone, Debug)]
pub enum RawItem {
    /// Element.
    Element(RawNode),
    /// Text or CDATA.
    Text(RawText),
    /// Comment or processing instruction.
    Markup(RawMarkup),
}

/// An element of the raw tree (ADR-0001 Layer 2, with names as resolved
/// QName + original prefix per its post-acceptance note).
#[derive(Clone, Debug)]
pub struct RawNode {
    pub(crate) name: XName,
    pub(crate) attrs: Vec<RawAttr>,
    pub(crate) children: Vec<RawItem>,
    /// `None` for an element created through the lens.
    pub(crate) span: Option<ElementSpan>,
    pub(crate) own_dirty: bool,
    pub(crate) subtree_dirty: bool,
}

impl RawNode {
    /// Element name.
    pub fn name(&self) -> &XName {
        &self.name
    }
    /// All attributes in document order, `xmlns` declarations included.
    pub fn attrs(&self) -> &[RawAttr] {
        &self.attrs
    }
    /// Child items (elements, text, markup) in document order.
    pub fn children(&self) -> &[RawItem] {
        &self.children
    }
    /// Original position in the source; `None` if created through the lens.
    pub fn span(&self) -> Option<ElementSpan> {
        self.span
    }

    fn force_rerender(&mut self) {
        self.own_dirty = true;
        self.subtree_dirty = true;
        for c in &mut self.children {
            match c {
                RawItem::Element(n) => n.force_rerender(),
                RawItem::Text(t) => t.span = None,
                RawItem::Markup(_) => {}
            }
        }
    }
}

/// A name as found in (or inserted into) a document.
///
/// `xmlns` declarations are stored as attributes in [`XMLNS_NS`]: the
/// default declaration has local name `xmlns` and no prefix, `xmlns:p` has
/// prefix `xmlns` and local name `p` (the usual DOM convention).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct XName {
    ns: Arc<str>,
    local: Box<str>,
    prefix: Option<Box<str>>,
}

impl XName {
    /// Builds a name from its parts.
    pub fn new(ns: Arc<str>, local: &str, prefix: Option<&str>) -> Self {
        Self {
            ns,
            local: local.into(),
            prefix: prefix.map(Into::into),
        }
    }

    /// Resolved namespace URI (`""` for no namespace).
    pub fn ns(&self) -> &str {
        &self.ns
    }

    /// Local name.
    pub fn local(&self) -> &str {
        &self.local
    }

    /// Original (or chosen) prefix; `None` means unprefixed.
    pub fn prefix(&self) -> Option<&str> {
        self.prefix.as_deref()
    }

    /// True for `xmlns` / `xmlns:*` declarations.
    pub fn is_namespace_declaration(&self) -> bool {
        &*self.ns == XMLNS_NS
    }

    /// Displays the name as written in markup (`prefix:local` or `local`),
    /// without building an intermediate `String`.
    pub fn display(&self) -> impl fmt::Display + '_ {
        fmt::from_fn(move |f| match &self.prefix {
            Some(p) => write!(f, "{p}:{}", self.local),
            None => f.write_str(&self.local),
        })
    }
}
