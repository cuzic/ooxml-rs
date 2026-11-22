use ooxml_core::{Package, QName};
use ooxml_pptx::{
    AuthorIdentity, CommentAnchor, CommentAuthor, NewComment, Presentation, ShapeKind,
};
use std::time::{Duration, UNIX_EPOCH};

pub const CASES: [(&str, &str, &str); 5] = [
    (
        "M1-slide-comment",
        "poi/sample.pptx",
        "Add a comment to a slide without a creation ID.",
    ),
    (
        "M2-title-comment",
        "project/powerpoint-shape-comment.pptx",
        "Add a second comment to the title shape.",
    ),
    (
        "M3-shape-without-creation-id",
        "poi/sample.pptx",
        "Add a comment to a shape without a creation ID.",
    ),
    (
        "M9-new-slide-comment",
        "poi/sample.pptx",
        "Add a slide, then comment on it.",
    ),
    (
        "M11-nested-picture-comment",
        "project/powerpoint-comment-anchor-kinds.pptx",
        "Wrap the fixture picture in a group, then comment on the picture.",
    ),
];

pub fn prepare(deck: &mut Presentation, case: usize) -> Result<(), Box<dyn std::error::Error>> {
    if case == 3 {
        let layout = deck.layouts()?.into_iter().next().ok_or("missing layout")?;
        deck.add_slide(&layout, None)?;
    } else if case == 4 {
        group_picture(deck)?;
    }
    Ok(())
}

// SYNTHETIC: retain the fixture picture and all of its existing references,
// and put it in an identity-transform group with the same bounds.
fn group_picture(deck: &mut Presentation) -> Result<(), Box<dyn std::error::Error>> {
    const P: &str = "http://schemas.openxmlformats.org/presentationml/2006/main";
    const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
    let id = deck.next_shape_id(0)?;
    let slide = deck.slide(0)?;
    let part = slide.info().part_name.clone();
    let picture = slide
        .shapes()
        .into_iter()
        .find(|shape| *shape.kind() == ShapeKind::Picture)
        .ok_or("missing picture")?;
    let raw = picture.raw();
    let span = raw.span().ok_or("missing picture span")?;
    let transform = raw
        .elements()
        .find(|n| n.is(QName::new(P, "spPr")))
        .and_then(|n| n.elements().find(|n| n.is(QName::new(A, "xfrm"))))
        .ok_or("missing picture transform")?;
    let offset = transform
        .elements()
        .find(|n| n.is(QName::new(A, "off")))
        .ok_or("missing picture offset")?;
    let extent = transform
        .elements()
        .find(|n| n.is(QName::new(A, "ext")))
        .ok_or("missing picture extent")?;
    let x: i64 = offset
        .attr(QName::new("", "x"))
        .ok_or("missing x")?
        .parse()?;
    let y: i64 = offset
        .attr(QName::new("", "y"))
        .ok_or("missing y")?
        .parse()?;
    let cx: u64 = extent
        .attr(QName::new("", "cx"))
        .ok_or("missing cx")?
        .parse()?;
    let cy: u64 = extent
        .attr(QName::new("", "cy"))
        .ok_or("missing cy")?
        .parse()?;
    let mut package = Package::from_bytes(deck.to_bytes()?)?;
    let mut xml = String::from_utf8(package.read_part(&part)?)?;
    let grouped = format!(
        "<p:grpSp><p:nvGrpSpPr><p:cNvPr id=\"{id}\" name=\"Picture group\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>         <p:grpSpPr><a:xfrm><a:off x=\"{x}\" y=\"{y}\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/>         <a:chOff x=\"{x}\" y=\"{y}\"/><a:chExt cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm></p:grpSpPr>{}</p:grpSp>",
        &xml[span.start..span.end()]
    );
    xml.replace_range(span.start..span.end(), &grouped);
    package.set_part(&part, xml.into_bytes())?;
    *deck = Presentation::from_bytes(package.to_bytes()?)?;
    Ok(())
}

pub fn edit(deck: &mut Presentation, case: usize) -> Result<(), Box<dyn std::error::Error>> {
    let (slide, anchor) = match case {
        0 => (0, CommentAnchor::Slide),
        1 => (0, CommentAnchor::Shape(2)),
        2 | 4 => {
            let shape_kind = if case == 2 {
                ShapeKind::Shape
            } else {
                ShapeKind::Picture
            };
            let id = deck
                .slide(0)?
                .shapes()
                .into_iter()
                .find(|shape| *shape.kind() == shape_kind && (case == 2 || shape.depth() > 0))
                .ok_or("missing target shape")?
                .id()?;
            (0, CommentAnchor::Shape(id))
        }
        3 => (deck.slide_count() - 1, CommentAnchor::Slide),
        _ => unreachable!(),
    };
    let author = CommentAuthor {
        name: "Ada Lovelace".into(),
        identity: AuthorIdentity::Local,
        initials: None,
    };
    deck.add_comment(
        slide,
        anchor,
        NewComment {
            author,
            text: "First line\r\nSecond line\u{b}continued".into(),
            created: Some(UNIX_EPOCH + Duration::from_millis(1_794_822_645_123)),
            lang: None,
        },
    )?;
    Ok(())
}
