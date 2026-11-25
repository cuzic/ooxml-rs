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
//!   A lens can opt into preserving unchanged attribute spellings and the
//!   existing close tag.
//!
//! Bytes before and after the root element (XML declaration, BOM, comments)
//! are always emitted verbatim.
//!
//! [`LensMut`] and the schemaless leaf-text API maintain the dirty flags
//! when editing the tree (ADR-0002 §4.2 and ADR-0015 §6.2).

mod interner;
mod leaf_text;
mod node;
mod parse;
mod source;
mod write;

use std::sync::Arc;

pub use leaf_text::LeafStep;
pub use node::RawNode;
pub(crate) use source::SStr;
use std::ops::Range;

use crate::error::Result;
use crate::lens::{Lens, LensMut};
use crate::name::{QName, XName};
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
    pub(crate) value: SStr,
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
    pub(crate) value: SStr,
    /// Original bytes; `None` for text written through the lens (escaped on output).
    span: [u32; 2],
    pub(crate) cdata: bool,
}

impl RawText {
    // Source ranges are ordered. A reversed range marks text that's written
    // from its value, even if this item still shares the source allocation.
    const WRITTEN: [u32; 2] = [1, 0];

    pub(crate) fn written(value: SStr, cdata: bool) -> Self {
        Self { value, span: Self::WRITTEN, cdata }
    }

    fn parsed(value: SStr, span: Range<usize>, cdata: bool) -> Self {
        Self { value, span: [span.start as u32, span.end as u32], cdata }
    }

    fn forget_span(&mut self) {
        self.span = Self::WRITTEN;
    }

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
        let [start, end] = self.span;
        (start <= end).then_some(start as usize..end as usize)
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
    pub(crate) raw: SStr,
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

/// Follows a child-index path from `root`.
pub(crate) fn node_at<'a>(root: &'a RawNode, path: &[usize]) -> &'a RawNode {
    path.iter().fold(root, |n, &i| {
        n.children()[i].as_element().expect("node path points at an element")
    })
}

pub(crate) fn node_at_mut<'a>(root: &'a mut RawNode, path: &[usize]) -> &'a mut RawNode {
    path.iter().fold(root, |n, &i| {
        n.child_mut(i).and_then(RawItem::as_element_mut).expect("node path points at an element")
    })
}

/// Marks the target and its ancestors dirty, then returns the target.
pub(crate) fn mark_path_dirty<'a>(root: &'a mut RawNode, path: &[usize]) -> &'a mut RawNode {
    root.mark_subtree_dirty();
    path.iter().fold(root, |node, &index| node.mark_child_dirty(index).expect("validated element path"))
}

/// Clears the dirty flags of the nodes along `path` (root first) from the
/// deepest up, while each has no dirty child element left and `may_clean`
/// agrees. Keeps the parents' counts.
fn clean_up(node: &mut RawNode, path: &[usize], depth: usize, may_clean: &mut impl FnMut(usize, &RawNode) -> bool) {
    if let Some((&i, rest)) = path.split_first() {
        let child = node.child_mut(i).and_then(RawItem::as_element_mut).expect("validated element path");
        let was_dirty = child.subtree_dirty();
        clean_up(child, rest, depth + 1, may_clean);
        if was_dirty && !child.subtree_dirty() {
            node.sub_dirty_child();
        }
    }
    if node.dirty_child_count() == 0 && node.subtree_dirty() && may_clean(depth, node) {
        node.clear_dirty();
    }
}

/// Default for [`ParseOptions::max_depth`].
///
/// The deepest element nesting in the parts we test against (Apache POI,
/// Open XML SDK, calamine, LibreOffice and python-pptx test files: 1,641
/// parts) is 33, the median 9. 256 leaves a 7x margin (a docx table nests
/// three elements per level, so 256 still allows about 80 nested tables).
///
/// The limit is what keeps the recursive operations on a tree (`Clone`,
/// `Drop`, `Debug`, dirty serialization, [`RawNode::structurally_eq`],
/// format-crate walks) inside a thread's stack. At depth 1024 a debug-build
/// `clone()` needs about 1.7 MiB, most of the 2 MiB default thread stack;
/// at 256 every one of them runs on a 1 MiB stack in a debug build
/// (`tests/depth_limit.rs`), and path-based lens addressing costs at most
/// 256 steps.
pub const DEFAULT_MAX_DEPTH: usize = 256;

/// Default for [`ParseOptions::max_part_bytes`]: 1 GiB.
///
/// Far above any XML part in the test corpora (the largest is a few MiB),
/// and above the decompressed size of nearly every media part; reading a
/// part is the only time it is decompressed (untouched parts are copied
/// through compressed).
pub const DEFAULT_MAX_PART_BYTES: u64 = 1 << 30;

/// Default for [`ParseOptions::max_package_bytes`]: 4 GiB.
pub const DEFAULT_MAX_PACKAGE_BYTES: u64 = 4 << 30;

/// Options for [`XmlDocument::parse_with`] and
/// [`Package::from_bytes_with`](crate::Package::from_bytes_with).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct ParseOptions {
    /// Maximum element nesting depth; the root element is depth 1. A deeper
    /// element makes the parse fail with
    /// [`Error::TooDeep`](crate::Error::TooDeep). The tree remembers it, and
    /// [`XmlDocument::lens_mut`] refuses inserts that would exceed it.
    ///
    /// **Raising it** raises the stack use of every recursive operation on
    /// the tree in proportion (a debug-build `clone()` needs about 1.7 KiB
    /// per level). Above the default, run those operations on a thread with
    /// a correspondingly larger stack, or an untrusted file can abort the
    /// process with a stack overflow.
    pub max_depth: usize,
    /// Package only: the most bytes one part may decompress to. Checked
    /// against the declared size before anything is allocated, and enforced
    /// while decompressing ([`Error::PartTooLarge`](crate::Error::PartTooLarge)).
    /// `[Content_Types].xml` and `_rels/.rels` are read at open, so a bomb
    /// there fails the open. Default [`DEFAULT_MAX_PART_BYTES`].
    pub max_part_bytes: u64,
    /// Package only: the most bytes all parts together may declare as their
    /// decompressed size, checked at open
    /// ([`Error::PackageTooLarge`](crate::Error::PackageTooLarge)). The zip
    /// reader refuses an entry that decompresses to more than it declares, so
    /// this bounds the total work of reading every part. Default
    /// [`DEFAULT_MAX_PACKAGE_BYTES`].
    pub max_package_bytes: u64,
    /// Elements whose children are validated now and built on demand.
    pub defer: &'static [QName],
    /// Element names to count during the initial parse, including deferred content.
    pub count: &'static [QName],
}

impl Default for ParseOptions {
    fn default() -> Self {
        ParseOptions {
            max_depth: DEFAULT_MAX_DEPTH,
            max_part_bytes: DEFAULT_MAX_PART_BYTES,
            max_package_bytes: DEFAULT_MAX_PACKAGE_BYTES,
            defer: &[],
            count: &[],
        }
    }
}

impl ParseOptions {
    /// Sets the names of elements whose children are built on demand.
    pub fn with_defer(mut self, defer: &'static [QName]) -> Self {
        self.defer = defer;
        self
    }

    /// Sets the element names to count during parsing.
    pub fn with_count(mut self, count: &'static [QName]) -> Self {
        self.count = count;
        self
    }

    /// Sets [`Self::max_depth`]. See the warning there before raising it.
    pub fn with_max_depth(mut self, max_depth: usize) -> Self {
        self.max_depth = max_depth.min(u32::MAX as usize);
        self
    }

    /// Sets [`Self::max_part_bytes`].
    pub fn with_max_part_bytes(mut self, max_part_bytes: u64) -> Self {
        self.max_part_bytes = max_part_bytes;
        self
    }

    /// Sets [`Self::max_package_bytes`].
    pub fn with_max_package_bytes(mut self, max_package_bytes: u64) -> Self {
        self.max_package_bytes = max_package_bytes;
        self
    }
}

/// A parsed XML part: the original source plus the raw tree.
#[derive(Debug)]
pub struct XmlDocument {
    source: Arc<str>,
    root: RawNode,
    census: Box<[(QName, usize)]>,
    /// The depth limit the document was parsed with.
    max_depth: usize,
    /// Identity of this tree for same-part copies
    /// ([`crate::lens::SamePartDetached`]); a clone gets a new one.
    token: DocToken,
}

impl Clone for XmlDocument {
    /// A copy with a **new** identity: a same-part copy taken from the
    /// original cannot be inserted into the clone (the clone may become
    /// another part, whose relationships differ).
    fn clone(&self) -> Self {
        XmlDocument { source: self.source.clone(), root: self.root.clone(), census: self.census.clone(), max_depth: self.max_depth, token: DocToken::new() }
    }
}

/// Identity of one [`XmlDocument`] (process-unique, never reused).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DocToken(u64);

impl DocToken {
    pub(crate) fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        DocToken(NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
    }
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

    pub(crate) fn parse_inflated(bytes: Vec<u8>, options: &ParseOptions) -> Result<Self> {
        parse::parse_inflated(bytes, options)
    }

    /// Number of elements of this name in the original parse, including
    /// deferred content. Returns `None` for an unrequested name and
    /// `Some(0)` for a requested name that was absent. Edits do not update
    /// this snapshot.
    pub fn census(&self, name: QName) -> Option<usize> {
        self.census.iter().find(|(q, _)| *q == name).map(|(_, count)| *count)
    }

    /// The original source text.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// The root element.
    pub fn root(&self) -> &RawNode {
        &self.root
    }

    /// The root element, mutably (registry primitives in `package`).
    pub(crate) fn root_mut(&mut self) -> &mut RawNode {
        &mut self.root
    }

    /// True if the tree has edits to serialize.
    pub fn is_dirty(&self) -> bool {
        self.root.subtree_dirty()
    }

    /// Serializes per the Layer 2 rules (verbatim spans for untouched
    /// subtrees). With no edits the result equals the input (T1).
    pub fn serialize(&self) -> String {
        write::serialize(self)
    }

    /// A read-only typed lens on the root element.
    pub fn lens<E: ElementSpec>(&self) -> Result<Lens<'_, E>> {
        Lens::root(&self.root).map(|l| l.with_doc(self.token))
    }

    /// A mutable typed lens on the root element. Inserts that would nest
    /// an element deeper than the limit the document was parsed with return
    /// [`Error::TooDeep`](crate::Error::TooDeep), so an edited tree always
    /// parses again with the same options.
    pub fn lens_mut<E: ElementSpec>(&mut self) -> Result<LensMut<'_, E>> {
        LensMut::root_with_limit(&mut self.root, self.max_depth).map(|l| l.with_doc(self.token))
    }

    /// A typed lens on the element at `path` (child indices into
    /// [`RawNode::children`]), which must be `E::QNAME`: [`Lens::at`] that
    /// also knows this document (for [`Lens::detach_same_part_as`]).
    pub fn lens_at<E: ElementSpec>(&self, path: &[usize]) -> Result<Lens<'_, E>> {
        Lens::at(&self.root, path).map(|l| l.with_doc(self.token))
    }

    /// A mutable typed lens on the element at `path`, which must be
    /// `E::QNAME`, with this document's depth limit and identity.
    pub fn lens_mut_at<E: ElementSpec>(&mut self, path: &[usize]) -> Result<LensMut<'_, E>> {
        LensMut::at_with_limit(&mut self.root, path, self.max_depth).map(|l| l.with_doc(self.token))
    }

    pub(crate) fn token(&self) -> DocToken {
        self.token
    }

    /// The element depth limit this document was parsed with (root =
    /// depth 1); inserts beyond it are refused with `TooDeep`.
    pub fn max_depth(&self) -> usize {
        self.max_depth
    }

    /// Removes the root's child item at `index` together with the
    /// whitespace-only text item immediately before it, if any (so an
    /// indented registry part stays tidy). Marks the root `subtree_dirty`;
    /// the root keeps its original tags (ADR-0002 §4.2). Used by
    /// [`Package::remove_part`](crate::Package::remove_part) only.
    pub(crate) fn remove_root_child_with_leading_ws(&mut self, index: usize) {
        self.root.remove_child(index);
        if index > 0 && matches!(&self.root.children()[index - 1], RawItem::Text(t) if t.is_whitespace() && !t.cdata) {
            self.root.remove_child(index - 1);
        }
        self.root.mark_subtree_dirty();
    }

    /// Puts `node` in place of the element at `path` (ADR-0024 C2: a snapshot
    /// restore). `node` is a copy taken earlier with its spans, cache and
    /// dirty state, and it keeps them. Then the ancestors are cleaned from the
    /// parent upward: an ancestor loses both dirty flags only when none of its
    /// child elements is dirty any more (the dirty-descendant count, O(depth))
    /// and `may_clean(depth, ancestor)` agrees (root = depth 0), which is how
    /// the caller says that the ancestor's own tags and child list are as they
    /// were when the copy was taken. If `node` itself is dirty, the ancestors
    /// are marked dirty instead.
    ///
    /// Returns `false` (and changes nothing) when `path` does not name an
    /// element.
    pub fn replace_element_at(&mut self, path: &[usize], node: RawNode, may_clean: impl FnMut(usize, &RawNode) -> bool) -> bool {
        let Some((&index, parent_path)) = path.split_last() else { return false };
        if !self.is_element_path(path) {
            return false;
        }
        let dirty = node.subtree_dirty();
        let parent = node_at_mut(&mut self.root, parent_path);
        parent.splice_children(index..index + 1, [RawItem::Element(node)]);
        self.after_restore(parent_path, dirty, may_clean);
        true
    }

    /// Removes the element at `path`, marks its ancestors dirty (the child
    /// list changed), then cleans them as [`Self::replace_element_at`] does. Returns the removed element, or
    /// `None` (changing nothing) when `path` does not name an element.
    pub fn remove_element_at(&mut self, path: &[usize], may_clean: impl FnMut(usize, &RawNode) -> bool) -> Option<RawNode> {
        let (&index, parent_path) = path.split_last()?;
        if !self.is_element_path(path) {
            return None;
        }
        let parent = node_at_mut(&mut self.root, parent_path);
        let RawItem::Element(removed) = parent.remove_child(index) else { unreachable!("checked above") };
        // the child list changed: dirty unless `may_clean` says it is as in the source
        self.after_restore(parent_path, true, |_, _| false);
        self.after_restore(parent_path, false, may_clean);
        Some(removed)
    }

    /// Inserts `node` (a copy taken earlier) as child item `index` of the
    /// element at `parent_path`. The parent's child list changes, so it and
    /// its ancestors are marked dirty. Returns `false` (changing nothing) for
    /// a bad path or index.
    pub fn insert_element_at(&mut self, parent_path: &[usize], index: usize, node: RawNode) -> bool {
        if !self.is_element_path(parent_path) || index > node_at(&self.root, parent_path).children().len() {
            return false;
        }
        let parent = node_at_mut(&mut self.root, parent_path);
        if parent.span().is_some_and(|s| s.close.is_none()) && parent.children().is_empty() {
            parent.mark_own_dirty(); // a self-closing parent gains its first child
        }
        parent.insert_child(index, RawItem::Element(node));
        mark_path_dirty(&mut self.root, parent_path);
        true
    }

    fn is_element_path(&self, path: &[usize]) -> bool {
        let mut n = &self.root;
        path.iter().all(|&i| match n.children().get(i).and_then(RawItem::as_element) {
            Some(c) => {
                n = c;
                true
            }
            None => false,
        })
    }

    fn after_restore(&mut self, parent_path: &[usize], dirty: bool, mut may_clean: impl FnMut(usize, &RawNode) -> bool) {
        if dirty {
            mark_path_dirty(&mut self.root, parent_path);
        } else {
            clean_up(&mut self.root, parent_path, 0, &mut may_clean);
        }
    }

    /// Every node's dirty-child count equals the number of its dirty child
    /// elements. For tests of code that edits the tree.
    #[doc(hidden)]
    pub fn dirty_counts_consistent(&self) -> bool {
        self.root.dirty_counts_consistent()
    }

    /// Marks every node `own_dirty` + `subtree_dirty` and forgets text spans,
    /// so that [`Self::serialize`] re-renders the whole tree from
    /// names/attributes/children. This is the T1b oracle's set-up
    /// (ADR-0001); not meant for normal editing.
    pub fn force_rerender(&mut self) {
        self.root.force_rerender();
    }
}

#[cfg(test)]
#[test]
fn preserve_attribute_spelling_on_edited_start_tag() {
    use crate::name::QName;
    use crate::schema::{AttrSpec, Content};

    struct Root;
    impl ElementSpec for Root {
        const QNAME: QName = QName::new("", "root");
        const SDK_NAME: &'static str = "root";
        const ATTRS: &'static [AttrSpec] = &[];
        const CONTENT: Content = Content::Empty;
    }

    let input = r#"<root  keep = '&#65;' edit='old'></root >"#;
    let mut doc = XmlDocument::parse(input.as_bytes()).unwrap();
    doc.lens_mut::<Root>().unwrap().preserve_attribute_spelling();
    assert!(!doc.is_dirty());
    assert_eq!(doc.serialize(), input);
    doc.lens_mut::<Root>()
        .unwrap()
        .set_attr(QName::new("", "edit"), "new")
        .unwrap();
    assert_eq!(
        doc.serialize(),
        r#"<root  keep = '&#65;' edit="new"></root >"#
    );
}

#[cfg(test)]
mod deferred_oracle;

#[cfg(test)]
mod restore_tests {
    use super::*;

    const SRC: &str = "<r><a x='1'><b/><c>t</c></a><d/></r>";

    fn doc() -> XmlDocument {
        XmlDocument::parse(SRC.as_bytes()).unwrap()
    }

    fn dirty_a(doc: &mut XmlDocument, path: &[usize]) {
        mark_path_dirty(&mut doc.root, path).mark_own_dirty();
    }

    #[test]
    fn counts_follow_marks_and_child_list_edits() {
        let mut d = doc();
        assert_eq!(d.root().dirty_child_count(), 0);
        dirty_a(&mut d, &[0, 0]);
        dirty_a(&mut d, &[0, 0]); // marking twice counts once
        dirty_a(&mut d, &[0, 1]);
        let a = node_at(d.root(), &[0]);
        assert_eq!(a.dirty_child_count(), 2);
        assert_eq!(d.root().dirty_child_count(), 1);
        assert!(d.dirty_counts_consistent());
        let b = node_at_mut(&mut d.root, &[0]).remove_child(0);
        assert!(b.as_element().unwrap().subtree_dirty());
        assert_eq!(node_at(d.root(), &[0]).dirty_child_count(), 1);
        d.force_rerender();
        assert!(d.dirty_counts_consistent());
        assert_eq!(d.root().dirty_child_count(), 2);
    }

    #[test]
    fn replace_cleans_ancestors_only_when_no_other_descendant_is_dirty() {
        let mut d = doc();
        let (b, c) = (node_at(d.root(), &[0, 0]).clone(), node_at(d.root(), &[0, 1]).clone());
        dirty_a(&mut d, &[0, 0]);
        dirty_a(&mut d, &[0, 1]);
        assert!(d.replace_element_at(&[0, 0], b, |_, _| true));
        assert!(d.root().subtree_dirty() && node_at(d.root(), &[0]).subtree_dirty(), "c is still dirty");
        assert!(d.dirty_counts_consistent());
        assert!(d.replace_element_at(&[0, 1], c, |_, _| true));
        assert!(!d.is_dirty() && !node_at(d.root(), &[0]).own_dirty());
        assert!(d.dirty_counts_consistent());
        assert_eq!(d.serialize(), SRC);
    }

    #[test]
    fn may_clean_can_veto_and_a_dirty_replacement_dirties_the_path() {
        let mut d = doc();
        let b = node_at(d.root(), &[0, 0]).clone();
        dirty_a(&mut d, &[0, 0]);
        assert!(d.replace_element_at(&[0, 0], b.clone(), |depth, _| depth != 1));
        // depth 1 (`a`) refused: it and the root stay dirty
        assert!(node_at(d.root(), &[0]).subtree_dirty() && d.is_dirty());
        assert_eq!(d.root().dirty_child_count(), 1);
        let mut dirty_b = b;
        dirty_b.mark_own_dirty();
        dirty_b.mark_subtree_dirty();
        let mut d2 = doc();
        assert!(d2.replace_element_at(&[0, 0], dirty_b, |_, _| true));
        assert!(d2.is_dirty() && d2.dirty_counts_consistent());
        assert!(!d2.replace_element_at(&[0, 5], node_at(d2.root(), &[1]).clone(), |_, _| true));
    }

    #[test]
    fn remove_and_insert_round_trip() {
        let mut d = doc();
        let before = node_at(d.root(), &[0, 1]).clone();
        let gone = d.remove_element_at(&[0, 1], |_, _| false).unwrap();
        assert_eq!(d.serialize(), "<r><a x='1'><b/></a><d/></r>");
        assert!(gone.structurally_eq(&before));
        assert!(d.insert_element_at(&[0], 1, gone));
        assert!(d.dirty_counts_consistent());
        assert_eq!(d.serialize(), SRC);
        assert!(!d.insert_element_at(&[0], 9, before));
    }
}
