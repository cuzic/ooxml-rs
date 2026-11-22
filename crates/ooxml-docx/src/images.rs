//! Word Phase2 inline images (ADR-0011 §3): `insert_image`.
//!
//! Order (§1): checks and the Layer 2 dry run (outer `w:r`, depth bound)
//! first, then the registry plan (media part and relationship, reused when
//! possible), `Package::apply`, and the tree edits.

use ooxml_core::{ElementSpec, NsDecl, PartNameRequest, QName, RegistryPlan, Target};
use ooxml_schema::dml::{
    AdjustValueList, Blip, Extents, FillRectangle, Graphic, GraphicData, GraphicFrameLocks, Offset, PresetGeometry,
    Stretch, Transform2D,
};
use ooxml_schema::wml::{
    BlipFill, DocProperties, Drawing, EffectExtent, Extent, Inline, NS_A, NS_PIC, NS_R, NS_WP,
    NonVisualDrawingProperties, NonVisualGraphicFrameDrawingProperties, NonVisualPictureDrawingProperties,
    NonVisualPictureProperties, ParagraphProperties, Picture, Run as R, ShapeProperties,
};

use crate::body::Wrapper;
use crate::document::Document;
use crate::error::{Error, Result};
use crate::fields::Operation;
use crate::names::is;
use crate::structure::RunPosition;

const IMAGE_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";
const PIC_URI: &str = "http://schemas.openxmlformats.org/drawingml/2006/picture";
const EMU_PER_INCH: u64 = 914_400;
// OOXML ST_PositiveCoordinate upper bound.
const MAX_IMAGE_EMU: u64 = 27_273_042_316_900;

fn q(local: &'static str) -> QName {
    QName::new("", local)
}

/// The size of an inserted image.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ImageSize {
    /// The pixel size at the image's DPI (96 when the file states none,
    /// Word's default *(u)*).
    #[default]
    Auto,
    /// An explicit size in EMU (914,400 per inch).
    Emu {
        /// Width.
        cx: u64,
        /// Height.
        cy: u64,
    },
}

/// The drawing id (`wp:docPr/@id`) of an inserted image.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DrawingId(pub u32);

/// What the magic bytes and the header say.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ImageInfo {
    pub content_type: &'static str,
    pub ext: &'static str,
    pub width: u32,
    pub height: u32,
    /// Dots per inch (x, y), when the file states them.
    pub dpi: Option<(f64, f64)>,
}

fn be16(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from(u16::from_be_bytes(b.get(i..i + 2)?.try_into().ok()?)))
}
fn be32(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(i..i + 4)?.try_into().ok()?))
}
fn le16(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from(u16::from_le_bytes(b.get(i..i + 2)?.try_into().ok()?)))
}

/// Sniffs PNG, JPEG or GIF from the magic bytes (never from a name) and
/// reads the pixel size and DPI from the header (ADR-0011 §3).
pub(crate) fn sniff(b: &[u8]) -> Option<ImageInfo> {
    if b.starts_with(b"\x89PNG\r\n\x1a\n") {
        if b.get(12..16)? != b"IHDR" {
            return None;
        }
        let (width, height) = (be32(b, 16)?, be32(b, 20)?);
        let mut dpi = None;
        let mut i = 8;
        while i + 8 <= b.len() {
            let len = be32(b, i)? as usize;
            let ty = b.get(i + 4..i + 8)?;
            if ty == b"pHYs" && len >= 9 {
                let (x, y, unit) = (be32(b, i + 8)?, be32(b, i + 12)?, *b.get(i + 16)?);
                if unit == 1 && x > 0 && y > 0 {
                    // pixels per metre, rounded to whole dots per inch (3780 → 96, 5670 → 144)
                    let dx = (f64::from(x) * 0.0254).round();
                    let dy = (f64::from(y) * 0.0254).round();
                    dpi = (dx > 0.0 && dy > 0.0).then_some((dx, dy));
                }
            }
            if ty == b"IDAT" || ty == b"IEND" {
                break;
            }
            i = i.checked_add(12 + len)?;
        }
        return Some(ImageInfo { content_type: "image/png", ext: "png", width, height, dpi });
    }
    if b.starts_with(&[0xFF, 0xD8]) {
        let mut i = 2;
        let mut dpi = None;
        while i + 4 <= b.len() {
            if b[i] != 0xFF {
                return None;
            }
            let marker = b[i + 1];
            if marker == 0xFF {
                i += 1;
                continue;
            }
            let len = be16(b, i + 2)? as usize;
            if marker == 0xE0 && b.get(i + 4..i + 9) == Some(b"JFIF\0") {
                let (unit, x, y) = (*b.get(i + 11)?, be16(b, i + 12)?, be16(b, i + 14)?);
                if x > 0 && y > 0 {
                    dpi = match unit {
                        1 => Some((f64::from(x), f64::from(y))),
                        2 => Some((f64::from(x) * 2.54, f64::from(y) * 2.54)),
                        _ => None,
                    };
                }
            }
            if matches!(marker, 0xC0..=0xCF) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
                let (height, width) = (be16(b, i + 5)?, be16(b, i + 7)?);
                return (width > 0 && height > 0).then_some(ImageInfo { content_type: "image/jpeg", ext: "jpeg", width, height, dpi });
            }
            i = i.checked_add(2 + len)?;
        }
        return None;
    }
    if b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a") {
        let (width, height) = (le16(b, 6)?, le16(b, 8)?);
        return (width > 0 && height > 0).then_some(ImageInfo { content_type: "image/gif", ext: "gif", width, height, dpi: None });
    }
    None
}

impl ImageInfo {
    fn emu(self) -> (u64, u64) {
        let (dx, dy) = self.dpi.unwrap_or((96.0, 96.0));
        let f = |px: u32, dpi: f64| ((f64::from(px) * EMU_PER_INCH as f64 / dpi).round() as u64).max(1);
        (f(self.width, dx), f(self.height, dy))
    }
}

impl Document {
    /// Every drawing id in use (ADR-0011 §2): `@id` of every `wp:docPr` and
    /// every DrawingML `*:cNvPr`, in every story part (both branches of
    /// `mc:AlternateContent`, text boxes).
    fn max_drawing_id(&self) -> u64 {
        let mut max = 0u64;
        for s in self.stories() {
            let mut stack = vec![s.raw()];
            while let Some(n) = stack.pop() {
                let name = n.name();
                let is_id_holder = (name.ns() == NS_WP && name.local() == "docPr") || name.local() == "cNvPr";
                if is_id_holder && let Some(v) = n.attr(q("id")).and_then(|v| v.trim().parse::<u64>().ok()) {
                    max = max.max(v);
                }
                stack.extend(n.elements());
            }
        }
        max
    }

    /// An existing media part with these bytes and content type, found
    /// through the image relationships of every story part (ADR-0011 §3).
    fn existing_media(&self, bytes: &[u8], content_type: &str) -> Result<Option<String>> {
        for s in self.stories() {
            let rels = self.pkg.relationships(s.id().part())?;
            for r in rels.by_type(IMAGE_REL) {
                let Some(name) = rels.resolve(r) else { continue };
                if !self.pkg.contains(&name) || self.pkg.content_type(&name) != Some(content_type) {
                    continue;
                }
                if self.pkg.read_part(&name)? == bytes {
                    return Ok(Some(name));
                }
            }
        }
        Ok(None)
    }

    /// Inserts an inline picture in a new run at the end of a paragraph or
    /// after a run that is a direct child of its paragraph (ADR-0011 §3).
    ///
    /// - The format (PNG, JPEG, GIF) is sniffed from the bytes; the size comes
    ///   from the header (`ImageSize::Auto`) or is given in EMU.
    /// - An existing media part with the same bytes and type is reused, and
    ///   an existing image relationship of the story part to it; otherwise
    ///   a new part `word/media/image{n}.<ext>` and relationship are added
    ///   through core's registry (never a new `Default` content type).
    /// - The XML is Word's minimal inline picture. `wp`, `r`, `a` and `pic`
    ///   are declared on the outermost created element that needs them,
    ///   only when no prefixed binding is in scope.
    /// - `wp:docPr/@id` is one more than every drawing id of every story.
    ///
    /// Errors, before any change: [`Self::write_gate_for`]
    /// ([`Operation::Format`]); the paragraph checks of
    /// [`Self::insert_paragraph`]; [`Error::UnsupportedImage`];
    /// [`Error::InvalidValue`] (extent outside `1..=27273042316900` EMU);
    /// [`Error::DrawingIdsExhausted`]; [`Error::NestedHyperlink`] /
    /// [`Error::UnsupportedContext`] for the run position;
    /// [`Error::RelIdReferenced`]; `Core(TooDeep)` and other dry-run
    /// failures; core registry refusals.
    pub fn insert_image(&mut self, at: RunPosition, image: &[u8], size: ImageSize, alt: &str) -> Result<DrawingId> {
        self.write_gate_for(Operation::Format)?;
        let info = sniff(image).ok_or(Error::UnsupportedImage)?;
        crate::text::map_text(alt)?;
        let para = match at {
            RunPosition::EndOf(p) => p,
            RunPosition::After(r) => r.para(),
        };
        let (ppath, slot_k, _) = self.run_insertion_point(at)?;
        let (cx, cy) = match size {
            ImageSize::Auto => info.emu(),
            ImageSize::Emu { cx, cy } => (cx, cy),
        };
        if !(1..=MAX_IMAGE_EMU).contains(&cx) || !(1..=MAX_IMAGE_EMU).contains(&cy) {
            return Err(Error::InvalidValue { what: "image size", value: format!("{cx}×{cy}") });
        }
        let id = self.max_drawing_id() + 1;
        let id = u32::try_from(id).ok().filter(|v| *v <= i32::MAX as u32).ok_or(Error::DrawingIdsExhausted)?;
        let kind = self.stories[para.story.0].id.kind;
        let part = self.stories[para.story.0].id.part.clone();
        let r_values = self.r_values(&part);
        let existing = self.existing_media(image, info.content_type)?;
        let existing_rel = match &existing {
            Some(name) => {
                let rels = self.pkg.relationships(&part)?;
                rels.by_type(IMAGE_REL).find(|r| rels.resolve(r).as_deref() == Some(name.as_str())).map(|r| r.id.clone())
            }
            None => None,
        };
        // 1. dry run: the outer w:r and the depth bound (9 levels below the paragraph)
        {
            let limit = self.pkg.parse_options().max_depth;
            if ppath.len() + 1 + 9 > limit {
                return Err(Error::Core(ooxml_core::Error::TooDeep { limit, position: 0 }));
            }
            let xml = self.pkg.xml_part_mut(&part)?;
            let p = crate::structure::para_lens(xml, kind, &ppath)?;
            p.check_insert_by::<R>(move |i, _| Some(i) == slot_k)?;
        }
        // 2. plan, 3. apply
        let rid = match existing_rel {
            Some(id) => id,
            None => {
                let mut plan = RegistryPlan::new(&self.pkg);
                let target = match existing {
                    Some(name) => name,
                    None => {
                        let ext = self
                            .pkg
                            .content_types()
                            .defaults()
                            .iter()
                            .find(|(_, t)| t.eq_ignore_ascii_case(info.content_type))
                            .map_or_else(|| info.ext.to_owned(), |(e, _)| e.clone());
                        plan.add_part(&self.pkg, PartNameRequest::Numbered(format!("word/media/image{{}}.{ext}")), info.content_type, image.to_vec())?
                    }
                };
                let id = plan.add_relationship(&self.pkg, Some(&part), IMAGE_REL, Target::Part(target))?;
                if r_values.contains(&id.to_ascii_lowercase()) {
                    return Err(Error::RelIdReferenced { id });
                }
                self.pkg.apply(plan)?;
                id
            }
        };
        // 4. tree edits
        let name = format!("Picture {id}");
        let xml = self.pkg.xml_part_mut(&part)?;
        let mut p = crate::structure::para_lens(xml, kind, &ppath)?;
        let mut run = p.insert_by::<R>(move |i, _| Some(i) == slot_k)?;
        let mut drawing = run.insert::<Drawing>()?;
        let mut inline = drawing.insert_with_ns::<Inline>(&[NsDecl { prefix: "wp", uri: NS_WP }, NsDecl { prefix: "r", uri: NS_R }])?;
        for a in ["distT", "distB", "distL", "distR"] {
            inline.set_attr(q(a), "0")?;
        }
        {
            let mut e = inline.insert::<Extent>()?;
            e.set_attr(q("cx"), &cx.to_string())?;
            e.set_attr(q("cy"), &cy.to_string())?;
        }
        {
            let mut e = inline.insert::<EffectExtent>()?;
            for a in ["l", "t", "r", "b"] {
                e.set_attr(q(a), "0")?;
            }
        }
        {
            let mut d = inline.insert::<DocProperties>()?;
            d.set_attr(q("id"), &id.to_string())?;
            d.set_attr(q("name"), &name)?;
            if !alt.is_empty() {
                d.set_attr(q("descr"), alt)?;
            }
        }
        inline
            .insert::<NonVisualGraphicFrameDrawingProperties>()?
            .insert_with_ns::<GraphicFrameLocks>(&[NsDecl { prefix: "a", uri: NS_A }])?
            .set_attr(q("noChangeAspect"), "1")?;
        let mut graphic = inline.insert_with_ns::<Graphic>(&[NsDecl { prefix: "a", uri: NS_A }])?;
        let mut data = graphic.insert::<GraphicData>()?;
        data.set_attr(q("uri"), PIC_URI)?;
        let mut pic = data.insert_with_ns::<Picture>(&[NsDecl { prefix: "pic", uri: NS_PIC }])?;
        {
            let mut nv = pic.insert::<NonVisualPictureProperties>()?;
            let mut c = nv.insert::<NonVisualDrawingProperties>()?;
            c.set_attr(q("id"), "0")?;
            c.set_attr(q("name"), &name)?;
            nv.insert::<NonVisualPictureDrawingProperties>()?;
        }
        {
            let mut fill = pic.insert::<BlipFill>()?;
            fill.insert::<Blip>()?.set_attr(QName::new(NS_R, "embed"), &rid)?;
            fill.insert::<Stretch>()?.insert::<FillRectangle>()?;
        }
        {
            let mut sp = pic.insert::<ShapeProperties>()?;
            let mut xfrm = sp.insert::<Transform2D>()?;
            let mut off = xfrm.insert::<Offset>()?;
            off.set_attr(q("x"), "0")?;
            off.set_attr(q("y"), "0")?;
            let mut ext = xfrm.insert::<Extents>()?;
            ext.set_attr(q("cx"), &cx.to_string())?;
            ext.set_attr(q("cy"), &cy.to_string())?;
            let mut geom = sp.insert::<PresetGeometry>()?;
            geom.set_attr(q("prst"), "rect")?;
            geom.insert::<AdjustValueList>()?;
        }
        self.reindex(para.story);
        self.bump(para);
        Ok(DrawingId(id))
    }

    /// Lower-cased values of the part's `r:` attributes (ADR-0011 §1 item 4).
    pub(crate) fn r_values(&self, part: &str) -> Vec<String> {
        let doc = self.pkg.parsed_part(part).expect("story parts are parsed");
        let mut out = Vec::new();
        let mut stack = vec![doc.root()];
        while let Some(n) = stack.pop() {
            out.extend(n.attrs().iter().filter(|a| a.name().ns() == NS_R).map(|a| a.value().to_ascii_lowercase()));
            stack.extend(n.elements());
        }
        out
    }

    /// Where a new run-level child goes: the paragraph's path, the in-slot
    /// index of the content child it goes before (`None`: the end), and the
    /// index the first new run will have.
    pub(crate) fn run_insertion_point(&self, at: RunPosition) -> Result<(Vec<usize>, Option<usize>, usize)> {
        let para = match at {
            RunPosition::EndOf(p) => p,
            RunPosition::After(r) => r.para(),
        };
        let (parent_path, item) = self.para_location(para)?;
        self.check_structure_neighbour(para)?;
        let p = self.paragraph_at(para).ok_or(Error::NoSuchParagraph(para.index))?;
        let after_item: Option<usize> = match at {
            RunPosition::EndOf(_) => None,
            RunPosition::After(r) => {
                let run = p.run(r.index()).ok_or(Error::NoSuchRun { paragraph: para.index, run: r.index() })?;
                let ws = run.wrappers();
                if ws.contains(&Wrapper::Hyperlink) {
                    return Err(Error::NestedHyperlink);
                }
                if let Some(w) = ws.first() {
                    return Err(Error::UnsupportedContext { ancestor: w.qname() });
                }
                let rpath = self.stories[para.story.0].index.path(run.info.frame);
                Some(*rpath.last().expect("run path"))
            }
        };
        let mut ppath = parent_path;
        ppath.push(item);
        let p_node = p.raw();
        // The position counts element children across slots (ADR-0020 §4.4).
        if p_node.elements().any(|n| n.is(ooxml_core::name::MC_ALTERNATE_CONTENT)) {
            return Err(ooxml_core::Error::UnresolvedSibling(ooxml_schema::wml::Paragraph::QNAME).into());
        }
        let k = after_item.map(|a| p_node.indexed_elements().filter(|(i, e)| *i <= a && !is::<ParagraphProperties>(e)).count());
        let runs_before = p
            .runs()
            .filter(|run| {
                let rp = self.stories[para.story.0].index.path(run.info.frame);
                after_item.is_none_or(|a| rp[ppath.len()] <= a)
            })
            .count();
        Ok((ppath, k, runs_before))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniffing() {
        // 1×1 PNG with pHYs 3780 px/m (96 dpi)
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x02\0\0\0\x03\x08\x06\0\0\0\0\0\0\0".to_vec();
        png.extend(b"\0\0\0\x09pHYs\0\0\x0e\xc4\0\0\x0e\xc4\x01\0\0\0\0");
        let i = sniff(&png).unwrap();
        assert_eq!((i.content_type, i.width, i.height), ("image/png", 2, 3));
        assert!((i.dpi.unwrap().0 - 96.0).abs() < 0.1);
        let gif = b"GIF89a\x05\0\x07\0\0\0\0";
        assert_eq!(sniff(gif).map(|i| (i.width, i.height)), Some((5, 7)));
        // JPEG: SOI, APP0 JFIF 72 dpi, SOF0 8x4
        let jpg = [
            0xFF, 0xD8, 0xFF, 0xE0, 0, 16, b'J', b'F', b'I', b'F', 0, 1, 1, 1, 0, 72, 0, 72, 0, 0, 0xFF, 0xC0, 0, 11, 8, 0,
            4, 0, 8, 1, 1, 0x11, 0,
        ];
        let i = sniff(&jpg).unwrap();
        assert_eq!((i.content_type, i.width, i.height, i.dpi), ("image/jpeg", 8, 4, Some((72.0, 72.0))));
        assert_eq!(i.emu(), (8 * 914_400 / 72, 4 * 914_400 / 72));
        assert!(sniff(b"<svg/>").is_none());
        assert!(sniff(b"\x89PNG\r\n\x1a\n").is_none());
    }
}
