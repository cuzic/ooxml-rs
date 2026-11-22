use std::assert_matches;

use ooxml_core::{AttrSpec, ChildOf, Content, ElementSpec, Error, ParseOptions, QName, SimpleType, Slot, XmlDocument};

const ROW: QName = QName::new("", "row");
const CELL: QName = QName::new("urn:cell", "c");
const KEY: QName = QName::new("", "k");
const ADDED: QName = QName::new("", "added");
const ATTRS: &[AttrSpec] = &[
    AttrSpec { name: KEY, ty: SimpleType::String, default: None },
    AttrSpec { name: ADDED, ty: SimpleType::String, default: None },
];
const DEFER: &[QName] = &[ROW, CELL];

enum Row {}
impl ElementSpec for Row {
    const QNAME: QName = ROW;
    const SDK_NAME: &'static str = "row";
    const ATTRS: &'static [AttrSpec] = ATTRS;
    const CONTENT: Content = Content::Slots(&[Slot { names: &[CELL], wildcard: None, max_each: None, max_total: None }]);
}

enum Cell {}
impl ElementSpec for Cell {
    const QNAME: QName = CELL;
    const SDK_NAME: &'static str = "c";
    const ATTRS: &'static [AttrSpec] = ATTRS;
    const CONTENT: Content = Content::Empty;
}
impl ChildOf<Row> for Cell {}

#[test]
fn attribute_edits_match_full_parse_with_and_without_expansion() {
    for row in [
        "<row k='before'/>",
        "<row k='before'></row >",
        "<row k='before'>\u{feff}<!-- x --><?p y?>a&#9;&amp;\r\n<![CDATA[z]]><c:x/></row >",
        "<row k='before'><c:c xmlns:c='urn:other'><c:x/></c:c><c:c>v</c:c></row >",
    ] {
        let xml = format!("\u{feff}<root xmlns:c='urn:cell'><?p keep?>{row}<!-- keep --></root>");
        for preserve_spelling in [false, true] {
            let mut full = XmlDocument::parse(xml.as_bytes()).unwrap();
            let mut lazy = XmlDocument::parse_with(xml.as_bytes(), &ParseOptions::default().with_defer(DEFER)).unwrap();
            assert_eq!(lazy.serialize(), xml);
            for doc in [&mut full, &mut lazy] {
                let mut lens = doc.lens_mut_at::<Row>(&[1]).unwrap();
                if preserve_spelling {
                    lens.preserve_attribute_spelling();
                }
                lens.set_attr(KEY, "after &").unwrap();
                lens.set_attr(ADDED, "\r").unwrap();
                assert!(lens.remove_attr(KEY).unwrap());
            }
            let out = lazy.serialize();
            assert!(lazy.lens_at::<Row>(&[1]).unwrap().raw().is_deferred());
            assert_eq!(out, full.serialize());
            let span = lazy.lens_at::<Row>(&[1]).unwrap().raw().span().unwrap();
            assert!(out.starts_with(&xml[..span.start]));
            assert!(out.ends_with(&xml[span.end()..]));
            assert!(lazy.root().structurally_eq(full.root()));
            assert!(lazy.lens_mut_at::<Row>(&[1]).unwrap().release_children());
            assert_eq!(lazy.serialize(), out);
        }
    }
}

#[test]
fn nested_deferral_census_absolute_spans_and_edits() {
    let xml = "<root xmlns:p='urn:cell'><row k='old'><p:c/><p:c><row>data</row></p:c></row><row/></root>";
    let opts = ParseOptions::default().with_defer(DEFER).with_count(&[ROW, CELL, ROW, ADDED]);
    let mut lazy = XmlDocument::parse_with(xml.as_bytes(), &opts).unwrap();
    assert_eq!(lazy.census(ROW), Some(3));
    assert_eq!(lazy.census(CELL), Some(2));
    assert_eq!(lazy.census(KEY), None);
    assert_eq!(lazy.census(ADDED), Some(0));
    let row = lazy.root().elements().next().unwrap();
    assert!(row.is_deferred());
    let cell = row.elements().nth(1).unwrap();
    assert!(cell.is_deferred());
    assert_eq!(&xml[cell.content_span().unwrap()], "<row>data</row>");
    assert_eq!(cell.elements().next().unwrap().name().ns(), "");
    assert_eq!(lazy.census(CELL), Some(2));

    let edit = |doc: &mut XmlDocument| {
        let mut cell = doc.lens_mut_at::<Cell>(&[0, 1]).unwrap();
        cell.set_attr(KEY, "changed").unwrap();
    };
    let mut full = XmlDocument::parse(xml.as_bytes()).unwrap();
    edit(&mut lazy);
    edit(&mut full);
    let mut row = lazy.lens_mut_at::<Row>(&[0]).unwrap();
    assert!(!row.release_children());
    assert_eq!(lazy.serialize(), full.serialize());

    for doc in [&mut lazy, &mut full] {
        let mut row = doc.lens_mut_at::<Row>(&[0]).unwrap();
        let original_content = row.as_lens().raw().content_span();
        assert!(row.remove_at(0));
        assert_eq!(row.as_lens().raw().content_span(), original_content);
        assert!(!row.release_children());
    }
    assert_eq!(lazy.serialize(), full.serialize());
    assert_eq!(lazy.census(CELL), Some(2));
}

#[test]
fn child_mutators_preserve_skipped_siblings_and_refusals_preserve_bytes() {
    let xml = "<row xmlns:p='urn:cell'><!--keep--><p:c k='old'/></row >";
    let mut lazy = XmlDocument::parse_with(xml.as_bytes(), &ParseOptions::default().with_defer(DEFER)).unwrap();
    let mut full = XmlDocument::parse(xml.as_bytes()).unwrap();
    assert_matches!(
        lazy.lens_mut::<Row>().unwrap().set_attr(QName::new("http://www.w3.org/2000/xmlns/", "p"), "bad"),
        Err(Error::NamespaceDeclaration(_))
    );
    assert!(lazy.root().is_deferred());
    assert_eq!(lazy.serialize(), xml);
    for doc in [&mut lazy, &mut full] {
        doc.lens_mut::<Row>().unwrap().insert::<Cell>().unwrap();
    }
    assert_eq!(lazy.serialize(), full.serialize());
    assert_eq!(lazy.serialize(), "<row xmlns:p='urn:cell'><!--keep--><p:c k='old'/><p:c/></row >");
}

#[test]
fn malformed_content_has_the_same_error_and_position() {
    for bad in [
        "<!DOCTYPE row>", "<c x='&undefined;'/>", "&undefined;", "&#0;", "<c a='1' a='2'/>",
        "<u:c/>", "<c u:a='1'/>", "<c xmlns:p=''/>", "<c></d>", "<c", "<?xml version='1.0'?>",
    ] {
        let xml = format!("<root><row>{bad}</row></root>");
        let full = XmlDocument::parse(xml.as_bytes()).unwrap_err();
        let lazy = XmlDocument::parse_with(xml.as_bytes(), &ParseOptions::default().with_defer(DEFER)).unwrap_err();
        assert_eq!(format!("{lazy:?}"), format!("{full:?}"), "{bad}");
    }
}

#[test]
fn release_keeps_descendant_attribute_spelling_preference() {
    let xml = "<row xmlns:p='urn:cell'><row><p:c k = '&#x61;'/></row></row>";
    let options = ParseOptions::default().with_defer(DEFER);
    let mut doc = XmlDocument::parse_with(xml.as_bytes(), &options).unwrap();
    doc.lens_mut_at::<Cell>(&[0, 0]).unwrap().preserve_attribute_spelling();
    assert!(!doc.lens_mut::<Row>().unwrap().release_children());
    assert_eq!(doc.serialize(), xml);

    doc.lens_mut_at::<Cell>(&[0, 0]).unwrap().set_attr(ADDED, "v").unwrap();
    assert_eq!(doc.serialize(), "<row xmlns:p='urn:cell'><row><p:c k = '&#x61;' added=\"v\"/></row></row>");
}

#[test]
fn deferred_siblings_restore_the_inherited_namespace() {
    let xml = "<r xmlns:p='u1'><x xmlns:p='u2'><row/></x><row><p:a/></row></r>";
    let options = ParseOptions::default().with_defer(DEFER);
    let doc = XmlDocument::parse_with(xml.as_bytes(), &options).unwrap();
    let row = doc.root().elements().nth(1).unwrap();
    assert!(row.is_deferred());
    assert_eq!(row.elements().next().unwrap().name().ns(), "u1");
}
