//! QNames ooxml-docx recognizes without a generated view (context elements,
//! ADR-0004 §4), and small helpers.

use ooxml_core::{ElementSpec, QName, RawNode};
use ooxml_schema::wml::NS_W;

/// Is `node` the element of spec `E`?
pub(crate) fn is<E: ElementSpec>(node: &RawNode) -> bool {
    node.is(E::QNAME)
}

const fn w(local: &'static str) -> QName {
    QName::new(NS_W, local)
}

pub(crate) const SDT: QName = w("sdt");
pub(crate) const SDT_CONTENT: QName = w("sdtContent");
pub(crate) const SMART_TAG: QName = w("smartTag");
pub(crate) const CUSTOM_XML: QName = w("customXml");
pub(crate) const DIR: QName = w("dir");
pub(crate) const BDO: QName = w("bdo");
pub(crate) const RUBY: QName = w("ruby");
pub(crate) const RUBY_BASE: QName = w("rubyBase");

/// Relationship types of the story parts scanned for fields (W2).
pub(crate) const STORY_RELATIONSHIPS: &[&str] = &[
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/header",
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer",
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes",
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/endnotes",
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/comments",
];

/// Relationship type of `settings.xml`.
pub(crate) const SETTINGS_RELATIONSHIP: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings";

/// `prefix:local` for messages.
pub(crate) fn display_name(node: &RawNode) -> String {
    match node.name().prefix() {
        Some(p) => format!("{p}:{}", node.name().local()),
        None => node.name().local().into(),
    }
}
