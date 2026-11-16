//! Layer 3 OPC registries: `[Content_Types].xml` and relationship parts
//! (ADR-0001). They are written only by [`crate::Package`]'s primitives, under
//! the append-only invariant (existing entries are never renumbered):
//! [`crate::Package::add_part`], [`crate::Package::add_relationship`] and
//! [`crate::Package::add_related_part`] append entries,
//! [`crate::Package::set_content_type`] sets the type of one part's
//! `Override` (ADR-0001 post-acceptance note "registry additions"), and
//! [`crate::Package::remove_part`] removes entries only together with the
//! part they register (ADR-0001 interim post-acceptance note of 2026-11-16;
//! ADR-0003 §4.3 when accepted).
//!
//! Part names in this crate are zip entry names: no leading `/`
//! (`xl/workbook.xml`). OPC's `/`-prefixed names are converted on input.

use crate::error::{Error, Result};
use crate::name::{CONTENT_TYPES_NS, PACKAGE_RELATIONSHIPS_NS, QName};
use crate::xml::XmlDocument;

/// Name of the content-types part.
pub const CONTENT_TYPES_PART: &str = "[Content_Types].xml";
/// Name of the package-level relationships part.
pub const PACKAGE_RELS_PART: &str = "_rels/.rels";

/// The `officeDocument` relationship type (Transitional).
pub const REL_OFFICE_DOCUMENT: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";

/// Parsed `[Content_Types].xml`.
#[derive(Clone, Debug, Default)]
pub struct ContentTypes {
    defaults: Vec<(String, String)>,
    overrides: Vec<(String, String)>,
}

impl ContentTypes {
    /// Parses the part's bytes.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let doc = XmlDocument::parse(bytes)?;
        let root = doc.root();
        if !root.is(QName::new(CONTENT_TYPES_NS, "Types")) {
            return Err(Error::Package("[Content_Types].xml root is not <Types>".into()));
        }
        let mut ct = ContentTypes::default();
        for e in root.elements() {
            let get = |a: &'static str| e.attr(QName::new("", a)).map(String::from);
            if e.is(QName::new(CONTENT_TYPES_NS, "Default")) {
                if let (Some(ext), Some(t)) = (get("Extension"), get("ContentType")) {
                    ct.defaults.push((ext, t));
                }
            } else if e.is(QName::new(CONTENT_TYPES_NS, "Override"))
                && let (Some(p), Some(t)) = (get("PartName"), get("ContentType"))
            {
                ct.overrides.push((p.trim_start_matches('/').to_owned(), t));
            }
        }
        Ok(ct)
    }

    /// `(extension, content type)` defaults in document order.
    pub fn defaults(&self) -> &[(String, String)] {
        &self.defaults
    }

    /// `(part name, content type)` overrides in document order.
    pub fn overrides(&self) -> &[(String, String)] {
        &self.overrides
    }

    /// Records a new override (after [`crate::Package::add_part`] or
    /// [`crate::Package::set_content_type`] wrote it to the XML).
    pub(crate) fn push_override(&mut self, part: &str, content_type: &str) {
        self.overrides.push((part.trim_start_matches('/').to_owned(), content_type.to_owned()));
    }

    /// Records a new default (after a registry plan wrote it to the XML).
    pub(crate) fn push_default(&mut self, ext: &str, content_type: &str) {
        self.defaults.push((ext.to_owned(), content_type.to_owned()));
    }

    /// Sets the content type of every override for `part`.
    pub(crate) fn set_override(&mut self, part: &str, content_type: &str) {
        for (p, t) in &mut self.overrides {
            if same_part_name(p, part) {
                *t = content_type.to_owned();
            }
        }
    }

    /// Drops every override for `part` (after [`crate::Package::remove_part`]
    /// removed it from the XML).
    pub(crate) fn remove_override(&mut self, part: &str) {
        self.overrides.retain(|(p, _)| !same_part_name(p, part));
    }

    /// Content type of a part: an override, else the extension default.
    /// Comparison is ASCII case-insensitive, as OPC requires.
    pub fn content_type_of(&self, part: &str) -> Option<&str> {
        let part = part.trim_start_matches('/');
        if let Some((_, t)) = self.overrides.iter().find(|(p, _)| p.eq_ignore_ascii_case(part)) {
            return Some(t);
        }
        let ext = part.rsplit_once('.').map(|(_, e)| e)?;
        self.defaults.iter().find(|(e, _)| e.eq_ignore_ascii_case(ext)).map(|(_, t)| t.as_str())
    }
}

/// The calcChain relationship type (Transitional): the type xlsx Phase1
/// allows [`crate::Package::remove_part`] to drop.
pub const REL_CALC_CHAIN: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/calcChain";

/// The calcChain relationship type (Strict). `allowed_rel_types` is an exact
/// string match, so a caller that accepts Strict packages passes both.
pub const REL_CALC_CHAIN_STRICT: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships/calcChain";

/// Trims XML whitespace (`#x20 #x9 #xA #xD`), as the schema's whitespace
/// collapse does for `xsd:anyURI`, `xsd:ID` and token attributes.
pub(crate) fn xml_trim(s: &str) -> &str {
    s.trim_matches([' ', '\t', '\n', '\r'])
}

/// A part name normalized for OPC part-name equivalence: no leading `/`,
/// percent-decoded (an IRI and its `%`-encoded UTF-8 form are the same
/// name), ASCII lower case.
pub(crate) fn normalize_part_name(s: &str) -> String {
    let s = s.trim_start_matches('/');
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && let Some(h) = b.get(i + 1..i + 3)
            && h.iter().all(u8::is_ascii_hexdigit)
            && let Ok(v) = u8::from_str_radix(std::str::from_utf8(h).unwrap_or("zz"), 16)
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_ascii_lowercase()
}

/// OPC part-name equivalence (see [`normalize_part_name`]).
pub(crate) fn same_part_name(a: &str, b: &str) -> bool {
    normalize_part_name(a) == normalize_part_name(b)
}

/// Is a relationship's target inside or outside the package?
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetMode {
    /// A part in this package.
    Internal,
    /// A URI outside the package.
    External,
}

/// One `<Relationship>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Relationship {
    /// `Id`.
    pub id: String,
    /// `Type`.
    pub rel_type: String,
    /// `Target`, as written.
    pub target: String,
    /// `TargetMode`.
    pub target_mode: TargetMode,
}

/// A parsed relationships part.
#[derive(Clone, Debug)]
pub struct Relationships {
    source: Option<String>,
    rels: Vec<Relationship>,
}

impl Relationships {
    /// Parses the relationships of `source` (`None` for the package).
    pub fn parse(source: Option<&str>, bytes: &[u8]) -> Result<Self> {
        let doc = XmlDocument::parse(bytes)?;
        let root = doc.root();
        if !root.is(QName::new(PACKAGE_RELATIONSHIPS_NS, "Relationships")) {
            return Err(Error::Package("relationships part root is not <Relationships>".into()));
        }
        let mut rels = Vec::new();
        for e in root.elements().filter(|e| e.is(QName::new(PACKAGE_RELATIONSHIPS_NS, "Relationship"))) {
            let get = |a: &'static str| e.attr(QName::new("", a));
            let (Some(id), Some(t), Some(target)) = (get("Id"), get("Type"), get("Target")) else {
                return Err(Error::Package("<Relationship> lacks Id, Type or Target".into()));
            };
            rels.push(Relationship {
                id: id.into(),
                rel_type: t.into(),
                target: target.into(),
                target_mode: if get("TargetMode") == Some("External") {
                    TargetMode::External
                } else {
                    TargetMode::Internal
                },
            });
        }
        Ok(Relationships { source: source.map(String::from), rels })
    }

    /// An empty set (for parts without a relationships part).
    pub fn empty(source: Option<&str>) -> Self {
        Relationships { source: source.map(String::from), rels: Vec::new() }
    }

    /// The same set without the relationships whose `Id` is in `ids`
    /// (after [`crate::Package::remove_part`]; no re-parse, so it cannot fail).
    pub(crate) fn without_ids(&self, ids: &[String]) -> Relationships {
        let ids: Vec<&str> = ids.iter().map(|i| xml_trim(i)).collect();
        Relationships {
            source: self.source.clone(),
            rels: self.rels.iter().filter(|r| !ids.contains(&xml_trim(&r.id))).cloned().collect(),
        }
    }

    /// Records a relationship appended by
    /// [`crate::Package::add_relationship`].
    pub(crate) fn push(&mut self, rel: Relationship) {
        self.rels.push(rel);
    }

    /// Rewrites the `Target` of the relationship `id` (after
    /// [`crate::RegistryPlan::set_relationship_target`]).
    pub(crate) fn set_target(&mut self, id: &str, target: &str) {
        for r in self.rels.iter_mut().filter(|r| xml_trim(&r.id) == xml_trim(id)) {
            r.target = target.to_owned();
        }
    }

    /// The source part (`None` = package).
    pub fn source(&self) -> Option<&str> {
        self.source.as_deref()
    }

    /// All relationships in document order.
    pub fn iter(&self) -> impl Iterator<Item = &Relationship> {
        self.rels.iter()
    }

    /// Relationship by `Id`.
    pub fn by_id(&self, id: &str) -> Option<&Relationship> {
        self.rels.iter().find(|r| r.id == id)
    }

    /// Relationships of a type, in document order.
    pub fn by_type<'s>(&'s self, rel_type: &'s str) -> impl Iterator<Item = &'s Relationship> {
        self.rels.iter().filter(move |r| r.rel_type == rel_type)
    }

    /// The part name an internal relationship points at; `None` for
    /// external targets.
    pub fn resolve(&self, rel: &Relationship) -> Option<String> {
        (rel.target_mode == TargetMode::Internal).then(|| resolve_target(self.source.as_deref(), &rel.target))
    }
}

/// Name of the relationships part for `source` (`None` = package):
/// `<dir>/_rels/<name>.rels`.
pub fn rels_part_name(source: Option<&str>) -> String {
    match source {
        None => PACKAGE_RELS_PART.to_owned(),
        Some(p) => {
            let p = p.trim_start_matches('/');
            match p.rsplit_once('/') {
                Some((dir, name)) => format!("{dir}/_rels/{name}.rels"),
                None => format!("_rels/{p}.rels"),
            }
        }
    }
}

/// The relative target that reaches `part` from `source` (`None` = the
/// package, whose targets are relative to the root): the inverse of
/// [`resolve_target`] for part names without `.`/`..` segments.
///
/// `relative_target(Some("xl/workbook.xml"), "xl/worksheets/sheet2.xml")` is
/// `worksheets/sheet2.xml`; from `xl/worksheets/sheet1.xml` to
/// `xl/drawings/drawing1.xml` it is `../drawings/drawing1.xml`.
pub fn relative_target(source: Option<&str>, part: &str) -> String {
    let part = part.trim_start_matches('/');
    let base: Vec<&str> = match source.map(|s| s.trim_start_matches('/')).and_then(|s| s.rsplit_once('/')) {
        Some((dir, _)) => dir.split('/').collect(),
        None => Vec::new(),
    };
    let target: Vec<&str> = part.split('/').collect();
    let common = base.iter().zip(&target).take_while(|(a, b)| a == b).count().min(target.len().saturating_sub(1));
    let mut out: Vec<&str> = std::iter::repeat_n("..", base.len() - common).collect();
    out.extend(&target[common..]);
    out.join("/")
}

/// Resolves a relative relationship target against its source part.
pub fn resolve_target(source: Option<&str>, target: &str) -> String {
    let target = target.split(['#', '?']).next().unwrap_or_default();
    let mut segs: Vec<&str> = Vec::new();
    if let Some(abs) = target.strip_prefix('/') {
        segs.extend(abs.split('/'));
    } else {
        if let Some((dir, _)) = source.map(|s| s.trim_start_matches('/')).and_then(|s| s.rsplit_once('/')) {
            segs.extend(dir.split('/'));
        }
        segs.extend(target.split('/'));
    }
    let mut out: Vec<&str> = Vec::new();
    for s in segs {
        match s {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            s => out.push(s),
        }
    }
    out.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(rels_part_name(Some("xl/workbook.xml")), "xl/_rels/workbook.xml.rels");
        assert_eq!(rels_part_name(None), "_rels/.rels");
        assert_eq!(resolve_target(Some("xl/workbook.xml"), "worksheets/sheet1.xml"), "xl/worksheets/sheet1.xml");
        assert_eq!(resolve_target(Some("xl/worksheets/sheet1.xml"), "../drawings/d.xml"), "xl/drawings/d.xml");
        assert_eq!(resolve_target(None, "/xl/workbook.xml"), "xl/workbook.xml");
        assert_eq!(resolve_target(None, "xl/workbook.xml"), "xl/workbook.xml");
        for (src, part, rel) in [
            (Some("xl/workbook.xml"), "xl/worksheets/sheet2.xml", "worksheets/sheet2.xml"),
            (Some("xl/worksheets/sheet1.xml"), "xl/drawings/drawing1.xml", "../drawings/drawing1.xml"),
            (None, "xl/workbook.xml", "xl/workbook.xml"),
            (Some("top.xml"), "a/b.xml", "a/b.xml"),
            (Some("a/b/c.xml"), "d.xml", "../../d.xml"),
            (Some("ppt/slides/slide1.xml"), "ppt/slides/slide2.xml", "slide2.xml"),
        ] {
            assert_eq!(relative_target(src, part), rel);
            assert_eq!(resolve_target(src, &relative_target(src, part)), part);
        }
    }
}
