//! Hyperlink and action writes (ADR-0016 §7).

use std::sync::Arc;

use iri_string::types::IriReferenceStr;
use ooxml_core::lens::{NsDecl, ext_uri};
use ooxml_core::opc::{Relationships, TargetMode};
use ooxml_core::package::{RegistryPlan, RegistryRefusal, Target};
use ooxml_schema::dml::{
    HyperlinkColor, HyperlinkColorEnum, HyperlinkColorViewMut, HyperlinkExtension,
    HyperlinkExtensionList, HyperlinkExtensionViewMut, HyperlinkOnClick, HyperlinkOnHover,
    HyperlinkOnMouseOver,
};

use super::*;
use crate::names::{R_NS, REL_HYPERLINK, REL_SLIDE};
use crate::{LinkEvent, LinkTarget, Presentation, ShowJump};

/// A slide destination scoped to the presentation session it came from.
#[derive(Clone, Debug)]
pub struct SlideLinkTarget {
    slide_id: u32,
    identity: Arc<()>,
}

impl SlideLinkTarget {
    /// Stable `p:sldId@id`, independent of slide order.
    pub fn slide_id(&self) -> u32 {
        self.slide_id
    }
}

impl PartialEq for SlideLinkTarget {
    fn eq(&self, other: &Self) -> bool {
        self.slide_id == other.slide_id && Arc::ptr_eq(&self.identity, &other.identity)
    }
}

impl Eq for SlideLinkTarget {}

impl Presentation {
    /// Creates a write target by stable slide id. The link setter checks that
    /// this slide still exists when the target is used.
    pub fn slide_link_target(&self, slide_id: u32) -> LinkTarget {
        LinkTarget::SlideId(SlideLinkTarget {
            slide_id,
            identity: self.link_identity.clone(),
        })
    }
}

/// The direct link or action to write.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkSpec {
    /// A URL, presentation-scoped slide id, show jump, or no action.
    pub target: LinkTarget,
    /// Optional hover text.
    pub tooltip: Option<String>,
    /// Whether a click highlights the link.
    pub highlight_click: bool,
    /// Keep the run's own text colour. Applies to runs and fields.
    pub text_color: bool,
}

impl LinkSpec {
    /// A link using the default appearance and no tooltip.
    pub fn new(target: LinkTarget) -> Self {
        Self {
            target,
            tooltip: None,
            highlight_click: false,
            text_color: false,
        }
    }
}

struct Destination {
    relationship: Option<(&'static str, Target)>,
    action: Option<&'static str>,
}

impl Destination {
    fn resolve(
        target: &LinkTarget,
        slides: &[crate::presentation::SlideEntry],
        identity: &Arc<()>,
    ) -> Result<Self> {
        let (relationship, action) = match target {
            LinkTarget::Url(url) => {
                if url.trim().split_once(':').is_some_and(|(scheme, _)| {
                    ["javascript", "vbscript", "data"]
                        .iter()
                        .any(|s| scheme.eq_ignore_ascii_case(s))
                }) {
                    return Err(Error::UnsafeLinkScheme);
                }
                if url.is_empty() || IriReferenceStr::new(url).is_err() {
                    return Err(Error::InvalidLinkUrl);
                }
                (Some((REL_HYPERLINK, Target::External(url.clone()))), None)
            }
            LinkTarget::SlideId(target) => {
                if !Arc::ptr_eq(identity, &target.identity) {
                    return Err(Error::ForeignLinkSlide);
                }
                let part = slides
                    .iter()
                    .find(|s| s.slide_id == target.slide_id)
                    .ok_or(Error::LinkSlideNotFound {
                        slide_id: target.slide_id,
                    })?
                    .part
                    .clone()
                    .ok_or(Error::DanglingSlide {
                        slide_id: target.slide_id,
                    })?;
                (
                    Some((REL_SLIDE, Target::Part(part))),
                    Some("ppaction://hlinksldjump"),
                )
            }
            LinkTarget::Jump(jump) => (
                None,
                Some(match jump {
                    ShowJump::First => "ppaction://hlinkshowjump?jump=firstslide",
                    ShowJump::Last => "ppaction://hlinkshowjump?jump=lastslide",
                    ShowJump::Next => "ppaction://hlinkshowjump?jump=nextslide",
                    ShowJump::Previous => "ppaction://hlinkshowjump?jump=previousslide",
                    ShowJump::LastViewed => "ppaction://hlinkshowjump?jump=lastslideviewed",
                    ShowJump::EndShow => "ppaction://hlinkshowjump?jump=endshow",
                }),
            ),
            LinkTarget::NoAction => (None, Some("ppaction://noaction")),
            _ => return Err(Error::UnsupportedAction),
        };
        Ok(Self {
            relationship,
            action,
        })
    }

    fn matches(&self, rels: &Relationships, id: Option<&str>) -> bool {
        match &self.relationship {
            None => id == Some(""),
            Some((kind, target)) => id.and_then(|id| rels.by_id(id)).is_some_and(|rel| {
                rel.rel_type == *kind
                    && match target {
                        Target::External(url) => {
                            rel.target_mode == TargetMode::External && rel.target == *url
                        }
                        Target::Part(part) => rels.resolve(rel).as_ref() == Some(part),
                    }
            }),
        }
    }
}

const ID: QName = QName::new(R_NS, "id");
const ACTION: QName = QName::new("", "action");
const TOOLTIP: QName = QName::new("", "tooltip");
const HIGHLIGHT: QName = QName::new("", "highlightClick");

fn alternate_content(node: &RawNode) -> bool {
    node.is(ooxml_core::name::MC_ALTERNATE_CONTENT) || node.elements().any(alternate_content)
}

fn own_text_color(node: &RawNode) -> bool {
    node.elements()
        .filter(|n| n.is(HyperlinkExtensionList::QNAME))
        .flat_map(RawNode::elements)
        .filter(|n| {
            n.is(HyperlinkExtension::QNAME)
                && n.attr(QName::new("", "uri")) == Some(ext_uri::HYPERLINK_COLOR)
        })
        .flat_map(RawNode::elements)
        .any(|n| n.is(HyperlinkColor::QNAME) && n.attr(QName::new("", "val")) == Some("tx"))
}

struct LinkEdit<'a> {
    // Indices of the old nodes for this event, in document order.
    old: Vec<usize>,
    new: Option<(&'a LinkSpec, &'a Destination, &'a str)>,
    text_color: bool,
}

impl LinkEdit<'_> {
    fn write<P, C>(&self, parent: &mut LensMut<'_, P>) -> Result<()>
    where
        P: ElementSpec,
        C: ChildOf<P>,
        HyperlinkExtensionList: ChildOf<C>,
    {
        for &index in self.old.iter().rev() {
            parent.remove_at(index);
        }
        if let Some((spec, destination, id)) = self.new {
            let mut link = parent.insert_with_ns::<C>(&[NsDecl::new("r", R_NS)])?;
            link.set_attr(ID, id)?;
            if let Some(action) = destination.action {
                link.set_attr(ACTION, action)?;
            }
            if let Some(tooltip) = &spec.tooltip {
                link.set_attr(TOOLTIP, tooltip)?;
            }
            if spec.highlight_click {
                link.set_attr(HIGHLIGHT, "1")?;
            }
            if self.text_color {
                let mut list = link.insert::<HyperlinkExtensionList>()?;
                let mut ext = HyperlinkExtensionViewMut(list.insert::<HyperlinkExtension>()?);
                ext.set_uri(ext_uri::HYPERLINK_COLOR)?;
                let mut color =
                    HyperlinkColorViewMut(ext.0.insert_with_ns::<HyperlinkColor>(&[
                        NsDecl::new("ahyp", HyperlinkColor::QNAME.ns),
                    ])?);
                color.set_val(HyperlinkColorEnum::Tx)?;
            }
        }
        Ok(())
    }
}

fn edit_link(
    doc: &mut EditPart<'_>,
    parent_path: Option<&[usize]>,
    name: QName,
    guard: &Guard,
    spec: Option<&LinkSpec>,
    text: bool,
    write: impl FnOnce(&mut XmlDocument, LinkEdit<'_>) -> Result<()>,
) -> Result<()> {
    let parent = parent_path.and_then(|path| node_at(doc.root(), path));
    if parent.is_some_and(alternate_content) {
        return Err(Error::InsideAlternateContent);
    }
    let old: Vec<_> = parent
        .into_iter()
        .flat_map(RawNode::indexed_elements)
        .filter(|(_, n)| n.is(name))
        .collect();
    if old.is_empty() && spec.is_none() {
        return Ok(());
    }

    let old_ids: std::collections::BTreeSet<_> = old
        .iter()
        .filter_map(|(_, n)| n.attr(ID))
        .map(ooxml_core::value::trim_xml_ws)
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
        .collect();
    let indices = old.iter().map(|(i, _)| *i).collect();
    let text_color = text && spec.is_some_and(|s| s.text_color);
    let EditPart::Package {
        package,
        name,
        slides,
        link_identity,
    } = &*doc
    else {
        return Err(Error::UnsupportedAction);
    };
    let destination = spec
        .map(|s| Destination::resolve(&s.target, slides, link_identity))
        .transpose()?;
    let rels = package.relationships(name)?;
    if let ([(.., node)], Some(spec), Some(destination)) =
        (old.as_slice(), spec, destination.as_ref())
        && node.attr(ACTION).filter(|a| !a.is_empty()) == destination.action
        && node.attr(TOOLTIP) == spec.tooltip.as_deref()
        && node
            .attr(HIGHLIGHT)
            .and_then(ooxml_core::value::parse_bool)
            .unwrap_or(false)
            == spec.highlight_click
        && own_text_color(node) == text_color
        && destination.matches(&rels, node.attr(ID))
    {
        return Ok(());
    }
    if !old.is_empty() && guard.links {
        return Err(Error::CommentAnchoredText {
            shape_id: guard.shape_id,
        });
    }

    let mut staged = doc.clone();
    let EditPart::Package { package, name, .. } = doc else {
        unreachable!()
    };
    let mut plan = RegistryPlan::new(package);
    let id = match destination.as_ref().and_then(|d| d.relationship.as_ref()) {
        Some((kind, target)) => plan.add_relationship(package, Some(name), kind, target.clone())?,
        None => String::new(),
    };
    let new = spec
        .zip(destination.as_ref())
        .map(|(spec, dest)| (spec, dest, id.as_str()));
    write(
        &mut staged,
        LinkEdit {
            old: indices,
            new,
            text_color,
        },
    )?;
    plan.stage_part(package, name, staged)?;
    for id in old_ids {
        let Some(rel) = rels.by_id(&id) else { continue };
        // Other relationship kinds belong to features outside link editing.
        if ![REL_HYPERLINK, REL_SLIDE].contains(&rel.rel_type.as_str()) {
            continue;
        }
        match plan.remove_relationship(package, Some(name), &id, &[REL_HYPERLINK, REL_SLIDE]) {
            Ok(())
            | Err(ooxml_core::Error::RegistryRefused {
                reason: RegistryRefusal::StillReferenced { .. },
                ..
            }) => {}
            Err(error) => return Err(error.into()),
        }
    }
    package.apply(plan)?;
    Ok(())
}

fn text_link<E: ChildOf<Paragraph>>(
    doc: &mut EditPart<'_>,
    loc: &ParaLoc,
    index: usize,
    missing: Error,
    event: LinkEvent,
    spec: Option<&LinkSpec>,
) -> Result<()>
where
    RunProperties: ChildOf<E>,
{
    let para = loc.node(doc)?;
    para_shape(para)?;
    let (item_index, item) = para
        .indexed_elements()
        .filter(|(_, n)| n.is(E::QNAME))
        .nth(index)
        .ok_or(missing)?;
    let props_path = item
        .indexed_elements()
        .find(|(_, n)| n.is(RunProperties::QNAME))
        .map(|(props_index, _)| {
            let mut path = loc.body.clone();
            path.extend([
                loc.child(doc).expect("paragraph exists"),
                item_index,
                props_index,
            ]);
            path
        });
    let name = match event {
        LinkEvent::Click => HyperlinkOnClick::QNAME,
        LinkEvent::Hover => HyperlinkOnMouseOver::QNAME,
    };
    edit_link(
        doc,
        props_path.as_deref(),
        name,
        &loc.guard,
        spec,
        true,
        |staged, edit| {
            let mut paragraph = loc.lens(staged)?;
            let mut item = paragraph
                .nth_child_mut::<E>(index)
                .expect("text item exists");
            let mut props = item.get_or_insert::<RunProperties>()?;
            match event {
                LinkEvent::Click => edit.write::<_, HyperlinkOnClick>(&mut props),
                LinkEvent::Hover => edit.write::<_, HyperlinkOnMouseOver>(&mut props),
            }
        },
    )
}

impl RunMut<'_> {
    /// Sets a direct click or hover link. Passing `None` clears it.
    pub fn set_link(&mut self, event: LinkEvent, link: Option<&LinkSpec>) -> Result<()> {
        text_link::<Run>(
            &mut self.doc,
            &self.loc,
            self.index,
            Error::RunNotFound(self.index),
            event,
            link,
        )
    }
}

/// A cached text field opened for link editing.
#[derive(Debug)]
pub struct FieldMut<'a> {
    doc: EditPart<'a>,
    loc: ParaLoc,
    index: usize,
}

impl ParagraphMut<'_> {
    /// Opens the `index`-th field. Runs and breaks are skipped.
    pub fn field_mut(&mut self, index: usize) -> Result<FieldMut<'_>> {
        if self
            .node()?
            .elements()
            .filter(|n| n.is(Field::QNAME))
            .nth(index)
            .is_none()
        {
            return Err(Error::FieldNotFound(index));
        }
        Ok(FieldMut {
            doc: self.doc.reborrow(),
            loc: self.loc.clone(),
            index,
        })
    }
}

impl FieldMut<'_> {
    /// Sets a direct click or hover link. Passing `None` clears it.
    pub fn set_link(&mut self, event: LinkEvent, link: Option<&LinkSpec>) -> Result<()> {
        text_link::<Field>(
            &mut self.doc,
            &self.loc,
            self.index,
            Error::FieldNotFound(self.index),
            event,
            link,
        )
    }
}

impl ShapeMut<'_> {
    /// Sets a direct click or hover action. Passing `None` clears it.
    pub fn set_action(&mut self, event: LinkEvent, link: Option<&LinkSpec>) -> Result<()> {
        let mut path = self.path.clone();
        path.extend(read::c_nv_pr_path(self.node()).ok_or(Error::InvalidShapeId)?);
        if !node_at(self.doc.root(), &path)
            .is_some_and(|n| n.is(pml::NonVisualDrawingProperties::QNAME))
        {
            return Err(Error::UnsupportedAction);
        }
        let name = match event {
            LinkEvent::Click => HyperlinkOnClick::QNAME,
            LinkEvent::Hover => HyperlinkOnHover::QNAME,
        };
        edit_link(
            &mut self.doc,
            Some(&path),
            name,
            &self.guard,
            link,
            false,
            |staged, edit| {
                let mut props = staged.lens_mut_at::<pml::NonVisualDrawingProperties>(&path)?;
                match event {
                    LinkEvent::Click => edit.write::<_, HyperlinkOnClick>(&mut props),
                    LinkEvent::Hover => edit.write::<_, HyperlinkOnHover>(&mut props),
                }
            },
        )
    }
}
