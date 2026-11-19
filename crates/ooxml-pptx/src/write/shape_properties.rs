//! Writes direct shape properties (ADR-0016 §6).

use super::preset_geometry::PRESET_ADJUSTMENTS;
use super::*;
use crate::Rect;
use crate::names::{A_NS, P_NS};
use crate::resolve::AffineAxis;

const fn a(local: &'static str) -> QName {
    QName::new(A_NS, local)
}
const fn p(local: &'static str) -> QName {
    QName::new(P_NS, local)
}
const fn un(local: &'static str) -> QName {
    QName::new("", local)
}
use ooxml_core::EnumType;
use ooxml_core::value::{parse_bool, parse_dml_percent, with_i64};
use ooxml_schema::dml::{
    AdjustValueList, ChildExtents, ChildOffset, Extents, Offset, PresetGeometry, ShapeGuide,
    Transform2D, TransformGroup,
};

/// A direct shape fill or a request to restore inheritance.
#[derive(Clone, Debug, PartialEq)]
pub enum ShapeFill {
    /// One solid DrawingML colour.
    Solid(ColorSpec),
    /// Explicitly transparent.
    NoFill,
    /// A linear gradient.
    Linear {
        /// Angle in 60000ths of a degree.
        angle: i32,
        /// Stops in document order, in thousandths of a percent.
        stops: Vec<(i32, ColorSpec)>,
    },
    /// Remove the direct fill.
    Inherit,
    /// Picture fills are unsupported by this setter.
    Picture,
    /// Pattern fills are unsupported by this setter.
    Pattern,
}

/// An editable group shape.
#[derive(Debug)]
pub struct GroupMut<'s, 'd> {
    shape: &'s mut ShapeMut<'d>,
}

const RECT_RANGE: Error = Error::ValueOutOfRange {
    what: "shape rectangle",
};
const FIT_WHAT: &str = "exact group fit";
const FIT_RANGE: Error = Error::ValueOutOfRange { what: FIT_WHAT };
const MAX_COORD: i64 = 27_273_042_316_900;

fn child(node: &RawNode, name: QName) -> Option<&RawNode> {
    node.elements().find(|node| node.is(name))
}

fn num(node: &RawNode, attr: &'static str) -> Option<i64> {
    node.attr(un(attr))?.parse().ok()
}

fn validate_rect(rect: Rect) -> Result<()> {
    if [rect.x, rect.y]
        .into_iter()
        .all(|v| (-27_273_042_329_600..=MAX_COORD).contains(&v))
        && [rect.cx, rect.cy]
            .into_iter()
            .all(|v| (0..=MAX_COORD).contains(&v))
    {
        Ok(())
    } else {
        Err(RECT_RANGE)
    }
}

fn read_box(node: &RawNode, offset: QName, extent: QName) -> Option<Rect> {
    let off = child(node, offset)?;
    let ext = child(node, extent)?;
    Some(Rect {
        x: num(off, "x")?,
        y: num(off, "y")?,
        cx: num(ext, "cx")?,
        cy: num(ext, "cy")?,
    })
}

fn rect(node: &RawNode) -> Option<Rect> {
    read_box(node, Offset::QNAME, Extents::QNAME)
}

fn set_num<E: ElementSpec>(
    node: &mut LensMut<'_, E>,
    attr: &'static str,
    value: i64,
) -> Result<()> {
    if num(node.node(), attr) != Some(value) {
        with_i64(value, |value| node.set_attr(un(attr), value))?;
    }
    Ok(())
}

fn write_box<E, O, X>(node: &mut LensMut<'_, E>, value: Rect) -> Result<()>
where
    E: ElementSpec,
    O: ChildOf<E>,
    X: ChildOf<E>,
{
    let mut off = node.get_or_insert::<O>()?;
    set_num(&mut off, "x", value.x)?;
    set_num(&mut off, "y", value.y)?;
    let mut ext = node.get_or_insert::<X>()?;
    set_num(&mut ext, "cx", value.cx)?;
    set_num(&mut ext, "cy", value.cy)
}

fn transform_parent(node: &RawNode) -> Option<&RawNode> {
    if node.is(GraphicFrame::QNAME) {
        Some(node)
    } else if node.is(GroupShape::QNAME) {
        child(node, p("grpSpPr"))
    } else {
        child(node, p("spPr"))
    }
}

fn xfrm(node: &RawNode) -> Option<&RawNode> {
    let name = if node.is(GraphicFrame::QNAME) {
        p("xfrm")
    } else {
        a("xfrm")
    };
    child(transform_parent(node)?, name)
}

fn checked_xfrm(node: &RawNode) -> Result<Option<&RawNode>> {
    let transform = xfrm(node);
    if transform_parent(node)
        .is_some_and(|parent| child(parent, ooxml_core::name::MC_ALTERNATE_CONTENT).is_some())
    {
        return Err(Error::InsideAlternateContent);
    }
    checked(transform)
}

fn graphics_error() -> Error {
    Error::UnsupportedContent("shape properties for graphic frame".into())
}

fn checked(node: Option<&RawNode>) -> Result<Option<&RawNode>> {
    fn has_mc(node: &RawNode) -> bool {
        node.is(ooxml_core::name::MC_ALTERNATE_CONTENT) || node.elements().any(has_mc)
    }
    if node.is_some_and(has_mc) {
        Err(Error::InsideAlternateContent)
    } else {
        Ok(node)
    }
}

fn props_mut<'d>(
    doc: &'d mut XmlDocument,
    path: &[usize],
) -> Result<LensMut<'d, pml::ShapeProperties>> {
    let node = node_at(doc.root(), path).ok_or(Error::ShapeNotFound)?;
    let (index, _) = node
        .indexed_elements()
        .find(|(_, node)| node.is(pml::ShapeProperties::QNAME))
        .ok_or_else(|| Error::UnsupportedContent("missing shape properties".into()))?;
    let mut path = path.to_vec();
    path.push(index);
    Ok(doc.lens_mut_at(&path)?)
}

fn edit_xfrm<E>(mut node: LensMut<'_, E>, edit: TransformEdit) -> Result<()>
where
    E: ElementSpec,
    Offset: ChildOf<E>,
    Extents: ChildOf<E>,
{
    match edit {
        TransformEdit::Rect(value) => write_box::<E, Offset, Extents>(&mut node, value),
        TransformEdit::Rotation(value) => {
            if transform_attr(node.node(), "rot", 0, |s| s.parse().ok()) != Some(value) {
                set_num(&mut node, "rot", i64::from(value))?;
            }
            Ok(())
        }
        TransformEdit::Flip(attr, value) => {
            if transform_attr(node.node(), attr, false, parse_bool) != Some(value) {
                node.set_attr(un(attr), ooxml_core::value::format_bool(value))?;
            }
            Ok(())
        }
    }
}

#[derive(Clone, Copy)]
enum TransformEdit {
    Rect(Rect),
    Rotation(i32),
    Flip(&'static str, bool),
}

fn transform_attr<T>(
    node: &RawNode,
    name: &'static str,
    default: T,
    parse: impl FnOnce(&str) -> Option<T>,
) -> Option<T> {
    match node.attr(un(name)) {
        Some(value) => parse(value),
        None => Some(default),
    }
}

impl TransformEdit {
    fn matches(self, node: Option<&RawNode>, inherited: Option<&crate::resolve::Xfrm>) -> bool {
        match self {
            Self::Rotation(value) => {
                let old = match (node, inherited) {
                    (Some(node), _) => transform_attr(node, "rot", 0, |s| s.parse().ok()),
                    (_, Some(inherited)) => inherited.rot.get(),
                    _ => Some(0),
                };
                old == Some(value)
            }
            Self::Flip(attr, value) => {
                let old = match (node, inherited) {
                    (Some(node), _) => transform_attr(node, attr, false, parse_bool),
                    (_, Some(inherited)) if attr == "flipH" => inherited.flip_h.get(),
                    (_, Some(inherited)) => inherited.flip_v.get(),
                    _ => Some(false),
                };
                old == Some(value)
            }
            Self::Rect(value) => node.and_then(rect) == Some(value),
        }
    }
}

fn materialize_placeholder<E>(
    node: &mut LensMut<'_, E>,
    inherited: crate::resolve::Xfrm,
    edit: TransformEdit,
) -> Result<()>
where
    E: ElementSpec,
    Offset: ChildOf<E>,
    Extents: ChildOf<E>,
{
    if !matches!(edit, TransformEdit::Rect(_)) {
        let value = Rect {
            x: inherited.x,
            y: inherited.y,
            cx: inherited.cx,
            cy: inherited.cy,
        };
        validate_rect(value)?;
        write_box::<_, Offset, Extents>(node, value)?;
    }
    if !matches!(edit, TransformEdit::Rotation(_)) && node.node().attr(un("rot")).is_none() {
        let rotation = inherited.rot.get().ok_or(RECT_RANGE)?;
        if rotation != 0 {
            set_num(node, "rot", i64::from(rotation))?;
        }
    }
    for (attr, value) in [("flipH", inherited.flip_h), ("flipV", inherited.flip_v)] {
        if !matches!(edit, TransformEdit::Flip(edited, _) if edited == attr)
            && node.node().attr(un(attr)).is_none()
            && value.get().ok_or(RECT_RANGE)?
        {
            node.set_attr(un(attr), "1")?;
        }
    }
    Ok(())
}

impl<'d> ShapeMut<'d> {
    /// Opens this shape as a group.
    pub fn group_mut(&mut self) -> Result<GroupMut<'_, 'd>> {
        if !self.node().is(GroupShape::QNAME) {
            return Err(Error::UnsupportedContent("expected a group shape".into()));
        }
        Ok(GroupMut { shape: self })
    }

    fn update_xfrm(&mut self, edit: TransformEdit) -> Result<()> {
        let shape = self.node();
        let transform = checked_xfrm(shape)?;
        let frame = shape.is(GraphicFrame::QNAME);
        if frame && matches!(edit, TransformEdit::Flip(..)) {
            return Err(graphics_error());
        }
        let group = shape.is(GroupShape::QNAME);
        let missing_box = transform
            .is_none_or(|node| child(node, a("off")).is_none() || child(node, a("ext")).is_none());
        let needs_inheritance = missing_box && !group && self.is_placeholder();
        let had_transform = transform.is_some();
        if had_transform && !needs_inheritance && edit.matches(transform, None) {
            return Ok(());
        }
        let group_inner = if group {
            match transform {
                None if !edit.matches(None, None) => {
                    Some(child_bounds(shape, "group member bounds")?)
                }
                Some(node)
                    if matches!(edit, TransformEdit::Rect(_))
                        && (child(node, a("chOff")).is_none()
                            || child(node, a("chExt")).is_none()) =>
                {
                    Some(group_boxes(node).ok_or(RECT_RANGE)?.1)
                }
                _ => None,
            }
        } else {
            None
        };
        let inherited = if needs_inheritance {
            let replacement = match edit {
                TransformEdit::Rect(rect) => Some(rect),
                _ => None,
            };
            self.inherited_xfrm(replacement)?
        } else {
            None
        };
        if !had_transform && edit.matches(None, inherited.as_ref()) {
            return Ok(());
        }
        let mut staged = self.doc.clone();
        if frame {
            let mut frame = staged.lens_mut_at::<GraphicFrame>(&self.path)?;
            let mut xfrm = frame.get_or_insert::<pml::Transform>()?;
            if needs_inheritance {
                xfrm.preserve_attribute_spelling();
                if let Some(inherited) = inherited {
                    materialize_placeholder(&mut xfrm, inherited, edit)?;
                }
            }
            edit_xfrm(xfrm, edit)?;
        } else if group {
            let mut group = staged.lens_mut_at::<GroupShape>(&self.path)?;
            let mut props = group.get_or_insert::<pml::GroupShapeProperties>()?;
            let mut xfrm = props.get_or_insert::<TransformGroup>()?;
            if let Some(inner) = group_inner {
                if !had_transform && !matches!(edit, TransformEdit::Rect(_)) {
                    write_box::<_, Offset, Extents>(&mut xfrm, inner)?;
                }
                write_box::<_, ChildOffset, ChildExtents>(&mut xfrm, inner)?;
            }
            edit_xfrm(xfrm, edit)?;
        } else {
            let mut props = props_mut(&mut staged, &self.path)?;
            let mut xfrm = props.get_or_insert::<Transform2D>()?;
            if needs_inheritance {
                xfrm.preserve_attribute_spelling();
                if let Some(inherited) = inherited {
                    materialize_placeholder(&mut xfrm, inherited, edit)?;
                }
            }
            edit_xfrm(xfrm, edit)?;
        }
        self.commit_shape_edit(staged);
        Ok(())
    }

    fn inherited_xfrm(
        &mut self,
        replacement: Option<Rect>,
    ) -> Result<Option<crate::resolve::Xfrm>> {
        let EditPart::Package { package, name } = &mut self.doc else {
            return Ok(None);
        };
        let main = package
            .main_document_part()
            .ok_or_else(|| Error::NotAPresentation("no officeDocument relationship".into()))?;
        let entries = [(self.slide_id, Some(name.clone()))];
        let crate::deck::Loaded {
            slides,
            rels,
            masters,
            layouts,
        } = crate::deck::load(package, &main, &entries)?;
        let pres = package
            .parsed_part(&main)
            .ok_or_else(|| Error::NotAPresentation(main.clone()))?;
        let deck = crate::Deck {
            pkg: package,
            pres_part: &main,
            pres,
            slides,
            rels,
            masters,
            layouts,
        };
        let slide = deck.slide(0)?;
        let shape = slide
            .slide()
            .shapes()
            .into_iter()
            .find(|shape| shape.path().as_slice() == self.path)
            .ok_or(Error::ShapeNotFound)?;
        let crate::Lookup::Set(value) = slide
            .sheet()
            .geometry_with_replacement(&shape, replacement)?
        else {
            return Ok(None);
        };
        Ok(Some(value.value.map_err(|_| RECT_RANGE)?))
    }

    fn commit_shape_edit(&mut self, staged: XmlDocument) {
        if staged.is_dirty() {
            *self.doc = staged;
        }
    }

    /// Sets position and size in the parent coordinate space.
    pub fn set_rect(&mut self, rect: Rect) -> Result<()> {
        validate_rect(rect)?;
        self.update_xfrm(TransformEdit::Rect(rect))
    }

    fn is_placeholder(&self) -> bool {
        read::shapes_of(&self.doc)
            .into_iter()
            .any(|shape| shape.path().as_slice() == self.path && shape.placeholder().is_some())
    }

    /// Removes the direct transform so a placeholder inherits its geometry.
    pub fn clear_rect(&mut self) -> Result<()> {
        let shape = self.node();
        let transform = checked_xfrm(shape)?;
        if shape.is(GroupShape::QNAME) || !self.is_placeholder() {
            return Err(Error::UnsupportedContent(
                "clear_rect on a non-placeholder".into(),
            ));
        }
        if transform.is_some() {
            let mut staged = self.doc.clone();
            if shape.is(GraphicFrame::QNAME) {
                staged
                    .lens_mut_at::<GraphicFrame>(&self.path)?
                    .remove::<pml::Transform>();
            } else {
                props_mut(&mut staged, &self.path)?.remove::<Transform2D>();
            }
            self.commit_shape_edit(staged);
        }
        Ok(())
    }

    /// Writes rotation in 60000ths of a degree.
    pub fn set_rotation(&mut self, angle: i32) -> Result<()> {
        self.update_xfrm(TransformEdit::Rotation(angle))
    }

    /// Sets horizontal flipping.
    pub fn set_flip_h(&mut self, value: bool) -> Result<()> {
        self.update_xfrm(TransformEdit::Flip("flipH", value))
    }

    /// Sets vertical flipping.
    pub fn set_flip_v(&mut self, value: bool) -> Result<()> {
        self.update_xfrm(TransformEdit::Flip("flipV", value))
    }

    /// Converts slide coordinates to the parent coordinate space, rounding once to EMU.
    pub fn set_slide_rect(&mut self, rect: Rect) -> Result<()> {
        validate_rect(rect)?;
        let (mut x, mut y, mut cx, mut cy) =
            (rect.x as f64, rect.y as f64, rect.cx as f64, rect.cy as f64);
        for k in 3..self.path.len() {
            let group = node_at(self.doc.root(), &self.path[..k]).ok_or(Error::ShapeNotFound)?;
            let Some(transform) = checked_xfrm(group)? else {
                continue;
            };
            if transformed(transform, false).ok_or(RECT_RANGE)? {
                return Err(Error::TransformedGroup {
                    group_id: raw_shape_id(group),
                });
            }
            let (outer, inner) = group_boxes(transform).ok_or(RECT_RANGE)?;
            let x_axis = AffineAxis::new(outer.x, outer.cx, inner.x, inner.cx).ok_or(RECT_RANGE)?;
            let y_axis = AffineAxis::new(outer.y, outer.cy, inner.y, inner.cy).ok_or(RECT_RANGE)?;
            if x_axis.scale <= 0.0 || y_axis.scale <= 0.0 {
                return Err(RECT_RANGE);
            }
            x = x_axis.unmap(x);
            y = y_axis.unmap(y);
            cx /= x_axis.scale;
            cy /= y_axis.scale;
        }
        if [x, y, cx, cy].into_iter().any(|value| !value.is_finite()) {
            return Err(RECT_RANGE);
        }
        self.set_rect(Rect {
            x: x.round() as i64,
            y: y.round() as i64,
            cx: cx.round() as i64,
            cy: cy.round() as i64,
        })
    }
}

fn group_boxes(transform: &RawNode) -> Option<(Rect, Rect)> {
    let outer = rect(transform)?;
    let pair = |name, x, y, fallback| match child(transform, name) {
        Some(node) => Some((num(node, x)?, num(node, y)?)),
        None => Some(fallback),
    };
    let (x, y) = pair(a("chOff"), "x", "y", (outer.x, outer.y))?;
    let (cx, cy) = pair(a("chExt"), "cx", "cy", (outer.cx, outer.cy))?;
    Some((outer, Rect { x, y, cx, cy }))
}

fn transformed(node: &RawNode, any_rotation: bool) -> Option<bool> {
    let rotation = match node.attr(un("rot")) {
        Some(value) => value.parse::<i64>().ok()?,
        None => 0,
    };
    let flip = |attr| match node.attr(un(attr)) {
        Some(value) => parse_bool(value),
        None => Some(false),
    };
    let (flip_h, flip_v) = (flip("flipH")?, flip("flipV")?);
    Some(
        if any_rotation {
            rotation != 0
        } else {
            rotation % 21_600_000 != 0
        } || flip_h
            || flip_v,
    )
}

fn fit_axis(
    old_off: i64,
    old_ext: i64,
    inner_off: i64,
    inner_ext: i64,
    start: i64,
    size: i64,
) -> Result<(i64, i64)> {
    if inner_ext <= 0 || size <= 0 || old_ext < 0 {
        return Err(FIT_RANGE);
    }
    let scale = |n: i128| {
        let product = n.checked_mul(i128::from(old_ext)).ok_or(FIT_RANGE)?;
        let divisor = i128::from(inner_ext);
        if product % divisor != 0 {
            return Err(FIT_RANGE);
        }
        Ok(product / divisor)
    };
    let start = i128::from(old_off)
        .checked_add(scale(i128::from(start) - i128::from(inner_off))?)
        .and_then(|n| i64::try_from(n).ok())
        .ok_or(FIT_RANGE)?;
    let size = i64::try_from(scale(i128::from(size))?).map_err(|_| FIT_RANGE)?;
    Ok((start, size))
}

fn child_bounds(group: &RawNode, what: &'static str) -> Result<Rect> {
    let range_error = || Error::ValueOutOfRange { what };
    let mut bounds: Option<(i64, i64, i64, i64)> = None;
    for shape in group
        .elements()
        .filter(|node| read::kind_of(node).is_some())
    {
        if shape.is(ooxml_core::name::MC_ALTERNATE_CONTENT) {
            return Err(Error::InsideAlternateContent);
        }
        let value = checked_xfrm(shape)?
            .and_then(rect)
            .ok_or_else(range_error)?;
        validate_rect(value)?;
        let right = value.x.checked_add(value.cx).ok_or_else(range_error)?;
        let bottom = value.y.checked_add(value.cy).ok_or_else(range_error)?;
        bounds = Some(match bounds {
            Some((x, y, r, b)) => (x.min(value.x), y.min(value.y), r.max(right), b.max(bottom)),
            None => (value.x, value.y, right, bottom),
        });
    }
    let (x, y, r, b) = bounds.ok_or_else(range_error)?;
    let result = Rect {
        x,
        y,
        cx: r.checked_sub(x).ok_or_else(range_error)?,
        cy: b.checked_sub(y).ok_or_else(range_error)?,
    };
    validate_rect(result)?;
    if result.cx == 0 || result.cy == 0 {
        return Err(range_error());
    }
    Ok(result)
}

impl GroupMut<'_, '_> {
    /// Fits the group box around its immediate children, preserving the affine transform exactly.
    pub fn fit_to_children(&mut self) -> Result<()> {
        let shape = self.shape.node();
        let inner = child_bounds(shape, FIT_WHAT)?;
        let outer = if let Some(transform) = checked_xfrm(shape)? {
            let (old_outer, old_inner) = group_boxes(transform).ok_or(FIT_RANGE)?;
            if transformed(transform, true).ok_or(FIT_RANGE)? {
                if old_inner == inner {
                    return Ok(());
                }
                return Err(Error::TransformedGroup {
                    group_id: raw_shape_id(shape),
                });
            }
            let (x, cx) = fit_axis(
                old_outer.x,
                old_outer.cx,
                old_inner.x,
                old_inner.cx,
                inner.x,
                inner.cx,
            )?;
            let (y, cy) = fit_axis(
                old_outer.y,
                old_outer.cy,
                old_inner.y,
                old_inner.cy,
                inner.y,
                inner.cy,
            )?;
            let outer = Rect { x, y, cx, cy };
            if old_outer == outer
                && read_box(transform, ChildOffset::QNAME, ChildExtents::QNAME) == Some(inner)
            {
                return Ok(());
            }
            outer
        } else {
            inner
        };
        validate_rect(outer)?;
        let mut staged = self.shape.doc.clone();
        let mut group = staged.lens_mut_at::<GroupShape>(&self.shape.path)?;
        let mut props = group.get_or_insert::<pml::GroupShapeProperties>()?;
        let mut xfrm = props.get_or_insert::<TransformGroup>()?;
        write_box::<_, Offset, Extents>(&mut xfrm, outer)?;
        write_box::<_, ChildOffset, ChildExtents>(&mut xfrm, inner)?;
        self.shape.commit_shape_edit(staged);
        Ok(())
    }
}

impl ShapeFill {
    fn validate(&self) -> Result<()> {
        match self {
            Self::Picture | Self::Pattern => Err(Error::UnsupportedFill),
            Self::Solid(color) => color.validate(),
            Self::Linear { angle, stops } => {
                if !(0..21_600_000).contains(angle) {
                    return Err(Error::ValueOutOfRange {
                        what: "gradient angle",
                    });
                }
                if stops.len() < 2 || stops.iter().any(|(pos, _)| !(0..=100_000).contains(pos)) {
                    return Err(Error::ValueOutOfRange {
                        what: "gradient stops",
                    });
                }
                stops.iter().try_for_each(|(_, color)| color.validate())
            }
            _ => Ok(()),
        }
    }

    fn matches(&self, node: &RawNode) -> bool {
        match self {
            Self::NoFill => node.is(a("noFill")),
            Self::Solid(color) => node.is(a("solidFill")) && color.matches(node),
            Self::Linear { angle, stops } => {
                let linear = child(node, a("lin"));
                let list = child(node, a("gsLst"));
                node.is(a("gradFill"))
                    && linear.and_then(|node| num(node, "ang")) == Some(i64::from(*angle))
                    && list.is_some_and(|list| {
                        list.elements().count() == stops.len()
                            && list.elements().zip(stops).all(|(node, (pos, color))| {
                                node.is(a("gs"))
                                    && node
                                        .attr(un("pos"))
                                        .and_then(parse_dml_percent)
                                        .is_some_and(|value| {
                                            value.percent() == f64::from(*pos) / 1000.0
                                        })
                                    && color.matches(node)
                            })
                    })
            }
            _ => false,
        }
    }

    fn write<E: ElementSpec>(&self, props: &mut LensMut<'_, E>) -> Result<()>
    where
        dml::SolidFill: ChildOf<E>,
        dml::NoFill: ChildOf<E>,
        dml::GradientFill: ChildOf<E>,
    {
        match self {
            Self::Solid(color) => color.write(&mut props.insert::<dml::SolidFill>()?),
            Self::NoFill => {
                props.insert::<dml::NoFill>()?;
                Ok(())
            }
            Self::Linear { angle, stops } => {
                let mut gradient = props.insert::<dml::GradientFill>()?;
                let mut list = gradient.insert::<dml::GradientStopList>()?;
                for (pos, color) in stops {
                    let mut stop = dml::GradientStopViewMut(list.insert::<dml::GradientStop>()?);
                    stop.set_position(*pos)?;
                    color.write(&mut stop.0)?;
                }
                dml::LinearGradientFillViewMut(gradient.insert::<dml::LinearGradientFill>()?)
                    .set_angle(*angle)?;
                Ok(())
            }
            Self::Inherit => Ok(()),
            Self::Picture | Self::Pattern => Err(Error::UnsupportedFill),
        }
    }
}

fn update_fill<E: ElementSpec>(props: &mut LensMut<'_, E>, fill: &ShapeFill) -> Result<()>
where
    dml::SolidFill: ChildOf<E>,
    dml::NoFill: ChildOf<E>,
    dml::GradientFill: ChildOf<E>,
{
    formatting::remove_choices(props, &formatting::FILL);
    fill.write(props)
}

impl ShapeMut<'_> {
    /// Sets a direct fill or removes it to restore inheritance.
    pub fn set_fill(&mut self, fill: ShapeFill) -> Result<()> {
        let shape = self.node();
        if shape.is(GraphicFrame::QNAME) {
            return Err(graphics_error());
        }
        fill.validate()?;
        let group = shape.is(GroupShape::QNAME);
        let props = child(shape, if group { p("grpSpPr") } else { p("spPr") });
        let props = checked(props)?;
        let unchanged = {
            let mut choices = props
                .into_iter()
                .flat_map(RawNode::elements)
                .filter(|node| formatting::FILL.iter().any(|&name| node.is(name)));
            match choices.next() {
                None => matches!(fill, ShapeFill::Inherit),
                Some(old) => fill.matches(old) && choices.next().is_none(),
            }
        };
        if unchanged {
            return Ok(());
        }
        let mut staged = self.doc.clone();
        if group {
            let mut group = staged.lens_mut_at::<GroupShape>(&self.path)?;
            update_fill(
                &mut group.get_or_insert::<pml::GroupShapeProperties>()?,
                &fill,
            )?;
        } else {
            update_fill(&mut props_mut(&mut staged, &self.path)?, &fill)?;
        }
        self.commit_shape_edit(staged);
        Ok(())
    }

    /// Updates the specified fields of a direct outline. Passing `None` restores line inheritance.
    pub fn set_line(&mut self, line: Option<&LineSpec>) -> Result<()> {
        let shape = self.node();
        if shape.is(GraphicFrame::QNAME) {
            return Err(graphics_error());
        }
        if shape.is(GroupShape::QNAME) {
            return Err(Error::UnsupportedContent("group outline".into()));
        }
        if let Some(line) = line {
            line.validate()?;
        }
        let props = checked(child(shape, p("spPr")))?;
        let outlines = props
            .into_iter()
            .flat_map(RawNode::elements)
            .filter(|node| node.is(dml::Outline::QNAME))
            .count();
        if outlines > 1 {
            return Err(Error::UnsupportedContent("multiple shape outlines".into()));
        }
        if line.is_none() && outlines == 0 {
            return Ok(());
        }
        let mut staged = self.doc.clone();
        let mut props = props_mut(&mut staged, &self.path)?;
        if let Some(line) = line {
            line.update_outline(&mut props.get_or_insert::<dml::Outline>()?)?;
        } else {
            props.remove::<dml::Outline>();
        }
        self.commit_shape_edit(staged);
        Ok(())
    }

    fn preset(&self) -> Result<Option<&RawNode>> {
        let shape = self.node();
        if shape.is(GraphicFrame::QNAME) {
            return Err(graphics_error());
        }
        if shape.is(GroupShape::QNAME) {
            return Err(Error::UnsupportedContent("group geometry".into()));
        }
        let props = checked(child(shape, p("spPr")))?;
        if props.is_some_and(|node| child(node, dml::CustomGeometry::QNAME).is_some()) {
            return Err(Error::UnsupportedGeometry);
        }
        Ok(props.and_then(|node| child(node, PresetGeometry::QNAME)))
    }

    /// Writes a preset shape. Changing the preset removes its old adjustment list.
    pub fn set_preset(&mut self, preset: dml::ShapeTypeValues) -> Result<()> {
        if self
            .preset()?
            .and_then(|node| node.attr(PresetGeometry::ATTR_PRST))
            == Some(preset.as_str())
        {
            return Ok(());
        }
        let mut staged = self.doc.clone();
        let mut props = props_mut(&mut staged, &self.path)?;
        let mut geometry = dml::PresetGeometryViewMut(props.get_or_insert::<PresetGeometry>()?);
        geometry.set_preset(preset)?;
        geometry.remove_adjust_value_list();
        self.commit_shape_edit(staged);
        Ok(())
    }

    /// Writes a constant adjustment of the current preset, including any
    /// aspect-ratio guides (`hf` and `vf`) declared by that preset.
    pub fn set_adjustment(&mut self, name: &str, value: i64) -> Result<()> {
        let geometry = self.preset()?;
        let preset = geometry.and_then(|node| node.attr(PresetGeometry::ATTR_PRST));
        let valid = preset.and_then(|preset| {
            let index = PRESET_ADJUSTMENTS
                .binary_search_by_key(&preset, |(name, _)| *name)
                .ok()?;
            Some(
                PRESET_ADJUSTMENTS[index]
                    .1
                    .iter()
                    .any(|(guide, _)| *guide == name),
            )
        });
        if valid != Some(true) {
            return Err(Error::UnknownAdjustment { name: name.into() });
        }
        let named = |node: &RawNode| node.attr(ShapeGuide::ATTR_NAME) == Some(name);
        let mut staged = self.doc.clone();
        let mut props = props_mut(&mut staged, &self.path)?;
        let mut geometry = props.child_mut::<PresetGeometry>().expect("checked preset");
        let mut list = geometry.get_or_insert::<AdjustValueList>()?;
        let mut guide = if list
            .node()
            .elements()
            .any(|node| node.is(ShapeGuide::QNAME) && named(node))
        {
            list.child_mut_by::<ShapeGuide>(named)
                .expect("checked guide")
        } else {
            let mut guide = list.insert::<ShapeGuide>()?;
            guide.set_attr(ShapeGuide::ATTR_NAME, name)?;
            guide
        };
        let formula = guide
            .node()
            .attr(ShapeGuide::ATTR_FMLA)
            .and_then(|formula| formula.strip_prefix("val "))
            .and_then(|value| value.parse().ok());
        if formula != Some(value) {
            guide.set_attr(ShapeGuide::ATTR_FMLA, &format!("val {value}"))?;
        }
        self.commit_shape_edit(staged);
        Ok(())
    }
}
