use ooxml_pptx::{
    ColorSpec, LineEnd, LineSpec, Presentation, Rect, RunFill, ShapeFill, WriteColor,
};
use ooxml_schema::dml::{
    CompoundLineValues, LineCapValues, LineEndLengthValues, LineEndValues, LineEndWidthValues,
    PresetLineDashValues, ShapeTypeValues,
};

pub const FIXTURE: &str = "poi/shapes.pptx";
pub const NAME: &str = "p3b-shape-properties";

pub fn edit(presentation: &mut Presentation) -> ooxml_pptx::Result<()> {
    let mut slide = presentation.slide_mut(2)?;
    let mut arrow = slide.shape_mut_by_id(2)?;
    arrow.set_slide_rect(Rect {
        x: 800_000,
        y: 1_100_000,
        cx: 2_000_000,
        cy: 850_000,
    })?;
    arrow.set_rotation(1_800_000)?;
    arrow.set_flip_h(true)?;
    arrow.set_preset(ShapeTypeValues::UpArrow)?;
    arrow.set_adjustment("adj1", 35_000)?;
    arrow.set_fill(ShapeFill::Solid(WriteColor::Rgb([18, 52, 86]).into()))?;
    arrow.set_line(Some(&LineSpec {
        width: Some(38_100),
        fill: Some(RunFill::Solid(WriteColor::Rgb([192, 80, 40]).into())),
        dash: Some(PresetLineDashValues::Dash),
        cap: Some(LineCapValues::Round),
        compound: Some(CompoundLineValues::Double),
        head: Some(LineEnd {
            kind: Some(LineEndValues::Triangle),
            width: Some(LineEndWidthValues::Large),
            length: Some(LineEndLengthValues::Small),
        }),
        tail: Some(LineEnd {
            kind: Some(LineEndValues::Oval),
            ..LineEnd::default()
        }),
    }))?;

    slide.shape_mut_by_id(3)?.set_fill(ShapeFill::Linear {
        angle: 5_400_000,
        stops: vec![
            (0, ColorSpec::from(WriteColor::Rgb([255, 255, 255]))),
            (100_000, WriteColor::Rgb([0, 128, 128]).into()),
        ],
    })?;

    slide.shape_mut_by_id(5)?.group_mut()?.fit_to_children()
}
