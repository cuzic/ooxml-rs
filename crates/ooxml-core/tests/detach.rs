//! `Lens::detach_as` / `LensMut::insert_detached[_by]` (ADR-0002
//! post-acceptance note "detach/insert") with small hand-written specs:
//! renaming through `SameType`, `skip`, refusals before any change, prefix
//! re-selection at the destination, slot placement, dirty duties and
//! T2-style locality (the output is the input plus exactly one insertion).

use std::assert_matches;

use ooxml_core::{
    ChildOf, Content, DetachRefusal, ElementSpec, Error, Lens, QName, SameType, Slot, Wildcard, XmlDocument,
};

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
const ANY: Slot = Slot { names: &[], wildcard: Some(Wildcard::Any), max_each: None, max_total: None };

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

// <p> : pPr? r* end?      <r> : rPr? t      rPr / end : any*
spec!(P, "p", Content::Slots(&[one(&[q("pPr")]), many(&[q("r")]), one(&[q("end")])]));
spec!(R, "r", Content::Slots(&[one(&[q("rPr")]), one(&[q("t")])]));
spec!(RPr, "rPr", Content::Slots(&[ANY]));
spec!(End, "end", Content::Slots(&[ANY]));
impl ChildOf<P> for R {}
impl ChildOf<P> for End {}
impl ChildOf<R> for RPr {}
// like a:endParaRPr / a:rPr (both a:CT_TextCharacterProperties)
impl SameType<RPr> for End {}
impl SameType<End> for RPr {}

fn doc(s: &str) -> XmlDocument {
    XmlDocument::parse(s.as_bytes()).unwrap()
}

/// T2-style locality: `after` is `before` with exactly `inserted` added at
/// one offset; returns that offset.
fn pure_insertion(before: &str, after: &str, inserted: &str) -> usize {
    assert_eq!(after.len(), before.len() + inserted.len(), "\nbefore: {before}\nafter:  {after}");
    let at = before.bytes().zip(after.bytes()).position(|(a, b)| a != b).unwrap_or(before.len());
    // the insertion may be ambiguous by a few bytes when its edges repeat; find one that fits
    let fits = |k: usize| after[..k] == before[..k] && &after[k..k + inserted.len()] == inserted && after[k + inserted.len()..] == before[k..];
    (at.saturating_sub(inserted.len())..=at).find(|&k| after.is_char_boundary(k) && fits(k)).unwrap_or_else(|| {
        panic!("not a pure insertion of {inserted:?}\nbefore: {before}\nafter:  {after}")
    })
}

const SRC: &str = concat!(
    r#"<x:p xmlns:x="urn:t" xmlns:y="urn:y" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">"#,
    r#"<x:r><x:rPr b="1" y:k="v" xml:lang="en"><x:fill><x:c v="1"/></x:fill><!--keep--><x:link r:id="rId1"/><y:ext>t &amp; u</y:ext></x:rPr><x:t>a</x:t></x:r>"#,
    r#"<x:r><x:t>b</x:t></x:r>"#,
    r#"<x:end sz="2"><x:fill/></x:end>"#,
    r#"</x:p>"#
);

fn first_rpr(d: &XmlDocument) -> Lens<'_, RPr> {
    d.lens::<P>().unwrap().children::<R>().next().unwrap().child::<RPr>().unwrap()
}

#[test]
fn copy_into_another_run_is_a_pure_insertion() {
    let mut d = doc(SRC);
    let copy = first_rpr(&d).detach_as::<RPr>(&[q("link")]).unwrap();
    assert!(copy.raw().name().prefix().is_none(), "a detached copy carries no prefixes");
    let mut p = d.lens_mut::<P>().unwrap();
    let mut r = p.nth_child_mut::<R>(1).unwrap();
    let new = r.insert_detached(copy).unwrap();
    assert!(new.node().own_dirty());
    let out = d.serialize();
    let inserted = r#"<x:rPr b="1" y:k="v" xml:lang="en"><x:fill><x:c v="1"/></x:fill><!--keep--><y:ext>t &amp; u</y:ext></x:rPr>"#;
    let at = pure_insertion(SRC, &out, inserted);
    assert_eq!(at, SRC.find("<x:r><x:t>b").unwrap() + "<x:r>".len(), "before a:t, in rPr's slot");
    // the untouched first run and end are emitted verbatim (not dirty)
    let root = d.root();
    assert!(!root.elements().next().unwrap().subtree_dirty());
    assert!(!root.elements().last().unwrap().subtree_dirty());
    // and the output re-parses to the same copy
    let again = doc(&out);
    let a = again.lens::<P>().unwrap().children::<R>().nth(1).unwrap().child::<RPr>().unwrap().detach_as::<RPr>(&[]).unwrap();
    let b = first_rpr(&again).detach_as::<RPr>(&[q("link")]).unwrap();
    assert_eq!(format!("{:?}", a.raw()), format!("{:?}", b.raw()));
}

#[test]
fn same_type_renames_and_slot_placement() {
    // end → rPr (SameType<End> for RPr) into the second run; rPr → end
    // into a paragraph that has none, placed after the runs.
    let mut d = doc(SRC);
    let end = d.lens::<P>().unwrap().child::<End>().unwrap().detach_as::<RPr>(&[]).unwrap();
    assert!(end.raw().is(q("rPr")));
    d.lens_mut::<P>().unwrap().nth_child_mut::<R>(1).unwrap().insert_detached(end).unwrap();
    let out = d.serialize();
    pure_insertion(SRC, &out, r#"<x:rPr sz="2"><x:fill/></x:rPr>"#);

    let src2 = r#"<x:p xmlns:x="urn:t" xmlns:y="urn:y"><x:r><x:t>a</x:t></x:r></x:p>"#;
    let rpr = first_rpr(&doc(SRC)).detach_as::<End>(&[q("link"), q("fill")]).unwrap();
    let mut d2 = doc(src2);
    d2.lens_mut::<P>().unwrap().insert_detached(rpr).unwrap();
    let out2 = d2.serialize();
    let at = pure_insertion(src2, &out2, r#"<x:end b="1" y:k="v" xml:lang="en"><!--keep--><y:ext>t &amp; u</y:ext></x:end>"#);
    assert_eq!(at, src2.find("</x:p>").unwrap());
}

#[test]
fn prefixes_are_chosen_at_the_destination() {
    // The destination binds urn:t as the default namespace and urn:y as `q`.
    let copy = first_rpr(&doc(SRC)).detach_as::<RPr>(&[q("link")]).unwrap();
    let dst = r#"<p xmlns="urn:t" xmlns:q="urn:y"><r><t>b</t></r></p>"#;
    let mut d = doc(dst);
    d.lens_mut::<P>().unwrap().nth_child_mut::<R>(0).unwrap().insert_detached(copy.clone()).unwrap();
    pure_insertion(dst, &d.serialize(), r#"<rPr b="1" q:k="v" xml:lang="en"><fill><c v="1"/></fill><!--keep--><q:ext>t &amp; u</q:ext></rPr>"#);
    // the same copy inserted twice (Detached is Clone)
    let dst2 = r#"<x:p xmlns:x="urn:t" xmlns:y="urn:y"><x:r><x:t>1</x:t></x:r><x:r><x:t>2</x:t></x:r></x:p>"#;
    let mut d = doc(dst2);
    for i in 0..2 {
        d.lens_mut::<P>().unwrap().nth_child_mut::<R>(i).unwrap().insert_detached(copy.clone()).unwrap();
    }
    assert_eq!(d.serialize().matches(r#"<x:rPr b="1" y:k="v""#).count(), 2);
}

#[test]
fn missing_namespace_at_destination_changes_nothing() {
    let copy = first_rpr(&doc(SRC)).detach_as::<RPr>(&[q("link")]).unwrap();
    let dst = r#"<x:p xmlns:x="urn:t"><x:r><x:t>b</x:t></x:r></x:p>"#; // urn:y not bound
    let mut d = doc(dst);
    let e = d.lens_mut::<P>().unwrap().nth_child_mut::<R>(0).unwrap().insert_detached(copy).unwrap_err();
    assert_matches!(e, Error::NamespaceNotInScope(ref u) if u == "urn:y");
    assert_eq!(d.serialize(), dst);
    assert!(!d.root().subtree_dirty(), "nothing was marked");
}

#[test]
fn slot_rules_apply() {
    // the first run already has an rPr (max 1): refused, unchanged
    let mut d = doc(SRC);
    let copy = first_rpr(&d).detach_as::<RPr>(&[q("link")]).unwrap();
    let e = d.lens_mut::<P>().unwrap().nth_child_mut::<R>(0).unwrap().insert_detached(copy).unwrap_err();
    assert_matches!(e, Error::AlreadyPresent(_));
    assert_eq!(d.serialize(), SRC);
    // a self-closing parent gaining its first child is re-rendered
    let copy = first_rpr(&doc(SRC)).detach_as::<RPr>(&[q("link"), q("fill"), QName::new("urn:y", "ext")]).unwrap();
    let dst = r#"<x:p xmlns:x="urn:t" xmlns:y="urn:y"><x:r/></x:p>"#;
    let mut d = doc(dst);
    d.lens_mut::<P>().unwrap().nth_child_mut::<R>(0).unwrap().insert_detached(copy).unwrap();
    assert!(d.root().elements().next().unwrap().own_dirty());
    assert_eq!(d.serialize(), r#"<x:p xmlns:x="urn:t" xmlns:y="urn:y"><x:r><x:rPr b="1" y:k="v" xml:lang="en"><!--keep--></x:rPr></x:r></x:p>"#);
    // insert_detached_by: caller order within a repeating slot
    let mut d = doc(SRC);
    let first = d.lens::<P>().unwrap().children::<R>().next().unwrap().detach_as::<R>(&[]).unwrap_err();
    assert_matches!(first, Error::NotDetachable { reason: DetachRefusal::RelationshipAttribute, .. });
    let second = d.lens::<P>().unwrap().children::<R>().nth(1).unwrap().detach_as::<R>(&[]).unwrap();
    d.lens_mut::<P>().unwrap().insert_detached_by(second, |k, _| k == 0).unwrap();
    pure_insertion(SRC, &d.serialize(), r#"<x:r><x:t>b</x:t></x:r>"#);
    assert!(d.serialize().starts_with(r#"<x:p xmlns:x="urn:t" xmlns:y="urn:y" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><x:r><x:t>b</x:t></x:r><x:r><x:rPr"#));
}

#[test]
fn refusals_happen_before_anything_is_copied() {
    // (subtree under <x:rPr>, declarations on the wrapper <x:p>)
    const HEAD: &str = r#"<x:p xmlns:x="urn:t" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><x:r><x:rPr>"#;
    let cases: &[(&str, DetachRefusal, &str)] = &[
        (r#"<x:link r:id="rId1"/>"#, DetachRefusal::RelationshipAttribute, "{http://schemas.openxmlformats.org/officeDocument/2006/relationships}id"),
        (r#"<x:fill><mc:AlternateContent/></x:fill>"#, DetachRefusal::AlternateContent, "{http://schemas.openxmlformats.org/markup-compatibility/2006}AlternateContent"),
        (r#"<x:fill mc:Ignorable="z"/>"#, DetachRefusal::MarkupCompatibility, "{http://schemas.openxmlformats.org/markup-compatibility/2006}Ignorable"),
        (r#"<x:fill><x:c xmlns:z="urn:z"/></x:fill>"#, DetachRefusal::NamespaceDeclaration, "{http://www.w3.org/2000/xmlns/}z"),
    ];
    for (inner, reason, name) in cases {
        let xml = format!("{HEAD}{inner}</x:rPr></x:r></x:p>");
        let d = doc(&xml);
        let e = first_rpr(&d).detach_as::<End>(&[]).unwrap_err();
        assert_matches!(e, Error::NotDetachable { reason: r, name: ref n } if r == *reason && n == name, "{xml}");
    }
    // a declaration on the detached element itself is refused too
    let d = doc(r#"<rPr xmlns="urn:t"/>"#);
    let e = d.lens::<RPr>().unwrap().detach_as::<End>(&[]).unwrap_err();
    assert_matches!(e, Error::NotDetachable { reason: DetachRefusal::NamespaceDeclaration, .. });
    // a relationship attribute inside a skipped child is fine
    let d = doc(&format!(r#"{HEAD}<x:link r:id="rId1"/></x:rPr></x:r></x:p>"#));
    let c = first_rpr(&d).detach_as::<End>(&[q("link")]).unwrap();
    assert_eq!(c.raw().elements().count(), 0);
}
