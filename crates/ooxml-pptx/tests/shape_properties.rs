//! Direct shape properties, affine group fits, and corpus round trips.

#[path = "oracle/cases.rs"]
mod cases;

use std::assert_matches;

use ooxml_core::Package;
use ooxml_pptx::{
    ColorSpec, Error, LineEnd, LineSpec, Presentation, Rect, RunFill, ShapeFill, ShapeKind,
    ShapeMut, Source, WriteColor,
};
use ooxml_schema::dml::{LineEndLengthValues, PresetLineDashValues, ShapeTypeValues};

const SLIDE: &str = "ppt/slides/slide1.xml";
const BOX: Rect = Rect {
    x: 900_000,
    y: 1_100_000,
    cx: 2_400_000,
    cy: 650_000,
};
const INNER: &str = r#"<a:chOff x="0" y="0"/><a:chExt cx="100" cy="100"/>"#;
const OUTER: &str = r#"<a:off x="1000" y="2000"/><a:ext cx="200" cy="400"/>"#;

fn fixture(name: &str) -> Presentation {
    Presentation::from_bytes(cases::read_fixture(name)).unwrap()
}

fn synthetic(children: &str) -> Presentation {
    let mut pkg = Package::from_bytes(cases::read_fixture("poi/sample.pptx")).unwrap();
    let xml = String::from_utf8(pkg.read_part(SLIDE).unwrap()).unwrap();
    assert!(xml.contains("</p:spTree>"));
    pkg.set_part(
        SLIDE,
        xml.replace("</p:spTree>", &format!("{children}</p:spTree>"))
            .into_bytes(),
    )
    .unwrap();
    Presentation::from_bytes(pkg.to_bytes().unwrap()).unwrap()
}

fn group(id: u32, xfrm: &str, members: &str) -> String {
    format!(
        r#"<p:grpSp><p:nvGrpSpPr><p:cNvPr id="{id}" name="G{id}"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr>{xfrm}</p:grpSpPr>{members}</p:grpSp>"#
    )
}

fn transform(attrs: &str, outer: &str, inner: &str) -> String {
    format!("<a:xfrm{attrs}>{outer}{inner}</a:xfrm>")
}

fn member(id: u32, x: i64, y: i64, cx: i64, cy: i64) -> String {
    format!(
        r#"<p:sp><p:nvSpPr><p:cNvPr id="{id}" name="M{id}"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="{x}" y="{y}"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm><a:prstGeom prst="rect"/></p:spPr></p:sp>"#
    )
}

fn edit(
    p: &mut Presentation,
    id: u32,
    f: impl FnOnce(&mut ShapeMut<'_>) -> Result<(), Error>,
) -> Result<(), Error> {
    f(&mut p.slide_mut(0)?.shape_mut_by_id(id)?)
}

fn refused(
    p: &mut Presentation,
    id: u32,
    f: impl FnOnce(&mut ShapeMut<'_>) -> Result<(), Error>,
) -> Error {
    let before = p.to_bytes().unwrap();
    let error = edit(p, id, f).unwrap_err();
    assert_eq!(p.to_bytes().unwrap(), before, "{error}");
    error
}

fn slide_boxes(
    p: &mut Presentation,
    index: usize,
    prefix: &[usize],
) -> Vec<(String, Option<Rect>)> {
    let deck = p.deck().unwrap();
    let slide = deck.slide(index).unwrap();
    slide
        .slide()
        .shapes()
        .iter()
        .filter_map(|shape| {
            let path = shape.path();
            (path.as_slice().len() > prefix.len() && path.as_slice().starts_with(prefix)).then(
                || {
                    (
                        format!("{:?}", shape.path()),
                        slide.slide_rect(shape).unwrap_or_else(|error| {
                            assert_matches!(error, Error::TransformedGroup { .. });
                            None
                        }),
                    )
                },
            )
        })
        .collect()
}

fn shape_box(p: &mut Presentation, id: u32) -> Option<Rect> {
    let deck = p.deck().unwrap();
    let slide = deck.slide(0).unwrap();
    let shape = slide
        .slide()
        .shapes()
        .into_iter()
        .find(|shape| shape.id().ok() == Some(id))
        .unwrap();
    slide.slide_rect(&shape).unwrap()
}

fn fit(shape: &mut ShapeMut<'_>) -> Result<(), Error> {
    shape.group_mut()?.fit_to_children()
}

#[test]
fn inverse_mapping_and_exact_fit_preserve_all_nested_members() {
    let nested = group(81, &transform("", OUTER, INNER), &member(82, 9, 19, 31, 41));
    let body = format!("{nested}{}", member(83, 50, 70, 60, 80));
    let mut p = synthetic(&group(80, &transform("", OUTER, INNER), &body));
    let desired = Rect {
        x: 3_040,
        y: 10_160,
        cx: 120,
        cy: 320,
    };
    edit(&mut p, 82, |shape| shape.set_slide_rect(desired)).unwrap();
    assert_eq!(shape_box(&mut p, 82), Some(desired));
    let before = p.to_bytes().unwrap();
    let group_path = group_path(&mut p, 80);
    let descendants = slide_boxes(&mut p, 0, &group_path);
    assert_eq!(descendants.len(), 3);
    edit(&mut p, 80, fit).unwrap();
    let output = p.to_bytes().unwrap();
    assert_ne!(output, before);
    let after = slide_boxes(&mut p, 0, &group_path);
    assert_eq!(descendants, after);
    assert_eq!(
        shape_box(&mut p, 80),
        Some(Rect {
            x: 1_100,
            y: 2_280,
            cx: 2_300,
            cy: 9_320
        })
    );
    edit(&mut p, 80, fit).unwrap();
    assert_eq!(p.to_bytes().unwrap(), output);
}

fn group_path(p: &mut Presentation, id: u32) -> Vec<usize> {
    p.slide(0)
        .unwrap()
        .shapes()
        .iter()
        .find(|shape| shape.id().ok() == Some(id))
        .unwrap()
        .path()
        .as_slice()
        .to_vec()
}

#[test]
fn fit_identity_and_unrepresentable_cases() {
    let member = member(81, 1, 2, 3, 4);
    let mut p = synthetic(&group(80, "", &member));
    let old = shape_box(&mut p, 81);
    edit(&mut p, 80, fit).unwrap();
    assert_eq!(shape_box(&mut p, 80), old);
    assert_eq!(shape_box(&mut p, 81), old);

    let half = r#"<a:off x="0" y="0"/><a:ext cx="2" cy="2"/>"#;
    let inner = r#"<a:chOff x="0" y="0"/><a:chExt cx="4" cy="4"/>"#;
    let mut p = synthetic(&group(80, &transform("", half, inner), &member));
    assert_matches!(refused(&mut p, 80, fit), Error::ValueOutOfRange { .. });

    let zero = r#"<a:chOff x="0" y="0"/><a:chExt cx="0" cy="100"/>"#;
    let mut p = synthetic(&group(80, &transform("", OUTER, zero), &member));
    assert_matches!(refused(&mut p, 80, fit), Error::ValueOutOfRange { .. });

    let zero_member = member.replace("cx=\"3\"", "cx=\"0\"");
    let mut p = synthetic(&group(80, &transform("", OUTER, INNER), &zero_member));
    assert_matches!(refused(&mut p, 80, fit), Error::ValueOutOfRange { .. });

    let extremes = r#"<a:off x="0" y="0"/><a:ext cx="9223372036854775807" cy="100"/>"#;
    let one = r#"<a:chOff x="0" y="0"/><a:chExt cx="1" cy="100"/>"#;
    let mut p = synthetic(&group(80, &transform("", extremes, one), &member));
    assert_matches!(refused(&mut p, 80, fit), Error::ValueOutOfRange { .. });

    for attrs in [r#" rot="1""#, r#" flipH="true""#, r#" flipV="1""#] {
        let mut p = synthetic(&group(80, &transform(attrs, OUTER, INNER), &member));
        assert_matches!(
            refused(&mut p, 80, fit),
            Error::TransformedGroup { group_id: Some(80) }
        );
        assert_matches!(
            refused(&mut p, 81, |s| s.set_slide_rect(BOX)),
            Error::TransformedGroup { group_id: Some(80) }
        );
        let no_op = transform(
            attrs,
            r#"<a:off x="1" y="2"/><a:ext cx="3" cy="4"/>"#,
            r#"<a:chOff x="1" y="2"/><a:chExt cx="3" cy="4"/>"#,
        );
        let mut p = synthetic(&group(80, &no_op, &member));
        let before = p.to_bytes().unwrap();
        edit(&mut p, 80, fit).unwrap();
        assert_eq!(p.to_bytes().unwrap(), before);
    }
}

#[test]
fn wrapped_transform_refusals_preserve_bytes() {
    let wrapped = format!(
        r#"<mc:AlternateContent xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><mc:Fallback>{}</mc:Fallback></mc:AlternateContent>"#,
        transform("", OUTER, INNER)
    );
    let mut p = synthetic(&group(80, &wrapped, &member(81, 1, 2, 3, 4)));
    assert_matches!(refused(&mut p, 80, fit), Error::InsideAlternateContent);
    assert_matches!(
        refused(&mut p, 80, |s| s.set_rotation(60_000)),
        Error::InsideAlternateContent
    );
    assert_matches!(
        refused(&mut p, 81, |s| s.set_slide_rect(BOX)),
        Error::InsideAlternateContent
    );
}

#[test]
fn geometry_and_paint_are_local_and_idempotent() {
    let element = member(81, 10, 20, 30, 40);
    let mut p = synthetic(&element);
    let original = String::from_utf8(p.package().read_part(SLIDE).unwrap()).unwrap();
    let (prefix, suffix) = original.split_once(&element).unwrap();
    let fill = ShapeFill::Linear {
        angle: 5_400_000,
        stops: vec![
            (0, ColorSpec::from(WriteColor::Rgb([32, 44, 56]))),
            (100_000, WriteColor::Rgb([68, 80, 92]).into()),
        ],
    };
    let line = LineSpec {
        width: Some(25_400),
        fill: Some(ooxml_pptx::RunFill::NoFill),
        ..LineSpec::default()
    };
    let format = |shape: &mut ShapeMut<'_>| {
        shape.set_rect(BOX)?;
        shape.set_rotation(1_800_000)?;
        shape.set_flip_h(true)?;
        shape.set_flip_v(false)?;
        shape.set_fill(fill.clone())?;
        shape.set_line(Some(&line))?;
        shape.set_preset(ShapeTypeValues::UpArrow)?;
        shape.set_adjustment("adj1", 35_000)
    };
    edit(&mut p, 81, format).unwrap();
    let output = String::from_utf8(p.package().read_part(SLIDE).unwrap()).unwrap();
    assert!(output.starts_with(prefix));
    assert!(output.ends_with(suffix));
    // Issue #113: the whole list, in the preset's order (PowerPoint refuses
    // an upArrow whose avLst holds adj1 alone).
    assert!(output.contains(r#"<a:avLst><a:gd name="adj1" fmla="val 35000"/><a:gd name="adj2" fmla="val 50000"/></a:avLst>"#), "{output}");
    assert!(output.contains(r#"<a:lin ang="5400000""#));
    assert!(output.contains(r#"flipH="1""#));
    assert!(!output.contains("flipV="));
    assert_eq!(shape_box(&mut p, 81), Some(BOX));

    // Test the no-op against reloaded bytes as well as the dirty tree.
    let bytes = p.to_bytes().unwrap();
    edit(&mut p, 81, format).unwrap();
    assert_eq!(bytes, p.to_bytes().unwrap());
    let mut p = Presentation::from_bytes(bytes.clone()).unwrap();
    edit(&mut p, 81, format).unwrap();
    assert_eq!(bytes, p.to_bytes().unwrap());
    assert_matches!(
        refused(&mut p, 81, |s| s.set_adjustment("nonexistent", 0)),
        Error::UnknownAdjustment { .. }
    );
    edit(&mut p, 81, |s| {
        s.set_preset(ShapeTypeValues::Ellipse)?;
        s.set_fill(ShapeFill::Inherit)?;
        s.set_line(None)
    })
    .unwrap();
    let output = String::from_utf8(p.package().read_part(SLIDE).unwrap()).unwrap();
    let changed = output
        .strip_prefix(prefix)
        .unwrap()
        .strip_suffix(suffix)
        .unwrap();
    assert!(changed.contains(r#"prst="ellipse""#));
    for removed in ["avLst", "gradFill", "<a:ln"] {
        assert!(!changed.contains(removed), "{changed}");
    }
}

#[test]
fn unsupported_geometry_fill_frames_and_invalid_values_are_atomic() {
    let mut p = fixture("poi/shapes.pptx");
    assert_matches!(
        refused(&mut p, 7, |s| s.set_preset(ShapeTypeValues::Ellipse)),
        Error::UnsupportedGeometry
    );
    assert_matches!(
        refused(&mut p, 7, |s| s.set_adjustment("adj", 0)),
        Error::UnsupportedGeometry
    );
    assert_matches!(
        refused(&mut p, 3, |s| s.set_fill(ShapeFill::NoFill)),
        Error::UnsupportedContent(_)
    );
    assert_matches!(
        refused(&mut p, 3, |s| s.set_line(None)),
        Error::UnsupportedContent(_)
    );
    assert_matches!(
        refused(&mut p, 3, |s| s.set_preset(ShapeTypeValues::Ellipse)),
        Error::UnsupportedContent(_)
    );
    assert_matches!(
        refused(&mut p, 3, |s| s.set_flip_v(true)),
        Error::UnsupportedContent(_)
    );
    for fill in [ShapeFill::Picture, ShapeFill::Pattern] {
        assert_matches!(
            refused(&mut p, 4, |s| s.set_fill(fill)),
            Error::UnsupportedFill
        );
    }
    assert_matches!(
        refused(&mut p, 4, |s| s.set_fill(ShapeFill::Linear {
            angle: 0,
            stops: vec![]
        })),
        Error::ValueOutOfRange { .. }
    );
    assert_matches!(
        refused(&mut p, 4, |s| s.set_rect(Rect { cx: -1, ..BOX })),
        Error::ValueOutOfRange { .. }
    );
    assert_matches!(
        refused(&mut p, 4, |s| s.set_line(Some(&LineSpec {
            width: Some(-1),
            ..LineSpec::default()
        }))),
        Error::ValueOutOfRange { .. }
    );

    edit(&mut p, 3, |s| {
        s.set_rect(BOX)?;
        s.set_rotation(42_000)
    })
    .unwrap();
    assert_eq!(shape_box(&mut p, 3), Some(BOX));
}

#[test]
fn placeholder_rotation_materializes_inherited_box_and_clear_restores_it() {
    let mut p = fixture("python-pptx/ph-inherit-props.pptx");
    let before = shape_box(&mut p, 4).expect("inherited box");
    let source = |p: &mut Presentation| {
        let deck = p.deck().unwrap();
        let slide = deck.slide(0).unwrap();
        let shape = slide
            .slide()
            .shapes()
            .into_iter()
            .find(|s| s.id().ok() == Some(4))
            .unwrap();
        matches!(
            slide.geometry(&shape).unwrap(),
            ooxml_pptx::Lookup::Set(ooxml_pptx::Resolved {
                source: Source::Direct,
                ..
            })
        )
    };
    assert!(!source(&mut p));
    edit(&mut p, 4, |s| s.set_rotation(600_000)).unwrap();
    assert!(source(&mut p));
    assert_eq!(shape_box(&mut p, 4), Some(before));
    edit(&mut p, 4, |shape| shape.clear_rect()).unwrap();
    assert!(!source(&mut p));
    assert_eq!(shape_box(&mut p, 4), Some(before));
}

#[test]
fn corpus_shape_edit_sweep() {
    let mut slides = 0;
    let mut applied = [0; 4];
    let mut refused = [0; 4];
    let mut checked_descendants = 0;
    for fixture_name in cases::FIXTURES {
        let mut deck = fixture(fixture_name);
        for index in 0..deck.slide_count() {
            slides += 1;
            let shapes = deck.slide(index).unwrap().shapes();
            let first = shapes.first().map(|shape| shape.path());
            let groups: Vec<_> = shapes
                .iter()
                .filter(|shape| *shape.kind() == ShapeKind::Group)
                .map(|shape| shape.path())
                .collect();
            if let Some(first) = first {
                let mut slide = deck.slide_mut(index).unwrap();
                if let Ok(mut shape) = slide.shape_mut(&first) {
                    let results = [
                        shape.set_rect(BOX),
                        shape.set_rotation(600_000),
                        shape.set_fill(ShapeFill::Solid(WriteColor::Rgb([18, 52, 86]).into())),
                    ];
                    for (i, result) in results.into_iter().enumerate() {
                        if result.is_ok() {
                            applied[i] += 1
                        } else {
                            refused[i] += 1
                        }
                    }
                }
            }
            for group in groups {
                let before = slide_boxes(&mut deck, index, group.as_slice());
                let result = deck
                    .slide_mut(index)
                    .unwrap()
                    .shape_mut(&group)
                    .and_then(|mut shape| fit(&mut shape));
                if result.is_ok() {
                    applied[3] += 1;
                    checked_descendants += before.len();
                    assert_eq!(
                        slide_boxes(&mut deck, index, group.as_slice()),
                        before,
                        "{fixture_name}, slide {index}"
                    );
                } else {
                    refused[3] += 1;
                }
            }
        }
        let mut reopened = Presentation::from_bytes(deck.to_bytes().unwrap()).unwrap();
        assert_eq!(reopened.slide_count(), deck.slide_count());
        for index in 0..reopened.slide_count() {
            reopened.slide(index).unwrap();
        }
    }
    println!(
        "{} fixtures, {slides} slides, applied {applied:?}, refused {refused:?}, {checked_descendants} descendant checks",
        cases::FIXTURES.len()
    );
    assert!(applied.into_iter().all(|count| count > 0));
}

#[test]
fn outline_patch_preserves_unedited_xml_and_inserts_in_order() {
    let children = concat!(
        "<a:solidFill><a:srgbClr val='FF0000'/></a:solidFill>",
        "<a:round /><a:headEnd type='triangle' w='lg'/>",
        "<a:extLst><a:ext uri='custom' /></a:extLst>",
    );
    let line = format!(r#"<a:ln w="12700" algn = 'i&#110;'>{children}</a:ln>"#);
    let shape = member(80, 1, 2, 3, 4).replace("</p:spPr>", &format!("{line}</p:spPr>"));
    let mut p = synthetic(&shape);
    let patch = LineSpec {
        width: Some(25400),
        ..LineSpec::default()
    };
    edit(&mut p, 80, |s| s.set_line(Some(&patch))).unwrap();
    let bytes = p.to_bytes().unwrap();
    let xml = String::from_utf8(
        Package::from_bytes(bytes.as_slice())
            .unwrap()
            .read_part(SLIDE)
            .unwrap(),
    )
    .unwrap();
    assert!(xml.contains(&line.replace("12700", "25400")), "{xml}");
    edit(&mut p, 80, |s| s.set_line(Some(&patch))).unwrap();
    assert_eq!(p.to_bytes().unwrap(), bytes);

    let patch = LineSpec {
        fill: Some(RunFill::NoFill),
        dash: Some(PresetLineDashValues::Dash),
        ..LineSpec::default()
    };
    edit(&mut p, 80, |s| s.set_line(Some(&patch))).unwrap();
    let xml = String::from_utf8(
        Package::from_bytes(p.to_bytes().unwrap())
            .unwrap()
            .read_part(SLIDE)
            .unwrap(),
    )
    .unwrap();
    assert!(
        xml.contains(concat!(
            r#"<a:ln w="25400" algn = 'i&#110;'><a:noFill/><a:prstDash val="dash"/>"#,
            "<a:round /><a:headEnd type='triangle' w='lg'/>",
            "<a:extLst><a:ext uri='custom' /></a:extLst></a:ln>",
        )),
        "{xml}"
    );

    let end_patch = LineSpec {
        head: Some(LineEnd {
            length: Some(LineEndLengthValues::Medium),
            ..LineEnd::default()
        }),
        ..LineSpec::default()
    };
    edit(&mut p, 80, |s| s.set_line(Some(&end_patch))).unwrap();
    let xml = String::from_utf8(p.package().read_part(SLIDE).unwrap()).unwrap();
    assert!(
        xml.contains(r#"<a:headEnd type='triangle' w='lg' len="med"/>"#),
        "{xml}"
    );

    let mut p = synthetic(&shape.replace("<a:round />", "<a:unknown/>"));
    assert_matches!(
        refused(&mut p, 80, |s| s.set_line(Some(&patch))),
        Error::UnsupportedContent(_)
    );
    let mut p = synthetic(&shape.replace("</a:ln>", "</a:ln><a:ln/>"));
    assert_matches!(
        refused(&mut p, 80, |s| s.set_line(Some(&patch))),
        Error::UnsupportedContent(_)
    );
}

#[test]
fn group_transform_edits_preserve_the_child_coordinate_space() {
    let shape = member(81, 100, 200, 300, 400);
    let mut p = synthetic(&group(80, "", &shape));
    let before = p.to_bytes().unwrap();
    let member_box = shape_box(&mut p, 81);
    edit(&mut p, 80, |s| s.set_rotation(0)).unwrap();
    assert_eq!(shape_box(&mut p, 81), member_box);
    assert_eq!(p.to_bytes().unwrap(), before);
    edit(&mut p, 80, |s| s.set_rotation(1)).unwrap();
    edit(&mut p, 80, |s| s.set_rotation(0)).unwrap();
    assert_eq!(shape_box(&mut p, 80), member_box);
    assert_eq!(shape_box(&mut p, 81), member_box);

    let destination = Rect {
        x: 5000,
        y: 5000,
        cx: 600,
        cy: 800,
    };
    let mut p = synthetic(&group(80, "", &shape));
    edit(&mut p, 80, |s| s.set_rect(destination)).unwrap();
    assert_eq!(shape_box(&mut p, 81), Some(destination));
    assert_matches!(
        refused(&mut p, 80, |s| s.clear_rect()),
        Error::UnsupportedContent(_)
    );
    assert_matches!(
        refused(&mut p, 81, |s| s.clear_rect()),
        Error::UnsupportedContent(_)
    );

    let legacy = transform("", OUTER, "");
    let mut p = synthetic(&group(80, &legacy, &member(81, 1000, 2000, 200, 400)));
    edit(&mut p, 80, |s| s.set_rect(destination)).unwrap();
    assert_eq!(shape_box(&mut p, 81), Some(destination));

    let mut p = synthetic(&group(80, "", ""));
    assert_matches!(
        refused(&mut p, 80, |s| s.set_rect(destination)),
        Error::ValueOutOfRange { .. }
    );
}

#[test]
fn effective_default_rotation_and_flips_are_byte_noops() {
    let mut p = synthetic(&member(80, 1, 2, 3, 4));
    let before = p.to_bytes().unwrap();
    edit(&mut p, 80, |s| {
        s.set_rotation(0)?;
        s.set_flip_h(false)?;
        s.set_flip_v(false)
    })
    .unwrap();
    assert_eq!(p.to_bytes().unwrap(), before);
}

#[test]
fn materialized_placeholder_keeps_other_inherited_transform_fields() {
    let mut pkg =
        Package::from_bytes(cases::read_fixture("python-pptx/ph-inherit-props.pptx")).unwrap();
    let layout = "ppt/slideLayouts/slideLayout1.xml";
    let xml = String::from_utf8(pkg.read_part(layout).unwrap()).unwrap();
    let layout_xfrm = transform(r#" rot="5400000" flipV="1""#, OUTER, "");
    let title = format!("<p:spPr>{layout_xfrm}</p:spPr>");
    assert_eq!(xml.matches("<p:spPr/>").count(), 1);
    pkg.set_part(layout, xml.replace("<p:spPr/>", &title).into_bytes())
        .unwrap();
    let mut p = Presentation::from_bytes(pkg.to_bytes().unwrap()).unwrap();
    let before = p.to_bytes().unwrap();
    edit(&mut p, 4, |s| s.set_rotation(5_400_000)).unwrap();
    assert_eq!(p.to_bytes().unwrap(), before);

    for (rect, rotation, flip_h) in [
        (false, None, Some(true)),
        (true, None, None),
        (false, Some(42), None),
    ] {
        let mut p = Presentation::from_bytes(before.clone()).unwrap();
        let old_box = shape_box(&mut p, 4);
        edit(&mut p, 4, |s| {
            if let Some(rotation) = rotation {
                s.set_rotation(rotation)
            } else if let Some(flip_h) = flip_h {
                s.set_flip_h(flip_h)
            } else {
                s.set_rect(BOX)
            }
        })
        .unwrap();
        assert_eq!(shape_box(&mut p, 4), if rect { Some(BOX) } else { old_box });
        let deck = p.deck().unwrap();
        let slide = deck.slide(0).unwrap();
        let shape = slide
            .slide()
            .shapes()
            .into_iter()
            .find(|s| s.id().ok() == Some(4))
            .unwrap();
        let ooxml_pptx::Lookup::Set(ooxml_pptx::Resolved {
            value,
            source: Source::Direct,
            ..
        }) = slide.geometry(&shape).unwrap()
        else {
            panic!("expected a direct transform")
        };
        let value = value.unwrap();
        assert_eq!(value.rot.get(), Some(rotation.unwrap_or(5_400_000)));
        assert_eq!(value.flip_h.get(), Some(flip_h.unwrap_or(false)));
        assert_eq!(value.flip_v.get(), Some(true));
    }

    let malformed = title.replace(r#"flipV="1""#, r#"flipV="invalid""#);
    pkg.set_part(layout, xml.replace("<p:spPr/>", &malformed).into_bytes())
        .unwrap();
    let mut p = Presentation::from_bytes(pkg.to_bytes().unwrap()).unwrap();
    assert_matches!(
        refused(&mut p, 4, |s| s.set_rect(BOX)),
        Error::ValueOutOfRange { .. }
    );
}

fn title_transform(p: &mut Presentation) -> (i32, bool, bool, bool) {
    let deck = p.deck().unwrap();
    let slide = deck.slide(0).unwrap();
    let shape = slide
        .slide()
        .shapes()
        .into_iter()
        .find(|s| s.id().ok() == Some(4))
        .unwrap();
    let ooxml_pptx::Lookup::Set(value) = slide.geometry(&shape).unwrap() else {
        panic!("expected a resolved title transform");
    };
    let direct = value.source == Source::Direct;
    let value = value.value.unwrap();
    (
        value.rot.get().unwrap(),
        value.flip_h.get().unwrap(),
        value.flip_v.get().unwrap(),
        direct,
    )
}

fn title_fixture(layout_box: &str, partial: Option<&str>) -> Presentation {
    let mut pkg =
        Package::from_bytes(cases::read_fixture("python-pptx/ph-inherit-props.pptx")).unwrap();
    let layout = "ppt/slideLayouts/slideLayout1.xml";
    let xml = String::from_utf8(pkg.read_part(layout).unwrap()).unwrap();
    assert_eq!(xml.matches("<p:spPr/>").count(), 1);
    let xfrm = transform(r#" rot="5400000" flipV="1""#, layout_box, "");
    pkg.set_part(
        layout,
        xml.replace("<p:spPr/>", &format!("<p:spPr>{xfrm}</p:spPr>"))
            .into_bytes(),
    )
    .unwrap();
    if let Some(partial) = partial {
        let xml = String::from_utf8(pkg.read_part(SLIDE).unwrap()).unwrap();
        let title = xml.find(r#"id="4" name="Title 3""#).unwrap();
        let props = title + xml[title..].find("<p:spPr/>").unwrap();
        let mut xml = xml;
        xml.replace_range(
            props..props + "<p:spPr/>".len(),
            &format!("<p:spPr>{partial}</p:spPr>"),
        );
        pkg.set_part(SLIDE, xml.into_bytes()).unwrap();
    }
    Presentation::from_bytes(pkg.to_bytes().unwrap()).unwrap()
}

#[test]
fn clearing_placeholder_transform_restores_inheritance_before_next_edit() {
    let mut p = title_fixture(OUTER, None);
    edit(&mut p, 4, |s| s.set_rect(BOX)).unwrap();
    assert_eq!(title_transform(&mut p), (5_400_000, false, true, true));
    edit(&mut p, 4, |s| s.clear_rect()).unwrap();
    assert_eq!(title_transform(&mut p), (5_400_000, false, true, false));
    edit(&mut p, 4, |s| s.set_rotation(0)).unwrap();
    edit(&mut p, 4, |s| s.set_flip_h(true)).unwrap();
    assert_eq!(title_transform(&mut p), (0, true, true, true));
}

#[test]
fn partial_placeholder_transform_preserves_existing_attribute_spelling() {
    let partial = r#"<a:xfrm  flipV = '&#48;'/>"#;
    for rect in [false, true] {
        let mut p = title_fixture(OUTER, Some(partial));
        let inherited = shape_box(&mut p, 4);
        assert_eq!(title_transform(&mut p), (5_400_000, false, true, false));
        if rect {
            edit(&mut p, 4, |s| s.set_rect(BOX)).unwrap();
        } else {
            edit(&mut p, 4, |s| s.set_flip_h(true)).unwrap();
        }
        assert_eq!(
            shape_box(&mut p, 4),
            if rect { Some(BOX) } else { inherited }
        );
        assert_eq!(title_transform(&mut p), (5_400_000, !rect, false, true));
        let pkg = Package::from_bytes(p.to_bytes().unwrap()).unwrap();
        let xml = String::from_utf8(pkg.read_part(SLIDE).unwrap()).unwrap();
        assert!(xml.contains("  flipV = '&#48;'"));
    }
    let mut p = title_fixture(OUTER, Some(partial));
    edit(&mut p, 4, |s| s.clear_rect()).unwrap();
    let pkg = Package::from_bytes(p.to_bytes().unwrap()).unwrap();
    let xml = String::from_utf8(pkg.read_part(SLIDE).unwrap()).unwrap();
    assert!(!xml.contains(partial));
}

#[test]
fn placeholder_set_rect_replaces_invalid_inherited_box() {
    for cx in ["-5", "invalid"] {
        let layout_box = OUTER.replace(r#"cx="200""#, &format!(r#"cx="{cx}""#));
        let mut p = title_fixture(&layout_box, None);
        assert_matches!(
            refused(&mut p, 4, |s| s.set_rotation(0)),
            Error::ValueOutOfRange { .. }
        );
        edit(&mut p, 4, |s| s.set_rect(BOX)).unwrap();
        assert_eq!(shape_box(&mut p, 4), Some(BOX));
        assert_eq!(title_transform(&mut p), (5_400_000, false, true, true));
    }
}

#[test]
fn rotated_group_reports_transform_before_fractional_fit() {
    let outer = r#"<a:off x="0" y="0"/><a:ext cx="2" cy="2"/>"#;
    let inner = r#"<a:chOff x="0" y="0"/><a:chExt cx="4" cy="4"/>"#;
    let xfrm = transform(r#" rot="1""#, outer, inner);
    let mut p = synthetic(&group(80, &xfrm, &member(81, 1, 2, 3, 4)));
    assert_matches!(
        refused(&mut p, 80, fit),
        Error::TransformedGroup { group_id: Some(80) }
    );
}

fn av_list(p: &Presentation) -> String {
    let xml = String::from_utf8(p.package().read_part(SLIDE).unwrap()).unwrap();
    let start = xml.find("<a:avLst").expect("avLst");
    let end = xml[start..].find("</a:avLst>").map_or_else(|| xml[start..].find("/>").unwrap() + 2, |e| e + "</a:avLst>".len());
    xml[start..start + end].to_owned()
}

#[test]
fn set_adjustment_writes_every_guide_of_the_preset() {
    // Issue #113: a partial avLst makes PowerPoint 365 refuse the file.
    for (preset, name, value, expected) in [
        (ShapeTypeValues::RightArrow, "adj2", 30_000, r#"<a:gd name="adj1" fmla="val 50000"/><a:gd name="adj2" fmla="val 30000"/>"#),
        (ShapeTypeValues::Hexagon, "adj", 10_000, r#"<a:gd name="adj" fmla="val 10000"/><a:gd name="vf" fmla="val 115470"/>"#),
        (
            ShapeTypeValues::AccentBorderCallout3,
            "adj5",
            1,
            r#"<a:gd name="adj1" fmla="val 18750"/><a:gd name="adj2" fmla="val -8333"/><a:gd name="adj3" fmla="val 18750"/><a:gd name="adj4" fmla="val -16667"/><a:gd name="adj5" fmla="val 1"/><a:gd name="adj6" fmla="val -16667"/><a:gd name="adj7" fmla="val 112963"/><a:gd name="adj8" fmla="val -8333"/>"#,
        ),
    ] {
        let mut p = synthetic(&member(81, 10, 20, 30, 40));
        edit(&mut p, 81, |s| {
            s.set_preset(preset)?;
            s.set_adjustment(name, value)
        })
        .unwrap();
        assert_eq!(av_list(&p), format!("<a:avLst>{expected}</a:avLst>"), "{preset:?}");
    }
}

#[test]
fn set_adjustment_completes_a_partial_list_in_order() {
    // A list written elsewhere with an unknown guide and adj2 alone: adj1 goes
    // after the preset guides before it (none, so first), the unknown guide
    // keeps its place relative to adj2.
    let mut p = synthetic(&member(81, 10, 20, 30, 40));
    edit(&mut p, 81, |s| {
        s.set_preset(ShapeTypeValues::UpArrow)?;
        s.set_adjustment("adj2", 40_000)
    })
    .unwrap();
    let bytes = p.to_bytes().unwrap();
    let xml = String::from_utf8(p.package().read_part(SLIDE).unwrap()).unwrap();
    let partial = xml.replace(r#"<a:gd name="adj1" fmla="val 50000"/>"#, "").replace(
        r#"<a:gd name="adj2" fmla="val 40000"/>"#,
        r#"<a:gd name="x" fmla="val 1"/><a:gd name="adj2" fmla="val 40000"/>"#,
    );
    let mut pkg = Package::from_bytes(bytes).unwrap();
    pkg.set_part(SLIDE, partial.into_bytes()).unwrap();
    let mut p = Presentation::from_bytes(pkg.to_bytes().unwrap()).unwrap();
    edit(&mut p, 81, |s| s.set_adjustment("adj2", 45_000)).unwrap();
    assert_eq!(
        av_list(&p),
        r#"<a:avLst><a:gd name="adj1" fmla="val 50000"/><a:gd name="x" fmla="val 1"/><a:gd name="adj2" fmla="val 45000"/></a:avLst>"#
    );
}

