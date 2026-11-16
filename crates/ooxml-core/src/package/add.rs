//! Layer 3 additions (ADR-0001 post-acceptance note "registry additions"):
//! adding parts and relationships, and setting a part's content type.
//!
//! Every operation is planned first, from `&self` and without changing
//! anything (not even part states); the plan is then applied by steps that
//! cannot fail. Each has a `check_*` twin that runs the same planning, so
//! `Err` from either means the package is exactly as before.
//!
//! The registries only grow: an `Override` or `Relationship` is appended as
//! the last child of its root, after a copy of the whitespace that precedes
//! the previous last entry (so an indented part stays indented); no existing
//! entry is moved, renumbered or rewritten. The one exception,
//! [`Package::set_content_type`], changes the `ContentType` of the
//! `Override`s of the one part it names.

use std::sync::Arc;

use super::{Package, Part, PartState, is_rels_part};
use crate::error::{Error, Result};
use crate::name::{CONTENT_TYPES_NS, PACKAGE_RELATIONSHIPS_NS, QName, XName};
use crate::opc::{
    CONTENT_TYPES_PART, Relationship, TargetMode, normalize_part_name, rels_part_name, resolve_target, same_part_name,
    xml_trim,
};
use crate::xml::{RawAttr, RawItem, RawNode, RawText, XmlDocument};
use zip::CompressionMethod;

/// Content type of relationships parts (ECMA-376 Part 2).
pub(crate) const RELATIONSHIPS_CONTENT_TYPE: &str = "application/vnd.openxmlformats-package.relationships+xml";

/// The content of a part to add.
#[derive(Clone, Debug)]
pub enum PartContent {
    /// Bytes, written as given. For an XML content type (`…/xml` or
    /// `…+xml`) they must be well-formed XML.
    Bytes(Vec<u8>),
    /// A tree, serialized on save; the part can be edited further through
    /// [`Package::xml_part_mut`].
    Xml(XmlDocument),
}

impl From<Vec<u8>> for PartContent {
    fn from(b: Vec<u8>) -> Self {
        PartContent::Bytes(b)
    }
}

impl From<&[u8]> for PartContent {
    fn from(b: &[u8]) -> Self {
        PartContent::Bytes(b.to_vec())
    }
}

impl From<XmlDocument> for PartContent {
    fn from(d: XmlDocument) -> Self {
        PartContent::Xml(d)
    }
}

/// What [`Package::add_part`] / [`Package::add_related_part`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AddedPart {
    /// The part name as stored (no leading `/`).
    pub name: String,
    /// Whether an `Override` was appended to `[Content_Types].xml` (`false`
    /// when an extension `Default` already gives exactly this type).
    pub content_type_override: bool,
    /// The `Id` of the relationship that [`Package::add_related_part`]
    /// appended (`None` for [`Package::add_part`]).
    pub relationship_id: Option<String>,
}

/// Why an addition was refused (nothing was changed).
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum AddRefusal {
    /// Not a valid OPC part name (ECMA-376 Part 2 §6.2.2 / §9.1.1).
    InvalidPartName {
        /// Which rule.
        reason: &'static str,
    },
    /// `[Content_Types].xml` or a relationships part: these are written only
    /// through the registry primitives.
    RegistryPart,
    /// A part with an equivalent name (ASCII case-insensitive,
    /// percent-decoded) exists.
    PartExists {
        /// The existing part.
        existing: String,
    },
    /// One name is a segment prefix of the other (`a/b` and `a/b/c`), which
    /// OPC forbids.
    SegmentConflict {
        /// The existing part.
        existing: String,
    },
    /// Not a media type (`type/subtype` with token characters, optional
    /// parameters).
    InvalidContentType,
    /// An `Override` for the name already exists with another content type
    /// (left over from a part that is gone).
    ContentTypeConflict {
        /// Its content type.
        existing: String,
    },
    /// The content type is XML but the bytes are not well-formed.
    MalformedXml {
        /// The parser's message.
        reason: String,
    },
    /// A registry part that must change cannot be edited in place.
    RegistryNotEditable {
        /// The registry part.
        part: String,
        /// Why (replaced wholesale, not well-formed, wrong root).
        reason: String,
    },
    /// The relationship's source part does not exist.
    SourceNotFound,
    /// An internal target resolves to no part.
    TargetNotFound {
        /// The resolved part name.
        resolved: String,
    },
    /// The target is empty, or has characters XML cannot carry.
    InvalidTarget,
    /// The relationship type is not an absolute URI.
    InvalidRelationshipType,
    /// No relationship `Id` or part name is free.
    Exhausted,
}

impl std::fmt::Display for AddRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AddRefusal::InvalidPartName { reason } => write!(f, "invalid part name: {reason}"),
            AddRefusal::RegistryPart => write!(f, "registry parts are written only by the registry primitives"),
            AddRefusal::PartExists { existing } => write!(f, "part {existing:?} already exists"),
            AddRefusal::SegmentConflict { existing } => {
                write!(f, "part {existing:?} and the new name are segment prefixes of each other")
            }
            AddRefusal::InvalidContentType => write!(f, "not a media type"),
            AddRefusal::ContentTypeConflict { existing } => write!(f, "an Override with content type {existing:?} exists"),
            AddRefusal::MalformedXml { reason } => write!(f, "content is not well-formed XML: {reason}"),
            AddRefusal::RegistryNotEditable { part, reason } => write!(f, "{part} cannot be edited in place: {reason}"),
            AddRefusal::SourceNotFound => write!(f, "the source part does not exist"),
            AddRefusal::TargetNotFound { resolved } => write!(f, "the target resolves to {resolved:?}, which is not a part"),
            AddRefusal::InvalidTarget => write!(f, "invalid relationship target"),
            AddRefusal::InvalidRelationshipType => write!(f, "the relationship type is not an absolute URI"),
            AddRefusal::Exhausted => write!(f, "no free Id or part name"),
        }
    }
}

/// Validates an OPC part name (zip entry form, no leading `/`):
/// ECMA-376 Part 2 §6.2.2.2–§6.2.2.3 (non-empty segments, no segment ending
/// in `.`, no `.`/`..` segments, no `%`-encoded `/` `\` or unreserved
/// characters, pchar only) plus "not ending in `/`".
pub(crate) fn validate_part_name(name: &str) -> std::result::Result<(), &'static str> {
    if name.is_empty() {
        return Err("empty");
    }
    if name.ends_with('/') {
        return Err("ends with '/'");
    }
    for seg in name.split('/') {
        if seg.is_empty() {
            return Err("empty segment");
        }
        if seg == "." || seg == ".." {
            return Err("'.' or '..' segment");
        }
        if seg.ends_with('.') {
            return Err("segment ends with '.'");
        }
    }
    let b = name.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == b'%' {
            let Some(h) = b.get(i + 1..i + 3).filter(|h| h.iter().all(u8::is_ascii_hexdigit)) else {
                return Err("'%' not followed by two hex digits");
            };
            let v = u8::from_str_radix(std::str::from_utf8(h).unwrap_or("00"), 16).unwrap_or(0);
            if v == b'/' || v == b'\\' {
                return Err("percent-encoded '/' or '\\'");
            }
            if v.is_ascii_alphanumeric() || matches!(v, b'-' | b'.' | b'_' | b'~') {
                return Err("percent-encoded unreserved character");
            }
            i += 3;
            continue;
        }
        let ok = c >= 0x80 // IRI ucschar (non-ASCII, already UTF-8)
            || c.is_ascii_alphanumeric()
            || matches!(c, b'-' | b'.' | b'_' | b'~' | b'!' | b'$' | b'&' | b'\'' | b'(' | b')' | b'*' | b'+' | b',' | b';' | b'=' | b':' | b'@' | b'/');
        if !ok {
            return Err("character not allowed in a part name");
        }
        i += 1;
    }
    Ok(())
}

/// `type/subtype` (RFC 7231 tokens) with optional `; name=value` parameters.
fn valid_media_type(ct: &str) -> bool {
    let token = |s: &str| {
        !s.is_empty() && s.bytes().all(|c| c.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&c))
    };
    let mut parts = ct.split(';');
    let Some((t, sub)) = parts.next().and_then(|m| m.split_once('/')) else { return false };
    token(t)
        && token(sub)
        && parts.all(|p| {
            let p = p.trim_matches(' ');
            p.split_once('=').is_some_and(|(k, v)| token(k) && !v.is_empty() && !v.contains(['\r', '\n']))
        })
}

fn is_xml_media_type(ct: &str) -> bool {
    let essence = ct.split(';').next().unwrap_or_default().trim().to_ascii_lowercase();
    essence.ends_with("/xml") || essence.ends_with("+xml")
}

fn valid_attr_text(s: &str) -> bool {
    crate::value::first_invalid_xml_char(s).is_none()
}

/// A registry part to append to (or to edit, for `set_content_type`).
struct RegistryEdit {
    /// Index into `parts`, or `None` when the relationships part is new.
    part: Option<usize>,
    /// Tree parsed during planning (the part was `Raw`) or built for a new
    /// relationships part; installed on apply.
    install: Option<XmlDocument>,
    action: RegistryAction,
}

enum RegistryAction {
    /// Append this element as the root's last child.
    Append(RawNode),
    /// Set the `ContentType` of these root children (`Override`s).
    SetContentType { indices: Vec<usize>, content_type: String },
}

/// What an addition will change, computed without mutating.
#[derive(Default)]
struct AddPlan {
    /// The new part (name, content type registered, content).
    part: Option<(String, Option<String>, PartContent)>,
    /// The new relationships part, if one is created: its name.
    new_rels_part: Option<(String, Option<String>)>,
    edits: Vec<RegistryEdit>,
    /// Relationship to record in the package-relationships cache.
    package_rel: Option<Relationship>,
    /// `(part, content type)` overrides to record in the cache.
    overrides: Vec<(String, String)>,
    /// `(part, content type)` overrides whose type changes in the cache.
    set_overrides: Vec<(String, String)>,
    relationship_id: Option<String>,
}

/// An element in the root's namespace and prefix, with unqualified
/// attributes in the given order.
fn element(root: &RawNode, local: &str, attrs: &[(&str, &str)]) -> RawNode {
    let mut n = RawNode::new_created(XName::new(Arc::from(root.name().ns()), local, root.name().prefix()));
    n.attrs = attrs
        .iter()
        .map(|(k, v)| RawAttr { name: XName::new(Arc::from(""), k, None), value: (*v).to_owned() })
        .collect();
    n
}

/// Appends `node` as the root's last child, after a copy of the whitespace
/// text that precedes the current last element (if any), so the layout of
/// an indented registry part is kept. Cannot fail.
fn append_root_child(doc: &mut XmlDocument, node: RawNode) {
    let root = doc.root_mut();
    let last_el = root.children.iter().rposition(|c| matches!(c, RawItem::Element(_)));
    let ws = last_el.and_then(|i| i.checked_sub(1)).and_then(|i| match &root.children[i] {
        RawItem::Text(t) if t.is_whitespace() && !t.cdata => Some(t.value.clone()),
        _ => None,
    });
    let at = last_el.map_or(root.children.len(), |i| i + 1);
    let mut items = Vec::new();
    if let Some(value) = ws {
        items.push(RawItem::Text(RawText { value, span: None, cdata: false }));
    }
    items.push(RawItem::Element(node));
    if root.span.is_some_and(|s| s.close.is_none()) && root.children.is_empty() {
        root.own_dirty = true; // a self-closing root gains its first child
    }
    root.children.splice(at..at, items);
    root.subtree_dirty = true;
}

impl Package {
    /// The tree of a registry part for planning: borrowed when parsed, parsed
    /// into `fresh` when raw (installed on apply). Replaced parts and trees
    /// whose root is not `root` are not editable.
    fn registry_tree<'s>(
        &'s self,
        i: usize,
        root: QName,
        fresh: &'s mut Option<XmlDocument>,
    ) -> std::result::Result<&'s XmlDocument, AddRefusal> {
        let not_editable = |reason: String| AddRefusal::RegistryNotEditable { part: self.parts[i].name.clone(), reason };
        let tree: &XmlDocument = match &self.parts[i].state {
            PartState::Parsed(d) => d,
            PartState::Raw => &*fresh.insert(self.parse_raw(i).map_err(not_editable)?),
            PartState::Replaced(_) => return Err(not_editable("replaced wholesale; cannot be edited in place".into())),
        };
        if !tree.root().is(root) {
            return Err(not_editable(format!("root is not <{}>", root.local)));
        }
        Ok(tree)
    }

    /// Plans the registration of a new part name: validity, collisions and
    /// the content-type entry. `extra` names parts this same plan creates.
    fn plan_new_name(
        &self,
        name: &str,
        content_type: &str,
        extra: &[&str],
        plan: &mut AddPlan,
    ) -> std::result::Result<Option<String>, AddRefusal> {
        validate_part_name(name).map_err(|reason| AddRefusal::InvalidPartName { reason })?;
        if !valid_media_type(content_type) || !valid_attr_text(content_type) {
            return Err(AddRefusal::InvalidContentType);
        }
        let norm = normalize_part_name(name);
        let existing = self.parts.iter().map(|p| p.name.as_str()).chain(extra.iter().copied());
        for e in existing {
            let ne = normalize_part_name(e);
            if ne == norm {
                return Err(AddRefusal::PartExists { existing: e.to_owned() });
            }
            if ne.starts_with(&format!("{norm}/")) || norm.starts_with(&format!("{ne}/")) {
                return Err(AddRefusal::SegmentConflict { existing: e.to_owned() });
            }
        }
        // An Override left for this name decides: same type → nothing to do.
        let stale: Vec<&str> = self
            .content_types
            .overrides()
            .iter()
            .filter(|(p, _)| same_part_name(p, name))
            .map(|(_, t)| t.as_str())
            .collect();
        if let Some(t) = stale.iter().find(|t| !t.eq_ignore_ascii_case(content_type)) {
            return Err(AddRefusal::ContentTypeConflict { existing: (*t).to_owned() });
        }
        if !stale.is_empty() {
            return Ok(None);
        }
        let ext = name.rsplit_once('.').map(|(_, e)| e).filter(|e| !e.contains('/'));
        let by_default = ext.and_then(|e| {
            self.content_types.defaults().iter().find(|(d, _)| d.eq_ignore_ascii_case(e)).map(|(_, t)| t.as_str())
        });
        if by_default.is_some_and(|t| t.eq_ignore_ascii_case(content_type)) {
            return Ok(None);
        }
        let ct = self.find(CONTENT_TYPES_PART).ok_or_else(|| AddRefusal::RegistryNotEditable {
            part: CONTENT_TYPES_PART.into(),
            reason: "missing".into(),
        })?;
        let mut fresh = None;
        let tree = self.registry_tree(ct, QName::new(CONTENT_TYPES_NS, "Types"), &mut fresh)?;
        let node = element(tree.root(), "Override", &[("PartName", &format!("/{name}")), ("ContentType", content_type)]);
        plan.edits.push(RegistryEdit { part: Some(ct), install: fresh, action: RegistryAction::Append(node) });
        plan.overrides.push((name.to_owned(), content_type.to_owned()));
        Ok(Some(content_type.to_owned()))
    }

    fn plan_add_part(&self, name: &str, content_type: &str, content: &PartContent) -> std::result::Result<AddPlan, AddRefusal> {
        let name = name.trim_start_matches('/');
        if name.eq_ignore_ascii_case(CONTENT_TYPES_PART) || is_rels_part(&normalize_part_name(name)) {
            return Err(AddRefusal::RegistryPart);
        }
        if let PartContent::Bytes(b) = content
            && is_xml_media_type(content_type)
        {
            XmlDocument::parse(b).map_err(|e| AddRefusal::MalformedXml { reason: e.to_string() })?;
        }
        let mut plan = AddPlan::default();
        let registered = self.plan_new_name(name, content_type, &[], &mut plan)?;
        plan.part = Some((name.to_owned(), registered, content.clone()));
        Ok(plan)
    }

    /// Plans a relationship from `source` (`None` = package). `pending` is a
    /// part this same plan adds (a valid internal target).
    fn plan_add_relationship(
        &self,
        source: Option<&str>,
        rel_type: &str,
        target: &str,
        mode: TargetMode,
        pending: Option<&str>,
        plan: &mut AddPlan,
    ) -> std::result::Result<String, AddRefusal> {
        let source = match source {
            None => None,
            Some(s) => {
                let i = self.find_ci(s).ok_or(AddRefusal::SourceNotFound)?;
                let s = self.parts[i].name.as_str();
                if s.eq_ignore_ascii_case(CONTENT_TYPES_PART) || is_rels_part(&normalize_part_name(s)) {
                    return Err(AddRefusal::RegistryPart);
                }
                Some(s)
            }
        };
        let uri_like = |s: &str| !s.is_empty() && valid_attr_text(s) && !s.chars().any(|c| c.is_whitespace() || c.is_control());
        if !uri_like(rel_type) || !rel_type.contains(':') {
            return Err(AddRefusal::InvalidRelationshipType);
        }
        if !uri_like(target) {
            return Err(AddRefusal::InvalidTarget);
        }
        if mode == TargetMode::Internal {
            if target.contains(['#', '?', '\\']) {
                return Err(AddRefusal::InvalidTarget);
            }
            let resolved = resolve_target(source, target);
            if self.find_ci(&resolved).is_none() && !pending.is_some_and(|p| same_part_name(p, &resolved)) {
                return Err(AddRefusal::TargetNotFound { resolved });
            }
        }
        let rels_name = rels_part_name(source);
        let mut attrs: Vec<(&str, &str)> = Vec::new();
        let (id, edit) = match self.find_ci(&rels_name) {
            Some(ri) => {
                let mut fresh = None;
                let tree = self.registry_tree(ri, QName::new(PACKAGE_RELATIONSHIPS_NS, "Relationships"), &mut fresh)?;
                let ids: Vec<String> = tree
                    .root()
                    .elements()
                    .filter(|e| e.is(QName::new(PACKAGE_RELATIONSHIPS_NS, "Relationship")))
                    .filter_map(|e| e.attr(QName::new("", "Id")).map(|v| xml_trim(v).to_owned()))
                    .collect();
                let id = next_relationship_id(&ids).ok_or(AddRefusal::Exhausted)?;
                attrs.extend([("Id", id.as_str()), ("Type", rel_type), ("Target", target)]);
                if mode == TargetMode::External {
                    attrs.push(("TargetMode", "External"));
                }
                let node = element(tree.root(), "Relationship", &attrs);
                (id.clone(), RegistryEdit { part: Some(ri), install: fresh, action: RegistryAction::Append(node) })
            }
            None => {
                // A new relationships part, registered like any new part.
                let registered = self.plan_new_name(&rels_name, RELATIONSHIPS_CONTENT_TYPE, pending.as_slice(), plan)?;
                let doc = XmlDocument::parse(
                    format!(
                        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<Relationships xmlns=\"{PACKAGE_RELATIONSHIPS_NS}\"/>"
                    )
                    .as_bytes(),
                )
                .map_err(|e| AddRefusal::MalformedXml { reason: e.to_string() })?;
                let id = "rId1".to_owned();
                attrs.extend([("Id", id.as_str()), ("Type", rel_type), ("Target", target)]);
                if mode == TargetMode::External {
                    attrs.push(("TargetMode", "External"));
                }
                let node = element(doc.root(), "Relationship", &attrs);
                plan.new_rels_part = Some((rels_name.clone(), registered));
                (id.clone(), RegistryEdit { part: None, install: Some(doc), action: RegistryAction::Append(node) })
            }
        };
        // A plan may append to one raw registry part twice (two Overrides):
        // apply installs the first tree and edits it for the rest.
        plan.edits.push(edit);
        if source.is_none() {
            plan.package_rel = Some(Relationship {
                id: id.clone(),
                rel_type: rel_type.to_owned(),
                target: target.to_owned(),
                target_mode: mode,
            });
        }
        plan.relationship_id = Some(id.clone());
        Ok(id)
    }

    fn plan_set_content_type(&self, name: &str, content_type: &str) -> std::result::Result<AddPlan, AddRefusal> {
        let i = self.find_ci(name).ok_or(AddRefusal::SourceNotFound)?;
        let name = self.parts[i].name.clone();
        if name.eq_ignore_ascii_case(CONTENT_TYPES_PART) || is_rels_part(&normalize_part_name(&name)) {
            return Err(AddRefusal::RegistryPart);
        }
        if !valid_media_type(content_type) || !valid_attr_text(content_type) {
            return Err(AddRefusal::InvalidContentType);
        }
        let mut plan = AddPlan::default();
        if self.content_type(&name).is_some_and(|t| t == content_type) {
            return Ok(plan);
        }
        let ct = self.find(CONTENT_TYPES_PART).ok_or_else(|| AddRefusal::RegistryNotEditable {
            part: CONTENT_TYPES_PART.into(),
            reason: "missing".into(),
        })?;
        let mut fresh = None;
        let tree = self.registry_tree(ct, QName::new(CONTENT_TYPES_NS, "Types"), &mut fresh)?;
        let indices: Vec<usize> = tree
            .root()
            .indexed_elements()
            .filter(|(_, e)| {
                e.is(QName::new(CONTENT_TYPES_NS, "Override"))
                    && e.attr(QName::new("", "PartName")).is_some_and(|p| same_part_name(xml_trim(p), &name))
            })
            .map(|(i, _)| i)
            .collect();
        let action = if indices.is_empty() {
            plan.overrides.push((name.clone(), content_type.to_owned()));
            RegistryAction::Append(element(
                tree.root(),
                "Override",
                &[("PartName", &format!("/{name}")), ("ContentType", content_type)],
            ))
        } else {
            plan.set_overrides.push((name.clone(), content_type.to_owned()));
            RegistryAction::SetContentType { indices, content_type: content_type.to_owned() }
        };
        plan.edits.push(RegistryEdit { part: Some(ct), install: fresh, action });
        Ok(plan)
    }

    /// Applies a validated plan. Nothing here can fail.
    fn apply_add(&mut self, plan: AddPlan) {
        // Two appends to one raw registry part in one plan: the first
        // installs the tree, later ones edit it.
        for edit in plan.edits {
            let doc: &mut XmlDocument = match edit.part {
                Some(i) => {
                    let part = &mut self.parts[i];
                    if let Some(doc) = edit.install
                        && !matches!(part.state, PartState::Parsed(_))
                    {
                        part.state = PartState::Parsed(doc);
                    }
                    let PartState::Parsed(doc) = &mut part.state else {
                        debug_assert!(false, "planned as parsed");
                        continue;
                    };
                    doc
                }
                None => {
                    let (name, _) = plan.new_rels_part.clone().expect("planned with a new relationships part");
                    self.parts.push(Part {
                        name,
                        index: 0,
                        added: true,
                        compression: CompressionMethod::Deflated,
                        state: PartState::Parsed(edit.install.expect("built during planning")),
                    });
                    let Some(Part { state: PartState::Parsed(doc), .. }) = self.parts.last_mut() else {
                        unreachable!()
                    };
                    doc
                }
            };
            match edit.action {
                RegistryAction::Append(node) => append_root_child(doc, node),
                RegistryAction::SetContentType { indices, content_type } => {
                    let root = doc.root_mut();
                    for i in indices {
                        if let Some(RawItem::Element(e)) = root.children.get_mut(i)
                            && let Some(a) = e.attrs.iter_mut().find(|a| a.name.is(QName::new("", "ContentType")))
                        {
                            a.value = content_type.clone();
                            e.own_dirty = true;
                            e.subtree_dirty = true;
                        }
                    }
                    root.subtree_dirty = true;
                }
            }
        }
        if let Some((name, _, content)) = plan.part {
            let state = match content {
                PartContent::Bytes(b) => PartState::Replaced(b),
                PartContent::Xml(d) => PartState::Parsed(d),
            };
            self.parts.push(Part { name, index: 0, added: true, compression: CompressionMethod::Deflated, state });
        }
        for (p, t) in plan.overrides {
            self.content_types.push_override(&p, &t);
        }
        for (p, t) in plan.set_overrides {
            self.content_types.set_override(&p, &t);
        }
        if let Some(r) = plan.package_rel {
            self.package_rels.push(r);
        }
    }

    /// Adds a new part and registers its content type: nothing when an
    /// extension `Default` already gives exactly `content_type` (ASCII
    /// case-insensitive) or an `Override` for the name already does, else
    /// an `Override` appended to `[Content_Types].xml`. The part is written
    /// fresh on save, after the original entries.
    ///
    /// `name` is an OPC part name in zip form (a leading `/` is dropped).
    /// No relationship is added; see [`Self::add_related_part`] and
    /// [`Self::add_relationship`].
    ///
    /// Refusals ([`Error::AddRefused`] with an [`AddRefusal`]; nothing is
    /// changed): an invalid part name or media type, a registry part, a part
    /// with an equivalent name (ASCII case-insensitive, percent-decoded) or
    /// a segment-prefix conflict, an `Override` left for the name with
    /// another type, XML bytes that are not well-formed for an XML content
    /// type, or a `[Content_Types].xml` that was replaced or does not parse.
    pub fn add_part(&mut self, name: &str, content_type: &str, content: impl Into<PartContent>) -> Result<AddedPart> {
        let content = content.into();
        let plan = self.plan_add_part(name, content_type, &content).map_err(|r| refused(name, r))?;
        Ok(self.finish(plan))
    }

    /// The validation of [`Self::add_part`], without changing anything.
    pub fn check_add_part(&self, name: &str, content_type: &str, content: &PartContent) -> Result<()> {
        self.plan_add_part(name, content_type, content).map(drop).map_err(|r| refused(name, r))
    }

    /// Adds a part and a relationship to it from `source` (`None` = the
    /// package) in one validated step: the target is `name` relative to the
    /// source (see [`crate::opc::relative_target`]). Refused if either half
    /// would be ([`Self::add_part`], [`Self::add_relationship`]); then
    /// neither is applied.
    pub fn add_related_part(
        &mut self,
        source: Option<&str>,
        rel_type: &str,
        name: &str,
        content_type: &str,
        content: impl Into<PartContent>,
    ) -> Result<AddedPart> {
        let content = content.into();
        let plan = self.plan_add_related(source, rel_type, name, content_type, &content)?;
        Ok(self.finish(plan))
    }

    /// The validation of [`Self::add_related_part`], without changing
    /// anything. Returns the relationship `Id` it would allocate.
    pub fn check_add_related_part(
        &self,
        source: Option<&str>,
        rel_type: &str,
        name: &str,
        content_type: &str,
        content: &PartContent,
    ) -> Result<String> {
        let plan = self.plan_add_related(source, rel_type, name, content_type, content)?;
        Ok(plan.relationship_id.unwrap_or_default())
    }

    fn plan_add_related(
        &self,
        source: Option<&str>,
        rel_type: &str,
        name: &str,
        content_type: &str,
        content: &PartContent,
    ) -> Result<AddPlan> {
        let mut plan = self.plan_add_part(name, content_type, content).map_err(|r| refused(name, r))?;
        let name = name.trim_start_matches('/');
        let source_name = match source {
            None => None,
            Some(s) => Some(
                self.find_ci(s)
                    .map(|i| self.parts[i].name.clone())
                    .ok_or_else(|| refused(s, AddRefusal::SourceNotFound))?,
            ),
        };
        let target = crate::opc::relative_target(source_name.as_deref(), name);
        self.plan_add_relationship(source_name.as_deref(), rel_type, &target, TargetMode::Internal, Some(name), &mut plan)
            .map_err(|r| refused(source.unwrap_or(""), r))?;
        Ok(plan)
    }

    /// Appends a relationship to `source`'s relationships part (`None` = the
    /// package's `_rels/.rels`) and returns its `Id`: `rId{n}` with `n` one
    /// more than the largest number among the existing `rId<digits>` Ids
    /// (`rId1` for a new part), skipping any Id already used under ASCII
    /// case folding. No existing relationship is renumbered or rewritten.
    /// A missing relationships part is created and registered (an
    /// `Override` only if no `rels` `Default` gives its type).
    ///
    /// `target` is written as given; an internal target must resolve, from
    /// the source, to an existing part (add the part first, or use
    /// [`Self::add_related_part`]) and must not contain `#`, `?` or `\`.
    ///
    /// Refusals ([`Error::AddRefused`]; nothing is changed): a missing or
    /// registry source, a relationship type that is not an absolute URI, an
    /// empty target or one with whitespace, control or non-XML characters,
    /// an internal target that resolves to no part, or a relationships part
    /// (or `[Content_Types].xml`, when it must change) that was replaced or
    /// does not parse.
    pub fn add_relationship(&mut self, source: Option<&str>, rel_type: &str, target: &str, mode: TargetMode) -> Result<String> {
        let mut plan = AddPlan::default();
        let id = self
            .plan_add_relationship(source, rel_type, target, mode, None, &mut plan)
            .map_err(|r| refused(source.unwrap_or(""), r))?;
        self.finish(plan);
        Ok(id)
    }

    /// The validation of [`Self::add_relationship`], without changing
    /// anything. Returns the `Id` it would allocate.
    pub fn check_add_relationship(&self, source: Option<&str>, rel_type: &str, target: &str, mode: TargetMode) -> Result<String> {
        let mut plan = AddPlan::default();
        self.plan_add_relationship(source, rel_type, target, mode, None, &mut plan)
            .map_err(|r| refused(source.unwrap_or(""), r))
    }

    /// Sets a part's content type: the `ContentType` of every `Override` for
    /// it (OPC name equivalence), or a new `Override` when only a `Default`
    /// covered it. A no-op when the type is already exactly `content_type`.
    /// Other entries, and every `Default`, are untouched (for example
    /// switching a workbook to `…macroEnabled.main+xml`).
    ///
    /// Refusals ([`Error::AddRefused`]): a missing part
    /// (`SourceNotFound`), a registry part, an invalid media type, or a
    /// `[Content_Types].xml` that was replaced or does not parse.
    pub fn set_content_type(&mut self, name: &str, content_type: &str) -> Result<()> {
        let plan = self.plan_set_content_type(name, content_type).map_err(|r| refused(name, r))?;
        self.finish(plan);
        Ok(())
    }

    /// The validation of [`Self::set_content_type`], without changing
    /// anything.
    pub fn check_set_content_type(&self, name: &str, content_type: &str) -> Result<()> {
        self.plan_set_content_type(name, content_type).map(drop).map_err(|r| refused(name, r))
    }

    /// The first free part name `pattern` gives (its single `{}` replaced by
    /// `start`, `start + 1`, …): a valid part name with no equivalent part,
    /// no `Override` left for it, and no segment-prefix conflict. Pure; a
    /// later `add_part` with the result succeeds unless the package changed.
    ///
    /// `next_free_part_name("xl/worksheets/sheet{}.xml", 4)`.
    pub fn next_free_part_name(&self, pattern: &str, start: u32) -> Result<String> {
        if pattern.matches("{}").count() != 1 {
            return Err(refused(pattern, AddRefusal::InvalidPartName { reason: "pattern must contain exactly one {}" }));
        }
        let pattern = pattern.trim_start_matches('/');
        for n in start..=u32::MAX {
            let name = pattern.replace("{}", &n.to_string());
            validate_part_name(&name).map_err(|reason| refused(&name, AddRefusal::InvalidPartName { reason }))?;
            let norm = normalize_part_name(&name);
            let taken = self.parts.iter().any(|p| {
                let np = normalize_part_name(&p.name);
                np == norm || np.starts_with(&format!("{norm}/")) || norm.starts_with(&format!("{np}/"))
            }) || self.content_types.overrides().iter().any(|(p, _)| same_part_name(p, &name));
            if !taken {
                return Ok(name);
            }
        }
        Err(refused(pattern, AddRefusal::Exhausted))
    }

    fn finish(&mut self, plan: AddPlan) -> AddedPart {
        let added = AddedPart {
            name: plan.part.as_ref().map(|(n, _, _)| n.clone()).unwrap_or_default(),
            content_type_override: plan.part.as_ref().is_some_and(|(_, ct, _)| ct.is_some()),
            relationship_id: plan.relationship_id.clone(),
        };
        self.apply_add(plan);
        added
    }
}

fn refused(part: &str, reason: AddRefusal) -> Error {
    Error::AddRefused { part: part.to_owned(), reason }
}

/// `rId{n}`: one more than the largest `rId<digits>` number, skipping Ids in
/// use under ASCII case folding.
fn next_relationship_id(ids: &[String]) -> Option<String> {
    let max = ids
        .iter()
        .filter_map(|id| id.strip_prefix("rId").filter(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit())))
        .filter_map(|d| d.parse::<u64>().ok())
        .max()
        .unwrap_or(0);
    let mut n = max.checked_add(1)?;
    loop {
        let id = format!("rId{n}");
        if !ids.iter().any(|e| e.eq_ignore_ascii_case(&id)) {
            return Some(id);
        }
        n = n.checked_add(1)?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn part_names() {
        for ok in ["xl/worksheets/sheet1.xml", "a", "x/%E7%94%BB.xml", "x/画.xml", "a/b-c_d~e!$&'()*+,;=:@.xml"] {
            assert_eq!(validate_part_name(ok), Ok(()), "{ok}");
        }
        for bad in [
            "", "a/", "a//b", "a/./b", "a/../b", "a./b", "a/b.", "a%2Fb", "a%5cb", "a%41", "a%2", "a b", "a\\b", "a#b",
            "a?b", "a[1]", "a\"b", "a<b",
        ] {
            assert!(validate_part_name(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn media_types() {
        for ok in ["application/xml", "application/vnd.ms-excel.sheet.macroEnabled.main+xml", "text/plain; charset=utf-8"] {
            assert!(valid_media_type(ok), "{ok}");
        }
        for bad in ["", "text", "/xml", "a/b c", "a/b;", "a/b; x", "a/b\"", "a/b/c"] {
            assert!(!valid_media_type(bad), "{bad}");
        }
        assert!(is_xml_media_type("application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"));
        assert!(is_xml_media_type("application/xml"));
        assert!(!is_xml_media_type("application/vnd.ms-office.vbaProject"));
    }

    #[test]
    fn relationship_ids() {
        let ids = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(next_relationship_id(&ids(&[])).as_deref(), Some("rId1"));
        assert_eq!(next_relationship_id(&ids(&["rId1", "rId5"])).as_deref(), Some("rId6"));
        assert_eq!(next_relationship_id(&ids(&["rId2", "rId10", "R1"])).as_deref(), Some("rId11"));
        assert_eq!(next_relationship_id(&ids(&["rId1", "RID2"])).as_deref(), Some("rId3"));
        assert_eq!(next_relationship_id(&ids(&["foo", "rIdx"])).as_deref(), Some("rId1"));
        assert_eq!(next_relationship_id(&ids(&["rId18446744073709551615"])), None);
    }
}
