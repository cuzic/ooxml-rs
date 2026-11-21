//! Qualified names.
//!
//! Two shapes exist on purpose:
//!
//! - [`QName`] — a `'static` (namespace URI, local name) pair. Generated
//!   schema tables and hand-written code use it to *ask* for a name.
//! - [`XName`] — a name as it appears in a parsed document: the resolved
//!   namespace URI, the local name, and the **original prefix** (needed when
//!   a dirty node is re-rendered, ADR-0001 post-acceptance note).
//!
//! Prefixes are per-document spelling and are never compared
//! (ADR-0002 §3, "QName identity").

use std::fmt;
use std::sync::Arc;

/// `http://www.w3.org/XML/1998/namespace` — always bound to `xml`, never declared.
pub const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";
/// `http://www.w3.org/2000/xmlns/` — the namespace of `xmlns` / `xmlns:*` declarations.
pub const XMLNS_NS: &str = "http://www.w3.org/2000/xmlns/";
/// Markup Compatibility (`mc:`), acted on by the lens runtime (ADR-0002 §4.3).
pub const MC_NS: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";
/// Office document relationships (`r:`), used for `r:id`-style attributes.
pub const R_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
/// Strict (ECMA-376 Part 1, Strict) counterpart of [`R_NS`].
pub const R_NS_STRICT: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships";
/// VML Office namespace (`o:`), whose `o:relid` is a relationship id.
pub const VML_O_NS: &str = "urn:schemas-microsoft-com:office:office";
/// Attributes that hold a relationship id: every attribute in a
/// relationships namespace (`(ns, None)`), plus single names elsewhere
/// (`(ns, Some(local))`). Used by `Lens::detach_as` to refuse copies that
/// would duplicate a reference into Layer 3.
pub const RELATIONSHIP_ATTRIBUTES: &[(&str, Option<&str>)] =
    &[(R_NS, None), (R_NS_STRICT, None), (VML_O_NS, Some("relid"))];

// Target namespaces from the vendored SDK schemas and Transitional XSDs,
// plus Markup Compatibility (ADR-0015 §6.2). Keep this in sync with the
// generator inputs. Part metadata alone does not imply an element schema.
pub(crate) const TYPED_ELEMENT_NAMESPACES: &[&str] = &[
    "http://schemas.microsoft.com/ink/2010/main",
    "http://schemas.microsoft.com/office/2006/01/customui",
    "http://schemas.microsoft.com/office/2006/activeX",
    "http://schemas.microsoft.com/office/2006/coverPageProps",
    "http://schemas.microsoft.com/office/2006/customDocumentInformationPanel",
    "http://schemas.microsoft.com/office/2006/metadata/contentType",
    "http://schemas.microsoft.com/office/2006/metadata/customXsn",
    "http://schemas.microsoft.com/office/2006/metadata/longProperties",
    "http://schemas.microsoft.com/office/2006/metadata/properties/metaAttributes",
    "http://schemas.microsoft.com/office/2009/07/customui",
    "http://schemas.microsoft.com/office/2019/extlst",
    "http://schemas.microsoft.com/office/2020/mipLabelMetadata",
    "http://schemas.microsoft.com/office/drawing/2007/8/2/chart",
    "http://schemas.microsoft.com/office/drawing/2008/diagram",
    "http://schemas.microsoft.com/office/drawing/2010/chartDrawing",
    "http://schemas.microsoft.com/office/drawing/2010/compatibility",
    "http://schemas.microsoft.com/office/drawing/2010/diagram",
    "http://schemas.microsoft.com/office/drawing/2010/main",
    "http://schemas.microsoft.com/office/drawing/2010/picture",
    "http://schemas.microsoft.com/office/drawing/2010/slicer",
    "http://schemas.microsoft.com/office/drawing/2012/chart",
    "http://schemas.microsoft.com/office/drawing/2012/chartStyle",
    "http://schemas.microsoft.com/office/drawing/2012/main",
    "http://schemas.microsoft.com/office/drawing/2012/timeslicer",
    "http://schemas.microsoft.com/office/drawing/2013/main/command",
    "http://schemas.microsoft.com/office/drawing/2014/chart",
    "http://schemas.microsoft.com/office/drawing/2014/chart/ac",
    "http://schemas.microsoft.com/office/drawing/2014/chartex",
    "http://schemas.microsoft.com/office/drawing/2014/main",
    "http://schemas.microsoft.com/office/drawing/2016/11/diagram",
    "http://schemas.microsoft.com/office/drawing/2016/11/main",
    "http://schemas.microsoft.com/office/drawing/2016/12/diagram",
    "http://schemas.microsoft.com/office/drawing/2016/SVG/main",
    "http://schemas.microsoft.com/office/drawing/2016/ink",
    "http://schemas.microsoft.com/office/drawing/2017/03/chart",
    "http://schemas.microsoft.com/office/drawing/2017/decorative",
    "http://schemas.microsoft.com/office/drawing/2017/model3d",
    "http://schemas.microsoft.com/office/drawing/2018/animation",
    "http://schemas.microsoft.com/office/drawing/2018/animation/model3d",
    "http://schemas.microsoft.com/office/drawing/2018/hyperlinkcolor",
    "http://schemas.microsoft.com/office/drawing/2018/sketchyshapes",
    "http://schemas.microsoft.com/office/drawing/2020/classificationShape",
    "http://schemas.microsoft.com/office/drawing/2021/livefeed",
    "http://schemas.microsoft.com/office/drawing/2021/oembed",
    "http://schemas.microsoft.com/office/drawing/2021/scriptlink",
    "http://schemas.microsoft.com/office/drawing/2022/imageformula",
    "http://schemas.microsoft.com/office/excel/2006/main",
    "http://schemas.microsoft.com/office/excel/2010/spreadsheetDrawing",
    "http://schemas.microsoft.com/office/powerpoint/2010/main",
    "http://schemas.microsoft.com/office/powerpoint/2012/main",
    "http://schemas.microsoft.com/office/powerpoint/2012/roamingSettings",
    "http://schemas.microsoft.com/office/powerpoint/2013/main/command",
    "http://schemas.microsoft.com/office/powerpoint/2015/main",
    "http://schemas.microsoft.com/office/powerpoint/2017/10/main",
    "http://schemas.microsoft.com/office/powerpoint/2017/3/main",
    "http://schemas.microsoft.com/office/powerpoint/2018/4/main",
    "http://schemas.microsoft.com/office/powerpoint/2018/8/main",
    "http://schemas.microsoft.com/office/powerpoint/2019/12/main",
    "http://schemas.microsoft.com/office/powerpoint/2019/9/main/command",
    "http://schemas.microsoft.com/office/powerpoint/2020/02/main",
    "http://schemas.microsoft.com/office/powerpoint/2021/06/main",
    "http://schemas.microsoft.com/office/powerpoint/2022/03/main",
    "http://schemas.microsoft.com/office/powerpoint/2022/08/main",
    "http://schemas.microsoft.com/office/powerpoint/2023/02/main",
    "http://schemas.microsoft.com/office/spreadsheetml/2009/9/main",
    "http://schemas.microsoft.com/office/spreadsheetml/2010/11/ac",
    "http://schemas.microsoft.com/office/spreadsheetml/2010/11/main",
    "http://schemas.microsoft.com/office/spreadsheetml/2011/1/ac",
    "http://schemas.microsoft.com/office/spreadsheetml/2014/11/main",
    "http://schemas.microsoft.com/office/spreadsheetml/2014/revision",
    "http://schemas.microsoft.com/office/spreadsheetml/2015/02/main",
    "http://schemas.microsoft.com/office/spreadsheetml/2015/revision2",
    "http://schemas.microsoft.com/office/spreadsheetml/2016/01/main",
    "http://schemas.microsoft.com/office/spreadsheetml/2016/pivotdefaultlayout",
    "http://schemas.microsoft.com/office/spreadsheetml/2016/revision10",
    "http://schemas.microsoft.com/office/spreadsheetml/2016/revision3",
    "http://schemas.microsoft.com/office/spreadsheetml/2016/revision5",
    "http://schemas.microsoft.com/office/spreadsheetml/2016/revision6",
    "http://schemas.microsoft.com/office/spreadsheetml/2016/revision9",
    "http://schemas.microsoft.com/office/spreadsheetml/2017/dynamicarray",
    "http://schemas.microsoft.com/office/spreadsheetml/2017/revision16",
    "http://schemas.microsoft.com/office/spreadsheetml/2017/richdata",
    "http://schemas.microsoft.com/office/spreadsheetml/2017/richdata2",
    "http://schemas.microsoft.com/office/spreadsheetml/2018/08/main",
    "http://schemas.microsoft.com/office/spreadsheetml/2018/calcfeatures",
    "http://schemas.microsoft.com/office/spreadsheetml/2018/threadedcomments",
    "http://schemas.microsoft.com/office/spreadsheetml/2019/extlinksprops",
    "http://schemas.microsoft.com/office/spreadsheetml/2019/namedsheetviews",
    "http://schemas.microsoft.com/office/spreadsheetml/2020/pivotNov2020",
    "http://schemas.microsoft.com/office/spreadsheetml/2020/richdatawebimage",
    "http://schemas.microsoft.com/office/spreadsheetml/2020/richvaluerefresh",
    "http://schemas.microsoft.com/office/spreadsheetml/2020/threadedcomments2",
    "http://schemas.microsoft.com/office/spreadsheetml/2021/extlinks2021",
    "http://schemas.microsoft.com/office/spreadsheetml/2022/featurepropertybag",
    "http://schemas.microsoft.com/office/spreadsheetml/2022/pivotRichData",
    "http://schemas.microsoft.com/office/spreadsheetml/2022/pivotVersionInfo",
    "http://schemas.microsoft.com/office/spreadsheetml/2022/richvaluerel",
    "http://schemas.microsoft.com/office/spreadsheetml/2023/dataSourceVersioning",
    "http://schemas.microsoft.com/office/spreadsheetml/2023/externalCodeService",
    "http://schemas.microsoft.com/office/spreadsheetml/2023/msForms",
    "http://schemas.microsoft.com/office/spreadsheetml/2023/pivot2023Calculation",
    "http://schemas.microsoft.com/office/spreadsheetml/2024/pivotAutoRefresh",
    "http://schemas.microsoft.com/office/spreadsheetml/2024/pivotDynamicArrays",
    "http://schemas.microsoft.com/office/spreadsheetml/2024/workbookCompatibilityVersion",
    "http://schemas.microsoft.com/office/spreadsheetml/2025/externalCodeService2",
    "http://schemas.microsoft.com/office/spreadsheetml/2025/pivotDataSource",
    "http://schemas.microsoft.com/office/tasks/2019/documenttasks",
    "http://schemas.microsoft.com/office/thememl/2012/main",
    "http://schemas.microsoft.com/office/webextensions/taskpanes/2010/11",
    "http://schemas.microsoft.com/office/webextensions/webextension/2010/11",
    "http://schemas.microsoft.com/office/word/2006/wordml",
    "http://schemas.microsoft.com/office/word/2010/wordml",
    "http://schemas.microsoft.com/office/word/2010/wordprocessingCanvas",
    "http://schemas.microsoft.com/office/word/2010/wordprocessingDrawing",
    "http://schemas.microsoft.com/office/word/2010/wordprocessingGroup",
    "http://schemas.microsoft.com/office/word/2010/wordprocessingShape",
    "http://schemas.microsoft.com/office/word/2012/wordml",
    "http://schemas.microsoft.com/office/word/2012/wordprocessingDrawing",
    "http://schemas.microsoft.com/office/word/2015/wordml/symex",
    "http://schemas.microsoft.com/office/word/2016/wordml/cid",
    "http://schemas.microsoft.com/office/word/2018/wordml",
    "http://schemas.microsoft.com/office/word/2018/wordml/cex",
    "http://schemas.microsoft.com/office/word/2020/oembed",
    "http://schemas.microsoft.com/office/word/2020/wordml/sdtdatahash",
    "http://schemas.microsoft.com/office/word/2023/wordml/word16du",
    "http://schemas.microsoft.com/office/word/2024/wordml/sdtformatlock",
    "http://schemas.openxmlformats.org/drawingml/2006/chart",
    "http://schemas.openxmlformats.org/drawingml/2006/chartDrawing",
    "http://schemas.openxmlformats.org/drawingml/2006/compatibility",
    "http://schemas.openxmlformats.org/drawingml/2006/diagram",
    "http://schemas.openxmlformats.org/drawingml/2006/lockedCanvas",
    "http://schemas.openxmlformats.org/drawingml/2006/main",
    "http://schemas.openxmlformats.org/drawingml/2006/picture",
    "http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing",
    "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing",
    MC_NS,
    "http://schemas.openxmlformats.org/officeDocument/2006/bibliography",
    "http://schemas.openxmlformats.org/officeDocument/2006/characteristics",
    "http://schemas.openxmlformats.org/officeDocument/2006/custom-properties",
    "http://schemas.openxmlformats.org/officeDocument/2006/customXml",
    "http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes",
    "http://schemas.openxmlformats.org/officeDocument/2006/extended-properties",
    "http://schemas.openxmlformats.org/officeDocument/2006/math",
    R_NS,
    "http://schemas.openxmlformats.org/officeDocument/2006/sharedTypes",
    "http://schemas.openxmlformats.org/presentationml/2006/main",
    "http://schemas.openxmlformats.org/schemaLibrary/2006/main",
    "http://schemas.openxmlformats.org/spreadsheetml/2006/main",
    "http://schemas.openxmlformats.org/wordprocessingml/2006/main",
    "http://www.w3.org/2003/04/emma",
    "http://www.w3.org/2003/InkML",
    XML_NS,
    "urn:schemas-microsoft-com:office:excel",
    VML_O_NS,
    "urn:schemas-microsoft-com:office:powerpoint",
    "urn:schemas-microsoft-com:office:word",
    "urn:schemas-microsoft-com:vml",
];

/// OPC package relationships (`_rels/*.rels` root namespace).
pub const PACKAGE_RELATIONSHIPS_NS: &str =
    "http://schemas.openxmlformats.org/package/2006/relationships";
/// OPC content types (`[Content_Types].xml` root namespace).
pub const CONTENT_TYPES_NS: &str = "http://schemas.openxmlformats.org/package/2006/content-types";
/// Namespace URI prefix shared by every ECMA-376 **Strict** namespace and
/// relationship type. Packages using it are refused (ADR-0001 scope).
pub const STRICT_NS_PREFIX: &str = "http://purl.oclc.org/ooxml/";

/// `mc:AlternateContent`.
pub const MC_ALTERNATE_CONTENT: QName = QName::new(MC_NS, "AlternateContent");
/// `xml:space`.
pub const XML_SPACE: QName = QName::new(XML_NS, "space");

/// A `'static` qualified name: namespace URI plus local name.
///
/// An empty `ns` means "no namespace" (e.g. SpreadsheetML's unprefixed
/// attributes `r`, `s`, `t`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct QName {
    /// Namespace URI (`""` for no namespace).
    pub ns: &'static str,
    /// Local name.
    pub local: &'static str,
}

impl QName {
    /// Builds a name; usable in `const` items.
    pub const fn new(ns: &'static str, local: &'static str) -> Self {
        Self { ns, local }
    }

    /// `const` equality (stable `const` code cannot call `PartialEq`).
    pub const fn const_eq(self, other: QName) -> bool {
        crate::schema::const_str_eq(self.ns, other.ns)
            && crate::schema::const_str_eq(self.local, other.local)
    }
}

impl fmt::Display for QName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.ns.is_empty() {
            f.write_str(self.local)
        } else {
            write!(f, "{{{}}}{}", self.ns, self.local)
        }
    }
}

/// A name as found in (or inserted into) a document.
///
/// `xmlns` declarations are stored as attributes in [`XMLNS_NS`]: the
/// default declaration has local name `xmlns` and no prefix, `xmlns:p` has
/// prefix `xmlns` and local name `p` (the usual DOM convention).
///
/// One thin pointer (ADR-0010 stage 1a): the parser interns names per
/// document, so equal names usually share one allocation. Equality compares
/// the pointer first, then the content; the hash is computed from the
/// content only, so it agrees with equality.
#[derive(Clone)]
pub struct XName(Arc<XNameData>);

#[derive(Debug, PartialEq, Eq, Hash)]
struct XNameData {
    ns: Arc<str>,
    local: Box<str>,
    prefix: Option<Box<str>>,
}

impl PartialEq for XName {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0) || *self.0 == *other.0
    }
}

impl Eq for XName {}

impl std::hash::Hash for XName {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl fmt::Debug for XName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("XName").field("ns", &self.0.ns).field("local", &self.0.local).field("prefix", &self.0.prefix).finish()
    }
}

impl XName {
    /// Builds an owned name from its parts.
    pub fn new(ns: Arc<str>, local: &str, prefix: Option<&str>) -> Self {
        XName(Arc::new(XNameData { ns, local: local.into(), prefix: prefix.map(Into::into) }))
    }

    /// Resolved namespace URI (`""` for no namespace).
    pub fn ns(&self) -> &str {
        &self.0.ns
    }

    /// Local name.
    pub fn local(&self) -> &str {
        &self.0.local
    }

    /// Original (or chosen) prefix; `None` means unprefixed.
    pub fn prefix(&self) -> Option<&str> {
        self.0.prefix.as_deref()
    }

    /// True if this name is `q` (namespace URI and local name; prefix ignored).
    pub fn is(&self, q: QName) -> bool {
        *self.0.ns == *q.ns && *self.0.local == *q.local
    }

    /// True for `xmlns` / `xmlns:*` declarations.
    pub fn is_namespace_declaration(&self) -> bool {
        &*self.0.ns == XMLNS_NS
    }

    /// The prefix a namespace declaration declares: `None` for the default
    /// namespace (`xmlns="…"`), `Some(p)` for `xmlns:p`. `None` also for
    /// non-declarations; check [`Self::is_namespace_declaration`] first.
    pub fn declared_prefix(&self) -> Option<&str> {
        if self.is_namespace_declaration() && self.0.prefix.is_some() {
            Some(&self.0.local)
        } else {
            None
        }
    }

    /// Displays the name as written in markup (`prefix:local` or `local`),
    /// without building an intermediate `String`.
    pub fn display(&self) -> impl fmt::Display + '_ {
        fmt::from_fn(move |f| match &self.0.prefix {
            Some(p) => write!(f, "{p}:{}", self.0.local),
            None => f.write_str(&self.0.local),
        })
    }
}
