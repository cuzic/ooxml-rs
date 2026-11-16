//! Same-part move of an element subtree (ADR-0002 post-acceptance note
//! "same-part move"): `LensMut::move_in[_by]` and its check twin
//! `check_move_in_by`.
//!
//! A move detaches nothing: the moved node keeps its source span and dirty
//! flags, so its bytes are written verbatim at the destination, and the
//! output differs from the input only by those bytes leaving the source
//! position and appearing at the destination (T2-style locality). Because
//! the bytes (prefixes included) do not change, every prefix the subtree
//! uses must mean the same namespace at the destination as at the source;
//! likewise `mc:Ignorable` and an inherited `xml:space` must not change the
//! subtree's meaning. Every check runs before the tree changes.

use std::collections::BTreeSet;

use super::{LensMut, Placement};
use crate::error::{Error, MoveRefusal, Result};
use crate::name::{MC_NS, QName, XML_NS, XMLNS_NS};
use crate::schema::{ChildOf, ElementSpec};
use crate::xml::{RawItem, RawNode};

const MC_IGNORABLE: QName = QName::new(MC_NS, "Ignorable");

/// What a move will do, computed by [`LensMut::check_move_in_by`] without
/// changing anything.
struct MovePlan {
    /// Source parent path and the child index there.
    src_parent: Vec<usize>,
    src_index: usize,
    /// Destination parent path **after** the removal, and the insertion
    /// index there.
    dst_parent: Vec<usize>,
    dst_index: usize,
}

fn refused(reason: MoveRefusal, name: &str) -> Error {
    core::hint::cold_path();
    Error::NotMovable { reason, name: name.to_owned() }
}

/// The element at `path`, or `Err(WrongElement)` naming `expected`.
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

fn height(n: &RawNode) -> usize {
    1 + n.elements().map(height).max().unwrap_or(0)
}

/// Namespace bindings, `mc:Ignorable` namespaces and the `xml:space` in
/// effect at (and including) the element at `path`.
struct Scope<'r> {
    decls: Vec<(Option<&'r str>, &'r str)>,
    ignorable: Vec<&'r str>,
    preserve: bool,
}

impl<'r> Scope<'r> {
    fn at(root: &'r RawNode, path: &[usize]) -> Self {
        let mut s = Scope { decls: Vec::new(), ignorable: Vec::new(), preserve: false };
        let mut n = root;
        let mut rest = path.iter();
        loop {
            s.decls.extend(n.attrs.iter().filter(|a| a.name.ns() == XMLNS_NS).map(|a| (a.name.declared_prefix(), a.value.as_str())));
            if let Some(v) = n.attr(MC_IGNORABLE) {
                for p in v.split_ascii_whitespace() {
                    if let Some((_, u)) = s.decls.iter().rev().find(|(q, _)| *q == Some(p)) {
                        s.ignorable.push(u);
                    }
                }
            }
            if let Some(p) = n.xml_space_preserve() {
                s.preserve = p;
            }
            let Some(&i) = rest.next() else { break };
            n = n.children[i].as_element().expect("validated path");
        }
        s
    }

    /// The URI `prefix` is bound to here; `""` for an unbound default.
    fn binding(&self, prefix: Option<&str>) -> Option<&'r str> {
        match self.decls.iter().rev().find(|(q, _)| *q == prefix) {
            Some((_, u)) => Some(u),
            None if prefix.is_none() => Some(""),
            None => None,
        }
    }
}

/// Prefixes (element prefixes, `None` = default, and non-empty attribute
/// prefixes) and namespace URIs the subtree uses, and whether it has text
/// whose whitespace handling it inherits (no own `xml:space` above it).
fn usage<'n>(n: &'n RawNode, prefixes: &mut BTreeSet<Option<&'n str>>, uris: &mut BTreeSet<&'n str>, inherits_text: &mut bool, own_space: bool) {
    let own_space = own_space || n.xml_space_preserve().is_some();
    if n.name.ns() != XML_NS {
        prefixes.insert(n.name.prefix());
    }
    uris.insert(n.name.ns());
    for a in &n.attrs {
        if a.name.ns() == XMLNS_NS || a.name.ns() == XML_NS || a.name.ns().is_empty() {
            continue;
        }
        prefixes.insert(a.name.prefix());
        uris.insert(a.name.ns());
    }
    for c in &n.children {
        match c {
            RawItem::Element(e) => usage(e, prefixes, uris, inherits_text, own_space),
            RawItem::Text(t) if !own_space && !t.value.is_empty() => *inherits_text = true,
            _ => {}
        }
    }
}

impl<E: ElementSpec> LensMut<'_, E> {
    /// Checks a move of the element at `from` (a path of child indices from
    /// the part root, like [`Self::path`]) into this element, placed as
    /// [`Self::insert_by`] would place a new `C`, **without changing
    /// anything**. `Ok` exactly when [`Self::move_in_by`] with the same
    /// arguments succeeds.
    ///
    /// Refused: the root or a missing element at `from` (`NotMovable` /
    /// `WrongElement`), an element not named `C::QNAME` (`WrongElement`),
    /// this element or one of its ancestors (`NotMovable { IntoItself }`),
    /// the insertion refusals of `insert_by` (computed on the children
    /// **without** the moved element when it is already one of them),
    /// `TooDeep` (the subtree's deepest element passes the part's limit),
    /// a prefix used in the subtree that is bound to another namespace (or
    /// unbound) at the destination (`NamespaceBinding`), a namespace of the
    /// subtree whose `mc:Ignorable` status differs (`IgnorableNamespace`),
    /// and text whose inherited `xml:space` differs (`XmlSpace`).
    pub fn check_move_in_by<C: ChildOf<E>>(&self, from: &[usize], before: impl FnMut(usize, &RawNode) -> bool) -> Result<()> {
        self.plan_move::<C>(from, before).map(|_| ())
    }

    /// Moves the element at `from` (and its subtree) into this element, in
    /// `C`'s slot, before the first existing child of the slot for which
    /// `before(index_in_slot, child)` is `true` (else at the end of the
    /// slot), and returns a lens on it. Refusals: those of
    /// [`Self::check_move_in_by`], all before any change.
    ///
    /// Dirty duties: the source parent and its ancestors get
    /// `subtree_dirty` (a parent with a close tag keeps its tags even when
    /// emptied); the destination parent and its ancestors get
    /// `subtree_dirty`, and `own_dirty` if it was self-closing and gains its
    /// first child; the moved subtree keeps its spans and flags, so it is
    /// written byte-for-byte.
    pub fn move_in_by<C: ChildOf<E>>(&mut self, from: &[usize], before: impl FnMut(usize, &RawNode) -> bool) -> Result<LensMut<'_, C>> {
        let plan = self.plan_move::<C>(from, before)?;
        let parent = super::node_at_mut(self.root, &plan.src_parent);
        let RawItem::Element(node) = parent.children.remove(plan.src_index) else { unreachable!("validated element") };
        mark(self.root, &plan.src_parent);
        self.path = plan.dst_parent;
        let dst = super::node_at_mut(self.root, &self.path);
        if dst.span.is_some_and(|s| s.close.is_none()) && dst.children.is_empty() {
            dst.own_dirty = true; // self-closing parent gains its first child
        }
        dst.children.insert(plan.dst_index, RawItem::Element(node));
        self.mark_path();
        Ok(self.descend(plan.dst_index))
    }

    /// [`Self::move_in_by`] to the end of `C`'s slot.
    pub fn move_in<C: ChildOf<E>>(&mut self, from: &[usize]) -> Result<LensMut<'_, C>> {
        self.move_in_by::<C>(from, |_, _| false)
    }

    fn plan_move<C: ChildOf<E>>(&self, from: &[usize], before: impl FnMut(usize, &RawNode) -> bool) -> Result<MovePlan> {
        let Some((&src_index, src_parent)) = from.split_last() else {
            return Err(refused(MoveRefusal::Root, &C::QNAME.to_string()));
        };
        let node = element_at(self.root, from, C::QNAME)?;
        if !node.is(C::QNAME) {
            return Err(Error::WrongElement { expected: C::QNAME, found_ns: node.name.ns().into(), found_local: node.name.local().into() });
        }
        if self.path.starts_with(from) {
            return Err(refused(MoveRefusal::IntoItself, &C::QNAME.to_string()));
        }
        // destination slot and placement, without the moved element
        let slots = self.slots()?;
        let slot = match Self::slot_of(slots, C::QNAME)[..] {
            [s] => s,
            [] => return Err(Error::NotAllowed { parent: E::QNAME, child: C::QNAME }),
            _ => return Err(Error::AmbiguousSlot(C::QNAME)),
        };
        let same_parent = src_parent == self.path.as_slice();
        let skip = same_parent.then_some(src_index);
        let Placement { index } = self.place_skipping(slots, slot, C::QNAME, before, skip)?;
        let dst_index = match skip {
            Some(s) if s < index => index - 1,
            _ => index,
        };
        // the destination path after the removal
        let mut dst_parent = self.path.clone();
        if dst_parent.len() > src_parent.len() && dst_parent.starts_with(src_parent) && dst_parent[src_parent.len()] > src_index {
            dst_parent[src_parent.len()] -= 1;
        }
        // depth: the moved element sits at (destination depth + 1)
        if (self.path.len() + 1).saturating_add(height(node)) > self.max_depth {
            return Err(Error::TooDeep { limit: self.max_depth, position: self.node().span.map_or(0, |s| s.start) });
        }
        // meaning at the destination
        let src = Scope::at(self.root, src_parent);
        let dst = Scope::at(self.root, &self.path);
        let (mut prefixes, mut uris, mut inherits_text) = (BTreeSet::new(), BTreeSet::new(), false);
        usage(node, &mut prefixes, &mut uris, &mut inherits_text, false);
        for p in prefixes {
            if src.binding(p) != dst.binding(p) {
                return Err(refused(MoveRefusal::NamespaceBinding, p.unwrap_or("(default namespace)")));
            }
        }
        for u in uris {
            if src.ignorable.contains(&u) != dst.ignorable.contains(&u) {
                return Err(refused(MoveRefusal::IgnorableNamespace, u));
            }
        }
        if inherits_text && src.preserve != dst.preserve {
            return Err(refused(MoveRefusal::XmlSpace, &node.name.display().to_string()));
        }
        Ok(MovePlan { src_parent: src_parent.to_vec(), src_index, dst_parent, dst_index })
    }
}

/// Sets `subtree_dirty` on the element at `path` and every ancestor.
fn mark(root: &mut RawNode, path: &[usize]) {
    let mut n = root;
    n.subtree_dirty = true;
    for &i in path {
        n = n.children[i].as_element_mut().expect("path points at an element");
        n.subtree_dirty = true;
    }
}
