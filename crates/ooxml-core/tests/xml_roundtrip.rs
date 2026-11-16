//! T1 (prolog/epilog + verbatim spans) and T1b (parser/renderer round-trip)
//! oracles of ADR-0001, on every XML part of the fixture and on synthetic
//! inputs covering shapes the fixture lacks.

use std::assert_matches;

use ooxml_core::{Error, Package, RawItem, XmlDocument};

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../reference/round_trip_corpus/fixture_minimal.xlsx");

/// Shapes the fixture does not cover (its README lists them).
const SYNTHETIC: &[&str] = &[
    // BOM, standalone decl, comment and PI in prolog/epilog, CRLF
    "\u{feff}<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<!-- c --><?pi x?>\r\n<a xmlns=\"urn:a\"><b/></a>\r\n<!-- after -->",
    // entities, char refs, CDATA, xml:space, mixed content, comments and PIs inside
    r#"<r xmlns="urn:r" xmlns:p="urn:p" xml:space="preserve"><t>  a &amp; b &lt; &#65;&#x42; </t><![CDATA[x <y> & z]]><!-- c --><?pi data?><p:q p:attr="v&quot;&apos;&#10;"/></r>"#,
    // mc:Ignorable, prefixed root, attribute with tab/newline refs, single quotes
    r#"<x:worksheet xmlns:x="urn:x" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" xmlns:x14ac="urn:x14ac" mc:Ignorable="x14ac"><x:row r='1' x14ac:dyDescent="0.25" t="a&#9;b"  ><x:c/></x:row></x:worksheet>"#,
    // default namespace undeclared on a child, re-declared prefixes
    r#"<a xmlns="urn:a" xmlns:p="urn:p1"><b xmlns=""><c/></b><p:d xmlns:p="urn:p2"><p:e/></p:d></a>"#,
];

fn fixture_xml_parts() -> Vec<(String, Vec<u8>)> {
    let pkg = Package::open(FIXTURE).unwrap();
    pkg.part_names()
        .filter(|n| n.ends_with(".xml") || n.ends_with(".rels"))
        .map(|n| (n.to_owned(), pkg.read_part(n).unwrap()))
        .collect::<Vec<_>>()
}

fn inputs() -> Vec<(String, Vec<u8>)> {
    let mut v = fixture_xml_parts();
    v.extend(SYNTHETIC.iter().enumerate().map(|(i, s)| (format!("synthetic #{i}"), s.as_bytes().to_vec())));
    v
}

/// T1: parse, serialize with zero edits, byte-identical.
#[test]
fn t1_unedited_parts_are_byte_identical() {
    let inputs = inputs();
    assert!(inputs.len() >= 9 + SYNTHETIC.len());
    for (name, bytes) in inputs {
        let doc = XmlDocument::parse(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(!doc.is_dirty());
        assert_eq!(doc.serialize().as_bytes(), bytes.as_slice(), "{name}");
    }
}

/// T1b: force every node dirty, re-render, re-parse, compare structurally.
#[test]
fn t1b_full_rerender_is_structurally_equal() {
    for (name, bytes) in inputs() {
        let doc = XmlDocument::parse(&bytes).unwrap();
        let mut forced = doc.clone();
        forced.force_rerender();
        assert!(forced.root().own_dirty() && forced.root().subtree_dirty());
        let out = forced.serialize();
        let reparsed = XmlDocument::parse(out.as_bytes()).unwrap_or_else(|e| panic!("{name}: {e}\n{out}"));
        assert!(doc.root().structurally_eq(reparsed.root()), "{name}: re-rendered tree differs\n{out}");
        // and the re-render really re-rendered (not the verbatim branch) where spellings differ
        if name == "synthetic #2" {
            assert!(out.contains(r#"r="1""#), "single quotes normalized on a dirty tag: {out}");
        }
    }
}

#[test]
fn spans_and_names() {
    let src = r#"<a xmlns="urn:a" xmlns:p="urn:p"><p:b x="1"/><c></c></a>"#;
    let doc = XmlDocument::parse(src.as_bytes()).unwrap();
    let root = doc.root();
    let span = root.span().unwrap();
    assert_eq!((span.start, span.close.map(|c| c.1)), (0, Some(src.len())));
    let b = root.elements().next().unwrap();
    assert_eq!((b.name().ns(), b.name().local(), b.name().prefix()), ("urn:p", "b", Some("p")));
    let bs = b.span().unwrap();
    assert_eq!(&src[bs.start..bs.end()], r#"<p:b x="1"/>"#);
    assert!(bs.is_self_closing());
    let c = root.elements().nth(1).unwrap();
    assert_eq!(c.name().ns(), "urn:a");
    let cs = c.span().unwrap();
    assert_eq!(&src[cs.start..cs.open_end], "<c>");
    assert_eq!(cs.close, Some((cs.open_end, cs.open_end + 4)));
}

#[test]
fn text_is_decoded_and_merged() {
    let doc = XmlDocument::parse(br#"<t xmlns="urn:x">a &amp; &#x42;<![CDATA[<c>]]></t>"#).unwrap();
    let root = doc.root();
    assert_eq!(root.text(), "a & B<c>");
    let texts: Vec<_> = root.children().iter().filter(|c| matches!(c, RawItem::Text(_))).collect();
    assert_eq!(texts.len(), 2, "entity refs merge into one text node; CDATA stays separate");
}

#[test]
fn malformed_and_unsupported_inputs_fail() {
    assert_matches!(XmlDocument::parse(b"<a><b></a>"), Err(Error::Xml { .. }));
    assert_matches!(XmlDocument::parse(b"<p:a/>"), Err(Error::Xml { .. }));
    assert_matches!(XmlDocument::parse(b"<a/><b/>"), Err(Error::Xml { .. }));
    assert_matches!(XmlDocument::parse(b"<a>&foo;</a>"), Err(Error::Xml { .. }));
    assert_matches!(XmlDocument::parse(&[0xFF, 0xFE, b'<', 0]), Err(Error::UnsupportedEncoding(_)));
    assert_matches!(
        XmlDocument::parse(br#"<?xml version="1.0" encoding="windows-1252"?><a/>"#),
        Err(Error::UnsupportedEncoding(_))
    );
}

#[test]
fn dtds_are_refused() {
    let xxe = br#"<?xml version="1.0"?><!DOCTYPE w [<!ENTITY xxe SYSTEM "file:///etc/passwd">]><w xmlns="urn:w">&xxe;</w>"#;
    assert_matches!(XmlDocument::parse(xxe), Err(Error::DtdNotAllowed { .. }));
    let bomb = br#"<!DOCTYPE a [<!ENTITY x "xx"><!ENTITY y "&x;&x;">]><a>&y;</a>"#;
    assert_matches!(XmlDocument::parse(bomb), Err(Error::DtdNotAllowed { .. }));
    assert_matches!(XmlDocument::parse(b"<!DOCTYPE a><a/>"), Err(Error::DtdNotAllowed { position: 0 }));
}
