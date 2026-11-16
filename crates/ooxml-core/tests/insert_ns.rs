//! `*_with_ns` inserts: namespace declarations on created elements
//! (ADR-0002 post-acceptance note). Shapes follow Word's inline picture
//! (`wp:inline/a:graphic/a:graphicData/pic:pic`, `r:embed`) and
//! PowerPoint's per-element `a16` extensions.

use std::assert_matches;

use ooxml_core::{AttrSpec, ChildOf, Content, ElementSpec, Error, LensMut, NsDecl, QName, RawNode, SimpleType, Slot, Wildcard, XmlDocument};

const WP: &str = "urn:wp";
const A: &str = "urn:a";
const PIC: &str = "urn:pic";
const R: &str = "urn:r";

const fn many(n: &'static [QName]) -> Slot {
    Slot { names: n, wildcard: None, max_each: None, max_total: None }
}
const fn one(n: &'static [QName]) -> Slot {
    Slot { names: n, wildcard: None, max_each: Some(1), max_total: Some(1) }
}

macro_rules! spec {
    ($t:ident, $ns:expr, $l:literal, $content:expr $(, $attrs:expr)?) => {
        enum $t {}
        impl ElementSpec for $t {
            const QNAME: QName = QName::new($ns, $l);
            const SDK_NAME: &'static str = $l;
            const ATTRS: &'static [AttrSpec] = spec!(@attrs $($attrs)?);
            const CONTENT: Content = $content;
        }
    };
    (@attrs) => { &[] };
    (@attrs $a:expr) => { $a };
}

// wp:inline : extent? graphic? ; a:graphic : graphicData ; a:graphicData : ##other* ; pic:pic : blip?
spec!(Inline, WP, "inline", Content::Slots(&[one(&[QName::new(WP, "extent")]), one(&[QName::new(A, "graphic")])]));
spec!(Extent, WP, "extent", Content::Empty);
spec!(Graphic, A, "graphic", Content::Slots(&[one(&[QName::new(A, "graphicData")])]));
spec!(GraphicData, A, "graphicData", Content::Slots(&[Slot { names: &[], wildcard: Some(Wildcard::Other(A)), max_each: None, max_total: None }]));
spec!(Pic, PIC, "pic", Content::Slots(&[many(&[QName::new(PIC, "blip")])]));
spec!(Blip, PIC, "blip", Content::Empty, &[AttrSpec { name: QName::new(R, "embed"), ty: SimpleType::String, default: None }]);
impl ChildOf<Inline> for Extent {}
impl ChildOf<Inline> for Graphic {}
impl ChildOf<Graphic> for GraphicData {}
impl ChildOf<GraphicData> for Pic {}
impl ChildOf<Pic> for Blip {}

fn doc(s: &str) -> XmlDocument {
    XmlDocument::parse(s.as_bytes()).unwrap()
}

fn ns_of(n: &RawNode) -> (String, String) {
    (n.name().ns().to_owned(), n.name().local().to_owned())
}

#[test]
fn word_inline_picture_shape() {
    let src = r#"<wp:inline xmlns:wp="urn:wp"><wp:extent/></wp:inline>"#;
    let mut d = doc(src);
    {
        let mut inline = d.lens_mut::<Inline>().unwrap();
        let mut g = inline.insert_with_ns::<Graphic>(&[NsDecl::new("a", A)]).unwrap();
        let mut gd = g.insert::<GraphicData>().unwrap(); // `a` is now in scope
        let mut pic = gd.insert_with_ns::<Pic>(&[NsDecl::new("pic", PIC), NsDecl::new("r", R)]).unwrap();
        let mut blip = pic.insert::<Blip>().unwrap();
        blip.set_attr(QName::new(R, "embed"), "rId7").unwrap(); // `r` declared on pic:pic
    }
    let out = d.serialize();
    assert_eq!(
        out,
        r#"<wp:inline xmlns:wp="urn:wp"><wp:extent/><a:graphic xmlns:a="urn:a"><a:graphicData><pic:pic xmlns:pic="urn:pic" xmlns:r="urn:r"><pic:blip r:embed="rId7"/></pic:pic></a:graphicData></a:graphic></wp:inline>"#
    );
    // T2: everything before the insertion point and the parent's close tag are unchanged.
    let at = src.find("</wp:inline>").unwrap();
    assert_eq!(&out[..at], &src[..at]);
    assert!(out.ends_with("</wp:inline>"));
    // The output re-parses with every name in its namespace.
    let again = doc(&out);
    let g = again.root().elements().nth(1).unwrap();
    assert_eq!(ns_of(g), (A.into(), "graphic".into()));
    let pic = g.elements().next().unwrap().elements().next().unwrap();
    assert_eq!(ns_of(pic), (PIC.into(), "pic".into()));
    let blip = pic.elements().next().unwrap();
    assert_eq!(blip.attr(QName::new(R, "embed")), Some("rId7"));
}

#[test]
fn bound_uris_are_not_redeclared_and_bindings_are_never_shadowed() {
    // already bound with a prefix: nothing declared, the existing prefix is used
    let mut d = doc(r#"<wp:inline xmlns:wp="urn:wp" xmlns:x="urn:a"/>"#);
    d.lens_mut::<Inline>().unwrap().insert_with_ns::<Graphic>(&[NsDecl::new("a", A)]).unwrap();
    assert_eq!(d.serialize(), r#"<wp:inline xmlns:wp="urn:wp" xmlns:x="urn:a"><x:graphic/></wp:inline>"#);

    // the preferred prefix is bound to another URI: the first free suffix
    let mut d = doc(r#"<wp:inline xmlns:wp="urn:wp" xmlns:a="urn:other" xmlns:a1="urn:other1"/>"#);
    d.lens_mut::<Inline>().unwrap().insert_with_ns::<Graphic>(&[NsDecl::new("a", A)]).unwrap();
    assert!(d.serialize().ends_with(r#"<a2:graphic xmlns:a2="urn:a"/></wp:inline>"#), "{}", d.serialize());

    // bound only as the default namespace: the element uses it, the prefixed
    // declaration is still added (attributes cannot use a default namespace)
    let mut d = doc(r#"<wp:inline xmlns:wp="urn:wp" xmlns="urn:a"/>"#);
    d.lens_mut::<Inline>().unwrap().insert_with_ns::<Graphic>(&[NsDecl::new("a", A)]).unwrap();
    assert!(d.serialize().ends_with(r#"<graphic xmlns:a="urn:a"/></wp:inline>"#), "{}", d.serialize());

    // a fallback prefix taken by an earlier request is skipped (p1 for urn:pic, so urn:r gets p11)
    let mut d = doc(r#"<wp:inline xmlns:wp="urn:wp" xmlns:p="urn:z"/>"#);
    d.lens_mut::<Inline>().unwrap().insert_with_ns::<Graphic>(&[NsDecl::new("a", A), NsDecl::new("p", PIC), NsDecl::new("p1", R)]).unwrap();
    assert!(d.serialize().contains(r#"<a:graphic xmlns:a="urn:a" xmlns:p1="urn:pic" xmlns:p11="urn:r"/>"#), "{}", d.serialize());
}

#[test]
fn refusals_change_nothing() {
    let src = r#"<wp:inline xmlns:wp="urn:wp"><wp:extent/></wp:inline>"#;
    let cases: &[(&[NsDecl<'_>], &str)] = &[
        (&[NsDecl::new("a", "")], "empty"),
        (&[NsDecl::new("a", "http://www.w3.org/XML/1998/namespace")], "xml"),
        (&[NsDecl::new("a", "http://www.w3.org/2000/xmlns/")], "xml"),
        (&[NsDecl::new("xml", A)], "reserved"),
        (&[NsDecl::new("XmLfoo", A)], "reserved"),
        (&[NsDecl::new("1a", A)], "NCName"),
        (&[NsDecl::new("a:b", A)], "NCName"),
        (&[NsDecl::new("", A)], "NCName"),
        (&[NsDecl::new("a", A), NsDecl::new("b", A)], "URI is requested twice"),
        (&[NsDecl::new("a", A), NsDecl::new("a", PIC)], "prefix is requested twice"),
    ];
    for (decls, why) in cases {
        let mut d = doc(src);
        let r = d.lens_mut::<Inline>().unwrap().insert_with_ns::<Graphic>(decls).map(|_| ());
        assert_matches!(&r, Err(Error::InvalidNamespaceDeclaration { reason, .. }) if reason.contains(why), "{decls:?}");
        assert!(!d.is_dirty());
        assert_eq!(d.serialize(), src);
    }
    // a character XML cannot hold in the URI
    let mut d = doc(src);
    assert_matches!(d.lens_mut::<Inline>().unwrap().insert_with_ns::<Graphic>(&[NsDecl::new("a", "urn:\u{1}")]), Err(Error::InvalidXmlChar { .. }));
    assert!(!d.is_dirty());
    // the element's own namespace not declared and not in scope
    let mut d = doc(src);
    assert_matches!(d.lens_mut::<Inline>().unwrap().insert_with_ns::<Graphic>(&[NsDecl::new("p", PIC)]), Err(Error::NamespaceNotInScope(u)) if u == A);
    assert!(!d.is_dirty());
    // placement refusal with declarations
    let mut d = doc(r#"<wp:inline xmlns:wp="urn:wp" xmlns:a="urn:a"><a:graphic/></wp:inline>"#);
    assert_matches!(d.lens_mut::<Inline>().unwrap().insert_with_ns::<Graphic>(&[NsDecl::new("r", R)]), Err(Error::AlreadyPresent(_)));
    assert!(!d.is_dirty());
}

#[test]
fn get_or_insert_returns_an_existing_child_unchanged() {
    let src = r#"<wp:inline xmlns:wp="urn:wp" xmlns:a="urn:a"><a:graphic/></wp:inline>"#;
    let mut d = doc(src);
    d.lens_mut::<Inline>().unwrap().get_or_insert_with_ns::<Graphic>(&[NsDecl::new("r", R)]).unwrap();
    assert!(!d.is_dirty());
    let mut d = doc(r#"<wp:inline xmlns:wp="urn:wp"/>"#);
    d.lens_mut::<Inline>().unwrap().get_or_insert_with_ns::<Graphic>(&[NsDecl::new("a", A)]).unwrap();
    assert_eq!(d.serialize(), r#"<wp:inline xmlns:wp="urn:wp"><a:graphic xmlns:a="urn:a"/></wp:inline>"#);
}

/// Every check twin answers exactly what its insert does (same error), and
/// an insert that fails changes nothing; brute force over documents,
/// declaration sets and operations.
#[test]
fn check_and_apply_agree() {
    let docs = [
        r#"<wp:inline xmlns:wp="urn:wp"/>"#,
        r#"<wp:inline xmlns:wp="urn:wp"><wp:extent/></wp:inline>"#,
        r#"<wp:inline xmlns:wp="urn:wp" xmlns:a="urn:a"><a:graphic/></wp:inline>"#,
        r#"<wp:inline xmlns:wp="urn:wp" xmlns:a="urn:other"><wp:extent/></wp:inline>"#,
        r#"<wp:inline xmlns:wp="urn:wp" xmlns="urn:a"/>"#,
        r#"<wp:inline xmlns:wp="urn:wp"><a:graphic xmlns:a="urn:a"/><wp:extent/></wp:inline>"#,
        r#"<wp:inline xmlns:wp="urn:wp"><mc:AlternateContent xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"/></wp:inline>"#,
    ];
    let decl_sets: &[&[NsDecl<'_>]] = &[
        &[],
        &[NsDecl::new("a", A)],
        &[NsDecl::new("a", A), NsDecl::new("r", R)],
        &[NsDecl::new("p", PIC)],
        &[NsDecl::new("xmlx", A)],
        &[NsDecl::new("a", A), NsDecl::new("a", R)],
        &[NsDecl::new("a", "")],
    ];
    type Op = fn(&mut LensMut<'_, Inline>, &[NsDecl<'_>]) -> (String, String);
    let ops: &[(&str, Op)] = &[
        ("insert graphic", |l, d| (format!("{:?}", l.check_insert_with_ns::<Graphic>(d).map(|_| ())), format!("{:?}", l.insert_with_ns::<Graphic>(d).map(|_| ())))),
        ("insert extent", |l, d| (format!("{:?}", l.check_insert_with_ns::<Extent>(d).map(|_| ())), format!("{:?}", l.insert_with_ns::<Extent>(d).map(|_| ())))),
        ("insert_by graphic", |l, d| (format!("{:?}", l.check_insert_by_with_ns::<Graphic>(|_, _| true, d).map(|_| ())), format!("{:?}", l.insert_by_with_ns::<Graphic>(|_, _| true, d).map(|_| ())))),
        ("insert_in_slot 1", |l, d| (format!("{:?}", l.check_insert_in_slot_with_ns::<Graphic>(1, |_, _| false, d).map(|_| ())), format!("{:?}", l.insert_in_slot_with_ns::<Graphic>(1, |_, _| false, d).map(|_| ())))),
        ("insert_in_slot 0", |l, d| (format!("{:?}", l.check_insert_in_slot_with_ns::<Graphic>(0, |_, _| false, d).map(|_| ())), format!("{:?}", l.insert_in_slot_with_ns::<Graphic>(0, |_, _| false, d).map(|_| ())))),
        ("get_or_insert", |l, d| (format!("{:?}", l.check_get_or_insert_with_ns::<Graphic>(d).map(|_| ())), format!("{:?}", l.get_or_insert_with_ns::<Graphic>(d).map(|_| ())))),
        ("insert_at 0", |l, d| (format!("{:?}", l.check_insert_at_with_ns::<Graphic>(0, d).map(|_| ())), format!("{:?}", l.insert_at_with_ns::<Graphic>(0, d).map(|_| ())))),
        ("insert_at end", |l, d| {
            let end = l.node().children().len();
            (format!("{:?}", l.check_insert_at_with_ns::<Graphic>(end, d).map(|_| ())), format!("{:?}", l.insert_at_with_ns::<Graphic>(end, d).map(|_| ())))
        }),
    ];
    let mut n = 0;
    for src in docs {
        for decls in decl_sets {
            for (name, op) in ops {
                let mut d = doc(src);
                let (check, apply) = op(&mut d.lens_mut::<Inline>().unwrap(), decls);
                assert_eq!(check, apply, "{name} on {src} with {decls:?}");
                if apply.starts_with("Err") {
                    assert!(!d.is_dirty(), "{name} on {src} with {decls:?}");
                    assert_eq!(d.serialize(), src);
                } else {
                    // whatever was inserted parses back
                    XmlDocument::parse(d.serialize().as_bytes()).unwrap_or_else(|e| panic!("{name} on {src} with {decls:?}: {e}"));
                }
                n += 1;
            }
        }
    }
    assert_eq!(n, 7 * 7 * 8);
}

/// PowerPoint declares extension namespaces on the element itself
/// (`<a16:rowId xmlns:a16="…">`); the same works for any `##other` child.
#[test]
fn extension_element_declares_its_own_namespace() {
    const A16: &str = "http://schemas.microsoft.com/office/drawing/2014/main";
    enum RowId {}
    impl ElementSpec for RowId {
        const QNAME: QName = QName::new(A16, "rowId");
        const SDK_NAME: &'static str = "a16:rowId";
        const ATTRS: &'static [AttrSpec] = &[];
        const CONTENT: Content = Content::Empty;
    }
    impl ChildOf<GraphicData> for RowId {}
    let mut d = doc(r#"<a:graphicData xmlns:a="urn:a"/>"#);
    d.lens_mut::<GraphicData>().unwrap().insert_with_ns::<RowId>(&[NsDecl::new("a16", A16)]).unwrap();
    assert_eq!(d.serialize(), format!(r#"<a:graphicData xmlns:a="urn:a"><a16:rowId xmlns:a16="{A16}"/></a:graphicData>"#));
}

/// Prefixes that Unicode calls alphabetic/alphanumeric but XML 1.0 does not
/// allow in names (review of PR #45, M1), next to ones it does allow.
#[test]
fn prefixes_follow_the_xml_name_productions() {
    for bad in ["a\u{B2}", "\u{AA}", "\u{B5}", "\u{24B6}", "a\u{BC}", "\u{B7}a", "-a", ".a", "\u{300}a", "a\u{D7}", "a\u{F7}", "a b", "a\u{2190}"] {
        let mut d = doc(r#"<wp:inline xmlns:wp="urn:wp"/>"#);
        let r = d.lens_mut::<Inline>().unwrap().insert_with_ns::<Graphic>(&[NsDecl::new(bad, A)]).map(|_| ());
        assert_matches!(&r, Err(Error::InvalidNamespaceDeclaration { reason, .. }) if reason.contains("NCName"), "{bad:?}");
        assert!(!d.is_dirty());
    }
    for good in ["\u{E9}", "\u{65E5}\u{672C}", "a\u{B7}b", "_x", "a-b.c9", "\u{10000}", "a\u{300}", "a\u{203F}"] {
        let mut d = doc(r#"<wp:inline xmlns:wp="urn:wp"/>"#);
        d.lens_mut::<Inline>().unwrap().insert_with_ns::<Graphic>(&[NsDecl::new(good, A)]).unwrap_or_else(|e| panic!("{good:?}: {e}"));
        assert!(d.serialize().contains(&format!("<{good}:graphic xmlns:{good}=\"urn:a\"/>")));
    }
}

/// Strict reference for NCName (XML 1.0 5th ed. §2.3, Namespaces §3), written
/// independently of the implementation, compared over every code point of
/// the BMP and a sample of the supplementary planes, as the first character
/// and as a later one.
#[test]
fn ncname_check_matches_the_xml_productions_everywhere() {
    fn start(c: char) -> bool {
        let u = c as u32;
        [(0x41, 0x5A), (0x5F, 0x5F), (0x61, 0x7A), (0xC0, 0xD6), (0xD8, 0xF6), (0xF8, 0x2FF), (0x370, 0x37D), (0x37F, 0x1FFF),
         (0x200C, 0x200D), (0x2070, 0x218F), (0x2C00, 0x2FEF), (0x3001, 0xD7FF), (0xF900, 0xFDCF), (0xFDF0, 0xFFFD), (0x10000, 0xEFFFF)]
            .iter()
            .any(|&(a, b)| (a..=b).contains(&u))
    }
    fn name(c: char) -> bool {
        let u = c as u32;
        start(c) || [(0x2D, 0x2E), (0x30, 0x39), (0xB7, 0xB7), (0x300, 0x36F), (0x203F, 0x2040)].iter().any(|&(a, b)| (a..=b).contains(&u))
    }
    let d = doc(r#"<wp:inline xmlns:wp="urn:wp"/>"#);
    let mut d2 = d.clone();
    let l = d2.lens_mut::<Inline>().unwrap();
    let accepted = |p: &str| match l.check_insert_with_ns::<Graphic>(&[NsDecl::new(p, A)]) {
        Ok(_) => true,
        Err(Error::InvalidNamespaceDeclaration { .. } | Error::InvalidXmlChar { .. }) => false,
        Err(e) => panic!("{p:?}: {e}"),
    };
    let mut checked = 0;
    for u in (0u32..0x1_0000).chain((0x1_0000..=0x10_FFFF).step_by(0x101)) {
        let Some(c) = char::from_u32(u) else { continue };
        if c == 'x' || c == 'X' {
            continue; // "x…" can start the reserved "xml"; covered elsewhere
        }
        assert_eq!(accepted(&c.to_string()), start(c), "U+{u:04X} as the first character");
        assert_eq!(accepted(&format!("a{c}")), name(c), "U+{u:04X} after the first character");
        checked += 1;
    }
    assert!(checked > 60_000);
}

/// A binding shadowed by an inner redeclaration does not count as bound
/// (review of PR #45, m3: a surviving mutant).
#[test]
fn a_shadowed_binding_is_not_reused() {
    let mut d = doc(r#"<wp:inline xmlns:wp="urn:wp" xmlns:x="urn:pic"><a:graphic xmlns:a="urn:a" xmlns:x="urn:z"><a:graphicData/></a:graphic></wp:inline>"#);
    {
        let inline = d.lens_mut::<Inline>().unwrap();
        let g = inline.into_child_at::<Graphic>(0).unwrap();
        let mut gd = g.into_child_at::<GraphicData>(0).unwrap();
        assert_eq!(gd.check_insert_with_ns::<Pic>(&[NsDecl::new("p", PIC)]).unwrap(), [("p".to_owned(), PIC.to_owned())]);
        gd.insert_with_ns::<Pic>(&[NsDecl::new("p", PIC)]).unwrap();
    }
    assert!(d.serialize().contains(r#"<a:graphicData><p:pic xmlns:p="urn:pic"/></a:graphicData>"#), "{}", d.serialize());
    // `x` still means urn:z inside a:graphic and urn:pic outside
    let again = doc(&d.serialize());
    let pic = again.root().elements().next().unwrap().elements().next().unwrap().elements().next().unwrap();
    assert_eq!(ns_of(pic), (PIC.into(), "pic".into()));
}

/// The prefix of a request is validated even when its URI is already bound
/// and nothing would be declared (review of PR #45, m3: a surviving mutant),
/// and by `get_or_insert_with_ns` even when the child exists (n1).
#[test]
fn requests_are_validated_even_when_nothing_is_declared() {
    let src = r#"<wp:inline xmlns:wp="urn:wp" xmlns:a="urn:a"><a:graphic/></wp:inline>"#;
    for bad in [NsDecl::new("xmlx", A), NsDecl::new("a\u{B2}", A), NsDecl::new("", A)] {
        let mut d = doc(src);
        let mut l = d.lens_mut::<Inline>().unwrap();
        assert_matches!(l.check_insert_in_slot_with_ns::<Graphic>(1, |_, _| true, &[bad]), Err(Error::InvalidNamespaceDeclaration { .. }));
        assert_matches!(l.check_get_or_insert_with_ns::<Graphic>(&[bad]), Err(Error::InvalidNamespaceDeclaration { .. }));
        assert_matches!(l.get_or_insert_with_ns::<Graphic>(&[bad]).map(|_| ()), Err(Error::InvalidNamespaceDeclaration { .. }));
        assert!(!d.is_dirty());
    }
    let mut d = doc(r#"<wp:inline xmlns:wp="urn:wp" xmlns:a="urn:a"/>"#);
    assert_matches!(d.lens_mut::<Inline>().unwrap().insert_with_ns::<Graphic>(&[NsDecl::new("xmlx", A)]), Err(Error::InvalidNamespaceDeclaration { .. }));
}

/// The check twins return exactly the declarations the insert writes.
#[test]
fn checks_return_the_planned_declarations() {
    type Case<'a> = (&'a str, &'a [NsDecl<'a>], &'a [(&'a str, &'a str)]);
    let cases: &[Case<'_>] = &[
        (r#"<wp:inline xmlns:wp="urn:wp"/>"#, &[NsDecl::new("a", A), NsDecl::new("r", R)], &[("a", A), ("r", R)]),
        (r#"<wp:inline xmlns:wp="urn:wp" xmlns:x="urn:a"/>"#, &[NsDecl::new("a", A), NsDecl::new("r", R)], &[("r", R)]),
        (r#"<wp:inline xmlns:wp="urn:wp" xmlns:a="urn:z" xmlns:r="urn:r"/>"#, &[NsDecl::new("a", A), NsDecl::new("r", R)], &[("a1", A)]),
        (r#"<wp:inline xmlns:wp="urn:wp" xmlns:a="urn:a" xmlns:r="urn:r"/>"#, &[NsDecl::new("a", A), NsDecl::new("r", R)], &[]),
    ];
    for (src, decls, want) in cases {
        let mut d = doc(src);
        let mut l = d.lens_mut::<Inline>().unwrap();
        let planned = l.check_insert_with_ns::<Graphic>(decls).unwrap();
        let want: Vec<(String, String)> = want.iter().map(|(p, u)| ((*p).to_owned(), (*u).to_owned())).collect();
        assert_eq!(planned, want, "{src}");
        assert_eq!(l.check_insert_at_with_ns::<Graphic>(0, decls).unwrap(), want, "{src} insert_at");
        let g = l.insert_with_ns::<Graphic>(decls).unwrap();
        let written: Vec<(String, String)> = g
            .node()
            .attrs()
            .iter()
            .filter(|a| a.name().is_namespace_declaration())
            .map(|a| (a.name().local().to_owned(), a.value().to_owned()))
            .collect();
        assert_eq!(written, want, "{src}");
    }
}

#[test]
fn insert_at_with_ns_declares_like_insert_by() {
    let src = r#"<wp:inline xmlns:wp="urn:wp"><wp:extent/></wp:inline>"#;
    let mut d = doc(src);
    d.lens_mut::<Inline>().unwrap().insert_at_with_ns::<Graphic>(1, &[NsDecl::new("a", A)]).unwrap();
    assert_eq!(d.serialize(), r#"<wp:inline xmlns:wp="urn:wp"><wp:extent/><a:graphic xmlns:a="urn:a"/></wp:inline>"#);
    // order is still checked locally: a:graphic before wp:extent is refused, nothing changes
    let mut d = doc(src);
    assert_matches!(d.lens_mut::<Inline>().unwrap().insert_at_with_ns::<Graphic>(0, &[NsDecl::new("a", A)]), Err(Error::UnresolvedSibling(_)));
    assert!(!d.is_dirty());
    // without the declaration the plain insert_at still refuses
    let mut d = doc(src);
    assert_matches!(d.lens_mut::<Inline>().unwrap().insert_at::<Graphic>(1), Err(Error::NamespaceNotInScope(_)));
}
