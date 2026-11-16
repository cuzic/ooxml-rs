//! Prototype of ADR-0010's compact Layer 2 tree.
//!
//! Same observable model as `ooxml_core::XmlDocument` (names resolved with
//! their original prefix, attributes in order with `xmlns` declarations,
//! text merged across entity references, comments/PIs verbatim, element
//! spans, the same serialization rule), with three changes:
//!
//! 1. **Interned names.** A [`Name`] is one thin `Arc` to `(ns, local,
//!    prefix)`, interned per parse: no allocation per element or attribute.
//! 2. **Span-backed values.** Attribute values, text and markup are
//!    [`SStr`]: an `Arc<str>` of the whole source plus a `u32` range. A value
//!    whose decoded form differs from its source bytes (entities, CR/LF/tab
//!    normalization) owns a decoded copy instead. Accessors still return
//!    `&str`, so the public `RawNode::attr(..) -> Option<&str>` style API
//!    survives unchanged.
//! 3. **Smaller nodes, exact-size vectors.** `u32` spans, attributes in a
//!    boxed slice, children collected in per-depth scratch vectors and moved
//!    into an exact-capacity `Vec` once the element closes.
//!
//! Not prototyped: mutation through lenses (it changes nothing in the
//! representation: a mutated value becomes an owned `SStr`, the dirty flags
//! are unchanged).

use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::Arc;

use quick_xml::XmlVersion;
use quick_xml::events::{BytesStart, Event};
use quick_xml::reader::Reader;

const XMLNS_NS: &str = "http://www.w3.org/2000/xmlns/";
const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";

/// A string that is either a range of the shared source or an owned
/// decoded copy (then `start == 0` and `len` covers the whole buffer).
#[derive(Clone)]
pub struct SStr {
    buf: Arc<str>,
    start: u32,
    len: u32,
}

impl SStr {
    fn span(buf: &Arc<str>, start: usize, end: usize) -> Self {
        SStr { buf: buf.clone(), start: start as u32, len: (end - start) as u32 }
    }
    pub fn new_owned(s: &str) -> Self {
        Self::owned(s)
    }
    fn owned(s: &str) -> Self {
        SStr { buf: Arc::from(s), start: 0, len: s.len() as u32 }
    }
    pub fn as_str(&self) -> &str {
        &self.buf[self.start as usize..(self.start + self.len) as usize]
    }
}

pub struct NameData {
    pub ns: Arc<str>,
    pub local: Box<str>,
    pub prefix: Option<Box<str>>,
}

/// An interned resolved name (thin pointer).
#[derive(Clone)]
pub struct Name(Arc<NameData>);

impl Name {
    pub fn ns(&self) -> &str {
        &self.0.ns
    }
    pub fn local(&self) -> &str {
        &self.0.local
    }
    pub fn prefix(&self) -> Option<&str> {
        self.0.prefix.as_deref()
    }
    fn display(&self) -> impl std::fmt::Display + '_ {
        std::fmt::from_fn(move |f| match self.prefix() {
            Some(p) => write!(f, "{p}:{}", self.local()),
            None => f.write_str(self.local()),
        })
    }
}

pub struct Attr {
    pub name: Name,
    pub value: SStr,
}

/// `u32` offsets; `close_start == u32::MAX` for a self-closing or created
/// element.
#[derive(Clone, Copy)]
pub struct Span {
    pub start: u32,
    pub open_end: u32,
    pub close_start: u32,
    pub close_end: u32,
}

impl Span {
    fn end(&self) -> u32 {
        if self.close_start == u32::MAX { self.open_end } else { self.close_end }
    }
}

pub struct Node {
    pub name: Name,
    pub attrs: Box<[Attr]>,
    pub children: Vec<Item>,
    pub span: Span,
    pub own_dirty: bool,
    pub subtree_dirty: bool,
}

pub struct Text {
    pub value: SStr,
    /// Source range; `start == u32::MAX` once forgotten (rerender).
    pub span: (u32, u32),
    pub cdata: bool,
}

pub enum Item {
    Element(Node),
    Text(Text),
    Markup(SStr),
}

pub struct CompactDoc {
    pub source: Arc<str>,
    pub root: Node,
}

/// Per-parse name interner. Keys borrow the source (the raw prefix and
/// local name) and identify the namespace by the address of its interned
/// URI, so a lookup neither formats nor allocates.
///
/// The keys are element and attribute names, i.e. **untrusted input**
/// (ADR-0010 §1, revision 4). Two layers:
///
/// - a **front cache**: 256 direct-mapped slots indexed by a cheap,
///   unkeyed mix of the name's length, first and last bytes, prefix length
///   and URI address. A hit is confirmed by comparing the key, so a crafted
///   name can only cause misses;
/// - on a miss, a `HashMap` keyed by std's `RandomState` (SipHash-1-3 with
///   per-instance keys from the OS). Revision 2's unkeyed FNV-1a let crafted
///   names collide and made parsing quadratic (18.8 MiB: 111 s).
///
/// Real parts repeat a few hundred names, so most lookups never reach
/// SipHash (review round 3: std + front cache is as fast as core's
/// interner, std alone ~19% slower). The hasher stays a type parameter so
/// the `hashdos` bench can compare candidates.
struct Interner<'s, S> {
    map: HashMap<Key<'s>, Name, S>,
    /// `None` disables the cache (the `hashdos` comparison only).
    front: Option<Box<FrontCache<'s>>>,
}

const FRONT: usize = 256;

/// The front cache's slots.
type FrontCache<'s> = [Option<(Key<'s>, Name)>; FRONT];

/// At most this many names are interned per parse. Past it, a new name is
/// built without inserting it (lookups continue). Duplicate `Name`s are
/// harmless, because equality falls back to content, so the cap only bounds
/// the cost of parts made of many distinct names (stage-1 acceptance,
/// review round 3 R3-1). Real parts use a few hundred names.
pub const INTERN_CAP: usize = 16_384;

/// Number of `Key` equality comparisons (the collision measure of the
/// stage-1 acceptance: time-independent, review round 3 R3-1).
pub static KEY_EQ_CALLS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The interner key: raw prefix, local name, address of the interned URI.
#[derive(Clone, Copy, Hash)]
struct Key<'s> {
    prefix: Option<&'s str>,
    local: &'s str,
    ns: usize,
}

impl PartialEq for Key<'_> {
    fn eq(&self, o: &Self) -> bool {
        KEY_EQ_CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.ns == o.ns && self.local == o.local && self.prefix == o.prefix
    }
}
impl Eq for Key<'_> {}

impl Key<'_> {
    /// The front cache's hit check: field comparison, not counted in
    /// `KEY_EQ_CALLS` (at most one per lookup by construction).
    fn same(&self, o: &Self) -> bool {
        self.ns == o.ns && self.local == o.local && self.prefix == o.prefix
    }

    /// The front-cache slot: unkeyed and cheap; correctness never depends
    /// on it (a hit is confirmed by `==`).
    fn slot(&self) -> usize {
        let l = self.local.as_bytes();
        let mix = l.len().wrapping_mul(31)
            ^ usize::from(*l.last().unwrap_or(&0))
            ^ (usize::from(*l.first().unwrap_or(&0)) << 3)
            ^ self.prefix.map_or(0, |p| p.len().wrapping_mul(7))
            ^ (self.ns >> 4);
        mix & (FRONT - 1)
    }
}

impl<'s, S: std::hash::BuildHasher> Interner<'s, S> {
    fn get(&mut self, ns: &Arc<str>, local: &'s str, prefix: Option<&'s str>) -> Name {
        let key = Key { prefix, local, ns: Arc::as_ptr(ns) as *const u8 as usize };
        let slot = key.slot();
        if let Some(front) = &self.front
            && let Some((k, n)) = &front[slot]
            && k.same(&key)
        {
            return n.clone();
        }
        let n = match self.map.get(&key) {
            Some(n) => n.clone(),
            None => {
                let n = Name(Arc::new(NameData { ns: ns.clone(), local: local.into(), prefix: prefix.map(Into::into) }));
                if self.map.len() < INTERN_CAP {
                    self.map.insert(key, n.clone());
                }
                n
            }
        };
        if let Some(front) = &mut self.front {
            front[slot] = Some((key, n.clone()));
        }
        n
    }
}

struct Open {
    name: Name,
    attrs: Box<[Attr]>,
    span: Span,
    scope_mark: usize,
}

struct Ctx<'s, S> {
    src: &'s str,
    source: Arc<str>,
    names: Interner<'s, S>,
    uris: HashMap<String, Arc<str>>,
    empty: Arc<str>,
    xmlns: Arc<str>,
    xml: Arc<str>,
    bindings: Vec<(Option<Box<str>>, Arc<str>)>,
    attr_scratch: Vec<(Option<&'s str>, &'s str, SStr)>,
    max_depth: usize,
}

impl<'s, S: std::hash::BuildHasher> Ctx<'s, S> {
    fn uri(&mut self, u: &str) -> Arc<str> {
        if let Some(a) = self.uris.get(u) {
            return a.clone();
        }
        let a: Arc<str> = Arc::from(u);
        self.uris.insert(u.to_owned(), a.clone());
        a
    }

    fn lookup(&self, p: Option<&str>) -> Option<Arc<str>> {
        self.bindings.iter().rev().find(|(q, _)| q.as_deref() == p).map(|(_, u)| u.clone())
    }

    /// Re-borrows a slice of the source with the source's lifetime.
    fn reslice(&self, b: &str) -> Result<&'s str, String> {
        let r = self.src.substr_range(b).ok_or("slice not in source")?;
        Ok(&self.src[r.start..r.end])
    }

    fn split(q: &str) -> (Option<&str>, &str) {
        match q.split_once(':') {
            Some((p, l)) => (Some(p), l),
            None => (None, q),
        }
    }

    fn open(&mut self, e: &BytesStart<'_>, depth: usize, before: usize, after: usize) -> Result<Open, String> {
        if depth >= self.max_depth {
            return Err("TooDeep".into());
        }
        let mark = self.bindings.len();
        let mut scratch = std::mem::take(&mut self.attr_scratch);
        scratch.clear();
        for a in e.attributes().with_checks(true) {
            let a = a.map_err(|e| e.to_string())?;
            let key = self.reslice(a.key.into_inner())?;
            let (prefix, local) = Self::split(key);
            let raw = self.src.substr_range(&a.value);
            let plain = raw.as_ref().is_some_and(|r| !self.src.as_bytes()[r.start..r.end].iter().any(|b| matches!(b, b'&' | b'\r' | b'\n' | b'\t')));
            let value = match raw {
                Some(r) if plain => SStr::span(&self.source, r.start, r.end),
                _ => SStr::owned(&a.normalized_value(XmlVersion::Implicit1_0).map_err(|e| e.to_string())?),
            };
            scratch.push((prefix, local, value));
        }
        for (prefix, local, value) in scratch.iter() {
            match (prefix, *local) {
                (None, "xmlns") => {
                    let u = self.uri(value.as_str());
                    self.bindings.push((None, u));
                }
                (Some("xmlns"), p) => {
                    let u = self.uri(value.as_str());
                    self.bindings.push((Some(p.into()), u));
                }
                _ => {}
            }
        }
        let mut attrs = Vec::with_capacity(scratch.len());
        for (prefix, local, value) in scratch.drain(..) {
            let ns = match (prefix, local) {
                (None, "xmlns") | (Some("xmlns"), _) => self.xmlns.clone(),
                (None, _) => self.empty.clone(),
                (Some("xml"), _) => self.xml.clone(),
                (Some(p), _) => self.lookup(Some(p)).filter(|u| !u.is_empty()).ok_or(format!("unbound prefix {p}"))?,
            };
            attrs.push(Attr { name: self.names.get(&ns, local, prefix), value });
        }
        self.attr_scratch = scratch;
        let qn = self.reslice(e.name().into_inner())?;
        let (prefix, local) = Self::split(qn);
        let ns = match prefix {
            None => self.lookup(None).unwrap_or_else(|| self.empty.clone()),
            Some("xml") => self.xml.clone(),
            Some(p) => self.lookup(Some(p)).filter(|u| !u.is_empty()).ok_or(format!("unbound prefix {p}"))?,
        };
        Ok(Open {
            name: self.names.get(&ns, local, prefix),
            attrs: attrs.into_boxed_slice(),
            span: Span { start: before as u32, open_end: after as u32, close_start: u32::MAX, close_end: u32::MAX },
            scope_mark: mark,
        })
    }
}

pub fn parse(bytes: &[u8], max_depth: usize) -> Result<CompactDoc, String> {
    parse_with::<super::NameHasher>(bytes, max_depth)
}

/// `parse` with the interner hasher given explicitly (the `hashdos` bench).
pub fn parse_with<S: std::hash::BuildHasher + Default>(bytes: &[u8], max_depth: usize) -> Result<CompactDoc, String> {
    parse_opts::<S>(bytes, max_depth, true)
}

/// As [`parse_with`], with the front cache switchable (`hashdos` only).
pub fn parse_opts<S: std::hash::BuildHasher + Default>(bytes: &[u8], max_depth: usize, front: bool) -> Result<CompactDoc, String> {
    let s = std::str::from_utf8(bytes).map_err(|e| e.to_string())?;
    let source: Arc<str> = Arc::from(s);
    let src: &str = &source;
    let root = parse_src::<S>(src, &source, max_depth, front)?;
    Ok(CompactDoc { source: source.clone(), root })
}

fn parse_src<S: std::hash::BuildHasher + Default>(src: &str, source: &Arc<str>, max_depth: usize, front: bool) -> Result<Node, String> {
    let base = if src.starts_with('\u{feff}') { 3 } else { 0 };
    let mut reader = Reader::from_str(&src[base..]);
    let cfg = reader.config_mut();
    cfg.trim_text(false);
    cfg.expand_empty_elements = false;
    cfg.check_end_names = true;
    let mut cx = Ctx {
        src,
        source: source.clone(),
        names: Interner { map: HashMap::<_, _, S>::default(), front: front.then(|| Box::new([const { None }; FRONT])) },
        uris: HashMap::new(),
        empty: Arc::from(""),
        xmlns: Arc::from(XMLNS_NS),
        xml: Arc::from(XML_NS),
        bindings: Vec::new(),
        attr_scratch: Vec::new(),
        max_depth,
    };
    let mut stack: Vec<Open> = Vec::new();
    // children of each open element (index = depth), reused
    let mut scratch: Vec<Vec<Item>> = vec![Vec::new()];
    let mut root: Option<Node> = None;
    loop {
        let before = base + reader.buffer_position() as usize;
        let ev = reader.read_event().map_err(|e| e.to_string())?;
        let after = base + reader.buffer_position() as usize;
        match ev {
            Event::Eof => break,
            Event::Decl(_) | Event::DocType(_) => {}
            Event::Start(e) => {
                let o = cx.open(&e, stack.len(), before, after)?;
                stack.push(o);
                if scratch.len() <= stack.len() {
                    scratch.push(Vec::new());
                }
            }
            Event::Empty(e) => {
                let o = cx.open(&e, stack.len(), before, after)?;
                cx.bindings.truncate(o.scope_mark);
                let node = Node { name: o.name, attrs: o.attrs, children: Vec::new(), span: o.span, own_dirty: false, subtree_dirty: false };
                attach(&mut scratch, &mut root, stack.len(), node);
            }
            Event::End(_) => {
                let mut o = stack.pop().ok_or("unmatched close")?;
                cx.bindings.truncate(o.scope_mark);
                o.span.close_start = before as u32;
                o.span.close_end = after as u32;
                // An exact-capacity copy; the scratch vector keeps its
                // capacity for the next sibling at this depth.
                #[expect(clippy::drain_collect, reason = "exact capacity; the scratch buffer is reused")]
                let children: Vec<Item> = scratch[stack.len() + 1].drain(..).collect();
                let node = Node { name: o.name, attrs: o.attrs, children, span: o.span, own_dirty: false, subtree_dirty: false };
                attach(&mut scratch, &mut root, stack.len(), node);
            }
            Event::Text(t) => {
                if stack.is_empty() {
                    continue;
                }
                let r = src.substr_range(&t).map_or(before..after, |r| r.start..r.end);
                let decoded = t.xml_content(XmlVersion::Implicit1_0);
                let value = if decoded.len() == r.end - r.start { SStr::span(source, r.start, r.end) } else { SStr::owned(&decoded) };
                push_text(&mut scratch[stack.len()], value, r.start, r.end);
            }
            Event::GeneralRef(rf) => {
                if stack.is_empty() {
                    continue;
                }
                let ch = match rf.resolve_char_ref().map_err(|e| e.to_string())? {
                    Some(c) => c,
                    None => match &*rf {
                        "lt" => '<',
                        "gt" => '>',
                        "amp" => '&',
                        "apos" => '\'',
                        "quot" => '"',
                        other => return Err(format!("undeclared entity {other}")),
                    },
                };
                push_text(&mut scratch[stack.len()], SStr::owned(ch.encode_utf8(&mut [0; 4])), before, after);
            }
            Event::CData(c) => {
                if stack.is_empty() {
                    continue;
                }
                let v = c.xml_content(XmlVersion::Implicit1_0);
                scratch[stack.len()].push(Item::Text(Text { value: SStr::owned(&v), span: (before as u32, after as u32), cdata: true }));
            }
            Event::Comment(_) | Event::PI(_) => {
                if !stack.is_empty() {
                    scratch[stack.len()].push(Item::Markup(SStr::span(source, before, after)));
                }
            }
        }
    }
    root.ok_or_else(|| "no root".into())
}

fn attach(scratch: &mut [Vec<Item>], root: &mut Option<Node>, depth: usize, node: Node) {
    if depth == 0 {
        *root = Some(node);
    } else {
        scratch[depth].push(Item::Element(node));
    }
}

/// Merges adjacent text pieces (as core does across entity references).
fn push_text(children: &mut Vec<Item>, value: SStr, start: usize, end: usize) {
    if let Some(Item::Text(prev)) = children.last_mut()
        && !prev.cdata
        && prev.span.1 as usize == start
    {
        let mut s = String::from(prev.value.as_str());
        s.push_str(value.as_str());
        prev.value = SStr::owned(&s);
        prev.span.1 = end as u32;
        return;
    }
    children.push(Item::Text(Text { value, span: (start as u32, end as u32), cdata: false }));
}

impl CompactDoc {
    pub fn serialize(&self) -> String {
        let src: &str = &self.source;
        let mut out = String::with_capacity(src.len() + 64);
        out.push_str(&src[..self.root.span.start as usize]);
        write_node(&mut out, src, &self.root);
        out.push_str(&src[self.root.span.end() as usize..]);
        out
    }

    pub fn force_rerender(&mut self) {
        fn go(n: &mut Node) {
            n.own_dirty = true;
            n.subtree_dirty = true;
            for c in &mut n.children {
                match c {
                    Item::Element(e) => go(e),
                    Item::Text(t) => t.span.0 = u32::MAX,
                    Item::Markup(_) => {}
                }
            }
        }
        go(&mut self.root);
    }
}

/// Renders one node by the serialization rule, `src` being the text its
/// spans refer to.
pub fn render(n: &Node, src: &str) -> String {
    let mut out = String::new();
    write_node(&mut out, src, n);
    out
}

fn write_node(out: &mut String, src: &str, n: &Node) {
    let span = n.span;
    let parsed = span.start != u32::MAX;
    if parsed && !n.subtree_dirty {
        out.push_str(&src[span.start as usize..span.end() as usize]);
        return;
    }
    let rerender = n.own_dirty || !parsed || (span.close_start == u32::MAX && !n.children.is_empty());
    if rerender {
        let _ = write!(out, "<{}", n.name.display());
        for a in n.attrs.iter() {
            let _ = write!(out, " {}=\"", a.name.display());
            for c in a.value.as_str().chars() {
                match c {
                    '&' => out.push_str("&amp;"),
                    '<' => out.push_str("&lt;"),
                    '"' => out.push_str("&quot;"),
                    '\t' => out.push_str("&#9;"),
                    '\n' => out.push_str("&#10;"),
                    '\r' => out.push_str("&#13;"),
                    c => out.push(c),
                }
            }
            out.push('"');
        }
        out.push_str(if n.children.is_empty() { "/>" } else { ">" });
    } else {
        out.push_str(&src[span.start as usize..span.open_end as usize]);
    }
    for c in &n.children {
        match c {
            Item::Element(e) => write_node(out, src, e),
            Item::Text(t) if t.span.0 != u32::MAX => out.push_str(&src[t.span.0 as usize..t.span.1 as usize]),
            Item::Text(t) if t.cdata => {
                out.push_str("<![CDATA[");
                out.push_str(&t.value.as_str().replace("]]>", "]]]]><![CDATA[>"));
                out.push_str("]]>");
            }
            Item::Text(t) => {
                for ch in t.value.as_str().chars() {
                    match ch {
                        '&' => out.push_str("&amp;"),
                        '<' => out.push_str("&lt;"),
                        '>' => out.push_str("&gt;"),
                        '\r' => out.push_str("&#13;"),
                        c => out.push(c),
                    }
                }
            }
            Item::Markup(m) => out.push_str(m.as_str()),
        }
    }
    if rerender {
        if !n.children.is_empty() {
            let _ = write!(out, "</{}>", n.name.display());
        }
    } else if span.close_start != u32::MAX {
        out.push_str(&src[span.close_start as usize..span.close_end as usize]);
    }
}

/// Structural equality with core's tree (names incl. prefix, attributes,
/// child items, text values, markup).
pub fn same_as_core(c: &Node, r: &ooxml_core::RawNode) -> bool {
    use ooxml_core::RawItem;
    let name_eq = |n: &Name, x: &ooxml_core::XName| n.ns() == x.ns() && n.local() == x.local() && n.prefix() == x.prefix();
    name_eq(&c.name, r.name())
        && c.attrs.len() == r.attrs().len()
        && c.attrs.iter().zip(r.attrs()).all(|(a, b)| name_eq(&a.name, b.name()) && a.value.as_str() == b.value())
        && c.children.len() == r.children().len()
        && c.children.iter().zip(r.children()).all(|(a, b)| match (a, b) {
            (Item::Element(a), RawItem::Element(b)) => same_as_core(a, b),
            (Item::Text(a), RawItem::Text(b)) => a.value.as_str() == b.value() && a.cdata == b.is_cdata(),
            (Item::Markup(a), RawItem::Markup(b)) => a.as_str() == b.raw(),
            _ => false,
        })
}
