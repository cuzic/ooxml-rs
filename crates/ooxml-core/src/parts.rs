//! Package part kinds, generated from the Open XML SDK's `data/parts/*.json`
//! (ADR-0002 §6). Used read-only by [`Package`](crate::Package) and the
//! format crates to identify parts by relationship or content type.

#[path = "generated/parts.rs"]
mod generated;

pub use generated::PARTS;

/// One part kind (e.g. `WorksheetPart`).
#[derive(Clone, Copy, Debug)]
pub struct PartSpec {
    /// SDK part class name (`WorksheetPart`).
    pub name: &'static str,
    /// SDK base class (`OpenXmlPart`, …).
    pub base: &'static str,
    /// Fixed content type, if the kind has one (`WorkbookPart` does not:
    /// `.xlsx`/`.xlsm`/`.xltx` differ).
    pub content_type: Option<&'static str>,
    /// Relationship type that targets this part.
    pub relationship_type: Option<&'static str>,
    /// Default target file stem (`sheet`).
    pub target: Option<&'static str>,
    /// Default file extension, when not `.xml` (`.bin`, `.vml`, …).
    pub extension: Option<&'static str>,
    /// Root element's SDK complex type (`x:CT_Worksheet`).
    pub root_type: Option<&'static str>,
    /// Root element's namespace URI.
    pub root_namespace: Option<&'static str>,
    /// Root element's local name (`worksheet`).
    pub root_element: Option<&'static str>,
    /// Default directory, relative to the source part (`worksheets`).
    pub general_path: Option<&'static str>,
    /// Office version that introduced the kind, if not the base standard.
    pub version: Option<&'static str>,
    /// Part kinds this one may have relationships to.
    pub children: &'static [PartChild],
}

/// A permitted child part kind.
#[derive(Clone, Copy, Debug)]
pub struct PartChild {
    /// Child part kind name.
    pub name: &'static str,
    /// SDK API name.
    pub api_name: &'static str,
    /// More than one may exist.
    pub multiple: bool,
    /// At least one is required.
    pub required: bool,
}

/// Part kind by SDK name (`WorksheetPart`).
pub fn by_name(name: &str) -> Option<&'static PartSpec> {
    PARTS.iter().find(|p| p.name == name)
}

/// Part kinds targeted by a relationship type.
pub fn by_relationship_type(rel_type: &str) -> impl Iterator<Item = &'static PartSpec> {
    PARTS.iter().filter(move |p| p.relationship_type == Some(rel_type))
}

/// Part kinds with a fixed content type.
pub fn by_content_type(content_type: &str) -> impl Iterator<Item = &'static PartSpec> {
    PARTS.iter().filter(move |p| p.content_type == Some(content_type))
}
