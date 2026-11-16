//! Read side: slides, shapes, text (ADR-0005 §3). Reading never changes a
//! part's state. Only **direct** values are returned (ADR-0005 §4).

use std::borrow::Cow;

use ooxml_core::{ElementSpec, EnumValue, Lens, QName, RawNode, XmlDocument};
use ooxml_schema::dml::{self, Break, Field, GraphicData, Paragraph, Run, Table, TableCell, TableCellView, TableRow, Text};
use ooxml_schema::pml::{
    self, ConnectionShape, ContentPart, GraphicFrame, GroupShape, Picture, PlaceholderShape, PlaceholderShapeView,
    PlaceholderValues, Shape, ShapeTree,
};

use crate::error::{Error, Result};
use crate::names::TABLE_URI;

/// One entry of `p:sldIdLst`, in presentation order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlideInfo {
    /// Position in `p:sldIdLst` (0-based).
    pub index: usize,
    /// `p:sldId@id`.
    pub slide_id: u32,
    /// The slide part's name (zip entry name, no leading `/`).
    pub part_name: String,
}

/// Child-index path from the slide root (`p:sld`) to a shape element, as
/// in ADR-0002 "node id". Stable as long as the slide is not edited
/// structurally above the shape (Phase 0 never adds or removes shapes).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ShapePath(pub(crate) Vec<usize>);

impl ShapePath {
    /// The raw child indices.
    pub fn as_slice(&self) -> &[usize] {
        &self.0
    }
}

/// What kind of shape a [`ShapeRef`] is.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ShapeKind {
    /// `p:sp` (text box, placeholder, autoshape).
    Shape,
    /// `p:grpSp`; its members follow it in [`Slide::shapes`].
    Group,
    /// `p:graphicFrame` holding an `a:tbl`.
    Table,
    /// Any other `p:graphicFrame` (chart, SmartArt, OLE, …): opaque.
    GraphicFrame {
        /// `a:graphicData@uri`.
        uri: Option<String>,
    },
    /// `p:pic`: opaque.
    Picture,
    /// `p:cxnSp`: opaque.
    Connector,
    /// `p:contentPart`: opaque.
    ContentPart,
    /// `mc:AlternateContent` in the shape tree: one read-only entry for all
    /// branches (ADR-0005 §3). Its id/name are those of the first shape of
    /// `mc:Fallback`, else of the first `mc:Choice`.
    AlternateContent,
    /// An element of the shape tree Phase 0 does not know.
    Other(String),
}

/// A shape on a slide (read-only).
#[derive(Clone, Debug)]
pub struct ShapeRef<'a> {
    pub(crate) node: &'a RawNode,
    pub(crate) path: ShapePath,
    pub(crate) kind: ShapeKind,
    pub(crate) depth: usize,
}

fn first_element(n: &RawNode, pred: impl Fn(&RawNode) -> bool) -> Option<&RawNode> {
    n.elements().find(|c| pred(c))
}

/// The `cNvPr` of a shape element, found structurally: the first child
/// whose local name starts with `nv` (`p:nvSpPr`, `p:nvPicPr`,
/// `p14:nvContentPartPr`, …), then its `cNvPr` child.
pub(crate) fn c_nv_pr(shape: &RawNode) -> Option<&RawNode> {
    let nv = first_element(shape, |c| c.name().local().starts_with("nv"))?;
    first_element(nv, |c| c.name().local() == "cNvPr")
}

/// `cNvPr@id` of a shape element.
pub(crate) fn raw_shape_id(shape: &RawNode) -> Option<u32> {
    c_nv_pr(shape)?.attr(QName::new("", "id")).and_then(ooxml_core::value::parse_u32)
}

/// For an `mc:AlternateContent` in the shape tree: the shape element that
/// represents it (first shape of `mc:Fallback`, else of the first `mc:Choice`).
pub(crate) fn ac_representative(ac: &RawNode) -> Option<&RawNode> {
    let branch = ac
        .elements()
        .find(|b| b.name().local() == "Fallback")
        .or_else(|| ac.elements().find(|b| b.name().local() == "Choice"))?;
    branch.elements().next()
}

/// Every shape id an `mc:AlternateContent` carries (top-level shapes of
/// all branches), deduplicated: the AC counts as one shape.
pub(crate) fn ac_ids(ac: &RawNode) -> Vec<u32> {
    let mut ids: Vec<u32> = ac.elements().flat_map(|b| b.elements()).filter_map(raw_shape_id).collect();
    ids.sort_unstable();
    ids.dedup();
    ids
}

fn graphic_data(frame: &RawNode) -> Option<&RawNode> {
    let g = first_element(frame, |c| c.is(dml::Graphic::QNAME))?;
    first_element(g, |c| c.is(GraphicData::QNAME))
}

pub(crate) fn frame_table(frame: &RawNode) -> Option<&RawNode> {
    let gd = graphic_data(frame)?;
    if gd.attr(QName::new("", "uri")) != Some(TABLE_URI) {
        return None;
    }
    first_element(gd, |c| c.is(Table::QNAME))
}

pub(crate) fn kind_of(n: &RawNode) -> Option<ShapeKind> {
    let q = |k: QName| n.is(k);
    Some(if q(Shape::QNAME) {
        ShapeKind::Shape
    } else if q(GroupShape::QNAME) {
        ShapeKind::Group
    } else if q(GraphicFrame::QNAME) {
        if frame_table(n).is_some() {
            ShapeKind::Table
        } else {
            ShapeKind::GraphicFrame { uri: graphic_data(n).and_then(|g| g.attr(QName::new("", "uri"))).map(String::from) }
        }
    } else if q(Picture::QNAME) {
        ShapeKind::Picture
    } else if q(ConnectionShape::QNAME) {
        ShapeKind::Connector
    } else if q(ContentPart::QNAME) {
        ShapeKind::ContentPart
    } else if q(ooxml_core::name::MC_ALTERNATE_CONTENT) {
        ShapeKind::AlternateContent
    } else if n.name().ns() == crate::names::P_NS && matches!(n.name().local(), "nvGrpSpPr" | "grpSpPr" | "extLst") {
        return None; // group properties, not shapes
    } else {
        ShapeKind::Other(n.name().local().to_owned())
    })
}

impl<'a> ShapeRef<'a> {
    /// Path from the slide root, for [`crate::SlideMut::shape_mut`].
    pub fn path(&self) -> &ShapePath {
        &self.path
    }

    /// Kind of shape.
    pub fn kind(&self) -> &ShapeKind {
        &self.kind
    }

    /// Group nesting depth (0 = directly in `p:spTree`).
    pub fn depth(&self) -> usize {
        self.depth
    }

    /// The raw element (`p:sp`, `p:grpSp`, …, or `mc:AlternateContent`).
    pub fn raw(&self) -> &'a RawNode {
        self.node
    }

    fn representative(&self) -> Option<&'a RawNode> {
        if self.kind == ShapeKind::AlternateContent { ac_representative(self.node) } else { Some(self.node) }
    }

    /// `cNvPr@id`. [`Error::InvalidShapeId`] if missing or not a number.
    pub fn id(&self) -> Result<u32> {
        self.representative().and_then(raw_shape_id).ok_or(Error::InvalidShapeId)
    }

    /// `cNvPr@name`.
    pub fn name(&self) -> Option<&'a str> {
        c_nv_pr(self.representative()?)?.attr(QName::new("", "name"))
    }

    /// The shape's own `p:nvPr/p:ph` (direct value; no layout lookup).
    pub fn placeholder(&self) -> Option<PlaceholderRef<'a>> {
        if self.kind == ShapeKind::AlternateContent {
            return None;
        }
        let nv = first_element(self.node, |c| c.name().local().starts_with("nv"))?;
        let nv_pr = first_element(nv, |c| c.is(pml::ApplicationNonVisualDrawingProperties::QNAME))?;
        let ph = first_element(nv_pr, |c| c.is(PlaceholderShape::QNAME))?;
        Lens::root(ph).ok().map(|l| PlaceholderRef(PlaceholderShapeView(l)))
    }

    /// The shape's `p:txBody`, if it is a `p:sp` with one.
    pub fn text_body(&self) -> Option<TextBodyRef<'a>> {
        if self.kind != ShapeKind::Shape {
            return None;
        }
        first_element(self.node, |c| c.is(pml::TextBody::QNAME)).map(|n| TextBodyRef { node: n })
    }

    /// The table of a table graphic frame.
    pub fn table(&self) -> Option<TableRef<'a>> {
        if self.kind != ShapeKind::Table {
            return None;
        }
        frame_table(self.node).map(|n| TableRef { node: n })
    }
}

/// A shape's own placeholder element (`p:ph`), read through the generated
/// [`PlaceholderShapeView`]; absent attributes take their XSD defaults.
#[derive(Clone, Debug)]
pub struct PlaceholderRef<'a>(PlaceholderShapeView<'a>);

impl<'a> PlaceholderRef<'a> {
    /// `@type` (absent → `obj`).
    pub fn kind(&self) -> EnumValue<'a, PlaceholderValues> {
        self.0.type_()
    }
    /// `@idx` (absent → 0).
    pub fn idx(&self) -> Result<u32> {
        Ok(self.0.index()?)
    }
    /// The generated view.
    pub fn view(&self) -> &PlaceholderShapeView<'a> {
        &self.0
    }
}

/// A text body (`p:txBody` or a table cell's `a:txBody`).
#[derive(Clone, Copy, Debug)]
pub struct TextBodyRef<'a> {
    pub(crate) node: &'a RawNode,
}

impl<'a> TextBodyRef<'a> {
    /// Paragraphs (`a:p`) in order.
    pub fn paragraphs(&self) -> Vec<ParagraphRef<'a>> {
        self.node.elements().filter(|c| c.is(Paragraph::QNAME)).map(|n| ParagraphRef { node: n }).collect()
    }
    /// Paragraph texts joined by `\n` (see [`ParagraphRef::text`]).
    pub fn text(&self) -> String {
        self.paragraphs().iter().map(ParagraphRef::text).collect::<Vec<_>>().join("\n")
    }
    /// The raw element.
    pub fn raw(&self) -> &'a RawNode {
        self.node
    }
}

/// One `a:p`.
#[derive(Clone, Copy, Debug)]
pub struct ParagraphRef<'a> {
    pub(crate) node: &'a RawNode,
}

/// A content item of a paragraph, in document order.
#[derive(Clone, Copy, Debug)]
pub enum TextItem<'a> {
    /// `a:r`.
    Run(RunRef<'a>),
    /// `a:br`.
    Break,
    /// `a:fld` (text is Office's cached value).
    Field(FieldRef<'a>),
    /// Anything else in the run slot (`a14:m`, `mc:AlternateContent`, …).
    Other(&'a RawNode),
}

impl<'a> ParagraphRef<'a> {
    /// Items (runs, breaks, fields, other); `a:pPr`/`a:endParaRPr` are not items.
    pub fn items(&self) -> Vec<TextItem<'a>> {
        self.node
            .elements()
            .filter_map(|c| {
                Some(if c.is(Run::QNAME) {
                    TextItem::Run(RunRef { node: c })
                } else if c.is(Break::QNAME) {
                    TextItem::Break
                } else if c.is(Field::QNAME) {
                    TextItem::Field(FieldRef { node: c })
                } else if c.is(dml::ParagraphProperties::QNAME) || c.is(dml::EndParagraphRunProperties::QNAME) {
                    return None;
                } else {
                    TextItem::Other(c)
                })
            })
            .collect()
    }
    /// The runs (`a:r`) only.
    pub fn runs(&self) -> Vec<RunRef<'a>> {
        self.node.elements().filter(|c| c.is(Run::QNAME)).map(|n| RunRef { node: n }).collect()
    }
    /// Run texts, `a:br` as `\v`, fields by their cached text (python-pptx's
    /// `_Paragraph.text` convention).
    pub fn text(&self) -> String {
        let mut s = String::new();
        for it in self.items() {
            match it {
                TextItem::Run(r) => s.push_str(&r.text()),
                TextItem::Break => s.push('\u{b}'),
                TextItem::Field(f) => s.push_str(&f.text()),
                TextItem::Other(_) => {}
            }
        }
        s
    }
    /// The raw element.
    pub fn raw(&self) -> &'a RawNode {
        self.node
    }
}

/// One `a:r`.
#[derive(Clone, Copy, Debug)]
pub struct RunRef<'a> {
    pub(crate) node: &'a RawNode,
}

impl<'a> RunRef<'a> {
    /// The `a:t` text.
    pub fn text(&self) -> Cow<'a, str> {
        self.node.elements().find(|c| c.is(Text::QNAME)).map(RawNode::text).unwrap_or_default()
    }
    /// The raw element.
    pub fn raw(&self) -> &'a RawNode {
        self.node
    }
}

/// One `a:fld` (read-only).
#[derive(Clone, Copy, Debug)]
pub struct FieldRef<'a> {
    pub(crate) node: &'a RawNode,
}

impl<'a> FieldRef<'a> {
    /// `@type` (`slidenum`, `datetime1`, …).
    pub fn field_type(&self) -> Option<&'a str> {
        self.node.attr(QName::new("", "type"))
    }
    /// The cached text Office wrote.
    pub fn text(&self) -> Cow<'a, str> {
        self.node.elements().find(|c| c.is(Text::QNAME)).map(RawNode::text).unwrap_or_default()
    }
    /// The raw element.
    pub fn raw(&self) -> &'a RawNode {
        self.node
    }
}

/// An `a:tbl`.
#[derive(Clone, Copy, Debug)]
pub struct TableRef<'a> {
    pub(crate) node: &'a RawNode,
}

impl<'a> TableRef<'a> {
    /// Rows (`a:tr`).
    pub fn rows(&self) -> Vec<Vec<CellRef<'a>>> {
        self.node
            .elements()
            .filter(|c| c.is(TableRow::QNAME))
            .map(|r| r.elements().filter(|c| c.is(TableCell::QNAME)).map(|n| CellRef { node: n }).collect())
            .collect()
    }
    /// The cell at (row, col): `col` counts `a:tc` elements in the row.
    pub fn cell(&self, row: usize, col: usize) -> Option<CellRef<'a>> {
        self.rows().get(row)?.get(col).copied()
    }
}

/// An `a:tc`.
#[derive(Clone, Copy, Debug)]
pub struct CellRef<'a> {
    pub(crate) node: &'a RawNode,
}

impl<'a> CellRef<'a> {
    fn view(&self) -> Option<TableCellView<'a>> {
        Lens::root(self.node).ok().map(TableCellView)
    }
    /// The cell's `a:txBody`.
    pub fn text_body(&self) -> Option<TextBodyRef<'a>> {
        self.node.elements().find(|c| c.is(dml::TextBody::QNAME)).map(|n| TextBodyRef { node: n })
    }
    /// `hMerge` or `vMerge` is true: covered by a neighbour's span.
    pub fn is_merged(&self) -> Result<bool> {
        let v = self.view().ok_or(Error::NotATable)?;
        Ok(v.horizontal_merge()? || v.vertical_merge()?)
    }
    /// `gridSpan` (default 1).
    pub fn grid_span(&self) -> Result<i32> {
        Ok(self.view().ok_or(Error::NotATable)?.grid_span()?)
    }
    /// `rowSpan` (default 1).
    pub fn row_span(&self) -> Result<i32> {
        Ok(self.view().ok_or(Error::NotATable)?.row_span()?)
    }
}

/// A slide, read-only.
#[derive(Debug)]
pub struct Slide<'a> {
    pub(crate) doc: &'a XmlDocument,
    pub(crate) info: SlideInfo,
}

pub(crate) fn walk<'a>(n: &'a RawNode, path: &mut Vec<usize>, depth: usize, out: &mut Vec<ShapeRef<'a>>) {
    for (i, c) in n.indexed_elements() {
        let Some(kind) = kind_of(c) else { continue };
        path.push(i);
        let is_group = kind == ShapeKind::Group;
        out.push(ShapeRef { node: c, path: ShapePath(path.clone()), kind, depth });
        if is_group {
            walk(c, path, depth + 1, out);
        }
        path.pop();
    }
}

pub(crate) fn shapes_of(doc: &XmlDocument) -> Vec<ShapeRef<'_>> {
    let root = doc.root();
    let mut out = Vec::new();
    let Some((ci, csld)) = root.indexed_elements().find(|(_, c)| c.is(pml::CommonSlideData::QNAME)) else {
        return out;
    };
    let Some((ti, tree)) = csld.indexed_elements().find(|(_, c)| c.is(ShapeTree::QNAME)) else {
        return out;
    };
    let mut path = vec![ci, ti];
    walk(tree, &mut path, 0, &mut out);
    out
}

/// Every id a shape entry contributes to the duplicate check.
pub(crate) fn entry_ids(s: &ShapeRef<'_>) -> Vec<u32> {
    if s.kind == ShapeKind::AlternateContent { ac_ids(s.node) } else { raw_shape_id(s.node).into_iter().collect() }
}

pub(crate) fn find_by_id<'a>(shapes: Vec<ShapeRef<'a>>, id: u32) -> Result<Option<ShapeRef<'a>>> {
    let mut found = shapes.into_iter().filter(|s| entry_ids(s).contains(&id));
    let first = found.next();
    if found.next().is_some() {
        return Err(Error::AmbiguousShapeId { id });
    }
    Ok(first)
}

impl<'a> Slide<'a> {
    /// Position, `sldId` and part name.
    pub fn info(&self) -> &SlideInfo {
        &self.info
    }

    /// All shapes, depth-first in document order (= z-order); a group's
    /// members follow the group. Shapes inside `mc:AlternateContent` are not
    /// listed separately (one [`ShapeKind::AlternateContent`] entry).
    pub fn shapes(&self) -> Vec<ShapeRef<'a>> {
        shapes_of(self.doc)
    }

    /// The shape with this `cNvPr@id`. An `mc:AlternateContent` entry counts
    /// once for the ids of all its branches.
    /// [`Error::AmbiguousShapeId`] if two entries carry the id.
    pub fn shape_by_id(&self, id: u32) -> Result<Option<ShapeRef<'a>>> {
        find_by_id(self.shapes(), id)
    }

    /// Shapes with their own `p:ph`.
    pub fn placeholders(&self) -> Vec<ShapeRef<'a>> {
        self.shapes().into_iter().filter(|s| s.placeholder().is_some()).collect()
    }

    /// The parsed slide part.
    pub fn document(&self) -> &'a XmlDocument {
        self.doc
    }
}
