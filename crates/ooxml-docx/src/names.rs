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
pub(crate) const FF_DATA: QName = w("ffData");
pub(crate) const TEXT_INPUT: QName = w("textInput");
pub(crate) const TEXT_INPUT_TYPE: QName = w("type");
pub(crate) const VAL: QName = w("val");
pub(crate) const W_ID: QName = w("id");
pub(crate) const W_TYPE: QName = w("type");
pub(crate) const R_ID: QName = QName::new(ooxml_schema::wml::NS_R, "id");
pub(crate) const PROOF_ERR: QName = w("proofErr");
pub(crate) const BOOKMARK_START: QName = w("bookmarkStart");
pub(crate) const NAME: QName = w("name");
pub(crate) const BOOKMARK_END: QName = w("bookmarkEnd");
pub(crate) const RPR_CHANGE: QName = w("rPrChange");
pub(crate) const HEADER_REFERENCE: QName = w("headerReference");
pub(crate) const FOOTER_REFERENCE: QName = w("footerReference");
pub(crate) const TITLE_PG: QName = w("titlePg");
pub(crate) const EVEN_AND_ODD_HEADERS: QName = w("evenAndOddHeaders");
pub(crate) const NO_PROOF: QName = w("noProof");
pub(crate) const LANG: QName = w("lang");
pub(crate) const RFONTS: QName = w("rFonts");
pub(crate) const HINT: QName = w("hint");

/// Range markers that may sit at a match's edges but not inside it
/// (ADR-0007 §1.1, "anchor"). `w:bookmarkStart`/`End` are anchors unless
/// collapsed.
pub(crate) const ANCHORS: &[&str] = &[
    "bookmarkStart",
    "bookmarkEnd",
    "commentRangeStart",
    "commentRangeEnd",
    "permStart",
    "permEnd",
    "moveFromRangeStart",
    "moveFromRangeEnd",
    "moveToRangeStart",
    "moveToRangeEnd",
    "customXmlInsRangeStart",
    "customXmlInsRangeEnd",
    "customXmlDelRangeStart",
    "customXmlDelRangeEnd",
    "customXmlMoveFromRangeStart",
    "customXmlMoveFromRangeEnd",
    "customXmlMoveToRangeStart",
    "customXmlMoveToRangeEnd",
];
pub(crate) const WRITE_PROTECTION: QName = w("writeProtection");
pub(crate) const RECOMMENDED: QName = w("recommended");

/// Relationship types of the story parts (W2 scan; ADR-0007 §2.1 stories),
/// in the order stories are listed after the main one.
pub(crate) const STORY_RELATIONSHIPS: &[&str] = &[
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/header",
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer",
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes",
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/endnotes",
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/comments",
];

/// Relationship type of `docProps/custom.xml` (package level).
pub(crate) const CUSTOM_PROPERTIES_RELATIONSHIP: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/custom-properties";

/// Namespace of `docProps/custom.xml`.
pub(crate) const CUSTOM_PROPERTIES_NS: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/custom-properties";

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
