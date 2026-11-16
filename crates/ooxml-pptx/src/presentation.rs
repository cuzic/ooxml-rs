//! `Presentation`: open/save, the slide list, lazy slide access (ADR-0005 §2).

use std::path::Path;

use ooxml_core::{ElementSpec, Lens, Package, QName, XmlDocument};
use ooxml_schema::pml::{self, SlideId, SlideIdList, SlideIdView};

use crate::error::{Error, Result};
use crate::names::MODERN_COMMENTS_REL;
use crate::read::{Slide, SlideInfo};
use crate::refs::SlideRefs;
use crate::write::SlideMut;

#[derive(Clone, Debug)]
struct SlideEntry {
    slide_id: u32,
    part: Option<String>,
}

/// An open PowerPoint package (`.pptx`, `.pptm`, `.ppsx`, `.potx`).
///
/// Every part that is not edited is written back with its original
/// compressed bytes (ADR-0001 Layer 1). Only slide parts are ever edited;
/// the presentation part is parsed read-only.
///
/// # Examples
///
/// ```no_run
/// use ooxml_pptx::Presentation;
///
/// let mut pres = Presentation::open("deck.pptx")?;
/// for i in 0..pres.slide_count() {
///     for shape in pres.slide(i)?.shapes() {
///         if let Some(body) = shape.text_body() {
///             println!("slide {i}, shape {:?}: {}", shape.name(), body.text());
///         }
///     }
/// }
/// let id = pres.slide(0)?.placeholders()[0].id()?;
/// pres.slide_mut(0)?.shape_mut_by_id(id)?.text_body_mut()?.set_text("New title")?;
/// pres.save("deck.pptx")?;
/// # Ok::<(), ooxml_pptx::Error>(())
/// ```
///
/// A complete, runnable version is `examples/replace_text.rs`.
#[derive(Debug)]
pub struct Presentation {
    pkg: Package,
    slides: Vec<SlideEntry>,
    /// Table edit state that outlives one `SlideMut` (deleted `a16` ids),
    /// by slide part (ADR-0013 §2: the session is the `Presentation`).
    tables: std::collections::BTreeMap<String, crate::write::TableSession>,
}

impl Presentation {
    /// Opens a file (read fully into memory).
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::from_package(Package::open(path)?)
    }

    /// Opens a package from bytes.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self> {
        Self::from_package(Package::from_bytes(bytes)?)
    }

    fn from_package(mut pkg: Package) -> Result<Self> {
        let main = pkg.main_document_part().ok_or_else(|| Error::NotAPresentation("no officeDocument relationship".into()))?;
        if !pkg.contains(&main) {
            return Err(Error::NotAPresentation(format!("officeDocument target {main:?} does not exist")));
        }
        let rels = pkg.relationships(&main)?;
        let doc = pkg.xml_part(&main)?;
        let pres = Lens::<pml::Presentation>::root(doc.root())
            .map_err(|_| Error::NotAPresentation(format!("{main:?} is not a p:presentation")))?;
        if pres.attr(QName::new("", "conformance")) == Some("strict") {
            return Err(Error::StrictConformance);
        }
        let mut ids = Vec::new();
        if let Some(list) = pres.child::<SlideIdList>() {
            for sld in list.children::<SlideId>() {
                let v = SlideIdView(sld);
                let slide_id = v.id()?.ok_or_else(|| Error::NotAPresentation("p:sldId without @id".into()))?;
                ids.push((slide_id, v.relationship_id().map(String::from)));
            }
        }
        let slides = ids
            .into_iter()
            .map(|(slide_id, rid)| SlideEntry {
                slide_id,
                part: rid
                    .and_then(|r| rels.by_id(&r).and_then(|r| rels.resolve(r)))
                    .filter(|p| pkg.contains(p)),
            })
            .collect();
        Ok(Presentation { pkg, slides, tables: Default::default() })
    }

    /// Saves to `path` (temp file + rename; the same path is safe).
    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        Ok(self.pkg.save(path)?)
    }

    /// The package as bytes.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        Ok(self.pkg.to_bytes()?)
    }

    /// The underlying package (read-only), e.g. to read other parts.
    pub fn package(&self) -> &Package {
        &self.pkg
    }

    /// Number of entries in `p:sldIdLst`.
    pub fn slide_count(&self) -> usize {
        self.slides.len()
    }

    fn entry(&self, index: usize) -> Result<(SlideInfo, String)> {
        let e = self.slides.get(index).ok_or(Error::SlideIndexOutOfRange { index, count: self.slides.len() })?;
        let part = e.part.clone().ok_or(Error::DanglingSlide { slide_id: e.slide_id })?;
        Ok((SlideInfo { index, slide_id: e.slide_id, part_name: part.clone() }, part))
    }

    /// Slides in `p:sldIdLst` order; an entry whose `r:id` does not resolve
    /// to an existing part is `Err(DanglingSlide)` (never skipped).
    pub fn slides(&self) -> impl Iterator<Item = Result<SlideInfo>> + '_ {
        (0..self.slides.len()).map(|i| self.entry(i).map(|(info, _)| info))
    }

    /// The `index`-th slide, parsed on first access. Never marks anything dirty.
    pub fn slide(&mut self, index: usize) -> Result<Slide<'_>> {
        let (info, part) = self.entry(index)?;
        let doc = self.pkg.xml_part(&part)?;
        Lens::<pml::Slide>::root(doc.root())?;
        Ok(Slide { doc, info })
    }

    /// The `index`-th slide for editing. Scans the slide's `p:timing` and
    /// its modern comment parts (read-only; they stay Raw) so that edits
    /// of referenced text can be refused (ADR-0005 §6).
    pub fn slide_mut(&mut self, index: usize) -> Result<SlideMut<'_>> {
        let (info, part) = self.entry(index)?;
        let mut refs = SlideRefs::default();
        let rels = self.pkg.relationships(&part)?;
        for r in rels.by_type(MODERN_COMMENTS_REL) {
            // Fail closed: a comment part that cannot be located, read or
            // parsed — for whatever reason, including resource limits of the
            // XML layer — is `None`, and `scan_comments(None)` locks the text
            // of **every** shape of the slide (ADR-0005 §6). The error itself
            // is not propagated: the slide stays readable and editable
            // outside text (and its text edits are refused with
            // `CommentAnchoredText`).
            let doc = match rels.resolve(r).filter(|p| self.pkg.contains(p)) {
                None => None,
                Some(p) => match self.pkg.read_part(&p) {
                    Err(_) => None,
                    Ok(b) => XmlDocument::parse(&b).ok(),
                },
            };
            refs.scan_comments(doc.as_ref());
        }
        let table_styles = self.table_style_ids();
        let tables = self.tables.entry(part.clone()).or_default();
        let doc = self.pkg.xml_part_mut(&part)?;
        if !doc.root().is(pml::Slide::QNAME) {
            return Err(Lens::<pml::Slide>::root(doc.root()).unwrap_err().into());
        }
        refs.scan_timing(doc.root());
        Ok(SlideMut { doc, info, refs, table_styles, tables })
    }

    /// `a:tblStyle@styleId`s defined in the presentation's `tableStyles`
    /// part (found by relationship); empty when absent or unreadable.
    fn table_style_ids(&self) -> Vec<String> {
        const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/tableStyles";
        let Some(main) = self.pkg.main_document_part() else { return Vec::new() };
        let Ok(rels) = self.pkg.relationships(&main) else { return Vec::new() };
        let Some(part) = rels.by_type(REL).filter_map(|r| rels.resolve(r)).find(|p| self.pkg.contains(p)) else {
            return Vec::new();
        };
        let Some(doc) = self.pkg.read_part(&part).ok().and_then(|b| XmlDocument::parse(&b).ok()) else { return Vec::new() };
        doc.root()
            .elements()
            .filter(|e| e.is(QName::new(crate::names::A_NS, "tblStyle")))
            .filter_map(|e| e.attr(QName::new("", "styleId")).map(str::to_owned))
            .collect()
    }
}
