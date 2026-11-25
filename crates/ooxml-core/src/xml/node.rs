use std::{borrow::Cow, ops::Range, sync::{Arc, OnceLock}};

use super::{ElementSpan, ParseOptions, RawAttr, RawItem, parse};
use crate::name::{MC_ALTERNATE_CONTENT, QName, XML_SPACE, XName};

const FLAG_BIT: u32 = 1 << 31;
pub(super) const MAX_SPAN_OFFSET: u32 = FLAG_BIT - 1;

// A parsed tag cannot end at offset zero. A missing open end means a
// created node, and a missing close end means a self-closing tag.
#[derive(Clone, Copy, Debug, Default)]
struct CompactSpan {
    // The high bit opts into preserving attribute spelling on a dirty tag.
    start: u32,
    // The high bit stores own_dirty.
    open_end: u32,
    // The high bit records that an AlternateContent child may be present.
    close_start: u32,
    // The high bit stores subtree_dirty.
    close_end: u32,
}

impl CompactSpan {
    fn expand(self) -> Option<ElementSpan> {
        let open_end = self.open_end & MAX_SPAN_OFFSET;
        let close_end = self.close_end & MAX_SPAN_OFFSET;
        (open_end != 0).then_some(ElementSpan {
            start: (self.start & MAX_SPAN_OFFSET) as usize,
            open_end: open_end as usize,
            close: (close_end != 0).then_some(((self.close_start & MAX_SPAN_OFFSET) as usize, close_end as usize)),
        })
    }
}

#[cfg(target_pointer_width = "64")]
// 64 bytes of node plus the dirty-child count (ADR-0024 C2).
const _: () = assert!(size_of::<RawNode>() == 72);

pub(super) type Bindings = Arc<[(Option<Range<usize>>, Arc<str>)]>;

#[derive(Clone)]
pub(super) struct Deferred {
    pub(super) source: Arc<str>,
    pub(super) options: ParseOptions,
    pub(super) depth: u32,
    pub(super) bindings: Bindings,
    children: OnceLock<Vec<RawItem>>,
}

impl std::fmt::Debug for Deferred {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Deferred").field("depth", &self.depth).field("children", &self.children).finish_non_exhaustive()
    }
}

impl Deferred {
    pub(super) fn new(source: &Arc<str>, options: ParseOptions, depth: usize, bindings: Bindings) -> Self {
        Self { source: source.clone(), options, depth: depth as u32, bindings, children: OnceLock::new() }
    }

    fn expand(&self, span: ElementSpan) -> Vec<RawItem> {
        if span.close.is_none_or(|(start, _)| start == span.open_end) {
            return Vec::new();
        }
        parse::expand(self, span)
    }
}

// The Vec's niche keeps both variants within the original child-list size.
#[derive(Clone, Debug)]
enum Children {
    Built(Vec<RawItem>),
    Deferred(Box<Deferred>),
}

/// An element of the raw tree (ADR-0001 Layer 2, with names as resolved
/// QName + original prefix per its post-acceptance note).
#[derive(Clone, Debug)]
pub struct RawNode {
    pub(crate) name: XName,
    pub(crate) attrs: Box<[RawAttr]>,
    children: Children,
    /// Original offsets and dirty flags.
    span: CompactSpan,
    /// How many child elements are `subtree_dirty`. Maintained by every
    /// operation that changes the child list or marks a child dirty, so a
    /// caller can ask "is anything below this node still dirty" without a
    /// walk (ADR-0024 C2).
    dirty_children: u32,
}

impl RawNode {
    pub(super) fn parsed(name: XName, attrs: Box<[RawAttr]>, start: usize, open_end: usize) -> Self {
        Self {
            name,
            attrs,
            children: Children::Built(Vec::new()),
            span: CompactSpan {
                start: start as u32,
                open_end: open_end as u32,
                close_start: 0,
                close_end: 0,
            },
            dirty_children: 0,
        }
    }

    pub(super) fn close(&mut self, start: usize, end: usize) {
        self.span.close_start = (self.span.close_start & FLAG_BIT) | start as u32;
        self.span.close_end = end as u32;
    }

    pub(crate) fn preserve_attribute_spelling(&mut self) {
        self.span.start |= FLAG_BIT;
    }

    pub(super) fn preserves_attribute_spelling(&self) -> bool {
        self.span.start & FLAG_BIT != 0
    }

    pub(crate) fn mark_own_dirty(&mut self) {
        self.span.open_end |= FLAG_BIT;
    }

    /// Sets `subtree_dirty` on a node that has no parent in the tree (the
    /// root, or a detached copy). A node inside a tree is marked through
    /// [`Self::mark_child_dirty`] on its parent, which keeps the count.
    pub(crate) fn mark_subtree_dirty(&mut self) {
        self.span.close_end |= FLAG_BIT;
    }

    /// Sets `subtree_dirty` on the child element at `index` and counts it.
    /// Returns the child.
    pub(crate) fn mark_child_dirty(&mut self, index: usize) -> Option<&mut RawNode> {
        let newly = {
            let child = self.child_mut(index).and_then(RawItem::as_element_mut)?;
            let newly = !child.subtree_dirty();
            child.mark_subtree_dirty();
            newly
        };
        self.dirty_children += u32::from(newly);
        self.child_mut(index).and_then(RawItem::as_element_mut)
    }

    /// How many child elements are `subtree_dirty`. Zero means nothing below
    /// this node is dirty (a dirty descendant makes its ancestors dirty).
    pub fn dirty_child_count(&self) -> usize {
        self.dirty_children as usize
    }

    pub(crate) fn sub_dirty_child(&mut self) {
        self.dirty_children -= 1;
    }

    /// Clears both dirty flags of this node. The caller has checked that
    /// nothing below it is dirty and that its own tags and child list are as
    /// in the source; the parent's count is the caller's to fix.
    pub(crate) fn clear_dirty(&mut self) {
        self.span.open_end &= MAX_SPAN_OFFSET;
        self.span.close_end &= MAX_SPAN_OFFSET;
    }

    /// Does every node of this subtree count its dirty children right? (For
    /// tests; a walk of the subtree.)
    #[doc(hidden)]
    pub fn dirty_counts_consistent(&self) -> bool {
        // Unexpanded deferred children are clean source and stay unexpanded.
        let children: &[RawItem] = match &self.children {
            Children::Built(children) => children,
            Children::Deferred(deferred) => deferred.children.get().map_or(&[], Vec::as_slice),
        };
        let dirty = children.iter().filter_map(RawItem::as_element).filter(|c| c.subtree_dirty()).count();
        dirty == self.dirty_children as usize
            && children.iter().filter_map(RawItem::as_element).all(RawNode::dirty_counts_consistent)
    }

    pub(super) fn defer(&mut self, deferred: Deferred) {
        self.children = Children::Deferred(Box::new(deferred));
    }

    pub(super) fn mark_alternate_content(&mut self) {
        self.span.close_start |= FLAG_BIT;
    }

    fn children_mut(&mut self) -> &mut Vec<RawItem> {
        match &mut self.children {
            Children::Deferred(deferred) => {
                if deferred.children.get().is_none() {
                    let children = deferred.expand(self.span.expand().expect("deferred node has a source span"));
                    deferred.children = OnceLock::from(children);
                }
                deferred.children.get_mut().expect("expanded above")
            }
            Children::Built(children) => children,
        }
    }

    // A structural edit makes the original child list unsuitable for release,
    // even if every remaining child still has a clean span.
    fn discard_deferral(&mut self) {
        if matches!(self.children, Children::Deferred(_)) {
            self.children = Children::Built(std::mem::take(self.children_mut()));
        }
    }

    fn edited_children(&mut self) -> &mut Vec<RawItem> {
        self.discard_deferral();
        self.children_mut()
    }

    pub(crate) fn child_mut(&mut self, index: usize) -> Option<&mut RawItem> {
        self.children_mut().get_mut(index)
    }

    pub(crate) fn children_slice_mut(&mut self) -> &mut [RawItem] {
        self.children_mut()
    }

    pub(crate) fn insert_child(&mut self, index: usize, item: RawItem) {
        self.span.close_start |= ac_bit(&item);
        self.dirty_children += dirty_item(&item);
        self.edited_children().insert(index, item);
    }

    pub(crate) fn remove_child(&mut self, index: usize) -> RawItem {
        let item = self.edited_children().remove(index);
        self.dirty_children -= dirty_item(&item);
        item
    }

    pub(crate) fn splice_children(&mut self, range: Range<usize>, items: impl IntoIterator<Item = RawItem>) {
        self.discard_deferral();
        let Children::Built(children) = &mut self.children else { unreachable!() };
        let close_start = &mut self.span.close_start;
        let mut dirty = self.dirty_children;
        dirty -= children[range.clone()].iter().map(dirty_item).sum::<u32>();
        children.splice(
            range,
            items.into_iter().inspect(|item| {
                *close_start |= ac_bit(item);
                dirty += dirty_item(item);
            }),
        );
        self.dirty_children = dirty;
    }

    pub(crate) fn retain_children(&mut self, mut keep: impl FnMut(&RawItem) -> bool) {
        let mut removed = 0;
        self.edited_children().retain(|item| {
            let kept = keep(item);
            if !kept {
                removed += dirty_item(item);
            }
            kept
        });
        self.dirty_children -= removed;
    }

    pub(crate) fn set_children(&mut self, children: Vec<RawItem>) {
        self.discard_deferral();
        self.finish_parsed_children(children);
    }

    pub(super) fn finish_parsed_children(&mut self, children: Vec<RawItem>) {
        debug_assert!(matches!(self.children, Children::Built(_)));
        if children.iter().any(|item| ac_bit(item) != 0) {
            self.mark_alternate_content();
        }
        self.dirty_children = children.iter().map(dirty_item).sum();
        self.children = Children::Built(children);
    }

    pub(super) fn into_parsed_children(self) -> Vec<RawItem> {
        let Children::Built(children) = self.children else { unreachable!("expansion builds its root") };
        children
    }

    pub(crate) fn may_have_alternate_content(&self) -> bool {
        self.span.close_start & FLAG_BIT != 0
    }

    pub(crate) fn push_attr(&mut self, attr: RawAttr) {
        let mut attrs = std::mem::take(&mut self.attrs).into_vec();
        attrs.push(attr);
        self.attrs = attrs.into_boxed_slice();
    }

    pub(crate) fn remove_attr(&mut self, index: usize) {
        let mut attrs = std::mem::take(&mut self.attrs).into_vec();
        attrs.remove(index);
        self.attrs = attrs.into_boxed_slice();
    }

    pub(crate) fn new_created(name: XName) -> Self {
        Self {
            name,
            attrs: Box::default(),
            children: Children::Built(Vec::new()),
            span: CompactSpan {
                open_end: FLAG_BIT,
                close_end: FLAG_BIT,
                ..CompactSpan::default()
            },
            dirty_children: 0,
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
        match &self.children {
            Children::Deferred(deferred) => deferred.children.get_or_init(|| {
                deferred.expand(self.span().expect("deferred node has a source span"))
            }),
            Children::Built(children) => children,
        }
    }

    /// Whether this node's children are still waiting to be built from the source.
    pub fn is_deferred(&self) -> bool {
        matches!(&self.children, Children::Deferred(d) if d.children.get().is_none())
    }

    /// Original byte range of the content between this node's tags. A
    /// self-closing tag has an empty range. Created nodes have no range.
    /// The range always refers to the original source, so it may describe
    /// stale content after edits to the child list or its descendants.
    pub fn content_span(&self) -> Option<Range<usize>> {
        let span = self.span()?;
        Some(span.open_end..span.close.map_or(span.open_end, |(start, _)| start))
    }

    /// Drops an expanded child list that can still be rebuilt from the
    /// source. Attribute edits on this element do not prevent release.
    /// Descendant edits and attribute-spelling preferences prevent release.
    /// Returns whether a list was dropped.
    pub fn release_children(&mut self) -> bool {
        let Children::Deferred(deferred) = &mut self.children else { return false };
        if deferred.children.get().is_some_and(|children| {
            children.iter().all(|item| item.as_element().is_none_or(|n| !n.subtree_dirty() && !n.has_spelling_preference()))
        }) {
            return deferred.children.take().is_some();
        }
        false
    }

    fn has_spelling_preference(&self) -> bool {
        if self.preserves_attribute_spelling() {
            return true;
        }
        let children = match &self.children {
            Children::Built(children) => children,
            Children::Deferred(deferred) => {
                let Some(children) = deferred.children.get() else { return false };
                children
            }
        };
        children.iter().filter_map(RawItem::as_element).any(Self::has_spelling_preference)
    }

    pub(super) fn deferred_content<'a>(&'a self, source: &str) -> Option<&'a str> {
        let Children::Deferred(deferred) = &self.children else { return None };
        if deferred.children.get().is_some() {
            return None;
        }
        debug_assert!(std::ptr::eq(deferred.source.as_ref(), source));
        Some(&deferred.source[self.content_span().expect("deferred node has a source span")])
    }

    /// Child elements in document order.
    pub fn elements(&self) -> impl Iterator<Item = &RawNode> {
        self.children().iter().filter_map(RawItem::as_element)
    }
    /// Child elements with their item index (the index used in node paths).
    pub fn indexed_elements(&self) -> impl Iterator<Item = (usize, &RawNode)> {
        self.children()
            .iter()
            .enumerate()
            .filter_map(|(i, c)| c.as_element().map(|n| (i, n)))
    }
    /// The concatenated text children (no descendants).
    pub fn text(&self) -> Cow<'_, str> {
        let mut texts = self.children().iter().filter_map(|c| match c {
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
        self.span.expand()
    }
    /// This element's own name/attributes changed, or it was created, or it
    /// was self-closing and gained a child.
    pub fn own_dirty(&self) -> bool {
        self.span.open_end & FLAG_BIT != 0
    }
    /// This element or a descendant changed.
    pub fn subtree_dirty(&self) -> bool {
        self.span.close_end & FLAG_BIT != 0
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
            && self.children().len() == other.children().len()
            && self.children().iter().zip(other.children()).all(|(a, b)| match (a, b) {
                (RawItem::Element(a), RawItem::Element(b)) => a.structurally_eq(b),
                (RawItem::Text(a), RawItem::Text(b)) => a.value == b.value && a.cdata == b.cdata,
                (RawItem::Markup(a), RawItem::Markup(b)) => a == b,
                _ => false,
            })
    }

    pub(super) fn force_rerender(&mut self) {
        self.mark_own_dirty();
        self.mark_subtree_dirty();
        let mut elements = 0;
        for c in self.edited_children() {
            match c {
                RawItem::Element(n) => {
                    n.force_rerender();
                    elements += 1;
                }
                RawItem::Text(t) => t.forget_span(),
                RawItem::Markup(_) => {}
            }
        }
        self.dirty_children = elements;
    }
}

fn dirty_item(item: &RawItem) -> u32 {
    u32::from(item.as_element().is_some_and(RawNode::subtree_dirty))
}

fn ac_bit(item: &RawItem) -> u32 {
    if item.as_element().is_some_and(|node| node.is(MC_ALTERNATE_CONTENT)) {
        FLAG_BIT
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::XmlDocument;

    #[test]
    fn deferred_alternate_content_flag_tracks_only_direct_children() {
        let source = format!(
            "<root xmlns:m='{}'><row><m:AlternateContent/></row><row><x><m:AlternateContent/></x></row></root>",
            MC_ALTERNATE_CONTENT.ns
        );
        const DEFER: &[QName] = &[QName::new("", "row"), QName::new("", "root")];
        let options = ParseOptions::default().with_defer(DEFER);
        let doc = XmlDocument::parse_with(source.as_bytes(), &options).unwrap();
        assert!(doc.root().is_deferred());
        assert!(!doc.root().may_have_alternate_content());
        for (row, expected) in doc.root().elements().zip([true, false]) {
            assert!(row.is_deferred());
            assert_eq!(row.may_have_alternate_content(), expected);
            let range = row.content_span().unwrap();
            assert!(source[range].contains("<m:AlternateContent/>"));
            let mut row = row.clone();
            row.children();
            assert!(row.release_children());
            assert_eq!(row.may_have_alternate_content(), expected);
        }
    }

    #[test]
    fn alternate_content_flag_survives_child_edits_and_close() {
        let source = format!(
            "<root xmlns:mc='{}'><mc:AlternateContent/></root>",
            MC_ALTERNATE_CONTENT.ns
        );
        let doc = XmlDocument::parse(source.as_bytes()).unwrap();
        let root = doc.root();
        assert!(root.may_have_alternate_content());
        let ac = root.children()[0].clone();
        assert!(!ac.as_element().unwrap().may_have_alternate_content());
        assert_eq!(doc.serialize(), source);

        let fresh = || RawNode::parsed(root.name.clone(), Box::default(), 0, 6);
        let mut parent = fresh();
        let ordinary = RawItem::Element(fresh());
        parent.insert_child(0, ordinary.clone());
        parent.splice_children(0..1, [ordinary.clone()]);
        parent.set_children(vec![ordinary]);
        assert!(!parent.may_have_alternate_content());

        let add: [fn(&mut RawNode, RawItem); 3] = [
            |parent, ac| parent.insert_child(0, ac),
            |parent, ac| parent.splice_children(0..0, [ac]),
            |parent, ac| parent.set_children(vec![ac]),
        ];
        for add_child in add {
            let mut parent = fresh();
            add_child(&mut parent, ac.clone());
            assert!(parent.may_have_alternate_content());
            parent.close(27, 34);
            assert!(parent.may_have_alternate_content());
            assert_eq!(parent.span().unwrap().close, Some((27, 34)));
            parent.remove_child(0);
            assert!(parent.may_have_alternate_content());
        }
    }
}
