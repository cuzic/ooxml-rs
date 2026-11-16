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
//! The ancestor scan ([`Scope`]) computes what PR #44's move computes; the
//! two are to be folded into one helper once both are merged.

use std::collections::BTreeSet;
use std::marker::PhantomData;

use super::{Lens, LensMut, Placement, element_at, not_detachable};
use crate::error::{DetachRefusal, Error, Result};
use crate::name::{MC_NS, QName, XML_NS, XMLNS_NS, XName};
use crate::schema::{ChildOf, Detachable, ElementSpec, SameType, UniqueAttr};
use crate::xml::{DocToken, RawItem, RawNode};

const MC_IGNORABLE: QName = QName::new(MC_NS, "Ignorable");

/// Extension URIs reviewed for same-part copies
/// ([`SamePartOptions::keep_extension`]).
pub mod ext_uri {
    /// `a:hlinkClick/a:extLst/a:ext` holding `ahyp:hlinkClr` (whether a
    /// hyperlink takes the theme's hyperlink colour): a boolean-like value,
    /// nothing document-unique. 14 in the 619-file pptx corpus (12 on
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
    /// The root was renamed to another namespace: its prefix is chosen at
    /// the destination.
    root_prefix_at_destination: bool,
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
            root_prefix_at_destination: self.root_prefix_at_destination,
            _c: PhantomData,
        }
    }
}

impl<C: ElementSpec> std::fmt::Debug for SamePartDetached<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SamePartDetached<{}>", C::SDK_NAME)
    }
}

/// Namespace bindings, `mc:Ignorable` namespaces and the `xml:space` in
/// effect at (and including) the element at `path` (validated).
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

/// What the copy uses: prefixes it does not declare itself (on the element
/// or an ancestor within the copy), namespace URIs of its names, and
/// whether it has text without an own `xml:space` above it.
#[derive(Default)]
struct Usage<'n> {
    free: BTreeSet<Option<&'n str>>,
    uris: BTreeSet<&'n str>,
    inherits_text: bool,
}

fn usage<'n>(n: &'n RawNode, declared: &[Option<&'n str>], own_space: bool, u: &mut Usage<'n>) {
    let mut declared = declared.to_vec();
    declared.extend(n.attrs.iter().filter(|a| a.name.ns() == XMLNS_NS).map(|a| a.name.declared_prefix()));
    let own_space = own_space || n.xml_space_preserve().is_some();
    if n.name.ns() != XML_NS {
        if !declared.contains(&n.name.prefix()) {
            u.free.insert(n.name.prefix());
        }
        u.uris.insert(n.name.ns());
    }
    for a in &n.attrs {
        if a.name.ns() == XMLNS_NS || a.name.ns() == XML_NS || a.name.ns().is_empty() {
            continue;
        }
        if !declared.contains(&a.name.prefix()) {
            u.free.insert(a.name.prefix());
        }
        u.uris.insert(a.name.ns());
    }
    for c in &n.children {
        match c {
            RawItem::Element(e) => usage(e, &declared, own_space, u),
            RawItem::Text(t) if !own_space && !t.value.is_empty() => u.inherits_text = true,
            _ => {}
        }
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

fn height(n: &RawNode) -> usize {
    1 + n.elements().map(height).max().unwrap_or(0)
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
        let doc = self.doc.ok_or(Error::WrongDocument)?;
        let unique: Vec<UniqueAttr> = E::UNIQUE_ATTRS.iter().chain(C::UNIQUE_ATTRS).copied().collect();
        let kept = || self.node.elements().filter(|c| !skip.iter().any(|q| c.is(*q)));
        if let Some(a) = self.node.attrs.iter().find(|a| a.name.ns() == MC_NS) {
            return Err(not_detachable(DetachRefusal::MarkupCompatibility, &a.name));
        }
        // the root itself (its own attributes) and every kept child
        for u in unique.iter().filter(|u| self.node.is(u.element)) {
            if let Some(v) = self.node.attr(u.attr)
                && Some(v) != u.default
            {
                let a = self.node.attrs.iter().find(|a| a.name.is(u.attr)).expect("present");
                return Err(not_detachable(DetachRefusal::UniqueAttribute, &a.name));
            }
        }
        if self.node.name.local() == "extLst" && !extensions_allowed(self.node, &opts.extension_uris) {
            return Err(not_detachable(DetachRefusal::ExtensionList, &self.node.name));
        }
        kept().try_for_each(|c| check_copyable(c, &unique, &opts.extension_uris))?;

        let mut root = self.node.clone();
        let before = root.children.len();
        root.children.retain(|c| !c.as_element().is_some_and(|e| skip.iter().any(|q| e.is(*q))));
        if root.children.len() != before {
            root.subtree_dirty = true; // children change: re-render them
        }
        let mut root_prefix_at_destination = false;
        if !self.node.is(C::QNAME) {
            let same_ns = C::QNAME.ns == self.node.name.ns();
            let prefix = if same_ns { self.node.name.prefix() } else { None };
            root.name = XName::new(C::QNAME.ns.into(), C::QNAME.local, prefix);
            root.own_dirty = true; // renamed: re-render its tags
            root.subtree_dirty = true;
            root_prefix_at_destination = !same_ns;
        }

        let parent = &self.path[..self.path.len().saturating_sub(1)];
        let src = if self.path.is_empty() { Scope { decls: Vec::new(), ignorable: Vec::new(), preserve: false } } else { Scope::at(self.root, parent) };
        let mut u = Usage::default();
        usage(&root, &[], false, &mut u);
        if root_prefix_at_destination {
            // the renamed root's prefix is chosen at the destination
            u.free.remove(&root.name.prefix());
        }
        let free = u.free.iter().map(|p| (p.map(str::to_owned), src.binding(*p).map(str::to_owned))).collect();
        let uris = u.uris.iter().map(|x| ((*x).to_owned(), src.ignorable.contains(x))).collect();
        let inherits_text = u.inherits_text;
        let height = height(&root);
        Ok(SamePartDetached {
            height,
            root,
            doc,
            free,
            uris,
            inherits_text,
            preserve: src.preserve,
            root_prefix_at_destination,
            _c: PhantomData,
        })
    }
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
    /// `TooDeep`, `NotDetachable { NamespaceBinding | IgnorableNamespace |
    /// InheritedXmlSpace }`), or `WrongElement` for a path that does not lead
    /// to an element; slot rules are not checked.
    pub fn check_insert_under(&self, doc: &crate::XmlDocument, parent_path: &[usize], created: usize) -> Result<()> {
        if doc.token() != self.doc {
            return Err(Error::WrongDocument);
        }
        let root = doc.root();
        let parent = element_at(root, parent_path, QName::new("", "(parent element at parent_path)"))?;
        if parent_path.len().saturating_add(1).saturating_add(created).saturating_add(self.height) > doc.max_depth() {
            return Err(Error::TooDeep { limit: doc.max_depth(), position: parent.span.map_or(0, |s| s.start) });
        }
        self.check_destination(root, parent_path)?;
        if self.root_prefix_at_destination {
            super::choose_element_prefix(root, parent_path, self.root.name.ns())?;
        }
        Ok(())
    }

    /// The copy means at the destination what it meant at the source.
    fn check_destination(&self, root: &RawNode, parent_path: &[usize]) -> Result<()> {
        let dst = Scope::at(root, parent_path);
        for (p, u) in &self.free {
            if dst.binding(p.as_deref()) != u.as_deref() {
                let name = XName::new(XMLNS_NS.into(), p.as_deref().unwrap_or("xmlns"), None);
                return Err(not_detachable(DetachRefusal::NamespaceBinding, &name));
            }
        }
        for (u, ignorable) in &self.uris {
            if dst.ignorable.contains(&u.as_str()) != *ignorable {
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
    /// counted at the copy's deepest element), and that the copy's prefixes,
    /// ignorable namespaces and inherited `xml:space` mean here what they
    /// meant at the source (`NotDetachable { NamespaceBinding |
    /// IgnorableNamespace | InheritedXmlSpace }`). All before any change.
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
        let mut node = d.root.clone();
        if d.root_prefix_at_destination {
            let prefix = super::choose_element_prefix(self.root, &self.path, node.name.ns())?;
            node.name = XName::new(node.name.ns().into(), node.name.local(), prefix.as_deref());
        }
        let parent = self.node_mut();
        if parent.span.is_some_and(|s| s.close.is_none()) && parent.children.is_empty() {
            parent.own_dirty = true; // self-closing parent gains its first child
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
