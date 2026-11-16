//! Typed lenses over the raw tree and the write primitives of ADR-0002 §4.2.
//!
//! - [`Lens<'a, E>`] — read-only view of one element whose schema is `E`.
//! - [`LensMut<'a, E>`] — mutable view: the part root by `&mut` plus the
//!   node's **path of child indices** (node id, ADR-0002 §4.2). Child lenses
//!   are reborrows, so only one exists at a time; inserting or removing a
//!   child invalidates later siblings' indices, which is why primitives
//!   return the new child's lens.
//!
//! All dirty-flag duties live here, never in generated code:
//!
//! - every insert, remove, attribute write and `set_text` sets
//!   `subtree_dirty` on the changed node and every ancestor up to the root;
//! - an attribute write sets `own_dirty` on that node;
//! - a parent gets `own_dirty` from a child change only when its original
//!   span was self-closing and it gains its first child (element or text);
//! - a parent that had a close tag keeps its original tags verbatim even
//!   when it loses its last child.

use std::borrow::Cow;
use std::marker::PhantomData;

use std::sync::Arc;

mod move_subtree;

pub use move_subtree::MoveSource;

use crate::error::{DetachRefusal, Error, Result};
use crate::name::{MC_ALTERNATE_CONTENT, MC_NS, QName, RELATIONSHIP_ATTRIBUTES, XML_NS, XML_SPACE, XMLNS_NS, XName};
use crate::schema::{ChildOf, Content, Detachable, ElementSpec, EnumType, EnumValue, AttrError, SameType, Slot, UniqueAttr};
use crate::value::{first_invalid_xml_char, has_edge_ws};

/// Refuses a value that XML 1.0 cannot represent (ADR-0002 post-acceptance
/// note, "XML characters").
fn check_xml_chars(value: &str) -> Result<()> {
    match first_invalid_xml_char(value) {
        None => Ok(()),
        Some((offset, ch)) => {
            core::hint::cold_path();
            Err(Error::InvalidXmlChar { ch, offset })
        }
    }
}
use crate::xml::{RawAttr, RawItem, RawNode, RawText, node_at, node_at_mut};

fn check_root<E: ElementSpec>(root: &RawNode) -> Result<()> {
    if root.is(E::QNAME) {
        Ok(())
    } else {
        Err(Error::WrongElement {
            expected: E::QNAME,
            found_ns: root.name.ns().into(),
            found_local: root.name.local().into(),
        })
    }
}

/// Read-only typed view of one element.
pub struct Lens<'a, E: ElementSpec> {
    root: &'a RawNode,
    node: &'a RawNode,
    path: Vec<usize>,
    _e: PhantomData<fn() -> E>,
}

impl<E: ElementSpec> Clone for Lens<'_, E> {
    fn clone(&self) -> Self {
        Lens { root: self.root, node: self.node, path: self.path.clone(), _e: PhantomData }
    }
}

impl<E: ElementSpec> std::fmt::Debug for Lens<'_, E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Lens<{}>{:?}", E::SDK_NAME, self.path)
    }
}

impl<'a, E: ElementSpec> Lens<'a, E> {
    /// A lens on `root`, which must be the element `E::QNAME`.
    pub fn root(root: &'a RawNode) -> Result<Self> {
        check_root::<E>(root)?;
        Ok(Lens { root, node: root, path: Vec::new(), _e: PhantomData })
    }

    /// The raw node.
    pub fn raw(&self) -> &'a RawNode {
        self.node
    }

    /// Path of child indices from the part root.
    pub fn path(&self) -> &[usize] {
        &self.path
    }

    /// Raw attribute value, without defaults.
    pub fn attr(&self, q: QName) -> Option<&'a str> {
        self.node.attr(q)
    }

    /// Raw attribute value, falling back to the schema default (ADR-0002 §1.2).
    pub fn attr_or_default(&self, q: QName) -> Option<&'a str> {
        self.node.attr(q).or_else(|| E::attr_spec(q).and_then(|s| s.default))
    }

    /// Typed read of an attribute without a default.
    pub fn attr_parsed<T>(&self, q: QName, parse: impl FnOnce(&'a str) -> Option<T>) -> Result<Option<T>, AttrError> {
        match self.node.attr(q) {
            None => Ok(None),
            Some(raw) => parse(raw).map(Some).ok_or_else(|| AttrError { attr: q, raw: raw.into() }),
        }
    }

    /// Typed read of an attribute, using `default` when absent.
    pub fn attr_parsed_or<T>(&self, q: QName, default: &'static str, parse: impl FnOnce(&'a str) -> Option<T>) -> Result<T, AttrError> {
        let raw = self.node.attr(q).unwrap_or(default);
        parse(raw).ok_or_else(|| AttrError { attr: q, raw: raw.into() })
    }

    /// Enum read: `Known` or `Unknown(raw)`, never an error.
    pub fn attr_enum<V: EnumType>(&self, q: QName) -> Option<EnumValue<'a, V>> {
        self.node.attr(q).map(EnumValue::from_raw)
    }

    /// Enum read with a default for an absent attribute.
    pub fn attr_enum_or<V: EnumType>(&self, q: QName, default: &'static str) -> EnumValue<'a, V> {
        EnumValue::from_raw(self.node.attr(q).unwrap_or(default))
    }

    /// The element's text (concatenated text children).
    pub fn text(&self) -> Cow<'a, str> {
        self.node.text()
    }

    /// Is `xml:space="preserve"` in effect here (inherited from ancestors)?
    pub fn xml_space_preserve(&self) -> bool {
        let mut result = self.root.xml_space_preserve().unwrap_or(false);
        let mut n = self.root;
        for &i in &self.path {
            n = n.children[i].as_element().expect("path points at an element");
            if let Some(p) = n.xml_space_preserve() {
                result = p;
            }
        }
        result
    }

    fn child_lens<C: ElementSpec>(&self, index: usize, node: &'a RawNode) -> Lens<'a, C> {
        let mut path = self.path.clone();
        path.push(index);
        Lens { root: self.root, node, path, _e: PhantomData }
    }

    /// Child elements named `C::QNAME`, in document order.
    pub fn children<C: ChildOf<E>>(&self) -> impl Iterator<Item = Lens<'a, C>> + use<'_, 'a, E, C> {
        self.node
            .indexed_elements()
            .filter(|(_, n)| n.is(C::QNAME))
            .map(|(i, n)| self.child_lens::<C>(i, n))
    }

    /// First child element named `C::QNAME`.
    pub fn child<C: ChildOf<E>>(&self) -> Option<Lens<'a, C>> {
        self.children::<C>().next()
    }

    /// Reinterprets this lens as another spec with the same QName (e.g. when
    /// the caller knows the context-specific type). Only the QName is
    /// checked; that `F` is the right complex type for this node is the
    /// caller's responsibility (it matters for [`Self::detach_as`]).
    pub fn cast<F: ElementSpec>(self) -> Result<Lens<'a, F>> {
        if F::QNAME != E::QNAME {
            return Err(Error::WrongElement {
                expected: F::QNAME,
                found_ns: E::QNAME.ns.into(),
                found_local: E::QNAME.local.into(),
            });
        }
        Ok(Lens { root: self.root, node: self.node, path: self.path, _e: PhantomData })
    }

    /// An owned, span-less copy of this element's subtree, renamed to
    /// `C::QNAME`, for [`LensMut::insert_detached`] (ADR-0002
    /// post-acceptance note "detach/insert"). Only [`Detachable`] elements
    /// (opted in per allowlist, never id-bearing subtrees) can be detached.
    /// Element and attribute names keep their namespace URIs; prefixes are
    /// chosen again at the destination. Direct children (only) whose name
    /// is in `skip` are left out with their whole subtree.
    ///
    /// Refused, before anything is copied ([`Error::NotDetachable`] with a
    /// [`DetachRefusal`] and the offending name), when the kept subtree has:
    /// a Markup Compatibility element or attribute (`mc:AlternateContent`,
    /// `mc:Ignorable`, …); an attribute holding a relationship id
    /// ([`crate::name::RELATIONSHIP_ATTRIBUTES`]); a namespace declaration;
    /// an element or attribute in a namespace that an **ancestor's**
    /// `mc:Ignorable` lists; or text while `xml:space="preserve"` is
    /// inherited from an ancestor (the lens must then know its ancestors:
    /// use a lens from [`crate::XmlDocument::lens`] or [`Lens::at`], not
    /// [`Lens::root`] on an inner node).
    pub fn detach_as<C>(&self, skip: &[QName]) -> Result<Detached<C>>
    where
        E: Detachable,
        C: SameType<E> + Detachable,
    {
        let ctx = ancestor_context(self.root, &self.path[..self.path.len().saturating_sub(1)], !self.path.is_empty());
        let text_inherits_space = self.node.xml_space_preserve().is_none() && has_text(self.node);
        if ctx.preserve && text_inherits_space {
            return Err(not_detachable(DetachRefusal::InheritedXmlSpace, &self.node.name));
        }
        let unique: Vec<UniqueAttr> = E::UNIQUE_ATTRS.iter().chain(C::UNIQUE_ATTRS).copied().collect();
        let mut root = detach_node(self.node, skip, &ctx.ignorable, &unique)?;
        root.name = XName::new(C::QNAME.ns.into(), C::QNAME.local, None);
        Ok(Detached { root, text_inherits_space, _c: PhantomData })
    }
}

/// What the elements on `path` from `root` (and `root` itself when
/// `include_root`; an empty path with `include_root = false` means "no
/// ancestors") contribute to a node below them: the namespaces their
/// `mc:Ignorable` lists, each resolved with the declarations in effect on
/// that element (innermost first), and the `xml:space` in effect. The path
/// must point at elements (callers validate it).
fn ancestor_context(root: &RawNode, path: &[usize], include_root: bool) -> AncestorContext {
    let mut decls: Vec<(Option<&str>, &str)> = Vec::new();
    let mut ctx = AncestorContext { ignorable: Vec::new(), preserve: false };
    if !include_root {
        return ctx;
    }
    let mut n = root;
    let mut rest = path.iter();
    loop {
        decls.extend(n.attrs.iter().filter(|a| a.name.ns() == XMLNS_NS).map(|a| (a.name.declared_prefix(), a.value.as_str())));
        if let Some(v) = n.attr(MC_IGNORABLE) {
            for p in v.split_ascii_whitespace() {
                if let Some((_, u)) = decls.iter().rev().find(|(q, _)| *q == Some(p)) {
                    ctx.ignorable.push((*u).to_owned());
                }
            }
        }
        if let Some(p) = n.xml_space_preserve() {
            ctx.preserve = p;
        }
        let Some(&i) = rest.next() else { break };
        n = n.children[i].as_element().expect("validated path");
    }
    ctx
}

struct AncestorContext {
    ignorable: Vec<String>,
    preserve: bool,
}

/// The element at `path`, or `Err(WrongElement)` (no panic) if the path
/// leaves the tree or ends on a non-element item.
fn element_at<'r>(root: &'r RawNode, path: &[usize], expected: QName) -> Result<&'r RawNode> {
    let mut node = root;
    for &i in path {
        node = node.children.get(i).and_then(RawItem::as_element).ok_or_else(|| Error::WrongElement {
            expected,
            found_ns: String::new(),
            found_local: "(no element at this path)".into(),
        })?;
    }
    Ok(node)
}

/// Destination conditions of an insertion under the element at
/// `parent_path` (validated): namespaces made ignorable by the parent or
/// its ancestors, and inherited `xml:space`, must not change the copy's
/// meaning (the source side is checked by `detach_as`).
fn check_destination(copy: &RawNode, text_inherits_space: bool, root: &RawNode, parent_path: &[usize]) -> Result<()> {
    let ctx = ancestor_context(root, parent_path, true);
    if ctx.preserve && text_inherits_space {
        return Err(not_detachable(DetachRefusal::InheritedXmlSpace, &copy.name));
    }
    fn uses<'n>(n: &'n RawNode, ignorable: &[String]) -> Option<&'n XName> {
        if ignorable.iter().any(|u| u == n.name.ns()) {
            return Some(&n.name);
        }
        if let Some(a) = n.attrs.iter().find(|a| ignorable.iter().any(|u| u == a.name.ns())) {
            return Some(&a.name);
        }
        n.elements().find_map(|c| uses(c, ignorable))
    }
    match uses(copy, &ctx.ignorable) {
        Some(name) => Err(not_detachable(DetachRefusal::IgnorableNamespace, name)),
        None => Ok(()),
    }
}

const MC_IGNORABLE: QName = QName::new(MC_NS, "Ignorable");

/// Number of element levels of a subtree (1 for an element without element
/// children). Shared by `Detached::height` and the same-part move.
fn subtree_height(n: &RawNode) -> usize {
    1 + n.elements().map(subtree_height).max().unwrap_or(0)
}

fn has_text(n: &RawNode) -> bool {
    n.children.iter().any(|c| match c {
        RawItem::Text(t) => !t.value.is_empty(),
        RawItem::Element(e) => e.xml_space_preserve().is_none() && has_text(e),
        RawItem::Markup(_) => false,
    })
}

impl<'a, E: ElementSpec> Lens<'a, E> {
    /// A lens on the element at `path` (child indices into
    /// [`RawNode::children`]) below `root`, which must be `E::QNAME`.
    /// Unlike [`Self::root`] on the inner node, the lens knows the node's
    /// ancestors (namespace declarations, `mc:Ignorable`, `xml:space`).
    pub fn at(root: &'a RawNode, path: &[usize]) -> Result<Self> {
        let node = element_at(root, path, E::QNAME)?;
        check_root::<E>(node)?;
        Ok(Lens { root, node, path: path.to_vec(), _e: PhantomData })
    }
}

/// An owned copy of an element subtree taken by [`Lens::detach_as`]: no
/// source spans, no prefixes (names carry namespace URIs only), typed as
/// `C`. Insert it, as often as needed, with [`LensMut::insert_detached`] /
/// [`LensMut::insert_detached_by`]; [`Self::check_insert_under`] tells in
/// advance, without changing anything, whether the prefixes will resolve.
pub struct Detached<C: ElementSpec> {
    root: RawNode,
    /// The copy has text whose whitespace handling comes from inherited
    /// `xml:space` (default at the source): the destination must not be
    /// `preserve`.
    text_inherits_space: bool,
    _c: PhantomData<fn() -> C>,
}

impl<C: ElementSpec> Clone for Detached<C> {
    fn clone(&self) -> Self {
        Detached { root: self.root.clone(), text_inherits_space: self.text_inherits_space, _c: PhantomData }
    }
}

impl<C: ElementSpec> std::fmt::Debug for Detached<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Detached<{}>", C::SDK_NAME)
    }
}

impl<C: ElementSpec> Detached<C> {
    /// The copied subtree (prefix-less names, no spans).
    pub fn raw(&self) -> &RawNode {
        &self.root
    }

    /// Number of element levels of the copy (1 for an element without
    /// element children).
    pub fn height(&self) -> usize {
        subtree_height(&self.root)
    }

    /// Would [`LensMut::insert_detached`] succeed, as far as depth,
    /// namespaces and the destination's `mc:Ignorable` / `xml:space` go,
    /// when the copy is inserted below the element at `parent_path` of
    /// `doc`, with `created` new elements in between (0 = directly under
    /// it; created elements declare nothing)? Changes nothing. Returns the
    /// error the insertion would return for those reasons (`TooDeep`,
    /// `NamespaceNotInScope`, `NotDetachable`), or `WrongElement` for a path
    /// that does not lead to an element; slot rules are not checked. It
    /// agrees with inserts through [`crate::XmlDocument::lens_mut`] (which
    /// carries `doc`'s depth limit; `LensMut::root` uses the default).
    /// A format crate calls it for every copy of a multi-insert edit before
    /// the edit's first change.
    pub fn check_insert_under(&self, doc: &crate::XmlDocument, parent_path: &[usize], created: usize) -> Result<()> {
        let root = doc.root();
        // (the path names the parent, whose type the copy does not know)
        let parent = element_at(root, parent_path, QName::new("", "(parent element at parent_path)"))?;
        // parent depth = path len + 1; the copy's deepest element
        // (saturating: a huge `created` must refuse, not wrap):
        if parent_path.len().saturating_add(1).saturating_add(created).saturating_add(self.height()) > doc.max_depth() {
            return Err(Error::TooDeep { limit: doc.max_depth(), position: parent.span.map_or(0, |s| s.start) });
        }
        check_destination(&self.root, self.text_inherits_space, root, parent_path)?;
        let decls = declarations(root, parent_path);
        with_prefixes(self.root.clone(), &decls, parent.name.prefix()).map(|_| ())
    }
}

fn not_detachable(reason: DetachRefusal, name: &XName) -> Error {
    core::hint::cold_path();
    let name = if name.ns().is_empty() { name.local().to_owned() } else { format!("{{{}}}{}", name.ns(), name.local()) };
    Error::NotDetachable { reason, name }
}

fn holds_relationship_id(name: &XName) -> bool {
    RELATIONSHIP_ATTRIBUTES.iter().any(|&(ns, local)| name.ns() == ns && local.is_none_or(|l| name.local() == l))
}

/// Copies `n` (whose direct children named in `skip` are left out) with
/// prefixes and spans cleared, refusing what [`Lens::detach_as`] refuses.
fn detach_node(n: &RawNode, skip: &[QName], ignorable: &[String], unique: &[UniqueAttr]) -> Result<RawNode> {
    if n.name.ns() == MC_NS {
        let reason = if n.is(MC_ALTERNATE_CONTENT) { DetachRefusal::AlternateContent } else { DetachRefusal::MarkupCompatibility };
        return Err(not_detachable(reason, &n.name));
    }
    if n.name.local() == "extLst" {
        return Err(not_detachable(DetachRefusal::ExtensionList, &n.name));
    }
    for u in unique.iter().filter(|u| n.is(u.element)) {
        if let Some(v) = n.attr(u.attr)
            && Some(v) != u.default
        {
            let a = n.attrs.iter().find(|a| a.name.is(u.attr)).expect("present");
            return Err(not_detachable(DetachRefusal::UniqueAttribute, &a.name));
        }
    }
    if ignorable.iter().any(|u| u == n.name.ns()) {
        return Err(not_detachable(DetachRefusal::IgnorableNamespace, &n.name));
    }
    let mut attrs = Vec::with_capacity(n.attrs.len());
    for a in &n.attrs {
        let reason = match a.name.ns() {
            XMLNS_NS => Some(DetachRefusal::NamespaceDeclaration),
            MC_NS => Some(DetachRefusal::MarkupCompatibility),
            _ if holds_relationship_id(&a.name) => Some(DetachRefusal::RelationshipAttribute),
            u if ignorable.iter().any(|i| i == u) => Some(DetachRefusal::IgnorableNamespace),
            _ => None,
        };
        if let Some(reason) = reason {
            return Err(not_detachable(reason, &a.name));
        }
        attrs.push(RawAttr { name: XName::new(a.name.ns().into(), a.name.local(), None), value: a.value.clone() });
    }
    let mut children = Vec::with_capacity(n.children.len());
    for c in &n.children {
        children.push(match c {
            RawItem::Element(e) if skip.iter().any(|q| e.is(*q)) => continue,
            RawItem::Element(e) => RawItem::Element(detach_node(e, &[], ignorable, unique)?),
            RawItem::Text(t) => RawItem::Text(RawText { value: t.value.clone(), span: None, cdata: t.cdata }),
            RawItem::Markup(m) => RawItem::Markup(m.clone()),
        });
    }
    let mut out = RawNode::new_created(XName::new(n.name.ns().into(), n.name.local(), None));
    out.attrs = attrs;
    out.children = children;
    Ok(out)
}

/// Mutable typed view: the part root plus a path of child indices.
pub struct LensMut<'a, E: ElementSpec> {
    root: &'a mut RawNode,
    path: Vec<usize>,
    /// Depth limit for inserted elements (the root is depth 1).
    max_depth: usize,
    _e: PhantomData<fn() -> E>,
}

impl<E: ElementSpec> std::fmt::Debug for LensMut<'_, E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "LensMut<{}>{:?}", E::SDK_NAME, self.path)
    }
}

/// Where a child goes, computed before mutating.
struct Placement {
    index: usize,
}

impl<'a, E: ElementSpec> LensMut<'a, E> {
    /// A mutable lens on `root`, which must be the element `E::QNAME`.
    /// Inserts are limited to [`DEFAULT_MAX_DEPTH`](crate::xml::DEFAULT_MAX_DEPTH);
    /// [`XmlDocument::lens_mut`](crate::XmlDocument::lens_mut) uses the
    /// limit the document was parsed with instead.
    pub fn root(root: &'a mut RawNode) -> Result<Self> {
        Self::root_with_limit(root, crate::xml::DEFAULT_MAX_DEPTH)
    }

    pub(crate) fn root_with_limit(root: &'a mut RawNode, max_depth: usize) -> Result<Self> {
        check_root::<E>(root)?;
        Ok(LensMut { root, path: Vec::new(), max_depth, _e: PhantomData })
    }

    /// The raw node (read-only; mutation goes through the primitives).
    pub fn node(&self) -> &RawNode {
        node_at(self.root, &self.path)
    }

    /// Path of child indices from the part root. The last index is this
    /// node's position in its parent's [`RawNode::children`].
    pub fn path(&self) -> &[usize] {
        &self.path
    }

    /// A read-only lens on the same node.
    pub fn as_lens(&self) -> Lens<'_, E> {
        Lens { root: self.root, node: node_at(self.root, &self.path), path: self.path.clone(), _e: PhantomData }
    }

    fn node_mut(&mut self) -> &mut RawNode {
        node_at_mut(self.root, &self.path)
    }

    /// Sets `subtree_dirty` on this node and every ancestor.
    fn mark_path(&mut self) {
        let mut n = &mut *self.root;
        n.subtree_dirty = true;
        for &i in &self.path {
            n = n.children[i].as_element_mut().expect("path points at an element");
            n.subtree_dirty = true;
        }
    }

    fn descend<C: ElementSpec>(&mut self, index: usize) -> LensMut<'_, C> {
        let mut path = self.path.clone();
        path.push(index);
        LensMut { root: &mut *self.root, path, max_depth: self.max_depth, _e: PhantomData }
    }

    /// Mutable lens on the first child named `C::QNAME`.
    pub fn child_mut<C: ChildOf<E>>(&mut self) -> Option<LensMut<'_, C>> {
        self.child_mut_by::<C>(|_| true)
    }

    /// Mutable lens on the first child named `C::QNAME` for which `pred` holds.
    pub fn child_mut_by<C: ChildOf<E>>(&mut self, mut pred: impl FnMut(&RawNode) -> bool) -> Option<LensMut<'_, C>> {
        let i = self.node().indexed_elements().find(|(_, n)| n.is(C::QNAME) && pred(n)).map(|(i, _)| i)?;
        Some(self.descend(i))
    }

    /// Mutable lens on the `n`-th (0-based) child named `C::QNAME`.
    pub fn nth_child_mut<C: ChildOf<E>>(&mut self, n: usize) -> Option<LensMut<'_, C>> {
        let i = self.node().indexed_elements().filter(|(_, c)| c.is(C::QNAME)).nth(n).map(|(i, _)| i)?;
        Some(self.descend(i))
    }

    /// Consumes this lens and descends to the child item at `index` (an
    /// index into [`RawNode::children`]), if it is a `C` element. Unlike the
    /// `&mut self` accessors, the result keeps the full lifetime `'a`, so a
    /// helper can return a grandchild lens.
    pub fn into_child_at<C: ChildOf<E>>(mut self, index: usize) -> Option<LensMut<'a, C>> {
        let is_c = self.node().children.get(index)?.as_element().is_some_and(|n| n.is(C::QNAME));
        if !is_c {
            return None;
        }
        self.path.push(index);
        Some(LensMut { root: self.root, path: self.path, max_depth: self.max_depth, _e: PhantomData })
    }

    /// Consuming variant of [`Self::child_mut_by`].
    pub fn into_child_by<C: ChildOf<E>>(self, mut pred: impl FnMut(&RawNode) -> bool) -> Option<LensMut<'a, C>> {
        let i = self.node().indexed_elements().find(|(_, n)| n.is(C::QNAME) && pred(n)).map(|(i, _)| i)?;
        self.into_child_at(i)
    }

    /// Reinterprets this lens as another spec with the same QName.
    pub fn cast<F: ElementSpec>(self) -> Result<LensMut<'a, F>> {
        if F::QNAME != E::QNAME {
            return Err(Error::WrongElement {
                expected: F::QNAME,
                found_ns: E::QNAME.ns.into(),
                found_local: E::QNAME.local.into(),
            });
        }
        Ok(LensMut { root: self.root, path: self.path, max_depth: self.max_depth, _e: PhantomData })
    }

    // ---- insertion ----

    fn slots(&self) -> Result<&'static [Slot]> {
        match E::CONTENT {
            Content::Slots(s) => Ok(s),
            Content::Unsupported(_) => Err(Error::UnsupportedContentModel(E::QNAME)),
            Content::Empty | Content::Text(_) => Err(Error::NotAllowed { parent: E::QNAME, child: E::QNAME }),
        }
    }

    /// Candidate slots of an element: named slots first; wildcard slots only
    /// when no slot lists the name.
    fn candidates(slots: &[Slot], name: &XName) -> Vec<usize> {
        let named: Vec<usize> = slots
            .iter()
            .enumerate()
            .filter(|(_, s)| s.names.iter().any(|q| name.is(*q)))
            .map(|(i, _)| i)
            .collect();
        if !named.is_empty() {
            return named;
        }
        slots
            .iter()
            .enumerate()
            .filter(|(_, s)| s.wildcard.is_some_and(|w| w.matches(name.ns())))
            .map(|(i, _)| i)
            .collect()
    }

    fn slot_of(slots: &[Slot], q: QName) -> Vec<usize> {
        let named: Vec<usize> = slots.iter().enumerate().filter(|(_, s)| s.contains(q)).map(|(i, _)| i).collect();
        if !named.is_empty() {
            return named;
        }
        slots
            .iter()
            .enumerate()
            .filter(|(_, s)| s.wildcard.is_some_and(|w| w.matches(q.ns)))
            .map(|(i, _)| i)
            .collect()
    }

    /// Greedy left-to-right slot assignment of the existing children
    /// (ADR-0002 §4.1) and the insertion point for a `q` child in `slot`.
    fn place(
        &self,
        slots: &[Slot],
        slot: usize,
        q: QName,
        before: impl FnMut(usize, &RawNode) -> bool,
    ) -> Result<Placement> {
        self.place_skipping(slots, slot, q, before, None)
    }

    /// [`Self::place`] as if the child item at `skip` were absent (a child
    /// being moved within the same parent); the returned index is still in
    /// the current coordinates.
    fn place_skipping(
        &self,
        slots: &[Slot],
        slot: usize,
        q: QName,
        mut before: impl FnMut(usize, &RawNode) -> bool,
        skip: Option<usize>,
    ) -> Result<Placement> {
        let node = self.node();
        let mut cur = 0usize;
        let (mut same_q, mut same_slot, mut k) = (0u32, 0u32, 0usize);
        let mut at = None;
        for (i, child) in node.indexed_elements() {
            if Some(i) == skip {
                continue;
            }
            if child.is(MC_ALTERNATE_CONTENT) {
                return Err(Error::UnresolvedSibling(E::QNAME));
            }
            let cands = Self::candidates(slots, &child.name);
            if cands.is_empty() {
                continue; // unlisted: attached to the listed sibling before it
            }
            let Some(&rank) = cands.iter().find(|&&r| r >= cur) else {
                return Err(Error::UnresolvedSibling(E::QNAME));
            };
            cur = rank;
            if rank == slot {
                same_slot += 1;
                same_q += u32::from(child.is(q));
                if at.is_none() && before(k, child) {
                    at = Some(i);
                }
                k += 1;
            } else if rank > slot && at.is_none() {
                at = Some(i);
            }
        }
        let s = slots[slot];
        if s.max_each.is_some_and(|m| same_q >= m) || s.max_total.is_some_and(|m| same_slot >= m) {
            return Err(Error::AlreadyPresent(q));
        }
        Ok(Placement { index: at.unwrap_or(node.children.len()) })
    }

    fn insert_at_slot<C: ElementSpec>(
        &mut self,
        slot: usize,
        before: impl FnMut(usize, &RawNode) -> bool,
    ) -> Result<LensMut<'_, C>> {
        // The new element's depth is this node's (path length + 1) plus one.
        // Refused like the parser would refuse it, so an edited tree can
        // always be parsed again and recursive walks stay bounded.
        if self.path.len() + 2 > self.max_depth {
            return Err(Error::TooDeep { limit: self.max_depth, position: self.node().span.map_or(0, |s| s.start) });
        }
        let slots = self.slots()?;
        let Placement { index } = self.place(slots, slot, C::QNAME, before)?;
        let prefix = choose_element_prefix(self.root, &self.path, C::QNAME.ns)?;
        let name = XName::new(C::QNAME.ns.into(), C::QNAME.local, prefix.as_deref());
        let parent = self.node_mut();
        if parent.span.is_some_and(|s| s.close.is_none()) && parent.children.is_empty() {
            parent.own_dirty = true; // self-closing parent gains its first child
        }
        parent.children.insert(index, RawItem::Element(RawNode::new_created(name)));
        self.mark_path();
        Ok(self.descend(index))
    }

    /// Inserts a new `C` into its slot. `before(index_in_slot, existing)` is
    /// called on the slot's existing children in document order; the new node
    /// goes before the first one for which it returns `true`, else before the
    /// first child of a higher-ranked slot, else at the end.
    ///
    /// Errors: [`Error::AlreadyPresent`] (slot limit),
    /// [`Error::AmbiguousSlot`] (QName in two slots — use
    /// [`Self::insert_in_slot`]), [`Error::UnresolvedSibling`]
    /// (`mc:AlternateContent` sibling or out-of-order children),
    /// [`Error::UnsupportedContentModel`], [`Error::NamespaceNotInScope`].
    pub fn insert_by<C: ChildOf<E>>(&mut self, before: impl FnMut(usize, &RawNode) -> bool) -> Result<LensMut<'_, C>> {
        let slots = self.slots()?;
        let slot = match Self::slot_of(slots, C::QNAME)[..] {
            [s] => s,
            [] => return Err(Error::NotAllowed { parent: E::QNAME, child: C::QNAME }),
            _ => return Err(Error::AmbiguousSlot(C::QNAME)),
        };
        self.insert_at_slot::<C>(slot, before)
    }

    /// Inserts a new `C` child at child item index `index` (an index into
    /// [`RawNode::children`], `0..=len`), for callers that already know the
    /// position among same-slot siblings (e.g. from their own row index).
    /// For a slot **without limits** the checks look only at the nearest
    /// listed neighbours (plus any unlisted children between them), so the
    /// cost is O(1) apart from the `Vec` insertion — where [`Self::insert_by`]
    /// scans every child. A slot with limits is counted over all children,
    /// as `insert_by` does.
    ///
    /// Schema order is checked **locally**: every slot the nearest listed
    /// element before `index` could belong to must not come after `C`'s, every
    /// slot of the nearest listed element after it must not come before
    /// `C`'s (so an ambiguous neighbour such as a name listed in two slots is
    /// refused unless every reading is in order: use `insert_by` or
    /// `insert_in_slot` there), and neither may be an `mc:AlternateContent`
    /// ([`Error::UnresolvedSibling`]). Disorder elsewhere in the parent is not
    /// examined (where `insert_by` refuses the whole parent). Slot limits are
    /// enforced exactly as by `insert_by` ([`Error::AlreadyPresent`]; an
    /// `mc:AlternateContent` anywhere then refuses). Unlisted
    /// children are skipped, as in `insert_by`. The caller owns the order
    /// within the slot. Dirty duties, depth limit and prefix choice are those
    /// of `insert_by`. [`Self::check_insert_at`] runs the same checks without
    /// changing anything.
    pub fn insert_at<C: ChildOf<E>>(&mut self, index: usize) -> Result<LensMut<'_, C>> {
        self.check_insert_at::<C>(index)?;
        let prefix = choose_element_prefix(self.root, &self.path, C::QNAME.ns)?;
        let name = XName::new(C::QNAME.ns.into(), C::QNAME.local, prefix.as_deref());
        let parent = self.node_mut();
        if parent.span.is_some_and(|s| s.close.is_none()) && parent.children.is_empty() {
            parent.own_dirty = true; // self-closing parent gains its first child
        }
        parent.children.insert(index, RawItem::Element(RawNode::new_created(name)));
        self.mark_path();
        Ok(self.descend(index))
    }

    /// Every check of [`Self::insert_at`] (index range, depth limit, slot,
    /// local schema order, slot limits, namespace prefix), without changing
    /// anything: if it returns `Ok`, `insert_at::<C>(index)` succeeds on the
    /// unchanged tree. Lets a caller validate a multi-step edit first.
    pub fn check_insert_at<C: ChildOf<E>>(&self, index: usize) -> Result<()> {
        let len = self.node().children.len();
        if index > len {
            return Err(Error::InsertIndexOutOfRange { index, len });
        }
        if self.path.len() + 2 > self.max_depth {
            return Err(Error::TooDeep { limit: self.max_depth, position: self.node().span.map_or(0, |s| s.start) });
        }
        let slots = self.slots()?;
        let slot = match Self::slot_of(slots, C::QNAME)[..] {
            [s] => s,
            [] => return Err(Error::NotAllowed { parent: E::QNAME, child: C::QNAME }),
            _ => return Err(Error::AmbiguousSlot(C::QNAME)),
        };
        let children = &self.node().children;
        let listed = |n: &RawNode| -> Result<Option<Vec<usize>>> {
            if n.is(MC_ALTERNATE_CONTENT) {
                return Err(Error::UnresolvedSibling(E::QNAME));
            }
            let c = Self::candidates(slots, &n.name);
            Ok((!c.is_empty()).then_some(c))
        };
        for item in children[..index].iter().rev() {
            if let Some(n) = item.as_element()
                && let Some(c) = listed(n)?
            {
                // every candidate: an ambiguous neighbour's actual slot depends
                // on the greedy assignment of the children before it (review F1)
                if !c.iter().all(|&r| r <= slot) {
                    return Err(Error::UnresolvedSibling(E::QNAME));
                }
                break;
            }
        }
        for item in &children[index..] {
            if let Some(n) = item.as_element()
                && let Some(c) = listed(n)?
            {
                if !c.iter().all(|&r| r >= slot) {
                    return Err(Error::UnresolvedSibling(E::QNAME));
                }
                break;
            }
        }
        // A slot with limits is counted exactly as `insert_by` counts it: the
        // full greedy assignment, which also refuses an `mc:AlternateContent`
        // anywhere in the parent (reviews F2, F3). O(children); unlimited
        // slots (rows, cells) never take this path.
        let s = slots[slot];
        if s.max_each.is_some() || s.max_total.is_some() {
            self.place(slots, slot, C::QNAME, |_, _| false)?;
        }
        choose_element_prefix(self.root, &self.path, C::QNAME.ns)?;
        Ok(())
    }

    /// `insert_by` that never returns `true`: append at the end of the slot.
    pub fn insert<C: ChildOf<E>>(&mut self) -> Result<LensMut<'_, C>> {
        self.insert_by::<C>(|_, _| false)
    }

    /// As [`Self::insert_by`], with the slot index given explicitly (needed
    /// only for a QName that appears in two slots).
    pub fn insert_in_slot<C: ChildOf<E>>(
        &mut self,
        slot: usize,
        before: impl FnMut(usize, &RawNode) -> bool,
    ) -> Result<LensMut<'_, C>> {
        let slots = self.slots()?;
        if !Self::slot_of(slots, C::QNAME).contains(&slot) {
            return Err(Error::NotAllowed { parent: E::QNAME, child: C::QNAME });
        }
        self.insert_at_slot::<C>(slot, before)
    }

    /// The first existing `C` child, or a newly inserted one. The normal path
    /// for `maxOccurs = 1` children.
    pub fn get_or_insert<C: ChildOf<E>>(&mut self) -> Result<LensMut<'_, C>> {
        let found = self.node().indexed_elements().find(|(_, n)| n.is(C::QNAME)).map(|(i, _)| i);
        match found {
            Some(i) => Ok(self.descend(i)),
            None => self.insert::<C>(),
        }
    }

    /// Inserts a [`Detached`] copy in `C`'s slot, placed exactly as
    /// [`Self::insert_by`] places a new `C` (same slot rules, limits and
    /// refusals). Every element and attribute of the copy gets its prefix
    /// chosen at the destination by the ADR-0002 §3 rules (the copy declares
    /// no namespaces, so all of it sees the parent's bindings). All prefixes
    /// are chosen before the tree changes: a namespace without a usable
    /// prefix is [`Error::NamespaceNotInScope`] and changes nothing. The
    /// inserted nodes are created nodes (rendered in full on output).
    pub fn insert_detached_by<C: ChildOf<E>>(
        &mut self,
        d: &Detached<C>,
        before: impl FnMut(usize, &RawNode) -> bool,
    ) -> Result<LensMut<'_, C>> {
        let slots = self.slots()?;
        let slot = match Self::slot_of(slots, C::QNAME)[..] {
            [s] => s,
            [] => return Err(Error::NotAllowed { parent: E::QNAME, child: C::QNAME }),
            _ => return Err(Error::AmbiguousSlot(C::QNAME)),
        };
        // The whole copy must fit under the depth limit (ADR-0002 note
        // "parser depth limit"): its deepest element is at
        // parent depth (path len + 1) + height.
        if self.path.len() + 1 + d.height() > self.max_depth {
            return Err(Error::TooDeep { limit: self.max_depth, position: self.node().span.map_or(0, |s| s.start) });
        }
        let Placement { index } = self.place(slots, slot, C::QNAME, before)?;
        check_destination(&d.root, d.text_inherits_space, self.root, &self.path)?;
        let decls = declarations(self.root, &self.path);
        let parent_prefix = self.node().name.prefix();
        let node = with_prefixes(d.root.clone(), &decls, parent_prefix)?;
        let parent = self.node_mut();
        if parent.span.is_some_and(|s| s.close.is_none()) && parent.children.is_empty() {
            parent.own_dirty = true; // self-closing parent gains its first child
        }
        parent.children.insert(index, RawItem::Element(node));
        self.mark_path();
        Ok(self.descend(index))
    }

    /// `insert_detached_by` that appends at the end of the slot.
    pub fn insert_detached<C: ChildOf<E>>(&mut self, d: &Detached<C>) -> Result<LensMut<'_, C>> {
        self.insert_detached_by(d, |_, _| false)
    }

    // ---- removal ----

    /// Removes the first `C` child (and its whole span). Returns whether one existed.
    pub fn remove<C: ChildOf<E>>(&mut self) -> bool {
        let found = self.node().indexed_elements().find(|(_, n)| n.is(C::QNAME)).map(|(i, _)| i);
        match found {
            Some(i) => self.remove_at(i),
            None => false,
        }
    }

    /// Removes the child item (element, text or markup) at `index`.
    /// Returns `false` if out of range.
    pub fn remove_at(&mut self, index: usize) -> bool {
        let parent = self.node_mut();
        if index >= parent.children.len() {
            return false;
        }
        parent.children.remove(index);
        self.mark_path();
        true
    }

    // ---- attributes ----

    /// Refusals shared by `set_attr` and `remove_attr`: an incomplete spec,
    /// and namespace declarations. A declaration is `{XMLNS_NS}*`, or an
    /// unqualified `xmlns` / `xmlns:*` (which would render as one). Writing
    /// one could unbind a prefix still in use (not well-formed output) or
    /// silently move existing names to another namespace (fuzz finding F-3).
    fn check_attr_write(&self, q: QName) -> Result<()> {
        if q.ns == XMLNS_NS || (q.ns.is_empty() && (q.local == "xmlns" || q.local.starts_with("xmlns:"))) {
            return Err(Error::NamespaceDeclaration(q));
        }
        if E::ATTRS_INCOMPLETE { Err(Error::IncompleteSpec(E::QNAME)) } else { Ok(()) }
    }

    /// Sets attribute `q` (untyped). An existing attribute keeps its position
    /// and prefix; a new one is appended. A namespaced attribute needs a
    /// non-empty prefix bound to its URI ([`Error::NamespaceNotInScope`]).
    /// A value with a character outside XML 1.0 `Char` is refused
    /// ([`Error::InvalidXmlChar`]) before anything changes. Namespace
    /// declarations are refused ([`Error::NamespaceDeclaration`]).
    pub fn set_attr(&mut self, q: QName, value: &str) -> Result<()> {
        self.check_attr_write(q)?;
        check_xml_chars(value)?;
        let existing = self.node().attrs.iter().position(|a| a.name.is(q));
        let prefix = match existing {
            Some(_) => None,
            None => choose_attr_prefix(self.root, &self.path, q.ns)?,
        };
        let node = self.node_mut();
        match existing {
            Some(i) => {
                if node.attrs[i].value == value {
                    return Ok(()); // no change, no dirt
                }
                node.attrs[i].value = value.into();
            }
            None => node.attrs.push(RawAttr { name: XName::new(q.ns.into(), q.local, prefix.as_deref()), value: value.into() }),
        }
        node.own_dirty = true;
        self.mark_path();
        Ok(())
    }

    /// Removes attribute `q`. Returns whether it existed. Namespace
    /// declarations are refused ([`Error::NamespaceDeclaration`]).
    pub fn remove_attr(&mut self, q: QName) -> Result<bool> {
        self.check_attr_write(q)?;
        let node = self.node_mut();
        let Some(i) = node.attrs.iter().position(|a| a.name.is(q)) else {
            return Ok(false);
        };
        node.attrs.remove(i);
        node.own_dirty = true;
        self.mark_path();
        Ok(true)
    }

    // ---- text ----

    /// Replaces the text content of a `Content::Text` element. If `value` has
    /// leading or trailing whitespace and `E::ATTRS` declares `xml:space`,
    /// also sets `xml:space="preserve"` (never removes it). A value with a
    /// character outside XML 1.0 `Char` is refused ([`Error::InvalidXmlChar`])
    /// before anything changes.
    pub fn set_text(&mut self, value: &str) -> Result<()> {
        if !matches!(E::CONTENT, Content::Text(_)) {
            return Err(Error::NotText(E::QNAME));
        }
        check_xml_chars(value)?;
        if has_edge_ws(value) && E::attr_spec(XML_SPACE).is_some() && self.node().attr(XML_SPACE) != Some("preserve") {
            self.set_attr(XML_SPACE, "preserve")?;
        }
        let node = self.node_mut();
        if node.text() == value && node.children.iter().all(|c| matches!(c, RawItem::Text(_))) {
            return Ok(());
        }
        let first = node.children.iter().position(|c| matches!(c, RawItem::Text(_)));
        node.children.retain(|c| !matches!(c, RawItem::Text(_)));
        if !value.is_empty() {
            if node.span.is_some_and(|s| s.close.is_none()) && node.children.is_empty() {
                node.own_dirty = true;
            }
            let at = first.unwrap_or(node.children.len()).min(node.children.len());
            node.children.insert(at, RawItem::Text(RawText { value: value.into(), span: None, cdata: false }));
        }
        self.mark_path();
        Ok(())
    }
}

/// Namespace declarations on the root and every element along `path`
/// (root first): (depth, declared prefix, uri).
fn declarations<'r>(root: &'r RawNode, path: &[usize]) -> Vec<(usize, Option<&'r str>, &'r str)> {
    let mut out = Vec::new();
    let mut n = root;
    let mut depth = 0;
    loop {
        out.extend(
            n.attrs
                .iter()
                .filter(|a| a.name.ns() == XMLNS_NS)
                .map(|a| (depth, a.name.declared_prefix(), a.value.as_str())),
        );
        let Some(&i) = path.get(depth) else { break };
        n = n.children[i].as_element().expect("path points at an element");
        depth += 1;
    }
    out
}

/// The binding in effect for `prefix` (innermost declaration wins).
fn effective<'r>(decls: &[(usize, Option<&'r str>, &'r str)], prefix: Option<&str>) -> Option<&'r str> {
    decls.iter().rev().find(|(_, p, _)| *p == prefix).map(|(_, _, u)| *u)
}

/// Prefix for a new element in namespace `uri` under the element at
/// `parent_path` (ADR-0002 §3, prefix rules). `Ok(None)` = unprefixed.
pub(crate) fn choose_element_prefix(root: &RawNode, parent_path: &[usize], uri: &str) -> Result<Option<String>> {
    let decls = declarations(root, parent_path);
    element_prefix_in(&decls, node_at(root, parent_path).name.prefix(), uri)
}

/// [`choose_element_prefix`] on given declarations and parent prefix.
fn element_prefix_in(decls: &[(usize, Option<&str>, &str)], pp: Option<&str>, uri: &str) -> Result<Option<String>> {
    if uri == XML_NS {
        return Ok(Some("xml".into()));
    }
    if uri.is_empty() {
        return match effective(decls, None) {
            None | Some("") => Ok(None),
            Some(_) => Err(Error::NamespaceNotInScope(String::new())),
        };
    }
    if effective(decls, pp) == Some(uri) {
        return Ok(pp.map(Into::into));
    }
    // Innermost element first; within one element the default namespace wins.
    let max_depth = decls.iter().map(|d| d.0).max().unwrap_or(0);
    for depth in (0..=max_depth).rev() {
        let here: Vec<_> = decls
            .iter()
            .filter(|(d, p, u)| *d == depth && *u == uri && effective(decls, *p) == Some(uri))
            .collect();
        if here.iter().any(|(_, p, _)| p.is_none()) {
            return Ok(None);
        }
        if let Some((_, Some(p), _)) = here.first() {
            return Ok(Some((*p).into()));
        }
    }
    Err(Error::NamespaceNotInScope(uri.into()))
}

/// Prefix for a new attribute in namespace `uri` on the element at `path`.
pub(crate) fn choose_attr_prefix(root: &RawNode, path: &[usize], uri: &str) -> Result<Option<String>> {
    attr_prefix_in(&declarations(root, path), uri)
}

/// [`choose_attr_prefix`] on given declarations.
fn attr_prefix_in(decls: &[(usize, Option<&str>, &str)], uri: &str) -> Result<Option<String>> {
    if uri.is_empty() {
        return Ok(None);
    }
    if uri == XML_NS {
        return Ok(Some("xml".into()));
    }
    decls
        .iter()
        .rev()
        .find(|(_, p, u)| *u == uri && p.is_some() && effective(decls, *p) == Some(uri))
        .and_then(|(_, p, _)| p.map(String::from))
        .map(Some)
        .ok_or_else(|| Error::NamespaceNotInScope(uri.into()))
}

/// A detached subtree with every element and attribute prefix chosen for
/// a parent with declarations `decls` and prefix `parent_prefix` (the copy
/// declares nothing itself, so `decls` hold for the whole subtree).
fn with_prefixes(mut n: RawNode, decls: &[(usize, Option<&str>, &str)], parent_prefix: Option<&str>) -> Result<RawNode> {
    let ns: Arc<str> = n.name.ns().into();
    let prefix = element_prefix_in(decls, parent_prefix, &ns)?;
    n.name = XName::new(ns, n.name.local(), prefix.as_deref());
    for a in &mut n.attrs {
        let ans: Arc<str> = a.name.ns().into();
        let p = attr_prefix_in(decls, &ans)?;
        a.name = XName::new(ans, a.name.local(), p.as_deref());
    }
    let children = std::mem::take(&mut n.children);
    n.children = children
        .into_iter()
        .map(|c| match c {
            RawItem::Element(e) => with_prefixes(e, decls, prefix.as_deref()).map(RawItem::Element),
            other => Ok(other),
        })
        .collect::<Result<_>>()?;
    Ok(n)
}
