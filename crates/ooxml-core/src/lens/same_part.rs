//! Same-part copy of an element subtree (ADR-0002 post-acceptance note
//! "same-part copy"): [`Lens::detach_same_part_as`] and
//! [`LensMut::insert_same_part_by`], with the check twin
//! [`SamePartDetached::check_insert_under`].
//!
//! Unlike [`Lens::detach_as`], the copy is a **span-preserving clone**: its
//! bytes are written verbatim at the destination, namespace declarations
//! and prefixes included, so the output differs from the input only by
//! those bytes appearing a second time (T2-style locality). Because the
//! bytes do not change, the copy is validated the way a same-part move is
//! (PR #44): every prefix the copy uses without declaring it must mean the
//! same namespace at the destination as at the source, every namespace it
//! uses must have the same `mc:Ignorable` status, and text that inherits
//! `xml:space` must inherit the same value. Relationship ids stay valid
//! because the part — and so its `.rels` — is the same; the copy can be
//! inserted only into the document it was taken from.
//!
//! The context is read with the lens's own helpers (`declarations` /
//! `effective` for bindings, `ancestor_context` for `mc:Ignorable` and
//! `xml:space`) and the subtree's needs with the move's `Usage`, so a copy is
//! checked exactly like a move: including the refusal out of and into an
//! `mc:AlternateContent` branch, and treating whitespace-only text between
//! child elements as indentation.
//!
//! Known limits (shared with the move): an ancestor's `mc:ProcessContent` and
//! `mc:MustUnderstand` are not compared, and a QName inside an attribute
//! value outside Markup Compatibility is not checked.

use std::marker::PhantomData;

use super::move_subtree::Usage;
use super::{Lens, LensMut, Placement, ancestor_context, declarations, effective, element_at, not_detachable, subtree_height};
use crate::error::{DetachRefusal, Error, Result};
use crate::name::{MC_NS, QName, XMLNS_NS, XName};
use crate::schema::{ChildOf, Detachable, ElementSpec, SameType, UniqueAttr};
use crate::xml::{DocToken, RawItem, RawNode};

/// `a == b` in a const context (the rename check below).
const fn str_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// Extension URIs reviewed for same-part copies
/// ([`SamePartOptions::keep_extension`]).
pub mod ext_uri {
    /// `a:hlinkClick/a:extLst/a:ext` holding `ahyp:hlinkClr` (whether a
    /// hyperlink takes the theme's hyperlink colour): a boolean-like value,
    /// nothing document-unique. 14 in the 619-file pptx corpus (slides,
    /// layouts and masters; 12 on
    /// slides), each declaring `xmlns:ahyp` on the element itself; the only
    /// extension below `a:rPr` there.
    pub const HYPERLINK_COLOR: &str = "{A12FA001-AC4F-418D-AE19-62706E023703}";
}

/// Options of [`Lens::detach_same_part_as`].
#[derive(Clone, Debug, Default)]
pub struct SamePartOptions {
    extension_uris: Vec<String>,
}

impl SamePartOptions {
    /// Allows an `extLst` whose element children are all `ext`s (of the
    /// list's namespace) with one of the allowed `uri`s, compared ASCII
    /// case-insensitively. Extension content is open, so the caller asserts,
    /// per extension, that it holds nothing document-unique; the content
    /// still passes every other check. Reviewed: [`ext_uri::HYPERLINK_COLOR`].
    /// Without any allowed URI every `extLst` is refused.
    pub fn keep_extension(mut self, uri: &str) -> Self {
        self.extension_uris.push(uri.to_owned());
        self
    }
}

/// A copy from [`Lens::detach_same_part_as`]: a clone of the source subtree
/// with its spans, declarations and prefixes, plus what the source scope
/// meant for it. It can be inserted **only** into the document it was taken
/// from ([`LensMut::insert_same_part`]; otherwise [`Error::WrongDocument`]).
/// It is not a [`super::Detached`], so [`LensMut::insert_detached`] does
/// not accept it.
///
/// **In a transaction** (ADR-0012/ADR-0016: clone the parsed part, edit the
/// clone, `stage_part` it), take the copy **from the staged clone** and pass
/// that clone to [`Self::check_insert_under`]: the clone is another document
/// (it may become another part), so a copy taken from the package's own tree
/// is refused there with [`Error::WrongDocument`] (safe, but surprising).
///
/// Known limit: a QName inside an attribute **value** (`type="foo:T"`) is
/// copied verbatim and its prefix is not checked at the destination (only
/// element and attribute names are). Markup Compatibility values are covered
/// by the MC refusal; DrawingML formatting has no other QName values.
pub struct SamePartDetached<C: ElementSpec> {
    root: RawNode,
    doc: DocToken,
    height: usize,
    /// Each prefix the copy uses without declaring it (`None` = default
    /// namespace), with its binding at the source (`None` = unbound).
    free: Vec<(Option<String>, Option<String>)>,
    /// Each namespace the copy's names use, and whether it was ignorable at
    /// the source.
    uris: Vec<(String, bool)>,
    /// The copy has text whose whitespace handling it inherits.
    inherits_text: bool,
    /// The inherited `xml:space` at the source (`true` = preserve).
    preserve: bool,
    _c: PhantomData<fn() -> C>,
}

impl<C: ElementSpec> Clone for SamePartDetached<C> {
    fn clone(&self) -> Self {
        SamePartDetached {
            root: self.root.clone(),
            doc: self.doc,
            height: self.height,
            free: self.free.clone(),
            uris: self.uris.clone(),
            inherits_text: self.inherits_text,
            preserve: self.preserve,
            _c: PhantomData,
        }
    }
}

impl<C: ElementSpec> std::fmt::Debug for SamePartDetached<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SamePartDetached<{}>", C::SDK_NAME)
    }
}

/// What a same-part copy still refuses (ADR-0002 note "same-part copy"):
/// Markup Compatibility, a document-unique attribute with a non-default
/// value, and an `extLst` with an extension that is not allowed. Lifted
/// compared with `detach_as`: relationship ids, namespace declarations, and
/// (checked at the destination instead) ignorable namespaces and inherited
/// `xml:space`.
fn check_copyable(n: &RawNode, unique: &[UniqueAttr], ext_uris: &[String]) -> Result<()> {
    if n.name.ns() == MC_NS {
        let reason = if n.is(crate::name::MC_ALTERNATE_CONTENT) { DetachRefusal::AlternateContent } else { DetachRefusal::MarkupCompatibility };
        return Err(not_detachable(reason, &n.name));
    }
    if let Some(a) = n.attrs.iter().find(|a| a.name.ns() == MC_NS) {
        return Err(not_detachable(DetachRefusal::MarkupCompatibility, &a.name));
    }
    for u in unique.iter().filter(|u| n.is(u.element)) {
        if let Some(v) = n.attr(u.attr)
            && Some(v) != u.default
        {
            let a = n.attrs.iter().find(|a| a.name.is(u.attr)).expect("present");
            return Err(not_detachable(DetachRefusal::UniqueAttribute, &a.name));
        }
    }
    if n.name.local() == "extLst" && !extensions_allowed(n, ext_uris) {
        return Err(not_detachable(DetachRefusal::ExtensionList, &n.name));
    }
    n.elements().try_for_each(|c| check_copyable(c, unique, ext_uris))
}

/// An `extLst` may be copied only when some extension URI is allowed and
/// every element child is an `ext` of the list's namespace whose `uri` is
/// allowed.
fn extensions_allowed(ext_lst: &RawNode, uris: &[String]) -> bool {
    !uris.is_empty()
        && ext_lst.elements().all(|e| {
            e.name.local() == "ext"
                && e.name.ns() == ext_lst.name.ns()
                && e.attr(QName::new("", "uri")).is_some_and(|u| uris.iter().any(|a| a.eq_ignore_ascii_case(u)))
        })
}

impl<E: ElementSpec> Lens<'_, E> {
    /// A **same-part** copy of this element's subtree, renamed to
    /// `C::QNAME` when the types differ (ADR-0002 note "same-part copy"):
    /// a clone that keeps its spans, so it is written byte for byte —
    /// relationship ids (`r:id`, `r:embed`, …) and namespace declarations
    /// included — and can be inserted only into this document
    /// ([`LensMut::insert_same_part`]). Direct children named in `skip` are
    /// left out.
    ///
    /// Still refused ([`Error::NotDetachable`]): Markup Compatibility
    /// content; an attribute listed in [`Detachable::UNIQUE_ATTRS`] with a
    /// non-default value (a non-zero `smtId`; `a:effectDag`/`a:cont`
    /// `@name`); an `extLst` with an extension `opts` does not allow.
    /// Checked at the destination instead: prefixes, `mc:Ignorable` and
    /// inherited `xml:space` must mean there what they mean here.
    ///
    /// The lens must know its document ([`crate::XmlDocument::lens`],
    /// [`crate::XmlDocument::lens_at`], or a lens derived from them);
    /// [`Lens::root`] and [`Lens::at`] do not: [`Error::WrongDocument`].
    pub fn detach_same_part_as<C>(&self, skip: &[QName], opts: &SamePartOptions) -> Result<SamePartDetached<C>>
    where
        E: Detachable,
        C: SameType<E> + Detachable,
    {
        // A rename keeps the namespace (and so the prefix); `SameType` pairs
        // across namespaces are not supported (review P49R2-m1).
        const { assert!(str_eq(C::QNAME.ns, E::QNAME.ns), "a same-part copy cannot be renamed into another namespace") };
        let doc = self.doc.ok_or(Error::WrongDocument)?;
        let parent = &self.path[..self.path.len().saturating_sub(1)];
        // Out of an mc:AlternateContent branch: refused as a move is (#44),
        // since consumers that pick another branch would see the copy appear
        // without its branch's `Requires` (review P49R2-M1).
        if let Some(mc) = mc_on_path(self.root, parent) {
            return Err(not_detachable(DetachRefusal::AlternateContent, mc));
        }
        let unique: Vec<UniqueAttr> = E::UNIQUE_ATTRS.iter().chain(C::UNIQUE_ATTRS).copied().collect();
        if let Some(a) = self.node.attrs.iter().find(|a| a.name.ns() == MC_NS) {
            return Err(not_detachable(DetachRefusal::MarkupCompatibility, &a.name));
        }
        // the root's own attributes, then every kept child
        for u in unique.iter().filter(|u| self.node.is(u.element)) {
            if let Some(v) = self.node.attr(u.attr)
                && Some(v) != u.default
            {
                let a = self.node.attrs.iter().find(|a| a.name.is(u.attr)).expect("present");
                return Err(not_detachable(DetachRefusal::UniqueAttribute, &a.name));
            }
        }
        self.node
            .elements()
            .filter(|c| !skip.iter().any(|q| c.is(*q)))
            .try_for_each(|c| check_copyable(c, &unique, &opts.extension_uris))?;

        let mut root = self.node.clone();
        let before = root.children.len();
        root.children.retain(|c| !c.as_element().is_some_and(|e| skip.iter().any(|q| e.is(*q))));
        if root.children.len() != before {
            root.subtree_dirty = true; // children change: re-render them
        }
        if !self.node.is(C::QNAME) {
            root.name = XName::new(C::QNAME.ns.into(), C::QNAME.local, self.node.name.prefix());
            root.own_dirty = true; // renamed: re-render its tags
            root.subtree_dirty = true;
        }

        // What the copy needs, and what it meant at the source (the scope of
        // its parent; none for a document root).
        let usage = Usage::of(&root);
        let (decls, ctx) = if self.path.is_empty() {
            (Vec::new(), ancestor_context(self.root, &[], false))
        } else {
            (declarations(self.root, parent), ancestor_context(self.root, parent, true))
        };
        let free = usage.free_prefixes.iter().map(|p| (p.map(str::to_owned), binding(&decls, *p))).collect();
        let uris = usage.uris.iter().map(|u| ((*u).to_owned(), ctx.ignorable.iter().any(|i| i == u))).collect();
        let inherits_text = usage.inherits_text;
        let height = subtree_height(&root);
        Ok(SamePartDetached {
            height,
            root,
            doc,
            free,
            uris,
            inherits_text,
            preserve: ctx.preserve,
            _c: PhantomData,
        })
    }
}

/// The binding of `prefix` in `decls`; an unbound default namespace means
/// "no namespace" (`""`), as for a move.
fn binding(decls: &[(usize, Option<&str>, &str)], prefix: Option<&str>) -> Option<String> {
    effective(decls, prefix).or(prefix.is_none().then_some("")).map(str::to_owned)
}

/// The first Markup Compatibility element on the path from `root` (included)
/// to the element at `path` (included), if any.
fn mc_on_path<'r>(root: &'r RawNode, path: &[usize]) -> Option<&'r XName> {
    let mut n = root;
    if n.name.ns() == MC_NS {
        return Some(&n.name);
    }
    for &i in path {
        n = n.children[i].as_element().expect("validated path");
        if n.name.ns() == MC_NS {
            return Some(&n.name);
        }
    }
    None
}

impl<C: ElementSpec> SamePartDetached<C> {
    /// The copied subtree (with its source spans and prefixes).
    pub fn raw(&self) -> &RawNode {
        &self.root
    }

    /// Number of element levels of the copy.
    pub fn height(&self) -> usize {
        self.height
    }

    /// Would [`LensMut::insert_same_part`] succeed, as far as the document,
    /// depth and the meaning of the copy's prefixes, ignorable namespaces
    /// and `xml:space` go, when the copy is inserted below the element at
    /// `parent_path` of `doc`, with `created` new elements in between (they
    /// declare nothing)? Changes nothing. Returns the error the insertion
    /// would return for those reasons ([`Error::WrongDocument`] first, then
    /// `TooDeep`, `NotDetachable { AlternateContent | NamespaceBinding |
    /// IgnorableNamespace | InheritedXmlSpace }`), or `WrongElement` for a
    /// path that does not lead to an element. Slot rules (`AlreadyPresent`,
    /// `NotAllowed`, …) are **not** checked, as with
    /// [`super::Detached::check_insert_under`].
    pub fn check_insert_under(&self, doc: &crate::XmlDocument, parent_path: &[usize], created: usize) -> Result<()> {
        if doc.token() != self.doc {
            return Err(Error::WrongDocument);
        }
        let root = doc.root();
        let parent = element_at(root, parent_path, QName::new("", "(parent element at parent_path)"))?;
        if parent_path.len().saturating_add(1).saturating_add(created).saturating_add(self.height) > doc.max_depth() {
            return Err(Error::TooDeep { limit: doc.max_depth(), position: parent.span.map_or(0, |s| s.start) });
        }
        self.check_destination(root, parent_path)
    }

    /// The copy means at the destination what it meant at the source.
    fn check_destination(&self, root: &RawNode, parent_path: &[usize]) -> Result<()> {
        // into an mc:AlternateContent branch: refused as a move is (#44)
        if let Some(mc) = mc_on_path(root, parent_path) {
            return Err(not_detachable(DetachRefusal::AlternateContent, mc));
        }
        let decls = declarations(root, parent_path);
        let dst = ancestor_context(root, parent_path, true);
        for (p, u) in &self.free {
            if binding(&decls, p.as_deref()) != *u {
                let name = XName::new(XMLNS_NS.into(), p.as_deref().unwrap_or("xmlns"), None);
                return Err(not_detachable(DetachRefusal::NamespaceBinding, &name));
            }
        }
        for (u, ignorable) in &self.uris {
            if dst.ignorable.iter().any(|i| i == u) != *ignorable {
                return Err(not_detachable(DetachRefusal::IgnorableNamespace, &XName::new(u.as_str().into(), "*", None)));
            }
        }
        if self.inherits_text && dst.preserve != self.preserve {
            return Err(not_detachable(DetachRefusal::InheritedXmlSpace, &self.root.name));
        }
        Ok(())
    }
}

impl<E: ElementSpec> LensMut<'_, E> {
    /// Inserts a [`SamePartDetached`] copy in `C`'s slot, placed exactly as
    /// [`Self::insert_by`] places a new `C` (same slot rules, limits and
    /// refusals), after checking that this lens is on the document the copy
    /// was taken from ([`Error::WrongDocument`] otherwise; a lens from
    /// [`LensMut::root`] knows no document), the depth limit (`TooDeep`,
    /// counted at the copy's deepest element), that the destination is not
    /// inside an `mc:AlternateContent` branch (`NotDetachable {
    /// AlternateContent }`), and that the copy's prefixes, ignorable
    /// namespaces and inherited `xml:space` mean here what they meant at the
    /// source (`NotDetachable { NamespaceBinding | IgnorableNamespace |
    /// InheritedXmlSpace }`). All before any change.
    /// The copy keeps its spans and is written byte for byte.
    pub fn insert_same_part_by<C: ChildOf<E>>(
        &mut self,
        d: &SamePartDetached<C>,
        before: impl FnMut(usize, &RawNode) -> bool,
    ) -> Result<LensMut<'_, C>> {
        if self.doc != Some(d.doc) {
            return Err(Error::WrongDocument);
        }
        let slots = self.slots()?;
        let slot = match Self::slot_of(slots, C::QNAME)[..] {
            [s] => s,
            [] => return Err(Error::NotAllowed { parent: E::QNAME, child: C::QNAME }),
            _ => return Err(Error::AmbiguousSlot(C::QNAME)),
        };
        if self.path.len() + 1 + d.height > self.max_depth {
            return Err(Error::TooDeep { limit: self.max_depth, position: self.node().span.map_or(0, |s| s.start) });
        }
        let Placement { index } = self.place(slots, slot, C::QNAME, before)?;
        d.check_destination(self.root, &self.path)?;
        let node = d.root.clone();
        let parent = self.node_mut();
        if parent.span.is_some_and(|s| s.close.is_none()) && parent.children.is_empty() {
            // A self-closing parent gains its first child. The writer already
            // re-renders such a parent from `subtree_dirty`, so the output does
            // not depend on this flag; it is set for the dirty duties
            // `insert_by` and the move document (`own_dirty` = the element's
            // own tags change), which callers may inspect (review n5).
            parent.own_dirty = true;
        }
        parent.children.insert(index, RawItem::Element(node));
        self.mark_path();
        Ok(self.descend(index))
    }

    /// `insert_same_part_by` that appends at the end of the slot.
    pub fn insert_same_part<C: ChildOf<E>>(&mut self, d: &SamePartDetached<C>) -> Result<LensMut<'_, C>> {
        self.insert_same_part_by(d, |_, _| false)
    }
}
