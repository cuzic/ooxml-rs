//! Hand-written runtime: QName, specs, a toy RawNode, slot-based insertion.
use std::cmp::Ordering;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QName { pub ns: &'static str, pub local: &'static str }

pub const MC_NS: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";

#[derive(Clone, Copy, Debug)]
pub enum SimpleType { Str, U32, Bool, Enum(&'static [&'static str]) }

#[derive(Clone, Copy, Debug)]
pub struct AttrSpec { pub name: QName, pub ty: SimpleType, pub default: Option<&'static str> }

#[derive(Clone, Copy, Debug)]
pub struct Item { pub name: QName, pub max: Option<u32> } // None = unbounded

#[derive(Clone, Copy, Debug)]
pub enum Content { Empty, Text, Sequence(&'static [Item]), Unsupported }

pub trait ElementSpec: 'static {
    const QNAME: QName;
    const ATTRS: &'static [AttrSpec];
    const CONTENT: Content;
}

#[diagnostic::on_unimplemented(
    message = "`{Self}` is not an allowed child of `{P}`",
    note = "allowed children are listed in `{P}::CONTENT`")]
pub trait ChildOf<P: ElementSpec>: ElementSpec {}

// ---- const helpers usable from generated `const _` items ----
pub const fn bytes_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() { return false; }
    let mut i = 0;
    while i < a.len() { if a[i] != b[i] { return false; } i += 1; }
    true
}
pub const fn qn_eq(a: QName, b: QName) -> bool { bytes_eq(a.ns, b.ns) && bytes_eq(a.local, b.local) }
pub const fn content_contains(c: Content, q: QName) -> bool {
    match c {
        Content::Sequence(items) => {
            let mut i = 0;
            while i < items.len() { if qn_eq(items[i].name, q) { return true; } i += 1; }
            false
        }
        _ => false,
    }
}
pub const fn no_duplicate_items(c: Content) -> bool {
    if let Content::Sequence(items) = c {
        let mut i = 0;
        while i < items.len() {
            let mut j = i + 1;
            while j < items.len() { if qn_eq(items[i].name, items[j].name) { return false; } j += 1; }
            i += 1;
        }
    }
    true
}
pub const fn default_is_valid(a: AttrSpec) -> bool {
    let Some(d) = a.default else { return true };
    match a.ty {
        SimpleType::Str => true,
        SimpleType::U32 => u32::from_str_radix(d, 10).is_ok(),
        SimpleType::Bool => bytes_eq(d, "true") || bytes_eq(d, "false") || bytes_eq(d, "1") || bytes_eq(d, "0"),
        SimpleType::Enum(vals) => {
            let mut i = 0;
            while i < vals.len() { if bytes_eq(vals[i], d) { return true; } i += 1; }
            false
        }
    }
}

// ---- toy raw tree (stands in for ADR-0001 RawNode) ----
#[derive(Debug, Clone)]
pub enum RawItem { Elem(RawNode), Text(String) }
#[derive(Debug, Clone)]
pub struct RawNode {
    pub name: QName,
    pub attrs: Vec<(QName, String)>,
    pub children: Vec<RawItem>,
    pub own_dirty: bool,
    pub subtree_dirty: bool,
}
impl RawNode {
    pub fn new(name: QName) -> Self { Self { name, attrs: vec![], children: vec![], own_dirty: true, subtree_dirty: true } }
    pub fn attr(&self, q: QName) -> Option<&str> { self.attrs.iter().find(|(n, _)| *n == q).map(|(_, v)| v.as_str()) }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Error { AlreadyPresent, UnsupportedContentModel, UnresolvedSibling, NotAllowed }

/// Read-only typed view.
pub struct Lens<'a, E: ElementSpec> { pub node: &'a RawNode, _e: std::marker::PhantomData<E> }
impl<'a, E: ElementSpec> Lens<'a, E> {
    pub fn new(node: &'a RawNode) -> Self { debug_assert_eq!(node.name, E::QNAME); Self { node, _e: Default::default() } }
    pub fn attr_str(&self, local: &'static str) -> Option<&'a str> {
        let spec = E::ATTRS.iter().find(|a| a.name.local == local).expect("attr not in spec");
        self.node.attr(spec.name).or(spec.default)
    }
    pub fn children<C: ChildOf<E>>(&self) -> impl Iterator<Item = Lens<'a, C>> + use<'a, E, C> {
        self.node.children.iter().filter_map(|c| match c { RawItem::Elem(n) if n.name == C::QNAME => Some(Lens::new(n)), _ => None })
    }
}

/// Mutable typed view. Dirty propagation to ancestors is the caller's (path) job in the real design;
/// here we just mark self.
pub struct LensMut<'a, E: ElementSpec> { pub node: &'a mut RawNode, _e: std::marker::PhantomData<E> }
impl<'a, E: ElementSpec> LensMut<'a, E> {
    pub fn new(node: &'a mut RawNode) -> Self { Self { node, _e: Default::default() } }

    fn rank_of(items: &[Item], q: QName) -> Option<usize> { items.iter().position(|i| i.name == q) }

    /// Slot-based insertion: schema decides the band; `cmp` orders within the band
    /// (`cmp(existing) == Greater` means existing sorts after the new node).
    pub fn insert_by<C: ChildOf<E>>(&mut self, cmp: impl Fn(&RawNode) -> Ordering) -> Result<&mut RawNode, Error> {
        let Content::Sequence(items) = E::CONTENT else { return Err(Error::UnsupportedContentModel) };
        let my = Self::rank_of(items, C::QNAME).ok_or(Error::NotAllowed)?;
        let max = items[my].max;
        let mut count = 0u32;
        let mut pos = None; // index of first child that must come after the new node
        for (i, ch) in self.node.children.iter().enumerate() {
            let RawItem::Elem(n) = ch else { continue };
            if n.name.ns == MC_NS { return Err(Error::UnresolvedSibling) } // M1 safe rule
            let Some(r) = Self::rank_of(items, n.name) else { continue }; // unlisted: attached to predecessor
            if r == my { count += 1; }
            if pos.is_none() && (r > my || (r == my && cmp(n) == Ordering::Greater)) { pos = Some(i); }
        }
        if let Some(m) = max && count >= m { return Err(Error::AlreadyPresent) }
        let at = pos.unwrap_or(self.node.children.len());
        self.node.own_dirty |= self.node.children.is_empty(); // self-closing gains first child
        self.node.subtree_dirty = true;
        let RawItem::Elem(n) = self.node.children.insert_mut(at, RawItem::Elem(RawNode::new(C::QNAME))) else { unreachable!() };
        Ok(n)
    }
    pub fn insert<C: ChildOf<E>>(&mut self) -> Result<&mut RawNode, Error> { self.insert_by::<C>(|_| Ordering::Less) }
}
