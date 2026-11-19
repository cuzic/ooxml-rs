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
