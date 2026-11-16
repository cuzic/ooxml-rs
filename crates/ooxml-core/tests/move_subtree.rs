//! `LensMut::move_in[_by]` / `check_move_in_by` (ADR-0002 post-acceptance
//! note "same-part move") with hand-written specs: placement, dirty
//! duties, T2-style locality (the moved bytes leave the source and appear
//! at the destination, nothing else changes), refusals before any change,
//! and a brute-force check that `check_move_in_by` agrees with the move.

use std::assert_matches;

use ooxml_core::{ChildOf, Content, ElementSpec, Error, Lens, MoveRefusal, MoveSource, ParseOptions, QName, Slot, XmlDocument};

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
    ($t:ident, $l:literal, $content:expr) => {
        #[derive(Debug)]
        enum $t {}
        impl ElementSpec for $t {
            const QNAME: QName = q($l);
            const SDK_NAME: &'static str = concat!("t:", $l);
            const ATTRS: &'static [ooxml_core::AttrSpec] = &[];
            const CONTENT: Content = $content;
        }
    };
}

// <doc>: box*   <box>: head? item* ext?   <item>: (item|sub)*   <sub/>
spec!(Doc, "doc", Content::Slots(&[many(&[q("box")])]));
spec!(Box_, "box", Content::Slots(&[one(&[q("head")]), many(&[q("item")]), one(&[q("ext")])]));
spec!(Head, "head", Content::Empty);
spec!(Ext, "ext", Content::Empty);
spec!(Item, "item", Content::Slots(&[many(&[q("item"), q("sub")])]));
spec!(Sub, "sub", Content::Empty);
impl ChildOf<Doc> for Box_ {}
impl ChildOf<Box_> for Head {}
impl ChildOf<Box_> for Item {}
impl ChildOf<Box_> for Ext {}
impl ChildOf<Item> for Item {}
impl ChildOf<Item> for Sub {}

fn doc(s: &str) -> XmlDocument {
    XmlDocument::parse(s.as_bytes()).unwrap()
}

/// The element at `path` as a typed move source.
fn at<T: ElementSpec>(d: &XmlDocument, path: &[usize]) -> MoveSource<T> {
    Lens::<T>::at(d.root(), path).unwrap().move_source()
}

/// The source with the bytes `src[a..b]` moved to offset `at` of the
/// remaining text.
fn moved(src: &str, a: usize, b: usize, at: usize) -> String {
    let piece = &src[a..b];
    let rest = format!("{}{}", &src[..a], &src[b..]);
    format!("{}{piece}{}", &rest[..at], &rest[at..])
}

/// Is `out` the source with exactly the bytes `src[a..b]` moved somewhere?
fn is_pure_move(src: &str, out: &str, a: usize, b: usize) -> bool {
    let rest = format!("{}{}", &src[..a], &src[b..]);
    out.len() == src.len() && (0..=rest.len()).any(|k| rest.is_char_boundary(k) && moved(src, a, b, k) == out)
}

const SRC: &str = concat!(
    r#"<x:doc xmlns:x="urn:t">"#,
    r#"<x:box><x:head/><x:item k="1"><x:sub/></x:item><x:item k="2"/><x:item k="3"/><x:ext/></x:box>"#,
    r#"<x:box><x:item k="4"></x:item></x:box>"#,
    r#"<x:box/>"#,
    r#"</x:doc>"#
);

#[test]
fn reorder_within_a_parent_is_a_pure_move() {
    let mut d = doc(SRC);
    let a = SRC.find(r#"<x:item k="3"/>"#).unwrap();
    let b = a + r#"<x:item k="3"/>"#.len();
    let s = at::<Item>(&d, &[0, 3]);
    let mut doc_lens = d.lens_mut::<Doc>().unwrap();
    let mut box0 = doc_lens.nth_child_mut::<Box_>(0).unwrap();
    // item k=3 (child index 3 of box0) before the first item of the slot
    let moved_lens = box0.move_in_by::<Item>(&s, |k, _| k == 0).unwrap();
    assert_eq!(moved_lens.path(), &[0, 1]);
    let out = d.serialize();
    assert!(is_pure_move(SRC, &out, a, b), "{out}");
    assert_eq!(out, moved(SRC, a, b, SRC.find(r#"<x:item k="1">"#).unwrap()));
    // the untouched boxes are not dirty
    let root = d.root();
    assert!(!root.elements().nth(1).unwrap().subtree_dirty());
    assert!(!root.elements().nth(2).unwrap().subtree_dirty());
}

#[test]
fn move_across_parents_and_dirty_duties() {
    // item k=4 from box1 (which keeps its close tag when emptied) into the
    // self-closing box2 (re-rendered: it gains its first child)
    let mut d = doc(SRC);
    let s = at::<Item>(&d, &[1, 0]);
    d.lens_mut::<Doc>().unwrap().nth_child_mut::<Box_>(2).unwrap().move_in::<Item>(&s).unwrap();
    let out = d.serialize();
    assert_eq!(
        out,
        SRC.replace(r#"<x:box><x:item k="4"></x:item></x:box>"#, "<x:box></x:box>")
            .replace("<x:box/>", r#"<x:box><x:item k="4"></x:item></x:box>"#)
    );
    let boxes: Vec<_> = d.root().elements().collect();
    assert!(boxes[1].subtree_dirty() && !boxes[1].own_dirty(), "the source parent keeps its tags");
    assert!(boxes[2].own_dirty(), "a self-closing parent gaining a child is re-rendered");
    assert!(!boxes[2].elements().next().unwrap().subtree_dirty(), "the moved element keeps its bytes");
}

#[test]
fn the_destination_path_follows_the_removal() {
    // item k=2 ([0, 2]) into item k=3 ([0, 3]): after the removal the
    // destination is [0, 2]; the returned lens points at the moved node
    let mut d = doc(SRC);
    let s = at::<Item>(&d, &[0, 2]);
    let mut l = d.lens_mut::<Doc>().unwrap();
    let mut b = l.nth_child_mut::<Box_>(0).unwrap();
    let mut item3 = b.nth_child_mut::<Item>(2).unwrap();
    assert_eq!(item3.path(), &[0, 3]);
    let m = item3.move_in::<Item>(&s).unwrap();
    assert_eq!(m.path(), &[0, 2, 0]);
    assert_eq!(m.node().attr(QName::new("", "k")), Some("2"));
    let out = d.serialize();
    // item k=3 was self-closing and gains a child: re-rendered open tag
    assert_eq!(out, SRC.replace(r#"<x:item k="2"/><x:item k="3"/>"#, r#"<x:item k="3"><x:item k="2"/></x:item>"#));
}

fn assert_refused(src: &str, f: impl FnOnce(&mut XmlDocument) -> Result<(), Error>) -> Error {
    let mut d = doc(src);
    let e = f(&mut d).unwrap_err();
    assert_eq!(d.serialize(), src, "refused move changed the tree ({e})");
    assert!(!d.root().subtree_dirty(), "refused move marked the tree");
    e
}

#[test]
fn refusals_change_nothing() {
    // the root (an item-rooted part, so the source can be typed as an item)
    let isrc = r#"<x:item xmlns:x="urn:t"><x:item/></x:item>"#;
    let e = assert_refused(isrc, |d| {
        let s = at::<Item>(d, &[]);
        d.lens_mut::<Item>()?.nth_child_mut::<Item>(0).unwrap().move_in::<Item>(&s).map(|_| ())
    });
    assert_matches!(e, Error::NotMovable { reason: MoveRefusal::Root, .. });
    // into itself: item k=1 into its own descendant position (itself)
    let e = assert_refused(SRC, |d| {
        let s = at::<Item>(d, &[0, 1]);
        let mut l = d.lens_mut::<Doc>()?;
        let mut b = l.nth_child_mut::<Box_>(0).unwrap();
        let mut item1 = b.nth_child_mut::<Item>(0).unwrap();
        item1.move_in::<Item>(&s).map(|_| ())
    });
    assert_matches!(e, Error::NotMovable { reason: MoveRefusal::IntoItself, .. });
    // a stale source: the path leads to another element, or to nothing
    let other = doc(r#"<x:doc xmlns:x="urn:t"><x:box><x:item/></x:box><x:box/><x:box/><x:box/><x:box/><x:box/><x:box/><x:box><x:item/></x:box></x:doc>"#);
    let (stale_head, stale_none) = (at::<Item>(&other, &[0, 0]), at::<Item>(&other, &[7, 0]));
    let e = assert_refused(SRC, |d| d.lens_mut::<Doc>()?.nth_child_mut::<Box_>(2).unwrap().move_in::<Item>(&stale_head).map(|_| ()));
    assert_matches!(e, Error::WrongElement { .. });
    let e = assert_refused(SRC, |d| d.lens_mut::<Doc>()?.nth_child_mut::<Box_>(2).unwrap().move_in::<Item>(&stale_none).map(|_| ()));
    assert_matches!(e, Error::WrongElement { .. });
    // slot limit: box0's head into box1 which ... (box1 has no head: allowed) — into a box that has one:
    let src = r#"<x:doc xmlns:x="urn:t"><x:box><x:head/></x:box><x:box><x:head/></x:box></x:doc>"#;
    let e = assert_refused(src, |d| { let s = at::<Head>(d, &[0, 0]); d.lens_mut::<Doc>()?.nth_child_mut::<Box_>(1).unwrap().move_in::<Head>(&s).map(|_| ()) });
    assert_matches!(e, Error::AlreadyPresent(_));
    // ... but moving an element within its own parent never counts itself
    let mut d = doc(src);
    let s = at::<Head>(&d, &[0, 0]);
    d.lens_mut::<Doc>().unwrap().nth_child_mut::<Box_>(0).unwrap().move_in::<Head>(&s).unwrap();
    assert_eq!(d.serialize(), src);
    // out-of-order children at the destination
    let src = r#"<x:doc xmlns:x="urn:t"><x:box><x:item/></x:box><x:box><x:ext/><x:head/></x:box></x:doc>"#;
    let e = assert_refused(src, |d| { let s = at::<Item>(d, &[0, 0]); d.lens_mut::<Doc>()?.nth_child_mut::<Box_>(1).unwrap().move_in::<Item>(&s).map(|_| ()) });
    assert_matches!(e, Error::UnresolvedSibling(_));
}

#[test]
fn meaning_must_not_change() {
    // a prefix declared on the source box only
    let src = r#"<x:doc xmlns:x="urn:t"><x:box xmlns:y="urn:y"><x:item y:k="1"/></x:box><x:box/></x:doc>"#;
    let e = assert_refused(src, |d| { let s = at::<Item>(d, &[0, 0]); d.lens_mut::<Doc>()?.nth_child_mut::<Box_>(1).unwrap().move_in::<Item>(&s).map(|_| ()) });
    assert_matches!(e, Error::NotMovable { reason: MoveRefusal::NamespaceBinding, ref name } if name == "y");
    // ... also when only a descendant of the moved element uses it
    let src = r#"<x:doc xmlns:x="urn:t"><x:box xmlns:y="urn:y"><x:item><x:sub y:k="1"/></x:item></x:box><x:box/></x:doc>"#;
    let e = assert_refused(src, |d| { let s = at::<Item>(d, &[0, 0]); d.lens_mut::<Doc>()?.nth_child_mut::<Box_>(1).unwrap().move_in::<Item>(&s).map(|_| ()) });
    assert_matches!(e, Error::NotMovable { reason: MoveRefusal::NamespaceBinding, ref name } if name == "y");
    // the same prefix bound to another URI at the destination
    let src = r#"<x:doc xmlns:x="urn:t" xmlns:y="urn:y"><x:box><x:item y:k="1"/></x:box><x:box xmlns:y="urn:other"/></x:doc>"#;
    let e = assert_refused(src, |d| { let s = at::<Item>(d, &[0, 0]); d.lens_mut::<Doc>()?.nth_child_mut::<Box_>(1).unwrap().move_in::<Item>(&s).map(|_| ()) });
    assert_matches!(e, Error::NotMovable { reason: MoveRefusal::NamespaceBinding, .. });
    // the default namespace re-bound at the destination
    let src = r#"<doc xmlns="urn:t"><box><item/></box><x:box xmlns:x="urn:t" xmlns="urn:t2"/></doc>"#;
    let e = assert_refused(src, |d| { let s = at::<Item>(d, &[0, 0]); d.lens_mut::<Doc>()?.nth_child_mut::<Box_>(1).unwrap().move_in::<Item>(&s).map(|_| ()) });
    assert_matches!(e, Error::NotMovable { reason: MoveRefusal::NamespaceBinding, ref name } if name == "(default namespace)");
    // a declaration inside the moved subtree travels with it
    let src = r#"<x:doc xmlns:x="urn:t"><x:box><x:item xmlns:y="urn:y" y:k="1"/></x:box><x:box></x:box></x:doc>"#;
    let mut d = doc(src);
    let s = at::<Item>(&d, &[0, 0]);
    d.lens_mut::<Doc>().unwrap().nth_child_mut::<Box_>(1).unwrap().move_in::<Item>(&s).unwrap();
    assert_eq!(d.serialize(), r#"<x:doc xmlns:x="urn:t"><x:box></x:box><x:box><x:item xmlns:y="urn:y" y:k="1"/></x:box></x:doc>"#);
    // ignorable at the source only
    let src = r#"<x:doc xmlns:x="urn:t" xmlns:e="urn:e" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><x:box mc:Ignorable="e"><x:item e:k="1"/></x:box><x:box/></x:doc>"#;
    let e = assert_refused(src, |d| { let s = at::<Item>(d, &[0, 0]); d.lens_mut::<Doc>()?.nth_child_mut::<Box_>(1).unwrap().move_in::<Item>(&s).map(|_| ()) });
    assert_matches!(e, Error::NotMovable { reason: MoveRefusal::IgnorableNamespace, ref name } if name == "urn:e");
    // text under a different inherited xml:space
    let src = r#"<x:doc xmlns:x="urn:t"><x:box xml:space="preserve"><x:item>  </x:item></x:box><x:box/></x:doc>"#;
    let e = assert_refused(src, |d| { let s = at::<Item>(d, &[0, 0]); d.lens_mut::<Doc>()?.nth_child_mut::<Box_>(1).unwrap().move_in::<Item>(&s).map(|_| ()) });
    assert_matches!(e, Error::NotMovable { reason: MoveRefusal::XmlSpace, .. });
    // ... unless the subtree states its own
    let src = r#"<x:doc xmlns:x="urn:t"><x:box xml:space="preserve"><x:item xml:space="preserve">  </x:item></x:box><x:box/></x:doc>"#;
    let mut d2 = doc(src);
    let s = at::<Item>(&d2, &[0, 0]);
    d2.lens_mut::<Doc>().unwrap().nth_child_mut::<Box_>(1).unwrap().move_in::<Item>(&s).unwrap();
}

#[test]
fn the_whole_subtree_obeys_the_depth_limit() {
    // doc 1, box 2, item 3, sub 4: moving item (height 2) under box1 is fine at 4, not under an item at 4
    let src = r#"<x:doc xmlns:x="urn:t"><x:box><x:item><x:sub/></x:item><x:item/></x:box></x:doc>"#;
    let mut d = XmlDocument::parse_with(src.as_bytes(), &ParseOptions::default().with_max_depth(4)).unwrap();
    let e = {
        let s = at::<Item>(&d, &[0, 0]);
        let mut l = d.lens_mut::<Doc>().unwrap();
        let mut b = l.nth_child_mut::<Box_>(0).unwrap();
        let mut item2 = b.nth_child_mut::<Item>(1).unwrap();
        item2.move_in::<Item>(&s).map(|_| ()).unwrap_err()
    };
    assert_matches!(e, Error::TooDeep { limit: 4, .. });
    assert_eq!(d.serialize(), src);
}

/// Every (source item, destination container, position) of a small tree:
/// `check_move_in_by` and `move_in_by` agree; a refusal changes nothing;
/// a success is a pure move of the element's bytes.
#[test]
fn brute_force_check_agrees_with_move() {
    brute_force(concat!(
        r#"<x:doc xmlns:x="urn:t">"#,
        r#"<x:box><x:head></x:head><x:item k="a"><x:sub></x:sub><x:item k="b"></x:item></x:item><x:item k="c"></x:item><x:ext></x:ext></x:box>"#,
        r#"<x:box><x:item k="d"><x:item k="e"></x:item></x:item></x:box>"#,
        r#"</x:doc>"#
    ), (20, 5));
}

/// The same over boxes that differ in declarations, `mc:Ignorable` and
/// `xml:space`, so every meaning check takes both branches (review of
/// PR #44, m1).
#[test]
fn brute_force_over_differing_contexts() {
    brute_force(concat!(
        r#"<x:doc xmlns:x="urn:t" xmlns:y="urn:y" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006">"#,
        r#"<x:box><x:item k="a" y:k="1"><x:item k="b"></x:item></x:item><x:item k="c">t</x:item></x:box>"#,
        r#"<x:box xmlns:y="urn:other"><x:item k="d"></x:item></x:box>"#,
        r#"<x:box mc:Ignorable="y"><x:item k="e" y:k="2"></x:item></x:box>"#,
        r#"<x:box xml:space="preserve"><x:item k="f">u</x:item></x:box>"#,
        r#"<x:box xmlns:z="urn:z"><x:item k="g" xmlns:y="urn:own" y:k="3" mc:Ignorable="z"></x:item></x:box>"#,
        r#"</x:doc>"#
    ), (20, 40));
}

fn brute_force(src: &str, (min_ok, min_err): (usize, usize)) {
    let base = doc(src);
    // every element path below the boxes
    fn paths(n: &ooxml_core::RawNode, at: Vec<usize>, out: &mut Vec<Vec<usize>>) {
        for (i, c) in n.indexed_elements() {
            let mut p = at.clone();
            p.push(i);
            out.push(p.clone());
            paths(c, p, out);
        }
    }
    let mut all = Vec::new();
    paths(base.root(), Vec::new(), &mut all);
    let items: Vec<Vec<usize>> = all.iter().filter(|p| node(&base, p).is(q("item"))).cloned().collect();
    let boxes: Vec<Vec<usize>> = all.iter().filter(|p| node(&base, p).is(q("box"))).cloned().collect();
    let containers: Vec<Vec<usize>> = items.iter().chain(&boxes).cloned().collect();
    let (mut oks, mut errs) = (0, 0);
    for from in &items {
        let span = node(&base, from).span().unwrap();
        for dst in &containers {
            for pos in 0..4usize {
                let mut d = doc(src);
                let from = &at::<Item>(&d, from);
                let before = move |k: usize, _: &ooxml_core::RawNode| k == pos;
                let (checked, applied) = if node(&base, dst).is(q("box")) {
                    let mut l = lens_at::<Box_>(&mut d, dst);
                    let c = l.check_move_in_by::<Item>(from, before);
                    (c, l.move_in_by::<Item>(from, before).map(|_| ()))
                } else {
                    let mut l = lens_at::<Item>(&mut d, dst);
                    let c = l.check_move_in_by::<Item>(from, before);
                    (c, l.move_in_by::<Item>(from, before).map(|_| ()))
                };
                assert_eq!(checked.is_ok(), applied.is_ok(), "check vs move: {from:?} -> {dst:?} @{pos}: {checked:?} / {applied:?}");
                let out = d.serialize();
                match applied {
                    Ok(()) => {
                        oks += 1;
                        assert!(is_pure_move(src, &out, span.start, span.end()), "{from:?} -> {dst:?} @{pos}:\n{out}");
                        XmlDocument::parse(out.as_bytes()).unwrap();
                    }
                    Err(_) => {
                        errs += 1;
                        assert_eq!(out, src);
                        assert!(!d.root().subtree_dirty());
                    }
                }
            }
        }
    }
    assert!(oks > min_ok && errs > min_err, "oks {oks}, errs {errs}");
}

fn node<'a>(d: &'a XmlDocument, path: &[usize]) -> &'a ooxml_core::RawNode {
    path.iter().fold(d.root(), |n, &i| n.children()[i].as_element().unwrap())
}

/// A typed lens at `path` (doc → box → item… by child indices).
fn lens_at<'d, T: ElementSpec>(d: &'d mut XmlDocument, path: &[usize]) -> ooxml_core::LensMut<'d, T> {
    let (&b, rest) = path.split_first().unwrap();
    let doc_lens = d.lens_mut::<Doc>().unwrap();
    let box_lens = doc_lens.into_child_at::<Box_>(b).unwrap();
    if rest.is_empty() {
        return box_lens.cast::<T>().unwrap();
    }
    let mut item = box_lens.into_child_at::<Item>(rest[0]).unwrap();
    for &i in &rest[1..] {
        item = item.into_child_at::<Item>(i).unwrap();
    }
    item.cast::<T>().unwrap()
}

#[test]
fn other_child_types_move_too() {
    // a sub from item a into item c; the ext of box0 into box1 (end of its slot order)
    let mut d = doc(SRC);
    {
        let s = at::<Sub>(&d, &[0, 1, 0]);
        let mut l = d.lens_mut::<Doc>().unwrap();
        let mut b = l.nth_child_mut::<Box_>(0).unwrap();
        b.nth_child_mut::<Item>(1).unwrap().move_in::<Sub>(&s).unwrap();
    }
    let s = at::<Ext>(&d, &[0, 4]);
    d.lens_mut::<Doc>().unwrap().nth_child_mut::<Box_>(1).unwrap().move_in::<Ext>(&s).unwrap();
    assert_eq!(
        d.serialize(),
        concat!(
            r#"<x:doc xmlns:x="urn:t">"#,
            r#"<x:box><x:head/><x:item k="1"></x:item><x:item k="2"><x:sub/></x:item><x:item k="3"/></x:box>"#,
            r#"<x:box><x:item k="4"></x:item><x:ext/></x:box>"#,
            r#"<x:box/>"#,
            r#"</x:doc>"#
        )
    );
}

const MC: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";

fn move_item_to_box1(src: &str) -> Result<(), Error> {
    let mut d = doc(src);
    let s = at::<Item>(&d, &[0, 0]);
    let r = d.lens_mut::<Doc>()?.nth_child_mut::<Box_>(1).unwrap().move_in::<Item>(&s).map(|_| ());
    if r.is_err() {
        assert_eq!(d.serialize(), src);
        assert!(!d.root().subtree_dirty());
    }
    r
}

/// Prefixes named in Markup Compatibility attribute values must keep their
/// binding too (review of PR #44, M1).
#[test]
fn prefixes_in_markup_compatibility_values_must_stay_bound() {
    for (attr_owner, why) in [
        (r#"<x:item mc:Ignorable="y"/>"#, "mc:Ignorable"),
        (r#"<x:item mc:MustUnderstand="y"/>"#, "mc:MustUnderstand"),
        (r#"<x:item mc:ProcessContent="y:*"/>"#, "mc:ProcessContent"),
        (r#"<x:item mc:PreserveElements="y:a"/>"#, "mc:PreserveElements"),
        (r#"<x:item mc:PreserveAttributes="y:a y:b"/>"#, "mc:PreserveAttributes"),
        (r#"<x:item><mc:AlternateContent><mc:Choice Requires="y"><x:sub/></mc:Choice><mc:Fallback/></mc:AlternateContent></x:item>"#, "mc:Choice/@Requires"),
    ] {
        let src = format!(r#"<x:doc xmlns:x="urn:t" xmlns:mc="{MC}"><x:box xmlns:y="urn:y">{attr_owner}</x:box><x:box/></x:doc>"#);
        assert_matches!(move_item_to_box1(&src), Err(Error::NotMovable { reason: MoveRefusal::NamespaceBinding, ref name }) if name == "y", "{why}");
        // bound the same way at the destination: fine
        let src = format!(r#"<x:doc xmlns:x="urn:t" xmlns:mc="{MC}" xmlns:y="urn:y"><x:box>{attr_owner}</x:box><x:box/></x:doc>"#);
        move_item_to_box1(&src).unwrap_or_else(|e| panic!("{why}: {e}"));
        // declared inside the subtree: travels with it
        let own = attr_owner.replacen("<x:item", r#"<x:item xmlns:y="urn:own""#, 1);
        let src = format!(r#"<x:doc xmlns:x="urn:t" xmlns:mc="{MC}"><x:box xmlns:y="urn:y">{own}</x:box><x:box xmlns:y="urn:other"/></x:doc>"#);
        move_item_to_box1(&src).unwrap_or_else(|e| panic!("{why} (own declaration): {e}"));
    }
    // `Requires` on another element is an ordinary attribute
    let src = r#"<x:doc xmlns:x="urn:t"><x:box xmlns:y="urn:y"><x:item Requires="y"/></x:box><x:box/></x:doc>"#;
    move_item_to_box1(src).unwrap();
}

/// A subtree's own declarations are not compared with outer scopes, and a
/// destination binding the same prefix to the same URI is fine (review of
/// PR #44, m3a; PR #45 n4).
#[test]
fn self_declared_prefixes_travel_with_the_subtree() {
    for dst in [r#"<x:box xmlns:y="urn:other"/>"#, r#"<x:box xmlns:y="urn:own"/>"#, "<x:box/>"] {
        let src = format!(r#"<x:doc xmlns:x="urn:t"><x:box xmlns:y="urn:y"><x:item xmlns:y="urn:own" y:k="1"><x:sub y:k="2"/></x:item></x:box>{dst}</x:doc>"#);
        move_item_to_box1(&src).unwrap_or_else(|e| panic!("{dst}: {e}"));
    }
    // a prefix declared on a descendant only covers that descendant
    let src = r#"<x:doc xmlns:x="urn:t"><x:box xmlns:y="urn:y"><x:item y:k="1"><x:sub xmlns:y="urn:own"/></x:item></x:box><x:box/></x:doc>"#;
    assert_matches!(move_item_to_box1(src), Err(Error::NotMovable { reason: MoveRefusal::NamespaceBinding, .. }));
}

/// The `mc:Ignorable` and `xml:space` checks in both directions, a
/// subtree's own `xml:space="default"`, a redeclared ignorable prefix, and
/// indentation (review of PR #44, m1 mutants B, C, D, F; m3b).
#[test]
fn meaning_checks_in_both_directions() {
    // B: ignorable at the destination only
    let src = format!(r#"<x:doc xmlns:x="urn:t" xmlns:e="urn:e" xmlns:mc="{MC}"><x:box><x:item e:k="1"/></x:box><x:box mc:Ignorable="e"/></x:doc>"#);
    assert_matches!(move_item_to_box1(&src), Err(Error::NotMovable { reason: MoveRefusal::IgnorableNamespace, .. }));
    // F: the Ignorable prefix resolved with the innermost declaration (urn:e2, not the root's urn:e1)
    let src = format!(
        r#"<x:doc xmlns:x="urn:t" xmlns:e="urn:e1" xmlns:mc="{MC}"><x:box xmlns:e="urn:e2" mc:Ignorable="e"><x:item xmlns:f="urn:e2" f:k="1"/></x:box><x:box/></x:doc>"#
    );
    assert_matches!(move_item_to_box1(&src), Err(Error::NotMovable { reason: MoveRefusal::IgnorableNamespace, ref name }) if name == "urn:e2");
    // C: default at the source, preserve at the destination
    let src = r#"<x:doc xmlns:x="urn:t"><x:box><x:item> a </x:item></x:box><x:box xml:space="preserve"/></x:doc>"#;
    assert_matches!(move_item_to_box1(src), Err(Error::NotMovable { reason: MoveRefusal::XmlSpace, .. }));
    // D: the subtree states xml:space="default" itself: fine either way
    let src = r#"<x:doc xmlns:x="urn:t"><x:box xml:space="preserve"><x:item xml:space="default"> a </x:item></x:box><x:box/></x:doc>"#;
    move_item_to_box1(src).unwrap();
    // m3b: indentation between child elements is not content
    let src = "<x:doc xmlns:x=\"urn:t\"><x:box xml:space=\"preserve\"><x:item>\n  <x:sub/>\n</x:item></x:box><x:box/></x:doc>";
    move_item_to_box1(src).unwrap();
    // ... but text next to elements is
    let src = r#"<x:doc xmlns:x="urn:t"><x:box xml:space="preserve"><x:item> a <x:sub/></x:item></x:box><x:box/></x:doc>"#;
    assert_matches!(move_item_to_box1(src), Err(Error::NotMovable { reason: MoveRefusal::XmlSpace, .. }));
}

/// A: a move that ends exactly at the depth limit succeeds and reparses.
#[test]
fn a_move_to_exactly_the_depth_limit_succeeds() {
    // doc 1, box 2, item 3, sub 4: item (height 2) from box0 into box1 lands at 3..4
    let src = r#"<x:doc xmlns:x="urn:t"><x:box><x:item><x:sub/></x:item></x:box><x:box/></x:doc>"#;
    let opts = ParseOptions::default().with_max_depth(4);
    let mut d = XmlDocument::parse_with(src.as_bytes(), &opts).unwrap();
    let s = at::<Item>(&d, &[0, 0]);
    d.lens_mut::<Doc>().unwrap().nth_child_mut::<Box_>(1).unwrap().move_in::<Item>(&s).unwrap();
    XmlDocument::parse_with(d.serialize().as_bytes(), &opts).unwrap();
}

/// Out of, or into, an `mc:AlternateContent` branch is refused (review of
/// PR #44, m5).
#[test]
fn alternate_content_branches_are_not_moved_across() {
    let src = format!(
        r#"<x:doc xmlns:x="urn:t" xmlns:mc="{MC}"><x:box><mc:AlternateContent><mc:Choice Requires="x"><x:item/></mc:Choice></mc:AlternateContent></x:box><x:box/></x:doc>"#
    );
    let mut d = doc(&src);
    let s = at::<Item>(&d, &[0, 0, 0, 0]);
    let r = d.lens_mut::<Doc>().unwrap().nth_child_mut::<Box_>(1).unwrap().move_in::<Item>(&s).map(|_| ());
    assert_matches!(r, Err(Error::NotMovable { reason: MoveRefusal::AlternateContent, .. }));
    assert_eq!(d.serialize(), src);
}

/// Paths after a move (review of PR #44, m4): later siblings of the
/// source shift down by one; moving several children last-first keeps the
/// earlier sources valid.
#[test]
fn moving_several_children_last_first() {
    let src = r#"<x:doc xmlns:x="urn:t"><x:box><x:item k="1"/><x:item k="2"/><x:item k="3"/></x:box><x:box></x:box></x:doc>"#;
    let mut d = doc(src);
    let sources: Vec<_> = (0..3).map(|i| at::<Item>(&d, &[0, i])).collect();
    for s in sources.iter().rev() {
        d.lens_mut::<Doc>().unwrap().nth_child_mut::<Box_>(1).unwrap().move_in_by::<Item>(s, |_, _| true).unwrap();
    }
    assert_eq!(d.serialize(), r#"<x:doc xmlns:x="urn:t"><x:box></x:box><x:box><x:item k="1"/><x:item k="2"/><x:item k="3"/></x:box></x:doc>"#);
    // first-to-last with the stale sources: the third source no longer leads to an element
    let mut d = doc(src);
    let sources: Vec<_> = (0..3).map(|i| at::<Item>(&d, &[0, i])).collect();
    d.lens_mut::<Doc>().unwrap().nth_child_mut::<Box_>(1).unwrap().move_in::<Item>(&sources[0]).unwrap();
    d.lens_mut::<Doc>().unwrap().nth_child_mut::<Box_>(1).unwrap().move_in::<Item>(&sources[1]).unwrap(); // moves k=3, not k=2
    assert!(d.serialize().contains(r#"<x:box><x:item k="1"/><x:item k="3"/></x:box>"#), "{}", d.serialize());
    assert_matches!(d.lens_mut::<Doc>().unwrap().nth_child_mut::<Box_>(1).unwrap().move_in::<Item>(&sources[2]), Err(Error::WrongElement { .. }));
}

/// An unbound default namespace and an explicit `xmlns=""` mean the same
/// (no namespace): moving an unprefixed no-namespace element between them is
/// fine (review of PR #44, m1 mutant G).
#[test]
fn an_undeclared_default_namespace_equals_xmlns_empty() {
    let src = r#"<x:doc xmlns:x="urn:t"><x:box><x:item><plain/></x:item></x:box><x:box xmlns=""/></x:doc>"#;
    move_item_to_box1(src).unwrap();
    let src = r#"<x:doc xmlns:x="urn:t"><x:box><x:item><plain/></x:item></x:box><x:box xmlns="urn:z"/></x:doc>"#;
    assert_matches!(move_item_to_box1(src), Err(Error::NotMovable { reason: MoveRefusal::NamespaceBinding, .. }));
}
