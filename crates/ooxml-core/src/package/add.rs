//! Layer 3 registry writes (ADR-0001 post-acceptance note "registry
//! writes"): adding parts, `Default` content types and relationships,
//! setting a part's content type, removing a relationship, and removing a
//! set of parts together with their own relationships parts.
//!
//! # Plan, then apply
//!
//! Additions are collected in a [`RegistryPlan`] built against `&Package`.
//! Each step validates completely before it records anything, and a step's
//! `Err` leaves the plan as it was. Later steps see earlier ones: a
//! relationship may target a part added earlier in the same plan, and a
//! second relationship from the same source gets the next `Id`. Nothing
//! changes in the package until [`Package::apply`], which cannot fail
//! except when the package changed since the plan was started
//! ([`RegistryRefusal::StalePlan`]). Building a plan and dropping it is the
//! validate-only twin of any multi-step edit; the one-step conveniences
//! ([`Package::add_part`], …) have `check_*` twins.
//!
//! Removal of parts is a separate call ([`Package::remove_parts`]), because
//! it is planned over the whole set at once (a cascade is one call).
//!
//! # Registries only grow
//!
//! New `Default`, `Override` and `Relationship` elements are appended as the
//! last child of their root, after a copy of the whitespace that precedes
//! the current last entry. No existing entry is moved, renumbered or
//! rewritten. The exceptions, each limited to what the caller names:
//! [`RegistryPlan::set_content_type`] rewrites the `ContentType` of one
//! part's `Override`s; [`RegistryPlan::remove_relationship`] removes one
//! relationship nobody refers to; [`Package::remove_parts`] removes the
//! entries that register the removed parts.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use super::{Package, Part, PartState, RelsSource, RemoveRefusal, is_rels_part, refers_to_rel_id, source_of_rels_part};
use crate::error::{Error, Result};
use crate::name::{CONTENT_TYPES_NS, PACKAGE_RELATIONSHIPS_NS, QName, XName};
use crate::opc::{
    CONTENT_TYPES_PART, Relationship, Relationships, TargetMode, normalize_part_name, rels_part_name, relative_target,
    resolve_target, same_part_name, xml_trim,
};
use crate::xml::{RawAttr, RawItem, RawNode, RawText, XmlDocument};
use zip::CompressionMethod;

/// Content type of relationships parts (ECMA-376 Part 2).
pub const RELATIONSHIPS_CONTENT_TYPE: &str = "application/vnd.openxmlformats-package.relationships+xml";

static NEXT_PACKAGE_ID: AtomicU64 = AtomicU64::new(1);

/// A process-unique id for a [`Package`] value (plans are bound to one).
pub(crate) fn new_package_id() -> u64 {
    NEXT_PACKAGE_ID.fetch_add(1, Ordering::Relaxed)
}

// ---------------------------------------------------------------- public types

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

/// How to name a new part.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PartNameRequest {
    /// This name (zip form; a leading `/` is dropped). Refused if anything
    /// uses it (see [`Package::part_name_in_use`]).
    Exact(String),
    /// A template with one `{}`, e.g. `xl/worksheets/sheet{}.xml`: the
    /// number is **one more than the largest number in use** for the
    /// template, compared under OPC equivalence, among existing parts,
    /// `Override`s, relationship targets (dangling ones included), names
    /// removed in this session and names planned earlier; `1` when none.
    /// Gaps are never filled (see the crate README for why).
    Numbered(String),
}

/// A relationship target.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// An internal part (existing, or added earlier in the same plan),
    /// written relative to the source (`relative_target`).
    Part(String),
    /// A URI outside the package (`TargetMode="External"`), written as given.
    External(String),
}

/// Why a registry write was refused (nothing was changed).
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum RegistryRefusal {
    /// Not a valid OPC part name (ECMA-376 Part 2 §6.2.2), or a template
    /// without exactly one `{}`.
    InvalidPartName {
        /// Which rule.
        reason: &'static str,
    },
    /// `[Content_Types].xml` or a relationships part: these are written only
    /// through the registry operations.
    RegistryPart,
    /// The name is in use: by a part (OPC equivalence), a segment-prefix
    /// conflict (`a/b` vs `a/b/c`), an `Override`, a relationship target
    /// or a part added earlier in the plan.
    NameInUse {
        /// What uses it.
        by: String,
    },
    /// Not a media type (`type/subtype` with token characters, optional
    /// parameters), or characters XML cannot carry.
    InvalidContentType,
    /// Not a valid extension for a `Default` (empty, or `.` `/` or a
    /// character a part name cannot hold).
    InvalidExtension,
    /// A `Default` for the extension exists with another content type.
    DefaultExists {
        /// Its content type.
        existing: String,
    },
    /// Adding the `Default` would change the content type of this existing
    /// part, which has no `Override`.
    WouldRetypePart {
        /// The part.
        part: String,
    },
    /// An `Override` for the name exists with another content type (left
    /// over from a part that is gone).
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
        /// Why (replaced wholesale, not well-formed, wrong root, missing).
        reason: String,
    },
    /// The part (or the relationship's source part) does not exist.
    PartNotFound,
    /// An internal target resolves to no part.
    TargetNotFound {
        /// The resolved part name.
        resolved: String,
    },
    /// The target is empty, or has whitespace, control or non-XML
    /// characters (or `#`, `?`, `\` for an internal one).
    InvalidTarget,
    /// The relationship type is not an absolute URI.
    InvalidRelationshipType,
    /// No relationship with this `Id` in the source's relationships part.
    RelationshipNotFound,
    /// The source part still refers to the relationship `Id` (any
    /// attribute, any namespace, after XML unescaping).
    StillReferenced {
        /// The source part.
        source: String,
    },
    /// The source part could not be scanned for references.
    SourceNotScannable {
        /// Why.
        reason: String,
    },
    /// The plan already changes this part's content type.
    AlreadyPlanned,
    /// The plan was made for another package, or the package changed
    /// since the plan started.
    StalePlan,
    /// The step contradicts an earlier step of the plan (e.g. removing a
    /// part the plan adds a relationship to, or changes the type of).
    ConflictsWithPlan,
    /// A removal refused for one of PR #14's reasons.
    Remove(RemoveRefusal),
    /// No relationship `Id` or part number is left.
    Exhausted,
}

impl std::fmt::Display for RegistryRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use RegistryRefusal as R;
        match self {
            R::InvalidPartName { reason } => write!(f, "invalid part name: {reason}"),
            R::RegistryPart => write!(f, "registry parts are written only by the registry operations"),
            R::NameInUse { by } => write!(f, "the name is in use by {by}"),
            R::InvalidContentType => write!(f, "not a media type"),
            R::InvalidExtension => write!(f, "not a valid extension"),
            R::DefaultExists { existing } => write!(f, "a Default with content type {existing:?} exists"),
            R::WouldRetypePart { part } => write!(f, "the Default would change the content type of {part:?}"),
            R::ContentTypeConflict { existing } => write!(f, "an Override with content type {existing:?} exists"),
            R::MalformedXml { reason } => write!(f, "content is not well-formed XML: {reason}"),
            R::RegistryNotEditable { part, reason } => write!(f, "{part} cannot be edited in place: {reason}"),
            R::PartNotFound => write!(f, "no such part"),
            R::TargetNotFound { resolved } => write!(f, "the target resolves to {resolved:?}, which is not a part"),
            R::InvalidTarget => write!(f, "invalid relationship target"),
            R::InvalidRelationshipType => write!(f, "the relationship type is not an absolute URI"),
            R::RelationshipNotFound => write!(f, "no such relationship"),
            R::StillReferenced { source } => write!(f, "{source} still refers to the relationship"),
            R::SourceNotScannable { reason } => write!(f, "cannot scan the source part: {reason}"),
            R::AlreadyPlanned => write!(f, "the plan already sets this part's content type"),
            R::StalePlan => write!(f, "the package changed since the plan was made"),
            R::ConflictsWithPlan => write!(f, "contradicts an earlier step of the plan"),
            R::Remove(r) => write!(f, "{r}"),
            R::Exhausted => write!(f, "no free Id or part number"),
        }
    }
}

/// One relationship pointing at a part ([`Package::incoming_relationships`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IncomingRelationship {
    /// The source part (`None` = the package).
    pub source: Option<String>,
    /// The relationships part.
    pub rels: String,
    /// `Id`.
    pub id: String,
    /// `Type`.
    pub rel_type: String,
}

/// What [`Package::remove_parts`] removed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RemovedParts {
    /// The parts, plus the relationships parts of theirs that went with
    /// them.
    pub parts: Vec<String>,
    /// `(relationships part, Id)` of each relationship removed from a
    /// relationships part that stays.
    pub relationships: Vec<(String, String)>,
    /// Number of `Override`s removed.
    pub overrides: usize,
    /// Internal targets the removed parts' own relationships pointed at that
    /// were **not** removed: `(removed source, target part, relationship
    /// type)`. The caller decides whether they are orphans now.
    pub kept_targets: Vec<(String, String, String)>,
}

// ---------------------------------------------------------------- validation helpers

/// Validates an OPC part name (zip form, no leading `/`): ECMA-376 Part 2
/// §6.2.2 — non-empty segments, no `.`/`..` segment, no segment ending in
/// `.`, no `%`-encoded `/`, `\` or unreserved character, pchar or non-ASCII
/// only, not ending in `/`.
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
        if !(c >= 0x80 || is_pchar(c) || c == b'/') {
            return Err("character not allowed in a part name");
        }
        i += 1;
    }
    Ok(())
}

fn is_pchar(c: u8) -> bool {
    c.is_ascii_alphanumeric()
        || matches!(c, b'-' | b'.' | b'_' | b'~' | b'!' | b'$' | b'&' | b'\'' | b'(' | b')' | b'*' | b'+' | b',' | b';' | b'=' | b':' | b'@')
}

/// `type/subtype` (RFC 7231 tokens) with optional `; name=value`
/// parameters, and only characters XML can carry.
fn valid_media_type(ct: &str) -> bool {
    let token = |s: &str| !s.is_empty() && s.bytes().all(|c| c.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&c));
    let mut parts = ct.split(';');
    let Some((t, sub)) = parts.next().and_then(|m| m.split_once('/')) else { return false };
    token(t)
        && token(sub)
        && parts.all(|p| {
            let p = p.trim_matches(' ');
            p.split_once('=').is_some_and(|(k, v)| token(k) && !v.is_empty() && !v.contains(['\r', '\n']))
        })
        && valid_attr_text(ct)
}

fn is_xml_media_type(ct: &str) -> bool {
    let essence = ct.split(';').next().unwrap_or_default().trim().to_ascii_lowercase();
    essence.ends_with("/xml") || essence.ends_with("+xml")
}

fn valid_attr_text(s: &str) -> bool {
    crate::value::first_invalid_xml_char(s).is_none()
}

fn uri_like(s: &str) -> bool {
    !s.is_empty() && valid_attr_text(s) && !s.chars().any(|c| c.is_whitespace() || c.is_control())
}

fn extension_of(name: &str) -> Option<&str> {
    let file = name.rsplit('/').next()?;
    file.rsplit_once('.').map(|(_, e)| e).filter(|e| !e.is_empty())
}

fn is_registry_name(name: &str) -> bool {
    name.trim_start_matches('/').eq_ignore_ascii_case(CONTENT_TYPES_PART) || is_rels_part(&normalize_part_name(name))
}

/// `rId{n}`: one more than the largest `rId<digits>` number (the prefix
/// matched ASCII case-insensitively), skipping Ids in use under ASCII case
/// folding. Other Ids (`R44c8…`) are left alone and cannot collide.
fn next_relationship_id<'a>(ids: impl Iterator<Item = &'a str> + Clone) -> Option<String> {
    let max = ids
        .clone()
        .filter_map(|id| {
            let (p, d) = id.split_at_checked(3)?;
            (p.eq_ignore_ascii_case("rId") && !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit())).then_some(d)
        })
        .filter_map(|d| d.parse::<u64>().ok())
        .max()
        .unwrap_or(0);
    let mut n = max.checked_add(1)?;
    loop {
        let id = format!("rId{n}");
        if !ids.clone().any(|e| e.eq_ignore_ascii_case(&id)) {
            return Some(id);
        }
        n = n.checked_add(1)?;
    }
}

/// An element in `root`'s namespace and prefix, with unqualified attributes.
fn element(root: &RawNode, local: &str, attrs: &[(&str, &str)]) -> RawNode {
    let mut n = RawNode::new_created(XName::new(Arc::from(root.name().ns()), local, root.name().prefix()));
    n.attrs = attrs.iter().map(|(k, v)| RawAttr { name: XName::new(Arc::from(""), k, None), value: (*v).to_owned() }).collect();
    n
}

/// Appends `node` as the root's last child, after a copy of the whitespace
/// text that precedes the current last element (if any). Cannot fail.
fn append_root_child(doc: &mut XmlDocument, node: RawNode) {
    let root = doc.root_mut();
    let last_el = root.children.iter().rposition(|c| matches!(c, RawItem::Element(_)));
    let ws = last_el.and_then(|i| i.checked_sub(1)).and_then(|i| match &root.children[i] {
        RawItem::Text(t) if t.is_whitespace() && !t.cdata => Some(t.value.clone()),
        _ => None,
    });
    let at = last_el.map_or(root.children.len(), |i| i + 1);
    let mut items = Vec::with_capacity(2);
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

fn types_q() -> QName {
    QName::new(CONTENT_TYPES_NS, "Types")
}
fn rels_root_q() -> QName {
    QName::new(PACKAGE_RELATIONSHIPS_NS, "Relationships")
}
fn rel_q() -> QName {
    QName::new(PACKAGE_RELATIONSHIPS_NS, "Relationship")
}
fn attr<'n>(e: &'n RawNode, a: &'static str) -> Option<&'n str> {
    e.attr(QName::new("", a)).map(xml_trim)
}

// ---------------------------------------------------------------- the plan

/// Pending edits of one existing registry part.
#[derive(Clone, Debug)]
struct RegEdit {
    part: usize,
    /// The tree, parsed during planning when the part was `Raw` (installed
    /// on apply only if the edit changes something).
    install: Option<XmlDocument>,
    /// Root child indices to remove (with their leading whitespace).
    removes: Vec<usize>,
    appends: Vec<RawNode>,
    /// `(root child index, content type)` for `Override`s.
    set_ct: Vec<(usize, String)>,
}

#[derive(Clone, Debug)]
struct NewPart {
    name: String,
    content: NewContent,
}

#[derive(Clone, Debug)]
enum NewContent {
    Given(PartContent),
    /// A new relationships part for `source`, with these relationships.
    Rels(Vec<RawNode>),
}

/// A set of registry additions, validated step by step against a
/// [`Package`] and applied all at once by [`Package::apply`]. See the
/// module docs.
#[derive(Clone, Debug)]
pub struct RegistryPlan {
    package: u64,
    generation: u64,
    edits: Vec<RegEdit>,
    new_parts: Vec<NewPart>,
    defaults: Vec<(String, String)>,
    overrides: Vec<(String, String)>,
    set_overrides: Vec<(String, String)>,
    /// `(rels part, Id)` added, for Id allocation within the plan.
    new_ids: Vec<(String, String)>,
    /// `(rels part, Id)` removed.
    removed_ids: Vec<(String, String)>,
    package_rels_added: Vec<Relationship>,
    /// Parts removed by the plan (indices into `parts`), and their names.
    gone: Vec<usize>,
    gone_names: Vec<String>,
    /// New content for existing parts (`stage_part`), by part index.
    staged: Vec<(usize, PartContent)>,
}

impl RegistryPlan {
    /// An empty plan for `pkg`.
    pub fn new(pkg: &Package) -> Self {
        RegistryPlan {
            package: pkg.id,
            generation: pkg.generation,
            edits: Vec::new(),
            new_parts: Vec::new(),
            defaults: Vec::new(),
            overrides: Vec::new(),
            set_overrides: Vec::new(),
            new_ids: Vec::new(),
            removed_ids: Vec::new(),
            package_rels_added: Vec::new(),
            gone: Vec::new(),
            gone_names: Vec::new(),
            staged: Vec::new(),
        }
    }

    /// Does the plan change anything?
    pub fn is_empty(&self) -> bool {
        self.new_parts.is_empty()
            && self.gone.is_empty()
            && self.staged.is_empty()
            && self.edits.iter().all(|e| e.removes.is_empty() && e.appends.is_empty() && e.set_ct.is_empty())
    }

    fn check(&self, pkg: &Package) -> std::result::Result<(), RegistryRefusal> {
        if self.package != pkg.id || self.generation != pkg.generation {
            return Err(RegistryRefusal::StalePlan);
        }
        Ok(())
    }

    /// Runs `f` on a copy of the plan and keeps the copy only on success,
    /// so a refused step leaves the plan as it was.
    fn step<T>(&mut self, pkg: &Package, part: &str, f: impl FnOnce(&mut Self) -> std::result::Result<T, RegistryRefusal>) -> Result<T> {
        self.check(pkg).map_err(|r| refused(part, r))?;
        let mut copy = self.clone();
        let out = f(&mut copy).map_err(|r| refused(part, r))?;
        *self = copy;
        Ok(out)
    }

    /// The content a part has in the plan's view: staged content, else the
    /// package's current content.
    fn view_bytes(&self, pkg: &Package, name: &str) -> std::result::Result<Vec<u8>, String> {
        if let Some(i) = pkg.find_ci(name)
            && let Some((_, c)) = self.staged.iter().find(|(j, _)| *j == i)
        {
            return Ok(match c {
                PartContent::Bytes(b) => b.clone(),
                PartContent::Xml(d) => d.serialize().into_bytes(),
            });
        }
        if let Some(NewPart { content: NewContent::Given(c), .. }) = self.new_parts.iter().find(|p| same_part_name(&p.name, name)) {
            return Ok(match c {
                PartContent::Bytes(b) => b.clone(),
                PartContent::Xml(d) => d.serialize().into_bytes(),
            });
        }
        pkg.read_part(name).map_err(|e| e.to_string())
    }

    fn pending_part(&self, name: &str) -> Option<&str> {
        self.new_parts.iter().find(|p| same_part_name(&p.name, name)).map(|p| p.name.as_str())
    }

    /// The edit entry for registry part `i`, parsing a `Raw` part into it.
    fn edit_for(&mut self, pkg: &Package, i: usize, root: QName) -> std::result::Result<usize, RegistryRefusal> {
        if let Some(pos) = self.edits.iter().position(|e| e.part == i) {
            return Ok(pos);
        }
        let not_editable = |reason: String| RegistryRefusal::RegistryNotEditable { part: pkg.parts[i].name.clone(), reason };
        let install = match &pkg.parts[i].state {
            PartState::Parsed(_) => None,
            PartState::Raw => Some(pkg.parse_raw(i).map_err(not_editable)?),
            PartState::Replaced(_) => return Err(not_editable("replaced wholesale; cannot be edited in place".into())),
        };
        let tree = match (&install, &pkg.parts[i].state) {
            (Some(d), _) | (None, PartState::Parsed(d)) => d,
            _ => unreachable!(),
        };
        if !tree.root().is(root) {
            return Err(not_editable(format!("root is not <{}>", root.local)));
        }
        self.edits.push(RegEdit { part: i, install, removes: Vec::new(), appends: Vec::new(), set_ct: Vec::new() });
        Ok(self.edits.len() - 1)
    }

    fn tree<'a>(&'a self, pkg: &'a Package, pos: usize) -> &'a XmlDocument {
        let e = &self.edits[pos];
        match (&e.install, &pkg.parts[e.part].state) {
            (Some(d), _) | (None, PartState::Parsed(d)) => d,
            _ => unreachable!("edit_for checked the state"),
        }
    }

    fn content_types_edit(&mut self, pkg: &Package) -> std::result::Result<usize, RegistryRefusal> {
        let ct = pkg.find(CONTENT_TYPES_PART).ok_or_else(|| RegistryRefusal::RegistryNotEditable {
            part: CONTENT_TYPES_PART.into(),
            reason: "missing".into(),
        })?;
        self.edit_for(pkg, ct, types_q())
    }

    /// The content type `name` would have after the plan, counting planned
    /// `Override`s and `Default`s (`None` for a part the plan removes).
    pub fn content_type<'a>(&'a self, pkg: &'a Package, name: &str) -> Option<&'a str> {
        if self.is_gone(name) {
            return None;
        }
        if let Some((_, t)) = self.overrides.iter().chain(&self.set_overrides).rev().find(|(p, _)| same_part_name(p, name)) {
            return Some(t);
        }
        if let Some((_, t)) = pkg.content_types.overrides().iter().find(|(p, _)| same_part_name(p, name)) {
            return Some(t);
        }
        let ext = extension_of(name)?;
        pkg.content_types
            .defaults()
            .iter()
            .chain(&self.defaults)
            .find(|(e, _)| e.eq_ignore_ascii_case(ext))
            .map(|(_, t)| t.as_str())
    }

    fn default_for<'a>(&'a self, pkg: &'a Package, ext: &str) -> Option<&'a str> {
        pkg.content_types.defaults().iter().chain(&self.defaults).find(|(e, _)| e.eq_ignore_ascii_case(ext)).map(|(_, t)| t.as_str())
    }

    /// Registers the content type of a new name: nothing when a `Default`
    /// (existing or planned) or a left-over `Override` already gives it,
    /// else a new `Override`.
    fn register_content_type(&mut self, pkg: &Package, name: &str, content_type: &str) -> std::result::Result<(), RegistryRefusal> {
        let stale: Vec<&str> =
            pkg.content_types.overrides().iter().filter(|(p, _)| same_part_name(p, name)).map(|(_, t)| t.as_str()).collect();
        if let Some(t) = stale.iter().find(|t| !t.eq_ignore_ascii_case(content_type)) {
            return Err(RegistryRefusal::ContentTypeConflict { existing: (*t).to_owned() });
        }
        if !stale.is_empty() {
            return Ok(());
        }
        if extension_of(name).and_then(|e| self.default_for(pkg, e)).is_some_and(|t| t.eq_ignore_ascii_case(content_type)) {
            return Ok(());
        }
        let pos = self.content_types_edit(pkg)?;
        let node = element(self.tree(pkg, pos).root(), "Override", &[("PartName", &format!("/{name}")), ("ContentType", content_type)]);
        self.edits[pos].appends.push(node);
        self.overrides.push((name.to_owned(), content_type.to_owned()));
        Ok(())
    }

    // ------------------------------------------------------------ steps

    /// Adds `Default Extension=ext ContentType=content_type`. Returns
    /// `false` (and plans nothing) if that exact `Default` exists.
    ///
    /// Refused if a `Default` for `ext` (ASCII case-insensitive) has
    /// another type (`DefaultExists`), or if an existing or planned part
    /// with that extension has no `Override` (`WouldRetypePart`): adding the
    /// `Default` would change its type.
    pub fn add_default_content_type(&mut self, pkg: &Package, ext: &str, content_type: &str) -> Result<bool> {
        self.step(pkg, CONTENT_TYPES_PART, |plan| {
            if ext.is_empty() || ext.bytes().any(|c| c == b'.' || c == b'/' || c == b'%' || !(c >= 0x80 || is_pchar(c))) {
                return Err(RegistryRefusal::InvalidExtension);
            }
            if !valid_media_type(content_type) {
                return Err(RegistryRefusal::InvalidContentType);
            }
            if let Some(t) = plan.default_for(pkg, ext) {
                return if t.eq_ignore_ascii_case(content_type) {
                    Ok(false)
                } else {
                    Err(RegistryRefusal::DefaultExists { existing: t.to_owned() })
                };
            }
            let names = pkg.parts.iter().map(|p| p.name.as_str()).chain(plan.new_parts.iter().map(|p| p.name.as_str()));
            for n in names {
                let has_override = pkg.content_types.overrides().iter().chain(&plan.overrides).any(|(p, _)| same_part_name(p, n));
                if extension_of(n).is_some_and(|e| e.eq_ignore_ascii_case(ext)) && !has_override && !is_registry_name(n) {
                    return Err(RegistryRefusal::WouldRetypePart { part: n.to_owned() });
                }
            }
            let pos = plan.content_types_edit(pkg)?;
            let node = element(plan.tree(pkg, pos).root(), "Default", &[("Extension", ext), ("ContentType", content_type)]);
            plan.edits[pos].appends.push(node);
            plan.defaults.push((ext.to_owned(), content_type.to_owned()));
            Ok(true)
        })
    }

    /// Adds a part and registers its content type: nothing when a `Default`
    /// for its extension (existing or planned) gives exactly `content_type`
    /// (ASCII case-insensitive), or an `Override` for the name is left with
    /// that type; else an `Override`. Returns the name. The part is written
    /// fresh on save, after the original entries.
    ///
    /// Refused: an invalid name, template or media type, a registry part,
    /// a name in use ([`Package::part_name_in_use`]), a left-over `Override`
    /// with another type, XML bytes that are not well-formed for an XML
    /// content type, a `[Content_Types].xml` that must change but was
    /// replaced or does not parse.
    pub fn add_part(&mut self, pkg: &Package, name: PartNameRequest, content_type: &str, content: impl Into<PartContent>) -> Result<String> {
        let content = content.into();
        let label = match &name {
            PartNameRequest::Exact(n) | PartNameRequest::Numbered(n) => n.clone(),
        };
        self.step(pkg, &label, |plan| {
            if !valid_media_type(content_type) {
                return Err(RegistryRefusal::InvalidContentType);
            }
            let name = match name {
                PartNameRequest::Exact(n) => {
                    let n = n.trim_start_matches('/').to_owned();
                    plan.check_free(pkg, &n)?;
                    n
                }
                PartNameRequest::Numbered(t) => plan.numbered(pkg, &t)?,
            };
            if let PartContent::Bytes(b) = &content
                && is_xml_media_type(content_type)
            {
                XmlDocument::parse(b).map_err(|e| RegistryRefusal::MalformedXml { reason: e.to_string() })?;
            }
            plan.register_content_type(pkg, &name, content_type)?;
            plan.new_parts.push(NewPart { name: name.clone(), content: NewContent::Given(content) });
            Ok(name)
        })
    }

    /// Validity and freeness of a name for a new part.
    fn check_free(&self, pkg: &Package, name: &str) -> std::result::Result<(), RegistryRefusal> {
        if is_registry_name(name) {
            return Err(RegistryRefusal::RegistryPart);
        }
        validate_part_name(name).map_err(|reason| RegistryRefusal::InvalidPartName { reason })?;
        if self.is_gone(name) {
            return Err(RegistryRefusal::NameInUse { by: "a part removed in this plan".into() });
        }
        if let Some(by) = pkg.name_user(name, self.new_parts.iter().map(|p| p.name.as_str())) {
            return Err(RegistryRefusal::NameInUse { by });
        }
        // A relationships part left for the name would become the new
        // part's relationships (review of ADR-0009, M6).
        if let Some(i) = pkg.find_ci(&rels_part_name(Some(name))) {
            return Err(RegistryRefusal::NameInUse { by: format!("its left-over relationships part {:?}", pkg.parts[i].name) });
        }
        Ok(())
    }

    fn numbered(&self, pkg: &Package, template: &str) -> std::result::Result<String, RegistryRefusal> {
        let template = template.trim_start_matches('/');
        let Some((pre, post)) = template.split_once("{}").filter(|(_, post)| !post.contains("{}")) else {
            return Err(RegistryRefusal::InvalidPartName { reason: "template must contain exactly one {}" });
        };
        let (npre, npost) = (normalize_part_name(pre), normalize_part_name(post));
        let number = |name: &str| -> Option<u64> {
            let n = normalize_part_name(name);
            let mid = n.strip_prefix(&npre)?.strip_suffix(&npost)?;
            (!mid.is_empty() && mid.bytes().all(|b| b.is_ascii_digit())).then(|| mid.parse().ok()).flatten()
        };
        let max = pkg
            .names_in_use()
            .iter()
            .map(String::as_str)
            .chain(self.new_parts.iter().map(|p| p.name.as_str()))
            .chain(pkg.removed_names.iter().map(String::as_str))
            .chain(self.gone_names.iter().map(String::as_str))
            .filter_map(number)
            .max()
            .unwrap_or(0);
        let mut n = max.checked_add(1).ok_or(RegistryRefusal::Exhausted)?;
        loop {
            let name = format!("{pre}{n}{post}");
            match self.check_free(pkg, &name) {
                Ok(()) => return Ok(name),
                Err(RegistryRefusal::NameInUse { .. }) => n = n.checked_add(1).ok_or(RegistryRefusal::Exhausted)?,
                Err(e) => return Err(e),
            }
        }
    }

    /// Adds a relationship from `source` (`None` = the package) and returns
    /// its `Id`: `rId{n}`, `n` one more than the largest `rId<digits>` in
    /// the source's relationships part (and earlier in the plan), skipping
    /// Ids used under ASCII case folding; other Ids are left alone. A
    /// missing relationships part is created and registered like a new
    /// part. The source may be a part added earlier in the plan.
    ///
    /// Refused: a missing or registry source, a relationship type that is
    /// not an absolute URI, an invalid target or one that resolves to no
    /// part, a relationships part (or `[Content_Types].xml`) that must
    /// change but was replaced or does not parse.
    pub fn add_relationship(&mut self, pkg: &Package, source: Option<&str>, rel_type: &str, target: Target) -> Result<String> {
        self.step(pkg, source.unwrap_or(""), |plan| plan.relationship(pkg, source, rel_type, target))
    }

    fn resolve_source(&self, pkg: &Package, source: Option<&str>) -> std::result::Result<Option<String>, RegistryRefusal> {
        let Some(s) = source else { return Ok(None) };
        let name = match pkg.find_ci(s) {
            Some(i) if !self.gone.contains(&i) => pkg.parts[i].name.clone(),
            Some(_) => return Err(RegistryRefusal::ConflictsWithPlan),
            None => self.pending_part(s).ok_or(RegistryRefusal::PartNotFound)?.to_owned(),
        };
        if is_registry_name(&name) {
            return Err(RegistryRefusal::RegistryPart);
        }
        Ok(Some(name))
    }

    fn relationship(&mut self, pkg: &Package, source: Option<&str>, rel_type: &str, target: Target) -> std::result::Result<String, RegistryRefusal> {
        let source = self.resolve_source(pkg, source)?;
        if !uri_like(rel_type) || !rel_type.contains(':') {
            return Err(RegistryRefusal::InvalidRelationshipType);
        }
        let (target, mode) = match target {
            Target::Part(p) => {
                let p = p.trim_start_matches('/');
                let canonical = match pkg.find_ci(p) {
                    Some(i) if !self.gone.contains(&i) => pkg.parts[i].name.clone(),
                    Some(_) => return Err(RegistryRefusal::ConflictsWithPlan),
                    None => self.pending_part(p).ok_or_else(|| RegistryRefusal::TargetNotFound { resolved: p.to_owned() })?.to_owned(),
                };
                if is_registry_name(&canonical) {
                    return Err(RegistryRefusal::RegistryPart);
                }
                let t = relative_target(source.as_deref(), &canonical);
                if !uri_like(&t) || t.contains(['#', '?', '\\']) || !same_part_name(&resolve_target(source.as_deref(), &t), &canonical) {
                    return Err(RegistryRefusal::InvalidTarget);
                }
                (t, TargetMode::Internal)
            }
            Target::External(u) => {
                if !uri_like(&u) {
                    return Err(RegistryRefusal::InvalidTarget);
                }
                (u, TargetMode::External)
            }
        };
        let rels_name = rels_part_name(source.as_deref());
        let mut attrs: Vec<(&str, &str)> = Vec::new();
        let planned_ids: Vec<String> =
            self.new_ids.iter().filter(|(r, _)| same_part_name(r, &rels_name)).map(|(_, id)| id.clone()).collect();
        let id;
        match pkg.find_ci(&rels_name) {
            Some(ri) => {
                let pos = self.edit_for(pkg, ri, rels_root_q())?;
                let tree = self.tree(pkg, pos);
                let existing: Vec<&str> = tree.root().elements().filter(|e| e.is(rel_q())).filter_map(|e| attr(e, "Id")).collect();
                id = next_relationship_id(existing.iter().copied().chain(planned_ids.iter().map(String::as_str)))
                    .ok_or(RegistryRefusal::Exhausted)?;
                attrs.extend([("Id", id.as_str()), ("Type", rel_type), ("Target", target.as_str())]);
                if mode == TargetMode::External {
                    attrs.push(("TargetMode", "External"));
                }
                let node = element(tree.root(), "Relationship", &attrs);
                self.edits[pos].appends.push(node);
            }
            None => {
                id = next_relationship_id(planned_ids.iter().map(String::as_str)).ok_or(RegistryRefusal::Exhausted)?;
                attrs.extend([("Id", id.as_str()), ("Type", rel_type), ("Target", target.as_str())]);
                if mode == TargetMode::External {
                    attrs.push(("TargetMode", "External"));
                }
                let proto = XmlDocument::parse(format!("<Relationships xmlns=\"{PACKAGE_RELATIONSHIPS_NS}\"/>").as_bytes())
                    .map_err(|e| RegistryRefusal::MalformedXml { reason: e.to_string() })?;
                let node = element(proto.root(), "Relationship", &attrs);
                match self.new_parts.iter_mut().find(|p| same_part_name(&p.name, &rels_name)) {
                    Some(NewPart { content: NewContent::Rels(v), .. }) => v.push(node),
                    Some(_) => return Err(RegistryRefusal::RegistryPart),
                    None => {
                        self.check_free(pkg, &rels_name).or_else(|e| match e {
                            // a relationships part is a registry part, but this is how it is created
                            RegistryRefusal::RegistryPart => Ok(()),
                            e => Err(e),
                        })?;
                        if let Some(by) = pkg.name_user(&rels_name, self.new_parts.iter().map(|p| p.name.as_str())) {
                            return Err(RegistryRefusal::NameInUse { by });
                        }
                        self.register_content_type(pkg, &rels_name, RELATIONSHIPS_CONTENT_TYPE)?;
                        self.new_parts.push(NewPart { name: rels_name.clone(), content: NewContent::Rels(vec![node]) });
                    }
                }
            }
        }
        self.new_ids.push((rels_name, id.clone()));
        if source.is_none() {
            self.package_rels_added.push(Relationship { id: id.clone(), rel_type: rel_type.to_owned(), target, target_mode: mode });
        }
        Ok(id)
    }

    /// Removes the relationship `id` from `source`'s relationships part
    /// (`None` = the package), with the whitespace before it. Refused when
    /// it does not exist (or was added in this plan), or when the source
    /// part still refers to `id` in any attribute (`StillReferenced`), or
    /// cannot be scanned. The target part is not touched.
    pub fn remove_relationship(&mut self, pkg: &Package, source: Option<&str>, id: &str) -> Result<()> {
        self.step(pkg, source.unwrap_or(""), |plan| {
            let source = match source {
                None => None,
                Some(s) => {
                    let i = pkg.find_ci(s).ok_or(RegistryRefusal::PartNotFound)?;
                    if plan.gone.contains(&i) {
                        return Err(RegistryRefusal::ConflictsWithPlan);
                    }
                    Some(pkg.parts[i].name.clone())
                }
            };
            if source.as_deref().is_some_and(is_registry_name) {
                return Err(RegistryRefusal::RegistryPart);
            }
            let rels_name = rels_part_name(source.as_deref());
            let ri = pkg.find_ci(&rels_name).ok_or(RegistryRefusal::RelationshipNotFound)?;
            let rels_name = pkg.parts[ri].name.clone();
            if plan.removed_ids.iter().any(|(r, i)| same_part_name(r, &rels_name) && i == xml_trim(id)) {
                return Err(RegistryRefusal::RelationshipNotFound);
            }
            let pos = plan.edit_for(pkg, ri, rels_root_q())?;
            let index = plan
                .tree(pkg, pos)
                .root()
                .indexed_elements()
                .find(|(_, e)| e.is(rel_q()) && attr(e, "Id") == Some(xml_trim(id)))
                .map(|(i, _)| i)
                .ok_or(RegistryRefusal::RelationshipNotFound)?;
            if let Some(src) = &source {
                let bytes = plan.view_bytes(pkg, src).map_err(|reason| RegistryRefusal::SourceNotScannable { reason })?;
                match refers_to_rel_id(&bytes, id) {
                    Ok(false) => {}
                    Ok(true) => return Err(RegistryRefusal::StillReferenced { source: src.clone() }),
                    Err(reason) => return Err(RegistryRefusal::SourceNotScannable { reason }),
                }
            }
            plan.edits[pos].removes.push(index);
            plan.removed_ids.push((rels_name, xml_trim(id).to_owned()));
            Ok(())
        })
    }

    /// Sets an existing part's content type: the `ContentType` of every
    /// `Override` for it, or a new `Override` when only a `Default` covered
    /// it. A no-op when the type is already exactly `content_type`. Every
    /// other entry, and every `Default`, is untouched.
    pub fn set_content_type(&mut self, pkg: &Package, name: &str, content_type: &str) -> Result<()> {
        self.step(pkg, name, |plan| {
            let i = pkg.find_ci(name).ok_or(RegistryRefusal::PartNotFound)?;
            if plan.gone.contains(&i) {
                return Err(RegistryRefusal::ConflictsWithPlan);
            }
            let name = pkg.parts[i].name.clone();
            if is_registry_name(&name) {
                return Err(RegistryRefusal::RegistryPart);
            }
            if !valid_media_type(content_type) {
                return Err(RegistryRefusal::InvalidContentType);
            }
            if plan.overrides.iter().chain(&plan.set_overrides).any(|(p, _)| same_part_name(p, &name)) {
                return Err(RegistryRefusal::AlreadyPlanned);
            }
            if pkg.content_type(&name) == Some(content_type) {
                return Ok(());
            }
            let pos = plan.content_types_edit(pkg)?;
            let indices: Vec<usize> = plan
                .tree(pkg, pos)
                .root()
                .indexed_elements()
                .filter(|(_, e)| e.is(QName::new(CONTENT_TYPES_NS, "Override")) && attr(e, "PartName").is_some_and(|p| same_part_name(p, &name)))
                .map(|(i, _)| i)
                .collect();
            if indices.is_empty() {
                let node = element(plan.tree(pkg, pos).root(), "Override", &[("PartName", &format!("/{name}")), ("ContentType", content_type)]);
                plan.edits[pos].appends.push(node);
                plan.overrides.push((name, content_type.to_owned()));
            } else {
                plan.edits[pos].set_ct.extend(indices.into_iter().map(|i| (i, content_type.to_owned())));
                plan.set_overrides.push((name, content_type.to_owned()));
            }
            Ok(())
        })
    }
}

fn refused(part: &str, reason: RegistryRefusal) -> Error {
    Error::RegistryRefused { part: part.to_owned(), reason }
}

// ---------------------------------------------------------------- Package API

impl Package {
    /// Applies a validated [`RegistryPlan`]. Refused only with
    /// [`RegistryRefusal::StalePlan`] (another package, or this one changed
    /// since the plan started); otherwise nothing here can fail.
    pub fn apply(&mut self, plan: RegistryPlan) -> Result<()> {
        plan.check(self).map_err(|r| refused("", r))?;
        if plan.is_empty() {
            return Ok(());
        }
        for edit in plan.edits {
            if edit.removes.is_empty() && edit.appends.is_empty() && edit.set_ct.is_empty() {
                continue; // parsed for planning only: leave the part as it was
            }
            let part = &mut self.parts[edit.part];
            if let Some(doc) = edit.install {
                part.state = PartState::Parsed(doc);
            }
            let PartState::Parsed(doc) = &mut part.state else {
                debug_assert!(false, "planned as parsed");
                continue;
            };
            {
                let root = doc.root_mut();
                for (i, ct) in &edit.set_ct {
                    if let Some(RawItem::Element(e)) = root.children.get_mut(*i)
                        && let Some(a) = e.attrs.iter_mut().find(|a| a.name.is(QName::new("", "ContentType")))
                    {
                        a.value.clone_from(ct);
                        e.own_dirty = true;
                        e.subtree_dirty = true;
                        root.subtree_dirty = true;
                    }
                }
            }
            let mut removes = edit.removes;
            removes.sort_unstable();
            removes.dedup();
            for &at in removes.iter().rev() {
                doc.remove_root_child_with_leading_ws(at);
            }
            for node in edit.appends {
                append_root_child(doc, node);
            }
        }
        for (i, c) in plan.staged {
            self.parts[i].state = match c {
                PartContent::Bytes(b) => PartState::Replaced(b),
                PartContent::Xml(d) => PartState::Parsed(d),
            };
        }
        let mut gone = plan.gone;
        gone.sort_unstable();
        for &i in gone.iter().rev() {
            self.parts.remove(i);
        }
        for n in &plan.gone_names {
            self.content_types.remove_override(n);
        }
        self.removed_names.extend(plan.gone_names);
        for p in plan.new_parts {
            let state = match p.content {
                NewContent::Given(PartContent::Bytes(b)) => PartState::Replaced(b),
                NewContent::Given(PartContent::Xml(d)) => PartState::Parsed(d),
                NewContent::Rels(nodes) => {
                    let mut doc = XmlDocument::parse(
                        format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<Relationships xmlns=\"{PACKAGE_RELATIONSHIPS_NS}\"/>")
                            .as_bytes(),
                    )
                    .expect("constant relationships skeleton");
                    for n in nodes {
                        append_root_child(&mut doc, n);
                    }
                    PartState::Parsed(doc)
                }
            };
            self.parts.push(Part { name: p.name, index: 0, added: true, compression: CompressionMethod::Deflated, state });
        }
        for (e, t) in plan.defaults {
            self.content_types.push_default(&e, &t);
        }
        for (p, t) in plan.overrides {
            self.content_types.push_override(&p, &t);
        }
        for (p, t) in plan.set_overrides {
            self.content_types.set_override(&p, &t);
        }
        let package_rels = rels_part_name(None);
        let removed_package_ids: Vec<String> =
            plan.removed_ids.iter().filter(|(r, _)| same_part_name(r, &package_rels)).map(|(_, i)| i.clone()).collect();
        if !removed_package_ids.is_empty() {
            self.package_rels = self.package_rels.without_ids(&removed_package_ids);
        }
        for r in plan.package_rels_added {
            self.package_rels.push(r);
        }
        self.generation += 1;
        Ok(())
    }

    /// Adds one part ([`RegistryPlan::add_part`] + [`Self::apply`]). Returns
    /// its name.
    pub fn add_part(&mut self, name: PartNameRequest, content_type: &str, content: impl Into<PartContent>) -> Result<String> {
        let mut plan = RegistryPlan::new(self);
        let name = plan.add_part(self, name, content_type, content)?;
        self.apply(plan)?;
        Ok(name)
    }

    /// The validation of [`Self::add_part`]; returns the name it would use.
    pub fn check_add_part(&self, name: PartNameRequest, content_type: &str, content: impl Into<PartContent>) -> Result<String> {
        RegistryPlan::new(self).add_part(self, name, content_type, content)
    }

    /// Adds a part and a relationship to it from `source` (`None` = the
    /// package), atomically. Returns `(part name, relationship Id)`.
    pub fn add_related_part(
        &mut self,
        source: Option<&str>,
        rel_type: &str,
        name: PartNameRequest,
        content_type: &str,
        content: impl Into<PartContent>,
    ) -> Result<(String, String)> {
        let (plan, out) = self.plan_related(source, rel_type, name, content_type, content)?;
        self.apply(plan)?;
        Ok(out)
    }

    /// The validation of [`Self::add_related_part`]; returns what it would
    /// allocate.
    pub fn check_add_related_part(
        &self,
        source: Option<&str>,
        rel_type: &str,
        name: PartNameRequest,
        content_type: &str,
        content: impl Into<PartContent>,
    ) -> Result<(String, String)> {
        self.plan_related(source, rel_type, name, content_type, content).map(|(_, out)| out)
    }

    fn plan_related(
        &self,
        source: Option<&str>,
        rel_type: &str,
        name: PartNameRequest,
        content_type: &str,
        content: impl Into<PartContent>,
    ) -> Result<(RegistryPlan, (String, String))> {
        let mut plan = RegistryPlan::new(self);
        let name = plan.add_part(self, name, content_type, content)?;
        let id = plan.add_relationship(self, source, rel_type, Target::Part(name.clone()))?;
        Ok((plan, (name, id)))
    }

    /// Adds one relationship ([`RegistryPlan::add_relationship`] +
    /// [`Self::apply`]) and returns its `Id`.
    pub fn add_relationship(&mut self, source: Option<&str>, rel_type: &str, target: Target) -> Result<String> {
        let mut plan = RegistryPlan::new(self);
        let id = plan.add_relationship(self, source, rel_type, target)?;
        self.apply(plan)?;
        Ok(id)
    }

    /// The validation of [`Self::add_relationship`]; returns the `Id` it
    /// would allocate.
    pub fn check_add_relationship(&self, source: Option<&str>, rel_type: &str, target: Target) -> Result<String> {
        RegistryPlan::new(self).add_relationship(self, source, rel_type, target)
    }

    /// Adds one `Default` ([`RegistryPlan::add_default_content_type`]).
    pub fn add_default_content_type(&mut self, ext: &str, content_type: &str) -> Result<bool> {
        let mut plan = RegistryPlan::new(self);
        let added = plan.add_default_content_type(self, ext, content_type)?;
        self.apply(plan)?;
        Ok(added)
    }

    /// The validation of [`Self::add_default_content_type`].
    pub fn check_add_default_content_type(&self, ext: &str, content_type: &str) -> Result<bool> {
        RegistryPlan::new(self).add_default_content_type(self, ext, content_type)
    }

    /// Removes one relationship ([`RegistryPlan::remove_relationship`]).
    pub fn remove_relationship(&mut self, source: Option<&str>, id: &str) -> Result<()> {
        let mut plan = RegistryPlan::new(self);
        plan.remove_relationship(self, source, id)?;
        self.apply(plan)
    }

    /// The validation of [`Self::remove_relationship`].
    pub fn check_remove_relationship(&self, source: Option<&str>, id: &str) -> Result<()> {
        RegistryPlan::new(self).remove_relationship(self, source, id)
    }

    /// Sets one part's content type ([`RegistryPlan::set_content_type`]).
    pub fn set_content_type(&mut self, name: &str, content_type: &str) -> Result<()> {
        let mut plan = RegistryPlan::new(self);
        plan.set_content_type(self, name, content_type)?;
        self.apply(plan)
    }

    /// The validation of [`Self::set_content_type`].
    pub fn check_set_content_type(&self, name: &str, content_type: &str) -> Result<()> {
        RegistryPlan::new(self).set_content_type(self, name, content_type)
    }

    /// The name a [`PartNameRequest`] would get now (pure).
    pub fn free_part_name(&self, request: PartNameRequest) -> Result<String> {
        let plan = RegistryPlan::new(self);
        let label = match &request {
            PartNameRequest::Exact(n) | PartNameRequest::Numbered(n) => n.clone(),
        };
        match request {
            PartNameRequest::Exact(n) => {
                let n = n.trim_start_matches('/').to_owned();
                plan.check_free(self, &n).map(|()| n)
            }
            PartNameRequest::Numbered(t) => plan.numbered(self, &t),
        }
        .map_err(|r| refused(&label, r))
    }

    /// What uses `name`, if anything: a part (OPC equivalence) or one it is
    /// a segment prefix of (or the reverse), an `Override`, or a relationship
    /// target (dangling ones included). `None` = free.
    pub fn part_name_in_use(&self, name: &str) -> Option<String> {
        self.name_user(name.trim_start_matches('/'), std::iter::empty())
    }

    fn name_user<'a>(&self, name: &str, planned: impl Iterator<Item = &'a str>) -> Option<String> {
        let norm = normalize_part_name(name);
        let conflict = |other: &str| {
            let o = normalize_part_name(other);
            o == norm || o.starts_with(&format!("{norm}/")) || norm.starts_with(&format!("{o}/"))
        };
        if let Some(p) = self.parts.iter().find(|p| conflict(&p.name)) {
            return Some(format!("part {:?}", p.name));
        }
        if let Some(p) = planned.into_iter().find(|p| conflict(p)) {
            return Some(format!("planned part {p:?}"));
        }
        if self.content_types.overrides().iter().any(|(p, _)| same_part_name(p, name)) {
            return Some("an Override".into());
        }
        self.relationship_targets().into_iter().find(|t| same_part_name(t, name)).map(|t| format!("a relationship target {t:?}"))
    }

    /// Every internal relationship target in the package, resolved
    /// (dangling ones included); unreadable relationships parts are skipped.
    fn relationship_targets(&self) -> Vec<String> {
        let mut out = Vec::new();
        for p in &self.parts {
            let n = normalize_part_name(&p.name);
            if !is_rels_part(&n) {
                continue;
            }
            let Some(src) = source_of_rels_part(&n) else { continue };
            let Ok(bytes) = self.read_part(&p.name) else { continue };
            let Ok(rels) = Relationships::parse(src.base().as_deref(), &bytes) else { continue };
            out.extend(rels.iter().filter(|r| r.target_mode == TargetMode::Internal).map(|r| resolve_target(src.base().as_deref(), xml_trim(&r.target))));
        }
        out
    }

    /// Names a [`PartNameRequest::Numbered`] template must not reuse.
    fn names_in_use(&self) -> Vec<String> {
        let mut v: Vec<String> = self.parts.iter().map(|p| p.name.clone()).collect();
        v.extend(self.content_types.overrides().iter().map(|(p, _)| p.clone()));
        v.extend(self.relationship_targets());
        v
    }

    /// Every relationship, in any relationships part, whose internal target
    /// resolves to `name` (OPC equivalence). For deciding whether a part is
    /// an orphan before removing it.
    pub fn incoming_relationships(&self, name: &str) -> Vec<IncomingRelationship> {
        let mut out = Vec::new();
        for p in &self.parts {
            let n = normalize_part_name(&p.name);
            if !is_rels_part(&n) {
                continue;
            }
            let Some(src) = source_of_rels_part(&n) else { continue };
            let Ok(bytes) = self.read_part(&p.name) else { continue };
            let Ok(rels) = Relationships::parse(src.base().as_deref(), &bytes) else { continue };
            for r in rels.iter().filter(|r| r.target_mode == TargetMode::Internal) {
                if same_part_name(&resolve_target(src.base().as_deref(), xml_trim(&r.target)), name) {
                    out.push(IncomingRelationship {
                        source: src.part().map(|s| self.find_ci(s).map_or_else(|| s.to_owned(), |i| self.parts[i].name.clone())),
                        rels: p.name.clone(),
                        id: xml_trim(&r.id).to_owned(),
                        rel_type: xml_trim(&r.rel_type).to_owned(),
                    });
                }
            }
        }
        out
    }

    /// Removes a set of parts ([`RegistryPlan::remove_parts`] +
    /// [`Self::apply`]).
    pub fn remove_parts(&mut self, names: &[&str], allowed_rel_types: &[&str]) -> Result<RemovedParts> {
        let mut plan = RegistryPlan::new(self);
        let removed = plan.remove_parts(self, names, allowed_rel_types)?;
        self.apply(plan)?;
        Ok(removed)
    }

    /// The validation of [`Self::remove_parts`], without changing anything.
    pub fn check_remove_parts(&self, names: &[&str], allowed_rel_types: &[&str]) -> Result<RemovedParts> {
        RegistryPlan::new(self).remove_parts(self, names, allowed_rel_types)
    }
}

impl RegistryPlan {
    fn is_gone(&self, name: &str) -> bool {
        self.gone_names.iter().any(|g| same_part_name(g, name))
    }

    /// Removes a closed set of parts, each **with its own relationships
    /// part**, as one step: a cascade (a sheet and its drawing, chart and
    /// comments; a slide, its notes and its now-unreferenced media) is
    /// validated as a whole, and references *inside* the set do not block
    /// it.
    ///
    /// For every relationship in a relationships part that **stays** whose
    /// internal target is in the set: its type must be in
    /// `allowed_rel_types` (parts found by relationship type are referenced
    /// implicitly) and its source part, if it stays, must not refer to the
    /// `Id` in any attribute (any namespace, after XML unescaping); the
    /// relationship is removed. A relationship whose source is in the set
    /// goes with the source's relationships part, unchecked. Every
    /// `Override` for a removed part, or for its relationships part, is
    /// removed; `Default`s never are. Returns what goes, and the internal
    /// targets the removed parts pointed at that stay (`kept_targets`), so
    /// the caller can decide whether they are orphans now
    /// ([`Package::incoming_relationships`]).
    ///
    /// Refusals ([`Error::RegistryRefused`]; the plan is unchanged):
    /// `PartNotFound` (also for a part added in this plan or already
    /// removed), `RegistryPart`, `ConflictsWithPlan` (a relationship this
    /// plan adds to or from a removed part, or a content-type change of
    /// one), or `Remove(…)` with PR #14's reasons:
    /// `RelationshipTypeNotAllowed`, `StillReferenced`,
    /// `SourceNotScannable`, `UnresolvedMention` (a relationship spelled
    /// with a removed part's file name that resolves to no part),
    /// `RegistryNotEditable`.
    pub fn remove_parts(&mut self, pkg: &Package, names: &[&str], allowed_rel_types: &[&str]) -> Result<RemovedParts> {
        let label = names.first().copied().unwrap_or("");
        self.step(pkg, label, |plan| plan.removal(pkg, names, allowed_rel_types))
    }

    fn removal(&mut self, pkg: &Package, names: &[&str], allowed: &[&str]) -> std::result::Result<RemovedParts, RegistryRefusal> {
        use RegistryRefusal as R;
        let mut idx: Vec<usize> = Vec::new();
        for n in names {
            let i = pkg.find_ci(n).ok_or(R::PartNotFound)?;
            if self.gone.contains(&i) {
                return Err(R::PartNotFound);
            }
            if is_registry_name(&pkg.parts[i].name) {
                return Err(R::RegistryPart);
            }
            if !idx.contains(&i) {
                idx.push(i);
            }
        }
        let set_names: Vec<String> = idx.iter().map(|&i| pkg.parts[i].name.clone()).collect();
        let in_set = |n: &str| set_names.iter().any(|s| same_part_name(s, n));
        // The plan must not use a removed part.
        if self.staged.iter().any(|(i, _)| idx.contains(i)) {
            return Err(R::ConflictsWithPlan);
        }
        if self.overrides.iter().chain(&self.set_overrides).any(|(p, _)| in_set(p)) {
            return Err(R::ConflictsWithPlan);
        }
        for p in &self.new_parts {
            let n = normalize_part_name(&p.name);
            if is_rels_part(&n)
                && let Some(src) = source_of_rels_part(&n).and_then(|s| s.part().map(str::to_owned))
                && in_set(&src)
            {
                return Err(R::ConflictsWithPlan);
            }
        }
        for (rels, node) in self.planned_relationships(pkg) {
            let src = source_of_rels_part(&normalize_part_name(&rels));
            let base = src.as_ref().and_then(RelsSource::base);
            if src.as_ref().and_then(|s| s.part().map(str::to_owned)).is_some_and(|s| in_set(&s))
                || (attr(&node, "TargetMode") != Some("External")
                    && attr(&node, "Target").is_some_and(|t| in_set(&resolve_target(base.as_deref(), t))))
            {
                return Err(R::ConflictsWithPlan);
            }
        }
        // Own relationships parts go with their parts.
        let mut gone = idx.clone();
        let mut kept_targets = Vec::new();
        for &i in &idx {
            let name = pkg.parts[i].name.clone();
            let Some(ri) = pkg.find_ci(&rels_part_name(Some(&name))) else { continue };
            if self.edits.iter().any(|e| e.part == ri && (!e.appends.is_empty() || !e.removes.is_empty())) {
                return Err(R::ConflictsWithPlan);
            }
            let not_editable = |reason: String| R::Remove(RemoveRefusal::RegistryNotEditable { part: pkg.parts[ri].name.clone(), reason });
            let bytes = pkg.read_part(&pkg.parts[ri].name).map_err(|e| not_editable(e.to_string()))?;
            let rels = Relationships::parse(Some(&name), &bytes).map_err(|e| not_editable(e.to_string()))?;
            for r in rels.iter().filter(|r| r.target_mode == TargetMode::Internal) {
                let t = resolve_target(Some(&name), xml_trim(&r.target));
                if !in_set(&t) {
                    kept_targets.push((name.clone(), t, xml_trim(&r.rel_type).to_owned()));
                }
            }
            gone.push(ri);
        }
        let gone_names: Vec<String> = gone.iter().map(|&i| pkg.parts[i].name.clone()).collect();
        let prior_gone = self.gone_names.clone();
        let is_gone = |n: &str| gone_names.iter().chain(&prior_gone).any(|g| same_part_name(g, n));
        let file_names: Vec<String> =
            set_names.iter().map(|n| normalize_part_name(n).rsplit('/').next().unwrap_or_default().to_owned()).collect();
        let mentions = |pi: usize| {
            let text = normalize_part_name(&String::from_utf8_lossy(&pkg.read_part(&pkg.parts[pi].name).unwrap_or_default()));
            file_names.iter().any(|f| !f.is_empty() && text.contains(f.as_str()))
        };
        let mut relationships = Vec::new();
        let mut package_ids = Vec::new();
        for pi in 0..pkg.parts.len() {
            let n = normalize_part_name(&pkg.parts[pi].name);
            if !is_rels_part(&n) || gone.contains(&pi) || self.gone.contains(&pi) {
                continue;
            }
            let Some(src) = source_of_rels_part(&n) else { continue };
            let pos = match self.edit_for(pkg, pi, rels_root_q()) {
                Ok(pos) => pos,
                Err(R::RegistryNotEditable { part, reason }) => {
                    if mentions(pi) {
                        return Err(R::Remove(RemoveRefusal::RegistryNotEditable { part, reason }));
                    }
                    continue;
                }
                Err(e) => return Err(e),
            };
            let tree = self.tree(pkg, pos);
            let mut indices = Vec::new();
            let mut ids = Vec::new();
            for (i, e) in tree.root().indexed_elements().filter(|(_, e)| e.is(rel_q())) {
                if attr(e, "TargetMode") == Some("External") {
                    continue;
                }
                let Some(target) = attr(e, "Target") else { continue };
                let resolved = resolve_target(src.base().as_deref(), target);
                if !in_set(&resolved) {
                    let seg = normalize_part_name(target);
                    let seg = seg.rsplit(['/', '\\']).next().unwrap_or_default();
                    if file_names.iter().any(|f| !f.is_empty() && f == seg) && pkg.find_ci(&resolved).is_none() {
                        return Err(R::Remove(RemoveRefusal::UnresolvedMention { rels: pkg.parts[pi].name.clone() }));
                    }
                    continue;
                }
                let id = attr(e, "Id").unwrap_or_default().to_owned();
                let rel_type = attr(e, "Type").unwrap_or_default();
                if !allowed.contains(&rel_type) {
                    return Err(R::Remove(RemoveRefusal::RelationshipTypeNotAllowed {
                        rels: pkg.parts[pi].name.clone(),
                        id,
                        rel_type: rel_type.to_owned(),
                    }));
                }
                if self.removed_ids.iter().any(|(r, x)| same_part_name(r, &pkg.parts[pi].name) && *x == id) {
                    continue; // already removed by remove_relationship in this plan
                }
                indices.push(i);
                ids.push(id);
            }
            if indices.is_empty() {
                continue;
            }
            if let Some(s) = src.part()
                && let Some(si) = pkg.find_ci(s)
                && !is_gone(&pkg.parts[si].name)
            {
                let src_name = pkg.parts[si].name.clone();
                let bytes = self.view_bytes(pkg, &src_name).map_err(|reason| R::SourceNotScannable { reason })?;
                for id in &ids {
                    match refers_to_rel_id(&bytes, id) {
                        Ok(false) => {}
                        Ok(true) => return Err(R::Remove(RemoveRefusal::StillReferenced { source: src_name, id: id.clone() })),
                        Err(reason) => return Err(R::Remove(RemoveRefusal::SourceNotScannable { source: src_name, reason })),
                    }
                }
            }
            let rels_name = pkg.parts[pi].name.clone();
            if src == RelsSource::Package {
                package_ids.extend(ids.iter().cloned());
            }
            self.edits[pos].removes.extend(&indices);
            self.removed_ids.extend(ids.iter().map(|id| (rels_name.clone(), id.clone())));
            relationships.extend(ids.into_iter().map(|id| (rels_name.clone(), id)));
        }
        // Overrides of everything that goes.
        let overrides_gone = pkg.content_types.overrides().iter().filter(|(p, _)| is_gone(p)).count();
        let mut overrides = 0;
        if overrides_gone > 0 {
            let pos = self.content_types_edit(pkg).map_err(|e| match e {
                R::RegistryNotEditable { part, reason } => R::Remove(RemoveRefusal::RegistryNotEditable { part, reason }),
                e => e,
            })?;
            let ix: Vec<usize> = self
                .tree(pkg, pos)
                .root()
                .indexed_elements()
                .filter(|(_, e)| e.is(QName::new(CONTENT_TYPES_NS, "Override")) && attr(e, "PartName").is_some_and(|p| gone_names.iter().any(|g| same_part_name(g, p))))
                .map(|(i, _)| i)
                .collect();
            overrides = ix.len();
            self.edits[pos].removes.extend(ix);
        }
        self.gone.extend(&gone);
        self.gone_names.extend(gone_names.iter().cloned());
        Ok(RemovedParts { parts: gone_names, relationships, overrides, kept_targets })
    }

    /// Stages new content for an existing, non-registry part (or replaces
    /// the content of a part added earlier in the plan), so that a content
    /// edit and the registry edits it goes with are validated and applied
    /// together: removing `<sheet r:id>` from `workbook.xml` together with
    /// the sheet; removing `<p:sldId>` from `presentation.xml` together
    /// with the slide.
    ///
    /// Typical use: `let mut doc = pkg.xml_part(name)?.clone();` (parsing
    /// does not make a plan stale; `xml_part_mut` does), edit `doc` through
    /// a lens, then `plan.stage_part(&pkg, name, doc)`. A tree cloned from
    /// the parsed part keeps its source spans, so untouched subtrees are
    /// still written byte-for-byte. Bytes replace the part wholesale
    /// (`Replaced`); XML content types must be well-formed.
    ///
    /// Later steps see the staged content: the reference scans of
    /// `remove_relationship` and `remove_parts` read it. Refused: a missing,
    /// removed or registry part (`PartNotFound`, `ConflictsWithPlan`,
    /// `RegistryPart`), malformed XML, or content that refers (in any
    /// attribute) to a relationship `Id` this plan removed from the part's
    /// relationships (`ConflictsWithPlan`).
    pub fn stage_part(&mut self, pkg: &Package, name: &str, content: impl Into<PartContent>) -> Result<()> {
        let content = content.into();
        self.step(pkg, name, |plan| {
            let bytes = match &content {
                PartContent::Bytes(b) => b.clone(),
                PartContent::Xml(d) => d.serialize().into_bytes(),
            };
            let (canonical, existing) = match pkg.find_ci(name) {
                Some(i) if plan.gone.contains(&i) => return Err(RegistryRefusal::ConflictsWithPlan),
                Some(i) => (pkg.parts[i].name.clone(), Some(i)),
                None => (plan.pending_part(name).ok_or(RegistryRefusal::PartNotFound)?.to_owned(), None),
            };
            if is_registry_name(&canonical) {
                return Err(RegistryRefusal::RegistryPart);
            }
            if let PartContent::Bytes(b) = &content
                && plan.content_type(pkg, &canonical).is_some_and(is_xml_media_type)
            {
                XmlDocument::parse(b).map_err(|e| RegistryRefusal::MalformedXml { reason: e.to_string() })?;
            }
            let rels = rels_part_name(Some(&canonical));
            for (r, id) in &plan.removed_ids {
                if same_part_name(r, &rels) && refers_to_rel_id(&bytes, id).unwrap_or(true) {
                    return Err(RegistryRefusal::ConflictsWithPlan);
                }
            }
            match existing {
                Some(i) => {
                    plan.staged.retain(|(j, _)| *j != i);
                    plan.staged.push((i, content));
                }
                None => {
                    let p = plan.new_parts.iter_mut().find(|p| same_part_name(&p.name, &canonical)).expect("pending");
                    if !matches!(p.content, NewContent::Given(_)) {
                        return Err(RegistryRefusal::RegistryPart);
                    }
                    p.content = NewContent::Given(content);
                }
            }
            Ok(())
        })
    }

    /// Gives `dst` the relationships of `src`, **with the same `Id`s**, so a
    /// duplicated part (a slide copied as XML) keeps resolving its `r:id`s.
    /// `dst` must have no relationships part yet (existing or planned); it
    /// is typically a part added earlier in the plan. Each internal target
    /// is written relative to `dst` so it resolves to the same part, or to
    /// the part `target_map` maps it to (e.g. a duplicated notes slide);
    /// external targets are copied as they are. Returns the copied `Id`s.
    ///
    /// Refused: a missing or removed `src`/`dst`, a registry part, `dst`
    /// already having relationships, an internal target (after mapping)
    /// that is not a part in the plan's view, or a `src` relationships part
    /// that does not parse.
    pub fn copy_relationships(&mut self, pkg: &Package, src: &str, dst: &str, target_map: &[(&str, &str)]) -> Result<Vec<String>> {
        self.step(pkg, dst, |plan| {
            let src = plan.resolve_source(pkg, Some(src))?.expect("Some");
            let dst = plan.resolve_source(pkg, Some(dst))?.expect("Some");
            let dst_rels = rels_part_name(Some(&dst));
            if pkg.find_ci(&dst_rels).is_some() || plan.pending_part(&dst_rels).is_some() {
                return Err(RegistryRefusal::NameInUse { by: format!("{dst_rels:?} (dst already has relationships)") });
            }
            let src_rels = rels_part_name(Some(&src));
            let rels = match pkg.find_ci(&src_rels) {
                Some(ri) => Relationships::parse(Some(&src), &pkg.read_part(&pkg.parts[ri].name).map_err(|e| {
                    RegistryRefusal::RegistryNotEditable { part: src_rels.clone(), reason: e.to_string() }
                })?)
                .map_err(|e| RegistryRefusal::RegistryNotEditable { part: src_rels.clone(), reason: e.to_string() })?,
                None => return Ok(Vec::new()),
            };
            let mut ids = Vec::new();
            let mut copy = plan.clone();
            for r in rels.iter() {
                if copy.removed_ids.iter().any(|(rp, id)| same_part_name(rp, &src_rels) && id == xml_trim(&r.id)) {
                    continue;
                }
                let target = match r.target_mode {
                    TargetMode::External => Target::External(xml_trim(&r.target).to_owned()),
                    TargetMode::Internal => {
                        let t = resolve_target(Some(&src), xml_trim(&r.target));
                        let t = target_map.iter().find(|(from, _)| same_part_name(from, &t)).map_or(t, |(_, to)| (*to).to_owned());
                        Target::Part(t)
                    }
                };
                copy.relationship_with_id(pkg, &dst, xml_trim(&r.rel_type), target, xml_trim(&r.id))?;
                ids.push(xml_trim(&r.id).to_owned());
            }
            *plan = copy;
            Ok(ids)
        })
    }

    /// `relationship` with a given `Id` into a new relationships part.
    fn relationship_with_id(&mut self, pkg: &Package, source: &str, rel_type: &str, target: Target, id: &str) -> std::result::Result<(), RegistryRefusal> {
        let before = self.new_ids.len();
        let allocated = self.relationship(pkg, Some(source), rel_type, target)?;
        // rewrite the allocated Id to the copied one (the .rels part is new, so
        // only copied Ids are in it: no collision)
        let rels_name = rels_part_name(Some(source));
        if let Some(NewPart { content: NewContent::Rels(nodes), .. }) = self.new_parts.iter_mut().find(|p| same_part_name(&p.name, &rels_name))
            && let Some(node) = nodes.last_mut()
            && let Some(a) = node.attrs.iter_mut().find(|a| a.name.is(QName::new("", "Id")))
        {
            debug_assert_eq!(a.value, allocated);
            a.value = id.to_owned();
        }
        if let Some((_, i)) = self.new_ids.get_mut(before) {
            *i = id.to_owned();
        }
        Ok(())
    }

    /// `(relationships part, Relationship node)` of every relationship the
    /// plan adds.
    fn planned_relationships(&self, pkg: &Package) -> Vec<(String, RawNode)> {
        let mut out = Vec::new();
        for e in &self.edits {
            for n in &e.appends {
                if n.is(rel_q()) {
                    out.push((pkg.parts[e.part].name.clone(), n.clone()));
                }
            }
        }
        for p in &self.new_parts {
            if let NewContent::Rels(nodes) = &p.content {
                out.extend(nodes.iter().map(|n| (p.name.clone(), n.clone())));
            }
        }
        out
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
            "", "a/", "a//b", "a/./b", "a/../b", "a./b", "a/b.", "a%2Fb", "a%5cb", "a%41", "a%2", "a b", "a\\b", "a#b", "a?b",
            "a[1]", "a\"b", "a<b",
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
        let next = |v: &[&str]| next_relationship_id(v.iter().copied());
        assert_eq!(next(&[]).as_deref(), Some("rId1"));
        assert_eq!(next(&["rId1", "rId5"]).as_deref(), Some("rId6"));
        assert_eq!(next(&["rId2", "rId10", "R44c83fbb49ca402c"]).as_deref(), Some("rId11"));
        assert_eq!(next(&["rId1", "RID2"]).as_deref(), Some("rId3"));
        assert_eq!(next(&["foo", "rIdx"]).as_deref(), Some("rId1"));
        assert_eq!(next(&["rId18446744073709551615"]), None);
    }
}
