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
        "M11-nested-shape-comment",
        "project/powerpoint-comment-anchor-kinds.pptx",
        "Add a comment to a shape inside a group.",
    ),
];

pub fn edit(deck: &mut Presentation, case: usize) -> Result<(), Box<dyn std::error::Error>> {
    let (slide, anchor) = match case {
        0 => (0, CommentAnchor::Slide),
        1 => (0, CommentAnchor::Shape(2)),
        2 | 4 => {
            let index = if case == 2 { 0 } else { 4 };
            let id = deck
                .slide(index)?
                .shapes()
                .into_iter()
                .find(|shape| *shape.kind() == ShapeKind::Shape && (case == 2 || shape.depth() > 0))
                .ok_or("missing target shape")?
                .id()?;
            (index, CommentAnchor::Shape(id))
        }
        3 => {
            let slide = deck.slide_count();
            let layout = deck.layouts()?.into_iter().next().ok_or("missing layout")?;
            deck.add_slide(&layout, None)?;
            (slide, CommentAnchor::Slide)
        }
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
