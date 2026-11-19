//! Shared P3c deck for the PowerPoint and python-pptx checks.

use ooxml_pptx::{Error, LinkEvent, LinkSpec, LinkTarget, Presentation, ShowJump};

pub const FIXTURE: &str = "poi/sample.pptx";
pub const NAME: &str = "p3c-links";

pub fn edit(deck: &mut Presentation) -> ooxml_pptx::Result<()> {
    let target_id = deck.slides().nth(1).expect("second slide")?.slide_id;
    let target = LinkSpec::new(deck.slide_link_target(target_id));
    let mut slide = deck.slide_mut(0)?;
    let shapes = slide.as_slide().shapes();
    let mut text_shapes = shapes.iter().filter(|shape| shape.text_body().is_some());
    let title_id = text_shapes.next().ok_or(Error::InvalidShapeId)?.id()?;
    let body_id = text_shapes.next().ok_or(Error::InvalidShapeId)?.id()?;
    let url = LinkSpec::new(LinkTarget::Url("https://example.com/slides?x=1&y=2".into()));
    let mut title = slide.shape_mut_by_id(title_id)?;
    title
        .text_body_mut()?
        .paragraph_mut(0)?
        .run_mut(0)?
        .set_link(LinkEvent::Click, Some(&url))?;
    title.set_action(LinkEvent::Click, Some(&target))?;
    slide.shape_mut_by_id(body_id)?.set_action(
        LinkEvent::Click,
        Some(&LinkSpec::new(LinkTarget::Jump(ShowJump::Last))),
    )
}
