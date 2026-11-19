//! Link transactions, locality, and preservation (ADR-0016 §7).

#[path = "oracle/cases.rs"]
mod cases;

use std::assert_matches;

use ooxml_core::Package;
use ooxml_core::opc::TargetMode;
use ooxml_pptx::{
    Error, FormatChange, LinkEvent, LinkSpec, LinkTarget, Presentation, RunFormat, ShapeMut,
    ShowJump,
};

const FIXTURE: &str = "poi/sample.pptx";
const PART: &str = "ppt/slides/slide1.xml";
const LINK_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink";
const SLIDE_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide";

fn open() -> Presentation {
    Presentation::from_bytes(cases::read_fixture(FIXTURE)).unwrap()
}

fn patch(bytes: Vec<u8>, part: &str, old: &str, new: &str) -> Presentation {
    let mut pkg = Package::from_bytes(bytes).unwrap();
    let xml = String::from_utf8(pkg.read_part(part).unwrap()).unwrap();
    assert!(xml.contains(old), "missing {old:?}");
    pkg.set_part(part, xml.replacen(old, new, 1).into_bytes())
        .unwrap();
    Presentation::from_bytes(pkg.to_bytes().unwrap()).unwrap()
}

fn edit(
    p: &mut Presentation,
    f: impl FnOnce(&mut ShapeMut<'_>) -> ooxml_pptx::Result<()>,
) -> ooxml_pptx::Result<()> {
    let mut slide = p.slide_mut(0)?;
    let id = slide
        .as_slide()
        .shapes()
        .into_iter()
        .find(|s| s.text_body().is_some())
        .unwrap()
        .id()?;
    f(&mut slide.shape_mut_by_id(id)?)
}

fn run(p: &mut Presentation, event: LinkEvent, link: Option<&LinkSpec>) -> ooxml_pptx::Result<()> {
    edit(p, |s| {
        s.text_body_mut()?
            .paragraph_mut(0)?
            .run_mut(0)?
            .set_link(event, link)
    })
}

fn action(
    p: &mut Presentation,
    event: LinkEvent,
    link: Option<&LinkSpec>,
) -> ooxml_pptx::Result<()> {
    edit(p, |s| s.set_action(event, link))
}

fn refused(
    p: &mut Presentation,
    f: impl FnOnce(&mut Presentation) -> ooxml_pptx::Result<()>,
) -> Error {
    let before = p.to_bytes().unwrap();
    let error = f(p).unwrap_err();
    assert_eq!(before, p.to_bytes().unwrap(), "{error}");
    error
}

fn url(url: &str) -> LinkSpec {
    LinkSpec::new(LinkTarget::Url(url.into()))
}

fn link_ids(p: &Presentation) -> Vec<String> {
    p.package()
        .relationships(PART)
        .unwrap()
        .by_type(LINK_REL)
        .map(|r| r.id.clone())
        .collect()
}

fn xml(p: &Presentation, part: &str) -> String {
    // Read the serialized part, even while its edited tree is cached.
    let saved = Package::from_bytes(p.to_bytes().unwrap()).unwrap();
    String::from_utf8(saved.read_part(part).unwrap()).unwrap()
}

#[test]
fn replacement_allocates_before_removing_and_preserves_unedited_bytes() {
    let mut p = open();
    let input = xml(&p, PART);
    let original = Package::from_bytes(p.to_bytes().unwrap()).unwrap();
    let properties_start = input.find("<a:rPr lang=\"en-US\"><a:sym").unwrap();
    let properties_end =
        properties_start + input[properties_start..].find("</a:rPr>").unwrap() + "</a:rPr>".len();

    let link = LinkSpec {
        tooltip: Some("Search & next".into()),
        text_color: true,
        highlight_click: true,
        ..url("https://example.com/é/a%20b?x=1&y=2")
    };
    run(&mut p, LinkEvent::Click, Some(&link)).unwrap();
    let first = link_ids(&p);
    assert_eq!(first.len(), 1);
    let edited = Package::from_bytes(p.to_bytes().unwrap()).unwrap();
    assert_eq!(original.part_names().count(), edited.part_names().count());
    for name in original.part_names() {
        if ![PART, "ppt/slides/_rels/slide1.xml.rels"].contains(&name) {
            assert_eq!(
                original.read_part(name).unwrap(),
                edited.read_part(name).unwrap(),
                "{name}"
            );
        }
    }
    let after = xml(&p, PART);
    assert!(after.starts_with(&input[..properties_start]));
    assert!(after.ends_with(&input[properties_end..]));
    assert!(after.contains("tooltip=\"Search &amp; next\""));
    assert!(after.contains("highlightClick=\"1\""));
    assert!(after.contains("<ahyp:hlinkClr xmlns:ahyp="));
    let before_equal = p.to_bytes().unwrap();
    run(&mut p, LinkEvent::Click, Some(&link)).unwrap();
    assert_eq!(p.to_bytes().unwrap(), before_equal);
    let rels = p.package().relationships(PART).unwrap();
    let rel = rels.by_id(&first[0]).unwrap();
    assert_eq!(rel.target, "https://example.com/é/a%20b?x=1&y=2");
    assert_eq!(rel.target_mode, TargetMode::External);
    assert!(xml(&p, "ppt/slides/_rels/slide1.xml.rels").contains("?x=1&amp;y=2"));

    // Reopen to discard the session's relationship high-water mark.
    let mut p = Presentation::from_bytes(before_equal).unwrap();
    run(&mut p, LinkEvent::Click, Some(&url("../read%20me#top"))).unwrap();
    let second = link_ids(&p);
    assert_eq!(second.len(), 1, "the old relationship was removed");
    let first_n: u32 = first[0].strip_prefix("rId").unwrap().parse().unwrap();
    assert_eq!(second[0], format!("rId{}", first_n + 1));
    let after = xml(&p, PART);
    assert!(after.starts_with(&input[..properties_start]));
    assert!(after.ends_with(&input[properties_end..]));
    assert!(!after.contains("hlinkClr"));
    run(&mut p, LinkEvent::Click, None).unwrap();
    assert!(link_ids(&p).is_empty());
    assert_eq!(xml(&p, PART), input);
}

#[test]
fn shared_ids_survive_run_split_and_non_relationship_attributes() {
    let mut p = open();
    run(
        &mut p,
        LinkEvent::Click,
        Some(&LinkSpec {
            text_color: true,
            ..url("https://example.com/")
        }),
    )
    .unwrap();
    let ids = link_ids(&p);
    edit(&mut p, |s| {
        s.text_body_mut()?.paragraph_mut(0)?.format_range(
            0,
            5,
            &RunFormat {
                bold: Some(FormatChange::Set(true)),
                ..RunFormat::default()
            },
        )
    })
    .unwrap();
    assert_eq!(xml(&p, PART).matches("<a:hlinkClick").count(), 2);
    run(&mut p, LinkEvent::Click, None).unwrap();
    assert_eq!(link_ids(&p), ids);
    let mut p = patch(
        p.to_bytes().unwrap(),
        PART,
        "<a:t> ipsum",
        &format!("<a:t custom=\" {} \"> ipsum", ids[0]),
    );
    edit(&mut p, |s| {
        s.text_body_mut()?
            .paragraph_mut(0)?
            .run_mut(1)?
            .set_link(LinkEvent::Click, None)
    })
    .unwrap();
    assert_eq!(link_ids(&p), ids);
}

#[test]
fn supported_actions_and_slide_target_survive_reordering() {
    let mut p = open();
    let target_id = p.slides().nth(1).unwrap().unwrap().slide_id;
    let target_part = p.slides().nth(1).unwrap().unwrap().part_name;
    let target = LinkSpec::new(p.slide_link_target(target_id));
    p.duplicate_slide(0, Some(2)).unwrap();
    p.move_slide(1, 2).unwrap();
    action(&mut p, LinkEvent::Click, Some(&target)).unwrap();
    let rels = p.package().relationships(PART).unwrap();
    let rels_to_slide: Vec<_> = rels.by_type(SLIDE_REL).collect();
    assert_eq!(rels_to_slide.len(), 1);
    assert_eq!(rels.resolve(rels_to_slide[0]), Some(target_part));
    assert!(xml(&p, PART).contains("ppaction://hlinksldjump"));
    assert_matches!(
        refused(&mut p, |p| p.delete_slide(2)),
        Error::SlideIsReferenced { .. }
    );

    for (jump, token) in [
        (ShowJump::Next, "nextslide"),
        (ShowJump::Previous, "previousslide"),
        (ShowJump::First, "firstslide"),
        (ShowJump::Last, "lastslide"),
        (ShowJump::LastViewed, "lastslideviewed"),
        (ShowJump::EndShow, "endshow"),
    ] {
        action(
            &mut p,
            LinkEvent::Hover,
            Some(&LinkSpec::new(LinkTarget::Jump(jump))),
        )
        .unwrap();
        assert!(xml(&p, PART).contains(&format!(
            "<a:hlinkHover r:id=\"\" action=\"ppaction://hlinkshowjump?jump={token}\""
        )));
    }
    action(
        &mut p,
        LinkEvent::Click,
        Some(&LinkSpec::new(LinkTarget::NoAction)),
    )
    .unwrap();
    assert!(xml(&p, PART).contains("<a:hlinkClick r:id=\"\" action=\"ppaction://noaction\""));
    assert_eq!(
        p.package()
            .relationships(PART)
            .unwrap()
            .by_type(SLIDE_REL)
            .count(),
        0
    );

    let deleted = LinkSpec::new(p.slide_link_target(target_id));
    p.delete_slide(2).unwrap();
    assert_matches!(
        refused(&mut p, |p| action(p, LinkEvent::Click, Some(&deleted))),
        Error::LinkSlideNotFound { .. }
    );
}

#[test]
fn fields_and_runs_have_independent_events_and_preserve_unknown_actions() {
    let mut p = patch(
        cases::read_fixture(FIXTURE),
        PART,
        "</a:r><a:endParaRPr",
        "</a:r><a:fld id=\"{3DD0307C-7D24-4FA7-9B03-E87BDB38757B}\" type=\"slidenum\"><a:t>1</a:t></a:fld><a:endParaRPr",
    );
    let click = LinkSpec::new(LinkTarget::NoAction);
    let hover = url("custom:opaque");
    edit(&mut p, |shape| {
        let mut body = shape.text_body_mut()?;
        let mut para = body.paragraph_mut(0)?;
        para.run_mut(0)?.set_link(LinkEvent::Hover, Some(&hover))?;
        let mut field = para.field_mut(0)?;
        field.set_link(LinkEvent::Click, Some(&click))?;
        field.set_link(LinkEvent::Hover, Some(&hover))?;
        Ok(())
    })
    .unwrap();
    let before = xml(&p, PART);
    assert_eq!(before.matches("<a:hlinkMouseOver").count(), 2);
    assert_eq!(link_ids(&p).len(), 2);
    assert!(before.contains("</a:rPr><a:t>1</a:t></a:fld>"));

    let mut p = patch(
        p.to_bytes().unwrap(),
        PART,
        "ppaction://noaction",
        "ppaction://media",
    );
    edit(&mut p, |shape| {
        shape
            .text_body_mut()?
            .paragraph_mut(0)?
            .field_mut(0)?
            .set_link(LinkEvent::Hover, None)
    })
    .unwrap();
    let after = xml(&p, PART);
    assert_eq!(after.matches("<a:hlinkMouseOver").count(), 1);
    assert!(after.contains("ppaction://media"));
    assert_eq!(link_ids(&p).len(), 1);
}

#[test]
fn refusals_preserve_original_bytes() {
    let mut p = open();
    run(
        &mut p,
        LinkEvent::Click,
        Some(&url("https://example.org/old")),
    )
    .unwrap();
    for unsafe_url in [" JaVaScRiPt:any ", "VBScript:any", "\tDATA:text/plain,x"] {
        assert_matches!(
            refused(&mut p, |p| run(p, LinkEvent::Click, Some(&url(unsafe_url)))),
            Error::UnsafeLinkScheme
        );
    }
    for invalid in [
        "bad space",
        "https://host/%xy",
        "http://host/\u{7f}",
        "https://host/a\u{a0}b",
        "",
    ] {
        assert_matches!(
            refused(&mut p, |p| run(p, LinkEvent::Click, Some(&url(invalid)))),
            Error::InvalidLinkUrl
        );
    }
    let bad_tooltip = LinkSpec {
        tooltip: Some("bad\u{1}".into()),
        ..url("https://example.org/new")
    };
    assert_matches!(
        refused(&mut p, |p| run(p, LinkEvent::Click, Some(&bad_tooltip))),
        Error::Core(_)
    );
    for target in [
        LinkTarget::Media,
        LinkTarget::Macro("demo".into()),
        LinkTarget::File("demo".into()),
        LinkTarget::Ole("demo".into()),
        LinkTarget::Program("demo".into()),
    ] {
        assert_matches!(
            refused(&mut p, |p| action(
                p,
                LinkEvent::Click,
                Some(&LinkSpec::new(target))
            )),
            Error::UnsupportedAction
        );
    }
    let other = open();
    let foreign =
        LinkSpec::new(other.slide_link_target(other.slides().next().unwrap().unwrap().slide_id));
    assert_matches!(
        refused(&mut p, |p| action(p, LinkEvent::Click, Some(&foreign))),
        Error::ForeignLinkSlide
    );
    let missing = LinkSpec::new(p.slide_link_target(u32::MAX));
    assert_matches!(
        refused(&mut p, |p| action(p, LinkEvent::Click, Some(&missing))),
        Error::LinkSlideNotFound { slide_id: u32::MAX }
    );
}

#[test]
fn comment_anchor_blocks_removal_and_replacement_but_allows_addition() {
    let bytes = cases::read_fixture("libreoffice/modern-comments.pptx");
    let part = "ppt/comments/modernComment_100_68C49C6.xml";
    let p = patch(bytes, part, "<ac:deMkLst ", "<ac:hlinkMkLst ");
    let mut p = patch(
        p.to_bytes().unwrap(),
        part,
        "</ac:deMkLst>",
        "</ac:hlinkMkLst>",
    );
    let old = url("https://example.org/");
    p.slide_mut(0)
        .unwrap()
        .shape_mut_by_id(2)
        .unwrap()
        .set_action(LinkEvent::Click, Some(&old))
        .unwrap();
    p.slide_mut(0)
        .unwrap()
        .shape_mut_by_id(2)
        .unwrap()
        .text_body_mut()
        .unwrap()
        .paragraph_mut(0)
        .unwrap()
        .run_mut(0)
        .unwrap()
        .set_link(LinkEvent::Click, Some(&old))
        .unwrap();

    let new = url("https://example.net/");
    for link in [None, Some(&new)] {
        assert_matches!(
            refused(&mut p, |p| p
                .slide_mut(0)?
                .shape_mut_by_id(2)?
                .set_action(LinkEvent::Click, link)),
            Error::CommentAnchoredText { shape_id: 2 }
        );
        assert_matches!(
            refused(&mut p, |p| p
                .slide_mut(0)?
                .shape_mut_by_id(2)?
                .text_body_mut()?
                .paragraph_mut(0)?
                .run_mut(0)?
                .set_link(LinkEvent::Click, link)),
            Error::CommentAnchoredText { shape_id: 2 }
        );
    }
    p.slide_mut(0)
        .unwrap()
        .shape_mut_by_id(3)
        .unwrap()
        .set_action(LinkEvent::Click, Some(&new))
        .unwrap();
}
