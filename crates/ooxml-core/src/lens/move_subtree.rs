//! Same-part move of an element subtree (ADR-0002 post-acceptance note
//! "same-part move"): `LensMut::move_in[_by]` and its check twin
//! `check_move_in_by`.
//!
//! A move detaches nothing: the moved node keeps its source span and dirty
//! flags, so its bytes are written verbatim at the destination, and the
//! output differs from the input only by those bytes leaving the source
//! position and appearing at the destination (T2-style locality). Because
//! the bytes (prefixes included) do not change, every prefix the subtree
//! uses without declaring it must mean the same namespace at the
//! destination as at the source (prefixes named inside Markup Compatibility
//! attribute values included); likewise `mc:Ignorable` and an inherited
//! `xml:space` must not change the subtree's meaning. Every check runs
//! before the tree changes.
//!
//! The ancestor walk (`ancestor_context`), the path lookup (`element_at`),
//! the binding lookup (`declarations` / `effective`) and the subtree height
//! are shared with the detach/insert primitives of the parent module.

use std::collections::BTreeSet;
use std::marker::PhantomData;

use super::{Lens, LensMut, Placement, ancestor_context, declarations, effective, element_at, subtree_height};
use crate::error::{Error, MoveRefusal, Result};
use crate::name::{MC_NS, XML_NS, XMLNS_NS};
use crate::schema::{ChildOf, ElementSpec, SameType};
use crate::xml::{RawItem, RawNode};

/// The element a move takes, captured from a typed [`Lens`] (so it was
/// reached through `ChildOf` links and has `S`'s complex type there, as
/// [`Lens::detach_as`] relies on). Holds only a path: take it, drop the
/// read lens, then call [`LensMut::move_in_by`]. A path is stale after any
/// structural change above or before it (see [`LensMut::move_in_by`]);
/// the move then refuses a path that no longer leads to an `S::QNAME`
/// element, but cannot tell a different element of the same name.
pub struct MoveSource<S: ElementSpec> {
    path: Vec<usize>,
    _s: PhantomData<fn() -> S>,
}

impl<S: ElementSpec> MoveSource<S> {
    /// The source path (child indices from the part root).
    pub fn path(&self) -> &[usize] {
        &self.path
    }
}

impl<S: ElementSpec> Clone for MoveSource<S> {
    fn clone(&self) -> Self {
        MoveSource { path: self.path.clone(), _s: PhantomData }
    }
}

impl<S: ElementSpec> std::fmt::Debug for MoveSource<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MoveSource<{}>{:?}", S::SDK_NAME, self.path)
    }
}

impl<E: ElementSpec> Lens<'_, E> {
    /// This element as the source of a same-part move
    /// ([`LensMut::move_in_by`]).
    pub fn move_source(&self) -> MoveSource<E> {
        MoveSource { path: self.path.clone(), _s: PhantomData }
    }
}

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

/// Markup Compatibility attributes whose values name prefixes
/// (ISO/IEC 29500-3 §7): every token is a prefix (`Ignorable`,
/// `MustUnderstand`) or `prefix:local` / `prefix:*` (the others).
const MC_PREFIX_LISTS: [&str; 5] = ["Ignorable", "MustUnderstand", "ProcessContent", "PreserveElements", "PreserveAttributes"];

/// What the subtree needs from its context.
#[derive(Default)]
struct Usage<'n> {
    /// Prefixes used but not declared inside the subtree (`None` = the
    /// default namespace), from element and attribute names and from Markup
    /// Compatibility attribute values.
    free_prefixes: BTreeSet<Option<&'n str>>,
    /// Namespaces of element and attribute names.
    uris: BTreeSet<&'n str>,
    /// Text whose whitespace handling the subtree inherits.
    inherits_text: bool,
}

impl<'n> Usage<'n> {
    fn of(n: &'n RawNode) -> Self {
        let mut u = Usage::default();
        let mut declared = Vec::new();
        u.walk(n, &mut declared, false);
        u
    }

    fn use_prefix(&mut self, p: Option<&'n str>, declared: &[Option<&'n str>]) {
        if !declared.contains(&p) {
            self.free_prefixes.insert(p);
        }
    }

    fn walk(&mut self, n: &'n RawNode, declared: &mut Vec<Option<&'n str>>, own_space: bool) {
        let mark = declared.len();
        declared.extend(n.attrs.iter().filter(|a| a.name.ns() == XMLNS_NS).map(|a| a.name.declared_prefix()));
        let own_space = own_space || n.xml_space_preserve().is_some();
        if n.name.ns() != XML_NS {
            self.use_prefix(n.name.prefix(), declared);
        }
        self.uris.insert(n.name.ns());
        let is_choice = n.name.ns() == MC_NS && n.name.local() == "Choice";
        for a in &n.attrs {
            let mc_list = a.name.ns() == MC_NS && MC_PREFIX_LISTS.contains(&a.name.local());
            let requires = is_choice && a.name.ns().is_empty() && a.name.local() == "Requires";
            if mc_list || requires {
                for token in a.value.split_ascii_whitespace() {
                    let p = token.split_once(':').map_or(token, |(p, _)| p);
                    self.use_prefix(Some(p), declared);
                }
            }
            if a.name.ns() == XMLNS_NS || a.name.ns() == XML_NS || a.name.ns().is_empty() {
                continue;
            }
            self.use_prefix(a.name.prefix(), declared);
            self.uris.insert(a.name.ns());
        }
        // Whitespace-only text between child elements is indentation, not
        // content (review of PR #44, m3b); a leaf's whitespace still counts.
        let has_elements = n.elements().next().is_some();
        for c in &n.children {
            match c {
                RawItem::Element(e) => self.walk(e, declared, own_space),
                RawItem::Text(t) if !own_space && !t.value.is_empty() && !(has_elements && t.is_whitespace()) => {
                    self.inherits_text = true
                }
                _ => {}
            }
        }
        declared.truncate(mark);
    }
}

impl<E: ElementSpec> LensMut<'_, E> {
    /// Checks a move of the element `from` (taken with
    /// [`Lens::move_source`]) into this element, placed as
    /// [`Self::insert_by`] would place a new `C`, **without changing
    /// anything**. `Ok` exactly when [`Self::move_in_by`] with the same
    /// arguments succeeds.
    ///
    /// Refused: the root (`NotMovable { Root }`), a path that no longer
    /// leads to an `S::QNAME` element (`WrongElement`), this element or one
    /// of its ancestors (`IntoItself`), a source or destination inside
    /// `mc:AlternateContent` (`AlternateContent`), the insertion refusals of
    /// `insert_by` (computed on the children **without** the moved element
    /// when it is already one of them), `TooDeep` (the subtree's deepest
    /// element passes the part's limit), a prefix the subtree uses without
    /// declaring it (in a name, or in a Markup Compatibility attribute value
    /// such as `mc:Ignorable` or `mc:Choice/@Requires`) that is bound to
    /// another namespace, or unbound, at the destination
    /// (`NamespaceBinding`), a namespace of the subtree whose `mc:Ignorable`
    /// status differs (`IgnorableNamespace`), and text whose inherited
    /// `xml:space` differs (`XmlSpace`; indentation between child elements
    /// does not count).
    ///
    /// **Batches.** `NamespaceBinding`, `IgnorableNamespace`, `XmlSpace`,
    /// `AlternateContent` and `TooDeep` depend only on the ancestors'
    /// attributes and the subtree, which no move changes: checked once, they
    /// stay valid for later moves of the same elements. Slot limits, order
    /// and `IntoItself` depend on the current children: re-check (or check
    /// on a clone of the document) after each move.
    pub fn check_move_in_by<C: ChildOf<E>>(
        &self,
        from: &MoveSource<impl SameType<C>>,
        before: impl FnMut(usize, &RawNode) -> bool,
    ) -> Result<()> {
        self.plan_move::<C>(from.path(), before).map(|_| ())
    }

    /// Moves the element `from` (and its subtree) into this element, in
    /// `C`'s slot, before the first existing child of the slot for which
    /// `before(index_in_slot, child)` is `true` (else at the end of the
    /// slot), and returns a lens on it. The source's spec `S` must have
    /// `C`'s complex type (`S: SameType<C>`, as for
    /// [`Lens::detach_as`]): a same-named element of another type (e.g.
    /// `w:pPr/w:rPr` vs `w:r/w:rPr`) would not fit `C`'s slot. Refusals: those
    /// of [`Self::check_move_in_by`], all before any change.
    ///
    /// ```compile_fail
    /// # use ooxml_core::*;
    /// # macro_rules! spec { ($t:ident, $l:literal, $c:expr) => {
    /// #     enum $t {}
    /// #     impl ElementSpec for $t { const QNAME: QName = QName::new("urn:t", $l); const SDK_NAME: &'static str = $l;
    /// #         const ATTRS: &'static [AttrSpec] = &[]; const CONTENT: Content = $c; } } }
    /// # const ITEMS: &[Slot] = &[Slot { names: &[QName::new("urn:t", "rPr")], wildcard: None, max_each: None, max_total: None }];
    /// spec!(Box_, "box", Content::Slots(ITEMS));
    /// spec!(RunRPr, "rPr", Content::Empty);   // CT_RPr
    /// spec!(ParaRPr, "rPr", Content::Empty);  // CT_ParaRPr: same QName, another type
    /// impl ChildOf<Box_> for RunRPr {}
    /// let mut d = XmlDocument::parse(br#"<box xmlns="urn:t"><rPr/></box>"#).unwrap();
    /// let from = Lens::<ParaRPr>::at(d.root(), &[0]).unwrap().move_source();
    /// d.lens_mut::<Box_>().unwrap().move_in::<RunRPr>(&from); // ParaRPr is not SameType<RunRPr>
    /// ```
    ///
    /// Dirty duties: the source parent and its ancestors get
    /// `subtree_dirty` (a parent with a close tag keeps its tags even when
    /// emptied); the destination parent and its ancestors get
    /// `subtree_dirty`, and `own_dirty` if it was self-closing and gains its
    /// first child; the moved subtree keeps its spans and flags, so it is
    /// written byte-for-byte.
    ///
    /// **Paths after a move.** Removing the element shifts, by one, the
    /// index of every later sibling in the source parent, and so the paths
    /// of those siblings and their descendants; inserting it shifts the
    /// destination parent's children from the insertion index on, likewise.
    /// Other paths are unchanged. A [`MoveSource`] taken before a move may
    /// therefore name another element afterwards (same name: not detected).
    /// To move several children of one parent, take the sources again
    /// after each move, or move them from the last to the first.
    pub fn move_in_by<C: ChildOf<E>>(
        &mut self,
        from: &MoveSource<impl SameType<C>>,
        before: impl FnMut(usize, &RawNode) -> bool,
    ) -> Result<LensMut<'_, C>> {
        let plan = self.plan_move::<C>(from.path(), before)?;
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
    pub fn move_in<C: ChildOf<E>>(&mut self, from: &MoveSource<impl SameType<C>>) -> Result<LensMut<'_, C>> {
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
        // Out of (or into) an mc:AlternateContent branch: consumers that
        // pick the other branch would see the element vanish or appear.
        for path in [src_parent, self.path.as_slice()] {
            let mut n: &RawNode = self.root;
            let mut inside = n.name.ns() == MC_NS;
            for &i in path {
                n = n.children[i].as_element().expect("validated path");
                inside |= n.name.ns() == MC_NS;
            }
            if inside {
                return Err(refused(MoveRefusal::AlternateContent, &n.name.display().to_string()));
            }
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
        if (self.path.len() + 1).saturating_add(subtree_height(node)) > self.max_depth {
            return Err(Error::TooDeep { limit: self.max_depth, position: self.node().span.map_or(0, |s| s.start) });
        }
        // meaning at the destination
        let usage = Usage::of(node);
        let (src_decls, dst_decls) = (declarations(self.root, src_parent), declarations(self.root, &self.path));
        // An unbound default namespace means "no namespace" (`""`).
        let binding = |decls: &[(usize, Option<&str>, &str)], p: Option<&str>| -> Option<String> {
            effective(decls, p).or(p.is_none().then_some("")).map(str::to_owned)
        };
        for p in &usage.free_prefixes {
            if binding(&src_decls, *p) != binding(&dst_decls, *p) {
                return Err(refused(MoveRefusal::NamespaceBinding, p.unwrap_or("(default namespace)")));
            }
        }
        let (src, dst) = (ancestor_context(self.root, src_parent, true), ancestor_context(self.root, &self.path, true));
        for u in &usage.uris {
            if src.ignorable.iter().any(|i| i == u) != dst.ignorable.iter().any(|i| i == u) {
                return Err(refused(MoveRefusal::IgnorableNamespace, u));
            }
        }
        if usage.inherits_text && src.preserve != dst.preserve {
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
