//! Lens primitives (ADR-0002 §4.2) with small hand-written specs: slot
//! placement, limits, greedy assignment, refusals, dirty duties, prefixes.

use std::assert_matches;

use ooxml_core::{AttrSpec, ChildOf, Content, ElementSpec, Error, QName, SimpleType, Slot, Wildcard, XmlDocument};

const NS: &str = "urn:t";
const fn q(l: &'static str) -> QName {
    QName::new(NS, l)
}
const fn one(n: &'static [QName]) -> Slot {
    Slot { names: n, wildcard: None, max_each: Some(1), max_total: Some(1) }
}
const fn many(n: &'static [QName]) -> Slot {
    Slot { names: n, wildcard: None, max_each: None, max_total: None }
}

macro_rules! spec {
    ($t:ident, $l:literal, $content:expr $(, $attrs:expr)? $(; incomplete $inc:literal)?) => {
        enum $t {}
        impl ElementSpec for $t {
            const QNAME: QName = q($l);
            const SDK_NAME: &'static str = concat!("t:", $l);
            const ATTRS: &'static [AttrSpec] = spec!(@attrs $($attrs)?);
            const CONTENT: Content = $content;
            $(const ATTRS_INCOMPLETE: bool = $inc;)?
        }
    };
    (@attrs) => { &[] };
    (@attrs $a:expr) => { $a };
}

// <p> : head? (a|b){0..2} item* ext? any(other)*      — slots 0..4
spec!(P, "p", Content::Slots(&[
    one(&[q("head")]),
    Slot { names: &[q("a"), q("b")], wildcard: None, max_each: Some(2), max_total: Some(2) },
    many(&[q("item")]),
    one(&[q("ext")]),
    Slot { names: &[], wildcard: Some(Wildcard::Other(NS)), max_each: None, max_total: None },
]));
spec!(Head, "head", Content::Empty);
spec!(A, "a", Content::Empty);
spec!(B, "b", Content::Empty);
spec!(Item, "item", Content::Empty, &[AttrSpec { name: QName::new("", "k"), ty: SimpleType::UInt32, default: None }]);
spec!(Ext, "ext", Content::Empty);
impl ChildOf<P> for Head {}
impl ChildOf<P> for A {}
impl ChildOf<P> for B {}
impl ChildOf<P> for Item {}
impl ChildOf<P> for Ext {}

// <tbl> : mark* pr? mark* row*   — `mark` in two slots (like w:tbl's range markers)
spec!(Tbl, "tbl", Content::Slots(&[many(&[q("mark")]), one(&[q("pr")]), many(&[q("mark"), q("row")])]));
spec!(Mark, "mark", Content::Empty);
spec!(Pr, "pr", Content::Empty);
spec!(Row, "row", Content::Empty);
impl ChildOf<Tbl> for Mark {}
impl ChildOf<Tbl> for Pr {}
impl ChildOf<Tbl> for Row {}

spec!(T, "t", Content::Text(SimpleType::String),
      &[AttrSpec { name: QName::new("http://www.w3.org/XML/1998/namespace", "space"), ty: SimpleType::String, default: None }]);
spec!(Odd, "odd", Content::Unsupported(&[q("t")]));
impl ChildOf<Odd> for T {}
spec!(Inc, "inc", Content::Empty; incomplete true);

fn doc(s: &str) -> XmlDocument {
    XmlDocument::parse(s.as_bytes()).unwrap()
}

fn names(d: &XmlDocument) -> Vec<String> {
    d.root().elements().map(|e| e.name().local().to_owned()).collect()
}

#[test]
fn insertion_follows_slot_order_and_caller_order_within_a_slot() {
    let mut d = doc(r#"<p xmlns="urn:t"><item k="1"/><item k="3"/><ext/></p>"#);
    let mut p = d.lens_mut::<P>().unwrap();
    let key = |n: &ooxml_core::RawNode| n.attr(QName::new("", "k")).unwrap().parse::<u32>().unwrap();
    p.insert_by::<Item>(|_, e| key(e) > 2).unwrap().set_attr(QName::new("", "k"), "2").unwrap();
    p.insert_by::<Item>(|_, e| key(e) > 4).unwrap().set_attr(QName::new("", "k"), "4").unwrap();
    p.insert::<Head>().unwrap();
    p.insert::<B>().unwrap();
    p.insert::<A>().unwrap();
    assert_eq!(names(&d), ["head", "b", "a", "item", "item", "item", "item", "ext"]);
    assert_eq!(
        d.serialize(),
        r#"<p xmlns="urn:t"><head/><b/><a/><item k="1"/><item k="2"/><item k="3"/><item k="4"/><ext/></p>"#
    );
    assert!(d.root().subtree_dirty() && !d.root().own_dirty(), "parent with a close tag keeps its tags");
}

#[test]
fn limits_are_enforced() {
    let mut d = doc(r#"<p xmlns="urn:t"><head/><a/></p>"#);
    let mut p = d.lens_mut::<P>().unwrap();
    assert_matches!(p.insert::<Head>(), Err(Error::AlreadyPresent(_)));
    p.insert::<A>().unwrap(); // max_each 2
    assert_matches!(p.insert::<A>(), Err(Error::AlreadyPresent(_)));
    assert_matches!(p.insert::<B>(), Err(Error::AlreadyPresent(_)), "max_total 2 reached");
    // get_or_insert reuses
    let before = d.serialize();
    let mut p = d.lens_mut::<P>().unwrap();
    p.get_or_insert::<Head>().unwrap();
    assert_eq!(d.serialize(), before);
}

#[test]
fn unlisted_children_never_decide_position_and_wildcards_rank() {
    // <other:z> matches the ##other wildcard (rank 4); <zz> (own namespace, unlisted) attaches to <item>.
    let mut d = doc(r#"<p xmlns="urn:t" xmlns:o="urn:o"><item/><zz/><o:z/></p>"#);
    let mut p = d.lens_mut::<P>().unwrap();
    p.insert::<Ext>().unwrap();
    p.insert::<Item>().unwrap();
    assert_eq!(d.serialize(), r#"<p xmlns="urn:t" xmlns:o="urn:o"><item/><zz/><item/><ext/><o:z/></p>"#);
}

#[test]
fn alternate_content_sibling_refuses_insertion() {
    let mut d = doc(
        r#"<p xmlns="urn:t" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><mc:AlternateContent/></p>"#,
    );
    assert_matches!(d.lens_mut::<P>().unwrap().insert::<Item>(), Err(Error::UnresolvedSibling(_)));
}

#[test]
fn out_of_order_children_refuse_insertion() {
    let mut d = doc(r#"<p xmlns="urn:t"><ext/><head/></p>"#);
    assert_matches!(d.lens_mut::<P>().unwrap().insert::<Item>(), Err(Error::UnresolvedSibling(_)));
}

#[test]
fn a_qname_in_two_slots_needs_an_explicit_slot_and_is_assigned_greedily() {
    let mut d = doc(r#"<tbl xmlns="urn:t"><mark/><pr/><row/></tbl>"#);
    let mut t = d.lens_mut::<Tbl>().unwrap();
    assert_matches!(t.insert::<Mark>(), Err(Error::AmbiguousSlot(_)));
    t.insert_in_slot::<Mark>(2, |_, _| false).unwrap();
    t.insert_in_slot::<Mark>(0, |_, _| false).unwrap();
    assert_matches!(t.insert_in_slot::<Pr>(0, |_, _| false), Err(Error::NotAllowed { .. }));
    assert_eq!(names(&d), ["mark", "mark", "pr", "row", "mark"]);
    let mut t = d.lens_mut::<Tbl>().unwrap();
    t.insert::<Row>().unwrap(); // `row` is in one slot only: no ambiguity
    assert_eq!(names(&d), ["mark", "mark", "pr", "row", "mark", "row"]);

    // leading marker is slot 0, so a new `pr` goes after it, not in front
    let mut d = doc(r#"<tbl xmlns="urn:t"><mark/><row/></tbl>"#);
    d.lens_mut::<Tbl>().unwrap().insert::<Pr>().unwrap();
    assert_eq!(names(&d), ["mark", "pr", "row"]);
}

#[test]
fn self_closing_parent_gains_and_loses_children() {
    let mut d = doc(r#"<p xmlns="urn:t" x="1"/>"#);
    let mut p = d.lens_mut::<P>().unwrap();
    p.insert::<Item>().unwrap();
    assert!(p.node().own_dirty(), "self-closing parent gaining its first child is own_dirty");
    assert_eq!(d.serialize(), r#"<p xmlns="urn:t" x="1"><item/></p>"#);
    let mut p = d.lens_mut::<P>().unwrap();
    assert!(p.remove::<Item>());
    assert_eq!(d.serialize(), r#"<p xmlns="urn:t" x="1"/>"#);

    // a parent with a close tag keeps it when emptied
    let mut d = doc(r#"<p xmlns="urn:t"  ><item/></p >"#);
    let mut p = d.lens_mut::<P>().unwrap();
    assert!(p.remove::<Item>());
    assert!(!p.node().own_dirty());
    assert_eq!(d.serialize(), r#"<p xmlns="urn:t"  ></p >"#);
}

#[test]
fn dirty_flags_propagate_only_along_the_path() {
    let src = r#"<p xmlns="urn:t"><item k="1"/><item  k = '2' /></p>"#;
    let mut d = doc(src);
    {
        let mut p = d.lens_mut::<P>().unwrap();
        let mut second = p.nth_child_mut::<Item>(1).unwrap();
        second.set_attr(QName::new("", "k"), "9").unwrap();
        assert!(second.node().own_dirty());
    }
    let root = d.root();
    assert!(root.subtree_dirty() && !root.own_dirty());
    let items: Vec<_> = root.elements().collect();
    assert!(!items[0].subtree_dirty(), "untouched sibling stays verbatim");
    assert_eq!(d.serialize(), r#"<p xmlns="urn:t"><item k="1"/><item k="9"/></p>"#);
    // setting the same value is not a change
    let mut d = doc(src);
    d.lens_mut::<P>().unwrap().nth_child_mut::<Item>(0).unwrap().set_attr(QName::new("", "k"), "1").unwrap();
    assert!(!d.is_dirty());
}

#[test]
fn set_text_duties() {
    let mut d = doc(r#"<odd xmlns="urn:t"><t/><t>keep</t></odd>"#);
    let mut odd = d.lens_mut::<Odd>().unwrap();
    assert_matches!(odd.insert::<T>(), Err(Error::UnsupportedContentModel(_)));
    let mut t = odd.child_mut::<T>().unwrap();
    t.set_text(" padded ").unwrap();
    assert!(t.node().own_dirty());
    assert_eq!(d.serialize(), r#"<odd xmlns="urn:t"><t xml:space="preserve"> padded </t><t>keep</t></odd>"#);
    // escaping, and xml:space is never removed
    let mut odd = d.lens_mut::<Odd>().unwrap();
    odd.child_mut::<T>().unwrap().set_text("a<b&c").unwrap();
    assert_eq!(d.serialize(), r#"<odd xmlns="urn:t"><t xml:space="preserve">a&lt;b&amp;c</t><t>keep</t></odd>"#);
    // a non-text element refuses set_text
    let mut d = doc(r#"<p xmlns="urn:t"/>"#);
    assert_matches!(d.lens_mut::<P>().unwrap().set_text("x"), Err(Error::NotText(_)));
}

#[test]
fn non_xml_characters_are_refused_before_any_change() {
    let src = r#"<odd xmlns="urn:t"><t>keep</t></odd>"#;
    let mut d = doc(src);
    for bad in ["a\u{b}b", "\u{1}", "x\u{FFFE}", "\u{FFFF}", "\u{0}"] {
        let mut odd = d.lens_mut::<Odd>().unwrap();
        let mut t = odd.child_mut::<T>().unwrap();
        assert_matches!(t.set_text(bad), Err(Error::InvalidXmlChar { .. }), "{bad:?}");
        assert_matches!(t.set_attr(QName::new("", "k"), bad), Err(Error::InvalidXmlChar { .. }), "{bad:?}");
    }
    assert!(!d.is_dirty());
    assert_eq!(d.serialize(), src);
    // tab, LF, CR and non-BMP characters are XML characters
    let mut odd = d.lens_mut::<Odd>().unwrap();
    odd.child_mut::<T>().unwrap().set_text("a\tb\nc\rd\u{1F600}").unwrap();
    assert_eq!(d.serialize(), "<odd xmlns=\"urn:t\"><t>a\tb\nc&#13;d\u{1F600}</t></odd>");
    let err = Error::InvalidXmlChar { ch: '\u{b}', offset: 1 };
    assert_eq!(err.to_string(), "character U+000B at byte 1 is not allowed in XML 1.0");
}

#[test]
fn incomplete_specs_refuse_attribute_writes() {
    let mut d = doc(r#"<inc xmlns="urn:t"/>"#);
    assert_matches!(d.lens_mut::<Inc>().unwrap().set_attr(QName::new("", "x"), "1"), Err(Error::IncompleteSpec(_)));
}

#[test]
fn prefix_rules_for_new_nodes() {
    // parent's own prefix binds the URI
    let mut d = doc(r#"<x:p xmlns:x="urn:t" xmlns="urn:t"/>"#);
    d.lens_mut::<P>().unwrap().insert::<Item>().unwrap();
    assert_eq!(d.serialize(), r#"<x:p xmlns:x="urn:t" xmlns="urn:t"><x:item/></x:p>"#);

    // the parent's prefix binds another URI: the default namespace wins on the same element
    let mut d = doc(r#"<t:p xmlns:t="urn:t" xmlns:f="urn:f" xmlns="urn:f"/>"#);
    d.lens_mut::<P>().unwrap().insert::<Foreign>().unwrap();
    assert_eq!(d.serialize(), r#"<t:p xmlns:t="urn:t" xmlns:f="urn:f" xmlns="urn:f"><fx/></t:p>"#);

    // otherwise the innermost binding in effect
    let mut d = doc(r#"<t:r xmlns:t="urn:t" xmlns:a="urn:f"><t:p xmlns:b="urn:f"/></t:r>"#);
    d.lens_mut::<Rr>().unwrap().child_mut::<P>().unwrap().insert::<Foreign>().unwrap();
    assert!(d.serialize().contains("<b:fx/>"), "{}", d.serialize());
    let mut d = doc(r#"<t:r xmlns:t="urn:t" xmlns:a="urn:f"><t:p/></t:r>"#);
    d.lens_mut::<Rr>().unwrap().child_mut::<P>().unwrap().insert::<Foreign>().unwrap();
    assert!(d.serialize().contains("<a:fx/>"), "{}", d.serialize());
    let mut d = doc(r#"<t:r xmlns:t="urn:t" xmlns:a="urn:f"><t:p xmlns:a="urn:x"/></t:r>"#);
    let mut r = d.lens_mut::<Rr>().unwrap();
    assert_matches!(r.child_mut::<P>().unwrap().insert::<Foreign>(), Err(Error::NamespaceNotInScope(_)));

    // a prefix re-declared to another URI is not in effect
    let mut d = doc(r#"<o:p xmlns:o="urn:t"><o:q xmlns:o="urn:other"/></o:p>"#);
    let mut p = d.lens_mut::<P>().unwrap();
    p.insert::<Item>().unwrap();
    assert!(d.serialize().contains("<o:item/>"));

    // namespaced attribute needs a non-empty prefix; xml: always works
    let mut d = doc(r#"<p xmlns="urn:t"/>"#);
    let mut p = d.lens_mut::<P>().unwrap();
    assert_matches!(p.set_attr(QName::new("urn:t", "k"), "1"), Err(Error::NamespaceNotInScope(_)));
    p.set_attr(QName::new("http://www.w3.org/XML/1998/namespace", "lang"), "en").unwrap();
    assert_eq!(d.serialize(), r#"<p xmlns="urn:t" xml:lang="en"/>"#);

    // no binding at all
    let mut d = doc(r#"<p xmlns="urn:t"><u xmlns="urn:u"/></p>"#);
    let mut p = d.lens_mut::<P>().unwrap();
    p.insert::<Item>().unwrap();
    assert!(d.serialize().contains("<item/>"));
}

spec!(Rr, "r", Content::Slots(&[many(&[q("p")])]));
impl ChildOf<Rr> for P {}

/// An element in another namespace, admitted by `P`'s `##other` wildcard.
enum Foreign {}
impl ElementSpec for Foreign {
    const QNAME: QName = QName::new("urn:f", "fx");
    const SDK_NAME: &'static str = "f:fx";
    const ATTRS: &'static [AttrSpec] = &[];
    const CONTENT: Content = Content::Empty;
}
impl ChildOf<P> for Foreign {}

#[test]
fn wrong_root_is_reported() {
    let d = doc(r#"<p xmlns="urn:other"/>"#);
    assert_matches!(d.lens::<P>(), Err(Error::WrongElement { .. }));
}
