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
fn malformed_alternate_content_sibling_refuses_insertion() {
    let source = r#"<p xmlns="urn:t" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><mc:AlternateContent/></p>"#;
    let mut d = doc(source);
    assert_matches!(d.lens_mut::<P>().unwrap().insert::<Item>(), Err(Error::UnresolvedSibling(_)));
    assert_eq!(d.serialize(), source);
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

/// `insert_at` / `check_insert_at`: positional insertion with local schema
/// checks. Agrees with `insert_by` wherever the caller's position is the one
/// `insert_by` would pick; refuses out-of-order neighbours, AC neighbours,
/// full slots and bad indices without changing anything.
#[test]
fn insert_at_places_at_the_index_with_local_checks() {
    let src = r#"<p xmlns="urn:t"><head/><!--c--><item k="1"/>  <item k="3"/><ext/></p>"#;
    // same result as insert_by for the position insert_by would choose
    for (k, at) in [("0", 2), ("2", 4), ("4", 5)] {
        let mut by = doc(src);
        let key = |n: &ooxml_core::RawNode| n.attr(QName::new("", "k")).unwrap().parse::<u32>().unwrap();
        let want: u32 = k.parse().unwrap();
        by.lens_mut::<P>().unwrap().insert_by::<Item>(|_, e| key(e) > want).unwrap().set_attr(QName::new("", "k"), k).unwrap();
        let mut at_doc = doc(src);
        {
            let mut p = at_doc.lens_mut::<P>().unwrap();
            p.check_insert_at::<Item>(at).unwrap();
            p.insert_at::<Item>(at).unwrap().set_attr(QName::new("", "k"), k).unwrap();
        }
        assert_eq!(at_doc.serialize(), by.serialize(), "k={k} at {at}");
    }
    // refusals: before head (item must follow head), after ext, out of range,
    // next to an mc:AlternateContent, a full slot
    for at in [0, 6] {
        let d = doc(src);
        let mut d2 = doc(src);
        let p = d2.lens_mut::<P>().unwrap();
        assert_matches!(p.check_insert_at::<Item>(at), Err(Error::UnresolvedSibling(_)), "at {at}");
        assert_eq!(d2.serialize(), d.serialize());
    }
    let mut d = doc(src);
    assert_matches!(d.lens_mut::<P>().unwrap().insert_at::<Item>(99), Err(Error::InsertIndexOutOfRange { index: 99, len: 6 }));
    assert!(!d.is_dirty());
    let mut d = doc(r#"<p xmlns="urn:t" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><item/><mc:AlternateContent/></p>"#);
    assert_matches!(d.lens_mut::<P>().unwrap().insert_at::<Item>(1), Err(Error::UnresolvedSibling(_)));
    let before = d.serialize();
    assert_matches!(d.lens_mut::<P>().unwrap().insert_at::<Item>(0), Err(Error::UnresolvedSibling(_)));
    assert_eq!(d.serialize(), before);
    let mut d = doc(r#"<p xmlns="urn:t"><head/></p>"#);
    assert_matches!(d.lens_mut::<P>().unwrap().insert_at::<Head>(1), Err(Error::AlreadyPresent(_)));
    assert!(!d.is_dirty());
    // a self-closing parent gains its first child
    let mut d = doc(r#"<p xmlns="urn:t"/>"#);
    d.lens_mut::<P>().unwrap().insert_at::<Item>(0).unwrap();
    assert_eq!(d.serialize(), r#"<p xmlns="urn:t"><item/></p>"#);
}

// <q> : w* (urn:o)  — a child in another namespace than its parent
spec!(Q, "q", Content::Slots(&[many(&[QName::new("urn:o", "w")])]));
enum W {}
impl ElementSpec for W {
    const QNAME: QName = QName::new("urn:o", "w");
    const SDK_NAME: &'static str = "o:w";
    const ATTRS: &'static [AttrSpec] = &[];
    const CONTENT: Content = Content::Empty;
}
impl ChildOf<Q> for W {}
impl ChildOf<P> for W {}

/// Review F1: ambiguous neighbours (a QName in two slots) are accepted only
/// if every reading is in order; review F4 (M16): the inserted QName itself
/// may not be ambiguous.
#[test]
fn insert_at_with_ambiguous_neighbours() {
    // `mark` can be slot 0 or 2: inserting a `row` before it could push it to
    // slot 2 behind `pr`; inserting `pr` after a `mark` that may be slot 2 too.
    for (src, check) in [
        (r#"<tbl xmlns="urn:t"><mark/><pr/></tbl>"#, 0usize),
        (r#"<tbl xmlns="urn:t"><row/><mark/></tbl>"#, 2),
    ] {
        let mut d = doc(src);
        let mut t = d.lens_mut::<Tbl>().unwrap();
        let r = if check == 0 { t.insert_at::<Row>(0).map(drop) } else { t.insert_at::<Pr>(2).map(drop) };
        assert_matches!(r, Err(Error::UnresolvedSibling(_)), "{src}");
        assert!(!d.is_dirty());
    }
    let mut d = doc(r#"<tbl xmlns="urn:t"/>"#);
    assert_matches!(d.lens_mut::<Tbl>().unwrap().check_insert_at::<Mark>(0), Err(Error::AmbiguousSlot(_)));
}

/// Limits use the greedy assignment. Malformed ACs anywhere in the parent
/// prevent placement, including in unlimited slots (ADR-0020 §4.3).
#[test]
fn insert_at_limits_and_far_alternate_content() {
    let mc = r#"xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006""#;
    // max_each 2 for `a`; max_total 2 for (a|b)
    let mut d = doc(r#"<p xmlns="urn:t"><a/><a/></p>"#);
    assert_matches!(d.lens_mut::<P>().unwrap().insert_at::<A>(2), Err(Error::AlreadyPresent(_)));
    let mut d = doc(r#"<p xmlns="urn:t"><a/><b/></p>"#);
    assert_matches!(d.lens_mut::<P>().unwrap().insert_at::<A>(2), Err(Error::AlreadyPresent(_)));
    let mut d = doc(r#"<p xmlns="urn:t"><a/></p>"#);
    d.lens_mut::<P>().unwrap().insert_at::<B>(1).unwrap();
    // an AC wrapping (a|b) content elsewhere: limited slots refuse as insert_by does
    let src = format!(r#"<p xmlns="urn:t" {mc}><a/><mc:AlternateContent/><item/></p>"#);
    let mut d = doc(&src);
    assert_matches!(d.lens_mut::<P>().unwrap().insert_at::<A>(0), Err(Error::UnresolvedSibling(_)));
    assert_eq!(d.serialize(), src);
    let src = format!(r#"<p xmlns="urn:t" {mc}><mc:AlternateContent/><item/></p>"#);
    let mut d = doc(&src);
    assert_matches!(d.lens_mut::<P>().unwrap().insert_at::<Item>(2), Err(Error::UnresolvedSibling(_)));
    assert_eq!(d.serialize(), src);
}

/// Review F4 (M8, M11, M12, M13): dirty duties, depth limit and namespace
/// prefix agree between `check_insert_at` and `insert_at`.
#[test]
fn insert_at_dirt_depth_and_prefix() {
    let mut d = doc(r#"<p xmlns="urn:t"/>"#);
    d.lens_mut::<P>().unwrap().insert_at::<Item>(0).unwrap();
    assert!(d.root().own_dirty() && d.root().subtree_dirty(), "a self-closing parent gains its first child");
    let mut d = doc(r#"<p xmlns="urn:t" x="1"></p>"#);
    d.lens_mut::<P>().unwrap().insert_at::<Item>(0).unwrap();
    assert!(!d.root().own_dirty() && d.root().subtree_dirty(), "a parent with a close tag keeps its tags");
    // depth: the root is depth 1, its new child depth 2
    let opts = ooxml_core::ParseOptions::default().with_max_depth(1);
    let mut d = XmlDocument::parse_with(br#"<p xmlns="urn:t"/>"#, &opts).unwrap();
    assert_matches!(d.lens_mut::<P>().unwrap().check_insert_at::<Item>(0), Err(Error::TooDeep { .. }));
    assert_matches!(d.lens_mut::<P>().unwrap().insert_at::<Item>(0), Err(Error::TooDeep { .. }));
    assert!(!d.is_dirty());
    let opts = ooxml_core::ParseOptions::default().with_max_depth(2);
    let mut d = XmlDocument::parse_with(br#"<p xmlns="urn:t"/>"#, &opts).unwrap();
    d.lens_mut::<P>().unwrap().insert_at::<Item>(0).unwrap();
    // prefix: urn:o is not bound anywhere
    let mut d = doc(r#"<q xmlns="urn:t"/>"#);
    assert_matches!(d.lens_mut::<Q>().unwrap().check_insert_at::<W>(0), Err(Error::NamespaceNotInScope(_)));
    assert_matches!(d.lens_mut::<Q>().unwrap().insert_at::<W>(0), Err(Error::NamespaceNotInScope(_)));
    assert!(!d.is_dirty());
}

/// Every index of small `P` and `Tbl` trees: `check_insert_at` and
/// `insert_at` agree; an `Err` changes nothing; an `Ok` result is in schema
/// order (greedy assignment, as `place` does) and within limits.
#[test]
fn insert_at_exhaustive_agreement_and_validity() {
    type Slots = &'static [Slot];
    fn valid(names: &[String], slots: Slots) -> bool {
        let mut cur = 0usize;
        let mut count = vec![0u32; slots.len()];
        let mut each: std::collections::HashMap<(usize, String), u32> = Default::default();
        for n in names {
            let cands: Vec<usize> =
                slots.iter().enumerate().filter(|(_, s)| s.names.iter().any(|q| q.local == n)).map(|(i, _)| i).collect();
            if cands.is_empty() {
                continue;
            }
            let Some(&r) = cands.iter().find(|&&r| r >= cur) else { return false };
            cur = r;
            count[r] += 1;
            *each.entry((r, n.clone())).or_default() += 1;
        }
        slots.iter().enumerate().all(|(i, s)| {
            s.max_total.is_none_or(|m| count[i] <= m)
                && each.iter().filter(|((r, _), _)| *r == i).all(|(_, c)| s.max_each.is_none_or(|m| *c <= m))
        })
    }
    fn seqs(alphabet: &[&str], max: usize) -> Vec<Vec<&'static str>> {
        let alpha: Vec<&'static str> = alphabet.iter().map(|s| -> &'static str { Box::leak(s.to_string().into_boxed_str()) }).collect();
        let mut out = vec![vec![]];
        let mut frontier = vec![vec![]];
        for _ in 0..max {
            let mut next = Vec::new();
            for s in &frontier {
                for a in &alpha {
                    let mut t: Vec<&'static str> = s.clone();
                    t.push(a);
                    next.push(t);
                }
            }
            out.extend(next.iter().cloned());
            frontier = next;
        }
        out
    }
    const P_SLOTS: Slots = match P::CONTENT {
        Content::Slots(s) => s,
        _ => &[],
    };
    const T_SLOTS: Slots = match Tbl::CONTENT {
        Content::Slots(s) => s,
        _ => &[],
    };
    let mut oks = 0;
    for (root, slots, alphabet) in [("p", P_SLOTS, &["head", "a", "b", "item", "ext"][..]), ("tbl", T_SLOTS, &["mark", "pr", "row"][..])] {
        for seq in seqs(alphabet, 3) {
            let names: Vec<String> = seq.iter().map(|s| s.to_string()).collect();
            if !valid(&names, slots) {
                continue;
            }
            let body: String = seq.iter().map(|n| format!("<{n}/>")).collect();
            let src = format!(r#"<{root} xmlns="urn:t">{body}</{root}>"#);
            for idx in 0..=seq.len() {
                for child in alphabet {
                    let mut d = doc(&src);
                    let (check, ins) = match (root, *child) {
                        ("p", "head") => { let mut l = d.lens_mut::<P>().unwrap(); (l.check_insert_at::<Head>(idx).is_ok(), l.insert_at::<Head>(idx).is_ok()) }
                        ("p", "a") => { let mut l = d.lens_mut::<P>().unwrap(); (l.check_insert_at::<A>(idx).is_ok(), l.insert_at::<A>(idx).is_ok()) }
                        ("p", "b") => { let mut l = d.lens_mut::<P>().unwrap(); (l.check_insert_at::<B>(idx).is_ok(), l.insert_at::<B>(idx).is_ok()) }
                        ("p", "item") => { let mut l = d.lens_mut::<P>().unwrap(); (l.check_insert_at::<Item>(idx).is_ok(), l.insert_at::<Item>(idx).is_ok()) }
                        ("p", "ext") => { let mut l = d.lens_mut::<P>().unwrap(); (l.check_insert_at::<Ext>(idx).is_ok(), l.insert_at::<Ext>(idx).is_ok()) }
                        ("tbl", "pr") => { let mut l = d.lens_mut::<Tbl>().unwrap(); (l.check_insert_at::<Pr>(idx).is_ok(), l.insert_at::<Pr>(idx).is_ok()) }
                        ("tbl", "row") => { let mut l = d.lens_mut::<Tbl>().unwrap(); (l.check_insert_at::<Row>(idx).is_ok(), l.insert_at::<Row>(idx).is_ok()) }
                        _ => continue, // `mark` is ambiguous: AmbiguousSlot (tested above)
                    };
                    assert_eq!(check, ins, "{src} + {child} at {idx}");
                    if ins {
                        oks += 1;
                        let out: Vec<String> = d.root().elements().map(|e| e.name().local().to_owned()).collect();
                        assert!(valid(&out, slots), "{src} + {child} at {idx} -> {out:?}");
                    } else {
                        assert!(!d.is_dirty(), "{src} + {child} at {idx}: Err changed the tree");
                    }
                }
            }
        }
    }
    assert!(oks > 100, "{oks}");
}

#[test]
fn bulk_removal_preserves_surrounding_spans() {
    let input = b"<p xmlns='urn:t'><head/> <!-- remove --> <a/><b/><!-- keep --><item k='7'/></p>";
    let mut doc = XmlDocument::parse(input).unwrap();
    let mut lens = doc.lens_mut::<P>().unwrap();
    assert!(!lens.remove_range(1..99));
    assert!(lens.remove_range(1..1));
    assert_eq!(doc.serialize().as_bytes(), input);

    assert!(doc.lens_mut::<P>().unwrap().remove_range(1..6));
    assert_eq!(doc.serialize().as_bytes(), b"<p xmlns='urn:t'><head/><!-- keep --><item k='7'/></p>");
}

const AC_OPEN: &str = r#"<p xmlns="urn:t" xmlns:t="urn:t" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006">"#;

fn ac_doc(body: &str) -> (XmlDocument, String) {
    let source = format!("{AC_OPEN}{body}</p>");
    (doc(&source), source)
}

fn wrapping(choice: &str, fallback: Option<&str>) -> String {
    let fallback = fallback.map_or_default(|body| format!("<mc:Fallback>{body}</mc:Fallback>"));
    format!(r#"<mc:AlternateContent><mc:Choice Requires="t">{choice}</mc:Choice>{fallback}</mc:AlternateContent>"#)
}

#[test]
fn alternate_content_interval_boundaries_and_locality() {
    // Both configurations occupy [1, 3]; inserting in 0, at 1, or after 3
    // preserves the AC's exact bytes, including the whitespace after it.
    for ac in [wrapping("<a/><ext/>", Some("")), wrapping("<a/>", Some("<ext/>"))] {
        let (mut d, _) = ac_doc(&format!("{ac}<!--keep-->"));
        d.lens_mut::<P>().unwrap().insert::<Head>().unwrap();
        assert_eq!(d.serialize(), format!("{AC_OPEN}<head/>{ac}<!--keep--></p>"));
        let (mut d, _) = ac_doc(&format!("{ac}<!--keep-->"));
        d.lens_mut::<P>().unwrap().insert::<B>().unwrap();
        assert_eq!(d.serialize(), format!("{AC_OPEN}<b/>{ac}<!--keep--></p>"));
        let (mut d, before) = ac_doc(&ac);
        let mut p = d.lens_mut::<P>().unwrap();
        assert_matches!(p.check_insert::<Item>(), Err(Error::UnresolvedSibling(_)));
        assert_matches!(p.insert::<Item>(), Err(Error::UnresolvedSibling(_)));
        assert_eq!(d.serialize(), before);
        let (mut d, _) = ac_doc(&ac);
        d.lens_mut::<P>().unwrap().insert_with_ns::<W>(&[ooxml_core::NsDecl::new("o", "urn:o")]).unwrap();
        assert_eq!(names(&d), ["AlternateContent", "w"]);
    }
}

#[test]
fn alternate_content_leading_unlisted_extends_interval_to_previous_sibling() {
    let ac = wrapping("<unknown/><ext/>", None);
    let (mut d, source) = ac_doc(&format!("<head/>{ac}"));
    assert_matches!(d.lens_mut::<P>().unwrap().insert::<Item>(), Err(Error::UnresolvedSibling(_)));
    assert_eq!(d.serialize(), source);

    let (mut d, source) = ac_doc(&ac);
    assert_matches!(d.lens_mut::<P>().unwrap().insert::<Head>(), Err(Error::UnresolvedSibling(_)));
    assert_eq!(d.serialize(), source);
}

#[test]
fn alternate_content_position_modes() {
    for ac in [wrapping("<item/>", None), wrapping("", Some(""))] {
        let (mut d, source) = ac_doc(&format!("<item/>{ac}<item/>"));
        let mut p = d.lens_mut::<P>().unwrap();
        assert_matches!(p.check_insert_by::<Item>(|_, _| true), Err(Error::UnresolvedSibling(_)));
        assert_matches!(p.insert_by::<Item>(|_, _| true), Err(Error::UnresolvedSibling(_)));
        assert_matches!(p.check_insert_in_slot::<Item>(2, |_, _| false), Err(Error::UnresolvedSibling(_)));
        assert_matches!(p.insert_in_slot::<Item>(2, |_, _| false), Err(Error::UnresolvedSibling(_)));
        for index in 0..=3 {
            assert_matches!(p.check_insert_at::<Item>(index), Err(Error::UnresolvedSibling(_)));
            assert_matches!(p.insert_at::<Item>(index), Err(Error::UnresolvedSibling(_)));
        }
        assert_eq!(d.serialize(), source);
        let mut p = d.lens_mut::<P>().unwrap();
        p.check_insert::<Item>().unwrap();
        p.insert::<Item>().unwrap();
        assert_eq!(d.serialize(), format!("{AC_OPEN}<item/>{ac}<item/><item/></p>"));
    }

    let ac = wrapping("<head/>", None);
    let (mut d, _) = ac_doc(&format!("{ac}<item/>"));
    let mut p = d.lens_mut::<P>().unwrap();
    p.check_insert_at::<Item>(1).unwrap();
    p.insert_at::<Item>(1).unwrap();
    p.insert_by::<Item>(|i, _| i == 0).unwrap();
    assert_eq!(d.serialize(), format!("{AC_OPEN}{ac}<item/><item/><item/></p>"));
}

#[test]
fn alternate_content_limits_use_each_branches_maximum_recursively() {
    let nested = wrapping("<a/><b/>", None);
    let ac = wrapping("<a/>", Some(&nested));
    let (mut d, source) = ac_doc(&ac);
    let mut p = d.lens_mut::<P>().unwrap();
    assert_matches!(p.check_get_or_insert::<B>(), Err(Error::AlreadyPresent(_)));
    assert_matches!(p.get_or_insert::<B>(), Err(Error::AlreadyPresent(_)));
    assert_eq!(d.serialize(), source);

    let ac = wrapping("<a/>", Some("<b/>"));
    let (mut d, _) = ac_doc(&ac);
    d.lens_mut::<P>().unwrap().insert::<A>().unwrap();
    assert_eq!(d.serialize(), format!("{AC_OPEN}{ac}<a/></p>"));

    let ac = wrapping("", Some("<head/>"));
    let (mut d, source) = ac_doc(&ac);
    assert_matches!(d.lens_mut::<P>().unwrap().get_or_insert::<Head>(), Err(Error::AlreadyPresent(_)));
    assert_eq!(d.serialize(), source);
}

#[test]
fn alternate_content_validates_every_choice_and_branch() {
    let empty_ac = wrapping("", None);
    for branches in [
        String::new(),
        "<mc:Fallback/>".into(),
        r#"<mc:Choice Requires="t"/><mc:Fallback/><mc:Fallback/>"#.into(),
        r#"<mc:Fallback/><mc:Choice Requires="t"/>"#.into(),
        r#"<mc:Choice Requires="t"/><item/>"#.into(),
        r#"<mc:Choice><item/></mc:Choice>"#.into(),
        r#"<mc:Choice Requires="   "><item/></mc:Choice>"#.into(),
        r#"<mc:Choice Requires="t missing"><item/></mc:Choice>"#.into(),
        r#"<mc:Choice Requires="t" xmlns:sibling="urn:s"/><mc:Choice Requires="sibling"/>"#.into(),
        r#"<mc:Choice Requires="t"><unknown/></mc:Choice>"#.into(),
        format!(r#"<mc:Choice Requires="t">{empty_ac}</mc:Choice>"#),
        r#"<mc:Choice Requires="t"><ext/><a/></mc:Choice>"#.into(),
    ] {
        let ac = format!("<mc:AlternateContent>{branches}</mc:AlternateContent>");
        let (mut d, before) = ac_doc(&ac);
        let mut p = d.lens_mut::<P>().unwrap();
        assert_matches!(p.insert::<Head>(), Err(Error::UnresolvedSibling(_)), "{branches}");
        assert_matches!(p.insert_by::<Head>(|_, _| false), Err(Error::UnresolvedSibling(_)), "{branches}");
        // Positional insertion checks branch shape and also the greedy order.
        assert_matches!(p.insert_at::<Head>(0), Err(Error::UnresolvedSibling(_)), "{branches}");
        assert_eq!(d.serialize(), before);
    }

    let ac = r#"<mc:AlternateContent xmlns:outer="urn:outer"><mc:Choice Requires="outer local" xmlns:local="urn:local"><item/></mc:Choice></mc:AlternateContent>"#;
    let (mut d, _) = ac_doc(ac);
    d.lens_mut::<P>().unwrap().insert::<Head>().unwrap();
    assert_eq!(d.serialize(), format!("{AC_OPEN}<head/>{ac}</p>"));
}

#[test]
fn alternate_content_greedy_slots_and_conservative_positional_neighbours() {
    let ac = wrapping("<mark/>", None);
    let mc = "http://schemas.openxmlformats.org/markup-compatibility/2006";
    let before = format!(r#"<tbl xmlns="urn:t" xmlns:t="urn:t" xmlns:mc="{mc}">{ac}<pr/></tbl>"#);
    let mut d = doc(&before);
    let mut t = d.lens_mut::<Tbl>().unwrap();
    // The greedy rank is 0, but the candidates for the AC are 0 and 2.
    assert_matches!(t.insert_at::<Row>(2), Err(Error::UnresolvedSibling(_)));
    assert_eq!(d.serialize(), before);
    d.lens_mut::<Tbl>().unwrap().insert::<Row>().unwrap();
    assert_eq!(names(&d), ["AlternateContent", "pr", "row"]);

    let (mut d, before) = ac_doc(&format!("{}{}", wrapping("<ext/>", Some("")), wrapping("", Some("<a/>"))));
    assert_matches!(d.lens_mut::<P>().unwrap().insert::<Head>(), Err(Error::UnresolvedSibling(_)));
    assert_eq!(d.serialize(), before);
}

#[test]
fn alternate_content_placement_agrees_with_expanding_each_branch() {
    // Independent schema model for the test sequences below. This checks the
    // actual output (including the placement chosen by the lens) after
    // substituting each possible Choice or Fallback, recursively.
    fn valid(parent: &ooxml_core::RawNode) -> bool {
        fn expand(parent: &ooxml_core::RawNode) -> Vec<Vec<usize>> {
            let mut prefixes = vec![vec![]];
            for member in parent.elements() {
                let alternatives = if member.is(ooxml_core::name::MC_ALTERNATE_CONTENT) {
                    let mut branches: Vec<_> = member.elements().flat_map(expand).collect();
                    if member.elements().all(|branch| branch.name().local() != "Fallback") {
                        branches.push(vec![]);
                    }
                    branches
                } else {
                    let rank = match member.name().local() {
                        "head" => 0,
                        "a" | "b" => 1,
                        "item" => 2,
                        "ext" => 3,
                        name => panic!("unexpected member {name}"),
                    };
                    vec![vec![rank]]
                };
                prefixes = prefixes.into_iter().flat_map(|prefix| {
                    alternatives.iter().map(move |branch| [prefix.as_slice(), branch].concat())
                }).collect();
            }
            prefixes
        }
        expand(parent).iter().all(|ranks| {
            ranks.is_sorted() && [(0, 1), (1, 2), (3, 1)].iter().all(|&(slot, max)| {
                ranks.iter().filter(|&&r| r == slot).count() <= max
            })
        })
    }

    fn exercise<C: ChildOf<P>>(parts: &[&str], new: &str, positional_refused: bool) {
        let (_, source) = ac_doc(&parts.concat());
        let possible_at: Vec<_> = (0..=parts.len()).filter(|&index| {
            let (expanded, _) = ac_doc(&format!("{}{new}{}", parts[..index].concat(), parts[index..].concat()));
            valid(expanded.root())
        }).collect();

        let mut d = doc(&source);
        let mut parent = d.lens_mut::<P>().unwrap();
        let preflight = parent.check_insert::<C>();
        assert_eq!(parent.insert::<C>().is_ok(), preflight.is_ok());
        assert_eq!(preflight.is_ok(), !possible_at.is_empty(), "{source}");
        if preflight.is_ok() {
            assert!(valid(d.root()), "{}", d.serialize());
        } else {
            assert_eq!(d.serialize(), source);
        }

        for index in 0..=parts.len() {
            let mut d = doc(&source);
            let mut parent = d.lens_mut::<P>().unwrap();
            let preflight = parent.check_insert_at::<C>(index);
            let result = parent.insert_at::<C>(index);
            assert_eq!(result.is_ok(), preflight.is_ok());
            assert_eq!(result.is_ok(), !positional_refused && possible_at.contains(&index));
            if result.is_ok() {
                assert!(valid(d.root()), "{}", d.serialize());
            } else {
                assert_eq!(d.serialize(), source);
            }
        }

        let mut d = doc(&source);
        let mut parent = d.lens_mut::<P>().unwrap();
        let preflight = parent.check_insert_by::<C>(|_, _| true);
        assert_eq!(parent.insert_by::<C>(|_, _| true).is_ok(), preflight.is_ok());
        assert_eq!(preflight.is_ok(), !positional_refused && !possible_at.is_empty());
        if preflight.is_ok() {
            assert!(valid(d.root()), "{}", d.serialize());
        } else {
            assert_eq!(d.serialize(), source);
        }
    }

    let spanning = wrapping("<a/><item/>", Some(""));
    let split = wrapping("<a/>", Some("<ext/>"));
    let nested = wrapping(&wrapping("<item/>", Some("")), None);
    let empty = wrapping("", Some(""));
    exercise::<Head>(&[&spanning], "<head/>", false);
    exercise::<B>(&[&spanning], "<b/>", true); // lo
    exercise::<Item>(&[&spanning], "<item/>", true); // hi
    exercise::<Ext>(&[&spanning], "<ext/>", false);
    exercise::<Item>(&[&split], "<item/>", true); // straddle
    exercise::<Item>(&["<head/>", &nested, "<ext/>"], "<item/>", true);
    exercise::<Item>(&["<head/>", &empty, "<ext/>"], "<item/>", true);
    exercise::<Head>(&["<head/>", &nested], "<head/>", false);
}

#[test]
fn removing_alternate_content_restores_local_unlimited_slot_checks() {
    let ac = wrapping("<item/>", None);
    let (mut d, _) = ac_doc(&format!("<ext/><head/>{ac}<item/>"));
    let mut p = d.lens_mut::<P>().unwrap();
    assert!(p.remove_at(2));
    // The AC bit is still set. The affected neighbourhood is in order.
    p.check_insert_at::<Item>(3).unwrap();
    p.insert_at::<Item>(3).unwrap();
    assert_eq!(d.serialize(), format!("{AC_OPEN}<ext/><head/><item/><item/></p>"));
}
