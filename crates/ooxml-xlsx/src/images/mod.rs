//! Floating pictures and read-only rich-value images (ADR-0017 §3).

mod drawing;
mod header;
mod rich;
pub(crate) use rich::image as cell_image;

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use ooxml_core::name::MC_ALTERNATE_CONTENT;
use ooxml_core::opc::Relationships;
use ooxml_core::{
    ElementSpec, NsDecl, Package, PartNameRequest, QName, RawNode, RegistryPlan, RegistryRefusal,
    Target, XmlDocument,
};
use ooxml_schema::sml;
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};
use crate::reference::CellIndex;
use crate::workbook::{ApplyStep, Workbook};
use crate::worksheet::Worksheet;

const XDR: &str = "http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing";
const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const ASVG: &str = "http://schemas.microsoft.com/office/drawing/2016/SVG/main";
const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const SML: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
pub(crate) const OFFICE: &str = "urn:schemas-microsoft-com:office:office";
const REL_DRAWING: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/drawing";
pub(crate) const REL_VML: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/vmlDrawing";
const REL_IMAGE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";
const REL_HYPERLINK: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink";
const DRAWING_TYPE: &str = "application/vnd.openxmlformats-officedocument.drawing+xml";
const REL_NS: [NsDecl<'static>; 1] = [NsDecl::new("r", R)];

const fn xdr(local: &'static str) -> QName {
    QName::new(XDR, local)
}
const fn attr(local: &'static str) -> QName {
    QName::new("", local)
}

fn child(node: &RawNode, name: QName) -> Option<&RawNode> {
    node.elements().find(|node| node.is(name))
}

pub(crate) fn walk(root: &RawNode) -> impl Iterator<Item = &RawNode> {
    let mut pending = vec![root];
    std::iter::from_fn(move || {
        let node = pending.pop()?;
        let start = pending.len();
        pending.extend(node.elements());
        pending[start..].reverse();
        Some(node)
    })
}

pub(crate) fn part_error(part: &str, reason: impl std::fmt::Display) -> Error {
    Error::PartNotEditable {
        part: part.into(),
        reason: reason.to_string(),
    }
}

pub(crate) fn internal_target(rels: &Relationships, id: &str, kind: &str) -> Option<String> {
    let rel = rels.by_id(id)?;
    (rel.rel_type == kind).then(|| rels.resolve(rel)).flatten()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Format {
    Png,
    Jpeg,
    Gif,
    Bmp,
    Emf,
    Wmf,
    Svg,
}

impl Format {
    fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpeg",
            Self::Gif => "gif",
            Self::Bmp => "bmp",
            Self::Emf => "emf",
            Self::Wmf => "wmf",
            Self::Svg => "svg",
        }
    }

    fn content_type(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Gif => "image/gif",
            Self::Bmp => "image/bmp",
            Self::Emf => "image/x-emf",
            Self::Wmf => "image/x-wmf",
            Self::Svg => "image/svg+xml",
        }
    }
}

/// Picture bytes and insertion options. The bytes are stored without re-encoding.
#[derive(Clone, Debug)]
pub struct Image {
    bytes: Vec<u8>,
    format: Format,
    extent: (u64, u64),
    svg: Option<Vec<u8>>,
    name: Option<String>,
    alt_text: Option<String>,
    hyperlink: Option<String>,
}

impl Image {
    /// Reads natural dimensions from a PNG, JPEG, GIF, BMP, EMF or placeable WMF header.
    pub fn from_bytes(bytes: impl Into<Vec<u8>>) -> Result<Self> {
        let bytes = bytes.into();
        let (format, extent) = header::read(&bytes)?;
        Ok(Self {
            bytes,
            format,
            extent,
            svg: None,
            name: None,
            alt_text: None,
            hyperlink: None,
        })
    }

    /// Creates an SVG picture with a PNG fallback, whose size supplies the natural dimensions.
    pub fn from_svg(svg: impl Into<Vec<u8>>, png_fallback: impl Into<Vec<u8>>) -> Result<Self> {
        let svg = svg.into();
        let doc = XmlDocument::parse(&svg).map_err(|_| Error::ImageFormat)?;
        let mut image = Self::from_bytes(png_fallback)?;
        if !doc
            .root()
            .is(QName::new("http://www.w3.org/2000/svg", "svg"))
            || image.format != Format::Png
        {
            return Err(Error::ImageFormat);
        }
        image.svg = Some(svg);
        Ok(image)
    }

    /// Sets the shape name.
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Sets the description, up to 255 UTF-16 code units.
    pub fn alt_text(mut self, text: impl Into<String>) -> Self {
        self.alt_text = Some(text.into());
        self
    }

    /// Sets an external hyperlink on the picture.
    pub fn hyperlink(mut self, target: impl Into<String>) -> Self {
        self.hyperlink = Some(target.into());
        self
    }

    /// Natural width and height in English Metric Units (EMU).
    pub fn natural_size(&self) -> (u64, u64) {
        self.extent
    }
}

/// A cell and offsets within it in English Metric Units (EMU).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Marker {
    /// The zero-based cell position.
    pub cell: CellIndex,
    /// Horizontal offset.
    pub x: i64,
    /// Vertical offset.
    pub y: i64,
}

impl From<CellIndex> for Marker {
    fn from(cell: CellIndex) -> Self {
        Self { cell, x: 0, y: 0 }
    }
}

/// Size for a one-cell anchor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Size {
    /// The dimensions read from the image header.
    Natural,
    /// Explicit dimensions in EMU.
    Emu {
        /// Width in EMU.
        width: u64,
        /// Height in EMU.
        height: u64,
    },
}

/// How a two-cell anchor responds to row and column resizing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditAs {
    /// Move and resize with cells.
    TwoCell,
    /// Move with cells.
    OneCell,
    /// Keep position and size.
    Absolute,
}

/// A drawing anchor. Coordinates and offsets are in EMU.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    /// One worksheet marker and a size.
    OneCell {
        /// Position.
        from: Marker,
        /// Dimensions.
        size: Size,
    },
    /// Two worksheet markers.
    TwoCell {
        /// Top-left position.
        from: Marker,
        /// Bottom-right position.
        to: Marker,
        /// How the anchor responds to cell edits.
        edit_as: EditAs,
    },
    /// Absolute sheet position.
    Absolute {
        /// Horizontal position.
        x: i64,
        /// Vertical position.
        y: i64,
        /// Width.
        width: u64,
        /// Height.
        height: u64,
    },
    /// An anchor in a compatibility wrapper, or an anchor with unreadable coordinates.
    Unresolved,
}

impl Anchor {
    /// Anchors the image relative to one cell.
    pub fn one_cell(at: impl Into<Marker>, size: Size) -> Self {
        Self::OneCell {
            from: at.into(),
            size,
        }
    }

    /// Anchors the image to two worksheet markers. The default behavior is move with cells.
    pub fn two_cell(from: impl Into<Marker>, to: impl Into<Marker>) -> Self {
        Self::TwoCell {
            from: from.into(),
            to: to.into(),
            edit_as: EditAs::OneCell,
        }
    }

    /// Anchors the image to an absolute position.
    pub fn absolute(x: i64, y: i64, width: u64, height: u64) -> Self {
        Self::Absolute {
            x,
            y,
            width,
            height,
        }
    }

    /// Sets the two-cell anchor behavior.
    pub fn edit_as(mut self, behavior: EditAs) -> Self {
        if let Self::TwoCell { edit_as, .. } = &mut self {
            *edit_as = behavior;
        }
        self
    }
}

/// An embedded image part.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageMedia {
    /// OPC part name.
    pub part: String,
    /// Exact embedded bytes.
    pub bytes: Vec<u8>,
}

/// A floating worksheet picture.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageInfo {
    /// Position of the containing anchor.
    pub anchor: Anchor,
    /// Nonvisual shape id, if readable.
    pub id: Option<u32>,
    /// Nonvisual shape name.
    pub name: Option<String>,
    /// Decoded description.
    pub alt_text: Option<String>,
    /// Embedded raster or metafile (the PNG fallback for SVG).
    pub media: Option<ImageMedia>,
    /// Optional SVG part.
    pub svg: Option<ImageMedia>,
}

/// A rich-value image stored in a cell.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CellImage {
    /// Embedded part and its bytes.
    pub media: ImageMedia,
    /// Decoded description, when stored with the rich value.
    pub alt_text: Option<String>,
}

type MediaIndex = HashMap<[u8; 32], Vec<String>>;

#[derive(Debug, Default)]
pub(crate) struct ObjectState {
    shape_high_water: HashMap<String, u32>,
    scanned_shapes: HashSet<String>,
    media: Option<MediaIndex>,
    known_shapes: HashMap<String, HashSet<u32>>,
    scanned_note_shapes: HashSet<String>,
}

fn media_part(pkg: &Package, rels: &Relationships, id: &str) -> Option<String> {
    let target = internal_target(rels, id, REL_IMAGE)?;
    let part = pkg.part_name(&target)?;
    let (kind, _) = pkg.content_type(part)?.split_once('/')?;
    kind.eq_ignore_ascii_case("image").then(|| part.to_owned())
}

fn read_media(pkg: &Package, rels: &Relationships, id: &str) -> Option<ImageMedia> {
    let part = media_part(pkg, rels, id)?;
    let bytes = pkg.read_part(&part).ok()?;
    Some(ImageMedia { part, bytes })
}

fn media_index(pkg: &mut Package, sheets: &[crate::census::SheetEntry]) -> MediaIndex {
    let mut index = MediaIndex::new();
    let mut seen = HashSet::new();
    for drawing in sheets
        .iter()
        .filter_map(|sheet| sheet.part.as_deref())
        .filter_map(|sheet| pkg.relationships(sheet).ok())
        .flat_map(|rels| {
            rels.by_type(REL_DRAWING)
                .filter_map(|rel| rels.resolve(rel))
                .collect::<Vec<_>>()
        })
        .collect::<HashSet<_>>()
    {
        let Ok(rels) = pkg.relationships(&drawing) else {
            continue;
        };
        let Ok(doc) = pkg.xml_part(&drawing) else {
            continue;
        };
        let parts: Vec<_> = walk(doc.root())
            .filter(|node| node.is(QName::new(A, "blip")) || node.is(QName::new(ASVG, "svgBlip")))
            .filter_map(|node| node.attr(QName::new(R, "embed")))
            .filter_map(|id| internal_target(&rels, id, REL_IMAGE))
            .collect();
        for target in parts {
            if let Some(part) = pkg.part_name(&target)
                && seen.insert(part.to_owned())
                && pkg
                    .content_type(part)
                    .and_then(|ty| ty.split_once('/'))
                    .is_some_and(|(kind, _)| kind.eq_ignore_ascii_case("image"))
                && let Ok(bytes) = pkg.read_part(part)
            {
                index
                    .entry(Sha256::digest(bytes).into())
                    .or_default()
                    .push(part.to_owned());
            }
        }
    }
    for candidates in index.values_mut() {
        candidates.sort_unstable();
    }
    index
}

fn add_media(
    pkg: &Package,
    plan: &mut RegistryPlan,
    index: &MediaIndex,
    drawing: &str,
    rels: &Relationships,
    format: Format,
    bytes: &[u8],
) -> Result<(String, String)> {
    let extension = format.extension();
    let digest: [u8; 32] = Sha256::digest(bytes).into();
    let existing = index.get(&digest).and_then(|candidates| {
        candidates.iter().find(|part| {
            part.rsplit_once('.')
                .is_some_and(|(_, ext)| ext.eq_ignore_ascii_case(extension))
                && pkg.read_part(part).is_ok_and(|current| current == bytes)
        })
    });
    let part = if let Some(part) = existing {
        part.clone()
    } else {
        match plan.add_default_content_type(pkg, extension, format.content_type()) {
            Ok(_)
            | Err(ooxml_core::Error::RegistryRefused {
                reason:
                    RegistryRefusal::DefaultExists { .. } | RegistryRefusal::WouldRetypePart { .. },
                ..
            }) => {}
            Err(error) => return Err(error.into()),
        }
        plan.add_part(
            pkg,
            PartNameRequest::Numbered(format!("xl/media/image{{}}.{extension}")),
            format.content_type(),
            bytes.to_vec(),
        )?
    };
    let rel = match rels.by_type(REL_IMAGE).find(|rel| {
        rels.resolve(rel)
            .as_deref()
            .and_then(|target| pkg.part_name(target))
            == Some(part.as_str())
    }) {
        Some(rel) => rel.id.clone(),
        None => plan.add_relationship(pkg, Some(drawing), REL_IMAGE, Target::Part(part.clone()))?,
    };
    Ok((rel, part))
}

fn drawing_target(pkg: &mut Package, sheet: &str) -> Result<Option<String>> {
    let rels = pkg.relationships(sheet)?;
    let target = {
        let doc = pkg.xml_part(sheet)?;
        let mut links = doc
            .root()
            .elements()
            .filter(|node| node.is(sml::Drawing::QNAME));
        let Some(link) = links.next() else {
            if doc
                .root()
                .elements()
                .filter(|node| node.is(MC_ALTERNATE_CONTENT))
                .any(|wrapper| walk(wrapper).any(|node| node.is(sml::Drawing::QNAME)))
            {
                return Err(ooxml_core::Error::UnresolvedSibling(sml::Worksheet::QNAME).into());
            }
            return Ok(None);
        };
        if links.next().is_some() {
            return Err(part_error(sheet, "multiple drawing references"));
        }
        let id = link
            .attr(QName::new(R, "id"))
            .ok_or_else(|| part_error(sheet, "drawing has no r:id"))?;
        internal_target(&rels, id, REL_DRAWING)
    };
    target
        .and_then(|part| pkg.part_name(&part).map(str::to_owned))
        .map(Some)
        .ok_or_else(|| part_error(sheet, "drawing relationship does not resolve"))
}

fn drawing_document(pkg: &mut Package, drawing: &str) -> Result<XmlDocument> {
    let options = pkg.parse_options();
    let doc = pkg
        .xml_part(drawing)
        .map_err(|error| part_error(drawing, error))?;
    if !doc.root().is(xdr("wsDr")) {
        return Err(part_error(drawing, "expected wsDr"));
    }
    if doc.is_dirty() {
        // Span edits must be based on this session's current bytes.
        return Ok(XmlDocument::parse_with(
            doc.serialize().as_bytes(),
            &options,
        )?);
    }
    Ok(doc.clone())
}

pub(crate) fn shape_number(value: &str) -> Option<u32> {
    let (_, number) = value.rsplit_once("_s")?;
    number.parse().ok()
}

fn vml_numbers(pkg: &Package, part: &str) -> Result<HashSet<u32>> {
    let canonical = pkg
        .part_name(part)
        .ok_or_else(|| part_error(part, "missing VML part"))?;
    let bytes = pkg
        .read_part(canonical)
        .map_err(|error| part_error(part, error))?;
    match XmlDocument::parse_with(&bytes, &pkg.parse_options()) {
        Ok(doc) => {
            return Ok(walk(doc.root())
                .flat_map(|node| [node.attr(attr("id")), node.attr(QName::new(OFFICE, "spid"))])
                .flatten()
                .filter_map(shape_number)
                .collect());
        }
        Err(error @ ooxml_core::Error::DtdNotAllowed { .. }) => {
            return Err(part_error(part, error));
        }
        Err(_) => {}
    }

    // Some producers store HTML-like markup in VML. Use the corpus
    // patterns only when the XML reader cannot read the part.
    static SHAPE: LazyLock<regex::bytes::Regex> = LazyLock::new(|| {
        regex::bytes::RegexBuilder::new(
            r#"\b(?:id|o:spid)\s*=\s*(?:"[^"]*_s(-?[0-9]+)"|'[^']*_s(-?[0-9]+)')"#,
        )
        .unicode(false)
        .build()
        .expect("fixed pattern")
    });
    if !SHAPE.is_match(&bytes) {
        return Err(part_error(part, "cannot read shape ids from malformed VML"));
    }
    Ok(SHAPE
        .captures_iter(&bytes)
        .filter_map(|m| {
            std::str::from_utf8(m.get(1).or_else(|| m.get(2))?.as_bytes())
                .ok()?
                .parse()
                .ok()
        })
        .collect())
}

fn shape_max(wb: &mut Workbook, sheet: &str, drawing: Option<&XmlDocument>) -> Result<u32> {
    let drawing_max = drawing
        .into_iter()
        .flat_map(|doc| walk(doc.root()))
        .filter(|node| node.is(xdr("cNvPr")))
        .filter_map(|node| node.attr(attr("id"))?.trim().parse::<u32>().ok())
        .max()
        .unwrap_or(1)
        .max(1);
    let high = wb
        .objects
        .shape_high_water
        .get(sheet)
        .copied()
        .unwrap_or_default();
    let mut max = high.max(drawing_max);
    if wb.objects.scanned_shapes.contains(sheet) {
        return Ok(max);
    }
    if let Some(id) = sheet_shape_numbers(wb, sheet)?.into_iter().max() {
        max = max.max(id);
    }
    Ok(max)
}

fn sheet_shape_numbers(wb: &mut Workbook, sheet: &str) -> Result<HashSet<u32>> {
    let rels = wb.pkg.relationships(sheet)?;
    let doc = wb.pkg.xml_part(sheet)?;
    let mut numbers = HashSet::new();
    let mut vml = Vec::new();
    for node in walk(doc.root()) {
        if matches!(node.name().local(), "control" | "oleObject")
            && node.name().ns() == SML
            && let Some(id) = node
                .attr(attr("shapeId"))
                .and_then(|id| id.trim().parse::<u32>().ok())
        {
            numbers.insert(id);
        }
        if node.is(sml::LegacyDrawing::QNAME) || node.is(QName::new(SML, "legacyDrawingHF")) {
            let part = node
                .attr(QName::new(R, "id"))
                .and_then(|id| internal_target(&rels, id, REL_VML))
                .ok_or_else(|| part_error(sheet, "VML relationship does not resolve"))?;
            vml.push(part);
        }
    }
    for part in vml {
        numbers.extend(vml_numbers(&wb.pkg, &part)?);
    }
    Ok(numbers)
}

pub(crate) fn note_shape_numbers(wb: &mut Workbook, sheet: &str) -> Result<(u32, HashSet<u32>)> {
    if wb.objects.scanned_note_shapes.contains(sheet) {
        return Ok((wb.objects.shape_high_water[sheet], wb.objects.known_shapes[sheet].clone()));
    }
    let mut used = sheet_shape_numbers(wb, sheet)?;
    let rels = wb.pkg.relationships(sheet)?;
    for rel in rels.by_type(REL_DRAWING) {
        let part = rels.resolve(rel).and_then(|part| wb.pkg.part_name(&part).map(str::to_owned))
            .ok_or_else(|| part_error(sheet, "invalid drawing target"))?;
        let doc = drawing_document(&mut wb.pkg, &part)?;
        used.extend(walk(doc.root()).filter(|node| node.is(xdr("cNvPr")))
            .filter_map(|node| node.attr(attr("id"))?.trim().parse::<u32>().ok()));
    }
    if let Some(allocated) = wb.objects.known_shapes.get(sheet) {
        used.extend(allocated);
    }
    let high = used.iter().copied().max().unwrap_or(1).max(1)
        .max(wb.objects.shape_high_water.get(sheet).copied().unwrap_or_default());
    wb.objects.shape_high_water.insert(sheet.into(), high);
    wb.objects.known_shapes.insert(sheet.into(), used.clone());
    wb.objects.scanned_shapes.insert(sheet.into());
    wb.objects.scanned_note_shapes.insert(sheet.into());
    Ok((high, used))
}

pub(crate) fn record_shape(wb: &mut Workbook, sheet: &str, id: u32) {
    wb.objects.known_shapes.entry(sheet.into()).or_default().insert(id);
    let high = wb.objects.shape_high_water.entry(sheet.into()).or_default();
    *high = (*high).max(id);
}

impl Worksheet<'_> {
    /// Inserts a picture above the existing drawing objects and returns its shape id.
    pub fn insert_image(&mut self, anchor: Anchor, image: &Image) -> Result<u32> {
        let wb = &mut *self.wb;
        wb.check_editable()?;
        let sheet = wb.sheets[self.sheet].part.clone().expect("worksheet");
        let position = drawing::Position::new(anchor, image.extent)?;
        if let Some(text) = &image.alt_text {
            let units = text.encode_utf16().count();
            if units > 255 {
                return Err(Error::InvalidSetting {
                    what: "image alt text".into(),
                    reason: "over 255 UTF-16 units".into(),
                });
            }
        }
        let target = drawing_target(&mut wb.pkg, &sheet)?;
        let existing = target
            .as_deref()
            .map(|part| drawing_document(&mut wb.pkg, part))
            .transpose()?;
        let id = shape_max(wb, &sheet, existing.as_ref())?
            .checked_add(1)
            .ok_or_else(|| part_error(&sheet, "shape id exhausted"))?;
        if target.is_none() {
            wb.pkg
                .xml_part_mut(&sheet)?
                .lens_mut::<sml::Worksheet>()?
                .check_insert_with_ns::<sml::Drawing>(&REL_NS)?;
        }
        if wb.objects.media.is_none() {
            wb.objects.media = Some(media_index(&mut wb.pkg, &wb.sheets));
        }
        let index = wb.objects.media.as_ref().expect("initialized");
        let mut plan = RegistryPlan::new(&wb.pkg);
        let (part, sheet_rel, rels) = if let Some(part) = target {
            let rels = wb.pkg.relationships(&part)?;
            (part, None, rels)
        } else {
            let part = plan.add_part(
                &wb.pkg,
                PartNameRequest::Numbered("xl/drawings/drawing{}.xml".into()),
                DRAWING_TYPE,
                format!("<xdr:wsDr xmlns:xdr=\"{XDR}\"/>").into_bytes(),
            )?;
            let rel = plan.add_relationship(
                &wb.pkg,
                Some(&sheet),
                REL_DRAWING,
                Target::Part(part.clone()),
            )?;
            let rels = Relationships::empty(Some(&part));
            (part, Some(rel), rels)
        };
        let (embed, media) = add_media(
            &wb.pkg,
            &mut plan,
            index,
            &part,
            &rels,
            image.format,
            &image.bytes,
        )?;
        let svg = image
            .svg
            .as_ref()
            .map(|svg| add_media(&wb.pkg, &mut plan, index, &part, &rels, Format::Svg, svg))
            .transpose()?;
        let hyperlink = image
            .hyperlink
            .as_ref()
            .map(|target| {
                plan.add_relationship(
                    &wb.pkg,
                    Some(&part),
                    REL_HYPERLINK,
                    Target::External(target.clone()),
                )
            })
            .transpose()?;
        let xml = position.picture(
            id,
            image,
            &embed,
            svg.as_ref().map(|(id, _)| id.as_str()),
            hyperlink.as_deref(),
        );
        let drawing = drawing::append(existing.as_ref(), &xml);
        let drawing = XmlDocument::parse_with(drawing.as_bytes(), &wb.pkg.parse_options())?;
        plan.stage_part(&wb.pkg, &part, drawing)?;
        wb.apply_registry_edit(plan, |wb| {
            if let Some(rel) = sheet_rel {
                wb.fault_point(ApplyStep::ImageDrawing)?;
                let doc = wb.pkg.xml_part_mut(&sheet)?;
                doc.lens_mut::<sml::Worksheet>()?
                    .insert_with_ns::<sml::Drawing>(&REL_NS)?
                    .set_attr(QName::new(R, "id"), &rel)?;
            }
            wb.objects.scanned_shapes.insert(sheet.clone());
            record_shape(wb, &sheet, id);
            let index = wb.objects.media.as_mut().expect("initialized");
            for (bytes, part) in std::iter::once((&image.bytes, media))
                .chain(image.svg.iter().zip(svg.map(|(_, part)| part)))
            {
                let candidates = index.entry(Sha256::digest(bytes).into()).or_default();
                if !candidates.contains(&part) {
                    candidates.push(part);
                }
            }
            Ok(())
        })?;
        Ok(id)
    }

    /// Lists pictures in drawing order, including pictures inside groups and compatibility branches.
    pub fn images(&mut self) -> Result<Vec<ImageInfo>> {
        self.wb.check_poisoned()?;
        let sheet = self.wb.sheets[self.sheet].part.clone().expect("worksheet");
        let Some(part) = drawing_target(&mut self.wb.pkg, &sheet)? else {
            return Ok(Vec::new());
        };
        let doc = drawing_document(&mut self.wb.pkg, &part)?;
        let rels = self.wb.pkg.relationships(&part)?;
        Ok(drawing::list(&self.wb.pkg, &rels, doc.root()))
    }

    /// Removes a top-level picture and any of its relationships and media that become unused.
    pub fn remove_image(&mut self, id: u32) -> Result<()> {
        let wb = &mut *self.wb;
        wb.check_editable()?;
        let sheet = wb.sheets[self.sheet].part.clone().expect("worksheet");
        let part = drawing_target(&mut wb.pkg, &sheet)?.ok_or(Error::NoSuchImage(id))?;
        let doc = drawing_document(&mut wb.pkg, &part)?;
        let (anchor, picture) = drawing::removable(doc.root(), id).ok_or(Error::NoSuchImage(id))?;
        let mut bytes = doc.source().as_bytes().to_vec();
        let span = anchor.span().expect("parsed anchor");
        bytes.drain(span.start..span.end());
        let rels = wb.pkg.relationships(&part)?;
        let mut plan = RegistryPlan::new(&wb.pkg);
        let edited = XmlDocument::parse_with(&bytes, &wb.pkg.parse_options())?;
        plan.stage_part(&wb.pkg, &part, edited)?;
        let mut used = HashSet::new();
        for rel_id in walk(picture)
            .flat_map(|node| node.attrs())
            .filter(|attribute| attribute.name().ns() == R)
            .map(|attribute| attribute.value())
            .filter(|id| used.insert(*id))
        {
            let Some(rel) = rels.by_id(rel_id) else {
                continue;
            };
            if !matches!(rel.rel_type.as_str(), REL_IMAGE | REL_HYPERLINK)
                || walk(doc.root())
                    .filter(|node| {
                        node.span().is_none_or(|other| {
                            other.start < span.start || other.start >= span.end()
                        })
                    })
                    .flat_map(|node| node.attrs())
                    .any(|attribute| attribute.name().ns() == R && attribute.value() == rel_id)
            {
                continue;
            }
            plan.remove_relationship(&wb.pkg, Some(&part), rel_id, &[REL_IMAGE, REL_HYPERLINK])?;
            if let Some(media) = media_part(&wb.pkg, &rels, rel_id)
                && plan.incoming_relationships(&wb.pkg, &media)?.is_empty()
            {
                plan.remove_parts(&wb.pkg, &[&media], &[REL_IMAGE])?;
            }
        }
        wb.apply_registry_edit(plan, |wb| {
            record_shape(wb, &sheet, id);
            Ok(())
        })
    }
}
