//! Same-part copies (`Lens::detach_same_part_as` /
//! `LensMut::insert_same_part[_by]`, ADR-0002 note "same-part copy") with
//! the hand-written specs of `detach.rs`: relationship ids kept, extension
//! lists only by allowed URI, the document check, the refusals that still
//! apply, and agreement of `check_insert_under` with the insertion.

use std::assert_matches;

use ooxml_core::lens::ext_uri;
use ooxml_core::{
    ChildOf, Content, DetachRefusal, Detachable, ElementSpec, Error, Lens, LensMut, QName, SameType, SamePartOptions, Slot, Wildcard,
    XmlDocument,
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
// Hand-written opt-in (tests only; see `Detachable`'s seal). RPr lists one
// unique attribute, like the generated a:rPr (smtId).
const UNIQUE: &[ooxml_core::UniqueAttr] = &[
    ooxml_core::UniqueAttr { element: q("rPr"), attr: QName::new("", "uid"), default: Some("0") },
    ooxml_core::UniqueAttr { element: q("end"), attr: QName::new("", "uid"), default: Some("0") },
];
impl ooxml_core::schema::__private::Sealed for RPr {}
impl ooxml_core::schema::__private::Sealed for End {}
impl ooxml_core::schema::__private::Sealed for R {}
impl Detachable for RPr {
    const UNIQUE_ATTRS: &'static [ooxml_core::UniqueAttr] = UNIQUE;
}
impl Detachable for End {
    const UNIQUE_ATTRS: &'static [ooxml_core::UniqueAttr] = UNIQUE;
}
impl Detachable for R {
    const UNIQUE_ATTRS: &'static [ooxml_core::UniqueAttr] = UNIQUE;
}

fn doc(s: &str) -> XmlDocument {
    XmlDocument::parse(s.as_bytes()).unwrap()
}

const R_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

/// A paragraph whose first run's properties hold a link (`r:id`) and, in
/// the link, an extension list.
fn src(ext_uri: &str) -> String {
    format!(
        concat!(
            r#"<x:p xmlns:x="urn:t" xmlns:r="{r}">"#,
            r#"<x:r><x:rPr b="1"><x:link r:id="rId7"><x:extLst><x:ext uri="{u}"><x:clr v="tx"/></x:ext></x:extLst></x:link></x:rPr><x:t>a</x:t></x:r>"#,
            r#"<x:r><x:t>b</x:t></x:r>"#,
            r#"</x:p>"#
        ),
        r = R_NS,
        u = ext_uri
    )
}

const PLAIN: &str = concat!(
    r#"<x:p xmlns:x="urn:t" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">"#,
    r#"<x:r><x:rPr b="1"><x:link r:id="rId7" r:embed="rId8"/></x:rPr><x:t>a</x:t></x:r>"#,
    r#"<x:r><x:t>b</x:t></x:r>"#,
    r#"</x:p>"#
);

fn first_rpr(d: &XmlDocument) -> Lens<'_, RPr> {
    d.lens::<P>().unwrap().children::<R>().next().unwrap().child::<RPr>().unwrap()
}

fn opts() -> SamePartOptions {
    SamePartOptions::default()
}

#[test]
fn relationship_ids_are_kept_within_the_part() {
    let mut d = doc(PLAIN);
    // the any-part copy still refuses them
    assert_matches!(first_rpr(&d).detach_as::<RPr>(&[]), Err(Error::NotDetachable { reason: DetachRefusal::RelationshipAttribute, .. }));
    let copy = first_rpr(&d).detach_same_part_as::<RPr>(&[], &opts()).unwrap();
    copy.check_insert_under(&d, &[1], 0).unwrap();
    let mut p = d.lens_mut::<P>().unwrap();
    p.nth_child_mut::<R>(1).unwrap().insert_same_part(&copy).unwrap();
    let out = d.serialize();
    let inserted = r#"<x:rPr b="1"><x:link r:id="rId7" r:embed="rId8"/></x:rPr>"#;
    assert_eq!(out, PLAIN.replace(r#"<x:r><x:t>b"#, &format!("<x:r>{inserted}<x:t>b")), "a pure insertion, ids unchanged");
    // and as the other SameType (renamed: its tags are re-rendered, the
    // prefix kept), and with a direct child skipped (children re-rendered)
    let end = first_rpr(&d).detach_same_part_as::<End>(&[], &opts()).unwrap();
    assert_eq!(end.raw().name().local(), "end");
    let bare = first_rpr(&d).detach_same_part_as::<RPr>(&[q("link")], &opts()).unwrap();
    let mut p = d.lens_mut::<P>().unwrap();
    p.insert_same_part(&end).unwrap();
    p.nth_child_mut::<R>(1).unwrap().remove::<RPr>();
    p.nth_child_mut::<R>(1).unwrap().insert_same_part(&bare).unwrap();
    let out = d.serialize();
    assert!(out.contains(r#"<x:end b="1"><x:link r:id="rId7" r:embed="rId8"/></x:end>"#), "{out}");
    assert!(out.contains(r#"<x:r><x:rPr b="1"></x:rPr><x:t>b</x:t></x:r>"#), "{out}");
    XmlDocument::parse(out.as_bytes()).unwrap();
}

#[test]
fn extension_lists_only_with_an_allowed_uri() {
    let u = ext_uri::HYPERLINK_COLOR;
    let d = doc(&src(u));
    // default: refused, as by detach_as
    assert_matches!(
        first_rpr(&d).detach_same_part_as::<RPr>(&[], &opts()),
        Err(Error::NotDetachable { reason: DetachRefusal::ExtensionList, .. })
    );
    assert_matches!(first_rpr(&d).detach_as::<RPr>(&[]), Err(Error::NotDetachable { .. }));
    // allowed URI, compared case-insensitively
    first_rpr(&d).detach_same_part_as::<RPr>(&[], &opts().keep_extension(u)).unwrap();
    first_rpr(&d).detach_same_part_as::<RPr>(&[], &opts().keep_extension(&u.to_ascii_lowercase())).unwrap();
    // another URI is refused
    assert_matches!(
        first_rpr(&d).detach_same_part_as::<RPr>(&[], &opts().keep_extension("{00000000-0000-0000-0000-000000000000}")),
        Err(Error::NotDetachable { reason: DetachRefusal::ExtensionList, .. })
    );
    // an extLst holding anything but allowed `ext`s is refused: a second ext,
    // an element of another name, an ext in another namespace, an ext without uri
    for inner in [
        format!(r#"<x:ext uri="{u}"/><x:ext uri="{{X}}"/>"#),
        format!(r#"<x:ext uri="{u}"/><x:other uri="{u}"/>"#),
        format!(r#"<y:ext xmlns:y="urn:y" uri="{u}"/>"#),
        r#"<x:ext/>"#.to_owned(),
    ] {
        let s = src(u).replace(&format!(r#"<x:ext uri="{u}"><x:clr v="tx"/></x:ext>"#), &inner);
        let d = doc(&s);
        assert_matches!(
            first_rpr(&d).detach_same_part_as::<RPr>(&[], &opts().keep_extension(u)),
            Err(Error::NotDetachable { .. }),
            "{inner}"
        );
    }
    // the content of an allowed extension still passes every other check
    let s = src(u).replace(r#"<x:clr v="tx"/>"#, r#"<x:clr mc:Ignorable="q" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"/>"#);
    let d = doc(&s);
    assert_matches!(first_rpr(&d).detach_same_part_as::<RPr>(&[], &opts().keep_extension(u)), Err(Error::NotDetachable { .. }));
    // and may keep same-part ids
    let s = src(u).replace(r#"<x:clr v="tx"/>"#, r#"<x:clr r:id="rId9"/>"#);
    let mut d = doc(&s);
    let copy = first_rpr(&d).detach_same_part_as::<RPr>(&[], &opts().keep_extension(u)).unwrap();
    d.lens_mut::<P>().unwrap().nth_child_mut::<R>(1).unwrap().insert_same_part(&copy).unwrap();
    assert_eq!(d.serialize().matches(r#"<x:clr r:id="rId9"/>"#).count(), 2);
}

#[test]
fn document_unique_attributes_and_the_other_refusals_still_apply() {
    // a listed unique attribute, on the root and nested (`end` is listed too)
    for rpr in [r#"<x:rPr uid="3"/>"#, r#"<x:rPr><x:end uid="1"/></x:rPr>"#] {
        let s = PLAIN.replace(r#"<x:rPr b="1"><x:link r:id="rId7" r:embed="rId8"/></x:rPr>"#, rpr);
        let d = doc(&s);
        assert_matches!(
            first_rpr(&d).detach_same_part_as::<RPr>(&[], &opts()),
            Err(Error::NotDetachable { reason: DetachRefusal::UniqueAttribute, .. }),
            "{rpr}"
        );
        // the default value is not an id
        let d = doc(&s.replace(r#"uid="3""#, r#"uid="0""#).replace(r#"uid="1""#, r#"uid="0""#));
        first_rpr(&d).detach_same_part_as::<RPr>(&[], &opts()).unwrap();
    }
    // Markup Compatibility (attribute or element)
    for rpr in [
        r#"<x:rPr mc:Ignorable="y" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"/>"#,
        r#"<x:rPr><mc:AlternateContent xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"/></x:rPr>"#,
    ] {
        let s = PLAIN.replace(r#"<x:rPr b="1"><x:link r:id="rId7" r:embed="rId8"/></x:rPr>"#, rpr);
        let d = doc(&s);
        assert_matches!(first_rpr(&d).detach_same_part_as::<RPr>(&[], &opts()), Err(Error::NotDetachable { .. }), "{rpr}");
    }
    // Checked at the destination (as a move is): an ignorable namespace
    // and an inherited xml:space are fine where they mean the same...
    let ign = concat!(
        r#"<x:p xmlns:x="urn:t" xmlns:y="urn:y" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006">"#,
        r#"<x:r mc:Ignorable="y"><x:rPr y:k="v"/><x:t>a</x:t></x:r><x:r mc:Ignorable="y"><x:t>b</x:t></x:r><x:r><x:t>c</x:t></x:r></x:p>"#
    );
    let spc = concat!(
        r#"<x:p xmlns:x="urn:t">"#,
        r#"<x:r xml:space="preserve"><x:rPr><x:note>a  b</x:note></x:rPr><x:t>a</x:t></x:r><x:r xml:space="preserve"><x:t>b</x:t></x:r><x:r><x:t>c</x:t></x:r></x:p>"#
    );
    for (s, reason) in [(ign, DetachRefusal::IgnorableNamespace), (spc, DetachRefusal::InheritedXmlSpace)] {
        let mut d = doc(s);
        let copy = first_rpr(&d).detach_same_part_as::<RPr>(&[], &opts()).unwrap();
        copy.check_insert_under(&d, &[1], 0).unwrap();
        // ...and refused where they would change its meaning
        assert_matches!(copy.check_insert_under(&d, &[2], 0), Err(Error::NotDetachable { reason: r, .. }) if r == reason);
        let before = d.serialize();
        let r = d.lens_mut::<P>().unwrap().nth_child_mut::<R>(2).unwrap().insert_same_part(&copy).map(drop);
        assert_matches!(r, Err(Error::NotDetachable { reason: r, .. }) if r == reason);
        assert_eq!(d.serialize(), before, "an Err changes nothing");
        d.lens_mut::<P>().unwrap().nth_child_mut::<R>(1).unwrap().insert_same_part(&copy).unwrap();
    }
}

#[test]
fn a_same_part_copy_goes_only_into_its_own_document() {
    let mut a = doc(PLAIN);
    let mut b = doc(PLAIN); // same bytes, another document
    let copy = first_rpr(&a).detach_same_part_as::<RPr>(&[], &opts()).unwrap();
    // check twin and insertion agree: Err(WrongDocument), nothing changes
    assert_matches!(copy.check_insert_under(&b, &[1], 0), Err(Error::WrongDocument));
    assert_matches!(b.lens_mut::<P>().unwrap().nth_child_mut::<R>(1).unwrap().insert_same_part(&copy).map(drop), Err(Error::WrongDocument));
    assert!(!b.is_dirty());
    assert_eq!(b.serialize(), PLAIN);
    // a clone is another document (it may become another part)
    let mut c = a.clone();
    assert_matches!(copy.check_insert_under(&c, &[1], 0), Err(Error::WrongDocument));
    assert_matches!(c.lens_mut::<P>().unwrap().nth_child_mut::<R>(1).unwrap().insert_same_part(&copy).map(drop), Err(Error::WrongDocument));
    assert!(!c.is_dirty());
    // lenses that do not know their document
    let lens = Lens::<RPr>::at(a.root(), &[0, 0]).unwrap();
    assert_matches!(lens.detach_same_part_as::<RPr>(&[], &opts()), Err(Error::WrongDocument));
    let raw_root = Lens::<P>::root(a.root()).unwrap();
    assert_matches!(
        raw_root.children::<R>().next().unwrap().child::<RPr>().unwrap().detach_same_part_as::<RPr>(&[], &opts()),
        Err(Error::WrongDocument)
    );
    let mut root = a.root().clone();
    let mut lm = LensMut::<P>::root(&mut root).unwrap();
    assert_matches!(lm.nth_child_mut::<R>(1).unwrap().insert_same_part(&copy).map(drop), Err(Error::WrongDocument));
    // its own document: fine
    a.lens_mut::<P>().unwrap().nth_child_mut::<R>(1).unwrap().insert_same_part(&copy).unwrap();
}

/// `check_insert_under` agrees with `insert_same_part` at every element of
/// the document (as a parent), including a destination where the `r`
/// namespace is not in scope (declared only on the source's run).
#[test]
fn check_twin_agrees_with_the_insertion() {
    let s = concat!(
        r#"<x:p xmlns:x="urn:t">"#,
        r#"<x:r xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><x:rPr><x:link r:id="rId1"/></x:rPr><x:t>a</x:t></x:r>"#,
        r#"<x:r><x:t>b</x:t></x:r>"#,
        r#"</x:p>"#
    );
    for target in 0..2usize {
        let mut d = doc(s);
        let copy = first_rpr(&d).detach_same_part_as::<RPr>(&[], &opts()).unwrap();
        let check = copy.check_insert_under(&d, &[target], 0).map_err(|e| format!("{e:?}"));
        let before = d.serialize();
        let ins = d.lens_mut::<P>().unwrap().nth_child_mut::<R>(target).unwrap().insert_same_part(&copy).map(drop).map_err(|e| format!("{e:?}"));
        // the slot rule (rPr max 1) is not part of the check; compare only the rest
        if target == 0 {
            assert!(check.is_ok() && ins.as_ref().is_err_and(|e| e.contains("AlreadyPresent")), "{check:?} {ins:?}");
            assert_eq!(d.serialize(), before);
        } else {
            assert_eq!(check, ins, "target run {target}");
            assert!(check.as_ref().is_err_and(|e| e.contains("NamespaceBinding")), "{check:?}");
            assert_eq!(d.serialize(), before, "an Err changes nothing");
        }
    }
}

/// The copy is a span-preserving clone: namespace declarations travel with
/// its bytes (extension content declares its namespace locally, as
/// `ahyp:hlinkClr` does). An empty `extLst` is refused unless an extension
/// is allowed.
#[test]
fn declarations_inside_an_allowed_extension_are_kept() {
    let u = ext_uri::HYPERLINK_COLOR;
    let s = src(u).replace(r#"<x:clr v="tx"/>"#, r#"<z:clr xmlns:z="urn:z" v="tx"><z:k/></z:clr>"#);
    let mut d = doc(&s);
    let copy = first_rpr(&d).detach_same_part_as::<RPr>(&[], &opts().keep_extension(u)).unwrap();
    d.lens_mut::<P>().unwrap().nth_child_mut::<R>(1).unwrap().insert_same_part(&copy).unwrap();
    let out = d.serialize();
    assert_eq!(out.matches(r#"<z:clr xmlns:z="urn:z" v="tx"><z:k/></z:clr>"#).count(), 2, "{out}");
    XmlDocument::parse(out.as_bytes()).unwrap();
    // a declaration anywhere in the copy travels with its bytes
    let s = PLAIN.replace(r#"<x:link r:id="rId7" r:embed="rId8"/>"#, r#"<z:k xmlns:z="urn:z" z:v="1"/>"#);
    let mut d = doc(&s);
    let copy = first_rpr(&d).detach_same_part_as::<RPr>(&[], &opts()).unwrap();
    d.lens_mut::<P>().unwrap().nth_child_mut::<R>(1).unwrap().insert_same_part(&copy).unwrap();
    assert_eq!(d.serialize().matches(r#"<z:k xmlns:z="urn:z" z:v="1"/>"#).count(), 2);
    // an empty extLst
    let s = PLAIN.replace(r#"<x:link r:id="rId7" r:embed="rId8"/>"#, r#"<x:extLst/>"#);
    let d = doc(&s);
    assert_matches!(first_rpr(&d).detach_as::<RPr>(&[]), Err(Error::NotDetachable { reason: DetachRefusal::ExtensionList, .. }));
    assert_matches!(
        first_rpr(&d).detach_same_part_as::<RPr>(&[], &opts()),
        Err(Error::NotDetachable { reason: DetachRefusal::ExtensionList, .. })
    );
    first_rpr(&d).detach_same_part_as::<RPr>(&[], &opts().keep_extension(u)).unwrap();
}

/// `XmlDocument::lens_at` / `lens_mut_at` carry the document (same-part
/// copies work through them) and check the element at the path.
#[test]
fn path_lenses_know_their_document() {
    let mut d = doc(PLAIN);
    let copy = d.lens_at::<RPr>(&[0, 0]).unwrap().detach_same_part_as::<RPr>(&[], &opts()).unwrap();
    d.lens_mut_at::<R>(&[1]).unwrap().insert_same_part(&copy).unwrap();
    assert_eq!(d.serialize().matches(r#"r:id="rId7""#).count(), 2);
    assert_matches!(d.lens_at::<RPr>(&[1]), Err(Error::WrongElement { .. }));
    assert_matches!(d.lens_mut_at::<R>(&[9]).map(drop), Err(Error::WrongElement { .. }));
}

/// A prefix the copy declares itself is not compared with the destination
/// (it travels with the bytes), even where the destination binds it to
/// another namespace; MC attributes deeper in the copy are refused; the
/// depth limit counts the copy's deepest element, in the check twin too.
#[test]
fn own_declarations_nested_mc_and_depth() {
    let s = concat!(
        r#"<x:p xmlns:x="urn:t">"#,
        r#"<x:r><x:rPr><z:k xmlns:z="urn:z" z:v="1"/></x:rPr><x:t>a</x:t></x:r>"#,
        r#"<x:r xmlns:z="urn:other"><x:t>b</x:t></x:r></x:p>"#
    );
    let mut d = doc(s);
    let copy = first_rpr(&d).detach_same_part_as::<RPr>(&[], &opts()).unwrap();
    copy.check_insert_under(&d, &[1], 0).unwrap();
    d.lens_mut::<P>().unwrap().nth_child_mut::<R>(1).unwrap().insert_same_part(&copy).unwrap();
    assert_eq!(d.serialize().matches(r#"<z:k xmlns:z="urn:z" z:v="1"/>"#).count(), 2);
    // nested MC attribute
    let s = PLAIN.replace(
        r#"<x:link r:id="rId7" r:embed="rId8"/>"#,
        r#"<x:fill xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" mc:Ignorable="x"/>"#,
    );
    let d = doc(&s);
    assert_matches!(
        first_rpr(&d).detach_same_part_as::<RPr>(&[], &opts()),
        Err(Error::NotDetachable { reason: DetachRefusal::MarkupCompatibility, .. })
    );
    // depth: p(1) / r(2) / rPr(3) / link(4). With a limit of 4 the copy fits
    // under a run, but not one created element deeper (check twin); the
    // insertion counts the copy's deepest element the same way.
    let opts4 = ooxml_core::ParseOptions::default().with_max_depth(4);
    let mut d = XmlDocument::parse_with(PLAIN.as_bytes(), &opts4).unwrap();
    let copy = d.lens::<P>().unwrap().children::<R>().next().unwrap().child::<RPr>().unwrap().detach_same_part_as::<RPr>(&[], &opts()).unwrap();
    assert_eq!(copy.height(), 2);
    copy.check_insert_under(&d, &[1], 0).unwrap();
    assert_matches!(copy.check_insert_under(&d, &[1], 1), Err(Error::TooDeep { .. }));
    d.lens_mut::<P>().unwrap().nth_child_mut::<R>(1).unwrap().insert_same_part(&copy).unwrap();
    let opts3 = ooxml_core::ParseOptions::default().with_max_depth(3);
    let shallow = PLAIN.replace(r#"<x:link r:id="rId7" r:embed="rId8"/>"#, "");
    let d3 = XmlDocument::parse_with(shallow.as_bytes(), &opts3).unwrap();
    let flat = d3.lens::<P>().unwrap().children::<R>().next().unwrap().child::<RPr>().unwrap().detach_same_part_as::<RPr>(&[], &opts()).unwrap();
    flat.check_insert_under(&d3, &[1], 0).unwrap();
    assert_matches!(flat.check_insert_under(&d3, &[1], 1), Err(Error::TooDeep { .. }));
}

/// Text under the copy's own `xml:space` does not inherit it, so the
/// destination's inherited value does not matter for it.
#[test]
fn own_xml_space_is_not_inherited() {
    let s = concat!(
        r#"<x:p xmlns:x="urn:t">"#,
        r#"<x:r xml:space="preserve"><x:rPr><x:note xml:space="preserve">a  b</x:note></x:rPr><x:t>a</x:t></x:r><x:r><x:t>c</x:t></x:r></x:p>"#
    );
    let mut d = doc(s);
    let copy = first_rpr(&d).detach_same_part_as::<RPr>(&[], &opts()).unwrap();
    copy.check_insert_under(&d, &[1], 0).unwrap();
    d.lens_mut::<P>().unwrap().nth_child_mut::<R>(1).unwrap().insert_same_part(&copy).unwrap();
    assert_eq!(d.serialize().matches(r#"<x:note xml:space="preserve">a  b</x:note>"#).count(), 2);
}
