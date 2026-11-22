use std::{borrow::Cow, ops::Range};

use super::{ElementSpan, RawAttr, RawItem};
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
const _: () = assert!(size_of::<RawNode>() == 64);

/// An element of the raw tree (ADR-0001 Layer 2, with names as resolved
/// QName + original prefix per its post-acceptance note).
#[derive(Clone, Debug)]
pub struct RawNode {
    pub(crate) name: XName,
    pub(crate) attrs: Box<[RawAttr]>,
    children: Vec<RawItem>,
    /// Original offsets and dirty flags.
    span: CompactSpan,
}

impl RawNode {
    pub(super) fn parsed(name: XName, attrs: Box<[RawAttr]>, start: usize, open_end: usize) -> Self {
        Self {
            name,
            attrs,
            children: Vec::new(),
            span: CompactSpan {
                start: start as u32,
                open_end: open_end as u32,
                close_start: 0,
                close_end: 0,
            },
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

    pub(crate) fn mark_subtree_dirty(&mut self) {
        self.span.close_end |= FLAG_BIT;
    }

    pub(crate) fn child_mut(&mut self, index: usize) -> Option<&mut RawItem> {
        self.children.get_mut(index)
    }

    pub(crate) fn children_slice_mut(&mut self) -> &mut [RawItem] {
        &mut self.children
    }

    pub(crate) fn insert_child(&mut self, index: usize, item: RawItem) {
        self.span.close_start |= ac_bit(&item);
        self.children.insert(index, item);
    }

    pub(crate) fn remove_child(&mut self, index: usize) -> RawItem {
        self.children.remove(index)
    }

    pub(crate) fn splice_children(&mut self, range: Range<usize>, items: impl IntoIterator<Item = RawItem>) {
        let close_start = &mut self.span.close_start;
        self.children.splice(range, items.into_iter().inspect(|item| *close_start |= ac_bit(item)));
    }

    pub(crate) fn retain_children(&mut self, keep: impl FnMut(&RawItem) -> bool) {
        self.children.retain(keep);
    }

    pub(crate) fn set_children(&mut self, children: Vec<RawItem>) {
        if children.iter().any(|item| ac_bit(item) != 0) {
            self.span.close_start |= FLAG_BIT;
        }
        self.children = children;
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
            children: Vec::new(),
            span: CompactSpan {
                open_end: FLAG_BIT,
                close_end: FLAG_BIT,
                ..CompactSpan::default()
            },
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
        self.children
            .iter()
            .enumerate()
            .filter_map(|(i, c)| c.as_element().map(|n| (i, n)))
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
            && self.children.len() == other.children.len()
            && self.children.iter().zip(&other.children).all(|(a, b)| match (a, b) {
                (RawItem::Element(a), RawItem::Element(b)) => a.structurally_eq(b),
                (RawItem::Text(a), RawItem::Text(b)) => a.value == b.value && a.cdata == b.cdata,
                (RawItem::Markup(a), RawItem::Markup(b)) => a == b,
                _ => false,
            })
    }

    pub(super) fn force_rerender(&mut self) {
        self.mark_own_dirty();
        self.mark_subtree_dirty();
        for c in &mut self.children {
            match c {
                RawItem::Element(n) => n.force_rerender(),
                RawItem::Text(t) => t.forget_span(),
                RawItem::Markup(_) => {}
            }
        }
    }
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
