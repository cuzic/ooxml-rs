//! Layer 2: the lossless raw XML tree (ADR-0001).
//!
//! A part is parsed into an [`XmlDocument`]: the original text plus a tree of
//! [`RawNode`]s. Every parsed element remembers its [`ElementSpan`] in the
//! source, and two dirty flags decide how it is written back:
//!
//! - `!subtree_dirty` → the element's full original span is emitted verbatim;
//! - `subtree_dirty && !own_dirty` → original open/close tags verbatim,
//!   children recursed;
//! - `own_dirty` → the open tag is re-rendered from name/attributes, the
//!   close tag rendered if there are children, else the tag self-closes.
//!
//! Bytes before and after the root element (XML declaration, BOM, comments)
//! are always emitted verbatim.
//!
//! The tree is only mutated through [`LensMut`], which owns
//! the dirty-flag duties (ADR-0002 §4.2).

mod parse;
mod write;

use std::borrow::Cow;
use std::ops::Range;

use crate::error::Result;
use crate::lens::{Lens, LensMut};
use crate::name::{QName, XML_SPACE, XName};
use crate::schema::ElementSpec;

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
    /// Was the element written as `<x/>`?
    pub fn is_self_closing(&self) -> bool {
        self.close.is_none()
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
    /// Is this text whitespace only?
    pub fn is_whitespace(&self) -> bool {
        self.value.bytes().all(|b| matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
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

impl RawItem {
    /// The element, if this item is one.
    pub fn as_element(&self) -> Option<&RawNode> {
        match self {
            RawItem::Element(n) => Some(n),
            _ => None,
        }
    }
    pub(crate) fn as_element_mut(&mut self) -> Option<&mut RawNode> {
        match self {
            RawItem::Element(n) => Some(n),
            _ => None,
        }
    }
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
    pub(crate) fn new_created(name: XName) -> Self {
        Self {
            name,
            attrs: Vec::new(),
            children: Vec::new(),
            span: None,
            own_dirty: true,
            subtree_dirty: true,
        }
    }

    /// Element name.
    pub fn name(&self) -> &XName {
        &self.name
    }
    /// Is this element `q`?
    pub fn is(&self, q: QName) -> bool {
        self.name.is(q)
    }
    /// All attributes in document order, `xmlns` declarations included.
    pub fn attrs(&self) -> &[RawAttr] {
        &self.attrs
    }
    /// Value of attribute `q`, if present.
    pub fn attr(&self, q: QName) -> Option<&str> {
        self.attrs.iter().find(|a| a.name.is(q)).map(|a| a.value.as_str())
    }
    /// Child items (elements, text, markup) in document order.
    pub fn children(&self) -> &[RawItem] {
        &self.children
    }
    /// Child elements in document order.
    pub fn elements(&self) -> impl Iterator<Item = &RawNode> {
        self.children.iter().filter_map(RawItem::as_element)
    }
    /// Child elements with their item index (the index used in node paths).
    pub fn indexed_elements(&self) -> impl Iterator<Item = (usize, &RawNode)> {
        self.children.iter().enumerate().filter_map(|(i, c)| c.as_element().map(|n| (i, n)))
    }
    /// The concatenated text children (no descendants).
    pub fn text(&self) -> Cow<'_, str> {
        let mut texts = self.children.iter().filter_map(|c| match c {
            RawItem::Text(t) => Some(t.value.as_str()),
            _ => None,
        });
        let Some(first) = texts.next() else {
            return Cow::Borrowed("");
        };
        match texts.next() {
            None => Cow::Borrowed(first),
            Some(second) => {
                let mut s = String::from(first);
                s.push_str(second);
                texts.for_each(|t| s.push_str(t));
                Cow::Owned(s)
            }
        }
    }
    /// Original position in the source; `None` if created through the lens.
    pub fn span(&self) -> Option<ElementSpan> {
        self.span
    }
    /// This element's own name/attributes changed, or it was created, or it
    /// was self-closing and gained a child.
    pub fn own_dirty(&self) -> bool {
        self.own_dirty
    }
    /// This element or a descendant changed.
    pub fn subtree_dirty(&self) -> bool {
        self.subtree_dirty
    }
    /// Is `xml:space="preserve"` set on this element itself?
    pub fn xml_space_preserve(&self) -> Option<bool> {
        self.attr(XML_SPACE).map(|v| v == "preserve")
    }

    /// Structural equality used by the T1b oracle: same names (namespace,
    /// local, prefix), same attributes in the same order, same child order,
    /// same text values and markup. Spans and dirty flags are ignored.
    pub fn structurally_eq(&self, other: &RawNode) -> bool {
        self.name == other.name
            && self.attrs == other.attrs
            && self.children.len() == other.children.len()
            && self.children.iter().zip(&other.children).all(|(a, b)| match (a, b) {
                (RawItem::Element(a), RawItem::Element(b)) => a.structurally_eq(b),
                (RawItem::Text(a), RawItem::Text(b)) => a.value == b.value && a.cdata == b.cdata,
                (RawItem::Markup(a), RawItem::Markup(b)) => a == b,
                _ => false,
            })
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

/// Follows a child-index path from `root`.
pub(crate) fn node_at<'a>(root: &'a RawNode, path: &[usize]) -> &'a RawNode {
    path.iter().fold(root, |n, &i| {
        n.children[i].as_element().expect("node path points at an element")
    })
}

pub(crate) fn node_at_mut<'a>(root: &'a mut RawNode, path: &[usize]) -> &'a mut RawNode {
    path.iter().fold(root, |n, &i| {
        n.children[i].as_element_mut().expect("node path points at an element")
    })
}

/// Default for [`ParseOptions::max_depth`].
///
/// The deepest element nesting in the corpora we test against (Apache POI
/// and Open XML SDK test files, 556 parts measured) is 33, the median 9;
/// 1024 leaves a 30x margin while keeping every recursive walk over the
/// tree (serialization, `Drop`, structural comparison, format-crate walks)
/// far from the 2 MiB default thread stack, and bounds path-based lens
/// addressing to O(1024) per node.
pub const DEFAULT_MAX_DEPTH: usize = 1024;

/// Options for [`XmlDocument::parse_with`] and
/// [`Package::from_bytes_with`](crate::Package::from_bytes_with).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct ParseOptions {
    /// Maximum element nesting depth; the root element is depth 1. A deeper
    /// element makes the parse fail with
    /// [`Error::TooDeep`](crate::Error::TooDeep).
    pub max_depth: usize,
}

impl Default for ParseOptions {
    fn default() -> Self {
        ParseOptions { max_depth: DEFAULT_MAX_DEPTH }
    }
}

impl ParseOptions {
    /// Sets [`Self::max_depth`].
    pub fn with_max_depth(mut self, max_depth: usize) -> Self {
        self.max_depth = max_depth;
        self
    }
}

/// A parsed XML part: the original source plus the raw tree.
#[derive(Clone, Debug)]
pub struct XmlDocument {
    source: Box<str>,
    root: RawNode,
}

impl XmlDocument {
    /// Parses a part. The input must be UTF-8 (a UTF-8 BOM is allowed and
    /// preserved); UTF-16 input returns [`Error::UnsupportedEncoding`](crate::Error::UnsupportedEncoding).
    ///
    /// Uses [`ParseOptions::default`]: elements nested deeper than
    /// [`DEFAULT_MAX_DEPTH`] return [`Error::TooDeep`](crate::Error::TooDeep).
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        parse::parse(bytes, &ParseOptions::default())
    }

    /// [`Self::parse`] with explicit options.
    pub fn parse_with(bytes: &[u8], options: &ParseOptions) -> Result<Self> {
        parse::parse(bytes, options)
    }

    /// The original source text.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// The root element.
    pub fn root(&self) -> &RawNode {
        &self.root
    }

    /// True if anything was changed through the lens.
    pub fn is_dirty(&self) -> bool {
        self.root.subtree_dirty
    }

    /// Serializes per the Layer 2 rules (verbatim spans for untouched
    /// subtrees). With no edits the result equals the input (T1).
    pub fn serialize(&self) -> String {
        write::serialize(self)
    }

    /// A read-only typed lens on the root element.
    pub fn lens<E: ElementSpec>(&self) -> Result<Lens<'_, E>> {
        Lens::root(&self.root)
    }

    /// A mutable typed lens on the root element.
    pub fn lens_mut<E: ElementSpec>(&mut self) -> Result<LensMut<'_, E>> {
        LensMut::root(&mut self.root)
    }

    /// Marks every node `own_dirty` + `subtree_dirty` and forgets text spans,
    /// so that [`Self::serialize`] re-renders the whole tree from
    /// names/attributes/children. This is the T1b oracle's set-up
    /// (ADR-0001); not meant for normal editing.
    pub fn force_rerender(&mut self) {
        self.root.force_rerender();
    }
}
