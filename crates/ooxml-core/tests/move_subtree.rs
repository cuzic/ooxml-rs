//! `LensMut::move_in[_by]` / `check_move_in_by` (ADR-0002 post-acceptance
//! note "same-part move") with hand-written specs: placement, dirty
//! duties, T2-style locality (the moved bytes leave the source and appear
//! at the destination, nothing else changes), refusals before any change,
//! and a brute-force check that `check_move_in_by` agrees with the move.

use std::assert_matches;

use ooxml_core::{ChildOf, Content, ElementSpec, Error, MoveRefusal, ParseOptions, QName, Slot, XmlDocument};

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
    let mut doc_lens = d.lens_mut::<Doc>().unwrap();
    let mut box0 = doc_lens.nth_child_mut::<Box_>(0).unwrap();
    // item k=3 (child index 3 of box0) before the first item of the slot
    let moved_lens = box0.move_in_by::<Item>(&[0, 3], |k, _| k == 0).unwrap();
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
    d.lens_mut::<Doc>().unwrap().nth_child_mut::<Box_>(2).unwrap().move_in::<Item>(&[1, 0]).unwrap();
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
    let mut l = d.lens_mut::<Doc>().unwrap();
    let mut b = l.nth_child_mut::<Box_>(0).unwrap();
    let mut item3 = b.nth_child_mut::<Item>(2).unwrap();
    assert_eq!(item3.path(), &[0, 3]);
    let m = item3.move_in::<Item>(&[0, 2]).unwrap();
    assert_eq!(m.path(), &[0, 2, 0]);
    assert_eq!(m.node().attr(QName::new("", "k")), Some("2"));
    let a = SRC.find(r#"<x:item k="2"/>"#).unwrap();
    let out = d.serialize();
    // item k=3 was self-closing and gains a child: re-rendered open tag
    assert_eq!(out, SRC.replace(r#"<x:item k="2"/><x:item k="3"/>"#, r#"<x:item k="3"><x:item k="2"/></x:item>"#));
    let _ = a;
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
    // the root
    let e = assert_refused(SRC, |d| d.lens_mut::<Doc>()?.nth_child_mut::<Box_>(0).unwrap().move_in::<Item>(&[]).map(|_| ()));
    assert_matches!(e, Error::NotMovable { reason: MoveRefusal::Root, .. });
    // into itself: item k=1 into its own descendant position (itself)
    let e = assert_refused(SRC, |d| {
        let mut l = d.lens_mut::<Doc>()?;
        let mut b = l.nth_child_mut::<Box_>(0).unwrap();
        let mut item1 = b.nth_child_mut::<Item>(0).unwrap();
        item1.move_in::<Item>(&[0, 1]).map(|_| ())
    });
    assert_matches!(e, Error::NotMovable { reason: MoveRefusal::IntoItself, .. });
    // wrong element at the path / no element there
    let e = assert_refused(SRC, |d| d.lens_mut::<Doc>()?.nth_child_mut::<Box_>(2).unwrap().move_in::<Item>(&[0, 0]).map(|_| ()));
    assert_matches!(e, Error::WrongElement { .. });
    let e = assert_refused(SRC, |d| d.lens_mut::<Doc>()?.nth_child_mut::<Box_>(2).unwrap().move_in::<Item>(&[7, 0]).map(|_| ()));
    assert_matches!(e, Error::WrongElement { .. });
    // slot limit: box0's head into box1 which ... (box1 has no head: allowed) — into a box that has one:
    let src = r#"<x:doc xmlns:x="urn:t"><x:box><x:head/></x:box><x:box><x:head/></x:box></x:doc>"#;
    let e = assert_refused(src, |d| d.lens_mut::<Doc>()?.nth_child_mut::<Box_>(1).unwrap().move_in::<Head>(&[0, 0]).map(|_| ()));
    assert_matches!(e, Error::AlreadyPresent(_));
    // ... but moving an element within its own parent never counts itself
    let mut d = doc(src);
    d.lens_mut::<Doc>().unwrap().nth_child_mut::<Box_>(0).unwrap().move_in::<Head>(&[0, 0]).unwrap();
    assert_eq!(d.serialize(), src);
    // out-of-order children at the destination
    let src = r#"<x:doc xmlns:x="urn:t"><x:box><x:item/></x:box><x:box><x:ext/><x:head/></x:box></x:doc>"#;
    let e = assert_refused(src, |d| d.lens_mut::<Doc>()?.nth_child_mut::<Box_>(1).unwrap().move_in::<Item>(&[0, 0]).map(|_| ()));
    assert_matches!(e, Error::UnresolvedSibling(_));
}

#[test]
fn meaning_must_not_change() {
    // a prefix declared on the source box only
    let src = r#"<x:doc xmlns:x="urn:t"><x:box xmlns:y="urn:y"><x:item y:k="1"/></x:box><x:box/></x:doc>"#;
    let e = assert_refused(src, |d| d.lens_mut::<Doc>()?.nth_child_mut::<Box_>(1).unwrap().move_in::<Item>(&[0, 0]).map(|_| ()));
    assert_matches!(e, Error::NotMovable { reason: MoveRefusal::NamespaceBinding, ref name } if name == "y");
    // ... also when only a descendant of the moved element uses it
    let src = r#"<x:doc xmlns:x="urn:t"><x:box xmlns:y="urn:y"><x:item><x:sub y:k="1"/></x:item></x:box><x:box/></x:doc>"#;
    let e = assert_refused(src, |d| d.lens_mut::<Doc>()?.nth_child_mut::<Box_>(1).unwrap().move_in::<Item>(&[0, 0]).map(|_| ()));
    assert_matches!(e, Error::NotMovable { reason: MoveRefusal::NamespaceBinding, ref name } if name == "y");
    // the same prefix bound to another URI at the destination
    let src = r#"<x:doc xmlns:x="urn:t" xmlns:y="urn:y"><x:box><x:item y:k="1"/></x:box><x:box xmlns:y="urn:other"/></x:doc>"#;
    let e = assert_refused(src, |d| d.lens_mut::<Doc>()?.nth_child_mut::<Box_>(1).unwrap().move_in::<Item>(&[0, 0]).map(|_| ()));
    assert_matches!(e, Error::NotMovable { reason: MoveRefusal::NamespaceBinding, .. });
    // the default namespace re-bound at the destination
    let src = r#"<doc xmlns="urn:t"><box><item/></box><x:box xmlns:x="urn:t" xmlns="urn:t2"/></doc>"#;
    let e = assert_refused(src, |d| d.lens_mut::<Doc>()?.nth_child_mut::<Box_>(1).unwrap().move_in::<Item>(&[0, 0]).map(|_| ()));
    assert_matches!(e, Error::NotMovable { reason: MoveRefusal::NamespaceBinding, ref name } if name == "(default namespace)");
    // a declaration inside the moved subtree travels with it
    let src = r#"<x:doc xmlns:x="urn:t"><x:box><x:item xmlns:y="urn:y" y:k="1"/></x:box><x:box></x:box></x:doc>"#;
    let mut d = doc(src);
    d.lens_mut::<Doc>().unwrap().nth_child_mut::<Box_>(1).unwrap().move_in::<Item>(&[0, 0]).unwrap();
    assert_eq!(d.serialize(), r#"<x:doc xmlns:x="urn:t"><x:box></x:box><x:box><x:item xmlns:y="urn:y" y:k="1"/></x:box></x:doc>"#);
    // ignorable at the source only
    let src = r#"<x:doc xmlns:x="urn:t" xmlns:e="urn:e" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><x:box mc:Ignorable="e"><x:item e:k="1"/></x:box><x:box/></x:doc>"#;
    let e = assert_refused(src, |d| d.lens_mut::<Doc>()?.nth_child_mut::<Box_>(1).unwrap().move_in::<Item>(&[0, 0]).map(|_| ()));
    assert_matches!(e, Error::NotMovable { reason: MoveRefusal::IgnorableNamespace, ref name } if name == "urn:e");
    // text under a different inherited xml:space
    let src = r#"<x:doc xmlns:x="urn:t"><x:box xml:space="preserve"><x:item>  </x:item></x:box><x:box/></x:doc>"#;
    let e = assert_refused(src, |d| d.lens_mut::<Doc>()?.nth_child_mut::<Box_>(1).unwrap().move_in::<Item>(&[0, 0]).map(|_| ()));
    assert_matches!(e, Error::NotMovable { reason: MoveRefusal::XmlSpace, .. });
    // ... unless the subtree states its own
    let src = r#"<x:doc xmlns:x="urn:t"><x:box xml:space="preserve"><x:item xml:space="preserve">  </x:item></x:box><x:box/></x:doc>"#;
    doc(src).lens_mut::<Doc>().unwrap().nth_child_mut::<Box_>(1).unwrap().move_in::<Item>(&[0, 0]).unwrap();
}

#[test]
fn the_whole_subtree_obeys_the_depth_limit() {
    // doc 1, box 2, item 3, sub 4: moving item (height 2) under box1 is fine at 4, not under an item at 4
    let src = r#"<x:doc xmlns:x="urn:t"><x:box><x:item><x:sub/></x:item><x:item/></x:box></x:doc>"#;
    let mut d = XmlDocument::parse_with(src.as_bytes(), &ParseOptions::default().with_max_depth(4)).unwrap();
    let e = {
        let mut l = d.lens_mut::<Doc>().unwrap();
        let mut b = l.nth_child_mut::<Box_>(0).unwrap();
        let mut item2 = b.nth_child_mut::<Item>(1).unwrap();
        item2.move_in::<Item>(&[0, 0]).map(|_| ()).unwrap_err()
    };
    assert_matches!(e, Error::TooDeep { limit: 4, .. });
    assert_eq!(d.serialize(), src);
}

/// Every (source item, destination container, position) of a small tree:
/// `check_move_in_by` and `move_in_by` agree; a refusal changes nothing;
/// a success is a pure move of the element's bytes.
#[test]
fn brute_force_check_agrees_with_move() {
    let src = concat!(
        r#"<x:doc xmlns:x="urn:t">"#,
        r#"<x:box><x:head></x:head><x:item k="a"><x:sub></x:sub><x:item k="b"></x:item></x:item><x:item k="c"></x:item><x:ext></x:ext></x:box>"#,
        r#"<x:box><x:item k="d"><x:item k="e"></x:item></x:item></x:box>"#,
        r#"</x:doc>"#
    );
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
    assert!(oks > 20 && errs > 5, "oks {oks}, errs {errs}");
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
        let mut l = d.lens_mut::<Doc>().unwrap();
        let mut b = l.nth_child_mut::<Box_>(0).unwrap();
        b.nth_child_mut::<Item>(1).unwrap().move_in::<Sub>(&[0, 1, 0]).unwrap();
    }
    d.lens_mut::<Doc>().unwrap().nth_child_mut::<Box_>(1).unwrap().move_in::<Ext>(&[0, 4]).unwrap();
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
