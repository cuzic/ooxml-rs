use ooxml_core::{Error, LeafStep, XmlDocument};
use std::mem::discriminant;

const CUSTOM: &str = "urn:x";
const WML: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

fn step<'a>(ns: &'a str, local: &'a str, index: usize) -> LeafStep<'a> {
    LeafStep { ns, local, index }
}

#[test]
fn replaces_text_without_changing_tags_or_siblings() {
    let xml = "<?xml version='1.0'?>\n<root xmlns='urn:x'><a  xml:space = 'preserve' >a&amp;<![CDATA[<b>]]>&#xD;</a ><b/></root>\n";
    let mut doc = XmlDocument::parse(xml.as_bytes()).unwrap();
    let path = [step(CUSTOM, "root", 0), step(CUSTOM, "a", 0)];
    let value = " <&>\r ";
    doc.check_set_leaf_text(&path, value).unwrap();
    assert!(!doc.is_dirty());
    assert_eq!(doc.serialize(), xml);

    doc.set_leaf_text(&path, value).unwrap();
    let expected = xml.replace("a&amp;<![CDATA[<b>]]>&#xD;", " &lt;&amp;&gt;&#13; ");
    assert_eq!(doc.serialize(), expected);
    let target = doc.root().elements().next().unwrap();
    assert!(target.subtree_dirty());
    assert!(!target.own_dirty());
    let reparsed = XmlDocument::parse(expected.as_bytes()).unwrap();
    assert_eq!(reparsed.root().elements().next().unwrap().text(), value);

    doc.set_leaf_text(&path, "").unwrap();
    assert_eq!(doc.serialize(), xml.replace("a&amp;<![CDATA[<b>]]>&#xD;", ""));
}

#[test]
fn expands_only_the_self_closing_target() {
    let xml = "<root xmlns='urn:x'><a>old</a><b  key = '&#x41;' /></root>";
    let mut doc = XmlDocument::parse(xml.as_bytes()).unwrap();
    let path = [step(CUSTOM, "root", 0), step(CUSTOM, "b", 0)];
    doc.set_leaf_text(&path, "").unwrap();
    assert!(!doc.is_dirty());
    assert_eq!(doc.serialize(), xml);

    doc.set_leaf_text(&path, "new").unwrap();
    assert_eq!(
        doc.serialize(),
        "<root xmlns='urn:x'><a>old</a><b  key = '&#x41;' >new</b></root>"
    );
    assert!(doc.root().subtree_dirty());
    assert!(!doc.root().own_dirty());
    assert!(doc.root().elements().nth(1).unwrap().own_dirty());
}

#[test]
fn selects_only_same_name_element_siblings() {
    let xml = "<root xmlns='urn:x' xmlns:y='urn:y' xmlns:x='urn:x'><group>\n<a>first</a><!-- keep --><b/><y:a>foreign</y:a><?keep pi?><x:a /></group></root>";
    let mut doc = XmlDocument::parse(xml.as_bytes()).unwrap();
    let path = [step(CUSTOM, "root", 0), step(CUSTOM, "group", 0), step(CUSTOM, "a", 1)];
    doc.set_leaf_text(&path, "second").unwrap();
    assert_eq!(doc.serialize(), xml.replace("<x:a />", "<x:a >second</x:a>"));
    assert!(doc.root().elements().next().unwrap().subtree_dirty());
}

#[test]
fn edits_core_properties_with_different_prefixes() {
    const CP: &str = "http://schemas.openxmlformats.org/package/2006/metadata/core-properties";
    const DC: &str = "http://purl.org/dc/elements/1.1/";
    let path = [step(CP, "coreProperties", 0), step(DC, "title", 0)];
    for (cp, dc) in [("cp", "dc"), ("props", "dublin")] {
        let xml = format!(
            "<{cp}:coreProperties xmlns:{cp}='{CP}' xmlns:{dc}='{DC}' xmlns:dcterms='http://purl.org/dc/terms/'><{dc}:title  >old</{dc}:title><dcterms:created/></{cp}:coreProperties>"
        );
        let mut doc = XmlDocument::parse(xml.as_bytes()).unwrap();
        doc.set_leaf_text(&path, "new").unwrap();
        assert_eq!(doc.serialize(), xml.replace(">old<", ">new<"));
    }
}

#[test]
fn edits_a_root_leaf_without_a_namespace() {
    let xml = "<title/>";
    let mut doc = XmlDocument::parse(xml.as_bytes()).unwrap();
    doc.set_leaf_text(&[step("", "title", 0)], "title").unwrap();
    assert_eq!(doc.serialize(), "<title>title</title>");
}

#[track_caller]
fn refuses(xml: &str, path: &[LeafStep<'_>], value: &str, expected: Error) {
    let mut doc = XmlDocument::parse(xml.as_bytes()).unwrap();
    let checked = doc.check_set_leaf_text(path, value).unwrap_err();
    assert_eq!(discriminant(&checked), discriminant(&expected), "{checked:?}");
    assert!(!doc.is_dirty());
    assert_eq!(doc.serialize().as_bytes(), xml.as_bytes());

    let written = doc.set_leaf_text(path, value).unwrap_err();
    assert_eq!(discriminant(&written), discriminant(&expected), "{written:?}");
    assert!(!doc.is_dirty());
    assert_eq!(doc.serialize().as_bytes(), xml.as_bytes());

    if let Error::InvalidXmlChar { ch, offset } = expected {
        for actual in [checked, written] {
            std::assert_matches!(actual, Error::InvalidXmlChar { ch: c, offset: at } if c == ch && at == offset);
        }
    }
}

#[test]
fn refuses_missing_elements_without_mutation() {
    let xml = "<root xmlns='urn:x'><a/><b/></root>";
    let root = step(CUSTOM, "root", 0);
    for path in [
        &[][..],
        &[step(CUSTOM, "wrong", 0)],
        &[step("urn:y", "root", 0)],
        &[step(CUSTOM, "root", 1)],
        &[root, step(CUSTOM, "a", 1)],
        &[root, step("urn:y", "a", 0)],
        &[root, step(CUSTOM, "missing", 0)],
    ] {
        refuses(xml, path, "new", Error::NoSuchElement);
    }
}

#[test]
fn refuses_children_markup_invalid_characters_and_nil_without_mutation() {
    let path = [step("", "root", 0), step("", "a", 0)];
    for child in ["<b/>", "<!-- keep -->", "<?keep pi?>"] {
        refuses(&format!("<root><a>old{child}</a></root>"), &path, "new", Error::NotLeaf);
    }
    refuses(
        "<root><a/></root>",
        &path,
        "é\0",
        Error::InvalidXmlChar { ch: '\0', offset: 2 },
    );
    for value in ["true", " 1 "] {
        refuses(
            &format!("<root xmlns:i='http://www.w3.org/2001/XMLSchema-instance'><a i:nil='{value}'/></root>"),
            &path,
            "new",
            Error::NilElement,
        );
    }
    let mut doc =
        XmlDocument::parse(b"<root xmlns:i='http://www.w3.org/2001/XMLSchema-instance'><a i:nil='false'/></root>")
            .unwrap();
    doc.set_leaf_text(&path, "new").unwrap();
    assert_eq!(
        doc.serialize(),
        "<root xmlns:i='http://www.w3.org/2001/XMLSchema-instance'><a i:nil='false'>new</a></root>"
    );
}

#[test]
fn refuses_word_document_property_edits_without_mutation() {
    let xml = format!(
        "<w:document xmlns:w='{WML}'><w:body><w:p><w:pPr></w:pPr><w:r><w:rPr><w:b/></w:rPr></w:r></w:p></w:body></w:document>"
    );
    let prefix = [step(WML, "document", 0), step(WML, "body", 0), step(WML, "p", 0)];
    let mut path = prefix.to_vec();
    path.push(step(WML, "pPr", 0));
    refuses(&xml, &path, "new", Error::TypedNamespace);

    path.truncate(prefix.len());
    path.extend([step(WML, "r", 0), step(WML, "rPr", 0), step(WML, "b", 0)]);
    refuses(&xml, &path, "new", Error::TypedNamespace);
}

#[test]
fn refuses_typed_intermediates_and_targets_inside_custom_xml() {
    for ns in [
        WML,
        "http://schemas.openxmlformats.org/spreadsheetml/2006/main",
        "http://schemas.openxmlformats.org/presentationml/2006/main",
        "http://schemas.openxmlformats.org/drawingml/2006/main",
        "urn:schemas-microsoft-com:vml",
        "http://schemas.openxmlformats.org/markup-compatibility/2006",
        "http://schemas.microsoft.com/office/word/2010/wordml",
    ] {
        let xml = format!("<root xmlns:t='{ns}'><t:wrapper><a/></t:wrapper><t:a/></root>");
        let root = step("", "root", 0);
        refuses(
            &xml,
            &[root, step(ns, "wrapper", 0), step("", "a", 0)],
            "new",
            Error::TypedNamespace,
        );
        refuses(&xml, &[root, step(ns, "a", 0)], "new", Error::TypedNamespace);
    }
}
