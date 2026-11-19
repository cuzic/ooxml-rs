//! ADR-0016 §§2–5 direct formatting, including locality and atomic refusals.

#[path = "oracle/cases.rs"]
mod cases;

use std::assert_matches;

use ooxml_core::{Package, QName, value::DmlPercent};
use ooxml_pptx::{
    AutoFitMode, BulletSpec, ColorSpec, Error, FontRef, FormatChange, Lookup, Presentation,
    RunFormat, RunItem, SchemeColor, Script, Source, Spacing, TextBodyMut, UnderlineFill,
    UnderlineLine, WriteColor,
};
use ooxml_schema::dml::{TextAlignmentTypeValues, TextUnderlineValues};

const PART: &str = "ppt/slides/slide1.xml";
const FIXTURE: &str = "poi/sample.pptx";

fn open() -> Presentation {
    Presentation::from_bytes(cases::read_fixture(FIXTURE)).unwrap()
}

fn patch(old: &str, new: &str) -> Presentation {
    let mut pkg = Package::from_bytes(cases::read_fixture(FIXTURE)).unwrap();
    let xml = String::from_utf8(pkg.read_part(PART).unwrap()).unwrap();
    assert!(xml.contains(old));
    pkg.set_part(PART, xml.replacen(old, new, 1).into_bytes())
        .unwrap();
    Presentation::from_bytes(pkg.to_bytes().unwrap()).unwrap()
}

fn edit(
    pres: &mut Presentation,
    apply: impl FnOnce(&mut TextBodyMut<'_>) -> Result<(), Error>,
) -> Result<(), Error> {
    let mut slide = pres.slide_mut(0)?;
    let id = slide
        .as_slide()
        .shapes()
        .into_iter()
        .find(|shape| shape.text_body().is_some())
        .unwrap()
        .id()?;
    apply(&mut slide.shape_mut_by_id(id)?.text_body_mut()?)
}

fn bold() -> RunFormat {
    RunFormat {
        bold: Some(FormatChange::Set(true)),
        ..RunFormat::default()
    }
}

fn refused(
    pres: &mut Presentation,
    apply: impl FnOnce(&mut TextBodyMut<'_>) -> Result<(), Error>,
) -> Error {
    let before = pres.to_bytes().unwrap();
    let error = edit(pres, apply).unwrap_err();
    assert_eq!(before, pres.to_bytes().unwrap(), "{error}");
    error
}

#[test]
fn split_unicode_preserves_text_and_siblings() {
    let mut pres = patch(
        "<a:t>Lorem ipsum dolor sit amet</a:t>",
        "<a:t>α🙂日beta</a:t>",
    );
    let original = String::from_utf8(pres.package().read_part(PART).unwrap()).unwrap();
    edit(&mut pres, |body| {
        body.paragraph_mut(0)?.format_range(1, 3, &bold())
    })
    .unwrap();
    let mut reopened = Presentation::from_bytes(pres.to_bytes().unwrap()).unwrap();
    let loaded = reopened.slide(0).unwrap();
    let body = loaded
        .shapes()
        .into_iter()
        .find_map(|shape| shape.text_body())
        .unwrap();
    let paragraph = body.paragraphs()[0];
    assert_eq!(paragraph.text(), "α🙂日beta");
    assert_eq!(
        paragraph
            .runs()
            .iter()
            .map(|run| run.text().into_owned())
            .collect::<Vec<_>>(),
        ["α", "🙂日", "beta"]
    );
    let output = String::from_utf8(reopened.package().read_part(PART).unwrap()).unwrap();
    let start = original.find("<a:r><a:rPr lang=\"en-US\"><a:sym").unwrap();
    let end = original[start..].find("</a:r>").unwrap() + start + "</a:r>".len();
    assert!(output.starts_with(&original[..start]));
    assert!(output.ends_with(&original[end..]));
    assert_eq!(
        output
            .matches("<a:sym typeface=\"Wingdings\" pitchFamily=\"2\" charset=\"2\"/>")
            .count(),
        original
            .matches("<a:sym typeface=\"Wingdings\" pitchFamily=\"2\" charset=\"2\"/>")
            .count()
            + 2
    );
}

#[test]
fn direct_no_ops_and_clear_restore_provenance() {
    let mut pres = patch(
        "<a:pPr hangingPunct=\"1\"/>",
        "<a:pPr hangingPunct=\"1\"><a:defRPr b=\"1\"/></a:pPr>",
    );
    fn source(pres: &mut Presentation) -> Source<'static> {
        let deck = pres.deck().unwrap();
        let slide = deck.slide(0).unwrap();
        let shape = slide
            .slide()
            .shapes()
            .into_iter()
            .find(|shape| shape.text_body().is_some())
            .unwrap();
        let body = shape.text_body().unwrap();
        let paragraph = body.paragraphs()[0];
        let run = paragraph.runs()[0];
        let lookup = slide
            .run_props(&shape, body, paragraph, RunItem::Run(run))
            .unwrap()
            .bold;
        assert_matches!(lookup, Lookup::Set(_));
        assert_eq!(lookup.value(), Some(&true));
        match lookup.source() {
            Some(Source::Direct) => Source::Direct,
            Some(Source::ParagraphDefault) => Source::ParagraphDefault,
            other => panic!("unexpected bold source {other:?}"),
        }
    }
    assert_eq!(source(&mut pres), Source::ParagraphDefault);
    edit(&mut pres, |body| {
        body.paragraph_mut(0)?.run_mut(0)?.bold(true)
    })
    .unwrap();
    assert_eq!(source(&mut pres), Source::Direct);
    let before = pres.to_bytes().unwrap();
    edit(&mut pres, |body| {
        body.paragraph_mut(0)?.format_range(1, 3, &bold())
    })
    .unwrap();
    assert_eq!(pres.to_bytes().unwrap(), before);
    edit(&mut pres, |body| {
        body.paragraph_mut(0)?.run_mut(0)?.clear_bold()
    })
    .unwrap();
    assert_eq!(source(&mut pres), Source::ParagraphDefault);
}

#[test]
fn slot_order_and_clear() {
    let mut pres = open();
    edit(&mut pres, |body| {
        body.autofit(AutoFitMode::Normal)?;
        let mut p = body.paragraph_mut(0)?;
        p.bullet(BulletSpec::Char {
            char: '•',
            font: None,
            size: None,
            color: Some(WriteColor::Rgb([1, 2, 3]).into()),
        })?;
        p.line_spacing(Spacing::Percent(DmlPercent::Thousandths(120_000)))?;
        p.align(TextAlignmentTypeValues::Center)?;
        let mut run = p.run_mut(0)?;
        run.font(FontRef::Typeface("Arial".into()), Script::Latin)?;
        run.underline(TextUnderlineValues::Single)?;
        run.underline_line(UnderlineLine::FollowText)?;
        run.underline_fill(UnderlineFill::FollowText)?;
        run.highlight(WriteColor::Rgb([255, 255, 0]).into())?;
        run.color(WriteColor::Rgb([1, 2, 3]).into())
    })
    .unwrap();
    let output = Presentation::from_bytes(pres.to_bytes().unwrap()).unwrap();
    let xml = String::from_utf8(output.package().read_part(PART).unwrap()).unwrap();
    assert!(xml.contains("<a:lnSpc><a:spcPct val=\"120000\"/></a:lnSpc><a:buClr><a:srgbClr val=\"010203\"/></a:buClr><a:buChar"));
    assert!(xml.contains("</a:solidFill><a:highlight><a:srgbClr val=\"FFFF00\"/></a:highlight><a:uLnTx/><a:uFillTx/><a:latin typeface=\"Arial\"/><a:sym"));
    edit(&mut pres, |body| {
        body.paragraph_mut(0)?.run_mut(0)?.clear_underline()
    })
    .unwrap();
    let xml = String::from_utf8(
        Presentation::from_bytes(pres.to_bytes().unwrap())
            .unwrap()
            .package()
            .read_part(PART)
            .unwrap(),
    )
    .unwrap();
    assert!(!xml.contains("<a:uLnTx"));
    assert!(!xml.contains("<a:uFillTx"));
}

#[test]
fn validates_before_mutating_either_boundary() {
    let mut pres = patch(
        "</a:t></a:r><a:endParaRPr lang=\"en-US\"/>",
        "</a:t></a:r><a:r><a:rPr smtId=\"42\"/><a:t>another</a:t></a:r><a:endParaRPr lang=\"en-US\"/>",
    );
    let length = "Lorem ipsum dolor sit amet".chars().count();
    assert_matches!(
        refused(&mut pres, |body| body.paragraph_mut(0)?.format_range(
            1,
            length + 2,
            &bold()
        )),
        Error::FormatNotCopyable(_)
    );
    let invalid = RunFormat {
        size: Some(FormatChange::Set(99)),
        ..bold()
    };
    assert_matches!(
        refused(&mut pres, |body| body.paragraph_mut(0)?.format_range(
            1,
            length + 2,
            &invalid
        )),
        Error::ValueOutOfRange { what: "sz" }
    );
    assert_matches!(
        refused(&mut pres, |body| body.paragraph_mut(0)?.format_range(
            1,
            100,
            &bold()
        )),
        Error::RangeOutOfBounds
    );
    assert_matches!(
        refused(&mut pres, |body| body.paragraph_mut(0)?.run_mut(0)?.color(
            ColorSpec::from(WriteColor::Scheme(SchemeColor::PhColor))
        )),
        Error::InvalidSchemeColor
    );
    assert_matches!(
        refused(&mut pres, |body| body
            .paragraph_mut(0)?
            .bullet(BulletSpec::Picture)),
        Error::UnsupportedBullet
    );
    let mut pres = patch(
        "</a:t></a:r><a:endParaRPr lang=\"en-US\"/>",
        "</a:t></a:r><a:fld id=\"{00000000-0000-0000-0000-000000000000}\"><a:t>123</a:t></a:fld><a:endParaRPr lang=\"en-US\"/>",
    );
    assert_matches!(
        refused(&mut pres, |body| body.paragraph_mut(0)?.format_range(
            1,
            length + 2,
            &bold()
        )),
        Error::RangeSplitsField
    );
}

#[test]
fn normal_autofit_noop_and_reset() {
    let mut pres = patch(
        "<a:bodyPr/>",
        "<a:bodyPr><a:normAutofit fontScale=\"87500\" lnSpcReduction=\"10000\"/></a:bodyPr>",
    );
    let original = pres.to_bytes().unwrap();
    edit(&mut pres, |body| body.autofit(AutoFitMode::Normal)).unwrap();
    assert_eq!(pres.to_bytes().unwrap(), original);
    edit(&mut pres, |body| body.reset_autofit_scale()).unwrap();
    let slide = pres.slide(0).unwrap();
    let body = slide
        .shapes()
        .into_iter()
        .find_map(|shape| shape.text_body())
        .unwrap();
    let props = body.raw().elements().next().unwrap();
    let normal = props
        .elements()
        .find(|node| node.name().local() == "normAutofit")
        .unwrap();
    assert_eq!(normal.attr(QName::new("", "fontScale")), None);
    assert_eq!(normal.attr(QName::new("", "lnSpcReduction")), None);
}

#[test]
fn picture_bullet_cleanup_is_atomic_and_keeps_shared_relationships() {
    const IMAGE: &str = "ppt/media/bullet.png";
    const IMAGE_REL: &str =
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";
    for shared in [false, true] {
        let mut pkg = Package::from_bytes(cases::read_fixture(FIXTURE)).unwrap();
        // The transaction only reads the relationship and leaves the media untouched.
        pkg.add_part(
            ooxml_core::PartNameRequest::Exact(IMAGE.into()),
            "image/png",
            b"media bytes".to_vec(),
        )
        .unwrap();
        let id = pkg
            .add_relationship(
                Some(PART),
                IMAGE_REL,
                ooxml_core::Target::Part(IMAGE.into()),
            )
            .unwrap();
        let xml = String::from_utf8(pkg.read_part(PART).unwrap()).unwrap();
        let picture = format!(r#"<a:buBlip><a:blip r:embed="{id}"/></a:buBlip>"#);
        let xml = xml.replacen(
            r#"<a:pPr hangingPunct="1"/>"#,
            &format!(r#"<a:pPr hangingPunct="1">{picture}</a:pPr>"#),
            1,
        );
        let xml = if shared {
            // Core registry safety counts every attribute, even outside a:blip.
            xml.replacen(
                r#"<a:endParaRPr lang="en-US"/>"#,
                &format!(r#"<a:endParaRPr lang="en-US" bmk="{id}"/>"#),
                1,
            )
        } else {
            xml
        };
        pkg.set_part(PART, xml.into_bytes()).unwrap();
        let mut pres = Presentation::from_bytes(pkg.to_bytes().unwrap()).unwrap();
        edit(&mut pres, |body| {
            let mut para = body.paragraph_mut(0)?;
            para.bullet(BulletSpec::None)?;
            // The same editing handle can still use the staged and committed slide.
            para.run_mut(0)?.italic(true)
        })
        .unwrap();
        let reopened = Presentation::from_bytes(pres.to_bytes().unwrap()).unwrap();
        let xml = String::from_utf8(reopened.package().read_part(PART).unwrap()).unwrap();
        assert!(xml.contains("<a:buNone/>"));
        assert!(!xml.contains(&picture));
        let rels = reopened.package().relationships(PART).unwrap();
        assert_eq!(rels.by_type(IMAGE_REL).any(|rel| rel.id == id), shared);
        assert_eq!(reopened.package().read_part(IMAGE).unwrap(), b"media bytes");
    }
    let mut pres = patch(
        r#"<a:pPr hangingPunct="1"/>"#,
        r#"<a:pPr hangingPunct="1"><a:buBlip><a:blip r:embed="rId1"/></a:buBlip></a:pPr>"#,
    );
    assert_matches!(
        refused(&mut pres, |body| body.paragraph_mut(0)?.clear_bullet()),
        Error::Core(ooxml_core::Error::RegistryRefused { .. })
    );
}

#[test]
fn splits_copy_hyperlink_and_allowlisted_extension_verbatim() {
    let original =
        r#"<a:rPr lang="en-US"><a:sym typeface="Wingdings" pitchFamily="2" charset="2"/></a:rPr>"#;
    let copy = r#"<a:rPr lang="en-US"><a:hlinkClick r:id="rId1"><a:extLst><a:ext uri="{A12FA001-AC4F-418D-AE19-62706E023703}"><ahyp:hlinkClr xmlns:ahyp="http://schemas.microsoft.com/office/drawing/2018/hyperlinkcolor" val="tx"/></a:ext></a:extLst></a:hlinkClick></a:rPr>"#;
    let mut pres = patch(original, copy);
    edit(&mut pres, |body| {
        body.paragraph_mut(0)?.format_range(1, 3, &bold())
    })
    .unwrap();
    let output = Presentation::from_bytes(pres.to_bytes().unwrap()).unwrap();
    let xml = String::from_utf8(output.package().read_part(PART).unwrap()).unwrap();
    assert_eq!(xml.matches(copy).count(), 2);
    assert_eq!(xml.matches(r#"<a:hlinkClick r:id="rId1">"#).count(), 3);
    assert_eq!(
        output
            .package()
            .read_part("ppt/slides/_rels/slide1.xml.rels")
            .unwrap(),
        Package::from_bytes(cases::read_fixture(FIXTURE))
            .unwrap()
            .read_part("ppt/slides/_rels/slide1.xml.rels")
            .unwrap()
    );
}

#[test]
fn range_splits_under_character_animation_lock() {
    const ID: u32 = 584_707;
    let mut pkg = Package::from_bytes(cases::read_fixture("poi/45545_Comment.pptx")).unwrap();
    let xml = String::from_utf8(pkg.read_part(PART).unwrap()).unwrap();
    let xml = xml.replace("<p:pRg ", "<p:charRg ");
    pkg.set_part(PART, xml.clone().into_bytes()).unwrap();
    let mut pres = Presentation::from_bytes(pkg.to_bytes().unwrap()).unwrap();
    let original_text = |pres: &mut Presentation| {
        pres.slide(0)
            .unwrap()
            .shape_by_id(ID)
            .unwrap()
            .unwrap()
            .text_body()
            .unwrap()
            .text()
    };
    let text = original_text(&mut pres);
    {
        let mut slide = pres.slide_mut(0).unwrap();
        let mut shape = slide.shape_mut_by_id(ID).unwrap();
        let mut body = shape.text_body_mut().unwrap();
        let mut paragraph = body.paragraph_mut(2).unwrap();
        assert_matches!(
            paragraph.add_run("x"),
            Err(Error::AnimatedCharacters { shape_id: ID })
        );
        paragraph.format_range(0, 1, &bold()).unwrap();
    }
    let mut reopened = Presentation::from_bytes(pres.to_bytes().unwrap()).unwrap();
    assert_eq!(original_text(&mut reopened), text);
    let result = String::from_utf8(reopened.package().read_part(PART).unwrap()).unwrap();
    let timing = |text: &str| text.find("<p:timing").unwrap();
    assert_eq!(result[timing(&result)..], xml[timing(&xml)..]);
    assert_ne!(result, xml);
}

#[test]
fn color_transform_order_and_same_value() {
    use ooxml_pptx::ColorTransform::{LuminanceModulation, LuminanceOffset};
    let color = ColorSpec {
        color: WriteColor::Scheme(SchemeColor::Accent1),
        transforms: vec![
            LuminanceOffset(10_000),
            LuminanceModulation(60_000),
            LuminanceOffset(20_000),
        ],
    };
    let mut pres = open();
    let set_color =
        |body: &mut TextBodyMut<'_>| body.paragraph_mut(0)?.run_mut(0)?.color(color.clone());
    edit(&mut pres, set_color).unwrap();
    let before = pres.to_bytes().unwrap();
    edit(&mut pres, set_color).unwrap();
    assert_eq!(pres.to_bytes().unwrap(), before);
    let pkg = Package::from_bytes(before).unwrap();
    let xml = String::from_utf8(pkg.read_part(PART).unwrap()).unwrap();
    assert!(xml.contains(concat!(
        "<a:schemeClr val=\"accent1\">",
        "<a:lumOff val=\"10000\"/><a:lumMod val=\"60000\"/><a:lumOff val=\"20000\"/>",
        "</a:schemeClr>",
    )));
}

#[test]
fn clear_paragraph_and_end_format_restore_inherited_sources() {
    let mut pres = patch(
        "<a:lstStyle/>",
        r#"<a:lstStyle><a:lvl1pPr algn="r"><a:defRPr i="1"/></a:lvl1pPr></a:lstStyle>"#,
    );
    let inherited = Source::ShapeListStyle { level: 0 };
    let check = |pres: &mut Presentation, expected: Source<'_>| {
        let deck = pres.deck().unwrap();
        let slide = deck.slide(0).unwrap();
        let shape = slide
            .slide()
            .shapes()
            .into_iter()
            .find(|shape| shape.text_body().is_some())
            .unwrap();
        let body = shape.text_body().unwrap();
        let paragraph = body.paragraphs()[0];
        let alignment = slide.para_props(&shape, body, paragraph).unwrap().align;
        let italic = slide
            .run_props(&shape, body, paragraph, RunItem::End)
            .unwrap()
            .italic;
        assert_eq!(alignment.source(), Some(&expected));
        assert_eq!(italic.source(), Some(&expected));
        let direct = expected == Source::Direct;
        assert_eq!(
            alignment.value(),
            Some(&ooxml_core::EnumValue::Known(if direct {
                TextAlignmentTypeValues::Center
            } else {
                TextAlignmentTypeValues::Right
            }))
        );
        assert_eq!(italic.value(), Some(&!direct));
    };
    check(&mut pres, inherited.clone());
    edit(&mut pres, |body| {
        let mut paragraph = body.paragraph_mut(0)?;
        paragraph.align(TextAlignmentTypeValues::Center)?;
        paragraph.end_format(&RunFormat {
            italic: Some(FormatChange::Set(false)),
            ..RunFormat::default()
        })
    })
    .unwrap();
    check(&mut pres, Source::Direct);
    edit(&mut pres, |body| {
        let mut paragraph = body.paragraph_mut(0)?;
        paragraph.clear_align()?;
        paragraph.end_format(&RunFormat {
            italic: Some(FormatChange::Clear),
            ..RunFormat::default()
        })
    })
    .unwrap();
    check(&mut pres, inherited);
}

#[test]
fn range_formats_slide_number_and_break_without_splitting_them() {
    let mut pres = patch(
        "</a:t></a:r><a:endParaRPr lang=\"en-US\"/>",
        concat!(
            "</a:t></a:r>",
            "<a:fld id='{00000000-0000-0000-0000-000000000001}' type='slidenum'>",
            "<a:rPr lang='en-US'/><a:t>1&#50;</a:t></a:fld>",
            "<a:br/><a:r><a:t>cd</a:t></a:r><a:endParaRPr lang=\"en-US\"/>"
        ),
    );
    let length = "Lorem ipsum dolor sit amet".len();
    for (start, end) in [(length + 1, length + 4), (1, length + 1)] {
        assert_matches!(
            refused(&mut pres, |body| body.paragraph_mut(0)?.format_range(
                start,
                end,
                &bold()
            )),
            Error::RangeSplitsField
        );
    }
    edit(&mut pres, |body| {
        body.paragraph_mut(0)?.format_range(1, length + 4, &bold())
    })
    .unwrap();
    let saved = pres.to_bytes().unwrap();
    let xml = String::from_utf8(
        Package::from_bytes(saved.clone())
            .unwrap()
            .read_part(PART)
            .unwrap(),
    )
    .unwrap();
    assert!(xml.contains("<a:fld id='{00000000-0000-0000-0000-000000000001}' type='slidenum'>"));
    assert!(xml.contains("<a:t>1&#50;</a:t></a:fld><a:br><a:rPr b=\"1\"/></a:br>"));
    edit(&mut pres, |body| {
        body.paragraph_mut(0)?.format_range(1, length + 4, &bold())
    })
    .unwrap();
    assert_eq!(saved, pres.to_bytes().unwrap());

    let deck = pres.deck().unwrap();
    let slide = deck.slide(0).unwrap();
    let shape = slide
        .slide()
        .shapes()
        .into_iter()
        .find(|shape| shape.text_body().is_some())
        .unwrap();
    let body = shape.text_body().unwrap();
    let paragraph = body.paragraphs()[0];
    assert_eq!(paragraph.text(), "Lorem ipsum dolor sit amet12\u{b}cd");
    let ooxml_pptx::TextItem::Field(field) = paragraph.items()[2] else {
        panic!("expected the slide number after the two split runs");
    };
    let field_bold = slide
        .run_props(&shape, body, paragraph, RunItem::Field(field))
        .unwrap()
        .bold;
    assert_eq!(field_bold.source(), Some(&Source::Direct));
    assert_eq!(field_bold.value(), Some(&true));
}

#[test]
fn formatting_refuses_alternate_content_in_properties() {
    fn alternate(choice: &str) -> String {
        format!(
            r#"<mc:AlternateContent xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><mc:Choice Requires="a">{choice}</mc:Choice></mc:AlternateContent>"#
        )
    }
    let mut run = patch(
        "<a:rPr lang=\"en-US\">",
        &format!(
            "<a:rPr lang=\"en-US\">{}",
            alternate("<a:solidFill><a:srgbClr val=\"000000\"/></a:solidFill>")
        ),
    );
    let color = ColorSpec::from(WriteColor::Rgb([0xff, 0, 0]));
    assert_matches!(
        refused(&mut run, |body| body
            .paragraph_mut(0)?
            .run_mut(0)?
            .color(color)),
        Error::InsideAlternateContent
    );
    assert_matches!(
        refused(&mut run, |body| body
            .paragraph_mut(0)?
            .run_mut(0)?
            .clear_color()),
        Error::InsideAlternateContent
    );
    assert_matches!(
        refused(&mut run, |body| body.paragraph_mut(0)?.format_range(
            1,
            2,
            &bold()
        )),
        Error::InsideAlternateContent
    );

    let mut paragraph = patch(
        "<a:pPr hangingPunct=\"1\"/>",
        &format!(
            "<a:pPr hangingPunct=\"1\">{}</a:pPr>",
            alternate("<a:buNone/>")
        ),
    );
    assert_matches!(
        refused(&mut paragraph, |body| body
            .paragraph_mut(0)?
            .bullet(BulletSpec::None)),
        Error::InsideAlternateContent
    );
    assert_matches!(
        refused(&mut paragraph, |body| body.paragraph_mut(0)?.clear_bullet()),
        Error::InsideAlternateContent
    );
    assert_matches!(
        refused(&mut paragraph, |body| body
            .paragraph_mut(0)?
            .space_before(Spacing::Points(100))),
        Error::InsideAlternateContent
    );

    let mut nested = patch(
        "<a:pPr hangingPunct=\"1\"/>",
        &format!(
            "<a:pPr><a:lnSpc>{}</a:lnSpc></a:pPr>",
            alternate("<a:spcPts val='100'/>")
        ),
    );
    assert_matches!(
        refused(&mut nested, |body| body
            .paragraph_mut(0)?
            .clear_line_spacing()),
        Error::InsideAlternateContent
    );

    let mut body = patch(
        "<a:bodyPr/>",
        &format!("<a:bodyPr>{}</a:bodyPr>", alternate("<a:normAutofit/>")),
    );
    assert_matches!(
        refused(&mut body, |body| body.autofit(AutoFitMode::Normal)),
        Error::InsideAlternateContent
    );
    assert_matches!(
        refused(&mut body, |body| body.clear_autofit()),
        Error::InsideAlternateContent
    );
    assert_matches!(
        refused(&mut body, |body| body.reset_autofit_scale()),
        Error::InsideAlternateContent
    );
}

#[test]
fn underline_replaces_unrepresented_line_properties() {
    for old in [
        "<a:uLn><a:round/></a:uLn>",
        "<a:uLn><a:bevel/></a:uLn>",
        "<a:uLn><a:miter lim='100000'/></a:uLn>",
        "<a:uLn><a:custDash><a:ds d='100000' sp='100000'/></a:custDash></a:uLn>",
        "<a:uLn><a:extLst/></a:uLn>",
        "<a:uLn algn='ctr'/>",
    ] {
        let mut pres = patch(
            "<a:rPr lang=\"en-US\">",
            &format!("<a:rPr lang=\"en-US\">{old}"),
        );
        edit(&mut pres, |body| {
            body.paragraph_mut(0)?
                .run_mut(0)?
                .underline_line(UnderlineLine::Line(Default::default()))
        })
        .unwrap();
        let saved = pres.to_bytes().unwrap();
        let xml = String::from_utf8(
            Package::from_bytes(saved.clone())
                .unwrap()
                .read_part(PART)
                .unwrap(),
        )
        .unwrap();
        assert!(xml.contains("<a:uLn/>"), "{old}");
        edit(&mut pres, |body| {
            body.paragraph_mut(0)?
                .run_mut(0)?
                .underline_line(UnderlineLine::Line(Default::default()))
        })
        .unwrap();
        assert_eq!(saved, pres.to_bytes().unwrap(), "{old}");
    }
}

#[test]
fn changing_typeface_drops_metadata_for_the_old_font() {
    let mut pres = patch(
        "<a:sym typeface=\"Wingdings\" pitchFamily=\"2\" charset=\"2\"/>",
        concat!(
            "<a:latin typeface='Arial' panose='020B0604020202020204' pitchFamily='34' charset='0'/>",
            "<a:sym typeface='Wingdings' panose='05000000000000000000' pitchFamily='2' charset='2'/>"
        ),
    );
    edit(&mut pres, |body| {
        let mut paragraph = body.paragraph_mut(0)?;
        let mut run = paragraph.run_mut(0)?;
        run.font(FontRef::Typeface("Aptos".into()), Script::Latin)?;
        run.font(FontRef::Typeface("Symbol".into()), Script::Symbol)
    })
    .unwrap();
    let xml = String::from_utf8(
        Package::from_bytes(pres.to_bytes().unwrap())
            .unwrap()
            .read_part(PART)
            .unwrap(),
    )
    .unwrap();
    assert!(xml.contains("<a:latin typeface=\"Aptos\"/><a:sym typeface=\"Symbol\"/>"));
}

#[test]
fn equal_formatting_keeps_the_existing_slide_tree() {
    let mut pres = open();
    let format = |body: &mut TextBodyMut<'_>| {
        body.autofit(AutoFitMode::Normal)?;
        let mut paragraph = body.paragraph_mut(0)?;
        paragraph.end_format(&bold())?;
        paragraph.run_mut(0)?.format(&bold())?;
        paragraph.format_range(1, 2, &bold())
    };
    edit(&mut pres, format).unwrap();
    let before = pres.to_bytes().unwrap();
    let shape_address = |pres: &mut Presentation| {
        std::ptr::from_ref(
            pres.slide(0)
                .unwrap()
                .shapes()
                .into_iter()
                .find(|shape| shape.text_body().is_some())
                .unwrap()
                .raw(),
        )
    };
    let shape = shape_address(&mut pres);
    edit(&mut pres, format).unwrap();
    assert_eq!(shape_address(&mut pres), shape);
    assert_eq!(pres.to_bytes().unwrap(), before);
}

fn project<T, U>(lookup: &Lookup<'_, T>, property: &str, project: impl FnOnce(&T) -> U) -> U {
    project(lookup.value().expect(property))
}

#[test]
fn every_run_property_has_direct_and_inherited_provenance() {
    use ooxml_core::EnumValue;
    use ooxml_pptx::{Fill, RunFill};
    use ooxml_schema::dml::{TextCapsValues, TextStrikeValues};

    let mut pres = patch(
        concat!(
            "<a:lstStyle/><a:p><a:pPr hangingPunct=\"1\"/><a:r>",
            "<a:rPr lang=\"en-US\"><a:sym typeface=\"Wingdings\" pitchFamily=\"2\" charset=\"2\"/></a:rPr>"
        ),
        concat!(
            "<a:lstStyle><a:lvl1pPr>",
            "<a:defRPr sz='2100' b='0' i='0' u='none' strike='noStrike' baseline='0' ",
            "cap='none' spc='0' kern='0' lang='en-GB' altLang='fr-FR'>",
            "<a:noFill/><a:highlight><a:srgbClr val='040506'/></a:highlight>",
            "<a:uLnTx/><a:uFillTx/><a:latin typeface='Latin default'/>",
            "<a:ea typeface='EA default'/><a:cs typeface='CS default'/><a:sym typeface='Symbol default'/>",
            "</a:defRPr></a:lvl1pPr></a:lstStyle><a:p><a:pPr hangingPunct='1'/><a:r>"
        ),
    );
    let color = WriteColor::Rgb([1, 2, 3]);
    macro_rules! run_cases {
        ($($format_field:ident => $lookup:ident : $input:expr, $project:expr, $direct:expr, $inherited:expr;)*) => {{
            let set = RunFormat {
                $($format_field: Some(FormatChange::Set($input)),)*
            };
            let clear = RunFormat {
                $($format_field: Some(FormatChange::Clear),)*
            };
            for (format, direct) in [(None, false), (Some(&set), true), (Some(&clear), false)] {
                if let Some(format) = format {
                    edit(&mut pres, |body| body.paragraph_mut(0)?.run_mut(0)?.format(format)).unwrap();
                }
                let deck = pres.deck().unwrap();
                let slide = deck.slide(0).unwrap();
                let shape = slide.slide().shapes().into_iter().find(|s| s.text_body().is_some()).unwrap();
                let body = shape.text_body().unwrap();
                let paragraph = body.paragraphs()[0];
                let run = paragraph.runs()[0];
                let props = slide.run_props(&shape, body, paragraph, RunItem::Run(run)).unwrap();
                let source = if direct { Source::Direct } else { Source::ShapeListStyle { level: 0 } };
                $(
                    assert_eq!(props.$lookup.source(), Some(&source), stringify!($lookup));
                    assert_eq!(
                        project(&props.$lookup, stringify!($lookup), $project),
                        if direct { $direct } else { $inherited },
                        stringify!($lookup)
                    );
                )*
            }
        }};
    }
    run_cases! {
        size => size: 2800, |v| *v, 2800, 2100;
        bold => bold: true, |v| *v, true, false;
        italic => italic: true, |v| *v, true, false;
        underline => underline: TextUnderlineValues::Single, |v| *v,
            EnumValue::Known(TextUnderlineValues::Single), EnumValue::Known(TextUnderlineValues::None);
        strike => strike: TextStrikeValues::SingleStrike, |v| *v,
            EnumValue::Known(TextStrikeValues::SingleStrike), EnumValue::Known(TextStrikeValues::NoStrike);
        baseline => baseline: DmlPercent::Thousandths(1000), |v: &DmlPercent| v.percent(), 1.0, 0.0;
        caps => caps: TextCapsValues::Small, |v| *v,
            EnumValue::Known(TextCapsValues::Small), EnumValue::Known(TextCapsValues::None);
        spacing => spacing: 200, |v| *v, 200, 0;
        kerning => kern: 1200, |v| *v, 1200, 0;
        lang => lang: "ja-JP".into(), |v| *v, "ja-JP", "en-GB";
        alt_lang => alt_lang: "de-DE".into(), |v| *v, "de-DE", "fr-FR";
        latin => latin: FontRef::Typeface("Latin direct".into()),
            |v: &ooxml_pptx::Font<'_>| v.typeface.to_owned(), "Latin direct", "Latin default";
        east_asian => east_asian: FontRef::Typeface("EA direct".into()),
            |v: &ooxml_pptx::Font<'_>| v.typeface.to_owned(), "EA direct", "EA default";
        complex_script => complex_script: FontRef::Typeface("CS direct".into()),
            |v: &ooxml_pptx::Font<'_>| v.typeface.to_owned(), "CS direct", "CS default";
        symbol => symbol: FontRef::Typeface("Symbol direct".into()),
            |v: &ooxml_pptx::Font<'_>| v.typeface.to_owned(), "Symbol direct", "Symbol default";
        fill => fill: RunFill::Solid(color.into()),
            |v| match v { Fill::Solid(c) => c.base_rgb, _ => None }, Some([1, 2, 3]), None;
        highlight => highlight: color.into(),
            |v: &ooxml_pptx::Color<'_>| v.base_rgb, Some([1, 2, 3]), Some([4, 5, 6]);
        underline_line => underline_line: UnderlineLine::Line(Default::default()),
            |v: &&ooxml_core::RawNode| v.name().local().to_owned(), "uLn", "uLnTx";
        underline_fill => underline_fill: UnderlineFill::Solid(color.into()),
            |v: &&ooxml_core::RawNode| v.name().local().to_owned(), "uFill", "uFillTx";
    }
}

#[test]
fn every_paragraph_property_has_direct_and_inherited_provenance() {
    use ooxml_core::EnumValue;
    use ooxml_pptx::{Bullet, BulletSize, TabStop};
    use ooxml_schema::dml::{TextFontAlignmentValues, TextTabAlignmentValues};

    let mut pres = patch(
        "<a:lstStyle/>",
        concat!(
            "<a:lstStyle><a:lvl1pPr algn='r' marL='20' indent='-20' rtl='0' fontAlgn='base'>",
            "<a:lnSpc><a:spcPts val='100'/></a:lnSpc>",
            "<a:spcBef><a:spcPts val='200'/></a:spcBef>",
            "<a:spcAft><a:spcPts val='300'/></a:spcAft>",
            "<a:buClrTx/><a:buSzTx/><a:buFontTx/><a:buNone/><a:tabLst/>",
            "</a:lvl1pPr></a:lstStyle>"
        ),
    );
    let raw_name = |v: &&ooxml_core::RawNode| v.name().local().to_owned();
    macro_rules! para_cases {
        ($($set:ident($input:expr) => $clear:ident;)* @ $($lookup:ident: $project:expr, $direct:expr, $inherited:expr;)*) => {{
            for phase in [None, Some(true), Some(false)] {
                if let Some(direct) = phase {
                    edit(&mut pres, |body| {
                        let mut paragraph = body.paragraph_mut(0)?;
                        if direct {
                            $(paragraph.$set($input)?;)*
                        } else {
                            $(paragraph.$clear()?;)*
                        }
                        Ok(())
                    }).unwrap();
                }
                let direct = phase == Some(true);
                let deck = pres.deck().unwrap();
                let slide = deck.slide(0).unwrap();
                let shape = slide.slide().shapes().into_iter().find(|s| s.text_body().is_some()).unwrap();
                let body = shape.text_body().unwrap();
                let props = slide.para_props(&shape, body, body.paragraphs()[0]).unwrap();
                let source = if direct { Source::Direct } else { Source::ShapeListStyle { level: 0 } };
                $(
                    assert_eq!(props.$lookup.source(), Some(&source), stringify!($lookup));
                    assert_eq!(
                        project(&props.$lookup, stringify!($lookup), $project),
                        if direct { $direct } else { $inherited },
                        stringify!($lookup)
                    );
                )*
            }
        }};
    }
    para_cases! {
        align(TextAlignmentTypeValues::Center) => clear_align;
        margin_left(40) => clear_margin_left;
        indent(-40) => clear_indent;
        rtl(true) => clear_rtl;
        font_align(TextFontAlignmentValues::Center) => clear_font_align;
        line_spacing(Spacing::Points(400)) => clear_line_spacing;
        space_before(Spacing::Points(500)) => clear_space_before;
        space_after(Spacing::Points(600)) => clear_space_after;
        bullet(BulletSpec::Char {
            char: '•',
            font: Some(FontRef::Typeface("Aptos".into())),
            size: Some(BulletSize::Points(200)),
            color: Some(WriteColor::Rgb([1, 2, 3]).into()),
        }) => clear_bullet;
        tab_stops(&[TabStop { position: 1000, alignment: TextTabAlignmentValues::Left }]) => clear_tab_stops;
        @
        align: |v| *v, EnumValue::Known(TextAlignmentTypeValues::Center), EnumValue::Known(TextAlignmentTypeValues::Right);
        margin_left: |v| *v, 40, 20;
        indent: |v| *v, -40, -20;
        rtl: |v| *v, true, false;
        font_align: |v| *v, EnumValue::Known(TextFontAlignmentValues::Center), EnumValue::Known(TextFontAlignmentValues::Baseline);
        line_spacing: |v| *v, Spacing::Points(400), Spacing::Points(100);
        space_before: |v| *v, Spacing::Points(500), Spacing::Points(200);
        space_after: |v| *v, Spacing::Points(600), Spacing::Points(300);
        bullet: |v| match v { Bullet::Char(c) => Some(c.to_string()), _ => None }, Some("•".into()), None;
        bullet_color: raw_name, "buClr", "buClrTx";
        bullet_size: raw_name, "buSzPts", "buSzTx";
        bullet_font: raw_name, "buFont", "buFontTx";
        tab_stops: |v: &&ooxml_core::RawNode| v.elements().count(), 1, 0;
    }

    // The resolver exposes the effective list level directly, without a Source.
    for (direct, expected) in [(true, 2), (false, 0)] {
        edit(&mut pres, |body| {
            let mut paragraph = body.paragraph_mut(0)?;
            if direct {
                paragraph.level(2)
            } else {
                paragraph.clear_level()
            }
        })
        .unwrap();
        let deck = pres.deck().unwrap();
        let slide = deck.slide(0).unwrap();
        let shape = slide
            .slide()
            .shapes()
            .into_iter()
            .find(|s| s.text_body().is_some())
            .unwrap();
        let body = shape.text_body().unwrap();
        assert_eq!(
            slide
                .para_props(&shape, body, body.paragraphs()[0])
                .unwrap()
                .level,
            Ok(expected)
        );
    }
}

#[test]
fn clear_autofit_restores_original_provenance() {
    use ooxml_pptx::Autofit;

    let mut pres = open();
    let inspect = |pres: &mut Presentation, direct| {
        let deck = pres.deck().unwrap();
        let slide = deck.slide(0).unwrap();
        let shape = slide
            .slide()
            .shapes()
            .into_iter()
            .find(|s| s.text_body().is_some())
            .unwrap();
        let autofit = slide.body_props(&shape).unwrap().autofit;
        if direct {
            assert_eq!(autofit.source(), Some(&Source::Direct));
            assert_eq!(autofit.value(), Some(&Autofit::Shape));
        }
        format!("{autofit:?}")
    };
    let original = inspect(&mut pres, false);
    edit(&mut pres, |body| body.autofit(AutoFitMode::Shape)).unwrap();
    inspect(&mut pres, true);
    edit(&mut pres, |body| body.clear_autofit()).unwrap();
    assert_eq!(inspect(&mut pres, false), original);
}
