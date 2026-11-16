//! Hand-written runtime stand-in for `ooxml-core` (ADR-0002 revision 3).
use std::marker::PhantomData;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QName { pub ns: &'static str, pub local: &'static str }

/// Namespaces the core itself must know, independent of `ooxml-schema` features.
pub const MC_NS: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";
pub const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";

#[derive(Clone, Copy, Debug)]
pub enum SimpleType { Str, U32, Bool, Enum(&'static [&'static str]) }
#[derive(Clone, Copy, Debug)]
pub struct AttrSpec { pub name: QName, pub ty: SimpleType, pub default: Option<&'static str> }

/// One schema rank. `max_each`: per-QName limit; `max_total`: whole-slot limit. None = unbounded.
#[derive(Clone, Copy, Debug)]
pub struct Slot { pub names: &'static [QName], pub max_each: Option<u32>, pub max_total: Option<u32> }

#[derive(Clone, Copy, Debug)]
pub enum Content {
    Empty,
    Text,
    Slots(&'static [Slot]),
    /// Not lowerable: reads only. Carries every QName the particle allows (for `ChildOf` checks).
    Unsupported(&'static [QName]),
}

pub trait ElementSpec: 'static {
    const QNAME: QName;
    const ATTRS: &'static [AttrSpec];
    const CONTENT: Content;
}

#[diagnostic::on_unimplemented(
    message = "`{Self}` is not an allowed child of `{P}`",
    note = "allowed children are listed in `{P}::CONTENT`")]
pub trait ChildOf<P: ElementSpec>: ElementSpec {}

// ---- const helpers for generated `const _` items ----
pub const fn bytes_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() { return false; }
    let mut i = 0;
    while i < a.len() { if a[i] != b[i] { return false; } i += 1; }
    true
}
pub const fn qn_eq(a: QName, b: QName) -> bool { bytes_eq(a.ns, b.ns) && bytes_eq(a.local, b.local) }
const fn in_list(l: &[QName], q: QName) -> bool {
    let mut i = 0;
    while i < l.len() { if qn_eq(l[i], q) { return true; } i += 1; }
    false
}
pub const fn content_contains(c: Content, q: QName) -> bool {
    match c {
        Content::Slots(s) => { let mut i = 0; while i < s.len() { if in_list(s[i].names, q) { return true; } i += 1; } false }
        Content::Unsupported(l) => in_list(l, q),
        _ => false,
    }
}
pub const fn no_duplicate_names(c: Content) -> bool {
    let Content::Slots(s) = c else { return true };
    let mut i = 0;
    while i < s.len() {
        let mut k = 0;
        while k < s[i].names.len() {
            let q = s[i].names[k];
            let mut j = 0;
            while j < s.len() {
                let mut m = 0;
                while m < s[j].names.len() {
                    if (i != j || k != m) && qn_eq(q, s[j].names[m]) { return false; }
                    m += 1;
                }
                j += 1;
            }
            k += 1;
        }
        i += 1;
    }
    true
}
pub const fn default_is_valid(a: AttrSpec) -> bool {
    let Some(d) = a.default else { return true };
    match a.ty {
        SimpleType::Str => true,
        SimpleType::U32 => u32::from_str_radix(d, 10).is_ok(),
        SimpleType::Bool => bytes_eq(d, "true") || bytes_eq(d, "false") || bytes_eq(d, "1") || bytes_eq(d, "0"),
        SimpleType::Enum(vals) => { let mut i = 0; while i < vals.len() { if bytes_eq(vals[i], d) { return true; } i += 1; } false }
    }
}

// ---- toy raw tree: ADR-0001 nested RawNode shape (no parent links) ----
#[derive(Debug, Clone)]
pub enum RawItem { Elem(RawNode), Text(String) }
#[derive(Debug, Clone)]
pub struct RawNode {
    pub name: QName,
    pub attrs: Vec<(QName, String)>,
    pub children: Vec<RawItem>,
    /// Stand-in for `span.close == None` in ADR-0001: element was self-closing in the source.
    pub was_self_closing: bool,
    pub own_dirty: bool,
    pub subtree_dirty: bool,
}
impl RawNode {
    pub fn parsed(name: QName, self_closing: bool) -> Self {
        Self { name, attrs: vec![], children: vec![], was_self_closing: self_closing, own_dirty: false, subtree_dirty: false }
    }
    fn created(name: QName) -> Self { Self { own_dirty: true, subtree_dirty: true, ..Self::parsed(name, true) } }
    pub fn attr(&self, q: QName) -> Option<&str> { self.attrs.iter().find(|(n, _)| *n == q).map(|(_, v)| v.as_str()) }
    fn child(&self, i: usize) -> &RawNode { match &self.children[i] { RawItem::Elem(n) => n, _ => unreachable!() } }
    fn child_mut(&mut self, i: usize) -> &mut RawNode { match &mut self.children[i] { RawItem::Elem(n) => n, _ => unreachable!() } }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Error { AlreadyPresent, UnsupportedContentModel, UnresolvedSibling, NotAllowed }
#[derive(Debug, PartialEq, Eq)]
pub struct AttrError { pub raw: String }

/// Read-only view.
pub struct Lens<'a, E: ElementSpec> { node: &'a RawNode, _e: PhantomData<E> }
impl<'a, E: ElementSpec> Lens<'a, E> {
    pub fn new(node: &'a RawNode) -> Self { debug_assert_eq!(node.name, E::QNAME); Self { node, _e: PhantomData } }
    pub fn raw_attr(&self, local: &'static str) -> Option<&'a str> {
        let spec = E::ATTRS.iter().find(|a| a.name.local == local).expect("attr not in spec");
        self.node.attr(spec.name).or(spec.default)
    }
    /// Typed read; a value outside the type's lexical space is returned raw, never dropped.
    pub fn attr_or_default<T: std::str::FromStr>(&self, local: &'static str) -> Result<Option<T>, AttrError> {
        match self.raw_attr(local) { None => Ok(None), Some(v) => v.parse().map(Some).map_err(|_| AttrError { raw: v.into() }) }
    }
}

impl<E: ElementSpec> std::fmt::Debug for LensMut<'_, E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "LensMut<{}>{:?}", E::QNAME.local, self.path) }
}

/// Mutable view: tree root + path of child indices (node id per ADR-0002 §4.2).
pub struct LensMut<'a, E: ElementSpec> { root: &'a mut RawNode, path: Vec<usize>, _e: PhantomData<E> }
impl<'a, E: ElementSpec> LensMut<'a, E> {
    pub fn root(root: &'a mut RawNode) -> Self { Self { root, path: vec![], _e: PhantomData } }
    pub fn node(&self) -> &RawNode { let mut n = &*self.root; for &i in &self.path { n = n.child(i); } n }
    fn node_mut(&mut self) -> &mut RawNode { let mut n = &mut *self.root; for &i in &self.path { n = n.child_mut(i); } n }
    /// Set `subtree_dirty` on self and every ancestor (ADR-0001 Layer 2 rule).
    fn mark_path(&mut self) {
        let mut n = &mut *self.root; n.subtree_dirty = true;
        for &i in &self.path { n = n.child_mut(i); n.subtree_dirty = true; }
    }
    fn descend<C: ElementSpec>(&mut self, i: usize) -> LensMut<'_, C> {
        let mut path = self.path.clone(); path.push(i);
        LensMut { root: &mut *self.root, path, _e: PhantomData }
    }

    /// Insert `C` into its slot. `pos(index_in_slot, existing)` returns true for the first
    /// same-slot sibling the new node must precede.
    pub fn insert_by<C: ChildOf<E>>(&mut self, mut pos: impl FnMut(usize, &RawNode) -> bool) -> Result<LensMut<'_, C>, Error> {
        let Content::Slots(slots) = E::CONTENT else { return Err(Error::UnsupportedContentModel) };
        let my = slots.iter().position(|s| s.names.contains(&C::QNAME)).ok_or(Error::NotAllowed)?;
        let (mut same_q, mut same_slot, mut at, mut k) = (0u32, 0u32, None, 0usize);
        let node = self.node();
        for (i, ch) in node.children.iter().enumerate() {
            let RawItem::Elem(n) = ch else { continue };
            if n.name.ns == MC_NS && n.name.local == "AlternateContent" { return Err(Error::UnresolvedSibling) }
            let Some(r) = slots.iter().position(|s| s.names.contains(&n.name)) else { continue }; // unlisted
            if r == my {
                same_slot += 1; same_q += (n.name == C::QNAME) as u32;
                if at.is_none() && pos(k, n) { at = Some(i); }
                k += 1;
            } else if r > my && at.is_none() { at = Some(i); }
        }
        let s = slots[my];
        if s.max_each.is_some_and(|m| same_q >= m) || s.max_total.is_some_and(|m| same_slot >= m) { return Err(Error::AlreadyPresent) }
        let at = at.unwrap_or(node.children.len());
        let n = self.node_mut();
        if n.was_self_closing && n.children.is_empty() { n.own_dirty = true; } // gains first child
        n.children.insert(at, RawItem::Elem(RawNode::created(C::QNAME)));
        self.mark_path();
        let mut child = self.descend::<C>(at);
        child.mark_path();
        Ok(child)
    }
    pub fn insert<C: ChildOf<E>>(&mut self) -> Result<LensMut<'_, C>, Error> { self.insert_by::<C>(|_, _| false) }
    pub fn get_or_insert<C: ChildOf<E>>(&mut self) -> Result<LensMut<'_, C>, Error> {
        match self.node().children.iter().position(|c| matches!(c, RawItem::Elem(n) if n.name == C::QNAME)) {
            Some(i) => Ok(self.descend(i)),
            None => self.insert::<C>(),
        }
    }
    pub fn set_attr(&mut self, q: QName, v: &str) {
        let n = self.node_mut();
        match n.attrs.iter_mut().find(|(k, _)| *k == q) { Some(a) => a.1 = v.into(), None => n.attrs.push((q, v.into())) }
        n.own_dirty = true; self.mark_path();
    }
}
