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
use crate::opc::{CONTENT_TYPES_PART, ContentTypes, REL_OFFICE_DOCUMENT, Relationships, rels_part_name};
use crate::xml::XmlDocument;

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
    /// Index of the original zip entry.
    index: usize,
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
}

impl Package {
    /// Opens a package file. The whole file is read into memory first.
    ///
    /// Errors: [`Error::EncryptedPackage`] for an OLE/CFB file,
    /// [`Error::StrictNotSupported`] for ECMA-376 Strict packages.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::from_bytes(fs::read(path)?)
    }

    /// Opens a package from bytes.
    pub fn from_bytes(data: impl Into<Arc<[u8]>>) -> Result<Self> {
        let data: Arc<[u8]> = data.into();
        if data.starts_with(&CFB_MAGIC) {
            return Err(Error::EncryptedPackage);
        }
        let mut archive = ZipArchive::new(Cursor::new(data))?;
        let mut parts = Vec::with_capacity(archive.len());
        for index in 0..archive.len() {
            let file = archive.by_index_raw(index)?;
            let name = file.name()?.into_owned();
            if parts.iter().any(|p: &Part| p.name.eq_ignore_ascii_case(&name)) {
                return Err(Error::Package(format!("duplicate part name {name:?}")));
            }
            parts.push(Part { name, index, compression: file.compression(), state: PartState::Raw });
        }
        let mut pkg = Package {
            archive,
            parts,
            content_types: ContentTypes::default(),
            package_rels: Relationships::empty(None),
        };
        pkg.content_types = ContentTypes::parse(&pkg.read_part(CONTENT_TYPES_PART)?)?;
        let rels_name = rels_part_name(None);
        if pkg.contains(&rels_name) {
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

    fn read_original(&self, index: usize) -> Result<Vec<u8>> {
        let mut archive = self.archive.clone();
        let mut file = archive.by_index(index)?;
        let mut buf = Vec::with_capacity(file.size() as usize);
        file.read_to_end(&mut buf)?;
        Ok(buf)
    }

    /// The current (decompressed) content of a part: original bytes, the
    /// serialized tree, or the replacement bytes.
    pub fn read_part(&self, name: &str) -> Result<Vec<u8>> {
        let part = self.get(name)?;
        match &part.state {
            PartState::Raw => self.read_original(part.index),
            PartState::Parsed(d) if !d.is_dirty() => self.read_original(part.index),
            PartState::Parsed(d) => Ok(d.serialize().into_bytes()),
            PartState::Replaced(b) => Ok(b.clone()),
        }
    }

    fn ensure_parsed(&mut self, name: &str) -> Result<usize> {
        let i = self.find(name).ok_or_else(|| Error::PartNotFound(name.into()))?;
        match self.parts[i].state {
            PartState::Parsed(_) => {}
            PartState::Raw => {
                let doc = XmlDocument::parse(&self.read_original(self.parts[i].index)?)?;
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
        match &mut self.parts[i].state {
            PartState::Parsed(d) => Ok(d),
            _ => unreachable!(),
        }
    }

    /// Replaces an existing part's bytes wholesale. Adding a new part is not
    /// supported yet ([`Error::NewPartUnsupported`]): it needs
    /// `[Content_Types].xml` and relationships updated together (ADR-0001).
    pub fn set_part(&mut self, name: &str, bytes: impl Into<Vec<u8>>) -> Result<()> {
        let i = self.find(name).ok_or_else(|| Error::NewPartUnsupported(name.into()))?;
        self.parts[i].state = PartState::Replaced(bytes.into());
        Ok(())
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
        let name = rels_part_name(Some(source));
        if !self.contains(&name) {
            return Ok(Relationships::empty(Some(source)));
        }
        Relationships::parse(Some(source), &self.read_part(&name)?)
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
                PartState::Parsed(d) if !d.is_dirty() => None,
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
