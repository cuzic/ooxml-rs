//! Notes slides: read and edit **existing** notes (ADR-0008 §2).
//!
//! A slide's notes slide is the target of its `…/notesSlide` relationship.
//! Its **notes body** is the first top-level `p:sp` of the notes shape tree
//! whose `p:ph@type` is `body` (python-pptx's `notes_placeholder`). The body
//! is edited with exactly the Phase 0 text API ([`TextBodyMut`]); only the
//! notes part becomes dirty. Creating a notes slide (part, relationship
//! pair, content type) is not part of Phase 1.
//!
//! Notes parts carry no `p:timing` and no comment anchors point into them,
//! so the animation and comment locks of ADR-0005 §6 never apply; a notes
//! body inside `mc:AlternateContent` is refused
//! ([`Error::InsideAlternateContent`]).

use ooxml_core::{ElementSpec, Package, RawNode, XmlDocument};
use ooxml_schema::pml;

use crate::deck::{child, p, rel, un};
use crate::error::{Error, Result};
use crate::read::{self, ShapeKind, ShapePath, ShapeRef, SlideInfo, TextBodyRef};
use crate::write::{Guard, TextBodyMut};

/// The notes slide part of `slide_part` (the first `notesSlide`
/// relationship whose target exists).
pub(crate) fn notes_part(pkg: &Package, slide_part: &str) -> Result<Option<String>> {
    let rels = pkg.relationships(slide_part)?;
    Ok(rels.by_type(rel::NOTES_SLIDE).filter_map(|r| rels.resolve(r)).find(|n| pkg.contains(n)))
}

fn is_body_placeholder(sp: &RawNode) -> bool {
    sp.is(pml::Shape::QNAME)
        && child(sp, p("nvSpPr")).and_then(|nv| child(nv, p("nvPr"))).and_then(|n| child(n, p("ph"))).is_some_and(|ph| ph.attr(un("type")) == Some("body"))
}

/// Where the notes body is.
#[derive(Debug)]
pub(crate) struct BodyLoc {
    /// The shape entry (an `mc:AlternateContent` for a body inside one).
    pub shape: ShapePath,
    /// Node path of the `p:txBody` from the part root.
    pub body: Vec<usize>,
    /// `cNvPr@id` of the body shape.
    pub shape_id: u32,
    /// The body is inside `mc:AlternateContent` (edits are refused).
    pub in_ac: bool,
}

/// The notes body of a notes part ([`Error::NoNotesBody`] if none): the
/// first top-level body placeholder, which must have a `p:txBody`. A body
/// inside `mc:AlternateContent` is a fallback when no top-level placeholder
/// exists (python-pptx does not see those at all).
pub(crate) fn body_loc(doc: &XmlDocument) -> Result<BodyLoc> {
    let mut in_ac = None;
    for s in read::shapes_of(doc) {
        if s.depth() > 0 {
            continue;
        }
        let base = s.path().as_slice().to_vec();
        let (sp, rel_path, ac) = match s.kind() {
            ShapeKind::Shape => (s.raw(), Vec::new(), false),
            ShapeKind::AlternateContent => {
                let Some(rep) = read::ac_representative(s.raw()) else { continue };
                // the branch and the shape inside it
                let Some((bi, branch)) = s.raw().indexed_elements().find(|(_, b)| b.elements().any(|c| std::ptr::eq(c, rep))) else { continue };
                let Some((si, _)) = branch.indexed_elements().find(|(_, c)| std::ptr::eq(*c, rep)) else { continue };
                (rep, vec![bi, si], true)
            }
            _ => continue,
        };
        if !is_body_placeholder(sp) {
            continue;
        }
        let Some((ti, _)) = sp.indexed_elements().find(|(_, c)| c.is(pml::TextBody::QNAME)) else {
            if ac {
                continue;
            }
            return Err(Error::NoNotesBody);
        };
        let shape_id = read::raw_shape_id(sp).unwrap_or(0);
        let loc = BodyLoc { shape: s.path(), body: [base, rel_path, vec![ti]].concat(), shape_id, in_ac: ac };
        if !ac {
            return Ok(loc);
        }
        in_ac.get_or_insert(loc);
    }
    in_ac.ok_or(Error::NoNotesBody)
}

/// A slide's notes slide, read-only (ADR-0008 §2).
#[derive(Clone, Debug)]
pub struct NotesRef<'a> {
    pub(crate) doc: &'a XmlDocument,
    pub(crate) slide: SlideInfo,
    pub(crate) part: String,
}

impl<'a> NotesRef<'a> {
    /// The slide the notes belong to.
    pub fn slide_info(&self) -> &SlideInfo {
        &self.slide
    }
    /// The notes slide part name.
    pub fn part_name(&self) -> &str {
        &self.part
    }
    /// Every shape of the notes shape tree (as [`crate::Slide::shapes`]).
    pub fn shapes(&self) -> Vec<ShapeRef<'a>> {
        read::shapes_of(self.doc)
    }
    /// The notes body shape (`None`: no body placeholder, or only one
    /// inside `mc:AlternateContent`).
    pub fn body_shape(&self) -> Option<ShapeRef<'a>> {
        let loc = body_loc(self.doc).ok().filter(|l| !l.in_ac)?;
        self.shapes().into_iter().find(|s| s.path() == loc.shape)
    }
    /// The notes body's text body.
    pub fn body(&self) -> Option<TextBodyRef<'a>> {
        self.body_shape()?.text_body()
    }
    /// The notes text with the Phase 0 conventions (`\v` for `a:br`, a
    /// field's cached text, `\n` between paragraphs); empty without a body
    /// (python-pptx `notes_slide.notes_text_frame.text`).
    pub fn text(&self) -> String {
        self.body().map(|b| b.text()).unwrap_or_default()
    }
    /// The parsed part.
    pub fn document(&self) -> &'a XmlDocument {
        self.doc
    }
}

/// A slide's notes slide opened for editing. Nothing is marked dirty until
/// a setter changes something; only the notes part is ever changed.
#[derive(Debug)]
pub struct NotesMut<'a> {
    pub(crate) doc: &'a mut XmlDocument,
    pub(crate) slide: SlideInfo,
    pub(crate) part: String,
}

impl NotesMut<'_> {
    /// The slide the notes belong to.
    pub fn slide_info(&self) -> &SlideInfo {
        &self.slide
    }
    /// The notes slide part name.
    pub fn part_name(&self) -> &str {
        &self.part
    }
    /// Read access to the same notes slide.
    pub fn as_ref(&self) -> NotesRef<'_> {
        NotesRef { doc: self.doc, slide: self.slide.clone(), part: self.part.clone() }
    }
    /// The notes body for editing, with every Phase 0 rule of
    /// [`TextBodyMut`].
    ///
    /// Errors: [`Error::NoNotesBody`], [`Error::InsideAlternateContent`].
    pub fn body_mut(&mut self) -> Result<TextBodyMut<'_>> {
        let loc = body_loc(self.doc)?;
        if loc.in_ac {
            return Err(Error::InsideAlternateContent);
        }
        Ok(TextBodyMut::new(&mut *self.doc, loc.body, Guard::unlocked(loc.shape_id)))
    }
}
