//! Lens edit sequences on a small hand-written schema.
//!
//! ```text
//! p    : head? (a|b){0..2} item* t* ext? ##other*      attrs: k
//! item : item* t*                                       attrs: k (UInt32)
//! t    : text                                           attrs: xml:space
//! ```
//!
//! `item` nests, so with a small parse depth limit the `TooDeep` refusal
//! of inserts is reachable.

use ooxml_core::name::{XML_NS, XMLNS_NS};
use ooxml_core::{
    AttrSpec, ChildOf, Content, ElementSpec, LensMut, ParseOptions, QName, RawNode, Result, SimpleType, Slot,
    Wildcard, XmlDocument,
};

use crate::input::Input;
use crate::xml::canon_eq;

/// Namespace of the test schema.
pub const NS: &str = "urn:t";

const fn q(l: &'static str) -> QName {
    QName::new(NS, l)
}
const fn one(n: &'static [QName]) -> Slot {
    Slot { names: n, wildcard: None, max_each: Some(1), max_total: Some(1) }
}
const fn many(n: &'static [QName]) -> Slot {
    Slot { names: n, wildcard: None, max_each: None, max_total: None }
}

const K: QName = QName::new("", "k");
const K_ATTR: &[AttrSpec] = &[AttrSpec { name: K, ty: SimpleType::UInt32, default: None }];

macro_rules! spec {
    ($t:ident, $l:literal, $content:expr, $attrs:expr) => {
        #[doc = concat!("`", $l, "` of the test schema.")]
        pub enum $t {}
        impl ElementSpec for $t {
            const QNAME: QName = q($l);
            const SDK_NAME: &'static str = concat!("t:", $l);
            const ATTRS: &'static [AttrSpec] = $attrs;
            const CONTENT: Content = $content;
        }
    };
}

spec!(
    P,
    "p",
    Content::Slots(&[
        one(&[q("head")]),
        Slot { names: &[q("a"), q("b")], wildcard: None, max_each: Some(2), max_total: Some(2) },
        many(&[q("item")]),
        many(&[q("t")]),
        one(&[q("ext")]),
        Slot { names: &[], wildcard: Some(Wildcard::Other(NS)), max_each: None, max_total: None },
    ]),
    K_ATTR
);
spec!(Head, "head", Content::Empty, &[]);
spec!(A, "a", Content::Empty, &[]);
spec!(B, "b", Content::Empty, &[]);
spec!(Ext, "ext", Content::Empty, &[]);
spec!(Item, "item", Content::Slots(&[many(&[q("item")]), many(&[q("t")])]), K_ATTR);
spec!(
    T,
    "t",
    Content::Text(SimpleType::String),
    &[AttrSpec { name: QName::new(XML_NS, "space"), ty: SimpleType::String, default: None }]
);
impl ChildOf<P> for Head {}
impl ChildOf<P> for A {}
impl ChildOf<P> for B {}
impl ChildOf<P> for Ext {}
impl ChildOf<P> for Item {}
impl ChildOf<P> for T {}
impl ChildOf<Item> for Item {}
impl ChildOf<Item> for T {}

/// Start documents the fuzzer can pick instead of supplying its own.
pub const BASE_DOCS: &[&str] = &[
    r#"<p xmlns="urn:t"/>"#,
    r#"<p xmlns="urn:t"><item k="1"/><item k="3"><t>x</t></item><ext/></p>"#,
    r#"<x:p xmlns:x="urn:t" xmlns:o="urn:o"><x:head/><x:item k="2"><x:item/></x:item><o:z/></x:p>"#,
    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n<p xmlns=\"urn:t\">\r\n  <!-- c -->\r\n  <item k=\"1\">\r\n    <t xml:space=\"preserve\"> a </t>\r\n  </item>\r\n  <?pi x?>\r\n</p>\r\n",
    r#"<p xmlns="urn:t" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><item/><mc:AlternateContent/></p>"#,
    r#"<p xmlns="urn:t"><ext/><head/><t><![CDATA[c]]>&amp;</t></p>"#,
];

/// Attribute names the edits use. The last one, an `xmlns:o` declaration,
/// must be refused by the lens (`NamespaceDeclaration`, fuzz finding F-3).
const ATTR_NAMES: &[QName] =
    &[K, QName::new(XML_NS, "space"), QName::new("urn:o", "x"), QName::new("", "zz"), QName::new(XMLNS_NS, "o")];

fn attr_name(inp: &mut Input<'_>) -> QName {
    ATTR_NAMES[usize::from(inp.u8()) % ATTR_NAMES.len()]
}

fn nth_k(inp: &mut Input<'_>) -> impl FnMut(usize, &RawNode) -> bool + use<> {
    let threshold = inp.u8();
    move |_, n| n.attr(K).and_then(|v| v.parse::<u32>().ok()).is_some_and(|k| k > u32::from(threshold % 8))
}

/// One primitive on an element that can hold `item` and `t`.
fn op_generic<E: ElementSpec>(l: &mut LensMut<'_, E>, op: u8, inp: &mut Input<'_>) -> Result<()>
where
    Item: ChildOf<E>,
    T: ChildOf<E>,
{
    match op % 12 {
        0 => l.insert::<Item>().map(drop),
        1 => {
            let pred = nth_k(inp);
            l.insert_by::<Item>(pred).map(drop)
        }
        2 => l.insert::<T>().map(drop),
        3 => l.get_or_insert::<T>().map(drop),
        4 => {
            l.remove::<Item>();
            Ok(())
        }
        5 => {
            let i = usize::from(inp.u8() % 8);
            l.remove_at(i);
            Ok(())
        }
        6 | 7 => {
            let name = attr_name(inp);
            l.set_attr(name, &inp.string())
        }
        8 => {
            let name = attr_name(inp);
            l.remove_attr(name).map(drop)
        }
        9 | 10 => {
            let n = usize::from(inp.u8() % 4);
            let s = inp.string();
            match l.nth_child_mut::<T>(n) {
                Some(mut t) => t.set_text(&s),
                None => Ok(()),
            }
        }
        _ => {
            let n = usize::from(inp.u8() % 4);
            let s = inp.string();
            match l.nth_child_mut::<T>(n) {
                Some(mut t) => {
                    let name = attr_name(inp);
                    t.set_attr(name, &s)
                }
                None => Ok(()),
            }
        }
    }
}

fn op_root(l: &mut LensMut<'_, P>, op: u8, inp: &mut Input<'_>) -> Result<()> {
    match op % 8 {
        0 => l.insert::<Head>().map(drop),
        1 => l.insert::<A>().map(drop),
        2 => l.insert::<B>().map(drop),
        3 => l.insert::<Ext>().map(drop),
        4 => l.get_or_insert::<Head>().map(drop),
        5 => {
            let slot = usize::from(inp.u8() % 7);
            l.insert_in_slot::<Item>(slot, |_, _| false).map(drop)
        }
        6 => {
            l.remove::<Head>();
            Ok(())
        }
        _ => op_generic(l, inp.u8(), inp),
    }
}

/// Applies one fuzzer-chosen primitive somewhere in the tree. `Ok(())` also
/// covers "the navigation found nothing".
fn apply_one(doc: &mut XmlDocument, inp: &mut Input<'_>) -> Result<()> {
    let mut root = doc.lens_mut::<P>()?;
    let sel = inp.u8();
    let depth = sel % 5;
    let op = inp.u8();
    if depth == 0 {
        return if sel & 0x80 != 0 { op_root(&mut root, op, inp) } else { op_generic(&mut root, op, inp) };
    }
    let mut picks = (0..depth).map(|_| usize::from(inp.u8() % 3)).collect::<Vec<_>>().into_iter();
    let first = picks.next().unwrap_or(0);
    let mut seen = 0;
    let Some(mut cur) = root.into_child_by::<Item>(|_| {
        seen += 1;
        seen > first
    }) else {
        return Ok(());
    };
    for pick in picks {
        let mut seen = 0;
        match cur.into_child_by::<Item>(|_| {
            seen += 1;
            seen > pick
        }) {
            Some(next) => cur = next,
            None => return Ok(()),
        }
    }
    op_generic(&mut cur, op, inp)
}

/// Runs a fuzzer-chosen edit sequence. Checks:
///
/// - every `Err` leaves the document unchanged: same serialization, same
///   structure, same dirty state (ADR-0002 §4.2 "refuse before change");
/// - the edited output parses again with the same depth limit and has the
///   same structure (adjacent text merged);
/// - re-rendering the edited tree gives the same structure too.
pub fn check_lens_ops(data: &[u8]) {
    let mut inp = Input::new(data);
    let max_depth = 2 + usize::from(inp.u8() % 8);
    let options = ParseOptions::default().with_max_depth(max_depth);
    let which = inp.u8();
    let mut doc = if usize::from(which) < BASE_DOCS.len() * 32 {
        match XmlDocument::parse_with(BASE_DOCS[usize::from(which) % BASE_DOCS.len()].as_bytes(), &options) {
            Ok(d) => d,
            Err(_) => return,
        }
    } else {
        let len = usize::from(inp.u16() % 2048);
        let xml = inp.take(len);
        match XmlDocument::parse_with(xml, &options) {
            Ok(d) => d,
            Err(_) => return,
        }
    };
    if doc.lens_mut::<P>().is_err() {
        return;
    }
    let mut steps = 0;
    while !inp.is_empty() && steps < 64 {
        steps += 1;
        let before = doc.clone();
        let before_s = doc.serialize();
        if let Err(e) = apply_one(&mut doc, &mut inp) {
            assert_eq!(doc.serialize(), before_s, "refused edit ({e}) changed the serialization");
            assert!(doc.root().structurally_eq(before.root()), "refused edit ({e}) changed the tree");
            assert_eq!(doc.is_dirty(), before.is_dirty(), "refused edit ({e}) changed the dirty state");
        }
    }
    let s = doc.serialize();
    let re = XmlDocument::parse_with(s.as_bytes(), &options)
        .unwrap_or_else(|e| panic!("edited output does not parse with the same options: {e}\n{s:?}"));
    assert!(canon_eq(re.root(), doc.root()), "edited output reparses to a different tree\n{s:?}");
    let mut r = doc.clone();
    r.force_rerender();
    let s2 = r.serialize();
    let re2 = XmlDocument::parse_with(s2.as_bytes(), &options)
        .unwrap_or_else(|e| panic!("re-rendered edited tree does not parse: {e}\n{s2:?}"));
    assert!(canon_eq(re2.root(), doc.root()), "re-rendered edited tree differs\n{s2:?}");
}
