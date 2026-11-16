//! Layer 1: OPC part-level passthrough (ADR-0001).
//!
//! Every part is in one of three states:
//!
//! - [`PartState::Raw`] — never parsed; written with
//!   `ZipWriter::raw_copy_file` (name, compression method, CRC-32 and
//!   compressed bytes unchanged).
//! - [`PartState::Parsed`] — parsed into an [`XmlDocument`]. While the tree
//!   has no dirty node it is *also* raw-copied: parsing for inspection never
//!   changes the output.
//! - [`PartState::Replaced`] — new bytes from [`Package::set_part`]; written
//!   fresh. A `Parsed` part with dirty nodes is re-serialized by the Layer 2
//!   rules and written fresh as well.
//!
//! [`Package::open`] reads the whole file into memory, and [`Package::save`]
//! writes a temporary file in the destination directory and renames it into
//! place, so saving over the source path is safe and a crash never leaves a
//! half-written file behind.

use std::fs::{self, File};
use std::io::{BufWriter, Cursor, Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::error::{Error, Result};
use crate::name::STRICT_NS_PREFIX;
use crate::opc::{CONTENT_TYPES_PART, ContentTypes, REL_OFFICE_DOCUMENT, Relationships, normalize_part_name, rels_part_name, same_part_name, xml_trim};
use crate::xml::{ParseOptions, XmlDocument};

mod add;
pub use add::{
    IncomingRelationship, PartContent, PartNameRequest, RELATIONSHIPS_CONTENT_TYPE, RegistryPlan, RegistryRefusal, RemovedParts, Target,
};

/// `[Content_Types].xml` or a relationships part.
fn is_opc_metadata(name: &str) -> bool {
    name == CONTENT_TYPES_PART || name.ends_with(".rels")
}

/// OLE/CFB compound file signature (encrypted or legacy binary Office files).
const CFB_MAGIC: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

/// The state of one part.
#[derive(Debug)]
pub enum PartState {
    /// Never parsed; raw-copied on save.
    Raw,
    /// Parsed; raw-copied on save unless the tree is dirty.
    Parsed(XmlDocument),
    /// Replaced wholesale; written fresh on save.
    Replaced(Vec<u8>),
}

/// What [`Package::remove_part`] removed besides the part itself.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RemovedPart {
    /// `(relationships part, relationship Id)` of every relationship that
    /// pointed at the part, in package order.
    pub relationships: Vec<(String, String)>,
    /// Whether an `Override` for the part was removed from
    /// `[Content_Types].xml`.
    pub content_type_override: bool,
}

/// Why [`Package::remove_part`] refused (nothing was changed).
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum RemoveRefusal {
    /// `[Content_Types].xml` or a relationships part.
    RegistryPart,
    /// The part has a relationships part of its own.
    HasOwnRelationships {
        /// That relationships part.
        rels: String,
    },
    /// A relationship to the part has a type the caller did not allow (the
    /// part may be referenced implicitly by that type).
    RelationshipTypeNotAllowed {
        /// The relationships part.
        rels: String,
        /// The relationship `Id`.
        id: String,
        /// Its `Type`.
        rel_type: String,
    },
    /// The source part still refers to a relationship that would be removed.
    StillReferenced {
        /// The source part.
        source: String,
        /// The relationship `Id`.
        id: String,
    },
    /// The source part could not be scanned for references.
    SourceNotScannable {
        /// The source part.
        source: String,
        /// Why.
        reason: String,
    },
    /// A relationship's target has the part's file name as its last segment
    /// (unescaped, percent-decoded, `\` counted as a separator) but resolves
    /// to no existing part: a spelling this code does not resolve might still
    /// point at the part (safety net; refusing is the safe side). An
    /// *unreadable* relationships part that mentions the file name refuses
    /// with [`RemoveRefusal::RegistryNotEditable`] instead.
    UnresolvedMention {
        /// The relationships part.
        rels: String,
    },
    /// A registry part that must change cannot be edited in place.
    RegistryNotEditable {
        /// The registry part.
        part: String,
        /// Why (replaced wholesale, not well-formed, wrong root).
        reason: String,
    },
}

impl std::fmt::Display for RemoveRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RemoveRefusal::RegistryPart => write!(f, "registry parts cannot be removed"),
            RemoveRefusal::HasOwnRelationships { rels } => {
                write!(f, "the part has relationships of its own ({rels}); cascading removal is not supported")
            }
            RemoveRefusal::RelationshipTypeNotAllowed { rels, id, rel_type } => {
                write!(f, "relationship {id} in {rels} has type {rel_type}, which the caller did not allow")
            }
            RemoveRefusal::StillReferenced { source, id } => write!(f, "{source} still refers to relationship {id}"),
            RemoveRefusal::SourceNotScannable { source, reason } => write!(f, "cannot scan {source} for references: {reason}"),
            RemoveRefusal::UnresolvedMention { rels } => {
                write!(f, "a relationship in {rels} has the part's file name but resolves to no part")
            }
            RemoveRefusal::RegistryNotEditable { part, reason } => write!(f, "{part} cannot be edited in place: {reason}"),
        }
    }
}

/// [`PartState`] without the payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartStateKind {
    /// See [`PartState::Raw`].
    Raw,
    /// See [`PartState::Parsed`]; `dirty` = has edited nodes.
    Parsed {
        /// The tree has at least one dirty node.
        dirty: bool,
    },
    /// See [`PartState::Replaced`].
    Replaced,
}

#[derive(Debug)]
struct Part {
    name: String,
    /// Index of the original zip entry (meaningless for an added part).
    index: usize,
    /// The content is not the original entry's: an added part, or an
    /// existing part given a new tree by a registry plan (`stage_part`,
    /// review of PR #38 M2). It is always written fresh, whatever its state,
    /// and `read_part` serializes it.
    detached: bool,
    compression: CompressionMethod,
    state: PartState,
}

/// An OPC package (zip of parts) opened for lossless editing.
#[derive(Debug)]
pub struct Package {
    archive: ZipArchive<Cursor<Arc<[u8]>>>,
    parts: Vec<Part>,
    content_types: ContentTypes,
    package_rels: Relationships,
    parse_options: ParseOptions,
    /// Process-unique id, so a [`RegistryPlan`] is applied only to the
    /// package it was planned against.
    id: u64,
    /// Bumped by every change; a plan made before a change is stale.
    generation: u64,
    /// Parts removed in this session (never reused by numbered names).
    removed_names: Vec<String>,
    /// `(relationships part, Id)` removed in this session: a new `Id` stays
    /// above them (session high-water; ADR-0009 review m2).
    removed_rel_ids: Vec<(String, String)>,
}

impl Package {
    /// Opens a package file. The whole file is read into memory first.
    ///
    /// Errors: [`Error::EncryptedPackage`] for an OLE/CFB file,
    /// [`Error::StrictNotSupported`] for ECMA-376 Strict packages,
    /// [`Error::Package`] `"duplicate part name …"` when two zip entries
    /// name the same OPC part (equal after ASCII case folding and
    /// percent-decoding; an invalid package, ADR-0001 Layer 1 note).
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::from_bytes(fs::read(path)?)
    }

    /// Opens a package from bytes.
    pub fn from_bytes(data: impl Into<Arc<[u8]>>) -> Result<Self> {
        Self::from_bytes_with(data, ParseOptions::default())
    }

    /// [`Self::open`] with explicit XML [`ParseOptions`].
    pub fn open_with(path: impl AsRef<Path>, options: ParseOptions) -> Result<Self> {
        Self::from_bytes_with(fs::read(path)?, options)
    }

    /// [`Self::from_bytes`] with explicit XML [`ParseOptions`], used for
    /// the parts parsed through [`Self::xml_part`] / [`Self::xml_part_mut`].
    ///
    /// OPC metadata (`[Content_Types].xml` and `.rels` parts) is never
    /// parsed with a limit below [`DEFAULT_MAX_DEPTH`](crate::xml::DEFAULT_MAX_DEPTH):
    /// the open-time readers and [`Self::relationships`] use
    /// [`ParseOptions::default`], and `xml_part` on a metadata part uses
    /// the larger of the default and `options.max_depth`. A lowered limit
    /// therefore never turns valid package metadata into an error.
    pub fn from_bytes_with(data: impl Into<Arc<[u8]>>, options: ParseOptions) -> Result<Self> {
        let data: Arc<[u8]> = data.into();
        if data.starts_with(&CFB_MAGIC) {
            return Err(Error::EncryptedPackage);
        }
        let len = data.len() as u64;
        let mut archive = ZipArchive::new(Cursor::new(data))?;
        let mut parts = Vec::with_capacity(archive.len());
        let mut declared: u64 = 0;
        // OPC part-name equivalence (ASCII case-insensitive, an IRI entry
        // name equals its percent-encoded form; review N8): each name is
        // normalized once, so the check is linear (review N11).
        let mut seen = std::collections::HashSet::with_capacity(archive.len());
        for index in 0..archive.len() {
            let file = archive.by_index_raw(index)?;
            let name = file.name()?.into_owned();
            // Raw copy writes `compressed_size` bytes from `data_start`: data
            // that is not all inside the archive would come out different
            // (fuzz finding F-2), so such an entry is refused here.
            let end = file.data_start().map_or(u64::MAX, |start| start.saturating_add(file.compressed_size()));
            if end > len {
                return Err(Error::TruncatedEntry { part: name, end, len });
            }
            declared = declared.saturating_add(file.size());
            if !seen.insert(normalize_part_name(&name)) {
                return Err(Error::Package(format!("duplicate part name {name:?}")));
            }
            parts.push(Part { name, index, detached: false, compression: file.compression(), state: PartState::Raw });
        }
        if declared > options.max_package_bytes {
            return Err(Error::PackageTooLarge { size: declared, limit: options.max_package_bytes });
        }
        let mut pkg = Package {
            archive,
            parts,
            content_types: ContentTypes::default(),
            package_rels: Relationships::empty(None),
            parse_options: options,
            id: add::new_package_id(),
            generation: 0,
            removed_names: Vec::new(),
            removed_rel_ids: Vec::new(),
        };
        pkg.content_types = ContentTypes::parse(&pkg.read_part(CONTENT_TYPES_PART)?)?;
        // matched as the registry matches it (OPC names are case-insensitive;
        // PR #38 review round 3, n13)
        if let Some(i) = pkg.find_ci(&rels_part_name(None)) {
            let rels_name = pkg.parts[i].name.clone();
            pkg.package_rels = Relationships::parse(None, &pkg.read_part(&rels_name)?)?;
        }
        if let Some(strict) = pkg.package_rels.iter().find(|r| r.rel_type.starts_with(STRICT_NS_PREFIX)) {
            return Err(Error::StrictNotSupported(strict.rel_type.clone()));
        }
        Ok(pkg)
    }

    fn find(&self, name: &str) -> Option<usize> {
        self.parts.iter().position(|p| p.name == name)
    }

    fn get(&self, name: &str) -> Result<&Part> {
        self.find(name).map(|i| &self.parts[i]).ok_or_else(|| Error::PartNotFound(name.into()))
    }

    /// The XML parse options this package uses.
    pub fn parse_options(&self) -> ParseOptions {
        self.parse_options
    }

    /// Part (zip entry) names in original order.
    pub fn part_names(&self) -> impl Iterator<Item = &str> {
        self.parts.iter().map(|p| p.name.as_str())
    }

    /// Does the package have this part?
    pub fn contains(&self, name: &str) -> bool {
        self.find(name).is_some()
    }

    /// Current state of a part.
    pub fn state(&self, name: &str) -> Option<PartStateKind> {
        self.find(name).map(|i| match &self.parts[i].state {
            PartState::Raw => PartStateKind::Raw,
            PartState::Parsed(d) => PartStateKind::Parsed { dirty: d.is_dirty() },
            PartState::Replaced(_) => PartStateKind::Replaced,
        })
    }

    /// Decompresses a part's original bytes, within
    /// [`ParseOptions::max_part_bytes`]. The declared size only decides the
    /// refusal, never the allocation (fuzz finding F-6: a 10-byte entry can
    /// declare 4 GB); the buffer grows with the data actually read.
    fn read_original(&self, part: &Part) -> Result<Vec<u8>> {
        /// First allocation; `read_to_end` grows the buffer from there.
        const INITIAL: u64 = 64 * 1024;
        let limit = self.parse_options.max_part_bytes;
        let too_large = || Error::PartTooLarge { part: part.name.clone(), limit };
        let mut archive = self.archive.clone();
        let file = archive.by_index(part.index)?;
        let declared = file.size();
        if declared > limit {
            return Err(too_large());
        }
        let mut buf = Vec::with_capacity(declared.min(INITIAL) as usize);
        file.take(limit.saturating_add(1)).read_to_end(&mut buf)?;
        if buf.len() as u64 > limit {
            return Err(too_large());
        }
        Ok(buf)
    }

    /// The current (decompressed) content of a part: original bytes, the
    /// serialized tree, or the replacement bytes.
    pub fn read_part(&self, name: &str) -> Result<Vec<u8>> {
        let part = self.get(name)?;
        match &part.state {
            PartState::Raw => self.read_original(part),
            PartState::Parsed(d) if !d.is_dirty() && !part.detached => self.read_original(part),
            PartState::Parsed(d) => Ok(d.serialize().into_bytes()),
            PartState::Replaced(b) => Ok(b.clone()),
        }
    }

    fn ensure_parsed(&mut self, name: &str) -> Result<usize> {
        let i = self.find(name).ok_or_else(|| Error::PartNotFound(name.into()))?;
        match self.parts[i].state {
            PartState::Parsed(_) => {}
            PartState::Raw => {
                let mut options = self.parse_options;
                if is_opc_metadata(name) {
                    options.max_depth = options.max_depth.max(crate::xml::DEFAULT_MAX_DEPTH);
                }
                let doc = XmlDocument::parse_with(&self.read_original(&self.parts[i])?, &options)?;
                self.parts[i].state = PartState::Parsed(doc);
            }
            PartState::Replaced(_) => {
                return Err(Error::Package(format!("part {name:?} was replaced and cannot be parsed in place")));
            }
        }
        Ok(i)
    }

    /// Parses a part (once) and returns its tree. Parsing alone never changes
    /// the saved output.
    pub fn xml_part(&mut self, name: &str) -> Result<&XmlDocument> {
        let i = self.ensure_parsed(name)?;
        match &self.parts[i].state {
            PartState::Parsed(d) => Ok(d),
            _ => unreachable!(),
        }
    }

    /// The tree of a part that [`Self::xml_part`] or [`Self::xml_part_mut`]
    /// already parsed, without parsing: `None` if the part is missing, still
    /// `Raw`, or `Replaced`. Lets a format crate parse its parts once (at
    /// open) and then serve reads through `&self`.
    pub fn parsed_part(&self, name: &str) -> Option<&XmlDocument> {
        match &self.parts[self.find(name)?].state {
            PartState::Parsed(d) => Some(d),
            _ => None,
        }
    }

    /// Parses a part (once) and returns its tree for editing through
    /// [`XmlDocument::lens_mut`].
    pub fn xml_part_mut(&mut self, name: &str) -> Result<&mut XmlDocument> {
        let i = self.ensure_parsed(name)?;
        // The caller may edit the tree: a registry plan made before is stale.
        self.touch();
        match &mut self.parts[i].state {
            PartState::Parsed(d) => Ok(d),
            _ => unreachable!(),
        }
    }

    /// Replaces an existing part's bytes wholesale. A name that is not a part
    /// is [`Error::NewPartUnsupported`]: new parts go through
    /// [`Self::add_part`], which registers the content type as well.
    pub fn set_part(&mut self, name: &str, bytes: impl Into<Vec<u8>>) -> Result<()> {
        let i = self.find(name).ok_or_else(|| Error::NewPartUnsupported(name.into()))?;
        self.parts[i].state = PartState::Replaced(bytes.into());
        self.touch();
        Ok(())
    }

    /// Removes a part together with its registrations: every internal
    /// relationship (in any relationships part) that resolves to it, and every
    /// `Override` for it in `[Content_Types].xml`. A `Default` is never
    /// touched.
    ///
    /// This is the narrow exception to ADR-0001 Layer 3's append-only
    /// registries (ADR-0001 interim note of 2026-11-16, ADR-0003 §4.3): an
    /// entry is removed only together with the part it registers, and no
    /// remaining entry is renumbered. Each removed element goes together with
    /// the whitespace-only text immediately before it, so indented registry
    /// parts stay tidy; nothing else in those parts changes.
    ///
    /// `allowed_rel_types`: every relationship that points at the part must
    /// have one of these types. A part found by relationship **type** (styles,
    /// shared strings, theme, calcChain, …) is referenced implicitly, which no
    /// scan can see, so the caller states which types it knows to be safe to
    /// drop (xlsx Phase1 passes only [`crate::opc::REL_CALC_CHAIN`]). The
    /// match is exact: Strict types (e.g.
    /// [`crate::opc::REL_CALC_CHAIN_STRICT`]) are separate entries. An empty
    /// slice still removes a part that no relationship points at.
    ///
    /// Refusals, all decided by the same validation as
    /// [`Self::check_remove_part`] before anything changes (no part changes
    /// state either): [`Error::PartNotFound`], or
    /// [`Error::RemovePartRefused`] with a [`RemoveRefusal`]:
    ///
    /// - `RegistryPart`: `[Content_Types].xml` or a relationships part;
    /// - `HasOwnRelationships`: the part has a relationships part (no
    ///   cascading removal);
    /// - `RelationshipTypeNotAllowed`: a relationship to it has a type not in
    ///   `allowed_rel_types`;
    /// - `StillReferenced`: the source part of a removed relationship has an
    ///   attribute (in any namespace: `r:id`, VML `o:relid`, Strict, …) whose
    ///   value, after XML unescaping, is that relationship's `Id`;
    /// - `SourceNotScannable`: that source part is not well-formed XML;
    /// - `RegistryNotEditable`: a registry part that must change was replaced
    ///   wholesale or cannot be parsed (a relationships part that cannot be
    ///   parsed and does not mention the part's file name is ignored).
    ///
    /// - `UnresolvedMention`: a relationship's target has the part's file
    ///   name as its last segment but resolves to no existing part (safety
    ///   net).
    ///
    /// Part names follow OPC equivalence: ASCII case-insensitive and
    /// percent-decoded, for the part, source parts, `_rels` folders, its own
    /// `.rels`, targets and `Override`s; XML whitespace around `Target` and
    /// `Id` is collapsed (`xsd:anyURI`, `xsd:ID`). Only the root
    /// `_rels/.rels` holds package relationships. After a successful
    /// validation the removal itself cannot fail.
    pub fn remove_part(&mut self, name: &str, allowed_rel_types: &[&str]) -> Result<RemovedPart> {
        let (plan, removed) = self.plan_removal(name, allowed_rel_types)?;
        self.apply(plan)?;
        Ok(removed)
    }

    /// The validation of [`Self::remove_part`], without changing anything
    /// (not even part states). If it returns `Ok`, `remove_part` with the
    /// same arguments succeeds as long as the package is not changed in
    /// between: both run this same validation. Lets a format crate validate
    /// a multi-part edit before applying any of it.
    pub fn check_remove_part(&self, name: &str, allowed_rel_types: &[&str]) -> Result<()> {
        self.plan_removal(name, allowed_rel_types).map(drop)
    }

    /// `remove_part` is the one-part case of [`RegistryPlan::remove_parts`]
    /// (one implementation of the removal rules; review of PR #38 m7), plus
    /// the `HasOwnRelationships` refusal and its own error type.
    fn plan_removal(&self, name: &str, allowed_rel_types: &[&str]) -> Result<(RegistryPlan, RemovedPart)> {
        let idx = self.find_ci(name).ok_or_else(|| Error::PartNotFound(name.into()))?;
        let name = self.parts[idx].name.clone();
        let refuse = |reason: RemoveRefusal| Error::RemovePartRefused { part: name.clone(), reason };
        if name.eq_ignore_ascii_case(CONTENT_TYPES_PART) || is_rels_part(&normalize_part_name(&name)) {
            return Err(refuse(RemoveRefusal::RegistryPart));
        }
        if let Some(i) = self.find_ci(&rels_part_name(Some(&name))) {
            return Err(refuse(RemoveRefusal::HasOwnRelationships { rels: self.parts[i].name.clone() }));
        }
        let mut plan = RegistryPlan::new(self);
        let removed = plan.remove_parts(self, &[name.as_str()], allowed_rel_types).map_err(|e| match e {
            Error::RegistryRefused { reason: RegistryRefusal::Remove(r), .. } => refuse(r),
            Error::RegistryRefused { reason: RegistryRefusal::RegistryPart, .. } => refuse(RemoveRefusal::RegistryPart),
            Error::RegistryRefused { reason: RegistryRefusal::PartNotFound, .. } => Error::PartNotFound(name.clone()),
            e => e,
        })?;
        Ok((plan, RemovedPart { relationships: removed.relationships, content_type_override: removed.overrides > 0 }))
    }

    /// Every change to the package goes through here: a registry plan made
    /// before it is stale (review of PR #38 m2).
    fn touch(&mut self) {
        self.generation += 1;
    }

    /// The part equivalent to `name` under OPC part-name equivalence
    /// (ASCII case-insensitive, percent-decoded).
    fn find_ci(&self, name: &str) -> Option<usize> {
        self.parts.iter().position(|p| same_part_name(&p.name, name))
    }

    /// Parses a `Raw` part for planning with the package's parse options
    /// (the tree is installed by apply, so parsing never changes a part's
    /// state on refusal).
    fn parse_raw(&self, i: usize) -> std::result::Result<XmlDocument, String> {
        let bytes = self.read_original(&self.parts[i]).map_err(|e| e.to_string())?;
        let mut options = self.parse_options;
        if is_opc_metadata(&self.parts[i].name) {
            options.max_depth = options.max_depth.max(crate::xml::DEFAULT_MAX_DEPTH);
        }
        XmlDocument::parse_with(&bytes, &options).map_err(|e| e.to_string())
    }

    /// The relationships parts of the package, each with whom it belongs to
    /// (review of PR #14 N17, PR #38 M1: the **one** place that derives a
    /// relationships part's source and base). Source and base come from the
    /// part's **raw** name; looking the source up (`find_ci`) decodes once.
    /// A part that is a relationships part only after percent-decoding
    /// (`%5Frels`, `.rel%73`: invalid OPC names) cannot be attributed and is
    /// reported as [`RelsKind::Unattributable`].
    fn rels_parts(&self) -> impl Iterator<Item = (usize, RelsKind)> + '_ {
        self.parts.iter().enumerate().filter_map(|(i, p)| rels_kind(&p.name).map(|k| (i, k)))
    }

    /// `[Content_Types].xml`, as opened.
    pub fn content_types(&self) -> &ContentTypes {
        &self.content_types
    }

    /// Content type of a part.
    pub fn content_type(&self, name: &str) -> Option<&str> {
        self.content_types.content_type_of(name)
    }

    /// Package-level relationships (`_rels/.rels`), as opened.
    pub fn package_relationships(&self) -> &Relationships {
        &self.package_rels
    }

    /// Relationships of a part (empty if it has no relationships part).
    pub fn relationships(&self, source: &str) -> Result<Relationships> {
        // OPC equivalence, as the registry operations look it up (review of
        // PR #38 round 2, n5: `Sheet1.xml` with `_rels/sheet1.xml.rels`)
        let Some(i) = self.find_ci(&rels_part_name(Some(source))) else {
            return Ok(Relationships::empty(Some(source)));
        };
        Relationships::parse(Some(source), &self.read_part(&self.parts[i].name)?)
    }

    /// The main document part (target of the package's `officeDocument`
    /// relationship), e.g. `xl/workbook.xml`.
    pub fn main_document_part(&self) -> Option<String> {
        let rel = self.package_rels.by_type(REL_OFFICE_DOCUMENT).next()?;
        self.package_rels.resolve(rel)
    }

    /// Writes the package. `Raw` and dirty-free `Parsed` parts are
    /// raw-copied; part order is preserved.
    pub fn write_to<W: Write + Seek>(&self, writer: W) -> Result<W> {
        let mut zw = ZipWriter::new(writer);
        let mut archive = self.archive.clone();
        for part in &self.parts {
            let fresh: Option<Vec<u8>> = match &part.state {
                PartState::Raw => None,
                PartState::Parsed(d) if !d.is_dirty() && !part.detached => None,
                PartState::Parsed(d) => Some(d.serialize().into_bytes()),
                PartState::Replaced(b) => Some(b.clone()),
            };
            match fresh {
                None => zw.raw_copy_file(archive.by_index_raw(part.index)?)?,
                Some(bytes) => {
                    let method = match part.compression {
                        CompressionMethod::Stored => CompressionMethod::Stored,
                        _ => CompressionMethod::Deflated,
                    };
                    zw.start_file(part.name.as_str(), SimpleFileOptions::default().compression_method(method))?;
                    zw.write_all(&bytes)?;
                }
            }
        }
        Ok(zw.finish()?)
    }

    /// Serializes the package to bytes.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        Ok(self.write_to(Cursor::new(Vec::new()))?.into_inner())
    }

    /// Saves atomically: writes a temporary file next to `path`, syncs it,
    /// then renames it over `path`. Saving to the path the package was
    /// opened from is safe.
    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        let file_name = path
            .file_name()
            .ok_or_else(|| Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, "path has no file name")))?;
        let dir = match path.parent() {
            Some(d) if !d.as_os_str().is_empty() => d.to_path_buf(),
            _ => PathBuf::from("."),
        };
        let (tmp, file) = create_temp(&dir, &file_name.to_string_lossy())?;
        let result = (|| -> Result<()> {
            let writer = self.write_to(BufWriter::new(file))?;
            let file = writer.into_inner().map_err(|e| Error::Io(e.into_error()))?;
            file.sync_all()?;
            drop(file);
            fs::rename(&tmp, path)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        result
    }
}

/// Whom a relationships part belongs to ([`Package::rels_parts`]).
#[derive(Debug, PartialEq, Eq)]
enum RelsKind {
    /// Its source and base, from the raw name.
    Of(RelsSource),
    /// A relationships part only after decoding, or one without a source:
    /// it may refer to anything, so an operation it might affect refuses if
    /// it mentions the name concerned.
    Unattributable,
}

fn rels_kind(raw_name: &str) -> Option<RelsKind> {
    if is_rels_part(raw_name) {
        Some(source_of_rels_part(raw_name).map_or(RelsKind::Unattributable, RelsKind::Of))
    } else if is_rels_part(&normalize_part_name(raw_name)) {
        Some(RelsKind::Unattributable)
    } else {
        None
    }
}

/// `…/_rels/….rels` (or the package's `_rels/.rels`).
fn is_rels_part(name: &str) -> bool {
    let (dir, file) = name.rsplit_once('/').unwrap_or(("", name));
    let dir_last = dir.rsplit('/').next().unwrap_or(dir);
    file.to_ascii_lowercase().ends_with(".rels") && dir_last.eq_ignore_ascii_case("_rels")
}

/// Who a relationships part belongs to (`_rels` matched ASCII
/// case-insensitively, as OPC part names are).
#[derive(Clone, Debug, PartialEq, Eq)]
enum RelsSource {
    /// The root `_rels/.rels` (and only it).
    Package,
    /// `dir/_rels/name.rels` → `dir/name`.
    Part(String),
    /// `dir/_rels/.rels` below the root: no part can own it; relative targets
    /// still resolve against `dir`.
    Orphan(String),
}

impl RelsSource {
    /// The base that relative targets resolve against (as a source part name).
    fn base(&self) -> Option<String> {
        match self {
            RelsSource::Package => None,
            RelsSource::Part(p) => Some(p.clone()),
            RelsSource::Orphan(dir) => Some(format!("{dir}/")),
        }
    }

    fn part(&self) -> Option<&str> {
        match self {
            RelsSource::Part(p) => Some(p),
            _ => None,
        }
    }
}

fn source_of_rels_part(rels: &str) -> Option<RelsSource> {
    let (dir, file) = rels.rsplit_once('/')?;
    let parent = if dir.eq_ignore_ascii_case("_rels") {
        ""
    } else {
        let cut = dir.len().checked_sub("/_rels".len())?;
        if !dir.is_char_boundary(cut) || !dir[cut..].eq_ignore_ascii_case("/_rels") {
            return None;
        }
        &dir[..cut]
    };
    let stem = &file[..file.len().checked_sub(".rels".len())?];
    Some(match (parent.is_empty(), stem.is_empty()) {
        (true, true) => RelsSource::Package,
        (false, true) => RelsSource::Orphan(parent.to_owned()),
        (true, false) => RelsSource::Part(stem.to_owned()),
        (false, false) => RelsSource::Part(format!("{parent}/{stem}")),
    })
}

/// Does any attribute, in any namespace (`r:id`, VML `o:relid`, Strict
/// relationships namespace, …), carry `id` after XML unescaping and
/// character-reference decoding? Over-matching only ever refuses a removal.
fn refers_to_rel_id(xml: &[u8], id: &str) -> std::result::Result<bool, String> {
    use quick_xml::XmlVersion;
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_reader(xml);
    loop {
        match reader.read_event() {
            Ok(Event::Start(e) | Event::Empty(e)) => {
                for a in e.attributes().with_checks(true) {
                    let a = a.map_err(|e| e.to_string())?;
                    let v = a.normalized_value(XmlVersion::Implicit1_0).map_err(|e| e.to_string())?;
                    if xml_trim(&v) == xml_trim(id) {
                        return Ok(true);
                    }
                }
            }
            Ok(Event::DocType(_)) => return Err("DTD (<!DOCTYPE>) is not allowed".into()),
            Ok(Event::Eof) => return Ok(false),
            Ok(_) => {}
            Err(e) => return Err(format!("not well-formed XML: {e}")),
        }
    }
}

fn create_temp(dir: &Path, stem: &str) -> Result<(PathBuf, File)> {
    let pid = std::process::id();
    for attempt in 0u32..1000 {
        let tmp = dir.join(format!(".{stem}.{pid}.{attempt}.tmp"));
        match File::create_new(&tmp) {
            Ok(f) => return Ok((tmp, f)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Err(Error::Io(std::io::Error::new(std::io::ErrorKind::AlreadyExists, "could not create a temporary file")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rels_part_names() {
        assert!(is_rels_part("_rels/.rels"));
        assert!(is_rels_part("xl/_rels/workbook.xml.rels"));
        assert!(is_rels_part("xl/_RELS/workbook.xml.RELS"));
        assert!(!is_rels_part("xl/workbook.xml"));
        assert!(!is_rels_part("xl/foo.rels"));
        assert!(!is_rels_part("xl/my_rels/foo.rels"));
        assert_eq!(source_of_rels_part("_rels/.rels"), Some(RelsSource::Package));
        assert_eq!(source_of_rels_part("_RELS/.rels"), Some(RelsSource::Package));
        assert_eq!(source_of_rels_part("foo/_rels/.rels"), Some(RelsSource::Orphan("foo".into())));
        assert_eq!(source_of_rels_part("xl/_RELS/workbook.xml.rels"), Some(RelsSource::Part("xl/workbook.xml".into())));
        assert_eq!(source_of_rels_part("_rels/top.xml.rels"), Some(RelsSource::Part("top.xml".into())));
        for s in ["xl/workbook.xml", "top.xml", "a/b/c.xml"] {
            assert_eq!(source_of_rels_part(&rels_part_name(Some(s))), Some(RelsSource::Part(s.into())));
        }
    }

    #[test]
    fn part_name_equivalence() {
        use crate::opc::same_part_name;
        assert!(same_part_name("xl/calc%E7%94%BB.xml", "xl/calc画.xml"));
        assert!(same_part_name("/XL/CalcChain.xml", "xl/calcchain.xml"));
        assert!(same_part_name("xl/calc%43hain.xml", "xl/calcChain.xml"));
        assert!(!same_part_name("xl/a.xml", "xl/b.xml"));
        assert!(!same_part_name("xl/a%2", "xl/a"));
    }
}
