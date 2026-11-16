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
//! part's `Override`s; [`RegistryPlan::set_relationship_target`] rewrites
//! one relationship's `Target`; [`RegistryPlan::remove_relationship`]
//! removes one relationship of an allowed type that its source does not
//! refer to; [`Package::remove_parts`] removes the entries that register
//! the removed parts.
//!
//! # The plan's view
//!
//! Every query a step makes (names in use, relationships of a part, content
//! types, part content for reference scans) reads the package **as the plan
//! leaves it**: parts it removes are gone, parts it adds and content it
//! stages are there, relationships it adds or removes are counted.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use super::{Package, Part, PartState, RelsKind, RelsSource, RemoveRefusal, is_rels_part, refers_to_rel_id, rels_kind, source_of_rels_part};
use crate::error::{Error, Result};
use crate::name::{CONTENT_TYPES_NS, PACKAGE_RELATIONSHIPS_NS, QName, XName};
use crate::opc::{
    CONTENT_TYPES_PART, Relationship, TargetMode, normalize_part_name, rels_part_name, relative_target, resolve_target,
    same_part_name, xml_trim,
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
    /// uses it (see [`Package::part_name_in_use`]). A name removed earlier
    /// in the plan or session may be reused (a part replaced in one plan);
    /// its relationships part then continues above the removed `Id`s.
    Exact(String),
    /// A template with one `{}`, e.g. `xl/worksheets/sheet{}.xml`: the
    /// number is **one more than the largest number in use** for the
    /// template, compared under OPC equivalence, among existing parts,
    /// `Override`s, relationship targets (dangling ones included), names
    /// removed in this session or plan and names planned earlier; `1` when
    /// none.
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
/// matched ASCII case-insensitively). Nothing can collide with it, even
/// under ASCII case folding: an Id equal to `rId{n}` up to case would be an
/// `rId<digits>` Id numbered `n`, which the maximum counts. Other Ids
/// (`R44c8…`) are left alone.
fn next_relationship_id<'a>(ids: impl Iterator<Item = &'a str>) -> Option<String> {
    let max = ids
        .filter_map(|id| {
            let (p, d) = id.split_at_checked(3)?;
            (p.eq_ignore_ascii_case("rId") && !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit())).then_some(d)
        })
        .filter_map(|d| d.parse::<u64>().ok())
        .max()
        .unwrap_or(0);
    Some(format!("rId{}", max.checked_add(1)?))
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
    /// The part's tree as planned against (the parsed tree, or the `Raw`
    /// part parsed for planning). Shared, so a step's copy of the plan is
    /// cheap (review of PR #38 m1). Installed on apply whatever the part's
    /// state then is (review m2 b): the generation check guarantees it is
    /// the part's current content.
    tree: Arc<XmlDocument>,
    /// Root child indices to remove (with their leading whitespace).
    removes: Vec<usize>,
    appends: Vec<RawNode>,
    /// `(root child index, attribute, value)`: `ContentType` of an
    /// `Override`, `Target` of a `Relationship`.
    set_attrs: Vec<(usize, &'static str, String)>,
}

impl RegEdit {
    fn changes(&self) -> bool {
        !(self.removes.is_empty() && self.appends.is_empty() && self.set_attrs.is_empty())
    }
}

#[derive(Clone, Debug)]
struct NewPart {
    name: String,
    content: NewContent,
}

#[derive(Clone, Debug)]
enum NewContent {
    Given(Arc<PartContent>),
    /// A new relationships part for `source`, with these relationships.
    Rels(Vec<RawNode>),
}

/// New content for an existing part (`stage_part`).
#[derive(Clone, Debug)]
struct Staged {
    part: usize,
    content: Arc<PartContent>,
    /// The tree is not the part's current content: written fresh on save
    /// (review M2). `false` only for an unedited clone of the current tree.
    detach: bool,
}

/// One relationship in the plan's view ([`RegistryPlan::view_rels`]).
#[derive(Clone, Debug)]
struct ViewRel {
    /// The relationships part.
    rels: String,
    /// Whom it belongs to (from the raw name).
    source: RelsSource,
    id: String,
    rel_type: String,
    /// `Target`, XML whitespace collapsed.
    target: String,
    external: bool,
}

impl ViewRel {
    fn from_node(rels: &str, source: RelsSource, e: &RawNode) -> Self {
        ViewRel {
            rels: rels.to_owned(),
            source,
            id: attr(e, "Id").unwrap_or_default().to_owned(),
            rel_type: attr(e, "Type").unwrap_or_default().to_owned(),
            target: attr(e, "Target").unwrap_or_default().to_owned(),
            external: attr(e, "TargetMode") == Some("External"),
        }
    }

    /// The part an internal target points at; `None` for external and
    /// fragment-only (`#_ftn1`) targets, which name no part (review m10).
    fn part(&self) -> Option<String> {
        if self.external {
            return None;
        }
        part_target(self.source.base().as_deref(), &self.target)
    }
}

/// The part an internal `target` names, relative to `base`; `None` for an
/// empty or fragment-only target.
fn part_target(base: Option<&str>, target: &str) -> Option<String> {
    let t = xml_trim(target);
    if t.is_empty() || t.starts_with('#') {
        return None;
    }
    Some(resolve_target(base, t))
}

fn dir_of(name: &str) -> &str {
    name.rsplit_once('/').map_or("", |(d, _)| d)
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
    /// `(Id, Target)` changed in the package relationships.
    package_rels_retargeted: Vec<(String, String)>,
    /// Parts removed by the plan (indices into `parts`), and their names.
    gone: Vec<usize>,
    gone_names: Vec<String>,
    staged: Vec<Staged>,
}

type R<T> = std::result::Result<T, RegistryRefusal>;

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
            package_rels_retargeted: Vec::new(),
            gone: Vec::new(),
            gone_names: Vec::new(),
            staged: Vec::new(),
        }
    }

    /// Does the plan change anything?
    pub fn is_empty(&self) -> bool {
        self.new_parts.is_empty() && self.gone.is_empty() && self.staged.is_empty() && !self.edits.iter().any(RegEdit::changes)
    }

    fn check(&self, pkg: &Package) -> R<()> {
        if self.package != pkg.id || self.generation != pkg.generation {
            return Err(RegistryRefusal::StalePlan);
        }
        Ok(())
    }

    /// Runs `f` on a copy of the plan and keeps the copy only on success,
    /// so a refused step leaves the plan as it was. Trees in the plan are
    /// shared (`Arc`), so the copy is shallow.
    fn step<T>(&mut self, pkg: &Package, part: &str, f: impl FnOnce(&mut Self) -> R<T>) -> Result<T> {
        self.check(pkg).map_err(|r| refused(part, r))?;
        let mut copy = self.clone();
        let out = f(&mut copy).map_err(|r| refused(part, r))?;
        *self = copy;
        Ok(out)
    }

    // ------------------------------------------------------------ the view

    /// The existing part `name` names, unless the plan removes it.
    fn live(&self, pkg: &Package, name: &str) -> Option<usize> {
        pkg.find_ci(name).filter(|i| !self.gone.contains(i))
    }

    fn pending_part(&self, name: &str) -> Option<&str> {
        self.new_parts.iter().find(|p| same_part_name(&p.name, name)).map(|p| p.name.as_str())
    }

    /// The canonical name of a part in the plan's view: an existing part the
    /// plan keeps, or one it adds. A removed one is `ConflictsWithPlan`.
    fn view_part(&self, pkg: &Package, name: &str) -> R<String> {
        if let Some(i) = self.live(pkg, name) {
            return Ok(pkg.parts[i].name.clone());
        }
        if let Some(p) = self.pending_part(name) {
            return Ok(p.to_owned());
        }
        if self.is_gone(name) { Err(RegistryRefusal::ConflictsWithPlan) } else { Err(RegistryRefusal::PartNotFound) }
    }

    fn is_gone(&self, name: &str) -> bool {
        self.gone_names.iter().any(|g| same_part_name(g, name))
    }

    /// The content a part has in the plan's view: staged content, content of
    /// a part added in the plan, else the package's current content.
    fn view_bytes(&self, pkg: &Package, name: &str) -> std::result::Result<Vec<u8>, String> {
        let bytes = |c: &PartContent| match c {
            PartContent::Bytes(b) => b.clone(),
            PartContent::Xml(d) => d.serialize().into_bytes(),
        };
        if let Some(i) = self.live(pkg, name) {
            if let Some(s) = self.staged.iter().find(|s| s.part == i) {
                return Ok(bytes(&s.content));
            }
            return pkg.read_part(&pkg.parts[i].name).map_err(|e| e.to_string());
        }
        if let Some(NewPart { content: NewContent::Given(c), .. }) = self.new_parts.iter().find(|p| same_part_name(&p.name, name)) {
            return Ok(bytes(c));
        }
        Err("no such part".into())
    }

    /// The edit entry for registry part `i`, parsing a `Raw` part into it.
    fn edit_for(&mut self, pkg: &Package, i: usize, root: QName) -> R<usize> {
        if let Some(pos) = self.edits.iter().position(|e| e.part == i) {
            return Ok(pos);
        }
        let not_editable = |reason: String| RegistryRefusal::RegistryNotEditable { part: pkg.parts[i].name.clone(), reason };
        let tree = match &pkg.parts[i].state {
            PartState::Parsed(d) => d.clone(),
            PartState::Raw => pkg.parse_raw(i).map_err(not_editable)?,
            PartState::Replaced(_) => return Err(not_editable("replaced wholesale; cannot be edited in place".into())),
        };
        if !tree.root().is(root) {
            return Err(not_editable(format!("root is not <{}>", root.local)));
        }
        self.edits.push(RegEdit { part: i, tree: Arc::new(tree), removes: Vec::new(), appends: Vec::new(), set_attrs: Vec::new() });
        Ok(self.edits.len() - 1)
    }

    fn tree(&self, pos: usize) -> &XmlDocument {
        &self.edits[pos].tree
    }

    fn content_types_edit(&mut self, pkg: &Package) -> R<usize> {
        let ct = pkg.find(CONTENT_TYPES_PART).ok_or_else(|| RegistryRefusal::RegistryNotEditable {
            part: CONTENT_TYPES_PART.into(),
            reason: "missing".into(),
        })?;
        self.edit_for(pkg, ct, types_q())
    }

    /// The relationships of the existing relationships part `pi` in the
    /// plan's view: its entries minus those the plan removes, plus those it
    /// appends. `Err(reason)` when the part cannot be read or parsed.
    fn view_rels_of(&self, pkg: &Package, pi: usize, source: &RelsSource) -> std::result::Result<Vec<ViewRel>, String> {
        let name = &pkg.parts[pi].name;
        let source_of = || source.clone();
        let mut out = Vec::new();
        if let Some(e) = self.edits.iter().find(|e| e.part == pi) {
            for (i, n) in e.tree.root().indexed_elements().filter(|(_, n)| n.is(rel_q())) {
                if !e.removes.contains(&i) {
                    let mut r = ViewRel::from_node(name, source_of(), n);
                    if let Some((_, _, t)) = e.set_attrs.iter().rev().find(|(j, a, _)| *j == i && *a == "Target") {
                        r.target = t.clone();
                    }
                    out.push(r);
                }
            }
            out.extend(e.appends.iter().filter(|n| n.is(rel_q())).map(|n| ViewRel::from_node(name, source_of(), n)));
            return Ok(out);
        }
        let bytes = pkg.read_part(name).map_err(|e| e.to_string())?;
        let doc = XmlDocument::parse(&bytes).map_err(|e| e.to_string())?;
        if !doc.root().is(rels_root_q()) {
            return Err("root is not <Relationships>".into());
        }
        out.extend(doc.root().elements().filter(|n| n.is(rel_q())).map(|n| ViewRel::from_node(name, source_of(), n)));
        Ok(out)
    }

    /// Every relationship in the plan's view (relationships parts the plan
    /// keeps, and those it creates). `strict`: a relationships part that
    /// cannot be read, or cannot be attributed, and mentions `mention` (or
    /// anything, when `mention` is empty) is `Err`; otherwise it is skipped.
    fn view_rels(&self, pkg: &Package, strict: bool, mention: &str) -> R<Vec<ViewRel>> {
        let mut out = Vec::new();
        for (pi, kind) in pkg.rels_parts() {
            if self.gone.contains(&pi) {
                continue;
            }
            let name = &pkg.parts[pi].name;
            let fail = |reason: String| -> R<()> {
                if strict && mentions(pkg, pi, mention) {
                    Err(RegistryRefusal::RegistryNotEditable { part: name.clone(), reason })
                } else {
                    Ok(())
                }
            };
            match kind {
                RelsKind::Unattributable => fail("a relationships part that cannot be attributed to a source (percent-encoded `_rels`/`.rels`)".into())?,
                RelsKind::Of(src) => match self.view_rels_of(pkg, pi, &src) {
                    Ok(v) => out.extend(v),
                    Err(reason) => fail(reason)?,
                },
            }
        }
        for p in &self.new_parts {
            if let NewContent::Rels(nodes) = &p.content
                && let Some(src) = source_of_rels_part(&p.name)
            {
                out.extend(nodes.iter().map(|n| ViewRel::from_node(&p.name, src.clone(), n)));
            }
        }
        Ok(out)
    }

    /// The content type `name` would have after the plan, counting planned
    /// `Override`s and `Default`s (`None` for a part the plan removes).
    pub fn content_type<'a>(&'a self, pkg: &'a Package, name: &str) -> Option<&'a str> {
        if self.is_gone(name) && self.pending_part(name).is_none() {
            return None;
        }
        if let Some((_, t)) = self.overrides.iter().chain(&self.set_overrides).rev().find(|(p, _)| same_part_name(p, name)) {
            return Some(t);
        }
        if !self.is_gone(name)
            && let Some((_, t)) = pkg.content_types.overrides().iter().find(|(p, _)| same_part_name(p, name))
        {
            return Some(t);
        }
        let ext = extension_of(name)?;
        self.default_for(pkg, ext)
    }

    fn default_for<'a>(&'a self, pkg: &'a Package, ext: &str) -> Option<&'a str> {
        pkg.content_types.defaults().iter().chain(&self.defaults).find(|(e, _)| e.eq_ignore_ascii_case(ext)).map(|(_, t)| t.as_str())
    }

    /// Registers the content type of a new name: nothing when a `Default`
    /// (existing or planned) gives exactly that type, else a new
    /// `Override`.
    fn register_content_type(&mut self, pkg: &Package, name: &str, content_type: &str) -> R<()> {
        if extension_of(name).and_then(|e| self.default_for(pkg, e)).is_some_and(|t| t.eq_ignore_ascii_case(content_type)) {
            return Ok(());
        }
        let pos = self.content_types_edit(pkg)?;
        let node = element(self.tree(pos).root(), "Override", &[("PartName", &format!("/{name}")), ("ContentType", content_type)]);
        self.edits[pos].appends.push(node);
        self.overrides.push((name.to_owned(), content_type.to_owned()));
        Ok(())
    }

    /// What uses `name` in the plan's view, if anything: a part the plan
    /// keeps or adds (OPC equivalence, segment prefixes), an `Override` that
    /// stays, or a relationship target (dangling ones included).
    fn name_user(&self, pkg: &Package, name: &str) -> Option<String> {
        let norm = normalize_part_name(name);
        let conflict = |other: &str| {
            let o = normalize_part_name(other);
            o == norm || o.starts_with(&format!("{norm}/")) || norm.starts_with(&format!("{o}/"))
        };
        if let Some(p) = pkg.parts.iter().enumerate().find(|(i, p)| !self.gone.contains(i) && conflict(&p.name)) {
            return Some(format!("part {:?}", p.1.name));
        }
        if let Some(p) = self.new_parts.iter().find(|p| conflict(&p.name)) {
            return Some(format!("planned part {:?}", p.name));
        }
        if pkg.content_types.overrides().iter().any(|(p, _)| same_part_name(p, name) && !self.is_gone(p)) {
            return Some("an Override".into());
        }
        let rels = self.view_rels(pkg, false, "").unwrap_or_default();
        rels.iter().filter_map(ViewRel::part).find(|t| same_part_name(t, name)).map(|t| format!("a relationship target {t:?}"))
    }

    // ------------------------------------------------------------ steps

    /// Adds `Default Extension=ext ContentType=content_type`. Returns
    /// `false` (and plans nothing) if that exact `Default` exists.
    ///
    /// Refused if a `Default` for `ext` (ASCII case-insensitive) has
    /// another type (`DefaultExists`), or if a part in the plan's view with
    /// that extension has no `Override` (`WouldRetypePart`): adding the
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
            let live = pkg.parts.iter().enumerate().filter(|(i, _)| !plan.gone.contains(i)).map(|(_, p)| p.name.as_str());
            for n in live.chain(plan.new_parts.iter().map(|p| p.name.as_str())) {
                let has_override = plan.overrides.iter().any(|(p, _)| same_part_name(p, n))
                    || pkg.content_types.overrides().iter().any(|(p, _)| same_part_name(p, n));
                if extension_of(n).is_some_and(|e| e.eq_ignore_ascii_case(ext)) && !has_override && !is_registry_name(n) {
                    return Err(RegistryRefusal::WouldRetypePart { part: n.to_owned() });
                }
            }
            let pos = plan.content_types_edit(pkg)?;
            let node = element(plan.tree(pos).root(), "Default", &[("Extension", ext), ("ContentType", content_type)]);
            plan.edits[pos].appends.push(node);
            plan.defaults.push((ext.to_owned(), content_type.to_owned()));
            Ok(true)
        })
    }

    /// Adds a part and registers its content type: nothing when a `Default`
    /// for its extension (existing or planned) gives exactly `content_type`
    /// (ASCII case-insensitive); else an `Override`. Returns the name. The
    /// part is written fresh on save, after the original entries.
    ///
    /// `Exact` may reuse the name of a part removed earlier (in this plan or
    /// session): the VBA project replaced in one plan. `Numbered` never does.
    ///
    /// Refused: an invalid name, template or media type, a registry part,
    /// a name in use in the plan's view ([`Package::part_name_in_use`]: a
    /// left-over `Override` or `.rels` for it counts), XML bytes that are
    /// not well-formed for an XML content type, a `[Content_Types].xml` that
    /// must change but was replaced or does not parse.
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
                XmlDocument::parse_with(b, &pkg.parse_options).map_err(|e| RegistryRefusal::MalformedXml { reason: e.to_string() })?;
            }
            plan.register_content_type(pkg, &name, content_type)?;
            plan.new_parts.push(NewPart { name: name.clone(), content: NewContent::Given(Arc::new(content)) });
            Ok(name)
        })
    }

    /// Validity and freeness of a name for a new part, in the plan's view.
    fn check_free(&self, pkg: &Package, name: &str) -> R<()> {
        if is_registry_name(name) {
            return Err(RegistryRefusal::RegistryPart);
        }
        validate_part_name(name).map_err(|reason| RegistryRefusal::InvalidPartName { reason })?;
        if let Some(by) = self.name_user(pkg, name) {
            return Err(RegistryRefusal::NameInUse { by });
        }
        // A relationships part left for the name would become the new
        // part's relationships (review of ADR-0009, M6).
        if let Some(i) = self.live(pkg, &rels_part_name(Some(name))) {
            return Err(RegistryRefusal::NameInUse { by: format!("its left-over relationships part {:?}", pkg.parts[i].name) });
        }
        Ok(())
    }

    fn numbered(&self, pkg: &Package, template: &str) -> R<String> {
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
        let targets: Vec<String> = pkg.relationship_targets();
        let max = pkg
            .parts
            .iter()
            .map(|p| p.name.as_str())
            .chain(pkg.content_types.overrides().iter().map(|(p, _)| p.as_str()))
            .chain(targets.iter().map(String::as_str))
            .chain(self.new_parts.iter().map(|p| p.name.as_str()))
            // (parts removed in this plan are still in `pkg.parts`)
            .chain(pkg.removed_names.iter().map(String::as_str))
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
    /// its `Id`: `rId{n}`, `n` one more than the largest `rId<digits>` among
    /// the source's relationships (in the plan's view), the `Id`s removed
    /// from it in this plan or session, **and every `rId<digits>` value of
    /// an attribute of the source part** (a dangling `r:id` is never
    /// adopted; review of PR #38 M7), skipping Ids used under ASCII case
    /// folding; other Ids are left alone. A missing relationships part is
    /// created and registered like a new part. The source may be a part
    /// added earlier in the plan.
    ///
    /// Refused: a missing or registry source, a relationship type that is
    /// not an absolute URI, an invalid target or one that resolves to no
    /// part in the plan's view, an XML source part that does not parse
    /// (`SourceNotScannable`), a relationships part (or
    /// `[Content_Types].xml`) that must change but was replaced or does not
    /// parse.
    pub fn add_relationship(&mut self, pkg: &Package, source: Option<&str>, rel_type: &str, target: Target) -> Result<String> {
        self.step(pkg, source.unwrap_or(""), |plan| plan.relationship(pkg, source, rel_type, target))
    }

    fn resolve_source(&self, pkg: &Package, source: Option<&str>) -> R<Option<String>> {
        let Some(s) = source else { return Ok(None) };
        let name = self.view_part(pkg, s)?;
        if is_registry_name(&name) {
            return Err(RegistryRefusal::RegistryPart);
        }
        Ok(Some(name))
    }

    /// An internal target for `source` naming the part `part` (in the view).
    fn internal_target(&self, pkg: &Package, source: Option<&str>, part: &str, suffix: &str) -> R<String> {
        let canonical = match self.view_part(pkg, part.trim_start_matches('/')) {
            Ok(c) => c,
            Err(RegistryRefusal::PartNotFound) => return Err(RegistryRefusal::TargetNotFound { resolved: part.to_owned() }),
            Err(e) => return Err(e),
        };
        if is_registry_name(&canonical) {
            return Err(RegistryRefusal::RegistryPart);
        }
        let t = relative_target(source, &canonical);
        if !uri_like(&t) || t.contains(['#', '?', '\\']) || !same_part_name(&resolve_target(source, &t), &canonical) {
            return Err(RegistryRefusal::InvalidTarget);
        }
        Ok(format!("{t}{suffix}"))
    }

    /// `rId<digits>` values of the source part's attributes (M7). A source
    /// that does not parse as XML has none, unless its content type is XML
    /// (`SourceNotScannable`).
    fn source_rid_values(&self, pkg: &Package, source: &str) -> R<Vec<String>> {
        let xml = self.content_type(pkg, source).is_some_and(is_xml_media_type);
        let bytes = match self.view_bytes(pkg, source) {
            Ok(b) => b,
            Err(reason) if xml => return Err(RegistryRefusal::SourceNotScannable { reason }),
            Err(_) => return Ok(Vec::new()),
        };
        match rid_like_values(&bytes) {
            Ok(v) => Ok(v),
            Err(reason) if xml => Err(RegistryRefusal::SourceNotScannable { reason }),
            Err(_) => Ok(Vec::new()),
        }
    }

    fn relationship(&mut self, pkg: &Package, source: Option<&str>, rel_type: &str, target: Target) -> R<String> {
        let source = self.resolve_source(pkg, source)?;
        if !uri_like(rel_type) || !rel_type.contains(':') {
            return Err(RegistryRefusal::InvalidRelationshipType);
        }
        let (target, mode) = match target {
            Target::Part(p) => (self.internal_target(pkg, source.as_deref(), &p, "")?, TargetMode::Internal),
            Target::External(u) => {
                if !uri_like(&u) {
                    return Err(RegistryRefusal::InvalidTarget);
                }
                (u, TargetMode::External)
            }
        };
        let rels_name = rels_part_name(source.as_deref());
        // Ids removed earlier in the session, and by this plan (including
        // those of a relationships part removed with its part), stay used.
        let mut used: Vec<String> = self
            .new_ids
            .iter()
            .chain(&self.removed_ids)
            .chain(&pkg.removed_rel_ids)
            .filter(|(r, _)| same_part_name(r, &rels_name))
            .map(|(_, id)| id.clone())
            .collect();
        if let Some(ri) = self.live(pkg, &rels_name) {
            let pos = self.edit_for(pkg, ri, rels_root_q())?;
            used.extend(self.tree(pos).root().elements().filter(|e| e.is(rel_q())).filter_map(|e| attr(e, "Id")).map(str::to_owned));
        }
        if let Some(src) = &source {
            used.extend(self.source_rid_values(pkg, src)?);
        }
        let id = next_relationship_id(used.iter().map(String::as_str)).ok_or(RegistryRefusal::Exhausted)?;
        self.push_relationship(pkg, source.as_deref(), &id, rel_type, &target, mode)?;
        Ok(id)
    }

    /// Appends one relationship (validated by the caller) to `source`'s
    /// relationships part, creating and registering that part if needed.
    fn push_relationship(&mut self, pkg: &Package, source: Option<&str>, id: &str, rel_type: &str, target: &str, mode: TargetMode) -> R<()> {
        let rels_name = rels_part_name(source);
        let mut attrs: Vec<(&str, &str)> = vec![("Id", id), ("Type", rel_type), ("Target", target)];
        if mode == TargetMode::External {
            attrs.push(("TargetMode", "External"));
        }
        match self.live(pkg, &rels_name) {
            Some(ri) => {
                let pos = self.edit_for(pkg, ri, rels_root_q())?;
                let node = element(self.tree(pos).root(), "Relationship", &attrs);
                self.edits[pos].appends.push(node);
            }
            None => {
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
                        if let Some(by) = self.name_user(pkg, &rels_name) {
                            return Err(RegistryRefusal::NameInUse { by });
                        }
                        self.register_content_type(pkg, &rels_name, RELATIONSHIPS_CONTENT_TYPE)?;
                        self.new_parts.push(NewPart { name: rels_name.clone(), content: NewContent::Rels(vec![node]) });
                    }
                }
            }
        }
        self.new_ids.push((rels_name, id.to_owned()));
        if source.is_none() {
            self.package_rels_added.push(Relationship {
                id: id.to_owned(),
                rel_type: rel_type.to_owned(),
                target: target.to_owned(),
                target_mode: mode,
            });
        }
        Ok(())
    }

    /// Where the relationship `id` of `source` is in the plan: an existing
    /// entry (edit position, child index) or one the plan adds.
    fn locate_relationship(&mut self, pkg: &Package, source: Option<&str>, id: &str) -> R<(String, RelAt)> {
        let rels_name = rels_part_name(source);
        let id = xml_trim(id);
        // (an entry the plan removed is in its edit's `removes`, skipped below)
        if let Some(ri) = self.live(pkg, &rels_name) {
            let rels_name = pkg.parts[ri].name.clone();
            let pos = self.edit_for(pkg, ri, rels_root_q())?;
            let e = &self.edits[pos];
            if let Some((i, _)) = e.tree.root().indexed_elements().find(|(i, n)| n.is(rel_q()) && attr(n, "Id") == Some(id) && !e.removes.contains(i)) {
                return Ok((rels_name, RelAt::Existing(pos, i)));
            }
            if let Some(k) = e.appends.iter().position(|n| n.is(rel_q()) && attr(n, "Id") == Some(id)) {
                return Ok((rels_name, RelAt::Appended(pos, k)));
            }
            return Err(RegistryRefusal::RelationshipNotFound);
        }
        if let Some(p) = self.new_parts.iter().position(|p| same_part_name(&p.name, &rels_name))
            && let NewContent::Rels(nodes) = &self.new_parts[p].content
            && let Some(k) = nodes.iter().position(|n| attr(n, "Id") == Some(id))
        {
            return Ok((self.new_parts[p].name.clone(), RelAt::New(p, k)));
        }
        Err(RegistryRefusal::RelationshipNotFound)
    }

    fn rel_node(&self, at: &RelAt) -> &RawNode {
        match *at {
            RelAt::Existing(pos, i) => match &self.edits[pos].tree.root().children[i] {
                RawItem::Element(e) => e,
                _ => unreachable!("located an element"),
            },
            RelAt::Appended(pos, k) => &self.edits[pos].appends[k],
            RelAt::New(p, k) => match &self.new_parts[p].content {
                NewContent::Rels(n) => &n[k],
                NewContent::Given(_) => unreachable!("located in a relationships part"),
            },
        }
    }

    /// Removes the relationship `id` from `source`'s relationships part
    /// (`None` = the package), with the whitespace before it; also one the
    /// plan added earlier. Its `Type` must be in `allowed_rel_types` (parts
    /// found by relationship **type** are referenced implicitly, which no
    /// scan can see: ADR-0001 note, review of PR #38 M5), and the source
    /// part (in the plan's view) must not refer to `id` in any attribute.
    /// The target part is not touched.
    ///
    /// Refused: `RelationshipNotFound`, `Remove(RelationshipTypeNotAllowed)`,
    /// `StillReferenced`, `SourceNotScannable`, a removed source
    /// (`ConflictsWithPlan`), a registry source.
    pub fn remove_relationship(&mut self, pkg: &Package, source: Option<&str>, id: &str, allowed_rel_types: &[&str]) -> Result<()> {
        self.step(pkg, source.unwrap_or(""), |plan| {
            let source = plan.resolve_source(pkg, source)?;
            let (rels_name, at) = plan.locate_relationship(pkg, source.as_deref(), id)?;
            let rel_type = attr(plan.rel_node(&at), "Type").unwrap_or_default().to_owned();
            let id = xml_trim(id).to_owned();
            if !allowed_rel_types.contains(&rel_type.as_str()) {
                return Err(RegistryRefusal::Remove(RemoveRefusal::RelationshipTypeNotAllowed { rels: rels_name, id, rel_type }));
            }
            if let Some(src) = &source {
                let bytes = plan.view_bytes(pkg, src).map_err(|reason| RegistryRefusal::SourceNotScannable { reason })?;
                match refers_to_rel_id(&bytes, &id) {
                    Ok(false) => {}
                    Ok(true) => return Err(RegistryRefusal::StillReferenced { source: src.clone() }),
                    Err(reason) => return Err(RegistryRefusal::SourceNotScannable { reason }),
                }
            }
            match at {
                RelAt::Existing(pos, i) => {
                    plan.edits[pos].removes.push(i);
                    plan.removed_ids.push((rels_name, id.clone()));
                }
                RelAt::Appended(pos, k) => {
                    plan.edits[pos].appends.remove(k);
                }
                RelAt::New(p, k) => {
                    if let NewContent::Rels(nodes) = &mut plan.new_parts[p].content {
                        nodes.remove(k);
                    }
                }
            }
            if source.is_none() {
                plan.package_rels_added.retain(|r| r.id != id);
            }
            Ok(())
        })
    }

    /// Rewrites the `Target` of an existing relationship of `source`
    /// (`None` = the package), or of one the plan added; `TargetMode`,
    /// `Id` and `Type` stay. For an internal relationship `target` is
    /// written as given: a fragment-only target (`#Sheet2!A1`) as it is,
    /// anything else must resolve to a part in the plan's view. For an
    /// external one it must be a URI. The one in-place change of a
    /// relationship besides removal (ADR-0009 rev 2, R2-M1: hyperlinks that
    /// follow a renamed sheet).
    ///
    /// Refused: `RelationshipNotFound`, `InvalidTarget`, `TargetNotFound`,
    /// a removed or registry source.
    pub fn set_relationship_target(&mut self, pkg: &Package, source: Option<&str>, id: &str, target: &str) -> Result<()> {
        self.step(pkg, source.unwrap_or(""), |plan| {
            let source = plan.resolve_source(pkg, source)?;
            let (_, at) = plan.locate_relationship(pkg, source.as_deref(), id)?;
            let external = attr(plan.rel_node(&at), "TargetMode") == Some("External");
            // a fragment-only target is a location (`#'My Sheet'!A1`), not a URI
            let fragment = !external && target.starts_with('#');
            let ok = if fragment { valid_attr_text(target) && !target.chars().any(char::is_control) } else { uri_like(target) };
            if !ok {
                return Err(RegistryRefusal::InvalidTarget);
            }
            // An internal target naming a part is written as `add_relationship`
            // writes it: the canonical name relative to the source (fragment
            // kept); registry parts are refused (review round 2, n3).
            let written;
            let target = if !external && let Some(p) = part_target(source.as_deref(), target) {
                let path = target.split(['#', '?']).next().unwrap_or_default();
                if path.contains('\\') {
                    return Err(RegistryRefusal::InvalidTarget);
                }
                let suffix = target.find(['#', '?']).map_or("", |k| &target[k..]);
                written = plan.internal_target(pkg, source.as_deref(), &p, suffix)?;
                written.as_str()
            } else {
                target
            };
            let set = |n: &mut RawNode| {
                if let Some(a) = n.attrs.iter_mut().find(|a| a.name.is(QName::new("", "Target"))) {
                    a.value = target.to_owned();
                }
            };
            match at {
                RelAt::Existing(pos, i) => plan.edits[pos].set_attrs.push((i, "Target", target.to_owned())),
                RelAt::Appended(pos, k) => set(&mut plan.edits[pos].appends[k]),
                RelAt::New(p, k) => {
                    if let NewContent::Rels(nodes) = &mut plan.new_parts[p].content {
                        set(&mut nodes[k]);
                    }
                }
            }
            if source.is_none() {
                let id = xml_trim(id).to_owned();
                match plan.package_rels_added.iter_mut().find(|r| r.id == id) {
                    Some(r) => r.target = target.to_owned(),
                    None => plan.package_rels_retargeted.push((id, target.to_owned())),
                }
            }
            Ok(())
        })
    }

    /// Sets a part's content type: the `ContentType` of every `Override`
    /// for it, or a new `Override` when only a `Default` covered it. A
    /// no-op when the type is already exactly `content_type`. Works for a
    /// part added earlier in the plan too. Every other entry, and every
    /// `Default`, is untouched.
    pub fn set_content_type(&mut self, pkg: &Package, name: &str, content_type: &str) -> Result<()> {
        self.step(pkg, name, |plan| {
            let name = plan.view_part(pkg, name)?;
            if is_registry_name(&name) {
                return Err(RegistryRefusal::RegistryPart);
            }
            if !valid_media_type(content_type) {
                return Err(RegistryRefusal::InvalidContentType);
            }
            if plan.set_overrides.iter().any(|(p, _)| same_part_name(p, &name)) {
                return Err(RegistryRefusal::AlreadyPlanned);
            }
            if plan.content_type(pkg, &name) == Some(content_type) {
                return Ok(());
            }
            let pos = plan.content_types_edit(pkg)?;
            if plan.pending_part(&name).is_some() {
                // a planned Override is rewritten; a Default-covered new part gets one
                if let Some(k) = plan.edits[pos].appends.iter().position(|e| {
                    e.is(QName::new(CONTENT_TYPES_NS, "Override")) && attr(e, "PartName").is_some_and(|p| same_part_name(p, &name))
                }) {
                    if let Some(a) = plan.edits[pos].appends[k].attrs.iter_mut().find(|a| a.name.is(QName::new("", "ContentType"))) {
                        a.value = content_type.to_owned();
                    }
                    for o in plan.overrides.iter_mut().filter(|(p, _)| same_part_name(p, &name)) {
                        o.1 = content_type.to_owned();
                    }
                } else {
                    let node = element(plan.tree(pos).root(), "Override", &[("PartName", &format!("/{name}")), ("ContentType", content_type)]);
                    plan.edits[pos].appends.push(node);
                    plan.overrides.push((name, content_type.to_owned()));
                }
                return Ok(());
            }
            if plan.overrides.iter().any(|(p, _)| same_part_name(p, &name)) {
                return Err(RegistryRefusal::AlreadyPlanned);
            }
            let indices: Vec<usize> = plan
                .tree(pos)
                .root()
                .indexed_elements()
                .filter(|(_, e)| e.is(QName::new(CONTENT_TYPES_NS, "Override")) && attr(e, "PartName").is_some_and(|p| same_part_name(p, &name)))
                .map(|(i, _)| i)
                .collect();
            if indices.is_empty() {
                let node = element(plan.tree(pos).root(), "Override", &[("PartName", &format!("/{name}")), ("ContentType", content_type)]);
                plan.edits[pos].appends.push(node);
                plan.overrides.push((name, content_type.to_owned()));
            } else {
                plan.edits[pos].set_attrs.extend(indices.into_iter().map(|i| (i, "ContentType", content_type.to_owned())));
                plan.set_overrides.push((name, content_type.to_owned()));
            }
            Ok(())
        })
    }

    /// Every relationship in the plan's view whose internal target is
    /// `name` (OPC equivalence; fragment-only targets name no part). Fails
    /// closed: a relationships part that cannot be read or attributed and
    /// mentions `name`'s file name is `Err(RegistryNotEditable)`, so "is it
    /// an orphan now" is never answered from a partial view (review m6).
    pub fn incoming_relationships(&self, pkg: &Package, name: &str) -> Result<Vec<IncomingRelationship>> {
        let file = file_name_of(name);
        let rels = self.view_rels(pkg, true, &file).map_err(|r| refused(name, r))?;
        Ok(rels
            .into_iter()
            .filter(|r| r.part().is_some_and(|t| same_part_name(&t, name)))
            .map(|r| IncomingRelationship {
                source: r.source.part().map(|s| self.view_part(pkg, s).unwrap_or_else(|_| s.to_owned())),
                rels: r.rels,
                id: r.id,
                rel_type: r.rel_type,
            })
            .collect())
    }
}

/// Where a relationship is in a plan.
#[derive(Clone, Copy, Debug)]
enum RelAt {
    /// `(edit position, root child index)` of an existing entry.
    Existing(usize, usize),
    /// `(edit position, index in appends)`.
    Appended(usize, usize),
    /// `(new part index, node index)` in a relationships part the plan
    /// creates.
    New(usize, usize),
}

/// The last segment of a part name, percent-decoded and case-folded.
fn file_name_of(name: &str) -> String {
    normalize_part_name(name).rsplit('/').next().unwrap_or_default().to_owned()
}

/// Does part `pi` mention `file` in its raw text (percent-decoded,
/// case-folded)? An unreadable part counts as mentioning it (fail closed;
/// review of PR #38 M3). An empty `file` matches anything.
fn mentions(pkg: &Package, pi: usize, file: &str) -> bool {
    match pkg.read_part(&pkg.parts[pi].name) {
        Ok(b) => file.is_empty() || normalize_part_name(&String::from_utf8_lossy(&b)).contains(file),
        Err(_) => true,
    }
}

/// Attribute values of the form `rId<digits>` (ASCII case-insensitive
/// prefix), in any namespace, after unescaping: `r:id`, `r:embed`, VML
/// `o:relid`, …
fn rid_like_values(xml: &[u8]) -> std::result::Result<Vec<String>, String> {
    use quick_xml::XmlVersion;
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_reader(xml);
    let mut out = Vec::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(e) | Event::Empty(e)) => {
                for a in e.attributes().with_checks(true) {
                    let a = a.map_err(|e| e.to_string())?;
                    let v = a.normalized_value(XmlVersion::Implicit1_0).map_err(|e| e.to_string())?;
                    let v = xml_trim(&v);
                    if v.split_at_checked(3).is_some_and(|(p, d)| p.eq_ignore_ascii_case("rId") && !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit())) {
                        out.push(v.to_owned());
                    }
                }
            }
            Ok(Event::DocType(_)) => return Err("DTD (<!DOCTYPE>) is not allowed".into()),
            Ok(Event::Eof) => return Ok(out),
            Ok(_) => {}
            Err(e) => return Err(format!("not well-formed XML: {e}")),
        }
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
        let package_rels = rels_part_name(None);
        for edit in plan.edits {
            if !edit.changes() {
                continue; // parsed for planning only: leave the part as it was
            }
            let part = &mut self.parts[edit.part];
            let mut doc = Arc::unwrap_or_clone(edit.tree);
            {
                let root = doc.root_mut();
                for (i, a, v) in &edit.set_attrs {
                    if let Some(RawItem::Element(e)) = root.children.get_mut(*i)
                        && let Some(at) = e.attrs.iter_mut().find(|x| x.name.is(QName::new("", a)))
                    {
                        at.value.clone_from(v);
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
                append_root_child(&mut doc, node);
            }
            part.state = PartState::Parsed(doc);
        }
        for s in plan.staged {
            let p = &mut self.parts[s.part];
            p.state = match Arc::unwrap_or_clone(s.content) {
                PartContent::Bytes(b) => PartState::Replaced(b),
                PartContent::Xml(d) => {
                    p.detached |= s.detach;
                    PartState::Parsed(d)
                }
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
        self.removed_rel_ids.extend(plan.removed_ids.iter().cloned());
        for p in plan.new_parts {
            let state = match p.content {
                NewContent::Given(c) => match Arc::unwrap_or_clone(c) {
                    PartContent::Bytes(b) => PartState::Replaced(b),
                    PartContent::Xml(d) => PartState::Parsed(d),
                },
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
            self.parts.push(Part { name: p.name, index: 0, detached: true, compression: CompressionMethod::Deflated, state });
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
        let removed_package_ids: Vec<String> =
            plan.removed_ids.iter().filter(|(r, _)| same_part_name(r, &package_rels)).map(|(_, i)| i.clone()).collect();
        if !removed_package_ids.is_empty() {
            self.package_rels = self.package_rels.without_ids(&removed_package_ids);
        }
        for r in plan.package_rels_added {
            self.package_rels.push(r);
        }
        for (id, t) in plan.package_rels_retargeted {
            self.package_rels.set_target(&id, &t);
        }
        self.touch();
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
    pub fn remove_relationship(&mut self, source: Option<&str>, id: &str, allowed_rel_types: &[&str]) -> Result<()> {
        let mut plan = RegistryPlan::new(self);
        plan.remove_relationship(self, source, id, allowed_rel_types)?;
        self.apply(plan)
    }

    /// The validation of [`Self::remove_relationship`].
    pub fn check_remove_relationship(&self, source: Option<&str>, id: &str, allowed_rel_types: &[&str]) -> Result<()> {
        RegistryPlan::new(self).remove_relationship(self, source, id, allowed_rel_types)
    }

    /// Rewrites one relationship's target ([`RegistryPlan::set_relationship_target`]).
    pub fn set_relationship_target(&mut self, source: Option<&str>, id: &str, target: &str) -> Result<()> {
        let mut plan = RegistryPlan::new(self);
        plan.set_relationship_target(self, source, id, target)?;
        self.apply(plan)
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
        RegistryPlan::new(self).name_user(self, name.trim_start_matches('/'))
    }

    /// Every internal relationship target in the package, resolved
    /// (dangling ones included; fragment-only targets name no part);
    /// unreadable relationships parts are skipped (names only).
    fn relationship_targets(&self) -> Vec<String> {
        RegistryPlan::new(self).view_rels(self, false, "").unwrap_or_default().iter().filter_map(ViewRel::part).collect()
    }

    /// Every relationship whose internal target is `name`
    /// ([`RegistryPlan::incoming_relationships`] on an empty plan). Fails
    /// closed on unreadable relationships parts that mention the name.
    pub fn incoming_relationships(&self, name: &str) -> Result<Vec<IncomingRelationship>> {
        RegistryPlan::new(self).incoming_relationships(self, name)
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
    /// `Id` in any attribute (any namespace, after XML unescaping; staged
    /// content is what is scanned); the relationship is removed. A
    /// relationship whose source is in the set goes with the source's
    /// relationships part, unchecked. Every `Override` for a removed part,
    /// or for its relationships part, is removed; `Default`s never are.
    /// Returns what goes, and the internal targets the removed parts pointed
    /// at that stay (`kept_targets`; fragment-only targets name no part), so
    /// the caller can decide whether they are orphans now
    /// ([`Self::incoming_relationships`]).
    ///
    /// Fails closed: a relationships part that cannot be read, parsed or
    /// attributed (percent-encoded `_rels`) and that mentions a removed
    /// part's file name refuses (`Remove(RegistryNotEditable)`); so does a
    /// relationship spelled with a removed part's file name that resolves to
    /// no part (`Remove(UnresolvedMention)`).
    ///
    /// Refusals ([`Error::RegistryRefused`]; the plan is unchanged):
    /// `PartNotFound` (also for a part added in this plan or already
    /// removed), `RegistryPart`, `ConflictsWithPlan` (a relationship this
    /// plan adds to or from a removed part, staged content for one, or a
    /// content-type change of one), or `Remove(…)` with PR #14's reasons:
    /// `RelationshipTypeNotAllowed`, `StillReferenced`,
    /// `SourceNotScannable`, `UnresolvedMention`, `RegistryNotEditable`.
    pub fn remove_parts(&mut self, pkg: &Package, names: &[&str], allowed_rel_types: &[&str]) -> Result<RemovedParts> {
        let label = names.first().copied().unwrap_or("");
        self.step(pkg, label, |plan| plan.removal(pkg, names, allowed_rel_types))
    }

    fn removal(&mut self, pkg: &Package, names: &[&str], allowed: &[&str]) -> R<RemovedParts> {
        use RegistryRefusal as Rf;
        let mut idx: Vec<usize> = Vec::new();
        for n in names {
            let i = self.live(pkg, n).ok_or(Rf::PartNotFound)?;
            if is_registry_name(&pkg.parts[i].name) {
                return Err(Rf::RegistryPart);
            }
            if !idx.contains(&i) {
                idx.push(i);
            }
        }
        let set_names: Vec<String> = idx.iter().map(|&i| pkg.parts[i].name.clone()).collect();
        let in_set = |n: &str| set_names.iter().any(|s| same_part_name(s, n));
        // The plan must not use a removed part.
        if self.staged.iter().any(|s| idx.contains(&s.part)) {
            return Err(Rf::ConflictsWithPlan);
        }
        if self.overrides.iter().chain(&self.set_overrides).any(|(p, _)| in_set(p)) {
            return Err(Rf::ConflictsWithPlan);
        }
        for p in &self.new_parts {
            if let NewContent::Rels(nodes) = &p.content
                && let Some(src) = source_of_rels_part(&p.name)
            {
                if src.part().is_some_and(in_set) {
                    return Err(Rf::ConflictsWithPlan);
                }
                if nodes.iter().map(|n| ViewRel::from_node(&p.name, src.clone(), n)).any(|r| r.part().is_some_and(|t| in_set(&t))) {
                    return Err(Rf::ConflictsWithPlan);
                }
            }
        }
        for e in &self.edits {
            if !pkg.parts[e.part].name.eq_ignore_ascii_case(CONTENT_TYPES_PART)
                && let Some(RelsKind::Of(src)) = rels_kind(&pkg.parts[e.part].name)
            {
                let added = e.appends.iter().map(|n| ViewRel::from_node(&pkg.parts[e.part].name, src.clone(), n));
                if added.clone().any(|r| r.part().is_some_and(|t| in_set(&t))) || (!e.appends.is_empty() && src.part().is_some_and(in_set)) {
                    return Err(Rf::ConflictsWithPlan);
                }
            }
        }
        // Own relationships parts go with their parts.
        let mut gone = idx.clone();
        let mut gone_ids: Vec<(String, String)> = Vec::new();
        let mut kept_targets = Vec::new();
        for &i in &idx {
            let name = pkg.parts[i].name.clone();
            let Some(ri) = self.live(pkg, &rels_part_name(Some(&name))) else { continue };
            if self.edits.iter().any(|e| e.part == ri && e.changes()) {
                return Err(Rf::ConflictsWithPlan);
            }
            let not_editable = |reason: String| Rf::Remove(RemoveRefusal::RegistryNotEditable { part: pkg.parts[ri].name.clone(), reason });
            let src = match rels_kind(&pkg.parts[ri].name) {
                Some(RelsKind::Of(s)) => s,
                _ => return Err(not_editable("cannot be attributed".into())),
            };
            for r in self.view_rels_of(pkg, ri, &src).map_err(not_editable)? {
                if let Some(t) = r.part()
                    && !in_set(&t)
                {
                    kept_targets.push((name.clone(), t, r.rel_type.clone()));
                }
                // The Ids of a relationships part that goes stay used: a part
                // re-created with the same name continues above them, and
                // content staged for it may not name them (review round 2, n4).
                gone_ids.push((r.rels.clone(), r.id.clone()));
            }
            gone.push(ri);
        }
        let gone_names: Vec<String> = gone.iter().map(|&i| pkg.parts[i].name.clone()).collect();
        let prior_gone = self.gone_names.clone();
        let is_gone = |n: &str| gone_names.iter().chain(&prior_gone).any(|g| same_part_name(g, n));
        let file_names: Vec<String> = set_names.iter().map(|n| file_name_of(n)).collect();
        let mentions_any = |pi: usize| file_names.iter().any(|f| !f.is_empty() && mentions(pkg, pi, f));
        let mut relationships = Vec::new();
        for (pi, kind) in pkg.rels_parts() {
            if gone.contains(&pi) || self.gone.contains(&pi) {
                continue;
            }
            let rels_name = pkg.parts[pi].name.clone();
            let src = match kind {
                RelsKind::Of(s) => s,
                RelsKind::Unattributable => {
                    if mentions_any(pi) {
                        return Err(Rf::Remove(RemoveRefusal::RegistryNotEditable {
                            part: rels_name,
                            reason: "an invalid relationships part name (percent-encoded `_rels`/`.rels`) that mentions the part".into(),
                        }));
                    }
                    continue;
                }
            };
            let pos = match self.edit_for(pkg, pi, rels_root_q()) {
                Ok(pos) => pos,
                Err(Rf::RegistryNotEditable { part, reason }) => {
                    if mentions_any(pi) {
                        return Err(Rf::Remove(RemoveRefusal::RegistryNotEditable { part, reason }));
                    }
                    continue;
                }
                Err(e) => return Err(e),
            };
            let base = src.base();
            let mut indices = Vec::new();
            let mut ids = Vec::new();
            let edit = &self.edits[pos];
            for (i, e) in edit.tree.root().indexed_elements().filter(|(_, e)| e.is(rel_q())) {
                if edit.removes.contains(&i) || attr(e, "TargetMode") == Some("External") {
                    continue; // removed earlier in the plan, or external
                }
                // the target in the plan's view: a `set_relationship_target`
                // earlier in the plan wins (review round 2, N2)
                let retarget = edit.set_attrs.iter().rev().find(|(j, a, _)| *j == i && *a == "Target").map(|(_, _, t)| t.as_str());
                let Some(target) = retarget.or_else(|| attr(e, "Target")) else { continue };
                let Some(resolved) = part_target(base.as_deref(), target) else { continue };
                if !in_set(&resolved) {
                    // Safety net (PR #14 N7, N12, N13): a target whose last
                    // segment (unescaped, percent-decoded, case-folded, `\`
                    // counted as a separator) is a removed part's file name
                    // but that resolves to no part might mean that part.
                    let seg = normalize_part_name(target.split(['#', '?']).next().unwrap_or_default());
                    let seg = seg.rsplit(['/', '\\']).next().unwrap_or_default();
                    if file_names.iter().any(|f| !f.is_empty() && f == seg) && pkg.find_ci(&resolved).is_none() && self.pending_part(&resolved).is_none() {
                        return Err(Rf::Remove(RemoveRefusal::UnresolvedMention { rels: rels_name }));
                    }
                    continue;
                }
                let id = attr(e, "Id").unwrap_or_default().to_owned();
                let rel_type = attr(e, "Type").unwrap_or_default();
                if !allowed.contains(&rel_type) {
                    return Err(Rf::Remove(RemoveRefusal::RelationshipTypeNotAllowed { rels: rels_name, id, rel_type: rel_type.to_owned() }));
                }
                indices.push(i);
                ids.push(id);
            }
            if indices.is_empty() {
                continue;
            }
            // The source stays: a source in the set (or removed earlier) has its
            // own relationships part in `gone`, which the loop skips.
            if let Some(s) = src.part()
                && let Some(si) = pkg.find_ci(s)
            {
                let src_name = pkg.parts[si].name.clone();
                let bytes = self
                    .view_bytes(pkg, &src_name)
                    .map_err(|reason| Rf::Remove(RemoveRefusal::SourceNotScannable { source: src_name.clone(), reason }))?;
                for id in &ids {
                    match refers_to_rel_id(&bytes, id) {
                        Ok(false) => {}
                        Ok(true) => return Err(Rf::Remove(RemoveRefusal::StillReferenced { source: src_name, id: id.clone() })),
                        Err(reason) => return Err(Rf::Remove(RemoveRefusal::SourceNotScannable { source: src_name, reason })),
                    }
                }
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
                Rf::RegistryNotEditable { part, reason } => Rf::Remove(RemoveRefusal::RegistryNotEditable { part, reason }),
                e => e,
            })?;
            let removes = self.edits[pos].removes.clone();
            let ix: Vec<usize> = self
                .tree(pos)
                .root()
                .indexed_elements()
                .filter(|(i, e)| {
                    !removes.contains(i)
                        && e.is(QName::new(CONTENT_TYPES_NS, "Override"))
                        && attr(e, "PartName").is_some_and(|p| gone_names.iter().any(|g| same_part_name(g, p)))
                })
                .map(|(i, _)| i)
                .collect();
            overrides = ix.len();
            self.edits[pos].removes.extend(ix);
        }
        self.gone.extend(&gone);
        self.gone_names.extend(gone_names.iter().cloned());
        self.removed_ids.extend(gone_ids);
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
    /// does not make a plan stale; `xml_part_mut` does, for every plan, so
    /// **every** content edit that belongs to a plan goes through a clone
    /// and `stage_part`), edit `doc` through a lens, then
    /// `plan.stage_part(&pkg, name, doc)`. A tree cloned from the parsed
    /// part keeps its source spans, so untouched subtrees are still written
    /// byte-for-byte. Any other tree (built from a template, parsed from
    /// other bytes) is what is saved, whether or not it is "dirty" (review
    /// M2). Bytes replace the part wholesale (`Replaced`); XML content types
    /// must be well-formed. Staging a part twice keeps the later content.
    ///
    /// Later steps see the staged content: the reference scans of
    /// `remove_relationship` and `remove_parts`, and the `Id` allocation of
    /// `add_relationship`, read it. Refused: a missing, removed or registry
    /// part (`PartNotFound`, `ConflictsWithPlan`, `RegistryPart`), malformed
    /// XML, or content that refers (in any attribute) to a relationship `Id`
    /// this plan removed from the part's relationships (`ConflictsWithPlan`).
    pub fn stage_part(&mut self, pkg: &Package, name: &str, content: impl Into<PartContent>) -> Result<()> {
        let content = content.into();
        self.step(pkg, name, |plan| {
            let bytes = match &content {
                PartContent::Bytes(b) => b.clone(),
                PartContent::Xml(d) => d.serialize().into_bytes(),
            };
            let canonical = plan.view_part(pkg, name)?;
            if is_registry_name(&canonical) {
                return Err(RegistryRefusal::RegistryPart);
            }
            if let PartContent::Bytes(b) = &content
                && plan.content_type(pkg, &canonical).is_some_and(is_xml_media_type)
            {
                XmlDocument::parse_with(b, &pkg.parse_options).map_err(|e| RegistryRefusal::MalformedXml { reason: e.to_string() })?;
            }
            let rels = rels_part_name(Some(&canonical));
            for (r, id) in &plan.removed_ids {
                if same_part_name(r, &rels) && refers_to_rel_id(&bytes, id).unwrap_or(true) {
                    return Err(RegistryRefusal::ConflictsWithPlan);
                }
            }
            match plan.live(pkg, &canonical) {
                Some(i) => {
                    // Not detached only when the tree is exactly the part's
                    // ORIGINAL zip entry, because an un-detached, clean tree
                    // saves that entry (review round 2, N1: comparing with the
                    // current content let a re-parse of a `set_part` or
                    // `xml_part_mut` change pass as unedited).
                    let detach = match &content {
                        PartContent::Bytes(_) => false,
                        PartContent::Xml(d) => {
                            d.is_dirty() || pkg.read_original(&pkg.parts[i]).map_or(true, |o| o != d.source().as_bytes())
                        }
                    };
                    plan.staged.retain(|s| s.part != i);
                    plan.staged.push(Staged { part: i, content: Arc::new(content), detach });
                }
                None => {
                    let p = plan.new_parts.iter_mut().find(|p| same_part_name(&p.name, &canonical)).expect("pending");
                    if !matches!(p.content, NewContent::Given(_)) {
                        return Err(RegistryRefusal::RegistryPart);
                    }
                    p.content = NewContent::Given(Arc::new(content));
                }
            }
            Ok(())
        })
    }

    /// Gives `dst` the relationships `src` has **in the plan's view** (its
    /// relationships part minus what the plan removes, plus what it adds;
    /// review M4), **with the same `Id`s**, so a duplicated part (a slide
    /// copied as XML) keeps resolving its `r:id`s. Relationships whose type
    /// is in `skip_types` are not copied (a duplicated slide without its
    /// comments; ADR-0012 N1). `dst` must have no relationships part yet
    /// (existing or planned); it is typically a part added earlier in the
    /// plan. Returns the copied `Id`s.
    ///
    /// Targets (review M6):
    /// - external: copied as they are;
    /// - internal, naming a part in the plan's view (after `target_map`,
    ///   e.g. a duplicated notes slide): written relative to `dst`, keeping
    ///   a `#fragment`;
    /// - internal and fragment-only (`#_ftn1`), or naming no part (dangling,
    ///   unresolvable spelling): copied **verbatim**, which keeps its meaning
    ///   only when `dst` is in the same folder as `src`; otherwise refused
    ///   (`TargetNotFound`). A `target_map` entry that maps to no part is
    ///   refused too.
    ///
    /// Refused also: a missing or removed `src`/`dst`, a registry part,
    /// `dst` already having relationships, a `src` relationships part that
    /// cannot be read.
    pub fn copy_relationships(
        &mut self,
        pkg: &Package,
        src: &str,
        dst: &str,
        target_map: &[(&str, &str)],
        skip_types: &[&str],
    ) -> Result<Vec<String>> {
        self.step(pkg, dst, |plan| {
            let src = plan.resolve_source(pkg, Some(src))?.expect("Some");
            let dst = plan.resolve_source(pkg, Some(dst))?.expect("Some");
            let dst_rels = rels_part_name(Some(&dst));
            if plan.live(pkg, &dst_rels).is_some() || plan.pending_part(&dst_rels).is_some() {
                return Err(RegistryRefusal::NameInUse { by: format!("{dst_rels:?} (dst already has relationships)") });
            }
            let src_rels = rels_part_name(Some(&src));
            let rels: Vec<ViewRel> = match plan.live(pkg, &src_rels) {
                Some(ri) => {
                    let not_editable = |reason: String| RegistryRefusal::RegistryNotEditable { part: pkg.parts[ri].name.clone(), reason };
                    match rels_kind(&pkg.parts[ri].name) {
                        Some(RelsKind::Of(s)) => plan.view_rels_of(pkg, ri, &s).map_err(not_editable)?,
                        _ => return Err(not_editable("cannot be attributed".into())),
                    }
                }
                None => match plan.new_parts.iter().find(|p| same_part_name(&p.name, &src_rels)) {
                    Some(NewPart { name, content: NewContent::Rels(nodes) }) => {
                        let s = source_of_rels_part(name).expect("created for a source");
                        nodes.iter().map(|n| ViewRel::from_node(name, s.clone(), n)).collect()
                    }
                    _ => Vec::new(),
                },
            };
            let same_folder = same_part_name(dir_of(&src), dir_of(&dst));
            let mut ids = Vec::new();
            for r in rels {
                if skip_types.contains(&r.rel_type.as_str()) {
                    continue;
                }
                let (target, mode) = if r.external {
                    (r.target.clone(), TargetMode::External)
                } else {
                    let verbatim = || -> R<String> {
                        if same_folder {
                            Ok(r.target.clone())
                        } else {
                            let resolved = part_target(Some(&src), &r.target).unwrap_or_else(|| r.target.clone());
                            Err(RegistryRefusal::TargetNotFound { resolved })
                        }
                    };
                    match part_target(Some(&src), &r.target) {
                        None => (verbatim()?, TargetMode::Internal),
                        Some(resolved) => {
                            let suffix = r.target.find(['#', '?']).map_or("", |k| &r.target[k..]);
                            let mapped = target_map.iter().find(|(from, _)| same_part_name(from, &resolved)).map(|(_, to)| (*to).to_owned());
                            let dest = mapped.clone().unwrap_or(resolved);
                            match plan.internal_target(pkg, Some(&dst), &dest, suffix) {
                                Ok(t) => (t, TargetMode::Internal),
                                Err(RegistryRefusal::TargetNotFound { .. } | RegistryRefusal::ConflictsWithPlan) if mapped.is_none() => {
                                    (verbatim()?, TargetMode::Internal)
                                }
                                Err(RegistryRefusal::InvalidTarget) if mapped.is_none() => (verbatim()?, TargetMode::Internal),
                                Err(e) => return Err(e),
                            }
                        }
                    }
                };
                plan.push_relationship(pkg, Some(&dst), &r.id, &r.rel_type, &target, mode)?;
                ids.push(r.id);
            }
            Ok(ids)
        })
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
